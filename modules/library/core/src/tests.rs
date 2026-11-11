use super::*;
use erika_library_protocol::library::{Commands, Engine, MatchNote, SourceContent};
use erika_store::Fingerprint;

pub(super) struct MetadataFixture {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    requests: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for MetadataFixture {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let result = thread.join();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
    }
}
pub(super) fn engine() -> (MetadataFixture, LibraryEngine) {
    engine_with_metadata(|_| {
        (
            200,
            serde_json::json!({"page":1,"total_pages":1,"results":[]}),
        )
    })
}

pub(super) fn engine_with_metadata(
    respond: impl Fn(&str) -> (u16, serde_json::Value) + Send + 'static,
) -> (MetadataFixture, LibraryEngine) {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let requests = Arc::new(AtomicUsize::new(0));
    let (done, count) = (stop.clone(), requests.clone());
    let thread = std::thread::spawn(move || {
        while !done.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    let mut chunk = [0; 4096];
                    while !bytes.ends_with(b"\r\n\r\n") && bytes.len() < 16384 {
                        let size = stream.read(&mut chunk).unwrap_or(0);
                        if size == 0 {
                            break;
                        }
                        bytes.extend_from_slice(&chunk[..size]);
                    }
                    if bytes.is_empty() {
                        continue;
                    }
                    let request = String::from_utf8_lossy(&bytes);
                    let lower = request.to_ascii_lowercase();
                    assert!(!lower.contains("authorization:") && !lower.contains("cookie:"));
                    assert!(request.starts_with("GET "));
                    let path = request.split_whitespace().nth(1).unwrap();
                    count.fetch_add(1, Ordering::SeqCst);
                    let (status, data) = respond(path);
                    let body = serde_json::json!({"schema_version":1,"data":data}).to_string();
                    write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Err(error) => panic!("fixture accept: {error}"),
            }
        }
    });
    let mut engine = LibraryEngine::default();
    engine.metadata =
        erika_catalog::Client::new(&format!("http://{address}/metadata/v1/"), "zh-CN").unwrap();
    (
        MetadataFixture {
            stop,
            requests,
            thread: Some(thread),
        },
        engine,
    )
}

#[test]
fn the_wall_holds_the_shows_with_files_filed_under_their_initials() {
    let mut engine = LibraryEngine::default();
    let empty = engine
        .listing(0, LibraryFilter::All, LibrarySort::Recent)
        .unwrap();
    assert!(empty.empty && empty.total == 0);
    for (id, title, number, position) in [
        (1, "葬送的芙莉莲", 1, 1440.0),
        (1, "葬送的芙莉莲", 2, 100.0),
        (2, "Fate/strange Fake", 1, 0.0),
    ] {
        let url = format!("file:///v/{id}-{number}.mkv");
        let file = engine
            .db
            .opened(
                &url,
                Some(&Fingerprint {
                    md5: url.clone(),
                    size: 1,
                }),
            )
            .unwrap();
        use erika_catalog::*;
        use serde_json::json;
        let work=tmdb::parse_work(Kind::Tv,id as u64,&json!({"id":id,"name":title,"seasons":[{"id":id*100,"season_number":1,"name":"Season 1"}]})).unwrap();
        let season=tmdb::parse_season(&work,1,&json!({"id":id*100,"season_number":1,"name":"Season 1","episodes":[{"id":id*10000+number,"season_number":1,"episode_number":number,"name":"Episode"}]})).unwrap();
        let item=&season.episodes[0];
        engine.db.catalog_apply(&[work.node,season.node,item.clone()],&[],&[FileBinding {
            file, decision:Decision::Auto,revision:0,evidence:"fixture".into(),
            items:vec![BindingItem {media:item.id.clone(),start_seconds:None,end_seconds:None}],
        }],&[]).unwrap();
        engine.db.save_progress(file, position, 1440.0);
        if position >= 1440.0 {
            let mut measured = erika_catalog::playback::Intervals::default();
            measured.add(0., 1440.).unwrap();
            engine
                .db
                .catalog_record_playback(
                    file,
                    engine.db.catalog_revision(file).unwrap(),
                    &measured,
                    1440.,
                    true,
                )
                .unwrap();
        }
    }
    engine.history_changed().unwrap();
    let named = engine
        .listing(0, LibraryFilter::All, LibrarySort::Name)
        .unwrap();
    assert_eq!(
        named
            .posters
            .iter()
            .map(|p| p.initial.as_str())
            .collect::<Vec<_>>(),
        ["F", "Z"]
    );
    assert_eq!((named.total, named.watching, named.unwatched), (2, 1, 1));
    assert_eq!(
        named
            .letters
            .iter()
            .find(|l| l.letter == "Z")
            .unwrap()
            .target,
        erika_catalog::MediaId::for_ref(&erika_catalog::ProviderRef::tmdb(erika_catalog::Kind::Tv, 1).unwrap()).0
    );
    // Videos opened one by one belong to no folder.
    assert!(
        engine
            .listing(5, LibraryFilter::All, LibrarySort::Name)
            .unwrap()
            .empty
    );
    engine
        .open(erika_catalog::MediaId::for_ref(&erika_catalog::ProviderRef::tmdb(erika_catalog::Kind::Tv, 1).unwrap()).0)
        .unwrap();
    assert_eq!(engine.state.show.title, "葬送的芙莉莲");
    assert_eq!(
        engine
            .open(erika_catalog::MediaId::new().0)
            .unwrap_err()
            .code,
        "unknown_show"
    );
}

/// A folder of videos on disk, removed when dropped.
struct Folder(std::path::PathBuf);

impl Folder {
    fn new(files: &[&str]) -> Folder {
        let root = std::env::temp_dir().join(format!(
            "erika-library-{}-{}",
            std::process::id(),
            files.len()
        ));
        let _ = std::fs::remove_dir_all(&root);
        for (index, file) in files.iter().enumerate() {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, format!("video {index}")).unwrap();
        }
        Folder(root)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_folder_is_scanned_browsed_and_matched_by_hand() {
    let folder = Folder::new(&[
        "地狱乐 S2/[ANi] Jigokuraku - 14 [1080P].mp4",
        "地狱乐 S2/[ANi] Jigokuraku - 13 [1080P].mp4",
        "地狱乐 S2/OVA.mp4",
        "未整理/第五话 修正版.mkv",
        "notes.txt",
    ]);
    let (metadata, mut engine) = engine();
    engine
        .add_folder(MediaSource::file(&folder.0), SourceContent::Anime)
        .unwrap();
    engine.wake();
    let source = engine.state.sources[0].clone();
    assert_eq!(source.status, SourceStatus::Ready { files: 4 });
    assert_eq!(source.unmatched, 4);
    // No account is needed for the anonymous metadata fallback.
    assert!(metadata.requests.load(std::sync::atomic::Ordering::SeqCst) > 0);
    assert_eq!(
        engine.state.notice,
        Some(LibraryNotice::Scanned {
            name: source.name.clone(),
            files: 4,
            unmatched: 4,
        })
    );
    assert_eq!(engine.state.unmatched_count, 4);
    assert_eq!(
        engine
            .state
            .unmatched
            .iter()
            .find(|file| file.name == "OVA.mp4")
            .unwrap()
            .place,
        format!("{} › 地狱乐 S2", source.name)
    );

    // Browsing: the folders, then the videos of one in the order of their numbers.
    engine.open_source(source.id).unwrap();
    let dirs: Vec<(String, i64)> = engine
        .state
        .folder
        .dirs
        .iter()
        .map(|dir| (dir.name.clone(), dir.files))
        .collect();
    assert_eq!(
        dirs,
        [("地狱乐 S2".to_string(), 3), ("未整理".to_string(), 1)]
    );
    engine.open_dir("地狱乐 S2".into()).unwrap();
    assert!(engine.state.folder.can_batch);
    assert_eq!(engine.state.folder.crumbs.len(), 2);

    // A folder at once: by the numbers in the names, OVA skipped; the
    // names go on from season one, so they are aligned to episode 1.
    engine.match_folder().unwrap();
    matching::tests::choose(&mut engine, 2);
    engine.match_align("1".into()).unwrap();
    engine.load();
    let matching = &engine.state.matching;
    assert!(matching.open && !matching.single && !matching.by_order);
    assert_eq!(
        (
            matching.on_count,
            matching.skipped_count,
            matching.lowest,
            matching.align_to
        ),
        (2, 1, 13, 1)
    );
    assert_eq!(
        matching
            .rows
            .iter()
            .map(|row| row.assignment.parse::<i64>().unwrap_or(0))
            .collect::<Vec<_>>(),
        [1, 2, 0]
    );
    assert!(matching.ready);
    // Typed over, the second becomes a second version of the first.
    let second = matching.rows[1].key;
    engine.match_number(second, "1".into()).unwrap();
    assert_eq!(engine.state.matching.rows[1].note, MatchNote::Twin);
    engine.match_number(second, "".into()).unwrap();
    engine.match_confirm().unwrap();
    assert_eq!(
        engine.state.notice,
        Some(LibraryNotice::Matched {
            count: 2,
            title: "地狱乐".into(),
        })
    );
    assert!(!engine.state.matching.open);
    assert_eq!(engine.state.unmatched_count, 2);
    let wall = engine
        .listing(source.id, LibraryFilter::All, LibrarySort::Name)
        .unwrap();
    assert_eq!((wall.posters.len(), wall.posters[0].episodes), (1, 2));
    assert!(engine.state.folder.files.iter().any(|file| file.matched && matches!(&file.note, FileNote::Content { title } if title.starts_with("地狱乐 · S02E"))));

    // One video: unlinked, it goes back among those not matched, and a
    // rescan leaves it so.
    let first = engine
        .state
        .folder
        .files
        .iter()
        .find(|file| file.matched)
        .unwrap()
        .key;
    engine.unlink(first).unwrap();
    assert_eq!(engine.state.unmatched_count, 3);
    // A video deleted from the folder is gone after a rescan.
    std::fs::remove_file(folder.0.join("未整理/第五话 修正版.mkv")).unwrap();
    engine.rescan(source.id).unwrap();
    engine.wake();
    assert_eq!(
        engine.state.sources[0].status,
        SourceStatus::Ready { files: 3 }
    );
    assert_eq!(engine.state.unmatched_count, 2);

    // By order, from episode 5.
    engine.match_folder().unwrap();
    engine.match_by_order(true).unwrap();
    engine.match_start("5".into()).unwrap();
    let numbers: Vec<i64> = engine
        .state
        .matching
        .rows
        .iter()
        .filter(|row| row.on)
        .map(|row| row.assignment.parse::<i64>().unwrap_or(0))
        .collect();
    assert_eq!(numbers, (5..5 + numbers.len() as i64).collect::<Vec<_>>());
    engine.match_cancel().unwrap();

    engine.remove_source(source.id).unwrap();
    assert!(
        !engine.state.has_sources && !engine.state.browsing && engine.state.unmatched.is_empty()
    );
}

#[test]
fn a_folder_of_tv_series_uses_the_anonymous_metadata_service() {
    let folder = Folder::new(&[
        "行尸走肉/The.Walking.Dead.S01E01.mkv",
        "行尸走肉/The.Walking.Dead.S01E02.mkv",
    ]);
    let (metadata, mut engine) = engine();
    engine
        .add_folder(MediaSource::file(&folder.0), SourceContent::Series)
        .unwrap();
    engine.wake();
    let source = engine.state.sources[0].clone();
    assert_eq!(source.content, SourceContent::Series);
    assert_eq!(source.status, SourceStatus::Ready { files: 2 });
    assert!(metadata.requests.load(std::sync::atomic::Ordering::SeqCst) > 0);
    assert_eq!(
        engine.state.notice,
        Some(LibraryNotice::Scanned {
            name: source.name.clone(),
            files: 2,
            unmatched: 2,
        })
    );
    engine.set_content(source.id, SourceContent::Anime).unwrap();
    assert_eq!(engine.state.sources[0].content, SourceContent::Anime);
    assert_eq!(
        engine.state.notice,
        Some(LibraryNotice::Retyped {
            name: source.name.clone(),
            content: SourceContent::Anime
        })
    );
    // Manual content matching is anonymous TMDB for every source kind too.
    engine
        .set_content(source.id, SourceContent::Series)
        .unwrap();
    engine.open_source(source.id).unwrap();
    engine.open_dir("行尸走肉".into()).unwrap();
    let before = metadata.requests.load(std::sync::atomic::Ordering::SeqCst);
    engine.match_folder().unwrap();
    engine.wake();
    assert!(metadata.requests.load(std::sync::atomic::Ordering::SeqCst) >= before + 2);
    assert!(engine.session.as_ref().unwrap().results.is_empty());
    assert!(!engine.state.matching.ready);
    assert!(!engine.state.matching.error.contains("登录"));
}

#[test]
fn tmdb_movies_and_specials_are_visible_without_provider_episode_pointers() {
    use erika_catalog::*;
    use serde_json::json;
    let mut engine = LibraryEngine::default();
    let source = engine
        .db
        .add_source("folder", "Videos", "file:///videos", "anime")
        .unwrap();
    let film = tmdb::parse_work(
        Kind::Movie,
        1,
        &json!({"id":1,"title":"电影","poster_path":"/movie.jpg"}),
    )
    .unwrap()
    .node;
    let tv=tmdb::parse_work(Kind::Tv,1,&json!({"id":1,"name":"剧集","poster_path":"/tv.jpg","seasons":[{"id":2,"season_number":0,"name":"Specials"}]})).unwrap();
    let season=tmdb::parse_season(&tv,0,&json!({"id":2,"season_number":0,"name":"Specials","episodes":[{"id":3,"season_number":0,"episode_number":1,"name":"特典"}]})).unwrap();
    let mut nodes = vec![film.clone(), tv.node.clone(), season.node];
    nodes.extend(season.episodes.clone());
    engine.db.catalog_save_nodes(&nodes).unwrap();
    for (name, node) in [("movie", &film), ("special", &season.episodes[0])] {
        let f = engine
            .db
            .saw(
                source,
                &format!("file:///videos/{name}.mkv"),
                100,
                1,
                Some(&Fingerprint {
                    md5: name.into(),
                    size: 100,
                }),
            )
            .unwrap();
        engine
            .db
            .catalog_apply(
                &[],
                &[],
                &[FileBinding {
                    file: f,
                    decision: Decision::Auto,
                    revision: 0,
                    evidence: "fixture".into(),
                    items: vec![BindingItem {
                        media: node.id.clone(),
                        start_seconds: None,
                        end_seconds: None,
                    }],
                }],
                &[],
            )
            .unwrap();
        assert!(engine.db.file_episode(f).is_none());
    }
    engine.load();
    engine.open_source(source).unwrap();
    assert_eq!(engine.state.posters.len(), 2);
    assert_eq!(engine.state.unmatched_count, 0);
    assert!(
        engine
            .state
            .folder
            .files
            .iter()
            .all(|f| f.matched && !f.show.is_empty())
    );
    assert!(
        engine
            .state
            .folder
            .files
            .iter()
            .any(|f| matches!(&f.note,FileNote::Content {title} if title.contains("S00E01")))
    );
    let wall = engine
        .listing(source, LibraryFilter::All, LibrarySort::Name)
        .unwrap();
    let film_poster = wall.posters.iter().find(|p| p.id == film.id.0).unwrap();
    assert!(film_poster.movie);
    assert_eq!(
        film_poster.poster.url,
        "https://image.tmdb.org/t/p/original/movie.jpg"
    );
    engine.open(film.id.0.clone()).unwrap();
    assert_eq!(engine.state.opened, film.id.0);
    engine.close().unwrap();
    assert!(engine.state.opened.is_empty());
}

#[test]
fn names_sort_as_people_count() {
    let mut names = vec!["10.mkv", "2.mkv", "1.mkv", "b", "a"];
    names.sort_by(|a, b| natural(a, b));
    assert_eq!(names, ["1.mkv", "2.mkv", "10.mkv", "a", "b"]);
}
