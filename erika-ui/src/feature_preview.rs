//! Shared feature-selection preview for onboarding and settings.
use bern::prelude::*;
use erika_protocol::erika::FeatureSelection;

#[component]
pub fn feature_preview_window(features: FeatureSelection, revision: i64, width: f64) -> impl View {
    ui! {
        // `.o-mw`: 188 points tall, as wide as it stands: 300 beside the
        // desktop's choices; on the phone its width less 20 on each side
        // in the onboarding (350), less 16 in the settings (358).
        Stack(variant: settings_card, width: width, height: 188, align: leading, nudge_key: revision, nudge_scale: 0.95, reflow: smooth) {
            Canvas {
                Shape(kind: rounded, corner: large, stroke: control.border, stroke_width: 0.5)
            }
            if (build.modules.anime && features.anime) || (build.modules.library && features.library) || (build.modules.downloads && features.downloads) {
                Row(gap: 0, height: 188, align: top, appear: feature_preview_in, disappear: fade) {
                    Stack(width: 72, height: 188, align: top_leading) {
                        Canvas {
                            Shape(kind: rounded, corner: medium, fill: fill.subtle)
                        }
                        Column(gap: 8, padding_x: 7, padding_y: 22, reflow: smooth) {
                            // `.ms i.on`: the first page is the one shown.
                            if (build.modules.anime && features.anime) {
                                Stack(align: leading, appear: feature_preview_item_in, disappear: fade) {
                                    // `--tonal`: the accent at 26 % over the card.
                                    Canvas { Shape(kind: rounded, corner: 5, fill: accent, opacity: 0.26) }
                                    Row(gap: 0, padding_x: 5, height: 16) {
                                        Label(title: tr! { "zh-Hans" => "首页", "en" => "Home" }, icon: "house", style: subheadline)
                                        Spacer
                                    }
                                }
                            }
                            if (build.modules.anime && features.anime) {
                                Label(title: tr! { "zh-Hans" => "新番", "en" => "New" }, icon: "calendar", style: subheadline, appear: feature_preview_item_in, disappear: fade)
                            }
                            if (build.modules.library && features.library) && !(build.modules.anime && features.anime) {
                                Stack(align: leading, appear: feature_preview_item_in, disappear: fade) {
                                    // `--tonal`: the accent at 26 % over the card.
                                    Canvas { Shape(kind: rounded, corner: 5, fill: accent, opacity: 0.26) }
                                    Row(gap: 0, padding_x: 5, height: 16) {
                                        Label(title: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, icon: "film.stack", style: subheadline)
                                        Spacer
                                    }
                                }
                            } else if (build.modules.library && features.library) {
                                Label(title: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, icon: "film.stack", style: subheadline, appear: feature_preview_item_in, disappear: fade)
                            }
                            if (build.modules.downloads && features.downloads) {
                                Label(title: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, icon: "arrow.down.circle", style: subheadline, appear: feature_preview_item_in, disappear: fade)
                            }
                        }
                        Positioned(x: 7, y: 161, width: 58, height: 16) {
                            Row(gap: 4, align: center, reflow: smooth) {
                                if build.modules.account && features.account {
                                    Label(title: tr! { "zh-Hans" => "账号", "en" => "Account" }, icon: "person.crop.circle", display: icon, style: caption, appear: feature_preview_item_in, disappear: fade)
                                }
                                Spacer
                                Label(title: tr! { "zh-Hans" => "设置", "en" => "Settings" }, icon: "gearshape", display: icon, style: caption)
                            }
                        }
                    }
                    // The design's flex rows (`.mb`, 12 inside) at the
                    // phone's 286 and 278 points: three 16 : 9 cards and five
                    // 2 : 3 posters across, 5 apart, under titles 40 % and
                    // 30 % of the row.
                    if width == 358.0 {
                        Canvas(width: 286, height: 188) {
                            Shape(kind: rounded, x: 12, y: 12, width: 104.8, height: 7, corner: 3, fill: fill.strong)
                            Shape(kind: rounded, x: 12, y: 27, width: 84, height: 47.25, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 101, y: 27, width: 84, height: 47.25, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 190, y: 27, width: 84, height: 47.25, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 12, y: 82.25, width: 78.6, height: 7, corner: 3, fill: fill.strong)
                            Shape(kind: rounded, x: 12, y: 97.25, width: 48.4, height: 72.6, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 65.4, y: 97.25, width: 48.4, height: 72.6, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 118.8, y: 97.25, width: 48.4, height: 72.6, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 172.2, y: 97.25, width: 48.4, height: 72.6, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 225.6, y: 97.25, width: 48.4, height: 72.6, corner: 4, fill: fill.strong)
                        }
                    } else if width == 350.0 {
                        Canvas(width: 278, height: 188) {
                            Shape(kind: rounded, x: 12, y: 12, width: 101.6, height: 7, corner: 3, fill: fill.strong)
                            Shape(kind: rounded, x: 12, y: 27, width: 81.333, height: 45.75, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 98.333, y: 27, width: 81.333, height: 45.75, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 184.667, y: 27, width: 81.333, height: 45.75, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 12, y: 80.75, width: 76.2, height: 7, corner: 3, fill: fill.strong)
                            Shape(kind: rounded, x: 12, y: 95.75, width: 46.8, height: 70.2, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 63.8, y: 95.75, width: 46.8, height: 70.2, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 115.6, y: 95.75, width: 46.8, height: 70.2, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 167.4, y: 95.75, width: 46.8, height: 70.2, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 219.2, y: 95.75, width: 46.8, height: 70.2, corner: 4, fill: fill.strong)
                        }
                    } else {
                        Canvas(width: 228, height: 188) {
                            Shape(kind: rounded, x: 12, y: 12, width: 82, height: 7, corner: 3, fill: fill.strong)
                            Shape(kind: rounded, x: 12, y: 27, width: 64, height: 36, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 81, y: 27, width: 64, height: 36, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 150, y: 27, width: 64, height: 36, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 12, y: 72, width: 62, height: 7, corner: 3, fill: fill.strong)
                            Shape(kind: rounded, x: 12, y: 87, width: 37, height: 56, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 54, y: 87, width: 37, height: 56, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 96, y: 87, width: 37, height: 56, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 138, y: 87, width: 37, height: 56, corner: 4, fill: fill.strong)
                            Shape(kind: rounded, x: 180, y: 87, width: 37, height: 56, corner: 4, fill: fill.strong)
                        }
                    }
                }
            }
            else {
                // `.pure > div`: 62 % by 58 % of the window, a 1.5-point
                // dashed outline (Chrome's dashes: 4.5 drawn, 3 apart).
                Stack(width: width, height: 188, appear: feature_preview_in, disappear: fade) {
                    Canvas(width: if width == 358.0 { 221.96 } else if width == 350.0 { 217.0 } else { 186.0 }, height: 109) {
                        Shape(kind: rounded, corner: 10, stroke: control.border, stroke_width: 1.5, dash: 4.5, dash_gap: 3)
                    }
                    Text(content: tr! { "zh-Hans" => "打开文件", "en" => "Open file" }, style: caption, emphasis: secondary)
                }
            }
        }
    }
}

#[component]
pub fn feature_preview_hud(features: FeatureSelection) -> impl View {
    ui! {
        Stack(variant: settings_hud, height: 34, appearance: dark) {
            Row(gap: 6, padding_x: 12, reflow: smooth) {
                Label(title: tr! { "zh-Hans" => "", "en" => "" }, icon: "play.fill", style: caption)
                Rail {
                    Canvas(from: 0, to: 1, height: 3) {
                        Shape(kind: capsule, fill: fill.strong)
                    }
                    Canvas(from: 0, to: 0.38, height: 3) {
                        Shape(kind: capsule, fill: accent)
                    }
                }
                if (build.modules.danmaku && features.danmaku) {
                    Label(title: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, icon: "text.bubble", style: caption, appear: feature_preview_item_in, disappear: fade)
                }
                if (build.modules.enhance && features.enhance) {
                    Label(title: tr! { "zh-Hans" => "", "en" => "" }, icon: "sparkles", style: caption, appear: feature_preview_item_in, disappear: fade)
                }
                Label(title: tr! { "zh-Hans" => "", "en" => "" }, icon: "captions.bubble", style: caption)
                Label(title: tr! { "zh-Hans" => "", "en" => "" }, icon: "arrow.up.left.and.arrow.down.right", style: caption)
            }
        }
    }
}
