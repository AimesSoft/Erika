//! Downloads with librqbit (`erika-torrent`), as NipaPlay's downloader runs
//! them. The session starts on another thread as the app does, its tasks
//! resumed from the app's data; the page's clock reads their progress. A
//! magnet link or a .torrent file is read on another thread to list its
//! files, then added into the folder chosen, in a folder of its own if the
//! user likes. A finished download says so and, if the user wants, hands
//! its folder to the library; a video plays from its file once done and
//! streams while it downloads.

use bern::{CommandError, media::MediaSource};
use erika_downloads_protocol::downloads::{
    self, AddProblem, AddSheet, DataSize, DownloadNotice, DownloadTab, DownloadTask, DownloadVideo,
    PreviewFile, SavePlace, State,
};
use erika_torrent::{Preview, Task, TaskFile, TaskState};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// The settings kept: the folders downloaded into (newest first, one a
/// line), the one chosen, a folder per task, and joining the library.
const PLACES: &str = "downloads.places";
const PLACE: &str = "downloads.place";
const OWN_FOLDER: &str = "downloads.own_folder";
const TO_LIBRARY: &str = "downloads.to_library";
/// How many folders the quick choice keeps, and files the sheet lists.
const MAX_PLACES: usize = 6;
const PREVIEW_FILES: usize = 30;

/// What work on other threads came back with.
enum Done {
    Started(erika_torrent::Result<()>),
    /// A torrent read for the sheet: which reading, from what, and it.
    Read(u64, String, erika_torrent::Result<Preview>),
    /// A task added, for the pending row it replaces.
    Added(u64, erika_torrent::Result<usize>),
}

/// A task being read and added, shown until librqbit has it.
struct Pending {
    key: u64,
    name: String,
}

pub struct DownloadsEngine {
    state: State,
    db: erika_store::Db,
    inbox: Arc<Mutex<Vec<Done>>>,
    starting: bool,
    started: bool,
    tasks: Vec<Task>,
    /// Each task's files, read once its torrent is known.
    files: HashMap<usize, Vec<TaskFile>>,
    /// The torrent read in the sheet and where it came from.
    preview: Option<(String, Preview)>,
    reading: u64,
    place: PathBuf,
    places: Vec<PathBuf>,
    own_folder: bool,
    pending: Vec<Pending>,
    next_pending: u64,
    /// The downloads seen finished; one finishing later is announced.
    finished: HashSet<String>,
    seen: bool,
}

/// Run `work` on another thread; its result waits in `inbox` and the engine
/// is woken to take it. The browser has no threads and tests want the work
/// done before they look: there it runs at once.
fn in_background(inbox: &Arc<Mutex<Vec<Done>>>, work: impl FnOnce() -> Done + Send + 'static) {
    let inbox = Arc::clone(inbox);
    let waker = bern::wake::waker();
    let run = move || {
        let done = work();
        inbox
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(done);
        if let Some(waker) = waker {
            waker.wake();
        }
    };
    if cfg!(any(target_arch = "wasm32", test)) {
        run();
    } else {
        std::thread::spawn(run);
    }
}

/// Where downloads go until the user picks: the user's Downloads folder on
/// a computer, where people look for them; elsewhere the app's data.
#[cfg(target_arch = "wasm32")]
fn default_place() -> PathBuf {
    PathBuf::from("downloads")
}

#[cfg(not(target_arch = "wasm32"))]
fn default_place() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    match home
        .map(|home| home.join("Downloads"))
        .filter(|downloads| downloads.is_dir())
    {
        Some(downloads) => downloads.join("Erika"),
        None => bern::storage::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("downloads"),
    }
}

/// A size as the rows write it.
fn data_size(bytes: u64) -> DataSize {
    const KB: f64 = 1024.0;
    let bytes = bytes as f64;
    let tenth = |value: f64| (value * 10.0).round() / 10.0;
    match bytes {
        0.0 => DataSize::Unknown,
        _ if bytes < KB * KB => DataSize::Kilobytes {
            kilobytes: (bytes / KB).ceil(),
        },
        _ if bytes < KB * KB * KB => DataSize::Megabytes {
            megabytes: (bytes / KB / KB).round(),
        },
        _ => DataSize::Gigabytes {
            gigabytes: tenth(bytes / KB / KB / KB),
        },
    }
}

fn unknown_task() -> CommandError {
    CommandError::new("unknown_task", "unknown download")
}

/// The name a magnet link gives its torrent (`dn`), if any.
fn magnet_name(link: &str) -> String {
    link.split(['?', '&'])
        .find_map(|part| part.strip_prefix("dn="))
        .map(|name| {
            let name = name.replace('+', " ");
            let bytes = name.as_bytes();
            let mut decoded = Vec::with_capacity(bytes.len());
            let mut at = 0;
            while at < bytes.len() {
                match (
                    bytes[at],
                    bytes.get(at + 1..at + 3).and_then(|hex| {
                        u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()
                    }),
                ) {
                    (b'%', Some(byte)) => {
                        decoded.push(byte);
                        at += 3;
                    }
                    (byte, _) => {
                        decoded.push(byte);
                        at += 1;
                    }
                }
            }
            String::from_utf8_lossy(&decoded).into_owned()
        })
        .unwrap_or_default()
}

impl Default for DownloadsEngine {
    fn default() -> Self {
        let db = erika_store::Db::open();
        let places: Vec<PathBuf> = db
            .setting(PLACES)
            .map(|text| {
                text.lines()
                    .filter(|line| !line.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        let place = db
            .setting(PLACE)
            .map(PathBuf::from)
            .unwrap_or_else(default_place);
        let own_folder = db.setting(OWN_FOLDER).is_none_or(|on| on == "1");
        let to_library = db.setting(TO_LIBRARY).is_none_or(|on| on == "1");
        let mut engine = Self {
            state: State {
                tab: DownloadTab::Active,
                active: Vec::new(),
                completed: Vec::new(),
                active_count: 0,
                completed_count: 0,
                running: false,
                available: erika_torrent::available() && bern::storage::data_dir().is_some(),
                adding: AddSheet {
                    open: false,
                    reading: false,
                    read: false,
                    name: String::new(),
                    size: DataSize::Unknown,
                    file_count: 0,
                    files: Vec::new(),
                    more_files: 0,
                    folder_name: String::new(),
                    place: String::new(),
                    places: Vec::new(),
                    has_places: false,
                    own_folder,
                    problem: None,
                },
                deleting: 0,
                deleting_title: String::new(),
                to_library,
                notice: None,
                notice_revision: 0,
            },
            db,
            inbox: Arc::default(),
            starting: false,
            started: false,
            tasks: Vec::new(),
            files: HashMap::new(),
            preview: None,
            reading: 0,
            place,
            places,
            own_folder,
            pending: Vec::new(),
            next_pending: 1,
            finished: HashSet::new(),
            seen: false,
        };
        engine.refresh();
        engine
    }
}

impl DownloadsEngine {
    fn notify(&mut self, notice: DownloadNotice) {
        self.state.notice = Some(notice);
        self.state.notice_revision += 1;
    }

    fn save_places(&self) {
        let text: Vec<String> = self
            .places
            .iter()
            .map(|place| place.display().to_string())
            .collect();
        self.db.set_setting(PLACES, Some(&text.join("\n")));
        self.db
            .set_setting(PLACE, Some(&self.place.display().to_string()));
    }

    /// Download into `place` from now on, and keep it among the choices.
    fn choose(&mut self, place: PathBuf) {
        self.places.retain(|known| *known != place);
        self.places.insert(0, place.clone());
        self.places.truncate(MAX_PLACES);
        self.place = place;
        self.save_places();
    }

    fn task(&self, id: i64) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id as i64 + 1 == id)
    }

    /// The folder a finished task joins the library by: the folder chosen
    /// for it, which holds its own folder, or its own.
    fn library_folder(&self, task: &Task) -> PathBuf {
        std::iter::once(&self.place)
            .chain(self.places.iter())
            .find(|place| task.folder.starts_with(place))
            .cloned()
            .unwrap_or_else(|| task.folder.clone())
    }

    /// The tasks as librqbit has them now; a download finished since the
    /// last look is announced and, if wanted, joins the library.
    fn read_tasks(&mut self) {
        if !self.started {
            return;
        }
        self.tasks = erika_torrent::tasks();
        for task in &self.tasks {
            if task.total > 0 && !self.files.contains_key(&task.id) {
                let files = erika_torrent::files(task.id);
                if !files.is_empty() {
                    self.files.insert(task.id, files);
                }
            }
        }
        let done: Vec<Task> = self
            .tasks
            .iter()
            .filter(|task| task.finished && !self.finished.contains(&task.info_hash))
            .cloned()
            .collect();
        for task in done {
            self.finished.insert(task.info_hash.clone());
            if self.seen {
                self.notify(DownloadNotice::Finished {
                    title: task.name.clone(),
                });
                if self.state.to_library {
                    downloads::emit::finished(MediaSource::file(self.library_folder(&task)));
                }
            }
        }
        self.seen = true;
        self.files
            .retain(|id, _| self.tasks.iter().any(|task| task.id == *id));
    }

    fn row(&self, task: &Task) -> DownloadTask {
        let files = self
            .files
            .get(&task.id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let videos: Vec<DownloadVideo> = files
            .iter()
            .filter(|file| file.is_video())
            .map(|file| DownloadVideo {
                index: file.index as i64,
                name: file.name().to_string(),
            })
            .collect();
        let progress = if task.total > 0 {
            (task.progress as f64 / task.total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let tenth = |value: f64| (value * 10.0).round() / 10.0;
        DownloadTask {
            id: task.id as i64 + 1,
            title: task.name.clone(),
            fetching: task.total == 0 && task.state == TaskState::Initializing,
            size: data_size(task.total),
            progress: if task.finished { 1.0 } else { progress },
            percent: if task.finished {
                100
            } else {
                (progress * 100.0).floor() as i64
            },
            speed: tenth(task.down),
            upload: tenth(task.up),
            running: !task.finished
                && matches!(task.state, TaskState::Live | TaskState::Initializing),
            completed: task.finished,
            failed: task.state == TaskState::Error,
            error: task.error.clone().unwrap_or_default(),
            first: false,
            first_video: videos.first().map_or(0, |video| video.index),
            playable: !videos.is_empty(),
            single: videos.len() == 1,
            videos,
            folder: task.folder.display().to_string(),
        }
    }

    /// The page as the tasks, the sheet and the settings stand.
    fn refresh(&mut self) {
        let mut active: Vec<DownloadTask> = self
            .pending
            .iter()
            .map(|pending| DownloadTask {
                id: -(pending.key as i64),
                title: pending.name.clone(),
                fetching: true,
                size: DataSize::Unknown,
                progress: 0.0,
                percent: 0,
                speed: 0.0,
                upload: 0.0,
                running: true,
                completed: false,
                failed: false,
                error: String::new(),
                first: false,
                videos: Vec::new(),
                playable: false,
                single: false,
                first_video: 0,
                folder: String::new(),
            })
            .collect();
        active.extend(
            self.tasks
                .iter()
                .filter(|task| !task.finished)
                .map(|task| self.row(task)),
        );
        let mut completed: Vec<DownloadTask> = self
            .tasks
            .iter()
            .filter(|task| task.finished)
            .map(|task| self.row(task))
            .collect();
        completed.reverse();
        for rows in [&mut active, &mut completed] {
            if let Some(first) = rows.first_mut() {
                first.first = true;
            }
        }
        self.state.active_count = active.len() as i64;
        self.state.completed_count = completed.len() as i64;
        self.state.running = active.iter().any(|task| task.running) || self.state.adding.reading;
        self.state.active = active;
        self.state.completed = completed;

        let sheet = &mut self.state.adding;
        sheet.place = self.place.display().to_string();
        sheet.places = self
            .places
            .iter()
            .enumerate()
            .map(|(id, place)| SavePlace {
                id: id as i64,
                name: place.file_name().map_or_else(
                    || place.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                ),
                path: place.display().to_string(),
                chosen: *place == self.place,
            })
            .collect();
        sheet.has_places = self.places.len() > 1
            || self
                .places
                .first()
                .is_some_and(|place| *place != self.place);
        sheet.own_folder = self.own_folder;
        match &self.preview {
            Some((_, preview)) => {
                sheet.read = true;
                sheet.name = preview.name.clone();
                sheet.size = data_size(preview.total);
                sheet.file_count = preview.files.len() as i64;
                sheet.files = preview
                    .files
                    .iter()
                    .take(PREVIEW_FILES)
                    .map(|file| PreviewFile {
                        index: file.index as i64,
                        name: file.path.clone(),
                        size: data_size(file.length),
                    })
                    .collect();
                sheet.more_files = preview.files.len().saturating_sub(PREVIEW_FILES) as i64;
                sheet.folder_name = preview.folder_name.clone();
            }
            None => {
                sheet.read = false;
                sheet.name.clear();
                sheet.size = DataSize::Unknown;
                sheet.file_count = 0;
                sheet.files.clear();
                sheet.more_files = 0;
                sheet.folder_name.clear();
            }
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
        for done in done {
            match done {
                Done::Started(Ok(())) => {
                    self.started = true;
                    self.read_tasks();
                }
                Done::Started(Err(error)) => {
                    eprintln!("erika: start downloads: {error}");
                    self.state.available = false;
                }
                Done::Read(reading, from, result) if reading == self.reading => {
                    self.state.adding.reading = false;
                    match result {
                        Ok(preview) => self.preview = Some((from, preview)),
                        Err(detail) => {
                            self.state.adding.problem = Some(AddProblem::Unreadable { detail })
                        }
                    }
                }
                Done::Read(..) => {}
                Done::Added(key, result) => {
                    self.pending.retain(|pending| pending.key != key);
                    match result {
                        Ok(_) => self.notify(DownloadNotice::Added),
                        Err(detail) => self.notify(DownloadNotice::Failed { detail }),
                    }
                    self.read_tasks();
                }
            }
        }
        self.refresh();
    }

    /// Start reading a torrent for the sheet; an older reading is dropped.
    fn read(
        &mut self,
        from: String,
        work: impl FnOnce() -> erika_torrent::Result<Preview> + Send + 'static,
    ) {
        self.reading += 1;
        let reading = self.reading;
        self.preview = None;
        self.state.adding.problem = None;
        self.state.adding.reading = true;
        in_background(&self.inbox, move || Done::Read(reading, from, work()));
    }
}

impl downloads::Commands for DownloadsEngine {
    fn set_tab(&mut self, tab: DownloadTab) -> Result<(), CommandError> {
        self.state.tab = tab;
        Ok(())
    }

    fn open_add(&mut self) -> Result<(), CommandError> {
        self.state.adding.open = true;
        self.state.adding.problem = (!self.state.available).then_some(AddProblem::Unavailable);
        self.refresh();
        Ok(())
    }

    fn close_add(&mut self) -> Result<(), CommandError> {
        self.state.adding.open = false;
        self.state.adding.reading = false;
        self.state.adding.problem = None;
        self.reading += 1;
        self.preview = None;
        self.refresh();
        Ok(())
    }

    fn read_link(&mut self, link: String) -> Result<(), CommandError> {
        let link = link.trim().to_string();
        if !self.started {
            self.state.adding.problem = Some(AddProblem::Unavailable);
        } else if link.is_empty() {
            self.state.adding.problem = Some(AddProblem::Empty);
        } else if !erika_torrent::is_magnet(&link) {
            self.state.adding.problem = Some(AddProblem::NotMagnet);
        } else {
            let from = link.clone();
            self.read(from, move || erika_torrent::preview_link(&link));
        }
        self.refresh();
        Ok(())
    }

    fn read_file(&mut self, file: MediaSource) -> Result<(), CommandError> {
        match bern::files::file_path(&file.url).filter(|_| self.started) {
            Some(path) => self.read(file.url.clone(), move || erika_torrent::preview_file(&path)),
            None if !self.started => self.state.adding.problem = Some(AddProblem::Unavailable),
            None => self.state.adding.problem = Some(AddProblem::Unreadable { detail: file.url }),
        }
        self.refresh();
        Ok(())
    }

    fn choose_place(&mut self, folder: MediaSource) -> Result<(), CommandError> {
        if let Some(path) = bern::files::file_path(&folder.url) {
            self.choose(path);
        }
        self.refresh();
        Ok(())
    }

    fn use_place(&mut self, id: i64) -> Result<(), CommandError> {
        if let Some(place) = self.places.get(id as usize).cloned() {
            self.choose(place);
        }
        self.refresh();
        Ok(())
    }

    fn forget_place(&mut self, id: i64) -> Result<(), CommandError> {
        if (id as usize) < self.places.len() {
            self.places.remove(id as usize);
            self.save_places();
        }
        self.refresh();
        Ok(())
    }

    fn set_own_folder(&mut self, on: bool) -> Result<(), CommandError> {
        self.own_folder = on;
        self.db
            .set_setting(OWN_FOLDER, Some(if on { "1" } else { "0" }));
        self.refresh();
        Ok(())
    }

    fn add(&mut self, link: String) -> Result<(), CommandError> {
        let link = link.trim().to_string();
        if !self.started {
            self.state.adding.problem = Some(AddProblem::Unavailable);
            self.refresh();
            return Ok(());
        }
        let read = self
            .preview
            .take()
            .filter(|(from, _)| link.is_empty() || *from == link)
            .map(|(_, preview)| preview);
        let name = match &read {
            Some(preview) => preview.name.clone(),
            None if link.is_empty() => {
                self.state.adding.problem = Some(AddProblem::Empty);
                self.refresh();
                return Ok(());
            }
            None if !erika_torrent::is_magnet(&link) => {
                self.state.adding.problem = Some(AddProblem::NotMagnet);
                self.refresh();
                return Ok(());
            }
            None => magnet_name(&link),
        };
        let (place, own_folder) = (self.place.clone(), self.own_folder);
        self.choose(place.clone());
        let key = self.next_pending;
        self.next_pending += 1;
        self.pending.push(Pending { key, name });
        self.state.adding.open = false;
        self.state.adding.problem = None;
        self.state.tab = DownloadTab::Active;
        in_background(&self.inbox, move || {
            let preview = match read {
                Some(preview) => Ok(preview),
                None => erika_torrent::preview_link(&link),
            };
            Done::Added(
                key,
                preview.and_then(|preview| erika_torrent::add(&preview, &place, own_folder)),
            )
        });
        self.refresh();
        Ok(())
    }

    fn toggle(&mut self, id: i64) -> Result<(), CommandError> {
        let task = self.task(id).cloned().ok_or_else(unknown_task)?;
        let result = match task.state {
            TaskState::Paused | TaskState::Error => {
                erika_torrent::resume(task.id).map(|()| DownloadNotice::Resumed)
            }
            _ => erika_torrent::pause(task.id).map(|()| DownloadNotice::Paused),
        };
        self.notify(result.unwrap_or_else(|detail| DownloadNotice::Failed { detail }));
        self.read_tasks();
        self.refresh();
        Ok(())
    }

    fn play(&mut self, id: i64, video: i64) -> Result<(), CommandError> {
        let task = self.task(id).cloned().ok_or_else(unknown_task)?;
        let Some(file) = self
            .files
            .get(&task.id)
            .and_then(|files| {
                files
                    .iter()
                    .find(|file| file.index as i64 == video && file.is_video())
            })
            .cloned()
        else {
            self.notify(DownloadNotice::NothingToPlay);
            return Ok(());
        };
        let path = task.folder.join(&file.path);
        if task.finished {
            downloads::emit::play(file.name().to_string(), MediaSource::file(&path), false);
            return Ok(());
        }
        match erika_torrent::stream_url(task.id, file.index, file.name()) {
            Ok(url) => downloads::emit::play(file.name().to_string(), MediaSource::new(url), true),
            Err(detail) => self.notify(DownloadNotice::Failed { detail }),
        }
        Ok(())
    }

    fn remove(&mut self, id: i64) -> Result<(), CommandError> {
        let task = self.task(id).cloned().ok_or_else(unknown_task)?;
        self.notify(
            erika_torrent::forget(task.id)
                .map(|()| DownloadNotice::Removed)
                .unwrap_or_else(|detail| DownloadNotice::Failed { detail }),
        );
        self.read_tasks();
        self.refresh();
        Ok(())
    }

    fn ask_delete(&mut self, id: i64) -> Result<(), CommandError> {
        let task = self.task(id).cloned().ok_or_else(unknown_task)?;
        self.state.deleting = id;
        self.state.deleting_title = task.name;
        Ok(())
    }

    fn cancel_delete(&mut self) -> Result<(), CommandError> {
        self.state.deleting = 0;
        Ok(())
    }

    fn delete(&mut self) -> Result<(), CommandError> {
        if let Some(task) = self.task(self.state.deleting).cloned() {
            self.notify(
                erika_torrent::delete(task.id)
                    .map(|()| DownloadNotice::Deleted)
                    .unwrap_or_else(|detail| DownloadNotice::Failed { detail }),
            );
        }
        self.state.deleting = 0;
        self.read_tasks();
        self.refresh();
        Ok(())
    }

    fn set_to_library(&mut self, on: bool) -> Result<(), CommandError> {
        self.state.to_library = on;
        self.db
            .set_setting(TO_LIBRARY, Some(if on { "1" } else { "0" }));
        Ok(())
    }

    fn tick(&mut self) -> Result<(), CommandError> {
        self.read_tasks();
        self.refresh();
        Ok(())
    }
}

impl downloads::Engine for DownloadsEngine {
    fn state(&self) -> &State {
        &self.state
    }

    /// Work comes back; as the app starts, the session starts on another
    /// thread and resumes the tasks it kept.
    fn wake(&mut self) {
        self.take_done();
        if !self.starting && self.state.available {
            self.starting = true;
            // Every task names its folder; the session's own stays in the
            // app's data, so nothing appears in the user's folders before
            // a download does.
            let data = bern::storage::data_dir().unwrap_or_default();
            let (session, fallback) = (data.join("torrents"), data.join("downloads"));
            in_background(&self.inbox, move || {
                Done::Started(erika_torrent::start(&session, &fallback))
            });
            self.take_done();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use downloads::Commands;
    use std::path::Path;

    #[test]
    fn sizes_read_as_people_write_them() {
        assert_eq!(data_size(0), DataSize::Unknown);
        assert_eq!(data_size(16_384), DataSize::Kilobytes { kilobytes: 16.0 });
        assert_eq!(
            data_size(860 << 20),
            DataSize::Megabytes { megabytes: 860.0 }
        );
        assert_eq!(
            data_size((1.24 * (1u64 << 30) as f64) as u64),
            DataSize::Gigabytes { gigabytes: 1.2 }
        );
    }

    #[test]
    fn magnet_links_name_their_torrent() {
        assert_eq!(
            magnet_name("magnet:?xt=urn:btih:abc&dn=%5BSakura%5D+Frieren+-+01.mkv&tr=x"),
            "[Sakura] Frieren - 01.mkv"
        );
        assert_eq!(magnet_name("magnet:?xt=urn:btih:abc"), "");
    }

    /// Without a data folder (tests) downloads do not start; the sheet says
    /// so rather than read anything.
    #[test]
    fn without_a_session_the_sheet_says_downloads_are_unavailable() {
        let mut engine = DownloadsEngine::default();
        assert!(!engine.state.available);
        engine.open_add().unwrap();
        assert!(engine.state.adding.open);
        assert_eq!(engine.state.adding.problem, Some(AddProblem::Unavailable));
        engine.add("magnet:?xt=urn:btih:abc".into()).unwrap();
        assert!(engine.state.active.is_empty());
        assert_eq!(engine.toggle(1).unwrap_err().code, "unknown_task");
        engine.close_add().unwrap();
        assert!(!engine.state.adding.open);
    }

    #[test]
    fn folders_used_come_first_and_stay_a_few() {
        let mut engine = DownloadsEngine::default();
        for index in 0..8 {
            engine.choose(PathBuf::from(format!("/d/{index}")));
        }
        engine.choose(PathBuf::from("/d/3"));
        engine.refresh();
        assert_eq!(engine.places.len(), MAX_PLACES);
        assert_eq!(engine.state.adding.place, "/d/3");
        assert!(engine.state.adding.places[0].chosen && engine.state.adding.has_places);
        engine.forget_place(0).unwrap();
        assert!(engine.places.iter().all(|place| place != Path::new("/d/3")));
    }
}
