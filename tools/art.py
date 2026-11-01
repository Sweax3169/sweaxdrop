"""README / site artwork for SweaxDrop as animated SVGs (dark + light, en + tr).

    python tools/art.py docs/art

Fonts (Bricolage Grotesque, Martian Mono; SIL OFL) are subset to the glyphs
each file uses and embedded, so GitHub's image proxy renders them.
Benchmark charts read bench/results/*.json; they draw only measured runs.
"""
import base64
import io
import json
import os
import sys

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FONTS = os.path.join(ROOT, 'ui', 'node_modules', '@fontsource-variable')

T = {
    'dark': dict(bg='#0c0e12', bg2='#11141a', s1='#151920', s2='#1b2029', s3='#262c37', ink='#eef0f4', ink2='#a9b0bd', ink3='#6d7584',
                 send='#6f86ff', recv='#2fd3ad', line='rgba(255,255,255,.07)', shadow='.55', dev='#e9ecf2', screen='#1b2029'),
    'light': dict(bg='#eef0ec', bg2='#e4e7e1', s1='#ffffff', s2='#f6f7f4', s3='#e3e6e0', ink='#12151a', ink2='#474e5b', ink3='#848b97',
                  send='#3553ff', recv='#009b7d', line='rgba(18,21,26,.08)', shadow='.16', dev='#12151a', screen='#f6f7f4'),
}

L = {
    'en': dict(
        kicker='Wireless file transfer · Windows · Android · iPhone',
        tag1='Your files, on', tag2='parallel lanes.',
        sub='Photos, videos, folders and text between your PC and your phones,',
        sub2='over your own Wi-Fi. Encrypted end to end, resumes where it stopped.',
        facts='TLS 1.3 by key  ·  BLAKE3 per chunk  ·  No cloud  ·  No account',
        sending='Sending to Pixel 8', file='Holiday cut 4K.mp4', lanes='4 lanes · TCP', left='31 s left',
        arch_title='One core, every platform', arch_core='SweaxDrop core (Rust)',
        arch_items=['Discovery: multicast + broadcast + mDNS', 'TLS 1.3, mutual, pinned by key', 'Parallel lanes, 4 MiB chunks, BLAKE3', 'Resume, pause, queue, history', 'SPAKE2 PIN and QR pairing', 'Browser client over HTTP'],
        arch_hosts=[('Windows', 'Tauri · WebView2', 'tray, Send to, drag and drop'), ('Android', 'Kotlin · JNI', 'share sheet, MediaStore, gallery'), ('iPhone', 'Swift · C ABI', 'Photos, Files, Bonjour'), ('Any browser', 'served by the PC', 'scan the QR, nothing to install')],
        arch_ui='Same UI everywhere (Svelte)',
        how_title='How one transfer moves', how_disk='disk', how_read='read + BLAKE3', how_tls='TLS 1.3', how_write='verify + write at offset',
        how_note='Each lane pulls the next chunk from one queue. Small files follow each other with no round trip; a lane that stalls holds nobody up.',
    ),
    'tr': dict(
        kicker='Kablosuz dosya aktarımı · Windows · Android · iPhone',
        tag1='Dosyaların', tag2='paralel şeritlerde.',
        sub='Fotoğraf, video, klasör ve metin; bilgisayarınla telefonların arasında,',
        sub2='kendi Wi-Fi’ın üzerinden. Uçtan uca şifreli, koptuğu yerden sürer.',
        facts='Anahtarla TLS 1.3  ·  Parça başına BLAKE3  ·  Bulut yok  ·  Hesap yok',
        sending='Pixel 8 cihazına gönderiliyor', file='Tatil kurgusu 4K.mp4', lanes='4 şerit · TCP', left='31 sn kaldı',
        arch_title='Tek çekirdek, her platform', arch_core='SweaxDrop çekirdeği (Rust)',
        arch_items=['Keşif: multicast + yayın + mDNS', 'Karşılıklı TLS 1.3, anahtarla sabitlenir', 'Paralel şerit, 4 MiB parça, BLAKE3', 'Kaldığı yerden devam, duraklat, sıra, geçmiş', 'SPAKE2 PIN ve QR eşleştirme', 'HTTP üzerinden tarayıcı istemcisi'],
        arch_hosts=[('Windows', 'Tauri · WebView2', 'tepsi, Gönder menüsü, sürükle-bırak'), ('Android', 'Kotlin · JNI', 'paylaş menüsü, MediaStore, galeri'), ('iPhone', 'Swift · C ABI', 'Fotoğraflar, Dosyalar, Bonjour'), ('Herhangi bir tarayıcı', 'PC sunar', 'QR’ı okut, kurulum yok')],
        arch_ui='Her yerde aynı arayüz (Svelte)',
        how_title='Bir aktarım nasıl akar', how_disk='disk', how_read='oku + BLAKE3', how_tls='TLS 1.3', how_write='doğrula + yerine yaz',
        how_note='Her şerit sıradaki parçayı tek kuyruktan alır. Küçük dosyalar beklemeden peş peşe gider; takılan şerit kimseyi bekletmez.',
    ),
}

_font_cache = {}


def font_face(family, file, weight, text, wdth=None):
    """Static instance of a variable font, subset to `text`, as @font-face."""
    key = (file, weight, wdth, ''.join(sorted(set(text))))
    if key in _font_cache:
        return _font_cache[key]
    f = TTFont(os.path.join(FONTS, file))
    axes = {'wght': weight}
    if 'opsz' in [a.axisTag for a in f['fvar'].axes]:
        axes['opsz'] = 48
    if wdth and 'wdth' in [a.axisTag for a in f['fvar'].axes]:
        axes['wdth'] = wdth
    for a in f['fvar'].axes:
        if a.axisTag not in axes:
            axes[a.axisTag] = a.defaultValue
    inst = instancer.instantiateVariableFont(f, axes)
    opts = subset.Options()
    opts.flavor = 'woff2'
    opts.layout_features = ['kern', 'liga']
    s = subset.Subsetter(opts)
    s.populate(text=text + ' ')
    s.subset(inst)
    buf = io.BytesIO()
    inst.flavor = 'woff2'
    inst.save(buf)
    b64 = base64.b64encode(buf.getvalue()).decode()
    css = f"@font-face{{font-family:'{family}';font-weight:{weight};src:url(data:font/woff2;base64,{b64}) format('woff2')}}"
    _font_cache[key] = css
    return css


def fonts_for(texts_by_style):
    css = []
    for (family, file, weight, wdth), text in texts_by_style.items():
        css.append(font_face(family, file, weight, text, wdth))
    return '\n'.join(css)


BRIC = 'bricolage-grotesque/files/bricolage-grotesque-latin-ext-standard-normal.woff2'
BRIC_L = 'bricolage-grotesque/files/bricolage-grotesque-latin-standard-normal.woff2'
MONO = 'martian-mono/files/martian-mono-latin-standard-normal.woff2'


def bric_file(text):
    return BRIC if any(ord(c) > 0x7e and c not in '·’—…' for c in text) else BRIC_L


def esc(s):
    return s.replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;')


def lanes_svg(x, y, w, n, c, color, speed=2.2, h=7, gap=7, delay=0.0):
    """Animated lanes: tracks with blocks flowing left to right."""
    out = []
    for i in range(n):
        yy = y + i * (h + gap)
        out.append(f'<rect x="{x}" y="{yy}" width="{w}" height="{h}" rx="{h/2}" fill="{c["s3"]}"/>')
        dur = speed * (1 + (0.12 if i == n - 1 else 0.0) - i * 0.03)
        out.append(f'<g clip-path="url(#lc{x}_{y})"><g>')
        for k in range(-2, int(w / 46) + 2):
            out.append(f'<rect x="{x + k*46}" y="{yy}" width="22" height="{h}" rx="{h/2}" fill="{color}" opacity=".9"/>')
        out.append(f'<animateTransform attributeName="transform" type="translate" from="0 0" to="46 0" dur="{dur/ (w/46):.3f}s" repeatCount="indefinite" begin="{delay + i*0.07:.2f}s"/></g></g>')
    clip = f'<clipPath id="lc{x}_{y}"><rect x="{x}" y="{y}" width="{w}" height="{n*(h+gap)}" rx="{h/2}"/></clipPath>'
    return clip + '\n'.join(out)


def phone(x, y, s, c, fill_color=None):
    return f'''<g transform="translate({x} {y}) scale({s})">
  <rect x="0" y="0" width="120" height="236" rx="26" fill="{c['dev']}"/>
  <rect x="9" y="12" width="102" height="198" rx="17" fill="{c['screen']}"/>
  {'' if not fill_color else f'<rect x="9" y="12" width="102" height="198" rx="17" fill="{fill_color}" opacity=".18"/>'}
  <rect x="44" y="219" width="32" height="6" rx="3" fill="{c['screen']}" opacity=".5"/>
</g>'''


def laptop(x, y, s, c):
    return f'''<g transform="translate({x} {y}) scale({s})">
  <rect x="20" y="0" width="300" height="196" rx="16" fill="{c['dev']}"/>
  <rect x="32" y="12" width="276" height="170" rx="8" fill="{c['screen']}"/>
  <path d="M0 206h340l-14 18a10 10 0 0 1-8 4H22a10 10 0 0 1-8-4z" fill="{c['dev']}"/>
</g>'''


def logo(x, y, s, c):
    return f'''<g transform="translate({x} {y}) scale({s})">
  <rect width="32" height="32" rx="9" fill="{c['ink']}"/>
  <rect x="6" y="9.2" width="12" height="3" rx="1.5" fill="{c['bg']}" opacity=".55"/>
  <rect x="6" y="14.5" width="14.5" height="3" rx="1.5" fill="{c['bg']}"/>
  <rect x="6" y="19.8" width="9" height="3" rx="1.5" fill="{c['bg']}" opacity=".55"/>
  <circle cx="24.2" cy="16" r="2.9" fill="{c['send']}"/>
</g>'''


def banner(theme, lang):
    c, t = T[theme], L[lang]
    W, H = 1280, 600
    texts = {
        ('Bric', bric_file(t['tag1'] + t['tag2'] + 'SweaxDrop'), 760, None): t['tag1'] + t['tag2'] + 'SweaxDrop',
        ('Bric', bric_file(t['kicker'] + t['sub'] + t['sub2'] + t['sending'] + t['file']), 470, None): t['kicker'] + t['sub'] + t['sub2'] + t['sending'] + t['file'] + t['lanes'] + t['left'],
        ('Mono', MONO, 420, 87.5): t['facts'] + '0123456789.,/ MBsGlaneTCP',
    }
    css = fonts_for(texts)
    card_x, card_y = 700, 300
    body = f'''<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" role="img" aria-label="SweaxDrop">
<style>{css}
.t{{font-family:Bric,sans-serif}} .m{{font-family:Mono,monospace}}
@keyframes rise{{from{{opacity:0;transform:translateY(14px)}}to{{opacity:1;transform:none}}}}
@keyframes pulse{{0%,100%{{opacity:.35}}50%{{opacity:1}}}}
@keyframes count{{0%{{opacity:1}}}}
.r1{{animation:rise .9s cubic-bezier(.2,.8,.2,1) both}} .r2{{animation:rise .9s .12s cubic-bezier(.2,.8,.2,1) both}} .r3{{animation:rise .9s .24s cubic-bezier(.2,.8,.2,1) both}} .r4{{animation:rise .9s .36s cubic-bezier(.2,.8,.2,1) both}}
.blink{{animation:pulse 1.6s ease-in-out infinite}}
</style>
<defs>
  <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{c['bg2']}"/><stop offset="1" stop-color="{c['bg']}"/></linearGradient>
  <radialGradient id="glow"><stop offset="0" stop-color="{c['send']}" stop-opacity=".22"/><stop offset="1" stop-color="{c['send']}" stop-opacity="0"/></radialGradient>
  <filter id="sh" x="-20%" y="-20%" width="140%" height="160%"><feGaussianBlur in="SourceAlpha" stdDeviation="18"/><feOffset dy="18"/><feComponentTransfer><feFuncA type="linear" slope="{c['shadow']}"/></feComponentTransfer><feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge></filter>
</defs>
<rect width="{W}" height="{H}" rx="28" fill="url(#bg)"/>
<circle cx="980" cy="250" r="360" fill="url(#glow)"/>
<g class="r1">{logo(72, 70, 1.25, c)}<text x="122" y="102" class="t" font-size="27" font-weight="760" fill="{c['ink']}" letter-spacing="-.5">SweaxDrop</text></g>
<text x="72" y="168" class="t r1" font-size="17" font-weight="470" fill="{c['ink3']}">{esc(t['kicker'])}</text>
<text x="70" y="244" class="t r2" font-size="64" font-weight="760" fill="{c['ink']}" letter-spacing="-2">{esc(t['tag1'])}</text>
<text x="70" y="316" class="t r2" font-size="64" font-weight="760" fill="{c['send']}" letter-spacing="-2">{esc(t['tag2'])}</text>
<text x="72" y="376" class="t r3" font-size="19" font-weight="470" fill="{c['ink2']}">{esc(t['sub'])}</text>
<text x="72" y="404" class="t r3" font-size="19" font-weight="470" fill="{c['ink2']}">{esc(t['sub2'])}</text>
<text x="72" y="470" class="m r4" font-size="13.5" font-weight="420" fill="{c['ink3']}">{esc(t['facts'])}</text>
<g class="r3">
  {laptop(730, 64, 0.82, c)}
  <g transform="translate(767 84)">{lanes_svg(18, 30, 190, 4, c, c['send'], speed=2.0, h=6, gap=6)}</g>
  {phone(1072, 92, 0.78, c, c['send'])}
  <g transform="translate(1086 120)">{lanes_svg(0, 40, 66, 4, c, c['send'], speed=1.6, h=5, gap=5)}</g>
</g>
<g class="r4" filter="url(#sh)">
  <rect x="{card_x}" y="{card_y}" width="500" height="196" rx="22" fill="{c['s1']}"/>
  <rect x="{card_x+20}" y="{card_y+22}" width="30" height="30" rx="9" fill="{c['send']}" fill-opacity=".16"/>
  <path d="M{card_x+35} {card_y+44}v-14m-6 6 6-6 6 6" stroke="{c['send']}" stroke-width="2.4" fill="none" stroke-linecap="round" stroke-linejoin="round"/>
  <text x="{card_x+62}" y="{card_y+34}" class="t" font-size="16" font-weight="760" fill="{c['ink']}">{esc(t['file'])}</text>
  <text x="{card_x+62}" y="{card_y+53}" class="t" font-size="13" font-weight="470" fill="{c['ink2']}">{esc(t['sending'])}</text>
  {lanes_svg(card_x+20, card_y+72, 460, 4, c, c['send'], speed=2.4, h=7, gap=6)}
  <text x="{card_x+20}" y="{card_y+160}" class="m" font-size="34" font-weight="420" fill="{c['ink']}">96,4</text>
  <text x="{card_x+122}" y="{card_y+160}" class="m" font-size="13" font-weight="420" fill="{c['ink2']}">MB/s</text>
  <text x="{card_x+480}" y="{card_y+142}" text-anchor="end" class="m" font-size="12" font-weight="420" fill="{c['ink2']}">1,24 GB / 2,87 GB</text>
  <text x="{card_x+480}" y="{card_y+160}" text-anchor="end" class="m blink" font-size="12" font-weight="420" fill="{c['ink3']}">{esc(t['left'])} · {esc(t['lanes'])}</text>
</g>
</svg>'''
    return body


def arch(theme, lang):
    c, t = T[theme], L[lang]
    W, H = 1280, 720
    alltext = t['arch_title'] + t['arch_core'] + ''.join(t['arch_items']) + ''.join(''.join(h) for h in t['arch_hosts']) + t['arch_ui']
    css = fonts_for({('Bric', bric_file(alltext), 700, None): alltext, ('Bric', bric_file(alltext), 450, None): alltext})
    hosts = t['arch_hosts']
    cols = []
    for i, (name, tech, what) in enumerate(hosts):
        x = 70 + i * 290
        cols.append(f'''<g style="animation:rise .8s {0.15 + i*0.1:.2f}s cubic-bezier(.2,.8,.2,1) both">
  <rect x="{x}" y="120" width="260" height="128" rx="20" fill="{c['s1']}" filter="url(#sh)"/>
  <text x="{x+22}" y="162" class="t" font-size="22" font-weight="700" fill="{c['ink']}">{esc(name)}</text>
  <text x="{x+22}" y="190" class="t" font-size="15" font-weight="450" fill="{c['send']}">{esc(tech)}</text>
  <text x="{x+22}" y="220" class="t" font-size="14" font-weight="450" fill="{c['ink3']}">{esc(what)}</text>
  <path d="M{x+130} 252 V 318" stroke="{c['ink3']}" stroke-width="2" stroke-dasharray="4 6"><animate attributeName="stroke-dashoffset" from="20" to="0" dur="1s" repeatCount="indefinite"/></path>
</g>''')
    items = []
    for i, it in enumerate(t['arch_items']):
        col, row = i % 2, i // 2
        x = 110 + col * 540
        y = 446 + row * 52
        items.append(f'''<g style="animation:rise .8s {0.5 + i*0.08:.2f}s cubic-bezier(.2,.8,.2,1) both">
  <rect x="{x}" y="{y}" width="500" height="40" rx="12" fill="{c['s2']}"/>
  <circle cx="{x+22}" cy="{y+20}" r="5" fill="{c['recv'] if i % 2 else c['send']}"/>
  <text x="{x+40}" y="{y+26}" class="t" font-size="16" font-weight="450" fill="{c['ink']}">{esc(it)}</text>
</g>''')
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" role="img" aria-label="{esc(t['arch_title'])}">
<style>{css}
.t{{font-family:Bric,sans-serif}}
@keyframes rise{{from{{opacity:0;transform:translateY(12px)}}to{{opacity:1;transform:none}}}}
</style>
<defs><filter id="sh" x="-20%" y="-20%" width="140%" height="160%"><feGaussianBlur in="SourceAlpha" stdDeviation="12"/><feOffset dy="10"/><feComponentTransfer><feFuncA type="linear" slope="{c['shadow']}"/></feComponentTransfer><feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge></filter></defs>
<rect width="{W}" height="{H}" rx="28" fill="{c['bg']}"/>
<text x="70" y="78" class="t" font-size="30" font-weight="700" fill="{c['ink']}" letter-spacing="-.6">{esc(t['arch_title'])}</text>
<text x="{W-70}" y="78" text-anchor="end" class="t" font-size="16" font-weight="450" fill="{c['ink3']}">{esc(t['arch_ui'])}</text>
{''.join(cols)}
<g style="animation:rise .8s .4s cubic-bezier(.2,.8,.2,1) both">
  <rect x="70" y="322" width="{W-140}" height="356" rx="26" fill="{c['s1']}" filter="url(#sh)"/>
  <g transform="translate(110 356)">{logo(0, 0, 1.2, c)}</g>
  <text x="162" y="388" class="t" font-size="24" font-weight="700" fill="{c['ink']}">{esc(t['arch_core'])}</text>
  <g transform="translate(0 0)">{lanes_svg(760, 362, 400, 3, c, c['send'], speed=2.2, h=6, gap=6)}</g>
</g>
{''.join(items)}
</svg>'''


def how(theme, lang):
    c, t = T[theme], L[lang]
    W, H = 1280, 520
    alltext = t['how_title'] + t['how_disk'] + t['how_read'] + t['how_tls'] + t['how_write'] + t['how_note'] + 'Sender Receiver Gönderen Alıcı 4 MiB'
    css = fonts_for({('Bric', bric_file(alltext), 700, None): alltext, ('Bric', bric_file(alltext), 450, None): alltext})
    snd = 'Gönderen' if lang == 'tr' else 'Sender'
    rcv = 'Alıcı' if lang == 'tr' else 'Receiver'
    rows = []
    for i in range(4):
        y = 150 + i * 62
        rows.append(f'''<rect x="230" y="{y}" width="190" height="44" rx="12" fill="{c['s2']}"/>
<text x="325" y="{y+28}" text-anchor="middle" class="t" font-size="15" font-weight="450" fill="{c['ink2']}">{esc(t['how_read'])}</text>
<rect x="860" y="{y}" width="230" height="44" rx="12" fill="{c['s2']}"/>
<text x="975" y="{y+28}" text-anchor="middle" class="t" font-size="15" font-weight="450" fill="{c['ink2']}">{esc(t['how_write'])}</text>
<g transform="translate(0 0)">{lanes_svg(440, y+15, 400, 1, c, c['send'] if i % 2 == 0 else c['recv'], speed=1.6 + i*0.25, h=14, gap=0)}</g>''')
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" role="img" aria-label="{esc(t['how_title'])}">
<style>{css}.t{{font-family:Bric,sans-serif}}</style>
<rect width="{W}" height="{H}" rx="28" fill="{c['bg']}"/>
<text x="70" y="78" class="t" font-size="30" font-weight="700" fill="{c['ink']}" letter-spacing="-.6">{esc(t['how_title'])}</text>
<text x="70" y="118" class="t" font-size="16" font-weight="450" fill="{c['ink3']}">{esc(snd)}</text>
<text x="{W-70}" y="118" text-anchor="end" class="t" font-size="16" font-weight="450" fill="{c['ink3']}">{esc(rcv)}</text>
<rect x="70" y="150" width="130" height="230" rx="18" fill="{c['s1']}"/><text x="135" y="272" text-anchor="middle" class="t" font-size="18" font-weight="700" fill="{c['ink']}">{esc(t['how_disk'])}</text>
<rect x="1110" y="150" width="100" height="230" rx="18" fill="{c['s1']}"/><text x="1160" y="272" text-anchor="middle" class="t" font-size="18" font-weight="700" fill="{c['ink']}">{esc(t['how_disk'])}</text>
<text x="640" y="138" text-anchor="middle" class="t" font-size="15" font-weight="700" fill="{c['ink2']}">{esc(t['how_tls'])} · 4 MiB</text>
{''.join(rows)}
<text x="70" y="440" class="t" font-size="17" font-weight="450" fill="{c['ink2']}">{esc(t['how_note'])}</text>
</svg>'''


def main(out):
    os.makedirs(out, exist_ok=True)
    for theme in T:
        for lang in L:
            for name, fn in (('banner', banner), ('architecture', arch), ('lanes', how)):
                with open(os.path.join(out, f'{name}-{lang}-{theme}.svg'), 'w', encoding='utf-8', newline='\n') as f:
                    f.write(fn(theme, lang))
    print('art written to', out)


if __name__ == '__main__':
    main(sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, 'docs', 'art'))
