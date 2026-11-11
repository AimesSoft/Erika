use super::*;
use erika_anime_protocol::anime::Commands;

fn work(id: u64) -> tmdb::Work {
    tmdb::parse_work(
        Kind::Movie,
        id,
        &json!({"id":id,"title":format!("TMDB {id}")}),
    )
    .unwrap()
}

fn source(engine: &mut AnimeEngine, id: i64, work: tmdb::Work, previous: Option<SourceWork>) {
    engine.take_metadata(Finished::Source {
        generation: engine.generation,
        id,
        previous,
        result: Ok(work),
    });
}

fn shared_engines(test: impl FnOnce(&mut AnimeEngine, &mut AnimeEngine)) {
    let root = std::env::temp_dir().join(format!("erika-source-cache-{}", MediaId::new().0));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("db.sqlite");
    let mut first = crate::tests::engine();
    let mut second = crate::tests::engine();
    first.db = erika_store::Db::at(&path).unwrap();
    second.db = erika_store::Db::at(&path).unwrap();
    for engine in [&mut first, &mut second] {
        engine.season = vec![
            serde_json::from_value(json!({"animeId":42,"animeTitle":"来源 A","airDay":1})).unwrap(),
            serde_json::from_value(json!({"animeId":43,"animeTitle":"来源 B","airDay":1})).unwrap(),
        ];
        engine.show_season();
        assert!(engine.metadata.sources.is_empty());
    }
    test(&mut first, &mut second);
    drop((first, second));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn two_engines_merge_only_their_new_result_and_see_each_others_projection() {
    shared_engines(|first, second| {
        let a = work(701);
        let b = work(702);
        source(first, 42, a.clone(), None);
        source(second, 43, b.clone(), None);
        let rows = read_sources(first.db.setting(SOURCE_CACHE).as_deref()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            second.resolved_source(&source_key(42)),
            Some(a.node.id.clone())
        );
        // Opening refreshes from storage, even before the next minute tick.
        first.open_scheduled(source_key(43)).unwrap();
        assert_eq!(first.state.opened, b.node.id.0);
        first.show_season();
        let ids: HashSet<_> = first
            .state
            .week
            .iter()
            .flat_map(|d| d.shows.iter().map(|s| s.id.clone()))
            .collect();
        assert!(ids.contains(&a.node.id.0) && ids.contains(&b.node.id.0));
        let snapshot = first.db.catalog_export().unwrap();
        assert!(
            snapshot.links.is_empty()
                && snapshot.bindings.is_empty()
                && snapshot.watched.is_empty()
        );
        // Reset/import of the current cache is also observable, not restored from RAM.
        second.db.set_setting(SOURCE_CACHE, None);
        first.show_season();
        assert!(first.metadata.sources.is_empty());
    });
}

#[test]
fn a_late_different_identity_cannot_replace_another_engines_result() {
    shared_engines(|first, second| {
        let accepted = work(701);
        let late = work(702);
        source(first, 42, accepted.clone(), None);
        source(second, 42, late.clone(), None);
        assert!(second.state.error.contains("身份已更新"));
        assert_eq!(
            second.resolved_source(&source_key(42)),
            Some(accepted.node.id.clone())
        );
        let before = second.db.setting(SOURCE_CACHE);
        second.state.opened = source_key(42);
        second.take_metadata(Finished::Detail {
            selection: second.metadata.selection,
            opened: second.state.opened.clone(),
            previous: None,
            result: Ok((late.clone(), None)),
        });
        assert!(second.state.details_failed);
        assert_eq!(second.state.opened, source_key(42));
        assert_eq!(second.db.setting(SOURCE_CACHE), before);
        // A new request that actually observed the current mapping may replace it.
        let previous = second.source_snapshot(42);
        second.take_metadata(Finished::Detail {
            selection: second.metadata.selection,
            opened: second.state.opened.clone(),
            previous,
            result: Ok((late.clone(), None)),
        });
        assert!(!second.state.details_failed);
        assert_eq!(second.state.opened, late.node.id.0);
        first.show_season();
        assert_eq!(first.resolved_source(&source_key(42)), Some(late.node.id));
    });
}

#[test]
fn matching_concurrent_results_are_not_reported_as_conflicts() {
    shared_engines(|first, second| {
        let accepted = work(701);
        source(first, 42, accepted.clone(), None);
        source(second, 42, accepted.clone(), None);
        assert!(second.state.error.is_empty(), "{}", second.state.error);
        assert_eq!(second.metadata.sources.len(), 1);
        assert_eq!(
            second.resolved_source(&source_key(42)),
            Some(accepted.node.id)
        );
    });
}

#[test]
fn invalid_cache_is_not_silently_replaced_or_reported_as_saved() {
    let mut engine = crate::tests::engine();
    engine.db.set_setting(SOURCE_CACHE, Some("broken cache"));
    engine.restore_metadata();
    assert_eq!(engine.state.error, "新番身份缓存格式不正确");
    assert!(engine.remember_source(42, MediaId::new(), None).is_err());
    assert_eq!(
        engine.db.setting(SOURCE_CACHE).as_deref(),
        Some("broken cache")
    );
    assert!(engine.metadata.sources.is_empty());
    engine.state.opened = source_key(42);
    engine.season =
        vec![serde_json::from_value(json!({"animeId":42,"animeTitle":"来源"})).unwrap()];
    engine.take_metadata(Finished::Detail {
        selection: engine.metadata.selection,
        opened: engine.state.opened.clone(),
        previous: None,
        result: Ok((work(701), None)),
    });
    assert!(engine.state.details_failed);
    assert_eq!(engine.state.opened, source_key(42));
}

#[test]
fn merging_a_full_cache_keeps_the_new_entry_when_timestamps_tie() {
    let mut engine = crate::tests::engine();
    let media = MediaId::new();
    let rows: Vec<_> = (1..=MAX_SOURCES)
        .map(|id| SourceWork {
            source: source_ref(id as i64).unwrap(),
            media: media.clone(),
            updated_at: erika_store::now(),
        })
        .collect();
    engine
        .db
        .set_setting(SOURCE_CACHE, Some(&serde_json::to_string(&rows).unwrap()));
    engine.remember_source(9999, media.clone(), None).unwrap();
    assert_eq!(engine.metadata.sources.len(), MAX_SOURCES);
    assert_eq!(engine.source_snapshot(9999).unwrap().media, media);
}

#[test]
fn metadata_completions_do_not_retain_destroyed_engines() {
    let inbox = Arc::new(Mutex::new(Vec::new()));
    let weak = Arc::downgrade(&inbox);
    let deliver = deliver_metadata(&inbox);
    assert_eq!(Arc::strong_count(&inbox), 1);
    drop(inbox);
    deliver(Finished::Configuration(Err(
        erika_catalog::Error::unavailable(),
    )));
    assert!(weak.upgrade().is_none());
}
