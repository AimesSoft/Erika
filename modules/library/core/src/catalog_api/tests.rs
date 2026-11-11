use super::*;
use bern::{Host, CallContext, Entry, bind::BernValue};
use bern::__private::protocol::{Invocation, Value};
use serde_json::json;
use std::time::{Duration, Instant};

fn call(host: &Host, name: &str, params: Vec<Value>) -> Result<Value, CommandError> {
    let command = host.descriptor().commands.iter().position(|c| c.name == name).expect("generated operation");
    host.query_blocking(Invocation {command, params}, CallContext {entry:Entry::Cli,request_id:MediaId::new().0}, Duration::from_secs(5)).result
}
fn finished(host: &Host, request: &str) -> CatalogJob {
    let deadline=Instant::now()+Duration::from_secs(5);
    loop {
        let result=call(host,"catalog_job",vec![request.to_string().to_value()]).unwrap();
        let job=CatalogJob::from_value(&result).unwrap();
        if !matches!(job.state.as_str(),"queued"|"running") {return job;}
        assert!(Instant::now()<deadline,"job did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn work() -> media::tmdb::Work {
    media::tmdb::parse_work(media::Kind::Movie,71,&json!({"id":71,"title":"Journey","release_date":"2026-01-01"})).unwrap()
}
fn file(engine: &LibraryEngine) -> i64 {
    engine.db.opened("file:///videos/Journey.2026.mkv",Some(&erika_store::Fingerprint {md5:"1".repeat(32),size:100})).unwrap()
}

#[test]
fn generated_cli_protocol_fetches_binds_previews_and_replays() {
    let (_fixture, engine)=crate::tests::engine_with_metadata(|path| {
        if path.contains("/images") {return (200,json!({"id":71,"posters":[],"backdrops":[],"logos":[]}));}
        assert!(path.starts_with("/metadata/v1/tmdb/movie/71?"));
        (200,json!({"id":71,"title":"Journey","overview":"TMDB synopsis","release_date":"2026-01-01"}))
    });
    let f=file(&engine);
    let mut local=work().node;
    local.id=MediaId::new();local.root=local.id.clone();
    engine.db.catalog_save_nodes(&[local.clone()]).unwrap();
    let host=Host::start(library::Adapter(engine));
    let fetch=MediaId::new().0;
    let reference=reference(local.reference.as_ref().unwrap());
    call(&host,"catalog_fetch",vec![fetch.to_value(),reference.to_value(),None::<i64>.to_value()]).unwrap();
    let job=finished(&host,&fetch);
    assert_eq!(job.state,"completed");assert_eq!(job.nodes.len(),1);
    assert_eq!(job.nodes[0].id,local.id.0);assert_eq!(job.nodes[0].revision,2);
    assert_eq!(job.nodes[0].overview,"TMDB synopsis");
    call(&host,"catalog_fetch",vec![fetch.to_value(),reference.to_value(),None::<i64>.to_value()]).unwrap();
    assert_eq!(finished(&host,&fetch).nodes[0].revision,2);
    let request=MediaId::new().0;
    let bindings=vec![CatalogBinding {file:f,revision:0,decision:CatalogDecision::Manual,items:vec![CatalogItem {media:local.id.0.clone(),start_seconds:None,end_seconds:None}],evidence:"CLI".into()}];
    call(&host,"catalog_bind",vec![request.to_value(),bindings.to_value(),true.to_value()]).unwrap();
    let inspect=CatalogInspection::from_value(&call(&host,"catalog_inspect",vec![f.to_value()]).unwrap()).unwrap();
    assert!(inspect.binding.is_none());assert_eq!(inspect.revision,0);
    assert!(call(&host,"catalog_receipt",vec![request.to_value()]).is_err());
    call(&host,"catalog_bind",vec![request.to_value(),bindings.to_value(),false.to_value()]).unwrap();
    let receipt=call(&host,"catalog_receipt",vec![request.to_value()]).unwrap();
    call(&host,"catalog_bind",vec![request.to_value(),bindings.to_value(),false.to_value()]).unwrap();
    assert_eq!(receipt,call(&host,"catalog_receipt",vec![request.to_value()]).unwrap());
    let receipt=CatalogReceipt::from_value(&receipt).unwrap();assert!(!receipt.change.is_empty());
    let inspect=CatalogInspection::from_value(&call(&host,"catalog_inspect",vec![f.to_value()]).unwrap()).unwrap();
    assert_eq!(inspect.revision,1);assert_eq!(inspect.binding.unwrap().items[0].media,local.id.0);
    assert_eq!(call(&host,"catalog_bind",vec![MediaId::new().0.to_value(),bindings.to_value(),false.to_value()]).unwrap_err().code,"conflict");
    call(&host,"catalog_undo",vec![receipt.change.to_value()]).unwrap();
    let inspect=CatalogInspection::from_value(&call(&host,"catalog_inspect",vec![f.to_value()]).unwrap()).unwrap();
    assert!(inspect.binding.is_none());assert_eq!(inspect.revision,2);
    host.stop();
}

#[test]
fn generated_search_preserves_kind_year_and_page_and_is_read_only() {
    let (_fixture, engine)=crate::tests::engine_with_metadata(|path| {
        assert!(path.starts_with("/metadata/v1/search?") && path.contains("kind=movie"),"{path}");
        assert!(path.contains("year=2026") && path.contains("page=3") && path.contains("query=Journey"),"{path}");
        (200,json!({"page":3,"total_pages":7,"results":[{"id":71,"title":"Journey","release_date":"2026-01-01"}]}))
    });
    let host=Host::start(library::Adapter(engine));
    let result=call(&host,"catalog_search",vec![CatalogKind::Movie.to_value(),"Journey".to_string().to_value(),Some(2026_i64).to_value(),3_i64.to_value()]).unwrap();
    let page=CatalogSearch::from_value(&result).unwrap();
    assert_eq!((page.page,page.total_pages),(3,7));assert_eq!(page.nodes.len(),1);
    assert!(call(&host,"catalog_media",vec![page.nodes[0].id.to_value()]).is_err());
    assert!(call(&host,"catalog_search",vec![CatalogKind::Season.to_value(),"Journey".to_string().to_value(),None::<i64>.to_value(),1_i64.to_value()]).is_err());
    assert!(call(&host,"catalog_search",vec![CatalogKind::Movie.to_value(),"Journey".to_string().to_value(),None::<i64>.to_value(),(-1_i64).to_value()]).is_err());
    host.stop();
}

#[test]
fn resolving_a_manual_or_blocked_binding_never_runs_recognition() {
    let (_fixture, mut engine)=crate::tests::engine_with_metadata(|_| panic!("an existing binding needs no network"));
    let f=file(&engine);let node=work().node;
    engine.db.catalog_apply(&[node.clone()],&[],&[media::FileBinding {file:f,revision:0,decision:media::Decision::Manual,items:vec![media::BindingItem {media:node.id.clone(),start_seconds:None,end_seconds:None}],evidence:"manual".into()}],&[]).unwrap();
    for dry_run in [true,false] {
        let request=MediaId::new().0;
        engine.api_resolve(request.clone(),f,1,dry_run).unwrap();
        let job=engine.api_job(&request).unwrap();
        assert_eq!(job.state,"completed");assert_eq!(job.items[0].media,node.id.0);
        assert_eq!(engine.db.catalog_revision(f).unwrap(),1);
    }
    engine.db.forget(1, &["file:///videos/Journey.2026.mkv".into()]);
    assert_eq!(engine.api_inspect(f).unwrap().binding.unwrap().items[0].media, node.id.0);
    assert!(engine.api_inspect(i64::MAX).is_err());
    let reuse = MediaId::new().0;
    engine.api_resolve(reuse.clone(), f, 1, false).unwrap();
    assert_eq!(engine.api_job(&reuse).unwrap().state, "completed");
    engine.catalog_unbind(MediaId::new().0,f,1,false).unwrap();
    let request=MediaId::new().0;
    engine.api_resolve(request.clone(),f,2,false).unwrap();
    let blocked=engine.api_job(&request).unwrap();
    assert_eq!(blocked.state,"completed");assert!(blocked.items.is_empty());assert!(blocked.reason.contains("解除绑定"));
    engine.catalog_allow_auto(f,2).unwrap();
    // An idempotent replay is about the original operation, not today's revision.
    engine.api_resolve(request.clone(),f,2,false).unwrap();
    assert_eq!(engine.db.catalog_revision(f).unwrap(),3);
    assert!(engine.api_resolve(request,f,3,false).is_err());
    let missing = MediaId::new().0;
    assert!(engine.api_resolve(missing.clone(), f, 3, false).is_err());
    assert!(engine.db.catalog_job(&missing).unwrap().is_none());
}

#[test]
fn resolution_preview_and_commit_share_results_but_not_side_effects() {
    let (_fixture, mut engine)=crate::tests::engine();
    let f=file(&engine);let work=work();
    let mut local=work.node.clone();local.id=MediaId::new();local.root=local.id.clone();
    engine.db.catalog_save_nodes(&[local.clone()]).unwrap();
    let report=FileResolution {recognition:None,recognition_error:None,resolution:Ok(Resolution::Bound {result:media::resolve::Resolved {items:vec![media::BindingItem {media:work.node.id.clone(),start_seconds:None,end_seconds:None}],work,season:None,recognition:None,evidence:"filename".into()}})};
    for dry_run in [true,false] {
        let id=MediaId::new().0;
        let request=json!({"operation":"resolve","file":f,"revision":0,"dry_run":dry_run});
        engine.api_start_job(&id,Some(f),0,"recognition",&request).unwrap();
        let changed=engine.take_api(Reply::Resolve {id:id.clone(),request:request.clone(),dry_run,danmaku_revision:0,report:report.clone()});
        assert_eq!(changed,!dry_run);
        let job=engine.api_job(&id).unwrap();
        assert_eq!(job.items[0].media,local.id.0);assert_eq!(job.nodes[0].id,local.id.0);
        assert_eq!(job.state,if dry_run {"completed"} else {"bound"});
        assert_eq!(engine.db.catalog_revision(f).unwrap(),u64::from(!dry_run));
        assert!(engine.api_replayed(&id,&request).unwrap());
        engine.api_resolve(id,f,0,dry_run).unwrap();
    }
}

#[test]
fn late_read_reply_cannot_change_the_catalogue_or_emit_file_changes() {
    let (_fixture, mut engine)=crate::tests::engine();
    let f=file(&engine);let id=MediaId::new().0;
    let request=json!({"operation":"resolve","file":f,"revision":0,"dry_run":true});
    engine.api_start_job(&id,Some(f),0,"recognition",&request).unwrap();
    engine.db.catalog_allow_auto(f,0).unwrap();
    let report=FileResolution {recognition:None,recognition_error:None,resolution:Err(media::Error::unavailable())};
    assert!(!engine.take_api(Reply::Resolve {id:id.clone(),request,dry_run:true,danmaku_revision:0,report}));
    assert_eq!(engine.api_job(&id).unwrap().state,"stale");
    assert!(engine.db.catalog_export().unwrap().nodes.is_empty());
}
