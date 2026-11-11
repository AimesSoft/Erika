use super::*;
use crate::LibraryEngine;
use erika_catalog::{DanmakuBinding, Provider, ProviderRef, tmdb};
use erika_library_protocol::library::Commands;
use serde_json::json;

fn work() -> Work {
    tmdb::parse_work(Kind::Tv, 800, &json!({"id":800,"name":"地狱乐","seasons":[
        {"id":810,"season_number":0,"name":"特别篇"}, {"id":820,"season_number":2,"name":"第二季"}
    ]})).unwrap()
}
fn season(work: &Work, number: u32) -> Season {
    tmdb::parse_season(work, number, &json!({"id":if number == 0 {810} else {820},"season_number":number,"name":"季",
        "episodes":(0..=12).map(|n| json!({"id":9000+number*100+n,"season_number":number,"episode_number":n,"name":format!("分集 {n}")})).collect::<Vec<_>>()
    })).unwrap()
}
pub(crate) fn choose(engine: &mut LibraryEngine, number: u32) {
    let s = engine.session.as_mut().unwrap();
    s.generation += 1;
    let work = work();
    let season = season(&work, number);
    s.selected = Some(work.node.clone());
    s.work = Some(work);
    s.selected_season = season.node.id.0.clone();
    s.season = Some(season);
    s.searching = false;
    s.loading = false;
    s.error.clear();
}
fn fixture(names: &[&str]) -> LibraryEngine {
    let mut e = LibraryEngine::default();
    add_videos(&mut e, names);
    e
}
fn add_videos(e: &mut LibraryEngine, names: &[&str]) {
    // These are database fixtures, not paths for the startup scanner to open.
    e.started = true;
    let source =
        e.db.add_source("folder", "fixture", "file:///matching-fixture", "anime")
            .unwrap();
    for (i, name) in names.iter().enumerate() {
        e.db.saw(
            source,
            &format!("file:///matching-fixture/{name}"),
            100,
            1,
            Some(&erika_store::Fingerprint {
                md5: format!("fixture-{i}"),
                size: 100,
            }),
        )
        .unwrap();
    }
    e.load();
    let videos =
        e.db.entries(Some(source))
            .iter()
            .map(|entry| e.video_of(entry.key).unwrap().0)
            .collect();
    e.session = Some(Session::folder(videos, "fixture".into()));
}

fn metadata(path: &str, sole: bool) -> (u16, serde_json::Value) {
    let url = url::Url::parse(&format!("http://fixture{path}")).unwrap();
    let path = url.path().strip_prefix("/metadata/v1/").unwrap();
    let data = match path {
        "search" => {
            let kind = url.query_pairs().find(|(key, _)| key == "kind").unwrap().1;
            json!({"page":1,"total_pages":1,"results":[
                if kind == "tv" { json!({"id":800,"name":"剧集","first_air_date":"2024-01-01"}) }
                else { json!({"id":800,"title":"电影","release_date":"2024-01-01"}) }
            ]})
        }
        "tmdb/movie/800" => json!({"id":800,"title":"电影"}),
        "tmdb/tv/800" => json!({"id":800,"name":"剧集","seasons":if sole {
            vec![json!({"id":810,"season_number":0,"name":"特别篇"})]
        } else {
            vec![json!({"id":810,"season_number":0,"name":"特别篇"}),json!({"id":820,"season_number":2,"name":"第二季"})]
        }}),
        "tmdb/tv/800/seasons/0" => json!({"id":810,"season_number":0,"name":"特别篇","episodes":[
            {"id":9001,"season_number":0,"episode_number":1,"name":"特典一"},
            {"id":9003,"season_number":0,"episode_number":3,"name":"特典三"}
        ]}),
        "tmdb/tv/800/seasons/2" => {
            json!({"id":820,"season_number":2,"name":"第二季","episodes":[]})
        }
        "tmdb/tv/800/images" | "tmdb/movie/800/images" => {
            json!({"id":800,"posters":[],"backdrops":[],"logos":[]})
        }
        _ => return (404, json!({})),
    };
    (200, data)
}

#[test]
fn anonymous_http_picker_uses_real_work_season_and_episode_ids() {
    use erika_library_protocol::library::Engine;
    let (_http, mut e) = crate::tests::engine_with_metadata(|p| metadata(p, false));
    add_videos(&mut e, &["Show S00E01-E03.mkv"]);
    e.match_search("同名作品".into()).unwrap();
    e.wake();
    assert_eq!(e.state.matching.results.len(), 2);
    let results = e.session.as_ref().unwrap().results.clone();
    let tv = results.iter().find(|n| n.kind == Kind::Tv).unwrap();
    let movie = results.iter().find(|n| n.kind == Kind::Movie).unwrap();
    assert_ne!(tv.id, movie.id);
    e.match_work(tv.id.0.clone()).unwrap();
    e.wake();
    assert_eq!(e.state.matching.seasons.len(), 2);
    assert!(e.state.matching.season.is_empty());
    assert!(!e.state.matching.ready);
    let special = e.session.as_ref().unwrap().work.as_ref().unwrap().seasons[0]
        .id
        .0
        .clone();
    e.match_season(special).unwrap();
    e.wake();
    assert!(!e.state.matching.ready);
    assert!(
        matches!(&e.state.matching.rows[0].note, MatchNote::Invalid { reason } if reason.contains("E02"))
    );
    let key = e.state.matching.rows[0].key;
    e.match_number(key, "1,3".into()).unwrap();
    assert!(e.state.matching.ready);
    e.match_confirm().unwrap();
    assert!(!e.state.matching.open);
    let file = e.db.entries(None)[0].file.unwrap();
    let bound = e.db.catalog_binding(file).unwrap().unwrap();
    assert_eq!(bound.items.len(), 2);
    let ids: Vec<_> = bound
        .items
        .iter()
        .map(|i| {
            e.db.catalog_node(&i.media)
                .unwrap()
                .unwrap()
                .reference
                .unwrap()
                .id
        })
        .collect();
    assert_eq!(ids, ["9001", "9003"]);
    assert!(e.db.catalog_danmaku(file).unwrap().is_none());
    e.assign(key).unwrap();
    e.match_search("同名作品".into()).unwrap();
    e.wake();
    e.match_work(movie.id.0.clone()).unwrap();
    e.wake();
    assert!(e.state.matching.movie && e.state.matching.ready);
    assert!(e.state.matching.seasons.is_empty() && e.state.matching.rows[0].assignment.is_empty());
    e.match_confirm().unwrap();
    assert_eq!(
        e.db.catalog_binding(file).unwrap().unwrap().items[0].media,
        movie.id
    );
}

#[test]
fn a_single_actual_season_is_loaded_and_failed_requests_can_be_retried() {
    use erika_library_protocol::library::Engine;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let unavailable = Arc::new(AtomicBool::new(true));
    let flag = unavailable.clone();
    let (_http, mut e) = crate::tests::engine_with_metadata(move |p| {
        if p.contains("/seasons/0") && flag.load(Ordering::SeqCst) {
            (503, json!({}))
        } else {
            metadata(p, true)
        }
    });
    add_videos(&mut e, &["Show S00E01.mkv"]);
    e.match_search("剧集".into()).unwrap();
    e.wake();
    let tv = e
        .session
        .as_ref()
        .unwrap()
        .results
        .iter()
        .find(|n| n.kind == Kind::Tv)
        .unwrap()
        .id
        .0
        .clone();
    e.match_work(tv).unwrap();
    e.wake();
    e.wake();
    assert!(!e.state.matching.loading && !e.state.matching.ready);
    assert!(!e.state.matching.error.is_empty());
    unavailable.store(false, Ordering::SeqCst);
    e.match_season(e.state.matching.season.clone()).unwrap();
    e.wake();
    assert!(e.state.matching.ready);
    assert!(e.state.matching.error.is_empty());
    e.match_confirm().unwrap();
    assert!(!e.state.matching.open);
}

#[test]
fn missing_seasons_holes_and_special_labels_never_invent_identities() {
    let mut e = fixture(&["Show S00E01.mkv", "Show S00E03.mkv", "OVA.mkv"]);
    choose(&mut e, 0);
    let s = e.session.as_mut().unwrap();
    let special = s.videos.iter().find(|v| v.name == "OVA.mkv").unwrap().key;
    assert!(!s.on.contains(&special));
    assert!(s.plan().is_ok());
    s.toggle(special);
    assert!(s.plan().is_err());
    s.type_number(special, "0".into());
    assert!(s.plan().is_ok());
    s.season
        .as_mut()
        .unwrap()
        .episodes
        .retain(|n| n.episode_number != Some(3));
    assert!(s.plan().unwrap_err_message().contains("E03"));
    s.season = None;
    assert!(!s.view().ready);
    assert!(s.view().season.contains('-'));
}
trait PlanError {
    fn unwrap_err_message(self) -> String;
}
impl PlanError for Result<Plan, String> {
    fn unwrap_err_message(self) -> String {
        match self {
            Err(e) => e,
            Ok(_) => panic!("expected invalid plan"),
        }
    }
}

#[test]
fn ranges_and_duplicate_versions_are_explicit_and_preserve_verified_chapters() {
    let mut e = fixture(&["Show S00E01-E03.mkv", "Show S00E01.mkv"]);
    choose(&mut e, 0);
    let s = e.session.as_mut().unwrap();
    let plan = s.plan().unwrap();
    let combined = plan.bindings.iter().find(|b| b.items.len() == 3).unwrap();
    assert!(combined.items.iter().all(|i| i.start_seconds.is_none()));
    assert!(s.view().rows.iter().any(|r| r.note == MatchNote::Twin));
    let video = s
        .videos
        .iter_mut()
        .find(|v| v.file == Some(combined.file))
        .unwrap();
    let mut old = combined.clone();
    for (i, item) in old.items.iter_mut().enumerate() {
        item.start_seconds = Some(i as f64 * 60.);
    }
    video.previous = Some(old.clone());
    assert_eq!(
        s.plan()
            .unwrap()
            .bindings
            .iter()
            .find(|b| b.file == combined.file)
            .unwrap()
            .items,
        old.items
    );
    let key = s
        .videos
        .iter()
        .find(|v| v.file == Some(combined.file))
        .unwrap()
        .key;
    s.type_number(key, "1,3".into());
    assert_eq!(
        s.plan()
            .unwrap()
            .bindings
            .iter()
            .find(|b| b.file == combined.file)
            .unwrap()
            .items
            .len(),
        2
    );
    assert!(
        s.plan()
            .unwrap()
            .bindings
            .iter()
            .find(|b| b.file == combined.file)
            .unwrap()
            .items
            .iter()
            .all(|i| i.start_seconds.is_none())
    );
    for invalid in [
        "1-3,2",
        "3-1",
        "-1",
        "1-999999999",
        "1.5",
        "999999999999999999999999",
    ] {
        s.type_number(key, invalid.into());
        assert!(!s.view().ready, "{invalid}");
    }
}

#[test]
fn movie_versions_have_no_episode_number_and_preserve_danmaku_and_resume() {
    let mut e = fixture(&["film.mkv", "film alternate.mp4"]);
    let movie = tmdb::parse_work(Kind::Movie, 800, &json!({"id":800,"title":"电影"})).unwrap();
    let s = e.session.as_mut().unwrap();
    s.selected = Some(movie.node.clone());
    s.work = Some(movie.clone());
    let f = s.videos[0].file.unwrap();
    let dm =
        e.db.catalog_set_danmaku(&DanmakuBinding {
            file: f,
            reference: ProviderRef {
                provider: Provider::Dandanplay,
                scope: String::new(),
                kind: Kind::Episode,
                id: "123456".into(),
            },
            media: None,
            manual: true,
            fingerprint_verified: false,
            shift_seconds: 0.,
            user_shift_seconds: 2.,
            revision: 0,
        })
        .unwrap();
    e.db.save_progress(f, 45., 100.);
    e.load();
    assert!(e.state.matching.movie && e.state.matching.ready);
    assert!(
        e.state
            .matching
            .rows
            .iter()
            .all(|r| r.assignment.is_empty() && r.destination == "电影")
    );
    e.match_confirm().unwrap();
    assert!(e.session.is_none());
    assert_eq!(
        e.db.catalog_binding(f).unwrap().unwrap().items[0].media,
        movie.node.id
    );
    assert_eq!(e.db.catalog_danmaku(f).unwrap(), Some(dm.clone()));
    assert_eq!(e.db.progress(f), Some((45., 100.)));
    let key =
        e.db.entries(None)
            .iter()
            .find(|entry| entry.file == Some(f))
            .unwrap()
            .key;
    e.unlink(key).unwrap();
    assert_eq!(
        e.db.catalog_binding(f).unwrap().unwrap().decision,
        Decision::Blocked
    );
    assert_eq!(e.db.catalog_danmaku(f).unwrap(), Some(dm));
    assert_eq!(e.db.progress(f), Some((45., 100.)));
}

#[test]
fn one_stale_binding_rolls_back_every_file_and_new_metadata() {
    let mut e = fixture(&["Show S00E01.mkv", "Show S00E02.mkv"]);
    choose(&mut e, 0);
    let plan = e.session.as_ref().unwrap().plan().unwrap();
    let mut concurrent = plan.bindings[1].clone();
    concurrent.decision = Decision::Blocked;
    concurrent.items.clear();
    e.db.catalog_apply(&[], &[], &[concurrent], &[]).unwrap();
    e.match_confirm().unwrap();
    assert!(e.session.is_some());
    assert!(e.state.matching.error.contains("整批均未保存"));
    assert!(
        e.db.catalog_binding(plan.bindings[0].file)
            .unwrap()
            .is_none()
    );
    assert!(e.db.catalog_node(&plan.nodes[0].id).unwrap().is_none());
}

#[test]
fn replacing_a_path_or_removing_a_source_invalidates_the_open_batch() {
    for remove in [false, true] {
        let mut e = fixture(&["Show S00E01.mkv", "Show S00E02.mkv"]);
        choose(&mut e, 0);
        let plan = e.session.as_ref().unwrap().plan().unwrap();
        let address = &plan.addresses[0];
        if remove {
            e.db.remove_source(address.source);
        } else {
            e.db.saw(
                address.source,
                &address.url,
                999,
                2,
                Some(&erika_store::Fingerprint {
                    md5: "replaced".into(),
                    size: 999,
                }),
            )
            .unwrap();
        }
        e.match_confirm().unwrap();
        assert!(e.session.is_some());
        assert!(e.state.matching.error.contains("整批均未保存"));
        assert!(e.db.catalog_node(&plan.nodes[0].id).unwrap().is_none());
    }
}

#[test]
fn same_file_addresses_cannot_commit_different_destinations() {
    let mut e = fixture(&["Show S00E01.mkv"]);
    choose(&mut e, 0);
    let s = e.session.as_mut().unwrap();
    let mut duplicate = s.videos[0].clone();
    duplicate.key += 1000;
    duplicate.address.as_mut().unwrap().key = duplicate.key;
    s.on.insert(duplicate.key);
    s.type_number(s.videos[0].key, "1".into());
    s.set.insert(duplicate.key, "2".into());
    s.videos.push(duplicate);
    assert!(s.plan().unwrap_err_message().contains("同一物理文件"));
}

#[test]
fn stale_replies_cannot_cross_sessions_or_selection_generations() {
    use super::engine::{Loaded, Reply};
    let mut e = fixture(&["Show S00E01.mkv"]);
    let old = e.session.as_ref().unwrap().id.clone();
    e.session.as_mut().unwrap().id = MediaId::new().0;
    e.take_picker(Reply {
        session: old,
        generation: 0,
        result: Ok(Loaded::Search(vec![work().node])),
    });
    assert!(e.session.as_ref().unwrap().results.is_empty());
    choose(&mut e, 0);
    let s = e.session.as_mut().unwrap();
    let id = s.id.clone();
    let old_generation = s.generation;
    s.generation += 2; // A -> B -> A
    let season_id = s.season.as_ref().unwrap().node.id.clone();
    e.take_picker(Reply {
        session: id,
        generation: old_generation,
        result: Ok(Loaded::Season(season(&work(), 2))),
    });
    assert_eq!(
        e.session.as_ref().unwrap().season.as_ref().unwrap().node.id,
        season_id
    );
}

#[test]
fn missing_or_inconsistent_address_snapshots_disable_confirmation() {
    let mut e = fixture(&["Show S00E01.mkv"]);
    choose(&mut e, 0);
    let s = e.session.as_mut().unwrap();
    let address = s.videos[0].address.clone().unwrap();
    s.videos[0].address = None;
    assert!(!s.view().ready);
    s.videos[0].address = Some(erika_store::catalog::AddressCheck {
        file: address.file + 1,
        ..address
    });
    assert!(!s.view().ready);
}

#[test]
fn a_name_from_another_season_requires_an_explicit_override() {
    let mut e = fixture(&["Show S02E01.mkv"]);
    choose(&mut e, 0);
    let s = e.session.as_mut().unwrap();
    assert!(!s.view().ready);
    s.type_number(s.videos[0].key, "1".into());
    assert!(s.view().ready);
    assert!(s.view().rows[0].destination.starts_with("S00E01"));
}
