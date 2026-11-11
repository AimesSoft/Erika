use super::*;

fn source_op(id: &str, server: &str) -> Op {
    let location = serde_json::json!(["test.restore", id, server]).to_string();
    Op {
        category: SOURCES.into(), key: format!("plugin\n{location}"),
        value: Some(Value::map([
            ("kind", Value::text("plugin")), ("name", Value::text("来源")),
            ("location", Value::text(location)), ("content", Value::text("movie")),
            ("added", Value::Int(123)),
        ])),
    }
}

#[test]
fn source_restore_errors_roll_back_media_and_settings_before_external_side_effects() {
    let dir = std::env::temp_dir().join(format!("erika-source-restore-{}", erika_catalog::MediaId::new().0));
    std::fs::create_dir(&dir).unwrap();
    let store = Store::directory(&dir);
    let vault = MemoryVault::default();
    vault.write(DANDANPLAY_TOKEN, "fake-restore-account");
    store.write("welcome-v1", b"old").unwrap();
    let db = Db::in_memory();
    let mut device = Device::new(&db, &store, &vault, Scope::Export);
    device.apply(&[source_op("library", "server-a")], None).unwrap();
    let before = device.collect(None).unwrap();
    let applied = device.applied.clone();
    let node = erika_catalog::tmdb::parse_work(erika_catalog::Kind::Movie, 71,
        &serde_json::json!({"id":71,"title":"待恢复电影"})).unwrap().node;
    let media = crate::catalog::records(&erika_store::catalog::CatalogSnapshot {
        nodes: vec![node], ..Default::default()
    }).unwrap();
    for mut bad in [source_op("library", "server-b"), source_op("library", "server-a")] {
        if bad.key.ends_with("server-a\"]") { bad.key = "plugin\nforged-key".into(); }
        device.collect(None).unwrap();
        let mut ops = crate::merge::diff(&State::new(), &media);
        ops.extend([
            Op { category: SETTINGS.into(), key:"autoplay".into(), value:Some(Value::text("false")) },
            Op { category: STORE.into(), key:"welcome-v1".into(), value:Some(Value::Bytes(b"new".to_vec())) },
            Op { category: ACCOUNTS.into(), key:"dandanplay".into(), value:None },
            source_op("new-library", "server-a"), bad,
        ]);
        assert!(device.apply(&ops, None).is_err());
        assert_eq!(device.collect(None).unwrap(), before);
        assert_eq!(device.applied, applied);
        assert_eq!(vault.read(DANDANPLAY_TOKEN).as_deref(),Some("fake-restore-account"));
        assert_eq!(store.read("welcome-v1").unwrap().unwrap(), b"old");
        assert_eq!(db.plugin_source_ids("test.restore").unwrap().len(),1);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn both_sync_scopes_restore_source_ownership_and_registration_is_idempotent() {
    let dir = std::env::temp_dir().join(format!("erika-source-roundtrip-{}", erika_catalog::MediaId::new().0));
    std::fs::create_dir(&dir).unwrap();
    let store = Store::directory(&dir);
    for scope in [Scope::Export, Scope::Repository] {
        let db = Db::in_memory();
        db.add_source("folder", "本机目录", "file:///different", "movie").unwrap();
        let mut device = Device::new(&db, &store, &NoVault, scope);
        device.apply(&[source_op("library", "server-a")], None).unwrap();
        let id = device.applied.new_sources[0];
        assert_eq!(db.plugin_source_ids("test.restore").unwrap(),vec![id]);
        let source = erika_store::plugin_sources::Descriptor {
            id:"library".into(), server_id:"server-a".into(), name:"来源".into(), content:"movie".into(),
        };
        assert_eq!(db.plugin_register_source("test.restore", &source).unwrap(),id);
        device.apply(&[source_op("library", "server-a")], None).unwrap();
        assert_eq!(device.applied.new_sources,vec![id]);
        assert_eq!(db.sources().len(),2);
        assert!(db.plugin_source_items("test.restore",id).unwrap().is_empty());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

const LOCAL: &[&str] = &[
    "device.id",
    "dandanplay.user",
    "bangumi.user",
    "sync.webdav.url",
    "anime.tmdb.sources.v1",
    "anime.calendar.dandanplay.v1",
    "anime.calendar.bangumi.v1",
    "anime.broadcast_schedule.v1",
];

#[test]
fn fetched_caches_and_private_settings_stay_local_in_both_directions() {
    let dir = std::env::temp_dir().join(format!(
        "erika-sync-cache-{}",
        erika_catalog::MediaId::new().0
    ));
    std::fs::create_dir(&dir).unwrap();
    let store = Store::directory(&dir);
    for scope in [Scope::Export, Scope::Repository] {
        let db = Db::in_memory();
        for key in LOCAL {
            db.set_setting(key, Some("local"));
        }
        db.set_setting("autoplay", Some("false"));
        let mut device = Device::new(&db, &store, &NoVault, scope);
        let state = device.collect(None).unwrap();
        assert_eq!(state[SETTINGS].len(), 1);
        for key in LOCAL {
            assert!(!state[SETTINGS].contains_key(*key));
            assert!(!portable(SETTINGS, key, &Value::text("incoming")));
        }
        for value in [Some(Value::text("incoming")), None] {
            let ops: Vec<_> = LOCAL
                .iter()
                .map(|key| Op {
                    category: SETTINGS.into(),
                    key: (*key).into(),
                    value: value.clone(),
                })
                .collect();
            device.apply(&ops, None).unwrap();
            for key in LOCAL {
                assert_eq!(db.setting(key).as_deref(), Some("local"), "{key}");
            }
        }
        assert_eq!(device.applied.changes, 0);
        assert!(!device.applied.settings);
        device
            .apply(
                &[Op {
                    category: SETTINGS.into(),
                    key: "autoplay".into(),
                    value: Some(Value::text("true")),
                }],
                None,
            )
            .unwrap();
        assert_eq!(db.setting("autoplay").as_deref(), Some("true"));
        assert_eq!(device.applied.changes, 1);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn repository_does_not_replace_or_delete_device_download_paths_but_backups_can() {
    let dir = std::env::temp_dir().join(format!(
        "erika-sync-path-{}",
        erika_catalog::MediaId::new().0
    ));
    std::fs::create_dir(&dir).unwrap();
    let store = Store::directory(&dir);
    for scope in [Scope::Export, Scope::Repository] {
        let db = Db::in_memory();
        db.set_setting("downloads.place", Some("local"));
        let mut device = Device::new(&db, &store, &NoVault, scope);
        for value in [Some(Value::text("remote")), None] {
            device
                .apply(
                    &[Op {
                        category: SETTINGS.into(),
                        key: "downloads.place".into(),
                        value: value.clone(),
                    }],
                    None,
                )
                .unwrap();
            let expected = if scope == Scope::Export {
                value.as_ref().and_then(Value::as_str)
            } else {
                Some("local")
            };
            assert_eq!(db.setting("downloads.place").as_deref(), expected);
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}
