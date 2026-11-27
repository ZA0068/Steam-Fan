//! GitHub releases: `https://github.com/STJr/SRB2`, `.../releases`, `.../releases/latest` or
//! `.../releases/tag/<tag>`. Many fan games publish their builds there (Sonic Robo Blast 2:
//! `SRB2-v2215-Full.zip`), so a project link is enough: Steam-Fan asks GitHub's API for the
//! release and picks its Windows game archive. The release's tag is the update stamp.
//! A direct asset link (`.../releases/download/<tag>/<file>`) is just a direct download.

use anyhow::{bail, Context, Result};
use serde_json::Value;

const UA: &str = "Steam-Fan (https://github.com/; fan game library)";

/// owner, repo and (if the link names one) a release tag.
pub fn repo_of(url: &str) -> Option<(String, String, Option<String>)> {
    let after = url.split("//").nth(1).unwrap_or(url);
    let mut parts = after.split(['?', '#']).next()?.split('/').filter(|p| !p.is_empty());
    let host = parts.next()?.to_lowercase();
    if host != "github.com" && host != "www.github.com" {
        return None;
    }
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.trim_end_matches(".git").to_string();
    let rest: Vec<&str> = parts.collect();
    match rest.as_slice() {
        [] | ["releases"] | ["releases", "latest"] => Some((owner, repo, None)),
        ["releases", "tag", tag, ..] => Some((owner, repo, Some(tag.to_string()))),
        // a file in a release, a branch, an issue...: not ours
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub tag: String,
}

impl Asset {
    pub fn stamp(&self) -> String {
        format!("github {} | {} | {}", self.tag, self.name, self.size)
    }
}

/// How well a release file fits as a Windows game download (None: not a game download).
fn score(name: &str) -> Option<i32> {
    let n = name.to_lowercase();
    let bad = ["source", "src", "symbol", "debug", ".pdb", "mac", "osx", "darwin", "android", ".apk", "linux", ".appimage", ".deb", ".rpm", ".flatpak", ".dmg", ".sha256", ".sig", ".asc", ".txt", ".md"];
    if bad.iter().any(|b| n.contains(b)) {
        return None;
    }
    let mut s = if n.ends_with(".zip") || n.ends_with(".7z") || n.ends_with(".rar") {
        100
    } else if n.ends_with(".exe") {
        40
    } else if n.ends_with(".tar.gz") || n.ends_with(".tar.xz") {
        20
    } else {
        return None;
    };
    for (word, add) in [("full", 30), ("win", 15), ("64", 5), ("patch", -60), ("update", -20), ("installer", -30), ("setup", -30), ("32", -3)] {
        if n.contains(word) {
            s += add;
        }
    }
    Some(s)
}

/// The best Windows game file of a release reply.
pub fn pick(release: &Value) -> Result<Asset> {
    let tag = release.get("tag_name").and_then(|t| t.as_str()).unwrap_or("").to_string();
    let assets = release.get("assets").and_then(|a| a.as_array()).cloned().unwrap_or_default();
    let names: Vec<String> = assets.iter().filter_map(|a| a.get("name")?.as_str().map(String::from)).collect();
    let best = assets
        .iter()
        .filter_map(|a| {
            let name = a.get("name")?.as_str()?;
            Some((score(name)?, a))
        })
        .max_by_key(|(s, _)| *s);
    let Some((_, a)) = best else {
        if names.is_empty() {
            bail!("release {tag} has no files to download (only source code)");
        }
        bail!("no Windows game download in release {tag}; it has: {}", names.join(", "));
    };
    Ok(Asset {
        name: a.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        url: a.get("browser_download_url").and_then(|v| v.as_str()).context("GitHub gave no download address")?.to_string(),
        size: a.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
        tag,
    })
}

/// The release a link means (the latest, or its tag) and the file to download.
pub fn resolve(owner: &str, repo: &str, tag: Option<&str>) -> Result<Asset> {
    let api = match tag {
        Some(t) => format!("https://api.github.com/repos/{owner}/{repo}/releases/tags/{t}"),
        None => format!("https://api.github.com/repos/{owner}/{repo}/releases/latest"),
    };
    let r = ureq::get(&api)
        .set("User-Agent", UA)
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(20))
        .call();
    let r = match r {
        Ok(r) => r,
        Err(ureq::Error::Status(404, _)) => bail!("{owner}/{repo} has no {} on GitHub", tag.map_or("releases".to_string(), |t| format!("release {t}"))),
        Err(ureq::Error::Status(403, _)) | Err(ureq::Error::Status(429, _)) => {
            bail!("GitHub is limiting requests right now (60 an hour without an account); try again later")
        }
        Err(e) => return Err(crate::source::net_error("https://api.github.com", e)),
    };
    let v: Value = r.into_json().context("GitHub's answer isn't JSON")?;
    pick(&v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links() {
        let r = |u: &str| repo_of(u);
        assert_eq!(r("https://github.com/STJr/SRB2"), Some(("STJr".into(), "SRB2".into(), None)));
        assert_eq!(r("https://github.com/STJr/SRB2/releases/latest"), Some(("STJr".into(), "SRB2".into(), None)));
        assert_eq!(r("https://github.com/STJr/SRB2/releases/tag/SRB2_release_2.2.15"), Some(("STJr".into(), "SRB2".into(), Some("SRB2_release_2.2.15".into()))));
        assert_eq!(r("https://github.com/STJr/SRB2/releases/download/SRB2_release_2.2.15/SRB2-v2215-Full.zip"), None, "a file: direct");
        assert_eq!(r("https://github.com/STJr/SRB2/issues/1"), None);
        assert_eq!(r("https://gitlab.com/a/b"), None);
    }

    #[test]
    fn picks_the_full_windows_zip() {
        let rel = serde_json::json!({
            "tag_name": "SRB2_release_2.2.15",
            "assets": [
                { "name": "SRB2-v2215-Installer.exe", "size": 1, "browser_download_url": "https://x/i.exe" },
                { "name": "SRB2-v2215-Full.zip", "size": 2, "browser_download_url": "https://x/f.zip" },
                { "name": "SRB2-v2215-Patch.zip", "size": 3, "browser_download_url": "https://x/p.zip" },
                { "name": "SRB2-v2215-macOS.dmg", "size": 4, "browser_download_url": "https://x/m.dmg" }
            ]
        });
        let a = pick(&rel).unwrap();
        assert_eq!((a.name.as_str(), a.tag.as_str(), a.size), ("SRB2-v2215-Full.zip", "SRB2_release_2.2.15", 2));
        assert!(a.stamp().contains("SRB2_release_2.2.15"));
        let src = serde_json::json!({ "tag_name": "v1", "assets": [] });
        assert!(pick(&src).unwrap_err().to_string().contains("only source code"));
        let mac = serde_json::json!({ "tag_name": "v1", "assets": [{ "name": "game-mac.dmg", "size": 1, "browser_download_url": "u" }] });
        assert!(pick(&mac).unwrap_err().to_string().contains("game-mac.dmg"));
    }
}
