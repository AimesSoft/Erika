//! The fixed action index of the desktop prototype.
use erika_protocol::erika::SearchActions;

pub(crate) fn actions(text: &str, anime_enabled: bool, danmaku_enabled: bool) -> SearchActions {
    let q = text.trim().to_lowercase();
    let matches = |name: &str| q.is_empty() || name.to_lowercase().contains(&q);
    let mut slots = if q.is_empty() { 3 } else { 4 };
    let mut setting = |name: &str| {
        let found = slots > 0 && matches(name);
        if found {
            slots -= 1;
        }
        found
    };
    let theme = setting("主题色 设置 › 外观 theme color appearance settings");
    let danmaku = danmaku_enabled && setting("弹幕不透明度 设置 › 弹幕 danmaku opacity settings");
    let skip_intro = setting("跳过片头秒数 设置 › 播放 skip intro playback settings");
    let optional = setting("可选功能 设置 optional features settings");
    let plugins = setting("插件市场 设置 › 插件 plugin marketplace settings");
    let keys = setting("快捷键 设置 keyboard shortcuts settings");
    let open_file = matches("打开文件… open file");
    let schedule = anime_enabled && matches("新番表 airing schedule");
    let dark = matches("切换深色模式 toggle dark mode");
    let has_settings = theme || danmaku || skip_intro || optional || plugins || keys;
    let has_operations = open_file || schedule || dark;
    SearchActions {
        theme,
        danmaku,
        skip_intro,
        optional,
        plugins,
        keys,
        open_file,
        schedule,
        dark,
        has_settings,
        has_operations,
        empty: !has_settings && !has_operations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_query_uses_three_settings_and_all_operations() {
        let a = actions("", true, true);
        assert!(a.theme && a.danmaku && a.skip_intro && a.open_file && a.schedule && a.dark);
        assert!(!a.optional && !a.plugins && !a.keys);
        assert!(actions("不存在", true, true).empty);
        assert!(actions("  深色  ", true, true).dark);
        let a = actions("设置", true, true);
        assert!(a.theme && a.danmaku && a.skip_intro && a.optional);
        assert!(!a.plugins && !a.keys);
        let disabled = actions("", false, false);
        assert!(!disabled.schedule && !disabled.danmaku && disabled.optional);
    }
}
