//! Resolve one incoming batch against current typed TMDB identities. This map
//! lives only for the transaction; it neither stores aliases nor changes UUIDs
//! already used by the local catalogue.
use crate::catalog::{Error, Result};
use erika_catalog::{DanmakuBinding, FileBinding, MediaId, MediaNode, ProviderLink, ProviderRef};
use std::collections::{HashMap, HashSet};

pub(crate) struct Identities(HashMap<MediaId, MediaId>);
impl Identities {
    pub fn prepare(
        nodes: &[MediaNode],
        by_id: impl Fn(&MediaId) -> Result<Option<MediaNode>>,
        by_ref: impl Fn(&ProviderRef) -> Result<Option<MediaNode>>,
    ) -> Result<Self> {
        if nodes.len() > 100_000 { return Err(Error::invalid("媒体记录过多")); }
        let mut ids = HashSet::new();
        let mut refs = HashSet::new();
        let mut map = HashMap::new();
        for n in nodes {
            n.validate()?;
            let reference = n.reference.as_ref().ok_or_else(|| Error::invalid("规范媒体缺少 TMDB 身份"))?;
            if !ids.insert(&n.id) || !refs.insert(reference) {
                return Err(Error::invalid("媒体记录重复"));
            }
            if by_id(&n.id)?.is_some_and(|old| old.reference.as_ref() != Some(reference)) {
                return Err(Error::conflict("UUID 与另一媒体身份冲突"));
            }
            if let Some(old) = by_ref(reference)? { map.insert(n.id.clone(), old.id); }
        }
        Ok(Self(map))
    }
    pub fn resolve(&self, id: &MediaId) -> MediaId {
        self.0.get(id).cloned().unwrap_or_else(|| id.clone())
    }
    pub fn node(&self, n: &MediaNode) -> MediaNode {
        let mut n = n.clone();
        n.id = self.resolve(&n.id);
        n.root = self.resolve(&n.root);
        n.parent = n.parent.as_ref().map(|id| self.resolve(id));
        n
    }
    pub fn binding(&self, b: &FileBinding) -> FileBinding {
        let mut b = b.clone();
        for item in &mut b.items { item.media = self.resolve(&item.media); }
        b
    }
    pub fn danmaku(&self, b: &DanmakuBinding) -> DanmakuBinding {
        let mut b = b.clone();
        b.media = b.media.as_ref().map(|id| self.resolve(id));
        b
    }
    pub fn link(&self, l: &ProviderLink) -> ProviderLink {
        let mut l = l.clone();
        l.media = self.resolve(&l.media);
        l.coverage = l.coverage.iter().map(|id| self.resolve(id)).collect();
        l
    }
}
