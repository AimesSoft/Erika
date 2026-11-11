use super::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortController, ReadableStreamDefaultReader, RequestCredentials, RequestRedirect};

struct AbortOnDrop(AbortController);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct Reader(ReadableStreamDefaultReader);
impl Drop for Reader {
    fn drop(&mut self) {
        // Consume cancellation rejection too; dropping a timed-out read must not
        // leave an unhandled Promise rejection or a locked response stream.
        let cancelled = self.0.cancel();
        wasm_bindgen_futures::spawn_local(async move {
            let _ = JsFuture::from(cancelled).await;
        });
        self.0.release_lock();
    }
}

pub(super) async fn execute(request: &Request) -> Result<Value, String> {
    let timeout = async {
        gloo_timers::future::TimeoutFuture::new(request.timeout.as_millis() as u32).await;
        Err("插件网络请求超时".into())
    };
    futures_lite::future::or(timeout, fetch(request)).await
}

async fn fetch(request: &Request) -> Result<Value, String> {
    let abort = AbortOnDrop(AbortController::new().map_err(|_| "浏览器无法创建请求".to_string())?);
    let mut outgoing = gloo_net::http::RequestBuilder::new(&request.url)
        .method(
            request
                .method
                .parse()
                .map_err(|_| "插件请求方法格式不正确".to_string())?,
        )
        .credentials(RequestCredentials::Omit)
        .redirect(if request.resolver {
            RequestRedirect::Manual
        } else {
            RequestRedirect::Follow
        })
        .abort_signal(Some(&abort.0.signal()));
    for (key, value) in &request.headers {
        if forbidden_header(key, value) {
            return Err("浏览器不允许设置此请求头，请使用签名地址或来源支持的认证方式".into());
        }
        outgoing = outgoing.header(key, value);
    }
    let outgoing = match &request.body {
        Some(body) => outgoing.body(body.as_str()),
        None => outgoing.build(),
    }
    .map_err(|_| "浏览器无法创建请求".to_string())?;
    let response = outgoing
        .send()
        .await
        .map_err(|_| "浏览器请求失败，请检查网络、HTTPS 和服务器 CORS 配置".to_string())?;
    let status = response.status();
    if status == 0 {
        // Browsers deliberately hide manual redirects, including Location.
        // Never turn an opaque redirect into success or silently follow it.
        return Err("浏览器不能读取此重定向；来源需要提供可直接请求的接口".into());
    }
    let headers = response
        .headers()
        .entries()
        .map(|(key, value)| (key.to_ascii_lowercase(), json!(value)))
        .collect();
    if response
        .headers()
        .get("content-length")
        .and_then(|s| s.parse::<u64>().ok())
        .is_some_and(|len| len > request.limit as u64)
    {
        return Err("插件响应过大".into());
    }
    let mut bytes = Vec::new();
    if let Some(stream) = response.body() {
        let reader = Reader(
            stream
                .get_reader()
                .dyn_into::<ReadableStreamDefaultReader>()
                .map_err(|_| "浏览器无法读取响应流".to_string())?,
        );
        loop {
            let chunk = JsFuture::from(reader.0.read())
                .await
                .map_err(|_| "插件响应读取失败".to_string())?;
            if js_sys::Reflect::get(&chunk, &JsValue::from_str("done"))
                .ok()
                .and_then(|v| v.as_bool())
                == Some(true)
            {
                break;
            }
            let value = js_sys::Reflect::get(&chunk, &JsValue::from_str("value"))
                .map_err(|_| "插件响应流格式不正确".to_string())?;
            let data = value
                .dyn_into::<js_sys::Uint8Array>()
                .map_err(|_| "插件响应流格式不正确".to_string())?;
            let length = data.length() as usize;
            if length > request.limit.saturating_sub(bytes.len()) {
                return Err("插件响应过大".into());
            }
            let start = bytes.len();
            bytes.resize(start + length, 0);
            data.copy_to(&mut bytes[start..]);
        }
    }
    request.response(status, headers, bytes)
}

fn forbidden_header(key: &str, value: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.starts_with("proxy-")
        || key.starts_with("sec-")
        || [
            "accept-charset",
            "accept-encoding",
            "access-control-request-headers",
            "access-control-request-method",
            "cookie",
            "cookie2",
            "date",
            "dnt",
            "expect",
            "origin",
            "permissions-policy",
            "referer",
            "user-agent",
            "via",
        ]
        .contains(&key.as_str())
        || [
            "x-http-method",
            "x-http-method-override",
            "x-method-override",
        ]
        .contains(&key.as_str())
            && value.split(',').any(|v| {
                matches!(
                    v.trim().to_ascii_uppercase().as_str(),
                    "CONNECT" | "TRACE" | "TRACK"
                )
            })
}
