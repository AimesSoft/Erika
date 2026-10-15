//! Four-step welcome flow, measured from design/next-screens.js.
use bern::prelude::*;
use crate::feature_preview::{feature_preview_hud, feature_preview_window};
use erika_protocol::erika::{self, Accent, OnboardingPreset};

#[component]
pub fn onboarding(app: erika::Client) -> impl View {
    ui! {
        Modal(width: 960, height: 600, appear: pop_in, disappear: fade) {
            Stack(variant: settings_page) {
                Column(gap: 0) {
                    if app.onboarding.step == 0 {
                        onboarding_welcome()
                    } else if app.onboarding.step == 3 {
                        ScrollView {
                            onboarding_ready(app: app)
                        }
                    } else {
                        ScrollView {
                            Column(gap: 0, padding_x: if window.medium { 28 } else { 44 }, padding_y: 36) {
                                if window.medium || window.compact {
                                    Column(gap: 22) {
                                        if app.onboarding.step == 1 { onboarding_features(app: app) }
                                        else { onboarding_appearance(app: app) }
                                        Row {
                                            Spacer
                                            onboarding_preview(app: app)
                                            Spacer
                                        }
                                    }
                                } else {
                                    Row(gap: 28, align: top) {
                                        if app.onboarding.step == 1 { onboarding_features(app: app) }
                                        else { onboarding_appearance(app: app) }
                                        onboarding_preview(app: app)
                                    }
                                }
                            }
                        }
                    }
                    onboarding_footer(app: app)
                }
            }
        }
    }
}

#[component]
pub fn onboarding_welcome() -> impl View {
    ui! {
        if window.medium || window.compact {
            ScrollView {
                Column(gap: 0) {
                    Stack(variant: onboarding_art, height: 170) {
                        Canvas { Pattern(name: academy) }
                        Image(source: "design/assets/icon.png", width: 130, height: 130, label: tr! { "zh-Hans" => "Erika", "en" => "Erika" }, appear: pop_in)
                    }
                    Column(gap: 16, padding: 28) {
                        onboarding_greeting()
                    }
                }
            }
        } else {
            Row(gap: 0, height: 528, align: top) {
                Stack(variant: onboarding_art, width: 480, height: 528) {
                    Canvas { Pattern(name: academy) }
                    Image(source: "design/assets/icon.png", width: 200, height: 200, label: tr! { "zh-Hans" => "Erika", "en" => "Erika" }, appear: pop_in)
                }
                Column(gap: 16, padding_x: 44, padding_y: 64) {
                    onboarding_greeting()
                    Spacer
                }
            }
        }
    }
}
#[component]
pub fn onboarding_greeting() -> impl View {
    ui! {
        Column(gap: 16) {
        Stack(variant: settings_wordmark) {
            Image(source: "erika-ui/assets/erika-wordmark.png", height: 84, rendering: template, label: tr! { "zh-Hans" => "ERIKA エリカ", "en" => "ERIKA エリカ" })
        }
        Text(content: tr! { "zh-Hans" => "欢迎使用 Erika", "en" => "Welcome to Erika" }, style: title2)
        Text(content: tr! { "zh-Hans" => "接下来一分钟，选出你需要的功能。没选的功能不会出现，也不占资源，之后随时可以在「设置 › 可选功能」里改。", "en" => "Take a minute to choose the features you need. Everything else stays out of your way and uses no resources. You can change your choices in Settings › Optional Features." }, font_size: 13, lines: 6, emphasis: secondary)
        }
    }
}
#[component]
pub fn onboarding_features(app: erika::Client) -> impl View {
    ui! {
        Column(gap: 0, appear: page_in) {
            Text(content: tr! { "zh-Hans" => "选一个起点", "en" => "Choose a starting point" }, style: title2)
            Canvas(height: 4)
            Text(content: tr! { "zh-Hans" => "右边的小窗口会跟着变。之后可以在设置里逐项调整。", "en" => "The little window changes with your choices. Fine-tune everything later in Settings." }, emphasis: secondary, lines: 3)
            Canvas(height: 22)
            Grid(min_width: 220, gap: 12) {
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
                        Button(id: disable, label: module.name, icon: "checkmark", shape: capsule, height: 30, prominence: prominent) => app.onboarding_toggle(module.id)
                    } else {
                        Button(id: enable, label: module.name, icon: "plus", shape: capsule, height: 30) => app.onboarding_toggle(module.id)
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
            Stack(variant: onboarding_card, height: if preset == OnboardingPreset::Pure || preset == OnboardingPreset::Anime { 83 } else { 67 }, align: top_trailing) {
                Column(gap: 4, padding_x: 16, padding_y: 14, height: if preset == OnboardingPreset::Pure || preset == OnboardingPreset::Anime { 83 } else { 67 }) {
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
                        Text(content: tr! { "zh-Hans" => "弹幕、媒体库和追番。推荐大多数人选这个。", "en" => "Danmaku, library and anime. A good choice for most people." }, style: caption, emphasis: secondary, lines: 3)
                    } else if preset == OnboardingPreset::All {
                        Text(content: tr! { "zh-Hans" => "全部功能，和现在的 NipaPlay 一样。", "en" => "Every feature, just like NipaPlay today." }, style: caption, emphasis: secondary, lines: 3)
                    } else {
                        Text(content: tr! { "zh-Hans" => "在当前选择的基础上逐项调整。", "en" => "Adjust individual features from your current selection." }, style: caption, emphasis: secondary, lines: 3)
                    }
                    Spacer
                }
                if app.onboarding.preset == preset {
                    Canvas {
                        Shape(kind: rounded, corner: 16, stroke: accent, stroke_width: 2)
                    }
                    Row(padding: 12) {
                        Spacer
                        Stack(width: 22, height: 22, appear: pop_in) {
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
            Text(content: tr! { "zh-Hans" => "外观", "en" => "Appearance" }, style: title2)
            Canvas(height: 4)
            Text(content: tr! { "zh-Hans" => "主题色取自图标，传给系统作为强调色。控件的样子跟随系统。", "en" => "Colors from the icon become the system accent. Controls follow your platform." }, emphasis: secondary, lines: 3)
            Canvas(height: 22)
            Text(content: tr! { "zh-Hans" => "主题色", "en" => "Accent color" }, style: caption, emphasis: secondary)
            Canvas(height: 14)
            Row(gap: 16) {
                onboarding_swatch(app: app, accent: Accent::Crimson)
                onboarding_swatch(app: app, accent: Accent::Ultramarine)
                onboarding_swatch(app: app, accent: Accent::Neon)
                Spacer
            }
            Canvas(height: 26)
            Text(content: tr! { "zh-Hans" => "明暗", "en" => "Appearance" }, style: caption, emphasis: secondary)
            Canvas(height: 10)
            Row(width: 258) {
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
        } else {
            Button(id: green, label: accent, variant: onboarding_green) {
                onboarding_swatch_content(app: app, accent: accent)
            } => app.set_accent(accent)
        }
    }
}
#[component]
pub fn onboarding_swatch_content(app: erika::Client, accent: Accent) -> impl View {
    ui! {
        Column(gap: 8, width: 52, align: center) {
            Stack(width: 52, height: 52) {
                if app.accent == accent {
                    Canvas { Shape(kind: ellipse, x: 1, y: 1, width: 50, height: 50, stroke: text.primary, stroke_width: 2) }
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
            Text(content: tr! { "zh-Hans" => "准备好了", "en" => "You're ready" }, style: title2)
            Canvas(height: 4)
            Column(width: 420, align: center) {
                Text(content: tr! { "zh-Hans" => "开启了 {n} 个模块。点「开始使用」，右边那扇小窗口会长成你的 Erika。", "en" => "{n} modules enabled. Click Get started to turn this little window into your Erika."; n: app.onboarding.count }, emphasis: secondary, lines: 3)
            }
            Canvas(height: 22)
            onboarding_preview(app: app)
        }
    }
}
#[component]
pub fn onboarding_footer(app: erika::Client) -> impl View {
    ui! {
        Stack(height: 72, align: top) {
            Divider
            Row(gap: 12, padding_x: 28, height: 72, align: center) {
                if app.onboarding.step < 3 {
                    Button(id: skip, label: tr! { "zh-Hans" => "跳过", "en" => "Skip" }, shape: capsule, height: 32) => app.onboarding_skip()
                }
                Spacer
                if app.onboarding.step > 0 {
                    Button(id: back, label: tr! { "zh-Hans" => "上一步", "en" => "Back" }, shape: capsule, height: 32) => app.onboarding_back()
                }
                if app.onboarding.step == 0 {
                    Button(id: start, label: tr! { "zh-Hans" => "开始", "en" => "Start" }, shape: capsule, height: 32, prominence: prominent) => app.onboarding_next()
                } else if app.onboarding.step == 3 {
                    Button(id: finish, label: tr! { "zh-Hans" => "开始使用", "en" => "Get started" }, shape: capsule, height: 32, prominence: prominent) => app.onboarding_next()
                } else {
                    Button(id: next, label: tr! { "zh-Hans" => "继续", "en" => "Continue" }, shape: capsule, height: 32, prominence: prominent) => app.onboarding_next()
                }
            }
            Row(gap: 6, height: 72, align: center) {
                onboarding_dot(app: app, step: 0)
                onboarding_dot(app: app, step: 1)
                onboarding_dot(app: app, step: 2)
                onboarding_dot(app: app, step: 3)
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
pub fn onboarding_preview(app: erika::Client) -> impl View {
    ui! {
        Column(gap: 0, width: 300, appear: settings_page_in, appear_delay: 0) {
            Stack(width: 300, height: 188, hero: onboarding_window) {
                feature_preview_window(features: app.onboarding.features, revision: app.onboarding.preview_revision)
            }
            Canvas(height: 10)
            feature_preview_hud(features: app.onboarding.features)
            Canvas(height: 14)
            Text(content: app.onboarding.summary, style: caption, emphasis: secondary, lines: 2)
        }
    }
}
