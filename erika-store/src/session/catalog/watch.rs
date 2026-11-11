//! Account deliveries are session-local in browsers, never included in exports.
use super::*;
use crate::{catalog::WatchSync, catalog_watch};

impl Db {
    pub fn catalog_watched_all(&self) -> Result<Vec<Watched>> {
        Ok(self.read(|s| {
            let mut records: Vec<_> = s.catalog.watched.values().cloned().collect();
            records.sort_by(|a, b| a.media.cmp(&b.media));
            records
        }))
    }
    pub fn catalog_local_marks(&self) -> Result<Vec<MediaMark>> {
        Ok(self.read(|s| {
            let mut records: Vec<_> = s
                .catalog
                .marks
                .iter()
                .filter(|m| m.reference.is_none())
                .cloned()
                .collect();
            records.sort_by(|a, b| a.media.cmp(&b.media));
            records
        }))
    }
    pub fn catalog_watch_enqueue(&self, input: &WatchSync) -> Result<WatchSync> {
        self.edit(|s| {
            catalog_watch::validate(
                input,
                s.catalog.node(&input.media)?,
                s.catalog.watched.get(&input.media),
                &s.catalog.links,
            )?;
            let dedup = catalog_watch::dedup(input)?;
            for previous in s.catalog.watch.values() {
                if catalog_watch::dedup(previous)? == dedup {
                    return Ok(previous.clone());
                }
            }
            let mut value = input.clone();
            value.operation = MediaId::new().0;
            value.error.clear();
            value.revision = 1;
            value.lease_until = 0;
            s.catalog
                .watch
                .insert(value.operation.clone(), value.clone());
            Ok(value)
        })
    }
    pub fn catalog_watch_queue(&self, account: &str) -> Result<Vec<WatchSync>> {
        Ok(self.read(|s| {
            s.catalog
                .watch
                .values()
                .filter(|v| v.account == account)
                .cloned()
                .collect()
        }))
    }
    pub fn catalog_watch_recover(&self, account: &str, at: i64) -> Result<usize> {
        if at < 0 {
            return Err(Error::invalid("恢复时间不正确"));
        }
        self.edit(|s| {
            let mut count = 0;
            for task in s
                .catalog
                .watch
                .values_mut()
                .filter(|v| v.account == account)
            {
                if let Some(value) = catalog_watch::recover(task, at)? {
                    *task = value;
                    count += 1;
                }
            }
            Ok(count)
        })
    }
    pub fn catalog_watch_transition(
        &self,
        expected: &WatchSync,
        state: &str,
        error: &str,
        observed: Option<bool>,
    ) -> Result<WatchSync> {
        self.edit(|s| {
            let current = s
                .catalog
                .watch
                .get(&expected.operation)
                .ok_or_else(|| Error::conflict("观看同步任务已改变"))?;
            let value = catalog_watch::transition(
                current,
                expected,
                state,
                error,
                observed,
                s.catalog.watched.get(&current.media),
                &s.catalog.links,
            )?;
            if value.state == "sending"
                && s.catalog
                    .watch
                    .values()
                    .any(|p| catalog_watch::blocks(p, &value))
            {
                return Err(Error::conflict("此远端目标尚有未确认的写入"));
            }
            s.catalog
                .watch
                .insert(value.operation.clone(), value.clone());
            Ok(value)
        })
    }
}
