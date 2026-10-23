//! Container chapters drive both navigation and named OP/ED skip intervals.
//! No chapter or ambiguous title means no fabricated marker or skip target.
use super::*;
use crate::kernel::Chapter;

#[derive(Default)]
pub(super) struct Timeline {
    source: Vec<Chapter>,
    duration: f64,
}

/// Explicit names only: “credits”/“intro” and anonymous chapter positions
/// may contain story, so they are not enough to offer a skip.
fn kind(title: &str) -> SkipKind {
    let lower = title.trim().to_lowercase();
    if ["mixed", "混合", "插入", "正片"]
        .iter()
        .any(|word| lower.contains(word))
    {
        return SkipKind::None;
    }
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let token = |key: &str| {
        words.iter().any(|word| {
            *word == key
                || word.strip_prefix(key).is_some_and(|rest| {
                    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
                })
        })
    };
    let opening = token("op")
        || token("opening")
        || ["片头", "片頭", "オープニング"]
            .iter()
            .any(|word| lower.contains(word));
    let ending = token("ed")
        || token("ending")
        || ["片尾", "エンディング"]
            .iter()
            .any(|word| lower.contains(word));
    match (opening, ending) {
        (true, false) => SkipKind::Opening,
        (false, true) => SkipKind::Ending,
        _ => SkipKind::None,
    }
}

fn chapters(source: &[Chapter], duration: f64) -> Vec<MediaChapter> {
    let mut source: Vec<_> = source
        .iter()
        .filter(|chapter| {
            chapter.start.is_finite()
                && chapter.start >= 0.0
                && (duration <= 0.0 || chapter.start < duration)
        })
        .collect();
    source.sort_by(|a, b| a.start.total_cmp(&b.start));
    source.dedup_by(|a, b| a.start == b.start);
    source
        .iter()
        .enumerate()
        .map(|(index, chapter)| {
            let next = source.get(index + 1).map(|chapter| chapter.start);
            let end = chapter
                .end
                .filter(|end| end.is_finite() && *end > chapter.start)
                .or(next)
                .or((duration > chapter.start).then_some(duration));
            let end = end
                .map(|end| next.map_or(end, |next| end.min(next)))
                .map(|end| {
                    if duration > 0.0 {
                        end.min(duration)
                    } else {
                        end
                    }
                })
                .unwrap_or(0.0);
            let at = if duration > 0.0 {
                chapter.start / duration
            } else {
                0.0
            };
            let next_at = if duration > 0.0 {
                next.unwrap_or(duration) / duration
            } else {
                0.0
            };
            MediaChapter {
                id: index as i64,
                // A language-neutral fallback; never label unnamed chapters OP/ED.
                title: if chapter.title.trim().is_empty() {
                    format!("#{}", index + 1)
                } else {
                    chapter.title.trim().to_string()
                },
                start: chapter.start,
                end,
                at,
                label_width: ((next_at - at) * 1074.0 - 6.0).clamp(0.0, 180.0),
                kind: kind(&chapter.title),
            }
        })
        .collect()
}

impl ErikaEngine {
    pub(super) fn clear_chapters(&mut self) {
        self.timeline = Timeline::default();
        self.state.player.chapters.clear();
        self.state.player.has_chapters = false;
        self.state.player.current_chapter = -1;
        self.state.player.skip_kind = SkipKind::None;
    }

    pub(super) fn refresh_chapters(&mut self, source: &[Chapter]) {
        let duration = self.state.duration;
        if self.timeline.source == source && self.timeline.duration == duration {
            return;
        }
        self.state.player.chapters = chapters(source, duration);
        self.state.player.has_chapters = !self.state.player.chapters.is_empty();
        self.timeline = Timeline {
            source: source.to_vec(),
            duration,
        };
    }

    pub(super) fn update_chapter_position(&mut self) {
        let player = &mut self.state.player;
        let position = self.state.elapsed;
        let chapter = player
            .chapters
            .iter()
            .rev()
            .find(|chapter| chapter.start <= position);
        player.current_chapter = chapter.map_or(-1, |chapter| chapter.id);
        player.skip_kind = if self.state.video && player.skip_intro {
            chapter
                .filter(|chapter| chapter.end > position && chapter.end > chapter.start)
                // Like NipaPlay, stay 500 ms short of EOF; don't offer a
                // button whose clamped target would seek backwards.
                .filter(|chapter| {
                    self.state.duration <= 0.0
                        || chapter.end.min((self.state.duration - 0.5).max(0.0)) > position
                })
                .map_or(SkipKind::None, |chapter| chapter.kind.clone())
        } else {
            SkipKind::None
        };
    }

    pub(super) fn jump_to_chapter(&mut self, id: i64) -> Result<(), CommandError> {
        self.require_playing()?;
        let chapter = self
            .state
            .player
            .chapters
            .iter()
            .find(|chapter| chapter.id == id)
            .ok_or_else(|| CommandError::new("no_such_chapter", "the media has no such chapter"))?;
        self.update_position(chapter.start);
        Ok(())
    }

    pub(super) fn skip_current_segment(&mut self) -> Result<(), CommandError> {
        self.require_playing()?;
        self.update_chapter_position();
        if self.state.player.skip_kind == SkipKind::None {
            return Ok(());
        }
        if let Some(chapter) = self
            .state
            .player
            .chapters
            .iter()
            .find(|chapter| chapter.id == self.state.player.current_chapter)
        {
            let target = if self.state.duration > 0.0 {
                chapter.end.min((self.state.duration - 0.5).max(0.0))
            } else {
                chapter.end
            };
            self.update_position(target);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_unmixed_op_ed_titles_are_skippable() {
        for title in [
            "OP",
            "01 - Opening",
            "OP2「曲名」",
            "片头曲",
            "オープニング",
        ] {
            assert_eq!(kind(title), SkipKind::Opening, "{title}");
        }
        for title in ["ED", "Ending", "片尾曲", "エンディング"] {
            assert_eq!(kind(title), SkipKind::Ending, "{title}");
        }
        for title in [
            "",
            "Chapter 2",
            "Part A",
            "Episode",
            "Credits",
            "mixed-op",
            "OP/ED",
            "正片含ED",
            "Stopped",
            "Edited",
        ] {
            assert_eq!(kind(title), SkipKind::None, "{title}");
        }
    }

    #[test]
    fn chapters_sort_validate_and_bound_intervals_to_next_start_and_duration() {
        let source = vec![
            Chapter {
                title: "ED".into(),
                start: 90.0,
                end: Some(120.0),
            },
            Chapter {
                title: "".into(),
                start: 0.0,
                end: None,
            },
            Chapter {
                title: "OP".into(),
                start: 10.0,
                end: Some(50.0),
            },
            Chapter {
                title: "Part A".into(),
                start: 30.0,
                end: None,
            },
            Chapter {
                title: "invalid".into(),
                start: f64::NAN,
                end: None,
            },
            Chapter {
                title: "past EOF".into(),
                start: 110.0,
                end: None,
            },
        ];
        let list = chapters(&source, 100.0);
        assert_eq!(list.len(), 4);
        assert_eq!(list[0].title, "#1");
        assert_eq!((list[1].start, list[1].end, list[1].at), (10.0, 30.0, 0.1));
        assert_eq!(list[3].end, 100.0);
        assert!(chapters(&[], 100.0).is_empty());
        let unknown_duration = chapters(
            &[Chapter {
                title: "ED".into(),
                start: 90.0,
                end: None,
            }],
            0.0,
        );
        assert_eq!(unknown_duration[0].end, 0.0);
    }
}
