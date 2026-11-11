use super::super::tests as fixture;
use super::*;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
const OWNER: &str = "test.source";

struct Server {
    address: String,
    seen: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new(status: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let requests = seen.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                if stopped.load(Ordering::Acquire) {
                    break;
                }
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut b = [0];
                while !bytes.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut b).unwrap() == 0 {
                        break;
                    }
                    bytes.push(b[0]);
                    assert!(bytes.len() < 8192);
                }
                let request = String::from_utf8(bytes).unwrap();
                let path = request
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .split('?')
                    .next()
                    .unwrap();
                requests.lock().unwrap().push(path.to_owned());
                let (code, data) = if path == "/tmdb/movie/71" {
                    (status, json!({"id":71,"title":"Verified movie"}))
                } else {
                    (404, Value::Null)
                };
                let body = json!({"schema_version":1,"data":data}).to_string();
                write!(stream,"HTTP/1.1 {code} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        Self {
            address,
            seen,
            stop,
            thread: Some(thread),
        }
    }
    fn client(&self) -> erika_catalog::Client {
        erika_catalog::Client::new(&format!("http://{}/", self.address), "zh-CN").unwrap()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(&self.address);
        self.thread.take().unwrap().join().unwrap();
    }
}
fn engine() -> ErikaEngine {
    let mut e = fixture::engine(&["library.sources"]);
    e.plugins.started = true;
    e.state.features.plugins = true;
    e.state.preferences.auto_match = true;
    e.plugins.saved.enabled.push(OWNER.into());
    let pack = e.plugins.find(OWNER).unwrap().pack.clone();
    let runtime = Runtime::start(&pack, "", &[], &e.plugin_snapshot(OWNER)).unwrap();
    e.plugins.runtimes.insert(OWNER.into(), runtime);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !e.plugins.find(OWNER).unwrap().loaded {
        e.poll_plugins();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    e
}
fn publish(e: &mut ErikaEngine, handle: &str) -> StoredItem {
    let source = e
        .source_catalogue_call(OWNER, "registerSource", &fixture::descriptor())
        .unwrap();
    let scan = e
        .source_catalogue_call(OWNER, "beginScan", &json!({"source":source}))
        .unwrap();
    let mut item = fixture::item();
    item["playbackHandle"] = json!(handle);
    e.source_catalogue_call(
        OWNER,
        "upsertItems",
        &json!({"scan":scan["id"],"page":0,"items":[item]}),
    )
    .unwrap();
    e.source_catalogue_call(
        OWNER,
        "finishScan",
        &json!({"scan":scan["id"],"pages":1,"complete":true}),
    )
    .unwrap();
    e.db.plugin_source_items(OWNER, source.as_i64().unwrap())
        .unwrap()
        .remove(0)
}
fn result(e: &ErikaEngine) -> (Work, FileResolution) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut inbox = e.plugins.inbox.lock().unwrap();
        if let Some(index) = inbox
            .iter()
            .position(|d| matches!(d, Done::SourceMetadata(..)))
        {
            let Done::SourceMetadata(work, report) = inbox.remove(index) else {
                unreachable!()
            };
            return (work, report);
        }
        drop(inbox);
        assert!(Instant::now() < deadline, "metadata worker did not return");
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn source_metadata_commits_verified_http_results_and_keeps_retry_state() {
    for status in [200, 503] {
        let server = Server::new(status);
        let mut e = engine();
        e.metadata = server.client();
        let item = publish(&mut e, "original");
        e.drive_source_metadata();
        let (work, report) = result(&e);
        let job = work.job.clone();
        e.source_metadata_finished(work, report);
        let saved = e.db.catalog_job(&job).unwrap().unwrap();
        assert_eq!(job_source(&saved).unwrap(), Some(item.clone()));
        if status == 200 {
            assert_eq!(saved.state, "bound");
            let binding = e.db.catalog_binding(item.file).unwrap().unwrap();
            let node = e.db.catalog_node(&binding.items[0].media).unwrap().unwrap();
            assert_eq!(node.kind, erika_catalog::Kind::Movie);
            assert_eq!(node.title, "Verified movie");
            assert!(!e.plugins.source_metadata.pending.contains_key(&item.url));
        } else {
            assert_eq!(saved.state, "retry");
            assert!(saved.next_attempt > erika_store::now());
            assert!(e.db.catalog_binding(item.file).unwrap().is_none());
            assert_eq!(
                e.plugins.source_metadata.pending[&item.url].1,
                saved.next_attempt
            );
            e.drive_source_metadata();
            assert!(e.plugins.source_metadata.active.is_none());
        }
        assert!(
            server
                .seen
                .lock()
                .unwrap()
                .iter()
                .all(|p| p.starts_with("/tmdb/movie/71"))
        );
    }
}
#[test]
fn cancelled_worker_cannot_complete_or_clear_a_new_attempt() {
    let server = Server::new(200);
    let mut e = engine();
    e.metadata = server.client();
    let item = publish(&mut e, "original");
    e.drive_source_metadata();
    let (old, report) = result(&e);
    e.stop_source_metadata(OWNER);
    assert_eq!(
        e.db.catalog_job(&old.job).unwrap().unwrap().state,
        "cancelled"
    );
    e.resume_source_metadata(OWNER);
    e.drive_source_metadata();
    let current = e
        .plugins
        .source_metadata
        .active
        .as_ref()
        .unwrap()
        .job
        .clone();
    assert_ne!(old.job, current);
    e.source_metadata_finished(old, report);
    assert_eq!(
        e.plugins.source_metadata.active.as_ref().unwrap().job,
        current
    );
    assert!(e.db.catalog_binding(item.file).unwrap().is_none());
    let (new, report) = result(&e);
    e.source_metadata_finished(new, report);
    assert!(e.db.catalog_binding(item.file).unwrap().is_some());
}
#[test]
fn revoked_metadata_permission_and_new_snapshots_reject_completed_network_work() {
    for change in ["disabled", "permission", "setting", "rescan", "manual"] {
        let server = Server::new(200);
        let mut e = engine();
        e.metadata = server.client();
        let item = publish(&mut e, "original");
        e.drive_source_metadata();
        let (work, report) = result(&e);
        let id = work.job.clone();
        match change {
            "disabled" => e.plugins.saved.enabled.clear(),
            "permission" => e.plugins.find_mut(OWNER).unwrap().pack.permissions.clear(),
            "setting" => e.state.preferences.auto_match = false,
            "rescan" => {
                publish(&mut e, "replaced");
            }
            "manual" => {
                e.db.catalog_apply(
                    &[],
                    &[],
                    &[erika_catalog::FileBinding {
                        file: item.file,
                        revision: 0,
                        decision: erika_catalog::Decision::Blocked,
                        items: vec![],
                        evidence: "manual".into(),
                    }],
                    &[],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        e.source_metadata_finished(work, report);
        let state = e.db.catalog_job(&id).unwrap().unwrap().state;
        assert!(
            matches!(state.as_str(), "cancelled" | "stale"),
            "{change}: {state}"
        );
        assert!(
            e.db.catalog_binding(item.file)
                .unwrap()
                .is_none_or(|b| b.decision == erika_catalog::Decision::Blocked)
        );
    }
}
#[test]
fn changed_scan_replaces_its_abandoned_queued_job_and_not_its_file() {
    let mut e = engine();
    let item = publish(&mut e, "original");
    let previous = e.new_source_metadata_job(&item, 0).unwrap();
    let next = publish(&mut e, "new-handle");
    assert_eq!(next.file, item.file);
    let work = e
        .prepare_source_metadata(OWNER, &item.url, erika_store::now())
        .unwrap()
        .unwrap();
    assert_ne!(previous.id, work.job);
    assert_eq!(
        e.db.catalog_job(&previous.id).unwrap().unwrap().state,
        "cancelled"
    );
    assert_eq!(work.entry, next);
}
#[test]
fn a_new_snapshot_replaces_a_future_retry_without_bypassing_the_unchanged_retry() {
    let mut e = engine();
    let item = publish(&mut e, "original");
    let work = e
        .prepare_source_metadata(OWNER, &item.url, erika_store::now())
        .unwrap()
        .unwrap();
    e.db.catalog_job_transition(
        &work.job,
        "running",
        "retry",
        "metadata",
        "稍后重试",
        erika_store::now() + 3600,
    )
    .unwrap();
    assert!(
        e.prepare_source_metadata(OWNER, &item.url, erika_store::now())
            .unwrap()
            .is_none()
    );
    let next = publish(&mut e, "new-handle");
    let replacement = e
        .prepare_source_metadata(OWNER, &item.url, erika_store::now())
        .unwrap()
        .unwrap();
    assert_ne!(work.job, replacement.job);
    assert_eq!(
        e.db.catalog_job(&work.job).unwrap().unwrap().state,
        "cancelled"
    );
    assert_eq!(replacement.entry, next);
}
#[test]
fn two_engines_share_the_file_lease_and_restart_can_recover_expiry() {
    let path = std::env::temp_dir().join(format!("erika-source-jobs-{}", MediaId::new().0));
    std::fs::create_dir(&path).unwrap();
    {
        let mut a = engine();
        let mut b = engine();
        a.db = erika_store::Db::at(&path.join("test.db")).unwrap();
        b.db = erika_store::Db::at(&path.join("test.db")).unwrap();
        let item = publish(&mut a, "original");
        let first = a
            .prepare_source_metadata(OWNER, &item.url, erika_store::now())
            .unwrap()
            .unwrap();
        assert_eq!(
            b.prepare_source_metadata(OWNER, &item.url, erika_store::now())
                .err()
                .unwrap()
                .code,
            "conflict"
        );
        let resumed = b
            .prepare_source_metadata(OWNER, &item.url, erika_store::now() + 301)
            .unwrap()
            .unwrap();
        assert_ne!(first.job, resumed.job);
        assert_eq!(
            a.db.catalog_job(&first.job).unwrap().unwrap().state,
            "stale"
        );
        assert_eq!(resumed.entry, item);
    }
    std::fs::remove_dir_all(path).unwrap();
}
