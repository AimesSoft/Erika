//! Four-step welcome flow, measured from design/next-screens.js.
use crate::feature_preview::{
    feature_preview_hud, feature_preview_hud_props, feature_preview_window,
    feature_preview_window_props,
};
use bern::prelude::*;
use erika_protocol::erika::{self, Accent, OnboardingPreset};

#[component]
pub fn onboarding(app: erika::Client) -> impl View {
    ui! {
        // A television shows the flow over the whole screen, in the
        // prototype's 1280 × 720 scene (design 10.1: 「电视全屏」).
        if window.tv {
            Stack(under_bars: true, appear: fade, disappear: fade) {
                ScaleBox(width: 1280, height: 720) {
                    // The whole screen is the window (`.o-win`, 1280 × 720):
                    // the steps above, the foot's 72 points below.
                    Stack(variant: onboarding_page, width: 1280, height: 720) {
                        onboarding_content(app: app)
                    }
                }
            }
        } else {
            // On the phone the flow covers the screen (`obOpen`: the whole
            // window), its foot 72 points at the bottom.
            Modal(width: 960, height: 600, cover: compact, appear: pop_in, disappear: fade) {
                Stack(variant: onboarding_page) {
                    onboarding_content(app: app)
                }
            }
        }
    }
}

/// The steps and their foot.
#[component]
pub fn onboarding_content(app: erika::Client) -> impl View {
    ui! {
        Column(gap: 0) {
            if app.onboarding.step == 0 {
                onboarding_welcome()
            } else if app.onboarding.step == 3 {
                // `.o-step` of the last step: centred in the screen.
                if window.compact {
                    Column(gap: 0, align: center) {
                        Spacer
                        onboarding_ready(app: app)
                        Spacer
                    }
                } else {
                    ScrollView {
                        onboarding_ready(app: app)
                    }
                }
            } else {
                ScrollView {
                    if window.compact {
                        // `.o-step` on the phone: 20 from the edges, the
                        // preview above the choices, 16 between.
                        Column(gap: 16, padding_x: 20) {
                            Canvas(height: 4)
                            onboarding_preview(app: app, wide: true)
                            if app.onboarding.step == 1 { onboarding_features(app: app) }
                            else { onboarding_appearance(app: app) }
                            Canvas(height: 4)
                        }
                    } else {
                    Column(gap: 0, padding_x: if window.medium { 28 } else { 44 }, padding_y: 36) {
                        if window.medium {
                            Column(gap: 22) {
                                if app.onboarding.step == 1 { onboarding_features(app: app) }
                                else { onboarding_appearance(app: app) }
                                Row {
                                    Spacer
                                    onboarding_preview(app: app, wide: false)
                                    Spacer
                                }
                            }
                        } else {
                            Row(gap: 28, align: top) {
                                if app.onboarding.step == 1 { onboarding_features(app: app) }
                                else { onboarding_appearance(app: app) }
                                onboarding_preview(app: app, wide: false)
                            }
                        }
                    }
                    }
                }
            }
            onboarding_footer(app: app)
        }
    }
}

#[component]
pub fn onboarding_welcome() -> impl View {
    ui! {
        if window.medium || window.compact {
            ScrollView {
                Column(gap: 0) {
                    // `grid-template-rows: 220px 1fr`; the icon min(46 %, 200).
                    Stack(variant: onboarding_art, height: if window.compact { 220.0 } else { 170.0 }) {
                        Canvas { Pattern(name: academy) }
                    Svg(source: "design/brand/erika-icon-animated.svg", static_source: "design/brand/erika-icon-export/logo/erika-logo-icon.svg", width: if window.compact { 179.0 } else { 130.0 }, height: if window.compact { 179.0 } else { 130.0 }, label: tr! { "zh-Hans" => "Erika", "en" => "Erika" })
                    }
                    // `.o-welcome .tx`: 24 from the edges; the wordmark 8 nearer.
                    Column(gap: 0, padding_y: if window.compact { 26.0 } else { 28.0 }) {
                        Row(gap: 0, padding_x: if window.compact { 16.0 } else { 20.0 }) {
                            onboarding_wordmark()
                        }
                        Column(gap: 0, padding_x: if window.compact { 24.0 } else { 28.0 }) {
                            onboarding_greeting()
                        }
                    }
                }
            }
        } else {
            // A television's art is the screen's left half (640 × 648).
            Row(gap: 0, height: if window.tv { 648 } else { 528 }, align: top) {
                Stack(variant: onboarding_art, width: if window.tv { 640 } else { 480 }, height: if window.tv { 648 } else { 528 }) {
                    Canvas { Pattern(name: academy) }
                    Svg(source: "design/brand/erika-icon-animated.svg", static_source: "design/brand/erika-icon-export/logo/erika-logo-icon.svg", width: 200, height: 200, label: tr! { "zh-Hans" => "Erika", "en" => "Erika" })
                }
                Column(gap: 0, padding_y: 64) {
                    Row(gap: 0, padding_x: 36) {
                        onboarding_wordmark()
                    }
                    Column(gap: 0, padding_x: 44) {
                        onboarding_greeting()
                    }
                    Spacer
                }
            }
        }
    }
}
/// `.o-welcome .tx img`: 84 points tall, 18 above the title.
#[component]
pub fn onboarding_wordmark() -> impl View {
    ui! {
        Stack(variant: settings_wordmark) {
            Image(source: "erika-ui/assets/erika-wordmark.png", height: 84, rendering: template, label: tr! { "zh-Hans" => "ERIKA エリカ", "en" => "ERIKA エリカ" })
        }
    }
}

/// `.o-welcome h4` (28 / 34, 10 above the words) and `p` (13, lines 1.6
/// apart), 18 under the wordmark.
#[component]
pub fn onboarding_greeting() -> impl View {
    ui! {
        Column(gap: 0) {
        Canvas(height: 18)
        Text(content: tr! { "zh-Hans" => "欢迎使用 Erika", "en" => "Welcome to Erika" }, style: title2, line_height: 34)
        Canvas(height: 10)
        Text(content: tr! { "zh-Hans" => "接下来一分钟，选出你需要的功能。没选的功能不会出现，也不占资源，之后随时可以在「设置 › 可选功能」里改。", "en" => "Take a minute to choose the features you need. Everything else stays out of your way and uses no resources. You can change your choices in Settings › Optional Features." }, font_size: 13, line_height: 20.8, lines: 6, emphasis: secondary)
        }
    }
}
#[component]
pub fn onboarding_features(app: erika::Client) -> impl View {
    ui! {
        Column(gap: 0, appear: page_in) {
            Text(content: tr! { "zh-Hans" => "选一个起点", "en" => "Choose a starting point" }, style: title2, line_height: 34)
            Canvas(height: 4)
            Text(content: tr! { "zh-Hans" => "右边的小窗口会跟着变。之后可以在设置里逐项调整。", "en" => "The little window changes with your choices. Fine-tune everything later in Settings." }, font_size: 13, line_height: 18, emphasis: secondary, lines: 3)
            Canvas(height: 22)
            Grid(min_width: if window.compact { 160.0 } else { 220.0 }, gap: 12) {
                onboarding_card(app: app, preset: OnboardingPreset::Pure)
                onboarding_card(app: app, preset: OnboardingPreset::Anime)
                onboarding_card(app: app, preset: OnboardingPreset::All)
                onboarding_card(app: app, preset: OnboardingPreset::Custom)
            }
            Canvas(height: 22)
            Text(content: tr! { "zh-Hans" => "微调 · 点一下开关单个模块", "en" => "Fine-tune · Toggle individual modules" }, style: caption, emphasis: secondary)
            Canvas(height: 10)
            Flow(gap: 8) {
                ForEach(items: app.onboarding.modules, key: id) |module| {
                    if module.enabled {
                        Button(id: disable, label: module.module, icon: "checkmark", shape: capsule, height: 30, prominence: prominent) => app.onboarding_toggle(module.id)
                    } else {
                        Button(id: enable, label: module.module, icon: "plus", shape: capsule, height: 30) => app.onboarding_toggle(module.id)
                    }
                }
            }
        }
    }
}
#[component]
pub fn onboarding_card(app: erika::Client, preset: OnboardingPreset) -> impl View {
    ui! {
        Button(id: preset, label: preset, variant: onboarding_card) {
            Stack(variant: onboarding_card, align: top_trailing) {
                // Keep the intended minimum size while letting wrapped text
                // grow the card instead of shrinking and clipping its lines.
                Canvas(height: if window.compact || preset == OnboardingPreset::Pure || preset == OnboardingPreset::Anime { 83 } else { 67 })
                Column(gap: 4, padding_x: 16, padding_y: 14) {
                    Row(gap: 6) {
                        Text(content: preset, font_size: 15, style: headline)
                        if preset == OnboardingPreset::Anime {
                            Text(content: tr! { "zh-Hans" => "推荐", "en" => "Recommended" }, style: caption2)
                        }
                        Spacer
                        Canvas(width: 18, height: 1)
                    }
                    if preset == OnboardingPreset::Pure {
                        Text(content: tr! { "zh-Hans" => "和 VLC 一样，打开就能播，没有多余的东西。", "en" => "Open and play, just like VLC. Nothing extra." }, style: caption, emphasis: secondary, lines: 3)
                    } else if preset == OnboardingPreset::Anime {
                        Text(content: tr! { "zh-Hans" => "账号、弹幕、媒体库和追番。推荐大多数人选这个。", "en" => "Account, danmaku, library and anime. A good choice for most people." }, style: caption, emphasis: secondary, lines: 3)
                    } else if preset == OnboardingPreset::All {
                        Text(content: tr! { "zh-Hans" => "全部功能，和现在的 NipaPlay 一样。", "en" => "Every feature, just like NipaPlay today." }, style: caption, emphasis: secondary, lines: 3)
                    } else {
                        Text(content: tr! { "zh-Hans" => "在当前选择的基础上逐项调整。", "en" => "Adjust individual features from your current selection." }, style: caption, emphasis: secondary, lines: 3)
                    }
                }
                if app.onboarding.preset == preset {
                    Canvas {
                        Shape(kind: rounded, corner: large, stroke: accent, stroke_width: 2)
                    }
                    Row(padding: 12) {
                        Spacer
                        Stack(variant: onboarding_check, width: 22, height: 22, appear: pop_in) {
                            Canvas { Shape(kind: ellipse, fill: accent) }
                            Label(title: tr! { "zh-Hans" => "已选择", "en" => "Selected" }, icon: "checkmark", style: caption2, display: icon)
                        }
                    }
                }
            }
        } => app.onboarding_preset(preset)
    }
}
#[component]
pub fn onboarding_appearance(app: erika::Client) -> impl View {
    ui! {
        Column(gap: 0, appear: page_in) {
            Text(content: tr! { "zh-Hans" => "外观", "en" => "Appearance" }, style: title2, line_height: 34)
            Canvas(height: 4)
            Text(content: tr! { "zh-Hans" => "主题色取自图标，传给系统作为强调色。控件的样子跟随系统。", "en" => "Colors from the icon become the system accent. Controls follow your platform." }, font_size: 13, line_height: 18, emphasis: secondary, lines: 3)
            Canvas(height: 22)
            Text(content: tr! { "zh-Hans" => "主题色", "en" => "Accent color" }, style: caption, emphasis: secondary)
            Canvas(height: 8)
            Row(gap: 16) {
                onboarding_swatch(app: app, accent: Accent::Crimson)
                onboarding_swatch(app: app, accent: Accent::Ultramarine)
                onboarding_swatch(app: app, accent: Accent::Neon)
                onboarding_swatch(app: app, accent: Accent::Ribbon)
                Spacer
            }
            Canvas(height: 26)
            Text(content: tr! { "zh-Hans" => "明暗", "en" => "Appearance" }, style: caption, emphasis: secondary)
            Canvas(height: 10)
            Row(width: if window.compact { 186.0 } else { 258.0 }) {
                Picker(id: appearance, label: tr! { "zh-Hans" => "明暗", "en" => "Appearance" }, selected: app.appearance, style: segmented, label_hidden: true) => app.set_appearance(app.appearance)
            }
            Spacer
        }
    }
}
#[component]
pub fn onboarding_swatch(app: erika::Client, accent: Accent) -> impl View {
    ui! {
        if accent == Accent::Crimson {
            Button(id: crimson, label: accent, variant: onboarding_crimson) {
                onboarding_swatch_content(app: app, accent: accent)
            } => app.set_accent(accent)
        } else if accent == Accent::Ultramarine {
            Button(id: blue, label: accent, variant: onboarding_blue) {
                onboarding_swatch_content(app: app, accent: accent)
            } => app.set_accent(accent)
        } else if accent == Accent::Neon {
            Button(id: green, label: accent, variant: onboarding_green) {
                onboarding_swatch_content(app: app, accent: accent)
            } => app.set_accent(accent)
        } else {
            Button(id: pink, label: accent, variant: onboarding_pink) {
                onboarding_swatch_content(app: app, accent: accent)
            } => app.set_accent(accent)
        }
    }
}
#[component]
pub fn onboarding_swatch_content(app: erika::Client, accent: Accent) -> impl View {
    ui! {
        // `.o-sws button`: a 44-point disc, its name 6 under; the chosen
        // one's ring 2 points out and 2 wide, outside its room.
        Column(gap: 6, width: 44, align: center) {
            Stack(width: 44, height: 44) {
                if app.accent == accent {
                    Canvas(width: 52, height: 52) { Shape(kind: ellipse, x: 1, y: 1, width: 50, height: 50, stroke: text.primary, stroke_width: 2) }
                }
                Label(title: accent, icon: "circle.fill", style: title, font_size: 44, display: icon)
            }
            Stack(variant: onboarding_swatch_label) { Text(content: accent, style: caption) }
        }
    }
}
#[component]
pub fn onboarding_ready(app: erika::Client) -> impl View {
    ui! {
        Column(gap: 0, padding_y: 26, align: center) {
            Row {
                Spacer
                Stack(variant: onboarding_check, width: 72, height: 72, appear: pop_in) {
                    Canvas { Shape(kind: ellipse, fill: accent) }
                    Label(title: tr! { "zh-Hans" => "准备好了", "en" => "Ready" }, icon: "checkmark", style: title, font_size: 36, display: icon)
                }
                Spacer
            }
            Canvas(height: 18)
            Text(content: tr! { "zh-Hans" => "准备好了", "en" => "You're ready" }, style: title2, line_height: 34)
            Canvas(height: 4)
            Column(width: if window.compact { 300 } else { 420 }, align: center) {
                Text(content: tr! { "zh-Hans" => "开启了 {n} 个模块。点「开始使用」，右边那扇小窗口会长成你的 Erika。", "en" => "{n} modules enabled. Click Get started to turn this little window into your Erika."; n: app.onboarding.count }, font_size: 13, line_height: 18, emphasis: secondary, lines: 3)
            }
            Canvas(height: 22)
            onboarding_preview(app: app, wide: false)
        }
    }
}
#[component]
pub fn onboarding_footer(app: erika::Client) -> impl View {
    ui! {
        Stack(height: 72, align: top) {
            Divider
            Column(gap: 0) {
                // `.o-foot`: skipping, the dots between growing room, then
                // back and next.
                Row(gap: 12, padding_x: 28, height: 72, align: center) {
                    // A button the step does not offer keeps its room, as
                    // the design hides it, so the dots stay put.
                    if app.onboarding.step < 3 {
                        Button(id: skip, label: tr! { "zh-Hans" => "跳过", "en" => "Skip" }, shape: capsule, height: if window.tv { 46 } else { 32 }) => app.onboarding_skip()
                    } else {
                        Canvas(width: if window.tv { 76 } else { 54 }, height: 32)
                    }
                    Spacer
                    Row(gap: 6, align: center) {
                        onboarding_dot(app: app, step: 0)
                        onboarding_dot(app: app, step: 1)
                        onboarding_dot(app: app, step: 2)
                        onboarding_dot(app: app, step: 3)
                    }
                    Spacer
                    if app.onboarding.step > 0 {
                        Button(id: back, label: tr! { "zh-Hans" => "上一步", "en" => "Back" }, shape: capsule, height: if window.tv { 46 } else { 32 }) => app.onboarding_back()
                    } else {
                        Canvas(width: if window.tv { 92 } else { 67 }, height: 32)
                    }
                    if app.onboarding.step == 0 {
                        Button(id: start, label: tr! { "zh-Hans" => "开始", "en" => "Start" }, shape: capsule, height: if window.tv { 46 } else { 32 }, prominence: prominent, default_focus: true) => app.onboarding_next()
                    } else if app.onboarding.step == 3 {
                        Button(id: finish, label: tr! { "zh-Hans" => "开始使用", "en" => "Get started" }, shape: capsule, height: if window.tv { 46 } else { 32 }, prominence: prominent, default_focus: true) => app.onboarding_next()
                    } else {
                        Button(id: next, label: tr! { "zh-Hans" => "继续", "en" => "Continue" }, shape: capsule, height: if window.tv { 46 } else { 32 }, prominence: prominent, default_focus: true) => app.onboarding_next()
                    }
                }
            }
        }
    }
}
#[component]
pub fn onboarding_dot(app: erika::Client, step: i64) -> impl View {
    ui! {
        Stack(width: if app.onboarding.step == step { 20 } else { 6 }, height: 6, animate: jelly) {
            if app.onboarding.step == step { Canvas { Shape(kind: capsule, fill: text.primary) } }
            else { Canvas { Shape(kind: capsule, fill: fill.strong) } }
        }
    }
}

#[component]
pub fn onboarding_preview(app: erika::Client, wide: bool) -> impl View {
    ui! {
        Column(gap: 0, width: if wide { 350.0 } else { 300.0 }, appear: settings_page_in, appear_delay: 0) {
            Stack(width: if wide { 350.0 } else { 300.0 }, height: 188, hero: onboarding_window) {
                feature_preview_window(features: app.onboarding.features, revision: app.onboarding.preview_revision, width: if wide { 350.0 } else { 300.0 })
            }
            Canvas(height: 10)
            feature_preview_hud(features: app.onboarding.features)
            Canvas(height: 14)
            // `.o-sum`: 13 / 18, 14 under the bar.
            if app.onboarding.preset == OnboardingPreset::Custom {
                Text(content: tr! { "zh-Hans" => "已开启 {count} 个模块", "en" => "{count, plural, one {# module} other {# modules}} on"; count: app.onboarding.count }, font_size: 13, line_height: 18, emphasis: secondary, lines: 2)
            } else {
                Text(content: tr! { "zh-Hans" => "已开启 {count} 个模块 · 正好是「{preset}」", "en" => "{count, plural, one {# module} other {# modules}} on · just like “{preset}”"; count: app.onboarding.count, preset: app.onboarding.preset }, font_size: 13, line_height: 18, emphasis: secondary, lines: 2)
            }
        }
    }
}
