//! Game artwork: covers, banners, logos and icons.
//!
//! Where a picture comes from, first match wins:
//! 1. `<shelf>/artwork/<game>/cover.png` etc.: set in the app (picked from
//!    SteamGridDB or uploaded by you). Kept on the shelf, so every PC gets it.
//! 2. A picture you put in the game's own folder: `cover.*`, `folder.*`, `boxart.*`,
//!    `hero.*`, `logo.*`, `icon.*` (and icons inside the program, or an `.ico`).
//!
//! A copy of each picture is kept on this PC (`~/.local/share/steam-fan/art/`),
//! so the app and Steam show it even when the NAS is off. The same pictures are
//! copied into Steam's own artwork folder, so Steam's library shows them too.

use crate::config::{secrets, Config};
use crate::shelf::{listing, Game, Kind};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Art {
    /// Portrait cover (600x900), the library grid.
    Cover,
    /// Wide banner behind the game page (1920x620).
    Hero,
    /// Transparent logo drawn over the banner.
    Logo,
    /// Small square icon.
    Icon,
    /// Landscape capsule (920x430), used by Steam's "recent games" row.
    Wide,
}

impl Art {
    pub fn all() -> [Art; 5] {
        [Art::Cover, Art::Hero, Art::Logo, Art::Icon, Art::Wide]
    }
    pub fn name(self) -> &'static str {
        match self {
            Art::Cover => "cover",
            Art::Hero => "hero",
            Art::Logo => "logo",
            Art::Icon => "icon",
            Art::Wide => "wide",
        }
    }
    pub fn parse(s: &str) -> Option<Art> {
        Art::all().into_iter().find(|a| a.name() == s)
    }
    /// Other file names people use for the same picture inside a game folder.
    fn folder_names(self) -> &'static [&'static str] {
        match self {
            Art::Cover => &["cover", "folder", "boxart", "box", "poster", "grid"],
            Art::Hero => &["hero", "background", "banner", "fanart"],
            Art::Logo => &["logo", "clearlogo"],
            Art::Icon => &["icon"],
            Art::Wide => &["wide", "header", "capsule"],
        }
    }
    /// File name in Steam's `config/grid` folder (without extension).
    fn steam_name(self, appid: u32) -> String {
        match self {
            Art::Cover => format!("{appid}p"),
            Art::Wide => format!("{appid}"),
            Art::Hero => format!("{appid}_hero"),
            Art::Logo => format!("{appid}_logo"),
            Art::Icon => format!("{appid}_icon"),
        }
    }
}

const EXTS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

fn is_image(name: &str) -> Option<&'static str> {
    let l = name.to_lowercase();
    EXTS.iter().copied().find(|e| l.ends_with(&format!(".{e}")))
}

// ---------------------------------------------------------------- places

fn art_base(cfg: &Config, g: &Game, rel: &str) -> PathBuf {
    let base = cfg.shelf().join("artwork");
    match g.kind {
        Kind::Rom => base.join("roms").join(&g.system).join(rel),
        Kind::Steam => base.join("steam").join(g.appid.to_string()),
        k => base.join(k.folder()).join(rel),
    }
}

/// `<shelf>/artwork/<kind folder>/<game>`: pictures and info, shared by all versions of a game.
pub fn shelf_dir(cfg: &Config, g: &Game) -> PathBuf {
    art_base(cfg, g, &g.title_rel())
}

/// `<shelf>/artwork/<kind folder>/<game>/<version>`: what belongs to one version (its checksums).
pub fn version_dir(cfg: &Config, g: &Game) -> PathBuf {
    art_base(cfg, g, &g.rel)
}

/// The copy on this PC.
pub fn cache_dir(g: &Game) -> PathBuf {
    crate::config::data_dir().join("art").join(format!("{:08x}", crc32fast::hash(g.title_id().as_bytes())))
}

/// Pictures and info that were set on one version (before versions were shown as one game)
/// become the whole game's, when it has none yet. Returns whether anything was taken.
pub fn adopt_version_art(cfg: &Config, title: &Game, versions: &[Game]) -> bool {
    let dst = shelf_dir(cfg, title);
    if !pictures_in(&dst).is_empty() {
        return false;
    }
    for v in versions {
        let src = version_dir(cfg, v);
        let pics = pictures_in(&src);
        if pics.is_empty() {
            continue;
        }
        if fs::create_dir_all(&dst).is_err() {
            return false;
        }
        for (_, p) in pics {
            let _ = fs::copy(&p, dst.join(p.file_name().unwrap()));
        }
        for extra in ["info.toml"] {
            if src.join(extra).is_file() && !dst.join(extra).exists() {
                let _ = fs::copy(src.join(extra), dst.join(extra));
            }
        }
        let shots = src.join("screenshots");
        if shots.is_dir() && !dst.join("screenshots").exists() {
            let _ = crate::transfer::copy_tree(&shots, &dst.join("screenshots"), &|_: &Path| false, &crate::transfer::Cancel::default(), &mut |_| {});
        }
        return true;
    }
    false
}

fn pictures_in(dir: &Path) -> Vec<(String, PathBuf)> {
    listing(dir)
        .map(|l| {
            l.into_iter()
                .filter(|e| e.is_file)
                .filter_map(|e| {
                    let ext = is_image(&e.name)?;
                    let stem = e.name[..e.name.len() - ext.len() - 1].to_lowercase();
                    Some((stem, dir.join(&e.name)))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn pick(pics: &[(String, PathBuf)], stems: &[&str]) -> Option<PathBuf> {
    stems.iter().find_map(|s| pics.iter().find(|(stem, _)| stem == s).map(|(_, p)| p.clone()))
}

/// The picture on this PC, if there is one (fast: no network).
pub fn local(g: &Game, art: Art) -> Option<PathBuf> {
    pick(&pictures_in(&cache_dir(g)), &[art.name()])
}

/// Every picture on the shelf for a game: what was set in the app first, then
/// pictures in the game's own folder. A few folder listings, no deep walk.
pub fn shelf_pictures(cfg: &Config, g: &Game) -> Vec<(Art, PathBuf)> {
    let set = pictures_in(&shelf_dir(cfg, g));
    let (inside, extra, icons) = if g.kind == Kind::Steam {
        (vec![], vec![], vec![])
    } else {
        let src = g.shelf_path(cfg);
        let dir = if src.is_file() { src.parent().map(Path::to_path_buf).unwrap_or(src) } else { src };
        let items = listing(&dir).unwrap_or_default();
        let has = |n: &str| items.iter().any(|e| e.is_dir && e.name.eq_ignore_ascii_case(n));
        (
            pictures_in(&dir),
            if has(".steam-fan") { pictures_in(&dir.join(".steam-fan")) } else { vec![] },
            if has("Icons") { pictures_in(&dir.join("Icons")) } else { vec![] },
        )
    };
    Art::all()
        .into_iter()
        .filter_map(|art| {
            let p = pick(&set, &[art.name()])
                .or_else(|| pick(&inside, art.folder_names()))
                .or_else(|| pick(&extra, &[art.name()]))
                .or_else(|| if art == Art::Icon { pick(&icons, &["icon"]) } else { None })?;
            Some((art, p))
        })
        .collect()
}

pub fn on_shelf(cfg: &Config, g: &Game, art: Art) -> Option<PathBuf> {
    shelf_pictures(cfg, g).into_iter().find(|(a, _)| *a == art).map(|(_, p)| p)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.len() == y.len() && x.modified().ok() <= y.modified().ok(),
        _ => false,
    }
}

fn clear_cached(g: &Game, art: Art) {
    for e in EXTS {
        let _ = fs::remove_file(cache_dir(g).join(format!("{}.{e}", art.name())));
    }
}

/// Bring the shelf's pictures for one game to this PC. Returns true if anything changed.
pub fn mirror(cfg: &Config, g: &Game) -> bool {
    if cfg.server_mode() {
        return crate::remote::fetch_art(cfg, g).unwrap_or(false);
    }
    let mut changed = false;
    let found = shelf_pictures(cfg, g);
    let _ = fs::create_dir_all(cache_dir(g)); // also marks the game as looked at
    for art in Art::all() {
        match found.iter().find(|(a, _)| *a == art) {
            Some((_, src)) => {
                let ext = is_image(&src.to_string_lossy()).unwrap_or("png");
                let dst = cache_dir(g).join(format!("{}.{ext}", art.name()));
                if !same_file(src, &dst) {
                    clear_cached(g, art);
                    if fs::copy(src, &dst).is_ok() {
                        changed = true;
                    }
                }
            }
            None if art == Art::Icon && local(g, Art::Icon).is_none() => {
                // nothing chosen: the icon inside the program, or an .ico in the game folder
                if let Some(p) = crate::icons::icon_for(cfg, g) {
                    if fs::copy(&p, cache_dir(g).join("icon.png")).is_ok() {
                        changed = true;
                    }
                }
            }
            None => {}
        }
    }
    if mirror_extras(cfg, g) {
        changed = true;
    }
    changed
}

/// Has this game's artwork been looked for on this PC before?
pub fn mirrored(g: &Game) -> bool {
    cache_dir(g).is_dir()
}

/// Save a picture for a game: on the shelf (so every PC gets it) and on this PC.
/// Pictures that Steam can't read (webp, ico, ...) are turned into PNG.
pub fn store(cfg: &Config, g: &Game, art: Art, bytes: &[u8]) -> Result<PathBuf> {
    let fmt = image::guess_format(bytes).context("that file isn't a picture")?;
    let (data, ext): (Vec<u8>, &str) = match fmt {
        image::ImageFormat::Png => (bytes.to_vec(), "png"),
        image::ImageFormat::Jpeg => (bytes.to_vec(), "jpg"),
        _ => {
            let img = image::load_from_memory(bytes).context("cannot read that picture")?;
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, image::ImageFormat::Png)?;
            (out.into_inner(), "png")
        }
    };
    if cfg.server_mode() {
        crate::remote::require(cfg)?.put_bytes(&format!("/art?id={}&kind={}", crate::remote::q(&g.title_id()), art.name()), &data)?;
    } else if cfg.shelf_online() {
        let dir = shelf_dir(cfg, g);
        fs::create_dir_all(&dir).with_context(|| format!("cannot write to {}", dir.display()))?;
        for e in EXTS {
            let _ = fs::remove_file(dir.join(format!("{}.{e}", art.name())));
        }
        fs::write(dir.join(format!("{}.{ext}", art.name())), &data)?;
    } else {
        bail!("the shelf isn't reachable, so the picture can't be saved there");
    }
    clear_cached(g, art);
    fs::create_dir_all(cache_dir(g))?;
    let p = cache_dir(g).join(format!("{}.{ext}", art.name()));
    fs::write(&p, &data)?;
    if cfg.server_mode() {
        crate::remote::update_entry(cfg, g)?;
    }
    Ok(p)
}

/// Remove a picture set in the app (pictures inside the game folder stay).
pub fn remove(cfg: &Config, g: &Game, art: Art) -> Result<()> {
    if cfg.server_mode() {
        crate::remote::require(cfg)?.delete(&format!("/art?id={}&kind={}", crate::remote::q(&g.title_id()), art.name()))?;
        clear_cached(g, art);
        crate::remote::update_entry(cfg, g)?;
        return Ok(());
    }
    let dir = shelf_dir(cfg, g);
    for e in EXTS {
        let _ = fs::remove_file(dir.join(format!("{}.{e}", art.name())));
    }
    clear_cached(g, art);
    mirror(cfg, g);
    Ok(())
}

pub fn store_file(cfg: &Config, g: &Game, art: Art, file: &Path) -> Result<PathBuf> {
    let bytes = fs::read(file).with_context(|| format!("cannot read {}", file.display()))?;
    store(cfg, g, art, &bytes)
}

// ---------------------------------------------------------------- Steam's artwork folder

/// Copy a game's pictures into Steam's `config/grid` folder under the given library id.
/// Steam reads them when it starts.
pub fn apply_to_steam(cfg: &Config, g: &Game, appid: u32) -> Result<usize> {
    let grid = crate::steam::user_config_dir(&cfg.steam_root())?.join("grid");
    fs::create_dir_all(&grid)?;
    let mut n = 0;
    for art in Art::all() {
        let Some(src) = local(g, art) else { continue };
        // a real Steam game keeps Steam's own art unless you picked something on purpose
        if g.kind == Kind::Steam && on_shelf(cfg, g, art).is_none() {
            continue;
        }
        let ext = match is_image(&src.to_string_lossy()) {
            Some("jpeg") => "jpg",
            Some(e) => e,
            None => continue,
        };
        let name = art.steam_name(appid);
        let dst = grid.join(format!("{name}.{ext}"));
        if same_file(&src, &dst) {
            continue;
        }
        for e in EXTS {
            let _ = fs::remove_file(grid.join(format!("{name}.{e}")));
        }
        fs::copy(&src, &dst)?;
        n += 1;
    }
    Ok(n)
}

// ---------------------------------------------------------------- SteamGridDB

fn none<T>() -> Option<T> {
    None
}

const SGDB: &str = "https://www.steamgriddb.com/api/v2";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SgdbGame {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub release_date: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SgdbImage {
    pub id: u64,
    pub url: String,
    pub thumb: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub style: String,
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct Resp<T> {
    success: bool,
    #[serde(default = "none")]
    data: Option<T>,
    #[serde(default)]
    errors: Vec<String>,
}

pub fn sgdb_key() -> String {
    secrets::get(secrets::STEAMGRIDDB)
}

fn sgdb_get<T: for<'de> Deserialize<'de>>(path: &str, query: &[(&str, &str)]) -> Result<T> {
    let key = sgdb_key();
    if key.is_empty() {
        bail!("no SteamGridDB API key: add it in Settings (it's kept in {})", secrets::path().display());
    }
    let mut req = ureq::get(&format!("{SGDB}{path}"))
        .set("Authorization", &format!("Bearer {key}"))
        .timeout(std::time::Duration::from_secs(20));
    for (k, v) in query {
        req = req.query(k, v);
    }
    let r: Resp<T> = match req.call() {
        Ok(r) => r.into_json()?,
        Err(ureq::Error::Status(401, _)) => bail!("SteamGridDB refused the API key"),
        Err(ureq::Error::Status(404, _)) => bail!("not found on SteamGridDB"),
        Err(_) => bail!("cannot reach SteamGridDB (is the internet connection up?)"),
    };
    if !r.success {
        bail!("SteamGridDB: {}", r.errors.join(", "));
    }
    r.data.context("SteamGridDB sent nothing")
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn sgdb_search(term: &str) -> Result<Vec<SgdbGame>> {
    sgdb_get(&format!("/search/autocomplete/{}", encode(term.trim())), &[])
}

pub fn sgdb_for_steam(appid: u32) -> Result<SgdbGame> {
    sgdb_get(&format!("/games/steam/{appid}"), &[])
}

pub fn sgdb_images(game_id: u64, art: Art) -> Result<Vec<SgdbImage>> {
    let id = game_id.to_string();
    match art {
        Art::Cover => sgdb_get(&format!("/grids/game/{id}"), &[("dimensions", "600x900")]),
        Art::Wide => sgdb_get(&format!("/grids/game/{id}"), &[("dimensions", "920x430,460x215")]),
        Art::Hero => sgdb_get(&format!("/heroes/game/{id}"), &[]),
        Art::Logo => sgdb_get(&format!("/logos/game/{id}"), &[]),
        Art::Icon => sgdb_get(&format!("/icons/game/{id}"), &[]),
    }
}

/// Download a picture (at most 40 MB).
pub fn download(url: &str) -> Result<Vec<u8>> {
    let get = || ureq::get(url).set("User-Agent", "Mozilla/5.0 (X11; Linux x86_64) Steam-Fan/0.1").timeout(std::time::Duration::from_secs(60)).call();
    let r = match get() {
        // a busy or briefly broken server (Cloudflare 52x): one more try
        Err(ureq::Error::Status(code, _)) if code >= 500 => {
            std::thread::sleep(std::time::Duration::from_secs(2));
            get()
        }
        r => r,
    };
    let r = r.map_err(|e| match e {
        ureq::Error::Status(code, _) => anyhow::anyhow!("{} ({code}{})", url, status_hint(code)),
        e => crate::source::net_error(url, e),
    })?;
    let mut buf = Vec::new();
    r.into_reader().take(40 << 20).read_to_end(&mut buf)?;
    Ok(buf)
}

fn status_hint(code: u16) -> &'static str {
    match code {
        403 => ": the site refuses it",
        404 => ": not there any more",
        520..=527 => ": the site's own server is broken behind Cloudflare; try later",
        500..=599 => ": the site has a problem; try later",
        _ => "",
    }
}

/// A name that finds the game on SteamGridDB: without disc ids like `[VBKP]` and version tags.
pub fn search_name(g: &Game) -> String {
    let mut n = g.name.clone();
    while let (Some(a), Some(b)) = (n.find(['[', '(']), n.rfind([']', ')'])) {
        if b <= a {
            break;
        }
        n.replace_range(a..=b, " ");
    }
    let n = n.replace('_', " ");
    // everything from a version tag on (v1, v2.2.13, 1.4) is left out, and platform words
    let is_version = |l: &str| {
        let d = l.strip_prefix('v').unwrap_or(l);
        d.chars().next().map_or(false, |c| c.is_ascii_digit()) && d.chars().all(|c| c.is_ascii_digit() || c == '.')
            && (l.starts_with('v') || d.contains('.'))
    };
    let words: Vec<&str> = n
        .split_whitespace()
        .take_while(|w| !is_version(&w.to_lowercase()))
        .filter(|w| !matches!(w.to_lowercase().as_str(), "demo" | "windows" | "win" | "win64" | "win32" | "linux" | "x64"))
        .collect();
    words.join(" ")
}

/// Pick the SteamGridDB entry for a game: the exact name if there is one, else the first result.
pub fn find_sgdb(g: &Game) -> Result<Option<SgdbGame>> {
    if g.kind == Kind::Steam {
        return Ok(sgdb_for_steam(g.appid).ok());
    }
    let term = search_name(g);
    if term.is_empty() {
        return Ok(None);
    }
    let hits = sgdb_search(&term)?;
    // Only take a result whose name really is this game. Fan games are often missing on
    // SteamGridDB, and the closest name is then a different game ("Adventure Pack Season 2
    // Demo" found a Taiko no Tatsujin pack). Better no picture than a wrong one; the user
    // can still pick any result by hand in the artwork picker.
    Ok(hits.into_iter().find(|h| same_title(&h.name, &term)))
}

fn squash(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// "Sonic 3 A.I.R." ~ "Sonic 3: Angel Island Revisited"? No. "Sonic Robo Blast 2" ~ "Sonic Robo Blast 2"? Yes.
pub fn same_title(a: &str, b: &str) -> bool {
    let (a, b) = (squash(a), squash(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    // one inside the other, and nearly the same length ("Super Mario 63" / "Super Mario 63 Remake" is not)
    let (short, long) = if a.len() <= b.len() { (&a, &b) } else { (&b, &a) };
    long.contains(short.as_str()) && short.len() * 10 >= long.len() * 9
}

/// Fill in the pictures a game is missing from SteamGridDB. Returns what was added.
pub fn auto_fill(cfg: &Config, g: &Game, sgdb_id: Option<u64>) -> Result<(Option<u64>, Vec<Art>)> {
    let have: Vec<Art> = shelf_pictures(cfg, g).into_iter().map(|(a, _)| a).collect();
    let missing: Vec<Art> = Art::all().into_iter().filter(|a| !have.contains(a)).collect();
    // Steam games already have Steam's art
    if g.kind == Kind::Steam || missing.is_empty() {
        return Ok((sgdb_id, vec![]));
    }
    let id = match sgdb_id {
        Some(i) => i,
        None => match find_sgdb(g)? {
            Some(s) => s.id,
            None => return Ok((None, vec![])),
        },
    };
    let mut added = vec![];
    for art in missing {
        // the program's own icon is good enough when there is one
        if art == Art::Icon && crate::icons::icon_for(cfg, g).is_some() {
            continue;
        }
        if let Ok(list) = sgdb_images(id, art) {
            if let Some(img) = list.first() {
                if let Ok(bytes) = download(&img.url) {
                    if store(cfg, g, art, &bytes).is_ok() {
                        added.push(art);
                    }
                }
            }
        }
    }
    Ok((Some(id), added))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shelf::Platform;

    fn game(name: &str) -> Game {
        Game {
            id: format!("fan:G/{name}"),
            kind: Kind::Fan,
            rel: format!("G/{name}"),
            system: String::new(),
            name: name.into(),
            version: String::new(),
            group: "G".into(),
            program: String::new(),
            platform: Platform::Windows,
            appid: 0,
            buildid: String::new(),
            installdir: String::new(),
            size: 0,
            built: 0,
        }
    }

    #[test]
    fn names_for_search() {
        assert_eq!(search_name(&game("Super Mario Galaxy [VBKP]")), "Super Mario Galaxy");
        assert_eq!(search_name(&game("RogAndRoll_Win_SAGE25_v1_3d")), "RogAndRoll SAGE25");
        assert_eq!(search_name(&game("Sonic 3 A.I.R.")), "Sonic 3 A.I.R.");
        assert_eq!(search_name(&game("Sonic Robo Blast 2 v2.2.13")), "Sonic Robo Blast 2");
        assert_eq!(encode("Kirby's Epic Yarn"), "Kirby%27s%20Epic%20Yarn");
        assert!(same_title("Sonic Robo Blast 2", "sonic robo-blast 2"));
        assert!(!same_title("Taiko no Tatsujin: Rhythmic Adventure Pack", "Adventure Pack Season 2 Demo"));
        assert!(!same_title("Super Mario 63 Remake", "Super Mario 63"));
    }

    #[test]
    fn store_and_find() {
        let t = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", t.path());
        std::env::set_var("XDG_DATA_HOME", t.path().join("data"));
        let shelf = t.path().join("shelf");
        fs::create_dir_all(shelf.join("fangames/G/Game")).unwrap();
        fs::write(shelf.join("fangames/G/Game/game.exe"), b"x").unwrap();
        let cfg = Config { shelf: shelf.to_string_lossy().into(), ..Default::default() };
        let g = game("Game");
        // a picture dropped into the game folder by hand is found
        let img = image::RgbaImage::from_pixel(4, 6, image::Rgba([10, 20, 30, 255]));
        img.save(shelf.join("fangames/G/Game/folder.png")).unwrap();
        assert!(on_shelf(&cfg, &g, Art::Cover).unwrap().ends_with("folder.png"));
        assert!(mirror(&cfg, &g));
        assert!(local(&g, Art::Cover).is_some());
        // one set in the app wins, and webp/bmp become png
        let mut bmp = std::io::Cursor::new(Vec::new());
        img.write_to(&mut bmp, image::ImageFormat::Bmp).unwrap();
        store(&cfg, &g, Art::Cover, &bmp.into_inner()).unwrap();
        assert!(on_shelf(&cfg, &g, Art::Cover).unwrap().ends_with("artwork/fangames/G/Game/cover.png"));
    }
}


// ---------------------------------------------------------------- thumbnails

/// A small copy of the cover for the library grid (about 300 px wide), made on first use.
/// Full covers are often 1 MB each; hundreds of them make scrolling slow.
pub fn thumb(g: &Game) -> Option<PathBuf> {
    let cover = local(g, Art::Cover)?;
    let out = cache_dir(g).join("cover-thumb.jpg");
    let fresh = match (fs::metadata(&cover), fs::metadata(&out)) {
        (Ok(c), Ok(t)) => t.modified().ok() >= c.modified().ok(),
        _ => false,
    };
    if !fresh {
        let img = image::open(&cover).ok()?;
        let small = img.resize(320, 480, image::imageops::FilterType::Triangle).to_rgb8();
        let mut f = fs::File::create(&out).ok()?;
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut f, 85).encode_image(&small).ok()?;
    }
    Some(out)
}

// ---------------------------------------------------------------- about the game

/// What a game is about, kept on the shelf in `artwork/<game>/info.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Info {
    pub description: String,
    pub developer: String,
    pub released: String,
    /// Web pages: SFGHQ, the game's site, a video...
    pub links: Vec<String>,
    /// The game's page (SFGHQ showcase, itch.io page...).
    pub page: String,
    /// Where the game's file can be downloaded (MEGA, Google Drive, a direct link).
    pub download: String,
    /// What the download link served last time (size, date), to notice a newer upload.
    pub download_stamp: String,
    /// The version Play uses, when the game is kept in several (its folder name).
    /// Empty: the newest.
    #[serde(default)]
    pub default_version: String,
    /// The version marked by hand as the newest (its folder name). Empty: worked out
    /// from the version names and program dates.
    #[serde(default)]
    pub newest_version: String,
}

/// The info on this PC (copied from the shelf), or empty.
pub fn info(g: &Game) -> Info {
    fs::read_to_string(cache_dir(g).join("info.toml")).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
}

pub fn save_info(cfg: &Config, g: &Game, info: &Info) -> Result<()> {
    let text = toml::to_string_pretty(info)?;
    if cfg.server_mode() {
        crate::remote::require(cfg)?.send_json::<serde_json::Value>("PUT", &format!("/info?id={}", crate::remote::q(&g.title_id())), serde_json::to_value(info)?)?;
        fs::create_dir_all(cache_dir(g))?;
        fs::write(cache_dir(g).join("info.toml"), &text)?;
        return Ok(());
    }
    if !cfg.shelf_online() {
        bail!("the shelf isn't reachable");
    }
    let dir = shelf_dir(cfg, g);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("info.toml"), &text)?;
    fs::create_dir_all(cache_dir(g))?;
    fs::write(cache_dir(g).join("info.toml"), &text)?;
    Ok(())
}

/// Screenshots on this PC (copied from the shelf), sorted by name.
pub fn screenshots(g: &Game) -> Vec<PathBuf> {
    // with a server: the ones it has (fetched when shown)
    if cache_dir(g).join("server-screenshots.json").is_file() {
        return crate::remote::screenshot_names(g).into_iter().map(|n| cache_dir(g).join("screenshots").join(n)).collect();
    }
    let mut v: Vec<PathBuf> = pictures_in(&cache_dir(g).join("screenshots")).into_iter().map(|(_, p)| p).collect();
    v.sort();
    v
}

/// Add pictures as screenshots: on the shelf and on this PC. Returns how many were added.
pub fn add_screenshots(cfg: &Config, g: &Game, files: &[PathBuf]) -> Result<usize> {
    if cfg.server_mode() {
        let s = crate::remote::require(cfg)?;
        for f in files {
            let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "screenshot.png".into());
            let bytes = fs::read(f).with_context(|| format!("cannot read {}", f.display()))?;
            let url = format!("/screenshot?id={}&name={}", crate::remote::q(&g.title_id()), crate::remote::q(&name));
            s.post_bytes(&url, &bytes)?;
        }
        crate::remote::update_entry(cfg, g)?;
        return Ok(files.len());
    }
    if !cfg.shelf_online() {
        bail!("the shelf isn't reachable");
    }
    let dir = shelf_dir(cfg, g).join("screenshots");
    let cache = cache_dir(g).join("screenshots");
    fs::create_dir_all(&dir)?;
    fs::create_dir_all(&cache)?;
    let mut n = 0;
    for f in files {
        let bytes = fs::read(f).with_context(|| format!("cannot read {}", f.display()))?;
        let fmt = image::guess_format(&bytes).with_context(|| format!("{} isn't a picture", f.display()))?;
        let (data, ext) = match fmt {
            image::ImageFormat::Png => (bytes, "png"),
            image::ImageFormat::Jpeg => (bytes, "jpg"),
            _ => {
                let mut out = std::io::Cursor::new(Vec::new());
                image::load_from_memory(&bytes)?.write_to(&mut out, image::ImageFormat::Png)?;
                (out.into_inner(), "png")
            }
        };
        // numbered names keep the order they were added in
        let next = pictures_in(&dir).len() + 1;
        let mut name = format!("{next:03}.{ext}");
        let mut k = next;
        while dir.join(&name).exists() {
            k += 1;
            name = format!("{k:03}.{ext}");
        }
        fs::write(dir.join(&name), &data)?;
        fs::write(cache.join(&name), &data)?;
        n += 1;
    }
    Ok(n)
}

pub fn remove_screenshot(cfg: &Config, g: &Game, name: &str) -> Result<()> {
    if name.contains('/') || name.contains("..") {
        bail!("bad name");
    }
    if cfg.server_mode() {
        crate::remote::require(cfg)?.delete(&format!("/screenshot?id={}&name={}", crate::remote::q(&g.title_id()), crate::remote::q(name)))?;
        let _ = fs::remove_file(cache_dir(g).join("screenshots").join(name));
        crate::remote::update_entry(cfg, g)?;
        return Ok(());
    }
    let _ = fs::remove_file(shelf_dir(cfg, g).join("screenshots").join(name));
    let _ = fs::remove_file(cache_dir(g).join("screenshots").join(name));
    Ok(())
}

/// Copy screenshots and info from the shelf. True if anything changed.
fn mirror_extras(cfg: &Config, g: &Game) -> bool {
    let mut changed = false;
    let sdir = shelf_dir(cfg, g);
    let info_src = sdir.join("info.toml");
    let info_dst = cache_dir(g).join("info.toml");
    if info_src.is_file() && !same_file(&info_src, &info_dst) && fs::copy(&info_src, &info_dst).is_ok() {
        changed = true;
    }
    let shots = pictures_in(&sdir.join("screenshots"));
    let cache = cache_dir(g).join("screenshots");
    let have: Vec<String> = pictures_in(&cache).into_iter().map(|(_, p)| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    if !shots.is_empty() {
        let _ = fs::create_dir_all(&cache);
    }
    let names: Vec<String> = shots.iter().map(|(_, p)| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    for (_, p) in &shots {
        let dst = cache.join(p.file_name().unwrap());
        if !same_file(p, &dst) && fs::copy(p, &dst).is_ok() {
            changed = true;
        }
    }
    // removed on the shelf (from another PC): remove here too
    let listed = listing(&sdir.join("screenshots")).is_ok();
    for h in have {
        if listed && !names.contains(&h) {
            let _ = fs::remove_file(cache.join(&h));
            changed = true;
        }
    }
    changed
}

// ---------------------------------------------------------------- pictures on a web page

/// Every picture on a web page (a game's SFGHQ showcase, its itch.io page...): the page's
/// own preview picture first, then the images in it and links to images. Avatars and icons are left out.
pub fn page_images(url: &str) -> Result<Vec<String>> {
    // Game Jolt pages are built in the browser: ask its API instead
    if let Some(id) = crate::gamejolt::game_id(url) {
        return crate::gamejolt::images(id);
    }
    let html = ureq::get(url)
        .set("User-Agent", "Mozilla/5.0 (X11; Linux x86_64) Steam-Fan/0.1")
        .timeout(std::time::Duration::from_secs(20))
        .call()
        .map_err(|e| crate::source::net_error(url, e))?
        .into_string()?;
    Ok(images_in_html(url, &html))
}

fn absolute(base: &str, link: &str) -> Option<String> {
    let link = link.trim().replace("&amp;", "&");
    if link.starts_with("data:") || link.is_empty() {
        return None;
    }
    if link.starts_with("http://") || link.starts_with("https://") {
        return Some(link);
    }
    let scheme = if base.starts_with("http://") { "http:" } else { "https:" };
    if let Some(rest) = link.strip_prefix("//") {
        return Some(format!("{scheme}//{rest}"));
    }
    let after = base.split("//").nth(1)?;
    let host = after.split('/').next()?;
    if link.starts_with('/') {
        return Some(format!("{scheme}//{host}{link}"));
    }
    let dir = base.split(['?', '#']).next()?.rsplit_once('/').map(|(d, _)| d)?;
    Some(format!("{dir}/{link}"))
}

pub fn images_in_html(page: &str, html: &str) -> Vec<String> {
    use regex::Regex;
    let meta = Regex::new(r#"(?i)<meta[^>]+(?:property|name)=["'](?:og:image|twitter:image)["'][^>]*content=["']([^"']+)["']|<meta[^>]+content=["']([^"']+)["'][^>]*(?:property|name)=["'](?:og:image|twitter:image)["']"#).unwrap();
    let img = Regex::new(r#"(?i)<img[^>]+?(?:data-src|data-url|src)=["']([^"']+)["']"#).unwrap();
    let href = Regex::new(r#"(?i)<a[^>]+href=["']([^"']+?(?:\.(?:png|jpe?g|webp|gif)(?:\?[^"']*)?|/attachments/[^"']+))["']"#).unwrap();
    let mut out: Vec<String> = vec![];
    let mut push = |u: Option<String>| {
        if let Some(u) = u {
            let low = u.to_lowercase();
            let junk = ["avatar", "smilie", "emoji", "favicon", "/styles/", "gravatar", "spacer", "pixel", "logo.svg", "badge", ".svg"]
                .iter()
                .any(|j| low.contains(j));
            if !junk && !out.contains(&u) {
                out.push(u);
            }
        }
    };
    for c in meta.captures_iter(html) {
        push(c.get(1).or(c.get(2)).and_then(|m| absolute(page, m.as_str())));
    }
    for c in href.captures_iter(html) {
        push(absolute(page, &c[1]));
    }
    for c in img.captures_iter(html) {
        push(absolute(page, &c[1]));
    }
    // a forum attachment and its small preview are the same picture
    let full: Vec<String> = out.clone();
    out.retain(|u| {
        let t = u.trim_end_matches('/');
        !(t.ends_with("/thumb") && full.iter().any(|f| f.trim_end_matches('/') == t.trim_end_matches("/thumb")))
    });
    out.truncate(80);
    out
}

/// Pictures added, and the ones that couldn't be downloaded (with why).
#[derive(Debug, Default, serde::Serialize)]
pub struct Added {
    pub added: usize,
    pub failed: Vec<String>,
}

/// Download pictures and add them as screenshots (onto the shelf). One that fails doesn't stop the others.
pub fn add_screenshots_from_urls(cfg: &Config, g: &Game, urls: &[String]) -> Result<Added> {
    if cfg.server_mode() {
        return crate::remote::screenshots_from_urls(cfg, g, urls);
    }
    let dir = dirs::cache_dir().unwrap_or_else(|| crate::config::home().join(".cache")).join("steam-fan/shots");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    let mut files = vec![];
    let mut out = Added::default();
    let mut seen = std::collections::HashSet::new();
    for (i, u) in urls.iter().enumerate() {
        match download(u) {
            Ok(bytes) => {
                // the same picture under two addresses
                if !seen.insert(<sha2::Sha256 as sha2::Digest>::digest(&bytes).to_vec()) {
                    continue;
                }
                let p = dir.join(format!("{i:03}"));
                fs::write(&p, bytes)?;
                files.push(p);
            }
            Err(e) => out.failed.push(format!("{e:#}")),
        }
    }
    let n = if files.is_empty() { Ok(0) } else { add_screenshots(cfg, g, &files) };
    let _ = fs::remove_dir_all(&dir);
    out.added = n?;
    if out.added == 0 && !out.failed.is_empty() {
        bail!("none of the pictures could be downloaded: {}", out.failed.join("; "));
    }
    Ok(out)
}

#[cfg(test)]
mod page_tests {
    use super::*;
    #[test]
    fn images_from_a_showcase_page() {
        let html = r#"<html><head><meta property="og:image" content="https://sonicfangameshq.com/data/showcase/742.jpg?1700"></head>
        <body><img src="/data/avatars/s/1.jpg"><a href="/forums/attachments/shot1-png.1234/"><img src="/forums/attachments/shot1-png.1234/thumb"></a>
        <img data-src="//cdn.example.com/a.webp"><img src="data:image/gif;base64,xx"><img src="images/rel.png"></body></html>"#;
        let got = images_in_html("https://sonicfangameshq.com/forums/showcase/advpack.742/", html);
        assert_eq!(
            got,
            [
                "https://sonicfangameshq.com/data/showcase/742.jpg?1700",
                "https://sonicfangameshq.com/forums/attachments/shot1-png.1234/",
                "https://cdn.example.com/a.webp",
                "https://sonicfangameshq.com/forums/showcase/advpack.742/images/rel.png",
            ]
        );
    }
}
