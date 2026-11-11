//! Page projections use catalogue identities; service IDs are only source evidence.
use bern::{CommandError, date::CalendarDate, media::MediaSource};
use erika_anime_protocol::anime::{self, EpisodeNote, EpisodeTitle, Show, WatchStatus, Watching};
use erika_catalog::{Kind, MediaId, MediaNode};
use erika_store::{
    Db,
    catalog::{MediaFile, MediaWatching},
};

pub(super) fn command(error: erika_store::catalog::Error) -> CommandError {
    CommandError::new(error.code, error.message)
}

pub(super) fn items(db: &Db, root: &MediaNode) -> erika_store::catalog::Result<Vec<MediaNode>> {
    if root.kind.playable() {
        return Ok(vec![root.clone()]);
    }
    let mut items = Vec::new();
    for child in db.catalog_children(&root.id)? {
        if child.kind == Kind::Season {
            items.extend(db.catalog_children(&child.id)?);
        } else if child.kind.playable() {
            items.push(child);
        }
    }
    items.sort_by(|a, b| {
        (a.season_number, a.episode_number, &a.id).cmp(&(b.season_number, b.episode_number, &b.id))
    });
    Ok(items)
}

pub(super) fn show(db: &Db, node: &MediaNode) -> erika_store::catalog::Result<Show> {
    let images = db.catalog_images();
    let items = items(db, node)?;
    let mut watched = 0;
    for item in &items {
        if db.catalog_watched(&item.id)?.is_some_and(|v| v.watched) {
            watched += 1;
        }
    }
    let marks = db.catalog_marks(&node.id)?;
    let mark = marks
        .iter()
        .filter(|m| m.reference.is_none())
        .max_by_key(|m| m.updated_at);
    let status = mark
        .and_then(|m| m.status.as_deref())
        .and_then(crate::marks::Level::parse)
        .map(|v| crate::marks::Level::status(Some(v)))
        .unwrap_or_else(|| {
            if watched == 0 {
                WatchStatus::Unmarked
            } else if watched == items.len() as i64 {
                WatchStatus::Watched
            } else {
                WatchStatus::Watching
            }
        });
    let date = crate::clock::parse_date(&node.date);
    let premiere = date.unwrap_or_else(|| CalendarDate::new(1970, 1, 1));
    Ok(Show {
        id: node.id.0.clone(),
        title: node.title.clone(),
        poster: MediaSource::new(images.url(node.artwork.poster.as_ref(), 780)),
        backdrop: MediaSource::new(images.url(node.artwork.backdrop.as_ref(), 1920)),
        logo: MediaSource::new(images.url(node.artwork.logo.as_ref(), 1000)),
        episodes: items.len() as i64,
        watched,
        rating: crate::reviews::cached_rating(db, &node.id),
        movie: node.kind == Kind::Movie,
        premiere,
        has_premiere: date.is_some(),
        tags: Vec::new(),
        summary: node.overview.clone(),
        status,
    })
}

pub(super) fn watching(db: &Db, row: &MediaWatching) -> Watching {
    let images = db.catalog_images();
    let still = row
        .still
        .as_deref()
        .filter(|s| std::path::Path::new(s).exists())
        .map(MediaSource::file)
        .unwrap_or_else(|| {
            MediaSource::new(
                images.url(
                    row.work
                        .artwork
                        .backdrop
                        .as_ref()
                        .or(row.work.artwork.poster.as_ref()),
                    1920,
                ),
            )
        });
    Watching {
        id: row.work.id.0.clone(),
        title: row.work.title.clone(),
        still,
        episode: row
            .episode
            .as_ref()
            .and_then(|e| e.episode_number)
            .map_or(0, i64::from),
        chapter: row.episode.as_ref().map(chapter).unwrap_or_default(),
        left: ((row.file.duration - row.file.position).max(0.) / 60.).ceil() as i64,
        progress: if row.file.duration > 0. {
            (row.file.position / row.file.duration).clamp(0., 1.)
        } else {
            0.
        },
    }
}

fn chapter(node: &MediaNode) -> String {
    match (node.season_number, node.episode_number) {
        (Some(s), Some(e)) => format!("S{s:02}E{e:02}"),
        (None, Some(e)) => format!("E{e:02}"),
        _ => String::new(),
    }
}

pub(super) struct SelectedFile {
    pub file: MediaFile,
    pub span: Option<(f64, Option<f64>)>,
}
impl SelectedFile {
    pub fn position(&self) -> f64 {
        match self.span {
            Some((start, end))
                if self.file.position < start || end.is_some_and(|e| self.file.position >= e) =>
            {
                start
            }
            _ if self.file.duration > 0. && self.file.position >= self.file.duration => 0.,
            _ => self.file.position,
        }
    }
}

/// Prefer a precisely positioned version; otherwise explicitly resume the
/// whole collection. A missing chapter map is not a missing physical file.
pub(super) fn file(db: &Db, id: &MediaId) -> erika_store::catalog::Result<Option<SelectedFile>> {
    let mut collection = None;
    for file in db.catalog_files(id)? {
        let Some(binding) = db.catalog_binding(file.file)? else {
            continue;
        };
        let Some(item) = binding.items.iter().find(|i| &i.media == id) else {
            continue;
        };
        if !binding.has_item_positions() {
            if collection.is_none() {
                collection = Some(SelectedFile { file, span: None });
            }
            continue;
        }
        let start = item.start_seconds.unwrap_or(0.);
        let end = binding
            .item_end(item)
            .or((file.duration > 0.).then_some(file.duration));
        return Ok(Some(SelectedFile {
            file,
            span: Some((start, end)),
        }));
    }
    Ok(collection)
}

pub(super) fn film(
    db: &Db,
    node: &MediaNode,
    season: &str,
) -> erika_store::catalog::Result<(Vec<anime::Episode>, String)> {
    let images = db.catalog_images();
    let mut cells = Vec::new();
    for item in items(db, node)?
        .into_iter()
        .filter(|i| season.is_empty() || i.parent.as_ref().is_some_and(|p| p.0 == season))
    {
        let file = file(db, &item.id)?;
        let watched = db.catalog_watched(&item.id)?.is_some_and(|w| w.watched);
        let (duration, position) = file.as_ref().map_or((0., 0.), |selected| {
            let Some((start, end)) = selected.span else {
                return (0., 0.);
            };
            let duration = end.map_or(0., |end| (end - start).max(0.));
            let position = (selected.file.position - start).max(0.);
            (
                duration,
                if duration > 0. {
                    position.min(duration)
                } else {
                    position
                },
            )
        });
        let label = match (item.season_number, item.episode_number) {
            (Some(s), Some(e)) => format!("S{s:02}E{e:02} · {}", item.title),
            _ => item.title.clone(),
        };
        cells.push(anime::Episode {
            id: item.id.0.clone(),
            number: item.episode_number.map_or(0, i64::from),
            season: item.season_number.map_or(-1, i64::from),
            title: EpisodeTitle::Content { label },
            note: match &file {
                None => EpisodeNote::Missing,
                Some(selected) if selected.span.is_none() => EpisodeNote::Collection,
                Some(_) if !watched && position > 0. && duration > 0. => EpisodeNote::Left {
                    minutes: ((duration - position).max(0.) / 60.).ceil() as i64,
                },
                Some(_) if duration > 0. => EpisodeNote::Length {
                    minutes: (duration / 60.).round() as i64,
                },
                Some(_) if watched => EpisodeNote::File,
                _ => EpisodeNote::Unwatched,
            },
            pictured: item.artwork.still.is_some() || node.artwork.backdrop.is_some(),
            still: MediaSource::new(
                images.url(
                    item.artwork
                        .still
                        .as_ref()
                        .or(node.artwork.backdrop.as_ref()),
                    780,
                ),
            ),
            numeral: chapter(&item),
            watched,
            next: false,
            progress: if watched {
                1.
            } else if duration > 0. {
                (position / duration).clamp(0., 1.)
            } else {
                0.
            },
        });
    }
    let next = cells
        .iter()
        .find(|e| !e.watched && e.progress > 0.)
        .or_else(|| cells.iter().find(|e| !e.watched))
        .or_else(|| cells.first())
        .map(|e| e.id.clone())
        .unwrap_or_default();
    for cell in &mut cells {
        cell.next = cell.id == next;
    }
    Ok((cells, next))
}

pub(super) fn results(
    db: &Db,
    shows: Vec<Show>,
    watching: &[MediaWatching],
) -> anime::SearchResults {
    let continuing = watching
        .iter()
        .filter(|w| shows.iter().any(|s| s.id == w.work.id.0))
        .map(|w| {
            let row = watching_row(db, w);
            anime::SearchEpisode {
                id: row.id,
                title: row.title,
                episode: row.episode,
                chapter: row.chapter,
            }
        })
        .collect::<Vec<_>>();
    anime::SearchResults {
        has_shows: !shows.is_empty(),
        has_continuing: !continuing.is_empty(),
        shows,
        continuing,
    }
}
fn watching_row(db: &Db, row: &MediaWatching) -> Watching {
    watching(db, row)
}
