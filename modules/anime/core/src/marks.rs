//! Where the user is with a show and which of its episodes they watched,
//! as this device, dandanplay and Bangumi each keep it, and how the three
//! come together: the status furthest along wins (watched, then watching,
//! then want to watch, then dropped), and an episode watched in one place
//! is watched in all. A place is written only when what came together is
//! further along than what it has, so a sync never takes back what an
//! account holds: wanting to watch here leaves "watched" on Bangumi alone,
//! and an episode not watched here leaves it watched on dandanplay.

use erika_anime_protocol::anime::{Score, WatchStatus};
use erika_dandanplay::bangumi::site::Interest;
use erika_dandanplay::mine::Follow;
use std::collections::BTreeSet;

/// A status, in the order of how far along it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Level {
    Dropped,
    Want,
    Watching,
    Watched,
}

impl Level {
    /// The word the database keeps it as.
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

    /// How the user calls it, for the notes of a sync.
    pub fn word(self) -> &'static str {
        match self {
            Self::Dropped => "抛弃",
            Self::Want => "想看",
            Self::Watching => "在看",
            Self::Watched => "看过",
        }
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

    /// dandanplay follows a show without telling wanting to watch it from
    /// watching it, so a follow reads as the lesser of the two.
    pub fn of_follow(follow: Follow) -> Self {
        match follow {
            Follow::Favorited => Self::Want,
            Follow::Finished => Self::Watched,
            Follow::Abandoned => Self::Dropped,
        }
    }

    pub fn follow(self) -> Follow {
        match self {
            Self::Want | Self::Watching => Follow::Favorited,
            Self::Watched => Follow::Finished,
            Self::Dropped => Follow::Abandoned,
        }
    }

    /// Bangumi's "on hold" reads as watching: the user started the show,
    /// and only finishing it is further along.
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

/// What one place keeps of a show: its status, and the numbers of the
/// episodes watched.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Side {
    pub level: Option<Level>,
    pub watched: BTreeSet<i64>,
}

/// The episodes of a show, and whether it is still airing, for what
/// watching them says of the show.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Film {
    pub numbers: Vec<i64>,
    pub on_air: bool,
}

/// What a place is sent: the status to write, when it must be written,
/// and the episodes to mark watched.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Push {
    pub level: Option<Level>,
    pub watched: Vec<i64>,
}

/// What a sync comes to: the status and the episodes watched, shown and
/// kept here, and what each account is sent.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Plan {
    pub level: Option<Level>,
    pub watched: BTreeSet<i64>,
    pub dandanplay: Push,
    pub bangumi: Push,
}

/// The status the episodes watched say: watching once one is, watched once
/// every one of a show no longer airing is.
pub(crate) fn implied(watched: &BTreeSet<i64>, film: &Film) -> Option<Level> {
    if !film.on_air
        && !film.numbers.is_empty()
        && film.numbers.iter().all(|number| watched.contains(number))
    {
        Some(Level::Watched)
    } else if !watched.is_empty() {
        Some(Level::Watching)
    } else {
        None
    }
}

/// Bring this device and the accounts signed in together. `held` is what
/// a place can keep of a status: dandanplay keeps watching as a follow,
/// which reads back as wanting to watch, so it is not written again.
pub(crate) fn plan(
    local: &Side,
    dandanplay: Option<&Side>,
    bangumi: Option<&Side>,
    film: &Film,
) -> Plan {
    let sides = [Some(local), dandanplay, bangumi];
    let watched: BTreeSet<i64> = sides
        .iter()
        .flatten()
        .flat_map(|side| side.watched.iter().copied())
        .collect();
    let level = sides
        .iter()
        .flatten()
        .map(|side| side.level)
        .chain([implied(&watched, film)])
        .max()
        .flatten();
    let push = |side: Option<&Side>, held: fn(Level) -> Level| match side {
        None => Push::default(),
        Some(side) => Push {
            level: level.filter(|level| side.level.is_none_or(|has| held(*level) > has)),
            watched: watched.difference(&side.watched).copied().collect(),
        },
    };
    Plan {
        dandanplay: push(dandanplay, |level| Level::of_follow(level.follow())),
        bangumi: push(bangumi, |level| level),
        level,
        watched,
    }
}

/// A score out of ten as the score dialog's step.
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

    fn side(level: Option<Level>, watched: &[i64]) -> Side {
        Side {
            level,
            watched: watched.iter().copied().collect(),
        }
    }

    fn film(episodes: i64, on_air: bool) -> Film {
        Film {
            numbers: (1..=episodes).collect(),
            on_air,
        }
    }

    #[test]
    fn the_status_furthest_along_wins_and_is_never_written_over() {
        use Level::*;
        let order = [Dropped, Want, Watching, Watched];
        for (i, &here) in order.iter().enumerate() {
            for (j, &there) in order.iter().enumerate() {
                let plan = super::plan(
                    &side(Some(here), &[]),
                    None,
                    Some(&side(Some(there), &[])),
                    &film(12, false),
                );
                assert_eq!(plan.level, Some(order[i.max(j)]), "{here:?} / {there:?}");
                // Bangumi is written only when this device is further along.
                assert_eq!(plan.bangumi.level, (i > j).then_some(here));
            }
        }
        // Wanting to watch here leaves "watched" on Bangumi, and shows it.
        let plan = super::plan(
            &side(Some(Want), &[]),
            Some(&side(None, &[])),
            Some(&side(Some(Watched), &[])),
            &film(12, false),
        );
        assert_eq!(plan.level, Some(Watched));
        assert_eq!(plan.bangumi.level, None);
        assert_eq!(plan.dandanplay.level, Some(Watched));
        assert_eq!(Level::status(plan.level), WatchStatus::Watched);
    }

    #[test]
    fn nothing_marked_anywhere_stays_unmarked() {
        let plan = super::plan(
            &Side::default(),
            Some(&Side::default()),
            Some(&Side::default()),
            &film(12, false),
        );
        assert_eq!(plan, Plan::default());
        assert_eq!(Level::status(plan.level), WatchStatus::Unmarked);
    }

    #[test]
    fn a_dandanplay_follow_is_not_written_again_for_watching() {
        let followed = side(Some(Level::of_follow(Follow::Favorited)), &[]);
        let plan = super::plan(
            &side(Some(Level::Watching), &[]),
            Some(&followed),
            None,
            &film(12, true),
        );
        assert_eq!(plan.level, Some(Level::Watching));
        assert_eq!(plan.dandanplay.level, None);
        // Dropped here is behind a follow there: dandanplay keeps it.
        let plan = plan_of(Some(Level::Dropped), Some(Follow::Favorited));
        assert_eq!(
            (plan.level, plan.dandanplay.level),
            (Some(Level::Want), None)
        );
        // Nothing there yet: wanting to watch and watching both follow it.
        for level in [Level::Want, Level::Watching] {
            let plan = plan_of(Some(level), None);
            assert_eq!(
                plan.dandanplay.level.map(Level::follow),
                Some(Follow::Favorited)
            );
        }
        let plan = plan_of(Some(Level::Watched), Some(Follow::Favorited));
        assert_eq!(
            plan.dandanplay.level.map(Level::follow),
            Some(Follow::Finished)
        );
    }

    fn plan_of(here: Option<Level>, follow: Option<Follow>) -> Plan {
        plan(
            &side(here, &[]),
            Some(&side(follow.map(Level::of_follow), &[])),
            None,
            &film(12, true),
        )
    }

    #[test]
    fn bangumi_on_hold_counts_as_watching_and_is_kept() {
        let on_hold = side(Some(Level::of_interest(Interest::OnHold)), &[]);
        let plan = super::plan(
            &side(Some(Level::Watching), &[1]),
            None,
            Some(&on_hold),
            &film(12, false),
        );
        assert_eq!(plan.bangumi.level, None);
        assert_eq!(plan.bangumi.watched, vec![1]);
        assert_eq!(Level::of_interest(Interest::Collect), Level::Watched);
        assert_eq!(Level::Watched.interest(), Interest::Collect);
    }

    #[test]
    fn episodes_watched_anywhere_are_watched_everywhere_and_never_taken_back() {
        let plan = super::plan(
            &side(None, &[1, 2]),
            Some(&side(None, &[2, 3])),
            Some(&side(None, &[5])),
            &film(12, true),
        );
        assert_eq!(plan.watched, [1, 2, 3, 5].into());
        assert_eq!(plan.dandanplay.watched, vec![1, 5]);
        assert_eq!(plan.bangumi.watched, vec![1, 2, 3]);
        // Watching an episode makes the show one being watched, everywhere.
        assert_eq!(plan.level, Some(Level::Watching));
        assert_eq!(plan.bangumi.level, Some(Level::Watching));
        // An episode not watched here does not unwatch it on an account.
        let plan = super::plan(
            &side(None, &[]),
            None,
            Some(&side(None, &[4])),
            &film(12, true),
        );
        assert_eq!(plan.watched, [4].into());
        assert!(plan.bangumi.watched.is_empty());
    }

    #[test]
    fn every_episode_of_a_finished_show_makes_it_watched() {
        let all: Vec<i64> = (1..=12).collect();
        let finished = super::plan(&side(Some(Level::Want), &all), None, None, &film(12, false));
        assert_eq!(finished.level, Some(Level::Watched));
        // One still airing, or one episode left, is only being watched.
        let airing = super::plan(&side(None, &all), None, None, &film(12, true));
        assert_eq!(airing.level, Some(Level::Watching));
        let left = super::plan(&side(None, &all[..11]), None, None, &film(12, false));
        assert_eq!(left.level, Some(Level::Watching));
        assert_eq!(implied(&BTreeSet::new(), &film(0, false)), None);
    }

    #[test]
    fn statuses_and_scores_read_back_as_they_are_kept() {
        for level in [Level::Dropped, Level::Want, Level::Watching, Level::Watched] {
            assert_eq!(Level::parse(level.key()), Some(level));
            assert_eq!(Level::of_status(&Level::status(Some(level))), Some(level));
        }
        assert_eq!(Level::of_status(&WatchStatus::Unmarked), None);
        assert_eq!(Level::parse("on_hold"), None);
        for points in 0..=10 {
            assert_eq!(self::points(&score(points)), points);
        }
        assert_eq!(score(11), Score::Unrated);
    }
}
