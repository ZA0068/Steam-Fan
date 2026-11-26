<script>
  import { onMount } from "svelte";
  import { api, pickFile, human } from "./api.js";

  let { path, online, onclose, ondroptarget } = $props();

  let found = $state(null); // what inspect found
  let kind = $state("Fan");
  let group = $state("");
  let name = $state("");
  let groups = $state({});
  let error = $state("");
  let looking = $state(false);

  const kinds = [
    { id: "Fan", label: "Fan game", key: "fan", help: "Made by fans: Sonic, Mario, Kirby…" },
    { id: "NonSteam", label: "Non-Steam game", key: "nonsteam", help: "Demos, indie and other PC games" },
    { id: "Rom", label: "Emulator game", key: "rom", help: "A game for Wii, Wii U, Switch…" },
  ];
  const suggestions = $derived(groups[kinds.find((k) => k.id === kind)?.key] ?? []);

  async function inspect(p) {
    if (!p) return;
    error = "";
    looking = true;
    found = null;
    try {
      found = await api.importInspect(p);
      kind = found.kind;
      name = found.name;
      group = found.kind === "Rom" ? found.system : "";
    } catch (e) {
      error = String(e);
    } finally {
      looking = false;
    }
  }

  async function add() {
    error = "";
    try {
      await api.importPlace(found.source, kind, group.trim(), name.trim());
      onclose();
    } catch (e) {
      error = String(e);
    }
  }

  onMount(async () => {
    groups = (await api.groups()) ?? {};
    ondroptarget((p) => inspect(p));
    if (path) inspect(path);
    return () => ondroptarget(null);
  });
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="modal">
    <header>
      <h2>Add a game to the shelf</h2>
      <button class="btn ghost" onclick={onclose}>✕</button>
    </header>
    <div class="body">
      {#if !found}
        <div class="drop">
          <img src="/logo.svg" alt="" />
          <h3>{looking ? "Looking inside…" : "Drop a download here"}</h3>
          <p class="muted">A zip, 7z or rar file, a game folder, or a ROM. It's unpacked straight onto the shelf.</p>
          <div class="row">
            <button class="btn primary" onclick={async () => inspect(await pickFile({ archives: true }))}>Choose a file…</button>
            <button class="btn" onclick={async () => inspect(await pickFile({ folder: true }))}>Choose a folder…</button>
          </div>
        </div>
      {:else}
        <div class="source">
          <span class="ic">{found.is_archive ? "🗜" : "📁"}</span>
          <div>
            <div class="mono">{found.source}</div>
            <div class="faint">{found.files} files · {human(found.size)}{found.is_archive ? " · unpacked onto the shelf" : " · copied onto the shelf"}</div>
          </div>
          <button class="btn small ghost" onclick={() => (found = null)}>Change</button>
        </div>

        <div class="kinds">
          {#each kinds as k}
            <button class="kind" class:on={kind === k.id} onclick={() => { kind = k.id; if (k.id !== found.kind) group = ""; }}>
              <b>{k.label}</b><span>{k.help}</span>
            </button>
          {/each}
        </div>

        <div class="fields">
          <label class="field">
            <span>{kind === "Rom" ? "System" : "Group"}</span>
            <input class="input" list="groups" bind:value={group} placeholder={kind === "Rom" ? "wii, WIIU, Switch" : "Sonic The Hedgehog, Kirby…"} />
            <datalist id="groups">{#each suggestions as s}<option value={s}></option>{/each}</datalist>
          </label>
          <label class="field">
            <span>Name</span>
            <input class="input" bind:value={name} />
          </label>
        </div>
        {#if suggestions.length}
          <div class="chips">
            {#each suggestions.slice(0, 14) as s}
              <button class="chip" class:on={group === s} onclick={() => (group = s)}>{s}</button>
            {/each}
          </div>
        {/if}

        {#if found.found.length}
          <p class="muted small">{kind === "Rom" ? "Game file" : "Program"} found: <span class="mono">{found.found.join(", ")}</span></p>
        {:else}
          <p class="warn small">No program or game file found inside. It can still go on the shelf, but won't be playable until there is one.</p>
        {/if}
      {/if}
      {#if error}<p class="error">{error}</p>{/if}
      {#if !online}<p class="warn small">The shelf isn't reachable right now.</p>{/if}
    </div>
    {#if found}
      <footer>
        <button class="btn" onclick={onclose}>Cancel</button>
        <button class="btn primary" disabled={!group.trim() || !name.trim() || !online} onclick={add}>Add to shelf</button>
      </footer>
    {/if}
  </div>
</div>

<style>
  .drop { border: 2px dashed var(--line-2); border-radius: 14px; padding: 34px 20px; text-align: center; background: var(--bg-2); }
  .drop img { width: 70px; }
  .drop h3 { margin: 10px 0 4px; }
  .row { display: flex; gap: 10px; justify-content: center; margin-top: 16px; }
  .source { display: flex; align-items: center; gap: 12px; padding: 12px 14px; border-radius: 12px; background: var(--bg-2); border: 1px solid var(--line); }
  .source .ic { font-size: 22px; }
  .source > div { flex: 1; min-width: 0; }
  .source .faint { font-size: 12px; margin-top: 3px; }
  .kinds { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; margin: 18px 0; }
  .kind { display: grid; gap: 4px; padding: 12px; text-align: left; border-radius: 12px; border: 1px solid var(--line-2); background: var(--bg-2); cursor: pointer; color: var(--text); }
  .kind span { font-size: 12px; color: var(--faint); }
  .kind.on { border-color: var(--accent); background: rgba(75, 147, 255, .12); box-shadow: 0 0 0 3px rgba(75, 147, 255, .12); }
  .fields { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 10px; }
  .chip { padding: 4px 10px; border-radius: 99px; border: 1px solid var(--line-2); background: var(--bg-2); color: var(--muted); cursor: pointer; font-size: 12px; }
  .chip.on { border-color: var(--accent); color: var(--text); }
  .small { font-size: 12.5px; margin-top: 14px; }
  .warn { color: var(--warn); }
  .error { color: var(--bad); user-select: text; }
</style>
