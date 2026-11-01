<script>
  // Pairing: show this device's QR and PIN, or type another device's PIN.
  import { onDestroy, onMount } from 'svelte';
  import { app, call, getBridge, toast } from '../lib/state.svelte.js';
  import { t } from '../lib/i18n.js';
  import Sheet from './Sheet.svelte';
  import QR from './QR.svelte';
  import Icon from './Icon.svelte';
  import Scanner from './Scanner.svelte';

  let { onclose, peer = null } = $props();

  let tab = $state(peer ? 'enter' : 'show');
  let pin = $state('');
  let target = $state(peer?.id || '');
  let digits = $state('');
  let busy = $state(false);
  let address = $state('');
  let scanning = $state(false);

  const candidates = $derived(app.peers.filter((p) => p.online && p.kind !== 'web' && !p.trusted));

  onMount(async () => {
    if (tab === 'show') await openPin();
  });
  onDestroy(() => {
    getBridge()
      .call('pin_close')
      .catch(() => {});
  });

  async function openPin() {
    const r = await call('pin_open');
    pin = r.pin;
    const q = await call('qr');
    app.qr = q.uri;
  }

  $effect(() => {
    if (app.pin) pin = app.pin;
  });

  async function submit() {
    if (!target || digits.replace(/\D/g, '').length !== 6) return;
    busy = true;
    try {
      await call('pair_pin', { peer: target, pin: digits });
      onclose();
    } catch {
      digits = '';
    } finally {
      busy = false;
    }
  }

  async function scan() {
    scanning = true;
  }

  async function scanned(text) {
    scanning = false;
    if (!text) return;
    busy = true;
    try {
      await call('pair_qr', { uri: text });
      onclose();
    } finally {
      busy = false;
    }
  }

  async function addAddress() {
    if (!address.trim()) return;
    busy = true;
    try {
      const r = await call('add_address', { address: address.trim() });
      toast(r.peer?.name || address, 'ok');
      target = r.peer?.id || target;
      address = '';
    } finally {
      busy = false;
    }
  }
  const pretty = (p) => (p ? `${String(p).slice(0, 3)} ${String(p).slice(3)}` : '··· ···');
</script>

<Sheet {onclose} width={520} label={t('Pair a device')}>
  <h2>{t('Pair a device')}</h2>
  <p class="muted lead">{t('Paired devices can send without asking and are checked by their key every time.')}</p>

  <div class="tabs" role="tablist">
    <button role="tab" class:on={tab === 'show'} onclick={() => { tab = 'show'; openPin(); }}>{t('Show code')}</button>
    <button role="tab" class:on={tab === 'enter'} onclick={() => (tab = 'enter')}>{t('Enter a code')}</button>
    <span class="ink" style:translate={tab === 'show' ? '0' : '100%'}></span>
  </div>

  {#if tab === 'show'}
    <div class="show">
      <div class="qr"><QR text={app.qr} size={196} /></div>
      <div class="side">
        <div class="label">{t('Scan with a phone')}</div>
        <p class="faint small">{t('With the SweaxDrop app it pairs; without it, the browser opens.')}</p>
        <div class="label">{t('Or type this code on the other device')}</div>
        <div class="pin num">{pretty(pin)}</div>
        <div class="faint small">{t('Valid for 5 minutes')} · {app.device?.name}</div>
      </div>
    </div>
  {:else if scanning}
    <Scanner onresult={scanned} oncancel={() => (scanning = false)} />
  {:else}
    <div class="enter">
      {#if app.caps.scan}
        <button class="btn" onclick={scan} disabled={busy}><Icon name="scan" size={18} /> {t('Scan a code')}</button>
      {/if}
      <label class="label" for="pair-target">{t('Type the six digits shown on')}</label>
      <select id="pair-target" bind:value={target}>
        <option value="" disabled>{t('Device')}…</option>
        {#each candidates as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
        {#if peer && !candidates.some((c) => c.id === peer.id)}<option value={peer.id}>{peer.name}</option>{/if}
      </select>
      <input class="digits num" inputmode="numeric" autocomplete="one-time-code" maxlength="7" placeholder="000 000" bind:value={digits} onkeydown={(e) => e.key === 'Enter' && submit()} />
      <button class="btn primary" onclick={submit} disabled={busy || !target || digits.replace(/\D/g, '').length !== 6}><Icon name="key" size={18} /> {t('Pair')}</button>

      <div class="divider"></div>
      <label class="label" for="addr">{t('Add by address')}</label>
      <div class="addr">
        <input id="addr" placeholder="192.168.1.20" bind:value={address} onkeydown={(e) => e.key === 'Enter' && addAddress()} />
        <button class="btn" onclick={addAddress} disabled={busy || !address.trim()}>{t('Add')}</button>
      </div>
    </div>
  {/if}
</Sheet>

<style>
  h2 { margin: 0 0 4px; font-size: 22px; letter-spacing: -0.02em; font-weight: 700; }
  .lead { margin: 0 0 18px; font-size: 14px; }
  .tabs { position: relative; display: grid; grid-template-columns: 1fr 1fr; background: var(--s3); border-radius: var(--r-md); padding: 3px; margin-bottom: 20px; }
  .tabs button { position: relative; z-index: 1; height: 34px; border-radius: 9px; font-weight: 600; font-size: 14px; color: var(--ink-2); transition: color 0.2s; }
  .tabs button.on { color: var(--ink); }
  .ink { position: absolute; top: 3px; left: 3px; width: calc(50% - 3px); height: 34px; border-radius: 9px; background: var(--s1); box-shadow: var(--shadow); transition: translate 0.5s var(--spring); }
  .show { display: flex; gap: 22px; align-items: center; flex-wrap: wrap; }
  .qr { padding: 8px; background: #fff; border-radius: 20px; box-shadow: var(--shadow); }
  .side { flex: 1; min-width: 200px; }
  .label { font-weight: 620; font-size: 14px; margin: 0 0 4px; display: block; }
  .small { font-size: 12.5px; margin: 0 0 16px; }
  .pin { font-size: 34px; letter-spacing: 0.08em; font-weight: 500; margin: 6px 0 4px; }
  .enter { display: flex; flex-direction: column; gap: 10px; }
  .digits { font-size: 26px; text-align: center; letter-spacing: 0.2em; height: 58px; }
  .divider { height: 1px; background: var(--s3); margin: 10px 0; }
  .addr { display: flex; gap: 8px; }
</style>
