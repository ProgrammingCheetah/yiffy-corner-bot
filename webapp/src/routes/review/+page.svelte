<script>
  // The moderation deck. Swipe right = approve, left = reject; the button
  // row adds "accept with tags", "reject with reason" and "request changes",
  // exactly like the DM dialogues.
  import SwipeDeck from '$lib/SwipeDeck.svelte';
  import Modal from '$lib/Modal.svelte';
  import { get, post } from '$lib/api.js';
  import { onMount } from 'svelte';

  let cards = [];
  let deck;
  let toast = '';
  let tagModal = false;
  let reasonModal = false;
  let changesModal = false;
  let extraTags = '';
  let reason = '';
  let changes = '';

  async function load() {
    const res = await get('/queue');
    cards = res.cards.map((c) => ({
      key: c.post_id,
      post_id: c.post_id,
      media: null,
      title: `#${c.post_id}`,
      tags: c.tags,
      artists: c.artists,
      source: c.source,
      footer: c.submitter ? `submitted by ${c.submitter.name ?? c.submitter.telegram_id}` : 'admin add'
    }));
    hydrate();
  }

  // Media resolves lazily for the top two cards (e621 rate pacing).
  async function hydrate() {
    for (const card of cards.slice(0, 2)) {
      if (card.media) continue;
      try {
        card.media = await get(`/posts/${card.post_id}/media`);
        cards = cards;
      } catch { card.media = { kind: 'link' }; cards = cards; }
    }
  }

  async function act(card, action, extra = {}) {
    cards = cards.filter((c) => c.key !== card.key);
    hydrate();
    try {
      const res = await post('/moderate', { post_id: card.post_id, action, ...extra });
      say(res.message);
    } catch (e) {
      say(e.message);
      load(); // put it back — the action failed
    }
  }

  function say(text) {
    toast = text;
    setTimeout(() => (toast = ''), 3000);
  }

  onMount(load);
</script>

<h2>Review queue <span class="count">{cards.length} waiting</span></h2>

<SwipeDeck
  bind:this={deck}
  {cards}
  rightLabel="APPROVE"
  leftLabel="REJECT"
  on:right={(e) => act(e.detail, 'approve')}
  on:left={(e) => act(e.detail, 'reject')}
>
  <span slot="empty">Queue's clean. Go touch grass 🌿</span>
</SwipeDeck>

{#if cards.length}
  <div class="actions">
    <button class="round nope" on:click={() => deck.fly(-1)} title="Reject">✖</button>
    <button class="round reason" on:click={() => { reason = ''; reasonModal = true; }} title="Reject with reason">📝</button>
    <button class="round changes" on:click={() => { changes = ''; changesModal = true; }} title="Request changes">✏️</button>
    <button class="round tags" on:click={() => { extraTags = ''; tagModal = true; }} title="Accept with tags">🏷</button>
    <button class="round like" on:click={() => deck.fly(1)} title="Approve">✔</button>
  </div>
{/if}

{#if toast}<div class="toast">{toast}</div>{/if}

<Modal bind:open={tagModal} title="Accept with extra tags">
  <input placeholder="extra tags (duplicates ignored)" bind:value={extraTags} />
  <button
    disabled={!extraTags.trim()}
    on:click={() => {
      const card = cards[0];
      tagModal = false;
      act(card, 'approve', { extra_tags: extraTags.split(/\s+/).filter(Boolean) });
    }}>Accept into the feed</button>
</Modal>

<Modal bind:open={changesModal} title="Request changes">
  <textarea rows="3" placeholder="What should the submitter change? They can re-submit the same link." bind:value={changes}></textarea>
  <button
    disabled={!changes.trim()}
    on:click={() => {
      const card = cards[0];
      changesModal = false;
      act(card, 'changes', { reason: changes });
    }}>Send the change request</button>
</Modal>

<Modal bind:open={reasonModal} title="Reject with reason">
  <textarea rows="3" placeholder="The reason is DM'd to the submitter" bind:value={reason}></textarea>
  <button
    disabled={!reason.trim()}
    on:click={() => {
      const card = cards[0];
      reasonModal = false;
      act(card, 'reject', { reason });
    }}>Reject and tell them why</button>
</Modal>

<style>
  h2 { margin-bottom: 10px; display: flex; justify-content: space-between; align-items: center; }
  .count {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
    padding: 3px 11px;
    border-radius: 999px;
  }
  .round.like { color: #4ade80; }
  .round.nope { color: #f87171; }
  .round.tags { color: #facc15; }
  .round.reason { color: #93c5fd; }
  .round.changes { color: #c4b5fd; }
</style>
