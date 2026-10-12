/* =====================================================================
   NipaPlay Next · 设计文档交互（共享部分在 core.js）
   ===================================================================== */
(() => {
  'use strict';

  const {
    $, $$, svg, kv, root, TOKENS, DEFAULTS, RM, Spring, run, tween, cancel, scaleTo, bindJelly,
    enter, leave, setShown, toast, setTheme, scaleOf, MODS, MOD, ALL, MiniWin, watchFrames, autoJelly,
  } = NP;
  const Store = NP.createStore(ALL);

  /* ------------------------------------------------------------------
     功能分层 · 模块列表
     ------------------------------------------------------------------ */
  function initTier() {
    const ul = $('#tierMods'); if (!ul) return;
    ul.innerHTML = MODS.map(m => `<li>${svg(m.icon)}<span>${m.name}</span>${m.deps.length ? `<span class="tiny faint" style="margin-left:auto">需要${m.deps.map(d => MOD[d].name).join('、')}</span>` : ''}</li>`).join('');
  }

  /* ------------------------------------------------------------------
     导航生长示意
     ------------------------------------------------------------------ */
  function initNavGrowth() {
    const host = $('#navGrowth'); if (!host) return;
    const cases = [
      { mods: [], t: '0 个页面', d: '纯粹播放：没有导航，窗口就是播放器' },
      { mods: ['library', 'danmaku'], t: '1 个页面', d: '只开媒体库：没有侧栏，工具栏里只有窗口按钮和搜索' },
      { mods: ['danmaku', 'library', 'anime'], t: '3 个页面', d: '看番：出现侧栏' },
      { mods: ALL, t: '4 个页面 +', d: '全都要：再加下载，以及插件提供的页面' },
    ];
    host.innerHTML = cases.map(c => `<div><div class="frame-wrap" style="margin:0"><div class="frame" data-w="440" data-h="275"></div></div><div style="margin-top:10px"><b class="small">${c.t}</b><div class="tiny muted">${c.d}</div></div></div>`).join('');
    $$('.frame', host).forEach((f, i) => new MiniWin(f).update(new Set(cases[i].mods), false));
  }

  /* ------------------------------------------------------------------
     主窗口侧栏（静态稿）
     ------------------------------------------------------------------ */
  function renderMainSides() {
    const items = [['home', '首页', 'home'], ['new', '新番', 'calendar'], ['lib', '媒体库', 'library'], ['dl', '下载', 'download']];
    $$('.wf-side[data-side]').forEach(el => {
      const sel = el.dataset.side;
      el.innerHTML = '<div class="dots3"><i></i><i></i><i></i></div>' +
        items.map(([k, n, ic]) => `<div class="nav-item${k === sel ? ' on' : ''}"${k === sel ? ' data-spec="侧栏项 · 高 34 · 侧栏宽 220"' : ''}>${svg(ic)}<span>${n}</span>${k === 'dl' ? '<span style="margin-left:auto;font-size:11px;color:var(--text-3)">2</span>' : ''}</div>`).join('') +
        `<div class="nav-label">插件</div><div class="nav-item">${svg('puzzle')}<span>插件页面（示例）</span></div>` +
        (el.dataset.mini
          ? `<div class="mini-player" data-spec="迷你播放器 · 196×56"><div class="thumb"><img src="assets/still1.jpg" alt=""></div><div class="tx"><b>只有我不存在的城市</b><span class="cap2">第 1 集 · 09:12</span><div class="line"><b></b></div></div><span class="ib" style="width:26px;height:26px">${svg('pause')}</span></div>`
          : '<div style="margin-top:auto"></div>') +
        `<div class="side-foot"><span class="avatar" style="background-image:url(assets/girl.png);background-size:170%;background-position:50% 12%"></span><div style="flex:1;line-height:1.25"><div style="font-size:13px;font-weight:650">Nipa</div><div class="cap2" style="font-size:11px">弹弹play · Bangumi</div></div><span class="ib" data-spec="设置 · S">${svg('gear')}</span></div>`;
    });
  }

  /* ------------------------------------------------------------------
     播放器
     ------------------------------------------------------------------ */
  function initPlayer() {
    const pl = $('#plA'); if (!pl) return;
    const lines = [['前方高能！！', ''], ['名场面来了', ''], ['这段作画太顶了', '#FFD24A'], ['OP 好听', ''], ['再上映——', ''], ['雏月！！', '#8EE3FF'], ['泪目', ''], ['这里有伏笔', ''], ['下周见', ''], ['悟，快跑', '#FFB3C7']];
    [['#dmLayer', 9, 40, 22], ['#dmLayer2', 5, 34, 17]].forEach(([sel, rows, gap, fs]) => {
      const L = $(sel); if (!L) return;
      L.innerHTML = lines.map(([t, c], i) => {
        const dur = 11 + ((i * 7) % 6), delay = -((i * 3.7) % dur);
        return `<span style="top:${(i % rows) * gap}px;animation-duration:${dur}s;animation-delay:${delay}s;font-size:${fs}px${c ? ';color:' + c : ''}">${t}</span>`;
      }).join('');
    });
    const w = 1240, h = 22;
    let d = `M0 ${h}`;
    for (let x = 0; x <= w; x += 8) {
      const v = 0.22 + 0.16 * Math.sin(x / 53) + 0.14 * Math.sin(x / 17 + 1) + 0.5 * Math.exp(-(((x - 820) / 60) ** 2)) + 0.3 * Math.exp(-(((x - 300) / 40) ** 2));
      d += ` L${x} ${(h - Math.max(0.06, Math.min(1, v)) * h).toFixed(1)}`;
    }
    $('#densPath').setAttribute('d', d + ` L${w} ${h} Z`);

    const pop = $('#dmPop'), btn = $('#dmBtn'), panel = $('#plPanel'), layer = $('#dmLayer');
    let popPinned = false, popOpen = false, hoverT = 0;
    function placePop() {
      const s = scaleOf(pl), r = btn.getBoundingClientRect(), P = pl.getBoundingClientRect();
      pop.style.left = ((r.left - P.left) / s + r.width / (2 * s) - 150) + 'px';
    }
    function showPop(on) {
      if (on === popOpen) return;
      popOpen = on;
      if (on) {
        if (!Store.has('danmaku')) { popOpen = false; return; }
        pop.style.display = ''; placePop(); pop.style.transformOrigin = '50% 100%';
        cancel(pop._tw);
        pop._tw = tween(TOKENS.bouncy, p => { pop.style.transform = `scale(${0.9 + 0.1 * p})`; pop.style.opacity = String(Math.min(1, p * 2)); });
      } else {
        cancel(pop._tw);
        pop._tw = tween(TOKENS.snappy, p => { pop.style.transform = `scale(${1 - 0.06 * p})`; pop.style.opacity = String(Math.max(0, 1 - p)); }, () => { if (!popOpen) pop.style.display = 'none'; });
      }
    }
    btn.addEventListener('pointerenter', e => { if (e.pointerType === 'mouse') { clearTimeout(hoverT); showPop(true); } });
    [btn, pop].forEach(el => el.addEventListener('pointerleave', () => { clearTimeout(hoverT); hoverT = setTimeout(() => { if (!popPinned) showPop(false); }, 260); }));
    pop.addEventListener('pointerenter', () => clearTimeout(hoverT));
    btn.addEventListener('click', () => {
      const on = !btn.classList.contains('on');
      btn.classList.toggle('on', on);
      btn.innerHTML = svg(on ? 'danmaku' : 'danmaku-off');
      setShown(layer, on && Store.has('danmaku'), null);
    });
    const tPop = $('#tPop'), tPanel = $('#tPanel');
    tPop.closest('label').addEventListener('click', () => { popPinned = !popPinned; tPop.classList.toggle('on', popPinned); showPop(popPinned); });
    let panelOpen = false;
    tPanel.closest('label').addEventListener('click', () => {
      panelOpen = !panelOpen; tPanel.classList.toggle('on', panelOpen);
      cancel(panel._tw);
      if (panelOpen) {
        panel.style.display = '';
        panel._tw = tween(TOKENS.bouncy, p => { panel.style.transform = `translateX(${(1 - p) * 100}%)`; });
      } else {
        panel._tw = tween(TOKENS.smooth, p => { panel.style.transform = `translateX(${Math.min(1, p) * 100}%)`; }, () => { if (!panelOpen) panel.style.display = 'none'; });
      }
    });

    function sync(animate) {
      $$('.player [data-mod]').forEach(el => {
        const show = el.dataset.mod.split('+').every(r => Store.has(r)) && !(el === layer && !btn.classList.contains('on'));
        const axis = el.classList.contains('dm-input') || el.classList.contains('pib') ? 'w' : null;
        setShown(el, show, axis, animate);
      });
      const tabs = $$('#plTabs span');
      tabs.forEach(s => s.classList.remove('on'));
      (Store.has('danmaku') ? tabs[0] : tabs[1]).classList.add('on');
      $('#plSub').style.display = Store.has('danmaku') ? 'none' : '';
      if (!Store.has('danmaku')) { popPinned = false; tPop.classList.remove('on'); showPop(false); }
      $$('.tg[data-store]').forEach(t => t.classList.toggle('on', Store.has(t.dataset.store)));
      if (popOpen) setTimeout(placePop, 450);
    }
    $$('.tg[data-store]').forEach(t => t.closest('label').addEventListener('click', () => {
      const res = Store.toggle(t.dataset.store);
      const list = res.added.length ? res.added : res.removed;
      if (list.length) toast($('#plA').closest('.win'), (res.added.length ? '已同时开启：' : '已同时关闭：') + list.map(x => MOD[x].name).join('、'));
    }));
    Store.on(() => sync(true));
    sync(false);
  }

  /* ------------------------------------------------------------------
     共享元素演示（T1 / T2）
     ------------------------------------------------------------------ */
  function initShared() {
    const se = $('#se'); if (!se) return;
    const T = ['寒蝉鸣泣之时 解', '只有神知道的世界 女神篇', '【我推的孩子】 第三季', '超时空辉夜姬！', '咒术回战 死灭回游', '为美好的世界献上祝福！3', '葬送的芙莉莲 第二季', '剧场版 魔法少女小圆', 'Fate/strange Fake', '地狱乐 第二季', '鬼灭之刃 无限城篇', '寒蝉鸣泣之时 礼'];
    const src = i => `assets/posters/p${i + 1}.jpg`;
    se.innerHTML = `
      <div class="wf-side" style="width:190px">
        <div class="dots3"><i></i><i></i><i></i></div>
        <div class="nav-item">${svg('home')}<span>首页</span></div>
        <div class="nav-item">${svg('calendar')}<span>新番</span></div>
        <div class="nav-item on">${svg('library')}<span>媒体库</span></div>
        <div class="nav-item">${svg('download')}<span>下载</span></div>
        <div class="se-mini" style="margin-top:auto;display:none;border-radius:12px;background:var(--wf-bg);border:1px solid var(--line);padding:6px;cursor:pointer;box-shadow:0 4px 14px -6px rgba(0,0,0,.2)">
          <div class="se-mthumb" style="aspect-ratio:16/9;border-radius:7px;overflow:hidden;background:#000"><img src="assets/still1.jpg" alt="" style="width:100%;height:100%;object-fit:cover"></div>
          <div style="display:flex;align-items:center;gap:6px;padding:6px 2px 0"><div style="flex:1;min-width:0;font-size:11.5px;font-weight:650;white-space:nowrap;overflow:hidden;text-overflow:ellipsis" class="se-mt"></div><span class="ib" style="width:22px;height:22px">${svg('pause', 'i', 'width:14px;height:14px')}</span></div>
        </div>
      </div>
      <div class="wf-main" style="position:relative">
        <div class="se-grid" style="position:absolute;inset:0;display:flex;flex-direction:column">
          <div class="wf-toolbar"><span class="grow"></span><div class="search" style="width:200px">${svg('search')}搜索</div></div>
          <div class="wf-content" style="padding-top:0"><div class="lt" style="margin:4px 0 18px">媒体库</div>
            <div style="display:grid;grid-template-columns:repeat(6, minmax(0, 1fr));gap:18px 16px">${T.map((t, i) => `<div><div class="poster se-p" data-i="${i}" style="cursor:pointer"><img src="${src(i)}" alt=""></div><div style="font-size:11.5px;font-weight:600;margin-top:6px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${t}</div></div>`).join('')}</div>
          </div>
        </div>
        <div class="se-detail" style="position:absolute;inset:0;display:none">
          <div class="se-bg" style="position:absolute;inset:0 0 auto 0;height:300px;background-size:cover;background-position:center 30%;filter:blur(36px) saturate(1.2);opacity:.32;-webkit-mask-image:linear-gradient(#000 30%,transparent);mask-image:linear-gradient(#000 30%,transparent)"></div>
          <div class="wf-toolbar" style="position:relative"><span class="ib se-back">${svg('chev-l')}</span><span class="cap2" style="font-size:13px">媒体库</span></div>
          <div style="position:relative;display:flex;gap:28px;padding:6px 28px 0">
            <div class="poster se-cover" style="width:170px;flex:none;box-shadow:0 20px 40px -16px rgba(0,0,0,.45)"><img alt=""></div>
            <div style="flex:1;padding-top:10px">
              <div class="cap2 st" style="font-size:12.5px">TV 动画 · 共 12 话</div>
              <div class="t1 st se-title" style="font-size:28px;margin:6px 0 10px"></div>
              <div class="st" style="font-size:13px;color:var(--text-2)"><b style="color:var(--text)">★ 8.1</b> Bangumi</div>
              <div class="st" style="margin:16px 0 20px;max-width:440px"><div class="bar" style="margin-bottom:8px"></div><div class="bar" style="width:92%;margin-bottom:8px"></div><div class="bar" style="width:55%"></div></div>
              <div class="st"><span class="b L p se-play">${svg('play')}播放 第 1 集</span></div>
            </div>
          </div>
          <div class="st" style="position:relative;padding:24px 28px 0"><div class="t2" style="font-size:15px;margin-bottom:10px">剧集</div>
            <div style="display:grid;grid-template-columns:repeat(4, minmax(0, 1fr));gap:14px">
              <div class="thumb se-ep" style="cursor:pointer"><img src="assets/still1.jpg" alt=""></div>
              <div class="thumb" style="display:grid;place-items:center"><b style="font-size:24px;color:var(--text-3)">02</b></div>
              <div class="thumb" style="display:grid;place-items:center"><b style="font-size:24px;color:var(--text-3)">03</b></div>
              <div class="thumb" style="display:grid;place-items:center"><b style="font-size:24px;color:var(--text-3)">04</b></div>
            </div>
          </div>
        </div>
      </div>
      <div class="se-player player" style="display:none;z-index:30">
        <img class="video" src="assets/still1.jpg" alt="">
        <div class="se-ui"><div class="shade-t"></div><div class="shade-b"></div>
          <div class="top"><span class="pib se-pback">${svg('chev-l')}</span><div class="ttl"><b class="se-ptitle"></b><span>第 1 集</span></div></div>
          <div class="hud"><div class="seek"><div class="track"><u style="width:50%"></u><b style="width:24%"></b></div><div class="knob" style="left:24%"></div></div>
            <div class="ctl"><span class="pib play">${svg('pause')}</span><span class="pib">${svg('next')}</span><span class="time">05:48 / 24:15</span><span class="grow"></span><span class="pib on">${svg('danmaku')}</span><span class="pib">${svg('cc')}</span><span class="pib">${svg('full')}</span></div>
          </div>
        </div>
      </div>`;
    const grid = $('.se-grid', se), detail = $('.se-detail', se), player = $('.se-player', se), ui = $('.se-ui', se);
    const mini = $('.se-mini', se), mthumb = $('.se-mthumb', se), cover = $('.se-cover', se);
    const cap = t => { const c = $('#seCap'); if (c) c.textContent = '状态：' + t; };
    let busy = false, curIdx = 0, miniOn = false;

    const rel = el => {
      const s = scaleOf(se), r = el.getBoundingClientRect(), R = se.getBoundingClientRect();
      return { x: (r.left - R.left) / s, y: (r.top - R.top) / s, w: r.width / s, h: r.height / s };
    };
    function fly(img, a, b, r0, r1, done) {
      const c = document.createElement('div');
      c.style.cssText = 'position:absolute;left:0;top:0;z-index:40;overflow:hidden;will-change:transform;background:#000';
      c.innerHTML = `<img src="${img}" alt="" style="width:100%;height:100%;object-fit:cover">`;
      se.appendChild(c);
      const L = (u, v, p) => u + (v - u) * p;
      tween(TOKENS.hero, p => {
        c.style.width = L(a.w, b.w, p) + 'px'; c.style.height = L(a.h, b.h, p) + 'px';
        c.style.transform = `translate(${L(a.x, b.x, p)}px, ${L(a.y, b.y, p)}px)`;
        c.style.borderRadius = Math.max(0, L(r0, r1, p)) + 'px';
      }, () => { done && done(); c.remove(); });
    }
    function fade(el, from, to, token = TOKENS.smooth, done) {
      el.style.opacity = String(from);
      tween(token, p => { el.style.opacity = String(from + (to - from) * Math.min(1, p)); }, () => { el.style.opacity = ''; done && done(); });
    }
    function stagger(els) {
      if (RM()) return;
      els.forEach((el, i) => {
        el.style.opacity = '0'; el.style.transform = 'translateY(14px)';
        setTimeout(() => tween(TOKENS.smooth, p => { el.style.opacity = String(Math.min(1, p)); el.style.transform = `translateY(${(1 - p) * 14}px)`; }, () => { el.style.transform = ''; el.style.opacity = ''; }), 60 + i * 35);
      });
    }

    $$('.se-p', se).forEach(p => p.addEventListener('click', () => {
      if (busy) return; busy = true;
      curIdx = +p.dataset.i;
      $('img', cover).src = src(curIdx);
      $('.se-title', detail).textContent = T[curIdx];
      $('.se-bg', detail).style.backgroundImage = `url(${src(curIdx)})`;
      $('.se-ptitle', player).textContent = T[curIdx];
      $('.se-mt', se).textContent = T[curIdx];
      detail.style.display = '';
      cover.style.visibility = 'hidden';
      const a = rel(p), b = rel(cover);
      p.style.visibility = 'hidden';
      fade(grid, 1, 0, TOKENS.snappy, () => { grid.style.display = 'none'; });
      stagger($$('.st', detail));
      fly(src(curIdx), a, b, 10, 10, () => { cover.style.visibility = ''; p.style.visibility = ''; busy = false; });
      cap('番剧详情（T1：海报 → 封面）');
    }));
    $('.se-back', se).addEventListener('click', () => {
      if (busy) return; busy = true;
      const p = $(`.se-p[data-i="${curIdx}"]`, se);
      grid.style.display = '';
      const a = rel(cover), b = rel(p);
      p.style.visibility = 'hidden'; cover.style.visibility = 'hidden';
      fade(detail, 1, 0, TOKENS.snappy, () => { detail.style.display = 'none'; cover.style.visibility = ''; });
      fade(grid, 0, 1);
      fly(src(curIdx), a, b, 10, 10, () => { p.style.visibility = ''; busy = false; });
      cap('媒体库（T1 反向）');
    });
    function openPlayer(fromEl, r0) {
      if (busy) return; busy = true;
      const a = rel(fromEl), b = { x: 0, y: 0, w: 960, h: 600 };
      fromEl.style.visibility = 'hidden';
      fly('assets/still1.jpg', a, b, r0, 0, () => {
        player.style.display = ''; fromEl.style.visibility = '';
        if (miniOn) { miniOn = false; mini.style.display = 'none'; }
        fade(ui, 0, 1);
        busy = false;
      });
      cap('全窗口播放（T2：缩略图 → 播放器）');
    }
    $('.se-play', se).addEventListener('click', () => openPlayer($('.se-ep', se), 10));
    $('.se-ep', se).addEventListener('click', () => openPlayer($('.se-ep', se), 10));
    mini.addEventListener('click', () => openPlayer(mthumb, 7));
    $('.se-pback', se).addEventListener('click', () => {
      if (busy) return; busy = true;
      mini.style.display = ''; miniOn = true;
      mthumb.style.visibility = 'hidden';
      if (!RM()) enter(mini, null);
      const a = { x: 0, y: 0, w: 960, h: 600 }, b = rel(mthumb);
      player.style.display = 'none';
      fly('assets/still1.jpg', a, b, 0, 7, () => { mthumb.style.visibility = ''; busy = false; });
      cap('迷你播放器（T2 反向：收进侧栏）');
    });
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
  function setAnnot(on) {
    root.classList.toggle('annot-on', on); kv.set('annot', on ? '1' : '0');
    const b = $('#annotBtn'); if (b) b.classList.toggle('on', on);
  }
  NP.onTheme(t => { const b = $('#themeBtn'); if (b) b.innerHTML = svg(t === 'dark' ? 'sun' : 'moon'); });
  NP.syncThemeUI();
  setAnnot(kv.get('annot') === '1');
  $('#themeBtn').addEventListener('click', () => setTheme(root.dataset.theme === 'dark' ? 'light' : 'dark'));
  $('#annotBtn').addEventListener('click', () => setAnnot(!root.classList.contains('annot-on')));

  initTier();
  renderMainSides();
  initNavGrowth();
  const optWin = $('#optWin');
  if (optWin) {
    optWin._toast = $('#optToast');
    NP.mountOptional($('#optRows'), Store, optWin, $('#optPreset'));
  }
  const syncSides = animate => {
    NP.renderSetSide($('#setSide'), 'optional', Store, animate);
    NP.renderSetSide($('#setSide2'), 'plugins', Store, animate);
  };
  Store.on(() => syncSides(true));
  syncSides(false);
  NP.renderMarket($('#mkt'));
  if ($('#ob')) NP.mountOnboarding($('#ob'), Store);
  initPlayer();
  initShared();
  initLab();
  initToc();

  watchFrames();
  autoJelly();
})();
