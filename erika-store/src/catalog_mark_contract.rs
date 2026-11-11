//! Field edits, whole-row CAS and transport clocks agree on both backends.
use crate::catalog::{MarkEdit, MediaMark};
use erika_catalog::{Kind, MediaId, MediaNode, tmdb};

fn movie() -> MediaNode {
    tmdb::parse_work(
        Kind::Movie,
        41,
        &serde_json::json!({"id":41,"title":"电影"}),
    )
    .unwrap()
    .node
}

macro_rules! contract {
    ($name:ident, $db:ty) => {
        mod $name {
            use super::*;
            type Db = $db;
            fn setup() -> (Db, MediaId) {
                let db = Db::in_memory();
                let node = movie();
                db.catalog_save_nodes(&[node.clone()]).unwrap();
                (db, node.id)
            }
            #[test]
            fn field_edits_preserve_other_choices_and_explicit_clears() {
                let (db, id) = setup();
                let first = db
                    .catalog_edit_mark(&id, &MarkEdit::Status("watching".into()))
                    .unwrap();
                assert_eq!(first.rating, None);
                assert_eq!(first.review, None);
                db.catalog_edit_mark(&id, &MarkEdit::Rating(8)).unwrap();
                db.catalog_edit_mark(&id, &MarkEdit::Review("喜欢".into()))
                    .unwrap();
                db.catalog_edit_mark(&id, &MarkEdit::Rating(0)).unwrap();
                let mark = db
                    .catalog_edit_mark(&id, &MarkEdit::Review(String::new()))
                    .unwrap();
                assert_eq!(mark.status.as_deref(), Some("watching"));
                assert_eq!(mark.rating, Some(0));
                assert_eq!(mark.review.as_deref(), Some(""));
                assert_eq!(mark.revision, 5);
            }
            #[test]
            fn whole_row_cas_rejects_stale_and_aba_updates() {
                let (db, id) = setup();
                let first = db.catalog_edit_mark(&id, &MarkEdit::Rating(8)).unwrap();
                db.catalog_edit_mark(&id, &MarkEdit::Rating(5)).unwrap();
                let latest = db.catalog_edit_mark(&id, &MarkEdit::Rating(8)).unwrap();
                let mut stale = first;
                stale.status = Some("watched".into());
                assert_eq!(db.catalog_set_mark(&stale).unwrap_err().code, "conflict");
                assert_eq!(db.catalog_marks(&id).unwrap(), vec![latest]);
            }
            #[test]
            fn invalid_edits_roll_back_fields_and_clock() {
                let (db, id) = setup();
                let before = db.catalog_edit_mark(&id, &MarkEdit::Rating(6)).unwrap();
                for edit in [
                    MarkEdit::Rating(11),
                    MarkEdit::Status("invented".into()),
                    MarkEdit::Review("字".repeat(201)),
                ] {
                    assert!(db.catalog_edit_mark(&id, &edit).is_err());
                    assert_eq!(db.catalog_marks(&id).unwrap(), vec![before.clone()]);
                }
                assert!(
                    db.catalog_edit_mark(&MediaId::new(), &MarkEdit::Rating(1))
                        .is_err()
                );
                assert_eq!(db.catalog_marks(&id).unwrap(), vec![before]);
            }
            #[test]
            fn snapshots_preserve_reviews_but_not_foreign_cas_clocks() {
                let (db, id) = setup();
                for _ in 0..4 {
                    db.catalog_edit_mark(&id, &MarkEdit::Review("短评".into()))
                        .unwrap();
                }
                let snapshot = db.catalog_export().unwrap();
                assert_eq!(snapshot.marks[0].revision, 4);
                let portable = snapshot.for_sync().unwrap();
                assert_eq!(portable.marks[0].revision, 0);
                let other = Db::in_memory();
                other.catalog_import(&snapshot, false).unwrap();
                let mark = other.catalog_marks(&id).unwrap().remove(0);
                assert_eq!(mark.review.as_deref(), Some("短评"));
                assert_eq!(mark.revision, 1);
                other.catalog_import(&snapshot, false).unwrap();
                assert_eq!(other.catalog_marks(&id).unwrap(), vec![mark]);
                let stale = other.catalog_export().unwrap();
                other
                    .catalog_edit_mark(&id, &MarkEdit::Review("本地更新".into()))
                    .unwrap();
                assert!(other.catalog_import(&snapshot, false).is_err());
                assert!(
                    other
                        .catalog_import_checked(&portable, &stale, &[], &[])
                        .is_err()
                );
                other.catalog_import(&snapshot, true).unwrap();
                let restored = other.catalog_marks(&id).unwrap().remove(0);
                assert_eq!(restored.review.as_deref(), Some("短评"));
                assert_eq!(restored.revision, 3);
            }
            #[test]
            fn provider_and_local_marks_keep_independent_clocks() {
                let (db, id) = setup();
                let local = db.catalog_edit_mark(&id, &MarkEdit::Rating(7)).unwrap();
                let mut external = MediaMark::local(id.clone());
                external.reference = Some(erika_catalog::ProviderRef {
                    provider: erika_catalog::Provider::Bangumi,
                    scope: String::new(),
                    kind: Kind::Subject,
                    id: "71".into(),
                });
                external.rating = Some(9);
                let external = db.catalog_set_mark(&external).unwrap();
                assert_eq!(external.revision, 1);
                assert!(db.catalog_marks(&id).unwrap().contains(&local));
                assert_eq!(
                    db.catalog_edit_mark(&id, &MarkEdit::Rating(0))
                        .unwrap()
                        .revision,
                    2
                );
                assert!(db.catalog_marks(&id).unwrap().contains(&external));
            }
        }
    };
}
contract!(sqlite, crate::sqlite::Db);
contract!(browser, crate::session::Db);

macro_rules! concurrent {
    ($observer:expr, $connections:expr) => {{
        let id = movie().id;
        $observer.catalog_save_nodes(&[movie()]).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let edits = [
            MarkEdit::Status("watching".into()),
            MarkEdit::Rating(7),
            MarkEdit::Review("并发短评".into()),
        ];
        let workers: Vec<_> = $connections
            .into_iter()
            .zip(edits)
            .map(|(db, edit)| {
                let id = id.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    for _ in 0..30 {
                        db.catalog_edit_mark(&id, &edit).unwrap();
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let mark = $observer.catalog_marks(&id).unwrap().remove(0);
        assert_eq!(mark.revision, 90);
        assert_eq!(mark.status.as_deref(), Some("watching"));
        assert_eq!(mark.rating, Some(7));
        assert_eq!(mark.review.as_deref(), Some("并发短评"));
        mark
    }};
}
#[test]
fn sqlite_connections_merge_fields_and_persist_after_reopen() {
    let root = std::env::temp_dir().join(format!("erika-mark-{}", MediaId::new()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("db.sqlite");
    let observer = crate::Db::at(&path).unwrap();
    let connections: Vec<_> = (0..3).map(|_| crate::Db::at(&path).unwrap()).collect();
    let expected = concurrent!(observer, connections);
    drop(observer);
    let reopened = crate::Db::at(&path).unwrap();
    assert_eq!(
        reopened.catalog_marks(&expected.media).unwrap(),
        vec![expected]
    );
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn browser_engines_merge_fields_under_the_same_lock() {
    let db = crate::session::Db::in_memory();
    concurrent!(db, vec![db.clone(), db.clone(), db.clone()]);
}
