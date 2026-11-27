<script>
  import { onMount } from "svelte";
  import { api, pickFile } from "./api.js";

  let { onsaved } = $props();
  let s = $state(null);
  let c = $state(null);
  let keys = $state({ STEAMGRIDDB_API_KEY: "", STEAM_API_KEY: "", STEAM_ID: "" });
  let reachable = $state(null);
  let error = $state("");
  let saved = $state("");

  async function load() {
    const r = await api.settings();
    c = JSON.parse(JSON.stringify(r.config));
    s = r;
  }
  onMount(load);

  // Steam-Fan-Server
  let newServer = $state("");
  let code = $state("");
  let pairing = $state(false);
  let up = $state({});
  $effect(() => {
    if (!c) return;
    for (const u of c.servers) if (!(u in up)) { up[u] = null; api.serverReachable(u).then((r) => (up[u] = r)).catch(() => (up[u] = false)); }
  });
  async function pair() {
    error = "";
    pairing = true;
    try {
      c.servers = await api.serverPair(newServer, code);
      newServer = code = "";
      await load();
      saved = "Paired with the server";
      setTimeout(() => (saved = ""), 2500);
    } catch (e) {
      error = String(e);
    } finally {
      pairing = false;
    }
  }
  function addServer() {
    const u = newServer.trim().replace(/\/+$/, "");
    if (u && !c.servers.includes(u)) c.servers.push(u);
    newServer = "";
  }

  $effect(() => {
    if (!c) return;
    const p = c.shelf;
    api.shelfReachable(p).then((r) => (reachable = r));
  });

  async function browse(field) {
    const p = await pickFile({ folder: true });
    if (p) c[field] = p;
  }
  async function saveKey(name) {
    error = "";
    try {
      await api.setSecret(name, keys[name]);
      keys[name] = "";
      await load();
      saved = "Key saved";
      setTimeout(() => (saved = ""), 2500);
    } catch (e) {
      error = String(e);
    }
  }
  async function save() {
    error = "";
    try {
      await api.saveSettings($state.snapshot(c));
      onsaved();
    } catch (e) {
      error = String(e);
    }
  }
  const words = (a) => a.map((w) => (w.includes(" ") ? `"${w}"` : w)).join(" ");
  const split = (t) => (t.match(/"[^"]*"|\S+/g) ?? []).map((w) => w.replace(/^"|"$/g, ""));
  const keyInfo = [
    { name: "STEAMGRIDDB_API_KEY", label: "SteamGridDB API key", help: "Covers, banners, logos and icons.", link: "https://www.steamgriddb.com/profile/preferences/api" },
    { name: "STEAM_API_KEY", label: "Steam Web API key", help: "Optional: list owned games that aren't archived yet.", link: "https://steamcommunity.com/dev/apikey" },
    { name: "STEAM_ID", label: "Steam id (64-bit)", help: "Optional, with the key above." },
  ];
</script>

<div class="page">
  <header><h1>Settings</h1></header>
  {#if c}
    <div class="scroll">
      <section>
        <h3>Steam-Fan-Server</h3>
        <p class="muted">With a server, the shelf is on it: games, pictures and saves come over the network, and the shelf setting below is not used. Addresses are tried in order (e.g. the home network first, then Tailscale). Leave empty to use the shelf folder directly.</p>
        {#each c.servers as u, i}
          <div class="line">
            <span class="pill"><span class="dot" class:good={up[u]} class:bad={up[u] === false}></span>{up[u] == null ? "checking…" : up[u] ? "answers" : "no answer"}</span>
            <span class="mono grow">{u}</span>
            <button class="btn small ghost" disabled={i === 0} onclick={() => { c.servers.splice(i, 1); c.servers.splice(i - 1, 0, u); }}>↑</button>
            <button class="btn small ghost danger" onclick={() => c.servers.splice(i, 1)}>Remove</button>
          </div>
        {/each}
        <div class="line">
          <input class="input" placeholder="http://10.0.0.36:8484" bind:value={newServer} />
          <input class="input code" placeholder="pairing code" maxlength="6" bind:value={code} />
          <button class="btn" disabled={!newServer.trim() || code.trim().length < 6 || pairing} onclick={pair}>{pairing ? "Pairing…" : "Pair"}</button>
          <button class="btn ghost" disabled={!newServer.trim()} onclick={addServer}>Add without pairing</button>
        </div>
        <p class="faint small">
          {#if s.paired}<span class="dot good"></span> This PC is paired.{:else}Not paired yet.{/if}
          Get a code on the NAS with <span class="mono">docker exec steam-fan-server steam-fan-server pair</span>. Another address of the same server needs no new code.
        </p>
      </section>

      <section>
        <h3>Shelf</h3>
        <p class="muted">Where all your games are stored: the NAS share mounted on this PC, or any folder.</p>
        <div class="line">
          <input class="input" bind:value={c.shelf} />
          <button class="btn" onclick={() => browse("shelf")}>Browse…</button>
        </div>
        <div class="line small">
          <button class="btn small" onclick={() => (c.shelf = "/mnt/nas-games")}>NAS (/mnt/nas-games)</button>
          <button class="btn small" onclick={() => (c.shelf = "~/Games/Shelf")}>Folder on this PC</button>
          <span class="pill"><span class="dot" class:good={reachable} class:bad={reachable === false}></span>{reachable ? "reachable" : "not reachable"}</span>
        </div>
      </section>

      <section>
        <h3>API keys</h3>
        <p class="muted">Kept in <span class="mono">{s.secrets_path}</span>, readable only by you. An environment variable with the same name takes priority.</p>
        {#each keyInfo as k}
          {@const cur = s.keys[k.name]}
          <div class="key">
            <div>
              <b>{k.label}</b>
              <div class="faint small">{k.help} {#if k.link}<a href={k.link} onclick={(e) => { e.preventDefault(); api.open(k.link); }}>Get one</a>{/if}</div>
              <div class="small">{#if cur.masked}<span class="dot good"></span> set ({cur.source}): <span class="mono">{cur.masked}</span>{:else}<span class="faint">not set</span>{/if}</div>
            </div>
            <div class="line">
              <input class="input" type="password" placeholder={cur.masked ? "Replace…" : "Paste here"} bind:value={keys[k.name]} />
              <button class="btn" disabled={!keys[k.name].trim()} onclick={() => saveKey(k.name)}>Save</button>
              {#if cur.source === "secrets.env"}<button class="btn ghost" onclick={() => { keys[k.name] = ""; api.setSecret(k.name, "").then(load); }}>Clear</button>{/if}
            </div>
          </div>
        {/each}
        {#if saved}<p class="good">{saved}</p>{/if}
      </section>

      <section>
        <h3>This PC</h3>
        <div class="grid2">
          <label class="field"><span>Fan games go to</span><input class="input" bind:value={c.fan_local} /></label>
          <label class="field"><span>Non-Steam games go to</span><input class="input" bind:value={c.nonsteam_local} /></label>
          <label class="field"><span>Emulator games go to</span><input class="input" bind:value={c.rom_local} /></label>
          <label class="field"><span>Steam folder</span><input class="input" bind:value={c.steam_root} /></label>
          <label class="field"><span>Proton for Windows games (Linux)</span><input class="input" bind:value={c.proton} placeholder="empty: Steam's default" /></label>
          <label class="field"><span>Wine for Windows games (Mac)</span><input class="input mono" value={words(c.wine)} oninput={(ev) => (c.wine = split(ev.target.value))} placeholder="empty: CrossOver bottle Steam-Fan, else wine" /></label>
        </div>
      </section>

      <section>
        <h3>Emulators</h3>
        <p class="muted">One entry per system folder in <span class="mono">roms/</span>. In the command, <span class="mono">{"{rom}"}</span> is the game file and <span class="mono">{"{dir}"}</span> the game folder.</p>
        {#each c.emulators as e, i}
          <div class="emu">
            <div class="grid3">
              <label class="field"><span>System folder</span><input class="input" bind:value={e.system} /></label>
              <label class="field"><span>Games subfolder</span><input class="input" bind:value={e.games_dir} placeholder="empty: the system folder" /></label>
              <label class="field"><span>File types</span><input class="input" value={e.extensions.join(", ")} oninput={(ev) => (e.extensions = ev.target.value.split(",").map((x) => x.trim()).filter(Boolean))} /></label>
            </div>
            <label class="field"><span>Command</span><input class="input mono" value={words(e.command)} oninput={(ev) => (e.command = split(ev.target.value))} /></label>
            <label class="field"><span>Save folders (one per line)</span><textarea class="input mono" rows="2" value={e.save_dirs.join("\n")} oninput={(ev) => (e.save_dirs = ev.target.value.split("\n").map((x) => x.trim()).filter(Boolean))}></textarea></label>
            <button class="btn small ghost danger" onclick={() => c.emulators.splice(i, 1)}>Remove {e.system}</button>
          </div>
        {/each}
        <button class="btn small" onclick={() => c.emulators.push({ system: "new", games_dir: "GAMES", extensions: [], command: [], save_dirs: [], skip: ["(UPDATE DATA)", "(DLC)", "Tools"] })}>＋ Add a system</button>
      </section>
    </div>
    <footer>
      {#if error}<span class="error">{error}</span>{/if}
      <span class="faint mono">{s.config_path}</span>
      <button class="btn primary" onclick={save}>Save settings</button>
    </footer>
  {/if}
</div>

<style>
  .grow { flex: 1; overflow: hidden; text-overflow: ellipsis; }
  .code { width: 130px; flex: none; }
  .page { display: flex; flex-direction: column; height: 100%; }
  header { padding: 18px 32px 6px; }
  h1 { margin: 0; font-size: 22px; }
  .scroll { flex: 1; overflow: auto; padding: 8px 32px 30px; }
  section { max-width: 920px; background: var(--panel); border: 1px solid var(--line); border-radius: 14px; padding: 18px 20px; margin-bottom: 16px; }
  h3 { margin: 0 0 6px; font-size: 15px; }
  section > p { margin: 0 0 12px; font-size: 13px; }
  .line { display: flex; gap: 8px; align-items: center; }
  .line.small { margin-top: 10px; }
  .key { display: grid; grid-template-columns: 1fr 1.2fr; gap: 16px; align-items: center; padding: 12px 0; border-top: 1px solid var(--line); }
  .small { font-size: 12.5px; margin-top: 4px; display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  a { color: var(--accent); }
  .grid2 { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  .grid3 { display: grid; grid-template-columns: 1fr 1fr 1.4fr; gap: 12px; }
  .emu { display: grid; gap: 10px; padding: 14px; border: 1px solid var(--line); border-radius: 12px; margin-bottom: 12px; background: var(--bg-2); }
  .emu .btn { justify-self: start; }
  textarea { resize: vertical; }
  footer { display: flex; align-items: center; gap: 14px; justify-content: flex-end; padding: 12px 32px; border-top: 1px solid var(--line); }
  .error { color: var(--bad); margin-right: auto; }
  .good { color: var(--good); }
</style>
