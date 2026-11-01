<script>
  import { call } from '../lib/state.svelte.js';
  import { t } from '../lib/i18n.js';
  import { rise, fade } from '../lib/motion.js';
  import Icon from './Icon.svelte';

  let { peer, anchor, onclose, onpair } = $props();
  const r = anchor.getBoundingClientRect();
  const left = Math.min(window.innerWidth - 236, Math.max(8, r.right - 220));
  const top = Math.min(window.innerHeight - 200, r.bottom + 6);

  async function run(cmd) {
    onclose();
    await call(cmd, { peer: peer.id });
  }
</script>

<div class="scrim" role="presentation" onclick={onclose} transition:fade={{ duration: 120 }}></div>
<div class="menu" role="menu" style:left="{left}px" style:top="{top}px" in:rise={{ y: -6, scale: 0.95 }} out:fade={{ duration: 100 }}>
  <div class="h">{peer.name}</div>
  {#if !peer.trusted && peer.online}
    <button role="menuitem" onclick={() => { onclose(); onpair(peer); }}><Icon name="key" size={16} />{t('Pair with code')}</button>
  {/if}
  {#if peer.trusted}
    <button role="menuitem" onclick={() => run('untrust')}><Icon name="shield" size={16} />{t('Untrust')}</button>
  {/if}
  <button role="menuitem" class="bad" onclick={() => run('forget')}><Icon name="trash" size={16} />{t('Forget')}</button>
</div>

<svelte:window onkeydown={(e) => e.key === 'Escape' && onclose()} />

<style>
  .scrim { position: fixed; inset: 0; z-index: 30; }
  .menu { position: fixed; z-index: 31; width: 220px; padding: 6px; background: var(--s1); border-radius: var(--r-md); box-shadow: var(--shadow-lift); transform-origin: top right; }
  .h { font-size: 12px; color: var(--ink-3); padding: 6px 10px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  button { display: flex; align-items: center; gap: 10px; width: 100%; padding: 9px 10px; border-radius: 9px; font-size: 14px; text-align: left; }
  button:hover { background: var(--s3); }
  .bad { color: var(--bad); }
</style>
