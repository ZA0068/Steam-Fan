//! Save backups on the shelf. Nothing on the shelf is ever deleted by a backup.
//!
//! Fan and non-Steam games, in `saves/<fangames|nonsteam>/<game>/`:
//! * `game/`   files in the game folder that differ from the shelf original (saves, settings)
//! * `prefix/` the Windows user folder of the game's Proton prefix (AppData, Documents, Saved Games)
//!
//! Emulator games, in `saves/emulators/<system>/<n>/`: the emulator's save
//! folders from the settings, kept in sync both ways (the newest file wins).
//!
//! With Steam-Fan-Server the same layout lives in this PC's copy of the server's save
//! area (`Config::saves_root`): newer files come from the server first, and go back after.

use crate::config::{expand, Config};
use crate::shelf::{Game, Kind};
use crate::transfer;
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

pub const MARKER: &str = ".steam-fan";
pub const OLD_MARKER: &str = ".nas-complete";
pub const PREFIX_FILE: &str = ".steam-fan-prefix";
pub const OLD_PREFIX_FILE: &str = ".nas-prefix";

fn our_files(rel: &Path) -> bool {
    let s = rel.to_string_lossy();
    [MARKER, OLD_MARKER, PREFIX_FILE, OLD_PREFIX_FILE].contains(&s.as_ref()) || s.ends_with(".steam-fan-part")
}

fn temp_files(rel: &Path) -> bool {
    rel.components().any(|c| c.as_os_str().eq_ignore_ascii_case("Temp"))
}

/// The Proton prefix the game ran in last time, if any.
pub fn prefix_user_dir(local: &Path) -> Option<PathBuf> {
    let f = [PREFIX_FILE, OLD_PREFIX_FILE].iter().map(|f| local.join(f)).find(|p| p.exists())?;
    let pfx = std::fs::read_to_string(f).ok()?.trim().to_string();
    Some(Path::new(&pfx).join("pfx/drive_c/users/steamuser"))
}

/// The shelf original to compare against when looking for changed files.
fn original(cfg: &Config, g: &Game) -> PathBuf {
    let s = g.shelf_path(cfg);
    if s.is_file() { s.parent().unwrap().to_path_buf() } else { s }
}

pub fn backup(cfg: &Config, g: &Game) -> Result<()> {
    if !cfg.shelf_online() {
        bail!("the shelf isn't reachable, so saves were not backed up");
    }
    let local = g.local_path(cfg);
    let saves = g.save_path(cfg);
    match g.kind {
        Kind::Fan | Kind::NonSteam => {
            let changed = if cfg.server_mode() {
                crate::remote::changed_files(cfg, g, &local, &our_files)?
            } else {
                transfer::changed_files(&local, &original(cfg, g), &our_files)?
            };
            transfer::copy_listed(&local, &saves.join("game"), &changed)?;
            if let Some(user) = prefix_user_dir(&local).filter(|p| p.is_dir()) {
                transfer::copy_newer(&user, &saves.join("prefix"), &temp_files)?;
            }
        }
        Kind::Rom => {
            if let Some(emu) = cfg.emulator(&g.system) {
                for (i, d) in emu.save_dirs.iter().enumerate() {
                    transfer::copy_newer(&expand(d), &saves.join(i.to_string()), &|_| false)?;
                }
            }
        }
        Kind::Steam => {}
    }
    if cfg.server_mode() {
        crate::remote::push_saves(cfg, g)?;
    }
    Ok(())
}

/// With a server: bring its newer save files to this PC first.
fn pull(cfg: &Config, g: &Game) -> Result<()> {
    if cfg.server_mode() {
        crate::remote::pull_saves(cfg, g)?;
    }
    Ok(())
}

/// Put the game-folder saves back after a fresh download.
pub fn restore_game_folder(cfg: &Config, g: &Game) -> Result<u64> {
    pull(cfg, g)?;
    let src = g.save_path(cfg).join("game");
    if !src.is_dir() {
        return Ok(0);
    }
    let n = transfer::copy_tree(&src, &g.local_path(cfg), &|_| false, &transfer::Cancel::default(), &mut |_| {})?;
    Ok(n.total_files)
}

/// Put the saves from the shelf into a Proton prefix.
pub fn restore_prefix(cfg: &Config, g: &Game, user_dir: &Path) -> Result<u64> {
    transfer::copy_newer(&g.save_path(cfg).join("prefix"), user_dir, &|_| false)
}

pub fn has_prefix_saves(cfg: &Config, g: &Game) -> bool {
    let _ = pull(cfg, g);
    g.save_path(cfg).join("prefix").is_dir()
}

/// Before an emulator starts: bring newer saves from the shelf.
pub fn restore_emulator(cfg: &Config, g: &Game) -> Result<u64> {
    let Some(emu) = cfg.emulator(&g.system) else { return Ok(0) };
    pull(cfg, g)?;
    let mut n = 0;
    for (i, d) in emu.save_dirs.iter().enumerate() {
        n += transfer::copy_newer(&g.save_path(cfg).join(i.to_string()), &expand(d), &|_| false)?;
    }
    Ok(n)
}
