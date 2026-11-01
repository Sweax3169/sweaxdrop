<script>
  // The staging tray: what is about to be sent, plus the ways to add to it.
  import { flip } from 'svelte/animate';
  import { app, clearStaged, pick, stage, stagedSummary, unstage } from '../lib/state.svelte.js';
  import { t } from '../lib/i18n.js';
  import { bytes, kindOf } from '../lib/format.js';
  import { rise } from '../lib/motion.js';
  import Icon from './Icon.svelte';

  let { oncompose } = $props();
  let over = $state(false);
  const icon = { image: 'photo', video: 'video', audio: 'file', archive: 'folder', doc: 'file', file: 'file' };

  function onDragOver(e) {
    if (!e.dataTransfer?.types?.includes('Files')) return;
    e.preventDefault();
    over = true;
  }
  function onDrop(e) {
    e.preventDefault();
    over = false;
    stage([...(e.dataTransfer?.files || [])].map((f) => ({ file: f, name: f.name, size: f.size, mime: f.type })));
  }
</script>

<section class="tray" class:full={app.staged.length > 0} class:over={over || (app.dragging && !app.dropTarget)} ondragover={onDragOver} ondragleave={() => (over = false)} ondrop={onDrop} aria-label={t('Ready to send')}>
  {#if app.staged.length}
    <div class="head">
      <div>
        <b>{t('Ready to send')}</b>
        <span class="faint num">{stagedSummary()}</span>
      </div>
      <button class="btn ghost small" onclick={clearStaged}>{t('Clear')}</button>
    </div>
    <ul class="chips scroll">
      {#each app.staged as s (s.key)}
        <li animate:flip={{ duration: 380 }} in:rise={{ y: 8, scale: 0.9 }}>
          <span class="ic"><Icon name={s.folder ? 'folder' : icon[kindOf(s.name)]} size={15} /></span>
          <span class="nm" title={s.path || s.name}>{s.name}</span>
          {#if s.size}<span class="sz num">{bytes(s.size)}</span>{/if}
          <button class="x" onclick={() => unstage(s.key)} aria-label={t('Remove')}><Icon name="x" size={13} stroke={2.4} /></button>
        </li>
      {/each}
    </ul>
  {:else}
    <div class="dropzone">
      <div class="art" aria-hidden="true">
        <span class="sheet s1"></span><span class="sheet s2"></span><span class="sheet s3"></span>
      </div>
      <div>
        <b>{app.wide ? t('Drop files here') : t('Choose what to send')}</b>
        {#if app.wide}<div class="faint small">{t('or on a device to send them right away')}</div>{/if}
      </div>
    </div>
  {/if}
  <div class="actions">
    {#if app.caps.photos}<button class="btn small" onclick={() => pick('photos')}><Icon name="photo" size={16} />{t('Photos')}</button>{/if}
    <button class="btn small" onclick={() => pick('files')}><Icon name="file" size={16} />{t('Files')}</button>
    {#if app.caps.folders}<button class="btn small" onclick={() => pick('folder')}><Icon name="folder" size={16} />{t('Folder')}</button>{/if}
    <button class="btn small" onclick={() => oncompose?.(null, 'text')}><Icon name="text" size={16} />{t('Text')}</button>
    {#if app.kind !== 'web'}<button class="btn small" onclick={() => oncompose?.(null, 'clipboard')}><Icon name="clip" size={16} />{t('Clipboard')}</button>{/if}
  </div>
</section>

<style>
  .tray {
    border-radius: var(--r-lg); background: var(--s1); box-shadow: var(--shadow); padding: 14px;
    display: flex; flex-direction: column; gap: 12px; transition: box-shadow 0.3s, transform 0.5s var(--spring);
  }
  .tray.over { box-shadow: 0 0 0 2px var(--send), var(--shadow-lift); transform: scale(1.01); }
  .head { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .head b { margin-right: 8px; }
  .head .num { font-size: 12px; }
  .chips { list-style: none; margin: 0; padding: 0; display: flex; flex-wrap: wrap; gap: 6px; max-height: 128px; }
  li { display: inline-flex; align-items: center; gap: 6px; height: 30px; padding: 0 4px 0 9px; border-radius: 9px; background: var(--s3); font-size: 13px; max-width: 260px; }
  .ic { color: var(--send); display: inline-flex; }
  .nm { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  .sz { font-size: 10.5px; color: var(--ink-3); }
  .x { width: 22px; height: 22px; border-radius: 6px; display: grid; place-items: center; color: var(--ink-3); flex: none; }
  .x:hover { background: var(--s4); color: var(--ink); }
  .dropzone { display: flex; align-items: center; gap: 16px; padding: 4px 4px 2px; }
  .small { font-size: 13px; }
  .art { position: relative; width: 50px; height: 42px; flex: none; }
  .sheet { position: absolute; width: 30px; height: 38px; border-radius: 6px; background: var(--s3); box-shadow: var(--shadow); transition: transform 0.6s var(--spring); }
  .s1 { left: 0; top: 4px; transform: rotate(-10deg); }
  .s2 { left: 10px; top: 0; background: var(--s4); }
  .s3 { left: 20px; top: 4px; transform: rotate(10deg); background: color-mix(in srgb, var(--send) 30%, var(--s2)); }
  .tray:hover .s1, .tray.over .s1 { transform: rotate(-18deg) translate(-4px, 2px); }
  .tray:hover .s3, .tray.over .s3 { transform: rotate(18deg) translate(4px, 2px); }
  .tray.over .s2 { transform: translateY(-6px); }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; }
  @media (max-width: 520px) { .actions .btn { flex: 1 1 auto; } }
</style>
