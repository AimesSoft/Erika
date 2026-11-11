use super::*;

#[test]
fn setting_write_failure_is_reported_without_publishing_the_result() {
    let db = Db::in_memory();
    db.set_setting("cache", Some("old"));
    db.conn.pragma_update(None, "query_only", true).unwrap();
    let result = db.update_setting("cache", |_| Ok((Some("new".into()), "published")));
    assert_eq!(result.unwrap_err().code, "storage");
    assert_eq!(db.try_setting("cache").unwrap().as_deref(), Some("old"));
    db.conn.pragma_update(None, "query_only", false).unwrap();
    db.update_setting("cache", |_| Ok((Some("new".into()), ())))
        .unwrap();
    assert_eq!(db.setting("cache").as_deref(), Some("new"));
}

#[test]
fn setting_read_failure_never_invokes_the_update_callback() {
    let db = Db::in_memory();
    db.conn.execute("DROP TABLE setting", []).unwrap();
    assert!(db.try_setting("cache").is_err());
    let result = db.update_setting::<()>("cache", |_| panic!("cannot fabricate an empty value"));
    assert_eq!(result.unwrap_err().code, "storage");
}
