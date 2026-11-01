<script>
  import { app } from '../lib/state.svelte.js';
  import { t } from '../lib/i18n.js';
  import { rise } from '../lib/motion.js';
  import DeviceTile from './DeviceTile.svelte';
  import QR from './QR.svelte';
  import Icon from './Icon.svelte';

  let { oncompose, onmenu, onpair } = $props();

  const peers = $derived(app.peers.filter((p) => p.online || p.trusted));
  const online = $derived(peers.filter((p) => p.online).length);
</script>

<section class="nearby">
  <div class="title">
    <h2>{t('Nearby')}</h2>
    {#if online > 0}<span class="count num">{online}</span>{/if}
    <span class="scan" class:on={online === 0}><span></span><span></span><span></span></span>
    <span class="hint faint">{app.staged.length ? (app.wide ? t('Pick a device to send to') : t('Tap a device to send')) : ''}</span>
  </div>

  {#if peers.length}
    <div class="grid">
      {#each peers as p, i (p.id)}
        <div in:rise={{ delay: i * 45 }}>
          <DeviceTile peer={p} {oncompose} {onmenu} />
        </div>
      {/each}
    </div>
  {/if}

  {#if online === 0 && app.kind !== 'web'}
    <div class="empty" in:rise>
      <div class="radar" aria-hidden="true"><span></span><span></span><span></span><i></i></div>
      <div class="copy">
        <b>{t('Looking for devices on this network…')}</b>
        <p class="muted">{t('Open SweaxDrop on your phone or another computer on the same Wi-Fi. They show up here by themselves.')}</p>
        {#if app.qr && app.wide}
          <div class="qrhint">
            <div class="qr"><QR text={app.qr} size={112} /></div>
            <div>
              <b>{t('No app on the phone?')}</b>
              <p class="muted small">{t('Scan the code with the camera. The phone opens SweaxDrop in its browser, nothing to install.')}</p>
            </div>
          </div>
        {:else}
          <button class="btn" onclick={onpair}><Icon name="qr" size={18} /> {t('Show code')}</button>
        {/if}
      </div>
    </div>
  {/if}
</section>

<style>
  .nearby { display: flex; flex-direction: column; gap: 14px; }
  .title { display: flex; align-items: center; gap: 10px; min-height: 28px; }
  h2 { margin: 0; font-size: 18px; font-weight: 700; letter-spacing: -0.02em; }
  .count { font-size: 12px; padding: 2px 8px; border-radius: 8px; background: var(--s3); color: var(--ink-2); }
  .hint { margin-left: auto; font-size: 13px; }
  .scan { display: inline-flex; gap: 3px; opacity: 0; transition: opacity 0.4s; }
  .scan.on { opacity: 1; }
  .scan span { width: 4px; height: 4px; border-radius: 50%; background: var(--ink-3); animation: blink 1.4s infinite; }
  .scan span:nth-child(2) { animation-delay: 0.2s; }
  .scan span:nth-child(3) { animation-delay: 0.4s; }
  @keyframes blink { 0%, 100% { opacity: 0.25; } 40% { opacity: 1; } }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 12px; }
  @media (max-width: 520px) { .grid { grid-template-columns: repeat(2, 1fr); gap: 10px; } }

  .empty { display: flex; gap: 22px; align-items: flex-start; padding: 22px; border-radius: var(--r-lg); background: var(--s1); box-shadow: var(--shadow); }
  .copy { flex: 1; min-width: 0; }
  .copy p { margin: 6px 0 14px; font-size: 14px; max-width: 46ch; }
  .small { font-size: 13px; margin: 4px 0 0; }
  .qrhint { display: flex; gap: 14px; align-items: center; padding: 10px; border-radius: var(--r-md); background: var(--s2); }
  .qr { padding: 6px; background: #fff; border-radius: 14px; flex: none; }
  .radar { position: relative; width: 84px; height: 84px; flex: none; }
  .radar span { position: absolute; inset: 0; border-radius: 50%; box-shadow: 0 0 0 2px color-mix(in srgb, var(--send) 50%, transparent); animation: wave 3s var(--ease-out) infinite; opacity: 0; }
  .radar span:nth-child(2) { animation-delay: 1s; }
  .radar span:nth-child(3) { animation-delay: 2s; }
  .radar i { position: absolute; left: 50%; top: 50%; width: 14px; height: 14px; margin: -7px; border-radius: 50%; background: var(--send); }
  @keyframes wave { 0% { transform: scale(0.2); opacity: 0.9; } 100% { transform: scale(1); opacity: 0; } }
  @media (max-width: 520px) { .empty { flex-direction: column; align-items: center; text-align: center; } }
</style>
