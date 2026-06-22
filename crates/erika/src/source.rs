use std::env;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::core::MediaSourceHint;

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("io error: {0}")]
    Io(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("unsupported source URI: {0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, SourceError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub length: Option<u64>,
}

impl ByteRange {
    pub fn suffix_from(start: u64) -> Self {
        Self {
            start,
            length: None,
        }
    }
}

pub trait MediaSource: Send {
    fn uri(&self) -> &str;
    fn len(&mut self) -> Result<Option<u64>>;
    fn read_range(&mut self, range: ByteRange) -> Result<Vec<u8>>;
}

#[derive(Debug)]
pub struct LocalFileSource {
    uri: String,
    path: PathBuf,
}

impl LocalFileSource {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let uri = format!("file://{}", path.display());
        Ok(Self { uri, path })
    }
}

impl MediaSource for LocalFileSource {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn len(&mut self) -> Result<Option<u64>> {
        let metadata =
            std::fs::metadata(&self.path).map_err(|error| SourceError::Io(error.to_string()))?;
        Ok(Some(metadata.len()))
    }

    fn read_range(&mut self, range: ByteRange) -> Result<Vec<u8>> {
        let mut file =
            File::open(&self.path).map_err(|error| SourceError::Io(error.to_string()))?;
        file.seek(SeekFrom::Start(range.start))
            .map_err(|error| SourceError::Io(error.to_string()))?;
        let mut reader: Box<dyn Read> = match range.length {
            Some(length) => Box::new(file.take(length)),
            None => Box::new(file),
        };
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|error| SourceError::Io(error.to_string()))?;
        Ok(bytes)
    }
}

pub struct HttpRangeSource {
    uri: String,
    agent: ureq::Agent,
    content_length: Option<u64>,
    cache_start: u64,
    cache_bytes: Vec<u8>,
    read_ahead_bytes: u64,
}

impl HttpRangeSource {
    const DEFAULT_READ_AHEAD_BYTES: u64 = 1024 * 1024;

    pub fn new(uri: impl Into<String>) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_response(Some(Duration::from_secs(15)))
            .timeout_recv_body(Some(Duration::from_secs(60)))
            .build()
            .into();
        Self {
            uri: uri.into(),
            agent,
            content_length: None,
            cache_start: 0,
            cache_bytes: Vec::new(),
            read_ahead_bytes: http_read_ahead_bytes(),
        }
    }

    fn cache_end(&self) -> u64 {
        self.cache_start
            .saturating_add(self.cache_bytes.len() as u64)
    }

    fn cached_slice(&self, range: ByteRange) -> Option<Vec<u8>> {
        let length = range.length?;
        let end = range.start.checked_add(length)?;
        if range.start < self.cache_start || end > self.cache_end() {
            return None;
        }
        let start_index = usize::try_from(range.start - self.cache_start).ok()?;
        let length = usize::try_from(length).ok()?;
        let end_index = start_index.checked_add(length)?;
        Some(self.cache_bytes[start_index..end_index].to_vec())
    }

    fn fetch_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        let header = match range.length {
            Some(length) if length > 0 => {
                let end = range.start.saturating_add(length).saturating_sub(1);
                format!("bytes={}-{}", range.start, end)
            }
            _ => format!("bytes={}-", range.start),
        };
        let started = Instant::now();
        let mut response = self
            .agent
            .get(&self.uri)
            .header("Range", &header)
            .call()
            .map_err(|error| SourceError::Http(error.to_string()))?;
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .read_to_end(&mut bytes)
            .map_err(|error| SourceError::Http(error.to_string()))?;
        http_trace_log(format!(
            "{{\"event\":\"http_range\",\"start\":{},\"length\":{},\"status\":{},\"bytes\":{},\"elapsed_ms\":{:.3}}}",
            range.start,
            range
                .length
                .map_or_else(|| "null".to_string(), |length| length.to_string()),
            status,
            bytes.len(),
            started.elapsed().as_secs_f64() * 1000.0,
        ));
        Ok(bytes)
    }
}

impl std::fmt::Debug for HttpRangeSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpRangeSource")
            .field("uri", &redacted_uri(&self.uri))
            .field("content_length", &self.content_length)
            .field("cache_start", &self.cache_start)
            .field("cache_bytes", &self.cache_bytes.len())
            .field("read_ahead_bytes", &self.read_ahead_bytes)
            .finish()
    }
}

impl MediaSource for HttpRangeSource {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn len(&mut self) -> Result<Option<u64>> {
        if self.content_length.is_some() {
            return Ok(self.content_length);
        }
        let started = Instant::now();
        let response = self
            .agent
            .head(&self.uri)
            .call()
            .map_err(|error| SourceError::Http(error.to_string()))?;
        let status = response.status().as_u16();
        let length = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        self.content_length = length;
        http_trace_log(format!(
            "{{\"event\":\"http_head\",\"status\":{},\"length\":{},\"elapsed_ms\":{:.3}}}",
            status,
            length.map_or_else(|| "null".to_string(), |length| length.to_string()),
            started.elapsed().as_secs_f64() * 1000.0,
        ));
        Ok(length)
    }

    fn read_range(&mut self, range: ByteRange) -> Result<Vec<u8>> {
        if let Some(bytes) = self.cached_slice(range) {
            http_trace_log(format!(
                "{{\"event\":\"http_cache_hit\",\"start\":{},\"length\":{},\"bytes\":{}}}",
                range.start,
                range.length.unwrap_or_default(),
                bytes.len(),
            ));
            return Ok(bytes);
        }

        let requested_length = range.length.unwrap_or(0);
        let fetch_length = match range.length {
            Some(length) => {
                let mut length = length.max(self.read_ahead_bytes);
                if let Some(total) = self.content_length.or_else(|| self.len().ok().flatten()) {
                    if range.start >= total {
                        return Ok(Vec::new());
                    }
                    length = length.min(total.saturating_sub(range.start));
                }
                Some(length.max(requested_length))
            }
            None => None,
        };
        let fetched = self.fetch_range(ByteRange {
            start: range.start,
            length: fetch_length,
        })?;
        if range.length.is_none() {
            return Ok(fetched);
        }

        self.cache_start = range.start;
        self.cache_bytes = fetched;
        let copy_len = requested_length.min(self.cache_bytes.len() as u64) as usize;
        Ok(self.cache_bytes[..copy_len].to_vec())
    }
}

pub fn source_from_uri(uri: &str) -> Result<Box<dyn MediaSource>> {
    source_from_uri_with_hint(uri, MediaSourceHint::Auto)
}

pub fn source_from_uri_with_hint(
    uri: &str,
    source_hint: MediaSourceHint,
) -> Result<Box<dyn MediaSource>> {
    match source_hint {
        MediaSourceHint::Auto => source_from_auto_uri(uri),
        MediaSourceHint::LocalFile => {
            Ok(Box::new(LocalFileSource::open(local_path_from_uri(uri))?))
        }
        MediaSourceHint::Http => {
            if uri.starts_with("http://") || uri.starts_with("https://") {
                Ok(Box::new(HttpRangeSource::new(uri)))
            } else {
                Err(SourceError::Unsupported(uri.to_string()))
            }
        }
    }
}

fn source_from_auto_uri(uri: &str) -> Result<Box<dyn MediaSource>> {
    if let Some(path) = uri.strip_prefix("file://") {
        return Ok(Box::new(LocalFileSource::open(path)?));
    }
    if uri.starts_with("http://") || uri.starts_with("https://") {
        return Ok(Box::new(HttpRangeSource::new(uri)));
    }
    let path = Path::new(uri);
    if path.exists() {
        return Ok(Box::new(LocalFileSource::open(path)?));
    }
    Err(SourceError::Unsupported(uri.to_string()))
}

fn local_path_from_uri(uri: &str) -> &str {
    uri.strip_prefix("file://").unwrap_or(uri)
}

fn http_read_ahead_bytes() -> u64 {
    env::var("ERIKA_HTTP_READAHEAD_BYTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(HttpRangeSource::DEFAULT_READ_AHEAD_BYTES)
}

fn http_trace_log(line: impl AsRef<str>) {
    if !crate::trace::env_flag("ERIKA_HTTP_TRACE") {
        return;
    }
    let line = line.as_ref();
    eprintln!("{line}");
    let path = env::var_os("ERIKA_HTTP_TRACE_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/erika_http_trace.jsonl"));
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| writeln!(file, "{line}"));
}

fn redacted_uri(uri: &str) -> String {
    let mut value = uri.to_string();
    for key in ["api_key=", "AccessToken="] {
        let mut search_from = 0;
        while let Some(relative) = value[search_from..].find(key) {
            let start = search_from + relative + key.len();
            let end = value[start..]
                .find('&')
                .map(|relative_end| start + relative_end)
                .unwrap_or(value.len());
            value.replace_range(start..end, "REDACTED");
            search_from = start + "REDACTED".len();
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn local_file_source_reads_ranges() {
        let path = std::env::temp_dir().join(format!("erika-source-{}.bin", std::process::id()));
        {
            let mut file = File::create(&path).unwrap();
            file.write_all(b"abcdef").unwrap();
        }

        let mut source = LocalFileSource::open(&path).unwrap();
        assert_eq!(source.len().unwrap(), Some(6));
        assert_eq!(
            source
                .read_range(ByteRange {
                    start: 2,
                    length: Some(3)
                })
                .unwrap(),
            b"cde"
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn source_from_uri_rejects_unknown_scheme() {
        match source_from_uri("smb://example/video.mkv") {
            Ok(_) => panic!("unexpectedly accepted unsupported source"),
            Err(error) => assert!(matches!(error, SourceError::Unsupported(_))),
        }
    }

    #[test]
    fn source_hint_controls_selection() {
        let source =
            source_from_uri_with_hint("https://example.invalid/video.mp4", MediaSourceHint::Http)
                .unwrap();
        assert_eq!(source.uri(), "https://example.invalid/video.mp4");

        assert!(matches!(
            source_from_uri_with_hint("file:///tmp/video.mp4", MediaSourceHint::Http),
            Err(SourceError::Unsupported(_))
        ));
    }

    #[test]
    fn redacted_uri_hides_access_tokens() {
        assert_eq!(
            redacted_uri("https://example.invalid/video.mkv?api_key=secret&x=1"),
            "https://example.invalid/video.mkv?api_key=REDACTED&x=1"
        );
        assert_eq!(
            redacted_uri("https://example.invalid/video.mkv?AccessToken=secret"),
            "https://example.invalid/video.mkv?AccessToken=REDACTED"
        );
    }
}
