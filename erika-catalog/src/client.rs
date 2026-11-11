//! Read-only metadata transport. Native requests run off the engine thread;
//! browser requests use fetch rather than a blocking native HTTP shim.
use crate::{Kind, ProviderRef, artwork, tmdb};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidPayload,
    NotFound,
    Ambiguous,
    Unauthorized,
    RateLimited,
    Unavailable,
    Conflict,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    pub retry_after: Option<u64>,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn invalid(message: &str) -> Self {
        Self {
            code: ErrorCode::InvalidPayload,
            message: message.into(),
            retry_after: None,
        }
    }
    pub fn unavailable() -> Self {
        Self {
            code: ErrorCode::Unavailable,
            message: "暂时连不上媒体资料服务".into(),
            retry_after: None,
        }
    }
    pub fn conflict(message: &str) -> Self {
        Self {
            code: ErrorCode::Conflict,
            message: message.into(),
            retry_after: None,
        }
    }
    fn status(status: u16, retry_after: Option<u64>) -> Self {
        let (code, message) = match status {
            401 | 403 => (ErrorCode::Unauthorized, "媒体资料服务暂不可访问"),
            404 => (ErrorCode::NotFound, "没有找到这条媒体资料"),
            429 => (ErrorCode::RateLimited, "媒体资料请求过于频繁，稍后重试"),
            _ => (ErrorCode::Unavailable, "媒体资料服务暂不可用"),
        };
        Self {
            code,
            message: message.into(),
            retry_after,
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Debug)]
pub struct Client {
    base: url::Url,
    pub language: String,
}
impl Client {
    /// Same built-in host as Erika's recognition service; no user account or
    /// client-side TMDB key is required. Deployment exposes this read-only route.
    pub fn built_in(language: &str) -> Result<Self> {
        Self::new("http://43.142.85.190/erika/metadata/v1/", language)
    }
    /// base is the application's trusted gateway root, ending in /metadata/v1/.
    pub fn new(base: &str, language: &str) -> Result<Self> {
        let base = url::Url::parse(&format!("{}/", base.trim_end_matches('/')))
            .map_err(|_| Error::invalid("元数据服务地址不正确"))?;
        if !matches!(base.scheme(), "https" | "http")
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(Error::invalid("元数据服务地址不正确"));
        }
        if language.len() > 16
            || !language
                .bytes()
                .all(|c| c.is_ascii_alphabetic() || c == b'-')
        {
            return Err(Error::invalid("资料语言不正确"));
        }
        Ok(Self {
            base,
            language: language.into(),
        })
    }
    pub async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        if path.starts_with('/')
            || path.contains("..")
            || path.contains(":")
            || path.contains('?')
            || path.contains('#')
        {
            return Err(Error::invalid("元数据请求路径不正确"));
        }
        let mut url = self
            .base
            .join(path)
            .map_err(|_| Error::invalid("元数据请求路径不正确"))?;
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(key, value)| (*key, value.as_str())));
        let body = fetch(url.as_str()).await?;
        if body["schema_version"].as_u64() != Some(1) {
            return Err(Error::invalid("媒体资料服务版本不兼容"));
        }
        body.get("data")
            .cloned()
            .ok_or_else(|| Error::invalid("媒体资料响应缺少内容"))
    }
    pub async fn search(
        &self,
        kind: Kind,
        query: &str,
        year: Option<u16>,
        page: u32,
    ) -> Result<tmdb::SearchPage> {
        if !matches!(kind, Kind::Tv | Kind::Movie)
            || query.trim().is_empty()
            || query.len() > 512
            || !(1..=500).contains(&page)
        {
            return Err(Error::invalid("搜索参数不正确"));
        }
        let mut params = vec![
            ("kind", kind.as_str().into()),
            ("query", query.trim().into()),
            ("language", self.language.clone()),
            ("page", page.to_string()),
        ];
        if let Some(year) = year {
            params.push(("year", year.to_string()));
        }
        tmdb::parse_search(kind, &self.get("search", &params).await?)
    }
    pub async fn work(&self, reference: &ProviderRef) -> Result<tmdb::Work> {
        reference.validate()?;
        if reference.provider != crate::Provider::Tmdb
            || !matches!(reference.kind, Kind::Tv | Kind::Movie)
        {
            return Err(Error::invalid("需要TMDB作品标识"));
        }
        let body = self
            .get(
                &format!("tmdb/{}/{}", reference.kind.as_str(), reference.id),
                &[("language", self.language.clone())],
            )
            .await?;
        tmdb::parse_work(
            reference.kind,
            reference
                .id
                .parse()
                .map_err(|_| Error::invalid("作品ID不正确"))?,
            &body,
        )
    }
    pub async fn season(&self, work: &tmdb::Work, number: u32) -> Result<tmdb::Season> {
        let reference = work
            .node
            .reference
            .as_ref()
            .ok_or_else(|| Error::invalid("作品没有TMDB身份"))?;
        if reference.kind != Kind::Tv {
            return Err(Error::invalid("电影没有季"));
        }
        let body = self
            .get(
                &format!("tmdb/tv/{}/seasons/{number}", reference.id),
                &[("language", self.language.clone())],
            )
            .await?;
        tmdb::parse_season(work, number, &body)
    }
    pub async fn images(&self, work: &tmdb::Work) -> Result<Vec<artwork::Asset>> {
        let reference = work
            .node
            .reference
            .as_ref()
            .ok_or_else(|| Error::invalid("作品没有TMDB身份"))?;
        let path = format!("tmdb/{}/{}/images", reference.kind.as_str(), reference.id);
        let languages = artwork::languages(&self.language, &work.node.original_language).join(",");
        let body = self
            .get(
                &path,
                &[
                    ("language", self.language.clone()),
                    ("include_image_language", languages),
                ],
            )
            .await?;
        let images = artwork::parse_images(reference, &body)?;
        if images.is_empty() {
            return artwork::parse_images(reference, &self.get(&path, &[]).await?);
        }
        Ok(images)
    }
    pub async fn image_configuration(&self) -> Result<artwork::ImageConfiguration> {
        artwork::ImageConfiguration::parse(&self.get("tmdb/images/configuration", &[]).await?)
    }
    pub async fn with_artwork(&self, mut work: tmdb::Work) -> tmdb::Work {
        if let Ok(images) = self.images(&work).await {
            let preferred = artwork::languages(&self.language, &work.node.original_language);
            work.node.artwork.refresh(artwork::Artwork {
                poster: artwork::choose(&images, artwork::ArtworkKind::Poster, &preferred, 780),
                backdrop: artwork::choose(
                    &images,
                    artwork::ArtworkKind::Backdrop,
                    &preferred,
                    1920,
                ),
                logo: artwork::choose(&images, artwork::ArtworkKind::Logo, &preferred, 1000),
                still: None,
            });
        }
        work
    }
}

const MAX_RESPONSE: usize = 8 * 1024 * 1024;
async fn fetch(url: &str) -> Result<Value> {
    let response = crate::http::get(
        url,
        &[("Accept", "application/json")],
        Duration::from_secs(20),
        MAX_RESPONSE,
        false,
    )
    .await?;
    if response.status != 200 {
        return Err(Error::status(response.status, response.retry_after));
    }
    response.json()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn(work: impl std::future::Future<Output = ()> + Send + 'static) {
    std::thread::spawn(move || futures_lite::future::block_on(work));
}
#[cfg(target_arch = "wasm32")]
pub fn spawn(work: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(work);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transport_errors_are_not_empty_catalogues() {
        assert_eq!(Error::status(429, Some(60)).code, ErrorCode::RateLimited);
        assert_eq!(Error::status(503, None).code, ErrorCode::Unavailable);
        assert_eq!(Error::status(404, None).code, ErrorCode::NotFound);
        assert!(Client::new("https://user:secret@example.com", "zh-CN").is_err());
        assert!(Client::new("file:///etc/passwd", "zh-CN").is_err());
    }
}
