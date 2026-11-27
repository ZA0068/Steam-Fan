# Steam-Fan-Server

The shelf as a service. It runs in Docker on the NAS, next to the games, and does
the heavy work so the PC app doesn't have to:

- **Catalog:** reads the shelf every 15 minutes (and when asked) and keeps the database.
- **Downloads:** direct links (SFGHQ attachments, itch.io files), GitHub projects' latest release, Game Jolt game pages, Google Drive,
  Dropbox and MEGA (megatools is in the image). Unpacks zip, 7z, rar and tar.
- **Updates:** every 12 hours it asks each game's download link whether it serves a
  new file. It reports it, or with `SF_AUTO_UPDATE=replace` downloads it by itself.
- **Integrity:** keeps SHA-256 checksums of every game; verify on the server.
- **Versions, artwork, info:** same rules as the app (SteamGridDB, uploads, screenshots).
- **Game files to the PC:** every file can be fetched on its own, resumably (HTTP `Range`),
  with its size and checksum, so a PC fetches only what changed.

```
PC (Steam-Fan app)  ── Tailscale ─▶  NAS: steam-fan-server (Docker)  ──▶  /volume1/steam-games
   asks, plays                         catalog · downloads · updates        (the shelf)
   ◀──────────── game files, pictures, info ──────────────
```

Status: **step 1 (the server) is built.** The PC app still reads the shelf through the
mounted share until step 2 switches it to the server. Both can run at the same time.

## Install on the DS423+

**One command, on the PC:**

```sh
cd ~/Projects/Steam-Fan && docker/deploy-nas.sh zain@10.0.0.36
```

It asks for your NAS password (SSH, then sudo) and:

1. finds your DSM user and group numbers, the share holding `fangames/`, and Docker;
2. copies the project to `/volume1/docker/steam-fan-src` and makes `/volume1/docker/steam-fan-server` for the server's data;
3. writes `docker/.env` there, which `compose.yaml` reads (user, shelf, data folder);
4. builds the image: **on the PC** if Docker runs on it (a few minutes, then the image is sent over),
   otherwise **on the NAS** (15 to 30 minutes the first time). `--build-on-nas` forces the NAS;
5. starts the container, shows its log and a **pairing code**.

Run it again to update the server. **Or, faster:** GitHub Actions builds the image on every push
(`ghcr.io/za0068/steam-fan-server`), so on the NAS only this is needed:

```sh
sudo sh /volume1/docker/steam-fan-src/docker/nas-up.sh pull
```

Before the first run:

- DSM > Control Panel > Terminal & SNMP: **Enable SSH service**.
- Your DSM user is in the **administrators** group (sudo needs it).
- **Container Manager** is installed (Package Center). It makes the `docker` shared folder.
- Your user can read and write the games share (it's the same user the container runs as).

Then check from the PC: `curl http://zainnas-1.snowy-rigel.ts.net:8484/api/health`.

**By hand instead:** copy the project to the NAS, put `SF_UID`, `SF_GID`, `SF_SHELF_HOST` and
`SF_DATA_HOST` into `docker/.env` (see `compose.yaml`), then Container Manager > Project > Create on
the `docker` folder, or `sudo docker compose up -d --build` in it.

## Pair a PC

Every request needs a token; each PC gets its own.

```sh
sudo docker exec steam-fan-server steam-fan-server pair      # prints a 6-digit code, valid 10 minutes
```

In the app: **Settings → Steam-Fan-Server**, type the address (`http://10.0.0.36:8484`) and the code, press **Pair**.
Add the Tailscale address too (**Add without pairing**: the same token works). Addresses are tried in order.
From the command line: `steam-fan server pair http://10.0.0.36:8484 123456`. By hand:

```sh
curl -X POST http://zainnas-1.snowy-rigel.ts.net:8484/api/pair \
     -H 'content-type: application/json' -d '{"code":"123456","device":"zain-pc"}'
```

A code works once and dies after 5 wrong tries. Only a hash of each token is kept
(`data/server/devices.json`). `steam-fan-server devices` lists PCs, `steam-fan-server revoke zain-pc` unpairs one.
When nothing is paired yet, the log shows a code at start (`sudo docker logs steam-fan-server`).

## Reaching it

| From | Address |
|---|---|
| anywhere (Tailscale) | `http://zainnas-1.snowy-rigel.ts.net:8484`: Tailscale already encrypts it |
| home network (fallback) | `http://10.0.0.36:8484` |
| HTTPS (optional) | `sudo tailscale serve --bg --https=8443 http://127.0.0.1:8484` on the NAS, then `https://zainnas-1.snowy-rigel.ts.net:8443` (needs HTTPS certificates turned on for your tailnet) |

Nothing is opened on the router. The NAS firewall can limit port 8484 to the home
network and Tailscale (100.64.0.0/10).

## Settings

Environment variables in `compose.yaml`:

| Variable | Default | |
|---|---|---|
| `SF_SHELF` | `/shelf` | the shelf inside the container |
| `SF_DATA` | `/data` | database, picture cache, downloads in progress, paired PCs |
| `SF_PORT` | `8484` | |
| `SF_SCAN_MINUTES` | `15` | read the shelf this often; `0`: only when asked |
| `SF_UPDATE_HOURS` | `12` | check download links this often; `0`: only when asked |
| `SF_AUTO_UPDATE` | `off` | `replace`: download a new file by itself (old copy kept until the new one is in place; saves stay) |
| `STEAMGRIDDB_API_KEY` | | for automatic covers; or in `data/config/steam-fan/secrets.env` |
| `SF_TOKEN` | | a fixed token for scripts (optional) |

Emulator settings come from `data/config/steam-fan/config.toml`, the same format as the app's.

## API

All under `/api`, with `Authorization: Bearer <token>` (or `?token=` for picture links).
A game is named by its id, e.g. `id=fan:Sonic The Hedgehog/Sonic Quantum Collision` (URL-encoded).
Long work answers `{"job": n}`; follow it with `GET /api/job?id=n`.

| | | |
|---|---|---|
| `GET` | `/health` | no token: name, version, shelf found, number of games |
| `POST` | `/pair` | no token: `{code, device}` → `{token}` |
| `GET` | `/library` | one entry per game, with its pictures, link and update state; a game kept in several versions lists them (`versions`, newest first) and is shown as the one Play uses |
| `POST` | `/set-version` | `{id, version}`: the version Play uses, for every PC (`""`: the newest) |
| `POST` | `/set-newest` | `{id, version}`: mark a version as the newest (`""`: worked out from names and dates) |
| `GET` | `/game?id=` | one game: info, screenshots, history, checksums kept |
| `GET` | `/manifest?id=` | its files: path, size, mtime, sha256 |
| `GET` | `/file?id=&path=` | one file; `Range` supported |
| `GET` `PUT` `DELETE` | `/art?id=&kind=` | `cover` `hero` `logo` `icon` `wide` (`thumb` to get a small cover); `&shot=<name>` for a screenshot |
| `POST` `DELETE` | `/screenshot?id=&name=` | add (body: the picture) or remove a screenshot |
| `PUT` | `/info?id=` | description, developer, page, download link… |
| `GET` | `/page-images?url=` | pictures on a game's web page (SFGHQ, Game Jolt, itch.io…) |
| `POST` | `/screenshots-from-urls` | `{id, urls}`: the server downloads them onto the shelf → `{added, failed}` |
| `POST` | `/refresh` | `{full}`: read the shelf (job) |
| `POST` | `/artwork` | `{id?, again}`: find artwork (job) |
| `POST` | `/add-url` | `{url, kind, group, name, version?, page?}`: the server downloads and adds a game (job) |
| `POST` | `/import?kind=&group=&name=&version=&filename=` | body: an archive from the PC (job) |
| `POST` | `/redownload` | `{id, version?}`: from its link; replaces it, or adds it as a version (job) |
| `POST` | `/redownload-file?id=&version=&filename=` | body: a file downloaded in the browser (job) |
| `POST` | `/check-update` | `{id}` → `{state: current/new/unknown/error, msg}` |
| `POST` | `/check-updates` | check every link (job) |
| `GET` | `/updates` | last check per game |
| `POST` | `/verify` | `{id}`: check the shelf copy against its checksums and patch damaged files from the download link (job) |
| `GET` | `/jobs`, `/job?id=` | progress, result or error |
| `POST` | `/job/cancel` | `{id}` |
| `GET` | `/devices` | paired PCs |

## The app as a client

With a server address in Settings, the app works through the server:

- **Library, details, pictures**: the library comes from `/snapshot`; pictures and screenshots are fetched when shown and kept in the cache.
- **Install / Play**: the app reads the game's manifest and downloads only missing or changed files. Downloads resume and are checked with sha256.
- **Saves**: they go up to the server after playing, and newer ones come down before playing. Nothing gets deleted.
- **Add game, Re-download, Check for update, Move, Versions**: these are requests. The server does the work and patches the shelf.
- **Verify**: checks this PC's copy against the server's manifest and downloads the bad files again. It never changes the shelf.
- **Archived Steam games** still use the shelf folder, so mount it for those.
- **No server answering, but the share is mounted** (Settings → Shelf): the app uses the share directly until the server is back.
  The server keeps its files on that same share, so nothing is lost: the server sees the changes at its next scan, and saves are matched by date.
- **No server and no share:** games that are already on the PC still play.
- **Find on my network** (Settings, or `steam-fan server find`) looks for the server on your home network, so you don't need to know its address. Tailscale addresses still have to be typed in.
