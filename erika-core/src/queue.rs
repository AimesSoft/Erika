//! Actual media sources behind the player's episode list.
use super::*;
use std::cmp::Ordering;
use std::path::Path;

#[derive(Clone)]
pub(super) struct Entry {
    pub source: MediaSource,
    pub title: String,
    pub episode: Option<i64>,
}

fn video(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        [
            "mkv", "mp4", "m4v", "mov", "avi", "webm", "ts", "m2ts", "mpg", "mpeg", "flv", "wmv",
            "ogv",
        ]
        .contains(&s.to_ascii_lowercase().as_str())
    })
}

// Compare digit runs numerically without overflowing on long filenames.
fn natural(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    while !a.is_empty() && !b.is_empty() {
        let order = if a[0].is_ascii_digit() && b[0].is_ascii_digit() {
            let (an, bn) = (
                a.iter().take_while(|c| c.is_ascii_digit()).count(),
                b.iter().take_while(|c| c.is_ascii_digit()).count(),
            );
            let (aa, bb) = (&a[..an], &b[..bn]);
            let aa = &aa[aa.iter().take_while(|c| **c == b'0').count()..];
            let bb = &bb[bb.iter().take_while(|c| **c == b'0').count()..];
            a = &a[an..];
            b = &b[bn..];
            aa.len().cmp(&bb.len()).then_with(|| aa.cmp(bb))
        } else {
            let order = a[0].cmp(&b[0]);
            a = &a[1..];
            b = &b[1..];
            order
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

pub(super) fn folder(path: &Path) -> Result<Vec<Entry>, CommandError> {
    let entries = std::fs::read_dir(path)
        .map_err(|e| CommandError::new("folder_unreadable", e.to_string()))?;
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && video(p))
        .collect::<Vec<_>>();
    paths
        .sort_by(|a, b| natural(&a.to_string_lossy(), &b.to_string_lossy()).then_with(|| a.cmp(b)));
    Ok(paths
        .into_iter()
        .map(|p| Entry {
            title: p
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            source: MediaSource::file(p),
            episode: None,
        })
        .collect())
}

impl ErikaEngine {
    pub(super) fn prepare_queue(&mut self, source: &MediaSource) {
        self.queue = self
            .file
            .and_then(|f| self.db.file_episode(f))
            .map(|(anime, _)| {
                self.db
                    .episode_files(anime.dandanplay_id)
                    .into_iter()
                    .filter(|f| bern::files::file_path(&f.url).is_none_or(|p| p.is_file()))
                    .map(|f| Entry {
                        source: MediaSource::new(f.url),
                        title: anime.title.clone(),
                        episode: Some(f.episode.number),
                    })
                    .collect()
            })
            .unwrap_or_default();
        if self.queue.len() < 2 {
            self.queue = bern::files::file_path(&source.url)
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .and_then(|p| folder(&p).ok())
                .unwrap_or_default();
        }
        if !self.queue.iter().any(|e| e.source.url == source.url) {
            self.queue = vec![Entry {
                source: source.clone(),
                title: self.state.title.clone(),
                episode: None,
            }];
        }
        self.publish_queue(source);
    }

    pub(super) fn publish_queue(&mut self, source: &MediaSource) {
        let index = self
            .queue
            .iter()
            .position(|e| e.source.url == source.url)
            .unwrap_or(0);
        let player = &mut self.state.player;
        player.queue_index = index as i64 + 1;
        player.episodes = self.queue.len() > 1;
        player.has_previous = index > 0;
        player.has_next = index + 1 < self.queue.len();
        player.episode_list = self
            .queue
            .iter()
            .enumerate()
            .map(|(i, entry)| MediaTrack {
                id: i as i64 + 1,
                name: entry
                    .episode
                    .map(|ep| format!("第 {ep} 集"))
                    .unwrap_or_else(|| entry.title.clone()),
            })
            .collect();
    }

    pub(super) fn play_queue(&mut self, index: i64, resume: bool) -> Result<(), CommandError> {
        let entry = index
            .checked_sub(1)
            .and_then(|i| self.queue.get(i as usize))
            .cloned()
            .ok_or_else(|| CommandError::new("episode_out_of_range", "播放列表中没有这一集"))?;
        if bern::files::file_path(&entry.source.url).is_some_and(|p| !p.is_file()) {
            return Err(CommandError::new("file_missing", "视频文件已移动或删除"));
        }
        let (file, fingerprint) = self.identify(&entry.source);
        let (start, progress) = if resume {
            self.saved_position(file)
        } else {
            (0.0, 0.0)
        };
        let note = entry
            .episode
            .map(|episode| PlayingNote::Episode { episode })
            .unwrap_or(PlayingNote::LocalFile);
        self.start(entry.title, note, no_picture(), progress);
        self.file = file;
        self.publish_queue(&entry.source);
        self.load(Some(entry.source.clone()), start);
        self.recognize(&entry.source, fingerprint);
        Ok(())
    }

    pub(super) fn advance_at_end(&mut self) {
        if self.state.player.has_next
            && self.state.preferences.autoplay
            && self.state.preferences.playback_end == PlaybackEnd::Next
        {
            if let Err(error) = self.play_queue(self.state.player.queue_index + 1, false) {
                self.state.paused = true;
                self.notify(Notice::OperationFailed {
                    reason: error.message,
                });
            }
        } else {
            self.state.paused = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filenames_sort_by_number_and_keep_specials() {
        let mut files = ["EP10.mkv", "EP2.mkv", "EP01.mkv", "SP1.mkv"];
        files.sort_by(|a, b| natural(a, b));
        assert_eq!(files, ["EP01.mkv", "EP2.mkv", "EP10.mkv", "SP1.mkv"]);
        assert!(video(Path::new("show.MKV")));
        assert!(!video(Path::new("show.xml")));
    }
}
