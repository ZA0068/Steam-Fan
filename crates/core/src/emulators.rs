//! Emulators kept on the shelf like games, set up to keep everything in their own folder
//! (portable), with their saves, save states and settings (controller layouts too) synced.
//!
//! On the shelf:
//!   `emulators/<id>/<os>/`        the emulator itself, one folder per system (linux, windows, macos)
//!   `emulators/<id>/data-<os>/`   its saves, save states and settings, the newest file wins
//! On this PC: `<emulator games folder>/Emulators/<id>/`, copied from the shelf when needed.
//!
//! Before an emulator game starts: the emulator comes from the shelf (only what changed), then
//! its newer data. After it closes: its newer data goes back. An emulator not on the shelf but
//! installed on this PC (a package) is used as it is, with its data kept in the same place.

use crate::config::{expand, Config};
use crate::transfer::{self, Cancel, Progress};
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// How an emulator is told to keep its things in its own folder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Portable {
    /// An AppImage: a folder `<file>.home` beside it becomes its home folder.
    AppImageHome,
    /// A file beside the program switches it to its own folder; `data` lists what it keeps there.
    File(&'static str),
    /// A folder beside the program holds everything.
    Dir(&'static str),
    /// It already keeps everything beside its program (`data` lists what).
    Already,
}

/// One download of an emulator for one system.
#[derive(Debug, Clone)]
pub struct Build {
    pub os: &'static str,
    /// GitHub `owner/repo` with its releases (empty: from its web page by hand)
    pub repo: &'static str,
    /// words the file name has to have (lower case), and ones it mustn't
    pub want: &'static [&'static str],
    pub avoid: &'static [&'static str],
    /// the program inside, first match (`*.AppImage` matches any AppImage)
    pub program: &'static [&'static str],
    pub portable: Portable,
    /// with `File`/`Already`: the files and folders beside the program that are its data
    pub data: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct Def {
    pub id: &'static str,
    pub name: &'static str,
    /// `roms/<system>` folders it plays when you pick it
    pub systems: &'static [&'static str],
    pub extensions: &'static [&'static str],
    /// where to get it by hand
    pub page: &'static str,
    pub builds: &'static [Build],
    /// commands it has when installed as a package on Linux
    pub installed: &'static [&'static str],
    /// for an installed one: how to point it at its data folder (`{data}`), if it can be
    pub data_arg: &'static [&'static str],
    /// what to start a game with (`{rom}`: the game file)
    pub args: &'static [&'static str],
}

const NOARCH: &[&str] = &["arm", "aarch64", "debug", "symbols", "pdb", ".zsync", ".sig", ".sha256", "source"];

pub const CATALOG: &[Def] = &[
    Def {
        id: "dolphin",
        name: "Dolphin",
        systems: &["wii", "gc", "gamecube", "ngc"],
        extensions: &["iso", "wbfs", "rvz", "gcz", "ciso", "wad", "dol", "elf"],
        page: "https://dolphin-emu.org/download/",
        builds: &[Build { os: "windows", repo: "", want: &[], avoid: &[], program: &["Dolphin.exe"], portable: Portable::File("portable.txt"), data: &["User"] }],
        installed: &["dolphin-emu"],
        data_arg: &["-u", "{data}"],
        args: &["-b", "-e", "{rom}"],
    },
    Def {
        id: "cemu",
        name: "Cemu",
        systems: &["wiiu"],
        extensions: &["rpx", "wud", "wux", "wua"],
        page: "https://cemu.info/",
        builds: &[
            Build { os: "linux", repo: "cemu-project/Cemu", want: &["appimage"], avoid: NOARCH, program: &["*.AppImage"], portable: Portable::AppImageHome, data: &[] },
            Build { os: "windows", repo: "cemu-project/Cemu", want: &["windows", ".zip"], avoid: NOARCH, program: &["Cemu.exe"], portable: Portable::Dir("portable"), data: &[] },
        ],
        installed: &["cemu", "Cemu"],
        data_arg: &[],
        args: &["-f", "-g", "{rom}"],
    },
    Def {
        id: "ryujinx",
        name: "Ryujinx",
        systems: &["switch", "nsw"],
        extensions: &["nsp", "xci", "nca", "nro"],
        page: "https://ryujinx.app/",
        builds: &[
            Build { os: "linux", repo: "", want: &[], avoid: &[], program: &["Ryujinx", "Ryujinx.sh", "*.AppImage"], portable: Portable::Dir("portable"), data: &[] },
            Build { os: "windows", repo: "", want: &[], avoid: &[], program: &["Ryujinx.exe"], portable: Portable::Dir("portable"), data: &[] },
        ],
        installed: &["ryujinx", "Ryujinx"],
        data_arg: &["-r", "{data}"],
        args: &["--fullscreen", "{rom}"],
    },
    Def {
        id: "pcsx2",
        name: "PCSX2",
        systems: &["ps2"],
        extensions: &["iso", "chd", "cso", "gz", "bin", "elf"],
        page: "https://pcsx2.net/downloads",
        builds: &[
            Build { os: "linux", repo: "PCSX2/pcsx2", want: &["appimage"], avoid: NOARCH, program: &["*.AppImage"], portable: Portable::AppImageHome, data: &[] },
            Build {
                os: "windows",
                repo: "PCSX2/pcsx2",
                want: &["windows", ".7z"],
                avoid: NOARCH,
                program: &["pcsx2-qt.exe"],
                portable: Portable::File("portable.ini"),
                data: &["bios", "memcards", "sstates", "inis", "gamesettings", "inputprofiles", "covers", "cheats", "patches"],
            },
        ],
        installed: &["pcsx2-qt", "pcsx2"],
        data_arg: &[],
        args: &["-fullscreen", "-batch", "--", "{rom}"],
    },
    Def {
        id: "duckstation",
        name: "DuckStation",
        systems: &["ps1", "psx", "playstation"],
        extensions: &["cue", "chd", "m3u", "pbp", "iso", "bin", "ecm", "mds"],
        page: "https://www.duckstation.org/",
        builds: &[
            Build { os: "linux", repo: "stenzek/duckstation", want: &["appimage"], avoid: NOARCH, program: &["*.AppImage"], portable: Portable::AppImageHome, data: &[] },
            Build {
                os: "windows",
                repo: "stenzek/duckstation",
                want: &["windows", "x64", ".zip"],
                avoid: NOARCH,
                program: &["duckstation-qt-x64-ReleaseLTCG.exe", "duckstation-qt*.exe"],
                portable: Portable::File("portable.txt"),
                data: &["bios", "memcards", "savestates", "settings.ini", "gamesettings", "inputprofiles", "covers", "cheats"],
            },
        ],
        installed: &["duckstation-qt", "duckstation"],
        data_arg: &[],
        args: &["-batch", "-fullscreen", "--", "{rom}"],
    },
    Def {
        id: "ppsspp",
        name: "PPSSPP",
        systems: &["psp"],
        extensions: &["iso", "cso", "pbp", "chd", "elf"],
        page: "https://www.ppsspp.org/download/",
        builds: &[Build { os: "windows", repo: "", want: &[], avoid: &[], program: &["PPSSPPWindows64.exe"], portable: Portable::Already, data: &["memstick"] }],
        installed: &["PPSSPPSDL", "ppsspp", "PPSSPPQt"],
        data_arg: &[],
        args: &["--fullscreen", "{rom}"],
    },
    Def {
        id: "rpcs3",
        name: "RPCS3",
        systems: &["ps3"],
        extensions: &["bin", "iso", "self", "elf"],
        page: "https://rpcs3.net/download",
        builds: &[
            Build { os: "linux", repo: "RPCS3/rpcs3-binaries-linux", want: &["appimage"], avoid: NOARCH, program: &["*.AppImage"], portable: Portable::AppImageHome, data: &[] },
            Build {
                os: "windows",
                repo: "RPCS3/rpcs3-binaries-win",
                want: &["win64", ".7z"],
                avoid: NOARCH,
                program: &["rpcs3.exe"],
                portable: Portable::Already,
                data: &["dev_hdd0/home", "config", "input_configs"],
            },
        ],
        installed: &["rpcs3"],
        data_arg: &[],
        args: &["--no-gui", "{rom}"],
    },
    Def {
        id: "mgba",
        name: "mGBA",
        systems: &["gba", "gb", "gbc"],
        extensions: &["gba", "gb", "gbc", "sgb", "zip"],
        page: "https://mgba.io/downloads.html",
        builds: &[
            Build { os: "linux", repo: "mgba-emu/mgba", want: &["appimage"], avoid: NOARCH, program: &["*.AppImage"], portable: Portable::AppImageHome, data: &[] },
            Build {
                os: "windows",
                repo: "mgba-emu/mgba",
                want: &["win64", ".7z"],
                avoid: &["installer", "debug", "arm"],
                program: &["mGBA.exe"],
                portable: Portable::File("portable.ini"),
                data: &["config.ini", "qt.ini", "nointro.dat", "cheats", "states"],
            },
        ],
        installed: &["mgba-qt", "mgba"],
        data_arg: &[],
        args: &["-f", "{rom}"],
    },
    Def {
        id: "melonds",
        name: "melonDS",
        systems: &["nds", "ds"],
        extensions: &["nds", "dsi", "srl"],
        page: "https://melonds.kuribo64.net/downloads.php",
        builds: &[
            Build { os: "linux", repo: "melonDS-emu/melonDS", want: &["appimage"], avoid: NOARCH, program: &["*.AppImage"], portable: Portable::AppImageHome, data: &[] },
            Build {
                os: "windows",
                repo: "melonDS-emu/melonDS",
                want: &["windows", ".zip"],
                avoid: NOARCH,
                program: &["melonDS.exe"],
                portable: Portable::Already,
                data: &["melonDS.toml", "melonDS.ini"],
            },
        ],
        installed: &["melonDS", "melonds"],
        data_arg: &[],
        args: &["-f", "{rom}"],
    },
    Def {
        id: "azahar",
        name: "Azahar",
        systems: &["3ds", "n3ds"],
        extensions: &["3ds", "cci", "cxi", "cia", "3dsx", "app"],
        page: "https://azahar-emu.org/",
        builds: &[
            Build { os: "linux", repo: "azahar-emu/azahar", want: &["appimage"], avoid: NOARCH, program: &["*.AppImage"], portable: Portable::AppImageHome, data: &[] },
            Build { os: "windows", repo: "azahar-emu/azahar", want: &["windows"], avoid: &["installer", "setup", "arm", "debug", "pdb"], program: &["azahar.exe", "azahar-qt.exe"], portable: Portable::Dir("user"), data: &[] },
        ],
        installed: &["azahar", "azahar-qt"],
        data_arg: &[],
        args: &["{rom}"],
    },
];

pub fn def(id: &str) -> Option<&'static Def> {
    CATALOG.iter().find(|d| d.id.eq_ignore_ascii_case(id))
}

/// The catalog's emulator for a system folder (`wiiu` → Cemu).
pub fn for_system(system: &str) -> Option<&'static Def> {
    CATALOG.iter().find(|d| d.systems.iter().any(|s| s.eq_ignore_ascii_case(system)))
}

/// This PC's system, as the shelf names it.
pub fn this_os() -> &'static str {
    if crate::platform::WINDOWS {
        "windows"
    } else if crate::platform::MACOS {
        "macos"
    } else {
        "linux"
    }
}

impl Def {
    pub fn build(&self, os: &str) -> Option<&'static Build> {
        let builds: &'static [Build] = self.builds;
        builds.iter().find(|b| b.os == os)
    }
}

/// The emulator's folder for one system on the shelf.
pub fn shelf_dir(cfg: &Config, id: &str, os: &str) -> PathBuf {
    cfg.shelf().join("emulators").join(id).join(os)
}

/// Its data on the shelf.
pub fn shelf_data(cfg: &Config, id: &str, os: &str) -> PathBuf {
    cfg.shelf().join("emulators").join(id).join(format!("data-{os}"))
}

/// The emulator on this PC.
pub fn local_dir(cfg: &Config, id: &str) -> PathBuf {
    expand(&cfg.rom_local).join("Emulators").join(id)
}

/// The id the server knows an emulator's files and data by.
pub fn remote_id(id: &str, os: &str) -> String {
    format!("emu:{id}:{os}")
}

/// Parse a server id back: (emulator, system, data?).
pub fn parse_remote_id(s: &str) -> Option<(&str, &str)> {
    let rest = s.strip_prefix("emu:")?;
    let (id, os) = rest.split_once(':')?;
    if def(id).is_none() || !["linux", "windows", "macos"].contains(&os) {
        return None;
    }
    Some((id, os))
}

/// Not worth syncing: caches (shaders!), logs, crash dumps.
pub fn not_data(rel: &Path) -> bool {
    rel.components().any(|c| {
        let s = c.as_os_str().to_string_lossy().to_lowercase();
        s.contains("cache") || s.contains("shader") || s == "logs" || s == "log" || s.ends_with(".log") || s.contains("crash") || s == "dumps" || s.ends_with(".steam-fan-part")
    })
}

fn glob_match(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern.eq_ignore_ascii_case(name),
        Some((a, b)) => {
            let (n, a, b) = (name.to_lowercase(), a.to_lowercase(), b.to_lowercase());
            n.len() >= a.len() + b.len() && n.starts_with(&a) && n.ends_with(&b)
        }
    }
}

/// The program in an emulator folder (also one folder down, as some downloads have it).
pub fn find_program(dir: &Path, b: &Build) -> Option<PathBuf> {
    let look = |d: &Path| -> Option<PathBuf> {
        let names: Vec<String> = fs::read_dir(d).ok()?.flatten().filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        b.program.iter().find_map(|p| names.iter().find(|n| glob_match(p, n)).map(|n| d.join(n)))
    };
    look(dir).or_else(|| fs::read_dir(dir).ok()?.flatten().filter(|e| e.path().is_dir()).find_map(|e| look(&e.path())))
}

/// Where a portable emulator keeps its data, next to its program.
fn data_dirs(program: &Path, b: &Build) -> Vec<(PathBuf, String)> {
    let dir = program.parent().unwrap_or(Path::new("."));
    match b.portable {
        Portable::AppImageHome => vec![(PathBuf::from(format!("{}.home", program.display())), String::new())],
        Portable::Dir(d) => vec![(dir.join(d), String::new())],
        Portable::File(_) | Portable::Already => b.data.iter().map(|d| (dir.join(d), d.to_string())).collect(),
    }
}

/// Set an emulator up to keep everything in its own folder.
pub fn make_portable(program: &Path, b: &Build) -> Result<()> {
    let dir = program.parent().unwrap_or(Path::new("."));
    match b.portable {
        Portable::AppImageHome => {
            fs::create_dir_all(format!("{}.home", program.display()))?;
        }
        Portable::Dir(d) => fs::create_dir_all(dir.join(d))?,
        Portable::File(f) => {
            if !dir.join(f).exists() {
                fs::write(dir.join(f), "")?;
            }
        }
        Portable::Already => {}
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = fs::metadata(program) {
            let mut p = m.permissions();
            p.set_mode(p.mode() | 0o755);
            let _ = fs::set_permissions(program, p);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- on the shelf

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct OnShelf {
    pub id: String,
    pub os: String,
    /// the release it came from (the tag), or "by hand"
    pub version: String,
    pub size: u64,
}

const VERSION_FILE: &str = ".steam-fan-emulator";

/// Which emulators the shelf has, for which systems.
pub fn on_shelf(cfg: &Config) -> Vec<OnShelf> {
    let mut out = vec![];
    for d in CATALOG {
        for os in ["linux", "windows", "macos"] {
            let dir = shelf_dir(cfg, d.id, os);
            if dir.is_dir() && fs::read_dir(&dir).map_or(false, |mut r| r.next().is_some()) {
                out.push(OnShelf {
                    id: d.id.into(),
                    os: os.into(),
                    version: fs::read_to_string(dir.join(VERSION_FILE)).map(|s| s.trim().to_string()).unwrap_or_else(|_| "by hand".into()),
                    size: transfer::dir_size(&dir),
                });
            }
        }
    }
    out
}

/// The download of the newest release for one system, from GitHub.
pub fn latest(d: &Def, os: &str) -> Result<(String, String, u64, String)> {
    let b = d.build(os).with_context(|| format!("there's no {} build of {} to download", os, d.name))?;
    if b.repo.is_empty() {
        bail!("{} for {os} isn't on GitHub: download it from {} and use Add from file", d.name, d.page);
    }
    let (owner, repo) = b.repo.split_once('/').unwrap();
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases?per_page=10");
    let rel: serde_json::Value = ureq::get(&url)
        .set("User-Agent", "steam-fan")
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(20))
        .call()
        .map_err(|e| crate::source::net_error(&url, e))?
        .into_json()?;
    let pick = |rel: &serde_json::Value| -> Option<(String, String, u64, String)> {
        let tag = rel.get("tag_name")?.as_str()?.to_string();
        rel.get("assets")?.as_array()?.iter().find_map(|a| {
            let name = a.get("name")?.as_str()?;
            let low = name.to_lowercase();
            if b.want.iter().all(|w| low.contains(w)) && !b.avoid.iter().any(|w| low.contains(w)) {
                Some((name.to_string(), a.get("browser_download_url")?.as_str()?.to_string(), a.get("size")?.as_u64().unwrap_or(0), tag.clone()))
            } else {
                None
            }
        })
    };
    let all = rel.as_array().cloned().unwrap_or_default();
    // a full release first, a preview build only if there's nothing else
    all.iter()
        .filter(|r| !r.get("prerelease").and_then(|p| p.as_bool()).unwrap_or(false) && !r.get("draft").and_then(|p| p.as_bool()).unwrap_or(false))
        .chain(all.iter())
        .find_map(pick)
        .with_context(|| format!("no {os} download of {} found on GitHub ({}); download it from {} and use Add from file", d.name, b.repo, d.page))
}

/// Put an emulator on the shelf from a downloaded file (an AppImage, or an archive) or a folder.
pub fn place(cfg: &Config, d: &Def, os: &str, source: &Path, version: &str, cancel: &Cancel, cb: &mut dyn FnMut(&Progress)) -> Result<PathBuf> {
    let b = d.build(os).cloned().unwrap_or(Build { os: "linux", repo: "", want: &[], avoid: &[], program: &["*.AppImage"], portable: Portable::AppImageHome, data: &[] });
    let dst = shelf_dir(cfg, d.id, os);
    let tmp = dst.with_file_name(format!(".{os}.new"));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp)?;
    let low = source.to_string_lossy().to_lowercase();
    if source.is_dir() {
        transfer::copy_tree(source, &tmp, &|_| false, cancel, cb)?;
    } else if low.ends_with(".appimage") || low.ends_with(".exe") {
        let name = source.file_name().unwrap();
        fs::copy(source, tmp.join(name))?;
    } else if crate::import::is_archive(source) || low.ends_with(".tar.gz") || low.ends_with(".tar.xz") || low.ends_with(".tgz") {
        let unpacked = crate::import::unpack(source)?;
        transfer::copy_tree(&unpacked, &tmp, &|_| false, cancel, cb)?;
        crate::import::clean_temp();
    } else {
        bail!("{} isn't an emulator download (an AppImage, an archive or a folder)", source.display());
    }
    let Some(program) = find_program(&tmp, &b) else {
        let _ = fs::remove_dir_all(&tmp);
        bail!("{} isn't in it (looked for {})", d.name, b.program.join(", "));
    };
    make_portable(&program, &b)?;
    fs::write(tmp.join(VERSION_FILE), version)?;
    // swap: the old one goes only when the new one is complete
    let old = dst.with_file_name(format!(".{os}.old"));
    let _ = fs::remove_dir_all(&old);
    if dst.exists() {
        fs::rename(&dst, &old)?;
    }
    fs::rename(&tmp, &dst)?;
    let _ = fs::remove_dir_all(&old);
    Ok(dst)
}

/// Download the newest release of an emulator for a system onto the shelf.
pub fn get(cfg: &Config, d: &Def, os: &str, cancel: &Cancel, status: &mut dyn FnMut(&str), cb: &mut dyn FnMut(&Progress)) -> Result<String> {
    status(&format!("looking for the newest {} for {os}", d.name));
    let (name, url, _size, tag) = latest(d, os)?;
    status(&format!("downloading {name}"));
    let file = crate::source::fetch(&url, cancel, cb)?;
    // keep the file's real name: the type decides how it's put on the shelf
    let named = file.with_file_name(&name);
    fs::rename(&file, &named)?;
    status(&format!("putting {} on the shelf", d.name));
    let r = place(cfg, d, os, &named, &tag, cancel, cb);
    let _ = fs::remove_file(&named);
    crate::source::downloads_dir_clean();
    r?;
    Ok(format!("{} {tag} for {os} is on the shelf", d.name))
}

// ---------------------------------------------------------------- playing

/// The emulator ready to start on this PC: its command (without the game) and its data folders.
pub struct Ready {
    pub command: Vec<String>,
    /// (folder on this PC, its name inside the synced data) — empty: nothing to sync
    pub data: Vec<(PathBuf, String)>,
}

/// Bring the emulator from the shelf (only changed files) and set it up. Falls back to one
/// installed on this PC as a package.
pub fn prepare(cfg: &Config, d: &Def, cancel: &Cancel, status: &mut dyn FnMut(&str), cb: &mut dyn FnMut(&Progress)) -> Result<Ready> {
    let os = this_os();
    let local = local_dir(cfg, d.id);
    if let Some(b) = d.build(os) {
        // from the shelf (or the server)
        if cfg.shelf_online() {
            if cfg.server_mode() {
                if crate::remote::emulator_present(cfg, &remote_id(d.id, os)).unwrap_or(false) {
                    status(&format!("getting {} from the server", d.name));
                    crate::remote::download_tree(cfg, &remote_id(d.id, os), &local, cancel, cb)?;
                }
            } else {
                let src = shelf_dir(cfg, d.id, os);
                if src.is_dir() {
                    status(&format!("getting {} from the shelf", d.name));
                    transfer::copy_newer(&src, &local, &|_| false)?;
                }
            }
        }
        if let Some(program) = find_program(&local, b) {
            make_portable(&program, b)?;
            let data = data_dirs(&program, b);
            return Ok(Ready { command: vec![program.to_string_lossy().into_owned()], data });
        }
    }
    // installed as a package
    if let Some(cmd) = d.installed.iter().find(|c| which(c)) {
        let mut command = vec![cmd.to_string()];
        let mut data = vec![];
        if !d.data_arg.is_empty() {
            let dir = local.join("data");
            fs::create_dir_all(&dir)?;
            command.extend(d.data_arg.iter().map(|a| a.replace("{data}", &dir.to_string_lossy())));
            data.push((dir, String::new()));
        }
        return Ok(Ready { command, data });
    }
    bail!("{} isn't on the shelf or on this PC: Settings → Emulators → Get {} (or install it)", d.name, d.name)
}

fn which(cmd: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&path).any(|d| {
        let p = d.join(cmd);
        p.is_file() || (crate::platform::WINDOWS && p.with_extension("exe").is_file())
    })
}

/// The command to start a game with an emulator.
pub fn command(d: &Def, ready: &Ready, rom: &Path) -> Vec<String> {
    let mut c = ready.command.clone();
    c.extend(d.args.iter().map(|a| a.replace("{rom}", &rom.to_string_lossy())));
    c
}

/// Before playing: newer saves, states and settings from the shelf.
pub fn pull_data(cfg: &Config, d: &Def, ready: &Ready) -> Result<u64> {
    let os = this_os();
    let mut n = 0;
    if cfg.server_mode() {
        let local = local_dir(cfg, d.id).join(".data-mirror");
        n += crate::remote::pull_tree(cfg, &remote_id(d.id, os), &local)?;
        for (dir, name) in &ready.data {
            n += transfer::copy_newer(&local.join(name), dir, &not_data)?;
        }
    } else {
        let src = shelf_data(cfg, d.id, os);
        for (dir, name) in &ready.data {
            n += transfer::copy_newer(&src.join(name), dir, &not_data)?;
        }
    }
    Ok(n)
}

/// After playing: newer saves, states and settings to the shelf.
pub fn push_data(cfg: &Config, d: &Def, ready: &Ready) -> Result<u64> {
    let os = this_os();
    let mut n = 0;
    if cfg.server_mode() {
        let local = local_dir(cfg, d.id).join(".data-mirror");
        for (dir, name) in &ready.data {
            transfer::copy_newer(dir, &local.join(name), &not_data)?;
        }
        n += crate::remote::push_tree(cfg, &remote_id(d.id, os), &local)?;
    } else {
        if !cfg.shelf_online() {
            bail!("the shelf isn't reachable");
        }
        let dst = shelf_data(cfg, d.id, os);
        for (dir, name) in &ready.data {
            n += transfer::copy_newer(dir, &dst.join(name), &not_data)?;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_sound() {
        for d in CATALOG {
            assert!(!d.systems.is_empty() && !d.args.is_empty(), "{}", d.id);
            assert!(d.args.iter().any(|a| a.contains("{rom}")), "{}", d.id);
            for b in d.builds {
                assert!(["linux", "windows", "macos"].contains(&b.os));
                assert!(!b.program.is_empty());
            }
        }
        assert_eq!(for_system("WiiU").unwrap().id, "cemu");
        assert_eq!(parse_remote_id("emu:cemu:linux"), Some(("cemu", "linux")));
        assert_eq!(parse_remote_id("emu:nope:linux"), None);
        assert_eq!(parse_remote_id("emu:cemu:../x"), None);
    }

    #[test]
    fn places_and_finds_a_portable_emulator() {
        let _env = crate::env_lock();
        let t = tempfile::tempdir().unwrap();
        let cfg = Config { shelf: t.path().join("shelf").to_string_lossy().into(), rom_local: t.path().join("pc").to_string_lossy().into(), ..Default::default() };
        fs::create_dir_all(cfg.shelf().join("roms")).unwrap();
        // a "download": an archive-like folder with the program one level down
        let src = t.path().join("dl/Cemu_2.6");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("Cemu-2.6-x86_64.AppImage"), b"#!/bin/sh\n").unwrap();
        let d = def("cemu").unwrap();
        place(&cfg, d, "linux", &t.path().join("dl"), "v2.6", &Cancel::default(), &mut |_| {}).unwrap();
        let on = on_shelf(&cfg);
        assert_eq!((on[0].id.as_str(), on[0].os.as_str(), on[0].version.as_str()), ("cemu", "linux", "v2.6"));
        // shelf → this PC, set up portable, data synced both ways
        if this_os() == "linux" {
            let ready = prepare(&cfg, d, &Cancel::default(), &mut |_| {}, &mut |_| {}).unwrap();
            assert!(ready.command[0].ends_with(".AppImage"));
            let (home, _) = &ready.data[0];
            assert!(home.to_string_lossy().ends_with(".AppImage.home") && home.is_dir());
            fs::create_dir_all(home.join(".local/share/Cemu/mlc01/usr/save")).unwrap();
            fs::write(home.join(".local/share/Cemu/mlc01/usr/save/game.sav"), b"save").unwrap();
            fs::create_dir_all(home.join(".cache/Cemu/shaderCache")).unwrap();
            fs::write(home.join(".cache/Cemu/shaderCache/x.bin"), b"big").unwrap();
            assert_eq!(push_data(&cfg, d, &ready).unwrap(), 1, "the save, not the shader cache");
            assert!(shelf_data(&cfg, "cemu", "linux").join(".local/share/Cemu/mlc01/usr/save/game.sav").is_file());
            let cmd = command(d, &ready, Path::new("/games/x.wua"));
            assert_eq!(&cmd[1..], &["-f", "-g", "/games/x.wua"]);
        }
    }
}
