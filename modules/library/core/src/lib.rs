//! The media library: the shows the user has files of, and the folders it
//! scans for them. A folder is scanned as it is added, at every start and
//! when asked; the videos dandanplay recognizes join the wall, the others
//! wait among the videos not matched until the user matches them, one at a
//! time or a folder at a time. Scanning and asking dandanplay run on other
//! threads; the engine alone writes the database.

mod matching;
mod names;
mod scan;
mod shared;
#[cfg(test)]
mod tests;

use bern::{CommandError, media::MediaSource, query::Answer};
use erika_dandanplay::{Details, Found};
use erika_library_protocol::library::{
    self, FileNote, LibraryCrumb, LibraryDir, LibraryFile, LibraryFilter, LibraryFilterChip,
    LibraryFolder, LibraryLetter, LibraryListing, LibraryNotice, LibraryPoster, LibrarySort,
    LibrarySourceRow, SourceContent, SourceStatus, State,
};
use erika_store::{Anime, Entry, Source};
use matching::{Session, Video};
use scan::{Matched, Scanned};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// How many shows 「最近添加」 on the home page holds.
const RECENT: usize = 8;

/// What work on other threads came back with.
enum Done {
    SharedMount(u64, String, String, Result<Vec<shared::Video>, String>),
    SharedRefresh(i64, String, Result<Vec<shared::Video>, String>),
    Scan(i64, Scanned),
    Match(i64, Matched),
    Details(i64, Result<Details, erika_dandanplay::Error>),
    Search(u64, Result<Vec<Found>, erika_dandanplay::Error>),
}

/// What a source is busy with, and how far it is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Busy {
    Scanning { done: usize, total: usize },
    Matching { done: usize, total: usize },
}

pub struct LibraryEngine {
    state: State,
    db: erika_store::Db,
    inbox: Arc<Mutex<Vec<Done>>>,
    busy: HashMap<i64, Busy>,
    /// Sources whose folder could not be read at the last scan.
    missing: HashSet<i64>,
    /// Sources whose scan the user started, which say when it is done.
    announce: HashSet<i64>,
    /// The folder being browsed: its source and the names leading to it.
    at: Option<(i64, Vec<String>)>,
    session: Option<Session>,
    /// The details dandanplay gave this run, and the anime asked about.
    details: HashMap<i64, Details>,
    asked: HashSet<i64>,
    /// Whether the first wake, as the app starts, has come.
    started: bool,
    shared_request: u64,
}

/// Run `work` on another thread, handing what it sends to the engine,
/// which is woken to take it in. The browser has no threads and tests want
/// the work done before they look: there it runs at once.
fn in_background(
    inbox: &Arc<Mutex<Vec<Done>>>,
    work: impl FnOnce(&(dyn Fn(Done) + Sync)) + Send + 'static,
) {
    let inbox = Arc::clone(inbox);
    let waker = bern::wake::waker();
    let run = move || {
        let send = |done: Done| {
            inbox
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(done);
            if let Some(waker) = &waker {
                waker.wake();
            }
        };
        work(&send);
    };
    if cfg!(any(target_arch = "wasm32", test)) {
        run();
    } else {
        std::thread::spawn(run);
    }
}

/// The letter a title files under: its first letter, or the first letter
/// of the pinyin of its first Chinese character; `#` for anything else.
fn initial(title: &str) -> String {
    use pinyin::ToPinyin;
    title
        .chars()
        .find_map(|c| {
            if c.is_ascii_alphabetic() {
                Some(c.to_ascii_uppercase().to_string())
            } else {
                c.to_pinyin()
                    .map(|pinyin| pinyin.first_letter().to_uppercase())
            }
        })
        .unwrap_or_else(|| "#".into())
}

fn poster(shelf: &erika_store::Shelf) -> LibraryPoster {
    let (episodes, watched) = (shelf.episodes.max(1), shelf.watched);
    LibraryPoster {
        id: shelf.anime.dandanplay_id,
        title: shelf.anime.title.clone(),
        poster: MediaSource::new(shelf.anime.image.clone()),
        initial: initial(&shelf.anime.title),
        episodes,
        watched,
        rating: 0.0,
        progress: watched as f64 / episodes as f64,
        watching: watched > 0 && watched < episodes,
        movie: false,
    }
}

fn nothing() -> LibraryPoster {
    LibraryPoster {
        id: 0,
        title: String::new(),
        poster: MediaSource::new(""),
        initial: String::new(),
        episodes: 0,
        watched: 0,
        rating: 0.0,
        progress: 0.0,
        watching: false,
        movie: false,
    }
}

/// What a source holds, as the database writes it and back.
fn content_of(text: &str) -> SourceContent {
    match text {
        "series" => SourceContent::Series,
        "movie" => SourceContent::Movie,
        _ => SourceContent::Anime,
    }
}

fn content_text(content: &SourceContent) -> &'static str {
    match content {
        SourceContent::Anime => "anime",
        SourceContent::Series => "series",
        SourceContent::Movie => "movie",
    }
}

/// Whether the user's own server recognizes what a source holds; dandanplay
/// recognizes anime.
fn by_server(content: &str) -> bool {
    content != "anime"
}

/// Whether the service that knows `id` can be asked: the user's server for
/// its (negative) ids, dandanplay with an account for the rest.
fn reachable(id: i64) -> bool {
    if erika_dandanplay::custom::is_ours(id) {
        erika_dandanplay::custom::server().is_some()
    } else {
        erika_dandanplay::token().is_some()
    }
}

fn no_folder() -> LibraryFolder {
    LibraryFolder {
        source: 0,
        shared: false,
        content: SourceContent::Anime,
        title: String::new(),
        location: String::new(),
        crumbs: Vec::new(),
        dirs: Vec::new(),
        files: Vec::new(),
        can_batch: false,
        empty: true,
        missing: false,
    }
}

/// The folder a source is, on this device.
fn root_of(source: &Source) -> Option<PathBuf> {
    bern::files::file_path(&source.location)
}

/// The names leading from `root` to the video at `url`, its own last.
fn parts(root: &Path, url: &str) -> Option<Vec<String>> {
    let path = bern::files::file_path(url)?;
    let rest = path.strip_prefix(root).ok()?;
    Some(
        rest.components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect(),
    )
}

/// Names in the order people count: `2` before `10`.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let number = |chars: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = chars.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        chars.next();
                    }
                    digits.trim_start_matches('0').to_string()
                };
                let (x, y) = (number(&mut a), number(&mut b));
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(&y));
                if order.is_ne() {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                a.next();
                b.next();
            }
        }
    }
}

fn note(entry: &Entry) -> FileNote {
    match (&entry.matched, entry.file) {
        (Some((anime, episode)), _) if entry.manual => FileNote::Manual {
            anime: anime.title.clone(),
            episode: episode.number,
        },
        (Some((anime, episode)), _) => FileNote::Matched {
            anime: anime.title.clone(),
            episode: episode.number,
        },
        (None, None) => FileNote::Unreadable,
        (None, Some(_)) => FileNote::Unmatched,
    }
}

fn file_row(entry: &Entry, name: &str, place: String) -> LibraryFile {
    LibraryFile {
        key: entry.key,
        show: entry
            .matched
            .as_ref()
            .map_or(0, |(anime, _)| anime.dandanplay_id),
        name: name.to_string(),
        note: note(entry),
        matched: entry.matched.is_some(),
        readable: entry.file.is_some(),
        place,
    }
}

impl Default for LibraryEngine {
    fn default() -> Self {
        let mut engine = Self {
            db: erika_store::Db::open(),
            state: State {
                shared_prompt: false,
                shared_connecting: false,
                shared_error: String::new(),
                show: nothing(),
                posters: Vec::new(),
                recent: Vec::new(),
                has_recent: false,
                opened: 0,
                sources: Vec::new(),
                has_sources: false,
                several_sources: false,
                unmatched: Vec::new(),
                unmatched_count: 0,
                browsing: false,
                folder: no_folder(),
                matching: matching::closed(),
                notice: None,
                notice_revision: 0,
            },
            inbox: Arc::default(),
            busy: HashMap::new(),
            missing: HashSet::new(),
            announce: HashSet::new(),
            at: None,
            session: None,
            details: HashMap::new(),
            asked: HashSet::new(),
            started: false,
            shared_request: 0,
        };
        engine.load();
        engine
    }
}

impl LibraryEngine {
    fn notify(&mut self, notice: LibraryNotice) {
        self.state.notice = Some(notice);
        self.state.notice_revision += 1;
    }

    /// Everything as the database has it now.
    fn load(&mut self) {
        let shelf = self.db.shelf(None);
        self.state.posters = shelf.iter().map(poster).collect();
        let mut added = shelf.clone();
        added.sort_by(|a, b| b.added.cmp(&a.added));
        self.state.recent = added.iter().take(RECENT).map(poster).collect();
        self.state.has_recent = !self.state.recent.is_empty();

        let sources = self.db.sources();
        self.state.sources = sources
            .iter()
            .map(|source| LibrarySourceRow {
                id: source.id,
                shared: source.kind == shared::KIND,
                name: source.name.clone(),
                content: content_of(&source.content),
                location: root_of(source).map_or_else(
                    || source.location.clone(),
                    |root| root.display().to_string(),
                ),
                status: match self.busy.get(&source.id) {
                    Some(Busy::Scanning { done, total }) => SourceStatus::Scanning {
                        done: *done as i64,
                        total: *total as i64,
                    },
                    Some(Busy::Matching { done, total }) => SourceStatus::Matching {
                        done: *done as i64,
                        total: *total as i64,
                    },
                    None if self.missing.contains(&source.id) && source.kind == shared::KIND => {
                        SourceStatus::Offline
                    }
                    None if self.missing.contains(&source.id) => SourceStatus::Missing,
                    None => SourceStatus::Ready {
                        files: source.files,
                    },
                },
                unmatched: source.unmatched,
            })
            .collect();
        self.state.has_sources = !sources.is_empty();
        self.state.several_sources = sources.len() > 1;

        let entries = self.db.entries(None);
        let mut unmatched: Vec<LibraryFile> = Vec::new();
        for source in &sources {
            let root = root_of(source);
            for entry in entries
                .iter()
                .filter(|entry| entry.source == source.id && entry.matched.is_none())
            {
                let parts = root
                    .as_deref()
                    .and_then(|root| parts(root, &entry.url))
                    .unwrap_or_default();
                let name = parts
                    .last()
                    .cloned()
                    .or_else(|| bern::files::name(&entry.url))
                    .unwrap_or_default();
                let place = std::iter::once(source.name.as_str())
                    .chain(
                        parts
                            .iter()
                            .take(parts.len().saturating_sub(1))
                            .map(String::as_str),
                    )
                    .collect::<Vec<_>>()
                    .join(" › ");
                unmatched.push(file_row(entry, &name, place));
            }
        }
        self.state.unmatched_count = unmatched.len() as i64;
        self.state.unmatched = unmatched;

        if self
            .at
            .as_ref()
            .is_some_and(|(id, _)| !sources.iter().any(|source| source.id == *id))
        {
            self.at = None;
        }
        self.state.browsing = self.at.is_some();
        self.state.folder = self.folder(&sources, &entries);
        self.state.matching = self
            .session
            .as_ref()
            .map_or_else(matching::closed, Session::view);
        self.ask_posters(&shelf);
    }

    /// The folder being browsed: the folders in it with how many videos
    /// they hold, and its own videos.
    fn folder(&self, sources: &[Source], entries: &[Entry]) -> LibraryFolder {
        let Some((id, path)) = &self.at else {
            return no_folder();
        };
        let Some(source) = sources.iter().find(|source| source.id == *id) else {
            return no_folder();
        };
        if source.kind == shared::KIND {
            return self.shared_folder(source, path, entries);
        }
        let Some(root) = root_of(source) else {
            return no_folder();
        };
        let mut dirs: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        let mut files = Vec::new();
        for entry in entries.iter().filter(|entry| entry.source == *id) {
            let Some(parts) = parts(&root, &entry.url) else {
                continue;
            };
            if parts.len() <= path.len() || parts[..path.len()] != path[..] {
                continue;
            }
            if parts.len() == path.len() + 1 {
                files.push(file_row(entry, &parts[path.len()], String::new()));
            } else {
                let counts = dirs.entry(parts[path.len()].clone()).or_default();
                counts.0 += 1;
                counts.1 += entry.matched.is_none() as i64;
            }
        }
        files.sort_by(|a, b| natural(&a.name, &b.name));
        let mut dirs: Vec<LibraryDir> = dirs
            .into_iter()
            .map(|(name, (files, unmatched))| LibraryDir {
                name,
                files,
                unmatched,
            })
            .collect();
        dirs.sort_by(|a, b| natural(&a.name, &b.name));
        let mut crumbs = vec![LibraryCrumb {
            depth: 0,
            name: source.name.clone(),
        }];
        crumbs.extend(path.iter().enumerate().map(|(index, name)| LibraryCrumb {
            depth: index as i64 + 1,
            name: name.clone(),
        }));
        let missing = self.missing.contains(id);
        LibraryFolder {
            source: *id,
            shared: false,
            content: content_of(&source.content),
            title: path.last().cloned().unwrap_or_else(|| source.name.clone()),
            location: path
                .iter()
                .fold(root, |at, name| at.join(name))
                .display()
                .to_string(),
            crumbs,
            can_batch: !missing && files.len() >= 2,
            empty: dirs.is_empty() && files.is_empty(),
            missing,
            dirs,
            files,
        }
    }

    fn import_shared(&mut self, id: i64, videos: &[shared::Video]) {
        let present: HashSet<&str> = videos.iter().map(|video| video.url.as_str()).collect();
        let gone: Vec<String> = self
            .db
            .seen(id)
            .into_iter()
            .map(|seen| seen.url)
            .filter(|url| !present.contains(url.as_str()))
            .collect();
        for video in videos {
            if let Some(file) = self.db.saw(
                id,
                &video.url,
                video.fingerprint.size as i64,
                0,
                Some(&video.fingerprint),
            ) {
                self.db
                    .link_episode(file, &video.anime, &video.episode, false);
            }
        }
        self.db.forget(id, &gone);
    }

    fn shared_folder(&self, source: &Source, path: &[String], entries: &[Entry]) -> LibraryFolder {
        let mut dirs: BTreeMap<String, i64> = BTreeMap::new();
        let mut files = Vec::new();
        for entry in entries.iter().filter(|entry| entry.source == source.id) {
            let Some((anime, episode)) = &entry.matched else {
                continue;
            };
            if path.is_empty() {
                *dirs.entry(anime.title.clone()).or_default() += 1;
            } else if path[0] == anime.title {
                files.push(file_row(entry, &episode.title, String::new()));
            }
        }
        files.sort_by(|a, b| natural(&a.name, &b.name));
        let mut crumbs = vec![LibraryCrumb {
            depth: 0,
            name: source.name.clone(),
        }];
        crumbs.extend(path.iter().enumerate().map(|(index, name)| LibraryCrumb {
            depth: index as i64 + 1,
            name: name.clone(),
        }));
        LibraryFolder {
            source: source.id,
            shared: true,
            content: SourceContent::Anime,
            title: path.last().cloned().unwrap_or_else(|| source.name.clone()),
            location: source.location.clone(),
            crumbs,
            empty: dirs.is_empty() && files.is_empty(),
            dirs: dirs
                .into_iter()
                .map(|(name, files)| LibraryDir {
                    name,
                    files,
                    unmatched: 0,
                })
                .collect(),
            files,
            can_batch: false,
            missing: self.missing.contains(&source.id),
        }
    }

    /// Scan source `id`, unless it is being scanned.
    fn start_scan(&mut self, id: i64) {
        if self.busy.contains_key(&id) {
            return;
        }
        if let Some(source) = self
            .db
            .sources()
            .into_iter()
            .find(|source| source.id == id && source.kind == shared::KIND)
        {
            self.busy.insert(id, Busy::Scanning { done: 0, total: 0 });
            in_background(&self.inbox, move |send| {
                let result = shared::fetch(&source.location);
                send(Done::SharedRefresh(id, source.location, result))
            });
            return;
        }
        if cfg!(target_arch = "wasm32") {
            self.notify(LibraryNotice::CannotScan);
            return;
        }
        let Some(root) = self
            .db
            .sources()
            .iter()
            .find(|source| source.id == id)
            .and_then(root_of)
        else {
            return;
        };
        let known: HashMap<String, scan::Known> = self
            .db
            .seen(id)
            .into_iter()
            .map(|seen| {
                (
                    seen.url,
                    scan::Known {
                        size: seen.size,
                        modified: seen.modified,
                        fingerprinted: seen.fingerprint.is_some(),
                    },
                )
            })
            .collect();
        self.busy.insert(id, Busy::Scanning { done: 0, total: 0 });
        in_background(&self.inbox, move |send| {
            scan::scan(&root, &known, &|done| send(Done::Scan(id, done)))
        });
    }

    /// Ask dandanplay about the videos of source `id` not matched yet and
    /// not unlinked by the user, when an account is signed in.
    fn start_matching(&mut self, id: i64) {
        if self
            .db
            .sources()
            .iter()
            .any(|source| source.id == id && source.kind == shared::KIND)
        {
            return;
        }
        if self.busy.contains_key(&id) {
            return;
        }
        let custom = self
            .db
            .sources()
            .iter()
            .find(|source| source.id == id)
            .is_some_and(|source| by_server(&source.content));
        let videos: Vec<(String, erika_store::Fingerprint)> = self
            .db
            .seen(id)
            .into_iter()
            .filter(|seen| !seen.matched && !seen.decided)
            .filter_map(|seen| {
                Some((
                    bern::files::name(&seen.url).unwrap_or_default(),
                    seen.fingerprint?,
                ))
            })
            .collect();
        let can_ask = if custom {
            erika_dandanplay::custom::server().is_some()
        } else {
            erika_dandanplay::token().is_some()
        };
        if videos.is_empty() || !can_ask {
            self.finish(id);
            return;
        }
        self.busy.insert(
            id,
            Busy::Matching {
                done: 0,
                total: videos.len(),
            },
        );
        in_background(&self.inbox, move |send| {
            scan::match_videos(videos, custom, &|done| send(Done::Match(id, done)))
        });
    }

    /// Source `id` is scanned and matched: say so when the user asked.
    fn finish(&mut self, id: i64) {
        if !self.announce.remove(&id) {
            return;
        }
        let Some(source) = self.db.sources().into_iter().find(|source| source.id == id) else {
            return;
        };
        if source.unmatched > 0
            && by_server(&source.content)
            && erika_dandanplay::custom::server().is_none()
        {
            self.notify(LibraryNotice::NoServer);
        } else if source.unmatched > 0
            && !by_server(&source.content)
            && erika_dandanplay::token().is_none()
        {
            self.notify(LibraryNotice::SignInToMatch);
        } else {
            self.notify(LibraryNotice::Scanned {
                name: source.name,
                files: source.files,
                unmatched: source.unmatched,
            });
        }
    }

    /// Ask dandanplay about anime `id`, once a run.
    fn ask_details(&mut self, id: i64) {
        if let Some(details) = self.details.get(&id).cloned() {
            self.took_details(id, &details);
            return;
        }
        if !self.asked.insert(id) || !reachable(id) {
            return;
        }
        in_background(&self.inbox, move |send| {
            send(Done::Details(id, erika_dandanplay::details(id)))
        });
    }

    /// Posters for the shows on the wall that have none: dandanplay's
    /// matching names an anime but gives no picture of it.
    fn ask_posters(&mut self, shelf: &[erika_store::Shelf]) {
        for id in shelf
            .iter()
            .filter(|shelf| shelf.anime.image.is_empty())
            .map(|shelf| shelf.anime.dandanplay_id)
            .collect::<Vec<_>>()
        {
            if !self.details.contains_key(&id) {
                self.ask_details(id);
            }
        }
    }

    fn took_details(&mut self, asked: i64, details: &Details) {
        if let Some(session) = self.session.as_mut() {
            session.details(asked, details);
        }
        if let Some(known) = self
            .db
            .anime(details.anime_id)
            .filter(|known| known.image.is_empty() && !details.image_url.is_empty())
        {
            self.db.remember_anime(&Anime {
                image: details.image_url.clone(),
                ..known
            });
        }
    }

    /// Take in what the other threads came back with.
    fn take_done(&mut self) {
        let done: Vec<Done> = std::mem::take(
            &mut *self
                .inbox
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        if done.is_empty() {
            return;
        }
        let mut changed = false;
        for done in done {
            match done {
                Done::SharedMount(request, name, address, result) => {
                    if request != self.shared_request || !self.state.shared_prompt {
                        continue;
                    }
                    self.state.shared_connecting = false;
                    match result {
                        Ok(videos) => {
                            if let Some(id) =
                                self.db.add_source(shared::KIND, &name, &address, "anime")
                            {
                                self.import_shared(id, &videos);
                                self.missing.remove(&id);
                                self.state.shared_prompt = false;
                                self.state.shared_error.clear();
                                changed = true;
                            } else {
                                self.state.shared_error = "无法保存共享媒体来源，请重试".into();
                            }
                        }
                        Err(error) => self.state.shared_error = error,
                    }
                }
                Done::SharedRefresh(id, address, result) => {
                    // A removed source's ID may have been reused by a different source.
                    if !self.db.sources().iter().any(|source| {
                        source.id == id && source.kind == shared::KIND && source.location == address
                    }) {
                        continue;
                    }
                    self.busy.remove(&id);
                    match result {
                        Ok(videos) => {
                            self.import_shared(id, &videos);
                            self.missing.remove(&id);
                            changed = true;
                            self.finish(id);
                        }
                        Err(_) => {
                            self.missing.insert(id);
                            self.announce.remove(&id);
                        }
                    }
                }
                Done::Scan(id, Scanned::Walked { urls }) => {
                    let present: HashSet<String> = urls.into_iter().collect();
                    let gone: Vec<String> = self
                        .db
                        .seen(id)
                        .into_iter()
                        .map(|seen| seen.url)
                        .filter(|url| !present.contains(url))
                        .collect();
                    changed |= !gone.is_empty();
                    self.db.forget(id, &gone);
                    self.missing.remove(&id);
                }
                Done::Scan(id, Scanned::Progress { done, total }) => {
                    self.busy.insert(id, Busy::Scanning { done, total });
                }
                Done::Scan(id, Scanned::Read { found, fingerprint }) => {
                    self.db.saw(
                        id,
                        &found.url,
                        found.size,
                        found.modified,
                        fingerprint.as_ref(),
                    );
                    changed = true;
                }
                Done::Scan(id, Scanned::Finished { failed }) => {
                    self.busy.remove(&id);
                    if failed {
                        self.missing.insert(id);
                        if self.announce.remove(&id) {
                            self.notify(LibraryNotice::FolderUnreadable);
                        }
                    } else {
                        self.start_matching(id);
                    }
                }
                Done::Match(id, Matched::Progress { done, total }) => {
                    self.busy.insert(id, Busy::Matching { done, total });
                }
                Done::Match(
                    _,
                    Matched::Episode {
                        fingerprint,
                        anime,
                        episode,
                    },
                ) => {
                    if let Some(file) = self.db.file_id(&fingerprint) {
                        self.db.link_episode(file, &anime, &episode, false);
                        changed = true;
                    }
                }
                Done::Match(id, Matched::Finished) => {
                    self.busy.remove(&id);
                    self.finish(id);
                }
                Done::Details(id, Ok(details)) => {
                    self.details.insert(id, details.clone());
                    self.took_details(id, &details);
                    changed = true;
                }
                Done::Details(id, Err(error)) => {
                    eprintln!("erika: details of {id}: {}", error.message)
                }
                Done::Search(search, result) => {
                    if let Some(session) = self
                        .session
                        .as_mut()
                        .filter(|session| session.search == search)
                    {
                        session.searching = false;
                        session.search_failed = result.is_err();
                        session.results = result.unwrap_or_default().into_iter().take(8).collect();
                    }
                }
            }
        }
        self.load();
        if changed {
            library::emit::files_changed();
        }
    }

    /// The video with address `key` as the matching page takes it, where it
    /// is, and whether its source is recognized by the user's own server.
    fn video_of(&self, key: i64) -> Option<(Video, String, bool)> {
        let entry = self
            .db
            .entries(None)
            .into_iter()
            .find(|entry| entry.key == key)?;
        let source = self
            .db
            .sources()
            .into_iter()
            .find(|source| source.id == entry.source)?;
        let custom = by_server(&source.content);
        let parts = root_of(&source)
            .and_then(|root| parts(&root, &entry.url))
            .unwrap_or_default();
        let name = parts
            .last()
            .cloned()
            .or_else(|| bern::files::name(&entry.url))
            .unwrap_or_default();
        let place = std::iter::once(source.name.clone())
            .chain(parts.iter().cloned())
            .collect::<Vec<_>>()
            .join(" › ");
        Some((
            Video {
                key,
                name,
                file: entry.file,
                now: entry
                    .matched
                    .map(|(anime, episode)| (anime, episode.number)),
            },
            place,
            custom,
        ))
    }

    /// Open matching: the episodes of its anime are asked for, or, when it
    /// has none yet, dandanplay is searched by `words`.
    fn begin(&mut self, session: Session, words: String) {
        let chosen = session
            .chosen
            .as_ref()
            .map(|chosen| chosen.anime.dandanplay_id);
        self.session = Some(session);
        match chosen {
            Some(id) => self.ask_details(id),
            None => self.search(words),
        }
    }

    fn search(&mut self, text: String) {
        let text = text.trim().to_string();
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if text.is_empty() {
            return;
        }
        session.search += 1;
        session.searching = true;
        session.search_failed = false;
        let (search, custom) = (session.search, session.custom);
        in_background(&self.inbox, move |send| {
            let found = if custom {
                erika_dandanplay::custom::search(&text)
            } else {
                erika_dandanplay::search(&text)
            };
            send(Done::Search(search, found))
        });
    }

    fn with_session(&mut self, change: impl FnOnce(&mut Session)) {
        if let Some(session) = self.session.as_mut() {
            change(session);
        }
        self.load();
    }

    /// Tell the user's server which episodes these files are, so their
    /// fingerprints are enough next time, anywhere.
    fn confirm_on_server(
        &self,
        anime: &Anime,
        links: &[(erika_store::FileId, erika_store::Episode)],
    ) {
        let names: HashMap<erika_store::FileId, String> = self
            .session
            .iter()
            .flat_map(|session| session.videos.iter())
            .filter_map(|video| Some((video.file?, video.name.clone())))
            .collect();
        let confirmed: Vec<(String, erika_store::Fingerprint, erika_store::Episode)> = links
            .iter()
            .filter_map(|(file, episode)| {
                Some((
                    names.get(file).cloned().unwrap_or_default(),
                    self.db.fingerprint_of(*file)?,
                    episode.clone(),
                ))
            })
            .collect();
        let anime = anime.clone();
        in_background(&self.inbox, move |_| {
            for (name, print, episode) in confirmed {
                let done =
                    erika_dandanplay::custom::confirm(&erika_dandanplay::custom::Confirmed {
                        file_name: &name,
                        md5: &print.md5,
                        size: print.size,
                        anime_id: anime.dandanplay_id,
                        episode_id: episode.dandanplay_id,
                        number: episode.number,
                        anime_title: &anime.title,
                        episode_title: &episode.title,
                        image: &anime.image,
                    });
                if let Err(error) = done {
                    eprintln!("erika: confirm {name} on the server: {}", error.message);
                }
            }
        });
    }

    /// Tell the user's server to forget what file `file` was.
    fn forget_on_server(&self, file: erika_store::FileId) {
        if let Some(print) = self.db.fingerprint_of(file) {
            in_background(&self.inbox, move |_| {
                if let Err(error) = erika_dandanplay::custom::forget(&print.md5, print.size) {
                    eprintln!("erika: forget a file on the server: {}", error.message);
                }
            });
        }
    }

    fn listing(&self, source: i64, filter: LibraryFilter, sort: LibrarySort) -> LibraryListing {
        let mut posters: Vec<LibraryPoster> = if source == 0 {
            self.state.posters.clone()
        } else {
            self.db.shelf(Some(source)).iter().map(poster).collect()
        };
        let total = posters.len() as i64;
        let watching = posters.iter().filter(|p| p.watching).count() as i64;
        let unwatched = posters.iter().filter(|p| p.watched == 0).count() as i64;
        let finished = total - watching - unwatched;
        posters.retain(|p| match filter {
            LibraryFilter::All => true,
            LibraryFilter::Watching => p.watching,
            LibraryFilter::Unwatched => p.watched == 0,
            LibraryFilter::Finished => p.watched >= p.episodes,
        });
        match sort {
            LibrarySort::Recent => posters.sort_by_key(|p| !p.watching),
            LibrarySort::Name => posters.sort_by(|a, b| {
                a.initial
                    .cmp(&b.initial)
                    .then_with(|| a.title.cmp(&b.title))
            }),
            LibrarySort::Rating => posters.sort_by(|a, b| b.rating.total_cmp(&a.rating)),
        }
        let letters = "ABCDEFGHJKLMNOPQRSTWXYZ"
            .chars()
            .map(|c| {
                let letter = c.to_string();
                let target = posters
                    .iter()
                    .find(|p| p.initial == letter)
                    .map_or(0, |p| p.id);
                LibraryLetter { letter, target }
            })
            .collect();
        let filters = [
            ("all", LibraryFilter::All, total),
            ("watching", LibraryFilter::Watching, watching),
            ("unwatched", LibraryFilter::Unwatched, unwatched),
            ("finished", LibraryFilter::Finished, finished),
        ]
        .into_iter()
        .map(|(id, value, count)| LibraryFilterChip {
            id: id.into(),
            value,
            count,
        })
        .collect();
        LibraryListing {
            filters,
            empty: posters.is_empty(),
            posters,
            letters,
            total,
            watching,
            unwatched,
            finished,
            source_name: self
                .state
                .sources
                .iter()
                .find(|row| row.id == source)
                .map(|row| row.name.clone())
                .unwrap_or_default(),
        }
    }
}

fn unknown_source() -> CommandError {
    CommandError::new("unknown_source", "unknown library source")
}

fn unknown_file() -> CommandError {
    CommandError::new("unknown_file", "unknown library file")
}

/// A number the user typed, when it is one above zero.
fn typed(text: &str) -> Option<i64> {
    text.trim().parse().ok().filter(|number| *number > 0)
}

impl library::Commands for LibraryEngine {
    fn open(&mut self, id: i64) -> Result<(), CommandError> {
        let show = self
            .state
            .posters
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| CommandError::new("unknown_show", "unknown library show"))?;
        self.state.show = show;
        self.state.opened = id;
        library::emit::open_show(id);
        Ok(())
    }

    fn close(&mut self) -> Result<(), CommandError> {
        self.state.opened = 0;
        Ok(())
    }

    fn history_changed(&mut self) -> Result<(), CommandError> {
        self.load();
        Ok(())
    }

    fn account_changed(&mut self, signed_in: bool) -> Result<(), CommandError> {
        if signed_in {
            for source in self.db.sources() {
                self.start_matching(source.id);
            }
            self.asked.clear();
        }
        self.load();
        Ok(())
    }

    fn add_folder(
        &mut self,
        folder: MediaSource,
        content: SourceContent,
    ) -> Result<(), CommandError> {
        if cfg!(target_arch = "wasm32") {
            self.notify(LibraryNotice::CannotScan);
            return Ok(());
        }
        let Some(path) = bern::files::file_path(&folder.url).filter(|path| path.is_dir()) else {
            self.notify(LibraryNotice::FolderUnreadable);
            return Ok(());
        };
        let location = MediaSource::file(&path).url;
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        if let Some(id) = self
            .db
            .add_source("folder", &name, &location, content_text(&content))
        {
            self.announce.insert(id);
            self.start_scan(id);
        }
        self.load();
        Ok(())
    }

    fn open_shared_mount(&mut self) -> Result<(), CommandError> {
        self.shared_request += 1;
        self.state.shared_prompt = true;
        self.state.shared_connecting = false;
        self.state.shared_error.clear();
        Ok(())
    }

    fn cancel_shared_mount(&mut self) -> Result<(), CommandError> {
        self.shared_request += 1;
        self.state.shared_prompt = false;
        self.state.shared_connecting = false;
        self.state.shared_error.clear();
        Ok(())
    }

    fn mount_shared(&mut self, name: String, address: String) -> Result<(), CommandError> {
        if self.state.shared_connecting {
            return Ok(());
        }
        self.state.shared_prompt = true;
        self.state.shared_error.clear();
        let address = match shared::address(&address) {
            Ok(address) => address,
            Err(error) => {
                self.state.shared_error = error;
                return Ok(());
            }
        };
        let name = if name.trim().is_empty() {
            address.clone()
        } else {
            name.trim().to_owned()
        };
        self.shared_request += 1;
        let request = self.shared_request;
        self.state.shared_connecting = true;
        in_background(&self.inbox, move |send| {
            let result = shared::fetch(&address);
            send(Done::SharedMount(request, name, address, result));
        });
        Ok(())
    }

    fn set_content(&mut self, source: i64, content: SourceContent) -> Result<(), CommandError> {
        let known = self
            .db
            .sources()
            .into_iter()
            .find(|known| known.id == source)
            .ok_or_else(unknown_source)?;
        self.db.set_source_content(source, content_text(&content));
        self.notify(LibraryNotice::Retyped {
            name: known.name,
            content,
        });
        self.start_matching(source);
        self.load();
        Ok(())
    }

    fn rescan(&mut self, source: i64) -> Result<(), CommandError> {
        if !self.db.sources().iter().any(|known| known.id == source) {
            return Err(unknown_source());
        }
        self.announce.insert(source);
        self.start_scan(source);
        self.load();
        Ok(())
    }

    fn remove_source(&mut self, source: i64) -> Result<(), CommandError> {
        let known = self
            .db
            .sources()
            .into_iter()
            .find(|known| known.id == source)
            .ok_or_else(unknown_source)?;
        self.db.remove_source(source);
        self.busy.remove(&source);
        self.missing.remove(&source);
        self.notify(LibraryNotice::Removed { name: known.name });
        self.load();
        library::emit::files_changed();
        Ok(())
    }

    fn open_source(&mut self, source: i64) -> Result<(), CommandError> {
        if !self.db.sources().iter().any(|known| known.id == source) {
            return Err(unknown_source());
        }
        self.at = Some((source, Vec::new()));
        self.load();
        Ok(())
    }

    fn open_dir(&mut self, name: String) -> Result<(), CommandError> {
        if let Some((_, path)) = self.at.as_mut() {
            path.push(name);
        }
        self.load();
        Ok(())
    }

    fn go_up(&mut self) -> Result<(), CommandError> {
        match self.at.as_mut() {
            Some((_, path)) if !path.is_empty() => {
                path.pop();
            }
            _ => self.at = None,
        }
        self.load();
        Ok(())
    }

    fn go_to(&mut self, depth: i64) -> Result<(), CommandError> {
        if let Some((_, path)) = self.at.as_mut() {
            path.truncate(depth.max(0) as usize);
        }
        self.load();
        Ok(())
    }

    fn close_source(&mut self) -> Result<(), CommandError> {
        self.at = None;
        self.session = None;
        self.load();
        Ok(())
    }

    fn assign(&mut self, key: i64) -> Result<(), CommandError> {
        let (video, place, custom) = self.video_of(key).ok_or_else(unknown_file)?;
        let words = names::search_words(&video.name);
        let mut session = Session::one(video, place);
        session.custom = custom;
        self.begin(session, words);
        self.load();
        Ok(())
    }

    fn unlink(&mut self, key: i64) -> Result<(), CommandError> {
        let (video, _, custom) = self.video_of(key).ok_or_else(unknown_file)?;
        if let Some(file) = video.file {
            if custom {
                self.forget_on_server(file);
            }
            self.db.unlink(file);
            self.notify(LibraryNotice::Unlinked);
            library::emit::files_changed();
        }
        self.load();
        Ok(())
    }

    fn match_folder(&mut self) -> Result<(), CommandError> {
        let keys: Vec<i64> = self
            .state
            .folder
            .files
            .iter()
            .map(|file| file.key)
            .collect();
        let found: Vec<(Video, String, bool)> =
            keys.iter().filter_map(|key| self.video_of(*key)).collect();
        let custom = found.iter().any(|(_, _, custom)| *custom);
        let videos: Vec<Video> = found.into_iter().map(|(video, _, _)| video).collect();
        if videos.is_empty() {
            return Ok(());
        }
        let place = self
            .state
            .folder
            .crumbs
            .iter()
            .map(|crumb| crumb.name.as_str())
            .collect::<Vec<_>>()
            .join(" › ");
        let words = names::search_words(&self.state.folder.title);
        let mut session = Session::folder(videos, place);
        session.custom = custom;
        self.begin(session, words);
        self.load();
        Ok(())
    }

    fn match_search(&mut self, text: String) -> Result<(), CommandError> {
        self.search(text);
        self.load();
        Ok(())
    }

    fn match_anime(&mut self, id: i64) -> Result<(), CommandError> {
        let Some(found) = self.session.as_ref().and_then(|session| {
            session
                .results
                .iter()
                .find(|found| found.anime_id == id)
                .cloned()
        }) else {
            return Ok(());
        };
        let anime = Anime {
            dandanplay_id: found.anime_id,
            title: found.anime_title,
            image: found.image_url,
        };
        self.with_session(|session| session.choose(anime, found.episode_count));
        self.ask_details(id);
        self.load();
        Ok(())
    }

    fn match_by_order(&mut self, by_order: bool) -> Result<(), CommandError> {
        self.with_session(|session| session.by_order = by_order);
        Ok(())
    }

    fn match_start(&mut self, text: String) -> Result<(), CommandError> {
        if let Some(start) = typed(&text) {
            self.with_session(|session| session.start = start);
        }
        Ok(())
    }

    fn match_align(&mut self, text: String) -> Result<(), CommandError> {
        if let Some(episode) = typed(&text) {
            self.with_session(|session| session.align(episode));
        }
        Ok(())
    }

    fn match_edit(&mut self, key: i64) -> Result<(), CommandError> {
        self.with_session(|session| {
            session.editing = if session.editing == Some(key) {
                None
            } else {
                Some(key)
            }
        });
        Ok(())
    }

    fn match_number(&mut self, key: i64, text: String) -> Result<(), CommandError> {
        self.with_session(|session| session.type_number(key, typed(&text)));
        Ok(())
    }

    fn match_toggle(&mut self, key: i64) -> Result<(), CommandError> {
        self.with_session(|session| session.toggle(key));
        Ok(())
    }

    fn match_odd(&mut self, only: bool) -> Result<(), CommandError> {
        self.with_session(|session| session.odd_only = only);
        Ok(())
    }

    fn match_confirm(&mut self) -> Result<(), CommandError> {
        let Some((anime, links)) = self.session.as_ref().and_then(Session::links) else {
            return Ok(());
        };
        let anime = Anime {
            image: self
                .db
                .anime(anime.dandanplay_id)
                .map(|known| known.image)
                .filter(|image| !image.is_empty())
                .unwrap_or(anime.image.clone()),
            ..anime
        };
        if self.db.link_files(&anime, &links) {
            if self.session.as_ref().is_some_and(|session| session.custom) {
                self.confirm_on_server(&anime, &links);
            }
            let numbers: Vec<i64> = links.iter().map(|(_, episode)| episode.number).collect();
            let (first, last) = (
                numbers.iter().min().copied().unwrap_or(0),
                numbers.iter().max().copied().unwrap_or(0),
            );
            self.notify(if links.len() == 1 {
                LibraryNotice::MatchedOne {
                    anime: anime.title.clone(),
                    episode: first,
                }
            } else {
                LibraryNotice::MatchedMany {
                    count: links.len() as i64,
                    anime: anime.title.clone(),
                    first,
                    last,
                }
            });
            self.session = None;
            library::emit::files_changed();
        } else {
            self.notify(LibraryNotice::MatchFailed);
        }
        self.load();
        Ok(())
    }

    fn match_unlink(&mut self) -> Result<(), CommandError> {
        let file = self
            .session
            .as_ref()
            .filter(|session| session.single)
            .and_then(|session| session.videos[0].file);
        if let Some(file) = file {
            if self.session.as_ref().is_some_and(|session| session.custom) {
                self.forget_on_server(file);
            }
            self.db.unlink(file);
            self.notify(LibraryNotice::Unlinked);
            self.session = None;
            library::emit::files_changed();
        }
        self.load();
        Ok(())
    }

    fn match_cancel(&mut self) -> Result<(), CommandError> {
        self.session = None;
        self.load();
        Ok(())
    }
}

impl library::Queries for LibraryEngine {
    fn browse(
        &self,
        source: i64,
        filter: LibraryFilter,
        sort: LibrarySort,
        answer: Answer<LibraryListing>,
    ) {
        answer.ok(self.listing(source, filter, sort));
    }
}

impl library::Engine for LibraryEngine {
    fn state(&self) -> &State {
        &self.state
    }

    /// Scans and requests come back; as the app starts, every folder is
    /// scanned for what changed while it was closed.
    fn wake(&mut self) {
        self.take_done();
        if !self.started {
            self.started = true;
            for source in self.db.sources() {
                self.start_scan(source.id);
            }
            self.take_done();
            self.load();
        }
    }
}
