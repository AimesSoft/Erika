use super::*;
use futures_lite::future::block_on;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    thread,
    time::Instant,
};

fn serve(
    action: impl FnOnce(TcpStream, String) + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n") {
            assert!(request.len() < 16 * 1024);
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        action(stream, String::from_utf8(request).unwrap());
    });
    (url, worker)
}

fn read(url: &str, limit: usize) -> Result<Response> {
    block_on(get(url, &[], Duration::from_secs(2), limit, false))
}

#[test]
fn get_preserves_status_headers_and_json_without_exposing_credentials() {
    let (url, worker) = serve(|mut stream, request| {
        assert!(request.starts_with("GET /calendar?language=zh-CN HTTP/1.1\r\n"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("x-test: calendar\r\n")
        );
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
        stream.write_all(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 17\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}").unwrap();
    });
    let response = block_on(get(
        &format!("{url}/calendar?language=zh-CN"),
        &[("X-Test", "calendar")],
        Duration::from_secs(2),
        1024,
        false,
    ))
    .unwrap();
    assert_eq!(response.status, 429);
    assert_eq!(response.retry_after, Some(17));
    assert_eq!(response.json().unwrap()["ok"], true);
    worker.join().unwrap();
}

#[test]
fn length_and_chunked_bodies_are_bounded() {
    for wire in [
        "HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\n123456789",
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\n1234\r\n5\r\n56789\r\n0\r\n\r\n",
    ] {
        let (url, worker) = serve(move |mut stream, _| {
            let _ = stream.write_all(wire.as_bytes());
        });
        assert!(read(&url, 8).is_err());
        worker.join().unwrap();
    }
    let (url, worker) = serve(|mut stream, _| {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\n12345678")
            .unwrap();
    });
    assert_eq!(read(&url, 8).unwrap().text().unwrap(), "12345678");
    worker.join().unwrap();
}

#[test]
fn timeout_covers_waiting_for_headers_and_waiting_for_body() {
    for send_headers in [false, true] {
        let (release, wait) = mpsc::channel();
        let (url, worker) = serve(move |mut stream, _| {
            if send_headers {
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\na");
            }
            let _ = wait.recv_timeout(Duration::from_secs(3));
        });
        let start = Instant::now();
        let result = block_on(get(&url, &[], Duration::from_millis(100), 1024, false));
        release.send(()).unwrap();
        worker.join().unwrap();
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}

#[test]
fn redirects_are_an_explicit_caller_policy() {
    let (destination, target) = serve(|mut stream, _| {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .unwrap();
    });
    let redirect = move |mut stream: TcpStream, _: String| {
        write!(stream, "HTTP/1.1 302 Found\r\nLocation: {destination}/target\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    };
    let (url, worker) = serve(redirect);
    assert_eq!(
        block_on(get(&url, &[], Duration::from_secs(2), 1024, true))
            .unwrap()
            .text()
            .unwrap(),
        "ok"
    );
    worker.join().unwrap();
    target.join().unwrap();

    let (url, worker) = serve(|mut stream, _| {
        stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    });
    let result = read(&url, 1024);
    assert!(result.is_err() || result.unwrap().status == 302);
    worker.join().unwrap();
}

#[test]
fn text_is_strict_utf8_and_errors_do_not_repeat_payloads() {
    let (url, worker) = serve(|mut stream, _| {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\n\xff")
            .unwrap();
    });
    assert!(read(&url, 8).unwrap().text().is_err());
    worker.join().unwrap();
    let error = Response {
        status: 200,
        retry_after: None,
        body: b"private-value".to_vec(),
    }
    .json()
    .unwrap_err();
    assert!(!error.message.contains("private-value"));
    for url in [
        "file:///private-value",
        "https://user:private-value@example.com/",
        "https://example.com/#private-value",
    ] {
        let error = read(url, 8).err().unwrap();
        assert!(!error.message.contains("private-value"));
    }
    assert!(block_on(get("https://example.com/", &[], Duration::ZERO, 8, false)).is_err());
    assert!(read("https://example.com/", 0).is_err());
}
