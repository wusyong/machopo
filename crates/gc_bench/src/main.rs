//! `gc_bench`: runs the GC microbenchmarks of the `gc_bench` crate, one
//! line of JSON per workload. `bench.py gc` runs it and reads the lines,
//! the trace and V8's `--trace-gc-nvp` lines.

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

use std::fs::File;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use gc_bench::{Heap, Workload};

// As `machopo`'s: Rust's allocations (the walks' vectors) go through mimalloc, C++'s (V8's) through the platform's.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

const USAGE: &str = "\
usage: gc_bench [--heap gc|gc-parallel|v8] [--workloads NAME,...]
                [--scale X] [--trace FILE] [--report FILE] [--v8-flags FLAGS]

Runs each workload on a heap of its own, of the kind given (default gc:
the new GC's, marking on one thread; gc-parallel: the new GC's, marking
on a worker for each core but one too; v8: V8's own, as JS objects, for
the patterns only), at X times its size (default 1), and prints a line
of JSON for each, or writes them to --report's FILE. The workloads:
build, churn, replace, list (a DOM's, on a tree), ids (elise's id
table), and the patterns chain, tree and graph, each -high or -low
(default: all the heap runs). With --trace, a Perfetto trace of the run goes to FILE: the GC's
phases, and the spans gc_bench.<workload> and gc_bench.forced_gc around
each workload's turns and its forced GC. --v8-flags go to V8 after
--expose-gc (e.g. --trace-gc-nvp, which V8 prints on stdout: hence
--report).";

struct Options {
    heap: Heap,
    workloads: Option<Vec<Workload>>,
    scale: f64,
    trace: Option<PathBuf>,
    report: Option<PathBuf>,
    v8_flags: String,
}

fn options(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options {
        heap: Heap::Gc,
        workloads: None,
        scale: 1.0,
        trace: None,
        report: None,
        v8_flags: String::new(),
    };
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--heap" => options.heap = value()?.parse()?,
            "--workloads" => {
                options.workloads = Some(
                    value()?
                        .split(',')
                        .map(str::parse)
                        .collect::<Result<_, _>>()?,
                );
            }
            "--scale" => {
                options.scale = value()?
                    .parse()
                    .ok()
                    .filter(|scale: &f64| *scale > 0.0)
                    .ok_or("--scale takes a number above 0")?;
            }
            "--trace" => options.trace = Some(value()?.into()),
            "--report" => options.report = Some(value()?.into()),
            "--v8-flags" => options.v8_flags = value()?,
            "--help" | "-h" => return Err(String::new()),
            _ => return Err(format!("unknown argument {arg:?}")),
        }
    }
    if let Some(workload) = options
        .workloads
        .iter()
        .flatten()
        .find(|workload| !options.heap.runs(**workload))
    {
        return Err(format!(
            "{} doesn't run on the {} heap",
            workload.name(),
            options.heap.name()
        ));
    }
    Ok(options)
}

fn usage(problem: &str) -> ExitCode {
    if !problem.is_empty() {
        eprintln!("gc_bench: {problem}");
    }
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

/// Starts the trace and V8 as `options` say, and opens the report.
fn start(options: &Options) -> Result<Box<dyn std::io::Write>, ExitCode> {
    // Before V8 starts: it records its events only if the trace is up.
    if let Some(trace) = &options.trace
        && let Err(problem) = bindings::trace::start(trace)
    {
        eprintln!(
            "gc_bench: can't trace into {}: {problem:?}",
            trace.display()
        );
        return Err(ExitCode::FAILURE);
    }
    let report: Box<dyn std::io::Write> = match &options.report {
        Some(path) => match File::create(path) {
            Ok(file) => Box::new(file),
            Err(problem) => {
                eprintln!("gc_bench: can't write {}: {problem}", path.display());
                return Err(ExitCode::FAILURE);
            }
        },
        None => Box::new(std::io::stdout()),
    };
    // The forced GC at the end of each workload, on V8's heap.
    bindings::initialize_with_flags(&format!("--expose-gc {}", options.v8_flags));
    Ok(report)
}

fn main() -> ExitCode {
    let options = match options(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(problem) => return usage(&problem),
    };
    let mut report = match start(&options) {
        Ok(report) => report,
        Err(code) => return code,
    };
    let heap = options.heap;
    let workloads = options.workloads.unwrap_or_else(|| {
        Workload::ALL
            .into_iter()
            .filter(|workload| heap.runs(*workload))
            .collect()
    });
    for workload in workloads {
        let line = gc_bench::run(heap, workload, options.scale).to_json();
        if let Err(problem) = writeln!(report, "{line}") {
            eprintln!("gc_bench: can't write the report: {problem}");
            return ExitCode::FAILURE;
        }
    }
    bindings::trace::stop();
    ExitCode::SUCCESS
}
