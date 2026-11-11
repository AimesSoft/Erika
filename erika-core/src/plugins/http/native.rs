use super::*;
use std::io::Read;

pub(super) fn execute(request: &Request, cancelled: &AtomicBool) -> Result<Value, String> {
    let mut config = ureq::Agent::config_builder()
        .timeout_global(Some(request.timeout))
        .http_status_as_error(false);
    if request.resolver {
        config = config.max_redirects(0);
    }
    let agent: ureq::Agent = config.build().into();
    let mut outgoing = ureq::http::Request::builder()
        .method(request.method.as_str())
        .uri(&request.url);
    for (key, value) in &request.headers {
        outgoing = outgoing.header(key.as_str(), value);
    }
    let outgoing = outgoing
        .body(request.body.clone().unwrap_or_default())
        .map_err(|_| "插件请求格式不正确".to_string())?;
    // Never return ureq's URL-bearing diagnostic to a plugin's public error UI.
    let mut response = agent
        .run(outgoing)
        .map_err(|_| "插件网络请求失败或超时".to_string())?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(key, value)| {
            Some((
                key.as_str().to_ascii_lowercase(),
                json!(value.to_str().ok()?),
            ))
        })
        .collect();
    let mut reader = response.body_mut().as_reader();
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("插件请求已取消".into());
        }
        let read = reader
            .read(&mut chunk)
            .map_err(|_| "插件响应读取失败或超时".to_string())?;
        if read == 0 {
            break;
        }
        if bytes.len() + read > request.limit {
            return Err("插件响应过大".into());
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    request.response(status, headers, bytes)
}
