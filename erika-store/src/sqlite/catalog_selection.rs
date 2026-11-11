//! Address and binding checks share the same write transaction.
use super::{
    Db,
    catalog::{apply, transaction},
};
use crate::catalog::{AddressCheck, BindingChange, Error, Result};
use erika_catalog::{FileBinding, MediaNode};
use rusqlite::params;

impl Db {
    /// All known addresses of a physical file, including directly opened videos.
    /// Prefer a library source (which supplies content hints), then recent sightings.
    /// A known file may have no current address; an unknown file is an error.
    pub fn catalog_addresses(&self, file: i64) -> Result<Vec<AddressCheck>> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM file WHERE id=?1)", [file], |r| r.get(0),
        )?;
        if !exists { return Err(Error::invalid("文件不存在")); }
        let mut stmt = self.conn.prepare(
            "SELECT rowid,source_id,path FROM address WHERE file_id=?1
             ORDER BY source_id=?2,seen_at DESC,rowid DESC",
        )?;
        Ok(stmt.query_map(params![file, super::OPENED], |r| Ok(AddressCheck {
            key: r.get(0)?, source: r.get(1)?, url: r.get(2)?, file,
        }))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn catalog_apply_selection(
        &self,
        addresses: &[AddressCheck],
        nodes: &[MediaNode],
        bindings: &[FileBinding],
    ) -> Result<BindingChange> {
        AddressCheck::validate_batch(addresses, bindings)?;
        let tx = transaction(&self.conn)?;
        for expected in addresses {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM address WHERE rowid=?1 AND source_id=?2 AND path=?3 AND file_id=?4)",
                params![expected.key, expected.source, expected.url, expected.file],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(Error::conflict("文件地址已改变，请重新打开匹配"));
            }
        }
        let change = apply(&tx, nodes, &[], bindings, &[])?;
        tx.commit()?;
        Ok(change)
    }
}
