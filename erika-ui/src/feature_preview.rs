//! Shared feature-selection preview for onboarding and settings.
use bern::prelude::*;
use erika_protocol::erika::FeatureSelection;

#[component]
pub fn feature_preview_window(features: FeatureSelection, revision: i64) -> impl View {
    ui! {
        Stack(variant: settings_card, width: 300, height: 188, align: leading, nudge_key: revision, nudge_scale: 0.95, reflow: smooth) {
            Canvas {
                Shape(kind: rounded, corner: 16, stroke: control.border, stroke_width: 0.5)
            }
            if features.anime || features.library || features.downloads {
                Row(gap: 0, height: 188, align: top, appear: feature_preview_in, disappear: fade) {
                    Stack(width: 72, height: 188, align: top_leading) {
                        Canvas {
                            Shape(kind: rounded, corner: 12, fill: fill.subtle)
                        }
                        Column(gap: 8, padding_x: 7, padding_y: 22, reflow: smooth) {
                            if features.anime {
                                Label(title: tr! { "zh-Hans" => "首页", "en" => "Home" }, icon: "house", style: subheadline, appear: feature_preview_item_in, disappear: fade)
                            }
                            if features.anime {
                                Label(title: tr! { "zh-Hans" => "新番", "en" => "New" }, icon: "calendar", style: subheadline, appear: feature_preview_item_in, disappear: fade)
                            }
                            if features.library {
                                Label(title: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, icon: "film.stack", style: subheadline, appear: feature_preview_item_in, disappear: fade)
                            }
                            if features.downloads {
                                Label(title: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, icon: "arrow.down.circle", style: subheadline, appear: feature_preview_item_in, disappear: fade)
                            }
                        }
                    }
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
            else {
                Stack(width: 300, height: 188, appear: feature_preview_in, disappear: fade) {
                    Canvas(width: 186, height: 109) {
                        Shape(kind: rounded, corner: 10, stroke: control.border, stroke_width: 1.5)
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
                if features.danmaku {
                    Label(title: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, icon: "text.bubble", style: caption, appear: feature_preview_item_in, disappear: fade)
                }
                if features.enhance {
                    Label(title: tr! { "zh-Hans" => "", "en" => "" }, icon: "sparkles", style: caption, appear: feature_preview_item_in, disappear: fade)
                }
                Label(title: tr! { "zh-Hans" => "", "en" => "" }, icon: "captions.bubble", style: caption)
                Label(title: tr! { "zh-Hans" => "", "en" => "" }, icon: "arrow.up.left.and.arrow.down.right", style: caption)
            }
        }
    }
}
