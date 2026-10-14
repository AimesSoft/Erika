/* =====================================================================
   Erika · 设计文档交互（共享部分在 core.js）
   ===================================================================== */
(() => {
  'use strict';

  const {
    $, $$, svg, kv, root, TOKENS, DEFAULTS, RM, Spring, run, tween, cancel, bindJelly,
    enter, leave, setTheme, MODS, MOD, watchFrames, autoJelly,
  } = NP;

  /* ------------------------------------------------------------------
     功能分层 · 模块列表
     ------------------------------------------------------------------ */
  function initTier() {
    const ul = $('#tierMods'); if (!ul) return;
    ul.innerHTML = MODS.map(m => `<li>${svg(m.icon)}<span>${m.name}</span>${m.deps.length ? `<span class="tiny faint" style="margin-left:auto">需要${m.deps.map(d => MOD[d].name).join('、')}</span>` : ''}</li>`).join('');
  }

  /* ------------------------------------------------------------------
     文档里的活窗口（第 02、05、10 章）：每个 .nx-live 挂一个 NX.App，.nx-bar 统一切换
     设计语言 / 设备 / 折叠屏姿态 / 功能，选择记在 nx.* 里；data-mods 固定了功能的窗口不跟着预设变；明暗跟随文档
     ------------------------------------------------------------------ */
  function initNX() {
    const lives = $$('.nx-live'); if (!lives.length || !window.NX) return;
    const { App, ALLOWED, fitStage } = NX, { PRESETS } = NP;
    const G = { ds: kv.get('nx.ds') || 'apple', dev: kv.get('nx.dev') || 'desk', preset: kv.get('nx.preset') || 'all', fold: kv.get('nx.fold') || 'open' };
    if (!ALLOWED[G.dev]) G.dev = 'desk';
    if (!PRESETS[G.preset]) G.preset = 'all';
    const fixDs = () => { if (!ALLOWED[G.dev].includes(G.ds)) { G.ds = 'apple'; kv.set('nx.ds', G.ds); } };
    fixDs();
    const th = () => (root.dataset.theme === 'dark' ? 'dark' : 'light');
    const fit = (box, rotate) => { if (box._app) fitStage($('.nxp-stage-wrap', box), $('.nxp-stage', box), box._app, box.clientWidth, Infinity, rotate); };
    function mount(box) {
      if (box._app) box._app.destroy();
      box._app = new App($('.nxp-stage', box), {
        start: box.dataset.start, dev: G.dev, ds: G.ds, th: th(), fold: G.fold,
        mods: PRESETS[box.dataset.mods || G.preset].mods,
        onLand: r => fit(box, r === 'fold' ? 'fold' : true), onTheme: t => setTheme(t),
      });
      fit(box);
    }
    const live = () => lives.filter(b => b._app && !b._app.dead);
    /* 快滚到时才挂上，十几扇窗口不在打开文档时一起初始化 */
    const io = new IntersectionObserver(es => es.forEach(e => { if (e.isIntersecting && !e.target._app) { mount(e.target); io.unobserve(e.target); } }), { rootMargin: '600px 0px' });
    lives.forEach(box => {
      io.observe(box);
      new ResizeObserver(() => fit(box)).observe(box);
      const r = $('[data-nx-reset]', box); if (r) r.addEventListener('click', () => mount(box));
    });
    /* 统一的时钟：播放进度、下载进度 */
    let n = 0;
    setInterval(() => { n++; live().forEach(b => { b._app.plTick(); if (n % 2) b._app.dlTick(); }); }, 250);
    NP.onTheme(t => live().forEach(b => { if (b._app.cfg.th !== t) b._app.setLook(G.ds, t); }));

    const sync = () => {
      $$('.nx-bar [data-g] > span[data-v]').forEach(s => {
        const k = s.parentElement.dataset.g;
        s.classList.toggle('on', s.dataset.v === G[k]);
        if (k === 'ds') s.style.display = ALLOWED[G.dev].includes(s.dataset.v) ? '' : 'none';
      });
      document.body.classList.toggle('dev-tv', G.dev === 'tv');
      document.body.classList.toggle('dev-fold', G.dev === 'fold');
    };
    const ON = {
      ds: () => live().forEach(b => b._app.setLook(G.ds, b._app.cfg.th)),
      dev: () => { fixDs(); live().forEach(mount); },
      preset: () => live().forEach(b => { if (!b.dataset.mods) b._app.store.set(PRESETS[G.preset].mods); }),
      fold: () => live().forEach(b => b._app.setFold(G.fold)),
    };
    $$('.nx-bar [data-g] > span[data-v]').forEach(s => s.addEventListener('click', () => {
      const k = s.parentElement.dataset.g; if (G[k] === s.dataset.v) return;
      G[k] = s.dataset.v; kv.set('nx.' + k, G[k]); ON[k](); sync();
    }));
    sync();
    document.fonts && document.fonts.load('24px "Material Symbols Rounded"', 'play_arrow').then(f => { if (f && f.length) root.classList.add('msr-ok'); }).catch(() => {});
  }

  /* ------------------------------------------------------------------
     动效实验台
     ------------------------------------------------------------------ */
  function initLab() {
    const lab = $('#lab'); if (!lab) return;
    const box = $('#labTokens'), Ti = $('#labT'), Zi = $('#labZ');
    let sel = 'jelly';
    box.innerHTML = Object.keys(TOKENS).map(k => `<button data-k="${k}">${k}</button>`).join('');
    function simulate(t) {
      const s = new Spring(0).cfg(t); s.target = 1;
      const pts = [], dt = 1 / 240; let settle = 0;
      for (let i = 0; i <= 1.2 / dt; i++) {
        pts.push(s.v);
        if (Math.abs(s.v - 1) > 0.01 || Math.abs(s.vel) > 0.05) settle = i * dt;
        s.step(dt);
      }
      return { pts, settle };
    }
    function draw() {
      const t = TOKENS[sel], { pts, settle } = simulate(t);
      const X = i => (i / (pts.length - 1)) * 300, Y = v => 100 - v * 70;
      const d = pts.map((v, i) => `${i ? 'L' : 'M'}${X(i).toFixed(1)} ${Y(v).toFixed(1)}`).join(' ');
      const peak = Math.max(...pts);
      $('#labCurve').innerHTML = `<line x1="0" x2="300" y1="30" y2="30" stroke="currentColor" stroke-opacity=".25" stroke-dasharray="3 3"/><line x1="0" x2="300" y1="100" y2="100" stroke="currentColor" stroke-opacity=".12"/><path d="${d}" fill="none" stroke="var(--accent)" stroke-width="2.5" vector-effect="non-scaling-stroke"/>`;
      const k = (2 * Math.PI / t.T) ** 2, c = 4 * Math.PI * t.z / t.T;
      $('#labConv').innerHTML =
        `<b>SwiftUI</b> .spring(response: ${t.T.toFixed(2)}, dampingFraction: ${t.z.toFixed(2)})<br>` +
        `<b>Core Animation</b> mass 1 · stiffness ${k.toFixed(1)} · damping ${c.toFixed(2)}<br>` +
        `<b>Android</b> SpringForce().setStiffness(${k.toFixed(1)}f).setDampingRatio(${t.z.toFixed(2)}f)<br>` +
        `<b>WinUI</b> DampingRatio ${t.z.toFixed(2)} · Period ${t.T.toFixed(2)}s<br>` +
        `<b>libadwaita</b> adw_spring_params_new(${t.z.toFixed(2)}, 1.0, ${k.toFixed(1)})<br>` +
        `<b>过冲</b> ${Math.max(0, (peak - 1) * 100).toFixed(0)}% · <b>稳定</b> ≈ ${settle.toFixed(2)}s`;
    }
    function sync() {
      const t = TOKENS[sel];
      Ti.value = t.T; Zi.value = t.z;
      $('#labTv').textContent = t.T.toFixed(2) + ' s'; $('#labZv').textContent = t.z.toFixed(2);
      $$('button', box).forEach(b => b.classList.toggle('on', b.dataset.k === sel));
      draw();
    }
    box.addEventListener('click', e => { const b = e.target.closest('button'); if (b) { sel = b.dataset.k; sync(); } });
    Ti.addEventListener('input', () => { TOKENS[sel].T = +Ti.value; sync(); });
    Zi.addEventListener('input', () => { TOKENS[sel].z = +Zi.value; sync(); });
    $('#labReset').addEventListener('click', () => { for (const k in DEFAULTS) Object.assign(TOKENS[k], DEFAULTS[k]); sync(); });
    const rm = $('#labRM');
    rm.closest('label').addEventListener('click', () => { const v = !NP.getForceRM(); NP.setForceRM(v); rm.classList.toggle('on', v); });
    sync();

    const heart = $('#labHeart');
    heart.addEventListener('click', () => {
      const on = heart.classList.toggle('liked');
      heart.style.color = on ? 'var(--accent-solo)' : '';
      $('svg', heart).style.fill = on ? 'currentColor' : '';
    });
    const tg = $('#labTg');
    tg.parentElement.addEventListener('click', () => tg.classList.toggle('on'));
    let n = 0;
    const pops = $('#labPops');
    $('#labAdd').addEventListener('click', () => {
      const live = $$('.chip', pops).filter(c => !c._leaving);
      if (live.length >= 4) { const f = live[0]; f._leaving = true; leave(f, 'w', () => f.remove()); }
      const c = document.createElement('span');
      c.className = 'chip'; c.textContent = ['#治愈', '#热血', '#百合', '#日常', '#悬疑', '#机战'][n++ % 6];
      c.style.cursor = 'pointer';
      c.addEventListener('click', () => { if (c._leaving) return; c._leaving = true; leave(c, 'w', () => c.remove()); });
      pops.appendChild(c); enter(c, null);
    });
    const vid = $('#labVideo'), ci = $('.center-icon', vid);
    let playing = true;
    vid.addEventListener('click', () => {
      playing = !playing;
      ci.innerHTML = svg(playing ? 'play' : 'pause');
      cancel(ci._tw); clearTimeout(ci._h);
      ci._tw = tween(TOKENS.jelly, p => { ci.style.transform = `scale(${0.6 + 0.4 * p})`; ci.style.opacity = String(Math.min(1, p * 2)); });
      ci._h = setTimeout(() => { ci._tw = tween(TOKENS.smooth, p => { ci.style.opacity = String(Math.max(0, 1 - p)); ci.style.transform = `scale(${1 + 0.15 * Math.min(1, p)})`; }); }, 420);
    });
    const pd = $('#labPanel'), pn = $('.pn', pd);
    let open = false;
    pd.addEventListener('click', () => {
      open = !open; cancel(pn._tw);
      const from = open ? 120 : 0, to = open ? 0 : 120;
      pn._tw = tween(open ? TOKENS.bouncy : TOKENS.smooth, p => { pn.style.transform = `translateX(${from + (to - from) * (open ? p : Math.min(1, p))}%)`; });
    });
    const seg = $('#labSeg'), ind = $('.ind', seg), spans = $$('span', seg);
    const ix = new Spring(0), iw = new Spring(0);
    const indA = { sp: [ix, iw], eps: 0.05, apply() { ind.style.transform = `translateX(${ix.v}px)`; ind.style.width = iw.v + 'px'; } };
    function moveTo(sp, animate) {
      spans.forEach(s => { s.classList.toggle('on', s === sp); s.style.boxShadow = 'none'; s.style.background = 'transparent'; });
      ix.cfg(TOKENS.snappy).target = sp.offsetLeft - 2; iw.cfg(TOKENS.snappy).target = sp.offsetWidth;
      if (!animate || RM()) { ix.snap(); iw.snap(); indA.apply(); } else run(indA);
    }
    spans.forEach(s => { s.addEventListener('click', () => moveTo(s, true)); bindJelly(s, 'M'); });
    requestAnimationFrame(() => moveTo(spans[0], false));
  }

  /* ------------------------------------------------------------------
     目录高亮
     ------------------------------------------------------------------ */
  function initToc() {
    const links = $$('.toc a');
    const map = new Map(links.map(a => [a.getAttribute('href').slice(1), a]));
    const io = new IntersectionObserver(es => {
      es.forEach(e => {
        if (!e.isIntersecting) return;
        links.forEach(l => l.classList.remove('on'));
        const a = map.get(e.target.id); if (a) a.classList.add('on');
      });
    }, { rootMargin: '-35% 0px -60% 0px' });
    map.forEach((a, id) => { const el = document.getElementById(id); if (el) io.observe(el); });
  }

  /* ------------------------------------------------------------------
     启动
     ------------------------------------------------------------------ */
  NP.onTheme(t => { const b = $('#themeBtn'); if (b) b.innerHTML = svg(t === 'dark' ? 'sun' : 'moon'); });
  NP.syncThemeUI();
  $('#themeBtn').addEventListener('click', () => setTheme(root.dataset.theme === 'dark' ? 'light' : 'dark'));

  initTier();
  initNX();
  initLab();
  initToc();

  watchFrames();
  autoJelly();
})();
