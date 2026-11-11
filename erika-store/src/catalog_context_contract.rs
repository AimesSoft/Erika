//! SQLite and browser snapshots expose the same immutable, credential-free view.
macro_rules! contract {
    ($name:ident, $db:ty) => { mod $name {
        use erika_catalog::*;
        use serde_json::json;
        type Db = $db;

        #[test]
        fn context_keeps_hierarchy_clocks_and_independent_danmaku_together() {
            let db = Db::in_memory();
            assert!(db.catalog_context(-1).is_err());
            let file = db.opened("https://private.invalid/video?token=secret", Some(&crate::Fingerprint { md5: "a".repeat(32), size: 100 })).unwrap();
            let empty = db.catalog_context(file).unwrap();
            assert!(empty.nodes.is_empty());
            assert_eq!(empty.revision, 0);
            let work = tmdb::parse_work(Kind::Tv, 71, &json!({"id":71,"name":"TV","seasons":[{"id":72,"season_number":0,"name":"特别篇"}]})).unwrap();
            let season = tmdb::parse_season(&work, 0, &json!({"id":72,"season_number":0,"name":"特别篇","episodes":[{"id":73,"season_number":0,"episode_number":1,"name":"SP"}]})).unwrap();
            let episode = season.episodes[0].clone();
            let binding = FileBinding { file, items: vec![BindingItem { media:episode.id.clone(), start_seconds:None, end_seconds:None }], decision:Decision::Manual, evidence:"test".into(), revision:0 };
            let danmaku = DanmakuBinding { file, reference:ProviderRef { provider:Provider::Dandanplay, scope:String::new(), kind:Kind::Episode, id:"901".into() }, media:Some(episode.id.clone()), manual:true, fingerprint_verified:false, shift_seconds:1., user_shift_seconds:0., revision:0 };
            let change = db.catalog_apply(&[work.node.clone(), season.node.clone(), episode.clone()], &[], &[binding], &[danmaku]).unwrap();
            let before = db.catalog_context(file).unwrap();
            assert_eq!(before.nodes.len(), 3);
            assert_eq!(before.revision, before.binding.as_ref().unwrap().revision);
            assert_eq!(before.danmaku_revision, before.danmaku.as_ref().unwrap().revision);
            assert_eq!(before.playing(10., 100.), Some(&episode.id));
            assert_eq!(before.nodes.iter().find(|n| n.kind==Kind::Season).unwrap().season_number, Some(0));
            let encoded = serde_json::to_string(&before).unwrap();
            assert!(!encoded.contains("secret"));
            assert!(!encoded.contains("private.invalid"));
            db.catalog_undo(&change.id).unwrap();
            let after = db.catalog_context(file).unwrap();
            assert_eq!(after.revision, 2);
            assert_eq!(after.danmaku_revision, 2);
            assert!(after.nodes.is_empty());
            assert!(after.binding.is_none());
            assert!(after.danmaku.is_none());
            assert_eq!(before.nodes.len(), 3, "previous snapshots are immutable");
        }
    }};
}
contract!(sqlite, crate::sqlite::Db);
contract!(browser, crate::session::Db);

#[test]
fn sqlite_snapshot_never_mixes_two_atomic_catalogue_commits() {
    use erika_catalog::*;
    use serde_json::json;
    use std::sync::{Arc, Barrier};
    let root = std::env::temp_dir().join(format!("erika-context-{}", MediaId::new().0));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("db.sqlite");
    let db = crate::Db::at(&path).unwrap();
    let file = db
        .opened(
            "https://example.invalid/video",
            Some(&crate::Fingerprint {
                md5: "b".repeat(32),
                size: 100,
            }),
        )
        .unwrap();
    let nodes: Vec<_> = [81, 82]
        .into_iter()
        .map(|id| {
            tmdb::parse_work(Kind::Movie, id, &json!({"id":id,"title":"Movie"}))
                .unwrap()
                .node
        })
        .collect();
    db.catalog_save_nodes(&nodes).unwrap();
    let ready = Arc::new(Barrier::new(2));
    let barrier = ready.clone();
    let writer = std::thread::spawn(move || {
        let db = crate::Db::at(&path).unwrap();
        barrier.wait();
        for revision in 0..200 {
            let node = &nodes[revision as usize % 2];
            let binding = FileBinding {
                file,
                items: vec![BindingItem {
                    media: node.id.clone(),
                    start_seconds: None,
                    end_seconds: None,
                }],
                decision: Decision::Manual,
                evidence: "test".into(),
                revision,
            };
            let danmaku = DanmakuBinding {
                file,
                reference: ProviderRef {
                    provider: Provider::Dandanplay,
                    scope: String::new(),
                    kind: Kind::Episode,
                    id: "901".into(),
                },
                media: Some(node.id.clone()),
                manual: true,
                fingerprint_verified: false,
                shift_seconds: 0.,
                user_shift_seconds: 0.,
                revision,
            };
            db.catalog_apply(&[], &[], &[binding], &[danmaku]).unwrap();
        }
    });
    ready.wait();
    for _ in 0..300 {
        let context = db.catalog_context(file).unwrap();
        assert_eq!(context.revision, context.danmaku_revision);
        if let Some(binding) = &context.binding {
            assert_eq!(binding.revision, context.revision);
            assert_eq!(
                Some(&binding.items[0].media),
                context.danmaku.as_ref().unwrap().media.as_ref()
            );
            assert_eq!(context.nodes[0].id, binding.items[0].media);
        }
    }
    writer.join().unwrap();
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}
