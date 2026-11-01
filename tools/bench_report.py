"""Turns bench/results into the published numbers: docs/data/bench.json (site),
docs/art/bench-*.svg (README charts) and a Markdown table on stdout.

Only measured runs are used: SweaxDrop rows are the median of N runs as
written by sweaxdrop-bench, LocalSend rows are the median of the JSON lines
written by tools/localsend_bench.py.
"""
import json
import os
import statistics
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RES = os.path.join(ROOT, 'bench', 'results')
sys.path.insert(0, os.path.join(ROOT, 'tools'))

CASES = [
    ('4 GiB file', 'One 4 GiB file', 'Tek 4 GiB dosya', 'A long video or a disk image.', 'Uzun bir video ya da disk kalıbı.'),
    ('200 photos (723 MB)', '200 photos, 723 MB', '200 fotoğraf, 723 MB', 'A camera roll: 3–6 MB each.', 'Bir galeri: her biri 3–6 MB.'),
    ('10 000 small files (338 MB)', '10 000 small files, 338 MB', '10 000 küçük dosya, 338 MB', 'A project folder or a chat export: 1–64 KB each, in 100 folders.', 'Bir proje klasörü ya da sohbet dışa aktarımı: her biri 1–64 KB, 100 klasörde.'),
    ('300 small files', '300 small files', '300 küçük dosya', 'The same kind of files, few enough for LocalSend’s own command-line sender.', 'Aynı türde dosyalar; LocalSend’in kendi komut satırı göndericisine sığacak kadar az.'),
]


def load():
    sx_runs = {}
    p = os.path.join(RES, 'compare.json')
    if os.path.exists(p):
        for r in json.load(open(p, encoding='utf-8')):
            sx_runs.setdefault(r['case'], []).append(r)
    sx = {}
    for k, rs in sx_runs.items():
        sx[k] = {
            'mb_s': statistics.median(x['mb_s'] for x in rs),
            'seconds': statistics.median(x['seconds'] for x in rs),
            'ttfb_ms': statistics.median(x['ttfb_ms'] for x in rs),
            'runs': len(rs),
        }
    ls_cli, ls_proto = {}, {}
    p = os.path.join(RES, 'localsend.jsonl')
    if os.path.exists(p):
        for line in open(p, encoding='utf-8'):
            line = line.strip()
            if not line.startswith('{'):
                continue
            r = json.loads(line)
            if not r.get('complete'):
                continue
            (ls_cli if r['tool'] == 'localsend-cli-1.18.2' else ls_proto).setdefault(r['case'], []).append(r)
    med = lambda rs: {
        'mb_s': statistics.median(x['mb_s'] for x in rs),
        'seconds': statistics.median(x['seconds'] for x in rs),
        'ttfb_ms': statistics.median(x['ttfb_ms'] for x in rs),
        'runs': len(rs),
        'tool': rs[0]['tool'],
    }
    return sx, {k: med(v) for k, v in ls_cli.items()}, {k: med(v) for k, v in ls_proto.items()}


def main():
    sx, cli, proto = load()
    cases = []
    for key, en, tr, what, what_tr in CASES:
        if key not in sx:
            continue
        # Compare with LocalSend's faster sender (of those with 3+ runs), so the
        # ratio is never flattered by the slower one.
        options = [x for x in (cli.get(key), proto.get(key)) if x and x['runs'] >= 3]
        if not options:
            continue
        ls = min(options, key=lambda x: x['seconds'])
        s = sx[key]
        cases.append({
            'key': key, 'name': en, 'name_tr': tr, 'what': what, 'what_tr': what_tr,
            'sweaxdrop': s,
            'localsend': ls,
            'localsend_cli': cli.get(key),
            'localsend_proto': proto.get(key),
            'localsend_sender': 'official CLI' if ls is cli.get(key) else 'LocalSend v2 protocol sender (2 at a time) to the official CLI receiver',
            'ratio': ls['seconds'] / s['seconds'],
        })
    out = {
        'version': '1.0.0',
        'machine': 'AMD Ryzen 5 7600, NVMe SSD, Windows 11; both programs on this PC, same files, same drive',
        'cases': cases,
        'cross_check': {k: v for k, v in proto.items()},
    }
    os.makedirs(os.path.join(ROOT, 'docs', 'data'), exist_ok=True)
    os.makedirs(os.path.join(ROOT, 'docs', 'art'), exist_ok=True)

    def secs(x, lang='en'):
        if not x:
            return '—'
        v = f"{x['seconds']:.2f} s" if lang == 'en' else f"{x['seconds']:.2f} sn".replace('.', ',')
        if x.get('runs', 3) < 3:
            v += ' (1 run)' if lang == 'en' else ' (1 ölçüm)'
        return v

    def verdict(r, lang):
        if r >= 1.15:
            return f'**{r:.1f}× faster**' if lang == 'en' else f'**{r:.1f} kat hızlı**'.replace('.', ',')
        if r <= 0.87:
            return f'{1 / r:.1f}× slower' if lang == 'en' else f'{1 / r:.1f} kat yavaş'.replace('.', ',')
        return 'about the same' if lang == 'en' else 'yaklaşık aynı'

    tables = {}
    for lang in ('en', 'tr'):
        if lang == 'en':
            head = '| Files | SweaxDrop 1.0.0 | LocalSend 1.18.2 (CLI) | LocalSend 1.18.2 (protocol sender) | SweaxDrop is |'
        else:
            head = '| Dosyalar | SweaxDrop 1.0.0 | LocalSend 1.18.2 (CLI) | LocalSend 1.18.2 (protokol göndericisi) | SweaxDrop |'
        rows = [head, '|---|---|---|---|---|']
        for c in cases:
            name = c['name'] if lang == 'en' else c['name_tr']
            rows.append(f"| {name} | **{secs(c['sweaxdrop'], lang)}** | {secs(c['localsend_cli'], lang)} | {secs(c['localsend_proto'], lang)} | {verdict(c['ratio'], lang)} |")
        tables[lang] = '\n'.join(rows)
    out['tables'] = tables
    json.dump(out, open(os.path.join(ROOT, 'docs', 'data', 'bench.json'), 'w', encoding='utf-8'), indent=1, ensure_ascii=False)
    print(tables['en'])
    print()
    print(tables['tr'])

    # Network path (no disk): lanes, TCP or QUIC, cipher.
    net = os.path.join(RES, 'network.json')
    if os.path.exists(net):
        rows = ['| Transport | Cipher | Lanes | Throughput | CPU (both ends) |', '|---|---|---|---|---|']
        for r in json.load(open(net, encoding='utf-8')):
            st = r['settings']
            rows.append(f"| {st['transport'].upper()} | {'AES-128-GCM' if st['cipher'] == 'aes' else 'ChaCha20-Poly1305'} | {st['streams']} | {r['mb_s']:,.0f} MB/s | {r['cpu_s_per_gb']:.1f} s per GB |")
        out['network'] = '\n'.join(rows)
        print()
        print(out['network'])
        json.dump(out, open(os.path.join(ROOT, 'docs', 'data', 'bench.json'), 'w', encoding='utf-8'), indent=1, ensure_ascii=False)

    # Fill the markers in the READMEs and docs/benchmarks.md.
    def fill(path, start, end, body):
        f = os.path.join(ROOT, path)
        t = open(f, encoding='utf-8').read()
        if start in t and end in t:
            i = t.index(start) + len(start)
            j = t.index(end)
            t = t[:i] + '\n' + body + '\n' + t[j:]
            open(f, 'w', encoding='utf-8', newline='\n').write(t)

    fill('README.md', '<!-- BENCH:START -->', '<!-- BENCH:END -->', tables['en'])
    fill('README.tr.md', '<!-- BENCH:START -->', '<!-- BENCH:END -->', tables['tr'])
    fill('docs/benchmarks.md', '<!-- RESULTS:START -->', '<!-- RESULTS:END -->', tables['en'] + '\n\nEvery run, in order, is in `bench/results/compare.json` (SweaxDrop) and `bench/results/localsend.jsonl` (LocalSend).')
    if 'network' in out:
        fill('docs/benchmarks.md', '<!-- TRANSPORT:START -->', '<!-- TRANSPORT:END -->', out['network'])

    import art
    for theme in art.T:
        for lang in ('en', 'tr'):
            with open(os.path.join(ROOT, 'docs', 'art', f'bench-{lang}-{theme}.svg'), 'w', encoding='utf-8', newline='\n') as f:
                f.write(chart(art, theme, lang, cases))


def chart(art, theme, lang, cases):
    c = art.T[theme]
    W = 1280
    row_h = 118
    H = 150 + row_h * len(cases) + 40
    title = 'SweaxDrop and LocalSend, same PC, same files' if lang == 'en' else 'SweaxDrop ve LocalSend; aynı PC, aynı dosyalar'
    note = 'Time until every file is on disk. Median of 4 runs. Lower is better.' if lang == 'en' else 'Her dosya diske yazılana kadar geçen süre. 4 ölçümün ortancası. Düşük olan iyi.'
    names = ''.join(x['name' if lang == 'en' else 'name_tr'] for x in cases)
    text = title + note + names + 'SweaxDrop LocalSend 1.18.2 kat× faster hızlı'
    css = art.fonts_for({('Bric', art.bric_file(text), 700, None): text, ('Bric', art.bric_file(text), 450, None): text, ('Mono', art.MONO, 420, 87.5): '0123456789.,× smMBkatfsrelow '})
    rows = []
    for i, x in enumerate(cases):
        y = 150 + i * row_h
        a, b = x['sweaxdrop']['seconds'], x['localsend']['seconds']
        mx = max(a, b)
        wa, wb = 760 * a / mx, 760 * b / mx
        nm = x['name' if lang == 'en' else 'name_tr']
        ratio = f"{x['ratio']:.1f}× {'faster' if lang == 'en' else 'kat hızlı'}" if x['ratio'] >= 1.15 else ('about the same' if lang == 'en' else 'yaklaşık aynı')
        fmt = lambda s: f'{s:.2f} s' if s < 10 else f'{s:.1f} s'
        rows.append(f'''<g style="animation:rise .8s {0.1 + i*0.1:.2f}s cubic-bezier(.2,.8,.2,1) both">
<text x="70" y="{y+18}" class="t" font-size="19" font-weight="700" fill="{c['ink']}">{art.esc(nm)}</text>
<text x="{W-70}" y="{y+18}" text-anchor="end" class="m" font-size="14" fill="{c['recv']}">{art.esc(ratio)}</text>
<text x="70" y="{y+52}" class="t" font-size="14" font-weight="450" fill="{c['ink2']}">SweaxDrop</text>
<rect x="250" y="{y+38}" width="760" height="18" rx="9" fill="{c['s3']}"/>
<rect x="250" y="{y+38}" width="{wa:.1f}" height="18" rx="9" fill="{c['send']}"><animate attributeName="width" from="0" to="{wa:.1f}" dur="1.2s" begin="{0.2 + i*0.1:.2f}s" fill="freeze" calcMode="spline" keySplines=".2 .8 .2 1"/></rect>
<text x="{W-70}" y="{y+52}" text-anchor="end" class="m" font-size="14" fill="{c['ink']}">{fmt(a)}</text>
<text x="70" y="{y+84}" class="t" font-size="14" font-weight="450" fill="{c['ink2']}">LocalSend 1.18.2</text>
<rect x="250" y="{y+70}" width="760" height="18" rx="9" fill="{c['s3']}"/>
<rect x="250" y="{y+70}" width="{wb:.1f}" height="18" rx="9" fill="{c['ink3']}"><animate attributeName="width" from="0" to="{wb:.1f}" dur="1.2s" begin="{0.25 + i*0.1:.2f}s" fill="freeze" calcMode="spline" keySplines=".2 .8 .2 1"/></rect>
<text x="{W-70}" y="{y+84}" text-anchor="end" class="m" font-size="14" fill="{c['ink']}">{fmt(b)}</text>
</g>''')
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" role="img" aria-label="{art.esc(title)}">
<style>{css}.t{{font-family:Bric,sans-serif}}.m{{font-family:Mono,monospace}}@keyframes rise{{from{{opacity:0;transform:translateY(10px)}}to{{opacity:1;transform:none}}}}</style>
<rect width="{W}" height="{H}" rx="28" fill="{c['bg']}"/>
<text x="70" y="76" class="t" font-size="28" font-weight="700" fill="{c['ink']}" letter-spacing="-.5">{art.esc(title)}</text>
<text x="70" y="108" class="t" font-size="15" font-weight="450" fill="{c['ink3']}">{art.esc(note)}</text>
{''.join(rows)}
</svg>'''


if __name__ == '__main__':
    main()
