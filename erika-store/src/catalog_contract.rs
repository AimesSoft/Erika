//! The same observable catalogue contract runs against both storage backends.
macro_rules! contract {
    ($name:ident,$db:ty) => { mod $name {
        use erika_catalog::*;
        use serde_json::json;
        type Db=$db;
        fn movie(id:u64)->MediaNode { tmdb::parse_work(Kind::Movie,id,&json!({"id":id,"title":format!("Movie {id}")})).unwrap().node }
        fn file(db:&Db,name:&str)->i64 {db.opened(&format!("https://example.invalid/{name}.mkv"),Some(&crate::Fingerprint {md5:name.repeat(32),size:100})).unwrap()}
        fn binding(file:i64,n:&MediaNode,revision:u64,decision:Decision)->FileBinding {FileBinding {file,items:vec![BindingItem {media:n.id.clone(),start_seconds:None,end_seconds:None}],decision,revision,evidence:"test".into()}}
        fn dm(file:i64)->DanmakuBinding {DanmakuBinding {file,reference:ProviderRef {provider:Provider::Dandanplay,scope:String::new(),kind:Kind::Episode,id:"opaque-episode".into()},media:None,manual:true,fingerprint_verified:false,shift_seconds:5.,user_shift_seconds:-1.,revision:0}}
        #[test]
        fn file_identity_and_resume_are_shared_between_addresses() {
            let db=Db::in_memory();let a=file(&db,"a");
            let copy=db.opened("https://other.invalid/a.mkv",Some(&crate::Fingerprint {md5:"a".repeat(32),size:100})).unwrap();
            assert_eq!(a,copy);assert_eq!(db.opened("https://example.invalid/a.mkv",None),Some(a));
            db.save_progress(a,15.,90.);assert_eq!(db.progress(copy),Some((15.,90.)));
            db.set_setting("test",Some("kept"));assert_eq!(db.setting("test").as_deref(),Some("kept"));
        }
        #[test]
        fn manual_binding_and_danmaku_are_independent_and_atomic() {
            let db=Db::in_memory();let f=file(&db,"a");let a=movie(1);let b=movie(2);
            let danmaku=db.catalog_set_danmaku(&dm(f)).unwrap();
            db.catalog_apply(&[a.clone()],&[],&[binding(f,&a,0,Decision::Manual)],&[]).unwrap();
            assert!(db.catalog_apply(&[b.clone()],&[],&[binding(f,&b,0,Decision::Auto)],&[]).is_err());
            assert!(db.catalog_node(&b.id).unwrap().is_none());
            assert_eq!(db.catalog_binding(f).unwrap().unwrap().items[0].media,a.id);
            assert_eq!(db.catalog_danmaku(f).unwrap().unwrap(),danmaku);
            assert!(db.catalog_links(&a.id).unwrap().is_empty());
            db.catalog_apply(&[],&[],&[FileBinding {file:f,decision:Decision::Blocked,items:vec![],evidence:"user".into(),revision:1}],&[]).unwrap();
            assert!(db.catalog_apply(&[b.clone()],&[],&[binding(f,&b,2,Decision::Auto)],&[]).is_err());
            assert_eq!(db.catalog_danmaku(f).unwrap().unwrap(),danmaku);
        }
        #[test]
        fn partial_metadata_refresh_keeps_details_and_checks_artwork_identity() {
            let db=Db::in_memory();
            let film=tmdb::parse_work(Kind::Movie,12,&json!({"id":12,"title":"TMDB title","overview":"TMDB synopsis","poster_path":"/poster.jpg"})).unwrap().node;
            db.catalog_save_nodes(&[film.clone()]).unwrap();
            let mut partial=movie(12);partial.overview.clear();partial.artwork=Default::default();
            db.catalog_save_nodes(&[partial]).unwrap();
            let saved=db.catalog_node(&film.id).unwrap().unwrap();
            assert_eq!(saved.overview,film.overview);assert_eq!(saved.artwork,film.artwork);
            let mut forged=movie(13);forged.artwork=film.artwork;
            assert!(db.catalog_save_nodes(&[forged]).is_err());
        }
        #[test]
        fn provider_records_never_create_catalogue_nodes_or_replace_bindings() {
            let db=Db::in_memory();let f=file(&db,"c");
            let a=crate::Anime {dandanplay_id:56,title:"Provider title".into(),image:String::new()};
            let e=crate::Episode {dandanplay_id:5601,number:1,title:"Provider episode".into()};
            db.link_episode(f,&a,&e,true);
            assert!(db.catalog_export().unwrap().nodes.is_empty());
            assert!(db.catalog_binding(f).unwrap().is_none());
            assert_eq!(db.file_episode(f).unwrap().1.dandanplay_id,5601);
            let film=movie(12);
            db.catalog_apply(&[film.clone()],&[],&[binding(f,&film,0,Decision::Manual)],&[]).unwrap();
            db.link_episode(f,&a,&e,true);
            assert_eq!(db.catalog_binding(f).unwrap().unwrap().items[0].media,film.id);
            assert!(db.portable_links().is_empty());
        }
        #[test]
        fn invalid_batch_does_not_commit_the_valid_prefix() {
            let db=Db::in_memory();let f=file(&db,"a");let n=movie(1);
            assert!(db.catalog_apply(&[n.clone()],&[],&[binding(f,&n,0,Decision::Manual),binding(i64::MAX,&n,0,Decision::Manual)],&[]).is_err());
            assert!(db.catalog_binding(f).unwrap().is_none());assert!(db.catalog_node(&n.id).unwrap().is_none());
            assert_eq!(db.catalog_revision(f).unwrap(),0);
        }
        fn addresses(db:&Db, source:i64)->Vec<crate::catalog::AddressCheck> {
            db.entries(Some(source)).into_iter().map(|a|crate::catalog::AddressCheck {key:a.key,source:a.source,url:a.url,file:a.file.unwrap()}).collect()
        }
        fn source_files(db:&Db)->(i64,Vec<crate::catalog::AddressCheck>) {
            let source=db.add_source("folder","Videos","file:///videos","anime").unwrap();
            for name in ["a","b"] {
                db.saw(source,&format!("file:///videos/{name}.mkv"),100,1,Some(&crate::Fingerprint {md5:name.repeat(32),size:100})).unwrap();
            }
            (source,addresses(db,source))
        }
        #[test]
        fn source_counts_and_scan_decisions_follow_only_content_bindings() {
            let db=Db::in_memory();let (source,files)=source_files(&db);
            let a=crate::Anime {dandanplay_id:56,title:"Provider title".into(),image:String::new()};
            let e=crate::Episode {dandanplay_id:5601,number:1,title:"Provider episode".into()};
            db.link_episode(files[0].file,&a,&e,true);
            db.unlink(files[1].file);
            assert_eq!(db.sources()[0].unmatched,2);
            assert!(db.seen(source).iter().all(|s| !s.matched && !s.decided));
            let film=movie(12);
            db.catalog_apply(&[film.clone()],&[],&[binding(files[0].file,&film,0,Decision::Manual)],&[]).unwrap();
            db.catalog_apply(&[],&[],&[FileBinding {file:files[1].file,items:vec![],decision:Decision::Blocked,revision:0,evidence:"user".into()}],&[]).unwrap();
            assert_eq!(db.sources()[0].unmatched,1);
            let seen=db.seen(source);
            assert!(seen.iter().all(|s| s.decided));
            assert_eq!(seen.iter().filter(|s| s.matched).count(),1);
            assert!(seen.iter().find(|s| s.url==files[0].url).unwrap().matched);
        }
        #[test]
        fn selection_requires_every_address_and_keeps_danmaku_and_resume() {
            let db=Db::in_memory();let (_,a)=source_files(&db);let n=movie(80);
            let bindings:Vec<_>=a.iter().map(|a|binding(a.file,&n,0,Decision::Manual)).collect();
            let track=db.catalog_set_danmaku(&dm(a[0].file)).unwrap();db.save_progress(a[0].file,12.,90.);
            let error=db.catalog_apply_selection(&a[..1],&[n.clone()],&bindings).unwrap_err();
            assert_eq!(error.code,"invalid");assert!(db.catalog_node(&n.id).unwrap().is_none());
            assert!(db.catalog_binding(a[0].file).unwrap().is_none());
            let change=db.catalog_apply_selection(&a,&[n.clone()],&bindings).unwrap();assert_eq!(change.after.len(),2);
            assert_eq!(db.catalog_danmaku(a[0].file).unwrap(),Some(track));assert_eq!(db.progress(a[0].file),Some((12.,90.)));
            assert!(db.catalog_apply_selection(&a,&[],&bindings).is_err());
        }
        #[test]
        fn selection_rolls_back_metadata_and_earlier_files_on_revision_conflict() {
            let db=Db::in_memory();let (_,a)=source_files(&db);let n=movie(81);
            let bindings:Vec<_>=a.iter().map(|a|binding(a.file,&n,0,Decision::Manual)).collect();
            db.catalog_apply(&[],&[],&[FileBinding {file:a[1].file,revision:0,decision:Decision::Blocked,items:vec![],evidence:"concurrent".into()}],&[]).unwrap();
            assert_eq!(db.catalog_apply_selection(&a,&[n.clone()],&bindings).unwrap_err().code,"conflict");
            assert!(db.catalog_node(&n.id).unwrap().is_none());assert!(db.catalog_binding(a[0].file).unwrap().is_none());
            assert_eq!(db.catalog_revision(a[0].file).unwrap(),0);
            assert_eq!(db.catalog_binding(a[1].file).unwrap().unwrap().decision,Decision::Blocked);
        }
        #[test]
        fn selection_rejects_replaced_removed_and_reused_addresses() {
            for mutation in ["replace","forget","remove","reuse"] {
                let db=Db::in_memory();let (source,a)=source_files(&db);let n=movie(82);
                let bindings:Vec<_>=a.iter().map(|a|binding(a.file,&n,0,Decision::Manual)).collect();
                match mutation {
                    "replace" => {db.saw(source,&a[1].url,200,2,Some(&crate::Fingerprint {md5:"replaced".into(),size:200})).unwrap();},
                    "forget" => db.forget(source,&[a[1].url.clone()]),
                    "remove" => db.remove_source(source),
                    _ => {
                        db.remove_source(source);
                        let replacement=db.add_source("folder","Other","file:///other","anime").unwrap();
                        // SQLite may reuse both rowids. The URL and physical file
                        // are still part of the identity, never just the rowid.
                        db.saw(replacement,"file:///other/a.mkv",200,2,Some(&crate::Fingerprint {md5:"replaced".into(),size:200})).unwrap();
                    }
                }
                assert_eq!(db.catalog_apply_selection(&a,&[n.clone()],&bindings).unwrap_err().code,"conflict","{mutation}");
                assert!(db.catalog_node(&n.id).unwrap().is_none());
                for a in &a {assert_eq!(db.catalog_revision(a.file).unwrap(),0);assert!(db.catalog_binding(a.file).unwrap().is_none());}
            }
        }
        #[test]
        fn undo_does_not_resurrect_an_old_generation() {
            let db=Db::in_memory();let f=file(&db,"a");let n=movie(1);
            let change=db.catalog_apply(&[n.clone()],&[],&[binding(f,&n,0,Decision::Manual)],&[dm(f)]).unwrap();
            db.catalog_undo(&change.id).unwrap();assert_eq!(db.catalog_revision(f).unwrap(),2);assert_eq!(db.catalog_danmaku_revision(f).unwrap(),2);
            assert!(db.catalog_binding(f).unwrap().is_none());assert!(db.catalog_danmaku(f).unwrap().is_none());
            assert!(db.catalog_set_danmaku(&dm(f)).is_err());assert!(db.catalog_undo(&change.id).is_err());
        }
        #[test]
        fn explicit_unwatched_overrides_completion_but_not_resume() {
            let db=Db::in_memory();let f=file(&db,"a");let n=movie(1);
            db.catalog_apply(&[n.clone()],&[],&[binding(f,&n,0,Decision::Auto)],&[]).unwrap();db.save_progress(f,99.,100.);
            assert_eq!(db.catalog_shelf(None).unwrap()[0].watched,0);
            let mut measured=playback::Intervals::default();measured.add(0.,90.).unwrap();
            db.catalog_record_playback(f,1,&measured,100.,false).unwrap();
            assert_eq!(db.catalog_shelf(None).unwrap()[0].watched,1);
            let mut w=db.catalog_set_watched(&Watched {media:n.id.clone(),watched:false,manual:true,updated_at:1,revision:1}).unwrap();
            assert!(db.catalog_record_playback(f,1,&measured,100.,true).unwrap().is_empty());
            w.manual=false;w.watched=true;assert!(db.catalog_set_watched(&w).is_err());
            assert_eq!(db.catalog_shelf(None).unwrap()[0].watched,0);assert_eq!(db.progress(f),Some((99.,100.)));
        }
        fn intervals(a:f64,b:f64)->playback::Intervals {let mut p=playback::Intervals::default();p.add(a,b).unwrap();p}
        #[test]
        fn measured_playback_accumulates_without_combining_encodes_or_stale_bindings() {
            let db=Db::in_memory();let a=file(&db,"a");let b=file(&db,"b");let n=movie(9);
            db.catalog_apply(&[n.clone()],&[],&[binding(a,&n,0,Decision::Manual),binding(b,&n,0,Decision::Manual)],&[]).unwrap();
            assert!(db.catalog_record_playback(a,1,&intervals(0.,50.),100.,false).unwrap().is_empty());
            assert!(db.catalog_record_playback(b,1,&intervals(50.,100.),100.,true).unwrap().is_empty());
            assert!(db.catalog_watched(&n.id).unwrap().is_none());
            assert_eq!(db.catalog_record_playback(a,1,&intervals(40.,90.),100.,false).unwrap(),[n.id.clone()]);
            assert!(db.progress(a).is_none());
            assert!(db.catalog_record_playback(a,1,&intervals(0.,100.),100.,true).unwrap().is_empty());
            let other=movie(10);
            db.catalog_apply(&[other.clone()],&[],&[binding(a,&other,1,Decision::Manual)],&[]).unwrap();
            assert!(db.catalog_record_playback(a,1,&intervals(0.,100.),100.,true).is_err());
            assert!(db.catalog_record_playback(a,2,&intervals(90.,100.),100.,true).unwrap().is_empty());
            assert!(db.catalog_watched(&other.id).unwrap().is_none());
        }
        #[test]
        fn chapter_completion_uses_intersections_and_untimed_collections_wait_for_eof() {
            for timed in [true,false] {
                let db=Db::in_memory();let f=file(&db,"a");
                let work=tmdb::parse_work(Kind::Tv,1,&json!({"id":1,"name":"Show","seasons":[{"id":10,"season_number":0,"name":"Specials"}]})).unwrap();
                let season=tmdb::parse_season(&work,0,&json!({"id":10,"season_number":0,"name":"Specials","episodes":[{"id":100,"season_number":0,"episode_number":1,"name":"OVA 1"},{"id":101,"season_number":0,"episode_number":2,"name":"OVA 2"}]})).unwrap();
                let mut nodes=vec![work.node,season.node];nodes.extend(season.episodes.clone());
                let b=FileBinding {file:f,decision:Decision::Manual,revision:0,evidence:"test".into(),items:season.episodes.iter().enumerate().map(|(i,n)|BindingItem {media:n.id.clone(),start_seconds:timed.then_some(i as f64*60.),end_seconds:None}).collect()};
                db.catalog_apply(&nodes,&[],&[b],&[]).unwrap();
                assert!(db.catalog_record_playback(f,1,&intervals(0.,55.),0.,false).unwrap().is_empty());
                let first=db.catalog_record_playback(f,1,&intervals(55.,56.),120.,false).unwrap();
                assert_eq!(first.len(),usize::from(timed));
                assert!(db.catalog_watched(&season.episodes[1].id).unwrap().is_none());
                let rest=db.catalog_record_playback(f,1,&intervals(56.,115.),120.,false).unwrap();
                assert_eq!(rest.len(),usize::from(timed));
                let end=db.catalog_record_playback(f,1,&intervals(115.,120.),120.,true).unwrap();
                assert_eq!(end.len(),if timed {0}else{2});
                assert_eq!(db.catalog_shelf(None).unwrap()[0].watched,2);
            }
        }
        #[test]
        fn real_specials_keep_season_zero_and_versions_stay_distinct() {
            let db=Db::in_memory();let a=file(&db,"a");let b=file(&db,"b");
            let work=tmdb::parse_work(Kind::Tv,1,&json!({"id":1,"name":"Show","seasons":[{"id":10,"season_number":0,"name":"Specials"}]})).unwrap();
            let season=tmdb::parse_season(&work,0,&json!({"id":10,"season_number":0,"name":"Specials","episodes":[{"id":100,"season_number":0,"episode_number":1,"name":"OVA"}]})).unwrap();
            let item=season.episodes[0].clone();
            db.catalog_apply(&[work.node.clone(),season.node,item.clone()],&[],&[binding(a,&item,0,Decision::Auto),binding(b,&item,0,Decision::Auto)],&[]).unwrap();
            assert_eq!(db.catalog_node(&item.id).unwrap().unwrap().season_number,Some(0));
            assert_eq!(db.catalog_files(&work.node.id).unwrap().len(),2);assert_eq!(db.catalog_shelf(None).unwrap()[0].episodes,1);
        }
        #[test]
        fn resume_uses_real_chapters_but_keeps_untimed_combined_files() {
            let db=Db::in_memory();let f=file(&db,"a");
            let work=tmdb::parse_work(Kind::Tv,1,&json!({"id":1,"name":"Show","seasons":[{"id":10,"season_number":0,"name":"Specials"}]})).unwrap();
            let season=tmdb::parse_season(&work,0,&json!({"id":10,"season_number":0,"name":"Specials","episodes":[{"id":100,"season_number":0,"episode_number":1,"name":"OVA 1"},{"id":101,"season_number":0,"episode_number":2,"name":"OVA 2"}]})).unwrap();
            let mut nodes=vec![work.node.clone(),season.node];nodes.extend(season.episodes.clone());
            let mut b=FileBinding {file:f,decision:Decision::Manual,revision:0,evidence:"chapters".into(),items:season.episodes.iter().enumerate().map(|(i,n)|BindingItem {media:n.id.clone(),start_seconds:Some(i as f64*60.),end_seconds:None}).collect()};
            b.items[1].end_seconds=Some(120.);
            db.catalog_apply(&nodes,&[],&[b.clone()],&[]).unwrap();
            db.remember("https://example.invalid/a.mkv","combined",None,75.,120.);
            for position in [60.,75.,120.] {
                db.save_progress(f,position,120.);
                let recent=db.catalog_watching(10).unwrap();assert_eq!(recent.len(),1);
                assert_eq!(recent[0].episode.as_ref().unwrap().id,season.episodes[1].id);
                assert_eq!(recent[0].file.position,position);
            }
            b.revision=1;b.items[1].start_seconds=None;
            db.catalog_apply(&[],&[],&[b],&[]).unwrap();
            let recent=db.catalog_watching(10).unwrap();assert_eq!(recent.len(),1);
            assert!(recent[0].episode.is_none());assert_eq!(recent[0].work.id,work.node.id);
            assert_eq!(recent[0].file.position,120.);assert_eq!(db.catalog_shelf(None).unwrap()[0].watched,0);
            assert!(db.catalog_watching(0).unwrap().is_empty());
        }
        #[test]
        fn snapshots_coalesce_the_same_tmdb_reference_and_remap_every_relationship() {
            let source=Db::in_memory();let f=file(&source,"a");
            let original=movie(1);let mut other_id=original.clone();other_id.id=MediaId::new();other_id.root=other_id.id.clone();
            let mut track=dm(f);track.media=Some(other_id.id.clone());
            source.catalog_apply(&[other_id.clone()],&[],&[binding(f,&other_id,0,Decision::Manual)],&[track]).unwrap();
            source.catalog_set_watched(&Watched {media:other_id.id.clone(),watched:false,manual:true,updated_at:9,revision:0}).unwrap();
            let snapshot=source.catalog_export().unwrap();assert_eq!(snapshot.bindings[0].binding.file,0);
            let receiver=Db::in_memory();file(&receiver,"b");receiver.catalog_save_nodes(&[original.clone()]).unwrap();
            receiver.catalog_import(&snapshot,false).unwrap();
            let received=receiver.opened("https://other.invalid/a.mkv",Some(&crate::Fingerprint {md5:"a".repeat(32),size:100})).unwrap();
            assert_ne!(f,received);
            assert!(receiver.catalog_node(&other_id.id).unwrap().is_none());
            assert_eq!(receiver.catalog_binding(received).unwrap().unwrap().items[0].media,original.id);
            assert_eq!(receiver.catalog_danmaku(received).unwrap().unwrap().media,Some(original.id.clone()));
            assert!(!receiver.catalog_watched(&original.id).unwrap().unwrap().watched);
            receiver.catalog_import(&snapshot,false).unwrap();
            let third=Db::in_memory();third.catalog_import(&receiver.catalog_export().unwrap(),false).unwrap();
            assert!(third.catalog_node(&other_id.id).unwrap().is_none());
            assert!(third.catalog_node(&original.id).unwrap().is_some());
            let before=receiver.catalog_binding(received).unwrap();
            let mut conflicting=snapshot;let second=movie(2);conflicting.nodes.push(second.clone());
            conflicting.bindings[0].binding.items[0].media=second.id.clone();
            assert!(receiver.catalog_import(&conflicting,false).is_err());
            assert_eq!(receiver.catalog_binding(received).unwrap(),before);
            assert!(receiver.catalog_node(&second.id).unwrap().is_none());
        }
        #[test]
        fn a_file_has_one_active_recognizer_and_expired_workers_cannot_commit() {
            let db=Db::in_memory();let f=file(&db,"a");
            let first=crate::catalog::CatalogJob {id:MediaId::new().0,file:Some(f),input_revision:0,phase:"recognition".into(),state:"queued".into(),payload:"{}".into(),error:String::new(),next_attempt:0};
            let second=crate::catalog::CatalogJob {id:MediaId::new().0,..first.clone()};
            db.catalog_job_enqueue(&first).unwrap();
            assert!(db.catalog_job_enqueue(&second).is_err());
            db.catalog_job_transition(&first.id,"queued","running","recognition","",0).unwrap();
            assert!(db.catalog_job_enqueue(&second).is_err());
            assert_eq!(db.catalog_file_job(f,crate::now()+301).unwrap().unwrap().state,"stale");
            db.catalog_job_enqueue(&second).unwrap();
            db.catalog_job_transition(&second.id,"queued","running","recognition","",0).unwrap();
            let report=resolve::FileResolution {recognition:None,recognition_error:None,resolution:Err(Error::unavailable())};
            assert!(db.catalog_finish_resolution(&first.id,0,&report).is_err());
            assert_eq!(db.catalog_finish_resolution(&second.id,0,&report).unwrap().state,"retry");
            let third=crate::catalog::CatalogJob {id:MediaId::new().0,..first};
            assert!(db.catalog_job_enqueue(&third).is_err());
            assert!(db.catalog_job_transition(&second.id,"retry","running","recognition","",0).is_err());
        }
        #[test]
        fn recognition_errors_keep_independent_danmaku_evidence() {
            let db=Db::in_memory();let f=file(&db,"a");
            let job=crate::catalog::CatalogJob {id:MediaId::new().0,file:Some(f),input_revision:0,phase:"recognition".into(),state:"queued".into(),payload:"{}".into(),error:String::new(),next_attempt:0};
            db.catalog_job_enqueue(&job).unwrap();db.catalog_job_transition(&job.id,"queued","running","recognition","",0).unwrap();
            assert!(db.catalog_job_transition(&job.id,"queued","running","recognition","",0).is_err());
            let report=resolve::FileResolution {recognition:Some(resolve::Recognition {episode:Some(dm(f).reference),shift_seconds:2.5,..Default::default()}),recognition_error:None,resolution:Err(Error::unavailable())};
            let completed=db.catalog_finish_resolution(&job.id,0,&report).unwrap();
            assert_eq!(completed.state,"retry");assert!(completed.next_attempt>crate::now());
            assert!(db.catalog_binding(f).unwrap().is_none());let d=db.catalog_danmaku(f).unwrap().unwrap();assert_eq!(d.shift_seconds,2.5);assert!(!d.fingerprint_verified);
            assert!(db.catalog_finish_resolution(&job.id,0,&report).is_err());
        }
        fn watch_task(db: &Db) -> crate::catalog::WatchSync {
            let film = movie(88); db.catalog_save_nodes(&[film.clone()]).unwrap();
            let watched = db.catalog_set_watched(&Watched {media:film.id.clone(),watched:true,manual:true,updated_at:1,revision:0}).unwrap();
            let link = db.catalog_link(&ProviderLink {media:film.id.clone(),reference:ProviderRef {provider:Provider::Bangumi,scope:String::new(),kind:Kind::Subject,id:"123".into()},relation:Relation::Exact,coverage:vec![],evidence:"confirmed".into(),confirmed:true,manual:true,revision:0}).unwrap();
            crate::catalog::WatchSync {operation:String::new(),media:film.id,account:"bangumi:1".into(),target:link.reference,watched:true,local_revision:watched.revision,mapping_revision:link.revision,state:"pending".into(),error:String::new(),remote_baseline:None,revision:0,lease_until:0}
        }
        #[test]
        fn watch_delivery_requires_confirmed_mapping_and_preserves_local_unwatched() {
            let db=Db::in_memory(); let mut task=watch_task(&db);
            let mut link=db.catalog_links(&task.media).unwrap().remove(0);
            link.confirmed=false;link=db.catalog_link(&link).unwrap();task.mapping_revision=link.revision;
            assert!(db.catalog_watch_enqueue(&task).is_err());
            link.confirmed=true;link=db.catalog_link(&link).unwrap();task.mapping_revision=link.revision;
            let pending=db.catalog_watch_enqueue(&task).unwrap();
            assert_eq!(db.catalog_watch_enqueue(&task).unwrap(),pending);
            let watched=db.catalog_set_watched(&Watched {media:task.media.clone(),watched:false,manual:true,updated_at:2,revision:task.local_revision}).unwrap();
            assert_eq!(db.catalog_watch_transition(&pending,"sending","",None).unwrap().state,"conflict");
            task.watched=false;task.local_revision=watched.revision;
            assert!(db.catalog_watch_enqueue(&task).is_ok());
            let ddp=db.catalog_link(&ProviderLink {reference:ProviderRef {provider:Provider::Dandanplay,scope:String::new(),kind:Kind::Episode,id:"456".into()},revision:0,..link}).unwrap();
            task.target=ddp.reference;task.mapping_revision=ddp.revision;
            assert!(db.catalog_watch_enqueue(&task).is_err());
            assert!(!db.catalog_watched(&task.media).unwrap().unwrap().watched);
        }
        #[test]
        fn interrupted_watch_delivery_recovers_to_unknown_and_fences_late_replies() {
            let db=Db::in_memory();let task=watch_task(&db);let pending=db.catalog_watch_enqueue(&task).unwrap();
            let sending=db.catalog_watch_transition(&pending,"sending","",None).unwrap();
            assert!(db.catalog_watch_transition(&pending,"sending","",None).is_err());
            assert_eq!(db.catalog_watch_recover(&task.account,sending.lease_until).unwrap(),0);
            assert_eq!(db.catalog_watch_recover(&task.account,sending.lease_until+1).unwrap(),1);
            let unknown=db.catalog_watch_queue(&task.account).unwrap().remove(0);assert_eq!(unknown.state,"unknown");
            assert!(db.catalog_watch_transition(&unknown,"pending","",None).is_err());
            assert!(db.catalog_watch_transition(&unknown,"pending","",Some(true)).is_err());
            assert!(db.catalog_watch_transition(&unknown,"sent","",Some(false)).is_err());
            let retry=db.catalog_watch_transition(&unknown,"pending","",Some(false)).unwrap();
            let current=db.catalog_watch_transition(&retry,"sending","",None).unwrap();
            assert!(current.revision>sending.revision);
            assert!(db.catalog_watch_transition(&sending,"sent","",None).is_err());
            assert!(db.catalog_watch_transition(&unknown,"sent","",Some(true)).is_err());
            let done=db.catalog_watch_transition(&current,"sent","",None).unwrap();assert_eq!(done.state,"sent");
            let copy=Db::in_memory();copy.catalog_import(&db.catalog_export().unwrap(),false).unwrap();
            assert!(copy.catalog_watch_queue(&task.account).unwrap().is_empty());
        }
        #[test]
        fn pending_watch_writes_cannot_overtake_unknown_outcomes_for_the_same_account() {
            let db=Db::in_memory();let task=watch_task(&db);let pending=db.catalog_watch_enqueue(&task).unwrap();
            let first=db.catalog_watch_transition(&pending,"sending","",None).unwrap();
            let mut next=task.clone();
            next.local_revision=db.catalog_set_watched(&Watched {media:task.media.clone(),watched:false,manual:true,updated_at:2,revision:task.local_revision}).unwrap().revision;
            next.watched=false;let next=db.catalog_watch_enqueue(&next).unwrap();
            assert!(db.catalog_watch_transition(&next,"sending","",None).is_err());
            let unknown=db.catalog_watch_transition(&first,"unknown","timeout",None).unwrap();
            assert!(db.catalog_watch_transition(&next,"sending","",None).is_err());
            let mut other=next.clone();other.account="bangumi:2".into();
            let other=db.catalog_watch_enqueue(&other).unwrap();
            assert_eq!(db.catalog_watch_transition(&other,"sending","",None).unwrap().state,"sending");
            db.catalog_watch_transition(&unknown,"sent","",Some(true)).unwrap();
            assert_eq!(db.catalog_watch_transition(&next,"sending","",None).unwrap().state,"sending");
        }
        #[test]
        fn remapping_an_account_target_invalidates_a_queued_write() {
            let db=Db::in_memory();let task=watch_task(&db);let pending=db.catalog_watch_enqueue(&task).unwrap();
            let mut link=db.catalog_links(&task.media).unwrap().remove(0);link.confirmed=false;db.catalog_link(&link).unwrap();
            assert_eq!(db.catalog_watch_transition(&pending,"sending","",None).unwrap().state,"conflict");
        }
        #[test]
        fn file_edits_reject_late_jobs_before_they_touch_danmaku() {
            let db=Db::in_memory();let f=file(&db,"a");let n=movie(1);
            let job=crate::catalog::CatalogJob {id:MediaId::new().0,file:Some(f),input_revision:0,phase:"recognition".into(),state:"queued".into(),payload:"{}".into(),error:String::new(),next_attempt:0};
            db.catalog_job_enqueue(&job).unwrap();db.catalog_job_transition(&job.id,"queued","running","recognition","",0).unwrap();
            db.catalog_apply(&[n.clone()],&[],&[binding(f,&n,0,Decision::Manual)],&[]).unwrap();
            let report=resolve::FileResolution {recognition:Some(resolve::Recognition {episode:Some(dm(f).reference),..Default::default()}),recognition_error:None,resolution:Err(Error::unavailable())};
            assert_eq!(db.catalog_finish_resolution(&job.id,0,&report).unwrap().state,"stale");assert!(db.catalog_danmaku(f).unwrap().is_none());
        }
    }};
}
#[cfg(not(target_arch = "wasm32"))]
contract!(sqlite, crate::Db);
contract!(browser, crate::session::Db);
