//! Interface of the downloads module. The application shows these components
//! where it places them: in a page slot, a slot, or by calling them.

use bern::prelude::*;
use erika_downloads_protocol::downloads;

/// The module's page.
#[component]
pub fn downloads_page(downloads: downloads::Client) -> impl View {
    ui! {
        Column(gap: 12, padding: 32, appear: page_in) {
            Text(content: tr! { "zh-Hans" => "下载", "en" => "Downloads" }, style: large_title)
            Text(content: tr! { "zh-Hans" => "进行中与已完成在下一步做", "en" => "In progress and done come next" }, emphasis: secondary)
        }
    }
}
