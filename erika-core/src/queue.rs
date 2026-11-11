//! Actual media sources behind the player's episode list.
use super::*;
use std::cmp::Ordering;
use std::path::Path;

#[derive(Clone)]
pub(super) struct Entry {
    pub source: MediaSource,
    pub title: String,
    pub note: PlayingNote,
    pub label: String,
    pub media: Option<erika_catalog::MediaId>,
    pub start: Option<f64>,
    pub end: Option<f64>,
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
            note: PlayingNote::LocalFile,
            label: String::new(),
            media: None,
            start: None,
            end: None,
        })
        .collect())
}

impl ErikaEngine {
    fn catalog_queue(&self) -> erika_store::catalog::Result<Vec<Entry>> {
        let Some(file) = self.file else {
            return Ok(Vec::new());
        };
        let Some(binding) = self.db.catalog_binding(file)? else {
            return Ok(Vec::new());
        };
        let Some(item) = binding.items.first() else {
            return Ok(Vec::new());
        };
        let Some(node) = self.db.catalog_node(&item.media)? else {
            return Ok(Vec::new());
        };
        let Some(root) = self.db.catalog_node(&node.root)? else {
            return Ok(Vec::new());
        };
        let mut files = self.db.catalog_files(&root.id)?;
        // A queue plays each content item once. Keep the version already playing
        // when a work has several encodes; the library still exposes all versions.
        files.sort_by_key(|f| f.file != file);
        let mut seen_media = std::collections::HashSet::new();
        let mut seen_files = std::collections::HashSet::new();
        let mut entries = Vec::new();
        for candidate in files {
            if bern::files::file_path(&candidate.url).is_some_and(|p| !p.is_file()) {
                continue;
            }
            let Some(node) = self.db.catalog_node(&candidate.media)? else {
                continue;
            };
            let Some(binding) = self.db.catalog_binding(candidate.file)? else {
                continue;
            };
            let Some(item) = binding.items.iter().find(|i| i.media == node.id) else {
                continue;
            };
            // A partially timed combined release is also indivisible. Reserve
            // all its content IDs together so alternate encodes cannot repeat
            // an episode already contained in this queue entry.
            let combined = !binding.has_item_positions();
            if combined {
                if !seen_files.insert(candidate.file)
                    || binding.items.iter().any(|i| seen_media.contains(&i.media))
                {
                    continue;
                }
                seen_media.extend(binding.items.iter().map(|i| i.media.clone()));
            } else if !seen_media.insert(node.id.clone()) {
                continue;
            }
            let (note, label) = match (node.kind, node.season_number, node.episode_number) {
                (erika_catalog::Kind::Episode, Some(season), Some(episode)) => (
                    PlayingNote::SeasonEpisode {
                        season: season.into(),
                        episode: episode.into(),
                    },
                    format!("S{season:02}E{episode:02} · {}", node.title),
                ),
                (erika_catalog::Kind::Movie, _, _) => (PlayingNote::Movie, node.title.clone()),
                _ => (
                    PlayingNote::Media {
                        label: node.title.clone(),
                    },
                    node.title.clone(),
                ),
            };
            let source = MediaSource::new(candidate.url);
            let combined_label = self.source_name(&source);
            let entry = Entry {
                label: if combined {
                    combined_label.clone()
                } else {
                    label
                },
                title: root.title.clone(),
                source,
                note: if combined {
                    PlayingNote::Media {
                        label: combined_label,
                    }
                } else {
                    note
                },
                media: (!combined).then_some(node.id),
                start: (!combined).then_some(item.start_seconds).flatten(),
                end: (!combined).then(|| binding.item_end(item)).flatten(),
            };
            entries.push((
                (node.season_number, node.episode_number, candidate.file),
                entry,
            ));
        }
        entries.sort_by_key(|(order, _)| *order);
        Ok(entries.into_iter().map(|(_, entry)| entry).collect())
    }

    pub(super) fn prepare_queue(&mut self, source: &MediaSource) {
        self.queue = match self.catalog_queue() {
            Ok(entries) => entries,
            Err(error) => {
                self.notify(Notice::OperationFailed {
                    reason: error.message,
                });
                Vec::new()
            }
        };
        if self.queue.is_empty() {
            self.queue = bern::files::file_path(&source.url)
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .and_then(|p| folder(&p).ok())
                .unwrap_or_default();
        }
        if !self.queue.iter().any(|e| e.source.url == source.url) {
            self.queue = vec![Entry {
                source: source.clone(),
                title: self.state.title.clone(),
                note: player::source_note(source),
                label: String::new(),
                media: None,
                start: None,
                end: None,
            }];
        }
        self.publish_queue(source);
    }

    pub(super) fn publish_queue(&mut self, source: &MediaSource) {
        let index = self
            .queue
            .iter()
            .position(|e| {
                e.source.url == source.url
                    && e.media
                        .as_ref()
                        .is_none_or(|id| self.media_playing.as_ref() == Some(id))
            })
            .or_else(|| {
                // A gap between known chapters has no current media ID, but
                // must not jump the list back to the file's first chapter.
                self.queue
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| {
                        e.source.url == source.url
                            && e.start.is_some_and(|start| start <= self.state.elapsed)
                    })
                    .max_by(|(_, a), (_, b)| a.start.unwrap().total_cmp(&b.start.unwrap()))
                    .map(|(i, _)| i)
            })
            .or_else(|| self.queue.iter().position(|e| e.source.url == source.url))
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
                name: if entry.label.is_empty() {
                    entry.title.clone()
                } else {
                    entry.label.clone()
                },
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
        let start = if let Some(begin) = entry.start {
            if start >= begin && entry.end.is_none_or(|end| start < end) {
                start
            } else {
                begin
            }
        } else {
            start
        };
        self.start(entry.title, entry.note, no_picture(), progress);
        self.file = file;
        self.media_playing = entry.media;
        self.publish_queue(&entry.source);
        self.load(Some(entry.source.clone()), start);
        self.recognize(&entry.source, fingerprint);
        Ok(())
    }

    pub(super) fn advance_at_end(&mut self) {
        // EOF consumes the entire physical file, including every chapter. Do
        // not reopen a later queue item that points back into that same file.
        let next = self
            .queue
            .iter()
            .enumerate()
            .skip(self.state.player.queue_index.max(0) as usize)
            .find(|(_, entry)| {
                self.current_source()
                    .is_none_or(|s| s.url != entry.source.url)
            })
            .map(|(index, _)| index as i64 + 1);
        if let Some(next) = next.filter(|_| {
            self.state.preferences.autoplay
                && self.state.preferences.playback_end == PlaybackEnd::Next
        }) {
            if let Err(error) = self.play_queue(next, false) {
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
