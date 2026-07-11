<script>
  // A user's profile: who they are, their submission record, and every
  // administrative action (role, ban, shadowban) in one place.
  import Loader from '$lib/Loader.svelte';
  import { page } from '$app/stores';
  import { get, post, patch } from '$lib/api.js';
  import { onMount } from 'svelte';

  const userId = $page.params.id;

  let profile = null;
  let submissions = [];
  let nextOffset = null;
  let busy = false;
  let toast = '';

  function say(t) { toast = t; setTimeout(() => (toast = ''), 3500); }

  async function load(offset = 0) {
    busy = true;
    try {
      const res = await get(`/users/${userId}/profile?offset=${offset}`);
      profile = res;
      submissions = offset === 0 ? res.submissions : [...submissions, ...res.submissions];
      nextOffset = res.next_offset;
    } catch (e) {
      say(e.message);
    }
    busy = false;
  }

  async function run(promise) {
    try {
      const res = await promise;
      say(res.message ?? 'done');
      await load(0);
    } catch (e) { say(e.message); }
  }

  async function confirmDialog(message) {
    const wa = window.Telegram?.WebApp;
    if (wa?.showConfirm && wa.isVersionAtLeast?.('6.2')) {
      return new Promise((resolve) => wa.showConfirm(message, resolve));
    }
    return confirm(message);
  }

  async function changeRole(role, select) {
    if (role === 'owner') {
      const ok = await confirmDialog(
        `Make ${profile.user.name ?? profile.user.telegram_id} an OWNER? Owners have full control. This is not easily undone.`
      );
      if (!ok) { select.value = profile.user.role; return; }
    }
    run(patch(`/users/${userId}`, { role }));
  }

  onMount(() => load(0));
</script>

{#if !profile}
  <Loader label="Loading profile…" />
{:else}
  <h2>
    <a class="back" href="/admin">←</a>
    <span class="name">{profile.user.name ?? `id ${profile.user.telegram_id}`}</span>
  </h2>
  <p class="muted sub">
    #{profile.user.telegram_id} · {profile.user.role}
    {#if profile.user.banned} · <span class="bad">banned</span>{/if}
    {#if profile.user.shadow_banned} · <span class="ghosted">👻 shadowbanned</span>{/if}
  </p>

  <div class="stats">
    <div class="stat">
      <strong>{profile.stats.total}</strong>
      <span class="muted">submitted</span>
    </div>
    {#each profile.stats.by_status as s (s.status)}
      <div class="stat">
        <strong>{s.count}</strong>
        <span class="muted">{s.status}</span>
      </div>
    {/each}
  </div>

  <div class="card">
    <h3>administrative</h3>
    <div class="row-btns">
      <select value={profile.user.role} on:change={(e) => changeRole(e.target.value, e.target)}>
        <option value="user">User</option>
        <option value="moderator">Moderator</option>
        <option value="owner">Owner</option>
      </select>
      <button class="ghost" on:click={() => run(patch(`/users/${userId}`, { banned: !profile.user.banned }))}>
        {profile.user.banned ? 'Unban' : 'Ban'}
      </button>
      <button class="ghost" title="They keep the full flow; nothing ever lands."
        on:click={() => run(post('/shadowban', { telegram_id: profile.user.telegram_id, banned: !profile.user.shadow_banned }))}>
        {profile.user.shadow_banned ? '👻 Lift shadowban' : '👻 Shadowban'}
      </button>
    </div>
  </div>

  <h3 class="section">Submitted artwork</h3>
  {#if !submissions.length}
    <div class="empty">Nothing submitted yet.</div>
  {/if}
  {#each submissions as s (s.post_id)}
    <div class="entry">
      <div class="body">
        <div>
          <strong>#{s.post_id}</strong>
          <span class="chip">{s.status}</span>
          {#if s.feed_position != null}<span class="chip">pos {s.feed_position}</span>{/if}
          <button class="bare" on:click={() =>
            (window.Telegram?.WebApp?.openLink ?? window.open)(s.source)}>
            Source ↗
          </button>
        </div>
        <div class="muted when">{new Date(s.submitted_at).toLocaleString()}</div>
      </div>
    </div>
  {/each}
  {#if nextOffset != null}
    <button class="more" disabled={busy} on:click={() => load(nextOffset)}>
      {busy ? 'Loading…' : 'Load more'}
    </button>
  {/if}
{/if}

{#if toast}<div class="toast">{toast}</div>{/if}

<style>
  h2 { display: flex; align-items: center; gap: 10px; margin-bottom: 2px; }
  .back { text-decoration: none; color: var(--accent); font-size: 1.2rem; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .sub { margin-bottom: 12px; }
  .bad { color: #f87171; }
  .ghosted { color: #c4b5fd; }
  .stats { display: flex; gap: 8px; flex-wrap: wrap; margin-bottom: 12px; }
  .stat {
    display: flex; flex-direction: column; align-items: center; min-width: 72px;
    background: var(--surface); border: 1px solid var(--line);
    border-radius: 12px; padding: 10px 12px;
  }
  .stat strong { font-size: 1.15rem; }
  .card {
    background: var(--surface); border: 1px solid var(--line);
    border-radius: 16px; padding: 14px; margin-bottom: 12px;
    display: flex; flex-direction: column; gap: 8px;
  }
  .card h3, .section {
    font-size: 0.78rem; font-weight: 700; text-transform: uppercase;
    letter-spacing: 0.08em; color: var(--hint);
  }
  .section { margin: 14px 0 8px; }
  .row-btns { display: flex; gap: 8px; flex-wrap: wrap; }
  .ghost { background: transparent; border: 1px solid var(--line); color: inherit; }
  select {
    font: inherit; border-radius: 10px; padding: 8px;
    border: 1px solid var(--line);
    background: var(--tg-theme-bg-color, #17212b); color: inherit;
  }
  .empty {
    padding: 18px; text-align: center; color: var(--hint); font-size: 0.9rem;
    border: 2px dashed var(--line); border-radius: 14px;
  }
  .entry {
    background: var(--surface); border: 1px solid var(--line);
    border-radius: 12px; padding: 10px 12px; margin-bottom: 8px;
  }
  .body { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .when { font-size: 0.78rem; }
  .bare { background: transparent; padding: 0; color: var(--accent); font-size: 0.85rem; }
  .more { width: 100%; margin-top: 6px; }
</style>
