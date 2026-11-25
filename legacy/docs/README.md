# Steam archive on the Synology

The NAS keeps a copy of your Steam games and updates it every day.
Your PC installs from Steam as usual. When Steam is unavailable, you copy any archived game from the NAS instead.

```
Steam ──(SteamCMD on the NAS, daily)──> /volume1/steam-games/steam ──(game-load)──> PC
```

Files in this folder:

| File | Runs on | Purpose |
|---|---|---|
| `nas/archive.sh` | NAS (in Docker) | Downloads and updates every game in `apps.txt` |
| `pc/make-apps-list.sh` | PC | Builds `apps.txt` from the games you own |
| `pc/game-load` | PC | Copies a game from the NAS into your Steam library |

---

## 1. Folders on the NAS

Create a shared folder `games` (Kontrolpanel → Delt mappe), then make these subfolders in File Station:

```
/volume1/steam-games/steam      the archive
/volume1/steam-games/scripts    archive.sh, apps.txt, steam_user, archive.log
/volume1/steam-games/fangames   fan games and demos (see FANGAMES.md)
```

Make sure there is room: 143 games can take several TB. The archive stops by itself when less than 200 GB is free.

## 2. The game list (automatic)

The archive job keeps `apps.txt` up to date by itself, including games you buy later. It needs two small files next to it:

```
/volume1/steam-games/scripts/steam_api_key   your Steam Web API key (https://steamcommunity.com/dev/apikey, domain: localhost)
/volume1/steam-games/scripts/steam_id        your 17-digit SteamID64
/volume1/steam-games/scripts/steam_user      your Steam account name
```

- At the start of every run, it adds every game you own that isn't in `apps.txt` yet: `linux` if the game has a native Linux build, otherwise `windows`.
- **To stop archiving a game,** put `#` in front of its line. Don't delete the line, or the next run adds it back.
- If you forced a Proton version on a game that has a Linux build, change its line to `windows`, so restores match what your PC's Steam installs.
- The API key file is readable by anyone who can open the `steam-games` share, so keep the share private.

Optional: `pc/make-apps-list.sh` builds the whole list on your PC instead, and also knows which games you forced onto Proton.

## 3. Log in once (over SSH)

```sh
sudo docker pull steamcmd/steamcmd:latest
sudo docker run -it --rm -v steamcmd-root:/root steamcmd/steamcmd:latest +login YOUR_ACCOUNT_NAME +quit
```

Type your password and Steam Guard code when asked. The login is then cached in the `steamcmd-root` Docker volume, and later runs need no password. Your password is never written to a file.

## 4. First run (by hand)

```sh
sudo docker run --rm --name steam-archive \
  -e TZ='CET-1CEST,M3.5.0,M10.5.0/3' \
  -v steamcmd-root:/root \
  -v /volume1/steam-games/steam:/archive \
  -v /volume1/steam-games/scripts:/scripts \
  --entrypoint bash steamcmd/steamcmd:latest /scripts/archive.sh
```

Watch for `ok <appid>` at the end. The full log is in `/volume1/steam-games/scripts/archive.log`.

## 5. Daily updates (Task Scheduler)

Kontrolpanel → Opgavestyring → Opret → Planlagt opgave → Brugerdefineret script:

- User: `root`
- Schedule: daily, during the day when you're usually not playing (for example 10:00). SteamCMD logs in with your account and can push the Steam client on your PC offline.
- Command: the same command as in step 4, on one line, with `/usr/local/bin/docker` instead of `sudo docker`.

The script stops starting new games after 22:30 because the NAS powers off at 23:00. A game that gets cut off resumes on the next run. The first full download takes several days.

Optional: a second weekly task with `-e VALIDATE=1` added, which re-checks every file.

## 6. PC: mount the archive

```sh
sudo pacman -S cifs-utils rsync
sudo mkdir -p /mnt/nas-games
sudo install -m 600 /dev/null /etc/nas-credentials
sudoedit /etc/nas-credentials     # two lines:  username=YOURDSMUSER   password=...
```

Add to `/etc/fstab` (one line):

```
//zainnas-1.snowy-rigel.ts.net/games  /mnt/nas-games  cifs  credentials=/etc/nas-credentials,uid=1000,gid=1000,iocharset=utf8,ro,_netdev,nofail,x-systemd.automount  0 0
```

Then:

```sh
sudo systemctl daemon-reload
ls /mnt/nas-games/steam
```

It mounts read-only, so nothing on the PC can damage the archive. `nofail` lets the PC boot normally while the NAS is off.

## 7. PC: install a game from the archive

```sh
install -m 755 pc/game-load ~/.local/bin/game-load
game-load --list
game-load "hollow knight"      # or the appid
```

- Close Steam first. Steam only notices new games when it starts.
- If your library is on another drive, set `LIB`, for example `export LIB=/mnt/SteamGames/SteamLibrary` in `~/.zshrc`.
- When you're done with a game, uninstall it in Steam. That only deletes the PC copy.

**Test with the one small game.** After `game-load`, start Steam. The game should show as installed and start (or show only a small update if Steam is online). If Steam starts a full download, the archived platform doesn't match: switch that line between `windows` and `linux` in `apps.txt`.

## 8. Prepare for Steam being down

- Offline mode only works if you've logged in before with the password saved. Test it once now: Steam → Gå offline.
- Games with extra DRM (Denuvo, online accounts) may not start offline even when installed. That's outside anyone's control.
- The NAS has a single 20 TB disk. For a real doomsday copy, keep your most important games on a second disk too.

## 9. Fan games and demos

See `FANGAMES.md`.
