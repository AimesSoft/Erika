//! Bounded read-only transport shared by public metadata and calendar readers.
//! The caller owns the endpoint policy. Errors never contain the requested URL
//! or credentials. Native callers run on a worker; the browser awaits Fetch.
use crate::{Error, Result};
use std::time::Duration;

pub struct Response {
    pub status: u16,
    pub retry_after: Option<u64>,
    pub body: Vec<u8>,
}
impl Response {
    pub fn text(self) -> Result<String> {
        String::from_utf8(self.body).map_err(|_| Error::invalid("服务响应不是 UTF-8 文本"))
    }
    pub fn json(self) -> Result<serde_json::Value> {
        serde_json::from_slice(&self.body).map_err(|_| Error::invalid("服务响应格式不正确"))
    }
}

pub async fn get(
    url: &str,
    headers: &[(&str, &str)],
    timeout: Duration,
    limit: usize,
    follow_redirects: bool,
) -> Result<Response> {
    let parsed = url::Url::parse(url).map_err(|_| Error::invalid("服务地址不正确"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
        || timeout.as_millis() == 0
        || timeout.as_millis() > u32::MAX as u128
        || limit == 0
    {
        return Err(Error::invalid("服务请求参数不正确"));
    }
    execute(url, headers, timeout, limit, follow_redirects).await
}

#[cfg(not(target_arch = "wasm32"))]
async fn execute(
    url: &str,
    headers: &[(&str, &str)],
    timeout: Duration,
    limit: usize,
    follow_redirects: bool,
) -> Result<Response> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(if follow_redirects { 5 } else { 0 })
        .http_status_as_error(false)
        .build()
        .into();
    let mut request = agent.get(url);
    for (key, value) in headers {
        request = request.header(*key, *value);
    }
    let mut response = request.call().map_err(|_| Error::unavailable())?;
    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    let body = response
        .body_mut()
        .with_config()
        // ureq's limit is exclusive: allow one byte for the EOF probe, then
        // enforce the same inclusive byte budget as the browser reader.
        .limit((limit as u64).saturating_add(1))
        .read_to_vec()
        .map_err(|error| match error {
            ureq::Error::BodyExceedsLimit(_) => Error::invalid("服务响应过大"),
            _ => Error::unavailable(),
        })?;
    if body.len() > limit {
        return Err(Error::invalid("服务响应过大"));
    }
    Ok(Response {
        status,
        retry_after,
        body,
    })
}

#[cfg(target_arch = "wasm32")]
async fn execute(
    url: &str,
    headers: &[(&str, &str)],
    timeout: Duration,
    limit: usize,
    follow_redirects: bool,
) -> Result<Response> {
    use gloo_net::http::Request;
    use js_sys::{Reflect, Uint8Array};
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{
        AbortController, ReadableStreamDefaultReader, RequestCredentials, RequestRedirect,
    };
    struct Abort(AbortController);
    impl Drop for Abort {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    struct Reader(ReadableStreamDefaultReader);
    impl Drop for Reader {
        fn drop(&mut self) {
            let cancelled = self.0.cancel();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = JsFuture::from(cancelled).await;
            });
            self.0.release_lock();
        }
    }
    let fetch = async {
        let abort = Abort(AbortController::new().map_err(|_| Error::unavailable())?);
        let mut request = Request::get(url)
            .credentials(RequestCredentials::Omit)
            .redirect(if follow_redirects {
                RequestRedirect::Follow
            } else {
                RequestRedirect::Error
            })
            .abort_signal(Some(&abort.0.signal()));
        for (key, value) in headers {
            // Fetch owns User-Agent; native public APIs still receive the app's UA.
            if !key.eq_ignore_ascii_case("user-agent") {
                request = request.header(key, value);
            }
        }
        let response = request.send().await.map_err(|_| Error::unavailable())?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.parse().ok());
        if response
            .headers()
            .get("content-length")
            .and_then(|v| v.parse::<usize>().ok())
            .is_some_and(|n| n > limit)
        {
            return Err(Error::invalid("服务响应过大"));
        }
        let mut body = Vec::new();
        if let Some(stream) = response.body() {
            let reader = Reader(
                stream
                    .get_reader()
                    .dyn_into()
                    .map_err(|_| Error::unavailable())?,
            );
            loop {
                let part = JsFuture::from(reader.0.read())
                    .await
                    .map_err(|_| Error::unavailable())?;
                if Reflect::get(&part, &JsValue::from_str("done"))
                    .ok()
                    .and_then(|v| v.as_bool())
                    == Some(true)
                {
                    break;
                }
                let value = Reflect::get(&part, &JsValue::from_str("value"))
                    .map_err(|_| Error::unavailable())?;
                let bytes = value
                    .dyn_into::<Uint8Array>()
                    .map_err(|_| Error::unavailable())?;
                let length = bytes.length() as usize;
                if length > limit.saturating_sub(body.len()) {
                    return Err(Error::invalid("服务响应过大"));
                }
                let start = body.len();
                body.resize(start + length, 0);
                bytes.copy_to(&mut body[start..]);
            }
        }
        Ok(Response {
            status,
            retry_after,
            body,
        })
    };
    let deadline = async {
        gloo_timers::future::TimeoutFuture::new(timeout.as_millis() as u32).await;
        Err(Error::unavailable())
    };
    futures_lite::future::or(deadline, fetch).await
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
