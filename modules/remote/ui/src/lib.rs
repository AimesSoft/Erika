//! Interface of the remote module. The application shows these components
//! where it places them: in a page slot, a slot, or by calling them.

use bern::prelude::*;
use erika_remote_protocol::remote;

/// The module's page.
#[component]
pub fn remote_page(remote: remote::Client) -> impl View {
    ui! {
        Column(gap: 12, padding: 16) {
            Text(content: tr! { "zh-Hans" => "按钮按下次数：{count}", "en" => "Button presses: {count}"; count: remote.count })
            Button(id: add_one, label: tr! { "zh-Hans" => "加一", "en" => "Add One" }) => remote.add_one()
        }
    }
}
