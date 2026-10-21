//! Engine of the anime module. The season's anime come from dandanplay,
//! with the airing times of bangumi-data; the details of a show from
//! dandanplay too; what is being watched from the app's database, where the
//! shell links the videos it plays to their episodes. Requests run on other
//! threads and come back through `wake`; what was fetched last is kept in
//! the cache folder, so the pages show it at once on the next start.

mod clock;
mod schedule;
mod season;
mod shows;
mod timeline;

use bern::CommandError;
use bern::media::MediaSource;
use clock::Clock;
use erika_anime_protocol::anime::{
    self, BackTo, NextAction, Show, State, WatchStatus,
};
use erika_dandanplay::{Details, Intro};
use schedule::Schedule;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The most shows the stage and the rows beside it hold.
const WATCHING: usize = 5;
/// The season's list as dandanplay gave it last, in the cache folder.
const SEASON_CACHE: &str = "dandanplay-season.json";

/// What the requests came back with.
enum Done {
    Season(Result<Vec<Intro>, erika_dandanplay::Error>),
    Schedule(i64, Vec<schedule::Slot>),
    Details(i64, Result<Details, erika_dandanplay::Error>),
}

pub struct AnimeEngine {
    state: State,
    db: erika_store::Db,
    season: Vec<Intro>,
    schedule: Schedule,
    details: HashMap<i64, Details>,
    watching: Vec<erika_store::Watching>,
    /// What the user chose for a show here: where they are with it, and
    /// its score.
    statuses: HashMap<i64, WatchStatus>,
    ratings: HashMap<i64, f64>,
    inbox: Arc<Mutex<Vec<Done>>>,
    /// Whether the first wake, as the app starts, has come.
    started: bool,
}

impl Default for AnimeEngine {
    fn default() -> Self {
        let signed_in = erika_dandanplay::token().is_some();
        let db = erika_store::Db::open();
        let mut engine = Self {
            season: if signed_in { cached_season() } else { Vec::new() },
            schedule: Schedule::cached().unwrap_or_default(),
            watching: db.watching(WATCHING),
            db,
            details: HashMap::new(),
            statuses: HashMap::new(),
            ratings: HashMap::new(),
            inbox: Arc::default(),
            started: false,
            state: State {
                signed_in,
                loading: false,
                error: String::new(),
                season_label: String::new(),
                has_stage: false,
                has_airing: false,
                today: bern::date::CalendarDate::new(1970, 1, 1),
                stage: shows::nothing_watched(),
                also: Vec::new(),
                airing: Vec::new(),
                breaks: Vec::new(),
                now_at: 0.0,
                track_width: timeline::FLOOR,
                track_wide: timeline::FLOOR_WIDE,
                now: 0,
                aired_count: 0,
                later_count: 0,
                week: Vec::new(),
                selected_day: 0,
                opened_from_schedule: false,
                opened: 0,
                show: shows::untitled(),
                episodes: Vec::new(),
                next_episode: 1,
                next: NextAction::Start,
                back: BackTo::Home,
            },
        };
        engine.show_season();
        engine.show_watching();
        engine
    }
}

fn cached_season() -> Vec<Intro> {
    bern::storage::cache_dir()
        .and_then(|dir| std::fs::read(dir.join(SEASON_CACHE)).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn keep_season(season: &[Intro]) {
    if let (Some(dir), Ok(bytes)) = (bern::storage::cache_dir(), serde_json::to_vec(season)) {
        let _ = std::fs::write(dir.join(SEASON_CACHE), bytes);
    }
}

/// Run `work` on another thread; its result waits in `inbox`, and the
/// engine is woken to take it in. The browser has no threads: there the
/// work runs at once.
fn in_background(inbox: &Arc<Mutex<Vec<Done>>>, work: impl FnOnce() -> Option<Done> + Send + 'static) {
    let inbox = Arc::clone(inbox);
    let waker = bern::wake::waker();
    let run = move || {
        if let Some(done) = work() {
            inbox.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(done);
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    };
    if cfg!(target_arch = "wasm32") {
        run();
    } else {
        std::thread::spawn(run);
    }
}

impl AnimeEngine {
    /// The week and today's timeline, as of now.
    fn show_season(&mut self) {
        let season = season::build(&self.season, &self.schedule, &Clock::now());
        let state = &mut self.state;
        state.today = season.today;
        state.season_label = season.label;
        state.week = season.week;
        state.has_airing = !season.timeline.groups.is_empty();
        state.airing = season.timeline.groups;
        state.breaks = season.timeline.breaks;
        state.now_at = season.timeline.now_at;
        state.track_width = season.timeline.width;
        state.track_wide = season.timeline.wide;
        state.now = season.now;
        state.aired_count = season.aired;
        state.later_count = season.later;
    }

    /// The stage and the rows beside it, from what was played last.
    fn show_watching(&mut self) {
        let mut rows = self.watching.iter().map(shows::watching);
        let stage = rows.next();
        self.state.has_stage = stage.is_some();
        self.state.stage = stage.unwrap_or_else(shows::nothing_watched);
        self.state.also = rows.collect();
    }

    /// Ask dandanplay for the season's list, and bangumi-data for the
    /// times when what is kept of them is old.
    fn refresh(&mut self) {
        if !self.state.signed_in {
            return;
        }
        self.state.loading = true;
        self.state.error.clear();
        in_background(&self.inbox, || Some(Done::Season(erika_dandanplay::shin())));
        if erika_store::now() - self.schedule.fetched > schedule::STALE_AFTER {
            #[cfg(not(target_arch = "wasm32"))]
            in_background(&self.inbox, || {
                let now = erika_store::now();
                match schedule::fetch() {
                    Ok(json) => Some(Done::Schedule(now, schedule::parse(&json, now))),
                    Err(error) => {
                        eprintln!("erika: bangumi-data: {error}");
                        None
                    }
                }
            });
        }
    }

    /// The show `id` as far as it is known: its details, the season's list,
    /// or what is being watched.
    fn find(&self, id: i64) -> Result<Show, CommandError> {
        let files = self.db.episode_files(id);
        let show = if let Some(details) = self.details.get(&id) {
            shows::from_details(details, &files)
        } else if let Some(intro) = self.season.iter().find(|intro| intro.anime_id == id) {
            shows::from_intro(intro, self.state.today)
        } else if let Some(watching) = self.watching.iter().find(|watching| watching.anime.dandanplay_id == id) {
            shows::from_watching(watching, self.state.today)
        } else if let Some(anime) = self.db.anime(id) {
            shows::from_anime(&anime, self.state.today)
        } else {
            return Err(CommandError::new("unknown_show", format!("no show {id}")));
        };
        Ok(Show {
            status: self.statuses.get(&id).cloned().unwrap_or(show.status),
            rating: self.ratings.get(&id).copied().unwrap_or(show.rating),
            ..show
        })
    }

    /// Fill the open show with what is known of it now.
    fn show_opened(&mut self) {
        let id = self.state.opened;
        if id == 0 {
            return;
        }
        let Ok(show) = self.find(id) else {
            return;
        };
        let files = self.db.episode_files(id);
        let (episodes, next) = shows::film(self.details.get(&id), &show, &files);
        self.state.next_episode = next;
        self.state.next = if show.watched == 0 { NextAction::Start } else { NextAction::Continue { episode: next } };
        self.state.episodes = episodes;
        self.state.show = show;
    }

    fn take_done(&mut self) {
        let done: Vec<Done> = std::mem::take(&mut *self.inbox.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
        for done in done {
            match done {
                Done::Season(Ok(season)) => {
                    self.state.loading = false;
                    for intro in &season {
                        self.db.remember_anime(&erika_store::Anime {
                            dandanplay_id: intro.anime_id,
                            title: intro.anime_title.clone(),
                            image: intro.image_url.clone(),
                        });
                    }
                    keep_season(&season);
                    self.season = season;
                    self.show_season();
                }
                Done::Season(Err(error)) => {
                    self.state.loading = false;
                    self.state.error = error.message;
                }
                Done::Schedule(fetched, slots) => {
                    schedule::keep(fetched, &slots);
                    self.schedule = Schedule::new(fetched, slots);
                    self.show_season();
                }
                Done::Details(id, Ok(details)) => {
                    self.db.remember_anime(&erika_store::Anime {
                        dandanplay_id: id,
                        title: details.anime_title.clone(),
                        image: details.image_url.clone(),
                    });
                    self.details.insert(id, details);
                    if self.state.opened == id {
                        self.show_opened();
                    }
                }
                Done::Details(id, Err(error)) => {
                    eprintln!("erika: details of {id}: {}", error.message);
                }
            }
        }
    }

    fn watching_of(&self, id: i64) -> Option<&erika_store::Watching> {
        self.watching.iter().find(|watching| watching.anime.dandanplay_id == id)
    }

    /// Ask the shell to play what is being watched of show `id`.
    fn play_watching(&self, watching: &erika_store::Watching) {
        let row = shows::watching(watching);
        anime::emit::play(row.title, row.episode, row.still, row.progress, MediaSource::new(watching.url.clone()));
    }
}

impl anime::Commands for AnimeEngine {
    fn open(&mut self, id: i64) -> Result<(), CommandError> {
        self.find(id)?;
        self.state.opened_from_schedule = false;
        self.state.back = BackTo::Home;
        self.state.opened = id;
        self.state.episodes = Vec::new();
        self.show_opened();
        if !self.details.contains_key(&id) && self.state.signed_in {
            in_background(&self.inbox, move || Some(Done::Details(id, erika_dandanplay::details(id))));
        }
        Ok(())
    }

    fn select_day(&mut self, day: i64) -> Result<(), CommandError> {
        if !(0..7).contains(&day) {
            return Err(CommandError::new("unknown_day", format!("no broadcast day {day}")));
        }
        self.state.selected_day = day;
        Ok(())
    }

    fn open_scheduled(&mut self, id: i64) -> Result<(), CommandError> {
        self.open(id)?;
        self.state.opened_from_schedule = true;
        self.state.back = BackTo::Schedule;
        Ok(())
    }

    fn open_from_library(&mut self, id: i64) -> Result<(), CommandError> {
        self.open(id)?;
        self.state.back = BackTo::Library;
        Ok(())
    }

    fn close(&mut self) -> Result<(), CommandError> {
        self.state.opened = 0;
        Ok(())
    }

    fn open_from_search(&mut self, id: i64) -> Result<(), CommandError> {
        self.open(id)?;
        self.state.back = BackTo::Search;
        Ok(())
    }

    fn continue_from_search(&mut self, id: i64) -> Result<(), CommandError> {
        match self.watching_of(id).cloned() {
            Some(watching) => {
                self.play_watching(&watching);
                Ok(())
            }
            None => self.open_from_search(id),
        }
    }

    fn stage(&mut self, id: i64) -> Result<(), CommandError> {
        let index = self
            .watching
            .iter()
            .skip(1)
            .position(|watching| watching.anime.dandanplay_id == id)
            .ok_or_else(|| CommandError::new("unknown_show", format!("show {id} is not beside the stage")))?;
        self.watching.swap(0, index + 1);
        self.show_watching();
        Ok(())
    }

    fn resume(&mut self, id: i64) -> Result<(), CommandError> {
        let watching = self
            .watching_of(id)
            .cloned()
            .ok_or_else(|| CommandError::new("unknown_show", format!("show {id} is not being watched")))?;
        self.play_watching(&watching);
        Ok(())
    }

    fn set_status(&mut self, status: WatchStatus) -> Result<(), CommandError> {
        self.statuses.insert(self.state.show.id, status.clone());
        self.state.show.status = status;
        Ok(())
    }

    fn rate(&mut self, score: i64) -> Result<(), CommandError> {
        let score = score.clamp(1, 10) as f64;
        self.ratings.insert(self.state.show.id, score);
        self.state.show.rating = score;
        Ok(())
    }

    fn play_episode(&mut self, number: i64) -> Result<(), CommandError> {
        let episode = self
            .state
            .episodes
            .iter()
            .find(|episode| episode.number == number)
            .ok_or_else(|| CommandError::new("unknown_episode", format!("no episode {number}")))?;
        let file = self
            .db
            .episode_files(self.state.show.id)
            .into_iter()
            .find(|file| file.episode.number == number);
        let (source, progress) = match file {
            Some(file) => (
                MediaSource::new(file.url),
                if file.duration > 0.0 { file.position / file.duration } else { 0.0 },
            ),
            None => (MediaSource::new(""), 0.0),
        };
        anime::emit::play(self.state.show.title.clone(), number, episode.still.clone(), progress, source);
        Ok(())
    }

    fn account_changed(&mut self, signed_in: bool) -> Result<(), CommandError> {
        self.state.signed_in = signed_in;
        if signed_in {
            self.refresh();
        } else {
            self.season.clear();
            self.details.clear();
            self.state.error.clear();
            self.state.loading = false;
            keep_season(&[]);
            self.show_season();
        }
        Ok(())
    }

    fn history_changed(&mut self) -> Result<(), CommandError> {
        self.watching = self.db.watching(WATCHING);
        self.show_watching();
        self.show_opened();
        Ok(())
    }
}

impl anime::Engine for AnimeEngine {
    fn state(&self) -> &State {
        &self.state
    }

    /// Requests come back; as the app starts, the season is fetched anew.
    fn wake(&mut self) {
        self.take_done();
        if !self.started {
            self.started = true;
            self.refresh();
        }
    }
}

impl anime::Queries for AnimeEngine {
    /// Shows by name: dandanplay's search while signed in, the season's
    /// list otherwise; and, among them, what can be continued.
    fn search(&self, text: String, answer: bern::prelude::Answer<anime::SearchResults>) {
        let text = text.trim().to_string();
        let today = self.state.today;
        let watching = self.watching.clone();
        let local: Vec<Show> = self
            .season
            .iter()
            .filter(|intro| text.is_empty() || intro.anime_title.to_lowercase().contains(&text.to_lowercase()))
            .take(6)
            .map(|intro| shows::from_intro(intro, today))
            .collect();
        if text.is_empty() || !self.state.signed_in {
            answer.ok(shows::results(local, &watching));
            return;
        }
        let cancelled = answer.cancellation();
        let run = move || {
            if cancelled.is_cancelled() {
                return;
            }
            let shows = match erika_dandanplay::search(&text) {
                Ok(found) => found.iter().take(6).map(|found| shows::from_found(found, today)).collect(),
                Err(_) => local,
            };
            answer.ok(shows::results(shows, &watching));
        };
        if cfg!(target_arch = "wasm32") {
            run();
        } else {
            std::thread::spawn(run);
        }
    }
}

#[cfg(test)]
mod tests;
