//! The signed-in user's own record of an anime on dandanplay: the episodes
//! watched, how the anime is followed, and its score. Ported from NipaPlay
//! (`addPlayHistory`, `addFavorite` and `submitUserRating` in
//! `dandanplay_service_io.dart`). dandanplay can add play history but not
//! take it back, and its follow states do not tell "want to watch" from
//! "watching".

use crate::{Details, Error, Result, request, token};

/// How the user follows an anime on dandanplay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Follow {
    /// Followed: wanting to watch it or watching it.
    Favorited,
    Finished,
    Abandoned,
}

impl Follow {
    fn key(self) -> &'static str {
        match self {
            Self::Favorited => "favorited",
            Self::Finished => "finished",
            Self::Abandoned => "abandoned",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "favorited" => Some(Self::Favorited),
            "finished" => Some(Self::Finished),
            "abandoned" => Some(Self::Abandoned),
            _ => None,
        }
    }
}

/// What the account says of an anime, read from its details as fetched
/// with the token: the follow state, the score, and the ids of the episodes
/// watched.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mine {
    pub follow: Option<Follow>,
    pub rating: i64,
    pub watched: Vec<i64>,
}

impl Mine {
    pub fn of(details: &Details) -> Self {
        Self {
            follow: details
                .favorite_status
                .as_deref()
                .and_then(Follow::parse)
                .or(details.is_favorited.then_some(Follow::Favorited)),
            rating: details.user_rating.clamp(0, 10),
            watched: details
                .episodes
                .iter()
                .filter(|episode| {
                    episode
                        .last_watched
                        .as_deref()
                        .is_some_and(|at| !at.trim().is_empty())
                })
                .map(|episode| episode.episode_id)
                .collect(),
        }
    }
}

/// The account's record of anime `anime_id`, from details fetched anew.
pub fn mine(anime_id: i64) -> Result<(Details, Mine)> {
    let details = crate::details_from(anime_id, false)?;
    let mine = Mine::of(&details);
    Ok((details, mine))
}

fn succeeded(reply: serde_json::Value, otherwise: &str) -> Result<()> {
    if reply.get("success").and_then(|v| v.as_bool()) == Some(false) {
        return Err(Error::new(
            reply
                .get("errorMessage")
                .and_then(|v| v.as_str())
                .filter(|message| !message.is_empty())
                .unwrap_or(otherwise),
        ));
    }
    Ok(())
}

/// Add episodes `episode_ids`, all of one anime, to the account's play
/// history; dandanplay takes at most 100 at a time.
pub fn watched(episode_ids: &[i64]) -> Result<()> {
    let token = token().ok_or_else(Error::sign_in)?;
    for chunk in episode_ids
        .iter()
        .copied()
        .filter(|id| *id > 0)
        .collect::<Vec<_>>()
        .chunks(100)
    {
        let body = serde_json::json!({
            "episodeIdList": chunk,
            "addToFavorite": false,
            "rating": 0,
        });
        let reply = request("POST", "/api/v2/playhistory", "", Some(body), Some(&token))?;
        succeeded(reply, "弹弹play 没能记下看过的剧集")?;
    }
    Ok(())
}

/// Follow anime `anime_id` as `follow`, and give it `rating` (1 to 10);
/// `None` and 0 leave what the account has.
pub fn follow(anime_id: i64, follow: Option<Follow>, rating: i64) -> Result<()> {
    if anime_id <= 0 {
        return Err(Error::new("这部作品不在弹弹play上"));
    }
    let token = token().ok_or_else(Error::sign_in)?;
    let body = serde_json::json!({
        "animeId": anime_id,
        "favoriteStatus": follow.map(Follow::key),
        "rating": rating.clamp(0, 10),
    });
    let reply = request("POST", "/api/v2/favorite", "", Some(body), Some(&token))?;
    succeeded(reply, "弹弹play 没能保存收藏状态")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_account_record_reads_from_the_details() {
        let details: Details = serde_json::from_str(
            r#"{"animeId":17675,"isFavorited":true,"favoriteStatus":"finished","userRating":8,
               "episodes":[{"episodeId":176750001,"episodeNumber":"1","lastWatched":"2026-10-01T20:00:00"},
                           {"episodeId":176750002,"episodeNumber":"2","lastWatched":null},
                           {"episodeId":176750003,"episodeNumber":"3"}]}"#,
        )
        .unwrap();
        assert_eq!(
            Mine::of(&details),
            Mine {
                follow: Some(Follow::Finished),
                rating: 8,
                watched: vec![176_750_001],
            }
        );
        let followed: Details =
            serde_json::from_str(r#"{"animeId":1,"isFavorited":true,"favoriteStatus":null}"#)
                .unwrap();
        assert_eq!(Mine::of(&followed).follow, Some(Follow::Favorited));
        let none: Details = serde_json::from_str(r#"{"animeId":1}"#).unwrap();
        assert_eq!(Mine::of(&none), Mine::default());
    }

    #[test]
    fn writing_needs_the_account() {
        crate::set_token(None);
        assert!(watched(&[1]).unwrap_err().sign_in);
        assert!(follow(1, Some(Follow::Finished), 0).unwrap_err().sign_in);
        assert!(!follow(-3, None, 8).unwrap_err().sign_in);
    }
}
