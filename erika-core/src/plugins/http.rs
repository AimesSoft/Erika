//! Bounded plugin HTTP. A request belongs to one runtime/resolve round, never
//! to a URL-keyed global cache. Dropping Pending invalidates its completion.
use serde_json::{Map, Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(not(target_arch = "wasm32"))]
mod native;

pub(super) struct Request {
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    resolver: bool,
    timeout: Duration,
    limit: usize,
}

impl Request {
    pub fn resolver(url: &str, headers: &Value) -> Result<Self, String> {
        let mut request = Self::fetch(url, &json!({"headers":headers}))?;
        request.resolver = true;
        request.timeout = Duration::from_secs(20);
        request.limit = 4 << 20;
        Ok(request)
    }

    pub fn fetch(url: &str, options: &Value) -> Result<Self, String> {
        if !super::links::valid_media_url(url) {
            return Err("插件请求地址格式不正确".into());
        }
        if !options.is_null() && !options.is_object() {
            return Err("插件请求选项格式不正确".into());
        }
        let method = match options.get("method") {
            None => "GET".into(),
            Some(Value::String(method)) => method.to_ascii_uppercase(),
            _ => return Err("插件请求方法格式不正确".into()),
        };
        if method.is_empty()
            || method.len() > 32
            || !method
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c))
            || matches!(method.as_str(), "CONNECT" | "TRACE" | "TRACK")
        {
            return Err("插件请求方法不可用".into());
        }
        let body = match options.get("body") {
            None | Some(Value::Null) => None,
            Some(Value::String(body)) => Some(body.clone()),
            Some(body) => Some(body.to_string()),
        };
        if body.as_ref().is_some_and(|body| body.len() > 16 << 20)
            || body.is_some() && matches!(method.as_str(), "GET" | "HEAD")
        {
            return Err("插件请求正文过大或不适用于此方法".into());
        }
        let headers = super::links::playback_headers(options.get("headers"))
            .map_err(|_| "插件请求头格式不正确".to_string())?;
        Ok(Self {
            url: url.into(),
            method,
            headers,
            body,
            resolver: false,
            timeout: Duration::from_secs(60),
            limit: 16 << 20,
        })
    }

    fn response(
        &self,
        status: u16,
        headers: Map<String, Value>,
        bytes: Vec<u8>,
    ) -> Result<Value, String> {
        let body = if self.resolver {
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            String::from_utf8(bytes).map_err(|_| "插件响应不是 UTF-8 文本".to_string())?
        };
        Ok(if self.resolver {
            json!({"url":self.url,"status":status,"headers":headers,"body":body})
        } else {
            json!({"status":status,"ok":(200..300).contains(&status),"headers":headers,"body":body})
        })
    }
}

pub(super) struct Pending {
    cancelled: Arc<AtomicBool>,
    #[cfg(target_arch = "wasm32")]
    _cancel: futures_channel::oneshot::Sender<()>,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

pub(super) fn start(
    request: Result<Request, String>,
    complete: impl FnOnce(Result<Value, String>) + Send + 'static,
) -> Pending {
    let cancelled = Arc::new(AtomicBool::new(false));
    let active = cancelled.clone();
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::spawn(move || {
            if active.load(Ordering::Acquire) {
                return;
            }
            let answer = request.and_then(|request| native::execute(&request, &active));
            if !active.load(Ordering::Acquire) {
                complete(answer);
            }
        });
        Pending { cancelled }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let (send, receive) = futures_channel::oneshot::channel::<()>();
        wasm_bindgen_futures::spawn_local(async move {
            let answer = futures_lite::future::or(
                async {
                    let _ = receive.await;
                    None
                },
                async {
                    Some(match request {
                        Ok(request) => browser::execute(&request).await,
                        Err(error) => Err(error),
                    })
                },
            )
            .await;
            if let Some(answer) = answer
                && !active.load(Ordering::Acquire)
            {
                complete(answer);
            }
        });
        Pending {
            cancelled,
            _cancel: send,
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
