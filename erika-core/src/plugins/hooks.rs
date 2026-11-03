//! Where plugins hand results back: the comments a load passes along them,
//! links they resolve, folders they suggest, and episodes they mark.

use super::Plugins;
use super::runtime::Purpose;
use crate::ErikaEngine;
use crate::kernel::Kernel;
use bern::media::MediaSource;
use erika_protocol::erika::FolderSuggestion;
use liberika_plugin::{BlockList, Comment};
use serde_json::{Value, json};
use std::sync::Arc;

/// A track of comments: as loaded, and as the plugins left it.
pub(crate) struct Track {
    loaded: Vec<Comment>,
    processed: Vec<Comment>,
}

/// A track passing along the plugins that modify danmaku.
pub(crate) struct Chain {
    track: String,
    order: Vec<String>,
    index: usize,
    comments: Vec<Comment>,
    tag: u64,
    replaced: bool,
}

/// The comments of `contents` as plugins see them: liberika reads any
/// danmaku file where it is linked, JSON is read elsewhere.
fn comments_of(contents: &str) -> Option<Vec<Comment>> {
    #[cfg(any(target_os = "macos", target_os = "windows", all(target_os = "linux", not(target_env = "ohos"))))]
    {
        let timeline = liberika::danmaku::DanmakuTimeline::parse_auto(contents).ok()?;
        Some(liberika::plugin::comments(&timeline))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", all(target_os = "linux", not(target_env = "ohos")))))]
    {
        let value: Value = serde_json::from_str(contents).ok()?;
        let list = match &value {
            Value::Array(list) => list,
            other => other.get("comments")?.as_array()?,
        };
        Some(list.iter().filter_map(Comment::from_value).collect())
    }
}

/// Show track `name` as the plugins left it, without what the block lists
/// name; how many comments show.
fn show_track(kernel: &Arc<dyn Kernel>, plugins: &Plugins, name: &str) -> Result<usize, String> {
    let Some(track) = plugins.tracks.get(name) else {
        return Ok(0);
    };
    let list = BlockList::new(plugins.block_words());
    let shown: Vec<Comment> = track
        .processed
        .iter()
        .filter(|comment| !list.blocks(&comment.content))
        .cloned()
        .collect();
    kernel.add_danmaku(name, &liberika_plugin::comments_json(&shown).to_string())
}

/// When each comment shown plays, across the tracks: what the plugins
/// left, without what the block lists hide.
pub(super) fn shown_times(plugins: &Plugins) -> Vec<f64> {
    let list = BlockList::new(plugins.block_words());
    plugins
        .tracks
        .values()
        .flat_map(|track| &track.processed)
        .filter(|comment| !list.blocks(&comment.content))
        .map(|comment| comment.time)
        .collect()
}

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
            Purpose::Danmaku(tag) => self.chain_answered(id, tag),
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

    /// A new video: the comments of the last one go.
    pub(crate) fn plugins_new_video(&mut self) {
        self.plugins.tracks.clear();
        self.plugins.chain = None;
        self.plugins.waiting_tracks.clear();
        self.refresh_danmaku_density();
    }

    /// Show `contents`, a danmaku file or the JSON liberika reads, as the
    /// track `name`: what the block lists name stays hidden, and the plugins
    /// that modify danmaku see the comments in turn (`danmakuLoaded`), each
    /// what the one before left, and may replace them.
    pub(crate) fn load_danmaku(
        &mut self,
        kernel: &Arc<dyn Kernel>,
        name: &str,
        contents: &str,
    ) -> Result<usize, String> {
        // What reads as no comments goes to the kernel as it is, which
        // knows more ways to read a file.
        let Some(comments) = comments_of(contents).filter(|comments| !comments.is_empty()) else {
            return kernel.add_danmaku(name, contents);
        };
        self.plugins.tracks.insert(
            name.to_string(),
            Track {
                loaded: comments.clone(),
                processed: comments,
            },
        );
        self.plugins.last_track = name.to_string();
        let count = show_track(kernel, &self.plugins, name)?;
        self.refresh_danmaku_density();
        self.plugins.waiting_tracks.retain(|waiting| waiting != name);
        self.plugins.waiting_tracks.push(name.to_string());
        self.next_chain();
        Ok(count)
    }

    /// Pass the next track waiting along the plugins that modify danmaku.
    fn next_chain(&mut self) {
        if self.plugins.chain.is_some() {
            return;
        }
        let order: Vec<String> = self
            .plugins
            .installed
            .iter()
            .filter(|plugin| plugin.loaded && plugin.pack.has("danmaku.modify"))
            .map(|plugin| plugin.pack.id.clone())
            .filter(|id| self.plugins.runtimes.contains_key(id))
            .collect();
        if order.is_empty() {
            self.plugins.waiting_tracks.clear();
            return;
        }
        while !self.plugins.waiting_tracks.is_empty() {
            let name = self.plugins.waiting_tracks.remove(0);
            let Some(track) = self.plugins.tracks.get(&name) else {
                continue;
            };
            self.plugins.next_tag += 1;
            self.plugins.chain = Some(Chain {
                comments: track.loaded.clone(),
                track: name,
                order,
                index: 0,
                tag: self.plugins.next_tag,
                replaced: false,
            });
            self.ask_chain();
            return;
        }
    }

    fn ask_chain(&mut self) {
        let Some(chain) = &self.plugins.chain else {
            return;
        };
        let (id, tag) = (chain.order[chain.index].clone(), chain.tag);
        let event = json!({
            "name": "danmakuLoaded",
            "data": { "danmaku": liberika_plugin::comments_json(&chain.comments) },
        });
        let snapshot = self.plugin_snapshot(&id);
        self.plugins
            .invoke(&id, "onEvent", Some(&event), &snapshot, tag, Purpose::Danmaku(tag));
    }

    /// The plugin whose turn it was answered: the next one's turn, or the
    /// track shows what they left.
    fn chain_answered(&mut self, id: &str, tag: u64) {
        let Some(chain) = &mut self.plugins.chain else {
            return;
        };
        if chain.tag != tag || chain.order.get(chain.index).is_none_or(|turn| turn != id) {
            return;
        }
        chain.index += 1;
        if chain.index < chain.order.len() {
            self.ask_chain();
            return;
        }
        let chain = self.plugins.chain.take().expect("a chain");
        if let Some(track) = self.plugins.tracks.get_mut(&chain.track) {
            track.processed = chain.comments;
            if chain.replaced {
                self.show_track(&chain.track);
            }
        }
        self.next_chain();
    }

    fn show_track(&mut self, name: &str) {
        if let Some(kernel) = self.kernel().filter(|_| self.state.video)
            && let Ok(count) = show_track(&kernel, &self.plugins, name)
        {
            self.add_danmaku_source(name.to_string(), count);
        }
        self.refresh_danmaku_density();
    }

    /// Show the comments loaded again, hiding what the block lists name now.
    pub(super) fn reload_plugin_danmaku(&mut self) {
        let names: Vec<String> = self.plugins.tracks.keys().cloned().collect();
        for name in names {
            self.show_track(&name);
        }
    }

    /// Pass every track along the plugins again, as a plugin came or went.
    pub(super) fn rerun_plugin_danmaku(&mut self) {
        let names: Vec<String> = self.plugins.tracks.keys().cloned().collect();
        self.plugins.chain = None;
        for name in &names {
            if let Some(track) = self.plugins.tracks.get_mut(name) {
                track.processed = track.loaded.clone();
            }
            self.show_track(name);
        }
        self.plugins.waiting_tracks = names;
        self.next_chain();
    }

    /// `danmaku.replace`: in its turn, what the next plugin sees; later, the
    /// comments the last track loaded shows from now on.
    pub(super) fn replace_plugin_danmaku(&mut self, id: &str, comments: Vec<Comment>) {
        if let Some(chain) = &mut self.plugins.chain
            && chain.order.get(chain.index).is_some_and(|turn| turn == id)
        {
            chain.comments = comments;
            chain.replaced = true;
            return;
        }
        let name = self.plugins.last_track.clone();
        if let Some(track) = self.plugins.tracks.get_mut(&name) {
            track.processed = comments;
            self.show_track(&name);
        }
    }

    /// What the running plugins add to the danmaku composer and to the
    /// episodes of a show changed.
    pub(super) fn plugin_controls_changed(&mut self) {
        let extensions = self.plugins.extensions();
        let colors: Vec<i64> = extensions
            .colors
            .iter()
            .filter_map(|(_, hex)| i64::from_str_radix(hex.trim_start_matches('#'), 16).ok())
            .collect();
        self.set_plugin_danmaku_colors(&colors, extensions.hex_input);
        #[cfg(feature = "bern-module-anime")]
        if self.plugins.watched_button != extensions.watched_button {
            self.plugins.watched_button = extensions.watched_button;
            erika_anime_protocol::anime::calls::set_episode_marking(extensions.watched_button);
        }
    }

    /// `library.setEpisodeWatched`: the open show's episode, here and on the
    /// accounts.
    pub(super) fn plugin_mark_watched(&mut self, anime: i64, episode: i64, watched: bool) {
        #[cfg(feature = "bern-module-anime")]
        if erika_anime_protocol::anime::current_state().is_some_and(|state| state.opened == anime) {
            erika_anime_protocol::anime::calls::mark_episode(episode, watched);
        }
        #[cfg(not(feature = "bern-module-anime"))]
        let _ = (anime, episode, watched);
    }
}
