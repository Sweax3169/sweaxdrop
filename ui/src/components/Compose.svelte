<script>
  // "What do you want to send to X?" — files, a folder, photos, text, clipboard.
  import { app, getBridge, pick, sendTo, toast } from '../lib/state.svelte.js';
  import { t } from '../lib/i18n.js';
  import Sheet from './Sheet.svelte';
  import Icon from './Icon.svelte';
  import DeviceGlyph from './DeviceGlyph.svelte';

  let { peer = null, startWith = null, onclose } = $props();

  let mode = $state(startWith || 'menu');
  let text = $state('');
  let area = $state();

  $effect(() => {
    if (mode === 'text') setTimeout(() => area?.focus(), 60);
  });

  async function choose(what) {
    if (what === 'text') {
      mode = 'text';
      return;
    }
    if (what === 'clipboard') {
      const c = await getBridge().clipboard.read();
      if (!c) {
        toast(t('Nothing to send yet'));
        return;
      }
      if (peer) {
        await sendTo(peer.id, [], c);
        onclose();
      } else {
        text = c;
        mode = 'text';
      }
      return;
    }
    const before = app.staged.length;
    await pick(what);
    if (peer && app.staged.length > before) {
      await sendTo(peer.id);
    }
    onclose();
  }

  async function sendText() {
    const v = text.trim();
    if (!v) return;
    if (!peer) {
      const online = app.peers.filter((p) => p.online);
      if (online.length !== 1) {
        toast(t('Pick a device to send to'));
        return;
      }
      await sendTo(online[0].id, [], v);
    } else {
      await sendTo(peer.id, [], v);
    }
    onclose();
  }

  const options = $derived(
    [
      app.caps.photos && { id: 'photos', icon: 'photo', label: t('Photos & videos') },
      { id: 'files', icon: 'file', label: t('Files') },
      app.caps.folders && { id: 'folder', icon: 'folder', label: t('Folder') },
      { id: 'text', icon: 'text', label: t('Text') },
      app.kind !== 'web' && { id: 'clipboard', icon: 'clip', label: t('Clipboard') },
    ].filter(Boolean),
  );
</script>

<Sheet {onclose} width={440} label={t('Choose what to send')}>
  <div class="head">
    {#if peer}
      <DeviceGlyph platform={peer.platform} model={peer.model} name={peer.name} kind={peer.kind} size={40} />
      <div>
        <div class="muted small">{t('Send')}</div>
        <div class="who">{peer.name}</div>
      </div>
    {:else}
      <div class="who">{t('Choose what to send')}</div>
    {/if}
  </div>

  {#if mode === 'menu'}
    <div class="grid">
      {#each options as o, i (o.id)}
        <button class="opt" onclick={() => choose(o.id)} style:--i={i}>
          <span class="ic"><Icon name={o.icon} size={22} /></span>
          <span>{o.label}</span>
        </button>
      {/each}
    </div>
  {:else}
    <textarea bind:this={area} bind:value={text} rows="5" placeholder={t('Type or paste a message')} onkeydown={(e) => (e.ctrlKey || e.metaKey) && e.key === 'Enter' && sendText()}></textarea>
    <div class="buttons">
      <button class="btn" onclick={() => (startWith ? onclose() : (mode = 'menu'))}>{t('Back')}</button>
      <button class="btn primary" onclick={sendText} disabled={!text.trim()}><Icon name="send" size={17} stroke={2.2} />{t('Send')}</button>
    </div>
  {/if}
</Sheet>

<style>
  .head { display: flex; gap: 12px; align-items: center; margin-bottom: 18px; }
  .who { font-size: 19px; font-weight: 680; letter-spacing: -0.015em; }
  .small { font-size: 12.5px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(118px, 1fr)); gap: 10px; }
  .opt {
    display: flex; flex-direction: column; align-items: flex-start; gap: 18px; padding: 14px; border-radius: var(--r-lg);
    background: var(--s2); text-align: left; font-weight: 600; font-size: 14px; box-shadow: var(--shadow);
    transition: transform 0.55s var(--spring), background 0.2s; animation: in 0.55s var(--spring) both; animation-delay: calc(var(--i) * 40ms);
  }
  .opt:hover { transform: translateY(-2px); background: var(--s3); }
  .opt:active { transform: scale(0.95); }
  .ic { width: 38px; height: 38px; border-radius: 11px; display: grid; place-items: center; background: color-mix(in srgb, var(--send) 14%, transparent); color: var(--send); }
  @keyframes in { from { opacity: 0; transform: translateY(10px) scale(0.94); } }
  textarea { resize: vertical; min-height: 120px; font-size: 15px; }
  .buttons { display: flex; justify-content: flex-end; gap: 10px; margin-top: 14px; }
</style>
