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
use gfx::Size;
use shell::automation::Context;
use shell::devtools_http_handler::{self, AllowedOrigins};
use shell::{automation_pipe, headless, scenario, to_url};
use tracing_log::AsLog;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const DEFAULT_URL: &str = "about:demo";
const DEFAULT_SIZE: Size = Size::new(800, 600);
// Rust's allocations in machopo (the nodes' data, the parser's, style's) go
// through mimalloc, as Chromium's go through PartitionAlloc (PartitionAlloc-
// Everywhere); C++'s (V8, Skia) stay with the platform's malloc, so memory
// one side allocates the other must never free. Only this executable: the
// other crates' tests keep the platform's allocator.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

const DEFAULT_LOG_FILTER: &str = "info,wgpu_core=warn,wgpu_hal=warn,naga=warn";

const USAGE: &str = "\
usage: machopo [--test-fonts] [--num-raster-threads N]
           [--remote-debugging-port PORT [--remote-allow-origins ORIGINS]] [URL|PATH]
       machopo --headless OUT.png [--size WxH] [--scale F] [--test-fonts]
           [--num-raster-threads N] [URL|PATH]
       machopo --automation SCENARIO [--out DIR] [--test-fonts]
           [--num-raster-threads N]
       machopo --automation-pipe [--window | --scale F] [--host-resolver-rules RULES]
           [--additional-trust-anchors PEM] [--test-fonts]
           [--num-raster-threads N] [--remote-debugging-port PORT
           [--remote-allow-origins ORIGINS]]

Options that take a value take it as --name VALUE or --name=VALUE.";

/// `machopo --help`. Keep it in step with `parse_args` and the environment
/// variables `bindings` and `main` read.
fn help() -> String {
    format!(
        "\
machopo: a Chromium-like browser in Rust.

{USAGE}

Opens URL or the file at PATH in a window, or with --headless renders it to
a PNG once it and everything it loads (stylesheets, images) have loaded or
failed. URLs: http://..., https://...,
file://..., about:blank, about:demo (the built-in demo page, the default).
Anything else is taken as a path; a relative one is resolved against the
working directory.

Options:
  --headless OUT.png  Render one frame to OUT.png instead of opening a window.
  --size WxH          Headless only: the viewport in CSS px (default {w}x{h}).
  --scale F           With --headless or --automation-pipe (but not --window):
                      the device scale factor (default 1), as Chromium's
                      --force-device-scale-factor. The headless PNG and the
                      pipe's screenshots are the viewport times F, in
                      physical pixels.
  --test-fonts        Use only the built-in test fonts: Ahem, and Lato for
                      every generic family, as the reftests do.
  --run-web-tests     Run testharness.js tests headless, as content_shell's
                      --run-web-tests (for a driver: shell's tests drive it):
                      each line of standard input is a test, its timeout in
                      milliseconds and its URL; each test's record is a line
                      of JSON on standard output (what the page reported to
                      testRunner, or a timeout). A line `screenshot TIMEOUT
                      URL PATH` is a reftest's page instead: its
                      screenshot, once reftest-wait is off, written to PATH
                      as a PNG. With --host-resolver-rules.
  --num-raster-threads N
                      Threads each page's tiles are rastered on at once, as
                      Chromium's switch (default 1: on the compositor's
                      thread). More raster a page of many tiles faster
                      (resizing it: about 4 times on 9 threads), but take
                      CPU and memory from the page while it runs; here,
                      {threads} would leave a processor to its main thread.
  --automation SCENARIO
                      Run the scenario in the file SCENARIO (the format is in
                      tests/scenarios/README.md) in a window, which stays on
                      top and ignores the mouse and keyboard meanwhile, then
                      close it. The exit status is 0 if every check passed,
                      1 if one failed (it says which), 2 if the scenario or
                      the options are wrong. Only the system's keyboard
                      shortcuts' `platform` blocks run.
  --out DIR           Automation only: where the scenario's screenshots go
                      (none are saved without it).
  --automation-pipe   Be driven through standard input and output, by machopo's
                      WebDriver driver (machopodriver): DevTools-protocol-shaped
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
        w = DEFAULT_SIZE.width,
        h = DEFAULT_SIZE.height,
        threads =
            std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1).max(1)),
    )
}

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Run(Box<Args>),
    RunWebTests(WebTests),
}

/// `--run-web-tests`, and the settings that go with it.
#[derive(Debug, PartialEq)]
struct WebTests {
    host_resolver_rules: Option<String>,
}

#[derive(Debug, PartialEq)]
struct Args {
    url: String,
    test_fonts: bool,
    headless: Option<headless::Options>,
    automation: Option<Automation>,
    automation_pipe: Option<AutomationPipe>,
    /// `--num-raster-threads`; `None`: the default.
    num_raster_threads: Option<usize>,
    /// `--remote-debugging-port` and `--remote-allow-origins`.
    remote_debugging: Option<devtools_http_handler::Options>,
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

/// `--automation`: the scenario's file, and where its screenshots go.
#[derive(Debug, PartialEq)]
struct Automation {
    scenario: PathBuf,
    out: Option<PathBuf>,
}

/// Parses the arguments after the program name. Errors say what was wrong.
fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut url = None;
    let mut out = None;
    let mut size = None;
    let mut scale = None;
    let mut test_fonts = false;
    let mut run_web_tests = false;
    let mut automation = None;
    let mut automation_out = None;
    let mut automation_pipe = false;
    let mut pipe_window = false;
    let mut host_resolver_rules = None;
    let mut additional_trust_anchors = None;
    let mut num_raster_threads = None;
    let mut remote_debugging_port = None;
    let mut remote_allow_origins = None;
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
            "--headless" => out = Some(value("--headless")?),
            "--size" => {
                let v = value("--size")?;
                let parsed = v.split_once('x').and_then(|(w, h)| {
                    Some(Size::new(w.parse().ok()?, h.parse().ok()?))
                        .filter(|s| s.width > 0 && s.height > 0)
                });
                size = Some(parsed.ok_or_else(|| {
                    format!("--size wants WIDTHxHEIGHT in whole CSS px, like 800x600, not {v:?}")
                })?);
            }
            "--scale" => {
                let v = value("--scale")?;
                let parsed = v.parse::<f32>().ok().filter(|f| f.is_finite() && *f > 0.0);
                scale = Some(parsed.ok_or_else(|| {
                    format!("--scale wants a positive number, like 2, not {v:?}")
                })?);
            }
            "--test-fonts" => test_fonts = true,
            "--num-raster-threads" => {
                let v = value("--num-raster-threads")?;
                let parsed = v.parse::<usize>().ok().filter(|n| *n > 0);
                num_raster_threads = Some(parsed.ok_or_else(|| {
                    format!("--num-raster-threads wants a whole number from 1, like 4, not {v:?}")
                })?);
            }
            "--automation" => automation = Some(PathBuf::from(value("--automation")?)),
            "--out" => automation_out = Some(PathBuf::from(value("--out")?)),
            "--automation-pipe" => automation_pipe = true,
            "--window" => pipe_window = true,
            "--host-resolver-rules" => host_resolver_rules = Some(value("--host-resolver-rules")?),
            "--additional-trust-anchors" => {
                additional_trust_anchors =
                    Some(PathBuf::from(value("--additional-trust-anchors")?));
            }
            "--run-web-tests" => run_web_tests = true,
            "--remote-debugging-port" => {
                let v = value("--remote-debugging-port")?;
                remote_debugging_port = Some(v.parse::<u16>().map_err(|_| {
                    format!("--remote-debugging-port wants a port, 0 to 65535, not {v:?}")
                })?);
            }
            "--remote-allow-origins" => {
                remote_allow_origins = Some(value("--remote-allow-origins")?);
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
    if out.is_none() && size.is_some() {
        return Err(
            "--size only applies with --headless; a window takes its size from the screen".into(),
        );
    }
    if out.is_none() && scale.is_some() && (!automation_pipe || pipe_window) {
        return Err(
            "--scale only applies with --headless or --automation-pipe; a window takes its \
             scale from the screen"
                .into(),
        );
    }
    if automation.is_some() && (out.is_some() || url.is_some()) {
        return Err(
            "--automation runs a scenario, which navigates itself: no --headless, URL or path"
                .into(),
        );
    }
    if automation.is_none() && automation_out.is_some() {
        return Err("--out only applies with --automation".into());
    }
    if automation_pipe && (out.is_some() || automation.is_some() || url.is_some()) {
        return Err(
            "--automation-pipe is driven through the pipe, which navigates: no --headless, \
             --automation, URL or path"
                .into(),
        );
    }
    if run_web_tests && (out.is_some() || automation.is_some() || automation_pipe || url.is_some())
    {
        return Err(
            "--run-web-tests reads its tests from standard input: no --headless, --automation, \
             --automation-pipe, URL or path"
                .into(),
        );
    }
    if run_web_tests {
        if pipe_window || additional_trust_anchors.is_some() {
            return Err(
                "--window and --additional-trust-anchors only apply with --automation-pipe".into(),
            );
        }
        return Ok(Command::RunWebTests(WebTests {
            host_resolver_rules,
        }));
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
    if remote_debugging_port.is_some() && (out.is_some() || automation.is_some()) {
        return Err(
            "--remote-debugging-port applies to a window or --automation-pipe: no --headless \
             or --automation"
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
    let size = size.unwrap_or(DEFAULT_SIZE);
    let scale = scale.unwrap_or(1.0);
    Ok(Command::Run(Box::new(Args {
        url: url.map_or_else(|| DEFAULT_URL.to_owned(), |arg| to_url(&arg)),
        headless: out.map(|out| headless::Options {
            out: out.into(),
            // `--size` is in CSS px; the renderer works in physical px.
            viewport_size: size.scale_to_ceiled(scale),
            device_scale_factor: scale,
            test_fonts,
            num_raster_threads,
        }),
        test_fonts,
        num_raster_threads,
        remote_debugging,
        automation: automation.map(|scenario| Automation {
            scenario,
            out: automation_out,
        }),
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

/// `--run-web-tests`: the tests standard input lists, a record each on
/// standard output (`shell::web_tests`); the log goes to standard error.
fn run_web_tests(tests: &WebTests) -> ExitCode {
    let mut network = NetworkContextParams::default();
    if let Some(rules) = &tests.host_resolver_rules {
        match HostMappingRules::parse(rules) {
            Ok(rules) => network.host_resolver_rules = rules,
            Err(e) => {
                eprintln!("machopo: --host-resolver-rules: {e}");
                return ExitCode::from(2);
            }
        }
    }
    let stdin = std::io::stdin();
    match shell::web_tests::run(&network, stdin.lock(), std::io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("machopo: --run-web-tests: {e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let (args, web_tests) = match parse_args(std::env::args().skip(1)) {
        Ok(Command::Run(args)) => (Some(*args), None),
        Ok(Command::Help) => {
            println!("{}", help());
            return ExitCode::SUCCESS;
        }
        Ok(Command::RunWebTests(tests)) => (None, Some(tests)),
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
    let args = match (args, web_tests) {
        (Some(args), _) => args,
        (None, Some(tests)) => return run_web_tests(&tests),
        (None, None) => unreachable!("a command"),
    };

    // A scenario that can't run is the caller's mistake, as wrong options
    // are: said before any window opens.
    let automation = match args.automation.as_ref().map(scenario_to_run) {
        Some(Err(e)) => {
            eprintln!("machopo: {e}");
            return ExitCode::from(2);
        }
        Some(Ok(automation)) => Some(automation),
        None => None,
    };

    let result = match (args.headless, &args.automation_pipe) {
        (Some(opts), _) => headless::run(&args.url, opts),
        (None, Some(pipe)) => match network_params(pipe) {
            Ok(network) if pipe.window => window::run(
                args.url,
                args.test_fonts,
                args.num_raster_threads,
                Some(window::Driver::Pipe(network)),
                args.remote_debugging,
            ),
            Ok(network) => automation_pipe::run(automation_pipe::Options {
                scale: pipe.scale,
                test_fonts: args.test_fonts,
                network,
                num_raster_threads: args.num_raster_threads,
                remote_debugging: args.remote_debugging,
                ..Default::default()
            }),
            Err(e) => {
                eprintln!("machopo: {e}");
                return ExitCode::from(2);
            }
        },
        (None, None) => window::run(
            args.url,
            args.test_fonts,
            args.num_raster_threads,
            automation.map(window::Driver::Scenario),
            args.remote_debugging,
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// The scenario `--automation` names, read and parsed, to run with the
/// system's keyboard shortcuts.
fn scenario_to_run(automation: &Automation) -> Result<window::Automation, String> {
    let path = &automation.scenario;
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let scenario = scenario::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(window::Automation {
        scenario,
        context: Context {
            base: path.parent().map(PathBuf::from).unwrap_or_default(),
            out: automation.out.clone(),
            mac: cfg!(target_os = "macos"),
        },
    })
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
        assert!(args.headless.is_none());
        assert!(!args.test_fonts);
        assert_eq!(parse(&["x", "--help"]), Ok(Command::Help));
        assert_eq!(parse(&["-h"]), Ok(Command::Help));
    }

    #[test]
    fn headless_options() {
        let args = run(&[
            "--headless",
            "out.png",
            "--size",
            "100x50",
            "--scale",
            "1.5",
            "--test-fonts",
            "--num-raster-threads",
            "3",
            "about:blank",
        ]);
        assert_eq!(args.url, "about:blank");
        assert_eq!(args.num_raster_threads, Some(3));
        assert_eq!(
            args.headless,
            Some(headless::Options {
                out: "out.png".into(),
                viewport_size: Size::new(150, 75),
                device_scale_factor: 1.5,
                test_fonts: true,
                num_raster_threads: Some(3),
            })
        );
        let default = run(&["--headless", "out.png"]);
        assert_eq!(default.num_raster_threads, None);
        let default = default.headless.unwrap();
        assert_eq!(default.viewport_size, DEFAULT_SIZE);
        assert_eq!(default.num_raster_threads, None);
    }

    /// Chrome's `--name=value`, as well as `--name value`; a switch
    /// without a value takes none.
    #[test]
    fn values_come_after_an_equals_sign_too() {
        let args = run(&["--headless=out.png", "--size=100x50", "about:blank?a=b"]);
        assert_eq!(args.url, "about:blank?a=b");
        let headless = args.headless.expect("headless");
        assert_eq!(headless.out, std::path::PathBuf::from("out.png"));
        assert_eq!(headless.viewport_size, Size::new(100, 50));
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
        assert!(
            err(&["--headless", "o.png", "--remote-debugging-port=0"])
                .contains("no --headless or --automation")
        );
    }

    #[test]
    fn errors_say_what_is_wrong() {
        let err = |args: &[&str]| parse(args).unwrap_err();
        assert!(err(&["--headless"]).contains("--headless needs a value"));
        assert!(err(&["--headless", "o.png", "--size", "800"]).contains("WIDTHxHEIGHT"));
        assert!(err(&["--headless", "o.png", "--size", "0x10"]).contains("WIDTHxHEIGHT"));
        assert!(err(&["--headless", "o.png", "--scale", "-1"]).contains("positive"));
        for n in ["0", "-1", "two"] {
            assert!(
                err(&["--num-raster-threads", n]).contains("whole number from 1"),
                "{n}"
            );
        }
        assert!(err(&["--size", "10x10"]).contains("only applies with --headless"));
        for args in [
            &["--scale", "2"][..],
            &["--automation-pipe", "--window", "--scale", "2"],
        ] {
            assert!(
                err(args).contains("only applies with --headless or --automation-pipe"),
                "{args:?}"
            );
        }
        assert!(err(&["--frobnicate"]).contains("unknown option"));
        assert!(err(&["a.html", "b.html"]).contains("more than one"));
        assert!(err(&["--script", "wasm"]).contains("--script"));
        assert!(err(&["--automation"]).contains("--automation needs a value"));
        assert!(err(&["--automation", "s.txt", "about:blank"]).contains("navigates itself"));
        assert!(
            err(&["--automation", "s.txt", "--headless", "o.png"]).contains("navigates itself")
        );
        assert!(err(&["--out", "shots"]).contains("only applies with --automation"));
        for args in [
            &["--automation-pipe", "about:blank"][..],
            &["--automation-pipe", "--headless", "o.png"],
            &["--automation-pipe", "--automation", "s.txt"],
        ] {
            assert!(err(args).contains("driven through the pipe"), "{args:?}");
        }
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
    fn automation() {
        let args = run(&["--automation", "tests/s.txt", "--out", "shots"]);
        assert_eq!(
            args.automation,
            Some(Automation {
                scenario: "tests/s.txt".into(),
                out: Some("shots".into()),
            })
        );
        assert_eq!(
            run(&["--automation", "s.txt"]).automation.unwrap().out,
            None
        );
        assert_eq!(run(&[]).automation, None);
    }

    /// The scenario is read and parsed before any window opens; relative
    /// paths in it are its file's.
    #[test]
    fn the_scenario_to_run() {
        let dir = std::env::temp_dir().join(format!("machopo-main-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("good.txt"), "window size=300x200\nwait-loaded").unwrap();
        std::fs::write(dir.join("bad.txt"), "wait-loaded\nfly").unwrap();
        let automation = |name: &str| Automation {
            scenario: dir.join(name),
            out: Some("shots".into()),
        };
        let good = scenario_to_run(&automation("good.txt")).unwrap();
        assert_eq!(good.scenario.window.size, Some((300.0, 200.0)));
        assert_eq!(good.scenario.steps.len(), 1);
        assert_eq!(good.context.base, dir);
        assert_eq!(good.context.out, Some("shots".into()));
        assert_eq!(good.context.mac, cfg!(target_os = "macos"));
        let Err(bad) = scenario_to_run(&automation("bad.txt")) else {
            panic!("bad.txt parses");
        };
        assert!(bad.ends_with("line 2: unknown command `fly`"), "{bad}");
        assert!(scenario_to_run(&automation("missing.txt")).is_err());
    }

    /// `--run-web-tests` reads its tests from standard input: with
    /// `--host-resolver-rules`, no URL, headless,
    /// automation or window.
    #[test]
    fn run_web_tests_reads_its_tests() {
        let tests = |args: &[&str]| match parse(args) {
            Ok(Command::RunWebTests(tests)) => tests,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            tests(&["--run-web-tests"]),
            WebTests {
                host_resolver_rules: None
            }
        );
        assert_eq!(
            tests(&[
                "--run-web-tests",
                "--host-resolver-rules",
                "MAP *.test 127.0.0.1"
            ]),
            WebTests {
                host_resolver_rules: Some("MAP *.test 127.0.0.1".into())
            }
        );
        for args in [
            &["--run-web-tests", "about:blank"][..],
            &["--run-web-tests", "--headless", "o.png"],
            &["--run-web-tests", "--automation-pipe"],
            &["--run-web-tests", "--window"],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn help_mentions_every_option() {
        let help = help();
        for option in [
            "--headless",
            "--size",
            "--scale",
            "--test-fonts",
            "--automation",
            "--out",
            "--automation-pipe",
            "--window",
            "--host-resolver-rules",
            "--additional-trust-anchors",
            "--num-raster-threads",
            "--run-web-tests",
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
