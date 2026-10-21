//! Streaming a file while it downloads, as NipaPlay does: an HTTP server on
//! 127.0.0.1 answering `GET /torrent/{task}/stream/{file}/{name}` with byte
//! ranges, reading through librqbit's stream, which waits for the pieces
//! the player asks for. Started on first use, one per process.

use crate::Result;
use crate::engine::{api, engine};
use std::io::{Read, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

static PORT: Mutex<Option<u16>> = Mutex::new(None);

/// The server's port, starting it when it is not running.
pub fn serve() -> Result<u16> {
    let mut port = PORT.lock().map_err(|_| "stream server lock poisoned".to_string())?;
    if let Some(port) = *port {
        return Ok(port);
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| format!("start the stream server: {error}"))?;
    let bound = listener.local_addr().map_err(|error| error.to_string())?.port();
    std::thread::Builder::new()
        .name("erika-torrent-stream".into())
        .spawn(move || {
            for incoming in listener.incoming() {
                match incoming {
                    Ok(mut socket) => {
                        let _ = std::thread::Builder::new().name("erika-torrent-client".into()).spawn(move || {
                            let mut started = false;
                            let answered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| answer(&mut socket, &mut started)));
                            let failure = match answered {
                                Ok(Ok(())) => return,
                                Ok(Err(error)) => error,
                                Err(_) => "the stream failed".to_string(),
                            };
                            eprintln!("erika-torrent: stream: {failure}");
                            // Say why, unless the answer had begun.
                            if !started {
                                let _ = write!(socket, "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{failure}", failure.len());
                            }
                        });
                    }
                    Err(error) => {
                        eprintln!("erika-torrent: stream server: {error}");
                        break;
                    }
                }
            }
        })
        .map_err(|error| format!("start the stream server: {error}"))?;
    *port = Some(bound);
    Ok(bound)
}

/// The bytes a request asks for: from `start`, to `end` when it says.
fn range(request: &str) -> Option<(u64, Option<u64>)> {
    let value = request.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim().eq_ignore_ascii_case("range").then(|| value.trim().to_string())
    })?;
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    let start = start.trim().parse().ok()?;
    let end = if end.trim().is_empty() { None } else { Some(end.trim().parse().ok()?) };
    Some((start, end))
}

/// The task and the file a path names.
fn target(path: &str) -> Option<(usize, usize)> {
    let path = path.split('?').next().unwrap_or(path);
    let mut parts = path.trim_start_matches('/').split('/');
    (parts.next()? == "torrent").then_some(())?;
    let task = parts.next()?.parse().ok()?;
    (parts.next()? == "stream").then_some(())?;
    Some((task, parts.next()?.parse().ok()?))
}

/// Answer one request; `started` turns true once the answer's head is
/// written. librqbit's streams belong to its runtime, so the thread enters
/// it first.
fn answer(socket: &mut TcpStream, started: &mut bool) -> std::result::Result<(), String> {
    let _runtime = engine().runtime.enter();
    let mut request = Vec::with_capacity(1024);
    let mut buffer = [0u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = socket.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if request.len() > 64 * 1024 {
            return Err("request headers too large".into());
        }
    }
    let request = String::from_utf8_lossy(&request).into_owned();
    let mut first = request.lines().next().unwrap_or_default().split_whitespace();
    let (method, path) = (first.next().unwrap_or_default(), first.next().unwrap_or_default());
    if method != "GET" && method != "HEAD" {
        *started = true;
        return write!(socket, "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").map_err(|error| error.to_string());
    }
    let (task, file) = target(path).ok_or_else(|| format!("not a stream: {path}"))?;
    let api = api()?;
    // A task just added checks its files first; the player waits for it.
    let ready_by = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut stream = loop {
        match api.api_stream(task.into(), file) {
            Ok(stream) => break stream,
            Err(error) if format!("{error:#}").contains("initializing") && std::time::Instant::now() < ready_by => {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(error) => return Err(format!("{error:#}")),
        }
    };
    let length = stream.len();
    let asked = range(&request);
    let start = asked.map_or(0, |(start, _)| start);
    let end = asked.and_then(|(_, end)| end).unwrap_or(length.saturating_sub(1)).min(length.saturating_sub(1));
    if length > 0 && (start >= length || start > end) {
        *started = true;
        return write!(socket, "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{length}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").map_err(|error| error.to_string());
    }
    let runtime = &engine().runtime;
    if start > 0 {
        runtime.block_on(stream.seek(SeekFrom::Start(start))).map_err(|error| error.to_string())?;
    }
    let kind = api.torrent_file_mime_type(task.into(), file).unwrap_or("application/octet-stream");
    let size = if length == 0 { 0 } else { end - start + 1 };
    let status = if asked.is_some() { "206 Partial Content" } else { "200 OK" };
    let mut head = format!("HTTP/1.1 {status}\r\nAccept-Ranges: bytes\r\nContent-Type: {kind}\r\nContent-Length: {size}\r\nConnection: close\r\n");
    if asked.is_some() && length > 0 {
        head.push_str(&format!("Content-Range: bytes {start}-{end}/{length}\r\n"));
    }
    head.push_str("\r\n");
    *started = true;
    socket.write_all(head.as_bytes()).map_err(|error| error.to_string())?;
    if method == "HEAD" {
        return Ok(());
    }
    let mut buffer = vec![0u8; 64 * 1024];
    let mut left = size;
    while left > 0 {
        let wanted = buffer.len().min(left as usize);
        let read = runtime.block_on(stream.read(&mut buffer[..wanted])).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        // The player went away: nothing is wrong.
        if socket.write_all(&buffer[..read]).is_err() {
            break;
        }
        left -= read as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_name_a_file_and_a_range() {
        assert_eq!(target("/torrent/3/stream/1/a%20b.mkv"), Some((3, 1)));
        assert_eq!(target("/other/3/stream/1/x"), None);
        assert_eq!(range("GET / HTTP/1.1\r\nRange: bytes=100-\r\n\r\n"), Some((100, None)));
        assert_eq!(range("GET / HTTP/1.1\r\nrange: bytes=5-9\r\n\r\n"), Some((5, Some(9))));
        assert_eq!(range("GET / HTTP/1.1\r\n\r\n"), None);
    }
}
