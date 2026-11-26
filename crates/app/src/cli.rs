//! The terminal commands. Each returns the exit code.

use std::os::unix::process::CommandExt;
use steam_fan_core::{archive, import, library, steam, transfer, Config, Db, Game, Kind};

fn setup() -> Result<(Config, Db), i32> {
    let cfg = Config::load().map_err(|e| {
        eprintln!("steam-fan: {e:#}");
        2
    })?;
    let db = Db::open_default().map_err(|e| {
        eprintln!("steam-fan: cannot open the library database: {e:#}");
        2
    })?;
    Ok((cfg, db))
}

fn fail(e: anyhow::Error) -> i32 {
    eprintln!("steam-fan: {e:#}");
    1
}

pub fn sync(dry_run: bool, only: &[String], rescan: bool) -> i32 {
    let Ok((cfg, mut db)) = setup() else { return 2 };
    let res = library::sync_to_steam(&cfg, &mut db, only, rescan, dry_run, |s| eprintln!("  {s} ..."));
    match res {
        Ok(out) => {
            for e in &out.refresh.errors {
                eprintln!("  ! could not read {e}");
            }
            for id in &out.steam.added {
                println!("  + {id}");
            }
            for id in &out.steam.updated {
                println!("  ~ {id}");
            }
            for id in &out.steam.removed {
                println!("  - {id}");
            }
            for id in &out.not_playable {
                println!("  ! no program found, skipped: {id}");
            }
            println!("\n{} (Windows games use {})", out.steam.summary(), out.steam.proton);
            if dry_run {
                println!("Dry run: nothing written.");
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Used by the Steam menu entry: update the library first, quietly, then start Steam.
pub fn steam(args: &[String]) -> i32 {
    if let Ok((cfg, mut db)) = setup() {
        if !steam::running() && shelf_reachable(&cfg) {
            match library::sync_to_steam(&cfg, &mut db, &[], false, false, |_| {}) {
                Ok(out) if !out.steam.is_empty() => steam_fan_core::notify("Steam-Fan", &out.steam.summary()),
                Ok(_) => {}
                Err(e) => steam_fan_core::notify("Steam-Fan: library not updated", &format!("{e:#}")),
            }
        }
    }
    let err = std::process::Command::new("steam").args(args).exec();
    eprintln!("steam-fan: cannot start steam: {err}");
    1
}

/// A NAS that is off can make the mount hang; give it 15 seconds.
fn shelf_reachable(cfg: &Config) -> bool {
    let shelf = cfg.shelf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(steam_fan_core::config::shelf_online(&shelf));
    });
    rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap_or(false)
}

pub fn find(db: &Db, what: &str) -> anyhow::Result<Game> {
    let games = db.games()?;
    if let Some(g) = games.iter().find(|g| g.id == what) {
        return Ok(g.clone());
    }
    let l = what.to_lowercase();
    let hits: Vec<&Game> = games.iter().filter(|g| g.name.to_lowercase() == l).collect();
    let hits = if hits.len() == 1 { hits } else { games.iter().filter(|g| g.id.to_lowercase().contains(&l)).collect() };
    match hits.len() {
        0 => anyhow::bail!("no game matches '{what}' (see: steam-fan list)"),
        1 => Ok(hits[0].clone()),
        _ => {
            for h in &hits {
                eprintln!("  {}", h.id);
            }
            anyhow::bail!("several games match '{what}'; be more specific")
        }
    }
}

pub fn list(only_installed: bool) -> i32 {
    let Ok((cfg, db)) = setup() else { return 2 };
    match db.games() {
        Ok(games) => {
            for g in games {
                let on_pc = library::is_installed(&cfg, &g);
                if only_installed && !on_pc {
                    continue;
                }
                println!("{:<4} {:<15} {}", if on_pc { "PC" } else { "" }, g.kind.prefix(), g.id);
            }
            0
        }
        Err(e) => fail(e),
    }
}

fn bar(p: &transfer::Progress) {
    eprint!(
        "\r  {:>3.0}%  {} of {}   ",
        p.fraction() * 100.0,
        transfer::human(p.done_bytes),
        transfer::human(p.total_bytes)
    );
}

pub fn install(what: &str) -> i32 {
    let Ok((cfg, db)) = setup() else { return 2 };
    let g = match find(&db, what) {
        Ok(g) => g,
        Err(e) => return fail(e),
    };
    let mut games = vec![g];
    if let Err(e) = library::find_programs(&cfg, &db, &mut games, &[], false, |_| {}) {
        return fail(e);
    }
    let g = &games[0];
    eprintln!("Copying {} from the shelf", g.name);
    match library::install(&cfg, &db, g, &transfer::Cancel::default(), &mut bar) {
        Ok(()) => {
            eprintln!("\nDone: {} is on this PC.", g.name);
            0
        }
        Err(e) => fail(e),
    }
}

pub fn remove(what: &str) -> i32 {
    let Ok((cfg, db)) = setup() else { return 2 };
    match find(&db, what).and_then(|g| library::remove(&cfg, &db, &g).map(|_| g)) {
        Ok(g) => {
            println!("Removed {} from this PC. Saves are on the shelf; it copies again when you press Play.", g.name);
            0
        }
        Err(e) => fail(e),
    }
}

pub fn archive_list() -> i32 {
    let Ok((cfg, db)) = setup() else { return 2 };
    let games = match db.games() {
        Ok(g) => g,
        Err(e) => return fail(e),
    };
    for g in games.iter().filter(|g| g.kind == Kind::Steam) {
        let pc = steam::installed(&cfg.steam_root(), g.appid).map(|(_, m)| m);
        let shelf = archive::shelf_manifest(&cfg, g.appid);
        let f = archive::freshness(shelf.as_ref(), pc.as_ref());
        println!("{:<10} {:<18} {}", g.appid, f.label(), g.name);
    }
    0
}

pub fn archive_restore(appid: u32) -> i32 {
    let Ok((cfg, _db)) = setup() else { return 2 };
    eprintln!("Installing {appid} from the shelf into Steam");
    match archive::restore(&cfg, appid, None, &transfer::Cancel::default(), &mut bar) {
        Ok(()) => {
            eprintln!("\nDone. Start Steam: the game shows as installed.");
            0
        }
        Err(e) => fail(e),
    }
}

pub fn archive_upload(appid: u32) -> i32 {
    let Ok((cfg, _db)) = setup() else { return 2 };
    eprintln!("Copying {appid} from this PC to the shelf");
    match archive::upload(&cfg, appid, &transfer::Cancel::default(), &mut bar) {
        Ok(()) => {
            eprintln!("\nDone.");
            0
        }
        Err(e) => fail(e),
    }
}

/// Open the library window, which is its own program next to this one.
pub fn open_window() -> i32 {
    let me = std::env::current_exe().ok();
    let beside = me.as_ref().and_then(|p| p.parent()).map(|d| d.join("steam-fan-app"));
    let prog = beside.filter(|p| p.exists()).map(|p| p.into_os_string()).unwrap_or_else(|| "steam-fan-app".into());
    let err = std::process::Command::new(&prog).exec();
    eprintln!("steam-fan: cannot open the window ({err}). Run ./install.sh in the Steam-Fan folder.");
    1
}

pub fn add(source: &str, kind: &str, group: &str, name: Option<&str>) -> i32 {
    let Ok((cfg, mut db)) = setup() else { return 2 };
    let src = std::path::PathBuf::from(source);
    let info = match import::inspect(&cfg, &src) {
        Ok(i) => i,
        Err(e) => return fail(e),
    };
    let kind = match kind {
        "" => info.kind,
        k => match Kind::from_prefix(k) {
            Some(k) => k,
            None => {
                eprintln!("steam-fan: --kind must be fan, nonsteam or rom");
                return 2;
            }
        },
    };
    let name = name.map(str::to_string).unwrap_or(info.name.clone());
    eprintln!("Adding {name} as {} in {group}", kind.label());
    let res = import::place(&cfg, &src, kind, group, &name, &transfer::Cancel::default(), &mut |s| eprintln!("  {s} ..."), &mut bar);
    match res {
        Ok(dst) => {
            eprintln!("\nOn the shelf at {}", dst.display());
            if let Err(e) = library::refresh(&cfg, &mut db, false, |_| {}) {
                return fail(e);
            }
            eprintln!("Run `steam-fan sync` (Steam closed) to put it in Steam.");
            0
        }
        Err(e) => fail(e),
    }
}

pub fn art(again: bool) -> i32 {
    let Ok((cfg, db)) = setup() else { return 2 };
    let games = match db.games() {
        Ok(g) => g,
        Err(e) => return fail(e),
    };
    let rep = library::refresh_art(&cfg, &db, &games, true, again, |s| eprintln!("  {s}"));
    for e in &rep.errors {
        eprintln!("  ! {e}");
    }
    println!("{} copied from the shelf, {} found on SteamGridDB, {} with nothing there", rep.copied, rep.fetched, rep.not_found);
    0
}
