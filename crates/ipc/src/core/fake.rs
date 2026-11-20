//! A channel's transport within one process, for the core's tests: what
//! one end writes the other reads, in pieces cut wherever a seed says, as
//! a socket may cut them. A piece's handles come with its first byte.

use std::sync::mpsc;

pub(crate) struct FakeSender<H> {
    to_peer: mpsc::Sender<(Vec<u8>, Vec<H>)>,
    /// Decides the next piece's length (an xorshift's state).
    seed: u64,
    /// The longest piece.
    longest: usize,
}

pub(crate) struct FakeReceiver<H> {
    from_peer: mpsc::Receiver<(Vec<u8>, Vec<H>)>,
}

/// One direction of a channel: `longest` is the longest piece the bytes
/// are read in.
pub(crate) fn one_way<H>(seed: u64, longest: usize) -> (FakeSender<H>, FakeReceiver<H>) {
    let (to_peer, from_peer) = mpsc::channel();
    (
        FakeSender {
            to_peer,
            seed: seed | 1,
            longest: longest.max(1),
        },
        FakeReceiver { from_peer },
    )
}

impl<H> FakeSender<H> {
    fn next_len(&mut self) -> usize {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed as usize % self.longest) + 1
    }

    /// Writes `bytes`, and `handles` with the first of them.
    pub(crate) fn write(&mut self, mut bytes: &[u8], handles: Vec<H>) {
        let mut handles = Some(handles);
        while !bytes.is_empty() {
            let (piece, rest) = bytes.split_at(self.next_len().min(bytes.len()));
            let _ = self
                .to_peer
                .send((piece.to_vec(), handles.take().unwrap_or_default()));
            bytes = rest;
        }
    }
}

impl<H> FakeReceiver<H> {
    /// The next piece; `None` once the sender is gone and all is read.
    pub(crate) fn read(&mut self) -> Option<(Vec<u8>, Vec<H>)> {
        self.from_peer.recv().ok()
    }
}
