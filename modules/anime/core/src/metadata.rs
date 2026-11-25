//! Anonymous source-list enrichment. This cache is presentation evidence only:
//! it never aliases files or grants permission to write a provider account.
use super::*;
use erika_catalog::{
    Client, Kind, MediaId, MediaNode, Provider, ProviderLink, ProviderRef, Relation,
    artwork::ImageConfiguration, tmdb,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

const SOURCE_CACHE: &str = "anime.tmdb.sources.v3";
const SOURCE_TTL: i64 = 24 * 60 * 60;
const MAX_SOURCES: usize = 4096;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SourceWork {
    source: ProviderRef,
    media: MediaId,
    #[serde(default)]
    updated_at: i64,
    /// Whether the entry's own subjects were recorded with this result.
    /// Results from before that was done are refreshed once.
    #[serde(default)]
    evidence: bool,
}
#[derive(Default)]
pub(super) struct Runtime {
    pub client: Option<Client>,
    sources: Vec<SourceWork>,
    source_cache: Option<String>,
    configuring: bool,
    attempted: HashSet<(u64, i64)>,
    waiting: VecDeque<(u64, i64, String)>,
    running: HashSet<(u64, i64)>,
    selection: u64,
    retry_armed: bool,
    bundling: bool,
    bundle_generation: Option<u64>,
    warmed_bangumi: HashSet<i64>,
    #[cfg(test)]
    pub network_disabled: bool,
}
impl Runtime {
    pub(super) fn network_enabled(&self) -> bool {
        #[cfg(test)]
        {
            !self.network_disabled
        }
        #[cfg(not(test))]
        {
            true
        }
    }
    pub fn cancel_opened(&mut self) {
        self.selection = self.selection.wrapping_add(1);
    }
    pub(super) fn client(&self) -> Client {
        self.client
            .clone()
            .unwrap_or_else(|| Client::built_in("zh-CN").expect("built-in metadata address"))
    }
    pub(super) fn is_idle(&self) -> bool {
        self.running.is_empty()
    }
}
/// What a calendar entry says about itself once its work is known: the TMDB
/// season it airs as (from its own dates) and its own provider subjects.
/// Stored as unconfirmed links; they show public reviews but never write an
/// account until the user confirms them.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Candidates {
    pub season: Option<ProviderRef>,
    pub subjects: Vec<ProviderRef>,
    /// `(provider, id, summary)` read while resolving the calendar entry.
    pub summaries: Vec<(Provider, String, String)>,
    /// The entry's own evidence was read (even if it names no season).
    pub recorded: bool,
}
pub(super) const CALENDAR_EVIDENCE: &str = "新番表关联，待确认";
pub(super) enum Finished {
    Configuration(erika_catalog::Result<ImageConfiguration>),
    Bundle {
        generation: u64,
        result: erika_catalog::Result<tmdb::SeasonBundle>,
    },
    Source {
        generation: u64,
        id: i64,
        previous: Option<SourceWork>,
        result: erika_catalog::Result<tmdb::Work>,
        candidates: Candidates,
    },
    Detail {
        selection: u64,
        opened: String,
        previous: Option<SourceWork>,
        result: erika_catalog::Result<(tmdb::Work, Option<tmdb::Season>)>,
        candidates: Candidates,
    },
    Search {
        answer: bern::prelude::Answer<anime::SearchResults>,
        result: erika_catalog::Result<Vec<MediaNode>>,
    },
    BangumiMeta(Vec<(i64, erika_dandanplay::bangumi::Subject)>),
}
fn source_ref(id: i64) -> Option<ProviderRef> {
    // Calendar IDs are public DDP/Bangumi identities. A negative custom ID
    // has no Misaka server scope and cannot be repurposed as a DDP subject.
    if id <= 0 {
        return None;
    }
    let bangumi = erika_dandanplay::bangumi::subject_id(id);
    Some(ProviderRef {
        provider: if bangumi.is_some() {
            Provider::Bangumi
        } else {
            Provider::Dandanplay
        },
        scope: String::new(),
        kind: Kind::Subject,
        id: bangumi.unwrap_or(id).to_string(),
    })
}
pub(super) fn source_key(id: i64) -> String {
    format!("source:{id}")
}
fn normalize_sources(mut sources: Vec<SourceWork>) -> Vec<SourceWork> {
    let now = erika_store::now();
    sources.retain(|s| {
        s.updated_at >= 0
            && s.updated_at <= now
            && s.source.validate().is_ok()
            && matches!(s.source.provider, Provider::Dandanplay | Provider::Bangumi)
            && s.source.kind == Kind::Subject
            && s.source.scope.is_empty()
            && s.source.id.parse::<u64>().is_ok_and(|id| id > 0)
            && MediaId::parse(&s.media.0).is_ok()
    });
    // Keep the newest record for each provider identity, including repeated refreshes.
    sources.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
    let mut seen = HashSet::new();
    sources.retain(|s| seen.insert(s.source.clone()));
    sources.truncate(MAX_SOURCES);
    sources
}
fn read_sources(text: Option<&str>) -> erika_store::catalog::Result<Vec<SourceWork>> {
    match text {
        None => Ok(Vec::new()),
        Some(text) => serde_json::from_str(text)
            .map(normalize_sources)
            .map_err(|_| erika_store::catalog::Error::invalid("新番身份缓存格式不正确")),
    }
}

fn nodes(work: &tmdb::Work, season: Option<&tmdb::Season>) -> Vec<MediaNode> {
    let mut nodes = vec![work.node.clone()];
    nodes.extend(work.seasons.clone());
    for split in &work.split_seasons {
        nodes.retain(|n| n.id != split.node.id && n.reference != split.node.reference);
        nodes.push(split.node.clone());
        for ep in &split.episodes {
            nodes.retain(|n| n.reference != ep.reference);
            nodes.push(ep.clone());
        }
    }
    if let Some(season) = season {
        nodes.retain(|n| n.id != season.node.id && n.reference != season.node.reference);
        nodes.push(season.node.clone());
        for ep in &season.episodes {
            nodes.retain(|n| n.reference != ep.reference);
            nodes.push(ep.clone());
        }
    }
    nodes
}
fn deliver_metadata(inbox: &Arc<Mutex<Vec<Done>>>) -> impl FnOnce(Finished) + Send + 'static {
    let deliver = receiver(inbox);
    move |result| deliver(Done::Metadata(result))
}

impl AnimeEngine {
    /// Cache only real TMDB nodes; a source list is not a content binding.
    fn save_metadata(
        &mut self,
        work: &tmdb::Work,
        season: Option<&tmdb::Season>,
    ) -> erika_store::catalog::Result<(MediaId, Option<MediaId>)> {
        let stored = self.db.catalog_save_nodes(&nodes(work, season))?;
        self.catalog_written = true;
        if let Some(key) = content::original_overview_key(&work.node) {
            if !work.node.overview.trim().is_empty() {
                self.db.set_setting(&key, None);
            } else if !work.original_overview.trim().is_empty() {
                self.db
                    .set_setting(&key, Some(work.original_overview.trim()));
            }
        }
        let resolve = |node: &MediaNode| {
            stored
                .iter()
                .find(|n| n.reference == node.reference)
                .map(|n| n.id.clone())
                .ok_or_else(|| erika_store::catalog::Error::invalid("媒体资料未保存"))
        };
        Ok((
            resolve(&work.node)?,
            season.map(|s| resolve(&s.node)).transpose()?,
        ))
    }

    pub(super) fn source_intro(&self, key: &str) -> Option<&Intro> {
        self.season.iter().find(|i| {
            !i.is_restricted && source_ref(i.anime_id).is_some() && source_key(i.anime_id) == key
        })
    }

    pub(super) fn resolved_source(&self, key: &str) -> Option<MediaId> {
        let source = source_ref(self.source_intro(key)?.anime_id)?;
        let id = &self
            .metadata
            .sources
            .iter()
            .find(|s| s.source == source)?
            .media;
        self.db
            .catalog_node(id)
            .ok()
            .flatten()
            .filter(|n| n.kind.work())
            .map(|n| n.id)
    }

    /// The current calendar's show that resolved to this work.
    pub(super) fn calendar_intro(&self, work: &MediaId) -> Option<&Intro> {
        self.season.iter().find(|i| {
            self.source_intro(&source_key(i.anime_id)).is_some()
                && source_ref(i.anime_id).is_some_and(|source| {
                    self.metadata
                        .sources
                        .iter()
                        .any(|s| s.source == source && &s.media == work)
                })
        })
    }

    pub(super) fn on_calendar(&self, work: &MediaId) -> bool {
        self.calendar_intro(work).is_some()
    }

    pub(super) fn calendar_season(
        &self,
        work: &MediaId,
        children: &[MediaNode],
    ) -> Option<MediaNode> {
        if let Some(intro) = self.calendar_intro(work)
            && let Some(ddp_ref) = source_ref(intro.anime_id)
        {
            if let Some((_, best)) = children
                .iter()
                .filter(|n| n.kind == Kind::Season)
                .filter_map(|child| {
                    let link = self
                        .db
                        .catalog_links(&child.id)
                        .unwrap_or_default()
                        .into_iter()
                        .find(|l| l.reference == ddp_ref && (l.confirmed || !l.manual))?;
                    Some((
                        (
                            link.confirmed,
                            link.relation != Relation::PartOf,
                            child.season_number,
                        ),
                        child.clone(),
                    ))
                })
                .max_by_key(|(rank, _)| *rank)
            {
                return Some(best);
            }
        }
        children
            .iter()
            .filter(|n| {
                n.kind == Kind::Season
                    && n.season_number.is_some_and(|s| s > 0)
                    && !n.date.is_empty()
            })
            .max_by(|a, b| {
                a.date
                    .cmp(&b.date)
                    .then(a.season_number.cmp(&b.season_number))
            })
            .or_else(|| {
                children
                    .iter()
                    .filter(|n| n.kind == Kind::Season && n.season_number.is_some_and(|s| s > 0))
                    .max_by_key(|n| n.season_number)
            })
            .cloned()
    }

    pub(super) fn source_preview(&self, key: &str) -> Option<Show> {
        let intro = self.source_intro(key)?;
        let mut show = shows::untitled();
        show.id = key.into();
        show.title = intro.anime_title.clone();
        show.summary = intro.summary.trim().into();
        Some(show)
    }

    fn source_snapshot(&self, id: i64) -> Option<SourceWork> {
        let source = source_ref(id)?;
        self.metadata
            .sources
            .iter()
            .find(|s| s.source == source)
            .cloned()
    }

    pub(super) fn restore_metadata(&mut self) {
        let result = (|| {
            let text = self.db.try_setting(SOURCE_CACHE)?;
            if text != self.metadata.source_cache {
                let sources = read_sources(text.as_deref())?;
                self.metadata.sources = sources;
                self.metadata.source_cache = text;
            }
            Ok::<_, erika_store::catalog::Error>(())
        })();
        if let Err(error) = result {
            self.state.error = error.message;
        }
    }
    pub(super) fn project_calendar(&mut self, calendar: &mut season::Season) {
        if self.calendar_projection.is_none() {
            self.restore_metadata();
            let images = self.db.catalog_images();
            let built = self
                .season
                .iter()
                .filter_map(|i| {
                    let key = source_key(i.anime_id);
                    let target = self.resolved_source(&key)?;
                    let node = self.db.catalog_node(&target).ok().flatten()?;
                    let airing = if node.kind == Kind::Tv {
                        let children = self.db.catalog_children(&node.id).unwrap_or_default();
                        self.calendar_season(&node.id, &children)
                    } else {
                        None
                    };
                    let (title, poster) = if let Some(season_node) = airing
                        .as_ref()
                        .filter(|s| s.season_number.is_some_and(|n| n > 1))
                    {
                        let (_, bgm_cn, bgm_cover) =
                            reviews::cached_season_meta(&self.db, &season_node.id);
                        let n = season_node.season_number.unwrap_or(2);
                        let title = bgm_cn
                            .or_else(|| {
                                let raw = i.anime_title.trim();
                                (!raw.is_empty() && content::is_chinese(raw))
                                    .then(|| raw.to_string())
                            })
                            .unwrap_or_else(|| format!("{} 第 {n} 季", node.title));
                        let poster = if let Some(p) = season_node.artwork.poster.as_ref() {
                            images.url(Some(p), 780)
                        } else if let Some(cover) = bgm_cover {
                            cover
                        } else {
                            images.url(node.artwork.poster.as_ref(), 780)
                        };
                        (title, poster)
                    } else {
                        let poster_ref = airing
                            .as_ref()
                            .and_then(|s| s.artwork.poster.as_ref())
                            .or(node.artwork.poster.as_ref());
                        (node.title.clone(), images.url(poster_ref, 780))
                    };
                    Some((key, (node.id.0, title, poster)))
                })
                .collect::<HashMap<_, _>>();
            self.calendar_projection = Some(built);
        }
        let map = self.calendar_projection.as_ref().unwrap();
        let project = |row: &mut anime::Broadcast| {
            if let Some((id, title, poster)) = map.get(&row.id) {
                row.id = id.clone();
                row.title = title.clone();
                row.poster = MediaSource::new(poster);
            }
        };
        for day in &mut calendar.week {
            for row in &mut day.shows {
                project(row);
            }
        }
        calendar.relayout();
    }
    /// Retry calendar enrichment once the service has room again. At most one
    /// wake is armed and it does not retain the engine.
    fn arm_metadata_retry(&mut self, after: Option<u64>) {
        if self.metadata.retry_armed {
            return;
        }
        self.metadata.retry_armed = true;
        #[cfg(not(test))]
        {
            let seconds = after.unwrap_or(30).clamp(10, 300);
            let inbox = std::sync::Arc::downgrade(&self.inbox);
            let wake = bern::wake::waker();
            erika_catalog::client::spawn(async move {
                #[cfg(not(target_arch = "wasm32"))]
                std::thread::sleep(std::time::Duration::from_secs(seconds));
                #[cfg(target_arch = "wasm32")]
                gloo_timers::future::TimeoutFuture::new(seconds as u32 * 1000).await;
                if let Some(inbox) = inbox.upgrade() {
                    inbox
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(Done::RetryMetadata);
                    if let Some(wake) = wake {
                        wake.wake();
                    }
                }
            });
        }
        #[cfg(test)]
        let _ = after;
    }
    pub(super) fn resume_metadata_retry(&mut self) {
        self.metadata.retry_armed = false;
        self.load_metadata();
    }
    pub(super) fn load_metadata(&mut self) {
        self.restore_metadata();
        if self.metadata.network_enabled()
            && !self.db.catalog_images_fresh(erika_store::now())
            && !self.metadata.configuring
        {
            self.metadata.configuring = true;
            let client = self.metadata.client();
            let deliver = deliver_metadata(&self.inbox);
            erika_catalog::client::spawn(async move {
                let result = client.image_configuration().await;
                deliver(Finished::Configuration(result));
            });
        }
        self.metadata
            .waiting
            .retain(|(g, _, _)| *g == self.generation);
        self.metadata
            .attempted
            .retain(|(g, _)| *g == self.generation);
        let now = erika_store::now();
        let any_stale = self.season.iter().any(|intro| {
            !intro.is_restricted
                && source_ref(intro.anime_id).is_some_and(|source| {
                    !self.metadata.sources.iter().any(|s| {
                        s.source == source
                            && s.evidence
                            && s.updated_at <= now
                            && now - s.updated_at < SOURCE_TTL
                            && self.db.catalog_node(&s.media).ok().flatten().is_some()
                    })
                })
        });
        if self.metadata.network_enabled()
            && any_stale
            && self.metadata.bundle_generation != Some(self.generation)
        {
            self.metadata.bundle_generation = Some(self.generation);
            self.metadata.bundling = true;
            let generation = self.generation;
            let client = self.metadata.client();
            let deliver = deliver_metadata(&self.inbox);
            erika_catalog::client::spawn(async move {
                let result = client.season_bundle().await;
                deliver(Finished::Bundle { generation, result });
            });
            return;
        }
        if self.metadata.bundling {
            return;
        }
        for intro in &self.season {
            if intro.is_restricted {
                continue;
            }
            let Some(source) = source_ref(intro.anime_id) else {
                continue;
            };
            let fresh = self.metadata.sources.iter().any(|s| {
                s.source == source
                    && s.evidence
                    && s.updated_at <= now
                    && now - s.updated_at < SOURCE_TTL
                    && self.db.catalog_node(&s.media).ok().flatten().is_some()
            });
            let key = (self.generation, intro.anime_id);
            if !fresh
                && !self.metadata.attempted.contains(&key)
                && !self.metadata.running.contains(&key)
                && !self
                    .metadata
                    .waiting
                    .iter()
                    .any(|(g, id, _)| (*g, *id) == key)
            {
                self.metadata
                    .waiting
                    .push_back((key.0, key.1, intro.anime_title.clone()));
            }
        }
        self.pump_metadata();
    }
    fn pump_metadata(&mut self) {
        if !self.metadata.network_enabled() {
            return;
        }
        self.restore_metadata();
        while self.metadata.running.len() < 4 {
            let Some((generation, id, title)) = self.metadata.waiting.pop_front() else {
                break;
            };
            self.metadata.running.insert((generation, id));
            self.metadata.attempted.insert((generation, id));
            let previous = self.source_snapshot(id);
            let known = self
                .resolved_source(&source_key(id))
                .and_then(|id| self.db.catalog_node(&id).ok().flatten())
                .and_then(|n| n.reference);
            let client = self.metadata.client();
            let deliver = deliver_metadata(&self.inbox);
            erika_catalog::client::spawn(async move {
                let (result, candidates) = match resolve_source(&client, id, &title, known).await {
                    Ok((work, candidates)) => (Ok(work), candidates),
                    Err(error) => (Err(error), Candidates::default()),
                };
                deliver(Finished::Source {
                    generation,
                    id,
                    previous,
                    result,
                    candidates,
                });
            });
        }
    }
    pub(super) fn refresh_metadata(&mut self) {
        self.metadata.cancel_opened();
        self.state.details_loading = false;
        self.state.details_failed = false;
        self.state.details_error.clear();
        let opened = self.state.opened.clone();
        if opened.is_empty() || !self.metadata.network_enabled() {
            return;
        }
        self.restore_metadata();
        let unresolved_source = self
            .source_intro(&opened)
            .map(|i| (i.anime_id, i.anime_title.clone()));
        let calendar = unresolved_source.clone().or_else(|| {
            let intro = self.calendar_intro(&MediaId(opened.clone()))?;
            Some((intro.anime_id, intro.anime_title.clone()))
        });
        let source = unresolved_source.or_else(|| {
            let (id, title) = calendar.clone()?;
            (!self.source_snapshot(id).is_some_and(|s| s.evidence)).then_some((id, title))
        });
        let previous = source
            .as_ref()
            .and_then(|(id, _)| self.source_snapshot(*id));
        let reference = if self.source_intro(&opened).is_none() {
            self.db
                .catalog_node(&MediaId(opened.clone()))
                .ok()
                .flatten()
                .and_then(|n| n.reference)
        } else {
            None
        };
        if source.is_none() && reference.is_none() {
            return;
        }
        let selected = self
            .state
            .seasons
            .iter()
            .find(|s| s.id == self.state.selected_season)
            .and_then(|s| u32::try_from(s.number).ok());
        let merge_seasons = self.db.setting("merge_seasons").as_deref() == Some("true");
        let selection = self.metadata.selection;
        let from_calendar = calendar.is_some();
        self.state.details_loading = true;
        let client = self.metadata.client();
        let deliver = deliver_metadata(&self.inbox);
        erika_catalog::client::spawn(async move {
            let mut candidates = Candidates::default();
            let result = async {
                let mut work = match (reference, source) {
                    (known, Some((id, title))) => {
                        let (work, found) = resolve_source(&client, id, &title, known).await?;
                        candidates = found;
                        work
                    }
                    (Some(reference), None) => {
                        client.with_artwork(client.work(&reference).await?).await
                    }
                    (None, None) => return Err(erika_catalog::Error::invalid("作品不存在")),
                };
                let regular_numbers: Vec<u32> = work
                    .seasons
                    .iter()
                    .filter_map(|s| s.season_number.filter(|n| *n > 0))
                    .collect();
                // A show on this season's calendar is airing the season its
                // own dates point at, else the newest one.
                let airing = from_calendar
                    .then(|| {
                        candidates
                            .season
                            .as_ref()
                            .and_then(|r| {
                                work.seasons
                                    .iter()
                                    .find(|s| s.reference.as_ref() == Some(r))
                            })
                            .or_else(|| {
                                work.seasons
                                    .iter()
                                    .filter(|s| {
                                        s.season_number.is_some_and(|n| n > 0) && !s.date.is_empty()
                                    })
                                    .max_by(|a, b| a.date.cmp(&b.date))
                            })
                            .and_then(|s| s.season_number)
                    })
                    .flatten();
                let number = selected
                    .or(airing)
                    .or_else(|| regular_numbers.iter().copied().min())
                    .or_else(|| work.seasons.first().and_then(|s| s.season_number));
                let season = match number {
                    Some(n) if work.node.kind == Kind::Tv => Some(client.season(&work, n).await?),
                    _ => None,
                };
                if merge_seasons
                    && work.node.kind == Kind::Tv
                    && work.split_seasons.is_empty()
                    && regular_numbers.len() > 1
                {
                    for n in regular_numbers.into_iter().take(12) {
                        if Some(n) == number {
                            continue;
                        }
                        if let Ok(extra) = client.season(&work, n).await {
                            work.split_seasons.push(extra);
                        }
                    }
                }
                Ok((work, season))
            }
            .await;
            deliver(Finished::Detail {
                selection,
                opened,
                previous,
                result,
                candidates,
            });
        });
    }
    pub(super) fn take_metadata(&mut self, result: Finished) {
        match result {
            Finished::Search { answer, result } => {
                if answer.cancellation().is_cancelled() {
                    return;
                }
                let result = result
                    .map_err(|e| CommandError::new("metadata", e.message))
                    .and_then(|nodes| self.keep_search_results(nodes));
                answer.send(result);
            }
            Finished::BangumiMeta(rows) => {
                let mut changed = false;
                for (subject, info) in rows {
                    if reviews::save_bangumi_subject(&self.db, subject, &info) {
                        changed = true;
                    }
                }
                if changed {
                    self.catalog_written = true;
                    self.show_season();
                    self.show_opened();
                }
            }
            Finished::Configuration(Ok(config)) => {
                self.metadata.configuring = false;
                match self.db.catalog_save_images(&config) {
                    Ok(()) => self.catalog_written = true,
                    Err(e) => self.state.error = e.message,
                }
                self.show_season();
                self.show_opened();
                self.show_watching();
            }
            Finished::Configuration(Err(_)) => {
                self.metadata.configuring = false;
            }
            Finished::Bundle { generation, result } => {
                self.metadata.bundling = false;
                if generation == self.generation
                    && let Ok(bundle) = result
                {
                    if let Some(config) = &bundle.images {
                        self.catalog_written |= self.db.catalog_save_images(config).is_ok();
                    }
                    let mut changed = false;
                    for item in bundle.items {
                        if item.validate().is_err() {
                            continue;
                        }
                        let previous = self.source_snapshot(item.anime_id);
                        let summaries = if item.summary.trim().is_empty() {
                            Vec::new()
                        } else {
                            vec![(
                                Provider::Dandanplay,
                                item.anime_id.to_string(),
                                item.summary,
                            )]
                        };
                        let candidates = Candidates {
                            season: item.season,
                            subjects: item.subjects,
                            summaries,
                            recorded: true,
                        };
                        if let Ok((media, _)) = self.save_metadata(&item.work, None)
                            && self
                                .remember_source(
                                    item.anime_id,
                                    media.clone(),
                                    previous.as_ref(),
                                    true,
                                )
                                .is_ok()
                        {
                            let _ = self.keep_candidates(&media, &candidates);
                            changed = true;
                        }
                    }
                    if changed {
                        self.show_season();
                        self.show_opened();
                        self.show_watching();
                        self.warm_calendar_bangumi();
                    }
                }
                self.load_metadata();
            }
            Finished::Source {
                generation,
                id,
                previous,
                result,
                candidates,
            } => {
                self.metadata.running.remove(&(generation, id));
                if generation == self.generation {
                    match result {
                        Ok(work) => match self.save_metadata(&work, None) {
                            Ok((media, _)) => {
                                if let Err(error) = self
                                    .remember_source(
                                        id,
                                        media.clone(),
                                        previous.as_ref(),
                                        candidates.recorded,
                                    )
                                    .and_then(|()| self.keep_candidates(&media, &candidates))
                                {
                                    self.state.error = error.message;
                                }
                                self.warm_calendar_bangumi();
                                self.show_season();
                                if self.state.opened == media.0 {
                                    self.show_opened();
                                    if !self.state.comments_loading && !self.state.has_comments {
                                        self.load_reviews();
                                    }
                                }
                            }
                            Err(e) => self.state.error = e.message,
                        },
                        // One show without a confirmed work is not a page
                        // error: its row keeps the calendar's own title and
                        // picture. A busy or unreachable service is retried.
                        Err(e) => {
                            if matches!(
                                e.code,
                                erika_catalog::client::ErrorCode::RateLimited
                                    | erika_catalog::client::ErrorCode::Unavailable
                            ) {
                                self.metadata.attempted.remove(&(generation, id));
                                self.arm_metadata_retry(e.retry_after);
                            }
                        }
                    }
                }
                self.pump_metadata();
            }
            Finished::Detail {
                selection,
                opened,
                previous,
                result,
                candidates,
            } => {
                if selection != self.metadata.selection || opened != self.state.opened {
                    return;
                }
                self.state.details_loading = false;
                let result =
                    result
                        .map_err(erika_store::catalog::Error::from)
                        .and_then(|(work, season)| {
                            if self.source_intro(&opened).is_none() {
                                let current = self
                                    .db
                                    .catalog_node(&MediaId(opened.clone()))?
                                    .ok_or_else(|| {
                                        erika_store::catalog::Error::conflict(
                                            "作品已改变，请重新打开详情",
                                        )
                                    })?;
                                if current.reference != work.node.reference {
                                    return Err(erika_store::catalog::Error::conflict(
                                        "返回的作品与当前详情不一致",
                                    ));
                                }
                            }
                            let saved = self.save_metadata(&work, season.as_ref())?;
                            let source_id =
                                self.source_intro(&opened).map(|i| i.anime_id).or_else(|| {
                                    candidates
                                        .recorded
                                        .then(|| self.calendar_intro(&saved.0).map(|i| i.anime_id))
                                        .flatten()
                                });
                            if let Some(id) = source_id {
                                self.remember_source(
                                    id,
                                    saved.0.clone(),
                                    previous.as_ref(),
                                    candidates.recorded,
                                )?;
                                self.keep_candidates(&saved.0, &candidates)?;
                            }
                            Ok(saved)
                        });
                match result {
                    Ok((work, season)) => {
                        self.state.details_failed = false;
                        self.state.details_error.clear();
                        let target = work.0;
                        let selected = season.map(|s| s.0).unwrap_or_default();
                        if self.state.opened != target || self.state.selected_season != selected {
                            self.syncs.change_view(&mut self.state);
                            self.state.review_open = false;
                            self.state.rating_open = false;
                            self.state.review_loading = false;
                            self.state.review_saving = false;
                        }
                        self.state.opened = target;
                        self.state.selected_season = selected;
                        self.show_opened();
                        self.show_season();
                        self.load_reviews();
                        self.sync_opened();
                    }
                    Err(e) => {
                        self.state.details_failed = true;
                        self.state.details_error = e.message;
                    }
                }
            }
        }
    }
    /// Pre-warm Bangumi per-season titles and Japanese key visuals for split
    /// multi-season calendar entries that have no TMDB per-season poster.
    fn warm_calendar_bangumi(&mut self) {
        if !self.metadata.network_enabled() {
            return;
        }
        let mut targets = Vec::new();
        for intro in &self.season {
            let key = source_key(intro.anime_id);
            let Some(work_id) = self.resolved_source(&key) else {
                continue;
            };
            let children = self.db.catalog_children(&work_id).unwrap_or_default();
            let Some(season_node) = self.calendar_season(&work_id, &children) else {
                continue;
            };
            if season_node.season_number.is_none_or(|n| n <= 1)
                || season_node.artwork.poster.is_some()
            {
                continue;
            }
            let Some(subject) = reviews::reading_subject(&self.db, &season_node.id) else {
                continue;
            };
            if self
                .db
                .setting(&format!("bangumi.cover.{subject}"))
                .is_some()
                || !self.metadata.warmed_bangumi.insert(subject)
            {
                continue;
            }
            targets.push(subject);
            if targets.len() >= 8 {
                break;
            }
        }
        if targets.is_empty() {
            return;
        }
        let deliver = deliver_metadata(&self.inbox);
        erika_catalog::client::spawn(async move {
            let mut rows = Vec::new();
            for subject in targets {
                if let Ok(info) = erika_dandanplay::bangumi::subject(subject).await {
                    rows.push((subject, info));
                }
            }
            if !rows.is_empty() {
                deliver(Finished::BangumiMeta(rows));
            }
        });
    }

    /// Record the calendar's own subjects on the season they air as (the work
    /// itself for a film), unconfirmed. A user's decision is never replaced.
    fn keep_candidates(
        &self,
        work: &MediaId,
        candidates: &Candidates,
    ) -> erika_store::catalog::Result<()> {
        for (provider, id, summary) in &candidates.summaries {
            let summary = summary.trim();
            if !summary.is_empty() {
                let prefix = match provider {
                    Provider::Bangumi => Some("bangumi.summary"),
                    Provider::Dandanplay => Some("dandanplay.summary"),
                    _ => None,
                };
                if let Some(prefix) = prefix {
                    self.db
                        .set_setting(&format!("{prefix}.{id}"), Some(summary));
                }
            }
        }
        if candidates.subjects.is_empty() {
            return Ok(());
        }
        let (target, relation) = match &candidates.season {
            Some(season) => (
                self.db
                    .catalog_by_ref(season)?
                    .into_iter()
                    .find(|n| n.kind == Kind::Season && &n.root == work),
                Relation::Related,
            ),
            None => {
                let root = self.db.catalog_node(work)?;
                if root.as_ref().is_some_and(|n| n.kind == Kind::Movie) {
                    (root, Relation::Related)
                } else {
                    // A multi-cour anime TMDB keeps inside one season (such as
                    // S01 with 60 episodes): attach to its newest regular
                    // season as `PartOf` so public reviews load automatically
                    // without claiming `Exact` whole-season account coverage.
                    let season = self
                        .db
                        .catalog_children(work)?
                        .into_iter()
                        .filter(|n| {
                            n.kind == Kind::Season && n.season_number.is_some_and(|s| s > 0)
                        })
                        .max_by(|a, b| {
                            a.date
                                .cmp(&b.date)
                                .then(a.season_number.cmp(&b.season_number))
                        });
                    (season.or(root), Relation::PartOf)
                }
            }
        };
        let Some(target) = target else {
            return Ok(());
        };
        if candidates.season.is_some() && target.kind == Kind::Season {
            for sibling in self
                .db
                .catalog_children(work)
                .unwrap_or_default()
                .into_iter()
                .filter(|n| n.kind == Kind::Season && n.id != target.id)
            {
                for reference in &candidates.subjects {
                    let _ = self.db.catalog_drop_auto_link(&sibling.id, reference);
                }
            }
        }
        let existing = self.db.catalog_links(&target.id)?;
        let links: Vec<_> = candidates
            .subjects
            .iter()
            .filter_map(|reference| {
                let previous = existing.iter().find(|l| &l.reference == reference);
                if previous
                    .is_some_and(|l| l.manual || l.confirmed || l.evidence == CALENDAR_EVIDENCE)
                {
                    return None;
                }
                Some(ProviderLink {
                    media: target.id.clone(),
                    reference: reference.clone(),
                    relation,
                    coverage: Vec::new(),
                    evidence: CALENDAR_EVIDENCE.into(),
                    confirmed: false,
                    manual: false,
                    revision: previous.map_or(0, |l| l.revision),
                })
            })
            .collect();
        if links.is_empty() {
            return Ok(());
        }
        // Automatic evidence goes through the automatic path; the manual
        // request interface refuses links it did not decide.
        self.db.catalog_apply(&[], &links, &[], &[]).map(|_| ())
    }

    fn remember_source(
        &mut self,
        id: i64,
        media: MediaId,
        previous: Option<&SourceWork>,
        evidence: bool,
    ) -> erika_store::catalog::Result<()> {
        let source = source_ref(id)
            .ok_or_else(|| erika_store::catalog::Error::invalid("新番来源标识不正确"))?;
        // Only merge this completion, never the engine's entire old snapshot.
        let result = self.db.update_setting(SOURCE_CACHE, |text| {
            let mut sources = read_sources(text)?;
            let current = sources.iter().find(|s| s.source == source);
            if current != previous && !current.is_some_and(|s| s.media == media) {
                return Err(erika_store::catalog::Error::conflict(
                    "新番身份已更新，请重新打开详情",
                ));
            }
            sources.retain(|s| s.source != source);
            // First among equal timestamps, so a full cache keeps this result.
            sources.insert(
                0,
                SourceWork {
                    source,
                    media,
                    updated_at: erika_store::now(),
                    evidence,
                },
            );
            let sources = normalize_sources(sources);
            let text = serde_json::to_string(&sources)
                .map_err(|_| erika_store::catalog::Error::invalid("新番身份缓存无法保存"))?;
            Ok((Some(text.clone()), (text, sources)))
        });
        match result {
            Ok((text, sources)) => {
                self.metadata.source_cache = Some(text);
                self.metadata.sources = sources;
                Ok(())
            }
            Err(error) => {
                self.restore_metadata();
                Err(error)
            }
        }
    }
    pub(super) fn keep_search_results(
        &self,
        nodes: Vec<MediaNode>,
    ) -> Result<anime::SearchResults, CommandError> {
        // Search summaries cannot overwrite full details already cached. The
        // engine owns the database; a worker must not open a second database.
        let mut fresh = Vec::new();
        let mut projected = Vec::new();
        let mut seen = HashSet::new();
        for node in nodes {
            let reference = node
                .reference
                .as_ref()
                .ok_or_else(|| CommandError::new("metadata", "规范媒体缺少 TMDB 身份"))?;
            let existing = self
                .db
                .catalog_by_ref(reference)
                .map_err(content::command)?
                .into_iter()
                .next();
            let is_new = existing.is_none();
            let node = existing.unwrap_or(node);
            if !seen.insert(node.id.clone()) {
                continue;
            }
            if is_new {
                fresh.push(node.clone());
            }
            projected.push(node);
        }
        let saved = self
            .db
            .catalog_save_nodes(&fresh)
            .map_err(content::command)?;
        for node in &mut projected {
            if let Some(stored) = saved.iter().find(|n| n.reference == node.reference) {
                *node = stored.clone();
            }
        }
        let shows = projected
            .iter()
            .map(|node| content::show(&self.db, node))
            .collect::<erika_store::catalog::Result<Vec<_>>>()
            .map_err(content::command)?;
        Ok(content::results(&self.db, shows, &self.watching))
    }

    pub(super) fn search_media(
        &self,
        text: String,
        answer: bern::prelude::Answer<anime::SearchResults>,
    ) {
        let text = text.trim().to_string();
        let cancelled = answer.cancellation();
        let client = self.metadata.client();
        if text.is_empty() {
            let result = self
                .db
                .catalog_shelf(None)
                .and_then(|rows| {
                    rows.iter()
                        .map(|r| content::show(&self.db, &r.node))
                        .collect::<erika_store::catalog::Result<Vec<_>>>()
                })
                .map(|shows| content::results(&self.db, shows, &self.watching));
            answer.send(result.map_err(content::command));
            return;
        }
        let deliver = deliver_metadata(&self.inbox);
        erika_catalog::client::spawn(async move {
            if cancelled.is_cancelled() {
                return;
            }
            let result = async {
                let mut nodes = Vec::new();
                for kind in [Kind::Tv, Kind::Movie] {
                    nodes.extend(client.search(kind, &text, None, 1).await?.results);
                    if cancelled.is_cancelled() {
                        break;
                    }
                }
                Ok(nodes)
            }
            .await;
            if !cancelled.is_cancelled() {
                deliver(Finished::Search { answer, result });
            }
        });
    }
}

async fn resolve_source(
    client: &Client,
    id: i64,
    title: &str,
    known: Option<ProviderRef>,
) -> erika_catalog::Result<(tmdb::Work, Candidates)> {
    let source =
        source_ref(id).ok_or_else(|| erika_catalog::Error::invalid("新番来源标识不正确"))?;
    if let Some(reference) = known {
        // A known work still records the entry's own subjects; missing
        // evidence never costs the work its details.
        let evidence = Evidence::of(&source, title, false).await;
        let work = client.with_artwork(client.work(&reference).await?).await;
        let candidates = evidence.map(|e| e.candidates(&work)).unwrap_or_default();
        return Ok((work, candidates));
    }
    let mut evidence = Evidence::of(&source, title, true).await?;
    let hints = source_hints(evidence.titles.clone(), &evidence.dates);
    let reference = if evidence.refs.len() == 1 {
        evidence.refs.remove(0)
    } else if evidence.refs.is_empty() {
        // Missing dates do not become the current year or the calendar quarter.
        if hints.year.is_none() {
            return Err(erika_catalog::Error::invalid(&format!(
                "《{title}》缺少首播年份，请确认 TMDB 作品"
            )));
        }
        erika_catalog::resolve::search_work(client, &hints, evidence.kind)
            .await?
            .accepted
            .ok_or_else(|| {
                erika_catalog::Error::invalid(&format!("尚未确认《{title}》的 TMDB 对应作品"))
            })?
    } else {
        return Err(erika_catalog::Error::invalid(&format!(
            "《{title}》的 TMDB 对应作品有冲突"
        )));
    };
    let work = client.with_artwork(client.work(&reference).await?).await;
    let candidates = evidence.candidates(&work);
    Ok((work, candidates))
}

/// What a calendar entry's own provider says about it.
struct Evidence {
    titles: Vec<String>,
    dates: Vec<String>,
    refs: Vec<ProviderRef>,
    kind: Option<Kind>,
    subjects: Vec<ProviderRef>,
    summaries: Vec<(Provider, String, String)>,
}
impl Evidence {
    /// `for_search` also reads the linked Bangumi subject's names.
    async fn of(
        source: &ProviderRef,
        title: &str,
        for_search: bool,
    ) -> erika_catalog::Result<Self> {
        let mut evidence = Self {
            titles: vec![title.to_owned()],
            dates: Vec::new(),
            refs: Vec::new(),
            kind: None,
            subjects: vec![source.clone()],
            summaries: Vec::new(),
        };
        let mut subject = None;
        if source.provider == Provider::Bangumi {
            subject = Some(
                source
                    .id
                    .parse()
                    .map_err(|_| erika_catalog::Error::invalid("Bangumi 条目标识不正确"))?,
            );
        } else {
            let details = erika_dandanplay::catalog::details(&source.id, true)
                .await
                .map_err(|e| erika_catalog::Error::invalid(&e.message))?;
            if details.anime_id.to_string() != source.id {
                return Err(erika_catalog::Error::invalid("新番详情与请求的作品不一致"));
            }
            if !details.summary.trim().is_empty() {
                evidence.summaries.push((
                    Provider::Dandanplay,
                    source.id.clone(),
                    details.summary.clone(),
                ));
            }
            evidence.refs = details.tmdb_refs();
            evidence.kind = source_kind(&details.media_type);
            evidence.titles.push(details.anime_title.clone());
            evidence
                .titles
                .extend(details.titles.iter().map(|t| t.title.clone()));
            evidence
                .dates
                .extend(details.seasons.iter().filter_map(|s| s.air_date.clone()));
            evidence
                .dates
                .extend(details.episodes.iter().filter_map(|e| e.air_date.clone()));
            let linked = details.bangumi_ref();
            // The linked Bangumi subject carries the original (usually Japanese)
            // name, the one TMDB lists as the original title.
            if (for_search && evidence.refs.is_empty()) || evidence.dates.is_empty() {
                subject = linked.as_ref().and_then(|r| r.id.parse::<i64>().ok());
            }
            evidence.subjects.extend(linked);
        }
        if let Some(id) = subject {
            match erika_dandanplay::bangumi::subject(id).await {
                Ok(subject) => {
                    if !subject.summary.trim().is_empty() {
                        evidence.summaries.push((
                            Provider::Bangumi,
                            id.to_string(),
                            subject.summary,
                        ));
                    }
                    // Original names first: they are the exact catalogue titles.
                    // Bangumi lists the Chinese name, then the original one.
                    for name in subject.titles {
                        evidence.titles.insert(1, name);
                    }
                    evidence.dates.push(subject.date);
                }
                // Only a Bangumi calendar has no other evidence to fall back on.
                Err(e) if source.provider == Provider::Bangumi => {
                    return Err(erika_catalog::Error::invalid(&e.message));
                }
                Err(_) => {}
            }
        }
        Ok(evidence)
    }
    fn candidates(self, work: &tmdb::Work) -> Candidates {
        Candidates {
            season: airing_season(work, &self.dates),
            subjects: self.subjects,
            summaries: self.summaries,
            recorded: true,
        }
    }
}

/// The regular season premiering within 60 days of the entry's own first air
/// date (closest wins). A single-season TV entry whose season date TMDB has not
/// filled in yet inherits the work's own premiere date. Two cours in the same
/// year (~90+ days apart) never share candidates, while a December pre-screening
/// still matches a January broadcast.
fn airing_season(work: &tmdb::Work, dates: &[String]) -> Option<ProviderRef> {
    let premiere = dates
        .iter()
        .filter_map(|v| clock::parse_date(v))
        .map(clock::days_of)
        .min()?;
    let regular: Vec<_> = work
        .seasons
        .iter()
        .filter(|s| s.season_number.is_some_and(|n| n > 0))
        .collect();
    let single = regular.len() == 1;
    regular
        .into_iter()
        .filter_map(|s| {
            let date = clock::parse_date(&s.date)
                .or_else(|| single.then(|| clock::parse_date(&work.node.date)).flatten())?;
            let diff = (clock::days_of(date) - premiere).abs();
            (diff <= 60).then_some((s, diff))
        })
        .min_by_key(|(_, diff)| *diff)
        .and_then(|(s, _)| s.reference.clone())
}

/// The calendar's own media type narrows the search; unknown types try both.
fn source_kind(media_type: &str) -> Option<Kind> {
    match media_type {
        "movie" | "jpmovie" => Some(Kind::Movie),
        "tvseries" | "web" | "tvspecial" | "jpdrama" => Some(Kind::Tv),
        _ => None,
    }
}

fn source_hints(titles: Vec<String>, dates: &[String]) -> erika_catalog::names::NameHints {
    let mut hints = erika_catalog::names::NameHints::default();
    for title in titles {
        let title = title.trim();
        if !title.is_empty() && !hints.titles.iter().any(|t| t == title) {
            hints.titles.push(title.into());
        }
    }
    hints.year = dates
        .iter()
        .filter_map(|value| clock::parse_date(value))
        .filter_map(|date| u16::try_from(date.year).ok())
        .min();
    hints
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    mod concurrency;

    #[test]
    fn source_dates_are_evidence_not_the_current_calendar_year() {
        let hints = source_hints(
            vec![" Show ".into(), "Show".into(), "原名".into()],
            &[
                "2024-02-29".into(),
                "2025-01-01T00:00:00Z".into(),
                "2023-02-29".into(),
                "0000-01-01".into(),
            ],
        );
        assert_eq!(hints.titles, ["Show", "原名"]);
        assert_eq!(hints.year, Some(2024));
        assert_eq!(source_hints(vec!["Show".into()], &[]).year, None);
        assert_eq!(
            source_hints(
                vec![],
                &["2024-13-01".into(), "2024-01-00".into(), "2024-2-01".into()]
            )
            .year,
            None
        );
        assert!(source_ref(-4).is_none());
        assert!(source_ref(0).is_none());
        assert_eq!(source_ref(42).unwrap().provider, Provider::Dandanplay);
        let bgm = source_ref(erika_dandanplay::bangumi::local_id(42).unwrap()).unwrap();
        assert_eq!((bgm.provider, bgm.id), (Provider::Bangumi, "42".into()));
    }

    #[test]
    fn source_cache_is_bounded_and_uses_the_newest_valid_identity() {
        let old = MediaId::new();
        let latest = MediaId::new();
        let mut rows = (1..=MAX_SOURCES + 10)
            .map(|id| SourceWork {
                source: source_ref(id as i64).unwrap(),
                media: old.clone(),
                updated_at: id as i64,
                evidence: false,
            })
            .collect::<Vec<_>>();
        rows.push(SourceWork {
            source: source_ref(42).unwrap(),
            media: latest.clone(),
            updated_at: erika_store::now(),
            evidence: false,
        });
        rows.push(SourceWork {
            source: source_ref(42).unwrap(),
            media: old.clone(),
            updated_at: i64::MAX,
            evidence: false,
        });
        rows.push(SourceWork {
            source: ProviderRef::tmdb(Kind::Tv, 42).unwrap(),
            media: latest.clone(),
            updated_at: 0,
            evidence: false,
        });
        let rows = normalize_sources(rows);
        assert_eq!(rows.len(), MAX_SOURCES);
        assert_eq!(
            rows.iter()
                .find(|s| s.source == source_ref(42).unwrap())
                .unwrap()
                .media,
            latest
        );
        assert_eq!(
            rows.iter()
                .filter(|s| s.source == source_ref(42).unwrap())
                .count(),
            1
        );
        assert!(
            rows.iter()
                .all(|s| s.source.provider == Provider::Dandanplay)
        );
    }

    #[test]
    fn one_generation_attempts_each_source_once_and_refresh_can_retry() {
        let mut engine = crate::tests::engine();
        engine.metadata.configuring = true;
        // Occupy the worker slots; this tests queue transitions without HTTP.
        engine.metadata.running.extend((100..104).map(|id| (0, id)));
        engine.season =
            vec![serde_json::from_value(json!({"animeId":42,"animeTitle":"Show"})).unwrap()];
        engine.load_metadata();
        engine.load_metadata();
        assert_eq!(engine.metadata.waiting.len(), 1);
        engine.metadata.waiting.clear();
        engine.metadata.attempted.insert((0, 42));
        engine.load_metadata();
        assert!(engine.metadata.waiting.is_empty());
        engine.generation = 1;
        engine.load_metadata();
        assert_eq!(engine.metadata.waiting.len(), 1);
        assert_eq!(engine.metadata.waiting[0].0, 1);
        engine.take_metadata(Finished::Configuration(Err(erika_catalog::Error::invalid(
            "offline",
        ))));
        assert!(!engine.metadata.configuring);
        assert!(!engine.db.catalog_images_fresh(erika_store::now()));
    }

    #[test]
    fn a_busy_service_retries_and_an_unconfirmed_show_is_not_a_page_error() {
        let mut engine = crate::tests::engine();
        engine.metadata.configuring = true;
        engine.metadata.running.extend((100..104).map(|id| (0, id)));
        engine.season = vec![
            serde_json::from_value(json!({"animeId":42,"animeTitle":"Show"})).unwrap(),
            serde_json::from_value(json!({"animeId":43,"animeTitle":"Other"})).unwrap(),
        ];
        engine.metadata.attempted.extend([(0, 42), (0, 43)]);
        engine.metadata.running.extend([(0, 42), (0, 43)]);
        let busy = erika_catalog::Error {
            code: erika_catalog::client::ErrorCode::RateLimited,
            message: "媒体资料请求过于频繁，稍后重试".into(),
            retry_after: Some(60),
        };
        for (id, result) in [
            (42, Err(busy)),
            (
                43,
                Err(erika_catalog::Error::invalid(
                    "尚未确认《Other》的 TMDB 对应作品",
                )),
            ),
        ] {
            engine.take_metadata(Finished::Source {
                generation: 0,
                id,
                previous: None,
                result,
                candidates: Default::default(),
            });
        }
        assert!(engine.state.error.is_empty(), "{}", engine.state.error);
        assert!(engine.metadata.retry_armed);
        assert!(!engine.metadata.attempted.contains(&(0, 42)));
        assert!(engine.metadata.attempted.contains(&(0, 43)));
        engine.resume_metadata_retry();
        assert!(!engine.metadata.retry_armed);
        assert_eq!(
            engine
                .metadata
                .waiting
                .iter()
                .map(|w| w.1)
                .collect::<Vec<_>>(),
            [42]
        );
    }

    #[test]
    fn unresolved_calendar_details_create_only_real_tmdb_nodes_and_can_reopen() {
        use erika_anime_protocol::anime::Commands;
        let mut engine = crate::tests::engine();
        engine.season =
            vec![serde_json::from_value(json!({"animeId":42,"animeTitle":"来源标题"})).unwrap()];
        engine.show_season();
        let key = source_key(42);
        engine.open_scheduled(key.clone()).unwrap();
        assert!(engine.db.catalog_export().unwrap().nodes.is_empty());
        assert_eq!(engine.state.show.title, "来源标题");
        assert!(!engine.state.can_rate && !engine.state.can_review);
        let selection = engine.metadata.selection;
        engine.take_metadata(Finished::Detail {
            previous: None,
            selection,
            opened: key.clone(),
            result: Err(erika_catalog::Error::unavailable()),
            candidates: Default::default(),
        });
        assert!(engine.state.details_failed);
        assert_eq!(engine.state.opened, key);
        assert!(engine.db.catalog_export().unwrap().nodes.is_empty());
        let work = tmdb::parse_work(Kind::Tv, 700, &json!({"id":700,"name":"TMDB 标题","overview":"TMDB 简介","poster_path":"/poster.jpg","seasons":[{"id":701,"season_number":0,"name":"特别篇"}]})).unwrap();
        let season = tmdb::parse_season(&work, 0, &json!({"id":701,"season_number":0,"name":"特别篇","episodes":[{"id":702,"season_number":0,"episode_number":1,"name":"特典"}]})).unwrap();
        engine.take_metadata(Finished::Detail {
            previous: None,
            selection,
            opened: key.clone(),
            result: Ok((work.clone(), Some(season.clone()))),
            candidates: Default::default(),
        });
        assert!(!engine.state.details_failed);
        assert_eq!(engine.state.opened, work.node.id.0);
        assert_eq!(engine.state.selected_season, season.node.id.0);
        assert_eq!(engine.state.show.title, "TMDB 标题");
        assert_eq!(engine.state.episodes[0].id, season.episodes[0].id.0);
        let snapshot = engine.db.catalog_export().unwrap();
        assert_eq!(snapshot.nodes.len(), 3);
        assert!(snapshot.nodes.iter().all(|n| {
            n.reference
                .as_ref()
                .is_some_and(|r| r.provider == Provider::Tmdb)
        }));
        assert!(
            snapshot.links.is_empty()
                && snapshot.bindings.is_empty()
                && snapshot.watched.is_empty()
        );
        assert!(!engine.state.can_rate && !engine.state.can_review);
        engine.close().unwrap();
        engine.open_scheduled(key.clone()).unwrap();
        assert_eq!(engine.state.opened, work.node.id.0);
        // Simulate rebuilding the engine's source cache from this database.
        engine.close().unwrap();
        engine.metadata = Runtime {
            network_disabled: true,
            ..Default::default()
        };
        engine.show_season();
        engine.open_scheduled(key).unwrap();
        assert_eq!(engine.state.opened, work.node.id.0);
    }

    #[test]
    fn closing_and_reopening_the_same_source_fences_its_previous_request() {
        use erika_anime_protocol::anime::Commands;
        let mut engine = crate::tests::engine();
        engine.season =
            vec![serde_json::from_value(json!({"animeId":42,"animeTitle":"来源标题"})).unwrap()];
        engine.show_season();
        let key = source_key(42);
        engine.open_scheduled(key.clone()).unwrap();
        let first = engine.metadata.selection;
        engine.close().unwrap();
        engine.open_scheduled(key.clone()).unwrap();
        assert_ne!(first, engine.metadata.selection);
        let work =
            tmdb::parse_work(Kind::Movie, 700, &json!({"id":700,"title":"迟到结果"})).unwrap();
        engine.take_metadata(Finished::Detail {
            previous: None,
            selection: first,
            opened: key.clone(),
            result: Ok((work, None)),
            candidates: Default::default(),
        });
        assert_eq!(engine.state.opened, key);
        assert_eq!(engine.state.show.title, "来源标题");
        assert!(engine.db.catalog_export().unwrap().nodes.is_empty());
        assert!(engine.db.setting(SOURCE_CACHE).is_none());
    }

    #[test]
    fn search_calendar_and_details_keep_the_current_local_uuid() {
        let mut engine = crate::tests::engine();
        let work = tmdb::parse_work(Kind::Tv, 700, &json!({"id":700,"name":"TMDB 标题","seasons":[{"id":701,"season_number":0,"name":"特别篇"}]})).unwrap();
        let season = tmdb::parse_season(&work, 0, &json!({"id":701,"season_number":0,"name":"特别篇","episodes":[{"id":702,"season_number":0,"episode_number":1,"name":"特典"}]})).unwrap();
        let mut local = nodes(&work, Some(&season));
        let ids: HashMap<_, _> = local
            .iter()
            .map(|n| (n.id.clone(), MediaId::new()))
            .collect();
        for n in &mut local {
            n.id = ids[&n.id].clone();
            n.root = ids[&n.root].clone();
            n.parent = n.parent.as_ref().map(|p| ids[p].clone());
        }
        engine.db.catalog_save_nodes(&local).unwrap();
        engine.keep_search_results(vec![work.node.clone()]).unwrap();
        assert!(engine.db.catalog_node(&work.node.id).unwrap().is_none());
        assert_eq!(
            engine
                .db
                .catalog_node(&local[0].id)
                .unwrap()
                .unwrap()
                .revision,
            1
        );
        engine.season =
            vec![serde_json::from_value(json!({"animeId":42,"animeTitle":"来源标题"})).unwrap()];
        engine.take_metadata(Finished::Source {
            previous: None,
            generation: engine.generation,
            id: 42,
            result: Ok(work.clone()),
            candidates: Default::default(),
        });
        assert_eq!(
            engine.resolved_source(&source_key(42)),
            Some(local[0].id.clone())
        );
        engine.state.opened = local[0].id.0.clone();
        engine.take_metadata(Finished::Detail {
            previous: None,
            selection: engine.metadata.selection,
            opened: engine.state.opened.clone(),
            result: Ok((work.clone(), Some(season))),
            candidates: Default::default(),
        });
        assert!(
            !engine.state.details_failed,
            "{}",
            engine.state.details_error
        );
        assert_eq!(engine.state.opened, local[0].id.0);
        assert_eq!(engine.state.selected_season, local[1].id.0);
        assert_eq!(engine.state.episodes[0].id, local[2].id.0);
        assert_eq!(engine.db.catalog_export().unwrap().nodes.len(), 3);
    }

    #[test]
    fn late_details_cannot_write_nodes_and_success_clears_previous_failure() {
        let mut engine = crate::tests::engine();
        let old = tmdb::parse_work(Kind::Movie, 1, &json!({"id":1,"title":"Old"})).unwrap();
        let current = tmdb::parse_work(Kind::Movie, 2, &json!({"id":2,"title":"Current"})).unwrap();
        engine
            .db
            .catalog_save_nodes(&[current.node.clone()])
            .unwrap();
        engine.state.opened = current.node.id.0.clone();
        engine.metadata.selection = 2;
        engine.take_metadata(Finished::Detail {
            previous: None,
            selection: 1,
            opened: engine.state.opened.clone(),
            result: Ok((old.clone(), None)),
            candidates: Default::default(),
        });
        assert!(engine.db.catalog_node(&old.node.id).unwrap().is_none());
        engine.take_metadata(Finished::Detail {
            previous: None,
            selection: 2,
            opened: old.node.id.0.clone(),
            result: Ok((old.clone(), None)),
            candidates: Default::default(),
        });
        assert!(engine.db.catalog_node(&old.node.id).unwrap().is_none());
        engine.take_metadata(Finished::Detail {
            previous: None,
            selection: 2,
            opened: engine.state.opened.clone(),
            result: Err(erika_catalog::Error::invalid("offline")),
            candidates: Default::default(),
        });
        assert!(engine.state.details_failed);
        engine.take_metadata(Finished::Detail {
            previous: None,
            selection: 2,
            opened: engine.state.opened.clone(),
            result: Ok((current.clone(), None)),
            candidates: Default::default(),
        });
        assert!(!engine.state.details_failed);
        assert!(engine.state.details_error.is_empty());
        assert_eq!(engine.state.opened, current.node.id.0);
        let stale = tmdb::parse_work(Kind::Movie, 3, &json!({"id":3,"title":"Stale"})).unwrap();
        engine.take_metadata(Finished::Source {
            previous: None,
            generation: engine.generation + 1,
            id: 9,
            result: Ok(stale.clone()),
            candidates: Default::default(),
        });
        assert!(engine.db.catalog_node(&stale.node.id).unwrap().is_none());
    }
}

#[cfg(test)]
mod candidate_tests {
    use super::*;
    use anime::{Commands, MappingProvider};
    use serde_json::json;

    fn work() -> tmdb::Work {
        tmdb::parse_work(
            Kind::Tv,
            700,
            &json!({
                "id":700,"name":"青之芦苇","first_air_date":"2022-04-09",
                "seasons":[
                    {"id":701,"season_number":0,"name":"特别篇","air_date":"2026-10-01"},
                    {"id":702,"season_number":1,"name":"第 1 季","air_date":"2022-04-09"},
                    {"id":703,"season_number":2,"name":"第 2 季","air_date":"2026-10-04"}
                ]
            }),
        )
        .unwrap()
    }
    fn subject(provider: Provider, id: &str) -> ProviderRef {
        ProviderRef {
            provider,
            scope: String::new(),
            kind: Kind::Subject,
            id: id.into(),
        }
    }
    fn resolved() -> (AnimeEngine, MediaId, MediaId) {
        let mut engine = crate::tests::engine();
        engine.season = vec![
            serde_json::from_value(json!({"animeId":19318,"animeTitle":"青之芦苇 第二季"}))
                .unwrap(),
        ];
        let work = work();
        let season = work.seasons[2].reference.clone();
        assert_eq!(
            airing_season(&work, &["2026-10-04T00:00:00".into(), "2026-10-11".into()]),
            season
        );
        engine.metadata.running.insert((0, 19318));
        engine.take_metadata(Finished::Source {
            generation: 0,
            id: 19318,
            previous: None,
            result: Ok(work.clone()),
            candidates: Candidates {
                season,
                subjects: vec![
                    subject(Provider::Dandanplay, "19318"),
                    subject(Provider::Bangumi, "555605"),
                ],
                summaries: Vec::new(),
                recorded: true,
            },
        });
        assert!(engine.state.error.is_empty(), "{}", engine.state.error);
        let root = engine.resolved_source(&source_key(19318)).unwrap();
        let second = engine
            .db
            .catalog_by_ref(work.seasons[2].reference.as_ref().unwrap())
            .unwrap()[0]
            .id
            .clone();
        (engine, root, second)
    }

    #[test]
    fn calendar_subjects_show_reviews_for_their_season_without_account_permission() {
        let (mut e, root, second) = resolved();
        let links = e.db.catalog_links(&second).unwrap();
        assert_eq!(links.len(), 2);
        assert!(
            links
                .iter()
                .all(|l| !l.confirmed && !l.manual && l.relation == Relation::Related)
        );
        assert!(e.db.catalog_links(&root).unwrap().is_empty());
        e.open_scheduled(root.0.clone()).unwrap();
        // Opens on the season this calendar entry airs as.
        assert_eq!(e.state.selected_season, second.0);
        assert_eq!(e.review_subject(), Some(555605));
        // Reading is not writing: no account target, no rating permission.
        assert_eq!(
            crate::sync::subject(&e.db, &second, Provider::Bangumi),
            Ok(None)
        );
        assert!(!e.state.can_rate);
        let first = e
            .state
            .seasons
            .iter()
            .find(|s| s.number == 1)
            .unwrap()
            .id
            .clone();
        e.select_season(first).unwrap();
        assert_eq!(e.review_subject(), None);
    }

    #[test]
    fn candidates_can_be_confirmed_or_dismissed_and_stay_decided() {
        let (mut e, root, second) = resolved();
        e.open_scheduled(root.0.clone()).unwrap();
        e.show_mappings(true).unwrap();
        assert_eq!(e.state.mapping_target, second.0);
        assert_eq!(e.state.mapping_rows.len(), 2);
        assert!(e.state.mapping_rows.iter().all(|r| !r.confirmed));
        let bangumi = e
            .state
            .mapping_rows
            .iter()
            .find(|r| r.provider == MappingProvider::Bangumi)
            .unwrap()
            .id
            .clone();
        e.confirm_provider_mapping(bangumi).unwrap();
        let link = crate::sync::subject(&e.db, &second, Provider::Bangumi)
            .unwrap()
            .unwrap();
        assert_eq!(
            (link.reference.id.as_str(), link.relation, link.manual),
            ("555605", Relation::Exact, true)
        );
        let dandanplay = e
            .state
            .mapping_rows
            .iter()
            .find(|r| r.provider == MappingProvider::Dandanplay && !r.confirmed)
            .unwrap()
            .id
            .clone();
        e.remove_provider_mapping(dandanplay).unwrap();
        assert_eq!(e.state.mapping_rows.len(), 1);
        // A later calendar refresh does not bring dismissed or confirmed back.
        let candidates = Candidates {
            season: work().seasons[2].reference.clone(),
            subjects: vec![
                subject(Provider::Dandanplay, "19318"),
                subject(Provider::Bangumi, "555605"),
            ],
            summaries: Vec::new(),
            recorded: true,
        };
        e.keep_candidates(&root, &candidates).unwrap();
        let links = e.db.catalog_links(&second).unwrap();
        assert!(links.iter().all(|l| l.manual));
        assert_eq!(e.review_subject(), Some(555605));
        // Another cour in the same year is not close enough to steal a sequel
        // whose TMDB season date is still missing.
        let split_cour = tmdb::parse_work(
            Kind::Tv,
            710,
            &json!({
                "id":710,"name":"分割放送","first_air_date":"2026-04-05",
                "seasons":[
                    {"id":711,"season_number":1,"name":"第 1 季","air_date":"2026-04-05"},
                    {"id":712,"season_number":2,"name":"第 2 季"}
                ]
            }),
        )
        .unwrap();
        assert_eq!(airing_season(&split_cour, &["2026-10-04".into()]), None);
        assert_eq!(
            airing_season(&work(), &["2026-09-27".into()]),
            work().seasons[2].reference
        );
    }

    #[test]
    fn a_dismissed_bangumi_candidate_is_not_read() {
        let (mut e, root, _) = resolved();
        e.open_scheduled(root.0.clone()).unwrap();
        e.show_mappings(true).unwrap();
        let bangumi = e
            .state
            .mapping_rows
            .iter()
            .find(|r| r.provider == MappingProvider::Bangumi)
            .unwrap()
            .id
            .clone();
        e.remove_provider_mapping(bangumi).unwrap();
        assert_eq!(e.review_subject(), None);
    }

    #[test]
    fn summary_prefers_tmdb_chinese_then_bangumi_then_dandanplay_then_original() {
        let mut e = crate::tests::engine();
        e.season = vec![
            serde_json::from_value(json!({
                "animeId": 19318,
                "animeTitle": "青之芦苇 第二季",
                "summary": "新番表中文简介"
            }))
            .unwrap(),
        ];
        let mut tmdb_work = work();
        tmdb_work.original_overview = "日本語のあらすじ".into();
        let season = tmdb_work.seasons[2].reference.clone();
        e.metadata.running.insert((0, 19318));
        e.take_metadata(Finished::Source {
            generation: 0,
            id: 19318,
            previous: None,
            result: Ok(tmdb_work.clone()),
            candidates: Candidates {
                season,
                subjects: vec![
                    subject(Provider::Dandanplay, "19318"),
                    subject(Provider::Bangumi, "555605"),
                ],
                summaries: Vec::new(),
                recorded: true,
            },
        });
        let root = e.resolved_source(&source_key(19318)).unwrap();
        // Stored MediaNode stays pure TMDB (empty overview), never polluted by fallbacks.
        assert!(
            e.db.catalog_node(&root)
                .unwrap()
                .unwrap()
                .overview
                .is_empty()
        );

        // Tier 3 (calendar dandanplay summary) beats Tier 4 (Japanese original_overview) on S2.
        e.open_scheduled(root.0.clone()).unwrap();
        assert_eq!(e.state.show.summary, "新番表中文简介");

        // Switching to S1 does not leak S2's calendar summary; S1 falls back to Tier 4.
        let first = e
            .state
            .seasons
            .iter()
            .find(|s| s.number == 1)
            .unwrap()
            .id
            .clone();
        let second = e
            .state
            .seasons
            .iter()
            .find(|s| s.number == 2)
            .unwrap()
            .id
            .clone();
        e.select_season(first.clone()).unwrap();
        assert_eq!(e.state.show.summary, "日本語のあらすじ");

        // Back on S2: an untranslated Japanese blurb on Bangumi yields to
        // dandanplay's Chinese summary; a Chinese Bangumi summary wins.
        e.db.set_setting("dandanplay.summary.19318", Some("弹弹详情中文简介"));
        e.db.set_setting(
            "bangumi.summary.555605",
            Some("まちのパンやからとびだすひとつのかげ"),
        );
        e.select_season(second).unwrap();
        assert_eq!(e.state.show.summary, "弹弹详情中文简介");
        e.db.set_setting("bangumi.summary.555605", Some("Bangumi 第二季中文简介"));
        e.refresh_opened_summary();
        assert_eq!(e.state.show.summary, "Bangumi 第二季中文简介");

        // Tier 1: a root series overview belongs to S1 / the series as a whole
        // and must NOT overwrite S2's own synopsis; TMDB's S2 overview wins on S2.
        tmdb_work.node.overview = "TMDB 第一季总简介".into();
        e.save_metadata(&tmdb_work, None).unwrap();
        e.show_opened();
        assert_eq!(e.state.show.summary, "Bangumi 第二季中文简介");
        assert!(
            e.db.setting(&content::original_overview_key(&tmdb_work.node).unwrap())
                .is_none()
        );
        tmdb_work.seasons[2].overview = "TMDB 第二季官方中文简介".into();
        e.save_metadata(&tmdb_work, None).unwrap();
        e.show_opened();
        assert_eq!(e.state.show.summary, "TMDB 第二季官方中文简介");
        e.select_season(first).unwrap();
        assert_eq!(e.state.show.summary, "TMDB 第一季总简介");
    }
}
