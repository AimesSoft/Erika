//! Background server-side cache for the daily new-season calendar.
//! Pre-resolves dandanplay's public season list into canonical TMDB works,
//! artwork, airing seasons and Bangumi/dandanplay subject links so clients
//! hydrate the entire calendar in a single request instead of hundreds.

use crate::Gateway;
use base64::Engine as _;
use erika_catalog::{
    Kind, MediaNode, Provider, ProviderRef,
    artwork::{self, Artwork, ArtworkKind, ImageConfiguration},
    names::{NameHints, sequel_base},
    resolve::{accept_search, earliest_premiere, parse_ymd},
    tmdb::{SearchPage, SeasonBundle, SeasonBundleItem, Work, parse_search, parse_work},
};
use serde::Deserialize;
use sha2::Digest as _;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LANGUAGE: &str = "zh-CN";
const HIT_TTL_SECS: i64 = 24 * 3600;
const MISS_TTL_SECS: i64 = 4 * 3600;
const REFRESH_INTERVAL: Duration = Duration::from_secs(2 * 3600);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicSeasonReply {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    bangumi_list: Vec<erika_dandanplay::Intro>,
}

#[derive(Deserialize)]
struct PublicDetailsReply {
    #[serde(default)]
    success: bool,
    bangumi: Option<erika_dandanplay::catalog::Details>,
}

pub fn spawn_warmer(gateway: Arc<Gateway>, dandanplay_base: String, dandanplay_host: String) {
    tokio::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };
        loop {
            warm_once(&gateway, &client, &dandanplay_base, &dandanplay_host).await;
            tokio::time::sleep(REFRESH_INTERVAL).await;
        }
    });
}

pub async fn warm_once(
    gateway: &Gateway,
    client: &reqwest::Client,
    dandanplay_base: &str,
    dandanplay_host: &str,
) {
    if !gateway.metadata.configured() {
        return;
    }
    let Some(season_bytes) =
        signed_get(client, dandanplay_base, dandanplay_host, "/public/season").await
    else {
        return;
    };
    let Ok(reply) = serde_json::from_slice::<PublicSeasonReply>(&season_bytes) else {
        return;
    };
    if !reply.success || reply.bangumi_list.is_empty() {
        return;
    }
    let intros: Vec<_> = reply
        .bangumi_list
        .into_iter()
        .filter(|i| !i.is_restricted && i.anime_id > 0)
        .collect();
    let ids: Vec<i64> = intros.iter().map(|i| i.anime_id).collect();
    let images = gateway
        .metadata
        .tmdb_data("tmdb/images/configuration", &[])
        .await
        .and_then(|v| ImageConfiguration::parse(&v).ok());

    publish_stored(gateway, &ids, images.clone());

    let mut updated_since_publish = 0usize;
    for intro in &intros {
        if let Some(existing) = gateway
            .store
            .season_entry(intro.anime_id, LANGUAGE, HIT_TTL_SECS)
            && existing != "null"
        {
            continue;
        }
        if gateway
            .store
            .season_entry(intro.anime_id, LANGUAGE, MISS_TTL_SECS)
            .as_deref()
            == Some("null")
        {
            continue;
        }
        match resolve_entry(gateway, client, dandanplay_base, dandanplay_host, intro).await {
            Some(item) if item.validate().is_ok() => {
                if let Ok(json) = serde_json::to_string(&item) {
                    gateway
                        .store
                        .save_season_entry(intro.anime_id, LANGUAGE, &json);
                    updated_since_publish += 1;
                }
            }
            _ => {
                gateway
                    .store
                    .save_season_entry(intro.anime_id, LANGUAGE, "null");
            }
        }
        if updated_since_publish >= 5 {
            publish_stored(gateway, &ids, images.clone());
            updated_since_publish = 0;
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    publish_stored(gateway, &ids, images);
}

fn publish_stored(gateway: &Gateway, ids: &[i64], images: Option<ImageConfiguration>) {
    let items: Vec<SeasonBundleItem> = gateway
        .store
        .season_entries(ids, LANGUAGE)
        .into_iter()
        .filter_map(|raw| serde_json::from_str::<SeasonBundleItem>(&raw).ok())
        .filter(|item| item.validate().is_ok())
        .collect();
    gateway
        .metadata
        .set_season_bundle(LANGUAGE, &SeasonBundle { images, items });
}

async fn signed_get(
    client: &reqwest::Client,
    base: &str,
    host: &str,
    path: &str,
) -> Option<Vec<u8>> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    let sig = base64::engine::general_purpose::STANDARD.encode(sha2::Sha256::digest(format!(
        "nipaplayv1{ts}{path}server-managed"
    )));
    let url = format!("{}{path}", base.trim_end_matches('/'));
    let mut req = client
        .get(&url)
        .header("Accept", "application/json")
        .header("X-AppId", "nipaplayv1")
        .header("X-Timestamp", &ts)
        .header("X-Signature", &sig);
    if !host.is_empty() {
        req = req.header("Host", host);
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.bytes().await.ok().map(|b| b.to_vec())
}

async fn resolve_entry(
    gateway: &Gateway,
    client: &reqwest::Client,
    dandanplay_base: &str,
    dandanplay_host: &str,
    intro: &erika_dandanplay::Intro,
) -> Option<SeasonBundleItem> {
    let id_str = intro.anime_id.to_string();
    let bytes = signed_get(
        client,
        dandanplay_base,
        dandanplay_host,
        &format!("/public/details/{id_str}"),
    )
    .await?;
    let reply: PublicDetailsReply = serde_json::from_slice(&bytes).ok()?;
    let details = reply
        .bangumi
        .filter(|d| reply.success && d.anime_id.to_string() == id_str)?;

    let mut subjects = vec![ProviderRef {
        provider: Provider::Dandanplay,
        scope: String::new(),
        kind: Kind::Subject,
        id: id_str,
    }];
    if let Some(bgm) = details.bangumi_ref() {
        subjects.push(bgm);
    }

    let mut titles = Vec::new();
    for t in std::iter::once(intro.anime_title.as_str())
        .chain(std::iter::once(details.anime_title.as_str()))
        .chain(details.titles.iter().map(|t| t.title.as_str()))
    {
        let trimmed = t.trim();
        if !trimmed.is_empty() && !titles.iter().any(|existing| existing == trimmed) {
            titles.push(trimmed.to_string());
        }
    }
    let dates: Vec<String> = details
        .seasons
        .iter()
        .filter_map(|s| s.air_date.clone())
        .chain(details.episodes.iter().filter_map(|e| e.air_date.clone()))
        .collect();
    let kind = match details.media_type.as_str() {
        "movie" | "jpmovie" => Some(Kind::Movie),
        "tvseries" | "web" | "tvspecial" | "jpdrama" => Some(Kind::Tv),
        _ => None,
    };

    let mut refs = details.tmdb_refs();
    let reference = if refs.len() == 1 {
        refs.remove(0)
    } else if refs.is_empty() {
        let (year, _) = earliest_premiere(dates.iter().map(String::as_str))?;
        let hints = NameHints {
            titles,
            year: Some(year),
            ..Default::default()
        };
        search_tmdb_work(gateway, &hints, kind).await?
    } else {
        return None;
    };

    let mut work = load_tmdb_work(gateway, &reference).await?;
    enrich_work(gateway, &mut work).await;
    let season = airing_season(&work, &dates);
    let summary = if !details.summary.trim().is_empty() {
        details.summary
    } else {
        intro.summary.clone()
    };
    Some(SeasonBundleItem {
        anime_id: intro.anime_id,
        work,
        season,
        subjects,
        summary,
    })
}

async fn load_tmdb_work(gateway: &Gateway, reference: &ProviderRef) -> Option<Work> {
    let id: u64 = reference.id.parse().ok()?;
    let path = format!("tmdb/{}/{}", reference.kind.as_str(), reference.id);
    let body = gateway
        .metadata
        .tmdb_data(&path, &[("language", LANGUAGE)])
        .await?;
    let mut work = parse_work(reference.kind, id, &body).ok()?;
    if reference.kind == Kind::Tv
        && work
            .seasons
            .iter()
            .filter(|s| s.season_number.is_some_and(|n| n > 0))
            .count()
            <= 1
        && let Some(groups) = gateway
            .metadata
            .tmdb_data(&format!("tmdb/tv/{id}/episode_groups"), &[])
            .await
        && let Some(group_id) = erika_catalog::tmdb::choose_seasons_group(&groups)
        && let Some(group_body) = gateway
            .metadata
            .tmdb_data(
                &format!("tmdb/tv/episode_group/{group_id}"),
                &[("language", LANGUAGE)],
            )
            .await
    {
        let _ = work.apply_episode_group(&group_body);
    }
    Some(work)
}

async fn enrich_work(gateway: &Gateway, work: &mut Work) {
    let Some(reference) = work.node.reference.clone() else {
        return;
    };
    let Ok(id) = reference.id.parse::<u64>() else {
        return;
    };
    let img_path = format!("tmdb/{}/{}/images", reference.kind.as_str(), reference.id);
    let langs = artwork::languages(LANGUAGE, &work.node.original_language).join(",");
    let mut parsed_images = gateway
        .metadata
        .tmdb_data(
            &img_path,
            &[("language", LANGUAGE), ("include_image_language", &langs)],
        )
        .await
        .and_then(|body| artwork::parse_images(&reference, &body).ok())
        .unwrap_or_default();
    if parsed_images.is_empty() {
        parsed_images = gateway
            .metadata
            .tmdb_data(&img_path, &[])
            .await
            .and_then(|body| artwork::parse_images(&reference, &body).ok())
            .unwrap_or_default();
    }
    if !parsed_images.is_empty() {
        let preferred = artwork::languages(LANGUAGE, &work.node.original_language);
        let original_preferred = artwork::logo_languages(LANGUAGE, &work.node.original_language);
        work.node.artwork.refresh(Artwork {
            poster: artwork::choose(
                &parsed_images,
                ArtworkKind::Poster,
                &original_preferred,
                780,
            ),
            backdrop: artwork::choose(&parsed_images, ArtworkKind::Backdrop, &preferred, 1920),
            logo: artwork::choose(&parsed_images, ArtworkKind::Logo, &original_preferred, 640),
            still: None,
        });
    }
    let orig = work.node.original_language.trim().to_string();
    if work.node.overview.is_empty() && !orig.is_empty() {
        let work_path = format!("tmdb/{}/{}", reference.kind.as_str(), reference.id);
        if let Some(zh_tw) = gateway
            .metadata
            .tmdb_data(&work_path, &[("language", "zh-TW")])
            .await
            && let Ok(chinese) = parse_work(reference.kind, id, &zh_tw)
        {
            if !chinese.node.overview.is_empty() {
                work.node.overview = chinese.node.overview;
            }
            for season in &mut work.seasons {
                if season.overview.is_empty()
                    && let Some(found) = chinese
                        .seasons
                        .iter()
                        .find(|s| s.season_number == season.season_number)
                {
                    season.overview.clone_from(&found.overview);
                }
            }
        }
        if work.node.overview.is_empty()
            && !orig.eq_ignore_ascii_case("zh")
            && orig.len() <= 16
            && orig.bytes().all(|c| c.is_ascii_alphabetic() || c == b'-')
            && let Some(fallback) = gateway
                .metadata
                .tmdb_data(&work_path, &[("language", &orig)])
                .await
            && let Ok(original) = parse_work(reference.kind, id, &fallback)
        {
            work.original_overview = original.node.overview;
        }
    }
}

async fn search_tmdb_work(
    gateway: &Gateway,
    hints: &NameHints,
    kind: Option<Kind>,
) -> Option<ProviderRef> {
    let year = hints.year?;
    let year_str = year.to_string();
    let kinds: Vec<Kind> = kind
        .filter(|k| matches!(k, Kind::Tv | Kind::Movie))
        .map_or(vec![Kind::Tv, Kind::Movie], |k| vec![k]);
    let bases: Vec<(String, String)> = hints
        .titles
        .iter()
        .filter_map(|t| sequel_base(t).map(|(b, _)| (t.clone(), b)))
        .collect();
    let mut queries: Vec<&String> = Vec::new();
    for title in &hints.titles {
        let q = bases
            .iter()
            .find(|(full, _)| full == title)
            .map_or(title, |(_, b)| b);
        if !queries.contains(&q) {
            queries.push(q);
        }
    }
    let mut pages = Vec::new();
    let mut complete = true;
    for title in queries.into_iter().take(4) {
        for k in &kinds {
            let Some(body) = gateway
                .metadata
                .tmdb_data(
                    "search",
                    &[
                        ("kind", k.as_str()),
                        ("query", title),
                        ("language", LANGUAGE),
                        ("page", "1"),
                        ("year", &year_str),
                    ],
                )
                .await
            else {
                complete = false;
                continue;
            };
            let Ok(page) = parse_search(*k, &body) else {
                complete = false;
                continue;
            };
            if page.page != 1 || page.total_pages > 1 {
                complete = false;
            }
            pages.push(page);
        }
    }
    if !complete || pages.is_empty() {
        return None;
    }
    let mut candidates = Vec::new();
    for page in pages {
        for cand in page.results {
            if !candidates.iter().any(|n: &MediaNode| n.id == cand.id) {
                candidates.push(cand);
            }
        }
    }
    let combined = SearchPage {
        results: candidates.clone(),
        page: 1,
        total_pages: 1,
    };
    if let Some(accepted) = accept_search(hints, &combined) {
        return Some(accepted);
    }
    let base_only: Vec<String> = bases.into_iter().map(|(_, b)| b).collect();
    if !base_only.is_empty()
        && let Some(accepted) = accept_search(
            &NameHints {
                titles: base_only.clone(),
                ..hints.clone()
            },
            &combined,
        )
    {
        return Some(accepted);
    }
    // Check long-running / single-season series airing in `year`.
    let norm = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    for names in [&hints.titles, &base_only] {
        let earlier: Vec<_> = candidates
            .iter()
            .filter(|n| {
                n.kind == Kind::Tv
                    && names.iter().any(|nm| {
                        let nm = norm(nm);
                        !nm.is_empty() && (nm == norm(&n.title) || nm == norm(&n.original_title))
                    })
                    && n.date
                        .get(..4)
                        .and_then(|y| y.parse::<u16>().ok())
                        .is_none_or(|y| y < year)
            })
            .take(4)
            .collect();
        let mut found = Vec::new();
        for node in earlier {
            let Some(reference) = &node.reference else {
                continue;
            };
            let Some(work) = load_tmdb_work(gateway, reference).await else {
                return None;
            };
            if work.airs_in(year) {
                found.push(reference.clone());
            }
        }
        match found.len() {
            0 => continue,
            1 => return found.pop(),
            _ => return None,
        }
    }
    None
}

fn days_of(text: &str) -> Option<i64> {
    let (y, m, d) = parse_ymd(text)?;
    let (y, m, d) = (i64::from(y), i64::from(m), i64::from(d));
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn airing_season(work: &Work, dates: &[String]) -> Option<ProviderRef> {
    let premiere = dates.iter().filter_map(|d| days_of(d)).min()?;
    let regular: Vec<_> = work
        .seasons
        .iter()
        .filter(|s| s.season_number.is_some_and(|n| n > 0))
        .collect();
    let single = regular.len() == 1;
    regular
        .into_iter()
        .filter_map(|s| {
            let d =
                days_of(&s.date).or_else(|| single.then(|| days_of(&work.node.date)).flatten())?;
            let diff = (d - premiere).abs();
            (diff <= 60).then_some((s, diff))
        })
        .min_by_key(|(_, diff)| *diff)
        .and_then(|(s, _)| s.reference.clone())
}
