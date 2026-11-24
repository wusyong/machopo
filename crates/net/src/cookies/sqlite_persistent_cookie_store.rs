//! The profile's `Cookies` database (Chromium's
//! `net::SQLitePersistentCookieStore`): the persistent cookies, read in all
//! at once when the network context starts and written in batches, on a
//! thread of its own so the network service's never waits on the disk.
//! Session cookies are never written. Values are kept as they are, not
//! encrypted (Chromium encrypts them with the system's key store).

use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use sql::Database;
use sql::rusqlite::{self, params};
use tokio::sync::oneshot;

use super::canonical_cookie::CanonicalCookie;
use crate::database_thread::DatabaseThread;
use super::cookie_constants::{CookieSameSite, CookieSourceScheme, CookieSourceType};
use super::cookie_monster::PersistentCookieStore;
use super::cookie_partition_key::CookiePartitionKey;
use super::time::Time;

/// The database's file in the profile's folder (Chromium's
/// `chrome::kCookieFilename`).
const DATABASE: &str = "Cookies";

/// Each version of the schema, from the one before. Chromium's columns,
/// but for `encrypted_value` (values are kept as they are), `priority`
/// (not parsed: every cookie is of medium priority), `has_expires` and
/// `is_persistent` (only persistent cookies are written) and
/// `last_update_utc`. A cookie is its name, domain, path and partition,
/// as the jar has one of each; times are microseconds since the Unix
/// epoch (Chromium's, since 1601).
const MIGRATIONS: &[&str] = &["CREATE TABLE cookies (
        creation_utc INTEGER NOT NULL,
        host_key TEXT NOT NULL,
        top_frame_site_key TEXT NOT NULL,
        has_cross_site_ancestor INTEGER NOT NULL,
        name TEXT NOT NULL,
        value TEXT NOT NULL,
        path TEXT NOT NULL,
        expires_utc INTEGER NOT NULL,
        is_secure INTEGER NOT NULL,
        is_httponly INTEGER NOT NULL,
        last_access_utc INTEGER NOT NULL,
        samesite INTEGER NOT NULL,
        source_scheme INTEGER NOT NULL,
        source_port INTEGER NOT NULL,
        source_type INTEGER NOT NULL,
        UNIQUE (host_key, top_frame_site_key, has_cross_site_ancestor, name, path)
    );"];

/// When the batched changes are written (Chromium's `kCommitInterval` and
/// `kCommitAfterBatchSize`): `interval` after the first of a batch, or as
/// soon as there are `batch_size`, and when the store closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommitOptions {
    pub(crate) interval: Duration,
    pub(crate) batch_size: usize,
}

impl Default for CommitOptions {
    /// Chromium's: 30 seconds, 512 changes.
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            batch_size: 512,
        }
    }
}

/// Which row a cookie is: its name, domain, path and partition (the
/// table's unique columns; an unpartitioned cookie's site is empty).
#[derive(Debug, Clone, PartialEq, Eq)]
struct RowKey {
    host_key: String,
    top_frame_site_key: String,
    has_cross_site_ancestor: bool,
    name: String,
    path: String,
}

impl RowKey {
    /// `cookie`'s row; none for a cookie of an opaque partition, which
    /// isn't written (no later machopo can be in that partition).
    fn of(cookie: &CanonicalCookie) -> Option<Self> {
        let (top_frame_site_key, has_cross_site_ancestor) = match cookie.partition_key() {
            Some(key) => key.serialize()?,
            None => (String::new(), false),
        };
        Some(Self {
            host_key: cookie.domain().to_owned(),
            top_frame_site_key,
            has_cross_site_ancestor,
            name: cookie.name().to_owned(),
            path: cookie.path().to_owned(),
        })
    }
}

/// A change to the database, as the thread is told of it (Chromium's
/// `PendingOperation`), and the order to close.
enum Op {
    Add(Box<CanonicalCookie>),
    UpdateAccessTime(RowKey, Time),
    Delete(RowKey),
    Close,
}

/// The jar's way to the database thread: it is told of each change to a
/// persistent cookie as it happens, and batches them.
pub(crate) struct SqlitePersistentCookieStore {
    ops: Sender<Op>,
}

impl PersistentCookieStore for SqlitePersistentCookieStore {
    fn add_cookie(&mut self, cookie: &CanonicalCookie) {
        if RowKey::of(cookie).is_some() {
            // Gone only once the thread has closed: no more is written.
            let _ = self.ops.send(Op::Add(Box::new(cookie.clone())));
        }
    }

    fn update_cookie_access_time(&mut self, cookie: &CanonicalCookie) {
        if let Some(row) = RowKey::of(cookie) {
            let _ = self
                .ops
                .send(Op::UpdateAccessTime(row, cookie.last_access_date()));
        }
    }

    fn delete_cookie(&mut self, cookie: &CanonicalCookie) {
        if let Some(row) = RowKey::of(cookie) {
            let _ = self.ops.send(Op::Delete(row));
        }
    }
}

/// The database thread, which ends (what it was told written) once this
/// is dropped, or closed: the network context's handles keep it, so the
/// last of them to go waits for the writes, wherever the jar is.
pub(crate) struct CookieStoreThread {
    ops: Sender<Op>,
    thread: DatabaseThread,
}

impl CookieStoreThread {
    /// Tells the thread to end as machopo ends, what it was told written:
    /// what the jar tells the store after is not.
    pub(crate) fn close(&self) {
        self.thread.closing();
        let _ = self.ops.send(Op::Close);
    }

    /// Whether the thread has ended by `deadline`.
    pub(crate) fn ended_by(&self, deadline: Instant) -> bool {
        self.thread.ended_by(deadline)
    }
}

impl Drop for CookieStoreThread {
    fn drop(&mut self) {
        let _ = self.ops.send(Op::Close);
        self.thread.join();
    }
}

/// The cookies read from the database, once they are.
pub(crate) type Loaded = oneshot::Receiver<Vec<CanonicalCookie>>;

/// Opens the `Cookies` database of the profile's folder `dir` on a thread
/// of its own, which reads in every persistent cookie not expired (and
/// deletes the expired), then writes the changes it is told of as
/// `options` say: the store the jar tells them to, the thread's handle,
/// and the cookies read, once they are (none if the database can't be
/// opened: the cookies are then kept in memory alone, as Chromium's are).
/// `committed`, if given, is told how many changes each commit wrote.
pub(crate) fn open(
    dir: &Path,
    options: CommitOptions,
    committed: Option<Sender<usize>>,
) -> std::io::Result<(SqlitePersistentCookieStore, CookieStoreThread, Loaded)> {
    let (ops, received) = mpsc::channel();
    let (loaded, loaded_rx) = oneshot::channel();
    let dir = dir.to_owned();
    let thread = DatabaseThread::spawn("CookieStore", move || {
        run(&dir, options, &received, loaded, committed.as_ref());
    })?;
    Ok((
        SqlitePersistentCookieStore { ops: ops.clone() },
        CookieStoreThread { ops, thread },
        loaded_rx,
    ))
}

fn run(
    dir: &Path,
    options: CommitOptions,
    ops: &Receiver<Op>,
    loaded: oneshot::Sender<Vec<CanonicalCookie>>,
    committed: Option<&Sender<usize>>,
) {
    let opened = Database::open(Some(dir), DATABASE).and_then(|mut database| {
        database.migrate(MIGRATIONS)?;
        Ok(database)
    });
    let mut database = match opened {
        Ok(database) => database,
        Err(error) => {
            tracing::warn!(%error, "couldn't open the cookie database: cookies kept in memory");
            let _ = loaded.send(Vec::new());
            // Until the network context goes: nothing to write them to.
            while !matches!(ops.recv(), Ok(Op::Close) | Err(_)) {}
            return;
        }
    };
    let cookies = load(database.connection(), Time::now()).unwrap_or_else(|error| {
        tracing::warn!(%error, "couldn't read the cookie database");
        Vec::new()
    });
    // The network context may have gone already.
    let _ = loaded.send(cookies);

    let write = |database: &mut Database, pending: &mut Vec<Op>| {
        let count = pending.len();
        commit(database.connection(), pending);
        if count > 0
            && let Some(committed) = committed
        {
            let _ = committed.send(count);
        }
    };
    let mut pending = Vec::new();
    let mut since = Instant::now();
    loop {
        let op = if pending.is_empty() {
            ops.recv().map_err(|_| RecvTimeoutError::Disconnected)
        } else {
            ops.recv_timeout(options.interval.saturating_sub(since.elapsed()))
        };
        match op {
            Ok(Op::Close) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => write(&mut database, &mut pending),
            Ok(op) => {
                if pending.is_empty() {
                    since = Instant::now();
                }
                pending.push(op);
                if pending.len() >= options.batch_size {
                    write(&mut database, &mut pending);
                }
            }
        }
    }
    write(&mut database, &mut pending);
}

/// Deletes the cookies expired by `now` and reads the others
/// (`LoadCookiesForKey` of every key, `MakeCookiesFromSQLStatement`): a row
/// that isn't a cookie `create` could have made is left out.
fn load(
    connection: &mut rusqlite::Connection,
    now: Time,
) -> rusqlite::Result<Vec<CanonicalCookie>> {
    connection.execute(
        "DELETE FROM cookies WHERE expires_utc <= ?1",
        params![now.unix_micros()],
    )?;
    let mut statement = connection.prepare(
        "SELECT creation_utc, host_key, top_frame_site_key, has_cross_site_ancestor, name, value, \
         path, expires_utc, is_secure, is_httponly, last_access_utc, samesite, source_scheme, \
         source_port, source_type FROM cookies",
    )?;
    let rows = statement.query_map([], |row| {
        let site: String = row.get(2)?;
        let partition_key = if site.is_empty() {
            Some(None)
        } else {
            CookiePartitionKey::from_storage(&site, row.get(3)?).map(Some)
        };
        Ok(partition_key.and_then(|partition_key| {
            CanonicalCookie::from_storage(
                row.get(4).ok()?,
                row.get(5).ok()?,
                row.get(1).ok()?,
                row.get(6).ok()?,
                Time::from_unix_micros(row.get(0).ok()?),
                Some(Time::from_unix_micros(row.get(7).ok()?)),
                Time::from_unix_micros(row.get(10).ok()?),
                row.get(8).ok()?,
                row.get(9).ok()?,
                same_site_from_storage(row.get(11).ok()?)?,
                partition_key,
                source_scheme_from_storage(row.get(12).ok()?)?,
                source_port_from_storage(row.get(13).ok()?)?,
                source_type_from_storage(row.get(14).ok()?)?,
            )
        }))
    })?;
    let mut cookies = Vec::new();
    for row in rows {
        match row? {
            Some(cookie) => cookies.push(cookie),
            None => tracing::warn!("a row of the cookie database isn't a cookie: left out"),
        }
    }
    Ok(cookies)
}

/// Writes `pending`, in order, in one transaction (`Commit`), and empties
/// it. A batch that fails is dropped, as Chromium's is.
fn commit(connection: &mut rusqlite::Connection, pending: &mut Vec<Op>) {
    if pending.is_empty() {
        return;
    }
    let written = connection.transaction().and_then(|transaction| {
        for op in pending.drain(..) {
            match op {
                Op::Add(cookie) => add(&transaction, &cookie)?,
                Op::UpdateAccessTime(row, last_access) => transaction.execute(
                    "UPDATE cookies SET last_access_utc = ?6 WHERE host_key = ?1 AND \
                     top_frame_site_key = ?2 AND has_cross_site_ancestor = ?3 AND name = ?4 AND \
                     path = ?5",
                    params![
                        row.host_key,
                        row.top_frame_site_key,
                        row.has_cross_site_ancestor,
                        row.name,
                        row.path,
                        last_access.unix_micros()
                    ],
                )?,
                Op::Delete(row) => transaction.execute(
                    "DELETE FROM cookies WHERE host_key = ?1 AND top_frame_site_key = ?2 AND \
                     has_cross_site_ancestor = ?3 AND name = ?4 AND path = ?5",
                    params![
                        row.host_key,
                        row.top_frame_site_key,
                        row.has_cross_site_ancestor,
                        row.name,
                        row.path
                    ],
                )?,
                Op::Close => 0,
            };
        }
        transaction.commit()
    });
    pending.clear();
    if let Err(error) = written {
        tracing::warn!(%error, "couldn't write the cookie database");
    }
}

/// Writes `cookie`'s row, in place of the one of its name, domain, path
/// and partition if there is one.
fn add(
    transaction: &rusqlite::Transaction<'_>,
    cookie: &CanonicalCookie,
) -> rusqlite::Result<usize> {
    let Some(row) = RowKey::of(cookie) else {
        return Ok(0);
    };
    transaction.execute(
        "INSERT OR REPLACE INTO cookies (creation_utc, host_key, top_frame_site_key, \
         has_cross_site_ancestor, name, value, path, expires_utc, is_secure, is_httponly, \
         last_access_utc, samesite, source_scheme, source_port, source_type) VALUES (?1, ?2, ?3, \
         ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            cookie.creation_date().unix_micros(),
            row.host_key,
            row.top_frame_site_key,
            row.has_cross_site_ancestor,
            row.name,
            cookie.value(),
            row.path,
            // Only persistent cookies are written.
            cookie.expiry_date().unwrap_or(Time::MAX).unix_micros(),
            cookie.is_secure(),
            cookie.is_http_only(),
            cookie.last_access_date().unix_micros(),
            same_site_to_storage(cookie.same_site()),
            source_scheme_to_storage(cookie.source_scheme()),
            cookie.source_port().map_or(-1, i64::from),
            source_type_to_storage(cookie.source_type()),
        ],
    )
}

// The enums as Chromium's database writes them (`CookieSameSiteForStorage`,
// `CookieSourceScheme`, `CookieSourceType`).

fn same_site_to_storage(same_site: CookieSameSite) -> i64 {
    match same_site {
        CookieSameSite::Unspecified => -1,
        CookieSameSite::NoRestriction => 0,
        CookieSameSite::LaxMode => 1,
        CookieSameSite::StrictMode => 2,
    }
}

fn same_site_from_storage(value: i64) -> Option<CookieSameSite> {
    Some(match value {
        -1 => CookieSameSite::Unspecified,
        0 => CookieSameSite::NoRestriction,
        1 => CookieSameSite::LaxMode,
        2 => CookieSameSite::StrictMode,
        _ => return None,
    })
}

fn source_scheme_to_storage(scheme: CookieSourceScheme) -> i64 {
    match scheme {
        CookieSourceScheme::NonSecure => 1,
        CookieSourceScheme::Secure => 2,
    }
}

fn source_scheme_from_storage(value: i64) -> Option<CookieSourceScheme> {
    Some(match value {
        1 => CookieSourceScheme::NonSecure,
        2 => CookieSourceScheme::Secure,
        _ => return None,
    })
}

/// A port as the database keeps it: -1 for none (`url::PORT_UNSPECIFIED`).
fn source_port_from_storage(value: i64) -> Option<Option<u16>> {
    match value {
        -1 => Some(None),
        port => u16::try_from(port).ok().map(Some),
    }
}

fn source_type_to_storage(source: CookieSourceType) -> i64 {
    match source {
        CookieSourceType::Http => 1,
        CookieSourceType::Script => 2,
        CookieSourceType::Other => 3,
    }
}

fn source_type_from_storage(value: i64) -> Option<CookieSourceType> {
    Some(match value {
        1 => CookieSourceType::Http,
        2 => CookieSourceType::Script,
        3 => CookieSourceType::Other,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use site::{AncestorChainBit, Site};
    use url::Url;

    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "machopo-cookie-store-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cookie(url: &str, line: &str, partition: Option<&CookiePartitionKey>) -> CanonicalCookie {
        CanonicalCookie::create(
            &Url::parse(url).unwrap(),
            line,
            Time::now(),
            None,
            partition,
            CookieSourceType::Http,
        )
        .unwrap()
    }

    fn partition(site: &str) -> CookiePartitionKey {
        CookiePartitionKey::new(
            Site::of_url(&Url::parse(site).unwrap()),
            AncestorChainBit::CrossSite,
        )
    }

    /// Opens the store of `dir` and waits for its cookies.
    fn open_loaded(
        dir: &Path,
        options: CommitOptions,
        committed: Option<Sender<usize>>,
    ) -> (
        SqlitePersistentCookieStore,
        CookieStoreThread,
        Vec<CanonicalCookie>,
    ) {
        let (store, thread, loaded) = open(dir, options, committed).unwrap();
        (store, thread, loaded.blocking_recv().unwrap())
    }

    fn sorted(mut cookies: Vec<CanonicalCookie>) -> Vec<CanonicalCookie> {
        cookies.sort_by(|a, b| {
            (a.name(), a.partition_key().is_some()).cmp(&(b.name(), b.partition_key().is_some()))
        });
        cookies
    }

    /// What the store is told is in the database once its thread has gone:
    /// each cookie as it was (but for its last use, as updated), in its
    /// partition, one of each name, domain, path and partition; deleted
    /// ones gone; a cookie of an opaque partition never.
    #[test]
    fn the_database_keeps_what_the_store_is_told() {
        let dir = scratch("keeps");
        let (_, _, loaded) = open_loaded(&dir, CommitOptions::default(), None);
        assert!(loaded.is_empty());
        let one = cookie(
            "https://www.a.example/p/q",
            "one=1; Domain=a.example; Path=/p; Max-Age=3600; Secure; HttpOnly; SameSite=Strict",
            None,
        );
        let mut two = cookie(
            "http://localhost:8080/",
            "two=2; Max-Age=3600; SameSite=Lax",
            None,
        );
        let in_partition = cookie(
            "https://a.example/",
            "two=3; Max-Age=3600; Secure; Partitioned; SameSite=None",
            Some(&partition("https://top.example")),
        );
        let opaque = cookie(
            "https://a.example/",
            "opaque=1; Max-Age=3600; Secure; Partitioned",
            Some(&CookiePartitionKey::new(
                Site::of_url(&Url::parse("data:,").unwrap()),
                AncestorChainBit::CrossSite,
            )),
        );
        let gone = cookie("https://a.example/", "gone=1; Max-Age=3600", None);
        let replaced = cookie(
            "https://a.example/",
            "one=0; Domain=a.example; Path=/p; Max-Age=60",
            None,
        );
        {
            let (mut store, _thread, _) = open_loaded(&dir, CommitOptions::default(), None);
            store.add_cookie(&replaced);
            for cookie in [&one, &two, &in_partition, &opaque, &gone] {
                store.add_cookie(cookie);
            }
            two.set_last_access_date(two.last_access_date().saturating_add_seconds(90));
            store.update_cookie_access_time(&two);
            store.delete_cookie(&gone);
        }
        let (_, thread, loaded) = open_loaded(&dir, CommitOptions::default(), None);
        assert_eq!(sorted(loaded), [one, two, in_partition]);
        drop(thread);
        let mut database = Database::open(Some(&dir), DATABASE).unwrap();
        let rows: i64 = database
            .connection()
            .query_row("SELECT count(*) FROM cookies", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 3);
    }

    /// Changes are written `interval` after the first of a batch, or once
    /// there are `batch_size`, and the rest when the store closes.
    #[test]
    fn changes_are_written_in_batches() {
        let dir = scratch("batches");
        let (committed, commits) = mpsc::channel();
        let options = CommitOptions {
            interval: Duration::from_millis(300),
            batch_size: 1000,
        };
        let (mut store, thread, _) = open_loaded(&dir, options, Some(committed.clone()));
        let at = |name: &str| cookie("https://a.example/", &format!("{name}=1; Max-Age=60"), None);
        store.add_cookie(&at("a"));
        store.add_cookie(&at("b"));
        assert!(commits.recv_timeout(Duration::from_millis(50)).is_err());
        assert_eq!(commits.recv_timeout(Duration::from_secs(10)), Ok(2));
        store.add_cookie(&at("c"));
        drop(thread);
        assert_eq!(commits.try_recv(), Ok(1));

        let options = CommitOptions {
            interval: Duration::from_secs(3600),
            batch_size: 3,
        };
        let (mut store, _thread, loaded) = open_loaded(&dir, options, Some(committed));
        assert_eq!(loaded.len(), 3);
        for name in ["d", "e", "f", "g"] {
            store.add_cookie(&at(name));
        }
        assert_eq!(commits.recv_timeout(Duration::from_secs(10)), Ok(3));
        assert!(commits.recv_timeout(Duration::from_millis(50)).is_err());
    }

    /// Opening deletes the rows expired by then; a row that isn't a cookie
    /// is left out.
    #[test]
    fn expired_rows_and_rows_that_arent_cookies_are_left_out() {
        let dir = scratch("left-out");
        {
            let mut database = Database::open(Some(&dir), DATABASE).unwrap();
            database.migrate(MIGRATIONS).unwrap();
            let connection = database.connection();
            let transaction = connection.transaction().unwrap();
            let kept = cookie("https://a.example/", "kept=1; Max-Age=60", None);
            add(&transaction, &kept).unwrap();
            let mut expired = cookie("https://a.example/", "expired=1; Max-Age=60", None);
            expired.set_creation_date(Time::UNIX_EPOCH);
            add(&transaction, &expired).unwrap();
            transaction
                .execute(
                    "UPDATE cookies SET expires_utc = 1, creation_utc = 0 WHERE name = 'expired'",
                    [],
                )
                .unwrap();
            // Each but for one column, which isn't a cookie's; `Secure`
            // but where that is what makes it none.
            for (name, column, value) in [
                ("same_site", "samesite", "7"),
                ("scheme", "source_scheme", "0"),
                ("type", "source_type", "0"),
                ("port", "source_port", "70000"),
                ("path", "path", "'p'"),
                ("site", "top_frame_site_key", "'https://www.top.example'"),
                ("insecure", "top_frame_site_key", "'https://top.example'"),
                ("name", "name", "'a;b'"),
                ("none", "samesite", "0"),
                ("backwards", "creation_utc", "expires_utc + 1"),
            ] {
                let secure = if matches!(name, "insecure" | "none") {
                    ""
                } else {
                    "; Secure"
                };
                let line = format!("{name}=1; Max-Age=60{secure}");
                add(&transaction, &cookie("https://a.example/", &line, None)).unwrap();
                transaction
                    .execute(
                        &format!("UPDATE cookies SET {column} = {value} WHERE name = '{name}'"),
                        [],
                    )
                    .unwrap();
            }
            transaction.commit().unwrap();
        }
        let (_, thread, loaded) = open_loaded(&dir, CommitOptions::default(), None);
        let names: Vec<&str> = loaded.iter().map(CanonicalCookie::name).collect();
        assert_eq!(names, ["kept"]);
        drop(thread);
        let mut database = Database::open(Some(&dir), DATABASE).unwrap();
        let expired: i64 = database
            .connection()
            .query_row(
                "SELECT count(*) FROM cookies WHERE name = 'expired'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(expired, 0);
    }

    /// A database that can't be opened has no cookies, and the store
    /// closes all the same: they are kept in memory alone.
    #[test]
    fn a_database_that_cant_be_opened_has_none() {
        let dir = scratch("cant-open");
        std::fs::create_dir(dir.join(DATABASE)).unwrap();
        let (mut store, thread, loaded) = open_loaded(&dir, CommitOptions::default(), None);
        assert!(loaded.is_empty());
        store.add_cookie(&cookie("https://a.example/", "a=1; Max-Age=60", None));
        drop(thread);
    }
}
