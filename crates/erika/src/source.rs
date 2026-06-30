use std::env;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::core::MediaSourceHint;
use crate::trace;

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
    prefetch: Option<PendingHttpFetch>,
}

struct PendingHttpFetch {
    range: ByteRange,
    handle: JoinHandle<Result<Vec<u8>>>,
}

impl HttpRangeSource {
    const DEFAULT_READ_AHEAD_BYTES: u64 = 2 * 1024 * 1024;

    pub fn new(uri: impl Into<String>) -> Self {
        let agent = http_agent();
        Self {
            uri: uri.into(),
            agent,
            content_length: None,
            cache_start: 0,
            cache_bytes: Vec::new(),
            read_ahead_bytes: http_read_ahead_bytes(),
            prefetch: None,
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

    fn cached_prefix(&self, range: ByteRange) -> Option<Vec<u8>> {
        let length = range.length?;
        let end = range.start.checked_add(length)?;
        let cache_end = self.cache_end();
        if range.start < self.cache_start || range.start >= cache_end || end <= cache_end {
            return None;
        }
        let start_index = usize::try_from(range.start - self.cache_start).ok()?;
        let end_index = usize::try_from(cache_end - self.cache_start).ok()?;
        Some(self.cache_bytes[start_index..end_index].to_vec())
    }

    fn fetch_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        fetch_http_range(&self.agent, &self.uri, range, "http_range")
    }

    fn fetch_length(&mut self, range: ByteRange) -> Result<Option<u64>> {
        let requested_length = range.length.unwrap_or(0);
        Ok(match range.length {
            Some(length) => {
                let mut length = length.max(self.read_ahead_bytes);
                if let Some(total) = self.content_length.or_else(|| self.len().ok().flatten()) {
                    if range.start >= total {
                        return Ok(Some(0));
                    }
                    length = length.min(total.saturating_sub(range.start));
                }
                Some(length.max(requested_length))
            }
            None => None,
        })
    }

    fn take_prefetch(&mut self, range: ByteRange) -> Option<Result<(u64, Vec<u8>)>> {
        let pending = self.prefetch.as_ref()?;
        if !range_contains(pending.range, range) {
            let _ = self.prefetch.take();
            return None;
        }
        if !pending.is_finished() {
            http_trace_log(format!(
                "{{\"event\":\"http_prefetch_pending\",\"start\":{},\"length\":{},\"requested_start\":{},\"requested_length\":{}}}",
                pending.range.start,
                pending
                    .range
                    .length
                    .map_or_else(|| "null".to_string(), |length| length.to_string()),
                range.start,
                range
                    .length
                    .map_or_else(|| "null".to_string(), |length| length.to_string()),
            ));
            return None;
        }

        let pending = self.prefetch.take()?;
        let join_started = Instant::now();
        let start = pending.range.start;
        let result = pending
            .handle
            .join()
            .map_err(|_| SourceError::Http("http prefetch thread panicked".to_string()))
            .and_then(|bytes| bytes);
        http_trace_log(format!(
            "{{\"event\":\"http_prefetch_join\",\"start\":{},\"length\":{},\"elapsed_ms\":{:.3}}}",
            start,
            pending
                .range
                .length
                .map_or_else(|| "null".to_string(), |length| length.to_string()),
            join_started.elapsed().as_secs_f64() * 1000.0,
        ));
        Some(result.map(|bytes| (start, bytes)))
    }

    fn maybe_start_prefetch(&mut self, range: ByteRange) {
        if self.prefetch.is_some() || self.cache_bytes.is_empty() {
            return;
        }
        let Some(length) = range.length else {
            return;
        };
        let Some(total) = self.content_length else {
            return;
        };
        let Some(end) = range.start.checked_add(length) else {
            return;
        };
        let cache_end = self.cache_end();
        if end > cache_end || cache_end >= total {
            return;
        }
        let remaining = cache_end.saturating_sub(end);
        if remaining > self.read_ahead_bytes / 2 {
            return;
        }
        let length = self.read_ahead_bytes.min(total.saturating_sub(cache_end));
        if length == 0 {
            return;
        }
        let prefetch_range = ByteRange {
            start: cache_end,
            length: Some(length),
        };
        self.prefetch = Some(PendingHttpFetch::spawn(self.uri.clone(), prefetch_range));
    }
}

impl PendingHttpFetch {
    fn spawn(uri: String, range: ByteRange) -> Self {
        http_trace_log(format!(
            "{{\"event\":\"http_prefetch_start\",\"start\":{},\"length\":{}}}",
            range.start,
            range
                .length
                .map_or_else(|| "null".to_string(), |length| length.to_string()),
        ));
        let handle = thread::spawn(move || {
            let agent = http_agent();
            fetch_http_range(&agent, &uri, range, "http_prefetch_range")
        });
        Self { range, handle }
    }

    fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }
}

fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(15)))
        .timeout_recv_body(Some(Duration::from_secs(60)))
        .build()
        .into()
}

fn fetch_http_range(
    agent: &ureq::Agent,
    uri: &str,
    range: ByteRange,
    event: &str,
) -> Result<Vec<u8>> {
    let header = match range.length {
        Some(length) if length > 0 => {
            let end = range.start.saturating_add(length).saturating_sub(1);
            format!("bytes={}-{}", range.start, end)
        }
        _ => format!("bytes={}-", range.start),
    };
    let started = Instant::now();
    let mut response = match agent.get(uri).header("Range", &header).call() {
        Ok(response) => response,
        Err(error) => {
            http_trace_log(format!(
                "{{\"event\":\"{}_error\",\"phase\":\"request\",\"start\":{},\"length\":{},\"elapsed_ms\":{:.3},\"error\":\"{}\"}}",
                event,
                range.start,
                range
                    .length
                    .map_or_else(|| "null".to_string(), |length| length.to_string()),
                started.elapsed().as_secs_f64() * 1000.0,
                json_escape(&error.to_string()),
            ));
            return Err(SourceError::Http(error.to_string()));
        }
    };
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    if let Err(error) = response.body_mut().as_reader().read_to_end(&mut bytes) {
        http_trace_log(format!(
            "{{\"event\":\"{}_error\",\"phase\":\"body\",\"start\":{},\"length\":{},\"status\":{},\"bytes\":{},\"elapsed_ms\":{:.3},\"error\":\"{}\"}}",
            event,
            range.start,
            range
                .length
                .map_or_else(|| "null".to_string(), |length| length.to_string()),
            status,
            bytes.len(),
            started.elapsed().as_secs_f64() * 1000.0,
            json_escape(&error.to_string()),
        ));
        return Err(SourceError::Http(error.to_string()));
    }
    http_trace_log(format!(
        "{{\"event\":\"{}\",\"start\":{},\"length\":{},\"status\":{},\"bytes\":{},\"elapsed_ms\":{:.3}}}",
        event,
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
        http_trace_log(format!(
            "[erika-http-trace] stage=head_request uri={} cache_start={} cache_end={} read_ahead={}",
            redacted_uri(&self.uri),
            self.cache_start,
            self.cache_end(),
            self.read_ahead_bytes,
        ));
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
            "[erika-http-trace] stage=head_response status={} length={} elapsed_ms={:.3}",
            status,
            length.map_or_else(|| "null".to_string(), |length| length.to_string()),
            started.elapsed().as_secs_f64() * 1000.0,
        ));
        Ok(length)
    }

    fn read_range(&mut self, range: ByteRange) -> Result<Vec<u8>> {
        if let Some(bytes) = self.cached_slice(range) {
            self.maybe_start_prefetch(range);
            http_trace_log(format!(
                "{{\"event\":\"http_cache_hit\",\"start\":{},\"length\":{},\"bytes\":{}}}",
                range.start,
                range.length.unwrap_or_default(),
                bytes.len(),
            ));
            return Ok(bytes);
        }

        let requested_length = range.length.unwrap_or(0);
        if let Some(prefix) = self.cached_prefix(range) {
            let prefix_length = prefix.len() as u64;
            let suffix_range = ByteRange {
                start: range.start.saturating_add(prefix_length),
                length: Some(requested_length.saturating_sub(prefix_length)),
            };
            if let Some(prefetch) = self.take_prefetch(suffix_range) {
                let (prefetch_start, prefetch_bytes) = prefetch?;
                let prefetch_range = ByteRange {
                    start: prefetch_start,
                    length: Some(prefetch_bytes.len() as u64),
                };
                if range_contains(prefetch_range, suffix_range) {
                    let mut cache_bytes = prefix;
                    cache_bytes.extend_from_slice(&prefetch_bytes);
                    self.cache_start = range.start;
                    self.cache_bytes = cache_bytes;
                    self.maybe_start_prefetch(range);
                    let copy_len = requested_length.min(self.cache_bytes.len() as u64) as usize;
                    return Ok(self.cache_bytes[..copy_len].to_vec());
                }
            }

            let _ = self.prefetch.take();
            let fetch_length = self.fetch_length(suffix_range)?;
            if fetch_length == Some(0) {
                return Ok(prefix);
            }
            let suffix = self.fetch_range(ByteRange {
                start: suffix_range.start,
                length: fetch_length,
            })?;
            let mut cache_bytes = prefix;
            cache_bytes.extend_from_slice(&suffix);
            self.cache_start = range.start;
            self.cache_bytes = cache_bytes;
            self.maybe_start_prefetch(range);
            let copy_len = requested_length.min(self.cache_bytes.len() as u64) as usize;
            return Ok(self.cache_bytes[..copy_len].to_vec());
        }
        let fetch_length = self.fetch_length(range)?;
        if fetch_length == Some(0) {
            return Ok(Vec::new());
        }
        let fetched = match self.take_prefetch(range) {
            Some(Ok((start, bytes))) => {
                self.cache_start = start;
                self.cache_bytes = bytes;
                let bytes = self.cached_slice(range).unwrap_or_default();
                self.maybe_start_prefetch(range);
                return Ok(bytes);
            }
            Some(Err(error)) => return Err(error),
            None => self.fetch_range(ByteRange {
                start: range.start,
                length: fetch_length,
            })?,
        };
        if range.length.is_none() {
            return Ok(fetched);
        }

        self.cache_start = range.start;
        self.cache_bytes = fetched;
        self.maybe_start_prefetch(range);
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

fn range_contains(container: ByteRange, range: ByteRange) -> bool {
    let (Some(container_length), Some(range_length)) = (container.length, range.length) else {
        return false;
    };
    let Some(container_end) = container.start.checked_add(container_length) else {
        return false;
    };
    let Some(range_end) = range.start.checked_add(range_length) else {
        return false;
    };
    range.start >= container.start && range_end <= container_end
}

fn http_read_ahead_bytes() -> u64 {
    env::var("ERIKA_HTTP_READAHEAD_BYTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(HttpRangeSource::DEFAULT_READ_AHEAD_BYTES)
}

fn http_trace_log(line: impl AsRef<str>) {
    if !trace::env_flag("ERIKA_HTTP_TRACE") {
        return;
    }
    let path = env::var_os("ERIKA_HTTP_TRACE_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/erika_http_trace.jsonl"));
    trace::append_line(line.as_ref(), path);
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

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
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
    fn http_default_read_ahead_is_streaming_sized() {
        assert_eq!(HttpRangeSource::DEFAULT_READ_AHEAD_BYTES, 2 * 1024 * 1024);
    }

    #[test]
    fn range_contains_accepts_inner_byte_ranges() {
        assert!(range_contains(
            ByteRange {
                start: 100,
                length: Some(200),
            },
            ByteRange {
                start: 128,
                length: Some(64),
            },
        ));
        assert!(!range_contains(
            ByteRange {
                start: 100,
                length: Some(200),
            },
            ByteRange {
                start: 280,
                length: Some(64),
            },
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
