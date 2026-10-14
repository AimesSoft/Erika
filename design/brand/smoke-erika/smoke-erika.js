/* =====================================================================
   Erika 图标 · 大理石纹工作台（米白 / 蓝圆 + 圆角播放按钮）
   参考肥皂泡微距照片的效果：很多层很细的彩色丝线，被一股平滑的大流动整体拉长、扭曲，
   一个大漩涡把丝线卷成螺旋；散着几颗深色小圆点；边缘一圈泛白。
   做法（和「流体逐帧模拟」不同）：
   - 起点是很多层细条纹（酒红、靛蓝、米白和它们之间的过渡色淡紫、玫瑰），条纹本身带一点扭曲。
   - 流场是解析的、不随时间变的：一个主漩涡 + 几个副漩涡（高斯涡）+ 一股斜向剪切，
     由流函数求出来，所以处处不可压缩（不会挤压或撑开颜色）。
   - 每个像素沿流场倒推 T 时间回到起点（RK2，几百小步），在起点取条纹颜色。只取一次，
     所以细丝不会像逐帧平流那样越算越糊；取色时按像素宽度做几次采样平均，细丝也不会锯齿。
   - 种子决定漩涡位置、条纹顺序和宽度、扭曲和斑点；种子 + 搅动程度可以精确复现。
   ===================================================================== */
(() => {
  'use strict';

  /* ---------- 图标几何（1024 坐标） ---------- */
  const C = { x: 512, y: 512, r: 430 };
  const COL = { cream: '#F3ECDF', blue: '#2456FF', button: '#FF0046', w: '#872938', i: '#404292', dot: '#1C1B3D' };

  /* ---------- 参数 ---------- */
  const P = {
    seed: 7, T: 5, main: 1, shear: 1, minor: 3, freq: 1.3, warp: 0.9, specks: 36, rim: true, rimW: 0.14,
    soften: 0, circle: 'cream', ts: 400, tr: 46, plate: false,
    /* palette：light = 通透（大面积浅色，深酒红、深靛蓝只做细丝）/ deep = 浓郁（第一版）
       outline：三角形外面一圈米白描边的宽度，把按钮和纹理分开；bg：none = 透明 / dark = 深色底（像参考图的黑底泡泡） */
    palette: 'light', outline: 12, bg: 'none',
  };

  function rrect(x, y, w, h, r) {
    r = Math.min(r, h / 2, w / 2);
    return `M${x + r} ${y} L${x + w - r} ${y} A${r} ${r} 0 0 1 ${x + w} ${y + r} L${x + w} ${y + h - r} A${r} ${r} 0 0 1 ${x + w - r} ${y + h} L${x + r} ${y + h} A${r} ${r} 0 0 1 ${x} ${y + h - r} L${x} ${y + r} A${r} ${r} 0 0 1 ${x + r} ${y} Z`;
  }
  /* 播放按钮：朝右的等边三角形，重心放在圆心；ts = 看得见的高度，tr = 圆角半径。
     圆角的做法：先画一个缩进 tr 的尖角三角形，再用同色、线宽 2 × tr、圆角连接的描边把它撑回原来的大小 */
  function triPts() {
    const h = P.ts - 2 * P.tr, w = h * 0.866, cx = C.x, cy = C.y;
    return [[cx - w / 3, cy - h / 2], [cx - w / 3, cy + h / 2], [cx + w * 2 / 3, cy]];
  }
  const triPath = () => { const t = triPts(); return `M${t[0][0]} ${t[0][1]} L${t[1][0]} ${t[1][1]} L${t[2][0]} ${t[2][1]} Z`; };
  const circlePath = () => `M${C.x - C.r} ${C.y} A${C.r} ${C.r} 0 1 0 ${C.x + C.r} ${C.y} A${C.r} ${C.r} 0 1 0 ${C.x - C.r} ${C.y} Z`;

  function rng(seed) { let a = seed >>> 0; return () => { a |= 0; a = a + 0x6D2B79F5 | 0; let t = Math.imul(a ^ a >>> 15, 1 | a); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t; return ((t ^ t >>> 14) >>> 0) / 4294967296; }; }
  const hex = h => [1, 3, 5].map(i => parseInt(h.slice(i, i + 2), 16));
  const mix = (a, b, t) => { const A = hex(a), B = hex(b); return '#' + A.map((v, i) => Math.round(v + (B[i] - v) * t).toString(16).padStart(2, '0')).join(''); };
  /* 条纹用的颜色：三种原色 + 两两之间的过渡（只在相邻色相之间混，不会发灰）。
     wide = 大片色带用的颜色，thin = 细丝用的颜色 */
  const PALS = {
    deep: {
      wide: [{ c: COL.cream, w: .26 }, { c: mix(COL.i, COL.cream, .55), w: .2 }, { c: COL.i, w: .2 }, { c: mix(COL.w, COL.cream, .5), w: .14 }, { c: COL.w, w: .2 }],
      thin: null, lineP: .7, lineN: 2, accentP: 0,
    },
    /* 通透：大面积是米白、浅淡紫、浅玫瑰；深色只出现在细丝和偶尔的一条窄带里 */
    light: {
      wide: [{ c: COL.cream, w: .26 }, { c: mix(COL.i, COL.cream, .62), w: .24 }, { c: mix(COL.w, COL.cream, .6), w: .2 }, { c: mix(COL.i, COL.cream, .4), w: .16 }, { c: mix(COL.w, COL.cream, .42), w: .14 }],
      thin: [{ c: COL.w, w: .4 }, { c: COL.i, w: .4 }, { c: mix(COL.i, COL.cream, .25), w: .2 }], lineP: .9, lineN: 3, accentP: .45,
    },
  };
  const pickFrom = (r, list, not) => { let c; do { let x = r() * list.reduce((s, p) => s + p.w, 0); c = list[list.length - 1].c; for (const p of list) { if ((x -= p.w) <= 0) { c = p.c; break; } } } while (c === not && list.length > 1); return c; };

  /* ---------- 由种子决定的布局 ---------- */
  function layout() {
    const r = rng(P.seed * 7919 + 104729);
    /* 主漩涡在右上（像参考图），副漩涡随机放在圆里，正反转都有 */
    const V = [[0.36 + (r() - .5) * .12, -0.36 + (r() - .5) * .12, 0.11 * P.main, 0.32]];
    for (let i = 0; i < P.minor; i++) {
      const a = r() * Math.PI * 2, d = 0.25 + r() * 0.6;
      V.push([Math.cos(a) * d, Math.sin(a) * d, (r() < .5 ? -1 : 1) * (0.03 + r() * 0.06), 0.22 + r() * 0.3]);
    }
    /* 剪切：沿大约 −20° 的斜向流动（左下往右上斜着拉长） */
    const sa = (70 + (r() - .5) * 30) * Math.PI / 180;
    /* 条纹：方向接近水平，带一点倾斜 */
    const da = (95 + (r() - .5) * 30) * Math.PI / 180;
    /* 条纹：大片的色带为主（宽 3–9），有的色带后面跟几根细线（宽 0.12–0.5）——
       参考图里就是大块颜色中间夹着细丝，不是一样宽的细条纹。条纹用单独的随机数，换配色不会挪动漩涡和斑点 */
    const rs = rng(P.seed * 31337 + 7), PL = PALS[P.palette] || PALS.light, thin = PL.thin || PL.wide;
    const stripes = []; let total = 0, last = null;
    for (let i = 0; i < 9; i++) {
      const c = pickFrom(rs, PL.wide, last), w = 3 + rs() * 6; stripes.push([c, w]); total += w; last = c;
      const lines = rs() < PL.lineP ? 1 + Math.floor(rs() * PL.lineN) : 0;
      for (let j = 0; j < lines; j++) { const lc = pickFrom(rs, thin, c), lw = 0.12 + rs() * 0.33, gap = 0.3 + rs() * 0.9; stripes.push([lc, lw], [c, gap]); total += lw + gap; }
      if (rs() < PL.accentP) { const ac = pickFrom(rs, thin, c), aw = 0.6 + rs() * 0.6; stripes.push([ac, aw]); total += aw; }
    }
    /* 斑点：两团聚在一起，加少量散落 */
    const clusters = [[-0.45 + r() * .3, -0.5 + r() * .3], [-0.2 + r() * .4, 0.35 + r() * .3]];
    const dots = [];
    for (let i = 0; i < P.specks; i++) {
      let x, y;
      if (r() < .75) { const c = clusters[Math.floor(r() * 2)], a = r() * Math.PI * 2, d = Math.abs((r() + r() + r() - 1.5)) * .35; x = c[0] + Math.cos(a) * d; y = c[1] + Math.sin(a) * d; }
      else { const a = r() * Math.PI * 2, d = Math.sqrt(r()) * .85; x = Math.cos(a) * d; y = Math.sin(a) * d; }
      if (Math.hypot(x, y) > .9) { i--; continue; }
      dots.push([x, y, 1.2 + Math.pow(r(), 2.2) * 6]);
    }
    return { V, sn: [Math.cos(sa), Math.sin(sa)], dir: [Math.cos(da), Math.sin(da)], stripes, total, dots, seed: [r() * 50, r() * 50] };
  }

  /* ---------- WebGL：一次画完 ---------- */
  const glCanvas = document.createElement('canvas');
  const gl = glCanvas.getContext('webgl2', { alpha: true, premultipliedAlpha: true, preserveDrawingBuffer: true, antialias: false });
  if (!gl) { document.body.insertAdjacentHTML('afterbegin', '<p style="color:red;padding:20px">这个浏览器不支持 WebGL2。</p>'); return; }

  const VS = `#version 300 es
  in vec2 aPos; out vec2 vUv;
  void main(){ vUv=aPos*.5+.5; gl_Position=vec4(aPos,0.,1.); }`;
  const FS = `#version 300 es
  precision highp float;
  in vec2 vUv; out vec4 o;
  uniform sampler2D uPal;
  uniform vec4 uV[8]; uniform int uNV;
  uniform vec2 uSN; uniform float uShear, uT, uFreq, uWarp, uScale; uniform int uSteps;
  uniform vec2 uDir, uSeed;
  float h(vec2 p){ return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453); }
  float n(vec2 p){ vec2 i=floor(p), f=fract(p); f=f*f*(3.-2.*f);
    return mix(mix(h(i),h(i+vec2(1,0)),f.x), mix(h(i+vec2(0,1)),h(i+vec2(1,1)),f.x), f.y); }
  /* 只取三层、频率低：起点的扭曲要平滑，细节全部交给流动去拉（高频噪声被拉开后像撕碎的纸边） */
  float fbm(vec2 p){ float s=0., a=.55; for(int i=0;i<3;i++){ s+=a*n(p); p=p*1.9+vec2(1.7,9.2); a*=.45; } return s; }
  /* 流函数 ψ = Σ Γ·exp(−|p−c|²/σ²) + ½·S·(n·p)²；速度 = (∂ψ/∂y, −∂ψ/∂x)，处处不可压缩 */
  vec2 vel(vec2 p){
    vec2 v=vec2(0.);
    for(int i=0;i<8;i++){ if(i>=uNV) break; vec4 V=uV[i]; vec2 d=p-V.xy; float s2=V.w*V.w; v+=vec2(-d.y,d.x)*(2.*V.z/s2)*exp(-dot(d,d)/s2); }
    v+=vec2(uSN.y,-uSN.x)*dot(uSN,p)*uShear;
    return v;
  }
  float stripe(vec2 p){ return dot(p,uDir)*uFreq + fbm(p*1.1+uSeed)*uWarp; }
  void main(){
    /* 像素 → 以圆半径为单位、圆心为原点、y 向下的坐标 */
    vec2 p=(vUv*2.-1.)*vec2(1.,-1.)*uScale;
    float dt=uT/float(uSteps);
    for(int k=0;k<480;k++){ if(k>=uSteps) break; vec2 a=vel(p); p-=dt*vel(p-.5*dt*a); }
    float s=stripe(p), w=clamp(fwidth(s),0.,.5);
    /* 按这个像素覆盖的条纹宽度取 5 次平均：细丝不起锯齿 */
    vec3 c=vec3(0.);
    for(int i=0;i<5;i++){ c+=texture(uPal,vec2(fract(s+(float(i)/4.-.5)*w),.5)).rgb; }
    o=vec4(c/5.,1.);
  }`;
  function compile(type, src) { const s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s); if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s)); return s; }
  const prog = gl.createProgram(); gl.attachShader(prog, compile(gl.VERTEX_SHADER, VS)); gl.attachShader(prog, compile(gl.FRAGMENT_SHADER, FS));
  gl.bindAttribLocation(prog, 0, 'aPos'); gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(prog));
  const U = {}; for (let i = 0, n = gl.getProgramParameter(prog, gl.ACTIVE_UNIFORMS); i < n; i++) { const nm = gl.getActiveUniform(prog, i).name; U[nm.replace('[0]', '')] = gl.getUniformLocation(prog, nm); }
  const quad = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, quad);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
  const palTex = gl.createTexture();

  /* 条纹色表：一行 1024 像素，按宽度排好颜色，边缘柔一点点（像墨线的边） */
  function uploadPalette(L) {
    const c = document.createElement('canvas'); c.width = 1024; c.height = 4;
    const x = c.getContext('2d'); let pos = 0;
    L.stripes.forEach(([col, w]) => { const W = w / L.total * 1024; x.fillStyle = col; x.fillRect(pos, 0, W + 1, 4); pos += W; });
    const s = document.createElement('canvas'); s.width = 1024; s.height = 4;
    const sx = s.getContext('2d'); sx.filter = 'blur(1.5px)';
    sx.drawImage(c, -1024, 0); sx.drawImage(c, 0, 0); sx.drawImage(c, 1024, 0);
    gl.bindTexture(gl.TEXTURE_2D, palTex);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, s);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  }
  let L = null;
  function marble(size) {
    if (glCanvas.width !== size) { glCanvas.width = glCanvas.height = size; }
    gl.bindFramebuffer(gl.FRAMEBUFFER, null); gl.viewport(0, 0, size, size);
    gl.useProgram(prog);
    gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, palTex); gl.uniform1i(U.uPal, 0);
    const flat = new Float32Array(32); L.V.forEach((v, i) => flat.set(v, i * 4));
    gl.uniform4fv(U.uV, flat); gl.uniform1i(U.uNV, L.V.length);
    gl.uniform2f(U.uSN, L.sn[0], L.sn[1]); gl.uniform1f(U.uShear, 0.35 * P.shear);
    gl.uniform1f(U.uT, P.T); gl.uniform1i(U.uSteps, Math.max(8, Math.min(480, Math.round(P.T * 36))));
    gl.uniform1f(U.uFreq, P.freq); gl.uniform1f(U.uWarp, P.warp); gl.uniform1f(U.uScale, 512 / C.r);
    gl.uniform2f(U.uDir, L.dir[0], L.dir[1]); gl.uniform2f(U.uSeed, L.seed[0], L.seed[1]);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }

  /* ---------- 合成：底色圆 → 大理石纹 → 斑点 → 边缘泛白 → 播放按钮 ---------- */
  const tmp = document.createElement('canvas');
  function reset() { L = layout(); uploadPalette(L); }
  /* part：'all'（默认）/ 'circle'（底色圆 + 纹理）/ 'button'（只有按钮），后两个给液态玻璃分层用 */
  function composite(canvas, size, part = 'all') {
    if (!L) reset();
    const ctx = canvas.getContext('2d');
    canvas.width = canvas.height = size;
    ctx.setTransform(1, 0, 0, 1, 0, 0); ctx.clearRect(0, 0, size, size);
    const k = size / 1024, plate = P.plate && part === 'all';
    ctx.setTransform(k, 0, 0, k, 0, 0);
    const dark = P.bg === 'dark' && part === 'all';
    if (plate) {
      ctx.fillStyle = dark ? '#0B0B10' : '#FFFFFF'; ctx.fill(new Path2D(rrect(102, 102, 820, 820, 184)));
      ctx.translate(512, 512); ctx.scale(0.86, 0.86); ctx.translate(-512, -512);
    } else if (dark) { ctx.fillStyle = '#0B0B10'; ctx.fillRect(0, 0, 1024, 1024); }
    const circle = new Path2D(circlePath()), tri = new Path2D(triPath());
    if (dark && P.rim) {
      /* 深色底：泡泡边缘外面一圈很淡的光 */
      const g = ctx.createRadialGradient(C.x, C.y, C.r * .98, C.x, C.y, C.r * 1.07);
      g.addColorStop(0, 'rgba(243,236,223,.32)'); g.addColorStop(1, 'rgba(243,236,223,0)');
      ctx.fillStyle = g; ctx.beginPath(); ctx.arc(C.x, C.y, C.r * 1.07, 0, Math.PI * 2); ctx.fill();
    }
    if (part !== 'button') {
      ctx.fillStyle = COL[P.circle] || COL.cream; ctx.fill(circle);
      /* 渲染分辨率比输出高一半，再缩下来：细丝更干净 */
      marble(Math.min(3072, Math.max(384, Math.round(size * 1.5 * (plate ? 0.86 : 1)))));
      const n = glCanvas.width; tmp.width = tmp.height = n;
      const tx = tmp.getContext('2d'); tx.clearRect(0, 0, n, n);
      tx.filter = P.soften > 0 ? `blur(${P.soften * n / 1024}px)` : 'none';
      tx.drawImage(glCanvas, 0, 0); tx.filter = 'none';
      ctx.save(); ctx.clip(circle);
      ctx.imageSmoothingQuality = 'high'; ctx.drawImage(tmp, 0, 0, 1024, 1024);
      /* 斑点：深靛蓝的小圆点 */
      ctx.fillStyle = COL.dot;
      L.dots.forEach(([x, y, r]) => { ctx.globalAlpha = .88; ctx.beginPath(); ctx.arc(C.x + x * C.r, C.y + y * C.r, r, 0, Math.PI * 2); ctx.fill(); });
      ctx.globalAlpha = 1;
      if (P.rim && P.circle === 'cream') {
        /* 边缘泛白：最外一圈慢慢退成米白，像肥皂泡的边 */
        const g = ctx.createRadialGradient(C.x, C.y, C.r * (1 - P.rimW), C.x, C.y, C.r);
        g.addColorStop(0, 'rgba(243,236,223,0)'); g.addColorStop(.7, 'rgba(243,236,223,.55)'); g.addColorStop(1, 'rgba(243,236,223,.95)');
        ctx.fillStyle = g; ctx.fill(circle);
      }
      ctx.restore();
    }
    if (part !== 'circle') {
      /* 米白描边：把按钮从纹理里分出来（先画一个更胖的米白三角，再盖上红色的） */
      if (P.outline > 0) { ctx.strokeStyle = COL.cream; ctx.lineWidth = 2 * (P.tr + P.outline); ctx.lineJoin = 'round'; ctx.fillStyle = COL.cream; ctx.fill(tri); ctx.stroke(tri); }
      ctx.fillStyle = COL.button; ctx.strokeStyle = COL.button; ctx.lineWidth = 2 * P.tr; ctx.lineJoin = 'round';
      ctx.fill(tri); ctx.stroke(tri);
    }
  }

  /* 和旧接口一致：renderFrame = 按当前参数重新布局；step 让搅动程度往前走一点（「播放」用） */
  function renderFrame() { reset(); }
  window.ErikaSmoke = { P, COL, renderFrame, composite, reset, get t() { return P.T; }, step() { P.T = Math.min(12, +(P.T + 0.05).toFixed(2)); }, ok: true };
})();
