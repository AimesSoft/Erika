//! A file's complete catalogue view, captured under one database read snapshot.
//! Addresses, playback credentials and account secrets are deliberately absent.
use crate::catalog::{Error, Result};
use erika_catalog::{DanmakuBinding, FileBinding, MediaId, MediaNode, ProviderLink};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaContext {
    pub file: i64,
    pub revision: u64,
    pub danmaku_revision: u64,
    pub binding: Option<FileBinding>,
    pub danmaku: Option<DanmakuBinding>,
    pub nodes: Vec<MediaNode>,
    pub mappings: Vec<ProviderLink>,
}

impl MediaContext {
    pub(crate) fn new(
        file: i64,
        revision: u64,
        danmaku_revision: u64,
        binding: Option<FileBinding>,
        danmaku: Option<DanmakuBinding>,
    ) -> Self {
        Self {
            file,
            revision,
            danmaku_revision,
            binding,
            danmaku,
            nodes: vec![],
            mappings: vec![],
        }
    }

    /// The caller must keep the same read transaction/lock throughout hydration.
    pub(crate) fn hydrate(
        mut self,
        mut read: impl FnMut(&MediaId) -> Result<(MediaNode, Vec<ProviderLink>)>,
    ) -> Result<Self> {
        let mut nodes = BTreeMap::new();
        let mut pending: Vec<_> = self
            .binding
            .iter()
            .flat_map(|b| b.items.iter().map(|i| i.media.clone()))
            .chain(self.danmaku.iter().filter_map(|d| d.media.clone()))
            .collect();
        while let Some(media) = pending.pop() {
            if nodes.contains_key(&media) {
                continue;
            }
            let (node, links) = read(&media)?;
            if node.id != media {
                return Err(Error::invalid("媒体上下文身份不一致"));
            }
            if let Some(parent) = &node.parent {
                pending.push(parent.clone());
            }
            nodes.insert(media, (node, links));
        }
        for (node, mut links) in nodes.into_values() {
            links.sort_by_key(|l| {
                (
                    format!("{:?}", l.reference.provider),
                    l.reference.scope.clone(),
                    l.reference.kind.as_str(),
                    l.reference.id.clone(),
                )
            });
            self.nodes.push(node);
            self.mappings.extend(links);
        }
        Ok(self)
    }

    pub fn playing(&self, position: f64, duration: f64) -> Option<&MediaId> {
        self.binding
            .as_ref()?
            .playing_item(position, duration)
            .map(|i| &i.media)
    }
}
