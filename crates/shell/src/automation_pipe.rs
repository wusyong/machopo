//! `machopo --automation-pipe`: the browser, driven by what reads and writes
//! its standard input and output (machopo's WebDriver driver, `machopodriver`):
//! `protocol::automation`'s messages, each followed by a NUL, as on
//! Chromium's `--remote-debugging-pipe` (`DevToolsPipeHandler`, which
//! uses file descriptors 3 and 4 instead). The browser is a window without
//! the platform's window, as the scenario tests' ([`windowless`]), or with
//! `--window` the real one (`window.rs`); it ends when its input does, or
//! at `Browser.close`.
//!
//! [`windowless`]: crate::windowless

use std::io::{BufRead, Read as _, Write};
use std::thread::JoinHandle;

use browser::NetworkContextParams;
use crossbeam_channel::{Receiver, unbounded};
use protocol::automation::{Command, Error, Message, SERVER_ERROR};

use crate::automation_session::{AutomationSession, ClientEvent};
use crate::browser_window::{self, BrowserWindow};
use crate::devtools_http_handler::{self, DevToolsHttpHandler};
use crate::windowless::{self, MemoryClipboard, physical};

/// How the browser starts.
#[derive(Debug, Clone)]
pub struct Options {
    /// The window, CSS px.
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    /// `RendererFlags::test_fonts`.
    pub test_fonts: bool,
    /// The network service's (WPT's domains, its root certificate).
    pub network: NetworkContextParams,
    /// `RendererFlags::num_raster_threads`.
    pub num_raster_threads: Option<usize>,
    /// `--remote-debugging-port`: other clients too, over HTTP and
    /// websockets.
    pub remote_debugging: Option<devtools_http_handler::Options>,
}

impl Default for Options {
    /// 800×600 at 1x.
    fn default() -> Self {
        Self {
            width: 800.0,
            height: 600.0,
            scale: 1.0,
            test_fonts: false,
            network: NetworkContextParams::default(),
            num_raster_threads: None,
            remote_debugging: None,
        }
    }
}

/// Gives the tab shown the document a tab driven through the pipe starts
/// with, as Chrome's new tab has: `about:blank`, as an initial entry, so
/// script can run before anything navigates and the first navigation
/// takes its place in the history.
pub fn start_blank(browser: &mut BrowserWindow) {
    let tab = browser.view().web_contents().tab();
    browser
        .view_mut()
        .update_contents(tab, |contents| contents.navigate_initial("about:blank"));
}

/// What the pipe's reader read: a command, or why a message isn't one,
/// with its id if it has one to answer to.
pub type Read = Result<Command, (Option<i32>, Error)>;

/// The largest message a client may send, on the pipe or a websocket:
/// room for the largest commands, which carry a whole script
/// (`Debugger.setScriptSource`), and no more (tungstenite's default).
/// Past it the pipe or the websocket closes.
pub const MAX_MESSAGE_SIZE: usize = 64 << 20;

/// A message a client sent, as a command.
pub fn read(message: &[u8]) -> Read {
    std::str::from_utf8(message)
        .map_err(|e| (None, Error::new(SERVER_ERROR, e.to_string())))
        .and_then(Command::from_json)
}

/// Reads commands from standard input, on a thread of its own, and hands
/// each to `send`, until it says the browser has gone; `ended` once the
/// input ends, or a message is longer than [`MAX_MESSAGE_SIZE`].
pub fn spawn_reader(
    send: impl Fn(Read) -> bool + Send + 'static,
    ended: impl FnOnce() + Send + 'static,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("AutomationPipeReader".into())
        .spawn(move || {
            let mut input = std::io::stdin().lock();
            let mut message = Vec::new();
            // One more than a message may be, to tell one too long.
            let limit = (MAX_MESSAGE_SIZE as u64).saturating_add(1);
            loop {
                message.clear();
                match (&mut input).take(limit).read_until(0, &mut message) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if message.last() == Some(&0) {
                    message.pop();
                } else if message.len() > MAX_MESSAGE_SIZE {
                    tracing::error!("a message on the pipe is over {MAX_MESSAGE_SIZE} bytes");
                    break;
                }
                if !send(read(&message)) {
                    break;
                }
            }
            ended();
        })
}

/// Writes what `messages` brings to standard output, in order, each
/// followed by a NUL, on a thread of its own, until the channel closes.
pub fn spawn_writer(messages: Receiver<Message>) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("AutomationPipeWriter".into())
        .spawn(move || {
            let mut output = std::io::stdout().lock();
            for message in messages {
                let mut bytes = message.to_json().into_bytes();
                bytes.push(0);
                if output
                    .write_all(&bytes)
                    .and_then(|()| output.flush())
                    .is_err()
                {
                    break;
                }
            }
        })
}

/// Runs the browser, without the platform's window, on the pipe until its
/// input ends or `Browser.close`.
pub fn run(options: Options) -> Result<(), String> {
    let size = physical(options.width, options.height, options.scale);
    let (host_tx, host_rx) = unbounded();
    let (mut browser, context_lost) = windowless::start(
        browser_window::Options {
            test_fonts: options.test_fonts,
            automation: true,
            network: options.network,
            num_raster_threads: options.num_raster_threads,
            // A web test's: no blinking, so each frame is the same.
            caret_blink_interval: None,
        },
        size,
        options.scale,
        Box::new(MemoryClipboard::default()),
        Box::new(move |tab, msg| {
            let _ = host_tx.send((tab, msg));
        }),
    )?;
    start_blank(&mut browser);

    let (outbox, messages) = unbounded::<Message>();
    let writer = spawn_writer(messages).map_err(|e| format!("start the pipe's writer: {e}"))?;
    let (command_tx, commands) = unbounded::<Read>();
    let (ended_tx, ended) = unbounded::<()>();
    spawn_reader(
        move |read| command_tx.send(read).is_ok(),
        move || {
            let _ = ended_tx.send(());
        },
    )
    .map_err(|e| format!("start the pipe's reader: {e}"))?;

    let mut session = AutomationSession::new(&browser, outbox);
    // Kept while the loop runs, so its channel stays open.
    let (clients_tx, clients) = unbounded::<ClientEvent>();
    let server = match &options.remote_debugging {
        Some(remote) => {
            let post = clients_tx.clone();
            Some(
                DevToolsHttpHandler::start(remote, move |event| post.send(event).is_ok())
                    .map_err(|e| format!("--remote-debugging-port: {e}"))?,
            )
        }
        None => None,
    };
    let ended = windowless::drive(
        &mut browser,
        &mut session,
        &host_rx,
        &context_lost,
        &commands,
        &clients,
        &ended,
        options.scale,
    );
    browser.shutdown();
    // The writer ends with the outbox's last sender, the session's (a
    // frame Viz still copies holds one only until it answers), having
    // written what it was given; so do the websockets' connections, which
    // the server waits for as it stops.
    drop(session);
    drop(server);
    drop(clients_tx);
    let _ = writer.join();
    match ended {
        windowless::Ended::Done(_) | windowless::Ended::Closed => Ok(()),
        windowless::Ended::RendererGone => Err("the renderer has gone".into()),
    }
}
