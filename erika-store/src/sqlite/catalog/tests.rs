use super::*;
use crate::Fingerprint;
use serde_json::json;

fn movie(id: u64) -> MediaNode {
    erika_catalog::tmdb::parse_work(Kind::Movie,id,&json!({"id":id,"title":format!("Movie {id}"),"overview":"summary","release_date":"2026-01-01"})).unwrap().node
}
fn tv() -> Vec<MediaNode> {
    let work = erika_catalog::tmdb::parse_work(Kind::Tv,42,&json!({"id":42,"name":"Series","seasons":[{"id":100,"season_number":0,"name":"Specials"},{"id":101,"season_number":1,"name":"Season 1"}]})).unwrap();
    let special = erika_catalog::tmdb::parse_season(&work,0,&json!({"id":100,"season_number":0,"name":"Specials","episodes":[{"id":1000,"season_number":0,"episode_number":1,"name":"OVA"}]})).unwrap();
    let regular = erika_catalog::tmdb::parse_season(&work,1,&json!({"id":101,"season_number":1,"name":"Season 1","episodes":[{"id":1001,"season_number":1,"episode_number":1,"name":"First"},{"id":1002,"season_number":1,"episode_number":2,"name":"Second"}]})).unwrap();
    vec![
        work.node,
        special.node,
        regular.node,
        special.episodes[0].clone(),
        regular.episodes[0].clone(),
        regular.episodes[1].clone(),
    ]
}
fn file(db: &Db, name: &str) -> i64 {
    db.opened(
        &format!("file:///{name}.mkv"),
        Some(&Fingerprint {
            md5: name.into(),
            size: 100,
        }),
    )
    .unwrap()
}
fn bind(file: i64, node: &MediaNode, revision: u64, decision: Decision) -> FileBinding {
    FileBinding {
        file,
        decision,
        items: vec![BindingItem {
            media: node.id.clone(),
            start_seconds: None,
            end_seconds: None,
        }],
        evidence: "test".into(),
        revision,
    }
}

#[test]
fn canonical_identity_has_real_seasons_movies_and_multiple_versions() {
    let db = Db::in_memory();
    let nodes = tv();
    let film = movie(42);
    db.catalog_save_nodes(&nodes).unwrap();
    db.catalog_save_nodes(&[film.clone()]).unwrap();
    assert_ne!(film.id, nodes[0].id);
    assert_eq!(db.catalog_children(&nodes[0].id).unwrap().len(), 2);
    assert_eq!(
        db.catalog_children(&nodes[1].id).unwrap()[0].season_number,
        Some(0)
    );
    let a = file(&db, "a");
    let b = file(&db, "b");
    let c = file(&db, "c");
    db.catalog_apply(
        &[],
        &[],
        &[
            bind(a, &nodes[3], 0, Decision::Auto),
            bind(b, &nodes[3], 0, Decision::Auto),
            bind(c, &film, 0, Decision::Auto),
        ],
        &[],
    )
    .unwrap();
    assert_eq!(db.catalog_files(&nodes[3].id).unwrap().len(), 2);
    let shelf = db.catalog_shelf(None).unwrap();
    assert_eq!(shelf.len(), 2);
    assert_eq!(
        shelf
            .iter()
            .find(|s| s.node.id == nodes[0].id)
            .unwrap()
            .episodes,
        1
    );
    assert_eq!(
        db.catalog_by_ref(film.reference.as_ref().unwrap()).unwrap()[0].id,
        film.id
    );
}

#[test]
fn apply_is_atomic_and_rejects_stale_jobs_and_manual_overrides() {
    let db = Db::in_memory();
    let a = movie(1);
    let b = movie(2);
    let f = file(&db, "a");
    db.catalog_apply(&[a.clone()], &[], &[bind(f, &a, 0, Decision::Manual)], &[])
        .unwrap();
    let error = db
        .catalog_apply(&[b.clone()], &[], &[bind(f, &b, 0, Decision::Auto)], &[])
        .unwrap_err();
    assert_eq!(error.code, "conflict");
    assert!(db.catalog_node(&b.id).unwrap().is_none());
    assert!(
        db.catalog_apply(&[b.clone()], &[], &[bind(f, &b, 1, Decision::Auto)], &[])
            .is_err()
    );
    assert!(db.catalog_node(&b.id).unwrap().is_none());
    let blocked = FileBinding {
        file: f,
        decision: Decision::Blocked,
        items: vec![],
        evidence: "user".into(),
        revision: 1,
    };
    db.catalog_apply(&[], &[], &[blocked], &[]).unwrap();
    assert!(db.catalog_files(&a.id).unwrap().is_empty());
    assert!(
        db.catalog_apply(&[], &[], &[bind(f, &a, 2, Decision::Auto)], &[])
            .is_err()
    );
}

#[test]
fn undo_keeps_a_monotonic_revision_even_when_initially_unbound() {
    let db = Db::in_memory();
    let film = movie(1);
    let f = file(&db, "a");
    let change = db
        .catalog_apply(
            &[film.clone()],
            &[],
            &[bind(f, &film, 0, Decision::Manual)],
            &[],
        )
        .unwrap();
    db.catalog_undo(&change.id).unwrap();
    assert!(db.catalog_binding(f).unwrap().is_none());
    assert_eq!(db.catalog_revision(f).unwrap(), 2);
    assert!(
        db.catalog_apply(&[], &[], &[bind(f, &film, 0, Decision::Auto)], &[])
            .is_err()
    );
    assert!(db.catalog_undo(&change.id).is_err());
    let change = db
        .catalog_apply(&[], &[], &[bind(f, &film, 2, Decision::Auto)], &[])
        .unwrap();
    db.catalog_apply(&[], &[], &[bind(f, &film, 3, Decision::Manual)], &[])
        .unwrap();
    assert!(db.catalog_undo(&change.id).is_err());
}

#[test]
fn invalid_batch_and_cross_show_hierarchy_are_rolled_back() {
    let db = Db::in_memory();
    let nodes = tv();
    let film = movie(1);
    let f = file(&db, "a");
    let mut invalid = nodes[3].clone();
    invalid.parent = Some(film.id.clone());
    invalid.root = film.id.clone();
    assert!(db.catalog_save_nodes(&[film.clone(), invalid]).is_err());
    assert!(db.catalog_node(&film.id).unwrap().is_none());
    db.catalog_save_nodes(&nodes).unwrap();
    assert!(
        db.catalog_apply(
            &[],
            &[],
            &[
                bind(f, &nodes[3], 0, Decision::Manual),
                bind(i64::MAX, &nodes[4], 0, Decision::Manual)
            ],
            &[]
        )
        .is_err()
    );
    assert!(db.catalog_binding(f).unwrap().is_none());
}

#[test]
fn explicit_unwatched_survives_progress_and_auto_completion() {
    let db = Db::in_memory();
    let film = movie(1);
    let f = file(&db, "a");
    db.catalog_apply(
        &[film.clone()],
        &[],
        &[bind(f, &film, 0, Decision::Auto)],
        &[],
    )
    .unwrap();
    db.save_progress(f, 95., 100.);
    assert_eq!(db.catalog_shelf(None).unwrap()[0].watched, 0);
    let mut played = erika_catalog::playback::Intervals::default();
    played.add(0., 95.).unwrap();
    assert_eq!(
        db.catalog_record_playback(f, 1, &played, 100., false)
            .unwrap(),
        vec![film.id.clone()]
    );
    assert_eq!(db.catalog_shelf(None).unwrap()[0].watched, 1);
    let mut value = Watched {
        media: film.id.clone(),
        watched: false,
        manual: true,
        updated_at: 123,
        revision: db.catalog_watched(&film.id).unwrap().unwrap().revision,
    };
    value = db.catalog_set_watched(&value).unwrap();
    assert!(
        db.catalog_record_playback(f, 1, &played, 100., true)
            .unwrap()
            .is_empty()
    );
    assert_eq!(db.catalog_watched(&film.id).unwrap(), Some(value.clone()));
    assert_eq!(db.catalog_shelf(None).unwrap()[0].watched, 0);
    value.manual = false;
    value.watched = true;
    assert!(db.catalog_set_watched(&value).is_err());
    assert_eq!(db.progress(f), Some((95., 100.)));
}

#[test]
fn multi_episode_file_does_not_guess_per_episode_completion() {
    let db = Db::in_memory();
    let nodes = tv();
    let f = file(&db, "a");
    let mut b = bind(f, &nodes[4], 0, Decision::Auto);
    b.items.push(BindingItem {
        media: nodes[5].id.clone(),
        start_seconds: None,
        end_seconds: None,
    });
    db.catalog_apply(&nodes, &[], &[b], &[]).unwrap();
    db.save_progress(f, 99., 100.);
    let shelf = db.catalog_shelf(None).unwrap();
    assert_eq!(shelf[0].episodes, 2);
    assert_eq!(shelf[0].watched, 0);
}

#[test]
fn provider_coverage_and_danmaku_are_independent_of_media_rematching() {
    let db = Db::in_memory();
    let nodes = tv();
    let f = file(&db, "a");
    db.catalog_save_nodes(&nodes).unwrap();
    let mut link = ProviderLink {
        media: nodes[1].id.clone(),
        reference: ProviderRef {
            provider: Provider::Bangumi,
            scope: String::new(),
            kind: Kind::Subject,
            id: "123".into(),
        },
        relation: Relation::Exact,
        coverage: vec![nodes[4].id.clone()],
        evidence: "source".into(),
        confirmed: true,
        manual: true,
        revision: 0,
    };
    assert!(db.catalog_link(&link).is_err());
    link.coverage = vec![nodes[3].id.clone()];
    db.catalog_link(&link).unwrap();
    let danmaku = DanmakuBinding {
        file: f,
        reference: ProviderRef {
            provider: Provider::Dandanplay,
            scope: String::new(),
            kind: Kind::Episode,
            id: "episode-x".into(),
        },
        media: None,
        manual: true,
        fingerprint_verified: false,
        shift_seconds: 5.,
        user_shift_seconds: -1.,
        revision: 0,
    };
    db.catalog_set_danmaku(&danmaku).unwrap();
    db.catalog_apply(&[], &[], &[bind(f, &nodes[4], 0, Decision::Manual)], &[])
        .unwrap();
    assert_eq!(
        db.catalog_danmaku(f).unwrap().unwrap().reference,
        danmaku.reference
    );
    assert_eq!(
        db.catalog_binding(f).unwrap().unwrap().items[0].media,
        nodes[4].id
    );
}

#[test]
fn portable_snapshot_remaps_file_rows_and_never_copies_account_jobs() {
    let db = Db::in_memory();
    let nodes = tv();
    let fingerprint = Fingerprint {
        md5: "a".repeat(32),
        size: 123,
    };
    let f = db.opened("file:///a.mkv", Some(&fingerprint)).unwrap();
    db.catalog_apply(&nodes, &[], &[bind(f, &nodes[4], 0, Decision::Manual)], &[])
        .unwrap();
    db.catalog_set_watched(&Watched {
        media: nodes[4].id.clone(),
        watched: false,
        manual: true,
        updated_at: 123,
        revision: 0,
    })
    .unwrap();
    let snapshot = db.catalog_export().unwrap();
    assert_eq!(snapshot.bindings[0].binding.file, 0);
    let other = Db::in_memory();
    file(&other, "unrelated");
    other.catalog_import(&snapshot, false).unwrap();
    let imported = other
        .opened("file:///other.mkv", Some(&fingerprint))
        .unwrap();
    assert_ne!(f, imported);
    assert_eq!(
        other.catalog_binding(imported).unwrap().unwrap().items[0].media,
        nodes[4].id
    );
    assert!(
        !other
            .catalog_watched(&nodes[4].id)
            .unwrap()
            .unwrap()
            .watched
    );
    assert!(other.catalog_watch_queue("account").unwrap().is_empty());
    other.catalog_import(&snapshot, false).unwrap();
    let mut conflicting = snapshot.clone();
    conflicting.bindings[0].binding.items[0].media = nodes[5].id.clone();
    assert!(other.catalog_import(&conflicting, false).is_err());
    assert_eq!(
        other.catalog_binding(imported).unwrap().unwrap().items[0].media,
        nodes[4].id
    );
    other.catalog_import(&conflicting, true).unwrap();
    assert_eq!(
        other.catalog_binding(imported).unwrap().unwrap().items[0].media,
        nodes[5].id
    );
}

#[test]
fn job_claim_and_stale_result_are_compare_and_swap() {
    use crate::catalog::CatalogJob;
    let db = Db::in_memory();
    let f = file(&db, "a");
    let film = movie(1);
    let job = CatalogJob {
        id: MediaId::new().0,
        file: Some(f),
        input_revision: 0,
        phase: "dandanplay".into(),
        state: "queued".into(),
        payload: "{}".into(),
        error: String::new(),
        next_attempt: 0,
    };
    db.catalog_job_enqueue(&job).unwrap();
    assert_eq!(
        db.catalog_job_transition(&job.id, "queued", "running", "dandanplay", "", 0)
            .unwrap()
            .state,
        "running"
    );
    assert!(
        db.catalog_job_transition(&job.id, "queued", "running", "dandanplay", "", 0)
            .is_err()
    );
    db.catalog_apply(
        &[film.clone()],
        &[],
        &[bind(f, &film, 0, Decision::Manual)],
        &[],
    )
    .unwrap();
    assert_eq!(
        db.catalog_job_transition(&job.id, "running", "pending", "tmdb", "", 0)
            .unwrap()
            .state,
        "stale"
    );
}

#[test]
fn two_account_outboxes_are_independent_and_timeouts_require_a_readback() {
    use crate::catalog::WatchSync;
    let db = Db::in_memory();
    let nodes = tv();
    db.catalog_save_nodes(&nodes).unwrap();
    let watched = db
        .catalog_set_watched(&Watched {
            media: nodes[4].id.clone(),
            watched: true,
            manual: true,
            updated_at: 1,
            revision: 0,
        })
        .unwrap();
    let reference = ProviderRef {
        provider: Provider::Bangumi,
        scope: String::new(),
        kind: Kind::Episode,
        id: "123".into(),
    };
    let link = db
        .catalog_link(&ProviderLink {
            media: nodes[4].id.clone(),
            reference: reference.clone(),
            relation: Relation::Exact,
            coverage: vec![],
            evidence: "verified".into(),
            confirmed: true,
            manual: true,
            revision: 0,
        })
        .unwrap();
    let task = WatchSync {
        operation: String::new(),
        media: nodes[4].id.clone(),
        account: "bangumi:1".into(),
        target: reference,
        watched: true,
        local_revision: watched.revision,
        mapping_revision: link.revision,
        state: "pending".into(),
        error: String::new(),
        remote_baseline: None,
        revision: 0,
        lease_until: 0,
    };
    let task = db.catalog_watch_enqueue(&task).unwrap();
    assert_eq!(
        db.catalog_watch_enqueue(&task).unwrap().operation,
        task.operation
    );
    let mut another = task.clone();
    another.account = "bangumi:2".into();
    assert_ne!(
        db.catalog_watch_enqueue(&another).unwrap().operation,
        task.operation
    );
    let sending = db
        .catalog_watch_transition(&task, "sending", "", None)
        .unwrap();
    let unknown = db
        .catalog_watch_transition(&sending, "unknown", "timeout", None)
        .unwrap();
    assert!(
        db.catalog_watch_transition(&unknown, "pending", "", None)
            .is_err()
    );
    assert!(
        db.catalog_watch_transition(&unknown, "pending", "", Some(true))
            .is_err()
    );
    db.catalog_watch_transition(&unknown, "sent", "", Some(true))
        .unwrap();
    assert_eq!(
        db.catalog_watch_queue("bangumi:2").unwrap()[0].state,
        "pending"
    );
}

#[test]
fn failed_or_stale_scans_never_prune_the_source() {
    let db = Db::in_memory();
    let source = db
        .add_source("folder", "test", "file:///a", "anime")
        .unwrap();
    db.saw(source, "file:///a/one.mkv", 10, 1, None);
    db.saw(source, "file:///a/two.mkv", 10, 1, None);
    let a = MediaId::new().0;
    let b = MediaId::new().0;
    let first = db.catalog_scan_begin(source, &a, 100).unwrap();
    assert!(db.catalog_scan_begin(source, &b, 101).is_err());
    db.catalog_scan_saw(source, &a, first, &["file:///a/one.mkv".into()], 102)
        .unwrap();
    assert_eq!(
        db.catalog_scan_finish(source, &a, first, false, 103)
            .unwrap(),
        0
    );
    assert_eq!(db.seen(source).len(), 2);
    let second = db.catalog_scan_begin(source, &a, 104).unwrap();
    let third = db.catalog_scan_begin(source, &b, 300).unwrap();
    assert!(third > second);
    assert!(
        db.catalog_scan_finish(source, &a, second, true, 301)
            .is_err()
    );
    db.catalog_scan_saw(source, &b, third, &["file:///a/two.mkv".into()], 301)
        .unwrap();
    assert_eq!(
        db.catalog_scan_finish(source, &b, third, true, 302)
            .unwrap(),
        1
    );
    assert_eq!(db.seen(source)[0].url, "file:///a/two.mkv");
}

fn recognition_job(db: &Db, file: i64) -> crate::catalog::CatalogJob {
    let job = crate::catalog::CatalogJob {
        id: MediaId::new().0,
        file: Some(file),
        input_revision: db.catalog_revision(file).unwrap(),
        phase: "recognition".into(),
        state: "queued".into(),
        payload: "{}".into(),
        error: String::new(),
        next_attempt: 0,
    };
    db.catalog_job_enqueue(&job).unwrap();
    db.catalog_job_transition(&job.id, "queued", "running", "recognition", "", 0)
        .unwrap()
}
fn recognition() -> erika_catalog::resolve::Recognition {
    erika_catalog::resolve::Recognition {
        episode: Some(ProviderRef {
            provider: Provider::Dandanplay,
            scope: String::new(),
            kind: Kind::Episode,
            id: "123".into(),
        }),
        shift_seconds: 2.5,
        ..Default::default()
    }
}
fn bound_report(film: &MediaNode) -> erika_catalog::resolve::FileResolution {
    use erika_catalog::resolve::{FileResolution, Resolution, Resolved};
    FileResolution {
        recognition: Some(recognition()),
        recognition_error: None,
        resolution: Ok(Resolution::Bound {
            result: Resolved {
                work: erika_catalog::tmdb::Work {
                    node: film.clone(),
                    seasons: vec![],
                },
                season: None,
                items: vec![BindingItem {
                    media: film.id.clone(),
                    start_seconds: None,
                    end_seconds: None,
                }],
                recognition: Some(recognition()),
                evidence: "confirmed TMDB work".into(),
            },
        }),
    }
}

#[test]
fn resolution_commits_job_and_binding_atomically_and_preserves_manual_danmaku() {
    let db = Db::in_memory();
    let f = file(&db, "a");
    let film = movie(1);
    let job = recognition_job(&db, f);
    let mut manual = DanmakuBinding {
        file: f,
        reference: recognition().episode.unwrap(),
        media: None,
        manual: true,
        fingerprint_verified: false,
        shift_seconds: 0.,
        user_shift_seconds: -9.,
        revision: 0,
    };
    manual.reference.id = "456".into();
    db.catalog_set_danmaku(&manual).unwrap();
    let completed = db
        .catalog_finish_resolution(&job.id, 0, &bound_report(&film))
        .unwrap();
    assert_eq!(completed.state, "bound");
    assert_eq!(
        db.catalog_binding(f).unwrap().unwrap().items[0].media,
        film.id
    );
    assert_eq!(db.catalog_danmaku(f).unwrap().unwrap().reference.id, "456");
    assert_eq!(
        db.catalog_danmaku(f).unwrap().unwrap().user_shift_seconds,
        -9.
    );
    assert!(
        db.catalog_finish_resolution(&job.id, 0, &bound_report(&film))
            .is_err()
    );
    assert!(db.catalog_watch_queue("account").unwrap().is_empty());
}

#[test]
fn metadata_failure_keeps_danmaku_evidence_and_retry_state_without_inventing_media() {
    let db = Db::in_memory();
    let f = file(&db, "a");
    let job = recognition_job(&db, f);
    let report = erika_catalog::resolve::FileResolution {
        recognition: Some(recognition()),
        recognition_error: None,
        resolution: Err(erika_catalog::Error::unavailable()),
    };
    let result = db.catalog_finish_resolution(&job.id, 0, &report).unwrap();
    assert_eq!(result.state, "retry");
    assert!(result.next_attempt > crate::now());
    assert!(result.payload.contains("123"));
    assert!(db.catalog_binding(f).unwrap().is_none());
    let danmaku = db.catalog_danmaku(f).unwrap().unwrap();
    assert_eq!(danmaku.reference.id, "123");
    assert_eq!(danmaku.shift_seconds, 2.5);
    assert!(!danmaku.fingerprint_verified);
}

#[test]
fn late_or_abandoned_recognition_cannot_override_a_new_binding() {
    let db = Db::in_memory();
    let f = file(&db, "a");
    let job = recognition_job(&db, f);
    let film = movie(1);
    db.catalog_apply(
        &[film.clone()],
        &[],
        &[bind(f, &film, 0, Decision::Manual)],
        &[],
    )
    .unwrap();
    let other = movie(2);
    assert_eq!(
        db.catalog_finish_resolution(&job.id, 0, &bound_report(&other))
            .unwrap()
            .state,
        "stale"
    );
    assert!(db.catalog_node(&other.id).unwrap().is_none());
    assert!(db.catalog_danmaku(f).unwrap().is_none());
    let g = file(&db, "b");
    let expired = recognition_job(&db, g);
    assert_eq!(
        db.catalog_file_job(g, crate::now() + 301)
            .unwrap()
            .unwrap()
            .state,
        "stale"
    );
    assert!(
        db.catalog_finish_resolution(&expired.id, 0, &bound_report(&other))
            .is_err()
    );
}

#[test]
fn danmaku_only_undo_has_its_own_monotonic_revision() {
    let db = Db::in_memory();
    let f = file(&db, "a");
    let value = DanmakuBinding {
        file: f,
        reference: recognition().episode.unwrap(),
        media: None,
        manual: true,
        fingerprint_verified: false,
        shift_seconds: 0.,
        user_shift_seconds: 0.,
        revision: 0,
    };
    let change = db.catalog_apply(&[], &[], &[], &[value.clone()]).unwrap();
    db.catalog_undo(&change.id).unwrap();
    assert!(db.catalog_danmaku(f).unwrap().is_none());
    assert_eq!(db.catalog_danmaku_revision(f).unwrap(), 2);
    assert_eq!(db.catalog_revision(f).unwrap(), 0);
    assert!(db.catalog_set_danmaku(&value).is_err());
}

#[test]
fn a_stale_scan_cannot_recreate_addresses_in_a_replaced_source() {
    let db = Db::in_memory();
    let source = db
        .add_source("folder", "old", "file:///old", "anime")
        .unwrap();
    let owner = MediaId::new().0;
    let generation = db.catalog_scan_begin(source, &owner, 100).unwrap();
    db.remove_source(source);
    db.conn.execute("INSERT INTO source(id,kind,name,location,content,added_at) VALUES(?1,'folder','new','file:///new','anime',0)", [source]).unwrap();
    assert!(
        db.catalog_scan_read(
            source,
            &owner,
            generation,
            "file:///old/video.mkv",
            1,
            0,
            None,
            101
        )
        .is_err()
    );
    assert!(db.seen(source).is_empty());
}

#[test]
fn an_empty_database_gets_the_complete_schema_once() {
    let db=Db::ready(Connection::open_in_memory().unwrap()).unwrap();
    assert_eq!(db.conn.query_row("PRAGMA application_id",[],|r|r.get::<_,i64>(0)).unwrap(),super::super::APPLICATION_ID);
    assert_eq!(db.conn.query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0)).unwrap(),super::super::SCHEMA_VERSION);
    db.set_setting("kept",Some("value"));
    db.catalog_save_nodes(&tv()).unwrap();
    let reopened=Db::ready(db.conn).unwrap();
    assert_eq!(reopened.setting("kept").as_deref(),Some("value"));
    assert_eq!(reopened.catalog_export().unwrap().nodes.len(),6);
    let tables:i64=reopened.conn.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('media_alias','legacy_identity')",[],|r|r.get(0)).unwrap();
    assert_eq!(tables,0);
}

#[test]
fn measured_playback_survives_reopen_and_serializes_concurrent_players() {
    use erika_catalog::playback::Intervals;
    use std::sync::{Arc, Barrier};
    let dir = std::env::temp_dir().join(format!("erika-playback-test-{}", MediaId::new().0));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("erika.db");
    let film = movie(52);
    let f;
    {
        let db = Db::at(&path).unwrap();
        f = file(&db, "playback");
        db.catalog_apply(
            &[film.clone()],
            &[],
            &[bind(f, &film, 0, Decision::Auto)],
            &[],
        )
        .unwrap();
        let mut first = Intervals::default();
        first.add(0., 30.).unwrap();
        assert!(
            db.catalog_record_playback(f, 1, &first, 100., false)
                .unwrap()
                .is_empty()
        );
    }
    let first = Db::at(&path).unwrap();
    let second = Db::at(&path).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = [(first, 30., 60.), (second, 60., 95.)]
        .into_iter()
        .map(|(db, a, b)| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut measured = Intervals::default();
                measured.add(a, b).unwrap();
                barrier.wait();
                db.catalog_record_playback(f, 1, &measured, 100., false)
                    .unwrap()
            })
        })
        .collect();
    let changed: Vec<_> = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    assert_eq!(changed, vec![film.id.clone()]);
    {
        let db = Db::at(&path).unwrap();
        let watched = db.catalog_watched(&film.id).unwrap().unwrap();
        assert!(watched.watched);
        assert_eq!(watched.revision, 1);
        assert!(db.progress(f).is_none());
        assert!(
            db.catalog_record_playback(f, 1, &Intervals::default(), 100., true)
                .unwrap()
                .is_empty()
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mismatched_databases_are_rejected_without_changes_or_backups() {
    let dir=std::env::temp_dir().join(format!("erika-format-test-{}",MediaId::new().0));
    std::fs::create_dir(&dir).unwrap();
    for (index,version,app) in [(0,0,0),(1,2,0),(2,999,super::super::APPLICATION_ID),(3,1,7)] {
        let path=dir.join(format!("{index}.db"));
        {
            let conn=Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE untouched(value TEXT); INSERT INTO untouched VALUES('keep');").unwrap();
            conn.pragma_update(None,"user_version",version).unwrap();
            conn.pragma_update(None,"application_id",app).unwrap();
        }
        let before=std::fs::read(&path).unwrap();
        assert!(Db::at(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(),before);
        let conn=Connection::open(&path).unwrap();
        assert_eq!(conn.query_row("SELECT value FROM untouched",[],|r|r.get::<_,String>(0)).unwrap(),"keep");
        assert_eq!(conn.query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0)).unwrap(),version);
    }
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(),4);
    std::fs::remove_dir_all(dir).unwrap();
}
