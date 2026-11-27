//! Game Jolt: game pages like `https://gamejolt.com/games/ChaosShards/415280`.
//!
//! Game Jolt has no plain download links: its own site asks its (undocumented) site API for the
//! game's builds, then for a download address that expires after a while. Steam-Fan does
//! the same:
//!
//! 1. `GET  site-api/web/discover/games/packages/<game>`: packages, releases, builds;
//! 2. pick the newest Windows build (64-bit preferred), as Proton runs it;
//! 3. `POST site-api/web/discover/games/builds/get-download-url/<build>`: the file's address.
//!
//! The build's id is the update stamp: a new release means a new build.
//! The API isn't public and can change; errors say what was found, and
//! `steam-fan link <url>` shows what a link resolves to without downloading.

use anyhow::{bail, Context, Result};
use serde_json::Value;

const API: &str = "https://gamejolt.com/site-api/web";
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) Steam-Fan/0.1";

/// The game's number in a Game Jolt link: `gamejolt.com/games/<name>/<number>[/...]`.
pub fn game_id(url: &str) -> Option<u64> {
    let low = url.to_lowercase();
    let after = low.split("//").nth(1).unwrap_or(&low);
    let mut parts = after.split(['/', '?', '#']);
    let host = parts.next()?;
    if !(host == "gamejolt.com" || host.ends_with(".gamejolt.com")) {
        return None;
    }
    let rest: Vec<&str> = parts.collect();
    let i = rest.iter().position(|p| *p == "games")?;
    rest[i + 1..].iter().find_map(|p| p.parse::<u64>().ok())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Build {
    pub id: u64,
    pub filename: String,
    pub size: u64,
    /// "windows-64", "windows", "linux"...
    pub os: String,
    pub version: String,
}

impl Build {
    /// What changes when the developer uploads a new release.
    pub fn stamp(&self) -> String {
        format!("gamejolt build {} | {} | {}", self.id, self.filename, self.size)
    }
}

fn get_json(url: &str) -> Result<Value> {
    let r = ureq::get(url).set("User-Agent", UA).set("Accept", "application/json").timeout(std::time::Duration::from_secs(20)).call();
    let r = match r {
        Ok(r) => r,
        Err(ureq::Error::Status(404, _)) => bail!("Game Jolt doesn't know this game (removed, or the link is wrong)"),
        Err(e) => return Err(crate::source::net_error(url, e)),
    };
    serde_json::from_reader(r.into_reader()).context("Game Jolt answered with something that isn't JSON")
}

/// Every array stored under `key`, anywhere in the reply.
fn arrays<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == key && x.is_array() {
                    out.extend(x.as_array().unwrap());
                } else {
                    arrays(x, key, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| arrays(x, key, out)),
        _ => {}
    }
}

fn flag(b: &Value, k: &str) -> bool {
    match b.get(k) {
        Some(Value::Bool(x)) => *x,
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0) != 0,
        _ => false,
    }
}

fn num(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0)
}

/// The build to download from a `packages` reply: the newest Windows build, 64-bit preferred.
pub fn pick_build(reply: &Value) -> Result<Build> {
    let mut builds = vec![];
    arrays(reply, "builds", &mut builds);
    let mut releases = vec![];
    arrays(reply, "releases", &mut releases);
    let version_of = |b: &Value| {
        let rid = num(b, "game_release_id").max(num(b, "release_id"));
        releases.iter().find(|r| num(r, "id") == rid).and_then(|r| r.get("version_number")).and_then(|v| v.as_str()).unwrap_or("").to_string()
    };
    let os_of = |b: &Value| {
        if flag(b, "os_windows_64") {
            "windows-64"
        } else if flag(b, "os_windows") {
            "windows"
        } else if flag(b, "os_linux") || flag(b, "os_linux_64") {
            "linux"
        } else if flag(b, "os_mac") || flag(b, "os_mac_64") {
            "mac"
        } else {
            "other"
        }
    };
    let downloadable = |b: &&&Value| b.get("type").and_then(|t| t.as_str()).map_or(true, |t| t == "downloadable" || t == "rom");
    let found: Vec<String> = builds.iter().map(|b| format!("{} ({})", os_of(b), b.get("type").and_then(|t| t.as_str()).unwrap_or("?"))).collect();
    let best = builds
        .iter()
        .filter(downloadable)
        .filter(|b| os_of(b).starts_with("windows"))
        .max_by_key(|b| (num(b, "game_release_id").max(num(b, "release_id")), os_of(b) == "windows-64", num(b, "id")));
    let Some(b) = best else {
        if found.is_empty() {
            bail!("this Game Jolt game has no downloadable files (browser-only, or not released yet)");
        }
        bail!("no Windows download on Game Jolt for this game; it has: {}", found.join(", "));
    };
    let file = b.get("primary_file").or_else(|| b.get("file")).cloned().unwrap_or(Value::Null);
    Ok(Build {
        id: num(b, "id"),
        filename: file.get("filename").and_then(|f| f.as_str()).unwrap_or("").to_string(),
        size: num(&file, "filesize").max(num(&file, "size")),
        os: os_of(b).to_string(),
        version: version_of(b),
    })
}

/// The build to download for a game.
pub fn latest_build(game: u64) -> Result<Build> {
    pick_build(&get_json(&format!("{API}/discover/games/packages/{game}"))?)
}

/// The first web address found under the key `url` in a reply.
fn url_in(v: &Value) -> Option<String> {
    match v {
        Value::Object(m) => m
            .get("url")
            .and_then(|u| u.as_str())
            .filter(|u| u.starts_with("http"))
            .map(String::from)
            .or_else(|| m.values().find_map(url_in)),
        Value::Array(a) => a.iter().find_map(url_in),
        _ => None,
    }
}

/// The (short-lived) address of a build's file.
pub fn download_url(build: u64) -> Result<String> {
    let u = format!("{API}/discover/games/builds/get-download-url/{build}");
    let post = ureq::post(&u).set("User-Agent", UA).timeout(std::time::Duration::from_secs(20)).send_json(serde_json::json!({ "forceDownload": true }));
    let reply: Value = match post {
        Ok(r) => r.into_json().context("Game Jolt's download answer isn't JSON")?,
        // some builds answer to GET only
        Err(_) => get_json(&u)?,
    };
    url_in(&reply).context("Game Jolt gave no download address (the game may need a login or payment)")
}

/// Resolve a game link to (file address, build).
pub fn resolve(game: u64) -> Result<(String, Build)> {
    let b = latest_build(game)?;
    Ok((download_url(b.id)?, b))
}

/// Picture addresses in a reply: Game Jolt's CDN images, except avatars and the like.
fn images_in(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                let k = k.to_lowercase();
                if ["user", "developer", "owner", "author", "avatar", "sticker", "community", "comments"].iter().any(|s| k.contains(s)) {
                    continue;
                }
                images_in(x, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| images_in(x, out)),
        Value::String(s) => {
            let low = s.to_lowercase();
            let path = low.split('?').next().unwrap_or("");
            if low.starts_with("https://") && low.contains("gjcdn.net") && [".png", ".jpg", ".jpeg", ".gif", ".webp"].iter().any(|e| path.ends_with(e)) && !out.contains(s) {
                out.push(s.clone());
            }
        }
        _ => {}
    }
}

/// Header, thumbnail and screenshots of a Game Jolt game.
pub fn images(game: u64) -> Result<Vec<String>> {
    let mut out = vec![];
    let mut err = None;
    for path in [format!("{API}/discover/games/overview/{game}"), format!("{API}/discover/games/{game}")] {
        match get_json(&path) {
            Ok(v) => images_in(&v, &mut out),
            Err(e) => err = Some(e),
        }
    }
    if out.is_empty() {
        if let Some(e) = err {
            return Err(e);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_from_links() {
        assert_eq!(game_id("https://gamejolt.com/games/ChaosShards/415280"), Some(415280));
        assert_eq!(game_id("https://gamejolt.com/games/ChaosShards/415280/download/build/1621836"), Some(415280));
        assert_eq!(game_id("https://www.gamejolt.com/games/sonic-x/12?foo=1"), Some(12));
        assert_eq!(game_id("https://gamejolt.com/@someone"), None);
        assert_eq!(game_id("https://example.com/games/x/5"), None);
    }

    #[test]
    fn picks_the_newest_windows_build() {
        let reply = serde_json::json!({ "payload": {
            "packages": [{ "id": 1, "title": "Chaos Shards" }],
            "releases": [{ "id": 10, "version_number": "1.0.0" }, { "id": 11, "version_number": "1.1.0" }],
            "builds": [
                { "id": 100, "game_release_id": 10, "type": "downloadable", "os_windows": true, "primary_file": { "filename": "cs-1.0.zip", "filesize": 5 } },
                { "id": 101, "game_release_id": 11, "type": "downloadable", "os_windows": true, "primary_file": { "filename": "cs-1.1-32.zip", "filesize": 6 } },
                { "id": 102, "game_release_id": 11, "type": "downloadable", "os_windows_64": true, "primary_file": { "filename": "cs-1.1.zip", "filesize": 7 } },
                { "id": 103, "game_release_id": 11, "type": "downloadable", "os_linux": true, "primary_file": { "filename": "cs-linux.zip", "filesize": 8 } },
                { "id": 104, "game_release_id": 11, "type": "html", "primary_file": { "filename": "web.zip", "filesize": 9 } }
            ]
        }});
        let b = pick_build(&reply).unwrap();
        assert_eq!((b.id, b.filename.as_str(), b.os.as_str(), b.version.as_str(), b.size), (102, "cs-1.1.zip", "windows-64", "1.1.0", 7));
        let linux_only = serde_json::json!({ "payload": { "builds": [{ "id": 1, "type": "downloadable", "os_linux": true }] } });
        assert!(pick_build(&linux_only).unwrap_err().to_string().contains("linux"));
        let web_only = serde_json::json!({ "payload": { "builds": [] } });
        assert!(pick_build(&web_only).unwrap_err().to_string().contains("no downloadable"));
    }

    #[test]
    fn download_address_and_pictures() {
        assert_eq!(url_in(&serde_json::json!({ "payload": { "url": "https://cdn.gamejolt.net/x.zip?t=1" } })).as_deref(), Some("https://cdn.gamejolt.net/x.zip?t=1"));
        let v = serde_json::json!({ "payload": {
            "game": { "header_media_item": { "img_url": "https://m.gjcdn.net/game-header/950/a.png" }, "developer": { "img_avatar": "https://m.gjcdn.net/user-avatar/200/u.png" } },
            "mediaItems": [{ "img_url": "https://m.gjcdn.net/screenshot-thumbnail/900x2000/s1.jpg" }, { "img_url": "https://m.gjcdn.net/screenshot-thumbnail/900x2000/s1.jpg" }]
        }});
        let mut out = vec![];
        images_in(&v, &mut out);
        assert_eq!(out, ["https://m.gjcdn.net/game-header/950/a.png", "https://m.gjcdn.net/screenshot-thumbnail/900x2000/s1.jpg"]);
    }
}
