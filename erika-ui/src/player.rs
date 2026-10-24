//! Full-window playback with the existing desktop, television and phone controls.
//! Controls and settings overlay the picture without changing its geometry.
use crate::AppWindows;
use crate::Players;
use bern::prelude::*;
use erika_protocol::erika::{self, *};

#[bern::session]
pub struct PlayerDraft {
    pub match_query: String,
    pub text: String,
    pub volume_open: bool,
    pub tracks_open: bool,
    pub rate_open: bool,
    pub enhancement_open: bool,
    pub danmaku_open: bool,
    pub composer_open: bool,
    /// The television's danmaku field, in place of the seek bar.
    pub saying: bool,
    pub subtitle_file: MediaSource,
    pub danmaku_file: MediaSource,
    /// The composer's hexadecimal color, when a plugin shows the field.
    pub color_hex: String,
}

#[component]
pub fn player(
    app: erika::Client,
    players: Players,
    windows: AppWindows,
    #[session] draft: PlayerDraft,
) -> impl View {
    ui! {
        Stack(appearance: dark, variant: player_black) {
            // A phone on its side plays in the design's 844 × 390 scene, the
            // controls in the two rails beside the picture (`RAILS`).
            // The television's controls run across the screen.
            if window.tv {
                Stack(under_bars: true) {
                    ScaleBox(width: 1280, height: 720) {
                        player_tv_scene(app: app, players: players, shown: !window.idle || app.paused || app.matching.open || app.player.panel || draft.saying)
                    }
                }
            } else if window.short {
                ScaleBox(width: 844, height: 390) {
                    player_phone_scene(app: app, players: players, shown: !window.idle || app.paused || app.matching.open || app.player.panel || draft.volume_open || draft.tracks_open || draft.rate_open || draft.danmaku_open || draft.composer_open)
                }
            } else {
                ScaleBox(width: 1280, height: 800) {
                    player_scene(app: app, players: players, windows: windows, shown: !window.idle || app.paused || app.matching.open || app.player.panel || draft.volume_open || draft.tracks_open || draft.rate_open || draft.enhancement_open || draft.danmaku_open || draft.composer_open)
                }
            }
            if let Some(notice) = app.notice {
                Toast(content: notice, key: app.notice_revision, duration: 3)
            }
        }
    }
}

#[component]
pub fn player_scene(
    app: erika::Client,
    players: Players,
    windows: AppWindows,
    shown: bool,
    #[session] draft: PlayerDraft,
) -> impl View {
    ui! {
        Stack(variant: player_black, width: 1280, height: 800, align: top_leading) {
            // The video keeps its full-window, aspect-fit geometry while
            // controls and settings appear above it. Bleed reaches the window
            // edges even when its aspect ratio differs from the design scene.
            Positioned(x: 0, y: 0, width: 1280, height: 800, bleed: true) {
                // The controls hide a second after a mouse comes to rest on
                // the picture, and stay while it is anywhere else.
                Button(id: picture, label: tr! { "zh-Hans" => "播放或暂停画面", "en" => "Play or pause picture" }, variant: player_picture, idle_area: 1) {
                    Stack {
                        ScaleBox(width: 1280, height: 720) {
                            Image(source: app.still, width: 1280, height: 720, corner: 0, fit: cover, label: app.title, hero: frame, hero_key: app.title, animate: hero)
                        }
                        // Give liberika the whole window: it fits the video and
                        // subtitles inside it, while danmaku can cross the bars.
                        if app.video {
                            Video(id: video, player: players.video, label: app.title, controls: none, sizing: fill, fit: contain, corner: 0)
                        }
                        ScaleBox(width: 1280, height: 720) {
                            player_overlays(app: app)
                        }
                    }
                } => app.toggle_pause()
            }
            if shown {
                Positioned(x: 35, y: 22, width: 1210, height: 64, appear: top_bar_in, disappear: top_bar_out) {
                    player_top(app: app)
                }
                Positioned(x: 103, y: 668.125, width: 1074, height: 60, animate: hero, appear: controls_in, disappear: controls_out) {
                    player_timeline(app: app)
                }
                Positioned(x: 35, y: 726, width: 1210, height: 64, animate: hero, appear: controls_in, disappear: controls_out) {
                    player_controls(app: app, windows: windows)
                }
            }
            if app.player.skip_kind != SkipKind::None {
                Positioned(x: 957, y: 608, width: 220, height: 40) {
                    player_skip_button(app: app)
                }
            }
        }
    }
}

/// The television's controls overlay a fixed 1280 × 720 picture. With
/// controls hidden the picture holds the remote: left/right skip 10 seconds,
/// select pauses, and Back leaves settings before leaving the player.
#[component]
pub fn player_tv_scene(
    app: erika::Client,
    players: Players,
    shown: bool,
    #[session] draft: PlayerDraft,
) -> impl View {
    ui! {
        Stack(variant: player_black, width: 1280, height: 720, align: top_leading) {
            Positioned(x: 0, y: 0, width: 1280, height: 720, bleed: true) {
                Stack {
                    Image(source: app.still, width: 1280, height: 720, corner: 0, fit: cover, label: app.title, hero: frame, hero_key: app.title, animate: hero)
                    if app.video {
                        Video(id: video_tv, player: players.video, label: app.title, controls: none, sizing: fill, fit: contain, corner: 0)
                    }
                    ScaleBox(width: 1280, height: 720) {
                        player_overlays(app: app)
                    }
                }
            }
            if shown {
                Positioned(x: 56, y: 0, width: 1168, height: 52, appear: top_bar_in, disappear: top_bar_out) {
                    player_tv_top(app: app)
                }
                Positioned(x: 200, y: 551, width: 880, height: 44, animate: hero, appear: controls_in, disappear: controls_out) {
                    Stack {
                        if draft.saying {
                            // 「发弹幕」: the field in the seek bar's place; the
                            // system's keyboard takes the remote's dictation.
                            FocusRegion {
                                Row(gap: 8, align: center) {
                                    Stack(variant: player_overlay, enabled: !app.player.sending_danmaku) {
                                        TextInput(id: danmaku_text_tv, label: tr! { "zh-Hans" => "发弹幕", "en" => "Send danmaku" }, label_hidden: true, prompt: tr! { "zh-Hans" => "发个友善的弹幕…", "en" => "Say something kind…" }, value: draft.text, clear_key: app.player.danmaku_sent_revision) => app.send_danmaku(draft.text)
                                    }
                                    PopoverButton(panel_appearance: surface, id: danmaku_color_tv, icon: "paintpalette", label: tr! { "zh-Hans" => "弹幕颜色", "en" => "Danmaku color" }) {
                                        Column(gap: 12, width: 320, padding: 10) {
                                            Text(content: tr! { "zh-Hans" => "弹幕颜色", "en" => "Danmaku color" }, style: title3)
                                            player_danmaku_colors(app: app)
                                        }
                                    }
                                    IconButton(id: unsay_tv, icon: "xmark", label: tr! { "zh-Hans" => "收起", "en" => "Close" }, height: 40) => draft.saying.set(false)
                                }
                            }
                        } else {
                            Stack(variant: player_overlay) {
                                Row(gap: 12, align: center) {
                                    Stack(variant: player_text) { Text(content: tr! { "zh-Hans" => "{time, duration, mm:ss}", "en" => "{time, duration, mm:ss}"; time: app.elapsed }, style: caption2) }
                                    // Left and right on the focused bar: 10 seconds
                                    // of a 24-minute episode.
                                    Slider(id: position_tv, appearance: dark, tint: text.primary, label: tr! { "zh-Hans" => "播放进度", "en" => "Playback position" }, label_hidden: true, current: app.progress, min: 0, max: 1, step: 0.00694) => app.seek(app.progress)
                                    Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "{time, duration, mm:ss}", "en" => "{time, duration, mm:ss}"; time: app.duration }, style: caption2) }
                                }
                            }
                        }
                    }
                }
                Positioned(x: 64, y: 624, width: 1152, height: 56, animate: hero, appear: controls_in, disappear: controls_out) {
                    player_tv_controls(app: app)
                }
            } else {
                // With the controls hidden the picture holds the remote's
                // focus, as tvOS's own player: select pauses or plays and
                // brings the controls, left and right skip 10 seconds.
                Positioned(x: 0, y: 0, width: 1280, height: 720) {
                    Button(id: picture_tv, label: tr! { "zh-Hans" => "播放或暂停画面", "en" => "Play or pause picture" }, variant: remote_surface, default_focus: true) {
                        Stack(width: 1280, height: 720) {}
                    } => app.toggle_pause()
                }
                Shortcut(key: "left") => app.skip(-10.0)
                Shortcut(key: "right") => app.skip(10.0)
            }
            Shortcut(key: "space") => app.toggle_pause()
            if app.player.panel {
                Shortcut(key: "escape") => app.player_panel(false)
            } else if !(build.modules.anime && app.features.anime) && !(build.modules.library && app.features.library) && !(build.modules.downloads && app.features.downloads) {
                Shortcut(key: "escape") => app.return_to_frame()
            } else {
                Shortcut(key: "escape") => app.minimize()
            }
            if app.player.skip_kind != SkipKind::None {
                Positioned(x: 1004, y: 490, width: 220, height: 44) {
                    player_skip_button(app: app)
                }
            }
        }
    }
}

/// The television's bar (`.ff-tv .p-top`, 52 tall, 56 in): the title with
/// its episode, then danmaku matching, picture in picture, the settings
/// and screenshot; no volume or full screen, which the remote and the television
/// have.
#[component]
pub fn player_tv_top(app: erika::Client) -> impl View {
    ui! {
        Row(gap: 12, height: 52, align: center) {
            Stack(variant: player_overlay) {
                Column(gap: 2) {
                    Stack(variant: player_text) { Text(content: app.title, style: title3, lines: 1) }
                    Stack(variant: player_secondary) { Text(content: app.subtitle, style: caption) }
                }
            }
            Spacer
            ControlGroup(label: tr! { "zh-Hans" => "播放选项", "en" => "Playback options" }, height: 46) {
                if (build.modules.danmaku && app.features.danmaku) {
                    PopoverButton(panel_appearance: surface, id: match_danmaku_tv, icon: "link", label: tr! { "zh-Hans" => "弹幕匹配", "en" => "Match danmaku" }, open: app.matching.open) { player_matching(app: app) } => app.show_matching(app.matching.open)
                }
                PopoverButton(panel_appearance: surface, id: settings_tv, icon: "slider.horizontal.3", label: tr! { "zh-Hans" => "播放设置", "en" => "Playback settings" }, open: app.player.panel) {
                    player_panel(app: app)
                } => app.player_panel(app.player.panel)
                IconButton(id: screenshot_tv, label: tr! { "zh-Hans" => "截图", "en" => "Screenshot" }, icon: "camera") => app.screenshot()
            }
        }
    }
}

/// The television's controls (`.p-ctl`, a grid of 1fr auto 1fr): danmaku
/// and 「发弹幕」 at the start, the transport in the middle, subtitles,
/// speed and enhancement at the end.
#[component]
pub fn player_tv_controls(app: erika::Client, #[session] draft: PlayerDraft) -> impl View {
    ui! {
        Stack {
            Row(gap: 8, align: center, height: 56) {
                if (build.modules.danmaku && app.features.danmaku) {
                    SplitButton(id: danmaku_tv, label: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, more_label: tr! { "zh-Hans" => "弹幕参数", "en" => "Danmaku options" }, icon: "text.bubble", checked: app.preferences.show_danmaku, shape: capsule, height: 46) {
                        player_danmaku_quick(app: app)
                    } => app.set_preference("show_danmaku", app.preferences.show_danmaku)
                    if (build.modules.danmaku && app.features.danmaku) {
                        Button(id: say_tv, label: tr! { "zh-Hans" => "发弹幕", "en" => "Send Danmaku" }, icon: "paperplane", shape: capsule, height: 46) => draft.saying.set(true)
                    }
                }
                Spacer
                player_tv_subtitle_menu(app: app)
                Picker(id: rate_tv, label: tr! { "zh-Hans" => "倍速", "en" => "Speed" }, selected: app.preferences.speed, label_hidden: true, style: menu, shape: capsule, height: 46) => app.set_speed(app.preferences.speed)
                if (build.modules.enhance && app.features.enhance) {
                    Picker(id: enhancement_tv, label: tr! { "zh-Hans" => "画质增强", "en" => "Enhancement" }, selected: app.player.enhancement, label_hidden: true, style: menu, shape: capsule, height: 46) => app.set_enhancement(app.player.enhancement)
                }
            }
            Row(gap: 14, align: center, height: 56) {
                if app.player.episodes {
                    IconButton(id: previous_tv, icon: "backward.end.fill", height: 46, label: tr! { "zh-Hans" => "上一集", "en" => "Previous episode" }, enabled: app.player.has_previous) => app.change_episode(-1)
                }
                IconButton(id: rewind_tv, icon: "gobackward.10", font_size: 20, height: 46, label: tr! { "zh-Hans" => "快退 10 秒", "en" => "Back 10 seconds" }) => app.skip(-10.0)
                if app.paused {
                    IconButton(id: play_tv, icon: "play.fill", font_size: 24, height: 56, prominence: prominent, default_focus: true, label: tr! { "zh-Hans" => "播放", "en" => "Play" }) => app.toggle_pause()
                } else {
                    IconButton(id: pause_tv, icon: "pause.fill", font_size: 24, height: 56, prominence: prominent, default_focus: true, label: tr! { "zh-Hans" => "暂停", "en" => "Pause" }) => app.toggle_pause()
                }
                IconButton(id: forward_tv, icon: "goforward.10", font_size: 20, height: 46, label: tr! { "zh-Hans" => "快进 10 秒", "en" => "Forward 10 seconds" }) => app.skip(10.0)
                if app.player.episodes {
                    IconButton(id: next_tv, icon: "forward.end.fill", height: 46, label: tr! { "zh-Hans" => "下一集", "en" => "Next episode" }, enabled: app.player.has_next) => app.change_episode(1)
                }
            }
        }
    }
}

/// The phone's landscape player: the rails, top bar and seek bar overlay
/// the full-window picture. Opening settings does not resize the video.
#[component]
pub fn player_phone_scene(
    app: erika::Client,
    players: Players,
    shown: bool,
    #[session] draft: PlayerDraft,
) -> impl View {
    ui! {
        Stack(variant: player_black, width: 844, height: 390, align: top_leading) {
            Positioned(x: 0, y: 0, width: 844, height: 390, bleed: true) {
                Button(id: picture_phone, label: tr! { "zh-Hans" => "播放或暂停画面", "en" => "Play or pause picture" }, variant: player_picture, idle_area: 1) {
                    Stack {
                        ScaleBox(width: 693, height: 390) {
                            Image(source: app.still, width: 693, height: 390, corner: 0, fit: cover, label: app.title, hero: frame, hero_key: app.title, animate: hero)
                        }
                        // The surface includes both side bars; only the video
                        // content is aspect-fit, not the danmaku canvas.
                        if app.video {
                            Video(id: video_phone, player: players.video, label: app.title, controls: none, sizing: fill, fit: contain, corner: 0)
                        }
                        ScaleBox(width: 693, height: 390) {
                            player_phone_overlays(app: app)
                        }
                    }
                } => app.toggle_pause()
            }
            if shown {
                // `.ph .p-top`: 8 points before the way back, 12 after the last action.
                Positioned(x: 0, y: 0, width: 840, height: 48, appear: top_bar_in, disappear: top_bar_out) {
                    player_phone_top(app: app)
                }
            }
            if shown {
                // The left rail: danmaku, then the previous episode and back 10.
                Positioned(x: 0, y: 148, width: 162, height: 34, appear: controls_in, disappear: controls_out) {
                    Row(gap: 0, align: center) {
                        Spacer
                        if (build.modules.danmaku && app.features.danmaku) {
                            SplitButton(id: danmaku_phone, label: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, more_label: tr! { "zh-Hans" => "弹幕参数", "en" => "Danmaku options" }, icon: "text.bubble", checked: app.preferences.show_danmaku, shape: capsule, height: 34) {
                                player_danmaku_quick(app: app)
                            } => app.set_preference("show_danmaku", app.preferences.show_danmaku)
                        }
                        Spacer
                    }
                }
                Positioned(x: 0, y: 194, width: 162, height: 46, appear: controls_in, disappear: controls_out) {
                    Row(gap: 8, align: center) {
                        Spacer
                        if app.player.episodes {
                            IconButton(id: previous_phone, icon: "backward.end.fill", font_size: 11, height: 46, label: tr! { "zh-Hans" => "上一集", "en" => "Previous episode" }, enabled: app.player.has_previous) => app.change_episode(-1)
                        }
                        IconButton(id: rewind_phone, icon: "gobackward.10", font_size: 12.5, height: 46, label: tr! { "zh-Hans" => "快退 10 秒", "en" => "Back 10 seconds" }) => app.skip(-10.0)
                        Spacer
                    }
                }
                // The right rail: subtitles, speed, the 60-point play button,
                // then forward 10 and the next episode.
                Positioned(x: 682, y: 91, width: 162, height: 32, appear: controls_in, disappear: controls_out) {
                    Row(gap: 0, align: center) {
                        Spacer
                        ControlGroup(height: 32) { player_subtitle_menu(app: app) }
                        Spacer
                    }
                }
                Positioned(x: 682, y: 135, width: 162, height: 32, appear: controls_in, disappear: controls_out) {
                    Row(gap: 0, align: center) {
                        Spacer
                        Picker(id: rate_phone, label: tr! { "zh-Hans" => "倍速", "en" => "Speed" }, selected: app.preferences.speed, label_hidden: true, style: menu, shape: capsule, height: 32) => app.set_speed(app.preferences.speed)
                        Spacer
                    }
                }
                Positioned(x: 682, y: 179, width: 162, height: 60, appear: controls_in, disappear: controls_out) {
                    Row(gap: 0, align: center) {
                        Spacer
                        if app.paused {
                            IconButton(id: play_phone, icon: "play.fill", font_size: 17, height: 60, prominence: prominent, label: tr! { "zh-Hans" => "播放", "en" => "Play" }) => app.toggle_pause()
                        } else {
                            IconButton(id: pause_phone, icon: "pause.fill", font_size: 17, height: 60, prominence: prominent, label: tr! { "zh-Hans" => "暂停", "en" => "Pause" }) => app.toggle_pause()
                        }
                        Spacer
                    }
                }
                Positioned(x: 682, y: 251, width: 162, height: 46, appear: controls_in, disappear: controls_out) {
                    Row(gap: 8, align: center) {
                        Spacer
                        IconButton(id: forward_phone, icon: "goforward.10", font_size: 12.5, height: 46, label: tr! { "zh-Hans" => "快进 10 秒", "en" => "Forward 10 seconds" }) => app.skip(10.0)
                        if app.player.episodes {
                            IconButton(id: next_phone, icon: "forward.end.fill", font_size: 11, height: 46, label: tr! { "zh-Hans" => "下一集", "en" => "Next episode" }, enabled: app.player.has_next) => app.change_episode(1)
                        }
                        Spacer
                    }
                }
            }
            if shown {
                // `.p-seek`, 3 under the picture: the time, the seek bar 54
                // in from both ends, the length.
                Positioned(x: 162.5, y: 357.1, width: 35.5, height: 16, animate: hero, appear: controls_in, disappear: controls_out) {
                    Stack(variant: player_overlay) {
                        Stack(variant: player_text) { Text(content: tr! { "zh-Hans" => "{time, duration, mm:ss}", "en" => "{time, duration, mm:ss}"; time: app.elapsed }, style: caption2, font_size: 12) }
                    }
                }
                Positioned(x: 216.5, y: 351.1, width: 411, height: 28, animate: hero, appear: controls_in, disappear: controls_out) {
                    Stack(variant: player_overlay) {
                        Slider(id: position_phone, appearance: dark, tint: text.primary, label: tr! { "zh-Hans" => "播放进度", "en" => "Playback position" }, label_hidden: true, current: app.progress, min: 0, max: 1) => app.seek(app.progress)
                    }
                }
                if app.player.has_chapters && app.duration > 0.0 {
                    Positioned(x: 216.5, y: 363.6, width: 411, height: 3, animate: hero, appear: controls_in, disappear: controls_out) {
                        Stack(variant: player_overlay) {
                            Rail {
                                ForEach(items: app.player.chapters, key: id) |chapter| {
                                    Canvas(at: chapter.at, width: 3, height: 3) { Shape(kind: ellipse, fill: text.primary) }
                                }
                            }
                        }
                    }
                }
                Positioned(x: 646, y: 357.1, width: 35.5, height: 16, animate: hero, appear: controls_in, disappear: controls_out) {
                    Stack(variant: player_overlay) {
                        Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "{time, duration, mm:ss}", "en" => "{time, duration, mm:ss}"; time: app.duration }, style: caption2, font_size: 12) }
                    }
                }
            }
            // `.ph .p-sheet`: the settings beside the picture.
            if app.player.skip_kind != SkipKind::None {
                Positioned(x: 488, y: 290, width: 180, height: 36) {
                    player_skip_button(app: app)
                }
            }
        }
    }
}

/// The phone player's bar (`.p-top`, 48 points): the way back on a glass
/// circle, the title with its episode, and its actions on one glass
/// capsule, Apple's 44 points tall.
#[component]
pub fn player_phone_top(app: erika::Client) -> impl View {
    ui! {
        Row(gap: 10, padding_x: 8, height: 48, align: center) {
            IconButton(id: back_phone, icon: "chevron.left", font_size: 18, height: 44, label: tr! { "zh-Hans" => "返回", "en" => "Back" }) => app.minimize()
            Stack(variant: player_overlay) {
                Row(gap: 8, align: baseline) {
                    Stack(variant: player_text) { Text(content: app.title, style: title3, lines: 1) }
                    Stack(variant: player_secondary) { Text(content: app.subtitle, style: caption, lines: 1) }
                }
            }
            Spacer
            ControlGroup(label: tr! { "zh-Hans" => "播放选项", "en" => "Playback options" }, height: 44) {
                if (build.modules.danmaku && app.features.danmaku) {
                    PopoverButton(panel_appearance: surface, id: match_danmaku_phone, icon: "link", label: tr! { "zh-Hans" => "弹幕匹配", "en" => "Match danmaku" }, open: app.matching.open) { player_matching(app: app) } => app.show_matching(app.matching.open)
                }
                PopoverButton(panel_appearance: surface, id: settings_phone, icon: "slider.horizontal.3", label: tr! { "zh-Hans" => "播放设置", "en" => "Playback settings" }, open: app.player.panel) {
                    player_panel(app: app)
                } => app.player_panel(app.player.panel)
                IconButton(id: screenshot_phone, label: tr! { "zh-Hans" => "截图", "en" => "Screenshot" }, icon: "camera") => app.screenshot()
            }
        }
    }
}

/// What shows over the picture: the play or pause it was given. Subtitles
/// and danmaku are in the picture, drawn by liberika.
#[component]
pub fn player_overlays(app: erika::Client) -> impl View {
    ui! {
        Stack(width: 1280, height: 720, align: top_leading) {
            if app.player.feedback {
                Positioned(x: 596, y: 316, width: 88, height: 88, appear: player_feedback_in, disappear: fade) {
                    Stack(variant: player_overlay) {
                        if app.paused {
                            Stack(variant: player_text) { Label(title: tr! { "zh-Hans" => "暂停", "en" => "Pause" }, icon: "pause.fill", display: icon, font_size: 44) }
                        } else {
                            Stack(variant: player_text) { Label(title: tr! { "zh-Hans" => "播放", "en" => "Play" }, icon: "play.fill", display: icon, font_size: 44) }
                        }
                    }
                }
            }
        }
    }
}

/// The phone's overlay (`.ph .cv`), laid out on the 693 × 390 picture and
/// scaled with it.
#[component]
pub fn player_phone_overlays(app: erika::Client) -> impl View {
    ui! {
        Stack(width: 693, height: 390, align: top_leading) {
            if app.player.feedback {
                Positioned(x: 302.5, y: 151, width: 88, height: 88, appear: player_feedback_in, disappear: fade) {
                    Stack(variant: player_overlay) {
                        if app.paused {
                            Stack(variant: player_text) { Label(title: tr! { "zh-Hans" => "暂停", "en" => "Pause" }, icon: "pause.fill", display: icon, font_size: 44) }
                        } else {
                            Stack(variant: player_text) { Label(title: tr! { "zh-Hans" => "播放", "en" => "Play" }, icon: "play.fill", display: icon, font_size: 44) }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn player_top(app: erika::Client, #[session] draft: PlayerDraft) -> impl View {
    ui! {
        Row(gap: 12, height: 64, align: center) {
            if draft.tracks_open {
                Shortcut(key: "escape") => draft.tracks_open.set(false)
            } else if draft.rate_open {
                Shortcut(key: "escape") => draft.rate_open.set(false)
            } else if draft.danmaku_open {
                Shortcut(key: "escape") => draft.danmaku_open.set(false)
            } else if draft.composer_open {
                Shortcut(key: "escape") => draft.composer_open.set(false)
            } else if draft.volume_open {
                Shortcut(key: "escape") => draft.volume_open.set(false)
            } else if app.player.panel {
                Shortcut(key: "escape") => app.player_panel(false)
            } else if !(build.modules.anime && app.features.anime) && !(build.modules.library && app.features.library) && !(build.modules.downloads && app.features.downloads) {
                Shortcut(key: "escape") => app.return_to_frame()
            } else {
                Shortcut(key: "escape") => app.minimize()
            }
            ControlGroup(height: 40) {
                // With no sidebar to shrink into, the picture goes back into
                // the empty frame (design 10.2).
                if !(build.modules.anime && app.features.anime) && !(build.modules.library && app.features.library) && !(build.modules.downloads && app.features.downloads) {
                    IconButton(id: back_to_frame, icon: "chevron.left", label: tr! { "zh-Hans" => "返回", "en" => "Back" }) => app.return_to_frame()
                } else {
                    IconButton(id: back, icon: "chevron.left", label: tr! { "zh-Hans" => "返回", "en" => "Back" }) => app.minimize()
                }
            }
            Stack(variant: player_overlay) {
                Column(gap: 2, padding_x: 4) {
                    Stack(variant: player_text) { Text(content: app.title, style: title3, lines: 1) }
                    Stack(variant: player_secondary) { Text(content: app.subtitle, style: caption) }
                }
            }
            Spacer
            ControlGroup(label: tr! { "zh-Hans" => "音量与弹幕", "en" => "Audio and danmaku" }, height: 40) {
                PopoverButton(panel_appearance: surface, id: volume, icon: "speaker.wave.2", label: tr! { "zh-Hans" => "音量", "en" => "Volume" }, presented: draft.volume_open) {
                    Column(gap: 12, width: 240, padding: 8) {
                        Row(align: center) {
                            Text(content: tr! { "zh-Hans" => "音量", "en" => "Volume" }) Spacer
                            Text(content: tr! { "zh-Hans" => "{value}%", "en" => "{value}%"; value: app.player.volume_percent })
                        }
                        Slider(id: volume_level, label: tr! { "zh-Hans" => "音量", "en" => "Volume" }, label_hidden: true, current: app.player.volume, continuous: true, min: 0, max: 1) => app.set_player_number("volume", app.player.volume)
                    }
                }
                if (build.modules.danmaku && app.features.danmaku) {
                    PopoverButton(panel_appearance: surface, id: match_danmaku, icon: "link", label: tr! { "zh-Hans" => "弹幕匹配", "en" => "Match danmaku" }, open: app.matching.open) { player_matching(app: app) } => app.show_matching(app.matching.open)
                }
            }
            ControlGroup(label: tr! { "zh-Hans" => "播放选项", "en" => "Playback options" }, height: 40) {
                PopoverButton(panel_appearance: surface, id: settings, icon: "slider.horizontal.3", label: tr! { "zh-Hans" => "播放设置", "en" => "Playback settings" }, open: app.player.panel) {
                    player_panel(app: app)
                } => app.player_panel(app.player.panel)
                IconButton(id: screenshot, label: tr! { "zh-Hans" => "截图", "en" => "Screenshot" }, icon: "camera", shortcut: "cmd+s") => app.screenshot()
            }
        }
    }
}

#[component]
pub fn player_timeline(app: erika::Client) -> impl View {
    ui! {
        Stack(variant: player_overlay, align: top_leading) {
            Positioned(x: 0, y: 0, width: 1074, height: 44) {
                Slider(id: position, appearance: dark, tint: text.primary, label: tr! { "zh-Hans" => "播放进度", "en" => "Playback position" }, label_hidden: true, current: app.progress, min: 0, max: 1) => app.seek(app.progress)
            }
            if app.player.has_chapters && app.duration > 0.0 {
                Positioned(x: 0, y: 20, width: 1074, height: 4) {
                    Rail {
                        ForEach(items: app.player.chapters, key: id) |chapter| {
                            Canvas(at: chapter.at, width: 2, height: 4) { Shape(kind: rect, fill: text.primary) }
                        }
                    }
                }
                Positioned(x: 0, y: 40, width: 1074, height: 24) {
                    Rail(anchor: start) {
                        ForEach(items: app.player.chapters, key: id) |chapter| {
                            if chapter.label_width >= 24.0 {
                                Stack(at: chapter.at, width: chapter.label_width, height: 24, align: leading) {
                                    Button(id: chapter, label: chapter.title, variant: player_picture) {
                                        Stack(variant: player_text) { Text(content: chapter.title, style: caption, lines: 1) }
                                    } => app.seek_chapter(chapter.id)
                                }
                            }
                        }
                    }
                }
            }
            Positioned(x: -68, y: 14, width: 54, height: 16) { Row { Spacer Stack(variant: player_text) { Text(content: tr! { "zh-Hans" => "{time, duration, mm:ss}", "en" => "{time, duration, mm:ss}"; time: app.elapsed }, style: caption2) } } }
            Positioned(x: 1088, y: 14, width: 54, height: 16) { Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "{time, duration, mm:ss}", "en" => "{time, duration, mm:ss}"; time: app.duration }, style: caption2) } }
        }
    }
}

/// Transport, danmaku composer and utility controls share the timeline's outer edges.
#[component]
pub fn player_controls(
    app: erika::Client,
    windows: AppWindows,
    #[session] draft: PlayerDraft,
) -> impl View {
    ui! {
        Row(gap: 18, align: center, height: 64) {
            Row(gap: 10, align: center, height: 64) {
                IconButton(id: previous, variant: transport, icon: "backward.end.fill", label: tr! { "zh-Hans" => "上一集", "en" => "Previous episode" }, height: 44, prominence: plain, enabled: app.player.has_previous) => app.change_episode(-1)
                IconButton(id: rewind, icon: "gobackward.10", variant: transport, label: tr! { "zh-Hans" => "快退 10 秒", "en" => "Back 10 seconds" }, height: 44, prominence: plain, shortcut: "left") => app.skip(-10.0)
                if app.paused {
                    IconButton(id: play, icon: "play.fill", label: tr! { "zh-Hans" => "播放", "en" => "Play" }, variant: player_primary, height: 64, prominence: plain, shortcut: "space") => app.toggle_pause()
                } else {
                    IconButton(id: pause, icon: "pause.fill", label: tr! { "zh-Hans" => "暂停", "en" => "Pause" }, variant: player_primary, height: 64, prominence: plain, shortcut: "space") => app.toggle_pause()
                }
                IconButton(id: forward, icon: "goforward.10", variant: transport, label: tr! { "zh-Hans" => "快进 10 秒", "en" => "Forward 10 seconds" }, height: 44, prominence: plain, shortcut: "right") => app.skip(10.0)
                IconButton(id: next, variant: transport, icon: "forward.end.fill", label: tr! { "zh-Hans" => "下一集", "en" => "Next episode" }, height: 44, prominence: plain, enabled: app.player.has_next) => app.change_episode(1)
            }
            if (build.modules.danmaku && app.features.danmaku) {
                ControlGroup(height: 40) {
                    PopoverButton(panel_appearance: surface, id: danmaku, label: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, icon: "text.bubble", presented: draft.danmaku_open) {
                        player_danmaku_quick(app: app)
                    }
                    PopoverButton(panel_appearance: surface, id: compose_danmaku, label: tr! { "zh-Hans" => "发弹幕", "en" => "Send danmaku" }, icon: "paperplane", presented: draft.composer_open) {
                        Column(gap: 14, width: 320, padding: 10) {
                            Text(content: tr! { "zh-Hans" => "发弹幕", "en" => "Send danmaku" }, style: title3)
                            FocusRegion {
                                Column(gap: 12) {
                                    Stack(enabled: !app.player.sending_danmaku) {
                                        TextInput(id: danmaku_text, label: tr! { "zh-Hans" => "弹幕内容", "en" => "Danmaku message" }, label_hidden: true, prompt: tr! { "zh-Hans" => "发个友善的弹幕…", "en" => "Say something kind…" }, value: draft.text, clear_key: app.player.danmaku_sent_revision) => app.send_danmaku(draft.text)
                                    }
                                    player_danmaku_colors(app: app)
                                    Row {
                                        Spacer
                                        Button(id: send, icon: "paperplane", label: tr! { "zh-Hans" => "发送", "en" => "Send" }, shape: capsule, height: 32, prominence: prominent, enabled: !app.player.sending_danmaku) => app.send_danmaku(draft.text)
                                    }
                                }
                            }
                        }
                    }
                }
                Shortcut(key: "d") => app.set_preference("show_danmaku", !app.preferences.show_danmaku)
            }
            Spacer
            ControlGroup(height: 40) {
                player_subtitle_menu(app: app)
                PopoverButton(panel_appearance: surface, id: rate, icon: "speedometer", label: tr! { "zh-Hans" => "播放速度", "en" => "Playback speed" }, presented: draft.rate_open) {
                    player_speed_menu(app: app)
                }
                if (build.modules.enhance && app.features.enhance) {
                    PopoverButton(panel_appearance: surface, id: enhancement, icon: "sparkles", label: tr! { "zh-Hans" => "画质增强", "en" => "Enhancement" }, presented: draft.enhancement_open) {
                        player_enhancement_menu(app: app)
                    }
                }
                if window.fullscreen {
                    IconButton(id: exit_fullscreen, icon: "arrow.down.right.and.arrow.up.left", label: tr! { "zh-Hans" => "退出全屏", "en" => "Exit full screen" }, shortcut: "f") => windows.main.toggle_fullscreen()
                } else {
                    IconButton(id: fullscreen, icon: "viewfinder", label: tr! { "zh-Hans" => "全屏", "en" => "Full screen" }, shortcut: "f") => windows.main.toggle_fullscreen()
                }
            }
        }
    }
}

#[component]
pub fn player_enhancement_menu(app: erika::Client) -> impl View {
    ui! {
        Column(gap: 4, width: 248, padding: 8) {
            Text(content: tr! { "zh-Hans" => "画质增强", "en" => "Enhancement" }, style: title3)
            Canvas(height: 8)
            Button(id: enhancement_off, label: tr! { "zh-Hans" => "关闭", "en" => "Off" }, variant: player_picture) {
                Row(height: 36, padding_x: 4) {
                    Text(content: tr! { "zh-Hans" => "关闭", "en" => "Off" }) Spacer
                    if app.player.enhancement == Enhancement::Off {
                        Stack(variant: player_accent) { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) }
                    }
                }
            } => app.set_enhancement(Enhancement::Off)
            Button(id: enhancement_artcnn, label: tr! { "zh-Hans" => "ArtCNN 2×", "en" => "ArtCNN 2×" }, variant: player_picture) {
                Row(height: 36, padding_x: 4) {
                    Text(content: tr! { "zh-Hans" => "ArtCNN 2×", "en" => "ArtCNN 2×" }) Spacer
                    if app.player.enhancement == Enhancement::ArtCnn {
                        Stack(variant: player_accent) { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) }
                    }
                }
            } => app.set_enhancement(Enhancement::ArtCnn)
            Button(id: enhancement_quality, label: tr! { "zh-Hans" => "ArtCNN 2× 高质量", "en" => "ArtCNN 2× high quality" }, variant: player_picture) {
                Row(height: 36, padding_x: 4) {
                    Text(content: tr! { "zh-Hans" => "ArtCNN 2× 高质量", "en" => "ArtCNN 2× high quality" }) Spacer
                    if app.player.enhancement == Enhancement::ArtCnnQuality {
                        Stack(variant: player_accent) { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) }
                    }
                }
            } => app.set_enhancement(Enhancement::ArtCnnQuality)
            Button(id: enhancement_denoise, label: tr! { "zh-Hans" => "ArtCNN 2× 降噪", "en" => "ArtCNN 2× denoise" }, variant: player_picture) {
                Row(height: 36, padding_x: 4) {
                    Text(content: tr! { "zh-Hans" => "ArtCNN 2× 降噪", "en" => "ArtCNN 2× denoise" }) Spacer
                    if app.player.enhancement == Enhancement::ArtCnnDenoise {
                        Stack(variant: player_accent) { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) }
                    }
                }
            } => app.set_enhancement(Enhancement::ArtCnnDenoise)
        }
    }
}

#[component]
pub fn player_speed_menu(app: erika::Client) -> impl View {
    ui! {
        Column(gap: 4, width: 180, padding: 8) {
            Text(content: tr! { "zh-Hans" => "播放速度", "en" => "Playback speed" }, style: title3)
            Canvas(height: 8)
            Button(id: rate_half, label: tr! { "zh-Hans" => "0.5×", "en" => "0.5×" }, variant: player_picture) {
                Row(height: 32, padding_x: 4) {
                    Text(content: tr! { "zh-Hans" => "0.5×", "en" => "0.5×" }) Spacer
                    if app.preferences.speed == PlaybackSpeed::Half {
                        Stack(variant: player_accent) { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) }
                    }
                }
            } => app.set_speed(PlaybackSpeed::Half)
            Button(id: rate_slow, label: tr! { "zh-Hans" => "0.75×", "en" => "0.75×" }, variant: player_picture) {
                Row(height: 32, padding_x: 4) {
                    Text(content: tr! { "zh-Hans" => "0.75×", "en" => "0.75×" }) Spacer
                    if app.preferences.speed == PlaybackSpeed::Slow {
                        Stack(variant: player_accent) { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) }
                    }
                }
            } => app.set_speed(PlaybackSpeed::Slow)
            Button(id: rate_normal, label: tr! { "zh-Hans" => "1.0×", "en" => "1.0×" }, variant: player_picture) {
                Row(height: 32, padding_x: 4) {
                    Text(content: tr! { "zh-Hans" => "1.0×", "en" => "1.0×" }) Spacer
                    if app.preferences.speed == PlaybackSpeed::Normal {
                        Stack(variant: player_accent) { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) }
                    }
                }
            } => app.set_speed(PlaybackSpeed::Normal)
            Button(id: rate_fast, label: tr! { "zh-Hans" => "1.25×", "en" => "1.25×" }, variant: player_picture) {
                Row(height: 32, padding_x: 4) {
                    Text(content: tr! { "zh-Hans" => "1.25×", "en" => "1.25×" }) Spacer
                    if app.preferences.speed == PlaybackSpeed::Fast {
                        Stack(variant: player_accent) { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) }
                    }
                }
            } => app.set_speed(PlaybackSpeed::Fast)
            Button(id: rate_faster, label: tr! { "zh-Hans" => "1.5×", "en" => "1.5×" }, variant: player_picture) {
                Row(height: 32, padding_x: 4) {
                    Text(content: tr! { "zh-Hans" => "1.5×", "en" => "1.5×" }) Spacer
                    if app.preferences.speed == PlaybackSpeed::Faster {
                        Stack(variant: player_accent) { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) }
                    }
                }
            } => app.set_speed(PlaybackSpeed::Faster)
            Button(id: rate_double, label: tr! { "zh-Hans" => "2.0×", "en" => "2.0×" }, variant: player_picture) {
                Row(height: 32, padding_x: 4) {
                    Text(content: tr! { "zh-Hans" => "2.0×", "en" => "2.0×" }) Spacer
                    if app.preferences.speed == PlaybackSpeed::Double {
                        Stack(variant: player_accent) { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) }
                    }
                }
            } => app.set_speed(PlaybackSpeed::Double)
        }
    }
}

#[component]
pub fn player_danmaku_quick(app: erika::Client) -> impl View {
    ui! {
        Column(gap: 14, padding: 10, width: 320) {
            Switch(id: show_danmaku, label: tr! { "zh-Hans" => "显示弹幕", "en" => "Show danmaku" }, checked: app.preferences.show_danmaku) => app.set_preference("show_danmaku", app.preferences.show_danmaku)
            Column(gap: 4) {
                Row(align: center) {
                    Text(content: tr! { "zh-Hans" => "不透明度", "en" => "Opacity" }) Spacer
                    Text(content: tr! { "zh-Hans" => "{value}%", "en" => "{value}%"; value: app.danmaku_settings.opacity_percent })
                }
                Slider(id: opacity, label: tr! { "zh-Hans" => "不透明度", "en" => "Opacity" }, label_hidden: true, current: app.danmaku_settings.opacity, continuous: true, min: 0, max: 1) => app.set_danmaku_number("opacity", app.danmaku_settings.opacity)
            }
            Column(gap: 4) {
                Row(align: center) {
                    Text(content: tr! { "zh-Hans" => "字号", "en" => "Font size" }) Spacer
                    Text(content: tr! { "zh-Hans" => "{value} pt", "en" => "{value} pt"; value: app.danmaku_settings.font_points })
                }
                Slider(id: font_size, label: tr! { "zh-Hans" => "字号", "en" => "Font size" }, label_hidden: true, current: app.danmaku_settings.font_size, continuous: true, min: 0, max: 1) => app.set_danmaku_number("font_size", app.danmaku_settings.font_size)
            }
            Picker(id: area, label: tr! { "zh-Hans" => "显示区域", "en" => "Display area" }, selected: app.preferences.danmaku_area, style: segmented) => app.set_danmaku_area(app.preferences.danmaku_area)
            Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "屏蔽", "en" => "Block" }, style: caption) }
            Row(gap: 8) {
                Switch(id: top, label: tr! { "zh-Hans" => "顶部", "en" => "Top" }, checked: app.danmaku_settings.block_top, style: button, shape: capsule, height: 30) => app.set_danmaku_switch("block_top", app.danmaku_settings.block_top)
                Switch(id: bottom, label: tr! { "zh-Hans" => "底部", "en" => "Bottom" }, checked: app.danmaku_settings.block_bottom, style: button, shape: capsule, height: 30) => app.set_danmaku_switch("block_bottom", app.danmaku_settings.block_bottom)
                Switch(id: scroll, label: tr! { "zh-Hans" => "滚动", "en" => "Scrolling" }, checked: app.danmaku_settings.block_scroll, style: button, shape: capsule, height: 30) => app.set_danmaku_switch("block_scroll", app.danmaku_settings.block_scroll)
            }
            // Five-second steps here; exact offsets in the danmaku settings.
            Column(gap: 6) {
                Row(align: center) {
                    Text(content: tr! { "zh-Hans" => "弹幕偏移", "en" => "Danmaku delay" }) Spacer
                    player_danmaku_offset(app: app)
                }
                Row(gap: 8) {
                    Button(id: danmaku_earlier, label: tr! { "zh-Hans" => "提前 5 秒", "en" => "5 s earlier" }, shape: capsule, height: 30) => app.shift_danmaku_offset(-5.0)
                    Button(id: danmaku_offset_reset, label: tr! { "zh-Hans" => "恢复", "en" => "Reset" }, shape: capsule, height: 30, enabled: app.danmaku_settings.delay < -0.05 || app.danmaku_settings.delay > 0.05) => app.set_danmaku_offset(0.0)
                    Button(id: danmaku_later, label: tr! { "zh-Hans" => "延后 5 秒", "en" => "5 s later" }, shape: capsule, height: 30) => app.shift_danmaku_offset(5.0)
                }
            }
            Button(id: more, label: tr! { "zh-Hans" => "更多弹幕设置…", "en" => "More danmaku settings…" }, prominence: plain) => app.player_tab(PlayerTab::Danmaku)
        }
    }
}

/// The danmaku offset as the viewer reads it: earlier, later or none.
#[component]
pub fn player_danmaku_offset(app: erika::Client) -> impl View {
    ui! {
        if app.danmaku_settings.delay < -0.05 {
            Text(content: tr! { "zh-Hans" => "提前 {value} 秒", "en" => "Advance {value} s"; value: app.danmaku_settings.delay_seconds })
        } else if app.danmaku_settings.delay > 0.05 {
            Text(content: tr! { "zh-Hans" => "延后 {value} 秒", "en" => "Delay {value} s"; value: app.danmaku_settings.delay_seconds })
        } else {
            Text(content: tr! { "zh-Hans" => "0.0 秒", "en" => "0.0 s" })
        }
    }
}

/// The composer's colors, one chip each with the chosen one selected, and
/// the hexadecimal field once a plugin turns it on. Only the color is chosen;
/// the comment shows in it when sent.
#[component]
pub fn player_danmaku_colors(app: erika::Client, #[session] draft: PlayerDraft) -> impl View {
    ui! {
        Column(gap: 8) {
            Flow(gap: 6) {
                ForEach(items: app.player.danmaku_colors, key: rgb) |color| {
                    Switch(id: danmaku_color, label: color.label, checked: app.player.danmaku_color == color.rgb, style: button, shape: capsule, height: 30) => app.set_danmaku_color(color.rgb)
                }
            }
            if app.player.danmaku_hex_input {
                TextInput(id: danmaku_color_hex, label: tr! { "zh-Hans" => "十六进制颜色", "en" => "Hex color" }, label_hidden: true, prompt: tr! { "zh-Hans" => "#RRGGBB，回车确定", "en" => "#RRGGBB, then Return" }, value: draft.color_hex) => app.set_danmaku_color_hex(draft.color_hex)
            }
        }
    }
}

#[component]
pub fn player_panel(app: erika::Client, #[session] draft: PlayerDraft) -> impl View {
    ui! {
        Column(gap: 0, width: 320) {
            // Keep navigation visible while long track lists scroll.
            Text(content: tr! { "zh-Hans" => "播放设置", "en" => "Playback settings" }, style: title3)
            Canvas(height: 12)
            // `.c-seg`: 34 points tall, whatever the platform's control.
            Row(height: 34, align: center) {
                // The picture page holds the enhance module's upscaling
                // alone, and the danmaku page the danmaku module's.
                if (build.modules.danmaku && app.features.danmaku) && (build.modules.enhance && app.features.enhance) {
                    Picker(id: tab, label: tr! { "zh-Hans" => "播放设置分页", "en" => "Playback settings tab" }, label_hidden: true, selected: app.player.tab, style: segmented) {
                        PickerOption(value: PlayerTab::Danmaku, label: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" })
                        PickerOption(value: PlayerTab::Subtitles, label: tr! { "zh-Hans" => "字幕", "en" => "Subtitles" })
                        PickerOption(value: PlayerTab::Picture, label: tr! { "zh-Hans" => "画面", "en" => "Picture" })
                        PickerOption(value: PlayerTab::Audio, label: tr! { "zh-Hans" => "音频", "en" => "Audio" })
                        PickerOption(value: PlayerTab::Playback, label: tr! { "zh-Hans" => "播放", "en" => "Playback" })
                    } => app.player_tab(app.player.tab)
                } else if (build.modules.danmaku && app.features.danmaku) {
                    Picker(id: tab_without_picture, label: tr! { "zh-Hans" => "播放设置分页", "en" => "Playback settings tab" }, label_hidden: true, selected: app.player.tab, style: segmented) {
                        PickerOption(value: PlayerTab::Danmaku, label: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" })
                        PickerOption(value: PlayerTab::Subtitles, label: tr! { "zh-Hans" => "字幕", "en" => "Subtitles" })
                        PickerOption(value: PlayerTab::Audio, label: tr! { "zh-Hans" => "音频", "en" => "Audio" })
                        PickerOption(value: PlayerTab::Playback, label: tr! { "zh-Hans" => "播放", "en" => "Playback" })
                    } => app.player_tab(app.player.tab)
                } else if (build.modules.enhance && app.features.enhance) {
                    Picker(id: tab_without_danmaku, label: tr! { "zh-Hans" => "播放设置分页", "en" => "Playback settings tab" }, label_hidden: true, selected: app.player.tab, style: segmented) {
                        PickerOption(value: PlayerTab::Subtitles, label: tr! { "zh-Hans" => "字幕", "en" => "Subtitles" })
                        PickerOption(value: PlayerTab::Picture, label: tr! { "zh-Hans" => "画面", "en" => "Picture" })
                        PickerOption(value: PlayerTab::Audio, label: tr! { "zh-Hans" => "音频", "en" => "Audio" })
                        PickerOption(value: PlayerTab::Playback, label: tr! { "zh-Hans" => "播放", "en" => "Playback" })
                    } => app.player_tab(app.player.tab)
                } else {
                    Picker(id: tab_plain, label: tr! { "zh-Hans" => "播放设置分页", "en" => "Playback settings tab" }, label_hidden: true, selected: app.player.tab, style: segmented) {
                        PickerOption(value: PlayerTab::Subtitles, label: tr! { "zh-Hans" => "字幕", "en" => "Subtitles" })
                        PickerOption(value: PlayerTab::Audio, label: tr! { "zh-Hans" => "音频", "en" => "Audio" })
                        PickerOption(value: PlayerTab::Playback, label: tr! { "zh-Hans" => "播放", "en" => "Playback" })
                    } => app.player_tab(app.player.tab)
                }
            }
            Canvas(height: 4)
            Stack(height: if window.short { 180 } else if app.player.tab == PlayerTab::Picture { 82 } else if app.player.tab == PlayerTab::Playback { 256 } else { 360 }) {
                ScrollView {
                    match app.player.tab {
                        PlayerTab::Danmaku => { Column(gap: 0, appear: settings_page_in) {
                                Row(height: 34, padding_x: 4) { Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "显示", "en" => "Display" }, style: caption) } Spacer }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "显示弹幕", "en" => "Show danmaku" }) Spacer Switch(id: show_danmaku, label: tr! { "zh-Hans" => "显示弹幕", "en" => "Show danmaku" }, label_hidden: true, style: switch, checked: app.preferences.show_danmaku) => app.set_preference("show_danmaku", app.preferences.show_danmaku) }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "高清弹幕", "en" => "High-resolution danmaku" }) Spacer Switch(id: danmaku_supersampling, label: tr! { "zh-Hans" => "高清弹幕", "en" => "High-resolution danmaku" }, label_hidden: true, style: switch, checked: app.danmaku_settings.supersampling) => app.set_danmaku_switch("supersampling", app.danmaku_settings.supersampling) }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Column(gap: 4, padding_x: 4, padding_y: 8) {
                                    Row(align: center) {
                                        Text(content: tr! { "zh-Hans" => "速度", "en" => "Speed" }) Spacer
                                        Text(content: tr! { "zh-Hans" => "{value} 秒/屏", "en" => "{value} s/screen"; value: app.danmaku_settings.scroll_seconds })
                                    }
                                    Slider(id: speed, label: tr! { "zh-Hans" => "速度", "en" => "Speed" }, label_hidden: true, current: app.danmaku_settings.speed, continuous: true, min: 0, max: 1) => app.set_danmaku_number("speed", app.danmaku_settings.speed)
                                }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "合并重复弹幕", "en" => "Merge duplicates" }) Spacer Switch(id: merge, label: tr! { "zh-Hans" => "合并重复弹幕", "en" => "Merge duplicates" }, label_hidden: true, style: switch, checked: app.danmaku_settings.merge) => app.set_danmaku_switch("merge", app.danmaku_settings.merge) }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "随机染色", "en" => "Random colors" }) Spacer Switch(id: random_colors, label: tr! { "zh-Hans" => "随机染色", "en" => "Random colors" }, label_hidden: true, style: switch, checked: app.danmaku_settings.random_colors) => app.set_danmaku_switch("random_colors", app.danmaku_settings.random_colors) }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Column(gap: 4, padding_x: 4, padding_y: 8) {
                                    Row(align: center) {
                                        Text(content: tr! { "zh-Hans" => "弹幕偏移", "en" => "Danmaku delay" }) Spacer
                                        player_danmaku_offset(app: app)
                                    }
                                    Slider(id: danmaku_delay, label: tr! { "zh-Hans" => "弹幕偏移", "en" => "Danmaku delay" }, label_hidden: true, current: app.danmaku_settings.delay, continuous: true, min: -10, max: 10) => app.set_danmaku_number("delay", app.danmaku_settings.delay)
                                }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(height: 34, padding_x: 4) { Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "来源", "en" => "Sources" }, style: caption) } Spacer }
                                // The files liberika shows, by name, with the comments they hold.
                                if app.player.danmaku_count > 0 {
                                    Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: app.player.danmaku_source, lines: 1) Spacer Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "{count} 条", "en" => "{count}"; count: app.player.danmaku_count }, style: body) } }
                                } else {
                                    Row(gap: 12, height: 46, padding_x: 4, align: center) { Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "没有弹幕", "en" => "No danmaku" }) } Spacer }
                                }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                // A television has no files to mount.
                                if !window.tv { Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "挂载本地弹幕", "en" => "Local danmaku" }) Spacer Button(id: local_danmaku, label: tr! { "zh-Hans" => "xml / json", "en" => "xml / json" }, prominence: plain, pick: draft.danmaku_file, files: ".xml .json") => app.mount_danmaku(draft.danmaku_file) } }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                            } }
                        PlayerTab::Subtitles => { Column(gap: 0, appear: settings_page_in) {
                                Row(height: 34, padding_x: 4) { Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "样式", "en" => "Style" }, style: caption) } Spacer }
                                Column(gap: 4, padding_x: 4, padding_y: 8) {
                                    Row(align: center) {
                                        Text(content: tr! { "zh-Hans" => "字号", "en" => "Font size" }) Spacer
                                        Text(content: tr! { "zh-Hans" => "{value}%", "en" => "{value}%"; value: app.player.subtitle_scale_percent })
                                    }
                                    Slider(id: subtitle_size, label: tr! { "zh-Hans" => "字号", "en" => "Font size" }, label_hidden: true, current: app.player.subtitle_size, continuous: true, min: 0, max: 1) => app.set_player_number("subtitle_size", app.player.subtitle_size)
                                }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "高清字幕", "en" => "High-resolution subtitles" }) Spacer Switch(id: subtitle_supersampling, label: tr! { "zh-Hans" => "高清字幕", "en" => "High-resolution subtitles" }, label_hidden: true, style: switch, checked: app.player.subtitle_supersampling) => app.set_subtitle_supersampling(app.player.subtitle_supersampling) }
                                Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "文字更清晰，大小不变；性能不足时自动降低精度。", "en" => "Sharper text at the same size; adjusts automatically for performance." }, style: caption) }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(height: 34, padding_x: 4) { Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "轨道", "en" => "Tracks" }, style: caption) } Spacer }
                                // The media's own tracks, as the file names them.
                                ForEach(items: app.player.subtitle_tracks, key: id) |track| {
                                    Button(id: subtitle_track, label: track.name, variant: player_picture) {
                                        Row(gap: 12, height: 46, padding_x: 4, align: center) {
                                            Text(content: track.name, lines: 1) Spacer
                                            if app.player.subtitle_track == track.id { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) }
                                        }
                                    } => app.select_subtitle_track(track.id)
                                    Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                }
                                Button(id: subtitles_off, label: tr! { "zh-Hans" => "关闭字幕", "en" => "Subtitles off" }, variant: player_picture) {
                                    Row(gap: 12, height: 46, padding_x: 4, align: center) {
                                        Text(content: tr! { "zh-Hans" => "关闭字幕", "en" => "Subtitles off" }) Spacer
                                        if app.player.subtitle_track < 0 { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) }
                                    }
                                } => app.select_subtitle_track(-1)
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                            } }
                        PlayerTab::Picture => { Column(gap: 0, appear: settings_page_in) {
                                Row(height: 34, padding_x: 4) { Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "画质增强", "en" => "Enhancement" }, style: caption) } Spacer }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "超分辨率", "en" => "Upscaling" }) Spacer Picker(id: enhancement, label: tr! { "zh-Hans" => "超分辨率", "en" => "Upscaling" }, label_hidden: true, style: menu, selected: app.player.enhancement) => app.set_enhancement(app.player.enhancement) }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                            } }
                        PlayerTab::Audio => { Column(gap: 0, appear: settings_page_in) {
                                Row(height: 34, padding_x: 4) { Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "音频", "en" => "Audio" }, style: caption) } Spacer }
                                Column(gap: 4, padding_x: 4, padding_y: 8) {
                                    Row(align: center) {
                                        Text(content: tr! { "zh-Hans" => "音量", "en" => "Volume" }) Spacer
                                        Text(content: tr! { "zh-Hans" => "{value}%", "en" => "{value}%"; value: app.player.volume_percent })
                                    }
                                    Slider(id: volume, label: tr! { "zh-Hans" => "音量", "en" => "Volume" }, label_hidden: true, current: app.player.volume, continuous: true, min: 0, max: 1) => app.set_player_number("volume", app.player.volume)
                                }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(height: 34, padding_x: 4) { Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "音轨", "en" => "Audio tracks" }, style: caption) } Spacer }
                                ForEach(items: app.player.audio_tracks, key: id) |track| {
                                    Button(id: audio_track, label: track.name, variant: player_picture) {
                                        Row(gap: 12, height: 46, padding_x: 4, align: center) {
                                            Text(content: track.name, lines: 1) Spacer
                                            if app.player.audio_track == track.id { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) }
                                        }
                                    } => app.select_audio_track(track.id)
                                    Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                }
                            } }
                        PlayerTab::Playback => { Column(gap: 0, appear: settings_page_in) {
                                if app.player.episodes {
                                    player_episodes(app: app)
                                    Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                }
                                Row(height: 34, padding_x: 4) { Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "播放", "en" => "Playback" }, style: caption) } Spacer }
                                if app.player.has_chapters {
                                    ForEach(items: app.player.chapters, key: id) |chapter| {
                                        Button(id: chapter, label: chapter.title, variant: player_picture) {
                                            Row(gap: 12, height: 40, padding_x: 4, align: center) {
                                                Text(content: tr! { "zh-Hans" => "{time, duration, mm:ss}", "en" => "{time, duration, mm:ss}"; time: chapter.start }, style: caption)
                                                Text(content: chapter.title, lines: 1) Spacer
                                                if app.player.current_chapter == chapter.id { Label(title: tr! { "zh-Hans" => "当前章节", "en" => "Current chapter" }, icon: "checkmark", display: icon) }
                                            }
                                        } => app.seek_chapter(chapter.id)
                                    }
                                    Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "片头／片尾跳过提示", "en" => "Opening / ending skip hint" }) Spacer Switch(id: skip_intro, label: tr! { "zh-Hans" => "片头／片尾跳过提示", "en" => "Opening / ending skip hint" }, label_hidden: true, style: switch, checked: app.player.skip_intro) => app.set_skip_intro(app.player.skip_intro) }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "自动连播下一集", "en" => "Play next automatically" }) Spacer Switch(id: autoplay, label: tr! { "zh-Hans" => "自动连播下一集", "en" => "Play next automatically" }, label_hidden: true, style: switch, checked: app.preferences.autoplay) => app.set_preference("autoplay", app.preferences.autoplay) }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(height: 34, padding_x: 4) { Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "信息", "en" => "Information" }, style: caption) } Spacer }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "播放内核", "en" => "Playback engine" }) Spacer Stack(variant: player_secondary) { Text(content: tr! { "zh-Hans" => "liberika", "en" => "liberika" }, style: body) } }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "视频", "en" => "Video" }) Spacer Stack(variant: player_secondary) { Text(content: app.player.video_info, style: body) } }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                            } }
                    }
                }
            }
        }
    }
}

#[component]
pub fn player_episodes(app: erika::Client) -> impl View {
    ui! {
        Column(gap: 12, padding: 12, width: 276) {
            Text(content: tr! { "zh-Hans" => "选集", "en" => "Episodes" }, style: headline)
            Stack(height: 280) {
                ScrollView {
                    Column(gap: 4) {
                        ForEach(items: app.player.episode_list, key: id) |episode| {
                            Switch(id: episode, label: episode.name, checked: app.player.queue_index == episode.id, style: button) => app.select_episode(episode.id)
                        }
                    }
                }
            }
        }
    }
}

/// Subtitle and audio tracks open together, beside the button that selects them.
#[component]
pub fn player_subtitle_menu(app: erika::Client, #[session] draft: PlayerDraft) -> impl View {
    ui! {
        PopoverButton(panel_appearance: surface, id: subtitles, label: tr! { "zh-Hans" => "字幕与音轨", "en" => "Subtitles and audio tracks" }, icon: "text.rectangle", presented: draft.tracks_open) {
            Column(gap: 12, width: 320, padding: 8) {
                Text(content: tr! { "zh-Hans" => "字幕与音轨", "en" => "Subtitles and audio tracks" }, style: title3)
                Stack(height: 320) {
                    ScrollView {
                        Column(gap: 4) {
                            Text(content: tr! { "zh-Hans" => "字幕轨道", "en" => "Subtitle tracks" }, style: caption)
                            Button(id: no_subtitle, label: tr! { "zh-Hans" => "关闭字幕", "en" => "Subtitles off" }, variant: player_picture) {
                                Row(height: 36, padding_x: 4) {
                                    Text(content: tr! { "zh-Hans" => "关闭字幕", "en" => "Subtitles off" }) Spacer
                                    if app.player.subtitle_track < 0 { Stack(variant: player_accent) { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) } }
                                }
                            } => app.select_subtitle_track(-1)
                            ForEach(items: app.player.subtitle_tracks, key: id) |track| {
                                Button(id: quick_subtitle_track, label: track.name, variant: player_picture) {
                                    Row(height: 36, padding_x: 4) {
                                        Text(content: track.name, lines: 1) Spacer
                                        if app.player.subtitle_track == track.id { Stack(variant: player_accent) { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) } }
                                    }
                                } => app.select_subtitle_track(track.id)
                            }
                            Button(id: external_subtitle, label: tr! { "zh-Hans" => "添加外挂字幕…", "en" => "Add external subtitles…" }, icon: "plus", prominence: plain, pick: draft.subtitle_file, files: ".ass .ssa .srt .vtt") => app.attach_subtitle(draft.subtitle_file)
                            Divider
                            Text(content: tr! { "zh-Hans" => "音频轨道", "en" => "Audio tracks" }, style: caption)
                            ForEach(items: app.player.audio_tracks, key: id) |track| {
                                Button(id: quick_audio_track, label: track.name, variant: player_picture) {
                                    Row(height: 36, padding_x: 4) {
                                        Text(content: track.name, lines: 1) Spacer
                                        if app.player.audio_track == track.id { Stack(variant: player_accent) { Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon) } }
                                    }
                                } => app.select_audio_track(track.id)
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The television's subtitles capsule: as the others, without files to
/// pick.
#[component]
pub fn player_tv_subtitle_menu(app: erika::Client) -> impl View {
    ui! {
        if app.player.subtitle_track >= 0 {
            Menu(id: subtitles_tv, label: app.player.subtitle_name, icon: "text.rectangle", shape: capsule, height: 46) {
                MenuItem(id: subtitle_tracks_tv, label: tr! { "zh-Hans" => "字幕轨道…", "en" => "Subtitle tracks…" }) => app.player_tab(PlayerTab::Subtitles)
                MenuItem(id: no_subtitle_tv, label: tr! { "zh-Hans" => "关闭字幕", "en" => "Subtitles off" }) => app.select_subtitle_track(-1)
                Divider
                MenuItem(id: audio_tracks_tv, label: tr! { "zh-Hans" => "音轨…", "en" => "Audio tracks…" }) => app.player_tab(PlayerTab::Audio)
            }
        } else {
            Menu(id: subtitles_off_tv, label: tr! { "zh-Hans" => "关闭", "en" => "Off" }, icon: "text.rectangle", shape: capsule, height: 46) {
                MenuItem(id: subtitle_tracks_off_tv, label: tr! { "zh-Hans" => "字幕轨道…", "en" => "Subtitle tracks…" }) => app.player_tab(PlayerTab::Subtitles)
                Divider
                MenuItem(id: audio_tracks_off_tv, label: tr! { "zh-Hans" => "音轨…", "en" => "Audio tracks…" }) => app.player_tab(PlayerTab::Audio)
            }
        }
    }
}

/// Available only inside a recognized interval; remains reachable when controls hide.
#[component]
pub fn player_skip_button(app: erika::Client) -> impl View {
    ui! {
        if app.player.skip_kind == SkipKind::Opening {
            Button(id: skip_opening, label: tr! { "zh-Hans" => "跳过片头", "en" => "Skip opening" }, icon: "forward.end.fill", shape: capsule) => app.skip_segment()
        } else if app.player.skip_kind == SkipKind::Ending {
            Button(id: skip_ending, label: tr! { "zh-Hans" => "跳过片尾", "en" => "Skip ending" }, icon: "forward.end.fill", shape: capsule) => app.skip_segment()
        }
    }
}

#[component]
pub fn matching_show_summary(show: MatchingShow, detailed: bool) -> impl View {
    ui! {
        Row(gap: 14, align: top) {
            if show.poster.url != "" {
                Image(source: show.poster, width: 72, height: 108, fit: cover, corner: 8, label: show.name)
            } else {
                Stack(variant: matching_poster, width: 72, height: 108) {
                    Label(title: tr! { "zh-Hans" => "暂无海报", "en" => "No poster" }, icon: "photo", display: icon)
                }
            }
            Column(gap: 7) {
                Text(content: show.name, style: headline, lines: 2)
                Row(gap: 8) {
                    if show.kind != "" { Text(content: show.kind, style: caption, emphasis: secondary) }
                    if show.episode_count > 0 {
                        Text(content: tr! { "zh-Hans" => "{count} 集", "en" => "{count} episodes"; count: show.episode_count }, style: caption, emphasis: secondary)
                    }
                    Spacer
                }
                if show.summary != "" {
                    if detailed {
                        Text(content: show.summary, style: body, lines: 5, emphasis: secondary)
                    } else {
                        Text(content: show.summary, style: body, lines: 3, emphasis: secondary)
                    }
                } else if show.loading {
                    Text(content: tr! { "zh-Hans" => "正在加载简介…", "en" => "Loading synopsis…" }, style: caption, emphasis: secondary)
                } else {
                    Text(content: tr! { "zh-Hans" => "暂无简介", "en" => "No synopsis available" }, style: caption, emphasis: secondary)
                }
            }
            if !detailed {
                Stack(variant: player_accent) { Label(title: tr! { "zh-Hans" => "选择集数", "en" => "Choose episode" }, icon: "chevron.right", display: icon) }
            }
        }
    }
}

#[component]
pub fn player_matching(app: erika::Client, #[session] draft: PlayerDraft) -> impl View {
    ui! {
        Column(gap: 14, width: if window.short { 340 } else { 420 }, padding: 8) {
            Text(content: tr! { "zh-Hans" => "匹配弹幕", "en" => "Match danmaku" }, style: title3)
            Row(gap: 10) {
                TextInput(id: match_query, label: tr! { "zh-Hans" => "作品名称", "en" => "Show title" }, label_hidden: true, prompt: tr! { "zh-Hans" => "输入作品名称", "en" => "Search by title" }, value: draft.match_query) => app.search_matching(draft.match_query)
                Button(id: search, label: tr! { "zh-Hans" => "搜索", "en" => "Search" }, shape: capsule, height: 32, prominence: prominent) => app.search_matching(draft.match_query)
            }
            if app.matching.error != "" { Text(content: app.matching.error, style: caption) }
            if let Some(show) = app.matching.selected {
                Row {
                    Button(id: match_back, label: tr! { "zh-Hans" => "返回搜索结果", "en" => "Back to results" }, icon: "chevron.left", shape: capsule, height: 30) => app.back_to_match_results()
                    Spacer
                }
                Stack(height: if window.short { 200 } else { 350 }) {
                    ScrollView {
                        Column(gap: 12) {
                            Stack(variant: matching_detail) { Column(padding: 12) { matching_show_summary(show: show, detailed: true) } }
                            Text(content: tr! { "zh-Hans" => "选择集数以匹配弹幕", "en" => "Choose an episode to match danmaku" }, style: caption, emphasis: secondary)
                            if app.matching.loading { Progress(label: tr! { "zh-Hans" => "正在加载集数…", "en" => "Loading episodes…" }) }
                            ForEach(items: app.matching.episodes, key: id) |episode| {
                                Button(id: match_episode, label: episode.name, width: fill, variant: matching_card, enabled: !app.matching.loading) {
                                    Row(gap: 12, align: center, height: 24) {
                                        Text(content: episode.name, lines: 1)
                                        Spacer
                                        Stack(variant: player_accent) { Label(title: tr! { "zh-Hans" => "匹配", "en" => "Match" }, icon: "link", display: icon) }
                                    }
                                } => app.confirm_match_episode(episode.id)
                            }
                        }
                    }
                }
            } else {
                Text(content: tr! { "zh-Hans" => "选择作品，再选择当前播放的集数", "en" => "Choose a show, then the episode you are watching" }, style: caption, emphasis: secondary)
                Stack(height: if window.short { 200 } else { 350 }) {
                    ScrollView {
                        Column(gap: 10) {
                            if app.matching.loading { Progress(label: tr! { "zh-Hans" => "正在搜索…", "en" => "Searching…" }) }
                            ForEach(items: app.matching.shows, key: id) |show| {
                                Button(id: match_show, label: show.name, width: fill, variant: matching_card, enabled: !app.matching.loading) {
                                    matching_show_summary(show: show, detailed: false)
                                } => app.choose_match_show(show.id)
                            }
                        }
                    }
                }
            }
        }
    }
}
