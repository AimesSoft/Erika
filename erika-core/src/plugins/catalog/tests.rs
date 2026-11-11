use super::*;
use erika_catalog::{BindingItem, Decision, FileBinding, MediaNode, ProviderLink, tmdb};
use std::time::{Duration, Instant};

fn engine(permissions: &[&str]) -> ErikaEngine {
    let mut e = ErikaEngine::default();
    e.db = erika_store::Db::in_memory();
    e.plugins = Plugins::default();
    let pack = Pack::from_pack(&json!({"id":"test.catalogue","name":"媒体测试","version":"1","category":"tools","permissions":permissions})).unwrap();
    e.plugins.installed.push(super::super::Installed {
        pack,
        dir: PathBuf::new(),
        entries: vec![],
        block_words: vec![],
        loaded: false,
        error: String::new(),
    });
    e
}
fn movie(id: u64) -> MediaNode {
    tmdb::parse_work(Kind::Movie, id, &json!({"id":id,"title":"Journey"}))
        .unwrap()
        .node
}
fn file(e: &ErikaEngine) -> i64 {
    e.db.opened(
        "file:///videos/Journey.mkv",
        Some(&erika_store::Fingerprint {
            md5: "1".repeat(32),
            size: 100,
        }),
    )
    .unwrap()
}
fn binding(file: i64, node: &MediaNode) -> FileBinding {
    FileBinding {
        file,
        items: vec![BindingItem {
            media: node.id.clone(),
            start_seconds: None,
            end_seconds: None,
        }],
        decision: Decision::Manual,
        revision: 0,
        evidence: "test".into(),
    }
}
fn call(e: &mut ErikaEngine, method: &str, input: Value) -> Result<Value> {
    e.catalogue_call("test.catalogue", method, &input)
}

#[test]
fn native_boundary_enforces_permissions_even_for_forged_js_calls() {
    let mut e = engine(&[]);
    let node = movie(71);
    let f = file(&e);
    e.db.catalog_save_nodes(&[node.clone()]).unwrap();
    let before = e.db.catalog_export().unwrap();
    for (method, input) in [
        ("getMedia", json!({"id":node.id})),
        ("getFiles", json!(node.id)),
        ("getWatched", json!(node.id)),
        (
            "setWatched",
            json!({"mediaId":node.id,"watched":true,"revision":0}),
        ),
        (
            "apply",
            json!({"request":MediaId::new(),"preview":false,"batch":CatalogBatch {bindings:vec![binding(f,&node)],..Default::default()}}),
        ),
    ] {
        assert_eq!(
            call(&mut e, method, input).unwrap_err().code,
            "permission_denied"
        );
    }
    assert!(
        e.plugin_snapshot("test.catalogue")
            .get("library.mediaContext")
            .is_none()
    );
    assert_eq!(e.db.catalog_export().unwrap(), before);
}

#[test]
fn v2_keeps_typed_ids_and_acknowledges_atomic_previews_commits_and_cas() {
    let mut e = engine(&["library.read", "library.write"]);
    let node = movie(71);
    let f = file(&e);
    e.db.catalog_save_nodes(&[node.clone()]).unwrap();
    let read = call(&mut e, "getMedia", json!({"reference":node.reference})).unwrap();
    assert_eq!(read[0]["id"], json!(node.id));
    let mut wrong = node.reference.clone().unwrap();
    wrong.kind = Kind::Tv;
    assert_eq!(
        call(&mut e, "getMedia", json!({"reference":wrong})).unwrap(),
        json!([])
    );
    let request = MediaId::new();
    let batch = CatalogBatch {
        bindings: vec![binding(f, &node)],
        ..Default::default()
    };
    let preview = call(
        &mut e,
        "apply",
        json!({"request":request,"preview":true,"batch":batch}),
    )
    .unwrap();
    assert_eq!(preview["preview"], true);
    assert!(e.db.catalog_binding(f).unwrap().is_none());
    let receipt = call(
        &mut e,
        "apply",
        json!({"request":request,"preview":false,"batch":batch}),
    )
    .unwrap();
    assert_eq!(e.db.catalog_revision(f).unwrap(), 1);
    assert_eq!(
        call(
            &mut e,
            "apply",
            json!({"request":request,"preview":false,"batch":batch})
        )
        .unwrap(),
        receipt
    );
    assert_eq!(e.db.catalog_revision(f).unwrap(), 1);
    let files = call(&mut e, "getFiles", json!(node.id)).unwrap();
    assert_eq!(files[0]["file"], f);
    let w = call(
        &mut e,
        "setWatched",
        json!({"mediaId":node.id,"watched":false,"revision":0}),
    )
    .unwrap();
    assert_eq!(w["revision"], 1);
    assert_eq!(w["watched"], false);
    assert_eq!(w["manual"], true);
    assert_eq!(
        call(
            &mut e,
            "setWatched",
            json!({"mediaId":node.id,"watched":true,"revision":0})
        )
        .unwrap_err()
        .code,
        "conflict"
    );
    assert_eq!(call(&mut e, "getWatched", json!(node.id)).unwrap(), w);
    call(&mut e, "undo", receipt["receipt"]["change"]["id"].clone()).unwrap();
    assert!(e.db.catalog_binding(f).unwrap().is_none());
}

#[test]
fn media_context_contains_ancestors_without_credentials_and_old_api_never_uses_tmdb_numbers() {
    let mut e = engine(&["library.read"]);
    let node = movie(71);
    let f = file(&e);
    e.db.catalog_apply(&[node.clone()], &[], &[binding(f, &node)], &[])
        .unwrap();
    e.file = Some(f);
    e.source = Some(MediaSource::new(
        "https://private.example/video?token=do-not-export",
    ));
    let context = e.plugin_media_context().unwrap();
    assert_eq!(context["binding"]["items"][0]["media"], json!(node.id));
    assert_eq!(context["nodes"][0]["reference"]["kind"], "movie");
    assert!(!context.to_string().contains("do-not-export"));
    assert_eq!(e.library_for_plugins(), json!([]));
    let link = ProviderLink {
        media: node.id.clone(),
        reference: ProviderRef {
            provider: Provider::Dandanplay,
            scope: String::new(),
            kind: Kind::Subject,
            id: "901".into(),
        },
        relation: Relation::Exact,
        coverage: vec![],
        confirmed: true,
        manual: true,
        evidence: "manual".into(),
        revision: 0,
    };
    e.db.catalog_link(&link).unwrap();
    assert_eq!(e.library_for_plugins()[0]["id"], 901);
    let snapshot = e.plugin_snapshot("test.catalogue");
    assert_eq!(snapshot["library.mediaContext"]["value"]["file"], f);
    let another = movie(72);
    e.db.catalog_save_nodes(&[another.clone()]).unwrap();
    let mut ambiguous = link;
    ambiguous.media = another.id;
    e.db.catalog_link(&ambiguous).unwrap();
    assert_eq!(e.library_for_plugins(), json!([]));
}

#[test]
fn playback_events_use_the_same_permission_filtered_context_as_the_js_snapshot() {
    for read in [false, true] {
        let permissions = if read {
            vec!["library.read", "storage"]
        } else {
            vec!["storage"]
        };
        let mut e = engine(&permissions);
        let node = movie(71);
        let f = file(&e);
        e.db.catalog_apply(&[node.clone()], &[], &[binding(f, &node)], &[])
            .unwrap();
        e.file = Some(f);
        let script = r#"
            function pluginOnEvent(event) {
                storage.set('event', event);
                if (event.data.mediaContext) storage.set('snapshot', library.getMediaContext());
                storage.set('done', true);
            }
        "#;
        let pack = e.plugins.installed[0].pack.clone();
        let runtime = Runtime::start(&pack, script, &[], &e.plugin_snapshot(&pack.id)).unwrap();
        e.plugins.runtimes.insert(pack.id.clone(), runtime);
        e.plugin_event("mediaChanged", json!({"title":"Movie"}));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !e
            .plugins
            .saved
            .storage
            .get(&pack.id)
            .is_some_and(|s| s.get("done").is_some_and(|v| v == "true"))
        {
            e.poll_plugins();
            assert!(Instant::now() < deadline, "plugin event did not settle");
            std::thread::sleep(Duration::from_millis(5));
        }
        let saved = &e.plugins.saved.storage[&pack.id];
        let event: Value = serde_json::from_str(&saved["event"]).unwrap();
        assert_eq!(event["name"], "mediaChanged");
        if read {
            let snapshot: Value = serde_json::from_str(&saved["snapshot"]).unwrap();
            assert_eq!(event["data"]["mediaContext"], snapshot);
            assert_eq!(snapshot["playing"], json!(node.id));
            assert_eq!(snapshot["revision"], 1);
        } else {
            assert!(event["data"].get("mediaContext").is_none());
            assert!(saved.get("snapshot").is_none());
        }
    }
    let mut data = json!({"title":"Movie"});
    attach_context(
        &mut data,
        &json!({"library.mediaContext":{"ok":false,"error":{"code":"storage","message":"读取失败"}}}),
    );
    assert!(data.get("mediaContext").is_none());
    assert_eq!(data["mediaContextError"]["code"], "storage");
}

#[test]
fn actual_js_bridge_waits_for_storage_and_propagates_structured_errors() {
    let mut e = engine(&["library.read", "library.write", "storage"]);
    let node = movie(71);
    let f = file(&e);
    e.db.catalog_save_nodes(&[node.clone()]).unwrap();
    let request = MediaId::new();
    let edit = json!({"request":request,"preview":false,"batch":CatalogBatch {bindings:vec![binding(f,&node)],..Default::default()}});
    let script = format!(
        r#"
        async function pluginOnInitialize() {{
            storage.set('version', library.apiVersion);
            const before = await library.getWatched({media});
            storage.set('before', before);
            const receipt = await library.apply({edit});
            const files = await library.getFiles({media});
            storage.set('after', {{receipt, files}});
            try {{ await library.setWatched({{mediaId:{media},watched:true,revision:99}}); }}
            catch (e) {{ storage.set('error', {{code:e.code,message:e.message}}); }}
            await library.undo(receipt.receipt.change.id);
            storage.set('done', true);
        }}
    "#,
        media = json!(node.id)
    );
    let pack = e.plugins.installed[0].pack.clone();
    let runtime = Runtime::start(&pack, &script, &[], &e.plugin_snapshot(&pack.id)).unwrap();
    e.plugins.runtimes.insert(pack.id.clone(), runtime);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        e.poll_plugins();
        if e.plugins
            .saved
            .storage
            .get(&pack.id)
            .is_some_and(|s| s.get("done").is_some_and(|v| v == "true"))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "plugin failed: {:?}",
            e.plugins.installed[0].error
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let saved = &e.plugins.saved.storage[&pack.id];
    assert_eq!(saved["before"], "null");
    assert_eq!(saved["version"], "2");
    let after: Value = serde_json::from_str(&saved["after"]).unwrap();
    assert_eq!(after["files"][0]["file"], f);
    let error: Value = serde_json::from_str(&saved["error"]).unwrap();
    assert_eq!(error["code"], "conflict");
    assert_eq!(e.db.catalog_revision(f).unwrap(), 2);
    assert!(e.db.catalog_binding(f).unwrap().is_none());
}
