//! Keyset pages pin the last published generation, not the scan being staged.
use super::{StoredItem, token};
use crate::catalog::{Error, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_LIMIT: usize = 100;
pub const MAX_LIMIT: usize = 1000;
pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListRequest {
    pub source: i64,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}
fn default_limit() -> usize {
    DEFAULT_LIMIT
}
impl ListRequest {
    pub fn new(source: i64) -> Self {
        Self {
            source,
            cursor: None,
            limit: DEFAULT_LIMIT,
        }
    }
    pub(crate) fn after(&self, owner: &str, generation: &str) -> Result<String> {
        if self.source <= 0 || !(1..=MAX_LIMIT).contains(&self.limit) {
            return Err(Error::invalid("来源或分页大小不正确"));
        }
        let Some(value) = &self.cursor else {
            return Ok(String::new());
        };
        if value.len() > 8192 {
            return Err(Error::invalid("来源游标过长"));
        }
        let cursor: Cursor = serde_json::from_str(value)?;
        if cursor.source != self.source || cursor.owner != owner || cursor.generation != generation
        {
            return Err(Error::conflict(
                "来源目录已改变或游标不属于此来源，请从第一页读取",
            ));
        }
        let (item, version): (String, String) = serde_json::from_str(&cursor.after)?;
        token(&item)?;
        token(&version)?;
        Ok(cursor.after)
    }
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemRequest {
    pub source: i64,
    pub source_item_id: String,
    pub media_source_id: String,
}
impl ItemRequest {
    pub(crate) fn slot(&self) -> Result<String> {
        if self.source <= 0 {
            return Err(Error::invalid("来源标识不正确"));
        }
        token(&self.source_item_id)?;
        token(&self.media_source_id)?;
        Ok(serde_json::to_string(&(
            &self.source_item_id,
            &self.media_source_id,
        ))?)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub generation: String,
    pub items: Vec<StoredItem>,
    pub next_cursor: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    owner: String,
    source: i64,
    generation: String,
    after: String,
}

pub(crate) fn collect(
    owner: &str,
    request: &ListRequest,
    generation: &str,
    rows: impl IntoIterator<Item = Result<StoredItem>>,
) -> Result<Page> {
    let mut page = Page {
        generation: generation.into(),
        items: Vec::new(),
        next_cursor: None,
    };
    // Reserve space for the envelope, escaping and the next cursor.
    let budget = MAX_RESPONSE_BYTES - 16384;
    let mut bytes = 0;
    for row in rows {
        let row = row?;
        let size = serde_json::to_vec(&row)?.len() + 1;
        if size > budget {
            return Err(Error::invalid("来源条目过大，无法分页读取"));
        }
        if page.items.len() >= request.limit || bytes + size > budget {
            let last = page
                .items
                .last()
                .ok_or_else(|| Error::invalid("来源分页为空"))?;
            page.next_cursor = Some(serde_json::to_string(&Cursor {
                owner: owner.into(),
                source: request.source,
                generation: generation.into(),
                after: last.item.slot(),
            })?);
            break;
        }
        bytes += size;
        page.items.push(row);
    }
    Ok(page)
}
