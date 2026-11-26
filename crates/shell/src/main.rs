//! `machopo`: the browser shell (`//chrome/browser/ui` + `content_shell`).
//! `machopo --help` describes the options and environment variables.
//!
//! Thread layout:
//!
//! - `main` (Chromium: `CrBrowserMain`): winit event loop, window, UI.
//! - `VizCompositorThread`: aggregates frames and presents with wgpu.
//! - `CrRendererMain`: one per tab; V8, pipeline, cc's main side.
//! - `Compositor`: one per tab; cc's impl side: Viz's `BeginFrame`s and the
//!   browser's input come here, and it scrolls without the main thread.
//! - `Chrome_IOThread`: renderer -> browser messages, and `RendererGone`
//!   once the renderer's channel closes; passes the renderer's subresource
//!   requests to the network service.
//! - `NetworkService`: every load (`net`), on a tokio runtime.

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

mod window;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use browser::{HostMappingRules, NetworkContextParams};
use ipc::platform::InheritError;
use shell::chrome::UiLayout;
use shell::devtools_http_handler::{self, AllowedOrigins};
use shell::theme::ColorMode;
use shell::{devtools_pipe_handler, to_url};
use tracing_log::AsLog;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const DEFAULT_URL: &str = "about:demo";
// Rust's allocations in machopo (the nodes' data, the parser's, style's) go
// through mimalloc, as Chromium's go through PartitionAlloc (PartitionAlloc-
// Everywhere); C++'s (V8, Skia) stay with the platform's malloc, so memory
// one side allocates the other must never free. Only this executable: the
// other crates' tests keep the platform's allocator.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

const DEFAULT_LOG_FILTER: &str = "info,wgpu_core=warn,wgpu_hal=warn,naga=warn";

const USAGE: &str = "\
usage: machopo [--ui-layout LAYOUT] [--ui-theme THEME] [--scale F] [--test-fonts]
           [--num-raster-threads N] [--user-data-dir DIR]
           [--single-process | --process-per-tab] [--no-sandbox]
           [--remote-debugging-port PORT [--remote-allow-origins ORIGINS]]
           [URL|PATH]
       machopo --remote-debugging-pipe [--headless] [--scale F]
           [--host-resolver-rules RULES] [--additional-trust-anchors PEM] [--test-fonts]
           [--num-raster-threads N] [--user-data-dir DIR]
           [--single-process | --process-per-tab] [--no-sandbox]
           [--remote-debugging-port PORT [--remote-allow-origins ORIGINS]]

Options that take a value take it as --name VALUE or --name=VALUE.";

/// `machopo --help`. Keep it in step with `parse_args` and the environment
/// variables `bindings` and `main` read.
fn help() -> String {
    format!(
        "\
machopo: a Chromium-like browser in Rust.

{USAGE}

Opens URL or the file at PATH in a window, or with --remote-debugging-pipe
is driven through a pipe by what started it. URLs:
http://..., https://..., file://..., about:blank, about:demo (the built-in
demo page, the default). Anything else is taken as a path; a relative one
is resolved against the working directory.

Options:
  --ui-layout LAYOUT  In a window (not with --remote-debugging-pipe): how the
                      UI is laid out at first, horizontal (the default: the
                      tabs above the toolbar) or sidebar (the toolbar and
                      the tabs down the left). Cmd+Shift+, (Ctrl+Shift+,
                      elsewhere) switches between them.
  --ui-theme THEME    In a window (not with --remote-debugging-pipe): light or
                      dark, the UI's colors (not the pages'). By default the
                      system's, changing as it does.
  --scale F           The device scale factor, as Chromium's
                      --force-device-scale-factor. With --headless it is 1
                      by default, and the pipe's screenshots are the viewport times F, in
                      physical pixels. A window takes F in place of the
                      screen's, F physical pixels a CSS pixel.
  --test-fonts        Use only the built-in test fonts: Ahem, and Lato for
                      every generic family, as the reftests do.
  --process-per-tab   Each tab's renderer is a process of its own (machopo
                      started again with --type=renderer), which a page
                      that crashes takes down alone. The default.
  --single-process    Every tab's renderer is threads of machopo's own
                      process, as Chromium's switch. They are then outside
                      the operating system's sandbox.
  --no-sandbox        A renderer process isn't put in the operating
                      system's sandbox, as Chromium's switch: a page that
                      takes over its renderer then has what machopo has.
                      For finding out whether the sandbox is what breaks
                      something. (The sandbox is Linux's, Landlock and
                      seccomp, and macOS's, a Seatbelt profile; a
                      renderer process doesn't start without it.)
  --num-raster-threads N
                      Threads each page's tiles are rastered on at once, as
                      Chromium's switch (default 1: on the compositor's
                      thread). More raster a page of many tiles faster
                      (resizing it: about 4 times on 9 threads), but take
                      CPU and memory from the page while it runs; here,
                      {threads} would leave a processor to its main thread.
  --remote-debugging-pipe
                      Be driven through a pipe, as Chrome's switch, by
                      machopo's WebDriver driver (machopodriver), the
                      scenario tests' runner or Chrome's clients
                      (Playwright, Puppeteer): DevTools-protocol JSON
                      messages, each followed by a NUL, read from file
                      descriptor 3 and written to 4 (on Windows, the
                      handles --remote-debugging-io-pipes=READ,WRITE
                      names). In a shell, 3<&0 4>&1 makes them standard
                      input and output. The window is a real one, which
                      stays on top and ignores the mouse and keyboard
                      meanwhile (but with --headless). Ends when the
                      pipe's input does. Logs go to standard error, as
                      always.
  --headless          With --remote-debugging-pipe: a window without the
                      platform's, as Chrome's switch: nothing shows.
  --host-resolver-rules RULES
                      With --remote-debugging-pipe: hosts that resolve to a fixed
                      address, or not at all, as Chromium's switch; web
                      tests map WPT's domains to this machine with
                      \"MAP nonexistent.*.test ^NOTFOUND, MAP *.test 127.0.0.1\".
  --additional-trust-anchors PEM
                      With --remote-debugging-pipe: trust the root
                      certificates in the PEM file too, as web tests do
                      WPT's.
  --user-data-dir DIR Keep the profile (what pages store) in the folder DIR,
                      made if it isn't there, as Chrome's switch, its HTTP
                      cache in DIR's Cache (for a DIR in the user's data
                      folder, at the same place in the system's folder
                      for caches, as the user's own is). Without it a
                      window's is the user's own ({default_profile}, its
                      HTTP cache in {default_cache}), and
                      --remote-debugging-pipe's is in memory, gone as it
                      ends.
                      Only one machopo has a folder at a time: a second one
                      doesn't start.
  --remote-debugging-port PORT
                      In a window or with --remote-debugging-pipe: serve the
                      DevTools protocol on 127.0.0.1:PORT (0: a free port),
                      as Chrome's switch, for Chrome's DevTools
                      (chrome://inspect) and other CDP clients: /json/list
                      lists the tabs, and websockets connect to the
                      browser or straight to a tab. Where it listens goes
                      to standard error. The methods are the pipe's; its
                      Machopo domain only with --remote-debugging-pipe.
  --remote-allow-origins ORIGINS
                      Pages of these origins (comma-separated, or *) may
                      connect too, as Chrome's switch; by default no page
                      may but Chrome's DevTools' own.
  -h, --help          Print this help.

Environment:
  RUST_LOG                 Log filter (default: {DEFAULT_LOG_FILTER}).
                           style=debug lists the CSS that was ignored.
                           style, layout, paint and compositor at debug log
                           what each frame did again: elements restyled,
                           boxes rebuilt and laid out, boxes painted, tiles
                           rastered.
  MACHOPO_V8_FLAGS             Extra V8 flags, e.g. \"--trace-gc\".
  MACHOPO_V8_THREAD_POOL_SIZE  V8 worker threads (default: V8 decides).
  MACHOPO_GC_OPTIONS           The DOM's heap's options for the GC's test runs,
                           comma-separated: stress (a GC's work at every
                           safepoint), verify (the heap checked after each
                           GC), nursery=BYTES (default: none).
  MIMALLOC_PURGE_DELAY     How long machopo's allocator (mimalloc, for the Rust
                           side) keeps freed memory before giving it back to
                           the system, in ms (default 10); 0 gives it back at
                           once: less memory at its peak, a little slower.
  MACHOPO_TRACE                A file to write a trace of what each thread did to,
                           as Perfetto's protobuf (Perfetto's UI opens it):
                           the spans, named as Chrome's trace events, V8's
                           own events (GC, deopts), cppgc's phases (the
                           DOM's heap), and the page's
                           performance.mark() and measure(), as Chrome's
                           blink.user_timing. A renderer in a process of
                           its own writes its own, the file's name with
                           its process id after it.
  MACHOPO_GPU_ADAPTER          cpu draws with the CPU adapter (WARP on Windows,
                           Mesa's lavapipe or llvmpipe on Linux) even with a
                           GPU; by default a GPU, the CPU adapter only if
                           there is none.

Keyboard and mouse shortcuts in the window: see crates/shell/README.md.",
        threads =
            std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1).max(1)),
        default_profile = browser::default_user_data_dir().map_or_else(
            || "nowhere here: the environment doesn't say where".to_owned(),
            |dir| dir.display().to_string()
        ),
        default_cache = browser::default_user_data_dir().map_or_else(
            || "nowhere".to_owned(),
            |dir| browser::user_cache_dir_on(browser::Platform::current(), &dir, |name| {
                std::env::var_os(name)
            })
            .join("Cache")
            .display()
            .to_string()
        ),
    )
}

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Run(Box<Args>),
}

#[derive(Debug, PartialEq)]
struct Args {
    url: String,
    /// `--ui-layout`.
    ui_layout: UiLayout,
    /// `--ui-theme`; `None`: the system's.
    ui_theme: Option<ColorMode>,
    test_fonts: bool,
    /// `--remote-debugging-pipe`.
    pipe: Option<PipeArgs>,
    /// `--scale` for a real window: in place of the screen's.
    window_scale: Option<f32>,
    /// `--num-raster-threads`; `None`: the default.
    num_raster_threads: Option<usize>,
    /// Not `--single-process`: each tab's renderer is a process of its
    /// own.
    process_per_tab: bool,
    /// `--no-sandbox`: the renderer processes aren't put in a sandbox.
    no_sandbox: bool,
    /// `--remote-debugging-port` and `--remote-allow-origins`.
    remote_debugging: Option<devtools_http_handler::Options>,
    /// `--user-data-dir`.
    user_data_dir: Option<PathBuf>,
}

/// `--remote-debugging-pipe`, and the settings that go with it.
#[derive(Debug, PartialEq)]
struct PipeArgs {
    /// `--headless`: the window without the platform's; else the real one.
    headless: bool,
    /// `--scale`: the device scale factor of the window without the
    /// platform's (a real window takes the screen's).
    scale: f32,
    host_resolver_rules: Option<String>,
    additional_trust_anchors: Option<PathBuf>,
    /// `--remote-debugging-io-pipes`: on Windows, the pipe's two handles
    /// (elsewhere it is 3 and 4, whatever this says, as Chrome's).
    io_pipes: Option<String>,
}

impl Default for PipeArgs {
    /// Headless, at 1x, the network as it is, no handles named.
    fn default() -> Self {
        Self {
            headless: true,
            scale: 1.0,
            host_resolver_rules: None,
            additional_trust_anchors: None,
            io_pipes: None,
        }
    }
}

/// Parses the arguments after the program name. Errors say what was wrong.
fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut url = None;
    let mut ui_layout = None;
    let mut ui_theme = None;
    let mut scale = None;
    let mut test_fonts = false;
    let mut pipe = false;
    let mut headless = false;
    let mut io_pipes = None;
    let mut host_resolver_rules = None;
    let mut additional_trust_anchors = None;
    let mut num_raster_threads = None;
    let mut single_process = false;
    let mut process_per_tab = false;
    let mut no_sandbox = false;
    let mut remote_debugging_port = None;
    let mut remote_allow_origins = None;
    let mut user_data_dir = None;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        // `--name=value`, as Chrome's switches are written (and the tools
        // that start Chrome write them), or `--name value`.
        let (name, mut inline) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => {
                (name.to_owned(), Some(value.to_owned()))
            }
            _ => (arg.clone(), None),
        };
        let mut value = |name: &str| match inline.take() {
            Some(value) => Ok(value),
            None => it.next().ok_or_else(|| format!("{name} needs a value")),
        };
        match name.as_str() {
            "--scale" => {
                let v = value("--scale")?;
                let parsed = v.parse::<f32>().ok().filter(|f| f.is_finite() && *f > 0.0);
                scale = Some(parsed.ok_or_else(|| {
                    format!("--scale wants a positive number, like 2, not {v:?}")
                })?);
            }
            "--ui-layout" => {
                let v = value("--ui-layout")?;
                ui_layout = Some(match v.as_str() {
                    "horizontal" => UiLayout::Horizontal,
                    "sidebar" => UiLayout::Sidebar,
                    _ => {
                        return Err(format!(
                            "--ui-layout wants horizontal or sidebar, not {v:?}"
                        ));
                    }
                });
            }
            "--ui-theme" => {
                let v = value("--ui-theme")?;
                ui_theme = Some(match v.as_str() {
                    "light" => ColorMode::Light,
                    "dark" => ColorMode::Dark,
                    _ => return Err(format!("--ui-theme wants light or dark, not {v:?}")),
                });
            }
            "--test-fonts" => test_fonts = true,
            "--num-raster-threads" => {
                let v = value("--num-raster-threads")?;
                let parsed = v.parse::<usize>().ok().filter(|n| *n > 0);
                num_raster_threads = Some(parsed.ok_or_else(|| {
                    format!("--num-raster-threads wants a whole number from 1, like 4, not {v:?}")
                })?);
            }
            "--single-process" => single_process = true,
            "--process-per-tab" => process_per_tab = true,
            "--no-sandbox" => no_sandbox = true,
            "--remote-debugging-pipe" => pipe = true,
            "--headless" => headless = true,
            "--remote-debugging-io-pipes" => io_pipes = Some(value("--remote-debugging-io-pipes")?),
            // What these were, now Chrome's.
            "--automation-pipe" => {
                return Err(
                    "--automation-pipe is --remote-debugging-pipe --headless now: the \
                            pipe is file descriptors 3 and 4, as Chrome's"
                        .into(),
                );
            }
            "--window" => {
                return Err(
                    "--window is gone: --remote-debugging-pipe without --headless is the real \
                     window"
                        .into(),
                );
            }
            "--host-resolver-rules" => host_resolver_rules = Some(value("--host-resolver-rules")?),
            "--additional-trust-anchors" => {
                additional_trust_anchors =
                    Some(PathBuf::from(value("--additional-trust-anchors")?));
            }
            "--remote-debugging-port" => {
                let v = value("--remote-debugging-port")?;
                remote_debugging_port = Some(v.parse::<u16>().map_err(|_| {
                    format!("--remote-debugging-port wants a port, 0 to 65535, not {v:?}")
                })?);
            }
            "--remote-allow-origins" => {
                remote_allow_origins = Some(value("--remote-allow-origins")?);
            }
            "--user-data-dir" => {
                let v = value("--user-data-dir")?;
                if v.is_empty() {
                    return Err("--user-data-dir wants a folder".into());
                }
                user_data_dir = Some(PathBuf::from(v));
            }
            "-h" | "--help" => return Ok(Command::Help),
            s if s.starts_with('-') => return Err(format!("unknown option {s:?}")),
            _ if url.is_none() => url = Some(arg),
            _ => return Err(format!("more than one URL or path: {arg:?}")),
        }
        if inline.is_some() {
            return Err(format!("{name} takes no value"));
        }
    }
    if single_process && process_per_tab {
        return Err(
            "--single-process and --process-per-tab say opposite things: give one of them".into(),
        );
    }
    // A process a tab unless told otherwise, as Chromium.
    let process_per_tab = !single_process;
    if pipe && ui_layout.is_some() {
        return Err(
            "--ui-layout only applies to a window: what drives the pipe starts horizontal, so \
             that the page's size doesn't change with it"
                .into(),
        );
    }
    if pipe && ui_theme.is_some() {
        return Err(
            "--ui-theme only applies to a window: what drives the pipe sees the light UI, so \
             that its screenshots don't change with the machine"
                .into(),
        );
    }
    if pipe && url.is_some() {
        return Err(
            "--remote-debugging-pipe is driven through the pipe, which navigates: no URL or path"
                .into(),
        );
    }
    if !pipe
        && (headless
            || host_resolver_rules.is_some()
            || additional_trust_anchors.is_some()
            || io_pipes.is_some())
    {
        return Err(
            "--headless, --host-resolver-rules, --additional-trust-anchors and \
             --remote-debugging-io-pipes only apply with --remote-debugging-pipe"
                .into(),
        );
    }
    if remote_debugging_port.is_none() && remote_allow_origins.is_some() {
        return Err("--remote-allow-origins only applies with --remote-debugging-port".into());
    }
    let remote_debugging = remote_debugging_port.map(|port| devtools_http_handler::Options {
        port,
        allowed_origins: remote_allow_origins
            .as_deref()
            .map(AllowedOrigins::parse)
            .unwrap_or_default(),
    });
    // A real window's, if given; the pipe's window without the platform's
    // has 1 by default.
    let window_scale = scale.filter(|_| !headless);
    let scale = scale.unwrap_or(1.0);
    Ok(Command::Run(Box::new(Args {
        window_scale,
        url: url.map_or_else(|| DEFAULT_URL.to_owned(), |arg| to_url(&arg)),
        ui_layout: ui_layout.unwrap_or_default(),
        ui_theme,
        test_fonts,
        num_raster_threads,
        process_per_tab,
        no_sandbox,
        remote_debugging,
        user_data_dir,
        pipe: pipe.then_some(PipeArgs {
            headless,
            scale,
            host_resolver_rules,
            additional_trust_anchors,
            io_pipes,
        }),
    })))
}

/// The network service's settings `--remote-debugging-pipe`'s options ask
/// for: the rules parsed, the certificates read.
fn network_params(pipe: &PipeArgs) -> Result<NetworkContextParams, String> {
    let mut params = NetworkContextParams::default();
    if let Some(rules) = &pipe.host_resolver_rules {
        params.host_resolver_rules =
            HostMappingRules::parse(rules).map_err(|e| format!("--host-resolver-rules: {e}"))?;
    }
    if let Some(path) = &pipe.additional_trust_anchors {
        let pem = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        params
            .add_trust_anchors_pem(&pem)
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(params)
}

/// The folder of the profile `args` ask for: `--user-data-dir`'s, as the
/// working directory has it now; without it the user's own for a window
/// (none where the environment doesn't say where the user's files are),
/// and none for what is driven through the pipe. None: a profile in
/// memory.
fn profile_dir(args: &Args) -> Option<PathBuf> {
    match &args.user_data_dir {
        Some(dir) => Some(std::path::absolute(dir).unwrap_or_else(|_| dir.clone())),
        None if args.pipe.is_some() => None,
        None => browser::default_user_data_dir(),
    }
}

/// The profile `args` ask for (`profile_dir`), its folder opened.
fn profile(args: &Args) -> Result<browser::Profile, browser::ProfileError> {
    match profile_dir(args) {
        Some(dir) => browser::Profile::open(dir),
        None => {
            if args.pipe.is_none() {
                tracing::warn!(
                    "the environment doesn't say where the user's files are: nothing pages store \
                     is kept (--user-data-dir says where)"
                );
            }
            Ok(browser::Profile::in_memory())
        }
    }
}

/// The log on standard error, as `filter` says.
fn log_layer<S>(filter: tracing_subscriber::EnvFilter) -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    tracing_subscriber::fmt::layer()
        .with_thread_names(true)
        // As Chrome's, with standard output left to what writes there.
        .with_writer(std::io::stderr)
        .with_filter(filter)
}

/// Starts the process's trace into `file`, if there is to be one: the
/// layer that records it, and what ends it when dropped. Before anything
/// starts V8, which records its events only if the trace is up by then.
fn start_trace<S>(
    file: Option<&Path>,
) -> (
    Option<impl tracing_subscriber::Layer<S> + Send + Sync + 'static>,
    Option<shell::trace::TraceEnd>,
)
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a> + Send + Sync,
{
    let Some(file) = file else {
        return (None, None);
    };
    match shell::trace::layer(file) {
        Ok((layer, end)) => (Some(layer), Some(end)),
        Err(e) => {
            eprintln!(
                "machopo: MACHOPO_TRACE: can't trace into {}: {e:?}",
                file.display()
            );
            (None, None)
        }
    }
}

/// A trace's file, made (or emptied) to be written: said if it can't be.
#[cfg(unix)]
fn open_trace(file: &Path) -> Option<(std::fs::File, &Path)> {
    match std::fs::File::create(file) {
        Ok(opened) => Some((opened, file)),
        Err(e) => {
            eprintln!(
                "machopo: MACHOPO_TRACE: can't trace into {}: {e}",
                file.display()
            );
            None
        }
    }
}

/// As [`start_trace`], into a file [`open_trace`] opened: a renderer
/// process, which opens it before it enters its sandbox and starts the
/// trace after.
#[cfg(unix)]
fn start_trace_into<S>(
    opened: Option<(std::fs::File, &Path)>,
) -> (
    Option<impl tracing_subscriber::Layer<S> + Send + Sync + 'static>,
    Option<shell::trace::TraceEnd>,
)
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a> + Send + Sync,
{
    let Some((opened, file)) = opened else {
        return (None, None);
    };
    match shell::trace::layer_into(opened) {
        Ok((layer, end)) => (Some(layer), Some(end)),
        Err(e) => {
            eprintln!(
                "machopo: MACHOPO_TRACE: can't trace into {}: {e:?}",
                file.display()
            );
            (None, None)
        }
    }
}

/// What to say of renderers that run outside the operating system's
/// sandbox, where this build has one: a page that takes over its renderer
/// then has everything machopo has.
fn sandbox_warning(no_sandbox: bool, process_per_tab: bool) -> Option<&'static str> {
    if !shell::renderer_sandbox::SUPPORTED {
        return None;
    }
    if !process_per_tab {
        Some(
            "--single-process: the renderers are threads of this process, outside the \
             operating system's sandbox. Only for pages you trust.",
        )
    } else if no_sandbox {
        Some(
            "--no-sandbox: the renderer processes run outside the operating system's \
             sandbox. Only for pages you trust.",
        )
    } else {
        None
    }
}

fn main() -> ExitCode {
    // A renderer process: nothing of the browser's is read or started.
    // Its trace is a file of its own, the browser's with this process's
    // id after it: a trace is one process's, and two writing one file
    // would spoil it. The two are on one clock, the machine's.
    if std::env::args().nth(1).as_deref() == Some(shell::renderer_sandbox::PROBE_SWITCH) {
        shell::renderer_sandbox::probe(std::env::args_os().skip(2));
    }
    if std::env::args().nth(1).as_deref() == Some(shell::RENDERER_PROCESS_SWITCH) {
        // What it writes is opened first, while it has the file system:
        // its census's file, a file of its own as its trace is, and its
        // trace's.
        base::census::own_file();
        let file = std::env::var_os("MACHOPO_TRACE").map(|file| {
            let mut file = file;
            file.push(format!(".{}", std::process::id()));
            std::path::PathBuf::from(file)
        });
        #[cfg(unix)]
        let opened = file.as_deref().and_then(open_trace);
        // Then into its sandbox, before the log and the trace: the trace
        // starts a thread, and the process enters while it is one.
        let start = match shell::start_renderer() {
            Ok(start) => start,
            Err(code) => return ExitCode::from(code),
        };
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| DEFAULT_LOG_FILTER.into());
        let log_records = filter
            .max_level_hint()
            .unwrap_or(LevelFilter::TRACE)
            .as_log();
        // Ended, and its last events written, as the process ends.
        #[cfg(unix)]
        let (trace, trace_end) = start_trace_into(opened);
        #[cfg(not(unix))]
        let (trace, trace_end) = start_trace(file.as_deref());
        tracing_subscriber::registry()
            .with(log_layer(filter))
            .with(trace)
            .init();
        // As the browser's: only as many of the `log` crate's records as the
        // log takes, not every one the trace's layer would let through
        // (html5ever's for each token, made only to be dropped).
        tracing_log::log::set_max_level(log_records);
        end(shell::renderer_main(start), trace_end);
    }
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(Command::Run(args)) => *args,
        Ok(Command::Help) => {
            println!("{}", help());
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("machopo: {e}\n\n{USAGE}\n\nRun machopo --help for the details.");
            return ExitCode::from(2);
        }
    };
    // Before anything is opened (the trace's file, the profile's): with 3
    // and 4 closed as machopo started, what it opened next would be given
    // those numbers, and taken for the pipe.
    let pipe = match args.pipe.as_ref().map(take_pipe).transpose() {
        Ok(pipe) => pipe,
        Err(e) => {
            eprintln!("machopo: --remote-debugging-pipe: {e}");
            return ExitCode::from(2);
        }
    };

    if let Some(warning) = sandbox_warning(args.no_sandbox, args.process_per_tab) {
        eprintln!("machopo: {warning}");
    }
    if args.no_sandbox {
        shell::renderer_sandbox::disable();
    }

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| DEFAULT_LOG_FILTER.into());
    let log_records = filter
        .max_level_hint()
        .unwrap_or(LevelFilter::TRACE)
        .as_log();
    let log = log_layer(filter);
    // The trace's end is written as the process ends (or when `trace_end`
    // drops, as `main` returns before anything has started). Before
    // anything starts V8, which records its events only if the trace is up
    // by then.
    let file = std::env::var_os("MACHOPO_TRACE").map(std::path::PathBuf::from);
    let (trace, trace_end) = start_trace(file.as_deref());
    tracing_subscriber::registry().with(log).with(trace).init();
    // The `log` crate's records go only to the log. `init` lets them through
    // as far as any layer takes anything, and the trace's takes spans of
    // every level (but no events): each record would be made, html5ever's
    // for every token it parses, only to be dropped.
    tracing_log::log::set_max_level(log_records);
    let profile = match profile(&args) {
        Ok(profile) => profile,
        Err(e) => {
            eprintln!("machopo: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = match (&args.pipe, pipe) {
        (Some(args_pipe), Some(pipe)) => match network_params(args_pipe) {
            Ok(network) if !args_pipe.headless => window::run(window::Options {
                url: args.url,
                ui_layout: UiLayout::Horizontal,
                ui_theme: Some(ColorMode::Light),
                test_fonts: args.test_fonts,
                num_raster_threads: args.num_raster_threads,
                process_per_tab: args.process_per_tab,
                pipe: Some((network, pipe)),
                remote_debugging: args.remote_debugging,
                profile,
                scale: args.window_scale,
            }),
            Ok(network) => devtools_pipe_handler::run(
                pipe,
                devtools_pipe_handler::Options {
                    scale: args_pipe.scale,
                    test_fonts: args.test_fonts,
                    network,
                    num_raster_threads: args.num_raster_threads,
                    process_per_tab: args.process_per_tab,
                    remote_debugging: args.remote_debugging,
                    profile,
                    ..Default::default()
                },
            ),
            Err(e) => {
                eprintln!("machopo: {e}");
                return ExitCode::from(2);
            }
        },
        _ => window::run(window::Options {
            url: args.url,
            ui_layout: args.ui_layout,
            ui_theme: args.ui_theme,
            test_fonts: args.test_fonts,
            num_raster_threads: args.num_raster_threads,
            process_per_tab: args.process_per_tab,
            pipe: None,
            remote_debugging: args.remote_debugging,
            profile,
            scale: args.window_scale,
        }),
    };
    let code = match result {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!("{e}");
            1
        }
    };
    end(code, trace_end)
}

/// The pipe this process was started with, for `--remote-debugging-pipe`
/// (`pipe`'s handles on Windows), or why there is none.
fn take_pipe(pipe: &PipeArgs) -> Result<devtools_pipe_handler::Pipe, String> {
    match ipc::platform::inherited_pipe(pipe.io_pipes.as_deref()) {
        Ok((input, output)) => Ok(devtools_pipe_handler::Pipe { input, output }),
        // What a person who started machopo in a shell is most likely to
        // need.
        Err(e @ (InheritError::Missing(_) | InheritError::NotAPipe(_))) if cfg!(unix) => {
            Err(format!(
                "{e}: what starts machopo gives it the pipe at file descriptors 3 and 4 (in a \
                 shell, 3<&0 4>&1 makes them standard input and output)"
            ))
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Ends this process, its renderers and its threads wound down, with
/// `code`: the trace's end written, and then at once, its destructors not
/// run (as Chromium's renderer and GPU process end,
/// `base::Process::TerminateCurrentProcessImmediately`). The GPU's drivers,
/// V8 and LLVM have destructors of their own, which would run beside the
/// threads that can't be stopped (the drivers' own, V8's workers, those a
/// shutdown gave up on) and crash under them. Nothing else is left to
/// write by then: the census is written as each thread that counts ends
/// (none is this one), the pipe's writer flushes each message, and the
/// profile is closed as the window shuts down.
fn end(code: u8, trace_end: Option<shell::trace::TraceEnd>) -> ! {
    drop(trace_end);
    ipc::platform::terminate_current_process_immediately(i32::from(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The memory mimalloc has committed now, from its statistics.
    fn mimalloc_committed() -> usize {
        let stats = mimalloc::MiMalloc::stats_json().expect("mimalloc's statistics");
        let stats = stats.to_str().expect("UTF-8");
        let committed = &stats[stats.find("\"committed\"").expect("committed")..];
        let current =
            &committed[committed.find("\"current\":").expect("current") + "\"current\":".len()..];
        current
            .trim_start()
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|n| n.parse().ok())
            .expect("a number")
    }

    /// Rust's allocations in machopo are mimalloc's (`GLOBAL`): a big one
    /// shows in mimalloc's own statistics, which the platform's
    /// allocator's wouldn't move.
    #[test]
    fn no_sandbox_is_a_switch_and_renderers_outside_one_are_warned_of() {
        assert!(!run(&["about:blank"]).no_sandbox);
        assert!(run(&["--no-sandbox"]).no_sandbox);
        assert!(run(&["--remote-debugging-pipe", "--headless", "--no-sandbox"]).no_sandbox);
        if !shell::renderer_sandbox::SUPPORTED {
            assert_eq!(sandbox_warning(true, true), None);
            return;
        }
        assert_eq!(sandbox_warning(false, true), None);
        assert!(
            sandbox_warning(true, true)
                .unwrap()
                .starts_with("--no-sandbox")
        );
        assert!(
            sandbox_warning(false, false)
                .unwrap()
                .starts_with("--single-process")
        );
        assert!(
            sandbox_warning(true, false)
                .unwrap()
                .starts_with("--single-process")
        );
    }

    #[test]
    fn renderers_are_processes_unless_told_otherwise() {
        let run = |args: &[&str]| match parse(args) {
            Ok(Command::Run(args)) => *args,
            other => panic!("{other:?}"),
        };
        // The default, and said outright; in a window and by the pipe.
        assert!(run(&["about:blank"]).process_per_tab);
        assert!(run(&["--remote-debugging-pipe", "--headless"]).process_per_tab);
        assert!(run(&["--remote-debugging-pipe"]).process_per_tab);
        assert!(!run(&["--single-process"]).process_per_tab);
        assert!(
            !run(&["--remote-debugging-pipe", "--headless", "--single-process"]).process_per_tab
        );
        assert!(!run(&["--remote-debugging-pipe", "--single-process"]).process_per_tab);
        assert!(run(&["--process-per-tab"]).process_per_tab);
        assert!(
            run(&["--remote-debugging-pipe", "--headless", "--process-per-tab"]).process_per_tab
        );
        assert!(run(&["--remote-debugging-pipe", "--process-per-tab"]).process_per_tab);
        let both = parse(&["--single-process", "--process-per-tab"]);
        assert!(
            matches!(&both, Err(e) if e.contains("opposite")),
            "{both:?}"
        );
    }

    #[test]
    fn rusts_allocations_are_mimallocs() {
        const SIZE: usize = 64 << 20;
        let before = mimalloc_committed();
        let block = vec![1u8; SIZE];
        assert!(mimalloc_committed() >= before + SIZE);
        drop(block);
    }

    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    fn run(args: &[&str]) -> Args {
        match parse(args) {
            Ok(Command::Run(args)) => *args,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn defaults() {
        let args = run(&[]);
        assert_eq!(args.url, DEFAULT_URL);
        assert!(args.pipe.is_none());
        assert!(!args.test_fonts);
        assert_eq!(args.num_raster_threads, None);
        assert!(args.process_per_tab);
        assert_eq!(parse(&["x", "--help"]), Ok(Command::Help));
        assert_eq!(parse(&["-h"]), Ok(Command::Help));
    }

    /// `--user-data-dir` names a folder, in a window or with the pipe;
    /// without it a window has the user's own, the pipe one in memory.
    #[test]
    fn the_user_data_dir() {
        assert_eq!(run(&[]).user_data_dir, None);
        let dir = run(&["--user-data-dir", "profile"]).user_data_dir;
        assert_eq!(dir, Some(PathBuf::from("profile")));
        let absolute = std::env::temp_dir().join("p");
        let flag = format!("--user-data-dir={}", absolute.display());
        let piped = run(&["--remote-debugging-pipe", "--headless", &flag]);
        assert_eq!(piped.user_data_dir.as_ref(), Some(&absolute));
        assert!(parse(&["--user-data-dir="]).is_err());
        assert!(parse(&["--user-data-dir"]).is_err());
        assert_eq!(
            profile_dir(&run(&["--remote-debugging-pipe", "--headless"])),
            None
        );
        assert_eq!(profile_dir(&run(&[])), browser::default_user_data_dir());
        // As the working directory has it as machopo starts.
        assert_eq!(
            profile_dir(&run(&["--user-data-dir", "profile"])),
            Some(std::env::current_dir().unwrap().join("profile"))
        );
        assert_eq!(profile_dir(&piped), Some(absolute));
    }

    #[test]
    fn window_options() {
        let args = run(&["--test-fonts", "--num-raster-threads", "3", "about:blank"]);
        assert_eq!(args.url, "about:blank");
        assert!(args.test_fonts);
        assert_eq!(args.num_raster_threads, Some(3));
    }

    /// Chrome's `--name=value`, as well as `--name value`; a switch
    /// without a value takes none.
    #[test]
    fn the_ui_layout_of_a_window() {
        assert_eq!(run(&[]).ui_layout, UiLayout::Horizontal);
        assert_eq!(
            run(&["--ui-layout", "sidebar"]).ui_layout,
            UiLayout::Sidebar
        );
        assert_eq!(
            run(&["--ui-layout=horizontal"]).ui_layout,
            UiLayout::Horizontal
        );
        let err = |args: &[&str]| parse(args).unwrap_err();
        assert!(err(&["--ui-layout", "vertical"]).contains("horizontal or sidebar"));
        // What drives the pipe starts horizontal.
        assert!(
            err(&[
                "--remote-debugging-pipe",
                "--headless",
                "--ui-layout",
                "sidebar"
            ])
            .contains("only applies")
        );
    }

    #[test]
    fn the_ui_theme_of_a_window() {
        assert_eq!(run(&[]).ui_theme, None);
        assert_eq!(run(&["--ui-theme", "dark"]).ui_theme, Some(ColorMode::Dark));
        assert_eq!(run(&["--ui-theme=light"]).ui_theme, Some(ColorMode::Light));
        let err = |args: &[&str]| parse(args).unwrap_err();
        assert!(err(&["--ui-theme", "auto"]).contains("light or dark"));
        // What drives the pipe sees the light UI.
        assert!(
            err(&[
                "--remote-debugging-pipe",
                "--headless",
                "--ui-theme",
                "dark"
            ])
            .contains("only applies")
        );
    }

    #[test]
    fn values_come_after_an_equals_sign_too() {
        let args = run(&["--num-raster-threads=2", "about:blank?a=b"]);
        assert_eq!(args.url, "about:blank?a=b");
        assert_eq!(args.num_raster_threads, Some(2));
        let args = run(&["--remote-debugging-pipe", "--headless", "--scale=1.5"]);
        assert_eq!(args.pipe.map(|pipe| pipe.scale), Some(1.5));
        let err = |args: &[&str]| parse(args).unwrap_err();
        assert!(err(&["--test-fonts=yes"]).contains("--test-fonts takes no value"));
    }

    #[test]
    fn remote_debugging_options() {
        let args = run(&["--remote-debugging-port=0"]);
        assert_eq!(
            args.remote_debugging,
            Some(devtools_http_handler::Options {
                port: 0,
                allowed_origins: AllowedOrigins::None,
            })
        );
        let args = run(&[
            "--remote-debugging-pipe",
            "--headless",
            "--remote-debugging-port",
            "9222",
            "--remote-allow-origins=*",
        ]);
        assert_eq!(
            args.remote_debugging,
            Some(devtools_http_handler::Options {
                port: 9222,
                allowed_origins: AllowedOrigins::Any,
            })
        );
        let err = |args: &[&str]| parse(args).unwrap_err();
        assert!(err(&["--remote-debugging-port=x"]).contains("wants a port"));
        assert!(err(&["--remote-debugging-port=65536"]).contains("wants a port"));
        assert!(
            err(&["--remote-allow-origins=*"])
                .contains("only applies with --remote-debugging-port")
        );
    }

    #[test]
    fn errors_say_what_is_wrong() {
        let err = |args: &[&str]| parse(args).unwrap_err();
        assert!(
            err(&["--remote-debugging-pipe", "--headless", "--scale", "-1"]).contains("positive")
        );
        assert!(
            err(&["--remote-debugging-pipe", "--headless", "--scale"])
                .contains("--scale needs a value")
        );
        for n in ["0", "-1", "two"] {
            assert!(
                err(&["--num-raster-threads", n]).contains("whole number from 1"),
                "{n}"
            );
        }
        // A real window's takes the place of the screen's; the pipe's
        // window without the platform's has its own.
        assert_eq!(run(&["--scale", "2"]).window_scale, Some(2.0));
        let window = run(&["--remote-debugging-pipe", "--scale=1"]);
        assert_eq!(window.window_scale, Some(1.0));
        assert_eq!(
            run(&["--remote-debugging-pipe", "--headless", "--scale", "2"]).window_scale,
            None
        );
        assert_eq!(run(&[]).window_scale, None);
        assert!(err(&["--frobnicate"]).contains("unknown option"));
        // The modes machopo has no more.
        for gone in ["--size", "--automation", "--out"] {
            assert!(err(&[gone, "x"]).contains("unknown option"), "{gone}");
        }
        // The switches Chrome's took the place of say what they are now.
        assert!(err(&["--automation-pipe"]).contains("--remote-debugging-pipe --headless now"));
        assert!(err(&["--window"]).contains("without --headless is the real window"));
        // As Chrome's: no value (`=cbor`, its other format, isn't machopo's).
        assert!(
            err(&["--remote-debugging-pipe=cbor"])
                .contains("--remote-debugging-pipe takes no value")
        );
        assert!(err(&["a.html", "b.html"]).contains("more than one"));
        assert!(err(&["--script", "wasm"]).contains("--script"));
        assert!(
            err(&["--remote-debugging-pipe", "--headless", "about:blank"])
                .contains("driven through the pipe")
        );
        assert!(err(&["--host-resolver-rules", "MAP * 127.0.0.1"]).contains("only apply with"));
        assert!(err(&["--headless"]).contains("only apply with"));
        assert!(err(&["--additional-trust-anchors", "a.pem"]).contains("only apply with"));
        assert!(err(&["--remote-debugging-io-pipes=1,2"]).contains("only apply with"));
        assert!(err(&["--host-resolver-rules"]).contains("needs a value"));
    }

    #[test]
    fn remote_debugging_pipe() {
        let args = run(&[
            "--remote-debugging-pipe",
            "--host-resolver-rules",
            "MAP *.test 127.0.0.1",
            "--additional-trust-anchors",
            "ca.pem",
            "--test-fonts",
            "--remote-debugging-io-pipes=12,34",
        ]);
        assert_eq!(
            args.pipe,
            Some(PipeArgs {
                headless: false,
                scale: 1.0,
                host_resolver_rules: Some("MAP *.test 127.0.0.1".into()),
                additional_trust_anchors: Some("ca.pem".into()),
                io_pipes: Some("12,34".into()),
            })
        );
        assert!(args.test_fonts);
        assert_eq!(
            run(&["--remote-debugging-pipe", "--headless"]).pipe,
            Some(PipeArgs::default())
        );
        assert_eq!(
            run(&["--remote-debugging-pipe", "--headless", "--scale", "2"]).pipe,
            Some(PipeArgs {
                scale: 2.0,
                ..Default::default()
            })
        );
        assert_eq!(run(&[]).pipe, None);
        // The rules are checked, and the certificates read, before the
        // browser starts.
        let bad = PipeArgs {
            host_resolver_rules: Some("FLY *".into()),
            ..Default::default()
        };
        assert!(
            network_params(&bad)
                .unwrap_err()
                .starts_with("--host-resolver-rules: ")
        );
        let missing = PipeArgs {
            additional_trust_anchors: Some("no-such.pem".into()),
            ..Default::default()
        };
        assert!(
            network_params(&missing)
                .unwrap_err()
                .starts_with("no-such.pem: ")
        );
    }

    #[test]
    fn help_mentions_every_option() {
        let help = help();
        for option in [
            "--scale",
            "--test-fonts",
            "--remote-debugging-pipe",
            "--headless",
            "--remote-debugging-io-pipes",
            "--host-resolver-rules",
            "--additional-trust-anchors",
            "--num-raster-threads",
            "--user-data-dir",
            "--help",
        ] {
            assert!(help.contains(option), "{option}");
        }
        for var in [
            "RUST_LOG",
            "MACHOPO_V8_FLAGS",
            "MACHOPO_V8_THREAD_POOL_SIZE",
            "MACHOPO_GC_OPTIONS",
            "MACHOPO_TRACE",
            "MIMALLOC_PURGE_DELAY",
            "MACHOPO_GPU_ADAPTER",
        ] {
            assert!(help.contains(var), "{var}");
        }
    }
}
