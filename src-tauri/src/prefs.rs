use crate::Dir;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use walkdir::WalkDir;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    cache_bytes: u64,
    tools_bytes: u64,
}

fn size(p: &Path) -> u64 {
    WalkDir::new(p).into_iter().flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()
}

fn remove(p: &Path, what: &str) -> Result<(), String> {
    let r = if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
    match r {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Falha ao remover {what}: {e}")),
        _ => Ok(()),
    }
}

/// Só o que o app grava no cache. A pasta `Dir::Cache` inteira NÃO pode ser apagada: no Windows
/// ela é a mesma do perfil do WebView2 (`EBWebView`), que fica em uso.
fn cache_items(app: &AppHandle) -> Result<[PathBuf; 2], String> {
    let d = crate::app_dir(app, Dir::Cache)?;
    Ok([d.join("catalog.json"), d.join("covers")])
}

fn tools_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(crate::app_dir(app, Dir::Data)?.join("tools"))
}

#[tauri::command]
pub fn storage_info(app: AppHandle) -> Result<StorageInfo, String> {
    Ok(StorageInfo {
        cache_bytes: cache_items(&app)?.iter().map(|p| size(p)).sum(),
        tools_bytes: size(&tools_dir(&app)?),
    })
}

/// Apaga catálogo em cache e capas. Não toca em manifesto de mods instalados (fica em `Dir::Data`).
#[tauri::command]
pub fn clear_cache(app: AppHandle) -> Result<(), String> {
    cache_items(&app)?.iter().try_for_each(|p| remove(p, "cache"))
}

/// Remove as ferramentas NSZ baixadas; são baixadas de novo no próximo uso.
#[tauri::command]
pub fn remove_tools(app: AppHandle) -> Result<(), String> {
    remove(&tools_dir(&app)?, "ferramentas")
}
