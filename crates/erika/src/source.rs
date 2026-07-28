#[cfg(target_os = "android")]
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
#[cfg(target_os = "android")]
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::path::{Path, PathBuf};
#[cfg(target_os = "android")]
use std::sync::{Mutex, OnceLock};
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
    #[error("invalid owned file descriptor URI: {0}")]
    InvalidFileDescriptorUri(String),
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

/// A seekable Android content descriptor owned by the media source.
///
/// The descriptor is closed automatically when this value is dropped. `offset`
/// and `length` expose an `AssetFileDescriptor` slice as a zero-based media file.
#[cfg(target_os = "android")]
#[derive(Debug)]
pub struct OwnedFileDescriptorSource {
    uri: String,
    file: File,
    offset: u64,
    length: Option<u64>,
}

/// Keeps an Android-owned descriptor registered until a synchronous native
/// source call either adopts it or returns an error.
///
/// Dropping the registration closes the descriptor when no `MediaSource`
/// consumed it. This closes the ownership gap between JNI validation and the
/// point where playback constructs `OwnedFileDescriptorSource`.
#[cfg(target_os = "android")]
#[derive(Debug)]
pub struct AndroidOwnedFdRegistration {
    fd: RawFd,
}

#[cfg(target_os = "android")]
impl Drop for AndroidOwnedFdRegistration {
    fn drop(&mut self) {
        if let Ok(mut registry) = android_owned_fd_registry().lock() {
            let _ = registry.remove(&self.fd);
        }
    }
}

/// Registers a descriptor transferred by the Android host for one synchronous
/// native invocation. `source_from_uri` consumes the registered `File`; if the
/// invocation fails before that boundary, the returned guard closes it.
#[cfg(target_os = "android")]
pub fn register_android_owned_fd(file: File) -> Result<AndroidOwnedFdRegistration> {
    let fd = file.as_raw_fd();
    if fd < 0 {
        return Err(SourceError::InvalidFileDescriptorUri(format!(
            "negative descriptor {fd}"
        )));
    }
    let mut registry = android_owned_fd_registry()
        .lock()
        .map_err(|_| SourceError::Io("Android owned-fd registry mutex poisoned".to_string()))?;
    if registry.contains_key(&fd) {
        // The existing entry already owns this raw descriptor. Closing a second
        // File wrapper here would invalidate that entry, so discard only the
        // duplicate wrapper and preserve the original ownership.
        std::mem::forget(file);
        return Err(SourceError::InvalidFileDescriptorUri(format!(
            "descriptor {fd} is already awaiting source adoption"
        )));
    }
    registry.insert(fd, file);
    Ok(AndroidOwnedFdRegistration { fd })
}

#[cfg(target_os = "android")]
fn android_owned_fd_registry() -> &'static Mutex<HashMap<RawFd, File>> {
    static REGISTRY: OnceLock<Mutex<HashMap<RawFd, File>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(target_os = "android")]
fn take_registered_android_owned_fd(fd: RawFd) -> Option<File> {
    android_owned_fd_registry().lock().ok()?.remove(&fd)
}

#[cfg(target_os = "android")]
impl OwnedFileDescriptorSource {
    /// Takes ownership of `fd`; callers must not close or reuse it afterwards.
    ///
    /// # Safety
    ///
    /// `fd` must be a valid, uniquely-owned, seekable descriptor.
    pub unsafe fn from_owned_fd(
        fd: RawFd,
        offset: u64,
        length: Option<u64>,
        uri: impl Into<String>,
    ) -> Result<Self> {
        if fd < 0 {
            return Err(SourceError::InvalidFileDescriptorUri(format!(
                "negative descriptor {fd}"
            )));
        }
        // SAFETY: ownership is transferred by the function contract.
        let file = unsafe { File::from_raw_fd(fd) };
        Self::from_owned_file(file, offset, length, uri.into())
    }

    fn from_owned_file(file: File, offset: u64, length: Option<u64>, uri: String) -> Result<Self> {
        let metadata = file
            .metadata()
            .map_err(|error| SourceError::Io(error.to_string()))?;
        let length = length.or_else(|| metadata.len().checked_sub(offset));
        Ok(Self {
            uri,
            file,
            offset,
            length,
        })
    }

    unsafe fn open_uri(uri: &str) -> Result<Self> {
        let fd = parse_owned_fd(uri)?;
        // Safe URI dispatch may only consume descriptors registered by the JNI
        // transferred-fd contract. Never adopt a registry miss by raw number:
        // that could seize or double-close an unrelated process descriptor.
        // Direct native callers with unique ownership must use `from_owned_fd`.
        let file = take_registered_android_owned_fd(fd).ok_or_else(|| {
            SourceError::InvalidFileDescriptorUri(format!(
                "{uri} (descriptor was not explicitly transferred)"
            ))
        })?;
        let spec = parse_fd_uri(uri)?;
        Self::from_owned_file(file, spec.offset, spec.length, uri.to_string())
    }
}

#[cfg(target_os = "android")]
impl MediaSource for OwnedFileDescriptorSource {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn len(&mut self) -> Result<Option<u64>> {
        Ok(self.length)
    }

    fn read_range(&mut self, range: ByteRange) -> Result<Vec<u8>> {
        let length = match self.length {
            Some(total) if range.start >= total => return Ok(Vec::new()),
            Some(total) => Some(
                range
                    .length
                    .unwrap_or_else(|| total.saturating_sub(range.start))
                    .min(total.saturating_sub(range.start)),
            ),
            None => range.length,
        };
        let absolute_start = self.offset.checked_add(range.start).ok_or_else(|| {
            SourceError::Io("owned descriptor seek offset overflowed u64".to_string())
        })?;
        self.file
            .seek(SeekFrom::Start(absolute_start))
            .map_err(|error| SourceError::Io(error.to_string()))?;
        let mut bytes = Vec::new();
        match length {
            Some(length) => (&mut self.file)
                .take(length)
                .read_to_end(&mut bytes)
                .map_err(|error| SourceError::Io(error.to_string()))?,
            None => self
                .file
                .read_to_end(&mut bytes)
                .map_err(|error| SourceError::Io(error.to_string()))?,
        };
        Ok(bytes)
    }
}

#[cfg(any(target_os = "android", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OwnedFdUri {
    fd: i32,
    offset: u64,
    length: Option<u64>,
}

#[cfg(any(target_os = "android", test))]
fn parse_fd_uri(uri: &str) -> Result<OwnedFdUri> {
    let body = uri
        .strip_prefix("fd://")
        .ok_or_else(|| SourceError::InvalidFileDescriptorUri(uri.to_string()))?;
    let (fd, query) = body.split_once('?').unwrap_or((body, ""));
    let fd = parse_owned_fd_value(fd, uri)?;
    let mut offset = None;
    let mut length = None;
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| SourceError::InvalidFileDescriptorUri(uri.to_string()))?;
        match key {
            "offset" => {
                if offset.is_some() {
                    return Err(SourceError::InvalidFileDescriptorUri(uri.to_string()));
                }
                offset = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| SourceError::InvalidFileDescriptorUri(uri.to_string()))?,
                );
            }
            "length" => {
                if length.is_some() {
                    return Err(SourceError::InvalidFileDescriptorUri(uri.to_string()));
                }
                length = Some(if value.is_empty() || value == "-1" {
                    None
                } else {
                    Some(
                        value
                            .parse::<u64>()
                            .map_err(|_| SourceError::InvalidFileDescriptorUri(uri.to_string()))?,
                    )
                });
            }
            // Display names/URIs may be appended by the Android host for diagnostics.
            "name" | "display_uri" => {}
            _ => return Err(SourceError::InvalidFileDescriptorUri(uri.to_string())),
        }
    }
    Ok(OwnedFdUri {
        fd,
        offset: offset.unwrap_or(0),
        length: length.flatten(),
    })
}

#[cfg(target_os = "android")]
fn parse_owned_fd(uri: &str) -> Result<i32> {
    let body = uri
        .strip_prefix("fd://")
        .ok_or_else(|| SourceError::InvalidFileDescriptorUri(uri.to_string()))?;
    let fd = body.split_once(['?', '/', '#']).map_or(body, |(fd, _)| fd);
    parse_owned_fd_value(fd, uri)
}

#[cfg(any(target_os = "android", test))]
fn parse_owned_fd_value(value: &str, uri: &str) -> Result<i32> {
    value
        .parse::<i32>()
        .ok()
        .filter(|fd| *fd >= 0)
        .ok_or_else(|| SourceError::InvalidFileDescriptorUri(uri.to_string()))
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
    handle: JoinHandle<Result<HttpRangeResponse>>,
}

/// Bytes fetched for one HTTP range request plus the resource total reported
/// by the server (`Content-Range` on 206, `Content-Length` on a whole-file
/// 200). The total lets callers backfill `content_length` when HEAD is
/// unavailable (e.g. servers answering HEAD with 405).
struct HttpRangeResponse {
    bytes: Vec<u8>,
    total_length: Option<u64>,
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

    fn fetch_range(&mut self, range: ByteRange) -> Result<Vec<u8>> {
        let response = fetch_http_range(&self.agent, &self.uri, range, "http_range")?;
        if self.content_length.is_none() {
            self.content_length = response.total_length;
        }
        Ok(response.bytes)
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
            // The pending fetch covers the requested bytes and they are needed
            // now: joining the in-flight thread is expected to be much cheaper
            // than issuing a duplicate synchronous download of the same range.
            http_trace_log(format!(
                "{{\"event\":\"http_prefetch_pending\",\"decision\":\"join\",\"start\":{},\"length\":{},\"requested_start\":{},\"requested_length\":{}}}",
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
        }

        let pending = self.prefetch.take()?;
        let join_started = Instant::now();
        let start = pending.range.start;
        let result = pending
            .handle
            .join()
            .map_err(|_| SourceError::Http("http prefetch thread panicked".to_string()))
            .and_then(|response| response);
        http_trace_log(format!(
            "{{\"event\":\"http_prefetch_join\",\"start\":{},\"length\":{},\"elapsed_ms\":{:.3}}}",
            start,
            pending
                .range
                .length
                .map_or_else(|| "null".to_string(), |length| length.to_string()),
            join_started.elapsed().as_secs_f64() * 1000.0,
        ));
        Some(result.map(|response| {
            if self.content_length.is_none() {
                self.content_length = response.total_length;
            }
            (start, response.bytes)
        }))
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

const HTTP_FETCH_MAX_ATTEMPTS: u32 = 3;
const HTTP_FETCH_RETRY_BACKOFF: [Duration; 2] =
    [Duration::from_millis(200), Duration::from_secs(1)];

fn http_retry_backoff(attempt: u32) -> Duration {
    let index = usize::try_from(attempt.saturating_sub(1)).unwrap_or(0);
    HTTP_FETCH_RETRY_BACKOFF
        .get(index)
        .copied()
        .unwrap_or(Duration::from_secs(1))
}

/// Whether a failed HTTP exchange is worth retrying: transport errors and 5xx
/// responses are transient; 4xx responses are deterministic client errors.
fn http_error_is_retryable(error: &ureq::Error) -> bool {
    match error {
        ureq::Error::StatusCode(status) => *status >= 500,
        _ => true,
    }
}

/// Parses the `total` out of a `Content-Range: bytes start-end/total` header.
/// Returns `None` for missing headers, unsatisfied-range (`*/total` still
/// yields the total), and unknown totals (`bytes 0-1/*`).
fn parse_content_range_total(value: &str) -> Option<u64> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (_, total) = rest.rsplit_once('/')?;
    total.trim().parse::<u64>().ok()
}

/// Parses the first byte offset out of a `Content-Range: bytes start-end/total`
/// header. `None` for an unsatisfied-range form (`bytes */total`), which names
/// no offset.
fn parse_content_range_start(value: &str) -> Option<u64> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (range, _) = rest.rsplit_once('/')?;
    let (start, _) = range.trim().split_once('-')?;
    start.trim().parse::<u64>().ok()
}

/// The strongest entity validator the response offers, preferred in the order
/// RFC 9110 recommends for `If-Range`.
fn response_entity_validator<T>(response: &ureq::http::Response<T>) -> Option<String> {
    ["etag", "last-modified"].into_iter().find_map(|name| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

/// Learns the total length from a one-byte GET, for origins that reject HEAD.
///
/// The body is deliberately never read. An origin that rejects HEAD *and*
/// ignores Range answers 200 with the whole object, so buffering the response
/// would turn a `len()` call into a full download of the media -- gigabytes
/// into memory before playback, just to learn a number the headers already
/// carry.
fn probe_http_total_length(agent: &ureq::Agent, uri: &str) -> Result<Option<u64>> {
    let probe = ByteRange {
        start: 0,
        length: Some(1),
    };
    let response = agent
        .get(uri)
        .header("Range", &http_range_header(probe))
        .call()
        .map_err(|error| {
            http_trace_log(format!(
                "{{\"event\":\"http_length_probe_error\",\"phase\":\"request\",\"error\":\"{}\"}}",
                json_escape(&error.to_string()),
            ));
            SourceError::Http(error.to_string())
        })?;
    let status = response.status().as_u16();
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    };
    let total_length = match status {
        206 => header("content-range")
            .as_deref()
            .and_then(parse_content_range_total),
        // Range was ignored; Content-Length is the whole object, which is
        // exactly the total being probed for.
        200 => header("content-length").and_then(|value| value.trim().parse::<u64>().ok()),
        _ => None,
    };
    http_trace_log(format!(
        "{{\"event\":\"http_length_probe\",\"status\":{},\"total\":{}}}",
        status,
        total_length.map_or_else(|| "null".to_string(), |total| total.to_string()),
    ));
    Ok(total_length)
}

fn http_range_header(range: ByteRange) -> String {
    match range.length {
        Some(length) if length > 0 => {
            let end = range.start.saturating_add(length).saturating_sub(1);
            format!("bytes={}-{}", range.start, end)
        }
        _ => format!("bytes={}-", range.start),
    }
}

fn fetch_http_range(
    agent: &ureq::Agent,
    uri: &str,
    range: ByteRange,
    event: &str,
) -> Result<HttpRangeResponse> {
    let mut bytes = Vec::new();
    let mut total_length = None;
    // Entity validator from the first response. A resumed request replays it as
    // `If-Range` so an origin that re-encoded or load-balanced to a different
    // variant answers 200 (which the status check below rejects for a non-zero
    // start) instead of handing back bytes from a different object to be spliced
    // onto the prefix we already hold.
    let mut validator: Option<String> = None;
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        // Resume from what already arrived: earlier attempts keep their bytes
        // and the Range start advances past them.
        let received = bytes.len() as u64;
        if range.length.is_some_and(|length| received >= length) {
            // A body error surfaced after every requested byte arrived; the
            // payload is complete, so do not re-request an open-ended tail.
            return Ok(HttpRangeResponse {
                bytes,
                total_length,
            });
        }
        let resume_range = ByteRange {
            start: range.start.saturating_add(received),
            length: range.length.map(|length| length.saturating_sub(received)),
        };
        let header = http_range_header(resume_range);
        let started = Instant::now();
        let mut request = agent.get(uri).header("Range", &header);
        if received > 0
            && let Some(validator) = validator.as_deref()
        {
            request = request.header("If-Range", validator);
        }
        let mut response = match request.call() {
            Ok(response) => response,
            Err(error) => {
                http_trace_log(format!(
                    "{{\"event\":\"{}_error\",\"phase\":\"request\",\"attempt\":{},\"start\":{},\"length\":{},\"elapsed_ms\":{:.3},\"error\":\"{}\"}}",
                    event,
                    attempt,
                    resume_range.start,
                    resume_range
                        .length
                        .map_or_else(|| "null".to_string(), |length| length.to_string()),
                    started.elapsed().as_secs_f64() * 1000.0,
                    json_escape(&error.to_string()),
                ));
                if attempt < HTTP_FETCH_MAX_ATTEMPTS && http_error_is_retryable(&error) {
                    let backoff = http_retry_backoff(attempt);
                    http_trace_log(format!(
                        "{{\"event\":\"{}_retry\",\"phase\":\"request\",\"attempt\":{},\"start\":{},\"received\":{},\"backoff_ms\":{}}}",
                        event,
                        attempt,
                        resume_range.start,
                        received,
                        backoff.as_millis(),
                    ));
                    thread::sleep(backoff);
                    continue;
                }
                return Err(SourceError::Http(error.to_string()));
            }
        };
        let status = response.status().as_u16();
        match status {
            206 => {
                let content_range = response
                    .headers()
                    .get("content-range")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string);
                // A resumed request must continue exactly where the prefix
                // ends. A server that answers 206 from a different offset --
                // or a changed entity that ignored If-Range -- would otherwise
                // be spliced onto the bytes already held and returned as
                // silently corrupt media.
                if let Some(start) = content_range.as_deref().and_then(parse_content_range_start)
                    && start != resume_range.start
                {
                    http_trace_log(format!(
                        "{{\"event\":\"{}_error\",\"phase\":\"content_range\",\"attempt\":{},\"start\":{},\"served_start\":{}}}",
                        event, attempt, resume_range.start, start,
                    ));
                    return Err(SourceError::Http(format!(
                        "server served range from {start}, expected {}",
                        resume_range.start
                    )));
                }
                if total_length.is_none() {
                    total_length = content_range.as_deref().and_then(parse_content_range_total);
                }
                if validator.is_none() {
                    validator = response_entity_validator(&response);
                }
            }
            200 => {
                if resume_range.start > 0 {
                    // The server sent the file from byte zero: treating that
                    // payload as `resume_range.start` data would silently
                    // corrupt the cache, so fail instead of retrying.
                    http_trace_log(format!(
                        "{{\"event\":\"{}_error\",\"phase\":\"status\",\"attempt\":{},\"start\":{},\"status\":200}}",
                        event, attempt, resume_range.start,
                    ));
                    return Err(SourceError::Http(
                        "server ignored Range request (status 200)".to_string(),
                    ));
                }
                if total_length.is_none() {
                    total_length = response
                        .headers()
                        .get("content-length")
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<u64>().ok());
                }
                if validator.is_none() {
                    validator = response_entity_validator(&response);
                }
            }
            // ureq maps 4xx/5xx to Error::StatusCode before this point; any
            // other status (204, 304, ...) carries no usable range payload.
            _ => {
                http_trace_log(format!(
                    "{{\"event\":\"{}_error\",\"phase\":\"status\",\"attempt\":{},\"start\":{},\"status\":{}}}",
                    event, attempt, resume_range.start, status,
                ));
                return Err(SourceError::Http(format!(
                    "unexpected HTTP status {status} for Range request"
                )));
            }
        }
        if let Err(error) = response.body_mut().as_reader().read_to_end(&mut bytes) {
            http_trace_log(format!(
                "{{\"event\":\"{}_error\",\"phase\":\"body\",\"attempt\":{},\"start\":{},\"length\":{},\"status\":{},\"bytes\":{},\"elapsed_ms\":{:.3},\"error\":\"{}\"}}",
                event,
                attempt,
                resume_range.start,
                resume_range
                    .length
                    .map_or_else(|| "null".to_string(), |length| length.to_string()),
                status,
                bytes.len(),
                started.elapsed().as_secs_f64() * 1000.0,
                json_escape(&error.to_string()),
            ));
            if attempt < HTTP_FETCH_MAX_ATTEMPTS {
                let backoff = http_retry_backoff(attempt);
                http_trace_log(format!(
                    "{{\"event\":\"{}_retry\",\"phase\":\"body\",\"attempt\":{},\"start\":{},\"received\":{},\"backoff_ms\":{}}}",
                    event,
                    attempt,
                    range.start.saturating_add(bytes.len() as u64),
                    bytes.len(),
                    backoff.as_millis(),
                ));
                thread::sleep(backoff);
                continue;
            }
            return Err(SourceError::Http(error.to_string()));
        }
        http_trace_log(format!(
            "{{\"event\":\"{}\",\"attempt\":{},\"start\":{},\"length\":{},\"status\":{},\"bytes\":{},\"elapsed_ms\":{:.3}}}",
            event,
            attempt,
            range.start,
            range
                .length
                .map_or_else(|| "null".to_string(), |length| length.to_string()),
            status,
            bytes.len(),
            started.elapsed().as_secs_f64() * 1000.0,
        ));
        return Ok(HttpRangeResponse {
            bytes,
            total_length,
        });
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
        http_trace_log(format!(
            "[erika-http-trace] stage=head_request uri={} cache_start={} cache_end={} read_ahead={}",
            redacted_uri(&self.uri),
            self.cache_start,
            self.cache_end(),
            self.read_ahead_bytes,
        ));
        let mut attempt = 0u32;
        let head_error = loop {
            attempt += 1;
            match self.agent.head(&self.uri).call() {
                Ok(response) => {
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
                    return Ok(length);
                }
                Err(error) => {
                    http_trace_log(format!(
                        "[erika-http-trace] stage=head_error attempt={} elapsed_ms={:.3} error={}",
                        attempt,
                        started.elapsed().as_secs_f64() * 1000.0,
                        json_escape(&error.to_string()),
                    ));
                    if attempt < HTTP_FETCH_MAX_ATTEMPTS && http_error_is_retryable(&error) {
                        let backoff = http_retry_backoff(attempt);
                        http_trace_log(format!(
                            "[erika-http-trace] stage=head_retry attempt={} backoff_ms={}",
                            attempt,
                            backoff.as_millis(),
                        ));
                        thread::sleep(backoff);
                        continue;
                    }
                    break error;
                }
            }
        };
        // Some servers reject HEAD (e.g. 405) yet still serve ranges. Probe
        // with a one-byte GET and take the total from Content-Range.
        http_trace_log(format!(
            "[erika-http-trace] stage=head_fallback_range error={}",
            json_escape(&head_error.to_string()),
        ));
        match probe_http_total_length(&self.agent, &self.uri) {
            Ok(total_length) => {
                self.content_length = total_length;
                Ok(self.content_length)
            }
            Err(_) => Err(SourceError::Http(head_error.to_string())),
        }
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
                if let Some(bytes) = self.cached_slice(range) {
                    self.maybe_start_prefetch(range);
                    return Ok(bytes);
                }
                // The prefetch returned fewer bytes than requested (short
                // read). An empty result here would be mistaken for EOF by
                // the AVIO layer, so fall back to a synchronous fetch.
                http_trace_log(format!(
                    "{{\"event\":\"http_prefetch_short_read\",\"start\":{},\"length\":{},\"cache_start\":{},\"cache_bytes\":{}}}",
                    range.start,
                    range
                        .length
                        .map_or_else(|| "null".to_string(), |length| length.to_string()),
                    self.cache_start,
                    self.cache_bytes.len(),
                ));
                self.fetch_range(ByteRange {
                    start: range.start,
                    length: fetch_length,
                })?
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

/// Reads an entire URI through the same MediaSource abstraction used by FFmpeg.
///
/// This is intentionally synchronous for small sidecar assets such as danmaku or
/// subtitle files. On Android it also establishes and completes the ownership
/// transfer for `fd://` descriptors within the native call.
pub fn read_uri_to_end(uri: &str) -> Result<Vec<u8>> {
    let mut source = source_from_uri(uri)?;
    source.read_range(ByteRange::suffix_from(0))
}

pub fn source_from_uri_with_hint(
    uri: &str,
    source_hint: MediaSourceHint,
) -> Result<Box<dyn MediaSource>> {
    match source_hint {
        MediaSourceHint::Auto => source_from_auto_uri(uri),
        MediaSourceHint::LocalFile => source_from_local_uri(uri),
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
    if uri.starts_with("fd://") {
        return source_from_local_uri(uri);
    }
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

fn source_from_local_uri(uri: &str) -> Result<Box<dyn MediaSource>> {
    if uri.starts_with("fd://") {
        #[cfg(target_os = "android")]
        {
            // SAFETY: accepting this URI is the ownership-transfer boundary.
            return Ok(Box::new(unsafe {
                OwnedFileDescriptorSource::open_uri(uri)?
            }));
        }
        #[cfg(not(target_os = "android"))]
        {
            return Err(SourceError::Unsupported(uri.to_string()));
        }
    }
    Ok(Box::new(LocalFileSource::open(local_path_from_uri(uri))?))
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
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    use super::*;

    struct MockResponse {
        delay: Duration,
        raw: Vec<u8>,
    }

    impl MockResponse {
        fn immediate(raw: Vec<u8>) -> Self {
            Self {
                delay: Duration::ZERO,
                raw,
            }
        }

        fn delayed(delay: Duration, raw: Vec<u8>) -> Self {
            Self { delay, raw }
        }
    }

    /// Serves each response over one connection (in order) and reports every
    /// received request head through the returned channel.
    fn spawn_mock_http_server(responses: Vec<MockResponse>) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let uri = format!("http://{}/video.mkv", listener.local_addr().unwrap());
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            for response in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    head.push_str(&line);
                }
                let _ = sender.send(head);
                if !response.delay.is_zero() {
                    thread::sleep(response.delay);
                }
                let _ = stream.write_all(&response.raw);
                let _ = stream.flush();
            }
        });
        (uri, receiver)
    }

    fn http_206_response(start: u64, total: u64, body: &[u8]) -> Vec<u8> {
        let end = start + body.len() as u64 - 1;
        let mut raw = format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{total}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len(),
        )
        .into_bytes();
        raw.extend_from_slice(body);
        raw
    }

    /// A 206 head that promises `declared_length` bytes but sends fewer before
    /// the connection closes, producing a body-phase transport error.
    fn http_206_truncated_response(
        start: u64,
        total: u64,
        declared_length: usize,
        body: &[u8],
    ) -> Vec<u8> {
        let end = start + declared_length as u64 - 1;
        let mut raw = format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{total}\r\nContent-Length: {declared_length}\r\nConnection: close\r\n\r\n",
        )
        .into_bytes();
        raw.extend_from_slice(body);
        raw
    }

    fn http_simple_response(status_line: &str, body: &[u8]) -> Vec<u8> {
        let mut raw = format!(
            "HTTP/1.1 {status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len(),
        )
        .into_bytes();
        raw.extend_from_slice(body);
        raw
    }

    fn recv_request_head(requests: &mpsc::Receiver<String>) -> String {
        requests
            .recv_timeout(Duration::from_secs(5))
            .expect("mock server should have received a request")
            .to_lowercase()
    }

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
        assert_eq!(
            read_uri_to_end(&format!("file://{}", path.display())).unwrap(),
            b"abcdef"
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
    fn owned_fd_uri_parses_asset_slice() {
        assert_eq!(
            parse_fd_uri("fd://42?offset=4096&length=8192").unwrap(),
            OwnedFdUri {
                fd: 42,
                offset: 4096,
                length: Some(8192),
            }
        );
        assert_eq!(
            parse_fd_uri("fd://7?length=-1").unwrap(),
            OwnedFdUri {
                fd: 7,
                offset: 0,
                length: None,
            }
        );
    }

    #[test]
    fn owned_fd_uri_rejects_invalid_or_ambiguous_values() {
        for uri in [
            "fd://-1",
            "fd://not-a-number",
            "fd://3?offset=x",
            "fd://3?offset=1&offset=2",
            "fd://3?unknown=1",
        ] {
            assert!(matches!(
                parse_fd_uri(uri),
                Err(SourceError::InvalidFileDescriptorUri(_))
            ));
        }
    }

    #[cfg(target_os = "android")]
    #[test]
    fn unregistered_owned_fd_uri_cannot_adopt_a_numeric_descriptor() {
        let error = unsafe { OwnedFileDescriptorSource::open_uri("fd://2147483647") }
            .expect_err("an unregistered descriptor must be rejected");
        assert!(matches!(
            error,
            SourceError::InvalidFileDescriptorUri(message)
                if message.contains("not explicitly transferred")
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
    fn content_range_total_parses_totals_and_rejects_unknown() {
        assert_eq!(parse_content_range_total("bytes 0-99/1234"), Some(1234));
        assert_eq!(parse_content_range_total("bytes 100-199/200"), Some(200));
        assert_eq!(parse_content_range_total("bytes */555"), Some(555));
        assert_eq!(parse_content_range_total("bytes 0-99/*"), None);
        assert_eq!(parse_content_range_total("items 0-99/1234"), None);
        assert_eq!(parse_content_range_total(""), None);
    }

    #[test]
    fn http_range_rejects_status_200_for_nonzero_offset() {
        let body = vec![b'a'; 100];
        let (uri, requests) = spawn_mock_http_server(vec![MockResponse::immediate(
            http_simple_response("200 OK", &body),
        )]);
        let mut source = HttpRangeSource::new(uri);
        source.content_length = Some(100);
        let error = source
            .read_range(ByteRange {
                start: 10,
                length: Some(10),
            })
            .expect_err("a 200 answer to a mid-file Range request must fail");
        assert!(matches!(
            error,
            SourceError::Http(message) if message.contains("ignored Range")
        ));
        assert!(recv_request_head(&requests).contains("range: bytes=10-99"));
    }

    #[test]
    fn http_range_accepts_status_200_for_whole_file_and_backfills_total() {
        let body = b"whole-file-payload".to_vec();
        let (uri, _requests) = spawn_mock_http_server(vec![MockResponse::immediate(
            http_simple_response("200 OK", &body),
        )]);
        let mut source = HttpRangeSource::new(uri);
        assert_eq!(
            source.read_range(ByteRange::suffix_from(0)).unwrap(),
            body.clone()
        );
        // Content-Length of the 200 response backfills the total without HEAD.
        assert_eq!(source.len().unwrap(), Some(body.len() as u64));
    }

    #[test]
    fn http_range_backfills_total_from_206_content_range() {
        let body = vec![b'x'; 16];
        let (uri, _requests) = spawn_mock_http_server(vec![MockResponse::immediate(
            http_206_response(0, 4096, &body),
        )]);
        let mut source = HttpRangeSource::new(uri);
        assert_eq!(source.read_range(ByteRange::suffix_from(0)).unwrap(), body);
        // The 206 Content-Range total satisfies len() without a HEAD request.
        assert_eq!(source.len().unwrap(), Some(4096));
    }

    #[test]
    fn http_range_retries_after_server_error() {
        let body: Vec<u8> = (0..64u8).collect();
        let (uri, requests) = spawn_mock_http_server(vec![
            MockResponse::immediate(http_simple_response("500 Internal Server Error", b"boom")),
            MockResponse::immediate(http_206_response(0, 64, &body)),
        ]);
        let mut source = HttpRangeSource::new(uri);
        source.content_length = Some(64);
        assert_eq!(
            source
                .read_range(ByteRange {
                    start: 0,
                    length: Some(64),
                })
                .unwrap(),
            body
        );
        assert!(recv_request_head(&requests).contains("range: bytes=0-63"));
        assert!(recv_request_head(&requests).contains("range: bytes=0-63"));
    }

    #[test]
    fn http_range_resumes_truncated_body_from_received_offset() {
        let body: Vec<u8> = (0..64u8).collect();
        let (uri, requests) = spawn_mock_http_server(vec![
            // Promises 64 bytes but closes after 32: a body-phase error.
            MockResponse::immediate(http_206_truncated_response(0, 64, 64, &body[..32])),
            MockResponse::immediate(http_206_response(32, 64, &body[32..])),
        ]);
        let mut source = HttpRangeSource::new(uri);
        source.content_length = Some(64);
        assert_eq!(
            source
                .read_range(ByteRange {
                    start: 0,
                    length: Some(64),
                })
                .unwrap(),
            body
        );
        assert!(recv_request_head(&requests).contains("range: bytes=0-63"));
        // The resumed request must start where the truncated body stopped.
        assert!(recv_request_head(&requests).contains("range: bytes=32-63"));
    }

    #[test]
    fn take_prefetch_joins_inflight_thread_instead_of_refetching() {
        let body: Vec<u8> = (0..100u8).collect();
        let (uri, requests) = spawn_mock_http_server(vec![MockResponse::delayed(
            Duration::from_millis(250),
            http_206_response(0, 100, &body),
        )]);
        let mut source = HttpRangeSource::new(uri.clone());
        source.content_length = Some(100);
        let range = ByteRange {
            start: 0,
            length: Some(100),
        };
        source.prefetch = Some(PendingHttpFetch::spawn(uri, range));
        assert_eq!(source.read_range(range).unwrap(), body);
        let _ = recv_request_head(&requests);
        // Joining the pending prefetch must not issue a duplicate download.
        assert!(requests.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn short_prefetch_read_falls_back_to_synchronous_fetch() {
        let body: Vec<u8> = (0..100u8).collect();
        let (uri, requests) = spawn_mock_http_server(vec![
            // Prefetch answer is complete HTTP but shorter than the request.
            MockResponse::immediate(http_206_response(0, 100, &body[..50])),
            MockResponse::immediate(http_206_response(0, 100, &body)),
        ]);
        let mut source = HttpRangeSource::new(uri.clone());
        source.content_length = Some(100);
        let range = ByteRange {
            start: 0,
            length: Some(100),
        };
        source.prefetch = Some(PendingHttpFetch::spawn(uri, range));
        // A short prefetch must trigger the synchronous fallback, not an
        // empty (fake-EOF) read.
        assert_eq!(source.read_range(range).unwrap(), body);
        let _ = recv_request_head(&requests);
        assert!(recv_request_head(&requests).contains("range: bytes=0-99"));
    }

    #[test]
    fn len_retries_head_before_succeeding() {
        let head_response = b"HTTP/1.1 200 OK\r\nContent-Length: 4321\r\nConnection: close\r\n\r\n";
        let (uri, requests) = spawn_mock_http_server(vec![
            MockResponse::immediate(http_simple_response("500 Internal Server Error", b"")),
            MockResponse::immediate(head_response.to_vec()),
        ]);
        let mut source = HttpRangeSource::new(uri);
        assert_eq!(source.len().unwrap(), Some(4321));
        assert!(recv_request_head(&requests).starts_with("head"));
        assert!(recv_request_head(&requests).starts_with("head"));
    }

    #[test]
    fn len_probe_does_not_download_a_body_that_ignores_range() {
        // HEAD is rejected and the origin ignores Range, answering 200 with the
        // whole object. The probe must take the total from Content-Length and
        // leave the payload on the wire instead of buffering the media.
        let payload = vec![b'x'; 512 * 1024];
        let mut raw = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        )
        .into_bytes();
        raw.extend_from_slice(&payload);
        let (uri, requests) = spawn_mock_http_server(vec![
            MockResponse::immediate(
                b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_vec(),
            ),
            MockResponse::immediate(raw),
        ]);

        let mut source = HttpRangeSource::new(uri);
        assert_eq!(source.len().unwrap(), Some(payload.len() as u64));
        assert!(recv_request_head(&requests).starts_with("head"));
        let probe = recv_request_head(&requests);
        assert!(probe.starts_with("get"));
        assert!(probe.contains("range: bytes=0-0"), "probe head: {probe}");
    }

    #[test]
    fn resumed_range_is_bound_to_the_first_response_entity() {
        let body: Vec<u8> = (0..64u8).collect();
        // Same truncated-then-resume shape as the resume test above, but the
        // first response carries a validator the retry has to replay.
        let mut truncated = format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-63/64\r\nContent-Length: 64\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n"
        )
        .into_bytes();
        truncated.extend_from_slice(&body[..32]);
        let (uri, requests) = spawn_mock_http_server(vec![
            MockResponse::immediate(truncated),
            MockResponse::immediate(http_206_response(32, 64, &body[32..])),
        ]);

        let mut source = HttpRangeSource::new(uri);
        source.content_length = Some(64);
        assert_eq!(
            source
                .read_range(ByteRange {
                    start: 0,
                    length: Some(64),
                })
                .unwrap(),
            body
        );

        let first = recv_request_head(&requests);
        assert!(!first.contains("if-range"), "first request: {first}");
        let resumed = recv_request_head(&requests);
        assert!(
            resumed.contains("if-range: \"v1\""),
            "resumed request must replay the validator: {resumed}"
        );
    }

    #[test]
    fn resumed_range_rejects_a_response_served_from_a_different_offset() {
        let body: Vec<u8> = (0..64u8).collect();
        let (uri, _requests) = spawn_mock_http_server(vec![
            MockResponse::immediate(http_206_truncated_response(0, 64, 64, &body[..32])),
            // The resume asked for byte 32; this answers from 0 instead, which
            // would splice mismatched bytes onto the prefix already held.
            MockResponse::immediate(http_206_response(0, 64, &body[..32])),
        ]);

        let mut source = HttpRangeSource::new(uri);
        source.content_length = Some(64);
        let error = source
            .read_range(ByteRange {
                start: 0,
                length: Some(64),
            })
            .unwrap_err();
        assert!(
            error.to_string().contains("served range from 0"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn len_falls_back_to_range_probe_when_head_is_rejected() {
        let (uri, requests) = spawn_mock_http_server(vec![
            MockResponse::immediate(
                b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_vec(),
            ),
            MockResponse::immediate(http_206_response(0, 1234, b"z")),
        ]);
        let mut source = HttpRangeSource::new(uri);
        assert_eq!(source.len().unwrap(), Some(1234));
        assert!(recv_request_head(&requests).starts_with("head"));
        let probe = recv_request_head(&requests);
        assert!(probe.starts_with("get"));
        assert!(probe.contains("range: bytes=0-0"));
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
