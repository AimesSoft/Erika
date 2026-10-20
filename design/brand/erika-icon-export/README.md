# Erika 图标导出

由 `build.py` 从图标工坊的两个快照生成（`design/brand/icon-designs/`）：

- 圆形版：`绘梨花·多边形帽（你的版本）`
- 正方形版：`绘梨花·正方形版`

设计改了以后，先在工坊里重新保存这两个快照，再运行 `python3 design/brand/erika-icon-export/build.py`，全部文件会重新生成。

## 目录

| 目录 | 内容 |
|---|---|
| `logo/` | **对外展示的主 logo**：`erika-logo-icon.svg` 和 64–2048 的 PNG。圆形，发饰六边形不裁切、完整超出圆，背景透明；画板四周各放宽 16，圆仍居中。同一个文件也复制到了 `design/brand/erika-icon.svg`。带出场动画的版本 `erika-logo-icon-animated.svg`（同 `design/brand/erika-icon-animated.svg`）：纯 CSS，放进 `<img>` 和 README 都会播，约 2.3 秒后停在最后一帧。出场之后双击会随机做一个小动作（wink、摸头、绘梨花的坏笑），这部分靠 SVG 里的脚本，要用 `<object>` 嵌入或直接打开才有 |
| `svg/` | `erika-circle.svg`（按圆裁切）、`erika-square.svg`（铺满正方形）、`erika-mono-black.svg` / `erika-mono-white.svg`（无背景纯黑、纯白）、`erika-foreground.svg`（去掉头发的前景）、`erika-background.svg`（纯色背景） |
| `png/` | 上面五种各出 16、24、32、48、64、128、256、512、1024 像素 |
| `macos/` | `Erika.icns`（正方形版放进苹果模板的底板：824 方块、圆角 185.4 的连续圆角矩形，和系统 `RoundedRectangle(style: .continuous)` 同一条路径，带投影。macOS 26 起底板必须是这个形状才按原样显示，原先的超椭圆底板会被系统放进灰色底板）、`Erika.iconset/`、`erika-macos.svg`、1024 预览图 |
| `windows/` | `erika.ico`（圆形版，16–256 共 7 个尺寸） |
| `linux/hicolor/` | 按 hicolor 主题目录放好的 16–512 PNG、`scalable/apps/erika.svg`、单色 `symbolic/apps/erika-symbolic.svg` |
| `ios/AppIcon.appiconset/` | `AppIcon.png`（1024 不透明正方形）、`AppIcon-tinted.png`（着色版）、`Contents.json`，直接拖进 Xcode 的 Assets |
| `tvos/Erika.brandassets/` | Apple TV 的分层图标：两层，背景是头发的蓝，前景是去掉头发的脸、帽子和发饰，焦点移到图标上时前景在背景上方视差浮动。主屏幕图标 400×240（@1x、@2x）、App Store 图标 1280×768、Top Shelf 横幅 1920×720 与宽版 2320×720（@1x、@2x）。`Bern.toml` 的 `[app.icon].tvos` 指向它 |
| `android/res/` | 自适应图标：`mipmap-anydpi-v26/ic_launcher*.xml` + 各密度的前景、单色层 PNG，背景色在 `values/ic_launcher_background.xml`；旧版图标 `ic_launcher.png` / `ic_launcher_round.png`。`android/layers/` 是三层的源 SVG，`playstore-icon-512.png` 用于 Google Play |
| `harmonyos/resources/base/media/` | 分层图标：`foreground.png`、`background.png`（1024）、`layered_image.json` |
| `web/` | `favicon.svg`、`favicon.ico`、`apple-touch-icon.png`（180）、`icon-192.png`、`icon-512.png`、`icon-maskable-512.png`、`site.webmanifest` |
| `demo/` | `erika-logo-demo.gif`：出场动画加三次双击演示（wink、摸头、坏笑），540×540，25 帧/秒，约 14 秒循环。由 `demo-gif.mjs` 生成（无头 Chrome 逐帧截图 + ffmpeg）；`build.py` 不会清掉这个目录，改了动画要先跑 `build.py` 再跑 `node design/brand/erika-icon-export/demo-gif.mjs` |

## 说明

- **纯黑、纯白单色版**：去掉头发的填色，只留圆的轮廓环（粗 48，跨在圆边上，耳尖和蝴蝶结离环还有 28 的缝）；六边形帽子四周挖一道 14 的缝，和圆环、耳朵分开。眼睛、嘴、竖发绳挖成镂空，按图层顺序处理，所以被六边形和品红发带盖住的地方仍然是实心。造型用圆形版（下巴是圆弧），按内容外框居中，四周留 4%。Linux 的 `erika-symbolic.svg` 也是这一版；安卓、iOS 的单色层由系统按自己的形状裁切，不画圆环。
- **安卓自适应图标**：图层 108dp，设计占中间 72dp；脸往下多画了一段，启动器做视差动画时不会露出底。单色层用于安卓 13 以上的主题图标，系统只看透明度。
- **iOS 深色图标没有单独提供**：脸是深蓝色，放在深色底上看不见。不提供时 iOS 会从主图自动生成深色版；要专门设计的话，需要先定深色版的配色。
- **鸿蒙**：按设计铺满 1024 导出，前景、背景分层。系统裁切的安全区需要在真机上确认一次。
- **六边形帽子**的位置超出了圆，所以圆形、macOS、安卓这些裁切过的版本里，它的右上角会被切掉一点，和工坊预览一致。
