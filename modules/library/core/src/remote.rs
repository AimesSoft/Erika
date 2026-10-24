//! A dandanplay PC client's remote access as a source, as NipaPlay's
//! 弹弹play远程媒体库 is: the client's library, read whole before the
//! library changes, so a failed refresh keeps the last videos. The client
//! matched most of them already; the others wait among the videos not
//! recognized, with the client's fingerprint to match them by.
use crate::shared::Video;
use erika_dandanplay::remote::{self, Item};
use erika_store::{Anime, Episode, Fingerprint};

pub const KIND: &str = "dandanplay";

/// The setting that keeps the API key of source `id`.
pub fn token_setting(id: i64) -> String {
    format!("library.dandanplay.{id}.token")
}

/// The videos of `items`, streamed from the client at `base`.
pub fn videos(base: &str, token: &str, items: &[Item]) -> Vec<Video> {
    items
        .iter()
        .filter_map(|item| {
            let url = remote::stream_url(base, item, token)?;
            let fingerprint = if item.has_hash() && item.size > 0 {
                Fingerprint {
                    md5: item.hash.to_ascii_lowercase(),
                    size: item.size,
                }
            } else {
                // A namespaced identity cannot collide with a local
                // file's MD5, and stays when the key changes.
                Fingerprint {
                    md5: format!("dandanplay:{base}/{}{}", item.id, item.hash),
                    size: 0,
                }
            };
            let matched = (item.anime_id > 0 && item.episode_id > 0).then(|| {
                let title = match item.anime_title.as_str() {
                    "" => item.file_name(),
                    title => title.to_owned(),
                };
                (
                    Anime {
                        dandanplay_id: item.anime_id,
                        title,
                        // The client keeps no poster; the library asks
                        // dandanplay for one.
                        image: String::new(),
                    },
                    Episode {
                        dandanplay_id: item.episode_id,
                        number: match item.number() {
                            0 => crate::names::episode_number(&item.episode_title)
                                .unwrap_or(1)
                                .max(1),
                            number => number,
                        },
                        title: match item.episode_title.as_str() {
                            "" => item.file_name(),
                            title => title.to_owned(),
                        },
                    },
                )
            });
            Some(Video {
                url,
                name: item.file_name(),
                fingerprint,
                matched,
            })
        })
        .collect()
}

/// The thumbnails of the shows in `items`, by anime: the poster of a show
/// dandanplay cannot be asked about, as NipaPlay falls back to it.
pub fn thumbnails(base: &str, token: &str, items: &[Item]) -> Vec<(i64, String)> {
    let mut seen = std::collections::HashSet::new();
    items
        .iter()
        .filter(|item| item.anime_id > 0 && item.episode_id > 0)
        .filter(|item| seen.insert(item.anime_id))
        .filter_map(|item| Some((item.anime_id, remote::image_url(base, item, token)?)))
        .collect()
}

/// Everything a refresh of the client at `base` brings: its videos, and the
/// thumbnails of its shows.
pub struct Snapshot {
    pub videos: Vec<Video>,
    pub thumbnails: Vec<(i64, String)>,
    /// The key to keep: none when the client does not ask for one.
    pub token: String,
}

/// Connect to the client at `base` with `token` and read its library.
pub fn fetch(base: &str, token: &str) -> Result<Snapshot, String> {
    let (token, items) = remote::connect(base, token)?;
    Ok(Snapshot {
        videos: videos(base, &token, &items),
        thumbnails: thumbnails(base, &token, &items),
        token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Done, LibraryEngine};
    use erika_library_protocol::library::{Commands, SourceStatus};

    /// `/api/v1/library` recorded from a client with three videos: two
    /// episodes it matched (one twice, in two folders) and one it did not.
    const LIBRARY: &str = r#"[
      {"AnimeId": 14198, "EpisodeId": 141980003, "AnimeTitle": "佐贺偶像是传奇",
       "EpisodeTitle": "第3话 DEAD OR LIVE SAGA", "Id": "c004e475-d9bb-41e1-976b-7fce00997f3a",
       "Hash": "03778309A0E8A09C2F43603A490F2E98", "Name": "[Zombieland Saga][03][BIG5][1080P].mp4",
       "Path": "Y:\\[Zombieland Saga][03][BIG5][1080P].mp4", "Size": 518754774, "Rate": 0,
       "IsStandalone": false, "Created": "2018-10-21T23:14:59", "LastMatch": "0001-01-01T00:00:00",
       "LastPlay": "2019-04-25T18:00:22", "LastThumbnail": "2019-03-05T22:03:20", "Duration": 1420},
      {"AnimeId": 14198, "EpisodeId": 141980004, "AnimeTitle": "佐贺偶像是传奇",
       "EpisodeTitle": "第4话 Walking Dead SAGA", "Id": "5b0c8a5e-0d55-4b8e-9d3c-111111111111",
       "Hash": "13778309A0E8A09C2F43603A490F2E98", "Name": "[Zombieland Saga][04][BIG5][1080P].mp4",
       "Path": "Y:\\[Zombieland Saga][04][BIG5][1080P].mp4", "Size": 512000000, "Duration": 1420},
      {"AnimeId": 0, "EpisodeId": 0, "AnimeTitle": "", "EpisodeTitle": "",
       "Id": "686b4d07-d8de-4f2e-b716-3cf0843cc903", "Hash": "334763177A0B9A5647B263AA74E0A013",
       "Name": "unknown [01].mkv", "Path": "Y:\\unknown [01].mkv", "Size": 128023969}
    ]"#;

    fn items() -> Vec<Item> {
        remote::parse_library(LIBRARY).unwrap()
    }

    #[test]
    fn matched_videos_keep_the_clients_episode_and_fingerprint() {
        let videos = videos("http://192.168.1.2", "key", &items());
        assert_eq!(videos.len(), 3);
        let (anime, episode) = videos[0].matched.as_ref().unwrap();
        assert_eq!(anime.dandanplay_id, 14198);
        assert_eq!(anime.title, "佐贺偶像是传奇");
        assert!(anime.image.is_empty());
        assert_eq!(episode.dandanplay_id, 141980003);
        assert_eq!(episode.number, 3);
        assert_eq!(episode.title, "第3话 DEAD OR LIVE SAGA");
        assert_eq!(
            videos[0].fingerprint,
            Fingerprint {
                md5: "03778309a0e8a09c2f43603a490f2e98".into(),
                size: 518754774
            }
        );
        assert_eq!(
            videos[0].url,
            "http://192.168.1.2/api/v1/stream/id/c004e475-d9bb-41e1-976b-7fce00997f3a?token=key"
        );
        assert_eq!(videos[0].name, "[Zombieland Saga][03][BIG5][1080P].mp4");
        assert!(videos[2].matched.is_none());
        let thumbnails = thumbnails("http://192.168.1.2", "", &items());
        assert_eq!(thumbnails.len(), 1);
        assert_eq!(
            thumbnails[0].1,
            "http://192.168.1.2/api/v1/image/id/c004e475-d9bb-41e1-976b-7fce00997f3a"
        );
    }

    /// A dandanplay client on this machine, answering `responses` in order:
    /// each the path it expects, the status and the body.
    fn server(
        responses: Vec<(&'static str, u16, &'static str)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            time::{Duration, Instant},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let thread = std::thread::spawn(move || {
            let mut requests = Vec::new();
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
                let request = String::from_utf8_lossy(&request).into_owned();
                assert!(
                    request.starts_with(&format!("GET {path} HTTP/1.1")),
                    "{request}"
                );
                requests.push(request);
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        (base, thread)
    }

    const WELCOME_KEY: &str = r#"{"message": "Hello dandanplay user!", "version": "11.2.0.0", "time": "10/24/2026 20:00:00", "tokenRequired": true}"#;
    const WELCOME_OPEN: &str = r#"{"message": "Hello dandanplay user!", "version": "11.2.0.0", "time": "10/24/2026 20:00:00", "tokenRequired": false}"#;

    #[test]
    fn mounting_checks_the_key_imports_the_library_and_keeps_the_key() {
        let (base, thread) = server(vec![
            ("/api/v1/welcome", 200, WELCOME_KEY),
            ("/api/v1/welcome", 200, WELCOME_KEY),
            ("/api/v1/library", 401, ""),
            ("/api/v1/welcome", 200, WELCOME_KEY),
            ("/api/v1/library", 200, LIBRARY),
        ]);
        let mut engine = LibraryEngine::default();
        engine.open_remote_mount().unwrap();
        // The client asks for a key and none is given.
        engine
            .mount_remote("书房的电脑".into(), base.clone(), String::new())
            .unwrap();
        engine.take_done();
        assert!(engine.state.remote_prompt);
        assert!(engine.state.remote_error.contains("API 密钥"));
        // A wrong key.
        engine
            .mount_remote("书房的电脑".into(), base.clone(), "wrong".into())
            .unwrap();
        engine.take_done();
        assert!(engine.state.remote_error.contains("API 密钥不对"));
        assert!(engine.state.sources.is_empty());
        engine
            .mount_remote("书房的电脑".into(), base.clone(), "secret".into())
            .unwrap();
        assert!(engine.state.remote_connecting);
        engine.take_done();
        let requests = thread.join().unwrap();
        assert!(
            requests[4]
                .to_ascii_lowercase()
                .contains("authorization: bearer secret")
        );
        assert!(!engine.state.remote_prompt && !engine.state.remote_connecting);
        assert_eq!(engine.state.remote_saved, 1);
        let row = &engine.state.sources[0];
        assert!(row.shared && row.dandanplay);
        assert_eq!(row.name, "书房的电脑");
        assert_eq!(row.location, base);
        assert_eq!(row.status, SourceStatus::Ready { files: 3 });
        assert_eq!(row.unmatched, 1);
        let source = row.id;
        assert_eq!(
            engine.db.setting(&token_setting(source)).as_deref(),
            Some("secret")
        );
        // The client's matches are on the wall, its thumbnail the poster
        // while dandanplay cannot be asked.
        assert_eq!(engine.state.posters.len(), 1);
        assert_eq!(engine.state.posters[0].id, 14198);
        assert_eq!(engine.state.posters[0].episodes, 2);
        assert!(
            engine.state.posters[0]
                .poster
                .url
                .contains("/api/v1/image/id/")
        );
        let files = engine.db.episode_files(14198);
        assert_eq!(files.len(), 2);
        assert!(files[0].url.ends_with("?token=secret"));
        // The video not matched is listed by its file name.
        assert_eq!(engine.state.unmatched[0].name, "unknown [01].mkv");
        assert_eq!(engine.state.unmatched[0].place, "书房的电脑");
        // The source opens as shows, and the video not matched beside them.
        engine.open_source(source).unwrap();
        assert!(engine.state.folder.dandanplay);
        assert_eq!(engine.state.folder.dirs[0].name, "佐贺偶像是传奇");
        assert_eq!(engine.state.folder.files[0].name, "unknown [01].mkv");
        assert!(engine.state.folder.files[0].readable);
        engine.open_dir("佐贺偶像是传奇".into()).unwrap();
        assert_eq!(engine.state.folder.files.len(), 2);
        assert_eq!(engine.state.folder.files[0].name, "第3话 DEAD OR LIVE SAGA");
        assert_eq!(engine.state.folder.files[0].show, 14198);

        // Removing it forgets its key.
        engine.remove_source(source).unwrap();
        assert!(engine.state.sources.is_empty());
        assert!(engine.db.setting(&token_setting(source)).is_none());
        assert!(engine.db.episode_files(14198).is_empty());
    }

    #[test]
    fn editing_keeps_empty_fields_and_a_failed_refresh_keeps_the_videos() {
        let (base, thread) = server(vec![
            ("/api/v1/welcome", 200, WELCOME_OPEN),
            ("/api/v1/library", 200, LIBRARY),
            ("/api/v1/welcome", 200, WELCOME_OPEN),
            ("/api/v1/library", 200, LIBRARY),
        ]);
        let mut engine = LibraryEngine::default();
        engine.open_remote_mount().unwrap();
        // A key the client does not ask for is not kept.
        engine
            .mount_remote(String::new(), base.clone(), "unused".into())
            .unwrap();
        engine.take_done();
        let source = engine.state.sources[0].id;
        assert_eq!(engine.state.sources[0].name, base);
        assert!(engine.db.setting(&token_setting(source)).is_none());
        assert!(!engine.db.episode_files(14198)[0].url.contains("token"));

        engine.edit_remote(source).unwrap();
        assert!(engine.state.remote_prompt);
        assert_eq!(engine.state.remote_editing, source);
        assert_eq!(engine.state.remote_address, base);
        engine
            .mount_remote("客厅".into(), String::new(), String::new())
            .unwrap();
        engine.take_done();
        thread.join().unwrap();
        assert!(!engine.state.remote_prompt);
        assert_eq!(engine.state.sources.len(), 1);
        assert_eq!(engine.state.sources[0].name, "客厅");
        assert_eq!(engine.state.sources[0].location, base);

        // The client went away: the source is offline and keeps its videos.
        engine.inbox.lock().unwrap().push(Done::RemoteRefresh(
            source,
            base.clone(),
            Err("offline".into()),
        ));
        engine.take_done();
        assert_eq!(engine.state.sources[0].status, SourceStatus::Offline);
        assert_eq!(engine.db.episode_files(14198).len(), 2);

        // Editing a source that is not there.
        assert!(engine.edit_remote(source + 100).is_err());
        engine.cancel_remote_mount().unwrap();
        assert!(!engine.state.remote_prompt);
    }
}
