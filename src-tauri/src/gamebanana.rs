//! GameBanana (API pública v11). Diferente das outras fontes, não há árvore para indexar: os mods
//! são buscados por jogo, na hora em que o usuário abre o jogo, e entram no catálogo em memória.
//! Modo curado = destacados pelo site (`Featured`) ou com `MIN_LIKES` curtidas ou mais.
use crate::catalog::{self, ModEntry, ModFile, ModKind, Source, HTTP, UA};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const API: &str = "https://gamebanana.com/apiv11";
const MIN_LIKES: u32 = 50;
const PER_PAGE: u32 = 50;
// teto de segurança contra paginação infinita (5000 mods por consulta); o fim normal é `_bIsComplete`
const MAX_PAGES: u32 = 100;

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

#[derive(Deserialize)]
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
    let resp = HTTP
        .get(format!("{API}/{path}?{}", qs.join("&")))
        .header("User-Agent", UA)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("Falha de rede: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GameBanana respondeu HTTP {}", resp.status()));
    }
    resp.json().await.map_err(|e| format!("Resposta inválida: {e}"))
}

/// Id do jogo no GameBanana: primeiro resultado da busca com o mesmo nome normalizado.
/// ponytail: só nome exato; jogos com título diferente no site ficam sem mods de lá.
async fn find_game(name: &str) -> Result<Option<u64>, String> {
    let q = [
        ("_sSearchString", name.to_string()),
        ("_sModelName", "Game".into()),
        ("_nPerpage", "15".into()),
    ];
    let page: Page<GameRec> = get("Util/Search/Results", &q).await?;
    let want = catalog::norm(name);
    Ok(page.records.into_iter().find(|g| catalog::norm(&g.name) == want).map(|g| g.id))
}

/// Mods do jogo por curtidas (decrescente); para ao cair abaixo de `min_likes`.
async fn fetch(game: u64, featured: bool, min_likes: u32, max_pages: u32, out: &mut BTreeMap<u64, Rec>) -> Result<(), String> {
    for page in 1..=max_pages {
        let mut q = vec![
            ("_nPage", page.to_string()),
            ("_nPerpage", PER_PAGE.to_string()),
            ("_sSort", "Generic_MostLiked".into()),
            ("_aFilters[Generic_Game]", game.to_string()),
        ];
        if featured {
            q.push(("_aFilters[Generic_WasFeatured]", "true".into()));
        }
        let p: Page<Rec> = get("Mod/Index", &q).await?;
        let empty = p.records.is_empty();
        for r in p.records {
            if r.likes < min_likes {
                return Ok(());
            }
            out.insert(r.id, r);
        }
        // página vazia sem `_bIsComplete` não pode girar até o teto
        if p.meta.complete || empty {
            break;
        }
    }
    Ok(())
}

#[derive(Serialize, Clone)]
pub struct GbMod {
    #[serde(flatten)]
    pub entry: ModEntry,
    pub thumb: Option<String>,
    pub likes: u32,
    pub featured: bool,
}

#[derive(Serialize)]
pub struct GbList {
    pub found: bool,
    pub mods: Vec<GbMod>,
}

fn thumb(media: &serde_json::Value) -> Option<String> {
    media["_aImages"].as_array()?.iter().find_map(|i| Some(format!("{}/{}", i["_sBaseUrl"].as_str()?, i["_sFile100"].as_str()?)))
}

/// Mods do jogo `tid`/`name`. `all` = tudo que o site tem; senão só os curados.
pub async fn list(tid: &str, name: &str, all: bool) -> Result<GbList, String> {
    let Some(game) = find_game(name).await? else { return Ok(GbList { found: false, mods: Vec::new() }) };
    let mut recs = BTreeMap::new();
    if all {
        fetch(game, false, 0, MAX_PAGES, &mut recs).await?;
    } else {
        fetch(game, false, MIN_LIKES, MAX_PAGES, &mut recs).await?;
        fetch(game, true, 0, MAX_PAGES, &mut recs).await?;
    }
    let mut mods: Vec<_> = recs
        .into_values()
        .filter(|r| r.has_files && !r.obsolete)
        .map(|r| GbMod {
            thumb: thumb(&r.media),
            likes: r.likes,
            featured: r.featured,
            entry: ModEntry {
                id: format!("{}{}", Source::Gamebanana.id_prefix(), r.id),
                tid: Some(tid.to_string()),
                name: r.name,
                // a versão do GameBanana é do mod, não do jogo: não serve para o filtro de versão
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
    Ok(GbList { found: true, mods })
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

/// URL e extensão do arquivo mais recente do mod que o instalador sabe extrair.
pub async fn download_url(mod_id: &str) -> Result<(String, String), String> {
    let f: Files = get(&format!("Mod/{mod_id}"), &[("_csvProperties", "_aFiles".into())]).await?;
    f.files
        .into_iter()
        .filter_map(|f| {
            let ext = f.name.rsplit_once('.')?.1.to_lowercase();
            matches!(ext.as_str(), "zip" | "7z" | "rar").then_some((f.added, f.url, ext))
        })
        .max_by_key(|(added, ..)| *added)
        .map(|(_, url, ext)| (url, ext))
        .ok_or_else(|| "Nenhum arquivo zip/7z/rar neste mod do GameBanana".into())
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
        assert_eq!(thumb(&p.records[0].media), Some("https://x/ss/100-a.jpg".into()));
        assert_eq!(thumb(&p.records[1].media), None);
    }

    /// Zelda TotK: curados vêm com id, e o mod mais curtido resolve um download.
    #[test]
    #[ignore]
    fn gamebanana_network() {
        tauri::async_runtime::block_on(async {
            let curated = list("0100F2C0115B6000", "The Legend of Zelda: Tears of the Kingdom", false).await.unwrap();
            let all = list("0100F2C0115B6000", "The Legend of Zelda: Tears of the Kingdom", true).await.unwrap();
            println!("curados {} / todos {}", curated.mods.len(), all.mods.len());
            assert!(curated.found && !curated.mods.is_empty() && curated.mods.len() < all.mods.len());
            let top = &curated.mods[0];
            let (url, ext) = download_url(&top.entry.files[0].src).await.unwrap();
            println!("{} {url} {ext}", top.entry.name);
            assert!(url.starts_with("https://gamebanana.com/dl/"));
        });
    }
}
