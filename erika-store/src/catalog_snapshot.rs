//! Snapshot identity resolution uses current TMDB references, not upgrade aliases.
use crate::catalog::{CatalogSnapshot, Error, Result};
use erika_catalog::{MediaId, MediaNode};
use std::collections::{HashMap, HashSet};

impl CatalogSnapshot {
    /// Stable transport identities and no device-local CAS counters. This is a
    /// projection of the current schema, not a persistent alias or a conversion
    /// from a previous file format.
    pub fn for_sync(&self) -> Result<Self> {
        let mut identities = self.nodes.clone();
        for node in &mut identities {
            node.id = MediaId::for_ref(
                node.reference
                    .as_ref()
                    .ok_or_else(|| Error::invalid("规范媒体缺少 TMDB 身份"))?,
            );
        }
        let mut out = normalize(self, &identities)?;
        for link in &mut out.links {
            link.revision = 0;
        }
        for entry in &mut out.bindings {
            entry.binding.file = 0;
            entry.binding.revision = 0;
        }
        for (_, binding) in &mut out.danmaku {
            binding.file = 0;
            binding.revision = 0;
        }
        for watched in &mut out.watched {
            watched.revision = 0;
        }
        for mark in &mut out.marks {
            mark.revision = 0;
        }
        Ok(out)
    }
}

pub(crate) fn normalize(
    incoming: &CatalogSnapshot,
    local: &[MediaNode],
) -> Result<CatalogSnapshot> {
    if [
        incoming.nodes.len(),
        incoming.links.len(),
        incoming.bindings.len(),
        incoming.danmaku.len(),
        incoming.watched.len(),
        incoming.marks.len(),
    ]
    .into_iter()
    .any(|n| n > 100_000)
    {
        return Err(Error::invalid("媒体库快照过大"));
    }
    let mut ids = HashSet::new();
    let mut references = HashSet::new();
    let by_id: HashMap<_, _> = local.iter().map(|n| (&n.id, n)).collect();
    let by_ref: HashMap<_, _> = local
        .iter()
        .filter_map(|n| n.reference.as_ref().map(|r| (r, &n.id)))
        .collect();
    let mut resolved = HashMap::new();
    for n in &incoming.nodes {
        n.validate()?;
        if !ids.insert(&n.id) || n.reference.as_ref().is_some_and(|r| !references.insert(r)) {
            return Err(Error::invalid("快照包含重复媒体身份"));
        }
        if by_id
            .get(&n.id)
            .is_some_and(|old| old.reference != n.reference)
        {
            return Err(Error::conflict("快照 UUID 与另一媒体身份冲突"));
        }
        if let Some(local_id) = n.reference.as_ref().and_then(|r| by_ref.get(r)) {
            resolved.insert(n.id.clone(), (*local_id).clone());
        }
    }
    let resolve = |id: &MediaId| resolved.get(id).cloned().unwrap_or_else(|| id.clone());
    let mut out = incoming.clone();
    for n in &mut out.nodes {
        n.id = resolve(&n.id);
        n.root = resolve(&n.root);
        n.parent = n.parent.as_ref().map(&resolve);
        n.revision = 0;
    }
    for link in &mut out.links {
        link.media = resolve(&link.media);
        link.coverage = link.coverage.iter().map(&resolve).collect();
    }
    for b in &mut out.bindings {
        b.file = normalize_file_key(&b.file)?;
        for item in &mut b.binding.items {
            item.media = resolve(&item.media);
        }
    }
    for (key, b) in &mut out.danmaku {
        *key = normalize_file_key(key)?;
        b.media = b.media.as_ref().map(&resolve);
    }
    for w in &mut out.watched {
        w.media = resolve(&w.media);
    }
    for m in &mut out.marks {
        m.media = resolve(&m.media);
    }
    Ok(out)
}

pub(crate) fn changed_nodes(incoming: &[MediaNode], local: &[MediaNode]) -> Vec<MediaNode> {
    let local: HashMap<_, _> = local
        .iter()
        .map(|node| {
            let mut node = node.clone();
            node.revision = 0;
            (node.id.clone(), node)
        })
        .collect();
    incoming
        .iter()
        .filter(|node| local.get(&node.id) != Some(*node))
        .cloned()
        .collect()
}

pub(crate) fn normalize_file_key(
    key: &crate::portable::FileKey,
) -> Result<crate::portable::FileKey> {
    use crate::portable::FileKey;
    match key {
        FileKey::Hash { md5, size } => {
            if *size > i64::MAX as u64
                || md5.len() != 32
                || !md5.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(Error::invalid("文件指纹格式不正确"));
            }
            Ok(FileKey::Hash {
                md5: md5.to_ascii_lowercase(),
                size: *size,
            })
        }
        FileKey::Item(item) => {
            if item.is_empty() || item.len() > 1024 || item.chars().any(char::is_control) {
                return Err(Error::invalid("服务器文件身份不正确"));
            }
            Ok(key.clone())
        }
    }
}
