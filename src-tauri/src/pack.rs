//! Pacote de traduções PT-BR (staticpiratex/Traducoes-SWITCH-PTBR): um único .zip de ~200 MB numa
//! release, com `atmosphere/contents/<TID>/romfs/...`. Baixar tudo para instalar um jogo seria
//! um desperdício, então o catálogo lê só o diretório central do zip (Range) e a instalação
//! extrai apenas as entradas do TID escolhido.
use crate::catalog::{ModEntry, ModFile, ModKind, Source, UA};
use std::collections::BTreeMap;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Nome do asset na release.
pub const ZIP: &str = "SWITCH-PTBR-1.0.zip";
const CONTENTS: &str = "atmosphere/contents/";
const NAME: &str = "Tradução PT-BR";
/// Tamanho do bloco lido por requisição (cada uma custa um redirect do GitHub para o CDN).
const BLOCK: u64 = 4 << 20;
/// Teto absoluto de bytes extraídos de um pacote (2 GiB, o máximo de um asset de release).
const MAX_BYTES: u64 = 2 << 30;

/// Faz um GET com `Range`; devolve o corpo e o tamanho total do arquivo (de `Content-Range`).
async fn range(client: &reqwest::Client, url: &str, spec: &str) -> Result<(Vec<u8>, u64), String> {
    let err = |e: &dyn std::fmt::Display| format!("Falha ao baixar {url}: {e}");
    let resp = client
        .get(url)
        .header("User-Agent", UA)
        .header("Range", format!("bytes={spec}"))
        .send()
        .await
        .map_err(|e| err(&e))?;
    if resp.status().as_u16() != 206 {
        return Err(err(&format!("HTTP {}", resp.status())));
    }
    let total = resp
        .headers()
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| err(&"resposta sem Content-Range"))?;
    let body = resp.bytes().await.map_err(|e| err(&e))?;
    Ok((body.to_vec(), total))
}

/// Componente de caminho aceito no Windows e no Linux: sem `..`, `:` (ADS), `\`, espaço/ponto no fim
/// nem nomes reservados de dispositivo. O zip é de terceiros, então nada de confiar no `enclosed_name`
/// (ele só barra `..` que saia do próprio caminho da entrada, e as 4 pastas do prefixo dão folga).
fn safe_component(c: &str) -> bool {
    let stem = c.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.ends_with(|d: char| d.is_ascii_digit()));
    !c.is_empty()
        && c != "."
        && c != ".."
        && !c.contains([':', '\\', '*', '?', '"', '<', '>', '|'])
        && !c.chars().any(|ch| ch.is_control())
        && !c.ends_with(['.', ' '])
        && !reserved
}

/// `atmosphere/contents/<TID>/(romfs|exefs)/<arquivo>` → (TID em maiúsculas, caminho a partir de romfs/exefs).
fn entry_tid(name: &str) -> Option<(String, &str)> {
    let (tid, rest) = name.strip_prefix(CONTENTS)?.split_once('/')?;
    let mut comps = rest.split('/');
    let dir = comps.next()?;
    let ok = tid.len() == 16
        && tid.bytes().all(|b| b.is_ascii_hexdigit())
        && (dir.eq_ignore_ascii_case("romfs") || dir.eq_ignore_ascii_case("exefs"))
        && rest.contains('/')
        && rest.split('/').all(safe_component);
    ok.then(|| (tid.to_ascii_uppercase(), rest))
}

fn le(b: &[u8], o: usize, n: usize) -> u64 {
    b[o..o + n].iter().rev().fold(0, |a, &x| (a << 8) | x as u64)
}

/// Um mod por TID, montado só com o diretório central do zip (2 requisições pequenas).
pub async fn list() -> Result<Vec<ModEntry>, String> {
    let bad = || "Falha ao ler o pacote de traduções: diretório central inválido".to_string();
    let url = Source::Ptbr.raw_url(ZIP);
    let client = reqwest::Client::new();
    // o CDN do GitHub responde 501 a `bytes=-N`: descobre o tamanho e pede o fim por faixa explícita
    let (_, total) = range(&client, &url, "0-0").await?;
    let last = total.checked_sub(1).ok_or_else(bad)?;
    let (tail, _) = range(&client, &url, &format!("{}-{last}", total.saturating_sub(65536))).await?;
    let eocd = tail.windows(4).rposition(|w| w == [0x50, 0x4b, 0x05, 0x06]).ok_or_else(bad)?;
    if eocd + 22 > tail.len() {
        return Err(bad());
    }
    let (cd_size, cd_off) = (le(&tail, eocd + 12, 4), le(&tail, eocd + 16, 4));
    // 16 MiB comporta ~100 mil entradas; mais que isso não é um pacote de traduções
    if cd_size == 0 || cd_size > 16 << 20 || cd_off == 0xFFFF_FFFF || cd_off + cd_size > total {
        return Err(bad());
    }
    let (cd, _) = range(&client, &url, &format!("{cd_off}-{}", cd_off + cd_size - 1)).await?;

    let mut sizes: BTreeMap<String, u64> = BTreeMap::new();
    let mut p = 0;
    while p + 46 <= cd.len() && cd[p..p + 4] == [0x50, 0x4b, 0x01, 0x02] {
        let (size, nl) = (le(&cd, p + 24, 4), le(&cd, p + 28, 2) as usize);
        let (el, cl) = (le(&cd, p + 30, 2) as usize, le(&cd, p + 32, 2) as usize);
        let name = cd.get(p + 46..p + 46 + nl).ok_or_else(bad)?;
        if let Some((tid, _)) = entry_tid(&String::from_utf8_lossy(name)) {
            *sizes.entry(tid).or_default() += size;
        }
        p += 46 + nl + el + cl;
    }
    if sizes.is_empty() {
        return Err("Pacote de traduções vazio".into());
    }
    Ok(sizes
        .into_iter()
        .map(|(tid, size)| ModEntry {
            id: format!("{}{tid}", Source::Ptbr.id_prefix()),
            files: vec![ModFile { src: tid.clone(), dest: String::new() }],
            tid: Some(tid),
            name: NAME.into(),
            version: None,
            kind: ModKind::Pack,
            size,
            group: "PT-BR".into(),
            source: Source::Ptbr,
        })
        .collect())
}

/// Arquivo remoto seekable: lê blocos alinhados por Range e guarda o último.
struct RangeReader {
    client: reqwest::Client,
    url: String,
    len: u64,
    pos: u64,
    start: u64,
    buf: Vec<u8>,
}

impl Read for RangeReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len || out.is_empty() {
            return Ok(0);
        }
        if self.pos < self.start || self.pos >= self.start + self.buf.len() as u64 {
            let s = self.pos / BLOCK * BLOCK;
            let e = (s + BLOCK).min(self.len) - 1;
            let (b, _) = tauri::async_runtime::block_on(range(&self.client, &self.url, &format!("{s}-{e}")))
                .map_err(io::Error::other)?;
            if b.is_empty() {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            (self.start, self.buf) = (s, b);
        }
        let off = (self.pos - self.start) as usize;
        if off >= self.buf.len() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let n = out.len().min(self.buf.len() - off);
        out[..n].copy_from_slice(&self.buf[off..off + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for RangeReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let new = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(d) => self.len.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        };
        self.pos = new.ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        Ok(self.pos)
    }
}

/// Extrai de `url` só as entradas romfs/exefs do `tid` para `out/<romfs|exefs>/...`.
/// Bloqueante (chamar em `spawn_blocking`). `progress(recebidos, total)`; `total` = bytes esperados.
pub fn extract(url: &str, tid: &str, total: u64, out: &Path, mut progress: impl FnMut(u64, u64)) -> Result<(), String> {
    let fail = |e: &dyn std::fmt::Display| format!("Falha ao extrair tradução: {e}");
    let client = reqwest::Client::new();
    let (_, len) = tauri::async_runtime::block_on(range(&client, url, "0-0"))?;
    let reader = RangeReader { client, url: url.into(), len, pos: 0, start: 0, buf: Vec::new() };
    let mut z = zip::ZipArchive::new(reader).map_err(|e| format!("Falha ao ler o pacote de traduções: {e}"))?;
    let wanted: Vec<usize> = (0..z.len())
        .filter(|&i| z.name_for_index(i).and_then(entry_tid).is_some_and(|(t, _)| t == tid))
        .collect();
    let (mut received, mut last) = (0u64, 0u64);
    let mut chunk = vec![0u8; 64 * 1024];
    for i in wanted {
        let mut f = z.by_index(i).map_err(|e| fail(&e))?;
        let Some(rest) = entry_tid(f.name()).map(|(_, r)| r.to_string()) else { continue };
        let dest = out.join(&rest);
        // entry_tid já rejeita `..`, `:`, `\` e nomes reservados; confirma a contenção mesmo assim
        if !dest.starts_with(out) {
            continue;
        }
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d).map_err(|e| fail(&e))?;
        }
        let declared = f.size();
        let mut written = 0u64;
        let mut file = std::fs::File::create(&dest).map_err(|e| fail(&e))?;
        loop {
            let n = f.read(&mut chunk).map_err(|e| fail(&e))?;
            if n == 0 {
                break;
            }
            // limites de verdade: o tamanho declarado de cada entrada, o total da listagem e um teto fixo
            // (o declarado vem do próprio zip, então sozinho não impede um zip que declara 50 GB)
            written += n as u64;
            if written > declared || received + n as u64 > total.min(MAX_BYTES) {
                return Err(fail(&"tamanho excede o limite"));
            }
            file.write_all(&chunk[..n]).map_err(|e| fail(&e))?;
            received += n as u64;
            if received - last >= 256 * 1024 {
                last = received;
                progress(received, total);
            }
        }
    }
    progress(received, total);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "atmosphere/contents/01006000040C2000";

    #[test]
    fn entry_tid_accepts_normal_files() {
        assert_eq!(
            entry_tid(&format!("{T}/romfs/PJ033/Content/Paks/a.pak")),
            Some(("01006000040C2000".into(), "romfs/PJ033/Content/Paks/a.pak"))
        );
        assert!(entry_tid("atmosphere/contents/01006000040c2000/exefs/main.npdm").is_some());
    }

    #[test]
    fn entry_tid_rejects_traversal_and_windows_tricks() {
        for bad in [
            "romfs/../../../../x",
            "romfs/a/../../../../../x",
            "romfs/a\\..\\..\\x",
            "romfs/x.txt:stream",
            "romfs/NUL",
            "romfs/com1.txt",
            "romfs/dir ./f",
            "romfs/",
            "romfs",
            "romfs//f",
            "other/f",
        ] {
            assert!(entry_tid(&format!("{T}/{bad}")).is_none(), "{bad}");
        }
        assert!(entry_tid("atmosphere/contents/..%2f/romfs/f").is_none());
        assert!(entry_tid("atmosphere/contents/0100600004/romfs/f").is_none());
    }
}
