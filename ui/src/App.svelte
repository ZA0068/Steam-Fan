<script>
  import { onMount } from "svelte";
  import { api, listen, onDrop } from "./lib/api.js";
  import Sidebar from "./lib/Sidebar.svelte";
  import Library from "./lib/Library.svelte";
  import Details from "./lib/Details.svelte";
  import Settings from "./lib/Settings.svelte";
  import AddGame from "./lib/AddGame.svelte";
  import Jobs from "./lib/Jobs.svelte";
  import * as theme from "./lib/theme.js";

  let lib = $state({ games: [], shelf: "", unknown_systems: [], has_sgdb_key: false, configured: true });
  let status = $state({ shelf_online: false, steam_running: false });
  // Where you are, Steam-style: every place you go is a step in the history; ‹ › walk it.
  // page: library | settings; view: a sidebar section; group/filter/search narrow it;
  // selected: an open game, with its tab.
  const home = { page: "library", view: "all", group: "", filter: "", search: "", selected: null, tab: "overview" };
  let hist = $state([home]);
  let pos = $state(0);
  const here = $derived(hist[pos]);
  const page = $derived(here.page);
  const view = $derived(here.view);
  const selected = $derived(here.selected);
  const same = (a, b) => Object.keys(home).every((k) => a[k] === b[k]);
  function go(change) {
    const next = { ...here, ...change };
    if (same(next, here)) return;
    hist = [...hist.slice(0, pos + 1), next].slice(-100);
    pos = hist.length - 1;
  }
  /** A section from the sidebar or a tag: a fresh list. */
  const goList = (view, more = {}) => go({ page: "library", view, group: "", filter: "", search: "", selected: null, tab: "overview", ...more });
  const back = () => pos > 0 && (pos -= 1);
  const forward = () => pos < hist.length - 1 && (pos += 1);
  /** A game was moved or renamed: its old id is gone everywhere in the history. */
  function renamed(old, id) {
    hist = hist.map((h) => (h.selected === old ? { ...h, selected: id } : h));
  }
  let adding = $state(null); // null: closed, "": open empty, "/path": open with a file
  let jobs = $state([]);
  let dragging = $state(false);
  let loaded = $state(false);
  /** Whoever wants dropped files right now (the artwork picker), else Add game. */
  let dropTarget = $state(null);

  async function load() {
    try {
      lib = await api.library();
    } finally {
      loaded = true;
    }
  }
  async function poll() {
    try {
      status = await api.status();
    } catch {}
  }

  const busy = $derived(jobs.some((j) => !j.done));
  const game = $derived(lib.games.find((g) => g.id === selected) ?? null);

  const titles = { all: "Library", pc: "On this PC", tags: "Collections", Fan: "Fan games", NonSteam: "Non-Steam games", Rom: "Emulator games", Steam: "Steam archive" };
  // soft groups: every collection name in use
  const allTags = $derived([...new Set(lib.games.flatMap((g) => g.tags ?? []))].sort((a, b) => a.localeCompare(b)));
  const filters = { shelf: "Only on the shelf", windows: "Windows · Proton", linux: "Linux" };
  // the path shown next to the arrows; every part but the last can be clicked
  const crumbs = $derived.by(() => {
    if (page === "settings") return [{ label: "Settings" }];
    if (game) {
      const c = [{ label: titles[game.kind], to: () => goList(game.kind) }];
      if (game.group) c.push({ label: game.group, to: () => goList(game.kind, { group: game.group }) });
      c.push({ label: game.version ? `${game.name} (${game.version})` : game.name });
      return c;
    }
    const c = [{ label: titles[view], to: () => goList(view) }];
    if (here.group) c.push({ label: here.group, to: () => goList(view, { group: here.group }) });
    if (here.filter) c.push({ label: filters[here.filter] ?? here.filter });
    if (here.search) c.push({ label: `“${here.search}”` });
    c[c.length - 1].to = null;
    return c;
  });

  // mouse side buttons and Alt+←/→, like Steam and browsers
  // Ctrl+Shift+0 (⌘⇧0 on a Mac): back to the default look, whatever a theme did
  let toast = $state("");
  async function defaultLook() {
    theme.apply(theme.DEFAULT);
    try {
      await theme.save(theme.DEFAULT);
      toast = "Default look restored. Settings › Appearance › Undo last save brings yours back.";
    } catch {
      toast = "Default look for now (it couldn't be saved)";
    }
    window.dispatchEvent(new Event("sf-theme-reset"));
    setTimeout(() => (toast = ""), 6000);
  }

  function onkey(e) {
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.code === "Digit0") {
      e.preventDefault();
      defaultLook();
      return;
    }
    if (!e.altKey || e.ctrlKey || e.metaKey) return;
    if (e.key === "ArrowLeft") { e.preventDefault(); back(); }
    if (e.key === "ArrowRight") { e.preventDefault(); forward(); }
  }
  function onmouse(e) {
    if (e.button === 3) { e.preventDefault(); back(); }
    if (e.button === 4) { e.preventDefault(); forward(); }
  }

  onMount(() => {
    load();
    poll();
    api.jobs().then((j) => (jobs = j ?? []));
    const t = setInterval(poll, 4000);
    const un = [
      listen("jobs", (j) => (jobs = j)),
      listen("library-changed", load),
      onDrop(
        (paths) => {
          dragging = false;
          if (!paths?.length) return;
          if (dropTarget) dropTarget(paths);
          else adding = paths[0];
        },
        (h) => (dragging = h),
      ),
    ];
    // first start with a reachable shelf: look for changes
    setTimeout(async () => {
      await poll();
      if (status.shelf_online && lib.configured) api.refresh(false);
      if (!lib.configured) go({ page: "settings", selected: null });
    }, 300);
    return () => {
      clearInterval(t);
      un.forEach((p) => p.then((f) => f && f()));
    };
  });
</script>

<svelte:window onkeydown={onkey} onmouseup={onmouse} />

<div class="shell">
  <Sidebar
    games={lib.games}
    view={game ? "" : view}
    {page}
    {status}
    shelf={lib.shelf}
    onview={(v) => goList(v)}
    onsettings={() => go({ page: "settings", selected: null })}
    onadd={() => (adding = "")}
  />
  <main>
    <div class="topnav">
      <button class="arrow" disabled={pos === 0} onclick={back} title="Back (Alt+←, mouse back button)">‹</button>
      <button class="arrow" disabled={pos >= hist.length - 1} onclick={forward} title="Forward (Alt+→)">›</button>
      <nav class="crumbs">
        {#each crumbs as c, i}
          {#if i}<span class="sep">›</span>{/if}
          {#if c.to}<button class="crumb" onclick={c.to}>{c.label}</button>{:else}<span class="crumb here">{c.label}</span>{/if}
        {/each}
      </nav>
    </div>
    <div class="view">
      {#if page === "settings"}
        <Settings onsaved={() => { load(); poll(); goList("all"); api.refresh(true); }} />
      {:else if game}
        <Details
          {game}
          {allTags}
          {status}
          {busy}
          tab={here.tab}
          hasKey={lib.has_sgdb_key}
          ontab={(t) => go({ tab: t })}
          onnav={(view, more) => goList(view, more)}
          ondroptarget={(f) => (dropTarget = f)}
          onchanged={load}
          onmoved={async (id) => { const old = game.id; await load(); renamed(old, id); }}
        />
      {:else}
        {#key `${view}|${here.group}|${here.filter}|${here.search}`}
          <Library
            games={lib.games}
            {view}
            group={here.group}
            filter={here.filter}
            filterLabel={filters[here.filter] ?? ""}
            initialSearch={here.search}
            {status}
            {busy}
            {loaded}
            hasKey={lib.has_sgdb_key}
            unknown={lib.unknown_systems}
            onopen={(id) => go({ selected: id, tab: "overview" })}
            onnav={(more) => go(more)}
            onadd={() => (adding = "")}
          />
        {/key}
      {/if}
    </div>
  </main>
</div>

{#if adding !== null}
  <AddGame
    path={adding}
    games={lib.games}
    online={status.shelf_online}
    onclose={() => { adding = null; dropTarget = null; }}
    ondroptarget={(f) => (dropTarget = f)}
  />
{/if}

{#if dragging && !dropTarget && adding === null}
  <div class="dropping">
    <div>
      <img src="/logo.svg" alt="" />
      <h2>Drop to add a game to the shelf</h2>
      <p class="muted">A zip, 7z or rar download, a game folder, or a ROM file</p>
    </div>
  </div>
{/if}

<Jobs {jobs} oncancel={(id) => api.cancelJob(id)} />

{#if toast}
  <div style="position:fixed !important;left:50% !important;top:18px !important;transform:translateX(-50%) !important;z-index:2147483647 !important;padding:10px 16px !important;background:#101826 !important;color:#fff !important;border:2px solid #4b93ff !important;border-radius:12px !important;font:14px system-ui,sans-serif !important;display:block !important;visibility:visible !important;opacity:1 !important;">{toast}</div>
{/if}

<style>
  .shell { display: grid; grid-template-columns: var(--sidebar-w, 232px) 1fr; height: 100vh; }
  /* sidebar on the right: same grid, other order */
  :global(html.side-right) .shell { grid-template-columns: 1fr var(--sidebar-w, 232px); }
  :global(html.side-right) .shell > main { order: -1; }
  :global(html.side-icons) { --sidebar-w: 64px; }
  main { display: flex; flex-direction: column; }
  .view { flex: 1; min-height: 0; position: relative; }
  .topnav { display: flex; align-items: center; gap: 4px; height: 42px; padding: 0 16px; border-bottom: 1px solid var(--line); background: rgba(var(--bg-rgb), .6); flex: none; }
  .arrow { width: 30px; height: 30px; border: 0; border-radius: var(--r); background: transparent; color: var(--text); font-size: 22px; line-height: 1; cursor: pointer; display: grid; place-items: center; padding-bottom: 3px; }
  .arrow:hover:not(:disabled) { background: var(--panel); }
  .arrow:disabled { color: var(--faint); opacity: .45; cursor: default; }
  .crumbs { display: flex; align-items: center; gap: 2px; margin-left: 10px; min-width: 0; overflow: hidden; white-space: nowrap; font-size: 13px; }
  .crumb { border: 0; background: none; color: var(--muted); padding: 4px 6px; border-radius: 6px; cursor: pointer; overflow: hidden; text-overflow: ellipsis; }
  button.crumb:hover { color: var(--text); background: var(--panel); }
  .crumb.here { color: var(--text); cursor: default; font-weight: 600; }
  .sep { color: var(--faint); }
  main { min-width: 0; overflow: hidden; position: relative; background: radial-gradient(1200px 600px at 70% -10%, rgba(var(--accent-2-rgb), calc(0.12 * var(--glow))), transparent 60%), var(--bg); }
  .dropping { position: fixed; inset: 10px; border: 2px dashed var(--accent); border-radius: calc(var(--radius) + 6px); background: rgba(var(--bg-rgb), 0.85); display: grid; place-items: center; text-align: center; z-index: 60; pointer-events: none; animation: fade .12s; }
  .dropping img { width: 88px; opacity: .9; }
  .dropping h2 { margin: 14px 0 6px; }
</style>
