//! Short reviews of a show, distinct from episode danmaku.
use crate::{Error, Result, request};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};

static BANGUMI_SIGNED_IN: AtomicBool = AtomicBool::new(false);
pub fn set_bangumi_signed_in(value: bool) {
    BANGUMI_SIGNED_IN.store(value, Ordering::Relaxed);
}
pub fn bangumi_signed_in() -> bool {
    BANGUMI_SIGNED_IN.load(Ordering::Relaxed)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Review {
    pub id: String,
    pub author: String,
    pub avatar: String,
    pub text: String,
    pub rating: i64,
    pub updated: String,
}

/// A local request using the user's token, or the narrow public live-review
/// endpoint for a Bangumi-only account. No app secret is sent to clients.
pub fn dandanplay(id: i64, live: bool, token: Option<&str>) -> Result<Vec<Review>> {
    let path = match (live, token) {
        (true, Some(_)) => format!("/api/v2/bangumi/{id}/comments"),
        (true, None) => format!("/public/live/comments/{id}"),
        (false, _) => format!("/public/comments/{id}"),
    };
    let query = if live && token.is_some() {
        "?page=0"
    } else {
        ""
    };
    let body: Value = request("GET", &path, query, None, if live { token } else { None })?;
    parse_dandanplay(&body)
}

fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

pub fn parse_dandanplay(body: &Value) -> Result<Vec<Review>> {
    if body.get("success") == Some(&Value::Bool(false)) {
        return Err(Error::new("弹弹play评论暂时不可用"));
    }
    let rows = body
        .get("comments")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::new("弹弹play评论格式不正确"))?;
    let mut result: Vec<Review> = rows
        .iter()
        .map(|row| Review {
            id: row.get("id").map(Value::to_string).unwrap_or_else(|| {
                format!("{}:{}", string(row, "externalUserId"), string(row, "text"))
            }),
            author: string(row, "userName"),
            avatar: string(row, "imageUrl"),
            text: string(row, "text"),
            rating: row
                .get("rating")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            updated: string(row, "updatedTime"),
        })
        .collect();
    result.sort_by(|a, b| b.updated.cmp(&a.updated));
    let mut ids = std::collections::HashSet::new();
    result.retain(|r| ids.insert(r.id.clone()));
    result.truncate(20);
    Ok(result)
}

/// Parse only a verified subject link; the similarly named dandanplay ID is
/// never treated as a Bangumi subject.
pub fn subject_id(url: &str) -> Option<i64> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/')?;
    if !matches!(host, "bgm.tv" | "bangumi.tv" | "chii.in") {
        return None;
    }
    let id = path
        .strip_prefix("subject/")?
        .split(['/', '?', '#'])
        .next()?
        .parse::<i64>()
        .ok()?;
    (id > 0).then_some(id)
}

pub async fn bangumi(subject: i64) -> Result<Vec<Review>> {
    if crate::bangumi::local_id(subject).is_none() {
        return Err(Error::new("无效的 Bangumi 条目"));
    }
    #[cfg(not(target_arch = "wasm32"))]
    let body = {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(4)))
            .max_redirects(0)
            .build()
            .into();
        let text = agent
            .get(format!(
                "https://next.bgm.tv/p1/subjects/{subject}/comments?limit=20&offset=0"
            ))
            .header("User-Agent", &crate::bangumi::user_agent())
            .header("Accept", "application/json")
            .call()
            .and_then(|mut r| {
                r.body_mut()
                    .with_config()
                    .limit(8 * 1024 * 1024)
                    .read_to_string()
            })
            .map_err(|_| Error::new("暂时连不上 Bangumi"))?;
        serde_json::from_str(&text).map_err(|_| Error::new("Bangumi评论格式不正确"))?
    };
    // next.bgm.tv does not allow cross-origin browser requests. The fixed public
    // gateway forwards no website cookie, account token or caller-chosen URL.
    #[cfg(target_arch = "wasm32")]
    let body = erika_catalog::client::Client::built_in("")
        .map_err(|error| Error::new(error.message))?
        .get(&format!("bangumi/subjects/{subject}/comments"), &[])
        .await
        .map_err(|error| Error::new(error.message))?;
    parse_bangumi(&body)
}

pub fn parse_bangumi(body: &Value) -> Result<Vec<Review>> {
    let rows = body
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::new("Bangumi评论格式不正确"))?;
    let mut rows = rows.clone();
    rows.sort_by_key(|r| {
        std::cmp::Reverse(
            r.get("updatedAt")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
        )
    });
    Ok(rows
        .iter()
        .take(20)
        .enumerate()
        .map(|(i, row)| {
            let user = &row["user"];
            let mut author = string(user, "nickname");
            if author.is_empty() {
                author = string(user, "username");
            }
            Review {
                id: format!("{}:{i}", user["id"]),
                author,
                avatar: string(&user["avatar"], "medium"),
                text: string(row, "comment"),
                rating: row["rate"].as_i64().unwrap_or_default(),
                updated: row["updatedAt"].as_i64().unwrap_or_default().to_string(),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn subject_links_never_confuse_the_two_services_ids() {
        assert_eq!(
            subject_id("https://bangumi.tv/subject/393010"),
            Some(393010)
        );
        assert_eq!(subject_id("17527"), None);
        assert_eq!(subject_id("https://example.com/subject/393010"), None);
    }
    #[test]
    fn reviews_are_newest_first_deduplicated_and_limited() {
        let mut rows: Vec<_> = (1..=25)
            .map(|i| json!({"id":i,"text":"评论","updatedTime":format!("2026-10-{i:02}")}))
            .collect();
        rows.push(rows[24].clone());
        let reviews = parse_dandanplay(&json!({"comments":rows})).unwrap();
        assert_eq!(reviews.len(), 20);
        assert_eq!(reviews[0].id, "25");
        assert!(parse_dandanplay(&json!({"success":false,"comments":[]})).is_err());
        assert_eq!(parse_bangumi(&json!({"data":[]})).unwrap(), vec![]);
        assert!(parse_bangumi(&json!({"error":"offline"})).is_err());
    }
}
