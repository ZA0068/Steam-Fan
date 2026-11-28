<script>
  // Settings > Emulators: which emulator plays each system folder in roms/, and the
  // emulators kept on the shelf (downloaded for you, or added from a file).
  import { onMount } from "svelte";
  import { api, pickFile, human } from "./api.js";

  let { c = $bindable(), onerror } = $props();
  let info = $state(null);
  const OS = [["linux", "Linux"], ["windows", "Windows"], ["macos", "macOS"]];

  async function load() {
    try {
      info = await api.emulatorsInfo();
    } catch (e) {
      onerror(String(e));
    }
  }
  onMount(load);
  // jobs end: look again
  $effect(() => {
    const t = setInterval(load, 8000);
    return () => clearInterval(t);
  });

  const shelfOf = (id, os) => info?.on_shelf.find((s) => s.id === id && s.os === os);
  const defOf = (id) => info?.catalog.find((d) => d.id === id);

  async function getEmu(id, os) {
    try {
      await api.emulatorGet(id, os);
    } catch (e) {
      onerror(String(e));
    }
  }
  async function addFile(id, os, folder) {
    const p = await pickFile(folder ? { folder: true } : {});
    if (!p) return;
    try {
      await api.emulatorAdd(id, os, p);
    } catch (e) {
      onerror(String(e));
    }
  }
  function addSystem(sys, id) {
    const d = defOf(id);
    c.emulators.push({ system: sys, uses: id, games_dir: "", extensions: d ? [...d.extensions] : [], command: [], save_dirs: [], skip: ["(UPDATE DATA)", "(DLC)", "Tools"] });
  }
  let newSystem = $state("");
  // every system the catalog knows that isn't set up yet
  const missing = $derived(info ? info.catalog.flatMap((d) => d.systems.map((s) => [s, d.id])).filter(([s]) => !c.emulators.some((e) => e.system.toLowerCase() === s)) : []);
</script>

<section>
  <h3>Emulators</h3>
  <p class="muted">
    Emulators are kept on the shelf like games: each PC copies the one for its system when you play, set up to keep everything in its own folder.
    Their <b>saves, save states and settings</b> (controller layouts too) go back to the shelf after playing, so every PC continues where you stopped.
  </p>

  {#if !info}
    <p class="faint">Looking…</p>
  {:else}
    <div class="table">
      <div class="th">Emulator</div><div class="th">Plays</div>
      {#each OS as [, label]}<div class="th">{label}</div>{/each}
      {#each info.catalog as d}
        <div class="name"><b>{d.name}</b>{#if d.installed_here}<span class="faint small"> · installed here</span>{/if}<br /><a class="faint small" href={d.page} onclick={(e) => { e.preventDefault(); api.open(d.page); }}>{d.page.replace(/^https?:\/\//, "").replace(/\/$/, "")}</a></div>
        <div class="faint small">{d.systems.join(", ")}</div>
        {#each OS as [os]}
          {@const s = shelfOf(d.id, os)}
          <div class="cell">
            {#if s}
              <span class="ok" title="{human(s.size)} on the shelf">✓ {s.version}</span>
              {#if d.github.includes(os)}<button class="link" onclick={() => getEmu(d.id, os)} title="Download the newest release">update</button>{/if}
              <button class="link" onclick={() => addFile(d.id, os, false)} title="Replace it with a file you downloaded">replace</button>
            {:else if d.builds.includes(os)}
              {#if d.github.includes(os)}
                <button class="btn small" onclick={() => getEmu(d.id, os)} title="Download the newest release from GitHub onto the shelf">Get</button>
              {/if}
              <button class="link" onclick={() => addFile(d.id, os, false)} title="An AppImage, zip or 7z you downloaded from {d.page}">file…</button>
              <button class="link" onclick={() => addFile(d.id, os, true)} title="A folder with the emulator in it">folder…</button>
            {:else}
              <span class="faint small">{os === "linux" && d.installed.length ? `package (${d.installed[0]})` : "—"}</span>
            {/if}
          </div>
        {/each}
      {/each}
    </div>
    <p class="faint small">
      <b>Get</b> downloads the newest release onto the shelf{info.server ? " (the server does it)" : ""}. Emulators without downloads on GitHub (Dolphin, Ryujinx, PPSSPP): download from their page and use <b>file…</b>.
      On Linux an emulator installed as a package is used when the shelf has none{#if info.os === "linux"}; Dolphin then keeps its data in Steam-Fan's folder too{/if}.
    </p>

    <h4>Systems</h4>
    <p class="faint small">One per system folder in <span class="mono">roms/</span>.</p>
    {#each c.emulators as e, i}
      {@const d = defOf(e.uses)}
      <div class="emu">
        <div class="grid4">
          <label class="field"><span>System folder</span><input class="input" bind:value={e.system} /></label>
          <label class="field"><span>Emulator</span>
            <select class="input" bind:value={e.uses}>
              {#each info.catalog.filter((x) => x.systems.includes(e.system.toLowerCase())) as x}<option value={x.id}>{x.name}</option>{/each}
              {#each info.catalog.filter((x) => !x.systems.includes(e.system.toLowerCase())) as x}<option value={x.id}>{x.name}</option>{/each}
              <option value="">My own command…</option>
            </select>
          </label>
          <label class="field"><span>Games subfolder</span><input class="input" bind:value={e.games_dir} placeholder="empty: the system folder" /></label>
          <label class="field"><span>File types</span><input class="input" value={e.extensions.join(", ")} oninput={(ev) => (e.extensions = ev.target.value.split(",").map((x) => x.trim()).filter(Boolean))} /></label>
        </div>
        {#if !e.uses}
          <label class="field"><span>Command (<span class="mono">{"{rom}"}</span>: the game file, <span class="mono">{"{dir}"}</span>: its folder)</span><input class="input mono" value={(e.command ?? []).join(" ")} oninput={(ev) => (e.command = ev.target.value.split(" ").filter(Boolean))} /></label>
          <label class="field"><span>Save folders to back up (one per line)</span><textarea class="input mono" rows="2" value={(e.save_dirs ?? []).join("\n")} oninput={(ev) => (e.save_dirs = ev.target.value.split("\n").map((x) => x.trim()).filter(Boolean))}></textarea></label>
        {:else if d}
          <p class="faint small">{d.name} starts games with <span class="mono">{d.args.join(" ")}</span>.</p>
        {/if}
        <button class="btn small ghost danger" onclick={() => c.emulators.splice(i, 1)}>Remove {e.system}</button>
      </div>
    {/each}
    <div class="row">
      {#if missing.length}
        <select class="input sel" bind:value={newSystem}>
          <option value="">Add a system…</option>
          {#each missing as [s, id]}<option value="{s}|{id}">{s} ({defOf(id)?.name})</option>{/each}
        </select>
        <button class="btn small" disabled={!newSystem} onclick={() => { const [s, id] = newSystem.split("|"); addSystem(s, id); newSystem = ""; }}>Add</button>
      {/if}
      <button class="btn small ghost" onclick={() => c.emulators.push({ system: "new", uses: "", games_dir: "", extensions: [], command: [], save_dirs: [], skip: [] })}>＋ Other system</button>
    </div>
    <p class="faint small">Press <b>Save settings</b> below after changing systems. Then <b>Read shelf</b> in the library finds their games.</p>
  {/if}
</section>

<style>
  section { max-width: 920px; background: var(--panel); border: 1px solid var(--line); border-radius: calc(var(--radius) + 2px); padding: 18px 20px; margin-bottom: 16px; }
  h3 { margin: 0 0 6px; font-size: 15px; }
  h4 { margin: 18px 0 4px; font-size: 13px; color: var(--muted); text-transform: uppercase; letter-spacing: .6px; }
  section > p { margin: 0 0 12px; font-size: 13px; }
  .table { display: grid; grid-template-columns: 1.3fr 1fr repeat(3, 1.2fr); border: 1px solid var(--line); border-radius: var(--r); overflow: hidden; font-size: 13px; }
  .table > div { padding: 8px 10px; border-top: 1px solid var(--line); display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .table > .name { display: block; }
  .th { background: var(--bg-2); color: var(--muted); font-size: 11.5px; text-transform: uppercase; letter-spacing: .5px; border-top: 0 !important; }
  .ok { color: var(--good); font-weight: 600; font-size: 12.5px; }
  .link { border: 0; background: none; color: var(--accent-text); cursor: pointer; font-size: 12px; padding: 0; text-decoration: underline; }
  .small { font-size: 12px; }
  .emu { display: grid; gap: 10px; padding: 14px; border: 1px solid var(--line); border-radius: var(--radius); margin: 10px 0; background: var(--bg-2); }
  .emu .btn { justify-self: start; }
  .grid4 { display: grid; grid-template-columns: 1fr 1.2fr 1fr 1.6fr; gap: 10px; }
  .row { display: flex; gap: 8px; align-items: center; margin: 10px 0; }
  .sel { width: 260px; }
  a { text-decoration: none; }
</style>
