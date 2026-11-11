//! Shared atomic-setting behavior for SQLite connections and browser engines.
use crate::catalog::Error;

macro_rules! contract {
    ($name:ident, $db:ty) => {
        mod $name {
            use super::*;
            type Db = $db;

            #[test]
            fn update_reads_current_value_and_can_remove_it() {
                let db = Db::in_memory();
                assert_eq!(db.try_setting("cache").unwrap(), None);
                let answer = db
                    .update_setting("cache", |old| {
                        assert_eq!(old, None);
                        Ok((Some("one".into()), 42))
                    })
                    .unwrap();
                assert_eq!(answer, 42);
                db.update_setting("cache", |old| {
                    assert_eq!(old, Some("one"));
                    Ok((Some(format!("{},two", old.unwrap())), ()))
                })
                .unwrap();
                assert_eq!(db.try_setting("cache").unwrap().as_deref(), Some("one,two"));
                db.update_setting("cache", |old| {
                    assert_eq!(old, Some("one,two"));
                    Ok((None, ()))
                })
                .unwrap();
                assert!(db.try_setting("cache").unwrap().is_none());
            }

            #[test]
            fn rejected_updates_leave_storage_intact() {
                let db = Db::in_memory();
                db.set_setting("cache", Some("kept"));
                let result = db.update_setting::<()>("cache", |old| {
                    assert_eq!(old, Some("kept"));
                    Err(Error::conflict("changed"))
                });
                assert_eq!(result.unwrap_err().code, "conflict");
                assert_eq!(db.setting("cache").as_deref(), Some("kept"));
                assert!(
                    db.update_setting::<()>("missing", |_| Err(Error::invalid("bad")))
                        .is_err()
                );
                assert!(db.setting("missing").is_none());
                // Failure releases the transaction/lock for the next writer.
                db.update_setting("cache", |_| Ok((Some("next".into()), ())))
                    .unwrap();
                assert_eq!(db.setting("cache").as_deref(), Some("next"));
            }
        }
    };
}
contract!(sqlite, crate::sqlite::Db);
contract!(browser, crate::session::Db);

macro_rules! increment {
    ($db:expr) => {
        $db.update_setting("counter", |old| {
            let value = old.unwrap_or("0").parse::<usize>().unwrap() + 1;
            // Force contention between reading and writing, inside the transaction.
            std::thread::sleep(std::time::Duration::from_micros(100));
            Ok((Some(value.to_string()), ()))
        })
        .unwrap()
    };
}

#[test]
fn sqlite_connections_do_not_lose_simultaneous_updates() {
    let root = std::env::temp_dir().join(format!(
        "erika-settings-{}",
        erika_catalog::MediaId::new().0
    ));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("db.sqlite");
    let observer = crate::Db::at(&path).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let connections: Vec<_> = (0..4).map(|_| crate::Db::at(&path).unwrap()).collect();
    let workers: Vec<_> = connections
        .into_iter()
        .map(|db| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..40 {
                    increment!(db);
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(observer.setting("counter").as_deref(), Some("160"));
    drop(observer);
    let reopened = crate::Db::at(&path).unwrap();
    assert_eq!(reopened.setting("counter").as_deref(), Some("160"));
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn session_engines_do_not_lose_simultaneous_updates() {
    let db = crate::session::Db::in_memory();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let db = db.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..40 {
                    increment!(db);
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(db.setting("counter").as_deref(), Some("160"));
}
