//! Which episode a video is. Once its file is known, dandanplay matches it
//! by its name and fingerprint, on another thread; the file is then linked
//! to the episode in the database, the player names the anime and the
//! episode, and dandanplay's danmaku of the episode come along. A file
//! matched before needs no request.

use super::*;
use erika_store::{Anime, Episode, FileId, Fingerprint};

/// What came back for the video playing.
pub(super) enum Found {
    Matched { file: FileId, anime: Anime, episode: Episode },
    Comments { episode: i64, json: String, count: usize },
}

impl ErikaEngine {
    /// Start recognizing the video at `source`, whose file was just read.
    pub(super) fn recognize(&mut self, source: &MediaSource, fingerprint: Option<Fingerprint>) {
        let Some(file) = self.file else {
            return;
        };
        if let Some((anime, episode)) = self.db.file_episode(file) {
            self.show_episode(&anime, &episode);
            return;
        }
        let (Some(fingerprint), Some(_)) = (fingerprint, erika_dandanplay::token()) else {
            return;
        };
        let name = source.file_name().unwrap_or_default();
        in_background(&self.found, move || {
            match erika_dandanplay::match_video(&name, &fingerprint.md5, fingerprint.size) {
                Ok(Some(matched)) => vec![Found::Matched {
                    file,
                    anime: Anime { dandanplay_id: matched.anime_id, title: matched.anime_title.clone(), image: String::new() },
                    episode: Episode { dandanplay_id: matched.episode_id, number: matched.number(), title: matched.episode_title },
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
        self.state.subtitle = PlayingNote::Episode { episode: episode.number };
        self.episode_playing = Some(episode.dandanplay_id);
        if !self.state.features.danmaku || erika_dandanplay::token().is_none() {
            return;
        }
        let id = episode.dandanplay_id;
        in_background(&self.found, move || match erika_dandanplay::comments(id) {
            Ok((json, count)) => vec![Found::Comments { episode: id, json, count }],
            Err(error) => {
                eprintln!("erika: danmaku of episode {id}: {}", error.message);
                Vec::new()
            }
        });
    }

    /// Take in what matching and fetching danmaku came back with.
    pub(super) fn take_found(&mut self) {
        let found: Vec<Found> = std::mem::take(&mut *self.found.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
        for found in found {
            match found {
                Found::Matched { file, anime, episode } => {
                    // The anime keeps the poster it was stored with.
                    let anime = Anime { image: self.db.anime(anime.dandanplay_id).map(|known| known.image).unwrap_or_default(), ..anime };
                    self.db.link_episode(file, &anime, &episode, false);
                    erika_anime_protocol::anime::calls::history_changed();
                    erika_library_protocol::library::calls::history_changed();
                    if self.file == Some(file) && self.state.playing {
                        self.notify(Notice::Matched { title: format!("{} 第 {} 话", anime.title, episode.number) });
                        self.show_episode(&anime, &episode);
                    }
                }
                Found::Comments { episode, json, count } => {
                    let Some(kernel) = self.kernel().filter(|_| self.state.video && self.episode_playing == Some(episode)) else {
                        continue;
                    };
                    if count > 0 && kernel.add_danmaku("弹弹play", &json).is_ok() {
                        self.add_danmaku_source("弹弹play".into(), count);
                    }
                }
            }
        }
    }
}
