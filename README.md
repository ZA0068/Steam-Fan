# Steam-Fan

**Your NAS is the shelf, your PC is the box, Steam is the launcher.**

Every game you have is kept on the shelf: fan games, other non-Steam games,
emulator games, and an offline archive of your Steam games. Steam on your PC
shows all of them. Press Play and the game is copied from the shelf to the PC
first, if it isn't there yet. When you quit, its saves go back to the shelf.
Remove it from the PC whenever you need the space: it stays in Steam and copies
again next time.

Steam games normally download from Steam. When Steam's servers are down, the
same game can be installed from the shelf instead. Then Steam starts it in
offline mode, and updates it as usual once Steam is back. The NAS keeps that
archive up to date by itself every day.

## Install

```bash
cd ~/Projects/Steam-Fan
./install.sh
```

This does four things:

- builds the app;
- puts `steam-fan` in `~/.local/bin`;
- adds **Steam-Fan** to the app menu;
- makes the **Steam** menu entry start Steam through Steam-Fan, so new shelf games show up every time Steam starts.

It also moves your existing Steam entries over from the old scripts and then removes those scripts. Copies are kept in `legacy/`.

You need Rust, which is already on your PC (`~/.cargo`).

## Using it

Open **Steam-Fan** from the app menu.

| Button | What it does |
|---|---|
| Read shelf | Looks for new or changed games on the shelf. It only re-reads folders that changed, so it's fast. |
| Update Steam library | Adds new games to Steam, updates changed ones, removes ones that left the shelf. Quit Steam first. |
| Add game to shelf | Copies a game folder from this PC onto the shelf, as a fan game, a non-Steam game or an emulator game. |
| Copy to this PC / Remove from PC | The same thing Play and the old right-click menu did. Saves are backed up before anything is deleted. |
| Install from shelf | For Steam games: installs or updates the game in Steam from the shelf archive (when Steam is offline). |
| Copy to shelf | For Steam games: if your PC has a newer version than the shelf, puts it on the shelf. |
| Settings | Where the shelf is (the NAS, or any folder on this PC), where games go on this PC, Proton, emulators. |

The terminal commands do the same things:

```text
steam-fan                    open the app
steam-fan sync [--dry-run] [--only "Mario 63"] [--rescan]
steam-fan list [--installed]
steam-fan install "Super Mario 63"
steam-fan remove  "Super Mario 63"
steam-fan archive list | restore <appid> | upload <appid>
steam-fan steam              update the library if Steam is closed, then start Steam
steam-fan launch <id> %command%   what Steam runs when you press Play
```

## The shelf

```text
<shelf>/fangames/<group>/<game>/              Sonic The Hedgehog/Sonic Mania Plus, ...
<shelf>/fangames/<group>/<subgroup>/<game>/
<shelf>/nonsteam/<group>/<game>/              Demos/..., same rules as fangames
<shelf>/roms/<system>/<GAMES>/<game>/         wii, WIIU, Switch: one folder or file per game
<shelf>/steam/<appid>/                        the Steam archive (SteamCMD on the NAS fills it)
<shelf>/saves/                                save backups, never deleted by Steam-Fan
```

**Which folder is a game.** A folder is one game when a program sits directly in it. It's also one game when its programs are only in build folders like `bin/` or `x64/`, or when fewer than two subfolders have programs. Otherwise it's a group of games. Folders that hold only zip files or source code are listed as "no program found" and left out of Steam.

**Emulator systems.** Each system folder in `roms/` needs an entry under **Settings > Emulators**, with the file types and the command to start it. The examples cover Dolphin (`wii`), Cemu (`WIIU`) and Ryujinx (`Switch`), using the Linux versions of those emulators. Install them first: `dolphin-emu`, `cemu`, `ryujinx`. Systems without an entry, like `Nintendo 64` and `XBOX 360`, which hold source code right now, are shown in the sidebar and skipped. Wii U update and DLC folders are skipped too. Install those into Cemu itself.

## Where things are on the PC

| What | Where |
|---|---|
| Settings | `~/.config/steam-fan/config.toml` (also holds the optional Steam API key, readable only by you) |
| Library database and history | `~/.local/share/steam-fan/library.db`. Safe to delete: it's rebuilt from the shelf. |
| Icons for Steam | `~/.local/share/steam-fan/icons/`, taken out of each game's `.exe` (no extra tools needed) |
| Fan games | `~/Games/NonSteam/<group>/<game>` (the same place the old scripts used, so games you already have count as installed) |
| Non-Steam games | `~/Games/NonSteam/Non-Steam Games/...` |
| Emulator games | `~/Games/Emulation/<system>/<game>` |
| Steam's files | Steam-Fan backs up `shortcuts.vdf` and `config.vdf` (`*.bak-<date>`) every time it changes them |

## How it's built

```text
crates/core   the library: no window code, all tested
  config.rs     settings file
  shelf.rs      reading the shelf (folder listings only, fast over the network), picking programs and ROM files
  db.rs         SQLite index: what was found, which program starts each game, history
  library.rs    refresh, install, remove, upload, keeping Steam in step
  steam.rs      Steam's library (shortcuts.vdf), Proton per game (config.vdf), manifests, libraries
  vdf.rs        Steam's binary and text file formats, order kept on save
  archive.rs    Steam archive: compare versions, install from shelf, copy to shelf
  launch.rs     Play: Proton prefix saves, emulator command, backups
  saves.rs      save backup and restore
  transfer.rs   copying that resumes and skips identical files, writes via temporary names
  icons.rs      icons from .exe / .ico / .png / Wii U meta
  webapi.rs     Steam Web API: owned games
crates/app    the steam-fan program: window (egui), Play progress window, terminal commands
legacy/       the earlier shell and Python scripts, the NAS archive script, docs
```

Run the tests with `cargo test`.

## Next steps

1. **Shelf service on the NAS.** A small container serves the shelf over HTTP. The box then works through Tailscale from anywhere, with no SMB mount. The Steam archive job (`legacy/nas/archive.sh`) moves into it, with progress visible in the app.
2. Covers and banners from SteamGridDB.
3. Remote: a "wake the PC and start this game" button. Steam Remote Play already streams every library entry, Steam-Fan ones included.
