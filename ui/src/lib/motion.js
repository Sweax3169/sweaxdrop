// Spring-shaped easings and transitions for Svelte. A damped oscillator
// sampled once; used by enter/exit transitions so lists settle with a
// little overshoot instead of a linear fade.

export function springEase(stiffness = 220, damping = 18, mass = 1) {
  const w0 = Math.sqrt(stiffness / mass);
  const zeta = damping / (2 * Math.sqrt(stiffness * mass));
  const wd = w0 * Math.sqrt(Math.max(0, 1 - zeta * zeta));
  // duration until the oscillation is visually settled
  const T = Math.min(2.2, 6 / (zeta * w0));
  const f = (t) => {
    const x = t * T;
    if (zeta < 1) return 1 - Math.exp(-zeta * w0 * x) * (Math.cos(wd * x) + ((zeta * w0) / wd) * Math.sin(wd * x));
    return 1 - Math.exp(-w0 * x) * (1 + w0 * x);
  };
  const end = f(1);
  const ease = (t) => f(t) / end;
  ease.duration = T * 1000;
  return ease;
}

export const pop = springEase(260, 17);
export const soft = springEase(170, 22);

/** Rises and settles: scale + lift + fade. */
export function rise(node, { delay = 0, y = 14, scale = 0.94, ease = pop } = {}) {
  return {
    delay,
    duration: Math.min(700, ease.duration),
    easing: ease,
    css: (t, u) => `opacity:${Math.min(1, t * 1.6)};transform:translateY(${u * y}px) scale(${scale + (1 - scale) * t})`,
  };
}

/** Slides in from one side (sheets, toasts). */
export function slide(node, { from = 'bottom', distance = 40, ease = soft } = {}) {
  const ax = from === 'left' ? -1 : from === 'right' ? 1 : 0;
  const ay = from === 'top' ? -1 : from === 'bottom' ? 1 : 0;
  return {
    duration: Math.min(620, ease.duration),
    easing: ease,
    css: (t, u) => `opacity:${Math.min(1, t * 2)};transform:translate(${ax * u * distance}px,${ay * u * distance}px)`,
  };
}

export function fade(node, { duration = 180 } = {}) {
  return { duration, css: (t) => `opacity:${t}` };
}
