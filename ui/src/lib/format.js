import { getLang, t } from './i18n.js';

const units = ['B', 'KB', 'MB', 'GB', 'TB'];

/** Decimal units (what file managers on phones and macOS show). */
export function bytes(n, digits = 1) {
  if (!Number.isFinite(n) || n <= 0) return '0 B';
  let i = 0;
  let v = n;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  const d = i === 0 ? 0 : v >= 100 ? 0 : v >= 10 ? Math.min(1, digits) : digits;
  return `${v.toLocaleString(getLang(), { minimumFractionDigits: d, maximumFractionDigits: d })} ${units[i]}`;
}

/** Splits a speed into number and unit, so the unit can be styled apart. */
export function speedParts(bps) {
  if (!Number.isFinite(bps) || bps <= 0) return ['0', 'MB/s'];
  const mb = bps / 1e6;
  if (mb >= 1000) return [(mb / 1000).toLocaleString(getLang(), { maximumFractionDigits: 2, minimumFractionDigits: 2 }), 'GB/s'];
  if (mb >= 1) return [mb.toLocaleString(getLang(), { maximumFractionDigits: mb >= 100 ? 0 : 1, minimumFractionDigits: mb >= 100 ? 0 : 1 }), 'MB/s'];
  return [(bps / 1e3).toLocaleString(getLang(), { maximumFractionDigits: 0 }), 'KB/s'];
}

export function speed(bps) {
  const [n, u] = speedParts(bps);
  return `${n} ${u}`;
}

export function duration(ms) {
  if (!Number.isFinite(ms) || ms < 0) return '—';
  const s = Math.round(ms / 1000);
  if (s < 1) return `${(ms / 1000).toFixed(1)} s`;
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}:${String(s % 60).padStart(2, '0')}`;
  return `${Math.floor(m / 60)}:${String(m % 60).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
}

export function ago(ms) {
  const d = Date.now() - ms;
  if (d < 60_000) return t('just now');
  if (d < 3_600_000) return t('{n} min ago', { n: Math.round(d / 60_000) });
  if (d < 86_400_000) return t('{n} h ago', { n: Math.round(d / 3_600_000) });
  if (d < 2 * 86_400_000) return t('yesterday');
  return new Date(ms).toLocaleDateString(getLang(), { day: 'numeric', month: 'short' });
}

export function fileName(path) {
  return String(path || '').split(/[\\/]/).pop();
}

export function filesLabel(n) {
  return n === 1 ? t('1 file') : t('{n} files', { n: n.toLocaleString(getLang()) });
}

const kinds = [
  ['image', /\.(jpe?g|png|gif|webp|heic|heif|avif|bmp|tiff?|dng|raw|svg)$/i],
  ['video', /\.(mp4|mov|m4v|mkv|webm|avi|3gp)$/i],
  ['audio', /\.(mp3|m4a|aac|wav|flac|ogg|opus)$/i],
  ['archive', /\.(zip|rar|7z|tar|gz|xz|apk|ipa)$/i],
  ['doc', /\.(pdf|docx?|xlsx?|pptx?|odt|txt|md|rtf|csv|json)$/i],
];

export function kindOf(name) {
  for (const [k, re] of kinds) if (re.test(name)) return k;
  return 'file';
}
