//! Interface of the anime module: the home page with its stage and today's
//! timeline, the details of a show, and the week of new shows. Sizes follow
//! the design's desktop prototype (`design/app.html`, Apple, 1280 × 800).

use bern::prelude::*;
use erika_anime_protocol::anime::{self, BackTo, WatchStatus};

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
        if anime.opened > 0 && !anime.opened_from_schedule && anime.back == BackTo::Home {
            details(anime: anime)
        } else {
            home(anime: anime)
        }
    }
}

#[component]
pub fn home(anime: anime::Client) -> impl View {
    ui! {
        Adaptive(id: home_layout) {
            Layout(size: compact) {
                phone_home(anime: anime)
            }
            Layout(size: expanded) {
                desktop_home(anime: anime)
            }
            // The television shows the desktop's stage, larger (design
            // 6.2 `TV`).
            Layout(size: tv) {
                desktop_home(anime: anime)
            }
        }
    }
}

#[component]
pub fn desktop_home(anime: anime::Client) -> impl View {
    ui! {
        ScrollView(appear: page_in) {
            // A television's page keeps the overscan's 56 points (`.x-page`).
            Column(gap: 0, padding_x: if window.tv { 56 } else { 32 }, padding_y: 6) {
                Row(gap: 14, align: baseline) {
                    Text(content: tr! { "zh-Hans" => "首页", "en" => "Home" }, style: large_title)
                    Text(content: tr! { "zh-Hans" => "{today, date, EEE · M 月 d 日}", "en" => "{today, date, EEE · MMM d}"; today: anime.today }, emphasis: secondary)
                    Spacer
                }
                Canvas(height: 18)
                stage(anime: anime)
                Canvas(height: 30)
                timeline(anime: anime)
                // The other modules' sections, as the library's recently added.
                Slot(id: home_sections)
                Canvas(height: 32)
            }
        }
    }
}

/// "Continue watching": the show on the stage, and the others beside it.
#[component]
pub fn stage(anime: anime::Client) -> impl View {
    ui! {
        // A television's stage is 560 × 315 with 36 points beside it
        // (`.h-stage`, `.h-frame` on the television).
        Row(gap: if window.tv { 36 } else { 28 }, align: top) {
            Button(id: stage, label: anime.stage.title, variant: stage) {
                Stack(align: center) {
                    Stack(align: bottom_leading) {
                        Image(source: anime.stage.still, width: if window.tv { 560 } else { 528 }, height: if window.tv { 315 } else { 297 }, corner: card, fit: cover, label: anime.stage.title, hero: frame, hero_key: anime.stage.title)
                        Rail {
                            Canvas(from: 0, to: anime.stage.progress, height: 5) {
                                Shape(kind: rect, fill: accent)
                            }
                        }
                    }
                    // The television always shows the way to play, on a dark
                    // disc (`.h-hint`).
                    if window.tv {
                        Stack(width: 69, height: 69, align: center, appearance: dark) {
                            Canvas(width: 69, height: 69) {
                                Shape(kind: ellipse, fill: surface.canvas, opacity: 0.45)
                            }
                            Label(title: tr! { "zh-Hans" => "播放", "en" => "Play" }, icon: "play.fill", display: icon, style: title3)
                        }
                    } else {
                        Label(title: tr! { "zh-Hans" => "播放", "en" => "Play" }, icon: "play.fill", display: icon, style: title2, appearance: dark, reveal: hover)
                    }
                }
            } => anime.resume(anime.stage.id)
            // The prototype's `528px 1fr`: this column takes the rest, so the
            // page never asks for more room than the window leaves it.
            Column(gap: 0) {
                Canvas(height: 4)
                Text(content: tr! { "zh-Hans" => "继续观看", "en" => "Continue watching" }, style: caption, emphasis: secondary)
                Canvas(height: 6)
                Text(content: anime.stage.title, style: title2, lines: 1)
                Canvas(height: 8)
                Row(gap: 6, align: baseline) {
                    Text(content: tr! { "zh-Hans" => "第 {episode} 集", "en" => "Episode {episode}"; episode: anime.stage.episode }, mark: accent)
                    Text(content: tr! { "zh-Hans" => "剩 {left} 分钟", "en" => "{left} min left"; left: anime.stage.left }, emphasis: secondary)
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
                                    Stack(align: bottom_leading, variant: thumbnail) {
                                        Image(source: watching.still, width: if window.tv { 120 } else { 96 }, height: if window.tv { 68 } else { 54 }, fit: cover, corner: thumb, label: watching.title, hero: frame, hero_key: watching.title)
                                        Rail {
                                            Canvas(from: 0, to: watching.progress, height: 3) {
                                                Shape(kind: rect, fill: accent)
                                            }
                                        }
                                        // The television's way to play it, a small disc
                                        // in the corner (`.h-pl`).
                                        if window.tv {
                                            Stack(width: 120, height: 68, align: bottom_trailing) {
                                                Column(gap: 0, padding_x: 6, padding_y: 8) {
                                                    Stack(width: 30, height: 30, align: center, appearance: dark) {
                                                        Canvas(width: 30, height: 30) {
                                                            Shape(kind: ellipse, fill: surface.canvas, opacity: 0.55)
                                                        }
                                                        Label(title: tr! { "zh-Hans" => "播放", "en" => "Play" }, icon: "play.fill", display: icon, style: caption)
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Column(gap: 3) {
                                        Text(content: watching.title, style: headline, lines: 1)
                                        Text(content: tr! { "zh-Hans" => "第 {episode} 集 · 剩 {left} 分钟", "en" => "Episode {episode} · {left} min left"; episode: watching.episode, left: watching.left }, style: caption, emphasis: secondary)
                                    }
                                    Spacer
                                }
                            } => anime.stage(watching.id)
                            // A pointer's way to play it at once; the remote
                            // puts it on the stage first.
                            if !window.tv {
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
}

/// Today's shows hung on a line at the time they air, with "now" between
/// the aired and the coming.
#[component]
pub fn timeline(anime: anime::Client) -> impl View {
    ui! {
        Column(gap: 0) {
            Row(gap: 10, align: baseline) {
                Text(content: tr! { "zh-Hans" => "今日更新", "en" => "Airing today" }, style: title3)
                Text(content: tr! { "zh-Hans" => "已更新 {aired} 部 · 今晚还有 {later} 部", "en" => "{aired} aired · {later} more tonight"; aired: anime.aired_count, later: anime.later_count }, emphasis: secondary)
                Spacer
                Button(id: schedule, label: tr! { "zh-Hans" => "新番表 ›", "en" => "Schedule ›" }, prominence: plain, variant: more_link) => pages.new.show()
            }
            Canvas(height: 12)
            timeline_track(anime: anime)
        }
    }
}

/// The track of the timeline: the posters at their times, the ruler with
/// the hours, and the "now" marker across.
#[component]
pub fn timeline_track(anime: anime::Client) -> impl View {
    ui! {
            Stack(align: top_leading) {
                Column(gap: 0) {
                    Canvas(height: 24)
                    Rail(overhang: if window.compact { 0.0 } else { 8.0 }) {
                        ForEach(items: anime.airing, key: id, stagger: 0.06) |show| {
                            Column(gap: 0, width: 88, at: show.at, appear: timeline_card_in, appear_delay: 0.22) {
                                Button(id: open_airing, label: show.title, variant: poster) {
                                    Image(source: show.poster, width: 88, height: 132, fit: cover, label: show.title)
                                } => anime.open(show.id)
                                Canvas(height: 6)
                                Text(content: show.title, style: caption, lines: 1)
                                if show.aired {
                                    Text(content: tr! { "zh-Hans" => "{time, duration, hh:mm} · 已更新", "en" => "{time, duration, hh:mm} · Aired"; time: show.time }, style: caption, emphasis: secondary, lines: 1)
                                } else {
                                    Text(content: tr! { "zh-Hans" => "{time, duration, hh:mm} · 稍后", "en" => "{time, duration, hh:mm} · Later"; time: show.time }, style: caption, emphasis: secondary, lines: 1)
                                }
                            }
                        }
                    }
                    Canvas(height: 38)
                    Canvas(height: 1, appear: timeline_rule_in, appear_delay: 0.12) {
                        Shape(kind: rect, fill: text.placeholder)
                    }
                    Rail(overhang: if window.compact { 0.0 } else { 8.0 }) {
                        ForEach(items: anime.hours, key: time) |hour| {
                            Column(gap: 2, at: hour.at) {
                                Canvas(width: 1, height: 5) {
                                    Shape(kind: rect, fill: text.placeholder)
                                }
                                Text(content: tr! { "zh-Hans" => "{time, duration, hh:mm}", "en" => "{time, duration, hh:mm}"; time: hour.time }, style: caption, emphasis: tertiary)
                            }
                        }
                    }
                }
                Rail(anchor: start, overhang: if window.compact { 0.0 } else { 8.0 }) {
                    // Prototype: 300 ms + seven sample posters × 60 ms;
                    // the marker grows upward from the ruler after the cards.
                    Row(gap: 7, align: top, at: anime.now_at, appear: timeline_now_in, appear_delay: 0.72) {
                        Canvas(width: 2, height: 232) {
                            Shape(kind: capsule, fill: accent)
                        }
                        Text(content: tr! { "zh-Hans" => "现在 {now, duration, hh:mm}", "en" => "Now {now, duration, hh:mm}"; now: anime.now }, style: caption2, mark: accent)
                    }
                }
            }
    }
}

/// The details of the opened show: a wash of the poster's color behind
/// the head, the poster, its facts and actions, then the film of episodes.
#[component]
pub fn details(anime: anime::Client) -> impl View {
    ui! {
        Adaptive(id: details_layout) {
            Layout(size: compact) {
                phone_details(anime: anime)
            }
            Layout(size: expanded) {
                desktop_details(anime: anime)
            }
            Layout(size: tv) {
                desktop_details(anime: anime)
            }
        }
    }
}

#[component]
pub fn desktop_details(anime: anime::Client, #[session] draft: AnimeDraft) -> impl View {
    ui! {
        ScrollView(appear: page_in) {
            Stack(align: top_leading) {
                Canvas(height: 440, appear: fade) {
                    Shape(kind: rect, fill_from: anime.show.poster, fade: down, opacity: 0.34)
                }
                Column(gap: 0, padding_x: if window.tv { 56 } else { 32 }, padding_y: 12) {
                    Row(gap: 36, align: top) {
                        Image(source: anime.show.poster, width: 220, height: 330, fit: cover, corner: poster, label: anime.show.title)
                        Column(gap: 0, stagger: 0.03) {
                            Canvas(height: 10)
                            if anime.show.movie {
                                Text(content: tr! { "zh-Hans" => "剧场版 · {premiere, date, y 年}", "en" => "Film · {premiere, date, y}"; premiere: anime.show.premiere }, style: caption, emphasis: secondary, appear: fade)
                            } else {
                                Text(content: tr! { "zh-Hans" => "TV 动画 · {premiere, date, y 年 M 月} · 共 {episodes} 话", "en" => "TV series · {premiere, date, MMM y} · {episodes, plural, one {# episode} other {# episodes}}"; premiere: anime.show.premiere, episodes: anime.show.episodes }, style: caption, emphasis: secondary, appear: fade)
                            }
                            Canvas(height: 8)
                            Text(content: anime.show.title, style: title, lines: 2, appear: fade)
                            Canvas(height: 12)
                            Row(gap: 10, align: center, appear: fade) {
                                if anime.show.rating > 0.0 {
                                    Text(content: tr! { "zh-Hans" => "★ {rating}", "en" => "★ {rating}"; rating: anime.show.rating }, style: callout)
                                }
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
                                Button(id: watch, label: anime.next, icon: "play.fill", prominence: prominent, variant: primary) => anime.play_episode(anime.next_episode)
                                Menu(id: status, label: anime.show.status, icon: "heart") {
                                    MenuItem(id: want, label: tr! { "zh-Hans" => "想看", "en" => "Want to Watch" }) => anime.set_status(WatchStatus::Want)
                                    MenuItem(id: watching_now, label: tr! { "zh-Hans" => "在看", "en" => "Watching" }) => anime.set_status(WatchStatus::Watching)
                                    MenuItem(id: watched, label: tr! { "zh-Hans" => "看过", "en" => "Watched" }) => anime.set_status(WatchStatus::Watched)
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
                        Text(content: tr! { "zh-Hans" => "共 {episodes} 话 · 看到第 {watched} 话", "en" => "{episodes, plural, one {# episode} other {# episodes}} · watched {watched}"; episodes: anime.show.episodes, watched: anime.show.watched }, emphasis: secondary)
                        Spacer
                        IconButton(id: film_back, icon: "chevron.left", label: tr! { "zh-Hans" => "上一页", "en" => "Previous" }) => draft.episode.set(1)
                        IconButton(id: film_on, icon: "chevron.right", label: tr! { "zh-Hans" => "下一页", "en" => "Next" }) => draft.episode.set(anime.show.episodes)
                    }
                    Canvas(height: 6)
                    // The film pages with the arrows above it, as the design's
                    // does; it shows no scroll bar. `.d-track`: 22 points above
                    // and below the cells.
                    ScrollView(axis: horizontal, snap: center, position: draft.episode, indicators: hidden) {
                        Row(gap: 30, padding_x: 32, padding_y: 22) {
                            ForEach(items: anime.episodes, key: number) |episode| {
                                film_cell(anime: anime, episode: episode)
                            }
                        }
                    }
                    Canvas(height: 18)
                    Picker(id: detail_tab, label: tr! { "zh-Hans" => "分页", "en" => "Tab" }, value: draft.tab, style: segmented, label_hidden: true)
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
        // `.d-ep`: 1.12 at the center, growing from 60 % of its height.
        Column(gap: 0, width: 220, scroll_scale: 1.12, scroll_origin_y: 0.6) {
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
            // `.et` and `.es`: the title's 14 / 19 and the caption's 11.5 / 15.
            Canvas(height: 8)
            if episode.next {
                Text(content: episode.title, style: headline, mark: accent, lines: 1, line_height: 19)
            } else {
                Text(content: episode.title, style: headline, lines: 1, line_height: 19)
            }
            Text(content: episode.note, style: caption, emphasis: secondary, line_height: 15)
        }
    }
}

/// A tag of a show: words on a soft capsule.
#[component]
pub fn tag_chip(#[content] name: String) -> impl View {
    ui! {
        // The pill takes the size of its words; a drawn capsule would grow
        // and squeeze them in a row.
        Stack(align: center, variant: tag_pill) {
            Row(gap: 0, padding_x: 9, padding_y: 3) {
                Text(content: name, style: caption, emphasis: secondary, lines: 1)
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
                Column(gap: 0) {
                if window.compact {
                    Slot(id: page_header)
                }
                // On the phone the title and the season are the header's
                // (`.x-pnav` and the line under it), 14 points above the days.
                Column(gap: 0, padding_x: if window.tv { 56 } else if window.compact { 16 } else if window.medium { 24 } else { 32 }, padding_y: if window.compact { 0 } else { 6 }) {
                    if window.compact {
                        Canvas(height: 14)
                    } else {
                        Row(gap: 14, align: baseline) {
                            Text(content: tr! { "zh-Hans" => "新番", "en" => "New Shows" }, style: large_title)
                            Text(content: tr! { "zh-Hans" => "2026 年 10 月 · 秋季新番 · 第 2 周", "en" => "October 2026 · Fall anime · Week 2" }, emphasis: secondary, lines: 1)
                            Spacer
                        }
                        Canvas(height: 18)
                    }
                    if window.compact || window.medium {
                        Column(gap: 8) {
                            ForEach(items: anime.week, key: id) |day| {
                                calendar_vertical(anime: anime, day: day)
                            }
                        }
                    } else {
                        // A television's week is 500 points tall (`.n-acc`).
                        Accordion(count: 7, selected: anime.selected_day, collapsed_width: 86, gap: 8, height: if window.tv { 500 } else { 560 }, corner: card, animate: bouncy) {
                            ForEach(items: anime.week, key: id, stagger: 0.036) |day| {
                                Stack(align: top_leading, height: if window.tv { 500 } else { 560 }, appear: calendar_column_in, appear_delay: 0.06) {
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
                                        Button(id: select_day, label: tr! { "zh-Hans" => "展开{day, date, EEE} · {count} 部", "en" => "Expand {day, date, EEE} · {count, plural, one {# show} other {# shows}}"; day: day.date, count: day.count }, variant: calendar_day) {
                                            Column(gap: 0, height: if window.tv { 500 } else { 560 }, align: center) {
                                                calendar_header_collapsed(day: day)
                                                Canvas(height: 19)
                                                Column(gap: 10, width: 86, align: center) {
                                                    ForEach(items: day.preview, key: id) |show| {
                                                        Column(gap: 4, align: center) {
                                                            Image(source: show.poster, width: 52, height: 78, fit: cover, corner: poster)
                                                            Row(gap: 0, height: 15) {
                                                                Text(content: tr! { "zh-Hans" => "{time, duration, hh:mm}", "en" => "{time, duration, hh:mm}"; time: show.time }, style: caption, emphasis: secondary, lines: 1)
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
}

/// The expanded list has its own card background. Collapsed days wrap
/// their visible content in a button so posters and labels remain clickable.
#[component]
pub fn calendar_background(anime: anime::Client, day: anime::BroadcastDay) -> impl View {
    ui! {
        Button(id: selected_day, label: tr! { "zh-Hans" => "{day, date, EEE} · {count} 部", "en" => "{day, date, EEE} · {count, plural, one {# show} other {# shows}}"; day: day.date, count: day.count }, variant: calendar_selected) {
            Canvas
        } => anime.select_day(day.id)
    }
}

/// The heading of a folded day: the same words, stacked in the column's
/// 86 points, 16 from its edge.
#[component]
pub fn calendar_header_collapsed(day: anime::BroadcastDay) -> impl View {
    ui! {
        Column(gap: 0, width: 86) {
            Canvas(height: 16)
            Column(gap: 0, padding_x: 16) {
            Text(content: tr! { "zh-Hans" => "{day, date, EEE}", "en" => "{day, date, EEE}"; day: day.date }, style: title3, lines: 1)
            if day.today {
                Text(content: tr! { "zh-Hans" => "今天 · {count} 部", "en" => "Today · {count}"; count: day.count }, style: caption, emphasis: secondary, lines: 1)
            } else if day.tomorrow {
                Text(content: tr! { "zh-Hans" => "明天 · {count} 部", "en" => "Tomorrow · {count}"; count: day.count }, style: caption, emphasis: secondary, lines: 1)
            } else if day.day_after {
                Text(content: tr! { "zh-Hans" => "后天 · {count} 部", "en" => "In 2 days · {count}"; count: day.count }, style: caption, emphasis: secondary, lines: 1)
            } else {
                Text(content: tr! { "zh-Hans" => "{count} 部", "en" => "{count} shows"; count: day.count }, style: caption, emphasis: secondary, lines: 1)
            }
            }
        }
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
                    // A television's heading and line are larger (`--t-h2`,
                    // `--t-cap`).
                    Row(gap: 0, height: if window.tv { 28 } else { 24 }) {
                        Text(content: tr! { "zh-Hans" => "{day, date, EEE}", "en" => "{day, date, EEE}"; day: day.date }, style: title3, lines: 1)
                        Spacer
                    }
                    Row(gap: 3, height: if window.tv { 18 } else { 15 }) {
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
        // `.n-list` (16 from the column's edges) or the phone's `.n-vbody`
        // (12, and 10 below the last).
        Column(gap: 0, padding_x: if window.compact { 6 } else { 10 }) {
            ForEach(items: day.shows, key: id, stagger: 0.03) |show| {
                Button(id: open_scheduled, label: tr! { "zh-Hans" => "{time, duration, hh:mm} · {title}", "en" => "{time, duration, hh:mm} · {title}"; time: show.time, title: show.title }, variant: calendar_row, appear: calendar_row_in, appear_delay: 0.09) {
                    Row(gap: 14, padding_x: 6, padding_y: 10, align: center) {
                        // `.n-row`: the time and the title at 14 / 19, the
                        // note at 11.5 / 15.
                        // A television's times are 17 points (`--t-title`).
                        Column(width: if window.tv { 52 } else { 44 }) {
                            if show.aired {
                                Text(content: tr! { "zh-Hans" => "{time, duration, hh:mm}", "en" => "{time, duration, hh:mm}"; time: show.time }, style: headline, emphasis: secondary, lines: 1, line_height: if window.tv { 22 } else { 19 })
                            } else {
                                Text(content: tr! { "zh-Hans" => "{time, duration, hh:mm}", "en" => "{time, duration, hh:mm}"; time: show.time }, style: headline, lines: 1, line_height: if window.tv { 22 } else { 19 })
                            }
                        }
                        Image(source: show.poster, width: 58, height: 87, fit: cover, corner: poster)
                        Column(gap: 0) {
                            Text(content: show.title, style: headline, lines: 1, line_height: if window.tv { 22 } else { 19 })
                            if show.aired {
                                Text(content: tr! { "zh-Hans" => "第 {episode} 集 · 已更新", "en" => "Episode {episode} · Updated"; episode: show.episode }, style: caption, emphasis: secondary, lines: 1, line_height: if window.tv { 18 } else { 15 })
                            } else if show.tonight {
                                Text(content: tr! { "zh-Hans" => "今晚更新", "en" => "Airing tonight" }, style: caption, emphasis: secondary, lines: 1, line_height: if window.tv { 18 } else { 15 })
                            } else {
                                Text(content: tr! { "zh-Hans" => "等待更新", "en" => "Upcoming" }, style: caption, emphasis: secondary, lines: 1, line_height: if window.tv { 18 } else { 15 })
                            }
                        }
                        Spacer
                    }
                } => anime.open_scheduled(show.id)
            }
            Canvas(height: if window.compact { 10 } else { 16 })
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
                Button(id: select_day, label: tr! { "zh-Hans" => "展开{day, date, EEE} · {count} 部", "en" => "Expand {day, date, EEE} · {count, plural, one {# show} other {# shows}}"; day: day.date, count: day.count }, variant: calendar_day) {
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
                Text(content: tr! { "zh-Hans" => "{day, date, EEE}", "en" => "{day, date, EEE}"; day: day.date }, style: title3, lines: 1)
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
                        Image(source: show.poster, width: 28, height: 42, fit: cover, corner: poster)
                    }
                    if day.remaining > 0 {
                        calendar_more(count: day.remaining)
                    }
                }
            }
        }
    }
}

/// The phone's home page, measured from the prototype at 390 × 844
/// (`phHTML('home')`, next.css `.ph-stage`, `.ph-cards`, `.ph-tlw`): the
/// stage across the width, what it is with the main button, the others
/// being watched as cards, and today's timeline, each 16 from the edges.
#[component]
pub fn phone_home(anime: anime::Client) -> impl View {
    ui! {
        ScrollView(appear: page_in) {
            Column(gap: 0) {
                // The shell's header row scrolls with the page.
                Slot(id: page_header)
                Column(gap: 0, padding_x: 16) {
                    phone_stage(anime: anime)
                    Canvas(height: 14.5)
                    Text(content: tr! { "zh-Hans" => "继续观看", "en" => "Continue watching" }, style: caption, emphasis: secondary)
                    Canvas(height: 11)
                    Text(content: anime.stage.title, style: title3, font_size: 20, lines: 1)
                    Canvas(height: 4)
                    Row(gap: 0, align: baseline) {
                        Text(content: tr! { "zh-Hans" => "第 {episode} 集", "en" => "Episode {episode}"; episode: anime.stage.episode }, font_size: 13, mark: accent)
                        Text(content: tr! { "zh-Hans" => " · 剩 {left} 分钟", "en" => " · {left} min left"; left: anime.stage.left }, font_size: 13, emphasis: secondary)
                    }
                    Canvas(height: 15.7)
                    Button(id: phone_resume, label: tr! { "zh-Hans" => "继续播放", "en" => "Continue" }, icon: "play.fill", prominence: prominent, shape: capsule, height: 42, width: fill, variant: phone_main) => anime.resume(anime.stage.id)
                    Canvas(height: 30)
                    Row(gap: 10, align: baseline) {
                        Text(content: tr! { "zh-Hans" => "也在看", "en" => "Also watching" }, style: title3)
                        Text(content: tr! { "zh-Hans" => "点一下换上舞台", "en" => "Tap one to put it on the stage" }, style: caption, emphasis: secondary)
                        Spacer
                    }
                    Canvas(height: 12)
                }
                // `.x-hrow`: 16 from the left edge, which it stops at, and
                // running under the right one.
                Row(gap: 0, align: top) {
                    Canvas(width: 16)
                    ScrollView(axis: horizontal, indicators: hidden) {
                        Row(gap: 0, align: top) {
                            Row(gap: 12, align: top) {
                                ForEach(items: anime.also, key: id) |watching| {
                                    phone_card(anime: anime, watching: watching)
                                }
                            }
                            Canvas(width: 16)
                        }
                    }
                }
                Column(gap: 0, padding_x: 16) {
                    Canvas(height: 32.7)
                    Row(gap: 10, align: baseline) {
                        Text(content: tr! { "zh-Hans" => "今日更新", "en" => "Airing today" }, style: title3)
                        Spacer
                        Button(id: phone_schedule, label: tr! { "zh-Hans" => "新番表", "en" => "Schedule" }) {
                            Text(content: tr! { "zh-Hans" => "新番表 ›", "en" => "Schedule ›" }, font_size: 13, emphasis: secondary)
                        } => pages.new.show()
                    }
                    Canvas(height: 12)
                }
                // The timeline opens with 22:00, the middle of its 900
                // points, in the middle of the view: `scrollLeft = 450 −
                // 374 / 2`, 263 of the 916 − 374 it scrolls.
                Row(gap: 0, align: top) {
                    Canvas(width: 16)
                    ScrollView(axis: horizontal, indicators: hidden, open_at: 0.4852398524) {
                        Row(gap: 0) {
                            Column(gap: 0, width: 900) {
                                timeline_track(anime: anime)
                            }
                            Canvas(width: 16)
                        }
                    }
                }
                // The other modules' sections, as the library's recently added.
                Slot(id: home_sections)
                Canvas(height: 24)
            }
        }
    }
}

/// The details' wash of the poster's color (`.d-wash`, 440 points from
/// under the status bar), behind the header row and the head.
#[component]
pub fn details_wash(anime: anime::Client) -> impl View {
    ui! {
        Canvas(height: 435, appear: fade) {
            Shape(kind: rect, fill_from: anime.show.poster, fade: down, opacity: 0.34)
        }
    }
}

/// `.ph-stage .h-frame`: 16 : 9 across the width, the card corner, the
/// progress along its foot (5 points over a 35 % black track) and the
/// play hint always shown (64 points, 45 % black), since touch has no hover.
#[component]
pub fn phone_stage(anime: anime::Client) -> impl View {
    ui! {
        Button(id: phone_stage, label: anime.stage.title, variant: stage) {
            Stack(align: center) {
                Stack(align: bottom_leading) {
                    Image(source: anime.stage.still, aspect: 1.7777777778, corner: card, fit: cover, label: anime.stage.title, hero: frame, hero_key: anime.stage.title)
                    Stack(appearance: dark) {
                        Canvas(height: 5) {
                            Shape(kind: rect, fill: surface.canvas, opacity: 0.35)
                        }
                    }
                    Rail {
                        Canvas(from: 0, to: anime.stage.progress, height: 5) {
                            Shape(kind: rect, fill: accent)
                        }
                    }
                }
                Stack(width: 64, height: 64, appearance: dark) {
                    Canvas(width: 64, height: 64) {
                        Shape(kind: ellipse, fill: surface.canvas, opacity: 0.45)
                    }
                    Label(title: tr! { "zh-Hans" => "播放", "en" => "Play" }, icon: "play.fill", display: icon, font_size: 28)
                }
            }
        } => anime.resume(anime.stage.id)
    }
}

/// One card of `.ph-cards`: a 168 × 94.5 thumbnail (12-point corners,
/// 4-point progress, a 30-point play button 8 and 10 from the corner),
/// then the title (13 / 19, semibold) and how far it is.
#[component]
pub fn phone_card(anime: anime::Client, watching: anime::Watching) -> impl View {
    ui! {
        Stack(align: top_leading) {
            Button(id: phone_swap, label: watching.title, variant: phone_card) {
                Column(gap: 0, width: 168) {
                    Stack(align: bottom_leading, variant: phone_thumb) {
                        Image(source: watching.still, width: 168, height: 94.5, fit: cover, corner: 12, label: watching.title, hero: frame, hero_key: watching.title)
                        Stack(appearance: dark) {
                            Canvas(height: 4) {
                                Shape(kind: rect, fill: surface.canvas, opacity: 0.35)
                            }
                        }
                        Rail {
                            Canvas(from: 0, to: watching.progress, height: 4) {
                                Shape(kind: rect, fill: accent)
                            }
                        }
                    }
                    Canvas(height: 8.6)
                    Text(content: watching.title, style: headline, font_size: 13, lines: 1)
                    Canvas(height: 4)
                    Text(content: tr! { "zh-Hans" => "第 {episode} 集 · 剩 {left} 分钟", "en" => "Episode {episode} · {left} min left"; episode: watching.episode, left: watching.left }, style: caption, emphasis: secondary, lines: 1)
                }
            } => anime.stage(watching.id)
            Stack(align: bottom_trailing, width: 168, height: 94.5) {
                Column(gap: 0, align: trailing) {
                    Row(gap: 0) {
                        Button(id: phone_play_also, label: tr! { "zh-Hans" => "直接播放", "en" => "Play" }, variant: preview_play) {
                            Stack(width: 30, height: 30, appearance: dark) {
                                Canvas(width: 30, height: 30) {
                                    Shape(kind: ellipse, fill: surface.canvas, opacity: 0.55)
                                }
                                Label(title: tr! { "zh-Hans" => "直接播放", "en" => "Play" }, icon: "play.fill", display: icon, font_size: 15)
                            }
                        } => anime.resume(watching.id)
                        Canvas(width: 8, height: 1)
                    }
                    Canvas(width: 1, height: 10)
                }
            }
        }
    }
}

/// The phone's details (`detailHTML(…, true)`, next.css `.d-head`,
/// `.d-acts`, `.d-strip`): the poster 118 × 177 beside its facts, the main
/// button across, the status, rating and more, the synopsis, then the
/// film, which is the desktop's. The wash of the poster's color lies
/// behind the shell's header row too, which the page holds at its top.
#[component]
pub fn phone_details(anime: anime::Client, #[session] draft: AnimeDraft) -> impl View {
    ui! {
        ScrollView(appear: page_in) {
            // `.d-wash` behind the header row and the head, scrolling with
            // them.
            Stack(align: top_leading) {
            details_wash(anime: anime)
            Column(gap: 0) {
                Slot(id: page_header)
                Column(gap: 0, padding_x: 16) {
                    Canvas(height: 8)
                    Row(gap: 16, align: top) {
                        Image(source: anime.show.poster, width: 118, height: 177, fit: cover, corner: poster, label: anime.show.title)
                        // `.d-info`: the 224 points beside the poster.
                        Column(gap: 0, width: 224) {
                            Canvas(height: 4)
                            if anime.show.movie {
                                Text(content: tr! { "zh-Hans" => "剧场版 · {premiere, date, y 年}", "en" => "Film · {premiere, date, y}"; premiere: anime.show.premiere }, style: caption, emphasis: secondary, lines: 1)
                            } else {
                                Text(content: tr! { "zh-Hans" => "TV 动画 · {premiere, date, y 年 M 月} · 共 {episodes} 话", "en" => "TV series · {premiere, date, MMM y} · {episodes, plural, one {# episode} other {# episodes}}"; premiere: anime.show.premiere, episodes: anime.show.episodes }, style: caption, emphasis: secondary, lines: 1)
                            }
                            Canvas(height: 6)
                            Text(content: anime.show.title, style: title3, font_size: 21, lines: 2)
                            Canvas(height: 8)
                            Row(gap: 10, align: center) {
                                if anime.show.rating > 0.0 {
                                    Text(content: tr! { "zh-Hans" => "★ {rating}", "en" => "★ {rating}"; rating: anime.show.rating }, style: headline, font_size: 16)
                                }
                                Text(content: tr! { "zh-Hans" => "Bangumi", "en" => "Bangumi" }, font_size: 13, emphasis: secondary)
                                Row(gap: 6) {
                                    ForEach(items: anime.show.tags, key: name) |tag| {
                                        tag_chip(name: tag.name)
                                    }
                                }
                            }
                        }
                    }
                    Canvas(height: 16)
                    Button(id: phone_watch, label: anime.next, icon: "play.fill", prominence: prominent, shape: capsule, height: 42, width: fill, variant: phone_main) => anime.play_episode(anime.next_episode)
                    Canvas(height: 11)
                    Row(gap: 10, align: center) {
                        Menu(id: phone_status, label: anime.show.status, icon: "heart", shape: capsule, height: 32) {
                            MenuItem(id: want, label: tr! { "zh-Hans" => "想看", "en" => "Want to Watch" }) => anime.set_status(WatchStatus::Want)
                            MenuItem(id: watching_now, label: tr! { "zh-Hans" => "在看", "en" => "Watching" }) => anime.set_status(WatchStatus::Watching)
                            MenuItem(id: watched, label: tr! { "zh-Hans" => "看过", "en" => "Watched" }) => anime.set_status(WatchStatus::Watched)
                        }
                        Menu(id: phone_rate, label: tr! { "zh-Hans" => "评分", "en" => "Rate" }, icon: "star", style: icon, height: 34) {
                            MenuItem(id: rate_10, label: tr! { "zh-Hans" => "10 分 · 神作", "en" => "10 · Masterpiece" }) => anime.rate(10)
                            MenuItem(id: rate_8, label: tr! { "zh-Hans" => "8 分 · 力荐", "en" => "8 · Great" }) => anime.rate(8)
                            MenuItem(id: rate_6, label: tr! { "zh-Hans" => "6 分 · 还行", "en" => "6 · Fine" }) => anime.rate(6)
                            MenuItem(id: rate_4, label: tr! { "zh-Hans" => "4 分 · 较差", "en" => "4 · Poor" }) => anime.rate(4)
                            MenuItem(id: rate_2, label: tr! { "zh-Hans" => "2 分 · 不行", "en" => "2 · Bad" }) => anime.rate(2)
                        }
                        Menu(id: phone_more, label: tr! { "zh-Hans" => "更多", "en" => "More" }, icon: "ellipsis", style: icon, height: 34) {
                            MenuItem(id: match_danmaku, label: tr! { "zh-Hans" => "匹配弹幕", "en" => "Match Danmaku" }) => anime.close()
                            MenuItem(id: refresh, label: tr! { "zh-Hans" => "刷新", "en" => "Refresh" }) => anime.close()
                            Divider
                            MenuItem(id: remove, label: tr! { "zh-Hans" => "移除", "en" => "Remove" }, role: destructive) => anime.close()
                        }
                        Spacer
                    }
                    Canvas(height: 14)
                    Text(content: anime.show.summary, font_size: 13, line_height: 18, emphasis: secondary, lines: 3)
                    Canvas(height: 34)
                    Row(gap: 10, align: baseline) {
                        Text(content: tr! { "zh-Hans" => "剧集", "en" => "Episodes" }, style: title3)
                        Text(content: tr! { "zh-Hans" => "共 {episodes} 话 · 看到第 {watched} 话", "en" => "{episodes, plural, one {# episode} other {# episodes}} · watched {watched}"; episodes: anime.show.episodes, watched: anime.show.watched }, font_size: 13, emphasis: secondary)
                        Spacer
                    }
                    Canvas(height: 6)
                }
                // `.d-strip`: 21.5 points above and below the cells.
                ScrollView(axis: horizontal, snap: center, position: draft.episode, indicators: hidden) {
                    Row(gap: 30, padding_x: 16, padding_y: 21.5) {
                        ForEach(items: anime.episodes, key: number) |episode| {
                            film_cell(anime: anime, episode: episode)
                        }
                    }
                }
                Column(gap: 0, padding_x: 16) {
                    Canvas(height: 18)
                    Row(gap: 0) {
                        // `.c-seg`: 212 points; iOS's segments would fill the row.
                        Row(gap: 0, width: 212) {
                            Picker(id: phone_detail_tab, label: tr! { "zh-Hans" => "分页", "en" => "Tab" }, value: draft.tab, style: segmented, label_hidden: true)
                        }
                        Spacer
                    }
                    Canvas(height: 14)
                    Text(content: anime.show.summary, font_size: 13, line_height: 18, emphasis: secondary)
                    Canvas(height: 32)
                }
            }
            }
        }
    }
}
