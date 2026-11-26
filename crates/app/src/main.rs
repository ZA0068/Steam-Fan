//! Steam-Fan: the NAS is the shelf, this PC is the box, Steam is the launcher.
//!
//! `steam-fan` with no arguments opens the library window (`steam-fan-app`). Steam uses `steam-fan launch`
//! when you press Play; the menu entry for Steam uses `steam-fan steam` so new
//! games from the shelf appear every time Steam starts.

mod cli;
mod launch_window;
mod util;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "steam-fan", version, about = "Your NAS is the shelf, your PC is the box, Steam is the launcher.")]
struct Args {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Used by Steam when you press Play (set up automatically in each game's launch options).
    Launch {
        id: String,
        /// What Steam passes as %command%.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Read the shelf and update Steam's library (Steam must be closed).
    Sync {
        #[arg(long)]
        dry_run: bool,
        /// Only games whose name or path contains this text.
        #[arg(long)]
        only: Vec<String>,
        /// Read every folder again and pick each game's program again.
        #[arg(long)]
        rescan: bool,
    },
    /// Update Steam's library if Steam is closed and the shelf is reachable, then start Steam.
    Steam {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Add a game to the shelf from a download (zip, 7z, rar), a folder or a ROM file.
    Add {
        source: String,
        /// fan, nonsteam or rom
        #[arg(long, default_value = "")]
        kind: String,
        /// Group (Sonic The Hedgehog, Kirby, ...) or, for emulator games, the system (wii, WIIU, ...)
        #[arg(long)]
        group: String,
        /// Name on the shelf (default: the file or folder name)
        #[arg(long)]
        name: Option<String>,
        /// Keep it as one of several versions: <group>/<name>/<version>
        #[arg(long, default_value = "")]
        version: String,
    },
    /// Copy artwork from the shelf, fetch missing covers from SteamGridDB, update Steam's artwork.
    Art {
        /// Look up games on SteamGridDB again, also those where nothing was found before.
        #[arg(long)]
        again: bool,
    },
    /// Check a game's files against its checksums (records them the first time) and repair the PC copy.
    Verify { game: String },
    /// Download a game again from its link (About > Download): replace the shelf copy, or add it as a version.
    Redownload {
        game: String,
        #[arg(long)]
        version: Option<String>,
        /// Use this downloaded file (zip, 7z, rar, folder) instead of the link
        #[arg(long)]
        file: Option<String>,
    },
    /// List the games (from the last shelf read).
    List {
        /// Only games on this PC.
        #[arg(long)]
        installed: bool,
    },
    /// Copy a game from the shelf to this PC.
    Install { game: String },
    /// Back up a game's saves to the shelf, then delete it from this PC.
    Remove { game: String },
    /// Steam games archived on the shelf.
    Archive {
        #[command(subcommand)]
        cmd: ArchiveCmd,
    },
}

#[derive(Subcommand)]
enum ArchiveCmd {
    /// Archived games and whether the shelf or this PC has the newer version.
    List,
    /// Install or update a game from the shelf into Steam (for when Steam's servers are down).
    Restore { appid: u32 },
    /// Copy a game from this PC to the shelf (when this PC has the newer version).
    Upload { appid: u32 },
}

fn main() {
    let args = Args::parse();
    let code = match args.cmd {
        None => cli::open_window(),
        Some(Cmd::Launch { id, command }) => launch_window::launch(&id, &command),
        Some(Cmd::Sync { dry_run, only, rescan }) => cli::sync(dry_run, &only, rescan),
        Some(Cmd::Steam { args }) => cli::steam(&args),
        Some(Cmd::List { installed }) => cli::list(installed),
        Some(Cmd::Add { source, kind, group, name, version }) => cli::add(&source, &kind, &group, name.as_deref(), &version),
        Some(Cmd::Art { again }) => cli::art(again),
        Some(Cmd::Verify { game }) => cli::verify(&game),
        Some(Cmd::Redownload { game, version, file }) => cli::redownload(&game, version.as_deref(), file.as_deref()),
        Some(Cmd::Install { game }) => cli::install(&game),
        Some(Cmd::Remove { game }) => cli::remove(&game),
        Some(Cmd::Archive { cmd }) => match cmd {
            ArchiveCmd::List => cli::archive_list(),
            ArchiveCmd::Restore { appid } => cli::archive_restore(appid),
            ArchiveCmd::Upload { appid } => cli::archive_upload(appid),
        },
    };
    std::process::exit(code);
}
