//! The sync transaction has the same failure and CAS semantics on both backends.
macro_rules! contract {
    ($name:ident, $db:path) => {
        mod $name {
            use $db as Db;
            use crate::{Fingerprint, catalog::{CatalogSnapshot, PortableBinding}, portable::FileKey};
            use erika_catalog::{tmdb, BindingItem, Decision, FileBinding, Kind, MediaId};
            fn setup() -> (Db, i64, erika_catalog::MediaNode, FileKey) {
                let db = Db::in_memory();
                let key = FileKey::Hash { md5: "a".repeat(32), size: 100 };
                let file = db.opened("file:///sync.mkv", Some(&Fingerprint { md5:"a".repeat(32), size:100 })).unwrap();
                let node = tmdb::parse_work(Kind::Movie,1,&serde_json::json!({"id":1,"title":"Movie"})).unwrap().node;
                db.catalog_apply(&[node.clone()],&[],&[FileBinding {
                    file, decision:Decision::Manual,
                    items:vec![BindingItem{media:node.id.clone(),start_seconds:None,end_seconds:None}],
                    evidence:String::new(), revision:0,
                }],&[]).unwrap();
                (db,file,node,key)
            }
            #[test]
            fn checked_import_is_atomic_and_rejects_stale_read() {
                let (db,file,node,key) = setup();
                let expected = db.catalog_export().unwrap();
                let mut bad = expected.for_sync().unwrap();
                bad.bindings.push(PortableBinding {
                    file:FileKey::Hash {md5:"b".repeat(32),size:200},
                    binding:FileBinding {file:0,decision:Decision::Manual,
                        items:vec![BindingItem {media:MediaId::new(),start_seconds:None,end_seconds:None}],
                        evidence:String::new(),revision:0},
                });
                assert!(db.catalog_import_checked(&bad,&expected,&[key.clone()],&[]).is_err());
                assert_eq!(db.catalog_export().unwrap(),expected,"deleted prefix rolled back too");
                db.catalog_set_mark(&crate::catalog::MediaMark {media:node.id,reference:None,status:None,rating:Some(9),review:None,revision:0,updated_at:1}).unwrap();
                let newer = db.catalog_export().unwrap();
                assert!(db.catalog_import_checked(&CatalogSnapshot::default(),&expected,&[key.clone()],&[]).is_err());
                assert_eq!(db.catalog_export().unwrap(),newer);
                let revision = db.catalog_revision(file).unwrap();
                db.catalog_import_checked(&CatalogSnapshot::default(),&newer,&[key],&[]).unwrap();
                assert!(db.catalog_binding(file).unwrap().is_none());
                assert!(db.catalog_revision(file).unwrap()>revision);
            }
            #[test]
            fn current_transport_ids_are_stable_and_import_does_not_churn_nodes() {
                let (db,_,node,_) = setup();
                let initial = db.catalog_export().unwrap();
                let portable = initial.for_sync().unwrap();
                assert_eq!(portable.nodes[0].id,MediaId::for_ref(node.reference.as_ref().unwrap()));
                assert_eq!(portable.nodes[0].revision,0);
                assert_eq!(portable.bindings[0].binding.file,0);
                assert_eq!(portable.bindings[0].binding.revision,0);
                db.catalog_import_checked(&portable,&initial,&[],&[]).unwrap();
                assert_eq!(db.catalog_export().unwrap(),initial);
            }
        }
    };
}
contract!(sqlite, crate::sqlite::Db);
contract!(browser, crate::session::Db);
