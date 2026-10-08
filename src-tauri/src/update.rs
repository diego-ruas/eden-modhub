use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

#[cfg(windows)]
const REPO: &str = "pendiego/Eden-Mod-Manager";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    current: String,
    version: Option<String>,
    notes: Option<String>,
}

async fn find(app: &AppHandle) -> Result<Option<tauri_plugin_updater::Update>, String> {
    let err = |e: tauri_plugin_updater::Error| format!("Falha ao procurar atualização: {e}");
    app.updater().map_err(err)?.check().await.map_err(err)
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<UpdateCheck, String> {
    let u = find(&app).await?;
    Ok(UpdateCheck {
        current: app.package_info().version.to_string(),
        version: u.as_ref().map(|u| u.version.clone()),
        notes: u.and_then(|u| u.body),
    })
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let u = find(&app).await?.ok_or("Nenhuma atualização disponível")?;
    #[cfg(windows)]
    if crate::portable_dir().is_some() {
        return replace_portable(&app, &u.version).await;
    }
    let (mut received, mut last) = (0u64, 0u64);
    u.download_and_install(
        |chunk, total| {
            received += chunk as u64;
            if received - last >= 256 * 1024 {
                last = received;
                let _ = app.emit("download-progress", serde_json::json!({ "received": received, "total": total }));
            }
        },
        || {},
    )
    .await
    .map_err(|e| format!("Falha ao atualizar: {e}"))?;
    app.restart() // Windows/NSIS: o plugin já fecha o app e o instalador o reabre
}

#[cfg(windows)]
async fn replace_portable(app: &AppHandle, version: &str) -> Result<(), String> {
    use std::io::Read;
    let fail = |e: &dyn std::fmt::Display| format!("Falha ao atualizar: {e}");
    let url = format!("https://github.com/{REPO}/releases/download/v{version}/EdenModHub-portable.zip");
    let zip_path = crate::app_dir(app, crate::Dir::Cache)?.join("update.zip");
    crate::install::download(&url, &zip_path, app).await?;
    let sig = reqwest::Client::new()
        .get(format!("{url}.sig"))
        .header("User-Agent", crate::catalog::UA)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Falha ao baixar {url}.sig: {e}"))?
        .text()
        .await
        .map_err(|e| format!("Falha ao baixar {url}.sig: {e}"))?;
    let bytes = std::fs::read(&zip_path).map_err(|e| fail(&e))?;
    let _ = std::fs::remove_file(&zip_path);
    let pubkey = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|p| p.as_str())
        .ok_or("Falha ao atualizar: pubkey ausente")?;
    verify(&bytes, &sig, pubkey)?;
    let mut exe = Vec::new();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).map_err(|e| fail(&e))?;
    z.by_name("EdenModHub.exe")
        .map_err(|e| fail(&e))?
        .read_to_end(&mut exe)
        .map_err(|e| fail(&e))?;
    let cur = std::env::current_exe().map_err(|e| fail(&e))?;
    let old = cur.with_extension("exe.old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&cur, &old).map_err(|e| fail(&e))?;
    if let Err(e) = std::fs::write(&cur, &exe) {
        let _ = std::fs::rename(&old, &cur);
        return Err(fail(&e));
    }
    std::process::Command::new(&cur).spawn().map_err(|e| fail(&e))?;
    app.exit(0);
    Ok(())
}

#[cfg(any(windows, test))]
fn verify(data: &[u8], sig_b64: &str, pubkey_b64: &str) -> Result<(), String> {
    use base64::Engine;
    let bad = || "Assinatura da atualização inválida".to_string();
    let dec = |s: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(s.trim())
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
    };
    let pk = minisign_verify::PublicKey::decode(&dec(pubkey_b64).ok_or_else(bad)?).map_err(|_| bad())?;
    let sig = minisign_verify::Signature::decode(&dec(sig_b64).ok_or_else(bad)?).map_err(|_| bad())?;
    pk.verify(data, &sig, true).map_err(|_| bad())
}

/// Remove o exe antigo deixado pela troca do portátil. O processo antigo pode ainda estar
/// fechando, então tenta de novo em uma thread.
pub fn cleanup_old_exe() {
    let Ok(old) = std::env::current_exe().map(|e| e.with_extension("exe.old")) else { return };
    if !old.exists() {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..20 {
            if std::fs::remove_file(&old).is_ok() || !old.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::verify;

    const PK: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEVCNkVBRDY2RTZFOUNCMDgKUldRSXkrbm1acTF1Njl3MEV6SEhqVzM2WnhkRWVlclNBTXpYVWwvT3VEZ1pxZm01RDVnS29sekEK";
    const SIG: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRSXkrbm1acTF1NnpiT0hVWEkyaHkvbTVRU1pUTEdYdlE4cFJCNUcvRnBOYzR2YXJpMG1GNnd1QW14TnlIS3R6VEUzbnZGK1pQeUVpNHM2UEkyOWJCd2FLSDdOemU0aWc4PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkxNDgwMDUxCWZpbGU6ZGF0YS5iaW4KcDRsUW1iS1VLQ3BEUFBuU1BhSzVrWkk3OXREZHRpdDA1QjFSNnhmei9LMHFubnJBMnZQeXpMSXgzTEFuYyt1STFsbnlEa0U2cFRWYmUrUjhDa0syQnc9PQo=";
    const PROD_PK: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEY4Qjk4QjJEQTRDRTM2MjUKUldRbE5zNmtMWXU1K0FXeHpTTHpJSHhyRUxaUTBXdm5oaUxoa2huUlU3b0lMZmN4ZDdiSCtJT1UK";

    #[test]
    fn verify_accepts_signed_and_rejects_tampered() {
        assert!(verify(b"eden-modhub", SIG, PK).is_ok());
        assert!(verify(b"eden-modhuB", SIG, PK).is_err());
        assert!(verify(b"eden-modhub", SIG, PROD_PK).is_err());
    }
}
