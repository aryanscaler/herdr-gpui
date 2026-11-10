//! A Herdr 2 run's session names its herdr-core terminal in a column older
//! databases gain on opening, without a new schema version.
use super::*;
use rusqlite::Connection;

#[test]
fn a_database_without_the_terminal_column_gains_it_and_keeps_herdr_1_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let path = store::database(&dir);
    let mut store = Store::open(&path).unwrap();
    store
        .save_run(&run("r1", Owner::Sheprd), Some(&session("r1")))
        .unwrap();
    drop(store);
    // As a build from before the column left it.
    Connection::open(&path)
        .unwrap()
        .execute_batch("ALTER TABLE herdr_sessions DROP COLUMN terminal")
        .unwrap();

    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.access(), Access::ReadWrite);
    assert_eq!(store.sessions().unwrap(), [session("r1")]);

    let herdr2 = HerdrSession {
        pane: Pane::Herdr2 {
            terminal: "t9".into(),
        },
        ..session("r2")
    };
    store
        .save_run(&run("r2", Owner::Sheprd), Some(&herdr2))
        .unwrap();
    let mut sessions = store.sessions().unwrap();
    sessions.sort_by(|a, b| a.run_id.cmp(&b.run_id));
    assert_eq!(sessions, [session("r1"), herdr2]);
    let version: i64 = Connection::open(&path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
}
