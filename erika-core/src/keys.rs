//! Keyboard shortcuts (「设置 › 快捷键」): every action's keys, kept in the
//! settings, each pressed differently from every other action's. Keys an
//! action has are refused to another, with a notice naming the one that
//! has them; the menu bar's own actions count only where the menu bar
//! shows, on the Mac.
use super::*;

/// Each action, its name in the settings, and its keys by default, as Bern
/// writes shortcuts.
const ACTIONS: &[(KeyAction, &str, &str)] = &[
    (KeyAction::PlayPause, "play_pause", "space"),
    (KeyAction::SeekBack, "seek_back", "left"),
    (KeyAction::SeekForward, "seek_forward", "right"),
    (KeyAction::Fullscreen, "fullscreen", "f"),
    (KeyAction::ToggleDanmaku, "toggle_danmaku", "d"),
    (KeyAction::Screenshot, "screenshot", "cmd+s"),
    (KeyAction::Back, "back", "escape"),
    (KeyAction::Confirm, "confirm", "return"),
    (KeyAction::OpenFile, "open_file", "cmd+o"),
    (KeyAction::Search, "search", "cmd+k"),
    (KeyAction::Settings, "settings", "cmd+,"),
    (KeyAction::SmallerPosters, "smaller_posters", "cmd+-"),
    (KeyAction::LargerPosters, "larger_posters", "cmd+="),
    (KeyAction::OpenFolder, "open_folder", "cmd+shift+o"),
    (KeyAction::OpenLink, "open_link", "cmd+l"),
    (KeyAction::PreviousEpisode, "previous_episode", "cmd+["),
    (KeyAction::NextEpisode, "next_episode", "cmd+]"),
];

/// Whether this is an Apple system, where Command and Control are two keys.
const APPLE: bool = cfg!(any(target_os = "macos", target_os = "ios"));

/// Whether the action has keys here: the menu bar's own only on the Mac,
/// the one system whose menu bar shows them.
fn here(action: &KeyAction) -> bool {
    cfg!(target_os = "macos")
        || !matches!(
            action,
            KeyAction::OpenFolder
                | KeyAction::OpenLink
                | KeyAction::PreviousEpisode
                | KeyAction::NextEpisode
        )
}

fn keys_of<'a>(keys: &'a KeyBindings, action: &KeyAction) -> &'a String {
    match action {
        KeyAction::PlayPause => &keys.play_pause,
        KeyAction::SeekBack => &keys.seek_back,
        KeyAction::SeekForward => &keys.seek_forward,
        KeyAction::Fullscreen => &keys.fullscreen,
        KeyAction::ToggleDanmaku => &keys.toggle_danmaku,
        KeyAction::Screenshot => &keys.screenshot,
        KeyAction::Back => &keys.back,
        KeyAction::Confirm => &keys.confirm,
        KeyAction::OpenFile => &keys.open_file,
        KeyAction::Search => &keys.search,
        KeyAction::Settings => &keys.settings,
        KeyAction::SmallerPosters => &keys.smaller_posters,
        KeyAction::LargerPosters => &keys.larger_posters,
        KeyAction::OpenFolder => &keys.open_folder,
        KeyAction::OpenLink => &keys.open_link,
        KeyAction::PreviousEpisode => &keys.previous_episode,
        KeyAction::NextEpisode => &keys.next_episode,
    }
}

fn keys_of_mut<'a>(keys: &'a mut KeyBindings, action: &KeyAction) -> &'a mut String {
    match action {
        KeyAction::PlayPause => &mut keys.play_pause,
        KeyAction::SeekBack => &mut keys.seek_back,
        KeyAction::SeekForward => &mut keys.seek_forward,
        KeyAction::Fullscreen => &mut keys.fullscreen,
        KeyAction::ToggleDanmaku => &mut keys.toggle_danmaku,
        KeyAction::Screenshot => &mut keys.screenshot,
        KeyAction::Back => &mut keys.back,
        KeyAction::Confirm => &mut keys.confirm,
        KeyAction::OpenFile => &mut keys.open_file,
        KeyAction::Search => &mut keys.search,
        KeyAction::Settings => &mut keys.settings,
        KeyAction::SmallerPosters => &mut keys.smaller_posters,
        KeyAction::LargerPosters => &mut keys.larger_posters,
        KeyAction::OpenFolder => &mut keys.open_folder,
        KeyAction::OpenLink => &mut keys.open_link,
        KeyAction::PreviousEpisode => &mut keys.previous_episode,
        KeyAction::NextEpisode => &mut keys.next_episode,
    }
}

/// The name an action's keys are saved under.
fn setting(action: &KeyAction) -> String {
    let name = ACTIONS
        .iter()
        .find(|(each, ..)| each == action)
        .map_or("", |(_, name, _)| name);
    format!("keys.{name}")
}

/// Every action's keys by default.
pub(crate) fn defaults() -> KeyBindings {
    let mut keys = KeyBindings {
        play_pause: String::new(),
        seek_back: String::new(),
        seek_forward: String::new(),
        fullscreen: String::new(),
        toggle_danmaku: String::new(),
        screenshot: String::new(),
        back: String::new(),
        confirm: String::new(),
        open_file: String::new(),
        search: String::new(),
        settings: String::new(),
        smaller_posters: String::new(),
        larger_posters: String::new(),
        open_folder: String::new(),
        open_link: String::new(),
        previous_episode: String::new(),
        next_episode: String::new(),
    };
    for (action, _, default) in ACTIONS {
        *keys_of_mut(&mut keys, action) = (*default).into();
    }
    keys
}

/// Keys as this computer writes them: ⌘O, ⇧⌘O and ␣ on Apple's systems,
/// Ctrl+O elsewhere; empty for none.
pub(crate) fn display(keys: &str) -> String {
    let Ok(normal) = bern::shortcut::parse_shortcut(keys) else {
        return String::new();
    };
    let mut parts: Vec<&str> = normal.split('+').collect();
    let key = parts.pop().unwrap_or_default();
    let named = |apple: &'static str, other: &'static str| if APPLE { apple } else { other };
    let shown = match key {
        "space" => named("␣", "Space").to_string(),
        "escape" => named("⎋", "Esc").to_string(),
        "return" => named("↩", "Enter").to_string(),
        "tab" => named("⇥", "Tab").to_string(),
        "delete" => named("⌫", "Backspace").to_string(),
        "up" => "↑".into(),
        "down" => "↓".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        "home" => named("↖", "Home").to_string(),
        "end" => named("↘", "End").to_string(),
        "pageup" => named("⇞", "PgUp").to_string(),
        "pagedown" => named("⇟", "PgDn").to_string(),
        other => other.to_uppercase(),
    };
    if APPLE {
        let mut text = String::new();
        for (part, symbol) in [("ctrl", "⌃"), ("alt", "⌥"), ("shift", "⇧"), ("cmd", "⌘")] {
            if parts.contains(&part) {
                text.push_str(symbol);
            }
        }
        return text + &shown;
    }
    let mut words: Vec<String> = Vec::new();
    if parts.contains(&"cmd") || parts.contains(&"ctrl") {
        words.push("Ctrl".into());
    }
    if parts.contains(&"alt") {
        words.push("Alt".into());
    }
    if parts.contains(&"shift") {
        words.push("Shift".into());
    }
    words.push(shown);
    words.join("+")
}

impl ErikaEngine {
    /// The keys saved last, over the defaults; keys that are no shortcut
    /// any more are left out.
    pub(super) fn restore_shortcuts(&mut self) {
        let mut keys = defaults();
        for (action, ..) in ACTIONS {
            let Some(saved) = self.db.setting(&setting(action)) else {
                continue;
            };
            if saved.is_empty() {
                keys_of_mut(&mut keys, action).clear();
            } else if let Ok(normal) = bern::shortcut::parse_shortcut(&saved) {
                *keys_of_mut(&mut keys, action) = normal;
            }
        }
        self.state.keys = keys;
        self.publish_shortcuts();
    }

    /// The action other than `action` whose keys `keys` are pressed with.
    fn holder(&self, keys: &str, action: &KeyAction) -> Option<KeyAction> {
        if keys.is_empty() {
            return None;
        }
        ACTIONS
            .iter()
            .map(|(other, ..)| other)
            .filter(|other| *other != action && here(other))
            .find(|other| bern::shortcut::same_keys(keys_of(&self.state.keys, other), keys, APPLE))
            .cloned()
    }

    pub(super) fn change_shortcut(
        &mut self,
        action: KeyAction,
        keys: String,
    ) -> Result<(), CommandError> {
        let keys = if keys.trim().is_empty() {
            String::new()
        } else {
            bern::shortcut::parse_shortcut(&keys)
                .map_err(|message| CommandError::new("invalid_shortcut", message))?
        };
        if *keys_of(&self.state.keys, &action) == keys {
            return Ok(());
        }
        if let Some(other) = self.holder(&keys, &action) {
            self.notify(Notice::ShortcutTaken {
                keys: display(&keys),
                action: other,
            });
            return Ok(());
        }
        *keys_of_mut(&mut self.state.keys, &action) = keys.clone();
        // The default is saved as no setting, so a later default follows.
        let default = keys_of(&self.state.default_keys, &action) == &keys;
        self.db
            .set_setting(&setting(&action), (!default).then_some(keys.as_str()));
        self.publish_shortcuts();
        Ok(())
    }

    pub(super) fn reset_shortcut_of(&mut self, action: KeyAction) -> Result<(), CommandError> {
        let default = keys_of(&self.state.default_keys, &action).clone();
        self.change_shortcut(action, default)
    }

    pub(super) fn reset_all_shortcuts(&mut self) {
        for (action, ..) in ACTIONS {
            self.db.set_setting(&setting(action), None);
        }
        self.state.keys = defaults();
        self.publish_shortcuts();
        self.notify(Notice::ShortcutsReset);
    }

    fn publish_shortcuts(&mut self) {
        self.state.default_keys = defaults();
        self.state.keys_changed = self.state.keys != self.state.default_keys;
        self.state.open_key = display(&self.state.keys.open_file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use erika::Commands;

    #[test]
    fn every_action_has_distinct_keys_by_default() {
        let keys = defaults();
        for (index, (action, name, default)) in ACTIONS.iter().enumerate() {
            assert_eq!(keys_of(&keys, action), default, "{name}");
            assert_eq!(
                bern::shortcut::parse_shortcut(default).as_deref(),
                Ok(*default),
                "{name} is written as Bern writes shortcuts"
            );
            for (other, other_name, other_default) in &ACTIONS[index + 1..] {
                assert!(
                    !bern::shortcut::same_keys(default, other_default, false)
                        && !bern::shortcut::same_keys(default, other_default, true),
                    "{name} and {other_name} share keys"
                );
                assert_ne!(action, other);
            }
        }
    }

    #[test]
    fn shortcuts_change_refuse_keys_another_action_has_and_reset() {
        let mut engine = ErikaEngine::default();
        assert_eq!(engine.state.keys, defaults());
        assert!(!engine.state.keys_changed);
        engine
            .set_shortcut(KeyAction::PlayPause, "Shift + K".into())
            .unwrap();
        assert_eq!(engine.state.keys.play_pause, "shift+k");
        assert!(engine.state.keys_changed);
        // ⌘O opens files: playing keeps its keys, and a notice says why.
        let revision = engine.state.notice_revision;
        engine
            .set_shortcut(KeyAction::PlayPause, "cmd+o".into())
            .unwrap();
        assert_eq!(engine.state.keys.play_pause, "shift+k");
        assert_eq!(engine.state.notice_revision, revision + 1);
        assert_eq!(
            engine.state.notice,
            Some(Notice::ShortcutTaken {
                keys: display("cmd+o"),
                action: KeyAction::OpenFile,
            })
        );
        assert_eq!(
            engine
                .set_shortcut(KeyAction::PlayPause, "cmd+f13".into())
                .unwrap_err()
                .code,
            "invalid_shortcut"
        );
        // Opening files takes other keys; its old ones are free again.
        engine
            .set_shortcut(KeyAction::OpenFile, "cmd+p".into())
            .unwrap();
        engine
            .set_shortcut(KeyAction::Search, "cmd+o".into())
            .unwrap();
        assert_eq!(engine.state.keys.search, "cmd+o");
        assert_eq!(engine.state.open_key, display("cmd+p"));
        // Its default is the search's now: resetting it is refused.
        engine.reset_shortcut(KeyAction::OpenFile).unwrap();
        assert_eq!(engine.state.keys.open_file, "cmd+p");
        // No keys at all are for no action.
        engine
            .set_shortcut(KeyAction::Confirm, String::new())
            .unwrap();
        engine.set_shortcut(KeyAction::Back, " ".into()).unwrap();
        assert_eq!(engine.state.keys.confirm, "");
        assert_eq!(engine.state.keys.back, "");
        engine.reset_shortcuts().unwrap();
        assert_eq!(engine.state.keys, defaults());
        assert!(!engine.state.keys_changed);
        assert_eq!(engine.state.notice, Some(Notice::ShortcutsReset));
    }

    #[test]
    fn shortcuts_are_kept_until_the_next_start() {
        let dir = std::env::temp_dir().join(format!("erika-keys-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("erika.db");
        let _ = std::fs::remove_file(&path);
        {
            let mut engine = ErikaEngine::default();
            engine.db = erika_store::Db::at(&path).unwrap();
            engine
                .set_shortcut(KeyAction::Fullscreen, "cmd+ctrl+f".into())
                .unwrap();
            engine
                .set_shortcut(KeyAction::Screenshot, String::new())
                .unwrap();
        }
        let mut engine = ErikaEngine::default();
        engine.db = erika_store::Db::at(&path).unwrap();
        engine.restore_shortcuts();
        assert_eq!(engine.state.keys.fullscreen, "cmd+ctrl+f");
        assert_eq!(engine.state.keys.screenshot, "");
        assert_eq!(engine.state.keys.play_pause, "space");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keys_read_as_this_computer_writes_them() {
        if APPLE {
            assert_eq!(display("cmd+shift+o"), "⇧⌘O");
            assert_eq!(display("cmd+,"), "⌘,");
            assert_eq!(display("space"), "␣");
        } else {
            assert_eq!(display("cmd+shift+o"), "Ctrl+Shift+O");
            assert_eq!(display("escape"), "Esc");
        }
        assert_eq!(display(""), "");
    }
}
