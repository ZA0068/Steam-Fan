//! Steam-Fan: the NAS is the shelf, this PC is the box, Steam is the launcher.
//!
//! `steam-fan` with no arguments opens the app. Steam uses `steam-fan launch`
//! when you press Play; the menu entry for Steam uses `steam-fan steam` so new
//! games from the shelf appear every time Steam starts.

mod cli;
mod gui;
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
        None => gui::run(),
        Some(Cmd::Launch { id, command }) => launch_window::launch(&id, &command),
        Some(Cmd::Sync { dry_run, only, rescan }) => cli::sync(dry_run, &only, rescan),
        Some(Cmd::Steam { args }) => cli::steam(&args),
        Some(Cmd::List { installed }) => cli::list(installed),
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
