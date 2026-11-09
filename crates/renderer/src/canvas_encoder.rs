//! The thread a canvas's `toBlob` files are encoded on, off the main
//! thread, one for the renderer, started the first time (Blink encodes a
//! PNG in the main thread's idle tasks and a JPEG on a worker thread:
//! `CanvasAsyncBlobCreator`). The main thread sends each call's bitmap as
//! it was at the call: a snapshot the canvas raster thread fills in later
//! is waited for here, never on the main thread. The files go back to the
//! main thread in the order the calls were made, each a task
//! (`MainThreadTask::CanvasEncoded`), which hands it to the DOM.

use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;

use canvas::export::{ExportBitmap, ImageEncoding};
use crossbeam_channel::Sender;
use dom::CanvasEncode;

use crate::task_queue::MainThreadTask;

/// The encoder thread, once started, and the main thread's queue it posts
/// the files to.
pub(crate) struct CanvasEncoder {
    to_main: Sender<MainThreadTask>,
    thread: Option<EncoderThread>,
}

struct EncoderThread {
    sender: Option<mpsc::Sender<CanvasEncode>>,
    handle: Option<JoinHandle<()>>,
}

impl CanvasEncoder {
    pub(crate) fn new(to_main: Sender<MainThreadTask>) -> Self {
        Self {
            to_main,
            thread: None,
        }
    }

    /// `encode` sent to the thread (started the first time); its file comes
    /// back as a task. If the thread can't be started, the call's callback
    /// gets null, in that task too.
    pub(crate) fn send(&mut self, encode: CanvasEncode) {
        let sent = match self.thread() {
            Some(sender) => sender.send(encode).map_err(|error| error.0),
            None => Err(encode),
        };
        if let Err(encode) = sent {
            let _ = self.to_main.send(MainThreadTask::CanvasEncoded {
                id: encode.id,
                file: None,
            });
        }
    }

    fn thread(&mut self) -> Option<&mpsc::Sender<CanvasEncode>> {
        if self.thread.is_none() {
            let (sender, receiver) = mpsc::channel();
            let to_main = self.to_main.clone();
            let handle = std::thread::Builder::new()
                .name("CanvasEncoder".into())
                .spawn(move || run(&receiver, &to_main));
            let handle = match handle {
                Ok(handle) => handle,
                Err(error) => {
                    tracing::error!(%error, "can't start the canvas encoder thread");
                    return None;
                }
            };
            self.thread = Some(EncoderThread {
                sender: Some(sender),
                handle: Some(handle),
            });
        }
        self.thread.as_ref()?.sender.as_ref()
    }
}

impl Drop for CanvasEncoder {
    fn drop(&mut self) {
        let Some(mut thread) = self.thread.take() else {
            return;
        };
        drop(thread.sender.take());
        if let Some(handle) = thread.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run(receiver: &Receiver<CanvasEncode>, to_main: &Sender<MainThreadTask>) {
    while let Ok(encode) = receiver.recv() {
        let _span = trace_event::trace_event!("CanvasEncoder::encode", id = encode.id);
        let file = encode
            .bitmap
            .as_ref()
            .and_then(|bitmap| encode_bitmap(bitmap, encode.encoding));
        // Gone if the renderer is shutting down.
        let _ = to_main.send(MainThreadTask::CanvasEncoded {
            id: encode.id,
            file,
        });
    }
}

/// `bitmap` as a file of `encoding`, its pixels waited for (Blink's
/// `ImageEncoder`): `None` if they never came, or can't be encoded.
pub(crate) fn encode_bitmap(bitmap: &ExportBitmap, encoding: ImageEncoding) -> Option<Vec<u8>> {
    if !bitmap.pixels.wait_ready() {
        return None;
    }
    let pixels = bitmap.pixels.as_bytes();
    let size = gfx::Size::new(bitmap.size.0, bitmap.size.1);
    match encoding {
        ImageEncoding::Png => raster::encode_png(pixels, size, bitmap.opaque),
        ImageEncoding::Jpeg { quality } => {
            raster::encode_jpeg(pixels, size, bitmap.opaque, quality)
        }
    }
}
