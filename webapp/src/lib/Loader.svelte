<script>
  // The shared loading state: bouncing dots + a quiet label.
  import Lottie from '$lib/Lottie.svelte';
  import loader from '$lib/lottie/loader.json';

  export let label = 'Loading…';
  export let size = 72;
</script>

<div class="ld">
  <Lottie animationData={loader} {size} />
  <span class="muted">{label}</span>
</div>

<style>
  .ld {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0;
    padding: 16px 0;
  }
</style>
