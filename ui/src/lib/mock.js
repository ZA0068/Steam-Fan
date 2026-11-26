// A demo library for working on the design in a plain browser (npm run dev).
const kinds = { fan: "Fan games", nonsteam: "Non-Steam games", rom: "Emulator games", steam: "Steam archive" };
const mk = (kind, group, name, extra = {}) => ({
  id: `${kind}:${group}/${name}`, name, kind: kind[0].toUpperCase() + kind.slice(1), kind_label: kinds[kind],
  group, system: kind === "rom" ? group : "", platform: kind === "rom" ? "Emulator" : "Windows",
  program: "game.exe", playable: true, on_pc: false, fresh: null, fresh_code: null, size: 0, appid: 0,
  buildid: "", steam_id: null, art: { cover: null, hero: null, logo: null, icon: null }, ...extra,
});
const games = [
  mk("fan", "Sonic The Hedgehog", "Sonic Robo Blast 2", { on_pc: true }),
  mk("fan", "Sonic The Hedgehog", "Sonic 3 A.I.R."),
  mk("fan", "Sonic The Hedgehog", "Sonic Worlds Delta"),
  mk("fan", "Super Mario Bros", "Super Mario 63", { on_pc: true, steam_id: 1 }),
  mk("fan", "Kirby", "Kirby's Dream Course DX"),
  mk("nonsteam", "Demos", "Rog and Roll"),
  mk("rom", "wii", "Super Mario Galaxy [VBKP]"),
  mk("rom", "WIIU", "SUPER MARIO 3D WORLD [ARDP]", { program: "" }),
  mk("steam", "Steam", "Portal 2", { fresh: "up to date", fresh_code: "Same", on_pc: true, size: 12e9, appid: 620 }),
];
export function mock(cmd) {
  switch (cmd) {
    case "library": return { games, shelf: "/mnt/nas-games", unknown_systems: ["Nintendo 64"], has_sgdb_key: false, configured: true };
    case "status": return { shelf_online: true, steam_running: false };
    case "details": return { shelf_path: "/mnt/nas-games/…", local_path: "~/Games/NonSteam/…", saves_path: "/mnt/nas-games/saves/…", rom_command: null, history: [["2026-11-26 07:00", "on this PC"]], art_on_shelf: [] };
    case "jobs": return [];
    case "groups": return { fan: ["Kirby", "Sonic The Hedgehog", "Super Mario Bros"], nonsteam: ["Demos"], rom: ["wii", "WIIU", "Switch"] };
    case "settings": return { config: { shelf: "/mnt/nas-games", fan_local: "~/Games/NonSteam", nonsteam_local: "~/Games/NonSteam/Non-Steam Games", rom_local: "~/Games/Emulation", steam_root: "~/.local/share/Steam", proton: "", steam_api_key: "", steam_id: "", emulators: [] }, config_path: "~/.config/steam-fan/config.toml", secrets_path: "~/.config/steam-fan/secrets.env", keys: { STEAMGRIDDB_API_KEY: { masked: "", source: "not set" }, STEAM_API_KEY: { masked: "", source: "not set" }, STEAM_ID: { masked: "", source: "not set" } } };
    default: return null;
  }
}
