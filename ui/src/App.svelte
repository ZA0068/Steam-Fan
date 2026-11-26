<script>
  import { onMount } from "svelte";
  import { api, listen, onDrop } from "./lib/api.js";
  import Sidebar from "./lib/Sidebar.svelte";
  import Library from "./lib/Library.svelte";
  import Details from "./lib/Details.svelte";
  import Settings from "./lib/Settings.svelte";
  import AddGame from "./lib/AddGame.svelte";
  import Jobs from "./lib/Jobs.svelte";

  let lib = $state({ games: [], shelf: "", unknown_systems: [], has_sgdb_key: false, configured: true });
  let status = $state({ shelf_online: false, steam_running: false });
  let view = $state("all");
  let page = $state("library");
  let selected = $state(null);
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
          if (dropTarget) dropTarget(paths[0]);
          else adding = paths[0];
        },
        (h) => (dragging = h),
      ),
    ];
    // first start with a reachable shelf: look for changes
    setTimeout(async () => {
      await poll();
      if (status.shelf_online && lib.configured) api.refresh(false);
      if (!lib.configured) page = "settings";
    }, 300);
    return () => {
      clearInterval(t);
      un.forEach((p) => p.then((f) => f && f()));
    };
  });
</script>

<div class="shell">
  <Sidebar
    games={lib.games}
    {view}
    {page}
    {status}
    shelf={lib.shelf}
    onview={(v) => { view = v; page = "library"; selected = null; }}
    onsettings={() => { page = "settings"; selected = null; }}
    onadd={() => (adding = "")}
  />
  <main>
    {#if page === "settings"}
      <Settings onsaved={() => { load(); poll(); page = "library"; api.refresh(true); }} />
    {:else if game}
      <Details
        {game}
        {status}
        {busy}
        hasKey={lib.has_sgdb_key}
        onback={() => (selected = null)}
        ondroptarget={(f) => (dropTarget = f)}
        onchanged={load}
      />
    {:else}
      <Library
        games={lib.games}
        {view}
        {status}
        {busy}
        {loaded}
        hasKey={lib.has_sgdb_key}
        unknown={lib.unknown_systems}
        onopen={(id) => (selected = id)}
        onadd={() => (adding = "")}
      />
    {/if}
  </main>
</div>

{#if adding !== null}
  <AddGame
    path={adding}
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

<style>
  .shell { display: grid; grid-template-columns: 232px 1fr; height: 100vh; }
  main { min-width: 0; overflow: hidden; position: relative; background: radial-gradient(1200px 600px at 70% -10%, rgba(42, 107, 179, 0.12), transparent 60%), var(--bg); }
  .dropping { position: fixed; inset: 10px; border: 2px dashed var(--accent); border-radius: 18px; background: rgba(10, 17, 30, 0.85); display: grid; place-items: center; text-align: center; z-index: 60; pointer-events: none; animation: fade .12s; }
  .dropping img { width: 88px; opacity: .9; }
  .dropping h2 { margin: 14px 0 6px; }
</style>
