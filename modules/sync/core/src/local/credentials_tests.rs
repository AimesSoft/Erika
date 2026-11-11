use super::*;
use crate::{export, import, repository::Keys};

const SOURCE_SECRET: &str = "source_secrets";
const TOKEN: &str = "fake-source-token-do-not-export-plain";
const PLUGIN_SECRET: &str = "fake-plugin-cookie-do-not-export-plain";

struct Fixture {
    dir: std::path::PathBuf,
    store: Store,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("erika-credentials-{}", erika_catalog::MediaId::new().0));
        std::fs::create_dir(&dir).unwrap();
        Self { store: Store::directory(&dir), dir }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) { std::fs::remove_dir_all(&self.dir).unwrap(); }
}
fn keys(passphrase: Option<&str>) -> Keys {
    let mut keys = Keys::new(passphrase.map(String::from));
    keys.rounds = Some(1);
    keys
}
fn setting(id: i64) -> String { format!("library.dandanplay.{id}.token") }
fn seed(db: &Db) -> i64 {
    let id = db.add_source("dandanplay", "书房", "https://source.example.invalid", "movie").unwrap();
    db.set_setting(&setting(id), Some(TOKEN));
    db.set_extension("plugins", Some(&serde_json::json!({
        "enabled":["test.source"], "text":{"test.source":{"password":PLUGIN_SECRET}},
        "storage":{"test.source":{"cookies":PLUGIN_SECRET}}, "switches":{}, "downloader":false,
    }).to_string()));
    id
}
fn no_plain_secrets(state: &State) {
    let text = format!("{state:?}");
    assert!(!text.contains(TOKEN), "source token escaped its sealed category");
    assert!(!text.contains(PLUGIN_SECRET), "plugin data escaped its sealed category");
    assert!(!text.contains("library.dandanplay."), "a device-local credential key was exported");
}

#[test]
fn credentials_never_leave_as_generic_settings_or_plain_extension_data() {
    let fixture = Fixture::new();
    let db = Db::in_memory();
    seed(&db);
    let key = SealKey::create("fake passphrase", 1);
    for scope in [Scope::Export, Scope::Repository] {
        let device = Device::new(&db, &fixture.store, &NoVault, scope);
        for key in [None, Some(&key)] {
            let state = device.collect(key).unwrap();
            no_plain_secrets(&state);
            if key.is_none() {
                assert!(!state.contains_key(SOURCE_SECRET));
                assert!(!state.contains_key(EXTENSIONS));
            }
        }
    }
}

#[test]
fn protected_backup_rebinds_source_credentials_to_the_receiving_source_id() {
    let fixture = Fixture::new();
    let sender = Db::in_memory();
    let from = seed(&sender);
    let bytes = export(&sender, &fixture.store, &NoVault, &mut keys(Some("fake passphrase")), "sender").unwrap();
    no_plain_secrets(&crate::format::read(&bytes).unwrap().state.unwrap());
    let receiver = Db::in_memory();
    let other = receiver.add_source("dandanplay", "另一台电脑", "https://other.example.invalid", "movie").unwrap();
    receiver.set_setting(&setting(other), Some("local-only-other-source"));
    let imported = import(&bytes, &receiver, &fixture.store, &NoVault, &mut keys(Some("fake passphrase"))).unwrap();
    assert_eq!(imported.applied.locked, 0);
    let target = receiver.sources().into_iter().find(|s| s.location == "https://source.example.invalid").unwrap().id;
    assert_ne!(from, target);
    assert_eq!(imported.applied.rescan_sources, vec![target]);
    assert!(imported.applied.extensions);
    assert_eq!(receiver.setting(&setting(target)).as_deref(), Some(TOKEN));
    assert_eq!(receiver.setting(&setting(other)).as_deref(), Some("local-only-other-source"));
    assert_eq!(receiver.extension("plugins"), sender.extension("plugins"));
    let again = import(&bytes, &receiver, &fixture.store, &NoVault, &mut keys(Some("fake passphrase"))).unwrap();
    assert_eq!(again.applied.changes, 0);
}

#[test]
fn missing_or_wrong_passphrase_preserves_existing_private_records() {
    let fixture = Fixture::new();
    let sender = Db::in_memory();
    seed(&sender);
    for sender_pass in [None, Some("fake passphrase")] {
        let bytes = export(&sender, &fixture.store, &NoVault, &mut keys(sender_pass), "sender").unwrap();
        for receiver_pass in [None, Some("wrong passphrase")] {
            let receiver = Db::in_memory();
            let id = seed(&receiver);
            receiver.set_setting(&setting(id), Some("receiver-credential"));
            receiver.set_extension("plugins", Some(r#"{"storage":{"local":"keep-me"}}"#));
            let imported = import(&bytes, &receiver, &fixture.store, &NoVault, &mut keys(receiver_pass)).unwrap();
            assert_eq!(receiver.setting(&setting(id)).as_deref(), Some("receiver-credential"));
            assert_eq!(receiver.extension("plugins").as_deref(), Some(r#"{"storage":{"local":"keep-me"}}"#));
            assert_eq!(imported.wrong_passphrase, sender_pass.is_some() && receiver_pass.is_some());
        }
    }
}

#[test]
fn private_records_cannot_be_injected_as_plaintext_or_reassigned_to_another_owner() {
    let fixture = Fixture::new();
    let db = Db::in_memory();
    let id = seed(&db);
    let other = db.add_source("dandanplay", "别处", "https://other.example.invalid", "movie").unwrap();
    db.set_setting(&setting(other), Some("other-credential"));
    let key = SealKey::create("fake passphrase", 1);
    let mut device = Device::new(&db, &fixture.store, &NoVault, Scope::Export);
    let before = device.collect(Some(&key)).unwrap();
    for value in [Some(Value::text("forged")), None] {
        device.apply(&[Op { category: SETTINGS.into(), key: setting(id), value }], Some(&key)).unwrap();
        assert_eq!(db.setting(&setting(id)).as_deref(), Some(TOKEN));
    }
    for (category, record, field) in [
        (SOURCE_SECRET, "dandanplay\nhttps://source.example.invalid", "token"),
        (EXTENSIONS, "plugins", "data"),
    ] {
        let plain = Op { category: category.into(), key: record.into(), value: Some(Value::map([(field, Value::text("unsealed"))])) };
        assert!(device.apply(&[plain], Some(&key)).is_err());
        let swapped = Op { category: category.into(), key: if category == EXTENSIONS { "another-plugin".into() } else { "dandanplay\nhttps://other.example.invalid".into() }, value: Some(before[category][record].clone()) };
        assert!(device.apply(&[swapped], Some(&key)).is_err());
    }
    assert_eq!(device.applied.locked, 0); // Failed drafts publish no state or counters.
    assert_eq!(db.setting(&setting(other)).as_deref(), Some("other-credential"));
    assert!(db.extension("another-plugin").is_none());
    assert_eq!(device.collect(Some(&key)).unwrap(), before);
    for category in [SOURCE_SECRET, EXTENSIONS] {
        let record = if category == SOURCE_SECRET { "dandanplay\nhttps://source.example.invalid" } else { "plugins" };
        device.apply(&[Op { category: category.into(), key: record.into(), value: None }], None).unwrap();
    }
    assert_eq!(device.collect(Some(&key)).unwrap(), before);
}

#[test]
fn source_deletion_forgets_its_local_credential_before_another_source_reuses_the_id() {
    let fixture = Fixture::new();
    let db = Db::in_memory();
    let id = seed(&db);
    let key = SealKey::create("fake passphrase", 1);
    let mut device = Device::new(&db, &fixture.store, &NoVault, Scope::Repository);
    device.apply(&[
        Op { category: SOURCES.into(), key: "dandanplay\nhttps://source.example.invalid".into(), value: None },
        Op { category: SOURCE_SECRET.into(), key: "dandanplay\nhttps://source.example.invalid".into(), value: None },
    ], Some(&key)).unwrap();
    assert!(db.setting(&setting(id)).is_none());
    let next = db.add_source("dandanplay", "新来源", "https://new.example.invalid", "movie").unwrap();
    assert!(db.setting(&setting(next)).is_none());
    assert!(!device.collect(Some(&key)).unwrap().contains_key(SOURCE_SECRET));
}

#[test]
fn unopenable_extension_data_rolls_back_other_database_changes() {
    let fixture = Fixture::new();
    let db = Db::in_memory();
    seed(&db);
    let key = SealKey::create("fake passphrase", 1);
    let mut device = Device::new(&db, &fixture.store, &NoVault, Scope::Export);
    let before = device.collect(Some(&key)).unwrap();
    let invalid = Value::map([("data", Value::Sealed(key.seal("extension\nplugins", b"not json")))]);
    assert!(device.apply(&[
        Op { category: SETTINGS.into(), key: "autoplay".into(), value: Some(Value::text("false")) },
        Op { category: EXTENSIONS.into(), key: "plugins".into(), value: Some(invalid) },
    ], Some(&key)).is_err());
    assert_eq!(device.collect(Some(&key)).unwrap(), before);
}

fn webdav(db: &Db, vault: &dyn Vault, host: &str, password: &str) {
    db.set_setting("sync.webdav.url", Some(&format!("https://{host}.example.invalid/dav")));
    db.set_setting("sync.webdav.user", Some(host));
    db.set_setting("sync.webdav.path", Some("/Erika"));
    db.set_setting("sync.auto", Some("false"));
    assert!(vault.write(WEBDAV_PASSWORD, password));
}

#[test]
fn webdav_credentials_and_destination_restore_as_one_sealed_record() {
    let fixture = Fixture::new();
    let sender = Db::in_memory();
    let sender_vault = MemoryVault::default();
    for password in ["fake-sender-dav-password", ""] {
        webdav(&sender, &sender_vault, "sender", password);
        let bytes = export(&sender, &fixture.store, &sender_vault, &mut keys(Some("fake passphrase")), "sender").unwrap();
        let state = crate::format::read(&bytes).unwrap().state.unwrap();
        assert!(matches!(state[SYNC]["webdav"].get("data"), Some(Value::Sealed(_))));
        assert!(!format!("{state:?}").contains("sender.example.invalid"));
        assert!(!format!("{state:?}").contains("fake-sender-dav-password"));
        for pass in [None, Some("wrong passphrase"), Some("fake passphrase")] {
            let receiver = Db::in_memory();
            let receiver_vault = MemoryVault::default();
            webdav(&receiver, &receiver_vault, "receiver", "fake-receiver-password");
            let imported = import(&bytes, &receiver, &fixture.store, &receiver_vault, &mut keys(pass)).unwrap();
            let opened = pass == Some("fake passphrase");
            assert_eq!(receiver.setting("sync.webdav.user").as_deref(), Some(if opened { "sender" } else { "receiver" }));
            assert_eq!(receiver.setting("sync.webdav.url").as_deref(), Some(if opened { "https://sender.example.invalid/dav" } else { "https://receiver.example.invalid/dav" }));
            assert_eq!(receiver_vault.read(WEBDAV_PASSWORD).as_deref(), Some(if opened { password } else { "fake-receiver-password" }));
            assert_eq!(imported.applied.sync, opened);
            if opened {
                let again = import(&bytes, &receiver, &fixture.store, &receiver_vault, &mut keys(pass)).unwrap();
                assert_eq!(again.applied.changes, 0);
            }
        }
        let plain = export(&sender, &fixture.store, &sender_vault, &mut keys(None), "sender").unwrap();
        assert!(!crate::format::read(&plain).unwrap().state.unwrap().contains_key(SYNC));
        let repository = Device::new(&sender, &fixture.store, &sender_vault, Scope::Repository);
        assert!(!repository.collect(Some(&SealKey::create("fake passphrase", 1))).unwrap().contains_key(SYNC));
    }
}

#[test]
fn webdav_plaintext_corruption_and_rejected_vault_writes_keep_the_local_pair() {
    struct RejectWrites(MemoryVault);
    impl Vault for RejectWrites {
        fn read(&self, name: &str) -> Option<String> { self.0.read(name) }
        fn write(&self, _: &str, _: &str) -> bool { false }
        fn delete(&self, _: &str) { panic!("a failed restore must not delete credentials") }
    }
    let fixture = Fixture::new();
    let sender = Db::in_memory();
    let sender_vault = MemoryVault::default();
    webdav(&sender, &sender_vault, "sender", "fake-sender-password");
    let key = SealKey::create("fake passphrase", 1);
    let record = Device::new(&sender, &fixture.store, &sender_vault, Scope::Export).collect(Some(&key)).unwrap()[SYNC]["webdav"].clone();
    let receiver = Db::in_memory();
    let receiver_vault = RejectWrites(MemoryVault::default());
    webdav(&receiver, &receiver_vault.0, "receiver", "fake-receiver-password");
    let mut device = Device::new(&receiver, &fixture.store, &receiver_vault, Scope::Export);
    let before = device.collect(Some(&key)).unwrap();
    for value in [
        record.clone(),
        Value::map([("url", Value::text("https://forged.example.invalid")), ("password", Value::text("plaintext"))]),
        Value::map([("data", Value::Sealed(key.seal("sync-webdav-configuration", b"not json")))]),
        Value::map([("data", Value::Sealed(key.seal("another-purpose", b"{}")))]),
    ] {
        assert!(device.apply(&[Op { category: SYNC.into(), key: "webdav".into(), value: Some(value) }], Some(&key)).is_err());
        assert_eq!(device.collect(Some(&key)).unwrap(), before);
        assert!(!device.applied.sync);
        assert_eq!(device.applied.changes, 0);
    }
    device.apply(&[Op { category: SYNC.into(), key: "webdav".into(), value: Some(record) }], None).unwrap();
    assert_eq!(device.collect(Some(&key)).unwrap(), before);
}

#[test]
fn repository_preserves_sealed_categories_while_locked_and_can_restore_them_later() {
    use crate::repository::{Base, MemoryTransport, Outcome, Sync};
    let fixture = Fixture::new();
    let sender = Db::in_memory();
    seed(&sender);
    let receiver = Db::in_memory();
    receiver.add_source("folder", "本机目录", "file:///different", "movie").unwrap();
    let mut transport = MemoryTransport::default();
    let mut run = |db: &Db, pass: Option<&str>, base: Option<Base>, name: &str| -> (Outcome, Base) {
        let mut local = Device::new(db, &fixture.store, &NoVault, Scope::Repository);
        Sync {
            transport: &mut transport, local: &mut local, keys: &mut keys(pass),
            root: "/protected".into(), place: "fixture".into(), device: name.into(), now: 100,
            categories: CATEGORIES.iter().map(|s| s.to_string()).collect(),
        }.run(base).unwrap()
    };
    let (_, first) = run(&sender, Some("fake passphrase"), None, "sender");
    no_plain_secrets(&first.state);
    let (locked, base) = run(&receiver, Some("wrong passphrase"), None, "receiver");
    assert!(locked.key_mismatch);
    assert_eq!(locked.uploaded, 0);
    assert_eq!(base.state[SOURCE_SECRET], first.state[SOURCE_SECRET]);
    assert_eq!(base.state[EXTENSIONS], first.state[EXTENSIONS]);
    assert!(receiver.extension("plugins").is_none());
    let base = Base::from_bytes(&base.to_bytes()).unwrap();
    assert!(!base.sealed_available);
    let (opened, base) = run(&receiver, Some("fake passphrase"), Some(base), "receiver");
    assert_eq!(opened.uploaded, 0);
    assert_eq!(receiver.extension("plugins"), sender.extension("plugins"));
    let id = receiver.sources().into_iter().find(|s| s.kind == "dandanplay").unwrap().id;
    assert_eq!(receiver.setting(&setting(id)).as_deref(), Some(TOKEN));
    let (again, _) = run(&receiver, Some("fake passphrase"), Some(base), "receiver");
    assert_eq!((again.uploaded, again.restored), (0, 0));
}
