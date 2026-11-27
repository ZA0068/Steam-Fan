//! The Steam archive on the shelf (`steam/<appid>/`, kept up to date by SteamCMD on the NAS).
//!
//! When Steam's servers are down, a game can be installed from the shelf into
//! Steam's library: Steam then sees it as installed and starts it in offline
//! mode. When Steam is back, Steam updates it as usual. A game updated on this
//! PC can also be copied to the shelf, so the shelf is never older than the PC.

use crate::config::Config;
use crate::steam::{self, Manifest};
use crate::transfer::{self, Cancel, Progress};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    OnlyShelf,
    OnlyPc,
    Same,
    ShelfNewer,
    PcNewer,
    Unknown,
}

impl Freshness {
    pub fn label(self) -> &'static str {
        match self {
            Freshness::OnlyShelf => "on shelf",
            Freshness::OnlyPc => "on PC only",
            Freshness::Same => "up to date",
            Freshness::ShelfNewer => "shelf is newer",
            Freshness::PcNewer => "PC is newer",
            Freshness::Unknown => "different version",
        }
    }
}

pub fn shelf_manifest(cfg: &Config, appid: u32) -> Option<Manifest> {
    steam::read_manifest(&cfg.shelf().join(format!("steam/{appid}/steamapps/appmanifest_{appid}.acf")))
}

/// Build ids only grow, so the bigger one is the newer version.
pub fn freshness(shelf: Option<&Manifest>, pc: Option<&Manifest>) -> Freshness {
    match (shelf, pc) {
        (Some(_), None) => Freshness::OnlyShelf,
        (None, Some(_)) => Freshness::OnlyPc,
        (None, None) => Freshness::Unknown,
        (Some(s), Some(p)) => match (s.buildid.parse::<u64>(), p.buildid.parse::<u64>()) {
            (Ok(a), Ok(b)) if a == b => Freshness::Same,
            (Ok(a), Ok(b)) if a > b => Freshness::ShelfNewer,
            (Ok(_), Ok(_)) => Freshness::PcNewer,
            _ => Freshness::Unknown,
        },
    }
}

/// The library to install into: the one already holding the game, else the first.
fn pick_library(cfg: &Config, appid: u32, wanted: Option<&Path>) -> Result<PathBuf> {
    if let Some(w) = wanted {
        return Ok(w.to_path_buf());
    }
    if let Some((lib, _)) = steam::installed(&cfg.steam_root(), appid) {
        return Ok(lib);
    }
    steam::libraries(&cfg.steam_root()).into_iter().find(|l| l.join("steamapps").is_dir()).context("no Steam library found")
}

/// Steam games' archives go through the shelf folder, also when fan games come from
/// Steam-Fan-Server: it has to be mounted for these.
fn folder_needed(cfg: &Config) -> Result<()> {
    if cfg.server_mode() && !crate::config::shelf_online(&cfg.shelf()) {
        bail!("archived Steam games need the shelf folder ({}) mounted on this PC; Steam-Fan-Server only serves fan, non-Steam and emulator games", cfg.shelf().display());
    }
    Ok(())
}

/// Install or update a game from the shelf into Steam's library. Steam must be closed.
pub fn restore(cfg: &Config, appid: u32, library: Option<&Path>, cancel: &Cancel, cb: &mut dyn FnMut(&Progress)) -> Result<()> {
    if steam::running() {
        bail!("Steam is running. Quit Steam first: it only notices new games when it starts.");
    }
    folder_needed(cfg)?;
    let m = shelf_manifest(cfg, appid).context("this game isn't in the archive on the shelf")?;
    if m.installdir.is_empty() {
        bail!("the archive manifest has no install folder");
    }
    let lib = pick_library(cfg, appid, library)?;
    let src = cfg.shelf().join(format!("steam/{appid}"));
    let dst = lib.join("steamapps/common").join(&m.installdir);
    let need = transfer::dir_size(&src);
    if let Some(free) = transfer::free_space(&lib) {
        if need > free {
            bail!("not enough space: {} needs {}, {} free", m.name, transfer::human(need), transfer::human(free));
        }
    }
    transfer::copy_tree(&src, &dst, &|r: &Path| r.starts_with("steamapps"), cancel, cb)?;
    std::fs::copy(
        src.join(format!("steamapps/appmanifest_{appid}.acf")),
        lib.join(format!("steamapps/appmanifest_{appid}.acf")),
    )?;
    Ok(())
}

/// Copy a game from Steam's library on this PC to the shelf, when the PC has the newer version.
pub fn upload(cfg: &Config, appid: u32, cancel: &Cancel, cb: &mut dyn FnMut(&Progress)) -> Result<()> {
    let (lib, m) = steam::installed(&cfg.steam_root(), appid).context("this game isn't installed on this PC")?;
    folder_needed(cfg)?;
    if !cfg.shelf_online() {
        bail!("the shelf isn't reachable");
    }
    let src = lib.join("steamapps/common").join(&m.installdir);
    let dst = cfg.shelf().join(format!("steam/{appid}"));
    transfer::copy_tree(&src, &dst, &|_| false, cancel, cb)?;
    std::fs::create_dir_all(dst.join("steamapps"))?;
    std::fs::copy(
        lib.join(format!("steamapps/appmanifest_{appid}.acf")),
        dst.join(format!("steamapps/appmanifest_{appid}.acf")),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn m(b: &str) -> Manifest {
        Manifest { buildid: b.into(), ..Default::default() }
    }
    #[test]
    fn versions() {
        assert_eq!(freshness(Some(&m("10")), Some(&m("10"))), Freshness::Same);
        assert_eq!(freshness(Some(&m("11")), Some(&m("10"))), Freshness::ShelfNewer);
        assert_eq!(freshness(Some(&m("9")), Some(&m("10"))), Freshness::PcNewer);
        assert_eq!(freshness(Some(&m("9")), None), Freshness::OnlyShelf);
    }
}
