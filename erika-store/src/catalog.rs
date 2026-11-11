//! Public catalogue storage records; local row IDs are never portable media IDs.
use erika_catalog::{FileBinding, MediaId, MediaNode, ProviderRef, Watched};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub code: String,
    pub message: String,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn conflict(message: &str) -> Self {
        Self {
            code: "conflict".into(),
            message: message.into(),
        }
    }
    pub fn invalid(message: &str) -> Self {
        Self {
            code: "invalid".into(),
            message: message.into(),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}
impl std::error::Error for Error {}
impl From<erika_catalog::Error> for Error {
    fn from(error: erika_catalog::Error) -> Self {
        Self {
            code: format!("{:?}", error.code),
            message: error.message,
        }
    }
}
impl From<serde_json::Error> for Error {
    fn from(_: serde_json::Error) -> Self {
        Self::invalid("媒体记录格式不正确")
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self {
            code: "storage".into(),
            message: format!("媒体库读写失败：{error}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaFile {
    pub file: i64,
    pub media: MediaId,
    pub url: String,
    pub position: f64,
    pub duration: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaShelf {
    pub node: MediaNode,
    pub episodes: i64,
    pub watched: i64,
    pub added: i64,
    pub seen: i64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaWatching {
    pub work: MediaNode,
    /// A combined file without complete chapter times has no guessed episode.
    pub episode: Option<MediaNode>,
    pub file: FileResume,
    pub still: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileResume {
    pub file: i64,
    pub url: String,
    pub position: f64,
    pub duration: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaMark {
    pub media: MediaId,
    /// None means local only, never implicitly Bangumi.
    pub reference: Option<ProviderRef>,
    pub status: Option<String>,
    /// None means no local rating decision; Some(0) explicitly clears it.
    pub rating: Option<i64>,
    /// None means no local review decision; Some("") explicitly clears it.
    pub review: Option<String>,
    pub updated_at: i64,
    /// Device-local CAS clock, never copied from another device.
    pub revision: u64,
}

/// A UI intent updates one field under the storage write lock. Unrelated fields
/// are read in that same transaction, not from an earlier UI/network snapshot.
#[derive(Clone, Debug, PartialEq)]
pub enum MarkEdit {
    Status(String),
    Rating(i64),
    Review(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CatalogJob {
    pub id: String,
    pub file: Option<i64>,
    pub input_revision: u64,
    pub phase: String,
    pub state: String,
    pub payload: String,
    pub error: String,
    pub next_attempt: i64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WatchSync {
    pub operation: String,
    pub media: MediaId,
    pub account: String,
    pub target: ProviderRef,
    pub watched: bool,
    pub local_revision: u64,
    pub mapping_revision: u64,
    pub state: String,
    pub error: String,
    pub remote_baseline: Option<bool>,
    /// Every transition advances this clock, including recovery after a crash.
    /// A late reply from a previous delivery cannot complete a retry.
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub lease_until: i64,
}
/// Address identity captured alongside a binding revision. A rescan may replace
/// the physical file without changing the old file's binding revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddressCheck {
    pub key: i64,
    pub source: i64,
    pub url: String,
    pub file: i64,
}

impl AddressCheck {
    pub(crate) fn validate_batch(addresses: &[Self], bindings: &[FileBinding]) -> Result<()> {
        if addresses.len() > 10000 || bindings.len() > 10000 {
            return Err(Error::invalid("批量修改过大"));
        }
        let files: std::collections::HashSet<_> = addresses.iter().map(|a| a.file).collect();
        if bindings.iter().any(|b| !files.contains(&b.file)) {
            return Err(Error::invalid("文件绑定缺少地址快照"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BindingChange {
    pub id: String,
    pub before: Vec<Option<FileBinding>>,
    pub after: Vec<FileBinding>,
    #[serde(default)]
    pub danmaku_before: Vec<Option<erika_catalog::DanmakuBinding>>,
    #[serde(default)]
    pub danmaku_after: Vec<erika_catalog::DanmakuBinding>,
}
/// Validate identity evidence again inside the transaction which writes a watch
/// mark. Neither a new competing association nor an ABA edit may be accepted.
pub(crate) fn check_watched_links(
    watched: &Watched,
    expected: &[erika_catalog::ProviderLink],
    mut read: impl FnMut(
        &erika_catalog::ProviderLink,
    ) -> Result<(Vec<erika_catalog::ProviderLink>, Vec<MediaId>)>,
) -> Result<()> {
    use erika_catalog::Relation;
    if expected.is_empty()
        || expected.len() > 8
        || !expected.iter().any(|link| link.media == watched.media)
    {
        return Err(Error::invalid("观看状态缺少身份快照"));
    }
    for link in expected {
        link.reference.validate()?;
        if !link.confirmed || link.relation != Relation::Exact {
            return Err(Error::invalid("观看状态需要已确认的精确关联"));
        }
        let (current, assigned) = read(link)?;
        let current: Vec<_> = current
            .iter()
            .filter(|l| {
                l.confirmed
                    && l.relation == Relation::Exact
                    && l.reference.provider == link.reference.provider
                    && l.reference.scope == link.reference.scope
                    && l.reference.kind == link.reference.kind
            })
            .collect();
        if current.as_slice() != [link]
            || assigned.len() != 1
            || assigned.first() != Some(&link.media)
        {
            return Err(Error::conflict("媒体关联已改变或不再唯一"));
        }
    }
    Ok(())
}

/// Explicit local decisions shared by the CLI and plugin host. Metadata is
/// fetched separately from TMDB; callers cannot inject arbitrary media nodes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CatalogBatch {
    pub bindings: Vec<FileBinding>,
    pub danmaku: Vec<erika_catalog::DanmakuBinding>,
    pub links: Vec<erika_catalog::ProviderLink>,
}
impl CatalogBatch {
    pub fn validate(&self) -> Result<()> {
        if self.bindings.len() + self.danmaku.len() + self.links.len() == 0
            || self.bindings.len() > 1000
            || self.danmaku.len() > 1000
            || self.links.len() > 1000
        {
            return Err(Error::invalid("批量修改需要 1 至 1000 条同类记录"));
        }
        if self
            .bindings
            .iter()
            .any(|b| b.decision == erika_catalog::Decision::Auto)
            || self
                .danmaku
                .iter()
                .any(|b| !b.manual || b.fingerprint_verified)
            || self.links.iter().any(|l| !l.manual)
        {
            return Err(Error::invalid("人工接口不能伪造自动识别或文件指纹验证"));
        }
        let mut unique = std::collections::HashSet::new();
        for link in &self.links {
            if !unique.insert((&link.media, &link.reference)) {
                return Err(Error::invalid("批量修改包含重复关联"));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CatalogReceipt {
    pub request: String,
    pub change: BindingChange,
    pub links: Vec<erika_catalog::ProviderLink>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PortableBinding {
    pub file: crate::portable::FileKey,
    pub binding: FileBinding,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CatalogSnapshot {
    pub nodes: Vec<MediaNode>,
    pub links: Vec<erika_catalog::ProviderLink>,
    pub bindings: Vec<PortableBinding>,
    pub danmaku: Vec<(crate::portable::FileKey, erika_catalog::DanmakuBinding)>,
    pub watched: Vec<Watched>,
    pub marks: Vec<MediaMark>,
}
