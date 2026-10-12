/* =====================================================================
   Erika · 画框版 —— 预览页：每一节一个可操作的窗口，顶部统一切换外观
   ===================================================================== */
(() => {
  'use strict';
  const { $, $$, svg, bindJelly, PRESETS, root } = NP;
  const get = (k, d) => { try { return localStorage.getItem('erika.nx.' + k) || d; } catch (e) { return d; } };
  const put = (k, v) => { try { localStorage.setItem('erika.nx.' + k, v); } catch (e) { /* 无痕模式 */ } };
  const G = { ds: get('ds', 'apple'), th: get('th', 'light'), dev: get('dev', 'desk'), preset: get('preset', 'all'), fold: get('fold', 'open') };
  const { ALLOWED, fitStage } = NX;
  if (!ALLOWED[G.dev]) G.dev = 'desk';
  const fixDs = () => { if (!ALLOWED[G.dev].includes(G.ds)) { G.ds = 'apple'; put('ds', G.ds); } };
  fixDs();
  const secs = $$('.nxp-sec[data-start]');

  function fit(sec, rotate) {
    const app = sec._app; if (!app || sec.dataset.insp) return;
    fitStage($('.nxp-stage-wrap', sec), $('.nxp-stage', sec), app, sec.clientWidth, Infinity, rotate);
  }
  function mount(sec) {
    if (sec._app) sec._app.destroy();
    const fixed = sec.dataset.mods;
    sec._app = new NX.App($('.nxp-stage', sec), {
      start: sec.dataset.start, dev: G.dev, ds: G.ds, th: G.th, fold: G.fold,
      mods: (fixed ? PRESETS[fixed] : PRESETS[G.preset]).mods,
      onLand: r => fit(sec, r === 'fold' ? 'fold' : true),
    });
    fit(sec);
  }
  secs.forEach(sec => {
    mount(sec);
    new ResizeObserver(() => fit(sec)).observe(sec);
    const r = $('[data-reset]', sec); if (r) r.addEventListener('click', () => mount(sec));
  });

  /* 统一的时钟：播放进度、下载进度 */
  let n = 0;
  setInterval(() => { n++; secs.forEach(s => { const a = s._app; if (!a || a.dead) return; a.plTick(); if (n % 2) a.dlTick(); }); }, 250);

  /* 顶部开关 */
  const syncDs = () => {
    $$('#gDs span[data-v]').forEach(s => { s.style.display = ALLOWED[G.dev].includes(s.dataset.v) ? '' : 'none'; s.classList.toggle('on', s.dataset.v === G.ds); });
    document.body.classList.toggle('dev-tv', G.dev === 'tv');
    document.body.classList.toggle('dev-fold', G.dev === 'fold');
  };
  const seg = (id, key, onSet) => {
    const el = $(id); if (!el) return;
    const sync = () => $$('span[data-v]', el).forEach(s => s.classList.toggle('on', s.dataset.v === G[key]));
    $$('span[data-v]', el).forEach(s => s.addEventListener('click', () => { if (G[key] === s.dataset.v) return; G[key] = s.dataset.v; put(key, G[key]); sync(); onSet(); }));
    sync();
  };
  seg('#gDs', 'ds', () => secs.forEach(s => s._app && s._app.setLook(G.ds, G.th)));
  seg('#gTh', 'th', () => secs.forEach(s => s._app && s._app.setLook(G.ds, G.th)));
  seg('#gDev', 'dev', () => { fixDs(); syncDs(); secs.forEach(mount); });
  syncDs();
  /* 折叠屏的姿态：合上 / 展开 / 悬停，所有窗口一起换 */
  seg('#gFold', 'fold', () => secs.forEach(s => s._app && s._app.setFold(G.fold)));
  seg('#gPre', 'preset', () => secs.forEach(s => { if (!s.dataset.mods && s._app) s._app.store.set(PRESETS[G.preset].mods); }));
  document.fonts && document.fonts.load('24px "Material Symbols Rounded"', 'play_arrow').then(f => { if (f && f.length) root.classList.add('msr-ok'); }).catch(() => {});

  const tb = $('#themeBtn');
  NP.onTheme(t => { tb.innerHTML = svg(t === 'dark' ? 'sun' : 'moon'); });
  NP.syncThemeUI();
  tb.addEventListener('click', () => NP.setTheme(root.dataset.theme === 'dark' ? 'light' : 'dark'));
  $$('.dock button, .nxp-bar .sw-b').forEach(b => bindJelly(b, 'S'));
})();
