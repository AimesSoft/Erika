//! Where the user is with each show and its score, kept in the app's
//! database (`anime_mark`), which the account sync carries to their other
//! devices.

use erika_anime_protocol::anime::WatchStatus;
use std::collections::HashMap;

fn status_of(text: &str) -> Option<WatchStatus> {
    Some(match text {
        "Want" => WatchStatus::Want,
        "Watching" => WatchStatus::Watching,
        "Watched" => WatchStatus::Watched,
        _ => return None,
    })
}

/// The statuses and scores the user gave.
pub fn load(db: &erika_store::Db) -> (HashMap<i64, WatchStatus>, HashMap<i64, f64>) {
    let mut statuses = HashMap::new();
    let mut ratings = HashMap::new();
    for mark in db.anime_marks() {
        if let Some(status) = mark.status.as_deref().and_then(status_of) {
            statuses.insert(mark.anime, status);
        }
        if mark.rating > 0.0 {
            ratings.insert(mark.anime, mark.rating);
        }
    }
    (statuses, ratings)
}

/// Keep what the user gave show `id`.
pub fn save(db: &erika_store::Db, id: i64, status: Option<&WatchStatus>, rating: Option<f64>) {
    db.set_anime_mark(&erika_store::portable::Mark {
        anime: id,
        status: status.map(|status| format!("{status:?}")),
        rating: rating.unwrap_or(0.0),
        at: erika_store::now(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_and_scores_come_back_from_the_database() {
        let db = erika_store::Db::in_memory();
        save(&db, 17675, Some(&WatchStatus::Watching), Some(9.0));
        save(&db, 42, None, Some(6.0));
        save(&db, 7, Some(&WatchStatus::Want), None);
        let (statuses, ratings) = load(&db);
        assert_eq!(statuses.get(&17675), Some(&WatchStatus::Watching));
        assert_eq!(statuses.get(&7), Some(&WatchStatus::Want));
        assert!(!statuses.contains_key(&42));
        assert_eq!(ratings.get(&42), Some(&6.0));
        assert!(!ratings.contains_key(&7));
    }
}
