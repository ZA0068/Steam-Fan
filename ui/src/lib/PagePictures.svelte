<script>
  // Pictures found on a web page (a game's SFGHQ page...). Pick one, or several when `multiple`.
  import { api } from "./api.js";
  let { page = "", multiple = false, onpick } = $props();
  // svelte-ignore state_referenced_locally
  let url = $state(page);
  let images = $state([]);
  let chosen = $state([]);
  let loading = $state(false);
  let error = $state("");

  async function load() {
    if (!url.trim()) return;
    error = "";
    loading = true;
    images = [];
    chosen = [];
    try {
      images = await api.pageImages(url.trim());
      if (!images.length) error = "No pictures found on that page.";
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }
  // a picture that doesn't load is gone from the list, and from the selection
  function drop(u) {
    images = images.filter((x) => x !== u);
    chosen = chosen.filter((x) => x !== u);
    if (!images.length) error = "None of the pictures on that page could be loaded.";
  }
  function click(u) {
    if (!multiple) return onpick([u]);
    chosen = chosen.includes(u) ? chosen.filter((x) => x !== u) : [...chosen, u];
  }
</script>

<form class="find" onsubmit={(e) => { e.preventDefault(); load(); }}>
  <input class="input" bind:value={url} placeholder="A game's page: SFGHQ showcase, Game Jolt, itch.io…" />
  <button class="btn" type="submit" disabled={loading}>{loading ? "Loading…" : "Show pictures"}</button>
</form>
{#if error}<p class="error">{error}</p>{/if}
{#if images.length}
  <div class="grid">
    {#each images as u (u)}
      <button class="pic" class:on={chosen.includes(u)} onclick={() => click(u)} title={u}>
        <img src={u} alt="" loading="lazy" onerror={() => drop(u)} />
        {#if multiple && chosen.includes(u)}<span class="tick">✓</span>{/if}
      </button>
    {/each}
  </div>
  {#if multiple}
    <div class="row">
      <button class="btn primary small" disabled={!chosen.length} onclick={() => onpick(chosen)}>Add {chosen.length || ""} selected</button>
      <button class="btn small ghost" onclick={() => (chosen = [...images])}>Select all</button>
    </div>
  {/if}
{/if}

<style>
  .find { display: flex; gap: 8px; margin: 10px 0; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 10px; max-height: 420px; overflow: auto; padding: 2px; }
  .pic { position: relative; padding: 0; border: 2px solid transparent; border-radius: var(--r); overflow: hidden; cursor: pointer; background: repeating-conic-gradient(var(--panel) 0 25%, var(--bg-2) 0 50%) 0 0 / 16px 16px; aspect-ratio: 4 / 3; }
  .pic:hover, .pic.on { border-color: var(--accent); }
  .pic img { width: 100%; height: 100%; object-fit: contain; display: block; }
  .tick { position: absolute; top: 6px; right: 6px; background: var(--accent); color: white; border-radius: 50%; width: 22px; height: 22px; display: grid; place-items: center; font-size: 13px; }
  .row { display: flex; gap: 8px; margin-top: 10px; }
  .error { color: var(--bad); user-select: text; }
</style>
