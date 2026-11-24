//! The profile's databases (Chromium's `//sql`): each kind of data a
//! database of its own, in a file of the profile's folder (or in memory,
//! for a profile there), used on its own thread through its own
//! connection, as Chromium isolates each feature's database and page
//! cache. SQLite, built in (`rusqlite`, which this crate re-exports: what
//! uses a database depends on this crate alone).

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

use std::path::{Path, PathBuf};

pub use rusqlite;
use rusqlite::{Connection, ErrorCode};

/// A database (Chromium's `sql::Database`), open: its connection, which
/// its user's thread has alone.
#[derive(Debug)]
pub struct Database {
    connection: Connection,
    /// Its file; none in memory.
    path: Option<PathBuf>,
    synchronous: Synchronous,
}

/// When a commit waits for the disk (SQLite's `PRAGMA synchronous`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Synchronous {
    /// Each commit synced (SQLite's default): the file survives the system
    /// crashing or losing power.
    #[default]
    Full,
    /// None: a commit is done once the system has it. The file survives
    /// machopo crashing, not the system: for data that can be lost, as a
    /// cache (Chromium's SQL disk cache backend sets it so).
    Off,
}

/// Why a database couldn't be opened or brought to its schema.
#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    /// A newer machopo made it, at a version of its schema this one doesn't
    /// know.
    Newer {
        version: u32,
        known: u32,
    },
    /// The file of one that wasn't a database couldn't be put aside.
    Io {
        path: PathBuf,
        error: std::io::Error,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(error) => write!(f, "{error}"),
            Self::Newer { version, known } => write!(
                f,
                "its schema is at version {version}, which a newer machopo made (this one knows \
                 {known})"
            ),
            Self::Io { path, error } => write!(f, "{}: {error}", path.display()),
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl Database {
    /// The database `name` in the profile's folder `dir` (a file of that
    /// name, made if it isn't there), or in memory if `dir` is none, set up
    /// as Chromium's `sql::Database` is by default: exclusive locking (no
    /// other connection opens it while this one has it), a rollback
    /// journal truncated at each commit, every commit synced. A file that
    /// isn't a database (or is one damaged past reading its schema) is put
    /// aside as `name.corrupt` and a new one made in its place (Chromium's
    /// `Database::RazeAndPoison`, after which its users start over).
    pub fn open(dir: Option<&Path>, name: &str) -> Result<Self, Error> {
        Self::open_with(dir, name, Synchronous::Full)
    }

    /// [`Self::open`], its commits as `synchronous` says.
    pub fn open_with(
        dir: Option<&Path>,
        name: &str,
        synchronous: Synchronous,
    ) -> Result<Self, Error> {
        let Some(dir) = dir else {
            return Self::set_up(Connection::open_in_memory()?, None, synchronous);
        };
        let path = dir.join(name);
        match Connection::open(&path)
            .map_err(Error::from)
            .and_then(|connection| Self::set_up(connection, Some(path.clone()), synchronous))
        {
            Err(Error::Sqlite(error)) if is_corrupt(&error) => {
                let aside = dir.join(format!("{name}.corrupt"));
                tracing::warn!(path = %path.display(), %error, "not a database: put aside, made again");
                std::fs::rename(&path, &aside).map_err(|error| Error::Io {
                    path: path.clone(),
                    error,
                })?;
                Self::set_up(Connection::open(&path)?, Some(path), synchronous)
            }
            opened => opened,
        }
    }

    fn set_up(
        connection: Connection,
        path: Option<PathBuf>,
        synchronous: Synchronous,
    ) -> Result<Self, Error> {
        // Locked is another connection's having it, which none should:
        // fail at once, as SQLite does unless told to wait (rusqlite waits
        // 5 seconds).
        connection.busy_timeout(std::time::Duration::ZERO)?;
        connection.pragma_update(None, "locking_mode", "EXCLUSIVE")?;
        // It answers with the mode it has (in memory, `memory`).
        let _: String =
            connection.query_row("PRAGMA journal_mode=TRUNCATE", [], |row| row.get(0))?;
        if synchronous == Synchronous::Off {
            connection.pragma_update(None, "synchronous", "OFF")?;
        }
        // Reading the schema takes the lock now, and finds a file that
        // isn't a database.
        let _: i64 =
            connection.query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))?;
        Ok(Self {
            connection,
            path,
            synchronous,
        })
    }

    /// Starts it over, empty, at version 0, set up as it was: its file
    /// (and journal) deleted and made again (Chromium's `Database::Raze`,
    /// for one found damaged while in use). One in memory is a new one.
    pub fn raze(&mut self) -> Result<(), Error> {
        // Closed first: the lock and the file go with the connection.
        drop(std::mem::replace(
            &mut self.connection,
            Connection::open_in_memory()?,
        ));
        let connection = match &self.path {
            Some(path) => {
                let mut journal = path.clone().into_os_string();
                journal.push("-journal");
                for file in [path.clone(), PathBuf::from(journal)] {
                    match std::fs::remove_file(&file) {
                        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                            return Err(Error::Io { path: file, error });
                        }
                        _ => {}
                    }
                }
                Connection::open(path)?
            }
            None => Connection::open_in_memory()?,
        };
        *self = Self::set_up(connection, self.path.take(), self.synchronous)?;
        Ok(())
    }

    /// Brings its schema to version `migrations.len()`: each migration
    /// after the version it is at (`PRAGMA user_version`) runs in a
    /// transaction of its own, with the version it brings it to (Chromium's
    /// `sql::MetaTable`). One a newer machopo made, past them, is `Newer`.
    pub fn migrate(&mut self, migrations: &[&str]) -> Result<(), Error> {
        let known = u32::try_from(migrations.len()).unwrap_or(u32::MAX);
        let version = self.version()?;
        if version > known {
            return Err(Error::Newer { version, known });
        }
        // Below `known`, which is `migrations.len()`: it fits.
        let done = usize::try_from(version).unwrap_or(usize::MAX);
        for (at, migration) in (version..known).zip(migrations.iter().skip(done)) {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(migration)?;
            transaction.pragma_update(None, "user_version", at.saturating_add(1))?;
            transaction.commit()?;
        }
        Ok(())
    }

    /// Its schema's version: 0 for a new one.
    pub fn version(&self) -> Result<u32, Error> {
        Ok(self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }

    /// Its connection, to read and write through.
    pub fn connection(&mut self) -> &mut Connection {
        &mut self.connection
    }
}

/// Whether SQLite found the file isn't a database it can read, or is one
/// damaged: what [`Database::raze`] is for.
pub fn is_corrupt(error: &rusqlite::Error) -> bool {
    error
        .sqlite_error_code()
        .is_some_and(|code| matches!(code, ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt))
}
