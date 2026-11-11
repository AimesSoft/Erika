use super::{
    Db,
    catalog::{decode, json, transaction},
};
use crate::{
    catalog::{Error, Result},
    plugin_sources::{self as model, Descriptor, Item, Scan, StoredItem},
};
use rusqlite::{Connection, OptionalExtension, params};

fn owned(conn: &Connection, owner: &str, source: i64) -> Result<String> {
    conn.query_row(
        "SELECT server_id FROM plugin_source WHERE source_id=?1 AND owner=?2",
        params![source, owner],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| Error::conflict("来源不存在或不属于此插件"))
}
fn scan(conn: &Connection, owner: &str, id: &str) -> Result<(Scan, String)> {
    let data: String=conn.query_row("SELECT sc.data FROM plugin_source_scan sc JOIN plugin_source s ON s.source_id=sc.source_id WHERE sc.id=?1 AND s.owner=?2",params![id,owner],|r|r.get(0)).optional()?
        .ok_or_else(||Error::conflict("扫描已失效或不属于此插件"))?;
    let scan: Scan = decode(data)?;
    let server = owned(conn, owner, scan.source)?;
    Ok((scan, server))
}
/// Used by registration and current-format restore inside their write boundary.
pub(super) fn register(
    conn: &Connection, owner: &str, descriptor: &Descriptor, added: i64,
) -> Result<(i64, bool)> {
    descriptor.validate(owner)?;
    let old: Option<(i64, String)> = conn.query_row(
        "SELECT source_id,server_id FROM plugin_source WHERE owner=?1 AND external_id=?2",
        params![owner, descriptor.id], |r| Ok((r.get(0)?, r.get(1)?)),
    ).optional()?;
    if let Some((id, server)) = old {
        if server != descriptor.server_id {
            return Err(Error::conflict("同一来源不能更换服务器身份"));
        }
        conn.execute("UPDATE source SET name=?2,content=?3 WHERE id=?1",
            params![id, descriptor.name, descriptor.content])?;
        return Ok((id, false));
    }
    conn.execute(
        "INSERT INTO source(kind,name,location,content,added_at) VALUES('plugin',?1,?2,?3,?4)",
        params![descriptor.name, descriptor.location(owner), descriptor.content, added],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO plugin_source(source_id,owner,external_id,server_id) VALUES(?1,?2,?3,?4)",
        params![id, owner, descriptor.id, descriptor.server_id],
    )?;
    Ok((id, true))
}
impl Db {
    pub fn plugin_source_ids(&self, owner: &str) -> Result<Vec<i64>> {
        self.conn
            .prepare("SELECT source_id FROM plugin_source WHERE owner=?1 ORDER BY source_id")?
            .query_map([owner], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()
            .map_err(Into::into)
    }
    pub fn plugin_register_source(&self, owner: &str, descriptor: &Descriptor) -> Result<i64> {
        self.put_source(&descriptor.portable(owner)).map(|(id, _)| id)
    }
    pub fn plugin_begin_scan(&self, owner: &str, source: i64) -> Result<Scan> {
        let tx = transaction(&self.conn)?;
        owned(&tx, owner, source)?;
        tx.execute(
            "DELETE FROM plugin_source_scan WHERE source_id=?1",
            [source],
        )?;
        let scan = Scan::new(source);
        tx.execute(
            "INSERT INTO plugin_source_scan(source_id,id,data) VALUES(?1,?2,?3)",
            params![source, scan.id, json(&scan)?],
        )?;
        tx.commit()?;
        Ok(scan)
    }
    pub fn plugin_upsert_items(
        &self,
        owner: &str,
        id: &str,
        page: u32,
        items: &[Item],
    ) -> Result<Scan> {
        let tx = transaction(&self.conn)?;
        let (mut scan, server) = scan(&tx, owner, id)?;
        scan.active()?;
        let data = model::validate_page(owner, &server, items)?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT data FROM plugin_source_page WHERE scan_id=?1 AND page=?2",
                params![id, page],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            if previous != data {
                return Err(Error::conflict("同一分页重试的内容不一致"));
            }
            tx.commit()?;
            return Ok(scan);
        }
        if page != scan.next_page {
            return Err(Error::conflict("扫描分页不连续"));
        }
        if page >= model::MAX_PAGES {
            return Err(Error::invalid("来源扫描分页过多"));
        }
        let mut count: i64 = tx.query_row(
            "SELECT count(*) FROM plugin_source_stage WHERE scan_id=?1",
            [id],
            |r| r.get(0),
        )?;
        for item in items {
            let slot = item.slot();
            let data = json(item)?;
            let previous: Option<String> = tx
                .query_row(
                    "SELECT data FROM plugin_source_stage WHERE scan_id=?1 AND slot=?2",
                    params![id, slot],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(old) = previous {
                if old != data {
                    return Err(Error::conflict("扫描中同一条目的内容互相冲突"));
                }
            } else {
                count += 1;
                if count > model::MAX_ITEMS as i64 {
                    return Err(Error::invalid("来源条目过多"));
                }
                tx.execute(
                    "INSERT INTO plugin_source_stage(scan_id,slot,data) VALUES(?1,?2,?3)",
                    params![id, slot, data],
                )?;
            }
        }
        scan.next_page += 1;
        scan.expires_at = crate::now() + model::LEASE_SECONDS;
        tx.execute(
            "INSERT INTO plugin_source_page(scan_id,page,data) VALUES(?1,?2,?3)",
            params![id, page, data],
        )?;
        tx.execute(
            "UPDATE plugin_source_scan SET data=?2 WHERE id=?1",
            params![id, json(&scan)?],
        )?;
        tx.commit()?;
        Ok(scan)
    }
    pub fn plugin_finish_scan(
        &self,
        owner: &str,
        id: &str,
        pages: u32,
        complete: bool,
    ) -> Result<Scan> {
        let tx = transaction(&self.conn)?;
        let (mut scan, _) = scan(&tx, owner, id)?;
        if !scan.finish(pages, complete)? {
            tx.commit()?;
            return Ok(scan);
        }
        if complete {
            let external: String = tx.query_row(
                "SELECT external_id FROM plugin_source WHERE source_id=?1",
                [scan.source], |r| r.get(0),
            )?;
            let rows: Vec<(String, String)> = tx
                .prepare(
                    "SELECT slot,data FROM plugin_source_stage WHERE scan_id=?1 ORDER BY slot",
                )?
                .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            for (slot, data) in rows {
                let item: Item = decode(data.clone())?;
                let file = super::catalog_portable::file(&tx, &item.key(owner)?)?;
                let url = model::address(owner, &external, &item.server_id, &slot);
                tx.execute("INSERT INTO plugin_source_item(source_id,slot,url,file_id,generation,data) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(source_id,slot) DO UPDATE SET file_id=excluded.file_id,generation=excluded.generation,data=excluded.data",params![scan.source,slot,url,file,id,data])?;
                tx.execute("INSERT INTO address(source_id,path,file_id,seen_at) VALUES(?1,?2,?3,?4) ON CONFLICT(source_id,path) DO UPDATE SET file_id=excluded.file_id,seen_at=excluded.seen_at",params![scan.source,url,file,crate::now()])?;
            }
            // Only a completed scan can hide entries. Files, manual bindings and
            // progress remain intact if another source finds them later.
            tx.execute("DELETE FROM address WHERE source_id=?1 AND path IN (SELECT url FROM plugin_source_item WHERE source_id=?1 AND generation!=?2)",params![scan.source,id])?;
            tx.execute(
                "DELETE FROM plugin_source_item WHERE source_id=?1 AND generation!=?2",
                params![scan.source, id],
            )?;
            tx.execute(
                "UPDATE plugin_source SET generation=?2 WHERE source_id=?1",
                params![scan.source, id],
            )?;
        }
        tx.execute(
            "UPDATE plugin_source_scan SET data=?2 WHERE id=?1",
            params![id, json(&scan)?],
        )?;
        tx.execute("DELETE FROM plugin_source_stage WHERE scan_id=?1", [id])?;
        tx.execute("DELETE FROM plugin_source_page WHERE scan_id=?1", [id])?;
        tx.commit()?;
        Ok(scan)
    }
    pub fn plugin_source_item(&self, url: &str) -> Result<Option<StoredItem>> {
        let row:Option<(i64,String,String,i64,String)>=self.conn.query_row("SELECT i.source_id,s.owner,i.generation,i.file_id,i.data FROM plugin_source_item i JOIN plugin_source s ON s.source_id=i.source_id WHERE i.url=?1",[url],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
        row.map(|(source, owner, generation, file, data)| {
            Ok(StoredItem {
                source,
                owner,
                generation,
                file,
                url: url.into(),
                item: decode(data)?,
            })
        })
        .transpose()
    }
    pub fn plugin_source_page(
        &self,
        owner: &str,
        request: &model::page::ListRequest,
    ) -> Result<model::page::Page> {
        let tx = self.conn.unchecked_transaction()?;
        owned(&tx, owner, request.source)?;
        let generation: String = tx.query_row(
            "SELECT generation FROM plugin_source WHERE source_id=?1",
            [request.source],
            |r| r.get(0),
        )?;
        let after = request.after(owner, &generation)?;
        let result = {
            let mut stmt = tx.prepare("SELECT url,file_id,generation,data FROM plugin_source_item WHERE source_id=?1 AND slot>?2 ORDER BY slot LIMIT ?3")?;
            let rows = stmt.query_map(
                params![request.source, after, (request.limit + 1) as i64],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )?;
            model::page::collect(
                owner,
                request,
                &generation,
                rows.map(|row| {
                    let (url, file, generation, data) = row?;
                    Ok(StoredItem {
                        source: request.source,
                        owner: owner.into(),
                        generation,
                        file,
                        url,
                        item: decode(data)?,
                    })
                }),
            )?
        };
        tx.commit()?;
        Ok(result)
    }
    pub fn plugin_source_version(
        &self,
        owner: &str,
        request: &model::page::ItemRequest,
    ) -> Result<Option<StoredItem>> {
        let slot = request.slot()?;
        let tx = self.conn.unchecked_transaction()?;
        owned(&tx, owner, request.source)?;
        let row: Option<(String,i64,String,String)> = tx.query_row(
            "SELECT url,file_id,generation,data FROM plugin_source_item WHERE source_id=?1 AND slot=?2",
            params![request.source,slot], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        ).optional()?;
        let result = row
            .map(|(url, file, generation, data)| -> Result<_> {
                Ok(StoredItem {
                    source: request.source,
                    owner: owner.into(),
                    generation,
                    file,
                    url,
                    item: decode(data)?,
                })
            })
            .transpose()?;
        tx.commit()?;
        Ok(result)
    }
    pub fn plugin_source_status(
        &self,
        owner: &str,
        request: &model::page::ItemRequest,
    ) -> Result<Option<model::status::Status>> {
        let slot = request.slot()?;
        let tx = self.conn.unchecked_transaction()?;
        let server = owned(&tx, owner, request.source)?;
        let external: String = tx.query_row(
            "SELECT external_id FROM plugin_source WHERE source_id=?1",
            [request.source], |r| r.get(0),
        )?;
        let url = model::address(owner, &external, &server, &slot);
        let result = if let Some(item) = self.plugin_source_item(&url)? {
            let revision = self.catalog_revision(item.file)?;
            let binding = self.catalog_binding(item.file)?;
            let row: Option<(String,i64)> = tx.query_row("SELECT data,lease_until FROM catalog_job WHERE file_id=?1 ORDER BY rowid DESC LIMIT 1",[item.file],|r| Ok((r.get(0)?,r.get(1)?))).optional()?;
            let job = row
                .map(|(data, lease)| -> Result<_> { Ok((decode(data)?, lease)) })
                .transpose()?;
            Some(model::status::project(
                item,
                revision,
                binding,
                job,
                crate::now(),
            )?)
        } else {
            None
        };
        tx.commit()?;
        Ok(result)
    }
    /// For host-side callers needing a full snapshot, use one query, not one query per item.
    pub fn plugin_source_items(&self, owner: &str, source: i64) -> Result<Vec<StoredItem>> {
        let tx = self.conn.unchecked_transaction()?;
        owned(&tx, owner, source)?;
        let result = {
            let mut stmt = tx.prepare("SELECT url,file_id,generation,data FROM plugin_source_item WHERE source_id=?1 ORDER BY slot")?;
            stmt.query_map([source], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .map(|r| {
                let (url, file, generation, data) = r?;
                Ok(StoredItem {
                    source,
                    owner: owner.into(),
                    generation,
                    file,
                    url,
                    item: decode(data)?,
                })
            })
            .collect::<Result<Vec<_>>>()?
        };
        tx.commit()?;
        Ok(result)
    }
    /// Metadata I/O completed: compare both the scan generation and file CAS in
    /// the same write transaction. Neither a removed source nor a user edit can
    /// be revived by a late response.
    pub fn plugin_bind_source(
        &self,
        expected: &StoredItem,
        revision: u64,
        resolved: &erika_catalog::resolve::Resolved,
    ) -> Result<()> {
        let tx = transaction(&self.conn)?;
        if self.plugin_source_item(&expected.url)?.as_ref() != Some(expected) {
            return Err(Error::conflict("来源条目在读取资料期间已改变"));
        }
        super::catalog::apply(
            &tx,
            &resolved.nodes(),
            &[],
            &[erika_catalog::FileBinding {
                file: expected.file,
                revision,
                decision: erika_catalog::Decision::Auto,
                items: resolved.items.clone(),
                evidence: resolved.evidence.clone(),
            }],
            &[],
        )?;
        tx.commit()?;
        Ok(())
    }
}
