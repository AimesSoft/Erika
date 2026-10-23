//! What a video's name says: the episode number in it, as fansub releases
//! write it (`- 01`, `[01]`, `第01话`, `S01E02`, `EP01`, a bare ` 01`).

/// What may follow an episode number.
fn ends_number(c: Option<&char>) -> bool {
    c.is_none_or(|c| c.is_whitespace() || "]】)）话話集vV_[-.".contains(*c))
}

/// The episode number in a video's name, `None` when it has none. A
/// number after a dash, in brackets, after 第 or after E/EP counts first;
/// otherwise the last one standing alone. Years (1900–2099) and numbers
/// such as 1080p are no episodes.
pub fn episode_number(name: &str) -> Option<i64> {
    let stem = match name.rsplit_once('.') {
        Some((stem, extension))
            if !extension.is_empty()
                && extension.len() <= 5
                && extension.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            stem
        }
        _ => name,
    };
    let chars: Vec<char> = stem.chars().collect();
    let mut loose = None;
    for start in 0..chars.len() {
        if !chars[start].is_ascii_digit() || (start > 0 && chars[start - 1].is_ascii_digit()) {
            continue;
        }
        let end = start
            + chars[start..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .count();
        if end - start > 4 || !ends_number(chars.get(end)) {
            continue;
        }
        let Ok(number) = chars[start..end].iter().collect::<String>().parse::<i64>() else {
            continue;
        };
        if end - start == 4 && (1900..2100).contains(&number) {
            continue;
        }
        let before = &chars[..start];
        let last = before.last().copied();
        let dash = last == Some('-')
            || (last.is_some_and(char::is_whitespace)
                && before.len() >= 2
                && before[before.len() - 2] == '-');
        let marked = matches!(last, Some('[' | '【' | '第' | '#'));
        let episode_letter = {
            let letters: String = before
                .iter()
                .rev()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let ahead = before
                .len()
                .checked_sub(letters.len() + 1)
                .map(|at| before[at]);
            matches!(letters.to_ascii_lowercase().as_str(), "e" | "ep")
                && ahead.is_none_or(|c| !c.is_alphabetic())
        };
        if dash || marked || episode_letter {
            return Some(number);
        }
        if last.is_some_and(|c| c.is_whitespace() || c == '_') {
            loose = Some(number);
        }
    }
    loose
}

/// A name to search dandanplay with: the name without its extension, what
/// is in brackets, episode numbers and the words around them.
pub fn search_words(name: &str) -> String {
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let mut depth = 0usize;
    let mut text = String::new();
    for c in stem.chars() {
        match c {
            '[' | '【' | '(' | '（' => depth += 1,
            ']' | '】' | ')' | '）' => depth = depth.saturating_sub(1),
            _ if depth == 0 => text.push(if c == '_' || c == '.' { ' ' } else { c }),
            _ => {}
        }
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let cut = words
        .iter()
        .position(|word| {
            *word == "-" || word.chars().all(|c| c.is_ascii_digit()) || word.starts_with('第')
        })
        .unwrap_or(words.len());
    let kept = words[..cut].join(" ");
    if kept.is_empty() {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        kept
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_read_as_releases_write_them() {
        for (name, number) in [
            ("[Sakura] Sousou no Frieren S2 - 01 [1080p].mkv", Some(1)),
            ("[ANi] Jigokuraku - 13 [1080P][Baha].mp4", Some(13)),
            ("[Skymoon-Raws] One Piece - 1003 [720p].mp4", Some(1003)),
            ("[SweetSub][Frieren][02][1080P][CHS].mkv", Some(2)),
            ("Mob Psycho 100 - 03 [x264].mkv", Some(3)),
            ("葬送的芙莉莲 第05话.mp4", Some(5)),
            ("Show.S01E07.1080p.WEB.mkv", Some(7)),
            ("Show EP12.mkv", Some(12)),
            ("Show 08.mkv", Some(8)),
            ("Show - 04v2.mkv", Some(4)),
            ("第五话 修正版.mkv", None),
            ("OVA.mp4", None),
            ("[Skymoon-Raws] One Piece SP 鬼岛前夜 [1080p].mp4", None),
            ("寒蝉鸣泣之时 解 录像 2024年07月04日.ts", None),
            ("Frieren 2023.mkv", None),
        ] {
            assert_eq!(episode_number(name), number, "{name}");
        }
    }

    #[test]
    fn search_words_drop_groups_numbers_and_tags() {
        assert_eq!(
            search_words("[Sakura] Sousou no Frieren S2 - 01 [1080p].mkv"),
            "Sousou no Frieren S2"
        );
        assert_eq!(
            search_words("[ANi] Jigokuraku - 13 [1080P][Baha].mp4"),
            "Jigokuraku"
        );
        assert_eq!(search_words("地狱乐 S2"), "地狱乐 S2");
    }
}
