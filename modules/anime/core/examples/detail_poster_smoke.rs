//! Unauthenticated TMDB detail artwork using an isolated app cache.
//! cargo run -p erika-anime-core --example detail_poster_smoke
use erika_anime_core::AnimeEngine;
use erika_anime_protocol::anime::{Commands, Engine};
use std::io::Read;
use std::time::{Duration, Instant};

fn wait(engine: &mut AnimeEngine, ready: impl Fn(&erika_anime_protocol::anime::State) -> bool) {
    let started = Instant::now();
    loop {
        engine.wake();
        if ready(engine.state()) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(75),
            "detail request timed out: {}",
            engine.state().details_error
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn main() {
    bern::storage::Store::install_application("com.aimessoft.erika.detail-poster-smoke");
    erika_dandanplay::set_token(None);
    erika_dandanplay::reviews::set_bangumi_signed_in(false);
    let mut engine = AnimeEngine::default();
    wait(&mut engine, |state| !state.loading);
    let id = engine
        .state()
        .week
        .iter()
        .flat_map(|day| &day.shows)
        .next()
        .expect("public schedule is empty")
        .id
        .clone();
    engine.open_scheduled(id).unwrap();
    wait(&mut engine, |state| !state.details_loading);
    assert!(
        !engine.state().details_failed,
        "{}",
        engine.state().details_error
    );
    assert!(!engine.state().signed_in);
    let media = engine.state().opened.clone();
    assert!(erika_catalog::MediaId::parse(&media).is_ok());
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
    engine.open(media).unwrap();
    assert_eq!(engine.state().show.poster, poster);
    println!(
        "Guest detail: TMDB cover {}×{}, {} bytes; stored artwork is immediate on reopen",
        image.width(),
        image.height(),
        bytes.len()
    );
}
