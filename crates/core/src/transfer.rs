//! Copying between shelf and box.
//!
//! Files that are already there with the same size and date are skipped, so an
//! interrupted copy continues where it stopped. Each file is written under a
//! temporary name and renamed when complete, so a half-copied file never looks
//! finished.

use anyhow::{Context, Result};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub done_files: u64,
    pub total_files: u64,
    pub current: String,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            if self.total_files == 0 { 1.0 } else { self.done_files as f32 / self.total_files as f32 }
        } else {
            self.done_bytes as f32 / self.total_bytes as f32
        }
    }
}

/// Lets another thread stop a copy.
#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);
impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed)
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Which files to leave out, by name, anywhere in the tree.
pub type Filter<'a> = &'a dyn Fn(&Path) -> bool;

fn skip_name(name: &str) -> bool {
    matches!(name, "@eaDir" | "#recycle" | "#snapshot" | "__MACOSX" | ".DS_Store" | "Thumbs.db")
}

struct Item {
    rel: PathBuf,
    len: u64,
    link: Option<PathBuf>,
}

fn list_files(src: &Path, exclude: Filter) -> Result<Vec<Item>> {
    let mut out = vec![];
    if src.is_file() {
        let len = fs::metadata(src)?.len();
        out.push(Item { rel: PathBuf::from(src.file_name().unwrap()), len, link: None });
        return Ok(out);
    }
    for e in walkdir::WalkDir::new(src).into_iter().filter_entry(|e| !skip_name(&e.file_name().to_string_lossy())) {
        let e = e.with_context(|| format!("cannot read {}", src.display()))?;
        let rel = e.path().strip_prefix(src).unwrap().to_path_buf();
        if rel.as_os_str().is_empty() || exclude(&rel) {
            continue;
        }
        let ft = e.file_type();
        if ft.is_symlink() {
            out.push(Item { rel, len: 0, link: Some(fs::read_link(e.path())?) });
        } else if ft.is_file() {
            out.push(Item { rel, len: e.metadata()?.len(), link: None });
        }
    }
    Ok(out)
}

/// Modification time in whole seconds (network shares often keep nothing finer).
fn secs(m: &fs::Metadata) -> i64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

fn same(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.len() == b.len() && secs(a) == secs(b)
}

fn copy_file(src: &Path, dst: &Path, p: &mut Progress, cb: &mut dyn FnMut(&Progress), cancel: &Cancel) -> Result<()> {
    let tmp = dst.with_file_name(format!(".{}.steam-fan-part", dst.file_name().unwrap().to_string_lossy()));
    let mut r = fs::File::open(src).with_context(|| format!("cannot open {}", src.display()))?;
    let mut w = fs::File::create(&tmp).with_context(|| format!("cannot write {}", tmp.display()))?;
    let mut buf = vec![0u8; 4 << 20];
    let mut last = std::time::Instant::now();
    loop {
        if cancel.is_cancelled() {
            drop(w);
            let _ = fs::remove_file(&tmp);
            anyhow::bail!("stopped");
        }
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        w.write_all(&buf[..n])?;
        p.done_bytes += n as u64;
        if last.elapsed().as_millis() > 150 {
            cb(p);
            last = std::time::Instant::now();
        }
    }
    let meta = fs::metadata(src)?;
    if let Ok(t) = meta.modified() {
        w.set_times(fs::FileTimes::new().set_modified(t))?;
    }
    w.sync_all()?;
    drop(w);
    fs::set_permissions(&tmp, meta.permissions())?;
    fs::rename(&tmp, dst)?;
    Ok(())
}

/// Copy `src` (a folder, or a single file) into the folder `dst`.
/// Only new or changed files are copied. Nothing in `dst` is deleted.
pub fn copy_tree(src: &Path, dst: &Path, exclude: Filter, cancel: &Cancel, cb: &mut dyn FnMut(&Progress)) -> Result<Progress> {
    let items = list_files(src, exclude)?;
    let mut p = Progress {
        total_bytes: items.iter().map(|i| i.len).sum(),
        total_files: items.len() as u64,
        ..Default::default()
    };
    cb(&p);
    let base = if src.is_file() { src.parent().unwrap().to_path_buf() } else { src.to_path_buf() };
    for it in items {
        let s = base.join(&it.rel);
        let d = dst.join(&it.rel);
        p.current = it.rel.to_string_lossy().into_owned();
        fs::create_dir_all(d.parent().unwrap())?;
        if let Some(target) = it.link {
            if fs::read_link(&d).ok().as_ref() != Some(&target) {
                let _ = fs::remove_file(&d);
                #[cfg(unix)]
                std::os::unix::fs::symlink(&target, &d)?;
            }
        } else {
            let sm = fs::metadata(&s)?;
            match fs::metadata(&d) {
                Ok(dm) if same(&sm, &dm) => p.done_bytes += it.len,
                _ => copy_file(&s, &d, &mut p, cb, cancel)?,
            }
        }
        p.done_files += 1;
        cb(&p);
    }
    Ok(p)
}

/// Copy files from `src` to `dst` when they are new there or newer than the copy there.
/// Used for saves: run it both ways and each side ends up with the latest of every file.
pub fn copy_newer(src: &Path, dst: &Path, exclude: Filter) -> Result<u64> {
    if !src.exists() {
        return Ok(0);
    }
    let mut n = 0;
    let none = Cancel::default();
    for it in list_files(src, exclude)? {
        if it.link.is_some() {
            continue;
        }
        let s = src.join(&it.rel);
        let d = dst.join(&it.rel);
        let sm = fs::metadata(&s)?;
        let newer = match fs::metadata(&d) {
            Ok(dm) => secs(&sm) > secs(&dm),
            Err(_) => true,
        };
        if newer {
            fs::create_dir_all(d.parent().unwrap())?;
            copy_file(&s, &d, &mut Progress::default(), &mut |_| {}, &none)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Files in `local` that differ from the original in `original` (or don't exist there).
/// That is what a game wrote: saves and settings.
pub fn changed_files(local: &Path, original: &Path, exclude: Filter) -> Result<Vec<PathBuf>> {
    let mut out = vec![];
    for it in list_files(local, exclude)? {
        if it.link.is_some() {
            continue;
        }
        let lm = fs::metadata(local.join(&it.rel))?;
        match fs::metadata(original.join(&it.rel)) {
            Ok(om) if same(&lm, &om) => {}
            _ => out.push(it.rel),
        }
    }
    Ok(out)
}

pub fn copy_listed(src: &Path, dst: &Path, files: &[PathBuf]) -> Result<()> {
    let none = Cancel::default();
    for rel in files {
        let d = dst.join(rel);
        fs::create_dir_all(d.parent().unwrap())?;
        copy_file(&src.join(rel), &d, &mut Progress::default(), &mut |_| {}, &none)?;
    }
    Ok(())
}

pub fn dir_size(p: &Path) -> u64 {
    walkdir::WalkDir::new(p).into_iter().flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()
}

pub fn human(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} kB", b / 1e3)
    } else {
        format!("{bytes} B")
    }
}

/// Free space where `p` lives.
pub fn free_space(p: &Path) -> Option<u64> {
    let mut q = p.to_path_buf();
    while !q.exists() {
        q = q.parent()?.to_path_buf();
    }
    let out = std::process::Command::new("df").arg("-PB1").arg(&q).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().nth(1)?.split_whitespace().nth(3)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copy_resume_and_newer() {
        let t = tempfile::tempdir().unwrap();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        fs::create_dir_all(a.join("x/@eaDir")).unwrap();
        fs::write(a.join("x/f.txt"), "hello").unwrap();
        fs::write(a.join("x/@eaDir/thumb"), "no").unwrap();
        fs::write(a.join("top.bin"), vec![1u8; 1000]).unwrap();
        let none = |_: &Path| false;
        let p = copy_tree(&a, &b, &none, &Cancel::default(), &mut |_| {}).unwrap();
        assert_eq!(p.total_files, 2);
        assert_eq!(fs::read_to_string(b.join("x/f.txt")).unwrap(), "hello");
        assert!(!b.join("x/@eaDir").exists());
        // second run copies nothing new
        assert_eq!(changed_files(&b, &a, &none).unwrap().len(), 0);
        // a save written by the game
        fs::write(b.join("save.dat"), "s").unwrap();
        assert_eq!(changed_files(&b, &a, &none).unwrap(), vec![PathBuf::from("save.dat")]);
        let c = t.path().join("c");
        assert_eq!(copy_newer(&b, &c, &none).unwrap(), 3);
        assert_eq!(copy_newer(&b, &c, &none).unwrap(), 0);
    }
}
