//! Shared image selection: all frontends receive the same asset, not a
//! different site's poster. Image votes never become a show's rating.
use crate::{Error, Provider, ProviderRef, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtworkKind {
    Poster,
    Backdrop,
    Logo,
    Still,
}
impl ArtworkKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Poster => "poster_sizes",
            Self::Backdrop => "backdrop_sizes",
            Self::Logo => "logo_sizes",
            Self::Still => "still_sizes",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub owner: ProviderRef,
    pub kind: ArtworkKind,
    pub file_path: String,
    pub language: Option<String>,
    pub width: u32,
    pub height: u32,
    pub aspect_ratio: f64,
    #[serde(default)]
    pub image_vote: f64,
    #[serde(default)]
    pub image_votes: u64,
}
impl Asset {
    /// The gateway configuration chooses a rendition; an uncached configuration
    /// may use TMDB's original, never another provider's picture.
    pub fn url(&self, configuration: Option<&ImageConfiguration>, width: u32) -> Result<String> {
        self.validate()?;
        match configuration {
            Some(configuration) => configuration.url(self, width),
            None => Ok(format!(
                "https://image.tmdb.org/t/p/original{}",
                self.file_path
            )),
        }
    }
    pub fn validate(&self) -> Result<()> {
        self.owner.validate()?;
        if self.owner.provider != Provider::Tmdb
            || !valid_path(&self.file_path)
            || !self.aspect_ratio.is_finite()
            || self.aspect_ratio < 0.0
            || !self.image_vote.is_finite()
        {
            return Err(Error::invalid("图片资源不正确"));
        }
        Ok(())
    }
}
pub fn valid_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 256
        && path.len() > 2
        && path[1..]
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
        && !path.contains("..")
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Artwork {
    pub poster: Option<Asset>,
    pub backdrop: Option<Asset>,
    pub logo: Option<Asset>,
    pub still: Option<Asset>,
}
impl Artwork {
    /// Missing fields are not erasures. Call only for the same media node.
    pub fn refresh(&mut self, fresh: Self) {
        if fresh.poster.is_some() {
            self.poster = fresh.poster;
        }
        if fresh.backdrop.is_some() {
            self.backdrop = fresh.backdrop;
        }
        if fresh.logo.is_some() {
            self.logo = fresh.logo;
        }
        if fresh.still.is_some() {
            self.still = fresh.still;
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageConfiguration {
    pub secure_base_url: String,
    pub sizes: BTreeMap<String, Vec<String>>,
}
impl ImageConfiguration {
    pub fn parse(body: &serde_json::Value) -> Result<Self> {
        let body = body.get("images").unwrap_or(body);
        let base = body["secure_base_url"]
            .as_str()
            .ok_or_else(|| Error::invalid("缺少图片服务配置"))?;
        let parsed = url::Url::parse(base).map_err(|_| Error::invalid("图片服务地址不正确"))?;
        // Configuration is data, not permission to fetch arbitrary hosts.
        if parsed.scheme() != "https"
            || parsed.host_str() != Some("image.tmdb.org")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.port().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != "/t/p/"
        {
            return Err(Error::invalid("图片服务地址不受支持"));
        }
        let mut sizes = BTreeMap::new();
        for key in [
            "poster_sizes",
            "backdrop_sizes",
            "logo_sizes",
            "still_sizes",
        ] {
            let values = body[key]
                .as_array()
                .ok_or_else(|| Error::invalid("图片尺寸配置不完整"))?;
            let values: Vec<String> = values
                .iter()
                .filter_map(|v| v.as_str())
                .filter(|v| {
                    *v == "original"
                        || v.strip_prefix('w')
                            .and_then(|n| n.parse::<u32>().ok())
                            .is_some_and(|n| n > 0 && n <= 16384)
                        || v.strip_prefix('h')
                            .and_then(|n| n.parse::<u32>().ok())
                            .is_some_and(|n| n > 0 && n <= 16384)
                })
                .map(String::from)
                .collect();
            if values.is_empty() {
                return Err(Error::invalid("图片没有有效尺寸"));
            }
            sizes.insert(key.into(), values);
        }
        Ok(Self {
            secure_base_url: base.to_string(),
            sizes,
        })
    }
    pub fn url(&self, asset: &Asset, target_width: u32) -> Result<String> {
        asset.validate()?;
        let sizes = self
            .sizes
            .get(asset.kind.key())
            .ok_or_else(|| Error::invalid("未知图片尺寸"))?;
        let mut widths: Vec<(u32, &str)> = sizes
            .iter()
            .filter_map(|v| {
                v.strip_prefix('w')
                    .and_then(|n| n.parse().ok())
                    .map(|n| (n, v.as_str()))
            })
            .collect();
        widths.sort_unstable();
        let size = if asset.file_path.ends_with(".svg") {
            sizes
                .iter()
                .find(|v| v.as_str() == "original")
                .map(|v| v.as_str())
        } else {
            widths
                .iter()
                .find(|(w, _)| *w >= target_width)
                .or_else(|| widths.last())
                .map(|(_, s)| *s)
                .or_else(|| {
                    sizes
                        .iter()
                        .find(|v| v.as_str() == "original")
                        .map(|v| v.as_str())
                })
        }
        .ok_or_else(|| Error::invalid("图片没有可用尺寸"))?;
        Ok(format!("{}{size}{}", self.secure_base_url, asset.file_path))
    }
}

pub fn languages(target: &str, original: &str) -> Vec<String> {
    let mut result = Vec::new();
    for value in [target, original, "en", "null"] {
        let value = value
            .split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if (value == "null" || value.len() == 2 && value.bytes().all(|c| c.is_ascii_alphabetic()))
            && !result.contains(&value)
        {
            result.push(value);
        }
    }
    result
}

pub fn choose(
    assets: &[Asset],
    kind: ArtworkKind,
    preferred: &[String],
    target_width: u32,
) -> Option<Asset> {
    fn rank(asset: &Asset, preferred: &[String]) -> usize {
        if asset.kind == ArtworkKind::Backdrop && asset.language.is_none() {
            return 0;
        }
        let tag = asset.language.as_deref().unwrap_or("null");
        preferred
            .iter()
            .position(|s| s == tag)
            .unwrap_or(preferred.len())
            + 1
    }
    let mut valid: Vec<&Asset> = assets
        .iter()
        .filter(|a| a.kind == kind && a.validate().is_ok())
        .collect();
    valid.sort_by(|a, b| {
        rank(a, preferred)
            .cmp(&rank(b, preferred))
            .then_with(|| (b.width >= target_width).cmp(&(a.width >= target_width)))
            .then_with(|| b.image_vote.total_cmp(&a.image_vote))
            .then_with(|| b.image_votes.cmp(&a.image_votes))
            .then_with(|| b.width.cmp(&a.width))
            .then_with(|| a.file_path.cmp(&b.file_path))
    });
    valid.first().map(|v| (*v).clone())
}

pub fn parse_images(owner: &ProviderRef, body: &serde_json::Value) -> Result<Vec<Asset>> {
    owner.validate()?;
    if body["id"].as_u64().map(|id| id.to_string()).as_deref() != Some(owner.id.as_str()) {
        return Err(Error::invalid("图片所属作品与请求不一致"));
    }
    let mut result = Vec::new();
    for (field, kind) in [
        ("posters", ArtworkKind::Poster),
        ("backdrops", ArtworkKind::Backdrop),
        ("logos", ArtworkKind::Logo),
        ("stills", ArtworkKind::Still),
    ] {
        if let Some(rows) = body.get(field) {
            let rows = rows
                .as_array()
                .ok_or_else(|| Error::invalid("图片列表不正确"))?;
            if rows.len() > 10000 {
                return Err(Error::invalid("图片列表太长"));
            }
            for row in rows {
                let asset = Asset {
                    owner: owner.clone(),
                    kind,
                    file_path: row["file_path"].as_str().unwrap_or_default().into(),
                    language: row["iso_639_1"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(String::from),
                    width: row["width"]
                        .as_u64()
                        .and_then(|v| v.try_into().ok())
                        .unwrap_or(0),
                    height: row["height"]
                        .as_u64()
                        .and_then(|v| v.try_into().ok())
                        .unwrap_or(0),
                    aspect_ratio: row["aspect_ratio"].as_f64().unwrap_or(0.0),
                    image_vote: row["vote_average"].as_f64().unwrap_or(0.0),
                    image_votes: row["vote_count"].as_u64().unwrap_or(0),
                };
                if asset.validate().is_ok() {
                    result.push(asset);
                }
            }
        }
    }
    Ok(result)
}

pub fn path_asset(owner: &ProviderRef, kind: ArtworkKind, path: &str) -> Option<Asset> {
    valid_path(path).then(|| Asset {
        owner: owner.clone(),
        kind,
        file_path: path.into(),
        language: None,
        width: 0,
        height: 0,
        aspect_ratio: 0.0,
        image_vote: 0.0,
        image_votes: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Kind;
    use serde_json::json;
    #[test]
    fn posters_and_logos_are_separate_and_null_backgrounds_survive() {
        let owner = ProviderRef::tmdb(Kind::Tv, 12).unwrap();
        let images = parse_images(
            &owner,
            &json!({"id":12,"posters":[{"file_path":"/p.jpg","iso_639_1":"zh","width":500}],
            "backdrops":[{"file_path":"/b.jpg","iso_639_1":null,"width":1920}],
            "logos":[{"file_path":"/l.png","iso_639_1":"ja","width":1000}]}),
        )
        .unwrap();
        let prefs = languages("zh-CN", "ja");
        assert_eq!(prefs, ["zh", "ja", "en", "null"]);
        assert_eq!(
            choose(&images, ArtworkKind::Logo, &prefs, 800)
                .unwrap()
                .file_path,
            "/l.png"
        );
        assert_eq!(
            choose(&images, ArtworkKind::Backdrop, &prefs, 1920)
                .unwrap()
                .file_path,
            "/b.jpg"
        );
        assert!(choose(&images, ArtworkKind::Still, &prefs, 800).is_none());
        assert!(!valid_path("//example.com/a.png"));
        assert!(!valid_path("/../private"));
    }
    #[test]
    fn movie_and_tv_artwork_owners_do_not_collide() {
        let owner = ProviderRef::tmdb(Kind::Movie, 9).unwrap();
        assert!(parse_images(&owner, &json!({"id":10})).is_err());
        let mut art = Artwork {
            poster: path_asset(&owner, ArtworkKind::Poster, "/a.jpg"),
            ..Default::default()
        };
        art.refresh(Artwork::default());
        assert!(art.poster.is_some());
    }
}
