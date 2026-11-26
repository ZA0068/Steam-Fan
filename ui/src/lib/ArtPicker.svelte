<script>
  import { onMount } from "svelte";
  import { api, pickFile } from "./api.js";

  let { game, art, hasKey, onclose, ondone, ondroptarget } = $props();
  const labels = { cover: "cover", hero: "banner", logo: "logo", icon: "icon" };

  // svelte-ignore state_referenced_locally
  let term = $state(game.name.replace(/\[.*?\]|\(.*?\)/g, "").trim());
  let results = $state([]);
  let chosen = $state(null);
  let images = $state([]);
  let loading = $state(false);
  let saving = $state(false);
  let error = $state("");
  let link = $state("");

  async function search() {
    if (!hasKey) return;
    error = "";
    loading = true;
    images = [];
    try {
      results = await api.artSearch(game.id, term);
      if (results.length) await choose(results[0]);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }
  async function choose(r) {
    chosen = r;
    loading = true;
    error = "";
    try {
      images = await api.artOptions(r.id, art);
    } catch (e) {
      images = [];
      error = String(e);
    } finally {
      loading = false;
    }
  }
  async function save(p) {
    saving = true;
    error = "";
    try {
      await p;
      ondone();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
  const useImage = (img) => save(api.artSetUrl(game.id, art, img.url, chosen?.id));
  const useFile = (path) => path && save(api.artSetFile(game.id, art, path));
  async function upload() {
    useFile(await pickFile({ images: true }));
  }

  onMount(() => {
    ondroptarget((path) => useFile(path));
    search();
    return () => ondroptarget(null);
  });
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="modal wide">
    <header>
      <h2>Choose a {labels[art]} for {game.name}</h2>
      <button class="btn ghost" onclick={onclose}>✕</button>
    </header>
    <div class="body">
      <div class="own">
        <div>
          <b>Your own picture</b>
          <p class="muted">Upload a file, drop one onto this window, or paste a link to an image.</p>
        </div>
        <button class="btn" disabled={saving} onclick={upload}>⬆ Upload…</button>
      </div>
      <div class="link">
        <input class="input" placeholder="https://… image link" bind:value={link} />
        <button class="btn" disabled={!link.trim() || saving} onclick={() => save(api.artSetUrl(game.id, art, link.trim(), null))}>Use link</button>
      </div>

      <div class="sgdb">
        <b>From SteamGridDB</b>
        {#if !hasKey}
          <p class="muted">Add your SteamGridDB API key in Settings to search here.</p>
        {:else}
          <form class="find" onsubmit={(e) => { e.preventDefault(); search(); }}>
            <input class="input" bind:value={term} placeholder="Game name" />
            <button class="btn" type="submit">Search</button>
          </form>
          {#if results.length}
            <div class="results">
              {#each results.slice(0, 12) as r (r.id)}
                <button class="chip" class:on={chosen?.id === r.id} onclick={() => choose(r)}>
                  {r.name}{#if r.verified} ✓{/if}
                </button>
              {/each}
            </div>
          {/if}
          {#if loading}
            <p class="muted">Looking…</p>
          {:else if chosen && images.length === 0 && !error}
            <p class="muted">No {labels[art]} for this game yet on SteamGridDB. Try another result, or upload your own.</p>
          {:else if !loading && hasKey && results.length === 0 && !error}
            <p class="muted">Nothing found. Try a shorter name.</p>
          {/if}
          <div class="thumbs {art}">
            {#each images.slice(0, 40) as img (img.id)}
              <button class="thumb" disabled={saving} onclick={() => useImage(img)} title="{img.width} × {img.height} {img.style}">
                <img src={img.thumb} alt="" loading="lazy" />
              </button>
            {/each}
          </div>
        {/if}
      </div>
      {#if error}<p class="error">{error}</p>{/if}
      {#if saving}<p class="muted">Saving to the shelf…</p>{/if}
    </div>
  </div>
</div>

<style>
  .wide { width: min(980px, calc(100vw - 48px)); }
  .own { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .own p { margin: 4px 0 0; font-size: 13px; }
  .link { display: flex; gap: 8px; margin: 12px 0 22px; }
  .sgdb { border-top: 1px solid var(--line); padding-top: 18px; }
  .find { display: flex; gap: 8px; margin: 10px 0; }
  .results { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 14px; }
  .chip { padding: 5px 11px; border-radius: 99px; border: 1px solid var(--line-2); background: var(--bg-2); color: var(--muted); cursor: pointer; font-size: 12.5px; }
  .chip.on { background: rgba(75, 147, 255, .2); border-color: var(--accent); color: var(--text); }
  .thumbs { display: grid; gap: 12px; grid-template-columns: repeat(auto-fill, minmax(130px, 1fr)); }
  .thumbs.hero { grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); }
  .thumbs.logo { grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); }
  .thumbs.icon { grid-template-columns: repeat(auto-fill, minmax(80px, 1fr)); }
  .thumb { padding: 0; border: 2px solid transparent; border-radius: 9px; overflow: hidden; cursor: pointer; background: repeating-conic-gradient(#121c30 0 25%, #0f1828 0 50%) 0 0 / 16px 16px; transition: transform .12s, border-color .12s; }
  .thumb:hover { border-color: var(--accent); transform: translateY(-2px); }
  .thumb img { width: 100%; display: block; }
  .thumbs.cover .thumb img { aspect-ratio: 2 / 3; object-fit: cover; }
  .thumbs.logo .thumb img { aspect-ratio: 16 / 9; object-fit: contain; padding: 8px; }
  .error { color: var(--bad); user-select: text; }
</style>
