<script>
  // Submit: paste a link → live preview (media, dup check, e621 tags) →
  // confirm. Non-e621 sources demand tags; artist:<name> credits artists.
  import Media from '$lib/Media.svelte';
  import { post } from '$lib/api.js';

  let url = '';
  let preview = null;
  let tags = '';
  let busy = false;
  let message = '';
  let error = '';

  async function resolve() {
    error = message = '';
    preview = null;
    if (!url.trim()) return;
    busy = true;
    try {
      preview = await post('/resolve', { url: url.trim() });
    } catch (e) {
      error = e.message;
    }
    busy = false;
  }

  async function send() {
    error = message = '';
    busy = true;
    try {
      const res = await post('/suggest', {
        url: url.trim(),
        tags: tags.split(/\s+/).filter(Boolean)
      });
      message = res.message;
      preview = null;
      url = tags = '';
    } catch (e) {
      error = e.message;
    }
    busy = false;
  }
</script>

<h2>Submit art</h2>
<p class="muted">e621 · FurAffinity · Twitter/X · BlueSky · DeviantArt · t.me</p>

<div class="row">
  <input placeholder="https://…" bind:value={url} on:change={resolve} />
  <button on:click={resolve} disabled={busy}>Preview</button>
</div>

{#if error}<p class="err">{error}</p>{/if}
{#if message}<p class="ok">{message}</p>{/if}

{#if preview}
  <div class="preview">
    <div class="pane"><Media media={preview.media} /></div>
    {#if preview.duplicate_of}
      <p class="err">Already in the system as post #{preview.duplicate_of}.</p>
    {:else}
      {#if preview.tags.length}
        <div>{#each preview.tags.slice(0, 20) as t}<span class="chip">{t}</span>{/each}</div>
      {/if}
      {#if preview.artists.length}
        <p class="muted">by {preview.artists.join(', ')}</p>
      {/if}
      {#if preview.needs_tags}
        <label>
          Tags (required) — credit with <code>artist:&lt;name&gt;</code>
          <input placeholder="wolf male solo artist:coolwolf" bind:value={tags} />
        </label>
      {:else}
        <label>
          Extra tags (optional)
          <input placeholder="extra tags…" bind:value={tags} />
        </label>
      {/if}
      <button
        class="confirm"
        disabled={busy || (preview.needs_tags && !tags.trim())}
        on:click={send}
      >
        ✅ Looks right — submit it
      </button>
    {/if}
  </div>
{/if}

<style>
  h2 { margin-bottom: 2px; }
  .row { display: flex; gap: 8px; margin: 14px 0; }
  .preview {
    display: flex;
    flex-direction: column;
    gap: 10px;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 18px;
    padding: 12px;
    box-shadow: 0 6px 22px rgba(0, 0, 0, 0.25);
    animation: pop 0.22s ease;
  }
  @keyframes pop {
    from { opacity: 0; transform: translateY(8px) scale(0.98); }
  }
  .pane { height: 42dvh; }
  .confirm { padding: 14px; font-size: 1rem; }
  .err,
  .ok {
    margin: 8px 0;
    padding: 10px 14px;
    border-radius: 12px;
    font-size: 0.9rem;
  }
  .err {
    color: #f87171;
    background: rgba(248, 113, 113, 0.12);
    border: 1px solid rgba(248, 113, 113, 0.35);
  }
  .ok {
    color: #4ade80;
    background: rgba(74, 222, 128, 0.12);
    border: 1px solid rgba(74, 222, 128, 0.35);
  }
  label { display: flex; flex-direction: column; gap: 6px; font-size: 0.85rem; }
</style>
