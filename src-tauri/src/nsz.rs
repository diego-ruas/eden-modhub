use crate::emu::{self, Emu, RomFile};
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use crate::{Dir};
use tauri::{AppHandle, Emitter};

const NSZ_VERSION: &str = "5.0.0";

fn asset() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("nsz-cli-windows-x64.exe")
    } else if cfg!(all(windows, target_arch = "aarch64")) {
        Some("nsz-cli-windows-arm64.exe")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("nsz-cli-linux-x64")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("nsz-cli-linux-arm64")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("nsz-cli-macos-arm64")
    } else {
        None
    }
}

async fn ensure_nsz(app: &AppHandle) -> Result<PathBuf, String> {
    let asset = asset().ok_or("nsz não disponível para esta plataforma")?;
    let dir = crate::app_dir(app, Dir::Data)?.join("tools").join(format!("nsz-{NSZ_VERSION}"));
    let exe = dir.join(asset);
    if exe.exists() {
        return Ok(exe);
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let part = dir.join(format!("{asset}.part"));
    let url = format!("https://github.com/nicoboss/nsz/releases/download/{NSZ_VERSION}/{asset}");
    if let Err(e) = crate::install::download(&url, &part, app).await {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, &exe).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    Ok(exe)
}

fn keys_dir(emu: &Emu) -> Result<PathBuf, String> {
    let d = emu.keys_dir();
    if d.join("prod.keys").exists() {
        Ok(d)
    } else {
        Err(format!("prod.keys não encontrado em {}. Configure as chaves no emulador primeiro.", d.display()))
    }
}

/// nsz 4.6.1 portátil (Python 3.10, abre onde o 5.x não abre). Só Windows x64; sem
/// `--keys`, lê `keys.txt` ao lado do executável, então copiamos o prod.keys do emulador.
async fn ensure_legacy(app: &AppHandle, keys: &std::path::Path) -> Result<Option<PathBuf>, String> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Ok(None);
    }
    const V: &str = "4.6.1";
    let dir = crate::app_dir(app, Dir::Data)?.join("tools").join(format!("nsz-{V}"));
    let app_dir = dir.join("app");
    if !app_dir.join("nsz.exe").exists() {
        let _ = app.emit("nsz-stage", "legacy");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let zip_path = dir.join("nsz.zip");
        let url = format!("https://github.com/nicoboss/nsz/releases/download/{V}/nsz_v{V}_win64_portable.zip");
        if let Err(e) = crate::install::download(&url, &zip_path, app).await {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
        let (zp, tmp) = (zip_path.clone(), dir.join("tmp"));
        tauri::async_runtime::spawn_blocking(move || {
            let f = std::fs::File::open(&zp).map_err(|e| e.to_string())?;
            zip::ZipArchive::new(f).and_then(|mut z| z.extract(&tmp)).map_err(|e| format!("Falha ao extrair zip: {e}"))
        })
        .await
        .map_err(|e| e.to_string())??;
        std::fs::rename(dir.join("tmp").join(format!("nsz_v{V}_win64_portable")), &app_dir).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&zip_path);
        let _ = std::fs::remove_dir_all(dir.join("tmp"));
    }
    std::fs::copy(keys.join("prod.keys"), app_dir.join("keys.txt")).map_err(|e| e.to_string())?;
    Ok(Some(app_dir.join("nsz.exe")))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NszResult {
    pub ok: bool,
    pub log: String,
    pub output: Option<String>,
    pub output_size: Option<u64>,
}

#[derive(Debug, PartialEq)]
enum Line {
    Progress { percent: Option<f64>, step: String },
    Log(String),
    Complete(String),
    Summary(bool),
    Ignore,
}

fn parse_line(line: &str) -> Line {
    if line.trim().is_empty() {
        return Line::Ignore;
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Line::Log(line.to_string());
    };
    let msg = || v["message"].as_str().unwrap_or("").to_string();
    match v["type"].as_str() {
        Some("progress") => Line::Progress {
            percent: v["percent"].as_f64(),
            step: v["step"].as_str().unwrap_or("").to_string(),
        },
        Some("info") => Line::Log(msg()),
        Some("warning") => Line::Log(format!("Aviso: {}", msg())),
        Some("error") => Line::Log(format!("Erro: {}", msg())),
        Some("complete") => match v["file"].as_str() {
            Some(f) if !f.is_empty() => Line::Complete(f.to_string()),
            _ => Line::Ignore,
        },
        Some("summary") => Line::Summary(v["success"].as_bool().unwrap_or(true)),
        _ => Line::Ignore,
    }
}

/// Argumentos do nsz. `keys` = Some → nsz 5.x (`--keys`, saída JSON); None → 4.6.x, que não tem essas opções.
fn build_args(op: &str, delete_source: bool, keys: Option<&std::path::Path>, path: &str) -> Result<Vec<String>, String> {
    let mut args: Vec<String> = Vec::new();
    if let Some(k) = keys {
        args.extend(["--machine-readable".into(), "--keys".into(), k.to_string_lossy().into_owned()]);
    }
    match op {
        "compress" | "decompress" => {
            args.push(if op == "compress" { "-C" } else { "-D" }.into());
            if delete_source {
                args.extend(["--quick-verify".into(), "--rm-source".into()]);
            }
        }
        "verify" if keys.is_none() => return Err("Verificar não está disponível neste computador (nsz 4.6)".into()),
        "verify" => args.push("--quick-verify".into()),
        "info" => args.push("--info".into()),
        _ => return Err("Operação inválida".into()),
    }
    args.push(path.to_string());
    Ok(args)
}

/// O nsz 5.x (PyInstaller, Python 3.11) não abre em algumas builds do Windows:
/// "Failed to load Python DLL … Acesso inválido ao local de memória".
const PY_LOAD_FAIL: &str = "Failed to load Python DLL";

#[tauri::command]
pub async fn nsz_run(app: AppHandle, op: String, path: String, delete_source: bool) -> Result<NszResult, String> {
    let emu = crate::resolve_emu(&app)?;
    let keys = keys_dir(&emu)?;
    let file = PathBuf::from(&path);
    if !file.is_file() {
        return Err("Arquivo não encontrado".into());
    }
    let args = build_args(&op, delete_source, Some(&keys), &path)?;
    // se o nsz 5 já falhou neste PC, vai direto para o 4.6
    let marker = crate::app_dir(&app, Dir::Data)?.join("tools").join("nsz5-broken");
    if marker.exists() && cfg!(all(windows, target_arch = "x86_64")) {
        let _ = app.emit("nsz-stage", "fallback");
        if let Some(legacy) = ensure_legacy(&app, &keys).await? {
            let _ = app.emit("nsz-stage", "run");
            return run_tool(&app, legacy, build_args(&op, delete_source, None, &path)?, file).await;
        }
    }
    let _ = app.emit("nsz-stage", "tool");
    let exe = ensure_nsz(&app).await?;
    let _ = app.emit("nsz-stage", "run");
    let res = run_tool(&app, exe, args, file.clone()).await?;
    if !res.log.contains(PY_LOAD_FAIL) {
        return Ok(res);
    }
    // o erro do primeiro executável não interessa ao usuário: a interface limpa o log
    let _ = app.emit("nsz-stage", "fallback");
    let Some(legacy) = ensure_legacy(&app, &keys).await? else { return Ok(res) };
    let _ = std::fs::write(&marker, b"");
    let _ = app.emit("nsz-stage", "run");
    run_tool(&app, legacy, build_args(&op, delete_source, None, &path)?, file).await
}

/// "Verificar" isolado só existe no nsz 5; se ele não abre neste PC (marcador), não há verificação.
#[tauri::command]
pub fn nsz_can_verify(app: AppHandle) -> bool {
    crate::app_dir(&app, Dir::Data).map(|d| !d.join("tools").join("nsz5-broken").exists()).unwrap_or(true)
}

/// Linhas do nsz 4.6 que só atrapalham: checagem de chaves.
fn is_noise(l: &str) -> bool {
    l.starts_with("Unconfirmed:") || l.trim().is_empty()
}

fn sibling_output(file: &std::path::Path, args: &[String]) -> Option<String> {
    let ext = match (file.extension()?.to_str()?.to_lowercase().as_str(), args.iter().any(|a| a == "-C")) {
        ("nsp", true) => "nsz",
        ("xci", true) => "xcz",
        ("nsz", false) => "nsp",
        ("xcz", false) => "xci",
        _ => return None,
    };
    let p = file.with_extension(ext);
    p.is_file().then(|| p.to_string_lossy().into_owned())
}

async fn run_tool(app: &AppHandle, exe: PathBuf, args: Vec<String>, file: PathBuf) -> Result<NszResult, String> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = Command::new(exe);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = file.parent() {
            cmd.current_dir(dir);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn().map_err(|e| format!("Falha ao executar nsz: {e}"))?;
        let mut err_pipe = child.stderr.take().unwrap();
        let app_err = app.clone();
        let err_thread = std::thread::spawn(move || {
            let mut s = String::new();
            for l in BufReader::new(&mut err_pipe).lines().map_while(Result::ok) {
                if is_noise(&l) {
                    continue;
                }
                let _ = app_err.emit("nsz-log", &l);
                s.push_str(&l);
                s.push('\n');
            }
            s
        });
        let mut log = String::new();
        let mut output = None;
        let mut summary_ok = None;
        for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
            match parse_line(&line) {
                Line::Progress { percent, step } => {
                    let _ = app.emit("nsz-progress", json!({ "percent": percent, "step": step }));
                }
                Line::Log(l) if is_noise(&l) => {}
                Line::Log(l) => {
                    let _ = app.emit("nsz-log", &l);
                    log.push_str(&l);
                    log.push('\n');
                }
                Line::Complete(f) => output = Some(f),
                Line::Summary(ok) => summary_ok = Some(ok),
                Line::Ignore => {}
            }
        }
        let status = child.wait().map_err(|e| format!("Falha ao executar nsz: {e}"))?;
        log.push_str(&err_thread.join().unwrap_or_default());
        let ok = status.success() && summary_ok.unwrap_or(true);
        // nsz 4.6 não informa o arquivo gerado: troca nsp↔nsz / xci↔xcz na mesma pasta
        if output.is_none() && ok {
            output = sibling_output(&file, &args);
        }
        let output_size = output.as_ref().and_then(|o| std::fs::metadata(o).ok()).map(|m| m.len());
        Ok(NszResult { ok, log, output, output_size })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn list_roms(app: AppHandle) -> Result<Vec<RomFile>, String> {
    let mut v: Vec<RomFile> = emu::rom_files(&crate::resolve_emu(&app)?)
        .into_iter()
        .map(|p| RomFile {
            name: p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            ext: p.extension().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default(),
            size: std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0),
            path: p.to_string_lossy().into_owned(),
        })
        .collect();
    v.sort_by_key(|r| r.name.to_lowercase());
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_line_cases() {
        assert_eq!(
            parse_line(r#"{"type":"progress","step":"x","percent":42.5}"#),
            Line::Progress { percent: Some(42.5), step: "x".into() }
        );
        assert_eq!(parse_line(r#"{"type":"complete","file":"C:/x.nsz"}"#), Line::Complete("C:/x.nsz".into()));
        assert_eq!(parse_line(r#"{"type":"summary","success":false,"errorCount":1}"#), Line::Summary(false));
        assert_eq!(parse_line(r#"{"type":"heartbeat","time":1}"#), Line::Ignore);
        assert_eq!(parse_line("Traceback"), Line::Log("Traceback".into()));
    }
}
