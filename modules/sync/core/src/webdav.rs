//! A WebDAV folder as a sync repository's place, as NipaPlay's WebDAV
//! sync talks to it: PROPFIND to look, MKCOL to make folders, GET, PUT
//! and DELETE for files; passing errors are tried again up to three times.

use crate::repository::{SyncError, Transport};
use base64::Engine;
use std::time::Duration;

const ATTEMPTS: u32 = 3;
const PROPFIND: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:"><D:prop><D:displayname/><D:resourcetype/></D:prop></D:propfind>"#;

pub struct WebDav {
    agent: ureq::Agent,
    /// The server's address, without a trailing slash.
    base: String,
    auth: Option<String>,
}

/// `path` with every byte outside the unreserved set percent-encoded; its
/// slashes stay.
fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn decode_path(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && let Some(byte) = text
                .get(at + 1..at + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            at += 3;
            continue;
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What a status means, for the user.
fn status_message(status: u16) -> String {
    match status {
        401 => "身份验证失败，请检查用户名和密码".into(),
        403 => "服务器拒绝访问，请检查账号和目录权限".into(),
        404 => "远端路径不存在".into(),
        405 | 501 => "服务器不支持这个 WebDAV 操作".into(),
        409 => "上级目录不存在".into(),
        429 => "服务器请求过于频繁，请稍后重试".into(),
        502..=504 => "服务器暂时不可用，请稍后重试".into(),
        507 => "服务器空间不足".into(),
        _ => format!("服务器返回 HTTP {status}"),
    }
}

impl WebDav {
    /// The server at `url`, signed in as `user` when one is given.
    pub fn new(url: &str, user: &str, password: &str) -> Result<WebDav, SyncError> {
        let url = url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(SyncError::new("WebDAV 地址需要以 http:// 或 https:// 开头"));
        }
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .allow_non_standard_methods(true)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_global(Some(Duration::from_secs(120)))
            .user_agent("WebDAVFS/3.0 (Erika)")
            .build();
        let auth = (!user.trim().is_empty()).then(|| {
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{password}", user.trim()))
            )
        });
        Ok(WebDav {
            agent: config.into(),
            base: url.trim_end_matches('/').to_string(),
            auth,
        })
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base,
            encode_path(path.trim_start_matches('/'))
        )
    }

    /// One request, tried again on errors that pass; the status and body.
    fn request(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        what: &str,
    ) -> Result<(u16, Vec<u8>), SyncError> {
        let mut last = SyncError::new(format!("{what}失败"));
        for attempt in 1..=ATTEMPTS {
            let mut builder = ureq::http::Request::builder()
                .method(method)
                .uri(self.url(path));
            if let Some(auth) = &self.auth {
                builder = builder.header("Authorization", auth);
            }
            for (name, value) in headers {
                builder = builder.header(*name, *value);
            }
            let request = builder
                .body(body.to_vec())
                .map_err(|error| SyncError::new(format!("{what}失败：{error}")))?;
            match self.agent.run(request) {
                Ok(mut response) => {
                    let status = response.status().as_u16();
                    let bytes = response
                        .body_mut()
                        .with_config()
                        .limit(1 << 30)
                        .read_to_vec()
                        .unwrap_or_default();
                    if matches!(status, 408 | 425 | 429 | 500 | 502 | 503 | 504)
                        && attempt < ATTEMPTS
                    {
                        last = SyncError {
                            message: format!("{what}失败：{}", status_message(status)),
                            status: Some(status),
                        };
                    } else {
                        return Ok((status, bytes));
                    }
                }
                Err(error) => {
                    let message = match error {
                        ureq::Error::Timeout(_) => {
                            "连接服务器超时，请检查网络或服务器地址".to_string()
                        }
                        ureq::Error::HostNotFound => "找不到服务器，请检查地址".to_string(),
                        ureq::Error::ConnectionFailed | ureq::Error::Io(_) => {
                            "无法连接服务器，请检查网络、地址和证书".to_string()
                        }
                        other => other.to_string(),
                    };
                    last = SyncError::new(format!("{what}失败：{message}"));
                }
            }
            std::thread::sleep(Duration::from_millis(600 * u64::from(attempt * attempt)));
        }
        Err(last)
    }

    fn failed(what: &str, status: u16) -> SyncError {
        SyncError {
            message: format!("{what}失败：{}", status_message(status)),
            status: Some(status),
        }
    }

    fn propfind(&self, path: &str, depth: &str) -> Result<(u16, Vec<u8>), SyncError> {
        let folder = format!("{}/", path.trim_end_matches('/'));
        self.request(
            "PROPFIND",
            &folder,
            &[
                ("Depth", depth),
                ("Content-Type", "text/xml; charset=\"utf-8\""),
                ("Accept", "application/xml,text/xml,*/*"),
            ],
            PROPFIND.as_bytes(),
            "读取同步目录",
        )
    }
}

impl Transport for WebDav {
    fn ensure_dir(&mut self, path: &str) -> Result<(), SyncError> {
        let (status, _) = self.propfind(path, "0")?;
        if matches!(status, 200 | 207) {
            return Ok(());
        }
        if status != 404 {
            return Err(Self::failed("检查同步目录", status));
        }
        // Make each folder down the path that is not there yet.
        let mut made = String::new();
        for part in path.split('/').filter(|part| !part.is_empty()) {
            made.push('/');
            made.push_str(part);
            let (status, _) =
                self.request("MKCOL", &format!("{made}/"), &[], &[], "创建同步目录")?;
            // 405: it is there already.
            if !matches!(status, 200 | 201 | 204 | 405) {
                return Err(Self::failed("创建同步目录", status));
            }
        }
        Ok(())
    }

    fn list(&mut self, path: &str) -> Result<Vec<String>, SyncError> {
        let (status, body) = self.propfind(path, "1")?;
        if !matches!(status, 200 | 207) {
            return Err(Self::failed("读取同步目录", status));
        }
        let text = String::from_utf8_lossy(&body);
        let document = roxmltree::Document::parse(&text)
            .map_err(|_| SyncError::new("服务器返回的目录列表无法解析"))?;
        let mut names = Vec::new();
        for response in document
            .descendants()
            .filter(|node| node.tag_name().name().eq_ignore_ascii_case("response"))
        {
            let folder = response
                .descendants()
                .any(|node| node.tag_name().name().eq_ignore_ascii_case("collection"));
            let Some(href) = response
                .descendants()
                .find(|node| node.tag_name().name().eq_ignore_ascii_case("href"))
                .and_then(|node| node.text())
            else {
                continue;
            };
            if folder {
                continue;
            }
            let href = href.trim().trim_end_matches('/');
            if let Some(name) = href.rsplit('/').next().map(decode_path)
                && !name.is_empty()
                && !names.contains(&name)
            {
                names.push(name);
            }
        }
        Ok(names)
    }

    fn read(&mut self, path: &str) -> Result<Option<Vec<u8>>, SyncError> {
        let (status, body) = self.request("GET", path, &[], &[], "下载同步文件")?;
        match status {
            200 => Ok(Some(body)),
            404 => Ok(None),
            _ => Err(Self::failed("下载同步文件", status)),
        }
    }

    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), SyncError> {
        let (status, _) = self.request(
            "PUT",
            path,
            &[("Content-Type", "application/octet-stream")],
            bytes,
            "上传同步文件",
        )?;
        match status {
            200 | 201 | 204 => Ok(()),
            _ => Err(Self::failed("上传同步文件", status)),
        }
    }

    fn delete(&mut self, path: &str) -> Result<(), SyncError> {
        let (status, _) = self.request("DELETE", path, &[], &[], "删除同步文件")?;
        match status {
            200 | 202 | 204 | 404 => Ok(()),
            _ => Err(Self::failed("删除同步文件", status)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_encoded_and_decoded_alike() {
        assert_eq!(
            encode_path("/Erika/同步 1/a.erika"),
            "/Erika/%E5%90%8C%E6%AD%A5%201/a.erika"
        );
        assert_eq!(decode_path("%E5%90%8C%E6%AD%A5%201"), "同步 1");
    }

    /// Against a real server: `ERIKA_WEBDAV_TEST="url user password"
    /// cargo test -p erika-sync-core -- --ignored webdav`.
    #[test]
    #[ignore]
    fn files_go_to_a_real_server_and_back() {
        let setting = std::env::var("ERIKA_WEBDAV_TEST").expect("ERIKA_WEBDAV_TEST");
        let mut parts = setting.split(' ');
        let (url, user, password) = (
            parts.next().unwrap(),
            parts.next().unwrap(),
            parts.next().unwrap(),
        );
        let root = format!("/erika-test-{}/同步 目录", crate::random_hex(3));
        let mut dav = WebDav::new(url, user, password).unwrap();
        dav.ensure_dir(&root).unwrap();
        dav.ensure_dir(&root).unwrap();
        let file = format!("{root}/manifest.erika");
        assert_eq!(dav.read(&file).unwrap(), None);
        dav.write(&file, b"one").unwrap();
        dav.write(&file, b"two").unwrap();
        dav.write(&format!("{root}/patch_v1_一.erika"), b"p")
            .unwrap();
        assert_eq!(dav.read(&file).unwrap().as_deref(), Some(&b"two"[..]));
        let mut names = dav.list(&root).unwrap();
        names.sort();
        assert_eq!(names, ["manifest.erika", "patch_v1_一.erika"]);
        dav.delete(&file).unwrap();
        dav.delete(&file).unwrap();
        assert_eq!(dav.read(&file).unwrap(), None);
        let wrong = WebDav::new(url, user, "wrong")
            .unwrap()
            .read(&file)
            .unwrap_err();
        assert_eq!(wrong.status, Some(401));
    }
}
