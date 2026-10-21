use super::*;

#[test]
fn folder_names_drop_what_file_systems_refuse() {
    assert_eq!(folder_name("[Sakura] Frieren: S2 - 01.mkv").as_deref(), Some("[Sakura] Frieren_ S2 - 01"));
    assert_eq!(folder_name(" ..  "), None);
    // As NipaPlay's: a path keeps its last part.
    assert_eq!(folder_name("a/b?c"), Some("b_c".into()));
}

#[test]
fn files_know_their_names_and_whether_they_play() {
    let file = TaskFile { index: 0, path: "Frieren S2/[Sakura] 01.MKV".into(), length: 1 };
    assert_eq!(file.name(), "[Sakura] 01.MKV");
    assert!(file.is_video());
    assert!(!TaskFile { index: 1, path: "a/readme.txt".into(), length: 1 }.is_video());
    assert!(is_magnet(" magnet:?xt=urn:btih:abc"));
    assert!(!is_magnet("https://x"));
}

/// A whole download from a .torrent file, offline: the session starts,
/// reads the torrent, adds it into a folder of its own, lists it and
/// removes it. librqbit needs no peers for any of this.
#[cfg(not(any(target_arch = "wasm32", target_os = "tvos")))]
#[test]
fn a_torrent_file_is_read_added_and_removed() {
    let root = std::env::temp_dir().join(format!("erika-torrent-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    // A one-file torrent of 16 KiB, its pieces never fetched.
    let torrent = root.join("frieren.torrent");
    std::fs::write(&torrent, one_file_torrent("[Sakura] Frieren - 01.mkv", 16_384)).unwrap();
    start(&root.join("session"), &root.join("downloads")).unwrap();
    let preview = preview_file(&torrent).unwrap();
    assert_eq!(preview.files.len(), 1);
    assert_eq!((preview.total, preview.folder_name.as_str()), (16_384, "[Sakura] Frieren - 01"));
    let id = add(&preview, &root.join("downloads"), true).unwrap();
    let task = tasks().into_iter().find(|task| task.id == id).unwrap();
    assert_eq!(task.total, 16_384);
    assert!(task.folder.ends_with("[Sakura] Frieren - 01"));
    assert_eq!(files(id).len(), 1);
    let url = stream_url(id, 0, "a b.mkv").unwrap();
    assert!(url.ends_with(&format!("/torrent/{id}/stream/0/a%20b.mkv")));
    // The stream server answers from librqbit's runtime: the head comes at
    // once, before any piece does.
    assert!(head(&url).starts_with("HTTP/1.1 200 OK"), "{}", head(&url));
    forget(id).unwrap();
    assert!(tasks().iter().all(|task| task.id != id));
    let _ = std::fs::remove_dir_all(&root);
}

/// The head of the answer to a HEAD request for `url`.
#[cfg(not(any(target_arch = "wasm32", target_os = "tvos")))]
fn head(url: &str) -> String {
    use std::io::{Read, Write};
    let rest = url.trim_start_matches("http://");
    let (address, path) = rest.split_once('/').unwrap();
    let mut socket = std::net::TcpStream::connect(address).unwrap();
    socket.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    write!(socket, "HEAD /{path} HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
    let mut answer = String::new();
    let _ = socket.read_to_string(&mut answer);
    answer
}

/// A bencoded single-file torrent of `length` bytes in 16 KiB pieces.
#[cfg(not(any(target_arch = "wasm32", target_os = "tvos")))]
fn one_file_torrent(name: &str, length: u64) -> Vec<u8> {
    let piece = 16_384u64;
    let pieces = length.div_ceil(piece) as usize;
    let mut info = Vec::new();
    info.extend_from_slice(format!("d6:lengthi{length}e4:name{}:{name}12:piece lengthi{piece}e6:pieces{}:", name.len(), pieces * 20).as_bytes());
    info.extend(std::iter::repeat_n(0u8, pieces * 20));
    info.push(b'e');
    let mut torrent = b"d8:announce22:http://127.0.0.1:1/ann4:info".to_vec();
    torrent.extend(info);
    torrent.push(b'e');
    torrent
}
