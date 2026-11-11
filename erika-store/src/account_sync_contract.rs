//! Durable subject delivery and shared lanes must agree on both backends.
use crate::{
    account_sync::Delivery,
    catalog::{MarkEdit, WatchSync},
};
use erika_catalog::{Kind, MediaId, Provider, ProviderLink, ProviderRef, Relation, tmdb};
const ACCOUNT: &str = "bangumi:isolated";
macro_rules! contract {
    ($name:ident, $db:ty) => {
        mod $name {
            use super::*;
            type Db = $db;
            fn setup(db: &Db) -> (MediaId, ProviderLink) {
                let movie = tmdb::parse_work(Kind::Movie, 71, &serde_json::json!({"id":71,"title":"测试电影"})).unwrap().node;
                db.catalog_save_nodes(&[movie.clone()]).unwrap();
                let link = db.catalog_link(&ProviderLink { media:movie.id.clone(), reference:ProviderRef {provider:Provider::Bangumi, scope:String::new(), kind:Kind::Subject, id:"77".into()}, relation:Relation::Exact, coverage:vec![], confirmed:true, manual:true, evidence:"isolated".into(), revision:0 }).unwrap();
                (movie.id, link)
            }
            #[test]
            fn claiming_before_read_serializes_engines_but_not_accounts() {
                let db = Db::in_memory(); let (id, link) = setup(&db);
                let mark = db.catalog_edit_mark(&id, &MarkEdit::Rating(8)).unwrap();
                let task = db.catalog_subject_enqueue(ACCOUNT,&mark,&link).unwrap();
                assert_eq!(task, db.catalog_subject_enqueue(ACCOUNT,&mark,&link).unwrap());
                let claim = db.catalog_account_claim(ACCOUNT,&link,&Delivery::Subject(task.clone())).unwrap();
                assert_eq!(db.catalog_account_claim(ACCOUNT,&link,&Delivery::Subject(task.clone())).unwrap_err().code,"busy");
                let other = db.catalog_subject_enqueue("bangumi:another",&mark,&link).unwrap();
                db.catalog_account_claim("bangumi:another",&link,&Delivery::Subject(other)).unwrap();
                let sent = db.catalog_subject_finish(&claim,&task,Some(true),"").unwrap();
                assert_eq!(sent.state,"sent");
                assert!(!db.catalog_account_holds(&claim).unwrap());
                assert!(db.catalog_subject_finish(&claim,&task,Some(true),"").is_err());
                let export = serde_json::to_string(&db.catalog_export().unwrap()).unwrap();
                assert!(!export.contains(ACCOUNT));
                assert!(!export.contains(&task.operation));
            }
            #[test]
            fn field_aba_and_mapping_aba_are_rechecked_at_send() {
                let db = Db::in_memory(); let (id, mut link) = setup(&db);
                let mark = db.catalog_edit_mark(&id, &MarkEdit::Rating(8)).unwrap();
                let task = db.catalog_subject_enqueue(ACCOUNT,&mark,&link).unwrap();
                let claim = db.catalog_account_claim(ACCOUNT,&link,&Delivery::Subject(task.clone())).unwrap();
                db.catalog_edit_mark(&id,&MarkEdit::Rating(7)).unwrap();
                db.catalog_edit_mark(&id,&MarkEdit::Rating(8)).unwrap();
                assert!(db.catalog_account_begin(&claim,&Delivery::Subject(task.clone())).is_err());
                let cancelled = db.catalog_subject_finish(&claim,&task,None,"").unwrap();
                assert_eq!(cancelled.state,"cancelled");
                let mark = db.catalog_marks(&id).unwrap().remove(0);
                let next = db.catalog_subject_enqueue(ACCOUNT,&mark,&link).unwrap();
                let claim = db.catalog_account_claim(ACCOUNT,&link,&Delivery::Subject(next.clone())).unwrap();
                link.confirmed = false; link = db.catalog_link(&link).unwrap();
                link.confirmed = true; db.catalog_link(&link).unwrap();
                assert!(db.catalog_account_begin(&claim,&Delivery::Subject(next.clone())).is_err());
                assert_eq!(db.catalog_subject_finish(&claim,&next,None,"").unwrap().state,"cancelled");
            }
            #[test]
            fn uncertain_old_write_blocks_new_choice_until_readback() {
                let db = Db::in_memory(); let (id, link) = setup(&db);
                let mark = db.catalog_edit_mark(&id,&MarkEdit::Review("旧短评".into())).unwrap();
                let task = db.catalog_subject_enqueue(ACCOUNT,&mark,&link).unwrap();
                let claim = db.catalog_account_claim(ACCOUNT,&link,&Delivery::Subject(task.clone())).unwrap();
                let Delivery::Subject(sending) = db.catalog_account_begin(&claim,&Delivery::Subject(task)).unwrap() else { panic!() };
                let unknown = db.catalog_subject_finish(&claim,&sending,None,"timeout").unwrap();
                assert_eq!(unknown.state,"unknown");
                let mark = db.catalog_edit_mark(&id,&MarkEdit::Review(String::new())).unwrap();
                let next = db.catalog_subject_enqueue(ACCOUNT,&mark,&link).unwrap();
                assert_eq!(db.catalog_account_claim(ACCOUNT,&link,&Delivery::Subject(next.clone())).unwrap_err().code,"busy");
                let retry = db.catalog_account_claim(ACCOUNT,&link,&Delivery::Subject(unknown.clone())).unwrap();
                assert_ne!(retry.token,claim.token);
                assert!(db.catalog_subject_finish(&claim,&sending,Some(true),"").is_err());
                assert_eq!(db.catalog_subject_finish(&retry,&unknown,Some(false),"").unwrap().state,"conflict");
                let claim = db.catalog_account_claim(ACCOUNT,&link,&Delivery::Subject(next.clone())).unwrap();
                let Delivery::Subject(sent) = db.catalog_account_begin(&claim,&Delivery::Subject(next)).unwrap() else { panic!() };
                assert_eq!(sent.mark.review.as_deref(),Some(""));
                assert_eq!(db.catalog_subject_finish(&claim,&sent,Some(true),"").unwrap().state,"sent");
            }
            #[test]
            fn reverse_mapping_ambiguity_rejects_preflight_and_send() {
                let db = Db::in_memory(); let (id, link) = setup(&db);
                let mark = db.catalog_edit_mark(&id,&MarkEdit::Rating(8)).unwrap();
                let task = db.catalog_subject_enqueue(ACCOUNT,&mark,&link).unwrap();
                let claim = db.catalog_account_claim(ACCOUNT,&link,&Delivery::Subject(task.clone())).unwrap();
                let other = tmdb::parse_work(Kind::Movie,72,&serde_json::json!({"id":72,"title":"另一个电影"})).unwrap().node;
                db.catalog_save_nodes(&[other.clone()]).unwrap();
                let mut duplicate = link.clone(); duplicate.media = other.id; duplicate.revision = 0;
                db.catalog_link(&duplicate).unwrap();
                assert!(db.catalog_account_begin(&claim,&Delivery::Subject(task.clone())).is_err());
                assert_eq!(db.catalog_subject_finish(&claim,&task,None,"").unwrap().state,"cancelled");
                assert!(db.catalog_subject_enqueue(ACCOUNT,&mark,&link).is_err());
            }
            #[test]
            fn a_watch_readback_token_cannot_finish_a_later_execution() {
                let db = Db::in_memory(); let (id, link) = setup(&db);
                let watched = db.catalog_set_watched(&erika_catalog::Watched {media:id.clone(), watched:true, manual:true, updated_at:1, revision:0}).unwrap();
                let task = db.catalog_watch_enqueue(&WatchSync {operation:String::new(),media:id.clone(),account:ACCOUNT.into(),target:link.reference.clone(),watched:true,local_revision:watched.revision,mapping_revision:link.revision,state:"pending".into(),error:String::new(),remote_baseline:None,revision:0,lease_until:0}).unwrap();
                let claim = db.catalog_account_claim(ACCOUNT,&link,&Delivery::Watch(task.clone())).unwrap();
                let Delivery::Watch(sending) = db.catalog_account_begin(&claim,&Delivery::Watch(task)).unwrap() else {panic!()};
                let unknown = db.catalog_account_finish_watch(&claim,&sending,"unknown","timeout",None).unwrap();
                let reader = db.catalog_account_claim(ACCOUNT,&link,&Delivery::Watch(unknown.clone())).unwrap();
                db.catalog_account_release(&reader,true).unwrap();
                let retry = db.catalog_account_claim(ACCOUNT,&link,&Delivery::Watch(unknown.clone())).unwrap();
                assert!(db.catalog_account_finish_watch(&reader,&unknown,"sent","",Some(true)).is_err());
                assert!(db.catalog_account_holds(&retry).unwrap());
                assert_eq!(db.catalog_watch_queue(ACCOUNT).unwrap(),vec![unknown.clone()]);
                assert_eq!(db.catalog_account_finish_watch(&retry,&unknown,"sent","",Some(true)).unwrap().state,"sent");
                assert!(!db.catalog_account_holds(&retry).unwrap());
                let mark = db.catalog_edit_mark(&id,&MarkEdit::Rating(8)).unwrap();
                let subject = db.catalog_subject_enqueue(ACCOUNT,&mark,&link).unwrap();
                db.catalog_account_claim(ACCOUNT,&link,&Delivery::Subject(subject)).unwrap();
            }
            fn watch(db: &Db, id: &MediaId, link: &ProviderLink, value: bool) -> WatchSync {
                let old = db.catalog_watched(id).unwrap();
                let watched = db.catalog_set_watched(&erika_catalog::Watched {
                    media: id.clone(), watched: value, manual: true, updated_at: 1,
                    revision: old.map_or(0, |w| w.revision),
                }).unwrap();
                db.catalog_watch_enqueue(&WatchSync {
                    operation: String::new(), media: id.clone(), account: ACCOUNT.into(),
                    target: link.reference.clone(), watched: value, local_revision: watched.revision,
                    mapping_revision: link.revision, state: "pending".into(), error: String::new(),
                    remote_baseline: None, revision: 0, lease_until: 0,
                }).unwrap()
            }
            #[test]
            fn contradictory_movie_choices_block_both_writes_without_changing_local_data() {
                for (status, value) in [("watching", true), ("dropped", true), ("watched", false)] {
                    let db = Db::in_memory(); let (id, link) = setup(&db);
                    let mark = db.catalog_edit_mark(&id, &MarkEdit::Status(status.into())).unwrap();
                    let subject = db.catalog_subject_enqueue(ACCOUNT, &mark, &link).unwrap();
                    let watch = watch(&db, &id, &link, value);
                    let local = db.catalog_export().unwrap();
                    for task in [Delivery::Subject(subject), Delivery::Watch(watch)] {
                        let error = db.catalog_account_claim(ACCOUNT, &link, &task).unwrap_err();
                        assert_eq!(error.code, "conflict");
                        assert!(error.message.contains("电影收藏状态与已看记录冲突"));
                    }
                    assert_eq!(db.catalog_export().unwrap(), local);
                    let mark = db.catalog_edit_mark(&id, &MarkEdit::Status(if value {"watched"} else {"watching"}.into())).unwrap();
                    let task = db.catalog_subject_enqueue(ACCOUNT, &mark, &link).unwrap();
                    let claim = db.catalog_account_claim(ACCOUNT, &link, &Delivery::Subject(task.clone())).unwrap();
                    db.catalog_account_begin(&claim, &Delivery::Subject(task)).unwrap();
                }
            }
            #[test]
            fn movie_preflight_rechecks_both_local_records_and_the_mark_revision() {
                let db = Db::in_memory(); let (id, link) = setup(&db);
                let mark = db.catalog_edit_mark(&id, &MarkEdit::Status("watching".into())).unwrap();
                let task = watch(&db, &id, &link, false);
                let claim = db.catalog_account_claim(ACCOUNT, &link, &Delivery::Watch(task.clone())).unwrap();
                assert_eq!(claim.movie_mark, Some(mark.clone()));
                let mut forged = claim.clone(); forged.movie_mark = None;
                assert!(!db.catalog_account_holds(&forged).unwrap());
                db.catalog_edit_mark(&id, &MarkEdit::Status("dropped".into())).unwrap();
                db.catalog_edit_mark(&id, &MarkEdit::Status("watching".into())).unwrap();
                assert!(db.catalog_account_begin(&claim, &Delivery::Watch(task.clone())).is_err());
                assert_eq!(db.catalog_watch_queue(ACCOUNT).unwrap(), vec![task]);
                db.catalog_account_release(&claim, false).unwrap();
                let mark = db.catalog_marks(&id).unwrap().remove(0);
                let task = db.catalog_subject_enqueue(ACCOUNT, &mark, &link).unwrap();
                let claim = db.catalog_account_claim(ACCOUNT, &link, &Delivery::Subject(task.clone())).unwrap();
                watch(&db, &id, &link, true);
                assert!(db.catalog_account_begin(&claim, &Delivery::Subject(task.clone())).is_err());
                assert_eq!(db.catalog_subject_queue(ACCOUNT).unwrap(), vec![task]);
            }
            #[test]
            fn a_movie_conflict_cannot_prevent_reading_back_an_uncertain_write() {
                let db = Db::in_memory(); let (id, link) = setup(&db);
                let mark = db.catalog_edit_mark(&id, &MarkEdit::Status("watching".into())).unwrap();
                let task = watch(&db, &id, &link, false);
                let claim = db.catalog_account_claim(ACCOUNT, &link, &Delivery::Watch(task.clone())).unwrap();
                let Delivery::Watch(sending) = db.catalog_account_begin(&claim, &Delivery::Watch(task)).unwrap() else {panic!()};
                let unknown = db.catalog_account_finish_watch(&claim, &sending, "unknown", "timeout", None).unwrap();
                db.catalog_edit_mark(&id, &MarkEdit::Status("watched".into())).unwrap();
                let reader = db.catalog_account_claim(ACCOUNT, &link, &Delivery::Watch(unknown.clone())).unwrap();
                assert_eq!(reader.movie_mark, Some(mark));
                assert!(db.catalog_account_begin(&reader, &Delivery::Watch(unknown.clone())).is_err());
                assert_eq!(db.catalog_account_finish_watch(&reader, &unknown, "sent", "", Some(false)).unwrap().state, "sent");
                assert_eq!(db.catalog_marks(&id).unwrap()[0].status.as_deref(), Some("watched"));
                assert!(!db.catalog_watched(&id).unwrap().unwrap().watched);
            }
            #[test]
            fn movie_watch_and_review_share_the_remote_collection_lane() {
                let db = Db::in_memory(); let (id, link) = setup(&db);
                let mark = db.catalog_edit_mark(&id,&MarkEdit::Review("保留评分和短评".into())).unwrap();
                let subject = db.catalog_subject_enqueue(ACCOUNT,&mark,&link).unwrap();
                let watched = db.catalog_set_watched(&erika_catalog::Watched {media:id.clone(), watched:true, manual:true, updated_at:1, revision:0}).unwrap();
                let watch = db.catalog_watch_enqueue(&WatchSync {operation:String::new(),media:id,account:ACCOUNT.into(),target:link.reference.clone(),watched:true,local_revision:watched.revision,mapping_revision:link.revision,state:"pending".into(),error:String::new(),remote_baseline:None,revision:0,lease_until:0}).unwrap();
                let claim = db.catalog_account_claim(ACCOUNT,&link,&Delivery::Watch(watch.clone())).unwrap();
                assert_eq!(db.catalog_account_claim(ACCOUNT,&link,&Delivery::Subject(subject.clone())).unwrap_err().code,"busy");
                let Delivery::Watch(sending) = db.catalog_account_begin(&claim,&Delivery::Watch(watch)).unwrap() else {panic!()};
                db.catalog_account_finish_watch(&claim,&sending,"sent","",Some(true)).unwrap();
                db.catalog_account_claim(ACCOUNT,&link,&Delivery::Subject(subject)).unwrap();
            }
        }
    };
}
contract!(sqlite, crate::sqlite::Db);
contract!(browser, crate::session::Db);

#[test]
fn sqlite_reopen_retains_uncertain_write_without_exporting_it() {
    let root = std::env::temp_dir().join(format!("erika-account-{}", MediaId::new()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("db.sqlite");
    let db = crate::Db::at(&path).unwrap();
    let movie = tmdb::parse_work(
        Kind::Movie,
        71,
        &serde_json::json!({"id":71,"title":"测试"}),
    )
    .unwrap()
    .node;
    db.catalog_save_nodes(&[movie.clone()]).unwrap();
    let link = db
        .catalog_link(&ProviderLink {
            media: movie.id.clone(),
            reference: ProviderRef {
                provider: Provider::Bangumi,
                scope: String::new(),
                kind: Kind::Subject,
                id: "77".into(),
            },
            relation: Relation::Exact,
            coverage: vec![],
            confirmed: true,
            manual: true,
            evidence: String::new(),
            revision: 0,
        })
        .unwrap();
    let mark = db
        .catalog_edit_mark(&movie.id, &MarkEdit::Rating(0))
        .unwrap();
    let task = db.catalog_subject_enqueue(ACCOUNT, &mark, &link).unwrap();
    let claim = db
        .catalog_account_claim(ACCOUNT, &link, &Delivery::Subject(task.clone()))
        .unwrap();
    let Delivery::Subject(sending) = db
        .catalog_account_begin(&claim, &Delivery::Subject(task))
        .unwrap()
    else {
        panic!()
    };
    let unknown = db
        .catalog_subject_finish(&claim, &sending, None, "timeout")
        .unwrap();
    drop(db);
    let db = crate::Db::at(&path).unwrap();
    assert_eq!(
        db.catalog_subject_queue(ACCOUNT).unwrap(),
        vec![unknown.clone()]
    );
    let claim = db
        .catalog_account_claim(ACCOUNT, &link, &Delivery::Subject(unknown.clone()))
        .unwrap();
    assert_eq!(
        db.catalog_subject_finish(&claim, &unknown, Some(true), "")
            .unwrap()
            .state,
        "sent"
    );
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}

fn durable_fixture() -> (
    std::path::PathBuf,
    crate::Db,
    crate::catalog::MediaMark,
    ProviderLink,
) {
    let root = std::env::temp_dir().join(format!("erika-account-crash-{}", MediaId::new()));
    std::fs::create_dir(&root).unwrap();
    let db = crate::Db::at(&root.join("db.sqlite")).unwrap();
    let movie = tmdb::parse_work(
        Kind::Movie,
        71,
        &serde_json::json!({"id":71,"title":"隔离电影"}),
    )
    .unwrap()
    .node;
    db.catalog_save_nodes(&[movie.clone()]).unwrap();
    let link = db
        .catalog_link(&ProviderLink {
            media: movie.id.clone(),
            reference: ProviderRef {
                provider: Provider::Bangumi,
                scope: String::new(),
                kind: Kind::Subject,
                id: "77".into(),
            },
            relation: Relation::Exact,
            coverage: vec![],
            confirmed: true,
            manual: true,
            evidence: String::new(),
            revision: 0,
        })
        .unwrap();
    let mark = db
        .catalog_edit_mark(&movie.id, &MarkEdit::Rating(8))
        .unwrap();
    (root, db, mark, link)
}

// Advance only the isolated fixture's persisted lease, not the system clock.
fn expire_fixture_lane(root: &std::path::Path) {
    let conn = rusqlite::Connection::open(root.join("db.sqlite")).unwrap();
    assert_eq!(conn.execute("UPDATE account_sync SET data=json_set(data,'$.lanes[0].lease_until',0) WHERE account=?1",[ACCOUNT]).unwrap(),1);
}

#[test]
fn concurrent_sqlite_connections_allow_exactly_one_preflight() {
    let (root, db, mark, link) = durable_fixture();
    let task = db.catalog_subject_enqueue(ACCOUNT, &mark, &link).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let path = root.join("db.sqlite");
            let barrier = barrier.clone();
            let link = link.clone();
            let task = task.clone();
            std::thread::spawn(move || {
                let db = crate::Db::at(&path).unwrap();
                barrier.wait();
                db.catalog_account_claim(ACCOUNT, &link, &Delivery::Subject(task))
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .all(|e| e.code == "busy")
    );
    let claim = results.into_iter().find_map(Result::ok).unwrap();
    assert!(db.catalog_account_holds(&claim).unwrap());
    db.catalog_subject_finish(&claim, &task, Some(true), "")
        .unwrap();
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn sqlite_crash_before_send_retries_but_after_send_requires_readback() {
    for sent in [false, true] {
        let (root, db, mark, mut link) = durable_fixture();
        let task = db.catalog_subject_enqueue(ACCOUNT, &mark, &link).unwrap();
        let claim = db
            .catalog_account_claim(ACCOUNT, &link, &Delivery::Subject(task.clone()))
            .unwrap();
        if sent {
            db.catalog_account_begin(&claim, &Delivery::Subject(task.clone()))
                .unwrap();
        }
        drop(db); // No completion callback or explicit release.
        expire_fixture_lane(&root);
        let db = crate::Db::at(&root.join("db.sqlite")).unwrap();
        let recovered = db.catalog_subject_queue(ACCOUNT).unwrap().remove(0);
        assert_eq!(recovered.state, if sent { "unknown" } else { "pending" });
        assert_eq!(
            recovered.revision,
            if sent {
                task.revision + 2
            } else {
                task.revision
            }
        );
        assert!(!db.catalog_account_holds(&claim).unwrap());
        if sent {
            let newer = db
                .catalog_edit_mark(&mark.media, &MarkEdit::Rating(9))
                .unwrap();
            let next = db.catalog_subject_enqueue(ACCOUNT, &newer, &link).unwrap();
            assert_eq!(
                db.catalog_account_claim(ACCOUNT, &link, &Delivery::Subject(next))
                    .unwrap_err()
                    .code,
                "busy"
            );
            let original = link.clone();
            link.confirmed = false;
            link = db.catalog_link(&link).unwrap();
            let reader = db
                .catalog_account_claim(ACCOUNT, &original, &Delivery::Subject(recovered.clone()))
                .unwrap();
            assert!(
                db.catalog_account_begin(&reader, &Delivery::Subject(recovered.clone()))
                    .is_err()
            );
            assert!(
                db.catalog_subject_finish(&claim, &recovered, Some(true), "")
                    .is_err()
            );
            assert_eq!(
                db.catalog_subject_finish(&reader, &recovered, Some(true), "")
                    .unwrap()
                    .state,
                "sent"
            );
            link.confirmed = true;
            link = db.catalog_link(&link).unwrap();
            let next = db.catalog_subject_enqueue(ACCOUNT, &newer, &link).unwrap();
            db.catalog_account_claim(ACCOUNT, &link, &Delivery::Subject(next))
                .unwrap();
        } else {
            let retry = db
                .catalog_account_claim(ACCOUNT, &link, &Delivery::Subject(recovered.clone()))
                .unwrap();
            assert_ne!(claim.token, retry.token);
            assert!(
                db.catalog_subject_finish(&claim, &recovered, Some(true), "")
                    .is_err()
            );
            assert!(
                matches!(db.catalog_account_begin(&retry,&Delivery::Subject(recovered)).unwrap(),Delivery::Subject(t) if t.state=="sending")
            );
        }
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn a_crashed_watch_holds_the_subject_lane_until_its_readback() {
    let (root, db, mark, link) = durable_fixture();
    let watched = db
        .catalog_set_watched(&erika_catalog::Watched {
            media: mark.media.clone(),
            watched: true,
            manual: true,
            updated_at: 1,
            revision: 0,
        })
        .unwrap();
    let watch = db
        .catalog_watch_enqueue(&WatchSync {
            operation: String::new(),
            media: mark.media.clone(),
            account: ACCOUNT.into(),
            target: link.reference.clone(),
            watched: true,
            local_revision: watched.revision,
            mapping_revision: link.revision,
            state: "pending".into(),
            error: String::new(),
            remote_baseline: None,
            revision: 0,
            lease_until: 0,
        })
        .unwrap();
    let claim = db
        .catalog_account_claim(ACCOUNT, &link, &Delivery::Watch(watch.clone()))
        .unwrap();
    let Delivery::Watch(sending) = db
        .catalog_account_begin(&claim, &Delivery::Watch(watch))
        .unwrap()
    else {
        panic!()
    };
    drop(db);
    expire_fixture_lane(&root);
    let db = crate::Db::at(&root.join("db.sqlite")).unwrap();
    db.catalog_watch_recover(ACCOUNT, sending.lease_until + 1)
        .unwrap();
    let unknown = db.catalog_watch_queue(ACCOUNT).unwrap().remove(0);
    assert_eq!(unknown.state, "unknown");
    let subject = db.catalog_subject_enqueue(ACCOUNT, &mark, &link).unwrap();
    assert_eq!(
        db.catalog_account_claim(ACCOUNT, &link, &Delivery::Subject(subject.clone()))
            .unwrap_err()
            .code,
        "busy"
    );
    let reader = db
        .catalog_account_claim(ACCOUNT, &link, &Delivery::Watch(unknown.clone()))
        .unwrap();
    assert!(
        db.catalog_account_finish_watch(&claim, &unknown, "sent", "", Some(true))
            .is_err()
    );
    db.catalog_account_finish_watch(&reader, &unknown, "sent", "", Some(true))
        .unwrap();
    db.catalog_account_claim(ACCOUNT, &link, &Delivery::Subject(subject))
        .unwrap();
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}
