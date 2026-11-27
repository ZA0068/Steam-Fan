//! Checksums, like Steam's "Verify integrity of game files".
//!
//! When a game goes onto the shelf, a SHA-256 fingerprint of every file is kept
//! in `<shelf>/artwork/<game>/checksums.sha256`. **Verify** compares the shelf
//! copy and the copy on this PC against it, the way Steam does:
//! * **the PC copy** is checked and any damaged or missing file is downloaded again from the
//!   shelf (Steam: from its content servers). Files the game rewrites (settings, saves) are kept;
//! * **the shelf** changes only when a game is added (zip, rar, folder, a link) or downloaded
//!   again. The server checks it too, and patches damaged files from the game's download link.
//!   A PC never writes game files to the shelf.

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
    crate::artwork::version_dir(cfg, g).join(FILE)
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

/// How damaged shelf files were patched from the game's download link.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ShelfRepair {
    pub from_link: Vec<String>,
    /// Still bad: the download didn't have them as they were.
    pub unfixed: Vec<String>,
    /// Why some couldn't be patched.
    pub note: String,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Report {
    pub recorded: bool,
    /// The shelf copy, when the shelf itself was checked (server side, or no copy on this PC).
    pub shelf: Option<Check>,
    pub shelf_repair: Option<ShelfRepair>,
    /// The copy on this PC.
    pub pc: Option<Check>,
    /// PC files downloaded again from the shelf.
    pub repaired: Vec<String>,
    /// PC files that couldn't be repaired: the shelf's copy of them is damaged too.
    pub pc_unfixed: Vec<String>,
    /// What Steam-Fan-Server reported when it checked its own copy.
    pub server_summary: Option<String>,
}

impl Report {
    /// Everything checked is good now.
    pub fn ok(&self) -> bool {
        let shelf = match (&self.shelf, &self.shelf_repair) {
            (Some(s), _) if s.ok() => true,
            (Some(_), Some(r)) => r.unfixed.is_empty(),
            (Some(_), None) => false,
            (None, _) => true,
        };
        shelf && self.pc_unfixed.is_empty()
    }

    pub fn summary(&self) -> String {
        if let Some(s) = &self.server_summary {
            return format!("on the server: {s}");
        }
        let mut parts = vec![];
        if self.recorded {
            parts.push("checksums recorded from the shelf copy".to_string());
        }
        if let Some(s) = &self.shelf {
            let r = self.shelf_repair.clone().unwrap_or_default();
            parts.push(if s.ok() {
                format!("shelf copy OK ({} files)", s.checked)
            } else if r.unfixed.is_empty() {
                format!("shelf copy patched: {} damaged or missing files downloaded again from its link", r.from_link.len())
            } else {
                let mut t = format!("shelf copy DAMAGED: {} files", r.unfixed.len());
                if !r.from_link.is_empty() {
                    t.push_str(&format!(" ({} others were patched from its link)", r.from_link.len()));
                }
                if !r.note.is_empty() {
                    t.push_str(&format!(": {}", r.note));
                }
                t
            });
        }
        if let Some(c) = &self.pc {
            let kept = if c.changed_by_game.is_empty() { String::new() } else { format!("; {} settings/save files changed by the game were kept", c.changed_by_game.len()) };
            parts.push(if c.ok() {
                format!("PC copy OK ({} files{kept})", c.checked)
            } else if self.pc_unfixed.is_empty() {
                format!("PC copy repaired: {} files downloaded again from the shelf{kept}", self.repaired.len())
            } else {
                format!(
                    "PC copy: {} files repaired, {} couldn't be: the shelf's copy of them is damaged too. Re-download the game (that fixes the shelf), then Verify again",
                    self.repaired.len(),
                    self.pc_unfixed.len()
                )
            });
        }
        parts.join("; ")
    }
}

/// Does this file have exactly the size and fingerprint recorded?
fn is_original(p: &Path, size: u64, hex: &str, cancel: &Cancel) -> bool {
    fs::metadata(p).map_or(false, |m| m.is_file() && m.len() == size) && hash_file(p, cancel, &mut |_| {}).map_or(false, |h| h == hex)
}

/// Replace one file without leaving a half-written one behind.
fn put_file(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst.parent().unwrap())?;
    let tmp = dst.with_file_name(format!(".{}.steam-fan-part", dst.file_name().unwrap().to_string_lossy()));
    fs::copy(src, &tmp)?;
    fs::rename(&tmp, dst)?;
    Ok(())
}

/// Copy from `from` the files in `left` that match their fingerprint into `to`; returns the ones copied.
fn copy_originals(from: &Path, to: &Path, m: &Manifest, left: &mut Vec<String>, cancel: &Cancel) -> Vec<String> {
    let mut took = vec![];
    left.retain(|rel| {
        let Some((size, hex)) = m.get(rel) else { return true };
        let src = from.join(rel);
        if is_original(&src, *size, hex, cancel) && put_file(&src, &to.join(rel)).is_ok() {
            took.push(rel.clone());
            false
        } else {
            true
        }
    });
    took
}

/// The checksums, recorded from the shelf copy (taken as good) the first time.
fn manifest_for(cfg: &Config, g: &Game, rep: &mut Report, cancel: &Cancel, status: &mut dyn FnMut(&str), cb: &mut dyn FnMut(&Progress)) -> Result<Manifest> {
    let mpath = manifest_path(cfg, g);
    if let Some(m) = read(&mpath) {
        return Ok(m);
    }
    status("recording checksums of the shelf copy");
    let m = hash_tree(&g.shelf_path(cfg), cancel, cb)?;
    write(&mpath, &m)?;
    rep.recorded = true;
    Ok(m)
}

/// Like Steam's "Verify integrity of game files": check the copy on this PC and download
/// any damaged or missing file again from the shelf. Files the game rewrites itself
/// (settings, saves) are kept. The shelf is never changed by this.
pub fn verify_pc(cfg: &Config, g: &Game, cancel: &Cancel, status: &mut dyn FnMut(&str), cb: &mut dyn FnMut(&Progress)) -> Result<Report> {
    if !cfg.shelf_online() {
        bail!("the shelf isn't reachable");
    }
    let mut rep = Report::default();
    let m = manifest_for(cfg, g, &mut rep, cancel, status, cb)?;
    if rep.recorded {
        // just recorded from the shelf: nothing on the shelf to compare against yet
        rep.shelf = Some(Check { checked: m.len(), ..Default::default() });
    }
    let local = g.local_path(cfg);
    status("checking the copy on this PC");
    let c = check(&m, &local, true, cancel, cb)?;
    if !c.ok() {
        status("downloading damaged files again from the shelf");
        let mut bad: Vec<String> = c.missing.iter().chain(&c.damaged).cloned().collect();
        // only shelf files that still match their fingerprint are copied
        rep.repaired = copy_originals(&base(&g.shelf_path(cfg)), &local, &m, &mut bad, cancel);
        rep.pc_unfixed = bad;
    }
    rep.pc = Some(c);
    Ok(rep)
}

/// Check the shelf copy, and patch damaged or missing files from the game's download link
/// (only those files, and only with files that match their fingerprint). This is the
/// server's own check; the PCs never write game files to the shelf.
pub fn verify_shelf(cfg: &Config, g: &Game, cancel: &Cancel, status: &mut dyn FnMut(&str), cb: &mut dyn FnMut(&Progress)) -> Result<Report> {
    if !cfg.shelf_online() {
        bail!("the shelf isn't reachable");
    }
    let mut rep = Report::default();
    let m = manifest_for(cfg, g, &mut rep, cancel, status, cb)?;
    let shelf = g.shelf_path(cfg);
    status("checking the shelf copy");
    let s = check(&m, &shelf, false, cancel, cb)?;
    if !s.ok() {
        let mut left: Vec<String> = s.missing.iter().chain(&s.damaged).cloned().collect();
        let mut r = ShelfRepair::default();
        let url = crate::artwork::info(g).download.trim().to_string();
        if url.is_empty() {
            r.note = "no download link to patch it from: Re-download it from a file, or add the link under About > Edit".into();
        } else {
            status("downloading to patch the shelf");
            let got = crate::source::fetch(&url, cancel, cb).and_then(|f| {
                if crate::import::is_archive(&f) {
                    status("unpacking");
                    crate::import::unpack(&f)
                } else {
                    Ok(f.parent().unwrap().to_path_buf())
                }
            });
            match got {
                Ok(root) => {
                    status("patching the shelf");
                    r.from_link = copy_originals(&root, &base(&shelf), &m, &mut left, cancel);
                    if !left.is_empty() {
                        r.note = "the download has different files now (a new version?): use Re-download".into();
                    }
                }
                Err(e) => r.note = format!("the download failed: {e:#}"),
            }
            crate::source::downloads_dir_clean();
            crate::import::clean_temp();
        }
        r.unfixed = left;
        rep.shelf_repair = Some(r);
    }
    rep.shelf = Some(s);
    Ok(rep)
}

/// Verify from this PC: its own copy if the game is on it, otherwise the shelf copy.
pub fn verify(cfg: &Config, g: &Game, cancel: &Cancel, status: &mut dyn FnMut(&str), cb: &mut dyn FnMut(&Progress)) -> Result<Report> {
    if cfg.server_mode() {
        return crate::remote::verify(cfg, g, cancel, status, cb);
    }
    if g.kind != crate::shelf::Kind::Steam && crate::library::is_installed(cfg, g) {
        verify_pc(cfg, g, cancel, status, cb)
    } else {
        verify_shelf(cfg, g, cancel, status, cb)
    }
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

    #[test]
    fn verify_repairs_the_pc_copy_and_never_the_shelf() {
        let _env = crate::env_lock();
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        std::env::set_var("XDG_DATA_HOME", r.join("data"));
        let s = |p: &str| r.join(p).to_string_lossy().into_owned();
        let cfg = Config { shelf: s("shelf"), fan_local: s("pc"), ..Default::default() };
        let shelf = r.join("shelf/fangames/Sonic/SQC");
        let pc = r.join("pc/Sonic/SQC");
        for d in [&shelf, &pc] {
            fs::create_dir_all(d.join("data")).unwrap();
            fs::write(d.join("SQC.exe"), b"MZ the game").unwrap();
            fs::write(d.join("data/level.bin"), b"level one").unwrap();
            fs::write(d.join("options.ini"), b"vol=5").unwrap();
        }
        fs::write(pc.join(crate::saves::MARKER), b"").unwrap();
        let mut g: Game = serde_json::from_str(r#"{"id":"fan:Sonic/SQC","kind":"Fan","rel":"Sonic/SQC","system":"","name":"SQC","group":"Sonic","program":"SQC.exe","platform":"Windows","appid":0,"buildid":"","installdir":"","size":0}"#).unwrap();
        g.version = String::new();
        let none = Cancel::default();
        assert!(verify(&cfg, &g, &none, &mut |_| {}, &mut |_| {}).unwrap().recorded);
        // the PC copy breaks; the game changed its settings
        fs::write(pc.join("SQC.exe"), b"MZ broken!!").unwrap();
        fs::remove_file(pc.join("data/level.bin")).unwrap();
        fs::write(pc.join("options.ini"), b"vol=9").unwrap();
        let rep = verify(&cfg, &g, &none, &mut |_| {}, &mut |_| {}).unwrap();
        assert!(rep.ok(), "{}", rep.summary());
        assert_eq!(rep.repaired, ["data/level.bin", "SQC.exe"]);
        assert_eq!(fs::read(pc.join("SQC.exe")).unwrap(), b"MZ the game");
        assert_eq!(fs::read(pc.join("options.ini")).unwrap(), b"vol=9", "the game's own settings are kept");
        assert!(rep.summary().contains("PC copy repaired"), "{}", rep.summary());
        // the shelf's file is damaged too: the PC isn't given a bad file, and the shelf isn't touched
        fs::write(shelf.join("SQC.exe"), b"MZ shelf bad").unwrap();
        fs::write(pc.join("SQC.exe"), b"MZ broken!!").unwrap();
        let rep = verify(&cfg, &g, &none, &mut |_| {}, &mut |_| {}).unwrap();
        assert!(!rep.ok());
        assert_eq!(rep.pc_unfixed, ["SQC.exe"]);
        assert_eq!(fs::read(pc.join("SQC.exe")).unwrap(), b"MZ broken!!");
        assert_eq!(fs::read(shelf.join("SQC.exe")).unwrap(), b"MZ shelf bad");
        assert!(rep.summary().contains("Re-download"), "{}", rep.summary());
        // the shelf check (the server's) reports it; with no link there's nothing to patch from
        let rep = verify_shelf(&cfg, &g, &none, &mut |_| {}, &mut |_| {}).unwrap();
        assert!(!rep.ok());
        assert_eq!(rep.shelf_repair.as_ref().unwrap().unfixed, ["SQC.exe"]);
    }
}
