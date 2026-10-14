/* =====================================================================
   Erika · 界面 —— 数据、工具、窗口外壳
   一个 App 实例 = 一个可操作的窗口
   桌面 1280×800 · 平板 1180×820（横放）· 手机 390×844（播放时转横屏 844×390）· 电视 1280×720（= 1920×1080 的 1.5 倍）
   折叠屏有两种硬件：苹果 = iPhone Duo（护照形，内外屏都是 1.41:1，系统栏和工具栏在右侧竖排），
   其他 = 书本式大折叠（Mate X7 / Galaxy Z Fold：外屏就是一台手机，内屏接近正方形）；姿态有 合上 / 展开 / 悬停 三种
   桌面、平板、电视共用「宽屏」外壳（侧栏 / 导航轨 / 顶栏），手机和折叠屏用手机外壳（折叠屏展开后分成左右两栏）；功能在五种设备上一样多
   ===================================================================== */
window.NX = (() => {
  'use strict';
  const { $, $$, svg, TOKENS, Spring, run, stop, tween, cancel, bindJelly, toast, RM, MODS, MOD, ALL, PRESETS, createStore, pagesFor } = NP;

  /* ---------------- 图标：通用线条图标；Material 外观换 Material Symbols ---------------- */
  const MS = { play: 'play_arrow', pause: 'pause', next: 'skip_next', prev: 'skip_previous', rw10: 'replay_10', ff10: 'forward_10', vol: 'volume_up', danmaku: 'chat', 'danmaku-off': 'comments_disabled', cc: 'closed_caption', audio: 'graphic_eq', sliders: 'tune', pip: 'picture_in_picture_alt', full: 'fullscreen', more: 'more_vert', link: 'link', 'chev-l': 'arrow_back', 'chev-r': 'chevron_right', sparkle: 'auto_awesome', send: 'send', episodes: 'video_library', check: 'check', x: 'close', camera: 'photo_camera', info: 'info', 'chev-d': 'arrow_drop_down', tv: 'screen_lock_landscape', home: 'home', calendar: 'calendar_month', library: 'video_library', download: 'download', search: 'search', plus: 'add', folder: 'folder_open', gear: 'settings', film: 'movie', heart: 'favorite', star: 'star', grid: 'grid_view', list: 'list', sort: 'sort', user: 'person', puzzle: 'extension', palette: 'palette', keyboard: 'keyboard', sync: 'sync', server: 'dns', wifi: 'wifi', eyeoff: 'visibility_off', filter: 'filter_alt', shield: 'shield', tv2: 'tv', scissors: 'content_cut', external: 'open_in_new', music: 'music_note', moon: 'dark_mode', sun: 'light_mode', annot: 'crop_free', code: 'code', 'danmaku-on': 'chat' };
  const I = (n, st = '') => `<span class="ic">${svg(n, 'i', st)}<span class="msr">${MS[n] || n}</span></span>`;

  /* ---------------- 数据（海报取自原项目） ---------------- */
  const P = i => `assets/posters/p${i}.jpg`;
  const STILL = 'assets/still1.jpg';
  const ANIME = [
    { id: 1, t: '寒蝉鸣泣之时 解', p: 1, eps: 24, w: 1, r: 8.4, tags: ['悬疑', '推理'], py: 'H' },
    { id: 2, t: '只有神知道的世界 女神篇', p: 2, eps: 12, w: 11, r: 7.9, tags: ['恋爱', '喜剧'], py: 'Z' },
    { id: 3, t: '【我推的孩子】 第三季', p: 3, eps: 11, w: 2, r: 7.8, tags: ['偶像', '剧情'], py: 'W' },
    { id: 4, t: '超时空辉夜姬！', p: 4, eps: 1, w: 0, r: 7.7, tags: ['科幻', '音乐'], movie: true, py: 'C' },
    { id: 5, t: '咒术回战 死灭回游', p: 5, eps: 12, w: 3, r: 7.6, tags: ['战斗', '奇幻'], py: 'Z' },
    { id: 6, t: '为美好的世界献上祝福！3', p: 6, eps: 11, w: 11, r: 7.9, tags: ['喜剧', '异世界'], py: 'W' },
    { id: 7, t: '葬送的芙莉莲 第二季', p: 7, eps: 10, w: 3, r: 8.4, tags: ['奇幻', '冒险'], py: 'Z' },
    { id: 8, t: '剧场版 魔法少女小圆', p: 8, eps: 1, w: 0, r: 8.4, tags: ['魔法少女', '剧情'], movie: true, py: 'J' },
    { id: 9, t: 'Fate/strange Fake', p: 9, eps: 13, w: 3, r: 8.0, tags: ['战斗', '奇幻'], py: 'F' },
    { id: 10, t: '地狱乐 第二季', p: 10, eps: 12, w: 2, r: 6.7, tags: ['战斗', '历史'], py: 'D' },
    { id: 11, t: '鬼灭之刃 无限城篇', p: 11, eps: 1, w: 0, r: 0, tags: ['战斗'], movie: true, py: 'G' },
    { id: 12, t: '寒蝉鸣泣之时 礼', p: 12, eps: 5, w: 0, r: 7.3, tags: ['悬疑'], py: 'H' },
    { id: 13, t: '魔都精兵的奴隶 第二季', p: 13, eps: 12, w: 0, r: 6.1, tags: ['战斗', '奇幻'], py: 'M' },
    { id: 14, t: '现在多闻君是哪一面！？', p: 14, eps: 12, w: 0, r: 5.8, tags: ['恋爱', '喜剧'], py: 'X' },
    { id: 15, t: '优雅贵族的休假指南。', p: 15, eps: 12, w: 0, r: 4.9, tags: ['异世界', '日常'], py: 'Y' },
    { id: 16, t: '厄里斯的圣杯', p: 16, eps: 12, w: 0, r: 5.4, tags: ['悬疑', '历史'], py: 'E' },
  ];
  const ERASED = { id: 0, t: '只有我不存在的城市', eps: 12, w: 0, still: true, tags: ['悬疑'], py: 'Z' };
  const byId = id => (+id === 0 ? ERASED : ANIME.find(a => a.id === +id));
  const imgOf = a => (a.still ? STILL : P(a.p));
  const CW = [
    { id: 0, ep: 1, prog: 0.38, left: 15, img: STILL, pos: '50% 50%' },
    { id: 7, ep: 4, prog: 0.35, left: 16, img: P(7), pos: '50% 30%' },
    { id: 3, ep: 3, prog: 0.2, left: 19, img: P(3), pos: '50% 25%' },
    { id: 5, ep: 4, prog: 0.8, left: 4, img: P(5), pos: '50% 30%' },
  ];
  /* 今天（周一）的更新，按播出时间挂在时间轴上；「现在」= 22:40 */
  const TODAY = [{ id: 13, t: '18:30' }, { id: 14, t: '19:45' }, { id: 15, t: '21:00' }, { id: 16, t: '22:15' }, { id: 9, t: '23:30' }, { id: 12, t: '00:30' }, { id: 2, t: '01:30' }];
  const NOW = 22 + 40 / 60;
  const hOf = s => { const [h, m] = s.split(':').map(Number); return (h < 12 ? h + 24 : h) + m / 60; };
  const DAYS = ['周一', '周二', '周三', '周四', '周五', '周六', '周日'];
  const TODAY_IDX = 0;
  const TIMES = ['18:00', '19:30', '21:00', '22:00', '23:00', '23:30', '00:30', '01:05'];
  const WEEK = DAYS.map((d, i) => {
    if (i === TODAY_IDX) return TODAY.map(x => ({ ...x, a: byId(x.id) }));
    const n = [7, 5, 6, 4, 6, 8, 7][i];
    return Array.from({ length: n }, (_, k) => ({ id: ANIME[(i * 5 + k * 3) % ANIME.length].id, t: TIMES[k % TIMES.length] })).map(x => ({ ...x, a: byId(x.id) }));
  });
  const RECENTS = [
    { t: '[Nekomoe] Sousou no Frieren S2 - 03 [1080p].mkv', prog: 0.62, left: '9:12', img: P(7) },
    { t: 'Boku dake ga Inai Machi - 01.mkv', prog: 0.38, left: '15:03', img: STILL },
    { t: 'Vacation_2026_Kyoto.mp4', prog: 0.9, left: '0:41', img: P(15) },
  ];
  const DL = [
    { t: '[LoliHouse] Sousou no Frieren S2 - 04 [1080p].mkv', size: '1.2 GB', prog: 0.64, speed: 8.4, on: true },
    { t: '[Nekomoe] Kaguya-sama - 01 [1080p].mkv', size: '860 MB', prog: 0.92, speed: 6.1, on: true },
    { t: '[ANi] Jujutsu Kaisen - 05 [1080p].mp4', size: '540 MB', prog: 0.22, speed: 0, on: false },
    { t: '[Sakurato] Oshi no Ko S3 - 03 [1080p].mkv', size: '980 MB', prog: 1, speed: 0, on: false },
  ];
  const COMMENTS = [
    { n: '路过的观众', s: 9, t: '作画一直很稳，节奏舒服，适合慢慢看。' },
    { n: '追番中', s: 8, t: '这一季的配乐比上一季更好，期待后面的展开。' },
    { n: 'Nipa 用户', s: 7, t: '前两集铺垫有点长，第三集开始好看起来了。' },
  ];
  const LINES = [['前方高能！！', ''], ['名场面来了', ''], ['这段作画太顶了', '#FFD24A'], ['OP 好听', ''], ['再上映——', ''], ['来了来了', '#8EE3FF'], ['泪目', ''], ['这里有伏笔', ''], ['下周见', ''], ['快跑！', '#FFB3C7']];
  const SUBS = ['这一次，我一定要救下她。', '……又回到那一天了吗？', '别怕，我就在这里。', '雪停之前，一起回家吧。'];
  const DUR = 1455;
  const CH = [{ t: 0, n: '序幕' }, { t: 92, n: 'OP' }, { t: 182, n: 'A 部分' }, { t: 700, n: 'B 部分' }, { t: 1295, n: 'ED' }, { t: 1385, n: '预告' }];
  const PEAKS = [342, 1098];

  /* ---------------- 工具 ---------------- */
  const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
  const fmt = s => { s = Math.max(0, Math.floor(s)); return `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`; };
  const rateLabel = r => (Number.isInteger(r) ? r.toFixed(1) : String(r)) + '×';
  const statusOf = a => (a.w === 0 ? '未看' : a.w >= a.eps ? '看完' : `看到 ${a.w} / ${a.eps}`);
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  /* 海报取色：画到 8×8 画布里求平均，给详情页头部铺一层同色调的底（不用模糊） */
  const COLORS = {};
  function colorOf(src) {
    if (COLORS[src]) return Promise.resolve(COLORS[src]);
    return new Promise(res => {
      const im = new Image(); im.onload = () => {
        try {
          const c = document.createElement('canvas'); c.width = c.height = 8; const g = c.getContext('2d'); g.drawImage(im, 0, 0, 8, 8);
          const d = g.getImageData(0, 0, 8, 8).data; let r = 0, gg = 0, b = 0;
          for (let i = 0; i < d.length; i += 4) { r += d[i]; gg += d[i + 1]; b += d[i + 2]; }
          const n = d.length / 4; COLORS[src] = `rgb(${Math.round(r / n)}, ${Math.round(gg / n)}, ${Math.round(b / n)})`;
        } catch (e) { COLORS[src] = '#888'; }
        res(COLORS[src]);
      };
      im.onerror = () => res('#888'); im.src = src;
    });
  }
  const pcard = (a, sub = '', extra = '') => `<div class="x-pcard"><div class="x-poster" data-act="detail" data-aid="${a.id}"${extra}><img src="${imgOf(a)}" alt="">${a.w > 0 && a.w < a.eps ? `<div class="x-prog"><b style="width:${(a.w / a.eps) * 100}%"></b></div>` : ''}</div><div class="x-pt">${a.t}</div>${sub ? `<div class="x-ps">${sub}</div>` : ''}</div>`;

  /* ---------------- 折叠屏硬件（逻辑像素）----------------
     duo：iPhone Duo，外屏 5.4"、内屏 7.6"，都是 1.41:1；合上竖拿 470×662，展开横拿 930×662（正好两块外屏宽），
          悬停 = 内屏转竖、上下对折 662×930
     matex：Mate X7 / Galaxy Z Fold 一类，外屏 20.4:9 和手机一样，内屏 8:7.3 接近正方形 740×808，悬停 = 转横、上下对折 808×740 */
  const FOLD_POSE = ['closed', 'open', 'tent'];
  const FOLD = {
    duo: { closed: { W: 470, H: 662 }, land: { W: 662, H: 470 }, open: { W: 930, H: 662 }, tent: { W: 662, H: 930 } },
    matex: { closed: { W: 390, H: 844 }, land: { W: 844, H: 390 }, open: { W: 740, H: 808 }, tent: { W: 808, H: 740 } },
  };
  const hwOf = ds => (ds === 'apple' ? 'duo' : 'matex');

  /* ======================================================================
     App：一个可操作的窗口
     ====================================================================== */
  class App {
    constructor(host, cfg) {
      this.host = host; this.cfg = cfg;
      /* cfg.persist：把模块选择存进这个键（交互原型用，刷新后还在） */
      this.store = createStore(cfg.mods || ALL, cfg.persist || null);
      this.dev = cfg.dev; this.land = false;
      /* 折叠屏：姿态（closed 合上 / open 展开 / tent 悬停）和硬件（duo = iPhone Duo，matex = 书本式大折叠） */
      this.fold = cfg.dev === 'fold' ? (FOLD_POSE.includes(cfg.fold) ? cfg.fold : 'open') : null;
      this.hw = hwOf(cfg.ds);
      this.S ={ page: 'home', detail: null, scroll: {}, lib: { seg: 'local', filter: 'all', sort: 'recent', zoom: 0.36 }, newDay: TODAY_IDX, set: 'optional', dlTab: 'on', phStack: [] };
      this.cw = CW.slice();
      this.dl = DL.map(d => ({ ...d }));
      this.P = null; this.overlays = [];
      this.timers = [];
      this.mount();
    }
    /* ph：手机外壳（底部标签 + 一层层推进的页面），手机和折叠屏都用；rot：播放时转横屏（手机、合上的折叠屏） */
    get ph() { return this.dev === 'phone' || this.dev === 'fold'; }
    get wide() { return !this.ph; }
    get tv() { return this.dev === 'tv'; }
    get touch() { return this.dev === 'phone' || this.dev === 'tablet' || this.dev === 'fold'; }
    get rot() { return this.dev === 'phone' || this.fold === 'closed'; }
    /* twin：折叠屏展开，分成两栏，折痕正好落在两栏中间（悬停时上下两栏：上半屏是画框，下半屏是手指能碰的那一半）；
       rbar：iPhone Duo 横着拿时，系统栏和工具栏竖排在右侧（悬停时内屏是竖的，栏回到顶部和底部） */
    get twin() { return this.dev === 'fold' && this.fold !== 'closed'; }
    get rbar() { return this.dev === 'fold' && this.hw === 'duo' && this.fold !== 'tent'; }
    get geo() { const g = FOLD[this.hw]; return this.fold === 'closed' ? (this.land ? g.land : g.closed) : g[this.fold]; }
    get W() { return this.dev === 'fold' ? this.geo.W : { desk: 1280, tablet: 1180, tv: 1280 }[this.dev] || (this.land ? 844 : 390); }
    get H() { return this.dev === 'fold' ? this.geo.H : { desk: 800, tablet: 820, tv: 720 }[this.dev] || (this.land ? 390 : 844); }
    get k() { const r = this.el.getBoundingClientRect(); return r.width / this.W || 1; }
    rel(node) {
      const S = this.el.getBoundingClientRect(), r = node.getBoundingClientRect(), k = S.width / this.W || 1;
      return { x: (r.left - S.left) / k, y: (r.top - S.top) / k, w: r.width / k, h: r.height / k };
    }
    destroy() { this.dead = true; this.timers.forEach(clearInterval); this.timers = []; if (this.P) clearTimeout(this.P.hideT); }
    look() {
      const d = this.el.dataset; d.ds = this.cfg.ds; d.th = this.cfg.th; d.dev = this.dev;
      if (this.dev === 'fold') { d.fold = this.fold; d.hw = this.hw; }
      this.el.classList.toggle('twin', this.twin); this.el.classList.toggle('rbar', this.rbar);
    }
    /* 窗口里自己改明暗（引导的「外观」、搜索里的操作）；cfg.onTheme 让外面的页面跟着变 */
    setTh(th) { this.cfg.th = th; this.look(); if (this.cfg.onTheme) this.cfg.onTheme(th); }
    setLook(ds, th) {
      /* 折叠屏换到 / 换出苹果 = 换了一台硬件（iPhone Duo ↔ 书本式大折叠），整个重排 */
      if (this.dev === 'fold' && hwOf(ds) !== this.hw) { this.cfg.ds = ds; this.cfg.th = th; this.hw = hwOf(ds); this.relayout(); return; }
      this.cfg.ds = ds; this.cfg.th = th; this.look();
      if (this.P && this.P.open) this.plLayout(this.P.lay, true);
      if (this.S.page === 'lib' && !this.S.detail && this.libLayout) setTimeout(() => this.libLayout(true), 30);
      if (this.S.page === 'new' && !this.S.detail && this.newLayout) setTimeout(() => (this.wide ? this.newLayout(true) : !this.S.phStack.length && this.phNewLayout(true)), 30);
    }
    /* 折叠屏换姿态：合上 / 展开 / 悬停 */
    setFold(f) {
      if (this.dev !== 'fold' || !FOLD_POSE.includes(f) || f === this.fold) return;
      this.fold = f; this.relayout();
    }
    /* 当场重排：停在同一页、同一层，正在放的视频接着放（折叠是一个实时事件，不是重新打开 App） */
    relayout() {
      const p = this.P, playing = !!(p && p.open), ob = this.overlays.some(o => o.kind === 'ob') ? this.ob : null;
      if (p) clearTimeout(p.hideT);
      this.menuKind = null; this.overlays = []; this.land = false; this.scr = null; this._pn = null;
      this.el.classList.remove('land');
      this.look(); this.build();
      if (playing && this.rot) {
        /* 合上时在放：外屏转横屏接着放 */
        this.renderPhone(false);
        this.setLand(true, () => { p.open = true; p.mode = 'full'; this.plBuild(); this.plLayout('shown', true); this.plWake(); this.renderMini(); });
        return;
      }
      this.render(false);
      if (playing) { p.open = true; p.mode = 'full'; this.plBuild(); this.plLayout('shown', true); this.plWake(); this.renderMini(); }
      if (ob) { this.obOpen(); this.ob = ob; this.obRender(); }
      this.cfg.onLand && this.cfg.onLand('fold');
    }
    pages() { return pagesFor(this.store.mods); }
    pure() { return !this.pages().length; }

    /* ---------------- 挂载 ---------------- */
    mount() {
      this.host.innerHTML = '<div class="nx"></div>';
      this.el = this.host.firstElementChild;
      this.look();
      this.el.addEventListener('click', e => this.onClick(e));
      this.el.addEventListener('pointerdown', e => this.onDown(e));
      this.el.addEventListener('input', e => this.onInput(e));
      this.el.addEventListener('keydown', e => this.onKey(e));
      this.el.addEventListener('dragenter', e => { e.preventDefault(); this.dragOver(true); });
      this.el.addEventListener('dragover', e => e.preventDefault());
      this.el.addEventListener('dragleave', e => { if (!this.el.contains(e.relatedTarget)) this.dragOver(false); });
      this.el.addEventListener('drop', e => { e.preventDefault(); this.dragOver(false); const f = e.dataTransfer.files && e.dataTransfer.files[0]; if (f) this.playFile(f); });
      this.el.addEventListener('pointermove', e => this.plWake && this.plWake(e));
      if (this.tv) this.tvInit();
      this.store.on((m, info) => this.onMods(info));
      this.build();
      const s = this.cfg.start || 'home';
      requestAnimationFrame(() => this.startAt(s));
    }
    build() {
      const wide = this.wide;
      const common = `
        <div class="x-player dk"></div>
        <div class="x-dim"></div>
        <div class="x-pal"></div>
        <div class="x-set"></div>
        <div class="o-win"></div>
        <div class="c-menu x-pop"></div>
        <input type="file" class="x-file" accept="video/*,.mkv,.ass,.srt" hidden>`;
      if (wide) {
        /* x-tstat：平板的系统状态栏；x-fab：Material 平板导航轨顶部的「打开」；x-open：电视顶栏右侧的「打开」 */
        this.el.innerHTML = `
          <div class="x-tstat"><span>9:41</span><span>10月12日 周一</span><span class="grow"></span><span style="display:flex;gap:6px;align-items:center"><i style="width:15px;height:10px;border-radius:2px;background:currentColor"></i><i style="width:24px;height:11px;border-radius:3px;border:1.5px solid currentColor"></i></span></div>
          <div class="x-wtitle"><img src="assets/icon.png" alt="">Erika</div>
          <aside class="x-side">
            <div class="x-side-top"><span class="x-lights"><i></i><i></i><i></i></span></div>
            <div class="x-brand"><img src="assets/icon.png" alt="">Erika</div>
            <span class="c-btn p x-fab" data-act="open" title="打开">${I('plus')}</span>
            <nav class="c-nav"></nav>
            <div class="x-mini"></div>
            <div class="x-acct"><span class="x-av"></span><div class="tx"><b>Nipa</b><span class="acct-sub"></span></div><span class="c-ib flat x-open" data-act="open" title="打开">${I('folder')}</span><span class="c-ib flat" data-act="settings" title="设置">${I('gear')}</span></div>
          </aside>
          <section class="x-main">
            <header class="x-tb"></header>
            <div class="x-scroll"><div class="x-page"></div></div>
          </section>
          <div class="x-idle"></div>
          <span class="x-caps"><i>—</i><i>☐</i><i>✕</i></span>
          ${common}`;
      } else {
        this.el.innerHTML = `
          <div class="x-status"><span>9:41</span><span style="display:flex;gap:5px;align-items:center"><i style="width:17px;height:10px;border-radius:2px;background:currentColor"></i><i style="width:24px;height:11px;border-radius:3px;border:1.5px solid currentColor"></i></span></div>
          <div class="x-pbody"></div>
          <div class="x-pmini"></div>
          <nav class="c-tabbar"></nav>
          ${this.rbar ? '<aside class="x-rbar"></aside>' : ''}
          ${this.dev === 'fold' ? '<i class="x-crease"></i>' : ''}
          ${common}`;
      }
      this.q = {
        side: $('.x-side', this.el), nav: $('.c-nav', this.el), mini: $('.x-mini', this.el), tb: $('.x-tb', this.el), scroll: $('.x-scroll', this.el), page: $('.x-page', this.el), idle: $('.x-idle', this.el),
        body: $('.x-pbody', this.el), pmini: $('.x-pmini', this.el), tabs: $('.c-tabbar', this.el), rbar: $('.x-rbar', this.el),
        player: $('.x-player', this.el), dim: $('.x-dim', this.el), pal: $('.x-pal', this.el), set: $('.x-set', this.el), ob: $('.o-win', this.el), pop: $('.x-pop', this.el), file: $('.x-file', this.el),
      };
      this.q.file.addEventListener('change', () => { const f = this.q.file.files && this.q.file.files[0]; if (f) this.playFile(f); this.q.file.value = ''; });
      this.q.dim.addEventListener('click', () => this.closeTop());
    }
    startAt(s) {
      if (this.dead) return;
      const [scr, sub] = s.split(':');
      if (scr === 'detail') { this.S.page = 'home'; this.render(false); this.openDetail(7, null, false); return; }
      if (scr === 'player') { this.S.page = 'home'; this.render(false); setTimeout(() => this.plOpen({ aid: 0, ep: 1 }, null), 200); return; }
      if (scr === 'search') { this.S.page = 'home'; this.render(false); setTimeout(() => this.palOpen('葬送'), 300); return; }
      if (scr === 'settings') { this.S.page = 'home'; this.render(false); setTimeout(() => this.setOpen(sub || 'optional'), 250); return; }
      if (scr === 'onboard') { this.S.page = 'home'; this.render(false); setTimeout(() => this.obOpen(), 250); return; }
      if (scr === 'mini') { this.S.page = 'home'; this.render(false); this.P = this.plNew({ aid: 7, ep: 4 }); this.P.mode = 'mini'; this.renderMini(); return; }
      if (scr === 'idle') { this.render(false); return; }
      this.S.page = scr; this.render(true);
    }

    /* ---------------- 外壳渲染 ---------------- */
    render(animate = true) {
      if (this.wide) this.renderDesk(animate); else this.renderPhone(animate);
    }
    renderDesk(animate) {
      const pages = this.pages(), pure = !pages.length;
      /* 电视：搜索是顶栏的第一个标签（桌面和平板用工具栏搜索框 / ⌘K） */
      const navs = this.tv && !pure ? [{ key: 'search', name: '搜索', icon: 'search' }].concat(pages) : pages;
      if (!pure && !navs.find(p => p.key === this.S.page)) { this.S.page = pages[0].key; this.S.detail = null; }
      this.q.side.style.display = pure ? 'none' : '';
      this.el.classList.toggle('pure', pure);
      this.q.nav.innerHTML = navs.map(p => `<div class="c-navi${p.key === this.S.page ? ' on' : ''}" data-act="nav" data-page="${p.key}">${I(p.icon)}<span>${p.name}</span>${p.key === 'dl' ? `<span class="cnt">${this.dl.filter(d => d.prog < 1).length}</span>` : ''}</div>`).join('');
      $$('.c-navi', this.q.nav).forEach(n => bindJelly(n, 'M'));
      $('.acct-sub', this.el).textContent = this.store.has('sync') ? '弹弹play · Bangumi' : '本地用户';
      this.renderTb();
      if (pure) { this.q.page.innerHTML = ''; this.renderIdle(animate); return; }
      this.q.idle.innerHTML = ''; this.q.idle.style.display = 'none';
      this.renderPage(animate);
      this.renderMini();
      if (this.tv) this.tvEnsure();
    }
    renderTb() {
      const pure = this.pure(), d = this.S.detail != null;
      const names = { home: '首页', new: '新番', lib: '媒体库', dl: '下载' };
      let h = '';
      if (pure) h += `<span class="x-lights" style="margin-right:6px"><i></i><i></i><i></i></span>`;
      if (d) h += `<span class="c-ib flat" data-act="back" title="返回">${I('chev-l')}</span><span class="x-crumb">${names[this.S.page] || ''}</span>`;
      h += '<span class="grow"></span>';
      if (!pure) h += `<label class="c-field x-search" style="width:250px" data-act="pal">${I('search')}<input placeholder="搜索番剧、文件、设置" readonly data-act="pal"><kbd>⌘K</kbd></label><span class="c-btn" data-act="open">${I('plus')}打开</span>`;
      if (pure) h += `<span class="c-ib" data-act="settings" title="设置">${I('gear')}</span>`;
      h += '<span class="x-gclose">✕</span>';
      this.q.tb.innerHTML = h;
      this.q.tb.style.paddingRight = this.cfg.ds === 'fluent' && this.pure() ? '150px' : '';
      $$('.c-btn, .c-ib', this.q.tb).forEach(b => bindJelly(b, b.classList.contains('c-btn') ? 'M' : 'S'));
    }
    renderPage(animate = true) {
      const p = this.q.page;
      if (this.S.detail != null) p.innerHTML = this.detailHTML(byId(this.S.detail));
      else p.innerHTML = this[this.S.page + 'HTML']();
      this.bindPage(p);
      const after = this['after_' + (this.S.detail != null ? 'detail' : this.S.page)];
      after && after.call(this, animate);
      if (animate) this.animIn(p);
      if (this.tv) this.tvEnsure();
    }
    bindPage(scope) {
      $$('.x-poster, .x-thumb:not(.nojelly), .h-frame', scope).forEach(e => bindJelly(e, 'card'));
      $$('.c-btn, .c-split > span, .c-chip, .c-seg > span', scope).forEach(e => bindJelly(e, 'M'));
      $$('.c-ib, .c-tr, .c-play', scope).forEach(e => bindJelly(e, 'S'));
      $$('.c-switch', scope).forEach(e => bindJelly(e, 'tg'));
    }
    go(page) {
      if (this.S.page === page && this.S.detail == null) return;
      if (this.wide) { this.S.scroll[this.S.page] = this.q.scroll.scrollTop; }
      this.S.page = page; this.S.detail = null;
      if (this.wide) {
        $$('.c-navi', this.q.nav).forEach(n => n.classList.toggle('on', n.dataset.page === page));
        this.renderTb(); this.renderPage(true); this.q.scroll.scrollTop = this.S.scroll[page] || 0;
      } else { this.S.phStack = []; this.renderPhone(true); }
    }
    onMods(info) {
      if (this.wide) { this.renderDesk(false); }
      else this.renderPhone(false);
      if (this.setRefresh) this.setRefresh();
    }

    /* ---------------- 动效小工具 ---------------- */
    animIn(el, dy = 12) {
      if (RM()) return;
      el.style.opacity = '0';
      tween(TOKENS.smooth, p => { el.style.opacity = String(Math.min(1, p)); el.style.transform = `translateY(${(1 - p) * dy}px)`; }, () => { el.style.transform = ''; el.style.opacity = ''; });
    }
    stagger(els, d0 = 40, dy = 14, gap = 32) {
      if (RM()) return;
      els.forEach((el, i) => {
        el.style.opacity = '0'; el.style.transform = `translateY(${dy}px)`;
        setTimeout(() => tween(TOKENS.jelly, p => { el.style.opacity = String(Math.min(1, p * 1.5)); el.style.transform = `translateY(${(1 - p) * dy}px)`; }, () => { el.style.transform = ''; el.style.opacity = ''; }), d0 + i * gap);
      });
    }
    /* 共享元素：拿一张图从 a 飞到 b（位置、尺寸、圆角一起走弹簧） */
    fly(img, a, b, r0, r1, done, token = TOKENS.hero, fit = 'cover') {
      if (RM()) { done && done(); return; }
      const c = document.createElement('div'); c.className = 'x-fly';
      c.innerHTML = `<img src="${img}" alt="" style="object-fit:${fit}">`;
      this.el.appendChild(c);
      const L = (u, v, p) => u + (v - u) * p;
      tween(token, p => {
        c.style.width = L(a.w, b.w, p) + 'px'; c.style.height = L(a.h, b.h, p) + 'px';
        c.style.transform = `translate(${L(a.x, b.x, p)}px, ${L(a.y, b.y, p)}px)`;
        c.style.borderRadius = Math.max(0, L(r0, r1, p)) + 'px';
      }, () => { done && done(); c.remove(); });
      return c;
    }
    rad(name = '--r-card') { return parseFloat(getComputedStyle(this.el).getPropertyValue(name)) || 0; }
    popIn(el, origin = '50% 0', token = TOKENS.bouncy) {
      el.style.display = 'block'; el.style.transformOrigin = origin; cancel(el._tw);
      if (RM()) { el.style.opacity = ''; el.style.transform = ''; return; }
      el._tw = tween(token, p => { el.style.transform = `scale(${0.92 + 0.08 * p})`; el.style.opacity = String(Math.min(1, p * 2)); });
    }
    popOut(el, done) {
      cancel(el._tw);
      if (RM()) { el.style.display = 'none'; done && done(); return; }
      el._tw = tween(TOKENS.snappy, p => { el.style.transform = `scale(${1 - 0.04 * Math.min(1, p)})`; el.style.opacity = String(Math.max(0, 1 - p * 1.4)); }, () => { el.style.display = 'none'; done && done(); });
    }
    showDim(on) {
      const d = this.q.dim; cancel(d._tw);
      if (on) { d.style.display = 'block'; if (!RM()) { d.style.opacity = '0'; d._tw = tween(TOKENS.smooth, p => { d.style.opacity = String(Math.min(1, p)); }); } }
      else if (d.style.display === 'block') { if (RM()) { d.style.display = 'none'; return; } d._tw = tween(TOKENS.smooth, p => { d.style.opacity = String(1 - Math.min(1, p)); }, () => { d.style.display = 'none'; d.style.opacity = ''; }); }
    }
    closeTop() {
      if (this.menuKind) { this.menuClose(); return true; }
      const o = this.overlays.pop(); if (!o) return false;
      o.close(); if (!this.overlays.length) this.showDim(false);
      return true;
    }
    toast(t) { toast(this.el, t); }

    /* ---------------- 弹出菜单（原生 Menu / MenuFlyout / Popover） ---------------- */
    menu(anchor, html, kind, onPick) {
      if (this.menuKind === kind) { this.menuClose(); return; }
      this.menuKind = kind; this.menuPick = onPick;
      const p = this.q.pop; p.classList.remove('dk'); p.innerHTML = html; p.style.display = 'block'; p.style.visibility = 'hidden'; p.style.zIndex = this.P && this.P.open ? 95 : 90;
      const a = this.rel(anchor), pw = p.offsetWidth, ph = p.offsetHeight;
      const up = a.y + a.h + ph + 12 > this.H;
      p.style.left = clamp(a.x + a.w / 2 - pw / 2, 8, this.W - pw - 8) + 'px';
      p.style.top = clamp(up ? a.y - ph - 8 : a.y + a.h + 8, 8, this.H - ph - 8) + 'px';
      const ax = clamp(a.x + a.w / 2 - parseFloat(p.style.left), 18, pw - 18);
      p.dataset.dir = up ? 'up' : 'down'; p.style.setProperty('--ax', ax + 'px');
      p.style.visibility = '';
      $$('.c-mi, .epg span', p).forEach(b => bindJelly(b, 'M'));
      this.popIn(p, `${ax}px ${up ? '100%' : '0'}`);
    }
    menuClose() { if (!this.menuKind) return; this.menuKind = null; this.popOut(this.q.pop); }

    /* ---------------- 点击分发 ---------------- */
    onDown(e) {
      if (this.menuKind && !e.target.closest('.x-pop') && !e.target.closest('[data-menu-anchor]')) this.menuClose();
      const s = e.target.closest('.c-slider');
      if (s && e.button === 0 && !s.closest('.x-player')) { e.preventDefault(); this.sliderDrag(s, e); }
      if (s && s.closest('.x-player') && this.plDown) this.plDown(e, s);
    }
    sliderDrag(s, e, onv) {
      const set = ev => { const r = s.getBoundingClientRect(); const v = clamp((ev.clientX - r.left) / r.width, 0, 1); s.style.setProperty('--v', v); onv ? onv(v) : s.dispatchEvent(new CustomEvent('nxslide', { detail: v, bubbles: true })); };
      s.classList.add('drag'); set(e);
      const mv = ev => set(ev), up = () => { s.classList.remove('drag'); removeEventListener('pointermove', mv); removeEventListener('pointerup', up); s.dispatchEvent(new CustomEvent('nxslideend', { bubbles: true })); onv && onv(null); };
      addEventListener('pointermove', mv); addEventListener('pointerup', up);
    }
    onInput(e) {
      if (e.target.closest('.x-pal')) this.palQuery(e.target.value);
      if (e.target.classList.contains('ph-q')) this.palQuery(e.target.value, $('.ph-res', this.el));
    }
    onKey(e) {
      if (this.tv && this.tvKey(e)) return;
      if (e.target.closest('.x-pal')) this.palKey(e);
      if (e.target.closest('.x-player') && this.plKey) this.plKey(e);
    }
    onClick(e) {
      if (e.target.closest('.x-player') && this.plClick) { this.plClick(e); return; }
      if (this.menuKind && e.target.closest('.x-pop')) {
        const m = e.target.closest('[data-mi]'); if (m && this.menuPick) { this.menuPick(m.dataset.mi, m); return; }
        const sg = e.target.closest('.c-seg > span'); if (sg) $$(':scope > span', sg.parentElement).forEach(y => y.classList.toggle('on', y === sg));
        const ch = e.target.closest('.c-chip'); if (ch) ch.classList.toggle('on');
        return;
      }
      if (e.target.closest('.c-switch') && !e.target.closest('[data-act]')) { const s = e.target.closest('.c-switch'); s.classList.toggle('on'); return; }
      if (e.target.closest('.c-seg > span') && !e.target.closest('[data-act]')) { const x = e.target.closest('.c-seg > span'); $$(':scope > span', x.parentElement).forEach(y => y.classList.toggle('on', y === x)); return; }
      if (e.target.closest('.c-chip') && !e.target.closest('[data-act]')) { e.target.closest('.c-chip').classList.toggle('on'); return; }
      const el = e.target.closest('[data-act]'); if (!el || !this.el.contains(el)) return;
      const k = el.dataset.act, f = this.ACT[k];
      if (f) f.call(this, el, e);
    }
    dragOver(on) { const f = $('.i-frame', this.el); if (f) { f.classList.toggle('over', on); if (on) NP.wobble(f); } }
    playFile(f) { this.plOpen({ file: f, title: f.name.replace(/\.[^.]+$/, '') }, this.pure() ? this.idleRect() : null); }
  }

  /* 点击动作表：各模块往里加 */
  App.prototype.ACT = {
    nav(el) { this.go(el.dataset.page); },
    back() { this.closeDetail(); },
    detail(el) { this.openDetail(+el.dataset.aid, el); },
    open() { this.q.file.click(); },
    settings() { this.setOpen(); },
    pal(el) { this.palOpen(''); },
  };

  /* ======================================================================
     电视：遥控器焦点。方向键在可操作的东西之间按空间位置移动，回车 = 点击，Esc / 返回键 = 返回。
     焦点样式跟随外观（tvOS 抬起放大、Google TV 描边胶囊），只用缩放和阴影。
     ====================================================================== */
  const FOC = '[data-act]:not(input):not(label), [data-pact], [data-pi], [data-pa]:not(.c-split), [data-ptab], [data-mi], .c-switch, .c-seg > span, .c-chip, .c-slider';
  const A = App.prototype;
  A.tvInit = function () {
    this.el.tabIndex = 0;
    this.el.addEventListener('pointerdown', e => { if (e.target.tagName !== 'INPUT') this.el.focus({ preventScroll: true }); });
    this.el.addEventListener('pointerover', e => { const f = e.target.closest(FOC); if (f && this.tvCands().includes(f)) this.tvFocus(f, true); });
  };
  A.tvLayer = function () {
    if (this.menuKind) return this.q.pop;
    if (this.P && this.P.open) return this.q.player;
    const top = this.overlays[this.overlays.length - 1];
    if (top) return { ob: this.q.ob, set: this.q.set, pal: this.q.pal }[top.kind] || this.el;
    return this.el;
  };
  A.tvCands = function () {
    const root = this.tvLayer();
    return $$(FOC, root).filter(el => {
      if (root === this.el && el.closest('.x-player, .o-win, .x-set, .x-pop, .x-pal')) return false;
      if (root === this.q.pop && el.closest('.c-seg') && !el.matches('.c-seg > span')) return false;
      const r = el.getBoundingClientRect(); if (!r.width || !r.height) return false;
      const cs = getComputedStyle(el); return cs.pointerEvents !== 'none' && cs.visibility !== 'hidden' && +cs.opacity > 0.05;
    });
  };
  A.tvFocus = function (el, quiet) {
    if (!el) return;
    if (this.tvF && this.tvF !== el) this.tvF.classList.remove('tvf');
    el.classList.add('tvf'); this.tvF = el;
    if (!quiet) this.tvReveal(el);
  };
  /* 默认焦点：每一层各有一个最常用的起点 */
  A.tvDefault = function (cs) {
    const pick = sel => cs.find(x => x.matches(sel));
    return pick('[data-pa="play"]') || pick('[data-act="obnext"]') || pick('.x-set .c-navi.on') || pick('.tv-kb [data-act="tvkey"]') || pick('.x-page [data-act="cwplay"], .x-page [data-act="epplay"]') || cs.find(x => x.closest('.x-page, .x-idle')) || cs[0];
  };
  A.tvEnsure = function () {
    if (!this.tv) return;
    clearTimeout(this._tvE);
    this._tvE = setTimeout(() => { const cs = this.tvCands(); if (!this.tvF || !this.tvF.isConnected || !cs.includes(this.tvF)) this.tvFocus(this.tvDefault(cs), false); }, 90);
  };
  /* 把焦点所在的格子滚进可见区域（只滚窗口里面的滚动容器，不滚整页） */
  A.tvReveal = function (el) {
    const k = this.k, r = el.getBoundingClientRect();
    for (let p = el.parentElement; p && p !== this.el; p = p.parentElement) {
      const cs = getComputedStyle(p), pr = p.getBoundingClientRect();
      if (/(auto|scroll)/.test(cs.overflowY) && p.scrollHeight > p.clientHeight + 2) {
        const top = (r.top - pr.top) / k, bot = (r.bottom - pr.top) / k, pad = 70;
        if (top < pad) p.scrollBy({ top: top - pad, behavior: 'smooth' }); else if (bot > p.clientHeight - pad) p.scrollBy({ top: bot - p.clientHeight + pad, behavior: 'smooth' });
      }
      if (/(auto|scroll)/.test(cs.overflowX) && p.scrollWidth > p.clientWidth + 2) {
        const l = (r.left - pr.left) / k, rr = (r.right - pr.left) / k, pad = 60;
        if (p.classList.contains('d-strip')) p.scrollBy({ left: (l + rr) / 2 - p.clientWidth / 2, behavior: 'smooth' });
        else if (l < pad) p.scrollBy({ left: l - pad, behavior: 'smooth' }); else if (rr > p.clientWidth - pad) p.scrollBy({ left: rr - p.clientWidth + pad, behavior: 'smooth' });
      }
    }
  };
  A.tvMove = function (dir) {
    const cs = this.tvCands(); if (!cs.length) return;
    const cur = this.tvF && cs.includes(this.tvF) ? this.tvF : null;
    if (!cur) { this.tvFocus(this.tvDefault(cs)); return; }
    const a = cur.getBoundingClientRect(), ax = a.left + a.width / 2, ay = a.top + a.height / 2;
    let best = null, bs = Infinity;
    cs.forEach(el => {
      /* 嵌在一行里的独立按钮（「也在看」的 ▶）也能单独拿到焦点：在那一行上按 ← 就到 ▶ */
      if (el === cur || ((cur.contains(el) || el.contains(cur)) && !el.matches('.h-pl') && !cur.matches('.h-pl'))) return;
      const b = el.getBoundingClientRect(), bx = b.left + b.width / 2, by = b.top + b.height / 2;
      let main, cross;
      if (dir === 'right') { if (bx <= ax + 2 || b.right <= a.right) return; main = Math.max(0, b.left - a.right); cross = Math.max(0, b.top - a.bottom, a.top - b.bottom); }
      if (dir === 'left') { if (bx >= ax - 2 || b.left >= a.left) return; main = Math.max(0, a.left - b.right); cross = Math.max(0, b.top - a.bottom, a.top - b.bottom); }
      if (dir === 'down') { if (by <= ay + 2 || b.bottom <= a.bottom) return; main = Math.max(0, b.top - a.bottom); cross = Math.max(0, b.left - a.right, a.left - b.right); }
      if (dir === 'up') { if (by >= ay - 2 || b.top >= a.top) return; main = Math.max(0, a.top - b.bottom); cross = Math.max(0, b.left - a.right, a.left - b.right); }
      const sc = main + cross * 3 + (dir === 'left' || dir === 'right' ? Math.abs(by - ay) : Math.abs(bx - ax)) * 0.15;
      if (sc < bs) { bs = sc; best = el; }
    });
    if (best) this.tvFocus(best); else NP.wobble(cur);
  };
  A.tvBack = function () {
    if (this.menuKind) { this.menuClose(); }
    else if (this.P && this.P.open) { if (this.P.lay === 'panel') { this.plLayout('shown'); this.plWake(); } else this.plBack(); }
    else if (this.overlays.length) this.closeTop();
    else if (this.S.detail != null) this.closeDetail();
    else { const first = this.pages()[0]; if (first && this.S.page !== first.key) this.go(first.key); }
    this.tvEnsure();
  };
  A.tvKey = function (e) {
    const D = { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' }, dir = D[e.key];
    const typing = e.target.tagName === 'INPUT';
    if (typing && !dir && e.key !== 'Escape' && e.key !== 'Enter') return false;
    if (typing && (dir === 'left' || dir === 'right')) return false;
    const p = this.P, f = this.tvF;
    /* 播放器：控制层藏着时，左右直接快退快进，其他键先把控制层叫出来 */
    if (p && p.open && !this.menuKind) {
      if (p.lay === 'full' && (dir || e.key === 'Enter')) {
        e.preventDefault();
        if (dir === 'left' || dir === 'right') { this.plSeek(p.t + (dir === 'right' ? 10 : -10)); this.toast(dir === 'right' ? '快进 10 秒' : '快退 10 秒'); return true; }
        this.plWake(); setTimeout(() => this.tvFocus($('[data-pa="play"]', this.q.player)), 60); return true;
      }
      this.plWake();
    }
    if (dir) {
      e.preventDefault();
      if (f && f.classList.contains('c-slider') && (dir === 'left' || dir === 'right') && this.tvCands().includes(f)) {
        if (f.classList.contains('seek') && p) { this.plSeek(p.t + (dir === 'right' ? 10 : -10)); return true; }
        const v = clamp((parseFloat(f.style.getPropertyValue('--v')) || 0) + (dir === 'right' ? 0.1 : -0.1), 0, 1);
        f.style.setProperty('--v', v); f.dispatchEvent(new CustomEvent('nxslide', { detail: v, bubbles: true })); return true;
      }
      this.tvMove(dir); return true;
    }
    if (e.key === 'Enter' || (e.key === ' ' && !typing)) {
      e.preventDefault();
      if (p && p.open && e.key === ' ') { this.plPlay(!p.playing, true); return true; }
      if (f && f.isConnected && !f.classList.contains('c-slider')) { f.click(); if (!RM()) { f.style.transform = 'scale(.94)'; setTimeout(() => (f.style.transform = ''), 120); } }
      this.tvEnsure(); setTimeout(() => this.tvEnsure(), 450);
      return true;
    }
    if (e.key === 'Escape' || (e.key === 'Backspace' && !typing)) { e.preventDefault(); this.tvBack(); return true; }
    return false;
  };

  /* ======================================================================
     页面共用：每种设备的设计语言，以及把窗口缩放进带设备边框的外框
     ====================================================================== */
  /* 每种设备上实际会出现的设计语言：鸿蒙有手机、电脑、平板、折叠屏；手机和平板没有 Fluent / libadwaita；
     折叠屏是 iPhone Duo、鸿蒙（Mate X）、Android（Galaxy / Pixel Fold）和网页；电视只有 tvOS 和 Google TV */
  const ALLOWED = {
    desk: ['apple', 'harmony', 'material', 'fluent', 'adwaita', 'web'], tablet: ['apple', 'harmony', 'material', 'web'], phone: ['apple', 'harmony', 'material', 'web'],
    fold: ['apple', 'harmony', 'material', 'web'], tv: ['apple', 'material'],
  };
  const BEZEL = { phone: 20, tablet: 24, tv: 20, fold: 20 };
  /* wrap 画设备边框，host 是 App 的挂载点；availW / availH 是外框连边框一共能占的宽高；
     rotate：true = 手机转屏时转一下；'fold' = 折叠屏换了姿态（展开 / 合上像书页翻开，进出悬停转 90°） */
  function fitStage(wrap, host, app, availW, availH = Infinity, rotate = false) {
    const W = app.W, H = app.H, fold = app.dev === 'fold', ph = app.dev === 'phone' || (fold && app.fold === 'closed'), bz = BEZEL[app.dev] || 0;
    wrap.classList.toggle('ph', ph); wrap.classList.toggle('land', ph && app.land);
    wrap.classList.toggle('tab', app.dev === 'tablet'); wrap.classList.toggle('tv', app.dev === 'tv');
    wrap.classList.toggle('fold', fold && !ph); wrap.classList.toggle('duo', fold && app.hw === 'duo');
    wrap.dataset.pose = fold ? app.fold : '';
    host.style.width = W + 'px'; host.style.height = H + 'px';
    const s = Math.max(0.1, Math.min(1, (availW - bz) / W, (availH - bz) / H));
    const prevW = parseFloat(wrap.style.width) || 0;
    wrap.style.width = W * s + 'px'; wrap.style.height = H * s + 'px';
    host.style.transform = `scale(${s})`;
    if (rotate === 'fold') {
      const was = wrap._pose; wrap._pose = app.fold;
      if (!was || was === app.fold || RM()) return s;
      if (was === 'tent' || app.fold === 'tent') {
        const d = app.fold === 'tent' ? 90 : -90;
        wrap.style.transform = `rotate(${d}deg) scale(.7)`;
        tween(TOKENS.hero, p => { wrap.style.transform = `rotate(${d * (1 - p)}deg) scale(${0.7 + 0.3 * p})`; }, () => { wrap.style.transform = ''; });
      } else if (prevW) {
        /* 像书页一样翻开 / 合上：外框的宽度从上一个姿态弹到这一个，新的布局从折痕那边露出来（不拉伸内容） */
        const w1 = W * s, tk = (wrap._ft = (wrap._ft || 0) + 1);
        wrap.style.width = prevW + 'px';
        tween(TOKENS.hero, p => { if (wrap._ft === tk) wrap.style.width = prevW + (w1 - prevW) * p + 'px'; }, () => { if (wrap._ft === tk) wrap.style.width = w1 + 'px'; });
      }
      return s;
    }
    if (fold) wrap._pose = app.fold;
    if (rotate && !RM()) {
      const on = app.land;
      wrap.style.transform = `rotate(${on ? 90 : -90}deg) scale(.7)`;
      tween(TOKENS.hero, p => { wrap.style.transform = `rotate(${(on ? 90 : -90) * (1 - p)}deg) scale(${0.7 + 0.3 * p})`; }, () => { wrap.style.transform = ''; });
    }
    return s;
  }

  return { App, ALLOWED, FOLD, hwOf, fitStage, I, P, STILL, ANIME, ERASED, byId, imgOf, CW, TODAY, NOW, hOf, DAYS, TODAY_IDX, WEEK, RECENTS, DL, COMMENTS, LINES, SUBS, DUR, CH, PEAKS, clamp, fmt, rateLabel, statusOf, esc, colorOf, pcard };
})();
