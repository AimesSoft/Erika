//! The numeric NipaPlay interface refers to DDP episode labels, never TMDB
//! episode numbers. Resolve a real provider episode before committing locally.
use super::*;
use erika_catalog::{Kind, MediaId, Provider, ProviderLink, Relation, Watched};
use erika_dandanplay::catalog::Details;
use erika_store::catalog::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
struct View {
    opened: String,
    season: String,
    generation: String,
}
impl View {
    fn current() -> Option<Self> {
        erika_anime_protocol::anime::current_state().map(|s| Self {
            opened: s.opened,
            season: s.selected_season,
            generation: s.view_generation,
        })
    }
    fn media(&self) -> Result<MediaId> {
        Ok(MediaId::parse(if self.season.is_empty() {
            &self.opened
        } else {
            &self.season
        })?)
    }
}

#[derive(Clone)]
struct Target {
    link: ProviderLink,
    revision: u64,
}
struct Request {
    owner: String,
    instance: u64,
    anime: i64,
    number: i64,
    watched: bool,
    view: View,
    subject: ProviderLink,
    targets: Vec<Target>,
}
#[derive(Default)]
pub(super) struct Pending(Vec<(u64, Request)>);
impl Pending {
    pub(super) fn stop(&mut self, owner: &str) {
        self.0.retain(|(_, r)| r.owner != owner);
    }
    fn take(&mut self, tag: u64) -> Option<Request> {
        let index = self.0.iter().position(|(id, _)| *id == tag)?;
        Some(self.0.swap_remove(index).1)
    }
}

fn exact(link: &ProviderLink, kind: Kind) -> bool {
    link.confirmed
        && link.relation == Relation::Exact
        && link.reference.provider == Provider::Dandanplay
        && link.reference.scope.is_empty()
        && link.reference.kind == kind
}
fn single_link(db: &erika_store::Db, media: &MediaId, kind: Kind) -> Result<ProviderLink> {
    let links: Vec<_> = db
        .catalog_links(media)?
        .into_iter()
        .filter(|l| exact(l, kind))
        .collect();
    let [link] = links.as_slice() else {
        return Err(Error::conflict("请先确认唯一的弹弹play关联"));
    };
    let assigned = db.catalog_by_ref(&link.reference)?;
    if assigned.len() != 1 || assigned[0].id != *media {
        return Err(Error::conflict("弹弹play关联对应多个媒体"));
    }
    Ok(link.clone())
}

/// Numeric legacy calls cannot express SP/C1/12.5/S00E01. Do not truncate,
/// decode an ID arithmetically, or take the first row when labels repeat.
fn episode_id(details: &Details, anime: i64, number: i64) -> Result<String> {
    if anime <= 0 || number < 0 || details.anime_id != anime as u64 {
        return Err(Error::invalid("弹弹play返回的作品不一致"));
    }
    let matches: Vec<_> = details
        .episodes
        .iter()
        .filter(|e| {
            let label = e.episode_number.trim();
            !label.is_empty()
                && label.bytes().all(|b| b.is_ascii_digit())
                && label.parse::<i64>() == Ok(number)
        })
        .collect();
    let [episode] = matches.as_slice() else {
        return Err(Error::conflict("弹弹play集号不唯一或不存在"));
    };
    if episode.episode_id == 0
        || details
            .episodes
            .iter()
            .filter(|e| e.episode_id == episode.episode_id)
            .count()
            != 1
    {
        return Err(Error::invalid("弹弹play分集标识不正确"));
    }
    Ok(episode.episode_id.to_string())
}

impl ErikaEngine {
    fn watch_allowed(&self, owner: &str, instance: u64) -> bool {
        self.state.features.plugins
            && self.plugins.enabled(owner)
            && self
                .plugins
                .find(owner)
                .is_some_and(|p| p.pack.has("library.write"))
            && self
                .plugins
                .runtimes
                .get(owner)
                .is_some_and(|r| r.instance == instance)
    }

    /// Capture local revisions before the public GET, including every possible
    /// target: the DDP label cannot select a canonical episode until it returns.
    fn prepare_plugin_watch(
        &mut self,
        owner: &str,
        anime: i64,
        number: i64,
        watched: bool,
        view: View,
    ) -> Result<Option<u64>> {
        let instance = self
            .plugins
            .runtimes
            .get(owner)
            .map(|r| r.instance)
            .ok_or_else(|| Error::conflict("插件已停止"))?;
        if !self.watch_allowed(owner, instance) {
            return Err(Error::conflict("插件媒体库权限已撤销"));
        }
        if anime <= 0 || number < 0 {
            return Err(Error::invalid("弹弹play作品或集号不正确"));
        }
        let media = view.media()?;
        let node = self
            .db
            .catalog_node(&media)?
            .ok_or_else(|| Error::invalid("媒体不存在"))?;
        let nodes = match node.kind {
            Kind::Season => self.db.catalog_children(&media)?,
            Kind::Movie => vec![node],
            _ => return Err(Error::conflict("请先打开明确的一季或影片")),
        };
        if nodes.len() > 10000 {
            return Err(Error::invalid("分集记录过多"));
        }
        let subject = single_link(&self.db, &media, Kind::Subject)?;
        if subject.reference.id != anime.to_string() {
            return Err(Error::conflict("插件指定的作品与当前媒体不一致"));
        }
        let mut targets = Vec::new();
        for node in nodes {
            if !node.kind.playable() {
                continue;
            }
            let links = self.db.catalog_links(&node.id)?;
            let links: Vec<_> = links
                .into_iter()
                .filter(|l| exact(l, Kind::Episode))
                .collect();
            if let [link] = links.as_slice() {
                targets.push(Target {
                    link: link.clone(),
                    revision: self.db.catalog_watched(&node.id)?.map_or(0, |w| w.revision),
                });
            }
        }
        if targets.is_empty() {
            return Err(Error::conflict("请先确认弹弹play分集关联"));
        }
        let request = Request {
            owner: owner.into(),
            instance,
            anime,
            number,
            watched,
            view,
            subject,
            targets,
        };
        // Multiple calls for the same provider label share one GET, but retain
        // the newest intent and its local snapshot. No unbounded request fan-out.
        if let Some((_, old)) = self.plugins.watches.0.iter_mut().find(|(_, r)| {
            r.owner == owner && r.instance == instance && r.anime == anime && r.number == number
        }) {
            *old = request;
            return Ok(None);
        }
        if self.plugins.watches.0.len() >= 16 {
            return Err(Error::conflict("插件待处理的观看标记过多"));
        }
        self.plugins.next_tag += 1;
        let tag = self.plugins.next_tag;
        self.plugins.watches.0.push((tag, request));
        Ok(Some(tag))
    }

    pub(super) fn plugin_mark_watched(
        &mut self,
        owner: &str,
        anime: i64,
        number: i64,
        watched: bool,
    ) {
        let result = View::current()
            .ok_or_else(|| Error::conflict("没有打开的作品详情"))
            .and_then(|view| self.prepare_plugin_watch(owner, anime, number, watched, view));
        match result {
            Ok(Some(tag)) => {
                let deliver = completion(&self.plugins.inbox, move |result| {
                    Done::EpisodeWatched(tag, result)
                });
                erika_catalog::client::spawn(async move {
                    // Public identity read only: never fetch the current user's
                    // account or send a remote mark directly from this adapter.
                    deliver(erika_dandanplay::catalog::details(&anime.to_string(), true).await);
                });
            }
            Ok(None) => {}
            Err(error) => self.notify(Notice::OperationFailed {
                reason: error.message,
            }),
        }
    }

    fn finish_plugin_watch(
        &mut self,
        tag: u64,
        result: erika_dandanplay::Result<Details>,
        view: Option<View>,
    ) -> Result<bool> {
        let Some(request) = self.plugins.watches.take(tag) else {
            return Ok(false);
        };
        if !self.watch_allowed(&request.owner, request.instance)
            || view.as_ref() != Some(&request.view)
        {
            return Ok(false);
        }
        let details = result.map_err(|e| Error::invalid(&e.message))?;
        let episode = episode_id(&details, request.anime, request.number)?;
        let targets: Vec<_> = request
            .targets
            .iter()
            .filter(|t| t.link.reference.id == episode)
            .collect();
        let [target] = targets.as_slice() else {
            return Err(Error::conflict("该弹弹play分集尚未唯一对应到当前媒体"));
        };
        self.db.catalog_set_watched_linked(
            &Watched {
                media: target.link.media.clone(),
                watched: request.watched,
                manual: true,
                updated_at: erika_store::now(),
                revision: target.revision,
            },
            &[request.subject, target.link.clone()],
        )?;
        Ok(true)
    }

    pub(super) fn plugin_watch_finished(
        &mut self,
        tag: u64,
        result: erika_dandanplay::Result<Details>,
    ) {
        match self.finish_plugin_watch(tag, result, View::current()) {
            Ok(true) => self.catalogue_changed(),
            Ok(false) => {}
            Err(error) => self.notify(Notice::OperationFailed {
                reason: error.message,
            }),
        }
    }
}

#[cfg(test)]
mod tests;
