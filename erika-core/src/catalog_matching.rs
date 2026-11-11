//! The content picker reads TMDB; the danmaku picker has a separate revision.
use super::*;
use erika_catalog::{
    BindingItem, Decision, FileBinding, Kind, MediaNode,
    artwork::ImageConfiguration,
    tmdb::{Season, Work},
};
use matching::Found;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Picker {
    pub nodes: Vec<MediaNode>,
    pub work: Option<Work>,
    pub season: Option<Season>,
    pub images: Option<ImageConfiguration>,
    revisions: Option<(erika_store::FileId, u64, u64)>,
    season_generation: u64,
}

pub(super) enum Reply {
    Search {
        revision: u64,
        nodes: Vec<MediaNode>,
        images: Option<ImageConfiguration>,
        error: String,
    },
    Work {
        revision: u64,
        id: String,
        result: erika_catalog::Result<Work>,
    },
    Season {
        revision: u64,
        generation: u64,
        id: String,
        result: erika_catalog::Result<Season>,
    },
}

fn command_error(e: erika_store::catalog::Error) -> CommandError {
    CommandError::new(e.code, e.message)
}

fn card(node: &MediaNode, images: Option<&ImageConfiguration>) -> MatchingShow {
    let poster = node
        .artwork
        .poster
        .as_ref()
        .and_then(|asset| images?.url(asset, 780).ok())
        .unwrap_or_default();
    MatchingShow {
        id: node.id.0.clone(),
        name: node.title.clone(),
        poster: MediaSource::new(poster),
        summary: node.overview.clone(),
        kind: format!(
            "{}{}",
            match node.kind {
                Kind::Tv => "TV",
                Kind::Movie => "Movie",
                _ => "",
            },
            node.date
                .get(..4)
                .map(|y| format!(" · {y}"))
                .unwrap_or_default()
        ),
        episode_count: 0,
        loading: false,
    }
}

fn option(node: &MediaNode) -> MatchingItem {
    let name = match (node.kind, node.season_number, node.episode_number) {
        (Kind::Season, Some(0), _) => format!("S00 · {}", node.title),
        (Kind::Season, Some(season), _) => format!("S{season:02} · {}", node.title),
        (Kind::Episode, Some(season), Some(episode)) => {
            format!("S{season:02}E{episode:02} · {}", node.title)
        }
        _ => node.title.clone(),
    };
    MatchingItem {
        id: node.id.0.clone(),
        name,
    }
}

impl ErikaEngine {
    pub(super) fn reset_match_picker(&mut self, target: MatchingTarget) {
        self.search_revision += 1;
        let open = self.state.matching.open;
        self.state.matching = matching::initial();
        self.state.matching.open = open;
        self.state.matching.target = target;
        self.catalog_picker = Picker::default();
        self.match_details = None;
        self.match_previews.clear();
        self.match_preview_queue.clear();
        self.match_previews_active = 0;
    }

    pub(super) fn capture_match_revisions(&mut self) -> Result<(), CommandError> {
        let file = self
            .file
            .ok_or_else(|| CommandError::new("no_file", "当前视频没有可绑定的文件身份"))?;
        self.catalog_picker.revisions = Some((
            file,
            self.db.catalog_revision(file).map_err(command_error)?,
            self.db
                .catalog_danmaku_revision(file)
                .map_err(command_error)?,
        ));
        Ok(())
    }

    pub(super) fn match_revisions(&self) -> Result<(erika_store::FileId, u64, u64), CommandError> {
        self.catalog_picker
            .revisions
            .filter(|(file, _, _)| self.file == Some(*file))
            .ok_or_else(|| CommandError::new("stale_file", "播放文件已改变，请重新打开匹配"))
    }

    pub(super) fn search_catalog_match(&mut self, query: String) -> Result<(), CommandError> {
        self.reset_match_picker(MatchingTarget::Content);
        self.capture_match_revisions()?;
        self.state.matching.loading = true;
        let (revision, client, inbox) = (
            self.search_revision,
            self.metadata.clone(),
            self.found.clone(),
        );
        let waker = bern::wake::waker();
        erika_catalog::client::spawn(async move {
            let mut nodes = Vec::new();
            let mut errors = Vec::new();
            for kind in [Kind::Tv, Kind::Movie] {
                match client.search(kind, &query, None, 1).await {
                    Ok(page) => nodes.extend(page.results),
                    Err(error) => errors.push(error.message),
                }
            }
            errors.sort();
            errors.dedup();
            let images = client.image_configuration().await.ok();
            inbox
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Found::Catalog(Reply::Search {
                    revision,
                    nodes,
                    images,
                    error: errors.join("；"),
                }));
            if let Some(waker) = waker {
                waker.wake();
            }
        });
        Ok(())
    }

    pub(super) fn choose_catalog_show(&mut self, id: String) -> Result<(), CommandError> {
        let node = self
            .catalog_picker
            .nodes
            .iter()
            .find(|n| n.id.0 == id)
            .ok_or_else(|| CommandError::new("unknown_show", "请先搜索作品"))?;
        let reference = node
            .reference
            .clone()
            .ok_or_else(|| CommandError::new("invalid_identity", "作品没有 TMDB 身份"))?;
        self.state.matching.selected = Some(card(node, self.catalog_picker.images.as_ref()));
        self.state.matching.seasons.clear();
        self.state.matching.season.clear();
        self.state.matching.episodes.clear();
        self.state.matching.error.clear();
        self.state.matching.loading = true;
        self.catalog_picker.work = None;
        self.catalog_picker.season = None;
        // A new selection invalidates details, including A → B → A responses.
        self.search_revision += 1;
        let (revision, client, inbox) = (
            self.search_revision,
            self.metadata.clone(),
            self.found.clone(),
        );
        let waker = bern::wake::waker();
        erika_catalog::client::spawn(async move {
            let result = match client.work(&reference).await {
                Ok(work) => Ok(client.with_artwork(work).await),
                Err(error) => Err(error),
            };
            inbox
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Found::Catalog(Reply::Work {
                    revision,
                    id,
                    result,
                }));
            if let Some(waker) = waker {
                waker.wake();
            }
        });
        Ok(())
    }

    pub(super) fn choose_catalog_season(&mut self, id: String) -> Result<(), CommandError> {
        if self.state.matching.target != MatchingTarget::Content {
            return Err(CommandError::new("wrong_target", "请先选择媒体作品"));
        }
        let work = self
            .catalog_picker
            .work
            .clone()
            .ok_or_else(|| CommandError::new("no_show", "请先选择作品"))?;
        let number = work
            .seasons
            .iter()
            .find(|s| s.id.0 == id)
            .and_then(|s| s.season_number)
            .ok_or_else(|| CommandError::new("no_season", "请选择作品中的季"))?;
        self.state.matching.season = id.clone();
        self.state.matching.episodes.clear();
        self.state.matching.error.clear();
        self.state.matching.loading = true;
        self.catalog_picker.season = None;
        self.catalog_picker.season_generation += 1;
        let generation = self.catalog_picker.season_generation;
        let (revision, client, inbox) = (
            self.search_revision,
            self.metadata.clone(),
            self.found.clone(),
        );
        let waker = bern::wake::waker();
        erika_catalog::client::spawn(async move {
            let result = client.season(&work, number).await;
            inbox
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Found::Catalog(Reply::Season {
                    revision,
                    generation,
                    id,
                    result,
                }));
            if let Some(waker) = waker {
                waker.wake();
            }
        });
        Ok(())
    }

    pub(super) fn take_catalog_match(&mut self, reply: Reply) {
        if self.state.matching.target != MatchingTarget::Content {
            return;
        }
        match reply {
            Reply::Search {
                revision,
                nodes,
                images,
                error,
            } if revision == self.search_revision => {
                self.state.matching.loading = false;
                self.state.matching.shows =
                    nodes.iter().map(|n| card(n, images.as_ref())).collect();
                self.state.matching.error = if nodes.is_empty() && error.is_empty() {
                    "没有找到作品，请换一个名称搜索".into()
                } else {
                    error
                };
                self.catalog_picker.nodes = nodes;
                self.catalog_picker.images = images;
            }
            Reply::Work {
                revision,
                id,
                result,
            } if revision == self.search_revision
                && self
                    .state
                    .matching
                    .selected
                    .as_ref()
                    .is_some_and(|s| s.id == id) =>
            {
                self.state.matching.loading = false;
                match result {
                    Ok(work) if work.node.id.0 == id => {
                        self.state.matching.selected =
                            Some(card(&work.node, self.catalog_picker.images.as_ref()));
                        self.state.matching.seasons = work.seasons.iter().map(option).collect();
                        if work.node.kind == Kind::Movie {
                            self.state.matching.episodes = vec![option(&work.node)];
                        }
                        // Only an actual sole season can be preselected. Never guess S01.
                        let sole = (work.seasons.len() == 1).then(|| work.seasons[0].id.0.clone());
                        self.catalog_picker.work = Some(work);
                        if let Some(id) = sole {
                            if let Err(error) = self.choose_catalog_season(id) {
                                self.state.matching.error = error.message;
                            }
                        }
                    }
                    Ok(_) => self.state.matching.error = "返回的作品身份不一致".into(),
                    Err(error) => self.state.matching.error = error.message,
                }
            }
            Reply::Season {
                revision,
                generation,
                id,
                result,
            } if revision == self.search_revision
                && generation == self.catalog_picker.season_generation
                && self.state.matching.season == id =>
            {
                self.state.matching.loading = false;
                match result {
                    Ok(season) if season.node.id.0 == id => {
                        self.state.matching.episodes = season.episodes.iter().map(option).collect();
                        if season.episodes.is_empty() {
                            self.state.matching.error = "这一季尚无分集资料".into();
                        }
                        self.catalog_picker.season = Some(season);
                    }
                    Ok(_) => self.state.matching.error = "返回的季身份不一致".into(),
                    Err(error) => self.state.matching.error = error.message,
                }
            }
            _ => {}
        }
    }

    pub(super) fn confirm_catalog_item(&mut self, id: String) -> Result<(), CommandError> {
        self.require_playing()?;
        let (file, revision, _) = self.match_revisions()?;
        let work = self
            .catalog_picker
            .work
            .as_ref()
            .ok_or_else(|| CommandError::new("no_show", "请先选择作品"))?;
        let mut nodes = vec![work.node.clone()];
        let item = if work.node.kind == Kind::Movie && work.node.id.0 == id {
            work.node.clone()
        } else {
            let season = self
                .catalog_picker
                .season
                .as_ref()
                .ok_or_else(|| CommandError::new("no_season", "请先选择季"))?;
            let item = season
                .episodes
                .iter()
                .find(|n| n.id.0 == id)
                .ok_or_else(|| CommandError::new("no_episode", "请选择列表中的分集"))?
                .clone();
            nodes.push(season.node.clone());
            nodes.push(item.clone());
            item
        };
        let binding = FileBinding {
            file,
            revision,
            decision: Decision::Manual,
            items: vec![BindingItem {
                media: item.id.clone(),
                start_seconds: None,
                end_seconds: None,
            }],
            evidence: "manual:tmdb-picker".into(),
        };
        self.db
            .catalog_apply(&nodes, &[], &[binding], &[])
            .map_err(command_error)?;
        self.match_revision += 1;
        self.show_catalog(file).map_err(command_error)?;
        self.state.matching.open = false;
        self.notify(Notice::Matched { title: item.title });
        self.catalog_changed();
        Ok(())
    }

    pub(super) fn block_catalog_match(&mut self) -> Result<(), CommandError> {
        self.require_playing()?;
        let (file, revision, _) = self.match_revisions()?;
        self.db
            .catalog_apply(
                &[],
                &[],
                &[FileBinding {
                    file,
                    revision,
                    decision: Decision::Blocked,
                    items: Vec::new(),
                    evidence: "manual:no-content-binding".into(),
                }],
                &[],
            )
            .map_err(command_error)?;
        self.match_revision += 1;
        self.media_playing = None;
        self.state.player.episode = 0;
        if let Some(source) = self.current_source().cloned() {
            self.state.subtitle = player::source_note(&source);
            self.state.title = self.source_name(&source);
            self.prepare_queue(&source);
        }
        self.state.matching.open = false;
        self.catalog_changed();
        Ok(())
    }

    fn catalog_changed(&self) {
        #[cfg(feature = "bern-module-anime")]
        erika_anime_protocol::anime::calls::history_changed();
        #[cfg(feature = "bern-module-library")]
        erika_library_protocol::library::calls::history_changed();
    }
}
