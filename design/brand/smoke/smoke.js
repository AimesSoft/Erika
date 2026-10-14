/* =====================================================================
   Erika 图标 · 烟雾工作台
   WebGL2 流体模拟：Stable Fluids（Jos Stam）+ 涡量增强，结构参考 Pavel Dobryakov 的
   WebGL-Fluid-Simulation（MIT）。每个形状一套独立的流体，速度场被形状挡住，烟只在里面飘；
   染料用「滤色」叠到纯色上，所以永远只会变亮，不会发灰。种子 + 帧数可以精确复现某一帧。
   ===================================================================== */
(() => {
  'use strict';
  const $ = s => document.querySelector(s);

  /* ---------- 图标几何（与 brand/icon/*.svg 一致，1024 坐标） ---------- */
  const PATHS = {
    green: 'M432.7 391.8 L632 741.2 A50 50 0 0 1 588.3 816 L186.2 813.9 A50 50 0 0 1 143.3 738.7 L346.1 391.3 A50 50 0 0 1 432.7 391.8 Z',
    blue: 'M146.8 493.3 A50 50 0 0 1 100.1 434.1 A418.6 418.6 0 1 1 530.9 929.6 A50 50 0 0 1 478.7 876.3 L503.5 511.1 Z',
    red: 'M386.8 718.4 L267.6 335.1 A50 50 0 0 1 326.3 271.4 L718.4 360 A50 50 0 0 1 744.1 442.7 L471.3 737.5 A50 50 0 0 1 386.8 718.4 Z',
  };
  const PLATE = 'M286 102 L740 102 C848.6 102 924 177.4 924 286 L924 740 C924 848.6 848.6 924 740 924 L286 924 C177.4 924 102 848.6 102 740 L102 286 C102 177.4 177.4 102 286 102 Z';
  const BAR = [[777.1, 255.2], [851.9, 495.8]];

  /* 烟从哪条边升起：一串发射点 + 往形状里吹的方向（SVG 坐标，y 向下） */
  function rimEmitters(cx, cy, r, a0, a1, n, swirl) {
    const out = [];
    for (let i = 0; i < n; i++) {
      const a = (a0 + (a1 - a0) * (i / (n - 1))) * Math.PI / 180;
      const x = cx + r * Math.cos(a), y = cy + r * Math.sin(a);
      let dx = cx - x, dy = cy - y; const L = Math.hypot(dx, dy); dx /= L; dy /= L;
      const s = swirl * Math.PI / 180, c = Math.cos(s), sn = Math.sin(s);
      out.push({ x, y, dx: dx * c - dy * sn, dy: dx * sn + dy * c });
    }
    return out;
  }
  function edgeEmitters(p0, p1, n, target) {
    const out = [];
    for (let i = 0; i < n; i++) {
      const t = i / (n - 1), x = p0[0] + (p1[0] - p0[0]) * t, y = p0[1] + (p1[1] - p0[1]) * t;
      let dx = target[0] - x, dy = target[1] - y; const L = Math.hypot(dx, dy);
      out.push({ x, y, dx: dx / L, dy: dy / L });
    }
    return out;
  }
  const REGIONS = {
    green: {
      base: '#07FF94', palette: ['#00C8FF', '#C8FF00', '#00E0FF'], focus: [175, 790, 90, 330],
      emitters: edgeEmitters([160, 792], [470, 800], 7, [330, 600]),
    },
    blue: {
      base: '#2A51D4', palette: ['#23C8FF', '#7A5CFF', '#FF3DBE', '#B04CFF'], focus: [820, 815, 120, 520],
      emitters: rimEmitters(511.5, 511.5, 395, 10, 100, 9, 28),
    },
    red: {
      base: '#FF2E55', palette: ['#FFB800', '#FF7A1A', '#FF3DBE'], focus: [440, 745, 90, 380],
      emitters: edgeEmitters([428, 735], [690, 470], 7, [430, 430]),
    },
  };
  const ORDER = ['green', 'blue', 'red'];

  /* ---------- 参数 ---------- */
  const P = {
    seed: 48, frame: 80, emit: 24, splats: 1, force: 520, radius: 0.005, curl: 18, dyeAmount: 0.5,
    exposure: 1.5, spread: 1, soften: 1.5, opacity: 0.92, dissipation: 0.994, simRes: 160, dyeRes: 768,
    plate: false, bar: false,
  };

  /* ---------- WebGL ---------- */
  const glCanvas = document.createElement('canvas');
  const gl = glCanvas.getContext('webgl2', { alpha: false, preserveDrawingBuffer: true, antialias: false, premultipliedAlpha: false });
  if (!gl) { document.body.insertAdjacentHTML('afterbegin', '<p style="color:red;padding:20px">这个浏览器不支持 WebGL2。</p>'); return; }
  const extCBF = gl.getExtension('EXT_color_buffer_float');
  gl.getExtension('OES_texture_float_linear');
  const HALF = gl.HALF_FLOAT;

  const VS = `#version 300 es
  in vec2 aPos; uniform vec2 texel;
  out vec2 vUv, vL, vR, vT, vB;
  void main(){ vUv=aPos*.5+.5; vL=vUv-vec2(texel.x,0.); vR=vUv+vec2(texel.x,0.); vT=vUv+vec2(0.,texel.y); vB=vUv-vec2(0.,texel.y); gl_Position=vec4(aPos,0.,1.); }`;
  const head = `#version 300 es
  precision highp float; precision highp sampler2D;
  in vec2 vUv, vL, vR, vT, vB; out vec4 o;`;
  const FS = {
    splat: head + `uniform sampler2D uTarget; uniform vec3 color; uniform vec2 point; uniform float radius;
      void main(){ vec2 p=vUv-point; vec3 s=exp(-dot(p,p)/radius)*color; o=vec4(texture(uTarget,vUv).xyz+s,1.); }`,
    advect: head + `uniform sampler2D uVelocity, uSource; uniform vec2 vTexel; uniform float dt, dissipation;
      void main(){ vec2 c=vUv-dt*texture(uVelocity,vUv).xy*vTexel; o=vec4(dissipation*texture(uSource,c).xyz,1.); }`,
    divergence: head + `uniform sampler2D uVelocity;
      void main(){ float L=texture(uVelocity,vL).x, R=texture(uVelocity,vR).x, T=texture(uVelocity,vT).y, B=texture(uVelocity,vB).y;
        vec2 C=texture(uVelocity,vUv).xy; if(vL.x<0.)L=-C.x; if(vR.x>1.)R=-C.x; if(vT.y>1.)T=-C.y; if(vB.y<0.)B=-C.y;
        o=vec4(.5*(R-L+T-B),0.,0.,1.); }`,
    curl: head + `uniform sampler2D uVelocity;
      void main(){ float L=texture(uVelocity,vL).y, R=texture(uVelocity,vR).y, T=texture(uVelocity,vT).x, B=texture(uVelocity,vB).x;
        o=vec4(.5*(R-L-T+B),0.,0.,1.); }`,
    vorticity: head + `uniform sampler2D uVelocity, uCurl; uniform float curl, dt;
      void main(){ float L=texture(uCurl,vL).x, R=texture(uCurl,vR).x, T=texture(uCurl,vT).x, B=texture(uCurl,vB).x, C=texture(uCurl,vUv).x;
        vec2 f=.5*vec2(abs(T)-abs(B), abs(R)-abs(L)); f/=length(f)+1e-4; f*=curl*C; f.y*=-1.;
        vec2 v=texture(uVelocity,vUv).xy+f*dt; o=vec4(clamp(v,-1000.,1000.),0.,1.); }`,
    pressure: head + `uniform sampler2D uPressure, uDivergence;
      void main(){ float L=texture(uPressure,vL).x, R=texture(uPressure,vR).x, T=texture(uPressure,vT).x, B=texture(uPressure,vB).x;
        o=vec4((L+R+B+T-texture(uDivergence,vUv).x)*.25,0.,0.,1.); }`,
    gradient: head + `uniform sampler2D uPressure, uVelocity;
      void main(){ float L=texture(uPressure,vL).x, R=texture(uPressure,vR).x, T=texture(uPressure,vT).x, B=texture(uPressure,vB).x;
        o=vec4(texture(uVelocity,vUv).xy-vec2(R-L,T-B),0.,1.); }`,
    scale: head + `uniform sampler2D uSource; uniform float value; void main(){ o=vec4(value*texture(uSource,vUv).xyz,1.); }`,
    mask: head + `uniform sampler2D uVelocity, uMask; void main(){ o=vec4(texture(uVelocity,vUv).xy*texture(uMask,vUv).r,0.,1.); }`,
    display: head + `uniform sampler2D uDye; uniform float exposure;
      void main(){ vec3 c=texture(uDye,vUv).rgb; o=vec4(1.-exp(-max(c,0.)*exposure),1.); }`,
  };
  function compile(type, src) {
    const s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
    return s;
  }
  const vs = compile(gl.VERTEX_SHADER, VS);
  const PROG = {};
  for (const k in FS) {
    const p = gl.createProgram(); gl.attachShader(p, vs); gl.attachShader(p, compile(gl.FRAGMENT_SHADER, FS[k]));
    gl.bindAttribLocation(p, 0, 'aPos'); gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
    const u = {}; const n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS);
    for (let i = 0; i < n; i++) { const name = gl.getActiveUniform(p, i).name; u[name] = gl.getUniformLocation(p, name); }
    PROG[k] = { p, u };
  }
  const quad = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, quad);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);

  function fbo(w, h, internal = gl.RGBA16F, format = gl.RGBA, type = HALF) {
    const tex = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texImage2D(gl.TEXTURE_2D, 0, internal, w, h, 0, format, type, null);
    const fb = gl.createFramebuffer(); gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
    gl.viewport(0, 0, w, h); gl.clearColor(0, 0, 0, 1); gl.clear(gl.COLOR_BUFFER_BIT);
    return { tex, fb, w, h, texel: [1 / w, 1 / h] };
  }
  function dfbo(w, h) {
    let a = fbo(w, h), b = fbo(w, h);
    return { get read() { return a; }, get write() { return b; }, swap() { [a, b] = [b, a]; }, w, h, texel: a.texel };
  }
  function free(f) { if (!f) return; if (f.read) { free(f.read); free(f.write); return; } gl.deleteTexture(f.tex); gl.deleteFramebuffer(f.fb); }
  let unit = 0;
  function bindTex(prog, name, f) { gl.activeTexture(gl.TEXTURE0 + unit); gl.bindTexture(gl.TEXTURE_2D, f.tex); gl.uniform1i(prog.u[name], unit); unit++; }
  function run(name, target, texel, set) {
    const prog = PROG[name]; gl.useProgram(prog.p); unit = 0;
    gl.uniform2f(prog.u.texel, texel[0], texel[1]);
    set(prog);
    if (target) { gl.bindFramebuffer(gl.FRAMEBUFFER, target.fb); gl.viewport(0, 0, target.w, target.h); }
    else { gl.bindFramebuffer(gl.FRAMEBUFFER, null); gl.viewport(0, 0, glCanvas.width, glCanvas.height); }
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }

  /* 形状遮罩：在模拟分辨率上画出形状，边缘稍微羽化，作为挡烟的墙 */
  function maskTexture(path, res) {
    const c = document.createElement('canvas'); c.width = c.height = res;
    const x = c.getContext('2d'); x.filter = 'blur(1.2px)'; x.scale(res / 1024, res / 1024);
    x.fillStyle = '#fff'; x.fill(new Path2D(path));
    const tex = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, c);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    return { tex };
  }

  function rng(seed) { let a = seed >>> 0; return () => { a |= 0; a = a + 0x6D2B79F5 | 0; let t = Math.imul(a ^ a >>> 15, 1 | a); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t; return ((t ^ t >>> 14) >>> 0) / 4294967296; }; }
  const hex = h => [parseInt(h.slice(1, 3), 16) / 255, parseInt(h.slice(3, 5), 16) / 255, parseInt(h.slice(5, 7), 16) / 255];

  /* ---------- 一个形状的一套流体 ---------- */
  class Sim {
    constructor(key, seedOffset) {
      this.key = key; this.R = REGIONS[key];
      const s = P.simRes, d = P.dyeRes;
      this.vel = dfbo(s, s); this.pres = dfbo(s, s); this.div = fbo(s, s); this.curl = fbo(s, s); this.dye = dfbo(d, d);
      this.mask = maskTexture(PATHS[key], s);
      this.rand = rng(P.seed * 7919 + seedOffset * 104729);
      this.t = 0;
    }
    dispose() { [this.vel, this.pres, this.div, this.curl, this.dye].forEach(free); gl.deleteTexture(this.mask.tex); }
    splat(x, y, vx, vy, col, radius) {
      const u = x / 1024, v = 1 - y / 1024;
      run('splat', this.vel.write, this.vel.texel, p => { bindTex(p, 'uTarget', this.vel.read); gl.uniform3f(p.u.color, vx, -vy, 0); gl.uniform2f(p.u.point, u, v); gl.uniform1f(p.u.radius, radius); });
      this.vel.swap();
      run('splat', this.dye.write, this.dye.texel, p => { bindTex(p, 'uTarget', this.dye.read); gl.uniform3f(p.u.color, col[0], col[1], col[2]); gl.uniform2f(p.u.point, u, v); gl.uniform1f(p.u.radius, radius * 1.4); });
      this.dye.swap();
    }
    emit() {
      const r = this.rand, E = this.R.emitters, pal = this.R.palette;
      const k = P.splats;
      for (let i = 0; i < k; i++) {
        const e = E[Math.floor(r() * E.length)];
        const jx = (r() - .5) * 26, jy = (r() - .5) * 26, wob = (r() - .5) * 0.9;
        const c = Math.cos(wob), s = Math.sin(wob);
        const dx = e.dx * c - e.dy * s, dy = e.dx * s + e.dy * c;
        const f = P.force * (0.6 + r() * 0.8);
        const col = hex(pal[Math.floor(r() * pal.length)]).map(v => v * P.dyeAmount * (0.7 + r() * 0.6));
        this.splat(e.x + jx, e.y + jy, dx * f, dy * f, col, P.radius * (0.6 + r() * 0.9));
      }
    }
    step(dt = 0.016) {
      if (this.t < P.emit) this.emit();
      const vt = this.vel.texel;
      run('curl', this.curl, vt, p => bindTex(p, 'uVelocity', this.vel.read));
      run('vorticity', this.vel.write, vt, p => { bindTex(p, 'uVelocity', this.vel.read); bindTex(p, 'uCurl', this.curl); gl.uniform1f(p.u.curl, P.curl); gl.uniform1f(p.u.dt, dt); });
      this.vel.swap();
      run('divergence', this.div, vt, p => bindTex(p, 'uVelocity', this.vel.read));
      run('scale', this.pres.write, vt, p => { bindTex(p, 'uSource', this.pres.read); gl.uniform1f(p.u.value, 0.8); });
      this.pres.swap();
      for (let i = 0; i < 20; i++) {
        run('pressure', this.pres.write, vt, p => { bindTex(p, 'uPressure', this.pres.read); bindTex(p, 'uDivergence', this.div); });
        this.pres.swap();
      }
      run('gradient', this.vel.write, vt, p => { bindTex(p, 'uPressure', this.pres.read); bindTex(p, 'uVelocity', this.vel.read); });
      this.vel.swap();
      run('mask', this.vel.write, vt, p => { bindTex(p, 'uVelocity', this.vel.read); bindTex(p, 'uMask', this.mask); });
      this.vel.swap();
      run('advect', this.vel.write, vt, p => { bindTex(p, 'uVelocity', this.vel.read); bindTex(p, 'uSource', this.vel.read); gl.uniform2f(p.u.vTexel, vt[0], vt[1]); gl.uniform1f(p.u.dt, dt); gl.uniform1f(p.u.dissipation, 0.995); });
      this.vel.swap();
      run('advect', this.dye.write, this.dye.texel, p => { bindTex(p, 'uVelocity', this.vel.read); bindTex(p, 'uSource', this.dye.read); gl.uniform2f(p.u.vTexel, vt[0], vt[1]); gl.uniform1f(p.u.dt, dt); gl.uniform1f(p.u.dissipation, P.dissipation); });
      this.dye.swap();
      this.t++;
    }
    display(size) {
      if (glCanvas.width !== size) { glCanvas.width = glCanvas.height = size; }
      run('display', null, this.dye.texel, p => { bindTex(p, 'uDye', this.dye.read); gl.uniform1f(p.u.exposure, P.exposure); });
    }
  }

  /* ---------- 合成：纯色形状 + 烟（滤色，裁在形状里） ---------- */
  let sims = null;
  const layers = {};
  function layer(n) { return layers[n] || (layers[n] = Object.assign(document.createElement('canvas'), { width: n, height: n })); }
  function reset() { if (sims) sims.forEach(s => s.dispose()); sims = ORDER.map((k, i) => new Sim(k, i + 1)); }
  /* only：只画某一个零件（green / blue / red），背景透明，用于液态玻璃图标的分层 */
  function composite(canvas, size, only = null) {
    const ctx = canvas.getContext('2d');
    canvas.width = canvas.height = size;
    ctx.setTransform(1, 0, 0, 1, 0, 0); ctx.clearRect(0, 0, size, size);
    const k = size / 1024;
    ctx.setTransform(k, 0, 0, k, 0, 0);
    if (P.plate && !only) {
      ctx.fillStyle = '#FFFFFF'; ctx.fill(new Path2D(PLATE));
      ctx.translate(512, 511.5); ctx.scale(0.846, 0.846); ctx.translate(-511.5, -511.5);
    }
    sims.forEach(sim => {
      if (only && sim.key !== only) return;
      const path = new Path2D(PATHS[sim.key]);
      ctx.fillStyle = sim.R.base; ctx.fill(path);
      sim.display(Math.min(2048, Math.max(256, Math.round(size * (P.plate && !only ? 0.846 : 1)))));
      // 位置遮罩：烟在冒烟的那条边最浓，往里淡掉；流体只负责纹理
      const n = glCanvas.width, tmp = layer(n), tx = tmp.getContext('2d');
      tx.globalCompositeOperation = 'source-over'; tx.clearRect(0, 0, n, n);
      tx.filter = P.soften > 0 ? `blur(${P.soften * n / 1024}px)` : 'none';
      tx.drawImage(glCanvas, 0, 0); tx.filter = 'none';
      const [fx, fy, r0, r1] = sim.R.focus, q = n / 1024;
      const g = tx.createRadialGradient(fx * q, fy * q, r0 * P.spread * q, fx * q, fy * q, r1 * P.spread * q);
      g.addColorStop(0, 'rgba(0,0,0,1)'); g.addColorStop(.55, 'rgba(0,0,0,.45)'); g.addColorStop(1, 'rgba(0,0,0,0)');
      tx.globalCompositeOperation = 'destination-in'; tx.fillStyle = g; tx.fillRect(0, 0, n, n);
      ctx.save(); ctx.clip(path);
      ctx.globalCompositeOperation = 'screen'; ctx.globalAlpha = P.opacity;
      ctx.drawImage(tmp, 0, 0, 1024, 1024);
      ctx.restore();
    });
    if (P.bar && (!only || only === 'red')) {
      ctx.strokeStyle = '#FF2E55'; ctx.lineWidth = 60; ctx.lineCap = 'round';
      ctx.beginPath(); ctx.moveTo(...BAR[0]); ctx.lineTo(...BAR[1]); ctx.stroke();
    }
  }

  /* ---------- 渲染指定帧（可复现） ---------- */
  function renderFrame(frame = P.frame) {
    reset();
    for (let i = 0; i < frame; i++) sims.forEach(s => s.step());
  }

  window.NPSmoke = { P, REGIONS, renderFrame, composite, reset, get sims() { return sims; }, step() { sims.forEach(s => s.step()); }, ok: !!extCBF };
})();
