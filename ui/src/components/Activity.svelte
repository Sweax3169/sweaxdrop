<script>
  import { flip } from 'svelte/animate';
  import { app, activeTransfers, getBridge } from '../lib/state.svelte.js';
  import { t } from '../lib/i18n.js';
  import { ago } from '../lib/format.js';
  import { rise, fade } from '../lib/motion.js';
  import TransferCard from './TransferCard.svelte';
  import Icon from './Icon.svelte';

  let { compact = false } = $props();
  const list = $derived(activeTransfers());
  let copied = $state(null);

  async function copy(x) {
    await getBridge().clipboard.write(x.text);
    copied = x.id;
    setTimeout(() => (copied = null), 1600);
  }
  const isUrl = (s) => /^https?:\/\/\S+$/.test(s.trim());
</script>

<div class="activity">
  {#each app.texts.slice(0, 3) as x (x.id)}
    <div class="text" animate:flip={{ duration: 400 }} in:rise>
      <div class="from"><Icon name="text" size={15} /> {t('From {name}', { name: x.from?.name || '' })} <span class="faint">· {ago(x.at)}</span></div>
      <p>{x.text}</p>
      <div class="row">
        {#if isUrl(x.text)}<button class="btn small" onclick={() => getBridge().openUrl(x.text.trim())}><Icon name="open" size={15} />{t('Open')}</button>{/if}
        <button class="btn small recv" onclick={() => copy(x)}><Icon name={copied === x.id ? 'check' : 'clip'} size={15} />{copied === x.id ? t('Copied') : t('Copy')}</button>
        <button class="btn ghost small icon" aria-label={t('Close')} onclick={() => (app.texts = app.texts.filter((y) => y !== x))}><Icon name="x" size={15} /></button>
      </div>
    </div>
  {/each}

  {#each list as tr (tr.id)}
    <div animate:flip={{ duration: 420 }} in:rise={{ y: 18 }} out:fade={{ duration: 160 }}>
      <TransferCard {tr} {compact} />
    </div>
  {:else}
    {#if !app.texts.length}
      <div class="empty" in:fade>
        <div class="lines" aria-hidden="true"><span></span><span></span><span></span></div>
        <b>{t('Nothing moving right now.')}</b>
        <span class="faint">{t('Transfers appear here with their speed, lane by lane.')}</span>
      </div>
    {/if}
  {/each}
</div>

<style>
  .activity { display: flex; flex-direction: column; gap: 10px; }
  .empty { display: flex; flex-direction: column; align-items: center; text-align: center; gap: 4px; padding: 30px 16px; border-radius: var(--r-lg); background: var(--s1); box-shadow: var(--shadow); font-size: 14px; }
  .lines { display: flex; flex-direction: column; gap: 5px; width: 120px; margin-bottom: 12px; }
  .lines span { height: 6px; border-radius: 3px; background: var(--s3); position: relative; overflow: hidden; }
  .lines span::after { content: ''; position: absolute; inset: 0; width: 24px; border-radius: 3px; background: var(--s4); animation: run 2.6s var(--ease-out) infinite; }
  .lines span:nth-child(2)::after { animation-delay: 0.3s; }
  .lines span:nth-child(3)::after { animation-delay: 0.6s; }
  @keyframes run { from { transform: translateX(-30px); } to { transform: translateX(130px); } }
  .text { padding: 12px 14px; border-radius: var(--r-lg); background: var(--s1); box-shadow: var(--shadow); box-shadow: inset 3px 0 0 var(--recv), var(--shadow); }
  .from { display: flex; align-items: center; gap: 6px; font-size: 12.5px; color: var(--recv); font-weight: 600; }
  .from .faint { font-weight: 400; }
  .text p { margin: 8px 0 10px; white-space: pre-wrap; word-break: break-word; max-height: 140px; overflow: auto; }
  .row { display: flex; gap: 6px; justify-content: flex-end; }
</style>
