<script>
  // Thin wrapper over lottie-web's LIGHT player (SVG renderer, no eval),
  // dynamically imported so it stays out of the entry chunk. Animations
  // are bundled JSON — no CDN.
  import { onMount, onDestroy } from 'svelte';

  export let animationData;
  export let loop = true;
  export let size = 80;

  let el;
  let anim;

  onMount(async () => {
    const lottie = (await import('lottie-web/build/player/lottie_light')).default;
    anim = lottie.loadAnimation({
      container: el,
      renderer: 'svg',
      loop,
      autoplay: true,
      animationData
    });
  });

  onDestroy(() => anim?.destroy());
</script>

<div bind:this={el} style="width:{size}px;height:{size}px;"></div>
