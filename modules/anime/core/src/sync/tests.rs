use super::*;
use crate::anime::{Commands, WatchStatus};
use erika_catalog::{ProviderRef, tmdb};
use erika_dandanplay::auth::Registry;
use serde_json::json;

fn session(engine: &mut AnimeEngine, provider: Provider) -> &'static Registry {
    let registry: &'static Registry = Box::leak(Box::new(Registry::new()));
    let (index, name) = if provider == Provider::Bangumi {
        (1, "bangumi:123")
    } else {
        (0, "dandanplay:test")
    };
    registry.set(Some(name.into()), Some("isolated-test-credential".into()));
    engine.syncs.sessions[index] = registry.capture();
    if index == 1 {
        engine.accounts.bangumi = true;
    } else {
        engine.accounts.dandanplay = true;
    }
    registry
}
fn link(
    engine: &AnimeEngine,
    media: &MediaId,
    provider: Provider,
    kind: Kind,
    id: &str,
) -> ProviderLink {
    engine
        .db
        .catalog_link(&ProviderLink {
            media: media.clone(),
            reference: ProviderRef {
                provider,
                kind,
                scope: String::new(),
                id: id.into(),
            },
            relation: Relation::Exact,
            coverage: Vec::new(),
            evidence: "test explicit mapping".into(),
            confirmed: true,
            manual: true,
            revision: 0,
        })
        .unwrap()
}
fn series() -> (AnimeEngine, tmdb::Season, tmdb::Season) {
    let mut engine = crate::tests::engine();
    let (work, special, regular) = crate::tests::series(&engine, 2);
    engine.open(work.node.id.0).unwrap();
    (engine, special, regular)
}
fn bgm(rating: i64, interest: Option<site::Interest>) -> Remote {
    let mut c = site::Collection::default();
    c.rating = rating;
    c.interest = interest;
    Remote::Bangumi(c)
}
fn reply(engine: &mut AnimeEngine, written: bool, remote: Result<Remote, String>) {
    let id = engine.syncs.running.as_ref().unwrap().id;
    engine.take_synced(Synced {
        id,
        written,
        result: remote,
    });
}
fn mark(engine: &AnimeEngine, media: &MediaId, value: bool) {
    let old = engine.db.catalog_watched(media).unwrap();
    engine
        .db
        .catalog_set_watched(&erika_catalog::Watched {
            media: media.clone(),
            watched: value,
            manual: true,
            updated_at: erika_store::now(),
            revision: old.map_or(0, |w| w.revision),
        })
        .unwrap();
}
#[test]
fn every_details_visit_has_a_distinct_public_generation() {
    let (mut e, special, regular) = series();
    let work = e.state.opened.clone();
    let mut generations = vec![e.state.view_generation.clone()];
    for id in [&special.node.id, &regular.node.id, &regular.node.id] {
        e.select_season(id.0.clone()).unwrap();
        generations.push(e.state.view_generation.clone());
    }
    e.close().unwrap();
    generations.push(e.state.view_generation.clone());
    e.open(work).unwrap();
    generations.push(e.state.view_generation.clone());
    assert_eq!(e.state.selected_season, regular.node.id.0);
    assert!(generations.iter().all(|g| MediaId::parse(g).is_ok()));
    let unique: std::collections::HashSet<_> = generations.iter().collect();
    assert_eq!(unique.len(), generations.len());
    e.show_opened();
    assert_eq!(&e.state.view_generation, generations.last().unwrap());
}

#[test]
fn local_statuses_use_the_selected_season_without_encoded_ids() {
    let (mut e, special, regular) = series();
    e.set_status(WatchStatus::Watched).unwrap();
    assert_eq!(
        e.db.catalog_marks(&regular.node.id).unwrap()[0]
            .status
            .as_deref(),
        Some("watched")
    );
    e.select_season(special.node.id.0.clone()).unwrap();
    assert_eq!(e.state.show.status, WatchStatus::Unmarked);
    e.set_status(WatchStatus::Want).unwrap();
    assert_eq!(e.state.show.status, WatchStatus::Want);
    e.select_season(regular.node.id.0).unwrap();
    assert_eq!(e.state.show.status, WatchStatus::Watched);
    assert!(e.syncs.dispatched.is_empty());
}
#[test]
fn ratings_only_target_confirmed_bangumi_subjects_on_the_selected_season() {
    let (mut e, special, regular) = series();
    session(&mut e, Provider::Dandanplay);
    link(
        &e,
        &regular.node.id,
        Provider::Dandanplay,
        Kind::Subject,
        "42",
    );
    assert!(e.rate(8).is_err());
    session(&mut e, Provider::Bangumi);
    let mut mapping = link(
        &e,
        &regular.node.id,
        Provider::Bangumi,
        Kind::Subject,
        "393010",
    );
    e.rate(8).unwrap();
    assert_eq!(e.state.my_score, Score::Eight);
    assert_eq!(e.syncs.dispatched.len(), 1);
    assert_eq!(
        e.syncs.dispatched[0].0.subject.reference.provider,
        Provider::Bangumi
    );
    // Revocation after preflight must prevent the write.
    mapping.confirmed = false;
    e.db.catalog_link(&mapping).unwrap();
    reply(&mut e, false, Ok(bgm(0, None)));
    assert_eq!(e.syncs.dispatched.len(), 1);
    assert_eq!(e.state.my_score, Score::Eight);
    e.select_season(special.node.id.0).unwrap();
    assert!(e.rate(9).is_err());
    assert_eq!(e.state.my_score, Score::Unrated);
}
#[test]
fn newer_local_choices_and_account_aba_reject_old_callbacks() {
    let (mut e, _, regular) = series();
    let registry = session(&mut e, Provider::Bangumi);
    link(
        &e,
        &regular.node.id,
        Provider::Bangumi,
        Kind::Subject,
        "393010",
    );
    e.rate(9).unwrap();
    let old_id = e.syncs.running.as_ref().unwrap().id;
    e.rate(6).unwrap();
    reply(&mut e, false, Ok(bgm(0, None)));
    assert_eq!(e.state.my_score, Score::Six);
    assert_eq!(e.syncs.dispatched.len(), 2);
    assert!(
        e.syncs
            .dispatched
            .iter()
            .all(|(_, remote)| remote.is_none())
    );
    // The callback cannot complete a different request now running.
    e.take_synced(Synced {
        id: old_id,
        written: true,
        result: Ok(bgm(9, None)),
    });
    assert_eq!(e.state.my_score, Score::Six);
    registry.set(Some("bangumi:456".into()), Some("other".into()));
    registry.set(
        Some("bangumi:123".into()),
        Some("isolated-test-credential".into()),
    );
    e.syncs.sessions[1] = registry.capture();
    reply(&mut e, false, Ok(bgm(0, None)));
    assert_eq!(e.syncs.dispatched.len(), 2);
    assert!(e.syncs.running.is_none());
}
#[test]
fn review_dialog_generations_and_unicode_limit_are_enforced() {
    let (mut e, _, regular) = series();
    session(&mut e, Provider::Bangumi);
    link(
        &e,
        &regular.node.id,
        Provider::Bangumi,
        Kind::Subject,
        "393010",
    );
    e.show_review(true).unwrap();
    e.show_review(false).unwrap();
    e.show_review(true).unwrap();
    let mut old = site::Collection::default();
    old.comment = "旧评价".into();
    reply(&mut e, false, Ok(Remote::Bangumi(old)));
    assert!(e.state.review_text.is_empty());
    assert!(e.state.review_loading);
    reply(&mut e, false, Ok(bgm(0, None)));
    assert!(!e.state.review_loading);
    let count = e.syncs.dispatched.len();
    e.submit_review("字".repeat(201)).unwrap();
    assert!(e.state.review_error.contains("201"));
    assert_eq!(e.syncs.dispatched.len(), count);
    e.submit_review("字".repeat(200)).unwrap();
    assert!(e.state.review_saving);
    reply(&mut e, false, Ok(bgm(0, None)));
    assert!(e.syncs.dispatched.last().unwrap().1.is_some());
    let mut saved = site::Collection::default();
    saved.comment = "字".repeat(200);
    reply(&mut e, true, Ok(Remote::Bangumi(saved)));
    assert!(!e.state.review_open);
    assert!(!e.state.review_saving);
}
fn grid(episode: i64, watched: bool) -> Remote {
    let mut c = site::Collection::default();
    c.episodes = vec![site::Episode {
        id: episode,
        number: 1,
        watched,
    }];
    Remote::Bangumi(c)
}
#[test]
fn exact_episode_delivery_recovers_unknown_writes_before_retrying() {
    let (mut e, _, regular) = series();
    session(&mut e, Provider::Bangumi);
    link(
        &e,
        &regular.node.id,
        Provider::Bangumi,
        Kind::Subject,
        "393010",
    );
    link(
        &e,
        &regular.episodes[0].id,
        Provider::Bangumi,
        Kind::Episode,
        "987654",
    );
    mark(&e, &regular.episodes[0].id, true);
    e.sync_played();
    reply(&mut e, false, Ok(grid(987654, false)));
    let task = e.db.catalog_watch_queue("bangumi:123").unwrap().remove(0);
    assert_eq!(task.state, "sending");
    reply(&mut e, true, Err("timeout".into()));
    assert_eq!(
        e.db.catalog_watch_queue("bangumi:123").unwrap()[0].state,
        "unknown"
    );
    let count = e.syncs.dispatched.len();
    e.retry_watches();
    assert_eq!(e.syncs.dispatched.len(), count + 1);
    assert!(e.syncs.dispatched.last().unwrap().1.is_none());
    // The timed-out write did arrive: observe it, never send it twice.
    reply(&mut e, false, Ok(grid(987654, true)));
    assert_eq!(
        e.db.catalog_watch_queue("bangumi:123").unwrap()[0].state,
        "sent"
    );
    assert_eq!(e.syncs.dispatched.len(), count + 1);
}
#[test]
fn explicit_unwatch_is_not_imported_back_and_uses_the_real_special_id() {
    let (mut e, special, _) = series();
    session(&mut e, Provider::Bangumi);
    link(
        &e,
        &special.node.id,
        Provider::Bangumi,
        Kind::Subject,
        "333",
    );
    link(
        &e,
        &special.episodes[0].id,
        Provider::Bangumi,
        Kind::Episode,
        "1099",
    );
    mark(&e, &special.episodes[0].id, false);
    e.select_season(special.node.id.0).unwrap();
    reply(&mut e, false, Ok(grid(1099, true)));
    assert!(
        !e.db
            .catalog_watched(&special.episodes[0].id)
            .unwrap()
            .unwrap()
            .watched
    );
    e.sync_played();
    reply(&mut e, false, Ok(grid(1099, true)));
    let Intent::Watch(task) = &e.syncs.running.as_ref().unwrap().intent else {
        panic!()
    };
    assert_eq!(task.target.id, "1099");
    assert!(!task.watched);
    reply(&mut e, true, Ok(grid(1099, false)));
    assert_eq!(
        e.db.catalog_watch_queue("bangumi:123").unwrap()[0].state,
        "sent"
    );
}
#[test]
fn an_uncertain_old_watch_cannot_overtake_a_new_unwatch() {
    let (mut e, _, regular) = series();
    session(&mut e, Provider::Bangumi);
    link(
        &e,
        &regular.node.id,
        Provider::Bangumi,
        Kind::Subject,
        "393010",
    );
    link(
        &e,
        &regular.episodes[0].id,
        Provider::Bangumi,
        Kind::Episode,
        "77",
    );
    mark(&e, &regular.episodes[0].id, true);
    e.sync_played();
    reply(&mut e, false, Ok(grid(77, false)));
    reply(&mut e, true, Err("timeout".into()));
    mark(&e, &regular.episodes[0].id, false);
    e.sync_played();
    let Intent::Watch(old) = &e.syncs.running.as_ref().unwrap().intent else {
        panic!()
    };
    assert!(old.watched);
    assert_eq!(old.state, "unknown");
    reply(&mut e, false, Ok(grid(77, true)));
    let Intent::Watch(new) = &e.syncs.running.as_ref().unwrap().intent else {
        panic!()
    };
    assert!(!new.watched);
    assert_eq!(new.state, "pending");
    reply(&mut e, false, Ok(grid(77, true)));
    reply(&mut e, true, Ok(grid(77, false)));
    assert!(
        e.db.catalog_watch_queue("bangumi:123")
            .unwrap()
            .iter()
            .all(|t| t.state == "sent")
    );
    assert!(
        !e.db
            .catalog_watched(&regular.episodes[0].id)
            .unwrap()
            .unwrap()
            .watched
    );
}
#[test]
fn offline_statuses_resume_for_the_explicitly_mapped_account() {
    let (mut e, _, regular) = series();
    e.set_status(WatchStatus::Want).unwrap();
    link(
        &e,
        &regular.node.id,
        Provider::Dandanplay,
        Kind::Subject,
        "42",
    );
    session(&mut e, Provider::Dandanplay);
    e.sync_stored_marks(None);
    assert_eq!(e.syncs.dispatched.len(), 1);
    assert_eq!(e.syncs.running.as_ref().unwrap().media, regular.node.id);
    let mut d = erika_dandanplay::catalog::Details {
        anime_id: 42,
        ..Default::default()
    };
    reply(&mut e, false, Ok(Remote::Dandanplay(d.clone())));
    assert!(e.syncs.dispatched.last().unwrap().1.is_some());
    d.favorite_status = Some("favorited".into());
    reply(&mut e, true, Ok(Remote::Dandanplay(d)));
    assert_eq!(e.state.show.status, WatchStatus::Want);
}
#[test]
fn a_remote_same_number_from_another_episode_never_counts_as_a_match() {
    let (mut e, _, regular) = series();
    session(&mut e, Provider::Bangumi);
    link(
        &e,
        &regular.node.id,
        Provider::Bangumi,
        Kind::Subject,
        "393010",
    );
    link(
        &e,
        &regular.episodes[0].id,
        Provider::Bangumi,
        Kind::Episode,
        "77",
    );
    mark(&e, &regular.episodes[0].id, true);
    e.sync_played();
    reply(&mut e, false, Ok(grid(88, false)));
    assert_eq!(e.syncs.dispatched.len(), 1);
    assert_eq!(
        e.db.catalog_watch_queue("bangumi:123").unwrap()[0].state,
        "pending"
    );
    assert!(e.state.sync_note.contains("分集 ID"));
}

#[test]
fn retry_wakes_without_an_open_view_and_never_duplicates_an_inflight_write() {
    let (mut e, _, regular) = series();
    let registry = session(&mut e, Provider::Bangumi);
    link(
        &e,
        &regular.node.id,
        Provider::Bangumi,
        Kind::Subject,
        "393010",
    );
    link(
        &e,
        &regular.episodes[0].id,
        Provider::Bangumi,
        Kind::Episode,
        "77",
    );
    mark(&e, &regular.episodes[0].id, true);
    e.sync_played();
    assert!(e.syncs.watch_timer);
    e.close().unwrap();
    reply(&mut e, false, Err("offline".into()));
    assert!(e.syncs.running.is_none());
    e.inbox.lock().unwrap().push(Done::RetryWatches);
    e.take_done();
    assert!(e.syncs.running.is_some());
    assert!(e.syncs.dispatched.last().unwrap().1.is_none());
    assert!(e.syncs.watch_timer);
    reply(&mut e, false, Ok(grid(77, false)));
    let count = e.syncs.dispatched.len();
    assert!(e.syncs.dispatched.last().unwrap().1.is_some());
    e.resume_watch_retry();
    e.retry_watches();
    assert_eq!(e.syncs.dispatched.len(), count);
    assert!(e.syncs.waiting.is_empty());
    reply(&mut e, true, Err("timeout".into()));
    e.resume_watch_retry();
    assert!(e.syncs.dispatched.last().unwrap().1.is_none());
    registry.set(None, None);
    reply(&mut e, false, Ok(grid(77, true)));
    let count = e.syncs.dispatched.len();
    e.resume_watch_retry();
    assert!(!e.syncs.watch_timer);
    assert_eq!(e.syncs.dispatched.len(), count);
    assert_eq!(
        e.db.catalog_watch_queue("bangumi:123").unwrap()[0].state,
        "unknown"
    );
}

#[test]
fn imported_watch_immediately_queues_the_other_enabled_provider() {
    let (mut e, _, regular) = series();
    for (provider, subject_id, episode_id) in [
        (Provider::Bangumi, "393010", "77"),
        (Provider::Dandanplay, "42", "42001"),
    ] {
        session(&mut e, provider);
        link(&e, &regular.node.id, provider, Kind::Subject, subject_id);
        link(
            &e,
            &regular.episodes[0].id,
            provider,
            Kind::Episode,
            episode_id,
        );
    }
    e.sync_intent(Intent::Read).unwrap();
    reply(&mut e, false, Ok(grid(77, true)));
    assert!(
        e.db.catalog_watched(&regular.episodes[0].id)
            .unwrap()
            .unwrap()
            .watched
    );
    assert_eq!(e.db.catalog_watch_queue("bangumi:123").unwrap().len(), 1);
    assert_eq!(
        e.db.catalog_watch_queue("dandanplay:test").unwrap().len(),
        1
    );
    let mut d = erika_dandanplay::catalog::Details {
        anime_id: 42,
        ..Default::default()
    };
    d.episodes.push(erika_dandanplay::catalog::Episode {
        episode_id: 42001,
        ..Default::default()
    });
    reply(&mut e, false, Ok(Remote::Dandanplay(d.clone())));
    reply(&mut e, false, Ok(grid(77, true)));
    assert_eq!(
        e.db.catalog_watch_queue("bangumi:123").unwrap()[0].state,
        "sent"
    );
    assert!(
        e.syncs
            .dispatched
            .iter()
            .all(
                |(j, remote)| j.subject.reference.provider != Provider::Bangumi || remote.is_none()
            )
    );
    reply(&mut e, false, Ok(Remote::Dandanplay(d.clone())));
    assert!(e.syncs.dispatched.last().unwrap().1.is_some());
    d.episodes[0].last_watched = Some("2026-11-09T12:00:00".into());
    reply(&mut e, true, Ok(Remote::Dandanplay(d)));
    assert_eq!(
        e.db.catalog_watch_queue("dandanplay:test").unwrap()[0].state,
        "sent"
    );
    e.resume_watch_retry();
    assert!(!e.syncs.watch_timer);
}

#[test]
fn local_rating_clear_and_review_survive_remote_failure_and_reopen() {
    let (mut e, _, regular) = series();
    session(&mut e, Provider::Bangumi);
    link(
        &e,
        &regular.node.id,
        Provider::Bangumi,
        Kind::Subject,
        "393010",
    );
    e.rate(0).unwrap();
    reply(&mut e, false, Ok(bgm(9, None)));
    assert_eq!(e.state.my_score, Score::Unrated);
    reply(&mut e, true, Err("offline".into()));
    e.show_review(true).unwrap();
    reply(&mut e, false, Ok(bgm(9, None)));
    e.submit_review("保留正文".into()).unwrap();
    // The uncertain rating write holds the lane; review is durable, not lost.
    assert!(e.syncs.running.is_none());
    assert_eq!(
        e.db.catalog_marks(&regular.node.id).unwrap()[0]
            .review
            .as_deref(),
        Some("保留正文")
    );
    e.show_review(false).unwrap();
    let requests = e.syncs.dispatched.len();
    e.show_review(true).unwrap();
    assert_eq!(e.state.review_text, "保留正文");
    assert!(!e.state.review_loading);
    assert_eq!(e.syncs.dispatched.len(), requests);
}

#[test]
fn clearing_a_rating_uses_the_public_picker_and_preserves_other_fields() {
    let (mut e, _, regular) = series();
    session(&mut e, Provider::Bangumi);
    link(
        &e,
        &regular.node.id,
        Provider::Bangumi,
        Kind::Subject,
        "393010",
    );
    e.db.catalog_edit_mark(&regular.node.id, &MarkEdit::Status("watching".into()))
        .unwrap();
    e.db.catalog_edit_mark(&regular.node.id, &MarkEdit::Review("保留短评".into()))
        .unwrap();
    e.rate(8).unwrap();
    let before = e.db.catalog_marks(&regular.node.id).unwrap()[0].clone();
    for invalid in [-1, 11, i64::MAX] {
        assert!(e.rate(invalid).is_err());
        assert_eq!(
            e.db.catalog_marks(&regular.node.id).unwrap(),
            vec![before.clone()]
        );
    }
    e.show_rating(true).unwrap();
    e.pick_score(Score::Unrated).unwrap();
    let cleared = e.db.catalog_marks(&regular.node.id).unwrap()[0].clone();
    assert_eq!(cleared.rating, Some(0));
    assert_eq!(cleared.status, before.status);
    assert_eq!(cleared.review, before.review);
    assert_eq!(cleared.revision, before.revision + 1);
    assert_eq!(e.state.my_score, Score::Unrated);
    assert!(!e.state.rating_open);
    // The superseded preflight cannot send 8 after the user chose Unrated.
    reply(&mut e, false, Ok(bgm(9, None)));
    assert!(
        e.syncs
            .dispatched
            .iter()
            .all(|(_, remote)| remote.is_none())
    );
    let Intent::Subject(task) = &e.syncs.running.as_ref().unwrap().intent else {
        panic!()
    };
    assert_eq!(task.mark, cleared);
    reply(&mut e, false, Ok(bgm(9, None)));
    assert!(e.syncs.dispatched.last().unwrap().1.is_some());
    let mut saved = site::Collection::default();
    saved.interest = Some(site::Interest::Do);
    saved.comment = "保留短评".into();
    reply(&mut e, true, Ok(Remote::Bangumi(saved)));
    assert!(e.syncs.running.is_none());
    assert_eq!(e.state.my_score, Score::Unrated);
}

#[test]
fn another_engine_review_is_not_overwritten_by_an_old_read() {
    let (mut e, _, regular) = series();
    session(&mut e, Provider::Bangumi);
    link(
        &e,
        &regular.node.id,
        Provider::Bangumi,
        Kind::Subject,
        "393010",
    );
    e.show_review(true).unwrap();
    e.db.catalog_edit_mark(&regular.node.id, &MarkEdit::Review(String::new()))
        .unwrap();
    let mut remote = site::Collection::default();
    remote.comment = "过期评价".into();
    reply(&mut e, false, Ok(Remote::Bangumi(remote)));
    assert_eq!(e.state.review_text, "");
    assert!(!e.state.review_loading);
}

#[test]
fn subject_readback_recovers_after_mapping_revocation_without_resending() {
    let (mut e, _, regular) = series();
    session(&mut e, Provider::Bangumi);
    let mut mapping = link(
        &e,
        &regular.node.id,
        Provider::Bangumi,
        Kind::Subject,
        "393010",
    );
    e.rate(8).unwrap();
    reply(&mut e, false, Ok(bgm(0, None)));
    mapping.confirmed = false;
    e.db.catalog_link(&mapping).unwrap();
    reply(&mut e, true, Err("timeout".into()));
    assert_eq!(
        e.db.catalog_subject_queue("bangumi:123").unwrap()[0].state,
        "unknown"
    );
    e.retry_watches();
    let before = e.syncs.dispatched.len();
    assert!(e.syncs.dispatched.last().unwrap().1.is_none());
    reply(&mut e, false, Ok(bgm(8, None)));
    assert_eq!(
        e.db.catalog_subject_queue("bangumi:123").unwrap()[0].state,
        "sent"
    );
    assert_eq!(e.syncs.dispatched.len(), before);
}

#[test]
fn independent_engines_merge_fields_then_write_one_fresh_collection() {
    let root = std::env::temp_dir().join(format!("erika-account-engines-{}", MediaId::new()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("db.sqlite");
    let mut a = crate::tests::engine();
    a.db = Db::at(&path).unwrap();
    let (work, _, regular) = crate::tests::series(&a, 2);
    a.open(work.node.id.0.clone()).unwrap();
    let mut b = crate::tests::engine();
    b.db = Db::at(&path).unwrap();
    b.open(work.node.id.0).unwrap();
    session(&mut a, Provider::Bangumi);
    session(&mut b, Provider::Bangumi);
    link(
        &a,
        &regular.node.id,
        Provider::Bangumi,
        Kind::Subject,
        "393010",
    );
    a.rate(8).unwrap();
    b.sync_intent(Intent::Review("另一窗口的短评".into()))
        .unwrap();
    assert!(
        b.syncs.dispatched.is_empty(),
        "second engine cannot read a stale collection while the first holds its lane"
    );
    reply(&mut a, false, Ok(bgm(0, None)));
    assert!(
        a.syncs.dispatched.iter().all(|(_, r)| r.is_none()),
        "superseded preflight may not write"
    );
    // The first engine may pick up the second engine's persisted intent.
    if a.syncs.running.is_none() {
        a.retry_watches();
    }
    let Intent::Subject(task) = &a.syncs.running.as_ref().unwrap().intent else {
        panic!()
    };
    assert_eq!(task.mark.rating, Some(8));
    assert_eq!(task.mark.review.as_deref(), Some("另一窗口的短评"));
    reply(&mut a, false, Ok(bgm(0, None)));
    assert!(a.syncs.dispatched.last().unwrap().1.is_some());
    let mut remote = site::Collection::default();
    remote.rating = 8;
    remote.comment = "另一窗口的短评".into();
    reply(&mut a, true, Ok(Remote::Bangumi(remote)));
    b.retry_watches();
    assert!(
        b.syncs.dispatched.is_empty(),
        "completed durable operation is not repeated by another engine"
    );
    drop(a);
    drop(b);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_same_text_review_aba_does_not_close_the_newer_save() {
    let (mut e, _, regular) = series();
    session(&mut e, Provider::Bangumi);
    link(
        &e,
        &regular.node.id,
        Provider::Bangumi,
        Kind::Subject,
        "393010",
    );
    e.show_review(true).unwrap();
    reply(&mut e, false, Ok(bgm(0, None)));
    e.submit_review("相同正文".into()).unwrap();
    reply(&mut e, false, Ok(bgm(0, None)));
    // Another engine changes away and back while the original send is running.
    e.db.catalog_edit_mark(&regular.node.id, &MarkEdit::Review("中间正文".into()))
        .unwrap();
    e.db.catalog_edit_mark(&regular.node.id, &MarkEdit::Review("相同正文".into()))
        .unwrap();
    e.sync_stored_marks(None);
    let mut saved = site::Collection::default();
    saved.comment = "相同正文".into();
    reply(&mut e, true, Ok(Remote::Bangumi(saved.clone())));
    assert!(e.state.review_open && e.state.review_saving);
    assert!(e.syncs.dispatched.last().unwrap().1.is_none());
    reply(&mut e, false, Ok(Remote::Bangumi(saved)));
    assert!(!e.state.review_open && !e.state.review_saving);
}

#[test]
fn conflicting_movie_choices_do_not_dispatch_and_resume_after_local_resolution() {
    let mut e = crate::tests::engine();
    let work = tmdb::parse_work(Kind::Movie, 1, &json!({"id":1,"title":"电影"})).unwrap();
    e.db.catalog_save_nodes(&[work.node.clone()]).unwrap();
    e.open(work.node.id.0.clone()).unwrap();
    session(&mut e, Provider::Bangumi);
    link(&e, &work.node.id, Provider::Bangumi, Kind::Subject, "444");
    e.db.catalog_edit_mark(&work.node.id, &MarkEdit::Status("watching".into()))
        .unwrap();
    mark(&e, &work.node.id, true);
    e.sync_stored_marks(None);
    e.sync_played();
    assert!(e.syncs.dispatched.is_empty());
    assert!(e.state.sync_note.contains("电影收藏状态与已看记录冲突"));
    assert!(
        e.db.catalog_watched(&work.node.id)
            .unwrap()
            .unwrap()
            .watched
    );
    assert_eq!(
        e.db.catalog_marks(&work.node.id).unwrap()[0]
            .status
            .as_deref(),
        Some("watching")
    );
    e.set_status(WatchStatus::Watched).unwrap();
    assert_eq!(e.state.show.status, WatchStatus::Watched);
    reply(&mut e, false, Ok(bgm(7, Some(site::Interest::Wish))));
    assert!(e.syncs.dispatched.last().unwrap().1.is_some());
    reply(&mut e, true, Ok(bgm(7, Some(site::Interest::Collect))));
    e.retry_watches();
    reply(&mut e, false, Ok(bgm(7, Some(site::Interest::Collect))));
    assert!(e.syncs.running.is_none());
    assert_eq!(
        e.db.catalog_watch_queue("bangumi:123").unwrap()[0].state,
        "sent"
    );
    assert_eq!(
        e.syncs
            .dispatched
            .iter()
            .filter(|(_, r)| r.is_some())
            .count(),
        1
    );
}

#[test]
fn a_movie_uses_a_subject_not_a_fabricated_episode() {
    let mut e = crate::tests::engine();
    let work = tmdb::parse_work(Kind::Movie, 1, &json!({"id":1,"title":"电影"})).unwrap();
    e.db.catalog_save_nodes(&[work.node.clone()]).unwrap();
    e.open(work.node.id.0.clone()).unwrap();
    session(&mut e, Provider::Bangumi);
    link(&e, &work.node.id, Provider::Bangumi, Kind::Subject, "444");
    e.mark_episode(work.node.id.0, true).unwrap();
    reply(&mut e, false, Ok(bgm(7, Some(site::Interest::Wish))));
    let Intent::Watch(task) = &e.syncs.running.as_ref().unwrap().intent else {
        panic!()
    };
    assert_eq!(task.target.kind, Kind::Subject);
    reply(&mut e, true, Ok(bgm(7, Some(site::Interest::Collect))));
    assert_eq!(
        e.db.catalog_watch_queue("bangumi:123").unwrap()[0].state,
        "sent"
    );
}
