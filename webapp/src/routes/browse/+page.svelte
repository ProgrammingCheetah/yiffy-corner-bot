<script>
  // e621 browsing as a deck: right = save into the feed, left = skip.
  import Icon from '$lib/Icon.svelte';
  import TagInput from '$lib/TagInput.svelte';
  import SwipeDeck from '$lib/SwipeDeck.svelte';
  import { get, post } from '$lib/api.js';
  import { loadJson, saveJson } from '$lib/store.js';
  import { onMount } from 'svelte';

  let pinned = [];
  let history = [];
  onMount(async () => {
    pinned = await loadJson('browse_pinned', []);
    history = await loadJson('browse_history', []);
  });

  $: isPinned = pinned.includes(query.trim());
  function togglePin() {
    const q = query.trim();
    if (!q) return;
    pinned = isPinned ? pinned.filter((p) => p !== q) : [q, ...pinned];
    saveJson('browse_pinned', pinned);
  }
  function unpin(q) {
    pinned = pinned.filter((p) => p !== q);
    saveJson('browse_pinned', pinned);
  }
  function remember(q) {
    history = [q, ...history.filter((h) => h !== q)].slice(0, 15);
    saveJson('browse_history', history);
  }
  function runChip(q) {
    query = q;
    search(true);
  }
  function clearHistory() {
    history = [];
    saveJson('browse_history', []);
  }

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
      if (reset && query.trim()) remember(query.trim());
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

  // Skip *forever*: the source goes on the server-side skiplist so browse
  // never shows it again — the manual verdict for video re-uploads that
  // dedupe can't catch.
  async function skipForever() {
    const card = cards[0];
    if (!card) return;
    try {
      const res = await post('/browse/skip', { url: card.source });
      say(res.message);
      deck.fly(-1);
    } catch (e) {
      say(e.message);
    }
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
  <TagInput placeholder="wolf male -young …" bind:value={query} on:change={() => search(true)} />
  <button class="pin" class:on={isPinned} title="Pin this query" on:click={togglePin}>
    {isPinned ? '📌' : '📍'}
  </button>
  <button on:click={() => search(true)} disabled={busy}>Go</button>
</div>

{#if pinned.length}
  <div class="qrow">
    <span class="lbl">📌</span>
    {#each pinned as q (q)}
      <span class="qchip">
        <button class="bare" on:click={() => runChip(q)}>{q}</button>
        <button class="bare x" title="Unpin" on:click={() => unpin(q)}>✕</button>
      </span>
    {/each}
  </div>
{/if}
{#if history.filter((h) => !pinned.includes(h)).length}
  <div class="qrow">
    <span class="lbl">🕘</span>
    {#each history.filter((h) => !pinned.includes(h)) as q (q)}
      <button class="qchip" on:click={() => runChip(q)}>{q}</button>
    {/each}
    <button class="bare clear" on:click={clearHistory}>clear</button>
  </div>
{/if}

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
    <button class="round nope" on:click={() => deck.fly(-1)} title="Skip for now"><Icon name="x" /></button>
    <button class="round never" on:click={skipForever} title="Never show again"><Icon name="ban" /></button>
    <button class="round like" on:click={() => deck.fly(1)} title="Save to the feed"><Icon name="save" /></button>
  </div>
{/if}
{#if toast}<div class="toast">{toast}</div>{/if}

<style>
  h2 { margin-bottom: 8px; }
  .row { display: flex; gap: 8px; margin-bottom: 12px; }
  .pin { background: transparent; padding: 6px 8px; font-size: 1.05rem; filter: grayscale(1); }
  .pin.on { filter: none; }
  .qrow {
    display: flex; flex-wrap: wrap; gap: 6px; align-items: center;
    margin: -4px 0 10px;
  }
  .lbl { font-size: 0.85rem; }
  .qchip {
    display: inline-flex; align-items: center; gap: 6px;
    background: var(--tg-theme-secondary-bg-color, #232e3c);
    color: inherit; padding: 4px 11px; border-radius: 999px; font-size: 0.78rem;
  }
  .bare { background: transparent; padding: 0; color: inherit; font-size: inherit; }
  .bare.x { color: #f87171; }
  .bare.clear { color: var(--tg-theme-hint-color, #7d8b99); font-size: 0.75rem; margin-left: 4px; }
  .round.like { color: #4ade80; }
  .round.nope { color: #f87171; }
</style>
