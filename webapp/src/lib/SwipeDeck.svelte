<script>
  // Tinder physics: the top card follows the pointer, tilts, and shows a
  // LIKE/NOPE stamp; past the threshold it flies out and fires the verdict.
  import Media from '$lib/Media.svelte';
  import { createEventDispatcher } from 'svelte';

  export let cards = []; // [{ key, media, title, tags, artists, footer, source }]
  export let rightLabel = 'ACCEPT';
  export let leftLabel = 'REJECT';

  const dispatch = createEventDispatcher();
  let dx = 0, dy = 0, dragging = false, leaving = 0;
  let startX = 0, startY = 0;

  $: top = cards[0];
  $: angle = dx / 18;
  $: verdictOpacity = Math.min(Math.abs(dx) / 90, 1);

  function down(e) {
    if (!top || leaving) return;
    dragging = true;
    startX = e.clientX;
    startY = e.clientY;
  }
  function move(e) {
    if (!dragging) return;
    dx = e.clientX - startX;
    dy = e.clientY - startY;
  }
  function up() {
    if (!dragging) return;
    dragging = false;
    if (dx > 110) fly(1);
    else if (dx < -110) fly(-1);
    else { dx = 0; dy = 0; }
  }
  export function fly(direction) {
    if (!top || leaving) return;
    leaving = direction;
    dx = direction * (window.innerWidth + 200);
    const card = top;
    setTimeout(() => {
      dispatch(direction > 0 ? 'right' : 'left', card);
      leaving = 0; dx = 0; dy = 0;
    }, 240);
  }
</script>

<svelte:window on:pointermove={move} on:pointerup={up} />

<div class="stage">
  {#if !top}
    <div class="empty"><slot name="empty">Nothing left to swipe 🎉</slot></div>
  {:else}
    {#if cards[1]}
      <article class="card under"><Media media={cards[1].media} /></article>
    {/if}
    <article
      class="card"
      class:animate={!dragging}
      style="transform: translate({dx}px, {dy * 0.4}px) rotate({angle}deg)"
      on:pointerdown={down}
    >
      <div class="media"><Media media={top.media} /></div>
      <div class="stamp like" style="opacity: {dx > 0 ? verdictOpacity : 0}">{rightLabel}</div>
      <div class="stamp nope" style="opacity: {dx < 0 ? verdictOpacity : 0}">{leftLabel}</div>
      <div class="meta">
        <div class="title-row">
          <strong>{top.title}</strong>
          {#if top.source}
            <button class="src" on:click|stopPropagation={() =>
              (window.Telegram?.WebApp?.openLink ?? window.open)(top.source)}>
              Source ↗
            </button>
          {/if}
        </div>
        {#if top.artists?.length}
          <div class="muted">by {top.artists.join(', ')}</div>
        {/if}
        {#if top.tags?.length}
          <div class="tags">
            {#each top.tags.slice(0, 14) as tag}<span class="chip">{tag}</span>{/each}
            {#if top.tags.length > 14}<span class="chip">+{top.tags.length - 14}</span>{/if}
          </div>
        {/if}
        {#if top.footer}<div class="muted">{top.footer}</div>{/if}
      </div>
    </article>
  {/if}
</div>

<style>
  .stage {
    position: relative;
    height: min(66dvh, 560px);
    touch-action: none;
    user-select: none;
  }
  .card {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    border-radius: 18px;
    background: var(--tg-theme-secondary-bg-color, #232e3c);
    box-shadow: 0 10px 32px rgba(0, 0, 0, 0.45);
    overflow: hidden;
  }
  .card.animate {
    transition: transform 0.24s ease;
  }
  .card.under {
    transform: scale(0.94) translateY(12px);
    filter: brightness(0.6);
  }
  .media {
    flex: 1;
    min-height: 0;
    padding: 8px;
  }
  .meta {
    padding: 6px 14px 12px;
  }
  .title-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }
  .src {
    padding: 4px 12px;
    font-size: 0.78rem;
    border-radius: 999px;
  }
  .tags {
    margin-top: 4px;
    max-height: 52px;
    overflow: hidden;
  }
  .stamp {
    position: absolute;
    top: 26px;
    font-size: 1.6rem;
    font-weight: 800;
    letter-spacing: 2px;
    padding: 4px 14px;
    border: 4px solid;
    border-radius: 10px;
    transform: rotate(-14deg);
    pointer-events: none;
  }
  .stamp.like {
    left: 18px;
    color: #4ade80;
    border-color: #4ade80;
  }
  .stamp.nope {
    right: 18px;
    color: #f87171;
    border-color: #f87171;
    transform: rotate(14deg);
  }
  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--tg-theme-hint-color, #7d8b99);
  }
</style>
