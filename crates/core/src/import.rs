//! Adding a new game to the shelf from a downloaded zip (or 7z/rar) or a folder.
//!
//! 1. `inspect` looks inside without unpacking and guesses what it is.
//! 2. `unpack` unpacks into a temporary folder on this PC and drops the usual
//!    clutter: a single extra folder level, `__MACOSX`.
//! 3. `place` copies the result onto the shelf where it belongs.

use crate::config::Config;
use crate::shelf::{self, Kind};
use crate::transfer::{self, Cancel, Progress};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct Inspect {
    pub source: String,
    pub is_archive: bool,
    /// Suggested name: the archive or folder name, tidied.
    pub name: String,
    pub kind: Kind,
    /// Emulator system folder, when it looks like an emulator game.
    pub system: String,
    /// Programs or game files found inside (first few).
    pub found: Vec<String>,
    pub files: usize,
    pub size: u64,
}

const ARCHIVES: [&str; 9] = [".zip", ".7z", ".rar", ".tar", ".tar.gz", ".tgz", ".tar.xz", ".tar.zst", ".tar.bz2"];

pub fn is_archive(p: &Path) -> bool {
    let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    p.is_file() && ARCHIVES.iter().any(|e| n.ends_with(e))
}

fn tidy_name(p: &Path) -> String {
    let mut n = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let low = n.to_lowercase();
    if let Some(e) = ARCHIVES.iter().find(|e| low.ends_with(*e)) {
        n.truncate(n.len() - e.len());
    }
    n.replace('_', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// (path inside, size) of every file in an archive, without unpacking.
fn archive_entries(p: &Path) -> Result<Vec<(String, u64)>> {
    let low = p.to_string_lossy().to_lowercase();
    if low.ends_with(".zip") {
        let mut z = zip::ZipArchive::new(fs::File::open(p)?).context("not a readable zip file")?;
        let mut out = vec![];
        for i in 0..z.len() {
            let f = z.by_index_raw(i)?;
            if f.is_file() {
                out.push((f.name().to_string(), f.size()));
            }
        }
        return Ok(out);
    }
    // everything else through bsdtar (libarchive, on every Arch system) or 7z
    for (prog, args) in [("bsdtar", vec!["-tvf"]), ("7z", vec!["l", "-ba", "-slt"])] {
        let Ok(out) = Command::new(prog).args(&args).arg(p).output() else { continue };
        if !out.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        if prog == "bsdtar" {
            // -rw-r--r--  0 user group  12345 Jan  1  2020 path/inside
            return Ok(text
                .lines()
                .filter(|l| !l.starts_with('d'))
                .filter_map(|l| {
                    let f: Vec<&str> = l.split_whitespace().collect();
                    let size = f.get(4)?.parse().ok()?;
                    let name = f.get(8..)?.join(" ");
                    Some((name, size))
                })
                .collect());
        }
        let mut out = vec![];
        let (mut path, mut size, mut dir) = (String::new(), 0u64, false);
        for l in text.lines().chain(std::iter::once("")) {
            if let Some(v) = l.strip_prefix("Path = ") {
                path = v.to_string();
            } else if let Some(v) = l.strip_prefix("Size = ") {
                size = v.parse().unwrap_or(0);
            } else if let Some(v) = l.strip_prefix("Attributes = ") {
                dir = v.starts_with('D');
            } else if l.is_empty() && !path.is_empty() {
                if !dir {
                    out.push((std::mem::take(&mut path), size));
                }
                path.clear();
                dir = false;
            }
        }
        return Ok(out);
    }
    bail!("can't open this kind of archive: install libarchive (bsdtar) or 7zip")
}

fn folder_entries(p: &Path) -> Vec<(String, u64)> {
    walkdir::WalkDir::new(p)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            let len = e.metadata().map(|m| m.len()).unwrap_or(0);
            (e.path().strip_prefix(p).unwrap().to_string_lossy().into_owned(), len)
        })
        .collect()
}

/// What kind of game the files look like.
pub fn guess(cfg: &Config, names: &[String]) -> (Kind, String, Vec<String>) {
    let ext_of = |n: &str| Path::new(n).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    for emu in &cfg.emulators {
        let exts: Vec<String> = emu.extensions.iter().map(|e| e.to_lowercase()).collect();
        let hits: Vec<String> = names.iter().filter(|n| exts.contains(&ext_of(n))).take(5).cloned().collect();
        if !hits.is_empty() {
            return (Kind::Rom, emu.system.clone(), hits);
        }
    }
    let progs: Vec<String> = names
        .iter()
        .filter(|n| shelf::is_program(Path::new(n).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default().as_str()))
        .take(5)
        .cloned()
        .collect();
    (Kind::Fan, String::new(), progs)
}

pub fn inspect(cfg: &Config, source: &Path) -> Result<Inspect> {
    if !source.exists() {
        bail!("{} doesn't exist", source.display());
    }
    let archive = is_archive(source);
    let entries = if archive {
        archive_entries(source)?
    } else if source.is_dir() {
        folder_entries(source)
    } else {
        vec![(source.file_name().unwrap().to_string_lossy().into_owned(), fs::metadata(source)?.len())]
    };
    let names: Vec<String> = entries.iter().map(|(n, _)| n.clone()).filter(|n| !n.contains("__MACOSX")).collect();
    let (kind, system, found) = guess(cfg, &names);
    Ok(Inspect {
        source: source.to_string_lossy().into_owned(),
        is_archive: archive,
        name: tidy_name(source),
        kind,
        system,
        found,
        files: names.len(),
        size: entries.iter().map(|(_, s)| s).sum(),
    })
}

fn temp_root() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(|| crate::config::home().join(".cache")).join("steam-fan/import")
}

/// Unpack an archive into a fresh temporary folder. Returns the folder holding the game.
pub fn unpack(source: &Path) -> Result<PathBuf> {
    let dest = temp_root().join(format!("{}", std::process::id() as u64 * 1000 + (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.subsec_millis() as u64)));
    let _ = fs::remove_dir_all(&dest);
    fs::create_dir_all(&dest)?;
    let low = source.to_string_lossy().to_lowercase();
    if low.ends_with(".zip") {
        let mut z = zip::ZipArchive::new(fs::File::open(source)?)?;
        z.extract(&dest).context("cannot unpack the zip file")?;
    } else {
        let ok = Command::new("bsdtar").arg("-xf").arg(source).arg("-C").arg(&dest).status().map_or(false, |s| s.success())
            || Command::new("7z").arg("x").arg("-y").arg(format!("-o{}", dest.display())).arg(source).status().map_or(false, |s| s.success())
            || Command::new("unrar").arg("x").arg("-o+").arg(source).arg(format!("{}/", dest.display())).status().map_or(false, |s| s.success());
        if !ok {
            bail!("can't unpack this archive: install libarchive (bsdtar) or 7zip");
        }
    }
    let _ = fs::remove_dir_all(dest.join("__MACOSX"));
    Ok(game_root(&dest))
}

/// If a folder holds nothing but one folder, the game is in that one.
fn game_root(dir: &Path) -> PathBuf {
    let mut cur = dir.to_path_buf();
    loop {
        let items: Vec<_> = shelf::listing(&cur).unwrap_or_default();
        match items.as_slice() {
            [only] if only.is_dir => cur = cur.join(&only.name),
            _ => return cur,
        }
    }
}

/// Where a new game would go on the shelf.
pub fn destination(cfg: &Config, kind: Kind, group: &str, name: &str) -> Result<PathBuf> {
    let clean = |s: &str| s.replace('/', "-").trim().to_string();
    let (group, name) = (clean(group), clean(name));
    if group.is_empty() || name.is_empty() {
        bail!("give the game a name and a group");
    }
    Ok(match kind {
        Kind::Fan | Kind::NonSteam => cfg.shelf().join(kind.folder()).join(group).join(name),
        Kind::Rom => shelf::rom_games_root(&cfg.shelf().join("roms").join(&group), cfg.emulator(&group)).join(name),
        Kind::Steam => bail!("Steam games are archived by the NAS"),
    })
}

/// Put a game on the shelf: unpack if needed, then copy. Returns where it went.
pub fn place(
    cfg: &Config,
    source: &Path,
    kind: Kind,
    group: &str,
    name: &str,
    cancel: &Cancel,
    status: &mut dyn FnMut(&str),
    cb: &mut dyn FnMut(&Progress),
) -> Result<PathBuf> {
    if !cfg.shelf_online() {
        bail!("the shelf isn't reachable");
    }
    let dst = destination(cfg, kind, group, name)?;
    if dst.exists() && shelf::listing(&dst).map_or(false, |l| !l.is_empty()) {
        bail!("{} is already on the shelf; pick another name", dst.display());
    }
    let (root, temp) = if is_archive(source) {
        status("unpacking");
        let r = unpack(source)?;
        (r, true)
    } else {
        (source.to_path_buf(), false)
    };
    status("copying to the shelf");
    fs::create_dir_all(&dst)?;
    let res = transfer::copy_tree(&root, &dst, &|_| false, cancel, cb);
    if temp {
        let _ = fs::remove_dir_all(temp_root());
    }
    res?;
    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn zip_import() {
        let t = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CACHE_HOME", t.path().join("cache"));
        let shelf = t.path().join("shelf");
        fs::create_dir_all(shelf.join("fangames")).unwrap();
        fs::write(shelf.join("fangames/.keep"), "").unwrap();
        let cfg = Config { shelf: shelf.to_string_lossy().into(), ..Default::default() };
        // zip with an extra folder level and mac clutter
        let zp = t.path().join("Sonic_Robo_Blast_2.zip");
        let mut z = zip::ZipWriter::new(fs::File::create(&zp).unwrap());
        let o: zip::write::SimpleFileOptions = Default::default();
        z.start_file("SRB2/srb2win.exe", o).unwrap();
        z.write_all(b"MZ").unwrap();
        z.start_file("SRB2/data/main.pk3", o).unwrap();
        z.write_all(b"data").unwrap();
        z.start_file("__MACOSX/._x", o).unwrap();
        z.finish().unwrap();
        let i = inspect(&cfg, &zp).unwrap();
        assert_eq!(i.name, "Sonic Robo Blast 2");
        assert_eq!(i.kind, Kind::Fan);
        assert_eq!(i.found, ["SRB2/srb2win.exe"]);
        let dst = place(&cfg, &zp, Kind::Fan, "Sonic The Hedgehog", &i.name, &Cancel::default(), &mut |_| {}, &mut |_| {}).unwrap();
        assert!(dst.join("srb2win.exe").exists());
        assert!(dst.join("data/main.pk3").exists());
        assert!(!dst.join("__MACOSX").exists());

        // a ROM is recognised by its type
        let (k, sys, _) = guess(&cfg, &["Galaxy/game.iso".to_string()]);
        assert_eq!((k, sys.as_str()), (Kind::Rom, "wii"));
    }
}
