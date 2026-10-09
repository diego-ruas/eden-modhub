use base64::Engine;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use walkdir::WalkDir;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub tid: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub icon: Option<String>,
}

static PV_TID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^01[0-9A-F]{14}$").unwrap());
static VER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+(?:\.\d+)+)").unwrap());

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Eden,
    Yuzu,
    Ryujinx,
}

impl Kind {
    pub fn default_dir(self) -> Option<PathBuf> {
        let (base, name, flatpak) = match self {
            Kind::Eden => (dirs::data_dir()?, "eden", None),
            Kind::Yuzu => (dirs::data_dir()?, "yuzu", Some(("org.yuzu_emu.yuzu", "data"))),
            Kind::Ryujinx => (dirs::config_dir()?, "Ryujinx", Some(("org.ryujinx.Ryujinx", "config"))),
        };
        let mut cands = vec![base.join(name)];
        if let Some(home) = dirs::home_dir() {
            if cfg!(target_os = "linux") {
                if let Some((id, sub)) = flatpak {
                    cands.push(home.join(".var").join("app").join(id).join(sub).join(name));
                }
            }
            // Eden/yuzu: modo portátil guarda tudo em ./user ao lado do executável
            if self != Kind::Ryujinx {
                for d in [home.join(name), home.join("Applications").join(name), home.join("Games").join(name)] {
                    cands.push(d.join("user"));
                }
            }
        }
        // prefere a pasta realmente usada (com config/mods); senão qualquer pasta existente
        cands.iter().find(|p| self.validate(p)).or_else(|| cands.iter().find(|p| p.is_dir())).cloned()
    }

    pub fn validate(self, p: &Path) -> bool {
        match self {
            Kind::Ryujinx => p.join("Config.json").is_file() || p.join("mods").is_dir(),
            _ => qt_config(p).is_file() || p.join("load").is_dir(),
        }
    }

    pub fn invalid_msg(self) -> &'static str {
        match self {
            Kind::Ryujinx => "Pasta inválida (esperado Config.json ou mods/)",
            _ => "Pasta inválida (esperado config/qt-config.ini ou load/)",
        }
    }
}

/// Emulador escolhido + sua pasta de dados.
pub struct Emu {
    pub kind: Kind,
    pub dir: PathBuf,
}

/// No Linux (XDG) config e cache ficam fora da pasta de dados (~/.config/<nome>, ~/.cache/<nome>);
/// no Flatpak, em ~/.var/app/<id>/{config,cache}/<nome>. Windows/macOS: subpasta da própria pasta.
fn xdg_sibling(dir: &Path, sub: &str) -> PathBuf {
    let inner = dir.join(sub);
    if inner.exists() {
        return inner;
    }
    let Some(name) = dir.file_name() else { return inner };
    let flatpak = dir
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "data"))
        .and_then(|p| p.parent())
        .map(|app| app.join(sub).join(name));
    let xdg = if sub == "config" { dirs::config_dir() } else { dirs::cache_dir() }.map(|d| d.join(name));
    [flatpak, xdg].into_iter().flatten().find(|p| p.as_path() != dir && p.exists()).unwrap_or(inner)
}

fn qt_config(dir: &Path) -> PathBuf {
    xdg_sibling(dir, "config").join("qt-config.ini")
}

fn read_ini(p: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(text) = std::fs::read_to_string(p) else { return map };
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = s.to_string();
        } else if let Some((k, v)) = line.split_once('=') {
            map.insert(format!("{section}/{k}"), v.trim().to_string());
        }
    }
    map
}

impl Emu {
    /// Pasta que contém uma subpasta por TID com os mods.
    pub fn mods_dir(&self) -> PathBuf {
        if self.kind == Kind::Ryujinx {
            return self.dir.join("mods").join("contents");
        }
        match read_ini(&qt_config(&self.dir)).get("Data%20Storage/load_directory") {
            Some(v) if !v.is_empty() => PathBuf::from(v),
            _ => self.dir.join("load"),
        }
    }

    /// Ryujinx usa o TID em minúsculas.
    pub fn tid_dir(&self, tid: &str) -> PathBuf {
        let name = if self.kind == Kind::Ryujinx { tid.to_lowercase() } else { tid.to_string() };
        self.mods_dir().join(name)
    }

    pub fn keys_dir(&self) -> PathBuf {
        self.dir.join(if self.kind == Kind::Ryujinx { "system" } else { "keys" })
    }
}

/// O TID vem do webview e vira nome de pasta: só 16 dígitos hexadecimais (nada de `..`, `\` ou caminho absoluto).
pub fn check_tid(tid: &str) -> Result<(), String> {
    if tid.len() == 16 && tid.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("TID inválido".into())
    }
}

/// TID do jogo base: updates (+0x800) e DLCs (+0x1000…) caem no app (últimos 13 bits zerados).
fn base_tid(tid: &str) -> String {
    u64::from_str_radix(tid, 16).map(|v| format!("{:016X}", v & !0x1FFF)).unwrap_or_else(|_| tid.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RomFile {
    pub path: String,
    pub name: String,
    pub ext: String,
    pub size: u64,
}

/// Pastas de jogos configuradas no emulador: (caminho, varredura recursiva).
fn configured_game_dirs(emu: &Emu) -> Vec<(String, bool)> {
    if emu.kind == Kind::Ryujinx {
        let cfg: serde_json::Value = std::fs::read(emu.dir.join("Config.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        // ponytail: assume varredura recursiva; o Config.json não tem esse flag
        return cfg["game_dirs"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).map(|s| (s.to_string(), true)).collect())
            .unwrap_or_default();
    }
    let ini = read_ini(&qt_config(&emu.dir));
    let n: usize = ini.get("UI/Paths\\gamedirs\\size").and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut out = Vec::new();
    for i in 1..=n {
        let Some(path) = ini.get(&format!("UI/Paths\\gamedirs\\{i}\\path")) else { continue };
        if matches!(path.as_str(), "SDMC" | "UserNAND" | "SysNAND") {
            continue;
        }
        let deep = ini.get(&format!("UI/Paths\\gamedirs\\{i}\\deep_scan")).is_some_and(|v| v == "true");
        out.push((path.clone(), deep));
    }
    out
}

/// Pastas configuradas; se não houver nenhuma existente, procura pastas comuns de jogos.
fn game_dirs(emu: &Emu) -> Vec<(String, bool)> {
    let mut out = configured_game_dirs(emu);
    if out.iter().any(|(p, _)| Path::new(p).is_dir()) {
        return out;
    }
    let Some(home) = dirs::home_dir() else { return out };
    let docs = dirs::document_dir().unwrap_or_else(|| home.join("Documents"));
    let bases = [home.clone(), docs];
    for b in &bases {
        for n in ["Games", "Jogos", "Roms", "ROMs", "Switch", "Nintendo Switch"] {
            for sub in ["", "Switch", "switch", "Nintendo Switch"] {
                let p = if sub.is_empty() { b.join(n) } else { b.join(n).join(sub) };
                let s = p.to_string_lossy().into_owned();
                if p.is_dir() && !out.iter().any(|(o, _)| *o == s) {
                    out.push((s, true));
                }
            }
        }
    }
    out
}

/// Arquivos nsp|xci|nsz|xcz nas pastas de jogos do emulador.
pub fn rom_files(emu: &Emu) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (path, deep) in game_dirs(emu) {
        if !Path::new(&path).is_dir() {
            continue;
        }
        for e in WalkDir::new(&path).max_depth(if deep { usize::MAX } else { 1 }).into_iter().flatten() {
            let ext = e.path().extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            if matches!(ext.as_str(), "nsp" | "xci" | "nsz" | "xcz") {
                out.push(e.path().to_path_buf());
            }
        }
    }
    out
}

pub fn list_games(emu: &Emu, names: &HashMap<String, String>) -> Vec<Game> {
    let mut games: HashMap<String, Game> = HashMap::new();

    // (a) cache da lista de jogos (Eden/yuzu)
    if let Ok(rd) = std::fs::read_dir(xdg_sibling(&emu.dir, "cache").join("game_list")) {
        for e in rd.flatten() {
            let fname = e.file_name().to_string_lossy().to_string();
            let (stem, is_icon) = match (fname.strip_suffix(".pv.txt"), fname.strip_suffix(".jpeg")) {
                (Some(s), _) => (s, false),
                (_, Some(s)) => (s, true),
                _ => continue,
            };
            let tid = base_tid(&stem.to_uppercase());
            if !PV_TID.is_match(&tid) {
                continue;
            }
            let g = games.entry(tid.clone()).or_insert_with(|| Game { tid: tid.clone(), name: None, version: None, icon: None });
            if is_icon {
                // capa do jogo base (o ícone de update/DLC é ignorado)
                if stem.eq_ignore_ascii_case(&tid) {
                    g.icon = std::fs::read(e.path()).ok().map(|b| {
                        format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(b))
                    });
                }
                continue;
            }
            let version = std::fs::read_to_string(e.path())
                .ok()
                .and_then(|c| VER.captures(&c).map(|m| m[1].to_string()));
            // o cache de DLC/update também cai aqui; a primeira versão achada vale
            if g.version.is_none() {
                g.version = version;
            }
        }
    }

    // (a') Ryujinx: games/<tid em minúsculas>/ existe para cada jogo já aberto
    if emu.kind == Kind::Ryujinx {
        if let Ok(rd) = std::fs::read_dir(emu.dir.join("games")) {
            for e in rd.flatten() {
                let tid = base_tid(&e.file_name().to_string_lossy().to_uppercase());
                if PV_TID.is_match(&tid) {
                    games.entry(tid.clone()).or_insert_with(|| Game { tid, name: None, version: None, icon: None });
                }
            }
        }
    }

    // (b) varredura das pastas de jogos
    for p in rom_files(emu) {
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let Some(tid) = crate::catalog::last_tid(&stem) else { continue };
        let base = base_tid(&tid);
        let name = stem.split(" [").next().unwrap_or(&stem).trim().to_string();
        let g = games
            .entry(base.clone())
            .or_insert_with(|| Game { tid: base, name: None, version: None, icon: None });
        if g.name.is_none() && !name.is_empty() && tid == g.tid {
            g.name = Some(name);
        }
    }

    let mut out: Vec<Game> = games.into_values().collect();
    for g in &mut out {
        if g.name.is_none() {
            g.name = names.get(&g.tid).cloned();
        }
    }
    out.sort_by(|a, b| {
        (a.name.as_deref().unwrap_or("~").to_lowercase(), &a.tid)
            .cmp(&(b.name.as_deref().unwrap_or("~").to_lowercase(), &b.tid))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ryujinx_layout() {
        let dir = std::env::temp_dir().join("eden-mod-manager-ryu-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Config.json"), r#"{"game_dirs":["D:/Jogos","E:/x"]}"#).unwrap();
        assert!(Kind::Ryujinx.validate(&dir));
        assert!(!Kind::Eden.validate(&dir));
        let emu = Emu { kind: Kind::Ryujinx, dir: dir.clone() };
        assert_eq!(game_dirs(&emu), vec![("D:/Jogos".into(), true), ("E:/x".into(), true)]);
        assert_eq!(emu.tid_dir("0100ABCD00001000"), dir.join("mods").join("contents").join("0100abcd00001000"));
        assert_eq!(emu.keys_dir(), dir.join("system"));
    }

    #[test]
    fn flatpak_layout_finds_config_outside_data_dir() {
        let root = std::env::temp_dir().join("eden-mod-manager-flatpak-test");
        let _ = std::fs::remove_dir_all(&root);
        let data = root.join("data").join("yuzu");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(root.join("config").join("yuzu")).unwrap();
        std::fs::write(root.join("config").join("yuzu").join("qt-config.ini"), "[Data%20Storage]\nload_directory=/x/load\n").unwrap();
        assert!(Kind::Yuzu.validate(&data));
        let emu = Emu { kind: Kind::Yuzu, dir: data };
        assert_eq!(emu.mods_dir(), PathBuf::from("/x/load"));
    }
}
