/* =====================================================================
   Erika · 界面 —— 各个屏幕（宽屏外壳 + 手机外壳）
   ===================================================================== */
(() => {
  'use strict';
  const { $, $$, svg, TOKENS, Spring, run, tween, cancel, bindJelly, RM, MODS, MOD, PRESETS, PLUGINS, installed } = NP;
  const { App, I, P, STILL, ANIME, byId, imgOf, CW, TODAY, NOW, hOf, DAYS, TODAY_IDX, WEEK, RECENTS, COMMENTS, clamp, fmt, statusOf, colorOf, pcard } = NX;
  const A = App.prototype;
  const SYN = '这里是来自 Bangumi 的番剧简介。原型里所有番剧共用这一段示意文字：旅程继续向前，新的同伴和旧的回忆交织在一起，每一集都把故事往终点推近一点。';

  /* =====================================================================
     首页：舞台 + 今日时间轴
     ===================================================================== */
  A.homeHTML = function () {
    const c0 = this.cw[0], a0 = byId(c0.id);
    const also = this.cw.slice(1).map((c, i) => { const a = byId(c.id); return `<div class="h-row" data-act="swap" data-i="${i + 1}"><div class="x-thumb nojelly"><img src="${c.img}" alt="" style="object-position:${c.pos}"><div class="x-prog"><b style="width:${c.prog * 100}%"></b></div><span class="h-pl" data-act="cwplay" data-i="${i + 1}" title="直接播放">${I('play')}</span></div><div class="tx"><b>${a.t}</b><span>第 ${c.ep} 集 · 剩 ${c.left} 分钟</span></div></div>`; }).join('');
    const lib = this.store.has('library') ? `<div class="x-sechd"><span class="x-h2">媒体库 · 最近添加</span><span class="x-more" data-act="nav" data-page="lib">全部 ›</span></div><div class="h-shelf">${[1, 2, 9, 10, 12, 6, 8, 16].map(id => `<div>${pcard(byId(id), statusOf(byId(id)))}</div>`).join('')}</div>` : '';
    return `<div class="x-lt">首页<small>周一 · 10 月 12 日</small></div>
      <section class="h-stage">
        <div class="h-frame" data-act="cwplay" data-i="0"><img src="${c0.img}" alt="" style="object-position:${c0.pos}"><div class="x-prog"><b style="width:${c0.prog * 100}%"></b></div><span class="h-hint">${I('play')}</span></div>
        <div class="h-info">
          <div class="x-kick">继续观看</div>
          <div class="h-title">${a0.t}</div>
          <div class="h-meta"><span class="hl">第 ${c0.ep} 集</span> · 剩 ${c0.left} 分钟</div>
          <div class="h-acts"><span class="c-btn p L" data-act="cwplay" data-i="0">${I('play')}继续播放</span>${a0.id ? `<span class="c-btn" data-act="detail" data-aid="${a0.id}">详情</span>` : ''}</div>
          <div class="x-kick" style="margin-top:24px">也在看 · 点一下换上舞台</div>
          <div class="h-also">${also}</div>
        </div>
      </section>
      ${this.store.has('anime') ? `<div class="x-sechd"><span class="x-h2">今日更新</span><span class="x-mut">已更新 4 部 · 今晚还有 3 部</span><span class="x-more" data-act="nav" data-page="new">新番表 ›</span></div>${this.timelineHTML(false)}` : ''}
      ${lib}`;
  };
  A.timelineHTML = function (phone) {
    const H0 = 18, H1 = 26, pct = h => ((h - H0) / (H1 - H0)) * 100;
    const hours = Array.from({ length: H1 - H0 + 1 }, (_, i) => `<span class="h-hour" style="left:${pct(H0 + i)}%">${String((H0 + i) % 24).padStart(2, '0')}:00</span>`).join('');
    const shows = TODAY.map(x => { const a = byId(x.id), h = hOf(x.t), up = h > NOW; return `<div class="h-show${up ? ' up' : ''}" style="left:calc(${pct(h)}% - 44px)" data-act="detail" data-aid="${a.id}"><div class="x-poster"><img src="${imgOf(a)}" alt=""></div><div class="nm">${a.t}</div><div class="tm">${x.t}${up ? ' · 稍后' : ' · 已更新'}</div><i class="stem"></i></div>`; }).join('');
    return `<div class="h-tl"${phone ? ' style="width:900px;flex:none;margin:0"' : ''}><div class="h-ruler"></div>${hours}${shows}<div class="h-now" style="left:${pct(NOW)}%"><span class="hl">现在 22:40</span></div></div>`;
  };
  A.after_home = function (animate) { if (animate) this.tlIn(this.wide ? this.q.page : this.scr); };
  A.tlIn = function (scope) {
    if (RM()) return;
    const tl = $('.h-tl', scope); if (!tl) return;
    const ru = $('.h-ruler', tl), now = $('.h-now', tl), shows = $$('.h-show', tl);
    ru.style.transform = 'scaleX(0)'; now.style.transform = 'scaleY(0)';
    setTimeout(() => tween(TOKENS.smooth, p => { ru.style.transform = `scaleX(${p})`; }, () => { ru.style.transform = ''; }), 120);
    shows.forEach((s, i) => { s.style.opacity = '0'; s.style.transform = 'translateY(-18px)'; setTimeout(() => tween(TOKENS.jelly, p => { s.style.opacity = String(Math.min(1, p * 1.6)); s.style.transform = `translateY(${(1 - p) * -18}px)`; }, () => { s.style.transform = ''; s.style.opacity = ''; }), 220 + i * 60); });
    setTimeout(() => tween(TOKENS.bouncy, p => { now.style.transform = `scaleY(${p})`; }, () => { now.style.transform = ''; }), 300 + shows.length * 60);
  };
  A.ACT.cwplay = function (el) {
    const i = +el.dataset.i, c = this.cw[i];
    const src = el.closest('.h-frame') || el.closest('.x-thumb') || $('.h-frame', this.el) || el;
    this.plOpen({ aid: c.id, ep: c.ep, prog: c.prog, img: c.img }, this.rel(src), src);
  };
  /* 换位：点「也在看」，缩略图飞上舞台，原来的舞台飞回这一行 */
  A.ACT.swap = function (el) {
    if (this.swapping) return;
    const i = +el.dataset.i, fr = $('.h-frame', this.q.page), th = $('.x-thumb', el);
    const a = this.rel(fr), b = this.rel(th), c0 = this.cw[0], ci = this.cw[i];
    const rf = this.rad('--r-card'), rt = Math.max(0, this.rad('--r-poster') - 2);
    if (RM()) { this.cw[0] = ci; this.cw[i] = c0; this.renderPage(false); return; }
    this.swapping = true;
    $('img', fr).style.visibility = 'hidden'; $('img', th).style.visibility = 'hidden';
    this.fly(ci.img, b, a, rt, rf, null);
    this.fly(c0.img, a, b, rf, rt, () => {
      this.cw[0] = ci; this.cw[i] = c0; this.swapping = false;
      const st = this.q.scroll.scrollTop; this.renderPage(false); this.q.scroll.scrollTop = st;
      this.stagger($$('.h-info > *', this.q.page), 0, 8, 26);
    });
  };

  /* =====================================================================
     番剧详情：画框延续 + 胶片条（下一集永远在正中）
     ===================================================================== */
  A.detailHTML = function (a, phone) {
    const anime = this.store.has('anime'), next = Math.min(a.eps, a.w + 1), n = Math.min(a.eps, 12);
    const eps = Array.from({ length: n }, (_, i) => {
      const e = i + 1, w = e <= a.w, cur = e === next && a.w < a.eps;
      return `<div class="d-ep${w ? ' w' : ''}${cur ? ' cur' : ''}" data-act="epplay" data-ep="${e}"><div class="x-thumb nojelly">${w || cur ? `<img src="${imgOf(a)}" alt="" style="object-position:50% ${15 + ((i * 23) % 70)}%">` : `<span class="num">${String(e).padStart(2, '0')}</span>`}${w ? `<span class="ck">${I('check')}</span>` : ''}${cur && a.w ? '<div class="x-prog"><b style="width:35%"></b></div>' : ''}</div><div class="et">${cur ? `<span class="hl">第 ${e} 话 · ${a.w ? '继续' : '开始'}</span>` : `第 ${e} 话`}</div><div class="es">${w ? '已看' : cur && a.w ? '剩 16 分钟' : '24 分钟'}</div></div>`;
    }).join('');
    const tabs = (anime ? [['info', '简介'], ['cmt', '评论'], ['staff', '制作']] : []).concat([['file', '文件']]);
    const status = a.w ? (a.w >= a.eps ? '看过' : '在看') : '想看';
    const acts = `<span class="c-btn p L" data-act="epplay" data-ep="${next}"${phone ? ' style="width:100%"' : ''}>${I('play')}${a.w ? `继续 第 ${next} 话` : '开始观看'}</span>${anime ? `<span class="c-btn" data-act="status" data-menu-anchor>${I('heart')}<span class="st-l">${status}</span><span class="chev">${I('chev-d')}</span></span><span class="c-ib" data-act="rate" data-menu-anchor title="评分">${I('star')}</span>` : ''}<span class="c-ib" data-act="dmore" data-menu-anchor title="更多">${I('more')}</span>`;
    return `<div class="d-wash"></div>
      <div class="d-head"${phone ? ' style="gap:16px"' : ''}><div class="x-poster d-poster"${phone ? ' style="width:118px"' : ''}><img src="${imgOf(a)}" alt=""></div>
        <div class="d-info"${phone ? ' style="padding-top:4px"' : ''}><div class="x-kick">${a.movie ? '剧场版' : 'TV 动画'} · 2026 年 1 月 · 共 ${a.eps} 话</div>
          <div class="d-title"${phone ? ' style="font-size:21px;margin:6px 0 8px"' : ''}>${a.t}</div>
          ${anime ? `<div class="d-rate">${a.r ? `<span class="sc">★ ${a.r}</span><span class="x-mut">Bangumi</span>` : ''}<span class="d-tags">${a.tags.map(t => `<span>${t}</span>`).join('')}</span></div>` : '<div class="x-mut">本地文件 · 没开「追番」模块，不显示评分和简介</div>'}
          ${phone ? '' : anime ? `<div class="d-syn">${SYN}</div>` : '<div style="height:20px"></div>'}
          ${phone ? '' : `<div class="d-acts">${acts}</div>`}
        </div></div>
      ${phone ? `<div class="d-acts" style="margin-top:16px">${acts}</div>${anime ? `<div class="d-syn" style="margin:14px 0 0">${SYN}</div>` : ''}` : ''}
      <div class="d-strip-hd"><span class="x-h2">剧集</span><span class="x-mut">共 ${a.eps} 话 · 看到第 ${a.w} 话</span><span class="grow"></span>${phone ? '' : `<span class="c-ib" data-act="strip" data-d="-1" title="上一页">${I('chev-l')}</span><span class="c-ib" data-act="strip" data-d="1" title="下一页">${I('chev-r')}</span>`}</div>
      <div class="d-strip"${phone ? ' style="margin:0 -16px"' : ''}><div class="d-track">${eps}</div></div>
      <span class="c-seg d-tabs">${tabs.map(([k, n], i) => `<span class="${i === 0 ? 'on' : ''}" data-act="dtab" data-tab="${k}">${n}</span>`).join('')}</span>
      <div class="d-tab">${this.dtab(a, tabs[0][0])}</div>`;
  };
  A.dtab = function (a, t) {
    if (t === 'info') return `<div class="d-bars" style="max-width:640px"><p class="x-mut" style="margin:0 0 14px">${SYN}</p><div style="display:flex;gap:8px;flex-wrap:wrap">${a.tags.concat([`共 ${a.eps} 话`, 'Madhouse', '2026 年 1 月']).map(x => `<span class="c-chip" style="pointer-events:none">${x}</span>`).join('')}</div></div>`;
    if (t === 'cmt') return COMMENTS.map(c => `<div class="d-cmt"><span class="x-av"></span><div><b>${c.n}</b><span class="st">${'★'.repeat(Math.round(c.s / 2))}${'☆'.repeat(5 - Math.round(c.s / 2))}</span><p>${c.t}</p></div></div>`).join('');
    if (t === 'staff') return `<div class="c-list" style="max-width:640px">${['原作', '导演', '系列构成', '角色设计', '音乐', '动画制作'].map((k, i) => `<div class="c-li"><div class="tx"><b>${k}</b></div><span class="v">${['山田钟人 / 阿部司', '北川朋哉', '铃木智寻', '长岛令子', 'Evan Call', 'Madhouse'][i]}</span></div>`).join('')}</div>`;
    return `<div class="c-list" style="max-width:640px">${Array.from({ length: Math.min(a.eps, 3) }, (_, i) => `<div class="c-li"><span class="lic">${I('film')}</span><div class="tx"><b>${a.t} - ${String(i + 1).padStart(2, '0')} [1080p].mkv</b><span>HEVC · 1920×1080 · ASS 字幕 · 1.1 GB</span></div></div>`).join('')}</div>`;
  };
  A.after_detail = function () {
    const scope = this.wide ? this.q.page : this.scr, a = byId(this.S.detail);
    const wash = $('.d-wash', scope);
    colorOf(imgOf(a)).then(c => { wash.style.setProperty('--pc', c); });
    if (this.ph) { wash.style.left = '-16px'; wash.style.right = '-16px'; wash.style.top = '-60px'; }
    const strip = $('.d-strip', scope), cur = $('.d-ep.cur', strip) || $('.d-ep', strip);
    const mag = () => {
      const S = strip.getBoundingClientRect(), cx = S.left + S.width / 2, k = this.k;
      $$('.d-ep', strip).forEach(ep => { const r = ep.getBoundingClientRect(); const d = Math.abs(r.left + r.width / 2 - cx) / k; ep.style.transform = `scale(${(1 + 0.12 * Math.max(0, 1 - d / 260)).toFixed(4)})`; });
    };
    requestAnimationFrame(() => {
      if (cur) strip.scrollLeft = cur.offsetLeft + cur.offsetWidth / 2 - strip.clientWidth / 2;
      mag();
    });
    let raf = 0; strip.addEventListener('scroll', () => { cancelAnimationFrame(raf); raf = requestAnimationFrame(mag); });
  };
  A.openDetail = function (aid, srcEl, animate = true) {
    if (+aid === 0) { this.toast('本地文件，没有番剧详情'); return; }
    const a = byId(aid);
    const src = srcEl ? (srcEl.classList.contains('x-poster') ? srcEl : $('.x-poster', srcEl) || srcEl) : null;
    const from = src && animate ? this.rel(src) : null;
    this.S.detail = +aid;
    if (this.wide) {
      this.S.scroll[this.S.page] = this.q.scroll.scrollTop;
      this.renderTb(); this.q.page.innerHTML = this.detailHTML(a); this.bindPage(this.q.page); this.after_detail(); this.q.scroll.scrollTop = 0;
    } else {
      const st = this.S.phStack, top = st[st.length - 1];
      if (this.twin && top && top.type === 'detail') st.pop();
      st.push({ type: 'detail', aid: +aid }); this.renderPhone(false, from ? 0 : 1);
    }
    const scope = this.wide ? this.q.page : this.scr, cover = $('.d-poster', scope);
    if (animate) this.stagger($$('.d-info > *, .d-acts, .d-strip-hd, .d-strip, .d-tabs, .d-tab', scope).filter(x => !x.closest('.d-info') || x.parentElement.classList.contains('d-info')), 60, 12, 28);
    if (from && !RM()) {
      const r = this.rad('--r-poster');
      cover.style.visibility = 'hidden'; src.style.visibility = 'hidden';
      requestAnimationFrame(() => this.fly(imgOf(a), from, this.rel(cover), r, r, () => { cover.style.visibility = ''; src.style.visibility = ''; }));
    }
  };
  A.closeDetail = function () {
    const a = byId(this.S.detail), cover = $('.d-poster', this.wide ? this.q.page : this.scr), from = cover && this.rel(cover);
    this.S.detail = null;
    if (this.ph) { this.S.phStack.pop(); this.renderPhone(false, -1); return; }
    this.renderTb(); this.renderPage(false); this.q.scroll.scrollTop = this.S.scroll[this.S.page] || 0;
    const target = $(`.x-poster[data-aid="${a.id}"], .h-show[data-aid="${a.id}"] .x-poster, .n-row[data-aid="${a.id}"] .x-poster`, this.q.page);
    const ok = target && (() => { const r = this.rel(target), v = this.rel(this.q.scroll); return r.y + r.h > v.y && r.y < v.y + v.h; })();
    if (from && ok && !RM()) { const r = this.rad('--r-poster'); target.style.visibility = 'hidden'; this.fly(imgOf(a), from, this.rel(target), r, r, () => { target.style.visibility = ''; }); this.animIn(this.q.page, 0); }
    else this.animIn(this.q.page);
  };
  Object.assign(A.ACT, {
    epplay(el) {
      const a = byId(this.S.detail), ep = +el.dataset.ep, scope = this.wide ? this.q.page : this.scr;
      const th = el.classList.contains('d-ep') ? $('.x-thumb', el) : ($(`.d-ep[data-ep="${ep}"] .x-thumb`, scope) || $('.d-poster', scope));
      this.plOpen({ aid: a.id, ep, img: imgOf(a) }, this.rel(th), th);
    },
    strip(el) { const s = $('.d-strip', this.q.page); s.scrollBy({ left: +el.dataset.d * 250 * 2, behavior: 'smooth' }); },
    dtab(el) { $$('[data-act="dtab"]', el.parentElement).forEach(x => x.classList.toggle('on', x === el)); const b = $('.d-tab', el.parentElement.parentElement); b.innerHTML = this.dtab(byId(this.S.detail), el.dataset.tab); this.bindPage(b); this.animIn(b, 8); },
    status(el) {
      const cur = $('.st-l', el).textContent;
      this.menu(el, ['想看', '在看', '看过', '搁置', '抛弃'].map(t => `<div class="c-mi${t === cur ? ' on' : ''}" data-mi="${t}"><span class="ck">${I('check')}</span>${t}</div>`).join(''), 'status', t => { $('.st-l', el).textContent = t; this.menuClose(); this.toast(this.store.has('sync') ? `已标记为「${t}」，同步到 Bangumi` : `已标记为「${t}」`); });
    },
    rate(el) { this.menu(el, `<div class="c-mlabel">评分</div>` + [10, 9, 8, 7, 6, 5].map(n => `<div class="c-mi" data-mi="${n}"><span class="ck">${I('check')}</span>${'★'.repeat(Math.round(n / 2))}${'☆'.repeat(5 - Math.round(n / 2))}<span class="k">${n} 分</span></div>`).join(''), 'rate', n => { this.menuClose(); NP.wobble(el); this.toast(`评分：${n} 分`); }); },
    dmore(el) { this.menu(el, [['danmaku', '手动匹配弹幕…'], ['folder', '在文件夹中显示'], ['sync', '刷新番剧信息'], ['x', '从媒体库移除']].map(([i, t]) => `<div class="c-mi" data-mi="${t}"><span class="mic">${I(i)}</span>${t}</div>`).join(''), 'dmore', t => { this.menuClose(); this.toast(`原型：${t}`); }); },
  });

  /* =====================================================================
     新番：手风琴周历
     ===================================================================== */
  A.newHTML = function () {
    return `<div class="x-lt">新番<small>2026 年 10 月 · 秋季新番 · 第 2 周</small></div><div class="n-acc">${WEEK.map((l, i) => this.newCol(l, i)).join('')}</div>`;
  };
  A.newRows = function (list, i) {
    return list.map(x => { const done = i < TODAY_IDX || (i === TODAY_IDX && hOf(x.t) <= NOW); return `<div class="n-row${done ? ' done' : ''}" data-act="detail" data-aid="${x.a.id}"><span class="tm">${x.t}</span><div class="x-poster"><img src="${imgOf(x.a)}" alt=""></div><div class="tx"><b>${x.a.t}</b><span>${done ? `第 ${Math.max(1, x.a.w + 1)} 集 · 已更新` : i === TODAY_IDX ? '今晚更新' : '等待更新'}</span></div></div>`; }).join('');
  };
  A.newCol = function (list, i) {
    const on = i === this.S.newDay, today = i === TODAY_IDX, peek = this.tv ? 3 : 4;
    return `<div class="n-col${on ? ' on' : ''}" data-act="day" data-d="${i}">
      <div class="n-hd"><b>${DAYS[i]}</b><span>${today ? '<span class="hl">今天</span> · ' : i === TODAY_IDX + 1 ? '明天 · ' : i === TODAY_IDX + 2 ? '后天 · ' : ''}${list.length} 部</span></div>
      <div class="n-peek">${list.slice(0, peek).map(x => `<div class="n-pk"><div class="x-poster"><img src="${imgOf(x.a)}" alt=""></div><span>${x.t}</span></div>`).join('')}${list.length > peek ? `<span class="n-more">+${list.length - peek}</span>` : ''}</div>
      <div class="n-list">${this.newRows(list, i)}</div></div>`;
  };
  A.newLayout = function (snap) {
    const acc = $('.n-acc', this.q.page); if (!acc) return;
    const cols = $$('.n-col', acc), W = acc.clientWidth, cw = 86, gap = 8, ew = W - 6 * cw - 6 * gap;
    if (!this._nw || this._nw.length !== 7) this._nw = cols.map((c, i) => new Spring(i === this.S.newDay ? ew : cw));
    cols.forEach((c, i) => { this._nw[i].cfg(TOKENS.bouncy).target = i === this.S.newDay ? ew : cw; $('.n-list', c).style.width = ew + 'px'; });
    const apply = () => cols.forEach((c, i) => { c.style.width = this._nw[i].v.toFixed(2) + 'px'; });
    if (snap || RM()) { this._nw.forEach(s => s.snap()); apply(); return; }
    cancel(this._nwA); this._nwA = run({ sp: this._nw, eps: 0.3, apply });
  };
  A.after_new = function (animate) { this._nw = null; this.newLayout(true); if (animate) this.stagger($$('.n-col', this.q.page), 60, 18, 36); };
  A.ACT.day = function (el) {
    const d = +el.dataset.d; if (d === this.S.newDay) return;
    this.S.newDay = d;
    $$('.n-col', this.q.page).forEach((c, i) => c.classList.toggle('on', i === d));
    this.newLayout(false);
    const col = $$('.n-col', this.q.page)[d];
    setTimeout(() => this.stagger($$('.n-row', col), 0, 10, 30), 90);
  };

  /* =====================================================================
     媒体库：缩放墙 + 拼音索引
     ===================================================================== */
  const SERVER_IDS = [7, 9, 3, 5, 10, 6, 1, 12, 2], SHARE_IDS = [6, 7, 13, 14, 1, 2];
  A.libSet = function () {
    const L = this.S.lib;
    let items = L.seg === 'server' ? SERVER_IDS.map(byId) : L.seg === 'share' ? SHARE_IDS.map(byId) : ANIME.slice();
    if (L.filter === 'watching') items = items.filter(a => a.w > 0 && a.w < a.eps);
    if (L.filter === 'unwatched') items = items.filter(a => a.w === 0);
    if (L.filter === 'done') items = items.filter(a => a.w >= a.eps);
    if (L.sort === 'name') items.sort((a, b) => a.py.localeCompare(b.py) || a.t.localeCompare(b.t, 'zh'));
    if (L.sort === 'rating') items.sort((a, b) => b.r - a.r);
    if (L.sort === 'recent') items.sort((a, b) => (b.w > 0 && b.w < b.eps) - (a.w > 0 && a.w < a.eps));
    return items;
  };
  A.libHTML = function (phone) {
    const L = this.S.lib;
    const segs = [['local', '本地'], this.store.has('sources') && ['server', '服务器'], this.store.has('remote') && ['share', '共享']].filter(Boolean);
    if (!segs.find(s => s[0] === L.seg)) L.seg = 'local';
    const all = ANIME, cnt = { all: all.length, watching: all.filter(a => a.w > 0 && a.w < a.eps).length, unwatched: all.filter(a => a.w === 0).length, done: all.filter(a => a.w >= a.eps).length };
    const chip = (k, n) => `<span class="c-chip${L.filter === k ? ' on' : ''}" data-act="libf" data-f="${k}">${L.filter === k ? I('check') : ''}${n} ${cnt[k]}</span>`;
    const sortName = { recent: '最近观看', name: '名称', rating: '评分' }[L.sort];
    const letters = 'ABCDEFGHJKLMNOPQRSTWXYZ'.split('');
    const zoom = `<span class="l-zoom" title="海报大小（⌘+ / ⌘−，触屏可以双指缩放）">${I('grid')}<div class="c-slider sm l-zs" style="--v:${L.zoom}"><i class="in"></i><i class="ac"></i><i class="th"></i></div><span class="big">${I('grid')}</span></span>`;
    const seg = segs.length > 1 ? `<span class="c-seg${phone ? ' sm' : ''}">${segs.map(([k, n]) => `<span class="${k === L.seg ? 'on' : ''}" data-act="libseg" data-seg="${k}">${n}</span>`).join('')}</span>` : '';
    const head = phone
      ? `<div class="x-pnav"><div class="x-lt">媒体库</div><span style="display:flex;gap:8px"><span class="c-ib" data-act="toast" data-t="管理媒体库：文件夹、扫描、服务器" title="管理">${I('folder')}</span><span class="c-ib" data-act="libsort" data-menu-anchor title="排序">${I('sort')}</span></span></div><div style="display:flex;align-items:center;gap:10px;margin:2px 0 12px">${seg}<span class="grow"></span>${zoom}</div><div class="x-hrow l-chips" style="flex-wrap:nowrap">`
      : `<div class="l-hd"><div class="x-lt">媒体库</div>${seg}<span class="grow"></span>${zoom}<span class="c-btn" data-act="libsort" data-menu-anchor>${I('sort')}<span class="lab">${sortName}</span><span class="chev">${I('chev-d')}</span></span><span class="c-btn" data-act="toast" data-t="管理媒体库：文件夹、扫描、服务器">${I('folder')}管理</span></div><div class="l-chips">`;
    return `${head}${chip('all', '全部')}${chip('watching', '在看')}${chip('unwatched', '未看')}${chip('done', '看完')}</div>
      <div class="l-wrap"${phone ? ' style="padding-right:22px"' : ''}><div class="l-grid">${ANIME.map(a => `<div class="l-item" data-id="${a.id}">${pcard(a, statusOf(a))}</div>`).join('')}</div><div class="l-idx">${letters.map(l => `<span data-act="libidx" data-l="${l}">${l}</span>`).join('')}</div></div>`;
  };
  A.libLayout = function (snap) {
    const scope = this.wide ? this.q.page : this.scr, grid = $('.l-grid', scope); if (!grid) return;
    const L = this.S.lib, ph = this.ph, W = grid.clientWidth;
    const size = ph ? 64 + L.zoom * (150 - 64) : this.tv ? 112 + L.zoom * (230 - 112) : 84 + L.zoom * (200 - 84);
    const gap = size < 110 ? 10 : ph ? 12 : 20;
    const cols = Math.max(2, Math.floor((W + gap) / (size + gap))), w = (W - gap * (cols - 1)) / cols;
    const titles = w >= (ph ? 96 : 112), ih = w * 1.5 + (titles ? 54 : 0), rg = titles ? 16 : gap;
    grid.classList.toggle('tiny', !titles);
    const vis = this.libSet(), pos = new Map(vis.map((a, i) => [a.id, i]));
    const rows = Math.ceil(vis.length / cols);
    const items = $$('.l-item', grid), all = [];
    items.forEach(it => {
      const id = +it.dataset.id, i = pos.get(id), on = i != null;
      if (!it._sp) it._sp = { x: new Spring(0), y: new Spring(0), w: new Spring(w), o: new Spring(0) };
      const sp = it._sp, tok = TOKENS.smooth;
      if (on) { sp.x.cfg(tok).target = (i % cols) * (w + gap); sp.y.cfg(tok).target = Math.floor(i / cols) * (ih + rg); sp.w.cfg(tok).target = w; }
      sp.o.cfg(TOKENS.snappy).target = on ? 1 : 0;
      if (snap) { if (!on && sp.o.v === 0) { sp.x.target = sp.x.v; sp.y.target = sp.y.v; } [sp.x, sp.y, sp.w, sp.o].forEach(s => s.snap()); }
      it.style.pointerEvents = on ? '' : 'none';
      all.push(sp.x, sp.y, sp.w, sp.o);
    });
    const gh = Math.max(0, rows * ih + (rows - 1) * rg);
    if (!this._lgh) this._lgh = new Spring(gh);
    this._lgh.cfg(TOKENS.smooth).target = gh; if (snap) this._lgh.snap();
    const apply = () => {
      items.forEach(it => { const s = it._sp; it.style.transform = `translate(${s.x.v.toFixed(1)}px, ${s.y.v.toFixed(1)}px) scale(${(0.86 + 0.14 * s.o.v).toFixed(3)})`; it.style.width = s.w.v.toFixed(1) + 'px'; it.style.opacity = s.o.v.toFixed(3); });
      grid.style.height = this._lgh.v + 'px';
    };
    const has = new Set(vis.map(a => a.py));
    $$('.l-idx span', scope).forEach(s => s.classList.toggle('has', has.has(s.dataset.l)));
    if (snap || RM()) { all.forEach(s => s.snap()); this._lgh.snap(); apply(); return; }
    cancel(this._lA); this._lA = run({ sp: all.concat([this._lgh]), eps: 0.2, apply });
  };
  A.after_lib = function (animate) {
    const scope = this.wide ? this.q.page : this.scr;
    $$('.l-item', scope).forEach(it => { it._sp = null; }); this._lgh = null;
    this.libLayout(true);
    const zs = $('.l-zs', scope);
    zs.addEventListener('nxslide', e => { this.S.lib.zoom = e.detail; this.libLayout(false); });
    if (animate) this.stagger($$('.l-item', scope).filter(x => x.style.pointerEvents !== 'none').map(x => $('.x-pcard', x)), 80, 16, 22);
  };
  Object.assign(A.ACT, {
    libf(el) { this.S.lib.filter = el.dataset.f; $$('[data-act="libf"]', el.parentElement).forEach(x => { const on = x === el; x.classList.toggle('on', on); const ic = $('.ic', x); if (on && !ic) x.insertAdjacentHTML('afterbegin', I('check')); if (!on && ic) ic.remove(); }); this.libLayout(false); },
    libseg(el) { this.S.lib.seg = el.dataset.seg; $$('[data-act="libseg"]', el.parentElement).forEach(x => x.classList.toggle('on', x === el)); this.libLayout(false); },
    libsort(el) {
      const L = this.S.lib;
      this.menu(el, `<div class="c-mlabel">排序</div>` + [['recent', '最近观看'], ['name', '名称（拼音）'], ['rating', '评分']].map(([k, t]) => `<div class="c-mi${L.sort === k ? ' on' : ''}" data-mi="${k}"><span class="ck">${I('check')}</span>${t}</div>`).join(''), 'libsort', k => {
        L.sort = k; this.menuClose(); const lab = $('.lab', el); if (lab) lab.textContent = { recent: '最近观看', name: '名称', rating: '评分' }[k]; this.libLayout(false);
      });
    },
    libidx(el) {
      const scope = this.wide ? this.q.page : this.scr, a = this.libSet().find(x => x.py === el.dataset.l);
      if (!a) { NP.wobble(el); return; }
      const it = $(`.l-item[data-id="${a.id}"]`, scope), sc = this.wide ? this.q.scroll : this.scr;
      const top = it._sp.y.target + $('.l-grid', scope).offsetTop - 12, from = sc.scrollTop;
      $$('.l-idx span', scope).forEach(x => x.classList.toggle('on', x === el));
      tween(TOKENS.smooth, p => { sc.scrollTop = from + (top - from) * p; }, () => { NP.wobble($('.x-poster', it)); setTimeout(() => el.classList.remove('on'), 600); });
    },
    toast(el) { this.toast(el.dataset.t); },
  });

  /* =====================================================================
     下载：行的底色就是进度
     ===================================================================== */
  A.dlRow = function (d, i) {
    const meta = d.prog >= 1 ? `${d.size} · 已完成` : `${d.size} · ${Math.round(d.prog * 100)}% · ${d.on ? d.speed.toFixed(1) + ' MB/s' : '已暂停'}`;
    return `<div class="c-li dl-row" data-i="${i}"><span class="dl-fill" style="width:${d.prog * 100}%"></span><span class="lic">${I(d.prog >= 1 ? 'check' : 'download')}</span><div class="tx"><b style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${d.t}</b><span class="dl-meta">${meta}</span></div>${d.prog >= 1 ? `<span class="c-btn" data-act="dlplay">${I('play')}播放</span>` : `<span class="c-ib" data-act="dltog" data-i="${i}" title="${d.on ? '暂停' : '继续'}">${I(d.on ? 'pause' : 'play')}</span>`}</div>`;
  };
  A.dlHTML = function (phone) {
    const on = this.S.dlTab === 'on', n1 = this.dl.filter(d => d.prog < 1).length, n2 = this.dl.length - n1;
    const list = this.dl.map((d, i) => [d, i]).filter(([d]) => (on ? d.prog < 1 : d.prog >= 1));
    const seg = `<span class="c-seg${phone ? ' sm' : ''}"><span class="${on ? 'on' : ''}" data-act="dltab" data-t="on">进行中 ${n1}</span><span class="${!on ? 'on' : ''}" data-act="dltab" data-t="done">已完成 ${n2}</span></span>`;
    return `${phone ? `<div class="x-pnav"><div class="x-lt">下载</div><span class="c-ib" data-act="dladd" title="添加任务">${I('plus')}</span></div><div style="margin:4px 0 14px">${seg}</div>` : `<div class="l-hd"><div class="x-lt">下载</div>${seg}<span class="grow"></span><span class="c-btn p" data-act="dladd">${I('plus')}添加任务</span></div>`}
      <div class="c-list dl-list">${list.map(([d, i]) => this.dlRow(d, i)).join('') || `<div class="c-li"><div class="tx"><b>${on ? '没有正在下载的任务' : '还没有下载完成的文件'}</b></div></div>`}</div>
      <p class="x-mut" style="margin-top:14px;font:var(--t-cap)">行的底色就是进度。下载完成的文件会自动加入媒体库。</p>`;
  };
  A.dlTick = function () {
    let done = null;
    this.dl.forEach((d, i) => {
      if (!d.on || d.prog >= 1) return;
      d.prog = Math.min(1, d.prog + d.speed / 6000);
      const row = $(`.dl-row[data-i="${i}"]`, this.el); if (row) { $('.dl-fill', row).style.width = d.prog * 100 + '%'; $('.dl-meta', row).textContent = `${d.size} · ${Math.round(d.prog * 100)}% · ${d.speed.toFixed(1)} MB/s`; }
      if (d.prog >= 1) { d.on = false; done = [d, i, row]; }
    });
    if (done) {
      const [d, i, row] = done;
      $$('.c-navi[data-page="dl"] .cnt', this.el).forEach(x => (x.textContent = this.dl.filter(z => z.prog < 1).length));
      if (row && !RM()) { const h = row.offsetHeight; row.style.overflow = 'hidden'; tween(TOKENS.smooth, p => { row.style.height = h * (1 - p) + 'px'; row.style.minHeight = '0'; row.style.opacity = String(1 - p); }, () => { if (this.S.page === 'dl') this.rerender(); }); }
      else if (this.S.page === 'dl') this.rerender();
      this.toast(`下载完成：${d.t.slice(0, 26)}…`);
    }
  };
  A.rerender = function () { if (this.wide) { const st = this.q.scroll.scrollTop; this.renderPage(false); this.q.scroll.scrollTop = st; } else this.renderPhone(false); };
  Object.assign(A.ACT, {
    dltab(el) { this.S.dlTab = el.dataset.t; this.rerender(); },
    dltog(el) { const d = this.dl[+el.dataset.i]; d.on = !d.on; if (d.on && !d.speed) d.speed = 4.2; el.innerHTML = I(d.on ? 'pause' : 'play'); NP.wobble(el); },
    dlplay() { this.plOpen({ aid: 3, ep: 3, img: P(3) }, null); },
    dladd() { this.dl.unshift({ t: '新任务 · 正在获取元数据…', size: '—', prog: 0.02, speed: 9, on: true }); this.S.dlTab = 'on'; this.rerender(); const r = $('.dl-row', this.el); r && NP.enter(r, 'h'); },
  });

  /* =====================================================================
     纯粹模式：空画框（和播放器退后时的画框一样大、一样的位置）
     ===================================================================== */
  A.idleRect = function () {
    if (this.wide) return this.plRect('shown');
    const f = $('.i-frame', this.el); return f ? this.rel(f) : { x: 16, y: 200, w: 358, h: 201 };
  };
  A.renderIdle = function (animate) {
    const R = this.idleRect(), q = this.q.idle;
    q.style.display = 'block';
    const T = { desk: ['把视频拖到这里', '支持文件、文件夹、链接和字幕文件，也可以按 ⌘O'], tablet: ['把视频拖到这里', '可以从「文件」或分屏里的另一个 App 拖进来，也可以用「打开方式」发给 Erika'], tv: ['打开一个视频', '从本机存储、U 盘或局域网共享打开，也可以在手机上把视频投送过来'] }[this.dev];
    q.innerHTML = `<div class="i-frame" style="left:${R.x}px;top:${R.y}px;width:${R.w}px;height:${R.h}px">
        <span class="big">${I('film')}</span><h4>${T[0]}</h4><p>${T[1]}</p>
        <div class="acts"><span class="c-btn p L" data-act="open">${I('folder')}打开文件</span><span class="c-btn" data-act="open">打开文件夹</span><span class="c-btn" data-act="openlink">打开链接…</span></div>
        <img class="fill" src="${STILL}" alt=""></div>
      <div class="i-recent" style="left:${R.x}px;top:${R.y + R.h + 22}px;width:${R.w}px">${RECENTS.map((r, i) => `<div class="i-rc" data-act="recent" data-i="${i}"><div class="x-thumb nojelly"><img src="${r.img}" alt=""><div class="x-prog"><b style="width:${r.prog * 100}%"></b></div></div><div class="tx"><b>${r.t}</b><span>剩 ${r.left}</span></div></div>`).join('')}<span class="c-btn i-key" data-act="recent" data-i="0">${I('play')}继续上次${this.dev === 'desk' ? '<kbd>空格</kbd>' : ''}</span></div>`;
    this.bindPage(q);
    if (animate && !RM()) { const f = $('.i-frame', q); f.style.transform = 'scale(.96)'; f.style.opacity = '0'; tween(TOKENS.jelly, p => { f.style.transform = `scale(${0.96 + 0.04 * p})`; f.style.opacity = String(Math.min(1, p * 2)); }, () => { f.style.transform = ''; f.style.opacity = ''; }); this.stagger($$('.i-rc, .i-key', q), 160, 10, 50); }
  };
  Object.assign(A.ACT, {
    recent(el) { const r = RECENTS[+el.dataset.i]; this.plOpen({ title: r.t.replace(/\.[^.]+$/, ''), img: r.img, aid: null, prog: r.prog }, this.pure() ? this.idleRect() : this.rel($('.x-thumb', el)), null, this.pure()); },
    openlink() { this.toast('原型：弹出「打开链接」对话框，支持 http(s) 和 magnet'); },
  });

  /* =====================================================================
     迷你播放器
     ===================================================================== */
  A.miniHTML = function (phone) {
    const p = this.P, r = (p.t / p.dur) * 100;
    if (phone) return `<div class="x-mp"><div class="x-thumb nojelly" data-act="miniexp"><img src="${p.img}" alt=""><div class="x-prog"><b class="mp-b" style="width:${r}%"></b></div></div><div class="r" data-act="miniexp"><div class="tx"><b>${p.title}</b><span>${p.sub2} · <span class="mp-t">${fmt(p.t)}</span></span></div></div><span class="c-ib" data-act="minitog">${I(p.playing ? 'pause' : 'play')}</span><span class="c-ib" data-act="miniclose" style="margin-right:6px">${I('x')}</span></div>`;
    return `<div class="x-mp"><div class="x-thumb nojelly" data-act="miniexp" title="回到播放器"><img src="${p.img}" alt=""><div class="x-prog"><b class="mp-b" style="width:${r}%"></b></div></div><div class="r"><div class="tx"><b>${p.title}</b><span>${p.sub2} · <span class="mp-t">${fmt(p.t)}</span></span></div><span class="c-ib" data-act="minitog">${I(p.playing ? 'pause' : 'play')}</span><span class="c-ib" data-act="miniclose" title="停止播放">${I('x')}</span></div></div>`;
  };
  A.renderMini = function () {
    const on = this.P && this.P.mode === 'mini';
    if (this.wide) { this.q.mini.innerHTML = on ? this.miniHTML() : ''; if (on) this.bindPage(this.q.mini); }
    else { this.q.pmini.style.display = on ? 'block' : 'none'; this.q.pmini.innerHTML = on ? this.miniHTML(true) : ''; if (on) this.bindPage(this.q.pmini); this.q.pmini.style.bottom = this.pure() ? '24px' : ''; }
  };
  Object.assign(A.ACT, {
    miniexp() { this.plExpand(); },
    minitog(el) { this.P.playing = !this.P.playing; el.innerHTML = I(this.P.playing ? 'pause' : 'play'); },
    miniclose() {
      const m = this.wide ? this.q.mini.firstElementChild : this.q.pmini.firstElementChild;
      const fin = () => { if (this.P && this.P.video) this.P.video.pause(); this.P = null; this.renderMini(); };
      if (m && !RM()) NP.leave(m, this.wide ? 'h' : null, fin); else fin();
    },
  });

  /* =====================================================================
     搜索：搜索框就地展开成结果面板
     ===================================================================== */
  const SET_INDEX = [['palette', '主题色', '设置 › 外观', 'appearance'], ['danmaku', '弹幕不透明度', '设置 › 弹幕', 'danmaku'], ['play', '跳过片头秒数', '设置 › 播放', 'player'], ['grid', '可选功能', '设置', 'optional'], ['puzzle', '插件市场', '设置 › 插件', 'plugins'], ['keyboard', '快捷键', '设置', 'keys']];
  A.palResults = function (q) {
    q = (q || '').trim();
    const an = ANIME.filter(a => !q || a.t.includes(q) || a.py.toLowerCase() === q.toLowerCase()).slice(0, 6);
    const items = [];
    let h = '';
    if (an.length) h += `<div class="gl">番剧</div><div class="pp">${an.map(a => `<div data-pact="detail" data-aid="${a.id}"><div class="x-poster"><img src="${imgOf(a)}" alt=""></div><span>${a.t}</span></div>`).join('')}</div>`;
    const ep = an.filter(a => a.w > 0 && a.w < a.eps).slice(0, 2);
    if (ep.length) { h += '<div class="gl">继续播放</div>'; ep.forEach(a => { items.push({ k: 'play', aid: a.id, ep: a.w + 1 }); h += `<div class="it" data-pi="${items.length - 1}"><span class="lic">${I('play')}</span>${a.t}<span class="sub">第 ${a.w + 1} 话</span><span class="k">↵</span></div>`; }); }
    const st = SET_INDEX.filter(s => !q || s[1].includes(q) || s[2].includes(q)).slice(0, q ? 4 : 3);
    if (st.length) { h += '<div class="gl">设置</div>'; st.forEach(s => { items.push({ k: 'set', sec: s[3] }); h += `<div class="it" data-pi="${items.length - 1}"><span class="lic">${I(s[0])}</span>${s[1]}<span class="sub">${s[2]}</span></div>`; }); }
    const ops = [['folder', '打开文件…', '⌘O', 'open'], ['calendar', '新番表', '', 'new'], ['moon', '切换深色模式', '', 'dark']].filter(o => !q || o[1].includes(q));
    if (ops.length) { h += '<div class="gl">操作</div>'; ops.forEach(o => { items.push({ k: 'op', op: o[3] }); h += `<div class="it" data-pi="${items.length - 1}"><span class="lic">${I(o[0])}</span>${o[1]}<span class="k">${o[2]}</span></div>`; }); }
    if (!h) h = `<div class="em">没有找到「${q}」</div>`;
    return { h, items };
  };
  A.searchHTML = function () {
    const keys = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('');
    return `<div class="x-lt">搜索<small>用遥控器选拼音首字母，或按住遥控器的语音键说出番名</small></div>
      <div class="tv-search"><div class="tv-kb">${keys.map(k => `<span class="c-btn" data-act="tvkey" data-k="${k}">${k}</span>`).join('')}<span class="c-btn w2" data-act="tvkey" data-k="space">空格</span><span class="c-btn w2" data-act="tvkey" data-k="del">删除</span><span class="c-btn w2" data-act="tvkey" data-k="clear">清空</span></div>
      <div class="tv-sr"><label class="c-field x-search">${I('search')}<input class="ph-q" placeholder="番剧、剧集、设置"></label><div class="ph-res x-pal" style="position:relative;display:block;background:none;box-shadow:none;backdrop-filter:none;-webkit-backdrop-filter:none"><div class="res" style="max-height:none;padding:0"></div></div></div></div>`;
  };
  A.after_search = function () { const i = $('.ph-q', this.q.page); this.palQuery(i ? i.value : '', $('.ph-res', this.q.page)); };
  A.ACT.tvkey = function (el) {
    const i = $('.ph-q', this.el), k = el.dataset.k; if (!i) return;
    i.value = k === 'del' ? i.value.slice(0, -1) : k === 'clear' ? '' : i.value + (k === 'space' ? ' ' : k);
    this.palQuery(i.value, $('.ph-res', this.el));
  };
  A.palQuery = function (q, host) {
    const res = host || $('.res', this.q.pal); if (!res) return;
    const { h, items } = this.palResults(q);
    this.palItems = items; this.palSel = 0;
    res.innerHTML = '<i class="sel"></i>' + h;
    $$('.x-poster', res).forEach(e => bindJelly(e, 'card'));
    this.palMark(true);
    /* 面板高度跟着结果走（弹簧） */
    const P = this._palSp;
    if (!host && P && this.q.pal.style.display === 'block') {
      const want = Math.min(480, 58 + res.scrollHeight + 6);
      P.to[3] = want; P.sp[3].cfg(TOKENS.bouncy).target = want;
      if (!P.anim) P.anim = run({ sp: [P.sp[3]], eps: 0.3, apply: () => { this.q.pal.style.height = P.sp[3].v + 'px'; }, done: () => { P.anim = null; } });
    }
  };
  A.palMark = function (snap) {
    const res = $('.res', this.q.pal) || $('.ph-res', this.el); if (!res) return;
    const sel = $('.sel', res), it = $(`.it[data-pi="${this.palSel}"]`, res);
    if (!it) { sel.style.opacity = '0'; return; }
    sel.style.opacity = '';
    if (!this._ps) this._ps = new Spring(it.offsetTop);
    this._ps.cfg(TOKENS.snappy).target = it.offsetTop;
    const apply = () => { sel.style.transform = `translateY(${this._ps.v}px)`; sel.style.top = '0'; };
    if (snap || RM()) { this._ps.snap(); apply(); return; }
    cancel(this._psA); this._psA = run({ sp: [this._ps], eps: 0.3, apply });
  };
  A.palKey = function (e) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); this.palSel = clamp(this.palSel + (e.key === 'ArrowDown' ? 1 : -1), 0, (this.palItems || []).length - 1); this.palMark(); }
    if (e.key === 'Enter' && this.palItems && this.palItems[this.palSel]) { e.preventDefault(); this.palRun(this.palItems[this.palSel]); }
    if (e.key === 'Escape') { e.preventDefault(); this.closeTop(); }
  };
  A.palRun = function (it, srcEl) {
    const panel = this.wide && !this.tv;
    if (panel) this.closeTop();
    const go = () => {
      if (it.k === 'detail') { if (this.wide && this.S.detail != null) this.S.detail = null; this.openDetail(it.aid, srcEl && !panel ? srcEl : null); }
      if (it.k === 'play') this.plOpen({ aid: it.aid, ep: it.ep, img: imgOf(byId(it.aid)) }, null);
      if (it.k === 'set') this.setOpen(it.sec);
      if (it.k === 'op') { if (it.op === 'open') this.q.file.click(); if (it.op === 'new') this.go('new'); if (it.op === 'dark') this.setTh(this.cfg.th === 'dark' ? 'light' : 'dark'); }
    };
    if (panel) setTimeout(go, 220); else go();
  };
  A.palOpen = function (q = '') {
    if (this.ph || this.tv) { this.go('search'); setTimeout(() => { const i = $('.ph-q', this.el); if (i) { i.value = q; this.palQuery(q, $('.ph-res', this.el)); } }, 50); return; }
    if (this.overlays.find(o => o.kind === 'pal')) return;
    const pal = this.q.pal, field = $('.x-search', this.q.tb);
    const from = field ? this.rel(field) : { x: 640, y: 40, w: 10, h: 10 };
    const sr = this.q.side && this.q.side.offsetWidth ? this.rel(this.q.side) : null, side = this.pure() || !sr ? 0 : sr.x + sr.w, w = 640, to = { x: side + (this.W - side - w) / 2, y: this.dev === 'tablet' ? 90 : 70, w, h: 480 };
    pal.innerHTML = `<div class="pin">${I('search')}<input placeholder="搜索番剧、剧集、设置和操作" value="${q}"><kbd style="font:var(--t-cap);color:var(--on3)">esc</kbd></div><div class="res"></div>`;
    const inner = $$('.pin, .res', pal);
    this._palSp = null; pal.style.display = 'block'; pal.style.height = '480px';
    this.palQuery(q);
    const want = Math.min(480, 58 + $('.res', pal).scrollHeight + 6); to.h = want;
    const r0 = parseFloat(getComputedStyle(field || pal).borderRadius) || 16, r1 = parseFloat(getComputedStyle(pal).borderRadius) || 16;
    const sp = ['x', 'y', 'w', 'h', 'r'].map((k, i) => new Spring([from.x, from.y, from.w, from.h, r0][i]));
    const tgt = [to.x, to.y, to.w, to.h, r1];
    this._palSp = { sp, from: [from.x, from.y, from.w, from.h, r0], to: tgt };
    const apply = () => { const [x, y, ww, hh, rr] = sp.map(s => s.v); pal.style.left = x + 'px'; pal.style.top = y + 'px'; pal.style.width = ww + 'px'; pal.style.height = hh + 'px'; pal.style.borderRadius = rr + 'px'; const p = Math.min(1, Math.max(0, (hh - from.h) / (to.h - from.h))); inner.forEach(e => (e.style.opacity = String(Math.max(0, p * 1.6 - 0.5)))); };
    sp.forEach((s, i) => { s.cfg(TOKENS.hero).target = tgt[i]; });
    if (RM()) { sp.forEach(s => s.snap()); apply(); } else run({ sp, eps: 0.2, apply });
    if (field) field.style.visibility = 'hidden';
    this.overlays.push({ kind: 'pal', close: () => this.palClose() });
    this.showDim(true);
    setTimeout(() => { const i = $('input', pal); i.focus({ preventScroll: true }); i.setSelectionRange(i.value.length, i.value.length); }, 60);
  };
  A.palClose = function () {
    const pal = this.q.pal, s = this._palSp, field = $('.x-search', this.q.tb), inner = $$('.pin, .res', pal);
    if (!s || RM()) { pal.style.display = 'none'; if (field) field.style.visibility = ''; return; }
    const from = field ? this.rel(field) : null;
    if (from) { s.from = [from.x, from.y, from.w, from.h, s.from[4]]; }
    s.sp.forEach((x, i) => { x.cfg(TOKENS.snappy).target = s.from[i]; });
    inner.forEach(e => (e.style.opacity = '0'));
    run({ sp: s.sp, eps: 0.3, apply: () => { const [x, y, w, h, r] = s.sp.map(v => v.v); pal.style.left = x + 'px'; pal.style.top = y + 'px'; pal.style.width = w + 'px'; pal.style.height = h + 'px'; pal.style.borderRadius = r + 'px'; }, done: () => { pal.style.display = 'none'; if (field) field.style.visibility = ''; } });
  };
  /* 结果点击（面板和手机搜索页共用） */
  const palClick = function (e) {
    const pi = e.target.closest('[data-pi]'), pa = e.target.closest('[data-pact]');
    if (pi && this.palItems) { this.palRun(this.palItems[+pi.dataset.pi]); return true; }
    if (pa) { this.palRun({ k: 'detail', aid: +pa.dataset.aid }, pa); return true; }
    return false;
  };
  const baseClick = A.onClick;
  A.onClick = function (e) {
    if ((e.target.closest('.x-pal') || e.target.closest('.ph-res')) && palClick.call(this, e)) return;
    baseClick.call(this, e);
  };

  /* =====================================================================
     设置：原生分组列表；可选功能旁边是一扇实时变化的小窗口
     ===================================================================== */
  A.setSecs = function () {
    const s = this.store;
    return [['general', '通用', 'gear'], ['appearance', '外观', 'palette'], ['player', '播放', 'play'],
      s.has('danmaku') && ['danmaku', '弹幕', 'danmaku'], s.has('library') && ['library', '媒体库', 'library'], s.has('anime') && ['anime', '追番', 'calendar'], s.has('sync') && ['sync', '账号同步', 'sync'],
      s.has('sources') && ['sources', '网络媒体源', 'server'], s.has('remote') && ['remote', '局域网与远程', 'wifi'], s.has('downloads') && ['downloads', '下载', 'download'], s.has('enhance') && ['enhance', '画质增强', 'sparkle'],
      '-', ['optional', '可选功能', 'grid'], ['plugins', '插件', 'puzzle'], ['keys', '快捷键', 'keyboard'], ['about', '关于', 'info']].filter(Boolean);
  };
  const row = (t, right = '', sub = '', ic = '', extra = '') => `<div class="c-li"${extra}>${ic ? `<span class="lic">${I(ic)}</span>` : ''}<div class="tx"><b>${t}</b>${sub ? `<span>${sub}</span>` : ''}</div>${right}</div>`;
  const grp = (label, rows) => `${label ? `<div class="c-glabel">${label}</div>` : ''}<div class="c-list">${rows.filter(Boolean).join('')}</div>`;
  const sw = on => `<span class="c-switch${on ? ' on' : ''}"></span>`;
  const dd = v => `<span class="c-btn" data-act="dd" data-menu-anchor>${v}<span class="chev">${I('chev-d')}</span></span>`;
  const sl = v => `<div class="c-slider sm" style="--v:${v};width:150px"><i class="in"></i><i class="ac"></i><i class="th"></i></div>`;
  const v = t => `<span class="v">${t}</span>`;
  A.setBody = function (k) {
    const s = this.store;
    const C = {
      general: () => grp('启动', [row('启动时打开', dd('首页')), row('播放结束后', dd('播放下一集'))]) + grp('语言', [row('界面语言', dd('跟随系统'))]) + grp('更新', [row('自动检查更新', sw(true)), row('更新通道', dd('稳定版'))]),
      appearance: () => grp('主题色', [`<div class="c-li" style="min-height:76px"><div class="tx"><b>主题色</b><span>取自 Erika 图标，传给系统作为强调色</span></div><div class="o-sws">${[['rose', '#FF2E55', '绯红'], ['blue', '#2A51D4', '群青'], ['mint', '#07FF94', '荧绿'], ['pink', '#F48BA6', '缎带粉']].map(([k, c, n]) => `<button data-set-accent="${k}" class="${document.documentElement.dataset.accent === k ? 'on' : ''}"><i style="background:${c};width:30px;height:30px"></i>${n}</button>`).join('')}</div></div>`]) + grp('外观', [row('明暗', `<span class="c-seg"><span>跟随系统</span><span class="${this.cfg.th === 'light' ? 'on' : ''}" data-act="th" data-th="light">浅色</span><span class="${this.cfg.th === 'dark' ? 'on' : ''}" data-act="th" data-th="dark">深色</span></span>`), row('减少动态效果', sw(false), '弹簧动画换成 150ms 淡入淡出')]),
      player: () => grp('播放', [row('默认倍速', dd('1.0×')), row('跳过片头', v('85 秒')), row('记住播放位置', sw(true)), row('自动连播下一集', sw(true))]) + grp('画框', [row('控制层出现时画面退后', sw(true), '关掉后控制层叠在画面底部，用纯色底'), row('画框圆角', v('跟随系统'))]) + grp('解码', [row('硬件解码', sw(true))]),
      danmaku: () => grp('显示', [row('显示弹幕', sw(true)), row('不透明度', sl(0.8)), row('字号', sl(0.45)), row('显示区域', `<span class="c-seg sm"><span>1/4</span><span class="on">半屏</span><span>全屏</span></span>`)]) + grp('来源', [row('弹弹play 自动匹配', sw(true)), row('本地弹幕文件', `<span class="c-btn">挂载…</span>`, 'xml / json')]),
      optional: () => this.optHTML(),
      plugins: () => this.mktHTML(),
      keys: () => grp('播放', [row('播放 / 暂停', v('空格')), row('快退 / 快进 5 秒', v('← / →')), s.has('danmaku') && row('开关弹幕', v('D')), row('全屏', v('F'))]) + grp('全局', [row('打开文件', v('⌘O')), row('搜索', v('⌘K')), row('设置', v('⌘,'))]),
      about: () => `<div style="text-align:center;padding:24px 0 10px"><object type="image/svg+xml" data="brand/erika-icon-animated.svg?${Date.now()}" aria-label="Erika 图标，双击有小动作" style="width:104px;height:104px;display:block;margin:6px auto 0"></object><img class="lg-lt" src="brand/erika-logo.svg?${Date.now()}" alt="ERIKA エリカ" style="height:72px;margin:14px auto 0"><img class="lg-dk" src="brand/erika-logo-dark.svg?${Date.now()}" alt="" style="height:72px;margin:14px auto 0"><div class="x-mut" style="margin-top:6px">Erika 0.2 · 设计原型</div></div>` + grp('', [row('检查更新', `<span class="c-btn">检查</span>`), row('开源许可', v('›')), row('致谢', v('›'))]),
    };
    return (C[k] || (() => grp('', [row('这一节的设置', v('示意'), '模块开启后才会出现在这里'), row('同步与存储', sw(true))])))();
  };
  A.optPreview = function (mods) {
    const pages = NP.pagesFor(mods), has = id => mods.has(id);
    const mw = pages.length ? `<div class="ms">${pages.map((p, i) => `<i class="${i === 0 ? 'on' : ''}" data-pg="${p.key}">${I(p.icon)}${p.name}</i>`).join('')}</div><div class="mb"><div class="ttl"></div><div class="r w"><i></i><i></i><i></i></div><div class="ttl" style="width:30%"></div><div class="r"><i></i><i></i><i></i><i></i><i></i></div></div>` : '<div class="pure"><div>打开文件</div></div>';
    const hud = `<div class="o-hud">${I('play')}<span class="bar"></span>${has('danmaku') ? `<span class="dm">${I('danmaku')}弹幕</span>` : ''}${has('enhance') ? I('sparkle') : ''}${I('cc')}${I('full')}</div>`;
    const near = Object.entries(PRESETS).find(([k, p]) => p.mods && p.mods.length === mods.size && p.mods.every(m => mods.has(m)));
    return `<div class="o-mw">${mw}</div>${hud}<div class="o-sum">已开启 <b style="font-weight:600;color:var(--on)">${mods.size}</b> 个模块${near ? ` · 正好是「${near[1].name}」` : ''}</div>`;
  };
  A.optHTML = function () {
    const m = this.store.mods;
    const groups = ['观看', '内容', '连接'].map(g => grp(g, MODS.filter(x => x.group === g).map(x => row(x.name, `<span class="c-switch${m.has(x.id) ? ' on' : ''}" data-act="modtog" data-mod="${x.id}"></span>`, x.short, x.icon, ` data-mod-row="${x.id}"`)))).join('');
    return `<div class="o-split"><div><p class="x-mut" style="margin:4px 0 0">决定 Erika 要做多少事。关掉的功能不显示，也不占资源。右边的小窗口会跟着变。</p>${groups}</div>
      <div class="o-prev"><div class="c-glabel">预览</div><div class="o-pv">${this.optPreview(m)}</div><div class="o-presets">${['pure', 'anime', 'all'].map(k => `<span class="c-btn" data-act="preset" data-p="${k}">${PRESETS[k].name}</span>`).join('')}</div></div></div>`;
  };
  A.mktCard = function (x) {
    const done = installed.has(x.id);
    return `<div class="m-card"><div class="hd"><span class="lg">${I(x.ic)}</span><div class="nm"><b>${x.n}</b><span>${x.a}</span></div><span class="c-btn m-get${done ? '' : ' p'}" data-act="get" data-pid="${x.id}"><span class="pf"></span><span class="lb">${done ? '打开' : '获取'}</span></span></div><p>${x.d}</p><div class="tags">${x.p.map(t => `<span>${t}</span>`).join('')}${x.req ? `<span>${x.req}</span>` : ''}</div></div>`;
  };
  A.mktHTML = function () {
    return `<div class="m-banner plaid-academy"><b>插件市场</b><span>小功能按需安装。插件用脚本编写，只能使用它声明过的权限。</span></div>
      <div class="m-tools"><span class="c-seg sm"><span class="on">市场</span><span>已安装 ${installed.size}</span></span><span class="grow"></span>${['全部', '弹幕', '画面', '工具', '外观'].map((c, i) => `<span class="c-chip${i ? '' : ' on'}" data-act="mcat" data-c="${c}">${c}</span>`).join('')}</div>
      <div class="m-grid">${PLUGINS.map(x => this.mktCard(x)).join('')}</div>`;
  };
  A.setRender = function (k, anim = true) {
    this.S.set = k;
    const s = this.q.set, side = $('.s-side', s), body = $('.s-body', s), title = $('.s-title', s);
    const secs = this.setSecs();
    side.innerHTML = '<div class="c-field" style="margin:0 2px 10px;height:30px">' + I('search') + '<input placeholder="搜索设置" readonly data-act="pal"></div>' + secs.map(x => x === '-' ? '<div class="x-sep">扩展</div>' : `<div class="c-navi${x[0] === k ? ' on' : ''}" data-act="setsec" data-k="${x[0]}">${I(x[2])}<span>${x[1]}</span></div>`).join('');
    const cur = secs.find(x => x[0] === k); title.textContent = cur ? cur[1] : '';
    body.innerHTML = this.setBody(k);
    this.bindPage(s); $$('.c-navi', side).forEach(n => bindJelly(n, 'M'));
    if (anim) this.animIn(body, 8);
  };
  A.setRefresh = function () {
    if (this.wide && this.overlays.find(o => o.kind === 'set')) { const b = $('.s-body', this.q.set), st = b.scrollTop; this.setRender(this.S.set, false); $('.s-body', this.q.set).scrollTop = st; }
    if (this.ph && this.S.phStack.length && this.S.phStack[this.S.phStack.length - 1].type === 'settings') { const st = this.scr.scrollTop; this.renderPhone(false); this.scr.scrollTop = st; }
  };
  A.setOpen = function (k = 'general') {
    if (this.ph) { this.S.phStack = [{ type: 'settings', sub: null }]; if (k && k !== 'general') this.S.phStack.push({ type: 'settings', sub: k }); this.renderPhone(false, 1); return; }
    if (this.overlays.find(o => o.kind === 'set')) { this.setRender(k); return; }
    /* 桌面、平板：从齿轮弹出的设置窗口；电视：全屏 */
    const s = this.q.set, w = this.tv ? this.W : 1000, h = this.tv ? this.H : this.dev === 'tablet' ? 700 : 660;
    s.style.left = (this.W - w) / 2 + 'px'; s.style.top = (this.H - h) / 2 + (this.dev === 'tablet' ? 12 : 0) + 'px'; s.style.width = w + 'px'; s.style.height = h + 'px';
    s.innerHTML = `<div class="sw"><aside class="s-side"></aside><div class="s-main"><div class="s-tb"><span class="s-title"></span><span class="grow"></span><span class="c-ib" data-act="setclose" title="关闭">${I('x')}</span></div><div class="s-body"></div></div></div>`;
    this.setRender(k, false);
    const g = $('[data-act="settings"]', this.el), gr = g ? this.rel(g) : { x: 640, y: 400, w: 0, h: 0 };
    this.popIn(s, `${gr.x + gr.w / 2 - parseFloat(s.style.left)}px ${gr.y + gr.h / 2 - parseFloat(s.style.top)}px`);
    this.overlays.push({ kind: 'set', close: () => this.popOut(s) });
    this.showDim(true);
  };
  Object.assign(A.ACT, {
    setsec(el) { this.setRender(el.dataset.k); },
    setclose() { this.closeTop(); },
    th(el) { this.setTh(el.dataset.th); $$('[data-act="th"]', el.parentElement).forEach(x => x.classList.toggle('on', x === el)); },
    dd(el) { const cur = el.firstChild.textContent; this.menu(el, ['跟随系统', cur].filter((x, i, a) => a.indexOf(x) === i).map(t => `<div class="c-mi${t === cur ? ' on' : ''}" data-mi="${t}"><span class="ck">${I('check')}</span>${t}</div>`).join(''), 'dd', t => { el.firstChild.textContent = t; this.menuClose(); }); },
    modtog(el) {
      const id = el.dataset.mod, before = new Set(this.store.mods), res = this.store.toggle(id), list = res.added.length ? res.added : res.removed;
      if (list.length) this.toast((res.added.length ? '已同时开启：' : '已同时关闭：') + list.map(x => MOD[x].name).join('、'));
      setTimeout(() => {
        list.concat([id]).forEach(x => { const r = $(`[data-mod-row="${x}"]`, this.el); if (r && x !== id) { r.classList.remove('flash'); void r.offsetWidth; r.classList.add('flash'); } });
        const pv = $('.o-pv', this.el); if (pv) $$('.ms i', pv).forEach(i => { if (!NP.pagesFor(before).find(p => p.key === i.dataset.pg)) NP.enter(i, 'h'); });
      }, 20);
    },
    preset(el) { this.store.set(PRESETS[el.dataset.p].mods); this.toast(`已切换到「${PRESETS[el.dataset.p].name}」`); },
    mcat(el) { $$('[data-act="mcat"]', el.parentElement).forEach(x => x.classList.toggle('on', x === el)); const c = el.dataset.c; const g = $('.m-grid', this.el); g.innerHTML = PLUGINS.filter(x => c === '全部' || x.cat === c).map(x => this.mktCard(x)).join(''); this.bindPage(g); this.stagger($$('.m-card', g), 0, 10, 30); },
    /* 「按钮就是进度」：获取 → 按钮里填满进度 → 变成「打开」 */
    get(el) {
      const id = el.dataset.pid;
      if (installed.has(id)) { this.toast('原型：打开插件设置页'); return; }
      const pf = $('.pf', el), lb = $('.lb', el);
      el.classList.add('ing'); lb.textContent = '安装中';
      tween({ T: 1.4, z: 1 }, p => { pf.style.width = Math.min(100, p * 100) + '%'; }, () => {
        installed.add(id); el.classList.remove('ing', 'p'); pf.style.width = '0'; lb.textContent = '打开'; NP.wobble(el); this.toast('已安装，可以在插件页里调整');
      });
    },
  });

  /* =====================================================================
     首次启动：选一个起点，右边的小窗口跟着变，最后小窗口长成真窗口
     ===================================================================== */
  A.obOpen = function () {
    const o = this.q.ob, desk = this.wide;
    /* 手机和折叠屏：铺满整个屏幕（iPhone Duo 留出右侧的系统栏） */
    const R = { desk: { x: 160, y: 100, w: 960, h: 600 }, tablet: { x: 110, y: 120, w: 960, h: 620 }, tv: { x: 0, y: 0, w: 1280, h: 720 } }[this.dev] || { x: 0, y: 0, w: this.W - (this.rbar ? 64 : 0), h: this.H };
    Object.assign(o.style, { left: R.x + 'px', top: R.y + 'px', width: R.w + 'px', height: R.h + 'px', borderRadius: '' });
    this.ob = { step: 0, mods: new Set(PRESETS.anime.mods), preset: 'anime' };
    o.innerHTML = `<div class="o-steps"></div><div class="o-foot"><span class="c-btn" data-act="obskip">跳过</span><span class="grow"></span><span class="o-dots">${'<i></i>'.repeat(4)}</span><span class="grow"></span><span class="c-btn" data-act="obprev">上一步</span><span class="c-btn p" data-act="obnext">继续</span></div>`;
    if (!desk) o.style.paddingTop = this.rbar ? '16px' : '50px';
    this.obRender();
    this.popIn(o, '50% 50%', TOKENS.hero);
    this.overlays.push({ kind: 'ob', close: () => this.popOut(o) });
    if (desk && !this.tv) this.showDim(true);
    if (this.tv) this.tvEnsure();
  };
  A.obRender = function () {
    /* 折叠屏展开：选项在左半、预览在右半，正好分在折痕两边；悬停用手机的上下排法（预览在上半屏） */
    const o = this.q.ob, st = this.ob.step, desk = this.wide || this.fold === 'open', box = $('.o-steps', o);
    const prev = `<div class="o-pv">${this.optPreview(this.ob.mods)}</div>`;
    const S = [
      () => `<div class="o-welcome"${desk ? '' : ' style="grid-template-columns:1fr;grid-template-rows:220px 1fr"'}><div class="art plaid-academy" style="display:grid;place-items:center"><object type="image/svg+xml" data="brand/erika-icon-animated.svg?${Date.now()}" aria-label="Erika 图标，双击有小动作" style="width:min(46%,200px);height:auto;filter:drop-shadow(0 12px 28px rgba(0,0,0,.35));aspect-ratio:1"></object></div><div class="tx"${desk ? '' : ' style="padding:26px 24px 0"'}><img class="lg-lt" src="brand/erika-logo.svg" alt="ERIKA エリカ"><img class="lg-dk" src="brand/erika-logo-dark.svg" alt=""><h4>欢迎使用 Erika</h4><p>接下来一分钟，选出你需要的功能。没选的功能不会出现，也不占资源，之后随时可以在「设置 › 可选功能」里改。</p></div></div>`,
      () => `<div class="o-step on"${desk ? '' : ' style="padding:20px 20px 0;overflow-y:auto"'}><div class="o-two"${desk ? '' : ' style="grid-template-columns:1fr;gap:16px"'}><div><div class="o-h">选一个起点</div><p class="o-sub">右边的小窗口会跟着变。之后可以在设置里逐项调整。</p><div class="o-pre">${['pure', 'anime', 'all', 'custom'].map(k => `<div class="o-card${this.ob.preset === k ? ' on' : ''}" data-act="obpre" data-p="${k}"><span class="ck">${I('check')}</span><b>${PRESETS[k].name}${k === 'anime' ? '<span class="hl">推荐</span>' : ''}</b><span class="ds">${PRESETS[k].desc}</span></div>`).join('')}</div><div class="c-glabel" style="margin-left:0">微调 · 点一下开关单个模块</div><div class="ob-mods" style="display:flex;flex-wrap:wrap;gap:8px">${this.obChips()}</div></div><div${desk ? '' : ' style="order:-1"'}>${prev}</div></div></div>`,
      () => `<div class="o-step on"${desk ? '' : ' style="padding:20px 20px 0"'}><div class="o-two"${desk ? '' : ' style="grid-template-columns:1fr;gap:16px"'}><div><div class="o-h">外观</div><p class="o-sub">主题色取自图标，传给系统作为强调色。控件的样子跟随系统。</p><div class="c-glabel" style="margin-left:0">主题色</div><div class="o-sws">${[['rose', '#FF2E55', '绯红'], ['blue', '#2A51D4', '群青'], ['mint', '#07FF94', '荧绿'], ['pink', '#F48BA6', '缎带粉']].map(([k, c, n]) => `<button data-set-accent="${k}" class="${document.documentElement.dataset.accent === k ? 'on' : ''}"><i style="background:${c}"></i>${n}</button>`).join('')}</div><div class="c-glabel" style="margin-left:0">明暗</div><span class="c-seg"><span>跟随系统</span><span class="${this.cfg.th === 'light' ? 'on' : ''}" data-act="th" data-th="light">浅色</span><span class="${this.cfg.th === 'dark' ? 'on' : ''}" data-act="th" data-th="dark">深色</span></span></div><div${desk ? '' : ' style="order:-1"'}>${prev}</div></div></div>`,
      () => `<div class="o-step on" style="display:flex;flex-direction:column;align-items:center;justify-content:center;text-align:center;padding-top:0"><div style="width:72px;height:72px;border-radius:50%;background:var(--acc);color:var(--on-acc);display:grid;place-items:center;margin-bottom:18px" class="ob-ok">${I('check', 'width:36px;height:36px;stroke-width:2.6')}</div><div class="o-h">准备好了</div><p class="o-sub" style="max-width:420px">开启了 ${this.ob.mods.size} 个模块。点「开始使用」，右边那扇小窗口会长成你的 Erika。</p><div style="width:300px">${prev}</div></div>`,
    ];
    box.innerHTML = S[st]();
    this.bindPage(box);
    $$('.o-dots i', o).forEach((d, i) => d.classList.toggle('on', i === st));
    $('[data-act="obprev"]', o).style.visibility = st ? '' : 'hidden';
    $('[data-act="obnext"]', o).innerHTML = st === 3 ? '开始使用' : st === 0 ? '开始' : '继续';
    $('[data-act="obskip"]', o).style.visibility = st === 3 ? 'hidden' : '';
    if (st === 3) { const ok = $('.ob-ok', o); ok && !RM() && tween(TOKENS.jelly, p => { ok.style.transform = `scale(${0.4 + 0.6 * p})`; }); }
    else this.animIn(box, 10);
  };
  A.obChips = function () { return MODS.map(m => { const on = this.ob.mods.has(m.id); return `<span class="c-chip${on ? ' on' : ''}" data-act="obmod" data-mod="${m.id}">${on ? I('check') : I('plus')}${m.name}</span>`; }).join(''); };
  A.obSync = function (bounce) {
    const o = this.q.ob, near = Object.entries(PRESETS).find(([k, p]) => p.mods && p.mods.length === this.ob.mods.size && p.mods.every(m => this.ob.mods.has(m)));
    this.ob.preset = near ? near[0] : 'custom';
    $$('.o-card', o).forEach(c => c.classList.toggle('on', c.dataset.p === this.ob.preset));
    const ch = $('.ob-mods', o); if (ch) { ch.innerHTML = this.obChips(); this.bindPage(ch); }
    const pv = $('.o-pv', o), before = new Set($$('.ms i', pv).map(i => i.dataset.pg));
    pv.innerHTML = this.optPreview(this.ob.mods);
    $$('.ms i', pv).forEach(i => { if (!before.has(i.dataset.pg)) NP.enter(i, 'h'); });
    if (bounce) NP.wobble($('.o-mw', pv));
  };
  /* 完成：引导窗口长成主窗口 */
  A.obFinish = function () {
    const o = this.q.ob, mods = [...this.ob.mods];
    this.overlays = this.overlays.filter(x => x.kind !== 'ob');
    this.store.set(mods);
    if (RM()) { o.style.display = 'none'; this.showDim(false); return; }
    const pv = $('.o-mw', o), a = pv ? this.rel(pv) : null;
    o.style.display = 'none'; this.showDim(false);
    if (!a) return;
    const c = document.createElement('div'); c.className = 'x-fly'; c.style.background = 'var(--bg)'; c.style.boxShadow = '0 30px 80px -20px rgba(0,0,0,.5)';
    this.el.appendChild(c);
    const b = { x: 0, y: 0, w: this.W, h: this.H }, L = (u, v, p) => u + (v - u) * p, r0 = this.rad('--r-l');
    const shell = $$('.x-side, .x-main, .x-pbody, .c-tabbar, .x-rbar', this.el); shell.forEach(e => (e.style.opacity = '0'));
    tween(TOKENS.hero, p => { c.style.width = L(a.w, b.w, p) + 'px'; c.style.height = L(a.h, b.h, p) + 'px'; c.style.transform = `translate(${L(a.x, b.x, p)}px, ${L(a.y, b.y, p)}px)`; c.style.borderRadius = L(r0, 0, p) + 'px'; },
      () => { shell.forEach(e => (e.style.opacity = '')); this.stagger(shell, 0, 8, 60); c.remove(); });
  };
  Object.assign(A.ACT, {
    obnext() { if (this.ob.step === 3) { this.obFinish(); return; } this.ob.step++; this.obRender(); },
    obprev() { this.ob.step = Math.max(0, this.ob.step - 1); this.obRender(); },
    obskip() { this.ob.mods = new Set(PRESETS.pure.mods); this.ob.step = 3; this.obRender(); },
    obpre(el) {
      const k = el.dataset.p;
      if (PRESETS[k].mods) this.ob.mods = new Set(PRESETS[k].mods);
      this.obSync(true);
      if (k === 'custom') { $$('.o-card', this.q.ob).forEach(c => c.classList.toggle('on', c === el)); const ch = $('.ob-mods', this.q.ob); ch && this.stagger($$('.c-chip', ch), 0, 6, 24); }
    },
    obmod(el) {
      const id = el.dataset.mod, m = this.ob.mods;
      if (m.has(id)) { const rm = x => { m.delete(x); MODS.filter(d => d.deps.includes(x) && m.has(d.id)).forEach(d => rm(d.id)); }; rm(id); }
      else { const add = x => { m.add(x); MOD[x].deps.forEach(d => !m.has(d) && add(d)); }; add(id); }
      this.obSync(false); NP.wobble($(`.ob-mods [data-mod="${id}"]`, this.q.ob));
    },
  });

  /* =====================================================================
     手机
     ===================================================================== */
  A.renderPhone = function (animate = true, dir = 0) {
    if (this.land) return;
    const pages = this.pages(), pure = !pages.length;
    const tabs = pure ? [] : pages.concat([{ key: 'search', name: '搜索', icon: 'search' }]);
    if (!pure && !tabs.find(t => t.key === this.S.page)) this.S.page = tabs[0].key;
    this.q.tabs.style.display = pure ? 'none' : '';
    const top = this.S.phStack[this.S.phStack.length - 1];
    /* 两栏时左栏一直是当前标签页，标签保持选中（设置除外，设置占了左栏） */
    const tabOn = t => t.key === this.S.page && (!top || (this.twin && top.type !== 'settings'));
    this.q.tabs.innerHTML = tabs.map(t => `<div class="c-tabi${tabOn(t) ? ' on' : ''}" data-act="tab" data-tab="${t.key}">${I(t.icon)}<span>${t.name}</span></div>`).join('');
    $$('.c-tabi', this.q.tabs).forEach(t => bindJelly(t, 'S'));
    if (this.rbar) this.renderRbar(tabs, tabOn);
    const scr = document.createElement('div'); scr.className = 'x-pscr';
    this.S.detail = top && top.type === 'detail' ? top.aid : null;
    let types, lk = null;
    if (this.twin) {
      /* 折叠屏展开：左栏 = 下面一层，右栏 = 最上面一层；什么都没点开时右栏是画框（继续观看 / 纯粹模式的空画框） */
      let L, R;
      if (top && top.type === 'settings') { const sub = top.sub || 'optional'; L = ['settings', { type: 'settings', sub: null }]; R = ['settings', { type: 'settings', sub }]; }
      else { L = [pure ? 'idle' : this.S.page, null]; R = top ? [top.type, top] : ['side', null]; }
      lk = L[0];
      scr.className = 'x-pscr x-twin';
      scr.innerHTML = `<div class="x-tc l">${this.phHTML(L[0], L[1], 'l')}</div><div class="x-tc r">${R[0] === 'side' ? this.sideHTML(pure) : this.phHTML(R[0], R[1], 'r')}</div>`;
      if (R[0] === 'settings') $$('.x-tc.l [data-act="phsetsub"]', scr).forEach(r => r.classList.toggle('on', r.dataset.k === R[1].sub));
      types = [L[0], R[0] === 'side' && pure ? 'idle' : R[0]];
    } else { const type = top ? top.type : pure ? 'idle' : this.S.page; scr.innerHTML = this.phHTML(type, top); types = [type]; }
    const old = this.scr; this.scr = scr; scr._lk = lk;
    const keepL = old && lk && old._lk === lk ? $('.x-tc.l', old).scrollTop : null;
    this.q.body.appendChild(scr);
    if (keepL != null) $('.x-tc.l', scr).scrollTop = keepL;
    /* iPhone Duo：页面自己的工具按钮（媒体库的管理、排序，下载的添加任务）也搬进右侧竖栏，排在全局操作上面 */
    if (this.rbar) {
      const box = $('.rb-page', this.q.rbar), acts = $$('.x-pnav .c-ib', scr).filter(b => !['open', 'phset', 'phback'].includes(b.dataset.act));
      if (box) { box.innerHTML = acts.map(b => b.outerHTML).join(''); $$('.c-ib', box).forEach(b => bindJelly(b, 'S')); }
      /* 页面自己已经有「＋」（下载的添加任务）时，全局的「打开文件」换成文件夹图标，免得两个一样的按钮 */
      const op = $('.rb-open', this.q.rbar); if (op) op.innerHTML = I(acts.some(b => b.dataset.act === 'dladd') ? 'folder' : 'plus');
    }
    this.bindPage(scr);
    const AF = { home: this.after_phhome, new: this.after_phnew, lib: this.after_lib, detail: this.after_detail, idle: this.after_phidle };
    [...new Set(types)].forEach(t => { const f = AF[t]; f && f.call(this, animate); });
    if (old) {
      if (dir && !RM() && keepL != null) {
        /* 两栏：左栏不动，只有右栏的新内容滑进来 */
        old.remove();
        const r = $('.x-tc.r', scr), dx = this.fold === 'tent' ? 0 : dir * 40, dy = this.fold === 'tent' ? dir * 24 : 0;
        r.style.opacity = '0'; r.style.transform = `translate(${dx}px, ${dy}px)`;
        tween(TOKENS.smooth, p => { r.style.opacity = String(Math.min(1, p * 1.5)); r.style.transform = `translate(${dx * (1 - p)}px, ${dy * (1 - p)}px)`; }, () => { r.style.transform = ''; r.style.opacity = ''; });
      } else if (dir && !RM()) {
        scr.style.transform = `translateX(${dir * 100}%)`;
        tween(TOKENS.smooth, p => { const q = Math.min(1, p); scr.style.transform = `translateX(${dir * 100 * (1 - p)}%)`; old.style.transform = `translateX(${-dir * 30 * q}%)`; old.style.opacity = String(1 - q * 0.6); }, () => { old.remove(); scr.style.transform = ''; });
      } else { old.remove(); if (animate) this.animIn(scr, 8); }
    }
    this.renderMini();
  };
  /* 两栏右栏的默认内容：画框。有模块时是「继续观看」的舞台，纯粹模式是空画框 */
  A.sideHTML = function (pure) {
    if (pure) return `<div class="x-pnav"><span></span></div>
      <div class="i-frame" style="position:relative;height:auto;aspect-ratio:16/9;margin:40px 0 0;padding:12px"><span class="big" style="width:40px;height:40px">${I('film', 'width:40px;height:40px;stroke-width:1.4')}</span><h4 style="font:var(--t-h2)">打开一个视频</h4><p style="font:var(--t-cap)">从「文件」App、相册或链接打开</p><img class="fill" src="${STILL}" alt=""></div>
      <span class="c-btn p L" data-act="open" style="width:100%;margin-top:18px;box-sizing:border-box">${I('folder')}打开文件</span>`;
    return `<div class="x-pnav"><span></span></div>${this.phStage()}`;
  };
  /* iPhone Duo：右侧竖排的系统栏 + 工具栏——返回在最上面，然后是标签，常用操作在下面，中间隔开 */
  A.renderRbar = function (tabs, tabOn) {
    const r = this.q.rbar; if (!r) return;
    r.innerHTML = `<div class="rb-sys"><b>9:41</b><i class="rb-island"></i></div>
      ${this.S.phStack.length ? `<span class="c-ib rb-back" data-act="phback" title="返回">${I('chev-l')}</span>` : ''}
      ${tabs.length ? `<nav class="rb-tabs">${tabs.map(t => `<div class="c-tabi${tabOn(t) ? ' on' : ''}" data-act="tab" data-tab="${t.key}" title="${t.name}">${I(t.icon)}</div>`).join('')}</nav>` : ''}
      <span class="grow"></span>
      <span class="rb-page"></span>
      <span class="c-ib rb-open" data-act="open" title="打开文件">${I('plus')}</span><span class="c-ib" data-act="phset" title="设置">${I('gear')}</span>
      <div class="rb-bat"><i></i></div>`;
    $$('.c-ib, .c-tabi', r).forEach(b => bindJelly(b, 'S'));
  };
  A.phStage = function () {
    const c0 = this.cw[0], a0 = byId(c0.id);
    return `<div class="ph-stage"><div class="h-frame" data-act="cwplay" data-i="0"><img src="${c0.img}" alt="" style="object-position:${c0.pos}"><div class="x-prog"><b style="width:${c0.prog * 100}%"></b></div><span class="h-hint" style="opacity:1;transform:none">${I('play')}</span></div>
          <div class="x-kick" style="margin-top:14px">继续观看</div><div class="h-title">${a0.t}</div><div class="h-meta"><span class="hl">第 ${c0.ep} 集</span> · 剩 ${c0.left} 分钟</div>
          <div class="ph-two" style="margin-top:14px"><span class="c-btn p L" data-act="cwplay" data-i="0">${I('play')}继续播放</span>${a0.id ? `<span class="c-btn L" data-act="detail" data-aid="${a0.id}">详情</span>` : ''}</div></div>`;
  };
  /* col：两栏时这一页在哪一栏（'l' / 'r'），单栏是 undefined */
  A.phHTML = function (type, top, col) {
    const avatar = `<span class="x-av" data-act="phset" style="cursor:pointer;width:34px;height:34px"></span>`;
    /* 右栏的详情用「关闭」代替「返回」（左栏还在）；右栏的设置子页不需要返回（左栏就是目录） */
    const back = l => (col === 'r' ? (type === 'settings' ? '' : `<div class="x-pnav"><span></span><span class="c-ib x-pclose" data-act="phback" title="关闭">${I('x')}</span></div>`) : `<div class="x-pnav"><span class="x-pback" data-act="phback">${I('chev-l')}${l}</span></div>`);
    if (type === 'home') {
      /* 两栏时舞台在右栏（画框那一半），左栏从「也在看」开始 */
      return `<div class="x-pnav"><div class="x-lt">首页</div><span style="display:flex;gap:10px;align-items:center"><span class="c-ib" data-act="open" title="打开文件">${I('plus')}</span>${avatar}</span></div>
        ${col === 'l' ? '' : this.phStage()}
        <div class="x-sechd"${col === 'l' ? ' style="margin-top:6px"' : ''}><span class="x-h2">也在看</span><span class="x-mut" style="font:var(--t-cap)">${col === 'l' ? '点一下换到右边的画框' : '点一下换上舞台'}</span></div>
        <div class="x-hrow ph-cards">${this.cw.slice(1).map((c, i) => { const a = byId(c.id); return `<div data-act="phswap" data-i="${i + 1}"><div class="x-thumb nojelly"><img src="${c.img}" alt="" style="object-position:${c.pos}"><div class="x-prog"><b style="width:${c.prog * 100}%"></b></div><span class="ph-pl" data-act="cwplay" data-i="${i + 1}" title="直接播放">${I('play')}</span></div><b>${a.t}</b><span>第 ${c.ep} 集 · 剩 ${c.left} 分钟</span></div>`; }).join('')}</div>
        ${this.store.has('anime') ? `<div class="x-sechd"><span class="x-h2">今日更新</span><span class="x-more" data-act="tab" data-tab="new">新番表 ›</span></div><div class="x-hrow ph-tlw">${this.timelineHTML(true)}</div>` : ''}
        ${this.store.has('library') ? `<div class="x-sechd"><span class="x-h2">最近添加</span><span class="x-more" data-act="tab" data-tab="lib">全部 ›</span></div><div class="x-hrow">${[1, 2, 9, 10, 12, 6].map(id => `<div style="width:108px;flex:none">${pcard(byId(id))}</div>`).join('')}</div>` : ''}`;
    }
    if (type === 'new') return `<div class="x-pnav"><div class="x-lt">新番</div></div><div class="x-mut" style="margin:-4px 0 14px">2026 年 10 月 · 秋季新番</div><div class="n-vacc">${WEEK.map((l, i) => `<div class="n-vrow${i === this.S.newDay ? ' on' : ''}" data-act="pday" data-d="${i}"><div class="n-vhd"><b>${DAYS[i]}</b><span class="cnt">${i === TODAY_IDX ? '<span class="hl">今天</span> · ' : ''}${l.length} 部</span><div class="mini">${l.slice(0, 4).map(x => `<div class="x-poster"><img src="${imgOf(x.a)}" alt=""></div>`).join('')}${l.length > 4 ? `<span class="n-more">+${l.length - 4}</span>` : ''}</div></div><div class="n-vbody">${this.newRows(l, i)}</div></div>`).join('')}</div>`;
    if (type === 'lib') return this.libHTML(true);
    if (type === 'dl') return this.dlHTML(true);
    if (type === 'search') return `<div class="x-pnav"><div class="x-lt">搜索</div></div><label class="c-field x-search" style="margin:4px 0 8px">${I('search')}<input class="ph-q" placeholder="番剧、剧集、设置"></label><div class="ph-res x-pal" style="position:relative;display:block;background:none;box-shadow:none;backdrop-filter:none;-webkit-backdrop-filter:none"><div class="res" style="max-height:none;padding:0"></div></div>`;
    if (type === 'detail') return back({ home: '首页', new: '新番', lib: '媒体库', search: '搜索' }[this.S.page] || '返回') + `<div style="position:relative">${this.detailHTML(byId(top.aid), true)}</div>`;
    if (type === 'settings') {
      if (!top.sub) return back('返回') + `<div class="x-lt" style="margin-top:-4px">设置</div><label class="c-field x-search" data-act="pal" style="margin:0 0 4px">${I('search')}<input placeholder="搜索设置" readonly data-act="pal"></label>` + grp('', [`<div class="c-li ph-acct"><span class="x-av"></span><div class="tx"><b>Nipa</b><span>${this.store.has('sync') ? '弹弹play · Bangumi 已登录' : '本地用户'}</span></div></div>`]) + grp('', this.setSecs().filter(x => x !== '-').map(x => `<div class="c-li" data-act="phsetsub" data-k="${x[0]}" style="cursor:pointer"><span class="lic">${I(x[2])}</span><div class="tx"><b>${x[1]}</b></div><span class="v">›</span></div>`));
      const t = (this.setSecs().find(x => x[0] === top.sub) || [0, ''])[1];
      if (top.sub === 'optional') return back('设置') + `<div class="x-lt" style="margin-top:-4px">${t}</div><div class="o-pv" style="margin-bottom:6px">${this.optPreview(this.store.mods)}</div><div class="o-presets" style="margin:0 0 4px">${['pure', 'anime', 'all'].map(k => `<span class="c-btn" data-act="preset" data-p="${k}">${PRESETS[k].name}</span>`).join('')}</div>` + ['观看', '内容', '连接'].map(g => grp(g, MODS.filter(x => x.group === g).map(x => row(x.name, `<span class="c-switch${this.store.has(x.id) ? ' on' : ''}" data-act="modtog" data-mod="${x.id}"></span>`, x.short, x.icon, ` data-mod-row="${x.id}"`)))).join('');
      if (top.sub === 'plugins') return back('设置') + `<div class="x-lt" style="margin-top:-4px">${t}</div>` + this.mktHTML().replace('class="m-grid"', 'class="m-grid" style="grid-template-columns:1fr"');
      return back('设置') + `<div class="x-lt" style="margin-top:-4px">${t}</div>` + this.setBody(top.sub);
    }
    if (type === 'idle') return `<div class="x-pnav"><span></span><span class="c-ib" data-act="phset" title="设置">${I('gear')}</span></div>
      ${col === 'l' ? '<div class="x-lt" style="margin-top:28px">打开</div>' : `<div class="i-frame" style="position:relative;height:auto;aspect-ratio:16/9;margin:70px 0 0;padding:12px"><span class="big" style="width:40px;height:40px">${I('film', 'width:40px;height:40px;stroke-width:1.4')}</span><h4 style="font:var(--t-h2)">打开一个视频</h4><p style="font:var(--t-cap)">从「文件」App、相册或链接打开</p><img class="fill" src="${STILL}" alt=""></div>
      <span class="c-btn p L" data-act="open" style="width:100%;margin-top:18px;box-sizing:border-box">${I('folder')}打开文件</span>`}
      <div class="ph-two"><span class="c-btn" data-act="open">打开文件夹</span><span class="c-btn" data-act="openlink">打开链接</span></div>
      <span class="c-btn" data-act="recent" data-i="0" style="width:100%;margin-top:10px;box-sizing:border-box">${I('play')}继续上次</span>
      <p class="x-mut" style="font:var(--t-cap);margin:10px 2px 0">在别的 App 里选「打开方式 › Erika」也能直接播放（对应电脑上把文件拖进窗口）。</p>
      <div class="c-glabel">最近播放</div><div class="c-list">${RECENTS.map((r, i) => `<div class="c-li" data-act="recent" data-i="${i}" style="cursor:pointer"><div class="x-thumb nojelly" style="width:72px;border-radius:6px"><img src="${r.img}" alt=""><div class="x-prog" style="height:3px"><b style="width:${r.prog * 100}%"></b></div></div><div class="tx"><b style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis;font-size:13px">${r.t}</b><span>剩 ${r.left}</span></div></div>`).join('')}</div>`;
    return '';
  };
  A.after_phhome = function (animate) {
    const w = $('.ph-tlw', this.scr); if (w) requestAnimationFrame(() => { w.scrollLeft = 900 * 0.5 - w.clientWidth / 2; });
    if (animate) this.tlIn(this.scr);
  };
  A.after_phidle = function (animate) {
    if (animate) { const f = $('.i-frame', this.scr); f && NP.wobble(f); }
  };
  A.after_phnew = function () { this.phNewLayout(true); };
  A.phNewLayout = function (snap) {
    const rows = $$('.n-vrow', this.scr);
    if (!this._pn || this._pn.length !== rows.length || snap) this._pn = rows.map(() => new Spring(64));
    rows.forEach((r, i) => { const on = i === this.S.newDay; this._pn[i].cfg(TOKENS.bouncy).target = on ? 64 + $('.n-vbody', r).scrollHeight : 64; });
    const apply = () => rows.forEach((r, i) => { r.style.height = this._pn[i].v.toFixed(1) + 'px'; });
    if (snap || RM()) { this._pn.forEach(s => s.snap()); apply(); return; }
    cancel(this._pnA); this._pnA = run({ sp: this._pn, eps: 0.3, apply });
  };
  Object.assign(A.ACT, {
    tab(el) { const t = el.dataset.tab; if (this.wide) { this.go(t); return; } this.S.phStack = []; this.S.page = t; this.renderPhone(true); },
    phback() {
      const st = this.S.phStack, top = st.pop();
      /* 两栏时设置的目录和子页同时在屏上，返回 = 整个退出设置 */
      if (this.twin && top && top.type === 'settings') while (st.length && st[st.length - 1].type === 'settings') st.pop();
      if (top && top.type === 'detail') this.S.detail = null;
      this.renderPhone(false, -1);
    },
    phset() { this.setOpen(); },
    phsetsub(el) {
      const st = this.S.phStack, top = st[st.length - 1];
      if (this.twin && top && top.type === 'settings' && top.sub) st.pop();
      st.push({ type: 'settings', sub: el.dataset.k }); this.renderPhone(false, 1);
    },
    /* 手机「也在看」：点卡片换上舞台（和桌面一样），卡片上的 ▶ 直接播放 */
    phswap(el) {
      const i = +el.dataset.i, th = $('.x-thumb', el), fr = $('.ph-stage .h-frame', this.scr);
      const a = this.rel(th), b = this.rel(fr), c0 = this.cw[0], ci = this.cw[i];
      const go = () => { this.cw[0] = ci; this.cw[i] = c0; const st = this.scr.scrollTop; this.renderPhone(false); this.scr.scrollTop = st; this.stagger($$('.ph-stage > *', this.scr), 0, 8, 26); };
      if (RM()) { go(); return; }
      $('img', th).style.visibility = 'hidden';
      this.fly(ci.img, a, b, this.rad('--r-m'), this.rad('--r-card'), go);
    },
    pday(el) { const d = +el.dataset.d; if (d === this.S.newDay) return; this.S.newDay = d; $$('.n-vrow', this.scr).forEach((r, i) => r.classList.toggle('on', i === d)); this.phNewLayout(false); setTimeout(() => this.stagger($$('.n-row', $$('.n-vrow', this.scr)[d]), 0, 8, 26), 80); },
  });
})();
