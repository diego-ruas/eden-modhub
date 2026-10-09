use crate::catalog::{self, ModKind, UA};
use crate::emu::Kind;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use tauri::{AppHandle, Emitter, State};
use walkdir::WalkDir;

const TMP_PREFIX: &str = ".eden-mod-manager-tmp-";

pub struct Root {
    key: String,
    name: String,
    files: Vec<(PathBuf, String)>,
}

pub struct PendingInstall {
    tid: String,
    mod_id: String,
    version: Option<String>,
    tmp: PathBuf,
    roots: Vec<Root>,
}

#[derive(Default)]
pub struct Pending(Mutex<HashMap<String, PendingInstall>>);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RootInfo {
    key: String,
    name: String,
    file_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prepared {
    token: String,
    roots: Vec<RootInfo>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    /// emulador onde foi instalado (manifestos antigos, sem o campo, são do Eden)
    #[serde(default)]
    emu: Kind,
    tid: String,
    folder: String,
    mod_id: String,
    root_key: String,
    name: String,
    version: Option<String>,
    installed_at: u64,
}

/// `Installed` + estado na config do emulador (não persistido no manifesto).
#[derive(Serialize)]
pub struct InstalledView {
    #[serde(flatten)]
    item: Installed,
    enabled: bool,
}

#[derive(Serialize, Clone)]
struct Progress {
    received: u64,
    total: Option<u64>,
}

// ---------- helpers ----------

fn encode_segment(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn encode_path(p: &str) -> String {
    p.split('/').map(encode_segment).collect::<Vec<_>>().join("/")
}

fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .collect();
    let s: String = s.trim_end_matches(|c| c == '.' || c == ' ').chars().take(80).collect();
    let s = s.trim_end_matches(|c| c == '.' || c == ' ').to_string();
    if s.is_empty() { "mod".into() } else { s }
}

fn manifest_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = crate::app_dir(app, crate::Dir::Data)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("installed.json"))
}

fn load_manifest(app: &AppHandle) -> Result<Vec<Installed>, String> {
    let p = manifest_path(app)?;
    Ok(std::fs::read(&p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default())
}

fn save_manifest(app: &AppHandle, m: &[Installed]) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(m).map_err(|e| e.to_string())?;
    std::fs::write(manifest_path(app)?, json).map_err(|e| format!("Falha ao salvar manifesto: {e}"))
}

pub fn cleanup_tmp(load: &Path) {
    if let Ok(rd) = std::fs::read_dir(load) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(TMP_PREFIX) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

/// GET com `Range` a partir de `from` (0 = download inteiro).
async fn get(url: &str, from: u64) -> Result<reqwest::Response, String> {
    let mut req = catalog::HTTP.get(url).header("User-Agent", UA);
    if from > 0 {
        req = req.header("Range", format!("bytes={from}-"));
    }
    let resp = req.send().await.map_err(|e| format!("Falha ao baixar {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Falha ao baixar {url}: HTTP {}", resp.status()));
    }
    Ok(resp)
}

/// Se a conexão cair no meio, retoma de onde parou (até 3 vezes); servidor que ignora `Range` reinicia do zero.
pub(crate) async fn download(url: &str, dest: &Path, app: &AppHandle) -> Result<(), String> {
    if let Some(d) = dest.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let mut resp = get(url, 0).await?;
    let total = resp.content_length();
    let mut f = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let (mut received, mut last, mut retries) = (0u64, 0u64, 0);
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                f.write_all(&chunk).map_err(|e| e.to_string())?;
                received += chunk.len() as u64;
                if received - last >= 256 * 1024 {
                    last = received;
                    let _ = app.emit("download-progress", Progress { received, total });
                }
            }
            Ok(None) => break,
            Err(e) => {
                retries += 1;
                if retries > 3 {
                    return Err(format!("Falha ao baixar {url}: {e}"));
                }
                resp = get(url, received).await?;
                if received > 0 && resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                    f.set_len(0).and_then(|_| f.seek(SeekFrom::Start(0))).map_err(|e| e.to_string())?;
                    (received, last) = (0, 0);
                }
            }
        }
    }
    let _ = app.emit("download-progress", Progress { received, total });
    Ok(())
}

fn unsafe_path(p: &Path) -> bool {
    p.is_absolute() || p.components().any(|c| matches!(c, Component::ParentDir | Component::Prefix(_) | Component::RootDir))
}

pub fn extract(archive: &Path, out: &Path) -> Result<(), String> {
    let ext = archive.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "zip" => {
            let f = std::fs::File::open(archive).map_err(|e| e.to_string())?;
            zip::ZipArchive::new(f)
                .and_then(|mut z| z.extract(out))
                .map_err(|e| format!("Falha ao extrair zip: {e}"))
        }
        "7z" => sevenz_rust2::decompress_file(archive, out).map_err(|e| format!("Falha ao extrair 7z: {e}")),
        "rar" => {
            let mut ar = unrar::Archive::new(archive)
                .open_for_processing()
                .map_err(|e| format!("Falha ao abrir rar: {e}"))?;
            while let Some(h) = ar.read_header().map_err(|e| format!("Falha ao ler rar: {e}"))? {
                ar = if unsafe_path(&h.entry().filename) {
                    h.skip()
                } else {
                    h.extract_with_base(out)
                }
                .map_err(|e| format!("Falha ao extrair rar: {e}"))?;
            }
            Ok(())
        }
        _ => Err("Formato não suportado".into()),
    }
}

fn stem(p: &str) -> String {
    let f = p.rsplit('/').next().unwrap_or(p);
    f.rsplit_once('.').map(|(s, _)| s).unwrap_or(f).to_string()
}

fn roots_from_extracted(dir: &Path, fallback: &str) -> Vec<Root> {
    let mut groups: BTreeMap<String, Vec<(PathBuf, String)>> = BTreeMap::new();
    for e in WalkDir::new(dir).into_iter().flatten().filter(|e| e.file_type().is_file()) {
        let rel = e.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
        // dest vem do zip/7z/rar de terceiros: `..` ou caminho absoluto sairiam da pasta do mod no commit
        if let Some((root, dest)) = catalog::mod_root(&rel).filter(|(_, d)| !unsafe_path(Path::new(d))) {
            groups.entry(root).or_default().push((e.path().to_path_buf(), dest));
        }
    }
    groups
        .into_iter()
        .map(|(key, files)| Root { name: catalog::display_name(&key, fallback), key, files })
        .collect()
}

async fn do_prepare(app: &AppHandle, m: &catalog::ModEntry, tmp: &Path) -> Result<Vec<Root>, String> {
    let url = |src: &str| m.source.raw_url(src);
    match m.kind {
        ModKind::Files => {
            let mut files = Vec::new();
            for f in &m.files {
                let dest = tmp.join(&f.dest);
                download(&url(&f.src), &dest, app).await?;
                files.push((dest, f.dest.clone()));
            }
            Ok(vec![Root { key: String::new(), name: m.name.clone(), files }])
        }
        ModKind::Archive => {
            let src = &m.files[0].src;
            let ext = src.rsplit('.').next().unwrap_or("").to_lowercase();
            let arc = tmp.join(format!("_archive.{ext}"));
            download(&url(src), &arc, app).await?;
            let x = tmp.join("x");
            let xx = x.clone();
            tauri::async_runtime::spawn_blocking(move || extract(&arc, &xx))
                .await
                .map_err(|e| e.to_string())??;
            Ok(roots_from_extracted(&x, &stem(src)))
        }
        ModKind::Pack => {
            let (url, tid, size) = (m.source.raw_url(crate::pack::ZIP), m.files[0].src.clone(), m.size);
            let x = tmp.join("x");
            let (xx, app2) = (x.clone(), app.clone());
            tauri::async_runtime::spawn_blocking(move || {
                crate::pack::extract(&url, &tid, size, &xx, |received, total| {
                    let _ = app2.emit("download-progress", Progress { received, total: Some(total) });
                })
            })
                .await
                .map_err(|e| e.to_string())??;
            Ok(roots_from_extracted(&x, &m.name))
        }
    }
}

// ---------- commands ----------

#[tauri::command]
pub async fn prepare_install(
    app: AppHandle,
    cat: State<'_, crate::CatalogState>,
    pending: State<'_, Pending>,
    tid: String,
    mod_id: String,
) -> Result<Prepared, String> {
    crate::emu::check_tid(&tid)?;
    let m = cat
        .0
        .lock()
        .as_ref()
        .and_then(|c| c.mods.iter().find(|m| m.id == mod_id).cloned())
        .ok_or("Mod não encontrado no catálogo")?;
    if m.tid.as_deref().is_some_and(|t| !t.eq_ignore_ascii_case(&tid)) {
        return Err("Este mod é de outro jogo".into());
    }
    let load = crate::resolve_emu(&app)?.mods_dir();
    let token = format!(
        "{:x}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    );
    let tmp = load.join(format!("{TMP_PREFIX}{token}"));
    std::fs::create_dir_all(&tmp).map_err(|e| format!("Falha ao criar pasta temporária: {e}"))?;
    let roots = match do_prepare(&app, &m, &tmp).await {
        Ok(r) if !r.is_empty() => r,
        Ok(_) => {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err("Layout do mod não reconhecido. Use 'Ver no GitHub'.".into());
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(e);
        }
    };
    let infos = roots
        .iter()
        .map(|r| RootInfo { key: r.key.clone(), name: r.name.clone(), file_count: r.files.len() })
        .collect();
    pending.0.lock().insert(token.clone(), PendingInstall { tid, mod_id, version: m.version.clone(), tmp, roots });
    Ok(Prepared { token, roots: infos })
}

fn commit(app: &AppHandle, p: PendingInstall, keys: &[String]) -> Result<Vec<Installed>, String> {
    let emu = crate::resolve_emu(app)?;
    let game_dir = emu.tid_dir(&p.tid);
    let mut manifest = load_manifest(app)?;
    let mut done = Vec::new();
    for root in p.roots.iter().filter(|r| keys.contains(&r.key)) {
        let base = sanitize(&root.name);
        let reinstall = manifest
            .iter()
            .position(|i| i.emu == emu.kind && i.tid == p.tid && i.mod_id == p.mod_id && i.root_key == root.key);
        if let Some(pos) = reinstall {
            let _ = std::fs::remove_dir_all(game_dir.join(&manifest[pos].folder));
            manifest.remove(pos);
        }
        let folder = unique_folder(&game_dir, &base);
        let target = game_dir.join(&folder);
        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        for (src, dest) in &root.files {
            let to = target.join(dest);
            if let Some(d) = to.parent() {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            std::fs::rename(src, &to).map_err(|e| format!("Falha ao mover arquivo: {e}"))?;
        }
        done.push(Installed {
            emu: emu.kind,
            tid: p.tid.clone(),
            folder,
            mod_id: p.mod_id.clone(),
            root_key: root.key.clone(),
            name: root.name.clone(),
            version: p.version.clone(),
            installed_at: catalog::now_secs(),
        });
    }
    manifest.extend(done.iter().cloned());
    save_manifest(app, &manifest)?;
    Ok(done)
}

/// `base`, ou `base (2)`, `base (3)`… o primeiro que ainda não existe em `dir`.
fn unique_folder(dir: &Path, base: &str) -> String {
    let mut folder = base.to_string();
    let mut n = 2;
    while dir.join(&folder).exists() {
        folder = format!("{base} ({n})");
        n += 1;
    }
    folder
}

#[tauri::command]
pub fn commit_install(
    app: AppHandle,
    pending: State<'_, Pending>,
    token: String,
    keys: Vec<String>,
) -> Result<Vec<Installed>, String> {
    let p = pending.0.lock().remove(&token).ok_or("Instalação pendente não encontrada")?;
    let tmp = p.tmp.clone();
    let r = commit(&app, p, &keys);
    let _ = std::fs::remove_dir_all(tmp);
    r
}

#[tauri::command]
pub fn cancel_install(pending: State<'_, Pending>, token: String) {
    if let Some(p) = pending.0.lock().remove(&token) {
        let _ = std::fs::remove_dir_all(p.tmp);
    }
}

#[tauri::command]
pub fn list_installed(app: AppHandle, tid: String) -> Result<Vec<InstalledView>, String> {
    crate::emu::check_tid(&tid)?;
    let emu = crate::resolve_emu(&app)?;
    let all = load_manifest(&app)?;
    let kept: Vec<Installed> = all
        .iter()
        .filter(|i| i.emu != emu.kind || i.tid != tid || emu.tid_dir(&i.tid).join(&i.folder).is_dir())
        .cloned()
        .collect();
    if kept.len() != all.len() {
        save_manifest(&app, &kept)?;
    }
    let off = crate::addons::disabled(&emu, &tid);
    Ok(kept
        .into_iter()
        .filter(|i| i.emu == emu.kind && i.tid == tid)
        .map(|item| InstalledView { enabled: !off.contains(&item.folder), item })
        .collect())
}

#[tauri::command]
pub fn set_mod_enabled(app: AppHandle, tid: String, folder: String, enabled: bool) -> Result<(), String> {
    crate::emu::check_tid(&tid)?;
    let emu = crate::resolve_emu(&app)?;
    if !load_manifest(&app)?.iter().any(|i| i.emu == emu.kind && i.tid == tid && i.folder == folder) {
        return Err("Pasta não foi instalada pelo Eden Mod Manager".into());
    }
    crate::addons::set_enabled(&emu, &tid, &folder, enabled)
}

#[tauri::command]
pub async fn list_conflicts(app: AppHandle, tid: String) -> Result<Vec<crate::addons::Conflict>, String> {
    crate::emu::check_tid(&tid)?;
    let emu = crate::resolve_emu(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::addons::conflicts(&emu.tid_dir(&tid), &crate::addons::disabled(&emu, &tid))
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn uninstall(app: AppHandle, tid: String, folder: String) -> Result<(), String> {
    crate::emu::check_tid(&tid)?;
    let mut m = load_manifest(&app)?;
    let emu = crate::resolve_emu(&app)?;
    let pos = m
        .iter()
        .position(|i| i.emu == emu.kind && i.tid == tid && i.folder == folder)
        .ok_or("Pasta não foi instalada pelo Eden Mod Manager")?;
    match std::fs::remove_dir_all(emu.tid_dir(&tid).join(&folder)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Falha ao remover: {e}")),
    }
    if crate::addons::disabled(&emu, &tid).contains(&folder) {
        let _ = crate::addons::set_enabled(&emu, &tid, &folder, true);
    }
    m.remove(pos);
    save_manifest(&app, &m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_path_rejects_traversal_and_absolute() {
        for p in ["../x", "a/../../x", "/x"] {
            assert!(unsafe_path(Path::new(p)), "{p}");
        }
        assert!(!unsafe_path(Path::new("romfs/a/b.bin")));
    }

    #[test]
    fn unique_folder_skips_taken_names() {
        let dir = std::env::temp_dir().join(format!("emm-unique-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(unique_folder(&dir, "m"), "m");
        std::fs::create_dir_all(dir.join("m")).unwrap();
        assert_eq!(unique_folder(&dir, "m"), "m (2)");
        std::fs::create_dir_all(dir.join("m (2)")).unwrap();
        assert_eq!(unique_folder(&dir, "m"), "m (3)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanitize_cases() {
        assert_eq!(sanitize("a:b/c. "), "a_b_c");
        assert_eq!(sanitize("..."), "mod");
    }

    #[test]
    fn encode_cases() {
        assert_eq!(encode_path("A B/[x]+é.txt"), "A%20B/%5Bx%5D%2B%C3%A9.txt");
    }

    #[test]
    #[ignore]
    fn rar_network() {
        let url = format!(
            "https://raw.githubusercontent.com/{}/{}/Mods/0100801011C3E000/1.0.0/60fps.rar",
            catalog::Source::Official.repo(),
            catalog::Source::Official.branch()
        );
        let dir = std::env::temp_dir().join("eden-mod-manager-rar-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bytes = tauri::async_runtime::block_on(async {
            reqwest::Client::new()
                .get(&url)
                .header("User-Agent", UA)
                .send()
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap()
        });
        let rar = dir.join("60fps.rar");
        std::fs::write(&rar, &bytes).unwrap();
        let out = dir.join("x");
        extract(&rar, &out).unwrap();
        let roots = roots_from_extracted(&out, "60fps");
        assert!(roots
            .iter()
            .any(|r| r.files.iter().any(|(_, d)| d.starts_with("exefs/") || d.starts_with("romfs/"))));
    }

    /// Extrai a tradução de um jogo pequeno do pacote PT-BR, de ponta a ponta até `roots_from_extracted`.
    #[test]
    #[ignore]
    fn pack_network() {
        let dir = std::env::temp_dir().join("eden-mod-manager-pack-test");
        let _ = std::fs::remove_dir_all(&dir);
        let mods = tauri::async_runtime::block_on(crate::pack::list()).unwrap();
        let m = mods.iter().find(|m| m.tid.as_deref() == Some("01006000040C2000")).unwrap();
        let url = m.source.raw_url(crate::pack::ZIP);
        let mut last = 0;
        crate::pack::extract(&url, &m.files[0].src, m.size, &dir, |r, t| {
            assert_eq!(t, m.size);
            last = r;
        })
        .unwrap();
        assert_eq!(last, m.size);
        let roots = roots_from_extracted(&dir, &m.name);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].name, m.name);
        assert!(!roots[0].files.is_empty());
        assert!(roots[0].files.iter().all(|(p, d)| d.starts_with("romfs/") && p.is_file()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[derive(Default)]
pub struct PeekCache(Mutex<HashMap<String, Vec<RootInfo>>>);

/// Lista os mods dentro de um pacote sem instalar (baixa em temp, extrai, apaga).
#[tauri::command]
pub async fn peek_archive(
    app: AppHandle,
    cat: State<'_, crate::CatalogState>,
    cache: State<'_, PeekCache>,
    mod_id: String,
) -> Result<Vec<RootInfo>, String> {
    if let Some(r) = cache.0.lock().get(&mod_id) {
        return Ok(r.clone());
    }
    let m = cat
        .0
        .lock()
        .as_ref()
        .and_then(|c| c.mods.iter().find(|m| m.id == mod_id).cloned())
        .ok_or("Mod não encontrado no catálogo")?;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let tmp = std::env::temp_dir().join(format!("eden-mod-manager-peek-{nanos:x}"));
    let _ = std::fs::remove_dir_all(&tmp);
    let r = do_prepare(&app, &m, &tmp).await;
    let _ = std::fs::remove_dir_all(&tmp);
    let infos: Vec<RootInfo> = r?
        .iter()
        .map(|r| RootInfo { key: r.key.clone(), name: r.name.clone(), file_count: r.files.len() })
        .collect();
    cache.0.lock().insert(mod_id, infos.clone());
    Ok(infos)
}
