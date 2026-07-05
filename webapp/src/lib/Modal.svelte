<script>
  export let open = false;
  export let title = '';
</script>

{#if open}
  <div class="backdrop" on:click|self={() => (open = false)}>
    <div class="sheet">
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
    display: flex;
    align-items: flex-end;
    justify-content: center;
    z-index: 40;
  }
  .sheet {
    width: 100%;
    max-width: 640px;
    background: var(--tg-theme-bg-color, #17212b);
    border-radius: 18px 18px 0 0;
    padding: 18px 16px calc(18px + env(safe-area-inset-bottom));
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
</style>
