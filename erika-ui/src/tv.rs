//! The television (design 3.1, 6.2 `TV`, next.css `[data-dev="tv"]`): the
//! desktop's pages in the prototype's 1280 × 720 scene, which the screen
//! shows 1.5 times as large, under the system's tabs along the top; search
//! is the first tab. 「打开」, the settings and the account stand at the
//! top right (`.x-acct`), what plays is a picture in the bottom right
//! corner (`.x-mini`), and the remote's Back closes a show's details. The
//! remote moves the focus; the system draws it. The scene fills the whole
//! screen: the design's 64-point margins are the overscan's safe area.
use crate::pure::{link_resolving, link_resolving_props, pure_link, pure_link_props};
use crate::search::{search_results, search_results_props};
use crate::{AppWindows, ErikaDraft, MainPage, avatar, avatar_props};
use bern::prelude::*;
#[cfg(feature = "bern-module-anime")]
use erika_anime_protocol::anime::{self, BackTo};
#[cfg(feature = "bern-module-anime")]
use erika_anime_ui::{details, details_props};
use erika_protocol::erika;

#[component]
pub fn tv_shell(
    app: erika::Client,
    #[cfg(feature = "bern-module-anime")] anime: anime::Client,
    windows: AppWindows,
    #[session] draft: ErikaDraft,
) -> impl View {
    ui! {
        Stack(variant: app_background, align: top_leading) {
            // Pure playback (design 10.2): no pages, so no tabs.
            if !(build.modules.anime && app.features.anime) && !(build.modules.library && app.features.library) && !(build.modules.downloads && app.features.downloads) {
                Stack(enabled: !app.link_prompt) {
                    tv_pure(app: app, windows: windows)
                }
            } else {
                // Behind 「打开链接」 the pages take no focus.
                Stack(enabled: !app.link_prompt, align: top_leading) {
                    Tabs(id: tv_tabs, selection: draft.page) {
                        // The search's symbol alone, named 「搜索」 for
                        // VoiceOver, and the pages' names alone, as the
                        // television's tabs show them (`.c-navi .ic`).
                        Tab(value: MainPage::Search, label: tr! { "zh-Hans" => "搜索", "en" => "Search" }, icon: "magnifyingglass", display: icon) {
                            tv_scene(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, windows: windows, page: MainPage::Search)
                        }
                        if (build.modules.anime && app.features.anime) {
                            Tab(value: MainPage::Home, label: tr! { "zh-Hans" => "首页", "en" => "Home" }) {
                                tv_scene(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, windows: windows, page: MainPage::Home)
                            }
                            Tab(value: MainPage::New, label: tr! { "zh-Hans" => "新番", "en" => "New" }) {
                                tv_scene(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, windows: windows, page: MainPage::New)
                            }
                        }
                        if (build.modules.library && app.features.library) {
                            Tab(value: MainPage::Library, label: tr! { "zh-Hans" => "媒体库", "en" => "Library" }) {
                                tv_scene(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, windows: windows, page: MainPage::Library)
                            }
                        }
                        if (build.modules.downloads && app.features.downloads) {
                            Tab(value: MainPage::Downloads, label: tr! { "zh-Hans" => "下载", "en" => "Downloads" }) {
                                tv_scene(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, windows: windows, page: MainPage::Downloads)
                            }
                        }
                    }
                }
            }
            // The remote's Back closes a show's details, as the design's
            // way back does (菜单 → 播放器 → 弹窗 → 详情 → 首页).
            if build.modules.anime && anime.opened > 0 && !app.playing {
                Shortcut(key: app.keys.back) => anime.close()
            }
            // 「打开链接」 in the television's scene.
            if app.link_prompt {
                Stack(under_bars: true) {
                    ScaleBox(width: 1280, height: 720) {
                        pure_link(app: app)
                    }
                }
            } else if app.link_resolving {
                Stack(under_bars: true) {
                    ScaleBox(width: 1280, height: 720) {
                        link_resolving(app: app)
                    }
                }
            }
        }
    }
}

/// One tab's page in the prototype's 1280 × 720 scene, the page 80 points
/// down, under the tabs (`.x-main { margin-top: 80px }`). 「打开」, the
/// settings and the account stand at the top right (`.x-acct`: 48 from the
/// right edge, 17 down), what plays in the bottom right corner
/// (`.x-mini`: 300 wide, 40 from the right, 32 from the bottom).
#[component]
pub fn tv_scene(
    app: erika::Client,
    #[cfg(feature = "bern-module-anime")] anime: anime::Client,
    windows: AppWindows,
    page: MainPage,
    #[session] draft: ErikaDraft,
) -> impl View {
    ui! {
        Stack(under_bars: true) {
            ScaleBox(width: 1280, height: 720) {
                Stack(width: 1280, height: 720, align: top_leading) {
                    Positioned(x: 0, y: 80, width: 1280, height: 640) {
                        if page == MainPage::Search {
                            if build.modules.anime && anime.opened > 0 && anime.back == BackTo::Search {
                                details(app: app, #[cfg(feature = "bern-module-anime")] anime: anime)
                            } else {
                                tv_search(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, windows: windows)
                            }
                        } else if build.modules.library && build.modules.anime && page == MainPage::Library && anime.opened > 0 && anime.back == BackTo::Library {
                            details(app: app, #[cfg(feature = "bern-module-anime")] anime: anime)
                        } else {
                            PageContent(slot: main, selected: draft.page)
                        }
                    }
                    Positioned(x: 1080, y: 17, width: 152, height: 46) {
                        tv_actions(app: app, windows: windows)
                    }
                    if app.playing && !app.expanded {
                        Positioned(x: 940, y: 519, width: 300, height: 169, appear: fade, disappear: fade) {
                            tv_mini_player(app: app)
                        }
                    }
                }
            }
        }
    }
}

/// The account (`.x-av`, 40 points), then 「打开」, which opens a link (a
/// television has no files to pick), and the gear, the settings, which
/// fill the screen; 10 points apart (`.x-acct`).
#[component]
pub fn tv_actions(app: erika::Client, windows: AppWindows) -> impl View {
    ui! {
        Row(gap: 10, align: center) {
            if build.modules.account && app.features.account {
                Stack(width: 40, height: 40, align: center) {
                    avatar(app: app)
                }
            }
            IconButton(id: tv_open, icon: "folder", label: tr! { "zh-Hans" => "打开", "en" => "Open" }, height: 46) => app.show_link_prompt(true)
            IconButton(id: tv_settings, icon: "gearshape", label: tr! { "zh-Hans" => "设置", "en" => "Settings" }, height: 46) => windows.settings.open()
        }
    }
}

/// The search tab (`searchHTML` on the television): the title with how
/// to type, the remote's keyboard of initials on the left (440 points,
/// seven keys of 56 × 52 to a row, 8 apart) and the field with the
/// results 40 points to its right. The field takes the system's keyboard
/// and dictation too.
#[component]
pub fn tv_search(
    app: erika::Client,
    #[cfg(feature = "bern-module-anime")] anime: anime::Client,
    windows: AppWindows,
    #[session] draft: ErikaDraft,
) -> impl View {
    ui! {
        ScrollView(appear: page_in) {
            Column(gap: 0, padding_x: 56, padding_y: 6) {
                Row(gap: 14, align: baseline) {
                    Text(content: tr! { "zh-Hans" => "搜索", "en" => "Search" }, style: large_title)
                    Text(content: tr! { "zh-Hans" => "用遥控器选拼音首字母，或按住遥控器的语音键说出番名", "en" => "Pick initials with the remote, or hold its voice button and say a title" }, emphasis: secondary, lines: 1)
                    Spacer
                }
                Canvas(height: 18)
                Row(gap: 40, align: top) {
                    tv_keyboard()
                    Column(gap: 0) {
                        Search(id: tv_search, value: draft.query, prompt: tr! { "zh-Hans" => "番剧、剧集、设置", "en" => "Shows, episodes, settings" }, height: 52, font_size: 18, suggest: true) {
                            Column(gap: 0) {
                                Canvas(height: 6)
                                search_results(app: app, #[cfg(feature = "bern-module-anime")] anime: anime, windows: windows)
                            }
                        }
                    }
                }
                Canvas(height: 64)
            }
        }
    }
}

/// The remote's keyboard (`.tv-kb`): A to Z, then space, delete and clear,
/// two keys wide.
#[component]
pub fn tv_keyboard(#[session] draft: ErikaDraft) -> impl View {
    ui! {
        Column(gap: 8, width: 440) {
            Row(gap: 8) {
                Button(id: key_a, label: tr! { "zh-Hans" => "A", "en" => "A" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "A", "en" => "A" }, font_size: 18)
                    }
                } => draft.query.push_str("A")
                Button(id: key_b, label: tr! { "zh-Hans" => "B", "en" => "B" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "B", "en" => "B" }, font_size: 18)
                    }
                } => draft.query.push_str("B")
                Button(id: key_c, label: tr! { "zh-Hans" => "C", "en" => "C" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "C", "en" => "C" }, font_size: 18)
                    }
                } => draft.query.push_str("C")
                Button(id: key_d, label: tr! { "zh-Hans" => "D", "en" => "D" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "D", "en" => "D" }, font_size: 18)
                    }
                } => draft.query.push_str("D")
                Button(id: key_e, label: tr! { "zh-Hans" => "E", "en" => "E" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "E", "en" => "E" }, font_size: 18)
                    }
                } => draft.query.push_str("E")
                Button(id: key_f, label: tr! { "zh-Hans" => "F", "en" => "F" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "F", "en" => "F" }, font_size: 18)
                    }
                } => draft.query.push_str("F")
                Button(id: key_g, label: tr! { "zh-Hans" => "G", "en" => "G" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "G", "en" => "G" }, font_size: 18)
                    }
                } => draft.query.push_str("G")
            }
            Row(gap: 8) {
                Button(id: key_h, label: tr! { "zh-Hans" => "H", "en" => "H" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "H", "en" => "H" }, font_size: 18)
                    }
                } => draft.query.push_str("H")
                Button(id: key_i, label: tr! { "zh-Hans" => "I", "en" => "I" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "I", "en" => "I" }, font_size: 18)
                    }
                } => draft.query.push_str("I")
                Button(id: key_j, label: tr! { "zh-Hans" => "J", "en" => "J" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "J", "en" => "J" }, font_size: 18)
                    }
                } => draft.query.push_str("J")
                Button(id: key_k, label: tr! { "zh-Hans" => "K", "en" => "K" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "K", "en" => "K" }, font_size: 18)
                    }
                } => draft.query.push_str("K")
                Button(id: key_l, label: tr! { "zh-Hans" => "L", "en" => "L" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "L", "en" => "L" }, font_size: 18)
                    }
                } => draft.query.push_str("L")
                Button(id: key_m, label: tr! { "zh-Hans" => "M", "en" => "M" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "M", "en" => "M" }, font_size: 18)
                    }
                } => draft.query.push_str("M")
                Button(id: key_n, label: tr! { "zh-Hans" => "N", "en" => "N" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "N", "en" => "N" }, font_size: 18)
                    }
                } => draft.query.push_str("N")
            }
            Row(gap: 8) {
                Button(id: key_o, label: tr! { "zh-Hans" => "O", "en" => "O" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "O", "en" => "O" }, font_size: 18)
                    }
                } => draft.query.push_str("O")
                Button(id: key_p, label: tr! { "zh-Hans" => "P", "en" => "P" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "P", "en" => "P" }, font_size: 18)
                    }
                } => draft.query.push_str("P")
                Button(id: key_q, label: tr! { "zh-Hans" => "Q", "en" => "Q" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "Q", "en" => "Q" }, font_size: 18)
                    }
                } => draft.query.push_str("Q")
                Button(id: key_r, label: tr! { "zh-Hans" => "R", "en" => "R" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "R", "en" => "R" }, font_size: 18)
                    }
                } => draft.query.push_str("R")
                Button(id: key_s, label: tr! { "zh-Hans" => "S", "en" => "S" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "S", "en" => "S" }, font_size: 18)
                    }
                } => draft.query.push_str("S")
                Button(id: key_t, label: tr! { "zh-Hans" => "T", "en" => "T" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "T", "en" => "T" }, font_size: 18)
                    }
                } => draft.query.push_str("T")
                Button(id: key_u, label: tr! { "zh-Hans" => "U", "en" => "U" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "U", "en" => "U" }, font_size: 18)
                    }
                } => draft.query.push_str("U")
            }
            Row(gap: 8) {
                Button(id: key_v, label: tr! { "zh-Hans" => "V", "en" => "V" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "V", "en" => "V" }, font_size: 18)
                    }
                } => draft.query.push_str("V")
                Button(id: key_w, label: tr! { "zh-Hans" => "W", "en" => "W" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "W", "en" => "W" }, font_size: 18)
                    }
                } => draft.query.push_str("W")
                Button(id: key_x, label: tr! { "zh-Hans" => "X", "en" => "X" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "X", "en" => "X" }, font_size: 18)
                    }
                } => draft.query.push_str("X")
                Button(id: key_y, label: tr! { "zh-Hans" => "Y", "en" => "Y" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "Y", "en" => "Y" }, font_size: 18)
                    }
                } => draft.query.push_str("Y")
                Button(id: key_z, label: tr! { "zh-Hans" => "Z", "en" => "Z" }, variant: tv_key) {
                    Stack(width: 56, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "Z", "en" => "Z" }, font_size: 18)
                    }
                } => draft.query.push_str("Z")
                Button(id: key_space, label: tr! { "zh-Hans" => "空格", "en" => "Space" }, variant: tv_key) {
                    Stack(width: 120, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "空格", "en" => "Space" }, font_size: 15)
                    }
                } => draft.query.push_str(" ")
            }
            Row(gap: 8) {
                Button(id: key_delete, label: tr! { "zh-Hans" => "删除", "en" => "Delete" }, variant: tv_key) {
                    Stack(width: 120, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "删除", "en" => "Delete" }, font_size: 15)
                    }
                } => draft.query.pop()
                Button(id: key_clear, label: tr! { "zh-Hans" => "清空", "en" => "Clear" }, variant: tv_key) {
                    Stack(width: 120, height: 52, align: center) {
                        Text(content: tr! { "zh-Hans" => "清空", "en" => "Clear" }, font_size: 15)
                    }
                } => draft.query.set("")
            }
        }
    }
}

/// `.x-mini` on the television: the picture with its progress along the
/// bottom edge; the focus reaches it, and select grows the player back.
#[component]
pub fn tv_mini_player(app: erika::Client) -> impl View {
    ui! {
        Button(id: tv_mini, label: app.title, variant: stage) {
            Stack(align: bottom_leading) {
                Image(source: app.still, width: 300, height: 169, corner: 22, fit: cover, label: app.title, hero: frame, hero_key: app.title)
                Rail {
                    Canvas(from: 0, to: app.progress, height: 3) {
                        Shape(kind: rect, fill: accent)
                    }
                }
            }
        } => app.expand()
    }
}

/// Pure playback on the television: the empty frame where the player's
/// frame steps back to (200, 52, 880 × 495), the ways to open a video a
/// television has, and the settings at the top right.
#[component]
pub fn tv_pure(app: erika::Client, windows: AppWindows) -> impl View {
    ui! {
        Stack(under_bars: true) {
            ScaleBox(width: 1280, height: 720) {
                Stack(width: 1280, height: 720, align: top_leading) {
                    Positioned(x: 1186, y: 17, width: 46, height: 46) {
                        IconButton(id: tv_pure_settings, icon: "gearshape", label: tr! { "zh-Hans" => "设置", "en" => "Settings" }, height: 46) => windows.settings.open()
                    }
                    Positioned(x: 200, y: 52, width: 880, height: 495) {
                        Stack(width: 880, height: 495, appear: pure_frame_in) {
                            Canvas(width: 880, height: 495) {
                                Shape(kind: rounded, corner: 22, fill: fill.subtle)
                                Shape(kind: rounded, x: 1, y: 1, width: 878, height: 493, corner: 21, stroke: text.placeholder, stroke_width: 2)
                                Shape(kind: rounded, x: 10.75, y: 10.75, width: 858.5, height: 473.5, corner: 15.25, stroke: text.placeholder, stroke_width: 1.5, dash: 4.5, dash_gap: 3)
                            }
                            Column(gap: 0, align: center) {
                                Stack(variant: pure_symbol, width: 56, height: 56, align: center) {
                                    Label(title: tr! { "zh-Hans" => "视频", "en" => "Video" }, icon: "play.rectangle", display: icon, font_size: 48)
                                }
                                Canvas(height: 14)
                                Text(content: tr! { "zh-Hans" => "打开一个视频", "en" => "Open a video" }, style: title2, line_height: 34)
                                Canvas(height: 8)
                                Text(content: tr! { "zh-Hans" => "打开链接，或者在手机上的 Erika 里投送到电视", "en" => "Open a link, or send a video from Erika on your phone" }, style: body, emphasis: secondary, line_height: 18)
                                Canvas(height: 26)
                                Row(gap: 10, align: center) {
                                    Button(id: tv_pure_open_link, label: tr! { "zh-Hans" => "打开链接…", "en" => "Open Link…" }, icon: "link", shape: capsule, height: 42, prominence: prominent, variant: primary) => app.show_link_prompt(true)
                                    Button(id: tv_continue_last, label: tr! { "zh-Hans" => "继续上次", "en" => "Continue" }, icon: "play.fill", shape: capsule, height: 42) => app.continue_last()
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
