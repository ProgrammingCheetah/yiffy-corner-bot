<script>
  // A tag-aware text input: the token under the caret is completed against
  // e621 (through our rate-limited proxy), suggestions drop down under the
  // field, and picking one replaces just that token. Rule/DSL punctuation
  // and negation prefixes are respected.
  import { get } from '$lib/api.js';

  export let value = '';
  export let placeholder = '';

  let input;
  let suggestions = [];
  let open = false;
  let timer;

  function currentToken() {
    const pos = input?.selectionStart ?? value.length;
    const before = value.slice(0, pos);
    const start =
      Math.max(before.lastIndexOf(' '), before.lastIndexOf('('), before.lastIndexOf('[')) + 1;
    let end = pos;
    while (end < value.length && !' )]'.includes(value[end])) end += 1;
    return { start, end, token: value.slice(start, pos) };
  }

  function onInput() {
    clearTimeout(timer);
    const { token } = currentToken();
    const bare = token.replace(/^-/, '');
    // Skip too-short prefixes and metatag/DSL tokens (order:, rating:, ]->[…).
    if (bare.length < 2 || /[:\]>]/.test(bare)) {
      open = false;
      return;
    }
    timer = setTimeout(async () => {
      try {
        const res = await get(`/tags/complete?q=${encodeURIComponent(bare)}`);
        suggestions = res.tags;
        open = suggestions.length > 0;
      } catch {
        open = false;
      }
    }, 250);
  }

  function pick(name) {
    const { start, end, token } = currentToken();
    const neg = token.startsWith('-') ? '-' : '';
    const caret = start + neg.length + name.length;
    value = value.slice(0, start) + neg + name + value.slice(end);
    open = false;
    suggestions = [];
    queueMicrotask(() => {
      input?.focus();
      input?.setSelectionRange(caret, caret);
    });
  }

  function count(n) {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${Math.round(n / 1_000)}k`;
    return `${n}`;
  }
</script>

<div class="wrap">
  <input
    bind:this={input}
    bind:value
    {placeholder}
    autocapitalize="off"
    autocorrect="off"
    spellcheck="false"
    on:input={onInput}
    on:blur={() => setTimeout(() => (open = false), 150)}
    on:change
    on:keydown
  />
  {#if open}
    <div class="drop">
      {#each suggestions as s (s.name)}
        <button type="button" class="opt" on:mousedown|preventDefault={() => pick(s.name)}>
          <span class="tag" data-category={s.category}>{s.name}</span>
          <span class="n">{count(s.post_count)}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .wrap { position: relative; flex: 1; min-width: 0; }
  .drop {
    position: absolute; top: calc(100% + 4px); left: 0; right: 0; z-index: 30;
    background: var(--tg-theme-bg-color, #17212b);
    border: 1px solid var(--line); border-radius: 12px;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.4);
    overflow: hidden;
    max-height: 240px; overflow-y: auto;
  }
  .opt {
    display: flex; justify-content: space-between; align-items: center; gap: 8px;
    width: 100%; background: transparent; color: inherit;
    padding: 9px 12px; border-radius: 0; font-weight: normal; font-size: 0.9rem;
    text-align: left;
  }
  .opt:active { background: color-mix(in srgb, var(--accent) 16%, transparent); }
  .tag { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tag[data-category="1"] { color: #fbbf24; } /* artist */
  .tag[data-category="4"] { color: #4ade80; } /* character */
  .tag[data-category="5"] { color: #93c5fd; } /* species */
  .n { color: var(--hint); font-size: 0.78rem; flex-shrink: 0; }
</style>
