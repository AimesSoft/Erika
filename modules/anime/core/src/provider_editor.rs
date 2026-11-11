//! Explicit links for current TMDB identities. The editor keeps the revisions
//! it displayed; a concurrent CLI/plugin edit cannot be silently overwritten.
use super::*;
use anime::{MappingKind, MappingProvider, MappingRelation, MappingRow, MappingTarget};
use erika_catalog::{Kind, MediaId, MediaNode, Provider, ProviderLink, ProviderRef, Relation};
use erika_store::catalog::{CatalogBatch, Error, Result};

#[derive(Default)]
pub(super) struct Editor {
    target: Option<MediaId>,
    links: Vec<(String, ProviderLink)>,
}

fn target(node: &MediaNode) -> MappingTarget {
    let title = match node.kind {
        Kind::Season => format!("S{:02} · {}", node.season_number.unwrap(), node.title),
        Kind::Episode => format!(
            "S{:02}E{:02} · {}",
            node.season_number.unwrap(),
            node.episode_number.unwrap(),
            node.title
        ),
        _ => node.title.clone(),
    };
    MappingTarget {
        id: node.id.0.clone(),
        title,
    }
}
fn relation(value: &MappingRelation) -> Relation {
    match value {
        MappingRelation::Exact => Relation::Exact,
        MappingRelation::PartOf => Relation::PartOf,
        MappingRelation::Contains => Relation::Contains,
        MappingRelation::Related => Relation::Related,
    }
}
fn relation_view(value: Relation) -> MappingRelation {
    match value {
        Relation::Exact => MappingRelation::Exact,
        Relation::PartOf => MappingRelation::PartOf,
        Relation::Contains => MappingRelation::Contains,
        Relation::Related => MappingRelation::Related,
    }
}

impl AnimeEngine {
    pub(super) fn open_provider_editor(&mut self) -> Result<()> {
        let root = self
            .db
            .catalog_node(&MediaId::parse(&self.state.opened)?)?
            .filter(|n| n.kind.work())
            .ok_or_else(|| Error::invalid("请先打开已识别的作品"))?;
        let mut targets = vec![target(&root)];
        let mut seasons = self.db.catalog_children(&root.id)?;
        seasons.sort_by_key(|n| n.season_number);
        for season in seasons {
            targets.push(target(&season));
            let mut episodes = self.db.catalog_children(&season.id)?;
            episodes.sort_by_key(|n| n.episode_number);
            targets.extend(episodes.iter().map(target));
        }
        self.state.mapping_targets = targets;
        let selected = if self.state.selected_season.is_empty() {
            root.id.0
        } else {
            self.state.selected_season.clone()
        };
        self.state.mapping_open = true;
        self.select_provider_target(&selected)?;
        Ok(())
    }

    pub(super) fn select_provider_target(&mut self, id: &str) -> Result<()> {
        if !self.state.mapping_open || !self.state.mapping_targets.iter().any(|t| t.id == id) {
            return Err(Error::invalid("不在当前作品的关联范围内"));
        }
        let media = MediaId::parse(id)?;
        let node = self
            .db
            .catalog_node(&media)?
            .filter(|n| n.root.0 == self.state.opened)
            .ok_or_else(|| Error::invalid("媒体已改变，请重新打开关联面板"))?;
        let links: Vec<_> = self
            .db
            .catalog_links(&media)?
            .into_iter()
            .filter(|l| {
                matches!(
                    l.reference.provider,
                    Provider::Bangumi | Provider::Dandanplay
                ) && l.reference.scope.is_empty()
                    && matches!(l.reference.kind, Kind::Subject | Kind::Episode)
            })
            .map(|l| (MediaId::new().0, l))
            .collect();
        self.state.mapping_rows = links
            .iter()
            .filter(|(_, l)| l.confirmed)
            .map(|(id, l)| MappingRow {
                id: id.clone(),
                provider: if l.reference.provider == Provider::Bangumi {
                    MappingProvider::Bangumi
                } else {
                    MappingProvider::Dandanplay
                },
                kind: if l.reference.kind == Kind::Subject {
                    MappingKind::Subject
                } else {
                    MappingKind::Episode
                },
                reference: l.reference.id.clone(),
                relation: relation_view(l.relation),
            })
            .collect();
        self.state.mapping_target = id.into();
        self.state.mapping_movie = node.kind == Kind::Movie;
        self.state.mapping_episode = node.kind == Kind::Episode;
        self.state.mapping_error.clear();
        self.provider_editor = Editor {
            target: Some(media),
            links,
        };
        Ok(())
    }

    fn editor_target(&self) -> Result<MediaId> {
        self.provider_editor
            .target
            .clone()
            .filter(|id| self.state.mapping_open && id.0 == self.state.mapping_target)
            .ok_or_else(|| Error::invalid("请先打开关联面板"))
    }

    pub(super) fn save_mapping(
        &mut self,
        provider: MappingProvider,
        kind: MappingKind,
        relation_value: MappingRelation,
        reference: &str,
    ) -> Result<()> {
        let media = self.editor_target()?;
        let external = reference
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| Error::invalid("请输入提供方条目的正整数 ID"))?;
        let reference = ProviderRef {
            provider: match provider {
                MappingProvider::Bangumi => Provider::Bangumi,
                MappingProvider::Dandanplay => Provider::Dandanplay,
            },
            scope: String::new(),
            kind: match kind {
                MappingKind::Subject => Kind::Subject,
                MappingKind::Episode => Kind::Episode,
            },
            id: external.to_string(),
        };
        let previous = self
            .provider_editor
            .links
            .iter()
            .find(|(_, l)| l.reference == reference)
            .map(|(_, l)| l);
        let link = ProviderLink {
            media,
            reference,
            relation: relation(&relation_value),
            coverage: previous.map(|l| l.coverage.clone()).unwrap_or_default(),
            confirmed: true,
            manual: true,
            evidence: "manual:details".into(),
            revision: previous.map_or(0, |l| l.revision),
        };
        self.commit_mapping(link)
    }

    pub(super) fn remove_mapping(&mut self, row: &str) -> Result<()> {
        self.editor_target()?;
        let mut link = self
            .provider_editor
            .links
            .iter()
            .find(|(id, l)| id == row && l.confirmed)
            .map(|(_, l)| l.clone())
            .ok_or_else(|| Error::invalid("关联已改变，请刷新"))?;
        link.confirmed = false;
        link.manual = true;
        link.evidence = "manual:details:revoke".into();
        self.commit_mapping(link)
    }

    fn commit_mapping(&mut self, link: ProviderLink) -> Result<()> {
        let media = link.media.0.clone();
        self.db.catalog_request(
            &MediaId::new().0,
            &CatalogBatch {
                links: vec![link],
                ..Default::default()
            },
            false,
        )?;
        // Invalidate in-flight observations; linking alone does not send account
        // writes or invent episode mappings from a subject's episode numbers.
        self.syncs.change_view(&mut self.state);
        self.reviews.cancel();
        self.state.comments.clear();
        self.state.has_comments = false;
        self.state.comments_source.clear();
        self.state.comments_error.clear();
        self.state.comments_loading = false;
        self.state.review_open = false;
        self.state.rating_open = false;
        self.show_opened();
        self.select_provider_target(&media)?;
        Ok(())
    }

    pub(super) fn mapping_result(
        &mut self,
        result: Result<()>,
    ) -> std::result::Result<(), CommandError> {
        if let Err(error) = &result {
            self.state.mapping_error = error.message.clone();
        }
        result.map_err(content::command)
    }
}

#[cfg(test)]
mod tests;
