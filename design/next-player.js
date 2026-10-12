/* =====================================================================
   Erika · 画框版 —— 播放器（沿用 player-frame.html 的画框布局）
   从被点的缩略图里长出来；控制层出现时画面退后进画框；返回时缩进迷你播放器
   ===================================================================== */
(() => {
  'use strict';
  const { $, $$, svg, TOKENS, Spring, run, stop, tween, cancel, bindJelly, RM } = NP;
  const { App, I, byId, imgOf, STILL, LINES, SUBS, DUR, CH, PEAKS, clamp, fmt, rateLabel } = NX;
  const A = App.prototype;
  const EPS = 12;
  /* 每种设备一套画框位置：full = 看片时铺满，shown = 控制层出现时退后，panel = 设置面板把画面推到一边 */
  /* 折叠屏：duoc = iPhone Duo 合上转横屏（662×470），duoo / xo = 展开（iPhone Duo 930×662 / Mate X 740×808），
     duot / xt = 悬停：画面在上半屏一直铺满，控件全在下半屏（seek 是进度行的固定位置），所以控件出现时画面不用退后 */
  const BASE = {
    desk: { W: 1280, H: 800, w: 1280, h: 720 }, tablet: { W: 1180, H: 820, w: 1180, h: 663.75 }, tv: { W: 1280, H: 720, w: 1280, h: 720 }, phone: { W: 844, H: 390, w: 693, h: 390 },
    duoc: { W: 662, H: 470, w: 662, h: 372.375 }, duoo: { W: 930, H: 662, w: 930, h: 523.125 }, xo: { W: 740, H: 808, w: 740, h: 416.25 },
    duot: { W: 662, H: 930, w: 662, h: 372.375, seek: { x: 24, y: 565, w: 614 } }, xt: { W: 808, H: 740, w: 808, h: 454.5, seek: { x: 24, y: 470, w: 760 } },
  };
  const RECT = {
    desk: { full: { x: 0, y: 40, w: 1280 }, shown: { x: 103, y: 64, w: 1074 }, panel: { x: 24, y: 114, w: 896 } },
    tablet: { full: { x: 0, y: 78, w: 1180 }, shown: { x: 60, y: 76, w: 1060 }, panel: { x: 24, y: 120, w: 788 } },
    tv: { full: { x: 0, y: 0, w: 1280 }, shown: { x: 200, y: 52, w: 880 }, panel: { x: 40, y: 100, w: 784 } },
    phone: { full: { x: 75.5, y: 0, w: 693 }, shown: { x: 162.5, y: 48, w: 519 }, panel: { x: 16, y: 56, w: 512 } },
    duoc: { full: { x: 0, y: 48.8, w: 662 }, shown: { x: 116, y: 80, w: 430 }, panel: { x: 16, y: 64, w: 340 } },
    duoo: { full: { x: 0, y: 69.4, w: 930 }, shown: { x: 125, y: 56, w: 680 }, panel: { x: 24, y: 76, w: 560 } },
    xo: { full: { x: 0, y: 195.9, w: 740 }, shown: { x: 24, y: 120, w: 692 }, panel: { x: 24, y: 80, w: 380 } },
    duot: { full: { x: 0, y: 46.3, w: 662 }, shown: { x: 0, y: 46.3, w: 662 }, panel: { x: 0, y: 46.3, w: 662 } },
    xt: { full: { x: 75.1, y: 0, w: 657.8 }, shown: { x: 75.1, y: 0, w: 657.8 }, panel: { x: 75.1, y: 0, w: 657.8 } },
  };
  const CTL = {
    desk: { shown: { x: 103, w: 1074 }, panel: { x: 24, w: 1232 } }, tablet: { shown: { x: 60, w: 1060 }, panel: { x: 24, w: 1132 } }, tv: { shown: { x: 64, w: 1152 }, panel: { x: 40, w: 1200 } }, phone: { shown: { x: 0, w: 0 }, panel: { x: 0, w: 0 } },
    duoc: { shown: { x: 0, w: 0 }, panel: { x: 0, w: 0 } }, duoo: { shown: { x: 125, w: 680 }, panel: { x: 24, w: 882 } }, xo: { shown: { x: 24, w: 692 }, panel: { x: 24, w: 692 } },
    duot: { shown: { x: 24, w: 614 }, panel: { x: 24, w: 614 } }, xt: { shown: { x: 24, w: 760 }, panel: { x: 24, w: 760 } },
  };
  /* RAILS：两侧竖排控件（手机横屏那一套）；TENT：悬停 */
  const RAILS = new Set(['phone', 'duoc']), TENT = new Set(['duot', 'xt']);
  const chAt = t => CH.filter(c => t >= c.t).pop();

  A.plNew = function (o) {
    const a = o.aid != null ? byId(o.aid) : null;
    const p = { aid: a ? a.id : null, ep: o.ep || 1, title: o.title || (a ? a.t : '未命名视频'), img: o.img || (a ? imgOf(a) : STILL),
      dur: a && a.movie ? 6420 : DUR, t: 0, playing: true, dmOn: true, rate: 1, sub: 0, audio: 0, enh: 1, mode: 'full', lay: 'full', auto: true, open: false, video: null };
    p.sub2 = a ? (a.movie ? '剧场版' : `第 ${p.ep} 集`) : o.file ? '本地文件' : '最近播放';
    p.t = o.prog != null ? o.prog * p.dur : a && a.w ? 0.35 * p.dur : 0;
    if (o.file) {
      const v = document.createElement('video'); v.className = 'vid'; v.playsInline = true; v.src = URL.createObjectURL(o.file);
      v.addEventListener('loadedmetadata', () => { if (isFinite(v.duration)) p.dur = v.duration; p.t = 0; v.play().catch(() => {}); });
      v.addEventListener('error', () => { p.video = null; v.remove(); this.toast('浏览器解不了这个格式，原型改用示意画面'); });
      p.video = v; p.t = 0;
    }
    return p;
  };
  A.plFF = function () {
    if (this.dev !== 'fold') return this.dev;
    if (this.fold === 'closed') return this.hw === 'duo' ? 'duoc' : 'phone';
    return (this.hw === 'duo' ? 'duo' : 'x') + (this.fold === 'open' ? 'o' : 't');
  };
  /* 画框某个状态的矩形（纯粹模式的空画框也用 shown，和播放器退后时完全重合） */
  A.plRect = function (kind) { const ff = this.plFF(), R = RECT[ff][kind], b = BASE[ff]; return { x: R.x, y: R.y, w: R.w, h: R.w * b.h / b.w }; };

  A.plHTML = function () {
    const p = this.P, ff = this.plFF(), ph = RAILS.has(ff), tv = ff === 'tv', b = BASE[ff], s = this.store;
    /* 发弹幕：桌面和平板是控制行里的输入框；手机和电视是一个按钮，点了在进度条的位置换出输入条（电视上用遥控器语音或手机输入） */
    const inline = ff === 'desk' || ff === 'tablet';
    const dm = s.has('danmaku'), dmi = dm && s.has('sync'), enh = s.has('enhance'), eps = p.aid != null && !byId(p.aid).movie;
    const tk = CH.slice(1).map(c => `<i class="tk" style="left:${(c.t / DUR) * 100}%"></i>`).join('');
    /* 手机进度条下面放不下章节名：弹幕高峰改成轨道上的主题色小点，章节名在拖动提示里显示 */
    const pkm = ph && dm && p.aid != null ? PEAKS.map(t => `<i class="pkm" style="left:${(t / DUR) * 100}%"></i>`).join('') : '';
    const slider = `<div class="c-slider seek" style="--v:${p.t / p.dur}"><i class="in"></i><i class="ac"></i>${p.aid != null ? tk : ''}${pkm}<i class="th"></i><span class="tip"></span></div>`;
    const split = dm ? `<span class="c-split${p.dmOn ? ' on' : ''}" data-pa="dmsplit"><span class="sa" data-pa="dm">${I(p.dmOn ? 'danmaku' : 'danmaku-off')}弹幕</span><span class="sb" data-pa="dmq" data-menu-anchor>${I('chev-d')}</span></span>` : '';
    const ddSub = `<span class="c-btn" data-pa="sub" data-menu-anchor>${I('cc')}<span class="lab-sub">${['简中', '日本語', '关闭'][p.sub]}</span><span class="chev">${I('chev-d')}</span></span>`;
    const ddRate = `<span class="c-btn" data-pa="rate" data-menu-anchor><span class="lab-rate">${rateLabel(p.rate)}</span><span class="chev">${I('chev-d')}</span></span>`;
    const ddEnh = enh ? `<span class="c-btn" data-pa="enh" data-menu-anchor>${I('sparkle')}<span class="lab-enh">${['关闭', 'Anime4K', 'ArtCNN'][p.enh]}</span><span class="chev">${I('chev-d')}</span></span>` : '';
    const play = `<span class="c-play" data-pa="play" title="播放 / 暂停（空格）">${I(p.playing ? 'pause' : 'play')}</span>`;
    const tr = (n, a, t) => `<span class="c-tr" data-pa="${a}" title="${t}">${I(n)}</span>`;
    const ib = (n, a, t, anc) => `<span class="c-ib" data-pa="${a}" title="${t}"${anc ? ' data-menu-anchor' : ''}>${I(n)}</span>`;
    const field = `<label class="c-field"${ph ? '' : ' style="width:230px"'}><input placeholder="发个友善的弹幕…" maxlength="40"><span class="c-send" data-pa="send">发送</span></label>`;
    const cv = `<div class="cv" style="width:${b.w}px;height:${b.h}px">${p.video ? '' : `<img class="vid" src="${p.img}" alt="">`}
        <div class="dm-layer"${dm && p.dmOn ? '' : ' style="display:none"'}>${LINES.map(([t, c], i) => { const d = 11 + ((i * 7) % 6); return `<span style="top:${(i % (ph ? 5 : 9)) * (ph ? 30 : 40)}px;animation-duration:${d}s;animation-delay:${-((i * 3.7) % d)}s${c ? ';color:' + c : ''}">${t}</span>`; }).join('')}</div>
        <div class="sub"><span>${SUBS[0]}</span></div><div class="fb"></div></div>`;
    const top = `<div class="p-top fx"><span class="p-lights"><i></i><i></i><i></i></span>${ib('chev-l', 'back', '返回（Esc）')}
        <div class="ttl"><b>${p.title}</b><span class="ep-lab">${p.sub2}</span></div><span class="grow"></span>
        ${ph && enh ? ib('sparkle', 'enh', '画质增强', 1) : ''}${ff === 'desk' ? ib('vol', 'vol', '音量', 1) : ''}${dm ? ib('link', 'match', '弹幕匹配') : ''}${ib('pip', 'pip', '画中画')}${ff === 'desk' ? ib('full', 'fs', '全屏') : ''}${ib('sliders', 'panel', '播放设置')}${ib('more', 'more', '更多', 1)}
        ${this.cfg.ds === 'fluent' && ff === 'desk' ? '<span style="display:flex;align-self:flex-start;margin:0 -20px 0 6px"><i style="width:46px;height:32px;display:grid;place-items:center;font-style:normal;font-size:13px;opacity:.85">—</i><i style="width:46px;height:32px;display:grid;place-items:center;font-style:normal;font-size:13px;opacity:.85">☐</i><i style="width:46px;height:32px;display:grid;place-items:center;font-style:normal;font-size:13px;opacity:.85">✕</i></span>' : ''}
        ${this.cfg.ds === 'adwaita' && ff === 'desk' ? '<span class="x-gclose" style="display:grid">✕</span>' : ''}
        ${this.cfg.ds === 'harmony' && ff === 'desk' ? '<span class="p-hcaps"><i>—</i><i>☐</i><i>✕</i></span>' : ''}</div>`;
    const seek = `<div class="p-seek fx"><span class="t now">${fmt(p.t)}</span>${slider}<span class="t dur">${fmt(p.dur)}</span>
        ${p.aid != null ? `<div class="chs">${CH.map(c => `<span style="left:${(c.t / DUR) * 100}%">${c.n}</span>`).join('')}${dm ? PEAKS.map(t => `<span class="pk" style="left:calc(${(t / DUR) * 100}% - 5px)" title="弹幕高峰">★</span>`).join('') : ''}</div>` : ''}</div>`;
    const body = ph
      ? `<div class="p-rail l fx hide-in-panel">${split}${dmi ? `<span class="c-btn" data-pa="say">${I('send')}发弹幕</span>` : ''}<div class="rrow">${eps ? tr('prev', 'prev', '上一集') : ''}${tr('rw10', 'rw', '快退 10 秒')}</div></div>
         <div class="p-rail r fx hide-in-panel">${ddSub}${ddRate}${play}<div class="rrow">${tr('ff10', 'ff', '快进 10 秒')}${eps ? tr('next', 'next', '下一集') : ''}</div></div>
         ${seek}${dmi ? `<div class="p-say fx">${field}${ib('x', 'unsay', '收起')}</div>` : ''}`
      : `${seek}${dmi && !inline ? `<div class="p-say fx">${field}${ib('x', 'unsay', '收起')}</div>` : ''}<div class="p-ctl fx"><div class="l">${split}${dmi ? (inline ? field : `<span class="c-btn" data-pa="say">${I('send')}发弹幕</span>`) : ''}</div><div class="c">${eps ? tr('prev', 'prev', '上一集') : ''}${tr('rw10', 'rw', '快退 10 秒')}${play}${tr('ff10', 'ff', '快进 10 秒')}${eps ? tr('next', 'next', '下一集') : ''}</div><div class="r">${ddSub}${ddRate}${ddEnh}</div></div>`;
    return cv + top + body + '<div class="p-sheet"></div>';
  };

  A.plBuild = function () {
    const p = this.P, ff = this.plFF(), q = this.q.player;
    q.className = 'x-player dk ff-' + ff + (RAILS.has(ff) ? ' ph' : '') + (TENT.has(ff) ? ' tent' : '');
    q.innerHTML = this.plHTML();
    q.style.display = 'block';
    p.cv = $('.cv', q); p.seek = $('.p-seek', q); p.ctl = $('.p-ctl', q); p.say = $('.p-say', q); p.sheet = $('.p-sheet', q);
    if (p.video) p.cv.insertBefore(p.video, p.cv.firstChild);
    const R = RECT[ff].full;
    p.sp = { x: new Spring(R.x), y: new Spring(R.y), w: new Spring(R.w), r: new Spring(0), cx: new Spring(CTL[ff].shown.x), cw: new Spring(CTL[ff].shown.w) };
    $$('.c-ib, .c-tr, .c-play', q).forEach(b => bindJelly(b, 'S'));
    $$('.c-btn, .c-split > span, .c-send', q).forEach(b => bindJelly(b, 'M'));
    $$('input', q).forEach(inp => inp.addEventListener('keydown', e => { e.stopPropagation(); if (e.key === 'Enter') { e.preventDefault(); this.plSend(inp); } if (e.key === 'Escape') this.plSay(false); }));
    this.plSync();
  };
  /* 画框动画：x / y / 宽 / 圆角走 hero 弹簧；进度行跟着画框边缘 */
  A.plApply = function () {
    const p = this.P; if (!p || !p.cv) return;
    const ff = this.plFF(), b = BASE[ff], s = p.sp, k = s.w.v / b.w, h = s.w.v * b.h / b.w;
    p.cv.style.transform = `translate(${s.x.v.toFixed(2)}px, ${s.y.v.toFixed(2)}px) scale(${k.toFixed(5)})`;
    p.cv.style.borderRadius = (s.r.v / k).toFixed(2) + 'px';
    p.cv.style.setProperty('--s', k.toFixed(4));
    /* 进度行贴着画框下沿；悬停时进度行在下半屏的固定位置 */
    const gap = RAILS.has(ff) ? 3 : 0, S = b.seek || { x: s.x.v, y: s.y.v + h + gap, w: s.w.v };
    p.seek.style.left = S.x + 'px'; p.seek.style.width = S.w + 'px'; p.seek.style.top = S.y + 'px';
    if (p.say) { p.say.style.left = S.x + 'px'; p.say.style.width = S.w + 'px'; p.say.style.top = S.y + 'px'; }
    if (p.ctl) { p.ctl.style.left = s.cx.v + 'px'; p.ctl.style.width = s.cw.v + 'px'; }
  };
  A.plLayout = function (kind, snap) {
    const p = this.P; if (!p || !p.sp) return;
    const ff = this.plFF();
    /* 悬停：控件在另一半屏，一直留着，画面也不用铺满 */
    if (TENT.has(ff) && kind === 'full') kind = 'shown';
    p.lay = kind;
    const R = RECT[ff][kind], C = CTL[ff][kind === 'panel' ? 'panel' : 'shown'];
    const r = kind === 'full' || TENT.has(ff) ? 0 : this.rad(RAILS.has(ff) ? '--r-l' : '--r-card');
    const q = this.q.player; q.classList.toggle('shown', kind !== 'full'); q.classList.toggle('panel', kind === 'panel');
    const s = p.sp, cfg = TOKENS.hero;
    s.x.cfg(cfg).target = R.x; s.y.cfg(cfg).target = R.y; s.w.cfg(cfg).target = R.w; s.r.cfg(cfg).target = r; s.cx.cfg(cfg).target = C.x; s.cw.cfg(cfg).target = C.w;
    const all = Object.values(s);
    if (snap || RM()) { all.forEach(x => x.snap()); this.plApply(); return; }
    if (!p.anim) p.anim = run({ sp: all, eps: 0.05, apply: () => this.plApply(), done: () => { p.anim = null; } });
  };

  /* ---------------- 打开 / 返回 ---------------- */
  A.plOpen = function (o, from, srcEl, fromIdle) {
    if (this.P && this.P.video) this.P.video.pause();
    this.menuClose();
    const startOpen = () => {
      this.P = this.plNew(o); const p = this.P; p.mode = 'full'; p.open = true;
      this.plBuild();
      const ff = this.plFF();
      if (from && !RAILS.has(ff) && !RM()) {
        /* 从缩略图（或空画框）的位置开始，长到画框位置 */
        const b = BASE[ff], k = from.w / b.w, hh = from.w * b.h / b.w;
        p.sp.x.v = from.x; p.sp.y.v = from.y + (from.h - hh) / 2; p.sp.w.v = from.w; p.sp.r.v = srcEl ? parseFloat(getComputedStyle(srcEl).borderTopLeftRadius) || 0 : this.rad('--r-card');
        Object.values(p.sp).forEach(s => { s.target = s.v; s.vel = 0; });
        p.sp.cx.v = p.sp.cx.target = CTL[ff].shown.x; p.sp.cw.v = p.sp.cw.target = CTL[ff].shown.w;
        this.plApply();
        if (srcEl) srcEl.style.visibility = 'hidden';
        setTimeout(() => { if (srcEl) srcEl.style.visibility = ''; }, 500);
        this.plLayout('shown');
      } else this.plLayout('shown', true);
      this.plWake();
      this.renderMini();
    };
    if (this.rot && !this.land) { this.setLand(true, startOpen); return; }
    if (fromIdle && !RM()) {
      /* 空画框先被画面填满，再把控件放到四周——画框位置完全一样，所以没有跳动 */
      const fill = $('.i-frame img.fill', this.el); if (fill) { fill.src = o.img || STILL; tween(TOKENS.smooth, q => { fill.style.opacity = String(Math.min(1, q)); }, () => { startOpen(); fill.style.opacity = ''; }); return; }
    }
    startOpen();
  };
  A.setLand = function (on, done) {
    this.land = on; this.el.classList.toggle('land', on);
    $$('.x-status, .x-pbody, .c-tabbar, .x-pmini', this.el).forEach(e => (e.style.display = on ? 'none' : ''));
    this.cfg.onLand && this.cfg.onLand(on);
    if (!on) { this.q.player.style.display = 'none'; this.renderPhone(false); }
    setTimeout(() => done && done(), on ? 60 : 0);
  };
  A.plBack = function () {
    const p = this.P; if (!p) return;
    this.menuClose(); this.plSay(false);
    clearTimeout(p.hideT); p.open = false; p.mode = 'mini';
    if (this.rot) { this.setLand(false); this.renderMini(); return; }
    if (this.ph) { this.q.player.style.display = 'none'; this.renderMini(); return; }
    if (this.pure()) {
      /* 纯粹模式没有侧栏：画面退回空画框，这个视频排到「最近播放」第一个 */
      if (p.video) p.video.pause();
      this.q.player.style.display = 'none'; this.P = null;
      NX.RECENTS.unshift({ t: p.title, prog: p.t / p.dur, left: fmt(p.dur - p.t), img: p.img }); NX.RECENTS.splice(3);
      this.renderIdle(false);
      const fill = $('.i-frame img.fill', this.el); if (fill && !RM()) { fill.src = p.img; fill.style.opacity = '1'; tween(TOKENS.smooth, q => { fill.style.opacity = String(1 - Math.min(1, q)); }, () => { fill.style.opacity = ''; }); }
      this.stagger($$('.i-rc', this.el), 120, 8, 40);
      return;
    }
    const from = this.rel(p.cv);
    this.q.player.style.display = 'none';
    this.renderMini();
    const th = $('.x-mini .x-thumb', this.el);
    if (th && !RM()) { th.style.visibility = 'hidden'; this.fly(p.img, from, this.rel(th), this.rad('--r-card'), 0, () => { th.style.visibility = ''; }, TOKENS.hero); NP.enter(this.q.mini.firstElementChild, 'h'); }
  };
  A.plExpand = function () {
    const p = this.P; if (!p) return;
    if (this.rot) { this.setLand(true, () => { p.open = true; p.mode = 'full'; this.plBuild(); this.plLayout('shown', true); this.plWake(); this.renderMini(); }); return; }
    const th = $('.x-mini .x-thumb, .x-pmini .x-thumb', this.el), from = th ? this.rel(th) : null;
    p.open = true; p.mode = 'full'; this.plBuild();
    if (from && !RM()) {
      const ff = this.plFF(); p.sp.x.v = from.x; p.sp.y.v = from.y; p.sp.w.v = from.w; p.sp.r.v = 0;
      Object.values(p.sp).forEach(s => { s.target = s.v; s.vel = 0; }); p.sp.cx.v = p.sp.cx.target = CTL[ff].shown.x; p.sp.cw.v = p.sp.cw.target = CTL[ff].shown.w;
      this.plApply();
    }
    this.renderMini();
    this.plLayout('shown', !from); this.plWake();
  };

  /* ---------------- 自动隐藏：动鼠标就退后，3 秒不动铺满 ---------------- */
  A.plWake = function (e) {
    const p = this.P; if (!p || !p.open) return;
    if (e && this.touch) return;
    if (p.lay === 'full') this.plLayout('shown');
    clearTimeout(p.hideT);
    p.hideT = setTimeout(() => {
      const ae = document.activeElement, typing = ae && ae.tagName === 'INPUT' && this.q.player.contains(ae);
      if (p.open && p.playing && !this.menuKind && !p.drag && !p.saying && p.lay !== 'panel' && !typing) this.plLayout('full');
    }, 3000);
  };

  /* ---------------- 播放状态 ---------------- */
  A.plSync = function () {
    const p = this.P, q = this.q.player; if (!p) return;
    const s = $('.c-slider.seek', q); if (s && !p.drag) s.style.setProperty('--v', clamp(p.t / p.dur, 0, 1));
    const n = $('.t.now', q); if (n) n.textContent = fmt(p.t);
    const d = $('.t.dur', q); if (d) d.textContent = fmt(p.dur);
    const sub = $('.sub span', q), line = SUBS[Math.floor(p.t / 4) % SUBS.length]; if (sub && sub.textContent !== line) sub.textContent = line;
    $$('.mp-b', this.el).forEach(b => (b.style.width = (p.t / p.dur) * 100 + '%'));
    $$('.mp-t', this.el).forEach(b => (b.textContent = fmt(p.t)));
  };
  A.plTick = function () {
    const p = this.P; if (!p) return;
    if (p.video && isFinite(p.video.currentTime)) p.t = p.video.currentTime;
    else if (p.playing && !p.drag) p.t = Math.min(p.dur, p.t + 0.25 * p.rate);
    this.plSync();
  };
  A.plPlay = function (on, fb) {
    const p = this.P; p.playing = on;
    if (p.video) { if (on) p.video.play().catch(() => {}); else p.video.pause(); }
    this.q.player.classList.toggle('paused', !on);
    $$('[data-pa="play"]', this.q.player).forEach(b => (b.innerHTML = I(on ? 'pause' : 'play')));
    if (fb) {
      const c = $('.fb', this.q.player); c.innerHTML = I(on ? 'play' : 'pause'); cancel(c._tw); clearTimeout(c._h);
      c._tw = tween(TOKENS.jelly, q => { c.style.transform = `scale(${0.6 + 0.4 * q})`; c.style.opacity = String(Math.min(1, q * 2)); });
      c._h = setTimeout(() => { c._tw = tween(TOKENS.smooth, q => { c.style.opacity = String(Math.max(0, 1 - q)); }); }, 420);
    }
    this.plWake();
  };
  A.plSeek = function (t) { const p = this.P; p.t = clamp(t, 0, p.dur); if (p.video) p.video.currentTime = p.t; this.plSync(); };
  A.plDm = function () {
    const p = this.P; p.dmOn = !p.dmOn;
    $$('.c-split', this.q.player).forEach(x => { x.classList.toggle('on', p.dmOn); $('.sa', x).innerHTML = I(p.dmOn ? 'danmaku' : 'danmaku-off') + '弹幕'; });
    const L = $('.dm-layer', this.q.player); if (L) L.style.display = p.dmOn ? '' : 'none';
  };
  A.plSay = function (on) {
    const p = this.P; if (!p) return; p.saying = on; this.q.player.classList.toggle('saying', on);
    const inp = p.say ? $('input', p.say) : null; if (on && inp) setTimeout(() => inp.focus({ preventScroll: true }), 150); else if (inp) inp.blur();
  };
  A.plSend = function (inp) {
    const p = this.P;
    if (!inp.value.trim()) { inp.focus({ preventScroll: true }); return; }
    const L = $('.dm-layer', this.q.player), s = document.createElement('span');
    s.textContent = inp.value.trim();
    s.style.cssText = `top:${Math.floor(Math.random() * 4) * (RAILS.has(this.plFF()) ? 30 : 40)}px;animation-duration:9s;animation-iteration-count:1;animation-delay:0s;outline:1.5px solid rgba(255,255,255,.85);outline-offset:3px;border-radius:4px`;
    L.appendChild(s); setTimeout(() => s.remove(), 9200); inp.value = '';
    if (!p.dmOn) this.plDm();
    if (!(this.dev === 'desk' || this.dev === 'tablet')) this.plSay(false);
  };
  A.plEp = function (n) {
    const p = this.P; p.ep = n; p.sub2 = `第 ${n} 集`; p.t = 0;
    $$('.ep-lab', this.q.player).forEach(x => (x.textContent = p.sub2));
    this.plSync(); this.toast(`第 ${n} 集`);
  };

  /* ---------------- 菜单 ---------------- */
  const mi = (t, on, d, k = '') => `<div class="c-mi${on ? ' on' : ''}" data-mi="${d}"><span class="ck">${I('check')}</span>${t}${k ? `<span class="k">${k}</span>` : ''}</div>`;
  A.plMenu = function (kind, anchor) {
    const p = this.P, ph = this.rot;
    const sl = (v, key) => `<div class="c-slider sm" data-k="${key}" style="--v:${v}"><i class="in"></i><i class="ac"></i><i class="th"></i></div>`;
    const H = {
      sub: () => `<div class="c-mlabel">字幕</div>${['简体中文（内嵌 ASS）', '日本語', '关闭'].map((x, i) => mi(x, i === p.sub, 'sub-' + i)).join('')}${mi('添加外挂字幕…', false, 'sub-add')}<div class="c-sep"></div><div class="c-mlabel">音轨</div>${['日本語 · FLAC 2.0', '中文配音 · AAC 2.0'].map((x, i) => mi(x, i === p.audio, 'au-' + i)).join('')}`,
      rate: () => `<div class="c-mlabel">倍速</div>${[0.5, 0.75, 1, 1.25, 1.5, 2].map(r => mi(rateLabel(r), r === p.rate, 'rate-' + r)).join('')}`,
      enh: () => `<div class="c-mlabel">画质增强</div>${['关闭', 'Anime4K', 'ArtCNN 2×'].map((x, i) => mi(x, i === p.enh, 'enh-' + i)).join('')}`,
      more: () => [mi('截图', false, 'm-shot', ph ? '' : 'Ctrl S'), mi('跳过片头（85 秒）', false, 'm-skip'), p.aid != null && !byId(p.aid).movie ? mi('选集…', false, 'm-eps') : '', mi('播放信息', false, 'm-info'), this.store.has('danmaku') ? mi('手动匹配弹幕…', false, 'm-match') : '', this.touch ? mi(ph ? '锁定横屏' : '锁定方向', !!p.lock, 'm-lock') : ''].join(''),
      vol: () => `<div class="c-pad dmq" style="width:240px"><div class="q"><span class="lbx">音量</span>${sl(0.7, 'vol')}</div></div>`,
      eps: () => `<div class="c-pad"><div class="c-mlabel" style="padding:0 0 10px">选集</div><div class="epg">${Array.from({ length: EPS }, (_, i) => `<span data-mi="ep-${i + 1}" class="${i + 1 === p.ep ? 'on' : ''}">${i + 1}</span>`).join('')}</div></div>`,
      dmq: () => `<div class="c-pad dmq" style="width:340px">
        <div class="q"><span class="lbx">不透明度</span>${sl(0.8, 'op')}</div><div class="q"><span class="lbx">字号</span>${sl(0.45, 'fs')}</div>
        <div class="q"><span class="lbx">显示区域</span><span class="c-seg sm">${['1/4', '半屏', '全屏'].map((x, i) => `<span class="${i === 1 ? 'on' : ''}">${x}</span>`).join('')}</span></div>
        <div class="q" style="align-items:flex-start"><span class="lbx" style="padding-top:6px">屏蔽</span><span class="chips">${['顶部', '底部', '滚动', '彩色'].map((x, i) => `<span class="c-chip${i === 1 ? ' on' : ''}">${x}</span>`).join('')}</span></div>
        <div class="c-mi" data-mi="dm-more" style="margin:2px -6px -6px"><span class="ck"></span>更多弹幕设置…</div></div>`,
    };
    this.menu(anchor, H[kind](), 'pl-' + kind, (d, m) => this.plPick(d, m));
    this.q.pop.classList.add('dk');
  };
  A.plPick = function (d, m) {
    const p = this.P, q = this.q.player, done = () => setTimeout(() => this.menuClose(), 160);
    $$('.c-mi, .epg span', this.q.pop).forEach(x => { if (x.dataset.mi && !d.startsWith('m-') && x.dataset.mi.split('-')[0] === d.split('-')[0]) x.classList.toggle('on', x === m); });
    if (d.startsWith('sub-')) { if (d === 'sub-add') this.toast('原型：打开文件选择器挂载 ASS / SRT'); else { p.sub = +d.slice(4); $$('.lab-sub', q).forEach(x => (x.textContent = ['简中', '日本語', '关闭'][p.sub])); $('.cv .sub', q).style.display = p.sub === 2 ? 'none' : ''; } return done(); }
    if (d.startsWith('au-')) { p.audio = +d.slice(3); return done(); }
    if (d.startsWith('rate-')) { p.rate = +d.slice(5); if (p.video) p.video.playbackRate = p.rate; $$('.lab-rate', q).forEach(x => (x.textContent = rateLabel(p.rate))); return done(); }
    if (d.startsWith('enh-')) { p.enh = +d.slice(4); $$('.lab-enh', q).forEach(x => (x.textContent = ['关闭', 'Anime4K', 'ArtCNN'][p.enh])); return done(); }
    if (d.startsWith('ep-')) { this.plEp(+d.slice(3)); return done(); }
    if (d === 'dm-more') { this.menuClose(); this.plPanel('dm'); return; }
    if (d === 'm-eps') { const anc = $('[data-pa="more"]', q); this.menuClose(); setTimeout(() => this.plMenu('eps', anc), 180); return; }
    if (d === 'm-info') { this.menuClose(); this.plPanel('play'); return; }
    if (d === 'm-skip') { this.plSeek(p.t + 85); this.toast('已跳过片头'); return done(); }
    if (d === 'm-shot') { this.toast(this.touch ? '截图已存到相册' : this.tv ? '截图已保存，可以在手机上的 Erika 里取回' : '截图已保存到「图片」'); return done(); }
    if (d === 'm-match') { this.toast('原型：打开弹幕匹配窗口，搜索弹弹play 的番剧和集数'); return done(); }
    if (d === 'm-lock') { p.lock = !p.lock; this.toast(p.lock ? (this.rot ? '已锁定横屏' : '已锁定方向') : '已解除方向锁定'); return done(); }
  };

  /* ---------------- 设置面板：推开画面，不盖住画面 ---------------- */
  A.plPanel = function (tab = 'dm') {
    const p = this.P; this.menuClose(); this.plSay(false);
    const sl = v => `<div class="c-slider sm" style="--v:${v}"><i class="in"></i><i class="ac"></i><i class="th"></i></div>`;
    const sw = on => `<span class="c-switch${on ? ' on' : ''}"></span>`;
    const r = (t, right) => `<div class="c-row"><span class="grow">${t}</span>${right}</div>`;
    const tabs = [this.store.has('danmaku') && ['dm', '弹幕'], ['sub', '字幕'], ['video', '画面'], ['audio', '音频'], ['play', '播放']].filter(Boolean);
    if (!tabs.find(t => t[0] === tab)) tab = tabs[0][0];
    const C = {
      dm: `<div class="gl">显示</div>${r('显示弹幕', sw(p.dmOn))}${r('速度', sl(0.5))}${r('合并重复弹幕', sw(true))}<div class="gl">来源</div>${r('弹弹play 自动匹配', '<span class="v">已匹配</span>')}${r('弹幕偏移', '<span class="v">0.0 秒</span>')}${r('挂载本地弹幕', '<span class="v">xml / json</span>')}`,
      sub: `<div class="gl">样式</div>${r('字号', sl(0.5))}${r('字幕延迟', '<span class="v">0.0 秒</span>')}<div class="gl">轨道</div>${r('简体中文（内嵌 ASS）', p.sub === 0 ? `<span class="v">${I('check')}</span>` : '')}${r('日本語', p.sub === 1 ? `<span class="v">${I('check')}</span>` : '')}`,
      video: `<div class="gl">画面</div>${r('亮度', sl(0.5))}${r('画面比例', '<span class="v">原始</span>')}${r('HDR', '<span class="v">自动</span>')}${this.store.has('enhance') ? `<div class="gl">画质增强</div>${r('超分辨率', '<span class="v">Anime4K</span>')}` : ''}`,
      audio: `<div class="gl">音频</div>${r('音量', sl(0.7))}${r('音频延迟', '<span class="v">0 毫秒</span>')}`,
      play: `<div class="gl">播放</div>${r('跳过片头', sw(true))}${r('自动连播下一集', sw(true))}${r('控制层出现时画面退后', sw(true))}<div class="gl">信息</div>${r('播放内核', '<span class="v">liberika</span>')}${r('视频', `<span class="v">${p.video ? '本地文件' : 'HEVC · 1080p'}</span>`)}`,
    };
    p.sheet.innerHTML = `<div class="hd"><b>播放设置</b><span class="c-ib" data-pa="unpanel" title="关闭">${I('x')}</span></div><span class="c-seg sm" style="margin-bottom:4px">${tabs.map(([k, n]) => `<span class="${k === tab ? 'on' : ''}" data-ptab="${k}">${n}</span>`).join('')}</span>${C[tab]}`;
    $$('.c-ib', p.sheet).forEach(b => bindJelly(b, 'S'));
    if (p.lay !== 'panel') this.plLayout('panel');
  };

  /* ---------------- 交互 ---------------- */
  A.plDown = function (e, s) {
    const p = this.P; if (!p || e.button > 0) return;
    e.preventDefault();
    if (s.classList.contains('seek')) {
      const tip = $('.tip', s); p.drag = true;
      this.sliderDrag(s, e, v => {
        if (v == null) { p.drag = false; tip.classList.remove('on'); this.plWake(); return; }
        this.plSeek(v * p.dur); tip.classList.add('on'); tip.style.left = v * 100 + '%'; tip.textContent = `${fmt(v * p.dur)}${p.aid != null ? ' · ' + chAt(v * DUR).n : ''}`;
      });
    } else this.sliderDrag(s, e, () => {});
  };
  A.plKey = function (e) {
    if (e.target.tagName === 'INPUT') return;
    if (e.key === ' ') { e.preventDefault(); this.plPlay(!this.P.playing, true); }
    if (e.key === 'Escape') this.plBack();
  };
  A.plClick = function (e) {
    const p = this.P; if (!p) return;
    const a = e.target.closest('[data-pa]'), q = this.q.player;
    if (e.target.closest('.c-switch')) { e.target.closest('.c-switch').classList.toggle('on'); return; }
    if (e.target.closest('[data-ptab]')) { this.plPanel(e.target.closest('[data-ptab]').dataset.ptab); return; }
    if (e.target.closest('.c-seg > span')) { const x = e.target.closest('.c-seg > span'); $$(':scope > span', x.parentElement).forEach(y => y.classList.toggle('on', y === x)); return; }
    if (a) {
      const k = a.dataset.pa;
      if (k === 'back') return this.plBack();
      if (k === 'play') return this.plPlay(!p.playing, true);
      if (k === 'dm') return this.plDm();
      if (k === 'dmsplit') return;
      if (k === 'rw' || k === 'ff') { this.plSeek(p.t + (k === 'ff' ? 10 : -10)); this.plWake(); return; }
      if (k === 'next' || k === 'prev') { const n = clamp(p.ep + (k === 'next' ? 1 : -1), 1, EPS); if (n === p.ep) this.toast(k === 'next' ? '已经是最后一集' : '已经是第一集'); else this.plEp(n); return; }
      if (k === 'send') return this.plSend($('input', a.closest('label')));
      if (k === 'say') return this.plSay(true);
      if (k === 'unsay') return this.plSay(false);
      if (k === 'panel') { if (p.lay === 'panel') { this.plLayout('shown'); this.plWake(); } else this.plPanel(); return; }
      if (k === 'unpanel') { this.plLayout('shown'); this.plWake(); return; }
      if (k === 'pip') { this.toast(this.dev === 'desk' ? '原型：系统画中画 / 自绘浮窗' : '原型：进入系统画中画'); return; }
      if (k === 'fs') { this.toast('原型：全屏'); return; }
      if (k === 'match') { this.toast('原型：打开弹幕匹配窗口'); return; }
      if (['sub', 'rate', 'enh', 'more', 'dmq', 'vol'].includes(k)) { this.plMenu(k, a); return; }
      return;
    }
    if (this.menuKind) { this.menuClose(); return; }
    if (e.target.closest('.p-sheet, label')) return;
    if (e.target.closest('.cv')) {
      if (this.touch) {
        clearTimeout(p.tapT);
        p.tapT = setTimeout(() => { if (p.lay === 'full') this.plWake(); else if (p.lay === 'shown') { clearTimeout(p.hideT); this.plLayout('full'); } }, 240);
        if (e.detail === 2) { clearTimeout(p.tapT); this.plPlay(!p.playing, true); }
      } else this.plPlay(!p.playing, true);
    }
  };
})();
