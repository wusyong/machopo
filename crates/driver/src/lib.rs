//! machopodriver: machopo's WebDriver (classic) server, as Chromium's
//! chromedriver is Chrome's. It serves WebDriver's HTTP API, and for each
//! session starts a `machopo --automation-pipe`, which it drives with
//! DevTools-protocol-shaped messages (`protocol::automation`) on machopo's
//! standard input and output.
//!
//! Three parts, on threads of their own:
//! - [`http`]: HTTP/1.1 (hyper, on a single-threaded tokio runtime). Each
//!   request is matched to a route of the `webdriver` crate, parsed into
//!   its command, and handed to the dispatcher; the answer comes back
//!   through a oneshot channel, so the server keeps taking requests.
//! - the dispatcher: carries out the commands one at a time ([`session`]).
//!   A command may wait (for a page to load): later ones wait their turn,
//!   as WebDriver's are serial anyway.
//! - [`machopo`]: one per session: the process, and the thread that reads its
//!   messages.

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

mod actions;
mod error;
mod http;
mod machopo;
mod session;

use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::thread::JoinHandle;

pub use error::Failure;

/// How sessions start machopo.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The `machopo` to start.
    pub binary: PathBuf,
    /// Its arguments, after `--automation-pipe` (`--test-fonts`, WPT's
    /// `--host-resolver-rules`...).
    pub binary_args: Vec<String>,
}

/// A running server.
pub struct Server {
    addr: SocketAddr,
    http: JoinHandle<()>,
}

impl Server {
    /// Where it listens.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Waits until the server stops (it doesn't, but for its errors).
    pub fn join(self) {
        let _ = self.http.join();
    }
}

/// Serves WebDriver on `listener` (bound to 127.0.0.1: WebDriver drives a
/// browser, and isn't for the network).
pub fn start(listener: TcpListener, options: Options) -> std::io::Result<Server> {
    let addr = listener.local_addr()?;
    let (commands, receiver) = crossbeam_channel::unbounded();
    std::thread::Builder::new()
        .name("WebDriverDispatcher".into())
        .spawn(move || session::dispatch(receiver, options))?;
    let http = http::serve(listener, addr, commands)?;
    Ok(Server { addr, http })
}
