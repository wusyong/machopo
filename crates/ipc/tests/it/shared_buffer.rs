//! `ipc::shared_buffer` over the platform within one process: a region
//! taken, written in place and sent is what the other end reads out of,
//! with no copy of it in the message or on the way; it is written again
//! only once whoever read it let go; and a peer that isn't ours gets
//! nothing by saying otherwise.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::Bytes;
use ipc::core::channel::{self, Gone, MAX_QUEUED_BYTES};
use ipc::core::control::{CONTROL_DESTINATION, Control};
use ipc::core::frame::BadMessage;
use ipc::core::region::HeapMemory;
use ipc::endpoint::{self, Config, Endpoint, Event, Handler, Link, Received, SendError};
use ipc::local::{self, Local};
use ipc::shared_buffer::SharedBuffer;
use serde::{Deserialize, Serialize};

const KIB: usize = 1 << 10;
const TILE: usize = 256 * KIB;

/// A tile's pixels, as a message has them.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Tile {
    name: u32,
    pixels: SharedBuffer,
}

#[derive(Default)]
struct Tiles(Vec<Tile>);

impl Handler<Local> for Tiles {
    fn message(&mut self, mut message: Received<'_, Local>) -> Result<(), BadMessage> {
        self.0.push(message.decode()?);
        Ok(())
    }
}

fn config(host: bool) -> Config {
    Config {
        destinations: 1,
        may_attach: host,
        peer_may_attach: !host,
        peer_may_open_pipes: !host,
        region_limit: 4 * TILE,
        peer_region_limit: 4 * TILE,
        queue_limit: MAX_QUEUED_BYTES,
    }
}

struct End {
    link: Link<Local>,
    endpoint: Endpoint<Local>,
    tiles: Tiles,
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
            tiles: Tiles::default(),
            gone: None,
            woken,
        }
    }

    fn turn(&mut self) -> bool {
        let mut bytes = Vec::new();
        let mut handles = Vec::new();
        let event = match self.woken.swap(false, Ordering::SeqCst) {
            true => Event::Woken,
            false => match self.link.platform().poll(&mut bytes, &mut handles) {
                Some(event) => event,
                None => return false,
            },
        };
        if self.gone.is_some() {
            return false;
        }
        self.gone = self
            .endpoint
            .turn(event, &bytes, &mut handles, &mut self.tiles);
        true
    }

    /// The bytes of regions this end writes into, and how many are free.
    fn regions(&self) -> (usize, usize) {
        self.link.regions()
    }
}

fn settle(host: &mut End, child: &mut End) {
    let mut turns = 0;
    while host.turn() | child.turn() {
        turns += 1;
        assert!(turns < 100_000, "the two ends never settle");
    }
}

/// A host and a child the host gave `tiles` regions of a tile's length
/// to write.
fn ends(tiles: usize) -> (End, End) {
    let (host, child) = local::pair(None);
    let (mut host, mut child) = (End::new(host, config(true)), End::new(child, config(false)));
    for _ in 0..tiles {
        host.link.grant_to_peer(TILE);
    }
    settle(&mut host, &mut child);
    assert_eq!(child.regions(), (tiles * TILE, tiles * TILE));
    (host, child)
}

fn pattern(len: usize, salt: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(salt))
        .collect()
}

/// A tile of `len` bytes drawn in a region of `end`'s.
fn drawn(end: &End, len: usize, salt: u8) -> SharedBuffer {
    let mut writer = end.link.take_region(len).expect("a free region");
    assert_eq!(writer.bytes_mut().len(), len);
    writer.bytes_mut().copy_from_slice(&pattern(len, salt));
    assert_eq!(writer.bytes(), &pattern(len, salt)[..]);
    writer.into()
}

fn read(pixels: &SharedBuffer) -> Vec<u8> {
    let mut read = vec![0; pixels.len()];
    assert!(pixels.copy_to(0, &mut read));
    read
}

#[test]
fn a_region_written_in_place_is_what_the_peer_reads_out_of() {
    let (mut host, mut child) = ends(1);
    let made = host.link.platform().created();
    let before = child.link.platform().written();
    let pixels = drawn(&child, 200 * KIB, 3);
    assert_eq!(pixels.len(), 200 * KIB);
    assert!(pixels.as_bytes().is_none());
    child.link.send(0, &Tile { name: 7, pixels }).unwrap();
    settle(&mut host, &mut child);

    // The pixels weren't in the message, and no region was made for them.
    assert!(child.link.platform().written() - before < KIB);
    assert_eq!(host.link.platform().created(), made);
    let tile = host.tiles.0.pop().unwrap();
    assert_eq!(tile.name, 7);
    assert_eq!(tile.pixels.len(), 200 * KIB);
    // Not memory of the host's own: only copied out of, any part of it.
    assert!(tile.pixels.as_bytes().is_none());
    assert_eq!(read(&tile.pixels), pattern(200 * KIB, 3));
    let mut part = [0; 5];
    assert!(tile.pixels.copy_to(200 * KIB - 5, &mut part));
    assert_eq!(part, pattern(200 * KIB, 3)[200 * KIB - 5..]);
    // Nothing past what the message said is there, though the region is
    // longer.
    assert!(!tile.pixels.copy_to(200 * KIB - 4, &mut part));
    assert!(!tile.pixels.copy_to(usize::MAX, &mut part));
    assert!(tile.pixels.copy_to(200 * KIB, &mut []));
    assert!(!tile.pixels.copy_to(200 * KIB + 1, &mut []));
    assert_eq!(part, pattern(200 * KIB, 3)[200 * KIB - 5..], "untouched");
    // By value too, a run at a time, for where there is no slice to
    // copy into: the same bytes, and no run that isn't whole and inside.
    let runs: Vec<[u8; 1024]> = tile.pixels.chunks(KIB, 199).unwrap().collect();
    assert_eq!(runs.len(), 199);
    assert_eq!(runs.as_flattened(), &pattern(200 * KIB, 3)[KIB..]);
    let mut odd = tile.pixels.chunks::<3>(200 * KIB - 7, 2).unwrap();
    assert_eq!(odd.len(), 2);
    assert_eq!(
        odd.next().unwrap(),
        pattern(200 * KIB, 3)[200 * KIB - 7..][..3]
    );
    assert_eq!(
        odd.next().unwrap(),
        pattern(200 * KIB, 3)[200 * KIB - 4..][..3]
    );
    assert_eq!((odd.len(), odd.next()), (0, None));
    assert!(tile.pixels.chunks::<3>(200 * KIB - 5, 2).is_none());
    assert!(tile.pixels.chunks::<1024>(0, 201).is_none());
    assert!(tile.pixels.chunks::<1024>(usize::MAX, 1).is_none());
    assert!(tile.pixels.chunks::<1024>(0, usize::MAX).is_none());
    assert_eq!(tile.pixels.chunks::<8>(200 * KIB, 0).unwrap().count(), 0);

    // The host still reads it: the child writes it no more, in place or
    // by a copy.
    assert_eq!(child.regions(), (TILE, 0));
    assert!(child.link.take_region(1).is_none());
    let held = tile.pixels.clone();
    drop(tile);
    settle(&mut host, &mut child);
    assert_eq!(child.regions(), (TILE, 0), "a clone is the same buffer");
    drop(held);
    settle(&mut host, &mut child);
    assert_eq!(child.regions(), (TILE, TILE));
    // And the next tile is drawn there.
    let pixels = drawn(&child, TILE, 9);
    child.link.send(0, &Tile { name: 8, pixels }).unwrap();
    settle(&mut host, &mut child);
    assert_eq!(read(&host.tiles.0[0].pixels), pattern(TILE, 9));
    assert_eq!((host.gone, child.gone), (None, None));
}

#[test]
fn a_region_taken_and_not_sent_is_free_again() {
    let (_host, child) = ends(2);
    let first = child.link.take_region(TILE).unwrap();
    let second = child.link.take_region(10).unwrap();
    assert!(child.link.take_region(1).is_none(), "both are out");
    assert_eq!(child.regions(), (2 * TILE, 0));
    drop(first);
    assert_eq!(child.regions(), (2 * TILE, TILE));
    // Made a buffer and dropped: the same.
    drop(SharedBuffer::from(second));
    assert_eq!(child.regions(), (2 * TILE, 2 * TILE));
    // More than any region holds.
    assert!(child.link.take_region(TILE + 1).is_none());
    assert_eq!(child.regions(), (2 * TILE, 2 * TILE));
}

#[test]
fn a_region_crosses_once_and_only_its_own_channel() {
    let (mut host, mut child) = ends(1);
    let (mut other_host, mut other_child) = ends(1);
    let pixels = drawn(&child, TILE, 1);
    let tile = Tile { name: 1, pixels };

    // Not by a serializer that isn't a channel's, nor by another
    // channel: the buffer keeps its region, and the other channel's own
    // are as they were, the one of the same number that a taker there
    // is writing too.
    assert!(postcard::to_allocvec(&tile).is_err());
    let mut others = other_child.link.take_region(TILE).unwrap();
    assert_eq!(other_child.link.send(0, &tile), Err(SendError::Refused));
    assert_eq!(other_child.regions(), (TILE, 0));
    assert_eq!(child.regions(), (TILE, 0));
    settle(&mut other_host, &mut other_child);
    assert!(other_host.tiles.0.is_empty());
    others.bytes_mut().copy_from_slice(&pattern(TILE, 6));
    let pixels = others.into();
    other_child.link.send(0, &Tile { name: 3, pixels }).unwrap();
    settle(&mut other_host, &mut other_child);
    assert_eq!(read(&other_host.tiles.0[0].pixels), pattern(TILE, 6));

    // By its own, once.
    child.link.send(0, &tile).unwrap();
    assert_eq!(child.link.send(0, &tile), Err(SendError::Refused));
    assert_eq!(child.link.send(0, &tile.clone()), Err(SendError::Refused));
    // Gone to its message, the buffer here reads as nothing.
    assert!(!tile.pixels.copy_to(0, &mut [0]));
    assert!(tile.pixels.chunks::<1>(0, 1).is_none());
    settle(&mut host, &mut child);
    assert_eq!(host.tiles.0.len(), 1);
    assert_eq!(read(&host.tiles.0[0].pixels), pattern(TILE, 1));
    // Dropped where it was sent from, the region is still the host's to
    // read: only the host's letting go frees it.
    drop(tile);
    assert_eq!(child.regions(), (TILE, 0));
    // What the host read isn't sent on.
    let back = Tile {
        name: 2,
        pixels: host.tiles.0[0].pixels.clone(),
    };
    assert_eq!(host.link.send(0, &back), Err(SendError::Refused));
    drop(back);
    host.tiles.0.clear();
    settle(&mut host, &mut child);
    assert_eq!(child.regions(), (TILE, TILE));
    // Read and free, the region is another taker's: the buffer that was
    // sent in it doesn't take it to a second message from under them.
    let sent = Tile {
        name: 4,
        pixels: drawn(&child, TILE, 7),
    };
    child.link.send(0, &sent).unwrap();
    settle(&mut host, &mut child);
    host.tiles.0.clear();
    settle(&mut host, &mut child);
    let mut next = child.link.take_region(TILE).unwrap();
    assert_eq!(child.link.send(0, &sent), Err(SendError::Refused));
    settle(&mut host, &mut child);
    assert!(host.tiles.0.is_empty());
    next.bytes_mut().copy_from_slice(&pattern(TILE, 8));
    let pixels = next.into();
    child.link.send(0, &Tile { name: 5, pixels }).unwrap();
    settle(&mut host, &mut child);
    assert_eq!(read(&host.tiles.0[0].pixels), pattern(TILE, 8));
    assert_eq!((host.gone, child.gone), (None, None));
}

#[test]
fn a_message_that_isnt_sent_gives_its_region_back() {
    // Closed at this end, and its IO thread hasn't seen to it yet: the
    // message that names the region doesn't go, and the region is free.
    let (mut host, mut child) = ends(1);
    let pixels = drawn(&child, TILE, 1);
    child.link.close();
    let tile = Tile { name: 1, pixels };
    assert!(matches!(child.link.send(0, &tile), Err(SendError::Gone(_))));
    assert_eq!(child.regions(), (TILE, TILE));
    // The buffer went with the message that wasn't sent: it gives
    // nothing back a second time, from under whoever took the region
    // next.
    let next = child.link.take_region(TILE).unwrap();
    drop(tile);
    assert_eq!(child.regions(), (TILE, 0));
    drop(next);
    settle(&mut host, &mut child);

    // Over, the channel's regions went with it: the message is refused,
    // there is nothing to take, and nothing goes wrong giving back.
    let (mut host, mut child) = ends(1);
    let pixels = drawn(&child, TILE, 1);
    host.link.close();
    settle(&mut host, &mut child);
    assert!(child.gone.is_some());
    assert!(child.link.send(0, &Tile { name: 1, pixels }).is_err());
    assert!(child.link.take_region(1).is_none());
    assert_eq!(child.regions(), (0, 0));
}

#[test]
fn bytes_of_the_senders_own_are_read_out_of_the_region_they_were_copied_to() {
    let (mut host, mut child) = ends(1);
    // Large enough for a region: copied into the child's once, and read
    // by the host where they are.
    let large = Bytes::from(pattern(100 * KIB, 5));
    let before = child.link.platform().written();
    let sent = Tile {
        name: 1,
        pixels: large.clone().into(),
    };
    assert_eq!(sent.pixels.as_bytes(), Some(&large));
    assert_eq!(read(&sent.pixels), &large[..]);
    let runs: Vec<[u8; 100]> = sent.pixels.chunks(24, 1023).unwrap().collect();
    assert_eq!(runs.as_flattened(), &large[24..][..102_300]);
    assert!(sent.pixels.chunks::<100>(101, 1023).is_none());
    child.link.send(0, &sent).unwrap();
    // The process's own bytes are sent as often as asked.
    assert_eq!(child.regions(), (TILE, 0));
    child.link.send(0, &sent).unwrap();
    settle(&mut host, &mut child);
    assert!(child.link.platform().written() - before > 100 * KIB);
    let [first, second] = &host.tiles.0[..] else {
        panic!("{:?}", host.tiles.0);
    };
    assert!(first.pixels.as_bytes().is_none(), "the region");
    assert_eq!(read(&first.pixels), &large[..]);
    assert_eq!(second.pixels.as_bytes(), Some(&large), "in the message");
    // (For which the child was given a region more, of the length for
    // that many bytes.)
    let more = TILE + 128 * KIB;
    assert_eq!(child.regions(), (more, 128 * KIB));
    host.tiles.0.clear();
    settle(&mut host, &mut child);
    assert_eq!(child.regions(), (more, more));

    // Short ones stay in the message, and the host's own go the same way
    // by the regions it makes.
    let small = Tile {
        name: 2,
        pixels: Bytes::from_static(b"four").into(),
    };
    child.link.send(0, &small).unwrap();
    host.link.send(0, &sent).unwrap();
    settle(&mut host, &mut child);
    assert_eq!(
        host.tiles.0[0].pixels.as_bytes().map(|b| &b[..]),
        Some(&b"four"[..])
    );
    assert!(child.tiles.0[0].pixels.as_bytes().is_none());
    assert_eq!(read(&child.tiles.0[0].pixels), &large[..]);
    let (total, free) = host.regions();
    assert!(total > 0 && free == 0);
    child.tiles.0.clear();
    settle(&mut host, &mut child);
    assert_eq!(host.regions(), (total, total));

    // Without a channel a buffer of the process's own is bytes like any
    // other.
    let bytes = postcard::to_allocvec(&small).unwrap();
    let back: Tile = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(back.name, 2);
    assert_eq!(back.pixels.as_bytes().map(|b| &b[..]), Some(&b"four"[..]));
    assert!(SharedBuffer::default().is_empty());
}

#[test]
fn a_buffer_outlives_its_channel() {
    let (mut host, mut child) = ends(2);
    let pixels = drawn(&child, TILE, 4);
    child.link.send(0, &Tile { name: 1, pixels }).unwrap();
    let mut writer = child.link.take_region(TILE).unwrap();
    settle(&mut host, &mut child);
    let tile = host.tiles.0.pop().unwrap();
    child.link.close();
    settle(&mut host, &mut child);
    assert!(host.gone.is_some() && child.gone.is_some());
    let End { link, endpoint, .. } = host;
    drop((link, endpoint, child.endpoint));
    // The mappings are theirs who hold them.
    assert_eq!(read(&tile.pixels), pattern(TILE, 4));
    writer.bytes_mut().fill(6);
    assert_eq!(writer.bytes()[TILE - 1], 6);
    drop(tile);
    drop(writer);
}

#[test]
fn a_child_with_no_region_free_asks_and_gets_no_more_than_it_may_have() {
    let (mut host, mut child) = ends(0);
    let send = |host: &mut End, child: &mut End, len: usize| {
        let pixels = Bytes::from(pattern(len, 1)).into();
        child.link.send(0, &Tile { name: 0, pixels }).unwrap();
        settle(host, child);
        let tile = host.tiles.0.pop().unwrap();
        assert_eq!(read(&tile.pixels), pattern(len, 1));
        let in_a_region = tile.pixels.as_bytes().is_none();
        // Read: the region is the child's to write again.
        drop(tile);
        settle(host, child);
        in_a_region
    };
    // No region: the bytes go in the message, and the host, told, makes
    // one for the next that long. Nothing is taken until it has.
    assert!(child.link.take_region(TILE).is_none());
    assert_eq!(child.regions(), (0, 0));
    assert!(!send(&mut host, &mut child, 200 * KIB));
    assert_eq!(child.regions(), (TILE, TILE));
    assert!(send(&mut host, &mut child, 200 * KIB));
    assert_eq!(
        child.regions(),
        (TILE, TILE),
        "one was free: none asked for"
    );
    assert!(child.link.take_region(TILE).is_some());
    // What is asked for is made at the length regions are: 3 of a tile's
    // bytes are 4 tiles'. That is past what the host makes for it (4 in
    // all), so none is made; 2 more tiles' are not.
    assert!(!send(&mut host, &mut child, 3 * TILE));
    assert_eq!(child.regions(), (TILE, TILE));
    assert!(!send(&mut host, &mut child, TILE + 1));
    assert_eq!(child.regions(), (3 * TILE, 3 * TILE));
    // Bytes too short for a region ask for none.
    assert!(!send(&mut host, &mut child, 64 * KIB - 1));
    assert_eq!(child.regions(), (3 * TILE, 3 * TILE));
    // What the host gives of itself counts too: of these two, and of one
    // asked for after, the first alone is under what it makes.
    let held: Vec<_> = (0..2).map(|_| child.link.take_region(1).unwrap()).collect();
    host.link.grant_to_peer(TILE);
    host.link.grant_to_peer(64 * KIB);
    settle(&mut host, &mut child);
    assert_eq!(child.regions(), (4 * TILE, TILE));
    let last = child.link.take_region(1).unwrap();
    assert!(!send(&mut host, &mut child, 64 * KIB));
    assert_eq!(child.regions(), (4 * TILE, 0));
    drop((held, last));
    assert_eq!((host.gone, child.gone), (None, None));

    // The end that makes the regions has nobody to ask, and makes one
    // itself: a child that is asked has a host that isn't ours.
    let (host, mut child) = rogue_host();
    control(&host, &Control::Wanted { len: TILE as u32 }, vec![]);
    while child.turn() {}
    assert_eq!(child.gone, Some(Gone::BadMessage(BadMessage::Rejected)));
}

#[test]
fn a_buffer_with_no_channel_reads_as_one_that_crossed() {
    let bytes = pattern(100, 8);
    let buffer = local::shared_buffer(&bytes).unwrap();
    assert!(buffer.as_bytes().is_none());
    assert_eq!(read(&buffer), bytes);
    assert!(!buffer.copy_to(1, &mut [0; 100]), "100 bytes of the region");
    assert!(local::shared_buffer(&[]).unwrap().is_empty());
    let mut writer = local::writer(70 * KIB).unwrap();
    writer.bytes_mut().fill(3);
    assert_eq!(writer.bytes(), &[3; 70 * KIB][..]);
    let buffer = SharedBuffer::from(writer);
    assert_eq!(buffer.len(), 70 * KIB);
    assert!(buffer.as_bytes().is_none());
    assert!(local::writer((1 << 30) + 1).is_none());
}

/// A host's end that isn't ours, for a child that is.
fn rogue_host() -> (channel::Sender<Local>, End) {
    let (host, child) = local::pair(None);
    let (sender, _) = channel::channel(
        host,
        channel::Config {
            destinations: 1,
            may_attach: true,
            peer_may_attach: false,
            queue_limit: MAX_QUEUED_BYTES,
        },
    );
    (sender, End::new(child, config(false)))
}

fn control(sender: &channel::Sender<Local>, control: &Control<'_>, handles: Vec<local::Region>) {
    let mut payload = Vec::new();
    control.encode(&mut payload);
    sender.send(CONTROL_DESTINATION, &payload, handles).unwrap();
}

fn granted(sender: &channel::Sender<Local>, number: u32) {
    let granted = Control::Granted {
        number,
        len: TILE as u32,
        writable: true,
    };
    control(sender, &granted, vec![Arc::new(HeapMemory::new(TILE))]);
}

#[test]
fn a_region_its_taker_still_writes_isnt_the_peers_to_free() {
    // Said read before any message named it: freed, the next taker would
    // write the memory the first still does.
    let (host, mut child) = rogue_host();
    granted(&host, 5);
    while child.turn() {}
    let mut writer = child.link.take_region(TILE).unwrap();
    control(&host, &Control::Released(vec![5]), vec![]);
    while child.turn() {}
    assert_eq!(
        child.gone,
        Some(Gone::BadMessage(BadMessage::RegionNotOut { number: 5 }))
    );
    writer.bytes_mut()[0] = 1;

    // Once a message named it, it is: a host that says so early reads
    // what it reads.
    let (host, mut child) = rogue_host();
    granted(&host, 5);
    while child.turn() {}
    let pixels = drawn(&child, TILE, 2);
    child.link.send(0, &Tile { name: 1, pixels }).unwrap();
    control(&host, &Control::Released(vec![5]), vec![]);
    while child.turn() {}
    assert_eq!(child.gone, None);
    assert_eq!(child.regions(), (TILE, TILE));
    // Twice is once too many, as for any region.
    control(&host, &Control::Released(vec![5]), vec![]);
    while child.turn() {}
    assert_eq!(
        child.gone,
        Some(Gone::BadMessage(BadMessage::RegionNotOut { number: 5 }))
    );
}

#[test]
fn a_message_names_only_regions_its_sender_was_given() {
    // A child that isn't ours, to a host that is.
    let says = |payload: Vec<u8>| {
        let (host, child) = local::pair(None);
        let (sender, _) = channel::channel(
            child,
            channel::Config {
                destinations: 1,
                may_attach: false,
                peer_may_attach: true,
                queue_limit: MAX_QUEUED_BYTES,
            },
        );
        let mut host = End::new(host, config(true));
        host.link.grant_to_peer(TILE);
        while host.turn() {}
        sender.send(0, &payload, Vec::new()).unwrap();
        while host.turn() {}
        (host.gone, host.tiles.0.len())
    };
    // A `Tile`: its name, then the bytes as the region's number and a
    // length (the host's first region is number 0).
    let tile = |number: u8, len: u32| {
        let mut payload = vec![1, 1, number];
        payload.extend(postcard::to_allocvec(&len).unwrap());
        payload
    };
    assert_eq!(says(tile(0, TILE as u32)), (None, 1));
    assert_eq!(says(tile(0, 0)), (None, 1));
    assert_eq!(
        says(tile(0, TILE as u32 + 1)),
        (
            Some(Gone::BadMessage(BadMessage::OutsideRegion {
                number: 0,
                len: TILE as u32 + 1
            })),
            0
        )
    );
    assert_eq!(
        says(tile(1, 4)),
        (
            Some(Gone::BadMessage(BadMessage::UnknownRegion { number: 1 })),
            0
        )
    );
}
