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
use shell::chrome::UiLayout;
use shell::devtools_http_handler::{self, AllowedOrigins};
use shell::theme::ColorMode;
use shell::{automation_pipe, to_url};
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
usage: machopo [--ui-layout LAYOUT] [--ui-theme THEME] [--test-fonts]
           [--num-raster-threads N] [--user-data-dir DIR]
           [--remote-debugging-port PORT [--remote-allow-origins ORIGINS]]
           [URL|PATH]
       machopo --automation-pipe [--window | --scale F] [--host-resolver-rules RULES]
           [--additional-trust-anchors PEM] [--test-fonts]
           [--num-raster-threads N] [--user-data-dir DIR]
           [--remote-debugging-port PORT [--remote-allow-origins ORIGINS]]

Options that take a value take it as --name VALUE or --name=VALUE.";

/// `machopo --help`. Keep it in step with `parse_args` and the environment
/// variables `bindings` and `main` read.
fn help() -> String {
    format!(
        "\
machopo: a Chromium-like browser in Rust.

{USAGE}

Opens URL or the file at PATH in a window, or with --automation-pipe is
driven by what reads and writes its standard input and output. URLs:
http://..., https://..., file://..., about:blank, about:demo (the built-in
demo page, the default). Anything else is taken as a path; a relative one
is resolved against the working directory.

Options:
  --ui-layout LAYOUT  In a window (not with --automation-pipe): how the UI is
                      laid out at first, horizontal (the default: the tabs
                      above the toolbar) or sidebar (the toolbar and the
                      tabs down the left). Cmd+Shift+, (Ctrl+Shift+,
                      elsewhere) switches between them.
  --ui-theme THEME    In a window (not with --automation-pipe): light or dark,
                      the UI's colors (not the pages'). By default the
                      system's, changing as it does.
  --scale F           With --automation-pipe (but not --window): the device
                      scale factor (default 1), as Chromium's
                      --force-device-scale-factor. The pipe's screenshots
                      are the viewport times F, in physical pixels.
  --test-fonts        Use only the built-in test fonts: Ahem, and Lato for
                      every generic family, as the reftests do.
  --num-raster-threads N
                      Threads each page's tiles are rastered on at once, as
                      Chromium's switch (default 1: on the compositor's
                      thread). More raster a page of many tiles faster
                      (resizing it: about 4 times on 9 threads), but take
                      CPU and memory from the page while it runs; here,
                      {threads} would leave a processor to its main thread.
  --automation-pipe   Be driven through standard input and output, by machopo's
                      WebDriver driver (machopodriver) or the scenario
                      tests' runner: DevTools-protocol-shaped
                      JSON messages, each followed by a NUL (as Chromium's
                      --remote-debugging-pipe). The window is a window
                      without the platform's: nothing shows (but with
                      --window). Ends when the input does. Logs go to
                      standard error, as always.
  --window            With --automation-pipe: a real window, which stays on
                      top and ignores the mouse and keyboard meanwhile.
  --host-resolver-rules RULES
                      With --automation-pipe: hosts that resolve to a fixed
                      address, or not at all, as Chromium's switch; web
                      tests map WPT's domains to this machine with
                      \"MAP nonexistent.*.test ^NOTFOUND, MAP *.test 127.0.0.1\".
  --additional-trust-anchors PEM
                      With --automation-pipe: trust the root certificates in
                      the PEM file too, as web tests do WPT's.
  --user-data-dir DIR Keep the profile (what pages store) in the folder DIR,
                      made if it isn't there, as Chrome's switch. Without
                      it a window's is the user's own
                      ({default_profile}), and
                      --automation-pipe's is in memory, gone as it ends.
                      Only one machopo has a folder at a time: a second one
                      doesn't start.
  --remote-debugging-port PORT
                      In a window or with --automation-pipe: serve the
                      DevTools protocol on 127.0.0.1:PORT (0: a free port),
                      as Chrome's switch, for Chrome's DevTools
                      (chrome://inspect) and other CDP clients: /json/list
                      lists the tabs, and websockets connect to the
                      browser or straight to a tab. Where it listens goes
                      to standard error. The methods are the pipe's; its
                      Machopo domain only with --automation-pipe.
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
                           blink.user_timing.
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
    automation_pipe: Option<AutomationPipe>,
    /// `--num-raster-threads`; `None`: the default.
    num_raster_threads: Option<usize>,
    /// `--remote-debugging-port` and `--remote-allow-origins`.
    remote_debugging: Option<devtools_http_handler::Options>,
    /// `--user-data-dir`.
    user_data_dir: Option<PathBuf>,
}

/// `--automation-pipe`, and the settings that go with it.
#[derive(Debug, PartialEq)]
struct AutomationPipe {
    /// `--window`: the real window.
    window: bool,
    /// `--scale`: the device scale factor of the window without the
    /// platform's (a real window takes the screen's).
    scale: f32,
    host_resolver_rules: Option<String>,
    additional_trust_anchors: Option<PathBuf>,
}

impl Default for AutomationPipe {
    /// No window, at 1x, the network as it is.
    fn default() -> Self {
        Self {
            window: false,
            scale: 1.0,
            host_resolver_rules: None,
            additional_trust_anchors: None,
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
    let mut automation_pipe = false;
    let mut pipe_window = false;
    let mut host_resolver_rules = None;
    let mut additional_trust_anchors = None;
    let mut num_raster_threads = None;
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
            "--automation-pipe" => automation_pipe = true,
            "--window" => pipe_window = true,
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
    if scale.is_some() && (!automation_pipe || pipe_window) {
        return Err(
            "--scale only applies with --automation-pipe; a window takes its scale from the \
             screen"
                .into(),
        );
    }
    if automation_pipe && ui_layout.is_some() {
        return Err(
            "--ui-layout only applies to a window: what drives the pipe starts horizontal, so \
             that the page's size doesn't change with it"
                .into(),
        );
    }
    if automation_pipe && ui_theme.is_some() {
        return Err(
            "--ui-theme only applies to a window: what drives the pipe sees the light UI, so \
             that its screenshots don't change with the machine"
                .into(),
        );
    }
    if automation_pipe && url.is_some() {
        return Err(
            "--automation-pipe is driven through the pipe, which navigates: no URL or path".into(),
        );
    }
    if !automation_pipe
        && (pipe_window || host_resolver_rules.is_some() || additional_trust_anchors.is_some())
    {
        return Err(
            "--window, --host-resolver-rules and --additional-trust-anchors only apply with \
             --automation-pipe"
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
    let scale = scale.unwrap_or(1.0);
    Ok(Command::Run(Box::new(Args {
        url: url.map_or_else(|| DEFAULT_URL.to_owned(), |arg| to_url(&arg)),
        ui_layout: ui_layout.unwrap_or_default(),
        ui_theme,
        test_fonts,
        num_raster_threads,
        remote_debugging,
        user_data_dir,
        automation_pipe: automation_pipe.then_some(AutomationPipe {
            window: pipe_window,
            scale,
            host_resolver_rules,
            additional_trust_anchors,
        }),
    })))
}

/// The network service's settings `--automation-pipe`'s options ask for:
/// the rules parsed, the certificates read.
fn network_params(pipe: &AutomationPipe) -> Result<NetworkContextParams, String> {
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
        None if args.automation_pipe.is_some() => None,
        None => browser::default_user_data_dir(),
    }
}

/// The profile `args` ask for (`profile_dir`), its folder opened.
fn profile(args: &Args) -> Result<browser::Profile, browser::ProfileError> {
    match profile_dir(args) {
        Some(dir) => browser::Profile::open(dir),
        None => {
            if args.automation_pipe.is_none() {
                tracing::warn!(
                    "the environment doesn't say where the user's files are: nothing pages store \
                     is kept (--user-data-dir says where)"
                );
            }
            Ok(browser::Profile::in_memory())
        }
    }
}

fn main() -> ExitCode {
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

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| DEFAULT_LOG_FILTER.into());
    let log_records = filter
        .max_level_hint()
        .unwrap_or(LevelFilter::TRACE)
        .as_log();
    let log = tracing_subscriber::fmt::layer()
        .with_thread_names(true)
        // Standard output is `--automation-pipe`'s.
        .with_writer(std::io::stderr)
        .with_filter(filter);
    // The trace's end is written when `_trace_end` drops, as `main` returns.
    // Before anything starts V8, which records its events only if the
    // trace is up by then.
    let (trace, _trace_end) = match std::env::var_os("MACHOPO_TRACE") {
        Some(file) => match shell::trace::layer(Path::new(&file)) {
            Ok((layer, end)) => (Some(layer), Some(end)),
            Err(e) => {
                eprintln!(
                    "machopo: MACHOPO_TRACE: can't trace into {}: {e:?}",
                    Path::new(&file).display()
                );
                (None, None)
            }
        },
        None => (None, None),
    };
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
    let result = match &args.automation_pipe {
        Some(pipe) => match network_params(pipe) {
            Ok(network) if pipe.window => window::run(window::Options {
                url: args.url,
                ui_layout: UiLayout::Horizontal,
                ui_theme: Some(ColorMode::Light),
                test_fonts: args.test_fonts,
                num_raster_threads: args.num_raster_threads,
                pipe: Some(network),
                remote_debugging: args.remote_debugging,
                profile,
            }),
            Ok(network) => automation_pipe::run(automation_pipe::Options {
                scale: pipe.scale,
                test_fonts: args.test_fonts,
                network,
                num_raster_threads: args.num_raster_threads,
                remote_debugging: args.remote_debugging,
                profile,
                ..Default::default()
            }),
            Err(e) => {
                eprintln!("machopo: {e}");
                return ExitCode::from(2);
            }
        },
        None => window::run(window::Options {
            url: args.url,
            ui_layout: args.ui_layout,
            ui_theme: args.ui_theme,
            test_fonts: args.test_fonts,
            num_raster_threads: args.num_raster_threads,
            pipe: None,
            remote_debugging: args.remote_debugging,
            profile,
        }),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
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
        assert!(args.automation_pipe.is_none());
        assert!(!args.test_fonts);
        assert_eq!(args.num_raster_threads, None);
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
        let piped = run(&["--automation-pipe", &flag]);
        assert_eq!(piped.user_data_dir.as_ref(), Some(&absolute));
        assert!(parse(&["--user-data-dir="]).is_err());
        assert!(parse(&["--user-data-dir"]).is_err());
        assert_eq!(profile_dir(&run(&["--automation-pipe"])), None);
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
        assert!(err(&["--automation-pipe", "--ui-layout", "sidebar"]).contains("only applies"));
    }

    #[test]
    fn the_ui_theme_of_a_window() {
        assert_eq!(run(&[]).ui_theme, None);
        assert_eq!(run(&["--ui-theme", "dark"]).ui_theme, Some(ColorMode::Dark));
        assert_eq!(run(&["--ui-theme=light"]).ui_theme, Some(ColorMode::Light));
        let err = |args: &[&str]| parse(args).unwrap_err();
        assert!(err(&["--ui-theme", "auto"]).contains("light or dark"));
        // What drives the pipe sees the light UI.
        assert!(err(&["--automation-pipe", "--ui-theme", "dark"]).contains("only applies"));
    }

    #[test]
    fn values_come_after_an_equals_sign_too() {
        let args = run(&["--num-raster-threads=2", "about:blank?a=b"]);
        assert_eq!(args.url, "about:blank?a=b");
        assert_eq!(args.num_raster_threads, Some(2));
        let args = run(&["--automation-pipe", "--scale=1.5"]);
        assert_eq!(args.automation_pipe.map(|pipe| pipe.scale), Some(1.5));
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
            "--automation-pipe",
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
        assert!(err(&["--automation-pipe", "--scale", "-1"]).contains("positive"));
        assert!(err(&["--automation-pipe", "--scale"]).contains("--scale needs a value"));
        for n in ["0", "-1", "two"] {
            assert!(
                err(&["--num-raster-threads", n]).contains("whole number from 1"),
                "{n}"
            );
        }
        for args in [
            &["--scale", "2"][..],
            &["--automation-pipe", "--window", "--scale", "2"],
        ] {
            assert!(
                err(args).contains("only applies with --automation-pipe"),
                "{args:?}"
            );
        }
        assert!(err(&["--frobnicate"]).contains("unknown option"));
        // The modes machopo has no more.
        for gone in ["--headless", "--size", "--automation", "--out"] {
            assert!(err(&[gone, "x"]).contains("unknown option"), "{gone}");
        }
        assert!(err(&["a.html", "b.html"]).contains("more than one"));
        assert!(err(&["--script", "wasm"]).contains("--script"));
        assert!(err(&["--automation-pipe", "about:blank"]).contains("driven through the pipe"));
        assert!(err(&["--host-resolver-rules", "MAP * 127.0.0.1"]).contains("only apply with"));
        assert!(err(&["--window"]).contains("only apply with"));
        assert!(err(&["--additional-trust-anchors", "a.pem"]).contains("only apply with"));
        assert!(err(&["--host-resolver-rules"]).contains("needs a value"));
    }

    #[test]
    fn automation_pipe() {
        let args = run(&[
            "--automation-pipe",
            "--host-resolver-rules",
            "MAP *.test 127.0.0.1",
            "--additional-trust-anchors",
            "ca.pem",
            "--test-fonts",
            "--window",
        ]);
        assert_eq!(
            args.automation_pipe,
            Some(AutomationPipe {
                window: true,
                scale: 1.0,
                host_resolver_rules: Some("MAP *.test 127.0.0.1".into()),
                additional_trust_anchors: Some("ca.pem".into()),
            })
        );
        assert!(args.test_fonts);
        assert_eq!(
            run(&["--automation-pipe"]).automation_pipe,
            Some(AutomationPipe::default())
        );
        assert_eq!(
            run(&["--automation-pipe", "--scale", "2"]).automation_pipe,
            Some(AutomationPipe {
                scale: 2.0,
                ..Default::default()
            })
        );
        assert_eq!(run(&[]).automation_pipe, None);
        // The rules are checked, and the certificates read, before the
        // browser starts.
        let bad = AutomationPipe {
            host_resolver_rules: Some("FLY *".into()),
            ..Default::default()
        };
        assert!(
            network_params(&bad)
                .unwrap_err()
                .starts_with("--host-resolver-rules: ")
        );
        let missing = AutomationPipe {
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
            "--automation-pipe",
            "--window",
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
