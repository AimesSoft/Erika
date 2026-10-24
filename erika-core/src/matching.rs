//! Which episode a video is. Once its file is known, dandanplay matches it
//! by its name and fingerprint, on another thread; the file is then linked
//! to the episode in the database, the player names the anime and the
//! episode, and dandanplay's danmaku of the episode come along. A file
//! matched before needs no request.

use super::*;
use erika_store::{Anime, Episode, FileId, Fingerprint};

/// What came back for the video playing.
pub(super) fn initial() -> PlayerMatching {
    PlayerMatching {
        open: false,
        loading: false,
        error: String::new(),
        selected: None,
        shows: Vec::new(),
        episodes: Vec::new(),
    }
}

fn search_show(show: erika_dandanplay::Found) -> MatchingShow {
    MatchingShow {
        id: show.anime_id,
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
            }],
            is_on_air: false,
            air_day: None,
            is_favorited: false,
        }
    }

    fn show() -> MatchingShow {
        MatchingShow {
            id: 42,
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
        assert_eq!(e.state.matching.episodes[0].id, 420002);
    }

    #[test]
    fn previews_enrich_results_and_cached_selection_keeps_the_results_on_back() {
        let mut e = ErikaEngine::default();
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
        e.choose_show(42).unwrap();
        assert!(!e.state.matching.loading);
        assert_eq!(
            e.state.matching.selected.as_ref().unwrap().poster.url,
            rich.image_url
        );
        assert_eq!(e.state.matching.episodes[0].id, 420002);
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
        selected.id = 99;
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
        let anime = Anime {
            dandanplay_id: 99,
            title: "旧匹配".into(),
            image: String::new(),
        };
        let episode = Episode {
            dandanplay_id: 990001,
            number: 1,
            title: "第一集".into(),
        };
        e.state.preferences.auto_match = false;
        e.found.lock().unwrap().push(Found::Matched {
            revision: e.match_revision,
            file,
            anime,
            episode,
        });
        e.take_found();
        assert!(e.db.file_episode(file).is_none());
        e.match_details = Some(details());
        e.state.matching.open = true;
        e.confirm_episode(420002).unwrap();
        let (saved_anime, saved_episode) = e.db.file_episode(file).unwrap();
        assert_eq!(saved_anime.dandanplay_id, 42);
        assert_eq!(saved_episode.dandanplay_id, 420002);
        assert_eq!(e.episode_playing, Some(420002));
        assert_eq!(e.state.subtitle, PlayingNote::Episode { episode: 2 });
        assert!(!e.state.matching.open);
    }
}

pub(super) enum Found {
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
        text: String,
        result: Result<(), String>,
    },
    Matched {
        revision: u64,
        file: FileId,
        anime: Anime,
        episode: Episode,
    },
    Comments {
        revision: u64,
        episode: i64,
        result: Result<(String, usize), String>,
    },
}

impl ErikaEngine {
    /// Start recognizing the video at `source`, whose file was just read.
    pub(super) fn recognize(&mut self, source: &MediaSource, fingerprint: Option<Fingerprint>) {
        if !self.state.preferences.auto_match {
            return;
        }
        let Some(file) = self.file else {
            return;
        };
        if let Some((anime, episode)) = self.db.file_episode(file) {
            self.show_episode(&anime, &episode);
            return;
        }
        let Some(fingerprint) = fingerprint else {
            return;
        };
        let official = erika_dandanplay::token().is_some();
        let custom = erika_dandanplay::custom::server().is_some();
        if !official && !custom {
            return;
        }
        let name = source.file_name().unwrap_or_default();
        let revision = self.match_revision;
        in_background(&self.found, move || {
            let result = if official {
                erika_dandanplay::match_video(&name, &fingerprint.md5, fingerprint.size)
            } else {
                Ok(None)
            };
            let result = if custom && !matches!(result, Ok(Some(_))) {
                erika_dandanplay::custom::match_video(&name, &fingerprint.md5, fingerprint.size)
            } else {
                result
            };
            match result {
                Ok(Some(matched)) => vec![Found::Matched {
                    revision,
                    file,
                    anime: Anime {
                        dandanplay_id: matched.anime_id,
                        title: matched.anime_title.clone(),
                        image: String::new(),
                    },
                    episode: Episode {
                        dandanplay_id: matched.episode_id,
                        number: matched.number(),
                        title: matched.episode_title,
                    },
                }],
                Ok(None) => Vec::new(),
                Err(error) => {
                    eprintln!("erika: match {name}: {}", error.message);
                    Vec::new()
                }
            }
        });
    }

    /// Name what plays after its anime and episode, and fetch the
    /// episode's danmaku.
    fn show_episode(&mut self, anime: &Anime, episode: &Episode) {
        if !anime.title.is_empty() {
            self.state.title = anime.title.clone();
        }
        self.state.subtitle = PlayingNote::Episode {
            episode: episode.number,
        };
        self.state.player.episode = episode.number;
        if let Some(source) = self.source.clone() {
            self.prepare_queue(&source);
        }
        self.episode_playing = Some(episode.dandanplay_id);
        // A video a dandanplay PC client streams has the danmaku that
        // client fetches, which need no account here.
        let remote = self
            .source
            .as_ref()
            .map(|source| source.url.clone())
            .filter(|url| erika_dandanplay::remote::comment_url(url).is_some())
            .filter(|_| episode.dandanplay_id > 0 && erika_dandanplay::token().is_none());
        if !self.state.features.danmaku
            || (episode.dandanplay_id > 0
                && erika_dandanplay::token().is_none()
                && remote.is_none())
        {
            return;
        }
        let id = episode.dandanplay_id;
        let revision = self.match_revision;
        in_background(&self.found, move || {
            let result = match remote {
                Some(stream) => erika_dandanplay::remote::comments(&stream),
                None => erika_dandanplay::comments(id).map_err(|e| e.message),
            };
            vec![Found::Comments {
                revision,
                episode: id,
                result,
            }]
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
                Found::Matched {
                    revision,
                    file,
                    anime,
                    episode,
                } => {
                    if !self.state.preferences.auto_match
                        || revision != self.match_revision
                        || self.file != Some(file)
                        || !self.state.playing
                    {
                        continue;
                    }
                    // The anime keeps the poster it was stored with.
                    let anime = Anime {
                        image: self
                            .db
                            .anime(anime.dandanplay_id)
                            .map(|known| known.image)
                            .unwrap_or_default(),
                        ..anime
                    };
                    self.db.link_episode(file, &anime, &episode, false);
                    #[cfg(feature = "bern-module-anime")]
                    erika_anime_protocol::anime::calls::history_changed();
                    #[cfg(feature = "bern-module-library")]
                    erika_library_protocol::library::calls::history_changed();
                    if self.file == Some(file) && self.state.playing {
                        self.notify(Notice::Matched {
                            title: format!("{} 第 {} 话", anime.title, episode.number),
                        });
                        self.show_episode(&anime, &episode);
                    }
                }
                Found::Comments {
                    revision,
                    episode,
                    result,
                } => {
                    if revision != self.match_revision
                        || self.episode_playing != Some(episode)
                        || !self.state.playing
                    {
                        continue;
                    }
                    match result {
                        Ok((json, count)) => {
                            if let Some(kernel) = self.kernel().filter(|_| self.state.video) {
                                if kernel.add_danmaku("弹弹play", &json).is_ok() {
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
                                .map(|s| s.id)
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
                        || self.state.matching.selected.as_ref().map(|s| s.id) != Some(id)
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
                    if let Some(show) = self.state.matching.shows.iter_mut().find(|s| s.id == id) {
                        show.loading = false;
                        if let Ok(details) = result {
                            enrich_show(show, &details);
                            if let Some(selected) =
                                self.state.matching.selected.as_mut().filter(|s| s.id == id)
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
                    text,
                    result,
                } => {
                    self.state.player.sending_danmaku = false;
                    match result {
                        Ok(()) => {
                            if self.state.playing && revision == self.match_revision {
                                self.send_own_danmaku(&text);
                            }
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

    pub(super) fn choose_show(&mut self, id: i64) -> Result<(), CommandError> {
        let show = self
            .state
            .matching
            .shows
            .iter()
            .find(|s| s.id == id)
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
            .find(|s| s.id == asked)
        {
            enrich_show(show, &details);
        }
        self.state.matching.episodes = details
            .episodes
            .iter()
            .map(|e| MediaTrack {
                id: e.episode_id,
                name: format!("{} {}", e.episode_number, e.episode_title),
            })
            .collect();
        if details.episodes.is_empty() {
            self.state.matching.error = "这个作品没有可匹配的集数".into();
        }
        self.match_previews.insert(asked, details.clone());
        self.match_details = Some(details);
    }

    pub(super) fn confirm_episode(&mut self, id: i64) -> Result<(), CommandError> {
        self.require_playing()?;
        let details = self
            .match_details
            .as_ref()
            .ok_or_else(|| CommandError::new("no_show", "请先选择作品"))?;
        let info = details
            .episodes
            .iter()
            .find(|e| e.episode_id == id)
            .ok_or_else(|| CommandError::new("no_episode", "请选择列表中的集数"))?;
        let anime = Anime {
            dandanplay_id: details.anime_id,
            title: details.anime_title.clone(),
            image: details.image_url.clone(),
        };
        let episode = Episode {
            dandanplay_id: id,
            number: info.episode_number.parse().unwrap_or(id.abs() % 10000),
            title: info.episode_title.clone(),
        };
        self.match_revision += 1;
        if let Some(file) = self.file {
            self.db.link_episode(file, &anime, &episode, true);
            // The recognition server knows the file by its fingerprint next
            // time, on every device.
            if erika_dandanplay::custom::is_ours(anime.dandanplay_id)
                && let Some(print) = self.db.fingerprint_of(file)
            {
                let name = self.source.as_ref().and_then(MediaSource::file_name).unwrap_or_default();
                let (anime, episode) = (anime.clone(), episode.clone());
                in_background(&self.found, move || {
                    let done = erika_dandanplay::custom::confirm(&erika_dandanplay::custom::Confirmed {
                        file_name: &name,
                        md5: &print.md5,
                        size: print.size,
                        anime_id: anime.dandanplay_id,
                        episode_id: episode.dandanplay_id,
                        number: episode.number,
                        anime_title: &anime.title,
                        episode_title: &episode.title,
                        image: &anime.image,
                    });
                    if let Err(error) = done {
                        eprintln!("erika: confirm {name} on the server: {}", error.message);
                    }
                    Vec::new()
                });
            }
        }
        self.show_episode(&anime, &episode);
        self.state.matching.open = false;
        self.notify(Notice::Matched {
            title: format!("{} {}", anime.title, episode.title),
        });
        #[cfg(feature = "bern-module-anime")]
        erika_anime_protocol::anime::calls::history_changed();
        #[cfg(feature = "bern-module-library")]
        erika_library_protocol::library::calls::history_changed();
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
        self.state.player.danmaku_text = text.clone();
        self.state.player.sending_danmaku = true;
        let revision = self.match_revision;
        let time = self.state.elapsed;
        in_background(&self.found, move || {
            let result =
                erika_dandanplay::send_comment(episode, time, &text).map_err(|e| e.message);
            vec![Found::Sent {
                revision,
                text,
                result,
            }]
        });
        Ok(())
    }
}
