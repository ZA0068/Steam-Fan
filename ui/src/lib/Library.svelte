<script>
  import { api } from "./api.js";
  import Cover from "./Cover.svelte";

  let { games, view, status, busy, loaded, hasKey, unknown, onopen, onadd } = $props();
  let search = $state("");
  let askRestart = $state(false);
  let zoom = $state(Number(localStorage.getItem("zoom") || 160));
  $effect(() => { try { localStorage.setItem("zoom", String(zoom)); } catch {} });

  const titles = { all: "Library", pc: "On this PC", Fan: "Fan games", NonSteam: "Non-Steam games", Rom: "Emulator games", Steam: "Steam archive" };
  const order = ["Fan", "NonSteam", "Rom", "Steam"];

  const shown = $derived.by(() => {
    const q = search.trim().toLowerCase();
    return games.filter((g) =>
      (view === "all" || (view === "pc" ? g.on_pc : g.kind === view)) &&
      (!q || g.name.toLowerCase().includes(q) || g.group.toLowerCase().includes(q)));
  });

  // sections: by kind on the overview pages, by group (Sonic, Kirby, wii...) inside a kind
  const sections = $derived.by(() => {
    const byKind = view === "all" || view === "pc";
    const map = new Map();
    for (const g of shown) {
      const key = byKind ? g.kind_label : g.group || "Other";
      if (!map.has(key)) map.set(key, []);
      map.get(key).push(g);
    }
    const keys = [...map.keys()];
    if (byKind) keys.sort((a, b) => order.indexOf(map.get(a)[0].kind) - order.indexOf(map.get(b)[0].kind));
    else keys.sort((a, b) => a.localeCompare(b));
    return keys.map((k) => ({ title: k, games: map.get(k).sort((a, b) => a.name.localeCompare(b.name)) }));
  });

  function badge(g) {
    if (g.kind === "Steam") {
      const c = g.fresh_code;
      return { cls: c === "Same" ? "good" : c === "ShelfNewer" || c === "PcNewer" ? "warn" : "", text: g.fresh };
    }
    if (!g.playable) return { cls: "bad", text: g.kind === "Rom" ? "no game file" : "no program" };
    if (g.on_pc) return { cls: "good", text: "on PC" };
    return null;
  }
</script>

<div class="page">
  <header>
    <h1>{titles[view]}</h1>
    <div class="search">
      <span>⌕</span>
      <input class="input" placeholder="Search games or groups" bind:value={search} />
    </div>
    <div class="actions">
      <button class="btn" disabled={busy || !status.shelf_online} onclick={() => api.refresh(false)} title="Look for new or changed games on the shelf">⟳ Read shelf</button>
      <button class="btn" disabled={busy || !status.shelf_online} onclick={() => api.refreshArt(false)} title="Copy artwork from the shelf and find missing covers on SteamGridDB">🖼 Artwork</button>
      {#if status.steam_running}
        <button class="btn primary" disabled={busy || !status.shelf_online}
          onclick={() => (askRestart = true)}
          title="Steam rewrites its library when it quits, so it has to be closed while the library is updated. This quits Steam, updates, and starts it again.">
          ⇪ Restart Steam &amp; update
        </button>
      {:else}
        <button class="btn primary" disabled={busy || !status.shelf_online} onclick={() => api.syncSteam()}
          title="Add new shelf games to Steam, update changed ones, remove ones that left the shelf">
          ⇪ Update Steam library
        </button>
      {/if}
    </div>
  </header>

  {#if !hasKey && (view === "all" || view !== "Steam")}
    <div class="hint">Covers for fan and non-Steam games come from SteamGridDB. Add your API key in <b>Settings</b>, or open a game and upload pictures yourself.</div>
  {/if}
  {#if unknown?.length && (view === "all" || view === "Rom")}
    <div class="hint">No emulator set for <b>{unknown.join(", ")}</b>: those folders are skipped (Settings → Emulators).</div>
  {/if}

  <div class="scroll">
    {#if loaded && games.length === 0}
      <div class="empty">
        <img src="/logo.svg" alt="" />
        <h2>Your shelf looks empty</h2>
        <p class="muted">Press <b>Read shelf</b> to look at it, or add your first game.</p>
        <button class="btn primary" onclick={onadd}>＋ Add game</button>
      </div>
    {:else if shown.length === 0 && loaded}
      <p class="muted none">No games match.</p>
    {/if}

    {#each sections as s (s.title)}
      <section>
        <h3>{s.title} <span>{s.games.length}</span></h3>
        <div class="grid" style="--w:{zoom}px">
          {#each s.games as g (g.id)}
            {@const b = badge(g)}
            <button class="card" onclick={() => onopen(g.id)} title={g.name}>
              <div class="art">
                <Cover game={g} />
                {#if b}<span class="badge {b.cls}">{b.text}</span>{/if}
              </div>
              <div class="title">{g.name}</div>
            </button>
          {/each}
        </div>
      </section>
    {/each}
  </div>

  <footer>
    <span class="faint">{shown.length} games</span>
    <label class="zoom"><span>−</span><input type="range" min="120" max="260" bind:value={zoom} /><span>+</span></label>
  </footer>
</div>

{#if askRestart}
  <div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && (askRestart = false)}>
    <div class="modal small">
      <header><h2>Restart Steam?</h2></header>
      <div class="body">
        <p>Steam rewrites its library when it quits, so it has to be closed while the library is updated.</p>
        <p class="muted">Steam-Fan quits Steam, updates the library, and starts Steam again. A game that's running in Steam is closed too.</p>
      </div>
      <footer>
        <button class="btn" onclick={() => (askRestart = false)}>Cancel</button>
        <button class="btn primary" onclick={() => { askRestart = false; api.syncSteamRestart(); }}>Restart Steam &amp; update</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .small { width: min(480px, calc(100vw - 48px)); }
  .page { display: flex; flex-direction: column; height: 100vh; }
  header { display: flex; align-items: center; gap: 16px; padding: 18px 28px 14px; }
  h1 { margin: 0; font-size: 22px; font-weight: 700; min-width: 150px; }
  .search { position: relative; flex: 1; max-width: 420px; }
  .search span { position: absolute; left: 12px; top: 50%; transform: translateY(-50%); color: var(--faint); font-size: 16px; }
  .search .input { padding-left: 34px; border-radius: 99px; }
  .actions { display: flex; gap: 8px; margin-left: auto; }
  .hint { margin: 0 28px 10px; padding: 9px 14px; border-radius: 10px; background: rgba(75, 147, 255, .08); border: 1px solid rgba(75, 147, 255, .2); color: var(--muted); font-size: 13px; }
  .scroll { flex: 1; overflow: auto; padding: 4px 28px 30px; }
  section { margin-bottom: 26px; animation: rise .25s ease-out; }
  h3 { margin: 6px 0 12px; font-size: 13px; text-transform: uppercase; letter-spacing: 1px; color: var(--muted); font-weight: 600; }
  h3 span { color: var(--faint); margin-left: 6px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, var(--w)); gap: 22px 20px; }
  /* cards out of view aren't drawn at all, which keeps scrolling smooth with hundreds of games */
  .card { content-visibility: auto; contain-intrinsic-size: auto var(--w) calc(var(--w) * 1.5 + 30px); }
  .card { display: flex; flex-direction: column; gap: 8px; padding: 0; border: 0; background: none; cursor: pointer; text-align: left; color: inherit; }
  .art { position: relative; border-radius: 10px; transition: transform .18s ease, box-shadow .18s ease; box-shadow: 0 6px 16px rgba(0, 0, 0, .35); }
  .card:hover .art { transform: translateY(-4px) scale(1.02); box-shadow: 0 14px 30px rgba(0, 0, 0, .5), 0 0 0 2px var(--accent); }
  .card:focus-visible { outline: none; }
  .card:focus-visible .art { box-shadow: 0 0 0 3px var(--accent); }
  .title { font-size: 13px; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; color: #cfd9ea; }
  .badge { position: absolute; top: 8px; left: 8px; font-size: 11px; font-weight: 600; padding: 3px 8px; border-radius: 99px; background: rgba(10, 17, 30, .82); backdrop-filter: blur(4px); border: 1px solid var(--line-2); color: var(--muted); }
  .badge.good { color: var(--good); border-color: rgba(70, 214, 143, .4); }
  .badge.warn { color: var(--warn); border-color: rgba(243, 185, 76, .4); }
  .badge.bad { color: var(--bad); border-color: rgba(255, 111, 111, .4); }
  .empty { text-align: center; margin-top: 12vh; }
  .empty img { width: 110px; opacity: .9; }
  .none { padding: 20px 0; }
  footer { display: flex; align-items: center; justify-content: space-between; padding: 8px 28px; border-top: 1px solid var(--line); font-size: 12px; }
  .zoom { display: flex; align-items: center; gap: 8px; color: var(--faint); }
  .zoom input { accent-color: var(--accent); width: 140px; }
</style>
