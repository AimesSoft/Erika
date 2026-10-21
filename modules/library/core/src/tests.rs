use super::*;
use erika_library_protocol::library::{Commands, Engine, MatchNote};
use erika_store::{Episode, Fingerprint};

#[test]
fn the_wall_holds_the_shows_with_files_filed_under_their_initials() {
    let mut engine = LibraryEngine::default();
    let empty = engine.listing(0, LibraryFilter::All, LibrarySort::Recent);
    assert!(empty.empty && empty.total == 0);
    for (id, title, number, position) in [(1, "葬送的芙莉莲", 1, 1440.0), (1, "葬送的芙莉莲", 2, 100.0), (2, "Fate/strange Fake", 1, 0.0)] {
        let url = format!("file:///v/{id}-{number}.mkv");
        let file = engine.db.opened(&url, Some(&Fingerprint { md5: url.clone(), size: 1 })).unwrap();
        let anime = Anime { dandanplay_id: id, title: title.into(), image: format!("https://img/{id}.jpg") };
        engine.db.link_episode(file, &anime, &Episode { dandanplay_id: id * 10_000 + number, number, title: String::new() }, false);
        engine.db.save_progress(file, position, 1440.0);
    }
    engine.history_changed().unwrap();
    let named = engine.listing(0, LibraryFilter::All, LibrarySort::Name);
    assert_eq!(named.posters.iter().map(|p| p.initial.as_str()).collect::<Vec<_>>(), ["F", "Z"]);
    assert_eq!((named.total, named.watching, named.unwatched), (2, 1, 1));
    assert_eq!(named.letters.iter().find(|l| l.letter == "Z").unwrap().target, 1);
    // Videos opened one by one belong to no folder.
    assert!(engine.listing(5, LibraryFilter::All, LibrarySort::Name).empty);
    engine.open(1).unwrap();
    assert_eq!(engine.state.show.title, "葬送的芙莉莲");
    assert_eq!(engine.open(3).unwrap_err().code, "unknown_show");
}

/// A folder of videos on disk, removed when dropped.
struct Folder(std::path::PathBuf);

impl Folder {
    fn new(files: &[&str]) -> Folder {
        let root = std::env::temp_dir().join(format!("erika-library-{}-{}", std::process::id(), files.len()));
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
    let mut engine = LibraryEngine::default();
    engine.add_folder(MediaSource::file(&folder.0)).unwrap();
    engine.wake();
    let source = engine.state.sources[0].clone();
    assert_eq!(source.status, SourceStatus::Ready { files: 4 });
    assert_eq!(source.unmatched, 4);
    // Without an account nothing is recognized, and the library says why.
    assert_eq!(engine.state.notice, Some(LibraryNotice::SignInToMatch));
    assert_eq!(engine.state.unmatched_count, 4);
    assert_eq!(engine.state.unmatched.iter().find(|file| file.name == "OVA.mp4").unwrap().place, format!("{} › 地狱乐 S2", source.name));

    // Browsing: the folders, then the videos of one in the order of their numbers.
    engine.open_source(source.id).unwrap();
    let dirs: Vec<(String, i64)> = engine.state.folder.dirs.iter().map(|dir| (dir.name.clone(), dir.files)).collect();
    assert_eq!(dirs, [("地狱乐 S2".to_string(), 3), ("未整理".to_string(), 1)]);
    engine.open_dir("地狱乐 S2".into()).unwrap();
    assert!(engine.state.folder.can_batch);
    assert_eq!(engine.state.folder.crumbs.len(), 2);

    // A folder at once: by the numbers in the names, OVA skipped; the
    // names go on from season one, so they are aligned to episode 1.
    engine.match_folder().unwrap();
    {
        let session = engine.session.as_mut().unwrap();
        session.choose(Anime { dandanplay_id: 18_000, title: "地狱乐 第二季".into(), image: String::new() }, 12);
    }
    engine.load();
    let matching = &engine.state.matching;
    assert!(matching.open && !matching.single && !matching.by_order);
    assert_eq!((matching.on_count, matching.skipped_count, matching.lowest, matching.align_to), (2, 1, 13, 1));
    assert_eq!(matching.rows.iter().map(|row| row.number).collect::<Vec<_>>(), [1, 2, 0]);
    assert!(matching.ready);
    // Typed over, the second becomes a second version of the first.
    let second = matching.rows[1].key;
    engine.match_number(second, "1".into()).unwrap();
    assert_eq!(engine.state.matching.rows[1].note, MatchNote::Twin);
    engine.match_number(second, "".into()).unwrap();
    engine.match_confirm().unwrap();
    assert_eq!(engine.state.notice, Some(LibraryNotice::MatchedMany { count: 2, anime: "地狱乐 第二季".into(), first: 1, last: 2 }));
    assert!(!engine.state.matching.open);
    assert_eq!(engine.state.unmatched_count, 2);
    let wall = engine.listing(source.id, LibraryFilter::All, LibrarySort::Name);
    assert_eq!((wall.posters.len(), wall.posters[0].episodes), (1, 2));
    assert!(engine.state.folder.files.iter().any(|file| file.note == FileNote::Manual { anime: "地狱乐 第二季".into(), episode: 1 }));

    // One video: unlinked, it goes back among those not matched, and a
    // rescan leaves it so.
    let first = engine.state.folder.files.iter().find(|file| file.matched).unwrap().key;
    engine.unlink(first).unwrap();
    assert_eq!(engine.state.unmatched_count, 3);
    // A video deleted from the folder is gone after a rescan.
    std::fs::remove_file(folder.0.join("未整理/第五话 修正版.mkv")).unwrap();
    engine.rescan(source.id).unwrap();
    engine.wake();
    assert_eq!(engine.state.sources[0].status, SourceStatus::Ready { files: 3 });
    assert_eq!(engine.state.unmatched_count, 2);

    // By order, from episode 5.
    engine.match_folder().unwrap();
    engine.match_by_order(true).unwrap();
    engine.match_start("5".into()).unwrap();
    let numbers: Vec<i64> = engine.state.matching.rows.iter().filter(|row| row.on).map(|row| row.number).collect();
    assert_eq!(numbers, (5..5 + numbers.len() as i64).collect::<Vec<_>>());
    engine.match_cancel().unwrap();

    engine.remove_source(source.id).unwrap();
    assert!(!engine.state.has_sources && !engine.state.browsing && engine.state.unmatched.is_empty());
}

#[test]
fn names_sort_as_people_count() {
    let mut names = vec!["10.mkv", "2.mkv", "1.mkv", "b", "a"];
    names.sort_by(|a, b| natural(a, b));
    assert_eq!(names, ["1.mkv", "2.mkv", "10.mkv", "a", "b"]);
}
