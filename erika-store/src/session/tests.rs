use super::*;
#[test]
fn engines_share_a_session_but_test_databases_are_isolated() {
    let a = Db::open();
    let b = Db::open();
    let key = format!("session-test-{}", erika_catalog::MediaId::new());
    a.set_setting(&key, Some("shared"));
    assert_eq!(b.setting(&key).as_deref(), Some("shared"));
    assert!(Db::in_memory().setting(&key).is_none());
    a.set_setting(&key, None);
}
#[test]
fn an_unhashed_browser_file_has_a_stable_session_identity() {
    let db = Db::in_memory();
    let a = db.opened("blob:example/video", None).unwrap();
    db.save_progress(a, 10., 100.);
    assert_eq!(db.opened("blob:example/video", None), Some(a));
    assert_eq!(db.progress(a), Some((10., 100.)));
    assert_ne!(db.opened("blob:example/another", None), Some(a));
}
#[test]
fn source_changes_and_provider_records_are_real_session_state() {
    let db = Db::in_memory();
    let source = db
        .add_source("remote", "Test", "https://example.invalid", "anime")
        .unwrap();
    let f = db
        .saw(
            source,
            "https://example.invalid/a.mkv",
            100,
            1,
            Some(&Fingerprint {
                md5: "a".repeat(32),
                size: 100,
            }),
        )
        .unwrap();
    let a = Anime {
        dandanplay_id: 1,
        title: "Provider title".into(),
        image: String::new(),
    };
    let e = Episode {
        dandanplay_id: 101,
        number: 1,
        title: "First".into(),
    };
    db.link_episode(f, &a, &e, true);
    assert!(db.catalog_shelf(None).unwrap().is_empty());
    assert!(!db.seen(source)[0].decided && !db.seen(source)[0].matched);
    assert_eq!(db.entries(Some(source))[0].matched, Some((a, e)));
    db.unlink(f);
    assert!(db.catalog_shelf(None).unwrap().is_empty());
    assert!(!db.seen(source)[0].decided);
    db.remove_source(source);
    assert!(db.seen(source).is_empty());
    assert!(db.sources().is_empty());
}
