//! A NipaPlay plugin, run in Boa through the engine as the app runs it.

use super::*;
use std::sync::OnceLock;
use std::time::Duration;

const ECHO: &str = r#"
const pluginManifest = {
  id: 'test.echo',
  name: '回声',
  version: '1.0.0',
  minHostVersion: '9.9.9',
  description: '测试用插件',
  author: 'Erika',
  permissions: ['ui.dialog', 'danmaku.modify', 'storage'],
};
const pluginBlockWords = ['剧透', '刷屏/哈{5,}/'];
var pluginUIEntries = [
  { id: 'hello', title: '打招呼' },
  { id: 'loud', title: '大声', enabled: false },
  { id: 'name', title: '名字', textSetting: { hintText: '谁', default: '世界' } },
];
function pluginOnInitialize() {
  storage.set('count', (storage.get('count') || 0) + 1);
}
function pluginHandleUIAction(id) {
  if (id === 'hello') {
    return { type: 'text', title: '你好', content: '你好，' + settings.getText('name') + (settings.getSwitch('loud') ? '！！' : '。') };
  }
  return null;
}
function pluginOnEvent(event) {
  if (event.name === 'play') {
    ui.showSnackBar('开始了');
    danmaku.hide();
  }
}
"#;

/// Plugins install in a folder of the tests' own; no market is read.
fn folder() -> PathBuf {
    static FOLDER: OnceLock<PathBuf> = OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let folder = std::env::temp_dir().join(format!("erika-plugins-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::create_dir_all(&folder).unwrap();
            // SAFETY: set once, before any test reads them.
            unsafe {
                std::env::set_var(store::FOLDER_VARIABLE, &folder);
                std::env::set_var(store::INDEX_VARIABLE, "-");
            }
            folder
        })
        .clone()
}

/// Wake the engine until `done`, as the app's host does when evaluations
/// finish.
fn settle(engine: &mut ErikaEngine, done: impl Fn(&ErikaEngine) -> bool) {
    for _ in 0..1000 {
        engine.plugins_wake();
        if done(engine) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the plugin did not answer");
}

fn row<'a>(engine: &'a ErikaEngine, id: &str) -> Option<&'a Plugin> {
    engine.state.plugins.iter().find(|plugin| plugin.id == id)
}

#[test]
fn a_nipaplay_plugin_runs_as_it_did_there() {
    let folder = folder();
    let script = folder.join("echo.js");
    std::fs::write(&script, ECHO).unwrap();
    let mut engine = ErikaEngine::default();
    engine.plugins_wake();
    engine.import_plugin_command(MediaSource::file(&script)).unwrap();
    settle(&mut engine, |engine| row(engine, "test.echo").is_some_and(|p| p.loaded));

    // Imported, turned on and loaded, its version requirement dropped.
    let plugin = row(&engine, "test.echo").unwrap();
    assert!(plugin.installed && plugin.enabled && plugin.nipaplay && plugin.has_settings);
    assert!(plugin.unsupported.is_empty(), "{}", plugin.unsupported);
    assert_eq!(plugin.category, PluginCategory::Danmaku);
    assert!(engine.plugins.block_words().contains(&"剧透".to_string()));
    assert_eq!(
        engine.plugins.saved.storage["test.echo"]["count"], "1",
        "pluginOnInitialize ran with its storage"
    );

    // Its settings: a button, a switch and a text field.
    engine.open_plugin_command("test.echo".into()).unwrap();
    let kinds: Vec<_> = engine.state.plugin_panel.entries.iter().map(|e| e.kind.clone()).collect();
    assert_eq!(kinds, [PluginEntryKind::Action, PluginEntryKind::Switch, PluginEntryKind::Text]);
    assert_eq!(engine.state.plugin_panel.entries[2].text, "世界");
    assert_eq!(engine.state.plugin_panel.entries[2].hint, "谁");

    engine.set_plugin_text_command("name".into(), "Erika".into()).unwrap();
    engine.set_plugin_switch_command("loud".into(), true).unwrap();
    engine.plugin_action_command("hello".into()).unwrap();
    settle(&mut engine, |engine| engine.state.plugin_panel.has_message);
    assert_eq!(engine.state.plugin_panel.message_title, "你好");
    assert_eq!(engine.state.plugin_panel.message, "你好，Erika！！");

    // Events reach it, and what it does comes back.
    assert!(engine.state.preferences.show_danmaku);
    engine.plugin_event("play", json!({}));
    settle(&mut engine, |engine| !engine.state.preferences.show_danmaku);
    assert!(matches!(
        &engine.state.notice,
        Some(Notice::PluginSays { name, message }) if name == "回声" && message == "开始了"
    ));

    // Turned off, it stops; uninstalled, its folder goes.
    engine.set_plugin_enabled_command("test.echo".into(), false).unwrap();
    assert!(!engine.plugins.block_words().contains(&"剧透".to_string()));
    engine.uninstall_plugin_command("test.echo".into()).unwrap();
    assert!(!folder.join("test.echo").exists());
    assert!(row(&engine, "test.echo").is_none());
}

/// Every plugin of NipaPlay's real market installs and loads. It reads the
/// network: `cargo test -p erika-core real_market -- --ignored --nocapture`.
#[test]
#[ignore]
fn the_real_market_installs_and_loads() {
    folder();
    let (listings, errors) = store::read_index(store::NIPAPLAY_INDEX)
        .map(|listings| (listings, Vec::<String>::new()))
        .unwrap();
    assert!(errors.is_empty() && !listings.is_empty());
    let mut engine = ErikaEngine::default();
    engine.plugins_wake();
    engine.plugins.listings = listings.clone();
    for listing in &listings {
        engine.install_plugin_command(listing.pack.id.clone()).unwrap();
    }
    settle(&mut engine, |engine| {
        engine.plugins.busy.is_empty()
            && engine.plugins.installed.len() == listings.len()
            && engine.plugins.installed.iter().all(|p| {
                p.loaded || !p.error.is_empty() || !p.pack.unsupported().is_empty()
            })
    });
    for plugin in &engine.plugins.installed {
        println!(
            "{} {} — {}",
            plugin.pack.id,
            plugin.pack.version,
            if !plugin.pack.unsupported().is_empty() {
                format!("unsupported: {:?}", plugin.pack.unsupported())
            } else if plugin.loaded {
                format!("loaded, {} entries, {} block words", plugin.entries.len(), plugin.block_words.len())
            } else {
                format!("failed: {}", plugin.error)
            }
        );
    }
}

/// Erika's own plugins install from an index of folders and load:
/// `ERIKA_PLUGINS_REPO=…/erika_plugins cargo test -p erika-core own_plugins -- --ignored --nocapture`.
#[test]
#[ignore]
fn erikas_own_plugins_install_and_load() {
    folder();
    let repo = std::env::var("ERIKA_PLUGINS_REPO").expect("ERIKA_PLUGINS_REPO");
    let listings = store::read_index(&format!("{repo}/plugins.json")).unwrap();
    assert!(listings.iter().all(|listing| matches!(listing.files, store::Files::Folder(_))));
    let mut engine = ErikaEngine::default();
    engine.plugins_wake();
    engine.plugins.listings = listings.clone();
    for listing in &listings {
        engine.install_plugin_command(listing.pack.id.clone()).unwrap();
    }
    settle(&mut engine, |engine| {
        engine.plugins.busy.is_empty()
            && listings.iter().all(|listing| {
                engine
                    .plugins
                    .find(&listing.pack.id)
                    .is_some_and(|p| p.loaded || !p.error.is_empty())
            })
    });
    for listing in &listings {
        let plugin = engine.plugins.find(&listing.pack.id).unwrap();
        let row = row(&engine, &listing.pack.id).unwrap();
        println!(
            "{} — loaded {} entries {} image {} error {:?}",
            plugin.pack.id,
            plugin.loaded,
            plugin.entries.len(),
            row.image.is_some(),
            plugin.error
        );
        assert!(plugin.loaded, "{}: {}", plugin.pack.id, plugin.error);
        assert!(row.image.is_some(), "{} shows its icon.png", plugin.pack.id);
    }
    let extended = engine.plugins.extensions();
    println!("{extended:?}");
    assert!(extended.speeds.contains(&5.0) && extended.speeds.contains(&0.25));
    assert!(extended.hex_input && extended.watched_button && extended.quick_select && extended.opening);
    assert_eq!(extended.colors.len(), 9);
    assert!(engine.state.speeds.iter().any(|speed| speed.label == "5.0×"));
    assert!(engine.state.speeds.iter().any(|speed| speed.label == "0.25×"));
    assert!(engine.state.download_quick_select);
}

const RESOLVER: &str = r#"
const pluginManifest = {
  id: 'test.resolver', name: '解析', version: '1.0.0', minHostVersion: '1.0.0',
  permissions: ['url.resolve'],
};
function pluginResolveUrl(input) {
  if (!input.state) {
    if (input.url.indexOf('https://example.com/watch/') !== 0) return null;
    return {
      type: 'select', title: '选集',
      items: [{ id: 'a', title: '第一集' }, { id: 'b', title: '第二集' }],
      preferredId: 'b', state: { step: 'picked' },
    };
  }
  return {
    type: 'play', url: 'https://cdn.example.com/' + input.selectedId + '.mp4',
    title: input.selectedId === 'a' ? '第一集' : '第二集',
    headers: { Referer: 'https://example.com/', Host: 'evil' },
  };
}
"#;

#[test]
fn a_plugin_resolves_a_link_in_rounds() {
    let folder = folder();
    let script = folder.join("resolver.js");
    std::fs::write(&script, RESOLVER).unwrap();
    let mut engine = ErikaEngine::default();
    engine.plugins_wake();
    engine.import_plugin_command(MediaSource::file(&script)).unwrap();
    settle(&mut engine, |engine| row(engine, "test.resolver").is_some_and(|p| p.loaded));

    erika::Commands::open_link(&mut engine, "https://example.com/watch/1".into()).unwrap();
    settle(&mut engine, |engine| engine.state.link_choosing);
    assert_eq!(engine.state.link_choice_title, "选集");
    assert_eq!(engine.state.link_choices.len(), 2);
    assert!(engine.state.link_choices[1].preferred);

    engine.link_chosen("a".into());
    settle(&mut engine, |engine| !engine.state.link_resolving);
    assert_eq!(engine.state.title, "第一集");
    assert_eq!(
        crate::kernel::link_headers("https://cdn.example.com/a.mp4"),
        vec![("Referer".to_string(), "https://example.com/".to_string())]
    );

    // Not its link: it plays as it is.
    erika::Commands::open_link(&mut engine, "https://other.example/v/clip.mp4".into()).unwrap();
    settle(&mut engine, |engine| !engine.state.link_resolving);
    assert_eq!(engine.state.title, "clip");
    engine.uninstall_plugin_command("test.resolver".into()).unwrap();
}
