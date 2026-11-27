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
    /// Which version, when a game is kept in several versions side by side
    /// (`Sonic of the Ring/V 0.6`). Empty for most games.
    #[serde(default)]
    pub version: String,
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
    /// When the program file was built (its date, Unix seconds): tells which version is newest.
    #[serde(default)]
    pub built: i64,
}

impl Game {
    /// The name with its version: `Sonic of the Ring (V 0.6)`.
    pub fn full_name(&self) -> String {
        if self.version.is_empty() { self.name.clone() } else { format!("{} ({})", self.name, self.version_label()) }
    }

    /// The version without the game's name in front: `Sonic World Release 10` in
    /// `Sonic World` is `Release 10`, `SurgeEngineDemo_v1.3` in `Sonic - Surge Engine` is `Demo v1.3`.
    pub fn version_label(&self) -> String {
        version_label(&self.name, &self.version)
    }

    /// Where the game's own folder is, relative to its kind's folder: for a version,
    /// the folder that holds all its versions. Artwork, info and the Steam entry belong to it.
    pub fn title_rel(&self) -> String {
        if self.version.is_empty() {
            self.rel.clone()
        } else {
            self.rel.rsplit_once('/').map_or(self.rel.clone(), |(p, _)| p.to_string())
        }
    }

    /// The id of the library entry: the same for every version of a game.
    pub fn title_id(&self) -> String {
        if self.version.is_empty() {
            self.id.clone()
        } else {
            Game::make_id(self.kind, &self.system, &self.title_rel())
        }
    }

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
            if (name.starts_with('.') && name != VERSIONS_MARKER && name != COLLECTION_MARKER) || SKIP.contains(&name.as_str()) {
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
    // Unreal games: Binaries/ + Engine/ + <Name>Game/ (UDK), or Engine/ + <Name>/Binaries/ (UE4)
    let has_dir = |n: &str| items.iter().any(|e| e.is_dir && e.name.eq_ignore_ascii_case(n));
    if has_dir("Engine") && (has_dir("Binaries") || items.iter().any(|e| e.is_dir && e.name.to_lowercase().ends_with("game") && e.name.len() > 4)) {
        return true;
    }
    let with_prog: Vec<&Entry> =
        items.iter().filter(|e| e.is_dir && has_top_program(&path.join(&e.name))).collect();
    if with_prog.iter().all(|e| BUILD_DIRS.contains(&e.name.to_lowercase().as_str())) {
        return true;
    }
    with_prog.len() < 2
}

/// A folder or file name that Windows file sharing (SMB) can pass on as it is. A name with
/// `:` shows up on the PC as an empty folder plus a garbled copy (`S6G8TJ~9`), so
/// `Sonic: Codename Backstreet` becomes `Sonic - Codename Backstreet`.
pub fn safe_name(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            ':' => out.push_str(" -"),
            '*' | '?' | '"' | '<' | '>' | '|' | '\\' => out.push(' '),
            c if (c as u32) < 32 => {}
            c => out.push(c),
        }
    }
    // no doubled spaces, no trailing dots or spaces (also lost over SMB)
    let mut squeezed = String::new();
    for w in out.split(' ').filter(|w| !w.is_empty()) {
        if !squeezed.is_empty() {
            squeezed.push(' ');
        }
        squeezed.push_str(w);
    }
    squeezed.trim_end_matches(['.', ' ']).to_string()
}

/// A folder that holds versions of one game, even if it has only one so far
/// (written when a version is added in the app).
pub const VERSIONS_MARKER: &str = ".steam-fan-versions";

/// A folder that holds different games (Infinity Engine games, Sonic Adventure fan games...):
/// each one is its own library entry, the folder is shown as their group.
/// Without it, a folder holding several games is taken as one game in several versions.
pub const COLLECTION_MARKER: &str = ".steam-fan-collection";

/// Folders inside a game folder that are parts of a game, never a version of it.
const NOT_VERSIONS: [&str; 10] =
    ["engine", "binaries", "content", "redist", "_commonredist", "directx", "plugins", "__macosx", "prerequisites", "support"];

/// A folder of earlier builds, kept for the record: never the default version.
pub fn is_old_version(name: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)(^|[^a-z])old([^a-z]|$)|april\s*fool|media\s*kit").unwrap()).is_match(name)
}

/// Natural order key: `Release 10` after `Release 9`.
fn natural(s: &str) -> Vec<(u64, String)> {
    let mut out = vec![];
    let mut num = String::new();
    let mut txt = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_digit() {
            if !txt.is_empty() {
                out.push((0, std::mem::take(&mut txt)));
            }
            num.push(c);
        } else {
            if !num.is_empty() {
                out.push((num.parse::<u64>().unwrap_or(u64::MAX).saturating_add(1), String::new()));
                num.clear();
            }
            txt.push(c);
        }
    }
    if !num.is_empty() {
        out.push((num.parse::<u64>().unwrap_or(u64::MAX).saturating_add(1), String::new()));
    }
    if !txt.is_empty() {
        out.push((0, txt));
    }
    out
}

/// The numbers in a version name: `V 0.6` is [0, 6], `0.2.5` is [0, 2, 5].
fn numbers(s: &str) -> VerNum {
    VerNum(s.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).map(String::from).collect())
}

/// Version numbers in order. A part with a leading zero is a decimal: `1.06` comes before `1.3`.
#[derive(PartialEq, Eq)]
struct VerNum(Vec<String>);

impl VerNum {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Ord for VerNum {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        for (a, b) in self.0.iter().zip(&other.0) {
            let decimal = (a.len() > 1 && a.starts_with('0')) || (b.len() > 1 && b.starts_with('0'));
            let o = if decimal {
                // as fractions: "06" vs "3" is .06 vs .30
                let w = a.len().max(b.len());
                format!("{a:0<w$}").cmp(&format!("{b:0<w$}"))
            } else {
                let (x, y) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
                x.len().cmp(&y.len()).then(x.cmp(y))
            };
            if o.is_ne() {
                return o;
            }
        }
        self.0.len().cmp(&other.0.len())
    }
}

impl PartialOrd for VerNum {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// A version name without its numbers (and a lone `v`): `V 0.6` and `0.2.5` are both "".
fn stem(s: &str) -> String {
    s.to_lowercase().split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty() && *w != "v").collect::<Vec<_>>().join(" ")
}

/// Versions of one game, newest first. An "Old" folder always comes last.
/// When every version is named alike apart from its numbers (`0.2.5`, `V 0.5`, `V 0.6`),
/// the numbers decide. Otherwise (`Release 10`, `DX 1.2.4`) the newest program date
/// decides; names break ties.
pub fn sort_versions(v: &mut [Game]) {
    let current: Vec<&Game> = v.iter().filter(|g| !is_old_version(&g.version)).collect();
    let by_number = current.len() > 1
        && current.iter().all(|g| !numbers(&g.version).is_empty())
        && current.iter().all(|g| stem(&g.version) == stem(&current[0].version));
    v.sort_by(|a, b| {
        let key = |g: &Game| {
            let num = if by_number { numbers(&g.version) } else { VerNum(vec![]) };
            (!is_old_version(&g.version), num, g.built, natural(&g.version))
        };
        key(b).cmp(&key(a))
    });
}

/// `version` without `name` in front (see `Game::version_label`).
pub fn version_label(name: &str, version: &str) -> String {
    let squash = |s: &str| s.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>();
    let v = version.trim();
    let mut prefixes = vec![squash(name)];
    // "Sonic - Surge Engine" holds "SurgeEngineDemo_v1.3"
    for lead in ["sonicthehedgehog", "sonic"] {
        if let Some(rest) = prefixes[0].strip_prefix(lead) {
            if rest.len() >= 3 {
                prefixes.push(rest.to_string());
            }
        }
    }
    for p in prefixes.iter().filter(|p| !p.is_empty()) {
        // walk the original, matching the squashed prefix
        let mut it = p.chars().peekable();
        let mut cut = None;
        for (i, c) in v.char_indices() {
            if it.peek().is_none() {
                cut = Some(i);
                break;
            }
            if c.is_ascii_alphanumeric() {
                if c.to_ascii_lowercase() == *it.peek().unwrap() {
                    it.next();
                } else {
                    break;
                }
            }
        }
        if cut.is_none() && it.peek().is_none() {
            cut = Some(v.len());
        }
        let Some(i) = cut else { continue };
        // only at a word's end: "Sonic Worlds" doesn't start with "Sonic World "
        let next = v[i..].chars().next();
        if !next.map_or(true, |c| !c.is_alphanumeric() || c.is_ascii_uppercase() || c.is_ascii_digit()) {
            continue;
        }
        let rest = v[i..].trim_start_matches(|c: char| " -_:.'".contains(c)).replace('_', " ");
        // "Sonic Omens (3.2.0)" is "3.2.0"
        let rest = rest.trim();
        let rest = rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')).unwrap_or(rest).trim();
        if !rest.is_empty() {
            let mut c = rest.chars();
            let first = c.next().unwrap();
            return first.to_uppercase().chain(c).collect();
        }
    }
    v.replace('_', " ")
}

pub fn is_version_name(n: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)^((v|ver|version)\s*\.?\s*)?\d+([._]\d+)*\s*[a-z]?$|^(alpha|beta|demo|build|release|rc|update|patch|final)\b|\b(alpha|beta|build|release|update|patch)\s*\d")
            .unwrap()
    })
    .is_match(n.trim())
}

pub fn holds_versions(items: &[Entry]) -> bool {
    if items.iter().any(|e| e.is_file && e.name == VERSIONS_MARKER) {
        return true;
    }
    let dirs: Vec<&Entry> = items.iter().filter(|e| e.is_dir).collect();
    dirs.len() >= 2
        && dirs.iter().all(|d| is_version_name(&d.name))
        && !items.iter().any(|e| e.is_file && is_program(&e.name))
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
        version: String::new(),
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
        built: 0,
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
        let marked = |m: &str| items.iter().any(|x| x.is_file && x.name == m);
        let versions = marked(VERSIONS_MARKER) || (!marked(COLLECTION_MARKER) && !is_game_folder(&epath, &items));
        if versions {
            // Sonic World/{Release 1 ... Release 10, DX 1.2.4}: one game, several versions
            subgroups.push((e.name.clone(), mtime(&epath)));
            for v in items.iter().filter(|v| v.is_dir && !NOT_VERSIONS.contains(&v.name.to_lowercase().as_str())) {
                let mut g = blank(kind, "", format!("{group}/{}/{}", e.name, v.name), e.name.clone(), group.into());
                g.version = v.name.clone();
                games.push(g);
            }
        } else if is_game_folder(&epath, &items) {
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
        touch(&f.join("Sonic The Hedgehog/Sonic of the Ring/0.2.5/sotr.exe"));
        touch(&f.join("Sonic The Hedgehog/Sonic of the Ring/V 0.6/sotr.exe"));
        touch(&f.join("Sonic The Hedgehog/Sonic Network Land v96/Sonic Network Land.exe"));
        touch(&f.join("Sonic The Hedgehog/Sonic Worlds/.steam-fan-versions"));
        touch(&f.join("Sonic The Hedgehog/Sonic Worlds/Delta/sw.exe"));
        // a folder of different games, marked
        touch(&f.join("Others/Infinity/.steam-fan-collection"));
        touch(&f.join("Others/Infinity/Omens/o.exe"));
        touch(&f.join("Others/Infinity/Arcadia/a.exe"));
        // an Unreal (UDK) game is one game, not four
        touch(&f.join("Sonic The Hedgehog/Sonic Souls/Binaries/Win32/UDK.exe"));
        touch(&f.join("Sonic The Hedgehog/Sonic Souls/Engine/Config/x.ini"));
        touch(&f.join("Sonic The Hedgehog/Sonic Souls/UDKGame/Content/y.upk"));
        touch(&f.join("Sonic The Hedgehog/Sonic Souls/Install Sonic Souls/setup.exe"));
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
                "fan:Sonic The Hedgehog/Sonic Network Land v96",
                "fan:Sonic The Hedgehog/Sonic of the Ring/0.2.5",
                "fan:Sonic The Hedgehog/Sonic of the Ring/V 0.6",
                "fan:Sonic The Hedgehog/Sonic Souls",
                "fan:Sonic The Hedgehog/Sonic Worlds/Delta",
                "fan:Others/Infinity/Arcadia",
                "fan:Others/Infinity/Omens",
                "fan:Others/Pack/A",
                "fan:Others/Pack/B",
                "fan:Others/Solo.exe",
                "fan:Phantasy Star/super-phantasy-fighters-windows",
            ]
        );
        let sotr = &all[3];
        assert_eq!((sotr.name.as_str(), sotr.version.as_str(), sotr.group.as_str()), ("Sonic of the Ring", "0.2.5", "Sonic The Hedgehog"));
        assert_eq!(all[4].full_name(), "Sonic of the Ring (V 0.6)");
        assert_eq!(all[6].version, "Delta", "marker file: versions even with one");
        assert_eq!((all[7].group.as_str(), all[7].version.as_str()), ("Infinity", ""), "a collection: separate games");
        let pack = &all[9];
        assert_eq!((pack.name.as_str(), pack.version.as_str(), pack.group.as_str()), ("Pack", "A", "Others"), "a folder of builds: one game in versions");
        assert_eq!((pack.title_id().as_str(), pack.title_rel().as_str()), ("fan:Others/Pack", "Others/Pack"));
        for v in ["0.2.5", "V 0.6", "v96", "Beta 2", "1.4.2b"] {
            assert!(is_version_name(v), "{v}");
        }
        for v in ["Sonic Classic", "Pack", "Delta"] {
            assert!(!is_version_name(v), "{v}");
        }
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

    #[test]
    fn version_names_and_order() {
        let label = |n: &str, v: &str| version_label(n, v);
        assert_eq!(label("Sonic World", "Sonic World Release 10"), "Release 10");
        assert_eq!(label("Sonic World", "Sonic World DX 1.2.4"), "DX 1.2.4");
        assert_eq!(label("Sonic - Surge Engine", "SurgeEngineDemo_v1.3"), "Demo v1.3");
        assert_eq!(label("Sonic Omens", "Sonic Omens (3.2.0)"), "3.2.0");
        assert_eq!(label("Sonic Before The Sequel", "Sonic Before the Sequel plus"), "Plus");
        assert_eq!(label("Sonic of the Ring", "V 0.6"), "V 0.6");
        assert_eq!(label("Sonic Boll (Series)", "Sonic Bolls 2"), "Sonic Bolls 2", "not cut in the middle of a word");
        assert_eq!(label("Sonic Momentum", "Sonic_Momentum_Beta_Edition_m1_8"), "Beta Edition m1 8");
        assert!(is_old_version("Old") && is_old_version("Sonic Eclipse (OLD)") && is_old_version("Poker Boll (April Fools)"));
        assert!(!is_old_version("Golden") && !is_old_version("Bolder"));

        let v = |name: &str, built: i64| {
            let mut g = blank(Kind::Fan, "", format!("S/W/{name}"), "W".into(), "S".into());
            g.version = name.into();
            g.built = built;
            g
        };
        let mut vs = vec![v("Release 9", 100), v("Old", 900), v("Release 10", 200), v("DX 1.2.4", 300)];
        sort_versions(&mut vs);
        let order: Vec<_> = vs.iter().map(|g| g.version.as_str()).collect();
        assert_eq!(order, ["DX 1.2.4", "Release 10", "Release 9", "Old"], "newest program first, an Old folder last");
        // Sonic of the Ring: numbered alike, so the numbers decide, not a re-unpacked copy's date
        let mut vs = vec![v("V0.2.5", 1_795_000_000), v("0.2.5", 1_537_000_000), v("V 0.6", 1_765_000_000), v("V 0.5", 1_586_000_000)];
        sort_versions(&mut vs);
        let order: Vec<_> = vs.iter().map(|g| g.version.as_str()).collect();
        assert_eq!(order, ["V 0.6", "V 0.5", "V0.2.5", "0.2.5"]);
        // Surge Engine: v1.3 after v1.06
        let mut vs = vec![v("SurgeEngineDemo_v1.06", 5), v("SurgeEngineDemo_v1.3", 4), v("SurgeEngineDemo_v1.01", 3)];
        sort_versions(&mut vs);
        let order: Vec<_> = vs.iter().map(|g| g.version.as_str()).collect();
        assert_eq!(order, ["SurgeEngineDemo_v1.3", "SurgeEngineDemo_v1.06", "SurgeEngineDemo_v1.01"], "1.06 is before 1.3");
        // Sonic World releases: 10 after 9
        let mut vs = vec![v("Sonic World Release 9", 9), v("Sonic World Release 10", 1), v("Sonic World Release 2", 5)];
        sort_versions(&mut vs);
        assert_eq!(vs[0].version, "Sonic World Release 10");
        // no dates known yet: by name, numbers counted as numbers
        let mut vs = vec![v("Release 9", 0), v("Release 10", 0), v("Release 2", 0)];
        sort_versions(&mut vs);
        assert_eq!(vs[0].version, "Release 10");
    }

    #[test]
    fn names_the_share_can_carry() {
        assert_eq!(safe_name("Sonic: Codename Backstreet"), "Sonic - Codename Backstreet");
        assert_eq!(safe_name("Sonic 3: SMS"), "Sonic 3 - SMS");
        assert_eq!(safe_name("Sonic - Exe finally snaps?"), "Sonic - Exe finally snaps");
        assert_eq!(safe_name("Demo v1.0. "), "Demo v1.0");
        assert_eq!(safe_name("Sonic World"), "Sonic World");
    }
}
