<script>
  let { games, view, page, status, shelf, onview, onsettings, onadd } = $props();

  const count = (f) => games.filter(f).length;
  const items = $derived([
    { id: "all", label: "Library", icon: "▦", n: games.length },
    { id: "pc", label: "On this PC", icon: "⬇", n: count((g) => g.on_pc) },
    null,
    { id: "Fan", label: "Fan games", icon: "★", n: count((g) => g.kind === "Fan") },
    { id: "NonSteam", label: "Non-Steam games", icon: "◆", n: count((g) => g.kind === "NonSteam") },
    { id: "Rom", label: "Emulator games", icon: "◉", n: count((g) => g.kind === "Rom") },
    { id: "Steam", label: "Steam archive", icon: "▲", n: count((g) => g.kind === "Steam") },
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

  <button class="btn primary add" onclick={onadd}>＋ Add game</button>

  <nav>
    {#each items as it}
      {#if it}
        <button class="item" class:on={page === "library" && view === it.id} onclick={() => onview(it.id)}>
          <span class="ic">{it.icon}</span>
          <span class="label">{it.label}</span>
          <span class="n">{it.n}</span>
        </button>
      {:else}
        <div class="sep"></div>
      {/if}
    {/each}
  </nav>

  <div class="foot">
    <div class="state" title={shelf}>
      <span class="dot" class:good={status.shelf_online} class:bad={!status.shelf_online}></span>
      <span>Shelf {status.shelf_online ? "online" : "offline"}</span>
    </div>
    <div class="state">
      <span class="dot" class:blue={status.steam_running}></span>
      <span>Steam {status.steam_running ? "running" : "closed"}</span>
    </div>
    <button class="item" class:on={page === "settings"} onclick={onsettings}>
      <span class="ic">⚙</span><span class="label">Settings</span>
    </button>
  </div>
</aside>

<style>
  aside { display: flex; flex-direction: column; gap: 14px; padding: 18px 12px 14px; background: linear-gradient(180deg, #0d1628, #0a111e); border-right: 1px solid var(--line); min-height: 0; }
  .brand { display: flex; align-items: center; gap: 10px; padding: 2px 8px 6px; }
  .brand img { width: 40px; height: 40px; }
  .brand strong { display: block; font-size: 16px; letter-spacing: .2px; }
  .brand small { color: var(--faint); font-size: 11.5px; }
  .add { justify-content: center; margin: 0 4px; }
  nav { display: flex; flex-direction: column; gap: 2px; overflow: auto; flex: 1; }
  .item { display: flex; align-items: center; gap: 10px; width: 100%; padding: 9px 10px; border: 0; border-radius: 9px; background: transparent; color: var(--muted); cursor: pointer; text-align: left; transition: background .12s, color .12s; }
  .item:hover { background: var(--panel); color: var(--text); }
  .item.on { background: linear-gradient(90deg, rgba(75, 147, 255, .22), rgba(75, 147, 255, .06)); color: var(--text); box-shadow: inset 2px 0 0 var(--accent); }
  .ic { width: 18px; text-align: center; opacity: .85; }
  .label { flex: 1; }
  .n { font-size: 12px; color: var(--faint); background: var(--bg-2); padding: 1px 7px; border-radius: 99px; }
  .on .n { color: var(--text); background: rgba(75, 147, 255, .25); }
  .sep { height: 1px; background: var(--line); margin: 8px 6px; }
  .foot { display: flex; flex-direction: column; gap: 6px; border-top: 1px solid var(--line); padding-top: 12px; }
  .state { display: flex; align-items: center; gap: 8px; font-size: 12.5px; color: var(--muted); padding: 2px 10px; }
</style>
