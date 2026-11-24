//! The HTTP cache on disk (Chromium's SQL disk cache backend,
//! `net/disk_cache/sql/`'s `SqlPersistentStore`): one SQLite database, the
//! profile's cache folder's `Cache`, on a thread of its own so the network
//! service's never waits on the disk. A response is a row of `resources`,
//! its head; its body is rows of `blobs`, pieces of at most
//! [`WRITE_BUFFER`] written as they come. A row is complete once its body
//! is all there: one that isn't (its load still going, or the browser gone
//! before it ended) isn't read, and goes at the next start.
//!
//! Commits don't wait for the disk (`synchronous=OFF`): what was written
//! survives machopo crashing, not the system. A database found damaged in
//! use is razed, and the cache starts over empty (`RazeAndPoison`).

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use bytes::Bytes;
use sql::rusqlite::{self, OptionalExtension, params};
use sql::{Database, Synchronous};
use tokio::sync::oneshot;

use super::entry::StoredResponse;
use super::vary::VaryData;
use crate::cookies::Time;

/// The database's file in the cache folder (Chromium's `Cache_Data` is a
/// folder of its backend's files).
const DATABASE: &str = "Cache";

/// The most of an entry's body held before it is written, a `blobs` row
/// each time (Chromium's `SqlDiskCacheMaxWriteBufferSizePerEntry`).
pub(crate) const WRITE_BUFFER: usize = 512 * 1024;

/// Each version of the schema, from the one before. A response's head is
/// in columns of its own; its `Vary` is `NULL` for none, empty for one no
/// request matches, else its values' SHA-256 (never the values: a
/// `Vary: Cookie` would put the cookie on disk). Times are microseconds
/// since the Unix epoch. A body's piece starts at `start`, its offset in
/// the body.
const MIGRATIONS: &[&str] = &["CREATE TABLE resources (
        res_id INTEGER PRIMARY KEY,
        cache_key TEXT NOT NULL,
        complete INTEGER NOT NULL,
        last_used INTEGER NOT NULL,
        size INTEGER NOT NULL,
        status INTEGER NOT NULL,
        status_text TEXT NOT NULL,
        protocol TEXT NOT NULL,
        headers BLOB NOT NULL,
        request_time INTEGER NOT NULL,
        response_time INTEGER NOT NULL,
        vary BLOB
    );
    CREATE INDEX resources_by_key ON resources (cache_key);
    CREATE TABLE blobs (
        res_id INTEGER NOT NULL,
        start INTEGER NOT NULL,
        blob BLOB NOT NULL,
        PRIMARY KEY (res_id, start)
    ) WITHOUT ROWID;"];

/// A complete entry as the database has it, for the cache's index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexEntry {
    pub(crate) key: String,
    pub(crate) size: u64,
    pub(crate) last_used: Time,
}

/// What the thread is told to do (Chromium's store's operations), and the
/// order to close. A write is known by its key: the cache lets one load
/// at a time write a key.
enum Op {
    Read(String, oneshot::Sender<Option<StoredResponse>>),
    /// A new entry for the key, its head (its body to come): a row not yet
    /// complete.
    Begin(String, Box<StoredResponse>),
    Append(String, Bytes),
    /// The body is all there: the entry replaces the key's last one.
    Finish(String, Time),
    /// The load ended short: what was written of it goes.
    Abandon(String),
    /// The key's complete entry has a new head (a `304`'s), the size it
    /// makes, and was used then.
    UpdateHead(String, Box<StoredResponse>, u64, Time),
    Remove(String),
    Touch(String, Time),
    Clear(Box<dyn FnOnce() + Send>),
    Close,
}

/// The cache's way to the database thread.
#[derive(Clone)]
pub(crate) struct SqlStore {
    ops: Sender<Op>,
}

/// The database thread, which ends (what it was told written) once this is
/// dropped: the network context's handles keep it, so the last of them to
/// go waits for the writes, and a context opened on the folder after finds
/// it unlocked.
pub(crate) struct SqlStoreThread {
    ops: Sender<Op>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for SqlStoreThread {
    fn drop(&mut self) {
        let _ = self.ops.send(Op::Close);
        if let Some(thread) = self.thread.take() {
            // Never the database thread itself: nothing of its holds this.
            let _ = thread.join();
        }
    }
}

/// The complete entries read from the database, once they are.
pub(crate) type Loaded = oneshot::Receiver<Vec<IndexEntry>>;

impl SqlStore {
    /// Opens the `Cache` database of the cache folder `dir` on a thread of
    /// its own, which deletes what wasn't complete and reads in the
    /// complete entries; the store, the thread's handle, and those entries
    /// once they are (none if the database can't be opened: then nothing
    /// is kept). `on_razed` is called on the thread each time the
    /// database, found damaged, is razed: what the cache knew of it is
    /// gone.
    pub(crate) fn open(
        dir: &Path,
        on_razed: impl Fn() + Send + 'static,
    ) -> std::io::Result<(Self, SqlStoreThread, Loaded)> {
        let (ops, received) = mpsc::channel();
        let (loaded, loaded_rx) = oneshot::channel();
        let dir = dir.to_owned();
        let thread = std::thread::Builder::new()
            .name("HttpCacheStore".into())
            .spawn(move || run(&dir, &received, loaded, &on_razed))?;
        Ok((
            Self { ops: ops.clone() },
            SqlStoreThread {
                ops,
                thread: Some(thread),
            },
            loaded_rx,
        ))
    }

    /// The key's complete entry, body and all, once read; none if there
    /// is none, or it can't be read.
    pub(crate) fn read(&self, key: &str) -> oneshot::Receiver<Option<StoredResponse>> {
        let (read, answer) = oneshot::channel();
        self.send(Op::Read(key.to_owned(), read));
        answer
    }

    /// Starts a new entry for `key`: `head` (its body ignored) with the
    /// body [`Self::append`] gives, complete at [`Self::finish`].
    pub(crate) fn begin(&self, key: &str, head: StoredResponse) {
        self.send(Op::Begin(key.to_owned(), Box::new(head)));
    }

    pub(crate) fn append(&self, key: &str, piece: Bytes) {
        self.send(Op::Append(key.to_owned(), piece));
    }

    /// The body is all there: the entry, last used at `now`, replaces the
    /// key's last one.
    pub(crate) fn finish(&self, key: &str, now: Time) {
        self.send(Op::Finish(key.to_owned(), now));
    }

    /// The body won't all come: what was written of it goes.
    pub(crate) fn abandon(&self, key: &str) {
        self.send(Op::Abandon(key.to_owned()));
    }

    /// The key's complete entry has `head` (its body ignored), which makes
    /// it `size`, and was used at `at`.
    pub(crate) fn update_head(&self, key: &str, head: StoredResponse, size: u64, at: Time) {
        self.send(Op::UpdateHead(key.to_owned(), Box::new(head), size, at));
    }

    /// The key's complete entry goes (one being written stays).
    pub(crate) fn remove(&self, key: &str) {
        self.send(Op::Remove(key.to_owned()));
    }

    /// The key's complete entry was used at `at`.
    pub(crate) fn touch(&self, key: &str, at: Time) {
        self.send(Op::Touch(key.to_owned(), at));
    }

    /// Every entry goes, those being written too; then `done`.
    pub(crate) fn clear(&self, done: impl FnOnce() + Send + 'static) {
        self.send(Op::Clear(Box::new(done)));
    }

    fn send(&self, op: Op) {
        // Gone only once the thread has closed.
        let _ = self.ops.send(op);
    }
}

/// An entry being written: its row, and its body's pieces not written yet.
struct Writing {
    res_id: i64,
    buffer: Vec<Bytes>,
    buffered: usize,
    /// Where the next piece written starts in the body.
    written: u64,
    size: u64,
}

/// The thread's: its database (none if it couldn't be opened) and the
/// entries being written.
struct Store<'a> {
    database: Option<Database>,
    writing: HashMap<String, Writing>,
    on_razed: &'a dyn Fn(),
}

fn run(
    dir: &Path,
    ops: &Receiver<Op>,
    loaded: oneshot::Sender<Vec<IndexEntry>>,
    on_razed: &dyn Fn(),
) {
    // The folder made if it isn't there (`CreateProfileDirectory`).
    let opened = std::fs::create_dir_all(dir)
        .map_err(|error| sql::Error::Io {
            path: dir.to_owned(),
            error,
        })
        .and_then(|()| Database::open_with(Some(dir), DATABASE, Synchronous::Off))
        .and_then(|mut db| {
            db.migrate(MIGRATIONS)?;
            Ok(db)
        });
    let database = opened
        .inspect_err(|error| {
            tracing::warn!(%error, "couldn't open the HTTP cache's database: nothing is kept");
        })
        .ok();
    let mut store = Store {
        database,
        writing: HashMap::new(),
        on_razed,
    };
    let entries = store.load().unwrap_or_default();
    // The cache may have gone already.
    let _ = loaded.send(entries);
    loop {
        match ops.recv() {
            Ok(Op::Close) | Err(_) => break,
            Ok(op) => store.handle(op),
        }
    }
}

impl Store<'_> {
    fn handle(&mut self, op: Op) {
        match op {
            Op::Read(key, read) => {
                let response = self.with(|db| read_entry(db, &key)).flatten();
                let _ = read.send(response);
            }
            Op::Begin(key, head) => self.begin(key, &head),
            Op::Append(key, piece) => self.append(&key, piece),
            Op::Finish(key, now) => self.finish(&key, now),
            Op::Abandon(key) => {
                if let Some(writing) = self.writing.remove(&key) {
                    self.with(|db| delete_rows(db, writing.res_id));
                }
            }
            Op::UpdateHead(key, head, size, at) => self.update_head(&key, &head, size, at),
            Op::Remove(key) => {
                self.with(|db| delete_complete(db, &key));
            }
            Op::Touch(key, at) => {
                self.with(|db| {
                    db.execute(
                        "UPDATE resources SET last_used = ? WHERE cache_key = ? AND complete = 1",
                        params![at.unix_micros(), key],
                    )
                });
            }
            Op::Clear(done) => {
                self.writing.clear();
                self.with(|db| db.execute_batch("DELETE FROM blobs; DELETE FROM resources;"));
                done();
            }
            Op::Close => {}
        }
    }

    /// Deletes what wasn't complete (the browser gone before its load
    /// ended), and reads the rest: the complete entries.
    fn load(&mut self) -> Option<Vec<IndexEntry>> {
        self.with(|db| {
            db.execute_batch(
                "DELETE FROM blobs WHERE res_id IN (SELECT res_id FROM resources WHERE complete = 0);
                 DELETE FROM resources WHERE complete = 0;",
            )?;
            let mut statement =
                db.prepare("SELECT cache_key, size, last_used FROM resources")?;
            let rows = statement.query_map([], |row| {
                Ok(IndexEntry {
                    key: row.get(0)?,
                    size: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
                    last_used: Time::from_unix_micros(row.get(2)?),
                })
            })?;
            rows.collect()
        })
    }

    fn begin(&mut self, key: String, head: &StoredResponse) {
        // A write of the key not finished (its load gone without a word)
        // gives way.
        if let Some(old) = self.writing.remove(&key) {
            self.with(|db| delete_rows(db, old.res_id));
        }
        let Some(headers) = encode_headers(&head.headers) else {
            return;
        };
        let vary = vary_column(head.vary.as_ref());
        let res_id = self.with(|db| {
            db.execute(
                "INSERT INTO resources (cache_key, complete, last_used, size, status, status_text,
                     protocol, headers, request_time, response_time, vary)
                 VALUES (?, 0, 0, 0, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    key,
                    head.status,
                    head.status_text,
                    head.protocol,
                    headers,
                    head.request_time.unix_micros(),
                    head.response_time.unix_micros(),
                    vary,
                ],
            )?;
            Ok(db.last_insert_rowid())
        });
        if let Some(res_id) = res_id {
            let size = StoredResponse {
                body: Vec::new(),
                ..head.clone()
            }
            .size();
            self.writing.insert(
                key,
                Writing {
                    res_id,
                    buffer: Vec::new(),
                    buffered: 0,
                    written: 0,
                    size,
                },
            );
        }
    }

    fn update_head(&mut self, key: &str, head: &StoredResponse, size: u64, at: Time) {
        let Some(headers) = encode_headers(&head.headers) else {
            return;
        };
        let vary = vary_column(head.vary.as_ref());
        self.with(|db| {
            db.execute(
                "UPDATE resources SET last_used = ?, size = ?, status = ?, status_text = ?,
                     protocol = ?, headers = ?, request_time = ?, response_time = ?, vary = ?
                 WHERE cache_key = ? AND complete = 1",
                params![
                    at.unix_micros(),
                    i64::try_from(size).unwrap_or(i64::MAX),
                    head.status,
                    head.status_text,
                    head.protocol,
                    headers,
                    head.request_time.unix_micros(),
                    head.response_time.unix_micros(),
                    vary,
                    key,
                ],
            )
        });
    }

    fn append(&mut self, key: &str, piece: Bytes) {
        let Some(writing) = self.writing.get_mut(key) else {
            // Cleared, or razed, since it began.
            return;
        };
        writing.size = writing
            .size
            .saturating_add(u64::try_from(piece.len()).unwrap_or(u64::MAX));
        writing.buffered = writing.buffered.saturating_add(piece.len());
        writing.buffer.push(piece);
        if writing.buffered >= WRITE_BUFFER {
            self.flush(key);
        }
    }

    /// Writes what `key`'s write holds of its body as a piece; a write
    /// that fails is abandoned.
    fn flush(&mut self, key: &str) {
        let Some(writing) = self.writing.get_mut(key) else {
            return;
        };
        if writing.buffer.is_empty() {
            return;
        }
        let piece = std::mem::take(&mut writing.buffer).concat();
        writing.buffered = 0;
        let (res_id, start) = (writing.res_id, writing.written);
        writing.written = start.saturating_add(u64::try_from(piece.len()).unwrap_or(u64::MAX));
        let written = self.with(|db| {
            db.execute(
                "INSERT INTO blobs (res_id, start, blob) VALUES (?, ?, ?)",
                params![res_id, i64::try_from(start).unwrap_or(i64::MAX), piece],
            )
        });
        if written.is_none()
            && let Some(writing) = self.writing.remove(key)
        {
            self.with(|db| delete_rows(db, writing.res_id));
        }
    }

    fn finish(&mut self, key: &str, now: Time) {
        self.flush(key);
        let Some(writing) = self.writing.remove(key) else {
            return;
        };
        let size = i64::try_from(writing.size).unwrap_or(i64::MAX);
        let finished = self.with(|db| {
            let transaction = db.transaction()?;
            delete_complete(&transaction, key)?;
            transaction.execute(
                "UPDATE resources SET complete = 1, size = ?, last_used = ? WHERE res_id = ?",
                params![size, now.unix_micros(), writing.res_id],
            )?;
            transaction.commit()
        });
        if finished.is_none() {
            self.with(|db| delete_rows(db, writing.res_id));
        }
    }

    /// `f` on the connection; none if there is no database, or it failed:
    /// one found damaged is razed (the cache starts over), another error
    /// is logged.
    fn with<T>(
        &mut self,
        f: impl FnOnce(&mut rusqlite::Connection) -> rusqlite::Result<T>,
    ) -> Option<T> {
        let database = self.database.as_mut()?;
        match f(database.connection()) {
            Ok(value) => Some(value),
            Err(error) if sql::is_corrupt(&error) => {
                tracing::warn!(%error, "the HTTP cache's database is damaged: started over");
                self.raze();
                None
            }
            Err(error) => {
                tracing::warn!(%error, "the HTTP cache's database failed");
                None
            }
        }
    }

    fn raze(&mut self) {
        self.writing.clear();
        let razed = self.database.as_mut().map(|database| {
            database.raze()?;
            database.migrate(MIGRATIONS)
        });
        if let Some(Err(error)) = razed {
            tracing::warn!(%error, "couldn't start the HTTP cache's database over: nothing is kept");
            self.database = None;
        }
        (self.on_razed)();
    }
}

/// The `vary` column of a `Vary`: never its values, their digest.
fn vary_column(vary: Option<&VaryData>) -> Option<Vec<u8>> {
    vary.map(|vary| {
        vary.digest()
            .map_or_else(Vec::new, |digest| digest.to_vec())
    })
}

/// The key's complete entry (and its body), if any.
fn delete_complete(db: &rusqlite::Connection, key: &str) -> rusqlite::Result<usize> {
    db.execute(
        "DELETE FROM blobs WHERE res_id IN
             (SELECT res_id FROM resources WHERE cache_key = ? AND complete = 1)",
        [key],
    )?;
    db.execute(
        "DELETE FROM resources WHERE cache_key = ? AND complete = 1",
        [key],
    )
}

fn delete_rows(db: &rusqlite::Connection, res_id: i64) -> rusqlite::Result<usize> {
    db.execute("DELETE FROM blobs WHERE res_id = ?", [res_id])?;
    db.execute("DELETE FROM resources WHERE res_id = ?", [res_id])
}

/// The key's complete entry, its body's pieces in order; none if there is
/// none, or its row isn't one this machopo could have written.
fn read_entry(db: &rusqlite::Connection, key: &str) -> rusqlite::Result<Option<StoredResponse>> {
    let row = db
        .query_row(
            "SELECT res_id, status, status_text, protocol, headers, request_time, response_time,
                 vary
             FROM resources WHERE cache_key = ? AND complete = 1",
            [key],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Option<Vec<u8>>>(7)?,
                ))
            },
        )
        .optional()?;
    let Some((res_id, status, status_text, protocol, headers, request, response, vary)) = row
    else {
        return Ok(None);
    };
    let mut statement = db.prepare("SELECT blob FROM blobs WHERE res_id = ? ORDER BY start")?;
    let body = statement
        .query_map([res_id], |row| row.get::<_, Vec<u8>>(0).map(Bytes::from))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let vary = match vary {
        None => Some(None),
        Some(bytes) if bytes.is_empty() => Some(Some(VaryData::Star)),
        Some(bytes) => <[u8; 32]>::try_from(bytes)
            .ok()
            .map(|digest| Some(VaryData::Digest(digest))),
    };
    let response = (|| -> Option<StoredResponse> {
        Some(StoredResponse {
            status: u16::try_from(status).ok()?,
            status_text,
            protocol: known_protocol(&protocol)?,
            headers: decode_headers(&headers)?,
            request_time: Time::from_unix_micros(request),
            response_time: Time::from_unix_micros(response),
            vary: vary?,
            body,
        })
    })();
    Ok(response)
}

/// The protocol's name as the transport gives it (`protocol_name`).
fn known_protocol(name: &str) -> Option<&'static str> {
    ["http/0.9", "http/1.0", "http/1.1", "h2", "h3"]
        .into_iter()
        .find(|known| *known == name)
}

/// Each header's name and value, each its length (4 bytes, little endian)
/// and its bytes; none for a header too long for its length.
fn encode_headers(headers: &[(String, String)]) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    for text in headers.iter().flat_map(|(name, value)| [name, value]) {
        bytes.extend_from_slice(&u32::try_from(text.len()).ok()?.to_le_bytes());
        bytes.extend_from_slice(text.as_bytes());
    }
    Some(bytes)
}

fn decode_headers(mut bytes: &[u8]) -> Option<Vec<(String, String)>> {
    let mut text = || -> Option<Option<String>> {
        if bytes.is_empty() {
            return Some(None);
        }
        let (len, rest) = bytes.split_first_chunk::<4>()?;
        let len = usize::try_from(u32::from_le_bytes(*len)).ok()?;
        let (text, rest) = rest.split_at_checked(len)?;
        bytes = rest;
        Some(Some(String::from_utf8(text.to_vec()).ok()?))
    };
    let mut headers = Vec::new();
    while let Some(name) = text()? {
        headers.push((name, text()??));
    }
    Some(headers)
}

/// How many rows `table` of the database in `dir` has, read once the
/// store has closed.
#[cfg(test)]
pub(crate) fn rows(dir: &Path, table: &str) -> i64 {
    let mut db = Database::open(Some(dir), DATABASE).unwrap();
    db.connection()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "machopo-http-cache-store-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn open(dir: &Path) -> (SqlStore, SqlStoreThread, Vec<IndexEntry>) {
        let (store, thread, loaded) = SqlStore::open(dir, || {}).unwrap();
        (store, thread, loaded.blocking_recv().unwrap())
    }

    fn head() -> StoredResponse {
        StoredResponse {
            status: 200,
            status_text: "OK".to_owned(),
            protocol: "h2",
            headers: vec![
                ("content-type".to_owned(), "text/css".to_owned()),
                ("etag".to_owned(), "\"v1\"".to_owned()),
                ("x-empty".to_owned(), String::new()),
            ],
            request_time: Time::from_unix_micros(1_000),
            response_time: Time::from_unix_micros(2_000),
            vary: None,
            body: vec![Bytes::from_static(b"ignored")],
        }
    }

    fn read(store: &SqlStore, key: &str) -> Option<StoredResponse> {
        store.read(key).blocking_recv().unwrap()
    }

    fn body(response: &StoredResponse) -> Vec<u8> {
        response.body.concat()
    }

    #[test]
    fn an_entry_written_is_read_again_after_a_restart() {
        let dir = scratch("restart");
        let (store, thread, entries) = open(&dir);
        assert_eq!(entries, []);
        store.begin("k", head());
        store.append("k", Bytes::from_static(b"p {"));
        store.append("k", Bytes::from_static(b"}"));
        store.finish("k", Time::from_unix_micros(5_000));
        let read_now = read(&store, "k").unwrap();
        drop(thread);
        let (store, _thread, entries) = open(&dir);
        let again = read(&store, "k").unwrap();
        assert_eq!(read_now, again);
        // Its head as it was, its body as it came, in one piece.
        assert_eq!(
            StoredResponse {
                body: Vec::new(),
                ..again.clone()
            },
            StoredResponse {
                body: Vec::new(),
                ..head()
            }
        );
        assert_eq!(body(&again), b"p {}");
        assert_eq!(again.body.len(), 1);
        // Its size counts its head and body, as the memory backend's.
        let size = StoredResponse {
            body: vec![Bytes::from_static(b"p {}")],
            ..head()
        }
        .size();
        assert_eq!(
            entries,
            [IndexEntry {
                key: "k".to_owned(),
                size,
                last_used: Time::from_unix_micros(5_000),
            }]
        );
        assert_eq!(read(&store, "other"), None);
    }

    #[test]
    fn a_body_is_written_in_pieces_as_it_comes() {
        let dir = scratch("pieces");
        let (store, thread, _) = open(&dir);
        store.begin("k", head());
        let piece = Bytes::from(vec![7u8; 200 * 1024]);
        for _ in 0..6 {
            store.append("k", piece.clone());
        }
        store.finish("k", Time::UNIX_EPOCH);
        let response = read(&store, "k").unwrap();
        assert_eq!(body(&response), [7u8; 1200 * 1024]);
        drop(thread);
        // 600 KiB, 600 KiB: a row each time 512 KiB or more is held.
        assert_eq!(rows(&dir, "blobs"), 2);
    }

    #[test]
    fn what_isnt_complete_isnt_read_and_goes_at_the_next_start() {
        let dir = scratch("incomplete");
        let (store, thread, _) = open(&dir);
        store.begin("k", head());
        store.append("k", Bytes::from(vec![1u8; WRITE_BUFFER]));
        assert_eq!(read(&store, "k"), None);
        // The browser goes before the load ends.
        drop(thread);
        assert_eq!(rows(&dir, "resources"), 1);
        assert_eq!(rows(&dir, "blobs"), 1);
        let (_store, thread, entries) = open(&dir);
        assert_eq!(entries, []);
        drop(thread);
        assert_eq!(rows(&dir, "resources"), 0);
        assert_eq!(rows(&dir, "blobs"), 0);
    }

    #[test]
    fn a_write_begun_again_drops_the_one_before() {
        let dir = scratch("begun-again");
        let (store, thread, _) = open(&dir);
        store.begin("k", head());
        store.append("k", Bytes::from(vec![1u8; WRITE_BUFFER]));
        store.begin("k", head());
        store.append("k", Bytes::from_static(b"x"));
        store.finish("k", Time::UNIX_EPOCH);
        assert_eq!(body(&read(&store, "k").unwrap()), b"x");
        drop(thread);
        assert_eq!(rows(&dir, "resources"), 1);
        assert_eq!(rows(&dir, "blobs"), 1);
    }

    #[test]
    fn an_abandoned_write_goes_and_leaves_the_last_entry() {
        let dir = scratch("abandon");
        let (store, thread, _) = open(&dir);
        store.begin("k", head());
        store.append("k", Bytes::from_static(b"old"));
        store.finish("k", Time::UNIX_EPOCH);
        store.begin("k", head());
        store.append("k", Bytes::from(vec![1u8; WRITE_BUFFER]));
        store.abandon("k");
        assert_eq!(body(&read(&store, "k").unwrap()), b"old");
        drop(thread);
        assert_eq!(rows(&dir, "resources"), 1);
        assert_eq!(rows(&dir, "blobs"), 1);
    }

    #[test]
    fn a_finished_write_replaces_the_last_entry() {
        let dir = scratch("replace");
        let (store, thread, _) = open(&dir);
        for text in [&b"old"[..], b"new"] {
            store.begin("k", head());
            store.append("k", Bytes::copy_from_slice(text));
            // The last one is read until this one is done.
            if text == b"new" {
                assert_eq!(body(&read(&store, "k").unwrap()), b"old");
            }
            store.finish("k", Time::UNIX_EPOCH);
        }
        assert_eq!(body(&read(&store, "k").unwrap()), b"new");
        drop(thread);
        assert_eq!(rows(&dir, "resources"), 1);
        assert_eq!(rows(&dir, "blobs"), 1);
    }

    #[test]
    fn remove_touch_and_clear() {
        let dir = scratch("remove");
        let (store, thread, _) = open(&dir);
        for key in ["a", "b"] {
            store.begin(key, head());
            store.append(key, Bytes::from_static(b"x"));
            store.finish(key, Time::UNIX_EPOCH);
        }
        store.touch("b", Time::from_unix_micros(9));
        store.remove("a");
        assert_eq!(read(&store, "a"), None);
        drop(thread);
        let (store, thread, entries) = open(&dir);
        let entries: Vec<_> = entries
            .into_iter()
            .map(|entry| (entry.key, entry.last_used))
            .collect();
        assert_eq!(entries, [("b".to_owned(), Time::from_unix_micros(9))]);
        // Cleared: those being written too.
        store.begin("c", head());
        let (cleared, done) = oneshot::channel();
        store.clear(move || {
            let _ = cleared.send(());
        });
        done.blocking_recv().unwrap();
        store.append("c", Bytes::from_static(b"x"));
        store.finish("c", Time::UNIX_EPOCH);
        assert_eq!(read(&store, "b"), None);
        assert_eq!(read(&store, "c"), None);
        drop(thread);
        assert_eq!(rows(&dir, "resources"), 0);
        assert_eq!(rows(&dir, "blobs"), 0);
    }

    #[test]
    fn vary_is_kept_as_its_digest() {
        let dir = scratch("vary");
        let (store, thread, _) = open(&dir);
        let request = [("cookie".to_owned(), "id=a-secret".to_owned())];
        let response = [("vary".to_owned(), "Cookie".to_owned())];
        for (key, vary) in [
            ("values", VaryData::new(&request, &response)),
            ("star", Some(VaryData::Star)),
        ] {
            store.begin(key, StoredResponse { vary, ..head() });
            store.finish(key, Time::UNIX_EPOCH);
        }
        let values = read(&store, "values").unwrap().vary.unwrap();
        assert!(matches!(values, VaryData::Digest(_)));
        assert!(values.matches(&request, &response));
        assert!(!values.matches(&[], &response));
        assert_eq!(read(&store, "star").unwrap().vary, Some(VaryData::Star));
        drop(thread);
        let file = std::fs::read(dir.join(DATABASE)).unwrap();
        assert!(!file.windows(11).any(|bytes| bytes == b"id=a-secret"));
    }

    #[test]
    fn a_damaged_database_is_razed_and_the_cache_starts_over() {
        let dir = scratch("razed");
        let (store, thread, _) = open(&dir);
        for key in ["a", "b"] {
            store.begin(key, head());
            store.append(key, Bytes::from(vec![1u8; 64 * 1024]));
            store.finish(key, Time::UNIX_EPOCH);
        }
        drop(thread);
        // Its schema readable, its tables' pages not.
        let path = dir.join(DATABASE);
        let mut file = std::fs::read(&path).unwrap();
        for byte in &mut file[4096..] {
            *byte = 0xa5;
        }
        std::fs::write(&path, file).unwrap();
        let razed = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&razed);
        let (store, thread, loaded) = SqlStore::open(&dir, move || {
            counted.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();
        assert_eq!(loaded.blocking_recv().unwrap(), []);
        assert_eq!(razed.load(Ordering::Relaxed), 1);
        // A new one, which works.
        store.begin("c", head());
        store.append("c", Bytes::from_static(b"x"));
        store.finish("c", Time::UNIX_EPOCH);
        assert_eq!(body(&read(&store, "c").unwrap()), b"x");
        drop(thread);
        assert_eq!(rows(&dir, "resources"), 1);
    }

    #[test]
    fn a_database_it_cant_open_keeps_nothing() {
        let dir = scratch("newer");
        {
            let mut db = Database::open(Some(&dir), DATABASE).unwrap();
            db.connection()
                .pragma_update(None, "user_version", 99)
                .unwrap();
        }
        let (store, _thread, entries) = open(&dir);
        assert_eq!(entries, []);
        store.begin("k", head());
        store.finish("k", Time::UNIX_EPOCH);
        assert_eq!(read(&store, "k"), None);
    }

    #[test]
    fn a_head_is_replaced_and_the_body_kept() {
        let dir = scratch("head");
        let (store, thread, _) = open(&dir);
        store.begin("k", head());
        store.append("k", Bytes::from_static(b"body"));
        store.finish("k", Time::UNIX_EPOCH);
        let request = [("accept".to_owned(), "text/css".to_owned())];
        let new = StoredResponse {
            status_text: "Fine".to_owned(),
            headers: vec![("etag".to_owned(), "\"v2\"".to_owned())],
            request_time: Time::from_unix_micros(7),
            response_time: Time::from_unix_micros(8),
            vary: VaryData::new(&request, &[("vary".to_owned(), "Accept".to_owned())]),
            ..head()
        };
        store.update_head("k", new.clone(), 99, Time::from_unix_micros(9));
        // A key with no complete entry has none to update.
        store.update_head("none", new.clone(), 99, Time::from_unix_micros(9));
        drop(thread);
        let (store, _thread, entries) = open(&dir);
        let read = read(&store, "k").unwrap();
        assert_eq!(
            read,
            StoredResponse {
                vary: new.vary.clone().map(VaryData::digested),
                body: vec![Bytes::from_static(b"body")],
                ..new
            }
        );
        assert_eq!(
            entries,
            [IndexEntry {
                key: "k".to_owned(),
                size: 99,
                last_used: Time::from_unix_micros(9),
            }]
        );
    }

    #[test]
    fn headers_are_read_as_they_were_written() {
        let headers = head().headers;
        let bytes = encode_headers(&headers).unwrap();
        assert_eq!(decode_headers(&bytes).unwrap(), headers);
        assert_eq!(decode_headers(&[]).unwrap(), []);
        // A name without its value, a length past the end.
        assert_eq!(decode_headers(&bytes[..bytes.len() - 6]), None);
        assert_eq!(decode_headers(&[9, 0, 0, 0, b'a']), None);
        assert_eq!(known_protocol("h2"), Some("h2"));
        assert_eq!(known_protocol("spdy"), None);
    }
}
