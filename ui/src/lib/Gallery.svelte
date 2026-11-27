<script>
  import { api, pickFile } from "./api.js";
  import PagePictures from "./PagePictures.svelte";
  let { game, shots, online, page = "", onchanged } = $props();
  let fromPage = $state(false);
  let adding = $state(false);
  async function addUrls(urls) {
    adding = true;
    error = "";
    try {
      const r = await api.screenshotsAddUrls(game.id, urls);
      fromPage = false;
      onchanged();
      // some pictures may not have come through; the rest are on the shelf
      if (r?.failed?.length) error = `${r.added} added; ${r.failed.length} couldn't be downloaded: ${r.failed.join("; ")}`;
    } catch (e) {
      error = String(e);
    } finally {
      adding = false;
    }
  }
  let open = $state(-1);
  let error = $state("");

  async function add(paths) {
    if (!paths?.length) return;
    error = "";
    try {
      await api.screenshotsAdd(game.id, paths);
      onchanged();
    } catch (e) {
      error = String(e);
    }
  }
  async function remove(name) {
    await api.screenshotRemove(game.id, name);
    open = -1;
    onchanged();
  }
  function key(e) {
    if (open < 0) return;
    if (e.key === "Escape") open = -1;
    if (e.key === "ArrowRight") open = (open + 1) % shots.length;
    if (e.key === "ArrowLeft") open = (open - 1 + shots.length) % shots.length;
  }
</script>

<svelte:window onkeydown={key} />

<div class="box">
  <div class="head">
    <h4>Screenshots</h4>
    <div class="row">
      <button class="btn small" disabled={!online} onclick={() => (fromPage = !fromPage)}>🌐 From a web page…</button>
      <button class="btn small" disabled={!online} onclick={async () => add(await pickFile({ images: true, multiple: true }))}>＋ Add screenshots…</button>
    </div>
  </div>
  {#if fromPage}
    <div class="from">
      <PagePictures {page} multiple onpick={addUrls} />
      {#if adding}<p class="muted">Downloading and adding…</p>{/if}
    </div>
  {/if}
  {#if shots.length}
    <div class="strip">
      {#each shots as [name, url], i (name)}
        <button class="shot" onclick={() => (open = i)}><img src={url} alt={name} loading="lazy" decoding="async" /></button>
      {/each}
    </div>
  {:else}
    <p class="muted">No screenshots yet. Add some, or drop pictures onto this page.</p>
  {/if}
  {#if error}<p class="error">{error}</p>{/if}
</div>

{#if open >= 0 && shots[open]}
  <div class="light" role="presentation" onclick={(e) => e.target === e.currentTarget && (open = -1)}>
    <img src={shots[open][1]} alt="" />
    <div class="bar">
      <button class="btn small" onclick={() => (open = (open - 1 + shots.length) % shots.length)}>←</button>
      <span>{open + 1} / {shots.length}</span>
      <button class="btn small" onclick={() => (open = (open + 1) % shots.length)}>→</button>
      <button class="btn small danger" disabled={!online} onclick={() => remove(shots[open][0])}>Remove</button>
      <button class="btn small" onclick={() => (open = -1)}>Close</button>
    </div>
  </div>
{/if}

<style>
  .box { background: var(--panel); border: 1px solid var(--line); border-radius: var(--radius); padding: 16px 18px; }
  .head { display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px; }
  h4 { margin: 0; font-size: 13px; text-transform: uppercase; letter-spacing: .8px; color: var(--muted); }
  .row { display: flex; gap: 8px; }
  .from { margin-bottom: 14px; padding: 12px; border: 1px solid var(--line); border-radius: var(--r); background: var(--bg-2); }
  .strip { display: grid; grid-auto-flow: column; grid-auto-columns: 300px; gap: 12px; overflow-x: auto; padding-bottom: 6px; }
  .shot { padding: 0; border: 2px solid transparent; border-radius: var(--r); overflow: hidden; cursor: zoom-in; background: var(--bg-2); transition: border-color .12s; }
  .shot:hover { border-color: var(--accent); }
  .shot img { width: 100%; aspect-ratio: 16 / 9; object-fit: cover; display: block; }
  .light { position: fixed; inset: 0; z-index: 80; background: rgba(3, 6, 12, .92); display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 14px; animation: fade .15s; }
  .light img { max-width: 92vw; max-height: 82vh; border-radius: var(--r); box-shadow: var(--shadow); }
  .bar { display: flex; gap: 8px; align-items: center; color: var(--muted); }
  .error { color: var(--bad); }
</style>
