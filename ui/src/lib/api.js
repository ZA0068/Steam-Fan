// Every call into the Rust side. Outside the Tauri window (plain browser,
// `npm run dev`), a small demo library is used so the design can be worked on.
import { mock } from "./mock.js";

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call(cmd, args = {}) {
  if (!inTauri) return mock(cmd, args);
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(cmd, args);
}

export const api = {
  library: () => call("library"),
  status: () => call("status"),
  details: (id) => call("details", { id }),
  jobs: () => call("jobs"),
  cancelJob: (id) => call("job_cancel", { id }),
  refresh: (full = false) => call("refresh", { full }),
  syncSteam: () => call("sync_steam"),
  install: (id) => call("install", { id }),
  remove: (id) => call("remove", { id }),
  archiveUpload: (id) => call("archive_upload", { id }),
  refreshArt: (again = false) => call("refresh_art", { again }),
  artSearch: (id, term) => call("art_search", { id, term }),
  artOptions: (sgdbId, art) => call("art_options", { sgdbId, art }),
  artSetUrl: (id, art, url, sgdbId) => call("art_set_url", { id, art, url, sgdbId }),
  artSetFile: (id, art, path) => call("art_set_file", { id, art, path }),
  artRemove: (id, art) => call("art_remove", { id, art }),
  importInspect: (path) => call("import_inspect", { path }),
  importPlace: (source, kind, group, name, version = "") => call("import_place", { source, kind, group, name, version }),
  groups: () => call("groups"),
  infoSave: (id, info) => call("info_save", { id, info }),
  screenshotsAdd: (id, paths) => call("screenshots_add", { id, paths }),
  screenshotRemove: (id, name) => call("screenshot_remove", { id, name }),
  relocate: (id, kind, group, name, version = "") => call("relocate", { id, kind, group, name, version }),
  syncSteamRestart: () => call("sync_steam_restart"),
  verify: (id) => call("verify", { id }),
  redownload: (id, version = null, file = null) => call("redownload", { id, version, file }),
  checkUpdate: (id) => call("check_update", { id }),
  pageImages: (url) => call("page_images", { url }),
  screenshotsAddUrls: (id, urls) => call("screenshots_add_urls", { id, urls }),
  settings: () => call("settings"),
  saveSettings: (config) => call("settings_save", { config }),
  setSecret: (name, value) => call("secret_set", { name, value }),
  shelfReachable: (path) => call("shelf_reachable", { path }),
  open: (target) => call("open", { target }),
  play: (id) => call("play", { id }),
};

export async function listen(event, fn) {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen(event, (e) => fn(e.payload));
}

/** Files dropped onto the window: fn(paths). */
export async function onDrop(fn, onHover) {
  if (!inTauri) return () => {};
  const { getCurrentWebview } = await import("@tauri-apps/api/webview");
  return getCurrentWebview().onDragDropEvent((e) => {
    const t = e.payload.type;
    if (t === "drop") fn(e.payload.paths);
    if (onHover) onHover(t === "enter" || t === "over");
  });
}

export async function pickFile({ images = false, archives = false, folder = false, multiple = false } = {}) {
  if (!inTauri) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const filters = images
    ? [{ name: "Pictures", extensions: ["png", "jpg", "jpeg", "webp", "ico", "bmp", "gif"] }]
    : archives
      ? [{ name: "Games and archives", extensions: ["zip", "7z", "rar", "tar", "gz", "xz", "zst", "exe", "iso", "wbfs", "rvz", "nsp", "xci", "wua", "wux", "appimage"] }]
      : undefined;
  const r = await open({ directory: folder, multiple, filters });
  return r || null;
}

export function human(bytes) {
  if (!bytes) return "";
  const u = ["B", "kB", "MB", "GB", "TB"];
  let i = 0, b = bytes;
  while (b >= 1000 && i < u.length - 1) { b /= 1000; i++; }
  return `${b.toFixed(b >= 100 || i === 0 ? 0 : 1)} ${u[i]}`;
}
