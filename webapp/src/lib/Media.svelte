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
  }
  .shimmer {
    animation: shimmer 1.2s infinite;
    background: linear-gradient(100deg, #1b2735 40%, #24344a 50%, #1b2735 60%);
    background-size: 200% 100%;
  }
  @keyframes shimmer {
    to {
      background-position-x: -200%;
    }
  }
</style>
