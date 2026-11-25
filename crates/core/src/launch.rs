//! What happens when you press Play in Steam.
//!
//! Steam runs `steam-fan launch "<id>" %command%`. The app first makes sure the
//! game is on this PC (copying it from the shelf if not; that part is in the
//! app so it can show a progress window), then this module starts it and backs
//! up the saves when it exits.

use crate::config::{expand, Config};
use crate::db::Db;
use crate::saves;
use crate::shelf::{Game, Kind};
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

/// Find a game by id, reading the shelf again if it isn't known yet.
pub fn find_game(cfg: &Config, db: &mut Db, id: &str) -> Result<Game> {
    let find = |db: &Db| -> Result<Option<Game>> {
        let mut games = db.games()?;
        let Some(i) = games.iter().position(|g| g.id == id) else { return Ok(None) };
        crate::library::find_programs(cfg, db, &mut games[i..=i], &[], false, |_| {})?;
        Ok(Some(games.swap_remove(i)))
    };
    if let Some(g) = find(db)? {
        return Ok(g);
    }
    if cfg.shelf_online() {
        crate::library::refresh(cfg, db, false, |_| {})?;
        if let Some(g) = find(db)? {
            return Ok(g);
        }
    }
    bail!("{id} isn't on the shelf (or the shelf isn't reachable)")
}

/// Is something running from this folder?
pub fn game_running(dir: &Path) -> bool {
    let needle = dir.to_string_lossy().into_owned();
    let Ok(rd) = std::fs::read_dir("/proc") else { return false };
    let me = std::process::id().to_string();
    rd.flatten().filter(|e| e.file_name().to_string_lossy() != me).any(|e| {
        std::fs::read(e.path().join("cmdline"))
            .map(|c| String::from_utf8_lossy(&c).replace('\0', " ").contains(&needle))
            .unwrap_or(false)
    })
}

/// Start a fan or non-Steam game. `cmd` is what Steam passed as %command%.
pub fn run_program(cfg: &Config, db: &Db, g: &Game, cmd: &[String]) -> Result<i32> {
    if cmd.is_empty() {
        bail!("Steam passed no command; the launch options must end with %command%");
    }
    let local = g.local_path(cfg);
    let mut restore_after = false;
    if let Ok(compat) = std::env::var("STEAM_COMPAT_DATA_PATH") {
        std::fs::write(local.join(saves::PREFIX_FILE), &compat)?;
        let user = Path::new(&compat).join("pfx/drive_c/users/steamuser");
        let done = Path::new(&compat).join(".steam-fan-restored");
        let done_old = Path::new(&compat).join(".nas-restored");
        if saves::has_prefix_saves(cfg, g) && !done.exists() && !done_old.exists() {
            if user.is_dir() {
                saves::restore_prefix(cfg, g, &user)?;
                std::fs::write(&done, "")?;
                db.log(&g.id, "saves restored into the Proton prefix");
            } else {
                restore_after = true; // Proton creates the prefix during this first start
            }
        }
    }
    db.log(&g.id, "started");
    let status = Command::new(&cmd[0]).args(&cmd[1..]).status().with_context(|| format!("cannot start {}", cmd[0]))?;
    let rc = status.code().unwrap_or(1);
    db.log(&g.id, &format!("exited ({rc})"));

    if restore_after {
        // first start on this PC: put the shelf saves into the new prefix instead of backing up empty ones
        if let Ok(compat) = std::env::var("STEAM_COMPAT_DATA_PATH") {
            let user = Path::new(&compat).join("pfx/drive_c/users/steamuser");
            if user.is_dir() {
                saves::restore_prefix(cfg, g, &user)?;
                std::fs::write(Path::new(&compat).join(".steam-fan-restored"), "")?;
                db.log(&g.id, "saves restored after the first start; they are there next time you play");
                crate::notify(&format!("{}: saves restored", g.name), "Your earlier saves are back from the next time you play.");
            }
        }
    } else if let Err(e) = saves::backup(cfg, g) {
        db.log(&g.id, &format!("saves not backed up: {e:#}"));
        crate::notify(&format!("{}: saves not backed up", g.name), "The shelf isn't reachable. They are copied next time you play while it's on.");
    } else {
        db.log(&g.id, "saves backed up");
    }
    Ok(rc)
}

/// The command line for an emulator game.
pub fn rom_command(cfg: &Config, g: &Game) -> Result<Vec<String>> {
    let emu = cfg.emulator(&g.system).with_context(|| format!("no emulator set for {}", g.system))?;
    if emu.command.is_empty() {
        bail!("the emulator for {} has no command in the settings", g.system);
    }
    let local = g.local_path(cfg);
    let rom = if local.is_dir() { local.join(&g.program) } else { local.clone() };
    Ok(emu
        .command
        .iter()
        .map(|a| {
            let a = a.replace("{rom}", &rom.to_string_lossy()).replace("{dir}", &local.to_string_lossy());
            expand(&a).to_string_lossy().into_owned()
        })
        .collect())
}

/// Start an emulator game: newest saves from the shelf first, back to the shelf after.
pub fn run_rom(cfg: &Config, db: &Db, g: &Game) -> Result<i32> {
    if g.kind != Kind::Rom {
        bail!("not an emulator game");
    }
    let cmd = rom_command(cfg, g)?;
    if cfg.shelf_online() {
        if let Ok(n) = saves::restore_emulator(cfg, g) {
            if n > 0 {
                db.log(&g.id, &format!("{n} newer save files taken from the shelf"));
            }
        }
    }
    db.log(&g.id, "started");
    let status = Command::new(&cmd[0])
        .args(&cmd[1..])
        .status()
        .with_context(|| format!("cannot start {}. Is it installed? (settings: emulator for {})", cmd[0], g.system))?;
    let rc = status.code().unwrap_or(1);
    db.log(&g.id, &format!("exited ({rc})"));
    match saves::backup(cfg, g) {
        Ok(()) => db.log(&g.id, "saves backed up"),
        Err(e) => db.log(&g.id, &format!("saves not backed up: {e:#}")),
    }
    Ok(rc)
}
