//! What a plugin says it is: the `pack.json` of an Erika plugin, or the
//! `pluginManifest` of a NipaPlay script. An Erika plugin may also keep its
//! declaration in the script, as NipaPlay's do; `pack.json` wins.

use erika_protocol::erika::{PluginCategory, PluginIcon, PluginPermission};
use serde_json::{Value, json};

/// A plugin's declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct Pack {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub github: String,
    /// The Erika version it needs. NipaPlay's plugins name NipaPlay's
    /// versions, which Erika drops until it has a market of its own.
    pub min_host: Option<String>,
    /// Permission ids, as declared.
    pub permissions: Vec<String>,
    pub category: PluginCategory,
    pub icon: PluginIcon,
    /// The icon's file in the plugin's folder.
    pub image: Option<String>,
    /// The script's file in the plugin's folder.
    pub main: String,
    /// Smaller goes first where plugins take turns.
    pub priority: i64,
    /// HTTPS scripts to run before the plugin's own.
    pub requires: Vec<External>,
    pub nipaplay: bool,
}

/// A script a plugin needs from elsewhere (`script.external`).
#[derive(Debug, Clone, PartialEq)]
pub struct External {
    pub id: String,
    pub url: String,
}

fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

fn texts(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

impl Pack {
    /// An Erika plugin's `pack.json`.
    pub fn from_pack(value: &Value) -> Result<Pack, String> {
        let mut pack = Pack::common(value)?;
        pack.min_host = Some(text(value, "minHostVersion")).filter(|v| !v.is_empty());
        let category = text(value, "category");
        pack.category = category_named(&category).ok_or_else(|| {
            format!("pack.json 的 category 必须是 danmaku、picture、tools 或 appearance，而不是「{category}」")
        })?;
        let image = text(value, "icon");
        pack.image = (!image.is_empty()).then_some(image);
        pack.icon = icon_named(&text(value, "iconId")).unwrap_or_else(|| guess_icon(&pack));
        let main = text(value, "main");
        if !main.is_empty() {
            pack.main = main;
        }
        Ok(pack)
    }

    /// A NipaPlay script's `pluginManifest`, its version requirement dropped.
    pub fn from_manifest(value: &Value) -> Result<Pack, String> {
        let mut pack = Pack::common(value)?;
        pack.nipaplay = true;
        pack.category = guess_category(&pack.permissions);
        pack.icon = guess_icon(&pack);
        Ok(pack)
    }

    fn common(value: &Value) -> Result<Pack, String> {
        let (id, name, version) = (text(value, "id"), text(value, "name"), text(value, "version"));
        if id.is_empty() || name.is_empty() || version.is_empty() {
            return Err("插件声明缺少 id、name 或 version".into());
        }
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return Err(format!("插件 id「{id}」只能有字母、数字、点、下划线和连字符"));
        }
        let requires = value
            .get("requires")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .map(|item| External {
                        id: text(item, "id"),
                        url: text(item, "url"),
                    })
                    .filter(|external| external.url.starts_with("https://"))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Pack {
            id,
            name,
            version,
            description: text(value, "description"),
            author: text(value, "author"),
            github: text(value, "github"),
            min_host: None,
            permissions: texts(value, "permissions"),
            category: PluginCategory::Tools,
            icon: PluginIcon::Puzzle,
            image: None,
            main: "main.js".into(),
            priority: value.get("priority").and_then(Value::as_i64).unwrap_or(50),
            requires,
            nipaplay: false,
        })
    }

    /// The declaration as `pack.json` holds it; NipaPlay's plugins keep
    /// theirs this way once installed.
    pub fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "version": self.version,
            "description": self.description,
            "author": self.author,
            "github": self.github,
            "minHostVersion": self.min_host,
            "permissions": self.permissions,
            "category": category_name(&self.category),
            "icon": self.image,
            "iconId": icon_name(&self.icon),
            "main": self.main,
            "priority": self.priority,
            "nipaplay": self.nipaplay,
        })
    }

    /// The plugin as `pack.json` keeps it once installed.
    pub fn from_installed(value: &Value) -> Result<Pack, String> {
        if value.get("nipaplay").and_then(Value::as_bool) == Some(true) {
            let mut pack = Pack::from_manifest(value)?;
            pack.main = text(value, "main");
            return Ok(pack);
        }
        Pack::from_pack(value)
    }

    pub fn has(&self, permission: &str) -> bool {
        self.permissions.iter().any(|p| p == permission)
    }

    /// The permissions Erika knows, as the market tags them.
    pub fn tags(&self) -> Vec<PluginPermission> {
        self.permissions.iter().filter_map(|id| permission(id)).collect()
    }

    /// What it needs that Erika cannot give, in words for the user.
    pub fn unsupported(&self) -> Vec<&'static str> {
        self.permissions
            .iter()
            .filter_map(|id| match id.as_str() {
                // A web view over the video: Erika's controls are the
                // system's own, with nothing to put one in.
                "danmaku.renderer" => Some("WebView 弹幕渲染层"),
                _ => None,
            })
            .collect()
    }

    /// Whether this version of Erika is new enough for it.
    pub fn fits(&self, host: &str) -> bool {
        self.min_host
            .as_deref()
            .is_none_or(|needed| !newer(needed, host))
    }
}

/// Whether version `a` is newer than `b`, comparing dotted numbers.
pub fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split(['.', '-', '+'])
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parts(a), parts(b));
    for index in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(index).copied().unwrap_or(0),
            b.get(index).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

pub fn permission(id: &str) -> Option<PluginPermission> {
    Some(match id {
        "url.resolve" => PluginPermission::UrlResolve,
        "player.control" => PluginPermission::PlayerControl,
        "danmaku.modify" => PluginPermission::ModifyDanmaku,
        "danmaku.renderer" => PluginPermission::DanmakuRenderer,
        "script.external" => PluginPermission::ExternalScript,
        "library.read" => PluginPermission::LibraryRead,
        "library.write" => PluginPermission::LibraryWrite,
        "library.sources" => PluginPermission::LibrarySources,
        "ui.dialog" => PluginPermission::Dialog,
        "settings.read" => PluginPermission::SettingsRead,
        "settings.modify" => PluginPermission::SettingsModify,
        "storage" => PluginPermission::Storage,
        "system.override" => PluginPermission::SystemOverride,
        "network" => PluginPermission::Network,
        "player.overlay" => PluginPermission::PlayerOverlay,
        _ => return None,
    })
}

pub fn category_named(name: &str) -> Option<PluginCategory> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "danmaku" | "弹幕" => PluginCategory::Danmaku,
        "picture" | "画面" => PluginCategory::Picture,
        "tools" | "tool" | "工具" => PluginCategory::Tools,
        "appearance" | "外观" => PluginCategory::Appearance,
        _ => return None,
    })
}

fn category_name(category: &PluginCategory) -> &'static str {
    match category {
        PluginCategory::Danmaku => "danmaku",
        PluginCategory::Picture => "picture",
        PluginCategory::Appearance => "appearance",
        PluginCategory::Tools | PluginCategory::All => "tools",
    }
}

/// NipaPlay's plugins name no category: the danmaku ones are danmaku, the
/// others tools.
fn guess_category(permissions: &[String]) -> PluginCategory {
    if permissions.iter().any(|p| p.starts_with("danmaku.")) {
        PluginCategory::Danmaku
    } else {
        PluginCategory::Tools
    }
}

fn guess_icon(pack: &Pack) -> PluginIcon {
    if pack.has("url.resolve") {
        PluginIcon::Link
    } else if pack.has("system.override") {
        PluginIcon::Gear
    } else {
        match pack.category {
            PluginCategory::Danmaku => PluginIcon::Bubble,
            PluginCategory::Picture => PluginIcon::Tv,
            PluginCategory::Appearance => PluginIcon::Palette,
            _ => PluginIcon::Puzzle,
        }
    }
}

/// The symbols plugins may name, by the names Erika's controls use.
const ICONS: &[(&str, PluginIcon)] = &[
    ("puzzlepiece.extension", PluginIcon::Puzzle),
    ("eye.slash", PluginIcon::EyeSlash),
    ("line.3.horizontal.decrease", PluginIcon::Filter),
    ("shield", PluginIcon::Shield),
    ("tv", PluginIcon::Tv),
    ("scissors", PluginIcon::Scissors),
    ("arrow.up.right.square", PluginIcon::External),
    ("music.note", PluginIcon::Music),
    ("paintpalette", PluginIcon::Palette),
    ("text.bubble", PluginIcon::Bubble),
    ("speedometer", PluginIcon::Speed),
    ("link", PluginIcon::Link),
    ("arrow.down.circle", PluginIcon::Download),
    ("checkmark", PluginIcon::Check),
    ("sparkles", PluginIcon::Sparkles),
    ("folder", PluginIcon::Folder),
    ("film", PluginIcon::Film),
    ("star", PluginIcon::Star),
    ("gearshape", PluginIcon::Gear),
    ("slider.horizontal.3", PluginIcon::Sliders),
    ("gobackward.10", PluginIcon::Back),
];

pub fn icon_named(name: &str) -> Option<PluginIcon> {
    ICONS
        .iter()
        .find(|(known, _)| *known == name.trim())
        .map(|(_, icon)| icon.clone())
}

fn icon_name(icon: &PluginIcon) -> &'static str {
    ICONS
        .iter()
        .find(|(_, known)| known == icon)
        .map_or("puzzlepiece.extension", |(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erika_packs_name_their_category_and_icon() {
        let pack = Pack::from_pack(&json!({
            "id": "erika.speed", "name": "倍速扩展", "version": "1.0.0",
            "minHostVersion": "0.2.0", "category": "tools",
            "permissions": ["player.control"], "iconId": "speedometer",
        }))
        .unwrap();
        assert_eq!(pack.category, PluginCategory::Tools);
        assert_eq!(pack.icon, PluginIcon::Speed);
        assert!(!pack.fits("0.1.0") && pack.fits("0.2.0") && pack.fits("1.0"));
        assert_eq!(Pack::from_installed(&pack.to_value()).unwrap(), pack);
        assert!(
            Pack::from_pack(&json!({ "id": "a", "name": "a", "version": "1", "category": "music" }))
                .is_err()
        );
    }

    #[test]
    fn nipaplay_manifests_drop_their_version_requirement() {
        let pack = Pack::from_manifest(&json!({
            "id": "titan_danmaku_renderer", "name": "JavaScript弹幕引擎", "version": "1.0.7",
            "minHostVersion": "99.0.0", "permissions": ["danmaku.renderer", "script.external"],
        }))
        .unwrap();
        assert!(pack.nipaplay && pack.fits("0.1.0"));
        assert_eq!(pack.category, PluginCategory::Danmaku);
        assert_eq!(pack.unsupported(), vec!["WebView 弹幕渲染层"]);
        assert_eq!(Pack::from_installed(&pack.to_value()).unwrap(), pack);
        assert!(newer("1.10.0", "1.9.9") && !newer("1.0", "1.0.0"));
    }
}
