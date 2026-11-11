use super::super::{Installed, Pack, Plugins, runtime::Runtime};
use super::*;
use crate::player::tests::FakeKernel;
use erika::{Commands, PlayingNote};
use erika_store::plugin_sources::{Descriptor, Item};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const OWNER: &str = "test.source";
const STREAM: &str = "https://example.invalid/movie.mkv?token=test-private";
const SCRIPT: &str = r#"
function pluginResolveUrl(input) {
    storage.set('input', input);
    if (!input.selectedId) {
        return {type:'select', title:'版本', items:[{id:'a',title:'原版'},{id:'b',title:'备选'}]};
    }
    return {type:'play', url:'https://example.invalid/movie.mkv?token=test-private',
            headers:{Authorization:'Bearer test-private'}};
}
function pluginOnEvent(event) {
    if (event.name === 'videoLoaded') storage.set('loaded', event.data);
}
"#;

fn settle(e: &mut ErikaEngine, done: impl Fn(&ErikaEngine) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        e.poll_plugins();
        if done(e) {
            return;
        }
        assert!(Instant::now() < deadline, "plugin did not settle");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn publish(e: &ErikaEngine, owner: &str, item: Option<Item>) -> StoredItem {
    let source =
        e.db.plugin_register_source(
            owner,
            &Descriptor {
                id: "movies".into(),
                server_id: "server-a".into(),
                name: "电影".into(),
                content: "movie".into(),
            },
        )
        .unwrap();
    let scan = e.db.plugin_begin_scan(owner, source).unwrap();
    if let Some(item) = item {
        e.db.plugin_upsert_items(owner, &scan.id, 0, &[item])
            .unwrap();
        e.db.plugin_finish_scan(owner, &scan.id, 1, true).unwrap();
    } else {
        e.db.plugin_finish_scan(owner, &scan.id, 0, true).unwrap();
    }
    e.db.plugin_source_items(owner, source)
        .unwrap()
        .into_iter()
        .next()
        .unwrap_or_else(|| StoredItem {
            source,
            owner: owner.into(),
            generation: scan.id,
            file: 0,
            url: String::new(),
            item: item_data(),
        })
}
fn item_data() -> Item {
    serde_json::from_value(json!({"sourceItemId":"movie-a","serverId":"server-a",
        "mediaSourceId":"original","kind":"movie","title":"来源电影",
        "providerRefs":[{"provider":"tmdb","scope":"","kind":"movie","id":"71"}],
        "playbackHandle":"movie-a-original"}))
    .unwrap()
}
fn add_plugin(e: &mut ErikaEngine, id: &str, script: &str) {
    let pack = Pack::from_pack(
        &json!({"id":id,"name":"来源测试","version":"1","category":"tools",
        "permissions":["url.resolve","library.sources","library.read","storage"]}),
    )
    .unwrap();
    e.plugins.installed.push(Installed {
        pack: pack.clone(),
        dir: Default::default(),
        entries: vec![],
        block_words: vec![],
        loaded: false,
        error: String::new(),
    });
    e.plugins.saved.enabled.push(id.into());
    let runtime = Runtime::start(&pack, script, &[], &e.plugin_snapshot(id)).unwrap();
    e.plugins.runtimes.insert(id.into(), runtime);
    settle(e, |e| e.plugins.find(id).unwrap().loaded);
}
fn fixture() -> (ErikaEngine, Arc<FakeKernel>, StoredItem) {
    let mut e = ErikaEngine::default();
    e.db = erika_store::Db::in_memory();
    e.plugins = Plugins::default();
    e.plugins.started = true;
    e.state.features.plugins = true;
    e.state.features.danmaku = false;
    e.state.preferences.auto_match = false;
    let kernel = Arc::new(FakeKernel::default());
    e.kernel = Some(kernel.clone());
    add_plugin(&mut e, OWNER, SCRIPT);
    let item = publish(&e, OWNER, Some(item_data()));
    (e, kernel, item)
}
fn begin(e: &mut ErikaEngine, item: &StoredItem) -> u64 {
    e.open_file_from(MediaSource::new(&item.url), Some(bern::Entry::Web));
    assert!(e.state.link_resolving);
    settle(e, |e| e.state.link_choosing);
    e.plugins.resolution.as_ref().unwrap().tag
}
fn answer(e: &mut ErikaEngine, tag: u64, value: Value) {
    e.resolver_answered(OWNER, tag, Some(value), None);
}
fn good() -> Value {
    json!({"type":"play","url":STREAM,"headers":{"Authorization":"Bearer test-private"}})
}

#[test]
fn plugins_set_only_safe_headers_and_valid_playback_urls() {
    assert!(allowed_header("Referer", "https://www.bilibili.com/"));
    for key in [
        "Host",
        "Content-Length",
        "Connection",
        "X A",
        "X:A",
        "X-中文",
        "",
    ] {
        assert!(!allowed_header(key, "value"));
    }
    assert!(!allowed_header("X-A", "a\r\nX-B: b"));
    assert!(playback_headers(Some(&json!({"Authorization":"test"}))).is_ok());
    for value in [
        json!([]),
        json!({"Host":"bad"}),
        json!({"X-A":3}),
        json!({"X-A":"x".repeat(65536)}),
    ] {
        assert!(playback_headers(Some(&value)).is_err());
    }
    for url in [
        "file:///private/file",
        "erika-source://1/x",
        "https://u:p@host/video",
        "https://host/video#secret",
        "https://host/\nx",
    ] {
        assert!(!valid_media_url(url));
    }
    assert!(valid_media_url(STREAM));
}

#[test]
fn actual_js_source_playback_preserves_file_resume_entry_and_public_identity() {
    let (mut e, kernel, item) = fixture();
    add_plugin(
        &mut e,
        "test.other",
        "function pluginResolveUrl(input) { storage.set('leaked',input); return null; }",
    );
    e.db.save_progress(item.file, 120., 1000.);
    begin(&mut e, &item);
    assert!(kernel.opens.lock().unwrap().is_empty());
    e.toggle_pause().unwrap();
    e.skip(30.).unwrap();
    e.player_tick().unwrap();
    assert_eq!(e.state.elapsed, 120.);
    assert!(e.state.paused);
    e.save_history();
    assert!(e.db.recents(10).is_empty());
    e.link_chosen("a".into());
    settle(&mut e, |e| {
        e.state.video && e.plugins.saved.storage[OWNER].contains_key("loaded")
    });
    assert_eq!(e.file, Some(item.file));
    assert_eq!(e.current_source().unwrap().url, item.url);
    assert_eq!(e.source.as_ref().unwrap().url, STREAM);
    assert_eq!(e.state.subtitle, PlayingNote::Link);
    assert_eq!(e.queue[0].note, PlayingNote::Link);
    assert_eq!(
        kernel.entries.lock().unwrap().as_slice(),
        &[Some(bern::Entry::Web)]
    );
    assert_eq!(
        kernel.opens.lock().unwrap().as_slice(),
        &[(
            STREAM.into(),
            120.,
            vec![("Authorization".into(), "Bearer test-private".into())]
        )]
    );
    let input: Value = serde_json::from_str(&e.plugins.saved.storage[OWNER]["input"]).unwrap();
    assert_eq!(input["sourceItem"]["file"], item.file);
    assert_eq!(input["playbackHandle"], item.item.playback_handle);
    let public = &e.plugins.saved.storage[OWNER]["loaded"];
    assert!(!public.contains("test-private"));
    assert!(public.contains("erika-source://"));
    assert!(
        !e.plugins
            .saved
            .storage
            .get("test.other")
            .is_some_and(|v| v.contains_key("leaked"))
    );
    *kernel.playback.lock().unwrap() = crate::kernel::Playback {
        source: Some(STREAM.into()),
        loaded: true,
        playing: true,
        position: 150.,
        duration: Some(1000.),
        ..Default::default()
    };
    e.seek_wanted = None;
    e.player_tick().unwrap();
    e.return_to_frame().unwrap();
    let recent = &e.state.recents[0];
    assert_eq!(recent.source.as_ref().unwrap().url, item.url);
    assert_eq!(recent.name, "来源电影");
    assert_eq!(e.db.progress(item.file).unwrap().0, 150.);
    e.play_recent("来源电影".into()).unwrap();
    settle(&mut e, |e| e.state.link_choosing);
    assert_eq!(e.file, Some(item.file));
    assert_eq!(e.state.elapsed, 150.);
    assert_eq!(
        kernel.opens.lock().unwrap().len(),
        1,
        "recent must obtain a fresh URL"
    );
    assert_eq!(e.state.subtitle, PlayingNote::Link);
    e.link_chosen("a".into());
    settle(&mut e, |e| !e.state.link_resolving);
    e.play_queue(1, true).unwrap();
    settle(&mut e, |e| e.state.link_choosing);
    assert_eq!(e.state.subtitle, PlayingNote::Link);
    assert_eq!(e.queue[0].note, PlayingNote::Link);
}

#[test]
fn source_content_unbinding_restores_the_logical_source_note() {
    use erika_catalog::{BindingItem, Decision, FileBinding, Kind, tmdb};
    for external_edit in [false, true] {
        let (mut e, _, item) = fixture();
        let work = tmdb::parse_work(
            Kind::Movie,
            71,
            &json!({"id":71,"title":"规范电影"}),
        )
        .unwrap();
        e.db.catalog_apply(
            &[work.node.clone()],
            &[],
            &[FileBinding {
                file: item.file,
                revision: 0,
                decision: Decision::Manual,
                items: vec![BindingItem {
                    media: work.node.id.clone(),
                    start_seconds: None,
                    end_seconds: None,
                }],
                evidence: "manual".into(),
            }],
            &[],
        )
        .unwrap();
        e.play_media(work.node.id.0, MediaSource::new(&item.url), 0., false)
            .unwrap();
        settle(&mut e, |e| e.state.link_choosing);
        e.link_chosen("a".into());
        settle(&mut e, |e| !e.state.link_resolving);
        assert_eq!(e.state.subtitle, PlayingNote::Movie);
        assert_eq!(e.current_source().unwrap().url, item.url);
        if external_edit {
            let revision = e.db.catalog_revision(item.file).unwrap();
            e.db.catalog_apply(
                &[],
                &[],
                &[FileBinding {
                    file: item.file,
                    revision,
                    decision: Decision::Blocked,
                    items: vec![],
                    evidence: "manual".into(),
                }],
                &[],
            )
            .unwrap();
            e.refresh_playing_catalog();
        } else {
            e.capture_match_revisions().unwrap();
            e.block_catalog_match().unwrap();
            e.refresh_playing_catalog();
        }
        assert_eq!(e.state.subtitle, PlayingNote::Link);
        assert_eq!(e.state.title, "来源电影");
        assert_eq!(e.queue[0].note, PlayingNote::Link);
        assert!(e.media_playing.is_none());
    }
}

#[test]
fn stale_source_results_cannot_open_after_rescan_removal_rebind_stop_or_close() {
    for action in [
        "rescan",
        "remove",
        "rebind",
        "stop",
        "permission",
        "close",
        "new_file",
        "cancel",
    ] {
        let (mut e, kernel, item) = fixture();
        let tag = begin(&mut e, &item);
        match action {
            "rescan" => {
                let mut next = item.item.clone();
                next.playback_handle = "changed".into();
                publish(&e, OWNER, Some(next));
            }
            "remove" => {
                publish(&e, OWNER, None);
            }
            "rebind" => {
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
            "stop" => e.stop_runtime(OWNER),
            "permission" => e
                .plugins
                .find_mut(OWNER)
                .unwrap()
                .pack
                .permissions
                .retain(|p| p != "url.resolve"),
            "close" => {
                e.close_player().unwrap();
            }
            "new_file" => {
                e.open_file(MediaSource::new("https://example.invalid/new.mkv"))
                    .unwrap();
            }
            "cancel" => e.cancel_resolution(),
            _ => unreachable!(),
        }
        let before = kernel.opens.lock().unwrap().len();
        answer(&mut e, tag, good());
        assert_eq!(kernel.opens.lock().unwrap().len(), before, "{action}");
        assert!(!e.state.link_resolving, "{action}");
    }
}

#[test]
fn invalid_source_play_replies_do_not_reach_kernel_or_expose_private_errors() {
    for value in [
        Value::Null,
        json!({"type":"play","url":STREAM,"expiresAt":0}),
        json!({"type":"play","url":STREAM,"headers":{"Host":"evil"}}),
        json!({"type":"play","url":"file:///private/movie"}),
        json!({"type":"error","message":STREAM}),
    ] {
        let (mut e, kernel, item) = fixture();
        let tag = begin(&mut e, &item);
        answer(&mut e, tag, value);
        assert!(kernel.opens.lock().unwrap().is_empty());
        assert!(!e.state.link_resolving);
        assert!(e.state.paused);
        assert!(!format!("{:?}", e.state.notice).contains("test-private"));
    }
}

#[test]
fn a_late_round_cannot_replace_the_active_round_or_its_selection() {
    let (mut e, kernel, item) = fixture();
    let old = begin(&mut e, &item);
    e.link_chosen("a".into());
    let current = e.plugins.resolution.as_ref().unwrap().tag;
    assert_ne!(old, current);
    answer(&mut e, old, good());
    assert!(kernel.opens.lock().unwrap().is_empty());
    assert_eq!(e.plugins.resolution.as_ref().unwrap().tag, current);
    settle(&mut e, |e| !e.state.link_resolving);
    assert_eq!(kernel.opens.lock().unwrap().len(), 1);
}

#[test]
fn source_references_are_owner_scoped_and_never_recursively_resolved() {
    let (mut e, kernel, item) = fixture();
    let foreign = publish(&e, "test.other", Some(item_data()));
    for source in [foreign.source, item.source] {
        assert!(e.resolve_link("https://example.invalid/watch", "页面"));
        let tag = e.plugins.resolution.as_ref().unwrap().tag;
        answer(
            &mut e,
            tag,
            json!({"type":"play","sourceItem":{"source":source,"sourceItemId":"movie-a","mediaSourceId":"original"}}),
        );
        assert!(kernel.opens.lock().unwrap().is_empty());
        if source == item.source {
            assert_eq!(e.current_source().unwrap().url, item.url);
            let nested = e.plugins.resolution.as_ref().unwrap().tag;
            answer(
                &mut e,
                nested,
                json!({"type":"play","sourceItem":{"source":source,"sourceItemId":"movie-a","mediaSourceId":"original"}}),
            );
        }
        assert!(e.plugins.resolution.is_none());
    }
}

fn replace_script(e: &mut ErikaEngine, script: &str) {
    let pack = e.plugins.find(OWNER).unwrap().pack.clone();
    let runtime = Runtime::start(&pack, script, &[], &e.plugin_snapshot(OWNER)).unwrap();
    e.plugins.runtimes.insert(OWNER.into(), runtime);
    e.plugins.find_mut(OWNER).unwrap().loaded = false;
    settle(e, |e| e.plugins.find(OWNER).unwrap().loaded);
}
fn settle_wake(e: &mut ErikaEngine, done: impl Fn(&ErikaEngine) -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        e.plugins_wake();
        if done(e) {
            return;
        }
        assert!(
            Instant::now() < until,
            "engine did not wake: {:?}",
            e.state.notice
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn pending_promise_is_not_a_null_reply_and_its_real_answer_opens_once() {
    let (mut e, kernel, item) = fixture();
    replace_script(
        &mut e,
        &format!(
            "async function pluginResolveUrl(input) {{ await Promise.resolve(); return {}; }}",
            good()
        ),
    );
    e.open_file_from(MediaSource::new(&item.url), Some(bern::Entry::Web));
    settle_wake(&mut e, |e| !e.state.link_resolving);
    assert_eq!(kernel.opens.lock().unwrap().len(), 1);
    assert_eq!(e.file, Some(item.file));
    assert_eq!(e.current_source().unwrap().url, item.url);
}

#[test]
fn stalled_promise_times_out_and_late_reply_cannot_open_or_stop_a_new_round() {
    let (mut e, kernel, item) = fixture();
    replace_script(
        &mut e,
        "function pluginResolveUrl() { storage.set('waiting',true); return new Promise(function(){}); }",
    );
    e.open_file_from(MediaSource::new(&item.url), Some(bern::Entry::Web));
    settle(&mut e, |e| {
        e.plugins
            .saved
            .storage
            .get(OWNER)
            .is_some_and(|s| s.contains_key("waiting"))
    });
    let tag = e.plugins.resolution.as_ref().unwrap().tag;
    assert!(e.state.link_resolving);
    // Exercise the real timer/inbox path, not just an explicit check call.
    let expire = completion(&e.plugins.inbox, move |()| Done::ResolveTimeout(tag));
    let r = e.plugins.resolution.as_mut().unwrap();
    r.expires = Some(web_time::Instant::now() + Duration::from_millis(5));
    r.deadline = Some(Deadline::after(
        Duration::from_millis(5),
        move || expire(()),
    ));
    settle_wake(&mut e, |e| !e.state.link_resolving);
    assert!(format!("{:?}", e.state.notice).contains("超时"));
    answer(&mut e, tag, good());
    assert!(kernel.opens.lock().unwrap().is_empty());
    e.open_file_from(MediaSource::new(&item.url), Some(bern::Entry::Web));
    let fresh = e.plugins.resolution.as_ref().unwrap().tag;
    e.plugins
        .inbox
        .lock()
        .unwrap()
        .push(Done::ResolveTimeout(tag));
    e.plugins_wake();
    assert_eq!(e.plugins.resolution.as_ref().unwrap().tag, fresh);
    answer(&mut e, tag, good());
    assert_eq!(e.plugins.resolution.as_ref().unwrap().tag, fresh);
    e.cancel_resolution();
}

#[test]
fn user_selection_pauses_deadline_but_replaced_runtime_invalidates_it() {
    let (mut e, kernel, item) = fixture();
    let tag = begin(&mut e, &item);
    assert!(e.plugins.resolution.as_ref().unwrap().expires.is_none());
    assert!(e.plugins.resolution.as_ref().unwrap().deadline.is_none());
    e.link_chosen("not-offered".into());
    assert_eq!(e.plugins.resolution.as_ref().unwrap().tag, tag);
    assert!(e.state.link_choosing);
    replace_script(&mut e, SCRIPT);
    answer(&mut e, tag, good());
    assert!(kernel.opens.lock().unwrap().is_empty());
    assert!(!e.state.link_resolving);
}

/// A local response held until the test releases it; no public service or account.
fn held_http(
    body: &'static str,
) -> (
    String,
    std::sync::mpsc::Receiver<()>,
    std::sync::mpsc::Sender<()>,
    std::thread::JoinHandle<()>,
) {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (ready, started) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut byte = [0];
        while !bytes.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            bytes.push(byte[0]);
        }
        ready.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    });
    (url, started, release, worker)
}

#[test]
fn actual_http_round_waits_then_revalidates_source_before_playing() {
    for remove in [false, true] {
        let (mut e, kernel, item) = fixture();
        let (url, started, release, server) = held_http("fixture-response");
        replace_script(
            &mut e,
            &format!(
                r#"
            function pluginResolveUrl(input) {{
                if (!input.response) return {{type:'request',request:{{url:{url}}}}};
                storage.set('response',input.response.body);
                return {play};
            }}"#,
                url = json!(url),
                play = good()
            ),
        );
        e.open_file_from(MediaSource::new(&item.url), Some(bern::Entry::Web));
        settle_wake(&mut e, |e| {
            e.plugins
                .resolution
                .as_ref()
                .is_some_and(|r| r.request.is_some())
        });
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(kernel.opens.lock().unwrap().is_empty());
        if remove {
            publish(&e, OWNER, None);
        }
        release.send(()).unwrap();
        server.join().unwrap();
        settle_wake(&mut e, |e| !e.state.link_resolving);
        assert_eq!(kernel.opens.lock().unwrap().len(), usize::from(!remove));
        if !remove {
            assert_eq!(
                e.plugins.saved.storage[OWNER]["response"],
                r#""fixture-response""#
            );
        }
    }
}

#[test]
fn net_fetch_completion_cannot_settle_a_restarted_runtime_with_the_same_request_id() {
    let (mut e, _, _) = fixture();
    e.plugins
        .find_mut(OWNER)
        .unwrap()
        .pack
        .permissions
        .push("network".into());
    let (old_url, old_started, old_release, old_server) = held_http("old");
    let (new_url, new_started, new_release, new_server) = held_http("new");
    let script = |url: &str| {
        format!(
            r#"async function pluginHandleUIAction() {{
        const r=await net.fetch({}); storage.set('body',r.body);
    }}"#,
            json!(url)
        )
    };
    replace_script(&mut e, &script(&old_url));
    e.invoke_action(OWNER, "fetch");
    settle_wake(&mut e, |e| !e.plugins.runtimes[OWNER].fetches.is_empty());
    old_started.recv_timeout(Duration::from_secs(2)).unwrap();
    let old_instance = e.plugins.runtimes[OWNER].instance;
    let request = *e.plugins.runtimes[OWNER].fetches.keys().next().unwrap();
    replace_script(&mut e, &script(&new_url));
    e.invoke_action(OWNER, "fetch");
    settle_wake(&mut e, |e| !e.plugins.runtimes[OWNER].fetches.is_empty());
    new_started.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(e.plugins.runtimes[OWNER].fetches.contains_key(&request));
    assert_ne!(old_instance, e.plugins.runtimes[OWNER].instance);
    e.plugin_fetched(OWNER, old_instance, request, Ok(json!({"body":"stale"})));
    assert!(e.plugins.runtimes[OWNER].fetches.contains_key(&request));
    assert!(
        !e.plugins
            .saved
            .storage
            .get(OWNER)
            .is_some_and(|s| s.contains_key("body"))
    );
    old_release.send(()).unwrap();
    old_server.join().unwrap();
    new_release.send(()).unwrap();
    new_server.join().unwrap();
    settle_wake(&mut e, |e| {
        e.plugins
            .saved
            .storage
            .get(OWNER)
            .is_some_and(|s| s.contains_key("body"))
    });
    assert_eq!(e.plugins.saved.storage[OWNER]["body"], r#""new""#);
}

#[test]
fn internal_addresses_never_become_filename_recognition_jobs() {
    let (mut e, _, item) = fixture();
    e.state.preferences.auto_match = true;
    e.open_file(MediaSource::new(&item.url)).unwrap();
    assert!(
        e.db.catalog_file_job(item.file, erika_store::now())
            .unwrap()
            .is_none()
    );
    // Metadata work was queued, not a DDP/filename request for opaque hex bytes.
    e.state.preferences.auto_match = false;
    e.close_player().unwrap();
}
