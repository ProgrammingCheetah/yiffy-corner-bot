<script>
  // e621 browsing as a deck: right = save into the feed, left = skip.
  import SwipeDeck from '$lib/SwipeDeck.svelte';
  import { get, post } from '$lib/api.js';

  let query = '';
  let page = 1;
  let cards = [];
  let deck;
  let busy = false;
  let toast = '';

  async function search(reset = true) {
    if (reset) { page = 1; cards = []; }
    busy = true;
    try {
      const res = await get(`/browse?tags=${encodeURIComponent(query)}&page=${page}&count=10`);
      cards = [
        ...cards,
        ...res.cards.map((c) => ({
          key: c.source,
          media: { kind: c.mp4_url ? 'video' : 'photo', url: c.mp4_url ?? c.file_url },
          title: c.artists.length ? c.artists.join(', ') : 'e621',
          tags: c.tags,
          artists: [],
          source: c.source
        }))
      ];
      page += 1;
    } catch (e) {
      say(e.message);
    }
    busy = false;
  }

  async function save(card) {
    cards = cards.filter((c) => c.key !== card.key);
    refill();
    try {
      const res = await post('/save', { url: card.source });
      say(res.message);
    } catch (e) {
      say(e.message);
    }
  }

  function skip(card) {
    cards = cards.filter((c) => c.key !== card.key);
    refill();
  }

  function refill() {
    if (cards.length <= 2 && !busy && query) search(false);
  }

  function say(text) {
    toast = text;
    setTimeout(() => (toast = ''), 3000);
  }
</script>

<h2>Browse e621</h2>
<div class="row">
  <input placeholder="wolf male -young …" bind:value={query} on:change={() => search(true)} />
  <button on:click={() => search(true)} disabled={busy}>Go</button>
</div>

<SwipeDeck
  bind:this={deck}
  {cards}
  rightLabel="SAVE"
  leftLabel="SKIP"
  on:right={(e) => save(e.detail)}
  on:left={(e) => skip(e.detail)}
>
  <span slot="empty">{busy ? 'Searching…' : 'Search something, then swipe.'}</span>
</SwipeDeck>

{#if cards.length}
  <div class="actions">
    <button class="round nope" on:click={() => deck.fly(-1)}>✖</button>
    <button class="round like" on:click={() => deck.fly(1)}>💾</button>
  </div>
{/if}
{#if toast}<div class="toast">{toast}</div>{/if}

<style>
  h2 { margin-bottom: 8px; }
  .row { display: flex; gap: 8px; margin-bottom: 12px; }
  .actions { display: flex; justify-content: center; gap: 24px; margin-top: 16px; }
  .round {
    width: 58px; height: 58px; border-radius: 50%; font-size: 1.3rem;
    background: var(--tg-theme-secondary-bg-color, #232e3c);
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.35);
  }
  .round.like { color: #4ade80; }
  .round.nope { color: #f87171; }
  .toast {
    position: fixed; bottom: 86px; left: 50%; transform: translateX(-50%);
    background: rgba(0, 0, 0, 0.85); color: #fff; padding: 10px 16px;
    border-radius: 12px; font-size: 0.85rem; max-width: 90vw; z-index: 50;
  }
</style>
