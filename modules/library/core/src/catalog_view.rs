//! Catalogue projections are shared by the wall, recent rows and source browser.
use super::*;
use erika_catalog::{Kind, MediaNode};
use erika_store::catalog::{MediaShelf, Result};

pub(super) struct Content {
    pub root: MediaNode,
    pub items: Vec<MediaNode>,
}
pub(super) fn poster(
    shelf: &MediaShelf,
    images: &erika_store::catalog_images::Images,
) -> LibraryPoster {
    let (episodes, watched) = (shelf.episodes.max(1), shelf.watched);
    LibraryPoster {
        id: shelf.node.id.0.clone(),
        title: shelf.node.title.clone(),
        poster: MediaSource::new(images.url(shelf.node.artwork.poster.as_ref(), 780)),
        initial: initial(&shelf.node.title),
        episodes,
        watched,
        rating: 0.,
        progress: watched as f64 / episodes as f64,
        watching: watched > 0 && watched < episodes,
        movie: shelf.node.kind == Kind::Movie,
    }
}
impl LibraryEngine {
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
