<script>
  // Modal surface: a centered card on wide screens, a bottom sheet on phones.
  import { app } from '../lib/state.svelte.js';
  import { slide, rise, fade } from '../lib/motion.js';

  let { onclose, width = 460, children, label = '' } = $props();

  function key(e) {
    if (e.key === 'Escape') onclose?.();
  }
</script>

<svelte:window onkeydown={key} />

<div class="scrim" transition:fade={{ duration: 200 }} onclick={onclose} role="presentation"></div>
{#if app.wide}
  <div class="sheet wide" style:width="{width}px" role="dialog" aria-modal="true" aria-label={label} in:rise={{ y: 18, scale: 0.96 }} out:fade={{ duration: 140 }}>
    {@render children()}
  </div>
{:else}
  <div class="sheet narrow" role="dialog" aria-modal="true" aria-label={label} in:slide={{ from: 'bottom', distance: 120 }} out:slide={{ from: 'bottom', distance: 80 }}>
    <div class="grip"></div>
    {@render children()}
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; background: color-mix(in srgb, var(--floor) 55%, transparent); backdrop-filter: blur(3px); z-index: 40; }
  .sheet {
    position: fixed; z-index: 41; background: var(--s1); box-shadow: var(--shadow-lift);
    max-height: calc(100% - 48px); overflow-y: auto; scrollbar-width: thin;
  }
  .wide { left: 50%; top: 50%; translate: -50% -50%; border-radius: var(--r-xl); padding: 24px; max-width: calc(100% - 48px); }
  .narrow {
    left: 0; right: 0; bottom: 0; border-radius: var(--r-xl) var(--r-xl) 0 0;
    padding: 10px 18px calc(18px + var(--safe-bottom));
    max-height: calc(100% - 40px);
  }
  .grip { width: 38px; height: 4px; border-radius: 4px; background: var(--s4); margin: 0 auto 14px; }
</style>
