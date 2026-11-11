use super::*;
use anime::Commands;
use erika_catalog::{BindingItem, Decision, FileBinding, Kind, MediaId, Watched, tmdb};
use erika_store::catalog::MediaMark;
use serde_json::json;

pub(super) fn engine() -> AnimeEngine {
    let mut engine = AnimeEngine::new(
        erika_store::Db::in_memory(),
        catalog::Accounts::default(),
        SeasonSource::Dandanplay,
        Vec::new(),
        Schedule::default(),
        catalog::Links::default(),
    );
    // Page tests never contact the production metadata service.
    engine.metadata.network_disabled = true;
    engine.metadata.client =
        Some(erika_catalog::Client::new("http://127.0.0.1:1/", "zh-CN").unwrap());
    engine.show_season();
    engine
}

fn intro(id: i64, title: &str) -> Intro {
    serde_json::from_value(json!({"animeId":id,"animeTitle":title,"imageUrl":"https://old.invalid/cover.jpg","airDay":3,"rating":8.5})).unwrap()
}

pub(super) fn series(engine: &AnimeEngine, count: u32) -> (tmdb::Work, tmdb::Season, tmdb::Season) {
    let work = tmdb::parse_work(Kind::Tv, 42, &json!({
        "id":42,"name":"旅途","overview":"TMDB 简介","first_air_date":"2023-09-29",
        "poster_path":"/poster.jpg","backdrop_path":"/backdrop.jpg",
        "seasons":[{"id":100,"season_number":0,"name":"特别篇"},{"id":101,"season_number":1,"name":"第一季"}]
    })).unwrap();
    let special = tmdb::parse_season(
        &work,
        0,
        &json!({
            "id":100,"name":"特别篇","season_number":0,
            "episodes":[{"id":1000,"name":"旅途之前","season_number":0,"episode_number":1}]
        }),
    )
    .unwrap();
    let regular = tmdb::parse_season(&work, 1, &json!({
        "id":101,"name":"第一季","season_number":1,
        "episodes":(1..=count).map(|n|json!({"id":2000+n,"name":format!("旅途 {n}"),"season_number":1,"episode_number":n})).collect::<Vec<_>>()
    })).unwrap();
    let mut nodes = vec![
        work.node.clone(),
        special.node.clone(),
        regular.node.clone(),
    ];
    nodes.extend(special.episodes.clone());
    nodes.extend(regular.episodes.clone());
    engine.db.catalog_save_nodes(&nodes).unwrap();
    (work, special, regular)
}

fn bind(
    engine: &AnimeEngine,
    url: &str,
    items: Vec<BindingItem>,
    position: f64,
    duration: f64,
) -> i64 {
    let file = engine
        .db
        .opened(
            url,
            Some(&erika_store::Fingerprint {
                md5: url.into(),
                size: 1,
            }),
        )
        .unwrap();
    engine
        .db
        .catalog_apply(
            &[],
            &[],
            &[FileBinding {
                file,
                decision: Decision::Manual,
                items,
                evidence: "test".into(),
                revision: 0,
            }],
            &[],
        )
        .unwrap();
    engine.db.save_progress(file, position, duration);
    engine.db.remember(url, "文件名", None, position, duration);
    file
}

fn item(media: &MediaId, start: Option<f64>, end: Option<f64>) -> BindingItem {
    BindingItem {
        media: media.clone(),
        start_seconds: start,
        end_seconds: end,
    }
}

fn watched(engine: &AnimeEngine, id: &MediaId, value: bool) {
    let previous = engine.db.catalog_watched(id).unwrap();
    engine
        .db
        .catalog_set_watched(&Watched {
            media: id.clone(),
            watched: value,
            manual: true,
            updated_at: erika_store::now(),
            revision: previous.map_or(0, |v| v.revision),
        })
        .unwrap();
}

#[test]
fn public_calendar_opens_without_an_account_or_provider_artwork() {
    let mut engine = engine();
    assert_eq!(engine.open("99".into()).unwrap_err().code, "unknown_show");
    assert_eq!(engine.select_day(7).unwrap_err().code, "unknown_day");
    engine.season = vec![intro(17675, "旅途")];
    engine.show_season();
    let source = metadata::source_key(17675);
    engine.open_scheduled(source.clone()).unwrap();
    assert_eq!(engine.state.opened, source);
    assert!(engine.db.catalog_export().unwrap().nodes.is_empty());
    assert_eq!(engine.state.back, BackTo::Schedule);
    assert_eq!(engine.state.show.title, "旅途");
    assert!(engine.state.show.poster.url.is_empty());
    assert!(engine.state.show.summary.is_empty());
    assert_eq!(engine.state.show.rating, 0.0);
    assert!(!engine.state.show.has_premiere);
    assert!(engine.state.episodes.is_empty());
    engine.close().unwrap();
    assert!(engine.state.opened.is_empty());
}

#[test]
fn files_progress_and_watched_state_use_canonical_episodes() {
    let mut engine = engine();
    let (work, _, regular) = series(&engine, 12);
    for (index, position) in [1440., 1440., 600.].into_iter().enumerate() {
        let episode = &regular.episodes[index];
        bind(
            &engine,
            &format!("file:///v/{index}.mkv"),
            vec![item(&episode.id, None, None)],
            position,
            1440.,
        );
        if index < 2 {
            watched(&engine, &episode.id, true);
        }
    }
    engine.history_changed().unwrap();
    assert!(engine.state.has_stage);
    assert_eq!(engine.state.stage.id, work.node.id.0);
    assert_eq!(engine.state.stage.episode, 3);
    assert_eq!(engine.state.stage.chapter, "S01E03");
    assert_eq!(engine.state.stage.left, 14);
    engine.open(work.node.id.0.clone()).unwrap();
    assert_eq!(engine.state.show.watched, 2);
    assert_eq!(engine.state.next_episode, regular.episodes[2].id.0);
    assert_eq!(engine.state.next, NextAction::Resume);
    assert_eq!(
        engine.state.episodes[2].note,
        anime::EpisodeNote::Left { minutes: 14 }
    );
    assert_eq!(
        engine.state.episodes[0].note,
        anime::EpisodeNote::Length { minutes: 24 }
    );
    assert_eq!(engine.state.episodes[5].note, anime::EpisodeNote::Missing);
    assert_eq!(engine.state.first_episode, regular.episodes[0].id.0);
    assert_eq!(engine.state.last_episode, regular.episodes[11].id.0);
    assert_eq!(engine.state.show.summary, "TMDB 简介");
    assert_eq!(
        engine.state.show.premiere,
        bern::date::CalendarDate::new(2023, 9, 29)
    );
    assert!(engine.state.show.has_premiere);
    assert!(
        engine
            .state
            .show
            .poster
            .url
            .starts_with("https://image.tmdb.org/")
    );
    assert_eq!(
        engine.resume(MediaId::new().0).unwrap_err().code,
        "unknown_show"
    );
}

#[test]
fn s00_and_s01_same_numbers_never_select_or_mark_each_other() {
    let mut engine = engine();
    let (work, special, regular) = series(&engine, 2);
    bind(
        &engine,
        "file:///v/special.mkv",
        vec![item(&special.episodes[0].id, None, None)],
        0.,
        0.,
    );
    engine.open(work.node.id.0).unwrap();
    assert_eq!(engine.state.selected_season, regular.node.id.0);
    assert_eq!(engine.state.episodes[0].number, 1);
    assert_eq!(
        engine
            .play_episode(special.episodes[0].id.0.clone())
            .unwrap_err()
            .code,
        "unknown_episode"
    );
    engine
        .mark_episode(regular.episodes[0].id.0.clone(), true)
        .unwrap();
    engine.select_season(special.node.id.0.clone()).unwrap();
    assert_eq!(engine.state.episodes.len(), 1);
    assert_eq!(engine.state.episodes[0].numeral, "S00E01");
    assert!(!engine.state.episodes[0].watched);
    assert_eq!(engine.state.first_episode, special.episodes[0].id.0);
    assert_eq!(engine.state.last_episode, special.episodes[0].id.0);
    engine
        .play_episode(special.episodes[0].id.0.clone())
        .unwrap();
    assert_eq!(
        engine
            .mark_episode(regular.episodes[0].id.0.clone(), false)
            .unwrap_err()
            .code,
        "unknown_episode"
    );
    assert_eq!(
        engine.select_season(MediaId::new().0).unwrap_err().code,
        "unknown_season"
    );
}

#[test]
fn movies_have_a_playable_identity_without_a_fabricated_episode() {
    let mut engine = engine();
    let movie = tmdb::parse_work(
        Kind::Movie,
        42,
        &json!({"id":42,"title":"电影","overview":"电影简介"}),
    )
    .unwrap();
    engine.db.catalog_save_nodes(&[movie.node.clone()]).unwrap();
    bind(
        &engine,
        "file:///v/movie.mkv",
        vec![item(&movie.node.id, None, None)],
        120.,
        7200.,
    );
    engine.history_changed().unwrap();
    assert_eq!(engine.state.stage.id, movie.node.id.0);
    assert!(engine.state.stage.chapter.is_empty());
    engine.open(movie.node.id.0.clone()).unwrap();
    assert!(engine.state.show.movie);
    assert!(!engine.state.show.has_premiere);
    assert!(engine.state.seasons.is_empty());
    assert_eq!(engine.state.episodes.len(), 1);
    assert_eq!(engine.state.episodes[0].id, movie.node.id.0);
    assert!(engine.state.episodes[0].numeral.is_empty());
    engine.play_episode(movie.node.id.0.clone()).unwrap();
    engine.mark_episode(movie.node.id.0, true).unwrap();
    assert!(engine.state.episodes[0].watched);
}

#[test]
fn a_collection_without_complete_chapters_is_available_as_file_resume() {
    let mut engine = engine();
    let (work, _, regular) = series(&engine, 2);
    bind(
        &engine,
        "file:///v/collection.mkv",
        vec![
            item(&regular.episodes[0].id, Some(0.), None),
            item(&regular.episodes[1].id, None, None),
        ],
        90.,
        200.,
    );
    engine.history_changed().unwrap();
    assert!(engine.state.stage.chapter.is_empty());
    assert_eq!(engine.state.stage.progress, 0.45);
    engine.open(work.node.id.0).unwrap();
    for cell in &engine.state.episodes {
        assert_eq!(cell.note, anime::EpisodeNote::Collection);
        assert_eq!(cell.progress, 0.);
        assert!(!cell.watched);
        let file = content::file(&engine.db, &MediaId(cell.id.clone()))
            .unwrap()
            .unwrap();
        assert!(file.span.is_none());
        assert_eq!(file.position(), 90.);
    }
    engine
        .play_episode(regular.episodes[1].id.0.clone())
        .unwrap();
}

#[test]
fn complete_chapters_and_versions_keep_absolute_file_timestamps() {
    let engine = engine();
    let (_, _, regular) = series(&engine, 2);
    bind(
        &engine,
        "file:///v/precise.mkv",
        vec![
            item(&regular.episodes[0].id, Some(0.), Some(50.)),
            item(&regular.episodes[1].id, Some(60.), Some(120.)),
        ],
        40.,
        120.,
    );
    // A more recently played but unaddressable collection must not displace
    // the version in which this episode has an exact chapter.
    bind(
        &engine,
        "file:///v/unknown.mkv",
        vec![
            item(&regular.episodes[0].id, None, None),
            item(&regular.episodes[1].id, None, None),
        ],
        100.,
        200.,
    );
    let first = content::file(&engine.db, &regular.episodes[0].id)
        .unwrap()
        .unwrap();
    let second = content::file(&engine.db, &regular.episodes[1].id)
        .unwrap()
        .unwrap();
    assert_eq!(first.file.url, "file:///v/precise.mkv");
    assert_eq!(first.position(), 40.);
    assert_eq!(second.position(), 60.);
    engine.db.save_progress(second.file.file, 80., 0.);
    assert_eq!(
        content::file(&engine.db, &regular.episodes[1].id)
            .unwrap()
            .unwrap()
            .position(),
        80.
    );
    engine.db.save_progress(second.file.file, 120., 120.);
    assert_eq!(
        content::file(&engine.db, &regular.episodes[1].id)
            .unwrap()
            .unwrap()
            .position(),
        60.
    );
}

#[test]
fn explicitly_unwatched_is_not_inferred_back_from_a_finished_file() {
    let mut engine = engine();
    let (work, _, regular) = series(&engine, 2);
    bind(
        &engine,
        "file:///v/finished.mkv",
        vec![item(&regular.episodes[0].id, None, None)],
        1440.,
        1440.,
    );
    watched(&engine, &regular.episodes[0].id, true);
    engine.open(work.node.id.0).unwrap();
    engine
        .mark_episode(regular.episodes[0].id.0.clone(), false)
        .unwrap();
    engine.history_changed().unwrap();
    assert!(!engine.state.episodes[0].watched);
    assert_eq!(engine.state.show.watched, 0);
    assert!(
        !engine
            .db
            .catalog_watched(&regular.episodes[0].id)
            .unwrap()
            .unwrap()
            .watched
    );
    assert_eq!(
        engine
            .mark_episode(MediaId::new().0, true)
            .unwrap_err()
            .code,
        "unknown_episode"
    );
    assert!(!engine.state.episode_marking);
    engine.set_episode_marking(true).unwrap();
    assert!(engine.state.episode_marking);
}

#[test]
fn search_results_are_kept_in_the_engine_database_and_do_not_replace_details() {
    let mut engine = engine();
    let (work, _, _) = series(&engine, 2);
    let mut summary = work.node.clone();
    summary.overview = "搜索摘要".into();
    summary.artwork = Default::default();
    let movie = tmdb::parse_work(
        Kind::Movie,
        42,
        &json!({"id":42,"title":"搜索到的电影","overview":"电影简介"}),
    )
    .unwrap();
    let results = engine
        .keep_search_results(vec![summary.clone(), summary, movie.node.clone()])
        .unwrap();
    assert_eq!(results.shows.len(), 2);
    assert_eq!(results.shows[0].summary, "TMDB 简介");
    assert!(!results.shows[0].poster.url.is_empty());
    assert_ne!(results.shows[0].id, results.shows[1].id);
    engine.open_from_search(movie.node.id.0).unwrap();
    assert_eq!(engine.state.show.title, "搜索到的电影");
    assert_eq!(engine.state.show.summary, "电影简介");
    assert_eq!(engine.state.back, BackTo::Search);
}

#[test]
fn marks_written_by_another_engine_show_after_history_changed() {
    let mut engine = engine();
    let (work, _, regular) = series(&engine, 2);
    engine.open(work.node.id.0.clone()).unwrap();
    assert_eq!(engine.state.show.status, WatchStatus::Unmarked);
    engine
        .db
        .catalog_set_mark(&MediaMark {
            media: regular.node.id.clone(),
            reference: None,
            status: Some("watched".into()),
            rating: Some(7),
            review: None,
            revision: 0,
            updated_at: erika_store::now(),
        })
        .unwrap();
    engine.history_changed().unwrap();
    assert_eq!(engine.state.show.status, WatchStatus::Watched);
    assert_eq!(engine.state.my_score, Score::Seven);
    // An explicit lower status is intentional, not an invitation to union it
    // back with a stale account response or a file that once reached EOF.
    engine
        .db
        .catalog_set_mark(&MediaMark {
            media: regular.node.id,
            reference: None,
            status: Some("want".into()),
            rating: Some(7),
            review: None,
            revision: 1,
            updated_at: erika_store::now() + 1,
        })
        .unwrap();
    engine.history_changed().unwrap();
    assert_eq!(engine.state.show.status, WatchStatus::Want);
}

#[test]
fn late_source_and_detail_responses_do_not_replace_the_selected_account() {
    let mut engine = engine();
    engine.generation = 2;
    engine.season = vec![intro(2, "当前来源")];
    engine.state.loading = true;
    engine
        .inbox
        .lock()
        .unwrap()
        .push(Done::Season(1, Ok(vec![intro(1, "旧来源")])));
    engine.take_done();
    assert_eq!(engine.season[0].anime_title, "当前来源");
    assert!(engine.state.loading);
}

#[test]
fn calendars_and_broadcasts_use_the_store_without_filesystem_caches() {
    let mut engine = engine();
    let rows = vec![intro(7, "公共新番")];
    keep_season(&engine.db, &SeasonSource::Dandanplay, &rows);
    assert_eq!(
        cached_season(&engine.db, &SeasonSource::Dandanplay)[0].anime_title,
        "公共新番"
    );
    assert!(cached_season(&engine.db, &SeasonSource::Bangumi).is_empty());
    let slots = vec![schedule::Slot {
        titles: vec!["公共新番".into()],
        first: 42,
        period: 604800,
    }];
    engine.schedule_loading = true;
    engine
        .inbox
        .lock()
        .unwrap()
        .push(Done::Schedule(Ok((100, slots.clone()))));
    engine.take_done();
    assert!(!engine.schedule_loading);
    let cached = Schedule::cached(&engine.db).unwrap();
    assert_eq!(cached.fetched, 100);
    assert_eq!(cached.find("公共新番", None), slots.first());
    engine.schedule_loading = true;
    engine
        .inbox
        .lock()
        .unwrap()
        .push(Done::Schedule(Err("offline".into())));
    engine.take_done();
    assert!(!engine.schedule_loading);
    assert_eq!(engine.schedule.fetched, 100);
    assert_eq!(Schedule::cached(&engine.db).unwrap().fetched, 100);
    assert!(schedule::parse("{\"error\":\"offline\"}", 0).is_err());
    assert!(schedule::parse("invalid", 0).is_err());
    assert_eq!(schedule::parse("{\"items\":[]}", 0).unwrap(), vec![]);
}

#[test]
fn completion_does_not_keep_a_dropped_engine_alive() {
    let inbox = Arc::new(Mutex::new(Vec::new()));
    let weak = Arc::downgrade(&inbox);
    let deliver = receiver(&inbox);
    assert_eq!(Arc::strong_count(&inbox), 1);
    drop(inbox);
    deliver(Done::Schedule(Err("offline".into())));
    assert!(weak.upgrade().is_none());
}

#[test]
fn the_clock_moves_once_the_minute_turns() {
    let mut engine = engine();
    engine.state.now = -1;
    engine.minute -= 1;
    engine.tick().unwrap();
    assert!((0..86_400).contains(&engine.state.now));
}

#[test]
fn a_later_season_does_not_use_dandanplay_absolute_numbers_as_keys() {
    let mut engine = engine();
    let (work, _, regular) = series(&engine, 2);
    bind(
        &engine,
        "file:///v/s1e1.mkv",
        vec![item(&regular.episodes[0].id, None, None)],
        0.,
        0.,
    );
    engine.open(work.node.id.0).unwrap();
    assert_eq!(engine.state.episodes[0].id, regular.episodes[0].id.0);
    assert_eq!(engine.state.episodes[0].note, anime::EpisodeNote::Unwatched);
    assert_eq!(
        engine.play_episode("29".into()).unwrap_err().code,
        "unknown_episode"
    );
    engine
        .play_episode(regular.episodes[0].id.0.clone())
        .unwrap();
}
