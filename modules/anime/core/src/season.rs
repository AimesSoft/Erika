//! The week of the season's anime, starting today, and today's timeline.
//! Each anime of dandanplay's list airs at its bangumi-data slot when one
//! is found; otherwise only its day is known, from dandanplay, and it is
//! listed after the timed ones of that day and kept off the timeline.

use crate::clock::{Clock, date_of, days_of};
use crate::schedule::Schedule;
use crate::timeline::{self, Timeline};
use bern::date::CalendarDate;
use erika_anime_protocol::anime::{Broadcast, BroadcastDay};
use erika_dandanplay::Intro;

pub struct Season {
    pub today: CalendarDate,
    pub label: String,
    pub week: Vec<BroadcastDay>,
    pub timeline: Timeline,
    pub now: i64,
    pub aired: i64,
    pub later: i64,
}

/// Where an anime falls in the week: its weekday (0 Monday to 6 Sunday),
/// and, when its slot is known, its hour in the broadcast day, the day of
/// its first broadcast and the days between broadcasts.
struct Placed<'a> {
    intro: &'a Intro,
    weekday: usize,
    slot: Option<(f64, i64, i64)>,
}

fn seconds(hour: f64) -> i64 {
    (hour * 3600.0).round() as i64 % 86_400
}

/// The season of `date` as the schedule's heading writes it.
fn label(date: CalendarDate) -> String {
    let (name, first_month) = match date.month {
        1..=3 => ("冬季", 1),
        4..=6 => ("春季", 4),
        7..=9 => ("夏季", 7),
        _ => ("秋季", 10),
    };
    let start = days_of(CalendarDate::new(date.year, first_month, 1));
    let week = (days_of(date) - start) / 7 + 1;
    format!(
        "{} 年 {} 月 · {name}新番 · 第 {week} 周",
        date.year, first_month
    )
}

pub fn build(intros: &[Intro], schedule: &Schedule, clock: &Clock) -> Season {
    let (today, now_hour) = clock.today();
    let today_days = days_of(today);
    let placed: Vec<Placed> = intros
        .iter()
        .filter(|intro| !intro.is_restricted)
        .filter_map(
            |intro| match schedule.find(&intro.anime_title, intro.air_day) {
                Some(slot) => {
                    let (day, hour) = clock.broadcast(slot.first);
                    Some(Placed {
                        intro,
                        weekday: day.weekday(),
                        slot: Some((hour, days_of(day), (slot.period / 86_400).max(1))),
                    })
                }
                // dandanplay counts from Sunday.
                None => intro.air_day.map(|day| Placed {
                    intro,
                    weekday: ((day + 6) % 7) as usize,
                    slot: None,
                }),
            },
        )
        .collect();
    let broadcast = |intro: &Intro, hour: Option<f64>, episode: i64, offset: i64| Broadcast {
        id: intro.anime_id,
        title: intro.anime_title.clone(),
        poster: crate::posters::source(&intro.image_url),
        time: hour.map_or(0, seconds),
        timed: hour.is_some(),
        episode,
        aired: offset == 0 && hour.is_some_and(|hour| hour <= now_hour),
        tonight: offset == 0,
    };
    let week: Vec<BroadcastDay> = (0..7)
        .map(|offset| {
            let days = today_days + offset;
            let date = date_of(days);
            let weekday = date.weekday();
            let mut timed: Vec<(f64, Broadcast)> = Vec::new();
            let mut untimed: Vec<(f64, Broadcast)> = Vec::new();
            for place in &placed {
                match place.slot {
                    Some((hour, first, period)) => {
                        let airs = days >= first
                            && (period == 1
                                || (place.weekday == weekday && (days - first) % period == 0));
                        if airs {
                            timed.push((
                                hour,
                                broadcast(
                                    place.intro,
                                    Some(hour),
                                    1 + (days - first) / period,
                                    offset,
                                ),
                            ));
                        }
                    }
                    None if place.weekday == weekday => {
                        untimed.push((place.intro.rating, broadcast(place.intro, None, 0, offset)));
                    }
                    None => {}
                }
            }
            timed.sort_by(|a, b| a.0.total_cmp(&b.0));
            untimed.sort_by(|a, b| b.0.total_cmp(&a.0));
            let shows: Vec<Broadcast> = timed
                .into_iter()
                .chain(untimed)
                .map(|(_, show)| show)
                .collect();
            BroadcastDay {
                id: offset,
                date,
                count: shows.len() as i64,
                today: offset == 0,
                tomorrow: offset == 1,
                day_after: offset == 2,
                preview: shows.iter().take(4).cloned().collect(),
                remaining: shows.len().saturating_sub(4) as i64,
                shows,
            }
        })
        .collect();
    // Today's timed shows, in the broadcast day's hours.
    let today_hours: Vec<(f64, &Broadcast)> = week[0]
        .shows
        .iter()
        .filter(|show| show.timed)
        .map(|show| {
            let hour = show.time as f64 / 3600.0;
            (if hour < 5.0 { hour + 24.0 } else { hour }, show)
        })
        .collect();
    let aired = today_hours.iter().filter(|(_, show)| show.aired).count() as i64;
    let later = today_hours.len() as i64 - aired;
    let timeline = timeline::lay_out(&today_hours, now_hour);
    Season {
        today,
        label: label(today),
        now: clock.seconds(),
        later,
        aired,
        timeline,
        week,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::parse_utc;
    use crate::schedule::Slot;

    fn intro(id: i64, title: &str, air_day: Option<i64>, rating: f64) -> Intro {
        serde_json::from_value(serde_json::json!({
            "animeId": id, "animeTitle": title, "imageUrl": format!("https://img/{id}.jpg"),
            "airDay": air_day, "rating": rating,
        }))
        .unwrap()
    }

    #[test]
    fn the_week_starts_today_with_timed_shows_first_and_the_small_hours_the_evening_before() {
        // Wednesday 2026-10-21, 22:40 in Beijing.
        let clock = Clock {
            unix: parse_utc("2026-10-21T14:40:00Z").unwrap(),
            offset: 8 * 3600,
        };
        let slot = |title: &str, first: &str| Slot {
            titles: vec![title.into()],
            first: parse_utc(first).unwrap(),
            period: 7 * 86_400,
        };
        let schedule = Schedule::new(
            0,
            vec![
                // Wednesdays at 22:00 and 01:30 (Thursday morning), from three weeks ago.
                slot("早", "2026-09-30T14:00:00Z"),
                slot("晚", "2026-09-30T17:30:00Z"),
                // A Wednesday at 23:30 starting next week.
                slot("新", "2026-10-28T15:30:00Z"),
            ],
        );
        // dandanplay's own days: Wednesday is 3, counted from Sunday.
        let intros = vec![
            intro(1, "早", Some(3), 7.0),
            intro(2, "晚", Some(4), 8.0),
            intro(3, "新", Some(3), 6.0),
            intro(4, "没时刻", Some(3), 9.0),
        ];
        let season = build(&intros, &schedule, &clock);
        assert_eq!(season.today, CalendarDate::new(2026, 10, 21));
        assert_eq!(season.label, "2026 年 10 月 · 秋季新番 · 第 3 周");
        let today = &season.week[0];
        assert_eq!(
            today
                .shows
                .iter()
                .map(|show| show.title.as_str())
                .collect::<Vec<_>>(),
            ["早", "晚", "没时刻"]
        );
        assert_eq!(
            (
                today.shows[0].time,
                today.shows[0].episode,
                today.shows[0].aired
            ),
            (22 * 3600, 4, true)
        );
        assert_eq!(
            (today.shows[1].time, today.shows[1].aired),
            (90 * 60, false)
        );
        assert!(!today.shows[2].timed);
        // Next Wednesday the new one joins.
        assert_eq!(season.week[0].date.weekday(), season.week[0].date.weekday());
        assert!(season.week.iter().all(|day| day.id != 0 || day.today));
        assert!(season.week[6].shows.is_empty() || season.week[6].date.weekday() != 2);
        // The timeline holds the timed ones, now between them.
        let timeline = &season.timeline;
        assert_eq!(timeline.groups.len(), 2);
        assert_eq!((season.aired, season.later), (1, 1));
        assert!(timeline.groups[0].at < timeline.now_at && timeline.now_at < timeline.groups[1].at);
    }
}
