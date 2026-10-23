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
fn without_an_account_nothing_is_listed_and_unknown_shows_are_refused() {
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
    engine
        .inbox
        .lock()
        .unwrap()
        .push(Done::Details(17675, Ok(details(17675, 12))));
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
    assert_eq!(engine.state.episodes[0].note, anime::EpisodeNote::Watched);
    assert_eq!(engine.state.episodes[5].note, anime::EpisodeNote::Missing);
    assert_eq!(engine.resume(1).unwrap_err().code, "unknown_show");
}

#[test]
fn signing_out_forgets_the_season() {
    let mut engine = AnimeEngine::default();
    engine.state.signed_in = true;
    engine.season = vec![intro(1, "一")];
    engine.account_changed(false).unwrap();
    assert!(!engine.state.signed_in && engine.season.is_empty());
}
