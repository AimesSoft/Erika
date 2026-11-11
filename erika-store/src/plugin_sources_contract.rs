//! The same source lifecycle contract runs against SQLite and browser storage.
macro_rules! contract {($name:ident,$db:ty)=>{mod $name {
    use crate::plugin_sources::*;
    use erika_catalog::{*,resolve::Resolved,source::Ordering};
    type Db=$db;
    const OWNER:&str="test.source";
    fn descriptor()->Descriptor { Descriptor{id:"library".into(),server_id:"server-a".into(),name:"资料库".into(),content:"series".into()} }
    fn item(id:&str)->Item {Item{source_item_id:id.into(),server_id:"server-a".into(),media_source_id:"original".into(),kind:Kind::Movie,series_item_id:None,season_item_id:None,provider_refs:vec![ProviderRef::tmdb(Kind::Movie,71).unwrap()],series_provider_refs:vec![],season_number:None,episode_number:None,episode_number_end:None,ordering:Ordering::Unknown,title:"电影".into(),playback_handle:format!("play-{id}"),fingerprint:None}}
    fn publish(db:&Db,source:i64,items:&[Item])->Scan {
        let scan=db.plugin_begin_scan(OWNER,source).unwrap();
        if !items.is_empty(){db.plugin_upsert_items(OWNER,&scan.id,0,items).unwrap();}
        db.plugin_finish_scan(OWNER,&scan.id,u32::from(!items.is_empty()),true).unwrap()
    }
    fn resolved()->Resolved {
        let work=tmdb::parse_work(Kind::Movie,71,&serde_json::json!({"id":71,"title":"电影"})).unwrap();
        Resolved{items:vec![BindingItem{media:work.node.id.clone(),start_seconds:None,end_seconds:None}],work,season:None,recognition:None,evidence:"source:verified-tmdb-movie".into()}
    }
    #[test]
    fn portable_source_restore_preserves_owner_and_reuses_registration_without_scan_leases() {
        let sender=Db::in_memory();
        let source=sender.plugin_register_source(OWNER,&descriptor()).unwrap();
        publish(&sender,source,&[item("one")]);
        let previous=sender.plugin_source_items(OWNER,source).unwrap().remove(0);
        let r=resolved();sender.plugin_bind_source(&previous,0,&r).unwrap();
        sender.save_progress(previous.file,17.25,30.);
        sender.remember(&previous.url,"最近来源影片",None,17.25,30.);
        let pending=sender.plugin_begin_scan(OWNER,source).unwrap();
        sender.plugin_upsert_items(OWNER,&pending.id,0,&[item("pending")]).unwrap();
        let portable=sender.portable_sources().remove(0);
        let receiver=Db::in_memory();
        receiver.add_source("folder","本机目录","file:///different","movie").unwrap();
        let (restored,new)=receiver.put_source(&portable).unwrap();
        assert!(new);assert_ne!(restored,source);
        assert_eq!(receiver.plugin_source_ids(OWNER).unwrap(),vec![restored]);
        assert!(receiver.plugin_source_items(OWNER,restored).unwrap().is_empty());
        assert!(receiver.plugin_finish_scan(OWNER,&pending.id,1,true).is_err());
        assert!(receiver.plugin_begin_scan("other.plugin",restored).is_err());
        assert_eq!(receiver.put_source(&portable).unwrap(),(restored,false));
        assert_eq!(receiver.plugin_register_source(OWNER,&descriptor()).unwrap(),restored);
        receiver.catalog_import(&sender.catalog_export().unwrap(),true).unwrap();
        for progress in sender.portable_progress(){receiver.put_progress(&progress);}
        for recent in sender.portable_recents(){receiver.put_recent(&recent);}
        publish(&receiver,restored,&[item("one")]);
        let after=receiver.plugin_source_items(OWNER,restored).unwrap().remove(0);
        assert_eq!(receiver.progress(after.file),Some((17.25,30.)));
        assert_eq!(receiver.catalog_binding(after.file).unwrap().unwrap().items,r.items);
        let recent=receiver.recents(10).remove(0);
        assert_eq!(receiver.plugin_source_item(&recent.url).unwrap(),Some(after.clone()));
        assert_eq!(after.url,previous.url);
        receiver.put_source(&portable).unwrap();
        assert_eq!(receiver.plugin_source_items(OWNER,restored).unwrap(),vec![after]);
        assert_eq!(receiver.sources().len(),2);
    }
    #[test]
    fn invalid_or_conflicting_portable_sources_are_rejected_without_partial_writes() {
        let db=Db::in_memory();
        let id=db.plugin_register_source(OWNER,&descriptor()).unwrap();
        let before=db.portable_sources();
        let mut conflict=descriptor();conflict.server_id="other-server".into();
        assert!(db.put_source(&conflict.portable(OWNER)).is_err());
        for location in ["not-json", "[\"owner\",\"id\"]", "[\"owner\",\"id\",\"https://private.invalid\"]"] {
            let mut bad=descriptor().portable(OWNER);bad.location=location.into();
            assert!(db.put_source(&bad).is_err());
        }
        assert_eq!(db.portable_sources(),before);
        assert_eq!(db.plugin_source_ids(OWNER).unwrap(),vec![id]);
        assert!(db.transaction(|draft| {
            draft.set_setting("restore-test",Some("partial"));
            let mut next=descriptor();next.id="second".into();
            draft.put_source(&next.portable(OWNER))?;
            draft.put_source(&conflict.portable(OWNER))?;
            Ok(())
        }).is_err());
        assert_eq!(db.portable_sources(),before);
        assert_eq!(db.setting("restore-test"),None);
    }
    #[test]
    fn a_caught_nested_restore_failure_rolls_back_only_its_draft() {
        let db=Db::in_memory();
        db.transaction(|outer| {
            outer.set_setting("outer",Some("kept"));
            assert!(outer.transaction(|inner| {
                inner.put_source(&descriptor().portable(OWNER))?;
                inner.set_setting("inner",Some("discarded"));
                Err::<(),_>(crate::catalog::Error::invalid("invalid fixture"))
            }).is_err());
            assert!(outer.plugin_source_ids(OWNER)?.is_empty());
            outer.put_source(&descriptor().portable(OWNER))?;
            Ok(())
        }).unwrap();
        assert_eq!(db.setting("outer").as_deref(),Some("kept"));
        assert_eq!(db.setting("inner"),None);
        assert_eq!(db.plugin_source_ids(OWNER).unwrap().len(),1);
    }
    #[test]
    fn source_addresses_scope_libraries_even_when_the_file_identity_is_shared() {
        let db=Db::in_memory();let source=db.plugin_register_source(OWNER,&descriptor()).unwrap();
        let mut other=descriptor();other.id="second-library".into();
        let second=db.plugin_register_source(OWNER,&other).unwrap();
        publish(&db,source,&[item("one")]);publish(&db,second,&[item("one")]);
        let first=db.plugin_source_items(OWNER,source).unwrap().remove(0);
        let next=db.plugin_source_items(OWNER,second).unwrap().remove(0);
        assert_eq!(first.file,next.file);assert_ne!(first.url,next.url);
        db.remove_source(source);
        assert!(db.plugin_source_item(&first.url).unwrap().is_none());
        assert_eq!(db.plugin_source_item(&next.url).unwrap(),Some(next));
    }
    #[test]
    fn scanning_is_owned_paged_idempotent_and_published_only_when_complete() {
        let db=Db::in_memory();let d=descriptor();
        let source=db.plugin_register_source(OWNER,&d).unwrap();
        assert_eq!(source,db.plugin_register_source(OWNER,&d).unwrap());
        let other=db.plugin_register_source("other.source",&d).unwrap();assert_ne!(other,source);
        assert!(db.plugin_begin_scan("other.source",source).is_err());
        publish(&db,source,&[item("old")]);
        let before=db.plugin_source_items(OWNER,source).unwrap();
        let scan=db.plugin_begin_scan(OWNER,source).unwrap();
        assert!(db.plugin_upsert_items("other.source",&scan.id,0,&[item("new")]).is_err());
        assert!(db.plugin_upsert_items(OWNER,&scan.id,1,&[item("new")]).is_err());
        let page=db.plugin_upsert_items(OWNER,&scan.id,0,&[item("new")]).unwrap();
        assert_eq!(page,db.plugin_upsert_items(OWNER,&scan.id,0,&[item("new")]).unwrap());
        assert!(db.plugin_upsert_items(OWNER,&scan.id,0,&[item("different")]).is_err());
        assert_eq!(db.plugin_source_items(OWNER,source).unwrap(),before);
        assert!(db.plugin_finish_scan(OWNER,&scan.id,2,true).is_err());
        let done=db.plugin_finish_scan(OWNER,&scan.id,1,true).unwrap();
        assert_eq!(done,db.plugin_finish_scan(OWNER,&scan.id,1,true).unwrap());
        let after=db.plugin_source_items(OWNER,source).unwrap();
        assert_eq!(after.len(),1);assert_eq!(after[0].item.source_item_id,"new");
        assert!(db.plugin_source_item(&before[0].url).unwrap().is_none());
        assert!(db.plugin_upsert_items(OWNER,&scan.id,1,&[item("late")]).is_err());
        assert_eq!(db.sources().iter().find(|s|s.id==source).unwrap().files,1);
    }
    #[test]
    fn cancellation_new_generations_and_invalid_pages_preserve_the_previous_directory() {
        let db=Db::in_memory();let source=db.plugin_register_source(OWNER,&descriptor()).unwrap();
        publish(&db,source,&[item("old")]);
        let before=db.plugin_source_items(OWNER,source).unwrap();
        let scan=db.plugin_begin_scan(OWNER,source).unwrap();
        let mut wrong=item("wrong");wrong.server_id="another".into();
        assert!(db.plugin_upsert_items(OWNER,&scan.id,0,&[item("first"),wrong]).is_err());
        db.plugin_upsert_items(OWNER,&scan.id,0,&[item("new")]).unwrap();
        db.plugin_finish_scan(OWNER,&scan.id,1,false).unwrap();
        assert_eq!(db.plugin_source_items(OWNER,source).unwrap(),before);
        let a=db.plugin_begin_scan(OWNER,source).unwrap();
        let b=db.plugin_begin_scan(OWNER,source).unwrap();
        assert!(db.plugin_finish_scan(OWNER,&a.id,0,true).is_err());
        assert_eq!(db.plugin_source_items(OWNER,source).unwrap(),before);
        db.plugin_finish_scan(OWNER,&b.id,0,true).unwrap();
        assert!(db.plugin_source_items(OWNER,source).unwrap().is_empty());
    }
    #[test]
    fn stable_identity_survives_handle_changes_but_not_servers_versions_or_owners() {
        let db=Db::in_memory();let source=db.plugin_register_source(OWNER,&descriptor()).unwrap();
        let a=item("one");let mut version=a.clone();version.media_source_id="other-cut".into();
        publish(&db,source,&[a.clone(),version]);
        let before=db.plugin_source_items(OWNER,source).unwrap();
        assert_ne!(before[0].file,before[1].file);
        let original=before.iter().find(|i|i.item.media_source_id=="original").unwrap();
        db.save_progress(original.file,33.,100.);
        let mut changed=a;changed.playback_handle="new-handle".into();
        publish(&db,source,&[changed]);
        let after=db.plugin_source_items(OWNER,source).unwrap();
        assert_eq!(after[0].file,original.file);assert_eq!(after[0].url,original.url);
        assert_eq!(db.progress(after[0].file),Some((33.,100.)));
        let mut d=descriptor();d.server_id="different-server".into();
        assert!(db.plugin_register_source(OWNER,&d).is_err());
        d.id="other-library".into();let other=db.plugin_register_source(OWNER,&d).unwrap();
        let mut remote=item("one");remote.server_id=d.server_id;
        publish(&db,other,&[remote]);
        assert_ne!(after[0].file,db.plugin_source_items(OWNER,other).unwrap()[0].file);
        let other=db.plugin_register_source("other.plugin",&descriptor()).unwrap();
        let scan=db.plugin_begin_scan("other.plugin",other).unwrap();
        db.plugin_upsert_items("other.plugin",&scan.id,0,&[item("one")]).unwrap();
        db.plugin_finish_scan("other.plugin",&scan.id,1,true).unwrap();
        assert_ne!(after[0].file,db.plugin_source_items("other.plugin",other).unwrap()[0].file);
    }
    #[test]
    fn verified_fingerprints_reuse_files_and_stale_results_cannot_override_manual_decisions() {
        let db=Db::in_memory();let source=db.plugin_register_source(OWNER,&descriptor()).unwrap();
        let fp=crate::Fingerprint{md5:"a".repeat(32),size:100};
        let local=db.opened("file:///movie.mkv",Some(&fp)).unwrap();
        let mut a=item("one");a.fingerprint=Some(fp);
        publish(&db,source,&[a.clone()]);
        let old=db.plugin_source_items(OWNER,source).unwrap().remove(0);assert_eq!(old.file,local);
        let r=resolved();db.plugin_bind_source(&old,0,&r).unwrap();
        assert_eq!(db.catalog_binding(local).unwrap().unwrap().items,r.items);
        publish(&db,source,&[a]);
        assert!(db.plugin_bind_source(&old,1,&r).is_err());
        let current=db.plugin_source_items(OWNER,source).unwrap().remove(0);
        let blocked=FileBinding{file:local,revision:1,decision:Decision::Blocked,items:vec![],evidence:"manual".into()};
        db.catalog_apply(&[],&[],&[blocked],&[]).unwrap();
        assert!(db.plugin_bind_source(&current,1,&r).is_err());
        assert!(db.plugin_bind_source(&current,2,&r).is_err());
        assert_eq!(db.catalog_revision(local).unwrap(),2);
        db.remove_source(source);
        assert!(db.plugin_source_item(&current.url).unwrap().is_none());
        assert!(db.plugin_bind_source(&current,2,&r).is_err());
        assert_eq!(db.catalog_binding(local).unwrap().unwrap().decision,Decision::Blocked);
    }
    #[test]
    fn durable_source_jobs_reject_rescans_and_preserve_pending_candidates() {
        use crate::catalog::CatalogJob;
        use erika_catalog::resolve::{FileResolution,Resolution};
        let db=Db::in_memory();let source=db.plugin_register_source(OWNER,&descriptor()).unwrap();
        publish(&db,source,&[item("one")]);
        let old=db.plugin_source_items(OWNER,source).unwrap().remove(0);
        let make_job=|entry:&StoredItem| CatalogJob {id:MediaId::new().0,file:Some(entry.file),input_revision:0,phase:"source".into(),state:"queued".into(),payload:serde_json::json!({"sourceItem":entry}).to_string(),error:String::new(),next_attempt:0};
        let j=make_job(&old);db.catalog_job_enqueue(&j).unwrap();db.catalog_job_transition(&j.id,"queued","running","source","",0).unwrap();
        publish(&db,source,&[item("one")]);
        let report=FileResolution{recognition:None,recognition_error:None,resolution:Ok(Resolution::Bound{result:resolved()})};
        assert_eq!(db.catalog_finish_resolution(&j.id,0,&report).unwrap().state,"stale");
        assert!(db.catalog_binding(old.file).unwrap().is_none());
        let current=db.plugin_source_items(OWNER,source).unwrap().remove(0);
        let j=make_job(&current);db.catalog_job_enqueue(&j).unwrap();db.catalog_job_transition(&j.id,"queued","running","source","",0).unwrap();
        let report=FileResolution{recognition:None,recognition_error:None,resolution:Ok(Resolution::Pending{candidates:vec![resolved().work.node],recognition:None,reason:"编排待确认".into()})};
        let finished=db.catalog_finish_resolution(&j.id,0,&report).unwrap();
        assert_eq!(finished.state,"pending");assert_eq!(job_source(&finished).unwrap(),Some(current));
        let value:serde_json::Value=serde_json::from_str(&finished.payload).unwrap();
        let restored:FileResolution=serde_json::from_value(value["result"].clone()).unwrap();
        assert!(matches!(restored.resolution,Ok(Resolution::Pending{candidates,..}) if candidates.len()==1));
        assert!(db.catalog_finish_resolution(&j.id,0,&report).is_err());
        assert!(db.catalog_binding(old.file).unwrap().is_none());
    }
    #[test]
    fn keyset_pages_are_bounded_scoped_and_pin_the_published_generation() {
        use page::{ListRequest,ItemRequest};
        let db=Db::in_memory();
        let source=db.plugin_register_source(OWNER,&descriptor()).unwrap();
        let mut request=ListRequest::new(source);request.limit=2;
        assert!(db.plugin_source_page(OWNER,&request).unwrap().items.is_empty());
        let items:Vec<_>=(0..5).map(|i|item(&format!("item-{i}"))).collect();
        let published=publish(&db,source,&items);
        let first=db.plugin_source_page(OWNER,&request).unwrap();
        assert_eq!(first.generation,published.id);assert_eq!(first.items.len(),2);
        request.cursor=first.next_cursor.clone();
        let stage=db.plugin_begin_scan(OWNER,source).unwrap();
        db.plugin_upsert_items(OWNER,&stage.id,0,&[item("new")]).unwrap();
        let second=db.plugin_source_page(OWNER,&request).unwrap();
        assert_eq!(second.items[0].item.source_item_id,"item-2");
        db.plugin_finish_scan(OWNER,&stage.id,1,false).unwrap();
        assert_eq!(second,db.plugin_source_page(OWNER,&request).unwrap());
        request.cursor=second.next_cursor;let last=db.plugin_source_page(OWNER,&request).unwrap();
        assert_eq!(last.items.len(),1);assert!(last.next_cursor.is_none());
        let query=ItemRequest {source,source_item_id:"item-3".into(),media_source_id:"original".into()};
        let direct=db.plugin_source_version(OWNER,&query).unwrap().unwrap();
        assert_eq!(direct,second.items[1]);
        assert!(db.plugin_source_version("another",&query).is_err());
        assert!(db.plugin_source_status("another",&query).is_err());
        assert!(db.plugin_source_page("another",&request).is_err());
        let mut d=descriptor();d.id="other".into();
        let other=db.plugin_register_source(OWNER,&d).unwrap();
        let mut crossed=request.clone();crossed.source=other;
        assert!(db.plugin_source_page(OWNER,&crossed).is_err());
        publish(&db,source,&[]);
        assert!(db.plugin_source_page(OWNER,&request).is_err());
        assert!(db.plugin_source_version(OWNER,&query).unwrap().is_none());
        assert!(db.plugin_source_status(OWNER,&query).unwrap().is_none());
        request.cursor=None;
        assert!(db.plugin_source_page(OWNER,&request).unwrap().items.is_empty());
        for limit in [0,1001,usize::MAX] {request.limit=limit;assert!(db.plugin_source_page(OWNER,&request).is_err());}
        request.limit=1;request.cursor=Some("x".repeat(8193));assert!(db.plugin_source_page(OWNER,&request).is_err());
    }
    #[test]
    fn byte_limited_pages_do_not_skip_or_repeat_large_entries() {
        let db=Db::in_memory();let source=db.plugin_register_source(OWNER,&descriptor()).unwrap();
        let rows:Vec<_>=(0..600).map(|i|{let mut v=item(&format!("item-{i:04}"));v.title="长".repeat(2730);v}).collect();
        let scan=db.plugin_begin_scan(OWNER,source).unwrap();
        for (page,chunk) in rows.chunks(300).enumerate(){db.plugin_upsert_items(OWNER,&scan.id,page as u32,chunk).unwrap();}
        db.plugin_finish_scan(OWNER,&scan.id,2,true).unwrap();
        let mut request=page::ListRequest::new(source);request.limit=1000;
        let mut read=Vec::new();let mut pages=0;
        loop {
            let page=db.plugin_source_page(OWNER,&request).unwrap();
            assert!(serde_json::to_vec(&page).unwrap().len()<=page::MAX_RESPONSE_BYTES);
            read.extend(page.items.into_iter().map(|i|i.item.source_item_id));pages+=1;
            let Some(cursor)=page.next_cursor else {break;};request.cursor=Some(cursor);
        }
        assert!(pages>1);assert_eq!(read,rows.into_iter().map(|i|i.source_item_id).collect::<Vec<_>>());
    }
    #[test]
    fn repeated_items_cannot_bypass_the_scan_page_limit() {
        let db=Db::in_memory();let source=db.plugin_register_source(OWNER,&descriptor()).unwrap();
        publish(&db,source,&[item("old")]);
        let scan=db.plugin_begin_scan(OWNER,source).unwrap();
        for page in 0..MAX_PAGES {db.plugin_upsert_items(OWNER,&scan.id,page,&[item("same")]).unwrap();}
        assert!(db.plugin_upsert_items(OWNER,&scan.id,MAX_PAGES,&[item("same")]).is_err());
        assert!(db.plugin_upsert_items(OWNER,&scan.id,MAX_PAGES-1,&[item("same")]).is_ok());
        db.plugin_finish_scan(OWNER,&scan.id,MAX_PAGES,false).unwrap();
        assert_eq!(db.plugin_source_items(OWNER,source).unwrap()[0].item.source_item_id,"old");
    }
    #[test]
    fn source_status_reports_candidates_and_revisions_without_exposing_job_inputs() {
        use crate::catalog::CatalogJob;
        use erika_catalog::resolve::{FileResolution,Resolution};
        let db=Db::in_memory();let source=db.plugin_register_source(OWNER,&descriptor()).unwrap();
        publish(&db,source,&[item("one")]);
        let query=page::ItemRequest{source,source_item_id:"one".into(),media_source_id:"original".into()};
        let entry=db.plugin_source_version(OWNER,&query).unwrap().unwrap();
        assert!(db.plugin_source_status(OWNER,&query).unwrap().unwrap().job.is_none());
        let j=CatalogJob{id:MediaId::new().0,file:Some(entry.file),input_revision:0,phase:"source".into(),state:"queued".into(),payload:serde_json::json!({"sourceItem":entry}).to_string(),error:String::new(),next_attempt:0};
        db.catalog_job_enqueue(&j).unwrap();db.catalog_job_transition(&j.id,"queued","running","source","",0).unwrap();
        let report=FileResolution{recognition:None,recognition_error:None,resolution:Ok(Resolution::Pending{candidates:vec![resolved().work.node],recognition:None,reason:"编排待确认".into()})};
        db.catalog_finish_resolution(&j.id,0,&report).unwrap();
        let status=db.plugin_source_status(OWNER,&query).unwrap().unwrap();
        let status_json=serde_json::to_value(&status).unwrap();
        assert!(status_json["job"].get("payload").is_none());
        let job=status.job.unwrap();assert!(job.current);assert_eq!(job.state,"pending");assert_eq!(job.error,"编排待确认");assert_eq!(job.candidate_count,1);assert_eq!(job.candidates.len(),1);
        db.catalog_apply(&[],&[],&[FileBinding{file:entry.file,revision:0,decision:Decision::Blocked,items:vec![],evidence:"manual".into()}],&[]).unwrap();
        let status=db.plugin_source_status(OWNER,&query).unwrap().unwrap();assert_eq!(status.revision,1);
        assert_eq!(status.binding.unwrap().decision,Decision::Blocked);
        let job=status.job.unwrap();assert!(!job.current);assert!(job.candidates.is_empty());
    }
}};}
contract!(sqlite, crate::sqlite::Db);
contract!(browser, crate::session::Db);
