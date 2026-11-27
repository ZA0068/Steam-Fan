<script>
  import { api, human, pickFile } from "./api.js";
  import Cover from "./Cover.svelte";
  import ArtPicker from "./ArtPicker.svelte";
  import Gallery from "./Gallery.svelte";
  import Move from "./Move.svelte";

  let { game, status, busy, hasKey, tab = "overview", ontab, onnav, ondroptarget, onchanged, onmoved } = $props();
  let moving = $state(false);
  let updateMsg = $state("");
  let redl = $state(null); // the re-download dialog
  let editing = $state(null); // a copy of the info while it's being edited
  let info = $state(null);
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
  // a game kept in several versions: Play uses the one chosen here (or the newest)
  let picker = $state(null);
  const newest = $derived(game.versions?.find((v) => !v.old) ?? game.versions?.[0]);
  const day = (secs) => new Date(secs * 1000).toISOString().slice(0, 10);
  async function chooseVersion(folder) {
    error = "";
    try {
      await api.setVersion(game.id, folder);
      onchanged();
    } catch (e) {
      error = String(e);
    }
  }

  async function separate(yes) {
    error = "";
    try {
      await api.setSeparate(game.version_id ?? game.id, yes);
      await onchanged();
      // the entry's id changes: back to the list it was in
      onnav(game.kind, yes ? { group: game.name } : { group: "" });
    } catch (e) {
      error = String(e);
    }
  }

  // pictures and the name can be changed by clicking them (not for Steam's own games)
  const editable = $derived(game.kind !== "Steam" && status.shelf_online);
  const kindFilter = $derived(
    game.kind === "Rom" ? { group: game.group } : game.kind === "Steam" ? {} : { filter: game.platform === "Linux" ? "linux" : "windows" },
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
    <div class="heroInner">
      {#if editable}
        <button class="poster edit" onclick={() => (picking = "cover")} title="Change the cover">
          <Cover {game} size="big" /><span class="pen">✎ Cover</span>
        </button>
      {:else}
        <div class="poster"><Cover {game} size="big" /></div>
      {/if}
      <div class="headline">
        {#if game.art.logo && editable}
          <button class="logoBtn edit" onclick={() => (picking = "logo")} title="Change the logo">
            <img class="logo" src={game.art.logo} alt={game.name} /><span class="pen">✎ Logo</span>
          </button>
        {:else if game.art.logo}
          <img class="logo" src={game.art.logo} alt={game.name} />
        {:else if editable}
          <button class="titleBtn edit" onclick={() => (moving = true)} title="Rename it, or move it to another group">
            <h1>{game.name}</h1><span class="pen">✎</span>
          </button>
        {:else}
          <h1>{game.name}</h1>
        {/if}
        <div class="meta">
          <button class="pill link" onclick={() => onnav(game.kind)} title="All {game.kind_label}">{game.kind_label}</button>
          {#if game.group}<button class="pill link" onclick={() => onnav(game.kind, { group: game.group })} title="Everything in {game.group}">{game.group}</button>{/if}
          {#if game.versions?.length > 1}<button class="pill ver link" onclick={() => picker?.focus()} title="Choose the version Play uses">{game.versions.length} versions</button>{/if}
          <button class="pill link" onclick={() => onnav(game.kind, kindFilter)} title="More like this">{kindText}</button>
          {#if game.size}<span class="pill">{human(game.size)}</span>{/if}
          {#if game.kind === "Steam"}
            <button class="pill link" onclick={() => onnav("Steam")}><span class="dot" class:good={game.fresh_code === "Same"} class:warn={game.fresh_code !== "Same"}></span>{game.fresh}</button>
          {:else if game.on_pc}
            <button class="pill link" onclick={() => onnav("pc")} title="Everything on this PC"><span class="dot good"></span>On this PC</button>
          {:else}
            <button class="pill link" onclick={() => onnav(game.kind, { filter: "shelf" })} title="Everything that's only on the shelf"><span class="dot"></span>On the shelf</button>
          {/if}
        </div>
        {#if game.versions?.length}
          <div class="vpick">
            <span class="faint">Version</span>
            <select class="input" bind:this={picker} value={game.pinned ? game.version_folder : ""} disabled={busy || !status.shelf_online}
              onchange={(e) => chooseVersion(e.currentTarget.value)} title="The version Play uses (and copies to this PC)">
              <option value="">Newest: {newest?.label ?? "?"}</option>
              {#each game.versions as v}
                <option value={v.folder}>{v.label}{v.built ? ` · ${day(v.built)}` : ""}{v.on_pc ? " · on this PC" : ""}{v.playable ? "" : " · no program found"}{v.old ? " · old" : ""}</option>
              {/each}
            </select>
            <button class="btn small ghost" disabled={busy || !status.shelf_online} onclick={() => separate(true)}
              title="These folders are different games, not versions of {game.name}: list each one on its own">Different games?</button>
          </div>
        {:else if game.in_collection}
          <div class="vpick">
            <button class="btn small ghost" disabled={busy || !status.shelf_online} onclick={() => separate(false)}
              title="The games in {game.group} are versions of one game: list them as one, with a version menu">Versions of one game?</button>
          </div>
        {/if}
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
    <button class:on={tab === "overview" || (tab === "art" && game.kind === "Steam")} onclick={() => ontab("overview")}>Overview</button>
    {#if game.kind !== "Steam"}<button class:on={tab === "art"} onclick={() => ontab("art")}>Artwork</button>{/if}
    <button class:on={tab === "history"} onclick={() => ontab("history")}>History</button>
  </nav>

  <div class="content">
    {#if tab === "overview" || (tab === "art" && game.kind === "Steam")}
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
            <div class="two">
              <label class="field"><span>Game page (SFGHQ, itch.io…)</span><input class="input mono" bind:value={editing.page} placeholder="https://sonicfangameshq.com/forums/showcase/…" /></label>
              <label class="field"><span>Download link (MEGA, Google Drive, direct)</span><input class="input mono" bind:value={editing.download} placeholder="https://mega.nz/file/…" /></label>
            </div>
            <label class="field"><span>Other links (one per line: website, video…)</span><textarea class="input mono" rows="2" bind:value={editing.linksText}></textarea></label>
            <div class="row">
              <button class="btn primary small" onclick={saveInfo}>Save</button>
              <button class="btn small" onclick={() => (editing = null)}>Cancel</button>
            </div>
          </div>
        {:else if info?.info?.description || info?.info?.developer || info?.info?.links?.length || info?.info?.page || info?.info?.download}
          {#if info.info.description}<p class="desc">{info.info.description}</p>{/if}
          <div class="facts">
            {#if info.info.developer}<span><span class="faint">Made by</span> {info.info.developer}</span>{/if}
            {#if info.info.released}<span><span class="faint">Released</span> {info.info.released}</span>{/if}
          </div>
          {#if info.info.links.length || info.info.page || info.info.download}
            <div class="row links">
              {#if info.info.page}<button class="btn small" onclick={() => api.open(info.info.page)}>↗ Game page</button>{/if}
              {#if info.info.download}<button class="btn small" onclick={() => api.open(info.info.download)}>⬇ Download page</button>{/if}
              {#each info.info.links as l}<button class="btn small ghost" onclick={() => api.open(l)}>↗ {l.replace(/^https?:\/\/(www\.)?/, "").slice(0, 48)}</button>{/each}
            </div>
          {/if}
        {:else}
          <p class="muted">{game.kind === "Steam" ? "Steam shows this game's description on its store page." : "No description yet. Press Edit to say what this game is, who made it, and where it's from."}</p>
        {/if}
      </div>
      {#if game.kind !== "Steam" && info}
        <Gallery {game} shots={info.screenshots} page={info.info.page} online={status.shelf_online} onchanged={loadInfo} />
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
        {#if game.kind !== "Steam"}
          <div class="box">
            <h4>Files</h4>
            <p class="muted small">Like Steam's Verify integrity: checks the copy on this PC against the game's fingerprints and downloads any damaged or missing file again from the shelf. Settings and saves the game changed are kept. The shelf itself only changes when you add or re-download the game. Not on this PC yet: the shelf copy is checked instead and patched from the download link.</p>
            <div class="row">
              <button class="btn small" disabled={busy || !status.shelf_online} onclick={() => run(api.verify(game.id))}>✔ Verify</button>
              <button class="btn small" disabled={!info?.info?.download} title={info?.info?.download ? "" : "Add a download link under About > Edit"}
                onclick={async () => { updateMsg = "Checking…"; try { updateMsg = await api.checkUpdate(game.id); } catch (e) { updateMsg = String(e); } }}>⟳ Check for update</button>
              <button class="btn small" disabled={busy || !status.shelf_online} onclick={() => (redl = { version: "", file: null })}>⬇ Re-download…</button>
            </div>
            {#if updateMsg}<p class="small note">{updateMsg}</p>{/if}
          </div>
        {/if}
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

{#if redl}
  <div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && (redl = null)}>
    <div class="modal small">
      <header><h2>Download {game.name} again</h2></header>
      <div class="body">
        <div class="redl-src">
          <label class="radio"><input type="radio" name="src" checked={!redl.file} disabled={!info?.info?.download} onchange={() => (redl.file = null)} /> <span><b>From its link</b><br /><span class="faint mono">{info?.info?.download || "no link yet: add it under About > Edit"}</span></span></label>
          <label class="radio"><input type="radio" name="src" checked={!!redl.file} onchange={async () => { const f = await pickFile({ archives: true }); redl.file = f; }} /> <span><b>Use a file I downloaded…</b><br /><span class="faint mono">{redl.file || "zip, 7z, rar… when the link needs a browser"}</span></span></label>
          {#if redl.file}<button class="btn small" onclick={async () => { const f = await pickFile({ archives: true }); if (f) redl.file = f; }}>Choose another file</button>{/if}
        </div>
        <hr />
        <label class="radio"><input type="radio" name="how" checked={!redl.asVersion} onchange={() => (redl.asVersion = false)} /> <span><b>Replace the shelf copy</b><br /><span class="faint">For a damaged copy. The old one is kept until the new one is in place; saves stay. The copy on this PC is removed (saves backed up first) and copied fresh on the next Play.</span></span></label>
        <label class="radio"><input type="radio" name="how" checked={redl.asVersion} onchange={() => (redl.asVersion = true)} /> <span><b>Add as a new version</b><br /><span class="faint">For an update: keeps this one, adds the download beside it.</span></span></label>
        {#if redl.asVersion}
          <label class="field"><span>Version name</span><input class="input" bind:value={redl.version} placeholder="e.g. V 0.8" /></label>
        {/if}
        <p class="faint small">Direct links (forum attachments, GitHub, itch.io files), Google Drive files and Dropbox download by themselves; MEGA needs megatools (sudo pacman -S megatools). If a link answers with a web page instead of a file, download it in your browser and pick it above. The file is checked and checksummed the same way.</p>
      </div>
      <footer>
        <button class="btn" onclick={() => (redl = null)}>Cancel</button>
        <button class="btn primary" disabled={(redl.asVersion && !redl.version.trim()) || (!redl.file && !info?.info?.download)} onclick={() => { run(api.redownload(game.id, redl.asVersion ? redl.version.trim() : null, redl.file)); redl = null; }}>{redl.file ? "Use this file" : "Download"}</button>
      </footer>
    </div>
  </div>
{/if}

{#if moving}
  <Move {game} onclose={() => (moving = false)} onmoved={(id) => { moving = false; onmoved(id); }} />
{/if}

{#if picking}
  <ArtPicker
    {game}
    art={picking}
    page={info?.info?.page ?? ""}
    {hasKey}
    {ondroptarget}
    onclose={() => { picking = null; ondroptarget(null); }}
    ondone={() => { picking = null; ondroptarget(null); onchanged(); loadInfo(); }}
  />
{/if}

<style>
  .redl-src { display: grid; gap: 6px; }
  .redl-src .mono { word-break: break-all; }

  .page { height: 100%; overflow-y: auto; overflow-x: hidden; }
  .edit { position: relative; border: 0; padding: 0; background: none; color: inherit; cursor: pointer; text-align: left; display: block; }
  .edit .pen { position: absolute; right: 8px; bottom: 8px; font-size: 12px; font-weight: 600; padding: 4px 9px; border-radius: 99px; background: rgba(10, 17, 30, .85); border: 1px solid var(--line-2); color: var(--text); opacity: 0; transition: opacity .12s; pointer-events: none; }
  .edit:hover .pen, .edit:focus-visible .pen { opacity: 1; }
  .logoBtn { border-radius: 10px; }
  .logoBtn:hover .logo { filter: drop-shadow(0 6px 20px rgba(0, 0, 0, .6)) drop-shadow(0 0 2px var(--accent)); }
  .logoBtn .pen { right: -4px; bottom: -4px; }
  .titleBtn { display: inline-flex; align-items: center; gap: 10px; }
  .titleBtn .pen { position: static; opacity: 0; }
  .titleBtn:hover h1 { text-decoration: underline; text-decoration-thickness: 2px; text-underline-offset: 6px; }
  .poster.edit .pen { right: 10px; bottom: 10px; }
  .poster.edit:hover { box-shadow: 0 18px 40px rgba(0, 0, 0, .55), 0 0 0 2px var(--accent); }
  .pill.link { cursor: pointer; color: inherit; font: inherit; }
  .vpick { display: flex; align-items: center; gap: 10px; margin-top: 14px; }
  .vpick select { width: auto; min-width: 260px; max-width: 520px; padding: 7px 10px; background: rgba(10, 17, 30, .7); backdrop-filter: blur(6px); }
  .pill.link:hover { border-color: var(--accent); color: var(--text); background: rgba(31, 55, 95, .9); }
  .hero { overflow: hidden; }
  .hero { position: relative; min-height: 380px; display: flex; align-items: flex-end; }
  .bg { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  .bg.blur { filter: blur(30px) saturate(1.3); transform: scale(1.2); opacity: .55; }
  .bg.plain { background: radial-gradient(800px 300px at 30% 20%, rgba(42, 107, 179, .45), transparent), linear-gradient(180deg, #13284d, var(--bg)); }
  .shade { position: absolute; inset: 0; background: linear-gradient(180deg, rgba(10, 17, 30, .2) 0%, rgba(10, 17, 30, .55) 55%, var(--bg) 100%); }
  .heroInner { position: relative; z-index: 1; display: flex; align-items: flex-end; gap: 26px; padding: 40px 32px 22px; width: 100%; }
  .poster { width: 190px; flex: none; box-shadow: 0 18px 40px rgba(0, 0, 0, .55); border-radius: 14px; animation: rise .3s ease-out; }
  .headline { flex: 1; min-width: 0; animation: rise .35s ease-out; }
  .logo { max-width: min(460px, 100%); max-height: 130px; object-fit: contain; filter: drop-shadow(0 6px 20px rgba(0, 0, 0, .6)); }
  h1 { margin: 0; font-size: 38px; line-height: 1.1; text-shadow: 0 4px 20px rgba(0, 0, 0, .6); }
  .meta { display: flex; flex-wrap: wrap; gap: 6px; margin: 14px 0 16px; }
  .meta .pill.ver { color: #cfe2ff; border-color: rgba(75, 147, 255, .6); }
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
  .small { font-size: 12.5px; }
  .note { margin: 10px 0 0; color: var(--text); }
  .modal.small { width: min(560px, calc(100vw - 48px)); }
  .radio { display: flex; gap: 10px; align-items: flex-start; padding: 10px 0; cursor: pointer; }
  .radio input { margin-top: 4px; accent-color: var(--accent); }
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
