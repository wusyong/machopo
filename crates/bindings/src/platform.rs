//! Process-wide V8 + cppgc initialisation.

use std::sync::OnceLock;

/// The process's V8 platform, once V8 is initialised.
static PLATFORM: OnceLock<v8::SharedRef<v8::Platform>> = OnceLock::new();

/// What Chrome's renderers give V8 that machopo's would otherwise lack, as
/// Blink's `gin::V8Initializer` sets V8's flags from Chrome's features
/// (Chrome for Testing's renderer's, by `--print-flag-values`): Sparkplug
/// calling the IC builtin its feedback picks and comparing and adding
/// inline, not through the generic builtins (`--sparkplug-plus`); the
/// inline caches' handlers shared across maps, and up to 10 maps before an
/// IC goes megamorphic; additions' feedback as Chrome records it. First, so
/// that a caller's flags and `MACHOPO_V8_FLAGS` can undo any. V8's heap's
/// settings (old space, compaction, the memory reducer) stay V8's own:
/// Chrome's trade memory for time otherwise, and measured alike.
const CHROME_FLAGS: &str = "--sparkplug-plus --homomorphic-ic \
    --max-valid-polymorphic-map-count=10 --no-additive-safe-int-feedback";

/// Appended after every other flag, so neither a test's flags nor
/// `MACHOPO_V8_FLAGS` can undo it.
///
/// `--no-single-threaded-gc-in-background`: V8 lets every GC cycle use
/// background threads from the start, even when the isolate is in
/// efficiency mode (`--efficiency-mode`, or a lowered isolate priority).
/// Otherwise, for a cycle that starts in efficiency mode, V8 later calls
/// `CppHeap::ReEnableConcurrentMarking`, which turns an incremental CppHeap
/// concurrent without checking its marking support. Nothing of machopo's is on
/// the CppHeap (the elise heap marks through it, with its own markers), so
/// cppgc's concurrent markers would find an empty heap; but that path has
/// never run under a V8 with its checks, and buys nothing here: machopo never
/// lowers an isolate's priority (no background tabs yet), which is all the
/// flag changes. Chrome turns it on for its background tabs' battery.
///
/// `--no-cppgc-young-generation`: V8 compiles cppgc's young generation in
/// whenever the caged heap is on (the default on x64 and arm64), and this
/// experimental flag would turn it on at run time: minor GCs, generational
/// barriers, and `TracedReference` slots remembered by address, none of
/// which the CppHeap, empty, needs; kept off, as Chrome keeps it.
const PINNED_FLAGS: &str = "--no-single-threaded-gc-in-background --no-cppgc-young-generation";

/// Initialises V8 and cppgc for the process. Idempotent; safe to call from
/// any thread.
///
/// Environment:
/// - `MACHOPO_V8_FLAGS`: extra V8 flags, e.g. `--stress-marking=50`.
/// - `MACHOPO_V8_THREAD_POOL_SIZE`: worker threads for the V8 platform (used by
///   concurrent marking and sweeping). Default: one per core.
pub fn initialize() {
    initialize_with_flags("");
}

/// Like [`initialize`] with flags of the caller's own; `MACHOPO_V8_FLAGS` is
/// appended, so a test binary's flags can be varied from outside (e.g. by a
/// job that runs the suite under random GC flags). Only the first call in
/// a process has any effect.
pub fn initialize_with_flags(flags: &str) {
    PLATFORM.get_or_init(|| {
        let extra = std::env::var("MACHOPO_V8_FLAGS").unwrap_or_default();
        let flags = format!("{CHROME_FLAGS} {flags} {extra} {PINNED_FLAGS}");
        v8::V8::set_flags_from_string(&flags);
        let threads = std::env::var("MACHOPO_V8_THREAD_POOL_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        // TODO: ship ICU data (e.g. via `deno_core_icudata`) and call
        // `v8::icu::set_common_data_*` here so `Intl` works.
        let platform = v8::new_default_platform(threads, false).make_shared();
        v8::V8::initialize_platform(platform.clone());
        // cppgc must be up before any `cppgc::Heap` (and so any isolate).
        v8::cppgc::initialize_process(platform.clone());
        v8::V8::initialize();
        tracing::info!(
            version = v8::V8::get_version(),
            flags = flags.trim(),
            threads,
            "V8 initialized"
        );
        platform
    });
}

/// The platform [`initialize`] set up (initialising first if needed): its
/// worker threads run V8's background work, and the elise heap's
/// (`gc_platform`).
pub fn platform() -> v8::SharedRef<v8::Platform> {
    initialize();
    PLATFORM.get().expect("initialized above").clone()
}

/// Whether V8 has been initialised in this process.
pub(crate) fn is_initialized() -> bool {
    PLATFORM.get().is_some()
}

pub fn v8_version() -> &'static str {
    v8::V8::get_version()
}

/// For a test binary whose tests share one process: initialises V8 with
/// `flags` before `main`, so before any test. Only the first initialisation
/// sets the flags, and this crate initialises V8 itself without any
/// ([`platform`], `V8PerIsolateData::new`): set from a test, the flags would be whichever
/// test ran first's. V8's `unittests` set its platform up the same way, in
/// gtest's global test environment (`DefaultPlatformEnvironment`).
///
/// A binary of its own, whose tests all want the same flags, may call
/// [`initialize_with_flags`] from them instead.
#[macro_export]
macro_rules! initialize_v8_before_main {
    ($flags:expr) => {
        $crate::__ctor::declarative::ctor!(
            // SAFETY: this runs before `main`, where std promises less. It
            // reads the environment (set up by then: the C runtime fills
            // it in before any constructor) and starts V8's platform, whose
            // worker threads are V8's own, not std's. Only test binaries
            // expand it, never a library, so it never runs under a dynamic
            // loader's lock. A panic aborts the process, which the test
            // run reports.
            #[ctor(unsafe)]
            fn initialize_v8_before_main() {
                $crate::initialize_with_flags($flags);
            }
        );
    };
}
