//! Browser adapter: the same policy under the shared rollback-capable lock.
use super::*;
use crate::{
    account_sync::{Queue, View},
    catalog::WatchSync,
};

impl View for State {
    fn node(&self, id: &MediaId) -> Result<Option<MediaNode>> {
        Ok(self.catalog.nodes.get(id).cloned())
    }
    fn links(&self, id: &MediaId) -> Result<Vec<ProviderLink>> {
        Ok(self
            .catalog
            .links
            .iter()
            .filter(|l| &l.media == id)
            .cloned()
            .collect())
    }
    fn assigned(&self, reference: &ProviderRef) -> Result<Vec<MediaId>> {
        Ok(self
            .catalog
            .nodes
            .values()
            .filter(|n| {
                n.reference.as_ref() == Some(reference)
                    || self.catalog.links.iter().any(|l| {
                        l.media == n.id
                            && l.reference == *reference
                            && l.confirmed
                            && l.relation == Relation::Exact
                    })
            })
            .map(|n| n.id.clone())
            .collect())
    }
    fn mark(&self, id: &MediaId) -> Result<Option<MediaMark>> {
        Ok(self
            .catalog
            .marks
            .iter()
            .find(|m| &m.media == id && m.reference.is_none())
            .cloned())
    }
    fn watched(&self, id: &MediaId) -> Result<Option<erika_catalog::Watched>> {
        Ok(self.catalog.watched.get(id).cloned())
    }
    fn watches(&self, account: &str) -> Result<Vec<WatchSync>> {
        Ok(self
            .catalog
            .watch
            .values()
            .filter(|t| t.account == account)
            .cloned()
            .collect())
    }
    fn transition_watch(
        &mut self,
        expected: &WatchSync,
        state: &str,
        error: &str,
        observed: Option<bool>,
    ) -> Result<WatchSync> {
        let current = self
            .catalog
            .watch
            .get(&expected.operation)
            .ok_or_else(|| Error::conflict("观看同步任务已改变"))?;
        let value = crate::catalog_watch::transition(
            current,
            expected,
            state,
            error,
            observed,
            self.catalog.watched.get(&current.media),
            &self.catalog.links,
        )?;
        if value.state == "sending"
            && self
                .catalog
                .watch
                .values()
                .any(|t| crate::catalog_watch::blocks(t, &value))
        {
            return Err(Error::conflict("此远端目标尚有未确认的写入"));
        }
        self.catalog
            .watch
            .insert(value.operation.clone(), value.clone());
        Ok(value)
    }
}
impl Db {
    fn edit_account<T>(
        &self,
        account: &str,
        edit: impl FnOnce(&mut Queue, &mut dyn View) -> Result<T>,
    ) -> Result<T> {
        self.edit(|s| {
            let mut queue = s.catalog.accounts.remove(account).unwrap_or_default();
            let value = edit(&mut queue, s)?;
            s.catalog.accounts.insert(account.into(), queue);
            Ok(value)
        })
    }
    crate::account_sync::account_api!();
}
