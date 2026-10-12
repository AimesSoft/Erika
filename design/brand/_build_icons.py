"""把手绘的播放图标方案装进 logo 与单独图标。图标本身的形状都写在下面的 OPTIONS 里（本地坐标：
三角形 A(0,-118) B(0,118) C(215,0)，外扩 18 的圆角轮廓是 #tri，#triClip 用来裁切）。"""
import re

BASE = open('nipaplay-logo-fresh.svg', encoding='utf-8').read()
T = 'translate(1046 270) rotate(-12) translate(-72 0)'

OPTIONS = {
    # A 软糖：两色切面 + 一道粗胶囊高光 + 一颗圆点，和字母上的高光同一种语言
    'gummy': ('软糖', '''
    <use href="#tri" fill="#8E7FFF"/>
    <rect x="-40" y="-170" width="300" height="170" fill="#ACA0FF" clip-path="url(#triClip)"/>
    <path d="M16.7 -86.1 L73.7 -54.8" stroke="#FFFFFF" stroke-opacity=".92" stroke-width="15" stroke-linecap="round"/>
    <circle cx="95.6" cy="-42.8" r="7.5" fill="#FFFFFF" fill-opacity=".92"/>'''),
    # B 星星：实心三角形中间一颗大白星，呼应「ニパ～☆」
    'star': ('星星', '''
    <use href="#tri" fill="#8E7FFF"/>
    <rect x="-40" y="-170" width="300" height="170" fill="#ACA0FF" clip-path="url(#triClip)"/>
    <path d="M94.6 -50.7 L98.6 -20.2 L127.9 -11.1 L100.1 2.1 L100.5 32.8 L79.4 10.4 L50.3 20.3 L65 -6.7 L46.7 -31.3 L76.9 -25.6 Z" fill="#FFFFFF" stroke="#FFFFFF" stroke-width="7" stroke-linejoin="round"/>'''),
    # C 糖条：沿上沿的四条粗色带，把字母的四种颜色收进图标里
    'candy': ('糖条', '''
    <g clip-path="url(#triClip)">
      <g transform="translate(0 -118) rotate(28.76)">
        <rect x="-120" y="-40" width="420" height="82" fill="#FF7FA8"/>
        <rect x="-120" y="42" width="420" height="44" fill="#FFB938"/>
        <rect x="-120" y="86" width="420" height="44" fill="#2FCB9E"/>
        <rect x="-120" y="130" width="420" height="140" fill="#3CB8F4"/>
      </g>
    </g>
    <circle cx="42" cy="-76" r="7.5" fill="#FFFFFF" fill-opacity=".9"/>'''),
    # D 大格子：粗格纹（每格 44），缩小也能看出是格子
    'gingham': ('大格子', '''
    <g clip-path="url(#triClip)">
      <rect x="-40" y="-170" width="300" height="340" fill="#B3A9FF"/>
      <g fill="#7A6AF5" fill-opacity=".5">
        <rect x="-18" y="-170" width="22" height="340"/><rect x="26" y="-170" width="22" height="340"/>
        <rect x="70" y="-170" width="22" height="340"/><rect x="114" y="-170" width="22" height="340"/>
        <rect x="158" y="-170" width="22" height="340"/><rect x="202" y="-170" width="22" height="340"/>
        <rect x="-40" y="-147" width="300" height="22"/><rect x="-40" y="-103" width="300" height="22"/>
        <rect x="-40" y="-59" width="300" height="22"/><rect x="-40" y="-15" width="300" height="22"/>
        <rect x="-40" y="29" width="300" height="22"/><rect x="-40" y="73" width="300" height="22"/>
        <rect x="-40" y="117" width="300" height="22"/>
      </g>
    </g>'''),
}

# logo 里原来的图标块
a = BASE.index('  <!-- ③ 播放图标')
b = BASE.index('  <!-- 压在图标上的字母补一道白边 -->')
# logo 里图标的左下部分被字母 y 压住，星星挪到右上方并缩小一点；单独图标保持居中
LOGO_BODY = {
    'star': OPTIONS['star'][1].replace(
        'M94.6 -50.7 L98.6 -20.2 L127.9 -11.1 L100.1 2.1 L100.5 32.8 L79.4 10.4 L50.3 20.3 L65 -6.7 L46.7 -31.3 L76.9 -25.6 Z',
        'M128.7 -52.9 L131.9 -28 L155.9 -20.5 L133.1 -9.8 L133.5 15.4 L116.3 -3 L92.4 5.1 L104.5 -16.9 L89.5 -37.1 L114.2 -32.4 Z').replace('stroke-width="7"', 'stroke-width="6"'),
}
for key, (name, body) in OPTIONS.items():
    lbody = LOGO_BODY.get(key, body)
    block = f'  <!-- ③ 播放图标 · {name} -->\n  <g transform="{T}">{lbody}\n  </g>\n\n'
    svg = BASE[:a] + block + BASE[b:]
    svg = svg.replace('<title>NipaPlay · 清新配色</title>', f'<title>NipaPlay · 清新配色 · 播放图标「{name}」</title>')
    open(f'nipaplay-logo-{key}.svg', 'w', encoding='utf-8').write(svg)
    # 单独图标：同样的贴纸三层
    defs = re.search(r'<path id="tri" d="[^"]+"/>', BASE).group(0)
    icon = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="-200 -196 412 412" width="412" height="412" role="img" aria-label="NipaPlay 播放图标 · {name}">
  <defs>
    {defs}
    <clipPath id="triClip"><use href="#tri"/></clipPath>
  </defs>
  <g transform="translate(12 15) rotate(-12) translate(-72 0)"><use href="#tri" fill="#7DB4F2" stroke="#7DB4F2" stroke-width="44" stroke-linejoin="round"/></g>
  <g transform="rotate(-12) translate(-72 0)">
    <use href="#tri" fill="#FFFFFF" stroke="#FFFFFF" stroke-width="44" stroke-linejoin="round"/>{body}
  </g>
</svg>
'''
    open(f'play-{key}.svg', 'w', encoding='utf-8').write(icon)
print('ok', list(OPTIONS))
