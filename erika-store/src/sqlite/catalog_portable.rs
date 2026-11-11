//! Portable catalogue snapshots. Network outboxes, addresses, scan leases and
//! cache bytes are intentionally excluded: importing never sends an account write.
use super::{
    Db,
    catalog::{
        binding, canonical, decode, put_binding, put_danmaku, put_link, put_mark, put_nodes,
        put_watched, revision, transaction,
    },
};
use crate::{
    catalog::{CatalogSnapshot, Error, PortableBinding, Result},
    portable::FileKey,
};
use erika_catalog::{Decision, MediaId, Watched};
use rusqlite::{Connection, params};
use serde::de::DeserializeOwned;
use std::collections::HashSet;

fn records<T: DeserializeOwned>(conn: &Connection, sql: &str) -> Result<Vec<T>> {
    let mut stmt = conn.prepare(sql)?;
    stmt.query_map([], |r| r.get::<_, String>(0))?
        .map(|r| decode(r?))
        .collect()
}
fn key(conn: &Connection, file: i64) -> Result<FileKey> {
    let (md5, size, item): (Option<String>, Option<i64>, Option<String>) = conn.query_row(
        "SELECT md5,size,server_item FROM file WHERE id=?1",
        [file],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    match (md5, size, item) {
        (Some(md5), Some(size), _) if size >= 0 => Ok(FileKey::Hash {
            md5,
            size: size as u64,
        }),
        (_, _, Some(item)) => Ok(FileKey::Item(item)),
        _ => Err(Error::invalid("文件缺少可同步的身份")),
    }
}
pub(super) fn file(conn: &Connection, key: &FileKey) -> Result<i64> {
    match key {
        FileKey::Hash { md5, size } => {
            let size = i64::try_from(*size).map_err(|_| Error::invalid("文件大小超出范围"))?;
            if md5.len() != 32 || !md5.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error::invalid("文件指纹格式不正确"));
            }
            let md5 = md5.to_ascii_lowercase();
            conn.execute(
                "INSERT INTO file(md5,size) VALUES(?1,?2) ON CONFLICT(md5,size) DO NOTHING",
                params![md5, size],
            )?;
            Ok(conn.query_row(
                "SELECT id FROM file WHERE md5=?1 AND size=?2",
                params![md5, size],
                |r| r.get(0),
            )?)
        }
        FileKey::Item(item) => {
            if item.is_empty() || item.len() > 1024 || item.chars().any(char::is_control) {
                return Err(Error::invalid("服务器文件身份不正确"));
            }
            conn.execute(
                "INSERT INTO file(server_item) VALUES(?1) ON CONFLICT(server_item) DO NOTHING",
                [item],
            )?;
            Ok(
                conn.query_row("SELECT id FROM file WHERE server_item=?1", [item], |r| {
                    r.get(0)
                })?,
            )
        }
    }
}
impl Db {
    pub fn catalog_export(&self) -> Result<CatalogSnapshot> {
        // A read transaction gives one snapshot across the different tables.
        let tx = self.conn.unchecked_transaction()?;
        let snapshot = self.export_catalog_in(&tx)?;
        tx.commit()?;
        Ok(snapshot)
    }
    fn export_catalog_in(&self, tx: &Connection) -> Result<CatalogSnapshot> {
        let mut snapshot = CatalogSnapshot {
            nodes: records(&tx, "SELECT data FROM media_node ORDER BY id")?,
            links: records(
                &tx,
                "SELECT data FROM provider_link ORDER BY media_id,provider,scope,kind,external_id",
            )?,
            marks: records(
                &tx,
                "SELECT data FROM media_mark ORDER BY media_id,reference",
            )?,
            ..Default::default()
        };
        let mut stmt = tx.prepare("SELECT file_id FROM file_binding ORDER BY file_id")?;
        for row in stmt.query_map([], |r| r.get::<_, i64>(0))? {
            let id = row?;
            let mut b = binding(&tx, id)?.ok_or_else(|| Error::invalid("绑定读取不一致"))?;
            b.file = 0; // Device-local row ids must not travel to another device.
            snapshot.bindings.push(PortableBinding {
                file: key(&tx, id)?,
                binding: b,
            });
        }
        drop(stmt);
        for mut b in records::<erika_catalog::DanmakuBinding>(
            &tx,
            "SELECT data FROM danmaku_binding ORDER BY file_id",
        )? {
            let portable = key(&tx, b.file)?;
            b.file = 0;
            snapshot.danmaku.push((portable, b));
        }
        let mut stmt=tx.prepare("SELECT media_id,watched,manual,updated_at,revision FROM media_watched ORDER BY media_id")?;
        snapshot.watched = stmt
            .query_map([], |r| {
                Ok(Watched {
                    media: MediaId(r.get(0)?),
                    watched: r.get(1)?,
                    manual: r.get(2)?,
                    updated_at: r.get(3)?,
                    revision: r.get::<_, i64>(4)? as u64,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        Ok(snapshot)
    }
    /// `replace_conflicts` is an explicit restore/user decision, never automatic
    /// account sync. Revisions are local CAS counters, not cross-device clocks.
    pub fn catalog_import(
        &self,
        snapshot: &CatalogSnapshot,
        replace_conflicts: bool,
    ) -> Result<()> {
        let tx = transaction(&self.conn)?;
        self.import_catalog_in(&tx, snapshot, replace_conflicts)?;
        tx.commit()?;
        Ok(())
    }

    /// Commit a sync merge only while the snapshot it was based on is current.
    /// Deletions here are undo of file decisions, not deletion of media identity.
    pub fn catalog_import_checked(
        &self,
        snapshot: &CatalogSnapshot,
        expected: &CatalogSnapshot,
        removed_bindings: &[FileKey],
        removed_danmaku: &[FileKey],
    ) -> Result<()> {
        let tx = super::portable::write::Write::begin(&self.conn)?;
        if &self.export_catalog_in(&tx)? != expected {
            return Err(Error::conflict("媒体库在同步期间已改变，请重新同步"));
        }
        for key in removed_bindings {
            let id = file(&tx, key)?;
            tx.execute("DELETE FROM file_binding WHERE file_id=?1", [id])?;
            super::catalog::bump(&tx, id, revision(&tx, id)?)?;
            tx.execute(
                "UPDATE file SET episode_id=NULL,matched_by=NULL WHERE id=?1",
                [id],
            )?;
        }
        for key in removed_danmaku {
            let id = file(&tx, key)?;
            tx.execute("DELETE FROM danmaku_binding WHERE file_id=?1", [id])?;
            let revision = super::catalog::danmaku_revision(&tx, id)?
                .checked_add(1)
                .ok_or_else(|| Error::conflict("弹幕版本超出范围"))?;
            let revision = super::catalog::sql_revision(revision)?;
            tx.execute("INSERT INTO danmaku_revision(file_id,revision) VALUES(?1,?2) ON CONFLICT(file_id) DO UPDATE SET revision=excluded.revision", params![id, revision])?;
        }
        // A causally agreed undo can restore an automatic binding. The CAS
        // above fences local edits; keep the monotonic clock, not the old row's
        // manual guard. Concurrent manual conflicts were resolved before here.
        for entry in &snapshot.bindings {
            if entry.binding.decision == Decision::Auto {
                let id = file(&tx, &entry.file)?;
                if binding(&tx, id)?.is_some_and(|b| b.decision != Decision::Auto) {
                    tx.execute("DELETE FROM file_binding WHERE file_id=?1", [id])?;
                }
            }
        }
        for (key, entry) in &snapshot.danmaku {
            if !entry.manual {
                let id = file(&tx, key)?;
                if self.catalog_danmaku(id)?.is_some_and(|b| b.manual) {
                    tx.execute("DELETE FROM danmaku_binding WHERE file_id=?1", [id])?;
                }
            }
        }
        self.import_catalog_in(&tx, snapshot, true)?;
        tx.commit()?;
        Ok(())
    }

    fn import_catalog_in(
        &self,
        tx: &Connection,
        snapshot: &CatalogSnapshot,
        replace_conflicts: bool,
    ) -> Result<()> {
        let local = records(&tx, "SELECT data FROM media_node")?;
        let snapshot = crate::catalog_snapshot::normalize(snapshot, &local)?;
        let nodes = crate::catalog_snapshot::changed_nodes(&snapshot.nodes, &local);
        put_nodes(&tx, &nodes)?;
        let mut unique = HashSet::new();
        for link in &snapshot.links {
            if !unique.insert((link.media.clone(), link.reference.clone())) {
                return Err(Error::invalid("快照包含重复关联"));
            }
            let previous = self
                .catalog_links(&link.media)?
                .into_iter()
                .find(|l| l.reference == link.reference);
            let mut value = link.clone();
            value.revision = previous.as_ref().map_or(0, |l| l.revision);
            if let Some(mut previous) = previous {
                previous.revision = value.revision;
                if previous == value {
                    continue;
                }
                if previous.manual && !replace_conflicts {
                    return Err(Error::conflict("远端关联与本地人工关联冲突"));
                }
                if replace_conflicts && previous.manual {
                    value.manual = true;
                }
            }
            put_link(&tx, &value)?;
        }
        let mut unique = HashSet::new();
        for entry in &snapshot.bindings {
            if !unique.insert(entry.file.clone()) {
                return Err(Error::invalid("快照包含重复文件绑定"));
            }
            let id = file(&tx, &entry.file)?;
            let mut value = entry.binding.clone();
            value.file = id;
            value.revision = revision(&tx, id)?;
            for item in &mut value.items {
                item.media = canonical(&tx, &item.media)?;
            }
            if let Some(previous) = binding(&tx, id)? {
                if previous.decision == value.decision
                    && previous.items == value.items
                    && previous.evidence == value.evidence
                {
                    continue;
                }
                if previous.decision != Decision::Auto && !replace_conflicts {
                    return Err(Error::conflict("远端绑定与本地人工决定冲突"));
                }
                if replace_conflicts
                    && previous.decision != Decision::Auto
                    && value.decision == Decision::Auto
                {
                    value.decision = Decision::Manual;
                }
            }
            put_binding(&tx, &value)?;
        }
        let mut unique = HashSet::new();
        for (portable, input) in &snapshot.danmaku {
            if !unique.insert(portable.clone()) {
                return Err(Error::invalid("快照包含重复弹幕关联"));
            }
            let id = file(&tx, portable)?;
            let mut value = input.clone();
            value.file = id;
            if let Some(media) = value.media.as_mut() {
                *media = canonical(&tx, media)?;
            }
            let previous = self.catalog_danmaku(id)?;
            value.revision = self.catalog_danmaku_revision(id)?;
            if let Some(previous) = previous {
                if previous == value {
                    continue;
                }
                if previous.manual && !replace_conflicts {
                    return Err(Error::conflict("远端弹幕关联与本地人工决定冲突"));
                }
                if replace_conflicts && previous.manual {
                    value.manual = true;
                }
            }
            put_danmaku(&tx, &value)?;
        }
        let mut unique = HashSet::new();
        for input in &snapshot.watched {
            let mut value = input.clone();
            value.media = canonical(&tx, &value.media)?;
            if !unique.insert(value.media.clone()) {
                return Err(Error::invalid("快照包含冲突的观看记录"));
            }
            let previous = self.catalog_watched(&value.media)?;
            value.revision = previous.as_ref().map_or(0, |w| w.revision);
            if let Some(previous) = previous {
                if previous == value
                    || (!replace_conflicts
                        && previous.watched == value.watched
                        && previous.manual > value.manual)
                {
                    continue;
                }
                if previous.manual && !replace_conflicts {
                    return Err(Error::conflict("远端观看记录与本地人工决定冲突"));
                }
                if !replace_conflicts && previous.updated_at > value.updated_at {
                    continue;
                }
                if replace_conflicts && previous.manual {
                    value.manual = true;
                }
            }
            put_watched(&tx, &value)?;
        }
        let mut unique = HashSet::new();
        for input in &snapshot.marks {
            if !unique.insert((&input.media, &input.reference)) {
                return Err(Error::invalid("快照包含重复作品状态"));
            }
            let mut value = input.clone();
            value.media = canonical(&tx, &value.media)?;
            let previous = super::catalog::mark(tx, &value.media, &value.reference)?;
            if let Some(value) = value.imported(previous.as_ref(), replace_conflicts)? {
                put_mark(tx, &value)?;
            }
        }
        Ok(())
    }
}
