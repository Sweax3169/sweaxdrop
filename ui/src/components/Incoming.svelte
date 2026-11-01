<script>
  import { app, answer, getBridge } from '../lib/state.svelte.js';
  import { t } from '../lib/i18n.js';
  import { bytes, fileName, filesLabel, kindOf } from '../lib/format.js';
  import Sheet from './Sheet.svelte';
  import DeviceGlyph from './DeviceGlyph.svelte';
  import Icon from './Icon.svelte';

  let { offer } = $props();

  const tr = $derived(offer.transfer);
  const items = $derived((tr.items || offer.files || []).filter((i) => !i.dir));
  let skip = $state(new Set());
  let trust = $state(false);
  let saveDir = $state('');
  let busy = $state(false);

  const chosen = $derived(items.filter((i) => !skip.has(i.id)));
  const chosenSize = $derived(chosen.reduce((a, i) => a + (i.size || 0), 0));
  const icon = { image: 'photo', video: 'video', audio: 'file', archive: 'folder', doc: 'file', file: 'file' };

  function toggle(id) {
    const s = new Set(skip);
    s.has(id) ? s.delete(id) : s.add(id);
    skip = s;
  }

  async function pickDir() {
    const d = await getBridge().pickDir();
    if (d) saveDir = d;
  }

  async function respond(accept) {
    busy = true;
    try {
      if (offer.web && accept) app.sheet = { type: 'webreceive', offer };
      await answer(offer, accept, { skip: [...skip], trust, save_dir: saveDir || undefined });
    } finally {
      busy = false;
    }
  }
</script>

<Sheet onclose={() => respond(false)} width={500} label={t('wants to send you')}>
  <div class="head">
    <DeviceGlyph platform={tr.peer?.platform} model={tr.peer?.model} name={tr.peer?.name} size={52} glow={0.5} dir="receive" />
    <div>
      <div class="who">{tr.peer?.name}</div>
      <div class="muted">
        {t('wants to send you')}
        <b>{items.length ? `${filesLabel(items.length)} · ${bytes(tr.total)}` : t('a message')}</b>
      </div>
    </div>
  </div>

  {#if tr.text && !items.length}
    <p class="msg">{tr.text}</p>
  {/if}

  {#if items.length}
    <ul class="files scroll">
      {#each items.slice(0, 200) as it (it.id)}
        <li class:off={skip.has(it.id)}>
          <button class="check" class:on={!skip.has(it.id)} onclick={() => toggle(it.id)} aria-label={fileName(it.path)}>
            {#if !skip.has(it.id)}<Icon name="check" size={13} stroke={3} />{/if}
          </button>
          <span class="ic"><Icon name={icon[kindOf(it.path)]} size={16} /></span>
          <span class="nm" title={it.path}>{it.path}</span>
          <span class="sz num">{bytes(it.size)}</span>
        </li>
      {/each}
      {#if items.length > 200}<li class="more faint">+ {(items.length - 200).toLocaleString()}</li>{/if}
    </ul>
  {/if}

  {#if !offer.trusted && offer.safety && !offer.web}
    <div class="safety">
      <Icon name="shield" size={18} />
      <div>
        <div class="row"><span>{t('Safety code')}</span><b class="num">{offer.safety}</b></div>
        <div class="faint">{t('The same six digits should be on the other screen.')}</div>
      </div>
    </div>
  {/if}

  {#if app.caps.saveDir && items.length && !offer.web}
    <div class="save">
      <span class="faint">{t('Save to')}</span>
      <span class="path" title={saveDir || app.settings.save_dir}>{saveDir || app.settings.save_dir || 'Downloads\\SweaxDrop'}</span>
      <button class="btn ghost small" onclick={pickDir}>{t('Change')}</button>
    </div>
  {/if}

  {#if !offer.web && !offer.trusted}
    <label class="trust">
      <input type="checkbox" bind:checked={trust} />
      <span>{t('Always accept from this device')}</span>
    </label>
  {/if}

  <div class="buttons">
    <button class="btn" onclick={() => respond(false)} disabled={busy}>{t('Decline')}</button>
    <button class="btn recv" onclick={() => respond(true)} disabled={busy || (items.length > 0 && chosen.length === 0)}>
      <Icon name="down" size={17} stroke={2.2} />
      {t('Accept')}{#if items.length && skip.size} · {bytes(chosenSize)}{/if}
    </button>
  </div>
</Sheet>

<style>
  .head { display: flex; gap: 14px; align-items: center; margin-bottom: 16px; }
  .who { font-size: 20px; font-weight: 680; letter-spacing: -0.015em; }
  .msg { padding: 12px 14px; background: var(--s3); border-radius: var(--r-md); white-space: pre-wrap; word-break: break-word; max-height: 200px; overflow: auto; margin: 0 0 12px; }
  .files { list-style: none; margin: 0 0 14px; padding: 4px; background: var(--s2); border-radius: var(--r-md); max-height: 260px; }
  li { display: flex; align-items: center; gap: 10px; padding: 6px 8px; border-radius: 8px; font-size: 13.5px; }
  li.off { opacity: 0.45; }
  li:hover { background: var(--s3); }
  .check { width: 18px; height: 18px; border-radius: 6px; display: grid; place-items: center; background: var(--s4); color: var(--recv-ink); flex: none; transition: background 0.2s, transform 0.4s var(--spring); }
  .check.on { background: var(--recv); }
  .check:active { transform: scale(0.85); }
  .ic { color: var(--ink-3); display: inline-flex; flex: none; }
  .nm { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sz { font-size: 11.5px; color: var(--ink-3); flex: none; }
  .more { justify-content: center; }
  .safety { display: flex; gap: 12px; align-items: flex-start; padding: 12px 14px; border-radius: var(--r-md); background: var(--s3); margin-bottom: 12px; font-size: 13.5px; }
  .safety .row { display: flex; justify-content: space-between; gap: 12px; }
  .safety b { font-size: 16px; letter-spacing: 0.06em; }
  .safety > div { flex: 1; }
  .save { display: flex; align-items: center; gap: 10px; font-size: 13.5px; margin-bottom: 10px; }
  .path { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .trust { display: flex; align-items: center; gap: 10px; font-size: 14px; margin: 4px 0 18px; cursor: pointer; }
  .trust input { width: 18px; height: 18px; accent-color: var(--recv); }
  .buttons { display: flex; gap: 10px; justify-content: flex-end; }
  .buttons .btn { min-width: 120px; }
  @media (max-width: 859px) { .buttons .btn { flex: 1; } }
</style>
