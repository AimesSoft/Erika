//! Desktop search palette, measured from design/next-screens.js and next.css.
use crate::{AppWindows, ErikaDraft, MainPage};
use bern::prelude::*;
use erika_anime_protocol::anime;
use erika_protocol::erika::{self, Appearance};

#[component]
pub fn search_results(
    app: erika::Client,
    anime: anime::Client,
    windows: AppWindows,
    #[session] draft: ErikaDraft,
) -> impl View {
    ui! {
        Query(id: commands, query: app.search_actions(draft.query)) |actions| {
            if app.features.anime {
              Query(id: shows, query: anime.search(draft.query)) |found| {
                Column(gap: 0) {
                    if found.has_shows {
                        Column(gap: 0, padding_x: 12) {
                            Canvas(height: 10)
                            Text(content: tr! { "zh-Hans" => "番剧", "en" => "Shows" }, style: caption, emphasis: secondary, line_height: 15)
                            Canvas(height: 6)
                        }
                        ScrollView(axis: horizontal) {
                            Row(gap: 12, padding_x: 12, padding_y: 4, align: top) {
                                ForEach(items: found.shows, key: id) |show| {
                                    SearchItem(id: show, label: show.title, navigation: false) {
                                        // A television's posters are 96 wide (`.tv-sr .pp`).
                                        Column(gap: 5, width: if window.tv { 96 } else { 70 }) {
                                            Image(source: show.poster, width: if window.tv { 96 } else { 70 }, height: if window.tv { 144 } else { 105 }, fit: cover, corner: poster, label: show.title)
                                            Text(content: show.title, style: caption, lines: 1, line_height: 15)
                                        }
                                    } => anime.open_from_search(show.id)
                                }
                            }
                        }
                        Canvas(height: 4)
                    }
                    if found.has_continuing {
                        Column(gap: 0, padding_x: 12) {
                            Canvas(height: 10)
                            Text(content: tr! { "zh-Hans" => "继续播放", "en" => "Continue Watching" }, style: caption, emphasis: secondary, line_height: 15)
                            Canvas(height: 6)
                        }
                        ForEach(items: found.continuing, key: id) |episode| {
                            SearchItem(id: continue_show, label: episode.title, icon: "play", detail: tr! { "zh-Hans" => "第 {episode} 话", "en" => "Episode {episode}"; episode: episode.episode }, hint: tr! { "zh-Hans" => "↵", "en" => "↵" }) => anime.continue_from_search(episode.id)
                        }
                    }
                    search_commands(app: app, windows: windows, actions: actions)
                    if !found.has_shows && actions.empty {
                        Row(padding: 30, align: center) {
                            Spacer
                            Text(content: tr! { "zh-Hans" => "没有找到「{query}」", "en" => "No results for “{query}”"; query: draft.query }, emphasis: secondary)
                            Spacer
                        }
                    }
                }
              }
            } else {
                Column(gap: 0) {
                    search_commands(app: app, windows: windows, actions: actions)
                    if actions.empty {
                        Row(padding: 30, align: center) {
                            Spacer
                            Text(content: tr! { "zh-Hans" => "没有找到「{query}」", "en" => "No results for “{query}”"; query: draft.query }, emphasis: secondary)
                            Spacer
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn search_commands(
    app: erika::Client,
    windows: AppWindows,
    actions: erika::SearchActions,
    #[session] draft: ErikaDraft,
) -> impl View {
    ui! {
        Column(gap: 0) {
            if actions.has_settings {
                Column(gap: 0, padding_x: 12) {
                    Canvas(height: 10)
                    Text(content: tr! { "zh-Hans" => "设置", "en" => "Settings" }, style: caption, emphasis: secondary, line_height: 15)
                    Canvas(height: 6)
                }
                if actions.theme {
                    SearchItem(id: theme, label: tr! { "zh-Hans" => "主题色", "en" => "Accent Color" }, icon: "paintpalette", detail: tr! { "zh-Hans" => "设置 › 外观", "en" => "Settings › Appearance" }) => windows.settings.open()
                }
                if actions.danmaku {
                    SearchItem(id: danmaku, label: tr! { "zh-Hans" => "弹幕不透明度", "en" => "Danmaku Opacity" }, icon: "text.bubble", detail: tr! { "zh-Hans" => "设置 › 弹幕", "en" => "Settings › Danmaku" }) => windows.settings.open()
                }
                if actions.skip_intro {
                    SearchItem(id: skip_intro, label: tr! { "zh-Hans" => "跳过片头秒数", "en" => "Skip Intro Seconds" }, icon: "play", detail: tr! { "zh-Hans" => "设置 › 播放", "en" => "Settings › Playback" }) => windows.settings.open()
                }
                if actions.optional {
                    SearchItem(id: optional, label: tr! { "zh-Hans" => "可选功能", "en" => "Optional Features" }, icon: "square.grid.2x2", detail: tr! { "zh-Hans" => "设置", "en" => "Settings" }) => windows.settings.open()
                }
                if actions.plugins {
                    SearchItem(id: plugins, label: tr! { "zh-Hans" => "插件市场", "en" => "Plugin Marketplace" }, icon: "puzzlepiece.extension", detail: tr! { "zh-Hans" => "设置 › 插件", "en" => "Settings › Plugins" }) => windows.settings.open()
                }
                if actions.keys {
                    SearchItem(id: keys, label: tr! { "zh-Hans" => "快捷键", "en" => "Keyboard Shortcuts" }, icon: "keyboard", detail: tr! { "zh-Hans" => "设置", "en" => "Settings" }) => windows.settings.open()
                }
            }
            if actions.has_operations {
                Column(gap: 0, padding_x: 12) {
                    Canvas(height: 10)
                    Text(content: tr! { "zh-Hans" => "操作", "en" => "Actions" }, style: caption, emphasis: secondary, line_height: 15)
                    Canvas(height: 6)
                }
                // A television has no files to open.
                if actions.open_file && !window.tv {
                    SearchItem(id: open_file, label: tr! { "zh-Hans" => "打开文件…", "en" => "Open File…" }, icon: "folder", hint: tr! { "zh-Hans" => "⌘O", "en" => "⌘O" }, pick: draft.file, files: "video") => app.open_file(draft.file)
                }
                if actions.schedule {
                    SearchItem(id: schedule, label: tr! { "zh-Hans" => "新番表", "en" => "Airing Schedule" }, icon: "calendar") => draft.page.set(MainPage::New)
                }
                if actions.dark {
                    SearchItem(id: dark, label: tr! { "zh-Hans" => "切换深色模式", "en" => "Toggle Dark Mode" }, icon: "moon") => app.set_appearance(if app.appearance == Appearance::Dark { Appearance::Light } else { Appearance::Dark })
                }
            }
        }
    }
}
