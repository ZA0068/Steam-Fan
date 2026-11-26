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

fn find(id: &str) -> Res<Game> {
    db()?.games().map_err(err)?.into_iter().find(|g| g.id == id).ok_or_else(|| format!("{id} is not in the library"))
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
    hero: Option<String>,
    logo: Option<String>,
    icon: Option<String>,
}

#[derive(Serialize)]
struct GameView {
    id: String,
    name: String,
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
    Some(format!("art://localhost/{}/{}?v={v}", encode(&g.id), a.name()))
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
        let ids = steam::shortcut_ids(&cfg);
        let views = games
            .into_iter()
            .map(|g| {
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
                        hero: art_url(&g, Art::Hero),
                        logo: art_url(&g, Art::Logo),
                        icon: art_url(&g, Art::Icon),
                    },
                    steam_id: if g.kind == Kind::Steam { Some(g.appid) } else { ids.get(&g.id).copied() },
                    kind_label: g.kind.label(),
                    playable: g.kind == Kind::Steam || !g.program.is_empty(),
                    fresh: fresh.map(|f| f.label()),
                    fresh_code: fresh.map(|f| format!("{f:?}")),
                    on_pc,
                    id: g.id,
                    name: g.name,
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
    let shelf = cfg(&app).shelf();
    // a NAS that is off can make the mount hang: give up after 4 seconds
    let online = blocking(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(steam_fan_core::config::shelf_online(&shelf));
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
            history: db.events(Some(&g.id), 12).map_err(err)?.into_iter().map(|e| (e.ts, e.msg)).collect(),
            art_on_shelf: if cfg.shelf_online() { artwork::shelf_pictures(&cfg, &g).into_iter().map(|(a, _)| a).collect() } else { vec![] },
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
    let appid = if g.kind == Kind::Steam { Some(g.appid) } else { steam::shortcut_ids(cfg).get(&g.id).copied() };
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

// ---------------------------------------------------------------- adding games

#[tauri::command]
async fn import_inspect(app: State<'_, App>, path: String) -> Res<import::Inspect> {
    let cfg = cfg(&app);
    blocking(move || import::inspect(&cfg, &PathBuf::from(path)).map_err(err)).await
}

#[tauri::command]
fn import_place(handle: tauri::AppHandle, app: State<'_, App>, source: String, kind: Kind, group: String, name: String) -> u64 {
    let cfg = cfg(&app);
    app.jobs.spawn(&handle, &format!("Adding {name} to the shelf"), move |ctx: Ctx| {
        let dst = import::place(&cfg, &PathBuf::from(&source), kind, &group, &name, &ctx.cancel, &mut |s| ctx.status(s), &mut |p| ctx.progress(p))?;
        ctx.status("reading the shelf");
        let mut db = Db::open_default()?;
        library::refresh(&cfg, &mut db, false, |_| {})?;
        let mut games: Vec<Game> = db.games()?.into_iter().filter(|g| g.shelf_path(&cfg) == dst).collect();
        library::find_programs(&cfg, &db, &mut games, &[], false, |_| {})?;
        ctx.status("artwork");
        library::refresh_art(&cfg, &db, &games, true, false, |s| ctx.status(s));
        for g in &games {
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
    }
}

#[tauri::command]
fn settings_save(app: State<'_, App>, config: Config) -> Res<()> {
    config.save().map_err(err)?;
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

#[tauri::command]
fn shelf_reachable(path: String) -> bool {
    steam_fan_core::config::shelf_online(&steam_fan_core::config::expand(&path))
}

// ---------------------------------------------------------------- desktop

#[tauri::command]
fn open(target: String) {
    let _ = std::process::Command::new("xdg-open").arg(target).spawn();
}

#[tauri::command]
fn play(id: String) -> Res<()> {
    let g = find(&id)?;
    let url = if g.kind == Kind::Steam {
        format!("steam://rungameid/{}", g.appid)
    } else {
        let cfg = Config::load().map_err(err)?;
        let appid = steam::shortcut_ids(&cfg).get(&g.id).copied().ok_or("not in Steam's library yet: press Update Steam library")?;
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
fn serve_art(req: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Cow<'static, [u8]>> {
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
    let (Some(art), id) = (Art::parse(kind), decode(id)) else { return reply(400, "text/plain", vec![]) };
    let stub = Game {
        id,
        kind: Kind::Fan,
        rel: String::new(),
        system: String::new(),
        name: String::new(),
        group: String::new(),
        program: String::new(),
        platform: Platform::Windows,
        appid: 0,
        buildid: String::new(),
        installdir: String::new(),
        size: 0,
    };
    match artwork::local(&stub, art).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
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
        .register_uri_scheme_protocol("art", |_, req| serve_art(&req))
        .manage(App { cfg: Mutex::new(cfg), jobs: Jobs::default() })
        .invoke_handler(tauri::generate_handler![
            library, status, details, jobs, job_cancel, refresh, sync_steam, install, remove, archive_upload,
            refresh_art, art_search, art_options, art_set_url, art_set_file, art_remove, import_inspect,
            import_place, groups, settings, settings_save, secret_set, shelf_reachable, open, play, human
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
