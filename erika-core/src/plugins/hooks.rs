//! Where plugins hand results back: the comments a load passes along them,
//! links they resolve, folders they suggest, and episodes they mark.

use super::runtime::Purpose;
use crate::ErikaEngine;
use liberika_plugin::Comment;
use serde_json::Value;

impl ErikaEngine {
    /// A hook's evaluation answered with `value`.
    pub(super) fn plugin_hook_answered(
        &mut self,
        id: &str,
        purpose: Purpose,
        value: Option<Value>,
        error: Option<String>,
    ) {
        if let Some(error) = error {
            eprintln!("erika: plugin {id}: {error}");
        }
        let _ = (purpose, value);
    }

    /// Show the comments loaded again, through the plugins as they are now.
    pub(super) fn reload_plugin_danmaku(&mut self) {}

    /// `danmaku.replace` outside a load: the comments shown from now on.
    pub(super) fn replace_plugin_danmaku(&mut self, comments: Vec<Comment>) {
        let _ = comments;
    }

    /// What the running plugins add to the danmaku composer and to the
    /// episodes of a show changed.
    pub(super) fn plugin_controls_changed(&mut self) {}

    /// `library.setEpisodeWatched`.
    pub(super) fn plugin_mark_watched(&mut self, anime: i64, episode: i64, watched: bool) {
        let _ = (anime, episode, watched);
    }
}
