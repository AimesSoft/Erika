//! A batch is a preview of real TMDB identities, not an arithmetic episode ID.
//! File revisions are captured when opening it. Content and danmaku edits are
//! independent; confirming this page never writes to an account or danmaku server.
mod engine;
#[cfg(test)]
pub(crate) mod tests;

pub(super) use engine::Reply;
use erika_catalog::{
    BindingItem, Decision, FileBinding, Kind, MediaId, MediaNode,
    names::NameHints,
    tmdb::{Season, Work},
};
use erika_library_protocol::library::{MatchNote, MatchRow, MatchSeason, MatchWork, Matching};
use erika_store::FileId;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct Video {
    pub key: i64,
    pub name: String,
    pub file: Option<FileId>,
    pub address: Option<erika_store::catalog::AddressCheck>,
    pub revision: u64,
    pub hints: NameHints,
    pub previous: Option<FileBinding>,
    pub current: String,
}

pub struct Session {
    /// New for every open: callbacks from a closed page cannot revive on reopen.
    pub id: String,
    pub generation: u64,
    pub single: bool,
    pub place: String,
    pub videos: Vec<Video>,
    pub on: HashSet<i64>,
    pub set: HashMap<i64, String>,
    pub by_order: bool,
    pub start: i64,
    pub offset: i64,
    pub selected: Option<MediaNode>,
    pub work: Option<Work>,
    pub selected_season: String,
    pub season: Option<Season>,
    pub editing: Option<i64>,
    pub odd_only: bool,
    pub results: Vec<MediaNode>,
    pub searching: bool,
    pub loading: bool,
    pub error: String,
}

pub struct Plan {
    pub addresses: Vec<erika_store::catalog::AddressCheck>,
    pub nodes: Vec<MediaNode>,
    pub bindings: Vec<FileBinding>,
}

fn label(node: &MediaNode) -> String {
    match (node.season_number, node.episode_number) {
        (Some(s), Some(e)) => format!("S{s:02}E{e:02} · {}", node.title),
        (Some(s), None) => format!("S{s:02} · {}", node.title),
        _ => node.title.clone(),
    }
}

/// A list/range is explicit coverage, not inferred chapter times. Keep malformed
/// input visible and invalid instead of silently reverting to filename hints.
fn numbers(text: &str) -> Result<Vec<u32>, String> {
    let mut out = Vec::new();
    for part in text.split([',', '，']) {
        let part = part.trim();
        let (start, end) = match part.split_once('-') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (part, part),
        };
        let parse = |s: &str| {
            s.parse::<u32>()
                .map_err(|_| "集数格式不正确，请填写 1、1-3 或 1,3".to_string())
        };
        let (start, end) = (parse(start)?, parse(end)?);
        if end < start || end - start > 999 || out.len() + (end - start + 1) as usize > 1000 {
            return Err("分集范围不正确".into());
        }
        for n in start..=end {
            if out.contains(&n) {
                return Err("同一文件不能重复指定同一集".into());
            }
            out.push(n);
        }
    }
    Ok(out)
}
impl Session {
    pub fn one(video: Video, place: String) -> Self {
        Self::new(true, place, vec![video])
    }
    pub fn folder(mut videos: Vec<Video>, place: String) -> Self {
        videos.sort_by(|a, b| {
            a.hints
                .episodes
                .first()
                .copied()
                .unwrap_or(u32::MAX)
                .cmp(&b.hints.episodes.first().copied().unwrap_or(u32::MAX))
                .then_with(|| crate::natural(&a.name, &b.name))
        });
        let mut session = Self::new(false, place, videos);
        // Unknown names (including SP/OVA) are never silently assigned by order.
        // The user can explicitly turn order on or type a real destination.
        if session.videos.iter().any(|v| !v.hints.episodes.is_empty()) {
            session.on.retain(|key| {
                session
                    .videos
                    .iter()
                    .any(|v| v.key == *key && !v.hints.episodes.is_empty())
            });
        }
        session
    }
    fn new(single: bool, place: String, videos: Vec<Video>) -> Self {
        let on = videos
            .iter()
            .filter(|v| v.file.is_some())
            .map(|v| v.key)
            .collect();
        Self {
            id: MediaId::new().0,
            generation: 0,
            single,
            place,
            videos,
            on,
            set: HashMap::new(),
            by_order: false,
            start: 1,
            offset: 0,
            selected: None,
            work: None,
            selected_season: String::new(),
            season: None,
            editing: None,
            odd_only: false,
            results: Vec::new(),
            searching: false,
            loading: false,
            error: String::new(),
        }
    }
    pub fn movie(&self) -> bool {
        self.selected
            .as_ref()
            .is_some_and(|n| n.kind == Kind::Movie)
    }
    fn values(&self) -> HashMap<i64, String> {
        let mut next = Some(self.start);
        self.videos
            .iter()
            .filter(|v| self.on.contains(&v.key))
            .map(|v| {
                let text = if self.movie() {
                    String::new()
                } else if let Some(text) = self.set.get(&v.key) {
                    text.clone()
                } else if self.by_order {
                    let value = next
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "超出范围".into());
                    next = next.and_then(|n| n.checked_add(1));
                    value
                } else {
                    v.hints
                        .episodes
                        .iter()
                        .map(|n| (i64::from(*n) + self.offset).to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                };
                (v.key, text)
            })
            .collect()
    }
    fn lowest(&self) -> Option<i64> {
        self.videos
            .iter()
            .filter(|v| self.on.contains(&v.key) && !self.set.contains_key(&v.key))
            .flat_map(|v| v.hints.episodes.iter().copied())
            .min()
            .map(i64::from)
    }
    pub fn align(&mut self, episode: i64) {
        if let Some(lowest) = self.lowest() {
            self.offset = episode - lowest;
        }
    }
    pub fn toggle(&mut self, key: i64) {
        if !self.on.remove(&key) && self.videos.iter().any(|v| v.key == key && v.file.is_some()) {
            self.on.insert(key);
        }
    }
    pub fn type_number(&mut self, key: i64, text: String) {
        if !self.videos.iter().any(|v| v.key == key && v.file.is_some()) {
            return;
        }
        if text.trim().is_empty() {
            self.set.remove(&key);
        } else {
            self.set.insert(key, text.trim().into());
            self.on.insert(key);
        }
        self.editing = None;
    }
    fn destinations<'a>(
        &'a self,
        video: &Video,
        value: &str,
    ) -> Result<Vec<&'a MediaNode>, String> {
        let work = self.work.as_ref().ok_or("请选择作品并等候资料载入")?;
        if work.node.kind == Kind::Movie {
            return Ok(vec![&work.node]);
        }
        let season = self.season.as_ref().ok_or("请选择季并等候分集载入")?;
        if !self.set.contains_key(&video.key)
            && video
                .hints
                .season
                .is_some_and(|s| Some(s) != season.node.season_number)
        {
            return Err(format!(
                "文件名属于 S{:02}，与所选季不同",
                video.hints.season.unwrap()
            ));
        }
        if value.is_empty() {
            return Err("请指定实际分集；特别篇标签不能代替季和集号".into());
        }
        numbers(value)?
            .into_iter()
            .map(|n| {
                let mut found = season
                    .episodes
                    .iter()
                    .filter(|e| e.episode_number == Some(n));
                let node = found
                    .next()
                    .ok_or_else(|| format!("所选季没有 E{n:02} 的分集资料"))?;
                if found.next().is_some() {
                    return Err(format!("E{n:02} 对应多个分集，请先核对资料"));
                }
                Ok(node)
            })
            .collect()
    }
    pub fn plan(&self) -> Result<Plan, String> {
        if self.loading {
            return Err("资料仍在载入".into());
        }
        let work = self.work.as_ref().ok_or("请先选择作品")?;
        let mut nodes = vec![work.node.clone()];
        if let Some(season) = &self.season {
            nodes.push(season.node.clone());
            nodes.extend(season.episodes.clone());
        }
        let values = self.values();
        let mut bindings: Vec<FileBinding> = Vec::new();
        let mut addresses = Vec::new();
        for video in self.videos.iter().filter(|v| self.on.contains(&v.key)) {
            let file = video.file.ok_or("文件无法读取")?;
            let address = video
                .address
                .as_ref()
                .ok_or("文件地址已失效，请重新打开匹配")?;
            if address.file != file || address.key != video.key {
                return Err("文件地址已改变，请重新打开匹配".into());
            }
            addresses.push(address.clone());
            let items = self.destinations(
                video,
                values.get(&video.key).map(String::as_str).unwrap_or(""),
            )?;
            let mut binding = FileBinding {
                file,
                revision: video.revision,
                decision: Decision::Manual,
                evidence: "manual:tmdb-library".into(),
                items: items
                    .iter()
                    .map(|n| BindingItem {
                        media: n.id.clone(),
                        start_seconds: None,
                        end_seconds: None,
                    })
                    .collect(),
            };
            // Preserve verified chapter spans only if the entire identity set is
            // unchanged. Never distribute a combined file's duration evenly.
            if let Some(old) = &video.previous {
                if old.items.len() == binding.items.len()
                    && binding
                        .items
                        .iter()
                        .all(|n| old.items.iter().any(|o| o.media == n.media))
                {
                    binding.items = old.items.clone();
                }
            }
            if let Some(other) = bindings.iter().find(|b| b.file == file) {
                if other != &binding {
                    return Err("同一物理文件的多个地址指向了不同分集".into());
                }
            } else {
                bindings.push(binding);
            }
        }
        if bindings.is_empty() {
            return Err("请选择要匹配的文件".into());
        }
        Ok(Plan {
            addresses,
            nodes,
            bindings,
        })
    }
    pub fn view(&self) -> Matching {
        let values = self.values();
        let mut taken = HashSet::new();
        let mut bad_count = 0;
        let rows: Vec<_> = self
            .videos
            .iter()
            .map(|video| {
                let on = self.on.contains(&video.key);
                let value = values.get(&video.key).cloned().unwrap_or_default();
                let destinations = self.destinations(video, &value);
                let twin = on
                    && destinations.as_ref().is_ok_and(|items| {
                        let overlap = items.iter().any(|n| taken.contains(&n.id));
                        taken.extend(items.iter().map(|n| n.id.clone()));
                        overlap
                    });
                let bad = on && destinations.is_err();
                bad_count += i64::from(bad);
                let note = if video.file.is_none() {
                    MatchNote::Unreadable
                } else if on && let Err(error) = &destinations {
                    MatchNote::Invalid {
                        reason: error.clone(),
                    }
                } else if twin {
                    MatchNote::Twin
                } else if !video.current.is_empty() {
                    MatchNote::Now {
                        title: video.current.clone(),
                    }
                } else {
                    MatchNote::Unmatched
                };
                MatchRow {
                    key: video.key,
                    name: video.name.clone(),
                    on,
                    assignment: value,
                    destination: if on {
                        destinations
                            .map(|items| {
                                items.into_iter().map(label).collect::<Vec<_>>().join(" / ")
                            })
                            .unwrap_or_default()
                    } else {
                        String::new()
                    },
                    editing: self.editing == Some(video.key),
                    note,
                    odd: !on || bad || twin,
                }
            })
            .collect();
        let on_count = rows.iter().filter(|r| r.on).count() as i64;
        let odd_count = rows.iter().filter(|r| r.odd).count() as i64;
        let lowest = if self.single || self.by_order {
            None
        } else {
            self.lowest()
        };
        let id = self
            .selected
            .as_ref()
            .map(|n| n.id.0.clone())
            .unwrap_or_default();
        Matching {
            open: true,
            single: self.single,
            place: self.place.clone(),
            work: id.clone(),
            title: self
                .selected
                .as_ref()
                .map(|n| n.title.clone())
                .unwrap_or_default(),
            movie: self.movie(),
            seasons: self
                .work
                .iter()
                .flat_map(|w| &w.seasons)
                .map(|s| MatchSeason {
                    id: s.id.0.clone(),
                    title: label(s),
                })
                .collect(),
            season: self.selected_season.clone(),
            loading: self.loading,
            error: self.error.clone(),
            by_order: self.by_order,
            start: self.start,
            lowest: lowest.unwrap_or(0),
            align_to: lowest.map_or(0, |n| n + self.offset),
            shown: rows
                .iter()
                .filter(|r| !self.odd_only || r.odd)
                .cloned()
                .collect(),
            all_count: rows.len() as i64,
            odd_count,
            on_count,
            skipped_count: rows.len() as i64 - on_count,
            bad_count,
            ready: self.plan().is_ok(),
            linked: self.single
                && self
                    .videos
                    .first()
                    .is_some_and(|v| v.previous.as_ref().is_some_and(|b| !b.items.is_empty())),
            results: self
                .results
                .iter()
                .map(|n| MatchWork {
                    id: n.id.0.clone(),
                    title: n.title.clone(),
                    detail: format!(
                        "{}{}",
                        if n.kind == Kind::Movie {
                            "电影"
                        } else {
                            "剧集"
                        },
                        n.date
                            .get(..4)
                            .map(|y| format!(" · {y}"))
                            .unwrap_or_default()
                    ),
                    chosen: n.id.0 == id,
                })
                .collect(),
            searching: self.searching,
            search_failed: !self.error.is_empty(),
            odd_only: self.odd_only,
            rows,
        }
    }
}

pub fn closed() -> Matching {
    let mut view = Session::new(false, String::new(), Vec::new()).view();
    view.open = false;
    view
}
