<script>
  import { app, sendTo, stage } from '../lib/state.svelte.js';
  import { t } from '../lib/i18n.js';
  import { filesLabel } from '../lib/format.js';
  import DeviceGlyph from './DeviceGlyph.svelte';
  import Icon from './Icon.svelte';

  let { peer, onmenu, oncompose } = $props();

  let hover = $state(false);
  let htmlDrag = $state(false);
  let launched = $state(0);

  const active = $derived(
    Object.values(app.transfers).find((x) => x.peer?.id === peer.id && ['connecting', 'waiting', 'running', 'paused', 'reconnecting'].includes(x.phase)),
  );
  const progress = $derived(active && active.total > 0 ? active.bytes / active.total : null);
  const targeted = $derived(app.dropTarget === peer.id || htmlDrag);
  const sub = $derived.by(() => {
    if (!peer.online) return t('offline');
    if (active?.phase === 'waiting') return t('Waiting to accept');
    if (active?.phase === 'connecting') return t('Connecting…');
    const via = peer.via === 'usb' ? t('USB') : peer.via === 'direct' ? t('Direct') : peer.kind === 'web' ? t('browser') : t('Wi-Fi');
    return via;
  });
  const platformLabel = $derived(
    { android: 'Android', ios: 'iOS', windows: 'Windows', macos: 'macOS', linux: 'Linux', web: t('browser') }[peer.platform] || peer.platform || t('Device'),
  );

  async function activate() {
    if (!peer.online) return;
    if (app.staged.length) {
      launched++;
      await sendTo(peer.id);
    } else {
      oncompose?.(peer);
    }
  }

  function onDragOver(e) {
    if (!e.dataTransfer?.types?.includes('Files')) return;
    e.preventDefault();
    htmlDrag = true;
  }
  async function onDrop(e) {
    e.preventDefault();
    htmlDrag = false;
    const files = [...(e.dataTransfer?.files || [])].map((f) => ({ file: f, name: f.name, size: f.size, mime: f.type }));
    if (!files.length) return;
    launched++;
    await sendTo(peer.id, files);
  }
</script>

<div
  class="tile"
  class:offline={!peer.online}
  class:targeted
  class:busy={!!active}
  class:armed={app.staged.length > 0 && peer.online}
  data-peer={peer.id}
  role="button"
  tabindex="0"
  onclick={activate}
  onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && activate()}
  onpointerenter={() => (hover = true)}
  onpointerleave={() => (hover = false)}
  ondragover={onDragOver}
  ondragleave={() => (htmlDrag = false)}
  ondrop={onDrop}
  style:--c={active?.dir === 'receive' ? 'var(--recv)' : 'var(--send)'}
>
  <div class="glyph">
    {#key launched}
      <div class="launch" class:go={launched > 0}></div>
    {/key}
    <DeviceGlyph platform={peer.platform} model={peer.model} name={peer.name} kind={peer.kind} glow={active ? 1 : targeted ? 0.6 : 0} {progress} dir={active?.dir || 'send'} />
    {#if peer.online}<span class="dot" title={t('online')}></span>{/if}
  </div>
  <div class="text">
    <div class="name" title={peer.name}>{peer.name}</div>
    <div class="sub">
      {#if peer.trusted}<span class="trust" title={t('paired')}><Icon name="shieldok" size={13} stroke={2} /></span>{/if}
      <span>{platformLabel}</span><span class="sep">·</span><span>{sub}</span>
    </div>
  </div>
  {#if targeted}
    <div class="cta drop">{t('Drop to send')}</div>
  {:else if app.staged.length > 0 && peer.online && hover}
    <div class="cta">{t('Send')} · {filesLabel(app.staged.length)}</div>
  {/if}
  {#if peer.kind !== 'web'}
    <button
      class="menu btn ghost small icon"
      aria-label="More"
      onclick={(e) => {
        e.stopPropagation();
        onmenu?.(peer, e.currentTarget);
      }}><Icon name="more" size={18} stroke={2.4} /></button
    >
  {/if}
</div>

<style>
  .tile {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 18px 16px 14px;
    border-radius: var(--r-lg);
    background: var(--s1);
    box-shadow: var(--shadow);
    cursor: pointer;
    min-width: 0;
    transition: transform 0.6s var(--spring), box-shadow 0.4s var(--ease-out), background 0.3s;
    outline: none;
  }
  .tile:hover { transform: translateY(-3px); box-shadow: var(--shadow-lift); background: var(--s2); }
  .tile:active { transform: translateY(0) scale(0.97); transition-duration: 0.1s; }
  .tile:focus-visible { box-shadow: 0 0 0 2px var(--focus), var(--shadow-lift); }
  .tile.armed { background: var(--s2); }
  .tile.targeted {
    transform: translateY(-6px) scale(1.04);
    box-shadow: 0 0 0 2px var(--c), 0 24px 50px -18px color-mix(in srgb, var(--c) 60%, transparent);
  }
  .tile.offline { cursor: default; opacity: 0.55; }
  .tile.offline:hover { transform: none; box-shadow: var(--shadow); }

  .glyph { position: relative; width: 56px; height: 56px; }
  .dot {
    position: absolute; right: -2px; top: 2px; width: 10px; height: 10px; border-radius: 50%;
    background: var(--ok); box-shadow: 0 0 0 3px var(--s1);
  }
  .tile:hover .dot, .tile.armed .dot { box-shadow: 0 0 0 3px var(--s2); }
  .launch { position: absolute; inset: -8px; border-radius: 50%; pointer-events: none; }
  .launch.go { animation: ripple 0.9s var(--ease-out); }
  @keyframes ripple {
    0% { box-shadow: 0 0 0 0 color-mix(in srgb, var(--c) 55%, transparent); }
    100% { box-shadow: 0 0 0 26px transparent; }
  }

  .text { min-width: 0; }
  .name { font-weight: 640; font-size: 15.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; letter-spacing: -0.01em; }
  .sub { display: flex; align-items: center; gap: 5px; font-size: 12.5px; color: var(--ink-3); white-space: nowrap; overflow: hidden; }
  .sep { opacity: 0.6; }
  .trust { color: var(--recv); display: inline-flex; }

  .cta {
    position: absolute; left: 12px; right: 12px; bottom: 12px;
    height: 30px; border-radius: 9px; display: grid; place-items: center;
    background: var(--c); color: var(--send-ink); font-weight: 620; font-size: 13px;
    animation: cta 0.45s var(--spring);
  }
  .tile:has(.cta) .text { visibility: hidden; }
  @keyframes cta { from { transform: translateY(8px) scale(0.9); opacity: 0; } }

  .menu { position: absolute; top: 8px; right: 8px; opacity: 0; transition: opacity 0.2s; color: var(--ink-3); }
  .tile:hover .menu, .menu:focus-visible { opacity: 1; }
  @media (hover: none) { .menu { opacity: 0.7; } }
</style>
