#!/usr/bin/env node
// 把 logo 的出场动画和双击小动作录成一个 GIF：
//
//     node design/brand/erika-icon-export/demo-gif.mjs
//
// 用无头 Chrome 打开一页只有 logo 的网页，按时间轴把每个 CSS 动画暂停在指定时刻、逐帧截图，再用 ffmpeg 合成 GIF。
// 输出 demo/erika-logo-demo.gif（build.py 不会清掉这个目录）。改了 build.py 里的动画以后，先跑 build.py 再跑这个。
//
// 需要：Node 22 以上（自带 WebSocket）、ffmpeg、Chrome。默认找 Playwright 缓存里的 chrome-headless-shell，
// 找不到再用 /Applications/Google Chrome.app；也可以用环境变量 CHROME 指定。
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const HERE = path.dirname(new URL(import.meta.url).pathname);
const OUT = path.join(HERE, 'demo', 'erika-logo-demo.gif');
const FPS = 25, W = 540, H = 540;

// ---------- 时间轴（秒） ----------
// 先播出场动画，然后鼠标移过来双击三次，依次演示 wink、摸头、坏笑
const LOGO = { x: 90, y: 52, size: 360 };                 // logo 在画面里的位置；SVG 画板 -16..1040
const at = (u, v) => [LOGO.x + (u + 16) * LOGO.size / 1056, LOGO.y + (v + 16) * LOGO.size / 1056];
const REACTS = [                                          // 第二下点击的时刻 = 动作开始
  { name: 'wink', start: 3.85, dur: 1.5, label: '双击 · wink', click: at(730, 830) },
  { name: 'pet', start: 6.45, dur: 2.6, label: '双击 · 摸头', click: at(512, 300) },
  { name: 'smug', start: 10.2, dur: 2.8, label: '双击 · 坏笑', click: at(730, 830) },
];
const END = 13.9;
const OFF = [W + 40, H + 60];                             // 鼠标藏在画面右下角外面

const ease = (t) => t < .5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
const lerp = (a, b, t) => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];

// 每一帧鼠标的位置：[x, y, 不透明度]；以及点击波纹
function cursorAt(t) {
  const [w, p, s] = REACTS;
  const rest = at(980, 1000);   // 两次双击之间，鼠标停在 logo 右下角外面
  const keys = [   // [时刻, 位置]，相邻两项之间缓动过去
    [3.0, OFF], [3.6, w.click], [4.3, w.click], [4.9, rest],
    [5.7, rest], [6.2, p.click], [9.1, p.click], [9.9, s.click],
    [10.7, s.click], [11.4, OFF],
  ];
  let pos = OFF;
  if (t >= keys[0][0]) {
    pos = keys[keys.length - 1][1];
    for (let i = 0; i < keys.length - 1; i++) {
      const [t0, a] = keys[i], [t1, b] = keys[i + 1];
      if (t >= t0 && t < t1) { pos = lerp(a, b, ease((t - t0) / (t1 - t0))); break; }
    }
  }
  // 摸头的时候鼠标在头顶左右蹭
  if (t > p.start + .15 && t < p.start + 2.1) {
    const k = (t - p.start - .15) / 1.95;
    pos = [pos[0] + Math.sin(k * Math.PI * 4) * 34 * Math.sin(k * Math.PI), pos[1] + Math.abs(Math.sin(k * Math.PI * 4)) * -4];
  }
  return pos;
}

function pageHtml(svg) {
  svg = svg.replace(/<script>[\s\S]*?<\/script>/, '')
    .replace(/width="1024" height="1024"/, `width="${LOGO.size}" height="${LOGO.size}"`);
  return `<!doctype html><html><head><meta charset="utf-8"><style>
  html, body { margin: 0; width: ${W}px; height: ${H}px; overflow: hidden; background: #FBFAF8; }
  #logo { position: absolute; left: ${LOGO.x}px; top: ${LOGO.y}px; width: ${LOGO.size}px; height: ${LOGO.size}px; }
  #cap { position: absolute; left: 0; right: 0; top: ${LOGO.y + LOGO.size + 34}px; text-align: center;
         font: 600 19px/1 "PingFang SC", system-ui, sans-serif; color: #4A4F63; letter-spacing: .02em; }
  #cur { position: absolute; left: 0; top: 0; width: 26px; height: 34px; pointer-events: none; }
  .rip { position: absolute; width: 0; height: 0; }
  .rip i { position: absolute; border: 3px solid #FFFFFF; border-radius: 50%; box-shadow: 0 0 0 1.5px rgba(22, 23, 27, .28), inset 0 0 0 1.5px rgba(22, 23, 27, .18); }
  #cur { transform-origin: 2px 2px; }
</style></head><body>
<div id="logo">${svg}</div>
<div id="cap"></div>
<div class="rip" id="r1"><i></i></div><div class="rip" id="r2"><i></i></div>
<svg id="cur" viewBox="0 0 26 34"><path d="M2 2L2 26L8.2 20.4L12.4 30.2L16.6 28.4L12.5 18.8L21 18.4Z" fill="#fff" stroke="#16171B" stroke-width="2" stroke-linejoin="round"/></svg>
</body></html>`;
}

// 页面里执行：把画面定格在第 t 秒
const SET_FRAME = `(t, reacts, cur, caption, ripples, press) => {
  const svg = document.querySelector('#logo svg');
  let cls = '', local = 0;
  for (const r of reacts) if (t >= r.start && t < r.start + r.dur) { cls = 'do-' + r.name; local = (t - r.start) * 1000; }
  if ((svg.getAttribute('class') || '') !== cls) { if (cls) svg.setAttribute('class', cls); else svg.removeAttribute('class'); }
  svg.getBoundingClientRect();
  for (const a of svg.getAnimations({ subtree: true })) {
    a.pause();
    a.currentTime = /^(wk|pt|sm)-/.test(a.animationName) ? local : t * 1000;
  }
  const c = document.getElementById('cur');
  c.style.transform = 'translate(' + (cur[0] - 2) + 'px,' + (cur[1] - 2) + 'px) scale(' + press + ')';
  const cap = document.getElementById('cap');
  cap.textContent = caption[0]; cap.style.opacity = caption[1];
  ripples.forEach((rp, i) => {
    const el = document.getElementById('r' + (i + 1)), ring = el.firstChild;
    el.style.left = rp[0] + 'px'; el.style.top = rp[1] + 'px';
    const r = rp[2];
    ring.style.width = ring.style.height = (2 * r) + 'px'; ring.style.left = ring.style.top = (-r) + 'px';
    ring.style.opacity = rp[3];
  });
}`;

function captionAt(t) {
  const fade = (t0, t1) => Math.max(0, Math.min(1, (t - t0) / .25, (t1 - t) / .25));
  if (t < 3.0) return ['出场动画', fade(.25, 3.0)];
  for (const r of REACTS) if (t >= r.start - .5 && t < r.start + r.dur + .3) return [r.label, fade(r.start - .5, r.start + r.dur + .3)];
  return ['', 0];
}

function pressAt(t) {   // 点下去的一瞬间鼠标缩一下
  for (const r of REACTS) for (const tc of [r.start - .15, r.start]) if (t >= tc - .02 && t < tc + .08) return .82;
  return 1;
}

function ripplesAt(t) {
  // 每次双击两个波纹：第一下在动作开始前 0.15 秒，第二下在动作开始时
  const out = [[0, 0, 0, 0], [0, 0, 0, 0]];
  for (const r of REACTS) [r.start - .15, r.start].forEach((tc, i) => {
    const k = (t - tc) / .35;
    if (k >= 0 && k <= 1) out[i] = [r.click[0], r.click[1], 6 + 20 * ease(k), 1 - k];
  });
  return out;
}

// ---------- Chrome DevTools 协议 ----------
function findChrome() {
  if (process.env.CHROME) return process.env.CHROME;
  const cache = path.join(os.homedir(), 'Library/Caches/ms-playwright');
  if (fs.existsSync(cache)) {
    for (const d of fs.readdirSync(cache).filter((n) => n.startsWith('chromium_headless_shell')).sort().reverse()) {
      const p = path.join(cache, d, 'chrome-headless-shell-mac-arm64', 'chrome-headless-shell');
      if (fs.existsSync(p)) return p;
    }
  }
  return '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
}

async function main() {
  const svg = fs.readFileSync(path.join(HERE, '..', 'erika-icon-animated.svg'), 'utf8');
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'erika-gif-'));
  const html = path.join(tmp, 'demo.html');
  fs.writeFileSync(html, pageHtml(svg));

  const chrome = spawn(findChrome(), ['--headless', '--remote-debugging-port=0', `--user-data-dir=${path.join(tmp, 'profile')}`,
    '--hide-scrollbars', '--force-device-scale-factor=1', '--no-first-run', 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
  const wsUrl = await new Promise((ok, fail) => {
    let buf = '';
    chrome.stderr.on('data', (d) => { buf += d; const m = buf.match(/ws:\/\/[^\s]+/); if (m) ok(m[0]); });
    chrome.on('exit', () => fail(new Error('Chrome 没有启动：\n' + buf)));
  });
  const port = new URL(wsUrl).port;
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  const ws = new WebSocket(targets.find((t) => t.type === 'page').webSocketDebuggerUrl);
  await new Promise((ok) => ws.addEventListener('open', ok));
  let seq = 0;
  const waiting = new Map();
  ws.addEventListener('message', (e) => {
    const m = JSON.parse(e.data);
    if (m.id && waiting.has(m.id)) { waiting.get(m.id)(m); waiting.delete(m.id); }
  });
  const send = (method, params = {}) => new Promise((ok, fail) => {
    const id = ++seq;
    waiting.set(id, (m) => (m.error ? fail(new Error(method + ': ' + m.error.message)) : ok(m.result)));
    ws.send(JSON.stringify({ id, method, params }));
  });

  try {
    await send('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false });
    await send('Page.enable');
    await send('Page.navigate', { url: 'file://' + html });
    await new Promise((ok) => setTimeout(ok, 800));
    const frames = path.join(tmp, 'frames');
    fs.mkdirSync(frames);
    const n = Math.round(END * FPS);
    const reacts = JSON.stringify(REACTS.map(({ name, start, dur }) => ({ name, start, dur })));
    for (let i = 0; i < n; i++) {
      const t = i / FPS;
      const expr = `(${SET_FRAME})(${t}, ${reacts}, ${JSON.stringify(cursorAt(t))}, ${JSON.stringify(captionAt(t))}, ${JSON.stringify(ripplesAt(t))}, ${pressAt(t)})`;
      const r = await send('Runtime.evaluate', { expression: expr });
      if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
      const shot = await send('Page.captureScreenshot', { format: 'png' });
      fs.writeFileSync(path.join(frames, String(i).padStart(4, '0') + '.png'), Buffer.from(shot.data, 'base64'));
    }
    fs.mkdirSync(path.dirname(OUT), { recursive: true });
    execFileSync('ffmpeg', ['-v', 'error', '-y', '-framerate', String(FPS), '-i', path.join(frames, '%04d.png'),
      '-vf', 'split[a][b];[a]palettegen=max_colors=160:stats_mode=full[p];[b][p]paletteuse=dither=bayer:bayer_scale=5',
      '-loop', '0', OUT]);
    console.log(`${OUT}  ${n} 帧，${END}s，${(fs.statSync(OUT).size / 1024 / 1024).toFixed(2)} MB`);
    if (process.env.KEEP_FRAMES) console.log('帧：' + frames);
  } finally {
    ws.close();
    chrome.kill();
    if (!process.env.KEEP_FRAMES) fs.rmSync(tmp, { recursive: true, force: true });
  }
}

main().catch((e) => { console.error(e); process.exit(1); });
