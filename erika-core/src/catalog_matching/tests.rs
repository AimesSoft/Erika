use super::*;
use erika_catalog::{DanmakuBinding, Provider, ProviderRef, tmdb};
use serde_json::json;

fn engine() -> ErikaEngine {
    let mut e = ErikaEngine::default();
    let file =
        e.db.opened(
            "https://example.invalid/test.mkv",
            Some(&erika_store::Fingerprint {
                md5: "01234567890123456789012345678901".into(),
                size: 4096,
            }),
        )
        .unwrap();
    e.file = Some(file);
    e.state.playing = true;
    e.state.features.danmaku = false;
    e.state.title = "test.mkv".into();
    e.state.subtitle = PlayingNote::Link;
    e.capture_match_revisions().unwrap();
    e
}
fn movie(id: u64) -> Work {
    tmdb::parse_work(
        Kind::Movie,
        id,
        &json!({"id":id,"title":"电影","overview":"来自 TMDB 的简介"}),
    )
    .unwrap()
}
fn series() -> (Work, Season) {
    let work = tmdb::parse_work(Kind::Tv, 100, &json!({"id":100,"name":"剧集","seasons":[
        {"id":200,"season_number":0,"name":"特别篇"},{"id":201,"season_number":1,"name":"第一季"}
    ]})).unwrap();
    let season = tmdb::parse_season(
        &work,
        0,
        &json!({"id":200,"name":"特别篇","season_number":0,"episodes":[
            {"id":300,"name":"特别篇一","season_number":0,"episode_number":1,"show_id":100}
        ]}),
    )
    .unwrap();
    (work, season)
}
fn bind_movie(e: &mut ErikaEngine) -> Work {
    let work = movie(100);
    e.catalog_picker.work = Some(work.clone());
    e.confirm_catalog_item(work.node.id.0.clone()).unwrap();
    work
}
fn danmaku(e: &ErikaEngine, shift: f64, user_shift: f64) -> DanmakuBinding {
    DanmakuBinding {
        file: e.file.unwrap(),
        reference: ProviderRef {
            provider: Provider::Dandanplay,
            scope: String::new(),
            kind: Kind::Episode,
            id: "990088".into(),
        },
        media: None,
        manual: true,
        fingerprint_verified: false,
        shift_seconds: shift,
        user_shift_seconds: user_shift,
        revision: 0,
    }
}

#[test]
fn a_movie_is_bound_without_an_episode_or_an_account_write() {
    let mut e = engine();
    let work = bind_movie(&mut e);
    let binding = e.db.catalog_binding(e.file.unwrap()).unwrap().unwrap();
    assert_eq!(binding.decision, Decision::Manual);
    assert_eq!(binding.items[0].media, work.node.id);
    assert_eq!(e.state.subtitle, PlayingNote::Movie);
    assert_eq!(e.state.player.episode, 0);
    assert!(e.episode_playing.is_none());
    assert!(
        e.db.catalog_watch_queue("dandanplay:test")
            .unwrap()
            .is_empty()
    );
    assert!(e.db.catalog_links(&work.node.id).unwrap().is_empty());
}

#[test]
fn specials_keep_season_zero_and_cannot_be_replaced_by_stale_confirmation() {
    let mut e = engine();
    let (work, season) = series();
    let item = season.episodes[0].clone();
    e.catalog_picker.work = Some(work);
    e.catalog_picker.season = Some(season);
    e.confirm_catalog_item(item.id.0.clone()).unwrap();
    assert_eq!(
        e.state.subtitle,
        PlayingNote::SeasonEpisode {
            season: 0,
            episode: 1
        }
    );
    assert_eq!(e.media_playing, Some(item.id));
    // The dialog captured revision 0; a second confirmation must not silently
    // overwrite the first write, even if both came from the same UI instance.
    e.catalog_picker.work = Some(movie(101));
    let id = e.catalog_picker.work.as_ref().unwrap().node.id.0.clone();
    assert!(e.confirm_catalog_item(id).is_err());
    assert_eq!(
        e.state.subtitle,
        PlayingNote::SeasonEpisode {
            season: 0,
            episode: 1
        }
    );
}

#[test]
fn changing_content_preserves_danmaku_and_blocking_is_a_durable_decision() {
    let mut e = engine();
    let dm = e.db.catalog_set_danmaku(&danmaku(&e, 2.5, -1.0)).unwrap();
    bind_movie(&mut e);
    assert_eq!(e.db.catalog_danmaku(e.file.unwrap()).unwrap().unwrap(), dm);
    assert_eq!(e.state.danmaku_settings.delay, 1.5);
    e.capture_match_revisions().unwrap();
    e.block_catalog_match().unwrap();
    assert!(e.media_playing.is_none());
    assert_eq!(
        e.db.catalog_binding(e.file.unwrap())
            .unwrap()
            .unwrap()
            .decision,
        Decision::Blocked
    );
    assert_eq!(e.db.catalog_danmaku(e.file.unwrap()).unwrap().unwrap(), dm);
}

#[test]
fn old_search_work_and_season_responses_do_not_change_the_picker() {
    let mut e = engine();
    let (work, season) = series();
    e.search_revision = 7;
    e.state.matching.selected = Some(card(&work.node, None));
    e.state.matching.loading = true;
    e.state.matching.season = season.node.id.0.clone();
    e.catalog_picker.season_generation = 4;
    e.take_catalog_match(Reply::Search {
        revision: 6,
        nodes: vec![movie(10).node],
        images: None,
        error: "旧错误".into(),
    });
    e.take_catalog_match(Reply::Work {
        revision: 6,
        id: work.node.id.0.clone(),
        result: Ok(work.clone()),
    });
    e.take_catalog_match(Reply::Season {
        revision: 7,
        generation: 3,
        id: season.node.id.0.clone(),
        result: Ok(season),
    });
    assert!(e.state.matching.loading);
    assert!(e.state.matching.error.is_empty());
    assert!(e.state.matching.shows.is_empty());
    assert!(e.state.matching.episodes.is_empty());
    e.take_catalog_match(Reply::Work {
        revision: 7,
        id: work.node.id.0.clone(),
        result: Ok(work),
    });
    assert!(!e.state.matching.loading);
    assert_eq!(e.state.matching.seasons.len(), 2);
    assert!(e.catalog_picker.season.is_none());
}

#[test]
fn file_offsets_preserve_the_recognition_shift_and_reset_for_the_next_file() {
    let mut e = engine();
    let dm = e.db.catalog_set_danmaku(&danmaku(&e, 8.0, -2.0)).unwrap();
    e.set_file_danmaku(dm);
    e.state.elapsed = 60.0;
    assert_eq!(e.online_danmaku_time().unwrap(), 54.0);
    e.set_offset(4.0).unwrap();
    let saved = e.db.catalog_danmaku(e.file.unwrap()).unwrap().unwrap();
    assert_eq!(saved.shift_seconds, 8.0);
    assert_eq!(saved.user_shift_seconds, -4.0);
    assert!(e.db.setting("danmaku.delay").is_none());
    assert_eq!(e.online_danmaku_time().unwrap(), 56.0);
    e.state.elapsed = 1.0;
    assert!(e.online_danmaku_time().is_err());
    e.reset_file_danmaku();
    assert_eq!(e.state.danmaku_settings.delay, 0.0);
    assert!(e.danmaku_binding.is_none());
}

#[test]
fn a_stale_offset_edit_is_not_published_to_the_kernel_state() {
    let mut e = engine();
    let dm = e.db.catalog_set_danmaku(&danmaku(&e, 5.0, 0.0)).unwrap();
    e.set_file_danmaku(dm.clone());
    let changed = DanmakuBinding {
        user_shift_seconds: 10.0,
        ..dm
    };
    e.db.catalog_set_danmaku(&changed).unwrap();
    assert!(e.set_offset(3.0).is_err());
    assert_eq!(e.state.danmaku_settings.delay, 5.0);
    assert_eq!(
        e.db.catalog_danmaku(e.file.unwrap())
            .unwrap()
            .unwrap()
            .user_shift_seconds,
        10.0
    );
}
