//! An open file in a message's value (`ipc::file::ReadOnlyFile`): the
//! codec attaches its handle wherever the value nests it and writes which
//! of the message's handles it is; the other end's value has a file of
//! its own, which it maps to read. Only the end that may attach handles
//! sends one, and a message's handles are each a value's once.
//!
//! On Windows and macOS a real child is given files too; on Windows what
//! it maps can't be written by anyone while it keeps the mapping.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ipc::codec::{self, EncodeError, NoRegions, Place, Resolve};
use ipc::core::channel::{self, Gone, MAX_QUEUED_BYTES};
use ipc::core::frame::{BadMessage, MAX_HANDLES};
use ipc::core::region::HeapMemory;
use ipc::endpoint::{self, Config, Endpoint, Event, Handler, Link, Received, SendError};
use ipc::file::ReadOnlyFile;
use ipc::local::{self, Local};
use serde::{Deserialize, Serialize};

/// A file of its own with `bytes` in it, removed when the value goes.
struct Scratch(PathBuf);

impl Scratch {
    fn new(bytes: &[u8]) -> Self {
        static NEXT: base::AtomicSequenceNumber<u32> = base::AtomicSequenceNumber::<u32>::new();
        let path = std::env::temp_dir().join(format!(
            "machopo-ipc-file-{}-{}",
            std::process::id(),
            NEXT.get_next()
        ));
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }

    fn open(&self) -> ReadOnlyFile {
        ReadOnlyFile::open(&self.0).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn bytes_of(file: ReadOnlyFile) -> Vec<u8> {
    file.map().expect("maps").to_vec()
}

/// An end that attaches handles: keeps each file in the order it was
/// given, as the message's handles.
#[derive(Default)]
struct Attaching {
    files: Vec<File>,
    /// Attaches no more than this many.
    limit: Option<usize>,
}

impl Place for Attaching {
    fn place(&mut self, _: &[u8]) -> Option<u32> {
        None
    }

    fn unplace(&mut self, _: u32) {}

    fn file(&mut self, file: File) -> Option<u32> {
        if self.limit.is_some_and(|limit| self.files.len() >= limit) {
            return None;
        }
        self.files.push(file);
        Some(self.files.len() as u32 - 1)
    }
}

/// The other end: the message's handles, each taken once.
struct Arrived(Vec<Option<File>>);

impl Resolve for Arrived {
    fn resolve(&mut self, number: u32, _: u32, _: &mut Vec<u8>) -> Result<(), BadMessage> {
        Err(BadMessage::UnknownRegion { number })
    }

    fn file(&mut self, index: u32) -> Result<File, BadMessage> {
        self.0
            .get_mut(index as usize)
            .and_then(Option::take)
            .ok_or(BadMessage::UnknownHandle { index })
    }
}

/// Files at every kind of place a value has.
#[derive(Serialize, Deserialize)]
struct Carrying {
    name: String,
    file: ReadOnlyFile,
    maybe: Option<ReadOnlyFile>,
    none: Option<ReadOnlyFile>,
    list: Vec<ReadOnlyFile>,
    map: BTreeMap<String, ReadOnlyFile>,
    after: u32,
}

#[test]
fn a_file_goes_as_which_handle_it_is_wherever_it_is_nested() {
    let scratch: Vec<Scratch> = [&b"file"[..], b"maybe", b"first", b"second", b"in a map"]
        .iter()
        .map(|bytes| Scratch::new(bytes))
        .collect();
    let value = Carrying {
        name: "fonts".into(),
        file: scratch[0].open(),
        maybe: Some(scratch[1].open()),
        none: None,
        list: vec![scratch[2].open(), scratch[3].open()],
        map: BTreeMap::from([("k".to_owned(), scratch[4].open())]),
        after: 7,
    };
    let mut attaching = Attaching::default();
    let mut payload = Vec::new();
    codec::encode(&value, &mut attaching, &mut payload).unwrap();
    // A handle each, in the order the value has them; the payload holds
    // indices, not files.
    assert_eq!(attaching.files.len(), 5);
    assert!(payload.len() < 30, "{}", payload.len());
    // The value keeps its own: what was attached is another handle.
    let Carrying { file: kept, .. } = value;
    assert_eq!(bytes_of(kept), b"file");

    let mut arrived = Arrived(attaching.files.into_iter().map(Some).collect());
    let back: Carrying = codec::decode(&payload, &mut arrived).unwrap();
    assert_eq!(back.name, "fonts");
    assert_eq!(back.after, 7);
    assert!(back.none.is_none());
    assert_eq!(bytes_of(back.file), b"file");
    assert_eq!(bytes_of(back.maybe.unwrap()), b"maybe");
    let list: Vec<Vec<u8>> = back.list.into_iter().map(bytes_of).collect();
    assert_eq!(list, [b"first".to_vec(), b"second".to_vec()]);
    assert_eq!(
        bytes_of(back.map.into_values().next().unwrap()),
        b"in a map"
    );
    assert!(
        arrived.0.iter().all(Option::is_none),
        "each handle taken by its value"
    );
}

#[test]
fn only_the_codec_sends_a_file_and_only_to_an_end_that_attaches() {
    let scratch = Scratch::new(b"kept");
    let file = scratch.open();
    assert!(serde_json::to_string(&file).is_err());
    // Read out of anything but a message, a file is nobody's to give:
    // not even the one another serializer was just refused.
    assert!(serde_json::from_str::<ReadOnlyFile>("0").is_err());
    // An end that attaches none: the value doesn't serialize.
    let mut payload = vec![9];
    assert_eq!(
        codec::encode(&file, &mut NoRegions, &mut payload),
        Err(EncodeError)
    );
    assert_eq!(payload, [9], "left as it was");
    // One that attaches only so many.
    let two = vec![scratch.open(), scratch.open()];
    let mut attaching = Attaching {
        limit: Some(1),
        ..Attaching::default()
    };
    assert_eq!(
        codec::encode(&two, &mut attaching, &mut payload),
        Err(EncodeError)
    );
    // Whatever was tried, the value still has its file, and nothing
    // else does: once it goes, the file is open nowhere.
    assert_eq!(bytes_of(file), b"kept");
}

#[cfg(windows)]
#[test]
fn a_file_another_serializer_was_refused_is_left_open_nowhere() {
    let scratch = Scratch::new(b"refused");
    let file = scratch.open();
    assert!(serde_json::to_string(&file).is_err());
    drop(file);
    // Nobody may write it while a handle to it is open anywhere.
    assert!(
        std::fs::OpenOptions::new()
            .write(true)
            .open(&scratch.0)
            .is_ok()
    );
}

#[test]
fn a_handle_that_isnt_the_messages_or_is_named_twice_is_a_bad_message() {
    let scratch = Scratch::new(b"once");
    let mut attaching = Attaching::default();
    let mut payload = Vec::new();
    codec::encode(&scratch.open(), &mut attaching, &mut payload).unwrap();
    // Which handle the payload says, twice: the second is a bad message.
    let mut twice = payload.clone();
    twice.extend_from_slice(&payload);
    let mut arrived = Arrived(attaching.files.into_iter().map(Some).collect());
    assert_eq!(
        codec::decode::<(ReadOnlyFile, ReadOnlyFile)>(&twice, &mut arrived).err(),
        Some(BadMessage::UnknownHandle { index: 0 })
    );
    // None at all.
    assert_eq!(
        codec::decode::<ReadOnlyFile>(&payload, &mut Arrived(Vec::new())).err(),
        Some(BadMessage::UnknownHandle { index: 0 })
    );
    // With no handles to give (an end with no regions either).
    assert_eq!(
        codec::decode::<ReadOnlyFile>(&payload, &mut NoRegions).err(),
        Some(BadMessage::UnknownHandle { index: 0 })
    );
}

#[test]
fn a_file_of_no_bytes_maps_to_no_bytes() {
    let scratch = Scratch::new(b"");
    assert!(scratch.open().map().unwrap().is_empty());
}

#[cfg(windows)]
#[test]
fn nobody_writes_an_open_file_but_it_may_be_renamed() {
    let scratch = Scratch::new(b"as it was");
    let file = scratch.open();
    let write = || std::fs::OpenOptions::new().write(true).open(&scratch.0);
    assert!(write().is_err(), "opened to write while it is open");
    let mapped = file.map().unwrap();
    assert!(write().is_err(), "opened to write while it is mapped");
    // Moved out of the way, as an update puts a new one in its place:
    // what is mapped stays as it was.
    let moved = Scratch(scratch.0.with_extension("old"));
    std::fs::rename(&scratch.0, &moved.0).unwrap();
    std::fs::write(&scratch.0, b"a new one").unwrap();
    assert_eq!(&*mapped, b"as it was");
    drop(mapped);
    assert!(
        std::fs::OpenOptions::new()
            .write(true)
            .open(&moved.0)
            .is_ok(),
        "once nothing has it open"
    );
}

// An endpoint over the local platform, turned by hand.

fn config(host: bool) -> Config {
    Config {
        destinations: 1,
        may_attach: host,
        peer_may_attach: !host,
        peer_may_open_pipes: false,
        region_limit: 1 << 20,
        peer_region_limit: 1 << 20,
        queue_limit: MAX_QUEUED_BYTES,
    }
}

#[derive(Serialize, Deserialize)]
struct Files(Vec<ReadOnlyFile>);

/// Keeps the bytes of each file that came.
#[derive(Default)]
struct Mapping(Vec<Vec<Vec<u8>>>);

impl Handler<Local> for Mapping {
    fn message(&mut self, mut message: Received<'_, Local>) -> Result<(), BadMessage> {
        let Files(files) = message.decode()?;
        self.0.push(files.into_iter().map(bytes_of).collect());
        Ok(())
    }
}

struct End {
    link: Link<Local>,
    endpoint: Endpoint<Local>,
    mapping: Mapping,
    gone: Option<Gone>,
    woken: Arc<AtomicBool>,
}

impl End {
    fn new(platform: Local, config: Config) -> Self {
        let (link, endpoint) = endpoint::endpoint(platform, config);
        let woken = Arc::new(AtomicBool::new(false));
        let flag = woken.clone();
        link.on_wake(Arc::new(move || flag.store(true, Ordering::SeqCst)));
        Self {
            link,
            endpoint,
            mapping: Mapping::default(),
            gone: None,
            woken,
        }
    }

    /// Turns until there is nothing to do.
    fn settle(&mut self) {
        loop {
            let mut bytes = Vec::new();
            let mut handles = Vec::new();
            let event = if self.woken.swap(false, Ordering::SeqCst) {
                Event::Woken
            } else if let Some(event) = self.link.platform().poll(&mut bytes, &mut handles) {
                event
            } else {
                return;
            };
            if self.gone.is_some() {
                return;
            }
            self.gone = self
                .endpoint
                .turn(event, &bytes, &mut handles, &mut self.mapping);
        }
    }
}

#[test]
fn a_host_sends_its_child_files_and_keeps_its_own() {
    let (host_end, child_end) = local::pair(None);
    let mut host = End::new(host_end, config(true));
    let mut child = End::new(child_end, config(false));
    let a = Scratch::new(b"one");
    let b = Scratch::new(b"two");
    let files = Files(vec![a.open(), b.open(), a.open()]);
    host.link.send(0, &files).unwrap();
    child.settle();
    host.settle();
    assert_eq!(child.gone, None);
    assert_eq!(
        child.mapping.0,
        [vec![b"one".to_vec(), b"two".to_vec(), b"one".to_vec()]]
    );
    let Files(kept) = files;
    assert_eq!(
        kept.into_iter().map(bytes_of).collect::<Vec<_>>(),
        [b"one".to_vec(), b"two".to_vec(), b"one".to_vec()]
    );
}

#[test]
fn a_child_sends_no_file() {
    let (host_end, child_end) = local::pair(None);
    let mut host = End::new(host_end, config(true));
    let child = End::new(child_end, config(false));
    let scratch = Scratch::new(b"mine");
    assert_eq!(
        child.link.send(0, &Files(vec![scratch.open()])),
        Err(SendError::Refused)
    );
    host.settle();
    assert_eq!(host.gone, None);
    assert!(host.mapping.0.is_empty());
}

#[test]
fn a_message_takes_no_more_files_than_a_message_has_handles() {
    let (host_end, child_end) = local::pair(None);
    let mut host = End::new(host_end, config(true));
    let mut child = End::new(child_end, config(false));
    let scratch = Scratch::new(b"many");
    let files = |n| Files((0..n).map(|_| scratch.open()).collect());
    let most = MAX_HANDLES as usize;
    assert_eq!(host.link.send(0, &files(most + 1)), Err(SendError::Refused));
    // Nothing went, and the channel goes on.
    host.link.send(0, &files(most)).unwrap();
    child.settle();
    host.settle();
    assert_eq!(child.gone, None);
    assert_eq!(child.mapping.0.len(), 1);
    assert_eq!(child.mapping.0[0].len(), most);
}

#[test]
fn a_handle_that_isnt_a_file_where_a_file_is_is_a_bad_message() {
    // A host that isn't ours: the message says its file is its handle,
    // which is a region.
    let (child_end, host_end) = local::pair(None);
    let (sender, _) = channel::channel(
        host_end,
        channel::Config {
            destinations: 1,
            may_attach: true,
            peer_may_attach: false,
            queue_limit: MAX_QUEUED_BYTES,
        },
    );
    let mut child = End::new(child_end, config(false));
    let mut payload = Vec::new();
    let scratch = Scratch::new(b"a file");
    codec::encode(
        &Files(vec![scratch.open()]),
        &mut Attaching::default(),
        &mut payload,
    )
    .unwrap();
    let region = Arc::new(HeapMemory::new(64 << 10));
    sender.send(0, &payload, vec![region.into()]).unwrap();
    child.settle();
    assert_eq!(child.gone, Some(Gone::BadMessage(BadMessage::Rejected)));
    assert!(child.mapping.0.is_empty());
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod process {
    //! A real child: the files it is sent are its own, even once the host
    //! has let go of its own; on Windows nobody writes one while the
    //! child keeps it mapped.

    use std::sync::mpsc;
    use std::thread;

    use ipc::endpoint::IoLoop;
    use ipc::file::MappedFile;
    use ipc::platform::{self, Channel};

    use super::*;
    use crate::process_channel::{self as pc, Host, Msg, ROLE, SOON, command, text};

    /// The child's side: maps what it is sent and says what it found.
    #[test]
    fn child() {
        if std::env::var(ROLE).as_deref() != Ok("files") {
            return;
        }
        thread::spawn(|| {
            thread::sleep(SOON * 3);
            std::process::exit(9);
        });
        let Some(channel) = platform::inherited() else {
            std::process::exit(pc::SETUP);
        };
        let (link, endpoint) = endpoint::endpoint(channel, pc::config(false));
        let (io, handle) = IoLoop::new().unwrap();
        let (over, was_over) = mpsc::channel();
        handle.serve(
            endpoint,
            Child {
                link,
                kept: Vec::new(),
                over,
                io: handle.clone(),
            },
        );
        io.run();
        let code = match was_over.recv().unwrap() {
            Gone::Closed => pc::CLOSED,
            Gone::PeerClosed => pc::PEER_CLOSED,
            _ => pc::OTHER,
        };
        std::process::exit(code);
    }

    struct Child {
        link: Link<Channel>,
        kept: Vec<MappedFile>,
        over: mpsc::Sender<Gone>,
        io: endpoint::LoopHandle<Channel>,
    }

    #[derive(Serialize, Deserialize)]
    enum ToChild {
        Files(Vec<ReadOnlyFile>),
        Quit,
    }

    impl Handler<Channel> for Child {
        fn message(&mut self, mut message: Received<'_, Channel>) -> Result<(), BadMessage> {
            match message.decode()? {
                ToChild::Files(files) => {
                    let mut said = Vec::new();
                    for file in files {
                        let mapped = file.map().ok_or(BadMessage::Rejected)?;
                        said.push(String::from_utf8_lossy(&mapped).into_owned());
                        self.kept.push(mapped);
                    }
                    let _ = self.link.send(0, &text(&said.join(",")));
                }
                ToChild::Quit => self.link.close(),
            }
            Ok(())
        }

        fn gone(&mut self, gone: Gone) {
            let _ = self.over.send(gone);
            self.io.stop();
        }
    }

    #[test]
    fn a_child_maps_the_files_its_host_sends_as_its_own() {
        let (child, channel) =
            platform::spawn(&mut command("file_in_message::process::child", "files")).unwrap();
        let host = Host::over(channel, child);
        let a = Scratch::new(b"alpha");
        let b = Scratch::new(b"beta");
        host.link
            .send(0, &ToChild::Files(vec![a.open(), b.open()]))
            .unwrap();
        assert_eq!(host.next(), (0, Msg::Text("alpha,beta".into())));
        // The host's own handles are gone (the value was dropped as it
        // was sent): the child's keep writers out.
        #[cfg(windows)]
        let write = |scratch: &Scratch| std::fs::OpenOptions::new().write(true).open(&scratch.0);
        #[cfg(windows)]
        assert!(write(&a).is_err() && write(&b).is_err());
        // One with no name any more: what the child is given is the open
        // file, not its path.
        let c = Scratch::new(b"gamma");
        let open = c.open();
        drop(c);
        host.link.send(0, &ToChild::Files(vec![open])).unwrap();
        assert_eq!(host.next(), (0, Msg::Text("gamma".into())));
        host.link.send(0, &ToChild::Quit).unwrap();
        assert_eq!(host.end(), (Gone::PeerClosed, pc::CLOSED));
        #[cfg(windows)]
        assert!(write(&a).is_ok(), "once the child is gone");
    }
}
