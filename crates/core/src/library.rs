//! The whole library: reading the shelf, installing on the box, keeping Steam in step.

use crate::config::Config;
use crate::artwork;
use crate::db::Db;
use crate::saves;
use crate::shelf::{self, Game, GroupScan, Kind, Platform};
use crate::steam::{self, Wanted};
use crate::transfer::{self, Cancel, Progress};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::fs;
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
    if cfg.server_mode() {
        on_read("Steam-Fan-Server");
        return crate::remote::refresh(cfg, db);
    }
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

/// The shelf group a game is read with (`fangames/Sonic`, `roms/gba`, `steam`).
fn group_key(g: &Game) -> String {
    match g.kind {
        Kind::Rom => format!("roms/{}", g.system),
        Kind::Steam => "steam".into(),
        _ => format!("{}/{}", g.kind.folder(), g.rel.split('/').next().unwrap_or_default()),
    }
}

/// Read the shelf again after changing it: the groups of `changed` are always read, since a
/// folder's time can look unchanged when the change happened in the same instant.
fn reread(cfg: &Config, db: &mut Db, changed: &[&Game]) -> Result<RefreshReport> {
    let mut cached = db.groups()?;
    let before = cached.len();
    for g in changed {
        cached.remove(&group_key(g));
    }
    if cached.len() != before {
        db.save_groups(&cached)?;
    }
    refresh(cfg, db, false, |_| {})
}

/// Read the shelf again after something landed at `path` on it (its group is always read).
pub fn refresh_after_adding(cfg: &Config, db: &mut Db, path: &Path) -> Result<RefreshReport> {
    let shelf = cfg.shelf();
    let key = path.strip_prefix(&shelf).ok().map(|r| {
        let parts: Vec<String> = r.components().take(2).map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        if parts.first().map(String::as_str) == Some("steam") { "steam".to_string() } else { parts.join("/") }
    });
    let mut cached = db.groups()?;
    if let Some(k) = key {
        if cached.remove(&k).is_some() {
            db.save_groups(&cached)?;
        }
    }
    refresh(cfg, db, false, |_| {})
}

pub fn matches(g: &Game, only: &[String]) -> bool {
    only.is_empty() || only.iter().any(|o| g.id.to_lowercase().contains(&o.to_lowercase()) || g.name.to_lowercase().contains(&o.to_lowercase()))
}

/// Find the program (or ROM file) of games that don't have one yet. Deep walk, once per game.
pub fn find_programs(cfg: &Config, db: &Db, games: &mut [Game], only: &[String], again: bool, mut on_game: impl FnMut(&Game)) -> Result<()> {
    if cfg.server_mode() {
        // the server found the programs; they came with the library
        return Ok(());
    }
    let known = db.programs()?;
    for g in games.iter_mut() {
        if g.kind == Kind::Steam || !matches(g, only) {
            continue;
        }
        if !again {
            if let Some((p, plat)) = known.get(&g.id) {
                g.program = p.clone();
                g.platform = *plat;
                // versions need their program's date to tell which is newest (one look each, once)
                if !g.version.is_empty() && g.built == 0 && !p.is_empty() {
                    g.built = program_date(cfg, g);
                    db.set_built(&g.id, g.built)?;
                }
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
        if !g.program.is_empty() {
            g.built = program_date(cfg, g);
            db.set_built(&g.id, g.built)?;
        }
    }
    Ok(())
}

/// The program file's date (Unix seconds): when that version was built.
fn program_date(cfg: &Config, g: &Game) -> i64 {
    let p = g.shelf_path(cfg);
    let exe = if p.is_file() { p } else { p.join(&g.program) };
    shelf::mtime(&exe) / 1_000_000_000
}

// ---------------------------------------------------------------- titles: one entry per game

/// One entry in the library: a game, or one game kept in several versions
/// (`Sonic World/{Release 1 ... Release 10, DX 1.2.4}`), shown once with the version Play uses.
#[derive(Debug, Clone)]
pub struct Title {
    /// The version Play uses (or the game itself).
    pub game: Game,
    /// Every version, newest first. Empty: a game without versions.
    pub versions: Vec<Game>,
    /// The version was chosen by hand, not just the newest.
    pub pinned: bool,
    /// The newest version was marked by hand.
    pub newest_marked: bool,
}

impl Title {
    pub fn id(&self) -> String {
        self.game.title_id()
    }
}

/// Group games into library entries. Keeps the order of `games`.
pub fn titles(games: Vec<Game>) -> Vec<Title> {
    let mut out: Vec<Title> = vec![];
    let mut at: HashMap<String, usize> = HashMap::new();
    for g in games {
        if g.version.is_empty() {
            out.push(Title { game: g, versions: vec![], pinned: false, newest_marked: false });
            continue;
        }
        let key = g.title_id();
        match at.get(&key) {
            Some(&i) => out[i].versions.push(g),
            None => {
                at.insert(key, out.len());
                out.push(Title { game: g.clone(), versions: vec![g], pinned: false, newest_marked: false });
            }
        }
    }
    for t in out.iter_mut().filter(|t| !t.versions.is_empty()) {
        shelf::sort_versions(&mut t.versions);
        let info = crate::artwork::info(&t.versions[0]);
        // marked by hand as the newest: first, whatever the names and dates say
        if let Some(i) = t.versions.iter().position(|v| !info.newest_version.is_empty() && v.version == info.newest_version) {
            let v = t.versions.remove(i);
            t.versions.insert(0, v);
            t.newest_marked = true;
        }
        let pick = info.default_version;
        let chosen = t.versions.iter().find(|v| !pick.is_empty() && v.version == pick);
        t.pinned = chosen.is_some();
        t.game = chosen.unwrap_or(&t.versions[0]).clone();
    }
    out
}

/// A game by id: a version's own id gives that version, an entry's id gives the version Play uses.
pub fn resolve(games: &[Game], id: &str) -> Option<Game> {
    if let Some(g) = games.iter().find(|g| g.id == id) {
        return Some(g.clone());
    }
    let mine: Vec<Game> = games.iter().filter(|g| !g.version.is_empty() && g.title_id() == id).cloned().collect();
    titles(mine).into_iter().next().map(|t| t.game)
}

/// Mark a version as the newest (its folder name; empty: work it out again).
/// Play uses the newest unless another version was chosen for it.
pub fn set_newest(cfg: &Config, t: &Title, version: &str) -> Result<()> {
    if t.versions.is_empty() {
        bail!("{} isn't kept in versions", t.game.name);
    }
    if !version.is_empty() && !t.versions.iter().any(|v| v.version == version) {
        bail!("{} has no version {version}", t.game.name);
    }
    let mut info = crate::artwork::info(&t.game);
    info.newest_version = version.to_string();
    crate::artwork::save_info(cfg, &t.game, &info)
}

/// Choose the version Play uses (its folder name; empty: always the newest).
/// It's kept with the game's info on the shelf, so every PC and the server agree.
pub fn set_version(cfg: &Config, t: &Title, version: &str) -> Result<()> {
    if t.versions.is_empty() {
        bail!("{} isn't kept in versions", t.game.name);
    }
    if !version.is_empty() && !t.versions.iter().any(|v| v.version == version) {
        bail!("{} has no version {version}", t.game.name);
    }
    let mut info = crate::artwork::info(&t.game);
    info.default_version = version.to_string();
    crate::artwork::save_info(cfg, &t.game, &info)
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
    if crate::platform::LINUX && installed.exists() {
        return installed;
    }
    // the window program (steam-fan-app) sits next to steam-fan, which is what Steam must run
    let me = std::env::current_exe().unwrap_or_else(|_| installed.clone());
    let beside = me.with_file_name(crate::platform::exe_name("steam-fan"));
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
            id: g.title_id(),
            name: g.name.clone(),
            exe: bin.to_string_lossy().into_owned(),
            start_dir: local.to_string_lossy().into_owned(),
            launch_options: steam::launch_options(&bin, &g.title_id(), false),
            tag: g.group.clone(),
            icon,
            proton: false,
        }),
        // Windows and macOS: Steam runs Steam-Fan, which starts the program (on a Mac through Wine)
        _ if !crate::platform::LINUX => Some(Wanted {
            id: g.title_id(),
            name: g.name.clone(),
            exe: bin.to_string_lossy().into_owned(),
            start_dir: local.to_string_lossy().into_owned(),
            launch_options: steam::launch_options(&bin, &g.title_id(), false),
            tag: g.group.clone(),
            icon,
            proton: false,
        }),
        _ => {
            let exe = local.join(&g.program);
            // one Steam entry per game: switching versions keeps its playtime and artwork
            Some(Wanted {
                id: g.title_id(),
                name: g.name.clone(),
                exe: exe.to_string_lossy().into_owned(),
                start_dir: exe.parent().unwrap_or(&local).to_string_lossy().into_owned(),
                launch_options: steam::launch_options(&bin, &g.title_id(), true),
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
    // a version's old Steam entry becomes the whole game's (keeps playtime and pictures)
    let mut renames = db.renames().unwrap_or_default();
    let all = titles(games.clone());
    for t in &all {
        for v in &t.versions {
            renames.entry(v.id.clone()).or_insert_with(|| t.id());
        }
    }
    let games: Vec<Game> = all.into_iter().map(|t| t.game).collect();
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
    let report = steam::sync_shortcuts_renamed(cfg, &list, &renames, prune, dry_run)?;
    if !dry_run && !renames.is_empty() {
        let _ = db.clear_renames();
    }
    if !dry_run {
        // the pictures go into Steam's artwork folder too (Steam reads them when it starts)
        let ids = steam::shortcut_ids(cfg);
        for g in games.iter().filter(|g| matches(g, only)) {
            if let Some(appid) = if g.kind == Kind::Steam { Some(g.appid) } else { ids.get(&g.title_id()).copied() } {
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

/// A folder holding several games: show them as separate games (`separate`), or as one game
/// in versions. `g` is one of them. A marker file in the folder remembers the choice.
pub fn set_separate(cfg: &Config, db: &mut Db, g: &Game, separate: bool) -> Result<()> {
    if cfg.server_mode() {
        return crate::remote::set_separate(cfg, db, g, separate);
    }
    if !matches!(g.kind, Kind::Fan | Kind::NonSteam) {
        bail!("only fan and non-Steam games can be kept in versions");
    }
    if !cfg.shelf_online() {
        bail!("the shelf isn't reachable");
    }
    if g.rel.split('/').count() < 3 {
        bail!("{} isn't inside another game's folder", g.name);
    }
    let folder = g.shelf_path(cfg).parent().context("no folder")?.to_path_buf();
    let (put, drop) = if separate { (shelf::COLLECTION_MARKER, shelf::VERSIONS_MARKER) } else { (shelf::VERSIONS_MARKER, shelf::COLLECTION_MARKER) };
    let _ = fs::remove_file(folder.join(drop));
    fs::write(folder.join(put), "")?;
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    db.log(&g.id, &if separate { format!("{name}: its folders are separate games") } else { format!("{name}: its folders are versions of one game") });
    reread(cfg, db, &[g])?;
    Ok(())
}

/// Move a file or folder (nothing to move: fine). Into a folder inside itself works too.
fn move_path(a: &Path, b: &Path) -> Result<()> {
    if !a.exists() {
        return Ok(());
    }
    let err = || format!("cannot move {} to {}", a.display(), b.display());
    if b.starts_with(a) {
        // into a folder inside itself (a game becoming one of its own versions): step aside first
        let tmp = a.with_file_name(format!("{}.steam-fan-moving", a.file_name().unwrap().to_string_lossy()));
        fs::rename(a, &tmp).with_context(err)?;
        fs::create_dir_all(b.parent().unwrap())?;
        fs::rename(&tmp, b).with_context(err)?;
    } else {
        fs::create_dir_all(b.parent().unwrap())?;
        fs::rename(a, b).with_context(err)?;
    }
    Ok(())
}

/// Move or rename a whole game with all its versions (its folder of versions).
/// Returns the game's new id.
pub fn relocate_title(cfg: &Config, db: &mut Db, t: &Title, kind: Kind, group: &str, name: &str) -> Result<String> {
    if cfg.server_mode() {
        return crate::remote::relocate(cfg, db, &t.game, kind, group, name, "", !t.versions.is_empty());
    }
    if t.versions.is_empty() {
        return relocate(cfg, db, &t.game, kind, group, name, "");
    }
    if kind == Kind::Rom || t.game.kind == Kind::Rom {
        bail!("emulator games aren't kept in versions");
    }
    if !cfg.shelf_online() {
        bail!("the shelf isn't reachable");
    }
    let clean = |s: &str| s.split('/').map(shelf::safe_name).filter(|p| !p.is_empty() && p != "." && p != "..").collect::<Vec<_>>().join("/");
    let (group, name) = (clean(group), clean(name).replace('/', "-"));
    if group.is_empty() || name.is_empty() {
        bail!("give the game a name and a group");
    }
    // the folder of versions, as if it were one game
    let mut old = t.game.clone();
    old.rel = t.game.title_rel();
    old.version.clear();
    old.id = Game::make_id(old.kind, "", &old.rel);
    let mut new = old.clone();
    new.kind = kind;
    new.rel = format!("{group}/{name}");
    new.name = name.clone();
    new.id = Game::make_id(kind, "", &new.rel);
    if new.id == old.id {
        return Ok(new.id);
    }
    if new.shelf_path(cfg).exists() {
        bail!("{} already exists on the shelf", new.shelf_path(cfg).display());
    }
    for v in &t.versions {
        let l = v.local_path(cfg);
        if l.exists() && crate::launch::game_running(&l) {
            bail!("{} seems to be running; close it first", v.full_name());
        }
    }
    move_path(&old.shelf_path(cfg), &new.shelf_path(cfg))?;
    move_path(&old.save_path(cfg), &new.save_path(cfg))?;
    move_path(&artwork::shelf_dir(cfg, &old), &artwork::shelf_dir(cfg, &new))?;
    let _ = move_path(&artwork::cache_dir(&old), &artwork::cache_dir(&new));
    let local_old = old.local_path(cfg);
    move_path(&local_old, &new.local_path(cfg))?;
    for p in [old.shelf_path(cfg).parent().map(Path::to_path_buf), local_old.parent().map(Path::to_path_buf)].into_iter().flatten() {
        let _ = fs::remove_dir(p);
    }
    for v in &t.versions {
        db.rename_game(&v.id, &Game::make_id(kind, "", &format!("{}/{}", new.rel, v.version)))?;
    }
    // Steam knows the game by this one
    db.rename_game(&old.id, &new.id)?;
    db.log(&new.id, &format!("moved from {} with its {} versions", old.id, t.versions.len()));
    reread(cfg, db, &[&old, &new])?;
    Ok(new.id)
}

/// Copy a game from the shelf to this PC and put its saves back.
pub fn install(cfg: &Config, db: &Db, g: &Game, cancel: &Cancel, cb: &mut dyn FnMut(&Progress)) -> Result<()> {
    if cfg.server_mode() && g.kind != Kind::Steam {
        return crate::remote::install(cfg, db, g, cancel, cb);
    }
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
    // check what arrived against the checksums, when the game has them; copy bad files once more
    if let Some(m) = crate::integrity::read(&crate::integrity::manifest_path(cfg, g)) {
        let c = crate::integrity::check(&m, &dst, false, cancel, cb)?;
        if !c.ok() {
            let again: Vec<std::path::PathBuf> = c.missing.iter().chain(&c.damaged).map(Into::into).collect();
            let base = if src.is_file() { src.parent().unwrap().to_path_buf() } else { src.clone() };
            transfer::copy_listed(&base, &dst, &again)?;
            let c2 = crate::integrity::check(&m, &dst, false, cancel, cb)?;
            if !c2.ok() {
                db.log(&g.id, "copy doesn't match its checksums: the shelf copy may be damaged");
                bail!(
                    "{} files of {} don't match their checksums, also after copying them again. The shelf copy may be damaged: use Verify on the game's page.",
                    c2.missing.len() + c2.damaged.len(),
                    g.full_name()
                );
            }
        }
        db.log(&g.id, "copy checked against its checksums");
    }
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
        bail!("{} seems to be running (or a program is open in its folder); close it first", g.name);
    }
    saves::backup(cfg, g).with_context(|| format!("{} was NOT removed", g.name))?;
    std::fs::remove_dir_all(&local)?;
    db.log(&g.id, "removed from this PC (saves on the shelf)");
    Ok(())
}

/// Put a game folder from this PC onto the shelf (`fangames/<group>/<name>` and so on).
pub fn upload(cfg: &Config, db: &Db, folder: &Path, kind: Kind, group: &str, cancel: &Cancel, cb: &mut dyn FnMut(&Progress)) -> Result<PathBuf> {
    if cfg.server_mode() {
        bail!("with Steam-Fan-Server, add games with Add game (the server stores them)");
    }
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
    if cfg.server_mode() {
        let mut rep = ArtReport::default();
        for t in titles(games.to_vec()) {
            match crate::remote::fetch_art(cfg, &t.game) {
                Ok(true) => rep.copied += 1,
                Ok(false) => {}
                Err(e) => rep.errors.push(format!("{}: {e:#}", t.game.name)),
            }
        }
        status("artwork from the server");
        return rep;
    }
    let mut rep = ArtReport::default();
    let online = cfg.shelf_online();
    // pictures belong to the game, not to each of its versions
    let ts = titles(games.to_vec());
    if online {
        for t in ts.iter().filter(|t| !t.versions.is_empty()) {
            crate::artwork::adopt_version_art(cfg, &t.game, &t.versions);
            // each version's own pictures and info
            for v in &t.versions {
                if crate::own::mirror(cfg, v) {
                    rep.copied += 1;
                }
            }
        }
    }
    let games: Vec<Game> = ts.into_iter().map(|t| t.game).collect();
    let games = &games[..];
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
            let known = db.art_lookup(&g.title_id());
            if again || known.is_none() {
                status(&format!("looking up {} on SteamGridDB", g.name));
                match artwork::auto_fill(cfg, g, known.flatten()) {
                    Ok((id, added)) => {
                        let _ = db.set_art_lookup(&g.title_id(), id);
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
/// `group` may be "Group/Subgroup". A `version` keeps the game as one of several
/// versions side by side (`<group>/<name>/<version>`). Returns the game's new id.
pub fn relocate(cfg: &Config, db: &mut Db, g: &Game, kind: Kind, group: &str, name: &str, version: &str) -> Result<String> {
    if cfg.server_mode() {
        return crate::remote::relocate(cfg, db, g, kind, group, name, version, false);
    }
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
    let clean = |s: &str| s.split('/').map(shelf::safe_name).filter(|p| !p.is_empty() && p != "." && p != "..").collect::<Vec<_>>().join("/");
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
    let version = clean(version).replace('/', "-");
    let (rel, system) = match kind {
        Kind::Rom => (name.clone(), group.clone()),
        _ if !version.is_empty() => (format!("{group}/{name}/{version}"), String::new()),
        _ => (format!("{group}/{name}"), String::new()),
    };
    let mut ng = g.clone();
    ng.kind = kind;
    ng.rel = rel.clone();
    ng.system = system.clone();
    ng.version = if kind == Kind::Rom { String::new() } else { version.clone() };
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
        bail!("{} seems to be running (or a program is open in its folder); close it first", g.name);
    }

    let mv = move_path;
    if !ng.version.is_empty() {
        crate::import::check_version_parent(&new_shelf.parent().unwrap(), Some(&old_shelf))?;
    }
    mv(&old_shelf, &new_shelf)?;
    if !ng.version.is_empty() {
        let _ = fs::write(new_shelf.parent().unwrap().join(crate::shelf::VERSIONS_MARKER), "");
    }
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
        // a versions folder whose last version moved out: only the marker is left
        if fs::read_dir(p).map(|d| d.flatten().all(|e| e.file_name() == crate::shelf::VERSIONS_MARKER)).unwrap_or(false) {
            let _ = fs::remove_file(p.join(crate::shelf::VERSIONS_MARKER));
        }
        let _ = fs::remove_dir(p);
    }
    db.rename_game(&g.id, &ng.id)?;
    db.log(&ng.id, &format!("moved from {}", g.id));
    reread(cfg, db, &[g, &ng])?;
    Ok(ng.id)
}

/// Download a game again from its link and put it on the shelf.
/// * `as_version` given: the download is added beside it as another version.
/// * otherwise it replaces the shelf copy (the old one is kept until the new one is in place);
///   the copy on this PC is removed after its saves are backed up, so the next Play gets the new files.
/// Returns where it went.
pub fn redownload(
    cfg: &Config,
    db: &mut Db,
    g: &Game,
    as_version: Option<&str>,
    // a file you downloaded yourself (when the link needs a browser): used instead of the link
    file: Option<&Path>,
    cancel: &Cancel,
    status: &mut dyn FnMut(&str),
    cb: &mut dyn FnMut(&Progress),
) -> Result<PathBuf> {
    if cfg.server_mode() {
        return crate::remote::redownload(cfg, db, g, as_version, file, cancel, status, cb);
    }
    use crate::{artwork, import, integrity, source};
    use std::fs;
    if g.kind == Kind::Steam {
        bail!("Steam games are updated by Steam and the NAS");
    }
    if !cfg.shelf_online() {
        bail!("the shelf isn't reachable");
    }
    let mut info = artwork::info(g);
    let url = info.download.trim().to_string();
    let from_link = file.is_none();
    let file = match file {
        Some(f) => {
            if !f.exists() {
                bail!("{} doesn't exist", f.display());
            }
            f.to_path_buf()
        }
        None => {
            if url.is_empty() {
                bail!("no download link: add it under About > Edit, or use a file you downloaded");
            }
            status("downloading");
            source::fetch(&url, cancel, cb)?
        }
    };
    let result = (|| -> Result<PathBuf> {
        if let Some(v) = as_version.filter(|v| !v.trim().is_empty()) {
            let group = match g.kind {
                Kind::Rom => g.system.clone(),
                _ => {
                    // the group is the path without the name (and version)
                    let parts: Vec<&str> = g.rel.split('/').collect();
                    let keep = parts.len().saturating_sub(if g.version.is_empty() { 1 } else { 2 });
                    parts[..keep].join("/")
                }
            };
            let (dst, m) = import::place(cfg, &file, g.kind, &group, &g.name, v, cancel, status, cb)?;
            reread(cfg, db, &[g])?;
            if let Some(ng) = db.games()?.into_iter().find(|x| x.shelf_path(cfg) == dst) {
                let _ = integrity::write(&integrity::manifest_path(cfg, &ng), &m);
                let mut ni = info.clone();
                if from_link {
                    source::save_stamp(cfg, &ng, &mut ni);
                }
                db.log(&ng.id, if from_link { "downloaded from its link" } else { "added from a downloaded file" });
            }
            return Ok(dst);
        }
        let shelf = g.shelf_path(cfg);
        if shelf.is_file() {
            bail!("this game is a single program file; download it by hand and replace it");
        }
        if is_installed(cfg, g) {
            status("backing up saves");
            remove(cfg, db, g)?;
        }
        let aside = shelf.with_file_name(format!("{}.steam-fan-old", shelf.file_name().unwrap().to_string_lossy()));
        let _ = fs::remove_dir_all(&aside);
        fs::rename(&shelf, &aside).context("cannot move the old copy aside")?;
        match import::place_at(&file, &shelf, cancel, status, cb) {
            Ok(m) => {
                let _ = fs::remove_dir_all(&aside);
                integrity::write(&integrity::manifest_path(cfg, g), &m)?;
                if from_link {
                    source::save_stamp(cfg, g, &mut info);
                }
                db.log(&g.id, if from_link { "downloaded again from its link; the shelf copy was replaced" } else { "the shelf copy was replaced from a downloaded file" });
                // the program may have a new name
                let _ = db.set_program(&g.id, "", g.platform);
                let mut v = vec![g.clone()];
                find_programs(cfg, db, &mut v, &[], true, |_| {})?;
                reread(cfg, db, &[g])?;
                Ok(shelf)
            }
            Err(e) => {
                let _ = fs::remove_dir_all(&shelf);
                let _ = fs::rename(&aside, &shelf);
                Err(e.context("the old copy was put back"))
            }
        }
    })();
    if from_link {
        source::downloads_dir_clean();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn move_keeps_saves_art_and_steam_entry() {
        let _env = crate::env_lock();
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

        let new = relocate(&cfg, &mut db, &g, Kind::Fan, "Super Mario Bros", "Super Mario 63", "").unwrap();
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

    #[test]
    fn game_becomes_one_of_its_versions() {
        let _env = crate::env_lock();
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        std::env::set_var("XDG_DATA_HOME", r.join("data"));
        fs::create_dir_all(r.join("shelf/fangames/Sonic/Sonic Network Land")).unwrap();
        fs::write(r.join("shelf/fangames/Sonic/Sonic Network Land/snl.exe"), b"MZ").unwrap();
        let cfg = Config { shelf: r.join("shelf").to_string_lossy().into(), fan_local: r.join("pc").to_string_lossy().into(), ..Default::default() };
        let mut db = Db::in_memory().unwrap();
        refresh(&cfg, &mut db, false, |_| {}).unwrap();
        let g = db.games().unwrap().pop().unwrap();
        let id = relocate(&cfg, &mut db, &g, Kind::Fan, "Sonic", "Sonic Network Land", "v96").unwrap();
        assert_eq!(id, "fan:Sonic/Sonic Network Land/v96");
        assert!(r.join("shelf/fangames/Sonic/Sonic Network Land/v96/snl.exe").exists());
        let g = db.games().unwrap().pop().unwrap();
        assert_eq!((g.name.as_str(), g.version.as_str()), ("Sonic Network Land", "v96"));
    }

    #[test]
    fn versions_are_one_entry_and_play_the_chosen_one() {
        let _env = crate::env_lock();
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        std::env::set_var("XDG_DATA_HOME", r.join("data"));
        let s = |p: &str| r.join(p).to_string_lossy().into_owned();
        let cfg = Config { shelf: s("shelf"), fan_local: s("pc"), ..Default::default() };
        for (v, exe) in [("Release 9", "sw9.exe"), ("Release 10", "sw10.exe"), ("Old", "old.exe")] {
            let p = r.join(format!("shelf/fangames/Sonic/Sonic World/{v}"));
            fs::create_dir_all(&p).unwrap();
            fs::write(p.join(exe), b"MZ").unwrap();
        }
        fs::create_dir_all(r.join("shelf/fangames/Sonic/Solo")).unwrap();
        fs::write(r.join("shelf/fangames/Sonic/Solo/solo.exe"), b"MZ").unwrap();
        let mut db = Db::in_memory().unwrap();
        refresh(&cfg, &mut db, true, |_| {}).unwrap();
        let mut games = db.games().unwrap();
        find_programs(&cfg, &db, &mut games, &[], false, |_| {}).unwrap();
        // Release 10 is the newest build, Old is never the default
        let set = |name: &str, secs: i64| {
            let f = fs::File::options().write(true).open(r.join("shelf/fangames/Sonic/Sonic World").join(name)).unwrap();
            f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs as u64)).unwrap();
        };
        set("Release 9/sw9.exe", 1_500_000_000);
        set("Release 10/sw10.exe", 1_600_000_000);
        set("Old/old.exe", 1_700_000_000);
        db.forget_programs().unwrap();
        let mut games = db.games().unwrap();
        find_programs(&cfg, &db, &mut games, &[], false, |_| {}).unwrap();
        let games = db.games().unwrap();
        let ts = titles(games.clone());
        assert_eq!(ts.len(), 2, "one entry for Sonic World, one for Solo");
        let sw = ts.iter().find(|t| t.game.name == "Sonic World").unwrap();
        assert_eq!(sw.id(), "fan:Sonic/Sonic World");
        assert_eq!(sw.game.version, "Release 10");
        assert_eq!(sw.versions.iter().map(|v| v.version.as_str()).collect::<Vec<_>>(), ["Release 10", "Release 9", "Old"]);
        // Steam gets one entry, named after the game, launched by the game's id
        let w = wanted(&cfg, &sw.game, None).unwrap();
        assert_eq!((w.id.as_str(), w.name.as_str()), ("fan:Sonic/Sonic World", "Sonic World"));
        assert!(w.launch_options.contains("\"fan:Sonic/Sonic World\""), "{}", w.launch_options);
        // the entry's id plays the default; a version's own id plays that version
        assert_eq!(resolve(&games, "fan:Sonic/Sonic World").unwrap().version, "Release 10");
        assert_eq!(resolve(&games, "fan:Sonic/Sonic World/Release 9").unwrap().version, "Release 9");
        // choose another version: saved with the game's info, Play follows
        fs::create_dir_all(r.join("shelf/steam")).unwrap();
        set_version(&cfg, sw, "Release 9").unwrap();
        artwork::mirror(&cfg, &sw.game);
        assert_eq!(resolve(&games, "fan:Sonic/Sonic World").unwrap().version, "Release 9");
        assert!(titles(games.clone()).iter().find(|t| t.game.name == "Sonic World").unwrap().pinned);
        assert!(set_version(&cfg, sw, "Release 99").is_err());
        // mark Release 9 the newest by hand: first in the list
        set_newest(&cfg, sw, "Release 9").unwrap();
        artwork::mirror(&cfg, &sw.game);
        let marked = titles(games.clone()).into_iter().find(|t| t.game.name == "Sonic World").unwrap();
        assert!(marked.newest_marked);
        assert_eq!(marked.versions[0].version, "Release 9");
        set_newest(&cfg, sw, "").unwrap();
        artwork::mirror(&cfg, &sw.game);
        assert_eq!(titles(games.clone()).into_iter().find(|t| t.game.name == "Sonic World").unwrap().versions[0].version, "Release 10");
        // Steam still runs Release 10's program: Play starts Release 9's
        let r9 = resolve(&games, "fan:Sonic/Sonic World").unwrap();
        let mut r9 = r9;
        r9.program = "sw9.exe".into();
        let old_exe = r.join("pc/Sonic/Sonic World/Release 10/sw10.exe").to_string_lossy().into_owned();
        let cmd = crate::launch::retarget(&cfg, &r9, &["/proton".into(), "waitforexitandrun".into(), old_exe]);
        assert_eq!(cmd[2], r.join("pc/Sonic/Sonic World/Release 9/sw9.exe").to_string_lossy());
        // rename the whole game: every version moves, ids follow
        let sw = titles(db.games().unwrap()).into_iter().find(|t| t.game.name == "Sonic World").unwrap();
        let new = relocate_title(&cfg, &mut db, &sw, Kind::Fan, "Sonic", "Sonic World Classic").unwrap();
        assert_eq!(new, "fan:Sonic/Sonic World Classic");
        assert!(r.join("shelf/fangames/Sonic/Sonic World Classic/Release 9/sw9.exe").exists());
        let renames = db.renames().unwrap();
        assert_eq!(renames.get("fan:Sonic/Sonic World").map(String::as_str), Some("fan:Sonic/Sonic World Classic"));
        assert_eq!(renames.get("fan:Sonic/Sonic World/Release 9").map(String::as_str), Some("fan:Sonic/Sonic World Classic/Release 9"));
        let after = titles(db.games().unwrap());
        let swc = after.iter().find(|t| t.game.name == "Sonic World Classic").unwrap();
        assert_eq!(swc.versions.len(), 3);
        // they're different games after all: each its own entry, the folder their group; and back
        set_separate(&cfg, &mut db, &swc.versions[0], true).unwrap();
        let sep = titles(db.games().unwrap());
        assert_eq!(sep.len(), 4);
        assert!(sep.iter().any(|t| t.id() == "fan:Sonic/Sonic World Classic/Release 9" && t.game.group == "Sonic World Classic" && t.versions.is_empty()));
        let one = sep.iter().find(|t| t.game.group == "Sonic World Classic").unwrap().game.clone();
        set_separate(&cfg, &mut db, &one, false).unwrap();
        assert_eq!(titles(db.games().unwrap()).len(), 2);
    }
}
