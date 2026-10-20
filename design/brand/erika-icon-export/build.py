#!/usr/bin/env python3
"""把 Erika 图标导出成各平台需要的全部格式。

    python3 design/brand/erika-icon-export/build.py

来源是图标工坊里的两个快照（design/brand/icon-designs/）：
  圆形版   绘梨花·多边形帽（你的版本）
  正方形版 绘梨花·正方形版
直接拼装工坊导出的 SVG 里的路径，不重画。设计改了以后，在工坊里重新保存这两个快照，再跑一遍本脚本。

需要：rsvg-convert（librsvg）、magick（ImageMagick）、iconutil（macOS 自带）。
"""
import json
import math
import os
import re
import shutil
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))
DESIGNS = os.path.join(HERE, '..', 'icon-designs')
CIRCLE, SQUARE = '绘梨花·多边形帽（你的版本）', '绘梨花·正方形版'
HAIR = '#3E5BD8'
HOLES = {'erika_eyeL', 'erika_eyeR', 'erika_mouth', 'erika_ribbonTail'}   # 单色版里挖空：眼睛、嘴、竖发绳
SIZES = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
RING = 48      # 单色版圆环的粗细，跨在圆边上（内外各 24）；耳尖和蝴蝶结离圆边只有 52，环往里只占一半，还留 28 的缝
HAT_GAP = 14   # 单色版帽子四周挖的缝


def load(name):
    d = json.load(open(os.path.join(DESIGNS, name + '.json'), encoding='utf-8'))
    svg = open(os.path.join(DESIGNS, name + '.svg'), encoding='utf-8').read()
    els = re.findall(r'<path\b[^>]*/>', svg)
    ids = [s['id'] for s in d['shapes'] if not s.get('hidden')]
    assert len(els) == len(ids), name
    return list(zip(ids, els))


def doc(view, body, w=1024, h=1024):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view}" width="{w}" height="{h}">{body}</svg>\n'


def paint(el, color):
    el = re.sub(r'fill="(?!none)[^"]*"', f'fill="{color}"', el)
    return re.sub(r'stroke="[^"]*"', f'stroke="{color}"', el)


def mono(pairs, color, box, ring=False):
    """单色：按图层顺序画遮罩，有墨的画白、要挖空的画黑，后画的盖住先画的；再用这个遮罩给一整块颜色。
    ring=True（无背景的纯黑、纯白和 Linux symbolic）：先画圆的轮廓环，帽子四周再挖一圈缝，让帽子和圆环、耳朵分开。
    安卓、iOS 的单色层由系统按自己的形状裁切，不画圆环"""
    x, y, w, h = box
    parts = f'<circle cx="512" cy="512" r="512" fill="none" stroke="white" stroke-width="{RING}"/>' if ring else ''
    for sid, el in pairs:
        if sid == 'erika_hair':
            continue
        if ring and not sid.startswith('erika_'):   # 帽子（六边形）
            parts += paint(el, 'black').replace('<path ', f'<path stroke="black" stroke-width="{2 * HAT_GAP}" stroke-linejoin="round" ', 1)
        parts += paint(el, 'black' if sid in HOLES else 'white')
    return (f'<defs><mask id="m" maskUnits="userSpaceOnUse" x="{x}" y="{y}" width="{w}" height="{h}">'
            f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="black"/>{parts}</mask></defs>'
            f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{color}" mask="url(#m)"/>')


def face_to(y_end):
    """正方形版的脸：两侧外轮廓沿原来那条三次曲线延伸到 y=1032（与工坊里一致），
    再顺着曲线末端的切线方向一直延伸到 y_end（只在安卓自适应图标的视差余量里露出来）"""
    P0, P1, P2, P3 = (765.73, 128.12), (830, 160), (920, 560), (892, 855)
    B = lambda t: tuple((1-t)**3*a + 3*(1-t)**2*t*b + 3*(1-t)*t**2*c + t**3*d for a, b, c, d in zip(P0, P1, P2, P3))
    dB = lambda t: tuple(3*((1-t)**2*(b-a) + 2*(1-t)*t*(c-b) + t**2*(d-c)) for a, b, c, d in zip(P0, P1, P2, P3))
    lo, hi = 1.0, 1.5
    for _ in range(60):
        m = (lo + hi) / 2
        lo, hi = (m, hi) if B(m)[1] < 1032 else (lo, m)
    T = (lo + hi) / 2
    Q1 = tuple(a + (b - a) * T for a, b in zip(P0, P1))
    Q2 = tuple((1-T)**2*a + 2*T*(1-T)*b + T**2*c for a, b, c in zip(P0, P1, P2))
    Q3 = B(T)
    tx, ty = dB(T)
    E = (Q3[0] + tx * (y_end - Q3[1]) / ty, y_end) if y_end > Q3[1] else Q3
    f = lambda v: ('%.2f' % v).rstrip('0').rstrip('.')
    p = lambda q: f(q[0]) + ' ' + f(q[1])
    mx = lambda q: (1024 - q[0], q[1])
    d = ('M332.7 344L691.3 344C719.74 344 739.28 321.03 734.74 292.97L714.19 166.12C708.04 128.11 731.23 111 765.73 128.12'
         f'C{p(Q1)} {p(Q2)} {p(Q3)}L{p(E)}L{p(mx(E))}L{p(mx(Q3))}C{p(mx(Q2))} {p(mx(Q1))} 258.27 128.12'
         'C292.77 111 315.96 128.11 309.81 166.12L289.26 292.97C284.72 321.03 304.26 344 332.7 344Z')
    return f'<path d="{d}" fill="#222B4F"/>'


def continuous_rect(x, y, w, h, r):
    """苹果的连续圆角矩形，和系统 RoundedRectangle(style: .continuous) 是同一条路径（系数按圆角半径归一）。
    macOS 图标模板是 1024 画布里 (100, 100) 起的 824 方块、圆角 185.4。macOS 26 起，旧式 .icns 的底板
    必须是这个形状才按原样显示；形状不符（比如超椭圆）时，系统把整个图标缩小放进灰色底板。"""
    a, b, c, d, e, f, g = (k * r for k in (1.52866483, 1.08849296, 0.86840694, 0.07491100,
                                           0.63149399, 0.16906001, 0.37282401))

    def corner(cx, cy, back, out):
        # 从来的边上距角 a 处，经三段曲线，到去的边上距角 a 处
        pt = lambda p, q: '%.3f %.3f' % (cx + p * back[0] + q * out[0], cy + p * back[1] + q * out[1])
        return (f'L{pt(a, 0)}C{pt(b, 0)} {pt(c, 0)} {pt(e, d)}C{pt(g, f)} {pt(f, g)} {pt(d, e)}'
                f'C{pt(0, c)} {pt(0, b)} {pt(0, a)}')

    right, bottom = x + w, y + h
    return ('M%.3f %.3f' % (x + a, y) + corner(right, y, (-1, 0), (0, 1)) + corner(right, bottom, (0, -1), (-1, 0))
            + corner(x, bottom, (1, 0), (0, -1)) + corner(x, y, (0, 1), (1, 0)) + 'Z')


def squircle(cx, cy, a, n=5, steps=240):
    """超椭圆（和工坊预览同一个公式）；安卓的圆角方形预览用它"""
    pts = []
    for i in range(steps + 1):
        t = i / steps * 2 * math.pi
        c, s = math.cos(t), math.sin(t)
        pts.append((cx + a * math.copysign(abs(c) ** (2 / n), c), cy + a * math.copysign(abs(s) ** (2 / n), s)))
    return 'M' + 'L'.join('%.2f %.2f' % q for q in pts) + 'Z'


NECK = 1200       # 动画里的脸用带脖子的版本：往下延伸到 y=1200，蹦到最高（上移 34、拉长 5%）时下沿也还在圆外，不会露出底
EAR_TOP = 121.47  # 耳尖最高点；压扁拉伸的原点放在圆的最低点 y=1024（地面），和原来不带脖子时一样

E = 'cubic-bezier(.3, .7, .4, 1) both'
ANIM_CSS = """
    .hair, .face, .eye, .blink, .mouth, .tail, .loop, .hat, .spark { transform-box: fill-box; animation-fill-mode: both; animation-timing-function: cubic-bezier(.3, .7, .4, 1); }
    .hair  { transform-origin: 50% 50%;  animation-name: pop;    animation-duration: .55s; }
    .face  { transform-origin: 50% FACE_ORIGIN; animation-name: hop;    animation-duration: .7s;  animation-delay: .3s; }
    .eye   { transform-origin: 50% 100%; animation-name: eye;    animation-duration: .42s; }
    .eye.l { animation-delay: .75s; } .eye.r { animation-delay: .83s; }
    .blink { transform-origin: 50% 35%;  animation-name: blink;  animation-duration: .24s; animation-delay: 2.05s; animation-timing-function: ease-in-out; }
    .mouth { transform-origin: 50% 50%;  animation-name: pop2;    animation-duration: .4s;  animation-delay: .95s; }
    .tail  { transform-origin: 50% 0;    animation-name: unroll; animation-duration: .45s; animation-delay: 1.0s; }
    .loop  { transform-origin: 0 0;      animation-name: swing;  animation-duration: .95s; animation-delay: 1.05s; animation-timing-function: ease-in-out; }
    .hat   { transform-origin: 50% 100%; animation-name: hat;    animation-duration: .8s;  animation-delay: 1.15s; }
    .spark { transform-origin: 50% 50%;  animation-name: twinkle; animation-duration: .85s; }
    .s1 { animation-delay: 1.55s; } .s2 { animation-delay: 1.65s; } .s3 { animation-delay: 1.75s; }
    @keyframes pop     { 0% { transform: scale(0); } 55% { transform: scale(1.03); } 76% { transform: scale(.96); } 90% { transform: scale(1.01); } 100% { transform: none; } }
    @keyframes pop2    { 0% { transform: scale(0); } 55% { transform: scale(1.25); } 76% { transform: scale(.9); } 90% { transform: scale(1.04); } 100% { transform: none; } }
    @keyframes hop     { 0% { transform: translateY(1000px); } 55% { transform: translateY(-34px) scale(.96, 1.05); } 72% { transform: translateY(0) scale(1.06, .94); } 87% { transform: translateY(-6px) scale(.99, 1.01); } 100% { transform: none; } }
    @keyframes eye     { 0% { transform: scaleY(0); } 60% { transform: scaleY(1.18); } 82% { transform: scaleY(.94); } 100% { transform: none; } }
    @keyframes blink   { 0%, 100% { transform: none; } 45%, 55% { transform: scaleY(.08); } }
    @keyframes unroll  { 0% { transform: scaleY(0); } 70% { transform: scaleY(1.06); } 100% { transform: none; } }
    @keyframes swing   { 0% { transform: rotate(38deg); opacity: 0; } 12% { opacity: 1; } 40% { transform: rotate(-8deg); } 62% { transform: rotate(4deg); } 80% { transform: rotate(-1.5deg); } 100% { transform: none; opacity: 1; } }
    @keyframes hat     { 0% { transform: translateY(-470px) rotate(-38deg); opacity: 0; } 12% { opacity: 1; } 55% { transform: translateY(14px) rotate(9deg) scale(1.08, .9); } 72% { transform: translateY(-2px) rotate(-5deg) scale(.98, 1.02); } 88% { transform: translateY(2px) rotate(2deg); } 100% { transform: none; opacity: 1; } }
    @keyframes twinkle { 0% { transform: scale(0) rotate(-30deg); opacity: 0; } 35% { transform: scale(1.15) rotate(10deg); opacity: 1; } 70% { transform: scale(.9); opacity: 1; } 100% { transform: scale(0) rotate(20deg); opacity: 0; } }
    @media (prefers-reduced-motion: reduce) { .hair, .face, .eye, .blink, .mouth, .tail, .loop, .hat, .spark { animation: none; } .spark { opacity: 0; } }

    /* ---- 双击的随机小动作：根元素加上 do-wink / do-pet / do-smug，动画放完由脚本去掉 ---- */
    svg { -webkit-user-select: none; user-select: none; touch-action: manipulation; }
    .rx, .r-eL, .r-eR, .r-m, .x { transform-box: fill-box; transform-origin: 50% 50%; }
    /* 两个 .rx 组里各垫了一个同样大的透明框，百分比原点在两组里指向同一个点。帽子顶离画框只有 8，所以歪头都绕帽子正下方的点转，帽子只左右动、不往上冒 */
    .rx { transform-origin: 50% RX_GROUND; }                       /* 摸头：圆的最低点 (512, 1024) */
    .do-wink .rx { transform-origin: RX_HATX RX_GROUND; }          /* wink：帽子正下方的地面 (855, 1024) */
    .do-smug .rx { transform-origin: RX_HATX RX_MID; }             /* 坏笑：帽子正下方 (855, 500) */
    .r-eL, .r-eR { transform-origin: 50% 75%; }        /* 眼睛往下合上 */

    /* wink：左眼眨成弧线、甩出一颗小星星，歪头 */
    .do-wink .rx    { animation: wk-head 1.5s E; }
    .do-wink .r-eL  { animation: wk-shut 1.5s E; }
    .do-wink .winkL { animation: wk-arch 1.5s E; }
    .do-wink .r-m   { animation: wk-mouth 1.5s E; }
    .do-wink .wstar { animation: wk-star 1.5s E; }
    @keyframes wk-head  { 0% { transform: rotate(0); } 16% { transform: rotate(-8deg); } 28%, 78% { transform: rotate(-6deg); } 90% { transform: rotate(1.5deg); } 100% { transform: rotate(0); } }
    @keyframes wk-shut  { 0% { transform: scaleY(1); } 10%, 80% { transform: scaleY(0); } 90% { transform: scaleY(1.08); } 100% { transform: scaleY(1); } }
    @keyframes wk-arch  { 0%, 8% { opacity: 0; transform: scale(.3); } 18% { opacity: 1; transform: scale(1.15); } 28%, 80% { opacity: 1; transform: scale(1); } 84%, 100% { opacity: 0; transform: scale(.6); } }
    @keyframes wk-mouth { 0% { transform: scale(1); } 12% { transform: scale(1.25); } 24% { transform: scale(.92); } 34%, 100% { transform: scale(1); } }
    @keyframes wk-star  { 0%, 12% { opacity: 0; transform: translate(0, 0) rotate(0) scale(0); } 30% { opacity: 1; transform: translate(-50px, -55px) rotate(80deg) scale(1.15); } 55% { opacity: 1; transform: translate(-85px, -95px) rotate(150deg) scale(.9); } 72%, 100% { opacity: 0; transform: translate(-100px, -112px) rotate(200deg) scale(0); } }

    /* 摸头：像猫一样被按住、左右蹭，眼睛眯成「︶」，嘴变「ω」，脸红，冒小心心 */
    .do-pet .rx     { animation: pt-head 2.6s E; }
    .do-pet .r-eL, .do-pet .r-eR { animation: pt-shut 2.6s E; }
    .do-pet .calm   { animation: pt-calm 2.6s E; }
    .do-pet .r-m    { animation: pt-mouth 2.6s E; }
    .do-pet .omega  { animation: pt-omega 2.6s E; }
    .do-pet .blush  { animation: pt-blush 2.6s E; }
    .do-pet .h1     { animation: pt-heart1 2.6s E; }
    .do-pet .h2     { animation: pt-heart2 2.6s E; }
    @keyframes pt-head   { 0% { transform: rotate(0) scale(1, 1); } 8% { transform: rotate(0) scale(1.03, .92); } 20% { transform: rotate(2deg) scale(1.03, .94); } 33% { transform: rotate(-2deg) scale(1.03, .93); } 46% { transform: rotate(2deg) scale(1.03, .94); } 59% { transform: rotate(-2deg) scale(1.03, .93); } 72% { transform: rotate(1deg) scale(1.02, .95); } 84% { transform: rotate(0) scale(.99, 1.006); } 92% { transform: rotate(0) scale(1.005, .997); } 100% { transform: rotate(0) scale(1, 1); } }
    @keyframes pt-shut   { 0% { transform: scaleY(1); } 7%, 84% { transform: scaleY(0); } 92% { transform: scaleY(1.08); } 100% { transform: scaleY(1); } }
    @keyframes pt-calm   { 0%, 6% { opacity: 0; transform: scale(.4); } 14% { opacity: 1; transform: scale(1.12); } 22%, 84% { opacity: 1; transform: scale(1); } 88%, 100% { opacity: 0; transform: scale(1); } }
    @keyframes pt-mouth  { 0% { transform: scale(1); } 7%, 84% { transform: scale(0); } 94% { transform: scale(1.1); } 100% { transform: scale(1); } }
    @keyframes pt-omega  { 0%, 8% { opacity: 0; transform: scale(.3); } 16% { opacity: 1; transform: scale(1.15); } 24%, 84% { opacity: 1; transform: scale(1); } 88%, 100% { opacity: 0; transform: scale(1); } }
    @keyframes pt-blush  { 0%, 10% { opacity: 0; transform: scale(.5); } 22% { opacity: 1; transform: scale(1.06); } 30%, 82% { opacity: 1; transform: scale(1); } 90%, 100% { opacity: 0; transform: scale(1); } }
    @keyframes pt-heart1 { 0%, 18% { opacity: 0; transform: translate(0, 0) scale(.2); } 30% { opacity: 1; transform: translate(-6px, -30px) scale(1.1); } 50% { opacity: 1; transform: translate(-18px, -80px) scale(1); } 70%, 100% { opacity: 0; transform: translate(-26px, -140px) scale(.7); } }
    @keyframes pt-heart2 { 0%, 36% { opacity: 0; transform: translate(0, 0) scale(.2); } 48% { opacity: 1; transform: translate(6px, -30px) scale(1.1); } 66% { opacity: 1; transform: translate(18px, -80px) scale(1); } 84%, 100% { opacity: 0; transform: translate(26px, -140px) scale(.7); } }

    /* 绘梨花的招牌表情：眼皮压下来变成斜着的半眯眼，一边眉毛皱起，嘴角一歪坏笑，抬下巴，帽子闪一下 */
    .do-smug .rx    { animation: sm-head 2.8s E; }
    .do-smug .lid   { animation: sm-lid 2.8s E; }
    .do-smug .brow  { animation: sm-brow 2.8s E; }
    .do-smug .r-m   { animation: sm-mouth 2.8s E; }
    .do-smug .smirk { animation: sm-smirk 2.8s E; }
    .do-smug .glint { animation: sm-glint 2.8s E; }
    @keyframes sm-head  { 0% { transform: rotate(0); } 12% { transform: rotate(5deg); } 22%, 84% { transform: rotate(3deg); } 93% { transform: rotate(-1deg); } 100% { transform: rotate(0); } }
    @keyframes sm-lid   { 0%, 10% { opacity: 1; transform: translateY(-140px); } 20% { opacity: 1; transform: translateY(6px); } 27%, 82% { opacity: 1; transform: translateY(0); } 94%, 100% { opacity: 1; transform: translateY(-140px); } }
    @keyframes sm-brow  { 0%, 14% { opacity: 0; transform: translateY(-16px); } 24% { opacity: 1; transform: translateY(2px); } 30%, 84% { opacity: 1; transform: translateY(0); } 90%, 100% { opacity: 0; transform: translateY(0); } }
    @keyframes sm-mouth { 0% { transform: scale(1); } 10%, 86% { transform: scale(0); } 95% { transform: scale(1.1); } 100% { transform: scale(1); } }
    @keyframes sm-smirk { 0%, 10% { opacity: 0; transform: scale(.3); } 20% { opacity: 1; transform: scale(1.1); } 28%, 86% { opacity: 1; transform: scale(1); } 90%, 100% { opacity: 0; transform: scale(1); } }
    @keyframes sm-glint { 0%, 28% { opacity: 0; transform: rotate(0) scale(0); } 38% { opacity: 1; transform: rotate(60deg) scale(1.25); } 50% { opacity: 1; transform: rotate(100deg) scale(.9); } 62%, 100% { opacity: 0; transform: rotate(140deg) scale(0); } }
    @media (prefers-reduced-motion: reduce) { .rx { animation: none !important; } }
"""

# 圆角四角星：尖角星描一圈同色的圆角描边，四个尖都变成圆头
STAR = 'M0 -.8C.16 -.16 .16 -.16 .8 0C.16 .16 .16 .16 0 .8C-.16 .16 -.16 .16 -.8 0C-.16 -.16 -.16 -.16 0 -.8Z'
HEART = 'M0 .78C-.5 .42 -.92 .08 -.92 -.32C-.92 -.68 -.64 -.9 -.34 -.9C-.16 -.9 -.04 -.8 0 -.66C.04 -.8 .16 -.9 .34 -.9C.64 -.9 .92 -.68 .92 -.32C.92 .08 .5 .42 0 .78Z'
PALE, NAVY, PINK = '#DCE6FF', '#222B4F', '#FF2E55'
RX_BOX = '<rect x="-100" y="-100" width="1224" height="1400" fill="none"/>'   # 撑开 .rx 的外框（要盖住脖子 y=1200），让两组的变换原点一致

REACT_JS = """
(function () {
  // 双击随机做一个小动作（连续两次不重复）。只在 SVG 自己是文档时生效：直接打开、<object>、<embed>；放进 <img> 时脚本不运行
  var me = document.currentScript, svg = (me && me.ownerSVGElement) || document.documentElement;
  var acts = { wink: 1500, pet: 2600, smug: 2800 }, names = Object.keys(acts), last = '', timer = 0;
  if (svg === document.documentElement) { svg.setAttribute('width', '100%'); svg.setAttribute('height', '100%'); }
  var calm = window.matchMedia && matchMedia('(prefers-reduced-motion: reduce)').matches;
  var ready = Date.now() + (calm ? 0 : 2300);   // 出场动画放完再接受双击
  svg.addEventListener('dblclick', function (e) {
    e.preventDefault();
    if (Date.now() < ready) return;
    var pool = names.filter(function (n) { return n !== last; });
    var a = pool[Math.floor(Math.random() * pool.length)];
    last = a;
    names.forEach(function (n) { svg.classList.remove('do-' + n); });
    svg.getBoundingClientRect();   // 强制重算样式，动画才会从头播
    svg.classList.add('do-' + a);
    clearTimeout(timer);
    timer = setTimeout(function () { svg.classList.remove('do-' + a); }, acts[a]);
  });
})();
"""


def animated_logo(pairs):
    """主 logo：出场动画 + 双击随机小动作（都写在 SVG 里）。
    出场动画是纯 CSS，放进 <img> 和 GitHub README 都会播放，播一次停在最后一帧，和静态 logo 完全一样（小星星淡出）。
    双击的小动作要靠 SVG 里的脚本，所以只在直接打开或用 <object> 嵌入时有；多出来的表情零件平时 opacity=0。
    每个零件外面包一层 <g> 来做动画，路径自己的 transform 不动"""
    el = dict(pairs)
    known = {'erika_hair', 'erika_face', 'erika_ribbonTail', 'erika_ribbonLoop', 'erika_eyeL', 'erika_eyeR', 'erika_mouth'}
    hat = ''.join(e for sid, e in pairs if sid not in known)
    def mark(d, x, y, s, c, sw):
        return f'<path fill="{c}" stroke="{c}" stroke-width="{sw}" stroke-linejoin="round" transform="translate({x} {y}) scale({s})" d="{d}"/>'
    star = lambda x, y, s, c, k: f'<g class="spark {k}">{mark(STAR, x, y, s, c, .34)}</g>'
    line = lambda d, x, y, w=24: (f'<path d="{d}" fill="none" stroke="{PALE}" stroke-width="{w}" stroke-linecap="round" '
                                  f'stroke-linejoin="round" transform="translate({x} {y})"/>')
    extra = lambda cls, inner: f'<g class="x {cls}" opacity="0">{inner}</g>'   # 平时看不见的表情零件
    # 半眯眼的眼皮：深蓝色斜块，从眼睛上沿压下来（左眼压得更低，内侧低外侧高）。
    # 眼皮比眼睛左右各宽 6，盖住眼睛边缘的抗锯齿；裁在「眼睛外框再放宽 6」的矩形里，压下来之前藏在框外
    lid = lambda pts: extra('lid', f'<path fill="{NAVY}" d="M{pts}Z"/>')
    eye = lambda sid, k, cls, lidpts: (
        f'<g class="eye {k}"><g class="blink"><g class="{cls}">{el[sid]}</g>'
        f'<g clip-path="url(#cl{k})">{lid(lidpts)}</g></g></g>')
    css = (ANIM_CSS.replace('FACE_ORIGIN', '%.2f%%' % ((1024 - EAR_TOP) / (NECK - EAR_TOP) * 100))
                   .replace('RX_GROUND', '%.2f%%' % ((1024 + 100) / 1400 * 100)).replace('RX_MID', '%.2f%%' % ((500 + 100) / 1400 * 100))
                   .replace('RX_HATX', '%.2f%%' % ((855 + 100) / 1224 * 100)).replace(' E;', f' {E};'))
    defs = ('<defs><clipPath id="ec"><circle cx="512" cy="512" r="512"/></clipPath>'
            '<clipPath id="cll"><rect x="300.5" y="422" width="111" height="228"/></clipPath>'
            '<clipPath id="clr"><rect x="612.5" y="422" width="111" height="228"/></clipPath></defs>')
    body = (f'<style>{css}</style>{defs}'
            f'<g class="hair">{el["erika_hair"]}</g>'
            f'<g clip-path="url(#ec)"><g class="rx">{RX_BOX}<g class="face">{face_to(NECK)}</g></g></g>'
            f'<g class="rx">{RX_BOX}'
            f'<g class="tail">{el["erika_ribbonTail"]}</g>'
            f'<g class="loop">{el["erika_ribbonLoop"]}</g>'
            + eye('erika_eyeL', 'l', 'r-eL', '300 420L412 420L412 548L300 514')
            + eye('erika_eyeR', 'r', 'r-eR', '612 420L724 420L724 506L612 534')
            + extra('winkL', line('M-50 20Q0 -48 50 20', 356, 588, 26))
            + extra('calm', line('M-46 -14Q0 40 46 -14', 356, 575)) + extra('calm', line('M-46 -14Q0 40 46 -14', 668, 575))
            + extra('brow', line('M-50 -14L50 14', 352, 390, 18) + line('M-50 6L50 -6', 670, 392, 18))
            + extra('blush', f'<ellipse cx="326" cy="680" rx="42" ry="19" fill="{PINK}"/>')
            + extra('blush', f'<ellipse cx="698" cy="680" rx="42" ry="19" fill="{PINK}"/>')
            + f'<g class="mouth"><g class="r-m">{el["erika_mouth"]}</g></g>'
            + extra('omega', line('M-50 -8Q-48 18 -25 18Q-2 18 0 -4Q2 18 25 18Q48 18 50 -8', 512, 712, 20))
            + extra('smirk', line('M-84 -14Q-14 34 86 -28', 512, 712, 20))
            + f'<g class="hat">{hat}</g>'
            + extra('wstar', mark(STAR, 300, 452, 34, '#F3E1A9', .34))
            + extra('glint', mark(STAR, 720, 74, 46, '#F3E1A9', .34))
            + extra('h1', mark(HEART, 430, 286, 30, PINK, .2))
            + extra('h2', mark(HEART, 594, 276, 24, PINK, .2))
            + '</g>'
            + star(686, 76, 54, '#F3E1A9', 's1') + star(984, 352, 42, '#F48BA6', 's2') + star(970, 36, 32, '#D340AD', 's3')
            + f'<script><![CDATA[{REACT_JS}]]></script>')
    note = ('<title>Erika</title><!--\n  Erika 图标 · 出场动画（纯 CSS，放进 <img> 和 GitHub README 都会播放；系统开了「减少动态效果」就直接显示）：\n'
            '    0.00s 头发的圆弹出来（果冻回弹）\n    0.30s 猫脸从底下蹦上来，落地压扁再弹回\n'
            '    0.75s 两只眼睛先后弹开 · 0.95s「^」嘴冒出来\n    1.00s 米黄竖发绳从上往下展开 · 1.05s 品红蝴蝶结圈绕顶点像钟摆一样晃几下\n'
            '    1.15s 帽子转着掉下来，落在耳朵上压扁、回弹、摆正\n    1.55s 帽子周围闪三颗圆角小星星，淡出\n'
            '    2.05s 眨一下眼；全程约 2.3s，停在最后一帧（和静态 logo 一样）\n'
            '  双击随机做一个小动作（要直接打开或用 <object> 嵌入，<img> 里不运行脚本）：\n'
            '    wink   左眼眨成弧线、甩出一颗小星星，歪头（1.5s）\n'
            '    摸头   像猫一样被按住左右蹭，眼睛眯成「︶」，嘴变「ω」，脸红，冒小心心（2.6s）\n'
            '    坏笑   古户绘梨花的招牌表情：眼皮压成斜的半眯眼，皱一边眉毛，嘴角一歪，抬下巴，帽子闪一下（2.8s）\n-->')
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="-16 -16 1056 1056" width="1024" height="1024" role="img" aria-label="Erika">'
            f'{note}{body}</svg>\n')


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'w', encoding='utf-8') as f:
        f.write(text)


def png(svg_path, out, size, opaque=None):
    os.makedirs(os.path.dirname(out), exist_ok=True)
    subprocess.run(['rsvg-convert', '-w', str(size), '-h', str(size), svg_path, '-o', out], check=True)
    if opaque:   # iOS / apple-touch 不允许透明
        subprocess.run(['magick', out, '-background', opaque, '-alpha', 'remove', '-alpha', 'off', out], check=True)


def bbox(svg_text, view_size):
    """渲染一次，用 ImageMagick 量出非透明内容的外框（单位 = 画板单位）"""
    tmp = os.path.join(HERE, '.bbox.svg')
    write(tmp, svg_text)
    out = subprocess.run(['sh', '-c', f'rsvg-convert -w {view_size} -h {view_size} "{tmp}" | magick png:- -format "%@" info:'],
                         capture_output=True, text=True, check=True).stdout
    os.remove(tmp)
    w, h, x, y = map(int, re.match(r'(\d+)x(\d+)\+(\d+)\+(\d+)', out).groups())
    return x, y, w, h


def export_tvos(S, J):
    """tvOS：分层图标（.brandassets），和安卓自适应图标同样两层：背景是头发的蓝，前景是去掉头发的脸、帽子和发饰，
    遥控器拨动焦点时前景在背景上方视差浮动。主屏幕图标 400×240（@1x、@2x），App Store 图标 1280×768，
    Top Shelf 横幅 1920×720 与宽版 2320×720（@1x、@2x）。前景按画面高度放，脸往下多出的一段给视差留余量。"""
    fg_pairs = [(sid, face_to(1300) if sid == 'erika_face' else el) for sid, el in S if sid != 'erika_hair']
    fg_body = ''.join(el for _, el in fg_pairs)
    root = J('tvos', 'Erika.brandassets')
    info = {'author': 'xcode', 'version': 1}

    def wide(w, h, share):
        """把 1024 见方的画稿放进 w×h 的画面：高度占 share，水平、垂直都居中"""
        side = 1024 / share
        width = side * w / h
        return '%.2f %.2f %.2f %.2f' % (512 - width / 2, 512 - side / 2, width, side)

    def layer_png(name, body, view, w, h, out):
        svg = os.path.join(root, f'_{name}.svg')
        write(svg, f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view}" width="{w}" height="{h}">{body}</svg>\n')
        os.makedirs(os.path.dirname(out), exist_ok=True)
        subprocess.run(['rsvg-convert', '-w', str(w), '-h', str(h), svg, '-o', out], check=True)
        os.remove(svg)

    def back(view):
        x, y, w, h = map(float, view.split())
        return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{HAIR}"/>'

    for stack, (w, h), scales in (('App Icon.imagestack', (400, 240), (1, 2)), ('App Icon - App Store.imagestack', (1280, 768), (1,))):
        view = wide(w, h, 0.92)
        for name, body in (('Front', fg_body), ('Back', back(view))):
            images = []
            for k in scales:
                file = f'{name.lower()}.png' if k == 1 else f'{name.lower()}@{k}x.png'
                layer_png(name, body, view, w * k, h * k, os.path.join(root, stack, f'{name}.imagestacklayer', 'Content.imageset', file))
                images.append({'filename': file, 'idiom': 'tv', 'scale': f'{k}x'})
            write(os.path.join(root, stack, f'{name}.imagestacklayer', 'Content.imageset', 'Contents.json'),
                  json.dumps({'images': images, 'info': info}, indent=2) + '\n')
            write(os.path.join(root, stack, f'{name}.imagestacklayer', 'Contents.json'), json.dumps({'info': info}, indent=2) + '\n')
        write(os.path.join(root, stack, 'Contents.json'),
              json.dumps({'info': info, 'layers': [{'filename': 'Front.imagestacklayer'}, {'filename': 'Back.imagestacklayer'}]}, indent=2) + '\n')
    for name, (w, h) in (('Top Shelf Image', (1920, 720)), ('Top Shelf Image Wide', (2320, 720))):
        view = wide(w, h, 0.62)
        images = []
        for k in (1, 2):
            file = 'top-shelf.png' if k == 1 else f'top-shelf@{k}x.png'
            layer_png(name.replace(' ', '-'), back(view) + fg_body, view, w * k, h * k, os.path.join(root, f'{name}.imageset', file))
            images.append({'filename': file, 'idiom': 'tv', 'scale': f'{k}x'})
        write(os.path.join(root, f'{name}.imageset', 'Contents.json'), json.dumps({'images': images, 'info': info}, indent=2) + '\n')
    write(os.path.join(root, 'Contents.json'), json.dumps({'assets': [
        {'filename': 'App Icon - App Store.imagestack', 'idiom': 'tv', 'role': 'primary-app-icon', 'size': '1280x768'},
        {'filename': 'App Icon.imagestack', 'idiom': 'tv', 'role': 'primary-app-icon', 'size': '400x240'},
        {'filename': 'Top Shelf Image Wide.imageset', 'idiom': 'tv', 'role': 'top-shelf-image-wide', 'size': '2320x720'},
        {'filename': 'Top Shelf Image.imageset', 'idiom': 'tv', 'role': 'top-shelf-image', 'size': '1920x720'},
    ], 'info': info}, indent=2) + '\n')


def export_macos(S, J):
    """macOS：正方形版放进苹果模板的连续圆角底板（824，圆角 185.4），带投影，打成 .icns"""
    plate = continuous_rect(100, 100, 824, 824, 185.4)
    mac = doc('0 0 1024 1024',
              f'<defs><clipPath id="p"><path d="{plate}"/></clipPath>'
              '<filter id="sh" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur in="SourceAlpha" stdDeviation="12"/>'
              '<feOffset dy="10" result="b"/><feFlood flood-color="#000" flood-opacity=".28"/><feComposite in2="b" operator="in"/>'
              '<feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge></filter></defs>'
              f'<g filter="url(#sh)"><path d="{plate}" fill="{HAIR}"/></g>'
              '<g clip-path="url(#p)"><g transform="translate(100 100) scale(0.8046875)">' + ''.join(el for _, el in S) + '</g></g>')
    write(J('macos', 'erika-macos.svg'), mac)
    iconset = J('macos', 'Erika.iconset')
    for n in (16, 32, 128, 256, 512):
        png(J('macos', 'erika-macos.svg'), os.path.join(iconset, f'icon_{n}x{n}.png'), n)
        png(J('macos', 'erika-macos.svg'), os.path.join(iconset, f'icon_{n}x{n}@2x.png'), n * 2)
    subprocess.run(['iconutil', '-c', 'icns', iconset, '-o', J('macos', 'Erika.icns')], check=True)
    png(J('macos', 'erika-macos.svg'), J('macos', 'erika-macos-1024.png'), 1024)


def main():
    out = HERE
    for sub in ('logo', 'preview', 'svg', 'png', 'macos', 'windows', 'linux', 'ios', 'tvos', 'android', 'harmonyos', 'web'):
        shutil.rmtree(os.path.join(out, sub), ignore_errors=True)
    C, S = load(CIRCLE), load(SQUARE)
    J = lambda *a: os.path.join(out, *a)

    # ---------- 基础 SVG ----------
    circle = doc('0 0 1024 1024', '<defs><clipPath id="c"><circle cx="512" cy="512" r="512"/></clipPath></defs>'
                 '<g clip-path="url(#c)">' + ''.join(el for _, el in C) + '</g>')
    square = doc('0 0 1024 1024', ''.join(el for _, el in S))
    write(J('svg', 'erika-circle.svg'), circle)
    write(J('svg', 'erika-square.svg'), square)

    # 单色（无背景）：用圆形版的造型（下巴是圆弧），保留圆的轮廓环，六边形完整保留；按内容外框居中，四周留 4%
    big = (-256, -256, 1536, 1536)
    raw = doc('-256 -256 1536 1536', mono(C, '#000000', big, ring=True), 1536, 1536)
    x, y, w, h = bbox(raw, 1536)
    x, y = x - 256, y - 256
    side = max(w, h) * 1.08
    view = '%.1f %.1f %.1f %.1f' % (x + w / 2 - side / 2, y + h / 2 - side / 2, side, side)
    for name, color in (('black', '#000000'), ('white', '#FFFFFF')):
        write(J('svg', f'erika-mono-{name}.svg'), doc(view, mono(C, color, big, ring=True)))

    # 前景 / 背景两层（正方形版的造型，去掉头发；脸往下多延伸，给安卓视差余量）
    fg_pairs = [(sid, face_to(1300) if sid == 'erika_face' else el) for sid, el in S if sid != 'erika_hair']
    fg_body = ''.join(el for _, el in fg_pairs)
    write(J('svg', 'erika-foreground.svg'), doc('0 0 1024 1024', fg_body))
    write(J('svg', 'erika-background.svg'), doc('0 0 1024 1024', f'<rect width="1024" height="1024" fill="{HAIR}"/>'))

    # ---------- 对外展示的主 logo：圆形，发饰（六边形）不裁切、完整地超出圆；画板四周放宽 16，圆仍居中 ----------
    logo = doc('-16 -16 1056 1056', ''.join(el for _, el in C))
    write(J('logo', 'erika-logo-icon.svg'), logo)
    write(J('..', 'erika-icon.svg'), logo)
    anim = animated_logo(C)
    write(J('logo', 'erika-logo-icon-animated.svg'), anim)
    write(J('..', 'erika-icon-animated.svg'), anim)
    for n in (64, 128, 256, 512, 1024, 2048):
        png(J('logo', 'erika-logo-icon.svg'), J('logo', f'erika-logo-icon-{n}.png'), n)

    # ---------- 通用 PNG ----------
    for key in ('circle', 'square', 'mono-black', 'mono-white', 'foreground'):
        for n in SIZES:
            png(J('svg', f'erika-{key}.svg'), J('png', key, f'erika-{key}-{n}.png'), n)

    export_macos(S, J)

    # ---------- Windows：圆形版 .ico ----------
    win = [J('windows', f'_{n}.png') for n in (16, 24, 32, 48, 64, 128, 256)]
    for n, p in zip((16, 24, 32, 48, 64, 128, 256), win):
        png(J('svg', 'erika-circle.svg'), p, n)
    subprocess.run(['magick', *win, J('windows', 'erika.ico')], check=True)
    for p in win:
        os.remove(p)

    # ---------- Linux：hicolor 主题目录（圆形版）+ 矢量 + 单色 symbolic ----------
    for n in (16, 22, 24, 32, 48, 64, 96, 128, 256, 512):
        png(J('svg', 'erika-circle.svg'), J('linux', 'hicolor', f'{n}x{n}', 'apps', 'erika.png'), n)
    shutil.copy(J('svg', 'erika-circle.svg'), os.path.join(os.makedirs(J('linux', 'hicolor', 'scalable', 'apps'), exist_ok=True) or J('linux', 'hicolor', 'scalable', 'apps'), 'erika.svg'))
    os.makedirs(J('linux', 'hicolor', 'symbolic', 'apps'), exist_ok=True)
    write(J('linux', 'hicolor', 'symbolic', 'apps', 'erika-symbolic.svg'), doc(view, mono(C, '#2E3436', big, ring=True), 16, 16))

    # ---------- iOS：AppIcon（不透明正方形）+ 着色版 ----------
    ios = J('ios', 'AppIcon.appiconset')
    png(J('svg', 'erika-square.svg'), os.path.join(ios, 'AppIcon.png'), 1024, opaque=HAIR)
    # 深色版不单独提供：脸是深蓝色，放在深色底上看不见；不给的话 iOS 会从主图自动生成深色版
    write(J('ios', '_tinted.svg'), doc('0 0 1024 1024', '<rect width="1024" height="1024" fill="#000"/>' + mono(fg_pairs, '#FFFFFF', (0, 0, 1024, 1024))))
    png(J('ios', '_tinted.svg'), os.path.join(ios, 'AppIcon-tinted.png'), 1024, opaque='#000000')
    os.remove(J('ios', '_tinted.svg'))
    write(os.path.join(ios, 'Contents.json'), json.dumps({'images': [
        {'filename': 'AppIcon.png', 'idiom': 'universal', 'platform': 'ios', 'size': '1024x1024'},
        {'appearances': [{'appearance': 'luminosity', 'value': 'tinted'}], 'filename': 'AppIcon-tinted.png', 'idiom': 'universal', 'platform': 'ios', 'size': '1024x1024'},
    ], 'info': {'author': 'xcode', 'version': 1}}, indent=2) + '\n')

    export_tvos(S, J)

    # ---------- 安卓：自适应图标（108dp 图层，设计占中间 72dp）+ 单色层 + 旧版图标 + 商店图标 ----------
    layer = '-256 -256 1536 1536'
    write(J('android', 'layers', 'foreground.svg'), doc(layer, fg_body))
    write(J('android', 'layers', 'monochrome.svg'), doc(layer, mono(fg_pairs, '#000000', (-256, -256, 1536, 1536))))
    write(J('android', 'layers', 'background.svg'), doc(layer, f'<rect x="-256" y="-256" width="1536" height="1536" fill="{HAIR}"/>'))
    res = J('android', 'res')
    for dpi, k in (('mdpi', 1), ('hdpi', 1.5), ('xhdpi', 2), ('xxhdpi', 3), ('xxxhdpi', 4)):
        png(J('android', 'layers', 'foreground.svg'), os.path.join(res, f'mipmap-{dpi}', 'ic_launcher_foreground.png'), int(108 * k))
        png(J('android', 'layers', 'monochrome.svg'), os.path.join(res, f'mipmap-{dpi}', 'ic_launcher_monochrome.png'), int(108 * k))
        png(J('svg', 'erika-circle.svg'), os.path.join(res, f'mipmap-{dpi}', 'ic_launcher.png'), int(48 * k))
        png(J('svg', 'erika-circle.svg'), os.path.join(res, f'mipmap-{dpi}', 'ic_launcher_round.png'), int(48 * k))
    adaptive = ('<?xml version="1.0" encoding="utf-8"?>\n<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">\n'
                '    <background android:drawable="@color/ic_launcher_background"/>\n'
                '    <foreground android:drawable="@mipmap/ic_launcher_foreground"/>\n'
                '    <monochrome android:drawable="@mipmap/ic_launcher_monochrome"/>\n</adaptive-icon>\n')
    write(os.path.join(res, 'mipmap-anydpi-v26', 'ic_launcher.xml'), adaptive)
    write(os.path.join(res, 'mipmap-anydpi-v26', 'ic_launcher_round.xml'), adaptive)
    write(os.path.join(res, 'values', 'ic_launcher_background.xml'),
          f'<?xml version="1.0" encoding="utf-8"?>\n<resources>\n    <color name="ic_launcher_background">{HAIR}</color>\n</resources>\n')
    png(J('svg', 'erika-square.svg'), J('android', 'playstore-icon-512.png'), 512, opaque=HAIR)

    # ---------- 鸿蒙：分层图标（前景 + 背景，1024） ----------
    hm = J('harmonyos', 'resources', 'base', 'media')
    png(J('svg', 'erika-foreground.svg'), os.path.join(hm, 'foreground.png'), 1024)
    png(J('svg', 'erika-background.svg'), os.path.join(hm, 'background.png'), 1024)
    write(os.path.join(hm, 'layered_image.json'), json.dumps({'layered-image': {'background': '$media:background', 'foreground': '$media:foreground'}}, indent=2) + '\n')

    # ---------- 网页 ----------
    shutil.copy(J('svg', 'erika-circle.svg'), os.path.join(os.makedirs(J('web'), exist_ok=True) or J('web'), 'favicon.svg'))
    fav = [J('web', f'_{n}.png') for n in (16, 32, 48)]
    for n, p in zip((16, 32, 48), fav):
        png(J('svg', 'erika-circle.svg'), p, n)
    subprocess.run(['magick', *fav, J('web', 'favicon.ico')], check=True)
    for p in fav:
        os.remove(p)
    png(J('svg', 'erika-square.svg'), J('web', 'apple-touch-icon.png'), 180, opaque=HAIR)
    png(J('svg', 'erika-circle.svg'), J('web', 'icon-192.png'), 192)
    png(J('svg', 'erika-circle.svg'), J('web', 'icon-512.png'), 512)
    write(J('web', '_maskable.svg'), doc(layer, f'<rect x="-256" y="-256" width="1536" height="1536" fill="{HAIR}"/>' + fg_body))
    png(J('web', '_maskable.svg'), J('web', 'icon-maskable-512.png'), 512)
    os.remove(J('web', '_maskable.svg'))
    write(J('web', 'site.webmanifest'), json.dumps({'name': 'Erika', 'icons': [
        {'src': 'icon-192.png', 'sizes': '192x192', 'type': 'image/png'},
        {'src': 'icon-512.png', 'sizes': '512x512', 'type': 'image/png'},
        {'src': 'icon-maskable-512.png', 'sizes': '512x512', 'type': 'image/png', 'purpose': 'maskable'},
    ]}, ensure_ascii=False, indent=2) + '\n')
    # ---------- 效果预览（给文档和品牌页用）：安卓不同桌面的遮罩、主题图标 ----------
    def squircle4(a=512):
        return squircle(512, 512, a, 4)
    masks = {'android-circle': '<circle cx="512" cy="512" r="512"/>', 'android-squircle': f'<path d="{squircle4()}"/>',
             'android-teardrop': '<path d="M512 0H1024V512A512 512 0 0 1 512 1024A512 512 0 0 1 0 512A512 512 0 0 1 512 0Z"/>'}
    for key, m in masks.items():
        write(J('preview', f'_{key}.svg'), doc('0 0 1024 1024', f'<defs><clipPath id="k">{m}</clipPath></defs><g clip-path="url(#k)">'
              f'<rect x="-256" y="-256" width="1536" height="1536" fill="{HAIR}"/>{fg_body}</g>'))
    mono_white = mono(fg_pairs, '#FFFFFF', (-256, -256, 1536, 1536)).replace('id="m"', 'id="mw"').replace('url(#m)', 'url(#mw)')
    for key, bgc, fgc in (('android-themed-light', '#D7E3F7', '#1F3A60'), ('android-themed-dark', '#1E2633', '#C9D8F2')):
        write(J('preview', f'_{key}.svg'), doc('0 0 1024 1024', '<defs><clipPath id="k"><circle cx="512" cy="512" r="512"/></clipPath>'
              f'<mask id="t" maskUnits="userSpaceOnUse" x="-256" y="-256" width="1536" height="1536"><rect x="-256" y="-256" width="1536" height="1536" fill="black"/>'
              f'{mono_white}</mask></defs><g clip-path="url(#k)"><rect width="1024" height="1024" fill="{bgc}"/>'
              f'<rect x="-256" y="-256" width="1536" height="1536" fill="{fgc}" mask="url(#t)"/></g>'))
    for key in list(masks) + ['android-themed-light', 'android-themed-dark']:
        png(J('preview', f'_{key}.svg'), J('preview', f'{key}.png'), 512)
        os.remove(J('preview', f'_{key}.svg'))
    print('done')


if __name__ == '__main__':
    main()
