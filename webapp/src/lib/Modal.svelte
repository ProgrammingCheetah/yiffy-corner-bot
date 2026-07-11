<script>
  export let open = false;
  export let title = '';
</script>

{#if open}
  <div class="backdrop" on:click|self={() => (open = false)}>
    <div class="sheet">
      <div class="grab"></div>
      <h3>{title}</h3>
      <slot />
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(4px);
    -webkit-backdrop-filter: blur(4px);
    display: flex;
    align-items: flex-end;
    justify-content: center;
    z-index: 40;
    animation: fade-in 0.18s ease;
  }
  .sheet {
    width: 100%;
    max-width: 640px;
    background: var(--tg-theme-bg-color, #17212b);
    border-radius: 22px 22px 0 0;
    border-top: 1px solid var(--line, rgba(128, 128, 128, 0.22));
    padding: 10px 16px calc(18px + env(safe-area-inset-bottom));
    display: flex;
    flex-direction: column;
    gap: 12px;
    animation: rise 0.24s cubic-bezier(0.2, 0.9, 0.3, 1.05);
  }
  .grab {
    width: 40px;
    height: 4px;
    border-radius: 999px;
    background: var(--line, rgba(128, 128, 128, 0.35));
    margin: 0 auto 4px;
  }
  @keyframes fade-in {
    from {
      opacity: 0;
    }
  }
  @keyframes rise {
    from {
      transform: translateY(40px);
      opacity: 0.6;
    }
  }
</style>
