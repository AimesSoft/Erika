//! Anonymous source-list enrichment. This cache is presentation evidence only:
//! it never aliases files or grants permission to write a provider account.
use super::*;
use erika_catalog::{
    Client, Kind, MediaId, MediaNode, Provider, ProviderRef, artwork::ImageConfiguration, tmdb,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

const SOURCE_CACHE: &str = "anime.tmdb.sources.v1";
const SOURCE_TTL: i64 = 24 * 60 * 60;
const MAX_SOURCES: usize = 4096;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SourceWork {
    source: ProviderRef,
    media: MediaId,
    #[serde(default)]
    updated_at: i64,
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
}
pub(super) enum Finished {
    Configuration(erika_catalog::Result<ImageConfiguration>),
    Source {
        generation: u64,
        id: i64,
        previous: Option<SourceWork>,
        result: erika_catalog::Result<tmdb::Work>,
    },
    Detail {
        selection: u64,
        opened: String,
        previous: Option<SourceWork>,
        result: erika_catalog::Result<(tmdb::Work, Option<tmdb::Season>)>,
    },
    Search {
        answer: bern::prelude::Answer<anime::SearchResults>,
        result: erika_catalog::Result<Vec<MediaNode>>,
    },
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
    if let Some(season) = season {
        nodes.retain(|n| n.id != season.node.id);
        nodes.push(season.node.clone());
        nodes.extend(season.episodes.clone());
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
        &self,
        work: &tmdb::Work,
        season: Option<&tmdb::Season>,
    ) -> erika_store::catalog::Result<(MediaId, Option<MediaId>)> {
        let stored = self.db.catalog_save_nodes(&nodes(work, season))?;
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

    pub(super) fn source_preview(&self, key: &str) -> Option<Show> {
        let intro = self.source_intro(key)?;
        let mut show = shows::untitled();
        show.id = key.into();
        show.title = intro.anime_title.clone();
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
        self.restore_metadata();
        let images = self.db.catalog_images();
        let map = self
            .season
            .iter()
            .filter_map(|i| {
                let key = source_key(i.anime_id);
                let target = self.resolved_source(&key)?;
                self.db
                    .catalog_node(&target)
                    .ok()
                    .flatten()
                    .map(|n| (key, n))
            })
            .collect::<HashMap<_, _>>();
        let project = |row: &mut anime::Broadcast| {
            if let Some(node) = map.get(&row.id) {
                row.id = node.id.0.clone();
                row.title = node.title.clone();
                row.poster = MediaSource::new(images.url(node.artwork.poster.as_ref(), 780));
            }
        };
        for day in &mut calendar.week {
            for row in &mut day.shows {
                project(row);
            }
        }
        calendar.relayout();
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
        for intro in &self.season {
            if intro.is_restricted {
                continue;
            }
            let Some(source) = source_ref(intro.anime_id) else {
                continue;
            };
            let fresh = self.metadata.sources.iter().any(|s| {
                s.source == source
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
                let result = resolve_source(&client, id, &title, known).await;
                deliver(Finished::Source {
                    generation,
                    id,
                    previous,
                    result,
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
        let source = self
            .source_intro(&opened)
            .map(|i| (i.anime_id, i.anime_title.clone()));
        let previous = source
            .as_ref()
            .and_then(|(id, _)| self.source_snapshot(*id));
        let reference = if source.is_none() {
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
        let selection = self.metadata.selection;
        self.state.details_loading = true;
        let client = self.metadata.client();
        let deliver = deliver_metadata(&self.inbox);
        erika_catalog::client::spawn(async move {
            let result = async {
                let work = match (reference, source) {
                    (Some(reference), _) => {
                        client.with_artwork(client.work(&reference).await?).await
                    }
                    (_, Some((id, title))) => resolve_source(&client, id, &title, None).await?,
                    _ => return Err(erika_catalog::Error::invalid("作品不存在")),
                };
                let number = selected
                    .or_else(|| {
                        work.seasons
                            .iter()
                            .filter_map(|s| s.season_number)
                            .filter(|n| *n > 0)
                            .min()
                    })
                    .or_else(|| work.seasons.first().and_then(|s| s.season_number));
                let season = match number {
                    Some(n) if work.node.kind == Kind::Tv => Some(client.season(&work, n).await?),
                    _ => None,
                };
                Ok((work, season))
            }
            .await;
            deliver(Finished::Detail {
                selection,
                opened,
                previous,
                result,
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
            Finished::Configuration(Ok(config)) => {
                self.metadata.configuring = false;
                if let Err(e) = self.db.catalog_save_images(&config) {
                    self.state.error = e.message;
                }
                self.show_season();
                self.show_opened();
                self.show_watching();
            }
            Finished::Configuration(Err(_)) => {
                self.metadata.configuring = false;
            }
            Finished::Source {
                generation,
                id,
                previous,
                result,
            } => {
                self.metadata.running.remove(&(generation, id));
                if generation == self.generation {
                    match result {
                        Ok(work) => match self.save_metadata(&work, None) {
                            Ok((media, _)) => {
                                if let Err(error) =
                                    self.remember_source(id, media, previous.as_ref())
                                {
                                    self.state.error = error.message;
                                }
                                self.show_season();
                            }
                            Err(e) => self.state.error = e.message,
                        },
                        Err(e) => self.state.error = e.message,
                    }
                }
                self.pump_metadata();
            }
            Finished::Detail {
                selection,
                opened,
                previous,
                result,
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
                            if let Some(id) = self.source_intro(&opened).map(|i| i.anime_id) {
                                self.remember_source(id, saved.0.clone(), previous.as_ref())?;
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
    fn remember_source(
        &mut self,
        id: i64,
        media: MediaId,
        previous: Option<&SourceWork>,
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
) -> erika_catalog::Result<tmdb::Work> {
    let source =
        source_ref(id).ok_or_else(|| erika_catalog::Error::invalid("新番来源标识不正确"))?;
    if let Some(reference) = known {
        return Ok(client.with_artwork(client.work(&reference).await?).await);
    }
    let mut titles = vec![title.to_owned()];
    let mut dates = Vec::new();
    let mut refs = Vec::new();
    if source.provider == Provider::Bangumi {
        let subject = erika_dandanplay::bangumi::subject(
            source
                .id
                .parse()
                .map_err(|_| erika_catalog::Error::invalid("Bangumi 条目标识不正确"))?,
        )
        .await
        .map_err(|e| erika_catalog::Error::invalid(&e.message))?;
        titles.extend(subject.titles);
        dates.push(subject.date);
    } else {
        let details = erika_dandanplay::catalog::details(&source.id, true)
            .await
            .map_err(|e| erika_catalog::Error::invalid(&e.message))?;
        if details.anime_id.to_string() != source.id {
            return Err(erika_catalog::Error::invalid("新番详情与请求的作品不一致"));
        }
        refs = details.tmdb_refs();
        titles.push(details.anime_title);
        dates.extend(details.seasons.iter().filter_map(|s| s.air_date.clone()));
        dates.extend(details.episodes.iter().filter_map(|e| e.air_date.clone()));
    }
    let reference = if refs.len() == 1 {
        refs.remove(0)
    } else if refs.is_empty() {
        let hints = source_hints(titles, &dates);
        // Missing dates do not become the current year or the calendar quarter.
        if hints.year.is_none() {
            return Err(erika_catalog::Error::invalid(&format!(
                "《{title}》缺少首播年份，请确认 TMDB 作品"
            )));
        }
        erika_catalog::resolve::search_work(client, &hints, None)
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
    Ok(client.with_artwork(client.work(&reference).await?).await)
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
            })
            .collect::<Vec<_>>();
        rows.push(SourceWork {
            source: source_ref(42).unwrap(),
            media: latest.clone(),
            updated_at: erika_store::now(),
        });
        rows.push(SourceWork {
            source: source_ref(42).unwrap(),
            media: old.clone(),
            updated_at: i64::MAX,
        });
        rows.push(SourceWork {
            source: ProviderRef::tmdb(Kind::Tv, 42).unwrap(),
            media: latest.clone(),
            updated_at: 0,
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
        });
        assert!(engine.db.catalog_node(&old.node.id).unwrap().is_none());
        engine.take_metadata(Finished::Detail {
            previous: None,
            selection: 2,
            opened: old.node.id.0.clone(),
            result: Ok((old.clone(), None)),
        });
        assert!(engine.db.catalog_node(&old.node.id).unwrap().is_none());
        engine.take_metadata(Finished::Detail {
            previous: None,
            selection: 2,
            opened: engine.state.opened.clone(),
            result: Err(erika_catalog::Error::invalid("offline")),
        });
        assert!(engine.state.details_failed);
        engine.take_metadata(Finished::Detail {
            previous: None,
            selection: 2,
            opened: engine.state.opened.clone(),
            result: Ok((current.clone(), None)),
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
        });
        assert!(engine.db.catalog_node(&stale.node.id).unwrap().is_none());
    }
}
