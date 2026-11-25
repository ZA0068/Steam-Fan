# NAS Games Cheat Sheet

Nov 22, 2026 · @Gateway

The NAS is the shelf (every game, kept up to date), the PC is the box (only what you play), and Steam is the launcher for everything.

## At a glance

&#91;embedded content: NAS shelf, PC box, Steam as launcher\]

The NAS keeps every game; the PC copies a game in when you play it, and Steam starts all of them.

## Status

The Steam archive runs daily but is stuck until the iSCSI LUN is deleted: the NAS has only \~71 GB free and the script stops below 200 GB.

**Done**

- Vaultwarden on HTTPS through Tailscale, working on phone and PC (hosts-file fix on the PC).
- Steam archive: SteamCMD login saved, daily task at 10:00, `apps.txt` with 133 games, new purchases added automatically through the env file. About 12 games archived so far.
- Fan games copied to `steam-games/fangames` (two runs were interrupted, so not yet verified there).
- Verified copies on the PC in `~/Games/temp`: Games, Fan games and Emulators all identical to the LUN.
- T7 formatted ext4, Emulators copied to it from `~/Games/temp`.
- 23:00 NAS shutdown switched off for the migration.

**To do, in order**

- [ ] Make the T7 readable for DSM: `tune2fs` fix (see Emulator games).
- [ ] Copy the T7's Emulators into `steam-games/roms` with USB Copy (Incremental).
- [ ] Check `SteamLibrary` on the LUN for anything not in `apps.txt`.
- [ ] Delete the LUN: PC logout, then SAN Manager → LUN → Slet.
- [ ] Verify `fangames` on the NAS against `~/Games/temp/Fan games`, and settle the folder layout before running `fan-sync`.
- [ ] Delete `~/Games/temp` once everything on the NAS is verified.
- [ ] Test `fan-sync` with Super Mario 63.
- [ ] Decide on the encrypted `steam-games` folder: decrypt it, or set auto-mount in Key Manager.
- [ ] Revoke the Steam API key that was pasted in chat, if not done yet.
- [ ] Exclude `~/Games` from Baloo file indexing.
- [ ] Remove the GameVault project and `docker/gamevault`, if not done yet.
- [ ] After the first full archive: turn the 23:00 shutdown back on, remove `STOP_AT=2400` if added, move snapshot tasks into daytime.
- [ ] Replace the Tailscale certificate in DSM before Feb 13, 2027.
- [ ] Later: emulator games into Steam with Steam ROM Manager.

## Addresses and folders

HTTPS addresses work only with Tailscale on; the IP addresses only with plain `http://`.

| Service | Address | Notes |
| --- | --- | --- |
| Vaultwarden | `https://zainnas-1.snowy-rigel.ts.net` | Proxy 443 → `localhost:8443` |
| Jellyfin | `https://zainnas-1.snowy-rigel.ts.net:8097` | Proxy 8097 → `localhost:8098` (container 8096) |
| DSM | `https://zainnas-1.snowy-rigel.ts.net:5001` | Or `http://10.0.0.36:5000` at home |
| NAS over SSH | `ssh zain0068@10.0.0.36` | Docker needs `sudo -i` first |

**On the NAS** (`/volume1/steam-games`)

| Folder | What it holds |
| --- | --- |
| `steam/` | Steam archive, one folder per appid |
| `scripts/` | `archive.sh`, `apps.txt`, `steam_user`, `archive.log` |
| `fangames/` | Fan games and demos |
| `roms/` | Emulator games (to fill from the T7) |
| `saves/fangames/` | Fan game save backups made by `fan-fetch` |
| `/volume1/docker/steam-archive.env` | `STEAM_API_KEY` and `STEAM_ID`, kept out of the games share |

**On the PC**

| Path | What it is |
| --- | --- |
| `/mnt/nas-games` | The `steam-games` share (SMB, read-write, line in `/etc/fstab`) |
| `/mnt/steam_games` | The iSCSI LUN (to be deleted) |
| `~/Games/Steam` | Steam library (`LIB` for `game-load`) |
| `~/Games/NonSteam` | Fan games downloaded by `fan-fetch` |
| `~/Games/temp` | Verified copies from the LUN (delete when done) |
| `/run/media/zain/T7` | External SSD, ext4 |
| `~/.local/bin` | `game-load`, `fan-fetch`, `fan-remove`, `fan-sync` |

## Steam archive

Every day at 10:00 the NAS logs into Steam with SteamCMD, adds new purchases to `apps.txt`, and downloads updates for every listed game. It works offline later; LanCache would not.

**Daily task** (Opgavestyring, user `root`, daily 10:00, "Fortsæt kørslen" unticked):

```
/usr/local/bin/docker run --rm --name steam-archive --env-file /volume1/docker/steam-archive.env -e TZ='CET-1CEST,M3.5.0,M10.5.0/3' -v steamcmd-root:/root -v /volume1/steam-games/steam:/archive -v /volume1/steam-games/scripts:/scripts --entrypoint bash steamcmd/steamcmd:latest /scripts/archive.sh
```

- Monthly check for damaged files: same command plus `-e VALIDATE=1`, e.g. Sunday 07:15.
- While the NAS stays on at night: add `-e STOP_AT=2400` so it doesn't stop at 22:30.
- "container name already in use" = a run is still going. Leave it.
- "Container stopped unexpectedly" in DSM = usually the normal end of a run.

**Watch progress** (on the NAS as root):

```
tail -f /volume1/steam-games/scripts/archive.log | grep --line-buffered -E 'updating|progress|ok |FAILED'
docker exec steam-archive sh -c 'tail -c 300 /tmp/tmp.*'
du -sh /volume1/steam-games/steam
```

The first line needs the live-log change in `archive.sh`; the second works without it. Edit `archive.sh` only while no run is active.

**`apps.txt` rules**

- Line format: `<appid> <platform> <name>`, e.g. `584400 windows Sonic Mania`.
- `linux` only for native Linux games you don't force onto Proton; everything else `windows`.
- To skip a game, put `#` in front. Never delete the line, or it gets added back.
- Games worth skipping: Microsoft Flight Simulator, Elite Dangerous, the Sonic Racing network test, and EA or Rockstar titles that need their own launcher online.

**Restoring a game when Steam is down** (on the PC):

1. Close Steam completely.
2. `game-load --list` to see the archive, then `game-load "sonic"` (or the appid).
3. Start Steam in offline mode; the game shows as installed.

Offline mode only works if prepared while online: log in once with the password saved, then try Steam → Go Offline.

## Fan games in Steam

Every fan game on the NAS appears in Steam; the first Play copies it to `~/Games/NonSteam`, then starts it (Proton for Windows games, native for Linux), and saves go to the NAS when you quit. Not tested on real Steam yet.

| Command | What it does |
| --- | --- |
| `fan-sync --dry-run` | Shows what would be added to Steam; writes nothing |
| `fan-sync --only "Super Mario 63"` | Adds only matching games (use for the first test) |
| `fan-sync` | Adds or updates all games; run after adding games to the NAS |
| `fan-sync --rescan` | Picks each game's program again |
| `fan-remove "mario 63"` | Backs up saves, deletes the PC copy; stays in Steam |
| `fan-fetch --list` | Fan games currently on the PC |

- Steam must be closed while `fan-sync` runs. It backs up Steam's files first (`*.bak-original` = before the first run).
- Expected layout: `fangames/<collection>/<category>/<game>`, e.g. `fangames/Fan games/Sonic The Hedgehog/Sonic 3 A.I.R`. The latest copy went to `fangames/Sonic The Hedgehog/...` without the `Fan games` level. Pick one layout before running `fan-sync`.
- Wrong program starts: right-click the game in Steam → Properties → Target. Keep the quotes and the launch options.
- New version: put it on the NAS as a new folder, run `fan-sync`, delete the old folder once happy, run `fan-sync` again.
- Log: `~/.local/state/nas-games/fan-fetch.log`.

**First test**

1. Rename or check `steam-games/fangames`, and install the scripts: `install -m 755 fan-fetch fan-remove fan-sync ~/.local/bin/`.
2. Close Steam, run `fan-sync --only "Super Mario 63"`.
3. Start Steam, press Play, save, quit.
4. Check the save reached the NAS under `saves/fangames/`.
5. `fan-remove "mario 63"`, press Play again: it downloads and the save is back.

## Emulator games

The emulator games (Switch, Wii, Wii U, Xbox 360, N64Recomp) go from the T7 into `steam-games/roms`; adding them to Steam comes later.

**1. Make the T7 readable for DSM** (on the PC). DSM's older kernel can't mount ext4 made with two newer features; this removes them without touching the files:

```
lsblk -o NAME,SIZE,MODEL,TRAN
sudo umount /run/media/zain/T7
sudo tune2fs -O ^orphan_file,^metadata_csum_seed /dev/sdX1
sudo e2fsck -f /dev/sdX1
```

Replace `sdX1` with the T7 partition (`usb`, `T7`). Never `nvme0n1`.

**2. Copy on the NAS** (needs free space, so after deleting the LUN). Either USB Copy: Opret → Import, source T7 `Emulators`, destination `steam-games/roms`, mode **Incremental**, "remove source" unticked. Or over SSH:

```
mkdir -p /volume1/steam-games/roms
rsync -rtl --info=progress2 /volumeUSB1/usbshare/Emulators/ /volume1/steam-games/roms/
```

**3. Later:** native Linux emulators (Ryujinx fork, Dolphin, Cemu; Xenia through Proton), games downloaded on first Play like the fan games, added to Steam with Steam ROM Manager. Saves backed up to the NAS after playing.

## rsync

Copy with `-rtl`, then verify with `-c -n`; a rerun only copies what is missing or different.

```
# copy (resumable: run again after any interruption)
rsync -rtl --info=progress2 "SOURCE/" "DEST/"

# verify: prints the number of real differences, 0 = identical
rsync -rtlc -n --itemize-changes "SOURCE/" "DEST/" | grep -vc '^\.d'

# verify with progress, differences saved to a file
rsync -rtlc -n --info=progress2 --out-format='%i %n' "SOURCE/" "DEST/" > ~/verify.txt
```

- **Always end the source with `/`.** Without it, rsync compares or copies the folder one level too deep (`Emulators/Emulators`).
- No `sudo` needed for your own files; it doesn't fix network errors.
- `-c` reads every file on both sides: hours for hundreds of GB. Output only appears at the end.
- `-n` = dry run, nothing written. Write speed 0 during a check is normal.
- Unfinished files are written as hidden `.name.XXXXXX` and only renamed when complete. With `--partial`, an unfinished file keeps its real name until the next run.
- For a literal byte-by-byte check: `diff -rq "SOURCE" "DEST"`.

| Output starts with | Meaning |
| --- | --- |
| `.d..t......` | Only a folder's date differs: harmless |
| `>f+++++++++` | File missing in the copy |
| `>fc` | File contents differ |
| `cd+++++++++` | Folder missing |
| (no output) | Identical |

## Troubleshooting

Every error we hit so far, newest kinds first.

| Error | Cause | Fix |
| --- | --- | --- |
| `Input/output error` on `/mnt/steam_games` | iSCSI connection lost (NAS restart, network drop) | `sudo umount -l /mnt/steam_games`, `sudo iscsiadm -m node --logout`, `--login`, then mount by UUID `5d882f56-…` |
| `change_dir ... No such file or directory` | Source disk or share not mounted | Mount it first; `findmnt /mnt/nas-games` must show `cifs` |
| `close failed ... Resource temporarily unavailable` + `Broken pipe` | NAS too slow to answer over SMB (busy disk or network blip) | Run the same rsync again; avoid running it while the archive job downloads |
| T7 not visible in DSM | ext4 with `orphan_file` / `metadata_csum_seed` | `tune2fs` fix in Emulator games |
| `chown: invalid group 'zain:zain'` | No group named zain | `sudo chown -R "$(id -un):$(id -gn)" /run/media/zain/T7` |
| rsync `Permission denied` / `failed to set times` on the T7 | Drive owned by root after formatting | The chown above, then rerun |
| `docker: permission denied ... docker.sock` | Normal user can't use Docker | `sudo -i` first |
| `container name "/steam-archive" is already in use` | A run is still active | `docker ps -a --filter name=steam-archive`; if `Exited`, `docker rm steam-archive` |
| `STOP: less than 200 GB free` | NAS almost full | Delete the LUN; skip giant games with `#` |
| Share locked after NAS restart | Encrypted `steam-games` folder | Decrypt it, or Key Manager → mount at startup |
| `Cached credentials not found` in SteamCMD | Login wasn't saved in the volume | `docker run -it --rm -v steamcmd-root:/root steamcmd/steamcmd:latest +login zainahmed0097 +quit` |
| Browser `refused to connect` on the ts.net name | Wrong line in `/etc/hosts` (name → 127.0.0.1) | Remove the bad line; keep `100.120.69.107 zainnas-1.snowy-rigel.ts.net` |
| `ERR_SSL_PROTOCOL_ERROR` / "secure context" in Vaultwarden | `https://` on port 8443, or `http://` used for the vault | Use `https://zainnas-1.snowy-rigel.ts.net` without a port |
| `unknown filesystem type 'vfat'` | Running kernel's modules removed by an update | Reboot normally (not hibernate) |
| Baloo reading 100+ MB/s | KDE indexing the copied games | `balooctl6 suspend`, exclude `~/Games` in File Search |
