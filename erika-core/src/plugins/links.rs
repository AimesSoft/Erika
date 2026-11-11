//! Links plugins resolve (`url.resolve`, `pluginResolveUrl`): the address of
//! a page becomes the media to play, in rounds the engine drives. Each round
//! the plugin answers with a request to make (`request`), items for the
//! user to pick from (`select`), the media (`play`), or an `error`. The
//! first plugin that answers keeps the link; when none does, it plays as it
//! is.

use super::runtime::Purpose;
use super::{Done, completion, deadline::Deadline, http, text_of};
use crate::ErikaEngine;
use bern::media::MediaSource;
use erika_protocol::erika::{self, LinkChoice, Notice};
use erika_store::plugin_sources::StoredItem;
use serde_json::{Map, Value, json};
use std::time::Duration;
use web_time::Instant;

#[derive(Clone)]
struct SourceIntent {
    entry: StoredItem,
    file_revision: u64,
    playback_revision: u64,
    start: f64,
}

/// The most rounds one link takes.
const ROUNDS: u32 = 16;
const ROUND_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct Resolution {
    url: String,
    /// What the link plays as when no plugin takes it.
    title: String,
    /// The plugins still to ask, in their order, and the one asked.
    left: Vec<String>,
    plugin: String,
    instance: u64,
    state: Value,
    round: u32,
    pub(super) tag: u64,
    expires: Option<Instant>,
    deadline: Option<Deadline>,
    request: Option<http::Pending>,
    /// The plugin took the link: an error is the link's.
    taken: bool,
    source: Option<SourceIntent>,
    entry: Option<bern::Entry>,
}

impl ErikaEngine {
    pub(crate) fn resolve_source(
        &mut self,
        source: &MediaSource,
        start: f64,
        entry: Option<bern::Entry>,
    ) {
        let prepared = (|| -> erika_store::catalog::Result<_> {
            let entry = self
                .db
                .plugin_source_item(&source.url)?
                .ok_or_else(|| erika_store::catalog::Error::conflict("来源条目已移除"))?;
            if !self.source_resolver_allowed(&entry.owner) {
                return Err(erika_store::catalog::Error::conflict(
                    "请启用此来源的插件及播放解析权限",
                ));
            }
            let file_revision = self.db.catalog_revision(entry.file)?;
            Ok(SourceIntent {
                entry,
                file_revision,
                playback_revision: self.match_revision,
                start,
            })
        })();
        let intent = match prepared {
            Ok(intent) => intent,
            Err(error) => {
                self.resolution_failed(error.message);
                return;
            }
        };
        self.cancel_resolution();
        self.file = Some(intent.entry.file);
        self.plugins.resolution = Some(Resolution {
            url: source.url.clone(),
            title: intent.entry.item.title.clone(),
            left: Vec::new(),
            plugin: intent.entry.owner.clone(),
            instance: self.plugins.runtimes[&intent.entry.owner].instance,
            state: Value::Null,
            round: 0,
            expires: None,
            deadline: None,
            request: None,
            tag: 0,
            taken: true,
            source: Some(intent),
            entry,
        });
        self.state.link_resolving = true;
        self.ask_resolver(self.round_input(Value::Null));
    }

    fn source_resolver_allowed(&self, owner: &str) -> bool {
        self.state.features.plugins
            && self.plugins.enabled(owner)
            && self.plugins.runtimes.contains_key(owner)
            && self.plugins.find(owner).is_some_and(|p| {
                p.loaded && p.pack.has("library.sources") && p.pack.has("url.resolve")
            })
    }

    fn resolution_current(&self) -> bool {
        self.plugins.resolution.as_ref().is_some_and(|r| {
            if self
                .plugins
                .runtimes
                .get(&r.plugin)
                .is_none_or(|runtime| runtime.instance != r.instance)
            {
                return false;
            }
            if let Some(source) = &r.source {
                self.source_resolver_allowed(&r.plugin)
                    && self.state.playing
                    && self.match_revision == source.playback_revision
                    && self.file == Some(source.entry.file)
                    && self
                        .logical_source
                        .as_ref()
                        .is_some_and(|s| s.url == source.entry.url)
                    && self.db.catalog_revision(source.entry.file).ok()
                        == Some(source.file_revision)
                    && self
                        .db
                        .plugin_source_item(&source.entry.url)
                        .ok()
                        .flatten()
                        .as_ref()
                        == Some(&source.entry)
            } else {
                self.state.features.plugins
                    && self.plugins.enabled(&r.plugin)
                    && self.plugins.runtimes.contains_key(&r.plugin)
                    && self
                        .plugins
                        .find(&r.plugin)
                        .is_some_and(|p| p.loaded && p.pack.has("url.resolve"))
            }
        })
    }

    /// Validate even without a reply: permission edits and expired promises
    /// cannot leave the player indefinitely displaying a resolving state.
    pub(super) fn check_resolution(&mut self) -> bool {
        let Some(resolution) = self.plugins.resolution.as_ref() else {
            return false;
        };
        if resolution.expires.is_some_and(|at| Instant::now() >= at) {
            self.resolution_failed("链接解析超时".into());
            return false;
        }
        if !self.resolution_current() {
            self.resolution_failed("来源或播放请求已改变，请重新打开".into());
            return false;
        }
        true
    }

    pub(super) fn resolver_stopped(&mut self, owner: &str) {
        if self
            .plugins
            .resolution
            .as_ref()
            .is_some_and(|r| r.plugin == owner)
        {
            self.resolution_failed("来源插件已停止".into());
        }
    }

    /// Offer `url` to the running plugins that resolve links; whether one
    /// may take it. `title` is what it plays as otherwise.
    pub(crate) fn resolve_link(&mut self, url: &str, title: &str) -> bool {
        let mut left: Vec<String> = self
            .plugins
            .installed
            .iter()
            .filter(|plugin| plugin.loaded && plugin.pack.has("url.resolve"))
            .map(|plugin| plugin.pack.id.clone())
            .filter(|id| self.plugins.runtimes.contains_key(id))
            .collect();
        if left.is_empty() {
            return false;
        }
        self.cancel_resolution();
        let plugin = left.remove(0);
        self.plugins.next_tag += 1;
        self.plugins.resolution = Some(Resolution {
            url: url.to_string(),
            title: title.to_string(),
            left,
            instance: self.plugins.runtimes[&plugin].instance,
            plugin,
            state: Value::Null,
            round: 0,
            expires: None,
            deadline: None,
            request: None,
            tag: self.plugins.next_tag,
            taken: false,
            source: None,
            entry: bern::current_entry(),
        });
        self.state.link_resolving = true;
        self.ask_resolver(json!({ "url": url }));
        true
    }

    fn ask_resolver(&mut self, mut input: Value) {
        if !self.check_resolution() {
            return;
        }
        let Some(resolution) = &mut self.plugins.resolution else {
            return;
        };
        resolution.round += 1;
        if resolution.round > ROUNDS {
            self.resolution_failed("解析步骤太多".into());
            return;
        }
        self.plugins.later.remove(&resolution.tag);
        self.plugins.next_tag += 1;
        resolution.tag = self.plugins.next_tag;
        let (id, tag) = (resolution.plugin.clone(), resolution.tag);
        resolution.expires = Some(Instant::now() + ROUND_TIMEOUT);
        let expire = completion(&self.plugins.inbox, move |()| Done::ResolveTimeout(tag));
        resolution.deadline = Some(Deadline::after(ROUND_TIMEOUT, move || expire(())));
        resolution.request = None;
        let snapshot = self.plugin_snapshot(&id);
        super::catalog::attach_context(&mut input, &snapshot);
        self.plugins.invoke(
            &id,
            "resolveUrl",
            Some(&input),
            &snapshot,
            tag,
            Purpose::Resolve(tag),
        );
    }

    /// What the input of the next round carries besides the link.
    fn round_input(&self, extra: Value) -> Value {
        let Some(resolution) = &self.plugins.resolution else {
            return Value::Null;
        };
        let mut input = Map::new();
        input.insert("url".into(), json!(resolution.url));
        input.insert("state".into(), resolution.state.clone());
        if let Some(source) = &resolution.source {
            input.insert("sourceItem".into(), json!(source.entry));
            input.insert(
                "playbackHandle".into(),
                json!(source.entry.item.playback_handle),
            );
        }
        if let Value::Object(extra) = extra {
            input.extend(extra);
        }
        Value::Object(input)
    }

    /// The plugin asked answered a round.
    pub(super) fn resolver_answered(
        &mut self,
        id: &str,
        tag: u64,
        value: Option<Value>,
        error: Option<String>,
    ) {
        if self
            .plugins
            .resolution
            .as_ref()
            .is_none_or(|r| r.tag != tag || r.plugin != id)
        {
            return;
        }
        if !self.check_resolution() {
            return;
        }
        let resolution = self
            .plugins
            .resolution
            .as_mut()
            .expect("checked resolution");
        if let Some(error) = error {
            let reason = if resolution.source.is_some() {
                "来源插件解析失败".into()
            } else {
                error
            };
            self.resolution_failed(reason);
            return;
        }
        let value = value.unwrap_or(Value::Null);
        let kind = value.get("type").and_then(Value::as_str).map(String::from);
        if kind.is_none() && value.is_null() && !resolution.taken {
            // Not this plugin's link: the next one's, or the link itself.
            if resolution.left.is_empty() {
                let (url, title, entry) = (
                    resolution.url.clone(),
                    resolution.title.clone(),
                    resolution.entry,
                );
                self.end_resolution();
                self.play_link_from(&url, title, Vec::new(), entry);
            } else {
                resolution.plugin = resolution.left.remove(0);
                resolution.instance = self
                    .plugins
                    .runtimes
                    .get(&resolution.plugin)
                    .map_or(0, |r| r.instance);
                resolution.round = 0;
                let url = resolution.url.clone();
                self.ask_resolver(json!({ "url": url }));
            }
            return;
        }
        resolution.taken = true;
        if let Some(state) = value.get("state") {
            resolution.state = state.clone();
        }
        match kind.as_deref() {
            Some("request") => {
                let request = value.get("request").cloned().unwrap_or(Value::Null);
                let address = text_of(&request, "url");
                let headers = request.get("headers").cloned().unwrap_or(Value::Null);
                let tag = resolution.tag;
                resolution.request = Some(http::start(
                    http::Request::resolver(&address, &headers),
                    completion(&self.plugins.inbox, move |answer| {
                        Done::Resolved(tag, answer)
                    }),
                ));
            }
            Some("select") => {
                let items: Vec<LinkChoice> = value
                    .get("items")
                    .and_then(Value::as_array)
                    .map(|items| {
                        let preferred = text_of(&value, "preferredId");
                        items
                            .iter()
                            .map(|item| (text_of(item, "id"), text_of(item, "title")))
                            .filter(|(id, title)| !id.is_empty() && !title.is_empty())
                            .take(2000)
                            .map(|(id, title)| LinkChoice {
                                preferred: id == preferred,
                                id,
                                title,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                match items.as_slice() {
                    [] => self.resolution_failed("插件没有给出可以选的条目".into()),
                    [only] => {
                        let input = self.round_input(json!({ "selectedId": only.id }));
                        self.ask_resolver(input);
                    }
                    _ => {
                        // A person choosing a version is not a stalled plugin.
                        resolution.deadline = None;
                        resolution.expires = None;
                        resolution.request = None;
                        self.state.link_choice_title = text_of(&value, "title");
                        self.state.link_choices = items;
                        self.state.link_choosing = true;
                    }
                }
            }
            Some("play") => self.play_resolved(&value),
            Some("error") => {
                let reason = if resolution.source.is_some() {
                    "来源插件解析失败".into()
                } else {
                    text_of(&value, "message")
                };
                self.resolution_failed(reason);
            }
            _ => self.resolution_failed("插件的回答看不懂".into()),
        }
    }

    /// A request a round asked for answered.
    pub(super) fn resolver_fetched(&mut self, tag: u64, answer: Result<Value, String>) {
        if self
            .plugins
            .resolution
            .as_ref()
            .is_none_or(|r| r.tag != tag)
        {
            return;
        }
        if !self.check_resolution() {
            return;
        }
        match answer {
            Ok(response) => {
                let input = self.round_input(json!({ "response": response }));
                self.ask_resolver(input);
            }
            Err(error) => {
                let reason = if self
                    .plugins
                    .resolution
                    .as_ref()
                    .is_some_and(|r| r.source.is_some())
                {
                    "来源请求失败".into()
                } else {
                    error
                };
                self.resolution_failed(reason);
            }
        }
    }

    /// The user picked an item to play.
    pub(crate) fn link_chosen(&mut self, id: String) {
        if self.plugins.resolution.is_none()
            || !self.state.link_choosing
            || !self.state.link_choices.iter().any(|choice| choice.id == id)
        {
            return;
        }
        self.state.link_choosing = false;
        let input = self.round_input(json!({ "selectedId": id }));
        self.ask_resolver(input);
    }

    pub(crate) fn cancel_resolution(&mut self) {
        self.end_resolution();
    }

    fn end_resolution(&mut self) {
        if self
            .plugins
            .resolution
            .as_ref()
            .is_some_and(|r| r.source.is_some())
        {
            self.state.paused = true;
        }
        if let Some(resolution) = self.plugins.resolution.take() {
            self.plugins.later.remove(&resolution.tag);
        }
        self.state.link_resolving = false;
        self.state.link_choosing = false;
        self.state.link_choices.clear();
    }

    fn resolution_failed(&mut self, reason: String) {
        self.end_resolution();
        let reason = if reason.trim().is_empty() {
            "链接解析失败".to_string()
        } else {
            reason
        };
        self.notify(Notice::OperationFailed { reason });
    }

    fn play_source_reference(&mut self, value: &Value) {
        let result = (|| -> erika_store::catalog::Result<_> {
            let reference = serde_json::from_value(value.clone())?;
            let r = self.plugins.resolution.as_ref().expect("active resolver");
            if r.source.is_some() || !self.source_resolver_allowed(&r.plugin) {
                return Err(erika_store::catalog::Error::invalid(
                    "来源播放不能再次转交来源",
                ));
            }
            self.db
                .plugin_source_version(&r.plugin, &reference)?
                .ok_or_else(|| erika_store::catalog::Error::conflict("来源条目不存在"))
        })();
        match result {
            Ok(item) => {
                let entry = self.plugins.resolution.as_ref().and_then(|r| r.entry);
                self.end_resolution();
                self.open_file_from(MediaSource::new(item.url), entry);
            }
            Err(error) => self.resolution_failed(error.message),
        }
    }

    /// `{ type: 'play', url, title, searchTitle, headers, expiresAt }`.
    fn play_resolved(&mut self, value: &Value) {
        if let Some(reference) = value.get("sourceItem") {
            self.play_source_reference(reference);
            return;
        }
        let media = text_of(value, "url");
        if !valid_media_url(&media) {
            self.resolution_failed("插件给的不是网址".into());
            return;
        }
        if value
            .get("expiresAt")
            .is_some_and(|expiry| expiry.as_i64().is_none_or(|at| at <= erika_store::now()))
        {
            self.resolution_failed("播放地址已过期或有效期格式不正确".into());
            return;
        }
        let headers = match playback_headers(value.get("headers")) {
            Ok(headers) => headers,
            Err(reason) => {
                self.resolution_failed(reason);
                return;
            }
        };
        let entry = self.plugins.resolution.as_ref().and_then(|r| r.entry);
        if let Some(source) = self
            .plugins
            .resolution
            .as_ref()
            .and_then(|r| r.source.clone())
        {
            self.end_resolution();
            // Do not call play_link/start: the file, chapter and resume seconds
            // were already selected against this immutable source snapshot.
            self.load_transport(Some(MediaSource::new(media)), source.start, headers, entry);
            self.refresh_playing_catalog();
            return;
        }
        let title = Some(text_of(value, "title"))
            .filter(|title| !title.trim().is_empty())
            .or_else(|| self.plugins.resolution.as_ref().map(|r| r.title.clone()))
            .unwrap_or_else(|| media.clone());
        let search = text_of(value, "searchTitle");
        self.end_resolution();
        self.play_link_from(&media, title, headers, entry);
        if !search.trim().is_empty() {
            let _ = erika::Commands::show_matching(self, true);
            let _ = erika::Commands::search_matching(self, search);
        }
    }
}

pub(super) fn valid_media_url(value: &str) -> bool {
    value.len() <= 16 * 1024
        && !value.chars().any(char::is_control)
        && url::Url::parse(value).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none()
        })
}

pub(super) fn playback_headers(value: Option<&Value>) -> Result<Vec<(String, String)>, String> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let invalid = || "播放请求头格式不正确".to_string();
    let object = value
        .as_object()
        .filter(|o| o.len() <= 64)
        .ok_or_else(invalid)?;
    if value.to_string().len() > 64 * 1024 {
        return Err(invalid());
    }
    object
        .iter()
        .map(|(k, v)| {
            let v = v
                .as_str()
                .filter(|v| allowed_header(k, v))
                .ok_or_else(invalid)?;
            Ok((k.clone(), v.into()))
        })
        .collect()
}

/// Headers a plugin may set: no line breaks, nothing that frames the
/// request itself.
fn allowed_header(key: &str, value: &str) -> bool {
    let valid_key = !key.is_empty()
        && key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c));
    let key = key.to_ascii_lowercase();
    let framing = [
        "host",
        "content-length",
        "connection",
        "transfer-encoding",
        "upgrade",
        "te",
        "trailer",
        "keep-alive",
        "proxy-connection",
    ];
    valid_key
        && !framing.contains(&key.as_str())
        && !value.chars().any(|c| c.is_control() && c != '\t')
}

#[cfg(test)]
#[path = "links_tests.rs"]
mod tests;
