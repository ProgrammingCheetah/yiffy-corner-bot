<script>
  // The feed queue: where every poster's cursor sits against the feed end,
  // plus the /feedafter view — everything still ahead of a given post.
  // Tapping a poster opens its own paginated queue page.
  import { goto } from '$app/navigation';
  import { get } from '$lib/api.js';
  import { onMount } from 'svelte';

  let queue = null;
  let token = '';
  let slice = null;
  let busy = false;
  let toast = '';

  function say(t) { toast = t; setTimeout(() => (toast = ''), 3500); }

  async function loadQueue() {
    try {
      queue = await get('/feed/queue');
    } catch (e) {
      say(e.message);
    }
  }

  async function lookAfter() {
    if (!token.trim()) return;
    busy = true;
    slice = null;
    try {
      slice = await get(`/feed/after/${encodeURIComponent(token.trim())}`);
    } catch (e) {
      say(e.message);
    }
    busy = false;
  }

  onMount(loadQueue);
</script>

<h2>
  Feed queue
  {#if queue}<span class="end">end at {queue.feed_end}</span>{/if}
</h2>

{#if queue}
  {#if !queue.posters.length}
    <p class="muted">No posters configured yet.</p>
  {/if}
  {#each queue.posters as p (p.id)}
    <div class="poster">
      <button class="bare head-btn" on:click={() => goto(`/feed/poster/${p.id}`)}>
        <div class="line">
          <strong>Poster #{p.id}</strong>
          <span class="muted">{p.chat_id ?? 'unbound'} · every {p.interval} min</span>
          <span class="behind" class:idle={p.behind === 0}>
            {p.behind === 0 ? 'at feed end' : `${p.behind} behind`}
          </span>
        </div>
        <div class="line muted">
          cursor {p.cursor}
          {#if p.subscribed_pretty} · wants: {p.subscribed_pretty}{/if}
        </div>
        <div class="bar">
          <div
            class="fill"
            style="width: {queue.feed_end ? (p.cursor / queue.feed_end) * 100 : 100}%"
          ></div>
        </div>
      </button>
    </div>
  {/each}
{:else}
  <p class="muted">Loading…</p>
{/if}

<h3>What comes after a post?</h3>
<div class="row">
  <input
    placeholder="post id or #CODE from a caption"
    bind:value={token}
    on:change={lookAfter}
  />
  <button on:click={lookAfter} disabled={busy}>Look</button>
</div>

{#if slice}
  <p class="muted">
    {slice.entries.length} entr{slice.entries.length === 1 ? 'y' : 'ies'} after
    #{slice.anchor.post_id} (position {slice.anchor.feed_position} → end {slice.feed_end})
  </p>
  {#if !slice.entries.length}
    <div class="empty">That post is at the feed end — nothing queued after it.</div>
  {/if}
  {#each slice.entries as e (e.post_id)}
    <div class="entry">
      <span class="pos">{e.feed_position}</span>
      <div class="body">
        <div>
          <strong>#{e.post_id}</strong>
          <span class="chip">{e.status}</span>
          <button class="bare" on:click={() =>
            (window.Telegram?.WebApp?.openLink ?? window.open)(e.source)}>
            Source ↗
          </button>
        </div>
        {#if e.tags.length}
          <div class="tags muted">{e.tags.slice(0, 8).join(' ')}{e.tags.length > 8 ? ' …' : ''}</div>
        {/if}
      </div>
    </div>
  {/each}
{/if}

{#if toast}<div class="toast">{toast}</div>{/if}

<style>
  h2 { margin-bottom: 12px; display: flex; align-items: center; gap: 10px; }
  h3 { margin: 18px 0 8px; font-size: 1rem; }
  .end {
    font-size: 0.75rem; font-weight: 600; color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
    padding: 3px 11px; border-radius: 999px;
  }
  .poster {
    background: var(--surface); border: 1px solid var(--line);
    border-radius: 14px; padding: 12px; margin-bottom: 10px;
    display: flex; flex-direction: column; gap: 6px;
  }
  .head-btn {
    display: flex; flex-direction: column; gap: 6px; width: 100%;
    text-align: left; font-weight: normal;
    color: inherit; font-size: inherit;
  }
  .line { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-size: 0.9rem; }
  .behind {
    margin-left: auto; font-size: 0.75rem; font-weight: 600; color: #facc15;
    background: rgba(250, 204, 21, 0.1); border: 1px solid rgba(250, 204, 21, 0.35);
    padding: 2px 9px; border-radius: 999px;
  }
  .behind.idle { color: #4ade80; background: rgba(74, 222, 128, 0.1); border-color: rgba(74, 222, 128, 0.35); }
  .bar {
    height: 5px; border-radius: 999px; overflow: hidden;
    background: rgba(128, 128, 128, 0.18);
  }
  .fill { height: 100%; background: var(--accent); border-radius: 999px; transition: width 0.4s ease; }
  .row { display: flex; gap: 8px; margin-bottom: 10px; }
  .empty {
    padding: 18px; text-align: center; color: var(--hint); font-size: 0.9rem;
    border: 2px dashed var(--line); border-radius: 14px;
  }
  .entry {
    display: flex; gap: 10px; align-items: flex-start;
    background: var(--surface); border: 1px solid var(--line);
    border-radius: 12px; padding: 10px 12px; margin-bottom: 8px;
  }
  .pos {
    font-size: 0.78rem; font-weight: 700; color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    border-radius: 8px; padding: 3px 8px; min-width: 34px; text-align: center;
  }
  .body { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .tags { font-size: 0.78rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bare { background: transparent; padding: 0; color: var(--accent); font-size: 0.85rem; }
</style>
