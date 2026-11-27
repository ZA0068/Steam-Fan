//! Server mode: the app talks to Steam-Fan-Server instead of a shared folder.
//!
//! The server keeps the shelf. This PC keeps what Steam needs: the library list (in the
//! database, as from a shelf read), the pictures (in the artwork cache), the games it plays
//! (copied file by file, only what changed, resumable, checked against their checksums) and a
//! copy of each game's save area, kept in step with the server's (newest file wins, nothing is
//! ever deleted).
//!
//! Addresses are tried in order (`servers` in the settings), e.g. the Tailscale name first and
//! the home address second; the first that answers is used for a while.

use crate::artwork::{self, Art, Info};
use crate::config::{secrets, Config};
use crate::db::Db;
use crate::shelf::{Game, GroupScan};
use crate::transfer::{Cancel, Progress};
use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, UNIX_EPOCH};

/// The token this PC got when it was paired, kept with the other secrets.
pub const TOKEN: &str = "STEAM_FAN_SERVER_TOKEN";

pub struct Server {
    pub base: String,
    token: String,
}

fn agent(read_secs: u64) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(4)).timeout_read(Duration::from_secs(read_secs)).build()
}

/// `http://nas:8484/` → `http://nas:8484`
fn clean(base: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    if b.contains("://") { b.to_string() } else { format!("http://{b}") }
}

/// Percent-encode a query value.
pub fn q(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

static ACTIVE: Mutex<Option<(Instant, Vec<String>, Option<String>)>> = Mutex::new(None);

/// The first server address that answers. Remembered for 30 seconds (an unreachable
/// server for 10), so the many "is it there?" checks don't each wait for the network.
pub fn active(cfg: &Config) -> Option<Server> {
    if cfg.servers.is_empty() {
        return None;
    }
    let mut cache = ACTIVE.lock().unwrap();
    if let Some((at, list, found)) = cache.as_ref() {
        let ttl = if found.is_some() { 30 } else { 10 };
        if *list == cfg.servers && at.elapsed().as_secs() < ttl {
            return found.clone().map(|base| Server { base, token: secrets::get(TOKEN) });
        }
    }
    let found = cfg.servers.iter().map(|s| clean(s)).find(|b| {
        agent(4).get(&format!("{b}/api/health")).call().map_or(false, |r| r.into_string().map_or(false, |t| t.contains("Steam-Fan-Server")))
    });
    *cache = Some((Instant::now(), cfg.servers.clone(), found.clone()));
    found.map(|base| Server { base, token: secrets::get(TOKEN) })
}

/// Forget which address answered (after the settings change).
pub fn forget() {
    *ACTIVE.lock().unwrap() = None;
}

/// The server, or why there's none.
pub fn require(cfg: &Config) -> Result<Server> {
    active(cfg).with_context(|| format!("Steam-Fan-Server isn't reachable (tried {})", cfg.servers.join(", ")))
}

fn api_error(e: ureq::Error) -> anyhow::Error {
    match e {
        ureq::Error::Status(401, _) => anyhow::anyhow!("this PC isn't paired with Steam-Fan-Server: pair it in Settings"),
        ureq::Error::Status(code, r) => {
            let text = r.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
                .unwrap_or_else(|| format!("the server answered {code}"));
            anyhow::anyhow!(msg)
        }
        e => crate::source::net_error("http://steam-fan-server", e),
    }
}

impl Server {
    fn url(&self, path: &str) -> String {
        format!("{}/api{path}", self.base)
    }
    fn req(&self, method: &str, path: &str, read_secs: u64) -> ureq::Request {
        agent(read_secs).request(method, &self.url(path)).set("Authorization", &format!("Bearer {}", self.token))
    }

    pub fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.req("GET", path, 120).call().map_err(api_error)?.into_json().context("the server's answer isn't JSON")
    }

    pub fn send_json<T: DeserializeOwned>(&self, method: &str, path: &str, body: serde_json::Value) -> Result<T> {
        self.req(method, path, 120).send_json(body).map_err(api_error)?.into_json().context("the server's answer isn't JSON")
    }

    pub fn post<T: DeserializeOwned>(&self, path: &str, body: serde_json::Value) -> Result<T> {
        self.send_json("POST", path, body)
    }

    pub fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let mut v = vec![];
        self.req("GET", path, 120).call().map_err(api_error)?.into_reader().read_to_end(&mut v)?;
        Ok(v)
    }

    pub fn post_bytes(&self, path: &str, bytes: &[u8]) -> Result<()> {
        self.req("POST", path, 300).send_bytes(bytes).map_err(api_error)?;
        Ok(())
    }

    pub fn put_bytes(&self, path: &str, bytes: &[u8]) -> Result<()> {
        self.req("PUT", path, 120).send_bytes(bytes).map_err(api_error)?;
        Ok(())
    }

    pub fn delete(&self, path: &str) -> Result<()> {
        self.req("DELETE", path, 60).call().map_err(api_error)?;
        Ok(())
    }

    /// Send a file as the body of a request (game archives: streamed, not read into memory).
    pub fn upload<T: DeserializeOwned>(&self, path: &str, file: &Path, cancel: &Cancel, cb: &mut dyn FnMut(&Progress)) -> Result<T> {
        struct Counting<'a> {
            f: fs::File,
            p: Progress,
            cb: &'a mut dyn FnMut(&Progress),
            cancel: &'a Cancel,
        }
        impl Read for Counting<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.cancel.is_cancelled() {
                    return Err(std::io::Error::other("stopped"));
                }
                let n = self.f.read(buf)?;
                self.p.done_bytes += n as u64;
                (self.cb)(&self.p);
                Ok(n)
            }
        }
        let size = fs::metadata(file)?.len();
        let p = Progress { total_bytes: size, total_files: 1, current: "sending to the server".into(), ..Default::default() };
        let r = Counting { f: fs::File::open(file)?, p, cb, cancel };
        self.req("POST", path, 3600)
            .set("Content-Length", &size.to_string())
            .send(r)
            .map_err(api_error)?
            .into_json()
            .context("the server's answer isn't JSON")
    }

    /// Download into `dst`, resuming a cut-off download (`.steam-fan-part` next to it).
    /// The file gets `mtime` as its date, so later checks see it's the same as the server's.
    pub fn download(&self, path: &str, dst: &Path, size: u64, mtime: i64, cancel: &Cancel, on_bytes: &mut dyn FnMut(u64)) -> Result<()> {
        fs::create_dir_all(dst.parent().context("no folder")?)?;
        let part = dst.with_file_name(format!(".{}.steam-fan-part", dst.file_name().unwrap().to_string_lossy()));
        let mut have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if have > size {
            have = 0;
        }
        let mut req = self.req("GET", path, 300);
        if have > 0 {
            req = req.set("Range", &format!("bytes={have}-"));
        }
        let resp = req.call().map_err(api_error)?;
        let resumed = resp.status() == 206;
        let mut out = if resumed {
            fs::OpenOptions::new().append(true).open(&part)?
        } else {
            have = 0;
            fs::File::create(&part)?
        };
        on_bytes(have);
        let mut rd = resp.into_reader();
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            if cancel.is_cancelled() {
                bail!("stopped");
            }
            let n = rd.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            on_bytes(n as u64);
        }
        out.flush()?;
        if mtime > 0 {
            out.set_modified(UNIX_EPOCH + Duration::from_secs(mtime as u64))?;
        }
        drop(out);
        let got = fs::metadata(&part)?.len();
        if got != size {
            bail!("{} arrived incomplete ({got} of {size} bytes); press Play again to resume", dst.display());
        }
        fs::rename(&part, dst)?;
        Ok(())
    }

    /// Follow a job on the server until it's done.
    pub fn wait(&self, job: u64, cancel: &Cancel, status: &mut dyn FnMut(&str), cb: &mut dyn FnMut(&Progress)) -> Result<String> {
        #[derive(Deserialize)]
        struct J {
            status: String,
            done: bool,
            error: Option<String>,
            result: Option<String>,
            done_bytes: u64,
            total_bytes: u64,
        }
        let mut last = String::new();
        loop {
            if cancel.is_cancelled() {
                let _: Result<bool> = self.post("/job/cancel", serde_json::json!({ "id": job }));
                bail!("stopped");
            }
            let j: J = self.get_json(&format!("/job?id={job}"))?;
            if j.status != last {
                status(&j.status);
                last = j.status.clone();
            }
            if j.total_bytes > 0 {
                cb(&Progress { done_bytes: j.done_bytes, total_bytes: j.total_bytes, ..Default::default() });
            }
            if j.done {
                return match j.error {
                    Some(e) => Err(anyhow::anyhow!(e)),
                    None => Ok(j.result.unwrap_or_default()),
                };
            }
            std::thread::sleep(Duration::from_millis(700));
        }
    }

    /// Start a job and follow it.
    pub fn run(&self, path: &str, body: serde_json::Value, cancel: &Cancel, status: &mut dyn FnMut(&str), cb: &mut dyn FnMut(&Progress)) -> Result<String> {
        #[derive(Deserialize)]
        struct Started {
            job: u64,
        }
        let s: Started = self.post(path, body)?;
        self.wait(s.job, cancel, status, cb)
    }
}

/// Pair this PC with a server: the code comes from `steam-fan-server pair` on the NAS.
/// The token is kept in the secrets file.
pub fn pair(base: &str, code: &str, device: &str) -> Result<()> {
    let base = clean(base);
    let r = agent(20)
        .post(&format!("{base}/api/pair"))
        .send_json(serde_json::json!({ "code": code.trim(), "device": device }))
        .map_err(api_error)?;
    let v: serde_json::Value = r.into_json()?;
    let token = v.get("token").and_then(|t| t.as_str()).context("the server sent no token")?;
    secrets::set(TOKEN, token)?;
    forget();
    Ok(())
}

/// This computer's name, for pairing.
pub fn device_name() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .or_else(|| fs::read_to_string("/etc/hostname").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "PC".into())
}

// ---------------------------------------------------------------- the library

#[derive(Deserialize)]
struct Snapshot {
    games: Vec<Game>,
    #[serde(default)]
    info: HashMap<String, Info>,
    #[serde(default)]
    art: HashMap<String, Vec<(Art, i64)>>,
    #[serde(default)]
    screenshots: HashMap<String, Vec<String>>,
}

const REMOTE_ART: &str = "server-art.json";
const REMOTE_SHOTS: &str = "server-screenshots.json";

/// The library from the server: stored as if the shelf had been read here.
pub fn refresh(cfg: &Config, db: &mut Db) -> Result<crate::library::RefreshReport> {
    let s = require(cfg)?;
    let snap: Snapshot = s.get_json("/snapshot")?;
    for g in &snap.games {
        db.set_program(&g.id, &g.program, g.platform)?;
        if g.built > 0 {
            db.set_built(&g.id, g.built)?;
        }
    }
    let n = snap.games.len();
    let mut groups = HashMap::new();
    groups.insert("server".to_string(), GroupScan { path: "server".into(), mtime: 0, subgroups: vec![], games: snap.games.clone() });
    db.save_groups(&groups)?;
    // info, and which pictures and screenshots each entry has
    for t in crate::library::titles(snap.games) {
        let id = t.id();
        let dir = artwork::cache_dir(&t.game);
        fs::create_dir_all(&dir)?;
        match snap.info.get(&id) {
            Some(i) => fs::write(dir.join("info.toml"), toml::to_string_pretty(i)?)?,
            None => {
                let _ = fs::remove_file(dir.join("info.toml"));
            }
        }
        fs::write(dir.join(REMOTE_ART), serde_json::to_string(snap.art.get(&id).unwrap_or(&vec![]))?)?;
        fs::write(dir.join(REMOTE_SHOTS), serde_json::to_string(snap.screenshots.get(&id).unwrap_or(&vec![]))?)?;
    }
    Ok(crate::library::RefreshReport { complete: true, games: n, ..Default::default() })
}

/// Bring a game's pictures from the server into the artwork cache (only new or changed ones).
/// Returns whether anything changed.
pub fn fetch_art(cfg: &Config, g: &Game) -> Result<bool> {
    let dir = artwork::cache_dir(g);
    let want: Vec<(Art, i64)> = fs::read_to_string(dir.join(REMOTE_ART)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut changed = false;
    let mut server = None;
    for a in Art::all() {
        let have = artwork::local(g, a);
        match want.iter().find(|(k, _)| *k == a) {
            None => {
                if let Some(p) = have {
                    let _ = fs::remove_file(p);
                    changed = true;
                }
            }
            Some((_, mtime)) => {
                let fresh = have.as_ref().map_or(false, |p| crate::shelf::mtime(p) / 1_000_000_000 >= *mtime);
                if fresh {
                    continue;
                }
                if server.is_none() {
                    server = Some(require(cfg)?);
                }
                let bytes = server.as_ref().unwrap().get_bytes(&format!("/art?id={}&kind={}", q(&g.title_id()), a.name()))?;
                if let Some(p) = have {
                    let _ = fs::remove_file(p);
                }
                let ext = match image::guess_format(&bytes) {
                    Ok(image::ImageFormat::Jpeg) => "jpg",
                    Ok(image::ImageFormat::WebP) => "webp",
                    _ => "png",
                };
                let p = dir.join(format!("{}.{ext}", a.name()));
                fs::write(&p, &bytes)?;
                fs::File::options().write(true).open(&p)?.set_modified(UNIX_EPOCH + Duration::from_secs(*mtime as u64))?;
                changed = true;
            }
        }
    }
    if changed {
        let _ = fs::remove_file(dir.join("cover-thumb.jpg"));
    }
    Ok(changed)
}

/// After a change: this entry's info, picture list and screenshots again from the server.
pub fn update_entry(cfg: &Config, g: &Game) -> Result<()> {
    #[derive(Deserialize)]
    struct Entry {
        info: Info,
        art: Vec<(Art, i64)>,
        screenshots: Vec<String>,
    }
    let e: Entry = require(cfg)?.get_json(&format!("/entry?id={}", q(&g.title_id())))?;
    let dir = artwork::cache_dir(g);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("info.toml"), toml::to_string_pretty(&e.info)?)?;
    fs::write(dir.join(REMOTE_ART), serde_json::to_string(&e.art)?)?;
    fs::write(dir.join(REMOTE_SHOTS), serde_json::to_string(&e.screenshots)?)?;
    fetch_art(cfg, g)?;
    Ok(())
}

/// The screenshots the server has for a game.
pub fn screenshot_names(g: &Game) -> Vec<String> {
    fs::read_to_string(artwork::cache_dir(g).join(REMOTE_SHOTS)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// One screenshot, from the cache or the server.
pub fn screenshot(cfg: &Config, g: &Game, name: &str) -> Result<PathBuf> {
    if name.contains('/') || name.contains('\\') || name.starts_with('.') {
        bail!("bad name");
    }
    let p = artwork::cache_dir(g).join("screenshots").join(name);
    if !p.is_file() {
        let bytes = require(cfg)?.get_bytes(&format!("/art?id={}&shot={}", q(&g.title_id()), q(name)))?;
        fs::create_dir_all(p.parent().unwrap())?;
        fs::write(&p, bytes)?;
    }
    Ok(p)
}

// ---------------------------------------------------------------- game files

#[derive(Debug, Clone, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub mtime: i64,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteManifest {
    pub single_file: bool,
    pub total: u64,
    pub checksums: bool,
    pub files: Vec<FileEntry>,
}

pub fn manifest(cfg: &Config, g: &Game) -> Result<RemoteManifest> {
    require(cfg)?.get_json(&format!("/manifest?id={}", q(&g.id)))
}

fn secs(p: &Path) -> Option<i64> {
    fs::metadata(p).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

/// Is this local file the server's (same size and date, as set when it was downloaded)?
fn same(p: &Path, f: &FileEntry) -> bool {
    fs::metadata(p).map_or(false, |m| m.is_file() && m.len() == f.size) && secs(p) == Some(f.mtime)
}

/// Copy a game from the server to this PC: only missing or changed files, each checked
/// against its checksum when the server has one.
pub fn install(cfg: &Config, db: &Db, g: &Game, cancel: &Cancel, cb: &mut dyn FnMut(&Progress)) -> Result<()> {
    let s = require(cfg)?;
    let m: RemoteManifest = s.get_json(&format!("/manifest?id={}", q(&g.id)))?;
    let dst = g.local_path(cfg);
    let todo: Vec<&FileEntry> = m.files.iter().filter(|f| !same(&dst.join(&f.path), f)).collect();
    let need: u64 = todo.iter().map(|f| f.size).sum();
    fs::create_dir_all(&dst)?;
    if let Some(free) = crate::transfer::free_space(&dst) {
        if need > free {
            bail!("not enough space: {} needs {}, {} free", g.name, crate::transfer::human(need), crate::transfer::human(free));
        }
    }
    db.log(&g.id, &format!("download started from the server ({} of {} files)", todo.len(), m.files.len()));
    let mut p = Progress { total_bytes: need, total_files: todo.len() as u64, ..Default::default() };
    for f in &todo {
        p.current = f.path.clone();
        let target = dst.join(&f.path);
        let before = p.done_bytes;
        let mut on = |n: u64| {
            p.done_bytes += n;
            cb(&p);
        };
        let path = format!("/file?id={}&path={}", q(&g.id), q(&f.path));
        s.download(&path, &target, f.size, f.mtime, cancel, &mut on)?;
        if let Some(want) = &f.sha256 {
            let got = crate::integrity::hash_file(&target, cancel, &mut |_| {})?;
            if &got != want {
                // once more from the start
                p.done_bytes = before;
                let _ = fs::remove_file(&target);
                let mut on = |n: u64| {
                    p.done_bytes += n;
                    cb(&p);
                };
                s.download(&path, &target, f.size, f.mtime, cancel, &mut on)?;
                if &crate::integrity::hash_file(&target, cancel, &mut |_| {})? != want {
                    bail!("{} doesn't match its checksum, also downloaded again: the server's copy may be damaged (Verify it there)", f.path);
                }
            }
        }
        p.done_files += 1;
        cb(&p);
    }
    if m.checksums {
        db.log(&g.id, "download checked against its checksums");
    }
    if matches!(g.kind, crate::shelf::Kind::Fan | crate::shelf::Kind::NonSteam) {
        let n = crate::saves::restore_game_folder(cfg, g)?;
        if n > 0 {
            db.log(&g.id, &format!("{n} saved files restored"));
        }
    }
    fs::write(dst.join(crate::saves::MARKER), crate::now_stamp())?;
    db.log(&g.id, "on this PC");
    Ok(())
}

/// Files in the PC copy that differ from the server's (saves, settings the game wrote).
pub fn changed_files(cfg: &Config, g: &Game, local: &Path, exclude: crate::transfer::Filter) -> Result<Vec<PathBuf>> {
    let m = manifest(cfg, g)?;
    let by: HashMap<&str, &FileEntry> = m.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let mut out = vec![];
    for e in walkdir::WalkDir::new(local).follow_links(false) {
        let e = e?;
        if !e.file_type().is_file() {
            continue;
        }
        let rel = e.path().strip_prefix(local)?.to_path_buf();
        if exclude(&rel) {
            continue;
        }
        let key = rel.to_string_lossy().replace('\\', "/");
        match by.get(key.as_str()) {
            Some(f) if same(e.path(), f) => {}
            _ => out.push(rel),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- saves

/// Bring newer save files from the server into this PC's copy of the save area.
pub fn pull_saves(cfg: &Config, g: &Game) -> Result<u64> {
    let s = require(cfg)?;
    let list: Vec<FileEntry> = s.get_json(&format!("/saves?id={}", q(&g.id)))?;
    let root = g.save_path(cfg);
    let mut n = 0;
    for f in list {
        let p = root.join(&f.path);
        if secs(&p).map_or(false, |t| t >= f.mtime) {
            continue;
        }
        s.download(&format!("/save?id={}&path={}", q(&g.id), q(&f.path)), &p, f.size, f.mtime, &Cancel::default(), &mut |_| {})?;
        n += 1;
    }
    Ok(n)
}

/// Send newer save files from this PC's copy of the save area to the server.
pub fn push_saves(cfg: &Config, g: &Game) -> Result<u64> {
    let root = g.save_path(cfg);
    if !root.is_dir() {
        return Ok(0);
    }
    let s = require(cfg)?;
    let list: Vec<FileEntry> = s.get_json(&format!("/saves?id={}", q(&g.id)))?;
    let theirs: HashMap<String, i64> = list.into_iter().map(|f| (f.path, f.mtime)).collect();
    let mut n = 0;
    for e in walkdir::WalkDir::new(&root).follow_links(false) {
        let e = e?;
        if !e.file_type().is_file() {
            continue;
        }
        let rel = e.path().strip_prefix(&root)?.to_string_lossy().replace('\\', "/");
        if rel.ends_with(".steam-fan-part") {
            continue;
        }
        let mine = secs(e.path()).unwrap_or(0);
        if theirs.get(&rel).map_or(false, |t| *t >= mine) {
            continue;
        }
        let bytes = fs::read(e.path())?;
        s.put_bytes(&format!("/save?id={}&path={}&mtime={mine}", q(&g.id), q(&rel)), &bytes)?;
        n += 1;
    }
    Ok(n)
}

// ---------------------------------------------------------------- asking the server to do things

/// Move or rename a game on the server (one version, or with `whole` the game and all its
/// versions), then read the library again. Returns the new id.
pub fn relocate(cfg: &Config, db: &mut Db, g: &Game, kind: crate::shelf::Kind, group: &str, name: &str, version: &str, whole: bool) -> Result<String> {
    let id = if whole { g.title_id() } else { g.id.clone() };
    let new: String = require(cfg)?.post(
        "/relocate",
        serde_json::json!({ "id": id, "kind": kind.prefix(), "group": group, "name": name, "version": version, "whole": whole }),
    )?;
    // the copy on this PC moves along, like with a shared folder
    let old_local = if whole { g.local_path(cfg).parent().map(Path::to_path_buf) } else { Some(g.local_path(cfg)) };
    refresh(cfg, db)?;
    let games = db.games()?;
    if let (Some(old), Some(ng)) = (old_local, crate::library::resolve(&games, &new)) {
        let new_local = if whole { ng.local_path(cfg).parent().map(Path::to_path_buf) } else { Some(ng.local_path(cfg)) };
        if let Some(nl) = new_local.filter(|nl| old.exists() && !nl.exists() && *nl != old) {
            fs::create_dir_all(nl.parent().unwrap())?;
            fs::rename(&old, &nl)?;
        }
    }
    db.rename_game(&id, &new)?;
    Ok(new)
}

/// Separate games or versions of one game (see `library::set_separate`), on the server.
pub fn set_separate(cfg: &Config, db: &mut Db, g: &Game, separate: bool) -> Result<()> {
    let _: String = require(cfg)?.post("/set-separate", serde_json::json!({ "id": g.id, "separate": separate }))?;
    refresh(cfg, db)?;
    Ok(())
}

/// Download a game again on the server (from its link, or a file sent from this PC).
pub fn redownload(cfg: &Config, db: &mut Db, g: &Game, as_version: Option<&str>, file: Option<&Path>, cancel: &Cancel, status: &mut dyn FnMut(&str), cb: &mut dyn FnMut(&Progress)) -> Result<PathBuf> {
    let s = require(cfg)?;
    let msg = match file {
        Some(f) => {
            #[derive(Deserialize)]
            struct Started {
                job: u64,
            }
            let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "upload.zip".into());
            status("sending the file to the server");
            let path = format!("/redownload-file?id={}&version={}&filename={}", q(&g.id), q(as_version.unwrap_or("")), q(&name));
            let st: Started = s.upload(&path, f, cancel, cb)?;
            s.wait(st.job, cancel, status, cb)?
        }
        None => s.run("/redownload", serde_json::json!({ "id": g.id, "version": as_version }), cancel, status, cb)?,
    };
    db.log(&g.id, &format!("downloaded again on the server: {msg}"));
    refresh(cfg, db)?;
    Ok(PathBuf::from(msg))
}

/// Add a game to the server's shelf from an archive or folder on this PC.
pub fn import(cfg: &Config, db: &mut Db, source: &Path, kind: crate::shelf::Kind, group: &str, name: &str, version: &str, cancel: &Cancel, status: &mut dyn FnMut(&str), cb: &mut dyn FnMut(&Progress)) -> Result<String> {
    let s = require(cfg)?;
    // a folder travels as a zip
    let tmp;
    let (file, fname) = if source.is_dir() {
        status("packing the folder");
        tmp = std::env::temp_dir().join(format!("steam-fan-upload-{}.zip", std::process::id()));
        zip_folder(source, &tmp)?;
        (tmp.clone(), format!("{name}.zip"))
    } else {
        (source.to_path_buf(), source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "upload.zip".into()))
    };
    #[derive(Deserialize)]
    struct Started {
        job: u64,
    }
    let path = format!("/import?kind={}&group={}&name={}&version={}&filename={}", kind.prefix(), q(group), q(name), q(version), q(&fname));
    status("sending to the server");
    let st: Result<Started> = s.upload(&path, &file, cancel, cb);
    if source.is_dir() {
        let _ = fs::remove_file(&file);
    }
    let msg = s.wait(st?.job, cancel, status, cb)?;
    refresh(cfg, db)?;
    Ok(msg)
}

fn zip_folder(dir: &Path, out: &Path) -> Result<()> {
    let f = fs::File::create(out)?;
    let mut z = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
    for e in walkdir::WalkDir::new(dir).follow_links(false) {
        let e = e?;
        let rel = e.path().strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
        if rel.is_empty() {
            continue;
        }
        if e.file_type().is_dir() {
            z.add_directory(format!("{rel}/"), opts)?;
        } else if e.file_type().is_file() {
            z.start_file(rel, opts)?;
            std::io::copy(&mut fs::File::open(e.path())?, &mut z)?;
        }
    }
    z.finish()?;
    Ok(())
}

/// Ask the server whether the game's link has a new file.
pub fn check_update(cfg: &Config, g: &Game) -> Result<String> {
    #[derive(Deserialize)]
    struct U {
        msg: String,
    }
    let u: U = require(cfg)?.post("/check-update", serde_json::json!({ "id": g.id }))?;
    Ok(u.msg)
}

/// Pictures on a web page, as the server sees it (it isn't behind this PC's VPN).
pub fn page_images(cfg: &Config, url: &str) -> Result<Vec<String>> {
    require(cfg)?.get_json(&format!("/page-images?url={}", q(url)))
}

/// The server downloads pictures from the web as screenshots.
pub fn screenshots_from_urls(cfg: &Config, g: &Game, urls: &[String]) -> Result<artwork::Added> {
    #[derive(Deserialize)]
    struct A {
        added: usize,
        failed: Vec<String>,
    }
    let a: A = require(cfg)?.post("/screenshots-from-urls", serde_json::json!({ "id": g.title_id(), "urls": urls }))?;
    update_entry(cfg, g)?;
    Ok(artwork::Added { added: a.added, failed: a.failed })
}

/// Verify a game: the copy on this PC against the server's file list and checksums
/// (damaged or missing files are downloaded again), or, if it isn't on this PC, the
/// server's own copy (the server patches it from the game's link).
pub fn verify(cfg: &Config, g: &Game, cancel: &Cancel, status: &mut dyn FnMut(&str), cb: &mut dyn FnMut(&Progress)) -> Result<crate::integrity::Report> {
    use crate::integrity::{Check, Report};
    let s = require(cfg)?;
    if !crate::library::is_installed(cfg, g) {
        let msg = s.run("/verify", serde_json::json!({ "id": g.id }), cancel, status, cb)?;
        return Ok(Report { shelf: Some(Check { checked: 0, ..Default::default() }), server_summary: Some(msg), ..Default::default() });
    }
    let m: RemoteManifest = s.get_json(&format!("/manifest?id={}", q(&g.id)))?;
    let local = g.local_path(cfg);
    status("checking the copy on this PC");
    let mut c = Check { checked: m.files.len(), ..Default::default() };
    let mut p = Progress { total_bytes: m.total, total_files: m.files.len() as u64, ..Default::default() };
    let mut bad = vec![];
    for f in &m.files {
        let path = local.join(&f.path);
        p.current = f.path.clone();
        if !path.is_file() {
            c.missing.push(f.path.clone());
            bad.push(f);
        } else if let Some(want) = &f.sha256 {
            let got = crate::integrity::hash_file(&path, cancel, &mut |n| {
                p.done_bytes += n;
                cb(&p);
            })?;
            if &got != want {
                if crate::integrity::rewritten_by_game(&f.path) {
                    c.changed_by_game.push(f.path.clone());
                } else {
                    c.damaged.push(f.path.clone());
                    bad.push(f);
                }
            }
            continue;
        }
        p.done_bytes += f.size;
        cb(&p);
    }
    let mut rep = Report::default();
    if !bad.is_empty() {
        status("downloading damaged files again from the server");
        for f in bad {
            let target = local.join(&f.path);
            let _ = fs::remove_file(&target);
            s.download(&format!("/file?id={}&path={}", q(&g.id), q(&f.path)), &target, f.size, f.mtime, cancel, &mut |_| {})?;
            rep.repaired.push(f.path.clone());
        }
    }
    rep.pc = Some(c);
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn addresses_and_queries() {
        assert_eq!(clean("10.0.0.36:8484/"), "http://10.0.0.36:8484");
        assert_eq!(clean("https://nas.example.ts.net:8443"), "https://nas.example.ts.net:8443");
        assert_eq!(q("fan:Sonic The Hedgehog/Sonic World"), "fan%3ASonic%20The%20Hedgehog%2FSonic%20World");
        let cfg = Config::default();
        assert!(active(&cfg).is_none(), "no servers: folder mode");
    }
}
