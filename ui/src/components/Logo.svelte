<script>
  // The mark: three lanes, the middle one carrying the drop out in front.
  let { size = 28, animated = false } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 32 32" class:animated aria-label="SweaxDrop">
  <rect width="32" height="32" rx="9" fill="var(--ink)" />
  <rect class="l l1" x="6" y="9.2" width="12" height="3" rx="1.5" fill="var(--bg)" opacity="0.55" />
  <rect class="l l2" x="6" y="14.5" width="14.5" height="3" rx="1.5" fill="var(--bg)" />
  <rect class="l l3" x="6" y="19.8" width="9" height="3" rx="1.5" fill="var(--bg)" opacity="0.55" />
  <circle class="d" cx="24.2" cy="16" r="2.9" fill="var(--send)" />
</svg>

<style>
  svg { display: block; flex: none; }
  .animated .l { animation: lane 2.4s var(--spring-soft) infinite; transform-box: fill-box; transform-origin: left center; }
  .animated .l1 { animation-delay: 0.12s; }
  .animated .l3 { animation-delay: 0.24s; }
  .animated .d { animation: drop 2.4s var(--spring) infinite; }
  @keyframes lane { 0%, 60%, 100% { transform: scaleX(1); } 30% { transform: scaleX(0.72); } }
  @keyframes drop { 0%, 60%, 100% { transform: translateX(0); } 30% { transform: translateX(-3px); } }
</style>
