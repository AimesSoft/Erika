//! Links plugins resolve (`url.resolve`, `pluginResolveUrl`): the address of
//! a page becomes the media to play, in rounds the engine drives. Each round
//! the plugin answers with a request to make (`request`), items for the
//! user to pick from (`select`), the media (`play`), or an `error`. The
//! first plugin that answers keeps the link; when none does, it plays as it
//! is.

use super::runtime::Purpose;
use super::{Done, text_of};
use crate::{ErikaEngine, in_background};
use erika_protocol::erika::{self, LinkChoice, Notice};
use serde_json::{Map, Value, json};

/// The most rounds one link takes.
const ROUNDS: u32 = 16;

pub(crate) struct Resolution {
    url: String,
    /// What the link plays as when no plugin takes it.
    title: String,
    /// The plugins still to ask, in their order, and the one asked.
    left: Vec<String>,
    plugin: String,
    state: Value,
    round: u32,
    tag: u64,
    /// The plugin took the link: an error is the link's.
    taken: bool,
}

impl ErikaEngine {
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
        let plugin = left.remove(0);
        self.plugins.next_tag += 1;
        self.plugins.resolution = Some(Resolution {
            url: url.to_string(),
            title: title.to_string(),
            left,
            plugin,
            state: Value::Null,
            round: 0,
            tag: self.plugins.next_tag,
            taken: false,
        });
        self.state.link_resolving = true;
        self.ask_resolver(json!({ "url": url }));
        true
    }

    fn ask_resolver(&mut self, input: Value) {
        let Some(resolution) = &mut self.plugins.resolution else {
            return;
        };
        resolution.round += 1;
        if resolution.round > ROUNDS {
            self.resolution_failed("解析步骤太多".into());
            return;
        }
        let (id, tag) = (resolution.plugin.clone(), resolution.tag);
        let snapshot = self.plugin_snapshot(&id);
        self.plugins
            .invoke(&id, "resolveUrl", Some(&input), &snapshot, tag, Purpose::Resolve(tag));
    }

    /// What the input of the next round carries besides the link.
    fn round_input(&self, extra: Value) -> Value {
        let Some(resolution) = &self.plugins.resolution else {
            return Value::Null;
        };
        let mut input = Map::new();
        input.insert("url".into(), json!(resolution.url));
        input.insert("state".into(), resolution.state.clone());
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
        let Some(resolution) = &mut self.plugins.resolution else {
            return;
        };
        if resolution.tag != tag || resolution.plugin != id {
            return;
        }
        if let Some(error) = error {
            self.resolution_failed(error);
            return;
        }
        let value = value.unwrap_or(Value::Null);
        let kind = value.get("type").and_then(Value::as_str).map(String::from);
        if kind.is_none() && value.is_null() && !resolution.taken {
            // Not this plugin's link: the next one's, or the link itself.
            if resolution.left.is_empty() {
                let (url, title) = (resolution.url.clone(), resolution.title.clone());
                self.end_resolution();
                self.play_link(&url, title);
            } else {
                resolution.plugin = resolution.left.remove(0);
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
                in_background(&self.plugins.inbox, move || {
                    vec![Done::Resolved(tag, get(&address, &headers))]
                });
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
                        self.state.link_choice_title = text_of(&value, "title");
                        self.state.link_choices = items;
                        self.state.link_choosing = true;
                    }
                }
            }
            Some("play") => self.play_resolved(&value),
            Some("error") => self.resolution_failed(text_of(&value, "message")),
            _ => self.resolution_failed("插件的回答看不懂".into()),
        }
    }

    /// A request a round asked for answered.
    pub(super) fn resolver_fetched(&mut self, tag: u64, answer: Result<Value, String>) {
        if self.plugins.resolution.as_ref().is_none_or(|r| r.tag != tag) {
            return;
        }
        match answer {
            Ok(response) => {
                let input = self.round_input(json!({ "response": response }));
                self.ask_resolver(input);
            }
            Err(error) => self.resolution_failed(error),
        }
    }

    /// The user picked an item to play.
    pub(crate) fn link_chosen(&mut self, id: String) {
        if self.plugins.resolution.is_none() || !self.state.link_choosing {
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
        self.plugins.resolution = None;
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

    /// `{ type: 'play', url, title, searchTitle, headers }`.
    fn play_resolved(&mut self, value: &Value) {
        let media = text_of(value, "url");
        if !(media.starts_with("https://") || media.starts_with("http://")) {
            self.resolution_failed("插件给的不是网址".into());
            return;
        }
        let headers: Vec<(String, String)> = value
            .get("headers")
            .and_then(Value::as_object)
            .map(|headers| {
                headers
                    .iter()
                    .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
                    .filter(|(key, value)| allowed_header(key, value))
                    .collect()
            })
            .unwrap_or_default();
        let title = Some(text_of(value, "title"))
            .filter(|title| !title.trim().is_empty())
            .or_else(|| self.plugins.resolution.as_ref().map(|r| r.title.clone()))
            .unwrap_or_else(|| media.clone());
        let search = text_of(value, "searchTitle");
        self.end_resolution();
        crate::kernel::set_link_headers(&media, headers);
        self.play_link(&media, title);
        if !search.trim().is_empty() {
            let _ = erika::Commands::show_matching(self, true);
            let _ = erika::Commands::search_matching(self, search);
        }
    }
}

/// Headers a plugin may set: no line breaks, nothing that frames the
/// request itself.
fn allowed_header(key: &str, value: &str) -> bool {
    let key = key.trim().to_ascii_lowercase();
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
    !key.is_empty()
        && !framing.contains(&key.as_str())
        && !key.contains(['\r', '\n', ':'])
        && !value.contains(['\r', '\n'])
}

/// A round's GET: its status, lower-case headers and body. Redirects are
/// not followed: the plugin reads `location` and decides.
#[cfg(not(target_arch = "wasm32"))]
fn get(url: &str, headers: &Value) -> Result<Value, String> {
    let web = url.starts_with("https://") || url.starts_with("http://");
    let credentials = url
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .is_some_and(|authority| authority.contains('@'));
    if !web || credentials {
        return Err(format!("插件要请求的地址不能用：{url}"));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into();
    let mut request = ureq::http::Request::builder().method("GET").uri(url);
    if let Some(headers) = headers.as_object() {
        for (key, value) in headers {
            if let Some(value) = value.as_str()
                && allowed_header(key, value)
            {
                request = request.header(key.as_str(), value);
            }
        }
    }
    let request = request.body(()).map_err(|error| error.to_string())?;
    let mut response = agent.run(request).map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let headers: Map<String, Value> = response
        .headers()
        .iter()
        .filter_map(|(key, value)| Some((key.as_str().to_ascii_lowercase(), json!(value.to_str().ok()?))))
        .collect();
    let body = response
        .body_mut()
        .with_config()
        .limit(4 << 20)
        .read_to_vec()
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "url": url,
        "status": status,
        "headers": headers,
        "body": String::from_utf8_lossy(&body),
    }))
}

#[cfg(target_arch = "wasm32")]
fn get(_url: &str, _headers: &Value) -> Result<Value, String> {
    Err("网页版的插件暂时不能联网".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn plugins_set_only_safe_headers() {
        assert!(super::allowed_header("Referer", "https://www.bilibili.com/"));
        assert!(!super::allowed_header("Host", "example.com"));
        assert!(!super::allowed_header("X-A", "a\r\nX-B: b"));
    }
}
