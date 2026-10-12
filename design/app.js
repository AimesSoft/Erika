/* =====================================================================
   Erika · 交互原型（画框版）：一整扇可操作的窗口，顶栏切换设备和设计语言
   界面全部来自 next-*.js 的 NX.App，和设计预览 next.html 是同一套；
   这里只管：第一次打开先走首次启动、记住模块选择、缩放、时钟、键盘
   ===================================================================== */
(() => {
  'use strict';
  const { $, $$, svg, kv, root, bindJelly, PRESETS, setTheme } = NP;
  const { App, ALLOWED, fitStage } = NX;
  const qs = new URLSearchParams(location.search);
  const EMBED = qs.has('embed'), START = qs.get('start');
  const MODS_KEY = 'proto.mods';
  const G = { dev: kv.get('proto.dev') || 'desk', ds: kv.get('proto.ds') || 'apple' };
  if (!ALLOWED[G.dev]) G.dev = 'desk';
  const fixDs = () => { if (!ALLOWED[G.dev].includes(G.ds)) G.ds = 'apple'; };
  fixDs();
  const stage = $('#stage'), wrap = $('#wrap'), host = $('#host');
  let app = null;

  const fit = rotate => { if (app) fitStage(wrap, host, app, stage.clientWidth - 32, stage.clientHeight - 32, rotate); };
  /* 还没存过模块选择 = 第一次打开 */
  const firstRun = () => kv.get(MODS_KEY) == null;
  function mount(start) {
    if (app) app.destroy();
    app = new App(host, {
      start, dev: G.dev, ds: G.ds, th: root.dataset.theme === 'dark' ? 'dark' : 'light',
      mods: PRESETS.anime.mods, persist: MODS_KEY,
      onLand: () => fit(true),
      onTheme: t => setTheme(t),
    });
    fit();
    hint();
  }
  /* 换设备时停在同一页（首次启动没走完就接着走） */
  const here = () => (firstRun() ? 'onboard' : app && !app.pure() ? app.S.page : 'home');

  /* 统一的时钟：播放进度、下载进度 */
  let n = 0;
  setInterval(() => { if (!app || app.dead) return; n++; app.plTick(); if (n % 2) app.dlTick(); }, 250);
  new ResizeObserver(() => fit()).observe(stage);

  /* 键盘：桌面的 ⌘ 快捷键；焦点不在窗口里时（刚点过顶栏、嵌在文档里），按键照样交给窗口 */
  document.addEventListener('keydown', e => {
    if (!app || app.dead) return;
    const mod = e.metaKey || e.ctrlKey, k = e.key.toLowerCase();
    if (mod && G.dev === 'desk' && (k === 'k' || k === ',' || k === 'o')) {
      e.preventDefault();
      if (k === 'k') app.palOpen(''); else if (k === ',') app.setOpen(); else app.q.file.click();
      return;
    }
    if (mod || app.el.contains(e.target) || e.target.closest('input, textarea')) return;
    if (app.tv) { app.tvKey(e); return; }
    if (app.P && app.P.open) { app.plKey(e); return; }
    if (e.key === 'Escape' && app.overlays.length) { app.closeTop(); return; }
    if (e.key === ' ' && G.dev === 'desk' && app.pure()) { const b = $('.i-key[data-act="recent"]', app.el); if (b) { e.preventDefault(); b.click(); } }
  });

  /* 顶栏 */
  const HINT = {
    desk: '快捷键：<kbd>⌘K</kbd> 搜索 · <kbd>⌘,</kbd> 设置 · <kbd>⌘O</kbd> 打开文件 · 播放时 <kbd>空格</kbd> 暂停、<kbd>Esc</kbd> 返回 · 本地视频可以直接拖进窗口',
    tablet: '平板：没有悬停，▶ 常驻在缩略图上；横放，播放即全屏。',
    phone: '手机：开始播放时转成横屏，返回时转回竖屏；搜索是一个标签页。',
    tv: '电视：先点一下原型，再用 <kbd>↑</kbd> <kbd>↓</kbd> <kbd>←</kbd> <kbd>→</kbd> 移动焦点，<kbd>回车</kbd> 确认，<kbd>Esc</kbd> 返回（遥控器的方向键、确认键和返回键）。',
  };
  function hint() { $('#pbHint').innerHTML = HINT[G.dev]; }
  function syncBar() {
    $$('#pbDev span[data-v]').forEach(s => s.classList.toggle('on', s.dataset.v === G.dev));
    /* 设计语言只列这种设备上真的会出现的 */
    $$('#pbDs span[data-v]').forEach(s => { s.style.display = ALLOWED[G.dev].includes(s.dataset.v) ? '' : 'none'; s.classList.toggle('on', s.dataset.v === G.ds); });
  }
  $$('#pbDev span[data-v]').forEach(s => s.addEventListener('click', () => {
    if (G.dev === s.dataset.v) return;
    const at = here();
    G.dev = s.dataset.v; fixDs(); kv.set('proto.dev', G.dev); kv.set('proto.ds', G.ds);
    syncBar(); mount(at);
  }));
  $$('#pbDs span[data-v]').forEach(s => s.addEventListener('click', () => {
    if (G.ds === s.dataset.v) return;
    G.ds = s.dataset.v; kv.set('proto.ds', G.ds);
    syncBar(); app.setLook(G.ds, app.cfg.th);
  }));
  const tb = $('#pbTheme');
  NP.onTheme(t => { tb.innerHTML = svg(t === 'dark' ? 'sun' : 'moon'); if (app && app.cfg.th !== t) app.setLook(G.ds, t); });
  NP.syncThemeUI();
  tb.addEventListener('click', () => setTheme(root.dataset.theme === 'dark' ? 'light' : 'dark'));
  $('#pbRerun').addEventListener('click', () => mount('onboard'));
  if (EMBED) { $('#pbDoc').style.display = 'none'; $('#pbFull').style.display = ''; }
  $$('.pb-sw, .pb-btn').forEach(b => bindJelly(b, 'S'));
  $$('.pb-txt').forEach(b => bindJelly(b, 'M'));
  document.fonts && document.fonts.load('24px "Material Symbols Rounded"', 'play_arrow').then(f => { if (f && f.length) root.classList.add('msr-ok'); }).catch(() => {});

  syncBar();
  mount(START || (firstRun() ? 'onboard' : 'home'));
})();
