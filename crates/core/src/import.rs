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

/// Remove what `unpack` left behind.
pub fn clean_temp() {
    let _ = fs::remove_dir_all(temp_root());
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
        keep_zip_dates(&mut z, &dest);
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

/// Files keep the dates stored in the zip (bsdtar and 7z do this themselves): a version's
/// program date tells which version is newest.
fn keep_zip_dates(z: &mut zip::ZipArchive<fs::File>, dest: &Path) {
    for i in 0..z.len() {
        let Ok(f) = z.by_index_raw(i) else { continue };
        if f.is_dir() {
            continue;
        }
        let (Some(rel), Some(d)) = (f.enclosed_name(), f.last_modified()) else { continue };
        let Some(t) = zip_time(d.year(), d.month(), d.day(), d.hour(), d.minute(), d.second()) else { continue };
        if let Ok(file) = fs::File::options().write(true).open(dest.join(rel)) {
            let _ = file.set_modified(t);
        }
    }
}

/// A zip date (no time zone; taken as UTC) as a point in time.
fn zip_time(y: u16, mo: u8, d: u8, h: u8, mi: u8, s: u8) -> Option<std::time::SystemTime> {
    if !(1980..2200).contains(&y) || !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    // days since 1970-01-01 (Howard Hinnant's days_from_civil)
    let (y, m) = (y as i64 - if mo <= 2 { 1 } else { 0 }, mo as i64);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let secs = days * 86400 + h as i64 * 3600 + mi as i64 * 60 + s as i64;
    Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs.max(0) as u64))
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

/// A folder about to hold versions must not be a game itself (programs right inside).
pub fn check_version_parent(parent: &Path, except: Option<&Path>) -> Result<()> {
    if except == Some(parent) {
        return Ok(()); // the game being turned into one of its versions
    }
    if let Ok(items) = shelf::listing(parent) {
        if !shelf::holds_versions(&items) && items.iter().any(|e| e.is_file && shelf::is_program(&e.name)) {
            bail!(
                "{} is already a single game. Open it and use Move / Rename to give it a version first, then add this one beside it.",
                parent.display()
            );
        }
    }
    Ok(())
}

/// Where a new game would go on the shelf. With a version: `<group>/<name>/<version>`.
pub fn destination(cfg: &Config, kind: Kind, group: &str, name: &str, version: &str) -> Result<PathBuf> {
    let clean = |s: &str| shelf::safe_name(&s.replace('/', "-"));
    // the group may have a subgroup: "Sonic The Hedgehog/Classic"
    let group = group.split('/').map(shelf::safe_name).filter(|p| !p.is_empty() && p != "." && p != "..").collect::<Vec<_>>().join("/");
    let (name, version) = (clean(name), clean(version));
    if group.is_empty() || name.is_empty() {
        bail!("give the game a name and a group");
    }
    Ok(match kind {
        Kind::Fan | Kind::NonSteam if !version.is_empty() => cfg.shelf().join(kind.folder()).join(group).join(name).join(version),
        Kind::Fan | Kind::NonSteam => cfg.shelf().join(kind.folder()).join(group).join(name),
        Kind::Rom => shelf::rom_games_root(&cfg.shelf().join("roms").join(&group), cfg.emulator(&group)).join(name),
        Kind::Steam => bail!("Steam games are archived by the NAS"),
    })
}

/// Put a game on the shelf: unpack if needed, then copy. Returns where it went, and
/// the checksums of what was copied (taken from the unpacked files on this PC).
pub fn place(
    cfg: &Config,
    source: &Path,
    kind: Kind,
    group: &str,
    name: &str,
    version: &str,
    cancel: &Cancel,
    status: &mut dyn FnMut(&str),
    cb: &mut dyn FnMut(&Progress),
) -> Result<(PathBuf, crate::integrity::Manifest)> {
    if !cfg.shelf_online() {
        bail!("the shelf isn't reachable");
    }
    let dst = destination(cfg, kind, group, name, version)?;
    let versioned = kind != Kind::Rom && !version.trim().is_empty();
    if versioned {
        check_version_parent(dst.parent().unwrap(), None)?;
    }
    if dst.exists() && shelf::listing(&dst).map_or(false, |l| !l.is_empty()) {
        bail!("{} is already on the shelf; pick another name", dst.display());
    }
    if versioned {
        fs::create_dir_all(dst.parent().unwrap())?;
        fs::write(dst.parent().unwrap().join(shelf::VERSIONS_MARKER), "")?;
    }
    let m = place_at(source, &dst, cancel, status, cb)?;
    Ok((dst, m))
}

/// Unpack (if needed) and copy `source` into the folder `dst`, with checksums of what was copied.
pub fn place_at(
    source: &Path,
    dst: &Path,
    cancel: &Cancel,
    status: &mut dyn FnMut(&str),
    cb: &mut dyn FnMut(&Progress),
) -> Result<crate::integrity::Manifest> {
    let (root, temp) = if is_archive(source) {
        status("unpacking");
        let r = unpack(source)?;
        (r, true)
    } else {
        (source.to_path_buf(), false)
    };
    let res = (|| {
        status("checksums");
        let m = crate::integrity::hash_tree(&root, cancel, cb)?;
        status("copying to the shelf");
        fs::create_dir_all(dst)?;
        transfer::copy_tree(&root, dst, &|_| false, cancel, cb)?;
        Ok(m)
    })();
    if temp {
        let _ = fs::remove_dir_all(temp_root());
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn zip_import() {
        let _env = crate::env_lock();
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
        let (dst, m) = place(&cfg, &zp, Kind::Fan, "Sonic The Hedgehog", &i.name, "", &Cancel::default(), &mut |_| {}, &mut |_| {}).unwrap();
        assert_eq!(m.len(), 2);
        assert!(dst.join("srb2win.exe").exists());
        assert!(dst.join("data/main.pk3").exists());
        assert!(!dst.join("__MACOSX").exists());

        // a second version beside the first is refused until the first has a version itself
        assert!(place(&cfg, &zp, Kind::Fan, "Sonic The Hedgehog", &i.name, "v2", &Cancel::default(), &mut |_| {}, &mut |_| {}).is_err());
        let (v, _) = place(&cfg, &zp, Kind::Fan, "Sonic The Hedgehog", "Sonic of the Ring", "V 0.6", &Cancel::default(), &mut |_| {}, &mut |_| {}).unwrap();
        assert!(v.ends_with("Sonic of the Ring/V 0.6") && v.join("srb2win.exe").exists());
        assert!(v.parent().unwrap().join(shelf::VERSIONS_MARKER).exists());

        // a ROM is recognised by its type
        let (k, sys, _) = guess(&cfg, &["Galaxy/game.iso".to_string()]);
        assert_eq!((k, sys.as_str()), (Kind::Rom, "wii"));
    }

    #[test]
    fn zip_dates() {
        let t = zip_time(2025, 12, 15, 10, 30, 0).unwrap();
        assert_eq!(t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(), 1765794600);
        assert!(zip_time(1979, 1, 1, 0, 0, 0).is_none());
    }
}
