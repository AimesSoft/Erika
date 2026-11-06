//! Interface of the remote module: its page in the settings, and the question
//! a device asking to control playback puts to the user.

use bern::prelude::*;
use erika_remote_protocol::remote;

#[bern::session]
pub struct RemoteDraft {
    pub port: String,
    pub trust: bool,
}

/// 「设置 › 局域网与远程」.
#[component]
pub fn remote_settings(remote: remote::Client, #[session] draft: RemoteDraft) -> impl View {
    ui! {
        Form(title: tr! { "zh-Hans" => "局域网与远程", "en" => "LAN and Remote" }, variant: settings_form, appear: settings_page_in) {
            Section {
                Switch(id: remote_enabled, label: tr! { "zh-Hans" => "远程访问", "en" => "Remote Access" }, checked: remote.enabled, style: switch) => remote.set_enabled(remote.enabled)
                Row(gap: 12, align: center) {
                    TextInput(id: remote_port, label: tr! { "zh-Hans" => "端口", "en" => "Port" }, value: draft.port, prompt: tr! { "zh-Hans" => "{port}", "en" => "{port}"; port: remote.port })
                    Button(id: remote_port_apply, label: tr! { "zh-Hans" => "应用", "en" => "Apply" }, enabled: "" != draft.port, clears: draft.port) => remote.set_port(draft.port)
                }
                Switch(id: remote_receiver, label: tr! { "zh-Hans" => "允许其他设备控制播放", "en" => "Let Other Devices Control Playback" }, checked: remote.receiver, style: switch) => remote.set_receiver(remote.receiver)
                if "" != remote.error {
                    Text(content: remote.error, style: caption, emphasis: secondary, lines: 2)
                }
            }
            if remote.running {
                Section(title: tr! { "zh-Hans" => "地址", "en" => "Addresses" }) {
                    ForEach(items: remote.addresses, key: url) |address| {
                        Row(gap: 12, align: center) {
                            Text(content: address.url, style: body, lines: 1)
                            Spacer
                            Button(id: remote_copy, label: tr! { "zh-Hans" => "复制", "en" => "Copy" }) => remote.copy_address(address.url)
                        }
                    }
                }
            }
            if remote.has_trusted {
                Section(title: tr! { "zh-Hans" => "受信任的设备", "en" => "Trusted Devices" }) {
                    ForEach(items: remote.trusted, key: key) |device| {
                        Row(gap: 12, align: center) {
                            Text(content: device.name, style: body, lines: 1)
                            Spacer
                            Button(id: remote_forget, label: tr! { "zh-Hans" => "移除", "en" => "Remove" }) => remote.forget_device(device.key)
                        }
                    }
                }
            }
        }
    }
}

/// Asked in the main window, whatever it shows.
#[component]
pub fn remote_prompt(remote: remote::Client, #[session] draft: RemoteDraft) -> impl View {
    ui! {
        Stack {
            if let Some(device) = remote.asking {
                Modal(width: 400, appear: pop_in, disappear: fade) {
                    Column(gap: 16, padding: 24) {
                        Text(content: tr! { "zh-Hans" => "{name} 想控制这台设备的播放", "en" => "{name} Wants to Control Playback Here"; name: device.name }, style: title3, lines: 2)
                        Switch(id: remote_trust, label: tr! { "zh-Hans" => "信任此设备", "en" => "Trust This Device" }, checked: draft.trust, style: switch) => draft.trust.toggle()
                        Row(gap: 12, align: center) {
                            Spacer
                            Button(id: remote_deny, label: tr! { "zh-Hans" => "拒绝", "en" => "Deny" }) => remote.answer(false, false)
                            Button(id: remote_allow, label: tr! { "zh-Hans" => "允许", "en" => "Allow" }, prominence: prominent) => remote.answer(true, draft.trust)
                        }
                    }
                }
            }
        }
    }
}
