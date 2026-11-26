//! Checksums, like Steam's "Verify integrity of game files".
//!
//! When a game goes onto the shelf, a SHA-256 fingerprint of every file is kept
//! in `<shelf>/artwork/<game>/checksums.sha256`. **Verify** compares the shelf
//! copy and the copy on this PC against it:
//! * a damaged or missing file on this PC is copied again from the shelf;
//! * a file the game itself rewrites (settings, saves) is left alone;
//! * a damaged shelf copy can't be fixed from here: re-download it from its source.

use crate::config::Config;
use crate::shelf::Game;
use crate::transfer::{Cancel, Progress};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const FILE: &str = "checksums.sha256";

/// path inside the game -> (size, sha256 hex)
pub type Manifest = BTreeMap<String, (u64, String)>;

fn ours(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.starts_with(".steam-fan") || name == ".nas-complete" || name == ".nas-prefix" || rel.contains("@eaDir") || name.ends_with(".steam-fan-part")
}

/// Files a game commonly rewrites itself: settings and saves. A difference there is not damage.
pub fn rewritten_by_game(rel: &str) -> bool {
    let low = rel.to_lowercase();
    let ext = low.rsplit('.').next().unwrap_or("");
    matches!(ext, "ini" | "cfg" | "conf" | "sav" | "save" | "json" | "txt" | "log" | "xml" | "dat" | "sol" | "bak")
        || low.split('/').any(|p| p.contains("save") || p == "config" || p == "settings")
}

pub fn manifest_path(cfg: &Config, g: &Game) -> PathBuf {
    crate::artwork::shelf_dir(cfg, g).join(FILE)
}

pub fn hash_file(p: &Path, cancel: &Cancel, done: &mut dyn FnMut(u64)) -> Result<String> {
    let mut f = fs::File::open(p).with_context(|| format!("cannot read {}", p.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.is_cancelled() {
            bail!("stopped");
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        done(n as u64);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn files_of(root: &Path) -> Vec<(String, u64)> {
    if root.is_file() {
        let n = root.file_name().unwrap().to_string_lossy().into_owned();
        return vec![(n, fs::metadata(root).map(|m| m.len()).unwrap_or(0))];
    }
    walkdir::WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let rel = e.path().strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/");
            (!ours(&rel)).then(|| (rel, e.metadata().map(|m| m.len()).unwrap_or(0)))
        })
        .collect()
}

fn base(root: &Path) -> PathBuf {
    if root.is_file() { root.parent().unwrap().to_path_buf() } else { root.to_path_buf() }
}

/// Fingerprint every file under `root` (a game folder, or a single program file).
pub fn hash_tree(root: &Path, cancel: &Cancel, cb: &mut dyn FnMut(&Progress)) -> Result<Manifest> {
    let files = files_of(root);
    let mut p = Progress { total_bytes: files.iter().map(|f| f.1).sum(), total_files: files.len() as u64, ..Default::default() };
    let mut out = Manifest::new();
    let b = base(root);
    for (rel, size) in files {
        p.current = rel.clone();
        let mut last = std::time::Instant::now();
        let hex = hash_file(&b.join(&rel), cancel, &mut |n| {
            p.done_bytes += n;
            if last.elapsed().as_millis() > 150 {
                cb(&p);
                last = std::time::Instant::now();
            }
        })?;
        p.done_files += 1;
        cb(&p);
        out.insert(rel, (size, hex));
    }
    Ok(out)
}

pub fn write(path: &Path, m: &Manifest) -> Result<()> {
    let mut text = String::from("# Steam-Fan checksums: sha256  size  file\n");
    for (rel, (size, hex)) in m {
        text.push_str(&format!("{hex}  {size}  {rel}\n"));
    }
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text)?;
    fs::rename(tmp, path)?;
    Ok(())
}

pub fn read(path: &Path) -> Option<Manifest> {
    let text = fs::read_to_string(path).ok()?;
    let mut m = Manifest::new();
    for l in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let mut it = l.splitn(3, "  ");
        let (hex, size, rel) = (it.next()?, it.next()?, it.next()?);
        m.insert(rel.to_string(), (size.parse().ok()?, hex.to_string()));
    }
    Some(m)
}

/// Record checksums for a game from a copy known to be good (`source`, e.g. the unpacked download).
pub fn record(cfg: &Config, g: &Game, source: &Path, cancel: &Cancel, cb: &mut dyn FnMut(&Progress)) -> Result<usize> {
    let m = hash_tree(source, cancel, cb)?;
    write(&manifest_path(cfg, g), &m)?;
    Ok(m.len())
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Check {
    pub missing: Vec<String>,
    pub damaged: Vec<String>,
    /// Different, but a file the game rewrites itself (settings, saves).
    pub changed_by_game: Vec<String>,
    pub checked: usize,
}

impl Check {
    pub fn ok(&self) -> bool {
        self.missing.is_empty() && self.damaged.is_empty()
    }
}

/// Compare a copy (`root`) with the manifest. Size first, then the full fingerprint.
pub fn check(m: &Manifest, root: &Path, tolerate_game_writes: bool, cancel: &Cancel, cb: &mut dyn FnMut(&Progress)) -> Result<Check> {
    let b = if root.is_file() { root.parent().unwrap().to_path_buf() } else { root.to_path_buf() };
    let mut c = Check::default();
    let mut p = Progress { total_bytes: m.values().map(|v| v.0).sum(), total_files: m.len() as u64, ..Default::default() };
    for (rel, (size, hex)) in m {
        p.current = rel.clone();
        let f = b.join(rel);
        let bad = match fs::metadata(&f) {
            Err(_) => {
                c.missing.push(rel.clone());
                p.done_bytes += size;
                None
            }
            Ok(md) if md.len() != *size => {
                p.done_bytes += size;
                Some(())
            }
            Ok(_) => {
                let mut last = std::time::Instant::now();
                let got = hash_file(&f, cancel, &mut |n| {
                    p.done_bytes += n;
                    if last.elapsed().as_millis() > 150 {
                        cb(&p);
                        last = std::time::Instant::now();
                    }
                })?;
                (got != *hex).then_some(())
            }
        };
        if bad.is_some() {
            if tolerate_game_writes && rewritten_by_game(rel) {
                c.changed_by_game.push(rel.clone());
            } else {
                c.damaged.push(rel.clone());
            }
        }
        c.checked += 1;
        p.done_files += 1;
        cb(&p);
    }
    Ok(c)
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Report {
    pub recorded: bool,
    pub shelf: Option<Check>,
    pub pc: Option<Check>,
    pub repaired: Vec<String>,
}

impl Report {
    pub fn summary(&self) -> String {
        let mut parts = vec![];
        if self.recorded {
            parts.push("checksums recorded from the shelf copy".to_string());
        }
        if let Some(s) = &self.shelf {
            parts.push(if s.ok() {
                format!("shelf copy OK ({} files)", s.checked)
            } else {
                format!("shelf copy DAMAGED: {} missing, {} changed. Re-download it from its source", s.missing.len(), s.damaged.len())
            });
        }
        if let Some(c) = &self.pc {
            parts.push(if c.ok() {
                format!("PC copy OK ({} files)", c.checked)
            } else if !self.repaired.is_empty() {
                format!("PC copy repaired: {} files copied again", self.repaired.len())
            } else {
                format!("PC copy damaged: {} missing, {} changed", c.missing.len(), c.damaged.len())
            });
        }
        parts.join("; ")
    }
}

/// Verify a game: shelf copy, and the PC copy if there is one, repairing the PC copy from a good shelf.
/// With no checksums yet, they are recorded from the shelf copy (taken as good).
pub fn verify(cfg: &Config, g: &Game, cancel: &Cancel, status: &mut dyn FnMut(&str), cb: &mut dyn FnMut(&Progress)) -> Result<Report> {
    if !cfg.shelf_online() {
        bail!("the shelf isn't reachable");
    }
    let mut rep = Report::default();
    let mpath = manifest_path(cfg, g);
    let shelf = g.shelf_path(cfg);
    let m = match read(&mpath) {
        Some(m) => {
            status("checking the shelf copy");
            let s = check(&m, &shelf, false, cancel, cb)?;
            rep.shelf = Some(s);
            m
        }
        None => {
            status("recording checksums of the shelf copy");
            let m = hash_tree(&shelf, cancel, cb)?;
            write(&mpath, &m)?;
            rep.recorded = true;
            m
        }
    };
    let local = g.local_path(cfg);
    if crate::library::is_installed(cfg, g) {
        status("checking the copy on this PC");
        let c = check(&m, &local, true, cancel, cb)?;
        let shelf_ok = rep.shelf.as_ref().map_or(true, |s| s.ok());
        if !c.ok() && shelf_ok {
            status("repairing the copy on this PC");
            let fix: Vec<PathBuf> = c.missing.iter().chain(&c.damaged).map(PathBuf::from).collect();
            crate::transfer::copy_listed(&base(&shelf), &local, &fix)?;
            rep.repaired = fix.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        }
        rep.pc = Some(c);
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_round_trip_and_check() {
        let t = tempfile::tempdir().unwrap();
        let g = t.path().join("game");
        fs::create_dir_all(g.join("data")).unwrap();
        fs::write(g.join("game.exe"), b"MZ program").unwrap();
        fs::write(g.join("data/level.dat"), b"level").unwrap();
        fs::write(g.join("options.ini"), b"vol=5").unwrap();
        let none = Cancel::default();
        let m = hash_tree(&g, &none, &mut |_| {}).unwrap();
        assert_eq!(m.len(), 3);
        write(&t.path().join("c.sha256"), &m).unwrap();
        assert_eq!(read(&t.path().join("c.sha256")).unwrap(), m);
        // damage: a changed program, a removed file, a rewritten settings file
        fs::write(g.join("game.exe"), b"MZ broken!").unwrap();
        fs::remove_file(g.join("data/level.dat")).unwrap();
        fs::write(g.join("options.ini"), b"vol=9").unwrap();
        let c = check(&m, &g, true, &none, &mut |_| {}).unwrap();
        assert_eq!(c.damaged, ["game.exe"]);
        assert_eq!(c.missing, ["data/level.dat"]);
        assert_eq!(c.changed_by_game, ["options.ini"]);
    }
}
