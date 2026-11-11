use bern::prelude::*;
use erika_anime_protocol::anime::{self, MappingKind, MappingProvider, MappingRelation};
use erika_protocol::erika;

#[bern::session]
pub struct MappingDraft {
    pub provider: MappingProvider,
    pub kind: MappingKind,
    pub relation: MappingRelation,
    pub reference: String,
}

#[component]
pub fn provider_dialog(
    app: erika::Client,
    anime: anime::Client,
    #[session] draft: MappingDraft,
) -> impl View {
    ui! {
        Modal(width: 480, appear: pop_in, disappear: fade) {
            Column(gap: 16, padding: 20) {
                Row(gap: 10, align: center) {
                    Text(content: tr! { "zh-Hans" => "提供方关联", "en" => "Provider links" }, style: title3)
                    Spacer
                    Button(id: mapping_close, label: tr! { "zh-Hans" => "完成", "en" => "Done" }, shortcut: app.keys.back, clears: draft.reference) => anime.show_mappings(false)
                }
                Column(height: 120) {
                    ScrollView {
                        Column(gap: 6) {
                            ForEach(items: anime.mapping_targets, key: id) |target| {
                                if anime.mapping_target == target.id {
                                    Button(id: mapping_selected, label: target.title, prominence: prominent) => anime.select_mapping_target(target.id)
                                } else {
                                    Button(id: mapping_target, label: target.title, prominence: plain) => anime.select_mapping_target(target.id)
                                }
                            }
                        }
                    }
                }
                Column(height: 120) {
                ScrollView {
                    Column(gap: 12) {
                        ForEach(items: anime.mapping_rows, key: id) |link| {
                            Row(gap: 12, align: center) {
                                Column(gap: 3) {
                                    Row(gap: 8) {
                                        Text(content: link.provider, style: headline)
                                        Text(content: link.reference)
                                    }
                                    Row(gap: 8) {
                                        Text(content: link.kind, style: caption, emphasis: secondary)
                                        Text(content: link.relation, style: caption, emphasis: secondary)
                                    }
                                }
                                Spacer
                                Button(id: mapping_remove, label: tr! { "zh-Hans" => "解除", "en" => "Unlink" }, role: destructive, prominence: plain) => anime.remove_provider_mapping(link.id)
                            }
                        }
                    }
                }
                }
                Divider
                Picker(id: mapping_provider, label: tr! { "zh-Hans" => "提供方", "en" => "Provider" }, value: draft.provider, style: segmented)
                if anime.mapping_movie {
                    Picker(id: mapping_kind, label: tr! { "zh-Hans" => "类型", "en" => "Type" }, value: draft.kind, style: segmented)
                }
                TextInput(id: mapping_reference, label: tr! { "zh-Hans" => "条目 ID", "en" => "Entry ID" }, value: draft.reference, prompt: tr! { "zh-Hans" => "正整数 ID", "en" => "Positive numeric ID" })
                Picker(id: mapping_relation, label: tr! { "zh-Hans" => "关系", "en" => "Relation" }, value: draft.relation)
                if draft.relation == MappingRelation::Exact {
                    Text(content: tr! { "zh-Hans" => "精确关联会用于已启用账号的同步；解除关联不会撤销远端记录。", "en" => "Exact links enable syncing with active accounts. Unlinking does not undo remote records." }, style: caption, emphasis: secondary)
                }
                if anime.mapping_error != "" {
                    Text(content: anime.mapping_error, style: caption)
                    Button(id: mapping_reload, label: tr! { "zh-Hans" => "刷新关联", "en" => "Reload links" }, prominence: plain) => anime.select_mapping_target(anime.mapping_target)
                }
                Row(gap: 10) {
                    Spacer
                    if draft.relation == MappingRelation::Exact {
                        Button(id: mapping_confirm_exact, label: tr! { "zh-Hans" => "确认精确关联", "en" => "Confirm exact link" }, prominence: prominent, enabled: draft.reference != "") => anime.save_provider_mapping(draft.provider, if anime.mapping_movie { draft.kind } else if anime.mapping_episode { MappingKind::Episode } else { MappingKind::Subject }, draft.relation, draft.reference)
                    } else {
                        Button(id: mapping_save, label: tr! { "zh-Hans" => "保存关联", "en" => "Save link" }, prominence: prominent, enabled: draft.reference != "") => anime.save_provider_mapping(draft.provider, if anime.mapping_movie { draft.kind } else if anime.mapping_episode { MappingKind::Episode } else { MappingKind::Subject }, draft.relation, draft.reference)
                    }
                }
            }
        }
    }
}
