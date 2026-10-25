# Erika 图标导出

由 `build.py` 从图标工坊的快照 `绘梨花·多边形帽（你的版本）`（`design/brand/icon-designs/`）生成。设计改了以后，先在工坊里重新保存这个快照，再运行 `python3 design/brand/erika-icon-export/build.py`，全部文件会重新生成。

软件图标就是主 logo：原样缩小放进纯白底，深色外观换成纯黑底。头发的圆占底板宽度的 76%，圆心居中，帽子完整留在底板里。有系统形状的平台（苹果、安卓、鸿蒙、Apple TV）放进底板；Windows、Linux 和网页标签页直接用透明背景的主 logo。

## 目录

| 目录 | 内容 |
|---|---|
| `logo/` | **对外展示的主 logo**：`erika-logo-icon.svg` 和 64–2048 的 PNG。圆形，发饰六边形不裁切、完整超出圆，背景透明；画板四周各放宽 16，圆仍居中。同一个文件也复制到了 `design/brand/erika-icon.svg`。带出场动画的版本 `erika-logo-icon-animated.svg`（同 `design/brand/erika-icon-animated.svg`）：纯 CSS，放进 `<img>` 和 README 都会播，约 2.3 秒后停在最后一帧。出场之后双击会随机做一个小动作（wink、摸头、绘梨花的坏笑），这部分靠 SVG 里的脚本，要用 `<object>` 嵌入或直接打开才有 |
| `svg/` | `erika-app.svg`（白底）、`erika-app-dark.svg`（黑底）、`erika-app-tinted.svg`（黑底上的白色单色版，给 iOS 着色图标），都是 1024 方块，系统自己裁圆角；`erika-foreground.svg`（透明背景的 logo，缩到 70%，给分层图标）、`erika-background.svg`（纯白）；`erika-mono-black.svg` / `erika-mono-white.svg`（无背景纯黑、纯白） |
| `png/` | `app`、`app-dark`、`mono-black`、`mono-white`、`foreground` 各出 16、24、32、48、64、128、256、512、1024 像素 |
| `macos/` | `Erika.icns`（logo 放进苹果模板的纯白底板：824 方块、圆角 185.4 的连续圆角矩形，和系统 `RoundedRectangle(style: .continuous)` 同一条路径，带投影。macOS 26 起底板必须是这个形状才按原样显示，否则会被系统放进灰色底板）、`Erika.iconset/`、`erika-macos.svg`、1024 预览图 |
| `apple/Erika.icon` | 苹果 26 的液态玻璃图标（Icon Composer 文档），iOS、macOS 通用：底色纯白，深色外观纯黑。层级和平面 logo 反过来：脸是完整的深色正圆垫在最下面，头发是正圆挖掉猫脸后的蓝色，盖在脸上；从前到后四组（帽子和发带、五官、头发、脸），每组一块玻璃，帽子在前盖住发带，除五官外都不透明。可以直接放进 Xcode 工程，也能用 Icon Composer 打开微调。嘴在这里是填充轮廓，不是描边：开口的描边会被玻璃当成填充。Bern 目前还不读 `.icon` |
| `windows/` | `erika.ico`（主 logo，16–256 共 7 个尺寸） |
| `linux/hicolor/` | 按 hicolor 主题目录放好的 16–512 PNG（主 logo）、`scalable/apps/erika.svg`、单色 `symbolic/apps/erika-symbolic.svg` |
| `ios/AppIcon.appiconset/` | `AppIcon.png`（白底）、`AppIcon-dark.png`（黑底，深色外观）、`AppIcon-tinted.png`（着色），都是 1024 不透明正方形，加 `Contents.json`，直接拖进 Xcode 的 Assets |
| `tvos/Erika.brandassets/` | Apple TV 的分层图标：两层，背景纯白，前景是主 logo，焦点移到图标上时 logo 在白底上方视差浮动。主屏幕图标 400×240（@1x、@2x，头发的圆占高度 66%）、App Store 图标 1280×768、Top Shelf 横幅 1920×720 与宽版 2320×720（@1x、@2x，占高度 56%）。`Bern.toml` 的 `[app.icon].tvos` 指向它 |
| `android/res/` | 自适应图标：`mipmap-anydpi-v26/ic_launcher*.xml` + 各密度的前景、单色层 PNG，背景色（纯白）在 `values/ic_launcher_background.xml`；旧版图标 `ic_launcher.png`（白色圆角方）/ `ic_launcher_round.png`（白色圆）。`android/layers/` 是三层的源 SVG，`playstore-icon-512.png`（白底）用于 Google Play |
| `harmonyos/resources/base/media/` | 分层图标：`foreground.png`（logo，缩到 70%）、`background.png`（纯白，1024）、`layered_image.json` |
| `web/` | `favicon.svg`、`favicon.ico`、`icon-192.png`、`icon-512.png`（主 logo，透明背景）、`apple-touch-icon.png`（180，白底）、`icon-maskable-512.png`（白底，logo 缩到 62%）、`site.webmanifest` |
| `skin/` | **备用图标「肤色」**：形状和主图标一样，脸换成浅肤色 `#FDE8DC`，眼睛和嘴换成深蓝 `#222B4F`。目录结构和上面完全相同（`logo/`、`svg/`、`png/`、`macos/`、`apple/`、`ios/`、`windows/`、`linux/`、`android/`、`web/` 等）。`Bern.toml` 的 `[app.alternate_icons.skin]` 指向这里，设置 › 外观 › 应用图标 里切换 |
| `png/app-thumb/` | 设置里「应用图标」列表的缩略图：白底圆角底板加一道浅灰细边，放在白色列表里也看得出底板（`skin/png/app-thumb/` 是肤色版） |
| `preview/` | 给品牌页和文档看的效果图：安卓圆形、圆角方、水滴三种遮罩和主题图标的浅色、深色；`glass-*.png` 是 `ictool` 渲染的液态玻璃六种外观（默认、深色、清透浅/深、着色浅/深） |
| `demo/` | `erika-logo-demo.gif`：出场动画加三次双击演示（wink、摸头、坏笑），540×540，25 帧/秒，约 14 秒循环。由 `demo-gif.mjs` 生成（无头 Chrome 逐帧截图 + ffmpeg）；`build.py` 不会清掉这个目录，改了动画要先跑 `build.py` 再跑 `node design/brand/erika-icon-export/demo-gif.mjs` |

## 说明

- **大小**：头发的圆占底板宽度的 76%（`APP_SCALE`），帽子右上角离底板圆角还有约 9%。安卓和鸿蒙同一张前景要套各种遮罩，帽子最远的角离圆心是半径的 1.26 倍，76% 时会碰到圆形遮罩的边，所以缩到 70%（`SAFE_SCALE`）。网页 maskable 图标的安全区是半径 40% 的圆，缩到 62%。
- **纯黑、纯白单色版**：去掉头发的填色，只留圆的轮廓环（粗 48，跨在圆边上，耳尖和蝴蝶结离环还有 28 的缝）；六边形帽子四周挖一道 14 的缝，和圆环、耳朵分开。眼睛、嘴、竖发绳挖成镂空，按图层顺序处理，所以被六边形和品红发带盖住的地方仍然是实心。按内容外框居中，四周留 4%。Linux 的 `erika-symbolic.svg`、iOS 着色图标和安卓主题图标的单色层都是这一版。
- **安卓自适应图标**：图层 108dp，设计占中间 72dp。单色层用于安卓 13 以上的主题图标，系统只看透明度。
- **液态玻璃的层级**：脸是完整的深色正圆垫在最下面，头发（正圆挖掉猫脸）盖在上面，帽子在发带前面。每组都不透明：半透明会透出白底，深色的脸冲成灰紫、肤色版的深蓝眼睛冲成灰蓝。深色外观里脸层指定自己的颜色，否则系统会把浅肤色提亮成白色。
- **液态玻璃预览**要用 Xcode 自带的 Icon Composer（`ictool`），没装 Xcode 时 `build.py` 照样生成 `Erika.icon`，只跳过预览图。
- **鸿蒙**：前景、背景分层，系统裁切的安全区需要在真机上确认一次。
