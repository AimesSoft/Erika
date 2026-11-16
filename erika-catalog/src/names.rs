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
static SEASON: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)(?:",
        r"\bS(?:eason)?[ ._-]*(\d{1,3})\b",
        r"|第\s*([0-9０-９一二三四五六七八九十]{1,3})\s*[季期]",
        r"|\b(\d{1,2})(?:st|nd|rd|th)\s*Season\b",
        r")",
    ))
});
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
            hints.season = (1..=3)
                .find_map(|i| c.get(i))
                .and_then(|m| small_number(m.as_str()));
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

static SEQUEL_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)(?:",
        r"\s*第\s*([0-9０-９一二三四五六七八九十]{1,3})\s*(?:[季期]|部分?)",
        r"|\s*([0-9０-９]{1,2})\s*期",
        r"|\s*\b(?:Season|S)\s*([0-9]{1,2})",
        r"|\s*([0-9]{1,2})(?:st|nd|rd|th)\s*(?:Season|シーズン)",
        r"|\s*第\s*[0-9]{1,2}\s*クール",
        r"|\s*(?:最终|完结|最終|完結)季|\s*\bFinal\s*Season|\s*\bPart\.?\s*[0-9]{1,2}",
        r"|\s*[(（](?:19|20)[0-9]{2}[)）]",
        r"|\s*(?:续篇|續篇|续集|續集|新作动画|新作動畫|新作アニメ)",
        r"|\s*[～~〜][^～~〜]+[～~〜]",
        r")\s*$"
    ))
});

fn small_number(text: &str) -> Option<u32> {
    let digits: String = text
        .chars()
        .map(|c| match c {
            '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap_or(c),
            _ => c,
        })
        .collect();
    if let Ok(n) = digits.parse() {
        return Some(n);
    }
    let value = |c| {
        "一二三四五六七八九"
            .chars()
            .position(|d| d == c)
            .map(|i| i as u32 + 1)
    };
    let chars: Vec<char> = digits.chars().collect();
    match chars.as_slice() {
        ['十'] => Some(10),
        [one] => value(*one),
        ['十', one] => Some(10 + value(*one)?),
        [tens, '十'] => Some(value(*tens)? * 10),
        [tens, '十', one] => Some(value(*tens)? * 10 + value(*one)?),
        _ => None,
    }
}

/// The work name under a sequel or arc label, as catalogues name a TV series
/// whose later seasons air under labels such as 「第二季」「续篇」「2nd Season」.
/// The season number is only a label; it never addresses a catalogue season.
pub fn sequel_base(title: &str) -> Option<(String, Option<u32>)> {
    let mut base = title.trim().to_string();
    let mut season = None;
    for _ in 0..3 {
        let Some(c) = SEQUEL_SUFFIX.captures(&base) else {
            break;
        };
        if season.is_none() {
            season = (1..=4)
                .find_map(|i| c.get(i))
                .and_then(|m| small_number(m.as_str()));
        }
        base.truncate(c.get(0).unwrap().start());
        base = base
            .trim_end_matches([' ', '　', '-', ':', '：', '·'])
            .to_string();
    }
    (!base.is_empty() && base != title.trim()).then_some((base, season))
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
    fn sequel_labels_leave_the_work_name() {
        for (title, base, season) in [
            ("青之芦苇 第二季", "青之芦苇", Some(2)),
            (
                "野生的大魔王出现了！第二季",
                "野生的大魔王出现了！",
                Some(2),
            ),
            ("归还者的魔法要特别 第2季", "归还者的魔法要特别", Some(2)),
            ("转生成为魔剑 第二期", "转生成为魔剑", Some(2)),
            ("药屋少女的呢喃 续篇", "药屋少女的呢喃", None),
            ("魔法骑士 新作动画", "魔法骑士", None),
            ("Ao Ashi Season 2", "Ao Ashi", Some(2)),
            (
                "Kikansha no Mahou wa Tokubetsu Desu (2026)",
                "Kikansha no Mahou wa Tokubetsu Desu",
                None,
            ),
            ("Show 2nd Season", "Show", Some(2)),
            ("アオアシ 第2期", "アオアシ", Some(2)),
            ("桃源暗鬼 ～日光・华严之泷篇～", "桃源暗鬼", None),
            ("某作 第十二季", "某作", Some(12)),
            ("钻石王牌 act2 第二季 第2部分", "钻石王牌 act2", Some(2)),
            ("一念永恒 完结季", "一念永恒", None),
        ] {
            assert_eq!(sequel_base(title), Some((base.into(), season)), "{title}");
        }
        for title in [
            "罗梅莉亚战记",
            "赛博朋克：边缘行者 2",
            "第2季",
            "Re:ZERO",
            "Ghosts 2",
            "Counterpart 2",
        ] {
            assert_eq!(sequel_base(title), None, "{title}");
        }
    }
    #[test]
    fn file_position_wins_over_directory() {
        let h = with_directory("Show S00E01.mkv", "Show Season 2");
        assert_eq!(h.season, Some(0));
        assert_eq!(
            with_directory("Show - 03.mkv", "Show Season 2").season,
            Some(2)
        );
        let h = parse("[ANi] 侦探已经死了 第二季 - 01 [1080P].mp4");
        assert_eq!(h.season, Some(2));
        assert_eq!(h.episodes, [1]);
        assert_eq!(h.titles[0], "侦探已经死了");
        let h = with_directory("[LoliHouse] Show - 04 [WebRip].mkv", "青之芦苇 第二季");
        assert_eq!(h.season, Some(2));
        assert!(h.titles.iter().any(|t| t == "青之芦苇"));
    }
}
