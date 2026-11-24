//! A profile's database: in memory or in its folder, set up as Chromium's,
//! its schema brought to its version, a damaged file made again.

use std::path::PathBuf;

use sql::{Database, Error, Synchronous};

/// A new, empty folder for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("sql")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const SCHEMA: [&str; 2] = [
    "CREATE TABLE kv (k TEXT PRIMARY KEY, v TEXT NOT NULL);",
    "ALTER TABLE kv ADD COLUMN at INTEGER NOT NULL DEFAULT 0;",
];

fn count(db: &mut Database) -> i64 {
    db.connection()
        .query_row("SELECT count(*) FROM kv", [], |row| row.get(0))
        .unwrap()
}

/// One in memory works as one in a file, and is gone with its connection.
#[test]
fn a_database_in_memory_keeps_nothing_after() {
    let mut db = Database::open(None, "Kept").unwrap();
    db.migrate(&SCHEMA).unwrap();
    db.connection()
        .execute("INSERT INTO kv (k, v) VALUES ('a', '1')", [])
        .unwrap();
    assert_eq!(count(&mut db), 1);
    drop(db);
    let mut again = Database::open(None, "Kept").unwrap();
    assert_eq!(again.version().unwrap(), 0);
    again.migrate(&SCHEMA).unwrap();
    assert_eq!(count(&mut again), 0);
}

/// One in a folder is the file of its name there, and keeps what was
/// written after it is closed.
#[test]
fn a_database_in_a_folder_keeps_what_was_written() {
    let dir = scratch("kept");
    let mut db = Database::open(Some(&dir), "Kept").unwrap();
    db.migrate(&SCHEMA).unwrap();
    db.connection()
        .execute("INSERT INTO kv (k, v) VALUES ('a', '1')", [])
        .unwrap();
    drop(db);
    assert!(dir.join("Kept").is_file());
    let mut again = Database::open(Some(&dir), "Kept").unwrap();
    assert_eq!(again.version().unwrap(), 2);
    again.migrate(&SCHEMA).unwrap();
    assert_eq!(count(&mut again), 1);
}

/// Set up as Chromium's `sql::Database` by default: exclusive locking (a
/// second connection can't read it while the first has it), a rollback
/// journal truncated at each commit.
#[test]
fn a_database_is_set_up_as_chromiums() {
    let dir = scratch("set-up");
    let mut db = Database::open(Some(&dir), "Data").unwrap();
    db.migrate(&SCHEMA).unwrap();
    let mode = |db: &mut Database, pragma: &str| -> String {
        db.connection()
            .query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))
            .unwrap()
    };
    assert_eq!(mode(&mut db, "journal_mode"), "truncate");
    assert_eq!(mode(&mut db, "locking_mode"), "exclusive");
    // At once: it doesn't wait for the lock.
    let tried = std::time::Instant::now();
    assert!(matches!(
        Database::open(Some(&dir), "Data"),
        Err(Error::Sqlite(_))
    ));
    assert!(tried.elapsed() < std::time::Duration::from_secs(1));
    drop(db);
    Database::open(Some(&dir), "Data").unwrap();
}

/// Each migration after the version it is at runs once; a later machopo's
/// version is refused; a migration that fails leaves it as it was.
#[test]
fn a_schema_is_brought_to_its_version_once() {
    let dir = scratch("migrate");
    let mut db = Database::open(Some(&dir), "Data").unwrap();
    db.migrate(&SCHEMA[..1]).unwrap();
    assert_eq!(db.version().unwrap(), 1);
    // The first isn't run again (the table is there): only the second.
    db.migrate(&SCHEMA).unwrap();
    assert_eq!(db.version().unwrap(), 2);
    db.migrate(&SCHEMA).unwrap();
    assert!(matches!(
        db.migrate(&SCHEMA[..1]),
        Err(Error::Newer {
            version: 2,
            known: 1
        })
    ));
    let broken = [
        SCHEMA[0],
        SCHEMA[1],
        "CREATE TABLE more (x); SELECT nothing FROM nowhere;",
    ];
    assert!(db.migrate(&broken).is_err());
    assert_eq!(db.version().unwrap(), 2);
    let tables: i64 = db
        .connection()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = 'more'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0);
}

/// A file that isn't a database is put aside and made again, empty.
#[test]
fn a_file_that_isnt_a_database_is_made_again() {
    let dir = scratch("corrupt");
    let garbage = vec![0x5a_u8; 8192];
    std::fs::write(dir.join("Data"), &garbage).unwrap();
    let mut db = Database::open(Some(&dir), "Data").unwrap();
    assert_eq!(db.version().unwrap(), 0);
    db.migrate(&SCHEMA).unwrap();
    assert_eq!(count(&mut db), 0);
    assert_eq!(std::fs::read(dir.join("Data.corrupt")).unwrap(), garbage);
}

/// One whose header is a database's but whose schema is damaged is made
/// again too: its schema is read as it opens (as Chromium's), not when it
/// is first used.
#[test]
fn a_database_with_a_damaged_schema_is_made_again() {
    let dir = scratch("damaged-schema");
    let mut db = Database::open(Some(&dir), "Data").unwrap();
    db.migrate(&SCHEMA).unwrap();
    drop(db);
    let path = dir.join("Data");
    let mut bytes = std::fs::read(&path).unwrap();
    // The first page's b-tree, after the file's 100-byte header: the
    // schema's table.
    for byte in &mut bytes[100..4096] {
        *byte = 0x5a;
    }
    std::fs::write(&path, &bytes).unwrap();
    let mut again = Database::open(Some(&dir), "Data").unwrap();
    assert_eq!(again.version().unwrap(), 0);
    again.migrate(&SCHEMA).unwrap();
    assert_eq!(count(&mut again), 0);
    assert_eq!(std::fs::read(dir.join("Data.corrupt")).unwrap(), bytes);
}

/// Commits synced unless asked not to (a cache's).
#[test]
fn a_database_syncs_its_commits_unless_asked_not_to() {
    let dir = scratch("synchronous");
    let synchronous = |db: &mut Database| -> i64 {
        db.connection()
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .unwrap()
    };
    // FULL is 2, OFF 0.
    assert_eq!(
        synchronous(&mut Database::open(Some(&dir), "Full").unwrap()),
        2
    );
    let mut off = Database::open_with(Some(&dir), "Off", Synchronous::Off).unwrap();
    assert_eq!(synchronous(&mut off), 0);
    assert_eq!(
        synchronous(&mut Database::open_with(None, "Off", Synchronous::Off).unwrap()),
        0
    );
}

/// Razed, it starts over empty, at version 0, set up as it was: in its
/// folder, its file made again; in memory, a new one.
#[test]
fn a_razed_database_starts_over_as_it_was() {
    let dir = scratch("raze");
    for folder in [Some(dir.as_path()), None] {
        let mut db = Database::open_with(folder, "Data", Synchronous::Off).unwrap();
        db.migrate(&SCHEMA).unwrap();
        db.connection()
            .execute("INSERT INTO kv (k, v) VALUES ('a', '1')", [])
            .unwrap();
        db.raze().unwrap();
        assert_eq!(db.version().unwrap(), 0);
        db.migrate(&SCHEMA).unwrap();
        assert_eq!(count(&mut db), 0);
        let synchronous: i64 = db
            .connection()
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .unwrap();
        assert_eq!(synchronous, 0);
        let locking: String = db
            .connection()
            .query_row("PRAGMA locking_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(locking, "exclusive");
    }
    // Its journal goes with it: one a crash left hot would be played into
    // the new file.
    let mut db = Database::open(Some(&dir), "Journal").unwrap();
    db.migrate(&SCHEMA).unwrap();
    assert!(dir.join("Journal-journal").exists());
    db.raze().unwrap();
    assert!(!dir.join("Journal-journal").exists());
    // What it has after is kept in its file.
    let mut db = Database::open(Some(&dir), "Data").unwrap();
    assert_eq!(db.version().unwrap(), 2);
    assert_eq!(count(&mut db), 0);
}

/// A file damaged past reading is corrupt, as `raze` is for.
#[test]
fn a_damaged_file_is_corrupt() {
    let dir = scratch("is-corrupt");
    std::fs::write(dir.join("Data"), b"not a database, not at all, no").unwrap();
    let error = rusqlite_open_error(&dir.join("Data"));
    assert!(sql::is_corrupt(&error));
}

fn rusqlite_open_error(path: &std::path::Path) -> sql::rusqlite::Error {
    let connection = sql::rusqlite::Connection::open(path).unwrap();
    connection
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_err()
}
