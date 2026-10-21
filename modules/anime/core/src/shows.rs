//! The shows as the pages show them, from what dandanplay returned and
//! what the database knows of the user's files.

use crate::clock::{date_of, parse_utc};
use bern::date::CalendarDate;
use bern::media::MediaSource;
use erika_anime_protocol::anime::{
    Episode, EpisodeNote, EpisodeTitle, SearchEpisode, SearchResults, Show, Tag, WatchStatus, Watching,
};
use erika_dandanplay::{Details, Found, Intro};
use erika_store::EpisodeFile;

/// How far into an episode it counts as watched.
const WATCHED: f64 = 0.9;
/// The most cells the film holds; a longer series shows those around the
/// episode to watch next.
const FILM: usize = 60;

fn status(watched: i64, episodes: i64) -> WatchStatus {
    match watched {
        0 => WatchStatus::Want,
        watched if episodes > 0 && watched >= episodes => WatchStatus::Watched,
        _ => WatchStatus::Watching,
    }
}

fn progress(file: &EpisodeFile) -> f64 {
    if file.duration > 0.0 { (file.position / file.duration).clamp(0.0, 1.0) } else { 0.0 }
}

/// How many episodes the user watched to the end.
fn watched(files: &[EpisodeFile]) -> i64 {
    files.iter().filter(|file| progress(file) >= WATCHED).count() as i64
}

fn show(id: i64, title: &str, image: &str, today: CalendarDate) -> Show {
    Show {
        id,
        title: title.into(),
        poster: MediaSource::new(image),
        episodes: 0,
        watched: 0,
        rating: 0.0,
        movie: false,
        premiere: today,
        tags: Vec::new(),
        summary: String::new(),
        status: WatchStatus::Want,
    }
}

/// The show while nothing is open.
pub fn untitled() -> Show {
    show(0, "", "", CalendarDate::new(1970, 1, 1))
}

/// The stage while nothing is being watched; the page hides it.
pub fn nothing_watched() -> Watching {
    Watching { id: 0, title: String::new(), still: MediaSource::new(""), episode: 1, left: 0, progress: 0.0 }
}

pub fn from_intro(intro: &Intro, today: CalendarDate) -> Show {
    Show { rating: intro.rating, ..show(intro.anime_id, &intro.anime_title, &intro.image_url, today) }
}

pub fn from_found(found: &Found, today: CalendarDate) -> Show {
    Show {
        rating: found.rating,
        episodes: found.episode_count,
        movie: is_movie(&found.type_description),
        ..show(found.anime_id, &found.anime_title, &found.image_url, today)
    }
}

pub fn from_anime(anime: &erika_store::Anime, today: CalendarDate) -> Show {
    show(anime.dandanplay_id, &anime.title, &anime.image, today)
}

pub fn from_watching(watching: &erika_store::Watching, today: CalendarDate) -> Show {
    from_anime(&watching.anime, today)
}

fn is_movie(kind: &str) -> bool {
    kind.contains("剧场版") || kind.contains("电影") || kind.to_lowercase().contains("movie")
}

pub fn from_details(details: &Details, files: &[EpisodeFile]) -> Show {
    let episodes = details.episodes.len() as i64;
    let watched = watched(files);
    let premiere = details
        .episodes
        .iter()
        .find_map(|episode| parse_utc(episode.air_date.as_deref()?))
        .map(|unix| date_of(unix.div_euclid(86_400)))
        .unwrap_or(CalendarDate::new(1970, 1, 1));
    Show {
        id: details.anime_id,
        title: details.anime_title.clone(),
        poster: MediaSource::new(details.image_url.clone()),
        episodes,
        watched,
        rating: details.rating,
        movie: is_movie(&details.type_description),
        premiere,
        tags: details.tags.iter().take(6).map(|tag| Tag { name: tag.name.clone() }).collect(),
        summary: details.summary.trim().to_string(),
        status: status(watched, episodes),
    }
}

/// A row of what is being watched.
pub fn watching(watching: &erika_store::Watching) -> Watching {
    let still = watching
        .still
        .as_deref()
        .filter(|still| std::path::Path::new(still).exists())
        .map(MediaSource::file)
        .unwrap_or_else(|| MediaSource::new(watching.anime.image.clone()));
    Watching {
        id: watching.anime.dandanplay_id,
        title: watching.anime.title.clone(),
        still,
        episode: watching.episode.number,
        left: ((watching.duration - watching.position).max(0.0) / 60.0).ceil() as i64,
        progress: if watching.duration > 0.0 { (watching.position / watching.duration).clamp(0.0, 1.0) } else { 0.0 },
    }
}

/// The number dandanplay writes for an episode, or its place in the list.
fn number(text: &str, index: usize) -> i64 {
    text.trim().parse().unwrap_or(index as i64 + 1)
}

/// The film of the details page, and the episode to watch next: the one
/// after the last the user watched to the end.
pub fn film(details: Option<&Details>, show: &Show, files: &[EpisodeFile]) -> (Vec<Episode>, i64) {
    let numbers: Vec<(i64, i64)> = match details {
        Some(details) => details
            .episodes
            .iter()
            .enumerate()
            .map(|(index, episode)| (number(&episode.episode_number, index), episode.episode_id))
            .collect(),
        // Until the details come, the episodes the user has files of.
        None => files.iter().map(|file| (file.episode.number, file.episode.dandanplay_id)).collect(),
    };
    let file_of = |id: i64| files.iter().find(|file| file.episode.dandanplay_id == id);
    let last_watched = numbers
        .iter()
        .filter(|(_, id)| file_of(*id).is_some_and(|file| progress(file) >= WATCHED))
        .map(|(number, _)| *number)
        .max()
        .unwrap_or(0);
    let next = numbers
        .iter()
        .map(|(number, _)| *number)
        .find(|number| *number > last_watched)
        .or_else(|| numbers.last().map(|(number, _)| *number))
        .unwrap_or(1);
    let middle = numbers.iter().position(|(number, _)| *number == next).unwrap_or(0);
    let start = middle.saturating_sub(FILM / 2).min(numbers.len().saturating_sub(FILM));
    let cells = numbers
        .iter()
        .skip(start)
        .take(FILM)
        .map(|&(number, id)| {
            let file = file_of(id);
            let progress = file.map_or(0.0, progress);
            let watched = progress >= WATCHED;
            let current = number == next && !watched;
            Episode {
                number,
                title: match (current, last_watched) {
                    (true, 0) => EpisodeTitle::Start { number },
                    (true, _) => EpisodeTitle::Continue { number },
                    _ => EpisodeTitle::Plain { number },
                },
                note: match file {
                    _ if watched => EpisodeNote::Watched,
                    Some(file) if progress > 0.0 => EpisodeNote::Left { minutes: ((file.duration - file.position).max(0.0) / 60.0).ceil() as i64 },
                    Some(file) if file.duration > 0.0 => EpisodeNote::Length { minutes: (file.duration / 60.0).round() as i64 },
                    Some(_) => EpisodeNote::Unwatched,
                    None => EpisodeNote::Missing,
                },
                pictured: watched || current,
                still: show.poster.clone(),
                numeral: format!("{number:02}"),
                watched,
                next: current,
                progress,
            }
        })
        .collect();
    (cells, next)
}

/// Search results, with the shows among them that can be continued.
pub fn results(shows: Vec<Show>, watching: &[erika_store::Watching]) -> SearchResults {
    let continuing: Vec<SearchEpisode> = shows
        .iter()
        .filter_map(|show| watching.iter().find(|watching| watching.anime.dandanplay_id == show.id))
        .take(2)
        .map(|watching| SearchEpisode {
            id: watching.anime.dandanplay_id,
            title: watching.anime.title.clone(),
            episode: watching.episode.number,
        })
        .collect();
    SearchResults { has_shows: !shows.is_empty(), has_continuing: !continuing.is_empty(), shows, continuing }
}
