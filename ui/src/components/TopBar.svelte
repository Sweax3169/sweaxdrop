<script>
  import { app, getBridge, setSetting } from '../lib/state.svelte.js';
  import { t } from '../lib/i18n.js';
  import { rise, fade } from '../lib/motion.js';
  import Logo from './Logo.svelte';
  import Icon from './Icon.svelte';

  let { onpair, onsettings } = $props();
  let menu = $state(false);

  const modes = $derived([
    ['everyone', t('Everyone'), 'globe'],
    ['trusted', t('Paired only'), 'shieldok'],
    ['off', t('Off'), 'lock'],
  ]);
  const cur = $derived(modes.find((m) => m[0] === (app.settings.receive || 'everyone')) || modes[0]);
  const tauri = $derived(app.kind === 'tauri');
  const web = $derived(app.kind === 'web');
  const win = () => getBridge().window;
</script>

<header class="top" class:tauri data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    <Logo size={26} animated={Object.values(app.transfers).some((x) => x.phase === 'running')} />
    <div class="names" data-tauri-drag-region>
      <span class="app" data-tauri-drag-region>SweaxDrop</span>
      {#if app.device}
        <span class="me faint" data-tauri-drag-region>
          {#if web}
            {app.web?.device ? t('Connected to {name}', { name: app.web.device.name }) : t('Connecting to the computer…')}
          {:else}
            {app.device.name}{#if app.wide} · <span class="num">{app.device.pretty_id?.slice(0, 9)}</span>{/if}
          {/if}
        </span>
      {/if}
    </div>
  </div>

  <div class="tools">
    {#if !web}
      <div class="mode">
        <button class="btn small chip" class:off={cur[0] === 'off'} onclick={() => (menu = !menu)} aria-haspopup="menu" aria-expanded={menu}>
          <Icon name={cur[2]} size={15} />
          {#if app.wide}<span>{cur[0] === 'off' ? t('Not receiving') : cur[1]}</span>{/if}
        </button>
        {#if menu}
          <div class="scrim" role="presentation" onclick={() => (menu = false)} transition:fade={{ duration: 120 }}></div>
          <div class="pop" role="menu" in:rise={{ y: -6, scale: 0.96 }} out:fade={{ duration: 100 }}>
            <div class="poph faint">{t('Receiving from')}</div>
            {#each modes as [v, l, ic]}
              <button role="menuitemradio" aria-checked={cur[0] === v} class:on={cur[0] === v} onclick={() => { setSetting({ receive: v }); menu = false; }}>
                <Icon name={ic} size={16} /><span>{l}</span>
                {#if cur[0] === v}<Icon name="check" size={15} stroke={2.4} />{/if}
              </button>
            {/each}
          </div>
        {/if}
      </div>
      <button class="btn small" onclick={onpair}><Icon name="qr" size={16} />{#if app.wide}<span>{t('Pair')}</span>{/if}</button>
    {/if}
    {#if app.wide || web}
      <button class="btn small icon ghost" onclick={onsettings} aria-label={t('Settings')}><Icon name="gear" size={18} /></button>
    {/if}
    {#if tauri}
      <div class="win">
        <button onclick={() => win().minimize()} aria-label="Minimize"><Icon name="min" size={15} /></button>
        <button onclick={() => win().maximize()} aria-label="Maximize"><Icon name="max" size={13} /></button>
        <button class="close" onclick={() => win().close()} aria-label={t('Close')}><Icon name="x" size={15} /></button>
      </div>
    {/if}
  </div>
</header>

<style>
  .top {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    padding: calc(10px + env(safe-area-inset-top)) 16px 10px; min-height: 56px; user-select: none;
  }
  .top.tauri { padding-right: 0; padding-top: 6px; min-height: 50px; }
  .brand { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .names { display: flex; flex-direction: column; line-height: 1.15; min-width: 0; }
  .app { font-weight: 720; letter-spacing: -0.02em; font-size: 16px; }
  .me { font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .me .num { font-size: 11px; }
  .tools { display: flex; align-items: center; gap: 6px; }
  .mode { position: relative; }
  .chip.off { color: var(--warn); }
  .scrim { position: fixed; inset: 0; z-index: 30; }
  .pop {
    position: absolute; right: 0; top: calc(100% + 6px); z-index: 31; min-width: 220px; padding: 6px;
    background: var(--s1); border-radius: var(--r-md); box-shadow: var(--shadow-lift); transform-origin: top right;
  }
  .poph { font-size: 12px; padding: 6px 10px 4px; }
  .pop button { display: flex; align-items: center; gap: 10px; width: 100%; padding: 9px 10px; border-radius: 9px; font-size: 14px; text-align: left; }
  .pop button span { flex: 1; }
  .pop button:hover { background: var(--s3); }
  .pop button.on { color: var(--send); font-weight: 600; }
  .win { display: flex; margin-left: 6px; align-self: flex-start; }
  .win button { width: 44px; height: 34px; display: grid; place-items: center; color: var(--ink-2); }
  .win button:hover { background: var(--s3); color: var(--ink); }
  .win .close:hover { background: #e5484d; color: #fff; }
</style>
