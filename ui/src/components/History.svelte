<script>
  import { app, call, getBridge } from '../lib/state.svelte.js';
  import { t } from '../lib/i18n.js';
  import { ago, bytes, duration, fileName, filesLabel, speed } from '../lib/format.js';
  import { rise } from '../lib/motion.js';
  import SpeedChart from './SpeedChart.svelte';
  import Icon from './Icon.svelte';

  let filter = $state('all');
  let open = $state(null);

  const list = $derived(app.history.filter((h) => filter === 'all' || h.dir === filter));
  const totals = $derived.by(() => {
    const done = app.history.filter((h) => h.phase === 'done');
    return {
      sent: done.filter((h) => h.dir === 'send').reduce((a, h) => a + h.total, 0),
      recv: done.filter((h) => h.dir === 'receive').reduce((a, h) => a + h.total, 0),
      best: Math.max(0, ...done.map((h) => h.average || 0)),
    };
  });

  function title(h) {
    if (!h.files && h.text) return t('Message');
    if (h.files === 1) return fileName(h.names?.[0]);
    return filesLabel(h.files);
  }
  async function clearAll() {
    await call('history_clear');
    app.history = [];
  }
</script>

<div class="history">
  <div class="stats">
    <div><span class="faint">{t('Sent')}</span><b class="num">{bytes(totals.sent)}</b></div>
    <div><span class="faint">{t('Received')}</span><b class="num">{bytes(totals.recv)}</b></div>
    <div><span class="faint">{t('Peak')}</span><b class="num">{speed(totals.best)}</b></div>
  </div>

  <div class="bar">
    <div class="seg">
      {#each [['all', t('All')], ['send', t('Sent')], ['receive', t('Received')]] as [v, l]}
        <button class:on={filter === v} onclick={() => (filter = v)}>{l}</button>
      {/each}
    </div>
    {#if app.history.length}<button class="btn ghost small" onclick={clearAll}><Icon name="trash" size={15} />{t('Clear history')}</button>{/if}
  </div>

  {#if !list.length}
    <p class="faint none">{t('No transfers yet.')}</p>
  {/if}
  <ul>
    {#each list.slice(0, 200) as h, i (h.id)}
      <li in:rise={{ delay: Math.min(i, 10) * 25, y: 8 }} class:open={open === h.id} style:--c={h.dir === 'send' ? 'var(--send)' : 'var(--recv)'}>
        <button class="line" onclick={() => (open = open === h.id ? null : h.id)}>
          <span class="dir"><Icon name={h.dir === 'send' ? 'up' : 'down'} size={14} stroke={2.4} /></span>
          <span class="main">
            <span class="ttl">{title(h)}</span>
            <span class="sub faint">{h.dir === 'send' ? '→' : '←'} {h.peer?.name} · {ago(h.finished_ms || h.created_ms)}{#if h.phase !== 'done'} · <em class="ph">{t(h.phase[0].toUpperCase() + h.phase.slice(1))}</em>{/if}</span>
          </span>
          <span class="right num">
            <span>{bytes(h.total)}</span>
            {#if h.phase === 'done' && h.average}<span class="faint">{speed(h.average)}</span>{/if}
          </span>
        </button>
        {#if open === h.id}
          <div class="detail" in:rise={{ y: -4, scale: 1 }}>
            {#if h.samples?.length > 2}
              <div class="label faint">{t('Speed over time')}</div>
              <SpeedChart samples={h.samples} dir={h.dir} height={86} average={h.average} showAxis />
            {/if}
            <div class="kv num">
              <span>{t('Average')}<b>{speed(h.average)}</b></span>
              <span>{t('Peak')}<b>{speed(h.peak)}</b></span>
              <span>{t('Duration')}<b>{duration(h.elapsed_ms)}</b></span>
              <span>{t('Size')}<b>{bytes(h.total)}</b></span>
              <span>{t('lanes')}<b>{h.lanes || '—'} · {(h.transport || 'tcp').toUpperCase()}</b></span>
            </div>
            {#if h.text}<p class="msg">{h.text}</p>{/if}
            {#if h.error}<p class="err">{h.error}</p>{/if}
            <div class="row">
              {#if h.dir === 'receive' && app.caps.reveal && (h.saved?.[0] || h.dest)}
                <button class="btn small" onclick={() => getBridge().reveal(h.saved?.[0] || h.dest)}><Icon name="folder" size={15} />{t('Show in folder')}</button>
              {/if}
              <button class="btn ghost small" onclick={async () => { await call('history_remove', { id: h.id }); app.history = app.history.filter((x) => x.id !== h.id); }}>{t('Remove')}</button>
            </div>
          </div>
        {/if}
      </li>
    {/each}
  </ul>
</div>

<style>
  .history { display: flex; flex-direction: column; gap: 12px; }
  .stats { display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px; }
  .stats div { padding: 12px 14px; border-radius: var(--r-lg); background: var(--s1); box-shadow: var(--shadow); display: flex; flex-direction: column; gap: 2px; font-size: 12.5px; }
  .stats b { font-size: 16px; font-weight: 500; }
  .bar { display: flex; justify-content: space-between; align-items: center; gap: 8px; }
  .seg { display: inline-flex; background: var(--s3); padding: 3px; border-radius: 11px; gap: 2px; }
  .seg button { height: 30px; padding: 0 12px; border-radius: 8px; font-size: 13.5px; font-weight: 560; color: var(--ink-2); }
  .seg button.on { background: var(--s1); color: var(--ink); box-shadow: var(--shadow); }
  .none { text-align: center; padding: 24px; }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  li { border-radius: var(--r-md); background: var(--s1); box-shadow: var(--shadow); overflow: hidden; }
  .line { width: 100%; display: flex; align-items: center; gap: 12px; padding: 10px 12px; text-align: left; }
  .line:hover { background: var(--s2); }
  .dir { width: 26px; height: 26px; border-radius: 8px; display: grid; place-items: center; flex: none; color: var(--c); background: color-mix(in srgb, var(--c) 14%, transparent); }
  .main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .ttl { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-size: 14px; }
  .sub { font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .ph { font-style: normal; color: var(--bad); }
  .right { display: flex; flex-direction: column; align-items: flex-end; font-size: 11.5px; flex: none; }
  .detail { padding: 4px 14px 14px; }
  .label { font-size: 12px; margin: 6px 0; }
  .kv { display: flex; flex-wrap: wrap; gap: 6px 18px; font-size: 11.5px; margin-top: 10px; color: var(--ink-3); }
  .kv span { display: flex; flex-direction: column; }
  .kv b { color: var(--ink); font-weight: 500; font-size: 13px; }
  .msg { white-space: pre-wrap; background: var(--s3); padding: 8px 10px; border-radius: 8px; font-size: 13.5px; }
  .err { color: var(--bad); font-size: 12.5px; }
  .row { display: flex; gap: 6px; margin-top: 10px; justify-content: flex-end; }
</style>
