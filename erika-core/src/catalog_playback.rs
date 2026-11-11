//! Project a file's content binding onto the current playback clock. The
//! catalogue can be edited by another engine while this player stays open.
use super::*;
use erika_catalog::{FileBinding, Kind, MediaNode};
mod progress;

#[derive(Default)]
pub(super) struct Current {
    pub clock: Option<(i64, u64, u64)>,
    binding: Option<FileBinding>,
    root: Option<MediaNode>,
    items: Vec<MediaNode>,
    metadata_clock: Vec<(erika_catalog::MediaId, u64)>,
    original: Option<(String, PlayingNote, i64)>,
    progress: progress::Progress,
}

impl ErikaEngine {
    pub(super) fn read_playing_content(
        &mut self,
        clock: (i64, u64, u64),
        binding: Option<FileBinding>,
    ) -> erika_store::catalog::Result<()> {
        let mut items = Vec::new();
        if let Some(binding) = &binding {
            for item in &binding.items {
                if let Some(node) = self.db.catalog_node(&item.media)? {
                    items.push(node);
                }
            }
        }
        let root = items
            .first()
            .map(|node| self.db.catalog_node(&node.root))
            .transpose()?
            .flatten();
        let metadata_clock = items
            .iter()
            .chain(root.iter())
            .map(|n| (n.id.clone(), n.revision))
            .collect();
        let original = self.catalog_playback.original.take();
        let progress =
            if self.catalog_playback.clock.map(|(f, r, _)| (f, r)) == Some((clock.0, clock.1)) {
                std::mem::take(&mut self.catalog_playback.progress)
            } else {
                Default::default()
            };
        self.catalog_playback = Current {
            clock: Some(clock),
            binding,
            root,
            items,
            metadata_clock,
            original,
            progress,
        };
        self.project_playing_content(true);
        if let Some(source) = self.current_source().cloned() {
            self.prepare_queue(&source);
        }
        Ok(())
    }

    pub(super) fn project_playing_content(&mut self, force: bool) {
        let active = self
            .catalog_playback
            .binding
            .as_ref()
            .and_then(|b| b.playing_item(self.state.elapsed, self.state.duration))
            .and_then(|item| {
                self.catalog_playback
                    .items
                    .iter()
                    .find(|n| n.id == item.media)
            });
        let media = active.map(|node| node.id.clone());
        if !force && self.media_playing == media {
            return;
        }
        self.media_playing = media;
        if self.catalog_playback.original.is_none() && self.catalog_playback.root.is_some() {
            let original = if let Some(source) = self.current_source() {
                (
                    self.source_name(source),
                    player::source_note(source),
                    0,
                )
            } else {
                (
                    self.state.title.clone(),
                    self.state.subtitle.clone(),
                    self.state.player.episode,
                )
            };
            self.catalog_playback.original = Some(original);
        }
        if let Some(root) = &self.catalog_playback.root {
            self.state.title = root.title.clone();
            self.state.player.episode = active.and_then(|n| n.episode_number).map_or(0, i64::from);
            self.state.subtitle = match active {
                Some(node) if node.kind == Kind::Movie => PlayingNote::Movie,
                Some(node) if node.kind == Kind::Episode => PlayingNote::SeasonEpisode {
                    season: i64::from(node.season_number.expect("validated season")),
                    episode: i64::from(node.episode_number.expect("validated episode")),
                },
                Some(node) => PlayingNote::Media {
                    label: node.title.clone(),
                },
                None => PlayingNote::Media {
                    label: self
                        .current_source()
                        .map(|source| self.source_name(source))
                        .unwrap_or_default(),
                },
            };
        } else if let Some((title, note, episode)) = self.catalog_playback.original.take() {
            self.state.title = title;
            self.state.subtitle = note;
            self.state.player.episode = episode;
        }
        if let Some(source) = self.current_source().cloned() {
            self.publish_queue(&source);
        }
        self.plugin_event("mediaChanged", self.plugin_media());
    }

    /// Revision-only reads cover both bindings and their displayed metadata;
    /// a metadata refresh must not reopen playback or discard its measured time.
    pub(super) fn refresh_playing_catalog(&mut self) {
        let Some(file) = self.file.filter(|_| self.state.playing) else {
            return;
        };
        let changed = (|| -> erika_store::catalog::Result<bool> {
            Ok(self.catalog_playback.clock
                != Some((
                    file,
                    self.db.catalog_revision(file)?,
                    self.db.catalog_danmaku_revision(file)?,
                ))
                || !self
                    .db
                    .catalog_nodes_unchanged(&self.catalog_playback.metadata_clock)?)
        })();
        match changed {
            Ok(true) => {
                if let Err(error) = self.show_catalog(file) {
                    self.notify(Notice::OperationFailed {
                        reason: error.message,
                    });
                }
            }
            Ok(false) => self.project_playing_content(false),
            Err(error) => self.notify(Notice::OperationFailed {
                reason: error.message,
            }),
        }
    }
}
