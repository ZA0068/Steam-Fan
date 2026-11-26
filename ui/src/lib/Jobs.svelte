<script>
  import { human } from "./api.js";
  let { jobs, oncancel } = $props();
  let hidden = $state(new Set());
  // finished jobs show for a few seconds (errors stay until closed)
  $effect(() => {
    for (const j of jobs) {
      if (j.done && !j.error && !hidden.has(j.id)) {
        setTimeout(() => (hidden = new Set([...hidden, j.id])), 7000);
      }
    }
  });
  const visible = $derived(jobs.filter((j) => !hidden.has(j.id)).slice(-5));
</script>

<div class="stack">
  {#each visible as j (j.id)}
    <div class="toast" class:err={j.error} class:ok={j.done && !j.error}>
      <div class="top">
        {#if !j.done}<span class="spin"></span>{:else if j.error}<span>⚠</span>{:else}<span>✓</span>{/if}
        <b>{j.title}</b>
        {#if !j.done}
          <button class="x" onclick={() => oncancel(j.id)}>Stop</button>
        {:else}
          <button class="x" onclick={() => (hidden = new Set([...hidden, j.id]))}>✕</button>
        {/if}
      </div>
      {#if !j.done}
        {#if j.fraction != null}
          <div class="bar"><div style="width:{Math.round(j.fraction * 100)}%"></div></div>
          <div class="sub">{Math.round(j.fraction * 100)}% · {human(j.done_bytes)} of {human(j.total_bytes)}</div>
        {/if}
        {#if j.status}<div class="sub ell">{j.status}</div>{/if}
      {:else}
        <div class="sub">{j.error ?? j.result}</div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .stack { position: fixed; right: 18px; bottom: 18px; display: flex; flex-direction: column; gap: 10px; z-index: 70; width: 360px; }
  .toast { background: rgba(18, 29, 49, .96); border: 1px solid var(--line-2); border-radius: 12px; padding: 12px 14px; box-shadow: var(--shadow); animation: rise .2s ease-out; backdrop-filter: blur(8px); }
  .toast.ok { border-color: rgba(70, 214, 143, .45); }
  .toast.err { border-color: rgba(255, 111, 111, .5); }
  .top { display: flex; align-items: center; gap: 8px; }
  .top b { flex: 1; font-size: 13px; }
  .ok .top span { color: var(--good); }
  .err .top span { color: var(--bad); }
  .x { border: 0; background: none; color: var(--muted); cursor: pointer; font-size: 12px; }
  .bar { height: 6px; border-radius: 6px; background: var(--bg-2); margin: 10px 0 6px; overflow: hidden; }
  .bar div { height: 100%; background: linear-gradient(90deg, var(--accent-2), var(--accent)); border-radius: 6px; transition: width .2s; }
  .sub { font-size: 12px; color: var(--muted); margin-top: 4px; user-select: text; }
  .ell { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .spin { width: 12px; height: 12px; border: 2px solid var(--line-2); border-top-color: var(--accent); border-radius: 50%; animation: s .8s linear infinite; }
  @keyframes s { to { transform: rotate(360deg); } }
</style>
