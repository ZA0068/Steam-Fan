<script>
  // A game's cover: the picture if there is one, else a tile drawn from its name and group.
  let { game, size = "card" } = $props();
  let broken = $state(false);

  function hue(s) {
    let h = 0;
    for (const c of s) h = (h * 31 + c.charCodeAt(0)) % 360;
    return h;
  }
  const h = $derived(hue(game.group || game.kind));
  const initials = $derived(
    game.name.replace(/\[.*?\]|\(.*?\)/g, "").split(/\s+/).filter((w) => /^[A-Za-z0-9]/.test(w)).slice(0, 3).map((w) => w[0]).join("").toUpperCase(),
  );
</script>

{#if game.art.cover && !broken}
  <img class="cover {size}" src={size === "card" ? game.art.thumb ?? game.art.cover : game.art.cover} alt={game.name} loading="lazy" decoding="async" onerror={() => (broken = true)} />
{:else}
  <div class="tile {size}" style="--h:{h}">
    {#if game.art.icon}
      <img class="icon" src={game.art.icon} alt="" loading="lazy" decoding="async" />
    {:else}
      <span class="initials">{initials || "?"}</span>
    {/if}
    <span class="name">{game.name}</span>
    <span class="group">{game.group}</span>
  </div>
{/if}

<style>
  .cover, .tile { width: 100%; aspect-ratio: 2 / 3; border-radius: 10px; display: block; }
  .cover { object-fit: cover; background: var(--panel); }
  .tile {
    position: relative; overflow: hidden; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; padding: 14px; text-align: center;
    background:
      radial-gradient(120% 80% at 20% 0%, hsla(var(--h), 70%, 55%, .45), transparent 60%),
      linear-gradient(160deg, hsl(var(--h), 45%, 22%), #0b1630 75%);
    border: 1px solid hsla(var(--h), 60%, 60%, .18);
  }
  .tile::after { content: ""; position: absolute; inset: 0; background: repeating-linear-gradient(135deg, rgba(255,255,255,.025) 0 8px, transparent 8px 16px); }
  .icon { width: 42%; aspect-ratio: 1; object-fit: contain; filter: drop-shadow(0 6px 12px rgba(0,0,0,.45)); z-index: 1; }
  .initials { font-size: 40px; font-weight: 800; letter-spacing: 1px; color: hsla(var(--h), 90%, 85%, .9); z-index: 1; }
  .name { font-weight: 700; font-size: 14px; line-height: 1.2; z-index: 1; display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
  .group { font-size: 11px; color: hsla(var(--h), 60%, 85%, .7); z-index: 1; text-transform: uppercase; letter-spacing: .8px; }
  .big { border-radius: 14px; }
  .big .initials { font-size: 56px; }
</style>
