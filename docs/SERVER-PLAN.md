# Steam-Fan server: plan

The shelf moves from "a folder the PC mounts" to a small service on the NAS
that the PC app talks to. Nothing here is built yet.

## Decisions (26 Nov 2026)

| Question | Decision |
|---|---|
| Reach from outside home | **Tailscale only.** No ports opened on the router, no public address. |
| Users | **Just me.** Device pairing; no user accounts or single sign-on. |
| Emulator save states | **Synced**, besides in-game saves. They hold the emulated console's memory, not the game, so they're tens of MB, not GB. |
| Fallback | **HTTPS over Tailscale first, the SMB share second, local copies always.** |

## Pieces

```
PC app (box)  ──HTTPS over Tailscale──►  Steam-Fan server (Docker on the NAS)
     │            (or SMB share if the       · catalog API, file downloads (resumable)
     │             server is down)           · save versions
     ▼                                       · SQLite database
   Steam ◄── Steam's own update channel      · SteamCMD archive job (replaces archive.sh)
```

## How the PC finds and trusts the server

1. **At home:** the server announces itself on the local network (mDNS, `_steam-fan._tcp`).
2. **Elsewhere:** the app uses the Tailscale name `zainnas-1.snowy-rigel.ts.net`.
3. **Pairing, once per PC:** the server's web page shows a 6-digit code; type it into the app. The PC gets its own token, kept in KDE Wallet. The server stores only a hash of it and lists paired devices, each of which can be revoked.
4. **Every request** needs the token and arrives over HTTPS, using the Tailscale certificate. The server listens only on the home network and on Tailscale.

## Fallback order when the app needs the shelf

1. Server over HTTPS (LAN address if at home, Tailscale name if not).
2. SMB share `/mnt/nas-games` (works today; read-only use).
3. Nothing reachable: play what's already on the PC; save uploads wait in a queue.

## Database (SQLite on the NAS)

Game files stay normal files on the Btrfs volume (snapshots protect them). The database holds:

- **games**: id, kind, name, group/system, start file, size, version
- **game_files**: per game version, the path, size and checksum of each file (used for resume, verify, and downloading only changed files)
- **devices**: paired PCs, token hash, last seen
- **save_versions**: game, device, time, based-on version, list of files with checksums
- **blobs**: save file contents, stored once per checksum in a folder on disk (not in the database)
- **sessions**: play time per game and device
- **jobs**: SteamCMD runs, uploads, scans, with progress

## Saves, like Steam Cloud

- **Before Play:** if the server has a newer save version than this PC, download it.
- **After quitting:** upload a new version, recording which version it was based on.
- **Conflict** (played elsewhere meanwhile): keep both, ask which to use.
- **History:** last 20 versions plus one per day; restore an older one from the app.

What counts as the save:

| Kind | Save |
|---|---|
| Fan / PC games (e.g. Super Mario 63, one exe) | Files in the game folder that differ from the shelf original, plus the Proton prefix user folder (AppData, Documents). The game itself is never uploaded. Later: Ludusavi's save-location list. |
| Wii (Dolphin) | That game's NAND save (by its ID, e.g. `VBKP`), GameCube memory card, save states |
| Wii U (Cemu) | `mlc01/usr/save/<title id>` (Cemu has no save states) |
| Switch (Ryujinx) | that game's `bis/user/save` folder (no save states) |

Save states are labelled with the emulator version, because a newer emulator often can't load an older state.

## Order of work

1. Server: catalog and file downloads, pairing, HTTPS on Tailscale. App: talks to it, falls back to SMB.
2. Save versions with history and conflicts.
3. Per-game emulator saves and states.
4. SteamCMD archive job inside the server, with progress in the app.
5. Server web page: devices, jobs, storage.
