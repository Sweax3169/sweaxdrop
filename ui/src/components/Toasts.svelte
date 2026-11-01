<script>
  import { flip } from 'svelte/animate';
  import { app } from '../lib/state.svelte.js';
  import { slide, fade } from '../lib/motion.js';
  import Icon from './Icon.svelte';
</script>

<div class="toasts" aria-live="polite">
  {#each app.toasts as x (x.id)}
    <div class="toast {x.kind}" animate:flip={{ duration: 300 }} in:slide={{ from: 'bottom', distance: 24 }} out:fade={{ duration: 160 }}>
      {#if x.kind === 'ok' || x.kind === 'recv'}<Icon name="check" size={16} stroke={2.4} />{:else if x.kind === 'bad'}<Icon name="x" size={16} stroke={2.4} />{/if}
      <span>{x.message}</span>
    </div>
  {/each}
</div>

<style>
  .toasts { position: fixed; left: 50%; translate: -50% 0; bottom: calc(18px + env(safe-area-inset-bottom)); z-index: 60; display: flex; flex-direction: column; gap: 8px; align-items: center; pointer-events: none; width: min(92vw, 440px); }
  .toast { display: flex; align-items: center; gap: 10px; padding: 10px 14px; border-radius: var(--r-md); background: var(--ink); color: var(--bg); box-shadow: var(--shadow-lift); font-size: 14px; max-width: 100%; }
  .toast span { overflow: hidden; text-overflow: ellipsis; }
  .toast.recv :global(svg), .toast.ok :global(svg) { color: var(--recv); }
  .toast.bad :global(svg) { color: var(--bad); }
  :global(.mobile-nav) ~ .toasts { bottom: calc(84px + env(safe-area-inset-bottom)); }
</style>
