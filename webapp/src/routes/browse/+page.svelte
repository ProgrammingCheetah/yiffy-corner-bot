<script>
  // e621 browsing as a deck: right = save into the feed, left = skip.
  import Loader from '$lib/Loader.svelte';
  import Icon from '$lib/Icon.svelte';
  import TagInput from '$lib/TagInput.svelte';
  import SwipeDeck from '$lib/SwipeDeck.svelte';
  import { get, post } from '$lib/api.js';
  import { loadJson, saveJson } from '$lib/store.js';
  import { session } from '$lib/browse_session.js';
  import { goto } from '$app/navigation';
  import { page as route } from '$app/stores';
  import { onMount } from 'svelte';

  let pinned = [];
  let history = [];
  // The "fulfilling request" toggle: while armed (server-side, survives
  // restarts), every save is stamped and the publication caption reads
  // "Fulfilling request <text>" — until it's turned off here or via
  // /fulfilling off in the bot.
  let fulfilling = null;
  let wishOpen = false;
  let wishInput = '';
  onMount(async () => {
    pinned = await loadJson('browse_pinned', []);
    history = await loadJson('browse_history', []);
    try {
      fulfilling = (await get('/browse/fulfilling')).request;
    } catch {
      // Non-moderators (or a flaky start) just don't see the toggle state.
    }
    // A query handed over from the Saved/History page: fill and run it.
    const handed = $route.url.searchParams.get('q');
    if (handed && handed !== query) {
      query = handed;
      search(true);
    }
  });

  $: isPinned = pinned.includes(query.trim());
  function togglePin() {
    const q = query.trim();
    if (!q) return;
    pinned = isPinned ? pinned.filter((p) => p !== q) : [q, ...pinned];
    saveJson('browse_pinned', pinned);
  }
  function remember(q) {
    history = [q, ...history.filter((h) => h !== q)].slice(0, 15);
    saveJson('browse_history', history);
  }

  // Restored from the module-level session, so the deck survives
  // navigating away and back.
  let query = session.query;
  let page = session.page;
  let cards = session.cards;
  $: Object.assign(session, { query, page, cards });
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

  async function armFulfilling() {
    const text = wishInput.trim();
    if (!text) return;
    try {
      const res = await post('/browse/fulfilling', { request: text });
      fulfilling = res.request;
      wishOpen = false;
      wishInput = '';
      say(res.message);
    } catch (e) {
      say(e.message);
    }
  }

  async function disarmFulfilling() {
    try {
      const res = await post('/browse/fulfilling', { request: '' });
      fulfilling = null;
      say(res.message);
    } catch (e) {
      say(e.message);
    }
  }

  function say(text) {
    toast = text;
    setTimeout(() => (toast = ''), 3000);
  }
</script>

<h2>Browse e621</h2>
<div class="row">
  <TagInput placeholder="wolf male -young …" bind:value={query} on:change={() => search(true)} />
  <button class="pin" class:on={isPinned} title={isPinned ? 'Unpin this query' : 'Pin this query'} on:click={togglePin}>
    <Icon name="pin" size={18} />
  </button>
  <button class="pin" title="Saved & history" on:click={() => goto('/browse/queries')}>
    <Icon name="clock" size={18} />
  </button>
  <button on:click={() => search(true)} disabled={busy}>Go</button>
</div>

{#if fulfilling !== null}
  <div class="fulfilling on">
    <Icon name="sparkles" size={16} />
    <span class="wish">Fulfilling request <b>“{fulfilling}”</b> — every save gets stamped.</span>
    <button class="ghosty" title="Stop fulfilling" on:click={disarmFulfilling}>
      <Icon name="x" size={16} />
    </button>
  </div>
{:else if wishOpen}
  <div class="fulfilling">
    <!-- svelte-ignore a11y-autofocus -->
    <input
      placeholder="what did they wish for…"
      bind:value={wishInput}
      autofocus
      on:keydown={(e) => e.key === 'Enter' && armFulfilling()}
    />
    <button on:click={armFulfilling} disabled={!wishInput.trim()}>On</button>
    <button class="ghosty" title="Never mind" on:click={() => (wishOpen = false)}>
      <Icon name="x" size={16} />
    </button>
  </div>
{:else}
  <button class="fulfill-hint" on:click={() => (wishOpen = true)}>
    <Icon name="sparkles" size={14} /> Fulfilling a request?
  </button>
{/if}

<SwipeDeck
  bind:this={deck}
  {cards}
  rightLabel="SAVE"
  leftLabel="SKIP"
  on:right={(e) => save(e.detail)}
  on:left={(e) => skip(e.detail)}
>
  <span slot="empty">
    {#if busy}<Loader label="Searching…" />{:else}Search something, then swipe.{/if}
  </span>
</SwipeDeck>

{#if cards.length}
  <div class="actions">
    <div class="action-col">
      <button class="round nope" on:click={() => deck.fly(-1)}><Icon name="x" /></button>
      <span class="action-lbl">Skip</span>
    </div>
    <div class="action-col">
      <button class="round never" on:click={skipForever}><Icon name="ban" /></button>
      <span class="action-lbl">Never</span>
    </div>
    <div class="action-col">
      <button class="round like" on:click={() => deck.fly(1)}><Icon name="save" /></button>
      <span class="action-lbl">Save</span>
    </div>
  </div>
{/if}
{#if toast}<div class="toast">{toast}</div>{/if}

<style>
  h2 { margin-bottom: 8px; }
  .row { display: flex; gap: 8px; margin-bottom: 12px; }
  .pin { background: transparent; padding: 6px 8px; color: var(--hint); }
  .pin.on { color: var(--accent); }
  .round.like { color: #4ade80; }
  .round.nope { color: #f87171; }
  .fulfilling {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: -4px 0 12px;
    padding: 6px 10px;
    border-radius: 10px;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .fulfilling.on { color: var(--accent); }
  .fulfilling .wish { flex: 1; font-size: 0.9em; }
  .fulfilling input { flex: 1; min-width: 0; }
  .ghosty { background: transparent; padding: 4px 6px; color: var(--hint); }
  .fulfill-hint {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin: -4px 0 12px;
    background: transparent;
    padding: 4px 6px;
    color: var(--hint);
    font-size: 0.9em;
  }
</style>
