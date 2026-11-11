//! Both backends implement the same explicit-edit and metadata-read contract.
macro_rules! contract {
    ($name:ident, $db:ty) => { mod $name {
        use erika_catalog::*;
        use crate::catalog::{CatalogBatch, CatalogJob};
        use serde_json::json;
        type Db = $db;
        fn movie(id: u64) -> MediaNode {
            tmdb::parse_work(Kind::Movie, id, &json!({"id":id,"title":format!("Movie {id}")})).unwrap().node
        }
        fn file(db: &Db, n: u64) -> i64 {
            db.opened(&format!("file:///test/{n}.mkv"), Some(&crate::Fingerprint { md5:format!("{n:032x}"),size:100 })).unwrap()
        }
        fn binding(f: i64, n: &MediaNode, revision: u64) -> FileBinding {
            FileBinding { file:f, revision, decision:Decision::Manual, evidence:"manual".into(), items:vec![BindingItem {media:n.id.clone(),start_seconds:None,end_seconds:None}] }
        }
        fn track(f: i64) -> DanmakuBinding {
            DanmakuBinding {file:f,reference:ProviderRef {provider:Provider::Dandanplay,scope:String::new(),kind:Kind::Episode,id:"123".into()},media:None,manual:true,fingerprint_verified:false,revision:0,shift_seconds:0.,user_shift_seconds:2.}
        }
        fn link(n: &MediaNode, external: &str) -> ProviderLink {
            ProviderLink { media:n.id.clone(),reference:ProviderRef {provider:Provider::Bangumi,scope:String::new(),kind:Kind::Subject,id:external.into()},relation:Relation::Exact,coverage:vec![],evidence:"confirmed by user".into(),confirmed:true,manual:true,revision:0 }
        }
        fn job(db: &Db, file: Option<i64>, phase: &str) -> CatalogJob {
            let j = CatalogJob {id:MediaId::new().0,file,input_revision:0,state:"queued".into(),phase:phase.into(),payload:"{}".into(),error:String::new(),next_attempt:0};
            db.catalog_job_enqueue(&j).unwrap();
            db.catalog_job_transition(&j.id,"queued","running",phase,"",0).unwrap()
        }
        #[test]
        fn file_addresses_include_direct_opens_and_distinguish_unknown_files() {
            let db = Db::in_memory();
            let fp = crate::Fingerprint { md5: "1".repeat(32), size: 100 };
            let f = db.opened("file:///direct.mkv", Some(&fp)).unwrap();
            let direct = db.catalog_addresses(f).unwrap();
            assert_eq!(direct.len(), 1);
            assert_eq!(direct[0].url, "file:///direct.mkv");
            let source = db.add_source("local", "Movies", "file:///movies", "movie").unwrap();
            assert_eq!(db.saw(source, "file:///movies/copy.mkv", 100, 0, Some(&fp)), Some(f));
            let addresses = db.catalog_addresses(f).unwrap();
            assert_eq!(addresses.len(), 2);
            assert_eq!(addresses[0].source, source);
            assert_eq!(addresses[1], direct[0]);
            db.remove_source(source);
            assert_eq!(db.catalog_addresses(f).unwrap(), direct);
            db.forget(1, &["file:///direct.mkv".into()]);
            assert!(db.catalog_addresses(f).unwrap().is_empty());
            assert_eq!(db.catalog_addresses(i64::MAX).unwrap_err().code, "invalid");
            assert_eq!(db.catalog_revision(f).unwrap(), 0);
        }
        #[test]
        fn preview_is_rollback_and_replay_keeps_the_original_receipt() {
            let db=Db::in_memory();let f=file(&db,1);let n=movie(1);db.catalog_save_nodes(&[n.clone()]).unwrap();
            let request=MediaId::new().0;
            let batch=CatalogBatch {bindings:vec![binding(f,&n,0)],danmaku:vec![track(f)],links:vec![link(&n,"12")]};
            let before=db.catalog_export().unwrap();
            let preview=db.catalog_request(&request,&batch,true).unwrap();
            assert_eq!(preview.change.after[0].revision,1);
            assert_eq!(db.catalog_export().unwrap(),before);
            assert!(db.catalog_receipt(&request).unwrap().is_none());
            assert!(db.catalog_undo(&preview.change.id).is_err());
            let receipt=db.catalog_request(&request,&batch,false).unwrap();
            assert_eq!(db.catalog_receipt(&request).unwrap(),Some(receipt.clone()));
            assert_eq!(db.catalog_request(&request,&batch,false).unwrap(),receipt);
            assert_eq!(db.catalog_request(&request,&batch,true).unwrap(),receipt);
            assert_eq!(db.catalog_revision(f).unwrap(),1);
            assert_eq!(db.catalog_danmaku_revision(f).unwrap(),1);
            let mut different=batch.clone();different.bindings[0].evidence="another operation".into();
            assert_eq!(db.catalog_request(&request,&different,false).unwrap_err().code,"conflict");
            db.catalog_undo(&receipt.change.id).unwrap();
            assert_eq!(db.catalog_request(&request,&batch,false).unwrap(),receipt);
            assert!(db.catalog_binding(f).unwrap().is_none());
            assert_eq!(db.catalog_revision(f).unwrap(),2);
        }
        #[test]
        fn late_failure_cannot_commit_an_earlier_binding_or_a_receipt() {
            let db=Db::in_memory();let f=file(&db,1);let n=movie(1);db.catalog_save_nodes(&[n.clone()]).unwrap();
            let request=MediaId::new().0;
            let mut batch=CatalogBatch {bindings:vec![binding(f,&n,0),binding(i64::MAX,&n,0)],..Default::default()};
            for preview in [true,false] {
                assert!(db.catalog_request(&request,&batch,preview).is_err());
                assert!(db.catalog_binding(f).unwrap().is_none());
                assert!(db.catalog_receipt(&request).unwrap().is_none());
                assert_eq!(db.catalog_revision(f).unwrap(),0);
            }
            batch.bindings.pop();
            db.catalog_request(&request,&batch,false).unwrap();
        }
        #[test]
        fn explicit_api_rejects_forged_automatic_provenance() {
            let db=Db::in_memory();let f=file(&db,1);let n=movie(1);db.catalog_save_nodes(&[n.clone()]).unwrap();
            let mut b=binding(f,&n,0);b.decision=Decision::Auto;
            let mut d=track(f);d.fingerprint_verified=true;
            let mut l=link(&n,"12");l.manual=false;
            for batch in [CatalogBatch::default(),CatalogBatch {bindings:vec![b],..Default::default()},CatalogBatch {danmaku:vec![d],..Default::default()},CatalogBatch {links:vec![l],..Default::default()}] {
                assert!(db.catalog_request(&MediaId::new().0,&batch,false).is_err());
            }
            assert!(db.catalog_binding(f).unwrap().is_none());
            assert!(db.catalog_danmaku(f).unwrap().is_none());
            assert!(db.catalog_links(&n.id).unwrap().is_empty());
        }
        #[test]
        fn exact_mapping_switch_is_explicit_and_atomic() {
            let db=Db::in_memory();let f=file(&db,1);let a=movie(1);let b=movie(2);db.catalog_save_nodes(&[a.clone(),b.clone()]).unwrap();
            let old=db.catalog_request(&MediaId::new().0,&CatalogBatch {links:vec![link(&a,"12")],..Default::default()},false).unwrap().links.remove(0);
            for wrong in [link(&b,"12"),link(&a,"13")] {
                let request=MediaId::new().0;
                let batch=CatalogBatch {bindings:vec![binding(f,&a,0)],links:vec![wrong],..Default::default()};
                assert_eq!(db.catalog_request(&request,&batch,false).unwrap_err().code,"conflict");
                assert!(db.catalog_binding(f).unwrap().is_none());
                assert!(db.catalog_receipt(&request).unwrap().is_none());
            }
            let mut revoke=old;revoke.confirmed=false;
            let batch=CatalogBatch {links:vec![link(&b,"12"),revoke],..Default::default()};
            db.catalog_request(&MediaId::new().0,&batch,false).unwrap();
            assert_eq!(db.catalog_by_ref(&link(&b,"12").reference).unwrap()[0].id,b.id);
            let mut bad=link(&a,"not-a-number");
            assert!(db.catalog_request(&MediaId::new().0,&CatalogBatch {links:vec![bad.clone()],..Default::default()},false).is_err());
            bad.reference.id="14".into();bad.reference.kind=Kind::Season;
            assert!(db.catalog_request(&MediaId::new().0,&CatalogBatch {links:vec![bad],..Default::default()},false).is_err());
        }
        #[test]
        fn allow_auto_keeps_danmaku_resume_and_a_monotonic_revision() {
            let db=Db::in_memory();let f=file(&db,1);let n=movie(1);db.catalog_save_nodes(&[n.clone()]).unwrap();
            db.save_progress(f,42.,100.);
            let receipt=db.catalog_request(&MediaId::new().0,&CatalogBatch {bindings:vec![binding(f,&n,0)],danmaku:vec![track(f)],..Default::default()},false).unwrap();
            assert!(db.catalog_allow_auto(f,0).is_err());
            assert_eq!(db.catalog_allow_auto(f,1).unwrap(),2);
            assert!(db.catalog_binding(f).unwrap().is_none());
            assert_eq!(db.catalog_danmaku(f).unwrap(),Some(receipt.change.danmaku_after[0].clone()));
            assert_eq!(db.progress(f),Some((42.,100.)));
            assert!(db.catalog_undo(&receipt.change.id).is_err());
            let mut b=binding(f,&n,2);b.decision=Decision::Auto;
            db.catalog_apply(&[],&[],&[b],&[]).unwrap();
            assert_eq!(db.catalog_revision(f).unwrap(),3);
        }
        fn television() -> Vec<MediaNode> {
            let w=tmdb::parse_work(Kind::Tv,1,&json!({"id":1,"name":"Show","seasons":[{"id":10,"season_number":0,"name":"Specials"}]})).unwrap();
            let s=tmdb::parse_season(&w,0,&json!({"id":10,"season_number":0,"name":"Specials","episodes":[{"id":100,"season_number":0,"episode_number":1,"name":"OVA"}]})).unwrap();
            vec![w.node,s.node,s.episodes[0].clone()]
        }
        fn local_ids(nodes: &[MediaNode]) -> Vec<MediaNode> {
            let mut result=nodes.to_vec();
            let ids:std::collections::HashMap<_,_>=nodes.iter().map(|n|(n.id.clone(),MediaId::new())).collect();
            for n in &mut result {n.id=ids[&n.id].clone();n.root=ids[&n.root].clone();n.parent=n.parent.as_ref().map(|p|ids[p].clone());}
            result
        }
        #[test]
        fn refresh_and_binding_resolve_typed_references_without_aliases() {
            let db=Db::in_memory();let incoming=television();let local=local_ids(&incoming);let f=file(&db,1);
            db.catalog_save_nodes(&local).unwrap();
            let saved=db.catalog_save_nodes(&incoming).unwrap();
            for (n,local) in saved.iter().zip(&local) {assert_eq!(n.id,local.id);assert_eq!(n.parent,local.parent);assert_eq!(n.root,local.root);assert_eq!(n.revision,2);}
            let film=movie(1);db.catalog_save_nodes(&[film.clone()]).unwrap();
            assert_ne!(film.id,saved[0].id);
            let mut d=track(f);d.media=Some(incoming[2].id.clone());
            let mut l=link(&incoming[1],"12");l.coverage=vec![incoming[2].id.clone()];
            let change=db.catalog_apply(&incoming,&[l],&[binding(f,&incoming[2],0)],&[d]).unwrap();
            assert_eq!(change.after[0].items[0].media,local[2].id);
            assert_eq!(change.danmaku_after[0].media,Some(local[2].id.clone()));
            assert_eq!(db.catalog_links(&local[1].id).unwrap()[0].coverage,[local[2].id.clone()]);
            for n in &incoming { assert!(db.catalog_node(&n.id).unwrap().is_none()); }
            let mut stale=saved;stale[2].title="must roll back".into();
            assert_eq!(db.catalog_save_nodes(&stale).unwrap_err().code,"conflict");
            assert_eq!(db.catalog_node(&local[0].id).unwrap().unwrap().revision,3);
        }
        #[test]
        fn uuid_collision_never_rewrites_a_different_tmdb_identity() {
            let db=Db::in_memory();let a=movie(1);db.catalog_save_nodes(&[a.clone()]).unwrap();
            let mut b=movie(2);b.id=a.id.clone();b.root=a.id.clone();
            assert!(db.catalog_save_nodes(&[b]).is_err());
            assert_eq!(db.catalog_node(&a.id).unwrap().unwrap().title,a.title);
        }
        #[test]
        fn fetch_receipt_has_stored_ids_metadata_and_revisions() {
            let db=Db::in_memory();let incoming=television();let mut local=local_ids(&incoming);local[0].overview="cached synopsis".into();
            db.catalog_save_nodes(&local).unwrap();
            let j=job(&db,None,"fetch");
            let done=db.catalog_complete_read(&j.id,&json!({"request":{"operation":"fetch"},"nodes":incoming}).to_string(),&incoming).unwrap();
            assert_eq!(done.state,"completed");assert_eq!(done.phase,"complete");
            let value:serde_json::Value=serde_json::from_str(&done.payload).unwrap();
            let saved:Vec<MediaNode>=serde_json::from_value(value["nodes"].clone()).unwrap();
            assert_eq!(saved[0].overview,"cached synopsis");
            for (n,local) in saved.iter().zip(&local) { assert_eq!(n.id,local.id);assert_eq!(db.catalog_node(&n.id).unwrap(),Some(n.clone())); }
            assert_eq!(db.catalog_jobs(Some("completed")).unwrap().len(),1);
            assert!(db.catalog_complete_read(&j.id,"{}",&incoming).is_err());
        }
        #[test]
        fn read_completion_is_atomic_and_dry_run_never_saves_nodes() {
            let db=Db::in_memory();let f=file(&db,1);let n=movie(1);
            let j=job(&db,None,"fetch");
            assert!(db.catalog_complete_read(&j.id,"not-json",&[n.clone()]).is_err());
            assert!(db.catalog_node(&n.id).unwrap().is_none());
            assert_eq!(db.catalog_job(&j.id).unwrap().unwrap().state,"running");
            let preview=job(&db,Some(f),"recognition");
            let report=resolve::FileResolution {recognition:None,recognition_error:None,resolution:Ok(resolve::Resolution::Pending {candidates:vec![n.clone()],reason:"choose".into(),recognition:None})};
            let done=db.catalog_complete_read(&preview.id,&json!({"report":report}).to_string(),&[]).unwrap();
            assert_eq!(done.state,"completed");assert_eq!(done.phase,"preview");
            assert!(db.catalog_node(&n.id).unwrap().is_none());
            assert_eq!(db.catalog_revision(f).unwrap(),0);
            let stale=job(&db,Some(f),"recognition");
            db.catalog_allow_auto(f,0).unwrap();
            assert_eq!(db.catalog_complete_read(&stale.id,"{}",&[n.clone()]).unwrap().state,"stale");
            assert!(db.catalog_node(&n.id).unwrap().is_none());
            let failed=job(&db,None,"recognition");
            let report=resolve::FileResolution {recognition:None,recognition_error:None,resolution:Err(Error::unavailable())};
            let done=db.catalog_complete_read(&failed.id,&json!({"report":report}).to_string(),&[]).unwrap();
            assert_eq!(done.state,"failed");assert!(!done.error.is_empty());
        }
    }};
}
#[cfg(not(target_arch="wasm32"))]
contract!(sqlite, crate::Db);
contract!(browser, crate::session::Db);
