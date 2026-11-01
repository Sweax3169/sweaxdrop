<script>
  // The live picture of a transfer: one track per parallel stream, chunks
  // flowing along it as fast as that stream really moves bytes. The speed of
  // each track comes from its own byte counter, so a lagging lane is visible.
  import { onMount } from 'svelte';

  let { lanes = [], phase = 'running', dir = 'send', height = 44, total = 0, bytes = 0 } = $props();

  let canvas;
  let wrap;
  let width = $state(300);

  const state = { rates: [], last: [], lastT: 0, offsets: [], glow: 0, done: 0 };

  function color(name) {
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || '#3553ff';
  }

  onMount(() => {
    const ro = new ResizeObserver(() => (width = wrap.clientWidth));
    ro.observe(wrap);
    let raf;
    let colors = {};
    const readColors = () => {
      colors = { accent: color(dir === 'send' ? '--send' : '--recv'), track: color('--s3'), ink: color('--ink-3') };
    };
    readColors();
    const mo = new MutationObserver(readColors);
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-palette'] });

    const frame = (now) => {
      raf = requestAnimationFrame(frame);
      const ctx = canvas.getContext('2d');
      const dpr = window.devicePixelRatio || 1;
      const w = width;
      const h = height;
      if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
        canvas.width = Math.round(w * dpr);
        canvas.height = Math.round(h * dpr);
      }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);

      const n = Math.max(1, lanes.length || 1);
      const dt = state.lastT ? Math.min(0.1, (now - state.lastT) / 1000) : 0;
      state.lastT = now;

      // Per-lane rate from the counters (smoothed so a 200 ms update cadence
      // does not make the flow stutter).
      for (let i = 0; i < n; i++) {
        const v = lanes[i] || 0;
        if (state.last[i] === undefined) state.last[i] = { v, t: now };
        if (v !== state.last[i].v) {
          const r = ((v - state.last[i].v) / Math.max(1, now - state.last[i].t)) * 1000;
          state.rates[i] = (state.rates[i] || 0) * 0.6 + Math.max(0, r) * 0.4;
          state.last[i] = { v, t: now };
        } else if (now - state.last[i].t > 700) {
          state.rates[i] = (state.rates[i] || 0) * 0.9;
        }
      }
      const moving = phase === 'running';
      const finished = phase === 'done';
      state.glow += ((moving ? 1 : 0) - state.glow) * Math.min(1, dt * 6);
      state.done += ((finished ? 1 : 0) - state.done) * Math.min(1, dt * 4);

      const gap = 5;
      const laneH = Math.max(3, Math.min(9, (h - gap * (n - 1)) / n));
      const usedH = laneH * n + gap * (n - 1);
      const top = (h - usedH) / 2;
      const peak = Math.max(1, ...state.rates.slice(0, n));
      const progress = total > 0 ? Math.min(1, bytes / total) : 0;

      for (let i = 0; i < n; i++) {
        const y = top + i * (laneH + gap);
        const r = laneH / 2;
        // track
        ctx.globalAlpha = 1;
        ctx.fillStyle = colors.track;
        roundRect(ctx, 0, y, w, laneH, r);
        ctx.fill();

        // progress tint under the flow
        ctx.globalAlpha = 0.18 + 0.82 * state.done;
        ctx.fillStyle = colors.accent;
        roundRect(ctx, 0, y, Math.max(laneH, w * (finished ? 1 : progress)), laneH, r);
        ctx.fill();

        if (state.done > 0.98) continue;
        // flowing chunks: speed ∝ this lane's share of the fastest lane
        const rate = state.rates[i] || 0;
        const rel = moving ? rate / peak : 0;
        const pxPerSec = 40 + 260 * Math.min(1, rate / 40e6) * (0.55 + 0.45 * rel);
        state.offsets[i] = ((state.offsets[i] || i * 23) + pxPerSec * dt * (moving ? 1 : 0)) % 46;
        const block = 22;
        const period = 46;
        ctx.save();
        roundRect(ctx, 0, y, w, laneH, r);
        ctx.clip();
        for (let x = -period + state.offsets[i]; x < w; x += period) {
          const fade = Math.min(1, Math.max(0, x / (w * 0.16))) * Math.min(1, Math.max(0, (w - x) / (w * 0.08)));
          ctx.globalAlpha = (0.25 + 0.75 * state.glow) * (0.35 + 0.65 * fade) * (1 - state.done);
          ctx.fillStyle = colors.accent;
          roundRect(ctx, x, y, block, laneH, r);
          ctx.fill();
        }
        ctx.restore();
      }
      ctx.globalAlpha = 1;
    };
    raf = requestAnimationFrame(frame);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      mo.disconnect();
    };
  });

  function roundRect(ctx, x, y, w, h, r) {
    ctx.beginPath();
    const rr = Math.min(r, w / 2, h / 2);
    ctx.moveTo(x + rr, y);
    ctx.arcTo(x + w, y, x + w, y + h, rr);
    ctx.arcTo(x + w, y + h, x, y + h, rr);
    ctx.arcTo(x, y + h, x, y, rr);
    ctx.arcTo(x, y, x + w, y, rr);
    ctx.closePath();
  }
</script>

<div class="lanes" bind:this={wrap} style:height="{height}px">
  <canvas bind:this={canvas} style:width="{width}px" style:height="{height}px"></canvas>
</div>

<style>
  .lanes { width: 100%; position: relative; }
  canvas { display: block; }
</style>
