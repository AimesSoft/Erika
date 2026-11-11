//! Plugin-owned catalogues. A scan is staged until a complete, ordered snapshot
//! is acknowledged; playback credentials never belong to these records.
use crate::catalog::{Error, Result};
use erika_catalog::{
    Kind, MediaId, ProviderRef,
    source::{MediaHint, Ordering},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub(crate) const LEASE_SECONDS: i64 = 3600;
pub(crate) const MAX_ITEMS: usize = 100_000;
pub(crate) const MAX_PAGES: u32 = 1000;
pub mod page;
pub mod status;

pub(crate) fn token(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 200
        || value.chars().any(char::is_control)
        || value.contains("://")
        || value.contains(['?', '#', '@'])
    {
        return Err(Error::invalid("来源标识必须是稳定 ID，不能是网址或凭据"));
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Descriptor {
    pub id: String,
    pub server_id: String,
    pub name: String,
    pub content: String,
}
impl Descriptor {
    pub(crate) fn validate(&self, owner: &str) -> Result<()> {
        token(owner)?;
        token(&self.id)?;
        token(&self.server_id)?;
        if self.name.trim().is_empty()
            || self.name.len() > 1024
            || !matches!(self.content.as_str(), "anime" | "series" | "movie")
        {
            return Err(Error::invalid("来源名称或内容类型不正确"));
        }
        Ok(())
    }
    /// Decode only the current portable source identity, never a local row id.
    pub(crate) fn from_source(source: &crate::portable::Source) -> Result<Option<(String, Self)>> {
        if source.kind != "plugin" {
            return Ok(None);
        }
        let (owner, id, server_id): (String, String, String) =
            serde_json::from_str(&source.location)
                .map_err(|_| Error::invalid("插件来源身份格式不正确"))?;
        let descriptor = Self {
            id,
            server_id,
            name: source.name.clone(),
            content: source.content.clone(),
        };
        descriptor.validate(&owner)?;
        if descriptor.location(&owner) != source.location {
            return Err(Error::invalid("插件来源身份不是规范格式"));
        }
        Ok(Some((owner, descriptor)))
    }
    pub(crate) fn portable(&self, owner: &str) -> crate::portable::Source {
        crate::portable::Source {
            kind: "plugin".into(),
            name: self.name.clone(),
            location: self.location(owner),
            content: self.content.clone(),
            added: crate::now(),
        }
    }
    pub(crate) fn location(&self, owner: &str) -> String {
        // Length-delimited JSON avoids delimiter collisions between instances.
        serde_json::to_string(&(owner, &self.id, &self.server_id)).expect("strings serialize")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Item {
    pub source_item_id: String,
    pub server_id: String,
    pub media_source_id: String,
    pub kind: Kind,
    pub series_item_id: Option<String>,
    pub season_item_id: Option<String>,
    #[serde(default)]
    pub provider_refs: Vec<ProviderRef>,
    #[serde(default)]
    pub series_provider_refs: Vec<ProviderRef>,
    pub season_number: Option<u32>,
    pub episode_number: Option<u32>,
    pub episode_number_end: Option<u32>,
    #[serde(default)]
    pub ordering: Ordering,
    pub title: String,
    /// Opaque lookup key, not a URL/token. Only the owning plugin receives it.
    pub playback_handle: String,
    pub fingerprint: Option<crate::Fingerprint>,
}
impl Item {
    pub fn hint(&self) -> MediaHint {
        MediaHint {
            kind: self.kind,
            provider_refs: self.provider_refs.clone(),
            series_provider_refs: self.series_provider_refs.clone(),
            season_number: self.season_number,
            episode_number: self.episode_number,
            episode_number_end: self.episode_number_end,
            ordering: self.ordering,
        }
    }
    pub fn key(&self, owner: &str) -> Result<crate::portable::FileKey> {
        let value = if let Some(fp) = &self.fingerprint {
            crate::portable::FileKey::of(fp)
        } else {
            crate::portable::FileKey::Item(serde_json::to_string(&(
                owner,
                &self.server_id,
                &self.source_item_id,
                &self.media_source_id,
            ))?)
        };
        crate::catalog_snapshot::normalize_file_key(&value)
    }
    /// Item versions remain separate even when they share a verified fingerprint.
    pub(crate) fn slot(&self) -> String {
        serde_json::to_string(&(&self.source_item_id, &self.media_source_id))
            .expect("strings serialize")
    }
    pub(crate) fn validate(&self, owner: &str, server: &str) -> Result<()> {
        token(&self.source_item_id)?;
        token(&self.server_id)?;
        token(&self.media_source_id)?;
        token(&self.playback_handle)?;
        for id in [&self.series_item_id, &self.season_item_id]
            .into_iter()
            .flatten()
        {
            token(id)?;
        }
        if self.server_id != server || self.title.trim().is_empty() || self.title.len() > 8192 {
            return Err(Error::invalid("来源条目属于另一服务器或标题不正确"));
        }
        if self.kind == Kind::Movie
            && (self.series_item_id.is_some() || self.season_item_id.is_some())
        {
            return Err(Error::invalid("电影不能属于季或剧集条目"));
        }
        self.hint().validate()?;
        self.key(owner)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredItem {
    pub source: i64,
    pub owner: String,
    pub generation: String,
    pub file: i64,
    pub url: String,
    pub item: Item,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scan {
    pub id: String,
    pub source: i64,
    pub next_page: u32,
    pub state: String,
    pub expires_at: i64,
}
impl Scan {
    pub(crate) fn new(source: i64) -> Self {
        Self {
            id: MediaId::new().0,
            source,
            next_page: 0,
            state: "running".into(),
            expires_at: crate::now() + LEASE_SECONDS,
        }
    }
    pub(crate) fn active(&self) -> Result<()> {
        if self.state != "running" || self.expires_at <= crate::now() {
            return Err(Error::conflict("来源扫描已结束、取消或过期"));
        }
        Ok(())
    }
    pub(crate) fn finish(&mut self, pages: u32, complete: bool) -> Result<bool> {
        let state = if complete { "complete" } else { "cancelled" };
        if self.state == state && pages == self.next_page {
            return Ok(false);
        }
        self.active()?;
        if pages != self.next_page {
            return Err(Error::conflict("扫描分页尚未完整落库"));
        }
        self.state = state.into();
        Ok(true)
    }
}
pub(crate) fn validate_page(owner: &str, server: &str, items: &[Item]) -> Result<String> {
    if items.is_empty() || items.len() > 1000 {
        return Err(Error::invalid("每页应包含 1 到 1000 个来源条目"));
    }
    let mut ids = HashSet::new();
    for item in items {
        item.validate(owner, server)?;
        if !ids.insert(item.slot()) {
            return Err(Error::invalid("同一页包含重复来源条目"));
        }
    }
    let json = serde_json::to_string(items)?;
    if json.len() > 4 * 1024 * 1024 {
        return Err(Error::invalid("来源分页过大"));
    }
    Ok(json)
}

/// Stable logical addresses contain neither local row ids nor transport secrets.
/// The owning plugin still has to publish the directory and resolve playback.
pub(crate) fn address(owner: &str, source: &str, server: &str, slot: &str) -> String {
    let scoped = serde_json::to_string(&(owner, source, server, slot)).expect("strings serialize");
    let encoded: String = scoped
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("erika-source://item/{encoded}")
}
pub fn is_address(url: &str) -> bool {
    url.starts_with("erika-source://")
}

/// The immutable source snapshot survives the job's result envelope. A forged
/// or malformed source input is an error, never an ordinary unguarded file job.
pub fn job_source(job: &crate::catalog::CatalogJob) -> Result<Option<StoredItem>> {
    let value: serde_json::Value = serde_json::from_str(&job.payload)?;
    let input = value.get("input").unwrap_or(&value);
    input
        .get("sourceItem")
        .map(|v| serde_json::from_value(v.clone()).map_err(Error::from))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_urls_and_credentials_in_lookup_keys() {
        for id in [
            "https://server/item?token=secret",
            "user@host",
            "key#secret",
            "a\nb",
            "",
        ] {
            assert!(token(id).is_err());
        }
        assert!(token("opaque-item-42").is_ok());
    }
}
