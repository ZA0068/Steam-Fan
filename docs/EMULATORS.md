# Emulators

Emulator games live in `roms/<system>/` on the shelf, for example `roms/wiiu/`, `roms/switch/Games/` or `roms/ps2/`. They show up in Steam like any other game. When you press Play, Steam-Fan:

1. copies the game to this PC if it isn't there yet;
2. gets the emulator for this PC's system from the shelf, copying only what changed;
3. brings the emulator's newer saves, save states and settings from the shelf;
4. starts the game full screen;
5. after it closes, sends the emulator's newer saves, states and settings back to the shelf.

So you can play on one PC and continue on another, or on the same PC over Steam Link.

## Setting it up

Go to **Settings → Emulators**.

**Emulators on the shelf** (the table):
- **Get** downloads the newest release from GitHub onto the shelf (with Steam-Fan-Server, the server downloads it). This works for Cemu, PCSX2, DuckStation, RPCS3, mGBA, melonDS and Azahar.
- **file…** / **folder…** add one you downloaded yourself. Use these for Dolphin, Ryujinx and PPSSPP, which don't publish builds on GitHub: get them from their page and add the AppImage, the zip/7z/tar, or the unpacked folder.
- **update** gets the newest release again.

**Systems:** pick which emulator plays each `roms/` folder. **Add a system…** lists the ones Steam-Fan knows. **My own command…** keeps any emulator you set up yourself, with your own command line.

On Linux, if the shelf has no copy of an emulator, an installed package is used instead (`dolphin-emu`, `ryujinx`, `pcsx2-qt`…). Dolphin and Ryujinx are pointed at a data folder in Steam-Fan's emulator folder, so their saves still sync.

| Emulator | Plays (`roms/…`) | Download |
|---|---|---|
| Dolphin | wii, gc | dolphin-emu.org (Linux: the `dolphin-emu` package) |
| Cemu | wiiu | Get |
| Ryujinx | switch | ryujinx.app, then file… |
| PCSX2 | ps2 | Get |
| DuckStation | ps1, psx | Get |
| PPSSPP | psp | ppsspp.org (Linux: package) |
| RPCS3 | ps3 | Get |
| mGBA | gba, gb, gbc | Get |
| melonDS | nds | Get |
| Azahar | 3ds | Get |

## Where things are

| | |
|---|---|
| `emulators/<name>/linux/`, `…/windows/` | the emulator on the shelf, one folder per system |
| `emulators/<name>/data-linux/`, `…/data-windows/` | its saves, save states and settings; the newest file wins, nothing is deleted |
| `~/Games/Emulation/Emulators/<name>/` | the emulator on this PC (Settings → This PC → Emulator games go to) |

Each emulator is set up to keep everything in its own folder:
- an AppImage gets a `<file>.AppImage.home` folder beside it, which it uses as its home folder;
- Windows builds get their portable marker (`portable.txt`, `portable.ini` or a `portable/` folder);
- Dolphin and Ryujinx as packages get `-u` / `-r` pointing at a data folder.

Caches (shader caches above all), logs and crash dumps aren't synced. Each PC builds its own.

Firmware and keys (Ryujinx `system/`, RPCS3's firmware, BIOS files for PCSX2 and DuckStation) live in the emulator's data folder. So once you've added them on one PC, every PC gets them.

## Controllers

Steam passes every controller to the games it starts as one standard Xbox-style controller (Steam Input). That's true for PlayStation, Switch Pro and 8BitDo controllers, a Steam Deck, and **the Steam Link app's touch controls**. So:

1. **Set up the controls in each emulator once**, on any PC, with any controller. Most emulators have an "automatic mapping" button.
2. Steam-Fan syncs the emulator's settings, so every PC and every controller uses that same setup.

If a controller does nothing in a game, make sure Steam Input is on for that game: in Steam, right-click it → **Properties → Controller → Enable Steam Input**.

## Steam Link (phone, tablet, TV)

Steam-Fan's games are normal Steam entries, so the Steam Link app streams them: fan games, non-Steam games and emulator games.

- **At home:** open the Steam Link app on the phone and pick your PC.
- **Away from home:** in Steam on the PC, go to **Settings → Remote Play** and turn it on. Then sign in to the same Steam account in the Steam Link app. It connects through Valve's servers, and the games still come from your NAS at home.
- **Touch controls:** they appear when no controller is connected to the phone. Non-Steam games start in mouse mode. While streaming, tap **…** → touch controls, switch to **Controller**, and pick a gamepad layout. Steam Link remembers it per game. For games that only read the keyboard, set Steam Input to send keys: in Steam on the PC, **Properties → Controller → Enable Steam Input → Controller Layout → Templates → "Gamepad with Keyboard"**.
- If the "copying from the shelf" window is in the way the first time you play a game, wait until it's done. The game starts right after it.
