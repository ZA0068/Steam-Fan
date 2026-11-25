//! Reading the shelf: which games are there and how each one starts.
//!
//! ```text
//! <shelf>/fangames/<group>/<game>              fan games (group = Sonic, Kirby, ...)
//! <shelf>/fangames/<group>/<subgroup>/<game>
//! <shelf>/nonsteam/<group>/<game>              other non-Steam games, same layout
//! <shelf>/roms/<system>/<games_dir>/<game>     emulator games (system = wii, WIIU, Switch, ...)
//! <shelf>/steam/<appid>/                       Steam games archived by SteamCMD on the NAS
//! <shelf>/saves/...                            save backups
//! ```
//!
//! Only folder listings are used to find games, never a deep walk, so a big
//! shelf over the network is read in seconds. Deep walks happen once per game
//! to pick its program, and the result is kept in the database.

use crate::config::{Config, Emulator};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Kind {
    Fan,
    NonSteam,
    Rom,
    Steam,
}

impl Kind {
    pub fn prefix(self) -> &'static str {
        match self {
            Kind::Fan => "fan",
            Kind::NonSteam => "nonsteam",
            Kind::Rom => "rom",
            Kind::Steam => "steam",
        }
    }
    pub fn folder(self) -> &'static str {
        match self {
            Kind::Fan => "fangames",
            Kind::NonSteam => "nonsteam",
            Kind::Rom => "roms",
            Kind::Steam => "steam",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Kind::Fan => "Fan games",
            Kind::NonSteam => "Non-Steam games",
            Kind::Rom => "Emulator games",
            Kind::Steam => "Steam archive",
        }
    }
    pub fn all() -> [Kind; 4] {
        [Kind::Fan, Kind::NonSteam, Kind::Rom, Kind::Steam]
    }
    pub fn from_prefix(s: &str) -> Option<Kind> {
        Kind::all().into_iter().find(|k| k.prefix() == s)
    }
}

/// How a game is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Platform {
    /// Windows program, runs through Proton.
    Windows,
    /// Native Linux program.
    Linux,
    /// Started by an emulator from the settings.
    Emulator,
    /// A real Steam game.
    Steam,
}

/// One game on the shelf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Game {
    /// `fan:<rel>`, `nonsteam:<rel>`, `rom:<system>/<rel>` or `steam:<appid>`.
    pub id: String,
    pub kind: Kind,
    /// Path under the kind's folder (for ROMs: under the system's games folder).
    pub rel: String,
    /// Emulator system folder name, for ROMs.
    pub system: String,
    pub name: String,
    /// Group shown as a Steam tag: Sonic The Hedgehog, wii, ...
    pub group: String,
    /// What starts the game, relative to the game folder (exe, rom file). Empty: not found yet.
    pub program: String,
    pub platform: Platform,
    /// Steam archive only.
    pub appid: u32,
    pub buildid: String,
    pub installdir: String,
    pub size: u64,
}

impl Game {
    pub fn make_id(kind: Kind, system: &str, rel: &str) -> String {
        match kind {
            Kind::Rom => format!("rom:{system}/{rel}"),
            _ => format!("{}:{rel}", kind.prefix()),
        }
    }

    /// Where the game sits on the shelf.
    pub fn shelf_path(&self, cfg: &Config) -> PathBuf {
        let root = cfg.shelf();
        match self.kind {
            Kind::Fan | Kind::NonSteam => root.join(self.kind.folder()).join(&self.rel),
            Kind::Rom => rom_games_root(&root.join("roms").join(&self.system), cfg.emulator(&self.system))
                .join(&self.rel),
            Kind::Steam => root.join("steam").join(self.appid.to_string()),
        }
    }

    /// Where the game is copied on this PC (not for Steam games: Steam's own library is used).
    pub fn local_path(&self, cfg: &Config) -> PathBuf {
        use crate::config::expand;
        match self.kind {
            Kind::Fan => expand(&cfg.fan_local).join(&self.rel),
            Kind::NonSteam => expand(&cfg.nonsteam_local).join(&self.rel),
            Kind::Rom => expand(&cfg.rom_local).join(&self.system).join(&self.rel),
            Kind::Steam => PathBuf::new(),
        }
    }

    /// Where this game's saves are kept on the shelf.
    pub fn save_path(&self, cfg: &Config) -> PathBuf {
        let s = cfg.shelf().join("saves");
        match self.kind {
            Kind::Fan => s.join("fangames").join(&self.rel),
            Kind::NonSteam => s.join("nonsteam").join(&self.rel),
            Kind::Rom => s.join("emulators").join(&self.system),
            Kind::Steam => s.join("steam").join(self.appid.to_string()),
        }
    }

    /// A single loose program file instead of a folder.
    pub fn is_single_file(&self, cfg: &Config) -> bool {
        self.shelf_path(cfg).is_file()
    }
}

pub fn rom_games_root(system_dir: &Path, emu: Option<&Emulator>) -> PathBuf {
    match emu {
        Some(e) if !e.games_dir.is_empty() => {
            // the games folder name is matched without regard to case
            if let Ok(rd) = fs::read_dir(system_dir) {
                for d in rd.flatten() {
                    if d.file_name().to_string_lossy().eq_ignore_ascii_case(&e.games_dir) {
                        return d.path();
                    }
                }
            }
            system_dir.join(&e.games_dir)
        }
        _ => system_dir.to_path_buf(),
    }
}

// ---------------------------------------------------------------- folder listings

const SKIP: [&str; 4] = ["@eaDir", "#recycle", "#snapshot", "__MACOSX"];

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub is_file: bool,
}

/// One directory read; the entry types come with the listing, so no extra
/// request per file is sent to the NAS.
pub fn listing(path: &Path) -> std::io::Result<Vec<Entry>> {
    let mut out: Vec<Entry> = fs::read_dir(path)?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || SKIP.contains(&name.as_str()) {
                return None;
            }
            let ft = e.file_type().ok()?;
            let (is_dir, is_file) = if ft.is_symlink() {
                let m = fs::metadata(e.path()).ok();
                (m.as_ref().map_or(false, |m| m.is_dir()), m.as_ref().map_or(false, |m| m.is_file()))
            } else {
                (ft.is_dir(), ft.is_file())
            };
            Some(Entry { name, is_dir, is_file })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

pub fn mtime(path: &Path) -> i64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as i64)
}

// ---------------------------------------------------------------- programs

fn ignore_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"(?i)^(unins\d*|unitycrashhandler(32|64)?|crashpad_handler|crashreport.*|dxsetup|dxwebsetup|vc_?redist.*|vcredist.*|dotnetfx.*|ndp\d.*|oalinst|physx.*|notification_helper|ue4prereqsetup.*|uecrashreporter.*|7za?)\.exe$",
        )
        .unwrap()
    })
}

fn bad_words() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)launcher|config|setting|setup|install|editor|server|dedicated|crash|update|patch|tool|debug")
            .unwrap()
    })
}

pub fn is_windows_program(name: &str) -> bool {
    name.to_lowercase().ends_with(".exe") && !ignore_re().is_match(name)
}
pub fn is_linux_program(name: &str) -> bool {
    let l = name.to_lowercase();
    l.ends_with(".x86_64") || l.ends_with(".appimage")
}
pub fn is_program(name: &str) -> bool {
    is_windows_program(name) || is_linux_program(name)
}

fn has_top_program(path: &Path) -> bool {
    listing(path).map(|l| l.iter().any(|e| e.is_file && is_program(&e.name))).unwrap_or(false)
}

const BUILD_DIRS: [&str; 14] = [
    "bin", "bin32", "bin64", "x86", "x64", "win32", "win64", "windows", "linux", "binaries", "game", "data",
    "release", "build",
];

/// Is this folder one game, or a group of games?
/// A game: a program sits directly inside, or the programs inside are all in
/// build folders like `bin/`, or fewer than two subfolders have programs.
pub fn is_game_folder(path: &Path, items: &[Entry]) -> bool {
    if items.iter().any(|e| e.is_file && is_program(&e.name)) {
        return true;
    }
    let with_prog: Vec<&Entry> =
        items.iter().filter(|e| e.is_dir && has_top_program(&path.join(&e.name))).collect();
    if with_prog.iter().all(|e| BUILD_DIRS.contains(&e.name.to_lowercase().as_str())) {
        return true;
    }
    with_prog.len() < 2
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// Every program file in a game folder (deep walk, done once per game).
fn programs_in(dir: &Path) -> Vec<(PathBuf, u64)> {
    walkdir::WalkDir::new(dir)
        .max_depth(6)
        .into_iter()
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            !SKIP.contains(&n.as_ref()) && (e.depth() == 0 || !n.starts_with('.'))
        })
        .flatten()
        .filter(|e| e.file_type().is_file() && is_program(&e.file_name().to_string_lossy()))
        .map(|e| {
            let len = e.metadata().map(|m| m.len()).unwrap_or(0);
            (e.into_path(), len)
        })
        .collect()
}

/// Choose the program that starts a fan or non-Steam game.
/// Returns the path inside the game folder and whether it is a Linux build.
pub fn pick_program(game_path: &Path, game_name: &str) -> Option<(String, Platform)> {
    if game_path.is_file() {
        let n = game_path.file_name()?.to_string_lossy().into_owned();
        return if is_linux_program(&n) {
            Some((n, Platform::Linux))
        } else if is_windows_program(&n) {
            Some((n, Platform::Windows))
        } else {
            None
        };
    }
    let all = programs_in(game_path);
    if all.is_empty() {
        return None;
    }
    let linux: Vec<_> = all.iter().filter(|(p, _)| is_linux_program(&p.to_string_lossy())).cloned().collect();
    let pool = if linux.is_empty() { all } else { linux.clone() }; // a native Linux build wins
    let gname = norm(game_name);
    let score = |(p, len): &(PathBuf, u64)| -> f64 {
        let rel = p.strip_prefix(game_path).unwrap_or(p);
        let depth = rel.components().count().saturating_sub(1) as f64;
        let stem = norm(&p.file_stem().unwrap_or_default().to_string_lossy());
        let mut s = 100.0 - 15.0 * depth;
        if !stem.is_empty() && !gname.is_empty() && (gname.contains(&stem) || stem.contains(&gname)) {
            s += 50.0;
        }
        if bad_words().is_match(&p.file_name().unwrap_or_default().to_string_lossy()) {
            s -= 80.0;
        }
        s + (*len as f64 / 1e6).min(50.0) / 5.0
    };
    let best = pool.iter().max_by(|a, b| score(a).total_cmp(&score(b)))?;
    let rel = best.0.strip_prefix(game_path).ok()?.to_string_lossy().into_owned();
    let plat = if is_linux_program(&rel) { Platform::Linux } else { Platform::Windows };
    Some((rel, plat))
}

/// Choose the file an emulator starts: the shallowest file with a matching type.
pub fn pick_rom(game_path: &Path, emu: &Emulator) -> Option<String> {
    let exts: Vec<String> = emu.extensions.iter().map(|e| e.trim_start_matches('.').to_lowercase()).collect();
    let matches = |p: &Path| {
        p.extension().map_or(false, |x| exts.contains(&x.to_string_lossy().to_lowercase()))
    };
    if game_path.is_file() {
        return matches(game_path).then(|| game_path.file_name().unwrap().to_string_lossy().into_owned());
    }
    let mut found: Vec<(usize, u64, PathBuf)> = walkdir::WalkDir::new(game_path)
        .max_depth(4)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file() && matches(e.path()))
        .map(|e| (e.depth(), e.metadata().map(|m| m.len()).unwrap_or(0), e.into_path()))
        .collect();
    // shallowest first, then the biggest (the game, not a small extra file)
    found.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let p = &found.first()?.2;
    Some(p.strip_prefix(game_path).ok()?.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------- scanning

/// What was found in one top folder, with the folder dates used to notice changes later.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupScan {
    /// Path of the top folder, relative to the shelf.
    pub path: String,
    pub mtime: i64,
    /// Subfolders that are groups themselves (name, date).
    pub subgroups: Vec<(String, i64)>,
    pub games: Vec<Game>,
}

impl GroupScan {
    pub fn unchanged(&self, shelf: &Path) -> bool {
        let p = shelf.join(&self.path);
        mtime(&p) == self.mtime && self.subgroups.iter().all(|(n, t)| mtime(&p.join(n)) == *t)
    }
}

fn blank(kind: Kind, system: &str, rel: String, name: String, group: String) -> Game {
    Game {
        id: Game::make_id(kind, system, &rel),
        kind,
        rel,
        system: system.to_string(),
        name,
        group,
        program: String::new(),
        platform: match kind {
            Kind::Rom => Platform::Emulator,
            Kind::Steam => Platform::Steam,
            _ => Platform::Windows,
        },
        appid: 0,
        buildid: String::new(),
        installdir: String::new(),
        size: 0,
    }
}

fn stem_name(file: &str) -> String {
    Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| file.to_string())
}

/// Read one group folder of `fangames/` or `nonsteam/`.
pub fn read_program_group(shelf: &Path, kind: Kind, group: &str) -> std::io::Result<GroupScan> {
    let gpath = shelf.join(kind.folder()).join(group);
    let mut games = Vec::new();
    let mut subgroups = Vec::new();
    for e in listing(&gpath)? {
        let epath = gpath.join(&e.name);
        if e.is_file {
            if is_program(&e.name) {
                games.push(blank(kind, "", format!("{group}/{}", e.name), stem_name(&e.name), group.into()));
            }
            continue;
        }
        if !e.is_dir {
            continue;
        }
        let items = listing(&epath)?;
        if is_game_folder(&epath, &items) {
            games.push(blank(kind, "", format!("{group}/{}", e.name), e.name.clone(), group.into()));
        } else {
            subgroups.push((e.name.clone(), mtime(&epath)));
            for g in items {
                if g.is_dir || (g.is_file && is_program(&g.name)) {
                    let name = if g.is_file { stem_name(&g.name) } else { g.name.clone() };
                    games.push(blank(kind, "", format!("{group}/{}/{}", e.name, g.name), name, e.name.clone()));
                }
            }
        }
    }
    Ok(GroupScan { path: format!("{}/{group}", kind.folder()), mtime: mtime(&gpath), subgroups, games })
}

/// Read one system folder of `roms/`.
pub fn read_rom_system(shelf: &Path, system: &str, emu: &Emulator) -> std::io::Result<GroupScan> {
    let sdir = shelf.join("roms").join(system);
    let root = rom_games_root(&sdir, Some(emu));
    let mut games = Vec::new();
    let exts: Vec<String> = emu.extensions.iter().map(|e| e.trim_start_matches('.').to_lowercase()).collect();
    for e in listing(&root)? {
        if emu.skip.iter().any(|s| !s.is_empty() && e.name.contains(s.as_str())) {
            continue;
        }
        let is_rom_file = e.is_file
            && Path::new(&e.name).extension().map_or(false, |x| exts.contains(&x.to_string_lossy().to_lowercase()));
        if e.is_dir || is_rom_file {
            let name = if e.is_file { stem_name(&e.name) } else { e.name.clone() };
            games.push(blank(Kind::Rom, system, e.name.clone(), name, system.to_string()));
        }
    }
    let rel_root = root.strip_prefix(shelf).unwrap_or(&root).to_string_lossy().into_owned();
    Ok(GroupScan { path: rel_root, mtime: mtime(&root), subgroups: vec![], games })
}

/// Read `steam/`: one manifest per archived game.
pub fn read_steam_archive(shelf: &Path) -> std::io::Result<GroupScan> {
    let root = shelf.join("steam");
    let mut games = Vec::new();
    for e in listing(&root)? {
        let Ok(appid) = e.name.parse::<u32>() else { continue };
        let acf = root.join(&e.name).join("steamapps").join(format!("appmanifest_{appid}.acf"));
        let Some(m) = crate::steam::read_manifest(&acf) else { continue };
        let mut g = blank(Kind::Steam, "", appid.to_string(), m.name.clone(), "Steam".into());
        g.appid = appid;
        g.buildid = m.buildid;
        g.installdir = m.installdir;
        g.size = m.size;
        games.push(g);
    }
    // per-game folder dates: a finished SteamCMD update rewrites the manifest folder
    let subgroups = games
        .iter()
        .map(|g| {
            let p = format!("{}/steamapps", g.appid);
            let t = mtime(&root.join(&p));
            (p, t)
        })
        .collect();
    Ok(GroupScan { path: "steam".into(), mtime: mtime(&root), subgroups, games })
}

/// Systems in `roms/` that have no emulator in the settings.
pub fn unknown_systems(cfg: &Config) -> Vec<String> {
    listing(&cfg.shelf().join("roms"))
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.is_dir && cfg.emulator(&e.name).is_none())
        .map(|e| e.name)
        .collect()
}

/// Every top folder that is read separately: (path relative to shelf, how to read it).
pub enum Source {
    Group(Kind, String),
    System(String),
    SteamArchive,
}

impl Source {
    pub fn key(&self) -> String {
        match self {
            Source::Group(k, g) => format!("{}/{g}", k.folder()),
            Source::System(s) => format!("roms/{s}"),
            Source::SteamArchive => "steam".into(),
        }
    }
}

pub fn sources(cfg: &Config) -> std::io::Result<Vec<Source>> {
    let shelf = cfg.shelf();
    let mut out = Vec::new();
    for kind in [Kind::Fan, Kind::NonSteam] {
        if let Ok(l) = listing(&shelf.join(kind.folder())) {
            out.extend(l.into_iter().filter(|e| e.is_dir).map(|e| Source::Group(kind, e.name)));
        }
    }
    if let Ok(l) = listing(&shelf.join("roms")) {
        out.extend(l.into_iter().filter(|e| e.is_dir && cfg.emulator(&e.name).is_some()).map(|e| Source::System(e.name)));
    }
    if shelf.join("steam").is_dir() {
        out.push(Source::SteamArchive);
    }
    if out.is_empty() && !crate::config::shelf_online(&shelf) {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "the shelf is empty or offline"));
    }
    Ok(out)
}

pub fn read_source(cfg: &Config, src: &Source) -> std::io::Result<GroupScan> {
    let shelf = cfg.shelf();
    match src {
        Source::Group(k, g) => read_program_group(&shelf, *k, g),
        Source::System(s) => read_rom_system(&shelf, s, cfg.emulator(s).expect("filtered in sources()")),
        Source::SteamArchive => read_steam_archive(&shelf),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};

    fn touch(p: &Path) {
        create_dir_all(p.parent().unwrap()).unwrap();
        write(p, b"x").unwrap();
    }

    #[test]
    fn layout_rules() {
        let t = tempfile::tempdir().unwrap();
        let s = t.path();
        let f = s.join("fangames");
        touch(&f.join("Super Mario Bros/Super Mario 63/sm63game.exe"));
        touch(&f.join("Sonic The Hedgehog/Sonic Classic/bin/sonic.exe"));
        touch(&f.join("Others/Pack/A/a.exe"));
        touch(&f.join("Others/Pack/B/b.exe"));
        touch(&f.join("Others/Solo.exe"));
        touch(&f.join("Others/readme.txt"));
        touch(&f.join("Phantasy Star/super-phantasy-fighters-windows/UnityCrashHandler64.exe"));
        touch(&f.join("Phantasy Star/super-phantasy-fighters-windows/SPF.exe"));
        let mut all = vec![];
        for g in ["Super Mario Bros", "Sonic The Hedgehog", "Others", "Phantasy Star"] {
            all.extend(read_program_group(s, Kind::Fan, g).unwrap().games);
        }
        let ids: Vec<_> = all.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "fan:Super Mario Bros/Super Mario 63",
                "fan:Sonic The Hedgehog/Sonic Classic",
                "fan:Others/Pack/A",
                "fan:Others/Pack/B",
                "fan:Others/Solo.exe",
                "fan:Phantasy Star/super-phantasy-fighters-windows",
            ]
        );
        assert_eq!(all[2].group, "Pack");
        assert_eq!(
            pick_program(&f.join("Sonic The Hedgehog/Sonic Classic"), "Sonic Classic").unwrap().0,
            "bin/sonic.exe"
        );
        assert_eq!(
            pick_program(&f.join("Phantasy Star/super-phantasy-fighters-windows"), "x").unwrap().0,
            "SPF.exe"
        );
    }

    #[test]
    fn roms_and_archive() {
        let t = tempfile::tempdir().unwrap();
        let s = t.path();
        touch(&s.join("roms/wii/GAMES/Super Mario Galaxy [VBKP]/game.iso"));
        touch(&s.join("roms/wii/BIN/Dolphin.exe"));
        touch(&s.join("roms/WIIU/GAMES/SUPER MARIO 3D WORLD [ARDP]/code/RedCarpet.rpx"));
        touch(&s.join("roms/WIIU/GAMES/MH3U (UPDATE DATA) (v32)/code/x.rpx"));
        let cfg = Config { shelf: s.to_string_lossy().into(), ..Default::default() };
        let wii = read_rom_system(s, "wii", cfg.emulator("wii").unwrap()).unwrap();
        assert_eq!(wii.games.len(), 1);
        assert_eq!(wii.games[0].id, "rom:wii/Super Mario Galaxy [VBKP]");
        let wiiu = read_rom_system(s, "WIIU", cfg.emulator("WIIU").unwrap()).unwrap();
        assert_eq!(wiiu.games.len(), 1, "update data is skipped");
        let g = &wiiu.games[0];
        assert_eq!(pick_rom(&g.shelf_path(&cfg), cfg.emulator("wiiu").unwrap()).unwrap(), "code/RedCarpet.rpx");

        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"620\"\n\t\"name\"\t\t\"Portal 2\"\n\t\"installdir\"\t\t\"Portal 2\"\n\t\"buildid\"\t\t\"42\"\n\t\"SizeOnDisk\"\t\t\"1000\"\n}\n";
        create_dir_all(s.join("steam/620/steamapps")).unwrap();
        write(s.join("steam/620/steamapps/appmanifest_620.acf"), acf).unwrap();
        let st = read_steam_archive(s).unwrap();
        assert_eq!(st.games[0].name, "Portal 2");
        assert_eq!(st.games[0].buildid, "42");
        assert!(st.unchanged(s));
    }
}
