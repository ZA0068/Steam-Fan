//! What one version of a game keeps for itself: its own pictures and its own info
//! (description, made by, released, links, download link).
//!
//! A version is a member of its game; the game is the group. Whatever a version leaves empty
//! comes from the newest version, then from the game itself. The download link is the
//! exception: an older version without its own link has none (the game's link brings the newest).
//!
//! On the shelf: `artwork/<kind>/<game>/<version folder>/` (beside the version's checksums).
//! On this PC: `<the game's picture cache>/versions/<crc of the folder name>/`.

use crate::artwork::{self, Art, Info, EXTS};
use crate::config::Config;
use crate::library::Title;
use crate::shelf::Game;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;

const INFO: &str = "info.toml";
/// With a server: which pictures it has for this version, with their dates.
const REMOTE_ART: &str = "server-art.json";

/// The version's own folder on the shelf.
pub fn dir(cfg: &Config, v: &Game) -> PathBuf {
    artwork::version_dir(cfg, v)
}

/// The version's own folder on this PC.
pub fn cache(v: &Game) -> PathBuf {
    cache_for(&v.title_id(), &v.version)
}

/// The same, from the game's id and the version's folder name (all a picture address carries).
pub fn cache_for(title_id: &str, folder: &str) -> PathBuf {
    crate::config::data_dir()
        .join("art")
        .join(format!("{:08x}", crc32fast::hash(title_id.as_bytes())))
        .join("versions")
        .join(format!("{:08x}", crc32fast::hash(folder.as_bytes())))
}

/// Only what a version can have for itself.
fn content(i: &Info) -> Info {
    Info {
        description: i.description.trim().to_string(),
        developer: i.developer.trim().to_string(),
        released: i.released.trim().to_string(),
        links: i.links.iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
        page: i.page.trim().to_string(),
        download: i.download.trim().to_string(),
        ..Default::default()
    }
}

/// The version's own info (empty fields: it has none of its own).
pub fn info(v: &Game) -> Info {
    fs::read_to_string(cache(v).join(INFO)).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
}

fn write_info(dir: &std::path::Path, i: &Info) -> Result<()> {
    let p = dir.join(INFO);
    if *i == Info::default() {
        let _ = fs::remove_file(p);
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    fs::write(p, toml::to_string_pretty(i)?)?;
    Ok(())
}

/// Save the version's own info. Empty fields go back to being inherited.
pub fn save_info(cfg: &Config, v: &Game, i: &Info) -> Result<()> {
    if v.version.is_empty() {
        bail!("{} isn't kept in versions", v.name);
    }
    let i = content(i);
    if cfg.server_mode() {
        crate::remote::require(cfg)?.send_json::<serde_json::Value>("PUT", &format!("/info?id={}&own=1", crate::remote::q(&v.id)), serde_json::to_value(&i)?)?;
        return crate::remote::update_own(cfg, v);
    } else {
        if !cfg.shelf_online() {
            bail!("the shelf isn't reachable");
        }
        write_info(&dir(cfg, v), &i)?;
    }
    write_info(&cache(v), &i)
}

/// The version's own picture on this PC.
pub fn local(v: &Game, art: Art) -> Option<PathBuf> {
    artwork::pick(&artwork::pictures_in(&cache(v)), &[art.name()])
}

/// Which pictures the version has for itself.
pub fn arts(v: &Game) -> Vec<Art> {
    Art::all().into_iter().filter(|a| local(v, *a).is_some()).collect()
}

fn clear(dir: &std::path::Path, art: Art) {
    for e in EXTS {
        let _ = fs::remove_file(dir.join(format!("{}.{e}", art.name())));
    }
}

/// Give the version a picture of its own.
pub fn store(cfg: &Config, v: &Game, art: Art, bytes: &[u8]) -> Result<PathBuf> {
    if v.version.is_empty() {
        bail!("{} isn't kept in versions", v.name);
    }
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
        crate::remote::require(cfg)?.put_bytes(&format!("/art?id={}&kind={}&own=1", crate::remote::q(&v.id), art.name()), &data)?;
    } else {
        if !cfg.shelf_online() {
            bail!("the shelf isn't reachable, so the picture can't be saved there");
        }
        let d = dir(cfg, v);
        fs::create_dir_all(&d).with_context(|| format!("cannot write to {}", d.display()))?;
        clear(&d, art);
        fs::write(d.join(format!("{}.{ext}", art.name())), &data)?;
    }
    let c = cache(v);
    fs::create_dir_all(&c)?;
    clear(&c, art);
    let p = c.join(format!("{}.{ext}", art.name()));
    fs::write(&p, &data)?;
    if cfg.server_mode() {
        crate::remote::update_own(cfg, v)?;
    }
    Ok(p)
}

/// Take a version's own picture away: it uses the newest version's, or the game's, again.
pub fn remove(cfg: &Config, v: &Game, art: Art) -> Result<()> {
    if cfg.server_mode() {
        crate::remote::require(cfg)?.delete(&format!("/art?id={}&kind={}&own=1", crate::remote::q(&v.id), art.name()))?;
        clear(&cache(v), art);
        return crate::remote::update_own(cfg, v);
    }
    clear(&dir(cfg, v), art);
    clear(&cache(v), art);
    Ok(())
}

/// Bring the version's own pictures and info from the shelf to this PC.
/// With a server: from the list the server sent (pictures come when shown).
pub fn mirror(cfg: &Config, v: &Game) -> bool {
    if v.version.is_empty() {
        return false;
    }
    if cfg.server_mode() {
        return fetch(cfg, v).unwrap_or(false);
    }
    let (src, dst) = (dir(cfg, v), cache(v));
    let mut changed = false;
    let found = artwork::pictures_in(&src);
    for a in Art::all() {
        match artwork::pick(&found, &[a.name()]) {
            Some(p) => {
                let ext = artwork::is_image(&p.to_string_lossy()).unwrap_or("png");
                let d = dst.join(format!("{}.{ext}", a.name()));
                if !artwork::same_file(&p, &d) {
                    let _ = fs::create_dir_all(&dst);
                    clear(&dst, a);
                    changed |= fs::copy(&p, &d).is_ok();
                }
            }
            None if local(v, a).is_some() => {
                clear(&dst, a);
                changed = true;
            }
            None => {}
        }
    }
    let i: Info = fs::read_to_string(src.join(INFO)).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default();
    if i != info(v) {
        let _ = write_info(&dst, &i);
        changed = true;
    }
    changed
}

/// What the server says this version has (from the library snapshot or after a change).
pub fn remember_remote(v: &Game, i: &Info, art: &[(Art, i64)]) -> Result<()> {
    let c = cache(v);
    fs::create_dir_all(&c)?;
    write_info(&c, i)?;
    fs::write(c.join(REMOTE_ART), serde_json::to_string(art)?)?;
    // pictures the server no longer has
    for a in Art::all() {
        if !art.iter().any(|(k, _)| *k == a) {
            clear(&c, a);
        }
    }
    Ok(())
}

/// With a server: fetch this version's pictures that are new or changed.
pub fn fetch(cfg: &Config, v: &Game) -> Result<bool> {
    let c = cache(v);
    let want: Vec<(Art, i64)> = fs::read_to_string(c.join(REMOTE_ART)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut changed = false;
    for (a, mtime) in want {
        let have = local(v, a);
        if have.as_ref().map_or(false, |p| crate::shelf::mtime(p) / 1_000_000_000 >= mtime) {
            continue;
        }
        let bytes = crate::remote::require(cfg)?.get_bytes(&format!("/art?id={}&kind={}&own=1", crate::remote::q(&v.id), a.name()))?;
        let ext = match image::guess_format(&bytes) {
            Ok(image::ImageFormat::Jpeg) => "jpg",
            Ok(image::ImageFormat::WebP) => "webp",
            _ => "png",
        };
        clear(&c, a);
        let p = c.join(format!("{}.{ext}", a.name()));
        fs::write(&p, &bytes)?;
        fs::File::options().write(true).open(&p)?.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(mtime.max(0) as u64))?;
        changed = true;
    }
    Ok(changed)
}

/// For the server: a version's own info and pictures (with dates), as a PC keeps them.
pub fn parts(v: &Game) -> (Info, Vec<(Art, i64)>) {
    let pics = Art::all().into_iter().filter_map(|a| local(v, a).map(|p| (a, crate::shelf::mtime(&p) / 1_000_000_000))).collect();
    (info(v), pics)
}

// ---------------------------------------------------------------- what is shown

/// The newest version of the game `v` belongs to (`v` itself for a game without versions).
fn newest<'a>(t: &'a Title) -> Option<&'a Game> {
    t.versions.first()
}

/// What a version shows: its own info, then the newest version's, then the game's.
pub fn effective_info(t: &Title, v: &Game) -> Info {
    merged(t, v, info(v))
}

/// What a version would show without anything of its own (shown as hints while editing).
pub fn inherited_info(t: &Title, v: &Game) -> Info {
    merged(t, v, Info::default())
}

fn merged(t: &Title, v: &Game, own: Info) -> Info {
    let game = artwork::info(&t.game);
    if v.version.is_empty() {
        return game;
    }
    let top = newest(t).filter(|n| n.id != v.id).map(info).unwrap_or_default();
    let pick = |a: &str, b: &str, c: &str| [a, b, c].into_iter().find(|s| !s.trim().is_empty()).unwrap_or("").to_string();
    let is_newest = newest(t).map_or(true, |n| n.id == v.id);
    Info {
        description: pick(&own.description, &top.description, &game.description),
        developer: pick(&own.developer, &top.developer, &game.developer),
        released: pick(&own.released, &top.released, &game.released),
        page: pick(&own.page, &top.page, &game.page),
        links: [own.links, top.links, game.links.clone()].into_iter().find(|l| !l.is_empty()).unwrap_or_default(),
        // a link brings one particular file: an older version doesn't get the newest's
        download: if !own.download.is_empty() {
            own.download
        } else if is_newest {
            game.download.clone()
        } else {
            String::new()
        },
        ..game
    }
}

/// Where a picture shown for a version comes from: its own, the newest version's, or the game's.
/// The folder name says which version's own picture it is (None: the game's).
pub fn effective_art(t: &Title, v: &Game, a: Art) -> Option<(PathBuf, Option<String>)> {
    if !v.version.is_empty() {
        if let Some(p) = local(v, a) {
            return Some((p, Some(v.version.clone())));
        }
        if let Some(n) = newest(t).filter(|n| n.id != v.id) {
            if let Some(p) = local(n, a) {
                return Some((p, Some(n.version.clone())));
            }
        }
    }
    artwork::local(&t.game, a).map(|p| (p, None))
}

/// Every picture a game and its versions show, with each picture folder read once
/// (the library asks for hundreds of games at a time).
pub struct Shown {
    game: Vec<(String, PathBuf)>,
    /// each version's own pictures, newest version first
    versions: Vec<(String, String, Vec<(String, PathBuf)>)>,
}

impl Shown {
    pub fn of(t: &Title) -> Shown {
        Shown {
            game: artwork::pictures_in(&artwork::cache_dir(&t.game)),
            versions: t.versions.iter().map(|v| (v.id.clone(), v.version.clone(), artwork::pictures_in(&cache(v)))).collect(),
        }
    }

    /// The picture shown for the game (`version` None) or one version: its own, the newest
    /// version's, the game's. With the version folder it came from (None: the game's).
    pub fn pick(&self, version: Option<&str>, a: Art) -> Option<(PathBuf, Option<String>)> {
        if let Some(id) = version {
            let mut chain: Vec<&(String, String, Vec<(String, PathBuf)>)> = self.versions.iter().filter(|(vid, _, _)| vid == id).collect();
            if let Some(n) = self.versions.first().filter(|(vid, _, _)| vid != id) {
                chain.push(n);
            }
            for (_, folder, pics) in chain {
                if let Some(p) = artwork::pick(pics, &[a.name()]) {
                    return Some((p, Some(folder.clone())));
                }
            }
        }
        artwork::pick(&self.game, &[a.name()]).map(|p| (p, None))
    }

    /// The pictures a version has of its own.
    pub fn own(&self, id: &str) -> Vec<Art> {
        self.versions
            .iter()
            .find(|(vid, _, _)| vid == id)
            .map(|(_, _, pics)| Art::all().into_iter().filter(|a| artwork::pick(pics, &[a.name()]).is_some()).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shelf::{Kind, Platform};

    fn game(rel: &str, version: &str) -> Game {
        Game {
            id: Game::make_id(Kind::Fan, "", rel),
            kind: Kind::Fan,
            rel: rel.into(),
            system: String::new(),
            name: "Sonic Boll".into(),
            version: version.into(),
            group: "Sonic".into(),
            program: "boll.exe".into(),
            platform: Platform::Windows,
            appid: 0,
            buildid: String::new(),
            installdir: String::new(),
            size: 0,
            built: 0,
        }
    }

    #[test]
    fn versions_inherit_from_the_newest_then_the_game() {
        let _env = crate::env_lock();
        let t = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", t.path().join("data"));
        let old = game("Sonic/Sonic Boll/0.0.1", "0.0.1");
        let new = game("Sonic/Sonic Boll/3.2.1", "3.2.1");
        let title = Title { game: new.clone(), versions: vec![new.clone(), old.clone()], pinned: false, newest_marked: false };
        // the game's own info
        fs::create_dir_all(artwork::cache_dir(&new)).unwrap();
        let g = Info { description: "A ball game".into(), developer: "Fans".into(), download: "https://x/3.2.1.zip".into(), ..Default::default() };
        fs::write(artwork::cache_dir(&new).join("info.toml"), toml::to_string(&g).unwrap()).unwrap();
        assert_eq!(effective_info(&title, &old).description, "A ball game");
        assert_eq!(effective_info(&title, &old).download, "", "an old version doesn't get the newest's link");
        assert_eq!(effective_info(&title, &new).download, "https://x/3.2.1.zip");
        // the newest version says something of its own: older ones show it too
        write_info(&cache(&new), &Info { description: "Boll 3".into(), ..Default::default() }).unwrap();
        assert_eq!(effective_info(&title, &old).description, "Boll 3");
        // the old one has its own
        write_info(&cache(&old), &Info { description: "The first demo".into(), download: "https://x/0.0.1.zip".into(), ..Default::default() }).unwrap();
        let e = effective_info(&title, &old);
        assert_eq!((e.description.as_str(), e.developer.as_str(), e.download.as_str()), ("The first demo", "Fans", "https://x/0.0.1.zip"));
        // pictures the same way
        let png = |dir: &PathBuf, n: &str| {
            fs::create_dir_all(dir).unwrap();
            image::RgbImage::new(2, 2).save(dir.join(format!("{n}.png"))).unwrap();
        };
        assert!(effective_art(&title, &old, Art::Logo).is_none());
        png(&artwork::cache_dir(&new), "logo");
        assert_eq!(effective_art(&title, &old, Art::Logo).unwrap().1, None);
        png(&cache(&new), "logo");
        assert_eq!(effective_art(&title, &old, Art::Logo).unwrap().1.as_deref(), Some("3.2.1"));
        png(&cache(&old), "logo");
        assert_eq!(effective_art(&title, &old, Art::Logo).unwrap().1.as_deref(), Some("0.0.1"));
        assert_eq!(arts(&old), vec![Art::Logo]);
        let shown = Shown::of(&title);
        assert_eq!(shown.pick(Some(&old.id), Art::Logo).unwrap().1.as_deref(), Some("0.0.1"));
        assert_eq!(shown.pick(Some(&old.id), Art::Cover), None);
        assert_eq!(shown.pick(None, Art::Logo).unwrap().1, None);
        assert_eq!(shown.own(&old.id), vec![Art::Logo]);
    }
}
