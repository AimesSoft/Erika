//! Transactional catalogue for the browser's shared in-memory session.
use super::{Db, State};
use crate::catalog::{
    BindingChange, Error, MediaFile, MediaMark, MediaShelf, MediaWatching, Result,
};
use erika_catalog::*;
use std::collections::{BTreeMap, HashSet};

mod jobs;
mod account_sync;
mod playback;
mod portable;
mod requests;
mod watch;

#[derive(Clone, Default)]
pub(super) struct Catalog {
    pub nodes: BTreeMap<MediaId, MediaNode>,
    pub bindings: BTreeMap<i64, FileBinding>,
    revisions: BTreeMap<i64, u64>,
    danmaku_revisions: BTreeMap<i64, u64>,
    danmaku: BTreeMap<i64, DanmakuBinding>,
    links: Vec<ProviderLink>,
    watched: BTreeMap<MediaId, Watched>,
    played: BTreeMap<i64, (u64, erika_catalog::playback::Intervals)>,
    marks: Vec<MediaMark>,
    changes: BTreeMap<String, BindingChange>,
    requests: BTreeMap<String, (crate::catalog::CatalogBatch, crate::catalog::CatalogReceipt)>,
    jobs: Vec<(crate::catalog::CatalogJob, i64)>,
    watch: BTreeMap<String, crate::catalog::WatchSync>,
    accounts: BTreeMap<String, crate::account_sync::Queue>,
}
fn next(value: u64) -> Result<u64> {
    value
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or_else(|| Error::conflict("记录版本超出范围"))
}
impl Catalog {
    pub fn revision(&self, file: i64) -> u64 {
        self.revisions.get(&file).copied().unwrap_or(0)
    }
    pub fn bump(&mut self, file: i64) -> Result<u64> {
        let revision = next(self.revision(file))?;
        self.revisions.insert(file, revision);
        Ok(revision)
    }
    fn danmaku_revision(&self, file: i64) -> u64 {
        self.danmaku_revisions.get(&file).copied().unwrap_or(0)
    }
    fn canonical(&self, id: &MediaId) -> Result<MediaId> {
        Ok(MediaId::parse(&id.0)?)
    }
    fn node(&self, id: &MediaId) -> Result<&MediaNode> {
        self.nodes
            .get(&self.canonical(id)?)
            .ok_or_else(|| Error::invalid("媒体尚未入库"))
    }
    fn identities(&self, nodes: &[MediaNode]) -> Result<crate::catalog_identity::Identities> {
        let by_ref: std::collections::HashMap<_, _> = self
            .nodes
            .values()
            .filter_map(|n| n.reference.as_ref().map(|r| (r, n)))
            .collect();
        crate::catalog_identity::Identities::prepare(
            nodes,
            |id| Ok(self.nodes.get(id).cloned()),
            |r| Ok(by_ref.get(r).map(|n| (*n).clone())),
        )
    }
    fn save_nodes(&mut self, nodes: &[MediaNode]) -> Result<Vec<MediaNode>> {
        let ids = self.identities(nodes)?;
        let nodes: Vec<_> = nodes.iter().map(|n| ids.node(n)).collect();
        self.put_nodes(&nodes)?;
        nodes.iter().map(|n| self.node(&n.id).cloned()).collect()
    }
    fn put_nodes(&mut self, nodes: &[MediaNode]) -> Result<()> {
        if nodes.len() > 100_000 {
            return Err(Error::invalid("媒体记录过多"));
        }
        let mut nodes: Vec<_> = nodes.iter().collect();
        nodes.sort_by_key(|n| match n.kind {
            Kind::Tv | Kind::Movie => 0,
            Kind::Season => 1,
            _ => 2,
        });
        let mut seen = HashSet::new();
        for input in nodes {
            if !seen.insert(&input.id) {
                return Err(Error::invalid("媒体记录重复"));
            }
            input.validate()?;
            let mut n = input.clone();
            if let Some(parent) = &n.parent {
                let p = self.node(parent)?;
                if !matches!(
                    (n.kind, p.kind),
                    (Kind::Season, Kind::Tv) | (Kind::Episode, Kind::Season)
                ) || n.root != p.root
                    || (n.kind == Kind::Episode && n.season_number != p.season_number)
                {
                    return Err(Error::invalid("媒体父级、季号或所属作品不一致"));
                }
            }
            let old = self.nodes.get(&n.id);
            if let Some(old) = old {
                if old.kind != n.kind
                    || old.parent != n.parent
                    || old.root != n.root
                    || old.reference != n.reference
                    || old.season_number != n.season_number
                    || old.episode_number != n.episode_number
                {
                    return Err(Error::conflict("媒体身份或层级不能由资料刷新改写"));
                }
                if n.revision != 0 && n.revision != old.revision {
                    return Err(Error::conflict("媒体资料版本已改变"));
                }
                n.retain_metadata(old);
            }
            if n.reference.is_some()
                && self
                    .nodes
                    .values()
                    .any(|old| old.id != n.id && old.reference == n.reference)
            {
                return Err(Error::conflict("TMDB 身份已经存在"));
            }
            n.validate_artwork()?;
            n.revision = next(old.map_or(0, |n| n.revision))?;
            self.nodes.insert(n.id.clone(), n);
        }
        Ok(())
    }
    fn put_link(&mut self, input: &ProviderLink) -> Result<ProviderLink> {
        input.reference.validate()?;
        let media = self.node(&input.media)?;
        if input.evidence.len() > 65536 {
            return Err(Error::invalid("关联证据过长"));
        }
        if input.reference.provider == Provider::Tmdb
            && (input.relation != Relation::Exact
                || media.reference.as_ref() != Some(&input.reference)
                || !input.coverage.is_empty())
        {
            return Err(Error::invalid("TMDB 关联必须是该节点的精确身份"));
        }
        let mut seen = HashSet::new();
        for id in &input.coverage {
            let n = self.node(id)?;
            if !n.kind.playable()
                || n.root != media.root
                || !seen.insert(id)
                || (media.kind == Kind::Season && n.parent.as_ref() != Some(&media.id))
                || (media.kind.playable() && n.id != media.id)
            {
                return Err(Error::invalid("关联覆盖范围不属于当前媒体"));
            }
        }
        let index = self
            .links
            .iter()
            .position(|l| l.media == input.media && l.reference == input.reference);
        let old = index.map(|i| &self.links[i]);
        if old.map_or(0, |l| l.revision) != input.revision {
            return Err(Error::conflict("外部关联版本已改变"));
        }
        if old.is_some_and(|l| l.manual) && !input.manual {
            return Err(Error::conflict("自动关联不能覆盖人工关联"));
        }
        let mut link = input.clone();
        link.revision = next(link.revision)?;
        if let Some(i) = index {
            self.links[i] = link.clone();
        } else {
            self.links.push(link.clone());
        }
        Ok(link)
    }
}
impl State {
    fn require_file(&self, file: i64) -> Result<()> {
        if self.files.contains_key(&file) {
            Ok(())
        } else {
            Err(Error::invalid("文件不存在"))
        }
    }
    pub(super) fn put_binding(&mut self, input: &FileBinding) -> Result<FileBinding> {
        self.require_file(input.file)?;
        input.validate()?;
        if input.decision != Decision::Blocked && input.items.is_empty() {
            return Err(Error::invalid("绑定必须包含可播放媒体"));
        }
        if input.evidence.len() > 65536 {
            return Err(Error::invalid("绑定证据过长"));
        }
        if input.revision != self.catalog.revision(input.file) {
            return Err(Error::conflict("文件绑定已改变，丢弃过期结果"));
        }
        if input.decision == Decision::Auto
            && self
                .catalog
                .bindings
                .get(&input.file)
                .is_some_and(|b| b.decision != Decision::Auto)
        {
            return Err(Error::conflict("自动识别不能覆盖人工决定"));
        }
        let mut roots = HashSet::new();
        for i in &input.items {
            let n = self.catalog.node(&i.media)?;
            if n.id != i.media || !n.kind.playable() {
                return Err(Error::invalid("文件只能绑定可播放媒体"));
            }
            roots.insert(&n.root);
        }
        if roots.len() > 1 {
            return Err(Error::invalid("一个文件的分集必须属于同一作品"));
        }
        let mut b = input.clone();
        b.revision = self.catalog.bump(input.file)?;
        self.catalog.bindings.insert(b.file, b.clone());
        self.links.insert(
            b.file,
            (
                None,
                Some(
                    match b.decision {
                        Decision::Auto => "auto",
                        Decision::Manual => "manual",
                        Decision::Blocked => "none",
                    }
                    .into(),
                ),
            ),
        );
        Ok(b)
    }
    fn put_danmaku(&mut self, input: &DanmakuBinding) -> Result<DanmakuBinding> {
        self.require_file(input.file)?;
        input.reference.validate()?;
        if input.reference.kind != Kind::Episode
            || !matches!(
                input.reference.provider,
                Provider::Dandanplay | Provider::Misaka
            )
            || !input.shift_seconds.is_finite()
            || !input.user_shift_seconds.is_finite()
            || !(input.shift_seconds + input.user_shift_seconds).is_finite()
        {
            return Err(Error::invalid("弹幕关联或偏移不正确"));
        }
        if let Some(id) = &input.media {
            if !self.catalog.node(id)?.kind.playable() {
                return Err(Error::invalid("弹幕关联的媒体不可播放"));
            }
        }
        if self.catalog.danmaku_revision(input.file) != input.revision {
            return Err(Error::conflict("弹幕关联已改变"));
        }
        if self
            .catalog
            .danmaku
            .get(&input.file)
            .is_some_and(|b| b.manual)
            && !input.manual
        {
            return Err(Error::conflict("自动匹配不能覆盖人工弹幕关联"));
        }
        let mut b = input.clone();
        b.revision = next(b.revision)?;
        self.catalog.danmaku_revisions.insert(b.file, b.revision);
        self.catalog.danmaku.insert(b.file, b.clone());
        Ok(b)
    }
    fn put_watched(&mut self, input: &Watched) -> Result<Watched> {
        let media = self.catalog.node(&input.media)?;
        if media.id != input.media || !media.kind.playable() {
            return Err(Error::invalid("只能标记可播放媒体的观看状态"));
        }
        let old = self.catalog.watched.get(&input.media);
        if old.map_or(0, |w| w.revision) != input.revision {
            return Err(Error::conflict("观看记录已改变"));
        }
        if old.is_some_and(|w| w.manual) && !input.manual {
            return Err(Error::conflict("自动进度不能覆盖人工观看状态"));
        }
        let mut w = input.clone();
        w.revision = next(w.revision)?;
        self.catalog.watched.insert(w.media.clone(), w.clone());
        Ok(w)
    }
    fn put_mark(&mut self, input: &MediaMark) -> Result<MediaMark> {
        let media = self.catalog.node(&input.media)?;
        let previous = self
            .catalog
            .marks
            .iter()
            .find(|m| m.media == input.media && m.reference == input.reference);
        let stored = input.stored(media, previous)?;
        self.catalog
            .marks
            .retain(|m| m.media != stored.media || m.reference != stored.reference);
        self.catalog.marks.push(stored.clone());
        Ok(stored)
    }
    pub(super) fn apply(
        &mut self,
        nodes: &[MediaNode],
        links: &[ProviderLink],
        bindings: &[FileBinding],
        danmaku: &[DanmakuBinding],
    ) -> Result<BindingChange> {
        if bindings.len() > 10000 || danmaku.len() > 10000 {
            return Err(Error::invalid("批量修改过大"));
        }
        let mut seen = HashSet::new();
        let mut change = BindingChange {
            id: MediaId::new().0,
            before: vec![],
            after: vec![],
            danmaku_before: vec![],
            danmaku_after: vec![],
        };
        let ids = self.catalog.identities(nodes)?;
        self.catalog
            .put_nodes(&nodes.iter().map(|n| ids.node(n)).collect::<Vec<_>>())?;
        for link in links {
            self.catalog.put_link(&ids.link(link))?;
        }
        for b in bindings {
            if !seen.insert(b.file) {
                return Err(Error::invalid("批量绑定包含重复文件"));
            }
            change
                .before
                .push(self.catalog.bindings.get(&b.file).cloned());
            change.after.push(self.put_binding(&ids.binding(b))?);
        }
        seen.clear();
        for b in danmaku {
            if !seen.insert(b.file) {
                return Err(Error::invalid("批量弹幕关联包含重复文件"));
            }
            change
                .danmaku_before
                .push(self.catalog.danmaku.get(&b.file).cloned());
            change
                .danmaku_after
                .push(self.put_danmaku(&ids.danmaku(b))?);
        }
        if !change.after.is_empty() || !change.danmaku_after.is_empty() {
            self.catalog
                .changes
                .insert(change.id.clone(), change.clone());
        }
        Ok(change)
    }
    fn media_files(&self, id: &MediaId) -> Result<Vec<MediaFile>> {
        MediaId::parse(&id.0)?;
        let mut rows = Vec::new();
        for b in self.catalog.bindings.values() {
            let Some(a) = self
                .addresses
                .iter()
                .filter(|a| a.file == Some(b.file))
                .max_by_key(|a| a.seen)
            else {
                continue;
            };
            for item in &b.items {
                let n = self.catalog.node(&item.media)?;
                if &n.id == id || &n.root == id || n.parent.as_ref() == Some(id) {
                    let p = self.progress.get(&b.file).copied().unwrap_or_default();
                    rows.push(MediaFile {
                        file: b.file,
                        media: n.id.clone(),
                        url: a.url.clone(),
                        position: p.0,
                        duration: p.1,
                    });
                }
            }
        }
        rows.sort_by_key(|r| {
            let n = &self.catalog.nodes[&r.media];
            (n.season_number, n.episode_number, r.file)
        });
        Ok(rows)
    }
}
impl Db {
    pub fn catalog_context(&self, file: i64) -> Result<crate::catalog_context::MediaContext> {
        self.read(|s| {
            s.require_file(file)?;
            crate::catalog_context::MediaContext::new(
                file,
                s.catalog.revision(file),
                s.catalog.danmaku_revision(file),
                s.catalog.bindings.get(&file).cloned(),
                s.catalog.danmaku.get(&file).cloned(),
            )
            .hydrate(|id| {
                Ok((
                    s.catalog.node(id)?.clone(),
                    s.catalog
                        .links
                        .iter()
                        .filter(|l| &l.media == id)
                        .cloned()
                        .collect(),
                ))
            })
        })
    }

    pub fn catalog_node(&self, id: &MediaId) -> Result<Option<MediaNode>> {
        self.read(|s| Ok(s.catalog.nodes.get(&s.catalog.canonical(id)?).cloned()))
    }
    pub fn catalog_nodes_unchanged(&self, expected: &[(MediaId, u64)]) -> Result<bool> {
        if expected.len() > 10000 {
            return Err(Error::invalid("媒体记录过多"));
        }
        Ok(self.read(|s| {
            expected.iter().all(|(id, revision)| {
                s.catalog
                    .nodes
                    .get(id)
                    .is_some_and(|n| n.revision == *revision)
            })
        }))
    }
    pub fn catalog_revision(&self, file: i64) -> Result<u64> {
        Ok(self.read(|s| s.catalog.revision(file)))
    }
    pub fn catalog_binding(&self, file: i64) -> Result<Option<FileBinding>> {
        Ok(self.read(|s| s.catalog.bindings.get(&file).cloned()))
    }
    pub fn catalog_save_nodes(&self, nodes: &[MediaNode]) -> Result<Vec<MediaNode>> {
        self.edit(|s| s.catalog.save_nodes(nodes))
    }
    pub fn catalog_link(&self, link: &ProviderLink) -> Result<ProviderLink> {
        self.edit(|s| s.catalog.put_link(link))
    }
    pub fn catalog_links(&self, id: &MediaId) -> Result<Vec<ProviderLink>> {
        self.read(|s| {
            let id = s.catalog.canonical(id)?;
            Ok(s.catalog
                .links
                .iter()
                .filter(|l| l.media == id)
                .cloned()
                .collect())
        })
    }
    pub fn catalog_by_ref(&self, reference: &ProviderRef) -> Result<Vec<MediaNode>> {
        reference.validate()?;
        Ok(self.read(|s| {
            s.catalog
                .nodes
                .values()
                .filter(|n| {
                    n.reference.as_ref() == Some(reference)
                        || s.catalog.links.iter().any(|l| {
                            l.media == n.id
                                && l.reference == *reference
                                && l.confirmed
                                && l.relation == Relation::Exact
                        })
                })
                .cloned()
                .collect()
        }))
    }
    pub fn catalog_children(&self, id: &MediaId) -> Result<Vec<MediaNode>> {
        self.read(|s| {
            let id = s.catalog.canonical(id)?;
            let mut nodes: Vec<_> = s
                .catalog
                .nodes
                .values()
                .filter(|n| n.parent.as_ref() == Some(&id))
                .cloned()
                .collect();
            nodes.sort_by_key(|n| (n.season_number, n.episode_number, n.id.clone()));
            Ok(nodes)
        })
    }
    /// Same physical-file lookup as SQLite; source listings exclude direct opens.
    pub fn catalog_addresses(&self, file: i64) -> Result<Vec<crate::catalog::AddressCheck>> {
        self.read(|s| {
            s.require_file(file)?;
            let mut addresses: Vec<_> = s
                .addresses
                .iter()
                .filter(|a| a.file == Some(file))
                .collect();
            addresses.sort_by_key(|a| {
                (
                    a.source == 1,
                    std::cmp::Reverse(a.seen),
                    std::cmp::Reverse(a.key),
                )
            });
            Ok(addresses
                .into_iter()
                .map(|a| crate::catalog::AddressCheck {
                    key: a.key,
                    source: a.source,
                    url: a.url.clone(),
                    file,
                })
                .collect())
        })
    }
    pub fn catalog_apply_selection(
        &self,
        addresses: &[crate::catalog::AddressCheck],
        nodes: &[MediaNode],
        bindings: &[FileBinding],
    ) -> Result<BindingChange> {
        crate::catalog::AddressCheck::validate_batch(addresses, bindings)?;
        self.edit(|s| {
            for expected in addresses {
                if !s.addresses.iter().any(|a| {
                    a.key == expected.key
                        && a.source == expected.source
                        && a.url == expected.url
                        && a.file == Some(expected.file)
                }) {
                    return Err(Error::conflict("文件地址已改变，请重新打开匹配"));
                }
            }
            s.apply(nodes, &[], bindings, &[])
        })
    }
    pub fn catalog_apply(
        &self,
        nodes: &[MediaNode],
        links: &[ProviderLink],
        bindings: &[FileBinding],
        danmaku: &[DanmakuBinding],
    ) -> Result<BindingChange> {
        self.edit(|s| s.apply(nodes, links, bindings, danmaku))
    }
    pub fn catalog_undo(&self, id: &str) -> Result<()> {
        self.edit(|s| {
            let c = s
                .catalog
                .changes
                .get(id)
                .cloned()
                .ok_or_else(|| Error::conflict("修改不存在或已撤销"))?;
            for b in &c.after {
                if s.catalog.bindings.get(&b.file) != Some(b) {
                    return Err(Error::conflict("文件在此后又有修改，不能覆盖撤销"));
                }
            }
            for b in &c.danmaku_after {
                if s.catalog.danmaku.get(&b.file) != Some(b) {
                    return Err(Error::conflict("弹幕关联在此后又有修改，不能覆盖撤销"));
                }
            }
            for (before, after) in c.before.iter().zip(&c.after) {
                s.catalog.bindings.remove(&after.file);
                s.links.remove(&after.file);
                if let Some(before) = before {
                    let mut b = before.clone();
                    b.revision = after.revision;
                    s.put_binding(&b)?;
                } else {
                    s.catalog.bump(after.file)?;
                }
            }
            for (before, after) in c.danmaku_before.iter().zip(&c.danmaku_after) {
                s.catalog.danmaku.remove(&after.file);
                if let Some(before) = before {
                    let mut b = before.clone();
                    b.revision = after.revision;
                    s.put_danmaku(&b)?;
                } else {
                    s.catalog
                        .danmaku_revisions
                        .insert(after.file, next(after.revision)?);
                }
            }
            s.catalog.changes.remove(id);
            Ok(())
        })
    }
    pub fn catalog_danmaku_revision(&self, file: i64) -> Result<u64> {
        Ok(self.read(|s| s.catalog.danmaku_revision(file)))
    }
    pub fn catalog_danmaku(&self, file: i64) -> Result<Option<DanmakuBinding>> {
        Ok(self.read(|s| s.catalog.danmaku.get(&file).cloned()))
    }
    pub fn catalog_set_danmaku(&self, b: &DanmakuBinding) -> Result<DanmakuBinding> {
        self.edit(|s| s.put_danmaku(b))
    }
    pub fn catalog_set_watched(&self, w: &Watched) -> Result<Watched> {
        self.edit(|s| s.put_watched(w))
    }
    /// Identity checks and the watch CAS run under the same session lock.
    pub fn catalog_set_watched_linked(
        &self,
        value: &Watched,
        links: &[ProviderLink],
    ) -> Result<Watched> {
        self.edit(|s| {
            crate::catalog::check_watched_links(value, links, |link| {
                let current = s
                    .catalog
                    .links
                    .iter()
                    .filter(|l| l.media == link.media)
                    .cloned()
                    .collect();
                let assigned = s
                    .catalog
                    .nodes
                    .values()
                    .filter(|n| {
                        n.reference.as_ref() == Some(&link.reference)
                            || s.catalog.links.iter().any(|l| {
                                l.media == n.id
                                    && l.reference == link.reference
                                    && l.confirmed
                                    && l.relation == Relation::Exact
                            })
                    })
                    .map(|n| n.id.clone())
                    .collect();
                Ok((current, assigned))
            })?;
            s.put_watched(value)
        })
    }
    pub fn catalog_watched(&self, id: &MediaId) -> Result<Option<Watched>> {
        self.read(|s| Ok(s.catalog.watched.get(&s.catalog.canonical(id)?).cloned()))
    }
    pub fn catalog_set_mark(&self, m: &MediaMark) -> Result<MediaMark> {
        self.edit(|s| s.put_mark(m))
    }
    pub fn catalog_edit_mark(
        &self,
        media: &MediaId,
        edit: &crate::catalog::MarkEdit,
    ) -> Result<MediaMark> {
        self.edit(|s| {
            let mut value = s
                .catalog
                .marks
                .iter()
                .find(|m| &m.media == media && m.reference.is_none())
                .cloned()
                .unwrap_or_else(|| MediaMark::local(media.clone()));
            value.edit(edit, crate::now());
            s.put_mark(&value)
        })
    }
    pub fn catalog_marks(&self, id: &MediaId) -> Result<Vec<MediaMark>> {
        self.read(|s| {
            let id = s.catalog.canonical(id)?;
            Ok(s.catalog
                .marks
                .iter()
                .filter(|m| m.media == id)
                .cloned()
                .collect())
        })
    }
    pub fn catalog_files(&self, id: &MediaId) -> Result<Vec<MediaFile>> {
        self.read(|s| s.media_files(&s.catalog.canonical(id)?))
    }
    pub fn catalog_shelf(&self, source: Option<i64>) -> Result<Vec<MediaShelf>> {
        self.read(|s| {
            let mut groups: BTreeMap<MediaId, (MediaNode, BTreeMap<MediaId, bool>, i64, i64)> =
                BTreeMap::new();
            for a in &s.addresses {
                if source.is_some_and(|id| id != a.source) {
                    continue;
                }
                let Some(b) = a.file.and_then(|id| s.catalog.bindings.get(&id)) else {
                    continue;
                };
                for item in &b.items {
                    let n = s.catalog.node(&item.media)?;
                    let root = s.catalog.node(&n.root)?;
                    let watched = s.catalog.watched.get(&n.id).is_some_and(|w| w.watched);
                    let group = groups.entry(root.id.clone()).or_insert((
                        root.clone(),
                        BTreeMap::new(),
                        a.seen,
                        a.seen,
                    ));
                    *group.1.entry(n.id.clone()).or_default() |= watched;
                    group.2 = group.2.min(a.seen);
                    group.3 = group.3.max(a.seen);
                }
            }
            let mut rows: Vec<_> = groups
                .into_values()
                .map(|(node, items, added, seen)| MediaShelf {
                    node,
                    episodes: items.len() as i64,
                    watched: items.values().filter(|v| **v).count() as i64,
                    added,
                    seen,
                })
                .collect();
            rows.sort_by(|a, b| b.seen.cmp(&a.seen).then_with(|| a.node.id.cmp(&b.node.id)));
            Ok(rows)
        })
    }
    pub fn catalog_watching(&self, limit: usize) -> Result<Vec<MediaWatching>> {
        self.read(|s| {
            let mut recent: Vec<_> = s.recents.values().collect();
            recent.sort_by_key(|r| std::cmp::Reverse(r.1));
            let mut seen = HashSet::new();
            let mut rows = Vec::new();
            for (r, _) in recent {
                let Some(file) = s
                    .addresses
                    .iter()
                    .find(|a| a.source == 1 && a.url == r.url)
                    .and_then(|a| a.file)
                else {
                    continue;
                };
                let Some(b) = s.catalog.bindings.get(&file) else {
                    continue;
                };
                let p = s
                    .progress
                    .get(&file)
                    .copied()
                    .unwrap_or((r.position, r.duration, 0., 0));
                let Some(first) = b.items.first() else {
                    continue;
                };
                let node = s.catalog.node(&first.media)?;
                let work = s.catalog.node(&node.root)?;
                let episode = b
                    .playing_item(p.0, p.1)
                    .map(|item| s.catalog.node(&item.media).cloned())
                    .transpose()?;
                if rows.len() >= limit || !seen.insert(&work.id) {
                    continue;
                }
                rows.push(MediaWatching {
                    work: work.clone(),
                    episode,
                    still: r.still.clone(),
                    file: crate::catalog::FileResume {
                        file,
                        url: r.url.clone(),
                        position: p.0,
                        duration: p.1,
                    },
                });
            }
            Ok(rows)
        })
    }
}
