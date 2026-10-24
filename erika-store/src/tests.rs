use super::*;

fn print(md5: &str, size: u64) -> Fingerprint {
    Fingerprint {
        md5: md5.into(),
        size,
    }
}

#[test]
fn one_file_in_two_places_shares_its_progress() {
    let db = Db::in_memory();
    let here = db
        .opened("file:///a/ep1.mkv", Some(&print("3f2a", 100)))
        .unwrap();
    let there = db
        .opened("file:///b/moved.mkv", Some(&print("3f2a", 100)))
        .unwrap();
    assert_eq!(here, there);
    // A download that stopped half-way shares the head but not the size.
    let partial = db
        .opened("file:///c/ep1.part", Some(&print("3f2a", 40)))
        .unwrap();
    assert_ne!(partial, here);
    assert_eq!(db.progress(here), None);
    db.save_progress(here, 600.0, 1440.0);
    db.save_progress(there, 612.5, 1440.0);
    assert_eq!(db.progress(here), Some((612.5, 1440.0)));
    assert_eq!(db.progress(partial), None);
    // Opening a known address again without its fingerprint keeps its file.
    assert_eq!(db.opened("file:///a/ep1.mkv", None), None);
    assert_eq!(
        db.opened("file:///a/ep1.mkv", Some(&print("3f2a", 100))),
        Some(here)
    );
}

#[test]
fn recent_videos_come_newest_first_and_keep_their_picture() {
    let db = Db::in_memory();
    db.remember("file:///one.mkv", "one", Some("/c/1.png"), 10.0, 100.0);
    db.remember("file:///two.mkv", "two", None, 20.0, 200.0);
    db.remember(
        "https://x/three.mp4",
        "three",
        Some("/c/3.png"),
        30.0,
        300.0,
    );
    // Played again: back on top, with its picture though none came now.
    db.remember("file:///one.mkv", "one", None, 50.0, 100.0);
    let recents = db.recents(3);
    assert_eq!(
        recents
            .iter()
            .map(|recent| recent.title.as_str())
            .collect::<Vec<_>>(),
        ["one", "three", "two"]
    );
    assert_eq!(recents[0].still.as_deref(), Some("/c/1.png"));
    assert_eq!(recents[0].position, 50.0);
    assert_eq!(db.recents(1).len(), 1);
    let mut stills = db.stills();
    stills.sort();
    assert_eq!(stills, ["/c/1.png", "/c/3.png"]);
}

#[test]
fn settings_and_anime_are_kept_and_replaced() {
    let db = Db::in_memory();
    assert_eq!(db.setting("account"), None);
    db.set_setting("account", Some("nipa"));
    db.set_setting("account", Some("erika"));
    assert_eq!(db.setting("account").as_deref(), Some("erika"));
    db.set_setting("account", None);
    assert_eq!(db.setting("account"), None);
    let anime = Anime {
        dandanplay_id: 17675,
        title: "葬送的芙莉莲".into(),
        image: "https://img/1.jpg".into(),
    };
    db.remember_anime(&anime);
    db.remember_anime(&Anime {
        image: "https://img/2.jpg".into(),
        ..anime.clone()
    });
    assert_eq!(db.anime(17675).unwrap().image, "https://img/2.jpg");
    assert_eq!(db.anime(1), None);
}

#[test]
fn a_database_file_is_migrated_once_and_shared_by_connections() {
    let dir = std::env::temp_dir().join(format!("erika-store-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("erika.db");
    let _ = std::fs::remove_file(&path);
    let first = Db::at(&path).unwrap();
    let second = Db::at(&path).unwrap();
    first.remember("file:///v.mkv", "v", None, 1.0, 2.0);
    assert_eq!(second.recents(3).len(), 1);
    drop((first, second));
    // Reopening runs no step twice.
    let again = Db::at(&path).unwrap();
    assert_eq!(again.recents(3)[0].title, "v");
    drop(again);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_fingerprint_reads_the_head_and_the_size() {
    let dir = std::env::temp_dir().join(format!("erika-print-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("v.bin");
    std::fs::write(&path, b"hello").unwrap();
    let file = std::fs::File::open(&path).unwrap();
    let print = fingerprint(file, 5).unwrap();
    assert_eq!(print.md5, "5d41402abc4b2a76b9719d911017c592");
    assert_eq!(print.size, 5);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn files_matched_to_episodes_make_what_is_being_watched() {
    let db = Db::in_memory();
    let frieren = Anime {
        dandanplay_id: 17675,
        title: "葬送的芙莉莲".into(),
        image: "https://img/f.jpg".into(),
    };
    let episode = |number: i64| Episode {
        dandanplay_id: 176_750_000 + number,
        number,
        title: format!("第{number}话"),
    };
    let one = db
        .opened("file:///a/01.mkv", Some(&print("01", 1)))
        .unwrap();
    let two = db
        .opened("file:///a/02.mkv", Some(&print("02", 2)))
        .unwrap();
    let other = db.opened("file:///b/x.mkv", Some(&print("x", 3))).unwrap();
    assert_eq!(db.file_episode(one), None);
    db.link_episode(one, &frieren, &episode(1), false);
    db.link_episode(two, &frieren, &episode(2), true);
    // An automatic match leaves the user's own alone.
    db.link_episode(two, &frieren, &episode(9), false);
    assert_eq!(db.file_episode(two).unwrap().1.number, 2);
    db.save_progress(one, 1400.0, 1440.0);
    db.save_progress(two, 300.0, 1440.0);
    db.remember("file:///a/01.mkv", "01.mkv", None, 1400.0, 1440.0);
    db.remember("file:///b/x.mkv", "x.mkv", None, 5.0, 10.0);
    db.remember(
        "file:///a/02.mkv",
        "02.mkv",
        Some("/c/2.png"),
        300.0,
        1440.0,
    );
    let watching = db.watching(3);
    assert_eq!(watching.len(), 1, "one per anime, and only episodes");
    assert_eq!(watching[0].episode.number, 2);
    assert_eq!(watching[0].still.as_deref(), Some("/c/2.png"));
    let files = db.episode_files(17675);
    assert_eq!(
        files
            .iter()
            .map(|file| file.episode.number)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(files[0].position, 1400.0);
    assert_eq!(db.file_episode(other), None);
    let shelf = db.shelf(None);
    assert_eq!(shelf.len(), 1);
    assert_eq!((shelf[0].episodes, shelf[0].watched), (2, 1));
}

#[test]
fn a_folder_source_keeps_what_scans_saw_and_what_the_user_decided() {
    let db = Db::in_memory();
    let folder = db
        .add_source("folder", "番剧", "file:///anime/", "anime")
        .unwrap();
    assert_eq!(
        db.add_source("folder", "番剧", "file:///anime/", "series"),
        Some(folder),
        "one source per folder"
    );
    let one = db
        .saw(
            folder,
            "file:///anime/a/01.mkv",
            10,
            100,
            Some(&print("01", 10)),
        )
        .unwrap();
    let two = db
        .saw(
            folder,
            "file:///anime/a/02.mkv",
            20,
            100,
            Some(&print("02", 20)),
        )
        .unwrap();
    assert_eq!(
        db.saw(folder, "file:///anime/broken.mkv", 5, 100, None),
        None
    );
    let sources = db.sources();
    assert_eq!(
        sources.len(),
        1,
        "videos opened one by one are no source the user added"
    );
    assert_eq!(
        (
            sources[0].files,
            sources[0].unmatched,
            sources[0].content.as_str()
        ),
        (3, 3, "anime")
    );
    db.set_source_content(folder, "series");
    assert_eq!(db.sources()[0].content, "series");
    assert_eq!(db.fingerprint_of(one), Some(print("01", 10)));

    // The user matches two files at once, then unlinks one.
    let frieren = Anime {
        dandanplay_id: 17675,
        title: "葬送的芙莉莲".into(),
        image: String::new(),
    };
    let episode = |number: i64| Episode {
        dandanplay_id: 176_750_000 + number,
        number,
        title: String::new(),
    };
    assert!(db.link_files(&frieren, &[(one, episode(1)), (two, episode(2))]));
    assert_eq!(db.sources()[0].unmatched, 1);
    let entries = db.entries(Some(folder));
    assert_eq!(entries.len(), 3);
    assert!(
        entries
            .iter()
            .filter(|entry| entry.matched.is_some())
            .all(|entry| entry.manual)
    );
    db.unlink(two);
    // Automatic matching leaves both the matched and the unlinked file alone.
    db.link_episode(one, &frieren, &episode(7), false);
    db.link_episode(two, &frieren, &episode(7), false);
    assert_eq!(db.file_episode(one).unwrap().1.number, 1);
    assert_eq!(db.file_episode(two), None);
    let seen = db.seen(folder);
    let of = |url: &str| seen.iter().find(|seen| seen.url == url).unwrap().clone();
    assert!(of("file:///anime/a/01.mkv").matched && of("file:///anime/a/01.mkv").decided);
    assert!(!of("file:///anime/a/02.mkv").matched && of("file:///anime/a/02.mkv").decided);
    assert_eq!(of("file:///anime/broken.mkv").fingerprint, None);
    assert_eq!(db.shelf(Some(folder)).len(), 1);
    assert!(db.shelf(Some(folder + 1)).is_empty());

    // A file gone from the folder is forgotten; removing the source forgets
    // the rest, while the files and their episodes stay.
    db.forget(folder, &["file:///anime/broken.mkv".to_string()]);
    assert_eq!(db.entries(None).len(), 2);
    db.remove_source(folder);
    assert!(db.sources().is_empty() && db.entries(None).is_empty());
    assert_eq!(db.file_episode(one).unwrap().1.number, 1);
}

#[test]
fn a_stream_a_source_holds_is_its_file_when_played() {
    let db = Db::in_memory();
    let source = db
        .add_source("dandanplay", "书房", "http://192.168.1.2", "anime")
        .unwrap();
    let url = "http://192.168.1.2/api/v1/stream/id/c004e475";
    let file = db
        .saw(source, url, 100, 0, Some(&print("0377", 100)))
        .unwrap();
    // A stream cannot be read for its fingerprint; its address says which
    // file it is, so its progress and episode follow it.
    assert_eq!(db.opened(url, None), Some(file));
    assert_eq!(
        db.opened("http://192.168.1.2/api/v1/stream/id/other", None),
        None
    );
    db.set_source(source, "客厅", "http://192.168.1.3");
    let renamed = db.sources().into_iter().find(|s| s.id == source).unwrap();
    assert_eq!(
        (renamed.name.as_str(), renamed.location.as_str()),
        ("客厅", "http://192.168.1.3")
    );
}
