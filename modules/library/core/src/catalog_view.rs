//! Catalogue projections are shared by the wall, recent rows and source browser.
use super::*;
use erika_catalog::{Kind, MediaNode};
use erika_store::catalog::{MediaShelf, MediaShelves, Result};

pub(super) struct Content {
    pub root: MediaNode,
    pub items: Vec<MediaNode>,
}

pub(super) fn merge_seasons(db: &erika_store::Db) -> bool {
    db.setting("merge_seasons").is_some_and(|v| v == "true")
}

fn season_bangumi_id(db: &erika_store::Db, media: &erika_catalog::MediaId) -> Option<i64> {
    let links = db.catalog_links(media).ok()?;
    links
        .iter()
        .filter(|l| {
            l.reference.provider == erika_catalog::Provider::Bangumi
                && l.reference.kind == Kind::Subject
                && l.reference.scope.is_empty()
                && (l.confirmed || !l.manual)
        })
        .max_by_key(|l| (l.confirmed, l.manual))
        .and_then(|l| l.reference.id.parse::<i64>().ok())
        .filter(|id| *id > 0)
}

pub(super) fn poster(
    db: &erika_store::Db,
    shelf: &MediaShelf,
    images: &erika_store::catalog_images::Images,
) -> LibraryPoster {
    let (episodes, watched) = (shelf.episodes.max(1), shelf.watched);
    let (title, poster_url) = if shelf.node.kind == Kind::Season {
        let root = db.catalog_node(&shelf.node.root).ok().flatten();
        let bgm = season_bangumi_id(db, &shelf.node.id);
        let base_title = root
            .as_ref()
            .map(|r| r.title.as_str())
            .unwrap_or(shelf.node.title.as_str());
        let title = bgm
            .and_then(|id| db.setting(&format!("bangumi.title_cn.{id}")))
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| match shelf.node.season_number {
                Some(0) => format!("{base_title} 特别篇"),
                Some(1) | None => base_title.to_string(),
                Some(n) => format!("{base_title} 第 {n} 季"),
            });
        let season_tmdb = images.url(shelf.node.artwork.poster.as_ref(), 780);
        let poster_url = if !season_tmdb.is_empty() {
            season_tmdb
        } else if let Some(cover) = bgm
            .and_then(|id| db.setting(&format!("bangumi.cover.{id}")))
            .filter(|u| u.starts_with("https://"))
        {
            cover
        } else {
            images.url(root.as_ref().and_then(|r| r.artwork.poster.as_ref()), 780)
        };
        (title, poster_url)
    } else {
        (
            shelf.node.title.clone(),
            images.url(shelf.node.artwork.poster.as_ref(), 780),
        )
    };
    LibraryPoster {
        id: shelf.node.id.0.clone(),
        initial: initial(&title),
        title,
        poster: MediaSource::new(poster_url),
        episodes,
        watched,
        rating: 0.,
        progress: watched as f64 / episodes as f64,
        watching: watched > 0 && watched < episodes,
        movie: shelf.node.kind == Kind::Movie,
    }
}
/// Only the parts of a scoped wall that the global posters don't describe.
/// This is a comparison key, not a second set of UI query results.
#[derive(Default, PartialEq)]
pub(super) struct SourceWalls {
    names: BTreeMap<i64, String>,
    /// The order matters for Recent and as the tie-breaker for Name/Rating.
    rows: BTreeMap<i64, Vec<(erika_catalog::MediaId, i64, i64)>>,
    /// A source with only specials may show the work root, even when the
    /// global wall shows only seasons. Its title and artwork matter too.
    extra: HashMap<erika_catalog::MediaId, LibraryPoster>,
}

impl LibraryEngine {
    pub(super) fn update_walls(&mut self, sources: &[Source], shelves: Result<MediaShelves>) {
        let mut shelves = match shelves {
            Ok(shelves) => shelves,
            Err(error) => {
                // One unreadable source can break the batch while other scopes
                // still answer. Without a comparison key, recheck live queries
                // on each possible write, not just on the first read failure.
                self.browse_sources = None;
                self.state.browse_revision += 1;
                self.notify(LibraryNotice::Failed {
                    reason: error.message,
                });
                return;
            }
        };
        let whole = shelves.remove(&0).unwrap_or_default();
        let images = self.db.catalog_images();
        let posters: Vec<_> = whole.iter().map(|s| poster(&self.db, s, &images)).collect();
        let whole_ids: HashSet<_> = whole.iter().map(|s| &s.node.id).collect();
        let mut inputs = SourceWalls {
            names: sources.iter().map(|s| (s.id, s.name.clone())).collect(),
            ..Default::default()
        };
        for (source, shelf) in shelves {
            let rows = shelf
                .into_iter()
                .map(|s| {
                    if !whole_ids.contains(&s.node.id) {
                        inputs
                            .extra
                            .entry(s.node.id.clone())
                            .or_insert_with(|| poster(&self.db, &s, &images));
                    }
                    (s.node.id, s.episodes, s.watched)
                })
                .collect();
            inputs.rows.insert(source, rows);
        }
        if posters != self.state.posters || self.browse_sources.as_ref() != Some(&inputs) {
            self.state.browse_revision += 1;
        }
        self.browse_sources = Some(inputs);
        let mut added: Vec<_> = whole.iter().zip(&posters).collect();
        added.sort_by_key(|(s, _)| std::cmp::Reverse(s.added));
        self.state.recent = added
            .into_iter()
            .take(RECENT)
            .map(|(_, p)| p.clone())
            .collect();
        self.state.has_recent = !self.state.recent.is_empty();
        self.state.posters = posters;
    }

    pub(super) fn load_contents(&mut self, entries: &[Entry]) -> Result<()> {
        let mut contents = HashMap::new();
        let mut seen = HashSet::new();
        for file in entries
            .iter()
            .filter_map(|e| e.file)
            .filter(|f| seen.insert(*f))
        {
            let Some(binding) = self.db.catalog_binding(file)? else {
                continue;
            };
            if binding.items.is_empty() {
                continue;
            }
            let items = binding
                .items
                .iter()
                .map(|i| {
                    self.db
                        .catalog_node(&i.media)?
                        .ok_or_else(|| erika_store::catalog::Error::invalid("文件绑定的媒体不存在"))
                })
                .collect::<Result<Vec<_>>>()?;
            let root = self
                .db
                .catalog_node(&items[0].root)?
                .ok_or_else(|| erika_store::catalog::Error::invalid("文件绑定的作品不存在"))?;
            contents.insert(file, Content { root, items });
        }
        self.contents = contents;
        Ok(())
    }
    pub(super) fn content(&self, entry: &Entry) -> Option<&Content> {
        entry.file.and_then(|f| self.contents.get(&f))
    }
    pub(super) fn file_row(&self, entry: &Entry, name: &str, place: String) -> LibraryFile {
        let content = self.content(entry);
        let note = if let Some(content) = content {
            let labels: Vec<_> = content
                .items
                .iter()
                .filter_map(|item| match item.kind {
                    Kind::Episode => item
                        .season_number
                        .zip(item.episode_number)
                        .map(|(s, e)| format!("S{s:02}E{e:02}")),
                    _ => None,
                })
                .collect();
            let mut title = content.root.title.clone();
            if !labels.is_empty() {
                title.push_str(" · ");
                title.push_str(&labels.join(" / "));
            }
            FileNote::Content { title }
        } else if entry.file.is_some() {
            FileNote::Unmatched
        } else {
            FileNote::Unreadable
        };
        LibraryFile {
            key: entry.key,
            show: content.map(|c| c.root.id.0.clone()).unwrap_or_default(),
            name: name.into(),
            note,
            matched: content.is_some(),
            readable: entry.file.is_some(),
            place,
        }
    }
}
