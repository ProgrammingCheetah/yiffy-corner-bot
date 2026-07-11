<script>
  // The report desk: every post with open reports — who flagged it, why —
  // with the same Take down / Dismiss resolution the DM buttons offer.
  import Icon from '$lib/Icon.svelte';
  import Media from '$lib/Media.svelte';
  import { get, post } from '$lib/api.js';
  import { onMount } from 'svelte';

  let cards = [];
  let loading = true;
  let toast = '';
  let media = {}; // post_id → resolved media (click-to-load)

  function say(t) { toast = t; setTimeout(() => (toast = ''), 3500); }

  async function load() {
    loading = true;
    try {
      cards = (await get('/reports')).cards;
    } catch (e) {
      say(e.message);
    }
    loading = false;
  }

  async function peek(card) {
    if (media[card.post_id]) { delete media[card.post_id]; media = media; return; }
    try {
      media[card.post_id] = await get(`/posts/${card.post_id}/media`);
      media = media;
    } catch { say("Couldn't resolve the media — use the source link."); }
  }

  async function confirmDialog(message) {
    const wa = window.Telegram?.WebApp;
    if (wa?.showConfirm && wa.isVersionAtLeast?.('6.2')) {
      return new Promise((resolve) => wa.showConfirm(message, resolve));
    }
    return confirm(message);
  }

  async function resolve(card, action) {
    if (action === 'takedown') {
      const ok = await confirmDialog(
        `Take post #${card.post_id} down? Its channel messages are deleted and the post is removed.`
      );
      if (!ok) return;
    }
    try {
      const res = await post('/reports/resolve', { post_id: card.post_id, action });
      say(res.message);
      cards = cards.filter((c) => c.post_id !== card.post_id);
    } catch (e) {
      say(e.message);
    }
  }

  function when(iso) {
    return new Date(iso).toLocaleString(undefined, {
      month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit'
    });
  }

  onMount(load);
</script>

<h2>
  Reports
  {#if cards.length}<span class="count">{cards.length} open</span>{/if}
</h2>

{#if loading}
  <p class="muted">Loading…</p>
{:else if !cards.length}
  <div class="empty">Nothing reported. All quiet ✨</div>
{/if}

{#each cards as card (card.post_id)}
  <div class="card">
    <div class="head">
      <strong>#{card.post_id}</strong>
      <span class="chip">{card.status}</span>
      <span class="chip warn">⚠️ {card.report_count}</span>
      <button class="bare src" on:click={() =>
        (window.Telegram?.WebApp?.openLink ?? window.open)(card.source)}>
        Source ↗
      </button>
    </div>

    {#if media[card.post_id]}
      <div class="pane"><Media media={media[card.post_id]} /></div>
    {/if}

    <ul class="reports">
      {#each card.reports as r}
        <li>
          <span class="who">{r.reporter_name ?? `id ${r.reporter_telegram_id}`}</span>
          {#if r.reporter_username}
            <button class="bare handle" on:click={() =>
              (window.Telegram?.WebApp?.openTelegramLink ?? window.open)(
                `https://t.me/${r.reporter_username}`
              )}>
              @{r.reporter_username}
            </button>
          {/if}
          <span class="muted">· {when(r.at)}</span>
          <div class="why">{r.reason ?? 'no reason given'}</div>
        </li>
      {/each}
    </ul>

    <div class="row-btns">
      <button class="ghost" on:click={() => peek(card)}>
        {media[card.post_id] ? 'Hide media' : 'Show media'}
      </button>
      <button class="ghost" on:click={() => resolve(card, 'dismiss')}><Icon name="check" size={16} /> Dismiss</button>
      <button class="danger" on:click={() => resolve(card, 'takedown')}><Icon name="trash" size={16} /> Take down</button>
    </div>
  </div>
{/each}

{#if toast}<div class="toast">{toast}</div>{/if}

<style>
  h2 { margin-bottom: 12px; display: flex; align-items: center; gap: 10px; }
  .count {
    font-size: 0.75rem;
    font-weight: 600;
    color: #f87171;
    background: rgba(248, 113, 113, 0.12);
    border: 1px solid rgba(248, 113, 113, 0.35);
    padding: 3px 11px;
    border-radius: 999px;
  }
  .empty {
    display: flex; align-items: center; justify-content: center;
    height: 30dvh; color: var(--hint); font-size: 0.95rem;
    border: 2px dashed var(--line); border-radius: 20px;
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 16px; padding: 14px; margin-bottom: 12px;
    display: flex; flex-direction: column; gap: 10px;
    box-shadow: 0 3px 14px rgba(0, 0, 0, 0.18);
  }
  .head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .chip.warn { color: #f87171; border-color: rgba(248, 113, 113, 0.4); }
  .bare { background: transparent; padding: 0; color: var(--accent); font-size: 0.85rem; }
  .src { margin-left: auto; }
  .pane { height: 34dvh; }
  .reports { list-style: none; padding: 0; display: flex; flex-direction: column; gap: 8px; }
  .who { font-weight: 600; font-size: 0.88rem; }
  .handle { margin-left: 6px; font-size: 0.85rem; }
  .why {
    margin-top: 2px; font-size: 0.9rem;
    background: rgba(0, 0, 0, 0.22); border-radius: 10px; padding: 8px 11px;
  }
  .row-btns { display: flex; gap: 8px; flex-wrap: wrap; justify-content: flex-end; }
  .ghost { background: transparent; border: 1px solid var(--line); color: inherit; }
  .danger { background: #7f1d1d; }
</style>
