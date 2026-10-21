//! The requests themselves, with the gateway failover of NipaPlay's
//! `NipaplayServerRouter`: a gateway that fails or answers with a server
//! error gives way to the next; the one that answered is tried first next
//! time.

use crate::{APP_ID, Error, GATEWAYS, Result, USER_AGENT, now, signature};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// The gateway that answered last.
static PREFERRED: AtomicUsize = AtomicUsize::new(0);

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Send one request and return the body of the answer.
pub(crate) fn send(
    method: &str,
    path: &str,
    query: &str,
    body: Option<serde_json::Value>,
    token: Option<&str>,
) -> Result<String> {
    let agent = agent();
    let first = PREFERRED.load(Ordering::Relaxed) % GATEWAYS.len();
    let mut last = Error::new("连不上弹弹play");
    for step in 0..GATEWAYS.len() {
        let index = (first + step) % GATEWAYS.len();
        let url = format!("{}{path}{query}", GATEWAYS[index]);
        match attempt(&agent, method, &url, path, body.as_ref(), token) {
            Ok(Some(text)) => {
                PREFERRED.store(index, Ordering::Relaxed);
                return Ok(text);
            }
            // The gateway itself failed: try the next.
            Ok(None) => last = Error::new("弹弹play的网关暂时不可用"),
            Err(Failure::Network(message)) => last = Error::new(format!("连不上弹弹play：{message}")),
            Err(Failure::Answer(error)) => return Err(error),
        }
    }
    Err(last)
}

enum Failure {
    /// The request did not get through.
    Network(String),
    /// The service answered with an error the user should see.
    Answer(Error),
}

/// One gateway's answer: its body, `None` when the gateway failed, or the
/// service's error.
fn attempt(
    agent: &ureq::Agent,
    method: &str,
    url: &str,
    path: &str,
    body: Option<&serde_json::Value>,
    token: Option<&str>,
) -> std::result::Result<Option<String>, Failure> {
    let timestamp = now();
    let signature = signature(timestamp, path);
    let timestamp = timestamp.to_string();
    let bearer = token.map(|token| format!("Bearer {token}"));
    macro_rules! headers {
        ($request:expr) => {{
            let mut request = $request
                .header("Accept", "application/json")
                .header("User-Agent", USER_AGENT)
                .header("X-AppId", APP_ID)
                .header("X-Signature", &signature)
                .header("X-Timestamp", &timestamp);
            if let Some(bearer) = &bearer {
                request = request.header("Authorization", bearer);
            }
            request
        }};
    }
    let result = match method {
        "POST" => {
            let request = headers!(agent.post(url)).header("Content-Type", "application/json");
            match body {
                Some(body) => request.send_json(body),
                None => request.send_empty(),
            }
        }
        _ => headers!(agent.get(url)).call(),
    };
    let mut response = result.map_err(|error| Failure::Network(error.to_string()))?;
    let status = response.status().as_u16();
    let message = response
        .headers()
        .get("x-error-message")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|error| Failure::Network(error.to_string()))?;
    match status {
        200..=299 => Ok(Some(text)),
        401 | 403 => Err(Failure::Answer(Error {
            message: message
                .or_else(|| error_message(&text))
                .unwrap_or_else(|| "登录已失效，请重新登录弹弹play".into()),
            sign_in: true,
        })),
        500..=599 => Ok(None),
        _ => Err(Failure::Answer(Error::new(
            message
                .or_else(|| error_message(&text))
                .unwrap_or_else(|| format!("弹弹play回答了 {status}")),
        ))),
    }
}

/// The `errorMessage` of a JSON answer.
fn error_message(text: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()?
        .get("errorMessage")?
        .as_str()
        .filter(|message| !message.is_empty())
        .map(str::to_string)
}
