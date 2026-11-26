//! Getting a game again from where it came from: its download link.
//!
//! * Plain file links (itch.io/GameJolt CDN links, GitHub releases, Dropbox) are downloaded directly.
//! * Google Drive share links are turned into their download address.
//! * MEGA links need the `megatools` program (`sudo pacman -S megatools`).
//! * Pages that need a browser (MediaFire, forum pages, captchas) can't be fetched:
//!   the error says so, and the app offers to open the page instead.

use crate::artwork::{self, Info};
use crate::config::Config;
use crate::shelf::Game;
use crate::transfer::{Cancel, Progress};
use anyhow::{bail, Context, Result};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) Steam-Fan/0.1";

#[derive(Debug, PartialEq)]
pub enum Host {
    Direct(String),
    Mega,
    /// Needs a browser; the name says which site.
    Manual(&'static str),
}

pub fn host_of(url: &str) -> Host {
    let low = url.to_lowercase();
    let domain = low.split("//").nth(1).unwrap_or(&low).split('/').next().unwrap_or("").to_string();
    let has = |d: &str| domain == d || domain.ends_with(&format!(".{d}"));
    if has("mega.nz") || has("mega.co.nz") {
        return Host::Mega;
    }
    if has("drive.google.com") || has("docs.google.com") {
        let id = url
            .split("/d/")
            .nth(1)
            .map(|r| r.split(['/', '?']).next().unwrap_or(""))
            .or_else(|| url.split("id=").nth(1).map(|r| r.split('&').next().unwrap_or("")))
            .unwrap_or("");
        if id.is_empty() {
            return Host::Manual("Google Drive (folder links)");
        }
        return Host::Direct(format!("https://drive.usercontent.google.com/download?id={id}&export=download&confirm=t"));
    }
    if has("dropbox.com") {
        let u = url.replace("dl=0", "dl=1");
        return Host::Direct(if u.contains("dl=1") { u } else if u.contains('?') { format!("{u}&dl=1") } else { format!("{u}?dl=1") });
    }
    // Everything else is tried as a direct download: forum attachment links (SFGHQ's
    // dl-attach.php), itch.io/GameJolt file links, GitHub releases... If the reply is a web
    // page instead of a file, the download stops and says so (see `direct`).
    Host::Direct(url.to_string())
}

fn file_name_from(url: &str, disposition: Option<&str>) -> String {
    if let Some(d) = disposition {
        for part in d.split(';') {
            let p = part.trim();
            if let Some(v) = p.strip_prefix("filename*=UTF-8''").or_else(|| p.strip_prefix("filename=")) {
                let v = v.trim_matches('"');
                if !v.is_empty() {
                    return v.replace('/', "-");
                }
            }
        }
    }
    let path = url.split('?').next().unwrap_or(url);
    let last = path.rsplit('/').next().unwrap_or("");
    if last.contains('.') { last.to_string() } else { "download.zip".to_string() }
}

fn direct(url: &str, dir: &Path, cancel: &Cancel, cb: &mut dyn FnMut(&Progress)) -> Result<PathBuf> {
    // a long timeout: a game can be gigabytes
    let agent = ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(20)).timeout_read(std::time::Duration::from_secs(120)).build();
    let r = agent.get(url).set("User-Agent", UA).call().context("download failed")?;
    let ctype = r.header("Content-Type").unwrap_or("").to_lowercase();
    if ctype.starts_with("text/html") {
        bail!(
            "the link answered with a web page, not a file (it may need a login, a captcha or a click). \
             Download it in your browser, then use Re-download > Use a file I downloaded."
        );
    }
    let total: u64 = r.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let name = file_name_from(url, r.header("Content-Disposition"));
    let out = dir.join(name);
    let mut f = fs::File::create(&out)?;
    let mut rd = r.into_reader();
    let mut buf = vec![0u8; 1 << 20];
    let mut p = Progress { total_bytes: total, total_files: 1, current: out.file_name().unwrap().to_string_lossy().into_owned(), ..Default::default() };
    let mut last = std::time::Instant::now();
    loop {
        if cancel.is_cancelled() {
            bail!("stopped");
        }
        let n = rd.read(&mut buf)?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n])?;
        p.done_bytes += n as u64;
        if last.elapsed().as_millis() > 150 {
            cb(&p);
            last = std::time::Instant::now();
        }
    }
    f.sync_all()?;
    Ok(out)
}

fn mega(url: &str, dir: &Path) -> Result<PathBuf> {
    let d = dir.to_string_lossy().into_owned();
    let tries: [(&str, Vec<&str>); 3] = [
        ("megatools", vec!["dl", "--no-progress", "--path", &d, url]),
        ("megadl", vec!["--no-progress", "--path", &d, url]),
        ("mega-get", vec![url, &d]),
    ];
    for (prog, args) in tries {
        let Ok(out) = Command::new(prog).args(&args).output() else { continue };
        if out.status.success() {
            let file = fs::read_dir(dir)?.flatten().map(|e| e.path()).find(|p| p.is_file() || p.is_dir());
            return file.context("MEGA download produced no file");
        }
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("MEGA download failed: {}", err.lines().last().unwrap_or("unknown error"));
    }
    bail!("MEGA links need megatools: sudo pacman -S megatools")
}

/// Download the game file from its link into a fresh temporary folder. Returns the file.
pub fn fetch(url: &str, cancel: &Cancel, cb: &mut dyn FnMut(&Progress)) -> Result<PathBuf> {
    let dir = dirs::cache_dir().unwrap_or_else(|| crate::config::home().join(".cache")).join("steam-fan/downloads");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    match host_of(url) {
        Host::Direct(u) => direct(&u, &dir, cancel, cb),
        Host::Mega => mega(url, &dir),
        Host::Manual(site) => bail!("{site} can't be downloaded automatically. Download it in your browser, then use Re-download > Use a file I downloaded."),
    }
}

/// What the link serves right now (size, date, tag), to notice a newer upload. None: can't tell for this kind of link.
pub fn remote_stamp(url: &str) -> Result<Option<String>> {
    let Host::Direct(u) = host_of(url) else { return Ok(None) };
    let r = match ureq::head(&u).set("User-Agent", UA).timeout(std::time::Duration::from_secs(20)).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(405, _)) => ureq::get(&u).set("User-Agent", UA).set("Range", "bytes=0-0").call()?,
        Err(e) => return Err(e).context("cannot reach the download link"),
    };
    let parts: Vec<String> = ["ETag", "Last-Modified", "Content-Length", "Content-Range"]
        .iter()
        .filter_map(|h| r.header(h).map(|v| v.to_string()))
        .collect();
    Ok((!parts.is_empty()).then(|| parts.join(" | ")))
}

pub fn downloads_dir_clean() {
    if let Some(d) = dirs::cache_dir() {
        let _ = fs::remove_dir_all(d.join("steam-fan/downloads"));
    }
}

/// Is there a newer file at the game's download link?
pub fn check_update(g: &Game) -> Result<String> {
    let info = artwork::info(g);
    if info.download.trim().is_empty() {
        bail!("no download link: add it under About > Edit");
    }
    match remote_stamp(info.download.trim())? {
        None => Ok("This kind of link (MEGA, a web page) can't be checked for updates; open it to see.".into()),
        Some(now) if info.download_stamp.is_empty() => Ok(format!("Can't tell yet: nothing was downloaded from this link by Steam-Fan. It serves: {now}")),
        Some(now) if now == info.download_stamp => Ok("Up to date: the link still serves the same file.".into()),
        Some(_) => Ok("A different file is at the link now: probably a new version. Use Re-download.".into()),
    }
}

pub fn save_stamp(cfg: &Config, g: &Game, info: &mut Info) {
    if let Ok(Some(st)) = remote_stamp(info.download.trim()) {
        info.download_stamp = st;
        let _ = artwork::save_info(cfg, g, info);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hosts() {
        assert_eq!(host_of("https://mega.nz/file/YQUg0J4L#Qck"), Host::Mega);
        assert_eq!(
            host_of("https://drive.google.com/file/d/1AbC_d/view?usp=sharing"),
            Host::Direct("https://drive.usercontent.google.com/download?id=1AbC_d&export=download&confirm=t".into())
        );
        assert_eq!(host_of("https://www.dropbox.com/s/x/game.zip?dl=0"), Host::Direct("https://www.dropbox.com/s/x/game.zip?dl=1".into()));
        let sfghq = "https://sonicfangameshq.com/forums/dl-attach.php?uri=attachments/sonic-quantum-collision_sage2020_etherealspeed-zip.8819/";
        assert_eq!(host_of(sfghq), Host::Direct(sfghq.into()));
        assert_eq!(host_of("https://drive.google.com/drive/folders/abc"), Host::Manual("Google Drive (folder links)"));
        assert_eq!(host_of("https://github.com/a/b/releases/download/v1/game.zip"), Host::Direct("https://github.com/a/b/releases/download/v1/game.zip".into()));
        assert_eq!(file_name_from("https://x/y/Game%20v2.zip?x=1", None), "Game%20v2.zip");
        assert_eq!(file_name_from("https://x/dl", Some("attachment; filename=\"SRB2.zip\"")), "SRB2.zip");
    }
}
