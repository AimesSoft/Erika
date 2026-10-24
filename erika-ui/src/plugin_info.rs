//! A plugin's page: who made it and what it may do, what can be done with
//! it, and its `README.md` — NipaPlay's from beside its script, Erika's
//! from its folder — shown block by block in the platform's own text.
use crate::settings::{plugin_icon, plugin_icon_props};
use bern::prelude::*;
use erika_protocol::erika::{self, *};

/// The page over the plugins, opened from a plugin's card.
#[component]
pub fn plugin_info(app: erika::Client) -> impl View {
    ui! {
        Modal(width: 560, appear: pop_in, disappear: fade) {
            Column(gap: 14, padding: 20) {
                if let Some(plugin) = app.plugin_info.plugin {
                    Row(gap: 14, align: center) {
                        plugin_icon(plugin: plugin)
                        Column(gap: 3) {
                            Text(content: plugin.name, style: title3, lines: 2)
                            Text(content: tr! { "zh-Hans" => "{author} · {version}", "en" => "{author} · {version}"; author: plugin.author, version: plugin.version }, style: caption, emphasis: secondary, lines: 1)
                        }
                        Spacer
                        plugin_actions(app: app, plugin: plugin)
                    }
                    Flow(gap: 6) {
                        ForEach(items: plugin.tags, key: id) |tag| {
                            Stack(variant: settings_tag) {
                                Row(padding_x: 8, padding_y: 2) {
                                    Text(content: tag.permission, style: caption, emphasis: secondary)
                                }
                            }
                        }
                    }
                    if "" != plugin.unsupported {
                        Text(content: tr! { "zh-Hans" => "Erika 暂不支持：{what}", "en" => "Erika does not support: {what}"; what: plugin.unsupported }, style: caption, emphasis: secondary, lines: 2)
                    }
                    if "" != plugin.error {
                        Text(content: tr! { "zh-Hans" => "没有载入：{error}", "en" => "Not loaded: {error}"; error: plugin.error }, style: caption, emphasis: secondary, lines: 3)
                    }
                }
                Divider
                ScrollView {
                    if app.plugin_info.loading {
                        Progress(label: tr! { "zh-Hans" => "正在读取说明", "en" => "Reading the description" })
                    } else {
                        document(blocks: app.plugin_info.blocks)
                    }
                }
                Row(gap: 10, align: center) {
                    if let Some(plugin) = app.plugin_info.plugin {
                        if plugin.installed {
                            if plugin.has_settings && plugin.enabled {
                                Button(id: plugin_info_settings, label: tr! { "zh-Hans" => "设置", "en" => "Settings" }, icon: "gearshape", prominence: plain) => app.open_plugin(plugin.id)
                            }
                            Button(id: plugin_info_uninstall, label: tr! { "zh-Hans" => "卸载", "en" => "Uninstall" }, role: destructive, prominence: plain) => app.uninstall_plugin(plugin.id)
                        }
                    }
                    Spacer
                    Button(id: plugin_info_done, label: tr! { "zh-Hans" => "完成", "en" => "Done" }, prominence: prominent, shortcut: app.keys.back) => app.close_plugin_info()
                }
            }
        }
    }
}

/// What can be done with a plugin where it is listed: get it, update it,
/// turn it on or off.
#[component]
pub fn plugin_actions(app: erika::Client, plugin: Plugin) -> impl View {
    ui! {
        Row(gap: 8, align: center) {
            if plugin.busy {
                Text(content: tr! { "zh-Hans" => "安装中…", "en" => "Installing…" }, style: caption, emphasis: secondary)
            } else if plugin.installed {
                if plugin.update {
                    Button(id: update_plugin, label: tr! { "zh-Hans" => "更新", "en" => "Update" }, prominence: prominent) => app.install_plugin(plugin.id)
                }
                Switch(id: plugin_enabled, label: tr! { "zh-Hans" => "启用 {name}", "en" => "Enable {name}"; name: plugin.name }, label_hidden: true, checked: plugin.enabled, style: switch) => app.set_plugin_enabled(plugin.id, plugin.enabled)
            } else if "" != plugin.unsupported {
                Text(content: tr! { "zh-Hans" => "不支持", "en" => "Unsupported" }, style: caption, emphasis: secondary)
            } else {
                Button(id: get_plugin, label: tr! { "zh-Hans" => "获取", "en" => "Get" }, prominence: prominent) => app.install_plugin(plugin.id)
            }
        }
    }
}

/// A Markdown document, block by block: titles, paragraphs, list items,
/// quotes, code, pictures, rules and the rows of a table, their words with
/// the emphasis, code and links Markdown gives them.
#[component]
pub fn document(blocks: Vec<DocBlock>) -> impl View {
    ui! {
        Column(gap: 10, padding_y: 4) {
            ForEach(items: blocks, key: id) |block| {
                match block.kind {
                    DocKind::Title => { Text(content: block.text, style: title2, markdown: true) }
                    DocKind::Heading => {
                        Column(gap: 0) {
                            Canvas(height: 6)
                            Text(content: block.text, style: title3, markdown: true)
                        }
                    }
                    DocKind::Subheading => { Text(content: block.text, style: headline, markdown: true) }
                    DocKind::Paragraph => { Text(content: block.text, markdown: true) }
                    DocKind::Item => {
                        Row(gap: 8, align: top) {
                            if block.depth > 0 {
                                Canvas(width: 16, height: 1)
                            }
                            Text(content: block.marker, emphasis: secondary)
                            Text(content: block.text, markdown: true)
                            Spacer
                        }
                    }
                    DocKind::Quote => {
                        Stack(variant: document_block, align: leading) {
                            Row(gap: 0, padding: 12) {
                                Text(content: block.text, emphasis: secondary, markdown: true)
                                Spacer
                            }
                        }
                    }
                    DocKind::Code => {
                        Stack(variant: document_block, align: leading) {
                            Row(gap: 0, padding: 12) {
                                Text(content: block.text, style: code)
                                Spacer
                            }
                        }
                    }
                    DocKind::Rule => { Divider }
                    DocKind::Image => {
                        if let Some(picture) = block.image {
                            Image(source: picture, width: 360, fit: contain)
                        }
                    }
                    DocKind::Term => {
                        Column(gap: 2) {
                            Text(content: block.text, style: headline, markdown: true)
                            Text(content: block.detail, emphasis: secondary, markdown: true)
                        }
                    }
                }
            }
        }
    }
}
