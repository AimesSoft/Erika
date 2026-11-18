use crate::{LibraryEngine, LibraryFilter, LibraryListing, LibrarySort};
use erika_catalog::{BindingItem, Decision, FileBinding, Kind, MediaId, MediaNode, Watched, tmdb};
use erika_library_protocol::library::Commands;
use erika_store::{Db, Fingerprint};
use serde_json::json;

fn list(engine: &LibraryEngine, source: i64) -> LibraryListing {
    engine
        .listing(source, LibraryFilter::All, LibrarySort::Recent)
        .unwrap()
}

fn source(db: &Db, dir: &str) -> i64 {
    db.add_source("folder", "视频", &format!("file:///{dir}"), "anime")
        .unwrap()
}

fn fingerprint(hex: &str) -> Fingerprint {
    Fingerprint {
        md5: hex.repeat(32),
        size: 100,
    }
}

fn add_file(db: &Db, source: i64, url: &str, hex: &str) -> i64 {
    db.saw(source, url, 100, 1, Some(&fingerprint(hex)))
        .unwrap()
}

/// Replace the physical file at an address with a fixed seen time. The test
/// must not accidentally invalidate the whole wall by crossing a wall-clock second.
#[cfg(not(target_arch = "wasm32"))]
fn place(db: &Db, source: i64, url: &str, hex: &str, at: i64) -> i64 {
    let owner = MediaId::new().0;
    let generation = db.catalog_scan_begin(source, &owner, at).unwrap();
    let fp = fingerprint(hex);
    let file = db
        .catalog_scan_read(source, &owner, generation, url, 100, 1, Some(&fp), at)
        .unwrap()
        .unwrap();
    db.catalog_scan_finish(source, &owner, generation, false, at)
        .unwrap();
    file
}

fn bind(db: &Db, file: i64, node: &MediaNode) {
    db.catalog_apply(
        &[],
        &[],
        &[FileBinding {
            file,
            decision: Decision::Manual,
            revision: 0,
            evidence: "fixture".into(),
            items: vec![BindingItem {
                media: node.id.clone(),
                start_seconds: None,
                end_seconds: None,
            }],
        }],
        &[],
    )
    .unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn swapping_two_sources_files_changes_browse_but_not_the_global_wall_or_source_counts() {
    for merged in ["false", "true"] {
        let mut engine = LibraryEngine::with_db(Db::in_memory());
        let (a, b) = (source(&engine.db, "a"), source(&engine.db, "b"));
        engine.db.set_setting("merge_seasons", Some(merged));
        let left = "file:///a/video.mkv";
        let right = "file:///b/video.mkv";
        let first = place(&engine.db, a, left, "a", 10);
        let second = place(&engine.db, b, right, "b", 10);
        for (id, title, file) in [(1, "甲", first), (2, "乙", second)] {
            let node = tmdb::parse_work(Kind::Movie, id, &json!({"id":id,"title":title}))
                .unwrap()
                .node;
            engine
                .db
                .catalog_save_nodes(std::slice::from_ref(&node))
                .unwrap();
            bind(&engine.db, file, &node);
        }
        engine.history_changed().unwrap();
        assert_eq!(list(&engine, a).posters[0].title, "甲");
        assert_eq!(list(&engine, b).posters[0].title, "乙");
        let revision = engine.state.browse_revision;
        let whole = engine.state.posters.clone();
        let sources = engine.state.sources.clone();

        assert_eq!(place(&engine.db, a, left, "b", 20), second);
        assert_eq!(place(&engine.db, b, right, "a", 20), first);
        engine.history_changed().unwrap();
        assert_eq!(engine.state.posters, whole);
        assert_eq!(engine.state.sources, sources);
        assert!(engine.state.browse_revision > revision);
        assert_eq!(list(&engine, a).posters[0].title, "乙");
        assert_eq!(list(&engine, b).posters[0].title, "甲");

        let revision = engine.state.browse_revision;
        engine.db.save_progress(first, 12., 90.);
        engine.history_changed().unwrap();
        assert_eq!(engine.state.browse_revision, revision);
        engine
            .db
            .catalog_set_watched(&Watched {
                media: MediaId(list(&engine, b).posters[0].id.clone()),
                watched: true,
                manual: true,
                updated_at: 30,
                revision: 0,
            })
            .unwrap();
        engine.history_changed().unwrap();
        assert!(engine.state.browse_revision > revision);
        assert_eq!(list(&engine, b).finished, 1);
        assert_eq!(list(&engine, a).unwatched, 1);
    }
}

#[test]
fn changing_sources_invalidates_browse_but_folder_navigation_and_noop_loads_do_not() {
    let mut engine = LibraryEngine::with_db(Db::in_memory());
    let empty = engine.state.browse_revision;
    let a = source(&engine.db, "a");
    add_file(&engine.db, a, "file:///a/season/a.mkv", "a");
    engine.history_changed().unwrap();
    let revision = engine.state.browse_revision;
    assert!(revision > empty);
    assert_eq!(list(&engine, a).source_name, "视频");
    engine.open_source(a).unwrap();
    engine.open_dir("season".into()).unwrap();
    assert_eq!(engine.state.folder.title, "season");
    engine.go_up().unwrap();
    engine.go_to(0).unwrap();
    engine.close_source().unwrap();
    engine.history_changed().unwrap();
    assert_eq!(engine.state.browse_revision, revision);

    engine.db.set_source(a, "归档", "file:///a");
    engine.history_changed().unwrap();
    assert!(engine.state.browse_revision > revision);
    let listing = list(&engine, a);
    assert!(listing.empty);
    assert_eq!(listing.source_name, "归档");
    let revision = engine.state.browse_revision;
    engine.db.remove_source(a);
    engine.history_changed().unwrap();
    assert!(engine.state.browse_revision > revision);
    assert!(list(&engine, a).source_name.is_empty());
}

#[test]
fn repeated_batch_errors_recheck_live_scopes_but_leave_source_navigation_available() {
    let mut engine = LibraryEngine::with_db(Db::in_memory());
    let a = source(&engine.db, "a");
    let b = source(&engine.db, "b");
    let file = add_file(&engine.db, a, "file:///a/movie.mkv", "a");
    let movie = tmdb::parse_work(Kind::Movie, 3, &json!({"id": 3, "title": "影片"}))
        .unwrap()
        .node;
    engine
        .db
        .catalog_save_nodes(std::slice::from_ref(&movie))
        .unwrap();
    bind(&engine.db, file, &movie);
    engine.history_changed().unwrap();
    let before = engine.state.browse_revision;

    // Feed the batch read boundary an error. The scope reads below still use
    // the real store, just as they do if only a different source is unreadable.
    let failed_batch = |engine: &mut LibraryEngine| {
        let sources = engine.load_sources();
        engine.update_walls(
            &sources,
            Err(erika_store::catalog::Error::invalid("unreadable shelf")),
        );
    };
    failed_batch(&mut engine);
    assert!(engine.state.browse_revision > before);
    assert!(matches!(
        &engine.state.notice,
        Some(crate::LibraryNotice::Failed { .. })
    ));
    assert_eq!(list(&engine, a).unwatched, 1);
    let failed = engine.state.browse_revision;
    engine.open_source(b).unwrap();
    engine
        .busy
        .insert(b, crate::Busy::Scanning { done: 1, total: 3 });
    engine.load_status();
    assert_eq!(engine.state.folder.source, b);
    assert_eq!(engine.state.browse_revision, failed);
    engine.close_source().unwrap();

    engine
        .db
        .catalog_set_watched(&Watched {
            media: MediaId(list(&engine, a).posters[0].id.clone()),
            watched: true,
            manual: true,
            updated_at: 20,
            revision: 0,
        })
        .unwrap();
    failed_batch(&mut engine);
    assert!(engine.state.browse_revision > failed);
    assert_eq!(list(&engine, a).finished, 1);
    assert_eq!(list(&engine, 0).finished, 1); // not the old successful wall
    let second_failure = engine.state.browse_revision;
    engine.history_changed().unwrap();
    let recovered = engine.state.browse_revision;
    assert!(recovered > second_failure);
    engine.history_changed().unwrap();
    assert_eq!(engine.state.browse_revision, recovered);
}

#[test]
fn a_specials_only_sources_root_artwork_can_change_with_no_change_to_global_posters() {
    let mut engine = LibraryEngine::with_db(Db::in_memory());
    let regular = source(&engine.db, "series");
    let specials = source(&engine.db, "specials");
    let mut work = tmdb::parse_work(
        Kind::Tv,
        1,
        &json!({
            "id":1,"name":"Series","poster_path":"/work.jpg","seasons":[
                {"id":10,"season_number":0,"name":"Specials"},
                {"id":11,"season_number":1,"name":"Season 1"}
            ]
        }),
    )
    .unwrap();
    engine
        .db
        .catalog_save_nodes(std::slice::from_ref(&work.node))
        .unwrap();
    for (number, source, dir, fp) in [(0, specials, "specials", "a"), (1, regular, "series", "b")] {
        let season = tmdb::parse_season(&work, number, &json!({
            "id":10+number,"season_number":number,"name":"Season","poster_path":"/season.jpg",
            "episodes":[{"id":100+number,"season_number":number,"episode_number":1,"name":"Episode"}]
        })).unwrap();
        let item = &season.episodes[0];
        engine
            .db
            .catalog_save_nodes(&[season.node, item.clone()])
            .unwrap();
        let file = add_file(&engine.db, source, &format!("file:///{dir}/a.mkv"), fp);
        bind(&engine.db, file, item);
    }
    engine.history_changed().unwrap();
    let whole = engine.state.posters.clone();
    assert_eq!(whole.len(), 1);
    assert_eq!(whole[0].episodes, 2);
    assert!(whole[0].poster.url.ends_with("/season.jpg"));
    let extra = list(&engine, specials);
    assert_eq!(extra.posters[0].id, work.node.id.0);
    assert!(extra.posters[0].poster.url.ends_with("/work.jpg"));
    let revision = engine.state.browse_revision;
    work.node.artwork.poster.as_mut().unwrap().file_path = "/replacement.jpg".into();
    engine.db.catalog_save_nodes(&[work.node]).unwrap();
    engine.history_changed().unwrap();
    assert!(engine.state.browse_revision > revision);
    assert_eq!(engine.state.posters, whole);
    assert!(
        list(&engine, specials).posters[0]
            .poster
            .url
            .ends_with("/replacement.jpg")
    );
}
