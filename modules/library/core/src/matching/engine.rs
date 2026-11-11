use super::*;
use crate::{Done, LibraryEngine};
use bern::CommandError;

pub(crate) struct Reply {
    pub session: String,
    pub generation: u64,
    pub result: Result<Loaded, String>,
}
pub(crate) enum Loaded {
    Search(Vec<MediaNode>),
    Work(Work),
    Season(Season),
}

enum Request {
    Search(String),
    Work(erika_catalog::ProviderRef),
    Season(Work, u32),
}
impl Request {
    async fn load(self, client: erika_catalog::Client) -> Result<Loaded, String> {
        match self {
            Self::Search(text) => {
                let mut nodes = Vec::new();
                for kind in [Kind::Tv, Kind::Movie] {
                    nodes.extend(
                        client
                            .search(kind, &text, None, 1)
                            .await
                            .map_err(|e| e.message)?
                            .results,
                    );
                }
                let mut seen = HashSet::new();
                nodes.retain(|n| seen.insert(n.id.clone()));
                Ok(Loaded::Search(nodes))
            }
            Self::Work(reference) => {
                let work = client.work(&reference).await.map_err(|e| e.message)?;
                Ok(Loaded::Work(client.with_artwork(work).await))
            }
            Self::Season(work, number) => client
                .season(&work, number)
                .await
                .map(Loaded::Season)
                .map_err(|e| e.message),
        }
    }
}

impl LibraryEngine {
    fn request_match(&mut self, request: Request) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        session.generation += 1;
        session.error.clear();
        let (id, generation, inbox, wake) = (
            session.id.clone(),
            session.generation,
            self.inbox.clone(),
            bern::wake::waker(),
        );
        let client = self.metadata.clone();
        let work = async move {
            let result = request.load(client).await;
            inbox
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Done::Picker(Reply {
                    session: id,
                    generation,
                    result,
                }));
            if let Some(wake) = wake {
                wake.wake();
            }
        };
        #[cfg(test)]
        futures_lite::future::block_on(work);
        #[cfg(not(test))]
        erika_catalog::client::spawn(work);
    }
    pub(crate) fn begin(&mut self, session: Session, words: String) {
        self.session = Some(session);
        self.search(words);
    }
    pub(crate) fn search(&mut self, text: String) {
        let text = text.trim().to_string();
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if text.is_empty() {
            return;
        }
        session.results.clear();
        session.selected = None;
        session.work = None;
        session.season = None;
        session.selected_season.clear();
        session.loading = false;
        session.searching = true;
        self.request_match(Request::Search(text));
    }
    pub(crate) fn choose_work(&mut self, id: String) -> Result<(), CommandError> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        let node = session
            .results
            .iter()
            .find(|n| n.id.0 == id)
            .cloned()
            .ok_or_else(|| CommandError::new("unknown_work", "请选择搜索结果中的作品"))?;
        let reference = node
            .reference
            .clone()
            .ok_or_else(|| CommandError::new("invalid_identity", "作品缺少 TMDB 身份"))?;
        session.selected = Some(node);
        session.work = None;
        session.season = None;
        session.selected_season.clear();
        session.loading = true;
        session.searching = false;
        self.request_match(Request::Work(reference));
        Ok(())
    }
    pub(crate) fn choose_season(&mut self, id: String) -> Result<(), CommandError> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        let work = session
            .work
            .clone()
            .ok_or_else(|| CommandError::new("no_work", "请先选择作品"))?;
        let number = work
            .seasons
            .iter()
            .find(|s| s.id.0 == id)
            .and_then(|s| s.season_number)
            .ok_or_else(|| CommandError::new("no_season", "请选择作品中的季"))?;
        session.selected_season = id;
        session.season = None;
        session.loading = true;
        self.request_match(Request::Season(work, number));
        Ok(())
    }
    pub(crate) fn take_picker(&mut self, reply: Reply) {
        let Some(session) = self
            .session
            .as_mut()
            .filter(|s| s.id == reply.session && s.generation == reply.generation)
        else {
            return;
        };
        session.loading = false;
        session.searching = false;
        let mut sole = None;
        match reply.result {
            Ok(Loaded::Search(nodes)) => {
                if nodes.is_empty() {
                    session.error = "没有找到作品，请换一个名称搜索".into();
                }
                session.results = nodes;
            }
            Ok(Loaded::Work(work))
                if session
                    .selected
                    .as_ref()
                    .is_some_and(|s| s.id == work.node.id) =>
            {
                session.selected = Some(work.node.clone());
                if work.node.kind == Kind::Movie {
                    // Movies have versions, not synthetic episode 1. Skipped files
                    // remain skipped; choosing a movie never expands the selection.
                    session.season = None;
                } else if work.seasons.len() == 1 {
                    sole = Some(work.seasons[0].id.0.clone());
                }
                session.work = Some(work);
            }
            Ok(Loaded::Season(season))
                if session.selected_season == season.node.id.0
                    && session
                        .work
                        .as_ref()
                        .is_some_and(|w| season.node.root == w.node.id) =>
            {
                if season.episodes.is_empty() {
                    session.error = "这一季尚无分集资料".into();
                }
                session.season = Some(season);
            }
            Ok(_) => session.error = "返回的媒体身份与当前选择不一致".into(),
            Err(error) => session.error = error,
        }
        if let Some(id) = sole {
            if let Err(error) = self.choose_season(id) {
                self.session.as_mut().unwrap().error = error.message;
            }
        }
    }
    pub(crate) fn confirm_content(&mut self) -> Result<(), CommandError> {
        let Some(session) = self.session.as_ref() else {
            return Ok(());
        };
        let plan = match session.plan() {
            Ok(plan) => plan,
            Err(error) => {
                self.session.as_mut().unwrap().error = error;
                return Ok(());
            }
        };
        let title = session.work.as_ref().unwrap().node.title.clone();
        match self
            .db
            .catalog_apply_selection(&plan.addresses, &plan.nodes, &plan.bindings)
        {
            Ok(_) => {
                self.notify(crate::LibraryNotice::Matched {
                    count: plan.bindings.len() as i64,
                    title,
                });
                self.session = None;
                crate::library::emit::files_changed();
            }
            Err(error) => {
                self.session.as_mut().unwrap().error = if error.code == "conflict" {
                    "文件绑定已改变，整批均未保存；请关闭后重新打开匹配".into()
                } else {
                    error.message
                };
            }
        }
        Ok(())
    }
    pub(crate) fn block_content(&self, video: &Video) -> Result<(), CommandError> {
        let file = video.file.ok_or_else(crate::unknown_file)?;
        self.db
            .catalog_apply_selection(
                &video.address.iter().cloned().collect::<Vec<_>>(),
                &[],
                &[FileBinding {
                    file,
                    revision: video.revision,
                    decision: Decision::Blocked,
                    items: Vec::new(),
                    evidence: "manual:library-unlink-content".into(),
                }],
            )
            .map(|_| ())
            .map_err(|e| CommandError::new(e.code, e.message))
    }
}
