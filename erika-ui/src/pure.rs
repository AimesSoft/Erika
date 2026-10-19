//! Pure playback (design 10.2, next-screens.js `renderIdle`, next.css
//! `.i-frame` and `.i-recent`): with no module adding pages the window has
//! no sidebar and is the frame before a picture is in it. The frame stands
//! exactly where the player's frame steps back to, (103, 64) 1074 × 604 in
//! the 1280 × 800 window, so a video fills it in place; what played last
//! lists under it, where the player's controls are.
use crate::phone::{
    phone_mini_player, phone_mini_player_props, phone_settings, phone_settings_props,
    phone_settings_section, phone_settings_section_props, PhoneSettings,
};
use crate::{AppWindows, ErikaDraft};
use bern::prelude::*;
use erika_protocol::erika;

#[bern::session]
pub struct PureDraft {
    /// The video picked or dropped last, and the folder.
    pub file: MediaSource,
    pub folder: MediaSource,
    /// Whether a video is held above the frame.
    pub over: bool,
    /// The link being typed.
    pub link: String,
}

/// The desktop window with no pages: the settings at the trailing end of
/// the bar, the empty frame and the recent videos in the window's scene,
/// which scales as the player's does.
#[component]
pub fn pure_window(app: erika::Client, windows: AppWindows) -> impl View {
    ui! {
        // The scene is the whole window, as the player's is, so the frame
        // meets the player's frame; the bar stands over it.
        Stack(align: top_leading, under_bars: true) {
            // `renderTb` with no pages: the settings alone (`.c-ib`).
            Toolbar {
                Spacer
                IconButton(id: pure_settings, icon: "gearshape", label: tr! { "zh-Hans" => "设置", "en" => "Settings" }, shortcut: "cmd+,") => windows.settings.open()
            }
            ScaleBox(width: 1280, height: 800) {
                Stack(width: 1280, height: 800, align: top_leading) {
                    Positioned(x: 103, y: 64, width: 1074, height: 604.125) {
                        pure_frame(app: app)
                    }
                    // 22 points under the frame (`.i-recent`).
                    Positioned(x: 103, y: 690.125, width: 1074, height: 61.5) {
                        pure_recents(app: app)
                    }
                }
            }
            if app.link_prompt {
                pure_link(app: app)
            }
        }
    }
}

/// `.i-frame`: the neutral fill inside a 2-point outline and a dashed one
/// 10 points in; a video held above turns both to the accent and the frame
/// wobbles. The film, the words and the three ways to open stand in the
/// middle.
#[component]
pub fn pure_frame(app: erika::Client, #[session] draft: PureDraft) -> impl View {
    ui! {
        Drop(id: pure_drop, label: tr! { "zh-Hans" => "把视频拖到这里", "en" => "Drop a video here" }, value: draft.file, files: "video", over: draft.over) {
            Stack(width: 1074, height: 604.125, nudge: draft.over, appear: pure_frame_in) {
                if draft.over {
                    Canvas(width: 1074, height: 604.125) {
                        Shape(kind: rounded, corner: 22, fill: accent, opacity: 0.08)
                        Shape(kind: rounded, x: 1, y: 1, width: 1072, height: 602.125, corner: 21, stroke: accent, stroke_width: 2)
                        Shape(kind: rounded, x: 10.75, y: 10.75, width: 1052.5, height: 582.625, corner: 15.25, stroke: accent, stroke_width: 1.5, dash: 4.5, dash_gap: 3)
                    }
                } else {
                    Canvas(width: 1074, height: 604.125) {
                        Shape(kind: rounded, corner: 22, fill: fill.subtle)
                        Shape(kind: rounded, x: 1, y: 1, width: 1072, height: 602.125, corner: 21, stroke: text.placeholder, stroke_width: 2)
                        Shape(kind: rounded, x: 10.75, y: 10.75, width: 1052.5, height: 582.625, corner: 15.25, stroke: text.placeholder, stroke_width: 1.5, dash: 4.5, dash_gap: 3)
                    }
                }
                // 56 + 14 + 34 + 8 + 18 + 26 + 42 points, in the middle.
                Column(gap: 0, align: center) {
                    Stack(variant: pure_symbol, width: 56, height: 56, align: center) {
                        Label(title: tr! { "zh-Hans" => "视频", "en" => "Video" }, icon: "play.rectangle", display: icon, font_size: 48)
                    }
                    Canvas(height: 14)
                    Text(content: tr! { "zh-Hans" => "把视频拖到这里", "en" => "Drop a video here" }, style: title2, line_height: 34)
                    Canvas(height: 8)
                    Text(content: tr! { "zh-Hans" => "支持文件、文件夹、链接和字幕文件，也可以按 {key}", "en" => "Files, folders, links and subtitle files work, or press {key}"; key: app.open_key }, style: body, emphasis: secondary, line_height: 18)
                    Canvas(height: 26)
                    Row(gap: 10, align: center) {
                        Button(id: pure_open_file, label: tr! { "zh-Hans" => "打开文件", "en" => "Open File" }, icon: "folder", shape: capsule, height: 42, prominence: prominent, variant: primary, pick: draft.file, files: "video", shortcut: "cmd+o") => app.open_file(draft.file)
                        Button(id: pure_open_folder, label: tr! { "zh-Hans" => "打开文件夹", "en" => "Open Folder" }, shape: capsule, height: 32, pick: draft.folder, files: "folder") => app.open_folder(draft.folder)
                        Button(id: pure_open_link, label: tr! { "zh-Hans" => "打开链接…", "en" => "Open Link…" }, shape: capsule, height: 32) => app.show_link_prompt(true)
                    }
                }
            }
        } => app.open_file(draft.file)
    }
}

/// `.i-recent`: the recent videos 14 points apart, then 「继续上次」 at the
/// trailing end, which space also presses. They rise in one after another
/// (`stagger(…, 160, 10, 50)`).
#[component]
pub fn pure_recents(app: erika::Client) -> impl View {
    ui! {
        Row(gap: 14, align: center, stagger: 0.05) {
            ForEach(items: app.recents, key: name) |recent| {
                // `.i-rc`: 6 inside, the picture 88 wide, the words 10 after.
                Button(id: pure_recent, label: recent.name, variant: recent_row, width: fill, appear: recent_in, appear_delay: 0.16) {
                    Row(gap: 10, align: center) {
                        Stack(align: bottom_leading, variant: thumbnail) {
                            Image(source: recent.still, width: 88, height: 49.5, fit: cover, corner: 8, label: recent.name)
                            // `.x-prog`: the bar 3 points high, black at 35 %
                            // (the light appearance's label) under the accent.
                            Rail {
                                Canvas(from: 0, to: 1, height: 3, appearance: light) {
                                    Shape(kind: rect, fill: text.primary, opacity: 0.41)
                                }
                            }
                            Rail {
                                Canvas(from: 0, to: recent.progress, height: 3) {
                                    Shape(kind: rect, fill: accent)
                                }
                            }
                        }
                        Column(gap: 0) {
                            Text(content: recent.name, style: headline, font_size: 13, lines: 1, line_height: 19)
                            Text(content: tr! { "zh-Hans" => "剩 {left, duration, m:ss}", "en" => "{left, duration, m:ss} left"; left: recent.remaining }, style: caption, emphasis: secondary, line_height: 18)
                        }
                        Spacer
                    }
                } => app.play_recent(recent.name)
            }
            // `.i-key`: the button with its key (`kbd`) after the words.
            Button(id: continue_last, label: tr! { "zh-Hans" => "继续上次", "en" => "Continue" }, shape: capsule, height: 32, shortcut: "space", appear: recent_in, appear_delay: 0.16) {
                Row(gap: 6, align: center) {
                    Label(title: tr! { "zh-Hans" => "继续上次", "en" => "Continue" }, icon: "play.fill", display: icon, font_size: 11)
                    Text(content: tr! { "zh-Hans" => "继续上次", "en" => "Continue" }, font_size: 13, lines: 1)
                    Stack(variant: key_chip, height: 17, align: center) {
                        Row(gap: 0, padding_x: 6, align: center) {
                            Text(content: tr! { "zh-Hans" => "空格", "en" => "Space" }, style: caption2, font_size: 11, lines: 1)
                        }
                    }
                }
            } => app.continue_last()
        }
    }
}

/// 「打开链接…」: a link to play, an http(s) or magnet address.
#[component]
pub fn pure_link(app: erika::Client, #[session] draft: PureDraft) -> impl View {
    ui! {
        Modal(width: 420, height: 176, cover: compact, appear: pop_in, disappear: fade) {
            Column(gap: 12, padding: 20) {
                Text(content: tr! { "zh-Hans" => "打开链接", "en" => "Open Link" }, style: title3)
                TextInput(id: pure_link_field, label: tr! { "zh-Hans" => "链接", "en" => "Link" }, label_hidden: true, value: draft.link, prompt: tr! { "zh-Hans" => "http(s):// 或 magnet: 链接", "en" => "An http(s):// or magnet: link" }) => app.open_link(draft.link)
                if app.link_invalid {
                    Stack(variant: link_error, align: leading) {
                        Text(content: tr! { "zh-Hans" => "链接要以 http://、https:// 或 magnet: 开头", "en" => "A link starts with http://, https:// or magnet:" }, style: caption, lines: 2)
                    }
                }
                Spacer
                Row(gap: 10, align: center) {
                    Spacer
                    Button(id: pure_link_cancel, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }, shortcut: "escape") => app.show_link_prompt(false)
                    Button(id: pure_link_open, label: tr! { "zh-Hans" => "打开", "en" => "Open" }, prominence: prominent, shortcut: "return") => app.open_link(draft.link)
                }
            }
        }
    }
}

/// The phone with no pages (`phHTML('idle')`, 390 × 844): no tab bar, the
/// settings at the top right, the frame 16:9 across the width, then a
/// column of ways to open and the recent videos in a grouped list. What
/// plays stays in a mini player 24 points above the bottom.
#[component]
pub fn phone_pure(app: erika::Client, #[session] draft: ErikaDraft) -> impl View {
    ui! {
        Stack(variant: phone_page, align: bottom) {
            if draft.phone_settings == PhoneSettings::Closed {
                phone_pure_page(app: app)
            } else if draft.phone_settings == PhoneSettings::List {
                phone_settings(app: app)
            } else {
                phone_settings_section(app: app)
            }
            // `.x-pmini` with no tab bar: 12 from the sides, 24 above the
            // screen's bottom edge, on the system's glass.
            if app.playing && !app.expanded {
                Stack(align: bottom, under_bars: true, appear: fade, disappear: fade) {
                    Column(gap: 0, padding_x: 12) {
                        ControlGroup(height: 48) {
                            phone_mini_player(app: app)
                        }
                        Canvas(height: 24)
                    }
                }
            }
            if app.link_prompt {
                pure_link(app: app)
            }
        }
    }
}

/// `phHTML('idle')`: the header row 54 points down, the frame 70 under it,
/// the ways to open 18, 10 and 10 apart, the note, and 「最近播放」.
#[component]
pub fn phone_pure_page(app: erika::Client, #[session] draft: ErikaDraft, #[session] pure: PureDraft) -> impl View {
    ui! {
        Form(appear: page_in, gap: 2, row_height: 44) {
            Section(style: plain) {
                Column(gap: 0) {
                    Canvas(height: 7)
                    // Apple's size for buttons in the bar: 44-point circles.
                    Row(gap: 0, height: 48, align: center) {
                        Spacer
                        IconButton(id: phone_pure_settings, icon: "gearshape", font_size: 18, height: 44, label: tr! { "zh-Hans" => "设置", "en" => "Settings" }) => draft.phone_settings.set(PhoneSettings::List)
                    }
                    Canvas(height: 70)
                    Drop(id: phone_pure_drop, label: tr! { "zh-Hans" => "打开一个视频", "en" => "Open a video" }, value: pure.file, files: "video", over: pure.over) {
                        Stack(width: 358, height: 201.375, nudge: pure.over, appear: pure_frame_in) {
                            if pure.over {
                                Canvas(width: 358, height: 201.375) {
                                    Shape(kind: rounded, corner: 22, fill: accent, opacity: 0.08)
                                    Shape(kind: rounded, x: 1, y: 1, width: 356, height: 199.375, corner: 21, stroke: accent, stroke_width: 2)
                                    Shape(kind: rounded, x: 10.75, y: 10.75, width: 336.5, height: 179.875, corner: 15.25, stroke: accent, stroke_width: 1.5, dash: 4.5, dash_gap: 3)
                                }
                            } else {
                                Canvas(width: 358, height: 201.375) {
                                    Shape(kind: rounded, corner: 22, fill: fill.subtle)
                                    Shape(kind: rounded, x: 1, y: 1, width: 356, height: 199.375, corner: 21, stroke: text.placeholder, stroke_width: 2)
                                    Shape(kind: rounded, x: 10.75, y: 10.75, width: 336.5, height: 179.875, corner: 15.25, stroke: text.placeholder, stroke_width: 1.5, dash: 4.5, dash_gap: 3)
                                }
                            }
                            // 40 + 14 + 24 + 8 + 15 points, in the middle.
                            Column(gap: 0, align: center) {
                                Stack(variant: pure_symbol, width: 40, height: 40, align: center) {
                                    Label(title: tr! { "zh-Hans" => "视频", "en" => "Video" }, icon: "play.rectangle", display: icon, font_size: 34)
                                }
                                Canvas(height: 14)
                                Text(content: tr! { "zh-Hans" => "打开一个视频", "en" => "Open a video" }, style: title3, line_height: 24)
                                Canvas(height: 8)
                                Text(content: tr! { "zh-Hans" => "从「文件」App、相册或链接打开", "en" => "From the Files app, Photos or a link" }, style: caption, emphasis: secondary, line_height: 15)
                            }
                        }
                    } => app.open_file(pure.file)
                    Canvas(height: 18)
                    Button(id: phone_pure_open_file, label: tr! { "zh-Hans" => "打开文件", "en" => "Open File" }, icon: "folder", width: fill, shape: capsule, height: 42, prominence: prominent, variant: phone_main, pick: pure.file, files: "video") => app.open_file(pure.file)
                    Canvas(height: 10)
                    Row(gap: 10, align: center) {
                        Button(id: phone_pure_open_folder, label: tr! { "zh-Hans" => "打开文件夹", "en" => "Open Folder" }, width: fill, shape: capsule, height: 32, pick: pure.folder, files: "folder") => app.open_folder(pure.folder)
                        Button(id: phone_pure_open_link, label: tr! { "zh-Hans" => "打开链接", "en" => "Open Link" }, width: fill, shape: capsule, height: 32) => app.show_link_prompt(true)
                    }
                    Canvas(height: 10)
                    Button(id: phone_continue_last, label: tr! { "zh-Hans" => "继续上次", "en" => "Continue" }, icon: "play.fill", width: fill, shape: capsule, height: 32) => app.continue_last()
                    Canvas(height: 10)
                    // `.x-mut`, 2 points in.
                    Row(gap: 0, padding_x: 2) {
                        Text(content: tr! { "zh-Hans" => "在别的 App 里选「打开方式 › Erika」也能直接播放（对应电脑上把文件拖进窗口）。", "en" => "Choose Open With › Erika in another app to play right away (as dropping a file into the window does on a computer)." }, style: caption, emphasis: secondary, line_height: 15, lines: 3)
                    }
                    Canvas(height: 8)
                }
            }
            // `.c-list` under its label (`.c-glabel`): the picture 72 wide.
            Section(title: tr! { "zh-Hans" => "最近播放", "en" => "Recently Played" }, padding_x: 14, padding_y: 6) {
                ForEach(items: app.recents, key: name) |recent| {
                    Button(id: phone_recent, label: recent.name) {
                        Row(gap: 12, align: center) {
                            Stack(align: bottom_leading, variant: phone_recent_thumb) {
                                Image(source: recent.still, width: 72, height: 40.5, fit: cover, corner: 6, label: recent.name)
                                Rail {
                                    Canvas(from: 0, to: 1, height: 3, appearance: light) {
                                        Shape(kind: rect, fill: text.primary, opacity: 0.41)
                                    }
                                }
                                Rail {
                                    Canvas(from: 0, to: recent.progress, height: 3) {
                                        Shape(kind: rect, fill: accent)
                                    }
                                }
                            }
                            Column(gap: 0) {
                                Text(content: recent.name, style: headline, font_size: 13, lines: 1, line_height: 18)
                                Text(content: tr! { "zh-Hans" => "剩 {left, duration, m:ss}", "en" => "{left, duration, m:ss} left"; left: recent.remaining }, style: caption, emphasis: secondary, line_height: 15)
                            }
                            Spacer
                        }
                    } => app.play_recent(recent.name)
                }
            }
        }
    }
}
