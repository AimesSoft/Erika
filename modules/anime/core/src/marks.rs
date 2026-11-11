//! Status and score vocabulary. Reconciliation is identity- and revision-based
//! in `sync`; states are not ordered and watched sets are never unioned.
use erika_anime_protocol::anime::{Score, WatchStatus};
use erika_dandanplay::bangumi::site::Interest;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Level {
    Dropped,
    Want,
    Watching,
    Watched,
}
impl Level {
    pub fn key(self) -> &'static str {
        match self {
            Self::Dropped => "dropped",
            Self::Want => "want",
            Self::Watching => "watching",
            Self::Watched => "watched",
        }
    }
    pub fn parse(key: &str) -> Option<Self> {
        [Self::Dropped, Self::Want, Self::Watching, Self::Watched]
            .into_iter()
            .find(|level| level.key() == key)
    }
    pub fn status(level: Option<Self>) -> WatchStatus {
        match level {
            None => WatchStatus::Unmarked,
            Some(Self::Dropped) => WatchStatus::Dropped,
            Some(Self::Want) => WatchStatus::Want,
            Some(Self::Watching) => WatchStatus::Watching,
            Some(Self::Watched) => WatchStatus::Watched,
        }
    }
    pub fn of_status(status: &WatchStatus) -> Option<Self> {
        match status {
            WatchStatus::Dropped => Some(Self::Dropped),
            WatchStatus::Want => Some(Self::Want),
            WatchStatus::Watching => Some(Self::Watching),
            WatchStatus::Watched => Some(Self::Watched),
            WatchStatus::Unmarked => None,
        }
    }
    pub fn of_interest(interest: Interest) -> Self {
        match interest {
            Interest::Wish => Self::Want,
            Interest::Collect => Self::Watched,
            Interest::Do | Interest::OnHold => Self::Watching,
            Interest::Dropped => Self::Dropped,
        }
    }
    pub fn interest(self) -> Interest {
        match self {
            Self::Want => Interest::Wish,
            Self::Watching => Interest::Do,
            Self::Watched => Interest::Collect,
            Self::Dropped => Interest::Dropped,
        }
    }
}
pub(crate) fn score(points: i64) -> Score {
    match points {
        1 => Score::One,
        2 => Score::Two,
        3 => Score::Three,
        4 => Score::Four,
        5 => Score::Five,
        6 => Score::Six,
        7 => Score::Seven,
        8 => Score::Eight,
        9 => Score::Nine,
        10 => Score::Ten,
        _ => Score::Unrated,
    }
}
pub(crate) fn points(score: &Score) -> i64 {
    (0..=10)
        .find(|points| self::score(*points) == *score)
        .unwrap_or(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn statuses_and_scores_round_trip_without_priority_rules() {
        for level in [Level::Dropped, Level::Want, Level::Watching, Level::Watched] {
            assert_eq!(Level::parse(level.key()), Some(level));
            assert_eq!(Level::of_status(&Level::status(Some(level))), Some(level));
            assert_eq!(Level::of_interest(level.interest()), level);
        }
        assert_eq!(Level::of_status(&WatchStatus::Unmarked), None);
        assert_eq!(Level::parse("on_hold"), None);
        assert_eq!(Level::of_interest(Interest::OnHold), Level::Watching);
        for value in 0..=10 {
            assert_eq!(points(&score(value)), value);
        }
        assert_eq!(score(11), Score::Unrated);
    }
}
