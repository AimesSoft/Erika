//! Interface of the enhance module. The application shows these components
//! where it places them: in a page slot, a slot, or by calling them.

use bern::prelude::*;
use erika_enhance_protocol::enhance;

/// The module's page.
#[component]
pub fn enhance_page(enhance: enhance::Client) -> impl View {
    ui! {
        Column(gap: 12, padding: 16) {
            Text(content: tr! { "zh-Hans" => "按钮按下次数：{count}", "en" => "Button presses: {count}"; count: enhance.count })
            Button(id: add_one, label: tr! { "zh-Hans" => "加一", "en" => "Add One" }) => enhance.add_one()
        }
    }
}
