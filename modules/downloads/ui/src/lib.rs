//! Downloads: progress is the row's background, with a two-point accent edge.
use bern::prelude::*;
use erika_downloads_protocol::downloads::{self, DownloadTab};

#[component]
pub fn downloads_clock(downloads: downloads::Client) -> impl View {
    ui! {
        Stack {
        Timer(interval: 0.5, enabled: downloads.running) => downloads.tick()
        if downloads.notice_revision > 0 {
            Toast(content: downloads.notice, key: downloads.notice_revision, icon: "checkmark.circle")
        }
        }
    }
}
#[component]
pub fn downloads_page(downloads: downloads::Client) -> impl View {
    ui! {
        Column(gap: 0, padding_x: if window.medium { 24.0 } else { 32.0 }, padding_y: 6, appear: page_in) {
            Adaptive {
                Layout(size: expanded) {
                    Row(gap: 12, align: center) {
                        Text(content: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, style: large_title)
                        download_tabs(downloads: downloads)
                        Spacer
                        Button(id: add, label: tr! { "zh-Hans" => "添加任务", "en" => "Add task" }, icon: "plus", shape: capsule, height: 32, prominence: prominent) => downloads.add_task()
                    }
                }
                Layout(size: compact) {
                    Column(gap: 12) {
                        Row(gap: 12, align: center) {
                            Text(content: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, style: large_title)
                            Spacer
                            IconButton(id: add_small, label: tr! { "zh-Hans" => "添加任务", "en" => "Add task" }, icon: "plus", prominence: plain) => downloads.add_task()
                        }
                        download_tabs(downloads: downloads)
                    }
                }
            }
            Canvas(height: 14)
            ScrollView {
                Column(gap: 14) {
                    Stack(variant: row_group) {
                        Column(gap: 0, animate: smooth) {
                            if downloads.tab == DownloadTab::Active {
                                if downloads.active_count == 0 {
                                    Row(padding_x: 14, height: 44) {
                                        Text(content: tr! { "zh-Hans" => "没有正在下载的任务", "en" => "No downloads in progress" })
                                        Spacer
                                    }
                                }
                                ForEach(items: downloads.active, key: id, reflow: smooth) |task| {
                                    download_row(downloads: downloads, task: task)
                                }
                            } else {
                                if downloads.completed_count == 0 {
                                    Row(padding_x: 14, height: 44) {
                                        Text(content: tr! { "zh-Hans" => "还没有下载完成的文件", "en" => "No completed downloads yet" })
                                        Spacer
                                    }
                                }
                                ForEach(items: downloads.completed, key: id, reflow: smooth) |task| {
                                    download_row(downloads: downloads, task: task)
                                }
                            }
                        }
                    }
                    Text(content: tr! { "zh-Hans" => "行的底色就是进度。下载完成的文件会自动加入媒体库。", "en" => "Each row's background shows its progress. Completed files are automatically added to the library." }, style: caption, font_size: 11.5, emphasis: secondary, lines: 3)
                }
            }
        }
    }
}
#[component]
pub fn download_tabs(downloads: downloads::Client) -> impl View {
    ui! {
        Row(width: 218) {
            Picker(id: tab, label: tr! { "zh-Hans" => "下载状态", "en" => "Download status" }, selected: downloads.tab, style: segmented, label_hidden: true) {
                PickerOption(value: DownloadTab::Active, label: tr! { "zh-Hans" => "进行中 {n}", "en" => "In progress {n}"; n: downloads.active_count })
                PickerOption(value: DownloadTab::Completed, label: tr! { "zh-Hans" => "已完成 {n}", "en" => "Completed {n}"; n: downloads.completed_count })
            } => downloads.set_tab(downloads.tab)
        }
    }
}
#[component]
pub fn download_row(downloads: downloads::Client, task: downloads::DownloadTask) -> impl View {
    ui! {
        Stack(align: bottom_leading, appear: page_in, disappear: fade) {
            Rail {
                Canvas(from: 0, to: task.progress, height: 46, animate: smooth) {
                    Shape(kind: rect, fill: fill.subtle)
                }
            }
            Column(gap: 0) {
                if !task.first { Divider }
                Row(gap: 12, padding_x: 14, height: 46, align: center) {
                    Stack(width: 30, height: 30) {
                        Canvas {
                            Shape(kind: rounded, corner: 8, fill: fill.strong)
                        }
                        if task.completed {
                            Label(title: tr! { "zh-Hans" => "已完成", "en" => "Completed" }, icon: "checkmark", display: icon)
                        } else {
                            Label(title: tr! { "zh-Hans" => "下载", "en" => "Download" }, icon: "arrow.down", display: icon)
                        }
                    }
                    Column(gap: 1) {
                        Text(content: task.title, font_size: 13, lines: 1)
                        if task.completed {
                            Text(content: tr! { "zh-Hans" => "{size} · 已完成", "en" => "{size} · Completed"; size: task.size }, style: caption, font_size: 11.5, emphasis: secondary)
                        } else if task.running {
                            Text(content: tr! { "zh-Hans" => "{size} · {percent}% · {speed} MB/s", "en" => "{size} · {percent}% · {speed} MB/s"; size: task.size, percent: task.percent, speed: task.speed }, style: caption, font_size: 11.5, emphasis: secondary)
                        } else {
                            Text(content: tr! { "zh-Hans" => "{size} · {percent}% · 已暂停", "en" => "{size} · {percent}% · Paused"; size: task.size, percent: task.percent }, style: caption, font_size: 11.5, emphasis: secondary)
                        }
                    }
                    Spacer
                    if task.completed {
                        Button(id: play, label: tr! { "zh-Hans" => "播放", "en" => "Play" }, icon: "play", shape: capsule, height: 32) => downloads.play(task.id)
                    } else if task.running {
                        IconButton(id: pause, label: tr! { "zh-Hans" => "暂停下载", "en" => "Pause download" }, icon: "pause", prominence: plain) => downloads.toggle(task.id)
                    } else {
                        IconButton(id: resume, label: tr! { "zh-Hans" => "继续下载", "en" => "Resume download" }, icon: "play", prominence: plain) => downloads.toggle(task.id)
                    }
                }
            }
            Rail {
                Canvas(from: 0, to: task.progress, height: 2, animate: smooth) {
                    Shape(kind: rect, fill: accent)
                }
            }
        }
    }
}
