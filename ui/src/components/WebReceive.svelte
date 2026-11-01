<script>
  // Browser client: what the computer sent, ready to download. Photos show
  // inline so a long press offers "Save to Photos" (iOS Safari over plain
  // HTTP has no other way into the photo library).
  import { getBridge } from '../lib/state.svelte.js';
  import { t } from '../lib/i18n.js';
  import { bytes, fileName, kindOf } from '../lib/format.js';
  import Sheet from './Sheet.svelte';
  import Icon from './Icon.svelte';

  let { offer, onclose } = $props();
  const b = getBridge();
  const files = $derived(offer.files || []);
  const images = $derived(files.filter((f) => kindOf(f.path) === 'image'));
  const s = () => encodeURIComponent(b.session?.() || '');
  const ios = /iPhone|iPad/.test(navigator.userAgent);
</script>

<Sheet {onclose} width={520} label={t('The computer sent you')}>
  <h2>{t('The computer sent you')}</h2>
  {#if offer.text}<p class="msg">{offer.text}</p>{/if}

  {#if images.length}
    <div class="grid">
      {#each images.slice(0, 60) as f (f.id)}
        <img src="{f.url}?inline=1&s={s()}" alt={fileName(f.path)} loading="lazy" />
      {/each}
    </div>
    {#if ios}<p class="faint small">{t('Touch and hold a photo, then choose Save to Photos.')}</p>{/if}
  {/if}

  <ul>
    {#each files as f (f.id)}
      <li>
        <span class="nm" title={f.path}>{fileName(f.path)}</span>
        <span class="faint num sz">{bytes(f.size)}</span>
        <button class="btn small" onclick={() => b.dl(f.url, fileName(f.path))}><Icon name="download" size={15} />{t('Download')}</button>
      </li>
    {/each}
  </ul>
  {#if ios}<p class="faint small">{t('Downloads go to the Files app.')}</p>{/if}

  <div class="buttons">
    <button class="btn" onclick={onclose}>{t('Close')}</button>
    {#if files.length > 1}
      <button class="btn recv" onclick={() => b.dl(offer.zip, 'SweaxDrop.zip')}><Icon name="download" size={17} />{t('Download all')}</button>
    {/if}
  </div>
</Sheet>

<style>
  h2 { margin: 0 0 12px; font-size: 20px; }
  .msg { padding: 10px 12px; border-radius: var(--r-md); background: var(--s3); white-space: pre-wrap; }
  .grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 4px; margin-bottom: 8px; }
  img { width: 100%; aspect-ratio: 1; object-fit: cover; border-radius: 8px; background: var(--s3); -webkit-touch-callout: default; }
  ul { list-style: none; margin: 8px 0; padding: 0; display: flex; flex-direction: column; gap: 4px; max-height: 260px; overflow: auto; }
  li { display: flex; align-items: center; gap: 10px; padding: 6px 4px; }
  .nm { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 14px; }
  .sz { font-size: 11.5px; }
  .small { font-size: 12.5px; margin: 4px 0; }
  .buttons { display: flex; justify-content: flex-end; gap: 10px; margin-top: 12px; }
</style>
