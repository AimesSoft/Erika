//! Browser-local session storage. Engines share the same session, not empty
//! placeholders; closing/reloading the page still discards this in-memory DB.

use crate::{
    Anime, Entry, Episode, EpisodeFile, FileId, Fingerprint, Mark, Recent, Seen, Shelf, Source,
    Watching, portable,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};

mod catalog;
mod portable_rows;
mod plugin_sources;

#[derive(Clone)]
pub struct Db {
    state: Arc<Mutex<State>>,
}

#[derive(Clone)]
struct Address {
    key: i64,
    source: i64,
    url: String,
    file: Option<i64>,
    size: i64,
    modified: i64,
    seen: i64,
}
#[derive(Clone, Default)]
struct State {
    next_file: i64,
    next_source: i64,
    next_address: i64,
    files: BTreeMap<FileId, Option<portable::FileKey>>,
    addresses: Vec<Address>,
    progress: BTreeMap<FileId, (f64, f64, f64, i64)>,
    recents: BTreeMap<String, (Recent, i64)>,
    settings: BTreeMap<String, String>,
    extensions: BTreeMap<String, String>,
    sources: BTreeMap<i64, portable::Source>,
    anime: BTreeMap<i64, portable::Anime>,
    episodes: BTreeMap<i64, portable::Episode>,
    links: BTreeMap<i64, (Option<i64>, Option<String>)>,
    watched: BTreeMap<String, portable::Watched>,
    marks: BTreeMap<i64, portable::Mark>,
    catalog: catalog::Catalog,
    plugin_sources: BTreeMap<i64, plugin_sources::Directory>,
}
impl State {
    fn link_provider_episode(&mut self, file: FileId, anime: &Anime, episode: &Episode, manual: bool) -> crate::catalog::Result<()> {
        if !self.files.contains_key(&file) { return Err(crate::catalog::Error::invalid("文件不存在")); }
        if self.catalog.bindings.contains_key(&file) { return Ok(()); }
        let old = self.anime.get(&anime.dandanplay_id);
        self.anime.insert(anime.dandanplay_id, portable::Anime {
            dandanplay_id: anime.dandanplay_id,
            title: anime.title.clone(),
            image: if anime.image.is_empty() { old.map(|a| a.image.clone()).unwrap_or_default() } else { anime.image.clone() },
            bangumi_id: old.and_then(|a| a.bangumi_id),
        });
        self.episodes.insert(episode.dandanplay_id, portable::Episode {
            dandanplay_id: episode.dandanplay_id, anime: anime.dandanplay_id,
            number: Some(episode.number), title: episode.title.clone(), bangumi_id: None,
        });
        if manual || self.links.get(&file).is_none_or(|(_, kind)| kind.as_deref().is_none_or(|k| k == "auto")) {
            self.links.insert(file, (Some(episode.dandanplay_id), Some(if manual { "manual" } else { "auto" }.into())));
        }
        Ok(())
    }

    fn file(&mut self, key: Option<&portable::FileKey>) -> FileId {
        if let Some(key) = key {
            if let Some((&id, _)) = self.files.iter().find(|(_, k)| k.as_ref() == Some(key)) {
                return id;
            }
        }
        self.next_file += 1;
        self.files.insert(self.next_file, key.cloned());
        self.next_file
    }
    fn address(
        &mut self,
        source: i64,
        url: &str,
        size: i64,
        modified: i64,
        fingerprint: Option<&Fingerprint>,
    ) -> FileId {
        let index = self
            .addresses
            .iter()
            .position(|a| a.source == source && a.url == url);
        let key = fingerprint.map(portable::FileKey::of);
        let known = index.and_then(|i| self.addresses[i].file).or_else(|| {
            self.addresses
                .iter()
                .find(|a| a.url == url)
                .and_then(|a| a.file)
        });
        let file = if key.is_some() {
            self.file(key.as_ref())
        } else {
            known.unwrap_or_else(|| self.file(None))
        };
        let at = crate::now();
        if let Some(i) = index {
            let a = &mut self.addresses[i];
            a.file = Some(file);
            a.size = size;
            a.modified = modified;
            a.seen = at;
        } else {
            self.next_address += 1;
            self.addresses.push(Address {
                key: self.next_address,
                source,
                url: url.into(),
                file: Some(file),
                size,
                modified,
                seen: at,
            });
        }
        file
    }
    fn fingerprint(&self, file: i64) -> Option<Fingerprint> {
        match self.files.get(&file)?.as_ref()? {
            portable::FileKey::Hash { md5, size } => Some(Fingerprint {
                md5: md5.clone(),
                size: *size,
            }),
            _ => None,
        }
    }
    fn file_episode(&self, file: i64) -> Option<(Anime, Episode)> {
        let id = self.links.get(&file)?.0?;
        let ep = self.episodes.get(&id)?;
        let a = self.anime.get(&ep.anime)?;
        Some((
            Anime {
                dandanplay_id: a.dandanplay_id,
                title: a.title.clone(),
                image: a.image.clone(),
            },
            Episode {
                dandanplay_id: ep.dandanplay_id,
                number: ep.number?,
                title: ep.title.clone(),
            },
        ))
    }
    fn entry(&self, a: &Address) -> Entry {
        Entry {
            key: a.key,
            source: a.source,
            url: a.url.clone(),
            file: a.file,
            matched: a.file.and_then(|id| self.file_episode(id)),
            manual: a
                .file
                .and_then(|id| self.catalog.bindings.get(&id))
                .is_some_and(|b| b.decision != erika_catalog::Decision::Auto),
        }
    }
}
impl Db {
    pub fn open() -> Db {
        static SHARED: OnceLock<Arc<Mutex<State>>> = OnceLock::new();
        Db {
            state: SHARED
                .get_or_init(|| Arc::new(Mutex::new(State::default())))
                .clone(),
        }
    }
    pub fn in_memory() -> Db {
        Db {
            state: Arc::new(Mutex::new(State::default())),
        }
    }
    fn read<T>(&self, f: impl FnOnce(&State) -> T) -> T {
        f(&self.state.lock().expect("session database lock poisoned"))
    }
    fn write<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        f(&mut self.state.lock().expect("session database lock poisoned"))
    }
    fn edit<T>(
        &self,
        f: impl FnOnce(&mut State) -> crate::catalog::Result<T>,
    ) -> crate::catalog::Result<T> {
        let mut state = self.state.lock().expect("session database lock poisoned");
        let mut draft = state.clone();
        let result = f(&mut draft)?;
        *state = draft;
        Ok(result)
    }
    pub fn opened(&self, url: &str, fingerprint: Option<&Fingerprint>) -> Option<FileId> {
        Some(self.write(|s| {
            s.address(
                1,
                url,
                fingerprint.map_or(0, |f| f.size as i64),
                0,
                fingerprint,
            )
        }))
    }
    pub fn progress(&self, file: FileId) -> Option<(f64, f64)> {
        self.read(|s| s.progress.get(&file).map(|p| (p.0, p.1)))
    }
    pub fn save_progress(&self, file: FileId, position: f64, duration: f64) {
        if !position.is_finite() || !duration.is_finite() || position < 0. || duration < 0. {
            return;
        }
        self.write(|s| {
            if s.files.contains_key(&file) {
                let offset = s.progress.get(&file).map_or(0., |p| p.2);
                s.progress
                    .insert(file, (position, duration, offset, crate::now()));
            }
        });
    }
    pub fn remember(
        &self,
        url: &str,
        title: &str,
        still: Option<&str>,
        position: f64,
        duration: f64,
    ) {
        self.write(|s| {
            let picture = still
                .map(str::to_owned)
                .or_else(|| s.recents.get(url).and_then(|r| r.0.still.clone()));
            s.address(1, url, 0, 0, None);
            s.recents.insert(
                url.into(),
                (
                    Recent {
                        url: url.into(),
                        title: title.into(),
                        still: picture,
                        position,
                        duration,
                    },
                    crate::now(),
                ),
            );
        });
    }
    pub fn recents(&self, limit: usize) -> Vec<Recent> {
        self.read(|s| {
            let mut rows: Vec<_> = s.recents.values().collect();
            rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.url.cmp(&b.0.url)));
            rows.into_iter().take(limit).map(|r| r.0.clone()).collect()
        })
    }
    pub fn stills(&self) -> Vec<String> {
        self.read(|s| {
            s.recents
                .values()
                .filter_map(|r| r.0.still.clone())
                .collect()
        })
    }
    pub fn setting(&self, key: &str) -> Option<String> {
        self.read(|s| s.settings.get(key).cloned())
    }
    /// Read a setting without treating a storage failure as a missing value.
    pub fn try_setting(&self, key: &str) -> crate::catalog::Result<Option<String>> {
        Ok(self.setting(key))
    }

    /// Atomically transform one setting shared by the session's engines.
    /// The callback must not re-enter the database while the lock is held.
    pub fn update_setting<T>(
        &self,
        key: &str,
        update: impl FnOnce(Option<&str>) -> crate::catalog::Result<(Option<String>, T)>,
    ) -> crate::catalog::Result<T> {
        self.write(|s| {
            // Do not mutate until the callback succeeds; no full-state copy needed.
            let (value, result) = update(s.settings.get(key).map(String::as_str))?;
            if let Some(value) = value {
                s.settings.insert(key.into(), value);
            } else {
                s.settings.remove(key);
            }
            Ok(result)
        })
    }

    pub fn set_setting(&self, key: &str, value: Option<&str>) {
        self.write(|s| {
            if let Some(v) = value {
                s.settings.insert(key.into(), v.into());
            } else {
                s.settings.remove(key);
            }
        });
    }
    pub fn remember_anime(&self, anime: &Anime) {
        self.write(|s| {
            let old = s.anime.get(&anime.dandanplay_id);
            let a = portable::Anime {
                dandanplay_id: anime.dandanplay_id,
                title: anime.title.clone(),
                bangumi_id: old.and_then(|a| a.bangumi_id),
                image: if anime.image.is_empty() {
                    old.map(|a| a.image.clone()).unwrap_or_default()
                } else {
                    anime.image.clone()
                },
            };
            s.anime.insert(a.dandanplay_id, a);
        });
    }
    pub fn anime(&self, id: i64) -> Option<Anime> {
        self.read(|s| {
            s.anime.get(&id).map(|a| Anime {
                dandanplay_id: id,
                title: a.title.clone(),
                image: a.image.clone(),
            })
        })
    }
    pub fn link_episode(&self, file: FileId, anime: &Anime, episode: &Episode, manual: bool) {
        let _ = self.edit(|s| s.link_provider_episode(file, anime, episode, manual));
    }
    pub fn file_episode(&self, file: FileId) -> Option<(Anime, Episode)> {
        self.read(|s| s.file_episode(file))
    }
    pub fn episode_files(&self, id: i64) -> Vec<EpisodeFile> {
        self.read(|s| {
            let mut addresses: Vec<_> = s.addresses.iter().collect();
            addresses.sort_by_key(|a| std::cmp::Reverse(a.seen));
            let mut seen = std::collections::HashSet::new();
            let mut files: Vec<_> = addresses
                .into_iter()
                .filter_map(|a| {
                    let file = a.file?;
                    let (anime, episode) = s.file_episode(file)?;
                    if anime.dandanplay_id != id || !seen.insert(episode.dandanplay_id) {
                        return None;
                    }
                    let (position, duration, _, _) =
                        s.progress.get(&file).copied().unwrap_or_default();
                    Some(EpisodeFile {
                        episode,
                        url: a.url.clone(),
                        position,
                        duration,
                    })
                })
                .collect();
            files.sort_by_key(|f| f.episode.number);
            files
        })
    }
    pub fn watching(&self, limit: usize) -> Vec<Watching> {
        self.read(|s| {
            let mut rows: Vec<_> = s.recents.values().collect();
            rows.sort_by_key(|r| std::cmp::Reverse(r.1));
            let mut seen = std::collections::HashSet::new();
            rows.into_iter()
                .filter_map(|(r, _)| {
                    let file = s.addresses.iter().find(|a| a.url == r.url)?.file?;
                    let (anime, episode) = s.file_episode(file)?;
                    if !seen.insert(anime.dandanplay_id) {
                        return None;
                    }
                    let (position, duration, _, _) = s
                        .progress
                        .get(&file)
                        .copied()
                        .unwrap_or((r.position, r.duration, 0., 0));
                    Some(Watching {
                        anime,
                        episode,
                        url: r.url.clone(),
                        still: r.still.clone(),
                        position,
                        duration,
                    })
                })
                .take(limit)
                .collect()
        })
    }
    pub fn shelf(&self, source: Option<i64>) -> Vec<Shelf> {
        self.read(|s| {
            let mut groups: BTreeMap<i64, (Anime, BTreeMap<i64, bool>, i64, i64)> = BTreeMap::new();
            for a in &s.addresses {
                if source.is_some_and(|id| id != a.source) {
                    continue;
                }
                let Some(file) = a.file else {
                    continue;
                };
                let Some((anime, episode)) = s.file_episode(file) else {
                    continue;
                };
                let watched = s
                    .watched
                    .values()
                    .any(|w| w.episode == Some(episode.dandanplay_id))
                    || s.progress
                        .get(&file)
                        .is_some_and(|p| p.1 > 0. && p.0 >= p.1 * 0.9);
                let entry = groups.entry(anime.dandanplay_id).or_insert((
                    anime,
                    BTreeMap::new(),
                    a.seen,
                    a.seen,
                ));
                *entry.1.entry(episode.dandanplay_id).or_default() |= watched;
                entry.2 = entry.2.min(a.seen);
                entry.3 = entry.3.max(a.seen);
            }
            let mut rows: Vec<_> = groups
                .into_values()
                .map(|(anime, episodes, added, seen)| Shelf {
                    anime,
                    episodes: episodes.len() as i64,
                    watched: episodes.values().filter(|v| **v).count() as i64,
                    added,
                    seen,
                })
                .collect();
            rows.sort_by_key(|r| std::cmp::Reverse(r.seen));
            rows
        })
    }
    pub fn file_id(&self, fp: &Fingerprint) -> Option<FileId> {
        let key = portable::FileKey::of(fp);
        self.read(|s| {
            s.files
                .iter()
                .find(|(_, v)| v.as_ref() == Some(&key))
                .map(|(id, _)| *id)
        })
    }
    pub fn fingerprint_of(&self, file: FileId) -> Option<Fingerprint> {
        self.read(|s| s.fingerprint(file))
    }
    pub fn add_source(&self, kind: &str, name: &str, location: &str, content: &str) -> Option<i64> {
        self.put_source(&portable::Source {
            kind: kind.into(),
            name: name.into(),
            location: location.into(),
            content: content.into(),
            added: crate::now(),
        })
        .ok()
        .map(|s| s.0)
    }
    pub fn set_source_content(&self, id: i64, content: &str) {
        self.write(|s| {
            if let Some(v) = s.sources.get_mut(&id) {
                v.content = content.into();
            }
        });
    }
    pub fn set_source(&self, id: i64, name: &str, location: &str) {
        self.write(|s| {
            if let Some(v) = s.sources.get_mut(&id) {
                v.name = name.into();
                v.location = location.into();
            }
        });
    }
    pub fn remove_source(&self, id: i64) {
        if id == 1 {
            return;
        }
        self.write(|s| {
            s.sources.remove(&id);
            s.plugin_sources.remove(&id);
            s.addresses.retain(|a| a.source != id);
        });
    }
    pub fn sources(&self) -> Vec<Source> {
        self.read(|s| {
            s.sources
                .iter()
                .map(|(&id, v)| {
                    let files: Vec<_> = s.addresses.iter().filter(|a| a.source == id).collect();
                    let unmatched = files
                        .iter()
                        .filter(|a| {
                            a.file
                                .and_then(|f| s.catalog.bindings.get(&f))
                                .is_none_or(|b| b.items.is_empty())
                        })
                        .count() as i64;
                    Source {
                        id,
                        kind: v.kind.clone(),
                        name: v.name.clone(),
                        location: v.location.clone(),
                        content: v.content.clone(),
                        files: files.len() as i64,
                        unmatched,
                    }
                })
                .collect()
        })
    }
    pub fn seen(&self, source: i64) -> Vec<Seen> {
        self.read(|s| {
            s.addresses
                .iter()
                .filter(|a| a.source == source)
                .map(|a| {
                    let binding = a.file.and_then(|f| s.catalog.bindings.get(&f));
                    Seen {
                        url: a.url.clone(),
                        size: a.size,
                        modified: a.modified,
                        fingerprint: a.file.and_then(|f| s.fingerprint(f)),
                        matched: binding.is_some_and(|b| !b.items.is_empty()),
                        decided: binding
                            .is_some_and(|b| b.decision != erika_catalog::Decision::Auto),
                    }
                })
                .collect()
        })
    }
    pub fn saw(
        &self,
        source: i64,
        url: &str,
        size: i64,
        modified: i64,
        fp: Option<&Fingerprint>,
    ) -> Option<FileId> {
        self.write(|s| {
            if source == 1 || s.sources.contains_key(&source) {
                Some(s.address(source, url, size, modified, fp))
            } else {
                None
            }
        })
    }
    pub fn forget(&self, source: i64, urls: &[String]) {
        self.write(|s| {
            s.addresses
                .retain(|a| a.source != source || !urls.contains(&a.url))
        });
    }
    pub fn entries(&self, source: Option<i64>) -> Vec<Entry> {
        self.read(|s| {
            s.addresses
                .iter()
                .filter(|a| source.is_none_or(|id| id == a.source))
                .map(|a| s.entry(a))
                .collect()
        })
    }
    pub fn link_files(&self, anime: &Anime, links: &[(FileId, Episode)]) -> bool {
        self.edit(|s| {
            for (f, e) in links {
                s.link_provider_episode(*f, anime, e, true)?;
            }
            Ok(())
        })
        .is_ok()
    }
    pub fn unlink(&self, file: FileId) {
        self.write(|s| {
            if s.files.contains_key(&file) { s.links.insert(file, (None, Some("none".into()))); }
        });
    }
    pub fn mark(&self, id: i64) -> Mark {
        self.read(|s| {
            s.marks
                .get(&id)
                .map(|m| Mark {
                    status: m.status.clone(),
                    rating: m.rating,
                })
                .unwrap_or_default()
        })
    }
    pub fn set_mark(&self, anime: &Anime, mark: &Mark) {
        self.remember_anime(anime);
        self.set_anime_mark(&portable::Mark {
            anime: anime.dandanplay_id,
            status: mark.status.clone(),
            rating: mark.rating,
            at: crate::now(),
        });
    }
    pub fn watched_numbers(&self, anime: i64) -> Vec<i64> {
        self.read(|s| {
            let mut numbers: Vec<_> = s
                .watched
                .values()
                .filter(|w| w.anime == anime)
                .filter_map(|w| w.number)
                .collect();
            numbers.sort();
            numbers.dedup();
            numbers
        })
    }
    pub fn set_watched(&self, anime: &Anime, episode: &Episode, watched: bool, manual: bool) {
        self.remember_anime(anime);
        let row = portable::Watched {
            anime: anime.dandanplay_id,
            episode: (episode.dandanplay_id != 0).then_some(episode.dandanplay_id),
            number: Some(episode.number),
            at: crate::now(),
            manual,
        };
        if watched {
            self.put_watched(&row);
        } else {
            self.remove_watched(&row);
        }
    }
}

#[cfg(test)]
mod tests;
