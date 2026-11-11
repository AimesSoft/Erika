//! A portable restore may compose independently atomic store operations.
//! Keep an IMMEDIATE root transaction and rollback nested operations to a
//! savepoint, including when the caller handles their error and continues.
use crate::catalog::Result;
use rusqlite::{Connection, Transaction, TransactionBehavior};

pub(in crate::sqlite) struct Write<'a> {
    conn: &'a Connection,
    root: Option<Transaction<'a>>,
    nested: bool,
}
impl<'a> Write<'a> {
    pub(in crate::sqlite) fn begin(conn: &'a Connection) -> Result<Self> {
        let root = if conn.is_autocommit() {
            Some(Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?)
        } else {
            // Savepoints with the same name form a stack; RELEASE / ROLLBACK TO
            // always target the most recent one on this connection.
            conn.execute_batch("SAVEPOINT erika_portable_write")?;
            None
        };
        let nested = root.is_none();
        Ok(Self { conn, root, nested })
    }
    pub(in crate::sqlite) fn commit(mut self) -> Result<()> {
        if let Some(root) = self.root.take() {
            root.commit()?;
        } else {
            self.conn.execute_batch("RELEASE erika_portable_write")?;
            self.nested = false;
        }
        Ok(())
    }
}
impl std::ops::Deref for Write<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn
    }
}
impl Drop for Write<'_> {
    fn drop(&mut self) {
        if self.nested {
            // No values from the imported record belong in diagnostic output.
            if self.conn.execute_batch(
                "ROLLBACK TO erika_portable_write; RELEASE erika_portable_write",
            ).is_err() {
                eprintln!("erika-store: could not roll back a portable write");
            }
        }
        // An uncommitted root Transaction rolls back on drop.
    }
}
