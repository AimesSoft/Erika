//! Durable recognition jobs, independent account deliveries, and scan leases.
use super::{
    Db,
    catalog::{decode, json, revision, transaction},
};
use crate::catalog::{CatalogJob, Error, Result, WatchSync};
use erika_catalog::MediaId;
use rusqlite::{OptionalExtension, params};

fn job_state(state: &str) -> bool {
    matches!(
        state,
        "queued" | "running" | "retry" | "pending" | "bound" | "completed" | "failed" | "cancelled" | "stale"
    )
}
impl Db {
    pub fn catalog_job_enqueue(&self, job: &CatalogJob) -> Result<()> {
        MediaId::parse(&job.id)?;
        if job.state != "queued"
            || job.payload.len() > 1024 * 1024
            || job.phase.len() > 128
            || job.error.len() > 8192
            || job.next_attempt < 0
        {
            return Err(Error::invalid("识别任务格式不正确"));
        }
        let tx = transaction(&self.conn)?;
        if let Some(file) = job.file {
            if revision(&tx, file)? != job.input_revision {
                return Err(Error::conflict("任务所用文件版本已改变"));
            }
            let active = {
                let mut stmt = tx.prepare("SELECT data,lease_until FROM catalog_job WHERE file_id=?1 AND state IN ('queued','running','retry')")?;
                stmt.query_map([file], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
            };
            let at = crate::now();
            for (data, lease) in active {
                let mut previous: CatalogJob = decode(data)?;
                if previous.input_revision == job.input_revision
                    && (previous.state == "queued"
                        || (previous.state == "running" && lease >= at)
                        || (previous.state == "retry" && previous.next_attempt > at))
                {
                    return Err(Error::conflict("此文件已有识别任务"));
                }
                previous.state = "stale".into();
                previous.error = "识别执行已被新任务替代".into();
                tx.execute(
                    "UPDATE catalog_job SET state='stale',lease_until=0,data=?2 WHERE id=?1",
                    params![previous.id, json(&previous)?],
                )?;
            }
        }
        tx.execute(
            "INSERT INTO catalog_job(id,file_id,state,next_attempt,data) VALUES(?1,?2,?3,?4,?5)",
            params![job.id, job.file, job.state, job.next_attempt, json(job)?],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Recover abandoned read-only work without reusing its operation id. A
    /// late response for the expired run can never apply to the replacement.
    pub fn catalog_file_job(&self, file: i64, at: i64) -> Result<Option<CatalogJob>> {
        let tx = transaction(&self.conn)?;
        let current: Option<(String, i64)> = tx.query_row(
            "SELECT data,lease_until FROM catalog_job WHERE file_id=?1 ORDER BY rowid DESC LIMIT 1",
            [file], |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        let result = if let Some((data, lease_until)) = current {
            let mut job: CatalogJob = decode(data)?;
            if job.state == "running" && lease_until < at {
                job.state = "stale".into();
                job.error = "识别执行已过期，可重新创建任务".into();
                tx.execute(
                    "UPDATE catalog_job SET state='stale',data=?2 WHERE id=?1",
                    params![job.id, json(&job)?],
                )?;
            }
            Some(job)
        } else {
            None
        };
        tx.commit()?;
        Ok(result)
    }

    pub fn catalog_jobs(&self, state: Option<&str>) -> Result<Vec<CatalogJob>> {
        if state.is_some_and(|s| !job_state(s)) {
            return Err(Error::invalid("未知任务状态"));
        }
        let mut stmt=self.conn.prepare("SELECT data FROM catalog_job WHERE ?1 IS NULL OR state=?1 ORDER BY next_attempt,id LIMIT 1000")?;
        stmt.query_map([state], |r| r.get::<_, String>(0))?
            .map(|r| decode(r?))
            .collect()
    }
    /// Claim exactly once. A failed CAS is a conflict, not a second worker.
    pub fn catalog_job_transition(
        &self,
        id: &str,
        expected: &str,
        state: &str,
        phase: &str,
        error: &str,
        next_attempt: i64,
    ) -> Result<CatalogJob> {
        let allowed = matches!(
            (expected, state),
            ("queued" | "retry", "running" | "cancelled")
                | (
                    "running",
                    "retry" | "pending" | "failed" | "cancelled" | "stale"
                )
                | ("pending", "queued" | "cancelled")
                | ("failed", "queued")
        );
        if !allowed || phase.len() > 128 || error.len() > 8192 || next_attempt < 0 {
            return Err(Error::invalid("识别任务状态转换不正确"));
        }
        let tx = transaction(&self.conn)?;
        let mut value: CatalogJob = tx
            .query_row(
                "SELECT data FROM catalog_job WHERE id=?1 AND state=?2",
                params![id, expected],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(decode)
            .transpose()?
            .ok_or_else(|| Error::conflict("任务已被其他执行者修改"))?;
        if state == "running" && value.next_attempt > crate::now() {
            return Err(Error::conflict("任务尚未到重试时间"));
        }
        value.state = state.into();
        value.phase = phase.into();
        value.error = error.into();
        value.next_attempt = next_attempt;
        if let Some(file) = value.file {
            if revision(&tx, file)? != value.input_revision {
                value.state = "stale".into();
                value.error = "文件绑定已改变".into();
            } else if matches!(value.state.as_str(), "queued" | "running") {
                let busy: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM catalog_job WHERE file_id=?1 AND id!=?2 AND (state='queued' OR (state='running' AND lease_until>=?3)))", params![file,id,crate::now()], |r|r.get(0))?;
                if busy {
                    return Err(Error::conflict("此文件已有识别任务"));
                }
            }
        }
        tx.execute(
            "UPDATE catalog_job SET state=?2,next_attempt=?3,data=?4,lease_until=?5 WHERE id=?1",
            params![
                id,
                value.state,
                value.next_attempt,
                json(&value)?,
                if value.state == "running" {
                    crate::now().saturating_add(300)
                } else {
                    0
                }
            ],
        )?;
        tx.commit()?;
        Ok(value)
    }
    /// Canonical watch records, including explicit unwatch marks with no local file.
    pub fn catalog_watched_all(&self) -> Result<Vec<erika_catalog::Watched>> {
        let mut stmt = self.conn.prepare("SELECT media_id,watched,manual,updated_at,revision FROM media_watched ORDER BY media_id")?;
        Ok(stmt
            .query_map([], |r| {
                Ok(erika_catalog::Watched {
                    media: MediaId(r.get(0)?),
                    watched: r.get(1)?,
                    manual: r.get(2)?,
                    updated_at: r.get(3)?,
                    revision: {
                        let value: i64 = r.get(4)?;
                        u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(4, value))?
                    },
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn catalog_local_marks(&self) -> Result<Vec<crate::catalog::MediaMark>> {
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM media_mark WHERE reference='null' ORDER BY media_id")?;
        stmt.query_map([], |r| r.get::<_, String>(0))?
            .map(|r| decode(r?))
            .collect()
    }
    pub fn catalog_watch_enqueue(&self, input: &WatchSync) -> Result<WatchSync> {
        let tx = transaction(&self.conn)?;
        let media = self
            .catalog_node(&input.media)?
            .ok_or_else(|| Error::invalid("媒体不存在"))?;
        let watched = self.catalog_watched(&input.media)?;
        let links = self.catalog_links(&input.media)?;
        crate::catalog_watch::validate(input, &media, watched.as_ref(), &links)?;
        let dedup = crate::catalog_watch::dedup(input)?;
        if let Some(previous) = tx
            .query_row(
                "SELECT data FROM account_watch_sync WHERE dedup=?1",
                [&dedup],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            return decode(previous);
        }
        let mut value = input.clone();
        value.operation = MediaId::new().0;
        value.error.clear();
        value.revision = 1;
        value.lease_until = 0;
        tx.execute("INSERT INTO account_watch_sync(operation,media_id,account,dedup,state,data) VALUES(?1,?2,?3,?4,?5,?6)",params![value.operation,value.media.0,value.account,dedup,value.state,json(&value)?])?;
        tx.commit()?;
        Ok(value)
    }
    pub fn catalog_watch_queue(&self, account: &str) -> Result<Vec<WatchSync>> {
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM account_watch_sync WHERE account=?1 ORDER BY operation")?;
        stmt.query_map([account], |r| r.get::<_, String>(0))?
            .map(|r| decode(r?))
            .collect()
    }
    /// Expired sends are uncertain, not failed. Keep them blocked until a readback.
    pub fn catalog_watch_recover(&self, account: &str, at: i64) -> Result<usize> {
        if at < 0 {
            return Err(Error::invalid("恢复时间不正确"));
        }
        let tx = transaction(&self.conn)?;
        let mut count = 0;
        for task in self.catalog_watch_queue(account)? {
            if let Some(value) = crate::catalog_watch::recover(&task, at)? {
                tx.execute(
                    "UPDATE account_watch_sync SET state=?2,data=?3 WHERE operation=?1",
                    params![value.operation, value.state, json(&value)?],
                )?;
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    }
    /// Compare the captured operation, including its generation, before accepting
    /// any delivery or readback. A late result cannot complete a newer attempt.
    pub fn catalog_watch_transition(
        &self,
        expected: &WatchSync,
        state: &str,
        error: &str,
        observed: Option<bool>,
    ) -> Result<WatchSync> {
        let tx = transaction(&self.conn)?;
        let current: WatchSync = tx
            .query_row(
                "SELECT data FROM account_watch_sync WHERE operation=?1",
                [&expected.operation],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(decode)
            .transpose()?
            .ok_or_else(|| Error::conflict("观看同步任务已改变"))?;
        let watched = self.catalog_watched(&current.media)?;
        let links = self.catalog_links(&current.media)?;
        let value = crate::catalog_watch::transition(
            &current,
            expected,
            state,
            error,
            observed,
            watched.as_ref(),
            &links,
        )?;
        if value.state == "sending"
            && self
                .catalog_watch_queue(&value.account)?
                .iter()
                .any(|p| crate::catalog_watch::blocks(p, &value))
        {
            return Err(Error::conflict("此远端目标尚有未确认的写入"));
        }
        tx.execute(
            "UPDATE account_watch_sync SET state=?2,data=?3 WHERE operation=?1",
            params![value.operation, value.state, json(&value)?],
        )?;
        tx.commit()?;
        Ok(value)
    }
    /// Lease heartbeat renews on every scan batch. Expired leases can be taken
    /// over after a crash; a previous owner can neither prune nor finish them.
    pub fn catalog_scan_begin(&self, source: i64, owner: &str, at: i64) -> Result<i64> {
        MediaId::parse(owner)?;
        if source <= super::OPENED || at < 0 || at > i64::MAX - 120 {
            return Err(Error::invalid("扫描参数不正确"));
        }
        let tx = transaction(&self.conn)?;
        let changed=tx.execute("INSERT INTO source_scan(source_id,owner,generation,lease_until,active) VALUES(?1,?2,1,?3,1) ON CONFLICT(source_id) DO UPDATE SET owner=excluded.owner,generation=source_scan.generation+1,lease_until=excluded.lease_until,active=1 WHERE source_scan.active=0 OR source_scan.lease_until<?4",params![source,owner,at+120,at])?;
        if changed == 0 {
            return Err(Error::conflict("此来源已有扫描正在进行"));
        }
        let generation = tx.query_row(
            "SELECT generation FROM source_scan WHERE source_id=?1",
            [source],
            |r| r.get(0),
        )?;
        tx.execute("DELETE FROM source_scan_item WHERE source_id=?1", [source])?;
        tx.commit()?;
        Ok(generation)
    }
    pub fn catalog_scan_saw(
        &self,
        source: i64,
        owner: &str,
        generation: i64,
        paths: &[String],
        at: i64,
    ) -> Result<()> {
        if paths.len() > 100000 || at < 0 || at > i64::MAX - 120 {
            return Err(Error::invalid("扫描批次不正确"));
        }
        let tx = transaction(&self.conn)?;
        let changed=tx.execute("UPDATE source_scan SET lease_until=?4 WHERE source_id=?1 AND owner=?2 AND generation=?3 AND active=1 AND lease_until>=?5",params![source,owner,generation,at+120,at])?;
        if changed == 0 {
            return Err(Error::conflict("扫描世代或持有者已改变"));
        }
        for path in paths {
            if path.len() > 32768 {
                return Err(Error::invalid("文件路径过长"));
            }
            tx.execute("INSERT INTO source_scan_item(source_id,path,generation) VALUES(?1,?2,?3) ON CONFLICT(source_id,path) DO UPDATE SET generation=excluded.generation",params![source,path,generation])?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Validate the lease and store an address in the same transaction. A late
    /// callback cannot write to a deleted/recreated source with a reused row id.
    pub fn catalog_scan_read(
        &self,
        source: i64,
        owner: &str,
        generation: i64,
        url: &str,
        size: i64,
        modified: i64,
        fingerprint: Option<&crate::Fingerprint>,
        at: i64,
    ) -> Result<Option<i64>> {
        if url.len() > 32768 || size < 0 || at < 0 || at > i64::MAX - 120 {
            return Err(Error::invalid("扫描文件格式不正确"));
        }
        let tx = transaction(&self.conn)?;
        let changed = tx.execute(
            "UPDATE source_scan SET lease_until=?4 WHERE source_id=?1 AND owner=?2 AND generation=?3 AND active=1 AND lease_until>=?5",
            params![source, owner, generation, at + 120, at],
        )?;
        if changed == 0 {
            return Err(Error::conflict("扫描已失效，未更新文件"));
        }
        let file = fingerprint.map(|f| self.file(f)).transpose()?;
        tx.execute(
            "INSERT INTO address(source_id,path,file_id,size,modified,seen_at) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(source_id,path) DO UPDATE SET file_id=excluded.file_id,size=excluded.size,modified=excluded.modified,seen_at=excluded.seen_at",
            params![source, url, file, size, modified, at],
        )?;
        tx.execute(
            "INSERT INTO source_scan_item(source_id,path,generation) VALUES(?1,?2,?3) ON CONFLICT(source_id,path) DO UPDATE SET generation=excluded.generation",
            params![source, url, generation],
        )?;
        tx.commit()?;
        Ok(file)
    }
    pub fn catalog_scan_finish(
        &self,
        source: i64,
        owner: &str,
        generation: i64,
        complete: bool,
        at: i64,
    ) -> Result<usize> {
        let tx = transaction(&self.conn)?;
        let changed=tx.execute("UPDATE source_scan SET active=0 WHERE source_id=?1 AND owner=?2 AND generation=?3 AND active=1 AND lease_until>=?4",params![source,owner,generation,at])?;
        if changed == 0 {
            return Err(Error::conflict("扫描已失效，未删除任何地址"));
        }
        let removed = if complete {
            tx.execute("DELETE FROM address WHERE source_id=?1 AND NOT EXISTS(SELECT 1 FROM source_scan_item i WHERE i.source_id=?1 AND i.path=address.path AND i.generation=?2)",params![source,generation])?
        } else {
            0
        };
        tx.execute("DELETE FROM source_scan_item WHERE source_id=?1", [source])?;
        tx.commit()?;
        Ok(removed)
    }
}
