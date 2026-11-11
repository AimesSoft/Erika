//! File names provide hints, never database IDs. Zero is a real season;
//! special labels and absolute numbering must survive until mapped explicitly.
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameHints {
    pub titles: Vec<String>,
    pub year: Option<u16>,
    pub season: Option<u32>,
    pub episodes: Vec<u32>,
    pub absolute: Option<u32>,
    pub special_label: Option<String>,
    pub release_group: Option<String>,
    pub version: Option<String>,
    pub evidence: Vec<String>,
}
fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static pattern")
}
static SE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\bS(\d{1,3})[ ._-]*E(\d{1,4})(?:[ ._]*(E|-E?)(\d{1,4}))?\b"));
static SX: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(\d{1,3})x(\d{1,4})(?:-(\d{1,4}))?\b"));
static SEASON: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)(?:\bS(?:eason)?[ ._-]*(\d{1,3})\b|第\s*(\d{1,3})\s*季)"));
static SPECIAL: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)(?:\b(?:SP|OVA|OAD|SPECIAL)[ ._-]*\d*(?:\.\d+)?\b|特别篇|特別篇)"));
static EPISODE: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)(?:\bEP?[ ._-]*|第\s*|\s-\s*|\[)(\d{1,4})(?:\.(\d+))?(?:[话話集\]]|\b)(?:v(\d+))?")
});
static LOOSE: LazyLock<Regex> = LazyLock::new(|| re(r"(?:^|\s)(\d{1,4})(?:v\d+)?(?:\s|$)"));
static YEAR: LazyLock<Regex> = LazyLock::new(|| re(r"\b((?:19|20)\d{2})\b"));
static TAGS: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b(?:\d{3,4}[pi]|[xh][ ._-]?26[45]|HEVC|AVC|AV1|WEB[ ._-]?(?:DL|RIP)?|BDRIP|BLURAY|REMUX|AAC|FLAC|DTS|CHS|CHT|GB|BIG5|10BIT|8BIT)\b",
    )
});
static BRACKETS: LazyLock<Regex> = LazyLock::new(|| re(r"\[([^\]]*)\]|【([^】]*)】"));

pub fn parse(name: &str) -> NameHints {
    // Paths are used locally only. Network callers send their basename.
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = name
        .rsplit_once('.')
        .filter(|(_, ext)| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "mkv"
                    | "mp4"
                    | "avi"
                    | "mov"
                    | "ts"
                    | "m2ts"
                    | "webm"
                    | "m4v"
                    | "wmv"
                    | "flv"
                    | "mpeg"
                    | "mpg"
            )
        })
        .map_or(name, |(stem, _)| stem);
    // Underscores separate release tokens just like spaces; regex word
    // boundaries otherwise treat Show_S01E01_1080p as one word.
    let normalized = stem.replace('_', " ");
    let stem = normalized.as_str();
    let mut hints = NameHints::default();
    hints.year = YEAR.captures(stem).and_then(|c| c[1].parse().ok());
    let mut cut = stem.len();
    let se = SE.captures(stem);
    let is_se = se.is_some();
    if let Some(c) = se.or_else(|| SX.captures(stem)) {
        hints.season = c[1].parse().ok();
        let start: u32 = c[2].parse().unwrap_or(0);
        let end = c
            .get(if is_se { 4 } else { 3 })
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(start);
        if is_se
            && c.get(3)
                .is_some_and(|m| m.as_str().eq_ignore_ascii_case("e"))
        {
            hints.episodes.push(start);
            if end != start {
                hints.episodes.push(end);
            }
        } else if end >= start && end - start <= 99 {
            hints.episodes = (start..=end).collect();
        }
        hints.evidence.push(c[0].to_string());
        cut = c.get(0).unwrap().start();
    } else {
        if let Some(c) = SEASON.captures(stem) {
            hints.season = c
                .get(1)
                .or_else(|| c.get(2))
                .and_then(|m| m.as_str().parse().ok());
            hints.evidence.push(c[0].to_string());
            cut = cut.min(c.get(0).unwrap().start());
        }
        if let Some(c) = EPISODE.captures(stem) {
            let n: u32 = c[1].parse().unwrap_or(0);
            if c.get(2).is_some() {
                hints.special_label = Some(c[0].to_string());
            } else if !(1900..2100).contains(&n) {
                hints.episodes.push(n);
                hints.absolute = Some(n);
            }
            hints.version = c.get(3).map(|m| m.as_str().to_string());
            hints.evidence.push(c[0].to_string());
            cut = cut.min(c.get(0).unwrap().start());
        } else if let Some(c) = LOOSE
            .captures_iter(stem)
            .filter(|c| {
                c[1].parse::<u32>()
                    .is_ok_and(|n| !(1900..2100).contains(&n))
            })
            .last()
        {
            let n = c[1].parse().unwrap();
            hints.absolute = Some(n);
            hints.episodes.push(n);
            hints.evidence.push(c[0].to_string());
            cut = cut.min(c.get(0).unwrap().start());
        }
    }
    if let Some(special) = SPECIAL.find(stem) {
        hints.special_label = Some(special.as_str().into());
        hints.evidence.push(special.as_str().into());
        cut = cut.min(special.start());
        // A special label alone never means season zero, episode one.
        if hints.season.is_none() {
            hints.episodes.clear();
            hints.absolute = None;
        }
    }
    let title = &stem[..cut];
    let mut variants = vec![title.to_string()];
    if title.starts_with('[') || title.starts_with('【') {
        if let Some(first) = BRACKETS.find(title) {
            hints.release_group = Some(first.as_str().trim_matches(['[', ']', '【', '】']).into());
            let rest = title[first.end()..].trim();
            if !rest.is_empty() {
                variants.insert(0, rest.into());
            }
        }
    }
    for variant in variants {
        let clean = TAGS.replace_all(&variant, " ");
        let clean = YEAR.replace_all(&clean, " ");
        let clean = clean.replace(['.', '_', '[', ']', '【', '】', '(', ')', '（', '）'], " ");
        let clean = clean
            .trim_matches([' ', '-'])
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !clean.is_empty() && !hints.titles.contains(&clean) {
            hints.titles.push(clean);
        }
    }
    hints
}

/// Add directory hints without overriding an explicit file position.
pub fn with_directory(name: &str, directory: &str) -> NameHints {
    let mut hints = parse(name);
    let directory = parse(directory);
    if hints.season.is_none() {
        hints.season = directory.season;
    }
    if hints.year.is_none() {
        hints.year = directory.year;
    }
    for title in directory.titles {
        if !hints.titles.contains(&title) {
            hints.titles.push(title);
        }
    }
    hints
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seasons_specials_ranges_and_years_survive() {
        let h = parse("Show.S00E01.1080p.WEB.mkv");
        assert_eq!(h.season, Some(0));
        assert_eq!(h.episodes, vec![1]);
        assert_eq!(h.titles[0], "Show");
        let h = parse("Show (2024) S02E03-E04.mkv");
        assert_eq!(h.year, Some(2024));
        assert_eq!(h.season, Some(2));
        assert_eq!(h.episodes, [3, 4]);
        let h = parse("[Group] Show SP1 1080p.mkv");
        assert_eq!(h.season, None);
        assert!(h.episodes.is_empty());
        assert_eq!(h.special_label.as_deref(), Some("SP1"));
        assert_eq!(parse("电影 (2024).mp4").episodes, Vec::<u32>::new());
        assert_eq!(
            parse("Show - 12.5.mkv").special_label.as_deref(),
            Some(" - 12.5")
        );
    }
    #[test]
    fn release_separators_and_explicit_episode_lists_are_not_ranges() {
        let h = parse("Show_2024_S01E01_1080p.mkv");
        assert_eq!(h.season, Some(1));
        assert_eq!(h.episodes, [1]);
        assert_eq!(h.year, Some(2024));
        assert_eq!(h.titles[0], "Show");
        assert_eq!(parse("Show S01E01E03.mkv").episodes, [1, 3]);
        assert_eq!(parse("Show S01E01-E03.mkv").episodes, [1, 2, 3]);
        assert_eq!(parse("Show_1x01-03.mkv").episodes, [1, 2, 3]);
    }
    #[test]
    fn file_position_wins_over_directory() {
        let h = with_directory("Show S00E01.mkv", "Show Season 2");
        assert_eq!(h.season, Some(0));
        assert_eq!(
            with_directory("Show - 03.mkv", "Show Season 2").season,
            Some(2)
        );
    }
}
