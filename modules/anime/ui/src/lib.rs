//! Interface of the anime module: the home page with its stage and today's
//! timeline, the details of a show, and the week of new shows. Sizes follow
//! the design's desktop prototype (`design/app.html`, Apple, 1280 × 800).

use bern::prelude::*;
use erika_anime_protocol::anime;

/// Tabs below the film of the details page.
#[bern::value]
pub enum DetailTab {
    #[label(tr! { "zh-Hans" => "简介", "en" => "About" })]
    About,
    #[label(tr! { "zh-Hans" => "评论", "en" => "Comments" })]
    Comments,
    #[label(tr! { "zh-Hans" => "制作", "en" => "Staff" })]
    Staff,
    #[label(tr! { "zh-Hans" => "文件", "en" => "Files" })]
    Files,
}

/// Drafts of the anime module's pages.
#[bern::session]
pub struct AnimeDraft {
    /// The episode at the middle of the film.
    #[initial(1)]
    pub episode: i64,
    pub tab: DetailTab,
}

/// The home page: the stage and today's timeline, or the details of the
/// show opened from them.
#[component]
pub fn home_page(anime: anime::Client) -> impl View {
    ui! {
        if anime.opened > 0 && !anime.opened_from_schedule && anime.back_label == "首页" {
            details(anime: anime)
        } else {
            home(anime: anime)
        }
    }
}

#[component]
pub fn home(anime: anime::Client) -> impl View {
    ui! {
        ScrollView(appear: page_in) {
            Column(gap: 0, padding_x: 32, padding_y: 6) {
                Row(gap: 14, align: baseline) {
                    Text(content: tr! { "zh-Hans" => "首页", "en" => "Home" }, style: large_title)
                    Text(content: anime.today, emphasis: secondary)
                    Spacer
                }
                Canvas(height: 18)
                stage(anime: anime)
                Canvas(height: 30)
                timeline(anime: anime)
                Canvas(height: 32)
            }
        }
    }
}

/// "Continue watching": the show on the stage, and the others beside it.
#[component]
pub fn stage(anime: anime::Client) -> impl View {
    ui! {
        Row(gap: 28, align: top) {
            Button(id: stage, label: anime.stage.title, variant: stage) {
                Stack(align: center) {
                    Stack(align: bottom_leading) {
                        Image(source: anime.stage.still, width: 528, height: 297, fit: cover, label: anime.stage.title)
                        Rail {
                            Canvas(from: 0, to: anime.stage.progress, height: 5) {
                                Shape(kind: rect, fill: accent)
                            }
                        }
                    }
                    Label(title: tr! { "zh-Hans" => "播放", "en" => "Play" }, icon: "play.fill", display: icon, style: title2, appearance: dark, reveal: hover)
                }
            } => anime.resume(anime.stage.id)
            Column(gap: 0, width: 420) {
                Canvas(height: 4)
                Text(content: tr! { "zh-Hans" => "继续观看", "en" => "Continue watching" }, style: caption, emphasis: secondary)
                Canvas(height: 6)
                Text(content: anime.stage.title, style: title2, lines: 1)
                Canvas(height: 8)
                Row(gap: 6, align: baseline) {
                    Text(content: anime.stage.episode_label, mark: accent)
                    Text(content: anime.stage.left, emphasis: secondary)
                }
                Canvas(height: 16)
                Button(id: resume, label: tr! { "zh-Hans" => "继续播放", "en" => "Continue" }, icon: "play.fill", prominence: prominent, variant: primary) => anime.resume(anime.stage.id)
                Canvas(height: 24)
                Text(content: tr! { "zh-Hans" => "也在看 · 点一下换上舞台", "en" => "Also watching · click one to put it on the stage" }, style: caption, emphasis: secondary)
                Canvas(height: 10)
                Column(gap: 4) {
                    ForEach(items: anime.also, key: id) |watching| {
                        Stack(align: leading) {
                            Button(id: swap, label: watching.title, variant: row) {
                                Row(gap: 12, align: center) {
                                    Stack(align: bottom_leading) {
                                        Image(source: watching.still, width: 96, height: 54, fit: cover, corner: 8, label: watching.title)
                                        Rail {
                                            Canvas(from: 0, to: watching.progress, height: 3) {
                                                Shape(kind: rect, fill: accent)
                                            }
                                        }
                                    }
                                    Column(gap: 3) {
                                        Text(content: watching.title, style: headline, lines: 1)
                                        Text(content: tr! { "zh-Hans" => "{episode} · {left}", "en" => "{episode} · {left}"; episode: watching.episode_label, left: watching.left }, style: caption, emphasis: secondary)
                                    }
                                    Spacer
                                }
                            } => anime.stage(watching.id)
                            Row(gap: 0, padding: 6, reveal: hover) {
                                Button(id: play_also, label: tr! { "zh-Hans" => "播放", "en" => "Play" }, variant: preview_play) {
                                    Stack(align: center, width: 96, height: 54, appearance: dark) {
                                        Label(title: tr! { "zh-Hans" => "播放", "en" => "Play" }, icon: "play.fill", display: icon, style: title3)
                                    }
                                } => anime.resume(watching.id)
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Today's shows hung on a line at the time they air, with "now" between
/// the aired and the coming.
#[component]
pub fn timeline(anime: anime::Client) -> impl View {
    ui! {
        Column(gap: 0) {
            Row(gap: 10, align: baseline) {
                Text(content: tr! { "zh-Hans" => "今日更新", "en" => "Airing today" }, style: title3)
                Text(content: anime.airing_summary, emphasis: secondary)
                Spacer
                Button(id: schedule, label: tr! { "zh-Hans" => "新番表 ›", "en" => "Schedule ›" }, prominence: plain) => anime.close()
            }
            Canvas(height: 12)
            Stack(align: top_leading) {
                Column(gap: 0) {
                    Canvas(height: 24)
                    Rail {
                        ForEach(items: anime.airing, key: id, stagger: 0.06) |show| {
                            Column(gap: 0, width: 88, at: show.at, appear: timeline_card_in, appear_delay: 0.22) {
                                Button(id: open_airing, label: show.title, variant: poster) {
                                    Image(source: show.poster, width: 88, height: 132, fit: cover, label: show.title)
                                } => anime.open(show.id)
                                Canvas(height: 6)
                                Text(content: show.title, style: caption, lines: 1)
                                Text(content: show.time, style: caption, emphasis: secondary, lines: 1)
                            }
                        }
                    }
                    Canvas(height: 38)
                    Canvas(height: 1, appear: timeline_rule_in, appear_delay: 0.12) {
                        Shape(kind: rect, fill: text.placeholder)
                    }
                    Rail {
                        ForEach(items: anime.hours, key: label) |hour| {
                            Column(gap: 2, at: hour.at) {
                                Canvas(width: 1, height: 5) {
                                    Shape(kind: rect, fill: text.placeholder)
                                }
                                Text(content: hour.label, style: caption, emphasis: tertiary)
                            }
                        }
                    }
                }
                Rail(anchor: start) {
                    // Prototype: 300 ms + seven sample posters × 60 ms;
                    // the marker grows upward from the ruler after the cards.
                    Row(gap: 7, align: top, at: anime.now_at, appear: timeline_now_in, appear_delay: 0.72) {
                        Canvas(width: 2, height: 232) {
                            Shape(kind: capsule, fill: accent)
                        }
                        Text(content: anime.now_label, style: caption2, mark: accent)
                    }
                }
            }
        }
    }
}

/// The details of the opened show: a wash of the poster's color behind
/// the head, the poster, its facts and actions, then the film of episodes.
#[component]
pub fn details(anime: anime::Client, #[session] draft: AnimeDraft) -> impl View {
    ui! {
        ScrollView(appear: page_in) {
            Stack(align: top_leading) {
                Canvas(height: 440, appear: fade) {
                    Shape(kind: rect, fill_from: anime.show.poster, fade: down, opacity: 0.34)
                }
                Column(gap: 0, padding_x: 32, padding_y: 12) {
                    Row(gap: 36, align: top) {
                        Image(source: anime.show.poster, width: 220, height: 330, fit: cover, corner: 10, label: anime.show.title)
                        Column(gap: 0, stagger: 0.03) {
                            Canvas(height: 10)
                            Text(content: anime.show.kind, style: caption, emphasis: secondary, appear: fade)
                            Canvas(height: 8)
                            Text(content: anime.show.title, style: title, lines: 2, appear: fade)
                            Canvas(height: 12)
                            Row(gap: 10, align: center, appear: fade) {
                                Text(content: anime.show.rating, style: callout)
                                Text(content: tr! { "zh-Hans" => "Bangumi", "en" => "Bangumi" }, emphasis: secondary)
                                Row(gap: 6) {
                                    ForEach(items: anime.show.tags, key: name) |tag| {
                                        tag_chip(name: tag.name)
                                    }
                                }
                            }
                            Canvas(height: 14)
                            Column(width: 600, appear: fade) {
                                Text(content: anime.show.summary, emphasis: secondary, lines: 3)
                            }
                            Canvas(height: 20)
                            Row(gap: 10, align: center, appear: fade) {
                                Button(id: watch, label: anime.next_label, icon: "play.fill", prominence: prominent, variant: primary) => anime.play_episode(anime.next_episode)
                                Menu(id: status, label: anime.show.status, icon: "heart") {
                                    MenuItem(id: want, label: tr! { "zh-Hans" => "想看", "en" => "Want to Watch" }) => anime.set_status("想看")
                                    MenuItem(id: watching_now, label: tr! { "zh-Hans" => "在看", "en" => "Watching" }) => anime.set_status("在看")
                                    MenuItem(id: watched, label: tr! { "zh-Hans" => "看过", "en" => "Watched" }) => anime.set_status("看过")
                                }
                                Menu(id: rate, label: tr! { "zh-Hans" => "评分", "en" => "Rate" }, icon: "star", style: icon) {
                                    MenuItem(id: rate_10, label: tr! { "zh-Hans" => "10 分 · 神作", "en" => "10 · Masterpiece" }) => anime.rate(10)
                                    MenuItem(id: rate_8, label: tr! { "zh-Hans" => "8 分 · 力荐", "en" => "8 · Great" }) => anime.rate(8)
                                    MenuItem(id: rate_6, label: tr! { "zh-Hans" => "6 分 · 还行", "en" => "6 · Fine" }) => anime.rate(6)
                                    MenuItem(id: rate_4, label: tr! { "zh-Hans" => "4 分 · 较差", "en" => "4 · Poor" }) => anime.rate(4)
                                    MenuItem(id: rate_2, label: tr! { "zh-Hans" => "2 分 · 不行", "en" => "2 · Bad" }) => anime.rate(2)
                                }
                                Menu(id: more, label: tr! { "zh-Hans" => "更多", "en" => "More" }, icon: "ellipsis", style: icon) {
                                    MenuItem(id: match_danmaku, label: tr! { "zh-Hans" => "匹配弹幕", "en" => "Match Danmaku" }) => anime.close()
                                    MenuItem(id: reveal, label: tr! { "zh-Hans" => "在文件夹中显示", "en" => "Show in Folder" }) => anime.close()
                                    MenuItem(id: refresh, label: tr! { "zh-Hans" => "刷新", "en" => "Refresh" }) => anime.close()
                                    Divider
                                    MenuItem(id: remove, label: tr! { "zh-Hans" => "移除", "en" => "Remove" }, role: destructive) => anime.close()
                                }
                            }
                        }
                    }
                    Canvas(height: 34)
                    Row(gap: 10, align: center, appear: fade) {
                        Text(content: tr! { "zh-Hans" => "剧集", "en" => "Episodes" }, style: title3)
                        Text(content: anime.progress_label, emphasis: secondary)
                        Spacer
                        IconButton(id: film_back, icon: "chevron.left", label: tr! { "zh-Hans" => "上一页", "en" => "Previous" }) => draft.episode.set(1)
                        IconButton(id: film_on, icon: "chevron.right", label: tr! { "zh-Hans" => "下一页", "en" => "Next" }) => draft.episode.set(anime.show.episodes)
                    }
                    Canvas(height: 6)
                    ScrollView(axis: horizontal, snap: center, position: draft.episode) {
                        Row(gap: 30, padding_x: 32, padding_y: 14) {
                            ForEach(items: anime.episodes, key: number) |episode| {
                                film_cell(anime: anime, episode: episode)
                            }
                        }
                    }
                    Canvas(height: 18)
                    Picker(id: detail_tab, label: tr! { "zh-Hans" => "分页", "en" => "Tab" }, value: draft.tab, style: segmented)
                    Canvas(height: 14)
                    Column(width: 640) {
                        Text(content: anime.show.summary, emphasis: secondary)
                    }
                    Canvas(height: 32)
                }
            }
        }
    }
}

/// One cell of the film: the still of an episode reached, or its number;
/// the cell nearest the middle is the largest.
#[component]
pub fn film_cell(anime: anime::Client, episode: anime::Episode) -> impl View {
    ui! {
        Column(gap: 0, width: 220, scroll_scale: 1.12) {
            Button(id: episode_cell, label: episode.title, variant: film) {
                if episode.pictured {
                    Stack(align: bottom_leading) {
                        Image(source: episode.still, width: 220, height: 124, fit: cover, label: episode.title)
                        Rail {
                            Canvas(from: 0, to: episode.progress, height: 4) {
                                Shape(kind: rect, fill: accent)
                            }
                        }
                    }
                } else {
                    Stack(align: center) {
                        Canvas(width: 220, height: 124) {
                            Shape(kind: rect, fill: fill.subtle)
                        }
                        Text(content: episode.numeral, style: large_title, emphasis: tertiary)
                    }
                }
            } => anime.play_episode(episode.number)
            Canvas(height: 8)
            if episode.next {
                Text(content: episode.title, style: headline, mark: accent, lines: 1)
            } else {
                Text(content: episode.title, style: headline, lines: 1)
            }
            Text(content: episode.note, style: caption, emphasis: secondary)
        }
    }
}

/// A tag of a show: words on a soft capsule.
#[component]
pub fn tag_chip(name: String) -> impl View {
    ui! {
        Stack(align: center) {
            Rail {
                Canvas(from: 0, to: 1) {
                    Shape(kind: capsule, fill: fill.subtle)
                }
            }
            Row(gap: 0, padding_x: 9, padding_y: 3) {
                Text(content: name, style: caption, emphasis: secondary)
            }
        }
    }
}

/// The prototype's seven-day accordion. The expanded column uses the
/// available width; the six collapsed columns stay 86 points wide.
#[component]
pub fn new_page(anime: anime::Client) -> impl View {
    ui! {
        if anime.opened > 0 && anime.opened_from_schedule {
            details(anime: anime)
        } else {
            ScrollView(appear: page_in) {
                Column(gap: 0, padding_x: if window.compact { 16 } else if window.medium { 24 } else { 32 }, padding_y: 6) {
                    Row(gap: 14, align: baseline) {
                        Text(content: tr! { "zh-Hans" => "新番", "en" => "New Shows" }, style: large_title)
                        Text(content: tr! { "zh-Hans" => "2026 年 10 月 · 秋季新番 · 第 2 周", "en" => "October 2026 · Fall anime · Week 2" }, emphasis: secondary, lines: 1)
                        Spacer
                    }
                    Canvas(height: 18)
                    if window.compact || window.medium {
                        Column(gap: 8) {
                            ForEach(items: anime.week, key: id) |day| {
                                calendar_vertical(anime: anime, day: day)
                            }
                        }
                    } else {
                        Accordion(count: 7, selected: anime.selected_day, collapsed_width: 86, gap: 8, height: 560, corner: 22, animate: bouncy) {
                            ForEach(items: anime.week, key: id, stagger: 0.036) |day| {
                                Stack(align: top_leading, height: 560, appear: calendar_column_in, appear_delay: 0.06) {
                                    if anime.selected_day == day.id {
                                        Rail {
                                            Column(gap: 0, from: 0, to: 1) {
                                                calendar_background(anime: anime, day: day)
                                            }
                                        }
                                        Column(gap: 0) {
                                            calendar_header(day: day)
                                            Canvas(height: 15)
                                            ScrollView {
                                                calendar_rows(anime: anime, day: day)
                                            }
                                        }
                                    } else {
                                        Button(id: select_day, label: tr! { "zh-Hans" => "展开{day} · {count} 部", "en" => "Expand {day} · {count} shows"; day: day.name, count: day.count }, variant: calendar_day) {
                                            Column(gap: 0, height: 560, align: center) {
                                                calendar_header(day: day)
                                                Canvas(height: 19)
                                                Column(gap: 10, width: 86, align: center) {
                                                    ForEach(items: day.preview, key: id) |show| {
                                                        Column(gap: 4, align: center) {
                                                            Image(source: show.poster, width: 52, height: 78, fit: cover, corner: 10)
                                                            Row(gap: 0, height: 15) {
                                                                Text(content: show.time, style: caption, emphasis: secondary, lines: 1)
                                                            }
                                                        }
                                                    }
                                                    if day.remaining > 0 {
                                                        calendar_more(count: day.remaining)
                                                    }
                                                }
                                                Spacer
                                            }
                                        } => anime.select_day(day.id)
                                    }
                                }
                            }
                        }
                    }
                    Canvas(height: 48)
                }
            }
        }
    }
}

/// The expanded list has its own card background. Collapsed days wrap
/// their visible content in a button so posters and labels remain clickable.
#[component]
pub fn calendar_background(anime: anime::Client, day: anime::BroadcastDay) -> impl View {
    ui! {
        Button(id: selected_day, label: tr! { "zh-Hans" => "{day} · {count} 部", "en" => "{day} · {count} shows"; day: day.name, count: day.count }, variant: calendar_selected) {
            Canvas
        } => anime.select_day(day.id)
    }
}

#[component]
pub fn calendar_header(day: anime::BroadcastDay) -> impl View {
    ui! {
        Column(gap: 0) {
            Canvas(height: 16)
            Row(gap: 0) {
                Canvas(width: 16, height: 39)
                Column(gap: 0) {
                    Row(gap: 0, height: 24) {
                        Text(content: tr! { "zh-Hans" => "{day}", "en" => "{day}"; day: day.name }, style: title3, lines: 1)
                        Spacer
                    }
                    Row(gap: 3, height: 15) {
                        if day.today {
                            Text(content: tr! { "zh-Hans" => "今天", "en" => "Today" }, style: caption, mark: accent, lines: 1)
                            Text(content: tr! { "zh-Hans" => "· {count} 部", "en" => "· {count}"; count: day.count }, style: caption, emphasis: secondary, lines: 1)
                        } else if day.tomorrow {
                            Text(content: tr! { "zh-Hans" => "明天 · {count} 部", "en" => "Tomorrow · {count}"; count: day.count }, style: caption, emphasis: secondary, lines: 1)
                        } else if day.day_after {
                            Text(content: tr! { "zh-Hans" => "后天 · {count} 部", "en" => "In 2 days · {count}"; count: day.count }, style: caption, emphasis: secondary, lines: 1)
                        } else {
                            Text(content: tr! { "zh-Hans" => "{count} 部", "en" => "{count} shows"; count: day.count }, style: caption, emphasis: secondary, lines: 1)
                        }
                        Spacer
                    }
                }
            }
        }
    }
}

#[component]
pub fn calendar_more(count: i64) -> impl View {
    ui! {
        Stack(align: center, width: 30, height: 22) {
            Canvas {
                Shape(kind: capsule, fill: fill.strong)
            }
            Text(content: tr! { "zh-Hans" => "+{count}", "en" => "+{count}"; count: count }, style: caption, emphasis: secondary, lines: 1)
        }
    }
}

#[component]
pub fn calendar_rows(anime: anime::Client, day: anime::BroadcastDay) -> impl View {
    ui! {
        Column(gap: 0, padding_x: 10) {
            ForEach(items: day.shows, key: id, stagger: 0.03) |show| {
                Button(id: open_scheduled, label: tr! { "zh-Hans" => "{time} · {title}", "en" => "{time} · {title}"; time: show.time, title: show.title }, variant: calendar_row, appear: calendar_row_in, appear_delay: 0.09) {
                    Row(gap: 14, padding_x: 6, padding_y: 10, align: center) {
                        Column(width: 44) {
                            if show.aired {
                                Text(content: show.time, style: headline, emphasis: secondary, lines: 1)
                            } else {
                                Text(content: show.time, style: headline, lines: 1)
                            }
                        }
                        Image(source: show.poster, width: 58, height: 87, fit: cover, corner: 10)
                        Column(gap: 0) {
                            Text(content: show.title, style: headline, lines: 1)
                            if show.aired {
                                Text(content: tr! { "zh-Hans" => "第 {episode} 集 · 已更新", "en" => "Episode {episode} · Updated"; episode: show.episode }, style: caption, emphasis: secondary, lines: 1)
                            } else if show.tonight {
                                Text(content: tr! { "zh-Hans" => "今晚更新", "en" => "Airing tonight" }, style: caption, emphasis: secondary, lines: 1)
                            } else {
                                Text(content: tr! { "zh-Hans" => "等待更新", "en" => "Upcoming" }, style: caption, emphasis: secondary, lines: 1)
                            }
                        }
                        Spacer
                    }
                } => anime.open_scheduled(show.id)
            }
            Canvas(height: 16)
        }
    }
}

/// The narrow layout keeps the same broadcasts and actions in a vertical
/// accordion, with the prototype's 28 × 42 cover previews in the header.
#[component]
pub fn calendar_vertical(anime: anime::Client, day: anime::BroadcastDay) -> impl View {
    ui! {
        Stack(align: top_leading, reflow: bouncy) {
            if anime.selected_day == day.id {
                Rail {
                    Column(gap: 0, from: 0, to: 1) {
                        calendar_background(anime: anime, day: day)
                    }
                }
                Column(gap: 0, reflow: bouncy) {
                    calendar_vertical_header(anime: anime, day: day)
                    calendar_rows(anime: anime, day: day)
                }
            } else {
                Button(id: select_day, label: tr! { "zh-Hans" => "展开{day} · {count} 部", "en" => "Expand {day} · {count} shows"; day: day.name, count: day.count }, variant: calendar_day) {
                    calendar_vertical_header(anime: anime, day: day)
                } => anime.select_day(day.id)
            }
        }
    }
}

#[component]
pub fn calendar_vertical_header(anime: anime::Client, day: anime::BroadcastDay) -> impl View {
    ui! {
        Row(gap: 10, padding_x: 14, height: 64) {
            Column(width: 58) {
                Text(content: tr! { "zh-Hans" => "{day}", "en" => "{day}"; day: day.name }, style: title3, lines: 1)
            }
            if day.today {
                Row(gap: 3) {
                    Text(content: tr! { "zh-Hans" => "今天", "en" => "Today" }, style: caption, mark: accent, lines: 1)
                    Text(content: tr! { "zh-Hans" => "· {count} 部", "en" => "· {count}"; count: day.count }, style: caption, emphasis: secondary, lines: 1)
                }
            } else {
                Text(content: tr! { "zh-Hans" => "{count} 部", "en" => "{count} shows"; count: day.count }, style: caption, emphasis: secondary, lines: 1)
            }
            Spacer
            if anime.selected_day != day.id {
                Row(gap: 5) {
                    ForEach(items: day.preview, key: id) |show| {
                        Image(source: show.poster, width: 28, height: 42, fit: cover, corner: 10)
                    }
                    if day.remaining > 0 {
                        calendar_more(count: day.remaining)
                    }
                }
            }
        }
    }
}
