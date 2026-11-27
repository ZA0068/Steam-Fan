<script>
  // One of the skin's icons: a glyph, an SVG, or a picture file from the skin's folder.
  import { look } from "./look.svelte.js";
  let { name, label = "" } = $props();
  const v = $derived(look.icons[name] ?? "");
</script>

{#if v.startsWith("<svg")}
  <span class="ic" aria-label={label || null}>{@html v}</span>
{:else if /^(art|https?|data):/.test(v)}
  <img class="ic img" src={v} alt={label} />
{:else}
  <span class="ic" aria-label={label || null}>{v}</span>
{/if}

<style>
  .ic { display: inline-flex; align-items: center; justify-content: center; line-height: 1; flex: none; }
  .ic :global(svg) { width: 1.05em; height: 1.05em; }
  .img { width: 1.1em; height: 1.1em; object-fit: contain; }
</style>
