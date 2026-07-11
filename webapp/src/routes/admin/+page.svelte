<script>
  // Owner panel: posters, tag policies, users, post lookup. Everything the
  // /set* command family does, with forms instead of syntax memory.
  import { goto } from '$app/navigation';
  import { get, post, patch, del } from '$lib/api.js';
  import { onMount } from 'svelte';

  let section = 'posters';
  let toast = '';
  function say(t) { toast = t; setTimeout(() => (toast = ''), 3500); }
  async function run(promise, reload) {
    try {
      const res = await promise;
      say(res.message ?? 'done');
      reload?.();
    } catch (e) { say(e.message); }
  }

  // Posters
  let posters = [];
  let np = { interval: 60, chat: '', tags: '' };
  let edits = {};
  async function loadPosters() {
    posters = (await get('/posters')).posters;
    for (const p of posters) {
      edits[p.id] ??= {
        tags: [...p.subscribed, ...p.forbidden.map((t) => `-${t}`)].join(' '),
        rules: p.rules.join(' '),
        interval: p.interval,
        chat: p.chat_id ?? ''
      };
    }
  }

  // Tag policies
  let policies = { forbidden: [], forbidden_with_reasons: [], required: [], spoilers: [] };
  let newTag = { forbidden: '', forbiddenReason: '', required: '', spoilers: '' };
  async function loadPolicies() { policies = await get('/tag-policies'); }

  // Users
  let users = [];
  async function loadUsers() { users = (await get('/users')).users; }

  // Post lookup
  let lookup = '';
  let info = null;
  async function doLookup() {
    info = null;
    try { info = await get(`/postinfo/${encodeURIComponent(lookup.trim())}`); }
    catch (e) { say(e.message); }
  }

  onMount(() => { loadPosters(); loadPolicies(); loadUsers(); });
</script>

<div class="tabs">
  {#each [['posters', 'Posters'], ['tags', 'Tags'], ['users', 'Users'], ['lookup', 'Post lookup']] as [id, label]}
    <button class:on={section === id} on:click={() => (section = id)}>{label}</button>
  {/each}
</div>
{#if toast}<div class="toast">{toast}</div>{/if}

{#if section === 'posters'}
  <div class="card">
    <h3>New poster</h3>
    <div class="grid">
      <input type="number" min="1" max="60" bind:value={np.interval} placeholder="minutes" />
      <input bind:value={np.chat} placeholder="@channel or -100…" />
    </div>
    <input bind:value={np.tags} placeholder="tags… (or groups) -forbidden…" />
    <button on:click={() => run(post('/posters', np), loadPosters)}>Create & bind</button>
  </div>
  {#each posters as p (p.id)}
    <div class="card">
      <pre class="summary">{p.summary}</pre>
      <label>Tags <input bind:value={edits[p.id].tags} /></label>
      {#if p.subscribed_pretty}
        <p class="pretty">wants: {p.subscribed_pretty}</p>
      {/if}
      <label>Rules <input bind:value={edits[p.id].rules} placeholder="[if…]->[then…] …" /></label>
      {#if p.rules_pretty?.length}
        {#each p.rules_pretty as rule}
          <p class="pretty">rule: {rule}</p>
        {/each}
      {/if}
      <div class="grid">
        <label>Interval <input type="number" min="1" max="60" bind:value={edits[p.id].interval} /></label>
        <label>Chat <input bind:value={edits[p.id].chat} /></label>
      </div>
      <div class="row-btns">
        <button on:click={() => run(patch(`/posters/${p.id}`, {
          tags: edits[p.id].tags,
          rules: edits[p.id].rules,
          interval: Number(edits[p.id].interval),
          chat: String(edits[p.id].chat)
        }), loadPosters)}>Save</button>
        <button class="ghost" on:click={() => run(patch(`/posters/${p.id}`, { announcements: !(p.announcements ?? true) }), loadPosters)}>
          {p.announcements === false ? '🔔 Unmute announcements' : '🔕 Mute announcements'}
        </button>
        <button class="danger" on:click={() => confirm(`Delete poster #${p.id}?`) && run(del(`/posters/${p.id}`), loadPosters)}>Delete</button>
      </div>
    </div>
  {/each}
{:else if section === 'tags'}
  <div class="card">
    <h3>forbidden</h3>
    {#each policies.forbidden_with_reasons ?? [] as entry (entry.tag)}
      <div class="frow">
        <button class="chip x" on:click={() => run(post('/tag-policies', { list: 'forbidden', tag: entry.tag, add: false }), loadPolicies)}>{entry.tag} ✕</button>
        <span class="muted why">{entry.reason ?? 'no reason recorded — re-add with one to set it'}</span>
      </div>
    {/each}
    <input bind:value={newTag.forbidden} placeholder="tag to forbid…" />
    <div class="grid">
      <input bind:value={newTag.forbiddenReason} placeholder="why? (optional, shown on refusals)" />
      <button on:click={() => { run(post('/tag-policies', { list: 'forbidden', tag: newTag.forbidden, add: true, reason: newTag.forbiddenReason }), loadPolicies); newTag.forbidden = newTag.forbiddenReason = ''; }}>Add</button>
    </div>
  </div>
  {#each ['required', 'spoilers'] as list}
    <div class="card">
      <h3>{list}</h3>
      <div>
        {#each policies[list] as tag}
          <button class="chip x" on:click={() => run(post('/tag-policies', { list, tag, add: false }), loadPolicies)}>{tag} ✕</button>
        {/each}
      </div>
      <div class="grid">
        <input bind:value={newTag[list]} placeholder="add a tag…" />
        <button on:click={() => { run(post('/tag-policies', { list, tag: newTag[list], add: true }), loadPolicies); newTag[list] = ''; }}>Add</button>
      </div>
    </div>
  {/each}
{:else if section === 'users'}
  {#each users as u (u.id)}
    <button class="card urow" on:click={() => goto(`/admin/user/${u.id}`)}>
      <span class="uname">{u.name ?? u.telegram_id}</span>
      <span class="muted uid">#{u.telegram_id}</span>
      {#if u.banned}<span class="chip x">banned</span>{/if}
      {#if u.shadow_banned}<span class="chip ghost-chip">👻</span>{/if}
      <span class="chip">{u.role}</span>
      <span class="chev">›</span>
    </button>
  {/each}
{:else}
  <div class="card">
    <div class="grid">
      <input bind:value={lookup} placeholder="post id or #CODE from a caption" />
      <button on:click={doLookup}>Look up</button>
    </div>
    {#if info}
      <h3>#{info.post_id} — {info.status}</h3>
      <p class="muted">{info.source}</p>
      <div>{#each info.tags.slice(0, 20) as t}<span class="chip">{t}</span>{/each}</div>
      <p class="muted">
        submitted {new Date(info.submitted_at).toLocaleString()}
        {info.submitter ? `by ${info.submitter.name ?? info.submitter.telegram_id}` : ''}
        · reports: {info.report_count}
      </p>
      {#if info.moderator}
        <p class="muted">moderated by {info.moderator.name ?? info.moderator.telegram_id}</p>
      {/if}
      <pre class="summary">{info.verdicts.join('\n')}</pre>
    {/if}
  </div>
{/if}

<style>
  .tabs {
    display: flex; gap: 4px; margin-bottom: 14px; flex-wrap: wrap;
    background: var(--surface); border: 1px solid var(--line);
    border-radius: 14px; padding: 4px;
  }
  .tabs button {
    flex: 1; background: transparent; color: var(--hint);
    padding: 8px 10px; border-radius: 10px; font-size: 0.88rem;
  }
  .tabs button.on {
    background: var(--accent); color: var(--tg-theme-button-text-color, #fff);
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.25);
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 16px; padding: 14px; margin-bottom: 12px;
    display: flex; flex-direction: column; gap: 8px;
    box-shadow: 0 3px 14px rgba(0, 0, 0, 0.18);
  }
  .card h3 {
    font-size: 0.78rem; font-weight: 700; text-transform: uppercase;
    letter-spacing: 0.08em; color: var(--hint);
  }
  .card.row { flex-direction: row; justify-content: space-between; align-items: center; flex-wrap: wrap; }
  /* Users are one uniform tappable line: the name truncates, controls
     live on the profile page. */
  button.card.urow {
    flex-direction: row; align-items: center; gap: 8px; width: 100%;
    background: var(--surface); color: inherit; font-weight: normal;
    text-align: left;
  }
  .uname {
    font-weight: 600; overflow: hidden; text-overflow: ellipsis;
    white-space: nowrap; min-width: 0; flex: 0 1 auto;
  }
  .uid { flex-shrink: 0; }
  .chev { margin-left: auto; color: var(--hint); font-size: 1.1rem; }
  .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .row-btns { display: flex; gap: 8px; flex-wrap: wrap; }
  .summary { white-space: pre-wrap; font-size: 0.8rem; background: rgba(0,0,0,0.25); border-radius: 10px; padding: 10px; }
  .pretty {
    font-size: 0.78rem; color: var(--hint); margin: -4px 0 0;
    padding-left: 2px; font-style: italic;
  }
  .frow { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .chip.ghost-chip { color: #c4b5fd; border-color: rgba(196, 181, 253, 0.4); }
  .why { font-size: 0.78rem; }
  .ghost { background: transparent; border: 1px solid var(--line); color: inherit; }
  .danger { background: #7f1d1d; }
  .chip.x { background: #7f1d1d; border: none; color: #fecaca; }
  select {
    font: inherit; border-radius: 10px; padding: 8px;
    border: 1px solid var(--line);
    background: var(--tg-theme-bg-color, #17212b); color: inherit;
  }
  label { font-size: 0.8rem; display: flex; flex-direction: column; gap: 4px; }
</style>
