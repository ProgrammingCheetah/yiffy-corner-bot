<script>
  import { paths } from '$lib/icons.js';
  export let name;
  export let size = 20;
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {@html paths[name] ?? ''}
</svg>

<style>
  svg { flex-shrink: 0; vertical-align: -0.18em; }
</style>
