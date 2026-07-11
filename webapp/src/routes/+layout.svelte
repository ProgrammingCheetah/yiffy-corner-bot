<script>
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { get } from '$lib/api.js';
  import { loadJson, saveJson } from '$lib/store.js';
  import { currentVersion } from '$lib/changelog.js';

  let me = null;
  let error = '';
  let showNews = false;

  onMount(async () => {
    window.Telegram?.WebApp?.ready();
    window.Telegram?.WebApp?.expand();
    // Announce the current version until THIS user dismisses THIS version
    // (CloudStorage — follows the user across devices).
    const dismissed = await loadJson('changelog_dismissed', null);
    showNews = dismissed !== currentVersion;
    try {
      me = await get('/me');
    } catch (e) {
      error = e.message;
    }
  });

  function dismissNews() {
    showNews = false;
    saveJson('changelog_dismissed', currentVersion);
  }

  // The API sends roles in their storage form: "user" | "moderator" | "owner".
  $: role = (me?.role ?? 'user').toLowerCase();
  $: tabs = [
    { href: '/', label: 'Submit', icon: '➕' },
    ...(role === 'moderator' || role === 'owner'
      ? [
          { href: '/review', label: 'Review', icon: '🔥' },
          { href: '/browse', label: 'Browse', icon: '🔎' },
          { href: '/feed', label: 'Feed', icon: '📜' },
          { href: '/reports', label: 'Reports', icon: '⚠️' }
        ]
      : []),
    ...(role === 'owner' ? [{ href: '/admin', label: 'Admin', icon: '⚙️' }] : []),
    { href: '/changelog', label: 'News', icon: '✨' }
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
    {#if showNews}
      <div class="news">
        <button class="news-body" on:click={() => { dismissNews(); goto('/changelog'); }}>
          ✨ Yiffy Corner v{currentVersion} — tap to see what's new
        </button>
        <button class="news-x" on:click={dismissNews}>✕</button>
      </div>
    {/if}
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
  :global(:root) {
    --accent: var(--tg-theme-button-color, #5288c1);
    --surface: var(--tg-theme-secondary-bg-color, #232e3c);
    --hint: var(--tg-theme-hint-color, #7d8b99);
    --line: rgba(128, 128, 128, 0.22);
  }
  :global(body) {
    font-family: -apple-system, system-ui, Roboto, sans-serif;
    background: var(--tg-theme-bg-color, #17212b);
    color: var(--tg-theme-text-color, #f5f5f5);
    overscroll-behavior: none;
    -webkit-tap-highlight-color: transparent;
  }
  :global(h2) {
    font-size: 1.35rem;
    letter-spacing: -0.02em;
  }
  :global(button) {
    font: inherit;
    font-weight: 600;
    border: none;
    border-radius: 12px;
    cursor: pointer;
    background: var(--accent);
    color: var(--tg-theme-button-text-color, #fff);
    padding: 10px 16px;
    transition: transform 0.12s ease, filter 0.12s ease, opacity 0.12s ease;
  }
  :global(button:active) {
    transform: scale(0.96);
    filter: brightness(1.08);
  }
  :global(button:disabled) {
    opacity: 0.55;
    cursor: default;
  }
  :global(input),
  :global(textarea) {
    font: inherit;
    width: 100%;
    padding: 10px 12px;
    border-radius: 12px;
    border: 1px solid var(--line);
    background: var(--surface);
    color: inherit;
    transition: border-color 0.15s ease, box-shadow 0.15s ease;
  }
  :global(input:focus),
  :global(textarea:focus) {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
  }
  :global(.muted) {
    color: var(--hint);
    font-size: 0.85rem;
  }
  :global(.chip) {
    display: inline-block;
    padding: 2px 9px;
    margin: 2px;
    border-radius: 999px;
    background: var(--surface);
    border: 1px solid var(--line);
    font-size: 0.78rem;
  }
  /* Shared by review/browse: the action row under a swipe deck. */
  :global(.actions) {
    display: flex;
    justify-content: center;
    gap: 18px;
    margin-top: 16px;
  }
  :global(button.round) {
    width: 58px;
    height: 58px;
    border-radius: 50%;
    font-size: 1.3rem;
    background: var(--surface);
    border: 1px solid var(--line);
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.3);
  }
  :global(button.round:active) {
    transform: scale(0.88);
  }
  /* Shared toast: every page pops the same pill. */
  :global(.toast) {
    position: fixed;
    bottom: 96px;
    left: 50%;
    transform: translateX(-50%);
    background: rgba(0, 0, 0, 0.85);
    color: #fff;
    padding: 10px 18px;
    border-radius: 999px;
    font-size: 0.85rem;
    max-width: 90vw;
    z-index: 50;
    box-shadow: 0 6px 22px rgba(0, 0, 0, 0.4);
    animation: toast-in 0.22s cubic-bezier(0.2, 0.9, 0.3, 1.2);
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translate(-50%, 12px);
    }
  }
  .shell {
    display: flex;
    flex-direction: column;
    min-height: 100dvh;
  }
  .news {
    display: flex;
    align-items: stretch;
    gap: 4px;
    max-width: 640px;
    width: calc(100% - 28px);
    margin: 10px auto 0;
    border-radius: 14px;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    animation: page-in 0.25s ease;
  }
  .news button {
    background: transparent;
    color: var(--accent);
    font-size: 0.85rem;
    padding: 10px 12px;
  }
  .news-body {
    flex: 1;
    text-align: left;
  }
  .news button.news-x {
    color: var(--hint);
  }
  main {
    flex: 1;
    padding: 16px 14px 104px;
    max-width: 640px;
    width: 100%;
    margin: 0 auto;
    animation: page-in 0.25s ease;
  }
  @keyframes page-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
  nav {
    position: fixed;
    bottom: calc(10px + env(safe-area-inset-bottom));
    left: 50%;
    transform: translateX(-50%);
    width: min(calc(100% - 24px), 480px);
    display: flex;
    justify-content: space-around;
    padding: 6px;
    border-radius: 22px;
    background: var(--surface);
    background: color-mix(in srgb, var(--surface) 78%, transparent);
    backdrop-filter: blur(14px);
    -webkit-backdrop-filter: blur(14px);
    border: 1px solid var(--line);
    box-shadow: 0 8px 28px rgba(0, 0, 0, 0.35);
  }
  nav a {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    text-decoration: none;
    color: var(--hint);
    font-size: 0.72rem;
    font-weight: 600;
    padding: 6px 14px;
    border-radius: 16px;
    transition: color 0.15s ease, background 0.15s ease;
  }
  nav a.active {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 16%, transparent);
  }
  .icon {
    font-size: 1.25rem;
    transition: transform 0.15s ease;
  }
  nav a.active .icon {
    transform: translateY(-1px) scale(1.08);
  }
  .boot-error {
    padding: 40px 20px;
    text-align: center;
  }
  .boot-error .hint {
    margin-top: 12px;
    font-size: 0.8rem;
    color: var(--hint);
  }
</style>
