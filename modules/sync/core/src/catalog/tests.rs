use super::*;
use crate::{
    local::{Device, MemoryVault, Scope},
    repository::{Base, Keys, Local, MemoryTransport, Outcome, Sync},
    value::Value,
};
use bern::storage::Store;
use erika_catalog::{
    BindingItem, DanmakuBinding, Decision, FileBinding, Kind, MediaId, Provider, ProviderRef,
    Watched, tmdb,
};
use erika_store::{Fingerprint, catalog::MediaMark};

struct TestDevice {
    db: Db,
    store: Store,
    dir: std::path::PathBuf,
    vault: MemoryVault,
}
impl TestDevice {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("erika-media-sync-{}", MediaId::new()));
        std::fs::create_dir_all(&dir).unwrap();
        Self {
            db: Db::in_memory(),
            store: Store::directory(&dir),
            dir,
            vault: MemoryVault::default(),
        }
    }
    fn device(&self) -> Device<'_> {
        Device::new(&self.db, &self.store, &self.vault, Scope::Repository)
    }
    fn sync(
        &self,
        server: &mut MemoryTransport,
        base: Option<Base>,
    ) -> Result<(Outcome, Base), SyncError> {
        Sync {
            transport: server,
            local: &mut self.device(),
            keys: &mut Keys::default(),
            root: "/test".into(),
            place: "test".into(),
            device: "test".into(),
            now: erika_store::now(),
            categories: crate::local::CATEGORIES
                .iter()
                .map(|c| c.to_string())
                .collect(),
        }
        .run(base)
    }
}
impl Drop for TestDevice {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn film(db: &Db, id: u64) -> erika_catalog::MediaNode {
    let n = tmdb::parse_work(
        Kind::Movie,
        id,
        &serde_json::json!({"id":id,"title":format!("电影{id}")}),
    )
    .unwrap()
    .node;
    db.catalog_save_nodes(&[n.clone()]).unwrap();
    n
}
fn file(db: &Db, n: u64) -> i64 {
    db.opened(
        &format!("file:///test/{n}.mkv"),
        Some(&Fingerprint {
            md5: format!("{n:032x}"),
            size: n,
        }),
    )
    .unwrap()
}
fn bind(db: &Db, f: i64, media: &MediaId, decision: Decision) {
    db.catalog_apply(
        &[],
        &[],
        &[FileBinding {
            file: f,
            items: if decision == Decision::Blocked {
                vec![]
            } else {
                vec![BindingItem {
                    media: media.clone(),
                    start_seconds: None,
                    end_seconds: None,
                }]
            },
            evidence: "test".into(),
            decision,
            revision: db.catalog_revision(f).unwrap(),
        }],
        &[],
    )
    .unwrap();
}
fn watched(db: &Db, media: &MediaId, value: bool, manual: bool, at: i64) {
    db.catalog_set_watched(&Watched {
        media: media.clone(),
        watched: value,
        manual,
        updated_at: at,
        revision: db.catalog_watched(media).unwrap().map_or(0, |w| w.revision),
    })
    .unwrap();
}

#[test]
fn current_backup_carries_media_without_aliases_or_device_row_ids() {
    let a = TestDevice::new();
    let b = TestDevice::new();
    let mut movie = film(&a.db, 7);
    let mut tv = tmdb::parse_work(
        Kind::Tv,
        7,
        &serde_json::json!({"id":7,"name":"剧集","seasons":[]}),
    )
    .unwrap()
    .node;
    tv.id = MediaId::new();
    tv.root = tv.id.clone();
    a.db.catalog_save_nodes(&[tv.clone()]).unwrap();
    // The receiver's UUID may differ, but the typed TMDB identity is the same.
    movie.id = MediaId::new();
    movie.root = movie.id.clone();
    b.db.catalog_save_nodes(&[movie.clone()]).unwrap();
    file(&b.db, 99);
    let f = file(&a.db, 1);
    let original = film(&a.db, 7);
    bind(&a.db, f, &original.id, Decision::Manual);
    a.db.catalog_set_danmaku(&DanmakuBinding {
        file: f,
        reference: ProviderRef {
            provider: Provider::Dandanplay,
            scope: String::new(),
            kind: Kind::Episode,
            id: "701".into(),
        },
        media: Some(original.id.clone()),
        manual: true,
        fingerprint_verified: false,
        shift_seconds: 4.0,
        user_shift_seconds: -2.0,
        revision: 0,
    })
    .unwrap();
    watched(&a.db, &original.id, false, true, 17);
    a.db.catalog_set_mark(&MediaMark {
        media: original.id.clone(),
        reference: None,
        status: Some("watching".into()),
        rating: Some(8),
        review: Some("本地短评".into()),
        revision: 0,
        updated_at: 18,
    })
    .unwrap();
    a.db.set_setting("anime.tmdb.sources.v1", Some("cache"));
    let bytes = crate::export(&a.db, &a.store, &a.vault, &mut Keys::default(), "test").unwrap();
    let doc = crate::format::read(&bytes).unwrap();
    let state = doc.state.unwrap();
    assert_eq!(state[NODES].len(), 2);
    assert!(
        !state
            .get(crate::local::SETTINGS)
            .is_some_and(|s| s.contains_key("anime.tmdb.sources.v1"))
    );
    assert!(
        state
            .keys()
            .all(|k| !k.contains("outbox") && !k.contains("lease"))
    );
    crate::import(&bytes, &b.db, &b.store, &b.vault, &mut Keys::default()).unwrap();
    let f2 = file(&b.db, 1);
    assert_ne!(f, f2);
    assert_eq!(
        b.db.catalog_binding(f2).unwrap().unwrap().items[0].media,
        movie.id
    );
    assert!(!b.db.catalog_watched(&movie.id).unwrap().unwrap().watched);
    let mark = b.db.catalog_marks(&movie.id).unwrap().remove(0);
    assert_eq!(mark.rating, Some(8));
    assert_eq!(mark.review.as_deref(), Some("本地短评"));
    assert_eq!(mark.revision, 1);
    assert_eq!(
        b.db.catalog_danmaku(f2).unwrap().unwrap().shift_seconds,
        4.0
    );
    assert_eq!(b.db.catalog_export().unwrap().nodes.len(), 2);
    let again = crate::import(&bytes, &b.db, &b.store, &b.vault, &mut Keys::default()).unwrap();
    assert_eq!(again.applied.changes, 0);
}

#[test]
fn devices_merge_different_records_and_keep_explicit_unwatched() {
    let a = TestDevice::new();
    let b = TestDevice::new();
    let one = film(&a.db, 1);
    let two = film(&a.db, 2);
    let f = file(&a.db, 1);
    bind(&a.db, f, &one.id, Decision::Auto);
    let mut server = MemoryTransport::default();
    let (_, abase) = a.sync(&mut server, None).unwrap();
    let (_, bbase) = b.sync(&mut server, None).unwrap();
    watched(&a.db, &one.id, false, true, 100);
    watched(&b.db, &one.id, true, false, 200);
    watched(&b.db, &two.id, true, true, 200);
    let (_, abase) = a.sync(&mut server, Some(abase)).unwrap();
    let (_, bbase) = b.sync(&mut server, Some(bbase)).unwrap();
    assert!(!b.db.catalog_watched(&one.id).unwrap().unwrap().watched);
    let (_, abase) = a.sync(&mut server, Some(abase)).unwrap();
    assert!(a.db.catalog_watched(&two.id).unwrap().unwrap().watched);
    let (outcome, _) = a.sync(&mut server, Some(abase)).unwrap();
    assert_eq!((outcome.uploaded, outcome.restored), (0, 0));
    let (outcome, _) = b.sync(&mut server, Some(bbase)).unwrap();
    assert_eq!((outcome.uploaded, outcome.restored), (0, 0));
}

#[test]
fn manual_conflicts_fail_before_settings_or_remote_files_change() {
    let a = TestDevice::new();
    let b = TestDevice::new();
    let one = film(&a.db, 1);
    let two = film(&a.db, 2);
    let f = file(&a.db, 1);
    bind(&a.db, f, &one.id, Decision::Auto);
    let mut server = MemoryTransport::default();
    let (_, abase) = a.sync(&mut server, None).unwrap();
    let (_, bbase) = b.sync(&mut server, None).unwrap();
    bind(&a.db, f, &two.id, Decision::Manual);
    bind(&b.db, file(&b.db, 1), &one.id, Decision::Blocked);
    a.db.set_setting("conflict-test", Some("remote"));
    a.sync(&mut server, Some(abase)).unwrap();
    let before = server.files.clone();
    let snapshot = b.db.catalog_export().unwrap();
    assert!(b.sync(&mut server, Some(bbase)).is_err());
    assert_eq!(server.files, before);
    assert_eq!(b.db.catalog_export().unwrap(), snapshot);
    assert_eq!(b.db.setting("conflict-test"), None);
}

#[test]
fn stale_collection_and_invalid_batch_cannot_partly_restore() {
    let a = TestDevice::new();
    let b = TestDevice::new();
    let movie = film(&a.db, 1);
    let f = file(&a.db, 1);
    bind(&a.db, f, &movie.id, Decision::Manual);
    a.db.set_setting("restore-test", Some("value"));
    let incoming = a.device().collect(None).unwrap();
    let mut device = b.device();
    let mine = device.collect(None).unwrap();
    let ops = crate::merge::diff(&mine, &incoming);
    film(&b.db, 2);
    let before = b.db.catalog_export().unwrap();
    assert!(device.apply(&ops, None).is_err());
    assert_eq!(b.db.catalog_export().unwrap(), before);
    assert_eq!(b.db.setting("restore-test"), None);
    let mut corrupt = incoming.clone();
    let Value::Map(record) = corrupt
        .get_mut(BINDINGS)
        .unwrap()
        .values_mut()
        .next()
        .unwrap()
    else {
        panic!()
    };
    record.insert("manual".into(), Value::Bool(false));
    assert!(validate(&corrupt).is_err());
    let mut invalid = incoming;
    let Value::Map(record) = invalid
        .get_mut(BINDINGS)
        .unwrap()
        .values_mut()
        .next()
        .unwrap()
    else {
        panic!()
    };
    let mut data = record["data"].to_json();
    data["binding"]["items"][0]["media"] = serde_json::json!(MediaId::new());
    record.insert("data".into(), Value::from_json(&data));
    let mine = device.collect(None).unwrap();
    let ops = crate::merge::diff(&mine, &crate::merge::merge_first(&invalid, &mine));
    assert!(device.apply(&ops, None).is_err());
    assert_eq!(b.db.catalog_export().unwrap(), before);
    assert_eq!(b.db.setting("restore-test"), None);
}

#[test]
fn undo_removes_bindings_without_reusing_their_revision() {
    let a = TestDevice::new();
    let b = TestDevice::new();
    let movie = film(&a.db, 1);
    let f = file(&a.db, 1);
    let change =
        a.db.catalog_apply(
            &[],
            &[],
            &[FileBinding {
                file: f,
                decision: Decision::Manual,
                items: vec![BindingItem {
                    media: movie.id.clone(),
                    start_seconds: None,
                    end_seconds: None,
                }],
                evidence: String::new(),
                revision: 0,
            }],
            &[],
        )
        .unwrap();
    let mut server = MemoryTransport::default();
    let (_, abase) = a.sync(&mut server, None).unwrap();
    let (_, bbase) = b.sync(&mut server, None).unwrap();
    let bf = file(&b.db, 1);
    let revision = b.db.catalog_revision(bf).unwrap();
    a.db.catalog_undo(&change.id).unwrap();
    let (_, mut abase) = a.sync(&mut server, Some(abase)).unwrap();
    for n in 0..65 {
        a.db.set_setting("compaction-test", Some(&n.to_string()));
        abase = a.sync(&mut server, Some(abase)).unwrap().1;
    }
    assert!(abase.snapshot > bbase.snapshot);
    b.sync(&mut server, Some(bbase)).unwrap();
    assert!(b.db.catalog_binding(bf).unwrap().is_none());
    assert!(b.db.catalog_revision(bf).unwrap() > revision);
}

#[test]
fn a_causal_undo_restores_auto_without_a_permanent_manual_guard() {
    let a = TestDevice::new();
    let b = TestDevice::new();
    let movie = film(&a.db, 1);
    let f = file(&a.db, 1);
    bind(&a.db, f, &movie.id, Decision::Auto);
    let change =
        a.db.catalog_apply(
            &[],
            &[],
            &[FileBinding {
                file: f,
                decision: Decision::Blocked,
                items: vec![],
                evidence: "blocked".into(),
                revision: a.db.catalog_revision(f).unwrap(),
            }],
            &[],
        )
        .unwrap();
    let mut server = MemoryTransport::default();
    let (_, abase) = a.sync(&mut server, None).unwrap();
    let (_, bbase) = b.sync(&mut server, None).unwrap();
    a.db.catalog_undo(&change.id).unwrap();
    a.sync(&mut server, Some(abase)).unwrap();
    let (_, bbase) = b.sync(&mut server, Some(bbase)).unwrap();
    assert_eq!(
        b.db.catalog_binding(file(&b.db, 1))
            .unwrap()
            .unwrap()
            .decision,
        Decision::Auto
    );
    let (outcome, _) = b.sync(&mut server, Some(bbase)).unwrap();
    assert_eq!((outcome.restored, outcome.uploaded), (0, 0));
}

#[test]
fn partial_metadata_converges_to_the_data_actually_stored() {
    let a = TestDevice::new();
    let b = TestDevice::new();
    let mut movie = film(&b.db, 71);
    movie.overview = "完整简介".into();
    movie.artwork.poster = erika_catalog::artwork::path_asset(
        movie.reference.as_ref().unwrap(),
        erika_catalog::artwork::ArtworkKind::Poster,
        "/poster.jpg",
    );
    b.db.catalog_save_nodes(&[movie.clone()]).unwrap();
    let mut partial = film(&a.db, 71);
    partial.title = "新标题".into();
    a.db.catalog_save_nodes(&[partial]).unwrap();
    let mut server = MemoryTransport::default();
    let (_, abase) = a.sync(&mut server, None).unwrap();
    let (_, bbase) = b.sync(&mut server, None).unwrap();
    let (_, abase) = a.sync(&mut server, Some(abase)).unwrap();
    for (device, base) in [(&a, abase), (&b, bbase)] {
        let state = device.device().collect(None).unwrap();
        assert_eq!(state.get(NODES), base.state.get(NODES));
        let (outcome, _) = device.sync(&mut server, Some(base)).unwrap();
        assert_eq!((outcome.restored, outcome.uploaded), (0, 0));
        let node = device.db.catalog_export().unwrap().nodes.remove(0);
        assert_eq!(node.title, "新标题");
        assert_eq!(node.overview, "完整简介");
        assert!(node.artwork.poster.is_some());
    }
    let bytes = crate::export(&b.db, &b.store, &b.vault, &mut Keys::default(), "test").unwrap();
    let imported = crate::import(&bytes, &a.db, &a.store, &a.vault, &mut Keys::default()).unwrap();
    assert_eq!(imported.applied.changes, 0);
}

#[test]
fn repository_does_not_publish_after_local_apply_failure() {
    struct Fails;
    impl Local for Fails {
        fn collect(&mut self, _: Option<&crate::seal::SealKey>) -> Result<State, SyncError> {
            Ok(State::from([(
                "settings".into(),
                std::collections::BTreeMap::from([("local".into(), Value::text("mine"))]),
            )]))
        }
        fn apply(&mut self, _: &[Op], _: Option<&crate::seal::SealKey>) -> Result<(), SyncError> {
            Err(SyncError::new("test failure"))
        }
    }
    let a = TestDevice::new();
    a.db.set_setting("remote", Some("theirs"));
    let mut server = MemoryTransport::default();
    a.sync(&mut server, None).unwrap();
    let before = server.files.clone();
    let result = Sync {
        transport: &mut server,
        local: &mut Fails,
        keys: &mut Keys::default(),
        root: "/test".into(),
        place: "test".into(),
        device: "test".into(),
        now: 0,
        categories: crate::local::CATEGORIES
            .iter()
            .map(|c| c.to_string())
            .collect(),
    }
    .run(None);
    assert!(result.is_err());
    assert_eq!(server.files, before);
}
