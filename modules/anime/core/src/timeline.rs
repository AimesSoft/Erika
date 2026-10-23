//! Today's timeline, laid out by time rather than to scale. Shows airing at
//! the same minute stand side by side as one group under one time; the
//! space between groups grows with the time between them, within bounds,
//! so a crowded evening spreads out and an empty afternoon shrinks to a
//! break. The track is as long as that takes and at least `FLOOR` points;
//! what a short day leaves over is shared out by time, so it still reads as
//! a day. The view scrolls a longer track sideways.
//!
//! Bern's expressions have no arithmetic, so the places are worked out here
//! in points and handed over as fractions of the track.

use erika_anime_protocol::anime::{Airing, AiringGroup, Broadcast, TimelineBreak};

/// A poster's width and the space between the posters of a group, as the
/// view draws them.
const CARD: f64 = 88.0;
const CARD_GAP: f64 = 8.0;
/// The space between groups: a point a minute, from 16 to 64 points.
const GAP_PER_MINUTE: f64 = 1.0;
const GAP_MIN: f64 = 16.0;
const GAP_MAX: f64 = 64.0;
/// Groups further apart than this have a break drawn between them, a mark
/// about this wide centred in the space.
const BREAK_HOURS: f64 = 2.0;
const BREAK_MARK: f64 = 12.0;
/// Room for "now" before the first group, and after the last for the bar
/// and the words beside it.
const NOW_BEFORE: f64 = 24.0;
const NOW_AFTER: f64 = 88.0;
/// How far the bar of "now" keeps from the posters, and its width.
const NOW_CLEARANCE: f64 = 6.0;
const NOW_BAR: f64 = 2.0;
/// The shortest track: the width of the phone's scrolling timeline in the
/// design, and a wide window's.
pub const FLOOR: f64 = 900.0;
pub const FLOOR_WIDE: f64 = 1120.0;
/// How much of the time between groups counts when space left over is
/// shared out, so an empty afternoon takes no more than two hours' share,
/// and the time before the first or after the last no more than one.
const SHARE_HOURS: f64 = 2.0;
const SHARE_EDGE_HOURS: f64 = 1.0;

pub struct Timeline {
    pub groups: Vec<AiringGroup>,
    pub breaks: Vec<TimelineBreak>,
    /// Where "now" falls, from 0 to 1.
    pub now_at: f64,
    /// The track's length in points, and on a wide window.
    pub width: f64,
    pub wide: f64,
}

/// One stretch of space on the track: its length, and its share of what is
/// left over.
struct Space {
    length: f64,
    share: f64,
}

/// Lay out `shows`, today's timed broadcasts in order with their hours in
/// the broadcast day (5 to 29), with "now" at `now_hour`.
pub fn lay_out(shows: &[(f64, &Broadcast)], now_hour: f64) -> Timeline {
    let mut groups: Vec<(f64, AiringGroup)> = Vec::new();
    for (hour, show) in shows {
        let airing = Airing {
            id: show.id,
            title: show.title.clone(),
            poster: show.poster.clone(),
            aired: show.aired,
        };
        match groups.last_mut() {
            Some((_, group)) if group.time == show.time => {
                group.aired &= show.aired;
                group.shows.push(airing);
            }
            _ => groups.push((
                *hour,
                AiringGroup {
                    time: show.time,
                    aired: show.aired,
                    shows: vec![airing],
                    width: 0.0,
                    at: 0.0,
                    span: 0.0,
                },
            )),
        }
    }
    for (_, group) in &mut groups {
        let count = group.shows.len() as f64;
        group.width = count * CARD + (count - 1.0) * CARD_GAP;
        group.span = group.width - CARD;
    }
    let (Some(&(first, _)), Some(&(last, _))) = (groups.first(), groups.last()) else {
        return Timeline {
            groups: Vec::new(),
            breaks: Vec::new(),
            now_at: 0.0,
            width: FLOOR,
            wide: FLOOR_WIDE,
        };
    };

    // The space before each group, then after the last.
    let mut spaces: Vec<Space> = Vec::with_capacity(groups.len() + 1);
    spaces.push(if now_hour < first {
        Space {
            length: NOW_BEFORE,
            share: (first - now_hour).min(SHARE_EDGE_HOURS),
        }
    } else {
        Space {
            length: 0.0,
            share: 0.0,
        }
    });
    for pair in groups.windows(2) {
        let hours = pair[1].0 - pair[0].0;
        spaces.push(Space {
            length: (hours * 60.0 * GAP_PER_MINUTE).clamp(GAP_MIN, GAP_MAX),
            share: hours.min(SHARE_HOURS),
        });
    }
    spaces.push(if now_hour >= last {
        Space {
            length: NOW_AFTER,
            share: (now_hour - last).min(SHARE_EDGE_HOURS),
        }
    } else {
        Space {
            length: 0.0,
            share: 0.0,
        }
    });

    let natural: f64 = groups.iter().map(|(_, group)| group.width).sum::<f64>()
        + spaces.iter().map(|space| space.length).sum::<f64>();
    let width = natural.max(FLOOR);
    let left_over = width - natural;
    let shares: f64 = spaces.iter().map(|space| space.share).sum();
    if shares > 0.0 {
        for space in &mut spaces {
            space.length += left_over * space.share / shares;
        }
    } else if let Some(end) = spaces.last_mut() {
        end.length += left_over;
    }

    // Each group's leading edge, and "now" in the space it falls in.
    let mut x = spaces[0].length;
    let mut now = (now_hour < first).then(|| {
        let room = (spaces[0].length - NOW_CLEARANCE - NOW_BAR).max(0.0);
        room * (1.0 - (first - now_hour).min(SHARE_EDGE_HOURS) / SHARE_EDGE_HOURS)
    });
    let mut breaks = Vec::new();
    for index in 0..groups.len() {
        let (hour, group) = &mut groups[index];
        group.at = x / width;
        x += group.width;
        let (hour, time) = (*hour, group.time);
        let after = &spaces[index + 1];
        match groups.get(index + 1) {
            Some(&(next, _)) => {
                if now.is_none() && now_hour >= hour && now_hour < next {
                    let room = (after.length - 2.0 * NOW_CLEARANCE - NOW_BAR).max(0.0);
                    now = Some(x + NOW_CLEARANCE + room * (now_hour - hour) / (next - hour));
                }
                if next - hour > BREAK_HOURS {
                    breaks.push(TimelineBreak {
                        after: time,
                        at: (x + (after.length - BREAK_MARK) / 2.0) / width,
                    });
                }
            }
            None if now.is_none() => {
                let room = (after.length - NOW_AFTER).max(0.0);
                now = Some(
                    x + NOW_CLEARANCE
                        + room * (now_hour - hour).min(SHARE_EDGE_HOURS) / SHARE_EDGE_HOURS,
                );
            }
            None => {}
        }
        x += after.length;
    }
    Timeline {
        groups: groups.into_iter().map(|(_, group)| group).collect(),
        breaks,
        now_at: now.unwrap_or(0.0) / width,
        width,
        wide: width.max(FLOOR_WIDE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bern::media::MediaSource;

    fn show(id: i64, hour: f64, now: f64) -> (f64, Broadcast) {
        let time = ((hour * 3600.0).round() as i64) % 86_400;
        (
            hour,
            Broadcast {
                id,
                title: format!("{id}"),
                poster: MediaSource::new(""),
                time,
                timed: true,
                episode: 1,
                aired: hour <= now,
                tonight: true,
            },
        )
    }

    fn lay(hours: &[f64], now: f64) -> Timeline {
        let shows: Vec<(f64, Broadcast)> = hours
            .iter()
            .enumerate()
            .map(|(id, hour)| show(id as i64, *hour, now))
            .collect();
        let shows: Vec<(f64, &Broadcast)> =
            shows.iter().map(|(hour, show)| (*hour, show)).collect();
        lay_out(&shows, now)
    }

    /// The groups' edges in points, leading and trailing.
    fn edges(timeline: &Timeline) -> Vec<(f64, f64)> {
        timeline
            .groups
            .iter()
            .map(|group| {
                (
                    group.at * timeline.width,
                    group.at * timeline.width + group.width,
                )
            })
            .collect()
    }

    #[test]
    fn a_crowded_saturday_scrolls_with_nothing_overlapping() {
        // Saturday's fifteen shows of the season, 22:40 now.
        let hours = [
            8.5, 15.48, 17.42, 21.0, 21.5, 22.0, 22.42, 22.5, 22.5, 22.75, 23.0, 23.5, 23.92, 24.0,
            24.5,
        ];
        let timeline = lay(&hours, 22.67);
        assert_eq!(timeline.groups.len(), 14);
        assert_eq!(timeline.groups[7].shows.len(), 2);
        assert_eq!(
            (timeline.groups[7].width, timeline.groups[7].span),
            (184.0, 96.0)
        );
        assert_eq!(timeline.groups[6].span, 0.0);
        assert!(timeline.width > FLOOR);
        let edges = edges(&timeline);
        assert!(
            edges
                .windows(2)
                .all(|pair| pair[1].0 - pair[0].1 >= GAP_MIN - 1e-9)
        );
        // Two breaks: after the morning show, and after 17:25.
        assert_eq!(timeline.breaks.len(), 2);
        // "Now" between 22:30 and 22:45, clear of both.
        let now = timeline.now_at * timeline.width;
        assert!(
            now >= edges[7].1 + NOW_CLEARANCE - 1e-9
                && now + NOW_BAR <= edges[8].0 - NOW_CLEARANCE + 1e-9
        );
        assert!(timeline.groups[7].aired && !timeline.groups[8].aired);
    }

    #[test]
    fn a_quiet_day_fills_the_floor_by_time() {
        let timeline = lay(&[21.0, 21.0, 21.5, 23.5, 24.5], 22.67);
        assert_eq!(timeline.width, FLOOR);
        assert_eq!(timeline.wide, FLOOR_WIDE);
        let edges = edges(&timeline);
        // The two hours from 21:30 get more room than the half hour before.
        assert!(edges[2].0 - edges[1].1 > 2.0 * (edges[1].0 - edges[0].1));
        assert!((edges[3].1 - FLOOR).abs() < 1e-6);
        assert!(timeline.breaks.is_empty());
    }

    #[test]
    fn now_before_the_first_or_after_the_last_has_room() {
        let early = lay(&[21.0, 22.0], 10.0);
        assert!(
            early.now_at * early.width + NOW_BAR
                <= early.groups[0].at * early.width - NOW_CLEARANCE + 1e-9
        );
        let late = lay(&[21.0, 22.0], 28.0);
        let last = &late.groups[1];
        let now = late.now_at * late.width;
        assert!(now >= last.at * late.width + last.width + NOW_CLEARANCE - 1e-9);
        assert!(now + NOW_AFTER <= late.width + NOW_CLEARANCE + 1e-9);
    }
}
