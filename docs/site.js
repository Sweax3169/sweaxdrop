// SweaxDrop site: lanes in the background, reveal on scroll, EN/TR,
// benchmark bars drawn from data/bench.json (measured runs only).

const TR = {
  nav_how: 'Nasıl çalışır', nav_speed: 'Hız', nav_sec: 'Güvenlik', nav_get: 'İndir',
  kicker: 'Windows · Android · iPhone · herhangi bir tarayıcı',
  h1a: 'Dosyaların,', h1b: 'paralel şeritlerde.',
  lead: 'Fotoğraf, video, klasör ve metin; bilgisayarınla telefonların arasında, kendi Wi-Fi’ın üzerinden. Uçtan uca şifreli, parça parça doğrulanır, bağlantı koparsa kaldığı yerden sürer.',
  cta_get: 'İndir', cta_try: 'Burada dene',
  f1: '8 şerit, şifreli, bu PC’nin loopback’inde', f2: 'iki taraf da anahtarıyla kanıtlanır', f3: 'hesap, sunucu, bulut',
  live: 'Canlı demo · gerçek arayüz, sahte bir ağla',
  b1t: 'Dosya, klasör, fotoğraf', b1: 'klasör yapısı korunur, telefonda fotoğraflar galeriye gider',
  b2t: 'Metin ve pano', b2: 'telefondaki bir bağlantı doğrudan PC panosuna',
  b3t: 'Sıra, duraklat, sürdür', b3: 'Wi-Fi giderse yalnız eksik parçalar yeniden gider',
  b4t: 'Telefonda uygulama yok mu?', b4: 'QR kodu okut, tarayıcı uygulama olur',
  how_h: 'Neden hızlı',
  how_p1: 'Çoğu araç her dosyayı ayrı bir istekle gönderir ve sıradakine geçmeden cevap bekler. İki bin tatil fotoğrafında zamanın çoğu bu beklemedir.',
  how_p2: 'SweaxDrop aynı anda birkaç şifreli şerit açar. Her şerit sıradaki 4 MiB’lık parçayı tek bir kuyruktan alır: büyük bir video hepsine bölünür, küçük dosyalar beklemeden peş peşe gider, takılan bir şerit kimseyi bekletmez. Okuma, hash, şifreleme ve yazma ayrı iş parçacıklarında üst üste biner.',
  how_p3: 'Uygulamada gördüğün şeritler gerçek şeritlerdir: her biri kendi bağlantısı ne kadar bayt taşıyorsa o hızda akar.',
  v1: 'Windows: dosyaları bir cihaza bırak, telefonun fotoğraflarını kabul et, hıza geri dön bak.',
  v2: 'Android ve iPhone: aynı uygulama, aynı şeritler.',
  speed_h: 'Ölçüldü, vaat edilmedi',
  speed_p: 'Bu sayfadaki her sayı depodaki sweaxdrop-bench ile tek bir PC’de (AMD Ryzen 5 7600, NVMe SSD, Windows 11) resmî LocalSend 1.18.2 komut satırı uygulamasına karşı, aynı dosyalarla, aynı sürücüde ölçüldü. Gerçek telefonlarla Wi-Fi sonuçları ölçüldükçe eklenir.',
  speed_note: 'Wi-Fi’da tek büyük dosyada sınır radyodur. Fark; çok sayıda küçük dosyada, başlama süresinde ve bağlantı koptuğunda ne olduğunda kalır.',
  sec_h: 'Baştan gizli',
  sec1: 'Her cihazın kendi Ed25519 anahtarı vardır. Kimliği bu anahtardan türetilir, ödünç alınamaz.',
  sec2: 'Her aktarım, iki tarafın da anahtarını kanıtladığı TLS 1.3 içinde akar. Eşleşen cihazlar sabitlenir: farklı bir anahtar reddedilir.',
  sec3: 'PIN ile eşleştirme SPAKE2 kullanır ve iki TLS anahtarını da kontrol eder; bağlantıyı araya girip aktaran biri senin yerine eşleşemez.',
  sec4: 'Her 4 MiB’lık parça bir BLAKE3 hash’i taşır ve yazılmadan önce doğrulanır.',
  sec5: 'Hiçbir şey ağından çıkmaz: hesap yok, sunucu yok, telemetri yok.',
  get_h: 'SweaxDrop’u edin',
  get_win: 'Kurulum dosyası, yaklaşık 5 MB. Güvenlik duvarı kuralı için bir kez (UAC) sorar.',
  get_and: 'Sürüm sayfasından APK. Sorulunca tarayıcından yüklemeye izin ver.',
  get_ios: 'Hemen: PC’deki QR kodu okut. Yerel uygulama: .ipa’yı yan yükle (adımlar README’de).',
  faq_h: 'Sorular',
  q1: 'İnternet gerekir mi?', a1: 'Hayır. Cihazlar yerel ağda birbirini bulur ve doğrudan konuşur. Telefon erişim noktası ya da USB bağlantısı da çalışır ve çoğu zaman modem üzerinden geçmekten hızlıdır.',
  q2: 'Her durumda LocalSend’den hızlı mı?', a2: 'Wi-Fi’da tek büyük dosyada değil: orada ikisinin de sınırı radyodur. Çok dosyada hızlıdır, daha erken başlar ve baştan başlamak yerine kaldığı yerden sürer. Yukarıdaki sayılar tam olarak nerede olduğunu gösteriyor.',
  q3: 'Tarayıcı istemcisi ne durumda?', a3: 'Ağında düz HTTP’dir ve QR koddaki tek kullanımlık sırla korunur; çünkü tarayıcılar PC’nin kendinden imzalı sertifikasını uyarısız kabul edemez. Uygulamalar şifreli protokolü kullanır.',
  q4: 'Gelen dosyalar nereye gider?', a4: 'Windows: İndirilenler\\SweaxDrop ya da seçtiğin klasör. Android: fotoğraf ve videolar galeriye, gerisi Download/SweaxDrop’a. iPhone: Fotoğraflar ve Dosyalar uygulamasında SweaxDrop.',
};

const EN = {};
let lang = (localStorage.getItem('swx-site-lang') || (navigator.language || 'en').slice(0, 2)) === 'tr' ? 'tr' : 'en';

function applyLang() {
  document.documentElement.lang = lang;
  document.getElementById('lang').textContent = lang === 'tr' ? 'EN' : 'TR';
  for (const el of document.querySelectorAll('[data-i]')) {
    const k = el.dataset.i;
    if (!(k in EN)) EN[k] = el.textContent;
    el.textContent = lang === 'tr' && TR[k] ? TR[k] : EN[k];
  }
  for (const el of document.querySelectorAll('[data-src-tr]')) {
    const attr = el.tagName === 'SOURCE' ? 'srcset' : 'src';
    if (!el.dataset.srcEn) el.dataset.srcEn = el.getAttribute(attr);
    el.setAttribute(attr, lang === 'tr' ? el.dataset.srcTr : el.dataset.srcEn);
  }
  for (const f of document.querySelectorAll('iframe')) {
    const u = new URL(f.getAttribute('src'), location.href);
    if (u.searchParams.get('lang') !== lang) {
      u.searchParams.set('lang', lang);
      f.src = u.pathname.split('/').slice(-2).join('/') + u.search;
    }
  }
  renderBench();
}

document.getElementById('lang').addEventListener('click', () => {
  lang = lang === 'tr' ? 'en' : 'tr';
  localStorage.setItem('swx-site-lang', lang);
  applyLang();
});

// Reveal on scroll (no pinning: content scrolls normally).
const io = new IntersectionObserver((es) => es.forEach((e) => e.isIntersecting && (e.target.classList.add('in'), io.unobserve(e.target))), { threshold: 0.12 });
document.querySelectorAll('.reveal').forEach((el) => io.observe(el));

// Background lanes: speed follows how fast you scroll.
(() => {
  const c = document.getElementById('lanes');
  const ctx = c.getContext('2d');
  let w, h, dpr, lanes;
  let v = 0;
  let lastY = scrollY;
  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
  function size() {
    dpr = Math.min(2, devicePixelRatio || 1);
    w = c.width = innerWidth * dpr;
    h = c.height = innerHeight * dpr;
    const n = Math.max(8, Math.round(innerHeight / 46));
    lanes = Array.from({ length: n }, (_, i) => ({ y: ((i + 0.5) / n) * h, off: Math.random() * 300, sp: 0.5 + Math.random() * 0.8, hue: i % 5 === 2 ? 'recv' : 'send' }));
  }
  size();
  addEventListener('resize', size);
  addEventListener('scroll', () => {
    v = Math.min(40, v + Math.abs(scrollY - lastY) * 0.08);
    lastY = scrollY;
  }, { passive: true });
  const css = () => getComputedStyle(document.documentElement);
  let col = { send: css().getPropertyValue('--send').trim(), recv: css().getPropertyValue('--recv').trim() };
  matchMedia('(prefers-color-scheme: light)').addEventListener('change', () => (col = { send: css().getPropertyValue('--send').trim(), recv: css().getPropertyValue('--recv').trim() }));
  function frame() {
    ctx.clearRect(0, 0, w, h);
    v *= 0.94;
    for (const l of lanes) {
      l.off = (l.off + (l.sp * (1 + v) * dpr) * (reduce ? 0 : 1)) % (180 * dpr);
      ctx.fillStyle = col[l.hue];
      for (let x = -180 * dpr + l.off; x < w; x += 180 * dpr) {
        const a = 0.05 + 0.08 * Math.sin((x / w) * Math.PI);
        ctx.globalAlpha = Math.max(0.02, a) * (0.6 + Math.min(0.4, v / 20));
        ctx.beginPath();
        ctx.roundRect(x, l.y - 2 * dpr, 64 * dpr, 4 * dpr, 2 * dpr);
        ctx.fill();
      }
    }
    ctx.globalAlpha = 1;
    requestAnimationFrame(frame);
  }
  frame();
})();

// Benchmarks
let bench = null;
fetch('data/bench.json')
  .then((r) => (r.ok ? r.json() : null))
  .then((d) => {
    bench = d;
    renderBench();
  })
  .catch(() => {});

function fmtTime(s) {
  return s < 1 ? `${(s * 1000).toFixed(0)} ms` : s < 60 ? `${s.toFixed(s < 10 ? 2 : 1)} s` : `${Math.floor(s / 60)} min ${Math.round(s % 60)} s`;
}

function renderBench() {
  const el = document.getElementById('bench');
  if (!el || !bench) return;
  const t = (en, tr) => (lang === 'tr' ? tr : en);
  el.innerHTML = '';
  for (const c of bench.cases) {
    const max = Math.max(c.sweaxdrop.mb_s, c.localsend.mb_s);
    const div = document.createElement('div');
    div.className = 'case';
    div.innerHTML = `<h3>${lang === 'tr' ? c.name_tr : c.name}</h3>
      <div class="what">${lang === 'tr' ? c.what_tr : c.what}</div>
      <div class="row sx"><span>SweaxDrop ${bench.version}</span><div class="bar"><i data-w="${(c.sweaxdrop.mb_s / max) * 100}"></i></div><span class="val">${c.sweaxdrop.mb_s.toFixed(0)} MB/s · ${fmtTime(c.sweaxdrop.seconds)}</span></div>
      <div class="row ls"><span>LocalSend 1.18.2</span><div class="bar"><i data-w="${(c.localsend.mb_s / max) * 100}"></i></div><span class="val">${c.localsend.mb_s.toFixed(0)} MB/s · ${fmtTime(c.localsend.seconds)}</span></div>
      <div class="ratio">${c.ratio >= 1.05 ? t(`${c.ratio.toFixed(1)}× faster`, `${c.ratio.toFixed(1)} kat hızlı`) : c.ratio <= 0.95 ? t(`${(1 / c.ratio).toFixed(1)}× slower`, `${(1 / c.ratio).toFixed(1)} kat yavaş`) : t('about the same', 'yaklaşık aynı')}</div>`;
    el.appendChild(div);
    requestAnimationFrame(() => setTimeout(() => div.querySelectorAll('i').forEach((i) => (i.style.width = i.dataset.w + '%')), 60));
  }
}

applyLang();
