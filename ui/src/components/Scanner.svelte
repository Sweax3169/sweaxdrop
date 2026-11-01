<script>
  // Camera QR reader (phones): getUserMedia + jsQR, no native code needed.
  import { onDestroy, onMount } from 'svelte';
  import jsQR from 'jsqr';
  import { t } from '../lib/i18n.js';

  let { onresult, oncancel } = $props();
  let video;
  let error = $state('');
  let stream;
  let raf;
  const canvas = document.createElement('canvas');
  const ctx = canvas.getContext('2d', { willReadFrequently: true });

  onMount(async () => {
    try {
      stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: 'environment', width: { ideal: 1280 } }, audio: false });
      video.srcObject = stream;
      await video.play();
      tick();
    } catch (e) {
      error = t('Camera not available');
    }
  });

  function tick() {
    raf = requestAnimationFrame(tick);
    if (!video || video.readyState < 2) return;
    const w = 480;
    const h = Math.round((video.videoHeight / video.videoWidth) * w) || 360;
    canvas.width = w;
    canvas.height = h;
    ctx.drawImage(video, 0, 0, w, h);
    const img = ctx.getImageData(0, 0, w, h);
    const code = jsQR(img.data, w, h, { inversionAttempts: 'dontInvert' });
    if (code?.data) {
      stop();
      onresult(code.data);
    }
  }

  function stop() {
    cancelAnimationFrame(raf);
    stream?.getTracks().forEach((tr) => tr.stop());
  }
  onDestroy(stop);
</script>

<div class="scan">
  {#if error}
    <p class="err">{error}</p>
  {:else}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video bind:this={video} playsinline muted></video>
    <div class="frame" aria-hidden="true"><span></span></div>
    <p class="faint">{t('Point the camera at the code on the other screen.')}</p>
  {/if}
  <button class="btn" onclick={() => { stop(); oncancel(); }}>{t('Cancel')}</button>
</div>

<style>
  .scan { position: relative; display: flex; flex-direction: column; gap: 12px; align-items: stretch; }
  video { width: 100%; aspect-ratio: 1; object-fit: cover; border-radius: var(--r-lg); background: #000; }
  .frame { position: absolute; left: 50%; top: calc(50% - 40px); width: 58%; aspect-ratio: 1; translate: -50% -50%; border-radius: 22px; box-shadow: 0 0 0 9999px rgba(0, 0, 0, 0.25); pointer-events: none; }
  .frame span { position: absolute; inset: 0; border-radius: 22px; border: 3px solid #fff; animation: breathe 1.8s var(--ease-out) infinite; }
  @keyframes breathe { 50% { transform: scale(0.94); opacity: 0.7; } }
  p { margin: 0; text-align: center; font-size: 14px; }
  .err { color: var(--bad); padding: 30px 0; }
</style>
