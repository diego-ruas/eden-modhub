use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::LazyLock;

pub const UA: &str = "Eden-Mod-Manager";

/// Fontes de mods, em ordem de prioridade (o primeiro vence duplicatas).
/// Wiki = espelho do wiki oficial do yuzu (sucessor do LexouilleTM/yuzu-mods-archive, removido).
/// Ptbr = traduções PT-BR num .zip de release (não é árvore de repositório; ver `pack.rs`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    #[default]
    Official,
    Theboy181,
    Wiki,
    Ptbr,
}

impl Source {
    /// Fontes que são árvores de repositório (as únicas que passam por `fetch_tree`/`build_catalog`).
    pub const ALL: [Source; 3] = [Source::Official, Source::Theboy181, Source::Wiki];

    pub fn repo(self) -> &'static str {
        match self {
            Source::Official => "ADEMOLA200/Switch-Emulator-Mod-Database",
            Source::Theboy181 => "theboy181/switch-ptchtxt-mods",
            Source::Wiki => "amakvana/Switch-Mods-Wiki-Archive",
            Source::Ptbr => "staticpiratex/Traducoes-SWITCH-PTBR",
        }
    }

    pub fn branch(self) -> &'static str {
        match self {
            Source::Official => "develop",
            Source::Ptbr => "NintendoSwitch",
            _ => "main",
        }
    }

    pub fn id_prefix(self) -> &'static str {
        match self {
            Source::Official => "",
            Source::Theboy181 => "theboy181:",
            Source::Wiki => "wiki:",
            Source::Ptbr => "ptbr:",
        }
    }

    /// URL de um arquivo na fonte: raw do branch, ou asset da release (Ptbr).
    pub fn raw_url(self, path: &str) -> String {
        if self == Source::Ptbr {
            return format!("https://github.com/{}/releases/download/{}/{}", self.repo(), self.branch(), crate::install::encode_path(path));
        }
        format!("https://raw.githubusercontent.com/{}/{}/{}", self.repo(), self.branch(), crate::install::encode_path(path))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModFile {
    pub src: String,
    pub dest: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ModKind {
    Archive,
    Files,
    /// entradas de um .zip remoto lido por Range (`files[0].src` = TID); ver `pack.rs`
    Pack,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModEntry {
    /// caminho no repositório; fontes além da oficial ganham o prefixo `id_prefix()` para ids únicos
    pub id: String,
    pub tid: Option<String>,
    pub name: String,
    pub version: Option<String>,
    pub kind: ModKind,
    pub files: Vec<ModFile>,
    pub size: u64,
    pub group: String,
    #[serde(default)]
    pub source: Source,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub tree_sha: String,
    pub fetched_at: u64,
    pub mods: Vec<ModEntry>,
    pub names: HashMap<String, String>,
    /// versão do formato: caches antigos (sem as demais fontes) são refeitos
    #[serde(default)]
    pub schema: u32,
}

pub const SCHEMA: u32 = 3;

#[derive(Deserialize)]
pub struct TreeItem {
    pub path: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub size: Option<u64>,
}

#[derive(Deserialize)]
struct TreeResp {
    sha: String,
    tree: Vec<TreeItem>,
    truncated: bool,
}

static TID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:^|[^0-9a-f])(01[0-9a-f]{14})(?:[^0-9a-f]|$)").unwrap());
// ponytail: exige delimitadores para não casar pedaços de BuildID de 32 hex
static TID3DS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:^|[^0-9a-f])0004[0-9a-f]{12}(?:[^0-9a-f]|$)").unwrap());
static VER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^v?\d+(\.\d+)+$").unwrap());
static TID_SEG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^01[0-9a-f]{14}$").unwrap());
static NORM_SUFFIX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*-\s*\d+$").unwrap());

const MOD_DIRS: [&str; 5] = ["exefs", "romfs", "romfslite", "romfs_ext", "cheats"];

/// Última ocorrência de TID Switch (maiúsculo).
pub fn last_tid(s: &str) -> Option<String> {
    let mut pos = 0;
    let mut last = None;
    while pos <= s.len() {
        let Some(c) = TID_RE.captures_at(s, pos) else { break };
        let m = c.get(1).unwrap();
        last = Some(m.as_str().to_uppercase());
        pos = m.start() + 1;
        while !s.is_char_boundary(pos) {
            pos += 1;
        }
    }
    last
}

pub fn norm(s: &str) -> String {
    let s = NORM_SUFFIX_RE.replace(s, "");
    s.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

fn ext_of(path: &str) -> String {
    Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn stem_of(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    match file.rfind('.') {
        Some(i) if i > 0 => file[..i].to_string(),
        _ => file.to_string(),
    }
}

/// (root, dest) de um arquivo dentro de um mod.
pub fn mod_root(path: &str) -> Option<(String, String)> {
    let segs: Vec<&str> = path.split('/').collect();
    let len = segs.len();
    if let Some(i) = (0..len.saturating_sub(1)).find(|&i| MOD_DIRS.contains(&segs[i].to_lowercase().as_str())) {
        // exefs/romfs/cheats em minúsculas: o emulador só os acha assim em sistemas de arquivos que diferenciam caixa (Linux)
        let rest = segs[i + 1..].iter().copied();
        let dest = std::iter::once(segs[i].to_lowercase()).chain(rest.map(str::to_string)).collect::<Vec<_>>().join("/");
        return Some((segs[..i].join("/"), dest));
    }
    if let Some(i) = segs.iter().position(|s| s.to_lowercase() == "exefs_patches") {
        if len >= 3 && i <= len - 3 {
            return Some((segs[..i + 2].join("/"), format!("exefs/{}", segs[len - 1])));
        }
    }
    let ext = ext_of(path);
    if ext == "pchtxt" || ext == "ips" {
        return Some((segs[..len - 1].join("/"), format!("exefs/{}", segs[len - 1])));
    }
    None
}

pub fn display_name(root: &str, fallback: &str) -> String {
    const SKIP: [&str; 4] = ["titles", "mods", "any", "exefs_patches"];
    for seg in root.split('/').rev() {
        let t = seg.trim_matches(|c| c == '[' || c == ']');
        if TID_SEG_RE.is_match(t) || VER_RE.is_match(seg) || (seg != "Titles" && SKIP.contains(&seg.to_lowercase().as_str())) {
            continue;
        }
        let t = t.trim();
        if t.is_empty() {
            continue;
        }
        return match t {
            "Titles" => "Cheats".into(),
            "NX-60FPS-RES-GFX-Cheats" => "60FPS-RES-GFX Cheats".into(),
            _ => t.to_string(),
        };
    }
    fallback.to_string()
}

pub fn version_of(path: &str) -> Option<String> {
    path.split('/')
        .find(|s| VER_RE.is_match(s))
        .map(|s| s.trim_start_matches('v').to_string())
}

fn is_archive(path: &str) -> bool {
    matches!(ext_of(path).as_str(), "zip" | "rar" | "7z")
}

pub fn build_catalog(tree: &[TreeItem], tree_sha: String, source: Source) -> Catalog {
    let items: Vec<&TreeItem> = tree
        .iter()
        .filter(|t| t.kind == "blob")
        .filter(|t| {
            let p = &t.path;
            if TID3DS_RE.is_match(p) || p.starts_with("Saves/") {
                return false;
            }
            if !p.contains('/') && !is_archive(p) {
                return false;
            }
            let file = p.rsplit('/').next().unwrap().to_lowercase();
            if file == "credits.txt" || file.starts_with("readme") {
                return false;
            }
            !matches!(ext_of(p).as_str(), "md" | "py" | "sh" | "yml" | "gitignore")
        })
        .collect();

    let mut names: HashMap<String, String> = HashMap::new();
    let mut reverse: HashMap<String, String> = HashMap::new();
    let mut learn = |names: &mut HashMap<String, String>, tid: String, name: &str| {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        reverse.entry(norm(name)).or_insert_with(|| tid.clone());
        names.entry(tid).or_insert_with(|| name.to_string());
    };
    // 1: NX-60FPS-RES-GFX-Cheats/titles/<TID>/<Nome>.txt
    for t in &items {
        let s: Vec<&str> = t.path.split('/').collect();
        if s.len() == 4
            && s[0] == "NX-60FPS-RES-GFX-Cheats"
            && s[1] == "titles"
            && TID_SEG_RE.is_match(s[2])
            && ext_of(s[3]) == "txt"
        {
            learn(&mut names, s[2].to_uppercase(), &stem_of(s[3]));
        }
    }
    // 2: zips na raiz "<Nome> [<TID>]…"
    for t in &items {
        if !t.path.contains('/') {
            if let (Some(tid), Some(i)) = (last_tid(&t.path), t.path.find(" [")) {
                learn(&mut names, tid, &t.path[..i]);
            }
        }
    }
    // 3: primeiro segmento de caminhos com TID
    for t in &items {
        if let Some((first, _)) = t.path.split_once('/') {
            if matches!(first, "Titles" | "Mods" | "NX-60FPS-RES-GFX-Cheats") {
                continue;
            }
            if let Some(tid) = last_tid(&t.path) {
                learn(&mut names, tid, first);
            }
        }
    }

    let mut mods: Vec<ModEntry> = Vec::new();
    let mut groups: BTreeMap<String, Vec<(&TreeItem, String)>> = BTreeMap::new();
    for t in &items {
        if is_archive(&t.path) {
            mods.push(ModEntry {
                id: t.path.clone(),
                tid: None,
                name: stem_of(&t.path),
                version: version_of(&t.path),
                kind: ModKind::Archive,
                files: vec![ModFile { src: t.path.clone(), dest: String::new() }],
                size: t.size.unwrap_or(0),
                group: t.path.split('/').next().unwrap().to_string(),
                source,
            });
        } else if let Some((root, dest)) = mod_root(&t.path) {
            if !root.is_empty() {
                groups.entry(root).or_default().push((t, dest));
            }
        }
    }
    for (root, files) in groups {
        mods.push(ModEntry {
            name: display_name(&root, "Mod"),
            version: version_of(&root),
            kind: ModKind::Files,
            size: files.iter().map(|(t, _)| t.size.unwrap_or(0)).sum(),
            group: root.split('/').next().unwrap().to_string(),
            files: files.into_iter().map(|(t, d)| ModFile { src: t.path.clone(), dest: d }).collect(),
            tid: None,
            id: root,
            source,
        });
    }
    for m in &mut mods {
        m.tid = last_tid(&m.id).or_else(|| reverse.get(&norm(&m.group)).cloned());
        m.source = source;
        m.id = format!("{}{}", source.id_prefix(), m.id);
    }
    mods.sort_by(|a, b| (&a.tid, &a.name).cmp(&(&b.tid, &b.name)));

    Catalog { tree_sha, fetched_at: now_secs(), mods, names, schema: SCHEMA }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

async fn fetch_tree(source: Source) -> Result<(String, Vec<TreeItem>), String> {
    let url = format!("https://api.github.com/repos/{}/git/trees/{}?recursive=1", source.repo(), source.branch());
    let resp = reqwest::Client::new()
        .get(&url)
        .header("User-Agent", UA)
        .send()
        .await
        .map_err(|e| format!("Falha de rede: {e}"))?;
    let status = resp.status();
    if status.as_u16() == 403 || status.as_u16() == 429 {
        return Err("Limite da API do GitHub atingido. Tente novamente mais tarde.".into());
    }
    if !status.is_success() {
        return Err(format!("GitHub respondeu HTTP {status}"));
    }
    let tr: TreeResp = resp.json().await.map_err(|e| format!("Resposta inválida: {e}"))?;
    if tr.truncated {
        return Err("Árvore do repositório truncada".into());
    }
    Ok((tr.sha, tr.tree))
}

/// Junta os catálogos (já na ordem de prioridade) sem sobreposição: um mod de fonte
/// menos prioritária some se uma anterior já tem mesmo jogo + mesmo nome (versões
/// iguais, ou alguma sem versão). ponytail: sem versão conta como "mesma"; um
/// refinamento seria comparar o tamanho.
fn merge(parts: Vec<Catalog>) -> Catalog {
    let mut names: HashMap<String, String> = HashMap::new();
    for p in &parts {
        for (tid, n) in &p.names {
            names.entry(tid.clone()).or_insert_with(|| n.clone());
        }
    }
    let mut reverse: HashMap<String, String> = HashMap::new();
    for (tid, n) in &names {
        reverse.entry(norm(n)).or_insert_with(|| tid.clone());
    }

    let tree_sha = parts.first().map(|p| p.tree_sha.clone()).unwrap_or_default();
    let mut seen: HashMap<(String, String), Vec<Option<String>>> = HashMap::new();
    let mut mods = Vec::new();
    for p in parts {
        let mut added: Vec<((String, String), Option<String>)> = Vec::new();
        for mut m in p.mods {
            if m.tid.is_none() {
                m.tid = reverse.get(&norm(&m.group)).cloned();
            }
            if let Some(tid) = &m.tid {
                let key = (tid.clone(), norm(&m.name));
                let dup = seen.get(&key).is_some_and(|vs| {
                    vs.iter().any(|v| v.is_none() || m.version.is_none() || *v == m.version)
                });
                if dup {
                    continue;
                }
                added.push((key, m.version.clone()));
            }
            mods.push(m);
        }
        for (k, v) in added {
            seen.entry(k).or_default().push(v);
        }
    }
    mods.sort_by(|a, b| (&a.tid, &a.name).cmp(&(&b.tid, &b.name)));
    Catalog { tree_sha, fetched_at: now_secs(), mods, names, schema: SCHEMA }
}

pub async fn fetch_catalog(cache: &Path) -> Result<Catalog, String> {
    // todas as fontes em paralelo (eram sequenciais); `parts` mantém a ordem de prioridade
    let trees: Vec<_> = Source::ALL
        .into_iter()
        .map(|s| tauri::async_runtime::spawn(async move { (s, fetch_tree(s).await) }))
        .collect();
    let pack = tauri::async_runtime::spawn(crate::pack::list());
    let mut parts = Vec::new();
    for h in trees {
        match h.await.map_err(|e| e.to_string())? {
            (s, Ok((sha, tree))) => parts.push(build_catalog(&tree, sha, s)),
            // fontes secundárias fora do ar não derrubam o catálogo; voltam no próximo "Atualizar"
            (Source::Official, Err(e)) => return Err(e),
            (_, Err(_)) => {}
        }
    }
    if let Ok(Ok(mods)) = pack.await {
        parts.push(Catalog { tree_sha: String::new(), fetched_at: now_secs(), mods, names: HashMap::new(), schema: SCHEMA });
    }
    let cat = merge(parts);
    if let Some(dir) = cache.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let json = serde_json::to_vec(&cat).map_err(|e| e.to_string())?;
    std::fs::write(cache, json).map_err(|e| format!("Falha ao gravar cache: {e}"))?;
    Ok(cat)
}

pub fn load_cached(cache: &Path) -> Option<Catalog> {
    serde_json::from_slice(&std::fs::read(cache).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mr(p: &str) -> Option<(String, String)> {
        mod_root(p)
    }

    #[test]
    fn mod_root_cases() {
        assert_eq!(
            mr("Animal Well/010020D01AD24000/2K/exefs/1.0.6.pchtxt"),
            Some(("Animal Well/010020D01AD24000/2K".into(), "exefs/1.0.6.pchtxt".into()))
        );
        assert_eq!(
            mr("AER: Memories of Old/exefs_patches/AER60FPS/15DCB7935B84A141B6CA738DEE0123A7.ips"),
            Some((
                "AER: Memories of Old/exefs_patches/AER60FPS".into(),
                "exefs/15DCB7935B84A141B6CA738DEE0123A7.ips".into()
            ))
        );
        assert_eq!(
            mr("Advance Wars 1+2 Re-Boot Camp/[0100300012F2A000]/60fps/1.0.0.pchtxt"),
            Some((
                "Advance Wars 1+2 Re-Boot Camp/[0100300012F2A000]/60fps".into(),
                "exefs/1.0.0.pchtxt".into()
            ))
        );
        assert_eq!(
            mr("A Hat in Time/titles/010056E00853A000/romfs/HatinTimeGame/Config/Switch/SwitchEngine.ini"),
            Some((
                "A Hat in Time/titles/010056E00853A000".into(),
                "romfs/HatinTimeGame/Config/Switch/SwitchEngine.ini".into()
            ))
        );
        assert_eq!(mr("Titles/01006FE013472000/credits.txt"), None);
    }

    #[test]
    fn display_names() {
        assert_eq!(display_name("Advance Wars 1+2 Re-Boot Camp/[0100300012F2A000]/60fps", "x"), "60fps");
        assert_eq!(display_name("AER: Memories of Old/exefs_patches/AER60FPS", "x"), "AER60FPS");
        assert_eq!(display_name("A Hat in Time/titles/010056E00853A000", "x"), "A Hat in Time");
        assert_eq!(display_name("Titles/01006FE013472000", "x"), "Cheats");
    }

    #[test]
    fn last_tid_adjacent() {
        assert_eq!(
            last_tid("a/0100000000000001/0100000000000002/x").as_deref(),
            Some("0100000000000002")
        );
    }

    #[test]
    fn catalog_build() {
        let b = |p: &str| TreeItem { path: p.into(), kind: "blob".into(), size: Some(10) };
        let tree = vec![
            b("Crystareino (USA)/00040000001C6C00.txt"),
            b("Animal Crossing New Horizons/16.10 Aspect Ratio.zip"),
            b("Animal Crossing New Horizons [01006F8002326000][mods].zip"),
            b("NX-60FPS-RES-GFX-Cheats/titles/0100A21017C42000/Another Crab's Treasure.txt"),
            b("Titles/01006FE013472000/cheats/2841E26D7FB8AA17.txt"),
            b("Titles/01006FE013472000/credits.txt"),
        ];
        let c = build_catalog(&tree, "sha".into(), Source::Official);
        assert!(!c.mods.iter().any(|m| m.id.contains("00040000001C6C00")));
        let ac = c.mods.iter().find(|m| m.id.ends_with("16.10 Aspect Ratio.zip")).unwrap();
        assert_eq!(ac.tid.as_deref(), Some("01006F8002326000"));
        assert_eq!(c.names["0100A21017C42000"], "Another Crab's Treasure");
        let ch = c.mods.iter().find(|m| m.id == "Titles/01006FE013472000").unwrap();
        assert_eq!(ch.name, "Cheats");
        assert_eq!(ch.tid.as_deref(), Some("01006FE013472000"));
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    fn arc(path: &str) -> TreeItem {
        TreeItem { path: path.into(), kind: "blob".into(), size: Some(10) }
    }

    #[test]
    fn merge_drops_overlap_and_resolves_wiki_tid() {
        let off = build_catalog(
            &[arc("Animal Crossing New Horizons [01006F8002326000][mods].zip"),
              arc("Animal Crossing New Horizons/60fps.zip")],
            "a".into(),
            Source::Official,
        );
        let boy = build_catalog(
            &[arc("ACNH/[01006F8002326000]/1.0.0/60FPS.rar"), arc("ACNH/[01006F8002326000]/1.0.0/4K.rar")],
            "b".into(),
            Source::Theboy181,
        );
        let wiki = build_catalog(
            &[arc("Animal Crossing New Horizons/60fps.zip"), arc("Animal Crossing New Horizons/21.9 Ultrawide.zip")],
            "c".into(),
            Source::Wiki,
        );
        let m = merge(vec![off, boy, wiki]);
        let of_game: Vec<_> = m.mods.iter().filter(|x| x.tid.as_deref() == Some("01006F8002326000")).collect();
        // 60fps existe nas três fontes: fica só a oficial
        let fps: Vec<_> = of_game.iter().filter(|x| norm(&x.name) == "60fps").collect();
        assert_eq!(fps.len(), 1);
        assert_eq!(fps[0].source, Source::Official);
        // 4K (só TheBoy181) e Ultrawide (só Wiki, tid achado pelo nome) entram
        assert!(of_game.iter().any(|x| x.source == Source::Theboy181 && x.name == "4K"));
        assert!(of_game.iter().any(|x| x.source == Source::Wiki && x.id == "wiki:Animal Crossing New Horizons/21.9 Ultrawide.zip"));
    }
}
