//! Estado habilitado/desabilitado dos mods (gravado na config do emulador) e arquivos em conflito entre mods.
//!
//! Eden/yuzu: seção `[DisabledAddOns]` do qt-config.ini (array do Qt: `i\title_id` em decimal e
//! `i\disabled\j\d` com o nome da pasta). Ryujinx: `games/<tid>/mods.json` (`{"mods":[{name,path,enabled}]}`;
//! mod sem entrada vale como habilitado). O emulador reescreve a config ao fechar: mudanças feitas com ele aberto se perdem.

use crate::emu::{Emu, Kind};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const SECTION: &str = "[DisabledAddOns]";

type Entries = Vec<(u64, Vec<String>)>;

fn unquote(v: &str) -> String {
    let v = v.trim();
    let inner = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(v);
    inner.replace("\\\"", "\"")
}

fn quote(v: &str) -> String {
    if v.contains([',', ';', '"']) { format!("\"{}\"", v.replace('"', "\\\"")) } else { v.to_string() }
}

fn parse(body: &[&str]) -> Entries {
    let mut tids: BTreeMap<usize, u64> = BTreeMap::new();
    let mut names: BTreeMap<(usize, usize), String> = BTreeMap::new();
    for l in body {
        let Some((k, v)) = l.split_once('=') else { continue };
        let p: Vec<&str> = k.split('\\').collect();
        match p.as_slice() {
            [i, "title_id"] => {
                if let (Ok(i), Ok(v)) = (i.parse(), v.trim().parse()) {
                    tids.insert(i, v);
                }
            }
            [i, "disabled", j, "d"] => {
                if let (Ok(i), Ok(j)) = (i.parse(), j.parse()) {
                    names.insert((i, j), unquote(v));
                }
            }
            _ => {}
        }
    }
    tids.into_iter()
        .map(|(i, tid)| (tid, names.range((i, 0)..=(i, usize::MAX)).map(|(_, n)| n.clone()).collect()))
        .collect()
}

fn render(e: &Entries) -> Vec<String> {
    let mut out = vec![format!("size={}", e.len())];
    for (i, (tid, names)) in e.iter().enumerate() {
        let i = i + 1;
        out.push(format!("{i}\\title_id\\default=false"));
        out.push(format!("{i}\\title_id={tid}"));
        out.push(format!("{i}\\disabled\\size={}", names.len()));
    }
    for (i, (_, names)) in e.iter().enumerate() {
        for (j, n) in names.iter().enumerate() {
            out.push(format!("{}\\disabled\\{}\\d\\default=false", i + 1, j + 1));
            out.push(format!("{}\\disabled\\{}\\d={}", i + 1, j + 1, quote(n)));
        }
    }
    out
}

/// Reescreve só a seção `[DisabledAddOns]`; o resto do arquivo passa intacto.
fn rewrite(text: &str, f: impl FnOnce(&mut Entries)) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| l.trim() == SECTION);
    let (before, body, after) = match start {
        Some(s) => {
            let end = lines[s + 1..].iter().position(|l| l.trim_start().starts_with('[')).map_or(lines.len(), |p| s + 1 + p);
            (&lines[..=s], &lines[s + 1..end], &lines[end..])
        }
        None => (&lines[..], &lines[..0], &lines[..0]),
    };
    let mut e = parse(body);
    f(&mut e);
    let mut out: Vec<String> = before.iter().map(|s| s.to_string()).collect();
    if start.is_none() {
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push(SECTION.into());
    }
    out.extend(render(&e));
    if !after.is_empty() {
        out.push(String::new());
        out.extend(after.iter().map(|s| s.to_string()));
    }
    out.join(eol) + eol
}

fn qt_disabled(text: &str, tid: u64) -> HashSet<String> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(s) = lines.iter().position(|l| l.trim() == SECTION) else { return HashSet::new() };
    let end = lines[s + 1..].iter().position(|l| l.trim_start().starts_with('[')).map_or(lines.len(), |p| s + 1 + p);
    parse(&lines[s + 1..end]).into_iter().filter(|(t, _)| *t == tid).flat_map(|(_, n)| n).collect()
}

fn ryu_json(emu: &Emu, tid: &str) -> PathBuf {
    emu.dir.join("games").join(tid.to_lowercase()).join("mods.json")
}

fn read_json(p: &Path) -> serde_json::Value {
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| serde_json::json!({ "mods": [] }))
}

fn write_atomic(p: &Path, data: &[u8]) -> Result<(), String> {
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, data).and_then(|_| std::fs::rename(&tmp, p)).map_err(|e| format!("Falha ao salvar configuração do emulador: {e}"))
}

/// Nomes das pastas de mods desabilitadas para o jogo.
pub fn disabled(emu: &Emu, tid: &str) -> HashSet<String> {
    if emu.kind == Kind::Ryujinx {
        let v = read_json(&ryu_json(emu, tid));
        let mods = v.get("mods").and_then(|m| m.as_array()).cloned().unwrap_or_default();
        return mods
            .iter()
            .filter(|m| m.get("enabled").and_then(|e| e.as_bool()) == Some(false))
            .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
            .collect();
    }
    let text = std::fs::read_to_string(crate::emu::qt_config(&emu.dir)).unwrap_or_default();
    u64::from_str_radix(tid, 16).map(|t| qt_disabled(&text, t)).unwrap_or_default()
}

/// O ModLoader do Ryujinx casa por `FullName.Contains(path)` e usa a primeira entrada: "…/Mod" também casa com "…/Mod (2)".
/// Por isso toda pasta irmã ganha entrada explícita e as entradas ficam da mais longa (específica) para a mais curta.
fn ryu_apply(mods: &mut Vec<serde_json::Value>, tid_dir: &Path, siblings: &[String], folder: &str, enabled: bool) {
    let name = |m: &serde_json::Value| m.get("name").and_then(|n| n.as_str()).map(String::from);
    for f in siblings.iter().map(String::as_str).chain([folder]) {
        if !mods.iter().any(|m| name(m).as_deref() == Some(f)) {
            mods.push(serde_json::json!({ "name": f, "path": tid_dir.join(f).to_string_lossy(), "enabled": true }));
        }
    }
    if let Some(m) = mods.iter_mut().find(|m| name(m).as_deref() == Some(folder)) {
        m["enabled"] = enabled.into();
    }
    mods.sort_by_key(|m| std::cmp::Reverse(m.get("path").and_then(|p| p.as_str()).map_or(0, str::len)));
}

pub fn set_enabled(emu: &Emu, tid: &str, folder: &str, enabled: bool) -> Result<(), String> {
    if emu.kind == Kind::Ryujinx {
        let p = ryu_json(emu, tid);
        let mut v = read_json(&p);
        if !v.get("mods").is_some_and(|m| m.is_array()) {
            v["mods"] = serde_json::json!([]);
        }
        let mods = v["mods"].as_array_mut().unwrap();
        let siblings: Vec<String> = std::fs::read_dir(emu.tid_dir(tid))
            .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        ryu_apply(mods, &emu.tid_dir(tid), &siblings, folder, enabled);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        return write_atomic(&p, &serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())?);
    }
    let p = crate::emu::qt_config(&emu.dir);
    let text = std::fs::read_to_string(&p).map_err(|e| format!("Falha ao ler configuração do emulador: {e}"))?;
    let tid = u64::from_str_radix(tid, 16).map_err(|e| e.to_string())?;
    let out = rewrite(&text, |e| {
        let pos = e.iter().position(|(t, _)| *t == tid);
        if enabled {
            if let Some(i) = pos {
                e[i].1.retain(|n| n != folder);
            }
        } else if let Some(i) = pos {
            if !e[i].1.iter().any(|n| n == folder) {
                e[i].1.push(folder.to_string());
            }
        } else {
            e.push((tid, vec![folder.to_string()]));
        }
    });
    write_atomic(&p, out.as_bytes())
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    folders: Vec<String>,
    count: usize,
    sample: String,
}

/// Arquivos em `romfs/` ou `exefs/` presentes em mais de uma pasta de mod habilitada, agrupados pelo conjunto de pastas.
pub fn conflicts(game_dir: &Path, disabled: &HashSet<String>) -> Vec<Conflict> {
    conflicts_for(game_dir, disabled, false)
}

/// A SD só contém mods ativos; metadados na raiz de cada mod não são recursos do jogo.
pub fn arcropolis_conflicts(game_dir: &Path) -> Vec<Conflict> {
    conflicts_for(game_dir, &HashSet::new(), true)
}

fn conflicts_for(game_dir: &Path, disabled: &HashSet<String>, arcropolis: bool) -> Vec<Conflict> {
    let Ok(rd) = std::fs::read_dir(game_dir) else { return vec![] };
    let mut owners: HashMap<String, Vec<String>> = HashMap::new();
    for e in rd.flatten().filter(|e| e.path().is_dir()) {
        let folder = e.file_name().to_string_lossy().into_owned();
        if disabled.contains(&folder) {
            continue;
        }
        for f in WalkDir::new(e.path()).into_iter().flatten().filter(|f| f.file_type().is_file()) {
            let rel = f.path().strip_prefix(e.path()).unwrap().to_string_lossy().replace('\\', "/");
            let low = rel.to_lowercase();
            let game_file = if arcropolis {
                crate::install::ARC_DIRS.contains(&low.split('/').next().unwrap_or(""))
            } else { low.starts_with("romfs/") || low.starts_with("exefs/") };
            if game_file {
                owners.entry(low).or_default().push(folder.clone());
            }
        }
    }
    let mut groups: BTreeMap<Vec<String>, (usize, String)> = BTreeMap::new();
    for (file, mut folders) in owners.into_iter().filter(|(_, o)| o.len() > 1) {
        folders.sort();
        let g = groups.entry(folders).or_insert((0, file.clone()));
        g.0 += 1;
        g.1 = g.1.clone().min(file);
    }
    groups.into_iter().map(|(folders, (count, sample))| Conflict { folders, count, sample }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arcropolis_conflicts_compare_game_payload_not_metadata_or_disabled_siblings() {
        let dir = std::env::temp_dir().join(format!("emm-arc-conflicts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (folder, rel) in [("mods/A", "fighter/snake/model.bin"), ("mods/B", "fighter/snake/model.bin"),
            ("mods/A", "config.json"), ("mods/B", "config.json"), ("mods/A", "romfs/normal.bin"),
            (".eden-mod-manager-disabled/C", "fighter/snake/model.bin"),
            ("ordinary/FPS", "romfs/normal.bin"), ("ordinary/Other", "romfs/normal.bin")] {
            let path = dir.join(folder).join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"payload").unwrap();
        }
        let arc = arcropolis_conflicts(&dir.join("mods"));
        assert_eq!(arc, vec![Conflict { folders: vec!["A".into(), "B".into()], count: 1,
            sample: "fighter/snake/model.bin".into() }]);
        let ordinary = conflicts(&dir.join("ordinary"), &HashSet::new());
        assert_eq!(ordinary.len(), 1);
        assert_eq!(ordinary[0].sample, "romfs/normal.bin");
        assert_eq!(ordinary[0].folders, vec!["FPS".to_string(), "Other".to_string()]);
        let mut combined = arc;
        combined.extend(ordinary);
        assert_eq!(combined.len(), 2);
        assert!(combined.iter().all(|c| c.count == 1 && c.folders.len() == 2));
        assert!(conflicts(&dir.join("mods"), &HashSet::new()).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    const CFG: &str = "[DisabledAddOns]\nsize=1\n1\\title_id\\default=false\n1\\title_id=72080821221203968\n1\\disabled\\size=1\n1\\disabled\\1\\d\\default=false\n1\\disabled\\1\\d=Update (NAND)\n\n\n[Controls]\nenable_raw_input=false\n";

    fn tid() -> u64 {
        72080821221203968
    }

    #[test]
    fn qt_roundtrip_keeps_other_sections_and_existing_entries() {
        let out = rewrite(CFG, |e| e[0].1.push("60fps, \"x\"".into()));
        assert!(out.contains("[Controls]\nenable_raw_input=false\n"));
        let d = qt_disabled(&out, tid());
        assert_eq!(d, HashSet::from(["Update (NAND)".to_string(), "60fps, \"x\"".to_string()]));
    }

    #[test]
    fn qt_adds_game_and_creates_missing_section_with_crlf() {
        let out = rewrite("[Other]\r\na=1\r\n", |e| e.push((5, vec!["m".into()])));
        assert!(out.starts_with("[Other]\r\na=1\r\n\r\n[DisabledAddOns]\r\nsize=1\r\n"));
        assert_eq!(qt_disabled(&out, 5), HashSet::from(["m".to_string()]));
        assert!(qt_disabled(&out, 6).is_empty());
    }

    #[test]
    fn conflicts_group_by_folder_set_and_skip_disabled() {
        let dir = std::env::temp_dir().join(format!("emm-conflicts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (f, rel) in [
            ("A", "romfs/x.bin"),
            ("B", "ROMFS/X.bin"),
            ("A", "exefs/main.pchtxt"),
            ("B", "exefs/main.pchtxt"),
            ("C", "romfs/x.bin"),
            ("A", "readme.txt"),
            ("B", "readme.txt"),
        ] {
            let p = dir.join(f).join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"").unwrap();
        }
        let c = conflicts(&dir, &HashSet::from(["C".to_string()]));
        assert_eq!(c, vec![Conflict { folders: vec!["A".into(), "B".into()], count: 2, sample: "exefs/main.pchtxt".into() }]);
        assert_eq!(conflicts(&dir, &HashSet::new()).len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod ryu_tests {
    use super::*;

    /// Reproduz o `Find(x => FullName.Contains(x.Path))` do Ryujinx.
    fn effective(mods: &[serde_json::Value], full: &str) -> Option<bool> {
        mods.iter().find(|m| full.contains(m["path"].as_str().unwrap())).and_then(|m| m["enabled"].as_bool())
    }

    #[test]
    fn disabling_mod_does_not_disable_numbered_sibling() {
        let dir = Path::new("/g/0100");
        let mut mods = vec![];
        ryu_apply(&mut mods, dir, &["Mod".into(), "Mod (2)".into()], "Mod", false);
        assert_eq!(effective(&mods, &dir.join("Mod").to_string_lossy()), Some(false));
        assert_eq!(effective(&mods, &dir.join("Mod (2)").to_string_lossy()), Some(true));
        ryu_apply(&mut mods, dir, &["Mod".into(), "Mod (2)".into()], "Mod (2)", false);
        ryu_apply(&mut mods, dir, &["Mod".into(), "Mod (2)".into()], "Mod", true);
        assert_eq!(effective(&mods, &dir.join("Mod").to_string_lossy()), Some(true));
        assert_eq!(effective(&mods, &dir.join("Mod (2)").to_string_lossy()), Some(false));
    }
}

#[cfg(test)]
mod reenable_tests {
    use super::*;

    #[test]
    fn qt_reenable_removes_only_that_folder() {
        let a = rewrite("", |e| e.push((7, vec!["A".into(), "B".into()])));
        let b = rewrite(&a, |e| e[0].1.retain(|n| n != "A"));
        assert_eq!(qt_disabled(&b, 7), HashSet::from(["B".to_string()]));
        let c = rewrite(&b, |e| e[0].1.clear());
        assert!(qt_disabled(&c, 7).is_empty());
    }
}
