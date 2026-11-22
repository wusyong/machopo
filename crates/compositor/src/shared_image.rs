//! The compositor's side of shared images (Chromium's
//! `gpu::SharedImageInterfaceProxy` and `ClientSharedImage`): a tile's
//! pixels go to Viz in a message as soon as they are drawn, under a number
//! of the compositor's own, and the frames that show the tile name the
//! number alone.
//!
//! The pixels are copied once on their way to the GPU, by Viz: a tile is
//! drawn straight into a buffer ([`PixelBuffer`]) that the message shares
//! rather than copies, and that is drawn into again once Viz has the
//! pixels on the GPU. Within the browser's process the buffer is one of
//! the pool here, which Viz hands back. From a process of the renderer's
//! own it is a region of shared memory the browser gave its channel,
//! free again when Viz lets go of it; when none is free the tile is drawn
//! in the pool's all the same, and its pixels are copied into the
//! message.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use base::AtomicSequenceNumber;
use bytes::Bytes;
use gfx::Size;
use protocol::viz::{CompositorFrameSinkMsg, Mailbox};
use protocol::{SharedBuffer, SharedBufferWriter};

/// How many buffers the pool keeps for the next tiles: more than a frame
/// rasters at once come back to it, and the rest are let go.
const POOL_BUFFERS: usize = 64;

/// A buffer the compositor's alone: a tile is drawn into it.
pub struct PixelBuffer(Buffer);

enum Buffer {
    /// One of the pool's.
    Pooled(Arc<[u8]>),
    /// A region of the channel to Viz.
    Region(SharedBufferWriter),
}

impl PixelBuffer {
    pub fn pixels(&self) -> &[u8] {
        match &self.0 {
            Buffer::Pooled(buffer) => buffer,
            Buffer::Region(region) => region.bytes(),
        }
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        match &mut self.0 {
            Buffer::Pooled(buffer) => {
                Arc::get_mut(buffer).expect("a buffer taken from the pool is held by no one else")
            }
            Buffer::Region(region) => region.bytes_mut(),
        }
    }
}

/// Where a region of shared memory to draw so many bytes in comes from,
/// if one is free.
type TakeRegion = Box<dyn Fn(usize) -> Option<SharedBufferWriter> + Send + Sync>;

/// The buffer as the bytes of a message: shared, not copied.
struct Lent(Arc<[u8]>);

impl AsRef<[u8]> for Lent {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Default)]
struct Pool {
    /// Buffers no one else holds.
    free: Vec<Arc<[u8]>>,
    /// The buffers Viz has, by the shared image each went with, until it
    /// says it holds one no more.
    lent: HashMap<Mailbox, Arc<[u8]>>,
    /// How many buffers were made, for tests: a pool that works makes few.
    made: usize,
}

/// A compositor's shared images: makes them, and keeps the pool their
/// pixels are drawn into. Shared by the threads that raster.
pub struct SharedImages {
    send: Box<dyn Fn(CompositorFrameSinkMsg) + Send + Sync>,
    /// For a compositor whose messages cross to another process.
    take_region: Option<TakeRegion>,
    mailboxes: AtomicSequenceNumber<u64>,
    pool: Mutex<Pool>,
}

impl SharedImages {
    /// `send` takes each message to Viz, in the order the frames go.
    pub fn new(send: impl Fn(CompositorFrameSinkMsg) + Send + Sync + 'static) -> Arc<Self> {
        Self::make(Box::new(send), None)
    }

    /// As [`new`](Self::new), for a compositor in a process apart from
    /// Viz's: `take_region` gives a region of the channel `send` writes
    /// to, to draw a tile's pixels in, when one is free.
    pub fn with_regions(
        send: impl Fn(CompositorFrameSinkMsg) + Send + Sync + 'static,
        take_region: impl Fn(usize) -> Option<SharedBufferWriter> + Send + Sync + 'static,
    ) -> Arc<Self> {
        Self::make(Box::new(send), Some(Box::new(take_region)))
    }

    fn make(
        send: Box<dyn Fn(CompositorFrameSinkMsg) + Send + Sync>,
        take_region: Option<TakeRegion>,
    ) -> Arc<Self> {
        Arc::new(Self {
            send,
            take_region,
            mailboxes: AtomicSequenceNumber::<u64>::starting_at(1),
            pool: Mutex::new(Pool::default()),
        })
    }

    fn pool(&self) -> std::sync::MutexGuard<'_, Pool> {
        self.pool.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A buffer of `len` bytes to draw a tile into: a region of the
    /// channel's where there is a free one, else one of the pool's of
    /// that length, or a new one. What it holds is whatever was drawn
    /// there before.
    pub fn take_buffer(&self, len: usize) -> PixelBuffer {
        let region = self.take_region.as_ref().and_then(|take| take(len));
        if let Some(region) = region {
            return PixelBuffer(Buffer::Region(region));
        }
        let mut pool = self.pool();
        let found = pool
            .free
            .iter()
            .position(|buffer| buffer.len() == len && Arc::strong_count(buffer) == 1);
        match found {
            Some(index) => PixelBuffer(Buffer::Pooled(pool.free.swap_remove(index))),
            None => {
                pool.made = pool.made.saturating_add(1);
                PixelBuffer(Buffer::Pooled(Arc::from(vec![0; len])))
            }
        }
    }

    /// A buffer not sent after all (its tile came out one color) is the
    /// pool's again, or its channel's.
    pub fn recycle(&self, buffer: PixelBuffer) {
        match buffer.0 {
            Buffer::Pooled(buffer) => self.put(buffer),
            // Dropped, a region is free.
            Buffer::Region(_) => {}
        }
    }

    fn put(&self, buffer: Arc<[u8]>) {
        let mut pool = self.pool();
        if pool.free.len() < POOL_BUFFERS {
            pool.free.push(buffer);
        }
    }

    /// The pixels in `buffer` as a shared image of `size`: told to Viz
    /// now, with the buffer itself. One of the pool's comes back to it
    /// when Viz says so ([`pixels_returned`](Self::pixels_returned)); a
    /// region is its channel's again when Viz lets go of it, with nothing
    /// said here.
    pub fn create(self: &Arc<Self>, size: Size, buffer: PixelBuffer) -> ClientSharedImage {
        let mailbox = Mailbox(self.mailboxes.get_next());
        let pixels = match buffer.0 {
            Buffer::Pooled(buffer) => {
                self.pool().lent.insert(mailbox, buffer.clone());
                Bytes::from_owner(Lent(buffer)).into()
            }
            Buffer::Region(region) => region.into(),
        };
        self.send_create(mailbox, size, pixels)
    }

    /// `pixels`, someone else's to keep (a canvas's snapshot), as a shared
    /// image of `size`: shared with Viz while it puts them on the GPU, not
    /// copied.
    pub fn create_from(self: &Arc<Self>, size: Size, pixels: Bytes) -> ClientSharedImage {
        let mailbox = Mailbox(self.mailboxes.get_next());
        self.send_create(mailbox, size, pixels.into())
    }

    fn send_create(
        self: &Arc<Self>,
        mailbox: Mailbox,
        size: Size,
        pixels: SharedBuffer,
    ) -> ClientSharedImage {
        (self.send)(CompositorFrameSinkMsg::CreateSharedImage {
            mailbox,
            size,
            pixels,
        });
        ClientSharedImage(Arc::new(Owned {
            mailbox,
            images: self.clone(),
        }))
    }

    /// Viz holds the buffer that went with `mailbox` no more
    /// (`CompositorMsg::SharedImagePixelsReturned`): it is the pool's.
    pub fn pixels_returned(&self, mailbox: Mailbox) {
        let returned = self.pool().lent.remove(&mailbox);
        if let Some(buffer) = returned {
            self.put(buffer);
        }
    }

    /// How many buffers the pool has made.
    pub fn buffers_made(&self) -> usize {
        self.pool().made
    }
}

/// A shared image the compositor made, held by what shows it (a tile, a
/// canvas's layer): cloned, it is the same one, and Viz is told it is done
/// with (`DestroySharedImage`) when the last holder lets go.
#[derive(Clone)]
pub struct ClientSharedImage(Arc<Owned>);

struct Owned {
    mailbox: Mailbox,
    images: Arc<SharedImages>,
}

impl ClientSharedImage {
    pub fn mailbox(&self) -> Mailbox {
        self.0.mailbox
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // A buffer Viz never said it gave back isn't kept past its image.
        self.images.pool().lent.remove(&self.mailbox);
        (self.images.send)(CompositorFrameSinkMsg::DestroySharedImage {
            mailbox: self.mailbox,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn images() -> (Arc<SharedImages>, Arc<Mutex<Vec<CompositorFrameSinkMsg>>>) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let to = sent.clone();
        (
            SharedImages::new(move |msg| to.lock().unwrap().push(msg)),
            sent,
        )
    }

    /// A tile drawn into a buffer of the pool goes to Viz as that buffer,
    /// not a copy of it; the buffer is drawn into again once Viz has
    /// handed it back, and not before. The image is destroyed when its
    /// last holder lets go.
    #[test]
    fn a_tiles_pixels_are_the_pools_buffer_and_come_back_to_it() {
        let (images, sent) = images();
        let mut buffer = images.take_buffer(8);
        buffer.pixels_mut().copy_from_slice(&[7; 8]);
        let drawn_at = buffer.pixels().as_ptr();
        let image = images.create(Size::new(2, 1), buffer);
        let held = image.clone();
        let sent_pixels = match sent.lock().unwrap().pop() {
            Some(CompositorFrameSinkMsg::CreateSharedImage {
                mailbox,
                size,
                pixels,
            }) => {
                assert_eq!((mailbox, size), (image.mailbox(), Size::new(2, 1)));
                pixels.as_bytes().expect("the process's own").clone()
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(sent_pixels.as_ptr(), drawn_at, "the buffer itself");
        assert_eq!(&sent_pixels[..], [7; 8]);

        // Viz still has it: the next tile gets another.
        let other = images.take_buffer(8);
        assert_ne!(other.pixels().as_ptr(), drawn_at);
        assert_eq!(images.buffers_made(), 2);
        // Told it came back while Viz still holds it (a Viz that lies):
        // not drawn into all the same, with no other buffer to take.
        images.pixels_returned(image.mailbox());
        let again = images.take_buffer(8);
        assert_ne!(again.pixels().as_ptr(), drawn_at);
        assert_eq!(images.buffers_made(), 3);
        drop(sent_pixels);
        let mut back = images.take_buffer(8);
        assert_eq!(back.pixels().as_ptr(), drawn_at);
        back.pixels_mut()[0] = 1;
        assert_eq!(images.buffers_made(), 3);
        images.recycle(other);
        images.recycle(again);
        // A buffer of another length isn't one of these.
        assert_eq!(images.take_buffer(4).pixels().len(), 4);
        assert_eq!(images.buffers_made(), 4);

        drop(image);
        assert!(sent.lock().unwrap().is_empty(), "a holder is left");
        let mailbox = held.mailbox();
        drop(held);
        assert!(matches!(
            sent.lock().unwrap()[..],
            [CompositorFrameSinkMsg::DestroySharedImage { mailbox: destroyed }] if destroyed == mailbox
        ));
    }

    /// A compositor whose messages cross to another process draws a tile
    /// in a region of its channel, and that region is the message's
    /// pixels: none of the pool's is made or lent for it. With no region
    /// free the tile is drawn in the pool's, as within a process.
    #[test]
    fn a_tile_is_drawn_in_a_region_of_the_channels_when_one_is_free() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let to = sent.clone();
        let free = Arc::new(Mutex::new(vec![ipc::local::writer(8).unwrap()]));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let (regions, asks) = (free.clone(), asked.clone());
        let images = SharedImages::with_regions(
            move |msg| to.lock().unwrap().push(msg),
            move |len| {
                asks.lock().unwrap().push(len);
                regions.lock().unwrap().pop()
            },
        );
        let pixels_of = |sent: &Mutex<Vec<CompositorFrameSinkMsg>>| match sent.lock().unwrap().pop()
        {
            Some(CompositorFrameSinkMsg::CreateSharedImage { pixels, .. }) => pixels,
            other => panic!("{other:?}"),
        };

        let mut buffer = images.take_buffer(8);
        buffer.pixels_mut().copy_from_slice(&[7; 8]);
        assert_eq!(buffer.pixels(), [7; 8]);
        let image = images.create(Size::new(2, 1), buffer);
        let pixels = pixels_of(&sent);
        assert!(pixels.as_bytes().is_none(), "the region, not bytes of ours");
        assert_eq!(pixels.len(), 8);
        assert_eq!(images.buffers_made(), 0);
        // Viz saying it has the pixels puts nothing in the pool: the
        // region comes back by its channel.
        images.pixels_returned(image.mailbox());
        assert!(images.pool().free.is_empty() && images.pool().lent.is_empty());

        // A region taken for a tile that came out one color is let go.
        *free.lock().unwrap() = vec![ipc::local::writer(8).unwrap()];
        let buffer = images.take_buffer(8);
        images.recycle(buffer);
        assert!(images.pool().free.is_empty());
        assert_eq!(images.buffers_made(), 0);

        // None free: the pool's, shared with the message as it is.
        let mut buffer = images.take_buffer(8);
        buffer.pixels_mut().copy_from_slice(&[9; 8]);
        let drawn_at = buffer.pixels().as_ptr();
        let _image = images.create(Size::new(2, 1), buffer);
        let pixels = pixels_of(&sent);
        assert_eq!(pixels.as_bytes().unwrap().as_ptr(), drawn_at);
        assert_eq!(images.buffers_made(), 1);
        assert_eq!(asked.lock().unwrap()[..], [8, 8, 8]);
        // A compositor in Viz's process asks for none.
        let (images, _sent) = super::tests::images();
        drop(images.take_buffer(8));
        assert_eq!(images.buffers_made(), 1);
    }

    /// Pixels kept elsewhere go to Viz shared too, and into no pool.
    #[test]
    fn pixels_of_someone_elses_are_shared_not_pooled() {
        let (images, sent) = images();
        let pixels = Bytes::from_static(&[1, 2, 3, 4]);
        let image = images.create_from(Size::new(1, 1), pixels.clone());
        match &sent.lock().unwrap()[0] {
            CompositorFrameSinkMsg::CreateSharedImage { pixels: sent, .. } => {
                assert_eq!(sent.as_bytes().unwrap().as_ptr(), pixels.as_ptr());
            }
            other => panic!("{other:?}"),
        }
        images.pixels_returned(image.mailbox());
        assert_eq!(images.buffers_made(), 0);
        assert_ne!(images.take_buffer(4).pixels().as_ptr(), pixels.as_ptr());
    }
}
