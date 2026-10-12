/* =====================================================================
   NipaPlay Next · 交互原型
   桌面窗口 1280×800 / 手机 390×844，所有界面都可以点；模块开关、主题色、首次配置会被记住。
   ===================================================================== */
(() => {
  'use strict';
  const {
    $, $$, svg, kv, root, TOKENS, RM, tween, cancel, scaleTo, bindJelly, wobble, autoJelly,
    enter, leave, setShown, reconcile, toast, setTheme, MODS, MOD, PRESETS, pagesFor, scaleOf,
  } = NP;

  const EMBED = new URLSearchParams(location.search).has('embed');
  const Store = NP.createStore(PRESETS.anime.mods, 'app.mods');

  /* ------------------------------------------------------------------
     数据（海报取自原项目截图；评分取自原项目界面）
     ------------------------------------------------------------------ */
  const P = i => `assets/posters/p${i}.jpg`;
  const STILL = 'assets/still1.jpg';
  const ANIME = [
    { id: 1, t: '寒蝉鸣泣之时 解', p: 1, eps: 24, w: 1, r: 8.4, tags: ['悬疑', '推理'] },
    { id: 2, t: '只有神知道的世界 女神篇', p: 2, eps: 12, w: 11, r: 7.9, tags: ['恋爱', '喜剧'] },
    { id: 3, t: '【我推的孩子】 第三季', p: 3, eps: 11, w: 2, r: 7.8, tags: ['偶像', '剧情'] },
    { id: 4, t: '超时空辉夜姬！', p: 4, eps: 1, w: 0, r: 7.7, tags: ['科幻', '音乐'], movie: true },
    { id: 5, t: '咒术回战 死灭回游', p: 5, eps: 12, w: 3, r: 7.6, tags: ['战斗', '奇幻'] },
    { id: 6, t: '为美好的世界献上祝福！3', p: 6, eps: 11, w: 11, r: 7.9, tags: ['喜剧', '异世界'] },
    { id: 7, t: '葬送的芙莉莲 第二季', p: 7, eps: 10, w: 3, r: 8.4, tags: ['奇幻', '冒险'] },
    { id: 8, t: '剧场版 魔法少女小圆', p: 8, eps: 1, w: 0, r: 8.4, tags: ['魔法少女', '剧情'], movie: true },
    { id: 9, t: 'Fate/strange Fake', p: 9, eps: 13, w: 3, r: 8.0, tags: ['战斗', '奇幻'] },
    { id: 10, t: '地狱乐 第二季', p: 10, eps: 12, w: 2, r: 6.7, tags: ['战斗', '历史'] },
    { id: 11, t: '鬼灭之刃 无限城篇', p: 11, eps: 1, w: 0, r: 0, tags: ['战斗'], movie: true },
    { id: 12, t: '寒蝉鸣泣之时 礼', p: 12, eps: 5, w: 0, r: 7.3, tags: ['悬疑'] },
    { id: 13, t: '魔都精兵的奴隶 第二季', p: 13, eps: 12, w: 0, r: 6.1, tags: ['战斗', '奇幻'] },
    { id: 14, t: '现在多闻君是哪一面！？', p: 14, eps: 12, w: 0, r: 5.8, tags: ['恋爱', '喜剧'] },
    { id: 15, t: '优雅贵族的休假指南。', p: 15, eps: 12, w: 0, r: 4.9, tags: ['异世界', '日常'] },
    { id: 16, t: '厄里斯的圣杯', p: 16, eps: 12, w: 0, r: 5.4, tags: ['悬疑', '历史'] },
  ];
  const ERASED = { id: 0, t: '只有我不存在的城市', eps: 12, w: 0, still: STILL };
  const byId = id => (+id === 0 ? ERASED : ANIME.find(a => a.id === +id));
  const CW = [
    { id: 0, ep: 1, prog: 0.38, left: 15, img: STILL, pos: '50% 50%' },
    { id: 7, ep: 4, prog: 0.35, left: 16, img: P(7), pos: '50% 30%' },
    { id: 3, ep: 3, prog: 0.2, left: 19, img: P(3), pos: '50% 25%' },
    { id: 5, ep: 4, prog: 0.8, left: 4, img: P(5), pos: '50% 30%' },
  ];
  const DAYS = ['周一', '周二', '周三', '周四', '周五', '周六', '周日'];
  const TODAY = (new Date().getDay() + 6) % 7;
  const TIMES = ['22:00', '22:30', '23:00', '23:30', '00:00', '00:30', '01:05'];
  const schedule = d => { const n = ANIME.length; return Array.from({ length: 7 }, (_, i) => ANIME[(d * 3 + i * 2) % n]); };
  const SERVERS = [{ n: 'Jellyfin · 家里 NAS', ic: 'assets/jellyfin.svg', ids: [7, 9, 3, 5, 10, 6] }, { n: 'Emby · 书房', ic: 'assets/emby.svg', ids: [1, 12, 2] }, { n: 'WebDAV · 网盘', ic: null, ids: [8, 11] }];
  const DEVICES = [{ n: '客厅的 Mac mini', ids: [6, 7, 13, 14] }, { n: '书房的 PC', ids: [1, 2, 12] }];
  const RECENTS = [
    { t: '[Nekomoe] Sousou no Frieren S2 - 03 [1080p].mkv', prog: 0.62, left: '9:12', aid: 7, ep: 3 },
    { t: 'Boku dake ga Inai Machi - 01.mkv', prog: 0.38, left: '15:03', aid: 0, ep: 1 },
    { t: 'Vacation_2026_Kyoto.mp4', prog: 0.9, left: '0:41', aid: null },
  ];
  const DL = [
    { t: '[LoliHouse] Sousou no Frieren S2 - 04 [1080p].mkv', size: '1.2 GB', prog: 0.64, speed: 8.4, on: true },
    { t: '[Nekomoe] Kaguya-sama - 01 [1080p].mkv', size: '860 MB', prog: 0.22, speed: 3.1, on: true },
    { t: '[ANi] Jujutsu Kaisen - 05 [1080p].mp4', size: '540 MB', prog: 1, speed: 0, on: false },
  ];
  const COMMENTS = [
    { n: '路过的观众', s: 9, t: '作画一直很稳，节奏舒服，适合慢慢看。' },
    { n: '追番中', s: 8, t: '这一季的配乐比上一季更好，期待后面的展开。' },
    { n: 'Nipa 用户', s: 7, t: '前两集铺垫有点长，第三集开始好看起来了。' },
  ];
  const LINES = [['前方高能！！', ''], ['名场面来了', ''], ['这段作画太顶了', '#FFD24A'], ['OP 好听', ''], ['再上映——', ''], ['来了来了', '#8EE3FF'], ['泪目', ''], ['这里有伏笔', ''], ['下周见', ''], ['快跑！', '#FFB3C7']];

  /* ------------------------------------------------------------------
     状态
     ------------------------------------------------------------------ */
  const S = {
    device: kv.get('app.device') ? (kv.get('app.device') === 'phone' ? 'phone' : 'desk') : (document.getElementById('stage').clientWidth < 700 ? 'phone' : 'desk'),
    page: 'home', detail: null, detailTab: 'eps', scrollMem: {},
    lib: { seg: 'local', filter: 'all', sort: 'recent', view: 'grid', server: 0, device: 0 },
    newDay: TODAY,
    player: null, mode: 'closed',
    setSec: 'optional',
  };

  /* ------------------------------------------------------------------
     舞台缩放
     ------------------------------------------------------------------ */
  const stage = $('#stage'), deskFrame = $('#deskFrame'), phoneFrame = $('#phoneFrame');
  const app = $('#app'), phone = $('#phone');
  function place(f, w, h) {
    const W = stage.clientWidth, H = stage.clientHeight, pad = W < 700 ? 12 : 40;
    const s = Math.min((W - pad) / w, (H - pad) / h, 1);
    f.style.width = w + 'px'; f.style.height = h + 'px';
    f.style.transform = `translate(${(W - w * s) / 2}px, ${(H - h * s) / 2}px) scale(${s})`;
    f.style.setProperty('--s', s); f._s = s;
  }
  function fitStage() {
    deskFrame.style.display = S.device === 'desk' ? '' : 'none';
    phoneFrame.style.display = S.device === 'phone' ? '' : 'none';
    if (S.device === 'desk') place(deskFrame, 1280, 800);
    else place(phoneFrame, M.land ? 844 : 390, M.land ? 390 : 844);
  }
  new ResizeObserver(fitStage).observe(stage);

  /* ------------------------------------------------------------------
     小工具
     ------------------------------------------------------------------ */
  const fmt = s => { s = Math.max(0, Math.floor(s)); const m = Math.floor(s / 60), ss = String(s % 60).padStart(2, '0'); return `${m}:${ss}`; };
  const fmt2 = s => { s = Math.max(0, Math.floor(s)); return `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`; };
  const pageName = k => ({ home: '首页', new: '新番', lib: '媒体库', dl: '下载' }[k] || '');
  const statusOf = a => (a.w === 0 ? '未看' : a.w >= a.eps ? '看完' : `已看 ${a.w} / ${a.eps}`);
  function rel(el, host = app) {
    const s = scaleOf(host), r = el.getBoundingClientRect(), R = host.getBoundingClientRect();
    return { x: (r.left - R.left) / s, y: (r.top - R.top) / s, w: r.width / s, h: r.height / s };
  }
  function fly(img, a, b, r0, r1, done, host = app, token = TOKENS.hero) {
    if (RM()) { done && done(); return; }
    const c = document.createElement('div');
    c.style.cssText = 'position:absolute;left:0;top:0;z-index:58;overflow:hidden;pointer-events:none;will-change:transform;background:#000';
    c.innerHTML = `<img src="${img}" alt="" style="width:100%;height:100%;object-fit:cover">`;
    host.appendChild(c);
    const L = (u, v, p) => u + (v - u) * p;
    tween(token, p => {
      c.style.width = L(a.w, b.w, p) + 'px'; c.style.height = L(a.h, b.h, p) + 'px';
      c.style.transform = `translate(${L(a.x, b.x, p)}px, ${L(a.y, b.y, p)}px)`;
      c.style.borderRadius = Math.max(0, L(r0, r1, p)) + 'px';
    }, () => { done && done(); c.remove(); });
  }
  function animIn(el, dy = 12) {
    if (RM()) return;
    el.style.opacity = '0';
    tween(TOKENS.smooth, p => { el.style.opacity = String(Math.min(1, p)); el.style.transform = `translateY(${(1 - p) * dy}px)`; }, () => { el.style.transform = ''; el.style.opacity = ''; });
  }
  function stagger(els, d0 = 60) {
    if (RM()) return;
    els.forEach((el, i) => {
      el.style.opacity = '0'; el.style.transform = 'translateY(14px)';
      setTimeout(() => tween(TOKENS.smooth, p => { el.style.opacity = String(Math.min(1, p)); el.style.transform = `translateY(${(1 - p) * 14}px)`; }, () => { el.style.transform = ''; el.style.opacity = ''; }), d0 + i * 35);
    });
  }
  function popIn(el, origin = '50% 0', token = TOKENS.bouncy) {
    el.style.display = ''; el.style.transformOrigin = origin;
    cancel(el._tw);
    el._tw = tween(token, p => { el.style.transform = `scale(${0.92 + 0.08 * p})`; el.style.opacity = String(Math.min(1, p * 2)); });
  }
  function popOut(el, done) {
    cancel(el._tw);
    el._tw = tween(TOKENS.snappy, p => { el.style.transform = `scale(${1 - 0.05 * Math.min(1, p)})`; el.style.opacity = String(Math.max(0, 1 - p * 1.4)); }, () => { el.style.display = 'none'; done && done(); });
  }
  const posterCard = (a, sub = '', badge = '') => `<div class="pcard"><div class="poster" data-act="detail" data-aid="${a.id}"><img src="${P(a.p)}" alt="">${badge ? `<span class="badge">${badge}</span>` : ''}</div><div class="tt">${a.t}</div>${sub ? `<div class="cap2">${sub}</div>` : ''}</div>`;

  /* ------------------------------------------------------------------
     桌面窗口骨架
     ------------------------------------------------------------------ */
  app.innerHTML = `
    <aside class="wf-side app-side">
      <div class="dots3"><i></i><i></i><i></i></div>
      <div class="app-nav"></div>
      <div class="app-mini-slot"></div>
      <div class="side-foot"><span class="avatar" style="background-image:url(assets/girl.png);background-size:170%;background-position:50% 12%"></span><div style="flex:1;line-height:1.25;min-width:0"><div style="font-size:13px;font-weight:650">Nipa</div><div class="cap2 acct" style="font-size:11px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis"></div></div><span class="ib" data-act="settings" title="设置（⌘,）" data-spec="设置 · S">${svg('gear')}</span></div>
    </aside>
    <section class="wf-main app-main">
      <div class="wf-toolbar app-tb"></div>
      <div class="app-scroll"><div class="app-page"></div></div>
    </section>
    <div class="float-mini" style="display:none"></div>
    <div class="pip" style="display:none"></div>
    <div class="player app-player isl" style="display:none"></div>
    <div class="app-dim" style="display:none"></div>
    <div class="app-modal settings" style="display:none"><div class="win">
      <aside class="wf-side" style="width:230px"><div class="dots3"><i></i><i></i><i></i></div><div class="search" style="margin:0 2px 10px;height:28px">${svg('search')}搜索设置</div><div class="set-side" style="display:flex;flex-direction:column;gap:1px"></div></aside>
      <div class="wf-main"><span class="ib set-close" data-act="close-settings" title="关闭（Esc）">${svg('x')}</span><div class="wf-toolbar" style="height:44px"></div><div class="set-content"></div></div>
    </div></div>
    <div class="app-modal ob" style="display:none"><div class="win col"></div></div>
    <div class="palette" style="display:none"><div class="pin">${svg('search')}<input placeholder="搜索番剧、设置和操作" aria-label="搜索"><kbd>esc</kbd></div><div class="res"></div></div>
    <div class="sheet-r" style="display:none"></div>
    <div class="dialog" style="display:none"></div>
    <div class="menu" style="display:none"></div>
    <div class="dropcover">松开即可播放</div>
    <input type="file" class="file-in" accept="video/*,.mkv,.ass,.srt" style="display:none">`;
  const side = $('.app-side', app), nav = $('.app-nav', app), slot = $('.app-mini-slot', app);
  const main = $('.app-main', app), tb = $('.app-tb', app), scroller = $('.app-scroll', app), page = $('.app-page', app);
  const floatMini = $('.float-mini', app), pip = $('.pip', app), player = $('.app-player', app);
  const dim = $('.app-dim', app), setModal = $('.app-modal.settings', app), obModal = $('.app-modal.ob', app);
  const palette = $('.palette', app), sheet = $('.sheet-r', app), dialog = $('.dialog', app), menuEl = $('.menu', app), dropcover = $('.dropcover', app);
  const fileIn = $('.file-in', app);

  /* ------------------------------------------------------------------
     侧栏 / 工具栏
     ------------------------------------------------------------------ */
  let sideShown = null;
  function setSide(show, animate) {
    if (show === sideShown) return;
    const first = sideShown === null; sideShown = show;
    const end = () => { side.style.width = show ? '' : '0px'; side.style.padding = show ? '' : '0'; side.style.borderRightWidth = show ? '' : '0'; };
    if (first || !animate || RM()) { end(); return; }
    side.style.padding = ''; side.style.borderRightWidth = '';
    const from = show ? 0 : 220, to = show ? 220 : 0;
    tween(TOKENS.smooth, p => { side.style.width = from + (to - from) * Math.min(1, p) + 'px'; }, end);
  }
  function renderChrome(animate = true) {
    const pages = pagesFor(Store.mods);
    if (!pages.find(p => p.key === S.page)) { S.page = pages.length ? pages[0].key : 'idle'; S.detail = null; }
    if (S.detail != null && !Store.has('library') && !Store.has('anime')) S.detail = null;
    setSide(pages.length >= 2, animate);
    reconcile(nav, pages, it => {
      const d = document.createElement('div');
      d.className = 'nav-item'; d.dataset.act = 'nav'; d.dataset.page = it.key;
      d.innerHTML = svg(it.icon) + `<span>${it.name}</span>` + (it.key === 'dl' ? '<span style="margin-left:auto;font-size:11px;color:var(--text-3)" class="dl-count">2</span>' : '');
      return d;
    }, (el, it) => el.classList.toggle('on', it.key === S.page), animate);
    $('.acct', app).textContent = Store.has('sync') ? '弹弹play · Bangumi' : '本地用户';
    main.classList.toggle('plaid-uniform', !pages.length);
    renderToolbar();
    renderMini();
  }
  function renderToolbar() {
    const pages = pagesFor(Store.mods), hasSide = pages.length >= 2;
    let h = '';
    if (!hasSide) h += '<div class="dots3" style="padding:0 8px 0 0"><i></i><i></i><i></i></div>';
    if (S.detail != null) h += `<span class="ib" data-act="back" title="返回" data-spec="返回 · 图标按钮 S">${svg('chev-l')}</span><span class="crumb">${pageName(S.page)}</span>`;
    else if (!hasSide && pages.length === 1) h += `<span class="crumb" style="font-weight:650;color:var(--text-2)">${pageName(pages[0].key)}</span>`;
    h += '<span class="grow"></span>';
    if (pages.length) h += `<div class="search" style="width:260px" data-act="palette" data-spec="搜索框 · M 32 · ⌘K">${svg('search')}搜索番剧、文件、设置<kbd>⌘K</kbd></div><span class="b" data-act="open-file" title="打开文件（⌘O）">${svg('plus')}打开</span>`;
    if (!hasSide) h += `${Store.has('sync') ? '<span class="avatar" style="width:28px;height:28px;background-image:url(assets/girl.png);background-size:170%;background-position:50% 12%"></span>' : ''}<span class="ib" data-act="settings" title="设置（⌘,）">${svg('gear')}</span>`;
    tb.innerHTML = h;
    autoJelly(tb);
  }

  /* ------------------------------------------------------------------
     页面
     ------------------------------------------------------------------ */
  function homeHTML() {
    const cw = CW.map((c, i) => { const a = byId(c.id); return `<div class="cw"><div class="thumb" data-act="cw" data-i="${i}"><img src="${c.img}" alt="" style="object-position:${c.pos}"><div class="prog"><b style="width:${c.prog * 100}%"></b></div><div class="playhint"><span>${svg('play')}</span></div></div><div class="tt">${a.t}</div><div class="cap2">第 ${c.ep} 集 · 剩 ${c.left} 分钟</div></div>`; }).join('');
    const today = schedule(TODAY).map(a => posterCard(a, '', a.id % 3 === 0 ? '新' : '')).join('');
    const recent = Store.has('library') ? `<div class="sec-title sec-gap"><span class="t2">媒体库 · 最近添加</span><span class="more click" data-act="nav" data-page="lib">全部 ›</span></div><div class="pgrid">${[1, 2, 9, 10, 12, 6, 8].map(id => posterCard(byId(id))).join('')}</div>` : '';
    return `<div class="lt">首页</div>
      <div class="sec-title"><span class="t2">继续观看</span></div><div class="cw-row">${cw}</div>
      <div class="sec-title sec-gap"><span class="t2">今日更新</span><span class="cap2">${DAYS[TODAY]} · 7 部</span><span class="more click" data-act="nav" data-page="new">新番表 ›</span></div><div class="pgrid">${today}</div>${recent}`;
  }
  function newHTML() {
    const hero = byId(7), list = schedule(S.newDay);
    return `<div class="lt">新番<span class="sketch">草图</span></div>
      <div class="banner"><div class="bg" style="background-image:url(${P(hero.p)})"></div><div class="shade"></div><div class="pv"><img src="${P(hero.p)}" alt=""></div>
        <div class="tx"><span class="cap2" style="color:rgba(255,255,255,.75)">本季推荐 · 每周六 23:00 更新</span><b>${hero.t}</b><div style="display:flex;gap:10px;margin-top:12px"><span class="b L p" data-act="detail" data-aid="${hero.id}">查看详情</span><span class="b" style="background:rgba(255,255,255,.18);color:#fff" data-act="play" data-aid="${hero.id}" data-ep="4">${svg('play')}继续 第 4 集</span></div></div></div>
      <div style="display:flex;align-items:center;gap:12px;margin-bottom:18px"><span class="seg">${DAYS.map((d, i) => `<span data-act="new-day" data-d="${i}" class="${i === S.newDay ? 'on' : ''}">${i === TODAY ? '今天' : d}</span>`).join('')}</span><span class="cap2">${list.length} 部 · 按更新时间排序</span></div>
      <div class="pgrid big">${list.map((a, i) => posterCard(a, `${TIMES[i]} 更新`)).join('')}</div>`;
  }
  function libItems() {
    let items = ANIME.slice();
    const f = S.lib.filter;
    if (f === 'watching') items = items.filter(a => a.w > 0 && a.w < a.eps);
    if (f === 'unwatched') items = items.filter(a => a.w === 0);
    if (f === 'done') items = items.filter(a => a.w >= a.eps);
    if (S.lib.sort === 'name') items.sort((a, b) => a.t.localeCompare(b.t, 'zh'));
    if (S.lib.sort === 'rating') items.sort((a, b) => b.r - a.r);
    return items;
  }
  function libHTML() {
    const segs = [['local', '本地'], Store.has('sources') && ['server', '服务器'], Store.has('remote') && ['share', '共享']].filter(Boolean);
    if (!segs.find(s => s[0] === S.lib.seg)) S.lib.seg = 'local';
    const sortName = { recent: '最近观看', name: '名称', rating: '评分' }[S.lib.sort];
    let body = '';
    if (S.lib.seg === 'local') {
      const all = ANIME.length, watching = ANIME.filter(a => a.w > 0 && a.w < a.eps).length, un = ANIME.filter(a => a.w === 0).length, done = ANIME.filter(a => a.w >= a.eps).length;
      const chip = (k, n, c) => `<span class="b S${S.lib.filter === k ? ' on' : ''}" data-act="lib-filter" data-f="${k}">${n} ${c}</span>`;
      const items = libItems();
      body = `<div class="chiprow" style="margin-bottom:20px">${chip('all', '全部', all)}${chip('watching', '在看', watching)}${chip('unwatched', '未看', un)}${chip('done', '看完', done)}</div>` +
        (S.lib.view === 'grid'
          ? `<div class="pgrid">${items.map(a => posterCard(a, statusOf(a), a.w > 0 && a.w < a.eps ? `${a.w}/${a.eps}` : '')).join('')}</div>`
          : `<div>${items.map(a => `<div class="lrow" data-act="detail" data-aid="${a.id}"><div class="poster"><img src="${P(a.p)}" alt=""></div><div class="tx"><div class="tt">${a.t}</div><div class="cap2">${statusOf(a)}${a.r ? ` · ★ ${a.r}` : ''}</div><div class="dl-bar" style="max-width:260px"><b style="width:${(a.w / a.eps) * 100}%"></b></div></div><span class="b S" data-act="play" data-aid="${a.id}" data-ep="${Math.min(a.eps, a.w + 1)}">${a.w ? '继续' : '播放'}</span></div>`).join('')}</div>`);
    } else if (S.lib.seg === 'server') {
      const sv = SERVERS[S.lib.server];
      body = `<div class="chiprow" style="margin-bottom:20px">${SERVERS.map((s, i) => `<span class="b S${i === S.lib.server ? ' on' : ''}" data-act="lib-server" data-i="${i}">${s.ic ? `<img src="${s.ic}" alt="" style="width:14px;height:14px">` : svg('server', 'i', 'width:14px;height:14px')}${s.n}</span>`).join('')}</div>
        <div class="pgrid">${sv.ids.map(id => posterCard(byId(id), sv.n.split(' · ')[0])).join('')}</div>`;
    } else {
      const dv = DEVICES[S.lib.device];
      body = `<div class="chiprow" style="margin-bottom:20px">${DEVICES.map((d, i) => `<span class="b S${i === S.lib.device ? ' on' : ''}" data-act="lib-device" data-i="${i}">${svg('wifi', 'i', 'width:14px;height:14px')}${d.n}</span>`).join('')}</div>
        <div class="pgrid">${dv.ids.map(id => posterCard(byId(id), '局域网')).join('')}</div>`;
    }
    return `<div style="display:flex;align-items:center;gap:14px;margin:6px 0 20px">
        <div class="lt" style="margin:0">媒体库</div>
        ${segs.length > 1 ? `<span class="seg" data-spec="分段控件 · M · 选项随模块变化">${segs.map(([k, n]) => `<span data-act="lib-seg" data-seg="${k}" class="${S.lib.seg === k ? 'on' : ''}">${n}</span>`).join('')}</span>` : ''}
        <span style="flex:1"></span>
        ${S.lib.seg === 'local' ? `<span class="b" data-act="lib-sort">${svg('sort')}${sortName}${svg('chev-d', 'i', 'width:14px')}</span>
        <span class="seg"><span data-act="lib-view" data-v="grid" class="${S.lib.view === 'grid' ? 'on' : ''}" style="padding:0 9px">${svg('grid', 'i', 'width:15px;height:15px')}</span><span data-act="lib-view" data-v="list" class="${S.lib.view === 'list' ? 'on' : ''}" style="padding:0 9px">${svg('list', 'i', 'width:15px;height:15px')}</span></span>` : ''}
        <span class="b" data-act="lib-manage">${svg('folder')}管理</span>
      </div>${body}`;
  }
  function dlHTML() {
    return `<div style="display:flex;align-items:center;gap:12px;margin:6px 0 22px"><div class="lt" style="margin:0">下载<span class="sketch">草图</span></div><span style="flex:1"></span><span class="b" data-act="dl-add">${svg('plus')}添加任务</span></div>
      <div class="row-list">${DL.map((d, i) => `<div class="row" style="min-height:70px"><div class="mi">${svg(d.prog >= 1 ? 'check' : 'download')}</div><div class="tx"><div class="tt" style="font-weight:600">${d.t}</div><div class="cap2 dl-meta" data-i="${i}">${dlMeta(d)}</div><div class="dl-bar"><b data-i="${i}" style="width:${d.prog * 100}%"></b></div></div>${d.prog >= 1 ? `<span class="b S" data-act="dl-play" data-i="${i}">播放</span>` : `<span class="ib M" data-act="dl-toggle" data-i="${i}" title="${d.on ? '暂停' : '继续'}">${svg(d.on ? 'pause' : 'play')}</span>`}</div>`).join('')}</div>
      <p class="cap2" style="margin-top:14px">下载完成的文件会自动加入媒体库。</p>`;
  }
  const dlMeta = d => d.prog >= 1 ? `${d.size} · 已完成` : `${d.size} · ${Math.round(d.prog * 100)}% · ${d.on ? d.speed.toFixed(1) + ' MB/s' : '已暂停'}`;
  function idleHTML() {
    return `<div class="app-idle">
      <div data-spec="拖放区 · 自绘表面 540×250" style="width:540px;height:250px;border-radius:22px;border:1.5px dashed var(--line-2);background:var(--wf-bg);display:flex;flex-direction:column;align-items:center;justify-content:center;gap:6px">
        ${svg('film', 'i', 'width:44px;height:44px;color:var(--text-3);stroke-width:1.6')}
        <div class="t2" style="font-size:19px;margin-top:6px">把视频拖到这里</div>
        <div class="cap2">支持文件、文件夹和字幕文件，也可以按 ⌘O。原型里可以真的打开本地视频</div>
        <div style="display:flex;gap:10px;margin-top:18px;align-items:center">
          <span class="b L p" data-act="open-file">${svg('folder')}打开文件</span><span class="b" data-act="open-file">打开文件夹</span><span class="b plain" data-act="open-link"><span>打开链接…</span></span>
        </div>
      </div>
      <div style="width:540px;margin-top:30px">
        <div style="display:flex;justify-content:space-between;align-items:baseline;margin:0 4px 8px"><span class="t2" style="font-size:14px">最近播放</span><span class="cap2">清除</span></div>
        <div class="row-list">${RECENTS.map((r, i) => `<div class="row click" style="min-height:44px" data-act="recent" data-i="${i}">${svg('film', 'i', 'color:var(--text-3)')}<div class="tx"><div class="tt" style="font-weight:500;font-size:13px">${r.t}</div><div class="sl" style="height:3px;margin-top:6px;flex:none;width:100%"><b style="width:${r.prog * 100}%"></b></div></div><span class="cap2 nowrap">剩 ${r.left}</span></div>`).join('')}</div>
      </div></div>`;
  }
  function detailHTML(a) {
    const anime = Store.has('anime'), next = Math.min(a.eps, a.w + 1);
    const tabs = anime ? [['eps', '剧集'], ['info', '简介'], ['cmt', '评论'], ['staff', '制作']] : [['eps', '剧集'], ['file', '文件信息']];
    if (!tabs.find(t => t[0] === S.detailTab)) S.detailTab = 'eps';
    const meta = anime
      ? `<div style="display:flex;gap:10px;align-items:center;font-size:13px;color:var(--text-2)">${a.r ? `<b style="color:var(--text);font-size:15px">★ ${a.r}</b> Bangumi <span class="faint">·</span>` : ''} ${a.tags.map(t => `<span class="chip" style="height:24px">${t}</span>`).join('')}</div>`
      : `<div class="cap2" style="font-size:13px">本地文件 · 未开启「追番」模块，所以不显示评分和简介</div>`;
    return `<div class="dt-bg" style="background-image:url(${P(a.p)})"></div>
      <div class="dt-head">
        <div class="poster" data-spec="封面 2:3 · 200×300 · 共享元素"><img src="${P(a.p)}" alt=""></div>
        <div style="flex:1;padding-top:14px">
          <div class="cap2 st" style="font-size:13px">${a.movie ? '剧场版' : 'TV 动画'} · 共 ${a.eps} 话 · ${statusOf(a)}</div>
          <div class="t1 st" style="font-size:34px;margin:8px 0 12px">${a.t}</div>
          <div class="st">${meta}</div>
          <div class="st" style="margin:18px 0 22px;max-width:560px"><div class="bar" style="width:100%;margin-bottom:8px"></div><div class="bar" style="width:94%;margin-bottom:8px"></div><div class="bar" style="width:60%"></div></div>
          <div class="st" style="display:flex;gap:10px;align-items:center">
            <span class="b L p" data-act="play" data-aid="${a.id}" data-ep="${next}" data-spec="主要按钮 · L · 每屏唯一">${svg('play')}${a.w ? `继续 第 ${next} 集` : '开始观看'}</span>
            ${anime ? `<span class="b" data-act="dt-status">${svg('heart')}<span class="dt-st">${a.w ? (a.w >= a.eps ? '看过' : '在看') : '想看'}</span>${svg('chev-d', 'i', 'width:14px')}</span><span class="ib M" data-act="dt-rate" title="评分">${svg('star')}</span>` : ''}
            <span class="ib M" data-act="dt-more" title="更多">${svg('more')}</span>
          </div>
        </div>
      </div>
      <div class="dt-body st" style="margin-top:30px">
        <div style="display:flex;align-items:center;gap:12px;margin-bottom:16px"><span class="seg">${tabs.map(([k, n]) => `<span data-act="dt-tab" data-tab="${k}" class="${S.detailTab === k ? 'on' : ''}">${n}</span>`).join('')}</span></div>
        <div class="dt-tab-body">${detailTab(a)}</div>
      </div>`;
  }
  function detailTab(a) {
    const t = S.detailTab;
    if (t === 'eps') {
      const n = Math.min(a.eps, 15);
      return `<div class="ep-grid">${Array.from({ length: n }, (_, i) => {
        const e = i + 1, watched = e <= a.w, cur = e === a.w + 1;
        return `<div class="ep"><div class="thumb" data-act="play" data-aid="${a.id}" data-ep="${e}">${watched || cur ? `<img src="${P(a.p)}" alt="" style="object-position:50% ${15 + (i * 23) % 70}%;${watched ? 'filter:saturate(.55)' : ''}">` : `<span class="num">${String(e).padStart(2, '0')}</span>`}${watched ? `<span class="badge" style="left:8px;right:auto">${svg('check', 'i', 'width:12px;height:12px;stroke-width:3')}</span>` : ''}${cur ? '<div class="prog"><b style="width:35%"></b></div>' : ''}</div><div class="tt">${cur ? `<span class="acc">第 ${e} 话 · 继续观看</span>` : `第 ${e} 话`}</div></div>`;
      }).join('')}</div>`;
    }
    if (t === 'info') return `<div style="max-width:720px"><div class="bar" style="margin-bottom:10px"></div><div class="bar" style="width:96%;margin-bottom:10px"></div><div class="bar" style="width:90%;margin-bottom:10px"></div><div class="bar" style="width:70%;margin-bottom:24px"></div><div class="chiprow">${a.tags.map(x => `<span class="chip">${x}</span>`).join('')}<span class="chip">共 ${a.eps} 话</span></div></div>`;
    if (t === 'cmt') return `<div style="max-width:760px">${!Store.has('sync') ? '<div class="callout" style="margin:0 0 10px"><p>开启「账号同步」并登录 Bangumi 后，可以在这里发表短评。</p></div>' : '<div class="search" style="height:40px;margin-bottom:8px;cursor:text">写一条短评…</div>'}${COMMENTS.map(c => `<div class="cmt"><span class="avatar" style="background:var(--wf-block-2)"></span><div><b>${c.n}</b><span class="stars">${'★'.repeat(Math.round(c.s / 2))}${'☆'.repeat(5 - Math.round(c.s / 2))}</span><div class="body" style="margin-top:4px">${c.t}</div></div></div>`).join('')}</div>`;
    if (t === 'staff') return `<div class="staff">${['原作', '导演', '系列构成', '角色设计', '音乐', '动画制作'].map(k => `<div><span>${k}</span><i class="bar" style="width:90px;display:block;margin-top:6px"></i></div>`).join('')}</div>`;
    if (t === 'file') return `<div class="row-list" style="max-width:640px">${Array.from({ length: Math.min(a.eps, 4) }, (_, i) => `<div class="row">${svg('film', 'i', 'color:var(--text-3)')}<div class="tx"><div class="tt" style="font-weight:500;font-size:13px">${a.t} - ${String(i + 1).padStart(2, '0')} [1080p].mkv</div><div class="ds">HEVC · 1920×1080 · ASS 字幕</div></div></div>`).join('')}</div>`;
    return '';
  }

  function pageHTML(k) { return { home: homeHTML, new: newHTML, lib: libHTML, dl: dlHTML, idle: idleHTML }[k](); }
  function renderPage(animate = true) {
    page.classList.toggle('idle', S.page === 'idle' && S.detail == null);
    page.innerHTML = S.detail != null ? detailHTML(byId(S.detail)) : pageHTML(S.page);
    autoJelly(page);
    if (animate) animIn(page);
  }
  function go(k) {
    if (S.page === k && S.detail == null) return;
    S.scrollMem[S.page] = scroller.scrollTop;
    S.page = k; S.detail = null;
    $$('.nav-item', nav).forEach(el => el.classList.toggle('on', el.dataset.page === k));
    renderToolbar(); renderPage(); scroller.scrollTop = S.scrollMem[k] || 0;
  }
  function openDetail(aid, src) {
    const a = byId(aid); if (!a || aid == 0) return;
    const from = src ? rel(src) : null;
    if (S.detail == null) S.scrollMem[S.page] = scroller.scrollTop;
    S.detail = +aid; S.detailTab = 'eps';
    renderToolbar(); renderPage(false); scroller.scrollTop = 0;
    const cover = $('.dt-head .poster', page);
    stagger($$('.st', page));
    if (from && cover && !RM()) {
      cover.style.visibility = 'hidden'; src.style.visibility = 'hidden';
      fly(P(a.p), from, rel(cover), 10, 10, () => { cover.style.visibility = ''; src.style.visibility = ''; });
    }
  }
  function closeDetail() {
    const a = byId(S.detail), cover = $('.dt-head .poster', page), from = cover && rel(cover);
    S.detail = null; renderToolbar(); renderPage(false); scroller.scrollTop = S.scrollMem[S.page] || 0;
    const target = $(`.poster[data-aid="${a.id}"]`, page);
    const ok = target && (() => { const r = rel(target), v = rel(scroller); return r.y + r.h > v.y && r.y < v.y + v.h; })();
    if (from && ok && !RM()) {
      target.style.visibility = 'hidden';
      fly(P(a.p), from, rel(target), 10, 10, () => { target.style.visibility = ''; });
      animIn(page, 0);
    } else animIn(page);
  }

  /* ------------------------------------------------------------------
     菜单 / 对话框 / 侧面板
     ------------------------------------------------------------------ */
  let menuCb = null;
  function openMenu(anchor, items, onPick, host = app) {
    if (menuEl.parentElement !== host) host.appendChild(menuEl);
    menuEl.innerHTML = items.map((it, i) => it === '-' ? '<div class="msep"></div>' : `<div class="mi2${it.on ? ' on' : ''}" data-i="${i}">${it.icon ? svg(it.icon) : ''}${it.t}</div>`).join('');
    menuCb = i => onPick(items[i], i);
    const a = rel(anchor, host), hw = host === app ? 1280 : (host.offsetWidth || 390);
    menuEl.style.display = ''; menuEl.style.visibility = 'hidden';
    const mw = menuEl.offsetWidth;
    menuEl.style.left = Math.min(Math.max(8, a.x), hw - mw - 8) + 'px';
    menuEl.style.top = a.y + a.h + 6 + 'px';
    menuEl.style.visibility = '';
    popIn(menuEl, '50% 0');
  }
  menuEl.style.display = 'none';
  menuEl.addEventListener('click', e => { const it = e.target.closest('.mi2'); if (!it) return; e.stopPropagation(); const cb = menuCb; popOut(menuEl); cb && cb(+it.dataset.i); });
  document.addEventListener('pointerdown', e => { if (menuEl.style.display === '' && !menuEl.contains(e.target) && !e.target.closest('[data-act="lib-sort"],[data-act="dt-status"],[data-act="dt-rate"],[data-act="dt-more"],[data-opts]')) popOut(menuEl); }, true);

  const overlays = [];
  function showDim(on) {
    if (on) { dim.style.display = ''; animInFade(dim); } else if (dim.style.display !== 'none') { cancel(dim._tw); dim._tw = tween(TOKENS.smooth, p => { dim.style.opacity = String(1 - Math.min(1, p)); }, () => { dim.style.display = 'none'; dim.style.opacity = ''; }); }
  }
  function animInFade(el) { cancel(el._tw); el.style.opacity = '0'; el._tw = tween(TOKENS.smooth, p => { el.style.opacity = String(Math.min(1, p)); }, () => { el.style.opacity = ''; }); }
  function openOverlay(el, kind, origin = '50% 40%') {
    overlays.push({ el, kind }); showDim(true); popIn(el, origin);
  }
  function closeTop() {
    const o = overlays.pop(); if (!o) return false;
    if (o.kind === 'ob') { overlays.push(o); return false; }
    if (o.kind === 'sheet') slideOut(o.el); else popOut(o.el);
    if (!overlays.length) showDim(false);
    return true;
  }
  dim.addEventListener('click', () => closeTop());
  dim.style.display = 'none';
  function slideIn(el) {
    overlays.push({ el, kind: 'sheet' }); showDim(true);
    el.style.display = ''; cancel(el._tw);
    el._tw = tween(TOKENS.bouncy, p => { el.style.transform = `translateX(${(1 - p) * 100}%)`; });
  }
  function slideOut(el) { cancel(el._tw); el._tw = tween(TOKENS.smooth, p => { el.style.transform = `translateX(${Math.min(1, p) * 100}%)`; }, () => { el.style.display = 'none'; }); }

  function openLinkDialog() {
    dialog.innerHTML = `<div class="t2">打开链接</div><input class="url" placeholder="https://example.com/video.mp4 或 magnet:…" value=""><div style="display:flex;justify-content:flex-end;gap:10px"><span class="b" data-act="close-top">取消</span><span class="b p" data-act="link-go">打开</span></div>`;
    autoJelly(dialog); openOverlay(dialog, 'dialog'); setTimeout(() => $('.url', dialog).focus(), 50);
  }
  function openManage() {
    const g = (l, rows) => `<div class="group-label">${l}</div><div class="row-list">${rows.join('')}</div>`;
    const r = (t, right, d = '') => `<div class="row"><div class="tx"><div class="tt">${t}</div>${d ? `<div class="ds">${d}</div>` : ''}</div>${right}</div>`;
    sheet.innerHTML = `<div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:6px"><b style="font-size:18px">管理媒体库</b><span class="ib" data-act="close-top">${svg('x')}</span></div>
      ${g('文件夹', [r('~/Movies/Anime', '<span class="cap2">312 个文件</span>'), r('/Volumes/NAS/番剧', '<span class="cap2">1,024 个文件</span>'), r('<span class="b plain" style="padding:0"><span>添加文件夹…</span></span>', '')])}
      ${g('扫描', [r('上次扫描', '<span class="cap2">2 分钟前</span>'), r('自动扫描新文件', '<span class="tg on"><i></i></span>'), r('', '<span class="b S" data-act="scan">立即扫描</span>')])}
      ${Store.has('sources') ? g('服务器', SERVERS.map(s => r(s.n, '<span class="cap2">已连接</span>'))) : ''}
      ${Store.has('remote') ? g('局域网共享', [r('允许局域网设备访问', '<span class="tg"><i></i></span>')]) : ''}`;
    autoJelly(sheet); slideIn(sheet);
  }

  /* ------------------------------------------------------------------
     设置
     ------------------------------------------------------------------ */
  const setSideHost = $('.set-side', setModal), setContent = $('.set-content', setModal);
  const sRow = (t, right, d = '') => `<div class="row"><div class="tx"><div class="tt">${t}</div>${d ? `<div class="ds">${d}</div>` : ''}</div>${right}</div>`;
  const sGroup = (l, rows) => `<div class="group-label">${l}</div><div class="row-list">${rows.filter(Boolean).join('')}</div>`;
  const sTg = on => `<span class="tg${on ? ' on' : ''}"><i></i></span>`;
  const sPull = (v, opts) => `<span class="b S" data-opts="${opts}"><span class="pv">${v}</span>${svg('chev-d', 'i', 'width:12px')}</span>`;
  const sSl = v => `<div class="sl" style="max-width:220px;flex:none;width:220px"><b style="width:${v}%"></b><i style="left:${v}%"></i></div>`;
  const sStep = (v, unit = '') => `<span class="stepper" data-unit="${unit}"><span class="ib" data-step="-1">−</span><b>${v}${unit}</b><span class="ib" data-step="1">+</span></span>`;
  const sSeg = (opts, on) => `<span class="seg">${opts.map((o, i) => `<span class="${i === on ? 'on' : ''}">${o}</span>`).join('')}</span>`;
  const logo = (src, mono) => `<img src="${src}" alt=""${mono ? ' class="mono-logo"' : ''}>`;
  const kbd = k => `<span>${k.split('+').map(x => `<kbd>${x}</kbd>`).join(' ')}</span>`;
  const SEC = {
    general: () => sGroup('启动', [sRow('启动时打开', sPull('首页', '首页|上次的页面|媒体库')), sRow('播放结束后', sPull('播放下一集', '播放下一集|回到详情页|什么也不做'))]) + sGroup('语言', [sRow('界面语言', sPull('跟随系统', '跟随系统|简体中文|繁體中文|English'))]) + sGroup('更新', [sRow('自动检查更新', sTg(true)), sRow('更新通道', sPull('稳定版', '稳定版|测试版'))]),
    appearance: () => sGroup('主题色', [`<div class="row" style="min-height:76px"><div class="tx"><div class="tt">主题色</div><div class="ds">取自 NipaPlay 图标，只用在按钮和文字的下划标注上</div></div><div class="sw-row">${[['rose', '#FF2E55', '绯红'], ['blue', '#2A51D4', '群青'], ['mint', '#07FF94', '荧绿']].map(([k, c, n]) => `<button class="sw" data-set-accent="${k}"><i style="background:${c}"></i>${n}</button>`).join('')}</div></div>`, sRow('外观', `<span class="seg"><span data-set-theme="system">跟随系统</span><span data-set-theme="light">浅色</span><span data-set-theme="dark">深色</span></span>`)]) + sGroup('界面', [sRow('减少动态效果', `<span class="tg${NP.getForceRM() ? ' on' : ''}" data-rm><i></i></span>`, '弹簧动画换成 150ms 淡入淡出'), sRow('界面音效', '<span class="b S" data-act="set-sec" data-sec="plugins">去插件市场</span>', '音效包由插件提供')]),
    player: () => sGroup('播放', [sRow('默认倍速', sPull('1.0×', '0.75×|1.0×|1.25×|1.5×|2.0×')), sRow('跳过片头', sStep(85, ' 秒'), '播放器「更多」菜单里一键跳过'), sRow('记住播放位置', sTg(true)), sRow('自动连播下一集', sTg(true))]) + sGroup('解码', [sRow('播放内核', sPull('Erika（推荐）', 'Erika（推荐）|libmpv'), 'libmpv 作为后备内核'), sRow('硬件解码', sTg(true)), sRow('HDR', '<span class="cap2">自动</span>')]),
    danmaku: () => sGroup('显示', [sRow('显示弹幕', sTg(true)), sRow('不透明度', sSl(80)), sRow('字号', sSl(45)), sRow('速度', sSl(50)), sRow('显示区域', sSeg(['1/4', '半屏', '全屏'], 1))]) + sGroup('来源', [sRow('弹弹play 自动匹配', sTg(true)), sRow('本地弹幕文件', '<span class="b S">挂载…</span>', 'xml / json')]) + sGroup('过滤', [sRow('屏蔽词', '<span class="cap2">3 个 ›</span>'), sRow('合并重复弹幕', sTg(true)), sRow('防剧透', NP.installed.has('spoiler') ? sTg(true) : '<span class="b S" data-act="set-sec" data-sec="plugins">安装插件</span>', '由「防剧透」插件提供')]),
    library: () => sGroup('文件夹', [sRow('~/Movies/Anime', '<span class="cap2">312 个文件</span>'), sRow('/Volumes/NAS/番剧', '<span class="cap2">1,024 个文件</span>'), sRow('<span class="b plain" style="padding:0"><span>添加文件夹…</span></span>', '')]) + sGroup('识别', [sRow('自动扫描新文件', sTg(true)), sRow('识别方式', sPull(Store.has('anime') ? '弹弹play + Bangumi' : '仅文件名', '弹弹play + Bangumi|仅文件名'))]),
    anime: () => sGroup('首页', [sRow('显示今日更新', sTg(true)), sRow('显示媒体库最近添加', sTg(true))]) + sGroup('提醒', [sRow('新番更新提醒', sTg(false)), sRow('提醒时间', sPull('更新后立即', '更新后立即|每天 20:00'))]),
    sync: () => sGroup('账号', [sRow(`${logo('assets/dandanplay.png', 1)}弹弹play`, '<span class="cap2">已登录 · Nipa</span>'), sRow(`${logo('assets/bangumi.png', 1)}Bangumi`, '<span class="cap2">已连接</span>')]) + sGroup('同步', [sRow('同步观看进度', sTg(true)), sRow('同步评分与收藏', sTg(true)), sRow('上次同步', '<span class="b S" data-act="sync-now">立即同步</span>', '3 分钟前')]),
    sources: () => sGroup('服务器', [sRow(`${logo('assets/jellyfin.svg')}家里 NAS · Jellyfin`, '<span class="cap2">已连接</span>'), sRow(`${logo('assets/emby.svg')}书房 · Emby`, '<span class="cap2">已连接</span>'), sRow('WebDAV · 网盘', '<span class="cap2">未连接</span>'), sRow('<span class="b plain" style="padding:0"><span>添加服务器…</span></span>', '')]) + sGroup('播放', [sRow('服务器转码', sTg(false)), sRow('默认清晰度', sPull('原画', '原画|1080p|720p'))]),
    remote: () => sGroup('局域网', [sRow('允许局域网设备访问媒体库', sTg(false)), sRow('设备名称', '<span class="cap2">Nipa 的 MacBook</span>')]) + sGroup('远程访问', [sRow('Web 远程访问', sTg(false)), sRow('访问地址', '<span class="cap2">http://192.168.1.8:1180</span>')]),
    downloads: () => sGroup('下载', [sRow('保存位置', '<span class="cap2">~/Downloads</span>'), sRow('同时下载', sStep(3)), sRow('下载完成后加入媒体库', sTg(true))]),
    enhance: () => sGroup('超分辨率', [sRow('默认算法', sSeg(['关', 'Anime4K', 'ArtCNN'], 1)), sRow('仅在低于 1080p 时启用', sTg(true))]) + sGroup('设备', [sRow('GPU', '<span class="cap2">Apple M3 · Metal</span>')]),
    optional: () => `<div class="body" style="margin-bottom:6px">决定 NipaPlay 要做多少事。关闭的功能不会显示，也不占用资源。</div><div style="display:flex;justify-content:flex-end;margin-top:-34px"><span class="b opt-preset"></span></div><div class="opt-rows"></div>
      <div class="row-list" style="margin-top:22px;display:flex;align-items:center;gap:14px;padding:14px 16px;position:relative;overflow:hidden"><div class="plaid-gingham" style="position:absolute;left:0;top:0;bottom:0;width:6px"></div><div class="mi">${svg('puzzle')}</div><div style="flex:1"><div class="tt" style="font-weight:650">想要更多？</div><div class="cap2" style="font-size:12.5px">防剧透、CRT 着色器、片段导出等小功能都在插件市场里。</div></div><span class="b" data-act="set-sec" data-sec="plugins">浏览插件市场</span></div>`,
    plugins: () => `<div class="plaid-academy" style="height:120px;border-radius:16px;padding:24px 26px;color:#fff;position:relative;overflow:hidden;margin-bottom:18px"><div style="font-size:24px;font-weight:800">插件市场</div><div style="opacity:.85;font-size:13.5px;margin-top:4px">小功能按需安装。插件用脚本编写，只能使用它声明过的权限。</div></div>
      <div style="display:flex;align-items:center;gap:8px;margin-bottom:16px"><span class="seg plug-seg"><span data-ps="market" class="on">市场</span><span data-ps="installed"><span>已安装 <b class="plug-n">${NP.installed.size}</b></span></span></span><span style="width:8px"></span><span class="chiprow plug-cats">${['全部', '弹幕', '画面', '工具', '外观'].map((c, i) => `<span class="b S${i ? '' : ' on'}" data-cat="${c}">${c}</span>`).join('')}</span></div>
      <div class="plug-body"></div>`,
    keys: () => sGroup('播放', [sRow('播放 / 暂停', kbd('空格')), sRow('快退 / 快进 5 秒', kbd('← / →')), Store.has('danmaku') && sRow('开关弹幕', kbd('D')), sRow('全屏', kbd('F')), sRow('返回 / 关闭', kbd('Esc'))]) + sGroup('全局', [sRow('打开文件', kbd('⌘+O')), sRow('搜索', kbd('⌘+K')), sRow('设置', kbd('⌘+,'))]),
    advanced: () => sGroup('播放内核', [sRow('内核', sPull('Erika', 'Erika|libmpv')), sRow('独立播放窗口', sTg(false), '桌面平台可以把播放器拆成单独的窗口')]) + (Store.has('danmaku') ? sGroup('实验功能', [sRow('Next2 弹幕引擎', sTg(false), '实验中，可能不稳定')]) : '') + sGroup('开发者', [sRow('开发者选项', sTg(false)), sRow('导出日志', '<span class="b S">导出…</span>')]) + sGroup('重置', [sRow('重新运行首次配置', '<span class="b S" data-act="rerun">重新运行</span>', '模块选择会从头开始'), sRow('恢复默认设置', '<span class="b S" data-act="reset-all">恢复</span>')]),
    about: () => `<div class="about"><img class="logo" src="assets/logo.png" alt="NipaPlay"><div class="t2" style="font-size:16px">NipaPlay Next</div><div class="cap2" style="margin:4px 0 18px">交互原型 v0.1 · 2026-10-12</div><img src="assets/girl.png" alt="" style="width:150px;margin:0 auto"><div class="cap2" style="margin-top:10px">看板娘插画 · MCDFsteve</div><div class="chiprow" style="justify-content:center;margin-top:18px"><span class="chip">GitHub</span><span class="chip">使用文档</span><span class="chip">反馈问题</span></div></div>`,
  };
  const SEC_TITLE = Object.fromEntries([...NP.SET_TOP, ...NP.SET_MOD, ...NP.SET_BOT.filter(x => x[1])].map(x => [x[0], x[1]]));
  function renderSetSideNow(animate) { NP.renderSetSide(setSideHost, S.setSec, Store, animate, k => { S.setSec = k; renderSetSideNow(false); renderSetContent(); }); }
  function renderSetContent() {
    const k = S.setSec;
    setContent.innerHTML = `<div class="lt" style="margin:0 0 12px">${SEC_TITLE[k] || ''}</div>` + (SEC[k] ? SEC[k]() : '');
    if (k === 'optional') {
      setModal._toast = null;
      NP.mountOptional($('.opt-rows', setContent), Store, setContent.closest('.win'), $('.opt-preset', setContent));
    }
    if (k === 'plugins') renderPlugins('market', '全部');
    NP.syncThemeUI();
    autoJelly(setContent);
    setContent.scrollTop = 0;
    animIn(setContent, 8);
  }
  function renderPlugins(tab, cat) {
    const body = $('.plug-body', setContent);
    $$('.plug-seg span', setContent).forEach(s => s.classList.toggle('on', s.dataset.ps === tab));
    $('.plug-cats', setContent).style.display = tab === 'market' ? '' : 'none';
    if (tab === 'market') {
      body.innerHTML = '<div class="mk" style="display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:12px"></div>';
      NP.renderMarket($('.mk', body), cat === '全部' ? null : cat);
    } else {
      body.innerHTML = `<div class="row-list">${NP.PLUGINS.filter(p => NP.installed.has(p.id)).map(p => `<div class="row" style="min-height:60px"><div class="mi" style="width:36px;height:36px;border-radius:10px">${svg(p.ic)}</div><div class="tx"><div class="tt">${p.n}</div><div class="ds">${p.a} · ${p.p.length ? '权限：' + p.p.join('、') : '不需要权限'}</div></div>${p.id === 'spoiler' ? '<span class="b S" data-act="plug-cfg">设置</span>' : ''}<span class="tg on"><i></i></span></div>`).join('')}</div>`;
      autoJelly(body);
    }
  }
  NP.onInstall(() => { const n = $('.plug-n', setContent); if (n) n.textContent = NP.installed.size; toast(setModal.querySelector('.win'), '插件已安装'); });
  function openSettings(sec) {
    if (sec) S.setSec = sec;
    if (!Store.has(S.setSec) && NP.SET_MOD.find(x => x[0] === S.setSec)) S.setSec = 'optional';
    closePalette(true);
    renderSetSideNow(false); renderSetContent();
    if (!overlays.find(o => o.el === setModal)) openOverlay(setModal, 'settings');
  }

  /* ------------------------------------------------------------------
     首次配置
     ------------------------------------------------------------------ */
  let obCtl = null;
  function openOnboarding() {
    if (S.device !== 'desk') setDevice('desk');
    while (overlays.length && overlays[overlays.length - 1].kind !== 'ob') { const o = overlays.pop(); o.el.style.display = 'none'; }
    const rootEl = $('.win', obModal);
    if (!obCtl) obCtl = NP.mountOnboarding(rootEl, Store, { onFinish: finishOnboarding });
    else obCtl.reset();
    openOverlay(obModal, 'ob', '50% 50%');
  }
  function finishOnboarding() {
    kv.set('app.done', '1');
    const i = overlays.findIndex(o => o.kind === 'ob'); if (i >= 0) overlays.splice(i, 1);
    if (!overlays.length) showDim(false);
    cancel(obModal._tw);
    obModal._tw = tween(TOKENS.hero, p => { obModal.style.transform = `scale(${1 + 0.06 * Math.min(1, p)})`; obModal.style.opacity = String(Math.max(0, 1 - p * 1.6)); }, () => { obModal.style.display = 'none'; obModal.style.transform = ''; obModal.style.opacity = ''; });
    setTimeout(() => toast(app, '设置好了 ☆ 随时可以在 设置 › 可选功能 里修改'), 350);
  }

  /* ------------------------------------------------------------------
     搜索面板
     ------------------------------------------------------------------ */
  const palIn = $('input', palette), palRes = $('.res', palette);
  let palItems = [], palSel = 0;
  function palBuild(q) {
    q = q.trim().toLowerCase();
    const hit = s => !q || s.toLowerCase().includes(q);
    const items = [];
    const acts = [
      { g: '操作', t: '打开文件…', ic: 'folder', k: '⌘O', run: () => fileIn.click() },
      { g: '操作', t: '打开链接…', ic: 'link', run: () => openLinkDialog() },
      { g: '操作', t: root.dataset.theme === 'dark' ? '切换到浅色模式' : '切换到深色模式', ic: root.dataset.theme === 'dark' ? 'sun' : 'moon', run: () => setTheme(root.dataset.theme === 'dark' ? 'light' : 'dark') },
      { g: '操作', t: '打开设置', ic: 'gear', k: '⌘,', run: () => openSettings() },
    ];
    if (Store.has('library') || Store.has('anime')) ANIME.filter(a => hit(a.t)).slice(0, q ? 8 : 5).forEach(a => items.push({ g: '番剧', t: a.t, poster: P(a.p), k: statusOf(a), run: () => openDetail(a.id) }));
    NP.settingsSections(Store).filter(s => s.n && hit(s.n)).slice(0, q ? 6 : 3).forEach(s => items.push({ g: '设置', t: s.n, ic: s.ic, run: () => openSettings(s.key) }));
    acts.filter(a => hit(a.t)).forEach(a => items.push(a));
    palItems = items; palSel = 0;
    let lastG = '';
    palRes.innerHTML = items.length ? items.map((it, i) => { const head = it.g !== lastG ? `<div class="gl">${it.g}</div>` : ''; lastG = it.g; return head + `<div class="it${i === 0 ? ' sel' : ''}" data-i="${i}">${it.poster ? `<div class="poster"><img src="${it.poster}" alt=""></div>` : `<div class="mi">${svg(it.ic)}</div>`}<span>${it.t}</span>${it.k ? `<span class="k">${it.k}</span>` : ''}</div>`; }).join('') : '<div class="gl" style="padding:18px">没有找到结果</div>';
  }
  function openPalette() {
    if (overlays.find(o => o.el === palette)) return;
    palIn.value = ''; palBuild('');
    openOverlay(palette, 'palette', '50% 0');
    setTimeout(() => palIn.focus(), 30);
  }
  function closePalette(silent) { const i = overlays.findIndex(o => o.el === palette); if (i < 0) return; overlays.splice(i, 1); popOut(palette); if (!overlays.length) showDim(false); palIn.blur(); }
  palIn.addEventListener('input', () => palBuild(palIn.value));
  palIn.addEventListener('keydown', e => {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); palSel = Math.max(0, Math.min(palItems.length - 1, palSel + (e.key === 'ArrowDown' ? 1 : -1))); $$('.it', palRes).forEach((x, i) => x.classList.toggle('sel', i === palSel)); }
    if (e.key === 'Enter' && palItems[palSel]) { const it = palItems[palSel]; closePalette(); it.run(); }
  });
  palRes.addEventListener('click', e => { const x = e.target.closest('.it'); if (!x) return; const it = palItems[+x.dataset.i]; closePalette(); it.run(); });

  /* ------------------------------------------------------------------
     播放器 · 胶囊岛（方向 B）
     控制层是一颗毛玻璃胶囊，浮层都从胶囊里长出来：悬停进度条长出预览，弹幕 / 字幕 / 倍速 / 画质
     长成选项卡，发弹幕时收窄成输入框；右上角的小胶囊长成「更多」菜单。底部对白跟着胶囊往上让。
     ------------------------------------------------------------------ */
  const { Spring, run } = NP;
  const DUR = 1455;
  const CHAPTERS = [{ t: 0, n: '序幕', s: '序幕' }, { t: 92, n: '片头', s: 'OP' }, { t: 182, n: 'A 部分', s: 'A' }, { t: 700, n: 'B 部分', s: 'B' }, { t: 1295, n: '片尾', s: 'ED' }, { t: 1385, n: '下集预告', s: '预告' }];
  const PEAKS = [{ t: 342, n: '名场面' }, { t: 1098, n: '高能' }];
  const SUB_LINES = ['这一次，我一定要救下她。', '……又回到那一天了吗？', '别怕，我就在这里。', '雪停之前，一起回家吧。'];
  const PANE_H = { prev: 184, dm: 164, sub: 172, rate: 122, enh: 122 };
  const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
  const rateLabel = r => (Number.isInteger(r) ? r.toFixed(1) : String(r)) + '×';
  /* 章节读 MKV 章节；剧场版和本地文件这里按没有章节处理 */
  const chaptersOf = p => (p.aid && p.dur === DUR ? CHAPTERS : []);
  const chapterAt = (p, t) => chaptersOf(p).filter(c => t >= c.t).pop();
  const gapMask = p => {
    const ch = chaptersOf(p).slice(1); if (!ch.length) return '';
    let m = 'linear-gradient(90deg', a = '0px';
    ch.forEach(c => { const x = (c.t / p.dur) * 100; m += `, #000 ${a} calc(${x}% - 1px), transparent calc(${x}% - 1px) calc(${x}% + 1px)`; a = `calc(${x}% + 1px)`; });
    m += `, #000 ${a} 100%)`;
    return `-webkit-mask-image:${m};mask-image:${m}`;
  };
  let tick = 0, hideT = 0, hoverT = 0;
  const isl = { pane: null, pin: false, saying: false, hover: null, drag: false, more: false,
    W: new Spring(880), H: new Spring(60), anim: null, L: new Spring(0), lAnim: null, mW: new Spring(0), mH: new Spring(40), mAnim: null };
  const HS = new Spring(0); let hsAnim = null;

  function newPlayer({ aid = null, ep = 1, file = null, url = null, start = null, title = null }) {
    const a = aid != null ? byId(aid) : null;
    const cw = a ? CW.find(c => c.id === a.id && c.ep === ep) : null;
    const p = {
      aid: a ? a.id : null, ep, playing: true, rate: 1, dmOn: true, sub: 0, audio: 0, enh: 1, area: 1,
      title: title || (a ? a.t : '未命名视频'), sub2: a ? (a.movie ? '剧场版' : `第 ${ep} 集`) : (file ? '本地文件' : url ? '网络链接' : ''),
      img: STILL, video: null, t: 0, dur: a && a.movie ? 6420 : DUR, panelTab: null,
    };
    p.t = start != null ? start : cw ? cw.prog * p.dur : 0;
    if (file || url) {
      const v = document.createElement('video');
      v.className = 'video'; v.playsInline = true; v.src = file ? URL.createObjectURL(file) : url;
      v.addEventListener('loadedmetadata', () => { if (isFinite(v.duration)) { p.dur = v.duration; const d = $('.il-t.dur', player); if (d) d.textContent = fmt2(p.dur); } v.play().catch(() => {}); updTime(); });
      v.addEventListener('error', () => { p.video = null; v.remove(); const img = $('img.video', player); if (img) img.style.display = ''; toast(app, '浏览器无法解码这个格式，原型改用示意画面'); });
      p.video = v; p.t = 0;
    }
    return p;
  }
  function playerHTML() {
    const p = S.player, dm = Store.has('danmaku'), dmi = dm && Store.has('sync'), enh = Store.has('enhance');
    const chip = (t, on, d, extra = '') => `<span class="il-chip${on ? ' on' : ''}${extra}" data-po="${d}">${t}</span>`;
    const menu = [['camera', '截图', 'shot'], ['ff10', '跳过片头（85 秒）', 'skip'], ['info', '播放信息', 'info'], dm && ['danmaku', '手动匹配弹幕…', 'match']].filter(Boolean);
    return `<img class="video" src="${p.img}" alt="" style="object-position:center 25%${p.video ? ';display:none' : ''}">
      <div class="dm-layer" style="inset:${['70px 0 520px', '70px 0 360px', '70px 0 130px'][p.area]};${dm && p.dmOn ? '' : 'display:none'}">${LINES.map(([t, c], i) => { const dur = 11 + ((i * 7) % 6), delay = -((i * 3.7) % dur); return `<span style="top:${(i % 9) * 40}px;animation-duration:${dur}s;animation-delay:${delay}s${c ? ';color:' + c : ''}">${t}</span>`; }).join('')}</div>
      <div class="np-sub"${p.sub === 2 ? ' style="display:none"' : ''}><span></span></div>
      <div class="center-fb"></div>
      <div class="ui">
        <div class="il-cap l glass" data-spec="片名胶囊 · 高 40"><span class="pbtn sm" data-pa="back" title="返回（Esc）">${svg('chev-l')}</span><b>${p.title}</b><span class="m">${p.sub2}</span></div>
        <div class="il-cap r glass" data-spec="工具胶囊 · 点「更多」长成菜单"><div class="cap-row">${dm ? `<span class="pbtn sm" data-pa="match" title="弹幕匹配">${svg('link')}</span>` : ''}<span class="pbtn sm" data-pa="panel" title="播放设置">${svg('sliders')}</span><span class="pbtn sm" data-pa="pip" title="画中画">${svg('pip')}</span><span class="pbtn sm" data-pa="more" title="更多">${svg('more')}</span></div>
          <div class="cap-menu">${menu.map(([ic, t, d]) => `<div class="mi3" data-po="${d}">${svg(ic)}${t}</div>`).join('')}</div></div>
        <div class="il glass">
          <div class="il-up">
            <div class="il-pane" data-pane="prev"><div class="il-fr"><img src="${p.img}" alt=""><div class="tm"><b></b><span></span></div></div><div class="il-chs"></div></div>
            ${dm ? `<div class="il-pane" data-pane="dm"><div class="il-dmq">
              <div class="q"><span class="k">不透明度</span><div class="sl dark"><b style="width:80%"></b><i style="left:80%"></i></div></div>
              <div class="q"><span class="k">显示区域</span><span class="seg dark">${['1/4', '半屏', '全屏'].map((x, i) => `<span class="${i === p.area ? 'on' : ''}" data-po="area-${i}">${x}</span>`).join('')}</span></div>
              <div class="q"><span class="k">字号</span><div class="sl dark"><b style="width:45%"></b><i style="left:45%"></i></div></div>
              <div class="q"><span class="k">屏蔽</span><span class="chips">${['顶部', '底部', '滚动', '彩色'].map((x, i) => chip(x, false, 'blk', i === 1 ? ' off' : '')).join('')}</span></div>
              <span class="lnk" data-po="more-dm">更多弹幕设置 ›</span></div></div>` : ''}
            <div class="il-pane" data-pane="sub"><div class="il-opts r2"><span class="k">字幕</span>${['简体中文', '日本語', '关闭'].map((x, i) => chip(x, i === p.sub, 'sub-' + i)).join('')}${chip('添加外挂…', false, 'sub-add')}</div><div class="il-opts"><span class="k">音轨</span>${['日本語 · FLAC 2.0', '中文配音 · AAC 2.0'].map((x, i) => chip(x, i === p.audio, 'au-' + i)).join('')}</div></div>
            <div class="il-pane" data-pane="rate"><div class="il-opts"><span class="k">倍速</span>${[0.5, 0.75, 1, 1.25, 1.5, 2].map(r => chip(r + '×', r === p.rate, 'rate-' + r)).join('')}</div></div>
            ${enh ? `<div class="il-pane" data-pane="enh"><div class="il-opts"><span class="k">画质</span>${['关闭', 'Anime4K', 'ArtCNN 2×'].map((x, i) => chip(x, i === p.enh, 'enh-' + i)).join('')}</div></div>` : ''}
          </div>
          <div class="il-row main">
            <span class="pbtn il-play" data-pa="play" title="播放 / 暂停（空格）">${svg(p.playing ? 'pause' : 'play')}</span><span class="pbtn" data-pa="next" title="下一集">${svg('next')}</span><span class="vol"><span class="pbtn" data-pa="mute" title="音量">${svg('vol')}</span><span class="sl-v"><b></b></span></span>
            <span class="il-t now"></span>
            <div class="il-pg"><div class="msk" style="${gapMask(p)}"><div class="tr"></div><div class="gh"></div><div class="bd"></div></div>${dm && chaptersOf(p).length ? PEAKS.map(k => `<i class="pk" style="left:${(k.t / p.dur) * 100}%" title="弹幕高峰 · ${k.n}"></i>`).join('') : ''}</div>
            <span class="il-t dur">${fmt2(p.dur)}</span><i class="il-sep"></i>
            ${dm ? `<span class="pbtn il-dm${p.dmOn ? ' on' : ''}" data-pa="dm" title="开关弹幕（D）">${svg(p.dmOn ? 'danmaku' : 'danmaku-off')}<i class="led"></i></span>` : ''}
            ${dmi ? `<span class="pbtn" data-pa="say" title="发弹幕">${svg('send')}</span>` : ''}
            <span class="pbtn" data-pa="sub" title="字幕与音轨">${svg('cc')}</span>
            <span class="il-txt" data-pa="rate" data-j="M" title="倍速">${rateLabel(p.rate)}</span>
            ${enh ? `<span class="pbtn" data-pa="enh" title="画质增强">${svg('sparkle')}</span>` : ''}
            <span class="pbtn" data-pa="fs" title="全屏（F）">${svg('full')}</span>
          </div>
          ${dmi ? `<div class="il-row say"><span class="pbtn il-dm${p.dmOn ? ' on' : ''}" data-pa="dm" title="开关弹幕">${svg(p.dmOn ? 'danmaku' : 'danmaku-off')}<i class="led"></i></span><label class="il-say"><input placeholder="发个友善的弹幕…" maxlength="40"><span class="send" data-pa="send">发送</span></label><span class="pbtn sm" data-pa="unsay" title="收起（Esc）">${svg('x')}</span></div>` : ''}
        </div>
      </div>
      <div class="panel-side app-ppanel" style="display:none"></div>`;
  }
  function buildPlayer() {
    const p = S.player;
    Object.assign(isl, { pane: null, pin: false, saying: false, hover: null, drag: false, more: false });
    [isl.anim, isl.mAnim, hsAnim].forEach(a => a && NP.stop(a)); isl.anim = isl.mAnim = hsAnim = null;
    isl.W.target = 880; isl.W.snap(); isl.H.target = 60; isl.H.snap();
    HS.target = p.t; HS.snap();
    player.innerHTML = playerHTML();
    if (p.video) player.insertBefore(p.video, player.firstChild);
    $$('.pbtn', player).forEach(b => bindJelly(b, 'S'));
    autoJelly(player);
    updTime();
    const inp = $('.il-say input', player);
    if (inp) inp.addEventListener('keydown', e => { e.stopPropagation(); if (e.key === 'Enter') { e.preventDefault(); sendDm(); } if (e.key === 'Escape') islClose(); });
    /* 进度条：悬停长出预览、点击和拖动跳转 */
    const pg = $('.il-pg', player);
    const frac = e => { const r = pg.getBoundingClientRect(); return r.width ? clamp((e.clientX - r.left) / r.width, 0, 1) : null; };
    pg.addEventListener('pointermove', e => { const f = frac(e); islHover(f); if (isl.drag && f != null) seekTo(f * S.player.dur); });
    pg.addEventListener('pointerleave', () => { if (!isl.drag) islHover(null); });
    pg.addEventListener('pointerdown', e => { const f = frac(e); if (e.button > 0 || f == null) return; isl.drag = true; pg.setPointerCapture(e.pointerId); seekTo(f * S.player.dur); });
    const up = e => { isl.drag = false; const r = pg.getBoundingClientRect(); if (e.clientX < r.left || e.clientX > r.right || e.clientY < r.top - 4 || e.clientY > r.bottom + 4) islHover(null); };
    pg.addEventListener('pointerup', up); pg.addEventListener('pointercancel', up);
    /* 弹幕按钮：点一下开关，鼠标停留时胶囊长出常用参数（第二层），完整设置在侧面板（第三层） */
    const dmb = $('.il-row.main [data-pa="dm"]', player), upEl = $('.il-up', player);
    const maybeClose = () => { clearTimeout(hoverT); hoverT = setTimeout(() => { if (isl.pane === 'dm' && !isl.pin && !upEl.matches(':hover') && !(dmb && dmb.matches(':hover'))) { isl.pane = null; islLayout(); } }, 280); };
    if (dmb) {
      dmb.addEventListener('pointerenter', e => { if (e.pointerType !== 'mouse' || isl.saying || isl.hover != null) return; clearTimeout(hoverT); if (isl.pane !== 'dm' && !isl.pin) { isl.pane = 'dm'; islLayout(); } });
      dmb.addEventListener('pointerleave', maybeClose);
      upEl.addEventListener('pointerleave', maybeClose);
      upEl.addEventListener('pointerdown', () => { if (isl.pane === 'dm') isl.pin = true; });
    }
    subLift(true);
    if (p.panelTab) openPanel(p.panelTab, false);
  }
  function islLayout() {
    const il = $('.il', player); if (!il) return;
    const pane = isl.saying ? null : isl.pane || (isl.hover != null ? 'prev' : null);
    $$('.il-pane', il).forEach(x => x.classList.toggle('on', x.dataset.pane === pane));
    il.classList.toggle('hov', pane === 'prev'); il.classList.toggle('saying', isl.saying);
    islSize(isl.saying ? 600 : 880, PANE_H[pane] || 60);
    subLift();
  }
  function islSize(w, h) {
    isl.W.cfg(TOKENS.jelly).target = w; isl.H.cfg({ T: TOKENS.jelly.T * 0.9, z: TOKENS.jelly.z }).target = h;
    const apply = () => { const e = $('.il', player); if (e) { e.style.width = isl.W.v + 'px'; e.style.height = isl.H.v + 'px'; } };
    if (RM()) { isl.W.snap(); isl.H.snap(); apply(); return; }
    if (!isl.anim) isl.anim = run({ sp: [isl.W, isl.H], eps: 0.05, apply, done: () => { isl.anim = null; } });
  }
  /* 字幕让位：控制层可见时，底部对白抬到胶囊上方 14px；胶囊长高时跟着再抬 */
  function subLift(snap) {
    const shown = S.mode === 'full' && !player.classList.contains('hide-ui');
    isl.L.cfg(TOKENS.smooth).target = shown ? 26 + isl.H.target + 14 - 48 : 0;
    const apply = () => { const e = $('.np-sub', player); if (e) e.style.transform = `translateY(${(-isl.L.v).toFixed(1)}px)`; };
    if (snap || RM()) { isl.L.snap(); apply(); return; }
    if (!isl.lAnim) isl.lAnim = run({ sp: [isl.L], eps: 0.1, apply, done: () => { isl.lAnim = null; } });
  }
  function islHover(f) {
    const p = S.player, il = $('.il', player); if (!p || !il) return;
    const was = isl.hover; isl.hover = f;
    const gh = $('.il-pg .gh', il);
    if (f == null) { gh.style.width = '0'; if (was != null) islLayout(); return; }
    const pg = $('.il-pg', il), row = $('.il-row.main', il), W = pg.offsetWidth, ox = row.offsetLeft + pg.offsetLeft;
    const x = f * W, hx = (HS.v / p.dur) * W, t = f * p.dur, fr = $('.il-fr', il), ch = chapterAt(p, t);
    gh.style.left = hx + 'px'; gh.style.width = Math.max(0, x - hx) + 'px';
    fr.style.left = clamp(ox + x - 84, 14, 880 - 182) + 'px';
    $('.tm b', fr).textContent = fmt2(t); $('.tm span', fr).textContent = ch ? ch.n : '';
    $('img', fr).style.objectPosition = `${30 + (t / p.dur) * 40}% ${20 + ((Math.floor(t) * 7) % 60)}%`;
    if (was == null || isl.chW !== W) {
      isl.chW = W;
      $('.il-chs', il).innerHTML = chaptersOf(p).map(c => `<span style="left:${ox + (c.t / p.dur) * W + (c.t ? 3 : 0)}px">${c.s}</span>`).join('');
    }
    if (was == null) {
      if (isl.pane && !isl.pin) isl.pane = null;
      if (!isl.pin) islLayout();
    }
  }
  function islPane(k) { isl.pane = isl.pane === k && isl.pin ? null : k; isl.pin = !!isl.pane; isl.saying = false; if (isl.more) capMore(false); islLayout(); wakeUi(); }
  function islSay(on) {
    isl.saying = on; isl.pane = null; isl.pin = false; islLayout(); wakeUi();
    const inp = $('.il-say input', player);
    if (on && inp) setTimeout(() => inp.focus({ preventScroll: true }), 120); else if (inp) inp.blur();
  }
  function islClose() { if (isl.more) capMore(false); isl.pane = null; isl.pin = false; if (isl.saying) islSay(false); else islLayout(); }
  /* 右上角工具胶囊长成「更多」菜单（向左下展开） */
  function capMore(open) {
    const cap = $('.il-cap.r', player); if (!cap) return;
    const row = $('.cap-row', cap);
    if (!cap.style.width) { isl.mW.v = isl.mW.target = cap.offsetWidth; isl.mH.v = isl.mH.target = 40; isl.mW.vel = isl.mH.vel = 0; }
    isl.more = open; cap.classList.toggle('open', open);
    const items = $$('.mi3', cap).length, w0 = Math.max(row.scrollWidth, 40);
    isl.mW.cfg(TOKENS.jelly).target = open ? Math.max(236, w0) : w0;
    isl.mH.cfg({ T: TOKENS.jelly.T * 0.9, z: TOKENS.jelly.z }).target = open ? 40 + 6 + items * 34 + 8 : 40;
    const apply = () => { cap.style.width = isl.mW.v + 'px'; cap.style.height = isl.mH.v + 'px'; };
    const done = () => { isl.mAnim = null; if (!isl.more) { cap.style.width = ''; cap.style.height = ''; } };
    if (RM()) { isl.mW.snap(); isl.mH.snap(); apply(); done(); return; }
    if (!isl.mAnim) isl.mAnim = run({ sp: [isl.mW, isl.mH], eps: 0.05, apply, done });
    wakeUi();
  }
  /* 兼容旧调用：收起所有从胶囊长出来的东西 */
  function closePop() { if (!S.player) return; Object.assign(isl, { pane: null, pin: false, hover: null, drag: false }); if (isl.more) capMore(false); if (isl.saying) islSay(false); else islLayout(); }
  function seekTo(t) {
    const p = S.player; if (!p) return;
    p.t = clamp(t, 0, p.dur); if (p.video) p.video.currentTime = p.t;
    HS.cfg(TOKENS.bouncy).target = p.t;
    if (RM()) { HS.snap(); updTime(); return; }
    if (!hsAnim) hsAnim = run({ sp: [HS], eps: 0.05, apply: updBand, done: () => { hsAnim = null; updBand(); } });
    updTime();
  }
  function updBand() {
    const p = S.player; if (!p) return;
    const v = hsAnim ? HS.v : p.t, r = clamp(v / p.dur, 0, 1);
    const b = $('.il-pg .bd', player), now = $('.il-t.now', player);
    if (b) b.style.width = r * 100 + '%';
    if (now) { const s = fmt2(v); if (now.textContent !== s) now.textContent = s; }
  }
  function updTime() {
    const p = S.player; if (!p) return;
    if (p.video && isFinite(p.video.currentTime)) p.t = p.video.currentTime;
    if (!hsAnim) { HS.target = p.t; HS.snap(); }
    updBand();
    const r = Math.min(1, p.t / p.dur);
    const sub = $('.np-sub span', player); if (sub) { const s = SUB_LINES[Math.floor(p.t / 4) % SUB_LINES.length]; if (sub.textContent !== s) sub.textContent = s; }
    const ml = $$('.mp-line b'), mt = $$('.mp-time');
    ml.forEach(x => (x.style.width = r * 100 + '%')); mt.forEach(x => (x.textContent = fmt(p.t)));
    $$('.mp-pb').forEach(x => (x.style.width = r * 100 + '%'));
    $$('.pip .prog b').forEach(x => (x.style.width = r * 100 + '%'));
  }
  function startTick() {
    clearInterval(tick);
    tick = setInterval(() => {
      const p = S.player; if (!p) return;
      if (!p.video && p.playing && !isl.drag) { p.t += 0.25 * p.rate; if (p.t >= p.dur) { p.t = p.dur; p.playing = false; syncPlayIcons(); } }
      updTime();
    }, 250);
  }
  function syncPlayIcons() {
    const p = S.player; if (!p) return;
    const pl = $('[data-pa="play"]', player); if (pl) pl.innerHTML = svg(p.playing ? 'pause' : 'play');
    $$('[data-act="mini-toggle"]').forEach(x => (x.innerHTML = svg(p.playing ? 'pause' : 'play', 'i', 'width:16px;height:16px')));
    if (M && M.land) renderLand();
  }
  function setPlaying(on, feedback = true) {
    const p = S.player; if (!p) return;
    p.playing = on;
    if (p.video) { if (on) p.video.play().catch(() => {}); else p.video.pause(); }
    syncPlayIcons();
    if (feedback) centerFb(on ? 'play' : 'pause');
    $$('.dm-layer span', player).forEach(s => (s.style.animationPlayState = on ? '' : 'paused'));
    wakeUi();
  }
  function setRate(r) {
    const p = S.player; p.rate = r; if (p.video) p.video.playbackRate = r;
    $$('[data-pa="rate"]', player).forEach(x => (x.textContent = rateLabel(r)));
    $$('[data-po^="rate-"]', player).forEach(x => x.classList.toggle('on', +x.dataset.po.slice(5) === r));
  }
  function centerFb(icon, host = player) {
    const c = $('.center-fb', host); if (!c) return;
    c.innerHTML = svg(icon); cancel(c._tw); clearTimeout(c._h);
    c._tw = tween(TOKENS.jelly, p => { c.style.transform = `scale(${0.6 + 0.4 * p})`; c.style.opacity = String(Math.min(1, p * 2)); });
    c._h = setTimeout(() => { c._tw = tween(TOKENS.smooth, p => { c.style.opacity = String(Math.max(0, 1 - p)); c.style.transform = `scale(${1 + 0.15 * Math.min(1, p)})`; }); }, 420);
  }
  function wakeUi() {
    player.classList.remove('hide-ui');
    subLift();
    clearTimeout(hideT);
    hideT = setTimeout(() => {
      const p = S.player, ae = document.activeElement;
      const typing = ae && ae.tagName === 'INPUT' && player.contains(ae);
      const over = $$('.il, .il-cap, .app-ppanel', player).some(x => x.matches(':hover'));
      if (p && p.playing && !isl.pane && !isl.saying && !isl.more && !isl.drag && !p.panelTab && S.mode === 'full' && !typing && !over) { player.classList.add('hide-ui'); islHover(null); subLift(); }
    }, 3000);
  }
  player.addEventListener('pointermove', wakeUi);
  function sendDm() {
    const inp = $('.il-say input', player); if (!inp) return;
    if (!inp.value.trim()) { inp.focus({ preventScroll: true }); return; }
    const layer = $('.dm-layer', player);
    const s = document.createElement('span');
    s.className = 'mine'; s.textContent = inp.value.trim();
    s.style.cssText = `top:${Math.floor(Math.random() * 6) * 40}px;animation-duration:9s;animation-iteration-count:1;animation-delay:0s`;
    layer.appendChild(s); setTimeout(() => s.remove(), 9200);
    inp.value = '';
    if (!S.player.dmOn) toggleDm();
  }
  function toggleDm() {
    const p = S.player; if (!p || !Store.has('danmaku')) return;
    p.dmOn = !p.dmOn;
    $$('.il-dm', player).forEach(b => { b.classList.toggle('on', p.dmOn); b.firstElementChild.outerHTML = svg(p.dmOn ? 'danmaku' : 'danmaku-off'); });
    setShown($('.dm-layer', player), p.dmOn, null);
  }
  function openPanel(tab, animate = true) {
    const p = S.player, pnl = $('.app-ppanel', player);
    if (isl.pane || isl.saying) { isl.pane = null; isl.pin = false; isl.saying = false; islLayout(); }
    if (isl.more) capMore(false);
    const tabs = [Store.has('danmaku') && ['dm', '弹幕'], ['sub', '字幕'], ['video', '画面'], ['audio', '音频'], ['play', '播放']].filter(Boolean);
    if (!tabs.find(t => t[0] === tab)) tab = tabs[0][0];
    p.panelTab = tab;
    const r = (t, right) => `<div class="r"><span class="grow">${t}</span>${right}</div>`;
    const tg = on => `<span class="tg dark${on ? ' on' : ''}"><i></i></span>`;
    const sl = v => `<div class="sl dark" style="width:150px;flex:none"><b style="width:${v}%"></b><i style="left:${v}%"></i></div>`;
    const seg = (o, on) => `<span class="seg dark">${o.map((x, i) => `<span class="${i === on ? 'on' : ''}">${x}</span>`).join('')}</span>`;
    const grp = (l, rows) => `<div class="gl">${l}</div><div class="grp">${rows.join('')}</div>`;
    const C = {
      dm: () => grp('显示', [r('显示弹幕', `<span class="tg dark${p.dmOn ? ' on' : ''}" data-pp="dm"><i></i></span>`), r('速度', sl(50)), r('合并重复弹幕', tg(true))]) + grp('来源', [r('弹弹play · 自动匹配', '<span class="v">已匹配 ✓</span>'), r('手动匹配…', '<span class="v">›</span>'), r('弹幕偏移', '<span class="v">− 0.0 s +</span>'), r('挂载本地弹幕文件', '<span class="v">xml / json ›</span>')]) + grp('过滤', [r('屏蔽词', '<span class="v">3 个 ›</span>'), r('插件：防剧透', `<span class="v">${NP.installed.has('spoiler') ? '已启用' : '未安装'} ›</span>`)]),
      sub: () => grp('字幕轨道', ['简体中文（内嵌 ASS）', '日本語', '关闭字幕'].map((x, i) => `<div class="r" data-pp="sub-${i}" style="cursor:pointer"><span class="grow">${x}</span>${i === p.sub ? '<span class="v" style="color:#fff">✓</span>' : ''}</div>`).concat([r('添加外挂字幕…', '<span class="v">›</span>')])) + grp('样式', [r('字号', sl(50)), r('字幕延迟', '<span class="v">− 0.0 s +</span>'), r('控制层出现时让位', tg(true))]),
      video: () => grp('画面', [r('画面比例', seg(['原始', '16:9', '4:3', '填充'], 0)), r('HDR', '<span class="v">自动</span>')]) + (Store.has('enhance') ? grp('画质增强', [r('超分辨率', seg(['关', 'Anime4K', 'ArtCNN'], p.enh))]) : ''),
      audio: () => grp('音轨', ['日本語 · FLAC 2.0', '中文配音 · AAC 2.0'].map((x, i) => `<div class="r"><span class="grow">${x}</span>${i === p.audio ? '<span class="v" style="color:#fff">✓</span>' : ''}</div>`)) + grp('调整', [r('音频延迟', '<span class="v">− 0 ms +</span>'), r('音量', sl(70))]),
      play: () => grp('播放', [r('跳过片头', tg(true)), r('跳过秒数', '<span class="v">85 秒</span>'), r('自动连播下一集', tg(true))]) + grp('信息', [r('播放内核', '<span class="v">Erika</span>'), r('视频', `<span class="v">${p.video ? '本地文件' : 'HEVC · 1080p'}</span>`), r('硬件解码', '<span class="v">VideoToolbox</span>')]),
    };
    pnl.innerHTML = `<div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:12px"><b style="font-size:16px">播放设置</b><span class="pbtn sm" data-pa="panel-close">${svg('x')}</span></div><span class="seg dark">${tabs.map(([k, n]) => `<span data-pp="tab-${k}" class="${k === tab ? 'on' : ''}">${n}</span>`).join('')}</span><div class="pb">${C[tab]()}</div>`;
    autoJelly(pnl); $$('.pbtn', pnl).forEach(b => bindJelly(b, 'S'));
    if (pnl.style.display === 'none') {
      pnl.style.display = ''; cancel(pnl._tw);
      if (animate && !RM()) pnl._tw = tween(TOKENS.bouncy, q => { pnl.style.transform = `translateX(calc(${(1 - q) * 100}% + ${(1 - q) * 24}px))`; pnl.style.opacity = String(Math.min(1, q * 2)); });
    }
  }
  function closePanel() {
    const pnl = $('.app-ppanel', player); if (!pnl || pnl.style.display === 'none') return;
    S.player.panelTab = null; cancel(pnl._tw);
    pnl._tw = tween(TOKENS.smooth, q => { pnl.style.transform = `translateX(calc(${Math.min(1, q) * 100}% + ${Math.min(1, q) * 24}px))`; }, () => { pnl.style.display = 'none'; pnl.style.opacity = ''; });
  }
  player.addEventListener('click', e => {
    const p = S.player; if (!p) return;
    const po = e.target.closest('[data-po]'), pp = e.target.closest('[data-pp]'), pa = e.target.closest('[data-pa]');
    if (isl.more && !e.target.closest('.il-cap.r')) capMore(false);
    if (po) {
      const d = po.dataset.po, later = f => setTimeout(f, 260);
      if (d.startsWith('area-')) { p.area = +d.slice(5); $$('[data-po^="area-"]', player).forEach(x => x.classList.toggle('on', x === po)); $('.dm-layer', player).style.inset = ['70px 0 520px', '70px 0 360px', '70px 0 130px'][p.area]; return; }
      if (d === 'blk') { po.classList.toggle('off'); return; }
      if (d === 'more-dm') { openPanel('dm'); return; }
      if (d === 'sub-add') { islClose(); toast(app, '原型：会打开文件选择器挂载 ASS / SRT'); return; }
      if (d.startsWith('sub-')) { p.sub = +d.slice(4); $$('[data-po^="sub-"]', player).forEach(x => x.classList.toggle('on', x === po)); $('.np-sub', player).style.display = p.sub === 2 ? 'none' : ''; later(islClose); return; }
      if (d.startsWith('au-')) { p.audio = +d.slice(3); $$('[data-po^="au-"]', player).forEach(x => x.classList.toggle('on', x === po)); later(islClose); return; }
      if (d.startsWith('rate-')) { setRate(+d.slice(5)); later(islClose); return; }
      if (d.startsWith('enh-')) { p.enh = +d.slice(4); $$('[data-po^="enh-"]', player).forEach(x => x.classList.toggle('on', x === po)); later(() => { islClose(); toast(app, '画质增强：' + po.textContent); }); return; }
      if (d === 'shot') { capMore(false); toast(app, '截图已保存到「图片」'); return; }
      if (d === 'skip') { capMore(false); seekTo(Math.min(p.dur, p.t + 85)); toast(app, '已跳过片头'); return; }
      if (d === 'info') { openPanel('play'); return; }
      if (d === 'match') { capMore(false); toast(app, '原型：会打开弹幕匹配窗口，搜索弹弹play 的番剧和集数'); return; }
    }
    if (pp) {
      const d = pp.dataset.pp;
      if (d.startsWith('tab-')) { openPanel(d.slice(4)); return; }
      if (d === 'dm') { toggleDm(); pp.classList.toggle('on', p.dmOn); return; }
      if (d.startsWith('sub-')) { p.sub = +d.slice(4); $('.np-sub', player).style.display = p.sub === 2 ? 'none' : ''; openPanel('sub'); return; }
    }
    if (pa) {
      const d = pa.dataset.pa;
      if (d === 'back') return playerToMini();
      if (d === 'play') return setPlaying(!p.playing);
      if (d === 'next') { if (p.aid) { const a = byId(p.aid); if (p.ep < a.eps) { p.ep++; p.t = 0; p.sub2 = `第 ${p.ep} 集`; $('.il-cap.l .m', player).textContent = p.sub2; updTime(); toast(app, `第 ${p.ep} 集`); } else toast(app, '已经是最后一集'); } return; }
      if (d === 'mute') { pa.innerHTML = pa.classList.toggle('muted') ? svg('vol', 'i', 'opacity:.45') : svg('vol'); return; }
      if (d === 'dm') { toggleDm(); return; }
      if (d === 'say') return islSay(true);
      if (d === 'unsay') return islSay(false);
      if (d === 'send') { sendDm(); return; }
      if (d === 'panel') { if (p.panelTab) closePanel(); else openPanel(Store.has('danmaku') ? 'dm' : 'sub'); return; }
      if (d === 'panel-close') return closePanel();
      if (d === 'pip') return playerToPip();
      if (d === 'fs') { toggleFullscreen(); return; }
      if (d === 'match') { toast(app, '原型：会打开弹幕匹配窗口'); return; }
      if (d === 'more') { capMore(!isl.more); return; }
      if (['sub', 'rate', 'enh'].includes(d)) { islPane(d); return; }
      return;
    }
    if (!e.target.closest('.il, .il-cap, .app-ppanel')) {
      if (isl.pane || isl.saying) islClose(); else if (p.panelTab) closePanel(); else setPlaying(!p.playing);
    }
  });
  player.addEventListener('dblclick', e => { if (!e.target.closest('.il, .il-cap, .app-ppanel')) toggleFullscreen(); });
  function toggleFullscreen() {
    const el = document.documentElement;
    if (document.fullscreenElement) document.exitFullscreen && document.exitFullscreen();
    else if (el.requestFullscreen) el.requestFullscreen().catch(() => toast(app, '当前环境不允许全屏'));
  }

  const FULL = { x: 0, y: 0, w: 1280, h: 800 };
  function play(opts, srcEl) {
    if (S.device === 'phone') return phonePlay(opts);
    const old = S.player;
    if (old && old.video) { old.video.pause(); }
    S.player = newPlayer(opts);
    buildPlayer(); startTick();
    hideMinis();
    const from = srcEl ? rel(srcEl) : { x: 540, y: 330, w: 200, h: 112 };
    const done = () => { player.style.display = ''; S.mode = 'full'; animInFade($('.ui', player)); wakeUi(); };
    if (RM()) return done();
    player.style.display = 'none';
    fly(S.player.img, from, FULL, srcEl ? 10 : 12, 0, done);
  }
  function hideMinis() { floatMini.style.display = 'none'; pip.style.display = 'none'; slot.innerHTML = ''; }
  function miniHTML(variant = 'row') {
    const p = S.player;
    if (variant === 'side') return `<div class="mini-player mp-side" data-spec="迷你播放器 · 侧栏底部"><div class="thumb mp-thumb" data-act="mini-expand"><img src="${p.img}" alt=""><div class="prog"><b class="mp-pb" style="width:${(p.t / p.dur) * 100}%"></b></div></div><div class="mp-row"><div class="tx"><b>${p.title}</b><span class="cap2">${p.sub2 || '正在播放'} · <span class="mp-time">${fmt(p.t)}</span></span></div><span class="ib" style="width:26px;height:26px" data-act="mini-toggle">${svg(p.playing ? 'pause' : 'play', 'i', 'width:16px;height:16px')}</span><span class="ib x" data-act="mini-close" title="停止播放">${svg('x', 'i', 'width:14px;height:14px')}</span></div></div>`;
    return `<div class="mini-player" data-spec="迷你播放器"><div class="thumb mp-thumb" data-act="mini-expand"><img src="${p.img}" alt=""></div><div class="tx"><b>${p.title}</b><span class="cap2">${p.sub2 || '正在播放'} · <span class="mp-time">${fmt(p.t)}</span></span><div class="line mp-line"><b style="width:${(p.t / p.dur) * 100}%"></b></div></div><span class="ib" style="width:26px;height:26px" data-act="mini-toggle">${svg(p.playing ? 'pause' : 'play', 'i', 'width:16px;height:16px')}</span><span class="ib x" data-act="mini-close" title="停止播放">${svg('x', 'i', 'width:14px;height:14px')}</span></div>`;
  }
  function renderMini() {
    if (S.mode !== 'mini' || !S.player) { slot.innerHTML = ''; floatMini.style.display = 'none'; return; }
    if (sideShown) { floatMini.style.display = 'none'; slot.innerHTML = miniHTML('side'); autoJelly(slot); }
    else { slot.innerHTML = ''; floatMini.innerHTML = miniHTML(); floatMini.style.display = ''; autoJelly(floatMini); }
  }
  function playerToMini() {
    const p = S.player; if (!p) return;
    closePop(); closePanel(); S.mode = 'mini'; renderMini();
    const th = $('.mp-thumb', sideShown ? slot : floatMini);
    player.style.display = 'none';
    if (!th || RM()) return;
    th.style.visibility = 'hidden';
    fly(p.img, FULL, rel(th), 0, 6, () => { th.style.visibility = ''; });
    if (!sideShown) enter(floatMini.firstElementChild, null);
  }
  function miniExpand() {
    const p = S.player; if (!p) return;
    const th = $('.mp-thumb', sideShown ? slot : floatMini) || (pip.style.display === '' ? pip : null);
    const from = th ? rel(th) : null;
    hideMinis(); S.mode = 'full';
    if (!from || RM()) { player.style.display = ''; wakeUi(); return; }
    fly(p.img, from, FULL, 6, 0, () => { player.style.display = ''; animInFade($('.ui', player)); wakeUi(); });
  }
  function playerToPip() {
    const p = S.player; if (!p) return;
    closePop(); closePanel(); S.mode = 'pip';
    pip.innerHTML = `<img src="${p.img}" alt=""><div class="pc"><span class="pib" data-act="pip-toggle">${svg(p.playing ? 'pause' : 'play')}</span><span class="pib" data-act="mini-expand">${svg('full')}</span><span class="pib" data-act="mini-close">${svg('x')}</span></div><div class="prog"><b style="width:${(p.t / p.dur) * 100}%"></b></div>`;
    player.style.display = 'none'; pip.style.display = '';
    if (RM()) return;
    pip.style.visibility = 'hidden';
    fly(p.img, FULL, rel(pip), 0, 12, () => { pip.style.visibility = ''; });
  }
  function stopPlayback() {
    const p = S.player; if (p && p.video) { p.video.pause(); URL.revokeObjectURL(p.video.src); }
    clearInterval(tick);
    const el = sideShown ? slot.firstElementChild : floatMini.style.display === '' ? floatMini.firstElementChild : pip.style.display === '' ? pip : null;
    const fin = () => { S.player = null; S.mode = 'closed'; hideMinis(); player.innerHTML = ''; };
    if (el && !RM()) leave(el, sideShown ? 'h' : null, fin); else fin();
  }
  function playFile(file) {
    const name = file.name.replace(/\.[^.]+$/, '');
    play({ file, title: name });
    RECENTS.unshift({ t: file.name, prog: 0, left: '—', aid: null });
    if (RECENTS.length > 5) RECENTS.pop();
  }
  fileIn.addEventListener('change', () => { const f = fileIn.files && fileIn.files[0]; if (f) playFile(f); fileIn.value = ''; });

  /* 拖放：真的可以拖一个视频进来播放 */
  let dragN = 0;
  app.addEventListener('dragenter', e => { e.preventDefault(); dragN++; dropcover.style.display = 'grid'; });
  app.addEventListener('dragover', e => e.preventDefault());
  app.addEventListener('dragleave', () => { dragN = Math.max(0, dragN - 1); if (!dragN) dropcover.style.display = 'none'; });
  app.addEventListener('drop', e => { e.preventDefault(); dragN = 0; dropcover.style.display = 'none'; const f = e.dataTransfer.files && e.dataTransfer.files[0]; if (f) playFile(f); });

  /* ------------------------------------------------------------------
     桌面点击分发
     ------------------------------------------------------------------ */
  const ACT = {
    nav: el => { if (S.detail != null && S.page === el.dataset.page) closeDetail(); else go(el.dataset.page); },
    detail: el => openDetail(el.dataset.aid, el.classList.contains('poster') ? el : el.querySelector('.poster')),
    back: () => closeDetail(),
    cw: el => { const c = CW[+el.dataset.i]; play({ aid: c.id, ep: c.ep }, el); },
    play: el => { const src = el.classList.contains('thumb') ? el : (el.closest('.dt-head') ? $('.dt-head .poster', page) : el); const a = byId(el.dataset.aid), ep = +el.dataset.ep || 1; play({ aid: a.id, ep, start: ep === a.w + 1 ? null : 0 }, src); },
    recent: el => { const r = RECENTS[+el.dataset.i]; if (r.aid != null) play({ aid: r.aid, ep: r.ep }, el); else play({ title: r.t.replace(/\.[^.]+$/, ''), aid: null, start: r.prog * DUR }, el); },
    settings: () => openSettings(),
    'close-settings': () => closeTop(),
    'close-top': () => closeTop(),
    'set-sec': el => { if (!overlays.find(o => o.el === setModal)) openSettings(el.dataset.sec); else { S.setSec = el.dataset.sec; renderSetSideNow(false); renderSetContent(); } },
    palette: () => openPalette(),
    'open-file': () => fileIn.click(),
    'open-link': () => openLinkDialog(),
    'link-go': () => { const v = $('.url', dialog).value.trim(); closeTop(); if (/^https?:\/\/.+\.(mp4|webm|m3u8|mov)(\?|$)/i.test(v)) play({ url: v, title: v.split('/').pop() }); else play({ title: v || '网络视频', aid: null }); },
    'lib-seg': el => { S.lib.seg = el.dataset.seg; renderPage(); },
    'lib-filter': el => { S.lib.filter = el.dataset.f; renderPage(false); },
    'lib-view': el => { S.lib.view = el.dataset.v; renderPage(false); animIn($('.pgrid, .app-page > div:last-child', page) || page, 8); },
    'lib-server': el => { S.lib.server = +el.dataset.i; renderPage(false); },
    'lib-device': el => { S.lib.device = +el.dataset.i; renderPage(false); },
    'lib-sort': el => openMenu(el, [{ t: '最近观看', k: 'recent' }, { t: '名称', k: 'name' }, { t: '评分', k: 'rating' }].map(x => ({ ...x, on: x.k === S.lib.sort })), it => { S.lib.sort = it.k; renderPage(false); }),
    'lib-manage': () => openManage(),
    scan: el => { el.textContent = '扫描中…'; setTimeout(() => { el.textContent = '立即扫描'; toast(sheet, '扫描完成，新增 2 集'); }, 1200); },
    'new-day': el => { S.newDay = +el.dataset.d; renderPage(false); stagger($$('.pgrid .pcard', page), 0); },
    'dl-toggle': el => { const d = DL[+el.dataset.i]; d.on = !d.on; renderPage(false); },
    'dl-play': () => play({ title: '[ANi] Jujutsu Kaisen - 05', aid: 5, ep: 5, start: 0 }),
    'dl-add': () => { dialog.innerHTML = `<div class="t2">添加下载任务</div><input class="url" placeholder="粘贴磁力链接或种子地址"><div style="display:flex;justify-content:flex-end;gap:10px"><span class="b" data-act="close-top">取消</span><span class="b p" data-act="dl-go">添加</span></div>`; autoJelly(dialog); openOverlay(dialog, 'dialog'); },
    'dl-go': () => { closeTop(); DL.unshift({ t: '新任务 · 正在获取元数据…', size: '—', prog: 0.01, speed: 0.5, on: true }); renderPage(false); },
    'dt-tab': el => { S.detailTab = el.dataset.tab; $$('[data-act="dt-tab"]', page).forEach(x => x.classList.toggle('on', x === el)); const b = $('.dt-tab-body', page); b.innerHTML = detailTab(byId(S.detail)); autoJelly(b); animIn(b, 8); },
    'dt-status': el => openMenu(el, ['想看', '在看', '看过', '搁置', '抛弃'].map(t => ({ t, on: $('.dt-st', el).textContent === t })), it => { $('.dt-st', el).textContent = it.t; toast(app, Store.has('sync') ? `已标记为「${it.t}」并同步到 Bangumi` : `已标记为「${it.t}」`); }),
    'dt-rate': el => openMenu(el, [10, 9, 8, 7, 6, 5].map(n => ({ t: `${'★'.repeat(Math.round(n / 2))} ${n} 分` })), it => toast(app, `评分：${it.t.split(' ')[1]} 分`)),
    'dt-more': el => openMenu(el, [Store.has('danmaku') && { t: '手动匹配弹幕…', icon: 'danmaku' }, { t: '在访达中显示', icon: 'folder' }, Store.has('anime') && { t: '刷新番剧信息', icon: 'sync' }, '-', { t: '从媒体库移除', icon: 'x' }].filter(Boolean), it => toast(app, `原型：${it.t}`)),
    'mini-expand': () => miniExpand(),
    'mini-toggle': () => setPlaying(!S.player.playing, false),
    'mini-close': () => stopPlayback(),
    'pip-toggle': () => { setPlaying(!S.player.playing, false); const b = $('[data-act="pip-toggle"]', pip); if (b) b.innerHTML = svg(S.player.playing ? 'pause' : 'play'); },
    'sync-now': el => { el.textContent = '同步中…'; setTimeout(() => { el.textContent = '立即同步'; toast(setModal.querySelector('.win'), '已同步 128 条观看记录'); }, 1000); },
    'plug-cfg': () => { S.setSec = 'plugins'; setContent.innerHTML = `<div class="ph-back" data-act="set-sec" data-sec="plugins" style="margin-bottom:10px">${svg('chev-l')}插件</div><div class="lt" style="margin:0 0 12px">防剧透</div><div class="body" style="margin-bottom:6px">这一页由插件提交的声明式描述生成，控件由宿主用原生控件渲染。</div>` + sGroup('设置', [sRow('启用防剧透', sTg(true)), sRow('接口', sPull('OpenAI 兼容', 'OpenAI 兼容|Gemini')), sRow('接口地址', '<span class="cap2">https://api.example.com/v1</span>'), sRow('API Key', '<span class="cap2">••••••••</span>'), sRow('屏蔽强度', sSl(60)), sRow('<span class="b plain" style="padding:0"><span>测试连接</span></span>', '')]) + sGroup('权限', [sRow('访问网络', '<span class="cap2">net.fetch</span>'), sRow('读取和隐藏弹幕', '<span class="cap2">danmaku.modify</span>')]); autoJelly(setContent); animIn(setContent, 8); },
    rerun: () => { closeTop(); setTimeout(openOnboarding, 200); },
    'reset-all': () => { Store.set(PRESETS.anime.mods); NP.setForceRM(false); setTheme('system'); NP.setAccent('rose'); renderSetContent(); toast(setModal.querySelector('.win'), '已恢复默认设置'); },
  };
  app.addEventListener('click', e => {
    const el = e.target.closest('[data-act]');
    if (el && app.contains(el) && !player.contains(el) && ACT[el.dataset.act]) { ACT[el.dataset.act](el, e); return; }
  });

  /* 设置页里的通用控件：开关、滑块、下拉、步进、分段 */
  function bindGenericControls(host) {
    host.addEventListener('click', e => {
      const t = e.target.closest('.tg');
      if (t && !t.closest('.row[data-m]') && !t.closest('[data-pp],[data-po]') && !t.dataset.po && !t.dataset.pp) {
        if (t.hasAttribute('data-rm')) { const v = !NP.getForceRM(); NP.setForceRM(v); t.classList.toggle('on', v); return; }
        t.classList.toggle('on'); return;
      }
      const st = e.target.closest('.stepper .ib');
      if (st) { const b = $('b', st.parentElement), unit = st.parentElement.dataset.unit || ''; const n = parseInt(b.textContent, 10) + (+st.dataset.step) * (unit ? 5 : 1); b.textContent = Math.max(0, n) + unit; return; }
      const op = e.target.closest('[data-opts]');
      if (op) { const opts = op.dataset.opts.split('|'), pv = $('.pv', op); openMenu(op, opts.map(o => ({ t: o, on: o === pv.textContent })), it => { pv.textContent = it.t; }, host.closest('.app') ? app : host); return; }
      const sg = e.target.closest('.seg > span');
      if (sg && !sg.dataset.act && !sg.dataset.setTheme && !sg.dataset.ps && !sg.dataset.pp && !sg.dataset.po && !sg.dataset.dev) { $$(':scope > span', sg.parentElement).forEach(x => x.classList.toggle('on', x === sg)); }
      const ps = e.target.closest('[data-ps]');
      if (ps) { renderPlugins(ps.dataset.ps, '全部'); return; }
      const cat = e.target.closest('.plug-cats [data-cat]');
      if (cat) { $$('.plug-cats [data-cat]', host).forEach(x => x.classList.toggle('on', x === cat)); renderPlugins('market', cat.dataset.cat); }
    });
    host.addEventListener('pointerdown', e => {
      const sl = e.target.closest('.sl'); if (!sl || sl.closest('.vol')) return;
      const set = ev => { const r = sl.getBoundingClientRect(); const x = Math.min(1, Math.max(0, (ev.clientX - r.left) / r.width)) * 100; const b = $('b', sl), i = $('i', sl); if (b) b.style.width = x + '%'; if (i) i.style.left = x + '%'; };
      set(e);
      const mv = ev => set(ev), up = () => { removeEventListener('pointermove', mv); removeEventListener('pointerup', up); };
      addEventListener('pointermove', mv); addEventListener('pointerup', up);
    });
  }
  bindGenericControls(setModal);
  bindGenericControls(sheet);
  bindGenericControls(player);

  /* 键盘 */
  addEventListener('keydown', e => {
    const mod = e.metaKey || e.ctrlKey, inInput = /INPUT|TEXTAREA/.test((document.activeElement || {}).tagName || '');
    if (S.device !== 'desk') return;
    if (mod && e.key.toLowerCase() === 'k') { e.preventDefault(); if (overlays.find(o => o.el === palette)) closePalette(); else if (pagesFor(Store.mods).length) openPalette(); return; }
    if (mod && e.key === ',') { e.preventDefault(); openSettings(); return; }
    if (mod && e.key.toLowerCase() === 'o') { e.preventDefault(); fileIn.click(); return; }
    if (e.key === 'Escape') {
      if (menuEl.style.display === '') { popOut(menuEl); return; }
      if (overlays.length && closeTop()) return;
      if (S.mode === 'full') { if (isl.more || isl.pane || isl.saying) islClose(); else if (S.player.panelTab) closePanel(); else playerToMini(); return; }
      if (S.detail != null) closeDetail();
      return;
    }
    if (inInput || overlays.length) return;
    if (S.mode === 'full' && S.player) {
      const p = S.player;
      if (e.key === ' ') { e.preventDefault(); setPlaying(!p.playing); }
      else if (e.key.toLowerCase() === 'd') toggleDm();
      else if (e.key.toLowerCase() === 'f') toggleFullscreen();
      else if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') { p.t = Math.max(0, Math.min(p.dur, p.t + (e.key === 'ArrowRight' ? 5 : -5))); if (p.video) p.video.currentTime = p.t; updTime(); centerFb(e.key === 'ArrowRight' ? 'ff10' : 'rw10'); wakeUi(); }
    }
  });

  /* 下载进度动画 */
  setInterval(() => {
    let changed = false;
    DL.forEach((d, i) => { if (d.on && d.prog < 1) { d.prog = Math.min(1, d.prog + d.speed / 900); changed = true; const b = $(`.dl-bar b[data-i="${i}"]`, page), m = $(`.dl-meta[data-i="${i}"]`, page); if (b) b.style.width = d.prog * 100 + '%'; if (m) m.textContent = dlMeta(d); if (d.prog >= 1) { d.on = false; if (S.page === 'dl' && S.detail == null) renderPage(false); } } });
    const n = DL.filter(d => d.prog < 1).length; $$('.dl-count', app).forEach(x => (x.textContent = n || ''));
    return changed;
  }, 500);

  /* 模块变化 → 整个应用重排 */
  Store.on(() => {
    renderChrome(true);
    if (S.detail == null || !$('.dt-head', page)) renderPage(false);
    else { const b = $('.dt-tab-body', page); if (b) renderPage(false); }
    if (S.player && S.mode === 'full') { const tab = S.player.panelTab; buildPlayer(); if (tab) openPanel(tab, false); }
    if (overlays.find(o => o.el === setModal)) renderSetSideNow(true);
    renderPhone();
  });

  /* ------------------------------------------------------------------
     手机
     ------------------------------------------------------------------ */
  const M = { tab: 'home', stack: [], land: false, libSeg: 'local', day: TODAY, q: '' };
  phone.innerHTML = `<div class="ph-status"><span>9:41</span><span style="display:flex;gap:5px;align-items:center"><i style="width:17px;height:10px;border-radius:2px;background:currentColor"></i><i style="width:22px;height:10px;border-radius:3px;border:1.5px solid currentColor"></i></span></div>
    <div class="ph-body"></div><div class="ph-mini" style="display:none"></div><div class="ph-tabs"></div><div class="ph-sheet-dim" style="display:none"></div><div class="ph-sheet" style="display:none"></div><div class="ph-land" style="display:none"></div>`;
  const phBody = $('.ph-body', phone), phTabs = $('.ph-tabs', phone), phMini = $('.ph-mini', phone), phDim = $('.ph-sheet-dim', phone), phSheet = $('.ph-sheet', phone), phLand = $('.ph-land', phone);
  const phStatus = $('.ph-status', phone);
  const tabsFor = m => { const p = pagesFor(m); if (!p.length) return []; return p.concat([{ key: 'search', name: '搜索', icon: 'search' }]).slice(0, 5); };
  const phPoster = (a, w = 110) => `<div style="width:${w}px;flex:none"><div class="poster" data-pact="detail" data-aid="${a.id}"><img src="${P(a.p)}" alt=""></div></div>`;
  function phScreenHTML(sc) {
    const m = Store.mods;
    const title = (t, right = '') => `<div class="ph-nav"><div class="lt" style="font-size:34px;margin:0">${t}</div>${right}</div>`;
    const avatar = `<span class="avatar" data-pact="me" style="width:34px;height:34px;cursor:pointer;background-image:url(assets/girl.png);background-size:170%;background-position:50% 12%"></span>`;
    const back = l => `<div class="ph-nav"><span class="ph-back" data-pact="pop">${svg('chev-l')}${l}</span></div>`;
    switch (sc.type) {
      case 'idle': return `<div style="display:flex;justify-content:flex-end;padding-top:4px"><span class="ib M" data-pact="push" data-sc="settings">${svg('gear')}</span></div>
        <div style="display:flex;flex-direction:column;align-items:center;padding:50px 4px 0;text-align:center">${svg('film', 'i', 'width:52px;height:52px;color:var(--text-3);stroke-width:1.5')}<div class="t1" style="margin:16px 0 6px">打开一个视频</div><div class="body" style="margin-bottom:26px">从「文件」App、相册或链接打开</div>
        <span class="b L p" style="width:240px;height:50px" data-pact="open">打开文件</span><span class="b" style="width:240px;height:44px;margin-top:10px" data-pact="open">打开链接</span>
        <div style="width:100%;margin-top:40px;text-align:left"><div class="t2" style="font-size:15px;margin:0 4px 8px">最近播放</div><div class="row-list">${RECENTS.slice(0, 3).map((r, i) => `<div class="row" style="min-height:56px;cursor:pointer" data-pact="recent" data-i="${i}"><div class="thumb" style="width:72px;flex:none;border-radius:6px"><img src="${r.aid ? P(byId(r.aid).p) : STILL}" alt="" style="object-position:50% 30%"></div><div class="tx"><div class="tt" style="font-size:13px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${r.t}</div><div class="ds">剩 ${r.left}</div></div></div>`).join('')}</div></div></div>`;
      case 'home': return title('首页', avatar) + `<div class="search" style="height:38px;margin:12px 0 22px;border-radius:11px;cursor:pointer" data-pact="tab" data-tab="search">${svg('search')}搜索</div>
        <div class="sec-title"><span class="t2">继续观看</span></div><div class="ph-hrow">${CW.map((c, i) => `<div style="width:250px;flex:none"><div class="thumb" data-pact="cw" data-i="${i}"><img src="${c.img}" alt="" style="object-position:${c.pos}"><div class="prog"><b style="width:${c.prog * 100}%"></b></div></div><div class="tt" style="font-weight:650;margin-top:8px;font-size:14px">${byId(c.id).t}</div><div class="cap2">第 ${c.ep} 集 · 剩 ${c.left} 分钟</div></div>`).join('')}</div>
        <div class="sec-title" style="margin-top:26px"><span class="t2">今日更新</span><span class="more" data-pact="tab" data-tab="new" style="cursor:pointer">新番表 ›</span></div><div class="ph-hrow">${schedule(TODAY).map(a => phPoster(a)).join('')}</div>
        ${m.has('library') ? `<div class="sec-title" style="margin-top:26px"><span class="t2">媒体库 · 最近添加</span></div><div class="ph-hrow">${[1, 2, 9, 10, 12].map(id => phPoster(byId(id))).join('')}</div>` : ''}`;
      case 'new': return title('新番') + `<div class="ph-hrow" style="margin:10px -16px 18px 0">${DAYS.map((d, i) => `<span class="b S${i === M.day ? '' : ''}" data-pact="day" data-d="${i}" style="${i === M.day ? 'background:var(--text);color:var(--wf-bg)' : ''}">${i === TODAY ? '今天' : d}</span>`).join('')}</div><div class="ph-grid">${schedule(M.day).map((a, i) => `<div><div class="poster" data-pact="detail" data-aid="${a.id}"><img src="${P(a.p)}" alt=""></div><div class="tt">${a.t}</div><div class="cap2">${TIMES[i]}</div></div>`).join('')}</div>`;
      case 'lib': { const segs = [['local', '本地'], m.has('sources') && ['server', '服务器'], m.has('remote') && ['share', '共享']].filter(Boolean); if (!segs.find(s => s[0] === M.libSeg)) M.libSeg = 'local'; const ids = M.libSeg === 'local' ? ANIME.map(a => a.id) : M.libSeg === 'server' ? SERVERS[0].ids : DEVICES[0].ids;
        return title('媒体库') + (segs.length > 1 ? `<span class="seg" style="display:flex;margin:12px 0 18px">${segs.map(([k, n]) => `<span style="flex:1" data-pact="libseg" data-seg="${k}" class="${k === M.libSeg ? 'on' : ''}">${n}</span>`).join('')}</span>` : '<div style="height:14px"></div>') + `<div class="ph-grid">${ids.map(id => { const a = byId(id); return `<div><div class="poster" data-pact="detail" data-aid="${a.id}"><img src="${P(a.p)}" alt=""></div><div class="tt">${a.t}</div><div class="cap2">${statusOf(a)}</div></div>`; }).join('')}</div>`; }
      case 'dl': return title('下载') + `<div class="row-list" style="margin-top:14px">${DL.map(d => `<div class="row" style="min-height:66px"><div class="tx"><div class="tt" style="font-size:13px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${d.t}</div><div class="ds">${dlMeta(d)}</div><div class="dl-bar"><b style="width:${d.prog * 100}%"></b></div></div></div>`).join('')}</div>`;
      case 'search': { const q = M.q.trim(); const res = ANIME.filter(a => !q || a.t.includes(q)); return title('搜索') + `<div class="search" style="height:38px;margin:12px 0 16px;border-radius:11px">${svg('search')}<input class="ph-q" value="${q.replace(/"/g, '&quot;')}" placeholder="番剧、文件" style="all:unset;flex:1;font-size:14px;color:var(--text)"></div><div class="row-list">${res.slice(0, 12).map(a => `<div class="row" style="cursor:pointer" data-pact="detail" data-aid="${a.id}"><div class="poster" style="width:34px;flex:none;border-radius:5px"><img src="${P(a.p)}" alt=""></div><div class="tx"><div class="tt" style="font-size:14px">${a.t}</div><div class="ds">${statusOf(a)}</div></div></div>`).join('') || '<div class="row"><span class="cap2">没有找到结果</span></div>'}</div>`; }
      case 'detail': { const a = byId(sc.aid), next = Math.min(a.eps, a.w + 1); return back(sc.from || '返回') + `<div style="display:flex;gap:16px;margin:8px 0 18px"><div class="poster" style="width:120px;flex:none;box-shadow:0 14px 30px -12px rgba(0,0,0,.45)"><img src="${P(a.p)}" alt=""></div><div style="padding-top:6px"><div class="cap2">${a.movie ? '剧场版' : 'TV 动画'} · 共 ${a.eps} 话</div><div class="t1" style="font-size:22px;margin:6px 0 8px">${a.t}</div>${m.has('anime') && a.r ? `<div style="font-size:13px"><b>★ ${a.r}</b> <span class="cap2">Bangumi</span></div>` : ''}</div></div>
        <span class="b L p" style="width:100%;height:50px" data-pact="play" data-aid="${a.id}" data-ep="${next}">${svg('play')}${a.w ? `继续 第 ${next} 集` : '开始观看'}</span>
        <div class="t2" style="font-size:16px;margin:24px 0 10px">剧集</div><div class="row-list">${Array.from({ length: Math.min(a.eps, 8) }, (_, i) => `<div class="row" style="cursor:pointer" data-pact="play" data-aid="${a.id}" data-ep="${i + 1}"><div class="thumb" style="width:96px;flex:none;border-radius:6px">${i < a.w + 1 ? `<img src="${P(a.p)}" alt="" style="object-position:50% ${20 + i * 17 % 60}%">` : `<span style="position:absolute;inset:0;display:grid;place-items:center;font-weight:800;color:var(--text-3)">${String(i + 1).padStart(2, '0')}</span>`}</div><div class="tx"><div class="tt" style="font-size:14px">${i === a.w ? `<span class="acc">第 ${i + 1} 话</span>` : `第 ${i + 1} 话`}</div><div class="ds">${i < a.w ? '已看' : i === a.w ? '继续观看' : '未看'}</div></div></div>`).join('')}</div>`; }
      case 'settings': return back(M.stack.length > 1 || M.tab ? '返回' : '') + `<div class="lt" style="font-size:34px;margin:0 0 6px">设置</div>` + sGroup('', [`<div class="row" style="cursor:pointer" data-pact="push" data-sc="appearance"><div class="mi">${svg('palette')}</div><div class="tx"><div class="tt">外观</div></div><span class="cap2">›</span></div>`, `<div class="row" style="cursor:pointer" data-pact="push" data-sc="optional"><div class="mi">${svg('grid')}</div><div class="tx"><div class="tt">可选功能</div><div class="ds">已开启 ${m.size} 个模块</div></div><span class="cap2">›</span></div>`, `<div class="row" style="cursor:pointer" data-pact="push" data-sc="plugins"><div class="mi">${svg('puzzle')}</div><div class="tx"><div class="tt">插件</div></div><span class="cap2">›</span></div>`]) + sGroup('已开启的模块', MODS.filter(x => m.has(x.id)).map(x => `<div class="row"><div class="mi">${svg(x.icon)}</div><div class="tx"><div class="tt">${x.name}</div></div><span class="cap2">›</span></div>`).concat(m.size ? [] : ['<div class="row"><span class="cap2">纯粹播放，没有开启模块</span></div>'])) + sGroup('', [`<div class="row" style="cursor:pointer" data-pact="push" data-sc="about"><div class="mi">${svg('info')}</div><div class="tx"><div class="tt">关于</div></div><span class="cap2">›</span></div>`]);
      case 'appearance': return back('设置') + `<div class="lt" style="font-size:30px;margin:0 0 6px">外观</div>` + sGroup('主题色', [`<div class="row" style="min-height:84px;justify-content:center"><div class="sw-row" style="gap:28px">${[['rose', '#FF2E55', '绯红'], ['blue', '#2A51D4', '群青'], ['mint', '#07FF94', '荧绿']].map(([k, c, n]) => `<button class="sw" data-set-accent="${k}"><i style="background:${c}"></i>${n}</button>`).join('')}</div></div>`]) + sGroup('外观', [`<div class="row"><span class="seg" style="display:flex;width:100%"><span style="flex:1" data-set-theme="system">跟随系统</span><span style="flex:1" data-set-theme="light">浅色</span><span style="flex:1" data-set-theme="dark">深色</span></span></div>`]);
      case 'optional': return back('设置') + `<div class="lt" style="font-size:30px;margin:0 0 6px">可选功能</div><div class="body" style="font-size:13px">关闭的功能不会显示，也不占用资源。</div><div class="ph-opt"></div>`;
      case 'plugins': return back('设置') + `<div class="lt" style="font-size:30px;margin:0 0 14px">插件</div><div class="ph-mk" style="display:grid;gap:12px"></div>`;
      case 'about': return back('设置') + SEC.about();
      default: return '';
    }
  }
  function curScreen() { return M.stack.length ? M.stack[M.stack.length - 1] : (pagesFor(Store.mods).length ? { type: M.tab } : { type: 'idle' }); }
  function mountScreen(el, sc) {
    el.innerHTML = phScreenHTML(sc);
    el.classList.toggle('plaid-uniform', sc.type === 'idle');
    if (sc.type === 'optional') NP.mountOptional($('.ph-opt', el), Store, phone, null);
    if (sc.type === 'plugins') NP.renderMarket($('.ph-mk', el));
    const q = $('.ph-q', el); if (q) q.addEventListener('input', () => { M.q = q.value; const pos = q.selectionStart; renderPhone(false); const nq = $('.ph-q', phBody); if (nq) { nq.focus(); nq.setSelectionRange(pos, pos); } });
    NP.syncThemeUI();
    autoJelly(el);
  }
  function renderPhone(animate = true, dir = 0) {
    if (M.land) return;
    const tabs = tabsFor(Store.mods);
    if (tabs.length && !tabs.find(t => t.key === M.tab)) M.tab = tabs[0].key;
    phTabs.style.display = tabs.length ? '' : 'none';
    phTabs.style.gridTemplateColumns = `repeat(${tabs.length || 1}, minmax(0, 1fr))`;
    reconcile(phTabs, tabs, it => { const d = document.createElement('div'); d.className = 'ph-tab'; d.dataset.pact = 'tab'; d.dataset.tab = it.key; d.innerHTML = svg(it.icon) + `<span>${it.name}</span>`; return d; }, (el, it) => { el.classList.toggle('on', it.key === M.tab && !M.stack.length); $('span', el).className = it.key === M.tab && !M.stack.length ? 'acc' : ''; }, animate, 'w');
    const old = $('.ph-screen', phBody), el = document.createElement('div');
    el.className = 'ph-screen';
    mountScreen(el, curScreen());
    phBody.appendChild(el);
    if (old) {
      if (dir && !RM()) {
        el.style.transform = `translateX(${dir * 100}%)`;
        tween(TOKENS.smooth, p => { const q = Math.min(1, p); el.style.transform = `translateX(${dir * 100 * (1 - p)}%)`; old.style.transform = `translateX(${-dir * 30 * q}%)`; old.style.opacity = String(1 - q * 0.5); }, () => { old.remove(); el.style.transform = ''; });
      } else { old.remove(); if (animate) animIn(el, 8); }
    }
    phMini.style.display = S.player && S.mode === 'mini' && S.device === 'phone' ? '' : 'none';
    phMini.style.bottom = tabs.length ? '92px' : '14px';
    if (phMini.style.display === '') { phMini.innerHTML = miniHTML().replace(/data-act=/g, 'data-pact='); autoJelly(phMini); }
  }
  function phPush(sc) { M.stack.push(sc); renderPhone(false, 1); }
  function phPop() { M.stack.pop(); renderPhone(false, -1); }
  function phSheetOpen(items) {
    phSheet.innerHTML = items.map((it, i) => `<div class="it" data-i="${i}">${svg(it.icon)}${it.t}</div>`).join('') + '<div class="it" data-i="-1" style="justify-content:center;font-weight:650">取消</div>';
    phSheet._items = items; phDim.style.display = ''; phSheet.style.display = ''; cancel(phSheet._tw);
    phSheet._tw = tween(TOKENS.bouncy, p => { phSheet.style.transform = `translateY(${(1 - p) * 110}%)`; });
  }
  function phSheetClose() { cancel(phSheet._tw); phSheet._tw = tween(TOKENS.smooth, p => { phSheet.style.transform = `translateY(${Math.min(1, p) * 110}%)`; }, () => { phSheet.style.display = 'none'; phDim.style.display = 'none'; }); }
  phDim.addEventListener('click', phSheetClose);
  phSheet.addEventListener('click', e => { const it = e.target.closest('.it'); if (!it) return; const i = +it.dataset.i; phSheetClose(); if (i >= 0) phSheet._items[i].run(); });
  function phonePlay(opts) {
    const old = S.player; if (old && old.video) old.video.pause();
    S.player = newPlayer(opts); startTick();
    setLand(true);
  }
  function renderLand() {
    const p = S.player; if (!p) return;
    const dm = Store.has('danmaku'), dmi = dm && Store.has('sync'), r = (p.t / p.dur) * 100;
    const sb = (ic, act, size, isz) => `<span class="pbtn glass" style="width:${size}px;height:${size}px" data-pact="${act}">${svg(ic, 'i', `width:${isz}px;height:${isz}px`)}</span>`;
    phLand.innerHTML = `<div class="player isl" style="border-radius:30px">${p.video ? '' : `<img class="video" src="${p.img}" alt="" style="object-position:center 25%">`}
      <div class="dm-layer" style="inset:58px 0 110px;${dm && p.dmOn ? '' : 'display:none'}">${LINES.slice(0, 7).map(([t, c], i) => `<span style="top:${(i % 5) * 34}px;font-size:17px;animation-duration:${11 + ((i * 7) % 6)}s;animation-delay:${-((i * 3.7) % 11)}s${c ? ';color:' + c : ''}">${t}</span>`).join('')}</div>
      <div class="np-sub" style="bottom:80px"${p.sub === 2 ? ' hidden' : ''}><span style="font-size:19px">${SUB_LINES[Math.floor(p.t / 4) % SUB_LINES.length]}</span></div>
      <div class="center-fb"></div>
      <div class="il-cap l glass" style="left:44px;top:14px;height:36px;max-width:440px"><span class="pbtn sm" data-pact="land-back">${svg('chev-l')}</span><b>${p.title}</b><span class="m">${p.sub2}</span></div>
      <div class="il-cap glass" style="right:44px;top:14px;height:36px">${dm ? `<span class="pbtn sm il-dm${p.dmOn ? ' on' : ''}" data-pact="land-dm">${svg(p.dmOn ? 'danmaku' : 'danmaku-off')}<i class="led" style="right:3px;top:3px"></i></span>` : ''}<span class="pbtn sm">${svg('more')}</span></div>
      <div style="position:absolute;left:0;right:0;top:50%;transform:translateY(-50%);display:flex;justify-content:center;align-items:center;gap:56px">${sb('rw10', 'land-rw', 52, 26)}${sb(p.playing ? 'pause' : 'play', 'land-play', 68, 30)}${sb('ff10', 'land-ff', 52, 26)}</div>
      <div class="il glass" style="left:44px;right:44px;width:auto;height:52px;bottom:14px;transform:none;border-radius:26px">
        <div class="il-row" style="left:0;right:0;height:52px;padding:0 10px 0 14px">
          <span class="il-t mp-time" style="width:42px">${fmt(p.t)}</span>
          <div class="il-pg" style="height:30px"><div class="msk" style="${gapMask(p)}"><div class="tr" style="top:13.5px"></div><div class="bd land-b" style="width:${r}%;top:12.5px"></div></div></div>
          <span class="il-t" style="width:46px">${fmt(p.dur)}</span><i class="il-sep"></i>
          ${dmi ? '<span class="il-chip" style="height:30px;padding:0 12px;font-size:12px;font-weight:500;color:rgba(255,255,255,.6)">发弹幕…</span>' : ''}
          <span class="il-txt">${rateLabel(p.rate)}</span><span class="pbtn sm">${svg('cc')}</span><span class="pbtn sm">${svg('episodes')}</span><span class="pbtn sm">${svg('sliders')}</span>
        </div>
      </div></div>`;
    if (p.video) $('.player', phLand).insertBefore(p.video, $('.player', phLand).firstChild);
    $$('.pbtn', phLand).forEach(b => bindJelly(b, 'S'));
  }
  function setLand(on) {
    M.land = on;
    const portrait = [phStatus, phBody, phTabs, phMini];
    if (on) { renderLand(); portrait.forEach(x => (x.style.visibility = 'hidden')); phLand.style.display = ''; phone.classList.add('land'); }
    else { phLand.style.display = 'none'; phLand.innerHTML = ''; portrait.forEach(x => (x.style.visibility = '')); phone.classList.remove('land'); S.mode = S.player ? 'mini' : 'closed'; renderPhone(false); }
    fitStage();
    if (!RM()) { phone.style.transform = `rotate(${on ? 90 : -90}deg) scale(.7)`; tween(TOKENS.hero, p => { phone.style.transform = `rotate(${(on ? 90 : -90) * (1 - p)}deg) scale(${0.7 + 0.3 * p})`; }, () => { phone.style.transform = ''; }); }
  }
  const PACT = {
    tab: el => { M.tab = el.dataset.tab; M.stack = []; renderPhone(true); },
    push: el => phPush({ type: el.dataset.sc }),
    pop: () => phPop(),
    detail: el => phPush({ type: 'detail', aid: +el.dataset.aid, from: { home: '首页', new: '新番', lib: '媒体库', search: '搜索' }[M.tab] || '返回' }),
    cw: el => { const c = CW[+el.dataset.i]; phonePlay({ aid: c.id, ep: c.ep }); },
    play: el => { const a = byId(el.dataset.aid), ep = +el.dataset.ep; phonePlay({ aid: a.id, ep, start: ep === a.w + 1 ? null : 0 }); },
    recent: el => { const r = RECENTS[+el.dataset.i]; phonePlay(r.aid != null ? { aid: r.aid, ep: r.ep } : { title: r.t, aid: null }); },
    open: () => fileIn.click(),
    day: el => { M.day = +el.dataset.d; renderPhone(false); },
    libseg: el => { M.libSeg = el.dataset.seg; renderPhone(false); },
    me: () => phSheetOpen([Store.has('sync') && { t: '账号 · 弹弹play / Bangumi', icon: 'user', run: () => toast(phone, '已登录 · Nipa') }, { t: '观看历史', icon: 'list', run: () => toast(phone, '原型：观看历史') }, { t: '设置', icon: 'gear', run: () => phPush({ type: 'settings' }) }].filter(Boolean)),
    'land-back': () => setLand(false),
    'land-play': () => { setPlaying(!S.player.playing, false); renderLand(); centerFb(S.player.playing ? 'play' : 'pause', phLand); },
    'land-rw': () => { const p = S.player; p.t = Math.max(0, p.t - 10); if (p.video) p.video.currentTime = p.t; centerFb('rw10', phLand); },
    'land-ff': () => { const p = S.player; p.t = Math.min(p.dur, p.t + 10); if (p.video) p.video.currentTime = p.t; centerFb('ff10', phLand); },
    'land-dm': () => { S.player.dmOn = !S.player.dmOn; renderLand(); },
    'mini-expand': () => setLand(true),
    'mini-toggle': () => { setPlaying(!S.player.playing, false); renderPhone(false); },
    'mini-close': () => { stopPlayback(); renderPhone(false); },
  };
  phone.addEventListener('click', e => { const el = e.target.closest('[data-pact]'); if (el && PACT[el.dataset.pact]) PACT[el.dataset.pact](el, e); });
  setInterval(() => { if (M.land && S.player) { const r = S.player.t / S.player.dur; const b = $('.land-b', phLand), k = $('.land-k', phLand); if (b) b.style.width = r * 100 + '%'; if (k) k.style.left = r * 100 + '%'; } }, 250);

  /* ------------------------------------------------------------------
     原型工具栏 / 启动
     ------------------------------------------------------------------ */
  function setDevice(d) {
    if (S.device === d) return;
    if (S.player && S.mode === 'full') playerToMini();
    if (M.land) setLand(false);
    S.device = d; kv.set('app.device', d);
    $$('#pbDevice span').forEach(s => s.classList.toggle('on', s.dataset.dev === d));
    fitStage();
    if (d === 'phone') { if (S.player) S.mode = 'mini'; renderPhone(true); }
    else { renderChrome(false); renderPage(false); }
    const f = d === 'desk' ? deskFrame : phoneFrame;
    if (!RM()) { f.firstElementChild.style.opacity = '0'; tween(TOKENS.smooth, p => { f.firstElementChild.style.opacity = String(Math.min(1, p)); }, () => { f.firstElementChild.style.opacity = ''; }); }
  }
  $$('#pbDevice span').forEach(s => s.addEventListener('click', () => setDevice(s.dataset.dev)));
  $$('#pbDevice span').forEach(s => s.classList.toggle('on', s.dataset.dev === S.device));
  NP.onTheme(t => { $('#pbTheme').innerHTML = svg(t === 'dark' ? 'sun' : 'moon'); });
  $('#pbTheme').addEventListener('click', () => setTheme(root.dataset.theme === 'dark' ? 'light' : 'dark'));
  $('#pbRerun').addEventListener('click', openOnboarding);
  if (EMBED) { $('#pbDoc').style.display = 'none'; $('#pbFull').style.display = ''; }

  renderChrome(false);
  renderPage(false);
  renderPhone(false);
  fitStage();
  NP.syncThemeUI();
  autoJelly(document);
  if (!kv.get('app.done')) setTimeout(openOnboarding, 400);
})();
