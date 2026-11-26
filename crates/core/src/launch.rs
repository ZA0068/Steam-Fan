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
    // moved or renamed since Steam's library was last updated: Steam still passes the old id
    let renamed = db.renames().ok().and_then(|r| r.get(id).cloned());
    let id = renamed.as_deref().unwrap_or(id);
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

/// Where Proton's log of a game's last run is kept.
pub fn log_dir(g: &Game) -> std::path::PathBuf {
    crate::config::data_dir().join("logs").join(format!("{:08x}", crc32fast::hash(g.id.as_bytes())))
}

/// The lines of the last Proton log that say what went wrong (errors, crashes), at most `n`.
pub fn log_summary(g: &Game, n: usize) -> Option<(std::path::PathBuf, Vec<String>)> {
    let newest = std::fs::read_dir(log_dir(g)).ok()?.flatten().max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())?.path();
    let text = String::from_utf8_lossy(&std::fs::read(&newest).ok()?).into_owned();
    let interesting = |l: &str| {
        let low = l.to_lowercase();
        (low.contains("err:") || low.contains("unhandled exception") || low.contains("page fault") || low.contains("could not load") || low.contains("not found") || low.contains("fatal"))
            && !low.contains("xalia")
    };
    let mut lines: Vec<String> = text.lines().filter(|l| interesting(l)).map(|l| l.chars().take(220).collect()).collect();
    lines.dedup();
    let keep = lines.split_off(lines.len().saturating_sub(n));
    Some((newest, keep))
}

/// Is the game running from this folder? Looks for programs started from it (also through
/// Proton, which names them `Z:\\home\\...`) or working in it. Shells are ignored: a terminal
/// that merely mentions the folder isn't the game.
pub fn game_running(dir: &Path) -> bool {
    let unix = dir.to_string_lossy().into_owned();
    let windows = format!("Z:{}", unix.replace('/', "\\")).to_lowercase();
    let Ok(rd) = std::fs::read_dir("/proc") else { return false };
    let me = std::process::id().to_string();
    rd.flatten().filter(|e| e.file_name().to_string_lossy() != me).any(|e| {
        let comm = std::fs::read_to_string(e.path().join("comm")).unwrap_or_default();
        if matches!(comm.trim(), "bash" | "zsh" | "sh" | "fish" | "dash" | "python3" | "steam-fan" | "steam-fan-app") {
            return false;
        }
        let in_dir = std::fs::read_link(e.path().join("cwd")).map_or(false, |c| c.starts_with(dir));
        in_dir
            || std::fs::read(e.path().join("cmdline")).map_or(false, |c| {
                let c = String::from_utf8_lossy(&c).replace('\0', " ");
                c.contains(&unix) || c.to_lowercase().contains(&windows)
            })
    })
}

/// Start a fan or non-Steam game. `cmd` is what Steam passed as %command%.
pub fn run_program(cfg: &Config, db: &Db, g: &Game, cmd: &[String]) -> Result<i32> {
    if cmd.is_empty() {
        bail!("Steam passed no command; the launch options must end with %command%");
    }
    if cmd.len() == 1 && cmd[0] == "%command%" {
        bail!(
            "this line only works when Steam runs it: Steam replaces %command% with the command that starts \
             the game through Proton. To start the game, press Play in Steam (or in Steam-Fan)."
        );
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
    // Start in the program's own folder. Steam would use the shortcut's "Start in"
    // folder, but on the first Play that folder didn't exist yet (the game was only
    // just copied), so Steam fell back to its own folder and games that load files
    // next to themselves (GameMaker, ...) couldn't find them.
    let exe = local.join(&g.program);
    let dir = if exe.is_file() { exe.parent().map(Path::to_path_buf).unwrap_or(local.clone()) } else { local.clone() };
    // Proton writes what went wrong into a log; keep the last run of each game
    let mut command = Command::new(&cmd[0]);
    command.args(&cmd[1..]).current_dir(&dir);
    if std::env::var_os("PROTON_LOG").is_none() {
        let logdir = log_dir(g);
        let _ = std::fs::create_dir_all(&logdir);
        if let Ok(rd) = std::fs::read_dir(&logdir) {
            for e in rd.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        command.env("PROTON_LOG", "1").env("PROTON_LOG_DIR", &logdir);
    }
    let status = command.status().with_context(|| format!("cannot start {}", cmd[0]))?;
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
    let local = g.local_path(cfg);
    let status = Command::new(&cmd[0])
        .args(&cmd[1..])
        .current_dir(if local.is_dir() { &local } else { local.parent().unwrap_or(&local) })
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
