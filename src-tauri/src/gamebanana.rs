//! GameBanana (API pública v11). Diferente das outras fontes, não há árvore para indexar: os mods
//! são buscados por jogo, na hora em que o usuário abre o jogo, e entram no catálogo em memória.
//! Modo curado = destacados pelo site (`Featured`) ou com `MIN_LIKES` curtidas ou mais.
use crate::catalog::{self, ModEntry, ModFile, ModKind, Source, HTTP_JSON, UA};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const API: &str = "https://gamebanana.com/apiv11";
const MIN_LIKES: u32 = 50;
const PER_PAGE: u32 = 50;
// teto de segurança contra paginação infinita (5000 mods por consulta); o fim normal é `_bIsComplete`
const MAX_PAGES: u32 = 100;
// páginas pedidas em paralelo; o índice pesa ~380 KB por página sem gzip
const BATCH: u32 = 6;
const MAX_INFLIGHT: usize = 6;
const TTL: Duration = Duration::from_secs(600);

// pedidos simultâneos ao GameBanana (todas as varreduras juntas)
static INFLIGHT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(MAX_INFLIGHT);
// (tid, todos?) -> mods; vale `TTL`
static CACHE: LazyLock<Mutex<HashMap<(String, bool), (Instant, Vec<GbMod>)>>> = LazyLock::new(Default::default);
// cache de id do jogo no GameBanana por nome normalizado (evita roundtrip ao Util/Search/Results)
static GAME_IDS: LazyLock<Mutex<HashMap<String, u64>>> = LazyLock::new(Default::default);

#[derive(Deserialize)]
struct Page<T> {
    #[serde(rename = "_aMetadata")]
    meta: Meta,
    #[serde(rename = "_aRecords")]
    records: Vec<T>,
}

#[derive(Deserialize)]
struct Meta {
    #[serde(rename = "_bIsComplete")]
    complete: bool,
}

#[derive(Deserialize, Clone)]
struct Rec {
    #[serde(rename = "_idRow")]
    id: u64,
    #[serde(rename = "_sName")]
    name: String,
    #[serde(rename = "_bHasFiles", default)]
    has_files: bool,
    #[serde(rename = "_bIsObsolete", default)]
    obsolete: bool,
    #[serde(rename = "_nLikeCount", default)]
    likes: u32,
    #[serde(rename = "_bWasFeatured", default)]
    featured: bool,
    #[serde(rename = "_nViewCount", default)]
    views: u32,
    // Value: APIs PHP mandam `[]` no lugar de objeto vazio
    #[serde(rename = "_aPreviewMedia", default)]
    media: serde_json::Value,
}

#[derive(Deserialize)]
struct GameRec {
    #[serde(rename = "_idRow")]
    id: u64,
    #[serde(rename = "_sName")]
    name: String,
}

async fn get<T: serde::de::DeserializeOwned>(path: &str, query: &[(&str, String)]) -> Result<T, String> {
    let qs: Vec<String> = query
        .iter()
        .map(|(k, v)| format!("{}={}", crate::install::encode_segment(k), crate::install::encode_segment(v)))
        .collect();
    let url = format!("{API}/{path}?{}", qs.join("&"));
    // no máximo MAX_INFLIGHT pedidos simultâneos no total, qualquer que seja o número de varreduras
    let _permit = INFLIGHT.acquire().await.map_err(|e| e.to_string())?;
    let mut tries = 0;
    let resp = loop {
        let resp = HTTP_JSON
            .get(&url)
            .header("User-Agent", UA)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|e| format!("Falha de rede: {e}"))?;
        // 429: uma nova tentativa depois de uma pausa (respeita Retry-After, até 5 s)
        if resp.status().as_u16() == 429 && tries == 0 {
            tries += 1;
            let wait = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(2u64);
            tokio::time::sleep(Duration::from_secs(wait.min(5))).await;
            continue;
        }
        break resp;
    };
    if !resp.status().is_success() {
        return Err(format!("GameBanana respondeu HTTP {}", resp.status()));
    }
    resp.json().await.map_err(|e| format!("Resposta inválida: {e}"))
}

/// Id do jogo no GameBanana: primeiro resultado da busca com o mesmo nome normalizado.
/// ponytail: só nome exato; jogos com título diferente no site ficam sem mods de lá.
async fn find_game(name: &str) -> Result<Option<u64>, String> {
    let want = catalog::norm(name);
    if let Some(&id) = GAME_IDS.lock().get(&want) {
        return Ok(Some(id));
    }
    let q = [
        ("_sSearchString", name.to_string()),
        ("_sModelName", "Game".into()),
        ("_nPerpage", "15".into()),
    ];
    let page: Page<GameRec> = get("Util/Search/Results", &q).await?;
    let found = page.records.into_iter().find(|g| catalog::norm(&g.name) == want).map(|g| g.id);
    if let Some(id) = found {
        GAME_IDS.lock().insert(want, id);
    }
    Ok(found)
}

/// Uma página do índice de mods do jogo, por curtidas (decrescente).
async fn fetch_page(game: u64, featured: bool, page: u32) -> Result<Page<Rec>, String> {
    let mut q = vec![
        ("_nPage", page.to_string()),
        ("_nPerpage", PER_PAGE.to_string()),
        ("_sSort", "Generic_MostLiked".into()),
        ("_aFilters[Generic_Game]", game.to_string()),
    ];
    if featured {
        q.push(("_aFilters[Generic_WasFeatured]", "true".into()));
    }
    get("Mod/Index", &q).await
}

/// Busca um lote contíguo de páginas e informa se atingiu o fim ou o corte de likes.
async fn fetch_chunk(
    game: u64,
    featured: bool,
    min_likes: u32,
    start_page: u32,
    count: u32,
) -> Result<(BTreeMap<u64, Rec>, bool), String> {
    let mut out = BTreeMap::new();
    let end = start_page + count;
    let jobs: Vec<_> = (start_page..end)
        .map(|p| tauri::async_runtime::spawn(fetch_page(game, featured, p)))
        .collect();
    let mut complete = false;
    for job in jobs {
        let p = job.await.map_err(|e| format!("Falha de rede: {e}"))??;
        let empty = p.records.is_empty();
        for r in p.records {
            if r.likes < min_likes {
                complete = true;
                return Ok((out, complete));
            }
            out.insert(r.id, r);
        }
        if p.meta.complete || empty {
            complete = true;
        }
    }
    Ok((out, complete))
}

/// Mods do jogo por curtidas (decrescente), `BATCH` páginas por vez; para ao cair abaixo de `min_likes`.
async fn fetch(game: u64, featured: bool, min_likes: u32, max_pages: u32) -> Result<BTreeMap<u64, Rec>, String> {
    let mut out = BTreeMap::new();
    let mut page = 1;
    while page <= max_pages {
        let count = BATCH.min(max_pages - page + 1);
        let (chunk, complete) = fetch_chunk(game, featured, min_likes, page, count).await?;
        out.extend(chunk);
        if complete {
            break;
        }
        page += count;
    }
    Ok(out)
}

/// Procura um mod do GameBanana em qualquer cache existente em memória.
pub fn find_cached_mod(id: &str) -> Option<ModEntry> {
    let c = CACHE.lock();
    for (_, (_, mods)) in c.iter() {
        if let Some(m) = mods.iter().find(|m| m.entry.id == id) {
            return Some(m.entry.clone());
        }
    }
    None
}

/// Lista em cache e ainda válida. Curados são subconjunto de "todos", então saem dele sem rede.
fn cached(tid: &str, all: bool) -> Option<Vec<GbMod>> {
    let c = CACHE.lock();
    let fresh = |all| c.get(&(tid.to_string(), all)).filter(|(at, _)| at.elapsed() < TTL).map(|(_, m)| m);
    if let Some(m) = fresh(all) {
        return Some(m.clone());
    }
    if all {
        return None;
    }
    fresh(true).map(|m| m.iter().filter(|m| m.likes >= MIN_LIKES || m.featured).cloned().collect())
}

#[derive(Serialize, Clone)]
pub struct GbMod {
    #[serde(flatten)]
    pub entry: ModEntry,
    pub thumb: Option<String>,
    pub likes: u32,
    pub views: u32,
    pub featured: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GbMore {
    pub tid: String,
    pub mods: Vec<GbMod>,
    pub complete: bool,
}

#[derive(Serialize)]
pub struct GbList {
    pub found: bool,
    pub mods: Vec<GbMod>,
}
/// Primeira imagem do mod com algum dos tamanhos `keys` (em ordem de preferência);
/// _sFile530/_sFile800 só existem na primeira imagem, as demais trazem apenas _sFile100.
fn image(media: &serde_json::Value, keys: &[&str]) -> Option<String> {
    media["_aImages"].as_array()?.iter().find_map(|i| {
        let file = keys.iter().find_map(|k| i[*k].as_str())?;
        Some(format!("{}/{}", i["_sBaseUrl"].as_str()?, file))
    })
}

/// Mods do jogo `tid`/`name`. `all` = tudo que o site tem; senão só os curados.
/// Resultado fica em memória por `TTL`; `fresh` ignora o cache (botão de atualizar).
fn to_gb_mods(recs: BTreeMap<u64, Rec>, tid: &str) -> Vec<GbMod> {
    let mut mods: Vec<_> = recs
        .into_values()
        .filter(|r| r.has_files && !r.obsolete)
        .map(|r| GbMod {
            thumb: image(&r.media, &["_sFile530", "_sFile100"]),
            likes: r.likes,
            views: r.views,
            featured: r.featured,
            entry: ModEntry {
                id: format!("{}{}", Source::Gamebanana.id_prefix(), r.id),
                tid: Some(tid.to_string()),
                name: r.name,
                version: None,
                kind: ModKind::Archive,
                files: vec![ModFile { src: r.id.to_string(), dest: String::new() }],
                size: 0,
                group: String::new(),
                source: Source::Gamebanana,
            },
        })
        .collect();
    mods.sort_by(|a, b| b.likes.cmp(&a.likes).then_with(|| a.entry.name.cmp(&b.entry.name)));
    mods
}

/// Mods do jogo `tid`/`name`. `all` = tudo que o site tem (com entrega progressiva); senão só os curados.
/// O primeiro lote (páginas 1..=BATCH) é retornado imediatamente; o restante continua em segundo plano
/// emitindo eventos `gamebanana-more` e atualizando o cache para não bloquear a interface.
pub async fn list(
    app: Option<&AppHandle>,
    tid: &str,
    name: &str,
    all: bool,
    fresh: bool,
) -> Result<GbList, String> {
    if !fresh {
        if let Some(mods) = cached(tid, all) {
            return Ok(GbList { found: true, mods });
        }
    }
    let Some(game) = find_game(name).await? else {
        return Ok(GbList { found: false, mods: Vec::new() });
    };

    if !all {
        let featured = tauri::async_runtime::spawn(fetch(game, true, 0, MAX_PAGES));
        let mut r = fetch(game, false, MIN_LIKES, MAX_PAGES).await?;
        r.extend(featured.await.map_err(|e| format!("Falha de rede: {e}"))??);
        let mods = to_gb_mods(r, tid);
        CACHE.lock().insert((tid.to_string(), false), (Instant::now(), mods.clone()));
        return Ok(GbList { found: true, mods });
    }

    // Modo "Todos": busca e entrega o primeiro lote imediatamente
    let (mut all_recs, complete) = fetch_chunk(game, false, 0, 1, BATCH).await?;
    let initial_mods = to_gb_mods(all_recs.clone(), tid);
    CACHE.lock().insert((tid.to_string(), true), (Instant::now(), initial_mods.clone()));

    // Se houver mais páginas a buscar, continua em segundo plano alimentando o cache e a UI
    if !complete && app.is_some() {
        let app_handle = app.cloned().unwrap();
        let tid_owned = tid.to_string();
        tauri::async_runtime::spawn(async move {
            let mut page = BATCH + 1;
            while page <= MAX_PAGES {
                let count = BATCH.min(MAX_PAGES - page + 1);
                let Ok((chunk, is_complete)) = fetch_chunk(game, false, 0, page, count).await else { break };
                if chunk.is_empty() {
                    break;
                }
                all_recs.extend(chunk);
                let current_mods = to_gb_mods(all_recs.clone(), &tid_owned);
                CACHE.lock().insert((tid_owned.clone(), true), (Instant::now(), current_mods.clone()));
                let is_last = is_complete || page + count > MAX_PAGES;
                let _ = app_handle.emit(
                    "gamebanana-more",
                    GbMore {
                        tid: tid_owned.clone(),
                        mods: current_mods,
                        complete: is_last,
                    },
                );
                if is_complete {
                    break;
                }
                page += count;
            }
        });
    }

    Ok(GbList { found: true, mods: initial_mods })
}

/// Dados da página do mod para o popup.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GbDetail {
    pub text: String,
    pub image: Option<String>,
    pub submitter: Option<String>,
    pub version: Option<String>,
    pub downloads: u64,
    pub size: u64,
    pub updated: u64,
}

pub async fn detail(id: u64) -> Result<GbDetail, String> {
    let props = "_sText,_sVersion,_tsDateUpdated,_tsDateModified,_nDownloadCount,_aSubmitter,_aPreviewMedia,_aFiles";
    let v: serde_json::Value = get(&format!("Mod/{id}"), &[("_csvProperties", props.into())]).await?;
    let latest = v["_aFiles"].as_array().and_then(|f| f.iter().max_by_key(|f| f["_tsDateAdded"].as_u64().unwrap_or(0)));
    Ok(GbDetail {
        text: v["_sText"].as_str().unwrap_or_default().to_string(),
        image: image(&v["_aPreviewMedia"], &["_sFile800", "_sFile530", "_sFile100"]),
        submitter: v["_aSubmitter"]["_sName"].as_str().map(str::to_string),
        version: v["_sVersion"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
        downloads: v["_nDownloadCount"].as_u64().unwrap_or(0),
        size: latest.and_then(|f| f["_nFilesize"].as_u64()).unwrap_or(0),
        updated: v["_tsDateUpdated"].as_u64().or_else(|| v["_tsDateModified"].as_u64()).unwrap_or(0),
    })
}

#[derive(Deserialize)]
struct Files {
    #[serde(rename = "_aFiles", default)]
    files: Vec<File>,
}

#[derive(Deserialize)]
struct File {
    #[serde(rename = "_sFile")]
    name: String,
    #[serde(rename = "_sDownloadUrl")]
    url: String,
    #[serde(rename = "_tsDateAdded", default)]
    added: u64,
}

fn select_download(files: Vec<File>) -> Option<(String, String)> {
    files
        .into_iter()
        .filter_map(|f| {
            let ext = f.name.rsplit_once('.')?.1.to_lowercase();
            let unsupported = f.name.to_ascii_lowercase().ends_with("_tkcl.zip");
            (!unsupported && matches!(ext.as_str(), "zip" | "7z" | "rar"))
                .then_some((f.added, f.url, ext))
        })
        .max_by_key(|(added, ..)| *added)
        .map(|(_, url, ext)| (url, ext))
}

/// URL e extensão do arquivo instalável mais recente do mod.
pub async fn download_url(mod_id: &str) -> Result<(String, String), String> {
    let f: Files = get(&format!("Mod/{mod_id}"), &[("_csvProperties", "_aFiles".into())]).await?;
    select_download(f.files)
        .ok_or_else(|| "Nenhum pacote compatível neste mod do GameBanana".into())
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Registro mínimo (campos opcionais ausentes) e extras desconhecidos não podem quebrar o parse.
    #[test]
    fn parses_sparse_index_page() {
        let j = r#"{"_aMetadata":{"_nRecordCount":2,"_bIsComplete":true,"_nPerpage":50},
            "_aRecords":[{"_idRow":1,"_sName":"A","_bHasFiles":true,"_nLikeCount":7,"_bWasFeatured":true,"_x":{},"_aPreviewMedia":{"_aImages":[{"_sBaseUrl":"https://x/ss","_sFile100":"100-a.jpg"}]}},
                         {"_idRow":2,"_sName":"B","_aPreviewMedia":[]}]}"#;
        let p: Page<Rec> = serde_json::from_str(j).unwrap();
        assert!(p.meta.complete);
        assert_eq!((p.records[0].likes, p.records[0].featured, p.records[0].has_files), (7, true, true));
        assert_eq!((p.records[1].likes, p.records[1].has_files, p.records[1].obsolete), (0, false, false));
        let keys = ["_sFile530", "_sFile100"];
        assert_eq!(image(&p.records[0].media, &keys), Some("https://x/ss/100-a.jpg".into()));
        assert_eq!(image(&p.records[1].media, &keys), None);
        // tamanho maior tem preferência quando existe
        let m = serde_json::json!({"_aImages": [{"_sBaseUrl": "https://x", "_sFile100": "s.jpg", "_sFile530": "b.jpg"}]});
        assert_eq!(image(&m, &keys), Some("https://x/b.jpg".into()));
    }

    #[test]
    fn selects_layeredfs_archive_instead_of_tkmm_container() {
        let files: Files = serde_json::from_str(
            r#"{"_aFiles":[
                {"_sFile":"tkmm_version_-_infinite_rocket_shield_tkcl.zip","_sDownloadUrl":"https://gamebanana.com/dl/1584175","_tsDateAdded":1766241688,"_sDescription":"Use this for TKMM - includes both versions of the mod"},
                {"_sFile":"infinite_rocket_shield_efe82.7z","_sDownloadUrl":"https://gamebanana.com/dl/1581292","_tsDateAdded":1765825170,"_sDescription":"Infinite rocket all the time"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(
            select_download(files.files),
            Some(("https://gamebanana.com/dl/1581292".into(), "7z".into()))
        );
    }

    #[test]
    fn rejects_tkmm_container_without_installable_alternative() {
        let files: Files = serde_json::from_str(
            r#"{"_aFiles":[{"_sFile":"tkmm_version_-_infinite_rocket_shield_tkcl.zip","_sDownloadUrl":"https://gamebanana.com/dl/1584175","_tsDateAdded":1766241688,"_sDescription":"Use this for TKMM"}]}"#,
        )
        .unwrap();
        assert_eq!(select_download(files.files), None);
    }

    #[test]
    fn keeps_layeredfs_archive_with_tkmm_description() {
        let files: Files = serde_json::from_str(
            r#"{"_aFiles":[
                {"_sFile":"layeredfs_compatible.zip","_sDownloadUrl":"https://gamebanana.com/dl/1584000","_tsDateAdded":1766241688,"_sDescription":"Not for TKMM; use LayeredFS"},
                {"_sFile":"older_layeredfs.7z","_sDownloadUrl":"https://gamebanana.com/dl/1583999","_tsDateAdded":1766241600,"_sDescription":"LayeredFS package"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(
            select_download(files.files),
            Some(("https://gamebanana.com/dl/1584000".into(), "zip".into()))
        );
    }

    /// Zelda TotK: curados vêm com id, e o mod mais curtido resolve um download.
    #[test]
    #[ignore]
    fn gamebanana_network() {
        tauri::async_runtime::block_on(async {
            let t0 = Instant::now();
            let curated = list(None, "0100F2C0115B6000", "The Legend of Zelda: Tears of the Kingdom", false, true).await.unwrap();
            println!("curados em {:?}", t0.elapsed());
            let t0 = Instant::now();
            let all = list(None, "0100F2C0115B6000", "The Legend of Zelda: Tears of the Kingdom", true, true).await.unwrap();
            println!("curados {} / todos {} (todos em {:?})", curated.mods.len(), all.mods.len(), t0.elapsed());
            assert!(curated.found && !curated.mods.is_empty() && all.found && !all.mods.is_empty());
            // curados saem do cache de "todos" sem rede e batem com a busca direta
            let t0 = Instant::now();
            let derived = list(None, "0100F2C0115B6000", "x", false, false).await.unwrap();
            assert!(t0.elapsed() < Duration::from_millis(50));
            assert_eq!(derived.mods.len(), curated.mods.len());
            let top = &curated.mods[0];
            let (url, ext) = download_url(&top.entry.files[0].src).await.unwrap();
            println!("{} {url} {ext}", top.entry.name);
            assert!(url.starts_with("https://gamebanana.com/dl/"));
        });
    }

}
