//! When the season's anime air, from bangumi-data (github.com/bangumi-data),
//! which dandanplay's list lacks: each anime's first broadcast and how often
//! it repeats, under its names. Only the shows on air around now are kept,
//! in a small file in the cache folder, fetched again when it is three days
//! old.
//!
//! dandanplay's list names no Bangumi subject (its `bangumiId` is its own
//! id), so an anime is found by name: the same name, or the same name once
//! season numbers, sequel words and brackets are read apart, or a name one
//! starts the other, on the weekday dandanplay gives.

use crate::clock::parse_utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

/// Where bangumi-data is published, in the order tried.
#[cfg(not(target_arch = "wasm32"))]
const SOURCES: &[&str] = &[
    "https://cdn.jsdelivr.net/npm/bangumi-data@0.3/dist/data.json",
    "https://unpkg.com/bangumi-data@0.3/dist/data.json",
];
const CACHE: &str = "bangumi-schedule.json";
/// How old the kept schedule gets before it is fetched again, in seconds.
pub const STALE_AFTER: i64 = 3 * 86_400;

/// When one anime airs each week.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Slot {
    /// Its names: the original and the Simplified Chinese ones.
    pub titles: Vec<String>,
    /// Its first broadcast, in seconds since 1970, and how far apart the
    /// broadcasts are.
    pub first: i64,
    pub period: i64,
}

/// The slots of the shows on air, found by name.
#[derive(Clone, Debug, Default)]
pub struct Schedule {
    pub fetched: i64,
    slots: Vec<Slot>,
    /// Every name of every slot, read apart, with the slot's place.
    names: Vec<(Name, usize)>,
    /// The slot of each name as names are compared.
    exact: HashMap<String, usize>,
}

/// The shortest name a prefix may be: four characters, such as 东京复仇者
/// or 魔法骑士, say enough; fewer could start any name.
const SHORTEST_PREFIX: usize = 4;

impl Schedule {
    pub fn new(fetched: i64, slots: Vec<Slot>) -> Self {
        let mut schedule = Schedule {
            fetched,
            ..Schedule::default()
        };
        for (index, slot) in slots.iter().enumerate() {
            for title in &slot.titles {
                let name = Name::of(title);
                schedule.exact.entry(name.plain.clone()).or_insert(index);
                schedule.names.push((name, index));
            }
        }
        schedule.slots = slots;
        schedule
    }

    /// The slot of the anime named `title`, which dandanplay says airs on
    /// `air_day` in Japan (0 Sunday to 6 Saturday). Looser names count
    /// only when they lead to one slot on that day.
    pub fn find(&self, title: &str, air_day: Option<i64>) -> Option<&Slot> {
        let name = Name::of(title);
        if let Some(&index) = self.exact.get(&name.plain) {
            return Some(&self.slots[index]);
        }
        let on_the_day = |index: usize| {
            air_day.is_none_or(|day| weekday_in_japan(self.slots[index].first) == day)
        };
        let only = |fits: &dyn Fn(&Name) -> bool| {
            let found: BTreeSet<usize> = self
                .names
                .iter()
                .filter(|(other, index)| fits(other) && on_the_day(*index))
                .map(|(_, index)| *index)
                .collect();
            match found.len() {
                1 => found.first().map(|index| &self.slots[*index]),
                _ => None,
            }
        };
        only(&|other| other.base == name.base && other.season == name.season).or_else(|| {
            let long = |base: &str| base.chars().count() >= SHORTEST_PREFIX;
            if !long(&name.base) {
                return None;
            }
            only(&|other| {
                long(&other.base)
                    && (other.base.starts_with(&name.base) || name.base.starts_with(&other.base))
                    && (name.season.is_none()
                        || other.season.is_none()
                        || other.season == name.season)
            })
        })
    }

    /// The schedule kept in the cache folder.
    pub fn cached() -> Option<Schedule> {
        let path = bern::storage::cache_dir()?.join(CACHE);
        let kept: Kept = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        Some(Schedule::new(kept.fetched, kept.slots))
    }
}

/// The weekday, 0 Sunday to 6 Saturday, of a moment in Japan, where
/// dandanplay counts its days.
fn weekday_in_japan(unix: i64) -> i64 {
    // 1970-01-01 was a Thursday.
    ((unix + 9 * 3600).div_euclid(86_400) + 4).rem_euclid(7)
}

/// Words that say a show is a sequel without saying which season.
const SEQUEL_WORDS: &[&str] = &[
    "续篇",
    "續篇",
    "新作动画",
    "新作",
    "最终季",
    "完结季",
    "完结篇",
];
/// What follows 第 and a number to make it a season.
const SEASON_WORDS: &[&str] = &["部分", "期", "季", "部", "クール", "シーズン"];

/// A name read apart for comparing.
#[derive(Clone, Debug, PartialEq)]
struct Name {
    /// Its letters and digits, full-width ones folded and in lower case.
    plain: String,
    /// The same without its season, sequel words and what is in brackets.
    base: String,
    /// The season it names, as 第二季, 第2期, (第3期), Season 2 or 2nd
    /// Season do.
    season: Option<u32>,
}

impl Name {
    fn of(title: &str) -> Name {
        let folded: String = title
            .chars()
            .map(|c| match c {
                '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
                _ => c,
            })
            .flat_map(char::to_lowercase)
            .filter(|c| !c.is_whitespace())
            .collect();
        let plain = folded.chars().filter(|c| c.is_alphanumeric()).collect();
        let (season, rest) = season_of(&folded);
        let mut rest = without_brackets(&rest);
        for word in SEQUEL_WORDS {
            rest = rest.replace(word, "");
        }
        Name {
            plain,
            base: rest.chars().filter(|c| c.is_alphanumeric()).collect(),
            season,
        }
    }
}

/// A number written in digits or in Chinese up to 十九.
fn number(text: &str) -> Option<u32> {
    if let Ok(number) = text.parse() {
        return Some(number);
    }
    let digit = |c: char| "一二三四五六七八九".find(c).map(|at| at as u32 / 3 + 1);
    let chars: Vec<char> = text.chars().collect();
    match chars.as_slice() {
        [one] if *one == '十' => Some(10),
        [one] => digit(*one),
        ['十', one] => Some(10 + digit(*one)?),
        _ => None,
    }
}

/// The first season `name` names, and the name without every season
/// marker in it.
fn season_of(name: &str) -> (Option<u32>, String) {
    let mut season = None;
    let mut rest = String::new();
    let mut text = name;
    'scan: while let Some(c) = text.chars().next() {
        let after = &text[c.len_utf8()..];
        // 第 n 季, 第二期, 第2クール …
        if c == '第' {
            let count: String = after
                .chars()
                .take_while(|c| c.is_ascii_digit() || "一二三四五六七八九十".contains(*c))
                .collect();
            let tail = &after[count.len()..];
            if let (Some(found), Some(word)) = (
                number(&count),
                SEASON_WORDS.iter().find(|word| tail.starts_with(**word)),
            ) {
                season = season.or(Some(found));
                text = &tail[word.len()..];
                continue 'scan;
            }
        }
        // season 2
        if let Some(tail) = text.strip_prefix("season") {
            let count: String = tail.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(found) = count.parse() {
                season = season.or(Some(found));
                text = &tail[count.len()..];
                continue 'scan;
            }
        }
        // 2nd season
        if c.is_ascii_digit() && !rest.ends_with(|c: char| c.is_ascii_digit()) {
            let count: String = text.chars().take_while(char::is_ascii_digit).collect();
            let tail = &text[count.len()..];
            if let Some(tail) = ["st", "nd", "rd", "th"]
                .iter()
                .find_map(|end| tail.strip_prefix(end)?.strip_prefix("season"))
            {
                season = season.or(count.parse().ok());
                text = tail;
                continue 'scan;
            }
        }
        rest.push(c);
        text = after;
    }
    (season, rest)
}

/// `name` without what it holds in round brackets: a year, a part, a cour.
fn without_brackets(name: &str) -> String {
    let mut depth = 0usize;
    name.chars()
        .filter(|c| match c {
            '(' => {
                depth += 1;
                false
            }
            ')' => {
                depth = depth.saturating_sub(1);
                false
            }
            _ => depth == 0,
        })
        .collect()
}

#[derive(Deserialize, Serialize)]
struct Kept {
    fetched: i64,
    slots: Vec<Slot>,
}

/// Keep `slots`, fetched at `fetched`, for the next start.
pub fn keep(fetched: i64, slots: &[Slot]) {
    let Some(dir) = bern::storage::cache_dir() else {
        return;
    };
    let kept = Kept {
        fetched,
        slots: slots.to_vec(),
    };
    if let Ok(bytes) = serde_json::to_vec(&kept) {
        let _ = std::fs::write(dir.join(CACHE), bytes);
    }
}

#[derive(Deserialize)]
struct Data {
    #[serde(default)]
    items: Vec<Item>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    #[serde(default)]
    title: String,
    #[serde(default)]
    title_translate: HashMap<String, Vec<String>>,
    #[serde(default)]
    begin: String,
    #[serde(default)]
    end: String,
    #[serde(default)]
    broadcast: Option<String>,
}

/// The slots of the shows in bangumi-data's `json` that air around `now`:
/// begun within the last year or the next two months and not ended over a
/// month ago, broadcast every day or week.
pub fn parse(json: &str, now: i64) -> Vec<Slot> {
    let Ok(data) = serde_json::from_str::<Data>(json) else {
        return Vec::new();
    };
    data.items
        .into_iter()
        .filter_map(|mut item| {
            let (first, period) = broadcast(item.broadcast.as_deref()?)?;
            let begin = parse_utc(&item.begin).unwrap_or(first);
            let ended = parse_utc(&item.end).is_some_and(|end| end < now - 30 * 86_400);
            if ended || begin < now - 365 * 86_400 || begin > now + 60 * 86_400 {
                return None;
            }
            let mut titles = vec![item.title];
            titles.extend(item.title_translate.remove("zh-Hans").unwrap_or_default());
            Some(Slot {
                titles,
                first,
                period,
            })
        })
        .collect()
}

/// The first broadcast and the period of an ISO 8601 repeating interval
/// such as `R/2023-09-29T14:00:00.000Z/P7D`; weekly and daily ones only.
fn broadcast(text: &str) -> Option<(i64, i64)> {
    let mut parts = text.split('/');
    let (_, start, period) = (parts.next()?, parts.next()?, parts.next()?);
    let days: i64 = period.strip_prefix('P')?.strip_suffix('D')?.parse().ok()?;
    if days != 1 && days != 7 {
        return None;
    }
    Some((parse_utc(start)?, days * 86_400))
}

/// Download bangumi-data from the first source that answers.
#[cfg(not(target_arch = "wasm32"))]
pub fn fetch() -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .into();
    let mut last = String::from("no source");
    for url in SOURCES {
        match agent.get(*url).call() {
            Ok(mut response) => match response
                .body_mut()
                .with_config()
                .limit(64 << 20)
                .read_to_string()
            {
                Ok(text) => return Ok(text),
                Err(error) => last = error.to_string(),
            },
            Err(error) => last = error.to_string(),
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATA: &str = r#"{"items":[
        {"title":"葬送のフリーレン 第2期","titleTranslate":{"zh-Hans":["葬送的芙莉莲 第二季"]},"begin":"2026-10-02T14:00:00.000Z","end":"","broadcast":"R/2026-10-02T14:00:00.000Z/P7D","sites":[{"site":"bangumi","id":"500001"},{"site":"bilibili","id":"1"}]},
        {"title":"Old","begin":"2001-01-01T00:00:00.000Z","end":"2001-03-01T00:00:00.000Z","broadcast":"R/2001-01-01T00:00:00.000Z/P7D","sites":[]},
        {"title":"Film","begin":"2026-10-10T00:00:00.000Z","end":"","broadcast":"R/2026-10-10T00:00:00.000Z/P0D","sites":[]}
    ]}"#;

    #[test]
    fn shows_on_air_are_found_by_name() {
        let now = parse_utc("2026-10-21T00:00:00Z").unwrap();
        let slots = parse(DATA, now);
        assert_eq!(slots.len(), 1);
        let schedule = Schedule::new(now, slots);
        assert_eq!(
            schedule.find("葬送的芙莉莲第二季", None).unwrap().titles[0],
            "葬送のフリーレン 第2期"
        );
        assert!(schedule.find("葬送のフリーレン　第２期", None).is_some());
        assert!(schedule.find("别的番", None).is_none());
    }

    /// Names as dandanplay and bangumi-data write the same shows.
    #[test]
    fn looser_names_find_the_one_show_of_that_day() {
        let slot = |titles: &[&str], first: &str| Slot {
            titles: titles.iter().map(|title| title.to_string()).collect(),
            first: parse_utc(first).unwrap(),
            period: 7 * 86_400,
        };
        // Late Friday, the small hours of Saturday, Sunday and Monday in Japan.
        let schedule = Schedule::new(
            0,
            vec![
                slot(
                    &["転生したら剣でしたⅡ", "转生成为魔剑 第二季"],
                    "2026-10-02T15:00:00Z",
                ),
                slot(
                    &[
                        "帰還者の魔法は特別です(第二期)",
                        "归还者的魔法要特别 第二季",
                    ],
                    "2026-10-02T14:00:00Z",
                ),
                slot(
                    &["薬屋のひとりごと(第3期)", "药屋少女的呢喃 第三季"],
                    "2026-10-02T16:00:00Z",
                ),
                slot(
                    &["東京リベンジャーズ 三天戦争編", "东京复仇者 三天战争篇"],
                    "2026-10-04T09:00:00Z",
                ),
                slot(&["タヌキとキツネ"], "2026-10-04T23:00:00Z"),
            ],
        );
        let title = |name: &str, day: i64| {
            schedule
                .find(name, Some(day))
                .map(|slot| slot.titles[0].as_str())
        };
        assert_eq!(title("转生成为魔剑 第二期", 6), Some("転生したら剣でしたⅡ"));
        assert_eq!(
            title("归还者的魔法要特别 第2季", 5),
            Some("帰還者の魔法は特別です(第二期)")
        );
        assert_eq!(
            title("药屋少女的呢喃 续篇", 6),
            Some("薬屋のひとりごと(第3期)")
        );
        assert_eq!(
            title("东京复仇者 续篇", 0),
            Some("東京リベンジャーズ 三天戦争編")
        );
        assert_eq!(title("タヌキとキツネ (2026)", 1), Some("タヌキとキツネ"));
        // Another weekday, another season or too short a name is another show.
        assert_eq!(title("东京复仇者 续篇", 3), None);
        assert_eq!(title("药屋少女的呢喃 第二季", 6), None);
        assert_eq!(title("东京", 0), None);
    }

    #[test]
    fn seasons_are_read_from_the_ways_names_write_them() {
        let season = |title: &str| Name::of(title).season;
        assert_eq!(season("某番 第十二季"), Some(12));
        assert_eq!(season("某番 Season 3"), Some(3));
        assert_eq!(season("某番 2nd Season"), Some(2));
        assert_eq!(season("某番(第2クール)"), Some(2));
        assert_eq!(season("某番 2026"), None);
        assert_eq!(
            Name::of("夜桜さんちの大作戦(第2期)第2クール").base,
            "夜桜さんちの大作戦"
        );
    }
}

#[cfg(test)]
mod real_data {
    /// `BANGUMI_DATA=/path/to/data.json cargo test -p erika-anime-core -- --ignored`
    #[test]
    #[ignore]
    fn parses_the_published_data() {
        let path = std::env::var("BANGUMI_DATA").expect("BANGUMI_DATA");
        let json = std::fs::read_to_string(path).unwrap();
        let now = crate::clock::parse_utc("2026-10-21T12:00:00Z").unwrap();
        let slots = super::parse(&json, now);
        println!("{} slots on air", slots.len());
        assert!(slots.len() > 50);
    }
}
