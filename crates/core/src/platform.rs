//! What differs between Linux, Windows and macOS, in one place: where Steam lives, whether it
//! runs, quitting and starting it, which programs run, opening a folder, notifications, and how
//! a game starts when Steam doesn't hand over a command (Windows, macOS).
//!
//! | | Linux | Windows | macOS |
//! |---|---|---|---|
//! | Steam | `~/.local/share/Steam` | registry `SteamPath` | `~/Library/Application Support/Steam` |
//! | Windows games | Proton, through Steam (`%command%`) | run directly | Wine or CrossOver |
//! | Steam entry runs | the game, wrapped by `steam-fan launch` | `steam-fan launch` | `steam-fan launch` |

use crate::config::{expand, Config};
use crate::shelf::Platform;
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const LINUX: bool = cfg!(target_os = "linux");
pub const WINDOWS: bool = cfg!(target_os = "windows");
pub const MACOS: bool = cfg!(target_os = "macos");

/// A program's file name on this system: `steam-fan` or `steam-fan.exe`.
pub fn exe_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// Where Steam is installed, as the settings' default.
pub fn default_steam_root() -> String {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        if let Ok(k) = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey("Software\\Valve\\Steam") {
            if let Ok(p) = k.get_value::<String, _>("SteamPath") {
                return p.replace('/', "\\");
            }
        }
        return "C:\\Program Files (x86)\\Steam".into();
    }
    #[cfg(target_os = "macos")]
    {
        return "~/Library/Application Support/Steam".into();
    }
    #[allow(unreachable_code)]
    "~/.local/share/Steam".into()
}

/// A running program.
pub struct Proc {
    pub pid: u32,
    pub name: String,
    pub cwd: Option<PathBuf>,
    pub cmd: String,
}

/// The programs running right now.
pub fn processes() -> Vec<Proc> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always).with_cmd(UpdateKind::Always),
    );
    sys.processes()
        .iter()
        .map(|(pid, p)| Proc {
            pid: pid.as_u32(),
            name: p.name().to_string_lossy().into_owned(),
            cwd: p.cwd().map(Path::to_path_buf),
            cmd: p.cmd().iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" "),
        })
        .collect()
}

/// Is the Steam app running?
pub fn steam_running() -> bool {
    processes().iter().any(|p| {
        let n = p.name.to_lowercase();
        matches!(n.as_str(), "steam" | "steam.exe" | "steamwebhelper" | "steamwebhelper.exe" | "steam_osx")
    })
}

/// Is a program running from (or with a file in) this folder? Shells and Steam-Fan itself don't count.
pub fn running_in(dir: &Path) -> bool {
    let me = std::process::id();
    let plain = dir.to_string_lossy().into_owned();
    // a Windows game under Proton sees the folder as Z:\home\...
    let wine = format!("Z:{}", plain.replace('/', "\\")).to_lowercase();
    processes().iter().filter(|p| p.pid != me).any(|p| {
        let n = p.name.to_lowercase();
        let n = n.trim_end_matches(".exe");
        if matches!(n, "bash" | "zsh" | "sh" | "fish" | "dash" | "python3" | "steam-fan" | "steam-fan-app" | "cmd" | "powershell" | "explorer") {
            return false;
        }
        p.cwd.as_deref().map_or(false, |c| c.starts_with(dir)) || p.cmd.contains(&plain) || (LINUX && p.cmd.to_lowercase().contains(&wine))
    })
}

fn steam_exe(cfg: &Config) -> PathBuf {
    cfg.steam_root().join("steam.exe")
}

/// Ask Steam to quit.
pub fn quit_steam(cfg: &Config) {
    let _ = cfg;
    if WINDOWS {
        let _ = Command::new(steam_exe(cfg)).arg("-shutdown").spawn();
    } else if MACOS {
        let _ = Command::new("osascript").args(["-e", "quit app \"Steam\""]).spawn();
    } else {
        let _ = Command::new("steam").arg("-shutdown").spawn();
    }
}

/// Start Steam, detached from Steam-Fan.
pub fn start_steam(cfg: &Config) {
    if WINDOWS {
        let _ = Command::new(steam_exe(cfg)).spawn();
    } else if MACOS {
        let _ = Command::new("open").args(["-a", "Steam"]).spawn();
    } else if Command::new("setsid").args(["-f", "steam"]).spawn().is_err() {
        let _ = Command::new("steam").spawn();
    }
}

/// Open a folder or web address with what the system uses for it.
pub fn open(target: &str) {
    let prog = if WINDOWS { "explorer" } else if MACOS { "open" } else { "xdg-open" };
    let _ = Command::new(prog).arg(target).spawn();
}

/// A desktop notification (best effort; nothing on Windows yet).
pub fn notify(title: &str, body: &str) {
    use std::process::Stdio;
    if MACOS {
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("display notification \"{}\" with title \"{}\"", esc(body), esc(title));
        let _ = Command::new("osascript").args(["-e", &script]).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    } else if LINUX {
        let _ = Command::new("notify-send")
            .args(["-a", "Steam-Fan", "-i", "steam", title, body])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

/// Wine on this Mac: CrossOver if it's installed (in a "Steam-Fan" bottle), else `wine`
/// (Homebrew's wine-stable or Gcenx's builds). The settings can name another.
pub fn wine(cfg: &Config) -> Vec<String> {
    if !cfg.wine.is_empty() {
        return cfg.wine.iter().map(|a| expand(a).to_string_lossy().into_owned()).collect();
    }
    let crossover = "/Applications/CrossOver.app/Contents/SharedSupport/CrossOver/bin/wine";
    if MACOS && Path::new(crossover).exists() {
        return vec![crossover.into(), "--bottle".into(), "Steam-Fan".into()];
    }
    vec!["wine".into()]
}

/// The command that starts a game's program when Steam doesn't pass one: on Windows the
/// program itself, on macOS a Windows program through Wine. On Linux Steam passes Proton's
/// command (`%command%`), unless the settings name a Wine to use instead.
pub fn game_command(cfg: &Config, exe: &Path, platform: Platform) -> Result<Vec<String>> {
    let exe_s = exe.to_string_lossy().into_owned();
    match platform {
        Platform::Windows if WINDOWS => Ok(vec![exe_s]),
        Platform::Windows if MACOS || !cfg.wine.is_empty() => {
            let mut c = wine(cfg);
            c.push(exe_s);
            Ok(c)
        }
        Platform::Windows => bail!(
            "on Linux, Windows games start through Steam with Proton: press Play in Steam \
             (or name a Wine in Steam-Fan's settings to start them without Steam)"
        ),
        Platform::Linux if LINUX => Ok(vec![exe_s]),
        Platform::Linux => bail!("this is a Linux game; it doesn't run on this computer"),
        _ => Ok(vec![exe_s]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_system() {
        assert!(LINUX || WINDOWS || MACOS);
        assert!(exe_name("steam-fan").starts_with("steam-fan"));
        assert!(!default_steam_root().is_empty());
        // this test program itself is running
        assert!(processes().iter().any(|p| p.pid == std::process::id()));
    }

    #[test]
    fn commands_per_system() {
        let cfg = Config::default();
        let exe = Path::new("/games/Sonic/sonic.exe");
        let c = game_command(&cfg, exe, Platform::Windows);
        if LINUX {
            assert!(c.is_err(), "Linux uses Proton through Steam");
            let mut w = Config::default();
            w.wine = vec!["wine64".into()];
            assert_eq!(game_command(&w, exe, Platform::Windows).unwrap(), ["wine64", "/games/Sonic/sonic.exe"]);
        } else if MACOS {
            assert_eq!(c.unwrap().last().unwrap(), "/games/Sonic/sonic.exe");
        } else {
            assert_eq!(c.unwrap(), ["/games/Sonic/sonic.exe"]);
        }
    }
}
