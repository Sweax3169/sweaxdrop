<script>
  import { app, call, chooseSaveDir, saveLabel, setSetting } from '../lib/state.svelte.js';
  import { t } from '../lib/i18n.js';
  import Icon from './Icon.svelte';
  import Logo from './Logo.svelte';

  let { onclose = null } = $props();

  const s = $derived(app.settings);
  let name = $state(app.settings.name || app.device?.name || '');
  const palettes = [
    { id: 'tide', a: '#3553ff', b: '#009b7d' },
    { id: 'ember', a: '#e8501f', b: '#1f8f86' },
    { id: 'orchid', a: '#8a3ffc', b: '#0f8fd6' },
    { id: 'forest', a: '#2f6f3e', b: '#b5651d' },
    { id: 'mono', a: '#1d2026', b: '#8a909c' },
  ];
  const paired = $derived(app.peers.filter((p) => p.trusted));
  const desktop = $derived(app.kind === 'tauri' || app.kind === 'mock');
  const web = $derived(app.kind === 'web');

  async function pickDir() {
    await chooseSaveDir();
  }
  function saveName() {
    if (name.trim() && name.trim() !== s.name) setSetting({ name: name.trim() });
  }
</script>

<div class="settings scroll">
  {#if onclose}
    <div class="top">
      <h2>{t('Settings')}</h2>
      <button class="btn ghost icon" onclick={onclose} aria-label={t('Close')}><Icon name="x" /></button>
    </div>
  {/if}

  {#if !web}
    <section>
      <h3>{t('This device')}</h3>
      <div class="row">
        <label for="dev-name">{t('Name')}</label>
        <input id="dev-name" bind:value={name} onblur={saveName} onkeydown={(e) => e.key === 'Enter' && e.currentTarget.blur()} maxlength="48" />
      </div>
      <div class="row ro">
        <span>{t('Device ID')}</span><span class="num faint id">{app.device?.pretty_id}</span>
      </div>
    </section>

    <section>
      <h3>{t('Receiving')}</h3>
      <div class="row">
        <span>{t('Receiving from')}</span>
        <div class="seg">
          {#each [['everyone', t('Everyone')], ['trusted', t('Paired only')], ['off', t('Off')]] as [v, l]}
            <button class:on={s.receive === v} onclick={() => setSetting({ receive: v })}>{l}</button>
          {/each}
        </div>
      </div>
      {#if app.caps.saveDir}
        <div class="row">
          <span>{t('Save to')}</span>
          <button class="path" onclick={pickDir} title={saveLabel(s)}>
            <Icon name="folder" size={16} /><span>{saveLabel(s)}</span>
          </button>
          {#if s.save_uri || s.save_dir}<button class="btn ghost small" onclick={() => setSetting({ save_uri: '', save_label: '', save_dir: '' })}>{t('Reset')}</button>{/if}
        </div>
      {/if}
      {@render toggle('auto_accept_trusted', t('Accept paired devices without asking'))}
      {@render toggle('copy_text', t('Put text from paired devices on the clipboard'))}
      {#if app.kind === 'android' || app.kind === 'ios'}{@render toggle('media_to_gallery', t('Photos and videos go to the gallery'))}{/if}
      {#if app.kind === 'android'}{@render toggle('stay_ready', t('Stay ready in the background'))}{/if}
      {@render toggle('notify', t('Notifications'))}
      {#if desktop}{@render toggle('open_when_done', t('Open the folder when a transfer finishes'))}{/if}
      {@render toggle('web_client', t('Browser client for phones without the app'))}
    </section>

    {#if desktop}
      <section>
        <h3>{t('Windows')}</h3>
        {@render toggle('start_with_windows', t('Start with Windows'))}
        {@render toggle('close_to_tray', t('Keep running in the tray when closed'))}
      </section>
    {/if}

    <section>
      <h3>{t('Speed')}</h3>
      <div class="row col">
        <div class="between"><span>{t('Parallel lanes')}</span><b class="num">{s.streams}</b></div>
        <input type="range" min="1" max="16" step="1" value={s.streams} oninput={(e) => setSetting({ streams: +e.currentTarget.value })} style:--p="{((s.streams - 1) / 15) * 100}%" />
        <span class="faint small">{t('More lanes fill fast Wi-Fi better. 4 is right for most networks.')}</span>
      </div>
      <div class="row">
        <span>{t('Chunk size')}</span>
        <div class="seg">
          {#each [1, 4, 8, 16] as v}<button class:on={s.chunk_mib === v} onclick={() => setSetting({ chunk_mib: v })}>{v} MB</button>{/each}
        </div>
      </div>
      <div class="row">
        <span>{t('Transport')}</span>
        <select value={s.transport} onchange={(e) => setSetting({ transport: e.currentTarget.value })}>
          <option value="tcp">{t('TCP (fastest in our tests)')}</option>
          <option value="quic">{t('QUIC (experimental)')}</option>
        </select>
      </div>
      <div class="row">
        <span>{t('Cipher')}</span>
        <select value={s.cipher} onchange={(e) => setSetting({ cipher: e.currentTarget.value })}>
          <option value="aes">{t('AES-GCM (best with AES hardware)')}</option>
          <option value="chacha">{t('ChaCha20 (older phones)')}</option>
        </select>
      </div>
    </section>
  {/if}

  <section>
    <h3>{t('Appearance')}</h3>
    <div class="row">
      <span>{t('Theme')}</span>
      <div class="seg">
        {#each [['system', t('System')], ['light', t('Light')], ['dark', t('Dark')]] as [v, l]}
          <button class:on={s.theme === v} onclick={() => setSetting({ theme: v })}>{l}</button>
        {/each}
      </div>
    </div>
    <div class="row">
      <span>{t('Colors')}</span>
      <div class="swatches">
        {#each palettes as p}
          <button class="sw" class:on={(s.palette || 'tide') === p.id} onclick={() => setSetting({ palette: p.id })} aria-label={p.id} title={p.id}>
            <span style:background={p.a}></span><span style:background={p.b}></span>
          </button>
        {/each}
      </div>
    </div>
    <div class="row">
      <span>{t('Language')}</span>
      <div class="seg">
        {#each [['en', 'English'], ['tr', 'Türkçe']] as [v, l]}
          <button class:on={(s.lang || document.documentElement.lang) === v} onclick={() => setSetting({ lang: v })}>{l}</button>
        {/each}
      </div>
    </div>
  </section>

  {#if paired.length && !web}
    <section>
      <h3>{t('Paired')}</h3>
      {#each paired as p (p.id)}
        <div class="row">
          <span class="pd"><Icon name="shieldok" size={16} /> {p.name}</span>
          <button class="btn ghost small" onclick={() => call('untrust', { peer: p.id })}>{t('Untrust')}</button>
        </div>
      {/each}
    </section>
  {/if}

  <section class="about">
    <Logo size={36} />
    <div>
      <b>SweaxDrop</b> <span class="faint num">{app.device?.version || '1.0.0'}</span>
      <div class="faint small">{t('Everything is encrypted end to end (TLS 1.3) and checked chunk by chunk (BLAKE3).')}</div>
      <div class="faint small">Sweax Studios · MIT · github.com/Sweax3169/sweaxdrop</div>
    </div>
  </section>
</div>

{#snippet toggle(key, label)}
  <label class="row toggle">
    <span>{label}</span>
    <input type="checkbox" checked={!!s[key]} onchange={(e) => setSetting({ [key]: e.currentTarget.checked })} />
    <span class="sw-toggle" aria-hidden="true"></span>
  </label>
{/snippet}

<style>
  .settings { height: 100%; padding: 4px 4px 24px; }
  .top { display: flex; align-items: center; justify-content: space-between; margin-bottom: 8px; }
  h2 { margin: 0; font-size: 22px; letter-spacing: -0.02em; }
  section { background: var(--s1); border-radius: var(--r-lg); padding: 6px 16px; margin-bottom: 12px; box-shadow: var(--shadow); }
  h3 { margin: 12px 0 4px; font-size: 13px; font-weight: 620; color: var(--ink-3); }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 14px; min-height: 48px; padding: 6px 0; font-size: 14.5px; }
  .row + .row { box-shadow: 0 -1px 0 var(--s3); }
  .row > input:not([type]), .row > select { max-width: 260px; }
  .row.col { flex-direction: column; align-items: stretch; gap: 8px; }
  .between { display: flex; justify-content: space-between; }
  .id { font-size: 12.5px; }
  .small { font-size: 12.5px; }
  .seg { display: inline-flex; background: var(--s3); padding: 3px; border-radius: 11px; gap: 2px; flex-wrap: wrap; }
  .seg button { height: 30px; padding: 0 12px; border-radius: 8px; font-size: 13.5px; font-weight: 560; color: var(--ink-2); transition: background 0.2s, color 0.2s; }
  .seg button.on { background: var(--s1); color: var(--ink); box-shadow: var(--shadow); }
  .path { display: inline-flex; align-items: center; gap: 8px; max-width: 280px; padding: 6px 10px; border-radius: 10px; background: var(--s3); font-size: 13.5px; }
  .path span { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .toggle { cursor: pointer; position: relative; }
  .toggle input { position: absolute; opacity: 0; pointer-events: none; }
  .sw-toggle { width: 42px; height: 26px; border-radius: 13px; background: var(--s4); position: relative; flex: none; transition: background 0.25s; }
  .sw-toggle::after { content: ''; position: absolute; top: 3px; left: 3px; width: 20px; height: 20px; border-radius: 50%; background: #fff; box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25); transition: transform 0.5s var(--spring); }
  .toggle input:checked + .sw-toggle { background: var(--send); }
  .toggle input:checked + .sw-toggle::after { transform: translateX(16px); }
  .toggle input:focus-visible + .sw-toggle { outline: 2px solid var(--focus); outline-offset: 2px; }
  input[type='range'] { appearance: none; height: 6px; border-radius: 3px; background: linear-gradient(90deg, var(--send) var(--p), var(--s4) var(--p)); padding: 0; }
  input[type='range']::-webkit-slider-thumb { appearance: none; width: 20px; height: 20px; border-radius: 50%; background: #fff; box-shadow: 0 1px 4px rgba(0, 0, 0, 0.3); }
  .swatches { display: flex; gap: 8px; }
  .sw { width: 34px; height: 34px; border-radius: 11px; overflow: hidden; display: grid; grid-template-columns: 1fr 1fr; transition: transform 0.4s var(--spring); }
  .sw span { display: block; }
  .sw.on { box-shadow: 0 0 0 2px var(--s1), 0 0 0 4px var(--ink); }
  .sw:active { transform: scale(0.9); }
  .pd { display: inline-flex; gap: 8px; align-items: center; }
  .pd :global(svg) { color: var(--recv); }
  .about { display: flex; gap: 14px; align-items: center; padding: 16px; font-size: 14px; }
</style>
