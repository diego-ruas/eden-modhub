mod catalog;
mod emu;
mod install;
mod nsz;
mod pack;
mod prefs;
mod update;

use catalog::Catalog;
use emu::{Emu, Kind};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

#[derive(Default)]
pub struct CatalogState(pub Mutex<Option<Catalog>>);

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Settings {
    emulator: Kind,
    /// pasta de dados escolhida por emulador
    dirs: HashMap<Kind, String>,
}

#[derive(Clone, Copy)]
pub enum Dir {
    Config,
    Data,
    Cache,
}

/// Pasta do executável quando o modo portátil está ativo (arquivo `portable` ao lado do exe).
pub fn portable_dir() -> Option<PathBuf> {
    let d = std::env::current_exe().ok()?.parent()?.to_path_buf();
    d.join("portable").exists().then_some(d)
}

/// Modo portátil: tudo (configurações, cache, ferramentas) fica em `data/` ao lado do exe,
/// sem tocar no perfil do usuário.
pub fn app_dir(app: &AppHandle, kind: Dir) -> Result<PathBuf, String> {
    if let Some(d) = portable_dir() {
        return Ok(d.join("data").join(match kind {
            Dir::Config => "config",
            Dir::Data => "data",
            Dir::Cache => "cache",
        }));
    }
    let p = app.path();
    match kind {
        Dir::Config => p.app_config_dir(),
        Dir::Data => p.app_data_dir(),
        Dir::Cache => p.app_cache_dir(),
    }
    .map_err(|e| e.to_string())
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_dir(app, Dir::Config)?.join("settings.json"))
}

fn load_settings(app: &AppHandle) -> Settings {
    settings_path(app)
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_settings(app: &AppHandle, s: &Settings) -> Result<(), String> {
    let sp = settings_path(app)?;
    std::fs::create_dir_all(sp.parent().unwrap()).map_err(|e| e.to_string())?;
    let json = serde_json::to_vec(s).map_err(|e| e.to_string())?;
    std::fs::write(sp, json).map_err(|e| e.to_string())
}

fn emu_for(s: &Settings, kind: Kind) -> Option<Emu> {
    let dir = s
        .dirs
        .get(&kind)
        .map(PathBuf::from)
        .filter(|p| kind.validate(p))
        .or_else(|| kind.default_dir())?;
    Some(Emu { kind, dir })
}

fn current_emu(app: &AppHandle) -> Option<Emu> {
    let s = load_settings(app);
    emu_for(&s, s.emulator)
}

pub fn resolve_emu(app: &AppHandle) -> Result<Emu, String> {
    current_emu(app).ok_or_else(|| "Pasta do emulador não configurada".to_string())
}

#[derive(Serialize)]
struct EmuInfo {
    kind: Kind,
    dir: Option<String>,
}

#[tauri::command]
fn get_emu(app: AppHandle) -> EmuInfo {
    EmuInfo {
        kind: load_settings(&app).emulator,
        dir: current_emu(&app).map(|e| e.dir.to_string_lossy().into_owned()),
    }
}

#[tauri::command]
fn set_emulator(app: AppHandle, kind: Kind) -> Result<(), String> {
    let mut s = load_settings(&app);
    s.emulator = kind;
    save_settings(&app, &s)
}

#[tauri::command]
fn set_emu_dir(app: AppHandle, kind: Kind, path: String) -> Result<(), String> {
    let mut s = load_settings(&app);
    if !kind.validate(std::path::Path::new(&path)) {
        return Err(kind.invalid_msg().into());
    }
    s.dirs.insert(kind, path);
    save_settings(&app, &s)
}

#[derive(Serialize)]
struct EmuDir {
    kind: Kind,
    dir: Option<String>,
}

#[tauri::command]
fn get_emu_dirs(app: AppHandle) -> Vec<EmuDir> {
    let s = load_settings(&app);
    [Kind::Eden, Kind::Yuzu, Kind::Ryujinx]
        .map(|kind| EmuDir { kind, dir: emu_for(&s, kind).map(|e| e.dir.to_string_lossy().into_owned()) })
        .into()
}

#[tauri::command]
async fn get_catalog(app: AppHandle, state: State<'_, CatalogState>, force: bool) -> Result<Catalog, String> {
    let cache = app_dir(&app, Dir::Cache)?.join("catalog.json");
    let cached = catalog::load_cached(&cache);
    if !force {
        if let Some(c) = cached
            .as_ref()
            .filter(|c| c.schema == catalog::SCHEMA && catalog::now_secs().saturating_sub(c.fetched_at) < 86400)
        {
            *state.0.lock() = Some(c.clone());
            return Ok(c.clone());
        }
    }
    let cat = match catalog::fetch_catalog(&cache).await {
        Ok(c) => c,
        Err(e) => cached.ok_or(e)?,
    };
    *state.0.lock() = Some(cat.clone());
    Ok(cat)
}

#[tauri::command]
fn list_games(app: AppHandle, state: State<'_, CatalogState>) -> Result<Vec<emu::Game>, String> {
    let emu = resolve_emu(&app)?;
    let names = state.0.lock().as_ref().map(|c| c.names.clone()).unwrap_or_default();
    Ok(emu::list_games(&emu, &names))
}

/// Capa de um jogo pela internet (api.nlib.cc), reduzida a 128px e guardada no cache do app.
/// Falhas viram `None`: capa é enfeite, não deve gerar aviso de erro.
#[tauri::command]
async fn game_cover(app: AppHandle, tid: String) -> Option<String> {
    use base64::Engine;
    if tid.len() != 16 || !tid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let file = app_dir(&app, Dir::Cache).ok()?.join("covers").join(format!("{}.jpg", tid.to_lowercase()));
    let bytes = match std::fs::read(&file) {
        Ok(b) => b,
        Err(_) => {
            let resp = reqwest::Client::new()
                .get(format!("https://api.nlib.cc/nx/{tid}/icon"))
                .header("User-Agent", catalog::UA)
                .timeout(std::time::Duration::from_secs(20))
                .send()
                .await
                .ok()?
                .error_for_status()
                .ok()?;
            let raw = resp.bytes().await.ok()?;
            let small = image::load_from_memory(&raw).ok()?.thumbnail(128, 128);
            let mut out = Vec::new();
            small.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg).ok()?;
            let _ = std::fs::create_dir_all(file.parent()?);
            let _ = std::fs::write(&file, &out);
            out
        }
    };
    Some(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[tauri::command]
fn open_mod_folder(app: AppHandle, tid: String) -> Result<(), String> {
    emu::check_tid(&tid)?;
    use tauri_plugin_opener::OpenerExt;
    let dir = resolve_emu(&app)?.tid_dir(&tid);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(CatalogState::default())
        .manage(install::Pending::default())
        .manage(install::PeekCache::default())
        .setup(|app| {
            if let Some(e) = current_emu(app.handle()) {
                install::cleanup_tmp(&e.mods_dir());
            }
            update::cleanup_old_exe();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_emu,
            set_emulator,
            set_emu_dir,
            get_catalog,
            list_games,
            game_cover,
            open_mod_folder,
            install::prepare_install,
            install::peek_archive,
            install::commit_install,
            install::cancel_install,
            install::list_installed,
            install::uninstall,
            nsz::nsz_run,
            nsz::nsz_can_verify,
            nsz::list_roms,
            update::check_update,
            update::install_update,
            prefs::storage_info,
            prefs::clear_cache,
            prefs::remove_tools,
            get_emu_dirs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
