<script>
  // A poster's profile: binding, cadence, taste (raw DSL + plain reading),
  // publish stats, and every management action in one place.
  import Loader from '$lib/Loader.svelte';
  import TagInput from '$lib/TagInput.svelte';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { get, patch, del } from '$lib/api.js';
  import { onMount } from 'svelte';

  const posterId = $page.params.id;

  let p = null;
  let edit = null;
  let toast = '';

  function say(t) { toast = t; setTimeout(() => (toast = ''), 3500); }

  async function load() {
    try {
      p = await get(`/posters/${posterId}/profile`);
      edit ??= {
        tags: [...p.subscribed, ...p.forbidden.map((t) => `-${t}`)].join(' '),
        rules: p.rules.join(' '),
        interval: p.interval,
        chat: p.chat_id ?? ''
      };
    } catch (e) {
      say(e.message);
    }
  }

  async function run(promise) {
    try {
      const res = await promise;
      say(res.message ?? 'done');
      await load();
    } catch (e) { say(e.message); }
  }

  onMount(load);
</script>

{#if !p}
  <Loader label="Loading poster…" />
{:else}
  <h2>
    <a class="back" href="/admin">←</a>
    Poster #{p.id}
    <span class="muted chatline">{p.chat_id ?? 'unbound'}</span>
  </h2>

  <div class="stats">
    <div class="stat"><strong>{p.published}</strong><span class="muted">published</span></div>
    <div class="stat"><strong>{p.behind}</strong><span class="muted">behind</span></div>
    <div class="stat"><strong>{p.interval}m</strong><span class="muted">interval</span></div>
    <div class="stat">
      <strong>{p.last_published ? new Date(p.last_published).toLocaleDateString() : '—'}</strong>
      <span class="muted">last post</span>
    </div>
  </div>

  <div class="row-btns nav-btns">
    <button on:click={() => goto(`/feed/poster/${p.id}`)}>📜 View queue</button>
  </div>

  <div class="card">
    <h3>taste</h3>
    <label>Tags <TagInput bind:value={edit.tags} /></label>
    {#if p.subscribed_pretty}<p class="pretty">wants: {p.subscribed_pretty}</p>{/if}
    <label>Rules <TagInput bind:value={edit.rules} placeholder="[if…]->[then…] …" /></label>
    {#each p.rules_pretty ?? [] as rule}
      <p class="pretty">rule: {rule}</p>
    {/each}
    <div class="grid">
      <label>Interval <input type="number" min="1" max="60" bind:value={edit.interval} /></label>
      <label>Chat <input bind:value={edit.chat} /></label>
    </div>
    <button on:click={() => run(patch(`/posters/${p.id}`, {
      tags: edit.tags,
      rules: edit.rules,
      interval: Number(edit.interval),
      chat: String(edit.chat)
    }))}>Save</button>
  </div>

  <div class="card">
    <h3>management</h3>
    <div class="row-btns">
      <button class="ghost" on:click={() => run(patch(`/posters/${p.id}`, { announcements: !(p.announcements ?? true) }))}>
        {p.announcements === false ? '🔔 Unmute announcements' : '🔕 Mute announcements'}
      </button>
      <button class="danger" on:click={() => confirm(`Delete poster #${p.id}?`) && run(del(`/posters/${p.id}`).then((r) => { goto('/admin'); return r; }))}>
        Delete poster
      </button>
    </div>
  </div>

  <pre class="summary">{p.summary}</pre>
{/if}

{#if toast}<div class="toast">{toast}</div>{/if}

<style>
  h2 { display: flex; align-items: center; gap: 10px; margin-bottom: 10px; }
  .back { text-decoration: none; color: var(--accent); font-size: 1.2rem; }
  .chatline { font-size: 0.85rem; }
  .stats { display: flex; gap: 8px; flex-wrap: wrap; margin-bottom: 10px; }
  .stat {
    display: flex; flex-direction: column; align-items: center; min-width: 72px;
    background: var(--surface); border: 1px solid var(--line);
    border-radius: 12px; padding: 10px 12px; flex: 1;
  }
  .stat strong { font-size: 1.05rem; }
  .nav-btns { margin-bottom: 12px; }
  .card {
    background: var(--surface); border: 1px solid var(--line);
    border-radius: 16px; padding: 14px; margin-bottom: 12px;
    display: flex; flex-direction: column; gap: 8px;
  }
  .card h3 {
    font-size: 0.78rem; font-weight: 700; text-transform: uppercase;
    letter-spacing: 0.08em; color: var(--hint);
  }
  .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .row-btns { display: flex; gap: 8px; flex-wrap: wrap; }
  .ghost { background: transparent; border: 1px solid var(--line); color: inherit; }
  .danger { background: #7f1d1d; }
  label { font-size: 0.8rem; display: flex; flex-direction: column; gap: 4px; }
  .pretty {
    font-size: 0.78rem; color: var(--hint); margin: -4px 0 0;
    padding-left: 2px; font-style: italic;
  }
  .summary { white-space: pre-wrap; font-size: 0.8rem; background: rgba(0,0,0,0.25); border-radius: 10px; padding: 10px; }
</style>
