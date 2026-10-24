use super::*;
use anime::Commands;
use erika_store::{Anime, Episode, Fingerprint};

fn intro(id: i64, title: &str) -> Intro {
    serde_json::from_value(serde_json::json!({ "animeId": id, "animeTitle": title, "imageUrl": format!("https://img/{id}.jpg"), "airDay": 3, "rating": 8.5 })).unwrap()
}

fn details(id: i64, episodes: i64) -> Details {
    serde_json::from_value(serde_json::json!({
        "animeId": id, "animeTitle": "葬送的芙莉莲", "imageUrl": "https://img/f.jpg", "summary": " 旅途 ", "rating": 9.1,
        "typeDescription": "TV动画", "tags": [{"name": "奇幻"}],
        "episodes": (1..=episodes).map(|n| serde_json::json!({ "episodeId": id * 10_000 + n, "episodeTitle": format!("第{n}话"), "episodeNumber": n.to_string(), "airDate": "2023-09-29T00:00:00" })).collect::<Vec<_>>(),
    }))
    .unwrap()
}

#[test]
fn without_an_account_unknown_shows_are_refused() {
    let mut engine = AnimeEngine::default();
    assert!(!engine.state.signed_in);
    assert!(engine.state.week.iter().all(|day| day.shows.is_empty()));
    assert!(!engine.state.has_stage && !engine.state.has_airing);
    assert_eq!(engine.open(99).unwrap_err().code, "unknown_show");
    assert_eq!(engine.select_day(7).unwrap_err().code, "unknown_day");
}

#[test]
fn a_show_of_the_season_opens_and_fills_in_when_its_details_come() {
    let mut engine = AnimeEngine::default();
    engine.season = vec![intro(17675, "葬送的芙莉莲")];
    engine.show_season();
    engine.open_scheduled(17675).unwrap();
    assert_eq!(
        (engine.state.opened, engine.state.back.clone()),
        (17675, BackTo::Schedule)
    );
    assert_eq!(engine.state.show.rating, 8.5);
    assert!(engine.state.episodes.is_empty());
    engine.inbox.lock().unwrap().push(Done::Details(
        engine.generation,
        17675,
        Ok(details(17675, 12)),
    ));
    anime::Engine::wake(&mut engine);
    assert_eq!(engine.state.show.episodes, 12);
    assert_eq!(engine.state.show.summary, "旅途");
    assert_eq!(
        engine.state.show.premiere,
        bern::date::CalendarDate::new(2023, 9, 29)
    );
    assert_eq!(engine.state.episodes.len(), 12);
    assert_eq!(engine.state.episodes[0].note, anime::EpisodeNote::Missing);
    assert_eq!(engine.state.next, NextAction::Start);
    engine.close().unwrap();
    assert_eq!(engine.state.opened, 0);
}

#[test]
fn what_was_played_and_matched_is_on_the_stage_and_the_film() {
    let mut engine = AnimeEngine::default();
    let frieren = Anime {
        dandanplay_id: 17675,
        title: "葬送的芙莉莲".into(),
        image: "https://img/f.jpg".into(),
    };
    for (number, position) in [(1, 1440.0), (2, 1440.0), (3, 600.0)] {
        let url = format!("file:///v/{number}.mkv");
        let file = engine
            .db
            .opened(
                &url,
                Some(&Fingerprint {
                    md5: format!("{number}"),
                    size: 1,
                }),
            )
            .unwrap();
        engine.db.link_episode(
            file,
            &frieren,
            &Episode {
                dandanplay_id: 176_750_000 + number,
                number,
                title: String::new(),
            },
            false,
        );
        engine.db.save_progress(file, position, 1440.0);
        engine.db.remember(&url, "x", None, position, 1440.0);
    }
    engine.history_changed().unwrap();
    assert!(engine.state.has_stage);
    assert_eq!(
        (
            engine.state.stage.id,
            engine.state.stage.episode,
            engine.state.stage.left
        ),
        (17675, 3, 14)
    );
    assert!(engine.state.also.is_empty());
    engine.details.insert(17675, details(17675, 12));
    engine.open(17675).unwrap();
    assert_eq!(engine.state.show.watched, 2);
    assert_eq!(engine.state.next_episode, 3);
    assert_eq!(engine.state.next, NextAction::Continue { episode: 3 });
    let third = &engine.state.episodes[2];
    assert!(third.next && !third.watched);
    assert_eq!(third.note, anime::EpisodeNote::Left { minutes: 14 });
    assert_eq!(
        engine.state.episodes[0].note,
        anime::EpisodeNote::Length { minutes: 24 }
    );
    assert!(engine.state.episodes[0].watched);
    assert_eq!(engine.state.episodes[5].note, anime::EpisodeNote::Missing);
    // The same verified show can receive Bangumi episode IDs without losing
    // the files and progress that were linked using dandanplay IDs.
    let mapped = engine.details.get_mut(&17675).unwrap();
    for (index, episode) in mapped.episodes.iter_mut().enumerate() {
        episode.episode_id = erika_dandanplay::bangumi::local_id(index as i64 + 1).unwrap();
    }
    engine.show_opened();
    assert_eq!(engine.state.show.watched, 2);
    assert_eq!(engine.state.next_episode, 3);
    assert_eq!(
        engine.state.episodes[2].note,
        anime::EpisodeNote::Left { minutes: 14 }
    );
    assert_eq!(engine.resume(1).unwrap_err().code, "unknown_show");
}

#[test]
fn signing_out_keeps_public_season_visible() {
    let mut engine = AnimeEngine::default();
    engine.accounts = catalog::Accounts {
        dandanplay: true,
        bangumi: true,
    };
    engine.state.signed_in = true;
    engine.season = vec![intro(1, "一")];
    engine.account_changed(false).unwrap();
    assert!(engine.state.signed_in);
    assert!(engine.accounts.bangumi);
    assert!(!engine.accounts.dandanplay);
    engine.bangumi_account_changed(false).unwrap();
    assert!(!engine.state.signed_in);
    assert_eq!(engine.season.len(), 1);
}

#[test]
fn a_search_result_outside_the_season_can_open_with_its_preview() {
    let mut engine = AnimeEngine::default();
    let id = erika_dandanplay::bangumi::local_id(393010).unwrap();
    engine.search_results.lock().unwrap().insert(id, serde_json::from_value(serde_json::json!({
        "animeId": id, "animeTitle": "搜索到的番剧", "summary": "简介", "imageUrl": "https://lain.bgm.tv/poster.jpg"
    })).unwrap());
    engine.open_from_search(id).unwrap();
    assert_eq!(engine.state.show.title, "搜索到的番剧");
    assert_eq!(engine.state.show.summary, "简介");
    assert_eq!(engine.state.back, BackTo::Search);
}

#[test]
fn responses_from_the_previous_account_selection_cannot_replace_current_data() {
    let mut engine = AnimeEngine::default();
    engine.generation = 2;
    engine.state.loading = true;
    engine.inbox.lock().unwrap().extend([
        Done::Season(1, Ok(vec![intro(1, "旧来源")])),
        Done::Details(1, 1, Ok(details(1, 2))),
        Done::Season(2, Ok(vec![intro(2, "当前来源")])),
    ]);
    engine.take_done();
    assert_eq!(engine.season[0].anime_title, "当前来源");
    assert!(!engine.details.contains_key(&1));
    assert!(!engine.state.loading);
}

#[test]
fn switching_season_source_refreshes_and_ignores_the_previous_response() {
    let mut engine = AnimeEngine::default();
    assert_eq!(
        engine
            .set_season_source(SeasonSource::Bangumi)
            .unwrap_err()
            .code,
        "accounts_required"
    );
    engine.accounts = catalog::Accounts {
        dandanplay: true,
        bangumi: true,
    };
    let previous = engine.generation;
    engine.set_season_source(SeasonSource::Bangumi).unwrap();
    assert!(engine.state.can_choose_season_source && engine.state.loading);
    assert_eq!(engine.state.season_source, SeasonSource::Bangumi);
    assert_eq!(
        catalog::saved_season_source(&engine.db),
        SeasonSource::Bangumi
    );
    assert!(engine.generation > previous);
    engine.inbox.lock().unwrap().extend([
        Done::Season(previous, Ok(vec![intro(1, "旧来源")])),
        Done::Season(engine.generation, Ok(vec![intro(2, "所选来源")])),
    ]);
    engine.take_done();
    assert_eq!(engine.season[0].anime_title, "所选来源");
    // Account loss temporarily overrides, but never replaces the saved choice.
    engine.bangumi_account_changed(false).unwrap();
    assert_eq!(engine.state.season_source, SeasonSource::Dandanplay);
    assert!(!engine.state.can_choose_season_source);
    assert_eq!(
        catalog::saved_season_source(&engine.db),
        SeasonSource::Bangumi
    );
    engine.bangumi_account_changed(true).unwrap();
    assert_eq!(engine.state.season_source, SeasonSource::Bangumi);
    assert!(engine.state.can_choose_season_source);
    let generation = engine.generation;
    engine.set_season_source(SeasonSource::Bangumi).unwrap();
    assert_eq!(engine.generation, generation);
    assert_ne!(
        season_cache(&SeasonSource::Bangumi),
        season_cache(&SeasonSource::Dandanplay)
    );
}

#[test]
fn a_failed_detail_refresh_keeps_cached_content_and_a_success_clears_the_error() {
    let mut engine = AnimeEngine::default();
    engine.state.opened = 17527;
    engine.details.insert(17527, details(17527, 8));
    engine.show_opened();
    engine.state.details_loading = true;
    engine.inbox.lock().unwrap().push(Done::Details(
        0,
        17527,
        Err(erika_dandanplay::Error {
            message: "offline".into(),
            sign_in: false,
        }),
    ));
    engine.take_done();
    assert!(engine.state.details_failed);
    assert!(!engine.state.details_loading);
    assert_eq!(engine.state.details_error, "offline");
    assert_eq!(engine.state.show.summary, "旅途");
    engine
        .inbox
        .lock()
        .unwrap()
        .push(Done::Details(0, 17527, Ok(details(17527, 13))));
    engine.take_done();
    assert!(!engine.state.details_failed);
    assert!(engine.state.details_error.is_empty());
    assert_eq!(engine.state.episodes.len(), 13);
}

#[test]
fn the_clock_moves_now_along_once_the_minute_turns() {
    let mut engine = AnimeEngine::default();
    engine.state.now = -1;
    engine.minute -= 1;
    engine.tick().unwrap();
    assert!((0..86_400).contains(&engine.state.now));
}

#[test]
fn episodes_and_the_status_marked_here_show_and_never_go_back() {
    let mut engine = AnimeEngine::default();
    engine.details.insert(17675, details(17675, 12));
    engine.open(17675).unwrap();
    assert_eq!(engine.state.show.status, WatchStatus::Unmarked);
    assert_eq!(engine.state.sync_targets, SyncTargets::Local);
    assert!(!engine.state.can_rate && !engine.state.can_review);
    // The film's buttons wait for the plugin that offers them.
    assert!(!engine.state.episode_marking);
    engine.set_episode_marking(true).unwrap();
    assert!(engine.state.episode_marking);
    engine.mark_episode(1, true).unwrap();
    engine.mark_episode(2, true).unwrap();
    let first = &engine.state.episodes[0];
    assert!(first.watched && first.progress == 1.0);
    assert_eq!(first.note, anime::EpisodeNote::Missing);
    assert_eq!(engine.state.show.watched, 2);
    assert_eq!(engine.state.next, NextAction::Continue { episode: 3 });
    assert_eq!(engine.state.show.status, WatchStatus::Watching);
    // Wanting to watch a show being watched keeps it watching, and says so.
    engine.set_status(WatchStatus::Want).unwrap();
    assert_eq!(engine.state.show.status, WatchStatus::Watching);
    assert!(engine.state.sync_note.contains("「在看」"));
    engine.set_status(WatchStatus::Watched).unwrap();
    assert_eq!(engine.state.show.status, WatchStatus::Watched);
    assert_eq!(engine.db.mark(17675).status.as_deref(), Some("watched"));
    // Taking a mark back is the user's to do.
    engine.mark_episode(2, false).unwrap();
    assert!(!engine.state.episodes[1].watched);
    assert_eq!(engine.db.watched_numbers(17675), vec![1]);
    assert_eq!(
        engine.mark_episode(99, true).unwrap_err().code,
        "unknown_episode"
    );
    engine.rate(8).unwrap();
    assert_eq!(engine.state.my_score, Score::Eight);
    engine.show_rating(true).unwrap();
    engine.pick_score(Score::Ten).unwrap();
    assert_eq!(engine.state.my_score, Score::Ten);
    assert!(!engine.state.rating_open);
    assert_eq!(engine.db.mark(17675).rating, 10);
}

#[test]
fn what_a_sync_brings_back_is_kept_and_shown() {
    let mut engine = AnimeEngine::default();
    engine.details.insert(17675, details(17675, 12));
    engine.open(17675).unwrap();
    engine.take_synced(sync::Synced {
        id: 17675,
        level: Some(Level::Watched),
        watched: [1, 2, 3].into(),
        rating: Some(9),
        review: Some("很好看".into()),
        reviewed: None,
        notes: vec!["Bangumi：暂时连不上 Bangumi".into()],
    });
    assert_eq!(engine.state.show.status, WatchStatus::Watched);
    assert_eq!(engine.state.show.watched, 3);
    assert!(
        engine.state.episodes[..3]
            .iter()
            .all(|episode| episode.watched)
    );
    assert!(!engine.state.episodes[3].watched);
    assert_eq!(engine.state.my_score, Score::Nine);
    assert_eq!(engine.state.review_text, "很好看");
    assert_eq!(engine.state.sync_note, "Bangumi：暂时连不上 Bangumi");
    assert_eq!(engine.db.watched_numbers(17675), vec![1, 2, 3]);
    // A sync of another show changes nothing on the open one.
    engine.take_synced(sync::Synced {
        id: 1,
        level: Some(Level::Dropped),
        ..Default::default()
    });
    assert_eq!(engine.state.show.status, WatchStatus::Watched);
    assert_eq!(engine.db.mark(1).status.as_deref(), Some("dropped"));
}

#[test]
fn a_review_needs_bangumi_and_fits_its_limit() {
    let mut engine = AnimeEngine::default();
    engine.details.insert(17675, details(17675, 12));
    engine.open(17675).unwrap();
    assert_eq!(
        engine.submit_review("好看".into()).unwrap_err().code,
        "bangumi_required"
    );
    engine.show_review(true).unwrap();
    assert!(engine.state.review_open && !engine.state.review_loading);
    assert!(!engine.state.review_error.is_empty());
    // Signed in, but this show has no Bangumi subject to write to.
    engine.accounts.bangumi = true;
    engine.submit_review("长".repeat(201)).unwrap();
    assert!(engine.state.review_error.contains("201"));
    assert!(!engine.state.review_saving);
    engine.submit_review(" 好看 ".into()).unwrap();
    assert!(!engine.state.review_saving);
    assert!(engine.state.review_open);
    assert_eq!(engine.state.review_text, "好看");
    assert!(!engine.state.review_error.is_empty());
    engine.show_review(false).unwrap();
    assert!(!engine.state.review_open);
}
