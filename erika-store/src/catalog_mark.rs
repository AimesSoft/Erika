//! Shared native/browser validation and field-wise local catalogue edits.
use crate::catalog::{Error, MarkEdit, MediaMark, Result};
use erika_catalog::{Kind, MediaId, MediaNode};

impl MediaMark {
    pub fn local(media: MediaId) -> Self {
        Self {
            media,
            reference: None,
            status: None,
            rating: None,
            review: None,
            updated_at: 0,
            revision: 0,
        }
    }

    pub(crate) fn edit(&mut self, edit: &MarkEdit, at: i64) {
        match edit {
            MarkEdit::Status(value) => self.status = Some(value.clone()),
            MarkEdit::Rating(value) => self.rating = Some(*value),
            MarkEdit::Review(value) => self.review = Some(value.clone()),
        }
        self.updated_at = at;
    }

    pub(crate) fn imported(&self, previous: Option<&Self>, replace: bool) -> Result<Option<Self>> {
        let mut value = self.clone();
        value.revision = previous.map_or(0, |m| m.revision);
        if let Some(previous) = previous {
            if previous == &value {
                return Ok(None);
            }
            if !replace
                && (previous.status != value.status
                    || previous.rating != value.rating
                    || previous.review != value.review)
            {
                return Err(Error::conflict("远端收藏、评分或短评与本地冲突"));
            }
        }
        Ok(Some(value))
    }

    pub(crate) fn stored(&self, node: &MediaNode, previous: Option<&Self>) -> Result<Self> {
        if self.media != node.id
            || !matches!(node.kind, Kind::Tv | Kind::Season | Kind::Movie)
            || self.rating.is_some_and(|n| !(0..=10).contains(&n))
            || self
                .status
                .as_deref()
                .is_some_and(|s| !matches!(s, "want" | "watching" | "watched" | "dropped"))
            || self
                .review
                .as_ref()
                .is_some_and(|s| s.chars().count() > 200)
            || self.updated_at < 0
        {
            return Err(Error::invalid("作品收藏、评分或短评格式不正确"));
        }
        if let Some(reference) = &self.reference {
            reference.validate()?;
        }
        if previous.map_or(0, |m| m.revision) != self.revision {
            return Err(Error::conflict("作品收藏记录已改变"));
        }
        let mut saved = self.clone();
        saved.revision = self
            .revision
            .checked_add(1)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or_else(|| Error::conflict("收藏记录版本超出范围"))?;
        Ok(saved)
    }
}
