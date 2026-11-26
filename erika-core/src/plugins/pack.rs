//! What a plugin says it is: the `pack.json` of an Erika plugin, or the
//! `pluginManifest` of a NipaPlay script. An Erika plugin may also keep its
//! declaration in the script, as NipaPlay's do; `pack.json` wins.

use erika_protocol::erika::{PluginCategory, PluginIcon, PluginPermission};
use serde_json::{Value, json};

/// One super-resolution network or post-processing filter bundled inside a
/// picture plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackShader {
    pub id: String,
    pub name: String,
    /// Optional sub-group label; defaults to the plugin's own name.
    pub group: String,
    /// Relative path inside the plugin directory (e.g. `"shaders/crt_lite.glsl"`).
    pub file: String,
}

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
    /// The script's file in the plugin's folder, or empty for pure asset
    /// plugins (such as shader-only picture packs).
    pub main: String,
    /// Smaller goes first where plugins take turns.
    pub priority: i64,
    /// HTTPS scripts to run before the plugin's own.
    pub requires: Vec<External>,
    /// Bundled super-resolution networks (`.esr` / supported mpv CNN shaders).
    pub upscalers: Vec<PackShader>,
    /// Bundled RGB post-processing shaders (mpv `//!HOOK MAIN` GLSL / WGSL).
    pub filters: Vec<PackShader>,
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

fn is_safe_relative_path(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.starts_with('\\') || path.contains(':') {
        return false;
    }
    path.split(['/', '\\'])
        .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn parse_shaders(value: &Value, key: &str, default_group: &str) -> Result<Vec<PackShader>, String> {
    let Some(list) = value.get(key).and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut shaders = Vec::with_capacity(list.len());
    for item in list {
        let id = text(item, "id");
        let name = text(item, "name");
        let file = text(item, "file");
        let group = {
            let g = text(item, "group");
            if g.is_empty() {
                default_group.to_string()
            } else {
                g
            }
        };
        if id.is_empty() || name.is_empty() || file.is_empty() {
            return Err(format!("pack.json 的 {key} 条目缺少 id、name 或 file"));
        }
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return Err(format!(
                "着色器 id「{id}」只能有字母、数字、点、下划线和连字符"
            ));
        }
        if !is_safe_relative_path(&file) {
            return Err(format!("着色器文件路径「{file}」必须在插件目录内"));
        }
        if shaders.iter().any(|existing: &PackShader| existing.id == id) {
            return Err(format!("pack.json 的 {key} 中存在重复的 id「{id}」"));
        }
        shaders.push(PackShader {
            id,
            name,
            group,
            file,
        });
    }
    Ok(shaders)
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
        pack.upscalers = parse_shaders(value, "upscalers", &pack.name)?;
        pack.filters = parse_shaders(value, "filters", &pack.name)?;
        pack.icon = icon_named(&text(value, "iconId")).unwrap_or_else(|| guess_icon(&pack));
        match value.get("main") {
            Some(Value::String(main)) => pack.main = main.trim().to_string(),
            Some(Value::Null) => pack.main.clear(),
            None if !pack.upscalers.is_empty() || !pack.filters.is_empty() => pack.main.clear(),
            _ => {}
        }
        if !pack.main.is_empty() && !is_safe_relative_path(&pack.main) {
            return Err(format!("脚本路径「{}」必须在插件目录内", pack.main));
        }
        Ok(pack)
    }

    /// Whether this plugin provides any bundled picture shaders.
    pub fn has_picture_shaders(&self) -> bool {
        !self.upscalers.is_empty() || !self.filters.is_empty()
    }

    /// Unique relative file paths declared by `upscalers` and `filters`.
    pub fn shader_files(&self) -> Vec<String> {
        let mut files = Vec::new();
        for entry in self.upscalers.iter().chain(&self.filters) {
            if !files.contains(&entry.file) {
                files.push(entry.file.clone());
            }
        }
        files
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
        let (id, name, version) = (
            text(value, "id"),
            text(value, "name"),
            text(value, "version"),
        );
        if id.is_empty() || name.is_empty() || version.is_empty() {
            return Err("插件声明缺少 id、name 或 version".into());
        }
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return Err(format!(
                "插件 id「{id}」只能有字母、数字、点、下划线和连字符"
            ));
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
            upscalers: Vec::new(),
            filters: Vec::new(),
            nipaplay: false,
        })
    }

    /// The declaration as `pack.json` holds it; NipaPlay's plugins keep
    /// theirs this way once installed.
    pub fn to_value(&self) -> Value {
        let shader_json = |items: &[PackShader]| -> Vec<Value> {
            items
                .iter()
                .map(|item| {
                    json!({
                        "id": item.id,
                        "name": item.name,
                        "group": item.group,
                        "file": item.file,
                    })
                })
                .collect()
        };
        let mut object = serde_json::Map::new();
        object.insert("id".into(), json!(self.id));
        object.insert("name".into(), json!(self.name));
        object.insert("version".into(), json!(self.version));
        object.insert("description".into(), json!(self.description));
        object.insert("author".into(), json!(self.author));
        object.insert("github".into(), json!(self.github));
        object.insert("minHostVersion".into(), json!(self.min_host));
        object.insert("permissions".into(), json!(self.permissions));
        object.insert("category".into(), json!(category_name(&self.category)));
        object.insert("icon".into(), json!(self.image));
        object.insert("iconId".into(), json!(icon_name(&self.icon)));
        object.insert("main".into(), json!(self.main));
        object.insert("priority".into(), json!(self.priority));
        object.insert("nipaplay".into(), json!(self.nipaplay));
        if !self.upscalers.is_empty() {
            object.insert("upscalers".into(), Value::Array(shader_json(&self.upscalers)));
        }
        if !self.filters.is_empty() {
            object.insert("filters".into(), Value::Array(shader_json(&self.filters)));
        }
        Value::Object(object)
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
        let mut tags = Vec::new();
        for id in &self.permissions {
            let mapped =
                permission(id).or_else(|| id.strip_prefix("cli:").map(|_| PluginPermission::Cli));
            if let Some(tag) = mapped
                && !tags.contains(&tag)
            {
                tags.push(tag);
            }
        }
        if !self.upscalers.is_empty() && !tags.contains(&PluginPermission::PictureUpscaler) {
            tags.push(PluginPermission::PictureUpscaler);
        }
        if !self.filters.is_empty() && !tags.contains(&PluginPermission::PictureFilter) {
            tags.push(PluginPermission::PictureFilter);
        }
        tags
    }

    /// Build the `bern.cli` allowlist / denylist policy for this plugin's
    /// JavaScript context from its declared permissions.
    ///
    /// Declaring `"cli"` grants unrestricted access (`CliPolicy::allow_all`);
    /// declaring `"cli:<pattern>"` grants specific command IDs or namespaces;
    /// otherwise Erika's standard permissions unlock their matching command
    /// namespaces. `catalog` and `help` are always readable and filtered by
    /// the active policy.
    pub fn cli_policy(&self) -> bern::script::CliPolicy {
        if self.has("cli") {
            return bern::script::CliPolicy::allow_all().with_label(self.id.clone());
        }
        let mut policy = bern::script::CliPolicy::deny_all()
            .allow("catalog")
            .allow("help")
            .with_label(self.id.clone());
        for id in &self.permissions {
            if let Some(pattern) = id.strip_prefix("cli:") {
                let pattern = pattern.trim();
                if !pattern.is_empty() {
                    policy = policy.allow(pattern);
                }
                continue;
            }
            match id.as_str() {
                "player.control" => {
                    policy = policy.allow_many([
                        "media.*",
                        "enhance.*",
                        "state",
                        "erika.open_file",
                        "erika.open_folder",
                        "erika.open_link",
                        "erika.open_url",
                        "erika.play_recent",
                        "erika.continue_last",
                        "erika.play_media",
                        "erika.play",
                        "erika.play_download",
                        "erika.minimize",
                        "erika.expand",
                        "erika.close_player",
                        "erika.toggle_pause",
                        "erika.seek",
                        "erika.skip",
                        "erika.seek_chapter",
                        "erika.skip_segment",
                        "erika.select_episode",
                        "erika.change_episode",
                        "erika.player_panel",
                        "erika.player_tab",
                        "erika.select_subtitle_track",
                        "erika.select_audio_track",
                        "erika.set_speed",
                        "erika.set_rate",
                        "erika.set_enhancement",
                        "erika.set_filter",
                        "erika.set_subtitle_supersampling",
                        "erika.set_player_number",
                        "erika.preview_player_volume",
                        "erika.set_skip_intro",
                        "erika.attach_subtitle",
                        "erika.screenshot",
                    ]);
                }
                "danmaku.modify" => {
                    policy = policy.allow_many([
                        "danmaku.*",
                        "erika.send_danmaku",
                        "erika.set_danmaku_area",
                        "erika.set_danmaku_number",
                        "erika.set_danmaku_switch",
                        "erika.mount_danmaku",
                        "erika.set_danmaku_offset",
                        "erika.shift_danmaku_offset",
                        "erika.set_danmaku_color",
                        "erika.set_danmaku_color_hex",
                    ]);
                }
                "library.read" => {
                    policy = policy.allow_many(["@queries", "state"]);
                }
                "library.write" => {
                    policy = policy.allow_many([
                        "library.*",
                        "anime.*",
                        "downloads.*",
                        "erika.open_library_file",
                        "erika.open_library_show",
                    ]);
                }
                "library.sources" => {
                    policy = policy.allow("sources.*");
                }
                "ui.dialog" => {
                    policy = policy.allow_many([
                        "ui.*",
                        "window.*",
                        "sessions",
                        "erika.show_notice",
                        "erika.show_link_prompt",
                        "erika.show_about",
                        "erika.show_help",
                    ]);
                }
                "settings.read" => {
                    policy =
                        policy.allow_many(["state", "erika.find_settings", "erika.search_actions"]);
                }
                "settings.modify" => {
                    policy = policy.allow_many([
                        "erika.set_startup",
                        "erika.set_playback_end",
                        "erika.set_language",
                        "erika.set_update_channel",
                        "erika.set_preference",
                        "erika.set_module",
                        "erika.apply_preset",
                        "erika.set_accent",
                        "erika.set_appearance",
                        "erika.set_app_icon",
                        "erika.set_shortcut",
                        "erika.reset_shortcut",
                        "erika.reset_shortcuts",
                        "remote.*",
                        "sync.*",
                    ]);
                }
                "system.override" => {
                    policy = policy.allow_many(["downloads.*", "erika.remote_control"]);
                }
                _ => {}
            }
        }
        policy
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
        "cli" => PluginPermission::Cli,
        "picture.upscaler" => PluginPermission::PictureUpscaler,
        "picture.filter" => PluginPermission::PictureFilter,
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
            Pack::from_pack(
                &json!({ "id": "a", "name": "a", "version": "1", "category": "music" })
            )
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

    #[test]
    fn cli_policy_maps_permissions_and_supports_full_or_scoped_cli() {
        use bern::script::CliTargetKind;

        let full = Pack::from_pack(&json!({
            "id": "erika.power", "name": "Power", "version": "1.0.0",
            "category": "tools", "permissions": ["cli"],
        }))
        .unwrap();
        assert_eq!(full.tags(), vec![PluginPermission::Cli]);
        let full_policy = full.cli_policy();
        assert_eq!(full_policy.label(), Some("erika.power"));
        assert!(full_policy.allows_target("media.seek", CliTargetKind::MediaCommand));
        assert!(full_policy.allows_target("downloads.add", CliTargetKind::EngineCommand));
        assert!(full_policy.allows_target("ui.activate", CliTargetKind::SessionCommand));
        assert!(full_policy.allows_target("state", CliTargetKind::State));

        let scoped = Pack::from_pack(&json!({
            "id": "erika.scoped", "name": "Scoped", "version": "1.0.0",
            "category": "tools", "permissions": ["player.control", "cli:downloads.add"],
        }))
        .unwrap();
        assert_eq!(
            scoped.tags(),
            vec![PluginPermission::PlayerControl, PluginPermission::Cli]
        );
        let scoped_policy = scoped.cli_policy();
        assert!(scoped_policy.allows_target("media.seek", CliTargetKind::MediaCommand));
        assert!(scoped_policy.allows_target("downloads.add", CliTargetKind::EngineCommand));
        assert!(!scoped_policy.allows_target("downloads.delete", CliTargetKind::EngineCommand));
        assert!(!scoped_policy.allows_target("ui.activate", CliTargetKind::SessionCommand));
        assert!(scoped_policy.allows_target("catalog", CliTargetKind::Catalog));
    }
}
