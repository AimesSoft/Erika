//! SQLite adapter for the shared account delivery policy.
use super::{
    Db,
    catalog::{decode, json, transaction},
};
use crate::{
    account_sync::{Queue, View},
    catalog::{Error, MediaMark, Result, WatchSync},
};
use erika_catalog::{MediaId, MediaNode, ProviderLink, ProviderRef};
use rusqlite::{OptionalExtension, params};

struct ReadView<'a>(&'a Db);
impl View for ReadView<'_> {
    fn node(&self, id: &MediaId) -> Result<Option<MediaNode>> {
        self.0.catalog_node(id)
    }
    fn links(&self, id: &MediaId) -> Result<Vec<ProviderLink>> {
        self.0.catalog_links(id)
    }
    fn assigned(&self, reference: &ProviderRef) -> Result<Vec<MediaId>> {
        Ok(self
            .0
            .catalog_by_ref(reference)?
            .into_iter()
            .map(|n| n.id)
            .collect())
    }
    fn mark(&self, id: &MediaId) -> Result<Option<MediaMark>> {
        super::catalog::mark(&self.0.conn, id, &None)
    }
    fn watched(&self, id: &MediaId) -> Result<Option<erika_catalog::Watched>> {
        self.0.catalog_watched(id)
    }
    fn watches(&self, account: &str) -> Result<Vec<WatchSync>> {
        self.0.catalog_watch_queue(account)
    }
    fn transition_watch(
        &mut self,
        expected: &WatchSync,
        state: &str,
        error: &str,
        observed: Option<bool>,
    ) -> Result<WatchSync> {
        let current = self
            .watches(&expected.account)?
            .into_iter()
            .find(|t| t.operation == expected.operation)
            .ok_or_else(|| Error::conflict("观看同步任务已改变"))?;
        let value = crate::catalog_watch::transition(
            &current,
            expected,
            state,
            error,
            observed,
            self.0.catalog_watched(&current.media)?.as_ref(),
            &self.links(&current.media)?,
        )?;
        if value.state == "sending"
            && self
                .watches(&current.account)?
                .iter()
                .any(|t| crate::catalog_watch::blocks(t, &value))
        {
            return Err(Error::conflict("此远端目标尚有未确认的写入"));
        }
        self.0.conn.execute(
            "UPDATE account_watch_sync SET state=?2,data=?3 WHERE operation=?1",
            params![value.operation, value.state, json(&value)?],
        )?;
        Ok(value)
    }
}
impl Db {
    fn edit_account<T>(
        &self,
        account: &str,
        edit: impl FnOnce(&mut Queue, &mut dyn View) -> Result<T>,
    ) -> Result<T> {
        let tx = transaction(&self.conn)?;
        let data: Option<String> = tx
            .query_row(
                "SELECT data FROM account_sync WHERE account=?1",
                [account],
                |r| r.get(0),
            )
            .optional()?;
        let mut queue = data.map(decode).transpose()?.unwrap_or_default();
        let value = edit(&mut queue, &mut ReadView(self))?;
        tx.execute("INSERT INTO account_sync(account,data) VALUES(?1,?2) ON CONFLICT(account) DO UPDATE SET data=excluded.data", params![account, json(&queue)?])?;
        tx.commit()?;
        Ok(value)
    }
    crate::account_sync::account_api!();
}
