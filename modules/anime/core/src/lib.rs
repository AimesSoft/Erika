//! Anime membership comes from the available dandanplay or Bangumi calendar,
//! broadcast times from bangumi-data, and display metadata from TMDB. Progress
//! is linked to canonical media in the database. Requests run on native workers
//! or browser Fetch and return through the originating engine's `wake` callback.
//! Calendar caches use the database: persisted on native platforms and shared
//! within the current browser session.

mod catalog;
mod clock;
mod content;
mod marks;
mod metadata;
mod provider_editor;
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
use erika_dandanplay::Intro;
use marks::Level;
use schedule::Schedule;
use std::sync::{Arc, Mutex};
use sync::Intent;

/// The most shows the stage and the rows beside it hold.
const WATCHING: usize = 5;
fn season_cache(source: &SeasonSource) -> &'static str {
    match source {
        SeasonSource::Dandanplay => "anime.calendar.dandanplay.v1",
        SeasonSource::Bangumi => "anime.calendar.bangumi.v1",
    }
}

/// What the requests came back with.
enum Done {
    Season(u64, Result<Vec<Intro>, erika_dandanplay::Error>),
    Metadata(metadata::Finished),
    Schedule(Result<(i64, Vec<schedule::Slot>), String>),
    Reviews(reviews::Finished),
    Synced(sync::Synced),
    RetryWatches,
}

pub struct AnimeEngine {
    state: State,
    accounts: catalog::Accounts,
    preferred_season_source: SeasonSource,
    links: catalog::Links,
    generation: u64,
    db: erika_store::Db,
    season: Vec<Intro>,
    schedule: Schedule,
    schedule_loading: bool,
    watching: Vec<erika_store::catalog::MediaWatching>,
    syncs: sync::Syncs,
    inbox: Arc<Mutex<Vec<Done>>>,
    /// Whether the first wake, as the app starts, has come.
    started: bool,
    /// The minute, since 1970, the timeline was laid out at.
    minute: i64,
    reviews: reviews::Runtime,
    metadata: metadata::Runtime,
    provider_editor: provider_editor::Editor,
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
        let season = cached_season(&db, &season_source);
        let schedule = Schedule::cached(&db).unwrap_or_default();
        Self::new(
            db,
            accounts,
            preferred_season_source,
            season,
            schedule,
            catalog::Links::cached(),
        )
    }
}

impl AnimeEngine {
    fn new(
        db: erika_store::Db,
        accounts: catalog::Accounts,
        preferred_season_source: SeasonSource,
        season: Vec<Intro>,
        schedule: Schedule,
        links: catalog::Links,
    ) -> Self {
        let season_source = accounts.season_source(&preferred_season_source);
        let mut engine = Self {
            accounts,
            preferred_season_source,
            links,
            generation: 0,
            season,
            schedule,
            schedule_loading: false,
            watching: db.catalog_watching(WATCHING).unwrap_or_default(),
            db,
            syncs: sync::Syncs::default(),
            inbox: Arc::default(),
            started: false,
            minute: 0,
            reviews: reviews::Runtime::default(),
            metadata: metadata::Runtime::default(),
            provider_editor: provider_editor::Editor::default(),
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
                opened: String::new(),
                view_generation: erika_catalog::MediaId::new().0,
                show: shows::untitled(),
                details_loading: false,
                details_failed: false,
                details_error: String::new(),
                seasons: Vec::new(),
                selected_season: String::new(),
                selected_season_title: String::new(),
                episodes: Vec::new(),
                first_episode: String::new(),
                last_episode: String::new(),
                next_episode: String::new(),
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
                can_map: false,
                mapping_open: false,
                mapping_targets: Vec::new(),
                mapping_target: String::new(),
                mapping_movie: false,
                mapping_episode: false,
                mapping_rows: Vec::new(),
                mapping_error: String::new(),
            },
        };
        engine.show_season();
        engine.show_watching();
        engine
    }
}

fn cached_season(db: &erika_store::Db, source: &SeasonSource) -> Vec<Intro> {
    db.setting(season_cache(source))
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn keep_season(db: &erika_store::Db, source: &SeasonSource, season: &[Intro]) {
    if let Ok(text) = serde_json::to_string(season) {
        db.set_setting(season_cache(source), Some(&text));
    }
}

/// Capture the originating engine before spawning. An HTTP completion must not
/// keep a destroyed engine alive or wake whichever engine is currently active.
fn receiver(inbox: &Arc<Mutex<Vec<Done>>>) -> impl FnOnce(Done) + Send + 'static {
    let inbox = Arc::downgrade(inbox);
    let waker = bern::wake::waker();
    move |done| {
        if let Some(inbox) = inbox.upgrade() {
            inbox
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(done);
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }
}

impl AnimeEngine {
    /// The week and today's timeline, as of now.
    fn show_season(&mut self) {
        let clock = Clock::now();
        self.minute = clock.unix.div_euclid(60);
        let mut season = season::build(&self.season, &self.schedule, &clock);
        self.project_calendar(&mut season);
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
        let mut rows = self.watching.iter().map(|w| content::watching(&self.db, w));
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
            let cached = cached_season(&self.db, &source);
            // An account disappearing must not blank the existing calendar
            // while public data refreshes. An explicit choice clears it below.
            if !cached.is_empty() {
                self.season = cached;
            }
            self.state.season_source = source.clone();
            self.show_season();
        }
        let links = self.links.clone();
        self.load_metadata();
        self.state.loading = true;
        self.state.error.clear();
        let deliver = receiver(&self.inbox);
        erika_catalog::client::spawn(async move {
            deliver(Done::Season(
                generation,
                catalog::season(accounts, &source, &links).await,
            ));
        });
        self.refresh_opened();
        if !self.schedule_loading
            && erika_store::now() - self.schedule.fetched > schedule::STALE_AFTER
        {
            self.schedule_loading = true;
            let deliver = receiver(&self.inbox);
            erika_catalog::client::spawn(async move {
                let now = erika_store::now();
                deliver(Done::Schedule(
                    schedule::fetch(now).await.map(|slots| (now, slots)),
                ));
            });
        }
    }

    fn refresh_opened(&mut self) {
        self.refresh_metadata();
    }

    fn find_media(&self, id: &str) -> Result<Show, CommandError> {
        if let Some(show) = self.source_preview(id) {
            return Ok(show);
        }
        let node = self
            .db
            .catalog_node(
                &erika_catalog::MediaId::parse(id)
                    .map_err(|e| CommandError::new("unknown_show", e.message))?,
            )
            .map_err(content::command)?
            .filter(|n| n.kind.work())
            .ok_or_else(|| CommandError::new("unknown_show", "没有这部作品"))?;
        content::show(&self.db, &node).map_err(content::command)
    }

    /// Fill the open show with what is known of it now.
    fn show_opened(&mut self) {
        self.state.can_map = false;
        if self.state.opened.is_empty() {
            return;
        }
        if let Some(show) = self.source_preview(&self.state.opened) {
            self.state.show = show;
            self.state.seasons.clear();
            self.state.selected_season.clear();
            self.state.selected_season_title.clear();
            self.state.episodes.clear();
            self.state.first_episode.clear();
            self.state.last_episode.clear();
            self.state.next_episode.clear();
            self.state.my_score = Score::Unrated;
            self.state.can_rate = false;
            self.state.can_review = false;
            return;
        }
        let result = (|| {
            let node = self
                .db
                .catalog_node(&erika_catalog::MediaId(self.state.opened.clone()))?
                .ok_or_else(|| erika_store::catalog::Error::invalid("作品已不存在"))?;
            let show = content::show(&self.db, &node)?;
            let mut seasons = self
                .db
                .catalog_children(&node.id)?
                .into_iter()
                .filter(|n| n.kind == erika_catalog::Kind::Season)
                .map(|n| anime::SeasonChoice {
                    id: n.id.0,
                    title: n.title,
                    number: n.season_number.map_or(-1, i64::from),
                })
                .collect::<Vec<_>>();
            seasons.sort_by_key(|s| s.number);
            if !seasons.iter().any(|s| s.id == self.state.selected_season) {
                self.state.selected_season = seasons
                    .iter()
                    .find(|s| s.number > 0)
                    .or_else(|| seasons.first())
                    .map(|s| s.id.clone())
                    .unwrap_or_default();
            }
            self.state.selected_season_title = seasons
                .iter()
                .find(|s| s.id == self.state.selected_season)
                .map(|s| s.title.clone())
                .unwrap_or_default();
            self.state.seasons = seasons;
            let (episodes, next) = content::film(&self.db, &node, &self.state.selected_season)?;
            let marks = self.db.catalog_marks(&node.id)?;
            Ok::<_, erika_store::catalog::Error>((show, episodes, next, marks))
        })();
        match result {
            Ok((show, episodes, next, marks)) => {
                self.state.can_map = true;
                self.state.my_score = marks
                    .iter()
                    .filter(|m| m.reference.is_none())
                    .max_by_key(|m| m.updated_at)
                    .map_or(Score::Unrated, |m| marks::score(m.rating.unwrap_or(0)));
                self.state.next = if episodes
                    .iter()
                    .any(|e| e.id == next && !e.watched && e.progress > 0.)
                {
                    NextAction::Resume
                } else {
                    NextAction::Start
                };
                self.state.next_episode = next;
                self.state.first_episode =
                    episodes.first().map(|e| e.id.clone()).unwrap_or_default();
                self.state.last_episode = episodes.last().map(|e| e.id.clone()).unwrap_or_default();
                self.state.episodes = episodes;
                self.state.show = show;
                self.show_account_marks();
            }
            Err(error) => {
                self.state.details_failed = true;
                self.state.details_error = error.message;
            }
        }
    }

    /// An account signed in or out: what the details offer changes, and
    /// what was kept while it was out goes to it now.
    fn accounts_changed(&mut self, _signed_in: bool) {
        self.sync_all_again();
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
                Done::Metadata(result) => self.take_metadata(result),
                Done::Season(generation, _) if generation != self.generation => {
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
                    keep_season(&self.db, &self.state.season_source, &season);
                    self.season = season;
                    self.load_metadata();
                    self.show_season();
                }
                Done::Season(_, Err(error)) => {
                    self.state.loading = false;
                    self.state.error = error.message;
                }
                Done::Schedule(result) => {
                    self.schedule_loading = false;
                    match result {
                        Ok((fetched, slots)) => {
                            schedule::keep(&self.db, fetched, &slots);
                            self.schedule = Schedule::new(fetched, slots);
                            self.show_season();
                        }
                        Err(error) => eprintln!("erika: bangumi-data: {error}"),
                    }
                }
                Done::Reviews(result) => self.take_reviews(result),
                Done::Synced(synced) => self.take_synced(synced),
                Done::RetryWatches => self.resume_watch_retry(),
            }
        }
    }

    fn watching_of(&self, id: &str) -> Option<&erika_store::catalog::MediaWatching> {
        self.watching.iter().find(|w| w.work.id.0 == id)
    }

    /// Ask the shell to play what is being watched of show `id`, from the
    /// home page's stage when `from_stage`.
    fn play_watching(&self, watching: &erika_store::catalog::MediaWatching, from_stage: bool) {
        anime::emit::play_media(
            watching
                .episode
                .as_ref()
                .map(|e| e.id.0.clone())
                .unwrap_or_default(),
            MediaSource::new(watching.file.url.clone()),
            watching.file.position,
            from_stage,
        );
    }
}

impl anime::Commands for AnimeEngine {
    fn open(&mut self, id: String) -> Result<(), CommandError> {
        self.restore_metadata();
        let id = self.resolved_source(&id).map(|id| id.0).unwrap_or(id);
        self.find_media(&id)?;
        self.state.opened_from_schedule = false;
        self.state.back = BackTo::Home;
        self.state.opened = id;
        self.state.episodes = Vec::new();
        self.state.selected_season.clear();
        self.state.seasons.clear();
        self.state.sync_note.clear();
        self.state.rating_open = false;
        self.state.review_open = false;
        self.state.mapping_open = false;
        self.state.review_text.clear();
        self.state.review_error.clear();
        self.syncs.change_view(&mut self.state);
        self.state.review_loading = false;
        self.state.review_saving = false;
        self.show_opened();
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

    fn open_scheduled(&mut self, id: String) -> Result<(), CommandError> {
        self.open(id)?;
        self.state.opened_from_schedule = true;
        self.state.back = BackTo::Schedule;
        Ok(())
    }

    fn open_from_library(&mut self, id: String) -> Result<(), CommandError> {
        self.open(id)?;
        self.state.back = BackTo::Library;
        Ok(())
    }

    fn close(&mut self) -> Result<(), CommandError> {
        self.state.opened.clear();
        self.metadata.cancel_opened();
        self.syncs.change_view(&mut self.state);
        self.state.review_loading = false;
        self.state.review_saving = false;
        self.state.rating_open = false;
        self.state.review_open = false;
        self.state.mapping_open = false;
        self.reviews.cancel();
        Ok(())
    }

    fn open_from_search(&mut self, id: String) -> Result<(), CommandError> {
        self.open(id)?;
        self.state.back = BackTo::Search;
        Ok(())
    }

    fn continue_from_search(&mut self, id: String) -> Result<(), CommandError> {
        match self.watching_of(&id).cloned() {
            Some(watching) => {
                self.play_watching(&watching, false);
                Ok(())
            }
            None => self.open_from_search(id),
        }
    }

    fn stage(&mut self, id: String) -> Result<(), CommandError> {
        let index = self
            .watching
            .iter()
            .skip(1)
            .position(|watching| watching.work.id.0 == id)
            .ok_or_else(|| {
                CommandError::new("unknown_show", format!("show {id} is not beside the stage"))
            })?;
        self.watching.swap(0, index + 1);
        self.show_watching();
        Ok(())
    }

    fn resume(&mut self, id: String) -> Result<(), CommandError> {
        let watching = self.watching_of(&id).cloned().ok_or_else(|| {
            CommandError::new("unknown_show", format!("show {id} is not being watched"))
        })?;
        self.play_watching(&watching, true);
        Ok(())
    }

    fn set_status(&mut self, status: WatchStatus) -> Result<(), CommandError> {
        if let Some(level) = Level::of_status(&status) {
            self.sync_intent(Intent::Status(level))?;
        }
        Ok(())
    }

    fn rate(&mut self, score: i64) -> Result<(), CommandError> {
        self.sync_intent(Intent::Rate(score))
    }

    fn pick_score(&mut self, score: Score) -> Result<(), CommandError> {
        self.rate(marks::points(&score))?;
        self.state.rating_open = false;
        Ok(())
    }

    fn show_rating(&mut self, open: bool) -> Result<(), CommandError> {
        self.state.rating_open = open && !self.state.opened.is_empty();
        Ok(())
    }

    fn show_review(&mut self, open: bool) -> Result<(), CommandError> {
        self.syncs.change_review();
        self.state.review_error.clear();
        self.state.review_open = open && !self.state.opened.is_empty();
        self.state.review_loading = false;
        self.state.review_saving = false;
        if self.state.review_open {
            self.state.review_loading = true;
            if let Err(error) = self.sync_intent(Intent::ReadReview) {
                self.state.review_loading = false;
                self.state.review_error = error.message;
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
        self.syncs.change_review();
        self.state.review_error.clear();
        self.state.review_loading = false;
        self.state.review_saving = true;
        if let Err(error) = self.sync_intent(Intent::Review(text)) {
            self.state.review_saving = false;
            self.state.review_error = error.message.clone();
            return Err(error);
        }
        Ok(())
    }

    fn mark_episode(&mut self, id: String, watched: bool) -> Result<(), CommandError> {
        if !self.state.episodes.iter().any(|e| e.id == id) {
            return Err(CommandError::new("unknown_episode", "没有这条影片或分集"));
        }
        let media = erika_catalog::MediaId(id);
        let previous = self.db.catalog_watched(&media).map_err(content::command)?;
        self.db
            .catalog_set_watched(&erika_catalog::Watched {
                media,
                watched,
                manual: true,
                updated_at: erika_store::now(),
                revision: previous.map_or(0, |w| w.revision),
            })
            .map_err(content::command)?;
        self.sync_played();
        self.show_opened();
        Ok(())
    }

    /// For the plugin system: a plugin that offers marking single episodes
    /// on the film turns their buttons on here, and off when it is turned
    /// off. The buttons call `mark_episode`.
    fn set_episode_marking(&mut self, enabled: bool) -> Result<(), CommandError> {
        self.state.episode_marking = enabled;
        Ok(())
    }

    fn show_mappings(&mut self, open: bool) -> Result<(), CommandError> {
        if !open {
            self.state.mapping_open = false;
            self.provider_editor = Default::default();
            return Ok(());
        }
        let result = self.open_provider_editor();
        self.mapping_result(result)
    }

    fn select_mapping_target(&mut self, id: String) -> Result<(), CommandError> {
        let result = self.select_provider_target(&id);
        self.mapping_result(result)
    }

    fn save_provider_mapping(
        &mut self,
        provider: anime::MappingProvider,
        kind: anime::MappingKind,
        relation: anime::MappingRelation,
        reference: String,
    ) -> Result<(), CommandError> {
        let result = self.save_mapping(provider, kind, relation, &reference);
        self.mapping_result(result)
    }

    fn remove_provider_mapping(&mut self, row: String) -> Result<(), CommandError> {
        let result = self.remove_mapping(&row);
        self.mapping_result(result)
    }

    fn sync_marks(&mut self) -> Result<(), CommandError> {
        self.sync_intent(Intent::Read)?;
        if let Ok(media) = self.account_media() {
            self.sync_stored_marks(Some(&media));
        }
        self.retry_watches();
        Ok(())
    }

    fn select_season(&mut self, id: String) -> Result<(), CommandError> {
        if !self.state.seasons.iter().any(|s| s.id == id) {
            return Err(CommandError::new("unknown_season", "没有这一季"));
        }
        self.syncs.change_view(&mut self.state);
        self.state.review_open = false;
        self.state.mapping_open = false;
        self.state.rating_open = false;
        self.state.review_loading = false;
        self.state.review_saving = false;
        self.state.review_text.clear();
        self.state.selected_season = id;
        self.show_opened();
        self.refresh_metadata();
        self.load_reviews();
        self.sync_opened();
        Ok(())
    }

    fn play_episode(&mut self, id: String) -> Result<(), CommandError> {
        if !self.state.episodes.iter().any(|e| e.id == id) {
            return Err(CommandError::new("unknown_episode", "没有这条影片或分集"));
        }
        let file = content::file(&self.db, &erika_catalog::MediaId(id.clone()))
            .map_err(content::command)?;
        let Some(file) = file else {
            return Err(CommandError::new("no_file", "没有可播放的文件"));
        };
        let position = file.position();
        anime::emit::play_media(
            if file.span.is_some() {
                id
            } else {
                String::new()
            },
            MediaSource::new(file.file.url),
            position,
            false,
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
            self.season = cached_season(&self.db, &source);
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
        self.watching = self
            .db
            .catalog_watching(WATCHING)
            .map_err(content::command)?;
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
            self.sync_all_again();
        }
    }
}

impl anime::Queries for AnimeEngine {
    fn search(&self, text: String, answer: bern::prelude::Answer<anime::SearchResults>) {
        self.search_media(text, answer);
    }
}

#[cfg(test)]
mod tests;
