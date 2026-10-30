//! 「关于 Erika」 and 「Erika 帮助」, which the Mac's menu bar opens over the
//! main window in place of the system's about panel and help book. The
//! about is the page 「设置 › 关于」 itself; the help tells what each
//! feature switched on does, so pure playback tells only of playing.
use crate::settings::{settings_about, settings_about_props};
use bern::prelude::*;
use erika_protocol::erika;

#[component]
pub fn info_dialogs(app: erika::Client) -> impl View {
    ui! {
        if app.about_dialog {
            // The settings' form keeps its own size: the panel gives it
            // the height its two sections take.
            Modal(width: 420, height: 376, appear: pop_in, disappear: fade) {
                Column(gap: 0) {
                    settings_about(app: app)
                    Row(gap: 12, align: center, padding_x: 20, padding_y: 16) {
                        Spacer
                        Button(id: about_done, label: tr! { "zh-Hans" => "完成", "en" => "Done" }, prominence: prominent, shortcut: app.keys.back) => app.show_about(false)
                    }
                }
            }
        } else if app.help_dialog {
            help_dialog(app: app)
        }
    }
}

/// One topic for each feature, as 「设置 › 可选功能」 names and orders them.
#[component]
pub fn help_dialog(app: erika::Client) -> impl View {
    ui! {
        Modal(width: 460, appear: pop_in, disappear: fade) {
            Column(gap: 16, padding: 24) {
                Text(content: tr! { "zh-Hans" => "Erika 帮助", "en" => "Erika Help" }, style: title3)
                ScrollView {
                    Column(gap: 16) {
                        Row(gap: 12, align: top) {
                            help_icon(feature: HelpTopic::Playback)
                            Row(gap: 0) {
                                Column(gap: 3) {
                                    Text(content: tr! { "zh-Hans" => "播放", "en" => "Playback" }, style: headline)
                                    Text(content: tr! { "zh-Hans" => "打开视频文件、文件夹或网络链接即可播放。播放时可以切换字幕与音轨、调整倍速、截图。", "en" => "Open a video file, a folder or a web link to play it. While it plays, switch subtitles and audio tracks, change the speed or take a screenshot." }, emphasis: secondary, lines: 4)
                                }
                                Spacer
                            }
                        }
                        if build.modules.danmaku && app.features.danmaku {
                            Row(gap: 12, align: top) {
                                help_icon(feature: HelpTopic::Danmaku)
                                Row(gap: 0) {
                                    Column(gap: 3) {
                                        Text(content: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, style: headline)
                                        Text(content: tr! { "zh-Hans" => "播放时从弹弹play 自动匹配弹幕，也可以挂载本地的弹幕文件。", "en" => "Danmaku is matched from dandanplay as you play; you can also mount a danmaku file of your own." }, emphasis: secondary, lines: 4)
                                    }
                                    Spacer
                                }
                            }
                        }
                        if build.modules.enhance && app.features.enhance {
                            Row(gap: 12, align: top) {
                                help_icon(feature: HelpTopic::Enhance)
                                Row(gap: 0) {
                                    Column(gap: 3) {
                                        Text(content: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }, style: headline)
                                        Text(content: tr! { "zh-Hans" => "用 ArtCNN 超分辨率让低分辨率的动画更清晰，在播放设置的「画面」里选择。", "en" => "ArtCNN upscaling sharpens low-resolution animation; choose it under Picture in the playback settings." }, emphasis: secondary, lines: 4)
                                    }
                                    Spacer
                                }
                            }
                        }
                        if build.modules.account && app.features.account {
                            Row(gap: 12, align: top) {
                                help_icon(feature: HelpTopic::Account)
                                Row(gap: 0) {
                                    Column(gap: 3) {
                                        Text(content: tr! { "zh-Hans" => "账号", "en" => "Account" }, style: headline)
                                        // What signing in adds, with the features
                                        // it serves: lines as close as a paragraph's.
                                        Column(gap: 0) {
                                            Text(content: tr! { "zh-Hans" => "登录弹弹play 与 Bangumi 账号。", "en" => "Sign in to dandanplay and Bangumi." }, emphasis: secondary, lines: 4)
                                            if build.modules.danmaku && app.features.danmaku {
                                                Text(content: tr! { "zh-Hans" => "登录弹弹play 后可以发送弹幕。", "en" => "Signed in to dandanplay, you can send danmaku." }, emphasis: secondary, lines: 4)
                                            }
                                            if build.modules.anime && app.features.anime {
                                                Text(content: tr! { "zh-Hans" => "追番状态、评分和看过的集数随账号同步。", "en" => "What you watch, your ratings and the episodes you have seen sync with the accounts." }, emphasis: secondary, lines: 4)
                                            }
                                        }
                                    }
                                    Spacer
                                }
                            }
                        }
                        if build.modules.library && app.features.library {
                            Row(gap: 12, align: top) {
                                help_icon(feature: HelpTopic::Library)
                                Row(gap: 0) {
                                    Column(gap: 3) {
                                        Text(content: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, style: headline)
                                        Text(content: tr! { "zh-Hans" => "添加本机的文件夹，或连接 NipaPlay、弹弹play 共享的媒体，自动识别其中的番剧与剧集，配上封面。", "en" => "Add folders on this computer, or connect to media NipaPlay or dandanplay shares; the shows and episodes in them are recognized and given posters." }, emphasis: secondary, lines: 4)
                                    }
                                    Spacer
                                }
                            }
                        }
                        if build.modules.anime && app.features.anime {
                            Row(gap: 12, align: top) {
                                help_icon(feature: HelpTopic::Anime)
                                Row(gap: 0) {
                                    Column(gap: 3) {
                                        Text(content: tr! { "zh-Hans" => "新番", "en" => "New Shows" }, style: headline)
                                        Text(content: tr! { "zh-Hans" => "首页列出最近在看的番剧，新番页按星期排出本季的放送。番剧详情里有简介、剧集与评论，可以标记看到第几集、打分。", "en" => "Home lists the shows you are watching, and New Shows lays out the season's broadcasts by weekday. A show's page has its story, episodes and comments, and lets you mark episodes and rate it." }, emphasis: secondary, lines: 4)
                                    }
                                    Spacer
                                }
                            }
                        }
                        if build.modules.sync && app.features.sync {
                            Row(gap: 12, align: top) {
                                help_icon(feature: HelpTopic::Sync)
                                Row(gap: 0) {
                                    Column(gap: 3) {
                                        Text(content: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, style: headline)
                                        Text(content: tr! { "zh-Hans" => "用 WebDAV 在几台设备之间同步，或把数据导出成一个备份文件，到另一台设备上导入。", "en" => "Sync between devices over WebDAV, or export your data as one backup file to import on another device." }, emphasis: secondary, lines: 4)
                                    }
                                    Spacer
                                }
                            }
                        }
                        if build.modules.downloads && app.features.downloads {
                            Row(gap: 12, align: top) {
                                help_icon(feature: HelpTopic::Downloads)
                                Row(gap: 0) {
                                    Column(gap: 3) {
                                        Text(content: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, style: headline)
                                        Text(content: tr! { "zh-Hans" => "用种子文件或磁力链接下载，边下边播；下载好的可以放进媒体库。", "en" => "Download from torrent files or magnet links and play while downloading; finished downloads can go into the library." }, emphasis: secondary, lines: 4)
                                    }
                                    Spacer
                                }
                            }
                        }
                        if app.features.plugins {
                            Row(gap: 12, align: top) {
                                help_icon(feature: HelpTopic::Plugins)
                                Row(gap: 0) {
                                    Column(gap: 3) {
                                        Text(content: tr! { "zh-Hans" => "插件", "en" => "Plugins" }, style: headline)
                                        Text(content: tr! { "zh-Hans" => "从插件市场安装小功能，或导入 Erika 插件包（.epk）与 NipaPlay 的 .js 插件。", "en" => "Install small extras from the plugin market, or import an Erika plugin package (.epk) or a NipaPlay .js plugin." }, emphasis: secondary, lines: 4)
                                    }
                                    Spacer
                                }
                            }
                        }
                    }
                }
                Row(gap: 12, align: center) {
                    Spacer
                    Button(id: help_done, label: tr! { "zh-Hans" => "完成", "en" => "Done" }, prominence: prominent, shortcut: app.keys.back) => app.show_help(false)
                }
            }
        }
    }
}

/// The topics of the help, each with the symbol 「设置」 gives its feature.
#[bern::value]
pub enum HelpTopic {
    Playback,
    Danmaku,
    Enhance,
    Account,
    Library,
    Anime,
    Sync,
    Downloads,
    Plugins,
}

/// A topic's symbol on the neutral tile of 「设置 › 可选功能」.
#[component]
pub fn help_icon(feature: HelpTopic) -> impl View {
    ui! {
        Stack(width: 28, height: 28) {
            Canvas {
                Shape(kind: rounded, corner: 7, fill: fill.strong)
            }
            match feature {
                HelpTopic::Playback => { Label(title: tr! { "zh-Hans" => "播放", "en" => "Playback" }, icon: "play", style: body, display: icon) }
                HelpTopic::Danmaku => { Label(title: tr! { "zh-Hans" => "弹幕", "en" => "Danmaku" }, icon: "text.bubble", style: body, display: icon) }
                HelpTopic::Enhance => { Label(title: tr! { "zh-Hans" => "画质增强", "en" => "Upscaling" }, icon: "sparkles", style: body, display: icon) }
                HelpTopic::Account => { Label(title: tr! { "zh-Hans" => "账号", "en" => "Account" }, icon: "person.crop.circle", style: body, display: icon) }
                HelpTopic::Library => { Label(title: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, icon: "film.stack", style: body, display: icon) }
                HelpTopic::Anime => { Label(title: tr! { "zh-Hans" => "新番", "en" => "New Shows" }, icon: "calendar", style: body, display: icon) }
                HelpTopic::Sync => { Label(title: tr! { "zh-Hans" => "账号同步", "en" => "Account Sync" }, icon: "arrow.triangle.2.circlepath", style: body, display: icon) }
                HelpTopic::Downloads => { Label(title: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, icon: "arrow.down.circle", style: body, display: icon) }
                HelpTopic::Plugins => { Label(title: tr! { "zh-Hans" => "插件", "en" => "Plugins" }, icon: "puzzlepiece.extension", style: body, display: icon) }
            }
        }
    }
}
