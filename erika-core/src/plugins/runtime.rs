//! One plugin's JavaScript context, and the evaluations it has running.
//! Contexts come from `bern::script`: the system's engine where Bern has a
//! web view, Boa elsewhere. Evaluations finish later; each one wakes the
//! engine as it does.

use super::pack::Pack;
use bern::script::{Context, Evaluation, Value};
use serde_json::json;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll, Wake, Waker};

const HOST: &str = include_str!("host.js");

/// What an evaluation was for.
#[derive(Debug, Clone, PartialEq)]
pub enum Purpose {
    /// The plugin's script ran, and its `pluginOnInitialize`.
    Load,
    /// `pluginOnEvent`; its calls are all that counts.
    Event,
    /// `pluginHandleUIAction` for a settings entry.
    Action(String),
    /// A link to resolve, by the engine's tag.
    Resolve(u64),
    /// The comments of a load, passed along the plugins in turn.
    Danmaku(u64),
    /// Folders to download a torrent into.
    Suggest(u64),
    /// A request's answer, or reading back what continuations queued.
    Settle,
}

/// Wakes the engine when an evaluation finishes.
struct EngineWake(Option<bern::wake::Waker>);

impl Wake for EngineWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        if let Some(waker) = &self.0 {
            waker.wake();
        }
    }
}

pub fn engine_waker() -> Waker {
    Waker::from(Arc::new(EngineWake(bern::wake::waker())))
}

pub struct Runtime {
    context: Context,
    running: VecDeque<(Evaluation, Purpose)>,
}

impl Runtime {
    /// A context running `pack`'s `script`, after the scripts it requires.
    pub fn start(
        pack: &Pack,
        script: &str,
        requires: &[String],
        snapshot: &Value,
    ) -> Result<Runtime, String> {
        let context = Context::new().map_err(|error| error.to_string())?;
        let config = json!({
            "id": pack.id,
            "name": pack.name,
            "version": pack.version,
            "permissions": pack.permissions,
        });
        let mut source = format!("var __erikaConfig = {config};\n{HOST}\n{}\n", liberika_plugin::PRELUDE);
        for required in requires {
            source.push_str(required);
            source.push_str("\n;\n");
        }
        source.push_str(script);
        // Read the plugin's declarations in the same evaluation: a web
        // view's context forgets top-level `const` once it ends.
        source.push_str(
            "\n;globalThis.__erikaPlugin = {\n\
             get manifest() { return typeof pluginManifest === 'undefined' ? undefined : pluginManifest; },\n\
             get entries() { return typeof pluginUIEntries === 'undefined' ? undefined : pluginUIEntries; },\n\
             get blockWords() { return typeof pluginBlockWords === 'undefined' ? undefined : pluginBlockWords; },\n\
             get onInitialize() { return typeof pluginOnInitialize === 'function' ? pluginOnInitialize : undefined; },\n\
             get onDestroy() { return typeof pluginOnDestroy === 'function' ? pluginOnDestroy : undefined; },\n\
             get onResume() { return typeof pluginOnResume === 'function' ? pluginOnResume : undefined; },\n\
             get onSuspend() { return typeof pluginOnSuspend === 'function' ? pluginOnSuspend : undefined; },\n\
             get onEvent() { return typeof pluginOnEvent === 'function' ? pluginOnEvent : undefined; },\n\
             get handleAction() { return typeof pluginHandleUIAction === 'function' ? pluginHandleUIAction : undefined; },\n\
             get resolveUrl() { return typeof pluginResolveUrl === 'function' ? pluginResolveUrl : undefined; },\n\
             get downloadSuggestions() { return typeof pluginDownloadSuggestions === 'function' ? pluginDownloadSuggestions : undefined; },\n\
             };\n",
        );
        source.push_str(&format!("__erikaLoad({snapshot});\n"));
        let mut runtime = Runtime {
            context,
            running: VecDeque::new(),
        };
        runtime.eval(source, Purpose::Load);
        Ok(runtime)
    }

    fn eval(&mut self, source: String, purpose: Purpose) {
        let evaluation = self.context.eval(source);
        self.running.push_back((evaluation, purpose));
    }

    /// Call the plugin's handler `name` (see `__erikaPlugin`) with `arg`.
    pub fn invoke(
        &mut self,
        name: &str,
        arg: Option<&Value>,
        snapshot: &Value,
        tag: u64,
        purpose: Purpose,
    ) {
        let arg = arg.map_or("undefined".to_string(), Value::to_string);
        self.eval(
            format!("__erikaInvoke({}, {arg}, {snapshot}, {tag});", json!(name)),
            purpose,
        );
    }

    /// The answer to the plugin's request `id`.
    pub fn settle(&mut self, id: u64, answer: Result<Value, String>, snapshot: &Value) {
        let (response, error) = match answer {
            Ok(response) => (response, Value::Null),
            Err(error) => (Value::Null, json!(error)),
        };
        self.eval(
            format!("__erikaSettle({id}, {response}, {error}, {snapshot});"),
            Purpose::Settle,
        );
        // What the request's continuations queue runs after the answer's
        // evaluation has read the queue: read it once more.
        self.drain();
    }

    pub fn drain(&mut self) {
        self.eval("__erikaReply(null, null);".into(), Purpose::Settle);
    }

    /// The evaluations that finished, in the order they were made; the
    /// others wake the engine when they do.
    pub fn finished(&mut self, waker: &Waker) -> Vec<(Purpose, Result<Value, String>)> {
        let mut cx = TaskContext::from_waker(waker);
        let mut done = Vec::new();
        let mut index = 0;
        while index < self.running.len() {
            let (evaluation, _) = &mut self.running[index];
            match Pin::new(evaluation).poll(&mut cx) {
                Poll::Ready(result) => {
                    let (_, purpose) = self.running.remove(index).unwrap();
                    done.push((purpose, result.map_err(|error| error.to_string())));
                }
                Poll::Pending => index += 1,
            }
        }
        done
    }
}
