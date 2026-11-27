<script>
  // Settings > Appearance. Changes show right away as a preview; Save keeps them.
  // Leaving without saving goes back to the saved look. Custom CSS has to be confirmed
  // within 15 seconds, or it's taken back by itself (in case it hid the button).
  import { onMount, onDestroy } from "svelte";
  import { PRESETS, DEFAULT, clean, apply, load, save, confirmLook, skinData, forgetSkins } from "./theme.js";
  import { BUILTIN } from "./skins.js";
  import { look } from "./look.svelte.js";
  import { api, pickFile } from "./api.js";
  import Icon from "./Icon.svelte";

  // skins: the built-in ones and the ones in the skins folder
  let userSkins = $state([]);
  const loadSkins = async () => (userSkins = (await api.skinsList().catch(() => [])) ?? []);
  async function useSkin(id) {
    error = "";
    try {
      const sk = await skinData(id);
      const th = clean({ ...DEFAULT, ...sk.theme });
      t = { ...th, skin: id, css: t.css };
      // the skin's own layout and cover size, not the ones picked before
      try { localStorage.removeItem("layout"); localStorage.removeItem("zoom"); } catch {}
    } catch (e) {
      error = `that skin can't be used: ${e}`;
    }
  }
  async function installSkin(folder) {
    error = "";
    const p = await pickFile(folder ? { folder: true } : { archives: true });
    if (!p) return;
    try {
      const id = await api.skinInstall(p);
      forgetSkins();
      await loadSkins();
      await useSkin(`user:${id}`);
      flash("Skin installed: Save look to use it");
    } catch (e) {
      error = String(e);
    }
  }
  let exportName = $state(null);
  async function exportSkin() {
    error = "";
    try {
      const cur = await skinData(t.skin);
      const snap = clean($state.snapshot(t));
      const { css, skin, v, ...theme } = snap;
      const json = JSON.stringify({ name: exportName.trim(), author: "", description: "Made in Steam-Fan", theme, layout: look.layout, cover: look.cover || undefined, icon_set: cur.icon_set ?? "glyph", icons: {} }, null, 2);
      const sheet = [cur.css, snap.css].filter((x) => x?.trim()).join("\n\n");
      const id = await api.skinExport(exportName.trim(), json, sheet);
      exportName = null;
      forgetSkins();
      await loadSkins();
      // the look now lives in the skin: your custom CSS went into its style.css
      await store({ ...snap, skin: `user:${id}`, css: "" });
      flash(`Saved as the skin "${id}": edit it in the skins folder, or share the folder`);
    } catch (e) {
      error = String(e);
    }
  }
  async function removeSkin(id) {
    error = "";
    try {
      await api.skinRemove(id);
      forgetSkins();
      if (t.skin === `user:${id}`) await useSkin("default");
      await loadSkins();
    } catch (e) {
      error = String(e);
    }
  }

  let saved = $state(structuredClone(DEFAULT));
  let t = $state(structuredClone(DEFAULT));
  let info = $state({ forced: false, damaged: false, path: "", previous: "" });
  let msg = $state("");
  let error = $state("");
  let countdown = $state(false); // waiting for Keep / Take back
  let ready = $state(false);
  let cancelConfirm = null;

  const same = (a, b) => JSON.stringify(clean(a)) === JSON.stringify(clean(b));
  let changed = $derived(!same(t, saved));
  let cssChanged = $derived((t.css ?? "") !== (saved.css ?? ""));
  // a skin from the skins folder brings a stylesheet: that one has to be confirmed too
  let skinChanged = $derived(t.skin !== saved.skin && t.skin.startsWith("user:"));

  async function reload() {
    const r = await load();
    const f = await api.themeLoad().catch(() => ({ previous: "" }));
    saved = r.theme;
    t = structuredClone(r.theme);
    info = { forced: r.forced, damaged: r.damaged, path: r.path, previous: f.previous ?? "" };
    ready = true;
  }
  onMount(() => {
    reload();
    loadSkins();
    window.addEventListener("sf-theme-reset", reload);
  });
  onDestroy(() => {
    window.removeEventListener("sf-theme-reset", reload);
    cancelConfirm?.();
    // not saved: back to the saved look
    if (ready) apply(saved);
  });

  // live preview of everything but the custom CSS (that one only with Try / Save)
  $effect(() => {
    const snap = $state.snapshot(t);
    if (!ready || countdown) return;
    apply({ ...snap, css: saved.css }, true, snap.skin === saved.skin || !snap.skin.startsWith("user:"));
  });

  function flash(text) {
    msg = text;
    setTimeout(() => (msg = ""), 3000);
  }

  async function store(theme) {
    error = "";
    try {
      await save(theme);
      saved = clean(theme);
      t = clean($state.snapshot(saved));
      info.forced = false;
      const f = await api.themeLoad();
      info.previous = f.previous ?? "";
      flash("Look saved");
    } catch (e) {
      error = String(e);
    }
  }

  /** Save: right away, or after confirming new custom CSS. */
  function onsave() {
    if ((cssChanged && t.css.trim()) || skinChanged) {
      countdown = true;
      apply($state.snapshot(t), true);
      cancelConfirm = confirmLook(15, keep, revert);
    } else {
      store($state.snapshot(t));
    }
  }
  function keep() {
    cancelConfirm = null;
    countdown = false;
    store($state.snapshot(t));
  }
  function revert() {
    cancelConfirm = null;
    countdown = false;
    t.css = saved.css;
    t.skin = saved.skin;
    apply(saved);
    flash("Taken back");
  }

  function usePreset(id) {
    t.preset = id;
    t.colors = {};
  }
  async function toDefault() {
    await store(structuredClone(DEFAULT));
    flash("Back to the default look (Undo brings yours back)");
  }
  async function undo() {
    if (!info.previous) return;
    try {
      await store(clean(JSON.parse(info.previous)));
      flash("Previous look restored");
    } catch {
      error = "the previous look couldn't be read";
    }
  }
  async function copyTheme() {
    try {
      await navigator.clipboard.writeText(JSON.stringify(clean($state.snapshot(t)), null, 2));
      flash("Copied: paste it to share the look");
    } catch {
      error = "couldn't copy";
    }
  }
  async function pasteTheme() {
    try {
      const text = await navigator.clipboard.readText();
      t = clean(JSON.parse(text));
      flash("Look pasted: Save to keep it");
    } catch {
      error = "the clipboard doesn't hold a Steam-Fan look";
    }
  }

  // the color pickers show what's in use, preset or your own
  let base = $derived({ good: "#46d68f", warn: "#f3b94c", bad: "#ff6f6f", ...PRESETS[t.preset], ...t.colors });
  const colorFields = [
    ["bg", "Background"],
    ["panel", "Panels"],
    ["text", "Text"],
    ["accent", "Accent"],
    ["good", "Good / online"],
    ["warn", "Warning"],
    ["bad", "Error"],
  ];
  const fonts = ["Inter", "Noto Sans", "Segoe UI", "SF Pro Text", "Ubuntu", "Cantarell", "Roboto", "JetBrains Mono", "Georgia", "Comic Neue"];
  const vars = "--bg --bg-2 --panel --panel-2 --line --line-2 --text --muted --faint --accent --accent-2 --good --warn --bad --sidebar --radius --r --font --font-size";
</script>

<section>
  <h3>Appearance</h3>
  <p class="muted">
    Changes show right away; <b>Save look</b> keeps them. Whatever happens, <span class="kbd">Ctrl</span>+<span class="kbd">Shift</span>+<span class="kbd">0</span>
    brings back the default look, and so does starting with <span class="mono">steam-fan-app --default-theme</span>.
  </p>
  {#if info.forced}<p class="note">Started with <span class="mono">--default-theme</span>: your saved look isn't used this time. Saving here makes this one your look.</p>{/if}
  {#if info.damaged}<p class="note bad">The saved look ({info.path}) couldn't be read, so the default is used. Saving here replaces it.</p>{/if}

  <h4>Skin</h4>
  <p class="faint small">A skin sets the layout, the icons and the style; the colors and sizes below start from it and can be changed. <span class="muted">Skins of your own are folders with a <span class="mono">skin.json</span> and a <span class="mono">style.css</span>.</span></p>
  <div class="skins">
    {#each Object.entries(BUILTIN) as [id, sk]}
      <button class="skin" class:on={t.skin === id} onclick={() => useSkin(id)} title={sk.description}>
        <span class="layoutIc"><Icon name={sk.layout} /></span>
        <b>{sk.name}</b>
        <span class="faint">{sk.description}</span>
      </button>
    {/each}
    {#each userSkins as sk}
      <div class="skin user" class:on={t.skin === `user:${sk.id}`}>
        <button class="pickSkin" onclick={() => useSkin(`user:${sk.id}`)} title={sk.description}>
          <b>{sk.name}</b>
          <span class="faint">{sk.description || "From the skins folder"}{sk.author ? ` · by ${sk.author}` : ""}</span>
        </button>
        <button class="x" title="Remove this skin" onclick={() => removeSkin(sk.id)}>×</button>
      </div>
    {/each}
  </div>
  <div class="actions tight">
    <button class="btn small" onclick={() => installSkin(false)}>Install skin (zip)…</button>
    <button class="btn small" onclick={() => installSkin(true)}>Install skin (folder)…</button>
    <button class="btn small ghost" onclick={async () => api.open(await api.skinsFolder())}>Open skins folder</button>
    {#if exportName === null}
      <button class="btn small ghost" onclick={() => (exportName = "My look")} title="Make the current look a skin you can edit and share">Save my look as a skin…</button>
    {:else}
      <input class="input small-in" bind:value={exportName} placeholder="Skin name" />
      <button class="btn small" disabled={!exportName.trim()} onclick={exportSkin}>Save skin</button>
      <button class="btn small ghost" onclick={() => (exportName = null)}>Cancel</button>
    {/if}
  </div>

  <h4>Colors</h4>
  <div class="presets">
    {#each Object.entries(PRESETS) as [id, p]}
      <button class="preset" class:on={t.preset === id && !Object.keys(t.colors).length} onclick={() => usePreset(id)} style="--pb:{p.bg};--pp:{p.panel};--pa:{p.accent};--pt:{p.text}">
        <span class="sw"><i></i><i></i><i></i></span>
        <span>{p.label}</span>
      </button>
    {/each}
  </div>
  <div class="colors">
    {#each colorFields as [k, label]}
      <label class="color">
        <input type="color" value={base[k]} oninput={(e) => (t.colors = { ...t.colors, [k]: e.target.value })} />
        <span>{label}</span>
        {#if t.colors[k]}<button class="x" title="Back to the preset's" onclick={(e) => { e.preventDefault(); const c = { ...t.colors }; delete c[k]; t.colors = c; }}>×</button>{/if}
      </label>
    {/each}
  </div>

  <h4>Layout</h4>
  <div class="grid">
    <div class="field">
      <span>Sidebar</span>
      <div class="seg">
        {#each [["left", "Left"], ["right", "Right"], ["icons", "Icons only"]] as [v, l]}<button class:on={t.sidebar === v} onclick={() => (t.sidebar = v)}>{l}</button>{/each}
      </div>
    </div>
    <div class="field">
      <span>Spacing</span>
      <div class="seg">
        {#each [["compact", "Compact"], ["normal", "Normal"], ["roomy", "Roomy"]] as [v, l]}<button class:on={t.density === v} onclick={() => (t.density = v)}>{l}</button>{/each}
      </div>
    </div>
    <label class="field"><span>Text size: {t.font_size}px</span><input type="range" min="11" max="20" bind:value={t.font_size} /></label>
    <label class="field"><span>Corners: {t.radius}px</span><input type="range" min="0" max="24" bind:value={t.radius} /></label>
    <label class="field">
      <span>Font</span>
      <input class="input" list="sf-fonts" placeholder="the default (Inter)" bind:value={t.font} />
      <datalist id="sf-fonts">{#each fonts as f}<option value={f}></option>{/each}</datalist>
    </label>
    <label class="field check"><input type="checkbox" bind:checked={t.glow} /><span>Accent glow behind the pages</span></label>
  </div>

  <h4>Custom CSS</h4>
  <p class="faint small">
    For anything else. Variables you can use: <span class="mono">{vars}</span>. Example:
    <span class="mono">.card .title {"{"} font-weight: 700 {"}"}</span>. New CSS has to be confirmed within 15 seconds after Save, or it's taken back.
  </p>
  <textarea class="input mono css" rows="6" spellcheck="false" placeholder="/* your CSS */" bind:value={t.css}></textarea>

  <div class="actions">
    <button class="btn primary" disabled={!changed} onclick={onsave}>Save look</button>
    <button class="btn" disabled={!changed} onclick={() => (t = clean($state.snapshot(saved)))}>Discard changes</button>
    <button class="btn ghost" onclick={toDefault}>Default look</button>
    <button class="btn ghost" disabled={!info.previous} onclick={undo} title="The look before the last save">Undo last save</button>
    <span class="grow"></span>
    <button class="btn ghost small" onclick={copyTheme}>Copy look</button>
    <button class="btn ghost small" onclick={pasteTheme}>Paste look</button>
  </div>
  {#if msg}<p class="good">{msg}</p>{/if}
  {#if error}<p class="error">{error}</p>{/if}
  {#if info.path}<p class="faint small">Kept in <span class="mono">{info.path}</span>. Delete that file to start over.</p>{/if}
</section>


<style>
  section { max-width: 920px; background: var(--panel); border: 1px solid var(--line); border-radius: calc(var(--radius) + 2px); padding: 18px 20px; margin-bottom: 16px; }
  h3 { margin: 0 0 6px; font-size: 15px; }
  section > p { margin: 0 0 12px; font-size: 13px; }
  p .mono { word-break: normal; }
  h4 { margin: 18px 0 8px; font-size: 13px; color: var(--muted); text-transform: uppercase; letter-spacing: .6px; }
  .note { padding: 8px 12px; border-radius: var(--r); background: rgba(var(--accent-rgb), .1); border: 1px solid rgba(var(--accent-rgb), .35); }
  .note.bad { background: rgba(var(--bad-rgb), .1); border-color: rgba(var(--bad-rgb), .4); }
  .kbd { display: inline-block; padding: 0 6px; border: 1px solid var(--line-2); border-bottom-width: 2px; border-radius: 5px; font-size: 12px; background: var(--bg-2); }
  .skins { display: grid; grid-template-columns: repeat(auto-fill, minmax(190px, 1fr)); gap: 10px; }
  .skin { display: grid; gap: 4px; align-content: start; padding: 12px; border-radius: var(--radius); border: 2px solid var(--line); background: var(--bg-2); color: var(--text); cursor: pointer; text-align: left; font-size: 12px; position: relative; }
  .skin.on { border-color: var(--accent); box-shadow: 0 0 0 3px rgba(var(--accent-rgb), .2); }
  .skin b { font-size: 13.5px; }
  .skin.user { padding: 0; }
  .pickSkin { all: unset; display: grid; gap: 4px; padding: 12px 28px 12px 12px; cursor: pointer; }
  .skin.user .x { position: absolute; top: 6px; right: 6px; }
  .layoutIc { font-size: 18px; color: var(--accent); }
  .actions.tight { margin-top: 10px; }
  .small-in { width: 180px; padding: 5px 10px; font-size: 12.5px; }
  .presets { display: grid; grid-template-columns: repeat(auto-fill, minmax(120px, 1fr)); gap: 10px; }
  .preset { display: grid; gap: 8px; padding: 10px; border-radius: var(--radius); border: 2px solid var(--line); background: var(--pb); color: var(--pt); cursor: pointer; text-align: left; font-size: 12.5px; }
  .preset.on { border-color: var(--accent); box-shadow: 0 0 0 3px rgba(var(--accent-rgb), .2); }
  .sw { display: flex; gap: 5px; }
  .sw i { width: 18px; height: 18px; border-radius: 50%; border: 1px solid rgba(127, 127, 127, .35); }
  .sw i:nth-child(1) { background: var(--pb); }
  .sw i:nth-child(2) { background: var(--pp); }
  .sw i:nth-child(3) { background: var(--pa); }
  .colors { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; }
  .color { display: inline-flex; align-items: center; gap: 8px; padding: 5px 10px 5px 5px; border: 1px solid var(--line); border-radius: 99px; background: var(--bg-2); font-size: 12.5px; cursor: pointer; }
  .color input { width: 26px; height: 26px; padding: 0; border: 0; border-radius: 50%; background: none; cursor: pointer; }
  .color input::-webkit-color-swatch-wrapper { padding: 0; }
  .color input::-webkit-color-swatch { border: 1px solid var(--line-2); border-radius: 50%; }
  .x { border: 0; background: none; color: var(--faint); cursor: pointer; font-size: 15px; padding: 0 2px; }
  .x:hover { color: var(--text); }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 14px 18px; }
  .field { display: grid; gap: 6px; font-size: 12.5px; color: var(--muted); }
  .field.check { display: flex; align-items: center; gap: 8px; align-self: end; padding-bottom: 6px; }
  .seg { display: inline-flex; border: 1px solid var(--line-2); border-radius: var(--r); overflow: hidden; width: max-content; }
  .seg button { border: 0; background: var(--bg-2); color: var(--muted); padding: 7px 12px; cursor: pointer; font-size: 12.5px; }
  .seg button + button { border-left: 1px solid var(--line-2); }
  .seg button.on { background: rgba(var(--accent-rgb), .18); color: var(--text); }
  input[type="range"] { accent-color: var(--accent); }
  .css { min-height: 110px; resize: vertical; font-size: 12px; word-break: normal; }
  .actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 16px; align-items: center; }
  .grow { flex: 1; }
  .small { font-size: 12px; }
  .good { color: var(--good); }
  .error { color: var(--bad); }
</style>
