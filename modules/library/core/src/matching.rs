//! Matching videos to episodes by hand, as the design's matching page does
//! (`valsOf`, `lowOf` and `offOf` in design/next-screens.js). By number,
//! each video goes to the episode its name gives, all shifted by `offset`
//! so a second season numbered after the first lines up; by order, the
//! videos go to consecutive episodes from `start`. Numbers typed for a
//! video win over both. One video is re-matching it.

use crate::names::episode_number;
use erika_dandanplay::{Details, Found};
use erika_library_protocol::library::{MatchAnime, MatchNote, MatchRow, Matching};
use erika_store::{Anime, Episode, FileId};
use std::collections::{HashMap, HashSet};

/// A video being matched.
#[derive(Clone, Debug)]
pub struct Video {
    pub key: i64,
    pub name: String,
    /// Its file, `None` when it could not be read.
    pub file: Option<FileId>,
    /// What it is matched to now: the anime and the episode's number.
    pub now: Option<(Anime, i64)>,
}

impl Video {
    fn number(&self) -> Option<i64> {
        episode_number(&self.name)
    }
}

/// The anime the videos go to, and its episodes once dandanplay gave them.
#[derive(Clone, Debug)]
pub struct Chosen {
    pub anime: Anime,
    /// How many episodes the search said it has; 0 when unknown.
    pub count: i64,
    pub episodes: Option<Vec<(i64, Episode)>>,
}

impl Chosen {
    /// How many episodes it has, when known.
    fn total(&self) -> Option<i64> {
        match &self.episodes {
            Some(episodes) if !episodes.is_empty() => episodes.iter().map(|(number, _)| *number).max(),
            _ => (self.count > 0).then_some(self.count),
        }
    }

    /// Its episode `number`: as dandanplay listed it, or by dandanplay's
    /// numbering, the anime's id times 10000 plus the number.
    fn episode(&self, number: i64) -> Episode {
        self.episodes
            .as_ref()
            .and_then(|episodes| episodes.iter().find(|(known, _)| *known == number))
            .map(|(_, episode)| episode.clone())
            .unwrap_or(Episode { dandanplay_id: self.anime.dandanplay_id * 10_000 + number, number, title: String::new() })
    }
}

pub struct Session {
    pub single: bool,
    pub place: String,
    pub videos: Vec<Video>,
    pub on: HashSet<i64>,
    /// Numbers typed for videos.
    pub set: HashMap<i64, i64>,
    pub by_order: bool,
    pub start: i64,
    pub offset: i64,
    pub chosen: Option<Chosen>,
    pub editing: Option<i64>,
    pub odd_only: bool,
    pub results: Vec<Found>,
    pub searching: bool,
    pub search_failed: bool,
    /// Which search the results answer; older answers are dropped.
    pub search: u64,
}

/// The anime most of `videos` are matched to now.
fn usual_anime(videos: &[Video]) -> Option<Anime> {
    let mut votes: Vec<(Anime, usize)> = Vec::new();
    for (anime, _) in videos.iter().filter_map(|video| video.now.as_ref()) {
        match votes.iter_mut().find(|(known, _)| known.dandanplay_id == anime.dandanplay_id) {
            Some((_, count)) => *count += 1,
            None => votes.push((anime.clone(), 1)),
        }
    }
    votes.into_iter().max_by_key(|(_, count)| *count).map(|(anime, _)| anime)
}

impl Session {
    /// Re-match one video: it keeps its episode, or the number in its name.
    pub fn one(video: Video, place: String) -> Session {
        let mut session = Session::new(true, place, vec![video]);
        let video = &session.videos[0];
        if let Some(number) = video.now.as_ref().map(|(_, number)| *number).or_else(|| video.number()) {
            session.set.insert(video.key, number);
        }
        session
    }

    /// Match the videos of a folder, in the order of their numbers. Those
    /// matched to another anime than most are skipped; so are those without
    /// a number while some have one, and then all go by order.
    pub fn folder(mut videos: Vec<Video>, place: String) -> Session {
        videos.sort_by(|a, b| a.number().unwrap_or(i64::MAX).cmp(&b.number().unwrap_or(i64::MAX)).then_with(|| a.name.cmp(&b.name)));
        let mut session = Session::new(false, place, videos);
        session.by_order = !session.videos.iter().any(|video| video.number().is_some());
        let usual = session.chosen.as_ref().map(|chosen| chosen.anime.dandanplay_id);
        let by_order = session.by_order;
        session.on = session
            .videos
            .iter()
            .filter(|video| video.file.is_some())
            .filter(|video| video.now.as_ref().is_none_or(|(anime, _)| Some(anime.dandanplay_id) == usual))
            .filter(|video| by_order || video.number().is_some())
            .map(|video| video.key)
            .collect();
        session
    }

    fn new(single: bool, place: String, videos: Vec<Video>) -> Session {
        let chosen = usual_anime(&videos).map(|anime| Chosen { anime, count: 0, episodes: None });
        let on = videos.iter().filter(|video| video.file.is_some()).map(|video| video.key).collect();
        Session {
            single,
            place,
            videos,
            on,
            set: HashMap::new(),
            by_order: false,
            start: 1,
            offset: 0,
            chosen,
            editing: None,
            odd_only: false,
            results: Vec::new(),
            searching: false,
            search_failed: false,
            search: 0,
        }
    }

    /// The episode each video goes to, by key; `None` for a video skipped
    /// or without one.
    fn values(&self) -> HashMap<i64, Option<i64>> {
        let mut next = self.start;
        self.videos
            .iter()
            .filter(|video| self.on.contains(&video.key))
            .map(|video| {
                let value = match self.set.get(&video.key) {
                    Some(number) => Some(*number),
                    None if self.by_order => {
                        next += 1;
                        Some(next - 1)
                    }
                    None => video.number().map(|number| number + self.offset),
                };
                (video.key, value)
            })
            .collect()
    }

    /// The lowest number the names of the videos going by their names give.
    fn lowest(&self) -> Option<i64> {
        self.videos
            .iter()
            .filter(|video| self.on.contains(&video.key) && !self.set.contains_key(&video.key))
            .filter_map(Video::number)
            .min()
    }

    /// When the names number past the anime's last episode, they likely go
    /// on from an earlier season: the lowest goes to episode 1.
    pub fn fit_offset(&mut self) {
        let numbers: Vec<i64> = self.videos.iter().filter(|video| self.on.contains(&video.key)).filter_map(Video::number).collect();
        let total = self.chosen.as_ref().and_then(Chosen::total);
        self.offset = match (total, numbers.iter().max(), numbers.iter().min()) {
            (Some(total), Some(&highest), Some(&lowest)) if highest > total => 1 - lowest,
            _ => 0,
        };
    }

    /// Align the names so the lowest number goes to `episode`.
    pub fn align(&mut self, episode: i64) {
        if let Some(lowest) = self.lowest() {
            self.offset = episode - lowest;
        }
    }

    /// Match to `anime`, which has `count` episodes as far as known.
    pub fn choose(&mut self, anime: Anime, count: i64) {
        if self.chosen.as_ref().is_some_and(|chosen| chosen.anime.dandanplay_id == anime.dandanplay_id) {
            return;
        }
        self.chosen = Some(Chosen { anime, count, episodes: None });
        if !self.single {
            self.fit_offset();
        }
    }

    /// The anime's episodes came.
    pub fn details(&mut self, details: &Details) {
        let Some(chosen) = self.chosen.as_mut().filter(|chosen| chosen.anime.dandanplay_id == details.anime_id) else {
            return;
        };
        if chosen.anime.image.is_empty() {
            chosen.anime.image = details.image_url.clone();
        }
        chosen.episodes = Some(
            details
                .episodes
                .iter()
                .filter_map(|episode| {
                    let number = episode.episode_number.trim().parse().ok()?;
                    Some((number, Episode { dandanplay_id: episode.episode_id, number, title: episode.episode_title.clone() }))
                })
                .collect(),
        );
        if !self.single {
            self.fit_offset();
        }
    }

    pub fn toggle(&mut self, key: i64) {
        if !self.on.remove(&key) && self.videos.iter().any(|video| video.key == key && video.file.is_some()) {
            self.on.insert(key);
        }
    }

    /// The number typed for `key`; nothing typed goes back to the name's.
    pub fn type_number(&mut self, key: i64, number: Option<i64>) {
        match number {
            Some(number) => {
                self.set.insert(key, number);
                if self.videos.iter().any(|video| video.key == key && video.file.is_some()) {
                    self.on.insert(key);
                }
            }
            None => {
                self.set.remove(&key);
            }
        }
        self.editing = None;
    }

    /// The files to link and their episodes, when everything to match has
    /// an episode the anime has.
    pub fn links(&self) -> Option<(Anime, Vec<(FileId, Episode)>)> {
        let chosen = self.chosen.as_ref()?;
        let values = self.values();
        let total = chosen.total();
        let links: Vec<(FileId, Episode)> = self
            .videos
            .iter()
            .filter(|video| self.on.contains(&video.key))
            .map(|video| {
                let number = values.get(&video.key).copied().flatten().filter(|number| *number > 0 && total.is_none_or(|total| *number <= total))?;
                Some((video.file?, chosen.episode(number)))
            })
            .collect::<Option<_>>()?;
        (!links.is_empty()).then(|| (chosen.anime.clone(), links))
    }

    /// The page as the view shows it.
    pub fn view(&self) -> Matching {
        let values = self.values();
        let total = self.chosen.as_ref().and_then(Chosen::total);
        let title = self.chosen.as_ref().map(|chosen| chosen.anime.title.clone()).unwrap_or_default();
        let mut taken = HashSet::new();
        let mut bad_count = 0;
        let rows: Vec<MatchRow> = self
            .videos
            .iter()
            .map(|video| {
                let on = self.on.contains(&video.key);
                let value = values.get(&video.key).copied().flatten();
                let twin = on && value.is_some_and(|value| !taken.insert(value));
                let beyond = on && value.zip(total).is_some_and(|(value, total)| value > total);
                let (note, bad) = if video.file.is_none() {
                    (MatchNote::Unreadable, false)
                } else if on && value.is_none() {
                    (MatchNote::NoNumber, true)
                } else if beyond {
                    (MatchNote::Beyond { anime: title.clone(), total: total.unwrap_or(0) }, true)
                } else if twin {
                    (MatchNote::Twin, false)
                } else {
                    match &video.now {
                        Some((anime, episode)) => (MatchNote::Now { anime: anime.title.clone(), episode: *episode }, false),
                        None => (MatchNote::Unmatched, false),
                    }
                };
                bad_count += bad as i64;
                MatchRow { key: video.key, name: video.name.clone(), on, number: value.unwrap_or(0), editing: self.editing == Some(video.key), note, odd: !on || bad || twin }
            })
            .collect();
        let on_count = rows.iter().filter(|row| row.on).count() as i64;
        let odd_count = rows.iter().filter(|row| row.odd).count() as i64;
        let lowest = if self.single || self.by_order { None } else { self.lowest() };
        let chosen_id = self.chosen.as_ref().map_or(0, |chosen| chosen.anime.dandanplay_id);
        Matching {
            open: true,
            single: self.single,
            place: self.place.clone(),
            anime: chosen_id,
            anime_title: title,
            episodes: total.unwrap_or(0),
            by_order: self.by_order,
            start: self.start,
            lowest: lowest.unwrap_or(0),
            align_to: lowest.map_or(0, |lowest| lowest + self.offset),
            shown: if self.odd_only { rows.iter().filter(|row| row.odd).cloned().collect() } else { rows.clone() },
            all_count: rows.len() as i64,
            odd_count,
            on_count,
            skipped_count: rows.len() as i64 - on_count,
            bad_count,
            ready: self.chosen.is_some() && on_count > 0 && bad_count == 0 && self.links().is_some(),
            linked: self.single && self.videos[0].now.is_some(),
            results: self
                .results
                .iter()
                .map(|found| MatchAnime { id: found.anime_id, title: found.anime_title.clone(), episodes: found.episode_count, chosen: found.anime_id == chosen_id })
                .collect(),
            searching: self.searching,
            search_failed: self.search_failed,
            odd_only: self.odd_only,
            rows,
        }
    }
}

/// The page while nothing is being matched.
pub fn closed() -> Matching {
    Matching {
        open: false,
        single: false,
        place: String::new(),
        anime: 0,
        anime_title: String::new(),
        episodes: 0,
        by_order: false,
        start: 1,
        lowest: 0,
        align_to: 0,
        rows: Vec::new(),
        shown: Vec::new(),
        odd_only: false,
        all_count: 0,
        odd_count: 0,
        on_count: 0,
        skipped_count: 0,
        bad_count: 0,
        ready: false,
        linked: false,
        results: Vec::new(),
        searching: false,
        search_failed: false,
    }
}
