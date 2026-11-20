//! A profile (Chromium's `BrowserContext`, over its user data directory):
//! where what the browser keeps of its user's is. One on disk is a folder
//! only one machopo has at a time (Chromium's `ProcessSingleton`); one in
//! memory keeps nothing past the browser's end.

use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The file in a profile's folder that the machopo using it holds locked;
/// it says that process's id.
const LOCK_FILE: &str = "lock";

/// A profile: its folder, held locked, or none for one in memory. Shared
/// by what uses it (a window's tabs, and the threads that keep its data:
/// Chromium's `Profile` outlives its windows), so counted across threads;
/// its folder is let go with the last of them.
#[derive(Debug, Clone)]
pub struct Profile {
    dir: Option<Arc<ProfileDir>>,
}

impl Default for Profile {
    /// One in memory.
    fn default() -> Self {
        Self::in_memory()
    }
}

#[derive(Debug)]
struct ProfileDir {
    path: PathBuf,
    /// The lock, the operating system's on the open file: let go as the
    /// file closes, a process that crashed included.
    _lock: File,
}

/// Why a profile's folder couldn't be opened.
#[derive(Debug)]
pub enum ProfileError {
    /// Another machopo has it, the process `pid` if its lock file says.
    InUse { dir: PathBuf, pid: Option<u32> },
    /// The folder or its lock file couldn't be made or opened.
    Io {
        path: PathBuf,
        error: std::io::Error,
    },
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InUse { dir, pid } => {
                write!(
                    f,
                    "the profile in {} is in use by another machopo",
                    dir.display()
                )?;
                if let Some(pid) = pid {
                    write!(f, " (process {pid})")?;
                }
                write!(
                    f,
                    ": for two at once, give this one another with --user-data-dir"
                )
            }
            Self::Io { path, error } => write!(f, "{}: {error}", path.display()),
        }
    }
}

impl Profile {
    /// One that keeps nothing past the browser's end: what automation and
    /// tests have, each alone with its own.
    pub fn in_memory() -> Self {
        Self { dir: None }
    }

    /// The one in the folder `dir`, made if it isn't there, and locked: it
    /// is `InUse` while another machopo has it.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, ProfileError> {
        let path = dir.into();
        let io = |path: &Path| {
            let path = path.to_owned();
            move |error| ProfileError::Io { path, error }
        };
        std::fs::create_dir_all(&path).map_err(io(&path))?;
        let lock_path = path.join(LOCK_FILE);
        let mut lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(io(&lock_path))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                // Best effort: a locked file may not be read (Windows).
                let mut said = String::new();
                let pid = lock
                    .read_to_string(&mut said)
                    .ok()
                    .and_then(|_| said.trim().parse().ok());
                return Err(ProfileError::InUse { dir: path, pid });
            }
            Err(std::fs::TryLockError::Error(error)) => {
                return Err(ProfileError::Io {
                    path: lock_path,
                    error,
                });
            }
        }
        // Whose it is now, for the one that finds it in use.
        lock.set_len(0)
            .and_then(|()| lock.rewind())
            .and_then(|()| write!(lock, "{}", std::process::id()))
            .and_then(|()| lock.flush())
            .map_err(io(&lock_path))?;
        Ok(Self {
            dir: Some(Arc::new(ProfileDir { path, _lock: lock })),
        })
    }

    /// Its folder: none for one in memory.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_ref().map(|dir| dir.path.as_path())
    }
}

/// The platforms' places for a user's data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Windows,
    /// Linux and the other Unixes: the XDG base directories.
    Xdg,
}

impl Platform {
    /// The one machopo is built for.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Xdg
        }
    }
}

/// The folder of the profile a person's machopo has unless told another
/// (Chromium's `chrome::GetDefaultUserDataDirectory`): none if the
/// environment doesn't say where the user's files are.
pub fn default_user_data_dir() -> Option<PathBuf> {
    user_data_dir_on(Platform::current(), |name| std::env::var_os(name))
}

/// [`default_user_data_dir`] on `platform`, the environment read with
/// `env`: `~/Library/Application Support/machopo` on macOS;
/// `%LOCALAPPDATA%\machopo\User Data` on Windows; elsewhere
/// `$XDG_CONFIG_HOME/machopo`, or `~/.config/machopo` if that isn't set
/// (or isn't an absolute path, which the XDG specification has ignored).
/// A variable set to nothing is as one not set.
pub fn user_data_dir_on(
    platform: Platform,
    env: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    let dir = |name: &str| {
        env(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    match platform {
        Platform::MacOs => Some(
            dir("HOME")?
                .join("Library")
                .join("Application Support")
                .join("machopo"),
        ),
        Platform::Windows => Some(dir("LOCALAPPDATA")?.join("machopo").join("User Data")),
        Platform::Xdg => {
            let config = dir("XDG_CONFIG_HOME")
                .filter(|path| path.is_absolute())
                .or_else(|| Some(dir("HOME")?.join(".config")))?;
            Some(config.join("machopo"))
        }
    }
}
