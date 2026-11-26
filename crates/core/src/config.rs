//! Settings, kept in `~/.config/steam-fan/config.toml`.
//!
//! The shelf is just a folder. Usually it is the NAS share mounted on the PC
//! (`/mnt/nas-games`), but any folder works, for example an external disk or a
//! folder on the desktop. The box is this PC.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// Expand a leading `~/`.
pub fn expand(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        home().join(rest)
    } else if p == "~" {
        home()
    } else {
        PathBuf::from(p)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// The shelf: where every game is stored. NAS mount or any local folder.
    pub shelf: String,
    /// Where fan games are copied on this PC when you press Play.
    pub fan_local: String,
    /// Where other non-Steam games (demos, indie games) are copied.
    pub nonsteam_local: String,
    /// Where emulator games are copied.
    pub rom_local: String,
    /// Steam's own folder.
    pub steam_root: String,
    /// Proton version for Windows games. Empty: the one Steam uses for all games.
    pub proton: String,
    /// Optional: lets the app list every game you own, to show which ones aren't archived yet.
    pub steam_api_key: String,
    pub steam_id: String,
    /// How each emulator system on the shelf is played.
    pub emulators: Vec<Emulator>,
}

/// One system folder in `roms/` and the emulator that plays it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Emulator {
    /// Folder name under `roms/`, e.g. `wii`. Matched without regard to case.
    pub system: String,
    /// Subfolder holding the games, e.g. `GAMES`. Empty: games sit directly in the system folder.
    pub games_dir: String,
    /// File types the emulator starts. The first matching file in a game folder is used.
    pub extensions: Vec<String>,
    /// Command line. `{rom}` becomes the file to start, `{dir}` the game folder.
    pub command: Vec<String>,
    /// The emulator's save folders on this PC. Backed up to the shelf after you play.
    pub save_dirs: Vec<String>,
    /// Game folders whose name contains one of these are not games (updates, DLC, tools).
    pub skip: Vec<String>,
}

impl Default for Emulator {
    fn default() -> Self {
        Emulator {
            system: String::new(),
            games_dir: "GAMES".into(),
            extensions: vec![],
            command: vec![],
            save_dirs: vec![],
            skip: vec!["(UPDATE DATA)".into(), "(DLC)".into(), "Tools".into()],
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            shelf: "/mnt/nas-games".into(),
            fan_local: "~/Games/NonSteam".into(),
            nonsteam_local: "~/Games/NonSteam/Non-Steam Games".into(),
            rom_local: "~/Games/Emulation".into(),
            steam_root: "~/.local/share/Steam".into(),
            proton: String::new(),
            steam_api_key: String::new(),
            steam_id: String::new(),
            emulators: default_emulators(),
        }
    }
}

pub fn default_emulators() -> Vec<Emulator> {
    let e = |system: &str, ext: &[&str], cmd: &[&str], saves: &[&str]| Emulator {
        system: system.into(),
        extensions: ext.iter().map(|s| s.to_string()).collect(),
        command: cmd.iter().map(|s| s.to_string()).collect(),
        save_dirs: saves.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    vec![
        e(
            "wii",
            &["iso", "wbfs", "rvz", "gcz", "ciso", "wad", "dol", "elf"],
            &["dolphin-emu", "-b", "-e", "{rom}"],
            &["~/.local/share/dolphin-emu/Wii/title", "~/.local/share/dolphin-emu/GC"],
        ),
        e(
            "wiiu",
            &["rpx", "wud", "wux", "wua"],
            &["cemu", "-g", "{rom}"],
            &["~/.local/share/Cemu/mlc01/usr/save"],
        ),
        Emulator {
            games_dir: "Games".into(),
            ..e(
                "switch",
                &["nsp", "xci", "nca", "nro"],
                &["ryujinx", "{rom}"],
                &["~/.config/Ryujinx/bis/user/save"],
            )
        },
    ]
}

impl Config {
    pub fn path() -> PathBuf {
        dirs::config_dir().unwrap_or_else(|| home().join(".config")).join("steam-fan/config.toml")
    }

    pub fn load() -> Result<Config> {
        let p = Self::path();
        match std::fs::read_to_string(&p) {
            Ok(s) => toml::from_str(&s).with_context(|| format!("{} has a mistake", p.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e).with_context(|| format!("cannot read {}", p.display())),
        }
    }

    pub fn save(&self) -> Result<()> {
        let p = Self::path();
        std::fs::create_dir_all(p.parent().unwrap())?;
        let tmp = p.with_extension("toml.tmp");
        std::fs::write(&tmp, toml::to_string_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // the file may hold a Steam API key
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(tmp, p)?;
        Ok(())
    }

    pub fn shelf(&self) -> PathBuf {
        expand(&self.shelf)
    }
    pub fn steam_root(&self) -> PathBuf {
        expand(&self.steam_root)
    }
    pub fn emulator(&self, system: &str) -> Option<&Emulator> {
        self.emulators.iter().find(|e| e.system.eq_ignore_ascii_case(system))
    }

    /// Is the shelf there right now? A NAS that is off shows as an empty or missing folder.
    pub fn shelf_online(&self) -> bool {
        shelf_online(&self.shelf())
    }
}

pub fn shelf_online(shelf: &Path) -> bool {
    std::fs::read_dir(shelf).map(|mut d| d.next().is_some()).unwrap_or(false)
}

/// Where Steam-Fan keeps its database, icons and log on this PC.
pub fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| home().join(".local/share")).join("steam-fan")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_round_trip() {
        let c = Config::default();
        let s = toml::to_string_pretty(&c).unwrap();
        assert_eq!(toml::from_str::<Config>(&s).unwrap(), c);
        // a partial file still loads
        let c2: Config = toml::from_str("shelf = \"/data\"").unwrap();
        assert_eq!(c2.shelf, "/data");
        assert_eq!(c2.emulators.len(), 3);
    }
}

// ---------------------------------------------------------------- secrets

/// API keys live in `~/.config/steam-fan/secrets.env`, one `NAME=value` per line,
/// readable only by you. An environment variable with the same name wins, so a
/// key can also come from your shell or a systemd unit instead of a file.
///
/// Names: `STEAMGRIDDB_API_KEY`, `STEAM_API_KEY`, `STEAM_ID`.
pub mod secrets {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    pub const STEAMGRIDDB: &str = "STEAMGRIDDB_API_KEY";
    pub const STEAM_API: &str = "STEAM_API_KEY";
    pub const STEAM_ID: &str = "STEAM_ID";

    pub fn path() -> PathBuf {
        super::Config::path().with_file_name("secrets.env")
    }

    fn read_file() -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for line in std::fs::read_to_string(path()).unwrap_or_default().lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            if let Some((k, v)) = line.split_once('=') {
                let v = v.trim().trim_matches('"').trim_matches('\'');
                out.insert(k.trim().to_string(), v.to_string());
            }
        }
        out
    }

    /// The value from the environment, else from the file, else empty.
    pub fn get(name: &str) -> String {
        std::env::var(name).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| read_file().remove(name).unwrap_or_default())
    }

    /// Where a key currently comes from, for the settings page.
    pub fn source(name: &str) -> &'static str {
        if std::env::var(name).map_or(false, |v| !v.is_empty()) {
            "environment"
        } else if read_file().contains_key(name) {
            "secrets.env"
        } else {
            "not set"
        }
    }

    /// Store a key in the file (empty value removes it). The file is made readable only by you.
    pub fn set(name: &str, value: &str) -> anyhow::Result<()> {
        let mut all = read_file();
        if value.trim().is_empty() {
            all.remove(name);
        } else {
            all.insert(name.to_string(), value.trim().to_string());
        }
        let p = path();
        std::fs::create_dir_all(p.parent().unwrap())?;
        let mut text = String::from("# Steam-Fan API keys. Keep this file private.\n");
        for (k, v) in &all {
            text.push_str(&format!("{k}={v}\n"));
        }
        let tmp = p.with_extension("env.tmp");
        std::fs::write(&tmp, text)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(tmp, p)?;
        Ok(())
    }

    /// `abcd…wxyz`, for showing that a key is set without showing it.
    pub fn masked(name: &str) -> String {
        let v = get(name);
        if v.len() <= 8 {
            return if v.is_empty() { String::new() } else { "••••".into() };
        }
        format!("{}…{}", &v[..4], &v[v.len() - 4..])
    }
}

impl Config {
    /// Steam Web API key: secrets file / environment first, then the old settings field.
    pub fn steam_api_key(&self) -> String {
        let s = secrets::get(secrets::STEAM_API);
        if s.is_empty() { self.steam_api_key.clone() } else { s }
    }
    pub fn steam_id(&self) -> String {
        let s = secrets::get(secrets::STEAM_ID);
        if s.is_empty() { self.steam_id.clone() } else { s }
    }
}
