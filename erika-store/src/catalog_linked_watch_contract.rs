//! The provider evidence and local watch revision are checked in one commit.
macro_rules! contract {
    ($name:ident, $db:path) => {
        mod $name {
            use $db as Db;
            use erika_catalog::{tmdb, Kind, MediaNode, Provider, ProviderLink, ProviderRef, Relation, Watched};
            use serde_json::json;
            fn setup() -> (Db, MediaNode, ProviderLink, Watched) {
                let db = Db::in_memory();
                let node = tmdb::parse_work(Kind::Movie, 701, &json!({"id":701,"title":"Film"})).unwrap().node;
                db.catalog_save_nodes(&[node.clone()]).unwrap();
                let link = db.catalog_link(&ProviderLink {
                    media: node.id.clone(), reference: ProviderRef { provider: Provider::Dandanplay,
                        scope: String::new(), kind: Kind::Episode, id: "12345".into() },
                    relation: Relation::Exact, coverage: vec![], confirmed: true, manual: true,
                    evidence: "test".into(), revision: 0,
                }).unwrap();
                let watch = Watched { media: node.id.clone(), watched: true, manual: true, revision: 0, updated_at: 1 };
                (db, node, link, watch)
            }
            #[test]
            fn linked_mark_commits_once_and_keeps_watch_cas() {
                let (db, node, link, watch) = setup();
                let stored = db.catalog_set_watched_linked(&watch, &[link.clone()]).unwrap();
                assert_eq!(stored.revision, 1);
                assert_eq!(db.catalog_set_watched_linked(&watch, &[link]).unwrap_err().code, "conflict");
                assert_eq!(db.catalog_watched(&node.id).unwrap(), Some(stored));
            }
            #[test]
            fn linked_mark_rejects_aba_edits_and_unconfirmed_evidence() {
                let (db, node, link, watch) = setup();
                let mut changed = link.clone();
                changed.confirmed = false;
                changed = db.catalog_link(&changed).unwrap();
                assert!(db.catalog_set_watched_linked(&watch, &[changed.clone()]).is_err());
                changed.confirmed = true;
                db.catalog_link(&changed).unwrap();
                assert_eq!(db.catalog_set_watched_linked(&watch, &[link]).unwrap_err().code, "conflict");
                assert!(db.catalog_watched(&node.id).unwrap().is_none());
            }
            #[test]
            fn linked_mark_rejects_both_directions_of_mapping_ambiguity() {
                for reverse in [false, true] {
                    let (db, node, link, watch) = setup();
                    let mut competing = link.clone();
                    competing.revision = 0;
                    if reverse {
                        let another = tmdb::parse_work(Kind::Movie, 702, &json!({"id":702,"title":"Another"})).unwrap().node;
                        db.catalog_save_nodes(&[another.clone()]).unwrap();
                        competing.media = another.id;
                    } else {
                        competing.reference.id = "67890".into();
                    }
                    db.catalog_link(&competing).unwrap();
                    assert_eq!(db.catalog_set_watched_linked(&watch, &[link]).unwrap_err().code, "conflict");
                    assert!(db.catalog_watched(&node.id).unwrap().is_none());
                }
            }
            #[test]
            fn every_guard_must_hold_and_no_prefix_is_published() {
                let (db, node, episode, watch) = setup();
                let mut subject = episode.clone();
                subject.reference.kind = Kind::Subject;
                subject.reference.id = "42".into();
                subject.revision = 0;
                let subject = db.catalog_link(&subject).unwrap();
                let mut edited = subject.clone();
                edited.evidence = "new evidence".into();
                db.catalog_link(&edited).unwrap();
                assert!(db.catalog_set_watched_linked(&watch, &[episode, subject]).is_err());
                assert!(db.catalog_watched(&node.id).unwrap().is_none());
                assert!(db.catalog_set_watched_linked(&watch, &[]).is_err());
            }
        }
    };
}
contract!(sqlite, crate::sqlite::Db);
contract!(browser, crate::session::Db);
