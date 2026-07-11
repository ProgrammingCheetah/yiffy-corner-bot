<script>
  // Home: the front door. The bottom nav keeps only the daily drivers;
  // everything else lives here as role-gated section cards.
  import Icon from '$lib/Icon.svelte';
  import { get } from '$lib/api.js';
  import { changelog, releaseName } from '$lib/changelog.js';
  import { onMount } from 'svelte';

  let me = null;
  onMount(async () => {
    try { me = await get('/me'); } catch { /* layout shows the boot error */ }
  });

  $: role = (me?.role ?? 'user').toLowerCase();
  $: cards = [
    { href: '/submit', icon: 'upload', title: 'Submit', blurb: 'Suggest art for the channels' },
    ...(role === 'moderator' || role === 'owner'
      ? [
          { href: '/review', icon: 'flame', title: 'Review', blurb: 'The moderation deck' },
          { href: '/browse', icon: 'search', title: 'Browse', blurb: 'Curate straight from e621' },
          { href: '/feed', icon: 'scroll', title: 'Feed', blurb: 'Poster cursors & queues' },
          { href: '/reports', icon: 'alert', title: 'Reports', blurb: 'Open reports, who & why' }
        ]
      : []),
    ...(role === 'owner'
      ? [{ href: '/admin', icon: 'settings', title: 'Admin', blurb: 'Posters, tags, users' }]
      : []),
    { href: '/changelog', icon: 'sparkles', title: "What's new", blurb: releaseName(changelog[0]) }
  ];
</script>

<h2>Yiffy Corner</h2>
<p class="muted">
  {#if me}Hey {me.name ?? 'there'} — {role}.{:else}Loading…{/if}
</p>

<div class="grid">
  {#each cards as card (card.href)}
    <a class="tile" href={card.href}>
      <span class="ic"><Icon name={card.icon} size={22} /></span>
      <strong>{card.title}</strong>
      <span class="muted blurb">{card.blurb}</span>
    </a>
  {/each}
</div>

<style>
  h2 { margin-bottom: 2px; }
  p { margin-bottom: 16px; }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .tile {
    display: flex; flex-direction: column; gap: 4px;
    background: var(--surface); border: 1px solid var(--line);
    border-radius: 16px; padding: 14px;
    text-decoration: none; color: inherit;
    box-shadow: 0 3px 14px rgba(0, 0, 0, 0.18);
    transition: transform 0.12s ease, border-color 0.15s ease;
  }
  .tile:active {
    transform: scale(0.97);
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .ic {
    color: var(--accent);
    width: 38px; height: 38px; display: flex; align-items: center; justify-content: center;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    border-radius: 12px; margin-bottom: 4px;
  }
  .blurb { font-size: 0.78rem; line-height: 1.35; }
</style>
