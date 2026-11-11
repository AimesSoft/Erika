//! Which episode a video is. Once its file is known, dandanplay matches it
//! by its name and fingerprint, on another thread; the file is then linked
//! to the episode in the database, the player names the anime and the
//! episode, and dandanplay's danmaku of the episode come along. A file
//! matched before needs no request.

use super::*;
use erika_store::{FileId, Fingerprint};

/// What came back for the video playing.
pub(super) fn initial() -> PlayerMatching {
    PlayerMatching {
        open: false,
        target: MatchingTarget::Content,
        seasons: Vec::new(),
        season: String::new(),
        loading: false,
        error: String::new(),
        selected: None,
        shows: Vec::new(),
        episodes: Vec::new(),
    }
}

pub(super) fn danmaku_key(id: i64) -> String {
    if id < 0 {
        format!("misaka:{}", id.unsigned_abs())
    } else {
        format!("dandanplay:{id}")
    }
}

fn danmaku_id(key: &str) -> Result<i64, CommandError> {
    let (provider, value) = key
        .split_once(':')
        .ok_or_else(|| CommandError::new("invalid_id", "弹幕标识不正确"))?;
    let id = value
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| CommandError::new("invalid_id", "弹幕标识不正确"))?;
    match provider {
        "dandanplay" => Ok(id),
        "misaka" => Ok(-id),
        _ => Err(CommandError::new("invalid_id", "弹幕来源不正确")),
    }
}

fn search_show(show: erika_dandanplay::Found) -> MatchingShow {
    MatchingShow {
        id: danmaku_key(show.anime_id),
        name: show.anime_title,
        loading: show.summary.trim().is_empty() || show.image_url.is_empty(),
        poster: MediaSource {
            url: show.image_url,
        },
        summary: show.summary,
        kind: show.type_description,
        episode_count: show.episode_count,
    }
}

fn enrich_show(show: &mut MatchingShow, details: &erika_dandanplay::Details) {
    if !details.image_url.is_empty() {
        show.poster.url = details.image_url.clone();
    }
    if !details.summary.trim().is_empty() {
        show.summary = details.summary.clone();
    }
    if !details.type_description.is_empty() {
        show.kind = details.type_description.clone();
    }
    if !details.episodes.is_empty() {
        show.episode_count = details.episodes.len() as i64;
    }
    show.loading = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn details() -> erika_dandanplay::Details {
        erika_dandanplay::Details {
            bangumi_url: String::new(),
            anime_id: 42,
            anime_title: "匹配测试".into(),
            image_url: String::new(),
            summary: String::new(),
            rating: 0.0,
            tags: Vec::new(),
            type_description: String::new(),
            episodes: vec![erika_dandanplay::EpisodeInfo {
                episode_id: 420002,
                episode_number: "2".into(),
                episode_title: "第二集".into(),
                air_date: None,
                last_watched: None,
            }],
            is_on_air: false,
            air_day: None,
            is_favorited: false,
            user_rating: 0,
            favorite_status: None,
        }
    }

    fn show() -> MatchingShow {
        MatchingShow {
            id: danmaku_key(42),
            name: "匹配测试".into(),
            poster: MediaSource { url: String::new() },
            summary: String::new(),
            kind: String::new(),
            episode_count: 1,
            loading: true,
        }
    }

    #[test]
    fn stale_search_and_details_cannot_replace_the_latest_selection() {
        let mut e = ErikaEngine::default();
        e.state.matching.target = MatchingTarget::Danmaku;
        e.search_revision = 3;
        e.state.matching.loading = true;
        e.state.matching.selected = Some(show());
        e.found.lock().unwrap().extend([
            Found::Search {
                revision: 2,
                result: Err("过期错误".into()),
            },
            Found::Details {
                revision: 2,
                id: 42,
                result: Ok(details()),
            },
        ]);
        e.take_found();
        assert!(e.state.matching.loading);
        assert!(e.state.matching.error.is_empty());
        assert!(e.match_details.is_none());
        e.found.lock().unwrap().push(Found::Details {
            revision: 3,
            id: 42,
            result: Ok(details()),
        });
        e.take_found();
        assert!(!e.state.matching.loading);
        assert_eq!(e.state.matching.episodes[0].id, danmaku_key(420002));
    }

    #[test]
    fn previews_enrich_results_and_cached_selection_keeps_the_results_on_back() {
        let mut e = ErikaEngine::default();
        e.state.matching.target = MatchingTarget::Danmaku;
        e.search_revision = 3;
        e.state.matching.shows = vec![show()];
        let mut rich = details();
        rich.image_url = "https://example.com/poster.jpg".into();
        rich.summary = "能帮助识别作品的简介。".into();
        e.match_previews_active = 1;
        e.found.lock().unwrap().push(Found::Preview {
            revision: 2,
            id: 42,
            result: Ok(rich.clone()),
        });
        e.take_found();
        assert!(e.state.matching.shows[0].poster.url.is_empty());
        assert_eq!(e.match_previews_active, 1);
        e.found.lock().unwrap().push(Found::Preview {
            revision: 3,
            id: 42,
            result: Ok(rich.clone()),
        });
        e.take_found();
        assert_eq!(e.state.matching.shows[0].summary, rich.summary);
        assert_eq!(e.match_previews_active, 0);
        e.choose_show(danmaku_key(42)).unwrap();
        assert!(!e.state.matching.loading);
        assert_eq!(
            e.state.matching.selected.as_ref().unwrap().poster.url,
            rich.image_url
        );
        assert_eq!(e.state.matching.episodes[0].id, danmaku_key(420002));
        e.back_to_match_results().unwrap();
        e.found.lock().unwrap().push(Found::Details {
            revision: 3,
            id: 42,
            result: Ok(rich),
        });
        e.take_found();
        assert!(e.state.matching.selected.is_none());
        assert!(e.state.matching.episodes.is_empty());
        assert!(e.match_details.is_none());
        assert_eq!(e.state.matching.shows.len(), 1);
    }

    #[test]
    fn missing_preview_preserves_search_metadata_and_does_not_block_selection() {
        let mut e = ErikaEngine::default();
        e.state.matching.target = MatchingTarget::Danmaku;
        let mut result = show();
        result.poster.url = "https://example.com/search-poster.jpg".into();
        e.state.matching.shows = vec![result];
        e.found.lock().unwrap().push(Found::Preview {
            revision: 0,
            id: 42,
            result: Err("offline".into()),
        });
        e.take_found();
        assert!(!e.state.matching.shows[0].loading);
        assert!(!e.state.matching.shows[0].poster.url.is_empty());
        assert!(e.state.matching.error.is_empty());
        // A late response for a different show must not replace the selection.
        let mut selected = show();
        selected.id = danmaku_key(99);
        e.state.matching.selected = Some(selected);
        e.state.matching.loading = true;
        e.found.lock().unwrap().push(Found::Details {
            revision: 0,
            id: 42,
            result: Ok(details()),
        });
        e.take_found();
        assert!(e.state.matching.loading);
        assert!(e.state.matching.episodes.is_empty());
    }

    #[test]
    fn manual_matching_is_saved_and_auto_match_can_be_disabled_mid_request() {
        let mut e = ErikaEngine::default();
        e.state.matching.target = MatchingTarget::Danmaku;
        e.state.playing = true;
        e.state.features.danmaku = false; // No network is needed for this state transition.
        let fingerprint = Fingerprint {
            md5: "11111111111111111111111111111111".into(),
            size: 123,
        };
        let file =
            e.db.opened("file:///match-test.mkv", Some(&fingerprint))
                .unwrap();
        e.file = Some(file);
        let job = erika_store::catalog::CatalogJob {
            id: erika_catalog::MediaId::new().0,
            file: Some(file),
            input_revision: 0,
            phase: "recognition".into(),
            state: "queued".into(),
            payload: "{}".into(),
            error: String::new(),
            next_attempt: 0,
        };
        e.db.catalog_job_enqueue(&job).unwrap();
        e.db.catalog_job_transition(&job.id, "queued", "running", "recognition", "", 0)
            .unwrap();
        e.state.preferences.auto_match = false;
        e.found.lock().unwrap().push(Found::Resolved {
            revision: e.match_revision,
            file,
            job: job.id.clone(),
            danmaku_revision: 0,
            report: erika_catalog::resolve::FileResolution {
                recognition: None,
                recognition_error: None,
                resolution: Ok(erika_catalog::resolve::Resolution::Pending {
                    candidates: Vec::new(),
                    recognition: None,
                    reason: "需要确认".into(),
                }),
            },
        });
        e.take_found();
        assert!(e.db.catalog_binding(file).unwrap().is_none());
        assert_eq!(
            e.db.catalog_job(&job.id).unwrap().unwrap().state,
            "cancelled"
        );
        e.match_details = Some(details());
        e.state.matching.open = true;
        e.capture_match_revisions().unwrap();
        let subtitle = e.state.subtitle.clone();
        e.confirm_episode(danmaku_key(420002)).unwrap();
        assert!(e.db.file_episode(file).is_none());
        assert!(e.db.catalog_binding(file).unwrap().is_none());
        assert_eq!(
            e.db.catalog_danmaku(file).unwrap().unwrap().reference.id,
            "420002"
        );
        assert_eq!(e.episode_playing, Some(420002));
        assert_eq!(e.state.subtitle, subtitle);
        assert!(!e.state.matching.open);
    }
}

pub(super) enum Found {
    Catalog(super::catalog_matching::Reply),
    Search {
        revision: u64,
        result: Result<Vec<erika_dandanplay::Found>, String>,
    },
    Details {
        revision: u64,
        id: i64,
        result: Result<erika_dandanplay::Details, String>,
    },
    Preview {
        revision: u64,
        id: i64,
        result: Result<erika_dandanplay::Details, String>,
    },
    Sent {
        revision: u64,
        request_generation: u64,
        text: String,
        /// The color it was sent in, `0xRRGGBB`.
        color: i64,
        result: Result<(), String>,
    },
    Resolved {
        revision: u64,
        file: FileId,
        job: String,
        danmaku_revision: u64,
        report: erika_catalog::resolve::FileResolution,
    },
    Comments {
        revision: u64,
        request_generation: u64,
        episode: i64,
        result: Result<(String, usize), String>,
    },
}

impl ErikaEngine {
    /// Start recognizing the video at `source`, whose file was just read.
    pub(super) fn recognize(&mut self, source: &MediaSource, fingerprint: Option<Fingerprint>) {
        let Some(file) = self.file else { return };
        match self.show_catalog(file) {
            Ok(true) => return,
            Ok(false) => {}
            Err(error) => {
                self.notify(Notice::OperationFailed {
                    reason: error.message,
                });
                return;
            }
        }
        if !self.state.preferences.auto_match {
            return;
        }
        if erika_store::plugin_sources::is_address(&source.url) {
            // The opaque address is not a filename, and an upstream fingerprint
            // is not evidence that this device read the file's bytes.
            if let Err(error) = self.recognize_source_item(&source.url) {
                self.notify(Notice::OperationFailed { reason: error.message });
            }
            return;
        }
        let name = source.file_name().unwrap_or_default();
        let directory = bern::files::file_path(&source.url)
            .and_then(|p| p.parent().map(|p| p.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let prepared = (|| -> erika_store::catalog::Result<_> {
            let input_revision = self.db.catalog_revision(file)?;
            let previous = self.db.catalog_file_job(file, erika_store::now())?;
            if previous.as_ref().is_some_and(|j| {
                j.input_revision == input_revision
                    && (matches!(j.state.as_str(), "running" | "pending" | "failed")
                        || (j.state == "retry" && j.next_attempt > erika_store::now()))
            }) {
                return Ok(None);
            }
            let job = match previous
                .filter(|j| j.state == "queued" && j.input_revision == input_revision)
            {
                Some(job) => job,
                None => {
                    let job = erika_store::catalog::CatalogJob {
                        id: erika_catalog::MediaId::new().0,
                        file: Some(file),
                        input_revision,
                        phase: "recognition".into(),
                        state: "queued".into(),
                        payload: serde_json::json!({"name":name,"directory":directory}).to_string(),
                        error: String::new(),
                        next_attempt: 0,
                    };
                    self.db.catalog_job_enqueue(&job)?;
                    job
                }
            };
            let claimed = self.db.catalog_job_transition(
                &job.id,
                "queued",
                "running",
                "recognition",
                "",
                0,
            )?;
            if claimed.state != "running" {
                return Ok(None);
            }
            Ok(Some((job.id, self.db.catalog_danmaku_revision(file)?)))
        })();
        let (job, danmaku_revision) = match prepared {
            Ok(Some(value)) => value,
            Ok(None) => return,
            Err(error) => {
                self.notify(Notice::OperationFailed {
                    reason: error.message,
                });
                return;
            }
        };
        let (revision, client, inbox) = (
            self.match_revision,
            self.metadata.clone(),
            self.found.clone(),
        );
        let waker = bern::wake::waker();
        erika_catalog::client::spawn(async move {
            let report = erika_dandanplay::catalog::resolve_file(
                &client,
                &name,
                &directory,
                fingerprint.as_ref().map(|f| (f.md5.as_str(), f.size)),
                None,
                None,
            )
            .await;
            inbox
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Found::Resolved {
                    revision,
                    file,
                    job,
                    danmaku_revision,
                    report,
                });
            if let Some(waker) = waker {
                waker.wake();
            }
        });
    }

    /// Content identity and danmaku identity are intentionally read separately.
    /// A metadata outage must not hide an already recognised danmaku track.
    pub(super) fn show_catalog(&mut self, file: FileId) -> erika_store::catalog::Result<bool> {
        let clock = (
            file,
            self.db.catalog_revision(file)?,
            self.db.catalog_danmaku_revision(file)?,
        );
        let binding = self.db.catalog_binding(file)?;
        let decided = binding.is_some();
        self.read_playing_content(clock, binding)?;
        if let Some(danmaku) = self.db.catalog_danmaku(file)? {
            let changed = self
                .danmaku_binding
                .as_ref()
                .is_none_or(|b| b.revision != danmaku.revision || b.file != file);
            if changed {
                let track_changed = self.set_file_danmaku(danmaku.clone());
                let id = danmaku
                    .reference
                    .id
                    .parse::<i64>()
                    .ok()
                    .filter(|id| *id > 0);
                self.episode_playing = match danmaku.reference.provider {
                    erika_catalog::Provider::Dandanplay => id,
                    erika_catalog::Provider::Misaka
                        if erika_dandanplay::custom::server().as_deref()
                            == Some(danmaku.reference.scope.as_str()) =>
                    {
                        id.map(|id| -id)
                    }
                    _ => None,
                };
                if track_changed && let Some(id) = self.episode_playing {
                    self.fetch_online_danmaku(id);
                }
            }
        } else if self.danmaku_binding.is_some() {
            self.reset_file_danmaku();
            self.apply_danmaku();
        }
        Ok(decided)
    }

    pub(super) fn fetch_online_danmaku(&mut self, id: i64) {
        if !self.state.features.danmaku || !self.state.video {
            return;
        }
        let remote = self
            .current_source()
            .map(|s| self.remote_transport(s).unwrap_or_else(|| s.clone()).url)
            .filter(|s| erika_dandanplay::remote::comment_url(s).is_some())
            .filter(|_| erika_dandanplay::token().is_none());
        if id > 0 && erika_dandanplay::token().is_none() && remote.is_none() {
            return;
        }
        self.danmaku_generation += 1;
        let request_generation = self.danmaku_generation;
        let (revision, inbox) = (self.match_revision, self.found.clone());
        let waker = bern::wake::waker();
        erika_catalog::client::spawn(async move {
            let result = match remote {
                Some(stream) => erika_dandanplay::remote::comments(&stream),
                None if id < 0 => erika_dandanplay::custom::comments(id).map_err(|e| e.message),
                None => erika_dandanplay::catalog::comments(id)
                    .await
                    .map_err(|e| e.message),
            };
            inbox
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Found::Comments {
                    revision,
                    request_generation,
                    episode: id,
                    result,
                });
            if let Some(waker) = waker {
                waker.wake();
            }
        });
    }

    /// Take in what matching and fetching danmaku came back with.
    pub(super) fn take_found(&mut self) {
        let found: Vec<Found> = std::mem::take(
            &mut *self
                .found
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        for found in found {
            match found {
                Found::Catalog(reply) => self.take_catalog_match(reply),
                Found::Resolved {
                    revision,
                    file,
                    job,
                    danmaku_revision,
                    report,
                } => {
                    if !self.state.preferences.auto_match
                        || revision != self.match_revision
                        || self.file != Some(file)
                        || !self.state.playing
                    {
                        let _ = self.db.catalog_job_transition(
                            &job,
                            "running",
                            "cancelled",
                            "cancelled",
                            "播放或自动匹配设置已改变",
                            0,
                        );
                        continue;
                    }
                    match self
                        .db
                        .catalog_finish_resolution(&job, danmaku_revision, &report)
                    {
                        Ok(_) => {
                            if let Err(error) = self.show_catalog(file) {
                                self.notify(Notice::OperationFailed {
                                    reason: error.message,
                                });
                            }
                            #[cfg(feature = "bern-module-anime")]
                            erika_anime_protocol::anime::calls::history_changed();
                            #[cfg(feature = "bern-module-library")]
                            erika_library_protocol::library::calls::history_changed();
                        }
                        Err(error) => self.notify(Notice::OperationFailed {
                            reason: error.message,
                        }),
                    }
                }
                Found::Comments {
                    revision,
                    request_generation,
                    episode,
                    result,
                } => {
                    if revision != self.match_revision
                        || self.danmaku_generation != request_generation
                        || self.episode_playing != Some(episode)
                        || !self.state.playing
                    {
                        continue;
                    }
                    match result {
                        Ok((json, count)) => {
                            if let Some(kernel) = self.kernel().filter(|_| self.state.video) {
                                if self.load_danmaku(&kernel, "弹弹play", &json).is_ok() {
                                    self.online_danmaku_count = count;
                                    self.add_danmaku_source("弹弹play".into(), count);
                                }
                            }
                        }
                        Err(reason) => self.notify(Notice::OperationFailed { reason }),
                    }
                }
                Found::Search { revision, result } => {
                    if revision != self.search_revision {
                        continue;
                    }
                    self.state.matching.loading = false;
                    match result {
                        Ok(shows) => {
                            self.state.matching.shows =
                                shows.into_iter().map(search_show).collect();
                            self.match_preview_queue = self
                                .state
                                .matching
                                .shows
                                .iter()
                                .filter(|s| s.loading)
                                .filter_map(|s| danmaku_id(&s.id).ok())
                                .collect();
                            self.load_match_previews();
                            if self.state.matching.shows.is_empty() {
                                self.state.matching.error = "没有找到作品，请换一个名称搜索".into();
                            }
                        }
                        Err(error) => self.state.matching.error = error,
                    }
                }
                Found::Details {
                    revision,
                    id,
                    result,
                } => {
                    if revision != self.search_revision
                        || self.state.matching.selected.as_ref().map(|s| s.id.as_str())
                            != Some(danmaku_key(id).as_str())
                    {
                        continue;
                    }
                    self.state.matching.loading = false;
                    match result {
                        Ok(details) => self.show_match_details(id, details),
                        Err(error) => self.state.matching.error = error,
                    }
                }
                Found::Preview {
                    revision,
                    id,
                    result,
                } => {
                    if revision != self.search_revision {
                        continue;
                    }
                    self.match_previews_active = self.match_previews_active.saturating_sub(1);
                    if let Some(show) = self
                        .state
                        .matching
                        .shows
                        .iter_mut()
                        .find(|s| s.id == danmaku_key(id))
                    {
                        show.loading = false;
                        if let Ok(details) = result {
                            enrich_show(show, &details);
                            if let Some(selected) = self
                                .state
                                .matching
                                .selected
                                .as_mut()
                                .filter(|s| s.id == danmaku_key(id))
                            {
                                enrich_show(selected, &details);
                            }
                            self.match_previews.insert(id, details);
                        }
                    }
                    self.load_match_previews();
                }
                Found::Sent {
                    revision,
                    request_generation,
                    text,
                    color,
                    result,
                } => {
                    if !self.state.playing
                        || revision != self.match_revision
                        || request_generation != self.danmaku_generation
                    {
                        continue;
                    }
                    self.state.player.sending_danmaku = false;
                    match result {
                        Ok(()) => {
                            self.send_own_danmaku(&text, color);
                            if self.state.player.danmaku_text == text {
                                self.state.player.danmaku_text.clear();
                                self.state.player.danmaku_sent_revision += 1;
                            }
                            self.notify(Notice::DanmakuSent);
                        }
                        Err(reason) => self.notify(Notice::OperationFailed { reason }),
                    }
                }
            }
        }
    }

    pub(super) fn search_match(&mut self, query: String) -> Result<(), CommandError> {
        self.require_playing()?;
        let query = query.trim().to_string();
        if query.is_empty() {
            return Err(CommandError::new("empty_query", "请输入作品名称"));
        }
        if self.state.matching.target == MatchingTarget::Content {
            return self.search_catalog_match(query);
        }
        self.capture_match_revisions()?;
        self.search_revision += 1;
        let revision = self.search_revision;
        self.state.matching.loading = true;
        self.state.matching.error.clear();
        self.state.matching.selected = None;
        self.state.matching.shows.clear();
        self.state.matching.episodes.clear();
        self.match_details = None;
        self.match_previews.clear();
        self.match_preview_queue.clear();
        self.match_previews_active = 0;
        in_background(&self.found, move || {
            let result = erika_dandanplay::search(&query);
            let custom = erika_dandanplay::custom::server()
                .map(|_| erika_dandanplay::custom::search(&query));
            let result = match (result, custom) {
                (Ok(mut shows), Some(Ok(extra))) => {
                    shows.extend(extra);
                    Ok(shows)
                }
                (Ok(shows), _) => Ok(shows),
                (Err(_), Some(Ok(shows))) => Ok(shows),
                (Err(e), _) => Err(e.message),
            };
            vec![Found::Search { revision, result }]
        });
        Ok(())
    }

    pub(super) fn choose_show(&mut self, key: String) -> Result<(), CommandError> {
        if self.state.matching.target == MatchingTarget::Content {
            return self.choose_catalog_show(key);
        }
        let id = danmaku_id(&key)?;
        let show = self
            .state
            .matching
            .shows
            .iter()
            .find(|s| s.id == danmaku_key(id))
            .ok_or_else(|| CommandError::new("unknown_show", "请先搜索作品"))?
            .clone();
        let revision = self.search_revision;
        self.state.matching.selected = Some(show);
        self.state.matching.loading = true;
        self.state.matching.error.clear();
        self.state.matching.episodes.clear();
        self.match_details = None;
        if let Some(details) = self.match_previews.get(&id).cloned() {
            self.show_match_details(id, details);
        } else {
            in_background(&self.found, move || {
                vec![Found::Details {
                    revision,
                    id,
                    result: erika_dandanplay::details(id).map_err(|e| e.message),
                }]
            });
        }
        Ok(())
    }

    /// Search results appear immediately. Fill their descriptions in the
    /// background, with at most three requests in flight for this search.
    fn load_match_previews(&mut self) {
        while self.match_previews_active < 3 {
            let Some(id) = self.match_preview_queue.pop_front() else {
                break;
            };
            let revision = self.search_revision;
            self.match_previews_active += 1;
            in_background(&self.found, move || {
                vec![Found::Preview {
                    revision,
                    id,
                    result: erika_dandanplay::details(id).map_err(|e| e.message),
                }]
            });
        }
    }

    /// The details of the show asked about as `asked`; a service may give
    /// them under the show's lasting id.
    fn show_match_details(&mut self, asked: i64, details: erika_dandanplay::Details) {
        self.state.matching.loading = false;
        if let Some(show) = self.state.matching.selected.as_mut() {
            enrich_show(show, &details);
        }
        if let Some(show) = self
            .state
            .matching
            .shows
            .iter_mut()
            .find(|s| s.id == danmaku_key(asked))
        {
            enrich_show(show, &details);
        }
        self.state.matching.episodes = details
            .episodes
            .iter()
            .map(|e| MatchingItem {
                id: danmaku_key(e.episode_id),
                name: format!("{} {}", e.episode_number, e.episode_title),
            })
            .collect();
        if details.episodes.is_empty() {
            self.state.matching.error = "这个作品没有可匹配的集数".into();
        }
        self.match_previews.insert(asked, details.clone());
        self.match_details = Some(details);
    }

    pub(super) fn confirm_episode(&mut self, key: String) -> Result<(), CommandError> {
        self.require_playing()?;
        if self.state.matching.target == MatchingTarget::Content {
            return self.confirm_catalog_item(key);
        }
        let id = danmaku_id(&key)?;
        let details = self
            .match_details
            .as_ref()
            .ok_or_else(|| CommandError::new("no_show", "请先选择弹幕作品"))?;
        let info = details
            .episodes
            .iter()
            .find(|e| e.episode_id == id)
            .ok_or_else(|| CommandError::new("no_episode", "请选择列表中的弹幕集数"))?;
        let title = format!("{} {}", details.anime_title, info.episode_title);
        let (file, _, revision) = self.match_revisions()?;
        let current = self
            .db
            .catalog_danmaku(file)
            .map_err(|e| CommandError::new(e.code, e.message))?;
        let provider = if id > 0 {
            erika_catalog::Provider::Dandanplay
        } else {
            erika_catalog::Provider::Misaka
        };
        let scope = if id < 0 {
            erika_dandanplay::custom::server()
                .ok_or_else(|| CommandError::new("no_server", "弹幕服务器未配置"))?
        } else {
            String::new()
        };
        let reference = erika_catalog::ProviderRef {
            provider,
            scope,
            kind: erika_catalog::Kind::Episode,
            id: id.unsigned_abs().to_string(),
        };
        let same = current.as_ref().is_some_and(|b| b.reference == reference);
        let binding = erika_catalog::DanmakuBinding {
            file,
            reference,
            media: self.media_playing.clone(),
            manual: true,
            fingerprint_verified: false,
            shift_seconds: current
                .as_ref()
                .filter(|_| same)
                .map_or(0.0, |b| b.shift_seconds),
            user_shift_seconds: current.as_ref().map_or(0.0, |b| b.user_shift_seconds),
            revision,
        };
        self.db
            .catalog_apply(&[], &[], &[], &[binding])
            .map_err(|e| CommandError::new(e.code, e.message))?;
        // Selecting a danmaku track is NOT a content rematch or an account mapping.
        self.show_catalog(file)
            .map_err(|e| CommandError::new(e.code, e.message))?;
        self.state.matching.open = false;
        self.notify(Notice::Matched { title });
        Ok(())
    }

    pub(super) fn send_online_danmaku(&mut self, text: String) -> Result<(), CommandError> {
        if self.state.player.sending_danmaku {
            return Err(CommandError::new("sending", "弹幕正在发送"));
        }
        self.state.player.danmaku_text = text.clone();
        if !self.state.video {
            return Err(CommandError::new("no_video", "请先打开视频"));
        }
        if erika_dandanplay::token().is_none() {
            return Err(CommandError::new("sign_in", "请先登录弹弹play再发送弹幕"));
        }
        let episode = self
            .episode_playing
            .ok_or_else(|| CommandError::new("no_match", "请先匹配视频集数"))?;
        let text = text.trim().to_string();
        if text.is_empty() || text.chars().count() > 40 {
            return Err(CommandError::new(
                "invalid_comment",
                "弹幕需要包含 1–40 个字符",
            ));
        }
        let time = self.online_danmaku_time()?;
        self.state.player.danmaku_text = text.clone();
        self.state.player.sending_danmaku = true;
        let revision = self.match_revision;
        let request_generation = self.danmaku_generation;
        let color = self.state.player.danmaku_color;
        in_background(&self.found, move || {
            let result = erika_dandanplay::send_comment(episode, time, &text, color as u32)
                .map_err(|e| e.message);
            vec![Found::Sent {
                revision,
                request_generation,
                text,
                color,
                result,
            }]
        });
        Ok(())
    }
}
