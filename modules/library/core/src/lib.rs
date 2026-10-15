//! First-phase sample catalog from design/next-core.js. No scanning or
//! network connections are started by this UI implementation.
use bern::{CommandError, media::MediaSource, query::Answer, resources::Resources};
use erika_library_protocol::library::{
    self, LibraryFilter, LibraryFilterChip, LibraryLetter, LibraryListing, LibraryPoster,
    LibrarySort, LibrarySource, State,
};

const SHOWS: &[(i64, &str, &str, i64, i64, f64)] = &[
    (1, "寒蝉鸣泣之时 解", "H", 24, 1, 8.4),
    (2, "只有神知道的世界 女神篇", "Z", 12, 11, 7.9),
    (3, "【我推的孩子】 第三季", "W", 11, 2, 7.8),
    (4, "超时空辉夜姬！", "C", 1, 0, 7.7),
    (5, "咒术回战 死灭回游", "Z", 12, 3, 7.6),
    (6, "为美好的世界献上祝福！3", "W", 11, 11, 7.9),
    (7, "葬送的芙莉莲 第二季", "Z", 10, 3, 8.4),
    (8, "剧场版 魔法少女小圆", "J", 1, 0, 8.4),
    (9, "Fate/strange Fake", "F", 13, 3, 8.0),
    (10, "地狱乐 第二季", "D", 12, 2, 6.7),
    (11, "鬼灭之刃 无限城篇", "G", 1, 0, 0.0),
    (12, "寒蝉鸣泣之时 礼", "H", 5, 0, 7.3),
    (13, "魔都精兵的奴隶 第二季", "M", 12, 0, 6.1),
    (14, "现在多闻君是哪一面！？", "X", 12, 0, 5.8),
    (15, "优雅贵族的休假指南。", "Y", 12, 0, 4.9),
    (16, "厄里斯的圣杯", "E", 12, 0, 5.4),
];
const SERVER: &[i64] = &[7, 9, 3, 5, 10, 6, 1, 12, 2];
const SHARED: &[i64] = &[6, 7, 13, 14, 1, 2];
// Within an initial, follow the prototype's Chinese title collation.
const NAME_ORDER: &[i64] = &[4, 10, 16, 9, 11, 1, 12, 8, 13, 3, 6, 14, 15, 7, 2, 5];

pub struct LibraryEngine {
    state: State,
}
impl Default for LibraryEngine {
    fn default() -> Self {
        let posters: Vec<_> = SHOWS
            .iter()
            .map(
                |&(id, title, initial, episodes, watched, rating)| LibraryPoster {
                    id,
                    title: title.into(),
                    initial: initial.into(),
                    episodes,
                    watched,
                    rating,
                    poster: MediaSource::resource(
                        &Resources::host(),
                        &format!("design/assets/posters/p{id}.jpg"),
                    )
                    .unwrap_or_else(|_| MediaSource::new("")),
                    progress: watched as f64 / episodes as f64,
                    watching: watched > 0 && watched < episodes,
                    movie: episodes == 1,
                },
            )
            .collect();
        Self {
            state: State {
                show: posters[0].clone(),
                posters,
                opened: 0,
                notice: 0,
            },
        }
    }
}
impl LibraryEngine {
    fn listing(
        &self,
        source: LibrarySource,
        filter: LibraryFilter,
        sort: LibrarySort,
    ) -> LibraryListing {
        let ids: Vec<i64> = match source {
            LibrarySource::Local => self.state.posters.iter().map(|p| p.id).collect(),
            LibrarySource::Server => SERVER.to_vec(),
            LibrarySource::Shared => SHARED.to_vec(),
        };
        let mut posters: Vec<_> = ids
            .iter()
            .filter_map(|id| self.state.posters.iter().find(|p| p.id == *id).cloned())
            .collect();
        let total = posters.len() as i64;
        let watching = posters.iter().filter(|p| p.watching).count() as i64;
        let unwatched = posters.iter().filter(|p| p.watched == 0).count() as i64;
        let finished = total - watching - unwatched;
        posters.retain(|p| match filter {
            LibraryFilter::All => true,
            LibraryFilter::Watching => p.watching,
            LibraryFilter::Unwatched => p.watched == 0,
            LibraryFilter::Finished => p.watched >= p.episodes,
        });
        match sort {
            LibrarySort::Recent => posters.sort_by_key(|p| !p.watching),
            LibrarySort::Name => {
                posters.sort_by_key(|p| NAME_ORDER.iter().position(|id| *id == p.id))
            }
            LibrarySort::Rating => posters.sort_by(|a, b| b.rating.total_cmp(&a.rating)),
        }
        let letters = "ABCDEFGHJKLMNOPQRSTWXYZ"
            .chars()
            .map(|c| {
                let letter = c.to_string();
                let target = posters
                    .iter()
                    .find(|p| p.initial == letter)
                    .map_or(0, |p| p.id);
                LibraryLetter { letter, target }
            })
            .collect();
        let filters = [
            ("all", LibraryFilter::All, total),
            ("watching", LibraryFilter::Watching, watching),
            ("unwatched", LibraryFilter::Unwatched, unwatched),
            ("finished", LibraryFilter::Finished, finished),
        ]
        .into_iter()
        .map(|(id, value, count)| LibraryFilterChip {
            id: id.into(),
            value,
            count,
        })
        .collect();
        LibraryListing {
            filters,
            empty: posters.is_empty(),
            posters,
            letters,
            total,
            watching,
            unwatched,
            finished,
        }
    }
}
impl library::Commands for LibraryEngine {
    fn open(&mut self, id: i64) -> Result<(), CommandError> {
        let show = self
            .state
            .posters
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| CommandError::new("unknown_show", "unknown library show"))?;
        self.state.show = show;
        self.state.opened = id;
        library::emit::open_show(id);
        Ok(())
    }
    fn close(&mut self) -> Result<(), CommandError> {
        self.state.opened = 0;
        Ok(())
    }
    fn manage(&mut self) -> Result<(), CommandError> {
        self.state.notice += 1;
        Ok(())
    }
}
impl library::Queries for LibraryEngine {
    fn browse(
        &self,
        source: LibrarySource,
        filter: LibraryFilter,
        sort: LibrarySort,
        answer: Answer<LibraryListing>,
    ) {
        answer.ok(self.listing(source, filter, sort));
    }
}
impl library::Engine for LibraryEngine {
    fn state(&self) -> &State {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_wall_matches_the_design() {
        let result = LibraryEngine::default().listing(
            LibrarySource::Local,
            LibraryFilter::All,
            LibrarySort::Recent,
        );
        assert_eq!(
            (
                result.total,
                result.watching,
                result.unwatched,
                result.finished
            ),
            (16, 7, 8, 1)
        );
        assert_eq!(
            result.posters.iter().map(|p| p.id).collect::<Vec<_>>(),
            [1, 2, 3, 5, 7, 9, 10, 4, 6, 8, 11, 12, 13, 14, 15, 16]
        );
    }
    #[test]
    fn filters_and_index_use_only_the_selected_source() {
        let engine = LibraryEngine::default();
        let result = engine.listing(
            LibrarySource::Server,
            LibraryFilter::Unwatched,
            LibrarySort::Name,
        );
        assert_eq!(result.total, 9);
        assert_eq!(
            result.posters.iter().map(|p| p.id).collect::<Vec<_>>(),
            [12]
        );
        assert_eq!(
            result
                .letters
                .iter()
                .filter(|l| l.target > 0)
                .map(|l| l.target)
                .collect::<Vec<_>>(),
            [12]
        );
        let shared = engine.listing(
            LibrarySource::Shared,
            LibraryFilter::Finished,
            LibrarySort::Recent,
        );
        assert_eq!(shared.total, 6);
        assert_eq!(shared.posters[0].id, 6);
    }
    #[test]
    fn sorting_is_stable_and_indices_target_visible_posters() {
        let engine = LibraryEngine::default();
        let named = engine.listing(LibrarySource::Local, LibraryFilter::All, LibrarySort::Name);
        assert_eq!(
            named.posters.iter().map(|p| p.id).collect::<Vec<_>>(),
            NAME_ORDER
        );
        let rated = engine.listing(
            LibrarySource::Local,
            LibraryFilter::All,
            LibrarySort::Rating,
        );
        assert_eq!(rated.posters[0].id, 1);
        assert_eq!(rated.posters.last().unwrap().id, 11);
        for result in [named, rated] {
            for letter in result.letters.iter().filter(|l| l.target != 0) {
                assert_eq!(
                    result
                        .posters
                        .iter()
                        .find(|p| p.initial == letter.letter)
                        .unwrap()
                        .id,
                    letter.target
                );
            }
        }
    }
}
