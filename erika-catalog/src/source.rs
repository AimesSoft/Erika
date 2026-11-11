//! Typed upstream identities are verified against TMDB, never inferred from a
//! provider's numeric range, filename or an unknown season ordering.
use crate::resolve::{Resolution, Resolved};
use crate::{BindingItem, Client, Error, Kind, Provider, ProviderRef, Result, tmdb};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ordering {
    #[default]
    Unknown,
    Tmdb,
}

/// Input claims, not a stored media snapshot. No URL, headers or credentials.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaHint {
    pub kind: Kind,
    #[serde(default)]
    pub provider_refs: Vec<ProviderRef>,
    #[serde(default)]
    pub series_provider_refs: Vec<ProviderRef>,
    pub season_number: Option<u32>,
    pub episode_number: Option<u32>,
    pub episode_number_end: Option<u32>,
    #[serde(default)]
    pub ordering: Ordering,
}

fn tmdb_ref(refs: &[ProviderRef], kind: Kind) -> Result<Option<ProviderRef>> {
    if refs.len() > 16 {
        return Err(Error::invalid("来源引用过多"));
    }
    let mut found = None;
    for r in refs {
        r.validate()?;
        if r.provider != Provider::Tmdb {
            continue;
        }
        if r.kind != kind {
            return Err(Error::invalid("来源的 TMDB 引用层级不正确"));
        }
        if found.as_ref().is_some_and(|known| known != r) {
            return Err(Error::invalid("来源包含互相冲突的 TMDB 身份"));
        }
        found = Some(r.clone());
    }
    Ok(found)
}

impl MediaHint {
    pub fn validate(&self) -> Result<()> {
        if !self.kind.playable() {
            return Err(Error::invalid("播放来源必须是电影或分集"));
        }
        tmdb_ref(&self.provider_refs, self.kind)?;
        tmdb_ref(&self.series_provider_refs, Kind::Tv)?;
        if self.kind == Kind::Movie
            && (!self.series_provider_refs.is_empty()
                || self.season_number.is_some()
                || self.episode_number.is_some()
                || self.episode_number_end.is_some())
        {
            return Err(Error::invalid("电影不能包含季集身份"));
        }
        if let Some(end) = self.episode_number_end {
            let start = self
                .episode_number
                .ok_or_else(|| Error::invalid("多集文件缺少起始集"))?;
            if end < start || end - start >= 1000 {
                return Err(Error::invalid("来源的多集范围不正确"));
            }
        }
        Ok(())
    }

    fn work_ref(&self) -> Result<Option<ProviderRef>> {
        tmdb_ref(
            if self.kind == Kind::Movie {
                &self.provider_refs
            } else {
                &self.series_provider_refs
            },
            if self.kind == Kind::Movie {
                Kind::Movie
            } else {
                Kind::Tv
            },
        )
    }
}

fn pending(work: Option<&tmdb::Work>, reason: &str) -> Resolution {
    Resolution::Pending {
        candidates: work.map(|w| vec![w.node.clone()]).unwrap_or_default(),
        recognition: None,
        reason: reason.into(),
    }
}

/// Resolve only explicit source claims; this never invokes name search or DDP.
pub async fn resolve(client: &Client, hint: &MediaHint) -> Result<Resolution> {
    hint.validate()?;
    let Some(reference) = hint.work_ref()? else {
        return Ok(pending(None, "来源缺少作品层级的 TMDB 身份"));
    };
    let work = client.work(&reference).await?;
    if hint.kind == Kind::Movie {
        return Ok(Resolution::Bound {
            result: Resolved {
                items: vec![item(&work.node)],
                work: client.with_artwork(work).await,
                season: None,
                recognition: None,
                evidence: "source:verified-tmdb-movie".into(),
            },
        });
    }
    let episode_ref = tmdb_ref(&hint.provider_refs, Kind::Episode)?;
    let multi = hint
        .episode_number_end
        .is_some_and(|end| Some(end) != hint.episode_number);
    if (episode_ref.is_none() || multi) && hint.ordering != Ordering::Tmdb {
        return Ok(pending(
            Some(&work),
            "来源编排未知，不能把来源集号当成 TMDB 集号",
        ));
    }
    if episode_ref.is_none() && (hint.season_number.is_none() || hint.episode_number.is_none()) {
        return Ok(pending(Some(&work), "来源缺少可验证的季集位置"));
    }
    let mut numbers: Vec<_> = work
        .seasons
        .iter()
        .filter_map(|s| s.season_number)
        .collect();
    // A source using another ordering may put an explicit TMDB episode in the
    // wrong season. Check the ID in real TMDB seasons; the hinted season is only
    // a fetch-order optimization, including S00.
    numbers.sort_by_key(|n| (Some(*n) != hint.season_number, *n));
    if episode_ref.is_none() {
        numbers.retain(|n| Some(*n) == hint.season_number);
    }
    for number in numbers {
        let season = client.season(&work, number).await?;
        if let Some(items) = select(hint, &work, &season)? {
            return Ok(Resolution::Bound {
                result: Resolved {
                    work: client.with_artwork(work).await,
                    season: Some(season),
                    items,
                    recognition: None,
                    evidence: "source:verified-tmdb-episode".into(),
                },
            });
        }
    }
    Ok(pending(
        Some(&work),
        "来源的分集身份或范围不在该 TMDB 作品中",
    ))
}

fn item(node: &crate::MediaNode) -> BindingItem {
    BindingItem {
        media: node.id.clone(),
        start_seconds: None,
        end_seconds: None,
    }
}

fn select(
    hint: &MediaHint,
    work: &tmdb::Work,
    season: &tmdb::Season,
) -> Result<Option<Vec<BindingItem>>> {
    if season.node.root != work.node.id {
        return Err(Error::invalid("来源季不属于该作品"));
    }
    let reference = tmdb_ref(&hint.provider_refs, Kind::Episode)?;
    let first = if let Some(reference) = &reference {
        season
            .episodes
            .iter()
            .find(|n| crate::resolve::same_episode(n, reference, &work.node.id))
    } else if hint.ordering == Ordering::Tmdb && hint.season_number == season.node.season_number {
        season
            .episodes
            .iter()
            .find(|n| n.episode_number == hint.episode_number)
    } else {
        None
    };
    let Some(first) = first else {
        return Ok(None);
    };
    let multi = hint
        .episode_number_end
        .is_some_and(|end| Some(end) != hint.episode_number);
    if !multi {
        return Ok(Some(vec![item(first)]));
    }
    if hint.ordering != Ordering::Tmdb
        || hint.season_number != first.season_number
        || hint.episode_number != first.episode_number
    {
        return Ok(None);
    }
    let start = first
        .episode_number
        .ok_or_else(|| Error::invalid("TMDB 分集缺少位置"))?;
    let end = hint
        .episode_number_end
        .expect("validated multi-episode range");
    Ok((start..=end)
        .map(|number| {
            season
                .episodes
                .iter()
                .find(|n| n.episode_number == Some(number))
                .map(item)
        })
        .collect())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod http;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (tmdb::Work, tmdb::Season) {
        let work = tmdb::parse_work(
            Kind::Tv,
            71,
            &json!({"id":71,"name":"TV","seasons":[{"id":72,"season_number":0,"name":"SP"}]}),
        )
        .unwrap();
        let season = tmdb::parse_season(
            &work,
            0,
            &json!({"id":72,"season_number":0,"name":"SP","episodes":[
                {"id":73,"season_number":0,"episode_number":1,"name":"SP1"},
                {"id":74,"season_number":0,"episode_number":2,"name":"SP2"}
            ]}),
        )
        .unwrap();
        (work, season)
    }
    fn hint() -> MediaHint {
        MediaHint {
            kind: Kind::Episode,
            provider_refs: vec![],
            series_provider_refs: vec![ProviderRef::tmdb(Kind::Tv, 71).unwrap()],
            season_number: Some(0),
            episode_number: Some(1),
            episode_number_end: None,
            ordering: Ordering::Unknown,
        }
    }
    #[test]
    fn unknown_ordering_never_promotes_a_position_but_verified_episode_id_wins() {
        let (work, season) = fixture();
        let mut h = hint();
        assert!(select(&h, &work, &season).unwrap().is_none());
        h.provider_refs
            .push(ProviderRef::tmdb(Kind::Episode, 74).unwrap());
        h.season_number = Some(9);
        let selected = select(&h, &work, &season).unwrap().unwrap();
        assert_eq!(selected[0].media, season.episodes[1].id);
        h.provider_refs[0] = ProviderRef::tmdb(Kind::Episode, 999).unwrap();
        assert!(select(&h, &work, &season).unwrap().is_none());
    }
    #[test]
    fn tmdb_ordering_keeps_specials_and_requires_every_multi_episode() {
        let (work, season) = fixture();
        let mut h = hint();
        h.ordering = Ordering::Tmdb;
        h.episode_number_end = Some(2);
        assert_eq!(select(&h, &work, &season).unwrap().unwrap().len(), 2);
        h.episode_number_end = Some(3);
        assert!(select(&h, &work, &season).unwrap().is_none());
        h.episode_number_end = Some(2);
        h.provider_refs
            .push(ProviderRef::tmdb(Kind::Episode, 74).unwrap());
        assert!(
            select(&h, &work, &season).unwrap().is_none(),
            "do not shorten a conflicting multi-episode file"
        );
    }
    #[test]
    fn source_ids_keep_their_hierarchy_and_ranges_are_bounded() {
        let mut h = hint();
        h.provider_refs
            .push(ProviderRef::tmdb(Kind::Tv, 71).unwrap());
        assert!(h.validate().is_err());
        h.provider_refs = vec![
            ProviderRef::tmdb(Kind::Episode, 73).unwrap(),
            ProviderRef::tmdb(Kind::Episode, 74).unwrap(),
        ];
        assert!(h.validate().is_err());
        h.provider_refs.clear();
        h.episode_number_end = Some(0);
        assert!(h.validate().is_err());
        h.episode_number_end = Some(1001);
        assert!(h.validate().is_err());
        h.episode_number_end = None;
        h.kind = Kind::Movie;
        assert!(h.validate().is_err());
        let decoded:MediaHint=serde_json::from_value(json!({"kind":"episode","seriesProviderRefs":[],"seasonNumber":null,"episodeNumber":null})).unwrap();
        assert_eq!(decoded.ordering, Ordering::Unknown);
        assert_eq!(decoded.season_number, None);
    }
}
