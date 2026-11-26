//! The whole library: reading the shelf, installing on the box, keeping Steam in step.

use crate::config::Config;
use crate::db::Db;
use crate::saves;
use crate::shelf::{self, Game, GroupScan, Kind, Platform};
use crate::steam::{self, Wanted};
use crate::transfer::{self, Cancel, Progress};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct RefreshReport {
    /// Top folders read again because they were new or changed.
    pub read: Vec<String>,
    pub errors: Vec<String>,
    /// False if a folder could not be read: then nothing is removed from Steam.
    pub complete: bool,
    pub games: usize,
}

/// Read the shelf. Folders unchanged since last time come from the database.
pub fn refresh(cfg: &Config, db: &mut Db, full: bool, mut on_read: impl FnMut(&str)) -> Result<RefreshReport> {
    let old = if full { HashMap::new() } else { db.groups()? };
    let shelf = cfg.shelf();
    let sources = shelf::sources(cfg).with_context(|| format!("cannot read the shelf at {}", shelf.display()))?;
    let mut new: HashMap<String, GroupScan> = HashMap::new();
    let mut rep = RefreshReport { complete: true, ..Default::default() };
    for src in sources {
        let key = src.key();
        match old.get(&key) {
            Some(g) if g.unchanged(&shelf) => {
                new.insert(key, g.clone());
            }
            _ => {
                on_read(&key);
                match shelf::read_source(cfg, &src) {
                    Ok(g) => {
                        new.insert(key.clone(), g);
                        rep.read.push(key);
                    }
                    Err(e) => {
                        rep.complete = false;
                        rep.errors.push(format!("{key}: {e}"));
                        if let Some(g) = old.get(&key) {
                            new.insert(key, g.clone());
                        }
                    }
                }
            }
        }
    }
    rep.games = new.values().map(|g| g.games.len()).sum();
    db.save_groups(&new)?;
    Ok(rep)
}

pub fn matches(g: &Game, only: &[String]) -> bool {
    only.is_empty() || only.iter().any(|o| g.id.to_lowercase().contains(&o.to_lowercase()) || g.name.to_lowercase().contains(&o.to_lowercase()))
}

/// Find the program (or ROM file) of games that don't have one yet. Deep walk, once per game.
pub fn find_programs(cfg: &Config, db: &Db, games: &mut [Game], only: &[String], again: bool, mut on_game: impl FnMut(&Game)) -> Result<()> {
    let known = db.programs()?;
    for g in games.iter_mut() {
        if g.kind == Kind::Steam || !matches(g, only) {
            continue;
        }
        if !again {
            if let Some((p, plat)) = known.get(&g.id) {
                g.program = p.clone();
                g.platform = *plat;
                continue;
            }
        }
        on_game(g);
        let path = g.shelf_path(cfg);
        let found = match g.kind {
            Kind::Rom => cfg.emulator(&g.system).and_then(|e| shelf::pick_rom(&path, e)).map(|p| (p, Platform::Emulator)),
            _ => shelf::pick_program(&path, &g.name),
        };
        let (p, plat) = found.unwrap_or((String::new(), g.platform));
        db.set_program(&g.id, &p, plat)?;
        g.program = p;
        g.platform = plat;
    }
    Ok(())
}

/// Is the game on this PC?
pub fn is_installed(cfg: &Config, g: &Game) -> bool {
    match g.kind {
        Kind::Steam => steam::installed(&cfg.steam_root(), g.appid).is_some(),
        _ => {
            let l = g.local_path(cfg);
            l.join(saves::MARKER).exists() || l.join(saves::OLD_MARKER).exists()
        }
    }
}

/// Path to this program, for Steam launch options.
pub fn self_exe() -> PathBuf {
    let installed = crate::config::home().join(".local/bin/steam-fan");
    if installed.exists() {
        return installed;
    }
    // the window program (steam-fan-app) sits next to steam-fan, which is what Steam must run
    let me = std::env::current_exe().unwrap_or_else(|_| installed.clone());
    let beside = me.with_file_name("steam-fan");
    if beside.exists() { beside } else { me }
}

/// The Steam library entry for a game.
pub fn wanted(cfg: &Config, g: &Game, icon: Option<PathBuf>) -> Option<Wanted> {
    if g.program.is_empty() || g.kind == Kind::Steam {
        return None;
    }
    let bin = self_exe();
    let local = g.local_path(cfg);
    let icon = icon.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    match g.kind {
        Kind::Rom => Some(Wanted {
            id: g.id.clone(),
            name: g.name.clone(),
            exe: bin.to_string_lossy().into_owned(),
            start_dir: local.to_string_lossy().into_owned(),
            launch_options: steam::launch_options(&bin, &g.id, false),
            tag: g.group.clone(),
            icon,
            proton: false,
        }),
        _ => {
            let exe = local.join(&g.program);
            Some(Wanted {
                id: g.id.clone(),
                name: g.name.clone(),
                exe: exe.to_string_lossy().into_owned(),
                start_dir: exe.parent().unwrap_or(&local).to_string_lossy().into_owned(),
                launch_options: steam::launch_options(&bin, &g.id, true),
                tag: g.group.clone(),
                icon,
                proton: g.platform == Platform::Windows,
            })
        }
    }
}

#[derive(Debug, Default)]
pub struct SyncOutcome {
    pub refresh: RefreshReport,
    pub steam: steam::SyncReport,
    pub not_playable: Vec<String>,
}

/// Read the shelf and update Steam's library. Steam must be closed (unless dry run).
pub fn sync_to_steam(
    cfg: &Config,
    db: &mut Db,
    only: &[String],
    again: bool,
    dry_run: bool,
    mut status: impl FnMut(&str),
) -> Result<SyncOutcome> {
    if !dry_run && steam::running() {
        bail!("Steam is running. Quit Steam (Steam > Exit) first, then try again.");
    }
    let refresh = refresh(cfg, db, again, |k| status(&format!("reading {k}")))?;
    let mut games = db.games()?;
    find_programs(cfg, db, &mut games, only, again, |g| status(&format!("looking for the program in {}", g.name)))?;
    let mut list = vec![];
    let mut not_playable = vec![];
    for g in games.iter().filter(|g| g.kind != Kind::Steam && matches(g, only)) {
        let icon = if dry_run {
            None
        } else {
            if !crate::artwork::mirrored(g) {
                crate::artwork::mirror(cfg, g);
            }
            crate::artwork::local(g, crate::artwork::Art::Icon).or_else(|| crate::icons::icon_for(cfg, g))
        };
        match wanted(cfg, g, icon) {
            Some(w) => list.push(w),
            None => not_playable.push(g.id.clone()),
        }
    }
    let prune = only.is_empty() && refresh.complete && !list.is_empty();
    let renames = db.renames().unwrap_or_default();
    let report = steam::sync_shortcuts_renamed(cfg, &list, &renames, prune, dry_run)?;
    if !dry_run && !renames.is_empty() {
        let _ = db.clear_renames();
    }
    if !dry_run {
        // the pictures go into Steam's artwork folder too (Steam reads them when it starts)
        let ids = steam::shortcut_ids(cfg);
        for g in games.iter().filter(|g| matches(g, only)) {
            if let Some(appid) = if g.kind == Kind::Steam { Some(g.appid) } else { ids.get(&g.id).copied() } {
                let _ = crate::artwork::apply_to_steam(cfg, g, appid);
            }
        }
        for id in &report.added {
            db.log(id, "added to Steam");
        }
        for id in &report.removed {
            db.log(id, "removed from Steam (no longer on the shelf)");
        }
    }
    Ok(SyncOutcome { refresh, steam: report, not_playable })
}

/// Copy a game from the shelf to this PC and put its saves back.
pub fn install(cfg: &Config, db: &Db, g: &Game, cancel: &Cancel, cb: &mut dyn FnMut(&Progress)) -> Result<()> {
    if g.kind == Kind::Steam {
        return crate::archive::restore(cfg, g.appid, None, cancel, cb);
    }
    if !cfg.shelf_online() {
        bail!("the shelf isn't reachable");
    }
    let src = g.shelf_path(cfg);
    if !src.exists() {
        bail!("{} is no longer on the shelf", g.name);
    }
    let dst = g.local_path(cfg);
    let need = transfer::dir_size(&src);
    if let Some(free) = transfer::free_space(&dst) {
        if need > free {
            bail!("not enough space: {} needs {}, {} free", g.name, transfer::human(need), transfer::human(free));
        }
    }
    std::fs::create_dir_all(&dst)?;
    db.log(&g.id, "download started");
    transfer::copy_tree(&src, &dst, &|r: &Path| r.to_string_lossy().ends_with(".steam-fan-part"), cancel, cb)?;
    if matches!(g.kind, Kind::Fan | Kind::NonSteam) {
        let n = saves::restore_game_folder(cfg, g)?;
        if n > 0 {
            db.log(&g.id, &format!("{n} saved files restored"));
        }
    }
    std::fs::write(dst.join(saves::MARKER), crate::now_stamp())?;
    db.log(&g.id, "on this PC");
    Ok(())
}

/// Back up the saves, then delete the copy on this PC. The game stays on the shelf and in Steam.
pub fn remove(cfg: &Config, db: &Db, g: &Game) -> Result<()> {
    if g.kind == Kind::Steam {
        bail!("uninstall Steam games in Steam (the shelf copy stays)");
    }
    let local = g.local_path(cfg);
    if !local.exists() {
        return Ok(());
    }
    if crate::launch::game_running(&local) {
        bail!("{} seems to be running; quit it first", g.name);
    }
    saves::backup(cfg, g).with_context(|| format!("{} was NOT removed", g.name))?;
    std::fs::remove_dir_all(&local)?;
    db.log(&g.id, "removed from this PC (saves on the shelf)");
    Ok(())
}

/// Put a game folder from this PC onto the shelf (`fangames/<group>/<name>` and so on).
pub fn upload(cfg: &Config, db: &Db, folder: &Path, kind: Kind, group: &str, cancel: &Cancel, cb: &mut dyn FnMut(&Progress)) -> Result<PathBuf> {
    if !cfg.shelf_online() {
        bail!("the shelf isn't reachable");
    }
    let name = folder.file_name().context("pick a folder")?;
    let base = match kind {
        Kind::Fan | Kind::NonSteam => cfg.shelf().join(kind.folder()).join(group),
        Kind::Rom => shelf::rom_games_root(&cfg.shelf().join("roms").join(group), cfg.emulator(group)),
        Kind::Steam => bail!("Steam games are archived by the NAS"),
    };
    let dst = base.join(name);
    transfer::copy_tree(folder, &dst, &|_| false, cancel, cb)?;
    db.log(&Game::make_id(kind, group, &format!("{group}/{}", name.to_string_lossy())), "uploaded to the shelf");
    Ok(dst)
}

#[derive(Debug, Default, Clone)]
pub struct ArtReport {
    pub copied: usize,
    pub fetched: usize,
    pub not_found: usize,
    pub errors: Vec<String>,
}

/// Bring artwork up to date: copy pictures from the shelf to this PC, fetch
/// missing ones from SteamGridDB (each game is looked up once; `again` looks up
/// again), and copy them into Steam's artwork folder.
pub fn refresh_art(cfg: &Config, db: &Db, games: &[Game], fetch: bool, again: bool, mut status: impl FnMut(&str)) -> ArtReport {
    use crate::artwork::{self, Art};
    let mut rep = ArtReport::default();
    let online = cfg.shelf_online();
    let can_fetch = fetch && online && !artwork::sgdb_key().is_empty();
    let ids = steam::shortcut_ids(cfg);
    // copying from the shelf waits on the network, not the CPU: do 8 games at a time
    if online {
        status("copying artwork from the shelf");
        let copied = std::sync::atomic::AtomicUsize::new(0);
        let chunk = games.len().div_ceil(8).max(1);
        std::thread::scope(|sc| {
            for part in games.chunks(chunk) {
                let copied = &copied;
                sc.spawn(move || {
                    for g in part {
                        if artwork::mirror(cfg, g) {
                            copied.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                });
            }
        });
        rep.copied = copied.into_inner();
    }
    for g in games {
        if can_fetch && g.kind != Kind::Steam && artwork::local(g, Art::Cover).is_none() {
            let known = db.art_lookup(&g.id);
            if again || known.is_none() {
                status(&format!("looking up {} on SteamGridDB", g.name));
                match artwork::auto_fill(cfg, g, known.flatten()) {
                    Ok((id, added)) => {
                        let _ = db.set_art_lookup(&g.id, id);
                        if added.is_empty() {
                            rep.not_found += 1;
                        } else {
                            rep.fetched += 1;
                            db.log(&g.id, &format!("artwork from SteamGridDB: {}", added.iter().map(|a| a.name()).collect::<Vec<_>>().join(", ")));
                        }
                    }
                    Err(e) => {
                        let msg = format!("{e:#}");
                        // a bad key or no connection: stop asking for every game
                        let fatal = msg.contains("API key") || msg.contains("cannot reach");
                        rep.errors.push(format!("{}: {msg}", g.name));
                        if fatal {
                            break;
                        }
                    }
                }
            }
        }
        let appid = if g.kind == Kind::Steam { Some(g.appid) } else { ids.get(&g.id).copied() };
        if let Some(appid) = appid {
            let _ = artwork::apply_to_steam(cfg, g, appid);
        }
    }
    rep
}


/// Move or rename a game on the shelf: its folder, its saves, its artwork, the copy
/// on this PC, and its history all follow. Its Steam entry is kept (with its play
/// time and artwork) and updated the next time Steam's library is updated.
/// `group` may be "Group/Subgroup". Returns the game's new id.
pub fn relocate(cfg: &Config, db: &mut Db, g: &Game, kind: Kind, group: &str, name: &str) -> Result<String> {
    use crate::artwork;
    use std::fs;
    if g.kind == Kind::Steam || kind == Kind::Steam {
        bail!("Steam games can't be moved");
    }
    if (g.kind == Kind::Rom) != (kind == Kind::Rom) {
        bail!("an emulator game can only move to another emulator system, and other games can't become emulator games");
    }
    if !cfg.shelf_online() {
        bail!("the shelf isn't reachable");
    }
    let clean = |s: &str| s.split('/').map(|p| p.trim().replace(['\\', ':'], "-")).filter(|p| !p.is_empty() && p != "." && p != "..").collect::<Vec<_>>().join("/");
    let (group, mut name) = (clean(group), clean(name).replace('/', "-"));
    if group.is_empty() || name.is_empty() {
        bail!("give the game a name and a group");
    }
    let old_shelf = g.shelf_path(cfg);
    // a single program file keeps its file type
    if old_shelf.is_file() {
        if let Some(ext) = old_shelf.extension() {
            let ext = format!(".{}", ext.to_string_lossy());
            if !name.to_lowercase().ends_with(&ext.to_lowercase()) {
                name.push_str(&ext);
            }
        }
    }
    let (rel, system) = match kind {
        Kind::Rom => (name.clone(), group.clone()),
        _ => (format!("{group}/{name}"), String::new()),
    };
    let mut ng = g.clone();
    ng.kind = kind;
    ng.rel = rel.clone();
    ng.system = system.clone();
    ng.id = Game::make_id(kind, &system, &rel);
    if ng.id == g.id {
        return Ok(ng.id);
    }
    let new_shelf = ng.shelf_path(cfg);
    if new_shelf.exists() {
        bail!("{} already exists on the shelf", new_shelf.display());
    }
    let local_old = g.local_path(cfg);
    if local_old.exists() && crate::launch::game_running(&local_old) {
        bail!("{} seems to be running; quit it first", g.name);
    }

    let mv = |a: &Path, b: &Path| -> Result<()> {
        if a.exists() {
            fs::create_dir_all(b.parent().unwrap())?;
            fs::rename(a, b).with_context(|| format!("cannot move {} to {}", a.display(), b.display()))?;
        }
        Ok(())
    };
    mv(&old_shelf, &new_shelf)?;
    // emulator saves belong to the whole system, so they stay where they are
    if kind != Kind::Rom {
        mv(&g.save_path(cfg), &ng.save_path(cfg))?;
    }
    mv(&artwork::shelf_dir(cfg, g), &artwork::shelf_dir(cfg, &ng))?;
    let _ = mv(&artwork::cache_dir(g), &artwork::cache_dir(&ng));
    if local_old.exists() {
        mv(&local_old, &ng.local_path(cfg))?;
    }
    // leave no empty group folders behind
    for p in [old_shelf.parent(), local_old.parent()].into_iter().flatten() {
        let _ = fs::remove_dir(p);
    }
    db.rename_game(&g.id, &ng.id)?;
    db.log(&ng.id, &format!("moved from {}", g.id));
    refresh(cfg, db, false, |_| {})?;
    Ok(ng.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn move_keeps_saves_art_and_steam_entry() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        std::env::set_var("XDG_DATA_HOME", r.join("data"));
        let s = |p: &str| r.join(p).to_string_lossy().into_owned();
        fs::create_dir_all(r.join("shelf/fangames/Mario/SM63")).unwrap();
        fs::write(r.join("shelf/fangames/Mario/SM63/sm63.exe"), b"MZ").unwrap();
        fs::create_dir_all(r.join("shelf/saves/fangames/Mario/SM63/game")).unwrap();
        fs::write(r.join("shelf/saves/fangames/Mario/SM63/game/save.dat"), b"1").unwrap();
        fs::create_dir_all(r.join("steam/userdata/7/config")).unwrap();
        let cfg = Config {
            shelf: s("shelf"),
            fan_local: s("pc/NonSteam"),
            steam_root: s("steam"),
            ..Default::default()
        };
        let mut db = Db::in_memory().unwrap();
        let out = sync_to_steam(&cfg, &mut db, &[], false, false, |_| {}).unwrap();
        assert_eq!(out.steam.added, ["fan:Mario/SM63"]);
        let before = steam::shortcut_ids(&cfg)["fan:Mario/SM63"];
        let g = db.games().unwrap().into_iter().find(|g| g.id == "fan:Mario/SM63").unwrap();
        crate::artwork::store(&cfg, &g, crate::artwork::Art::Cover, &{
            let mut b = std::io::Cursor::new(Vec::new());
            image::RgbImage::new(2, 3).write_to(&mut b, image::ImageFormat::Png).unwrap();
            b.into_inner()
        })
        .unwrap();

        let new = relocate(&cfg, &mut db, &g, Kind::Fan, "Super Mario Bros", "Super Mario 63").unwrap();
        assert_eq!(new, "fan:Super Mario Bros/Super Mario 63");
        assert!(r.join("shelf/fangames/Super Mario Bros/Super Mario 63/sm63.exe").exists());
        assert!(!r.join("shelf/fangames/Mario").exists(), "empty old group removed");
        assert!(r.join("shelf/saves/fangames/Super Mario Bros/Super Mario 63/game/save.dat").exists());
        assert!(r.join("shelf/artwork/fangames/Super Mario Bros/Super Mario 63/cover.png").exists());
        assert_eq!(db.programs().unwrap()[&new].0, "sm63.exe");

        // Steam keeps the same entry (same id, so play time and artwork stay)
        let out = sync_to_steam(&cfg, &mut db, &[], false, false, |_| {}).unwrap();
        assert!(out.steam.added.is_empty() && out.steam.removed.is_empty(), "{out:?}");
        assert_eq!(steam::shortcut_ids(&cfg)[&new], before);
        assert!(db.renames().unwrap().is_empty());
    }
}
