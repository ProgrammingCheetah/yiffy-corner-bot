<script>
  export let media = null; // { kind, url }
</script>

{#if !media}
  <div class="ph shimmer"></div>
{:else if media.kind === 'photo'}
  <img src={media.url} alt="" />
{:else if media.kind === 'video'}
  <!-- svelte-ignore a11y_media_has_caption -->
  <video src={media.url} autoplay muted loop playsinline controls></video>
{:else if media.kind === 'animation'}
  <img src={media.url} alt="" />
{:else}
  <div class="ph">
    <span>🔗 media lives at the source link</span>
  </div>
{/if}

<style>
  img,
  video,
  .ph {
    width: 100%;
    height: 100%;
    object-fit: contain;
    border-radius: 16px;
    background: #000;
  }
  .ph {
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--tg-theme-hint-color, #7d8b99);
    font-size: 0.85rem;
    background: color-mix(in srgb, var(--surface, #232e3c) 60%, black);
    border: 2px dashed var(--line, rgba(128, 128, 128, 0.22));
  }
  .shimmer {
    border: none;
    animation: shimmer 1.2s infinite;
    background: linear-gradient(
      100deg,
      color-mix(in srgb, var(--surface, #1b2735) 88%, black) 40%,
      color-mix(in srgb, var(--surface, #24344a) 82%, white) 50%,
      color-mix(in srgb, var(--surface, #1b2735) 88%, black) 60%
    );
    background-size: 200% 100%;
  }
  @keyframes shimmer {
    to {
      background-position-x: -200%;
    }
  }
</style>
