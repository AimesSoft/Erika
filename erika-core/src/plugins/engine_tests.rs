//! A NipaPlay plugin, run in Boa through the engine as the app runs it.

use super::*;
use std::sync::{Mutex, OnceLock};
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
    assert!(extended.hex_input && extended.watched_button && extended.quick_select);
    // The automatic offset looks past Netflix's ident and the black after it.
    assert_eq!(extended.opening, 30.0);
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
    headers: { Referer: 'https://example.com/' },
  };
}
"#;

#[test]
fn a_plugin_resolves_a_link_in_rounds() {
    let folder = folder();
    let script = folder.join("resolver.js");
    std::fs::write(&script, RESOLVER).unwrap();
    let mut engine = ErikaEngine::default();
    let kernel = Arc::new(crate::player::tests::FakeKernel::default());
    engine.kernel = Some(kernel.clone());
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
        kernel.opens.lock().unwrap().as_slice(),
        &[("https://cdn.example.com/a.mp4".into(), 0., vec![("Referer".into(), "https://example.com/".into())])]
    );

    // Not its link: it plays as it is.
    erika::Commands::open_link(&mut engine, "https://other.example/v/clip.mp4".into()).unwrap();
    settle(&mut engine, |engine| !engine.state.link_resolving);
    assert_eq!(engine.state.title, "clip");
    engine.uninstall_plugin_command("test.resolver".into()).unwrap();
}

/// A kernel that keeps the comments it is given, by track.
#[derive(Default)]
struct Shown(Mutex<BTreeMap<String, Vec<String>>>);

impl crate::kernel::Kernel for Shown {
    fn ready(&self) -> bool {
        true
    }
    fn open(&self, _url: &str, _start: f64) {}
    fn play(&self) {}
    fn pause(&self) {}
    fn seek(&self, _seconds: f64) {}
    fn set_volume(&self, _volume: f64) {}
    fn set_rate(&self, _rate: f64) {}
    fn select_audio(&self, _id: i64) {}
    fn select_subtitle(&self, _id: Option<i64>) {}
    fn add_subtitle(&self, _url: &str) {}
    fn set_subtitle_scale(&self, _scale: f64) {}
    fn set_subtitle_supersampling(&self, _enabled: bool) {}
    fn set_enhancement(&self, _enhancement: Enhancement) {}
    fn set_danmaku(&self, _config: &crate::kernel::DanmakuConfig) {}
    fn add_danmaku(&self, name: &str, contents: &str) -> Result<usize, String> {
        let comments: Value = serde_json::from_str(contents).map_err(|e| e.to_string())?;
        let texts: Vec<String> = comments
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["content"].as_str().unwrap().to_string())
            .collect();
        let count = texts.len();
        self.0.lock().unwrap().insert(name.to_string(), texts);
        Ok(count)
    }
    fn send_danmaku(&self, _text: &str, _rgb: u32) {}
    fn capture(&self, _path: &std::path::Path, _width: Option<u32>) {}
    fn close(&self) {}
    fn playback(&self) -> crate::kernel::Playback {
        Default::default()
    }
}

const TRIM: &str = r#"
const pluginManifest = {
  id: 'test.trim', name: '去掉哈哈', version: '1.0.0', minHostVersion: '1.0.0',
  permissions: ['danmaku.modify'], priority: 10,
};
function pluginOnEvent(event) {
  if (event.name !== 'danmakuLoaded') return;
  var kept = event.data.danmaku.filter(function (c) { return c.content !== '哈哈哈'; });
  danmaku.replace({ count: kept.length, comments: kept });
}
"#;

const HIDE: &str = r#"
const pluginManifest = {
  id: 'test.hide', name: '屏蔽剧透', version: '1.0.0', minHostVersion: '1.0.0',
  permissions: ['danmaku.modify'], priority: 20,
};
var pluginBlockWords = ['剧透'];
"#;

#[test]
fn comments_pass_along_the_plugins_and_their_block_lists() {
    let folder = folder();
    for (name, script) in [("trim.js", TRIM), ("hide.js", HIDE)] {
        std::fs::write(folder.join(name), script).unwrap();
    }
    let mut engine = ErikaEngine::default();
    let shown = Arc::new(Shown::default());
    engine.kernel = Some(shown.clone());
    engine.state.video = true;
    engine.plugins_wake();
    engine.import_plugin_command(MediaSource::file(folder.join("trim.js"))).unwrap();
    engine.import_plugin_command(MediaSource::file(folder.join("hide.js"))).unwrap();
    settle(&mut engine, |engine| {
        ["test.trim", "test.hide"]
            .iter()
            .all(|id| row(engine, id).is_some_and(|p| p.loaded))
    });

    let kernel: Arc<dyn crate::kernel::Kernel> = shown.clone();
    let contents = r#"[{"time":1,"content":"好看"},{"time":2,"content":"哈哈哈"},{"time":3,"content":"前方剧透"}]"#;
    // The block list hides at once; the plugin's turn follows.
    assert_eq!(engine.load_danmaku(&kernel, "弹弹play", contents).unwrap(), 2);
    settle(&mut engine, |_| shown.0.lock().unwrap()["弹弹play"] == ["好看"]);

    // Turned off, what it took out comes back.
    engine.set_plugin_enabled_command("test.trim".into(), false).unwrap();
    assert_eq!(shown.0.lock().unwrap()["弹弹play"], ["好看", "哈哈哈"]);
    for id in ["test.trim", "test.hide"] {
        engine.uninstall_plugin_command(id.into()).unwrap();
    }
    assert_eq!(shown.0.lock().unwrap()["弹弹play"].len(), 3);
}

const OVERLAY: &str = r#"
const pluginManifest = {
  id: 'test.overlay', name: '画面叠加', version: '1.0.0', minHostVersion: '1.0.0',
  permissions: ['player.overlay'],
};
var pluginUIEntries = [
  { id: 'show', title: '显示', enabled: true, placement: 'danmakuMenu' },
  { id: 'color', title: '颜色', colorSetting: { default: 'red' }, placement: 'danmakuMenu' },
  { id: 'about', title: '关于' },
];
function apply() {
  var color = settings.getSwitch('show', true) ? settings.getText('color', 'red') : null;
  extend.progressStrip(color);
  extend.danmakuDensity(color);
}
function pluginOnInitialize() { apply(); }
function pluginHandleUIAction(id) { apply(); return null; }
"#;

#[test]
fn a_plugin_puts_its_settings_in_the_danmaku_menu_and_draws_on_the_picture() {
    let folder = folder();
    std::fs::write(folder.join("overlay.js"), OVERLAY).unwrap();
    let mut engine = ErikaEngine::default();
    let shown = Arc::new(Shown::default());
    engine.kernel = Some(shown.clone());
    engine.state.video = true;
    engine.state.duration = 10.0;
    engine.state.progress = 0.5;
    engine.plugins_wake();
    engine.import_plugin_command(MediaSource::file(folder.join("overlay.js"))).unwrap();
    settle(&mut engine, |engine| !engine.state.player.progress_strip.is_empty());

    // Its two settings are in the danmaku menu, the third on its page.
    let menu = &engine.state.player.plugin_menu;
    assert_eq!(menu.len(), 2);
    assert_eq!((menu[0].kind.clone(), menu[0].on), (PluginEntryKind::Switch, true));
    assert_eq!((menu[1].kind.clone(), menu[1].color.clone()), (PluginEntryKind::Color, PluginColor::Red));
    assert!(row(&engine, "test.overlay").unwrap().has_settings);
    assert_eq!(engine.state.player.progress_strip_color, PluginColor::Red);
    assert_eq!(engine.state.player.progress_strip[200], 1.0);
    assert_eq!(engine.state.player.progress_strip[201], 0.0);

    // The comments shown make the density chart.
    let kernel: Arc<dyn crate::kernel::Kernel> = shown.clone();
    let contents = r#"[{"time":1.5,"content":"好看"},{"time":1.6,"content":"好看"},{"time":8.5,"content":"再见"}]"#;
    engine.load_danmaku(&kernel, "弹弹play", contents).unwrap();
    assert_eq!(engine.state.player.danmaku_density.len(), 10);
    assert_eq!(engine.state.player.danmaku_density[1], 1.0);

    // The menu's choices reach the plugin, which draws accordingly.
    engine
        .set_plugin_menu_color_command("test.overlay".into(), "color".into(), PluginColor::Green)
        .unwrap();
    settle(&mut engine, |engine| engine.state.player.danmaku_density_color == PluginColor::Green);
    assert_eq!(engine.state.player.plugin_menu[1].color, PluginColor::Green);
    engine
        .set_plugin_menu_switch_command("test.overlay".into(), "show".into(), false)
        .unwrap();
    settle(&mut engine, |engine| engine.state.player.progress_strip.is_empty());
    assert!(engine.state.player.danmaku_density.is_empty());
    assert!(!engine.state.player.plugin_menu[0].on);
    assert!(
        engine
            .set_plugin_menu_switch_command("test.overlay".into(), "about".into(), true)
            .is_err()
    );

    engine.uninstall_plugin_command("test.overlay".into()).unwrap();
    assert!(engine.state.player.plugin_menu.is_empty());
}
