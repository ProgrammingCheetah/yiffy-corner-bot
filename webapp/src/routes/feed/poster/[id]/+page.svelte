<script>
  // One poster's upcoming queue: only what it WOULD post, page by page so
  // long backlogs never lag the device.
  import Loader from '$lib/Loader.svelte';
  import { page } from '$app/stores';
  import Media from '$lib/Media.svelte';
  import { get, del } from '$lib/api.js';
  import { onMount } from 'svelte';

  const posterId = $page.params.id;

  let entries = [];
  let cursor = null;
  let feedEnd = null;
  let nextAfter = null;
  let busy = false;
  let firstLoad = true;
  let toast = '';

  function say(t) { toast = t; setTimeout(() => (toast = ''), 3500); }

  async function loadPage() {
    busy = true;
    try {
      const after = nextAfter != null ? `&after=${nextAfter}` : '';
      const res = await get(`/posters/${posterId}/queue?limit=20${after}`);
      cursor = res.cursor;
      feedEnd = res.feed_end;
      nextAfter = res.next_after;
      entries = [...entries, ...res.entries];
    } catch (e) {
      say(e.message);
    }
    busy = false;
    firstLoad = false;
  }

  async function confirmDialog(message) {
    const wa = window.Telegram?.WebApp;
    if (wa?.showConfirm && wa.isVersionAtLeast?.('6.2')) {
      return new Promise((resolve) => wa.showConfirm(message, resolve));
    }
    return confirm(message);
  }

  // Single-slot media preview: loading a post unloads the previous one,
  // so at most one media element is alive at a time.
  let loadedId = null;
  let loadedMedia = null;
  async function toggleLoad(entry) {
    if (loadedId === entry.post_id) {
      loadedId = loadedMedia = null;
      return;
    }
    loadedId = entry.post_id;
    loadedMedia = null; // shimmer while it resolves
    try {
      const media = await get(`/posts/${entry.post_id}/media`);
      if (loadedId === entry.post_id) loadedMedia = media;
    } catch {
      if (loadedId === entry.post_id) loadedMedia = { kind: 'link' };
    }
  }

  // Removal is feed-wide: soft-delete, every poster skips it.
  async function removePost(entry) {
    const ok = await confirmDialog(
      `Remove post #${entry.post_id} from the feed? Every channel skips it — this is not per-poster.`
    );
    if (!ok) return;
    try {
      const res = await del(`/posts/${entry.post_id}`);
      say(res.message);
      entries = entries.filter((e) => e.post_id !== entry.post_id);
    } catch (e) {
      say(e.message);
    }
  }

  onMount(loadPage);
</script>

<h2>
  <a class="back" href="/feed">←</a>
  Poster #{posterId} queue
  {#if feedEnd != null}<span class="end">cursor {cursor} → end {feedEnd}</span>{/if}
</h2>

{#if firstLoad}
  <Loader label="Scanning the queue…" />
{:else if !entries.length}
  <div class="empty">Nothing this poster would publish — it's all caught up.</div>
{/if}

{#each entries as e (e.post_id)}
  <div class="entry">
    <span class="pos">{e.feed_position}</span>
    <div class="body">
      <div>
        <strong>#{e.post_id}</strong>
        <button class="bare" on:click={() => toggleLoad(e)}>
          {loadedId === e.post_id ? 'Unload' : '▶ Load'}
        </button>
        <button class="bare" on:click={() =>
          (window.Telegram?.WebApp?.openLink ?? window.open)(e.source)}>
          Source ↗
        </button>
        <button class="bare remove" on:click={() => removePost(e)}>🗑 Remove from feed</button>
      </div>
      {#if e.tags.length}
        <div class="tags muted">{e.tags.slice(0, 8).join(' ')}{e.tags.length > 8 ? ' …' : ''}</div>
      {/if}
      {#if loadedId === e.post_id}
        <div class="pane"><Media media={loadedMedia} /></div>
      {/if}
    </div>
  </div>
{/each}

{#if nextAfter != null}
  <button class="more" disabled={busy} on:click={loadPage}>
    {busy ? 'Loading…' : 'Load more'}
  </button>
{:else if entries.length}
  <p class="muted done">That's the whole queue.</p>
{/if}

{#if toast}<div class="toast">{toast}</div>{/if}

<style>
  h2 { margin-bottom: 12px; display: flex; align-items: center; gap: 10px; }
  .back { text-decoration: none; color: var(--accent); font-size: 1.2rem; }
  .end {
    font-size: 0.75rem; font-weight: 600; color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
    padding: 3px 11px; border-radius: 999px;
  }
  .empty {
    padding: 24px; text-align: center; color: var(--hint); font-size: 0.9rem;
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
  .body { display: flex; flex-direction: column; gap: 3px; min-width: 0; flex: 1; }
  .tags { font-size: 0.78rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bare { background: transparent; padding: 0; color: var(--accent); font-size: 0.85rem; }
  .remove { color: #f87171; margin-left: 10px; }
  .more { width: 100%; margin-top: 6px; }
  .pane { height: 34dvh; margin-top: 6px; }
  .done { text-align: center; margin-top: 10px; }
</style>
