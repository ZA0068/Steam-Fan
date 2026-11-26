<script>
  import { api, human } from "./api.js";
  import Cover from "./Cover.svelte";
  import ArtPicker from "./ArtPicker.svelte";
  import Gallery from "./Gallery.svelte";
  import Move from "./Move.svelte";

  let { game, status, busy, hasKey, onback, ondroptarget, onchanged, onmoved } = $props();
  let moving = $state(false);
  let editing = $state(null); // a copy of the info while it's being edited
  let info = $state(null);
  let tab = $state("overview");
  let picking = $state(null); // which picture is being chosen
  let error = $state("");

  async function loadInfo() {
    try {
      info = await api.details(game.id);
    } catch (e) {
      error = String(e);
    }
  }
  $effect(() => {
    game.id;
    loadInfo();
  });
  // pictures dropped on this page become screenshots (the artwork picker takes over while open)
  $effect(() => {
    if (picking || moving || game.kind === "Steam") return;
    ondroptarget(async (paths) => {
      const pics = paths.filter((p) => /\.(png|jpe?g|webp|bmp|gif)$/i.test(p));
      if (!pics.length) return;
      await run(api.screenshotsAdd(game.id, pics));
      loadInfo();
    });
    return () => ondroptarget(null);
  });
  async function saveInfo() {
    const i = $state.snapshot(editing);
    i.links = (i.linksText ?? "").split("\n").map((l) => l.trim()).filter(Boolean);
    delete i.linksText;
    await run(api.infoSave(game.id, i));
    editing = null;
    loadInfo();
  }

  async function run(p) {
    error = "";
    try {
      await p;
    } catch (e) {
      error = String(e);
    }
  }

  const arts = [
    { id: "cover", label: "Cover", hint: "Portrait, 600 × 900", ratio: "2 / 3" },
    { id: "hero", label: "Banner", hint: "Wide, 1920 × 620", ratio: "31 / 10" },
    { id: "logo", label: "Logo", hint: "Transparent PNG", ratio: "16 / 9" },
    { id: "icon", label: "Icon", hint: "Square", ratio: "1 / 1" },
  ];
  const kindText = $derived(
    game.kind === "Steam" ? "Steam" : game.kind === "Rom" ? `Emulator · ${game.system}` : game.platform === "Linux" ? "Linux" : "Windows · Proton",
  );
  const canRestore = $derived(["OnlyShelf", "ShelfNewer", "Unknown"].includes(game.fresh_code));
</script>

<div class="page">
  <div class="hero">
    {#if game.art.hero}
      <img class="bg" src={game.art.hero} alt="" />
    {:else if game.art.cover}
      <img class="bg blur" src={game.art.cover} alt="" />
    {:else}
      <div class="bg plain"></div>
    {/if}
    <div class="shade"></div>
    <button class="btn ghost back" onclick={onback}>← Library</button>
    <div class="heroInner">
      <div class="poster"><Cover {game} size="big" /></div>
      <div class="headline">
        {#if game.art.logo}
          <img class="logo" src={game.art.logo} alt={game.name} />
        {:else}
          <h1>{game.name}</h1>
        {/if}
        <div class="meta">
          <span class="pill">{game.kind_label}</span>
          <span class="pill">{game.group}</span>
          <span class="pill">{kindText}</span>
          {#if game.size}<span class="pill">{human(game.size)}</span>{/if}
          {#if game.kind === "Steam"}
            <span class="pill"><span class="dot" class:good={game.fresh_code === "Same"} class:warn={game.fresh_code !== "Same"}></span>{game.fresh}</span>
          {:else if game.on_pc}
            <span class="pill"><span class="dot good"></span>On this PC</span>
          {:else}
            <span class="pill"><span class="dot"></span>On the shelf</span>
          {/if}
        </div>
        <div class="buttons">
          {#if game.steam_id}
            <button class="btn play" onclick={() => run(api.play(game.id))}>▶ Play</button>
          {:else if game.kind !== "Steam"}
            <button class="btn play" disabled title="Press Update Steam library first">▶ Play</button>
          {/if}
          {#if game.kind === "Steam"}
            <button class="btn" disabled={busy || status.steam_running || !canRestore || !status.shelf_online} onclick={() => run(api.install(game.id))}
              title={status.steam_running ? "Quit Steam first" : "Install this version into Steam, for when Steam's servers are down"}>⬇ Install from shelf</button>
            <button class="btn" disabled={busy || game.fresh_code !== "PcNewer" || !status.shelf_online} onclick={() => run(api.archiveUpload(game.id))}
              title="This PC has a newer version: put it on the shelf too">⬆ Copy to shelf</button>
          {:else if game.on_pc}
            <button class="btn danger" disabled={busy} onclick={() => run(api.remove(game.id))}
              title="Saves go to the shelf first. It stays in Steam and copies again when you press Play.">🗑 Remove from PC</button>
          {:else}
            <button class="btn" disabled={busy || !status.shelf_online || !game.playable} onclick={() => run(api.install(game.id))}>⬇ Copy to this PC</button>
          {/if}
          {#if game.kind !== "Steam"}
            <button class="btn" disabled={busy || !status.shelf_online} onclick={() => (moving = true)} title="Move it to another group, or rename it">✎ Move / Rename</button>
          {/if}
        </div>
        {#if error}<p class="error">{error}</p>{/if}
      </div>
    </div>
  </div>

  <nav class="tabs">
    <button class:on={tab === "overview"} onclick={() => (tab = "overview")}>Overview</button>
    {#if game.kind !== "Steam"}<button class:on={tab === "art"} onclick={() => (tab = "art")}>Artwork</button>{/if}
    <button class:on={tab === "history"} onclick={() => (tab = "history")}>History</button>
  </nav>

  <div class="content">
    {#if tab === "overview"}
      <div class="box about">
        <div class="head">
          <h4>About</h4>
          {#if game.kind !== "Steam" && !editing}
            <button class="btn small" disabled={!status.shelf_online || !info} onclick={() => (editing = { ...info.info, linksText: info.info.links.join("\n") })}>Edit</button>
          {/if}
        </div>
        {#if editing}
          <div class="edit">
            <label class="field"><span>Description</span><textarea class="input" rows="5" bind:value={editing.description} placeholder="What is this game about?"></textarea></label>
            <div class="two">
              <label class="field"><span>Made by</span><input class="input" bind:value={editing.developer} /></label>
              <label class="field"><span>Released</span><input class="input" bind:value={editing.released} placeholder="2024, or SAGE 2025" /></label>
            </div>
            <label class="field"><span>Links (one per line: SFGHQ page, website, video…)</span><textarea class="input mono" rows="3" bind:value={editing.linksText}></textarea></label>
            <div class="row">
              <button class="btn primary small" onclick={saveInfo}>Save</button>
              <button class="btn small" onclick={() => (editing = null)}>Cancel</button>
            </div>
          </div>
        {:else if info?.info?.description || info?.info?.developer || info?.info?.links?.length}
          {#if info.info.description}<p class="desc">{info.info.description}</p>{/if}
          <div class="facts">
            {#if info.info.developer}<span><span class="faint">Made by</span> {info.info.developer}</span>{/if}
            {#if info.info.released}<span><span class="faint">Released</span> {info.info.released}</span>{/if}
          </div>
          {#if info.info.links.length}
            <div class="row links">
              {#each info.info.links as l}<button class="btn small ghost" onclick={() => api.open(l)}>↗ {l.replace(/^https?:\/\/(www\.)?/, "").slice(0, 48)}</button>{/each}
            </div>
          {/if}
        {:else}
          <p class="muted">{game.kind === "Steam" ? "Steam shows this game's description on its store page." : "No description yet. Press Edit to say what this game is, who made it, and where it's from."}</p>
        {/if}
      </div>
      {#if game.kind !== "Steam" && info}
        <Gallery {game} shots={info.screenshots} online={status.shelf_online} onchanged={loadInfo} />
      {/if}
      <div class="cards">
        <div class="box">
          <h4>Where it is</h4>
          {#if info}
            <dl>
              <dt>Shelf</dt><dd class="mono">{info.shelf_path}</dd>
              {#if info.local_path}<dt>This PC</dt><dd class="mono">{info.local_path}</dd>{/if}
              <dt>Saves</dt><dd class="mono">{info.saves_path}</dd>
            </dl>
            <div class="row">
              {#if status.shelf_online}<button class="btn small" onclick={() => api.open(info.shelf_path)}>Open on shelf</button>{/if}
              {#if game.on_pc && info.local_path}<button class="btn small" onclick={() => api.open(info.local_path)}>Open on PC</button>{/if}
              {#if status.shelf_online && game.kind !== "Steam"}<button class="btn small" onclick={() => api.open(info.saves_path)}>Open saves</button>{/if}
            </div>
          {/if}
        </div>
        <div class="box">
          <h4>How it starts</h4>
          <dl>
            {#if game.kind === "Steam"}
              <dt>App id</dt><dd>{game.appid}</dd>
              <dt>Shelf build</dt><dd>{game.buildid}</dd>
            {:else}
              <dt>Program</dt><dd class="mono">{game.program || "none found"}</dd>
              {#if info?.rom_command}<dt>Command</dt><dd class="mono">{info.rom_command}</dd>{/if}
              <dt>In Steam</dt><dd>{game.steam_id ? "yes" : "not yet: Update Steam library"}</dd>
            {/if}
          </dl>
        </div>
      </div>
    {:else if tab === "art"}
      <p class="muted">Pictures are kept on the shelf, so every PC gets them, and copied into Steam's library too (Steam shows them after it restarts).</p>
      <div class="arts">
        {#each arts as a}
          {@const url = game.art[a.id]}
          <div class="slot">
            <div class="frame" style="aspect-ratio:{a.ratio}" class:logo={a.id === "logo"}>
              {#if url}<img src={url} alt={a.label} />{:else}<span class="faint">None yet</span>{/if}
            </div>
            <div class="slotBar">
              <div><b>{a.label}</b> <span class="faint">{a.hint}</span></div>
              <div class="row">
                <button class="btn small" disabled={!status.shelf_online} onclick={() => (picking = a.id)}>Change</button>
                {#if url && info?.art_on_shelf?.includes(a.id)}
                  <button class="btn small ghost" onclick={async () => { await run(api.artRemove(game.id, a.id)); onchanged(); loadInfo(); }}>Remove</button>
                {/if}
              </div>
            </div>
          </div>
        {/each}
      </div>
    {:else}
      <div class="box">
        {#if info?.history?.length}
          <ul class="history">
            {#each info.history as [ts, msg]}<li><span class="faint">{ts}</span> {msg}</li>{/each}
          </ul>
        {:else}
          <p class="muted">Nothing yet.</p>
        {/if}
      </div>
    {/if}
  </div>
</div>

{#if moving}
  <Move {game} onclose={() => (moving = false)} onmoved={(id) => { moving = false; onmoved(id); }} />
{/if}

{#if picking}
  <ArtPicker
    {game}
    art={picking}
    {hasKey}
    {ondroptarget}
    onclose={() => { picking = null; ondroptarget(null); }}
    ondone={() => { picking = null; ondroptarget(null); onchanged(); loadInfo(); }}
  />
{/if}

<style>
  .page { height: 100vh; overflow-y: auto; overflow-x: hidden; }
  .hero { overflow: hidden; }
  .hero { position: relative; min-height: 380px; display: flex; align-items: flex-end; }
  .bg { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  .bg.blur { filter: blur(30px) saturate(1.3); transform: scale(1.2); opacity: .55; }
  .bg.plain { background: radial-gradient(800px 300px at 30% 20%, rgba(42, 107, 179, .45), transparent), linear-gradient(180deg, #13284d, var(--bg)); }
  .shade { position: absolute; inset: 0; background: linear-gradient(180deg, rgba(10, 17, 30, .2) 0%, rgba(10, 17, 30, .55) 55%, var(--bg) 100%); }
  .back { position: absolute; top: 16px; left: 20px; z-index: 2; background: rgba(10, 17, 30, .55); color: var(--text); backdrop-filter: blur(6px); }
  .heroInner { position: relative; z-index: 1; display: flex; align-items: flex-end; gap: 26px; padding: 70px 32px 22px; width: 100%; }
  .poster { width: 190px; flex: none; box-shadow: 0 18px 40px rgba(0, 0, 0, .55); border-radius: 14px; animation: rise .3s ease-out; }
  .headline { flex: 1; min-width: 0; animation: rise .35s ease-out; }
  .logo { max-width: min(460px, 100%); max-height: 130px; object-fit: contain; filter: drop-shadow(0 6px 20px rgba(0, 0, 0, .6)); }
  h1 { margin: 0; font-size: 38px; line-height: 1.1; text-shadow: 0 4px 20px rgba(0, 0, 0, .6); }
  .meta { display: flex; flex-wrap: wrap; gap: 6px; margin: 14px 0 16px; }
  .meta .pill { background: rgba(18, 29, 49, .75); backdrop-filter: blur(6px); }
  .buttons { display: flex; flex-wrap: wrap; gap: 10px; }
  .buttons .btn { padding: 10px 18px; }
  .play { font-size: 15px; padding: 10px 26px !important; }
  .error { color: var(--bad); margin: 10px 0 0; user-select: text; }
  .tabs { display: flex; gap: 4px; padding: 0 32px; border-bottom: 1px solid var(--line); position: sticky; top: 0; background: var(--bg); z-index: 3; }
  .tabs button { padding: 12px 16px; border: 0; background: none; color: var(--muted); cursor: pointer; border-bottom: 2px solid transparent; }
  .tabs button.on { color: var(--text); border-color: var(--accent); }
  .content { padding: 22px 32px 40px; }
  .about { margin-bottom: 16px; }
  .about + :global(.box) { margin-bottom: 16px; }
  .head { display: flex; justify-content: space-between; align-items: center; margin-bottom: 10px; }
  .head h4 { margin: 0; }
  .desc { white-space: pre-wrap; line-height: 1.55; margin: 0 0 12px; user-select: text; max-width: 900px; }
  .facts { display: flex; gap: 22px; flex-wrap: wrap; font-size: 13px; }
  .links { margin-top: 10px; }
  .edit { display: grid; gap: 12px; }
  .two { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  .cards { margin-top: 16px; display: grid; grid-template-columns: repeat(auto-fit, minmax(320px, 1fr)); gap: 16px; }
  .box { background: var(--panel); border: 1px solid var(--line); border-radius: var(--radius); padding: 16px 18px; }
  h4 { margin: 0 0 12px; font-size: 13px; text-transform: uppercase; letter-spacing: .8px; color: var(--muted); }
  dl { display: grid; grid-template-columns: 90px 1fr; gap: 8px 12px; margin: 0 0 12px; }
  dt { color: var(--faint); }
  dd { margin: 0; }
  .row { display: flex; gap: 8px; flex-wrap: wrap; }
  .arts { display: grid; grid-template-columns: 250px minmax(0, 1fr); grid-auto-rows: auto; gap: 16px; margin-top: 14px; }
  .slot { background: var(--panel); border: 1px solid var(--line); border-radius: var(--radius); padding: 12px; display: flex; flex-direction: column; gap: 10px; }
  .slot:nth-child(1) { grid-row: span 2; }
  .frame { width: 100%; border-radius: 9px; background: repeating-conic-gradient(#121c30 0 25%, #0f1828 0 50%) 0 0 / 18px 18px; display: grid; place-items: center; overflow: hidden; }
  .frame img { width: 100%; height: 100%; object-fit: cover; }
  .frame.logo img { object-fit: contain; padding: 12px; }
  .slot:nth-child(4) .frame { max-width: 140px; }
  .slotBar { display: flex; justify-content: space-between; align-items: center; gap: 10px; font-size: 13px; }
  .history { list-style: none; margin: 0; padding: 0; display: grid; gap: 8px; }
  .history .faint { margin-right: 10px; font-size: 12px; }
</style>
