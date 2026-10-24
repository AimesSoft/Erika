//! Read-only smoke test against the deployed public endpoints, without credentials.
//! cargo run -p erika-anime-core --example public_cache_smoke
use erika_anime_core::AnimeEngine;
use erika_anime_protocol::anime::{Commands, Engine};
use std::time::{Duration, Instant};

fn wait(engine: &mut AnimeEngine, comments: bool) {
    let started = Instant::now();
    loop {
        engine.wake();
        if !(if comments {
            engine.state().comments_loading
        } else {
            engine.state().loading
        }) {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(75),
            "request did not finish"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn main() {
    bern::storage::Store::install_application("com.aimessoft.erika.cache-smoke");
    erika_dandanplay::set_token(None);
    let mut engine = AnimeEngine::default();
    engine.wake();
    wait(&mut engine, false);
    let count: usize = engine.state().week.iter().map(|d| d.shows.len()).sum();
    assert!(count > 0, "{}", engine.state().error);
    assert!(!engine.state().signed_in);
    let id = 17527;
    engine.open(id).unwrap();
    wait(&mut engine, true);
    assert_eq!(
        engine.state().comments.len(),
        20,
        "{}",
        engine.state().comments_error
    );
    assert!(!engine.state().show.summary.is_empty());
    let poster_started = Instant::now();
    while !engine.state().show.poster.url.starts_with("file:") {
        engine.wake();
        assert!(
            poster_started.elapsed() < Duration::from_secs(60),
            "poster did not reach native image cache"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(engine.state().show.poster.open().is_ok());
    println!(
        "Guest: {count} shows, cached poster and summary, {} reviews ({})",
        engine.state().comments.len(),
        engine.state().comments_source
    );
    // Simulate only the account marker; this public comments API needs no cookie.
    engine.bangumi_account_changed(true).unwrap();
    wait(&mut engine, true);
    assert!(
        engine.state().comments_error.is_empty(),
        "{}",
        engine.state().comments_error
    );
    assert!(!engine.state().comments.is_empty());
    println!(
        "Bangumi preference: {} reviews from {}",
        engine.state().comments.len(),
        engine.state().comments_source
    );
}
