<script>
  import { onMount } from "svelte";
  import { api } from "./api.js";

  let { game, onclose, onmoved } = $props();
  // svelte-ignore state_referenced_locally
  let kind = $state(game.kind);
  // svelte-ignore state_referenced_locally
  const parts = game.id.slice(game.id.indexOf(":") + 1).split("/");
  // svelte-ignore state_referenced_locally
  let version = $state(game.version ?? "");
  // svelte-ignore state_referenced_locally
  let group = $state(game.kind === "Rom" ? game.system : parts.slice(0, game.version ? -2 : -1).join("/"));
  // svelte-ignore state_referenced_locally
  let name = $state((game.version ? parts.at(-2) : parts.at(-1)).replace(/\.(exe|x86_64|appimage)$/i, ""));
  let groups = $state({});
  let error = $state("");
  let working = $state(false);

  const kinds = game.kind === "Rom"
    ? [{ id: "Rom", label: "Emulator game", key: "rom" }]
    : [{ id: "Fan", label: "Fan game", key: "fan" }, { id: "NonSteam", label: "Non-Steam game", key: "nonsteam" }];
  const suggestions = $derived(groups[kinds.find((k) => k.id === kind)?.key] ?? []);

  async function go() {
    error = "";
    working = true;
    try {
      const id = await api.relocate(game.id, kind, group.trim(), name.trim(), kind === "Rom" ? "" : version.trim());
      onmoved(id);
    } catch (e) {
      error = String(e);
    } finally {
      working = false;
    }
  }
  onMount(async () => (groups = (await api.groups()) ?? {}));
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="modal">
    <header>
      <h2>Move or rename {game.name}</h2>
      <button class="btn ghost" onclick={onclose}>✕</button>
    </header>
    <div class="body">
      <p class="muted">The folder moves on the shelf, and its saves, artwork, screenshots, history and the copy on this PC go with it. Its Steam entry stays the same, with its play time, and is updated the next time Steam's library is updated.</p>
      {#if kinds.length > 1}
        <div class="kinds">
          {#each kinds as k}
            <button class="kind" class:on={kind === k.id} onclick={() => (kind = k.id)}>{k.label}</button>
          {/each}
        </div>
      {/if}
      <div class="fields">
        <label class="field">
          <span>{kind === "Rom" ? "System" : "Group"}</span>
          <input class="input" list="mgroups" bind:value={group} />
          <datalist id="mgroups">{#each suggestions as s}<option value={s}></option>{/each}</datalist>
        </label>
        <label class="field"><span>Name</span><input class="input" bind:value={name} /></label>
      </div>
      {#if kind !== "Rom"}
        <label class="field ver"><span>Version <i class="faint">(optional)</i></span><input class="input" bind:value={version} placeholder="e.g. v96: keeps several versions of this game side by side" /></label>
        <p class="faint small">A group can have a subgroup: <span class="mono">Sonic The Hedgehog/Classic</span>. With a version, the game goes into <span class="mono">{group || "…"}/{name || "…"}/{version}</span>.</p>
      {/if}
      {#if error}<p class="error">{error}</p>{/if}
    </div>
    <footer>
      <button class="btn" onclick={onclose}>Cancel</button>
      <button class="btn primary" disabled={working || !group.trim() || !name.trim()} onclick={go}>{working ? "Moving…" : "Move"}</button>
    </footer>
  </div>
</div>

<style>
  .kinds { display: flex; gap: 8px; margin: 14px 0; }
  .kind { padding: 9px 14px; border-radius: 10px; border: 1px solid var(--line-2); background: var(--bg-2); color: var(--text); cursor: pointer; }
  .kind.on { border-color: var(--accent); background: rgba(75, 147, 255, .12); }
  .fields { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; margin-top: 12px; }
  .small { font-size: 12px; margin-top: 8px; }
  .ver { margin-top: 12px; }
  .error { color: var(--bad); user-select: text; }
</style>
