//! Icons for the Steam library.
//!
//! Steam on Linux can't read the icon inside a Windows `.exe`, and the exe isn't
//! on the PC before the first Play anyway. So the icon is taken from the copy on
//! the shelf and saved as a PNG on the PC (Steam reads it from there, also when
//! the NAS is off).
//!
//! Looked for, in order: the program's own icon, an `.ico`/`.png` in the game
//! folder or in an `Icons`/`icon` subfolder, a Wii U `meta/iconTex.tga`.

use crate::shelf::{listing, Game, Kind};
use anyhow::{Context, Result};
use image::DynamicImage;
use std::path::{Path, PathBuf};

pub fn icon_dir() -> PathBuf {
    crate::config::data_dir().join("icons")
}

fn exe_icon(exe: &Path) -> Result<DynamicImage> {
    use pelite::FileMap;
    let map = FileMap::open(exe).context("cannot open program")?;
    let res = match pelite::PeFile::from_bytes(map.as_ref()).context("not a Windows program")? {
        pelite::Wrap::T32(f) => pelite::pe32::Pe::resources(f),
        pelite::Wrap::T64(f) => pelite::pe64::Pe::resources(f),
    }
    .context("no resources")?;
    let (_, group) = res.icons().flatten().next().context("no icon")?;
    let mut ico = Vec::new();
    group.write(&mut ico).context("bad icon")?;
    Ok(image::load_from_memory_with_format(&ico, image::ImageFormat::Ico)?)
}

fn picture_in(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = vec![];
    let mut look = |d: &Path| {
        if let Ok(l) = listing(d) {
            for e in l {
                let low = e.name.to_lowercase();
                if e.is_file && (low.ends_with(".ico") || low.ends_with(".png")) {
                    found.push(d.join(&e.name));
                } else if e.is_dir && (low == "icons" || low == "icon") {
                    if let Ok(sub) = listing(&d.join(&e.name)) {
                        for s in sub {
                            let l2 = s.name.to_lowercase();
                            if s.is_file && (l2.ends_with(".ico") || l2.ends_with(".png")) {
                                found.push(d.join(&e.name).join(&s.name));
                            }
                        }
                    }
                }
            }
        }
    };
    look(dir);
    // prefer files called icon, .ico over .png (a png is often a screenshot), short names
    found.sort_by_key(|p| {
        let n = p.file_name().unwrap().to_string_lossy().to_lowercase();
        (!n.contains("icon") && !p.to_string_lossy().to_lowercase().contains("/icons/"), !n.ends_with(".ico"), n.len())
    });
    found.into_iter().next()
}

/// Make (or reuse) the PNG icon for a game. Returns its path, or None.
pub fn icon_for(cfg: &crate::config::Config, g: &Game) -> Option<PathBuf> {
    let key = format!("{:08x}", crc32fast::hash(g.id.as_bytes()));
    let out = icon_dir().join(format!("{key}.png"));
    if out.exists() {
        return Some(out);
    }
    let src = g.shelf_path(cfg);
    let dir = if src.is_file() { src.parent()?.to_path_buf() } else { src.clone() };
    let mut img = None;
    if matches!(g.kind, Kind::Fan | Kind::NonSteam) && g.program.to_lowercase().ends_with(".exe") {
        let exe = if src.is_file() { src.clone() } else { src.join(&g.program) };
        img = exe_icon(&exe).ok();
    }
    if img.is_none() {
        img = picture_in(&dir).and_then(|p| image::open(p).ok());
    }
    if img.is_none() && g.kind == Kind::Rom {
        img = image::open(dir.join("meta/iconTex.tga")).ok();
    }
    let img = img?;
    std::fs::create_dir_all(icon_dir()).ok()?;
    img.save_with_format(&out, image::ImageFormat::Png).ok()?;
    Some(out)
}
