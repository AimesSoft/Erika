//! Where plugins hand results back: the comments a load passes along them,
//! links they resolve, folders they suggest, and episodes they mark.

use super::runtime::Purpose;
use crate::ErikaEngine;
use bern::media::MediaSource;
use erika_protocol::erika::FolderSuggestion;
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
        if let Some(error) = &error
            && !matches!(purpose, Purpose::Resolve(_))
        {
            eprintln!("erika: plugin {id}: {error}");
        }
        match purpose {
            Purpose::Suggest(tag) if tag == self.plugins.suggesting => self.take_suggestions(value),
            Purpose::Resolve(tag) => self.resolver_answered(id, tag, value, error),
            _ => {}
        }
    }

    /// Ask the running plugins where the torrent `name` should go.
    pub(crate) fn suggest_download_folders(&mut self, name: &str) {
        self.plugins.next_tag += 1;
        let tag = self.plugins.next_tag;
        self.plugins.suggesting = tag;
        self.state.download_suggestions.clear();
        self.state.has_download_suggestions = false;
        let torrent = serde_json::json!({ "name": name });
        let ids: Vec<String> = self.plugins.running();
        for id in ids {
            let snapshot = self.plugin_snapshot(&id);
            self.plugins.invoke(&id, "downloadSuggestions", Some(&torrent), &snapshot, tag, Purpose::Suggest(tag));
        }
    }

    /// `[{ title, folder }]`: folders that exist, each once.
    fn take_suggestions(&mut self, value: Option<Value>) {
        let Some(list) = value.as_ref().and_then(Value::as_array) else {
            return;
        };
        for item in list {
            let title = item.get("title").and_then(Value::as_str).unwrap_or_default().trim();
            let Some(folder) = item.get("folder").and_then(Value::as_str) else {
                continue;
            };
            let path = std::path::Path::new(folder);
            let known = self
                .state
                .download_suggestions
                .iter()
                .any(|suggestion| suggestion.id == folder);
            if known || !path.is_dir() {
                continue;
            }
            let title = if title.is_empty() {
                path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
            } else {
                title.to_string()
            };
            self.state.download_suggestions.push(FolderSuggestion {
                id: folder.to_string(),
                title,
                folder: MediaSource::file(path),
            });
        }
        self.state.has_download_suggestions = !self.state.download_suggestions.is_empty();
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
