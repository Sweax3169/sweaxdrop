<script>
  // Speed over time as a filled curve; the newest point carries a dot.
  import { speed as fmtSpeed } from '../lib/format.js';

  let { samples = [], dir = 'send', height = 64, showAxis = false, average = 0, peak = 0, live = false } = $props();

  let w = $state(300);
  const id = 'g' + Math.random().toString(36).slice(2, 8);

  const geo = $derived.by(() => {
    const pts = samples.filter((s) => Number.isFinite(s.bps));
    if (pts.length < 2) return null;
    const t0 = pts[0].t;
    const t1 = Math.max(pts[pts.length - 1].t, t0 + 1);
    const maxV = Math.max(...pts.map((p) => p.bps), 1) * 1.15;
    const pad = 3;
    const xy = pts.map((p) => [((p.t - t0) / (t1 - t0)) * (w - pad * 2) + pad, height - pad - (p.bps / maxV) * (height - pad * 2)]);
    // monotone smoothing (Fritsch–Carlson style tangents)
    let d = `M${xy[0][0].toFixed(1)},${xy[0][1].toFixed(1)}`;
    for (let i = 1; i < xy.length; i++) {
      const [x0, y0] = xy[i - 1];
      const [x1, y1] = xy[i];
      const cx = (x0 + x1) / 2;
      d += ` C${cx.toFixed(1)},${y0.toFixed(1)} ${cx.toFixed(1)},${y1.toFixed(1)} ${x1.toFixed(1)},${y1.toFixed(1)}`;
    }
    const area = `${d} L${xy[xy.length - 1][0].toFixed(1)},${height} L${xy[0][0].toFixed(1)},${height} Z`;
    const avgY = average > 0 ? height - pad - (average / maxV) * (height - pad * 2) : null;
    return { d, area, last: xy[xy.length - 1], avgY, maxV };
  });
</script>

<div class="chart" bind:clientWidth={w} style:height="{height}px" style:--c={dir === 'send' ? 'var(--send)' : 'var(--recv)'}>
  {#if geo}
    <svg width={w} {height} viewBox="0 0 {w} {height}">
      <defs>
        <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stop-color="var(--c)" stop-opacity="0.32" />
          <stop offset="1" stop-color="var(--c)" stop-opacity="0" />
        </linearGradient>
      </defs>
      <path d={geo.area} fill="url(#{id})" />
      <path d={geo.d} fill="none" stroke="var(--c)" stroke-width="2" stroke-linejoin="round" />
      {#if geo.avgY !== null}
        <line x1="0" x2={w} y1={geo.avgY} y2={geo.avgY} stroke="var(--ink-3)" stroke-width="1" stroke-dasharray="2 4" />
      {/if}
      {#if live}
        <circle cx={geo.last[0]} cy={geo.last[1]} r="6" fill="var(--c)" opacity="0.18" class="pulse" />
        <circle cx={geo.last[0]} cy={geo.last[1]} r="3" fill="var(--c)" />
      {/if}
    </svg>
    {#if showAxis}
      <span class="axis num">{fmtSpeed(geo.maxV / 1.15)}</span>
    {/if}
  {:else}
    <div class="empty"></div>
  {/if}
</div>

<style>
  .chart { position: relative; width: 100%; }
  svg { display: block; overflow: visible; }
  .axis { position: absolute; top: -2px; right: 0; font-size: 10.5px; color: var(--ink-3); }
  .empty { height: 100%; border-radius: 8px; background: repeating-linear-gradient(90deg, var(--s3) 0 2px, transparent 2px 8px); opacity: 0.4; }
  .pulse { animation: pulse 1.6s ease-out infinite; transform-box: fill-box; transform-origin: center; }
  @keyframes pulse { 0% { transform: scale(0.6); opacity: 0.4; } 100% { transform: scale(2.2); opacity: 0; } }
</style>
