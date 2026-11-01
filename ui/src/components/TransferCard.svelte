<script>
  import { app, call, getBridge } from '../lib/state.svelte.js';
  import { t } from '../lib/i18n.js';
  import { bytes, duration, fileName, filesLabel, speedParts } from '../lib/format.js';
  import Lanes from './Lanes.svelte';
  import SpeedChart from './SpeedChart.svelte';
  import Icon from './Icon.svelte';

  let { tr, compact = false } = $props();

  const send = $derived(tr.dir === 'send');
  const name = $derived(tr.peer?.name || t('Device'));
  const finished = $derived(['done', 'failed', 'cancelled', 'declined'].includes(tr.phase));
  const pct = $derived(tr.total > 0 ? Math.min(100, (tr.bytes / tr.total) * 100) : tr.phase === 'done' ? 100 : 0);
  const sp = $derived(speedParts(tr.phase === 'running' ? tr.speed : finished ? tr.average : 0));
  const textOnly = $derived(tr.files === 0 && tr.text);

  const status = $derived.by(() => {
    switch (tr.phase) {
      case 'queued': return t('Queued');
      case 'connecting': return t('Connecting…');
      case 'waiting': return t('Waiting for {name} to accept', { name });
      case 'asking': return t('Waiting for your answer');
      case 'paused': return t('Paused');
      case 'reconnecting': return t('Reconnecting — it picks up where it stopped');
      case 'done': return send ? t('Sent to {name}', { name }) : t('Received from {name}', { name });
      case 'failed': return t('Failed');
      case 'cancelled': return t('Cancelled');
      case 'declined': return t('Declined');
      default: return send ? t('Sending to {name}', { name }) : t('Receiving from {name}', { name });
    }
  });

  const title = $derived.by(() => {
    if (textOnly) return t('Message');
    if (tr.files === 1) return fileName(tr.names?.[0] || '');
    const top = new Set((tr.names || []).map((n) => n.split('/')[0]));
    if (top.size === 1 && (tr.names?.[0] || '').includes('/')) return [...top][0];
    return filesLabel(tr.files);
  });

  async function pause() {
    await call('pause', { id: tr.id, paused: tr.phase !== 'paused' });
  }
  async function cancel() {
    await call('cancel', { id: tr.id });
  }
  function reveal() {
    const p = tr.saved?.[0] || tr.dest;
    if (p) getBridge().reveal(p);
  }
</script>

<article class="card" class:send class:finished class:compact style:--c={send ? 'var(--send)' : 'var(--recv)'}>
  <header>
    <span class="dir"><Icon name={send ? 'up' : 'down'} size={16} stroke={2.2} /></span>
    <div class="what">
      <div class="title" title={title}>{title}</div>
      <div class="status" class:bad={tr.phase === 'failed'}>{status}</div>
    </div>
    <div class="actions">
      {#if !finished && tr.phase !== 'asking'}
        {#if tr.phase === 'running' || tr.phase === 'paused'}
          <button class="btn ghost small icon" onclick={pause} aria-label={tr.phase === 'paused' ? t('Resume') : t('Pause')} title={tr.phase === 'paused' ? t('Resume') : t('Pause')}>
            <Icon name={tr.phase === 'paused' ? 'play' : 'pause'} size={16} stroke={2.2} />
          </button>
        {/if}
        <button class="btn ghost small icon" onclick={cancel} aria-label={t('Stop')} title={t('Stop')}><Icon name="x" size={16} stroke={2.2} /></button>
      {:else if tr.phase === 'done' && !send && app.caps.reveal && tr.files > 0}
        <button class="btn ghost small" onclick={reveal}><Icon name="folder" size={15} /> {t('Show in folder')}</button>
      {/if}
    </div>
  </header>

  {#if textOnly}
    <p class="msg">{tr.text}</p>
  {:else}
    <Lanes lanes={tr.lanes?.length ? tr.lanes : [0, 0, 0, 0]} phase={tr.phase} dir={tr.dir} height={compact ? 30 : 40} total={tr.total} bytes={tr.bytes} />

    <div class="numbers">
      <div class="speed">
        <span class="big num">{sp[0]}</span><span class="unit num">{sp[1]}</span>
        {#if finished && tr.phase === 'done'}<span class="faint small">{t('avg')}</span>{/if}
      </div>
      <div class="meta num">
        <span>{bytes(tr.bytes)} <span class="faint">/ {bytes(tr.total)}</span></span>
        {#if !finished && tr.eta_ms}<span class="faint">{duration(tr.eta_ms)} {t('left')}</span>{/if}
        {#if finished && tr.elapsed_ms}<span class="faint">{duration(tr.elapsed_ms)}</span>{/if}
      </div>
    </div>

    {#if !compact}
      <div class="foot">
        <span class="faint">{t('{a} of {b} files', { a: (tr.files_done || 0).toLocaleString(), b: (tr.files || 0).toLocaleString() })}</span>
        <span class="faint num">{tr.lanes?.length || 0} {t('lanes')} · {(tr.transport || 'tcp').toUpperCase()}</span>
      </div>
      {#if (app.series[tr.id]?.length || 0) > 2}
        <div class="chart"><SpeedChart samples={app.series[tr.id]} dir={tr.dir} height={46} live={tr.phase === 'running'} average={finished ? tr.average : 0} /></div>
      {/if}
    {/if}
    <div class="bar"><div style:width="{pct}%"></div></div>
  {/if}
  {#if tr.error && tr.phase !== 'done'}
    <p class="err">{tr.error}</p>
  {/if}
</article>

<style>
  .card {
    position: relative;
    padding: 14px 16px 16px;
    border-radius: var(--r-lg);
    background: var(--s1);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  .card.compact { padding: 12px 14px 14px; }
  header { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; }
  .dir {
    width: 28px; height: 28px; border-radius: 9px; display: grid; place-items: center; flex: none;
    background: color-mix(in srgb, var(--c) 16%, transparent); color: var(--c);
  }
  .what { min-width: 0; flex: 1; }
  .title { font-weight: 640; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; letter-spacing: -0.01em; }
  .status { font-size: 12.5px; color: var(--ink-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .status.bad { color: var(--bad); }
  .actions { display: flex; gap: 2px; flex: none; }

  .numbers { display: flex; align-items: flex-end; justify-content: space-between; gap: 12px; margin-top: 12px; }
  .speed { display: flex; align-items: baseline; gap: 6px; }
  .big { font-size: 26px; font-weight: 500; line-height: 1; color: var(--ink); }
  .compact .big { font-size: 21px; }
  .unit { font-size: 12px; color: var(--ink-2); }
  .small { font-size: 12px; }
  .meta { display: flex; flex-direction: column; align-items: flex-end; font-size: 11.5px; gap: 2px; color: var(--ink-2); }
  .foot { display: flex; justify-content: space-between; font-size: 12px; margin-top: 10px; }
  .chart { margin-top: 8px; }
  .bar { position: absolute; left: 0; right: 0; bottom: 0; height: 3px; background: transparent; }
  .bar > div { height: 100%; background: var(--c); transition: width 0.3s var(--ease-out); }
  .finished .bar { opacity: 0; transition: opacity 1s 0.6s; }
  .msg { margin: 0; padding: 10px 12px; border-radius: var(--r-md); background: var(--s3); white-space: pre-wrap; word-break: break-word; font-size: 14px; max-height: 120px; overflow: auto; }
  .err { margin: 10px 0 0; font-size: 12.5px; color: var(--bad); word-break: break-word; }
</style>
