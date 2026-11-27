//! The Steam-Fan library window (Tauri). The interface is the web app in `ui/`;
//! everything it does goes through the commands below into `steam-fan-core`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod jobs;

use jobs::{Jobs, Ctx};
use serde::Serialize;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use steam_fan_core::archive;
use steam_fan_core::artwork::{self, Art};
use steam_fan_core::config::secrets;
use steam_fan_core::steam::{self, Manifest};
use steam_fan_core::{import, library, transfer, Config, Db, Game, Kind, Platform};
use tauri::{Manager, State};

pub struct App {
    cfg: Mutex<Config>,
    jobs: Jobs,
}

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

fn cfg(app: &State<App>) -> Config {
    app.cfg.lock().unwrap().clone()
}

fn db() -> Res<Db> {
    Db::open_default().map_err(err)
}

/// A game by id. A game kept in versions: the version Play uses (a version's own id: that version).
fn find(id: &str) -> Res<Game> {
    library::resolve(&db()?.games().map_err(err)?, id).ok_or_else(|| format!("{id} is not in the library"))
}

/// The library entry with this id (all versions of a game).
fn find_title(id: &str) -> Res<library::Title> {
    let games = db()?.games().map_err(err)?;
    let g = library::resolve(&games, id).ok_or_else(|| format!("{id} is not in the library"))?;
    let tid = g.title_id();
    library::titles(games.into_iter().filter(|x| x.title_id() == tid).collect()).into_iter().next().ok_or_else(|| format!("{id} is not in the library"))
}

/// Run blocking work off the window's thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(err)?
}

// ---------------------------------------------------------------- library

#[derive(Serialize)]
struct ArtView {
    /// `art://` address with a version, so a changed picture is loaded again.
    cover: Option<String>,
    /// Small cover for the grid.
    thumb: Option<String>,
    hero: Option<String>,
    logo: Option<String>,
    icon: Option<String>,
}

/// One version of a game kept in several.
#[derive(Serialize)]
struct VersionView {
    /// Its own id (to act on this version only).
    id: String,
    /// Its folder name.
    folder: String,
    /// Shown: the folder name without the game's name in front.
    label: String,
    on_pc: bool,
    playable: bool,
    /// Program date (Unix seconds), 0: not known.
    built: i64,
    /// An "Old" folder: never the default.
    old: bool,
}

#[derive(Serialize)]
struct GameView {
    /// The library entry's id: the same for all versions of a game.
    id: String,
    name: String,
    /// The version Play uses (shown name; empty: the game has no versions).
    version: String,
    /// Its folder name and own id.
    version_folder: String,
    version_id: String,
    /// Every version, newest first (empty: no versions).
    versions: Vec<VersionView>,
    /// The version was chosen by hand (otherwise: the newest).
    pinned: bool,
    /// One of several different games in one folder (can be made versions of one game).
    in_collection: bool,
    /// The first version in `versions` was marked as the newest by hand.
    newest_marked: bool,
    kind: Kind,
    kind_label: &'static str,
    group: String,
    system: String,
    platform: Platform,
    program: String,
    playable: bool,
    on_pc: bool,
    /// Steam archive only: up to date / shelf is newer / ...
    fresh: Option<&'static str>,
    fresh_code: Option<String>,
    size: u64,
    appid: u32,
    buildid: String,
    /// The id Steam knows it by (shortcut or real app id), if it's in Steam's library.
    steam_id: Option<u32>,
    art: ArtView,
}

#[derive(Serialize)]
struct LibraryView {
    games: Vec<GameView>,
    shelf: String,
    unknown_systems: Vec<String>,
    has_sgdb_key: bool,
    configured: bool,
}

fn art_url(g: &Game, a: Art) -> Option<String> {
    let p = artwork::local(g, a)?;
    let v = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(format!("art://localhost/{}/{}?v={v}", encode(&g.title_id()), a.name()))
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Steam's ids for our entries, also for games moved since Steam's library was last updated.
fn steam_ids(cfg: &Config) -> HashMap<String, u32> {
    let mut ids = steam::shortcut_ids(cfg);
    if let Ok(r) = db().and_then(|d| d.renames().map_err(err)) {
        for (old, new) in r {
            if let Some(v) = ids.get(&old).copied() {
                ids.insert(new, v);
            }
        }
    }
    ids
}

fn local_manifests(cfg: &Config) -> HashMap<u32, Manifest> {
    let mut out = HashMap::new();
    for lib in steam::libraries(&cfg.steam_root()) {
        if let Ok(rd) = std::fs::read_dir(lib.join("steamapps")) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with("appmanifest_") && n.ends_with(".acf") {
                    if let Some(m) = steam::read_manifest(&e.path()) {
                        out.insert(m.appid, m);
                    }
                }
            }
        }
    }
    out
}

#[tauri::command]
async fn library(app: State<'_, App>) -> Res<LibraryView> {
    let cfg = cfg(&app);
    blocking(move || {
        let games = db()?.games().map_err(err)?;
        let local = local_manifests(&cfg);
        let mut ids = steam_ids(&cfg);
        let views = library::titles(games)
            .into_iter()
            .map(|t| {
                let pinned = t.pinned;
                let newest_marked = t.newest_marked;
                let versions: Vec<VersionView> = t
                    .versions
                    .iter()
                    .map(|v| VersionView {
                        id: v.id.clone(),
                        folder: v.version.clone(),
                        label: v.version_label(),
                        on_pc: library::is_installed(&cfg, v),
                        playable: !v.program.is_empty(),
                        built: v.built,
                        old: steam_fan_core::shelf::is_old_version(&v.version),
                    })
                    .collect();
                // before Steam's library is updated, a version's old Steam entry stands for the game
                if !ids.contains_key(&t.id()) {
                    if let Some(a) = t.versions.iter().find_map(|v| ids.get(&v.id).copied()) {
                        ids.insert(t.id(), a);
                    }
                }
                let g = t.game;
                let (on_pc, fresh) = if g.kind == Kind::Steam {
                    let pc = local.get(&g.appid);
                    let shelf = Manifest { buildid: g.buildid.clone(), ..Default::default() };
                    (pc.is_some(), Some(archive::freshness(Some(&shelf), pc)))
                } else {
                    (library::is_installed(&cfg, &g), None)
                };
                GameView {
                    art: ArtView {
                        cover: art_url(&g, Art::Cover),
                        thumb: art_url(&g, Art::Cover).map(|u| u.replacen("/cover?", "/cover-thumb?", 1)),
                        hero: art_url(&g, Art::Hero),
                        logo: art_url(&g, Art::Logo),
                        icon: art_url(&g, Art::Icon),
                    },
                    steam_id: if g.kind == Kind::Steam { Some(g.appid) } else { ids.get(&g.title_id()).copied() },
                    kind_label: g.kind.label(),
                    playable: g.kind == Kind::Steam || !g.program.is_empty(),
                    fresh: fresh.map(|f| f.label()),
                    fresh_code: fresh.map(|f| format!("{f:?}")),
                    on_pc,
                    version: if g.version.is_empty() { String::new() } else { g.version_label() },
                    version_id: g.id.clone(),
                    id: g.title_id(),
                    name: g.name,
                    in_collection: g.version.is_empty() && matches!(g.kind, Kind::Fan | Kind::NonSteam) && g.rel.split('/').count() >= 3,
                    version_folder: g.version,
                    versions,
                    pinned,
                    newest_marked,
                    kind: g.kind,
                    group: g.group,
                    system: g.system,
                    platform: g.platform,
                    program: g.program,
                    size: g.size,
                    appid: g.appid,
                    buildid: g.buildid,
                }
            })
            .collect();
        Ok(LibraryView {
            games: views,
            shelf: cfg.shelf().display().to_string(),
            unknown_systems: steam_fan_core::shelf::unknown_systems(&cfg),
            has_sgdb_key: !artwork::sgdb_key().is_empty(),
            configured: Config::path().exists(),
        })
    })
    .await
}

#[derive(Serialize)]
struct Status {
    shelf_online: bool,
    steam_running: bool,
}

#[tauri::command]
async fn status(app: State<'_, App>) -> Res<Status> {
    let c = cfg(&app);
    // a NAS that is off can make the mount hang: give up after 4 seconds
    // (with a server: whether it answers)
    let online = blocking(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(c.shelf_online());
        });
        Ok(rx.recv_timeout(std::time::Duration::from_secs(4)).unwrap_or(false))
    })
    .await?;
    Ok(Status { shelf_online: online, steam_running: steam::running() })
}

#[derive(Serialize)]
struct Details {
    shelf_path: String,
    local_path: String,
    saves_path: String,
    rom_command: Option<String>,
    history: Vec<(String, String)>,
    art_on_shelf: Vec<Art>,
    info: artwork::Info,
    /// (file name, art:// address)
    screenshots: Vec<(String, String)>,
}

#[tauri::command]
async fn details(app: State<'_, App>, id: String) -> Res<Details> {
    let cfg = cfg(&app);
    blocking(move || {
        let g = find(&id)?;
        let db = db()?;
        Ok(Details {
            shelf_path: g.shelf_path(&cfg).display().to_string(),
            local_path: if g.kind == Kind::Steam { String::new() } else { g.local_path(&cfg).display().to_string() },
            saves_path: g.save_path(&cfg).display().to_string(),
            rom_command: (g.kind == Kind::Rom).then(|| {
                steam_fan_core::launch::rom_command(&cfg, &g).map(|c| c.join(" ")).unwrap_or_else(|e| format!("{e:#}"))
            }),
            history: {
                // the game's own events and its version's, newest first
                let mut h = db.events(Some(&g.id), 12).map_err(err)?;
                if g.title_id() != g.id {
                    h.extend(db.events(Some(&g.title_id()), 12).map_err(err)?);
                    h.sort_by(|a, b| b.ts.cmp(&a.ts));
                    h.truncate(12);
                }
                h.into_iter().map(|e| (e.ts, e.msg)).collect()
            },
            art_on_shelf: if cfg.shelf_online() { artwork::shelf_pictures(&cfg, &g).into_iter().map(|(a, _)| a).collect() } else { vec![] },
            info: artwork::info(&g),
            screenshots: artwork::screenshots(&g)
                .into_iter()
                .map(|p| {
                    let n = p.file_name().unwrap().to_string_lossy().into_owned();
                    let url = format!("art://localhost/{}/shot:{}", encode(&g.title_id()), encode(&n));
                    (n, url)
                })
                .collect(),
        })
    })
    .await
}

// ---------------------------------------------------------------- jobs

#[tauri::command]
fn jobs(app: State<'_, App>) -> Vec<jobs::JobView> {
    app.jobs.list()
}

#[tauri::command]
fn job_cancel(app: State<'_, App>, id: u64) {
    app.jobs.cancel(id);
}

#[tauri::command]
fn refresh(handle: tauri::AppHandle, app: State<'_, App>, full: bool) -> u64 {
    let cfg = cfg(&app);
    app.jobs.spawn(&handle, "Reading the shelf", move |ctx: Ctx| {
        let mut db = Db::open_default()?;
        let rep = library::refresh(&cfg, &mut db, full, |k| ctx.status(&format!("reading {k}")))?;
        let mut games = db.games()?;
        library::find_programs(&cfg, &db, &mut games, &[], full, |g| ctx.status(&format!("looking for the program in {}", g.name)))?;
        ctx.reload();
        ctx.status("artwork");
        let art = library::refresh_art(&cfg, &db, &games, true, false, |s| ctx.status(s));
        let mut msg = format!("{} games on the shelf", rep.games);
        if art.fetched > 0 {
            msg.push_str(&format!(", artwork found for {}", art.fetched));
        }
        if !rep.errors.is_empty() {
            msg.push_str(&format!(" ({} folders could not be read)", rep.errors.len()));
        }
        if let Some(e) = art.errors.first() {
            msg.push_str(&format!(". Artwork: {e}"));
        }
        Ok(msg)
    })
}

#[tauri::command]
fn sync_steam(handle: tauri::AppHandle, app: State<'_, App>) -> u64 {
    let cfg = cfg(&app);
    app.jobs.spawn(&handle, "Updating Steam's library", move |ctx: Ctx| {
        let mut db = Db::open_default()?;
        let out = library::sync_to_steam(&cfg, &mut db, &[], false, false, |m| ctx.status(m))?;
        let mut msg = format!("Steam library: {}", out.steam.summary());
        if !out.not_playable.is_empty() {
            msg.push_str(&format!("; {} games have no program and were skipped", out.not_playable.len()));
        }
        Ok(msg)
    })
}

#[tauri::command]
fn install(handle: tauri::AppHandle, app: State<'_, App>, id: String) -> Res<u64> {
    let cfg = cfg(&app);
    let g = find(&id)?;
    let title = if g.kind == Kind::Steam { format!("Installing {} from the shelf", g.name) } else { format!("Copying {} to this PC", g.name) };
    Ok(app.jobs.spawn(&handle, &title, move |ctx: Ctx| {
        let db = Db::open_default()?;
        let mut v = vec![g];
        library::find_programs(&cfg, &db, &mut v, &[], false, |_| {})?;
        library::install(&cfg, &db, &v[0], &ctx.cancel, &mut |p| ctx.progress(p))?;
        Ok(format!("{} is on this PC", v[0].name))
    }))
}

#[tauri::command]
fn remove(handle: tauri::AppHandle, app: State<'_, App>, id: String) -> Res<u64> {
    let cfg = cfg(&app);
    let g = find(&id)?;
    Ok(app.jobs.spawn(&handle, &format!("Removing {}", g.name), move |_| {
        let db = Db::open_default()?;
        library::remove(&cfg, &db, &g)?;
        Ok(format!("Removed {} from this PC; saves are on the shelf", g.name))
    }))
}

#[tauri::command]
fn archive_upload(handle: tauri::AppHandle, app: State<'_, App>, id: String) -> Res<u64> {
    let cfg = cfg(&app);
    let g = find(&id)?;
    Ok(app.jobs.spawn(&handle, &format!("Copying {} to the shelf", g.name), move |ctx: Ctx| {
        archive::upload(&cfg, g.appid, &ctx.cancel, &mut |p| ctx.progress(p))?;
        Ok(format!("{} copied to the shelf", g.name))
    }))
}

#[tauri::command]
fn refresh_art(handle: tauri::AppHandle, app: State<'_, App>, again: bool) -> u64 {
    let cfg = cfg(&app);
    app.jobs.spawn(&handle, "Updating artwork", move |ctx: Ctx| {
        let db = Db::open_default()?;
        let games = db.games()?;
        let rep = library::refresh_art(&cfg, &db, &games, true, again, |s| ctx.status(s));
        if let Some(e) = rep.errors.first() {
            if rep.fetched == 0 {
                anyhow::bail!("{e}");
            }
        }
        Ok(format!("Artwork: {} found on SteamGridDB, {} with nothing there", rep.fetched, rep.not_found))
    })
}

// ---------------------------------------------------------------- artwork

#[tauri::command]
async fn art_search(id: String, term: Option<String>) -> Res<Vec<artwork::SgdbGame>> {
    blocking(move || {
        let g = find(&id)?;
        if g.kind == Kind::Steam && term.as_deref().map_or(true, str::is_empty) {
            return Ok(artwork::sgdb_for_steam(g.appid).map(|x| vec![x]).unwrap_or_default());
        }
        let t = term.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| artwork::search_name(&g));
        artwork::sgdb_search(&t).map_err(err)
    })
    .await
}

#[tauri::command]
async fn art_options(sgdb_id: u64, art: String) -> Res<Vec<artwork::SgdbImage>> {
    let a = Art::parse(&art).ok_or("unknown picture type")?;
    blocking(move || artwork::sgdb_images(sgdb_id, a).map_err(err)).await
}

/// After a picture changed: copy it into Steam's artwork folder too.
fn after_art_change(cfg: &Config, g: &Game) {
    let appid = if g.kind == Kind::Steam { Some(g.appid) } else { steam_ids(cfg).get(&g.id).copied() };
    if let Some(appid) = appid {
        let _ = artwork::apply_to_steam(cfg, g, appid);
    }
}

#[tauri::command]
async fn art_set_url(app: State<'_, App>, id: String, art: String, url: String, sgdb_id: Option<u64>) -> Res<()> {
    let cfg = cfg(&app);
    let a = Art::parse(&art).ok_or("unknown picture type")?;
    blocking(move || {
        let g = find(&id)?;
        let bytes = artwork::download(&url).map_err(err)?;
        artwork::store(&cfg, &g, a, &bytes).map_err(err)?;
        let db = db()?;
        if let Some(s) = sgdb_id {
            let _ = db.set_art_lookup(&g.id, Some(s));
        }
        db.log(&g.id, &format!("{} picture changed", a.name()));
        after_art_change(&cfg, &g);
        Ok(())
    })
    .await
}

#[tauri::command]
async fn art_set_file(app: State<'_, App>, id: String, art: String, path: String) -> Res<()> {
    let cfg = cfg(&app);
    let a = Art::parse(&art).ok_or("unknown picture type")?;
    blocking(move || {
        let g = find(&id)?;
        artwork::store_file(&cfg, &g, a, &PathBuf::from(path)).map_err(err)?;
        db()?.log(&g.id, &format!("{} picture uploaded", a.name()));
        after_art_change(&cfg, &g);
        Ok(())
    })
    .await
}

#[tauri::command]
async fn art_remove(app: State<'_, App>, id: String, art: String) -> Res<()> {
    let cfg = cfg(&app);
    let a = Art::parse(&art).ok_or("unknown picture type")?;
    blocking(move || {
        let g = find(&id)?;
        artwork::remove(&cfg, &g, a).map_err(err)?;
        after_art_change(&cfg, &g);
        Ok(())
    })
    .await
}

#[tauri::command]
async fn info_save(app: State<'_, App>, id: String, info: artwork::Info) -> Res<()> {
    let cfg = cfg(&app);
    blocking(move || {
        let g = find(&id)?;
        artwork::save_info(&cfg, &g, &info).map_err(err)
    })
    .await
}

#[tauri::command]
async fn screenshots_add(app: State<'_, App>, id: String, paths: Vec<String>) -> Res<usize> {
    let cfg = cfg(&app);
    blocking(move || {
        let g = find(&id)?;
        let files: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let n = artwork::add_screenshots(&cfg, &g, &files).map_err(err)?;
        db()?.log(&g.id, &format!("{n} screenshots added"));
        Ok(n)
    })
    .await
}

#[tauri::command]
async fn screenshot_remove(app: State<'_, App>, id: String, name: String) -> Res<()> {
    let cfg = cfg(&app);
    blocking(move || {
        let g = find(&id)?;
        artwork::remove_screenshot(&cfg, &g, &name).map_err(err)
    })
    .await
}

/// Move or rename a game. Returns its new id.
#[tauri::command]
async fn relocate(app: State<'_, App>, id: String, kind: Kind, group: String, name: String, version: String) -> Res<String> {
    let cfg = cfg(&app);
    blocking(move || {
        let g = find(&id)?;
        let mut db = db()?;
        library::relocate(&cfg, &mut db, &g, kind, &group, &name, &version).map_err(err)
    })
    .await
}

/// Move or rename a whole game with all its versions.
#[tauri::command]
async fn relocate_title(app: State<'_, App>, id: String, kind: Kind, group: String, name: String) -> Res<String> {
    let cfg = cfg(&app);
    blocking(move || {
        let t = find_title(&id)?;
        let mut db = db()?;
        library::relocate_title(&cfg, &mut db, &t, kind, &group, &name).map_err(err)
    })
    .await
}

/// A folder of several games: separate games (true) or one game in versions (false).
#[tauri::command]
async fn set_separate(app: State<'_, App>, id: String, separate: bool) -> Res<()> {
    let cfg = cfg(&app);
    blocking(move || {
        let g = find(&id)?;
        let mut db = db()?;
        library::set_separate(&cfg, &mut db, &g, separate).map_err(err)
    })
    .await
}

/// Mark a version as the newest ("" = work it out from names and dates).
#[tauri::command]
async fn set_newest(app: State<'_, App>, id: String, version: String) -> Res<()> {
    let cfg = cfg(&app);
    blocking(move || {
        let t = find_title(&id)?;
        library::set_newest(&cfg, &t, &version).map_err(err)?;
        artwork::mirror(&cfg, &t.game);
        let label = t.versions.iter().find(|v| v.version == version).map(|v| v.version_label());
        db()?.log(&t.id(), &match label {
            Some(l) => format!("{l} marked as the newest version"),
            None => "the newest version is worked out again".into(),
        });
        Ok(())
    })
    .await
}

/// Choose the version Play uses ("" = always the newest).
#[tauri::command]
async fn set_version(app: State<'_, App>, id: String, version: String) -> Res<()> {
    let cfg = cfg(&app);
    blocking(move || {
        let t = find_title(&id)?;
        library::set_version(&cfg, &t, &version).map_err(err)?;
        artwork::mirror(&cfg, &t.game);
        let label = t.versions.iter().find(|v| v.version == version).map(|v| v.version_label());
        db()?.log(&t.id(), &match label {
            Some(l) => format!("Play uses version {l}"),
            None => "Play uses the newest version".into(),
        });
        Ok(())
    })
    .await
}

/// Quit Steam if it runs, update its library, start it again.
#[tauri::command]
fn sync_steam_restart(handle: tauri::AppHandle, app: State<'_, App>) -> u64 {
    let cfg = cfg(&app);
    app.jobs.spawn(&handle, "Updating Steam's library", move |ctx: Ctx| {
        let was_running = steam::running();
        if was_running {
            ctx.status("quitting Steam");
            steam_fan_core::platform::quit_steam(&cfg);
            let start = std::time::Instant::now();
            while steam::running() {
                if start.elapsed().as_secs() > 90 {
                    anyhow::bail!("Steam didn't quit within 90 seconds; quit it yourself (Steam > Exit) and try again");
                }
                if ctx.cancel.is_cancelled() {
                    anyhow::bail!("stopped");
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            std::thread::sleep(std::time::Duration::from_secs(2)); // let it finish writing its files
        }
        let mut db = Db::open_default()?;
        let out = library::sync_to_steam(&cfg, &mut db, &[], false, false, |m| ctx.status(m))?;
        if was_running {
            ctx.status("starting Steam");
            // on its own, so it keeps running when this window closes
            steam_fan_core::platform::start_steam(&cfg);
        }
        Ok(format!("Steam library: {}{}", out.steam.summary(), if was_running { "; Steam is starting again" } else { "" }))
    })
}

/// Check a game's files against its checksums; repair the PC copy from the shelf.
#[tauri::command]
fn verify(handle: tauri::AppHandle, app: State<'_, App>, id: String) -> Res<u64> {
    let cfg = cfg(&app);
    let g = find(&id)?;
    Ok(app.jobs.spawn(&handle, &format!("Verifying {}", g.full_name()), move |ctx: Ctx| {
        let rep = steam_fan_core::integrity::verify(&cfg, &g, &ctx.cancel, &mut |s| ctx.status(s), &mut |p| ctx.progress(p))?;
        let msg = rep.summary();
        Db::open_default()?.log(&g.id, &format!("verified: {msg}"));
        if !rep.ok() {
            anyhow::bail!("{msg}");
        }
        Ok(msg)
    }))
}

/// Download a game again from its link: replace the shelf copy, or add it as a new version.
#[tauri::command]
fn redownload(handle: tauri::AppHandle, app: State<'_, App>, id: String, version: Option<String>, file: Option<String>) -> Res<u64> {
    let cfg = cfg(&app);
    let g = find(&id)?;
    Ok(app.jobs.spawn(&handle, &format!("Downloading {}", g.full_name()), move |ctx: Ctx| {
        let mut db = Db::open_default()?;
        let file = file.filter(|f| !f.is_empty()).map(PathBuf::from);
        let dst = library::redownload(&cfg, &mut db, &g, version.as_deref(), file.as_deref(), &ctx.cancel, &mut |s| ctx.status(s), &mut |p| ctx.progress(p))?;
        Ok(format!("Downloaded and put on the shelf at {}", dst.display()))
    }))
}

#[tauri::command]
async fn check_update(app: State<'_, App>, id: String) -> Res<String> {
    let cfg = cfg(&app);
    blocking(move || {
        let g = find(&id)?;
        if cfg.server_mode() {
            return steam_fan_core::remote::check_update(&cfg, &g).map_err(err);
        }
        steam_fan_core::source::check_update(&g).map_err(err)
    })
    .await
}

#[tauri::command]
async fn page_images(app: State<'_, App>, url: String) -> Res<Vec<String>> {
    let cfg = cfg(&app);
    blocking(move || {
        if cfg.server_mode() {
            return steam_fan_core::remote::page_images(&cfg, &url).map_err(err);
        }
        artwork::page_images(&url).map_err(err)
    })
    .await
}

#[tauri::command]
async fn screenshots_add_urls(app: State<'_, App>, id: String, urls: Vec<String>) -> Res<artwork::Added> {
    let cfg = cfg(&app);
    blocking(move || {
        let g = find(&id)?;
        let a = artwork::add_screenshots_from_urls(&cfg, &g, &urls).map_err(err)?;
        db()?.log(&g.id, &format!("{} screenshots added from a web page", a.added));
        Ok(a)
    })
    .await
}

// ---------------------------------------------------------------- adding games

#[tauri::command]
async fn import_inspect(app: State<'_, App>, path: String) -> Res<import::Inspect> {
    let cfg = cfg(&app);
    blocking(move || import::inspect(&cfg, &PathBuf::from(path)).map_err(err)).await
}

#[tauri::command]
fn import_place(handle: tauri::AppHandle, app: State<'_, App>, source: String, kind: Kind, group: String, name: String, version: String) -> u64 {
    let cfg = cfg(&app);
    app.jobs.spawn(&handle, &format!("Adding {name} to the shelf"), move |ctx: Ctx| {
        if cfg.server_mode() {
            let mut db = Db::open_default()?;
            let msg = steam_fan_core::remote::import(&cfg, &mut db, &PathBuf::from(&source), kind, &group, &name, &version, &ctx.cancel, &mut |s| ctx.status(s), &mut |p| ctx.progress(p))?;
            if !steam::running() {
                let _ = library::sync_to_steam(&cfg, &mut db, &[], false, false, |_| {});
            }
            return Ok(msg);
        }
        let (dst, sums) = import::place(&cfg, &PathBuf::from(&source), kind, &group, &name, &version, &ctx.cancel, &mut |s| ctx.status(s), &mut |p| ctx.progress(p))?;
        ctx.status("reading the shelf");
        let mut db = Db::open_default()?;
        library::refresh(&cfg, &mut db, false, |_| {})?;
        let mut games: Vec<Game> = db.games()?.into_iter().filter(|g| g.shelf_path(&cfg) == dst).collect();
        library::find_programs(&cfg, &db, &mut games, &[], false, |_| {})?;
        ctx.status("artwork");
        library::refresh_art(&cfg, &db, &games, true, false, |s| ctx.status(s));
        for g in &games {
            let _ = steam_fan_core::integrity::write(&steam_fan_core::integrity::manifest_path(&cfg, g), &sums);
            db.log(&g.id, "added to the shelf");
        }
        let mut msg = format!("{name} is on the shelf");
        if steam::running() {
            msg.push_str(". Quit Steam and press Update Steam library to see it there");
        } else {
            let only: Vec<String> = games.iter().map(|g| g.id.clone()).collect();
            if !only.is_empty() {
                let out = library::sync_to_steam(&cfg, &mut db, &only, false, false, |_| {})?;
                if !out.steam.added.is_empty() {
                    msg.push_str(" and in Steam");
                }
            }
        }
        Ok(msg)
    })
}

/// Group or system names in use, for suggestions.
#[tauri::command]
async fn groups(app: State<'_, App>) -> Res<HashMap<String, Vec<String>>> {
    let cfg = cfg(&app);
    blocking(move || {
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        for g in db()?.games().map_err(err)? {
            let key = g.kind.prefix().to_string();
            let name = if g.kind == Kind::Rom { g.system.clone() } else { g.rel.split('/').next().unwrap_or("").to_string() };
            let v = out.entry(key).or_default();
            if !v.contains(&name) {
                v.push(name);
            }
        }
        let roms = out.entry("rom".into()).or_default();
        for e in &cfg.emulators {
            if !roms.iter().any(|r| r.eq_ignore_ascii_case(&e.system)) {
                roms.push(e.system.clone());
            }
        }
        for v in out.values_mut() {
            v.sort_by_key(|s| s.to_lowercase());
        }
        Ok(out)
    })
    .await
}

// ---------------------------------------------------------------- settings

#[derive(Serialize)]
struct Key {
    masked: String,
    source: &'static str,
}

#[derive(Serialize)]
struct SettingsView {
    config: Config,
    config_path: String,
    secrets_path: String,
    keys: HashMap<&'static str, Key>,
    /// whether this PC holds a server token
    paired: bool,
}

#[tauri::command]
fn settings(app: State<'_, App>) -> SettingsView {
    let mut keys = HashMap::new();
    for k in [secrets::STEAMGRIDDB, secrets::STEAM_API, secrets::STEAM_ID] {
        keys.insert(k, Key { masked: secrets::masked(k), source: secrets::source(k) });
    }
    SettingsView {
        config: cfg(&app),
        config_path: Config::path().display().to_string(),
        secrets_path: secrets::path().display().to_string(),
        keys,
        paired: !secrets::get(steam_fan_core::remote::TOKEN).is_empty(),
    }
}

#[tauri::command]
fn settings_save(app: State<'_, App>, config: Config) -> Res<()> {
    config.save().map_err(err)?;
    steam_fan_core::remote::forget();
    *app.cfg.lock().unwrap() = config;
    Ok(())
}

#[tauri::command]
fn secret_set(name: String, value: String) -> Res<()> {
    if ![secrets::STEAMGRIDDB, secrets::STEAM_API, secrets::STEAM_ID].contains(&name.as_str()) {
        return Err("unknown key".into());
    }
    secrets::set(&name, &value).map_err(err)
}

/// Pair this PC with a Steam-Fan-Server using the code `steam-fan-server pair` shows.
/// The address is added to the server list.
#[tauri::command]
async fn server_pair(app: State<'_, App>, url: String, code: String) -> Res<Vec<String>> {
    let url = url.trim().trim_end_matches('/').to_string();
    let u = url.clone();
    blocking(move || steam_fan_core::remote::pair(&u, code.trim(), &steam_fan_core::remote::device_name()).map_err(err)).await?;
    let mut c = cfg(&app);
    if !c.servers.contains(&url) {
        c.servers.insert(0, url);
    }
    c.save().map_err(err)?;
    steam_fan_core::remote::forget();
    let list = c.servers.clone();
    *app.cfg.lock().unwrap() = c;
    Ok(list)
}

/// Whether a server address answers.
#[tauri::command]
async fn server_reachable(url: String) -> Res<bool> {
    blocking(move || {
        let mut c = Config::default();
        c.servers = vec![url];
        steam_fan_core::remote::forget();
        let ok = steam_fan_core::remote::active(&c).is_some();
        steam_fan_core::remote::forget();
        Ok(ok)
    })
    .await
}

#[tauri::command]
fn shelf_reachable(path: String) -> bool {
    steam_fan_core::config::shelf_online(&steam_fan_core::config::expand(&path))
}

// ---------------------------------------------------------------- desktop

#[tauri::command]
fn open(target: String) {
    steam_fan_core::platform::open(&target);
}

#[tauri::command]
fn play(id: String) -> Res<()> {
    let g = find(&id)?;
    let url = if g.kind == Kind::Steam {
        format!("steam://rungameid/{}", g.appid)
    } else {
        let cfg = Config::load().map_err(err)?;
        let appid = steam_ids(&cfg).get(&g.id).copied().ok_or("not in Steam's library yet: press Update Steam library")?;
        steam::run_url(appid)
    };
    open(url);
    Ok(())
}

#[tauri::command]
fn human(bytes: u64) -> String {
    transfer::human(bytes)
}

/// Pictures for the interface: `art://localhost/<game id>/<cover|hero|logo|icon|wide>`.
/// With a server, pictures not cached yet are fetched from it first.
fn serve_art(cfg: &Config, req: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Cow<'static, [u8]>> {
    let path = req.uri().path().trim_start_matches('/');
    let reply = |status: u16, mime: &str, body: Vec<u8>| {
        tauri::http::Response::builder()
            .status(status)
            .header("Content-Type", mime)
            .header("Cache-Control", "max-age=31536000")
            .body(Cow::Owned(body))
            .unwrap()
    };
    let Some((id, kind)) = path.rsplit_once('/') else { return reply(400, "text/plain", vec![]) };
    let kind = decode(kind);
    let stub = Game {
        id: decode(id),
        kind: Kind::Fan,
        rel: String::new(),
        system: String::new(),
        name: String::new(),
        version: String::new(),
        group: String::new(),
        program: String::new(),
        platform: Platform::Windows,
        appid: 0,
        buildid: String::new(),
        installdir: String::new(),
        size: 0,
        built: 0,
    };
    if cfg.server_mode() {
        if let Some(name) = kind.strip_prefix("shot:") {
            let _ = steam_fan_core::remote::screenshot(cfg, &stub, name);
        } else {
            let _ = steam_fan_core::remote::fetch_art(cfg, &stub);
        }
    }
    let file = if kind == "cover-thumb" {
        artwork::thumb(&stub)
    } else if let Some(name) = kind.strip_prefix("shot:") {
        artwork::screenshots(&stub).into_iter().find(|p| p.file_name().map_or(false, |f| f.to_string_lossy() == name))
    } else {
        Art::parse(&kind).and_then(|a| artwork::local(&stub, a))
    };
    match file.and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
        Some((p, body)) => {
            let mime = match p.extension().and_then(|e| e.to_str()) {
                Some("jpg" | "jpeg") => "image/jpeg",
                Some("webp") => "image/webp",
                _ => "image/png",
            };
            reply(200, mime, body)
        }
        None => reply(404, "text/plain", vec![]),
    }
}

fn main() {
    let cfg = Config::load().unwrap_or_default();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("art", |ctx, req, responder| {
            // off the window's thread: fetching from the server can take a moment
            let cfg = ctx.app_handle().state::<App>().cfg.lock().unwrap().clone();
            std::thread::spawn(move || responder.respond(serve_art(&cfg, &req)));
        })
        .manage(App { cfg: Mutex::new(cfg), jobs: Jobs::default() })
        .invoke_handler(tauri::generate_handler![
            library, status, details, jobs, job_cancel, refresh, sync_steam, install, remove, archive_upload,
            refresh_art, art_search, art_options, art_set_url, art_set_file, art_remove, import_inspect,
            import_place, groups, info_save, screenshots_add, screenshot_remove, relocate, relocate_title, set_version, set_newest, set_separate, sync_steam_restart, verify, redownload, check_update, page_images, screenshots_add_urls, settings, settings_save, secret_set, shelf_reachable, server_pair, server_reachable, open, play, human
        ])
        .setup(|app| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title("Steam-Fan");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("cannot start the Steam-Fan window");
}
