//! The thread canvases' 2D contexts are drawn on, off the main thread
//! (Chromium sends an accelerated canvas's paint ops to the GPU process,
//! whose `RasterDecoder` plays them into the canvas's shared image). The
//! main thread sends what a context recorded and asks for snapshots; it
//! waits only when script reads the pixels, or when the thread falls too
//! far behind. The thread waits for nobody: what it is sent only refers to
//! snapshots it was sent the making of before.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread::JoinHandle;

use base::SequenceNumber;
use canvas::CanvasResource;
use compositor_paint::{DisplayItemList, PaintOp};
use gfx::{PixelsFiller, Size};
use raster::RasterCache;

/// How many recorded ops may be sent and not drawn yet before the main
/// thread waits for the thread to catch up, so a script drawing without
/// end doesn't queue without end. Script's own batches only; a frame's go
/// by `MAX_FRAMES_BEHIND`. (Chromium has no such limit: the command
/// buffer to the GPU process is what fills up.)
pub(crate) const MAX_QUEUED_OPS: usize = 100_000;

/// How many frames' canvases may be sent and not drawn yet before the main
/// thread waits for the oldest (Blink's `kMaxCanvasAnimationBacklog`, and
/// its `SharedContextRateLimiter`).
pub(crate) const MAX_FRAMES_BEHIND: usize = 2;

/// One thing for the thread to do, on the bitmap it knows by `id`.
pub(crate) enum Item {
    /// A bitmap of `size`: `pixels` if it had some on the main thread,
    /// else each pixel `background` (premultiplied). One there isn't the
    /// memory for is never made: what is sent to it is dropped.
    Create {
        id: u64,
        size: Size,
        opaque: bool,
        background: [u8; 4],
        pixels: Option<Vec<u8>>,
    },
    /// What a context recorded, drawn onto the bitmap.
    Draw { id: u64, ops: Vec<PaintOp> },
    /// The bitmap's pixels as they are now, shared until it is drawn on
    /// again: a snapshot's filled in with them.
    Snapshot { id: u64, filler: PixelsFiller },
    /// The bitmap let go of: its context is gone, or reset.
    Release { id: u64 },
    /// Waits until `resume` says to go on, panicking if it never will: what
    /// lets a test hold the thread.
    #[cfg(test)]
    Pause(Receiver<()>),
}

/// What is sent at once: what a flush or a frame needs.
struct Batch {
    items: Vec<Item>,
    /// Its `Draw`s' ops.
    ops: usize,
    /// Whether it is a frame's.
    frame: bool,
}

/// What was sent and not drawn yet, which the main thread waits on.
#[derive(Default)]
struct Backlog {
    state: Mutex<BacklogState>,
    changed: Condvar,
}

#[derive(Default)]
struct BacklogState {
    /// Script's batches' ops.
    ops: usize,
    frames: usize,
    /// The thread is gone (or going): nobody waits for it.
    closed: bool,
}

impl BacklogState {
    /// Whether a batch of `ops` (a frame's if `frame`) would go past the
    /// limits. A batch past `MAX_QUEUED_OPS` on its own isn't, once nothing
    /// else is queued.
    fn full(&self, ops: usize, frame: bool) -> bool {
        if frame {
            self.frames >= MAX_FRAMES_BEHIND
        } else {
            self.ops > 0 && self.ops.saturating_add(ops) > MAX_QUEUED_OPS
        }
    }
}

impl Backlog {
    fn state(&self) -> MutexGuard<'_, BacklogState> {
        // Counts, each updated in one step: a panic leaves them whole.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Waits until a batch of `ops` (a frame's if `frame`) can be sent
    /// without going past the limits (`full`), and counts it in.
    fn add(&self, ops: usize, frame: bool) {
        let mut state = self.state();
        while !state.closed && state.full(ops, frame) {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        if frame {
            state.frames = state.frames.saturating_add(1);
        } else {
            state.ops = state.ops.saturating_add(ops);
        }
    }

    /// Counts a batch out, drawn.
    fn done(&self, ops: usize, frame: bool) {
        let mut state = self.state();
        if frame {
            state.frames = state.frames.saturating_sub(1);
        } else {
            state.ops = state.ops.saturating_sub(ops);
        }
        self.changed.notify_all();
    }

    fn close(&self) {
        self.state().closed = true;
        self.changed.notify_all();
    }

    fn is_closed(&self) -> bool {
        self.state().closed
    }
}

/// Closes the backlog when the thread ends, a panic included, so the main
/// thread never waits for it after.
struct CloseOnExit(Arc<Backlog>);

impl Drop for CloseOnExit {
    fn drop(&mut self) {
        self.0.close();
    }
}

struct RasterThread {
    sender: Option<Sender<Batch>>,
    backlog: Arc<Backlog>,
    handle: Option<JoinHandle<()>>,
}

/// The main thread's side (in Chromium, the canvas's
/// `CanvasResourceProvider` and its `RasterInterface`): the thread, made
/// the first time a canvas is moved there, and the bitmaps it has.
pub(crate) struct CanvasRaster {
    /// Typefaces and decoded images: the thread draws with them, the main
    /// thread too (a canvas that stays there, `measureText`'s ink).
    cache: Arc<RasterCache>,
    thread: Option<RasterThread>,
    ids: SequenceNumber<u64>,
    /// The bitmaps the thread has, each with what its context holds:
    /// gone, the bitmap is let go of at the next frame.
    resources: Vec<(Weak<CanvasResource>, u64)>,
    /// How many batches were sent (or dropped, with no thread), for tests.
    batches_sent: usize,
}

impl Default for CanvasRaster {
    fn default() -> Self {
        Self {
            cache: Arc::default(),
            thread: None,
            ids: SequenceNumber::starting_at(1),
            resources: Vec::new(),
            batches_sent: 0,
        }
    }
}

impl std::fmt::Debug for CanvasRaster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CanvasRaster")
            .field("started", &self.thread.is_some())
            .field("resources", &self.resources.len())
            .finish()
    }
}

impl CanvasRaster {
    pub(crate) fn cache(&self) -> &RasterCache {
        &self.cache
    }

    /// A new bitmap for the thread, and what the context holds it by; the
    /// `Create` for it is the caller's to send.
    pub(crate) fn new_resource(&mut self) -> Arc<CanvasResource> {
        let id = self.ids.get_next();
        let resource = Arc::new(CanvasResource::new(id));
        self.resources.push((Arc::downgrade(&resource), id));
        resource
    }

    /// `Release`s for the bitmaps whose contexts let go of them.
    pub(crate) fn releases(&mut self) -> Vec<Item> {
        let mut released = Vec::new();
        self.resources.retain(|(resource, id)| {
            let held = resource.strong_count() > 0;
            if !held {
                released.push(Item::Release { id: *id });
            }
            held
        });
        released
    }

    /// Sends `items` (a frame's if `frame`), once the thread is far enough
    /// along. Nothing for none. If the thread is gone, they are dropped:
    /// the snapshots in them say their pixels never come.
    pub(crate) fn send(&mut self, items: Vec<Item>, frame: bool) {
        if items.is_empty() {
            return;
        }
        let ops = items
            .iter()
            .map(|item| match item {
                Item::Draw { ops, .. } => ops.len(),
                _ => 0,
            })
            .fold(0, usize::saturating_add);
        self.batches_sent = self.batches_sent.wrapping_add(1);
        let Some(thread) = self.thread() else {
            return;
        };
        let _span = trace_event::trace_event!("CanvasRaster::send", ops, frame);
        thread.backlog.add(ops, frame);
        let batch = Batch { items, ops, frame };
        let sent = thread
            .sender
            .as_ref()
            .is_some_and(|sender| sender.send(batch).is_ok());
        if !sent {
            thread.backlog.done(ops, frame);
        }
    }

    /// The thread, started the first time. `None` if it can't be.
    fn thread(&mut self) -> Option<&RasterThread> {
        if self.thread.is_none() {
            let (sender, receiver) = mpsc::channel();
            let backlog = Arc::new(Backlog::default());
            let handle = std::thread::Builder::new()
                .name("CanvasRaster".into())
                .spawn({
                    let backlog = backlog.clone();
                    let cache = self.cache.clone();
                    move || run(&receiver, &backlog, &cache)
                });
            let handle = match handle {
                Ok(handle) => handle,
                Err(error) => {
                    tracing::error!(%error, "can't start the canvas raster thread");
                    return None;
                }
            };
            self.thread = Some(RasterThread {
                sender: Some(sender),
                backlog,
                handle: Some(handle),
            });
        }
        self.thread.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn batches_sent_for_testing(&self) -> usize {
        self.batches_sent
    }

    /// Script's ops and frames queued, not drawn yet. For tests.
    #[cfg(test)]
    pub(crate) fn backlog_for_testing(&self) -> (usize, usize) {
        let state = self.thread.as_ref().expect("started").backlog.state();
        (state.ops, state.frames)
    }

    /// Waits until everything sent so far is drawn. For tests.
    #[cfg(test)]
    pub(crate) fn wait_for_idle_for_testing(&mut self) {
        let Some(thread) = &self.thread else {
            return;
        };
        let mut state = thread.backlog.state();
        while !state.closed && (state.ops > 0 || state.frames > 0) {
            state = thread
                .backlog
                .changed
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

impl Drop for CanvasRaster {
    fn drop(&mut self) {
        let Some(mut thread) = self.thread.take() else {
            return;
        };
        // What is still queued isn't drawn: nobody will see it.
        thread.backlog.close();
        drop(thread.sender.take());
        if let Some(handle) = thread.handle.take() {
            let _ = handle.join();
        }
    }
}

/// A bitmap on the thread: its pixels shared with the snapshots made of
/// them since it was last drawn on.
struct Bitmap {
    size: Size,
    opaque: bool,
    pixels: Arc<Vec<u8>>,
}

fn run(receiver: &Receiver<Batch>, backlog: &Arc<Backlog>, cache: &RasterCache) {
    let _close = CloseOnExit(backlog.clone());
    let mut bitmaps: HashMap<u64, Bitmap> = HashMap::new();
    while let Ok(batch) = receiver.recv() {
        if backlog.is_closed() {
            return;
        }
        let _span = trace_event::trace_event!("CanvasRaster::batch", ops = batch.ops);
        for item in batch.items {
            run_item(item, &mut bitmaps, cache);
        }
        backlog.done(batch.ops, batch.frame);
    }
}

fn run_item(item: Item, bitmaps: &mut HashMap<u64, Bitmap>, cache: &RasterCache) {
    match item {
        Item::Create {
            id,
            size,
            opaque,
            background,
            pixels,
        } => {
            let pixels = pixels.or_else(|| filled(size, background));
            match pixels {
                Some(pixels) if Some(pixels.len()) == size.checked_byte_size() => {
                    bitmaps.insert(
                        id,
                        Bitmap {
                            size,
                            opaque,
                            pixels: Arc::new(pixels),
                        },
                    );
                }
                _ => tracing::warn!(?size, "no memory for a canvas's bitmap"),
            }
        }
        Item::Draw { id, ops } => {
            let Some(bitmap) = bitmaps.get_mut(&id) else {
                return;
            };
            // Shared with a snapshot: drawn on a copy (Blink's
            // copy-on-write of a canvas's resource a snapshot holds).
            if Arc::get_mut(&mut bitmap.pixels).is_none() {
                let mut copy = Vec::new();
                if copy.try_reserve_exact(bitmap.pixels.len()).is_err() {
                    tracing::warn!(size = ?bitmap.size, "no memory to draw on a canvas");
                    return;
                }
                copy.extend_from_slice(&bitmap.pixels);
                bitmap.pixels = Arc::new(copy);
            }
            let pixels = Arc::get_mut(&mut bitmap.pixels).expect("copied if shared");
            let mut list = DisplayItemList::record();
            for op in ops {
                list.push(op, None);
            }
            list.finalize();
            raster::draw_into(&list, cache, bitmap.size, bitmap.opaque, pixels);
        }
        Item::Snapshot { id, filler } => {
            // No bitmap: the filler dropped says the pixels never come.
            if let Some(bitmap) = bitmaps.get(&id) {
                filler.fill(bitmap.pixels.clone());
            }
        }
        Item::Release { id } => {
            bitmaps.remove(&id);
        }
        #[cfg(test)]
        Item::Pause(resume) => resume.recv().expect("a paused thread is resumed"),
    }
}

/// `size`'s pixels, each `background`; `None` without the memory.
fn filled(size: Size, background: [u8; 4]) -> Option<Vec<u8>> {
    let len = size.checked_byte_size()?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(len).ok()?;
    if background == [0; 4] {
        pixels.resize(len, 0);
    } else {
        pixels.extend(std::iter::repeat_n(background, len / 4).flatten());
    }
    Some(pixels)
}

#[cfg(test)]
pub(crate) mod tests {
    use compositor_paint::{FilterQuality, PaintFlags};
    use gfx::{Color, ImageData, RectF, Transform};

    use super::*;

    const SIZE: Size = Size::new(2, 1);

    /// The left pixel filled with `color`.
    fn fill(color: Color) -> Vec<PaintOp> {
        vec![
            PaintOp::SetMatrix {
                transform: Transform::IDENTITY,
            },
            PaintOp::DrawRect {
                rect: RectF::new(0.0, 0.0, 1.0, 1.0),
                flags: PaintFlags::fill(color),
            },
        ]
    }

    /// A transparent 2×1 bitmap on the thread.
    fn bitmap(raster: &mut CanvasRaster) -> (Arc<CanvasResource>, u64) {
        let resource = raster.new_resource();
        let id = resource.id();
        raster.send(
            vec![Item::Create {
                id,
                size: SIZE,
                opaque: false,
                background: [0; 4],
                pixels: None,
            }],
            false,
        );
        (resource, id)
    }

    fn snapshot(raster: &mut CanvasRaster, id: u64) -> ImageData {
        let (image, filler) = ImageData::pending(SIZE);
        raster.send(vec![Item::Snapshot { id, filler }], false);
        image
    }

    fn read(image: &ImageData) -> Vec<u8> {
        assert!(image.wait_ready(), "the pixels of {image:?} came");
        image.as_bytes().to_vec()
    }

    #[test]
    fn the_main_thread_waits_past_either_limit() {
        let mut state = BacklogState::default();
        // A batch past the limit on its own goes when nothing is queued.
        assert!(!state.full(MAX_QUEUED_OPS + 1, false));
        state.ops = 1;
        assert!(!state.full(MAX_QUEUED_OPS - 1, false));
        assert!(state.full(MAX_QUEUED_OPS, false));
        // Frames go by how many are behind, not by the ops queued.
        assert!(!state.full(0, true));
        state.frames = MAX_FRAMES_BEHIND - 1;
        assert!(!state.full(0, true));
        state.frames = MAX_FRAMES_BEHIND;
        assert!(state.full(0, true));
        state.frames = 0;
        state.ops = MAX_QUEUED_OPS;
        assert!(!state.full(MAX_QUEUED_OPS, true));
    }

    /// Nobody waits for a thread that is gone.
    #[test]
    fn a_closed_backlog_takes_any_batch() {
        let backlog = Backlog::default();
        backlog.add(MAX_QUEUED_OPS, false);
        backlog.close();
        backlog.add(MAX_QUEUED_OPS, false);
        for _ in 0..=MAX_FRAMES_BEHIND {
            backlog.add(0, true);
        }
        assert!(backlog.is_closed());
    }

    /// What is sent is drawn in order, on the thread, pixels written
    /// between the draws too; a snapshot keeps the pixels it was made of,
    /// the bitmap drawn on a copy after.
    #[test]
    fn a_bitmap_is_drawn_in_order_and_its_snapshots_copied_on_write() {
        let draws = raster::draws_into_for_testing();
        let mut raster = CanvasRaster::default();
        let (_resource, id) = bitmap(&mut raster);
        let empty = snapshot(&mut raster, id);
        raster.send(
            vec![Item::Draw {
                id,
                ops: fill(Color::rgb(255, 0, 0)),
            }],
            false,
        );
        let red = snapshot(&mut raster, id);
        let mut ops = vec![PaintOp::WritePixels {
            x: 1,
            y: 0,
            size: Size::new(1, 1),
            pixels: Arc::from([0, 0, 255, 255]),
        }];
        ops.extend(fill(Color::rgb(0, 255, 0)));
        raster.send(vec![Item::Draw { id, ops }], false);
        let green = snapshot(&mut raster, id);
        assert_eq!(read(&empty), [0; 8]);
        assert_eq!(read(&red), [255, 0, 0, 255, 0, 0, 0, 0]);
        assert_eq!(read(&green), [0, 255, 0, 255, 0, 0, 255, 255]);
        assert_eq!(raster::draws_into_for_testing(), draws, "not drawn here");
        assert_eq!(raster.batches_sent_for_testing(), 6);
    }

    /// A bitmap draws a snapshot of another, and of itself, as they were
    /// when it was made: what is sent only refers to what was sent before.
    #[test]
    fn a_bitmap_draws_snapshots_as_they_were_made() {
        let mut raster = CanvasRaster::default();
        let (_a, a) = bitmap(&mut raster);
        let (_b, b) = bitmap(&mut raster);
        raster.send(
            vec![Item::Draw {
                id: a,
                ops: fill(Color::rgb(255, 0, 0)),
            }],
            false,
        );
        let red = snapshot(&mut raster, a);
        let draw = |image: &ImageData, x| PaintOp::DrawImage {
            image: image.clone(),
            src: None,
            dst: RectF::new(x, 0.0, 2.0, 1.0),
            filter: FilterQuality::None,
            flags: None,
        };
        raster.send(
            vec![
                Item::Draw {
                    id: b,
                    ops: vec![draw(&red, 0.0)],
                },
                // Itself, moved right, over what it had cleared.
                Item::Draw {
                    id: a,
                    ops: vec![
                        PaintOp::ClearRect {
                            rect: RectF::new(0.0, 0.0, 2.0, 1.0),
                        },
                        draw(&red, 1.0),
                    ],
                },
            ],
            false,
        );
        let (b, a) = (snapshot(&mut raster, b), snapshot(&mut raster, a));
        assert_eq!(read(&b), [255, 0, 0, 255, 0, 0, 0, 0]);
        assert_eq!(read(&a), [0, 0, 0, 0, 255, 0, 0, 255]);
        assert_eq!(read(&red), [255, 0, 0, 255, 0, 0, 0, 0]);
    }

    /// A bitmap starts as the pixels it had on the main thread, or all its
    /// background.
    #[test]
    fn a_bitmap_starts_with_its_pixels_or_background() {
        let mut raster = CanvasRaster::default();
        let [moved, black] = [Some(vec![1, 2, 3, 4, 5, 6, 7, 8]), None].map(|pixels| {
            let resource = raster.new_resource();
            raster.send(
                vec![Item::Create {
                    id: resource.id(),
                    size: SIZE,
                    opaque: true,
                    background: [0, 0, 0, 255],
                    pixels,
                }],
                false,
            );
            snapshot(&mut raster, resource.id())
        });
        assert_eq!(read(&moved), [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(read(&black), [0, 0, 0, 255, 0, 0, 0, 255]);
    }

    /// A bitmap no context holds is let go of: what is sent to it after is
    /// dropped, a snapshot's pixels never coming. One held is kept.
    #[test]
    fn a_bitmap_no_context_holds_is_let_go_of() {
        let mut raster = CanvasRaster::default();
        let (held, kept) = bitmap(&mut raster);
        let (resource, gone) = bitmap(&mut raster);
        assert!(raster.releases().is_empty());
        drop(resource);
        let released = raster.releases();
        assert!(matches!(released[..], [Item::Release { id }] if id == gone));
        raster.send(released, true);
        assert!(raster.releases().is_empty(), "once");
        assert!(!snapshot(&mut raster, gone).wait_ready());
        assert!(snapshot(&mut raster, kept).wait_ready());
        assert_eq!(held.id(), kept);
    }

    /// The thread held until the sender returned is dropped (or told to go
    /// on).
    pub(crate) fn pause(raster: &mut CanvasRaster) -> Sender<()> {
        let (resume, paused) = mpsc::channel();
        raster.send(vec![Item::Pause(paused)], false);
        resume
    }

    /// What is sent counts until it is drawn: a script's batch by its ops,
    /// a frame's as a frame.
    #[test]
    fn what_is_sent_counts_until_drawn() {
        let mut raster = CanvasRaster::default();
        let (_resource, id) = bitmap(&mut raster);
        let resume = pause(&mut raster);
        let red = fill(Color::rgb(255, 0, 0));
        raster.send(
            vec![Item::Draw {
                id,
                ops: red.clone(),
            }],
            false,
        );
        raster.send(vec![Item::Draw { id, ops: red }], true);
        assert_eq!(raster.backlog_for_testing(), (2, 1));
        resume.send(()).unwrap();
        raster.wait_for_idle_for_testing();
        assert_eq!(raster.backlog_for_testing(), (0, 0));
    }

    /// A frame past `MAX_FRAMES_BEHIND` waits for the oldest to be drawn.
    #[test]
    fn a_frame_too_far_ahead_waits() {
        let mut raster = CanvasRaster::default();
        let (_resource, id) = bitmap(&mut raster);
        let resume = pause(&mut raster);
        for _ in 0..MAX_FRAMES_BEHIND {
            raster.send(
                vec![Item::Draw {
                    id,
                    ops: Vec::new(),
                }],
                true,
            );
        }
        let resumed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let resumer = std::thread::spawn({
            let resumed = resumed.clone();
            move || {
                // Late enough that the next frame is waiting by then.
                std::thread::sleep(std::time::Duration::from_millis(50));
                resumed.store(true, std::sync::atomic::Ordering::SeqCst);
                resume.send(()).unwrap();
            }
        });
        raster.send(
            vec![Item::Draw {
                id,
                ops: Vec::new(),
            }],
            true,
        );
        assert!(
            resumed.load(std::sync::atomic::Ordering::SeqCst),
            "sent before the thread went on"
        );
        resumer.join().unwrap();
    }

    /// A thread that panicked is waited for by nobody: what is sent after
    /// is dropped, a snapshot's pixels never coming.
    #[test]
    fn a_thread_that_panicked_is_waited_for_by_nobody() {
        let mut raster = CanvasRaster::default();
        let (_resource, id) = bitmap(&mut raster);
        // Never resumed: it panics.
        drop(pause(&mut raster));
        for _ in 0..=MAX_FRAMES_BEHIND {
            raster.send(
                vec![Item::Draw {
                    id,
                    ops: Vec::new(),
                }],
                true,
            );
        }
        assert!(!snapshot(&mut raster, id).wait_ready());
    }

    /// A thread that is gone draws nothing more: the snapshots sent to it
    /// never come, and nobody waits.
    #[test]
    fn a_closed_thread_drops_what_it_is_sent() {
        let mut raster = CanvasRaster::default();
        let (_resource, id) = bitmap(&mut raster);
        raster.wait_for_idle_for_testing();
        raster.thread.as_ref().expect("started").backlog.close();
        // Seen closed when it takes the next batch, which it drops.
        assert!(!snapshot(&mut raster, id).wait_ready());
    }
}
