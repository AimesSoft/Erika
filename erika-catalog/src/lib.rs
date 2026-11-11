//! Media identity is independent of the services used to recognise a file,
//! fetch its artwork, read danmaku, or synchronise an account. No encoded IDs.

pub mod artwork;
pub mod client;
pub mod http;
pub mod names;
pub mod playback;
pub mod resolve;
pub mod source;
pub mod tmdb;

pub use client::{Client, Error, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MediaId(pub String);

impl MediaId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
    pub fn parse(value: &str) -> Result<Self> {
        Uuid::parse_str(value)
            .map(|v| Self(v.to_string()))
            .map_err(|_| Error::invalid("无效的媒体标识"))
    }
    pub fn for_ref(reference: &ProviderRef) -> Self {
        Self::stable(&serde_json::to_string(reference).expect("reference serializes"))
    }
    fn stable(value: &str) -> Self {
        Self(
            Uuid::new_v5(
                &Uuid::NAMESPACE_URL,
                format!("urn:erika:media:{value}").as_bytes(),
            )
            .to_string(),
        )
    }
}
impl Default for MediaId {
    fn default() -> Self {
        Self::new()
    }
}
impl std::fmt::Display for MediaId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Tmdb,
    Dandanplay,
    Bangumi,
    Misaka,
    Jellyfin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Tv,
    Movie,
    Season,
    Episode,
    Subject,
    Item,
}
impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tv => "tv",
            Self::Movie => "movie",
            Self::Season => "season",
            Self::Episode => "episode",
            Self::Subject => "subject",
            Self::Item => "item",
        }
    }
    pub fn playable(self) -> bool {
        matches!(self, Self::Movie | Self::Episode)
    }
    pub fn work(self) -> bool {
        matches!(self, Self::Tv | Self::Movie)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProviderRef {
    pub provider: Provider,
    #[serde(default)]
    pub scope: String,
    pub kind: Kind,
    pub id: String,
}
impl ProviderRef {
    pub fn tmdb(kind: Kind, id: u64) -> Result<Self> {
        let value = Self {
            provider: Provider::Tmdb,
            scope: String::new(),
            kind,
            id: id.to_string(),
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty()
            || self.id.len() > 256
            || self.scope.len() > 256
            || self.id.chars().any(char::is_control)
            || self.scope.chars().any(char::is_control)
        {
            return Err(Error::invalid("外部媒体标识不正确"));
        }
        if matches!(
            self.provider,
            Provider::Tmdb | Provider::Bangumi | Provider::Dandanplay
        ) && !self.scope.is_empty()
        {
            return Err(Error::invalid("公共提供方不能带服务器实例"));
        }
        if matches!(self.provider, Provider::Misaka | Provider::Jellyfin) && self.scope.is_empty() {
            return Err(Error::invalid("远程标识缺少服务器实例"));
        }
        if self.provider == Provider::Tmdb
            && (!matches!(
                self.kind,
                Kind::Tv | Kind::Movie | Kind::Season | Kind::Episode
            ) || self.id.parse::<u64>().ok().filter(|id| *id > 0).is_none())
        {
            return Err(Error::invalid("TMDB标识必须有正确类型和正整数ID"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaNode {
    pub id: MediaId,
    pub kind: Kind,
    pub parent: Option<MediaId>,
    /// Root work; equal to id for movies/TV.
    pub root: MediaId,
    pub season_number: Option<u32>,
    pub episode_number: Option<u32>,
    #[serde(default)]
    pub original_label: String,
    pub title: String,
    #[serde(default)]
    pub original_title: String,
    #[serde(default)]
    pub overview: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub original_language: String,
    #[serde(default)]
    pub runtime_minutes: Option<u32>,
    pub reference: Option<ProviderRef>,
    #[serde(default)]
    pub artwork: artwork::Artwork,
    #[serde(default)]
    pub revision: u64,
}
impl MediaNode {
    /// Keep cached details when a partial refresh supplies only a title.
    pub fn retain_metadata(&mut self, previous: &Self) {
        if self.reference != previous.reference { return; }
        if self.overview.is_empty() {
            self.overview.clone_from(&previous.overview);
        }
        if self.original_title.is_empty() {
            self.original_title.clone_from(&previous.original_title);
        }
        if self.original_language.is_empty() {
            self.original_language
                .clone_from(&previous.original_language);
        }
        if self.date.is_empty() {
            self.date.clone_from(&previous.date);
        }
        let mut artwork = previous.artwork.clone();
        artwork.refresh(self.artwork.clone());
        self.artwork = artwork;
    }

    pub fn validate_artwork(&self) -> Result<()> {
        let owner = self.reference.as_ref();
        for asset in [
            &self.artwork.poster,
            &self.artwork.backdrop,
            &self.artwork.logo,
            &self.artwork.still,
        ]
        .into_iter()
        .flatten()
        {
            asset.validate()?;
            if Some(&asset.owner) != owner {
                return Err(Error::invalid("图片属于另一媒体"));
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        MediaId::parse(&self.id.0)?;
        MediaId::parse(&self.root.0)?;
        if self.title.trim().is_empty()
            || self.title.len() > 8192
            || self.overview.len() > 128 * 1024
        {
            return Err(Error::invalid("媒体标题或简介不正确"));
        }
        if self.kind.work() {
            if self.parent.is_some()
                || self.root != self.id
                || self.season_number.is_some()
                || self.episode_number.is_some()
            {
                return Err(Error::invalid("作品不能带季集位置或父节点"));
            }
        } else if let Some(parent) = &self.parent {
            MediaId::parse(&parent.0)?;
            if parent == &self.id || self.root == self.id {
                return Err(Error::invalid("媒体层级不能成环"));
            }
        } else {
            return Err(Error::invalid("分集或季缺少父节点"));
        }
        match self.kind {
            Kind::Season if self.season_number.is_none() || self.episode_number.is_some() => {
                return Err(Error::invalid("季位置不正确"));
            }
            Kind::Episode if self.season_number.is_none() || self.episode_number.is_none() => {
                return Err(Error::invalid("分集位置不正确"));
            }
            Kind::Subject | Kind::Item => return Err(Error::invalid("提供方条目不是规范媒体节点")),
            _ => {}
        }
        if matches!(
            self.kind,
            Kind::Tv | Kind::Movie | Kind::Season | Kind::Episode
        ) && self.reference.is_none()
        {
            return Err(Error::invalid("规范媒体缺少 TMDB 身份"));
        }
        if let Some(reference) = &self.reference {
            reference.validate()?;
            if reference.provider != Provider::Tmdb || reference.kind != self.kind {
                return Err(Error::invalid("规范引用的提供方或类型不正确"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    Exact,
    PartOf,
    Contains,
    Related,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderLink {
    pub media: MediaId,
    pub reference: ProviderRef,
    pub relation: Relation,
    #[serde(default)]
    pub coverage: Vec<MediaId>,
    pub evidence: String,
    pub confirmed: bool,
    pub manual: bool,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Auto,
    Manual,
    Blocked,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BindingItem {
    pub media: MediaId,
    pub start_seconds: Option<f64>,
    pub end_seconds: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileBinding {
    pub file: i64,
    pub decision: Decision,
    pub items: Vec<BindingItem>,
    pub evidence: String,
    pub revision: u64,
}
impl FileBinding {
    pub fn validate(&self) -> Result<()> {
        if self.file <= 0
            || self.items.len() > 1000
            || (self.decision == Decision::Blocked && !self.items.is_empty())
        {
            return Err(Error::invalid("文件绑定不正确"));
        }
        let mut seen = std::collections::HashSet::new();
        for item in &self.items {
            MediaId::parse(&item.media.0)?;
            if !seen.insert(&item.media) {
                return Err(Error::invalid("同一文件不能重复绑定同一分集"));
            }
            if item
                .start_seconds
                .is_some_and(|v| !v.is_finite() || v < 0.0)
                || item
                    .end_seconds
                    .is_some_and(|v| !v.is_finite() || v <= item.start_seconds.unwrap_or(0.0))
            {
                return Err(Error::invalid("文件章节时间不正确"));
            }
        }
        if self.items.len() > 1 && self.items.iter().all(|i| i.start_seconds.is_some()) {
            let mut timed: Vec<_> = self.items.iter().collect();
            timed.sort_by(|a, b| {
                a.start_seconds
                    .unwrap()
                    .total_cmp(&b.start_seconds.unwrap())
            });
            for pair in timed.windows(2) {
                let next = pair[1].start_seconds.unwrap();
                if pair[0].start_seconds == pair[1].start_seconds
                    || pair[0].end_seconds.is_some_and(|end| end > next)
                {
                    return Err(Error::invalid("文件章节时间不能重叠"));
                }
            }
        }
        Ok(())
    }

    /// A combined file without complete chapter starts has no guessed current
    /// episode. For timed files, a following start bounds an open-ended chapter.
    pub fn active_item(&self, seconds: f64) -> Option<&BindingItem> {
        if !seconds.is_finite() || seconds < 0.0 || !self.has_item_positions() {
            return None;
        }
        self.items.iter().find(|item| {
            seconds >= item.start_seconds.unwrap_or(0.0)
                && self.item_end(item).is_none_or(|end| seconds < end)
        })
    }

    /// Whether every item can be addressed independently in this file.
    pub fn has_item_positions(&self) -> bool {
        self.items.len() <= 1 || self.items.iter().all(|item| item.start_seconds.is_some())
    }

    /// At EOF the last displayed instant belongs to the interval immediately
    /// before duration, not to a hypothetical episode starting after the file.
    pub fn playing_item(&self, seconds: f64, duration: f64) -> Option<&BindingItem> {
        if !seconds.is_finite() || seconds < 0.0 {
            return None;
        }
        let at = if duration.is_finite() && duration > 0.0 && seconds >= duration {
            duration.next_down()
        } else {
            seconds
        };
        self.active_item(at)
    }

    pub fn item_end(&self, item: &BindingItem) -> Option<f64> {
        item.end_seconds.or_else(|| {
            let start = item.start_seconds?;
            self.items
                .iter()
                .filter_map(|i| i.start_seconds)
                .filter(|s| *s > start)
                .min_by(f64::total_cmp)
        })
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DanmakuBinding {
    pub file: i64,
    pub reference: ProviderRef,
    pub media: Option<MediaId>,
    pub manual: bool,
    pub fingerprint_verified: bool,
    pub shift_seconds: f64,
    pub user_shift_seconds: f64,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Watched {
    pub media: MediaId,
    pub watched: bool,
    pub manual: bool,
    pub updated_at: i64,
    pub revision: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_keep_providers_and_entity_types_apart() {
        let tv = ProviderRef::tmdb(Kind::Tv, 42).unwrap();
        let movie = ProviderRef::tmdb(Kind::Movie, 42).unwrap();
        assert_ne!(MediaId::for_ref(&tv), MediaId::for_ref(&movie));
        assert_eq!(MediaId::for_ref(&tv), MediaId::for_ref(&tv));
        assert!(ProviderRef::tmdb(Kind::Tv, 0).is_err());
        assert!(MediaId::parse("42").is_err());
    }
    fn combined() -> FileBinding {
        FileBinding {
            file: 1,
            decision: Decision::Manual,
            evidence: "chapters".into(),
            revision: 0,
            items: [Some(0.0), Some(60.0), Some(120.0)]
                .into_iter()
                .map(|start| BindingItem {
                    media: MediaId::new(),
                    start_seconds: start,
                    end_seconds: None,
                })
                .collect(),
        }
    }
    #[test]
    fn chapter_boundaries_are_shared_and_do_not_depend_on_item_order() {
        let mut binding = combined();
        let second = binding.items[1].media.clone();
        let last = binding.items[2].media.clone();
        binding.items[2].end_seconds = Some(180.0);
        assert_eq!(binding.item_end(&binding.items[0]), Some(60.0));
        binding.items.reverse();
        assert!(binding.validate().is_ok());
        assert_eq!(binding.active_item(60.0).unwrap().media, second);
        assert!(binding.active_item(180.0).is_none());
        assert_eq!(binding.playing_item(180.0, 180.0).unwrap().media, last);
        assert!(binding.playing_item(200.0, 200.0).is_none());
        for time in [f64::NAN, f64::INFINITY, -1.0] {
            assert!(binding.playing_item(time, 180.0).is_none());
        }
    }
    #[test]
    fn incomplete_chapters_and_gaps_do_not_guess_an_episode() {
        let mut binding = combined();
        binding.items[1].start_seconds = None;
        assert!(binding.validate().is_ok());
        assert!(!binding.has_item_positions());
        assert!(binding.active_item(10.0).is_none());
        assert!(binding.active_item(140.0).is_none());
        binding.items[1].start_seconds = Some(60.0);
        binding.items[0].end_seconds = Some(40.0);
        assert!(binding.validate().is_ok());
        assert!(binding.active_item(50.0).is_none());
        binding.items[0].end_seconds = Some(61.0);
        assert!(binding.validate().is_err());
        binding.items[0].end_seconds = None;
        binding.items[1].start_seconds = Some(0.0);
        assert!(binding.validate().is_err());
    }
    #[test]
    fn bindings_reject_false_chapters_and_duplicate_episodes() {
        let item = BindingItem {
            media: MediaId::new(),
            start_seconds: None,
            end_seconds: None,
        };
        let mut binding = FileBinding {
            file: 1,
            decision: Decision::Auto,
            items: vec![item.clone()],
            evidence: String::new(),
            revision: 0,
        };
        assert!(binding.validate().is_ok());
        binding.items.push(item);
        assert!(binding.validate().is_err());
        binding.items.truncate(1);
        binding.items[0].start_seconds = Some(f64::NAN);
        assert!(binding.validate().is_err());
    }
}
