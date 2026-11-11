//! NipaPlay's `/api/media/local/share` API. Fetch a complete snapshot before
//! changing the library, so a failed refresh keeps the last available files.
use erika_store::{Anime, Episode, Fingerprint};
use serde_json::Value;
use url::Url;

pub const KIND: &str = "nipaplay";

/// A video a shared device serves: where it streams from, its file name,
/// which file it is and, when the device matched it, the episode.
pub struct Video {
    pub url: String,
    pub name: String,
    pub fingerprint: Fingerprint,
    pub matched: Option<(Anime, Episode)>,
}

fn local(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']);
    host == "localhost"
        || host.ends_with(".local")
        || host.parse::<std::net::IpAddr>().is_ok_and(|ip| match ip {
            std::net::IpAddr::V4(ip) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
            std::net::IpAddr::V6(ip) => {
                ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()
            }
        })
}

pub fn address(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("请填写共享设备的访问地址".into());
    }
    let text = if input.contains("://") {
        input.to_owned()
    } else {
        format!("http://{input}")
    };
    let mut url = Url::parse(&text).map_err(|_| "访问地址格式不正确".to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("请填写共享设备的 HTTP 或 HTTPS 地址".into());
    }
    // URL normalizes explicit :80 to None, so inspect the authority too.
    let authority = text
        .split("://")
        .nth(1)
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("");
    let explicit_port = if authority.starts_with('[') {
        authority.contains("]:")
    } else {
        authority.contains(':')
    };
    if url.scheme() == "http" && !explicit_port && local(url.host_str().unwrap_or("")) {
        url.set_port(Some(1180))
            .map_err(|_| "访问地址格式不正确".to_string())?;
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn number(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("").trim()
}

fn media_url(base: &str, path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    // Keep a reverse proxy's base path, as NipaPlay's client does.
    let root = Url::parse(&format!("{base}/")).ok()?;
    let url = root.join(path.trim_start_matches('/')).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

/// Whether `url` is on the same scheme, host and port as the share at
/// `base`.
fn same_origin(base: &str, url: &str) -> bool {
    match (Url::parse(base), Url::parse(url)) {
        (Ok(base), Ok(url)) => base.origin() == url.origin(),
        _ => false,
    }
}

fn parse_videos(base: &str, summary: &Value, payload: &Value) -> Result<Vec<Video>, String> {
    let id = number(&summary["animeId"])
        .filter(|id| *id > 0)
        .ok_or_else(|| "共享媒体返回了无效的番剧信息".to_string())?;
    let detail = payload.get("data").unwrap_or(payload);
    let episodes = detail["episodes"]
        .as_array()
        .ok_or_else(|| "共享媒体返回了无效的剧集列表".to_string())?;
    let title = match text(summary, "nameCn") {
        "" => text(summary, "name"),
        name => name,
    };
    let raw_image = text(summary, "imageUrl");
    let image = if raw_image.is_empty() {
        String::new()
    } else if raw_image.starts_with("http://") || raw_image.starts_with("https://") {
        raw_image.to_string()
    } else {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::URL_SAFE.encode(raw_image);
        let mut url = Url::parse(&format!("{base}/api/image_proxy"))
            .map_err(|_| "访问地址格式不正确".to_string())?;
        url.query_pairs_mut().append_pair("url", &encoded);
        url.into()
    };
    let anime = Anime {
        dandanplay_id: id,
        title: title.to_owned(),
        image,
    };
    let mut videos = Vec::new();
    for (index, episode) in episodes.iter().enumerate() {
        if episode["fileExists"].as_bool() == Some(false) {
            continue;
        }
        let Some(episode_id) = number(&episode["episodeId"]).filter(|id| *id > 0) else {
            continue;
        };
        let stream = text(episode, "streamPath");
        let url = if stream.is_empty() {
            // A file-system path is not a relative stream route. Only an
            // absolute HTTP URL can stand in for a missing streamPath.
            let Ok(original) = Url::parse(text(episode, "originalFilePath")) else {
                continue;
            };
            if !matches!(original.scheme(), "http" | "https") {
                continue;
            }
            original.to_string()
        } else {
            let Some(url) = media_url(base, stream) else {
                continue;
            };
            url
        };
        // Import only streams served by this device, not historical links
        // to files on a different device, regardless of which field holds it.
        if !same_origin(base, &url) {
            continue;
        }
        let size = number(&episode["fileSize"]).unwrap_or(0).max(0) as u64;
        let hash = text(episode, "videoHash");
        let fingerprint =
            if size > 0 && hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                Fingerprint {
                    md5: hash.to_ascii_lowercase(),
                    size,
                }
            } else {
                // A namespaced identity cannot collide with a local file's MD5.
                Fingerprint {
                    md5: format!("nipaplay:{url}"),
                    size: 0,
                }
            };
        videos.push(Video {
            url,
            name: match text(episode, "fileName") {
                "" => text(episode, "title").to_owned(),
                name => name.to_owned(),
            },
            fingerprint,
            matched: Some((
                anime.clone(),
                Episode {
                    dandanplay_id: episode_id,
                    number: number(&episode["episodeNumber"])
                        .or_else(|| crate::names::episode_number(text(episode, "title")))
                        .or_else(|| crate::names::episode_number(text(episode, "fileName")))
                        .unwrap_or(index as i64 + 1)
                        .max(1),
                    title: text(episode, "title").to_owned(),
                },
            )),
        });
    }
    Ok(videos)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn fetch(base: &str) -> Result<Vec<Video>, String> {
    use std::time::Duration;
    let host = Url::parse(base).map_err(|_| "访问地址格式不正确".to_string())?;
    let mut config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false);
    if local(host.host_str().unwrap_or("")) {
        config = config.proxy(None);
    }
    let agent: ureq::Agent = config.build().into();
    let get = |path: &str| -> Result<Value, String> {
        let mut response = agent
            .get(format!("{base}/api/media/local/share/{path}"))
            .header("Accept", "application/json")
            .call()
            .map_err(|_| "无法连接共享设备，请检查地址和远程访问设置".to_string())?;
        if !response.status().is_success() {
            return Err(format!(
                "共享设备返回 HTTP {}，请检查访问地址和远程访问设置",
                response.status().as_u16()
            ));
        }
        let value: Value = response
            .body_mut()
            .read_json()
            .map_err(|_| "该地址没有返回有效的 NipaPlay共享媒体数据".to_string())?;
        if value["success"].as_bool() == Some(false) {
            return Err("共享设备暂时无法提供媒体列表".into());
        }
        Ok(value)
    };
    let payload = get("animes")?;
    let summaries = payload
        .get("items")
        .or_else(|| payload.get("data"))
        .and_then(Value::as_array)
        .ok_or_else(|| "该地址没有返回有效的 NipaPlay共享媒体列表".to_string())?;
    let mut videos = Vec::new();
    for summary in summaries {
        let id = number(&summary["animeId"])
            .filter(|id| *id > 0)
            .ok_or_else(|| "共享媒体返回了无效的番剧信息".to_string())?;
        videos.extend(parse_videos(base, summary, &get(&format!("animes/{id}"))?)?);
    }
    Ok(videos)
}

#[cfg(target_arch = "wasm32")]
pub fn fetch(_: &str) -> Result<Vec<Video>, String> {
    Err("请在桌面客户端或连接桌面客户端的 WebUI 中挂载共享媒体".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Done, LibraryEngine};
    use erika_library_protocol::library::{Commands, SourceStatus};
    use serde_json::json;

    fn summary() -> Value {
        json!({"animeId": 42, "name": "Test", "nameCn": "测试番剧", "imageUrl": "https://example.com/poster.jpg"})
    }

    fn detail() -> Value {
        json!({"success": true, "data": {"episodes": [
            {"episodeId": 795321, "title": "第02话", "fileName": "show - 02.mkv", "fileSize": 1234,
             "videoHash": "0123456789ABCDEF0123456789ABCDEF", "fileExists": true,
             "streamPath": "/api/media/local/share/episodes/abc/stream"},
            {"episodeId": 795322, "title": "第03话", "fileExists": false, "streamPath": "/gone"},
            {"episodeId": 795323, "title": "第04话", "originalFilePath": "emby://server/item"},
            {"episodeId": 795324, "title": "第05话", "streamPath": null,
             "originalFilePath": "http://192.168.31.65:8080/api/media/local/share/episodes/def/stream"},
            {"episodeId": 795325, "title": "第06话", "fileName": "show - 06.mkv",
             "originalFilePath": "https://media.example.com/nipa/api/media/local/share/episodes/ghi/stream"}
        ]}})
    }

    /// An isolated NipaPlay-compatible host, with a bounded request deadline.
    fn server(responses: Vec<(&'static str, u16, Value)>) -> (String, std::thread::JoinHandle<()>) {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            time::{Duration, Instant},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let thread = std::thread::spawn(move || {
            for (path, status, body) in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "expected request for {path}");
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                }
                assert!(
                    String::from_utf8_lossy(&request).starts_with(&format!("GET {path} HTTP/1.1"))
                );
                let body = body.to_string();
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        (base, thread)
    }

    #[test]
    fn addresses_follow_nipaplay_ports_and_preserve_proxy_paths() {
        for (input, expected) in [
            (" 192.168.1.100 ", "http://192.168.1.100:1180"),
            ("localhost", "http://localhost:1180"),
            ("http://[::1]", "http://[::1]:1180"),
            ("192.168.1.100:2345/", "http://192.168.1.100:2345"),
            ("http://localhost:80/", "http://localhost"),
            (
                "https://media.example.com/nipa/",
                "https://media.example.com/nipa",
            ),
        ] {
            assert_eq!(address(input).unwrap(), expected);
        }
        for input in [
            "",
            "file:///tmp/videos",
            "ftp://example.com",
            "http://",
            "https://example.com?token=x",
        ] {
            assert!(address(input).is_err(), "{input}");
        }
    }

    #[test]
    fn shared_payload_preserves_hashes_ids_streams_and_episode_numbers() {
        let videos = parse_videos("https://media.example.com/nipa", &summary(), &detail()).unwrap();
        // Another device's file the share once played is not the share's;
        // one at the share's own address is.
        assert_eq!(videos.len(), 2);
        assert_eq!(
            videos[1].url,
            "https://media.example.com/nipa/api/media/local/share/episodes/ghi/stream"
        );
        let video = &videos[0];
        assert_eq!(
            video.url,
            "https://media.example.com/nipa/api/media/local/share/episodes/abc/stream"
        );
        let (anime, episode) = video.matched.as_ref().unwrap();
        assert_eq!(episode.dandanplay_id, 795321);
        assert_eq!(episode.number, 2); // The opaque episode ID is not its number.
        assert_eq!(video.fingerprint.md5, "0123456789abcdef0123456789abcdef");
        assert_eq!(anime.title, "测试番剧");
        assert_eq!(video.name, "show - 02.mkv");
        assert!(parse_videos("http://localhost:1180", &summary(), &json!({})).is_err());
    }

    #[test]
    fn shared_payload_rejects_local_paths_and_foreign_stream_origins() {
        let base = "https://media.example.com/nipa";
        for path in [
            "/home/user/show.mkv",
            r"C:\Videos\show.mkv",
            r"\\server\videos\show.mkv",
            "show.mkv",
            "file:///home/user/show.mkv",
            "https://other.example.com/stream",
            "http://media.example.com/stream",
            "https://media.example.com:8443/stream",
        ] {
            let payload = json!({"episodes": [{"episodeId": 1, "originalFilePath": path}]});
            assert!(
                parse_videos(base, &summary(), &payload).unwrap().is_empty(),
                "{path}"
            );
        }
        for path in [
            "https://other.example.com/stream",
            "http://media.example.com/stream",
            "https://media.example.com:8443/stream",
        ] {
            let payload = json!({"episodes": [{"episodeId": 1, "streamPath": path}]});
            assert!(
                parse_videos(base, &summary(), &payload).unwrap().is_empty(),
                "{path}"
            );
        }
        let payload = json!({"episodes": [{
            "episodeId": 1,
            "streamPath": "/api/media/local/share/episodes/local/stream",
            "originalFilePath": r"C:\Videos\show.mkv"
        }]});
        let videos = parse_videos(base, &summary(), &payload).unwrap();
        assert_eq!(videos.len(), 1);
        assert_eq!(
            videos[0].url,
            format!("{base}/api/media/local/share/episodes/local/stream")
        );
    }

    #[test]
    fn mounting_imports_playable_files_and_repeating_it_does_not_duplicate_sources() {
        let mut responses = Vec::new();
        for _ in 0..2 {
            responses.push((
                "/api/media/local/share/animes",
                200,
                json!({"items": [summary()]}),
            ));
            responses.push(("/api/media/local/share/animes/42", 200, detail()));
        }
        let (base, thread) = server(responses);
        let mut engine = LibraryEngine::default();
        for _ in 0..2 {
            engine.open_shared_mount().unwrap();
            engine
                .mount_shared("家里的电脑".into(), base.clone())
                .unwrap();
            assert!(engine.state.shared_connecting);
            engine.take_done();
            assert!(!engine.state.shared_prompt && !engine.state.shared_connecting);
            assert!(engine.state.shared_error.is_empty());
            assert_eq!(engine.state.sources.len(), 1);
        }
        thread.join().unwrap();
        let source = engine.state.sources[0].id;
        assert!(engine.state.sources[0].shared);
        assert_eq!(
            engine.state.sources[0].status,
            SourceStatus::Ready { files: 1 }
        );
        assert!(engine.state.posters.is_empty());
        assert_eq!(
            engine.db.episode_files(42)[0].url,
            format!("{base}/api/media/local/share/episodes/abc/stream")
        );
        engine.open_source(source).unwrap();
        assert!(engine.state.folder.dirs.is_empty());
        assert_eq!(engine.state.folder.files.len(), 1);
        let file = &engine.state.folder.files[0];
        assert!(file.readable && !file.matched && file.show.is_empty());
        assert_eq!(file.name, "show - 02.mkv");
        let entries = engine.db.entries(Some(source));
        let address = entries.iter().find(|entry| entry.key == file.key).unwrap();
        assert_eq!(address.url, format!("{base}/api/media/local/share/episodes/abc/stream"));
        assert_eq!(engine.state.sources[0].unmatched, 1);
        assert_eq!(engine.state.unmatched_count, 1);
        assert!(!engine.state.folder.can_batch);

        // A refresh also removes foreign URLs imported by older clients.
        let mut previous = parse_videos(&base, &summary(), &detail()).unwrap();
        previous.push(Video {
            url: "http://192.168.31.65:8080/old-stream".into(),
            name: "old.mkv".into(),
            fingerprint: Fingerprint {
                md5: "nipaplay:old-stream".into(),
                size: 0,
            },
            matched: previous[0].matched.clone(),
        });
        engine.import_shared(source, &previous);
        assert_eq!(engine.db.seen(source).len(), 2);
        engine.inbox.lock().unwrap().push(Done::SharedRefresh(
            source,
            base.clone(),
            Ok(parse_videos(&base, &summary(), &detail()).unwrap()),
        ));
        engine.take_done();
        let refreshed = engine.db.seen(source);
        assert_eq!(refreshed.len(), 1);
        assert_eq!(refreshed[0].url, previous[0].url);

        engine.inbox.lock().unwrap().push(Done::SharedRefresh(
            source,
            base.clone(),
            Err("offline".into()),
        ));
        engine.take_done();
        assert_eq!(engine.state.sources[0].status, SourceStatus::Offline);
        assert_eq!(engine.db.episode_files(42).len(), 1);
        engine.remove_source(source).unwrap();
        engine.inbox.lock().unwrap().push(Done::SharedRefresh(
            source,
            base.clone(),
            Ok(parse_videos(&base, &summary(), &detail()).unwrap()),
        ));
        engine.take_done();
        assert!(engine.state.sources.is_empty());
    }

    #[test]
    fn failed_or_cancelled_connections_do_not_create_sources() {
        let (base, thread) = server(vec![("/api/media/local/share/animes", 403, json!({}))]);
        let mut engine = LibraryEngine::default();
        engine.mount_shared(String::new(), base).unwrap();
        engine.take_done();
        thread.join().unwrap();
        assert!(engine.state.shared_prompt);
        assert!(!engine.state.shared_connecting);
        assert!(engine.state.shared_error.contains("403"));
        assert!(engine.state.sources.is_empty());
        engine.mount_shared(String::new(), "".into()).unwrap();
        assert!(!engine.state.shared_error.is_empty());

        engine.open_shared_mount().unwrap();
        let request = engine.shared_request;
        engine.cancel_shared_mount().unwrap();
        engine.inbox.lock().unwrap().push(Done::SharedMount(
            request,
            "Cancelled".into(),
            "http://localhost:1180".into(),
            Ok(Vec::new()),
        ));
        engine.take_done();
        assert!(!engine.state.shared_prompt);
        assert!(engine.state.sources.is_empty());
    }
}
