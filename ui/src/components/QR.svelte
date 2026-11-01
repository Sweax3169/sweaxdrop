<script>
  // QR code drawn as soft dots with rounded finder rings, in the ink color.
  import qrcode from 'qrcode-generator';

  let { text = '', size = 220 } = $props();

  const geo = $derived.by(() => {
    if (!text) return null;
    const qr = qrcode(0, 'M');
    qr.addData(text);
    qr.make();
    const n = qr.getModuleCount();
    const finder = (r, c) => (r < 7 && c < 7) || (r < 7 && c >= n - 7) || (r >= n - 7 && c < 7);
    let dots = '';
    for (let r = 0; r < n; r++) {
      for (let c = 0; c < n; c++) {
        if (!qr.isDark(r, c) || finder(r, c)) continue;
        dots += `M${c + 0.5},${r + 0.08}a.42.42 0 0 1 0 .84a.42.42 0 0 1 0-.84z`;
      }
    }
    const corners = [
      [0, 0],
      [n - 7, 0],
      [0, n - 7],
    ];
    return { n, dots, corners };
  });
</script>

{#if geo}
  <svg width={size} height={size} viewBox="-2 -2 {geo.n + 4} {geo.n + 4}" role="img" aria-label="QR code">
    <rect x="-2" y="-2" width={geo.n + 4} height={geo.n + 4} rx="3" fill="#fff" />
    <path d={geo.dots} fill="#12151a" />
    {#each geo.corners as [x, y]}
      <rect x={x + 0.5} y={y + 0.5} width="6" height="6" rx="1.9" fill="none" stroke="#12151a" stroke-width="1" />
      <rect x={x + 2} y={y + 2} width="3" height="3" rx="0.9" fill="#12151a" />
    {/each}
  </svg>
{/if}

<style>
  svg { display: block; border-radius: 14px; }
</style>
