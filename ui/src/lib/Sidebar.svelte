<script>
  import Icon from "./Icon.svelte";
  let { games, view, page, status, shelf, onview, onsettings, onadd } = $props();

  const count = (f) => games.filter(f).length;
  const items = $derived([
    { id: "all", label: "Library", icon: "library", n: games.length },
    { id: "pc", label: "On this PC", icon: "pc", n: count((g) => g.on_pc) },
    { id: "tags", label: "Collections", icon: "tags", n: count((g) => g.tags?.length) },
    null,
    { id: "Fan", label: "Fan games", icon: "fan", n: count((g) => g.kind === "Fan") },
    { id: "NonSteam", label: "Non-Steam games", icon: "nonsteam", n: count((g) => g.kind === "NonSteam") },
    { id: "Rom", label: "Emulator games", icon: "rom", n: count((g) => g.kind === "Rom") },
    { id: "Steam", label: "Steam archive", icon: "steam", n: count((g) => g.kind === "Steam") },
  ]);
</script>

<aside>
  <div class="brand">
    <img src="/logo.svg" alt="" />
    <div>
      <strong>Steam-Fan</strong>
      <small>shelf · box · Steam</small>
    </div>
  </div>

  <button class="btn primary add" onclick={onadd} title="Add game"><Icon name="add" /><span class="addLabel">Add game</span></button>

  <nav>
    {#each items as it}
      {#if it}
        <button class="item" class:on={page === "library" && view === it.id} onclick={() => onview(it.id)} title="{it.label} ({it.n})">
          <span class="ic"><Icon name={it.icon} /></span>
          <span class="label">{it.label}</span>
          <span class="n">{it.n}</span>
        </button>
      {:else}
        <div class="sep"></div>
      {/if}
    {/each}
  </nav>

  <div class="foot">
    <div class="state" title="Shelf {status.shelf_online ? 'online' : 'offline'}: {status.via ?? shelf}">
      <span class="dot" class:good={status.shelf_online} class:bad={!status.shelf_online}></span>
      <span>Shelf {status.shelf_online ? "online" : "offline"}</span>
    </div>
    <div class="state" title="Steam {status.steam_running ? 'running' : 'closed'}">
      <span class="dot" class:blue={status.steam_running}></span>
      <span>Steam {status.steam_running ? "running" : "closed"}</span>
    </div>
    <button class="item" class:on={page === "settings"} onclick={onsettings} title="Settings">
      <span class="ic"><Icon name="settings" /></span><span class="label">Settings</span>
    </button>
  </div>
</aside>

<style>
  aside { display: flex; flex-direction: column; gap: 14px; padding: 18px 12px 14px; background: linear-gradient(180deg, var(--sidebar), var(--bg)); border-right: 1px solid var(--line); min-height: 0; }
  .brand { display: flex; align-items: center; gap: 10px; padding: 2px 8px 6px; }
  .brand img { width: 40px; height: 40px; }
  .brand strong { display: block; font-size: 16px; letter-spacing: .2px; }
  .brand small { color: var(--faint); font-size: 11.5px; }
  .add { justify-content: center; margin: 0 4px; }
  nav { display: flex; flex-direction: column; gap: 2px; overflow: auto; flex: 1; }
  .item { display: flex; align-items: center; gap: 10px; width: 100%; padding: 9px 10px; border: 0; border-radius: var(--r); background: transparent; color: var(--muted); cursor: pointer; text-align: left; transition: background .12s, color .12s; }
  .item:hover { background: var(--panel); color: var(--text); }
  .item.on { background: linear-gradient(90deg, rgba(var(--accent-rgb), .22), rgba(var(--accent-rgb), .06)); color: var(--text); box-shadow: inset 2px 0 0 var(--accent); }
  .ic { width: 18px; text-align: center; opacity: .85; }
  .label { flex: 1; }
  .n { font-size: 12px; color: var(--faint); background: var(--bg-2); padding: 1px 7px; border-radius: 99px; }
  .on .n { color: var(--text); background: rgba(var(--accent-rgb), .25); }
  .sep { height: 1px; background: var(--line); margin: 8px 6px; }
  .foot { display: flex; flex-direction: column; gap: 6px; border-top: 1px solid var(--line); padding-top: 12px; }
  .state { display: flex; align-items: center; gap: 8px; font-size: 12.5px; color: var(--muted); padding: 2px 10px; }
  :global(html.side-right) aside { border-right: 0; border-left: 1px solid var(--line); }
  :global(html.side-right) .item.on { box-shadow: inset -2px 0 0 var(--accent); }
  /* icons only */
  :global(html.side-icons) aside { padding: 18px 8px 14px; align-items: stretch; }
  :global(html.side-icons) .brand > div, :global(html.side-icons) .label, :global(html.side-icons) .n, :global(html.side-icons) .state span:not(.dot) { display: none; }
  :global(html.side-icons) .brand { justify-content: center; padding: 2px 0 6px; }
  :global(html.side-icons) .brand img { width: 34px; height: 34px; }
  :global(html.side-icons) .item { justify-content: center; padding: 10px 0; }
  :global(html.side-icons) .ic { width: auto; font-size: 17px; }
  :global(html.side-icons) .state { justify-content: center; padding: 4px 0; }
  :global(html.side-icons) .add { padding: 9px 0; margin: 0; font-size: 16px; }
  :global(html.side-icons) .addLabel { display: none; }
</style>
