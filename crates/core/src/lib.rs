//! Steam-Fan core: the shelf (NAS) holds every game, the box (this PC) plays
//! them, and Steam is the one launcher for all of them.

pub mod archive;
pub mod artwork;
pub mod config;
pub mod db;
pub mod icons;
pub mod import;
pub mod integrity;
pub mod launch;
pub mod library;
pub mod saves;
pub mod shelf;
pub mod source;
pub mod steam;
pub mod transfer;
pub mod vdf;
pub mod webapi;

pub use config::Config;
pub use db::Db;
pub use shelf::{Game, Kind, Platform};

/// `20261124-201500`, for backups and markers.
pub fn now_stamp() -> String {
    let out = std::process::Command::new("date").arg("+%Y%m%d-%H%M%S").output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => format!(
            "{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
        ),
    }
}

/// A desktop notification, if `notify-send` is installed. Never fails.
pub fn notify(title: &str, body: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["-a", "Steam-Fan", "-i", "steam", title, body])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
