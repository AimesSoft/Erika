use super::*;
use std::time::{Duration, Instant};

pub(super) fn engine(permissions: &[&str]) -> ErikaEngine {
    let mut e = ErikaEngine::default();
    e.db = erika_store::Db::in_memory();
    e.plugins = Plugins::default();
    e.state.preferences.auto_match = false;
    for id in ["test.source", "test.other"] {
        let pack = Pack::from_pack(&json!({"id":id,"name":"来源测试","version":"1","category":"tools","permissions":permissions})).unwrap();
        e.plugins.installed.push(super::super::Installed {
            pack,
            dir: PathBuf::new(),
            entries: vec![],
            block_words: vec![],
            loaded: false,
            error: String::new(),
        });
    }
    e
}
pub(super) fn descriptor() -> Value {
    json!({"id":"movies","serverId":"server-a","name":"电影","content":"movie"})
}
pub(super) fn item() -> Value {
    json!({"sourceItemId":"movie-a","serverId":"server-a","mediaSourceId":"original","kind":"movie","providerRefs":[{"provider":"tmdb","scope":"","kind":"movie","id":"71"}],"title":"Movie","playbackHandle":"movie-a-original"})
}
fn call(e: &mut ErikaEngine, method: &str, value: Value) -> Result<Value> {
    e.catalogue_call("test.source", method, &value)
}

#[test]
fn native_boundary_checks_source_permission_ownership_and_request_shapes() {
    let mut e = engine(&["library.read", "library.write"]);
    for method in [
        "registerSource",
        "beginScan",
        "upsertItems",
        "finishScan",
        "getSourceItems",
        "getSourceItem",
        "getSourceStatus",
    ] {
        assert_eq!(
            call(&mut e, method, descriptor()).unwrap_err().code,
            "permission_denied"
        );
    }
    e.plugins.installed[0]
        .pack
        .permissions
        .push("library.sources".into());
    e.plugins.installed[1]
        .pack
        .permissions
        .push("library.sources".into());
    let source = call(&mut e, "registerSource", descriptor()).unwrap();
    for method in ["beginScan", "getSourceItems"] {
        assert!(
            e.catalogue_call("test.other", method, &json!({"source":source}))
                .is_err()
        );
    }
    for value in [
        json!({"source":source,"owner":"test.other"}),
        json!({"source":"1"}),
        json!({"source":-1}),
    ] {
        assert!(call(&mut e, "beginScan", value).is_err());
    }
    let scan = call(&mut e, "beginScan", json!({"source":source})).unwrap();
    for page in [json!(-1), json!(4294967296_u64), json!(0.5), Value::Null] {
        assert!(
            call(
                &mut e,
                "upsertItems",
                json!({"scan":scan["id"],"items":[item()],"page":page})
            )
            .is_err()
        );
    }
    let mut leaking = item();
    leaking["url"] = json!("https://private.example/movie?token=secret");
    assert!(
        call(
            &mut e,
            "upsertItems",
            json!({"scan":scan["id"],"items":[leaking],"page":0})
        )
        .is_err()
    );
    call(
        &mut e,
        "upsertItems",
        json!({"scan":scan["id"],"items":[item()],"page":0}),
    )
    .unwrap();
    assert!(
        call(
            &mut e,
            "finishScan",
            json!({"scan":scan["id"],"pages":1,"complete":"true"})
        )
        .is_err()
    );
    call(
        &mut e,
        "finishScan",
        json!({"scan":scan["id"],"pages":1,"complete":true}),
    )
    .unwrap();
    assert_eq!(
        call(&mut e, "getSourceItems", json!({"source":source})).unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn actual_js_source_scan_waits_for_commits_and_keeps_previous_catalogue_on_cancel() {
    let mut e = engine(&["library.sources", "storage"]);
    let script = format!(
        r#"
        async function pluginOnInitialize() {{
            const source = await library.registerSource({descriptor});
            const first = await library.beginScan(source);
            const page = await library.upsertItems(first.id, [{item}], 0);
            const retry = await library.upsertItems(first.id, [{item}], 0);
            storage.set('pages', [page.nextPage, retry.nextPage]);
            storage.set('before', (await library.getSourceItems(source)).items);
            try {{ await library.finishScan(first.id, {{pages:2, complete:true}}); }}
            catch (e) {{ storage.set('conflict', e.code); }}
            await library.finishScan(first.id, {{pages:1, complete:true}});
            const complete = await library.getSourceItems(source);
            storage.set('complete', complete);
            const cancelled = await library.beginScan(source);
            await library.finishScan(cancelled.id, {{pages:0, complete:false}});
            storage.set('cancelled', await library.getSourceItems(source));
            const stale = await library.beginScan(source);
            const current = await library.beginScan(source);
            try {{ await library.upsertItems(stale.id, [{item}], 0); }}
            catch (e) {{ storage.set('stale', e.code); }}
            await library.finishScan(current.id, {{pages:0, complete:true}});
            storage.set('empty', (await library.getSourceItems(source)).items);
            const batch = await library.beginScan(source);
            const a = {item};
            const b = Object.assign({{}}, a, {{mediaSourceId:'second', playbackHandle:'movie-a-second'}});
            await library.upsertItems(batch.id, [a,b], 0);
            await library.finishScan(batch.id, {{pages:1, complete:true}});
            const p1 = await library.getSourceItems(source, {{limit:1}});
            const p2 = await library.getSourceItems(source, {{limit:1,cursor:p1.nextCursor}});
            storage.set('readPages', [p1.items.length,p2.items.length,p2.nextCursor]);
            storage.set('versions', [p1.items[0].item.mediaSourceId,p2.items[0].item.mediaSourceId]);
            const ref = {{source:source,sourceItemId:a.sourceItemId,mediaSourceId:a.mediaSourceId}};
            storage.set('direct', await library.getSourceItem(ref));
            storage.set('status', await library.getSourceStatus(ref));
            try {{ await library.getSourceItems(source, {{limit:1001}}); }}
            catch (e) {{ storage.set('badLimit',e.code); }}
            const replace = await library.beginScan(source);
            await library.finishScan(replace.id, {{pages:0,complete:true}});
            try {{ await library.getSourceItems(source, {{cursor:p1.nextCursor}}); }}
            catch (e) {{ storage.set('oldCursor',e.code); }}
            storage.set('missing', await library.getSourceStatus(ref));
            storage.set('done', true);
        }}
    "#,
        descriptor = descriptor(),
        item = item()
    );
    let pack = e.plugins.installed[0].pack.clone();
    let runtime = Runtime::start(&pack, &script, &[], &e.plugin_snapshot(&pack.id)).unwrap();
    e.plugins.runtimes.insert(pack.id.clone(), runtime);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !e
        .plugins
        .saved
        .storage
        .get(&pack.id)
        .is_some_and(|s| s.get("done").is_some_and(|v| v == "true"))
    {
        e.poll_plugins();
        assert!(
            Instant::now() < deadline,
            "source bridge failed: {:?}",
            e.plugins.installed[0].error
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let saved = &e.plugins.saved.storage[&pack.id];
    assert_eq!(saved["pages"], "[1,1]");
    assert_eq!(saved["before"], "[]");
    assert_eq!(saved["conflict"], "\"conflict\"");
    assert_eq!(saved["stale"], "\"conflict\"");
    assert_eq!(saved["complete"], saved["cancelled"]);
    assert_eq!(saved["empty"], "[]");
    let complete: Value = serde_json::from_str(&saved["complete"]).unwrap();
    assert_eq!(complete["items"][0]["owner"], "test.source");
    assert!(complete["items"][0]["file"].as_i64().unwrap() > 0);
    assert!(
        complete["items"][0]["url"]
            .as_str()
            .unwrap()
            .starts_with("erika-source://")
    );
    assert!(!complete.to_string().contains("https://"));
    assert_eq!(saved["readPages"], "[1,1,null]");
    assert_eq!(saved["versions"], r#"["original","second"]"#);
    assert_eq!(saved["badLimit"], r#""invalid""#);
    assert_eq!(saved["oldCursor"], r#""conflict""#);
    assert_eq!(saved["missing"], "null");
    let direct: Value = serde_json::from_str(&saved["direct"]).unwrap();
    let status: Value = serde_json::from_str(&saved["status"]).unwrap();
    assert_eq!(direct, status["item"]);
    assert_eq!(status["revision"], 0);
    assert!(status["job"].is_null());
    assert!(status["binding"].is_null());
}
