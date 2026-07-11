<script>
  // Saved and historic browse queries, out of the Browse page's way.
  // Tapping one jumps back to Browse and runs it.
  import Icon from '$lib/Icon.svelte';
  import { goto } from '$app/navigation';
  import { loadJson, saveJson } from '$lib/store.js';
  import { onMount } from 'svelte';

  let tab = 'saved';
  let pinned = [];
  let history = [];

  onMount(async () => {
    pinned = await loadJson('browse_pinned', []);
    history = await loadJson('browse_history', []);
  });

  function run(q) {
    goto(`/browse?q=${encodeURIComponent(q)}`);
  }

  function unpin(q) {
    pinned = pinned.filter((p) => p !== q);
    saveJson('browse_pinned', pinned);
  }

  function pin(q) {
    if (!pinned.includes(q)) {
      pinned = [q, ...pinned];
      saveJson('browse_pinned', pinned);
    }
  }

  function forget(q) {
    history = history.filter((h) => h !== q);
    saveJson('browse_history', history);
  }

  function clearHistory() {
    history = [];
    saveJson('browse_history', []);
  }
</script>

<h2>
  <a class="back" href="/browse">←</a>
  Queries
</h2>

<div class="tabs">
  <button class:on={tab === 'saved'} on:click={() => (tab = 'saved')}>
    <Icon name="pin" size={15} /> Saved
  </button>
  <button class:on={tab === 'history'} on:click={() => (tab = 'history')}>
    <Icon name="clock" size={15} /> History
  </button>
</div>

{#if tab === 'saved'}
  {#if !pinned.length}
    <div class="empty">Nothing saved — pin a query from Browse or from History.</div>
  {/if}
  {#each pinned as q (q)}
    <div class="qrow">
      <button class="bare q" on:click={() => run(q)}>{q}</button>
      <button class="bare act" on:click={() => unpin(q)}><Icon name="x" size={16} /></button>
    </div>
  {/each}
{:else}
  {#if !history.length}
    <div class="empty">No search history yet.</div>
  {:else}
    <button class="clear" on:click={clearHistory}>Clear history</button>
  {/if}
  {#each history as q (q)}
    <div class="qrow">
      <button class="bare q" on:click={() => run(q)}>{q}</button>
      {#if !pinned.includes(q)}
        <button class="bare act" title="Save" on:click={() => pin(q)}><Icon name="pin" size={16} /></button>
      {/if}
      <button class="bare act" on:click={() => forget(q)}><Icon name="x" size={16} /></button>
    </div>
  {/each}
{/if}

<style>
  h2 { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; }
  .back { text-decoration: none; color: var(--accent); font-size: 1.2rem; }
  .tabs {
    display: flex; gap: 4px; margin-bottom: 14px;
    background: var(--surface); border: 1px solid var(--line);
    border-radius: 14px; padding: 4px;
  }
  .tabs button {
    flex: 1; background: transparent; color: var(--hint);
    padding: 8px 10px; border-radius: 10px; font-size: 0.88rem;
    display: flex; align-items: center; justify-content: center; gap: 6px;
  }
  .tabs button.on {
    background: var(--accent); color: var(--tg-theme-button-text-color, #fff);
  }
  .empty {
    padding: 24px; text-align: center; color: var(--hint); font-size: 0.9rem;
    border: 2px dashed var(--line); border-radius: 14px; margin-bottom: 10px;
  }
  .qrow {
    display: flex; align-items: center; gap: 8px;
    background: var(--surface); border: 1px solid var(--line);
    border-radius: 12px; padding: 4px 12px; margin-bottom: 8px;
  }
  .bare { background: transparent; padding: 8px 0; color: inherit; font-weight: normal; }
  .q {
    flex: 1; text-align: left; min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    color: var(--accent);
  }
  .act { color: var(--hint); flex-shrink: 0; padding: 8px 4px; }
  .clear {
    width: 100%; margin-bottom: 10px;
    background: transparent; border: 1px solid var(--line); color: var(--hint);
  }
</style>
