const mockTheme = { now: "", prev: "" };
// A demo library for working on the design in a plain browser (npm run dev).
const kinds = { fan: "Fan games", nonsteam: "Non-Steam games", rom: "Emulator games", steam: "Steam archive" };
const mk = (kind, group, name, extra = {}) => ({
  id: `${kind}:${group}/${name}`, name, kind: kind[0].toUpperCase() + kind.slice(1), kind_label: kinds[kind],
  group, system: kind === "rom" ? group : "", platform: kind === "rom" ? "Emulator" : "Windows",
  program: "game.exe", playable: true, on_pc: false, fresh: null, fresh_code: null, size: 0, appid: 0,
  buildid: "", steam_id: null, art: { cover: null, hero: null, logo: null, icon: null }, tags: [], ...extra,
});
const games = [
  mk("fan", "Sonic The Hedgehog", "Sonic Robo Blast 2", { on_pc: true, tags: ["Favourites", "Multiplayer"] }),
  mk("fan", "Sonic The Hedgehog", "Sonic 3 A.I.R."),
  mk("fan", "Sonic The Hedgehog", "Sonic Worlds Delta"),
  mk("fan", "Super Mario Bros", "Super Mario 63", { on_pc: true, steam_id: 1, tags: ["Favourites"] }),
  mk("fan", "Kirby", "Kirby's Dream Course DX"),
  mk("fan", "Sonic The Hedgehog", "Sonic World", {
    version: "DX 1.2.4", version_folder: "Sonic World DX 1.2.4", version_id: "fan:Sonic The Hedgehog/Sonic World/Sonic World DX 1.2.4", pinned: false, on_pc: true,
    versions: [
      { id: "v1", folder: "Sonic World DX 1.2.4", label: "DX 1.2.4", on_pc: true, playable: true, built: 1702425600, old: false, art: { cover: null, hero: null, logo: null, icon: null }, own_art: [], own_info: true },
      { id: "v2", folder: "Sonic World Release 10", label: "Release 10", on_pc: false, playable: true, built: 1596153600, old: false, art: { cover: null, hero: null, logo: null, icon: null }, own_art: [], own_info: false },
      { id: "v3", folder: "Sonic World Release 9", label: "Release 9", on_pc: false, playable: true, built: 1542585600, old: false, art: { cover: null, hero: null, logo: null, icon: null }, own_art: [], own_info: false },
      { id: "v4", folder: "Old", label: "Old", on_pc: false, playable: true, built: 1390000000, old: true, art: { cover: null, hero: null, logo: null, icon: null }, own_art: [], own_info: false },
    ],
  }),
  mk("nonsteam", "Demos", "Rog and Roll"),
  mk("rom", "wii", "Super Mario Galaxy [VBKP]"),
  mk("rom", "WIIU", "SUPER MARIO 3D WORLD [ARDP]", { program: "" }),
  mk("steam", "Steam", "Portal 2", { fresh: "up to date", fresh_code: "Same", on_pc: true, size: 12e9, appid: 620 }),
];
export function mock(cmd, args = {}) {
  switch (cmd) {
    case "library": return { games, shelf: "/mnt/nas-games", unknown_systems: ["Nintendo 64"], has_sgdb_key: false, configured: true };
    case "status": return { shelf_online: true, steam_running: false, via: "folder /mnt/nas-games" };
    case "details": return { own: args.version ? { description: "", developer: "", released: "", links: [], page: "", download: "" } : null, inherited: args.version ? { description: "The newest release's text", developer: "Fans", released: "2023", links: [], page: "", download: "" } : null, info: { description: "", developer: "", released: "", links: [], page: "", download: "" }, screenshots: [], shelf_path: "/mnt/nas-games/…", local_path: "~/Games/NonSteam/…", saves_path: "/mnt/nas-games/saves/…", rom_command: null, history: [["2026-11-26 07:00", "on this PC"]], art_on_shelf: [] };
    case "jobs": return [];
    case "groups": return { fan: ["Kirby", "Sonic The Hedgehog", "Super Mario Bros"], nonsteam: ["Demos"], rom: ["wii", "WIIU", "Switch"] };
    case "settings": return { config: { shelf: "/mnt/nas-games", fan_local: "~/Games/NonSteam", nonsteam_local: "~/Games/NonSteam/Non-Steam Games", rom_local: "~/Games/Emulation", steam_root: "~/.local/share/Steam", proton: "", steam_api_key: "", steam_id: "", emulators: [{ system: "wiiu", uses: "cemu", games_dir: "", extensions: ["rpx", "wua"], command: [], save_dirs: [], skip: [] }, { system: "switch", uses: "ryujinx", games_dir: "Games", extensions: ["nsp", "xci"], command: [], save_dirs: [], skip: [] }], servers: [], wine: [] }, config_path: "~/.config/steam-fan/config.toml", secrets_path: "~/.config/steam-fan/secrets.env", keys: { STEAMGRIDDB_API_KEY: { masked: "", source: "not set" }, STEAM_API_KEY: { masked: "", source: "not set" }, STEAM_ID: { masked: "", source: "not set" } }, paired: false };
    case "server_pair": return [args.url];
    case "theme_load": return { json: mockTheme.now, forced_default: false, path: "~/.config/steam-fan/theme.json", previous: mockTheme.prev };
    case "emulators_info": return {
      os: "linux", server: true,
      on_shelf: [{ id: "cemu", os: "linux", version: "v2.6", size: 52000000 }, { id: "ryujinx", os: "linux", version: "by hand", size: 120000000 }],
      catalog: [
        { id: "dolphin", name: "Dolphin", systems: ["wii", "gc"], extensions: ["iso", "wbfs", "rvz"], page: "https://dolphin-emu.org/download/", builds: ["windows"], github: [], installed: ["dolphin-emu"], installed_here: true, args: ["-b", "-e", "{rom}"] },
        { id: "cemu", name: "Cemu", systems: ["wiiu"], extensions: ["rpx", "wua"], page: "https://cemu.info/", builds: ["linux", "windows"], github: ["linux", "windows"], installed: ["cemu"], installed_here: false, args: ["-f", "-g", "{rom}"] },
        { id: "ryujinx", name: "Ryujinx", systems: ["switch"], extensions: ["nsp", "xci"], page: "https://ryujinx.app/", builds: ["linux", "windows"], github: [], installed: ["ryujinx"], installed_here: false, args: ["--fullscreen", "{rom}"] },
        { id: "pcsx2", name: "PCSX2", systems: ["ps2"], extensions: ["iso", "chd"], page: "https://pcsx2.net/downloads", builds: ["linux", "windows"], github: ["linux", "windows"], installed: ["pcsx2-qt"], installed_here: false, args: ["-fullscreen", "-batch", "--", "{rom}"] },
      ] };
    case "emulator_get": case "emulator_add": return 7;
    case "skins_list": return [{ id: "My Neon", name: "My Neon", author: "you", description: "An exported look" }];
    case "skin_load": return { json: JSON.stringify({ name: "My Neon", theme: { colors: { bg: "#0b0013", panel: "#1a0630", text: "#ffe9ff", accent: "#ff2bd6" }, radius: 2 }, layout: "grid", icon_set: "line" }), css: ".card .title { text-transform: uppercase; letter-spacing: 1px }", base: "art://localhost/skin/My%20Neon" };
    case "skin_install": return "My Neon";
    case "skin_export": return args.name;
    case "skins_folder": return "~/.config/steam-fan/skins";
    case "skin_remove": return null;
    case "theme_save": mockTheme.prev = mockTheme.now; mockTheme.now = args.json; return null;
    case "server_reachable": return true;
    case "server_discover": return ["http://10.0.0.36:8484"];
    default: return null;
  }
}
