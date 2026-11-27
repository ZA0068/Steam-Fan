// Built-in skins and icon sets. A skin sets the colors and sizes (as a starting point you can
// change), the library layout, the icons, and optionally a stylesheet of its own.
// Your own skins are folders in the skins folder (Settings > Appearance > Open skins folder):
//   skin.json   { "name", "author", "description", "theme": {...}, "layout": "grid|list|shelves",
//                 "cover": 190, "icon_set": "glyph|line", "icons": { "library": "icons/lib.svg", "play": "►" } }
//   style.css   anything else; url(...) inside it is relative to the skin's folder
//   icons/, fonts, pictures: whatever skin.json and style.css point to

const svg = (body) =>
  `<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;

export const ICON_NAMES = ["library", "pc", "tags", "fan", "nonsteam", "rom", "steam", "settings", "add", "play", "search", "refresh", "artwork", "update", "download", "remove", "edit", "grid", "list", "shelves"];

export const ICON_SETS = {
  glyph: {
    library: "▦", pc: "⬇", tags: "🏷", fan: "★", nonsteam: "◆", rom: "◉", steam: "▲", settings: "⚙", add: "＋", play: "▶",
    search: "⌕", refresh: "⟳", artwork: "🖼", update: "⇪", download: "⬇", remove: "🗑", edit: "✎", grid: "▦", list: "☰", shelves: "▤",
  },
  line: {
    library: svg('<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>'),
    pc: svg('<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>'),
    tags: svg('<path d="M3 12V4h8l10 10-8 8z"/><circle cx="7.5" cy="8.5" r="1.3"/>'),
    fan: svg('<path d="M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1L3.2 9.5l6.1-.9z"/>'),
    nonsteam: svg('<rect x="2" y="7" width="20" height="10" rx="5"/><path d="M7 10v4M5 12h4"/><circle cx="16" cy="11" r=".8"/><circle cx="18" cy="13" r=".8"/>'),
    rom: svg('<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="2.5"/>'),
    steam: svg('<rect x="3" y="4" width="18" height="5" rx="1"/><path d="M5 9v10h14V9M10 13h4"/>'),
    settings: svg('<path d="M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1"/><circle cx="15" cy="6" r="2"/><circle cx="9" cy="12" r="2"/><circle cx="17" cy="18" r="2"/>'),
    add: svg('<path d="M12 5v14M5 12h14"/>'),
    play: svg('<path d="M7 4.5l12.5 7.5L7 19.5z" fill="currentColor"/>'),
    search: svg('<circle cx="11" cy="11" r="7"/><path d="M20 20l-4-4"/>'),
    refresh: svg('<path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/>'),
    artwork: svg('<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="2"/><path d="M21 16l-5-5-9 9"/>'),
    update: svg('<path d="M12 16V4M7 9l5-5 5 5M4 20h16"/>'),
    download: svg('<path d="M12 4v12M7 11l5 5 5-5M4 20h16"/>'),
    remove: svg('<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/>'),
    edit: svg('<path d="M4 20l4-1 11-11-3-3L5 16z"/>'),
    grid: svg('<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>'),
    list: svg('<path d="M9 6h12M9 12h12M9 18h12"/><path d="M4 6h1M4 12h1M4 18h1"/>'),
    shelves: svg('<rect x="3" y="5" width="5" height="8" rx="1"/><rect x="10" y="5" width="5" height="8" rx="1"/><rect x="17" y="5" width="4" height="8" rx="1"/><path d="M3 18h18"/>'),
  },
};

const T = (o) => ({ preset: "default", colors: {}, radius: 12, font_size: 14, font: "", density: "normal", sidebar: "left", glow: true, ...o });

export const BUILTIN = {
  default: {
    name: "Steam-Fan",
    description: "The standard look: covers in a grid, a sidebar of sections.",
    theme: T({}),
    layout: "grid",
    icon_set: "glyph",
    css: "",
  },
  cards: {
    name: "Cards",
    description: "Big rounded cards with bold titles, in the style of self-hosted libraries like Gameyfin.",
    theme: T({ colors: { bg: "#100f17", panel: "#1b1a27", text: "#eeebf8", accent: "#8b5cf6" }, radius: 18, glow: true }),
    layout: "grid",
    cover: 190,
    icon_set: "line",
    css: `.card { background: var(--panel); border-radius: var(--radius); padding: 8px 8px 12px; box-shadow: 0 8px 24px rgba(0,0,0,.35); transition: transform .18s; }
.card:hover { transform: translateY(-4px); }
.card .art { box-shadow: none !important; transform: none !important; }
.card .title { font-weight: 700; text-align: center; padding: 0 4px; }`,
  },
  details: {
    name: "Details",
    description: "A dense list with every detail in columns, in the style of Playnite's details view.",
    theme: T({ colors: { bg: "#1a1c21", panel: "#23262d", text: "#dde2ea", accent: "#2f9be0" }, radius: 4, font_size: 13, density: "compact", glow: false }),
    layout: "list",
    icon_set: "line",
    css: "",
  },
  media: {
    name: "Media center",
    description: "Big covers in rows with a spotlight, made for the TV and arrow keys, in the style of Kodi.",
    theme: T({ colors: { bg: "#080c14", panel: "#111a2a", text: "#f0f5fd", accent: "#17b2e8" }, radius: 10, font_size: 16, density: "roomy", sidebar: "icons" }),
    layout: "shelves",
    cover: 200,
    icon_set: "line",
    css: "",
  },
  steam: {
    name: "Steam",
    description: "Steam's colors with Steam-Fan's layout.",
    theme: T({ preset: "steam", radius: 4 }),
    layout: "grid",
    icon_set: "glyph",
    css: "",
  },
};
