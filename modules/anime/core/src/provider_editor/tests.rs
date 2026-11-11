use super::*;
use crate::tests::{engine, series};
use anime::Commands;
use erika_catalog::tmdb;
use serde_json::json;

#[test]
fn editor_targets_exact_hierarchy_and_never_guesses_episode_links() {
    let mut e = engine();
    let (work, special, regular) = series(&e, 2);
    e.open(work.node.id.0.clone()).unwrap();
    e.show_mappings(true).unwrap();
    assert_eq!(e.state.mapping_target, regular.node.id.0);
    assert_eq!(e.state.mapping_targets.len(), 6);
    assert!(
        e.state
            .mapping_targets
            .iter()
            .any(|t| t.title.starts_with("S00E01"))
    );
    e.save_provider_mapping(
        MappingProvider::Bangumi,
        MappingKind::Subject,
        MappingRelation::Exact,
        "123".into(),
    )
    .unwrap();
    assert_eq!(e.db.catalog_links(&regular.node.id).unwrap().len(), 1);
    assert!(e.db.catalog_links(&special.node.id).unwrap().is_empty());
    assert!(
        e.db.catalog_links(&regular.episodes[0].id)
            .unwrap()
            .is_empty()
    );
    assert!(e.db.catalog_local_marks().unwrap().is_empty());
    e.select_mapping_target(special.episodes[0].id.0.clone())
        .unwrap();
    assert!(e.state.mapping_episode);
    assert!(
        e.save_provider_mapping(
            MappingProvider::Bangumi,
            MappingKind::Subject,
            MappingRelation::Exact,
            "124".into()
        )
        .is_err()
    );
    e.save_provider_mapping(
        MappingProvider::Bangumi,
        MappingKind::Episode,
        MappingRelation::Exact,
        "125".into(),
    )
    .unwrap();
    assert!(
        e.db.catalog_links(&regular.episodes[0].id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        e.db.catalog_links(&special.episodes[0].id).unwrap()[0]
            .reference
            .id,
        "125"
    );
    assert!(e.select_mapping_target(MediaId::new().0).is_err());
    e.close().unwrap();
    assert!(
        e.save_provider_mapping(
            MappingProvider::Bangumi,
            MappingKind::Episode,
            MappingRelation::Exact,
            "126".into()
        )
        .is_err()
    );
}

#[test]
fn editor_requires_unlinking_conflicts_and_preserves_concurrent_edits() {
    let mut e = engine();
    let (work, _, regular) = series(&e, 1);
    e.open(work.node.id.0.clone()).unwrap();
    e.show_mappings(true).unwrap();
    e.save_provider_mapping(
        MappingProvider::Dandanplay,
        MappingKind::Subject,
        MappingRelation::Exact,
        "10".into(),
    )
    .unwrap();
    assert_eq!(
        e.save_provider_mapping(
            MappingProvider::Dandanplay,
            MappingKind::Subject,
            MappingRelation::Exact,
            "11".into()
        )
        .unwrap_err()
        .code,
        "conflict"
    );
    assert_eq!(e.db.catalog_links(&regular.node.id).unwrap().len(), 1);
    let row = e.state.mapping_rows[0].id.clone();
    let mut external_edit = e.db.catalog_links(&regular.node.id).unwrap().remove(0);
    external_edit.evidence = "CLI concurrent edit".into();
    e.db.catalog_request(
        &MediaId::new().0,
        &CatalogBatch {
            links: vec![external_edit],
            ..Default::default()
        },
        false,
    )
    .unwrap();
    assert_eq!(e.remove_provider_mapping(row).unwrap_err().code, "conflict");
    assert_eq!(
        e.save_provider_mapping(
            MappingProvider::Dandanplay,
            MappingKind::Subject,
            MappingRelation::Related,
            "10".into()
        )
        .unwrap_err()
        .code,
        "conflict"
    );
    assert!(!e.state.mapping_error.is_empty());
    e.select_mapping_target(regular.node.id.0.clone()).unwrap();
    e.remove_provider_mapping(e.state.mapping_rows[0].id.clone())
        .unwrap();
    e.save_provider_mapping(
        MappingProvider::Dandanplay,
        MappingKind::Subject,
        MappingRelation::Exact,
        "11".into(),
    )
    .unwrap();
    let active: Vec<_> =
        e.db.catalog_links(&regular.node.id)
            .unwrap()
            .into_iter()
            .filter(|l| l.confirmed)
            .collect();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].reference.id, "11");
}

#[test]
fn movie_subject_and_episode_links_are_independent_and_related_is_not_exact() {
    let mut e = engine();
    let node = tmdb::parse_work(Kind::Movie, 42, &json!({"id":42,"title":"电影"}))
        .unwrap()
        .node;
    e.db.catalog_save_nodes(&[node.clone()]).unwrap();
    e.open(node.id.0.clone()).unwrap();
    e.show_mappings(true).unwrap();
    assert!(e.state.mapping_movie);
    assert_eq!(e.state.mapping_targets.len(), 1);
    e.save_provider_mapping(
        MappingProvider::Bangumi,
        MappingKind::Subject,
        MappingRelation::Related,
        "12".into(),
    )
    .unwrap();
    assert!(
        crate::sync::subject(&e.db, &node.id, Provider::Bangumi)
            .unwrap()
            .is_none()
    );
    e.save_provider_mapping(
        MappingProvider::Bangumi,
        MappingKind::Subject,
        MappingRelation::Exact,
        "12".into(),
    )
    .unwrap();
    e.save_provider_mapping(
        MappingProvider::Bangumi,
        MappingKind::Episode,
        MappingRelation::Exact,
        "13".into(),
    )
    .unwrap();
    assert_eq!(e.state.mapping_rows.len(), 2);
    for invalid in ["0", "-1", "wrong", "https://bgm.tv/subject/12"] {
        assert!(
            e.save_provider_mapping(
                MappingProvider::Bangumi,
                MappingKind::Subject,
                MappingRelation::Exact,
                invalid.into()
            )
            .is_err()
        );
    }
    assert_eq!(e.db.catalog_links(&node.id).unwrap().len(), 2);
}
