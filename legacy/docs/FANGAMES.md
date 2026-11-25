# Fan games and demos in Steam

Every fan game on the NAS shows up in your Steam library. The first time you press **Play**, it's copied from the NAS to your PC and then starts. Windows games run through Proton, and Linux games run natively. When you quit, its saves are copied to the NAS.

```
NAS  steam-games/fangames/<collection>/<category>/<game>    the shelf (all games)
NAS  steam-games/saves/fangames/<game>/                     save backups
PC   ~/Games/NonSteam/<collection>/<category>/<game>        games you've played (the box)
```

| Script | What it does |
|---|---|
| `fan-sync` | Adds the NAS games to Steam (run again after adding games to the NAS) |
| `fan-fetch` | Runs when you press Play: downloads the game, starts it, backs up saves |
| `fan-remove <name>` | Backs up saves, then deletes the game from your PC. It stays in Steam |

---

## One-time setup

**1. NAS folder.** In File Station, rename `steam-games/_inbox` to `steam-games/fangames`. Inside, you keep your own layout (`Fan games/Sonic The Hedgehog/...`, `Games/Demos/...`).

**2. The NAS share must be writable** (it is, since you switched `ro` to `rw`) and mounted at `/mnt/nas-games`.

**3. Install the scripts:**

```sh
cd ~/Downloads/steam-archive/pc        # wherever you unzipped it
install -m 755 fan-fetch fan-remove fan-sync ~/.local/bin/
```

`fan-fetch` must be at `~/.local/bin/fan-fetch`, because Steam calls it from there.

## First test: one game

1. Close Steam completely (Steam → Exit).
2. Preview, which writes nothing:
   ```sh
   fan-sync --dry-run --only "Super Mario 63"
   ```
   It should show `+ Fan games/Super Mario Bros/Super Mario 63 -> Super Mario 63.exe (windows)`.
3. Add it to Steam:
   ```sh
   fan-sync --only "Super Mario 63"
   ```
4. Start Steam. **Super Mario 63** is in your library. Press Play.
   - You get a "Downloading Super Mario 63" notification. The game starts after the copy.
   - Play a bit, save, and quit.
5. Check that the save reached the NAS:
   ```sh
   find "/mnt/nas-games/saves/fangames/Fan games/Super Mario Bros/Super Mario 63" -type f
   ```
6. Test removing and re-downloading:
   ```sh
   fan-remove "mario 63"
   ```
   Press Play in Steam again. It downloads again, and your save should still be there.

If something goes wrong, the log is at `~/.local/state/nas-games/fan-fetch.log`.

## Then add everything

```sh
fan-sync --dry-run      # look at the list first
fan-sync                # with Steam closed
```

- Lines with `!` are folders where no program was found, such as a folder with only data files. They're skipped.
- If a game starts the wrong program (some folders contain several), fix it in Steam: right-click the game → Properties → **Target**. Keep the quotes. Leave the launch options as they are.

## Day to day

- **Play:** press Play in Steam. Only the first time downloads.
- **Add new games:** put the folder on the NAS in `fangames/<collection>/<category>/`, close Steam, run `fan-sync`.
- **Free space:** `fan-remove <name>`. `fan-fetch --list` shows what's on your PC.
- **New version of a game:** put it on the NAS as a new folder, e.g. `Super Mario 127 v0.8`, and run `fan-sync`. It appears as a separate entry. Once you're happy, delete the old folder on the NAS and run `fan-sync` again. That removes the old entry from Steam.

## Good to know

- **Saves:** two kinds are backed up after each session:
  - files inside the game folder that differ from the NAS original (`game/`)
  - the game's Windows user folder in Proton (`prefix/`: AppData, Documents)

  Nothing on the NAS is ever deleted by these scripts.
- **NAS off:** games already on your PC still start. Their saves are backed up the next time you play with the NAS on.
- **New PC:** on the first Play, Proton creates a fresh Windows folder. Your old saves are put back after that first session, and a notification tells you.
- **Steam must be closed** when running `fan-sync`, because it edits Steam's list of non-Steam games. Your Steam files are backed up first. `*.bak-original` is the state before `fan-sync` ever ran.
- `fan-sync` only touches entries it made itself. Your own non-Steam games are left alone.
- Windows games use the same Proton version Steam uses for all games. To choose one, run `PROTON=proton_experimental fan-sync`.
