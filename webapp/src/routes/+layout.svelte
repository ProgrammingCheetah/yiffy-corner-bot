<script>
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { get } from '$lib/api.js';

  let me = null;
  let error = '';

  onMount(async () => {
    window.Telegram?.WebApp?.ready();
    window.Telegram?.WebApp?.expand();
    try {
      me = await get('/me');
    } catch (e) {
      error = e.message;
    }
  });

  // The API sends roles in their storage form: "user" | "moderator" | "owner".
  $: role = (me?.role ?? 'user').toLowerCase();
  $: tabs = [
    { href: '/', label: 'Submit', icon: '➕' },
    ...(role === 'moderator' || role === 'owner'
      ? [
          { href: '/review', label: 'Review', icon: '🔥' },
          { href: '/browse', label: 'Browse', icon: '🔎' }
        ]
      : []),
    ...(role === 'owner' ? [{ href: '/admin', label: 'Admin', icon: '⚙️' }] : [])
  ];
</script>

<div class="shell">
  {#if error}
    <div class="boot-error">
      <h2>Can't sign you in</h2>
      <p>{error}</p>
      <p class="hint">
        Opened outside Telegram? Get a token with /apitoken and store it:
        <code>localStorage.setItem('ycb_token', '…')</code>
      </p>
    </div>
  {:else}
    <main><slot /></main>
    <nav>
      {#each tabs as tab}
        <a href={tab.href} class:active={$page.url.pathname === tab.href}>
          <span class="icon">{tab.icon}</span>
          <span>{tab.label}</span>
        </a>
      {/each}
    </nav>
  {/if}
</div>

<style>
  :global(*) {
    box-sizing: border-box;
    margin: 0;
  }
  :global(body) {
    font-family: -apple-system, system-ui, Roboto, sans-serif;
    background: var(--tg-theme-bg-color, #17212b);
    color: var(--tg-theme-text-color, #f5f5f5);
    overscroll-behavior: none;
  }
  :global(button) {
    font: inherit;
    border: none;
    border-radius: 12px;
    cursor: pointer;
    background: var(--tg-theme-button-color, #5288c1);
    color: var(--tg-theme-button-text-color, #fff);
    padding: 10px 16px;
  }
  :global(input),
  :global(textarea) {
    font: inherit;
    width: 100%;
    padding: 10px 12px;
    border-radius: 12px;
    border: 1px solid rgba(128, 128, 128, 0.35);
    background: var(--tg-theme-secondary-bg-color, #232e3c);
    color: inherit;
  }
  :global(.muted) {
    color: var(--tg-theme-hint-color, #7d8b99);
    font-size: 0.85rem;
  }
  :global(.chip) {
    display: inline-block;
    padding: 2px 9px;
    margin: 2px;
    border-radius: 999px;
    background: var(--tg-theme-secondary-bg-color, #232e3c);
    font-size: 0.78rem;
  }
  .shell {
    display: flex;
    flex-direction: column;
    min-height: 100dvh;
  }
  main {
    flex: 1;
    padding: 14px 14px 80px;
    max-width: 640px;
    width: 100%;
    margin: 0 auto;
  }
  nav {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    display: flex;
    justify-content: space-around;
    padding: 6px 4px calc(6px + env(safe-area-inset-bottom));
    background: var(--tg-theme-secondary-bg-color, #232e3c);
    border-top: 1px solid rgba(128, 128, 128, 0.2);
  }
  nav a {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    text-decoration: none;
    color: var(--tg-theme-hint-color, #7d8b99);
    font-size: 0.72rem;
    padding: 4px 12px;
    border-radius: 10px;
  }
  nav a.active {
    color: var(--tg-theme-button-color, #5288c1);
  }
  .icon {
    font-size: 1.25rem;
  }
  .boot-error {
    padding: 40px 20px;
    text-align: center;
  }
  .boot-error .hint {
    margin-top: 12px;
    font-size: 0.8rem;
    color: var(--tg-theme-hint-color, #7d8b99);
  }
</style>
