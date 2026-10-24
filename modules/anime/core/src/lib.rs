//! Engine of the anime module. The season and details come from the available
//! dandanplay or Bangumi catalogue, with airing times from bangumi-data;
//! what is being watched comes from the app's database, where the
//! shell links the videos it plays to their episodes. Requests run on other
//! threads and come back through `wake`; what was fetched last is kept in
//! the cache folder, so the pages show it at once on the next start.

mod catalog;
mod clock;
mod detail_posters;
mod marks;
mod posters;
mod reviews;
mod schedule;
mod season;
mod shows;
mod sync;
mod timeline;

use bern::CommandError;
use bern::media::MediaSource;
use clock::Clock;
use erika_anime_protocol::anime::{
    self, BackTo, NextAction, Score, SeasonSource, Show, State, SyncTargets, WatchStatus,
};
use erika_dandanplay::{Details, Intro};
use marks::Level;
use schedule::Schedule;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use sync::Intent;

/// The most shows the stage and the rows beside it hold.
const WATCHING: usize = 5;
fn season_cache(source: &SeasonSource) -> &'static str {
    match source {
        SeasonSource::Dandanplay => "dandanplay-season.json",
        SeasonSource::Bangumi => "bangumi-season.json",
    }
}

/// What the requests came back with.
enum Done {
    Season(u64, Result<Vec<Intro>, erika_dandanplay::Error>),
    Schedule(i64, Vec<schedule::Slot>),
    Details(u64, i64, Result<Details, erika_dandanplay::Error>),
    Reviews(reviews::Finished),
    Poster(String),
    DetailPoster(detail_posters::Key),
    Synced(sync::Synced),
    /// The review on Bangumi, read as its dialog opened.
    Review(i64, Result<String, String>),
}

pub struct AnimeEngine {
    state: State,
    accounts: catalog::Accounts,
    preferred_season_source: SeasonSource,
    links: catalog::Links,
    generation: u64,
    search_results: Arc<Mutex<HashMap<i64, erika_dandanplay::Found>>>,
    db: erika_store::Db,
    season: Vec<Intro>,
    schedule: Schedule,
    details: HashMap<i64, Details>,
    watching: Vec<erika_store::Watching>,
    /// What is kept here of the shows seen: where the user is with each,
    /// its score and the episodes marked watched; and the syncs of them
    /// with the accounts.
    kept: HashMap<i64, sync::Kept>,
    syncs: sync::Syncs,
    inbox: Arc<Mutex<Vec<Done>>>,
    /// Whether the first wake, as the app starts, has come.
    started: bool,
    /// The minute, since 1970, the timeline was laid out at.
    minute: i64,
    reviews: reviews::Runtime,
    posters: posters::Downloads,
    detail_posters: detail_posters::Downloads,
}

impl Default for AnimeEngine {
    fn default() -> Self {
        let accounts = catalog::Accounts {
            dandanplay: erika_dandanplay::token().is_some(),
            bangumi: erika_dandanplay::reviews::bangumi_signed_in(),
        };
        let db = erika_store::Db::open();
        let preferred_season_source = catalog::saved_season_source(&db);
        let season_source = accounts.season_source(&preferred_season_source);
        let mut engine = Self {
            accounts,
            preferred_season_source,
            links: catalog::Links::cached(),
            generation: 0,
            search_results: Arc::default(),
            season: cached_season(&season_source),
            schedule: Schedule::cached().unwrap_or_default(),
            watching: db.watching(WATCHING),
            db,
            details: HashMap::new(),
            kept: HashMap::new(),
            syncs: sync::Syncs::default(),
            inbox: Arc::default(),
            started: false,
            minute: 0,
            reviews: reviews::Runtime::default(),
            posters: posters::Downloads::default(),
            detail_posters: detail_posters::Downloads::default(),
            state: State {
                signed_in: accounts.signed_in(),
                season_source,
                can_choose_season_source: accounts.can_choose(),
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
                details_loading: false,
                details_failed: false,
                details_error: String::new(),
                episodes: Vec::new(),
                next_episode: 1,
                next: NextAction::Start,
                back: BackTo::Home,
                comments: Vec::new(),
                comments_loading: false,
                has_comments: false,
                comments_source: String::new(),
                comments_error: String::new(),
                my_score: Score::Unrated,
                can_rate: accounts.signed_in(),
                fine_scores: accounts.bangumi,
                can_review: false,
                sync_targets: SyncTargets::Local,
                syncing: false,
                sync_note: String::new(),
                rating_open: false,
                review_open: false,
                review_loading: false,
                review_saving: false,
                review_text: String::new(),
                review_error: String::new(),
                episode_marking: false,
            },
        };
        engine.show_season();
        engine.show_watching();
        engine
    }
}

fn cached_season(source: &SeasonSource) -> Vec<Intro> {
    bern::storage::cache_dir()
        .and_then(|dir| std::fs::read(dir.join(season_cache(source))).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn keep_season(source: &SeasonSource, season: &[Intro]) {
    if let (Some(dir), Ok(bytes)) = (bern::storage::cache_dir(), serde_json::to_vec(season)) {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(season_cache(source)), bytes);
    }
}

/// Run `work` on another thread; its result waits in `inbox`, and the
/// engine is woken to take it in. The browser has no threads: there the
/// work runs at once.
fn in_background(
    inbox: &Arc<Mutex<Vec<Done>>>,
    work: impl FnOnce() -> Option<Done> + Send + 'static,
) {
    let inbox = Arc::clone(inbox);
    let waker = bern::wake::waker();
    let run = move || {
        if let Some(done) = work() {
            inbox
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(done);
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
        let clock = Clock::now();
        self.minute = clock.unix.div_euclid(60);
        let season = season::build(&self.season, &self.schedule, &clock);
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

    /// Ask the available catalogue for the season's list, and bangumi-data for the
    /// times when what is kept of them is old.
    fn refresh(&mut self) {
        self.generation += 1;
        let generation = self.generation;
        let accounts = self.accounts;
        let source = accounts.season_source(&self.preferred_season_source);
        self.state.can_choose_season_source = accounts.can_choose();
        if self.state.season_source != source {
            let cached = cached_season(&source);
            // An account disappearing must not blank the existing calendar
            // while public data refreshes. An explicit choice clears it below.
            if !cached.is_empty() {
                self.season = cached;
            }
            self.state.season_source = source.clone();
            self.show_season();
        }
        let links = self.links.clone();
        self.load_posters();
        self.state.loading = true;
        self.state.error.clear();
        in_background(&self.inbox, move || {
            Some(Done::Season(
                generation,
                catalog::season(accounts, &source, &links),
            ))
        });
        self.refresh_opened();
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

    fn refresh_opened(&mut self) {
        self.state.details_loading = false;
        self.state.details_failed = false;
        self.state.details_error.clear();
        let id = self.state.opened;
        if id == 0 || (id < 0 && self.details.contains_key(&id)) {
            return;
        }
        let generation = self.generation;
        self.state.details_loading = true;
        let accounts = self.accounts;
        let links = self.links.clone();
        let hint = self.details.get(&id).cloned();
        in_background(&self.inbox, move || {
            Some(Done::Details(
                generation,
                id,
                catalog::details(accounts, &links, id, hint.as_ref()),
            ))
        });
    }

    /// The show `id` as far as it is known: its details, the season's list,
    /// or what is being watched.
    fn find(&self, id: i64) -> Result<Show, CommandError> {
        let files = self.db.episode_files(id);
        let show = if let Some(details) = self.details.get(&id) {
            shows::from_details(details, &files)
        } else if let Some(intro) = self.season.iter().find(|intro| intro.anime_id == id) {
            shows::from_intro(intro, self.state.today)
        } else if let Some(watching) = self
            .watching
            .iter()
            .find(|watching| watching.anime.dandanplay_id == id)
        {
            shows::from_watching(watching, self.state.today)
        } else if let Some(anime) = self.db.anime(id) {
            shows::from_anime(&anime, self.state.today)
        } else if let Some(found) = self
            .search_results
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&id)
        {
            shows::from_found(found, self.state.today)
        } else {
            return Err(CommandError::new("unknown_show", format!("no show {id}")));
        };
        Ok(show)
    }

    /// Fill the open show with what is known of it now.
    fn show_opened(&mut self) {
        let id = self.state.opened;
        if id == 0 {
            return;
        }
        let Ok(mut show) = self.find(id) else {
            return;
        };
        if let Some(details) = self.details.get(&id)
            && let Some(poster) = detail_posters::cached(details)
        {
            show.poster = poster;
        }
        let files = self.db.episode_files(id);
        // What is watched and where the user is with it, as this device and
        // the accounts' last sync have it.
        let watched = self.watched_here(id);
        show.status = Level::status(self.level_of(id, &watched));
        let numbers: Vec<i64> = match self.details.get(&id) {
            Some(details) => shows::numbered(details)
                .into_iter()
                .map(|(n, _)| n)
                .collect(),
            None => files.iter().map(|file| file.episode.number).collect(),
        };
        show.watched = numbers.iter().filter(|n| watched.contains(n)).count() as i64;
        let kept = self.kept(id);
        self.state.my_score = marks::score(kept.mark.rating);
        self.state.can_rate = self.accounts.signed_in();
        self.state.fine_scores = self.accounts.bangumi;
        self.state.can_review = self.accounts.bangumi && self.subject(id).is_some();
        self.state.sync_targets = self.targets(id);
        self.state.syncing = self.syncs.busy(id);
        let (episodes, next) = shows::film(self.details.get(&id), &show, &files, &kept.watched);
        self.state.next_episode = next;
        self.state.next = if show.watched == 0 {
            NextAction::Start
        } else {
            NextAction::Continue { episode: next }
        };
        self.state.episodes = episodes;
        self.state.show = show;
    }

    /// An account signed in or out: what the details offer changes, and
    /// what was kept while it was out goes to it now.
    fn accounts_changed(&mut self, signed_in: bool) {
        if signed_in {
            self.sync_all_again();
        }
        self.show_opened();
    }

    fn take_done(&mut self) {
        let done: Vec<Done> = std::mem::take(
            &mut *self
                .inbox
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        for done in done {
            match done {
                Done::Season(generation, _) | Done::Details(generation, _, _)
                    if generation != self.generation =>
                {
                    continue;
                }
                Done::Season(_, Ok(season)) => {
                    self.state.loading = false;
                    for intro in &season {
                        self.db.remember_anime(&erika_store::Anime {
                            dandanplay_id: intro.anime_id,
                            title: intro.anime_title.clone(),
                            image: intro.image_url.clone(),
                        });
                    }
                    keep_season(&self.state.season_source, &season);
                    self.season = season;
                    self.load_posters();
                    self.show_season();
                }
                Done::Season(_, Err(error)) => {
                    self.state.loading = false;
                    self.state.error = error.message;
                }
                Done::Schedule(fetched, slots) => {
                    schedule::keep(fetched, &slots);
                    self.schedule = Schedule::new(fetched, slots);
                    self.show_season();
                }
                Done::Details(_, id, Ok(details)) => {
                    self.links.remember(&details);
                    reviews::keep_details(&details);
                    self.db.remember_anime(&erika_store::Anime {
                        dandanplay_id: id,
                        title: details.anime_title.clone(),
                        image: details.image_url.clone(),
                    });
                    self.details.insert(id, details);
                    self.load_posters();
                    if self.state.opened == id {
                        self.state.details_loading = false;
                        self.state.details_failed = false;
                        self.state.details_error.clear();
                        self.show_opened();
                        self.load_detail_poster();
                        self.sync_opened();
                    }
                }
                Done::Details(_, id, Err(error)) => {
                    eprintln!("erika: details of {id}: {}", error.message);
                    if self.state.opened == id {
                        self.state.details_loading = false;
                        self.state.details_failed = true;
                        self.state.details_error = error.message;
                    }
                }
                Done::Reviews(result) => self.take_reviews(result),
                Done::Poster(url) => {
                    self.posters.finished(&url, &self.inbox);
                    self.show_season();
                    self.show_opened();
                    self.show_watching();
                }
                Done::Synced(synced) => self.take_synced(synced),
                Done::Review(id, result) => {
                    if self.state.opened == id && self.state.review_open {
                        self.state.review_loading = false;
                        match result {
                            Ok(text) => self.state.review_text = text,
                            Err(error) => self.state.review_error = error,
                        }
                    }
                }
                Done::DetailPoster(key) => {
                    self.detail_posters.finished(&key);
                    if self
                        .details
                        .get(&self.state.opened)
                        .is_some_and(|d| detail_posters::Downloads::matches(&key, d))
                    {
                        self.show_opened();
                    }
                }
            }
        }
    }

    fn watching_of(&self, id: i64) -> Option<&erika_store::Watching> {
        self.watching
            .iter()
            .find(|watching| watching.anime.dandanplay_id == id)
    }

    fn load_posters(&mut self) {
        self.posters.add(
            self.details
                .values()
                .map(|d| d.image_url.clone())
                .chain(self.season.iter().map(|i| i.image_url.clone()))
                .chain(self.watching.iter().map(|w| w.anime.image.clone())),
            &self.inbox,
        );
    }

    fn load_detail_poster(&mut self) {
        if let Some(details) = self.details.get(&self.state.opened) {
            self.detail_posters.request(details, &self.inbox);
        }
    }

    /// Ask the shell to play what is being watched of show `id`.
    fn play_watching(&self, watching: &erika_store::Watching) {
        let row = shows::watching(watching);
        anime::emit::play(
            row.title,
            row.episode,
            row.still,
            row.progress,
            MediaSource::new(watching.url.clone()),
        );
    }
}

impl anime::Commands for AnimeEngine {
    fn open(&mut self, id: i64) -> Result<(), CommandError> {
        self.find(id)?;
        self.state.opened_from_schedule = false;
        self.state.back = BackTo::Home;
        self.state.opened = id;
        self.state.episodes = Vec::new();
        self.state.sync_note.clear();
        self.state.rating_open = false;
        self.state.review_open = false;
        self.state.review_text.clear();
        self.state.review_error.clear();
        self.syncs.opened = 0;
        if !self.details.contains_key(&id)
            && let Some(details) = reviews::cached_details(id)
        {
            self.details.insert(id, details);
        }
        self.show_opened();
        self.load_posters();
        self.load_detail_poster();
        self.refresh_opened();
        self.load_reviews();
        self.sync_opened();
        Ok(())
    }

    fn select_day(&mut self, day: i64) -> Result<(), CommandError> {
        if !(0..7).contains(&day) {
            return Err(CommandError::new(
                "unknown_day",
                format!("no broadcast day {day}"),
            ));
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
        self.state.rating_open = false;
        self.state.review_open = false;
        self.reviews.cancel();
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
            .ok_or_else(|| {
                CommandError::new("unknown_show", format!("show {id} is not beside the stage"))
            })?;
        self.watching.swap(0, index + 1);
        self.show_watching();
        Ok(())
    }

    fn resume(&mut self, id: i64) -> Result<(), CommandError> {
        let watching = self.watching_of(id).cloned().ok_or_else(|| {
            CommandError::new("unknown_show", format!("show {id} is not being watched"))
        })?;
        self.play_watching(&watching);
        Ok(())
    }

    fn set_status(&mut self, status: WatchStatus) -> Result<(), CommandError> {
        if let Some(level) = Level::of_status(&status) {
            self.sync(self.state.opened, vec![Intent::Status(level)], true);
        }
        Ok(())
    }

    fn rate(&mut self, score: i64) -> Result<(), CommandError> {
        self.sync(
            self.state.opened,
            vec![Intent::Rate(score.clamp(1, 10))],
            true,
        );
        Ok(())
    }

    fn pick_score(&mut self, score: Score) -> Result<(), CommandError> {
        let points = marks::points(&score);
        if points > 0 {
            self.rate(points)?;
        }
        self.state.rating_open = false;
        Ok(())
    }

    fn show_rating(&mut self, open: bool) -> Result<(), CommandError> {
        self.state.rating_open = open && self.state.opened != 0;
        Ok(())
    }

    fn show_review(&mut self, open: bool) -> Result<(), CommandError> {
        let id = self.state.opened;
        self.state.review_error.clear();
        self.state.review_open = open && id != 0;
        if !self.state.review_open {
            return Ok(());
        }
        match self.subject(id).filter(|_| self.accounts.bangumi) {
            Some(subject) => {
                self.state.review_loading = true;
                in_background(&self.inbox, move || {
                    let result = erika_dandanplay::bangumi::site::collection(subject)
                        .map(|collection| collection.comment)
                        .map_err(|error| error.message);
                    Some(Done::Review(id, result))
                });
            }
            None => {
                self.state.review_loading = false;
                self.state.review_error =
                    "要先登录 Bangumi，而且这部作品要有对应的 Bangumi 条目".into();
            }
        }
        Ok(())
    }

    fn submit_review(&mut self, text: String) -> Result<(), CommandError> {
        if !self.accounts.bangumi {
            return Err(CommandError::new(
                "bangumi_required",
                "请先登录 Bangumi 账号",
            ));
        }
        let text = text.trim().to_string();
        self.state.review_text = text.clone();
        let length = text.chars().count();
        if length > sync::REVIEW_LIMIT {
            self.state.review_error = format!(
                "Bangumi 的评价最多 {} 字，现在是 {length} 字",
                sync::REVIEW_LIMIT
            );
            return Ok(());
        }
        self.state.review_error.clear();
        self.state.review_saving = true;
        self.sync(self.state.opened, vec![Intent::Review(text)], true);
        Ok(())
    }

    fn mark_episode(&mut self, number: i64, watched: bool) -> Result<(), CommandError> {
        let id = self.state.opened;
        let known = self.numbered(id).iter().any(|(n, _)| *n == number)
            || self
                .state
                .episodes
                .iter()
                .any(|episode| episode.number == number);
        if id == 0 || !known {
            return Err(CommandError::new(
                "unknown_episode",
                format!("no episode {number}"),
            ));
        }
        if !watched && shows::played(&self.db.episode_files(id)).contains(&number) {
            // Played to the end here: it stays watched, here and there.
            self.state.sync_note = format!("第 {number} 话在本机已播放到结尾，仍算看过");
            return Ok(());
        }
        self.sync(id, vec![Intent::Mark { number, watched }], true);
        Ok(())
    }

    /// For the plugin system: a plugin that offers marking single episodes
    /// on the film turns their buttons on here, and off when it is turned
    /// off. The buttons call `mark_episode`.
    fn set_episode_marking(&mut self, enabled: bool) -> Result<(), CommandError> {
        self.state.episode_marking = enabled;
        Ok(())
    }

    fn sync_marks(&mut self) -> Result<(), CommandError> {
        self.sync(self.state.opened, Vec::new(), true);
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
                if file.duration > 0.0 {
                    file.position / file.duration
                } else {
                    0.0
                },
            ),
            None => (MediaSource::new(""), 0.0),
        };
        anime::emit::play(
            self.state.show.title.clone(),
            number,
            episode.still.clone(),
            progress,
            source,
        );
        Ok(())
    }

    fn account_changed(&mut self, signed_in: bool) -> Result<(), CommandError> {
        self.accounts.dandanplay = signed_in;
        self.state.signed_in = self.accounts.signed_in();
        self.refresh();
        self.load_reviews();
        self.accounts_changed(signed_in);
        Ok(())
    }

    fn bangumi_account_changed(&mut self, signed_in: bool) -> Result<(), CommandError> {
        self.accounts.bangumi = signed_in;
        self.state.signed_in = self.accounts.signed_in();
        self.refresh();
        self.load_reviews();
        self.accounts_changed(signed_in);
        Ok(())
    }

    fn set_season_source(&mut self, source: SeasonSource) -> Result<(), CommandError> {
        if !self.accounts.can_choose() {
            return Err(CommandError::new(
                "accounts_required",
                "请先登录弹弹play和 Bangumi 账号",
            ));
        }
        if self.preferred_season_source != source {
            catalog::save_season_source(&self.db, &source);
            self.season = cached_season(&source);
            self.state.season_source = source.clone();
            self.show_season();
            self.preferred_season_source = source;
            self.refresh();
        }
        Ok(())
    }

    fn refresh_comments(&mut self) -> Result<(), CommandError> {
        if !self.state.comments_loading {
            self.load_reviews();
        }
        Ok(())
    }

    fn refresh_details(&mut self) -> Result<(), CommandError> {
        if !self.state.details_loading {
            self.refresh_opened();
        }
        Ok(())
    }

    fn history_changed(&mut self) -> Result<(), CommandError> {
        // What is kept of the shows is read again: another engine (the
        // account sync) may have written it. The browser keeps it nowhere
        // else.
        if !cfg!(target_arch = "wasm32") {
            self.kept.clear();
        }
        self.watching = self.db.watching(WATCHING);
        self.show_watching();
        self.show_opened();
        self.sync_played();
        Ok(())
    }

    /// Lay the timeline out anew once the minute has turned.
    fn tick(&mut self) -> Result<(), CommandError> {
        if Clock::now().unix.div_euclid(60) != self.minute {
            self.show_season();
        }
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
            self.sync_played();
        }
    }
}

impl anime::Queries for AnimeEngine {
    /// Search the available catalogue, falling back to the cached season;
    /// among those results, include what can be continued.
    fn search(&self, text: String, answer: bern::prelude::Answer<anime::SearchResults>) {
        let text = text.trim().to_string();
        let today = self.state.today;
        let watching = self.watching.clone();
        let local: Vec<Show> = self
            .season
            .iter()
            .filter(|intro| {
                text.is_empty()
                    || intro
                        .anime_title
                        .to_lowercase()
                        .contains(&text.to_lowercase())
            })
            .take(6)
            .map(|intro| shows::from_intro(intro, today))
            .collect();
        if text.is_empty() {
            answer.ok(shows::results(local, &watching));
            return;
        }
        let cancelled = answer.cancellation();
        let accounts = self.accounts;
        let links = self.links.clone();
        let results = Arc::clone(&self.search_results);
        let run = move || {
            if cancelled.is_cancelled() {
                return;
            }
            let shows = match catalog::search(accounts, &links, &text) {
                Ok(found) => {
                    if cancelled.is_cancelled() {
                        return;
                    }
                    let mut results = results.lock().unwrap_or_else(|p| p.into_inner());
                    results.clear();
                    found
                        .into_iter()
                        .take(6)
                        .map(|found| {
                            let show = shows::from_found(&found, today);
                            results.insert(found.anime_id, found);
                            show
                        })
                        .collect()
                }
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
