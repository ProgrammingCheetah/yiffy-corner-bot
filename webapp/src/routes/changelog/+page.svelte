<script>
  import { changelog, currentVersion } from '$lib/changelog.js';
</script>

<h2>Changelog</h2>

{#each changelog as entry (entry.version)}
  <div class="release" class:current={entry.version === currentVersion}>
    <div class="head">
      <strong>v{entry.version}</strong>
      {#if entry.version === currentVersion}<span class="chip now">current</span>{/if}
      <span class="muted">{entry.date}</span>
    </div>
    <ul>
      {#each entry.changes as change}
        <li>{change}</li>
      {/each}
    </ul>
  </div>
{/each}

<style>
  h2 { margin-bottom: 12px; }
  .release {
    background: var(--surface); border: 1px solid var(--line);
    border-radius: 16px; padding: 14px; margin-bottom: 12px;
  }
  .release.current { border-color: color-mix(in srgb, var(--accent) 45%, transparent); }
  .head { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
  .chip.now {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  ul { margin: 0; padding-left: 20px; display: flex; flex-direction: column; gap: 6px; }
  li { font-size: 0.9rem; line-height: 1.45; }
</style>
