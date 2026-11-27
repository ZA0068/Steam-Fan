//! Steam-Fan-Server: the shelf as a service.
//!
//! Runs in Docker on the NAS next to the games. It owns the catalog, downloads games
//! (direct links, Google Drive, Dropbox, MEGA), checks their links for updates,
//! keeps checksums and versions, finds artwork, and streams game files to the PCs.
//! The PC app only asks.
//!
//! ```text
//! steam-fan-server            run the server (the default)
//! steam-fan-server pair       print a 6-digit code to pair a PC (valid 10 minutes)
//! steam-fan-server devices    list paired PCs
//! steam-fan-server revoke N   unpair the PC named N
//! ```
//!
//! Settings come from environment variables (see `docker/compose.yaml`).

mod auth;
mod jobs;
mod updates;

use anyhow::{bail, Context, Result};
use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Query, Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use steam_fan_core::artwork::{self, Art, Info};
use steam_fan_core::{import, integrity, launch, library, source, Config, Db, Game, Kind};
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------- settings

fn env(name: &str, default: &str) -> String {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| default.to_string())
}

struct Settings {
    port: u16,
    /// Read the shelf for new or changed games this often (0: only on request).
    scan_minutes: u64,
    /// Ask download links for updates this often (0: only on request).
    update_hours: u64,
    /// Download changed games by themselves (replacing the shelf copy) instead of only reporting them.
    auto_update: bool,
}

/// The data folder holds the database, artwork cache, downloads in progress and paired devices.
/// Steam-Fan's core keeps those in the XDG folders, so they all point into it.
fn setup() -> (Config, Settings, PathBuf) {
    let data = PathBuf::from(env("SF_DATA", "/data"));
    let _ = std::fs::create_dir_all(&data);
    for (var, sub) in [("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "share"), ("XDG_CACHE_HOME", "cache"), ("TMPDIR", "tmp")] {
        let p = data.join(sub);
        let _ = std::fs::create_dir_all(&p);
        // no threads run yet
        std::env::set_var(var, p);
    }
    std::env::set_var("HOME", &data);
    let mut cfg = Config::load().unwrap_or_else(|e| {
        eprintln!("config.toml: {e:#}; using defaults");
        Config::default()
    });
    cfg.shelf = env("SF_SHELF", "/shelf");
    let s = Settings {
        port: env("SF_PORT", "8484").parse().unwrap_or(8484),
        scan_minutes: env("SF_SCAN_MINUTES", "15").parse().unwrap_or(15),
        update_hours: env("SF_UPDATE_HOURS", "12").parse().unwrap_or(12),
        auto_update: env("SF_AUTO_UPDATE", "off").eq_ignore_ascii_case("replace"),
    };
    (cfg, s, data)
}

// ---------------------------------------------------------------- state and errors

#[derive(Clone)]
struct App {
    cfg: Arc<Config>,
    jobs: Arc<jobs::Jobs>,
    updates: Arc<updates::Updates>,
    auth: Arc<auth::Auth>,
    data: Arc<PathBuf>,
}

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        let msg = format!("{e:#}");
        let code = if msg.contains("is not in the library") { StatusCode::NOT_FOUND } else { StatusCode::BAD_REQUEST };
        ApiError(code, msg)
    }
}

type Res<T> = std::result::Result<T, ApiError>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Res<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(ApiError::from)
}

fn find(cfg: &Config, id: &str) -> Result<Game> {
    let mut db = Db::open_default()?;
    launch::find_game(cfg, &mut db, id)
}

fn parse_kind(s: &str) -> Result<Kind> {
    Ok(match s.to_lowercase().replace(['-', '_', ' '], "").as_str() {
        "fan" | "fangame" | "fangames" => Kind::Fan,
        "nonsteam" => Kind::NonSteam,
        "rom" | "roms" => Kind::Rom,
        _ => bail!("kind must be fan, nonsteam or rom"),
    })
}

fn parse_art(s: &str) -> Result<Art> {
    serde_json::from_value(serde_json::Value::String(s.to_lowercase())).context("picture kind must be cover, hero, logo, icon or wide")
}

// ---------------------------------------------------------------- auth

fn token_of(req: &Request) -> String {
    if let Some(v) = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        return v.strip_prefix("Bearer ").unwrap_or(v).trim().to_string();
    }
    // picture links in a web page can't send headers
    req.uri()
        .query()
        .unwrap_or("")
        .split('&')
        .find_map(|kv| kv.strip_prefix("token="))
        .unwrap_or("")
        .to_string()
}

async fn require_token(State(app): State<App>, req: Request, next: Next) -> Response {
    let tok = token_of(&req);
    let auth = app.auth.clone();
    let ok = tokio::task::spawn_blocking(move || auth.check(&tok)).await.ok().flatten();
    match ok {
        Some(_) => next.run(req).await,
        None => ApiError(StatusCode::UNAUTHORIZED, "not paired: pair this PC with the server first".into()).into_response(),
    }
}

#[derive(Deserialize)]
struct PairReq {
    code: String,
    #[serde(default)]
    device: String,
}

async fn pair(State(app): State<App>, Json(r): Json<PairReq>) -> Res<Json<serde_json::Value>> {
    let auth = app.auth.clone();
    match blocking(move || auth.pair(&r.code, &r.device)).await {
        Ok(token) => Ok(Json(serde_json::json!({ "token": token }))),
        Err(e) => {
            // slow down guessing
            tokio::time::sleep(Duration::from_secs(1)).await;
            Err(ApiError(StatusCode::FORBIDDEN, e.1))
        }
    }
}

async fn devices(State(app): State<App>) -> Json<serde_json::Value> {
    let d: Vec<_> = app.auth.devices().into_iter().map(|d| serde_json::json!({ "name": d.name, "paired": d.paired, "last_seen": d.last_seen })).collect();
    Json(serde_json::json!(d))
}

// ---------------------------------------------------------------- catalog

#[derive(Serialize)]
struct Health {
    name: &'static str,
    version: &'static str,
    shelf: String,
    shelf_online: bool,
    games: usize,
    auto_update: bool,
}

async fn health(State(app): State<App>) -> Res<Json<Health>> {
    let cfg = app.cfg.clone();
    let auto = std::env::var("SF_AUTO_UPDATE").map_or(false, |v| v.eq_ignore_ascii_case("replace"));
    blocking(move || {
        let games = Db::open_default().and_then(|db| db.games()).map(|g| g.len()).unwrap_or(0);
        Ok(Json(Health { name: "Steam-Fan-Server", version: VERSION, shelf: cfg.shelf.clone(), shelf_online: cfg.shelf_online(), games, auto_update: auto }))
    })
    .await
}

#[derive(Serialize)]
struct VersionOut {
    id: String,
    folder: String,
    label: String,
    built: i64,
    old: bool,
}

#[derive(Serialize)]
struct GameOut {
    /// The library entry's id: the same for every version of a game.
    title_id: String,
    /// Every version, newest first (empty: no versions); `game` is the one Play uses.
    versions: Vec<VersionOut>,
    pinned: bool,
    #[serde(flatten)]
    game: Game,
    /// Pictures the server has for it.
    art: Vec<Art>,
    has_link: bool,
    update: Option<updates::Update>,
}

fn game_out(app: &App, t: library::Title) -> GameOut {
    let g = t.game;
    let art = Art::all().into_iter().filter(|a| artwork::local(&g, *a).is_some()).collect();
    let has_link = !artwork::info(&g).download.trim().is_empty();
    let update = app.updates.get(&g.id);
    let versions = t
        .versions
        .iter()
        .map(|v| VersionOut { id: v.id.clone(), folder: v.version.clone(), label: v.version_label(), built: v.built, old: steam_fan_core::shelf::is_old_version(&v.version) })
        .collect();
    GameOut { title_id: g.title_id(), versions, pinned: t.pinned, game: g, art, has_link, update }
}

/// One entry per game; a game kept in several versions lists them, with the one Play uses.
async fn library_list(State(app): State<App>) -> Res<Json<Vec<GameOut>>> {
    blocking(move || {
        let games = Db::open_default()?.games()?;
        Ok(Json(library::titles(games).into_iter().map(|t| game_out(&app, t)).collect()))
    })
    .await
}

fn find_title(id: &str) -> Result<library::Title> {
    let games = Db::open_default()?.games()?;
    let g = library::resolve(&games, id).with_context(|| format!("{id} is not in the library"))?;
    let tid = g.title_id();
    library::titles(games.into_iter().filter(|x| x.title_id() == tid).collect()).into_iter().next().context("not in the library")
}

#[derive(Deserialize)]
struct SetVersionReq {
    id: String,
    /// The version's folder name; empty: always the newest.
    #[serde(default)]
    version: String,
}

/// Choose the version Play uses, for every PC.
async fn set_version(State(app): State<App>, Json(r): Json<SetVersionReq>) -> Res<Json<&'static str>> {
    let cfg = app.cfg.clone();
    blocking(move || {
        let t = find_title(&r.id)?;
        library::set_version(&cfg, &t, &r.version)?;
        artwork::mirror(&cfg, &t.game);
        Ok(Json("saved"))
    })
    .await
}

#[derive(Deserialize)]
struct IdQ {
    id: String,
}

#[derive(Serialize)]
struct Details {
    #[serde(flatten)]
    game: GameOut,
    info: Info,
    screenshots: Vec<String>,
    history: Vec<(String, String)>,
    /// Checksums are kept for it (clients can then fetch only changed files).
    checksums: bool,
}

async fn game_details(State(app): State<App>, Query(q): Query<IdQ>) -> Res<Json<Details>> {
    blocking(move || {
        let t = find_title(&q.id)?;
        let g = t.game.clone();
        let db = Db::open_default()?;
        let checksums = integrity::manifest_path(&app.cfg, &g).is_file();
        let history = db.events(Some(&g.id), 20)?.into_iter().map(|e| (e.ts, e.msg)).collect();
        let screenshots = artwork::screenshots(&g).into_iter().filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
        let info = artwork::info(&g);
        Ok(Json(Details { game: game_out(&app, t), info, screenshots, history, checksums }))
    })
    .await
}

// ---------------------------------------------------------------- game files

#[derive(Serialize, Debug, PartialEq)]
struct FileOut {
    path: String,
    size: u64,
    /// Seconds since 1970.
    mtime: i64,
    sha256: Option<String>,
}

#[derive(Serialize)]
struct ManifestOut {
    id: String,
    /// The game is one program file, not a folder.
    single_file: bool,
    total: u64,
    checksums: bool,
    files: Vec<FileOut>,
}

/// Files Steam-Fan and the NAS put next to games, never sent to a PC.
fn ours(rel: &str) -> bool {
    rel.split('/').any(|p| p == "@eaDir" || p == "#recycle" || p.starts_with(".steam-fan") || p == ".nas-complete" || p == ".nas-prefix" || p.ends_with(".steam-fan-part"))
}

fn mtime_of(m: &std::fs::Metadata) -> i64 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64)
}

/// Every file of a game, with checksums where they are recorded (and the size still matches).
fn list_files(root: &Path, sums: Option<&integrity::Manifest>) -> Result<Vec<FileOut>> {
    let mut out = Vec::new();
    for e in walkdir::WalkDir::new(root).follow_links(false).sort_by_file_name() {
        let e = e?;
        if !e.file_type().is_file() {
            continue;
        }
        let rel = e.path().strip_prefix(root)?.to_string_lossy().replace('\\', "/");
        if ours(&rel) {
            continue;
        }
        let m = e.metadata()?;
        let sha256 = sums.and_then(|s| s.get(&rel)).filter(|(size, _)| *size == m.len()).map(|(_, h)| h.clone());
        out.push(FileOut { path: rel, size: m.len(), mtime: mtime_of(&m), sha256 });
    }
    Ok(out)
}

async fn manifest(State(app): State<App>, Query(q): Query<IdQ>) -> Res<Json<ManifestOut>> {
    blocking(move || {
        let g = find(&app.cfg, &q.id)?;
        let root = g.shelf_path(&app.cfg);
        let sums = integrity::read(&integrity::manifest_path(&app.cfg, &g));
        let (single_file, files) = if root.is_file() {
            let m = std::fs::metadata(&root)?;
            let name = root.file_name().unwrap().to_string_lossy().into_owned();
            let sha256 = sums.as_ref().and_then(|s| s.values().next()).filter(|(size, _)| *size == m.len()).map(|(_, h)| h.clone());
            (true, vec![FileOut { path: name, size: m.len(), mtime: mtime_of(&m), sha256 }])
        } else if root.is_dir() {
            (false, list_files(&root, sums.as_ref())?)
        } else {
            bail!("{} is missing on the shelf", root.display());
        };
        Ok(Json(ManifestOut { id: g.id, single_file, total: files.iter().map(|f| f.size).sum(), checksums: sums.is_some(), files }))
    })
    .await
}

/// The file `rel` inside a game, refusing anything that would leave the game's folder.
fn resolve(root: &Path, rel: &str) -> Result<PathBuf> {
    if root.is_file() {
        if rel.is_empty() || root.file_name().map_or(false, |n| n.to_string_lossy() == rel) {
            return Ok(root.to_path_buf());
        }
        bail!("this game is a single file");
    }
    let rp = Path::new(rel);
    if rel.is_empty() || rp.components().any(|c| !matches!(c, Component::Normal(_))) || ours(rel) {
        bail!("not a file of this game: {rel}");
    }
    let p = root.join(rp).canonicalize().with_context(|| format!("{rel} isn't in this game"))?;
    if !p.starts_with(root.canonicalize()?) || !p.is_file() {
        bail!("not a file of this game: {rel}");
    }
    Ok(p)
}

#[derive(Deserialize)]
struct FileQ {
    id: String,
    #[serde(default)]
    path: String,
}

/// One game file. Supports `Range`, so a PC can resume a cut-off download.
async fn file(State(app): State<App>, Query(q): Query<FileQ>, req: Request) -> Res<Response> {
    let cfg = app.cfg.clone();
    let p = blocking(move || resolve(&find(&cfg, &q.id)?.shelf_path(&cfg), &q.path)).await?;
    serve(p, req).await
}

async fn serve(p: PathBuf, req: Request) -> Res<Response> {
    let res = ServeFile::new(p).oneshot(req).await.map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(res.map(Body::new))
}

// ---------------------------------------------------------------- pictures and info

#[derive(Deserialize)]
struct ArtQ {
    id: String,
    /// cover, hero, logo, icon, wide, or thumb (a small cover)
    #[serde(default)]
    kind: String,
    /// a screenshot's file name
    #[serde(default)]
    shot: String,
}

async fn art_get(State(app): State<App>, Query(q): Query<ArtQ>, req: Request) -> Res<Response> {
    let cfg = app.cfg.clone();
    let p = blocking(move || {
        let g = find(&cfg, &q.id)?;
        let pick = |g: &Game| -> Result<Option<PathBuf>> {
            if !q.shot.is_empty() {
                if q.shot.contains('/') || q.shot.starts_with('.') {
                    bail!("bad screenshot name");
                }
                return Ok(artwork::screenshots(g).into_iter().find(|p| p.file_name().map_or(false, |n| n.to_string_lossy() == q.shot)));
            }
            if q.kind == "thumb" {
                return Ok(artwork::thumb(g));
            }
            Ok(artwork::local(g, parse_art(&q.kind)?))
        };
        if let Some(p) = pick(&g)? {
            return Ok(p);
        }
        // not copied from the shelf yet
        artwork::mirror(&cfg, &g);
        pick(&g)?.context("no such picture")
    })
    .await?;
    serve(p, req).await
}

async fn art_put(State(app): State<App>, Query(q): Query<ArtQ>, body: Bytes) -> Res<Json<&'static str>> {
    let cfg = app.cfg.clone();
    blocking(move || {
        let g = find(&cfg, &q.id)?;
        artwork::store(&cfg, &g, parse_art(&q.kind)?, &body)?;
        Db::open_default()?.log(&g.id, &format!("{} picture set", q.kind));
        Ok(Json("saved"))
    })
    .await
}

async fn art_delete(State(app): State<App>, Query(q): Query<ArtQ>) -> Res<Json<&'static str>> {
    let cfg = app.cfg.clone();
    blocking(move || {
        let g = find(&cfg, &q.id)?;
        artwork::remove(&cfg, &g, parse_art(&q.kind)?)?;
        Ok(Json("removed"))
    })
    .await
}

#[derive(Deserialize)]
struct ShotQ {
    id: String,
    name: String,
}

async fn screenshot_add(State(app): State<App>, Query(q): Query<ShotQ>, body: Bytes) -> Res<Json<usize>> {
    let (cfg, data) = (app.cfg.clone(), app.data.clone());
    blocking(move || {
        let g = find(&cfg, &q.id)?;
        let dir = data.join("tmp").join(format!("shot-{}", jobs::now()));
        std::fs::create_dir_all(&dir)?;
        let p = dir.join(safe_name(&q.name, "screenshot.png"));
        std::fs::write(&p, &body)?;
        let n = artwork::add_screenshots(&cfg, &g, &[p]);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(Json(n?))
    })
    .await
}

async fn screenshot_delete(State(app): State<App>, Query(q): Query<ShotQ>) -> Res<Json<&'static str>> {
    let cfg = app.cfg.clone();
    blocking(move || {
        artwork::remove_screenshot(&cfg, &find(&cfg, &q.id)?, &q.name)?;
        Ok(Json("removed"))
    })
    .await
}

#[derive(Deserialize)]
struct UrlQ {
    url: String,
}

/// Pictures on a game's web page (SFGHQ showcase, Game Jolt, itch.io...), fetched by the server.
async fn page_images(Query(q): Query<UrlQ>) -> Res<Json<Vec<String>>> {
    blocking(move || artwork::page_images(q.url.trim()).map(Json)).await
}

#[derive(Deserialize)]
struct ShotsReq {
    id: String,
    urls: Vec<String>,
}

/// The server downloads the pictures and puts them on the shelf; ones that fail are listed.
async fn screenshots_from_urls(State(app): State<App>, Json(r): Json<ShotsReq>) -> Res<Json<artwork::Added>> {
    let cfg = app.cfg.clone();
    blocking(move || {
        let g = find(&cfg, &r.id)?;
        let a = artwork::add_screenshots_from_urls(&cfg, &g, &r.urls)?;
        Db::open_default()?.log(&g.id, &format!("{} screenshots added from a web page", a.added));
        Ok(Json(a))
    })
    .await
}

async fn info_put(State(app): State<App>, Query(q): Query<IdQ>, Json(info): Json<Info>) -> Res<Json<&'static str>> {
    let cfg = app.cfg.clone();
    blocking(move || {
        artwork::save_info(&cfg, &find(&cfg, &q.id)?, &info)?;
        Ok(Json("saved"))
    })
    .await
}

// ---------------------------------------------------------------- uploads

/// A file name from a client, without any folders.
fn safe_name(name: &str, default: &str) -> String {
    let n = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    if n.is_empty() || n.starts_with('.') {
        default.to_string()
    } else {
        n.to_string()
    }
}

/// Stream a request body to `<data>/uploads/<n>/<name>`. Returns the file.
async fn save_upload(data: &Path, name: &str, body: Body) -> Res<PathBuf> {
    let dir = data.join("uploads").join(format!("{}-{}", jobs::now(), std::process::id() ^ (body_id() as u32)));
    tokio::fs::create_dir_all(&dir).await.map_err(|e| ApiError::from(anyhow::Error::from(e)))?;
    let p = dir.join(safe_name(name, "upload.zip"));
    let mut f = tokio::fs::File::create(&p).await.map_err(|e| ApiError::from(anyhow::Error::from(e)))?;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                drop(f);
                let _ = tokio::fs::remove_dir_all(&dir).await;
                return Err(ApiError(StatusCode::BAD_REQUEST, format!("the upload was cut off: {e}")));
            }
        };
        f.write_all(&chunk).await.map_err(|e| ApiError::from(anyhow::Error::from(e)))?;
    }
    f.flush().await.map_err(|e| ApiError::from(anyhow::Error::from(e)))?;
    Ok(p)
}

fn body_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

fn remove_upload(p: &Path) {
    if let Some(dir) = p.parent() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

// ---------------------------------------------------------------- jobs

#[derive(Serialize)]
struct Started {
    job: u64,
}

fn started(r: Result<u64>) -> Res<Json<Started>> {
    r.map(|job| Json(Started { job })).map_err(|e| ApiError(StatusCode::CONFLICT, format!("{e:#}")))
}

/// After a game folder lands on the shelf: read it, find its program and pictures, keep its checksums.
fn after_place(cfg: &Config, db: &mut Db, dst: &Path, sums: &integrity::Manifest, ctx: &jobs::Ctx, what: &str) -> Result<Vec<Game>> {
    ctx.status("reading the shelf");
    library::refresh(cfg, db, false, |_| {})?;
    let mut games: Vec<Game> = db.games()?.into_iter().filter(|g| g.shelf_path(cfg) == dst).collect();
    library::find_programs(cfg, db, &mut games, &[], false, |_| {})?;
    ctx.status("artwork");
    library::refresh_art(cfg, db, &games, true, false, |s| ctx.status(s));
    for g in &games {
        let _ = integrity::write(&integrity::manifest_path(cfg, g), sums);
        db.log(&g.id, what);
    }
    Ok(games)
}

fn start_refresh(app: &App, full: bool) -> Result<u64> {
    if app.jobs.running("Reading the shelf") {
        bail!("the shelf is being read already");
    }
    let cfg = app.cfg.clone();
    app.jobs.spawn("Reading the shelf", None, move |ctx| {
        if !cfg.shelf_online() {
            bail!("the shelf folder {} is empty or missing: check the volume in compose.yaml", cfg.shelf);
        }
        let mut db = Db::open_default()?;
        let rep = library::refresh(&cfg, &mut db, full, |k| ctx.status(&format!("reading {k}")))?;
        let mut games = db.games()?;
        library::find_programs(&cfg, &db, &mut games, &[], full, |g| ctx.status(&format!("looking for the program in {}", g.name)))?;
        ctx.status("artwork");
        let art = library::refresh_art(&cfg, &db, &games, true, false, |s| ctx.status(s));
        let mut msg = format!("{} games on the shelf", rep.games);
        if !rep.read.is_empty() {
            msg.push_str(&format!(", {} folders read again", rep.read.len()));
        }
        if art.fetched > 0 {
            msg.push_str(&format!(", artwork found for {}", art.fetched));
        }
        if !rep.errors.is_empty() {
            msg.push_str(&format!(" ({} folders could not be read: {})", rep.errors.len(), rep.errors[0]));
        }
        Ok(msg)
    })
}

#[derive(Deserialize, Default)]
struct RefreshReq {
    #[serde(default)]
    full: bool,
}

async fn refresh(State(app): State<App>, body: Option<Json<RefreshReq>>) -> Res<Json<Started>> {
    started(start_refresh(&app, body.map(|b| b.0).unwrap_or_default().full))
}

#[derive(Deserialize)]
struct ArtworkReq {
    #[serde(default)]
    id: Option<String>,
    /// look again on SteamGridDB even where it found nothing before
    #[serde(default)]
    again: bool,
}

async fn artwork_job(State(app): State<App>, Json(r): Json<ArtworkReq>) -> Res<Json<Started>> {
    let cfg = app.cfg.clone();
    let game = r.id.clone();
    started(app.jobs.spawn("Finding artwork", game.as_deref(), move |ctx| {
        let db = Db::open_default()?;
        let games: Vec<Game> = match &r.id {
            Some(id) => vec![find(&cfg, id)?],
            None => db.games()?,
        };
        let rep = library::refresh_art(&cfg, &db, &games, true, r.again, |s| ctx.status(s));
        let mut msg = format!("artwork: {} copied, {} found on SteamGridDB, {} without", rep.copied, rep.fetched, rep.not_found);
        if let Some(e) = rep.errors.first() {
            msg.push_str(&format!(" ({e})"));
        }
        Ok(msg)
    }))
}

fn start_redownload(app: &App, g: Game, version: Option<String>, file: Option<PathBuf>) -> Result<u64> {
    let (cfg, updates) = (app.cfg.clone(), app.updates.clone());
    let title = format!("Downloading {}", g.full_name());
    let id = g.id.clone();
    app.jobs.spawn(&title, Some(&id), move |ctx| {
        let mut db = Db::open_default()?;
        let res = library::redownload(&cfg, &mut db, &g, version.as_deref(), file.as_deref(), &ctx.cancel, &mut |s| ctx.status(s), &mut |p| ctx.progress(p));
        if let Some(f) = &file {
            remove_upload(f);
        }
        let dst = res?;
        if version.is_none() {
            updates.clear(&g.id);
        }
        Ok(format!("on the shelf at {}", dst.display()))
    })
}

#[derive(Deserialize)]
struct RedownloadReq {
    id: String,
    /// Add it beside the current one as this version, instead of replacing it.
    #[serde(default)]
    version: Option<String>,
}

async fn redownload(State(app): State<App>, Json(r): Json<RedownloadReq>) -> Res<Json<Started>> {
    let cfg = app.cfg.clone();
    let id = r.id.clone();
    let g = blocking(move || find(&cfg, &id)).await?;
    started(start_redownload(&app, g, r.version.filter(|v| !v.trim().is_empty()), None))
}

#[derive(Deserialize)]
struct RedownloadFileQ {
    id: String,
    #[serde(default)]
    version: String,
    filename: String,
}

/// The fallback when a link needs a browser: the PC uploads the file it downloaded.
async fn redownload_file(State(app): State<App>, Query(q): Query<RedownloadFileQ>, body: Body) -> Res<Json<Started>> {
    let cfg = app.cfg.clone();
    let id = q.id.clone();
    let g = blocking(move || find(&cfg, &id)).await?;
    let f = save_upload(&app.data, &q.filename, body).await?;
    let version = Some(q.version).filter(|v| !v.trim().is_empty());
    let r = start_redownload(&app, g, version, Some(f.clone()));
    if r.is_err() {
        remove_upload(&f);
    }
    started(r)
}

async fn check_update(State(app): State<App>, Json(r): Json<IdQ>) -> Res<Json<updates::Update>> {
    blocking(move || {
        let g = find(&app.cfg, &r.id)?;
        app.updates.check(&g).context("no download link: add it to the game's info first").map(Json)
    })
    .await
}

fn start_check_all(app: &App, auto: bool) -> Result<u64> {
    if app.jobs.running("Checking for updates") {
        bail!("already checking for updates");
    }
    let a = app.clone();
    app.jobs.spawn("Checking for updates", None, move |ctx| {
        let games = Db::open_default()?.games()?;
        let (mut checked, mut new) = (0, Vec::new());
        for g in games.iter().filter(|g| g.kind != Kind::Steam) {
            if ctx.cancel.is_cancelled() {
                bail!("stopped");
            }
            ctx.status(&g.full_name());
            if let Some(u) = a.updates.check(g) {
                checked += 1;
                if u.state == updates::State::New {
                    new.push(g.clone());
                }
            }
        }
        let mut msg = format!("{checked} links checked, {} with a new file", new.len());
        if auto {
            for g in new {
                match start_redownload(&a, g.clone(), None, None) {
                    Ok(_) => msg.push_str(&format!("; downloading {}", g.full_name())),
                    Err(e) => msg.push_str(&format!("; {}: {e:#}", g.full_name())),
                }
            }
        }
        Ok(msg)
    })
}

async fn check_updates(State(app): State<App>) -> Res<Json<Started>> {
    started(start_check_all(&app, false))
}

async fn updates_list(State(app): State<App>) -> Json<std::collections::BTreeMap<String, updates::Update>> {
    Json(app.updates.all())
}

#[derive(Deserialize)]
struct ImportQ {
    kind: String,
    group: String,
    name: String,
    #[serde(default)]
    version: String,
    filename: String,
}

/// Add a game from an archive (or a single program) the PC uploads.
async fn import_upload(State(app): State<App>, Query(q): Query<ImportQ>, body: Body) -> Res<Json<Started>> {
    let kind = parse_kind(&q.kind)?;
    if q.group.trim().is_empty() || q.name.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "group and name are needed".into()));
    }
    let f = save_upload(&app.data, &q.filename, body).await?;
    let cfg = app.cfg.clone();
    let file = f.clone();
    let r = app.jobs.spawn(&format!("Adding {} to the shelf", q.name), None, move |ctx| {
        let res = (|| {
            let (dst, sums) = import::place(&cfg, &file, kind, &q.group, &q.name, &q.version, &ctx.cancel, &mut |s| ctx.status(s), &mut |p| ctx.progress(p))?;
            let mut db = Db::open_default()?;
            after_place(&cfg, &mut db, &dst, &sums, &ctx, "added to the shelf (uploaded)")?;
            Ok(format!("{} is on the shelf", q.name))
        })();
        remove_upload(&file);
        res
    });
    if r.is_err() {
        remove_upload(&f);
    }
    started(r)
}

#[derive(Deserialize)]
struct AddUrlReq {
    url: String,
    kind: String,
    group: String,
    name: String,
    #[serde(default)]
    version: String,
    /// The game's web page (SFGHQ showcase...), kept in its info.
    #[serde(default)]
    page: String,
}

/// Add a game straight from a download link: the server downloads, unpacks and files it.
async fn add_url(State(app): State<App>, Json(r): Json<AddUrlReq>) -> Res<Json<Started>> {
    let kind = parse_kind(&r.kind)?;
    if r.group.trim().is_empty() || r.name.trim().is_empty() || r.url.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "link, group and name are needed".into()));
    }
    let cfg = app.cfg.clone();
    started(app.jobs.spawn(&format!("Downloading {}", r.name), None, move |ctx| {
        let url = r.url.trim().to_string();
        ctx.status("downloading");
        let res = (|| {
            let file = source::fetch(&url, &ctx.cancel, &mut |p| ctx.progress(p))?;
            let (dst, sums) = import::place(&cfg, &file, kind, &r.group, &r.name, &r.version, &ctx.cancel, &mut |s| ctx.status(s), &mut |p| ctx.progress(p))?;
            let mut db = Db::open_default()?;
            let games = after_place(&cfg, &mut db, &dst, &sums, &ctx, "downloaded from its link")?;
            for g in &games {
                let mut info = artwork::info(g);
                info.download = url.clone();
                if !r.page.trim().is_empty() {
                    info.page = r.page.trim().to_string();
                }
                artwork::save_info(&cfg, g, &info)?;
                source::save_stamp(&cfg, g, &mut info);
            }
            Ok(format!("{} is on the shelf", r.name))
        })();
        source::downloads_dir_clean();
        res
    }))
}

async fn verify(State(app): State<App>, Json(r): Json<IdQ>) -> Res<Json<Started>> {
    let cfg = app.cfg.clone();
    let id = r.id.clone();
    let g = blocking(move || find(&cfg, &id)).await?;
    let cfg = app.cfg.clone();
    started(app.jobs.spawn(&format!("Verifying {}", g.full_name()), Some(&g.id.clone()), move |ctx| {
        let rep = integrity::verify_shelf(&cfg, &g, &ctx.cancel, &mut |s| ctx.status(s), &mut |p| ctx.progress(p))?;
        let msg = rep.summary();
        Db::open_default()?.log(&g.id, &format!("verified on the server: {msg}"));
        if !rep.ok() {
            bail!("{msg}");
        }
        Ok(msg)
    }))
}

async fn jobs_list(State(app): State<App>) -> Json<Vec<jobs::JobView>> {
    Json(app.jobs.list())
}

#[derive(Deserialize)]
struct JobQ {
    id: u64,
}

async fn job_get(State(app): State<App>, Query(q): Query<JobQ>) -> Res<Json<jobs::JobView>> {
    app.jobs.get(q.id).map(Json).ok_or(ApiError(StatusCode::NOT_FOUND, "no such job".into()))
}

async fn job_cancel(State(app): State<App>, Json(q): Json<JobQ>) -> Json<bool> {
    Json(app.jobs.cancel(q.id))
}

// ---------------------------------------------------------------- the server

fn router(app: App) -> Router {
    let private = Router::new()
        .route("/library", get(library_list))
        .route("/game", get(game_details))
        .route("/manifest", get(manifest))
        .route("/file", get(file))
        .route("/art", get(art_get).put(art_put).delete(art_delete))
        .route("/screenshot", post(screenshot_add).delete(screenshot_delete))
        .route("/info", put(info_put))
        .route("/set-version", post(set_version))
        .route("/page-images", get(page_images))
        .route("/screenshots-from-urls", post(screenshots_from_urls))
        .route("/refresh", post(refresh))
        .route("/artwork", post(artwork_job))
        .route("/redownload", post(redownload))
        .route("/redownload-file", post(redownload_file))
        .route("/check-update", post(check_update))
        .route("/check-updates", post(check_updates))
        .route("/updates", get(updates_list))
        .route("/import", post(import_upload))
        .route("/add-url", post(add_url))
        .route("/verify", post(verify))
        .route("/jobs", get(jobs_list))
        .route("/job", get(job_get))
        .route("/job/cancel", post(job_cancel))
        .route("/devices", get(devices))
        .layer(middleware::from_fn_with_state(app.clone(), require_token));
    let open = Router::new().route("/health", get(health)).route("/pair", post(pair));
    Router::new()
        .nest("/api", private.merge(open))
        .route("/", get(|| async { "Steam-Fan-Server. The API is under /api; pair a PC with: steam-fan-server pair\n" }))
        // pictures and info; game uploads stream and aren't limited
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .with_state(app)
}

/// Reads the shelf now and then, and asks download links for updates.
async fn scheduler(app: App, s: Settings) {
    let start = jobs::now();
    let (mut last_scan, mut last_check) = (0u64, start);
    loop {
        let now = jobs::now();
        let scan_due = last_scan == 0 || (s.scan_minutes > 0 && now - last_scan >= s.scan_minutes * 60);
        if scan_due {
            last_scan = now;
            let _ = start_refresh(&app, false);
        }
        // the first update check 5 minutes after start, when the shelf has been read
        let first = last_check == start && now - start >= 300;
        if s.update_hours > 0 && (first || now - last_check >= s.update_hours * 3600) {
            last_check = now;
            let _ = start_check_all(&app, s.auto_update);
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

fn main() -> Result<()> {
    let (cfg, settings, data) = setup();
    let auth = auth::Auth::new(&data.join("server"));
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("serve") => {}
        Some("pair") => {
            let code = auth.new_code()?;
            println!("Pairing code: {code}  (valid 10 minutes; type it into Steam-Fan on the PC)");
            return Ok(());
        }
        Some("devices") => {
            for d in auth.devices() {
                println!("{}  paired {}  last seen {}", d.name, d.paired, d.last_seen);
            }
            return Ok(());
        }
        Some("revoke") => {
            let name = args.get(1).context("usage: steam-fan-server revoke <name>")?;
            println!("{}", if auth.revoke(name)? { "unpaired" } else { "no device with that name" });
            return Ok(());
        }
        Some("--version") => {
            println!("steam-fan-server {VERSION}");
            return Ok(());
        }
        Some(other) => bail!("unknown command {other}: use serve, pair, devices or revoke"),
    }

    eprintln!("Steam-Fan-Server {VERSION}");
    eprintln!("  shelf: {} ({})", cfg.shelf, if cfg.shelf_online() { "found" } else { "EMPTY OR MISSING: check the volume" });
    eprintln!("  data:  {}", data.display());
    eprintln!(
        "  reading the shelf every {} min, update checks every {} h, auto update: {}",
        settings.scan_minutes,
        settings.update_hours,
        if settings.auto_update { "replace" } else { "off (report only)" }
    );
    if auth.devices().is_empty() && std::env::var("SF_TOKEN").map_or(true, |t| t.trim().is_empty()) {
        let code = auth.new_code()?;
        eprintln!("  no PC is paired yet. Pairing code: {code} (valid 10 minutes; later: steam-fan-server pair)");
    }
    let app = App {
        cfg: Arc::new(cfg),
        jobs: Arc::new(jobs::Jobs::default()),
        updates: Arc::new(updates::Updates::load(&data.join("server/updates.json"))),
        auth: Arc::new(auth),
        data: Arc::new(data),
    };
    let port = settings.port;
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(async move {
        tokio::spawn(scheduler(app.clone(), settings));
        let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await.with_context(|| format!("cannot listen on port {port}"))?;
        eprintln!("  listening on port {port}");
        axum::serve(listener, router(app))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn paths_stay_inside_the_game() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("game");
        fs::create_dir_all(root.join("data")).unwrap();
        fs::write(root.join("data/a.wav"), b"x").unwrap();
        fs::write(t.path().join("secret"), b"x").unwrap();
        assert!(resolve(&root, "data/a.wav").is_ok());
        assert!(resolve(&root, "../secret").is_err());
        assert!(resolve(&root, "/etc/passwd").is_err());
        assert!(resolve(&root, "").is_err());
        assert!(resolve(&root, "data").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(t.path().join("secret"), root.join("link")).unwrap();
            assert!(resolve(&root, "link").is_err(), "a link pointing outside is refused");
        }
        let single = t.path().join("SM63.exe");
        fs::write(&single, b"MZ").unwrap();
        assert_eq!(resolve(&single, "SM63.exe").unwrap(), single);
        assert!(resolve(&single, "other.exe").is_err());
    }

    #[test]
    fn file_list_skips_nas_files_and_keeps_checksums() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        fs::create_dir_all(root.join("@eaDir")).unwrap();
        fs::write(root.join("@eaDir/thumb"), b"x").unwrap();
        fs::write(root.join("game.exe"), b"MZ12").unwrap();
        fs::write(root.join("old.dat"), b"changed size").unwrap();
        let mut m = integrity::Manifest::new();
        m.insert("game.exe".into(), (4, "aa".into()));
        m.insert("old.dat".into(), (3, "bb".into()));
        let files = list_files(root, Some(&m)).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "game.exe");
        assert_eq!(files[0].sha256.as_deref(), Some("aa"));
        assert_eq!(files[1].sha256, None, "a stale checksum isn't passed on");
    }

    #[test]
    fn names_and_kinds() {
        assert_eq!(safe_name("../../etc/x.zip", "u"), "x.zip");
        assert_eq!(safe_name("C:\\Users\\a\\sqc.zip", "u"), "sqc.zip");
        assert_eq!(safe_name(".hidden", "u"), "u");
        assert_eq!(parse_kind("Fan").unwrap(), Kind::Fan);
        assert_eq!(parse_kind("non-steam").unwrap(), Kind::NonSteam);
        assert!(parse_kind("steam").is_err());
        assert_eq!(parse_art("Cover").unwrap(), Art::Cover);
    }
}
