//! The player in design/next-player.js: one frame, three geometries.
//! Native controls surround the picture, with a separate settings column.
use bern::prelude::*;
use erika_protocol::erika::{self, *};
use crate::Players;

#[bern::session]
pub struct PlayerDraft {
    pub text: String,
    pub volume_open: bool,
    pub episodes_open: bool,
    /// The television's danmaku field, in place of the seek bar.
    pub saying: bool,
    pub subtitle_file: MediaSource,
    pub danmaku_file: MediaSource,
}

#[component]
pub fn player(app: erika::Client, players: Players, #[session] draft: PlayerDraft) -> impl View {
    ui! {
        Stack(appearance: dark, variant: player_black) {
            // A phone on its side plays in the design's 844 × 390 scene, the
            // controls in the two rails beside the picture (`RAILS`).
            // The television's player (design 6.2 `TV`): the frame steps back
            // less, the controls run across the screen.
            if window.tv {
                Stack(under_bars: true) {
                    ScaleBox(width: 1280, height: 720) {
                        player_tv_scene(app: app, players: players, shown: !window.idle || app.paused || app.player.panel || draft.episodes_open || draft.saying)
                    }
                }
            } else if window.short {
                ScaleBox(width: 844, height: 390) {
                    player_phone_scene(app: app, players: players, shown: !window.idle || app.paused || app.player.panel || draft.volume_open || draft.episodes_open)
                }
            } else {
                ScaleBox(width: 1280, height: 800) {
                    player_scene(app: app, players: players, shown: !window.idle || app.paused || app.player.panel || draft.volume_open || draft.episodes_open)
                }
            }
            if let Some(notice) = app.notice {
                Toast(content: notice, key: app.notice_revision, duration: 3)
            }
        }
    }
}

#[component]
pub fn player_scene(app: erika::Client, players: Players, shown: bool, #[session] draft: PlayerDraft) -> impl View {
    ui! {
        Stack(variant: player_black, width: 1280, height: 800, align: top_leading) {
            // The scene keeps the design's 1280 × 800 at any window shape:
            // the background and the picture when it fills the window bleed
            // past the scene's edges to the window's (`.x-player`, whose
            // background is the window's).
            if shown { Positioned(x: 0, y: 0, width: 1280, height: 800, bleed: true, appear: fade, disappear: fade) { Stack(variant: player_background) {} } }
            // Filling the window, the picture is the whole scene, letterboxed
            // (`.cv video { object-fit: contain }`): 1280 × 720 in the
            // design's window, and as wide as a wider window.
            Positioned(x: if app.player.panel { 24 } else if shown && app.preferences.step_back { 103 } else { 0 }, y: if app.player.panel { 114 } else if shown && app.preferences.step_back { 64 } else { 0 }, width: if app.player.panel { 896 } else if shown && app.preferences.step_back { 1074 } else { 1280 }, height: if app.player.panel { 504 } else if shown && app.preferences.step_back { 604.125 } else { 800 }, bleed: !app.player.panel && !(shown && app.preferences.step_back), animate: hero) {
                // The controls hide a second after a mouse comes to rest on
                // the picture, and stay while it is anywhere else.
                Button(id: picture, label: tr! { "zh-Hans" => "播放或暂停画面", "en" => "Play or pause picture" }, variant: player_picture, idle_area: 1) {
                    // The picture keeps the video's 16:9 inside its place;
                    // the corner is the frame's on screen once scaled to 1074
                    // or 896.
                    ScaleBox(width: 1280, height: 720) {
                        Stack(width: 1280, height: 720) {
                            Image(source: app.still, width: 1280, height: 720, corner: if app.player.panel { frame_beside } else if shown && app.preferences.step_back { frame_back } else { 0 }, fit: cover, label: app.title, hero: frame, hero_key: app.title, animate: hero)
                            // liberika's picture over the still, letterboxed
                            // in the frame with its subtitles and danmaku.
                            if app.video {
                                Video(id: video, player: players.video, label: app.title, controls: none, corner: if app.player.panel { frame_beside } else if shown && app.preferences.step_back { frame_back } else { 0 }, animate: hero)
                            }
                            player_overlays(app: app)
                        }
                    }
                } => app.toggle_pause()
            }
            if shown {
                Positioned(x: 14, y: 0, width: 1246, height: 64, appear: top_bar_in, disappear: top_bar_out) {
                    player_top(app: app)
                }
                Positioned(x: if app.player.panel { 24 } else { 103 }, y: if app.player.panel { 618 } else { 668.125 }, width: if app.player.panel { 896 } else { 1074 }, height: 60, animate: hero, appear: controls_in, disappear: controls_out) {
                    player_timeline(app: app)
                }
                Positioned(x: if app.player.panel { 24 } else { 103 }, y: 734, width: if app.player.panel { 1232 } else { 1074 }, height: 48, animate: hero, appear: controls_in, disappear: controls_out) {
                    player_controls(app: app)
                }
            }
            if app.player.panel {
                Positioned(x: 936, y: 64, width: 320, height: 604, appear: player_panel_in, disappear: player_panel_in) {
                    player_panel(app: app)
                }
            }
        }
    }
}

/// The television's player (next-player.js `RECT.tv`, `CTL.tv`, next.css
/// `.ff-tv`), in the 1280 × 720 scene the screen shows 1.5 times as large:
/// the picture fills the screen, steps back to (200, 52) 880 × 495 when the
/// controls show, or to (40, 100) 784 × 441 beside the settings. The bar
/// is 52 tall, the seek bar under the picture, and the controls run across
/// the screen at 624 (64, 1152 wide). With the controls hidden the picture
/// holds the remote, as on tvOS's own player: left and right skip 10
/// seconds and select pauses; Back leaves the settings, then the player.
#[component]
pub fn player_tv_scene(app: erika::Client, players: Players, shown: bool, #[session] draft: PlayerDraft) -> impl View {
    ui! {
        Stack(variant: player_black, width: 1280, height: 720, align: top_leading) {
            if shown { Positioned(x: 0, y: 0, width: 1280, height: 720) { Stack(variant: player_background, width: 1280, height: 720) {} } }
            Positioned(x: if app.player.panel { 40 } else if shown { 200 } else { 0 }, y: if app.player.panel { 100 } else if shown { 52 } else { 0 }, width: if app.player.panel { 784 } else if shown { 880 } else { 1280 }, height: if app.player.panel { 441 } else if shown { 495 } else { 720 }, animate: hero) {
                Stack {
                    Image(source: app.still, width: if app.player.panel { 784 } else if shown { 880 } else { 1280 }, height: if app.player.panel { 441 } else if shown { 495 } else { 720 }, corner: if app.player.panel || shown { card } else { 0 }, fit: cover, label: app.title, hero: frame, hero_key: app.title, animate: hero)
                    if app.video {
                        Video(id: video_tv, player: players.video, label: app.title, controls: none, corner: if app.player.panel || shown { card } else { 0 }, animate: hero)
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
                Positioned(x: if app.player.panel { 40 } else { 200 }, y: if app.player.panel { 545 } else { 551 }, width: if app.player.panel { 784 } else { 880 }, height: 44, animate: hero, appear: controls_in, disappear: controls_out) {
                    if draft.saying {
                        // 「发弹幕」: the field in the seek bar's place; the
                        // system's keyboard takes the remote's dictation.
                        Row(gap: 8, align: center) {
                            TextInput(id: danmaku_text_tv, label: tr! { "zh-Hans" => "发弹幕", "en" => "Send danmaku" }, label_hidden: true, prompt: tr! { "zh-Hans" => "发个友善的弹幕…", "en" => "Say something kind…" }, value: draft.text, submit: clear) => app.send_danmaku(draft.text)
                            IconButton(id: unsay_tv, icon: "xmark", label: tr! { "zh-Hans" => "收起", "en" => "Close" }, height: 40) => draft.saying.set(false)
                        }
                    } else {
                        Row(gap: 12, align: center) {
                            Text(content: tr! { "zh-Hans" => "{time, duration, mm:ss}", "en" => "{time, duration, mm:ss}"; time: app.elapsed }, style: caption2)
                            // Left and right on the focused bar: 10 seconds
                            // of a 24-minute episode.
                            Slider(id: position_tv, label: tr! { "zh-Hans" => "播放进度", "en" => "Playback position" }, label_hidden: true, current: app.progress, min: 0, max: 1, step: 0.00694) => app.seek(app.progress)
                            Text(content: tr! { "zh-Hans" => "{time, duration, mm:ss}", "en" => "{time, duration, mm:ss}"; time: app.duration }, style: caption2, emphasis: secondary)
                        }
                    }
                }
                Positioned(x: if app.player.panel { 40 } else { 64 }, y: 624, width: if app.player.panel { 784 } else { 1152 }, height: 56, animate: hero, appear: controls_in, disappear: controls_out) {
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
                // `.ff-tv .p-sheet`: 848, 52, 392 × 548.
                Positioned(x: 848, y: 52, width: 392, height: 548, appear: player_panel_in, disappear: player_panel_in) {
                    player_panel(app: app)
                }
            } else if !app.features.anime && !app.features.library && !app.features.downloads {
                Shortcut(key: "escape") => app.return_to_frame()
            } else {
                Shortcut(key: "escape") => app.minimize()
            }
        }
    }
}

/// The television's bar (`.ff-tv .p-top`, 52 tall, 56 in): the title with
/// its episode, then danmaku matching, picture in picture, the settings
/// and more; no volume or full screen, which the remote and the television
/// have.
#[component]
pub fn player_tv_top(app: erika::Client, #[session] draft: PlayerDraft) -> impl View {
    ui! {
        Row(gap: 12, height: 52, align: center) {
            Column(gap: 2) {
                Text(content: app.title, style: title3, lines: 1)
                Text(content: app.subtitle, style: caption, emphasis: secondary)
            }
            Spacer
            ControlGroup(label: tr! { "zh-Hans" => "播放选项", "en" => "Playback options" }, height: 46) {
                if app.features.danmaku {
                    IconButton(id: match_danmaku_tv, icon: "link", label: tr! { "zh-Hans" => "弹幕匹配", "en" => "Match danmaku" }) => app.show_notice(Notice::DanmakuMatchPending)
                }
                IconButton(id: settings_tv, icon: "slider.horizontal.3", label: tr! { "zh-Hans" => "播放设置", "en" => "Playback settings" }) => app.player_panel(!app.player.panel)
                Menu(id: more_tv, label: tr! { "zh-Hans" => "更多", "en" => "More" }, icon: "ellipsis", style: icon) {
                    MenuItem(id: screenshot_tv, label: tr! { "zh-Hans" => "截图", "en" => "Screenshot" }, icon: "camera") => app.screenshot()
                    MenuItem(id: skip_intro_tv, label: tr! { "zh-Hans" => "跳过片头（85 秒）", "en" => "Skip opening (85 seconds)" }) => app.skip(85.0)
                    MenuItem(id: episodes_tv, label: tr! { "zh-Hans" => "选集…", "en" => "Episodes…" }, enabled: app.player.episodes) => draft.episodes_open.set(true)
                    MenuItem(id: info_tv, label: tr! { "zh-Hans" => "播放信息", "en" => "Playback information" }) => app.player_tab(PlayerTab::Playback)
                }
            }
            if draft.episodes_open {
                PopoverButton(id: episode_list_tv, label: tr! { "zh-Hans" => "选集", "en" => "Episodes" }, presented: draft.episodes_open) {
                    player_episodes(app: app)
                }
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
                if app.features.danmaku {
                    SplitButton(id: danmaku_tv, label: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, more_label: tr! { "zh-Hans" => "弹幕参数", "en" => "Danmaku options" }, icon: "text.bubble", checked: app.preferences.show_danmaku, shape: capsule, height: 46) {
                        player_danmaku_quick(app: app)
                    } => app.set_preference("show_danmaku", app.preferences.show_danmaku)
                    if app.features.sync {
                        Button(id: say_tv, label: tr! { "zh-Hans" => "发弹幕", "en" => "Send Danmaku" }, icon: "paperplane", shape: capsule, height: 46) => draft.saying.set(true)
                    }
                }
                Spacer
                player_tv_subtitle_menu(app: app)
                Picker(id: rate_tv, label: tr! { "zh-Hans" => "倍速", "en" => "Speed" }, selected: app.preferences.speed, label_hidden: true, style: menu, shape: capsule, height: 46) => app.set_speed(app.preferences.speed)
                if app.features.enhance {
                    Picker(id: enhancement_tv, label: tr! { "zh-Hans" => "画质增强", "en" => "Enhancement" }, selected: app.player.enhancement, label_hidden: true, style: menu, shape: capsule, height: 46) => app.set_enhancement(app.player.enhancement)
                }
            }
            Row(gap: 14, align: center, height: 56) {
                if app.player.episodes {
                    IconButton(id: previous_tv, icon: "backward.end.fill", height: 46, label: tr! { "zh-Hans" => "上一集", "en" => "Previous episode" }, enabled: app.player.episode > 1) => app.change_episode(-1)
                }
                IconButton(id: rewind_tv, icon: "gobackward.10", font_size: 20, height: 46, label: tr! { "zh-Hans" => "快退 10 秒", "en" => "Back 10 seconds" }) => app.skip(-10.0)
                if app.paused {
                    IconButton(id: play_tv, icon: "play.fill", font_size: 24, height: 56, prominence: prominent, default_focus: true, label: tr! { "zh-Hans" => "播放", "en" => "Play" }) => app.toggle_pause()
                } else {
                    IconButton(id: pause_tv, icon: "pause.fill", font_size: 24, height: 56, prominence: prominent, default_focus: true, label: tr! { "zh-Hans" => "暂停", "en" => "Pause" }) => app.toggle_pause()
                }
                IconButton(id: forward_tv, icon: "goforward.10", font_size: 20, height: 46, label: tr! { "zh-Hans" => "快进 10 秒", "en" => "Forward 10 seconds" }) => app.skip(10.0)
                if app.player.episodes {
                    IconButton(id: next_tv, icon: "forward.end.fill", height: 46, label: tr! { "zh-Hans" => "下一集", "en" => "Next episode" }, enabled: app.player.episode < 12) => app.change_episode(1)
                }
            }
        }
    }
}

/// The phone's player on its side (next-player.js `RECT.phone`, `RAILS`):
/// the picture filling the height, or with the controls 519 points wide
/// between two rails of 162; the bar above, the seek bar under it.
#[component]
pub fn player_phone_scene(app: erika::Client, players: Players, shown: bool, #[session] draft: PlayerDraft) -> impl View {
    ui! {
        Stack(variant: player_black, width: 844, height: 390, align: top_leading) {
            if shown { Positioned(x: 0, y: 0, width: 844, height: 390) { Stack(variant: player_background, width: 844, height: 390) {} } }
            // `RECT.phone`: full, shown, or beside the settings panel.
            Positioned(x: if app.player.panel { 16 } else if shown { 162.5 } else { 75.5 }, y: if app.player.panel { 56 } else if shown { 48 } else { 0 }, width: if app.player.panel { 512 } else if shown { 519 } else { 693 }, height: if app.player.panel { 288.14 } else if shown { 292.125 } else { 389.8125 }, animate: hero) {
                Button(id: picture_phone, label: tr! { "zh-Hans" => "播放或暂停画面", "en" => "Play or pause picture" }, variant: player_picture, idle_area: 1) {
                    Stack {
                        Image(source: app.still, width: if app.player.panel { 512 } else if shown { 519 } else { 693 }, height: if app.player.panel { 288.14 } else if shown { 292.125 } else { 389.8125 }, corner: if shown || app.player.panel { 16 } else { 0 }, fit: cover, label: app.title, hero: frame, hero_key: app.title, animate: hero)
                        if app.video {
                            Video(id: video_phone, player: players.video, label: app.title, controls: none, corner: if shown || app.player.panel { 16 } else { 0 }, animate: hero)
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
            if shown && !app.player.panel {
                // The left rail: danmaku, then the previous episode and back 10.
                Positioned(x: 0, y: 148, width: 162, height: 34, appear: controls_in, disappear: controls_out) {
                    Row(gap: 0, align: center) {
                        Spacer
                        if app.features.danmaku {
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
                            IconButton(id: previous_phone, icon: "backward.end.fill", font_size: 11, height: 46, label: tr! { "zh-Hans" => "上一集", "en" => "Previous episode" }, enabled: app.player.episode > 1) => app.change_episode(-1)
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
                        player_subtitle_menu(app: app, height: 32)
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
                            IconButton(id: next_phone, icon: "forward.end.fill", font_size: 11, height: 46, label: tr! { "zh-Hans" => "下一集", "en" => "Next episode" }, enabled: app.player.episode < 12) => app.change_episode(1)
                        }
                        Spacer
                    }
                }
            }
            if shown {
                // `.p-seek`, 3 under the picture: the time, the seek bar 54
                // in from both ends, the length.
                Positioned(x: if app.player.panel { 16 } else { 162.5 }, y: if app.player.panel { 361.1 } else { 357.1 }, width: 35.5, height: 16, animate: hero, appear: controls_in, disappear: controls_out) {
                    Text(content: tr! { "zh-Hans" => "{time, duration, mm:ss}", "en" => "{time, duration, mm:ss}"; time: app.elapsed }, style: caption2, font_size: 12)
                }
                Positioned(x: if app.player.panel { 70 } else { 216.5 }, y: if app.player.panel { 355.1 } else { 351.1 }, width: if app.player.panel { 404 } else { 411 }, height: 28, animate: hero, appear: controls_in, disappear: controls_out) {
                    Slider(id: position_phone, label: tr! { "zh-Hans" => "播放进度", "en" => "Playback position" }, label_hidden: true, current: app.progress, min: 0, max: 1) => app.seek(app.progress)
                }
                // The chapters as dots on the track; no room for their names
                // under it, so the danmaku peaks are accent dots above it.
                if app.player.episodes {
                    Positioned(x: if app.player.panel { 70 } else { 216.5 }, y: if app.player.panel { 367.6 } else { 363.6 }, width: if app.player.panel { 404 } else { 411 }, height: 3, animate: hero, appear: controls_in, disappear: controls_out) {
                        Rail {
                        Canvas(at: 0.06323, width: 3, height: 3) { Shape(kind: ellipse, fill: text.primary, opacity: 0.7) }
                        Canvas(at: 0.12509, width: 3, height: 3) { Shape(kind: ellipse, fill: text.primary, opacity: 0.7) }
                        Canvas(at: 0.4811, width: 3, height: 3) { Shape(kind: ellipse, fill: text.primary, opacity: 0.7) }
                        Canvas(at: 0.89003, width: 3, height: 3) { Shape(kind: ellipse, fill: text.primary, opacity: 0.7) }
                        Canvas(at: 0.95189, width: 3, height: 3) { Shape(kind: ellipse, fill: text.primary, opacity: 0.7) }
                        }
                    }
                    if app.features.danmaku {
                        Positioned(x: if app.player.panel { 70 } else { 216.5 }, y: if app.player.panel { 353.6 } else { 349.6 }, width: if app.player.panel { 404 } else { 411 }, height: 10, animate: hero, appear: controls_in, disappear: controls_out) {
                            Rail {
                            Canvas(at: 0.23505, width: 10, height: 10) { Shape(kind: ellipse, fill: surface.canvas, opacity: 0.4) Shape(kind: ellipse, x: 1.5, y: 1.5, width: 7, height: 7, fill: accent) }
                            Canvas(at: 0.75464, width: 10, height: 10) { Shape(kind: ellipse, fill: surface.canvas, opacity: 0.4) Shape(kind: ellipse, x: 1.5, y: 1.5, width: 7, height: 7, fill: accent) }
                            }
                        }
                    }
                }
                Positioned(x: if app.player.panel { 492.5 } else { 646 }, y: if app.player.panel { 361.1 } else { 357.1 }, width: 35.5, height: 16, animate: hero, appear: controls_in, disappear: controls_out) {
                    Text(content: tr! { "zh-Hans" => "{time, duration, mm:ss}", "en" => "{time, duration, mm:ss}"; time: app.duration }, style: caption2, font_size: 12, emphasis: secondary)
                }
            }
            // `.ph .p-sheet`: the settings beside the picture.
            if app.player.panel {
                Positioned(x: 544, y: 56, width: 284, height: 322, appear: player_panel_in, disappear: player_panel_in) {
                    player_panel(app: app)
                }
            }
        }
    }
}

/// The phone player's bar (`.p-top`, 48 points): the way back on a glass
/// circle, the title with its episode, and its actions on one glass
/// capsule, Apple's 44 points tall.
#[component]
pub fn player_phone_top(app: erika::Client, #[session] draft: PlayerDraft) -> impl View {
    ui! {
        Row(gap: 10, padding_x: 8, height: 48, align: center) {
            IconButton(id: back_phone, icon: "chevron.left", font_size: 18, height: 44, label: tr! { "zh-Hans" => "返回", "en" => "Back" }) => app.minimize()
            Row(gap: 8, align: baseline) {
                Text(content: app.title, style: title3, lines: 1)
                Text(content: app.subtitle, style: caption, emphasis: secondary, lines: 1)
            }
            Spacer
            ControlGroup(label: tr! { "zh-Hans" => "播放选项", "en" => "Playback options" }, height: 44) {
                if app.features.danmaku {
                    IconButton(id: match_danmaku_phone, icon: "link", label: tr! { "zh-Hans" => "弹幕匹配", "en" => "Match danmaku" }) => app.show_notice(Notice::DanmakuMatchPending)
                }
                IconButton(id: settings_phone, icon: "slider.horizontal.3", label: tr! { "zh-Hans" => "播放设置", "en" => "Playback settings" }) => app.player_panel(!app.player.panel)
                Menu(id: more_phone, label: tr! { "zh-Hans" => "更多", "en" => "More" }, icon: "ellipsis", style: icon) {
                    MenuItem(id: skip_intro_phone, label: tr! { "zh-Hans" => "跳过片头（85 秒）", "en" => "Skip opening (85 seconds)" }) => app.skip(85.0)
                    MenuItem(id: episodes_phone, label: tr! { "zh-Hans" => "选集…", "en" => "Episodes…" }, enabled: app.player.episodes) => draft.episodes_open.set(true)
                    MenuItem(id: info_phone, label: tr! { "zh-Hans" => "播放信息", "en" => "Playback information" }) => app.player_tab(PlayerTab::Playback)
                }
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
                    if app.paused {
                        Label(title: tr! { "zh-Hans" => "暂停", "en" => "Pause" }, icon: "pause.fill", display: icon, font_size: 44)
                    } else {
                        Label(title: tr! { "zh-Hans" => "播放", "en" => "Play" }, icon: "play.fill", display: icon, font_size: 44)
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
                    if app.paused {
                        Label(title: tr! { "zh-Hans" => "暂停", "en" => "Pause" }, icon: "pause.fill", display: icon, font_size: 44)
                    } else {
                        Label(title: tr! { "zh-Hans" => "播放", "en" => "Play" }, icon: "play.fill", display: icon, font_size: 44)
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
            WindowButtons
            ControlGroup(height: 40) {
                // With no sidebar to shrink into, the picture goes back into
                // the empty frame (design 10.2).
                if !app.features.anime && !app.features.library && !app.features.downloads {
                    IconButton(id: back_to_frame, icon: "chevron.left", label: tr! { "zh-Hans" => "返回", "en" => "Back" }, shortcut: "escape") => app.return_to_frame()
                } else {
                    IconButton(id: back, icon: "chevron.left", label: tr! { "zh-Hans" => "返回", "en" => "Back" }, shortcut: "escape") => app.minimize()
                }
            }
            Column(gap: 2, padding_x: 4) {
                Text(content: app.title, style: title3, lines: 1)
                Text(content: app.subtitle, style: caption, emphasis: secondary)
            }
            Spacer
            ControlGroup(label: tr! { "zh-Hans" => "音量与弹幕", "en" => "Audio and danmaku" }, height: 40) {
                PopoverButton(id: volume, icon: "speaker.wave.2", label: tr! { "zh-Hans" => "音量", "en" => "Volume" }, presented: draft.volume_open) {
                    Column(gap: 12, width: 240, padding: 8) {
                        Slider(id: volume_level, label: tr! { "zh-Hans" => "音量", "en" => "Volume" }, current: app.player.volume, min: 0, max: 1) => app.set_player_number("volume", app.player.volume)
                    }
                }
                if app.features.danmaku {
                    IconButton(id: match_danmaku, icon: "link", label: tr! { "zh-Hans" => "弹幕匹配", "en" => "Match danmaku" }) => app.show_notice(Notice::DanmakuMatchPending)
                }
            }
            ControlGroup(label: tr! { "zh-Hans" => "画面模式", "en" => "Viewing mode" }, height: 40) {
                IconButton(id: fullscreen, icon: "arrow.up.left.and.arrow.down.right", label: tr! { "zh-Hans" => "全屏", "en" => "Full screen" }, shortcut: "f") => app.show_notice(Notice::FullScreenHint)
            }
            ControlGroup(label: tr! { "zh-Hans" => "播放选项", "en" => "Playback options" }, height: 40) {
                IconButton(id: settings, icon: "slider.horizontal.3", label: tr! { "zh-Hans" => "播放设置", "en" => "Playback settings" }) => app.player_panel(!app.player.panel)
                Menu(id: more, label: tr! { "zh-Hans" => "更多", "en" => "More" }, icon: "ellipsis", style: icon) {
                    MenuItem(id: screenshot, label: tr! { "zh-Hans" => "截图", "en" => "Screenshot" }, icon: "camera", shortcut: "cmd+s") => app.screenshot()
                    MenuItem(id: skip_intro, label: tr! { "zh-Hans" => "跳过片头（85 秒）", "en" => "Skip opening (85 seconds)" }) => app.skip(85.0)
                    MenuItem(id: episodes, label: tr! { "zh-Hans" => "选集…", "en" => "Episodes…" }, enabled: app.player.episodes) => draft.episodes_open.set(true)
                    MenuItem(id: info, label: tr! { "zh-Hans" => "播放信息", "en" => "Playback information" }) => app.player_tab(PlayerTab::Playback)
                    MenuItem(id: manual_match, label: tr! { "zh-Hans" => "手动匹配弹幕…", "en" => "Match danmaku…" }, enabled: app.features.danmaku) => app.show_notice(Notice::DanmakuMatchPending)
                }
            }
            if draft.episodes_open {
                PopoverButton(id: episode_list, label: tr! { "zh-Hans" => "选集", "en" => "Episodes" }, presented: draft.episodes_open) {
                    player_episodes(app: app)
                }
            }
        }
    }
}

#[component]
pub fn player_timeline(app: erika::Client) -> impl View {
    ui! {
        Stack(align: top_leading) {
            Positioned(x: if app.player.panel { 54 } else { 0 }, y: 0, width: if app.player.panel { 788 } else { 1074 }, height: 44) {
                Slider(id: position, label: tr! { "zh-Hans" => "播放进度", "en" => "Playback position" }, label_hidden: true, current: app.progress, min: 0, max: 1) => app.seek(app.progress)
            }
            if app.player.episodes {
                Positioned(x: if app.player.panel { 54 } else { 0 }, y: 20, width: if app.player.panel { 788 } else { 1074 }, height: 4) {
                    Rail {
                        Canvas(at: 0.06323, width: 2, height: 4) { Shape(kind: rect, fill: text.secondary, opacity: 0.7) }
                        Canvas(at: 0.12509, width: 2, height: 4) { Shape(kind: rect, fill: text.secondary, opacity: 0.7) }
                        Canvas(at: 0.4811, width: 2, height: 4) { Shape(kind: rect, fill: text.secondary, opacity: 0.7) }
                        Canvas(at: 0.89003, width: 2, height: 4) { Shape(kind: rect, fill: text.secondary, opacity: 0.7) }
                        Canvas(at: 0.95189, width: 2, height: 4) { Shape(kind: rect, fill: text.secondary, opacity: 0.7) }
                    }
                }
                Positioned(x: if app.player.panel { 54 } else { 0 }, y: 44, width: if app.player.panel { 788 } else { 1074 }, height: 16) {
                    Rail(anchor: start) {
                        Text(at: 0, content: tr! { "zh-Hans" => "序幕", "en" => "Prologue" }, style: caption, emphasis: secondary)
                        Text(at: 0.06323, content: tr! { "zh-Hans" => "OP", "en" => "OP" }, style: caption, emphasis: secondary)
                        Text(at: 0.12509, content: tr! { "zh-Hans" => "A 部分", "en" => "Part A" }, style: caption, emphasis: secondary)
                        Text(at: 0.4811, content: tr! { "zh-Hans" => "B 部分", "en" => "Part B" }, style: caption, emphasis: secondary)
                        Text(at: 0.89003, content: tr! { "zh-Hans" => "ED", "en" => "ED" }, style: caption, emphasis: secondary)
                        Text(at: 0.95189, content: tr! { "zh-Hans" => "预告", "en" => "Preview" }, style: caption, emphasis: secondary)
                        if app.features.danmaku {
                            Text(at: 0.23505, content: tr! { "zh-Hans" => "★", "en" => "★" }, style: caption)
                            Text(at: 0.75464, content: tr! { "zh-Hans" => "★", "en" => "★" }, style: caption)
                        }
                    }
                }
            }
            if app.player.panel {
                Positioned(x: 0, y: 14, width: 40, height: 16) { Text(content: tr! { "zh-Hans" => "{time, duration, mm:ss}", "en" => "{time, duration, mm:ss}"; time: app.elapsed }, style: caption2) }
                Positioned(x: 856, y: 14, width: 40, height: 16) { Text(content: tr! { "zh-Hans" => "{time, duration, mm:ss}", "en" => "{time, duration, mm:ss}"; time: app.duration }, style: caption2, emphasis: secondary) }
            }
            if !app.player.panel {
                Positioned(x: -68, y: 14, width: 54, height: 16) { Row { Spacer Text(content: tr! { "zh-Hans" => "{time, duration, mm:ss}", "en" => "{time, duration, mm:ss}"; time: app.elapsed }, style: caption2) } }
                Positioned(x: 1088, y: 14, width: 60, height: 16) { Text(content: tr! { "zh-Hans" => "{time, duration, mm:ss}", "en" => "{time, duration, mm:ss}"; time: app.duration }, style: caption2, emphasis: secondary) }
            }
        }
    }
}

#[component]
pub fn player_controls(app: erika::Client, #[session] draft: PlayerDraft) -> impl View {
    ui! {
        Stack {
            Row(gap: 8, align: center, height: 48) {
                if app.features.danmaku {
                    SplitButton(id: danmaku, label: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, more_label: tr! { "zh-Hans" => "弹幕参数", "en" => "Danmaku options" }, icon: "text.bubble", checked: app.preferences.show_danmaku, shape: capsule, height: 34, shortcut: "d") {
                        player_danmaku_quick(app: app)
                    } => app.set_preference("show_danmaku", app.preferences.show_danmaku)
                    if app.features.sync {
                        Row(gap: 8, width: 230) {
                            TextInput(id: danmaku_text, label: tr! { "zh-Hans" => "发弹幕", "en" => "Send danmaku" }, label_hidden: true, prompt: tr! { "zh-Hans" => "发个友善的弹幕…", "en" => "Say something kind…" }, value: draft.text, submit: clear) => app.send_danmaku(draft.text)
                            Button(id: send, label: tr! { "zh-Hans" => "发送", "en" => "Send" }, prominence: plain, clears: draft.text) => app.send_danmaku(draft.text)
                        }
                    }
                }
                Spacer
                Row(variant: subtitle_box) {
                    player_subtitle_menu(app: app, height: 32)
                }
                Row(variant: rate_box) { Picker(id: rate, label: tr! { "zh-Hans" => "倍速", "en" => "Speed" }, selected: app.preferences.speed, label_hidden: true, style: menu, shape: capsule, height: 32) => app.set_speed(app.preferences.speed) }
                if app.features.enhance {
                    Row(variant: enhancement_box) { Picker(id: enhancement, label: tr! { "zh-Hans" => "画质增强", "en" => "Enhancement" }, selected: app.player.enhancement, label_hidden: true, style: menu, shape: capsule, height: 32) => app.set_enhancement(app.player.enhancement) }
                }
            }
            Row(gap: 14, align: center, width: 248, height: 48) {
                if app.player.episodes {
                    IconButton(id: previous, icon: "backward.end.fill", label: tr! { "zh-Hans" => "上一集", "en" => "Previous episode" }, prominence: plain, enabled: app.player.episode > 1) => app.change_episode(-1)
                }
                IconButton(id: rewind, icon: "gobackward.10", variant: transport, label: tr! { "zh-Hans" => "快退 10 秒", "en" => "Back 10 seconds" }, prominence: plain, shortcut: "left") => app.skip(-10.0)
                if app.paused {
                    IconButton(id: play, icon: "play.fill", label: tr! { "zh-Hans" => "播放", "en" => "Play" }, variant: player_primary, prominence: prominent, shortcut: "space") => app.toggle_pause()
                } else {
                    IconButton(id: pause, icon: "pause.fill", label: tr! { "zh-Hans" => "暂停", "en" => "Pause" }, variant: player_primary, prominence: prominent, shortcut: "space") => app.toggle_pause()
                }
                IconButton(id: forward, icon: "goforward.10", variant: transport, label: tr! { "zh-Hans" => "快进 10 秒", "en" => "Forward 10 seconds" }, prominence: plain, shortcut: "right") => app.skip(10.0)
                if app.player.episodes {
                    IconButton(id: next, icon: "forward.end.fill", label: tr! { "zh-Hans" => "下一集", "en" => "Next episode" }, prominence: plain, enabled: app.player.episode < 12) => app.change_episode(1)
                }
            }
        }
    }
}

#[component]
pub fn player_danmaku_quick(app: erika::Client) -> impl View {
    ui! {
        Column(gap: 14, padding: 10, width: 320) {
            Slider(id: opacity, label: tr! { "zh-Hans" => "不透明度", "en" => "Opacity" }, current: app.danmaku_settings.opacity, min: 0, max: 1) => app.set_danmaku_number("opacity", app.danmaku_settings.opacity)
            Slider(id: font_size, label: tr! { "zh-Hans" => "字号", "en" => "Font size" }, current: app.danmaku_settings.font_size, min: 0, max: 1) => app.set_danmaku_number("font_size", app.danmaku_settings.font_size)
            Picker(id: area, label: tr! { "zh-Hans" => "显示区域", "en" => "Display area" }, selected: app.preferences.danmaku_area, style: segmented) => app.set_danmaku_area(app.preferences.danmaku_area)
            Text(content: tr! { "zh-Hans" => "屏蔽", "en" => "Block" }, style: caption, emphasis: secondary)
            Row(gap: 8) {
                Switch(id: top, label: tr! { "zh-Hans" => "顶部", "en" => "Top" }, checked: app.danmaku_settings.block_top, style: button, shape: capsule, height: 30) => app.set_danmaku_switch("block_top", app.danmaku_settings.block_top)
                Switch(id: bottom, label: tr! { "zh-Hans" => "底部", "en" => "Bottom" }, checked: app.danmaku_settings.block_bottom, style: button, shape: capsule, height: 30) => app.set_danmaku_switch("block_bottom", app.danmaku_settings.block_bottom)
                Switch(id: scroll, label: tr! { "zh-Hans" => "滚动", "en" => "Scrolling" }, checked: app.danmaku_settings.block_scroll, style: button, shape: capsule, height: 30) => app.set_danmaku_switch("block_scroll", app.danmaku_settings.block_scroll)
            }
            Button(id: more, label: tr! { "zh-Hans" => "更多弹幕设置…", "en" => "More danmaku settings…" }, prominence: plain) => app.player_tab(PlayerTab::Danmaku)
        }
    }
}

#[component]
pub fn player_panel(app: erika::Client, #[session] draft: PlayerDraft) -> impl View {
    ui! {
        Stack(variant: player_panel) {
            ScrollView {
                Column(gap: 0, padding_x: 16, padding_y: 18) {
                    // `.p-sheet .hd`: as tall as its 34-point close button.
                    Row(height: 34, align: center) {
                        Text(content: tr! { "zh-Hans" => "播放设置", "en" => "Playback settings" }, style: title3)
                        Spacer
                        IconButton(id: close, icon: "xmark", label: tr! { "zh-Hans" => "关闭播放设置", "en" => "Close playback settings" }) => app.player_panel(false)
                    }
                    Canvas(height: 12)
                    // `.c-seg`: 34 points tall, whatever the platform's control.
                    Row(height: 34, align: center) {
                    // The picture page holds the enhance module's upscaling
                    // alone, and the danmaku page the danmaku module's.
                    if app.features.danmaku && app.features.enhance {
                        Picker(id: tab, label: tr! { "zh-Hans" => "播放设置分页", "en" => "Playback settings tab" }, label_hidden: true, selected: app.player.tab, style: segmented) {
                            PickerOption(value: PlayerTab::Danmaku, label: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" })
                            PickerOption(value: PlayerTab::Subtitles, label: tr! { "zh-Hans" => "字幕", "en" => "Subtitles" })
                            PickerOption(value: PlayerTab::Picture, label: tr! { "zh-Hans" => "画面", "en" => "Picture" })
                            PickerOption(value: PlayerTab::Audio, label: tr! { "zh-Hans" => "音频", "en" => "Audio" })
                            PickerOption(value: PlayerTab::Playback, label: tr! { "zh-Hans" => "播放", "en" => "Playback" })
                        } => app.player_tab(app.player.tab)
                    } else if app.features.danmaku {
                        Picker(id: tab_without_picture, label: tr! { "zh-Hans" => "播放设置分页", "en" => "Playback settings tab" }, label_hidden: true, selected: app.player.tab, style: segmented) {
                            PickerOption(value: PlayerTab::Danmaku, label: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" })
                            PickerOption(value: PlayerTab::Subtitles, label: tr! { "zh-Hans" => "字幕", "en" => "Subtitles" })
                            PickerOption(value: PlayerTab::Audio, label: tr! { "zh-Hans" => "音频", "en" => "Audio" })
                            PickerOption(value: PlayerTab::Playback, label: tr! { "zh-Hans" => "播放", "en" => "Playback" })
                        } => app.player_tab(app.player.tab)
                    } else if app.features.enhance {
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
                    match app.player.tab {
                        PlayerTab::Danmaku => { Column(gap: 0, appear: settings_page_in) {
                                Row(height: 34, padding_x: 4) { Text(content: tr! { "zh-Hans" => "显示", "en" => "Display" }, style: caption, emphasis: secondary) Spacer }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "显示弹幕", "en" => "Show danmaku" }) Spacer Switch(id: show_danmaku, label: tr! { "zh-Hans" => "显示弹幕", "en" => "Show danmaku" }, label_hidden: true, style: switch, checked: app.preferences.show_danmaku) => app.set_preference("show_danmaku", app.preferences.show_danmaku) }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "速度", "en" => "Speed" }) Spacer Row(width: 130) { Slider(id: speed, label: tr! { "zh-Hans" => "速度", "en" => "Speed" }, label_hidden: true, current: app.danmaku_settings.speed, min: 0, max: 1) => app.set_danmaku_number("speed", app.danmaku_settings.speed) } }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "合并重复弹幕", "en" => "Merge duplicates" }) Spacer Switch(id: merge, label: tr! { "zh-Hans" => "合并重复弹幕", "en" => "Merge duplicates" }, label_hidden: true, style: switch, checked: app.danmaku_settings.merge) => app.set_danmaku_switch("merge", app.danmaku_settings.merge) }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "弹幕偏移", "en" => "Danmaku delay" }) Spacer Row(width: 130) { Slider(id: danmaku_delay, label: tr! { "zh-Hans" => "弹幕偏移", "en" => "Danmaku delay" }, label_hidden: true, current: app.danmaku_settings.delay, min: -10, max: 10) => app.set_danmaku_number("delay", app.danmaku_settings.delay) } }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(height: 34, padding_x: 4) { Text(content: tr! { "zh-Hans" => "来源", "en" => "Sources" }, style: caption, emphasis: secondary) Spacer }
                                // The files liberika shows, by name, with the comments they hold.
                                if app.player.danmaku_count > 0 {
                                    Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: app.player.danmaku_source, lines: 1) Spacer Text(content: tr! { "zh-Hans" => "{count} 条", "en" => "{count}"; count: app.player.danmaku_count }, style: body, emphasis: secondary) }
                                } else {
                                    Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "没有弹幕", "en" => "No danmaku" }, emphasis: secondary) Spacer }
                                }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                // A television has no files to mount.
                                if !window.tv { Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "挂载本地弹幕", "en" => "Local danmaku" }) Spacer Button(id: local_danmaku, label: tr! { "zh-Hans" => "xml / json", "en" => "xml / json" }, prominence: plain, pick: draft.danmaku_file, files: ".xml .json") => app.mount_danmaku(draft.danmaku_file) } }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                            } }
                        PlayerTab::Subtitles => { Column(gap: 0, appear: settings_page_in) {
                                Row(height: 34, padding_x: 4) { Text(content: tr! { "zh-Hans" => "样式", "en" => "Style" }, style: caption, emphasis: secondary) Spacer }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "字号", "en" => "Font size" }) Spacer Row(width: 130) { Slider(id: subtitle_size, label: tr! { "zh-Hans" => "字号", "en" => "Font size" }, label_hidden: true, current: app.player.subtitle_size, min: 0, max: 1) => app.set_player_number("subtitle_size", app.player.subtitle_size) } }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(height: 34, padding_x: 4) { Text(content: tr! { "zh-Hans" => "轨道", "en" => "Tracks" }, style: caption, emphasis: secondary) Spacer }
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
                                Row(height: 34, padding_x: 4) { Text(content: tr! { "zh-Hans" => "画质增强", "en" => "Enhancement" }, style: caption, emphasis: secondary) Spacer }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "超分辨率", "en" => "Upscaling" }) Spacer Picker(id: enhancement, label: tr! { "zh-Hans" => "超分辨率", "en" => "Upscaling" }, label_hidden: true, style: menu, selected: app.player.enhancement) => app.set_enhancement(app.player.enhancement) }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                            } }
                        PlayerTab::Audio => { Column(gap: 0, appear: settings_page_in) {
                                Row(height: 34, padding_x: 4) { Text(content: tr! { "zh-Hans" => "音频", "en" => "Audio" }, style: caption, emphasis: secondary) Spacer }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "音量", "en" => "Volume" }) Spacer Row(width: 130) { Slider(id: volume, label: tr! { "zh-Hans" => "音量", "en" => "Volume" }, label_hidden: true, current: app.player.volume, min: 0, max: 1) => app.set_player_number("volume", app.player.volume) } }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(height: 34, padding_x: 4) { Text(content: tr! { "zh-Hans" => "音轨", "en" => "Audio tracks" }, style: caption, emphasis: secondary) Spacer }
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
                                Row(height: 34, padding_x: 4) { Text(content: tr! { "zh-Hans" => "播放", "en" => "Playback" }, style: caption, emphasis: secondary) Spacer }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "跳过片头", "en" => "Skip opening" }) Spacer Switch(id: skip_intro, label: tr! { "zh-Hans" => "跳过片头", "en" => "Skip opening" }, label_hidden: true, style: switch, checked: app.player.skip_intro) => app.set_skip_intro(app.player.skip_intro) }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "自动连播下一集", "en" => "Play next automatically" }) Spacer Switch(id: autoplay, label: tr! { "zh-Hans" => "自动连播下一集", "en" => "Play next automatically" }, label_hidden: true, style: switch, checked: app.preferences.autoplay) => app.set_preference("autoplay", app.preferences.autoplay) }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "控制层出现时画面退后", "en" => "Step back for controls" }) Spacer Switch(id: step_back, label: tr! { "zh-Hans" => "控制层出现时画面退后", "en" => "Step back for controls" }, label_hidden: true, style: switch, checked: app.preferences.step_back) => app.set_preference("step_back", app.preferences.step_back) }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(height: 34, padding_x: 4) { Text(content: tr! { "zh-Hans" => "信息", "en" => "Information" }, style: caption, emphasis: secondary) Spacer }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "播放内核", "en" => "Playback engine" }) Spacer Text(content: tr! { "zh-Hans" => "liberika", "en" => "liberika" }, style: body, emphasis: secondary) }
                                Canvas(height: 0.5) { Shape(kind: rect, fill: control.border, opacity: 0.55) }
                                Row(gap: 12, height: 46, padding_x: 4, align: center) { Text(content: tr! { "zh-Hans" => "视频", "en" => "Video" }) Spacer Text(content: app.player.video_info, style: body, emphasis: secondary) }
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
            Grid(min_width: 52, gap: 8) {
                Switch(id: ep_1, label: tr! { "zh-Hans" => "1", "en" => "1" }, checked: app.player.episode == 1, style: button) => app.select_episode(1)
                Switch(id: ep_2, label: tr! { "zh-Hans" => "2", "en" => "2" }, checked: app.player.episode == 2, style: button) => app.select_episode(2)
                Switch(id: ep_3, label: tr! { "zh-Hans" => "3", "en" => "3" }, checked: app.player.episode == 3, style: button) => app.select_episode(3)
                Switch(id: ep_4, label: tr! { "zh-Hans" => "4", "en" => "4" }, checked: app.player.episode == 4, style: button) => app.select_episode(4)
                Switch(id: ep_5, label: tr! { "zh-Hans" => "5", "en" => "5" }, checked: app.player.episode == 5, style: button) => app.select_episode(5)
                Switch(id: ep_6, label: tr! { "zh-Hans" => "6", "en" => "6" }, checked: app.player.episode == 6, style: button) => app.select_episode(6)
                Switch(id: ep_7, label: tr! { "zh-Hans" => "7", "en" => "7" }, checked: app.player.episode == 7, style: button) => app.select_episode(7)
                Switch(id: ep_8, label: tr! { "zh-Hans" => "8", "en" => "8" }, checked: app.player.episode == 8, style: button) => app.select_episode(8)
                Switch(id: ep_9, label: tr! { "zh-Hans" => "9", "en" => "9" }, checked: app.player.episode == 9, style: button) => app.select_episode(9)
                Switch(id: ep_10, label: tr! { "zh-Hans" => "10", "en" => "10" }, checked: app.player.episode == 10, style: button) => app.select_episode(10)
                Switch(id: ep_11, label: tr! { "zh-Hans" => "11", "en" => "11" }, checked: app.player.episode == 11, style: button) => app.select_episode(11)
                Switch(id: ep_12, label: tr! { "zh-Hans" => "12", "en" => "12" }, checked: app.player.episode == 12, style: button) => app.select_episode(12)
            }
        }
    }
}

/// The subtitles' capsule (`.p-ctl` 字幕): the track shown, or 「关闭」.
/// Its menu turns them off, adds a file, and opens the settings' track
/// lists, which list what the media has.
#[component]
pub fn player_subtitle_menu(app: erika::Client, height: f64, #[session] draft: PlayerDraft) -> impl View {
    ui! {
        if app.player.subtitle_track >= 0 {
            Menu(id: subtitles, label: app.player.subtitle_name, icon: "captions.bubble", shape: capsule, height: height) {
                MenuItem(id: subtitle_tracks, label: tr! { "zh-Hans" => "字幕轨道…", "en" => "Subtitle tracks…" }) => app.player_tab(PlayerTab::Subtitles)
                MenuItem(id: no_subtitle, label: tr! { "zh-Hans" => "关闭字幕", "en" => "Subtitles off" }) => app.select_subtitle_track(-1)
                MenuItem(id: external_subtitle, label: tr! { "zh-Hans" => "添加外挂字幕…", "en" => "Add external subtitles…" }, pick: draft.subtitle_file, files: ".ass .ssa .srt .vtt") => app.attach_subtitle(draft.subtitle_file)
                Divider
                MenuItem(id: audio_tracks, label: tr! { "zh-Hans" => "音轨…", "en" => "Audio tracks…" }) => app.player_tab(PlayerTab::Audio)
            }
        } else {
            Menu(id: subtitles_off, label: tr! { "zh-Hans" => "关闭", "en" => "Off" }, icon: "captions.bubble", shape: capsule, height: height) {
                MenuItem(id: subtitle_tracks_off, label: tr! { "zh-Hans" => "字幕轨道…", "en" => "Subtitle tracks…" }) => app.player_tab(PlayerTab::Subtitles)
                MenuItem(id: external_subtitle_off, label: tr! { "zh-Hans" => "添加外挂字幕…", "en" => "Add external subtitles…" }, pick: draft.subtitle_file, files: ".ass .ssa .srt .vtt") => app.attach_subtitle(draft.subtitle_file)
                Divider
                MenuItem(id: audio_tracks_off, label: tr! { "zh-Hans" => "音轨…", "en" => "Audio tracks…" }) => app.player_tab(PlayerTab::Audio)
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
            Menu(id: subtitles_tv, label: app.player.subtitle_name, icon: "captions.bubble", shape: capsule, height: 46) {
                MenuItem(id: subtitle_tracks_tv, label: tr! { "zh-Hans" => "字幕轨道…", "en" => "Subtitle tracks…" }) => app.player_tab(PlayerTab::Subtitles)
                MenuItem(id: no_subtitle_tv, label: tr! { "zh-Hans" => "关闭字幕", "en" => "Subtitles off" }) => app.select_subtitle_track(-1)
                Divider
                MenuItem(id: audio_tracks_tv, label: tr! { "zh-Hans" => "音轨…", "en" => "Audio tracks…" }) => app.player_tab(PlayerTab::Audio)
            }
        } else {
            Menu(id: subtitles_off_tv, label: tr! { "zh-Hans" => "关闭", "en" => "Off" }, icon: "captions.bubble", shape: capsule, height: 46) {
                MenuItem(id: subtitle_tracks_off_tv, label: tr! { "zh-Hans" => "字幕轨道…", "en" => "Subtitle tracks…" }) => app.player_tab(PlayerTab::Subtitles)
                Divider
                MenuItem(id: audio_tracks_off_tv, label: tr! { "zh-Hans" => "音轨…", "en" => "Audio tracks…" }) => app.player_tab(PlayerTab::Audio)
            }
        }
    }
}
