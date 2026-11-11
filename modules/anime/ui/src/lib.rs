//! Interface of the anime module: the home page with its stage and today's
//! timeline, the details of a show, and the week of new shows. Sizes follow
//! the design's desktop prototype (`design/app.html`, Apple, 1280 × 800).

use bern::prelude::*;
use erika_anime_protocol::anime::{self, BackTo, Score, WatchStatus};
use erika_protocol::erika;
mod provider_editor;
use provider_editor::*;

/// Tabs below the film of the details page.
#[bern::value]
pub enum DetailTab {
    #[label(tr! { "zh-Hans" => "简介", "en" => "About" })]
    About,
    #[label(tr! { "zh-Hans" => "评论", "en" => "Comments" })]
    Comments,
}

/// Drafts of the anime module's pages.
#[bern::session]
pub struct AnimeDraft {
    /// Canonical UUID at the leading edge of the film.
    pub episode: String,
    pub tab: DetailTab,
    /// The review being written, started from the one on Bangumi.
    pub review: String,
}

/// Mounted everywhere: the clock that moves "now" along today's timeline
/// and turns the schedule over to the next broadcast day.
#[component]
pub fn anime_clock(anime: anime::Client) -> impl View {
    ui! {
        Stack {
            Timer(interval: 10) => anime.tick()
        }
    }
}

/// The home page: the stage and today's timeline, or the details of the
/// show opened from them.
#[component]
pub fn home_page(app: erika::Client, anime: anime::Client) -> impl View {
    ui! {
        // A foldable open flat shows the details in its right half; a
        // window with a sidebar covers it with them, leaving this page as it
        // is underneath.
        if anime.opened != "" && !anime.opened_from_schedule && anime.back == BackTo::Home && !window.unfolded && (window.compact || window.tv) {
            details(app: app, anime: anime)
        } else {
            home(app: app, anime: anime)
        }
    }
}

#[component]
pub fn home(app: erika::Client, anime: anime::Client) -> impl View {
    ui! {
        // By the window, as the shell chooses: beside a sidebar the page is
        // the desktop's however little room the sidebar leaves it. The
        // television shows the desktop's stage, larger (design 6.2 `TV`).
        if window.compact {
            phone_home(app: app, anime: anime)
        } else {
            desktop_home(app: app, anime: anime)
        }
    }
}

#[component]
pub fn desktop_home(app: erika::Client, anime: anime::Client) -> impl View {
    ui! {
        ScrollView(appear: page_in) {
            // A television's page keeps the overscan's 56 points (`.x-page`).
            Column(gap: 0, padding_x: if window.tv { 56 } else if window.medium { 24 } else { 32 }, padding_y: 6) {
                Row(gap: 14, align: baseline) {
                    Text(content: tr! { "zh-Hans" => "首页", "en" => "Home" }, style: large_title)
                    Text(content: tr! { "zh-Hans" => "{today, date, EEE · M 月 d 日}", "en" => "{today, date, EEE · MMM d}"; today: anime.today }, emphasis: secondary)
                    Spacer
                }
                Canvas(height: 18)
                if anime.has_stage {
                    stage(anime: anime)
                    Canvas(height: 30)
                }
                timeline(app: app, anime: anime)
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
        // A tablet held upright has no room beside the frame.
        if window.medium {
            Column(gap: 16) {
                phone_stage(anime: anime)
                stage_side(anime: anime)
            }
        } else {
            // A television's stage is 560 × 315 with 36 points beside it
            // (`.h-stage`, `.h-frame` on the television).
            Row(gap: if window.tv { 36 } else { 28 }, align: top) {
                stage_frame(anime: anime)
                stage_side(anime: anime)
            }
        }
    }
}

/// The frame of what was watched last, with its way to go on: what a
/// foldable's right half shows while nothing is open there.
#[component]
pub fn stage_column(anime: anime::Client) -> impl View {
    ui! {
        ScrollView(appear: page_in) {
            Column(gap: 16, padding_x: 16, padding_y: 16) {
                if anime.has_stage {
                    phone_stage(anime: anime)
                    stage_side(anime: anime)
                }
            }
        }
    }
}

#[component]
pub fn stage_frame(anime: anime::Client) -> impl View {
    ui! {
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
    }
}

#[component]
pub fn stage_side(anime: anime::Client) -> impl View {
    ui! {
            // The prototype's `528px 1fr`: this column takes the rest, so the
            // page never asks for more room than the window leaves it.
            Column(gap: 0) {
                Canvas(height: 4)
                Text(content: tr! { "zh-Hans" => "继续观看", "en" => "Continue watching" }, style: caption, emphasis: secondary)
                Canvas(height: 6)
                Text(content: anime.stage.title, style: title2, lines: 1)
                Canvas(height: 8)
                Row(gap: 6, align: baseline) {
                    if anime.stage.chapter != "" {
                        Text(content: anime.stage.chapter, mark: accent)
                    }
                    Text(content: tr! { "zh-Hans" => "剩 {left} 分钟", "en" => "{left} min left"; left: anime.stage.left }, emphasis: secondary)
                }
                Canvas(height: 16)
                Button(id: resume, label: tr! { "zh-Hans" => "继续播放", "en" => "Continue" }, icon: "play.fill", prominence: prominent, variant: primary) => anime.resume(anime.stage.id)
                Canvas(height: 24)
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
                                        Row(gap: 6) {
                                            if watching.chapter != "" {
                                                Text(content: watching.chapter, style: caption, emphasis: secondary)
                                            }
                                            Text(content: tr! { "zh-Hans" => "剩 {left} 分钟", "en" => "{left} min left"; left: watching.left }, style: caption, emphasis: secondary)
                                        }
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

/// Today's shows hung on a line at the time they air, with "now" between
/// the aired and the coming.
#[component]
pub fn timeline(app: erika::Client, anime: anime::Client) -> impl View {
    ui! {
        Column(gap: 0) {
            Row(gap: 10, align: baseline) {
                Text(content: tr! { "zh-Hans" => "今日更新", "en" => "Airing today" }, style: title3)
                if anime.has_airing {
                    Text(content: tr! { "zh-Hans" => "已更新 {aired} 部 · 今晚还有 {later} 部", "en" => "{aired} aired · {later} more tonight"; aired: anime.aired_count, later: anime.later_count }, emphasis: secondary)
                }
                Spacer
                Button(id: schedule, label: tr! { "zh-Hans" => "新番表 ›", "en" => "Schedule ›" }, prominence: plain, variant: more_link) => pages.new.show()
            }
            Canvas(height: 12)
            if anime.has_airing {
                // A track longer than the page scrolls; it opens with "now"
                // in the middle, or as near it as the track reaches.
                ScrollView(axis: horizontal, indicators: mouse, open_center: anime.now_at) {
                    Column(gap: 0, width: if window.large { anime.track_wide } else { anime.track_width }) {
                        timeline_track(anime: anime)
                    }
                }
            } else {
                season_note(app: app, anime: anime)
            }
        }
    }
}

/// What the timeline or the schedule says when it has nothing to show:
/// that an account is needed, that the season is loading, why it failed,
/// or that nothing airs today at a known time.
#[component]
pub fn season_note(app: erika::Client, anime: anime::Client) -> impl View {
    ui! {
        Column(gap: 4, padding_y: 10) {
            if anime.loading {
                Text(content: tr! { "zh-Hans" => "正在载入这一季的新番…", "en" => "Loading this season's shows…" }, emphasis: secondary)
            } else {
                Text(content: tr! { "zh-Hans" => "今天没有知道播出时刻的番；新番表按星期列着这一季的全部。", "en" => "Nothing airs today at a known time; the schedule lists the whole season by day." }, emphasis: secondary)
                Text(content: anime.error, style: caption, emphasis: tertiary)
            }
        }
    }
}

/// The track of the timeline: the shows of each time side by side above
/// the ruler, a stem down to that time under them, breaks where an empty
/// stretch was shrunk, and "now" between the aired and the coming. The engine places
/// every group as a fraction of the track and gives its length; the caller
/// sizes the track and scrolls it.
#[component]
pub fn timeline_track(anime: anime::Client) -> impl View {
    ui! {
            Stack(align: top_leading) {
                Column(gap: 0) {
                    Canvas(height: 24)
                    Rail(anchor: start) {
                        ForEach(items: anime.airing, key: time, stagger: 0.06) |group| {
                            Row(gap: 8, align: top, at: group.at, appear: timeline_card_in, appear_delay: 0.22) {
                                ForEach(items: group.shows, key: id) |show| {
                                    Column(gap: 0, width: 88) {
                                        Button(id: open_airing, label: show.title, variant: poster) {
                                            Image(source: show.poster, width: 88, height: 132, fit: cover, label: show.title)
                                        } => anime.open(show.id)
                                        Canvas(height: 6)
                                        Text(content: show.title, style: caption, lines: 1)
                                        if show.aired {
                                            Text(content: tr! { "zh-Hans" => "已更新", "en" => "Aired" }, style: caption, emphasis: secondary, lines: 1)
                                        } else {
                                            Text(content: tr! { "zh-Hans" => "稍后", "en" => "Later" }, style: caption, emphasis: secondary, lines: 1)
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // A stem from each time's posters down to its dot on the
                    // ruler, the dot hollow until they air; a bracket first
                    // joins the posters of a time.
                    Rail(anchor: start) {
                        ForEach(items: anime.airing, key: time) |group| {
                            Column(gap: 0, width: group.width, align: center, at: group.at) {
                                Canvas(height: 8)
                                Canvas(width: group.span, height: 1) {
                                    Shape(kind: rect, fill: text.placeholder)
                                }
                                Canvas(width: 1, height: 22) {
                                    Shape(kind: rect, fill: text.placeholder)
                                }
                                if group.aired {
                                    Canvas(width: 7, height: 7) {
                                        Shape(kind: ellipse, fill: text.secondary)
                                    }
                                } else {
                                    Canvas(width: 7, height: 7) {
                                        Shape(kind: ellipse, x: 0.75, y: 0.75, width: 5.5, height: 5.5, stroke: text.placeholder, stroke_width: 1.5)
                                    }
                                }
                            }
                        }
                    }
                    Canvas(height: 1, appear: timeline_rule_in, appear_delay: 0.12) {
                        Shape(kind: rect, fill: text.placeholder)
                    }
                    Rail(anchor: start) {
                        ForEach(items: anime.airing, key: time) |group| {
                            Column(gap: 0, width: group.width, align: center, at: group.at) {
                                Canvas(height: 7)
                                Text(content: tr! { "zh-Hans" => "{time, duration, hh:mm}", "en" => "{time, duration, hh:mm}"; time: group.time }, style: caption, emphasis: secondary)
                            }
                        }
                        ForEach(items: anime.breaks, key: after) |gap| {
                            Text(content: tr! { "zh-Hans" => "⋯", "en" => "⋯" }, style: caption, emphasis: tertiary, at: gap.at)
                        }
                    }
                }
                Rail(anchor: start) {
                    // The marker grows upward from the ruler after the cards.
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

/// The opened show: a viewport-sized backdrop behind independently
/// scrolling facts, actions and episodes.
#[component]
pub fn details(app: erika::Client, anime: anime::Client) -> impl View {
    ui! {
        Stack(align: top_leading) {
            // Phones extend the backdrop into their system safe areas. Desktop
            // toolbars keep their own room, joining the backdrop's solid top edge.
            if window.compact || window.unfolded {
                Stack(under_bars: true, align: top_leading) {
                    details_wash(anime: anime)
                }
            } else {
                details_wash(anime: anime)
            }
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
            if anime.rating_open {
                rating_dialog(app: app, anime: anime)
            }
            if anime.review_open {
                review_dialog(app: app, anime: anime)
            }
            if anime.mapping_open {
                provider_dialog(app: app, anime: anime)
            }
        }
    }
}

/// The score dialog: Bangumi's ten steps with a Bangumi account, the even
/// five without; picking one rates the show and closes it.
#[component]
pub fn rating_dialog(app: erika::Client, anime: anime::Client) -> impl View {
    ui! {
        Modal(width: 360, appear: pop_in, disappear: fade) {
            Column(gap: 12, padding: 24) {
                Text(content: tr! { "zh-Hans" => "给这部作品评分", "en" => "Rate this show" }, style: title3)
                Text(content: anime.sync_targets, style: caption, emphasis: secondary)
                if anime.fine_scores {
                    Picker(id: score_steps, label: tr! { "zh-Hans" => "评分", "en" => "Score" }, selected: anime.my_score, style: radio) {
                        PickerOption(value: Score::Ten, label: tr! { "zh-Hans" => "10 分 · 超神作", "en" => "10 · All-time best" })
                        PickerOption(value: Score::Nine, label: tr! { "zh-Hans" => "9 分 · 神作", "en" => "9 · Masterpiece" })
                        PickerOption(value: Score::Eight, label: tr! { "zh-Hans" => "8 分 · 力荐", "en" => "8 · Great" })
                        PickerOption(value: Score::Seven, label: tr! { "zh-Hans" => "7 分 · 推荐", "en" => "7 · Good" })
                        PickerOption(value: Score::Six, label: tr! { "zh-Hans" => "6 分 · 还行", "en" => "6 · Fair" })
                        PickerOption(value: Score::Five, label: tr! { "zh-Hans" => "5 分 · 不过不失", "en" => "5 · Average" })
                        PickerOption(value: Score::Four, label: tr! { "zh-Hans" => "4 分 · 较差", "en" => "4 · Poor" })
                        PickerOption(value: Score::Three, label: tr! { "zh-Hans" => "3 分 · 差", "en" => "3 · Bad" })
                        PickerOption(value: Score::Two, label: tr! { "zh-Hans" => "2 分 · 很差", "en" => "2 · Very bad" })
                        PickerOption(value: Score::One, label: tr! { "zh-Hans" => "1 分 · 不忍直视", "en" => "1 · Unwatchable" })
                    } => anime.pick_score(anime.my_score)
                } else {
                    Picker(id: score_even, label: tr! { "zh-Hans" => "评分", "en" => "Score" }, selected: anime.my_score, style: radio) {
                        PickerOption(value: Score::Ten, label: tr! { "zh-Hans" => "10 分 · 超神作", "en" => "10 · All-time best" })
                        PickerOption(value: Score::Eight, label: tr! { "zh-Hans" => "8 分 · 力荐", "en" => "8 · Great" })
                        PickerOption(value: Score::Six, label: tr! { "zh-Hans" => "6 分 · 还行", "en" => "6 · Fair" })
                        PickerOption(value: Score::Four, label: tr! { "zh-Hans" => "4 分 · 较差", "en" => "4 · Poor" })
                        PickerOption(value: Score::Two, label: tr! { "zh-Hans" => "2 分 · 很差", "en" => "2 · Very bad" })
                    } => anime.pick_score(anime.my_score)
                }
                Row(gap: 10, align: center) {
                    if anime.my_score != Score::Unrated {
                        Button(id: rating_clear, label: tr! { "zh-Hans" => "清除评分", "en" => "Clear Rating" }, role: destructive, prominence: plain) => anime.pick_score(Score::Unrated)
                    }
                    Spacer
                    Button(id: rating_done, label: tr! { "zh-Hans" => "完成", "en" => "Done" }, shortcut: app.keys.back) => anime.show_rating(false)
                }
            }
        }
    }
}

/// The review dialog: the review on Bangumi, read anew as it opens, is
/// where the writing starts; sending it saves it on Bangumi.
#[component]
pub fn review_dialog(app: erika::Client, anime: anime::Client, #[session] draft: AnimeDraft) -> impl View {
    ui! {
        Modal(width: 480, appear: pop_in, disappear: fade) {
            Column(gap: 12, padding: 24) {
                // Seed once when the read finishes, never when the user empties the draft.
                if !anime.review_loading {
                    Watch(value: anime.review_loading) => draft.review.set(anime.review_text)
                }
                Text(content: tr! { "zh-Hans" => "写评价", "en" => "Write a Review" }, style: title3)
                Text(content: tr! { "zh-Hans" => "保存在你的 Bangumi 收藏里，最多 200 字", "en" => "Saved with your Bangumi collection, up to 200 characters" }, style: caption, emphasis: secondary)
                if anime.review_loading {
                    Text(content: tr! { "zh-Hans" => "正在读取你在 Bangumi 上的评价…", "en" => "Reading your review on Bangumi…" }, style: caption, emphasis: secondary)
                } else {
                    TextInput(id: review_field, label: tr! { "zh-Hans" => "评价", "en" => "Review" }, label_hidden: true, value: draft.review, kind: multiline, prompt: tr! { "zh-Hans" => "说说你对这部作品的看法", "en" => "What did you think of it?" })
                }
                if anime.review_error != "" {
                    Text(content: anime.review_error, style: caption, lines: 3)
                }
                Row(gap: 10, align: center) {
                    if anime.review_text != "" {
                        Button(id: review_remove, label: tr! { "zh-Hans" => "删除评价", "en" => "Delete Review" }, role: destructive, prominence: plain, enabled: !anime.review_saving, clears: draft.review) => anime.submit_review("")
                    }
                    Spacer
                    Button(id: review_cancel, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }, role: cancel, prominence: plain, shortcut: app.keys.back, clears: draft.review) => anime.show_review(false)
                    if anime.review_saving {
                        Button(id: review_sending, label: tr! { "zh-Hans" => "正在提交…", "en" => "Sending…" }, prominence: prominent, enabled: false) => anime.show_review(true)
                    } else {
                        Button(id: review_send, label: tr! { "zh-Hans" => "提交", "en" => "Send" }, prominence: prominent, enabled: !anime.review_loading) => anime.submit_review(draft.review)
                    }
                }
            }
        }
    }
}

/// How the marks of the open show are syncing, under its actions.
#[component]
pub fn sync_status(anime: anime::Client) -> impl View {
    ui! {
        Column(gap: 0) {
            if anime.syncing {
                Text(content: tr! { "zh-Hans" => "正在同步观看记录…", "en" => "Syncing your watch records…" }, style: caption, emphasis: secondary, lines: 1)
            } else if anime.sync_note != "" {
                Text(content: anime.sync_note, style: caption, emphasis: secondary, lines: 3)
            }
        }
    }
}

#[component]
pub fn desktop_details(anime: anime::Client, #[session] draft: AnimeDraft) -> impl View {
    ui! {
        ScrollView(appear: page_in) {
            Stack(align: top_leading) {
                Column(gap: 0, padding_x: if window.tv { 56 } else { 32 }, padding_y: 12) {
                    Row(gap: 36, align: top) {
                        Image(source: anime.show.poster, width: 220, height: 330, fit: cover, corner: poster, label: anime.show.title)
                        Column(gap: 0, stagger: 0.03) {
                            Canvas(height: 10)
                            detail_kind(anime: anime)
                            Canvas(height: 8)
                            if anime.show.logo.url != "" {
                                Image(source: anime.show.logo, width: 320, height: 90, fit: contain, label: anime.show.title)
                            } else {
                                Text(content: anime.show.title, style: title, lines: 2, appear: fade)
                            }
                            Canvas(height: 12)
                            Row(gap: 10, align: center, appear: fade) {
                                if anime.show.rating > 0.0 {
                                    Text(content: tr! { "zh-Hans" => "★ {rating}", "en" => "★ {rating}"; rating: anime.show.rating }, style: callout)
                                }
                                if !window.medium {
                                    Row(gap: 6) {
                                        ForEach(items: anime.show.tags, key: name) |tag| {
                                            tag_chip(name: tag.name)
                                        }
                                    }
                                }
                            }
                            // Upright on a tablet the tags and the summary
                            // have only what the poster leaves them.
                            if window.medium {
                                Canvas(height: 10)
                                ScrollView(axis: horizontal, indicators: mouse) {
                                    Row(gap: 6) {
                                        ForEach(items: anime.show.tags, key: name) |tag| {
                                            tag_chip(name: tag.name)
                                        }
                                    }
                                }
                                Canvas(height: 12)
                                Column(appear: fade) {
                                    Text(content: anime.show.summary, lines: 3)
                                    detail_fetch_status(anime: anime)
                                }
                            } else {
                                Canvas(height: 14)
                                Column(width: 600, appear: fade) {
                                    Text(content: anime.show.summary, lines: 3)
                                    detail_fetch_status(anime: anime)
                                }
                            }
                            Canvas(height: 20)
                            Row(gap: 10, align: center, appear: fade) {
                                Button(id: watch, label: anime.next, icon: "play.fill", prominence: prominent, variant: primary) => anime.play_episode(anime.next_episode)
                                Menu(id: status, label: anime.show.status, icon: "heart") {
                                    MenuItem(id: want, label: tr! { "zh-Hans" => "想看", "en" => "Want to Watch" }) => anime.set_status(WatchStatus::Want)
                                    MenuItem(id: watching_now, label: tr! { "zh-Hans" => "在看", "en" => "Watching" }) => anime.set_status(WatchStatus::Watching)
                                    MenuItem(id: watched, label: tr! { "zh-Hans" => "看过", "en" => "Watched" }) => anime.set_status(WatchStatus::Watched)
                                    MenuItem(id: dropped, label: tr! { "zh-Hans" => "抛弃", "en" => "Dropped" }) => anime.set_status(WatchStatus::Dropped)
                                }
                                if anime.can_rate {
                                    Button(id: rate, label: anime.my_score, icon: "star") => anime.show_rating(true)
                                }
                                if anime.can_review {
                                    Button(id: write_review, label: tr! { "zh-Hans" => "写评价", "en" => "Review" }, icon: "note.text") => anime.show_review(true)
                                }
                            }
                            Canvas(height: 8)
                            sync_status(anime: anime)
                        }
                    }
                    Canvas(height: 34)
                    Row(gap: 10, align: center, appear: fade) {
                        episode_heading(anime: anime)
                        Spacer
                        IconButton(id: film_back, icon: "chevron.left", label: tr! { "zh-Hans" => "上一页", "en" => "Previous" }) => draft.episode.set(anime.first_episode)
                        IconButton(id: film_on, icon: "chevron.right", label: tr! { "zh-Hans" => "下一页", "en" => "Next" }) => draft.episode.set(anime.last_episode)
                    }
                    Canvas(height: 6)
                    // The film pages with the arrows above it, as the design's
                    // does; only a mouse, which cannot drag it, gets a scroll
                    // bar. `.d-track`: 22 points above and below the cells.
                    ScrollView(axis: horizontal, snap: start, position: draft.episode, indicators: mouse) {
                        Row(gap: 30, padding_y: 22) {
                            ForEach(items: anime.episodes, key: id) |episode| {
                                film_cell(anime: anime, episode: episode)
                            }
                        }
                    }
                    Canvas(height: 18)
                    detail_information(anime: anime)
                    Canvas(height: 32)
                }
            }
        }
    }
}

/// One cell of the film: the still of an episode reached, or its number.
/// Equal-sized cells keep the leading episode as the browsing anchor.
#[component]
pub fn film_cell(anime: anime::Client, episode: anime::Episode) -> impl View {
    ui! {
        Column(gap: 0, width: 220) {
            Stack(align: top_trailing) {
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
            } => anime.play_episode(episode.id)
            // The plugin that marks single episodes turns these on.
            if anime.episode_marking {
                Row(gap: 0, padding: 6) {
                    if episode.watched {
                        IconButton(id: mark_unwatched, icon: "checkmark.circle", label: tr! { "zh-Hans" => "标记为未看", "en" => "Mark as not watched" }, prominence: prominent) => anime.mark_episode(episode.id, false)
                    } else {
                        IconButton(id: mark_watched, icon: "circle", label: tr! { "zh-Hans" => "标记为已看", "en" => "Mark as watched" }, prominence: prominent) => anime.mark_episode(episode.id, true)
                    }
                }
            }
            }
            // `.et` and `.es`: the title's 14 / 19 and the caption's 11.5 / 15.
            Canvas(height: 8)
            if episode.next {
                Text(content: episode.title, style: headline, mark: accent, lines: 1, line_height: 19)
            } else {
                Text(content: episode.title, style: headline, lines: 1, line_height: 19)
            }
            Row(gap: 4, align: center) {
                Text(content: episode.note, style: caption, emphasis: secondary, line_height: 15)
                if episode.watched {
                    Text(content: tr! { "zh-Hans" => "· 已看", "en" => "· Watched" }, style: caption, emphasis: secondary, line_height: 15)
                }
            }
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
pub fn new_page(app: erika::Client, anime: anime::Client) -> impl View {
    ui! {
        if anime.opened != "" && anime.opened_from_schedule && !window.unfolded && (window.compact || window.tv) {
            details(app: app, anime: anime)
        } else if window.compact || window.medium {
            ScrollView(appear: page_in) {
                calendar_page(app: app, anime: anime)
            }
        } else {
            Stack(appear: page_in) {
                calendar_page(app: app, anime: anime)
            }
        }
    }
}

/// Wide windows give the week the space below the heading; narrow layouts
/// keep their outer scroll view and expand each day vertically.
#[component]
pub fn calendar_page(app: erika::Client, anime: anime::Client) -> impl View {
    ui! {
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
                        Text(content: anime.season_label, emphasis: secondary, lines: 1)
                        Spacer
                    }
                    Canvas(height: 18)
                }
                if anime.loading {
                    season_note(app: app, anime: anime)
                    Canvas(height: 12)
                }
                if window.compact || window.medium {
                    Column(gap: 8) {
                        ForEach(items: anime.week, key: id) |day| {
                            calendar_vertical(anime: anime, day: day)
                        }
                    }
                } else {
                    Accordion(count: 7, selected: anime.selected_day, collapsed_width: 86, gap: 8, corner: card, animate: bouncy) {
                        ForEach(items: anime.week, key: id, stagger: 0.036) |day| {
                            Stack(align: top_leading, appear: calendar_column_in, appear_delay: 0.06) {
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
                                        Column(gap: 0, align: center) {
                                            calendar_header_collapsed(day: day)
                                            Canvas(height: 19)
                                            PreviewList(items: day.shows, key: id, item_height: 97, gap: 10, more_height: 22) |show| {
                                                Column(gap: 4, align: center) {
                                                    Image(source: show.poster, width: 52, height: 78, fit: cover, corner: poster)
                                                    Row(gap: 0, height: 15) {
                                                        if show.timed {
                                                            Text(content: tr! { "zh-Hans" => "{time, duration, hh:mm}", "en" => "{time, duration, hh:mm}"; time: show.time }, style: caption, emphasis: secondary, lines: 1)
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    } => anime.select_day(day.id)
                                }
                            }
                        }
                    }
                }
                Canvas(height: if window.compact || window.medium { 48 } else { 24 })
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
/// 86 points, 16 from its leading edge and running to the other, as the
/// design pads it, so "今天 · 15 部" fits.
#[component]
pub fn calendar_header_collapsed(day: anime::BroadcastDay) -> impl View {
    ui! {
        Column(gap: 0, width: 86) {
            Canvas(height: 16)
            Row(gap: 0, height: if window.tv { 46 } else { 39 }) {
            Canvas(width: 16)
            Column(gap: 0, width: 70) {
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
                            if !show.timed {
                                Text(content: tr! { "zh-Hans" => "—", "en" => "—" }, style: headline, emphasis: tertiary, lines: 1, line_height: if window.tv { 22 } else { 19 })
                            } else if show.aired {
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
                            } else if show.tonight && show.timed {
                                Text(content: tr! { "zh-Hans" => "今晚更新", "en" => "Airing tonight" }, style: caption, emphasis: secondary, lines: 1, line_height: if window.tv { 18 } else { 15 })
                            } else if !show.timed {
                                Text(content: tr! { "zh-Hans" => "播出时刻未知", "en" => "Time unknown" }, style: caption, emphasis: secondary, lines: 1, line_height: if window.tv { 18 } else { 15 })
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
pub fn phone_home(app: erika::Client, anime: anime::Client) -> impl View {
    ui! {
        ScrollView(appear: page_in) {
            Column(gap: 0) {
                // The shell's header row scrolls with the page.
                Slot(id: page_header)
                if anime.has_stage {
                Column(gap: 0, padding_x: 16) {
                    phone_stage(anime: anime)
                    Canvas(height: 14.5)
                    Text(content: tr! { "zh-Hans" => "继续观看", "en" => "Continue watching" }, style: caption, emphasis: secondary)
                    Canvas(height: 11)
                    Text(content: anime.stage.title, style: title3, font_size: 20, lines: 1)
                    Canvas(height: 4)
                    Row(gap: 6, align: baseline) {
                        if anime.stage.chapter != "" {
                            Text(content: anime.stage.chapter, font_size: 13, mark: accent)
                        }
                        Text(content: tr! { "zh-Hans" => "剩 {left} 分钟", "en" => "{left} min left"; left: anime.stage.left }, font_size: 13, emphasis: secondary)
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
                    ScrollView(axis: horizontal, indicators: mouse) {
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
                }
                Column(gap: 0, padding_x: 16) {
                    Canvas(height: if anime.has_stage { 32.7 } else { 4 })
                    Row(gap: 10, align: baseline) {
                        Text(content: tr! { "zh-Hans" => "今日更新", "en" => "Airing today" }, style: title3)
                        Spacer
                        Button(id: phone_schedule, label: tr! { "zh-Hans" => "新番表", "en" => "Schedule" }) {
                            Text(content: tr! { "zh-Hans" => "新番表 ›", "en" => "Schedule ›" }, font_size: 13, emphasis: secondary)
                        } => pages.new.show()
                    }
                    Canvas(height: 12)
                }
                // The timeline, at least the design's 900 points, opens with
                // "now" in the middle, or as near it as the track reaches.
                if anime.has_airing {
                Row(gap: 0, align: top) {
                    Canvas(width: 16)
                    ScrollView(axis: horizontal, indicators: mouse, open_center: anime.now_at) {
                        Row(gap: 0) {
                            Column(gap: 0, width: anime.track_width) {
                                timeline_track(anime: anime)
                            }
                            Canvas(width: 16)
                        }
                    }
                }
                } else {
                    Column(gap: 0, padding_x: 16) {
                        season_note(app: app, anime: anime)
                    }
                }
                // The other modules' sections, as the library's recently added.
                Slot(id: home_sections)
                Canvas(height: 24)
            }
        }
    }
}

/// A fixed, edge-to-edge backdrop, not part of the scrolling document.
/// Only its edges disappear into the canvas; the middle keeps the artwork.
#[component]
pub fn details_wash(anime: anime::Client) -> impl View {
    ui! {
        Stack(align: top_leading, appear: fade) {
            Canvas {
                Shape(kind: rect, fill: surface.canvas)
            }
            if anime.show.backdrop.url != "" {
                Image(source: anime.show.backdrop, fit: cover, appear: fade)
                Canvas {
                    Shape(kind: rect, fill: surface.canvas, opacity: 0.32)
                }
            } else {
                Canvas {
                    Shape(kind: rect, fill_from: anime.show.poster, opacity: 0.34)
                }
            }
            Rail(axis: vertical) {
                Canvas(from: 0, to: 0.3) {
                    Shape(kind: rect, fill: surface.canvas, fade: down)
                }
                Canvas(from: 0.42, to: 1) {
                    Shape(kind: rect, fill: surface.canvas, fade: up)
                }
            }
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
                    Row(gap: 6) {
                        if watching.chapter != "" {
                            Text(content: watching.chapter, style: caption, emphasis: secondary, lines: 1)
                        }
                        Text(content: tr! { "zh-Hans" => "剩 {left} 分钟", "en" => "{left} min left"; left: watching.left }, style: caption, emphasis: secondary, lines: 1)
                    }
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
            Stack(align: top_leading) {
            Column(gap: 0) {
                // Beside the page that opened them the details close by
                // themselves; the rest of the header stays on that page.
                if window.unfolded {
                    Canvas(height: 7)
                    Row(gap: 0, padding_x: 16, height: 48, align: center) {
                        IconButton(id: fold_back, icon: "chevron.left", font_size: 18, height: 44, label: anime.back) => anime.close()
                        Spacer
                    }
                } else if window.compact {
                    Slot(id: page_header)
                }
                Column(gap: 0, padding_x: 16) {
                    Canvas(height: 8)
                    Row(gap: 16, align: top) {
                        Image(source: anime.show.poster, width: 118, height: 177, fit: cover, corner: poster, label: anime.show.title)
                        // Use the space left by the poster, including narrow
                        // phones and foldable columns, not a prototype width.
                        Column(gap: 0) {
                            Canvas(height: 4)
                            detail_kind(anime: anime)
                            Canvas(height: 6)
                            if anime.show.logo.url != "" {
                                Image(source: anime.show.logo, height: 72, aspect: 3.0555555556, fit: contain, label: anime.show.title)
                            } else {
                                Text(content: anime.show.title, style: title3, font_size: 21, lines: 2)
                            }
                            Canvas(height: 8)
                            Row(gap: 10, align: center) {
                                if anime.show.rating > 0.0 {
                                    Text(content: tr! { "zh-Hans" => "★ {rating}", "en" => "★ {rating}"; rating: anime.show.rating }, style: headline, font_size: 16)
                                }
                            }
                            Canvas(height: 8)
                            ScrollView(axis: horizontal, indicators: mouse) {
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
                            MenuItem(id: dropped, label: tr! { "zh-Hans" => "抛弃", "en" => "Dropped" }) => anime.set_status(WatchStatus::Dropped)
                        }
                        if anime.can_rate {
                            Button(id: phone_rate, label: anime.my_score, icon: "star", shape: capsule, height: 32) => anime.show_rating(true)
                        }
                        if anime.can_review {
                            Button(id: phone_review, label: tr! { "zh-Hans" => "写评价", "en" => "Review" }, icon: "note.text", shape: capsule, height: 32) => anime.show_review(true)
                        }
                        Spacer
                    }
                    Canvas(height: 6)
                    sync_status(anime: anime)
                    Canvas(height: 14)
                    Text(content: anime.show.summary, font_size: 13, line_height: 18, lines: 3)
                    detail_fetch_status(anime: anime)
                    Canvas(height: 34)
                    Row(gap: 10, align: baseline) {
                        episode_heading(anime: anime)
                        Spacer
                    }
                    Canvas(height: 6)
                }
                // `.d-strip`: 21.5 points above and below the cells; a
                // scroll bar only for a mouse.
                ScrollView(axis: horizontal, snap: start, position: draft.episode, indicators: mouse) {
                    Row(gap: 30, padding_x: 16, padding_y: 21.5) {
                        ForEach(items: anime.episodes, key: id) |episode| {
                            film_cell(anime: anime, episode: episode)
                        }
                    }
                }
                Column(gap: 0, padding_x: 16) {
                    Canvas(height: 18)
                    detail_information(anime: anime)
                    Canvas(height: 32)
                }
            }
            }
        }
    }
}

/// Content kind and only a known premiere date: missing metadata is not 1970.
#[component]
pub fn detail_kind(anime: anime::Client) -> impl View {
    ui! {
        Row(gap: 6, align: baseline) {
            if anime.show.movie {
                Text(content: tr! { "zh-Hans" => "电影", "en" => "Film" }, style: caption, emphasis: secondary)
            } else {
                Text(content: tr! { "zh-Hans" => "剧集", "en" => "Series" }, style: caption, emphasis: secondary)
            }
            if anime.show.has_premiere {
                Text(content: tr! { "zh-Hans" => "{premiere, date, y 年 M 月}", "en" => "{premiere, date, MMM y}"; premiere: anime.show.premiere }, style: caption, emphasis: secondary)
            }
        }
    }
}

/// Season identity is distinct from an episode's display number, including S00.
#[component]
pub fn episode_heading(anime: anime::Client) -> impl View {
    ui! {
        Row(gap: 10, align: center) {
            if anime.show.movie {
                Text(content: tr! { "zh-Hans" => "影片", "en" => "Film" }, style: title3)
            } else {
                if anime.selected_season != "" {
                    ScrollView(axis: horizontal, indicators: mouse) {
                        Row(gap: 8) {
                            ForEach(items: anime.seasons, key: id) |season| {
                                if anime.selected_season == season.id {
                                    Button(id: selected_season, label: season.title, prominence: prominent) => anime.select_season(season.id)
                                } else {
                                    Button(id: select_season, label: season.title) => anime.select_season(season.id)
                                }
                            }
                        }
                    }
                } else {
                    Text(content: tr! { "zh-Hans" => "剧集", "en" => "Episodes" }, style: title3)
                }
                if anime.show.watched > 0 {
                    Text(content: tr! { "zh-Hans" => "已看 {watched} 集", "en" => "{watched} watched"; watched: anime.show.watched }, style: caption, emphasis: secondary)
                }
            }
        }
    }
}

/// Shared by the phone, desktop and television details page.
#[component]
pub fn detail_information(anime: anime::Client, #[session] draft: AnimeDraft) -> impl View {
    ui! {
        Column(gap: 16) {
            Row(gap: 10, align: center) {
                Row(width: if window.tv { 220 } else { 160 }) {
                    Picker(id: information_tab, label: tr! { "zh-Hans" => "简介与评论", "en" => "About and comments" }, value: draft.tab, style: segmented, label_hidden: true)
                }
                Spacer
                if anime.can_map {
                    Button(id: provider_links, label: tr! { "zh-Hans" => "关联", "en" => "Links" }, icon: "link", prominence: plain) => anime.show_mappings(true)
                }
                if draft.tab == DetailTab::Comments {
                    Button(id: refresh_reviews, label: tr! { "zh-Hans" => "刷新", "en" => "Refresh" }, enabled: !anime.comments_loading, prominence: plain) => anime.refresh_comments()
                }
            }
            if draft.tab == DetailTab::About {
                if window.compact {
                    Text(content: anime.show.summary, font_size: 13, line_height: 18)
                } else if window.medium {
                    Text(content: anime.show.summary)
                } else {
                    Column(width: 640) {
                        Text(content: anime.show.summary)
                    }
                }
            } else {
                review_section(anime: anime)
            }
        }
    }
}

#[component]
pub fn detail_fetch_status(anime: anime::Client) -> impl View {
    ui! {
        Column(gap: 8) {
            if anime.details_loading {
                Text(content: tr! { "zh-Hans" => "正在加载详情…", "en" => "Loading details…" }, style: caption, emphasis: secondary)
            } else if anime.details_failed {
                Text(content: anime.details_error, style: caption, emphasis: secondary)
                Button(id: retry_details, label: tr! { "zh-Hans" => "重试", "en" => "Retry" }, icon: "arrow.triangle.2.circlepath") => anime.refresh_details()
            }
        }
    }
}

#[component]
pub fn review_section(anime: anime::Client) -> impl View {
    ui! {
        Column(gap: 14) {
            if anime.comments_loading {
                Text(content: tr! { "zh-Hans" => "正在更新评论…", "en" => "Updating comments…" }, style: caption, emphasis: secondary)
            } else if !anime.has_comments {
                Text(content: tr! { "zh-Hans" => "暂无评论", "en" => "No comments yet" }, emphasis: secondary)
            }
            Text(content: anime.comments_error, style: caption, emphasis: secondary)
            ForEach(items: anime.comments, key: id) |comment| {
                Column(gap: 8) {
                    Row(gap: 10, align: center) {
                        Image(source: comment.avatar, width: 32, height: 32, fit: cover, corner: poster, label: comment.author)
                        Text(content: comment.author, style: headline, lines: 1)
                        Spacer
                        if comment.rating > 0 {
                            Text(content: tr! { "zh-Hans" => "★ {rating}", "en" => "★ {rating}"; rating: comment.rating }, style: caption)
                        }
                        Text(content: comment.updated, style: caption, emphasis: secondary)
                    }
                    Text(content: comment.text, emphasis: secondary)
                    Divider
                }
            }
        }
    }
}
