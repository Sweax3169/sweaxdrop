<script>
  import { onMount } from 'svelte';
  import { app, init } from './lib/state.svelte.js';
  import { t } from './lib/i18n.js';
  import TopBar from './components/TopBar.svelte';
  import Nearby from './components/Nearby.svelte';
  import Tray from './components/Tray.svelte';
  import Activity from './components/Activity.svelte';
  import History from './components/History.svelte';
  import Settings from './components/Settings.svelte';
  import Incoming from './components/Incoming.svelte';
  import Compose from './components/Compose.svelte';
  import PairSheet from './components/PairSheet.svelte';
  import PeerMenu from './components/PeerMenu.svelte';
  import WebReceive from './components/WebReceive.svelte';
  import Sheet from './components/Sheet.svelte';
  import Toasts from './components/Toasts.svelte';
  import Icon from './components/Icon.svelte';

  let side = $state('activity');
  let menu = $state(null);

  onMount(init);

  const sheet = $derived(app.sheet);
  const offer = $derived(app.incoming[0]);
  const running = $derived(Object.values(app.transfers).filter((x) => ['running', 'paused', 'waiting', 'connecting', 'reconnecting'].includes(x.phase)).length);

  function compose(peer, startWith = null) {
    app.sheet = { type: 'compose', peer, startWith };
  }
  function pair(peer = null) {
    app.sheet = { type: 'pair', peer };
  }
  function close() {
    app.sheet = null;
  }

  // Phones without the app: drag & drop onto the page also stages files.
  function onWindowDragOver(e) {
    if (app.kind !== 'tauri' && e.dataTransfer?.types?.includes('Files')) {
      e.preventDefault();
      app.dragging = true;
    }
  }
</script>

<svelte:window ondragover={onWindowDragOver} ondragleave={(e) => !e.relatedTarget && (app.dragging = false)} ondrop={(e) => { e.preventDefault(); app.dragging = false; }} />

{#if !app.ready}
  <div class="boot"><span></span><span></span><span></span></div>
{:else}
  <div class="shell" class:wide={app.wide} class:dragging={app.dragging}>
    <TopBar onpair={() => pair()} onsettings={() => (app.sheet = { type: 'settings' })} />

    {#if app.error}
      <div class="fatal">
        <b>{app.kind !== 'web' ? t('Something went wrong') : app.error === 'no code' ? t('Open SweaxDrop on your computer and scan its QR code with this phone.') : t('This code expired. Scan the QR code on the computer again.')}</b>
        {#if app.kind !== 'web'}<p class="faint">{app.error}</p>{/if}
      </div>
    {:else if app.wide}
      <main class="cols">
        <div class="left scroll">
          <Nearby oncompose={(p) => compose(p)} onmenu={(p, el) => (menu = { peer: p, el })} onpair={() => pair()} />
          <Tray oncompose={(p, w) => compose(p, w)} />
        </div>
        <aside class="right">
          <div class="tabs" role="tablist">
            <button role="tab" class:on={side === 'activity'} onclick={() => (side = 'activity')}>
              {t('Activity')}{#if running}<span class="badge num">{running}</span>{/if}
            </button>
            <button role="tab" class:on={side === 'history'} onclick={() => (side = 'history')}>{t('History')}</button>
          </div>
          <div class="pane scroll">
            {#if side === 'activity'}<Activity />{:else}<History />{/if}
          </div>
        </aside>
      </main>
    {:else}
      <main class="mobile scroll">
        {#if app.tab === 'send'}
          <Nearby oncompose={(p) => compose(p)} onmenu={(p, el) => (menu = { peer: p, el })} onpair={() => pair()} />
          <Tray oncompose={(p, w) => compose(p, w)} />
          <Activity compact />
        {:else if app.tab === 'activity'}
          <Activity />
          <History />
        {:else}
          <Settings />
        {/if}
      </main>
      {#if app.kind !== 'web'}
        <nav class="mobile-nav">
          {#each [['send', 'send', t('Send')], ['activity', 'history', t('Activity')], ['settings', 'gear', t('Settings')]] as [id, ic, label]}
            <button class:on={app.tab === id} onclick={() => (app.tab = id)}>
              <span class="ic"><Icon name={ic} size={21} />{#if id === 'activity' && running}<i class="dot"></i>{/if}</span>
              <span>{label}</span>
            </button>
          {/each}
        </nav>
      {/if}
    {/if}
  </div>

  {#if offer && !sheet}
    {#key offer.transfer.id}<Incoming {offer} />{/key}
  {/if}
  {#if sheet?.type === 'compose'}
    <Compose peer={sheet.peer} startWith={sheet.startWith} onclose={close} />
  {:else if sheet?.type === 'pair'}
    <PairSheet peer={sheet.peer} onclose={close} />
  {:else if sheet?.type === 'settings'}
    <Sheet onclose={close} width={560} label={t('Settings')}><Settings onclose={close} /></Sheet>
  {:else if sheet?.type === 'webreceive'}
    <WebReceive offer={sheet.offer} onclose={close} />
  {/if}
  {#if menu}
    <PeerMenu peer={menu.peer} anchor={menu.el} onclose={() => (menu = null)} onpair={(p) => pair(p)} />
  {/if}
  <Toasts />
{/if}

<style>
  .boot { height: 100%; display: grid; place-content: center; grid-auto-flow: column; gap: 6px; }
  .boot span { width: 8px; height: 8px; border-radius: 50%; background: var(--ink-3); animation: b 1s infinite; }
  .boot span:nth-child(2) { animation-delay: 0.15s; }
  .boot span:nth-child(3) { animation-delay: 0.3s; }
  @keyframes b { 0%, 100% { opacity: 0.2; transform: translateY(0); } 50% { opacity: 1; transform: translateY(-4px); } }

  .shell { height: 100%; display: flex; flex-direction: column; }
  .cols { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr) minmax(330px, 400px); gap: 16px; padding: 0 16px 16px; }
  .left { display: flex; flex-direction: column; gap: 16px; min-height: 0; padding: 4px 4px 8px; margin: -4px; }
  .left > :global(.tray) { margin-top: auto; position: sticky; bottom: 0; }
  .right { display: flex; flex-direction: column; min-height: 0; background: var(--floor); border-radius: var(--r-xl); padding: 8px; }
  .tabs { display: flex; gap: 4px; padding: 4px 4px 10px; }
  .tabs button { height: 34px; padding: 0 14px; border-radius: 10px; font-weight: 600; font-size: 14px; color: var(--ink-3); display: inline-flex; align-items: center; gap: 8px; transition: color 0.2s, background 0.2s; }
  .tabs button.on { color: var(--ink); background: var(--s1); box-shadow: var(--shadow); }
  .badge { font-size: 11px; min-width: 20px; height: 20px; padding: 0 6px; border-radius: 7px; display: inline-grid; place-items: center; background: var(--send); color: var(--send-ink); }
  .pane { flex: 1; min-height: 0; padding: 0 4px 4px; }

  .mobile { flex: 1; min-height: 0; padding: 0 14px 24px; display: flex; flex-direction: column; gap: 16px; }
  .mobile-nav { display: grid; grid-template-columns: repeat(3, 1fr); padding: 6px 10px calc(6px + var(--safe-bottom)); background: var(--s1); box-shadow: 0 -1px 0 var(--s3); }
  .mobile-nav button { display: flex; flex-direction: column; align-items: center; gap: 2px; padding: 6px; font-size: 11.5px; font-weight: 600; color: var(--ink-3); border-radius: 12px; transition: color 0.2s; }
  .mobile-nav button.on { color: var(--ink); }
  .mobile-nav .ic { position: relative; display: grid; place-items: center; width: 52px; height: 30px; border-radius: 15px; transition: background 0.35s var(--spring); }
  .mobile-nav button.on .ic { background: color-mix(in srgb, var(--send) 16%, transparent); color: var(--send); }
  .dot { position: absolute; top: 3px; right: 12px; width: 7px; height: 7px; border-radius: 50%; background: var(--send); }
  .fatal { margin: 40px auto; max-width: 420px; padding: 24px; text-align: center; background: var(--s1); border-radius: var(--r-lg); box-shadow: var(--shadow); }
</style>
