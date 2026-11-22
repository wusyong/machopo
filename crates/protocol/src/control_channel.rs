//! The control channel between the browser and a renderer in a process of
//! its own (the pipe Chromium's `mojo::core::Channel` is, with its
//! associated interfaces): which of an end's destinations takes which
//! kind of message, and the renderer's way to send the browser one,
//! whichever kind of renderer it is.
//!
//! A renderer that is a thread of the browser's has no such channel: its
//! messages are moved, not serialized.

use crossbeam_channel::Sender;
use ipc::core::channel::MAX_QUEUED_BYTES;
use ipc::endpoint::{Config, Link, SendError};

use crate::renderer::RendererHostMsg;

/// The channel's end, as a process's threads send by it.
pub type ChannelLink = Link<ipc::platform::Channel>;

/// The destinations at a renderer's end: what the browser's messages are
/// for.
pub mod to_renderer {
    /// [`crate::renderer::RendererMsg`], for the main thread; the first
    /// one is what the renderer starts with.
    pub const MAIN: u32 = 0;
    /// [`crate::renderer::CompositorMsg`], for the compositor thread.
    pub const COMPOSITOR: u32 = 1;
    /// How many there are.
    pub const COUNT: u32 = 2;
}

/// The destinations at the browser's end: what a renderer's messages are
/// for.
pub mod to_browser {
    /// [`crate::renderer::RendererHostMsg`].
    pub const HOST: u32 = 0;
    /// [`crate::viz::CompositorFrameSinkMsg`], for Viz: the browser adds
    /// which sink it is from.
    pub const FRAME_SINK: u32 = 1;
    /// How many there are.
    pub const COUNT: u32 = 2;
}

/// The most bytes of shared memory one end of a channel writes large
/// data into (the regions kept to be used again): past it, large data
/// goes in the message.
const REGION_LIMIT: usize = 64 << 20;

/// How the browser's end of a renderer's channel is made (`browser`), and
/// the renderer's. Only the browser attaches handles, so it makes the
/// shared memory both ways and is the one that opens pipes; a renderer is
/// trusted with neither.
pub const fn config(browser: bool) -> Config {
    Config {
        destinations: if browser {
            to_browser::COUNT
        } else {
            to_renderer::COUNT
        },
        may_attach: browser,
        peer_may_attach: !browser,
        peer_may_open_pipes: !browser,
        region_limit: REGION_LIMIT,
        queue_limit: MAX_QUEUED_BYTES,
    }
}

/// The browser has gone, or the way to it: the message is dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserGone;

/// A renderer's way to send the browser a message (a `mojo::Remote` to
/// the browser's interfaces), from any of its threads, without waiting.
#[derive(Clone)]
pub struct BrowserSender(Way);

#[derive(Clone)]
enum Way {
    /// A renderer in the browser's process: the message is moved.
    Local(Sender<RendererHostMsg>),
    /// A renderer in its own: the message is written to its channel.
    Channel(ChannelLink),
}

impl BrowserSender {
    /// For a renderer in a process of its own, whose channel `link` is.
    pub fn over(link: ChannelLink) -> Self {
        Self(Way::Channel(link))
    }

    /// Fails once the browser's end has gone; the message is dropped.
    pub fn send(&self, msg: RendererHostMsg) -> Result<(), BrowserGone> {
        match &self.0 {
            Way::Local(sender) => sender.send(msg).map_err(|_| BrowserGone),
            Way::Channel(link) => send_over(link, to_browser::HOST, &msg).map_err(|_| BrowserGone),
        }
    }
}

/// For a renderer in the browser's process, and the tests that stand in
/// for a browser.
impl From<Sender<RendererHostMsg>> for BrowserSender {
    fn from(sender: Sender<RendererHostMsg>) -> Self {
        Self(Way::Local(sender))
    }
}

impl std::fmt::Debug for BrowserSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            Way::Local(_) => "BrowserSender(local)",
            Way::Channel(_) => "BrowserSender(channel)",
        })
    }
}

/// The channel is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelGone;

/// Writes `msg` to the channel for `destination`. A message the channel
/// refuses (one that doesn't serialize, or is longer than a message may
/// be) is this end's own mistake: it is dropped and said so, and the
/// channel goes on.
pub fn send_over<T: serde::Serialize>(
    link: &ChannelLink,
    destination: u32,
    msg: &T,
) -> Result<(), ChannelGone> {
    match link.send(destination, msg) {
        Ok(()) => Ok(()),
        Err(SendError::Gone(_)) => Err(ChannelGone),
        Err(SendError::Refused) => {
            tracing::error!(
                destination,
                kind = std::any::type_name::<T>(),
                "a message the channel refused was dropped"
            );
            Ok(())
        }
    }
}
