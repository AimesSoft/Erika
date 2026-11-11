//! Public market resources only. A mirror is a transport candidate, never
//! the resource's identity or the base of its relative links.
use super::{Pack, Resource};
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use url::Url;

const LIMIT: u64 = 8 << 20;
const MIRRORS: [&str; 3] = [
    "https://gh-proxy.com/",
    "https://ghfast.top/",
    "https://ghproxy.net/",
];

pub(super) fn eligible(address: &str) -> bool {
    // Reject encoded separators/dot segments before URL normalization.
    if address.contains(['%', '\\', '?', '#']) {
        return false;
    }
    let Ok(url) = Url::parse(address) else {
        return false;
    };
    if url.scheme() != "https"
        || url.host_str() != Some("raw.githubusercontent.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return false;
    }
    let parts: Vec<_> = url.path().split('/').collect();
    let original_path = address.strip_prefix("https://raw.githubusercontent.com/");
    original_path.is_some_and(|path| !path.split('/').any(|p| matches!(p, "." | ".." | "")))
        && parts.len() >= 5
        && parts[1] == "AimesSoft"
        && matches!(parts[2], "Nipaplay-plugins" | "erika_plugins")
}

fn candidates(address: &str) -> Vec<String> {
    let mut urls = vec![address.to_string()];
    if eligible(address) {
        urls.extend(MIRRORS.map(|mirror| format!("{mirror}{address}")));
    }
    urls
}

fn fetch_once(address: &str) -> Result<Vec<u8>, String> {
    // No business cookies or authentication headers. Defaults retain system
    // proxy discovery; redirects carry no application credentials.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .https_only(address.starts_with("https://"))
        .build()
        .into();
    let mut response = agent.get(address).call().map_err(|e| e.to_string())?;
    if response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(';').next().is_some_and(|mime| {
                matches!(
                    mime.trim().to_ascii_lowercase().as_str(),
                    "text/html" | "application/xhtml+xml"
                )
            })
        })
    {
        return Err("服务器返回了 HTML 页面而不是资源".into());
    }
    response
        .body_mut()
        .with_config()
        .limit(LIMIT)
        .read_to_vec()
        .map_err(|e| e.to_string())
}

pub(super) fn read(address: &str, resource: Resource) -> Result<Vec<u8>, String> {
    read_with(address, resource, |candidate| {
        if candidate.starts_with("https://") || candidate.starts_with("http://") {
            fetch_once(candidate)
        } else {
            super::get(candidate)
        }
    })
}

fn read_with(
    address: &str,
    resource: Resource,
    mut fetch: impl FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    let mut failures = Vec::new();
    for (attempt, candidate) in candidates(address).iter().enumerate() {
        let result = fetch(candidate).and_then(|bytes| {
            if bytes.len() as u64 > LIMIT {
                return Err("文件超过 8 MiB".into());
            }
            validate(resource, &bytes)?;
            Ok(bytes)
        });
        match result {
            Ok(bytes) => return Ok(bytes),
            Err(error) => {
                let source = if attempt == 0 {
                    "原站"
                } else {
                    MIRRORS[attempt - 1]
                };
                failures.push(format!("{source}：{error}"));
            }
        }
    }
    // The original address already appears in the market's caller error;
    // never copy potentially sensitive non-market URLs into diagnostics.
    Err(format!(
        "{}读取失败（{}）",
        resource.label(),
        failures.join("；")
    ))
}

fn text(bytes: &[u8]) -> Result<&str, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "不是 UTF-8 文本".to_string())?;
    let start = text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .to_ascii_lowercase();
    if start.starts_with("<!doctype html") || start.starts_with("<html") {
        return Err("服务器返回了 HTML 页面而不是资源".into());
    }
    Ok(text)
}

fn validate(resource: Resource, bytes: &[u8]) -> Result<(), String> {
    match resource {
        Resource::Index => {
            let value: serde_json::Value =
                serde_json::from_slice(bytes).map_err(|_| "插件索引不是有效 JSON".to_string())?;
            if !value
                .get("plugins")
                .is_some_and(serde_json::Value::is_array)
            {
                return Err("插件索引里没有 plugins 数组".into());
            }
        }
        Resource::Pack => {
            let value =
                serde_json::from_slice(bytes).map_err(|_| "pack.json 不是有效 JSON".to_string())?;
            Pack::from_pack(&value)?;
        }
        Resource::Script => {
            if text(bytes)?.trim().is_empty() {
                return Err("插件脚本为空".into());
            }
        }
        Resource::Readme => {
            text(bytes)?;
        }
        Resource::Image => {
            image_format(bytes)?;
        }
    }
    Ok(())
}

fn image_format(bytes: &[u8]) -> Result<&'static str, String> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        if let Ok(doc) = roxmltree::Document::parse(text) {
            let root = doc.root_element();
            if root.tag_name().name() == "svg"
                && root.tag_name().namespace() == Some("http://www.w3.org/2000/svg")
            {
                return Ok("svg");
            }
        }
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "图片格式无效".to_string())?;
    let format = reader.format().ok_or_else(|| "图片格式无效".to_string())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(64 << 20);
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    reader.limits(limits);
    reader.decode().map_err(|_| "图片内容无效".to_string())?;
    match format {
        image::ImageFormat::Png => Ok("png"),
        image::ImageFormat::Jpeg => Ok("jpg"),
        image::ImageFormat::WebP => Ok("webp"),
        image::ImageFormat::Gif => Ok("gif"),
        image::ImageFormat::Bmp => Ok("bmp"),
        image::ImageFormat::Ico => Ok("ico"),
        _ => Err("图片格式无效".into()),
    }
}

/// Run from the market/README worker, never the UI thread. Only approved
/// remote pictures are localized; other images keep their normal loader.
pub(super) fn cached_image(address: &str) -> Option<String> {
    if !eligible(address) {
        return None;
    }
    let root = bern::storage::cache_dir()?.join("plugin-market");
    cache_image_with(&root, address, |url| read(url, Resource::Image))
        .map(|path| bern::media::MediaSource::file(path).url)
}

fn cache_image_with(
    root: &Path,
    address: &str,
    fetch: impl FnOnce(&str) -> Result<Vec<u8>, String>,
) -> Option<PathBuf> {
    let key = format!("{:x}", Sha256::digest(address.as_bytes()));
    let bytes = match fetch(address) {
        Ok(bytes) => bytes,
        Err(_) => {
            // A validated older copy remains useful during an outage.
            return std::fs::read_dir(root)
                .ok()?
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(&format!("{key}-"))
                })
                .filter_map(|entry| {
                    let path = entry.path();
                    let metadata = entry.metadata().ok()?;
                    if !metadata.is_file() || metadata.len() > LIMIT {
                        return None;
                    }
                    let bytes = std::fs::read(&path).ok()?;
                    if bytes.len() as u64 > LIMIT {
                        return None;
                    }
                    image_format(&bytes).ok()?;
                    Some(path)
                })
                .next();
        }
    };
    if bytes.len() as u64 > LIMIT {
        return None;
    }
    let extension = image_format(&bytes).ok()?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    std::fs::create_dir_all(root).ok()?;
    let path = root.join(format!("{key}-{digest}.{extension}"));
    if path.is_file() {
        if std::fs::metadata(&path).ok()?.len() <= LIMIT
            && std::fs::read(&path).ok().as_deref() == Some(bytes.as_slice())
        {
            return Some(path);
        }
        // A damaged cache entry must not shadow freshly validated bytes.
        std::fs::remove_file(&path).ok()?;
    }
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let tmp = root.join(format!(
        ".{}-{}.tmp",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&tmp, &bytes).ok()?;
    if std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
        if !path.is_file() {
            return None;
        }
    }
    Some(path)
}

#[cfg(test)]
mod tests;
