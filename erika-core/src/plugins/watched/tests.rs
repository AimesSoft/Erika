use super::*;
use erika_catalog::{MediaNode, ProviderRef, tmdb};

const OWNER: &str = "test.watched";
fn setup() -> (ErikaEngine, View, Vec<MediaNode>) {
    let mut e = ErikaEngine::default();
    e.db = erika_store::Db::in_memory();
    e.plugins = Plugins::default();
    e.state.features.plugins = true;
    e.plugins.saved.enabled.push(OWNER.into());
    let pack = Pack::from_pack(&json!({"id":OWNER,"name":"观看测试","version":"1","category":"tools","permissions":["library.write"]})).unwrap();
    let runtime = Runtime::start(&pack, "", &[], &json!({})).unwrap();
    e.plugins.runtimes.insert(OWNER.into(), runtime);
    e.plugins.installed.push(super::super::Installed {
        pack,
        dir: PathBuf::new(),
        entries: vec![],
        block_words: vec![],
        loaded: true,
        error: String::new(),
    });
    let work = tmdb::parse_work(
        Kind::Tv,
        701,
        &json!({"id":701,"name":"剧集","seasons":[{"id":702,"season_number":0,"name":"特别篇"}]}),
    )
    .unwrap();
    let season = tmdb::parse_season(
        &work,
        0,
        &json!({"id":702,"name":"特别篇","season_number":0,"episodes":[
            {"id":703,"season_number":0,"episode_number":1,"name":"A"},
            {"id":704,"season_number":0,"episode_number":2,"name":"B"}
        ]}),
    )
    .unwrap();
    let view = View {
        opened: work.node.id.0.clone(),
        season: season.node.id.0.clone(),
        generation: MediaId::new().0,
    };
    let nodes = vec![
        work.node,
        season.node,
        season.episodes[0].clone(),
        season.episodes[1].clone(),
    ];
    e.db.catalog_save_nodes(&nodes).unwrap();
    for (node, kind, id) in [
        (&nodes[1], Kind::Subject, "42"),
        (&nodes[2], Kind::Episode, "918273"),
        (&nodes[3], Kind::Episode, "987654"),
    ] {
        e.db.catalog_link(&ProviderLink {
            media: node.id.clone(),
            reference: ProviderRef {
                provider: Provider::Dandanplay,
                kind,
                scope: String::new(),
                id: id.into(),
            },
            relation: Relation::Exact,
            coverage: vec![],
            confirmed: true,
            manual: true,
            evidence: "test".into(),
            revision: 0,
        })
        .unwrap();
    }
    (e, view, nodes)
}
fn details() -> Details {
    serde_json::from_value(json!({"animeId":42,"episodes":[
        {"episodeId":918273,"episodeNumber":"25","seasonId":"opaque"},
        {"episodeId":987654,"episodeNumber":"1","seasonId":"opaque"}
    ]}))
    .unwrap()
}
fn queue(e: &mut ErikaEngine, view: &View, number: i64, watched: bool) -> u64 {
    e.prepare_plugin_watch(OWNER, 42, number, watched, view.clone())
        .unwrap()
        .unwrap()
}
fn finish(e: &mut ErikaEngine, view: &View, tag: u64) -> Result<bool> {
    e.finish_plugin_watch(tag, Ok(details()), Some(view.clone()))
}

#[test]
fn provider_labels_select_real_ids_not_tmdb_numbers_including_special_seasons() {
    let (mut e, view, nodes) = setup();
    let tag = queue(&mut e, &view, 1, true);
    assert!(finish(&mut e, &view, tag).unwrap());
    assert!(
        e.db.catalog_watched(&nodes[2].id).unwrap().is_none(),
        "not TMDB E01"
    );
    assert!(e.db.catalog_watched(&nodes[3].id).unwrap().unwrap().watched);
    let tag = queue(&mut e, &view, 25, true);
    assert!(finish(&mut e, &view, tag).unwrap());
    assert!(e.db.catalog_watched(&nodes[2].id).unwrap().unwrap().watched);
}

#[test]
fn labels_are_lossless_and_ambiguous_or_malformed_details_are_rejected() {
    for label in ["12.5", "SP1", "C1", "S00E01", "", "-1", "+1"] {
        let mut d = details();
        d.episodes.truncate(1);
        d.episodes[0].episode_number = label.into();
        assert!(episode_id(&d, 42, 1).is_err(), "{label}");
    }
    let mut d = details();
    d.episodes[0].episode_number = "001".into();
    assert!(
        episode_id(&d, 42, 1).is_err(),
        "duplicate numbers across provider seasons"
    );
    d.episodes.pop();
    assert_eq!(episode_id(&d, 42, 1).unwrap(), "918273");
    d.episodes[0].episode_id = 0;
    assert!(episode_id(&d, 42, 1).is_err());
    assert!(episode_id(&details(), 43, 1).is_err());
    let mut d = details();
    d.episodes[1].episode_id = d.episodes[0].episode_id;
    assert!(
        episode_id(&d, 42, 25).is_err(),
        "one ID with multiple labels"
    );
}

#[test]
fn late_results_cannot_overwrite_changed_or_aba_provider_mappings() {
    for index in [1, 2] {
        let (mut e, view, nodes) = setup();
        let tag = queue(&mut e, &view, 25, true);
        let mut link = e.db.catalog_links(&nodes[index].id).unwrap().remove(0);
        link.confirmed = false;
        link = e.db.catalog_link(&link).unwrap();
        link.confirmed = true;
        e.db.catalog_link(&link).unwrap();
        assert_eq!(finish(&mut e, &view, tag).unwrap_err().code, "conflict");
        assert!(e.db.catalog_watched(&nodes[2].id).unwrap().is_none());
    }
}

#[test]
fn late_results_cannot_overwrite_a_new_manual_watch_decision() {
    let (mut e, view, nodes) = setup();
    let tag = queue(&mut e, &view, 25, true);
    let latest =
        e.db.catalog_set_watched(&Watched {
            media: nodes[2].id.clone(),
            watched: false,
            manual: true,
            revision: 0,
            updated_at: erika_store::now(),
        })
        .unwrap();
    assert_eq!(finish(&mut e, &view, tag).unwrap_err().code, "conflict");
    assert_eq!(e.db.catalog_watched(&nodes[2].id).unwrap(), Some(latest));
}

#[test]
fn navigation_permission_revocation_disable_and_restarted_runtime_drop_old_work() {
    for scenario in 0..5 {
        let (mut e, mut view, nodes) = setup();
        let tag = queue(&mut e, &view, 25, true);
        match scenario {
            0 => view.season = MediaId::new().0,
            1 => e.plugins.saved.enabled.clear(),
            2 => e.state.features.plugins = false,
            3 => {
                e.plugins.installed[0].pack = Pack::from_pack(&json!({"id":OWNER,"name":"test","version":"1","category":"tools","permissions":[]})).unwrap();
            }
            _ => {
                let pack = &e.plugins.installed[0].pack;
                let runtime = Runtime::start(pack, "", &[], &json!({})).unwrap();
                e.plugins.runtimes.insert(OWNER.into(), runtime);
            }
        }
        assert!(!finish(&mut e, &view, tag).unwrap());
        assert!(e.db.catalog_watched(&nodes[2].id).unwrap().is_none());
    }
}

#[test]
fn reopening_the_same_details_rejects_the_previous_visits_reply() {
    let (mut e, view, nodes) = setup();
    let tag = queue(&mut e, &view, 25, true);
    let revisited = View {
        generation: MediaId::new().0,
        ..view.clone()
    };
    assert!(!finish(&mut e, &revisited, tag).unwrap());
    assert!(e.db.catalog_watched(&nodes[2].id).unwrap().is_none());
    let tag = queue(&mut e, &revisited, 25, true);
    assert!(finish(&mut e, &revisited, tag).unwrap());
    assert!(e.db.catalog_watched(&nodes[2].id).unwrap().unwrap().watched);
}

#[test]
fn repeated_calls_share_a_bounded_get_and_keep_the_latest_intent() {
    let (mut e, view, nodes) = setup();
    let tag = queue(&mut e, &view, 25, true);
    assert!(
        e.prepare_plugin_watch(OWNER, 42, 25, false, view.clone())
            .unwrap()
            .is_none()
    );
    assert_eq!(e.plugins.watches.0.len(), 1);
    assert!(finish(&mut e, &view, tag).unwrap());
    assert!(!e.db.catalog_watched(&nodes[2].id).unwrap().unwrap().watched);
    assert!(!finish(&mut e, &view, tag).unwrap(), "duplicate callback");
    for number in 0..16 {
        queue(&mut e, &view, number, true);
    }
    assert!(
        e.prepare_plugin_watch(OWNER, 42, 100, true, view.clone())
            .is_err()
    );
    e.plugins.watches.stop(OWNER);
    assert!(e.plugins.watches.0.is_empty());
}

#[test]
fn missing_episode_evidence_never_falls_back_to_a_subject_or_a_ui_number() {
    let (mut e, view, nodes) = setup();
    let mut link = e.db.catalog_links(&nodes[3].id).unwrap().remove(0);
    link.confirmed = false;
    e.db.catalog_link(&link).unwrap();
    let tag = queue(&mut e, &view, 1, true);
    assert!(finish(&mut e, &view, tag).is_err());
    assert!(e.db.catalog_export().unwrap().watched.is_empty());
    let root_view = View {
        opened: view.opened,
        season: String::new(),
        generation: MediaId::new().0,
    };
    assert!(
        e.prepare_plugin_watch(OWNER, 42, 1, true, root_view)
            .is_err()
    );
}

#[test]
fn failed_public_get_changes_no_watch_state() {
    let (mut e, view, _) = setup();
    let tag = queue(&mut e, &view, 25, true);
    assert!(
        e.finish_plugin_watch(
            tag,
            Err(erika_dandanplay::Error {
                message: "读取失败".into(),
                sign_in: false
            }),
            Some(view)
        )
        .is_err()
    );
    assert!(e.db.catalog_export().unwrap().watched.is_empty());
    assert!(e.plugins.watches.0.is_empty());
}
