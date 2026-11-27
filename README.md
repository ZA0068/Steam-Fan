<p align="center"><img src="docs/logo.svg" width="160" alt="Steam-Fan logo"></p>

# Steam-Fan

**Your NAS is the shelf, your PC is the box, Steam is the launcher.**

Every game you have is kept on the shelf: fan games, other non-Steam games,
emulator games, and an offline archive of your Steam games. Steam on your PC
shows all of them. Press Play and the game is copied from the shelf to the PC
first, if it isn't there yet. When you quit, its saves go back to the shelf.

## The idea

Think of a **huge shelf** at home, a **small box** you carry around, and the
**stores** outside where new things come from.

```mermaid
flowchart LR
    STORE["🏬<br/><b>The stores</b><br/>Steam's servers<br/>fan game sites<br/>SteamGridDB"]
    SHELF["🗄️<br/><b>The huge shelf</b><br/>your NAS<br/><br/>every game you own<br/>saves · artwork · checksums"]
    BOX["📦<br/><b>The small box</b><br/>your PC<br/><br/>only what you<br/>play right now"]
    STORE -- "new games and updates<br/>are put on the shelf" --> SHELF
    SHELF -- "take a game out to play it" --> BOX
    BOX -- "saves go back,<br/>the game goes back when<br/>you need space" --> SHELF
    STORE -. "Steam games also come<br/>straight to the box, as always" .-> BOX
    style STORE fill:#fff4e0,stroke:#e0a040,color:#000
    style SHELF fill:#e3eeff,stroke:#2a6bb3,color:#000
    style BOX fill:#e6f7ea,stroke:#3aa060,color:#000
```

- **The shelf** holds everything, and it's always there for you, even when a store closes (Steam's servers down, a fan game's link gone).
- **The box** only holds what you're playing. Taking a game out of the box loses nothing: it's still on the shelf, and so are its saves.
- **The stores** are where new games and updates come from. They fill the shelf; Steam also delivers its own games straight to the box, as it always has.

## The pieces

```mermaid
flowchart TB
    subgraph NET["Internet"]
        direction LR
        STEAM[("☁️ Steam servers")]
        SGDB[("🖼️ SteamGridDB")]
        SRC[("🌐 Fan game sites<br/>SFGHQ · MEGA · Drive")]
    end

    subgraph NAS["🗄️ NAS: the shelf (Synology)"]
        direction TB
        CMD["SteamCMD container<br/>daily archive job"]
        FILES["steam-games share<br/>steam/ · fangames/ · nonsteam/ · roms/<br/>saves/ · artwork/"]
        CMD --> FILES
    end

    subgraph PC["💻 Desktop PC: the box"]
        direction TB
        APP["Steam-Fan<br/>(library window + steam-fan)"]
        STEAMAPP["Steam app<br/>(the launcher for everything)"]
        LOCAL["Games on this PC<br/>~/Games · Steam library"]
        APP -- "adds every shelf game<br/>to Steam's library" --> STEAMAPP
        STEAMAPP -- "Play" --> APP
        APP --> LOCAL
        STEAMAPP --> LOCAL
    end

    STEAM == "① updates the Steam archive<br/>every day (SteamCMD)" ==> CMD
    STEAM == "② downloads and updates<br/>Steam games, as usual" ==> STEAMAPP
    FILES <== "③ games to the PC, saves back<br/>(home network or Tailscale)" ==> APP
    SGDB -. "covers, banners, logos" .-> APP
    SRC -. "new fan games, re-downloads,<br/>update checks" .-> APP
```

| Link | What travels | When |
|---|---|---|
| ① Steam servers → NAS | Every Steam game you own, kept up to date by SteamCMD | Every day at 10:00, by itself |
| ② Steam servers → PC | Steam games you install and play, updated by Steam | As always, when Steam is online |
| ③ NAS ↔ PC | Fan games, non-Steam games and ROMs when you press Play; Steam games when Steam's servers are down; saves back after playing; artwork; games you add or re-download | Whenever you play or add something |

Artwork from SteamGridDB and pictures from a game's web page are saved **on the shelf**, so every PC gets them and they stay even if the site goes away.

## How it works

### Pressing Play on a fan game

```mermaid
sequenceDiagram
    actor You
    participant Steam as Steam app
    participant SF as steam-fan
    participant NAS as Shelf (NAS)
    participant Game

    You->>Steam: Play
    Steam->>SF: steam-fan launch "fan:…" + the Proton command
    alt not on this PC yet
        SF->>NAS: copy the game (progress window)
        SF->>SF: check every file against its checksum
        NAS-->>SF: your saves from last time
    end
    SF->>Game: start it (through Proton for Windows games)
    You->>Game: play
    Game-->>SF: you quit
    SF->>NAS: back up the saves
    Note over SF,NAS: closed within seconds? A window shows Proton's error log
```

### A Steam game while Steam's servers are down

```mermaid
sequenceDiagram
    actor You
    participant SF as Steam-Fan
    participant NAS as Shelf (NAS)
    participant Steam as Steam app

    Note over NAS: every day, SteamCMD has kept the archive up to date
    You->>SF: game page → Install from shelf (Steam closed)
    SF->>NAS: copy the archived game
    SF->>Steam: put it in Steam's library with its manifest
    You->>Steam: start Steam in offline mode → Play
    Note over Steam: when the servers are back, Steam updates it as usual
```

### Adding a new fan game

```mermaid
flowchart TB
    A["Download<br/>(zip, 7z, rar, folder)"] --> B["Drop it on Steam-Fan<br/>or + Add game"]
    B --> C{"What is it?"}
    C -->|"Fan game"| D["fangames/&lt;group&gt;/&lt;name&gt;[/&lt;version&gt;]"]
    C -->|"Non-Steam game"| E["nonsteam/&lt;group&gt;/&lt;name&gt;"]
    C -->|"Emulator game"| F["roms/&lt;system&gt;/GAMES/&lt;name&gt;"]
    D & E & F --> G["Unpacked onto the shelf<br/>+ checksums recorded"]
    G --> H["Artwork found<br/>(SteamGridDB or its web page)"]
    H --> I["Added to Steam's library<br/>(right away if Steam is closed,<br/>else with the next Update)"]
```

### Keeping everything healthy (the way Steam does it)

Like Steam's content servers, **the shelf only changes when a game is added or downloaded**:
a zip/rar/folder you add, a link the server downloads, a Re-download, an automatic update.
A PC never writes game files to it. **Verify on the PC** fixes the PC's copy from the shelf,
like Steam's "Verify integrity of game files". The server checks the shelf itself and
patches it from the game's download link.

```mermaid
flowchart LR
    subgraph In["The shelf changes only here"]
        A["Add game<br/>(zip, rar, folder)"] & L["Download from a link"] & RD["Re-download /<br/>auto update"] --> SH[("Shelf<br/>+ checksums")]
    end
    SH -->|"Play: copy to the PC"| PC["PC copy"]
    PC -->|"won't run? Verify"| V{"PC files match<br/>the checksums?"}
    V -->|"no"| FIX["Download the damaged files<br/>again from the shelf<br/>(settings and saves kept)"]
    V -->|"yes"| OK["All good"]
    SH -.->|"server check"| SV{"Shelf files<br/>match?"}
    SV -.->|"no"| PL["Patch them from<br/>the download link"]
```

The diagrams show on GitHub and in VS Code (with a Mermaid preview extension).

## Install

```bash
sudo pacman -S --needed webkit2gtk-4.1     # once: the library window needs it
cd ~/Projects/Steam-Fan
./install.sh
```

The script:

- builds two programs and puts them in `~/.local/bin`:
  - `steam-fan-app` is the library window;
  - `steam-fan` is what Steam runs when you press Play, plus the terminal commands;
- adds **Steam-Fan** to the app menu;
- makes the **Steam** menu entry start Steam through Steam-Fan, so new shelf games show up every time Steam starts;
- moves your Steam entries over from the old scripts and removes them (copies are in `legacy/`). Quit Steam first.

## Using it

Open **Steam-Fan** from the app menu.

- **Library:** a cover grid of everything on the shelf, grouped by kind or by group (Sonic, Kirby, wii, ...). Search at the top; the slider at the bottom changes the cover size. "on PC" marks games copied to this PC.
- **Getting around, like Steam:** the ‹ › arrows at the top go back and forward through where you've been (sections, games, their tabs); so do the mouse's side buttons and Alt+←/→. Next to them, the path ("Fan games › Sonic Boll (Series) › Boll Deluxe") can be clicked. A game's tags lead to their section: its kind, its group, Windows/Linux, on this PC or only on the shelf. Section titles in a list open just that group.
- **A game's page:** a big banner with the logo, then (click the cover or logo to change it, the title to rename it):
  - **Play** (through Steam), **Copy to this PC** / **Remove from PC** (saves go to the shelf first).
  - For Steam games: **Install from shelf** when Steam is down, **Copy to shelf** when this PC has the newer version.
  - **About**: a description, who made it, when, the game's page (SFGHQ, itch.io...), its download link (MEGA, Google Drive, direct) and other links. Press **Edit** to write it.
  - **Files**: **Verify** works like Steam's "Verify integrity": it checks the copy on this PC against the game's fingerprints (SHA-256) and downloads damaged or missing files again from the shelf, keeping settings and saves the game changed. It never changes the shelf. A game not on this PC gets its shelf copy checked instead, patched from its download link. Games added in the app get fingerprints right away; for others the first Verify records them. **Check for update** asks the download link whether it serves a different file now. **Re-download** gets the game again from its link, either replacing a damaged shelf copy or adding the download as a new version. Download links can be direct (forum attachments like SFGHQ's, GitHub, itch.io files), **Game Jolt game pages** (the newest Windows build is picked), **GitHub projects** like `https://github.com/STJr/SRB2` (the latest release's Windows game file, e.g. `SRB2-v2215-Full.zip`; a new release counts as an update), Google Drive, Dropbox or MEGA (`sudo pacman -S megatools`). A link that only works in a browser: download it there and use Re-download > Use a file I downloaded. `steam-fan link <url>` shows what a link leads to without downloading it.
  - **Screenshots**: add as many as you like (button, drop pictures onto the page, or **From a web page**: paste the SFGHQ page and pick its pictures); click one to see it large, arrow keys to flip through.
  - **Move / Rename**: move it to another group (or between fan and non-Steam games), or rename it. Saves, artwork, screenshots, history and the copy on this PC go with it, and Steam keeps the same entry with its play time.
  - **Artwork** tab: cover, banner, logo, icon. For each, **Change** lets you pick from a web page (the game's SFGHQ page), from SteamGridDB, upload a file, drop a picture onto the window, or paste a link. Automatic artwork only uses SteamGridDB results whose name really matches, so a fan game without an entry there gets no picture rather than a wrong one.
- **Add game:** drop a download (zip, 7z, rar), a folder or a ROM onto the window, or press **Add game**. Pick **Fan game / Non-Steam game / Emulator game**, the group or system, and the name. It's unpacked straight onto the shelf, gets artwork, and goes into Steam if Steam is closed.
- **Read shelf** looks for new or changed games, then fills in missing artwork. **Artwork** does only the artwork. **Update Steam library** adds, updates and removes Steam entries. When Steam is running, the button becomes **Restart Steam & update**: it quits Steam, updates the library and starts Steam again (Steam rewrites its library when it quits, so it can't stay open).
- **Settings:** where the shelf is (the NAS, or any folder on this PC), API keys, where games go on this PC, Proton, emulators.

### Artwork

Pictures you pick or upload are saved **on the shelf** in `artwork/<game>/` (with `info.toml` for the About text and `screenshots/`), so every PC gets them and they survive with the games. Pictures you put inside a game's own folder also count: `cover.*`, `folder.*`, `boxart.*`, `hero.*`, `logo.*`, `icon.*`. Missing covers are fetched from SteamGridDB automatically, once per game.

A copy of each picture is kept on this PC, and the pictures are also copied into **Steam's own artwork folder**. Steam's library then shows the same covers, banners, logos and icons. Steam reads new artwork when it starts.

### API keys

Keys live in `~/.config/steam-fan/secrets.env`, readable only by you, not in the settings file:

```text
STEAMGRIDDB_API_KEY=...
STEAM_API_KEY=...        # optional
STEAM_ID=...             # optional
```

Set them in **Settings** or edit the file. An environment variable with the same name wins over the file.

### Terminal commands

```text
steam-fan                    open the library window
steam-fan add <zip|folder|rom> --group "Sonic The Hedgehog" [--kind fan|nonsteam|rom] [--name ...]
steam-fan art [--again]      artwork: copy from the shelf, fetch missing ones, update Steam's
steam-fan verify "Sonic Robo Blast 2"          repair the PC copy from the shelf (like Steam's Verify integrity)
steam-fan redownload "Sonic Robo Blast 2" [--version "v2.2.14"]   get it again from its download link
steam-fan redownload "Sonic Quantum Collision" --file ~/Downloads/sqc.zip   same, from a file you downloaded
steam-fan link "https://gamejolt.com/games/ChaosShards/415280"          what a download link leads to (file, size, version)
steam-fan link "https://github.com/STJr/SRB2"                           the latest GitHub release and the file it would download
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
<shelf>/fangames/<group>/<game>/<version>/    Sonic World/Sonic World Release 10, ...: one game, several versions
<shelf>/fangames/<group>/<collection>/<game>/ Sonic - Infinity Engine/...: different games (marked)
<shelf>/nonsteam/<group>/<game>/              Demos/..., same rules as fangames
<shelf>/roms/<system>/<GAMES>/<game>/         wii, WIIU, Switch: one folder or file per game
<shelf>/steam/<appid>/                        the Steam archive (SteamCMD on the NAS fills it)
<shelf>/saves/                                save backups, never deleted by Steam-Fan
<shelf>/artwork/                              pictures picked or uploaded in the app
```

**Which folder is a game.** A folder is one game when a program sits directly in it. It's also one game when its programs are only in build folders like `bin/` or `x64/`, when fewer than two subfolders have programs, or when it's laid out like an Unreal game (`Binaries/`, `Engine/`, `<Name>Game/`). Folders that hold only zip files or source code are listed as "no program found" and left out of Steam.

**Versions.** A folder holding several games (`Sonic World/` with `Sonic World Release 1` ... `Release 10`, `Sonic World DX 1.2.4`) is **one game in several versions**: one library entry, one Steam entry, and a **Version** menu on its page, like Steam's betas.
- **Play uses the newest version.** That's the version whose program file is the newest build; folders called `Old` are never picked.
- **Or a version you choose.** The choice is kept with the game's info on the shelf, so every PC and the server agree.
- **Switching versions:** the Steam entry stays the same, with its playtime and pictures. The next Play copies the chosen version to the PC if it isn't there yet.
- **Saves:** each version has its own.
- **Moving or renaming:** Move/Rename moves the whole game or just one version.

**Collections.** A folder of *different* games (`Sonic - Infinity Engine/`, `Sonic Adventure/` with Blast, Remix, SX...) is marked as a collection: each game is its own entry, and the folder is shown as their group. Switch between the two on a game's page ("Different games?" / "Versions of one game?"). That writes a small marker file into the folder: `.steam-fan-collection`, or `.steam-fan-versions` for a folder with just one version so far.

**Names.** Windows file sharing can't pass on `:` in a name: such a folder shows up on the PC as an empty folder plus a garbled copy (`S6G8TJ~9`). Steam-Fan never makes such names: `Sonic: Codename Backstreet` becomes `Sonic - Codename Backstreet`.

**Emulator systems.** Each system folder in `roms/` needs an entry under **Settings > Emulators**, with the file types and the command to start it. The examples cover Dolphin (`wii`), Cemu (`WIIU`) and Ryujinx (`Switch`), using the Linux versions of those emulators. Install them first: `dolphin-emu`, `cemu`, `ryujinx`. Systems without an entry, like `Nintendo 64` and `XBOX 360`, which hold source code right now, are shown in the sidebar and skipped. Wii U update and DLC folders are skipped too. Install those into Cemu itself.

## Where things are on the PC

| What | Where |
|---|---|
| Settings | `~/.config/steam-fan/config.toml` (also holds the optional Steam API key, readable only by you) |
| Library database and history | `~/.local/share/steam-fan/library.db`. Safe to delete: it's rebuilt from the shelf. |
| Artwork | `~/.local/share/steam-fan/art/` (copies from the shelf), `icons/` (icons taken out of each `.exe`) |
| API keys | `~/.config/steam-fan/secrets.env` |
| Fan games | `~/Games/NonSteam/<group>/<game>` (the same place the old scripts used, so games you already have count as installed) |
| Non-Steam games | `~/Games/NonSteam/Non-Steam Games/...` |
| Emulator games | `~/Games/Emulation/<system>/<game>` |
| Steam's files | Steam-Fan backs up `shortcuts.vdf` and `config.vdf` (`*.bak-<date>`) every time it changes them |

## How it's built

```text
crates/core      the library: no window code, all tested
  config.rs        settings file and secrets.env
  shelf.rs         reading the shelf (folder listings only, fast over the network), picking programs and ROM files
  db.rs            SQLite index: what was found, which program starts each game, artwork lookups, history
  library.rs       refresh, install, remove, upload, artwork pass, keeping Steam in step
  artwork.rs       pictures on the shelf and on this PC, SteamGridDB, Steam's grid folder
  import.rs        adding games from zip / 7z / rar / folders
  steam.rs         Steam's library (shortcuts.vdf), Proton per game (config.vdf), manifests
  vdf.rs           Steam's binary and text file formats
  archive.rs       Steam archive: compare versions, install from shelf, copy to shelf
  launch.rs        Play: Proton prefix saves, emulator command, backups
  saves.rs         save backup and restore
  transfer.rs      copying that resumes and skips identical files
  icons.rs         icons from .exe / .ico / .png / Wii U meta
crates/server   steam-fan-server: the shelf as a service in Docker on the NAS (catalog, downloads, updates, file streaming). See docs/SERVER.md
crates/desktop   steam-fan-app: the Tauri window; commands the interface calls, background jobs, art:// pictures
crates/app       steam-fan: Play (with a small progress window), terminal commands
ui/              the interface (Svelte). ui/dist is the built copy the window uses
docker/          Dockerfile and compose.yaml for the server
legacy/          the earlier scripts, the NAS archive script, docs
```

**Changing the interface:** you need Node.js. Run `cd ui && npm install` once. Then:

- **Live editing:** run `npm run dev` in `ui/`, and in another terminal `cargo run -p steam-fan-app --no-default-features`. The window reloads as you edit `ui/src`.
- **When you're done:** run `npm run build`, then `./install.sh`.

Run the tests with `cargo test`.

## Next steps

1. **Steam-Fan-Server** (step 1 built: [docs/SERVER.md](docs/SERVER.md)). The NAS runs its own Steam-Fan in Docker: it downloads (MEGA, Google Drive, direct links), checks links for updates, keeps checksums and versions, and streams game files to the PC.
2. **The app as its client.** It asks the server instead of reading a mounted share, so it works over Tailscale from anywhere and on Windows. Saves sync through the server.
3. The Steam archive job (`legacy/nas/archive.sh`) moves into the server, with progress visible in the app.
4. Remote: a "wake the PC and start this game" button. Steam Remote Play already streams every library entry, Steam-Fan ones included.
