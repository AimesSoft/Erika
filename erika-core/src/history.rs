//! What was played: the recent videos and where each stopped, kept in the
//! app's database so they outlive the run. Progress belongs to the file
//! (the same video moved or copied picks up where it stopped); the recent
//! list names the address it was played from.

use super::*;
use erika_store::{Db, FileId};
use std::path::{Path, PathBuf};

/// How often progress is saved while a video plays, in seconds of it.
const SAVE_EVERY: f64 = 10.0;

/// Where a video picks up: where it stopped, unless that was in its first
/// seconds or at its very end.
fn resume_point(position: f64, duration: f64) -> f64 {
    if position > 5.0 && (duration <= 0.0 || position < duration - 15.0) {
        position
    } else {
        0.0
    }
}

/// The recent videos as 「最近播放」 lists them.
pub(super) fn recents(db: &Db) -> Vec<RecentPlay> {
    db.recents(RECENT_COUNT)
        .into_iter()
        .map(|recent| RecentPlay {
            name: recent.title,
            progress: if recent.duration > 0.0 {
                (recent.position / recent.duration).clamp(0.0, 1.0)
            } else {
                0.0
            },
            remaining: (recent.duration - recent.position).max(0.0),
            still: recent
                .still
                .filter(|still| Path::new(still).exists())
                .map(MediaSource::file)
                .unwrap_or_else(no_picture),
            source: Some(MediaSource::new(recent.url)),
        })
        .collect()
}

/// Where pictures of the videos are kept: the cache folder, or the
/// temporary one where the app has none.
pub(super) fn stills_dir() -> PathBuf {
    #[cfg(target_arch = "wasm32")]
    {
        PathBuf::from("stills")
    } // Only an identifier; browser captures are blob URLs.
    #[cfg(not(target_arch = "wasm32"))]
    {
        bern::storage::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("stills")
    }
}

/// Pictures no recent video names any more, except those taken in this
/// run.
pub(super) fn forget_stills(db: &Db, run: &str) {
    let kept = db.stills();
    for entry in std::fs::read_dir(stills_dir())
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        let named = kept.iter().any(|still| Path::new(still) == path);
        if !named && !entry.file_name().to_string_lossy().starts_with(run) {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl ErikaEngine {
    /// Which file `source` is, read from its head, noted in the database
    /// with the address it was opened from; and the fingerprint read.
    /// Neither for a web address.
    pub(super) fn identify(
        &self,
        source: &MediaSource,
    ) -> (Option<FileId>, Option<erika_store::Fingerprint>) {
        if erika_store::plugin_sources::is_address(&source.url) {
            return self.db.plugin_source_item(&source.url).ok().flatten()
                .map(|entry| (Some(entry.file), entry.item.fingerprint))
                .unwrap_or((None, None));
        }
        #[cfg(target_arch = "wasm32")]
        let fingerprint = None; // Session storage has no fingerprint lookup; avoid reading 16 MiB on the UI thread.
        #[cfg(not(target_arch = "wasm32"))]
        let fingerprint = source.open().ok().and_then(|file| {
            let size = file.len();
            erika_store::fingerprint(file, size).ok()
        });
        (
            self.db.opened(&source.url, fingerprint.as_ref()),
            fingerprint,
        )
    }

    /// Where `file` stopped last time, in seconds, and how far that is.
    pub(super) fn saved_position(&self, file: Option<FileId>) -> (f64, f64) {
        if !self.state.preferences.remember_position {
            return (0.0, 0.0);
        }
        match file.and_then(|file| self.db.progress(file)) {
            Some((position, duration)) => {
                let start = resume_point(position, duration);
                (
                    start,
                    if duration > 0.0 {
                        start / duration
                    } else {
                        0.0
                    },
                )
            }
            None => (0.0, 0.0),
        }
    }

    /// Keep where the video playing is: its file's progress, and its place
    /// at the top of the recent videos. Simulated media is not kept.
    pub(super) fn save_history(&mut self) {
        // An accepted open request can still fail or be cancelled before loading.
        // Keep the previous record until this transport has reported real playback.
        if self.source.is_none() || !self.playback_observed { return; }
        let Some(source) = self.current_source().cloned() else { return; };
        self.save_played();
        let (position, duration) = (self.state.elapsed, self.state.duration);
        if let Some(file) = self.file
            && duration > 0.0
            && self.state.preferences.remember_position
        {
            self.db.save_progress(file, position, duration);
        }
        let still = self.still_path.as_ref().and_then(|path| path.to_str());
        // Listed by the file's own name, as when it was first opened; a
        // dandanplay client's stream, whose address names no file, by its
        // episode.
        let name = match &self.state.subtitle {
            PlayingNote::Episode { episode }
                if erika_dandanplay::remote::comment_url(&source.url).is_some() =>
            {
                format!("{} 第 {episode} 话", self.state.title)
            }
            // Remote endpoint selectors are not filenames. Keep the title
            // supplied by the library or resolved canonical metadata.
            _ if erika_dandanplay::remote::comment_url(&source.url).is_some() => self.state.title.clone(),
            _ if erika_store::plugin_sources::is_address(&source.url) => self.source_name(&source),
            _ => source
                .file_name()
                .unwrap_or_else(|| self.state.title.clone()),
        };
        self.db.remember(
            &source.url,
            &name,
            still,
            if self.state.preferences.remember_position {
                position
            } else {
                0.0
            },
            duration,
        );
        self.saved_at = position;
        #[cfg(feature = "bern-module-anime")]
        erika_anime_protocol::anime::calls::history_changed();
        #[cfg(feature = "bern-module-library")]
        erika_library_protocol::library::calls::history_changed();
    }

    /// List the recent videos as the database has them now.
    pub(super) fn show_recents(&mut self) {
        self.state.recents = recents(&self.db);
        self.state.has_recents = !self.state.recents.is_empty();
    }

    /// Save now and then while a video plays.
    pub(super) fn save_history_now_and_then(&mut self) {
        if self.source.is_some() && (self.state.elapsed - self.saved_at).abs() >= SAVE_EVERY {
            self.save_history();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_video_picks_up_where_it_stopped_unless_it_barely_began_or_ended() {
        assert_eq!(resume_point(600.0, 1440.0), 600.0);
        assert_eq!(resume_point(3.0, 1440.0), 0.0);
        assert_eq!(resume_point(1430.0, 1440.0), 0.0);
        assert_eq!(resume_point(60.0, 0.0), 60.0);
    }
}
