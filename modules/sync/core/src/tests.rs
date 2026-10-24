use super::*;
use crate::format::{Op, State};
use crate::local::{MemoryVault, Vault};
use crate::repository::{Local, MemoryTransport, Sync, Transport};
use crate::seal::SealKey;
use erika_store::Fingerprint;
use erika_store::portable::{FileKey, Mark, Watched};
use std::collections::BTreeMap;

/// Few rounds: tests need the shape of the key, not its slowness.
const TEST_ROUNDS: u32 = 64;

fn keys(passphrase: Option<&str>) -> Keys {
    let mut keys = Keys::new(passphrase.map(String::from));
    keys.rounds = Some(TEST_ROUNDS);
    keys
}

fn state(entries: &[(&str, &str, Value)]) -> State {
    let mut state = State::new();
    for (category, key, value) in entries {
        state
            .entry(category.to_string())
            .or_default()
            .insert(key.to_string(), value.clone());
    }
    state
}

fn timed(at: i64, position: f64) -> Value {
    Value::map([("position", Value::Float(position)), ("at", Value::Int(at))])
}

/// A store in a folder of its own.
fn store(name: &str) -> (Store, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("erika-sync-test-{name}-{}", random_hex(4)));
    std::fs::create_dir_all(&dir).unwrap();
    (Store::directory(&dir), dir)
}

#[test]
fn every_kind_of_value_reads_back_as_written() {
    let value = Value::map([
        ("null", Value::Null),
        ("yes", Value::Bool(true)),
        ("no", Value::Bool(false)),
        ("small", Value::Int(-1)),
        ("big", Value::Int(i64::MIN)),
        ("max", Value::Int(i64::MAX)),
        ("float", Value::Float(-0.0)),
        ("nan", Value::Float(f64::NAN)),
        ("text", Value::text("葬送的芙莉莲 · Frieren")),
        ("bytes", Value::Bytes(vec![0, 255, 7])),
        ("list", Value::List(vec![Value::Int(1), Value::text("二")])),
        ("sealed", Value::Sealed(vec![1, 2, 3])),
        (
            "nested",
            Value::map([("deep", Value::List(vec![Value::Null]))]),
        ),
    ]);
    let bytes = value::encode(&value);
    assert_eq!(value::decode(&bytes).unwrap(), value);
    // Cut anywhere, it is refused rather than misread.
    for end in 0..bytes.len() {
        assert!(value::decode(&bytes[..end]).is_err(), "{end}");
    }
}

#[test]
fn json_goes_in_and_comes_out_the_same() {
    let json: serde_json::Value = serde_json::from_str(
        r#"{"enabled":["regex","words"],"regex":{"rules":["^前方高能$"],"case":false},"weight":0.5,"n":3,"none":null}"#,
    )
    .unwrap();
    let value = Value::from_json(&json);
    assert_eq!(
        value::decode(&value::encode(&value)).unwrap().to_json(),
        json
    );
}

#[test]
fn a_document_reads_back_and_damage_is_caught() {
    let mut document = Document::new(Kind::Export).with_head("device", Value::text("mac"));
    let mut big = BTreeMap::new();
    for n in 0..300 {
        big.insert(format!("file{n}"), timed(n, n as f64));
    }
    document.state = Some(BTreeMap::from([("progress".to_string(), big)]));
    document.ops = Some(vec![Op {
        category: "settings".into(),
        key: "autoplay".into(),
        value: None,
    }]);
    let bytes = format::write(&document);
    // The body is large enough to be deflated.
    assert_eq!(bytes[11] & 1, 1);
    assert_eq!(format::read(&bytes).unwrap(), document);

    let mut flipped = bytes.clone();
    flipped[40] ^= 1;
    assert!(matches!(
        format::read(&flipped),
        Err(format::FormatError::Damaged(_))
    ));
    assert_eq!(
        format::read(b"{\"version\":2}"),
        Err(format::FormatError::NotErika)
    );
    let mut newer = bytes.clone();
    newer[8] = 2;
    assert_eq!(format::read(&newer), Err(format::FormatError::Newer(2)));
}

#[test]
fn sections_a_later_version_adds_are_skipped() {
    let document = Document::new(Kind::Patch).with_head("id", Value::text("p1"));
    let bytes = format::write(&document);
    // Rebuild the file with an unknown section 99 after the head.
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let mut body = bytes[16..16 + length].to_vec();
    body.extend_from_slice(&[99, 3, 1, 2, 3]);
    let mut rebuilt = bytes[..12].to_vec();
    rebuilt.extend_from_slice(&(body.len() as u32).to_le_bytes());
    rebuilt.extend_from_slice(&body);
    let digest = format::sha256(&rebuilt);
    rebuilt.extend_from_slice(&digest);
    assert_eq!(format::read(&rebuilt).unwrap(), document);
}

#[test]
fn sealed_secrets_open_only_with_their_passphrase_and_purpose() {
    let key = SealKey::create("correct horse", TEST_ROUNDS);
    let sealed = key.seal("dandanplay-token", b"token-123");
    assert!(!sealed.windows(9).any(|window| window == b"token-123"));
    // The same secret seals alike: syncing sees no change.
    assert_eq!(key.seal("dandanplay-token", b"token-123"), sealed);
    assert_eq!(
        key.open_sealed("dandanplay-token", &sealed).unwrap(),
        b"token-123"
    );
    assert_eq!(key.open_sealed("bangumi-cookies", &sealed), None);
    let mut changed = sealed.clone();
    *changed.last_mut().unwrap() ^= 1;
    assert_eq!(key.open_sealed("dandanplay-token", &changed), None);

    let again = SealKey::open("correct horse", key.params()).unwrap();
    assert_eq!(
        again.open_sealed("dandanplay-token", &sealed).unwrap(),
        b"token-123"
    );
    assert!(SealKey::open("wrong horse", key.params()).is_none());
}

#[test]
fn pbkdf2_matches_the_standard_vectors() {
    // RFC 7914, section 11.
    let mut out = [0u8; 64];
    seal::pbkdf2(b"passwd", b"salt", 1, &mut out);
    assert_eq!(
        format::hex(&out),
        "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc49ca9cccf179b645991664b39d77ef317c71b845b1e30bd509112041d3a19783"
    );
}

#[test]
fn diff_and_apply_rebuild_the_target() {
    let before = state(&[
        ("settings", "same", Value::text("1")),
        ("settings", "changed", Value::text("1")),
        ("settings", "removed", Value::text("old")),
    ]);
    let after = state(&[
        ("settings", "same", Value::text("1")),
        ("settings", "changed", Value::text("2")),
        ("settings", "added", Value::text("new")),
        ("marks", "17675", timed(5, 0.0)),
    ]);
    let ops = merge::diff(&before, &after);
    assert_eq!(ops.len(), 4);
    let mut rebuilt = before.clone();
    merge::apply(&mut rebuilt, ops);
    assert_eq!(rebuilt, after);
}

#[test]
fn a_first_sync_keeps_both_sides_and_the_later_record() {
    let remote = state(&[
        ("settings", "autoplay", Value::text("false")),
        ("progress", "a:1", timed(100, 10.0)),
        ("progress", "b:1", timed(300, 30.0)),
    ]);
    let local = state(&[
        ("settings", "autoplay", Value::text("true")),
        ("settings", "local", Value::text("x")),
        ("progress", "a:1", timed(200, 20.0)),
        ("progress", "b:1", timed(250, 25.0)),
    ]);
    let merged = merge::merge_first(&remote, &local);
    assert_eq!(merged["settings"]["autoplay"], Value::text("false"));
    assert_eq!(merged["settings"]["local"], Value::text("x"));
    assert_eq!(merged["progress"]["a:1"], timed(200, 20.0));
    assert_eq!(merged["progress"]["b:1"], timed(300, 30.0));
}

#[test]
fn three_way_merge_keeps_both_devices_changes() {
    let base = state(&[
        ("settings", "a", Value::text("0")),
        ("settings", "b", Value::text("0")),
        ("settings", "c", Value::text("0")),
        ("progress", "f:1", timed(10, 1.0)),
        ("recents", "https://x/1.mp4", timed(10, 1.0)),
    ]);
    let remote = state(&[
        ("settings", "a", Value::text("remote")),
        ("settings", "b", Value::text("0")),
        ("settings", "c", Value::text("remote")),
        ("progress", "f:1", timed(30, 3.0)),
    ]);
    let local = state(&[
        ("settings", "a", Value::text("0")),
        ("settings", "b", Value::text("local")),
        ("settings", "c", Value::text("local")),
        ("progress", "f:1", timed(20, 2.0)),
        ("recents", "https://x/1.mp4", timed(10, 1.0)),
        ("marks", "1", timed(5, 0.0)),
    ]);
    let merged = merge::merge_three(&base, &remote, &local);
    assert_eq!(merged["settings"]["a"], Value::text("remote"));
    assert_eq!(merged["settings"]["b"], Value::text("local"));
    // Both changed an untimed value: the device syncing now wins.
    assert_eq!(merged["settings"]["c"], Value::text("local"));
    // Both changed a timed record: the later wins.
    assert_eq!(merged["progress"]["f:1"], timed(30, 3.0));
    // Removed elsewhere, untouched here: removed.
    assert!(
        !merged
            .get("recents")
            .is_some_and(|r| r.contains_key("https://x/1.mp4"))
    );
    assert_eq!(merged["marks"]["1"], timed(5, 0.0));
}

/// A device for repository tests: its state in memory.
struct Memory {
    state: State,
    applied: Vec<Op>,
}

impl Local for Memory {
    fn collect(&mut self, key: Option<&SealKey>) -> State {
        let mut state = self.state.clone();
        if let (Some(key), Some(accounts)) = (key, state.get_mut(local::ACCOUNTS)) {
            for value in accounts.values_mut() {
                if let Value::Text(secret) = value {
                    *value = Value::Sealed(key.seal("test", secret.as_bytes()));
                }
            }
        } else {
            state.remove(local::ACCOUNTS);
        }
        state
    }
    fn apply(&mut self, ops: &[Op], key: Option<&SealKey>) {
        let opened = ops.iter().map(|op| Op {
            value: match &op.value {
                Some(Value::Sealed(bytes)) => key
                    .and_then(|key| key.open_sealed("test", bytes))
                    .map(|text| Value::Text(String::from_utf8(text).unwrap())),
                other => other.clone(),
            },
            ..op.clone()
        });
        let opened: Vec<Op> = opened.collect();
        self.applied.extend(opened.iter().cloned());
        merge::apply(&mut self.state, opened);
    }
}

fn all_categories() -> std::collections::BTreeSet<String> {
    local::CATEGORIES
        .iter()
        .map(|category| category.to_string())
        .collect()
}

fn sync_once(
    transport: &mut MemoryTransport,
    device: &mut Memory,
    keys: &mut Keys,
    name: &str,
    now: i64,
    base: Option<Base>,
) -> (Outcome, Base) {
    Sync {
        transport,
        local: device,
        keys,
        root: "/Erika/sync".into(),
        place: "test".into(),
        device: name.into(),
        now,
        categories: all_categories(),
    }
    .run(base)
    .unwrap()
}

#[test]
fn two_devices_sync_through_a_repository() {
    let mut server = MemoryTransport::default();
    let mut mac = Memory {
        state: state(&[
            ("settings", "autoplay", Value::text("false")),
            ("accounts", "dandanplay", Value::text("token-mac")),
            ("progress", "a:1", timed(100, 10.0)),
        ]),
        applied: Vec::new(),
    };
    let mut phone = Memory {
        state: state(&[("progress", "b:2", timed(150, 15.0))]),
        applied: Vec::new(),
    };
    let mut mac_keys = keys(Some("shared secret"));
    let mut phone_keys = keys(Some("shared secret"));

    let (outcome, mac_base) = sync_once(&mut server, &mut mac, &mut mac_keys, "mac", 1000, None);
    assert!(outcome.created);
    assert!(server.files.contains_key("/Erika/sync/manifest.erika"));
    // The token is sealed on the server.
    assert!(
        !server
            .files
            .values()
            .any(|bytes| bytes.windows(9).any(|w| w == b"token-mac"))
    );

    let (outcome, phone_base) = sync_once(
        &mut server,
        &mut phone,
        &mut phone_keys,
        "phone",
        1100,
        None,
    );
    assert_eq!(outcome.uploaded, 1);
    assert_eq!(
        phone.state["accounts"]["dandanplay"],
        Value::text("token-mac")
    );
    assert_eq!(phone.state["settings"]["autoplay"], Value::text("false"));
    assert_eq!(phone.state["progress"]["a:1"], timed(100, 10.0));

    // The phone watches on; the Mac changes a setting.
    phone
        .state
        .get_mut("progress")
        .unwrap()
        .insert("a:1".into(), timed(1200, 50.0));
    let (_, phone_base) = sync_once(
        &mut server,
        &mut phone,
        &mut phone_keys,
        "phone",
        1200,
        Some(phone_base),
    );
    mac.state
        .get_mut("settings")
        .unwrap()
        .insert("autoplay".into(), Value::text("true"));
    let (outcome, mac_base) = sync_once(
        &mut server,
        &mut mac,
        &mut mac_keys,
        "mac",
        1300,
        Some(mac_base),
    );
    assert_eq!(outcome.downloaded, 2);
    assert_eq!(mac.state["progress"]["a:1"], timed(1200, 50.0));
    assert_eq!(mac.state["progress"]["b:2"], timed(150, 15.0));

    let (_, _) = sync_once(
        &mut server,
        &mut phone,
        &mut phone_keys,
        "phone",
        1400,
        Some(phone_base),
    );
    assert_eq!(phone.state["settings"]["autoplay"], Value::text("true"));
    // Nothing new: a sync uploads nothing.
    let (outcome, _) = sync_once(
        &mut server,
        &mut mac,
        &mut mac_keys,
        "mac",
        1500,
        Some(mac_base),
    );
    assert_eq!((outcome.uploaded, outcome.restored), (0, 0));
}

#[test]
fn another_passphrase_leaves_the_accounts_alone() {
    let mut server = MemoryTransport::default();
    let mut mac = Memory {
        state: state(&[("accounts", "dandanplay", Value::text("token-mac"))]),
        applied: Vec::new(),
    };
    sync_once(
        &mut server,
        &mut mac,
        &mut keys(Some("one")),
        "mac",
        1,
        None,
    );
    let mut other = Memory {
        state: state(&[("accounts", "dandanplay", Value::text("token-other"))]),
        applied: Vec::new(),
    };
    let (outcome, _) = sync_once(
        &mut server,
        &mut other,
        &mut keys(Some("two")),
        "other",
        2,
        None,
    );
    assert!(outcome.key_mismatch);
    assert_eq!(
        other.state["accounts"]["dandanplay"],
        Value::text("token-other")
    );
    let mut none = Memory {
        state: State::new(),
        applied: Vec::new(),
    };
    let (outcome, _) = sync_once(&mut server, &mut none, &mut keys(None), "none", 3, None);
    assert!(outcome.no_passphrase);
    assert!(!none.state.contains_key("accounts"));
    // The Mac still reads its own token back.
    let mut fresh = Memory {
        state: State::new(),
        applied: Vec::new(),
    };
    sync_once(
        &mut server,
        &mut fresh,
        &mut keys(Some("one")),
        "fresh",
        4,
        None,
    );
    assert_eq!(
        fresh.state["accounts"]["dandanplay"],
        Value::text("token-mac")
    );
}

#[test]
fn a_patch_listed_by_no_manifest_is_still_found() {
    let mut server = MemoryTransport::default();
    let mut mac = Memory {
        state: state(&[("settings", "a", Value::text("1"))]),
        applied: Vec::new(),
    };
    let mut keys_mac = keys(None);
    let (_, base) = sync_once(&mut server, &mut mac, &mut keys_mac, "mac", 1, None);
    // Another device wrote its patch but lost the race for the manifest.
    let manifest = server.files["/Erika/sync/manifest.erika"].clone();
    let mut phone = Memory {
        state: state(&[("settings", "b", Value::text("2"))]),
        applied: Vec::new(),
    };
    sync_once(&mut server, &mut phone, &mut keys(None), "phone", 2, None);
    server
        .files
        .insert("/Erika/sync/manifest.erika".into(), manifest);
    sync_once(&mut server, &mut mac, &mut keys_mac, "mac", 3, Some(base));
    assert_eq!(mac.state["settings"]["b"], Value::text("2"));
}

#[test]
fn many_patches_are_compacted_into_a_snapshot() {
    let mut server = MemoryTransport::default();
    let mut device = Memory {
        state: State::new(),
        applied: Vec::new(),
    };
    let mut device_keys = keys(None);
    let mut base = None;
    for n in 0..140 {
        device
            .state
            .entry("settings".into())
            .or_default()
            .insert("n".into(), Value::Int(n));
        let (_, next) = sync_once(&mut server, &mut device, &mut device_keys, "mac", n, base);
        base = Some(next);
    }
    let base = base.unwrap();
    assert!(base.snapshot >= 3, "{}", base.snapshot);
    let names: Vec<&String> = server.files.keys().collect();
    assert!(
        !names.iter().any(|name| name.ends_with("snap_v1.erika")),
        "{names:?}"
    );
    // A new device rebuilds the same state from what is left.
    let mut fresh = Memory {
        state: State::new(),
        applied: Vec::new(),
    };
    sync_once(&mut server, &mut fresh, &mut keys(None), "fresh", 999, None);
    assert_eq!(fresh.state["settings"]["n"], Value::Int(139));
    let _ = server.list("/Erika/sync");
}

fn fill(db: &Db, store: &Store, vault: &dyn Vault) {
    db.set_setting("autoplay", Some("false"));
    db.set_setting("danmaku.opacity", Some("0.6"));
    db.set_setting("device.id", Some("mac-id"));
    db.set_setting("downloads.place", Some("/Users/me/Downloads"));
    store
        .write(
            "welcome-v1",
            b"erika-welcome-2\naccount,library\nNeon\nDark\n",
        )
        .unwrap();
    db.add_source("folder", "番剧", "file:///Volumes/Anime/", "anime");
    db.add_source(
        "nipaplay",
        "家里的 NipaPlay",
        "http://192.168.1.5:1180",
        "anime",
    );
    let file = db
        .opened(
            "file:///Volumes/Anime/frieren-04.mkv",
            Some(&Fingerprint {
                md5: "3f2a".into(),
                size: 1000,
            }),
        )
        .unwrap();
    db.link_episode(
        file,
        &erika_store::Anime {
            dandanplay_id: 17675,
            title: "葬送的芙莉莲".into(),
            image: "https://img/1.jpg".into(),
        },
        &erika_store::Episode {
            dandanplay_id: 176750004,
            number: 4,
            title: "第4话".into(),
        },
        true,
    );
    db.save_progress(file, 600.0, 1440.0);
    db.remember(
        "file:///Volumes/Anime/frieren-04.mkv",
        "frieren-04",
        Some("/cache/1.png"),
        600.0,
        1440.0,
    );
    db.remember("https://example.com/kyoto.mp4", "kyoto", None, 30.0, 300.0);
    db.remember(
        "http://127.0.0.1:49268/torrent/2/stream/8/a.webm",
        "streaming",
        None,
        3.0,
        0.0,
    );
    db.set_anime_mark(&Mark {
        anime: 17675,
        status: Some("watching".into()),
        rating: 9,
        at: 50,
    });
    // One watched episode with its id, one only by its number.
    for (episode, number) in [(Some(176750004), 4), (None, 5)] {
        db.put_watched(&Watched {
            episode,
            anime: 17675,
            number: Some(number),
            at: 60,
            manual: true,
        });
    }
    db.set_extension(
        "plugins",
        Some(r#"{"enabled":["regex"],"regex":{"rules":["^前方高能$"]}}"#),
    );
    vault.write("dandanplay-token", "token-123");
    db.set_setting("dandanplay.user", Some("nipa@qq.com"));
    db.set_setting("dandanplay.name", Some("Nipa"));
    vault.write("bangumi-cookies", "chii_auth=abc");
    db.set_setting("bangumi.user", Some("42"));
    db.set_setting("bangumi.name", Some("Bangumi 用户"));
    db.set_setting("sync.webdav.url", Some("https://dav.example.com/dav"));
    db.set_setting("sync.webdav.user", Some("me"));
    vault.write("sync-webdav-password", "dav-pass");
}

#[test]
fn an_export_brings_everything_to_a_clean_device() {
    let (old_store, old_dir) = store("old");
    let old_db = Db::in_memory();
    let old_vault = MemoryVault::default();
    fill(&old_db, &old_store, &old_vault);
    let bytes = export(
        &old_db,
        &old_store,
        &old_vault,
        &mut keys(Some("passphrase")),
        "mac-id",
    );
    assert!(!bytes.windows(9).any(|w| w == b"token-123"));

    let (new_store, new_dir) = store("new");
    let new_db = Db::in_memory();
    let new_vault = MemoryVault::default();
    let imported = import(
        &bytes,
        &new_db,
        &new_store,
        &new_vault,
        &mut keys(Some("passphrase")),
    )
    .unwrap();
    assert!(!imported.wrong_passphrase && !imported.no_passphrase);
    assert_eq!(imported.applied.locked, 0);
    assert!(imported.applied.accounts && imported.applied.settings && imported.applied.library);
    assert_eq!(imported.applied.new_sources.len(), 2);

    assert_eq!(new_db.setting("autoplay").as_deref(), Some("false"));
    assert_eq!(new_db.setting("danmaku.opacity").as_deref(), Some("0.6"));
    // The device keeps its own id.
    assert_eq!(new_db.setting("device.id"), None);
    assert_eq!(
        new_store.read("welcome-v1").unwrap().unwrap(),
        old_store.read("welcome-v1").unwrap().unwrap()
    );
    assert_eq!(
        new_vault.read("dandanplay-token").as_deref(),
        Some("token-123")
    );
    assert_eq!(new_db.setting("dandanplay.name").as_deref(), Some("Nipa"));
    assert_eq!(
        new_vault.read("bangumi-cookies").as_deref(),
        Some("chii_auth=abc")
    );
    assert_eq!(
        new_vault.read("sync-webdav-password").as_deref(),
        Some("dav-pass")
    );
    assert_eq!(
        new_db.setting("sync.webdav.url").as_deref(),
        Some("https://dav.example.com/dav")
    );
    let sources = new_db.sources();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[1].name, "家里的 NipaPlay");
    // The same file, found anywhere, is the episode and resumes there.
    let file = new_db
        .file_id(&Fingerprint {
            md5: "3f2a".into(),
            size: 1000,
        })
        .unwrap();
    assert_eq!(new_db.progress(file), Some((600.0, 1440.0)));
    let (anime, episode) = new_db.file_episode(file).unwrap();
    assert_eq!((anime.title.as_str(), episode.number), ("葬送的芙莉莲", 4));
    let recents = new_db.recents(5);
    assert_eq!(recents.len(), 3);
    assert_eq!(recents[1].title, "kyoto");
    // Its picture was this device's cache.
    assert!(recents.iter().all(|recent| recent.still.is_none()));
    assert_eq!(new_db.anime_marks(), old_db.anime_marks());
    assert_eq!(new_db.anime_marks()[0].status.as_deref(), Some("watching"));
    let watched = |db: &Db| {
        let mut keys: Vec<String> = db.portable_watched().iter().map(Watched::key).collect();
        keys.sort();
        keys
    };
    assert_eq!(watched(&new_db), ["17675#5", "176750004"]);
    let plugins: serde_json::Value =
        serde_json::from_str(&new_db.extension("plugins").unwrap()).unwrap();
    assert_eq!(plugins["regex"]["rules"][0], "^前方高能$");

    // Importing it again changes nothing.
    let again = import(
        &bytes,
        &new_db,
        &new_store,
        &new_vault,
        &mut keys(Some("passphrase")),
    )
    .unwrap();
    assert_eq!(again.applied.changes, 0);
    let _ = std::fs::remove_dir_all(old_dir);
    let _ = std::fs::remove_dir_all(new_dir);
}

#[test]
fn an_export_without_the_passphrase_brings_all_but_the_accounts() {
    let (old_store, old_dir) = store("locked-old");
    let old_db = Db::in_memory();
    let old_vault = MemoryVault::default();
    fill(&old_db, &old_store, &old_vault);
    let bytes = export(
        &old_db,
        &old_store,
        &old_vault,
        &mut keys(Some("passphrase")),
        "mac-id",
    );
    for passphrase in [None, Some("another one")] {
        let (new_store, new_dir) = store("locked-new");
        let new_db = Db::in_memory();
        let new_vault = MemoryVault::default();
        let imported = import(
            &bytes,
            &new_db,
            &new_store,
            &new_vault,
            &mut keys(passphrase),
        )
        .unwrap();
        assert_eq!(imported.no_passphrase, passphrase.is_none());
        assert_eq!(imported.wrong_passphrase, passphrase.is_some());
        assert_eq!(new_vault.read("dandanplay-token"), None);
        assert_eq!(new_vault.read("sync-webdav-password"), None);
        assert_eq!(new_db.setting("autoplay").as_deref(), Some("false"));
        assert_eq!(new_db.sources().len(), 2);
        let _ = std::fs::remove_dir_all(new_dir);
    }
    // Without a passphrase on the exporting device no secret is written.
    let plain = export(&old_db, &old_store, &old_vault, &mut keys(None), "mac-id");
    let document = format::read(&plain).unwrap();
    assert!(document.key.is_none());
    assert!(!document.state.unwrap().contains_key(local::ACCOUNTS));
    let _ = std::fs::remove_dir_all(old_dir);
}

#[test]
fn a_repository_leaves_this_devices_folders_and_files_out() {
    let (store, dir) = store("scope");
    let db = Db::in_memory();
    let vault = MemoryVault::default();
    fill(&db, &store, &vault);
    let state = Device::new(&db, &store, &vault, Scope::Repository).collect(None);
    assert_eq!(state[local::SOURCES].len(), 1);
    assert_eq!(
        state[local::RECENTS].keys().collect::<Vec<_>>(),
        ["https://example.com/kyoto.mp4"]
    );
    assert!(!state[local::SETTINGS].contains_key("downloads.place"));
    assert!(!state.contains_key(local::SYNC));
    assert!(
        state[local::PROGRESS].contains_key(
            &FileKey::Hash {
                md5: "3f2a".into(),
                size: 1000
            }
            .text()
        )
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn export_names_and_times_read_as_dates() {
    assert_eq!(civil(0), (1970, 1, 1));
    assert_eq!(civil(20_750), (2026, 10, 24));
    let name = export_name(bern::date::Now {
        unix: 1_792_800_000 + 3_723,
        offset: 8 * 3600,
    });
    assert_eq!(name, "erika-backup-20261024-090203.erika");
}

#[test]
fn the_webdav_form_keeps_what_is_left_empty() {
    use sync::{Commands, Engine};
    let mut engine = SyncEngine::default();
    assert!(!engine.state().configured);
    assert_eq!(
        engine
            .set_webdav("ftp://x".into(), "u".into(), String::new(), String::new())
            .unwrap_err()
            .code,
        "invalid_url"
    );
    engine
        .set_webdav(
            "https://dav.example.com/dav/".into(),
            "me".into(),
            "pw".into(),
            "Erika/备份/".into(),
        )
        .unwrap();
    let state = engine.state();
    assert!(state.configured && !state.editing);
    assert_eq!(
        (state.url.as_str(), state.path.as_str()),
        ("https://dav.example.com/dav", "/Erika/备份")
    );
    engine.edit_webdav(true).unwrap();
    engine
        .set_webdav(String::new(), "other".into(), String::new(), String::new())
        .unwrap();
    let state = engine.state();
    assert_eq!(
        (state.url.as_str(), state.user.as_str(), state.path.as_str()),
        ("https://dav.example.com/dav", "other", "/Erika/备份")
    );
    assert!(!state.editing);
    assert_eq!(
        engine.set_passphrase("short".into()).unwrap_err().code,
        "passphrase_short"
    );
    engine.clear_webdav().unwrap();
    assert!(!engine.state().configured);
    assert_eq!(engine.state().path, DEFAULT_PATH);
}
