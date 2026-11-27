//! Upstream updates: every few hours the server asks each game's download link what it
//! serves now and compares that with what was downloaded last time.
//! With `SF_AUTO_UPDATE=replace` a changed file is downloaded and replaces the shelf copy
//! (saves stay; PCs get the new files on their next Play).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use steam_fan_core::{artwork, source, Game};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    /// The link serves the same file as last time.
    Current,
    /// A different file is at the link: probably a new version.
    New,
    /// This kind of link can't be checked (MEGA, a web page), or nothing was downloaded from it yet.
    Unknown,
    /// The link didn't answer.
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Update {
    pub state: State,
    pub msg: String,
    pub checked: u64,
}

pub struct Updates {
    path: PathBuf,
    map: Mutex<BTreeMap<String, Update>>,
}

/// Compare what was downloaded last time (`had`) with what the link serves now.
pub fn judge(had: &str, now: Option<String>) -> (State, String) {
    match now {
        None => (State::Unknown, "this kind of link (MEGA, a web page) can't be checked; open it to see".into()),
        Some(n) if had.is_empty() => (State::Unknown, format!("nothing was downloaded from this link by Steam-Fan yet; it serves: {n}")),
        Some(n) if n == had => (State::Current, "up to date: the link still serves the same file".into()),
        Some(_) => (State::New, "a different file is at the link now: probably a new version".into()),
    }
}

impl Updates {
    pub fn load(path: &Path) -> Updates {
        let map = std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Updates { path: path.to_path_buf(), map: Mutex::new(map) }
    }

    fn save(&self, map: &BTreeMap<String, Update>) {
        if let Ok(text) = serde_json::to_string_pretty(map) {
            let tmp = self.path.with_extension("tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(tmp, &self.path);
            }
        }
    }

    pub fn all(&self) -> BTreeMap<String, Update> {
        self.map.lock().unwrap().clone()
    }

    pub fn get(&self, id: &str) -> Option<Update> {
        self.map.lock().unwrap().get(id).cloned()
    }

    pub fn set(&self, id: &str, u: Update) {
        let mut m = self.map.lock().unwrap();
        m.insert(id.to_string(), u);
        self.save(&m);
    }

    /// The game was downloaded again: whatever we knew is old news.
    pub fn clear(&self, id: &str) {
        let mut m = self.map.lock().unwrap();
        if m.remove(id).is_some() {
            self.save(&m);
        }
    }

    /// Ask the game's link. None: the game has no link.
    pub fn check(&self, g: &Game) -> Option<Update> {
        let info = artwork::info(g);
        let url = info.download.trim();
        if url.is_empty() {
            return None;
        }
        let (state, msg) = match source::remote_stamp(url) {
            Ok(now) => judge(&info.download_stamp, now),
            Err(e) => (State::Error, format!("{e:#}")),
        };
        let u = Update { state, msg, checked: crate::jobs::now() };
        self.set(&g.id, u.clone());
        Some(u)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn judging() {
        assert_eq!(judge("a | 5", Some("a | 5".into())).0, State::Current);
        assert_eq!(judge("a | 5", Some("b | 6".into())).0, State::New);
        assert_eq!(judge("", Some("b".into())).0, State::Unknown);
        assert_eq!(judge("a", None).0, State::Unknown);
        let t = tempfile::tempdir().unwrap();
        let u = Updates::load(&t.path().join("u.json"));
        u.set("fan:x", Update { state: State::New, msg: String::new(), checked: 1 });
        assert_eq!(Updates::load(&t.path().join("u.json")).get("fan:x").unwrap().state, State::New);
        u.clear("fan:x");
        assert!(Updates::load(&t.path().join("u.json")).get("fan:x").is_none());
    }
}
