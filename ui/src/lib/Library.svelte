<script>
  import { api, human } from "./api.js";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";
  import { look } from "./look.svelte.js";

  let { games, view, group = "", filter = "", filterLabel = "", initialSearch = "", status, busy, loaded, hasKey, unknown, onopen, onnav, onadd } = $props();
  // svelte-ignore state_referenced_locally
  let search = $state(initialSearch);
  const keep = {
    shelf: (g) => !g.on_pc,
    windows: (g) => g.kind !== "Steam" && g.kind !== "Rom" && g.platform !== "Linux",
    linux: (g) => g.platform === "Linux",
  };
  let askRestart = $state(false);
  // cover size and layout: what you picked here, else what the skin says
  const stored = (k) => { try { return localStorage.getItem(k); } catch { return null; } };
  let zoomPicked = $state(Number(stored("zoom")) || 0);
  const zoom = $derived(zoomPicked || look.cover || 160);
  function setZoom(v) {
    zoomPicked = +v;
    try { localStorage.setItem("zoom", String(zoomPicked)); } catch {}
  }
  let layoutPicked = $state(stored("layout") ?? "");
  const layout = $derived(["grid", "list", "shelves"].includes(layoutPicked) ? layoutPicked : look.layout);
  function setLayout(l) {
    layoutPicked = l;
    try { localStorage.setItem("layout", l); } catch {}
  }
  // shelves: the game in the spotlight (hovered or focused)
  let spot = $state(null);
  const spotGame = $derived(spot ? games.find((g) => g.id === spot) : shown[0]);
  /** Arrow keys walk the shelves like a TV remote: ←→ along a row, ↑↓ between rows. */
  function shelfKeys(e) {
    const card = e.target.closest?.(".scard");
    if (!card) return;
    const row = card.parentElement;
    const cards = [...row.children];
    const i = cards.indexOf(card);
    let next = null;
    if (e.key === "ArrowRight") next = cards[i + 1];
    else if (e.key === "ArrowLeft") next = cards[i - 1];
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      const rows = [...e.currentTarget.querySelectorAll(".shelf")];
      const r = rows[rows.indexOf(row) + (e.key === "ArrowDown" ? 1 : -1)];
      if (r) next = r.children[Math.min(i, r.children.length - 1)];
    } else return;
    e.preventDefault();
    if (next) {
      next.focus();
      next.scrollIntoView({ block: "nearest", inline: "center", behavior: "smooth" });
    }
  }

  const titles = { all: "Library", pc: "On this PC", tags: "Collections", Fan: "Fan games", NonSteam: "Non-Steam games", Rom: "Emulator games", Steam: "Steam archive" };
  const order = ["Fan", "NonSteam", "Rom", "Steam"];

  const shown = $derived.by(() => {
    const q = search.trim().toLowerCase();
    return games.filter((g) =>
      (view === "all" || (view === "pc" ? g.on_pc : view === "tags" ? g.tags?.length : g.kind === view)) &&
      (!group || (view === "tags" ? g.tags?.includes(group) : g.group === group)) &&
      (!filter || (keep[filter] ?? (() => true))(g)) &&
      (!q || g.name.toLowerCase().includes(q) || g.group.toLowerCase().includes(q) || (g.tags ?? []).some((t) => t.toLowerCase().includes(q)) || (g.versions ?? []).some((v) => v.label.toLowerCase().includes(q))));
  });

  // sections: by kind on the overview pages, by group (Sonic, Kirby, wii...) inside a kind
  const sections = $derived.by(() => {
    const byKind = view === "all" || view === "pc";
    const map = new Map();
    for (const g of shown) {
      // a collection page: a game shows under each collection it's in
      const keys = view === "tags" ? (group ? [group] : g.tags) : [byKind ? g.kind_label : g.group || "Other"];
      for (const key of keys) {
        if (!map.has(key)) map.set(key, []);
        map.get(key).push(g);
      }
    }
    const keys = [...map.keys()];
    if (byKind) keys.sort((a, b) => order.indexOf(map.get(a)[0].kind) - order.indexOf(map.get(b)[0].kind));
    else keys.sort((a, b) => a.localeCompare(b));
    return keys.map((k) => ({ title: k, games: map.get(k).sort((a, b) => a.name.localeCompare(b.name) || (a.version ?? "").localeCompare(b.version ?? "", undefined, { numeric: true })) }));
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
    <h1>{group || titles[view]}</h1>
    {#if group || filter}
      <div class="chips">
        {#if group}<button class="chip" onclick={() => onnav({ group: "" })} title="Show all of {titles[view]}">{group} ✕</button>{/if}
        {#if filter}<button class="chip" onclick={() => onnav({ filter: "" })}>{filterLabel} ✕</button>{/if}
      </div>
    {/if}
    <div class="search">
      <span><Icon name="search" /></span>
      <input class="input" placeholder="Search games or groups" bind:value={search} />
    </div>
    <div class="actions">
      <button class="btn" disabled={busy || !status.shelf_online} onclick={() => api.refresh(false)} title="Look for new or changed games on the shelf"><Icon name="refresh" /> Read shelf</button>
      <button class="btn" disabled={busy || !status.shelf_online} onclick={() => api.refreshArt(false)} title="Copy artwork from the shelf and find missing covers on SteamGridDB"><Icon name="artwork" /> Artwork</button>
      {#if status.steam_running}
        <button class="btn primary" disabled={busy || !status.shelf_online}
          onclick={() => (askRestart = true)}
          title="Steam rewrites its library when it quits, so it has to be closed while the library is updated. This quits Steam, updates, and starts it again.">
          <Icon name="update" /> Restart Steam &amp; update
        </button>
      {:else}
        <button class="btn primary" disabled={busy || !status.shelf_online} onclick={() => api.syncSteam()}
          title="Add new shelf games to Steam, update changed ones, remove ones that left the shelf">
          <Icon name="update" /> Update Steam library
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

  {#if layout === "shelves" && spotGame}
    <div class="spotlight">
      {#if spotGame.art.hero}<img class="sbg" src={spotGame.art.hero} alt="" />{:else if spotGame.art.cover}<img class="sbg blur" src={spotGame.art.cover} alt="" />{/if}
      <div class="sshade"></div>
      <div class="sbody">
        {#if spotGame.art.logo}<img class="slogo" src={spotGame.art.logo} alt={spotGame.name} />{:else}<h2>{spotGame.name}</h2>{/if}
        <div class="smeta">
          <span>{spotGame.kind_label}</span>{#if spotGame.group}<span>{spotGame.group}</span>{/if}
          {#if spotGame.versions?.length > 1}<span>{spotGame.versions.length} versions · {spotGame.version}</span>{:else if spotGame.version}<span>{spotGame.version}</span>{/if}
          {#if spotGame.on_pc}<span class="good">On this PC</span>{/if}
        </div>
        <button class="btn primary" onclick={() => onopen(spotGame.id)}>Open</button>
      </div>
    </div>
  {/if}

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="scroll" class:shelves={layout === "shelves"} onkeydown={layout === "shelves" ? shelfKeys : undefined}>
    {#if loaded && games.length === 0}
      <div class="empty">
        <img src="/logo.svg" alt="" />
        <h2>Your shelf looks empty</h2>
        <p class="muted">Press <b>Read shelf</b> to look at it, or add your first game.</p>
        <button class="btn primary" onclick={onadd}>＋ Add game</button>
      </div>
    {:else if shown.length === 0 && loaded && view === "tags" && !group}
      <div class="empty">
        <h2>No collections yet</h2>
        <p class="muted">A collection groups games by name only: nothing moves on the shelf, and a game can be in several.
          Open a game and press <b>＋ Collection</b> under its name. To move folders together instead, use <b>Move / Rename</b>.</p>
      </div>
    {:else if shown.length === 0 && loaded}
      <p class="muted none">No games match.</p>
    {/if}

    {#each sections as s (s.title)}
      <section>
        <h3>
          {#if !group && view !== "all" && view !== "pc"}
            <button class="sect" onclick={() => onnav({ group: s.title })} title="Only {s.title}">{s.title}</button>
          {:else}{s.title}{/if}
          <span>{s.games.length}</span>
        </h3>
        {#if layout === "list"}
          <div class="list">
            {#each s.games as g (g.id)}
              {@const b = badge(g)}
              <button class="row" onclick={() => onopen(g.id)}>
                <span class="thumb">{#if g.art.icon}<img src={g.art.icon} alt="" loading="lazy" />{:else if g.art.cover}<img src={g.art.thumb ?? g.art.cover} alt="" loading="lazy" />{/if}</span>
                <span class="rname">{g.name}{#if g.version}<span class="rver">{g.version}{g.versions?.length > 1 ? ` · ${g.versions.length}` : ""}</span>{/if}</span>
                <span class="rcol">{g.group}</span>
                <span class="rcol">{g.kind_label}</span>
                <span class="rcol">{(g.tags ?? []).join(", ")}</span>
                <span class="rcol num">{g.size ? human(g.size) : ""}</span>
                <span class="rcol">{#if b}<span class="rbadge {b.cls}">{b.text}</span>{/if}</span>
              </button>
            {/each}
          </div>
        {:else if layout === "shelves"}
          <div class="shelf" style="--w:{zoom}px">
            {#each s.games as g (g.id)}
              {@const b = badge(g)}
              <button class="scard" onclick={() => onopen(g.id)} onmouseenter={() => (spot = g.id)} onfocus={() => (spot = g.id)} title={g.name}>
                <div class="art"><Cover game={g} />{#if b}<span class="badge {b.cls}">{b.text}</span>{/if}</div>
              </button>
            {/each}
          </div>
        {:else}
        <div class="grid" style="--w:{zoom}px">
            {#each s.games as g (g.id)}
              {@const b = badge(g)}
              <button class="card" onclick={() => onopen(g.id)} title={g.versions?.length > 1 ? `${g.name}: ${g.versions.length} versions, Play uses ${g.version}` : g.name}>
                <div class="art">
                  <Cover game={g} />
                  {#if b}<span class="badge {b.cls}">{b.text}</span>{/if}
                  {#if g.versions?.length > 1}<span class="ver">{g.version} <b>· {g.versions.length}</b></span>{:else if g.version}<span class="ver">{g.version}</span>{/if}
                </div>
                <div class="title">{g.name}</div>
              </button>
            {/each}
          </div>
        {/if}
      </section>
    {/each}
  </div>

  <footer>
    <span class="faint">{shown.length} games</span>
    <div class="layouts" role="group" aria-label="Layout">
      {#each [["grid", "Grid"], ["list", "List"], ["shelves", "Shelves"]] as [id, label]}
        <button class:on={layout === id} onclick={() => setLayout(id)} title="{label} layout"><Icon name={id} /></button>
      {/each}
    </div>
    {#if layout !== "list"}
      <label class="zoom"><span>−</span><input type="range" min="100" max="300" value={zoom} oninput={(e) => setZoom(e.currentTarget.value)} /><span>+</span></label>
    {:else}<span></span>{/if}
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
  .page { display: flex; flex-direction: column; height: 100%; }
  header { display: flex; align-items: center; gap: 16px; padding: 18px 28px 14px; }
  h1 { margin: 0; font-size: 22px; font-weight: 700; min-width: 150px; }
  .search { position: relative; flex: 1; max-width: 420px; }
  .search span { position: absolute; left: 12px; top: 50%; transform: translateY(-50%); color: var(--faint); font-size: 16px; }
  .search .input { padding-left: 34px; border-radius: 99px; }
  .actions { display: flex; gap: 8px; margin-left: auto; }
  .hint { margin: 0 28px 10px; padding: 9px 14px; border-radius: var(--r); background: rgba(var(--accent-rgb), .08); border: 1px solid rgba(var(--accent-rgb), .2); color: var(--muted); font-size: 13px; }
  .scroll { flex: 1; overflow: auto; padding: 4px 28px 30px; }
  section { margin-bottom: 26px; animation: rise .25s ease-out; }
  h3 { margin: 6px 0 12px; font-size: 13px; text-transform: uppercase; letter-spacing: 1px; color: var(--muted); font-weight: 600; }
  h3 span { color: var(--faint); margin-left: 6px; }
  .sect { all: unset; cursor: pointer; }
  .sect:hover { color: var(--text); text-decoration: underline; text-underline-offset: 3px; }
  .chips { display: flex; gap: 6px; }
  .chip { border: 1px solid rgba(var(--accent-rgb), .5); background: rgba(var(--accent-rgb), .12); color: var(--accent-text); border-radius: 99px; padding: 4px 10px; font-size: 12.5px; cursor: pointer; }
  .chip:hover { background: rgba(var(--accent-rgb), .25); }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, var(--w)); gap: 22px 20px; }
  /* cards out of view aren't drawn at all, which keeps scrolling smooth with hundreds of games */
  .card { content-visibility: auto; contain-intrinsic-size: auto var(--w) calc(var(--w) * 1.5 + 30px); }
  .card { display: flex; flex-direction: column; gap: 8px; padding: 0; border: 0; background: none; cursor: pointer; text-align: left; color: inherit; }
  .art { position: relative; border-radius: var(--r); transition: transform .18s ease, box-shadow .18s ease; box-shadow: 0 6px 16px rgba(0, 0, 0, .35); }
  .card:hover .art { transform: translateY(-4px) scale(1.02); box-shadow: 0 14px 30px rgba(0, 0, 0, .5), 0 0 0 2px var(--accent); }
  .card:focus-visible { outline: none; }
  .card:focus-visible .art { box-shadow: 0 0 0 3px var(--accent); }
  .title { font-size: 13px; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; color: var(--text); }
  .badge { position: absolute; top: 8px; left: 8px; font-size: 11px; font-weight: 600; padding: 3px 8px; border-radius: 99px; background: rgba(var(--bg-rgb), .82); backdrop-filter: blur(4px); border: 1px solid var(--line-2); color: var(--muted); }
  .ver { position: absolute; bottom: 8px; right: 8px; font-size: 11px; font-weight: 700; padding: 3px 8px; border-radius: 6px; background: rgba(var(--accent-rgb), .9); color: white; box-shadow: 0 2px 8px rgba(0,0,0,.4); }
  .badge.good { color: var(--good); border-color: rgba(var(--good-rgb), .4); }
  .badge.warn { color: var(--warn); border-color: rgba(var(--warn-rgb), .4); }
  .badge.bad { color: var(--bad); border-color: rgba(var(--bad-rgb), .4); }
  .empty { text-align: center; margin-top: 12vh; }
  .empty img { width: 110px; opacity: .9; }
  .none { padding: 20px 0; }
  footer { display: flex; align-items: center; justify-content: space-between; padding: 8px 28px; border-top: 1px solid var(--line); font-size: 12px; }
  .layouts { display: inline-flex; gap: 2px; margin-left: auto; margin-right: 14px; }
  .layouts button { border: 0; background: none; color: var(--faint); padding: 4px 7px; border-radius: 6px; cursor: pointer; font-size: 14px; }
  .layouts button:hover { color: var(--text); background: var(--panel); }
  .layouts button.on { color: var(--accent); background: rgba(var(--accent-rgb), .14); }
  /* list */
  .list { display: grid; border: 1px solid var(--line); border-radius: var(--r); overflow: hidden; }
  .row { display: grid; grid-template-columns: 34px minmax(200px, 2.2fr) 1.2fr 1fr 1fr 80px 110px; align-items: center; gap: 12px; padding: 6px 12px; border: 0; border-top: 1px solid var(--line); background: var(--panel); color: var(--text); cursor: pointer; text-align: left; font-size: 13px; content-visibility: auto; contain-intrinsic-size: auto 42px; }
  .row:first-child { border-top: 0; }
  .row:nth-child(even) { background: var(--bg-2); }
  .row:hover, .row:focus-visible { background: rgba(var(--accent-rgb), .14); outline: none; }
  .thumb { width: 28px; height: 28px; display: grid; place-items: center; }
  .thumb img { max-width: 28px; max-height: 28px; border-radius: 4px; object-fit: cover; }
  .rname { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .rver { margin-left: 8px; font-size: 11px; font-weight: 600; color: var(--accent-text); background: rgba(var(--accent-rgb), .15); padding: 1px 6px; border-radius: 5px; }
  .rcol { color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .rcol.num { text-align: right; font-variant-numeric: tabular-nums; }
  .rbadge { font-size: 11.5px; font-weight: 600; }
  .rbadge.good { color: var(--good); } .rbadge.warn { color: var(--warn); } .rbadge.bad { color: var(--bad); }
  /* shelves */
  .spotlight { position: relative; height: 30vh; min-height: 180px; max-height: 340px; margin: 0 28px 14px; border-radius: var(--radius); overflow: hidden; background: var(--panel); flex: none; }
  .sbg { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  .sbg.blur { filter: blur(28px) saturate(1.3); transform: scale(1.2); opacity: .7; }
  .sshade { position: absolute; inset: 0; background: linear-gradient(90deg, rgba(var(--bg-rgb), .92) 0%, rgba(var(--bg-rgb), .55) 45%, transparent 80%); }
  .sbody { position: relative; height: 100%; display: flex; flex-direction: column; justify-content: flex-end; gap: 12px; padding: 24px 28px; max-width: 560px; }
  .sbody h2 { margin: 0; font-size: 30px; }
  .slogo { max-width: 340px; max-height: 120px; object-fit: contain; object-position: left bottom; filter: drop-shadow(0 4px 14px rgba(0,0,0,.6)); }
  .smeta { display: flex; flex-wrap: wrap; gap: 6px 14px; color: var(--muted); font-size: 13px; }
  .smeta .good { color: var(--good); }
  .sbody .btn { justify-self: start; width: max-content; }
  .shelf { display: flex; gap: 16px; overflow-x: auto; overflow-y: hidden; padding: 14px 10px 20px; scroll-snap-type: x proximity; scrollbar-width: thin; }
  .scard { flex: none; width: var(--w); padding: 0; border: 0; background: none; cursor: pointer; scroll-snap-align: start; content-visibility: auto; contain-intrinsic-size: var(--w) calc(var(--w) * 1.5); }
  .scard .art { transition: transform .18s ease, box-shadow .18s ease; }
  .scard:hover .art, .scard:focus-visible .art { transform: scale(1.05); box-shadow: 0 14px 30px rgba(0, 0, 0, .55), 0 0 0 3px var(--accent); }
  .scard:focus-visible { outline: none; }
  .zoom { display: flex; align-items: center; gap: 8px; color: var(--faint); }
  .zoom input { accent-color: var(--accent); width: 140px; }
</style>
