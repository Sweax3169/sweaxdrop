<script>
  // Small device drawings: the screen lights up in the direction color while
  // something moves, and fills like a gauge with the progress.
  let { platform = '', model = '', name = '', kind = 'app', size = 56, glow = 0, progress = null, dir = 'send' } = $props();

  const shape = $derived.by(() => {
    const n = (model + ' ' + name).toLowerCase();
    if (kind === 'web') return 'browser';
    if (platform === 'android' || platform === 'ios') return /tab|ipad|pad\b/.test(n) ? 'tablet' : 'phone';
    if (/laptop|book|notebook/.test(n)) return 'laptop';
    return 'desktop';
  });
  const c = $derived(dir === 'send' ? 'var(--send)' : 'var(--recv)');
</script>

<svg width={size} height={size} viewBox="0 0 56 56" aria-hidden="true" style:--c={c} style:--g={glow}>
  {#if shape === 'phone' || shape === 'browser'}
    <rect x="16" y="5" width="24" height="46" rx="6" class="body" />
    <rect x="18.5" y="9" width="19" height="36" rx="3" class="screen" />
    {#if progress !== null}<rect x="18.5" y={9 + 36 * (1 - progress)} width="19" height={36 * progress} rx="3" class="fill" />{/if}
    <rect x="25" y="47" width="6" height="1.6" rx="0.8" class="detail" />
    {#if shape === 'browser'}
      <circle cx="28" cy="27" r="6" class="detail-line" />
      <path d="M22 27h12M28 21c2 2 2 10 0 12M28 21c-2 2-2 10 0 12" class="detail-line" />
    {/if}
  {:else if shape === 'tablet'}
    <rect x="9" y="8" width="38" height="42" rx="6" class="body" />
    <rect x="12" y="11.5" width="32" height="35" rx="3" class="screen" />
    {#if progress !== null}<rect x="12" y={11.5 + 35 * (1 - progress)} width="32" height={35 * progress} rx="3" class="fill" />{/if}
  {:else if shape === 'laptop'}
    <rect x="9" y="12" width="38" height="26" rx="3.5" class="body" />
    <rect x="11.5" y="14.5" width="33" height="21" rx="2" class="screen" />
    {#if progress !== null}<rect x="11.5" y={14.5 + 21 * (1 - progress)} width="33" height={21 * progress} rx="2" class="fill" />{/if}
    <path d="M4 41h48l-2.5 3.5a2 2 0 0 1-1.6.8H8.1a2 2 0 0 1-1.6-.8z" class="body" />
  {:else}
    <rect x="6" y="9" width="44" height="30" rx="3.5" class="body" />
    <rect x="8.5" y="11.5" width="39" height="25" rx="2" class="screen" />
    {#if progress !== null}<rect x="8.5" y={11.5 + 25 * (1 - progress)} width="39" height={25 * progress} rx="2" class="fill" />{/if}
    <path d="M24 39h8l1.5 7h-11z" class="body" />
    <rect x="18" y="46" width="20" height="2.6" rx="1.3" class="body" />
  {/if}
</svg>

<style>
  svg { display: block; overflow: visible; }
  .body { fill: var(--ink); }
  .screen { fill: color-mix(in srgb, var(--c) calc(var(--g) * 70%), var(--s2)); transition: fill 0.5s var(--ease-out); }
  .fill { fill: var(--c); opacity: 0.9; transition: y 0.35s var(--ease-out), height 0.35s var(--ease-out); }
  .detail { fill: var(--s2); opacity: 0.5; }
  .detail-line { fill: none; stroke: var(--ink-3); stroke-width: 1.4; }
</style>
