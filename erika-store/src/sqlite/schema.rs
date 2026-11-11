//! Validate the actual current schema before opening a durable store for writes.
//! Version stamps alone cannot distinguish unfinished development schemas. This
//! only compares definitions; it never upgrades, repairs or recreates a file.
use super::*;
use std::sync::OnceLock;

type Structure = Vec<(String, String, String, Option<String>)>;

pub(super) fn structure(conn: &Connection) -> Result<Structure> {
    conn.prepare(
        "SELECT type,name,tbl_name,sql FROM sqlite_master
         WHERE name NOT GLOB 'sqlite_*' ORDER BY type,name",
    )?
    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
    .collect()
}

pub(super) fn create(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    conn.execute_batch(include_str!("catalog.sql"))?;
    conn.execute_batch(include_str!("plugin_sources.sql"))
}

pub(super) fn matches(actual: &Structure) -> Result<bool> {
    static EXPECTED: OnceLock<Structure> = OnceLock::new();
    if let Some(expected) = EXPECTED.get() {
        return Ok(actual == expected);
    }
    let reference = Connection::open_in_memory()?;
    create(&reference)?;
    let expected = structure(&reference)?;
    let same = actual == &expected;
    let _ = EXPECTED.set(expected);
    Ok(same)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_same_version_files_are_rejected_without_repair() {
        let root =
            std::env::temp_dir().join(format!("erika-schema-{}", erika_catalog::MediaId::new()));
        std::fs::create_dir(&root).unwrap();
        for (index, alter) in [
            "DROP TABLE account_sync",
            "ALTER TABLE account_sync RENAME COLUMN data TO old_data",
            "DROP INDEX tmdb_exact_identity",
            "CREATE TRIGGER unexpected_write AFTER INSERT ON setting BEGIN DELETE FROM account_sync; END",
        ].iter().enumerate() {
            let path = root.join(format!("{index}.sqlite"));
            let conn = Connection::open(&path).unwrap();
            create(&conn).unwrap();
            conn.pragma_update(None, "application_id", APPLICATION_ID).unwrap();
            conn.pragma_update(None, "user_version", SCHEMA_VERSION).unwrap();
            conn.execute_batch(alter).unwrap();
            drop(conn);
            let before = std::fs::read(&path).unwrap();
            let error = match Db::at(&path) {
                Ok(_) => panic!("mismatched schema was accepted: {alter}"),
                Err(error) => error,
            };
            assert!(error.to_string().contains("数据库格式不匹配"));
            assert_eq!(before, std::fs::read(&path).unwrap());
            assert!(!path.with_extension("sqlite-wal").exists());
            assert!(!path.with_extension("sqlite-shm").exists());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn view_only_files_are_not_empty_databases() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE VIEW keep_me AS SELECT 1")
            .unwrap();
        let before = structure(&conn).unwrap();
        assert!(Db::check_schema(&conn).is_err());
        assert_eq!(before, structure(&conn).unwrap());
    }

    #[test]
    fn data_changes_and_sqlite_statistics_do_not_change_the_format() {
        let db = Db::in_memory();
        db.set_setting("test", Some("kept"));
        db.conn.execute_batch("ANALYZE").unwrap();
        assert!(Db::check_schema(&db.conn).unwrap());
        assert_eq!(db.setting("test").as_deref(), Some("kept"));
    }
}
