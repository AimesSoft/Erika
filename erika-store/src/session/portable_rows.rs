//! Provider records and file progress exchanged by account sync.
//! TMDB identities travel separately in the catalogue snapshot.
use super::Db;
use crate::portable::*;

impl Db {
    /// Publish the entire database draft only after all restore operations succeed.
    /// The callback must use the supplied draft, not re-enter the original Db.
    pub fn transaction<T>(
        &self,
        work: impl FnOnce(&Self) -> crate::catalog::Result<T>,
    ) -> crate::catalog::Result<T> {
        let mut state = self.state.lock().expect("session database lock poisoned");
        let draft = Self { state: std::sync::Arc::new(std::sync::Mutex::new(state.clone())) };
        let result = work(&draft)?;
        *state = draft.state.lock().expect("session database lock poisoned").clone();
        Ok(result)
    }
    pub fn settings(&self) -> Vec<(String, String)> {
        self.read(|s| {
            s.settings
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
    }
    pub fn portable_sources(&self) -> Vec<Source> {
        self.read(|s| s.sources.values().cloned().collect())
    }
    pub fn put_source(&self, source: &Source) -> crate::catalog::Result<(i64, bool)> {
        let descriptor = crate::plugin_sources::Descriptor::from_source(source)?;
        self.edit(|s| {
            if let Some((owner, descriptor)) = descriptor {
                return s.register_source(&owner, &descriptor, source.added);
            }
            if let Some(id) = s.sources.iter()
                .find(|(_, v)| v.kind == source.kind && v.location == source.location)
                .map(|(id, _)| *id)
            {
                let previous = s.sources.get_mut(&id).unwrap();
                previous.name = source.name.clone();
                previous.content = source.content.clone();
                return Ok((id, false));
            }
            s.next_source = s.next_source.max(1) + 1;
            s.sources.insert(s.next_source, source.clone());
            Ok((s.next_source, true))
        })
    }
    pub fn remove_source_at(&self, kind: &str, location: &str) {
        let id = self.read(|s| {
            s.sources
                .iter()
                .find(|(_, v)| v.kind == kind && v.location == location)
                .map(|(id, _)| *id)
        });
        if let Some(id) = id {
            self.remove_source(id);
        }
    }
    pub fn portable_anime(&self) -> Vec<Anime> {
        self.read(|s| s.anime.values().cloned().collect())
    }
    pub fn put_anime(&self, anime: &Anime) {
        self.write(|s| {
            let mut row = anime.clone();
            if let Some(old) = s.anime.get(&row.dandanplay_id) {
                if row.image.is_empty() {
                    row.image = old.image.clone();
                }
                if row.bangumi_id.is_none() {
                    row.bangumi_id = old.bangumi_id;
                }
            }
            if s.anime.values().any(|a| {
                a.dandanplay_id != row.dandanplay_id
                    && a.bangumi_id.is_some()
                    && a.bangumi_id == row.bangumi_id
            }) {
                row.bangumi_id = s.anime.get(&row.dandanplay_id).and_then(|a| a.bangumi_id);
            }
            s.anime.insert(row.dandanplay_id, row);
        });
    }
    pub fn portable_episodes(&self) -> Vec<Episode> {
        self.read(|s| s.episodes.values().cloned().collect())
    }
    pub fn put_episode(&self, episode: &Episode) {
        self.write(|s| {
            if s.anime.contains_key(&episode.anime) {
                s.episodes.insert(episode.dandanplay_id, episode.clone());
            }
        });
    }
    pub fn portable_links(&self) -> Vec<Link> {
        self.read(|s| {
            s.links
                .iter()
                .filter_map(|(&id, (episode, matched_by))| {
                    let file = s.files.get(&id)?.clone()?;
                    if s.catalog.bindings.contains_key(&id) { return None; }
                    Some(Link {
                        file,
                        episode: *episode,
                        matched_by: matched_by.clone(),
                    })
                })
                .collect()
        })
    }
    pub fn put_link(&self, link: &Link) {
        self.write(|s| {
            let file = s.file(Some(&link.file));
            if s.catalog.bindings.contains_key(&file) || s.links.get(&file).is_some_and(|(_, kind)| matches!(kind.as_deref(), Some("manual" | "none"))) { return; }
            if link.episode.is_some_and(|id| !s.episodes.contains_key(&id)) { return; }
            s.links.insert(file, (link.episode, link.matched_by.clone()));
        });
    }
    pub fn remove_link(&self, key: &FileKey) {
        self.write(|s| {
            let file = s.file(Some(key));
            if !s.catalog.bindings.contains_key(&file) && !s.links.get(&file).is_some_and(|(_, kind)| matches!(kind.as_deref(), Some("manual" | "none"))) { s.links.remove(&file); }
        });
    }
    pub fn portable_progress(&self) -> Vec<Progress> {
        self.read(|s| {
            s.progress
                .iter()
                .filter_map(|(id, p)| {
                    Some(Progress {
                        file: s.files.get(id)?.clone()?,
                        position: p.0,
                        duration: p.1,
                        danmaku_offset: p.2,
                        at: p.3,
                    })
                })
                .collect()
        })
    }
    pub fn put_progress(&self, row: &Progress) {
        if !row.position.is_finite()
            || !row.duration.is_finite()
            || !row.danmaku_offset.is_finite()
            || row.position < 0.
            || row.duration < 0.
        {
            return;
        }
        self.write(|s| {
            let f = s.file(Some(&row.file));
            s.progress
                .insert(f, (row.position, row.duration, row.danmaku_offset, row.at));
        });
    }
    pub fn remove_progress(&self, key: &FileKey) {
        self.write(|s| {
            if let Some(id) = s
                .files
                .iter()
                .find(|(_, v)| v.as_ref() == Some(key))
                .map(|(id, _)| *id)
            {
                s.progress.remove(&id);
            }
        });
    }
    pub fn portable_watched(&self) -> Vec<Watched> {
        self.read(|s| s.watched.values().cloned().collect())
    }
    pub fn put_watched(&self, row: &Watched) {
        self.write(|s| {
            if s.watched
                .get(&row.key())
                .is_none_or(|old| !old.manual || row.manual)
            {
                s.watched.insert(row.key(), row.clone());
            }
        });
    }
    pub fn remove_watched(&self, row: &Watched) {
        self.write(|s| {
            s.watched.remove(&row.key());
        });
    }
    pub fn portable_recents(&self) -> Vec<Recent> {
        self.read(|s| {
            s.recents
                .values()
                .map(|(r, at)| Recent {
                    url: r.url.clone(),
                    title: r.title.clone(),
                    position: r.position,
                    duration: r.duration,
                    at: *at,
                    file: s
                        .addresses
                        .iter()
                        .find(|a| a.url == r.url && a.source == 1)
                        .and_then(|a| a.file)
                        .and_then(|f| s.files.get(&f).cloned().flatten()),
                })
                .collect()
        })
    }
    pub fn put_recent(&self, row: &Recent) {
        self.write(|s| {
            s.address(1, &row.url, 0, 0, None);
            if let Some(key) = &row.file {
                let file = s.file(Some(key));
                if let Some(a) = s
                    .addresses
                    .iter_mut()
                    .find(|a| a.source == 1 && a.url == row.url)
                {
                    a.file = Some(file);
                }
            }
            let still = s.recents.get(&row.url).and_then(|r| r.0.still.clone());
            s.recents.insert(
                row.url.clone(),
                (
                    crate::Recent {
                        url: row.url.clone(),
                        title: row.title.clone(),
                        still,
                        position: row.position,
                        duration: row.duration,
                    },
                    row.at,
                ),
            );
        });
    }
    pub fn remove_recent(&self, url: &str) {
        self.write(|s| {
            s.recents.remove(url);
        });
    }
    pub fn anime_marks(&self) -> Vec<Mark> {
        self.read(|s| s.marks.values().cloned().collect())
    }
    pub fn set_anime_mark(&self, mark: &Mark) {
        self.write(|s| {
            if s.anime.contains_key(&mark.anime) {
                s.marks.insert(mark.anime, mark.clone());
            }
        });
    }
    pub fn remove_anime_mark(&self, anime: i64) {
        self.write(|s| {
            s.marks.remove(&anime);
        });
    }
    pub fn extensions(&self) -> Vec<(String, String)> {
        self.read(|s| {
            s.extensions
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
    }
    pub fn extension(&self, id: &str) -> Option<String> {
        self.read(|s| s.extensions.get(id).cloned())
    }
    pub fn set_extension(&self, id: &str, json: Option<&str>) {
        self.write(|s| {
            if let Some(v) = json {
                s.extensions.insert(id.into(), v.into());
            } else {
                s.extensions.remove(id);
            }
        });
    }
}
