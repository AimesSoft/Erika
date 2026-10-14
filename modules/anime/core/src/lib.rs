//! Engine of the anime module. In this first phase the shows are the
//! design's sample data; asking to play only tells the shell's player.

use bern::CommandError;
use bern::media::MediaSource;
use bern::resources::Resources;
use erika_anime_protocol::anime::{self, Airing, Episode, Hour, Show, State, Tag, Watching};

/// The sample shows of the design: id, title, poster, episodes, watched,
/// rating, tags. Show 100 has no poster, only a still.
const SHOWS: &[(i64, &str, i64, i64, i64, f64, &[&str])] = &[
    (1, "寒蝉鸣泣之时 解", 1, 24, 1, 8.4, &["悬疑", "推理"]),
    (2, "只有神知道的世界 女神篇", 2, 12, 11, 7.9, &["恋爱", "喜剧"]),
    (3, "【我推的孩子】 第三季", 3, 11, 2, 7.8, &["偶像", "剧情"]),
    (5, "咒术回战 死灭回游", 5, 12, 3, 7.6, &["战斗", "奇幻"]),
    (6, "为美好的世界献上祝福！3", 6, 11, 11, 7.9, &["喜剧", "异世界"]),
    (7, "葬送的芙莉莲 第二季", 7, 10, 3, 8.4, &["奇幻", "冒险"]),
    (9, "Fate/strange Fake", 9, 13, 3, 8.0, &["战斗", "奇幻"]),
    (10, "地狱乐 第二季", 10, 12, 2, 6.7, &["战斗", "历史"]),
    (12, "寒蝉鸣泣之时 礼", 12, 5, 0, 7.3, &["悬疑"]),
    (13, "魔都精兵的奴隶 第二季", 13, 12, 0, 6.1, &["战斗", "奇幻"]),
    (14, "现在多闻君是哪一面！？", 14, 12, 0, 5.8, &["恋爱", "喜剧"]),
    (15, "优雅贵族的休假指南。", 15, 12, 0, 4.9, &["异世界", "日常"]),
    (16, "厄里斯的圣杯", 16, 12, 0, 5.4, &["悬疑", "历史"]),
    (100, "只有我不存在的城市", 0, 12, 0, 8.6, &["悬疑"]),
];

/// Shows being watched: id, episode, progress, minutes left.
const WATCHING: &[(i64, i64, f64, i64)] = &[(100, 1, 0.38, 15), (7, 4, 0.35, 16), (3, 3, 0.2, 19), (5, 4, 0.8, 4)];

/// Today's shows and when they air, in hours after midnight of today; the
/// small hours count past 24.
const AIRING: &[(i64, &str, f64)] = &[
    (13, "18:30", 18.5),
    (14, "19:45", 19.75),
    (15, "21:00", 21.0),
    (16, "22:15", 22.25),
    (9, "23:30", 23.5),
    (12, "00:30", 24.5),
    (2, "01:30", 25.5),
];

/// "Now" on the timeline: 22:40.
const NOW: f64 = 22.0 + 40.0 / 60.0;

const SUMMARY: &str = "这里是来自 Bangumi 的番剧简介。原型里所有番剧共用这一段示意文字：旅程继续向前，新的同伴和旧的回忆交织在一起，每一集都把故事往终点推近一点。";

fn picture(name: &str) -> MediaSource {
    MediaSource::resource(&Resources::host(), name).unwrap_or_else(|_| MediaSource::new(""))
}

fn poster(number: i64) -> MediaSource {
    if number == 0 {
        picture("design/assets/still1.jpg")
    } else {
        picture(&format!("design/assets/posters/p{number}.jpg"))
    }
}

fn show(id: i64) -> Option<Show> {
    let &(id, title, number, episodes, watched, rating, tags) =
        SHOWS.iter().find(|(show, ..)| *show == id)?;
    Some(Show {
        id,
        title: title.into(),
        poster: poster(number),
        episodes,
        watched,
        rating: if rating > 0.0 { format!("★ {rating}") } else { String::new() },
        kind: format!("TV 动画 · 2026 年 1 月 · 共 {episodes} 话"),
        tags: tags.iter().map(|tag| Tag { name: (*tag).into() }).collect(),
        summary: SUMMARY.into(),
        status: status(watched, episodes).into(),
    })
}

fn status(watched: i64, episodes: i64) -> &'static str {
    match watched {
        0 => "想看",
        watched if watched >= episodes => "看过",
        _ => "在看",
    }
}

fn watching(&(id, episode, progress, left): &(i64, i64, f64, i64)) -> Watching {
    let show = show(id).expect("watched shows are sample shows");
    Watching {
        id,
        title: show.title,
        still: show.poster,
        episode,
        episode_label: format!("第 {episode} 集"),
        left: format!("剩 {left} 分钟"),
        progress,
    }
}

/// The timeline spans 18:00 to 02:00 (hour 26), eight hours over 992
/// points that start 8 points before the page's column of 976.
const FIRST_HOUR: f64 = 18.0;
const HOURS: f64 = 8.0;

fn timeline(hour: f64) -> f64 {
    (-8.0 + (hour - FIRST_HOUR) / HOURS * 992.0) / 976.0
}

fn hours() -> Vec<Hour> {
    (0..=HOURS as i64)
        .map(|step| {
            let hour = FIRST_HOUR as i64 + step;
            Hour {
                label: format!("{:02}:00", hour % 24),
                at: timeline(hour as f64),
            }
        })
        .collect()
}

/// The film of a show: twelve cells at most; the episodes reached show a
/// still, the others their number.
fn episodes(show: &Show) -> Vec<Episode> {
    let next = (show.watched + 1).min(show.episodes);
    (1..=show.episodes.min(12))
        .map(|number| {
            let watched = number <= show.watched;
            let current = number == next && show.watched < show.episodes;
            Episode {
                number,
                title: match (current, show.watched) {
                    (true, 0) => format!("第 {number} 话 · 开始"),
                    (true, _) => format!("第 {number} 话 · 继续"),
                    _ => format!("第 {number} 话"),
                },
                note: if watched {
                    "已看".into()
                } else if current && show.watched > 0 {
                    "剩 16 分钟".into()
                } else {
                    "24 分钟".into()
                },
                pictured: watched || current,
                still: show.poster.clone(),
                numeral: format!("{number:02}"),
                watched,
                next: current,
                progress: if current && show.watched > 0 { 0.35 } else { 0.0 },
            }
        })
        .collect()
}

pub struct AnimeEngine {
    state: State,
}

impl Default for AnimeEngine {
    fn default() -> Self {
        let mut watching = WATCHING.iter().map(watching);
        let stage = watching.next().expect("someone is watching");
        let aired = AIRING.iter().filter(|(_, _, hour)| *hour <= NOW).count();
        Self {
            state: State {
                today: "周一 · 10 月 12 日".into(),
                stage,
                also: watching.collect(),
                airing: AIRING
                    .iter()
                    .map(|&(id, time, hour)| {
                        let show = show(id).expect("airing shows are sample shows");
                        let when = if hour <= NOW { "已更新" } else { "稍后" };
                        Airing {
                            id,
                            title: show.title,
                            poster: show.poster,
                            time: format!("{time} · {when}"),
                            at: timeline(hour),
                        }
                    })
                    .collect(),
                hours: hours(),
                now_at: timeline(NOW),
                now_label: "现在 22:40".into(),
                airing_summary: format!(
                    "已更新 {aired} 部 · 今晚还有 {} 部",
                    AIRING.len() - aired
                ),
                opened: 0,
                show: show(100).expect("the stage's show is a sample show"),
                episodes: Vec::new(),
                progress_label: String::new(),
                next_episode: 1,
                next_label: String::new(),
                back_label: "首页".into(),
            },
        }
    }
}

impl AnimeEngine {
    fn find(&self, id: i64) -> Result<Show, CommandError> {
        show(id).ok_or_else(|| CommandError::new("unknown_show", format!("no show {id}")))
    }

    fn watching(&self, id: i64) -> Option<&Watching> {
        std::iter::once(&self.state.stage)
            .chain(&self.state.also)
            .find(|watching| watching.id == id)
    }
}

impl anime::Commands for AnimeEngine {
    fn open(&mut self, id: i64) -> Result<(), CommandError> {
        let show = self.find(id)?;
        let next = (show.watched + 1).min(show.episodes);
        self.state.episodes = episodes(&show);
        self.state.progress_label = format!("共 {} 话 · 看到第 {} 话", show.episodes, show.watched);
        self.state.next_episode = next;
        self.state.next_label = if show.watched == 0 {
            "开始观看".into()
        } else {
            format!("继续 第 {next} 话")
        };
        self.state.opened = id;
        self.state.show = show;
        Ok(())
    }

    fn close(&mut self) -> Result<(), CommandError> {
        self.state.opened = 0;
        Ok(())
    }

    fn stage(&mut self, id: i64) -> Result<(), CommandError> {
        let index = self
            .state
            .also
            .iter()
            .position(|watching| watching.id == id)
            .ok_or_else(|| CommandError::new("unknown_show", format!("show {id} is not beside the stage")))?;
        std::mem::swap(&mut self.state.stage, &mut self.state.also[index]);
        Ok(())
    }

    fn resume(&mut self, id: i64) -> Result<(), CommandError> {
        let watching = self
            .watching(id)
            .ok_or_else(|| CommandError::new("unknown_show", format!("show {id} is not being watched")))?;
        anime::emit::play(
            watching.title.clone(),
            watching.episode_label.clone(),
            watching.still.clone(),
            watching.progress,
        );
        Ok(())
    }

    fn set_status(&mut self, status: String) -> Result<(), CommandError> {
        self.state.show.status = status;
        Ok(())
    }

    fn rate(&mut self, score: i64) -> Result<(), CommandError> {
        self.state.show.rating = format!("★ {}", score.clamp(1, 10));
        Ok(())
    }

    fn play_episode(&mut self, number: i64) -> Result<(), CommandError> {
        let episode = self
            .state
            .episodes
            .iter()
            .find(|episode| episode.number == number)
            .ok_or_else(|| CommandError::new("unknown_episode", format!("no episode {number}")))?;
        anime::emit::play(
            self.state.show.title.clone(),
            format!("第 {number} 集"),
            episode.still.clone(),
            0.0,
        );
        Ok(())
    }
}

impl anime::Engine for AnimeEngine {
    fn state(&self) -> &State {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anime::Commands;

    #[test]
    fn details_open_on_the_episode_to_watch_next() {
        let mut engine = AnimeEngine::default();
        engine.open(7).unwrap();
        assert_eq!(engine.state.opened, 7);
        assert_eq!(engine.state.next_label, "继续 第 4 话");
        assert_eq!(engine.state.episodes.len(), 10);
        assert!(engine.state.episodes[3].next);
        engine.close().unwrap();
        assert_eq!(engine.state.opened, 0);
        assert_eq!(engine.open(99).unwrap_err().code, "unknown_show");
    }

    #[test]
    fn the_timeline_puts_now_between_the_aired_and_the_coming() {
        let engine = AnimeEngine::default();
        let aired: Vec<_> = engine.state.airing.iter().filter(|show| show.at < engine.state.now_at).collect();
        assert_eq!(aired.len(), 4);
        assert_eq!(engine.state.airing_summary, "已更新 4 部 · 今晚还有 3 部");
    }

    #[test]
    fn a_show_beside_the_stage_takes_its_place() {
        let mut engine = AnimeEngine::default();
        engine.stage(7).unwrap();
        assert_eq!(engine.state.stage.id, 7);
        assert_eq!(engine.state.also[0].id, 100);
    }
}
