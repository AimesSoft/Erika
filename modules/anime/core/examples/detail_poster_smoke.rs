//! Unauthenticated detail-cover upgrade using an isolated app cache.
//! cargo run -p erika-anime-core --example detail_poster_smoke
use erika_anime_core::AnimeEngine;
use erika_anime_protocol::anime::{Commands, Engine};
use std::io::Read;
use std::time::{Duration, Instant};

fn main() {
    bern::storage::Store::install_application("com.aimessoft.erika.detail-poster-smoke");
    erika_dandanplay::set_token(None);
    erika_dandanplay::reviews::set_bangumi_signed_in(false);
    let details = erika_dandanplay::details_from(17527, true).unwrap();
    erika_store::Db::open().remember_anime(&erika_store::Anime {
        dandanplay_id: details.anime_id,
        title: details.anime_title,
        image: details.image_url,
    });
    let mut engine = AnimeEngine::default();
    engine.open(17527).unwrap();
    let started = Instant::now();
    loop {
        engine.wake();
        if engine.state().show.poster.url.contains("/detail-posters/") {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "HD cover did not arrive: {}",
            engine.state().details_error
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!engine.state().signed_in);
    let mut bytes = Vec::new();
    engine
        .state()
        .show
        .poster
        .open()
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let image = image::load_from_memory(&bytes).unwrap();
    assert!(image.height().max(image.width()) > 400);
    let poster = engine.state().show.poster.clone();
    engine.close().unwrap();
    engine.open(17527).unwrap();
    assert_eq!(engine.state().show.poster, poster);
    println!(
        "Guest detail: Bangumi cover {}×{}, {} bytes; cached cover is immediate on reopen",
        image.width(),
        image.height(),
        bytes.len()
    );
}
