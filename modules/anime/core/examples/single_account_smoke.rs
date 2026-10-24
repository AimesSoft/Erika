//! Read-only Bangumi-only browse/search/detail check, isolated from real accounts.
//! cargo run -p erika-anime-core --example single_account_smoke
use bern::{CallContext, Entry, Host, Invocation, Value};
use erika_anime_core::AnimeEngine;
use erika_anime_protocol::anime::{self, Commands, Engine};
use std::time::{Duration, Instant};

fn wait(engine: &mut AnimeEngine, ready: impl Fn(&anime::State) -> bool) {
    let started = Instant::now();
    loop {
        engine.wake();
        if ready(engine.state()) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(75),
            "catalogue request timed out"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn context() -> CallContext {
    CallContext {
        entry: Entry::Test,
        request_id: "single-account-smoke".into(),
    }
}

fn main() {
    bern::storage::Store::install_application("com.aimessoft.erika.single-account-smoke");
    erika_dandanplay::set_token(None);
    // Process-local preference only; public browsing needs no Bangumi cookie.
    erika_dandanplay::reviews::set_bangumi_signed_in(true);
    let mut engine = AnimeEngine::default();
    wait(&mut engine, |state| !state.loading);
    assert!(engine.state().signed_in);
    assert!(engine.state().error.is_empty(), "{}", engine.state().error);
    let count: usize = engine.state().week.iter().map(|day| day.shows.len()).sum();
    let show = engine
        .state()
        .week
        .iter()
        .flat_map(|day| &day.shows)
        .find(|show| erika_dandanplay::bangumi::subject_id(show.id) == Some(393010))
        .or_else(|| engine.state().week.iter().flat_map(|day| &day.shows).next())
        .unwrap();
    let id = show.id;
    assert!(erika_dandanplay::bangumi::subject_id(id).is_some());
    engine.open_scheduled(id).unwrap();
    wait(&mut engine, |state| !state.details_loading);
    assert!(
        !engine.state().details_failed,
        "{}",
        engine.state().details_error
    );
    assert!(!engine.state().show.summary.is_empty());
    assert!(!engine.state().episodes.is_empty());
    println!(
        "Bangumi only: {count} scheduled shows; detail has a summary, poster and {} episodes",
        engine.state().episodes.len()
    );
    assert!(!engine.state().show.poster.url.is_empty());

    let host = Host::start(anime::Adapter(engine));
    let result = host
        .query_blocking(
            Invocation {
                command: host.resolve("anime.search").unwrap(),
                params: vec![Value::Text("葬送的芙莉莲".into())],
            },
            context(),
            Duration::from_secs(35),
        )
        .result
        .unwrap();
    use bern::bind::BernValue;
    let found = anime::SearchResults::from_value(&result).unwrap();
    assert!(found.has_shows);
    assert!(
        found
            .shows
            .iter()
            .all(|show| erika_dandanplay::bangumi::subject_id(show.id).is_some())
    );
    let id = found.shows[0].id;
    host.dispatch_blocking(
        Invocation {
            command: host.resolve("anime.open_from_search").unwrap(),
            params: vec![Value::Int(id)],
        },
        context(),
    )
    .result
    .unwrap();
    println!(
        "Bangumi only: {} search results; the first opens directly",
        found.shows.len()
    );
}
