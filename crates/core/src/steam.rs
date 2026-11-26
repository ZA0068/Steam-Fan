//! Talking to the Steam app on this PC through its files.
//!
//! Steam has no official way for other programs to add games while it runs,
//! so the library (`shortcuts.vdf`) and Proton choices (`config.vdf`) are edited
//! while Steam is closed. The app checks that first.

use crate::config::Config;
use crate::vdf::{self, BMap, BValue, TBlock, TValue};
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- manifests

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Manifest {
    pub appid: u32,
    pub name: String,
    pub installdir: String,
    pub buildid: String,
    pub size: u64,
}

pub fn read_manifest(path: &Path) -> Option<Manifest> {
    let t = vdf::text_load(&fs::read_to_string(path).ok()?).ok()?;
    let a = t.block("AppState")?;
    Some(Manifest {
        appid: a.str("appid")?.parse().ok()?,
        name: a.str("name").unwrap_or("").to_string(),
        installdir: a.str("installdir").unwrap_or("").to_string(),
        buildid: a.str("buildid").unwrap_or("").to_string(),
        size: a.str("SizeOnDisk").and_then(|s| s.parse().ok()).unwrap_or(0),
    })
}

// ---------------------------------------------------------------- the Steam app

/// Is the Steam app running? Read straight from /proc, no tools needed.
pub fn running() -> bool {
    let Ok(rd) = fs::read_dir("/proc") else { return false };
    rd.flatten().any(|e| {
        let comm = fs::read_to_string(e.path().join("comm")).unwrap_or_default();
        matches!(comm.trim(), "steam" | "steamwebhelper")
    })
}

/// Steam library folders (each has a `steamapps/` folder).
pub fn libraries(steam_root: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let lf = steam_root.join("steamapps/libraryfolders.vdf");
    if let Some(t) = fs::read_to_string(lf).ok().and_then(|s| vdf::text_load(&s).ok()) {
        if let Some(b) = t.block("libraryfolders") {
            for (_, v) in &b.0 {
                if let TValue::Block(f) = v {
                    if let Some(p) = f.str("path") {
                        out.push(PathBuf::from(p));
                    }
                }
            }
        }
    }
    if out.is_empty() {
        out.push(steam_root.to_path_buf());
    }
    out.dedup();
    out
}

/// The installed copy of a Steam game, if any: (library, manifest).
pub fn installed(steam_root: &Path, appid: u32) -> Option<(PathBuf, Manifest)> {
    libraries(steam_root).into_iter().find_map(|lib| {
        let m = read_manifest(&lib.join(format!("steamapps/appmanifest_{appid}.acf")))?;
        Some((lib, m))
    })
}

/// The config folder of the Steam user who logged in last.
pub fn user_config_dir(steam_root: &Path) -> Result<PathBuf> {
    let ud = steam_root.join("userdata");
    let best = fs::read_dir(&ud)
        .with_context(|| format!("no Steam users in {}; start Steam once and log in", ud.display()))?
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.chars().all(|c| c.is_ascii_digit()) && n != "0"
        })
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .context("no Steam user found; start Steam once and log in")?;
    let p = best.path().join("config");
    fs::create_dir_all(&p)?;
    Ok(p)
}

/// Steam's id for a non-Steam shortcut.
pub fn shortcut_appid(exe: &str, name: &str) -> u32 {
    crc32fast::hash(format!("{exe}{name}").as_bytes()) | 0x8000_0000
}

/// `steam://rungameid/<id>` starts a shortcut in a running Steam.
pub fn run_url(appid: u32) -> String {
    format!("steam://rungameid/{}", ((appid as u64) << 32) | 0x0200_0000)
}

// ---------------------------------------------------------------- shortcuts

/// A library entry Steam-Fan wants to exist.
#[derive(Debug, Clone, PartialEq)]
pub struct Wanted {
    /// The game id, written into the launch options so the entry can be found again.
    pub id: String,
    pub name: String,
    pub exe: String,
    pub start_dir: String,
    pub launch_options: String,
    pub tag: String,
    pub icon: String,
    /// Run through Proton.
    pub proton: bool,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SyncReport {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub removed: Vec<String>,
    pub proton: String,
}

impl SyncReport {
    pub fn summary(&self) -> String {
        format!("{} added, {} updated, {} removed", self.added.len(), self.updated.len(), self.removed.len())
    }
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.updated.is_empty() && self.removed.is_empty()
    }
}

/// Find the game id in an entry's launch options, also for entries made by the old scripts.
pub fn id_from_launch(opts: &str) -> Option<String> {
    if opts.contains(" launch ") {
        // "<steam-fan>" launch "<kind>:<path>" ...
        if let Some(id) = first_quoted_after(opts, " launch") {
            if ["fan:", "nonsteam:", "rom:"].iter().any(|k| id.starts_with(k)) {
                return Some(id);
            }
        }
    }
    if opts.contains("fan-fetch") {
        // made by the old fan-sync script: "fan-fetch" "<rel>" %command%
        return first_quoted_after(opts, "fan-fetch\"")
            .or_else(|| first_quoted_after(opts, "fan-fetch"))
            .map(|r| format!("fan:{r}"));
    }
    None
}

/// The text of the first `"..."` after `marker`, with `\"` unescaped.
fn first_quoted_after(opts: &str, marker: &str) -> Option<String> {
    let i = opts.find(marker)? + marker.len();
    let start = opts[i..].find('"')? + i + 1;
    let mut out = String::new();
    let mut chars = opts[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?),
            '"' => return Some(out),
            c => out.push(c),
        }
    }
    None
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\\\""))
}

/// Launch options that send Play through Steam-Fan.
pub fn launch_options(bin: &Path, id: &str, wraps_command: bool) -> String {
    let base = format!("{} launch {}", quote(&bin.to_string_lossy()), quote(id));
    if wraps_command {
        format!("{base} %command%")
    } else {
        base
    }
}

fn backup(path: &Path, stamp: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let orig = PathBuf::from(format!("{}.bak-original", path.display()));
    if !orig.exists() {
        fs::copy(path, &orig)?;
    }
    let mut dest = PathBuf::from(format!("{}.bak-{stamp}", path.display()));
    let mut n = 2;
    while dest.exists() {
        dest = PathBuf::from(format!("{}.bak-{stamp}-{n}", path.display()));
        n += 1;
    }
    fs::copy(path, dest)?;
    Ok(())
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    fs::write(&tmp, data)?;
    fs::rename(tmp, path)?;
    Ok(())
}

/// Bring Steam's library in line with `wanted`.
///
/// Only entries Steam-Fan made (or the old scripts made) are changed. With
/// `prune`, those whose game is no longer wanted are removed; pass `false`
/// unless `wanted` is the complete list from a full shelf read.
pub fn sync_shortcuts(cfg: &Config, wanted: &[Wanted], prune: bool, dry_run: bool) -> Result<SyncReport> {
    sync_shortcuts_renamed(cfg, wanted, &std::collections::HashMap::new(), prune, dry_run)
}

/// Like `sync_shortcuts`, but entries for games that were moved or renamed
/// (old id -> new id) are kept and updated, so Steam keeps their play time and artwork.
pub fn sync_shortcuts_renamed(
    cfg: &Config,
    wanted: &[Wanted],
    renames: &std::collections::HashMap<String, String>,
    prune: bool,
    dry_run: bool,
) -> Result<SyncReport> {
    if !dry_run && running() {
        bail!("Steam is running. Quit Steam (Steam > Exit) first, then try again.");
    }
    let root = cfg.steam_root();
    let cfgdir = user_config_dir(&root)?;
    let sc_path = cfgdir.join("shortcuts.vdf");
    let cv_path = root.join("config/config.vdf");

    let mut shortcuts = match fs::read(&sc_path) {
        Ok(b) if !b.is_empty() => vdf::binary_load(&b).context("cannot read shortcuts.vdf")?,
        _ => BMap::default(),
    };
    let mut config = match fs::read_to_string(&cv_path) {
        Ok(s) => vdf::text_load(&s).context("cannot read config.vdf")?,
        Err(_) => TBlock::default(),
    };

    let compat = config.path_mut(&["InstallConfigStore", "Software", "Valve", "Steam", "CompatToolMapping"]);
    let tool = if !cfg.proton.is_empty() {
        cfg.proton.clone()
    } else {
        compat.block("0").and_then(|b| b.str("name")).filter(|s| !s.is_empty()).unwrap_or("proton_experimental").to_string()
    };

    let entries: Vec<BMap> = shortcuts
        .map("shortcuts")
        .map(|m| m.0.iter().filter_map(|(_, v)| if let BValue::Map(e) = v { Some(e.clone()) } else { None }).collect())
        .unwrap_or_default();
    let mut entries = entries;
    let mut report = SyncReport { proton: tool.clone(), ..Default::default() };

    let ours = |e: &BMap| id_from_launch(e.str("LaunchOptions")).map(|id| renames.get(&id).cloned().unwrap_or(id));
    for w in wanted {
        let idx = entries.iter().position(|e| ours(e).as_deref() == Some(w.id.as_str()));
        let fields = [
            ("AppName", w.name.as_str()),
            ("Exe", &quote(&w.exe)),
            ("StartDir", &quote(&w.start_dir)),
            ("LaunchOptions", w.launch_options.as_str()),
        ];
        let e = match idx {
            Some(i) => {
                let e = &mut entries[i];
                let mut changed = false;
                for (k, v) in fields {
                    if e.str(k) != v {
                        e.set(k, BValue::Str(v.to_string()));
                        changed = true;
                    }
                }
                if !w.icon.is_empty() && e.str("icon") != w.icon {
                    e.set("icon", BValue::Str(w.icon.clone()));
                    changed = true;
                }
                if changed {
                    report.updated.push(w.id.clone());
                }
                e
            }
            None => {
                let exe_q = quote(&w.exe);
                let appid = shortcut_appid(&exe_q, &w.name);
                let mut e = BMap::default();
                e.set("appid", BValue::I32(appid as i32));
                for (k, v) in fields {
                    e.set(k, BValue::Str(v.to_string()));
                }
                e.set("icon", BValue::Str(w.icon.clone()));
                e.set("ShortcutPath", BValue::Str(String::new()));
                for k in ["IsHidden", "OpenVR", "Devkit", "DevkitOverrideAppID", "LastPlayTime"] {
                    e.set(k, BValue::I32(0));
                }
                e.set("AllowDesktopConfig", BValue::I32(1));
                e.set("AllowOverlay", BValue::I32(1));
                e.set("DevkitGameID", BValue::Str(String::new()));
                e.set("FlatpakAppID", BValue::Str(String::new()));
                e.map_mut("tags").set("0", BValue::Str(w.tag.clone()));
                entries.push(e);
                report.added.push(w.id.clone());
                entries.last_mut().unwrap()
            }
        };
        let key = (e.i32("appid").unwrap_or(0) as u32).to_string();
        compat.remove(&key);
        if w.proton {
            let b = compat.block_mut(&key);
            b.0 = vec![
                ("name".into(), TValue::Str(tool.clone())),
                ("config".into(), TValue::Str(String::new())),
                ("priority".into(), TValue::Str("250".into())),
            ];
        }
    }

    if prune {
        let keep: std::collections::HashSet<&str> = wanted.iter().map(|w| w.id.as_str()).collect();
        entries.retain(|e| match ours(e) {
            Some(id) if !keep.contains(id.as_str()) => {
                compat.remove(&(e.i32("appid").unwrap_or(0) as u32).to_string());
                report.removed.push(id);
                false
            }
            _ => true,
        });
    }

    if dry_run || report.is_empty() {
        return Ok(report);
    }
    let stamp = crate::now_stamp();
    backup(&sc_path, &stamp)?;
    backup(&cv_path, &stamp)?;
    let list = shortcuts.map_mut("shortcuts");
    list.0 = entries.into_iter().enumerate().map(|(i, e)| (i.to_string(), BValue::Map(e))).collect();
    write_atomic(&sc_path, &vdf::binary_dump(&shortcuts))?;
    write_atomic(&cv_path, vdf::text_dump(&config).as_bytes())?;
    Ok(report)
}

/// The shortcut ids Steam-Fan made, by game id (to start a game from the app).
pub fn shortcut_ids(cfg: &Config) -> std::collections::HashMap<String, u32> {
    let mut out = std::collections::HashMap::new();
    let Ok(dir) = user_config_dir(&cfg.steam_root()) else { return out };
    let Ok(b) = fs::read(dir.join("shortcuts.vdf")) else { return out };
    let Ok(sc) = vdf::binary_load(&b) else { return out };
    if let Some(list) = sc.map("shortcuts") {
        for (_, v) in &list.0 {
            if let BValue::Map(e) = v {
                if let Some(id) = id_from_launch(e.str("LaunchOptions")) {
                    out.insert(id, e.i32("appid").unwrap_or(0) as u32);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_from_launch_options() {
        let bin = Path::new("/home/zain/.local/bin/steam-fan");
        let o = launch_options(bin, "fan:Super Mario Bros/Super \"63\"", true);
        assert_eq!(id_from_launch(&o).unwrap(), "fan:Super Mario Bros/Super \"63\"");
        let old = r#""/home/zain/.local/bin/fan-fetch" "Super Mario Bros/Super Mario 63" %command%"#;
        assert_eq!(id_from_launch(old).unwrap(), "fan:Super Mario Bros/Super Mario 63");
        assert_eq!(id_from_launch("-novid"), None);
    }

    #[test]
    fn appid_matches_python() {
        // value from the old fan-sync (crc32 of exe+name | 0x80000000)
        let exe = "\"/x/a.exe\"";
        assert_eq!(shortcut_appid(exe, "A"), crc32fast::hash(b"\"/x/a.exe\"A") | 0x8000_0000);
        assert_eq!(run_url(0x8000_0001), format!("steam://rungameid/{}", (0x8000_0001u64 << 32) | 0x0200_0000));
    }
}
