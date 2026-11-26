//! `machopo --remote-debugging-pipe` (Chromium's `DevToolsPipeHandler`): the
//! browser, driven through a pipe by what started it (machopo's WebDriver
//! driver, `machopodriver`, the scenario tests' runner, or Chrome's clients:
//! Playwright, Puppeteer): `protocol::automation`'s messages, each followed
//! by a NUL. The pipe is file descriptors 3 (what machopo reads) and 4 (what
//! it writes) on macOS and Linux, and on Windows the two handles
//! `--remote-debugging-io-pipes` names, as Chrome's is
//! ([`ipc::platform::inherited_pipe`]); standard input and output are left
//! to be what they are. With `--headless` the browser is a window without
//! the platform's window, as the scenario tests' ([`windowless`]), and
//! otherwise the real one (`window.rs`); it ends when the pipe's input
//! does, or at `Browser.close`.
//!
//! [`windowless`]: crate::windowless

use std::io::{BufRead, BufReader, PipeReader, PipeWriter, Read as _, Write};
use std::thread::JoinHandle;

use browser::NetworkContextParams;
use crossbeam_channel::{Receiver, unbounded};
use protocol::automation::{Command, Error, Message, SERVER_ERROR};

use crate::automation_session::{AutomationSession, ClientEvent};
use crate::browser_window::{self, BrowserWindow};
use crate::devtools_http_handler::{self, DevToolsHttpHandler};
use crate::windowless::{self, MemoryClipboard, physical};

/// The pipe's two ends, as this process was started with them
/// ([`ipc::platform::inherited_pipe`]).
#[derive(Debug)]
pub struct Pipe {
    /// What the commands come in on.
    pub input: PipeReader,
    /// What the answers and events go out on.
    pub output: PipeWriter,
}

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
    /// Each tab's renderer is a process of its own (`--process-per-tab`).
    pub process_per_tab: bool,
    /// `--remote-debugging-port`: other clients too, over HTTP and
    /// websockets.
    pub remote_debugging: Option<devtools_http_handler::Options>,
    /// `--user-data-dir`'s; in memory without it, each run alone with its
    /// own (as chromedriver gives each session a new folder).
    pub profile: browser::Profile,
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
            process_per_tab: false,
            remote_debugging: None,
            profile: browser::Profile::in_memory(),
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

/// Reads commands from the pipe's `input`, on a thread of its own, and
/// hands each to `send`, until it says the browser has gone; `ended` once
/// the input ends, or a message is longer than [`MAX_MESSAGE_SIZE`].
pub fn spawn_reader(
    input: PipeReader,
    send: impl Fn(Read) -> bool + Send + 'static,
    ended: impl FnOnce() + Send + 'static,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("AutomationPipeReader".into())
        .spawn(move || {
            let mut input = BufReader::new(input);
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

/// Writes what `messages` brings to the pipe's `output`, in order, each
/// followed by a NUL, on a thread of its own, until the channel closes.
pub fn spawn_writer(
    mut output: PipeWriter,
    messages: Receiver<Message>,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("AutomationPipeWriter".into())
        .spawn(move || {
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

/// How long the pipe's writer has, past machopo's limit, to write what the
/// session left it once the session has gone: its last answers
/// (`Browser.close`'s), to a client that reads them.
const WRITER_GRACE: std::time::Duration = std::time::Duration::from_millis(100);

/// Waits for the pipe's writer, its senders gone, until machopo's limit
/// since it began `ending` (or [`WRITER_GRACE`] from now, if that is
/// later): a client that doesn't read the pipe's output, the pipe
/// full, would hold it up for ever (Chromium's `PipeIOBase::Shutdown`
/// joins its threads in a task that doesn't hold the end up). One not done
/// by then is left, with a line; the process ends without it.
pub fn wait_for_writer(writer: JoinHandle<()>, ending: std::time::Instant) {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "seconds from now: within any clock's range"
    )]
    let deadline =
        (ending + browser::shutdown::LIMIT).max(std::time::Instant::now() + WRITER_GRACE);
    while !writer.is_finished() {
        if std::time::Instant::now() >= deadline {
            browser::shutdown::gave_up(
                "the automation pipe's writer",
                "left, its client not reading",
                ending.elapsed(),
            );
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let _ = writer.join();
}

/// Viz has no GPU device that draws, `error` says why: a line, as a step
/// of the end that couldn't be as it should, and the error machopo ends
/// with (as closing it does, its profile and trace written).
pub fn gpu_failed(error: &str) -> String {
    browser::shutdown::gave_up(
        "drawing",
        &format!("Viz having no GPU device that draws ({error})"),
        std::time::Duration::ZERO,
    );
    format!("Viz can't draw: {error}")
}

/// Runs the browser, without the platform's window, on `pipe` until its
/// input ends or `Browser.close`.
pub fn run(pipe: Pipe, options: Options) -> Result<(), String> {
    let size = physical(options.width, options.height, options.scale);
    let (host_tx, host_rx) = unbounded();
    let (mut browser, context_lost) = windowless::start(
        browser_window::Options {
            test_fonts: options.test_fonts,
            automation: true,
            network: options.network,
            num_raster_threads: options.num_raster_threads,
            process_per_tab: options.process_per_tab,
            // A web test's: no blinking, so each frame is the same.
            caret_blink_interval: None,
            profile: options.profile,
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
    let writer =
        spawn_writer(pipe.output, messages).map_err(|e| format!("start the pipe's writer: {e}"))?;
    let (command_tx, commands) = unbounded::<Read>();
    let (ended_tx, ended) = unbounded::<()>();
    // A signal ends it as the input's end does.
    let signalled = ended_tx.clone();
    crate::shutdown_signals::listen(move || {
        let _ = signalled.send(());
    });
    spawn_reader(
        pipe.input,
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
    let ending = std::time::Instant::now();
    browser.shutdown();
    // The writer ends with the outbox's last sender, the session's (a
    // frame Viz still copies holds one only until it answers), having
    // written what it was given; so do the websockets' connections, which
    // the server waits for as it stops.
    drop(session);
    drop(server);
    drop(clients_tx);
    wait_for_writer(writer, ending);
    match ended {
        windowless::Ended::Done(_) | windowless::Ended::Closed => Ok(()),
        windowless::Ended::RendererGone => Err("the renderer has gone".into()),
        windowless::Ended::GpuFailed(error) => Err(gpu_failed(&error)),
    }
}
