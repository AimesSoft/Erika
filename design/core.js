/* =====================================================================
   NipaPlay Next · 共享核心（设计文档 index.html 与交互原型 app.html 共用）
   - 图标 sprite
   - 弹簧引擎（T / ζ 参数，与文档 8.2 节一致）、果冻按压、出现 / 消失
   - 主题色 / 明暗（跨页面同步）
   - 线框稿缩放
   - 模块定义与状态、迷你主窗口、首次配置、设置侧栏、可选功能、插件市场
   ===================================================================== */
window.NP = (() => {
  'use strict';

  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const root = document.documentElement;
  const svg = (id, cls = 'i', style = '') => `<svg class="${cls}"${style ? ` style="${style}"` : ''}><use href="#i-${id}"/></svg>`;
  const kv = {
    get(k) { try { return localStorage.getItem('npn.' + k); } catch (e) { return null; } },
    set(k, v) { try { localStorage.setItem('npn.' + k, v); } catch (e) { /* 无痕模式等 */ } },
    del(k) { try { localStorage.removeItem('npn.' + k); } catch (e) { /* ignore */ } },
  };

  /* 图标 sprite（演示用；实现时映射到各平台系统符号） */
  const SPRITE = `<svg width="0" height="0" style="position:absolute" aria-hidden="true">
  <defs>
    <symbol id="i-play" viewBox="0 0 24 24"><path d="M8 5.2v13.6l10.6-6.8z" fill="currentColor" stroke="none"/></symbol>
    <symbol id="i-pause" viewBox="0 0 24 24"><rect x="6.5" y="5" width="4" height="14" rx="1.2" fill="currentColor" stroke="none"/><rect x="13.5" y="5" width="4" height="14" rx="1.2" fill="currentColor" stroke="none"/></symbol>
    <symbol id="i-next" viewBox="0 0 24 24"><path d="M6 6v12l8.5-6z" fill="currentColor" stroke="none"/><path d="M17.5 6v12"/></symbol>
    <symbol id="i-prev" viewBox="0 0 24 24"><path d="M18 6v12L9.5 12z" fill="currentColor" stroke="none"/><path d="M6.5 6v12"/></symbol>
    <symbol id="i-vol" viewBox="0 0 24 24"><path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z" fill="currentColor" stroke="none"/><path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11"/></symbol>
    <symbol id="i-danmaku" viewBox="0 0 24 24"><rect x="3" y="4" width="18" height="16" rx="3.5"/><path d="M7 9h6M10 12.5h7M7 16h4"/></symbol>
    <symbol id="i-danmaku-off" viewBox="0 0 24 24"><rect x="3" y="4" width="18" height="16" rx="3.5"/><path d="M7 9h3M13 12.5h4M7 16h4M4 3l16 18"/></symbol>
    <symbol id="i-cc" viewBox="0 0 24 24"><rect x="3" y="5" width="18" height="14" rx="3"/><path d="M7 11h10M7 14.5h6"/></symbol>
    <symbol id="i-audio" viewBox="0 0 24 24"><path d="M4 10v4M8 7v10M12 4v16M16 8v8M20 11v2"/></symbol>
    <symbol id="i-speed" viewBox="0 0 24 24"><path d="M4.5 17a8.5 8.5 0 1 1 15 0"/><path d="M12 13l4-4"/></symbol>
    <symbol id="i-sliders" viewBox="0 0 24 24"><path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2.2"/><circle cx="9" cy="17" r="2.2"/></symbol>
    <symbol id="i-gear" viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/><path d="M19.4 13.5a7.6 7.6 0 0 0 0-3l2-1.5-2-3.4-2.3.9a7.6 7.6 0 0 0-2.6-1.5L14 2.5h-4l-.5 2.5a7.6 7.6 0 0 0-2.6 1.5l-2.3-.9-2 3.4 2 1.5a7.6 7.6 0 0 0 0 3l-2 1.5 2 3.4 2.3-.9a7.6 7.6 0 0 0 2.6 1.5l.5 2.5h4l.5-2.5a7.6 7.6 0 0 0 2.6-1.5l2.3.9 2-3.4z"/></symbol>
    <symbol id="i-pip" viewBox="0 0 24 24"><rect x="3" y="5" width="18" height="14" rx="2.5"/><rect x="12" y="11" width="7" height="6" rx="1.2" fill="currentColor" stroke="none"/></symbol>
    <symbol id="i-full" viewBox="0 0 24 24"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/></symbol>
    <symbol id="i-search" viewBox="0 0 24 24"><circle cx="11" cy="11" r="6.5"/><path d="M16 16l4.5 4.5"/></symbol>
    <symbol id="i-plus" viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></symbol>
    <symbol id="i-folder" viewBox="0 0 24 24"><path d="M3.5 7.5a2 2 0 0 1 2-2h4l2 2.5h7a2 2 0 0 1 2 2v7.5a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z"/></symbol>
    <symbol id="i-link" viewBox="0 0 24 24"><path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1"/><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/></symbol>
    <symbol id="i-home" viewBox="0 0 24 24"><path d="M4 10.5L12 4l8 6.5V19a1.5 1.5 0 0 1-1.5 1.5H15v-6H9v6H5.5A1.5 1.5 0 0 1 4 19z"/></symbol>
    <symbol id="i-calendar" viewBox="0 0 24 24"><rect x="3.5" y="5" width="17" height="15.5" rx="2.5"/><path d="M3.5 10h17M8 3v4M16 3v4"/></symbol>
    <symbol id="i-library" viewBox="0 0 24 24"><rect x="3.5" y="8" width="17" height="12.5" rx="2.5"/><path d="M6 5h12M8.5 2.5h7"/></symbol>
    <symbol id="i-download" viewBox="0 0 24 24"><path d="M12 4v11M7 10.5l5 5 5-5M5 20h14"/></symbol>
    <symbol id="i-puzzle" viewBox="0 0 24 24"><path d="M10 4.5a2 2 0 1 1 4 0V6h4a1 1 0 0 1 1 1v4h-1.5a2 2 0 1 0 0 4H19v4a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1v-4h1.5a2 2 0 1 0 0-4H5V7a1 1 0 0 1 1-1h4z"/></symbol>
    <symbol id="i-chev-l" viewBox="0 0 24 24"><path d="M15 5l-7 7 7 7"/></symbol>
    <symbol id="i-chev-r" viewBox="0 0 24 24"><path d="M9 5l7 7-7 7"/></symbol>
    <symbol id="i-chev-d" viewBox="0 0 24 24"><path d="M6 9l6 6 6-6"/></symbol>
    <symbol id="i-more" viewBox="0 0 24 24"><circle cx="5.5" cy="12" r="1.6" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.6" fill="currentColor" stroke="none"/><circle cx="18.5" cy="12" r="1.6" fill="currentColor" stroke="none"/></symbol>
    <symbol id="i-star" viewBox="0 0 24 24"><path d="M12 3.5l2.6 5.4 5.9.8-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.8z"/></symbol>
    <symbol id="i-heart" viewBox="0 0 24 24"><path d="M12 20s-7.5-4.6-7.5-10A4.3 4.3 0 0 1 12 7.3 4.3 4.3 0 0 1 19.5 10c0 5.4-7.5 10-7.5 10z"/></symbol>
    <symbol id="i-check" viewBox="0 0 24 24"><path d="M5 12.5l4.5 4.5L19 7.5"/></symbol>
    <symbol id="i-server" viewBox="0 0 24 24"><rect x="4" y="4" width="16" height="7" rx="2"/><rect x="4" y="13" width="16" height="7" rx="2"/><path d="M8 7.5h.01M8 16.5h.01"/></symbol>
    <symbol id="i-wifi" viewBox="0 0 24 24"><path d="M4.5 10a11 11 0 0 1 15 0M7.5 13.5a6.5 6.5 0 0 1 9 0"/><circle cx="12" cy="17.5" r="1.3" fill="currentColor" stroke="none"/></symbol>
    <symbol id="i-sparkle" viewBox="0 0 24 24"><path d="M11 3.5l1.8 5.2 5.2 1.8-5.2 1.8L11 17.5l-1.8-5.2L4 10.5l5.2-1.8z"/><path d="M18.5 15l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z"/></symbol>
    <symbol id="i-sync" viewBox="0 0 24 24"><path d="M19.5 9A8 8 0 0 0 5 7.5M4.5 15A8 8 0 0 0 19 16.5"/><path d="M19.5 4v5h-5M4.5 20v-5h5"/></symbol>
    <symbol id="i-user" viewBox="0 0 24 24"><circle cx="12" cy="8.5" r="3.8"/><path d="M4.5 20a7.5 7.5 0 0 1 15 0"/></symbol>
    <symbol id="i-film" viewBox="0 0 24 24"><rect x="3.5" y="4.5" width="17" height="15" rx="2.5"/><path d="M10 9.2v5.6l4.6-2.8z" fill="currentColor" stroke="none"/></symbol>
    <symbol id="i-grid" viewBox="0 0 24 24"><rect x="4" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5"/></symbol>
    <symbol id="i-list" viewBox="0 0 24 24"><path d="M9 6h11M9 12h11M9 18h11M4.5 6h.01M4.5 12h.01M4.5 18h.01"/></symbol>
    <symbol id="i-sun" viewBox="0 0 24 24"><circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4"/></symbol>
    <symbol id="i-moon" viewBox="0 0 24 24"><path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z"/></symbol>
    <symbol id="i-send" viewBox="0 0 24 24"><path d="M4 12l16-7-6 16-2.5-6.5z"/></symbol>
    <symbol id="i-x" viewBox="0 0 24 24"><path d="M6 6l12 12M18 6L6 18"/></symbol>
    <symbol id="i-shield" viewBox="0 0 24 24"><path d="M12 3.5l7 3v5.5c0 4.5-3 7.5-7 8.5-4-1-7-4-7-8.5V6.5z"/></symbol>
    <symbol id="i-scissors" viewBox="0 0 24 24"><circle cx="6.5" cy="7" r="2.5"/><circle cx="6.5" cy="17" r="2.5"/><path d="M8.5 8.5L20 18M8.5 15.5L20 6"/></symbol>
    <symbol id="i-eyeoff" viewBox="0 0 24 24"><path d="M3 3l18 18"/><path d="M10.6 6.2A9.8 9.8 0 0 1 12 6c5 0 8.5 4.5 9.5 6-.5.8-1.5 2.2-3 3.5M6.3 7.8C4.5 9 3.3 10.8 2.5 12c1 1.5 4.5 6 9.5 6 1.6 0 3-.4 4.3-1.1"/></symbol>
    <symbol id="i-tv" viewBox="0 0 24 24"><rect x="3" y="6" width="18" height="13" rx="3"/><path d="M8 3l4 3 4-3"/></symbol>
    <symbol id="i-external" viewBox="0 0 24 24"><path d="M14 4h6v6M20 4l-9 9M18 14v4.5a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 4 18.5v-11A1.5 1.5 0 0 1 5.5 6H10"/></symbol>
    <symbol id="i-filter" viewBox="0 0 24 24"><path d="M4 5h16l-6 7.5V19l-4-2v-4.5z"/></symbol>
    <symbol id="i-music" viewBox="0 0 24 24"><path d="M9 18V6l11-2v12"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="17.5" cy="16" r="2.5"/></symbol>
    <symbol id="i-palette" viewBox="0 0 24 24"><path d="M12 3.5a8.5 8.5 0 1 0 0 17c1.2 0 1.8-.8 1.8-1.7 0-1.2-1-1.5-1-2.6 0-1 .8-1.7 1.8-1.7h2.2a3.7 3.7 0 0 0 3.7-3.7c0-4.2-3.8-7.3-8.5-7.3z"/><circle cx="7.5" cy="11" r="1.1" fill="currentColor" stroke="none"/><circle cx="10.5" cy="7.5" r="1.1" fill="currentColor" stroke="none"/><circle cx="15" cy="8" r="1.1" fill="currentColor" stroke="none"/></symbol>
    <symbol id="i-keyboard" viewBox="0 0 24 24"><rect x="2.5" y="6" width="19" height="12" rx="2.5"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10"/></symbol>
    <symbol id="i-camera" viewBox="0 0 24 24"><path d="M4 8.5A1.5 1.5 0 0 1 5.5 7h2.3L9.5 4.5h5L16.2 7h2.3A1.5 1.5 0 0 1 20 8.5v9a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 17.5z"/><circle cx="12" cy="12.5" r="3.3"/></symbol>
    <symbol id="i-info" viewBox="0 0 24 24"><circle cx="12" cy="12" r="8.5"/><path d="M12 11v5M12 8h.01"/></symbol>
    <symbol id="i-code" viewBox="0 0 24 24"><path d="M8.5 7L3.5 12l5 5M15.5 7l5 5-5 5"/></symbol>
    <symbol id="i-annot" viewBox="0 0 24 24"><path d="M3.5 8V3.5H8M16 3.5h4.5V8M20.5 16v4.5H16M8 20.5H3.5V16"/><rect x="8" y="8" width="8" height="8" rx="1.5"/></symbol>
    <symbol id="i-rw10" viewBox="0 0 24 24"><path d="M4.5 12a7.5 7.5 0 1 0 2.2-5.3"/><path d="M4.5 3.5V8H9"/><text x="12.3" y="15.3" text-anchor="middle" font-size="7.5" font-weight="700" fill="currentColor" stroke="none" font-family="system-ui,sans-serif">10</text></symbol>
    <symbol id="i-ff10" viewBox="0 0 24 24"><path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3"/><path d="M19.5 3.5V8H15"/><text x="11.7" y="15.3" text-anchor="middle" font-size="7.5" font-weight="700" fill="currentColor" stroke="none" font-family="system-ui,sans-serif">10</text></symbol>
    <symbol id="i-sort" viewBox="0 0 24 24"><path d="M7 5v14M4 16l3 3 3-3M17 19V5M14 8l3-3 3 3"/></symbol>
    <symbol id="i-episodes" viewBox="0 0 24 24"><rect x="3.5" y="4" width="17" height="11" rx="2"/><path d="M6 19h12"/></symbol>
  </defs>
</svg>`;
  if (!document.getElementById('i-play')) document.body.insertAdjacentHTML('afterbegin', SPRITE);

  /* ------------------------------------------------------------------
     弹簧引擎
     ------------------------------------------------------------------ */
  const TOKENS = {
    press: { T: 0.12, z: 1 },
    jelly: { T: 0.42, z: 0.42 },
    bouncy: { T: 0.5, z: 0.62 },
    hero: { T: 0.48, z: 0.86 },
    smooth: { T: 0.45, z: 1 },
    snappy: { T: 0.22, z: 0.85 },
  };
  const DEFAULTS = JSON.parse(JSON.stringify(TOKENS));
  const mqRM = matchMedia('(prefers-reduced-motion: reduce)');
  let forceRM = false;
  const RM = () => forceRM || mqRM.matches;

  class Spring {
    constructor(v = 0) { this.v = v; this.vel = 0; this.target = v; this.cfg(TOKENS.smooth); }
    cfg(t) { this.k = (2 * Math.PI / t.T) ** 2; this.c = 4 * Math.PI * t.z / t.T; return this; }
    step(dt) {
      const n = Math.max(1, Math.ceil(dt * 600)), h = dt / n;
      for (let i = 0; i < n; i++) {
        const a = -this.k * (this.v - this.target) - this.c * this.vel;
        this.vel += a * h; this.v += this.vel * h;
      }
    }
    rest(eps) { return Math.abs(this.vel) < eps * 4 && Math.abs(this.v - this.target) < eps; }
    snap() { this.v = this.target; this.vel = 0; }
  }

  const running = new Set();
  let raf = 0, last = 0;
  function loop(t) {
    const dt = last ? Math.min(0.034, (t - last) / 1000) : 1 / 60;
    last = t;
    for (const a of [...running]) {
      let done = true;
      for (const s of a.sp) { s.step(dt); if (!s.rest(a.eps || 0.0008)) done = false; }
      if (done) a.sp.forEach(s => s.snap());
      a.apply();
      if (done) { running.delete(a); a.done && a.done(); }
    }
    if (running.size) raf = requestAnimationFrame(loop); else { raf = 0; last = 0; }
  }
  function run(a) { running.add(a); if (!raf) { last = 0; raf = requestAnimationFrame(loop); } return a; }
  function stop(a) { if (a) running.delete(a); }

  /** 0 → 1 的弹簧进度；减少动态效果时改为 150ms 无回弹 */
  function tween(token, update, done) {
    if (RM()) {
      const t0 = performance.now();
      const o = { cancelled: false };
      const step = now => {
        if (o.cancelled) return;
        const p = Math.min(1, (now - t0) / 150);
        update(1 - (1 - p) * (1 - p));
        if (p < 1) requestAnimationFrame(step); else done && done();
      };
      requestAnimationFrame(step);
      return o;
    }
    const s = new Spring(0).cfg(token); s.target = 1;
    return run({ sp: [s], eps: 0.001, apply() { update(s.v); }, done });
  }
  function cancel(t) { if (!t) return; if ('cancelled' in t) t.cancelled = true; else stop(t); }

  /* 按压果冻：缩放变换加在元素自身（对应实现中的外层容器） */
  function J(el) {
    if (el._j) return el._j;
    const sx = new Spring(1), sy = new Spring(1);
    const a = {
      sp: [sx, sy], eps: 0.0006,
      apply() { el.style.transform = (sx.v === 1 && sy.v === 1) ? '' : `scale(${sx.v.toFixed(4)},${sy.v.toFixed(4)})`; },
    };
    return (el._j = { a, sx, sy });
  }
  function scaleTo(el, x, y, tx, ty) {
    const j = J(el);
    j.sx.cfg(tx).target = x; j.sy.cfg(ty || tx).target = y;
    if (RM()) { j.sx.snap(); j.sy.snap(); j.a.apply(); return; }
    run(j.a);
  }
  const PROFILES = {
    S: { down: [0.84, 0.84], hover: 1.08 },
    M: { down: [0.94, 0.94], hover: 1.03 },
    L: { down: [0.97, 0.9], hover: 1.03, squash: true },
    card: { down: [0.97, 0.97], hover: 1.03 },
    tg: { down: [0.9, 0.9], hover: 1 },
  };
  function bindJelly(el, kind) {
    if (el._jb) return; el._jb = 1;
    const p = PROFILES[kind] || PROFILES.M;
    let pressed = false, hovered = false;
    const release = () => {
      const h = hovered ? p.hover : 1, jt = TOKENS.jelly;
      if (p.squash) scaleTo(el, h, h, jt, { T: jt.T * 0.82, z: Math.max(0.15, jt.z * 0.85) });
      else scaleTo(el, h, h, jt);
    };
    el.addEventListener('pointerdown', e => {
      if (e.button > 0) return;
      pressed = true; scaleTo(el, p.down[0], p.down[1], TOKENS.press);
    });
    const up = () => { if (!pressed) return; pressed = false; release(); };
    el.addEventListener('pointerup', up);
    el.addEventListener('pointercancel', up);
    el.addEventListener('pointerenter', e => {
      if (e.pointerType !== 'mouse') return;
      hovered = true; if (!pressed) scaleTo(el, p.hover, p.hover, TOKENS.snappy);
    });
    el.addEventListener('pointerleave', () => {
      hovered = false;
      if (pressed) { pressed = false; release(); } else scaleTo(el, 1, 1, TOKENS.snappy);
    });
  }
  function wobble(el) {
    scaleTo(el, 0.95, 0.95, TOKENS.press);
    setTimeout(() => scaleTo(el, 1, 1, TOKENS.jelly), 90);
  }
  function autoJelly(scope = document) {
    $$('[data-j]', scope).forEach(el => bindJelly(el, el.dataset.j));
    $$('.frame .b.L, .preset', scope).forEach(el => bindJelly(el, el.classList.contains('preset') ? 'card' : 'L'));
    $$('.frame .b:not(.L), .frame .seg > span, .frame .nav-item, .frame .row[data-m]', scope).forEach(el => bindJelly(el, 'M'));
    $$('.frame .ib, .frame .pib, .dock button, .accent-pick .sw', scope).forEach(el => bindJelly(el, 'S'));
    $$('.frame .poster, .frame .thumb', scope).forEach(el => {
      if (!el.closest('.mini-player') && !el.closest('.mw') && !el.closest('[data-nojelly]')) bindJelly(el, 'card');
    });
    $$('.frame .tg, .switch-line .tg', scope).forEach(el => bindJelly(el, 'tg'));
  }

  /* 元素出现 / 消失：尺寸用 smooth，缩放用 jelly */
  function cancelAnims(el) { (el._anims || []).forEach(cancel); el._anims = []; }
  function clearInline(el) {
    ['width', 'height', 'opacity', 'transform', 'overflow', 'pointerEvents'].forEach(p => (el.style[p] = ''));
  }
  function enter(el, axis = 'h') {
    cancelAnims(el); clearInline(el);
    if (RM()) return;
    const prop = axis === 'h' ? 'height' : axis === 'w' ? 'width' : null;
    const full = prop ? (axis === 'h' ? el.offsetHeight : el.offsetWidth) : 0;
    if (prop) { el.style.overflow = 'hidden'; el.style[prop] = '0px'; }
    el.style.transform = 'scale(.4)'; el.style.opacity = '0';
    if (prop) el._anims.push(tween(TOKENS.smooth, p => { el.style[prop] = full * Math.min(1, p) + 'px'; }, () => { el.style[prop] = ''; el.style.overflow = ''; }));
    el._anims.push(tween(TOKENS.jelly, p => {
      el.style.transform = `scale(${0.4 + 0.6 * p})`; el.style.opacity = String(Math.min(1, p * 1.6));
    }, () => { el.style.transform = ''; el.style.opacity = ''; }));
  }
  function leave(el, axis = 'h', done) {
    cancelAnims(el);
    if (RM()) { done && done(); return; }
    const prop = axis === 'h' ? 'height' : axis === 'w' ? 'width' : null;
    const full = prop ? (axis === 'h' ? el.offsetHeight : el.offsetWidth) : 0;
    el.style.overflow = 'hidden'; el.style.pointerEvents = 'none';
    el._anims.push(tween(TOKENS.smooth, p => {
      const q = Math.min(1, p);
      if (prop) el.style[prop] = full * (1 - q) + 'px';
      el.style.opacity = String(Math.max(0, 1 - q * 1.8));
      el.style.transform = `scale(${1 - 0.35 * q})`;
    }, done));
  }
  function setShown(el, show, axis, animate = true) {
    const visible = el.style.display !== 'none' && !el._hiding;
    if (show === visible) return;
    if (show) {
      el._hiding = false; el.style.display = '';
      if (animate) enter(el, axis); else { cancelAnims(el); clearInline(el); }
    } else {
      if (!animate) { cancelAnims(el); clearInline(el); el.style.display = 'none'; return; }
      el._hiding = true;
      leave(el, axis, () => { if (el._hiding) { el._hiding = false; clearInline(el); el.style.display = 'none'; } });
    }
  }
  /* 有序列表的增删（导航项、控制条按钮、设置侧栏） */
  function reconcile(c, items, make, upd, animate = true, axis = 'h') {
    const existing = new Map();
    [...c.children].forEach(el => { if (el.dataset.key && !el._leaving) existing.set(el.dataset.key, el); });
    const keys = new Set(items.map(i => i.key));
    existing.forEach((el, k) => {
      if (keys.has(k)) return;
      el._leaving = true;
      if (animate) leave(el, axis, () => el.remove()); else el.remove();
    });
    let prev = null;
    const added = [];
    items.forEach(it => {
      let el = existing.get(it.key);
      if (!el) { el = make(it); el.dataset.key = it.key; added.push(el); }
      upd && upd(el, it);
      const ref = prev ? prev.nextSibling : c.firstChild;
      if (ref !== el) c.insertBefore(el, ref);
      prev = el;
    });
    if (animate) added.forEach(el => enter(el, axis));
    if (added.length) autoJelly(c);
  }

  function toast(container, text) {
    let t = container._toast;
    if (!t) { t = document.createElement('div'); t.className = 'toast'; container.appendChild(t); container._toast = t; }
    t.innerHTML = `<span class="sailor"></span>${text}`;
    t.style.display = '';
    const ver = (t._ver = (t._ver || 0) + 1);
    cancel(t._tw);
    t._tw = tween(TOKENS.bouncy, p => {
      t.style.transform = `translate(-50%, ${(1 - p) * 26}px)`; t.style.opacity = String(Math.min(1, p * 2));
    });
    clearTimeout(t._h);
    t._h = setTimeout(() => {
      t._tw = tween(TOKENS.smooth, p => {
        t.style.transform = `translate(-50%, ${p * 16}px)`; t.style.opacity = String(Math.max(0, 1 - p));
      }, () => { if (t._ver === ver) t.style.display = 'none'; });
    }, 2600);
  }

  /* ------------------------------------------------------------------
     主题色 / 明暗（[data-set-accent] / [data-set-theme] 自动生效，跨页面同步）
     ------------------------------------------------------------------ */
  const mqDark = matchMedia('(prefers-color-scheme: dark)');
  let themePref = kv.get('theme') || 'system';
  const themeHooks = [];
  function applyTheme() {
    const t = themePref === 'system' ? (mqDark.matches ? 'dark' : 'light') : themePref;
    root.dataset.theme = t;
    $$('[data-set-theme]').forEach(s => s.classList.toggle('on', s.dataset.setTheme === themePref));
    themeHooks.forEach(f => f(t, themePref));
  }
  function setTheme(p) { themePref = p; kv.set('theme', p); applyTheme(); }
  function setAccent(a) {
    root.dataset.accent = a; kv.set('accent', a);
    $$('[data-set-accent]').forEach(b => b.classList.toggle('on', b.dataset.setAccent === a));
  }
  function syncThemeUI() { setAccent(root.dataset.accent || 'rose'); applyTheme(); }
  if (mqDark.addEventListener) mqDark.addEventListener('change', applyTheme);
  addEventListener('storage', e => {
    if (e.key === 'npn.accent' && e.newValue) setAccent(e.newValue);
    if (e.key === 'npn.theme') { themePref = e.newValue || 'system'; applyTheme(); }
  });
  document.addEventListener('click', e => {
    const a = e.target.closest('[data-set-accent]'); if (a) setAccent(a.dataset.setAccent);
    const t = e.target.closest('[data-set-theme]'); if (t) setTheme(t.dataset.setTheme);
  });
  setAccent(kv.get('accent') || 'rose');
  applyTheme();

  /* ------------------------------------------------------------------
     线框稿缩放
     ------------------------------------------------------------------ */
  function fit() {
    $$('.frame-wrap').forEach(w => {
      const f = w.firstElementChild;
      if (!f || !f.classList.contains('frame')) return;
      const W = +f.dataset.w, H = +f.dataset.h;
      f.style.width = W + 'px'; f.style.height = H + 'px';
      const cw = w.clientWidth; if (!cw) return;
      const s = Math.min(1, cw / W);
      f.style.transform = `translateX(${Math.max(0, (cw - W * s) / 2)}px) scale(${s})`;
      f.style.setProperty('--s', s);
      f._s = s;
      w.style.height = H * s + 'px';
    });
  }
  let fitQueued = false;
  const queueFit = () => { if (fitQueued) return; fitQueued = true; requestAnimationFrame(() => { fitQueued = false; fit(); }); };
  const ro = new ResizeObserver(queueFit);
  ro.observe(document.body);
  function watchFrames(scope = document) {
    $$('.frame-wrap', scope).forEach(w => { if (!w._watched) { w._watched = 1; ro.observe(w); } });
    queueFit();
  }
  const scaleOf = el => { let s = 1, f = el.closest('.frame'); while (f) { s *= f._s || 1; f = f.parentElement && f.parentElement.closest('.frame'); } return s; };

  /* ------------------------------------------------------------------
     模块与状态
     ------------------------------------------------------------------ */
  const MODS = [
    { id: 'danmaku', name: '弹幕', icon: 'danmaku', group: '观看', deps: [], short: '弹弹play 匹配与渲染、发送弹幕', desc: '弹弹play 自动匹配、弹幕渲染与过滤、弹幕列表与偏移、本地弹幕文件' },
    { id: 'enhance', name: '画质增强', icon: 'sparkle', group: '观看', deps: [], short: 'Anime4K / ArtCNN 超分辨率', desc: 'Anime4K 与 ArtCNN 超分辨率，在播放器「画面」分页里调节' },
    { id: 'library', name: '媒体库', icon: 'library', group: '内容', deps: [], short: '扫描文件夹，识别番剧与封面', desc: '扫描本地文件夹，自动识别番剧、封面和观看进度' },
    { id: 'anime', name: '追番', icon: 'calendar', group: '内容', deps: [], short: '首页推荐、新番表、番剧详情', desc: '首页推荐、新番表、番剧详情、Bangumi 评分与短评' },
    { id: 'sync', name: '账号同步', icon: 'sync', group: '内容', deps: ['anime'], short: '弹弹play / Bangumi 同步 · 需要追番', desc: '弹弹play 与 Bangumi 账号，同步进度、评分和历史' },
    { id: 'sources', name: '网络媒体源', icon: 'server', group: '连接', deps: ['library'], short: 'Jellyfin、Emby、WebDAV、SMB · 需要媒体库', desc: '连接 Jellyfin、Emby、WebDAV、SMB' },
    { id: 'remote', name: '局域网与远程', icon: 'wifi', group: '连接', deps: ['library'], short: '局域网共享、远程访问 · 需要媒体库', desc: '局域网共享、Web 远程访问与遥控' },
    { id: 'downloads', name: '下载', icon: 'download', group: '连接', deps: [], short: '种子与磁力链接', desc: '种子与磁力链接下载' },
  ];
  const MOD = Object.fromEntries(MODS.map(m => [m.id, m]));
  const ALL = MODS.map(m => m.id);
  const GROUPS = ['观看', '内容', '连接'];
  const PRESETS = {
    pure: { name: '纯粹播放', desc: '和 VLC 一样，打开就能播，没有多余的东西。', mods: [] },
    anime: { name: '看番', desc: '弹幕、媒体库和追番。推荐大多数人选这个。', mods: ['danmaku', 'library', 'anime'] },
    all: { name: '全都要', desc: '全部功能，和现在的 NipaPlay 一样。', mods: ALL },
    custom: { name: '自己挑', desc: '在当前选择的基础上逐项调整。', mods: null },
  };

  function createStore(initial = ALL, persistKey = null) {
    let saved = null;
    if (persistKey) { try { saved = JSON.parse(kv.get(persistKey)); } catch (e) { saved = null; } }
    const start = Array.isArray(saved) ? saved.filter(x => MOD[x]) : initial;
    return {
      mods: new Set(start),
      subs: new Set(),
      on(f) { this.subs.add(f); },
      emit(info = {}) {
        if (persistKey) kv.set(persistKey, JSON.stringify([...this.mods]));
        this.subs.forEach(f => f(this.mods, info));
      },
      set(list, info) { this.mods = new Set(list); this.emit(info); },
      has(id) { return this.mods.has(id); },
      toggle(id, on = !this.mods.has(id)) {
        const m = new Set(this.mods), added = [], removed = [];
        if (on) {
          const add = x => { if (!m.has(x)) { m.add(x); if (x !== id) added.push(x); } MOD[x].deps.forEach(add); };
          add(id);
        } else {
          const rm = x => { if (m.has(x)) { m.delete(x); if (x !== id) removed.push(x); } MODS.filter(d => d.deps.includes(x)).forEach(d => rm(d.id)); };
          rm(id);
        }
        this.mods = m;
        this.emit({ id, on, added, removed });
        return { added, removed };
      },
    };
  }
  const presetOf = mods => {
    for (const k of ['pure', 'anime', 'all']) {
      const p = PRESETS[k].mods;
      if (p.length === mods.size && p.every(x => mods.has(x))) return k;
    }
    return 'custom';
  };
  const depToast = (container, r) => {
    const list = r.added.length ? r.added : r.removed;
    if (!list.length) return;
    toast(container, (r.added.length ? '已同时开启：' : '已同时关闭：') + list.map(x => MOD[x].name).join('、'));
  };
  function pagesFor(m) {
    const p = [];
    if (m.has('anime')) { p.push({ key: 'home', name: '首页', icon: 'home' }); p.push({ key: 'new', name: '新番', icon: 'calendar' }); }
    if (m.has('library')) p.push({ key: 'lib', name: '媒体库', icon: 'library' });
    if (m.has('downloads')) p.push({ key: 'dl', name: '下载', icon: 'download' });
    return p;
  }

  /* ------------------------------------------------------------------
     迷你主窗口（引导预览 / 导航生长示意）
     ------------------------------------------------------------------ */
  const I6 = '<i></i><i></i><i></i><i></i><i></i><i></i>';
  const BODIES = {
    idle: `<div class="idle"><div>${svg('film', 'i ic')}<span class="b p">打开文件</span><div class="bar h6" style="width:130px"></div><div class="bar h6 t" style="width:96px;margin-top:6px"></div></div></div>`,
    home: `<div class="mt">首页</div><div class="mbar"></div><div class="mrow"><i></i><i></i><i></i><i></i></div><div class="mbar" style="width:30%"></div><div class="mrow p">${I6}</div>`,
    lib: `<div class="mt">媒体库</div><div class="mrow p">${I6}</div><div class="mrow p">${I6}</div><div class="mrow p">${I6}</div>`,
    dl: `<div class="mt">下载</div><div class="bar h8" style="margin:10px 0 14px;width:90%"></div><div class="bar h8" style="margin-bottom:14px;width:80%"></div><div class="bar h8" style="width:86%"></div>`,
  };
  class MiniWin {
    constructor(host) {
      this.host = host;
      host.innerHTML = `<div class="mw"><div class="ms" style="flex:none"><div class="d3"><i></i><i></i><i></i></div><div class="mnav" style="display:flex;flex-direction:column;gap:2px"></div><div class="mfoot"><i></i><b></b></div></div><div class="mc"><div class="mtb"><div class="d3"><i></i><i></i><i></i></div><span class="grow"></span><span class="sbx"></span></div><div class="mbody"></div></div></div>`;
      this.ms = $('.ms', host); this.nav = $('.mnav', host); this.d3 = $('.mtb .d3', host);
      this.sbx = $('.sbx', host); this.body = $('.mbody', host);
      this.side = null; this.bodyKey = null;
    }
    update(m, animate = true) {
      const pages = pagesFor(m), side = pages.length >= 2;
      if (side !== this.side) { this.setSide(side, animate && this.side !== null); this.side = side; }
      this.d3.style.visibility = side ? 'hidden' : 'visible';
      this.sbx.style.visibility = pages.length ? 'visible' : 'hidden';
      reconcile(this.nav, pages.map((p, i) => ({ ...p, on: i === 0 })), it => {
        const d = document.createElement('div'); d.className = 'mn';
        d.innerHTML = svg(it.icon) + `<span>${it.name}</span>`; return d;
      }, (el, it) => el.classList.toggle('on', it.on), animate);
      const key = pages.length ? (pages[0].key === 'new' ? 'home' : pages[0].key) : 'idle';
      if (key !== this.bodyKey) {
        this.body.innerHTML = BODIES[key];
        if (animate && this.bodyKey !== null && !RM()) {
          const b = this.body;
          tween(TOKENS.smooth, p => { b.style.opacity = String(Math.min(1, p)); b.style.transform = `translateY(${(1 - p) * 10}px)`; }, () => { b.style.transform = ''; });
        }
        this.bodyKey = key;
      }
    }
    setSide(show, animate) {
      const ms = this.ms;
      const full = (this.host.clientWidth || 440) * 0.27;
      const end = () => {
        ms.style.width = show ? '' : '0px';
        ms.style.padding = show ? '' : '22px 0 6px';
        ms.style.borderRightWidth = show ? '' : '0';
        ms.style.overflow = 'hidden';
      };
      if (!animate) { end(); return; }
      ms.style.padding = ''; ms.style.borderRightWidth = ''; ms.style.overflow = 'hidden';
      const from = show ? 0 : full, to = show ? full : 0;
      tween(TOKENS.smooth, p => { ms.style.width = from + (to - from) * Math.min(1, p) + 'px'; }, end);
    }
  }

  class HudStrip {
    constructor(host) { this.host = host; }
    update(m, animate = true) {
      const items = [
        { key: 'play', html: svg('pause') }, { key: 'next', html: svg('next') }, { key: 'vol', html: svg('vol') },
        { key: 'g1', grow: true },
        m.has('danmaku') && { key: 'dm', html: svg('danmaku'), on: true },
        m.has('danmaku') && m.has('sync') && { key: 'dmi', input: true },
        { key: 'g2', grow: true },
        { key: 'cc', html: svg('cc') }, { key: 'au', html: svg('audio') },
        { key: 'sp', html: '<span class="lbl" style="font-size:11px">1×</span>' },
        m.has('enhance') && { key: 'en', html: svg('sparkle') },
        { key: 'set', html: svg('sliders') }, { key: 'fs', html: svg('full') },
      ].filter(Boolean);
      reconcile(this.host, items, it => {
        let d;
        if (it.grow) { d = document.createElement('span'); d.className = 'grow'; }
        else if (it.input) { d = document.createElement('div'); d.className = 'dm-input'; d.innerHTML = '发弹幕…<span class="send">发送</span>'; }
        else { d = document.createElement('span'); d.className = 'pib' + (it.on ? ' on' : ''); d.innerHTML = it.html; }
        return d;
      }, null, animate, 'w');
    }
  }

  /* ------------------------------------------------------------------
     设置侧栏 + 可选功能页
     ------------------------------------------------------------------ */
  const SET_TOP = [['general', '通用', 'gear'], ['appearance', '外观', 'palette'], ['player', '播放', 'film']];
  const SET_MOD = [['danmaku', '弹幕', 'danmaku'], ['library', '媒体库', 'library'], ['anime', '追番', 'calendar'], ['sync', '账号', 'user'], ['sources', '网络媒体源', 'server'], ['remote', '局域网与远程', 'wifi'], ['downloads', '下载', 'download'], ['enhance', '画质增强', 'sparkle']];
  const SET_BOT = [['sep'], ['optional', '可选功能', 'grid'], ['plugins', '插件', 'puzzle'], ['keys', '快捷键', 'keyboard'], ['advanced', '高级', 'code'], ['about', '关于', 'info']];
  function settingsSections(store) {
    return [...SET_TOP, ...SET_MOD.filter(x => store.has(x[0])), ...SET_BOT].map(([key, n, ic]) => ({ key, n, ic }));
  }
  function renderSetSide(host, sel, store, animate = true, onPick = null) {
    if (!host) return;
    reconcile(host, settingsSections(store), it => {
      const d = document.createElement('div');
      if (it.key === 'sep') { d.className = 'nav-sep'; return d; }
      d.className = 'nav-item';
      d.style.height = '30px';
      d.innerHTML = svg(it.ic) + `<span>${it.n}</span>`;
      if (onPick) { d.style.cursor = 'pointer'; d.addEventListener('click', () => onPick(it.key)); }
      return d;
    }, (el, it) => {
      if (it.key === 'sep') return;
      const on = it.key === sel;
      el.classList.toggle('on', on);
      if (on) el.dataset.spec = '设置侧栏 · 模块分节随开关出现'; else delete el.dataset.spec;
    }, animate);
  }
  function mountOptional(rows, store, toastHost, presetBtn) {
    rows.innerHTML = GROUPS.map(g => `<div class="group-label" style="margin:16px 4px 6px">${g}</div><div class="row-list">${MODS.filter(m => m.group === g).map(m => `<div class="row" data-m="${m.id}" style="min-height:54px;cursor:pointer"${m.id === 'danmaku' ? ' data-spec="分组列表行 · 最小高 54 · 开关"' : ''}><div class="mi">${svg(m.icon)}</div><div class="tx"><div class="tt">${m.name}</div><div class="ds" style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${m.desc}${m.deps.length ? ` · 需要${m.deps.map(d => MOD[d].name).join('、')}` : ''}</div></div><span class="tg"><i></i></span></div>`).join('')}</div>`).join('');
    $$('.row', rows).forEach(r => r.addEventListener('click', () => {
      const res = store.toggle(r.dataset.m);
      depToast(toastHost, res);
      [...res.added, ...res.removed].forEach(x => { const rr = $(`.row[data-m="${x}"]`, rows); rr && wobble(rr); });
    }));
    const order = ['pure', 'anime', 'all'];
    if (presetBtn) presetBtn.addEventListener('click', () => {
      const cur = presetOf(store.mods);
      const next = order[(order.indexOf(cur) + 1) % order.length];
      store.set(PRESETS[next].mods, { src: 'preset' });
      toast(toastHost, '已套用预设：' + PRESETS[next].name);
    });
    const sync = () => {
      $$('.row', rows).forEach(r => $('.tg', r).classList.toggle('on', store.has(r.dataset.m)));
      if (presetBtn) {
        const p = presetOf(store.mods);
        presetBtn.innerHTML = `预设：${p === 'custom' ? '自定义' : PRESETS[p].name}${svg('chev-d', 'i', 'width:14px')}`;
      }
    };
    store.on(sync); sync();
    autoJelly(rows);
  }

  /* ------------------------------------------------------------------
     插件市场
     ------------------------------------------------------------------ */
  const PLUGINS = [
    { id: 'spoiler', n: '防剧透', a: 'NipaPlay 官方', ic: 'eyeoff', d: '用你配置的 AI 接口识别并遮挡剧透弹幕', p: ['网络', '修改弹幕'], req: '需要弹幕模块', cat: '弹幕' },
    { id: 'regex', n: '正则弹幕过滤', a: 'NipaPlay 官方', ic: 'filter', d: '用正则表达式屏蔽弹幕', p: ['修改弹幕'], inst: true, cat: '弹幕' },
    { id: 'words', n: '敏感词屏蔽', a: 'NipaPlay 官方', ic: 'shield', d: '内置中文敏感词词库，一键屏蔽', p: ['修改弹幕'], inst: true, cat: '弹幕' },
    { id: 'crt', n: '复古 CRT', a: '社区 · shaderlab', ic: 'tv', d: '扫描线与荧光屏质感的着色器', p: ['着色器'], cat: '画面' },
    { id: 'clip', n: '片段导出', a: 'NipaPlay 官方', ic: 'scissors', d: '把精彩片段导出为 GIF 或 MP4', p: ['媒体导出', '写入文件'], cat: '工具' },
    { id: 'ext', n: '外部播放器', a: '社区 · ext-player', ic: 'external', d: '用 mpv、PotPlayer 或 IINA 打开当前视频', p: ['启动程序'], cat: '工具' },
    { id: 'sfx', n: '界面音效包', a: 'NipaPlay 官方', ic: 'music', d: '给按钮和转场配上轻快的音效', p: [], inst: true, cat: '外观' },
    { id: 'theme', n: '主题与壁纸', a: '社区 · mochi', ic: 'palette', d: '自定义背景图与配色方案', p: ['外观'], cat: '外观' },
    { id: 'titan', n: 'Titan 弹幕', a: '社区 · titan', ic: 'danmaku', d: '用 WebView 渲染的高级弹幕特效', p: ['WebView 叠加'], cat: '弹幕' },
  ];
  const installed = new Set(PLUGINS.filter(p => p.inst).map(p => p.id));
  const marketHooks = [];
  function marketCard(x, i) {
    const done = installed.has(x.id);
    return `<div class="row-list mk-card" data-cat="${x.cat}" style="padding:14px 16px;display:flex;flex-direction:column;gap:8px;min-height:140px">
      <div style="display:flex;gap:12px;align-items:center"><div class="mi" style="width:44px;height:44px;border-radius:12px">${svg(x.ic, 'i', 'width:22px;height:22px')}</div><div style="flex:1;min-width:0"><div class="tt" style="font-weight:700;font-size:14.5px">${x.n}</div><div class="cap2">${x.a}</div></div>
      <span class="b get"${i === 0 ? ' data-spec="获取 · 胶囊按钮 M · 灰底 + 下划标注"' : ''} data-pid="${x.id}" data-state="${done ? 'done' : 'get'}"><span>${done ? '打开' : '获取'}</span></span></div>
      <div class="body" style="font-size:13px">${x.d}</div>
      <div style="display:flex;gap:6px;flex-wrap:wrap;margin-top:auto">${x.p.map(t => `<span class="chip" style="height:22px;font-size:11.5px;padding:0 9px">${t}</span>`).join('')}${x.req ? `<span class="chip" style="height:22px;font-size:11.5px;padding:0 9px;background:transparent;border:1px dashed var(--line-2)">${x.req}</span>` : ''}</div></div>`;
  }
  function renderMarket(host, filter = null) {
    if (!host) return;
    host.innerHTML = PLUGINS.filter(x => !filter || x.cat === filter).map(marketCard).join('');
    $$('.b.get', host).forEach(b => b.addEventListener('click', () => {
      if (b.dataset.state !== 'get') return;
      b.dataset.state = 'loading';
      b.innerHTML = '<svg viewBox="0 0 20 20" style="width:18px;height:18px;transform:rotate(-90deg)"><circle cx="10" cy="10" r="8" fill="none" stroke="var(--wf-block-2)" stroke-width="2.5"/><circle class="ring" cx="10" cy="10" r="8" fill="none" stroke="var(--accent)" stroke-width="2.5" stroke-linecap="round" stroke-dasharray="50.27" stroke-dashoffset="50.27"/></svg>';
      const ring = $('.ring', b), t0 = performance.now();
      const step = now => {
        const p = Math.min(1, (now - t0) / 1100);
        ring.setAttribute('stroke-dashoffset', String(50.27 * (1 - p)));
        if (p < 1) requestAnimationFrame(step);
        else {
          b.dataset.state = 'done'; b.innerHTML = '<span>打开</span>';
          installed.add(b.dataset.pid);
          scaleTo(b, 0.8, 0.8, TOKENS.press); setTimeout(() => scaleTo(b, 1, 1, TOKENS.jelly), 60);
          marketHooks.forEach(f => f(b.dataset.pid));
        }
      };
      requestAnimationFrame(step);
    }));
    autoJelly(host);
  }

  /* ------------------------------------------------------------------
     首次启动配置（引导）
     ------------------------------------------------------------------ */
  const OB_HTML = `<div class="ob">
<div class="ob-stage">
<!-- step 0 欢迎 -->
<div class="ob-step on" data-step="0" style="padding:0">
<div class="plaid-academy" style="height:250px;position:relative">
<img src="assets/logo.png" alt="" style="position:absolute;left:44px;top:58px;height:120px;filter:drop-shadow(0 8px 18px rgba(0,0,0,.35))">
</div>
<img src="assets/girl.png" alt="" style="position:absolute;right:52px;top:70px;width:270px">
<div style="padding:30px 48px 0;max-width:560px">
<h5>欢迎使用 NipaPlay</h5>
<p class="sub">接下来一分钟，选出你需要的功能。没选的功能不会显示，也不会占用资源。之后随时可以在 设置 › 可选功能 里修改。</p>
<div class="meta"><span class="chip">约 1 分钟</span><span class="chip">随时可改</span><span class="chip">没开的功能不占资源</span></div>
</div>
</div>
<!-- step 1 预设 -->
<div class="ob-step" data-step="1">
<h5>你打算怎么用 NipaPlay？</h5>
<p class="sub">选一个起点，下一步还可以逐项调整。</p>
<div class="presets ob-presets"></div>
</div>
<!-- step 2 微调 -->
<div class="ob-step" data-step="2" style="padding-top:22px">
<div class="mod-split">
<div>
<h5 style="font-size:22px;margin-bottom:2px">挑选功能</h5>
<div class="mod-list ob-mods"></div>
</div>
<div class="prev-col" style="padding-top:8px">
<div class="prev-box"><div class="frame-wrap" style="margin:0"><div class="frame ob-prev" data-w="456" data-h="284"></div></div></div>
<div class="hud-strip ob-hud"></div>
<div class="prev-cap"><span>实时预览　上：主窗口　下：播放器控制条</span><span class="ob-count"></span></div>
</div>
</div>
</div>
<!-- step 3 外观 -->
<div class="ob-step" data-step="3">
<h5>选一个主题色</h5>
<p class="sub">取自 NipaPlay 图标的三种纯色，只会出现在按钮和文字上。</p>
<div class="accent-pick ob-accent">
<div class="sw" data-set-accent="rose"><i style="background:#FF2E55"></i>绯红</div>
<div class="sw" data-set-accent="blue"><i style="background:#2A51D4"></i>群青</div>
<div class="sw" data-set-accent="mint"><i style="background:#07FF94"></i>荧绿</div>
</div>
<div class="t2" style="font-size:15px;margin-bottom:10px">外观</div>
<span class="seg ob-theme"><span data-set-theme="system">跟随系统</span><span data-set-theme="light">浅色</span><span data-set-theme="dark">深色</span></span>
<div style="position:absolute;right:48px;top:40px;width:330px;height:206px"><div class="frame-wrap" style="margin:0"><div class="frame ob-prev2" data-w="440" data-h="275"></div></div></div>
</div>
<!-- step 4 按需设置 -->
<div class="ob-step" data-step="4">
<h5>还差几件小事</h5>
<p class="sub">只列出你开启的模块需要的设置，都可以以后再做。</p>
<div class="setup-cards ob-setup"></div>
</div>
<!-- step 5 完成 -->
<div class="ob-step" data-step="5" style="padding:0">
<div class="plaid-skirt" style="height:120px"></div>
<div style="padding:30px 48px 0">
<h5>准备好了 ☆</h5>
<p class="sub">已开启的功能如下。想改的话：设置 › 可选功能。</p>
<div class="meta ob-done" style="margin-bottom:22px"></div>
<div class="body" style="max-width:520px">点「开始使用」后，这个窗口会直接变成主窗口（转场 T7）。</div>
</div>
</div>
</div>
<div class="ob-foot">
<span class="b plain ob-skip" data-spec="无边框按钮 · M"><span>跳过</span></span>
<span class="b ob-back" data-spec="次要按钮 · M 32" style="display:none">上一步</span>
<span class="grow"></span>
<div class="pdots ob-dots" data-spec="页码点 · 自绘"></div>
<span class="grow"></span>
<span class="b L p ob-next" data-spec="主要按钮 · L 42 · 每屏唯一">开始</span>
</div>
</div>`;
  function mountOnboarding(rootEl, store, opts = {}) {
    rootEl.innerHTML = OB_HTML;
    const q = s => rootEl.querySelector(s), qa = s => [...rootEl.querySelectorAll(s)];
    const steps = qa('.ob-step'), N = steps.length;
    const dots = q('.ob-dots');
    dots.innerHTML = steps.map(() => '<i></i>').join('');
    let cur = 0, preset = presetOf(store.mods), stepTw = null;

    // 预设卡片
    const pc = q('.ob-presets');
    pc.innerHTML = Object.entries(PRESETS).map(([k, p]) => `<div class="preset" data-p="${k}"><div class="pv"><div class="frame-wrap" style="margin:0"><div class="frame" data-w="440" data-h="275"></div></div></div><div class="nm">${p.name}</div><div class="ds">${p.desc}</div><div class="cnt">${p.mods ? (p.mods.length ? p.mods.length + ' 个模块' : '不开启任何模块') : '跟随当前选择'}</div><span class="ck">${svg('check')}</span></div>`).join('');
    const presetWins = {};
    $$('.preset', pc).forEach(el => {
      const k = el.dataset.p, mw = new MiniWin($('.frame', el));
      presetWins[k] = mw;
      mw.update(PRESETS[k].mods ? new Set(PRESETS[k].mods) : store.mods, false);
      el.addEventListener('click', () => {
        preset = k;
        if (PRESETS[k].mods) store.set(PRESETS[k].mods, { src: 'preset' });
        markPresets();
      });
    });
    function markPresets() { $$('.preset', pc).forEach(el => el.classList.toggle('on', el.dataset.p === preset)); }

    // 模块列表
    const ml = q('.ob-mods');
    ml.innerHTML = GROUPS.map(g => `<div class="group-label" style="margin:8px 4px 4px;font-size:11px">${g}</div><div class="row-list">${MODS.filter(m => m.group === g).map(m => `<div class="row" data-m="${m.id}" style="min-height:44px;padding:4px 12px"><div class="mi" style="width:28px;height:28px;border-radius:8px">${svg(m.icon, 'i', 'width:16px;height:16px')}</div><div class="tx"><div class="tt" style="font-size:13.5px">${m.name}</div><div class="ds" style="font-size:11.5px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${m.short}</div></div><span class="tg"><i></i></span></div>`).join('')}</div>`).join('');
    $$('.row', ml).forEach(r => r.addEventListener('click', () => {
      const res = store.toggle(r.dataset.m);
      depToast(rootEl, res);
      [...res.added, ...res.removed].forEach(x => { const rr = $(`.row[data-m="${x}"]`, ml); rr && wobble(rr); });
    }));

    const mw = new MiniWin(q('.ob-prev'));
    const mw2 = new MiniWin(q('.ob-prev2'));
    const hud = new HudStrip(q('.ob-hud'));

    function sync(mods, info, animate = true) {
      $$('.row', ml).forEach(r => $('.tg', r).classList.toggle('on', mods.has(r.dataset.m)));
      if (!(info.src === 'preset' && PRESETS[preset].mods)) preset = presetOf(mods);
      markPresets();
      mw.update(mods, animate);
      hud.update(mods, animate);
      presetWins.custom && presetWins.custom.update(mods, animate);
      const n = mods.size;
      q('.ob-count').textContent = n ? `${n} 个模块 · ${pagesFor(mods).length} 个页面` : '纯粹播放 · 0 个页面';
    }
    store.on((m, info) => sync(m, info, true));
    sync(store.mods, {}, false);

    function renderSetup() {
      const m = store.mods, cards = [];
      const ico = (src, w = 16) => `<img src="${src}" alt="" class="${/dandanplay|bangumi/.test(src) ? 'mono-logo' : ''}" style="width:${w}px;height:${w}px;border-radius:4px">`;
      if (m.has('library')) cards.push({ icon: 'folder', t: '添加媒体文件夹', d: 'NipaPlay 会扫描文件夹，自动识别番剧和封面。', a: '<span class="b S">选择文件夹…</span><span class="b plain S"><span>稍后</span></span>' });
      if (m.has('sync')) cards.push({ icon: 'user', t: '登录账号', d: '同步观看进度和评分，登录后还能发弹幕。', a: `<span class="b S">${ico('assets/dandanplay.png')}弹弹play</span><span class="b S">${ico('assets/bangumi.png')}Bangumi</span>` });
      if (m.has('sources')) cards.push({ icon: 'server', t: '连接媒体服务器', d: 'Jellyfin、Emby、WebDAV 或 SMB。', a: `<span class="b S">${ico('assets/jellyfin.svg', 14)}Jellyfin</span><span class="b S">${ico('assets/emby.svg', 14)}Emby</span><span class="b S">更多…</span>` });
      if (m.has('remote')) cards.push({ icon: 'wifi', t: '局域网共享', d: '允许同一网络里的设备浏览你的媒体库，默认关闭。', a: '<span class="tg"><i></i></span>' });
      if (m.has('danmaku') && !m.has('sync')) cards.push({ icon: 'danmaku', t: '弹幕已就绪', d: '播放时自动匹配弹弹play 的弹幕，不需要登录。', a: '' });
      if (m.has('downloads')) cards.push({ icon: 'download', t: '下载位置', d: '默认保存到系统的「下载」文件夹。', a: '<span class="b S">更改…</span>' });
      const host = q('.ob-setup');
      host.innerHTML = cards.length
        ? cards.map(c => `<div class="setup-card"><div class="mi">${svg(c.icon)}</div><div class="t2">${c.t}</div><div class="cap2">${c.d}</div><div class="acts">${c.a}</div></div>`).join('')
        : `<div style="grid-column:1/-1;display:flex;gap:28px;align-items:center;padding:10px 0"><img src="assets/girl.png" alt="" style="width:160px"><div><div class="t2" style="font-size:19px;margin-bottom:6px">纯粹模式不需要额外设置</div><div class="body">打开视频就能看。以后想要更多功能，去 设置 › 可选功能。</div></div></div>`;
      autoJelly(host);
      $$('.tg', host).forEach(t => t.addEventListener('click', () => t.classList.toggle('on')));
      if (!RM()) $$('.setup-card', host).forEach((c, i) => {
        c.style.opacity = '0';
        setTimeout(() => tween(TOKENS.smooth, p => { c.style.opacity = String(Math.min(1, p)); c.style.transform = `translateY(${(1 - p) * 14}px)`; }, () => { c.style.transform = ''; }), i * 40);
      });
    }
    function renderDone() {
      const m = store.mods;
      const chips = [`<span class="chip">${svg('film', 'i', 'width:14px;height:14px')}核心播放</span>`]
        .concat(m.size ? MODS.filter(x => m.has(x.id)).map(x => `<span class="chip acc-chip">${svg(x.icon, 'i', 'width:14px;height:14px')}${x.name}</span>`) : ['<span class="chip">纯粹播放 · 不开启模块</span>']);
      q('.ob-done').innerHTML = chips.join('');
      if (!RM()) $$('.chip', q('.ob-done')).forEach((c, i) => { c.style.transform = 'scale(.4)'; c.style.opacity = '0'; setTimeout(() => enter(c, null), 120 + i * 45); });
    }

    function go(n, animate = true) {
      if (n < 0 || n >= N || n === cur) return;
      const from = steps[cur], to = steps[n], dir = n > cur ? 1 : -1;
      cancel(stepTw);
      steps.forEach(s => { if (s !== from && s !== to) { s.classList.remove('on'); s.style.transform = s.style.opacity = ''; } });
      if (n === 3) mw2.update(store.mods, false);
      if (n === 4) renderSetup();
      if (n === 5) renderDone();
      to.classList.add('on'); queueFit();
      cur = n; updFoot();
      if (RM() || !animate) { from.classList.remove('on'); return; }
      stepTw = tween(TOKENS.smooth, p => {
        const qn = Math.min(1, p);
        from.style.transform = `translateX(${-dir * 60 * qn}px)`; from.style.opacity = String(1 - qn);
        to.style.transform = `translateX(${dir * 60 * (1 - p)}px)`; to.style.opacity = String(Math.min(1, qn * 1.2));
      }, () => {
        if (from !== steps[cur]) from.classList.remove('on');
        from.style.transform = from.style.opacity = ''; to.style.transform = to.style.opacity = '';
      });
    }
    const next = q('.ob-next'), back = q('.ob-back'), skip = q('.ob-skip');
    function updFoot() {
      back.style.display = cur > 0 && cur < 5 ? '' : 'none';
      skip.style.display = cur === 0 ? '' : 'none';
      next.textContent = ['开始', '继续', '继续', '继续', '继续', '开始使用'][cur];
      $$('i', dots).forEach((d, i) => d.classList.toggle('on', i === cur));
    }
    next.addEventListener('click', () => { if (cur < N - 1) go(cur + 1); else finish(); });
    back.addEventListener('click', () => go(cur - 1));
    skip.addEventListener('click', () => { preset = 'pure'; store.set([], { src: 'preset' }); go(5); });

    function reset() {
      cancel(stepTw);
      steps.forEach(s => { s.classList.remove('on'); s.style.transform = s.style.opacity = ''; });
      steps[0].classList.add('on'); cur = 0; updFoot();
      preset = presetOf(store.mods); markPresets();
    }

    // T7：完成 → 主窗口（文档里演示变形；原型里交给调用方）
    let morph = null, morphWin = null, again = null;
    function finish() {
      if (opts.onFinish) { opts.onFinish(store.mods); return; }
      if (!morph) {
        morph = document.createElement('div');
        morph.style.cssText = 'position:absolute;inset:0;z-index:25;display:none';
        morph.innerHTML = '<div class="frame" data-w="440" data-h="275" style="position:absolute;left:0;top:0;width:440px;height:275px;transform-origin:0 0"></div><span class="b" style="position:absolute;left:50%;bottom:24px;transform:translateX(-50%);display:none">重新体验引导</span>';
        rootEl.appendChild(morph);
        morphWin = new MiniWin($('.frame', morph));
        again = $('.b', morph);
        again.addEventListener('click', () => { morph.style.display = 'none'; morph.style.background = ''; reset(); });
        bindJelly(again, 'M');
      }
      morphWin.update(store.mods, false);
      morph.style.display = ''; again.style.display = 'none';
      const W = rootEl.clientWidth || 960, H = rootEl.clientHeight || 600, s1 = W / 440, s0 = 0.42;
      const f = $('.frame', morph);
      tween(TOKENS.hero, p => {
        const s = s0 + (s1 - s0) * p;
        f.style.transform = `translate(${(W - 440 * s) / 2}px, ${(H - 275 * s) / 2}px) scale(${s})`;
        f.style.opacity = String(Math.min(1, p * 2.2));
      }, () => { morph.style.background = 'var(--wf-bg)'; again.style.display = ''; enter(again, null); });
    }
    updFoot();
    markPresets();
    syncThemeUI();
    autoJelly(rootEl);
    watchFrames(rootEl);
    return { reset, go };
  }

  return {
    $, $$, svg, kv, root,
    TOKENS, DEFAULTS, RM, setForceRM: v => { forceRM = v; }, getForceRM: () => forceRM,
    Spring, run, stop, tween, cancel, scaleTo, bindJelly, wobble, autoJelly,
    enter, leave, setShown, reconcile, toast,
    setTheme, setAccent, applyTheme, syncThemeUI, onTheme: f => themeHooks.push(f), themePref: () => themePref,
    fit, queueFit, watchFrames, scaleOf,
    MODS, MOD, ALL, GROUPS, PRESETS, createStore, presetOf, depToast, pagesFor,
    MiniWin, HudStrip,
    SET_TOP, SET_MOD, SET_BOT, settingsSections, renderSetSide, mountOptional,
    PLUGINS, installed, renderMarket, onInstall: f => marketHooks.push(f),
    mountOnboarding,
  };
})();
