use bern::prelude::*;
use erika_anime_protocol::anime::{self, MappingKind, MappingProvider, MappingRelation};

#[bern::session]
pub struct MappingDraft {
    pub provider: MappingProvider,
    pub kind: MappingKind,
    pub relation: MappingRelation,
    pub reference: String,
}

#[component]
pub fn provider_popover(anime: anime::Client, #[session] draft: MappingDraft) -> impl View {
    ui! {
        Column(gap: 12, width: 380) {
            Text(content: tr! { "zh-Hans" => "提供方关联", "en" => "Provider links" }, style: headline)
            Column(height: 96) {
                ScrollView {
                    Column(gap: 4) {
                        ForEach(items: anime.mapping_targets, key: id) |target| {
                            Button(id: mapping_target, label: target.title) {
                                Row(gap: 8, height: 30, padding_x: 4, align: center) {
                                    Text(content: target.title, lines: 1)
                                    Spacer
                                    if anime.mapping_target == target.id {
                                        Label(title: tr! { "zh-Hans" => "已选", "en" => "Selected" }, icon: "checkmark", display: icon)
                                    }
                                }
                            } => anime.select_mapping_target(target.id)
                        }
                    }
                }
            }
            Divider
            Column(height: 96) {
                ScrollView {
                    Column(gap: 10) {
                        ForEach(items: anime.mapping_rows, key: id) |link| {
                            Row(gap: 10, align: center) {
                                Column(gap: 2) {
                                    Row(gap: 6) {
                                        Text(content: link.provider, style: headline)
                                        Text(content: link.reference)
                                    }
                                    Row(gap: 6) {
                                        Text(content: link.kind, style: caption, emphasis: secondary)
                                        if link.confirmed {
                                            Text(content: link.relation, style: caption, emphasis: secondary)
                                        }
                                    }
                                }
                                Spacer
                                if link.confirmed {
                                    Button(id: mapping_remove, label: tr! { "zh-Hans" => "解除", "en" => "Unlink" }, role: destructive, prominence: plain) => anime.remove_provider_mapping(link.id)
                                } else {
                                    Button(id: mapping_dismiss, label: tr! { "zh-Hans" => "忽略", "en" => "Dismiss" }, prominence: plain) => anime.remove_provider_mapping(link.id)
                                    Button(id: mapping_accept, label: tr! { "zh-Hans" => "确认", "en" => "Confirm" }) => anime.confirm_provider_mapping(link.id)
                                }
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
            if anime.mapping_error != "" {
                Text(content: anime.mapping_error, style: caption)
                Button(id: mapping_reload, label: tr! { "zh-Hans" => "刷新关联", "en" => "Reload links" }, prominence: plain) => anime.select_mapping_target(anime.mapping_target)
            }
            Row(gap: 10) {
                Spacer
                Button(id: mapping_save, label: tr! { "zh-Hans" => "保存关联", "en" => "Save link" }, enabled: draft.reference != "") => anime.save_provider_mapping(draft.provider, if anime.mapping_movie { draft.kind } else if anime.mapping_episode { MappingKind::Episode } else { MappingKind::Subject }, draft.relation, draft.reference)
            }
        }
    }
}
