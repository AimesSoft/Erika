//! Interface of the library module. The application shows these components
//! where it places them: in a page slot, a slot, or by calling them.

use bern::prelude::*;
use erika_library_protocol::library;

/// The module's page.
#[component]
pub fn library_page(library: library::Client) -> impl View {
    ui! {
        Column(gap: 12, padding: 32, appear: page_in) {
            Text(content: tr! { "zh-Hans" => "媒体库", "en" => "Library" }, style: large_title)
            Text(content: tr! { "zh-Hans" => "缩放墙与拼音索引在下一步做", "en" => "The zoom wall and pinyin index come next" }, emphasis: secondary)
        }
    }
}
