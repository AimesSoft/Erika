use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;

fn request_text(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut byte = [0];
    while !bytes.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
    }
    let headers = String::from_utf8(bytes.clone()).unwrap();
    let length = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    bytes.extend(body);
    String::from_utf8(bytes).unwrap()
}
fn server(response: Vec<u8>) -> (String, std::thread::JoinHandle<String>) {
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", socket.local_addr().unwrap());
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = socket.accept().unwrap();
        let input = request_text(&mut stream);
        let _ = stream.write_all(&response);
        input
    });
    (url, worker)
}
fn response(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
        body.len()
    )
    .into_bytes();
    bytes.extend_from_slice(body);
    bytes
}
fn execute(request: Request) -> Result<Value, String> {
    native::execute(&request, &AtomicBool::new(false))
}

#[test]
fn validates_complete_requests_without_echoing_secrets() {
    for url in [
        "file:///private",
        "https://u:secret@example.org",
        "https://example.org/#secret",
        "http://x/\nx",
    ] {
        assert!(
            !Request::fetch(url, &Value::Null)
                .err()
                .unwrap()
                .contains("secret")
        );
    }
    for options in [
        json!([]),
        json!({"method":3}),
        json!({"method":"GET\r\nHost:x"}),
        json!({"method":"TRACE"}),
        json!({"body":"data"}),
        json!({"headers":{"X-A":3}}),
        json!({"headers":{"Connection":"close"}}),
        json!({"headers":{"X-A":"a\nb"}}),
    ] {
        assert!(Request::fetch("https://example.org", &options).is_err());
    }
}

#[test]
fn fetch_keeps_post_body_headers_and_non_success_status() {
    let (url, server) = server(response("403 Forbidden", "X-Test: yes\r\n", b"denied"));
    let answer = execute(
        Request::fetch(
            &url,
            &json!({"method":"post", "headers":{"Authorization":"Bearer fixture"}, "body":{"a":1}}),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(answer["status"], 403);
    assert_eq!(answer["ok"], false);
    assert_eq!(answer["headers"]["x-test"], "yes");
    assert_eq!(answer["body"], "denied");
    let input = server.join().unwrap();
    assert!(input.starts_with("POST / HTTP/1.1"));
    assert!(
        input
            .to_ascii_lowercase()
            .contains("authorization: bearer fixture")
    );
    assert!(input.ends_with("{\"a\":1}"));
}

#[test]
fn resolver_exposes_redirect_without_following_but_fetch_follows() {
    let (url, first) = server(response(
        "302 Found",
        "Location: http://127.0.0.1:1/not-contacted\r\n",
        b"",
    ));
    let answer = execute(Request::resolver(&url, &Value::Null).unwrap()).unwrap();
    assert_eq!(answer["status"], 302);
    assert_eq!(answer["url"], url);
    first.join().unwrap();
    let (target, second) = server(response("200 OK", "", b"final"));
    let (url, first) = server(response(
        "302 Found",
        &format!("Location: {target}\r\n"),
        b"",
    ));
    let answer = execute(Request::fetch(&url, &Value::Null).unwrap()).unwrap();
    assert_eq!(answer["body"], "final");
    first.join().unwrap();
    second.join().unwrap();
}

#[test]
fn reads_chunked_bodies_with_a_limit_and_preserves_resolver_lossy_utf8() {
    let (url, server) = server(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n8\r\n12345678\r\n0\r\n\r\n".to_vec());
    let mut request = Request::resolver(&url, &Value::Null).unwrap();
    request.limit = 4;
    assert_eq!(execute(request).unwrap_err(), "插件响应过大");
    server.join().unwrap();
    for resolver in [true, false] {
        let (url, server) = self::server(response("200 OK", "", &[255]));
        let request = if resolver {
            Request::resolver(&url, &Value::Null)
        } else {
            Request::fetch(&url, &Value::Null)
        }
        .unwrap();
        let answer = execute(request);
        if resolver {
            assert_eq!(answer.unwrap()["body"], "�");
        } else {
            assert!(answer.is_err());
        }
        server.join().unwrap();
    }
}

#[test]
fn timeout_and_cancel_do_not_deliver_a_late_completion() {
    for cancel in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/?token=fixture-secret",
            listener.local_addr().unwrap()
        );
        let (ready, started) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            request_text(&mut stream);
            ready.send(()).unwrap();
            let _ = wait.recv_timeout(Duration::from_secs(2));
            let _ = stream.write_all(&response("200 OK", "", b"late"));
        });
        let mut request = Request::resolver(&url, &Value::Null).unwrap();
        request.timeout = Duration::from_millis(50);
        let (send, receive) = mpsc::channel();
        let pending = start(Ok(request), move |answer| {
            let _ = send.send(answer);
        });
        started.recv_timeout(Duration::from_secs(1)).unwrap();
        if cancel {
            drop(pending);
            release.send(()).unwrap();
            assert_eq!(
                receive.recv_timeout(Duration::from_secs(1)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            );
        } else {
            let error = receive
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap_err();
            assert!(!error.contains("fixture-secret"));
            assert!(error.contains("超时"));
            release.send(()).unwrap();
            drop(pending);
        }
        server.join().unwrap();
    }
}
