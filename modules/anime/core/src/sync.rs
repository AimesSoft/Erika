//! Syncing a show's marks with the accounts signed in. On a thread of its
//! own, a sync reads what dandanplay and Bangumi have, brings it together
//! with what is kept here by the rules of [`marks`](crate::marks), sends
//! each account what it lacks, and comes back with what to keep and show.
//! What the user does on the details (a status, a score, an episode, a
//! review) is kept here at once and rides along with the next sync.

use crate::catalog::Accounts;
use crate::marks::{self, Film, Level, Side};
use crate::{AnimeEngine, Done, in_background, shows};
use erika_anime_protocol::anime::SyncTargets;
use erika_dandanplay::bangumi::site::{self, Interest};
use erika_dandanplay::mine;
use std::collections::{BTreeSet, HashMap, VecDeque};

/// Bangumi takes this many characters of a review.
pub(crate) const REVIEW_LIMIT: usize = 200;

/// Something the user did, to send with the next sync of the show.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Intent {
    Status(Level),
    Rate(i64),
    Mark { number: i64, watched: bool },
    Review(String),
}

/// What a sync of one show needs, taken on the engine's thread.
#[derive(Clone, Debug)]
pub(crate) struct Job {
    pub id: i64,
    pub accounts: Accounts,
    /// The Bangumi subject, when a verified link names it.
    pub subject: Option<i64>,
    /// The status kept here, and the episodes watched here: marked, or
    /// played to the end.
    pub local: Side,
    /// The episodes as the details known here list them, by number and
    /// id, and whether the show still airs.
    pub episodes: Vec<(i64, i64)>,
    pub on_air: bool,
    pub intents: Vec<Intent>,
}

/// What a sync came back with.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Synced {
    pub id: i64,
    pub level: Option<Level>,
    pub watched: BTreeSet<i64>,
    /// The user's score where it is known: as given now, or as the
    /// accounts have it.
    pub rating: Option<i64>,
    /// The review on Bangumi, when it was read.
    pub review: Option<String>,
    /// How sending a review went, when one was sent.
    pub reviewed: Option<Result<(), String>>,
    pub notes: Vec<String>,
}

impl Job {
    /// Whether an account could take part: dandanplay knows only its own
    /// ids, Bangumi only shows linked to a subject.
    pub fn reaches(&self) -> bool {
        self.dandanplay() || self.bangumi()
    }

    fn dandanplay(&self) -> bool {
        self.accounts.dandanplay
            && self.id > 0
            && erika_dandanplay::bangumi::subject_id(self.id).is_none()
    }

    fn bangumi(&self) -> bool {
        self.accounts.bangumi && self.subject.is_some()
    }
}

/// The number dandanplay writes for an episode, when it is a plain one.
fn number(text: &str) -> Option<i64> {
    text.trim().parse().ok().filter(|number| *number > 0)
}

fn ids_of(map: &[(i64, i64)], numbers: &[i64]) -> Vec<i64> {
    numbers
        .iter()
        .filter_map(|number| map.iter().find(|(n, _)| n == number).map(|(_, id)| *id))
        .collect()
}

/// Why the status chosen gave way to `level`, further along: where that
/// one was kept.
fn kept(level: Level, chosen: Level, dandanplay: Option<&Side>, bangumi: Option<&Side>) -> String {
    let place = if bangumi.is_some_and(|side| side.level == Some(level)) {
        "Bangumi 上"
    } else if dandanplay.is_some_and(|side| side.level == Some(level)) {
        "弹弹play 上"
    } else {
        return format!(
            "已有看过的剧集，状态保留为「{}」，没有改成「{}」",
            level.word(),
            chosen.word()
        );
    };
    format!(
        "{place}已是「{}」，按优先级保留，没有改成「{}」",
        level.word(),
        chosen.word()
    )
}

/// Run one sync. The network calls block; this runs on its own thread.
pub(crate) fn run(job: Job) -> Synced {
    let mut notes = Vec::new();
    let mut local = job.local.clone();
    let mut chosen = None;
    let mut rate = None;
    let mut review = None;
    let mut unmarked = Vec::new();
    for intent in &job.intents {
        match intent {
            Intent::Status(level) => {
                local.level = Some(*level);
                chosen = Some(*level);
            }
            Intent::Rate(points) => rate = Some((*points).clamp(1, 10)),
            Intent::Mark { number, watched } => {
                if *watched {
                    local.watched.insert(*number);
                    unmarked.retain(|n| n != number);
                } else {
                    local.watched.remove(number);
                    unmarked.push(*number);
                }
            }
            Intent::Review(text) => review = Some(text.clone()),
        }
    }
    // A score or a review puts the show on the accounts' lists: as wanted,
    // unless it is further along somewhere.
    if (rate.is_some() || review.is_some()) && local.level.is_none() {
        local.level = Some(Level::Want);
    }

    let dandanplay = if job.dandanplay() {
        match mine::mine(job.id) {
            Ok((details, mine)) => Some((details, mine)),
            Err(error) => {
                notes.push(format!("弹弹play：{}", error.message));
                None
            }
        }
    } else {
        None
    };
    let dandanplay_map: Vec<(i64, i64)> = dandanplay
        .as_ref()
        .map(|(details, _)| {
            details
                .episodes
                .iter()
                .filter_map(|episode| Some((number(&episode.episode_number)?, episode.episode_id)))
                .collect()
        })
        .unwrap_or_default();
    let dandanplay_side = dandanplay.as_ref().map(|(_, mine)| Side {
        level: mine.follow.map(Level::of_follow),
        watched: dandanplay_map
            .iter()
            .filter(|(_, id)| mine.watched.contains(id))
            .map(|(number, _)| *number)
            .collect(),
    });

    let subject = job.subject.filter(|_| job.bangumi());
    let bangumi = subject.and_then(|subject| match site::collection(subject) {
        Ok(collection) => Some(collection),
        Err(error) => {
            notes.push(format!("Bangumi：{}", error.message));
            None
        }
    });
    let bangumi_map: Vec<(i64, i64)> = bangumi
        .as_ref()
        .map(|collection| {
            collection
                .episodes
                .iter()
                .map(|episode| (episode.number, episode.id))
                .collect()
        })
        .unwrap_or_default();
    let mut bangumi_side = bangumi.as_ref().map(|collection| Side {
        level: collection.interest.map(Level::of_interest),
        watched: collection
            .episodes
            .iter()
            .filter(|episode| episode.watched)
            .map(|episode| episode.number)
            .collect(),
    });
    // Taking a mark back is the user's own doing: Bangumi takes it back
    // too, unless dandanplay, which cannot, still has it watched.
    let mut taken_back = Vec::new();
    if let Some(side) = bangumi_side.as_mut() {
        for number in &unmarked {
            let stays = dandanplay_side
                .as_ref()
                .is_some_and(|side| side.watched.contains(number));
            if !stays && side.watched.remove(number) {
                taken_back.push(*number);
            }
        }
    }

    let film = Film {
        numbers: if !dandanplay_map.is_empty() {
            dandanplay_map.iter().map(|(number, _)| *number).collect()
        } else {
            job.episodes.iter().map(|(number, _)| *number).collect()
        },
        on_air: dandanplay
            .as_ref()
            .map_or(job.on_air, |(details, _)| details.is_on_air),
    };
    let plan = marks::plan(
        &local,
        dandanplay_side.as_ref(),
        bangumi_side.as_ref(),
        &film,
    );

    if let Some((_, mine)) = &dandanplay {
        let ids = ids_of(&dandanplay_map, &plan.dandanplay.watched);
        if !ids.is_empty() {
            eprintln!(
                "erika: sync {}: dandanplay watched {:?}",
                job.id, plan.dandanplay.watched
            );
            if let Err(error) = mine::watched(&ids) {
                notes.push(format!("弹弹play：{}", error.message));
            }
        }
        let follow = plan.dandanplay.level.map(Level::follow);
        let points = rate.unwrap_or(0);
        if follow.is_some() || points > 0 {
            eprintln!(
                "erika: sync {}: dandanplay follow {follow:?} rating {points}",
                job.id
            );
            if let Err(error) = mine::follow(job.id, follow, points) {
                notes.push(format!("弹弹play：{}", error.message));
            }
        }
        for number in &unmarked {
            if let Some(&(_, id)) = dandanplay_map.iter().find(|(n, _)| n == number)
                && mine.watched.contains(&id)
            {
                notes.push(format!(
                    "弹弹play 不能取消已看，第 {number} 话在那里仍是看过"
                ));
            }
        }
    }

    let mut reviewed = None;
    if let (Some(subject), Some(collection)) = (subject, &bangumi) {
        // Episodes first: marking them may collect the show as watching,
        // which the collection saved after sets right.
        let ids = ids_of(&bangumi_map, &plan.bangumi.watched);
        if !ids.is_empty() {
            eprintln!(
                "erika: sync {}: bangumi {subject} watched {:?}",
                job.id, plan.bangumi.watched
            );
            if let Err(error) = site::mark(subject, collection, &ids, true) {
                notes.push(format!("Bangumi：{}", error.message));
            }
        }
        let ids = ids_of(&bangumi_map, &taken_back);
        if !ids.is_empty() {
            eprintln!(
                "erika: sync {}: bangumi {subject} not watched {taken_back:?}",
                job.id
            );
            if let Err(error) = site::mark(subject, collection, &ids, false) {
                notes.push(format!("Bangumi：{}", error.message));
            }
        }
        let marked_watched = !plan.bangumi.watched.is_empty() && collection.interest.is_none();
        if plan.bangumi.level.is_some() || rate.is_some() || review.is_some() || marked_watched {
            let mut saved = collection.clone();
            saved.interest = plan
                .bangumi
                .level
                .map(Level::interest)
                .or(saved.interest)
                .or(plan.level.map(Level::interest))
                .or(Some(Interest::Wish));
            if let Some(points) = rate {
                saved.rating = points;
            }
            if let Some(text) = &review {
                saved.comment = text.clone();
            }
            eprintln!(
                "erika: sync {}: bangumi {subject} collection {:?} rating {} review {}",
                job.id,
                saved.interest,
                saved.rating,
                if review.is_some() { "changed" } else { "kept" }
            );
            let result = site::save(subject, &saved).map_err(|error| error.message);
            if let Err(error) = &result {
                notes.push(format!("Bangumi：{error}"));
            }
            if review.is_some() {
                reviewed = Some(result);
            }
        }
    }
    if review.is_some() && reviewed.is_none() {
        reviewed = Some(Err(notes
            .iter()
            .find(|note| note.starts_with("Bangumi"))
            .cloned()
            .unwrap_or_else(|| "这部作品没有对应的 Bangumi 条目".into())));
    }

    if let (Some(chosen), Some(level)) = (chosen, plan.level)
        && level > chosen
    {
        notes.insert(
            0,
            kept(
                level,
                chosen,
                dandanplay_side.as_ref(),
                bangumi_side.as_ref(),
            ),
        );
    }
    for note in &notes {
        eprintln!("erika: sync {}: {note}", job.id);
    }
    Synced {
        id: job.id,
        level: plan.level,
        watched: plan.watched,
        rating: rate
            .or(bangumi.as_ref().map(|c| c.rating).filter(|r| *r > 0))
            .or(dandanplay
                .as_ref()
                .map(|(_, m)| m.rating)
                .filter(|r| *r > 0)),
        review: review.or(bangumi.map(|collection| collection.comment)),
        reviewed,
        notes,
    }
}

/// What is kept here of a show: its mark, and the episodes marked watched.
#[derive(Clone, Debug, Default)]
pub(crate) struct Kept {
    pub mark: erika_store::Mark,
    pub watched: BTreeSet<i64>,
}

impl Kept {
    pub fn level(&self) -> Option<Level> {
        self.mark.status.as_deref().and_then(Level::parse)
    }
}

/// The syncs waiting and the one under way: one at a time, so that a
/// start with several shows to sync does not flood the accounts.
#[derive(Default)]
pub(crate) struct Syncs {
    running: Option<i64>,
    waiting: VecDeque<i64>,
    intents: HashMap<i64, Vec<Intent>>,
    /// The episodes played to the end, by show, as last synced.
    played: HashMap<i64, BTreeSet<i64>>,
    /// The show synced as it opened, so that its details coming do not
    /// sync it again.
    pub opened: i64,
}

impl Syncs {
    pub fn busy(&self, id: i64) -> bool {
        self.running == Some(id) || self.waiting.contains(&id)
    }
}

impl AnimeEngine {
    /// What is kept here of show `id`.
    pub(crate) fn kept(&mut self, id: i64) -> Kept {
        let db = &self.db;
        self.kept
            .entry(id)
            .or_insert_with(|| Kept {
                mark: db.mark(id),
                watched: db.watched_numbers(id).into_iter().collect(),
            })
            .clone()
    }

    /// The episodes of show `id` played to the end here.
    fn played(&self, id: i64) -> BTreeSet<i64> {
        shows::played(&self.db.episode_files(id))
    }

    /// The episodes of show `id` watched as far as this device knows.
    pub(crate) fn watched_here(&mut self, id: i64) -> BTreeSet<i64> {
        let mut watched = self.kept(id).watched;
        watched.extend(self.played(id));
        watched
    }

    /// The episodes of show `id` by number and id, from its details.
    pub(crate) fn numbered(&self, id: i64) -> Vec<(i64, i64)> {
        self.details
            .get(&id)
            .map(shows::numbered)
            .unwrap_or_default()
    }

    pub(crate) fn film_of(&self, id: i64) -> Film {
        Film {
            numbers: self
                .numbered(id)
                .into_iter()
                .map(|(number, _)| number)
                .collect(),
            on_air: self.details.get(&id).is_some_and(|d| d.is_on_air),
        }
    }

    /// The status shown for show `id`: the one kept here, or the one its
    /// episodes say, whichever is further along.
    pub(crate) fn level_of(&mut self, id: i64, watched: &BTreeSet<i64>) -> Option<Level> {
        let film = self.film_of(id);
        self.kept(id).level().max(marks::implied(watched, &film))
    }

    /// The Bangumi subject of show `id`, when a verified link names it.
    pub(crate) fn subject(&self, id: i64) -> Option<i64> {
        erika_dandanplay::bangumi::subject_id(id)
            .or_else(|| {
                self.details
                    .get(&id)
                    .and_then(|d| erika_dandanplay::reviews::subject_id(&d.bangumi_url))
            })
            .or_else(|| self.links.subject(id))
    }

    fn job(&mut self, id: i64, intents: Vec<Intent>) -> Job {
        Job {
            id,
            accounts: self.accounts,
            subject: self.subject(id),
            local: Side {
                level: self.kept(id).level(),
                watched: self.watched_here(id),
            },
            episodes: self.numbered(id),
            on_air: self.details.get(&id).is_some_and(|d| d.is_on_air),
            intents,
        }
    }

    /// Where the marks of show `id` go besides this device.
    pub(crate) fn targets(&self, id: i64) -> SyncTargets {
        let reach = self.job_reach(id);
        match reach {
            (true, true) => SyncTargets::Both,
            (true, false) => SyncTargets::Dandanplay,
            (false, true) => SyncTargets::Bangumi,
            (false, false) => SyncTargets::Local,
        }
    }

    fn job_reach(&self, id: i64) -> (bool, bool) {
        let job = Job {
            id,
            accounts: self.accounts,
            subject: self.subject(id),
            local: Side::default(),
            episodes: Vec::new(),
            on_air: false,
            intents: Vec::new(),
        };
        (job.dandanplay(), job.bangumi())
    }

    fn anime_of(&self, id: i64) -> erika_store::Anime {
        if let Some(details) = self.details.get(&id) {
            return erika_store::Anime {
                dandanplay_id: id,
                title: details.anime_title.clone(),
                image: details.image_url.clone(),
            };
        }
        self.db.anime(id).unwrap_or_else(|| erika_store::Anime {
            dandanplay_id: id,
            title: self.find(id).map(|show| show.title).unwrap_or_default(),
            image: self
                .season
                .iter()
                .find(|intro| intro.anime_id == id)
                .map(|intro| intro.image_url.clone())
                .unwrap_or_default(),
        })
    }

    fn episode_of(&self, id: i64, number: i64) -> erika_store::Episode {
        let episode = self.details.get(&id).and_then(|details| {
            details
                .episodes
                .iter()
                .enumerate()
                .find(|(index, episode)| shows::number(&episode.episode_number, *index) == number)
                .map(|(_, episode)| episode)
        });
        erika_store::Episode {
            dandanplay_id: episode.map_or(0, |episode| episode.episode_id),
            number,
            title: episode.map(|e| e.episode_title.clone()).unwrap_or_default(),
        }
    }

    fn keep_mark(&mut self, id: i64, mark: erika_store::Mark) {
        if self.kept(id).mark != mark {
            self.db.set_mark(&self.anime_of(id), &mark);
            self.kept.entry(id).or_default().mark = mark;
        }
    }

    fn keep_watched(&mut self, id: i64, number: i64, watched: bool, manual: bool) {
        let kept = self.kept(id);
        if kept.watched.contains(&number) == watched && !manual {
            return;
        }
        self.db.set_watched(
            &self.anime_of(id),
            &self.episode_of(id, number),
            watched,
            manual,
        );
        let kept = self.kept.entry(id).or_default();
        if watched {
            kept.watched.insert(number);
        } else {
            kept.watched.remove(&number);
        }
    }

    /// Keep what the user did at once, and sync it with the accounts.
    /// `first` puts the show before the others waiting.
    pub(crate) fn sync(&mut self, id: i64, intents: Vec<Intent>, first: bool) {
        if id == 0 {
            return;
        }
        for intent in &intents {
            match intent {
                Intent::Status(level) => {
                    let mark = erika_store::Mark {
                        status: Some(level.key().into()),
                        ..self.kept(id).mark
                    };
                    self.keep_mark(id, mark);
                }
                Intent::Rate(points) => {
                    let mark = erika_store::Mark {
                        rating: *points,
                        ..self.kept(id).mark
                    };
                    self.keep_mark(id, mark);
                }
                Intent::Mark { number, watched } => self.keep_watched(id, *number, *watched, true),
                Intent::Review(_) => {}
            }
        }
        self.syncs.intents.entry(id).or_default().extend(intents);
        if self.syncs.running != Some(id) {
            self.syncs.waiting.retain(|waiting| *waiting != id);
            if first {
                self.syncs.waiting.push_front(id);
            } else {
                self.syncs.waiting.push_back(id);
            }
        }
        self.pump();
        if self.state.opened == id {
            self.show_opened();
        }
    }

    /// Start the next sync waiting, unless one is under way. A show no
    /// account can take is brought together here at once.
    fn pump(&mut self) {
        while self.syncs.running.is_none()
            && let Some(id) = self.syncs.waiting.pop_front()
        {
            let intents = self.syncs.intents.remove(&id).unwrap_or_default();
            let job = self.job(id, intents);
            if job.reaches() {
                self.syncs.running = Some(id);
                in_background(&self.inbox, move || Some(Done::Synced(run(job))));
            } else {
                let synced = run(job);
                self.keep_synced(synced);
            }
        }
        self.state.syncing = self.syncs.busy(self.state.opened);
    }

    /// A sync came back: keep and show what it came to, and start the next.
    pub(crate) fn take_synced(&mut self, synced: Synced) {
        if self.syncs.running == Some(synced.id) {
            self.syncs.running = None;
        }
        self.keep_synced(synced);
        self.pump();
    }

    fn keep_synced(&mut self, synced: Synced) {
        let id = synced.id;
        let kept = self.kept(id);
        let mark = erika_store::Mark {
            status: synced.level.map(|level| level.key().to_string()),
            rating: synced.rating.unwrap_or(kept.mark.rating),
        };
        self.keep_mark(id, mark);
        for number in synced
            .watched
            .difference(&kept.watched)
            .copied()
            .collect::<Vec<_>>()
        {
            self.keep_watched(id, number, true, false);
        }
        self.syncs.played.insert(id, self.played(id));
        if self.state.opened != id {
            return;
        }
        self.state.sync_note = synced.notes.join("；");
        if let Some(review) = synced.review
            && !self.state.review_saving
        {
            self.state.review_text = review;
        }
        if let Some(reviewed) = synced.reviewed {
            self.state.review_saving = false;
            match reviewed {
                Ok(()) => {
                    self.state.review_open = false;
                    self.state.review_error.clear();
                }
                Err(error) => self.state.review_error = error,
            }
        }
        self.show_opened();
    }

    /// Sync the shows being watched whose episodes played to the end
    /// changed since they were last synced, as the app starts and after
    /// each video.
    pub(crate) fn sync_played(&mut self) {
        for watching in self.watching.clone() {
            let id = watching.anime.dandanplay_id;
            let played = self.played(id);
            if played.is_empty() || self.syncs.played.get(&id) == Some(&played) {
                continue;
            }
            self.syncs.played.insert(id, played);
            self.sync(id, Vec::new(), false);
        }
    }

    /// An account signed in: what was kept while it was out goes to it.
    pub(crate) fn sync_all_again(&mut self) {
        self.syncs.played.clear();
        self.syncs.opened = 0;
        self.sync_opened();
        self.sync_played();
    }

    /// Sync the open show as it opens, once its episodes are known.
    pub(crate) fn sync_opened(&mut self) {
        let id = self.state.opened;
        if id != 0 && self.syncs.opened != id && self.details.contains_key(&id) {
            self.syncs.opened = id;
            self.sync(id, Vec::new(), true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(accounts: Accounts, id: i64, subject: Option<i64>) -> Job {
        Job {
            id,
            accounts,
            subject,
            local: Side::default(),
            episodes: (1..=12).map(|n| (n, id * 10_000 + n)).collect(),
            on_air: false,
            intents: Vec::new(),
        }
    }

    #[test]
    fn only_accounts_that_know_the_show_take_part() {
        let both = Accounts {
            dandanplay: true,
            bangumi: true,
        };
        assert!(job(both, 17675, Some(393010)).reaches());
        assert!(job(both, 17675, None).reaches());
        let bangumi_only = erika_dandanplay::bangumi::local_id(393010).unwrap();
        let only = Accounts {
            dandanplay: true,
            bangumi: false,
        };
        assert!(!job(only, bangumi_only, Some(393010)).reaches());
        assert!(!job(only, -5, None).reaches());
        assert!(!job(Accounts::default(), 17675, Some(393010)).reaches());
    }

    #[test]
    fn without_a_reachable_account_intents_stay_here() {
        let mut job = job(Accounts::default(), -5, None);
        job.local.watched.insert(2);
        job.intents = vec![
            Intent::Status(Level::Want),
            Intent::Mark {
                number: 1,
                watched: true,
            },
            Intent::Mark {
                number: 2,
                watched: false,
            },
        ];
        let synced = run(job);
        assert_eq!(synced.watched, [1].into());
        assert_eq!(synced.level, Some(Level::Watching));
        assert_eq!(
            synced.notes[0],
            "已有看过的剧集，状态保留为「在看」，没有改成「想看」"
        );
        assert_eq!(synced.reviewed, None);
    }

    #[test]
    fn a_review_without_bangumi_reports_why() {
        let mut job = job(Accounts::default(), 17675, None);
        job.intents = vec![Intent::Review("好看".into()), Intent::Rate(8)];
        let synced = run(job);
        assert!(synced.reviewed.unwrap().is_err());
        assert_eq!(synced.rating, Some(8));
        assert_eq!(synced.level, Some(Level::Want));
        assert_eq!(synced.review.as_deref(), Some("好看"));
    }
}
