//! The media library: the shows the user has files of, and the folders it
//! scans for them. A folder is scanned as it is added, at every start and
//! when asked; the videos dandanplay recognizes join the wall, the others
//! wait among the videos not matched until the user matches them, one at a
//! time or a folder at a time. Scanning and asking dandanplay run on other
//! threads; the engine alone writes the database.

mod catalog_api;
mod catalog_view;
mod matching;
mod names;
mod remote;
mod scan;
mod shared;
#[cfg(test)]
mod tests;

use bern::{CommandError, media::MediaSource, query::Answer};
use erika_library_protocol::library::{
    self, FileNote, LibraryCrumb, LibraryDir, LibraryFile, LibraryFilter, LibraryFilterChip,
    LibraryFolder, LibraryLetter, LibraryListing, LibraryNotice, LibraryPoster, LibrarySort,
    LibrarySourceRow, SourceContent, SourceStatus, State,
};
use erika_store::{Entry, Source};
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
    /// A dandanplay client was read for mounting (or editing source
    /// `editing`) under a name at an address, or for refreshing a source.
    RemoteMount(u64, i64, String, String, Result<remote::Snapshot, String>),
    RemoteRefresh(i64, String, Result<remote::Snapshot, String>),
    #[cfg(not(target_arch = "wasm32"))]
    Scan(i64, ScanTicket, Scanned),
    #[cfg(not(target_arch = "wasm32"))]
    Match(i64, String, Matched),
    Picker(matching::Reply),
    Catalog(catalog_api::Reply),
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct ScanTicket {
    owner: String,
    generation: i64,
}

#[cfg(not(target_arch = "wasm32"))]
struct MatchingRun {
    id: String,
    videos: std::collections::VecDeque<scan::Video>,
    kind: Option<erika_catalog::Kind>,
    done: usize,
    total: usize,
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
    #[cfg(not(target_arch = "wasm32"))]
    scans: HashMap<i64, ScanTicket>,
    #[cfg(not(target_arch = "wasm32"))]
    match_runs: HashMap<i64, MatchingRun>,
    metadata: erika_catalog::Client,
    /// Sources whose folder could not be read at the last scan.
    missing: HashSet<i64>,
    /// Sources whose scan the user started, which say when it is done.
    announce: HashSet<i64>,
    /// The folder being browsed: its source and the names leading to it.
    at: Option<(i64, Vec<String>)>,
    session: Option<Session>,
    /// Whether the first wake, as the app starts, has come.
    started: bool,
    shared_request: u64,
    remote_request: u64,
    /// The file names of the videos shared devices serve, by address: their
    /// addresses name no file.
    contents: HashMap<i64, catalog_view::Content>,
    names: HashMap<String, String>,
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

use catalog_view::poster;

fn nothing() -> LibraryPoster {
    LibraryPoster {
        id: String::new(),
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

/// Whether a source is a device sharing its videos over the network, whose
/// videos are streamed rather than scanned.
fn network(kind: &str) -> bool {
    kind == shared::KIND || kind == remote::KIND
}

fn no_folder() -> LibraryFolder {
    LibraryFolder {
        source: 0,
        shared: false,
        dandanplay: false,
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

impl Default for LibraryEngine {
    fn default() -> Self {
        let mut engine = Self {
            db: erika_store::Db::open(),
            state: State {
                shared_prompt: false,
                shared_connecting: false,
                shared_error: String::new(),
                remote_prompt: false,
                remote_connecting: false,
                remote_error: String::new(),
                remote_editing: 0,
                remote_name: String::new(),
                remote_address: String::new(),
                remote_has_token: false,
                remote_saved: 0,
                show: nothing(),
                posters: Vec::new(),
                recent: Vec::new(),
                has_recent: false,
                opened: String::new(),
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
                catalog_result: None,
            },
            inbox: Arc::default(),
            busy: HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            scans: HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            match_runs: HashMap::new(),
            metadata: erika_catalog::Client::built_in("zh-CN").expect("built-in metadata endpoint"),
            missing: HashSet::new(),
            announce: HashSet::new(),
            at: None,
            session: None,
            started: false,
            shared_request: 0,
            remote_request: 0,
            contents: HashMap::new(),
            names: HashMap::new(),
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
        let entries = self.db.entries(None);
        let shelf = match self.db.catalog_shelf(None).and_then(|shelf| {
            self.load_contents(&entries)?;
            Ok(shelf)
        }) {
            Ok(shelf) => shelf,
            Err(error) => {
                self.notify(LibraryNotice::Failed {
                    reason: error.message,
                });
                return;
            }
        };
        let images = self.db.catalog_images();
        self.state.posters = shelf.iter().map(|s| poster(s, &images)).collect();
        let mut added = shelf.clone();
        added.sort_by(|a, b| b.added.cmp(&a.added));
        self.state.recent = added
            .iter()
            .take(RECENT)
            .map(|s| poster(s, &images))
            .collect();
        self.state.has_recent = !self.state.recent.is_empty();

        let sources = self.db.sources();
        self.state.sources = sources
            .iter()
            .map(|source| LibrarySourceRow {
                id: source.id,
                shared: network(&source.kind),
                dandanplay: source.kind == remote::KIND,
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
                    None if self.missing.contains(&source.id) && network(&source.kind) => {
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

        let mut unmatched: Vec<LibraryFile> = Vec::new();
        for source in &sources {
            let root = root_of(source);
            for entry in entries
                .iter()
                .filter(|entry| entry.source == source.id && self.content(entry).is_none())
            {
                let parts = root
                    .as_deref()
                    .and_then(|root| parts(root, &entry.url))
                    .unwrap_or_default();
                let name = self.name_of(&entry.url, &parts);
                let place = std::iter::once(source.name.as_str())
                    .chain(
                        parts
                            .iter()
                            .take(parts.len().saturating_sub(1))
                            .map(String::as_str),
                    )
                    .collect::<Vec<_>>()
                    .join(" › ");
                unmatched.push(self.file_row(entry, &name, place));
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
        if network(&source.kind) {
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
                files.push(self.file_row(entry, &parts[path.len()], String::new()));
            } else {
                let counts = dirs.entry(parts[path.len()].clone()).or_default();
                counts.0 += 1;
                counts.1 += self.content(entry).is_none() as i64;
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
            dandanplay: false,
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
            self.names.insert(video.url.clone(), video.name.clone());
            if let Some(file) = self.db.saw(
                id,
                &video.url,
                video.fingerprint.size as i64,
                0,
                Some(&video.fingerprint),
            ) && let Some((anime, episode)) = &video.matched
            {
                self.db.link_episode(file, anime, episode, false);
            }
        }
        self.db.forget(id, &gone);
    }

    /// Import file and provider identities; artwork is resolved from TMDB.
    fn import_remote(&mut self, id: i64, snapshot: &remote::Snapshot) {
        self.import_shared(id, &snapshot.videos);
    }

    /// The name of the video at `url`: the last of `parts` leading to it
    /// in its folder, the name its shared device gave, or its address's.
    fn name_of(&self, url: &str, parts: &[String]) -> String {
        parts
            .last()
            .cloned()
            .or_else(|| self.names.get(url).cloned())
            .or_else(|| bern::files::name(url))
            .unwrap_or_default()
    }

    fn shared_folder(&self, source: &Source, path: &[String], entries: &[Entry]) -> LibraryFolder {
        let mut dirs: BTreeMap<String, i64> = BTreeMap::new();
        let mut files = Vec::new();
        for entry in entries.iter().filter(|entry| entry.source == source.id) {
            let Some(content) = self.content(entry) else {
                if path.is_empty() {
                    files.push(self.file_row(entry, &self.name_of(&entry.url, &[]), String::new()));
                }
                continue;
            };
            if path.is_empty() {
                *dirs.entry(content.root.title.clone()).or_default() += 1;
            } else if path[0] == content.root.title {
                files.push(self.file_row(entry, &self.name_of(&entry.url, &[]), String::new()));
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
            dandanplay: source.kind == remote::KIND,
            content: content_of(&source.content),
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
            .find(|source| source.id == id && source.kind == remote::KIND)
        {
            let token = self
                .db
                .setting(&remote::token_setting(id))
                .unwrap_or_default();
            self.busy.insert(id, Busy::Scanning { done: 0, total: 0 });
            in_background(&self.inbox, move |send| {
                let result = remote::fetch(&source.location, &token);
                send(Done::RemoteRefresh(id, source.location, result))
            });
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
        #[cfg(not(target_arch = "wasm32"))]
        self.start_local_scan(id);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_local_scan(&mut self, id: i64) {
        let Some(root) = self
            .db
            .sources()
            .iter()
            .find(|source| source.id == id)
            .and_then(root_of)
        else {
            return;
        };
        let owner = erika_catalog::MediaId::new().0;
        let generation = match self.db.catalog_scan_begin(id, &owner, erika_store::now()) {
            Ok(generation) => generation,
            Err(error) => {
                eprintln!("erika: cannot claim source scan: {error}");
                self.announce.remove(&id);
                return;
            }
        };
        let ticket = ScanTicket { owner, generation };
        self.scans.insert(id, ticket.clone());
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
            scan::scan(&root, &known, &|done| {
                send(Done::Scan(id, ticket.clone(), done))
            })
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn took_scan(&mut self, id: i64, ticket: &ScanTicket, event: Scanned) -> bool {
        if self.scans.get(&id) != Some(ticket) {
            return false;
        }
        let at = erika_store::now();
        let mut changed = false;
        let result: erika_store::catalog::Result<()> = (|| {
            match event {
                Scanned::Walked { urls } => {
                    for paths in urls.chunks(10000) {
                        self.db.catalog_scan_saw(
                            id,
                            &ticket.owner,
                            ticket.generation,
                            paths,
                            at,
                        )?;
                    }
                }
                Scanned::Heartbeat | Scanned::Progress { .. } => {
                    self.db
                        .catalog_scan_saw(id, &ticket.owner, ticket.generation, &[], at)?;
                    if let Scanned::Progress { done, total } = event {
                        self.busy.insert(id, Busy::Scanning { done, total });
                    }
                }
                Scanned::Read { found, fingerprint } => {
                    self.db.catalog_scan_read(
                        id,
                        &ticket.owner,
                        ticket.generation,
                        &found.url,
                        found.size,
                        found.modified,
                        fingerprint.as_ref(),
                        at,
                    )?;
                    changed = true;
                }
                Scanned::Finished { failed } => {
                    changed |= self.db.catalog_scan_finish(
                        id,
                        &ticket.owner,
                        ticket.generation,
                        !failed,
                        at,
                    )? > 0;
                    self.scans.remove(&id);
                    self.busy.remove(&id);
                    if failed {
                        self.missing.insert(id);
                        if self.announce.remove(&id) {
                            self.notify(LibraryNotice::FolderUnreadable);
                        }
                    } else {
                        self.missing.remove(&id);
                        self.start_matching(id);
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("erika: source scan result rejected: {error}");
            self.scans.remove(&id);
            self.busy.remove(&id);
            self.missing.insert(id);
            if self.announce.remove(&id) {
                self.notify(LibraryNotice::FolderUnreadable);
            }
        }
        changed
    }

    /// Anonymous filename/TMDB resolution remains available without an account.
    fn start_matching(&mut self, id: i64) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(error) = self.prepare_matching(id) {
            eprintln!("erika: cannot prepare recognition: {error}");
            self.announce.remove(&id);
        }
        #[cfg(target_arch = "wasm32")]
        self.finish(id);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn prepare_matching(&mut self, id: i64) -> erika_store::catalog::Result<()> {
        if self.busy.contains_key(&id) {
            return Ok(());
        }
        let Some(source) = self.db.sources().into_iter().find(|s| s.id == id) else {
            return Ok(());
        };
        if source.kind == shared::KIND {
            return Ok(());
        }
        let kind = match source.content.as_str() {
            "movie" => Some(erika_catalog::Kind::Movie),
            "series" => Some(erika_catalog::Kind::Tv),
            _ => None,
        };
        let mut videos = std::collections::VecDeque::new();
        let mut files = HashSet::new();
        for seen in self
            .db
            .seen(id)
            .into_iter()
            .filter(|s| !s.matched && !s.decided)
        {
            let Some(fingerprint) = seen.fingerprint else {
                continue;
            };
            let Some(file) = self.db.file_id(&fingerprint) else {
                continue;
            };
            if !files.insert(file) {
                continue;
            }
            let revision = self.db.catalog_revision(file)?;
            let previous = self.db.catalog_file_job(file, erika_store::now())?;
            if previous.as_ref().is_some_and(|job| {
                job.input_revision == revision
                    && (matches!(job.state.as_str(), "running" | "pending" | "failed")
                        || (job.state == "retry" && job.next_attempt > erika_store::now()))
            }) {
                continue;
            }
            let name = self.name_of(&seen.url, &[]);
            let directory = bern::files::file_path(&seen.url)
                .and_then(|p| p.parent().map(|p| p.to_string_lossy().into_owned()))
                .unwrap_or_default();
            let job = match previous.filter(|j| j.state == "queued" && j.input_revision == revision)
            {
                Some(job) => job,
                None => {
                    let job = erika_store::catalog::CatalogJob {
                        id: erika_catalog::MediaId::new().0,
                        file: Some(file),
                        input_revision: revision,
                        phase: "recognition".into(),
                        state: "queued".into(),
                        payload: serde_json::json!({"name":name,"directory":directory,"kind":kind})
                            .to_string(),
                        error: String::new(),
                        next_attempt: 0,
                    };
                    self.db.catalog_job_enqueue(&job)?;
                    job
                }
            };
            videos.push_back(scan::Video {
                job: job.id,
                danmaku_revision: self.db.catalog_danmaku_revision(file)?,
                name,
                directory,
                fingerprint,
            });
        }
        if videos.is_empty() {
            self.finish(id);
            return Ok(());
        }
        self.match_runs.insert(
            id,
            MatchingRun {
                id: erika_catalog::MediaId::new().0,
                total: videos.len(),
                videos,
                kind,
                done: 0,
            },
        );
        self.next_matching_batch(id);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn next_matching_batch(&mut self, id: i64) {
        let Some(run) = self.match_runs.get_mut(&id) else {
            return;
        };
        let mut batch = Vec::new();
        while batch.len() < scan::AT_ONCE {
            let Some(video) = run.videos.pop_front() else {
                break;
            };
            match self.db.catalog_job_transition(
                &video.job,
                "queued",
                "running",
                "recognition",
                "",
                0,
            ) {
                Ok(job) if job.state == "running" => batch.push(video),
                Ok(_) => run.done += 1,
                Err(error) => {
                    eprintln!("erika: recognition claim skipped: {error}");
                    run.done += 1;
                }
            }
        }
        if batch.is_empty() {
            self.match_runs.remove(&id);
            self.busy.remove(&id);
            self.finish(id);
            return;
        }
        self.busy.insert(
            id,
            Busy::Matching {
                done: run.done,
                total: run.total,
            },
        );
        let (generation, kind, client) = (run.id.clone(), run.kind, self.metadata.clone());
        in_background(&self.inbox, move |send| {
            scan::match_videos(batch, &client, kind, &|event| {
                send(Done::Match(id, generation.clone(), event))
            });
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
        self.notify(LibraryNotice::Scanned {
            name: source.name,
            files: source.files,
            unmatched: source.unmatched,
        });
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
                Done::RemoteMount(request, editing, name, address, result) => {
                    if request != self.remote_request || !self.state.remote_prompt {
                        continue;
                    }
                    self.state.remote_connecting = false;
                    let snapshot = match result {
                        Ok(snapshot) => snapshot,
                        Err(error) => {
                            self.state.remote_error = error;
                            continue;
                        }
                    };
                    let id = if editing > 0 {
                        if !self.db.sources().iter().any(|source| source.id == editing) {
                            self.state.remote_error = "这个来源已经被移除了".into();
                            continue;
                        }
                        self.db.set_source(editing, &name, &address);
                        Some(editing)
                    } else {
                        self.db.add_source(remote::KIND, &name, &address, "anime")
                    };
                    let Some(id) = id else {
                        self.state.remote_error = "无法保存弹弹play共享媒体来源，请重试".into();
                        continue;
                    };
                    self.db.set_setting(
                        &remote::token_setting(id),
                        Some(snapshot.token.as_str()).filter(|token| !token.is_empty()),
                    );
                    self.import_remote(id, &snapshot);
                    self.missing.remove(&id);
                    self.state.remote_prompt = false;
                    self.state.remote_error.clear();
                    self.state.remote_editing = 0;
                    self.state.remote_saved += 1;
                    self.announce.insert(id);
                    self.start_matching(id);
                    changed = true;
                }
                Done::RemoteRefresh(id, address, result) => {
                    // A removed source's ID may have been reused by a different source.
                    if !self.db.sources().iter().any(|source| {
                        source.id == id && source.kind == remote::KIND && source.location == address
                    }) {
                        continue;
                    }
                    self.busy.remove(&id);
                    match result {
                        Ok(snapshot) => {
                            self.import_remote(id, &snapshot);
                            self.missing.remove(&id);
                            changed = true;
                            self.start_matching(id);
                        }
                        Err(_) => {
                            self.missing.insert(id);
                            self.announce.remove(&id);
                        }
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                Done::Scan(id, ticket, event) => {
                    changed |= self.took_scan(id, &ticket, event);
                }
                #[cfg(not(target_arch = "wasm32"))]
                Done::Match(id, generation, event) => {
                    if !self
                        .match_runs
                        .get(&id)
                        .is_some_and(|run| run.id == generation)
                    {
                        continue;
                    }
                    match event {
                        Matched::Resolved {
                            job,
                            danmaku_revision,
                            report,
                        } => {
                            match self
                                .db
                                .catalog_finish_resolution(&job, danmaku_revision, &report)
                            {
                                Ok(_) => changed = true,
                                Err(error) => {
                                    eprintln!("erika: recognition result rejected: {error}")
                                }
                            }
                            if let Some(run) = self.match_runs.get_mut(&id) {
                                run.done += 1;
                                self.busy.insert(
                                    id,
                                    Busy::Matching {
                                        done: run.done,
                                        total: run.total,
                                    },
                                );
                            }
                        }
                        Matched::Finished => self.next_matching_batch(id),
                    }
                }
                Done::Picker(reply) => self.take_picker(reply),
                Done::Catalog(reply) => { changed |= self.take_api(reply); }
            }
        }
        self.load();
        if changed {
            library::emit::files_changed();
        }
    }

    /// Capture both the address identity and the binding revision before any
    /// network work. A subsequent scan or another editor invalidates the batch.
    fn video_of(&self, key: i64) -> Option<(Video, String)> {
        let entry = self.db.entries(None).into_iter().find(|e| e.key == key)?;
        let source = self
            .db
            .sources()
            .into_iter()
            .find(|s| s.id == entry.source)?;
        let parts = root_of(&source)
            .and_then(|r| parts(&r, &entry.url))
            .unwrap_or_default();
        let name = self.name_of(&entry.url, &parts);
        let place = std::iter::once(source.name.clone())
            .chain(parts.iter().cloned())
            .collect::<Vec<_>>()
            .join(" › ");
        let directory = parts
            .get(parts.len().wrapping_sub(2))
            .map(String::as_str)
            .unwrap_or("");
        let mut hints = erika_catalog::names::with_directory(&name, directory);
        let (previous, revision) = match entry.file {
            Some(f) => {
                let revision = self.db.catalog_revision(f).ok()?;
                let previous = self.db.catalog_binding(f).ok()?;
                if previous.as_ref().is_some_and(|b| b.revision != revision) {
                    return None;
                }
                (previous, revision)
            }
            None => (None, 0),
        };
        // A reliable existing canonical binding is a better preview than a name.
        if let Some(content) = self.content(&entry) {
            if content
                .items
                .iter()
                .all(|n| n.kind == erika_catalog::Kind::Episode)
                && content
                    .items
                    .iter()
                    .all(|n| n.parent == content.items[0].parent)
            {
                hints.season = content.items[0].season_number;
                hints.episodes = content
                    .items
                    .iter()
                    .filter_map(|n| n.episode_number)
                    .collect();
            }
        }
        let current = match self.file_row(&entry, &name, String::new()).note {
            FileNote::Content { title } => title,
            _ => String::new(),
        };
        let address = entry.file.map(|file| erika_store::catalog::AddressCheck {
            key,
            source: entry.source,
            url: entry.url.clone(),
            file,
        });
        Some((
            Video {
                key,
                name,
                file: entry.file,
                address,
                revision,
                hints,
                previous,
                current,
            },
            place,
        ))
    }

    fn with_session(&mut self, change: impl FnOnce(&mut Session)) {
        if let Some(session) = self.session.as_mut() {
            change(session);
        }
        self.load();
    }

    fn listing(
        &self,
        source: i64,
        filter: LibraryFilter,
        sort: LibrarySort,
    ) -> erika_store::catalog::Result<LibraryListing> {
        let images = self.db.catalog_images();
        let mut posters: Vec<LibraryPoster> = if source == 0 {
            self.state.posters.clone()
        } else {
            self.db
                .catalog_shelf(Some(source))?
                .iter()
                .map(|s| poster(s, &images))
                .collect()
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
                    .map(|p| p.id.clone())
                    .unwrap_or_default();
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
        Ok(LibraryListing {
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
        })
    }
}

fn unknown_source() -> CommandError {
    CommandError::new("unknown_source", "unknown library source")
}

fn unknown_file() -> CommandError {
    CommandError::new("unknown_file", "unknown library file")
}

/// A real TMDB episode may be numbered zero. Bound numeric input before arithmetic.
fn typed(text: &str) -> Option<i64> {
    text.trim()
        .parse()
        .ok()
        .filter(|number| (0..=u32::MAX as i64).contains(number))
}

impl LibraryEngine {
    /// Open poster `id`, from the home page when `home`.
    fn open_poster(&mut self, id: String, home: bool) -> Result<(), CommandError> {
        let show = self
            .state
            .posters
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| CommandError::new("unknown_show", "unknown library show"))?;
        self.state.show = show;
        self.state.opened = id.clone();
        library::emit::open_show(id, home);
        Ok(())
    }
}

impl library::Commands for LibraryEngine {
    fn catalog_fetch(&mut self, request: String, reference: library::CatalogReference, season: Option<i64>) -> Result<(), CommandError> {
        self.api_fetch(request, reference, season)
    }
    fn catalog_resolve(&mut self, request: String, file: i64, revision: i64, dry_run: bool) -> Result<(), CommandError> {
        self.api_resolve(request, file, revision, dry_run)
    }
    fn catalog_bind(&mut self, request: String, bindings: Vec<library::CatalogBinding>, preview: bool) -> Result<(), CommandError> {
        let batch = erika_store::catalog::CatalogBatch { bindings: catalog_api::input_bindings(bindings)?, ..Default::default() };
        self.api_apply(request, batch, preview)
    }
    fn catalog_unbind(&mut self, request: String, file: i64, revision: i64, preview: bool) -> Result<(), CommandError> {
        self.catalog_bind(request, vec![library::CatalogBinding { file, revision, decision: library::CatalogDecision::Blocked, items: vec![], evidence: "explicit unbind".into() }], preview)
    }
    fn catalog_bind_danmaku(&mut self, request: String, bindings: Vec<library::CatalogDanmaku>, preview: bool) -> Result<(), CommandError> {
        let batch = erika_store::catalog::CatalogBatch { danmaku: catalog_api::input_danmaku(bindings)?, ..Default::default() };
        self.api_apply(request, batch, preview)
    }
    fn catalog_map_provider(&mut self, request: String, mappings: Vec<library::CatalogMapping>, preview: bool) -> Result<(), CommandError> {
        let batch = erika_store::catalog::CatalogBatch { links: catalog_api::input_mappings(mappings)?, ..Default::default() };
        self.api_apply(request, batch, preview)
    }
    fn catalog_allow_auto(&mut self, file: i64, revision: i64) -> Result<(), CommandError> {
        let revision = u64::try_from(revision).map_err(|_| CommandError::new("invalid", "版本不能为负数"))?;
        self.db.catalog_allow_auto(file, revision).map_err(catalog_api::store_error)?;
        self.load();
        library::emit::files_changed();
        Ok(())
    }
    fn catalog_undo(&mut self, change: String) -> Result<(), CommandError> {
        self.db.catalog_undo(&change).map_err(catalog_api::store_error)?;
        self.load();
        library::emit::files_changed();
        Ok(())
    }
    fn open(&mut self, id: String) -> Result<(), CommandError> {
        self.open_poster(id, false)
    }

    fn open_recent(&mut self, id: String) -> Result<(), CommandError> {
        self.open_poster(id, true)
    }

    fn close(&mut self) -> Result<(), CommandError> {
        self.state.opened.clear();
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

    fn open_remote_mount(&mut self) -> Result<(), CommandError> {
        self.remote_request += 1;
        self.state.remote_prompt = true;
        self.state.remote_connecting = false;
        self.state.remote_error.clear();
        self.state.remote_editing = 0;
        self.state.remote_name.clear();
        self.state.remote_address.clear();
        self.state.remote_has_token = false;
        Ok(())
    }

    fn edit_remote(&mut self, source: i64) -> Result<(), CommandError> {
        let known = self
            .db
            .sources()
            .into_iter()
            .find(|known| known.id == source && known.kind == remote::KIND)
            .ok_or_else(unknown_source)?;
        self.remote_request += 1;
        self.state.remote_prompt = true;
        self.state.remote_connecting = false;
        self.state.remote_error.clear();
        self.state.remote_editing = source;
        self.state.remote_name = known.name;
        self.state.remote_address = known.location;
        self.state.remote_has_token = self.db.setting(&remote::token_setting(source)).is_some();
        Ok(())
    }

    fn cancel_remote_mount(&mut self) -> Result<(), CommandError> {
        self.remote_request += 1;
        self.state.remote_prompt = false;
        self.state.remote_connecting = false;
        self.state.remote_error.clear();
        self.state.remote_editing = 0;
        Ok(())
    }

    fn mount_remote(
        &mut self,
        name: String,
        address: String,
        token: String,
    ) -> Result<(), CommandError> {
        if self.state.remote_connecting {
            return Ok(());
        }
        self.state.remote_prompt = true;
        self.state.remote_error.clear();
        let editing = self.state.remote_editing;
        let known = self
            .db
            .sources()
            .into_iter()
            .find(|known| editing > 0 && known.id == editing);
        let address = match (address.trim(), &known) {
            ("", Some(known)) => known.location.clone(),
            (address, _) => match erika_dandanplay::remote::address(address) {
                Ok(address) => address,
                Err(error) => {
                    self.state.remote_error = error;
                    return Ok(());
                }
            },
        };
        if editing > 0
            && self.db.sources().iter().any(|other| {
                other.id != editing && other.kind == remote::KIND && other.location == address
            })
        {
            self.state.remote_error = "这个地址已经挂载过了".into();
            return Ok(());
        }
        let name = match (name.trim(), &known) {
            ("", Some(known)) => known.name.clone(),
            ("", None) => address.clone(),
            (name, _) => name.to_owned(),
        };
        let token = match token.trim() {
            "" if editing > 0 => self
                .db
                .setting(&remote::token_setting(editing))
                .unwrap_or_default(),
            token => token.to_owned(),
        };
        self.remote_request += 1;
        let request = self.remote_request;
        self.state.remote_connecting = true;
        in_background(&self.inbox, move |send| {
            let result = remote::fetch(&address, &token);
            send(Done::RemoteMount(request, editing, name, address, result));
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
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.scans.remove(&source);
            self.match_runs.remove(&source);
        }
        self.db.set_setting(&remote::token_setting(source), None);
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

    fn play_file(&mut self, key: i64) -> Result<(), CommandError> {
        // Re-read the address rather than trusting an earlier visible row.
        // Neither a provider episode nor a canonical binding is required.
        let entry = self.db.entries(None).into_iter()
            .find(|entry| entry.key == key && entry.file.is_some())
            .ok_or_else(unknown_file)?;
        let source = self.db.sources().into_iter().find(|source| source.id == entry.source)
            .ok_or_else(unknown_file)?;
        let parts = root_of(&source).and_then(|root| parts(&root, &entry.url)).unwrap_or_default();
        let title = self.name_of(&entry.url, &parts);
        library::emit::play_source(MediaSource::new(entry.url), title);
        Ok(())
    }

    fn assign(&mut self, key: i64) -> Result<(), CommandError> {
        let (video, place) = self.video_of(key).ok_or_else(unknown_file)?;
        let words = names::search_words(&video.name);
        let session = Session::one(video, place);
        self.begin(session, words);
        self.load();
        Ok(())
    }

    fn unlink(&mut self, key: i64) -> Result<(), CommandError> {
        let (video, _) = self.video_of(key).ok_or_else(unknown_file)?;
        self.block_content(&video)?;
        self.notify(LibraryNotice::Unlinked);
        library::emit::files_changed();
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
        let found: Vec<(Video, String)> =
            keys.iter().filter_map(|key| self.video_of(*key)).collect();
        let videos: Vec<Video> = found.into_iter().map(|(video, _)| video).collect();
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
        let session = Session::folder(videos, place);
        self.begin(session, words);
        self.load();
        Ok(())
    }

    fn match_search(&mut self, text: String) -> Result<(), CommandError> {
        self.search(text);
        self.load();
        Ok(())
    }

    fn match_work(&mut self, id: String) -> Result<(), CommandError> {
        self.choose_work(id)?;
        self.load();
        Ok(())
    }

    fn match_season(&mut self, id: String) -> Result<(), CommandError> {
        self.choose_season(id)?;
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
        self.with_session(|session| session.type_number(key, text));
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
        self.confirm_content()?;
        self.load();
        Ok(())
    }

    fn match_unlink(&mut self) -> Result<(), CommandError> {
        if let Some(video) = self
            .session
            .as_ref()
            .filter(|s| s.single)
            .and_then(|s| s.videos.first())
        {
            self.block_content(video)?;
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
    fn catalog_inspect(&self, file: i64, answer: Answer<library::CatalogInspection>) { answer.send(self.api_inspect(file)); }
    fn catalog_media(&self, id: String, answer: Answer<library::CatalogNode>) { answer.send(self.api_media(&id)); }
    fn catalog_children(&self, id: String, answer: Answer<Vec<library::CatalogNode>>) { answer.send(self.api_children(&id)); }
    fn catalog_search(&self, kind: library::CatalogKind, text: String, year: Option<i64>, page: i64, answer: Answer<library::CatalogSearch>) { self.api_search(kind, text, year, page, answer); }
    fn catalog_job(&self, id: String, answer: Answer<library::CatalogJob>) { answer.send(self.api_job(&id)); }
    fn catalog_receipt(&self, request: String, answer: Answer<library::CatalogReceipt>) { answer.send(self.api_receipt(&request)); }
    fn browse(
        &self,
        source: i64,
        filter: LibraryFilter,
        sort: LibrarySort,
        answer: Answer<LibraryListing>,
    ) {
        answer.send(
            self.listing(source, filter, sort)
                .map_err(|e| CommandError::new(e.code, e.message)),
        );
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
