//! An open file that crosses with a message, to be read (Chromium's
//! `base::File` in a `mojo_base.mojom.ReadOnlyFile`): what a process that
//! may open no files is given to read, by the one that may. It goes in a
//! message's value as a [`ReadOnlyFile`]; the codec moves its handle into
//! the message's (the payload holds which of them it is), and the value
//! read back has the handle the receiving end was given. Only the end
//! that may attach handles sends one.
//!
//! What is read is a mapping of the file ([`MappedFile`]), never its path:
//! the receiving end may have no way to open it by name.

use std::cell::RefCell;
use std::fmt;
use std::fs::File;
use std::io;
use std::ops::Deref;
use std::path::Path;

use serde::de::{self, Deserialize, Deserializer};
use serde::ser::{self, Serialize, Serializer};

/// A file open to read, which a message may take to the peer. A value
/// sent goes on being this end's: the peer is given a handle of its own.
pub struct ReadOnlyFile(File);

impl ReadOnlyFile {
    /// Opens `path` to read. On Windows nobody may open it to write while
    /// any handle to it is open, here or at a peer (it may be renamed or
    /// deleted, which leaves its bytes as they are): a mapping of it is
    /// what a file that changes would change under. Elsewhere that it
    /// doesn't change is an assumption.
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_DELETE, FILE_SHARE_READ};
            options.share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE);
        }
        options.open(path).map(Self)
    }

    /// The file's bytes, mapped to read (a file of no bytes is an empty
    /// mapping). `None` when it isn't a file, or can't be mapped.
    pub fn map(self) -> Option<MappedFile> {
        if !self.0.metadata().ok()?.is_file() {
            return None;
        }
        // SAFETY: the mapping is only read, and only as long as the
        // file stays open (the mapping keeps it). What would make the
        // bytes under it change is the file being written or cut
        // short while it is mapped: on Windows nobody may open it to
        // write while it is open (a value is made by `open`, here or
        // at the peer that sent it, which is trusted: only the end
        // that may attach handles sends any) and a mapped file can't
        // be cut short; elsewhere the files sent are ones nobody
        // writes (the system's fonts), an assumption.
        let map = unsafe { memmap2::Mmap::map(&self.0) }.ok()?;
        Some(MappedFile { map, _file: self.0 })
    }
}

impl fmt::Debug for ReadOnlyFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadOnlyFile").finish_non_exhaustive()
    }
}

/// A file's bytes, mapped to read, for as long as the value is kept.
pub struct MappedFile {
    map: memmap2::Mmap,
    // The file, open for as long as it is mapped: on Windows that is what
    // keeps writers out.
    _file: File,
}

impl Deref for MappedFile {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.map
    }
}

impl AsRef<[u8]> for MappedFile {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl fmt::Debug for MappedFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MappedFile")
            .field("len", &self.len())
            .finish_non_exhaustive()
    }
}

/// What a serializer is told a file is (a newtype struct of this name):
/// how this crate's codec knows one, in a value of any type.
pub(crate) const FILE_NAME: &str = "$ipc::file::ReadOnlyFile";

thread_local! {
    /// The file on its way between a value and the codec, for the length
    /// of one call: serde's data model has no place for a handle, so it
    /// goes beside it. Set and taken on the one thread that is
    /// serializing or deserializing the value.
    static CROSSING: RefCell<Option<File>> = const { RefCell::new(None) };
}

/// The file a value being serialized is sending, for the codec.
pub(crate) fn take_crossing() -> Option<File> {
    CROSSING.with(|crossing| crossing.borrow_mut().take())
}

/// The file the codec has for the value it is about to hand its visitor.
pub(crate) fn set_crossing(file: File) {
    CROSSING.with(|crossing| *crossing.borrow_mut() = Some(file));
}

/// What a serializer that isn't the codec gets to write of a file:
/// nothing.
struct OnlyInAMessage;

impl Serialize for OnlyInAMessage {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(ser::Error::custom(
            "an open file is serialized only into an ipc message",
        ))
    }
}

/// Sends another handle to the file, which the codec moves into the
/// message: the value keeps its own. Only this crate's codec serializes
/// one; any other serializer is refused.
impl Serialize for ReadOnlyFile {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let file = self
            .0
            .try_clone()
            .map_err(|_| ser::Error::custom("an open file that can't be sent"))?;
        set_crossing(file);
        let written = serializer.serialize_newtype_struct(FILE_NAME, &OnlyInAMessage);
        // Not taken: the serializer wasn't the codec.
        drop(take_crossing());
        written
    }
}

/// The file the message's sender put there, which only this crate's
/// codec has to give.
impl<'de> Deserialize<'de> for ReadOnlyFile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Arrived;

        impl<'de> de::Visitor<'de> for Arrived {
            type Value = ReadOnlyFile;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an open file, in an ipc message")
            }

            fn visit_u32<E: de::Error>(self, _index: u32) -> Result<ReadOnlyFile, E> {
                take_crossing()
                    .map(ReadOnlyFile)
                    .ok_or_else(|| E::custom("an open file is read only out of an ipc message"))
            }
        }

        deserializer.deserialize_newtype_struct(FILE_NAME, Arrived)
    }
}
