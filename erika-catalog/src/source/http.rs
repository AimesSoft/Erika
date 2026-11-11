use super::*;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
    },
};
struct Server {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new(fail_season: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                if stopped.load(AtomicOrdering::Acquire) {
                    break;
                }
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut byte = [0];
                while !bytes.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).unwrap() == 0 {
                        break;
                    }
                    bytes.push(byte[0]);
                    assert!(bytes.len() < 8192);
                }
                let request = String::from_utf8(bytes).unwrap();
                let route = request
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .split('?')
                    .next()
                    .unwrap()
                    .to_owned();
                seen.lock().unwrap().push(route.clone());
                let (status, data) = match route.as_str() {
                    "/tmdb/tv/71" => (
                        200,
                        serde_json::json!({"id":71,"name":"TV","seasons":[{"id":72,"season_number":0,"name":"SP"},{"id":75,"season_number":1,"name":"S1"}]}),
                    ),
                    "/tmdb/movie/71" => (200, serde_json::json!({"id":71,"title":"Movie"})),
                    "/tmdb/tv/71/seasons/1" => (
                        200,
                        serde_json::json!({"id":75,"season_number":1,"name":"S1","episodes":[{"id":76,"season_number":1,"episode_number":1,"name":"E1"}]}),
                    ),
                    "/tmdb/tv/71/seasons/0" if !fail_season => (
                        200,
                        serde_json::json!({"id":72,"season_number":0,"name":"SP","episodes":[{"id":73,"season_number":0,"episode_number":1,"name":"SP1"},{"id":74,"season_number":0,"episode_number":2,"name":"SP2"}]}),
                    ),
                    "/tmdb/tv/71/seasons/0" => (503, serde_json::Value::Null),
                    _ => (404, serde_json::Value::Null),
                };
                let body = serde_json::json!({"schema_version":1,"data":data}).to_string();
                write!(stream,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        Self {
            base,
            requests,
            stop,
            thread: Some(thread),
        }
    }
    fn resolve(&self, hint: &MediaHint) -> Result<Resolution> {
        futures_lite::future::block_on(resolve(&Client::new(&self.base, "zh-CN").unwrap(), hint))
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, AtomicOrdering::Release);
        let _ = TcpStream::connect(
            self.base
                .trim_start_matches("http://")
                .trim_end_matches('/'),
        );
        self.thread.take().unwrap().join().unwrap();
    }
}
fn hint() -> MediaHint {
    MediaHint {
        kind: Kind::Episode,
        provider_refs: vec![],
        series_provider_refs: vec![ProviderRef::tmdb(Kind::Tv, 71).unwrap()],
        season_number: Some(1),
        episode_number: Some(1),
        episode_number_end: None,
        ordering: Ordering::Unknown,
    }
}
#[test]
fn explicit_episode_id_is_verified_in_real_seasons_without_search_or_recognition() {
    let server = Server::new(false);
    let mut h = hint();
    h.provider_refs
        .push(ProviderRef::tmdb(Kind::Episode, 74).unwrap());
    let Resolution::Bound { result } = server.resolve(&h).unwrap() else {
        panic!("expected verified episode")
    };
    assert_eq!(result.season.unwrap().node.season_number, Some(0));
    assert_eq!(
        result.items[0].media,
        crate::MediaId::for_ref(&h.provider_refs[0])
    );
    assert_eq!(
        &server.requests.lock().unwrap()[..3],
        [
            "/tmdb/tv/71",
            "/tmdb/tv/71/seasons/1",
            "/tmdb/tv/71/seasons/0"
        ]
    );
}
#[test]
fn unknown_ordering_and_missing_work_stay_pending_without_positional_search() {
    let server = Server::new(false);
    let mut h = hint();
    assert!(matches!(
        server.resolve(&h).unwrap(),
        Resolution::Pending { .. }
    ));
    assert_eq!(*server.requests.lock().unwrap(), ["/tmdb/tv/71"]);
    h.series_provider_refs.clear();
    server.requests.lock().unwrap().clear();
    assert!(matches!(
        server.resolve(&h).unwrap(),
        Resolution::Pending { .. }
    ));
    assert!(server.requests.lock().unwrap().is_empty());
}
#[test]
fn unavailable_is_an_error_not_a_missing_episode_and_movies_keep_their_type() {
    let server = Server::new(true);
    let mut h = hint();
    h.season_number = Some(0);
    h.ordering = Ordering::Tmdb;
    assert_eq!(
        server.resolve(&h).unwrap_err().code,
        crate::client::ErrorCode::Unavailable
    );
    h.kind = Kind::Movie;
    h.season_number = None;
    h.episode_number = None;
    h.series_provider_refs.clear();
    h.provider_refs = vec![ProviderRef::tmdb(Kind::Movie, 71).unwrap()];
    let Resolution::Bound { result } = server.resolve(&h).unwrap() else {
        panic!("expected movie")
    };
    assert_eq!(result.work.node.kind, Kind::Movie);
    assert!(result.season.is_none());
    assert_ne!(
        result.items[0].media,
        crate::MediaId::for_ref(&ProviderRef::tmdb(Kind::Tv, 71).unwrap())
    );
}
