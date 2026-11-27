// Looks: colors, sizes, layout and custom CSS.
// A theme is kept in theme.json, apart from the settings, so a bad theme can never stop
// Steam-Fan from starting. Anything in it that doesn't make sense falls back to the default.
// Ways back to the default look, whatever a theme did:
//   Settings > Appearance > Default look;  Ctrl+Shift+0 anywhere;  `steam-fan-app --default-theme`;
//   deleting theme.json. Custom CSS only stays after you press Keep (it reverts by itself otherwise).
import { api } from "./api.js";
import { look } from "./look.svelte.js";
import { BUILTIN, ICON_SETS, ICON_NAMES } from "./skins.js";

export const PRESETS = {
  default: { label: "Steam-Fan", bg: "#0a111e", panel: "#121d31", text: "#e8eef8", accent: "#4b93ff" },
  steam: { label: "Steam", bg: "#171d25", panel: "#1f2a37", text: "#c7d5e0", accent: "#66c0f4" },
  oled: { label: "Black (OLED)", bg: "#000000", panel: "#0e0e11", text: "#f1f1f1", accent: "#4b93ff" },
  emerald: { label: "Green Hill", bg: "#07130e", panel: "#0f2119", text: "#e5f4ea", accent: "#34c27a" },
  gold: { label: "Super", bg: "#0a0f24", panel: "#131b3d", text: "#eef1ff", accent: "#ffc21a" },
  rose: { label: "Amy", bg: "#170a14", panel: "#251225", text: "#f6e8f1", accent: "#ff5fa2" },
  light: { label: "Light", bg: "#eef2f8", panel: "#ffffff", text: "#172033", accent: "#2f6fe0" },
};

export const DEFAULT = Object.freeze({
  v: 1,
  skin: "default", // a built-in skin's name, or "user:<folder>" for one in the skins folder
  preset: "default",
  colors: {}, // your own colors on top of the preset: bg, panel, text, accent, good, warn, bad
  radius: 12,
  font_size: 14,
  font: "",
  density: "normal", // compact | normal | roomy
  sidebar: "left", // left | right | icons
  glow: true,
  css: "",
});

const HEX = /^#[0-9a-f]{6}$/i;
const COLOR_KEYS = ["bg", "panel", "text", "accent", "good", "warn", "bad"];
const clamp = (n, lo, hi, d) => (Number.isFinite(+n) ? Math.min(hi, Math.max(lo, +n)) : d);

/** Whatever was stored, as a theme that is safe to apply. */
export function clean(t) {
  if (!t || typeof t !== "object") return structuredClone(DEFAULT);
  const colors = {};
  for (const k of COLOR_KEYS) if (typeof t.colors?.[k] === "string" && HEX.test(t.colors[k])) colors[k] = t.colors[k].toLowerCase();
  const skin = typeof t.skin === "string" && (BUILTIN[t.skin] || /^user:[^/\\.][^/\\]{0,59}$/.test(t.skin)) ? t.skin : "default";
  return {
    v: 1,
    skin,
    preset: PRESETS[t.preset] ? t.preset : "default",
    colors,
    radius: clamp(t.radius, 0, 24, DEFAULT.radius),
    font_size: clamp(t.font_size, 11, 20, DEFAULT.font_size),
    font: typeof t.font === "string" ? t.font.replace(/[;{}<>]/g, "").slice(0, 200) : "",
    density: ["compact", "normal", "roomy"].includes(t.density) ? t.density : "normal",
    sidebar: ["left", "right", "icons"].includes(t.sidebar) ? t.sidebar : "left",
    glow: t.glow !== false,
    css: typeof t.css === "string" ? t.css.slice(0, 50000) : "",
  };
}

const rgb = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
const hex = (c) => "#" + c.map((x) => Math.round(x).toString(16).padStart(2, "0")).join("");
/** a toward b by t (0..1) */
const mix = (a, b, t) => hex(rgb(a).map((x, i) => x + (rgb(b)[i] - x) * t));
const lum = (h) => {
  const [r, g, b] = rgb(h).map((x) => ((x /= 255) <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};

/** The CSS variables for a theme's colors. The plain default leaves the stylesheet as it is. */
export function variables(t) {
  if (t.preset === "default" && !Object.keys(t.colors).length) return {};
  const c = { good: "#46d68f", warn: "#f3b94c", bad: "#ff6f6f", ...PRESETS[t.preset], ...t.colors };
  const light = lum(c.bg) > 0.45;
  const v = {
    bg: c.bg,
    "bg-2": mix(c.bg, c.panel, 0.4),
    panel: c.panel,
    "panel-2": mix(c.panel, c.accent, light ? 0.06 : 0.09),
    line: mix(mix(c.panel, c.accent, 0.1), c.text, light ? 0.1 : 0.06),
    "line-2": mix(mix(c.panel, c.accent, 0.16), c.text, light ? 0.16 : 0.1),
    text: c.text,
    muted: mix(c.text, c.bg, 0.38),
    faint: mix(c.text, c.bg, 0.58),
    accent: c.accent,
    "accent-2": mix(c.accent, c.bg, 0.35),
    good: c.good,
    warn: c.warn,
    bad: c.bad,
    sidebar: mix(c.bg, c.panel, 0.3),
    hover: mix(mix(c.panel, c.accent, 0.09), c.accent, 0.1),
    "accent-text": mix(c.accent, c.text, light ? 0.25 : 0.7),
    "on-accent": lum(c.accent) > 0.5 ? "#10131a" : "#ffffff",
  };
  for (const k of ["bg", "panel", "panel-2", "accent", "accent-2", "good", "warn", "bad"]) v[`${k}-rgb`] = rgb(v[k]).join(", ");
  v.shadow = light ? "0 10px 30px rgba(20, 30, 60, 0.15)" : "0 10px 30px rgba(0, 0, 0, 0.35)";
  return v;
}

let applied = [];

// ---------------------------------------------------------------- skins

const skins = new Map();

/** A skin's parts: theme, layout, cover size, icons (resolved) and stylesheet. */
export async function skinData(id) {
  if (skins.has(id)) return skins.get(id);
  let sk;
  if (BUILTIN[id]) {
    sk = { ...BUILTIN[id], id, builtin: true, icons: { ...ICON_SETS[BUILTIN[id].icon_set ?? "glyph"] } };
  } else if (id.startsWith("user:")) {
    const f = await api.skinLoad(id.slice(5));
    const j = JSON.parse(f.json);
    const base = f.base;
    const rel = (u) => (/^(https?:|data:|art:|#|\/)/.test(u) ? u : `${base}/${u.replace(/^\.\//, "")}`);
    const icons = { ...ICON_SETS[ICON_SETS[j.icon_set] ? j.icon_set : "glyph"] };
    for (const [k, v] of Object.entries(j.icons ?? {})) {
      // a picture file in the skin, or a few characters; never markup (it could run code)
      if (!ICON_NAMES.includes(k) || typeof v !== "string" || v.includes("<")) continue;
      icons[k] = /\.(svg|png|webp|jpe?g|gif)$/i.test(v) ? rel(v) : v.slice(0, 8);
    }
    sk = {
      id,
      builtin: false,
      name: j.name ?? id.slice(5),
      description: j.description ?? "",
      author: j.author ?? "",
      theme: j.theme ?? {},
      layout: ["grid", "list", "shelves"].includes(j.layout) ? j.layout : "grid",
      cover: Number.isFinite(+j.cover) ? Math.min(320, Math.max(90, +j.cover)) : 0,
      icon_set: j.icon_set ?? "glyph",
      icons,
      // url(...) in a skin's stylesheet points into the skin's folder
      css: (f.css ?? "").replace(/url\(\s*(['"]?)([^'")]+)\1\s*\)/g, (_, q, u) => `url("${rel(u.trim())}")`),
    };
  } else {
    return skinData("default");
  }
  skins.set(id, sk);
  return sk;
}

/** Forget loaded skins (after one was installed, changed or removed). */
export const forgetSkins = () => skins.clear();

function styleTag(id) {
  let el = document.getElementById(id);
  if (!el) {
    el = document.createElement("style");
    el.id = id;
    document.head.appendChild(el);
  }
  return el;
}

let skinCall = 0;
async function applySkin(id, withCss) {
  const n = ++skinCall;
  let sk;
  try {
    sk = await skinData(id);
  } catch {
    sk = await skinData("default");
  }
  if (n !== skinCall) return; // a newer look was put on meanwhile
  look.skin = sk.id;
  look.icons = sk.icons;
  look.layout = sk.layout;
  look.cover = sk.cover ?? 0;
  // the built-in skins' styles are part of Steam-Fan; a skin from the skins folder needs Keep first
  styleTag("sf-skin-css").textContent = withCss || sk.builtin ? sk.css : "";
}

/**
 * Put a theme on the window. `withCss` false: everything but custom CSS (yours, and a
 * stylesheet from the skins folder); `skinCss` decides the skin's stylesheet on its own.
 */
export function apply(t, withCss = true, skinCss = withCss) {
  t = clean(t);
  const root = document.documentElement;
  for (const k of applied) root.style.removeProperty(k);
  applied = [];
  const set = (k, val) => {
    root.style.setProperty(k, val);
    applied.push(k);
  };
  const vars = variables(t);
  for (const [k, val] of Object.entries(vars)) set(`--${k}`, val);
  root.style.colorScheme = vars.bg && lum(vars.bg) > 0.45 ? "light" : "dark";
  set("--radius", `${t.radius}px`);
  set("--r", `${Math.round(t.radius * 0.75)}px`);
  if (t.font_size !== DEFAULT.font_size) set("--font-size", `${t.font_size}px`);
  if (t.font.trim()) set("--font", `${t.font}, "Inter", "Noto Sans", "Segoe UI", system-ui, sans-serif`);
  set("--glow", t.glow ? "1" : "0");
  root.classList.remove("compact", "roomy", "side-right", "side-icons");
  if (t.density !== "normal") root.classList.add(t.density);
  if (t.sidebar !== "left") root.classList.add(`side-${t.sidebar}`);
  // the skin's stylesheet first, yours after it (so yours wins)
  styleTag("sf-skin-css");
  styleTag("sf-custom-css").textContent = withCss ? t.css : "";
  applySkin(t.skin, skinCss);
}

/** The saved theme (or the default when there's none, it's damaged, or the app was started with --default-theme). */
export async function load() {
  try {
    const f = await api.themeLoad();
    if (f.forced_default) return { theme: structuredClone(DEFAULT), forced: true, damaged: false, path: f.path };
    if (!f.json.trim()) return { theme: structuredClone(DEFAULT), forced: false, damaged: false, path: f.path };
    try {
      return { theme: clean(JSON.parse(f.json)), forced: false, damaged: false, path: f.path };
    } catch {
      return { theme: structuredClone(DEFAULT), forced: false, damaged: true, path: f.path };
    }
  } catch {
    return { theme: structuredClone(DEFAULT), forced: false, damaged: false, path: "" };
  }
}

export const save = (t) => api.themeSave(JSON.stringify(clean(t), null, 2));

/**
 * "Keep this look?" for new custom CSS. Drawn outside the page (on <html>, in its own
 * shadow root) so CSS that hides or restyles the page can't hide it; and if it's hidden
 * anyway, the look is taken back when the time runs out. Esc takes it back at once.
 */
export function confirmLook(seconds, onkeep, onrevert) {
  const host = document.createElement("div");
  host.style.cssText = "all:initial;position:fixed;left:0;right:0;bottom:24px;z-index:2147483647;display:flex;justify-content:center;pointer-events:none;";
  const root = host.attachShadow({ mode: "open" });
  root.innerHTML = `
    <style>
      .bar { pointer-events: auto; display: flex; gap: 12px; align-items: center; padding: 12px 16px; background: #101826; color: #fff;
             border: 2px solid #4b93ff; border-radius: 12px; font: 14px system-ui, sans-serif; box-shadow: 0 10px 30px rgba(0,0,0,.5); }
      button { cursor: pointer; padding: 6px 12px; border: 0; border-radius: 8px; color: #fff; font: inherit; }
      .keep { background: #4b93ff; font-weight: 600; }
      .back { background: #2a3446; }
    </style>
    <div class="bar"><span></span><button class="keep">Keep</button><button class="back">Take back</button></div>`;
  const text = root.querySelector("span");
  let left = seconds;
  const show = () => (text.textContent = `Keep this look? Taken back in ${left} s (Esc).`);
  show();
  document.documentElement.appendChild(host);
  const done = (keep) => {
    clearInterval(timer);
    window.removeEventListener("keydown", onkey, true);
    host.remove();
    (keep ? onkeep : onrevert)();
  };
  const onkey = (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      done(false);
    }
  };
  const timer = setInterval(() => {
    left -= 1;
    if (left <= 0) done(false);
    else show();
  }, 1000);
  window.addEventListener("keydown", onkey, true);
  root.querySelector(".keep").onclick = () => done(true);
  root.querySelector(".back").onclick = () => done(false);
  return () => done(false);
}
