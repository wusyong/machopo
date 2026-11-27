//! The renderer process's sandbox (Chromium's `sandbox/policy`, the
//! renderer's): what a renderer still needs of the operating system once
//! it handles a page, and the switches its browser starts it with for
//! that. The mechanism is `sunaba`'s; this is machopo's list.
//!
//! A renderer process enters, then takes its channel and starts its
//! threads: on Linux and macOS entering is the first thing it does (on
//! Linux, while the process is one thread). On Windows a renderer process
//! is in its sandbox from its start: its browser starts it in one
//! ([`sandbox`]), and entering is the renderer lowering itself the rest
//! of the way. Elsewhere [`enter`] does nothing. On all three the
//! system's fonts are its browser's to open and hand it.

use std::ffi::OsString;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

/// The renderer isn't put in a sandbox: the browser's switch, and the one
/// it hands its renderer processes.
pub const NO_SANDBOX_SWITCH: &str = "--no-sandbox";
/// Only the built-in fonts: nothing of the system's is to be read.
const TEST_FONTS_SWITCH: &str = "--test-fonts";

/// What tells the executable to test the renderer's sandbox from inside
/// (its first argument, as `--type=renderer` is): it enters as a renderer
/// process with the switches that follow would, then tries the one thing
/// named after `--` and says what came of it (`sunaba`'s probe). What a
/// sandbox refuses can only be seen from inside one, and by the same
/// executable's same entering.
pub const PROBE_SWITCH: &str = "--type=renderer-sandbox-probe";

/// What the executable exits with as a renderer process that couldn't
/// enter its sandbox.
pub const NO_SANDBOX_EXIT: u8 = 5;

/// Whether the browser was started with `--no-sandbox` (the process's
/// command line, as Chromium's `CommandLine::ForCurrentProcess`: read
/// wherever a renderer process is started).
static DISABLED: AtomicBool = AtomicBool::new(false);

/// The browser was started with `--no-sandbox`: its renderer processes
/// are told the same.
pub fn disable() {
    DISABLED.store(true, Ordering::Relaxed);
}

pub fn disabled() -> bool {
    DISABLED.load(Ordering::Relaxed)
}

/// DLLs Microsoft didn't sign may be loaded into a renderer process: the
/// browser's switch (Chromium's `--allow-third-party-modules`), for a
/// machine where other software puts its own into every process and a
/// renderer doesn't start without. Only Windows has the restriction.
pub const ALLOW_THIRD_PARTY_MODULES_SWITCH: &str = "--allow-third-party-modules";

static THIRD_PARTY_MODULES: AtomicBool = AtomicBool::new(false);

/// The browser was started with `--allow-third-party-modules`.
pub fn allow_third_party_modules() {
    THIRD_PARTY_MODULES.store(true, Ordering::Relaxed);
}

/// What a renderer process is started in, where its sandbox is its
/// starter's to give (Windows): an app container of its own, by a name
/// no other process of this browser's is given. `None` where a renderer
/// enters its own, or the browser was told `--no-sandbox`.
pub fn sandbox() -> Option<ipc::platform::Sandbox> {
    if disabled() {
        return None;
    }
    #[cfg(windows)]
    {
        static NEXT: base::AtomicSequenceNumber<u64> = base::AtomicSequenceNumber::<u64>::new();
        let number = NEXT.get_next();
        let mut policy =
            sunaba::windows::Policy::new(windows::app_container(std::process::id(), number));
        if THIRD_PARTY_MODULES.load(Ordering::Relaxed) {
            policy.allow_third_party_modules();
        }
        Some(policy)
    }
    #[cfg(not(windows))]
    None
}

/// Whether this build puts a renderer process in a sandbox at all.
pub const SUPPORTED: bool = cfg!(any(target_os = "linux", target_os = "macos", windows));

/// Whether a renderer process is started with only the variables of the
/// browser's environment that [`passes_to_renderer`] lets through, in a
/// sandbox or out of one. Where it isn't (until a platform's list is
/// measured there), it inherits the browser's whole environment.
pub const LIMITS_ENVIRONMENT: bool = cfg!(any(target_os = "linux", target_os = "macos"));

/// The variables a renderer process is given by name: none is a secret,
/// each changes how the renderer runs.
const RENDERER_VARIABLES: &[&str] = &[
    // The log's filter and colours, a panic's backtrace, a thread's stack.
    "RUST_LOG",
    "NO_COLOR",
    "RUST_BACKTRACE",
    "RUST_LIB_BACKTRACE",
    "RUST_MIN_STACK",
    // ICU's (V8's): the default locale and time zone `Intl` has, and where
    // its data is when it isn't built in.
    "LANG",
    "TZ",
    "ICU_DATA",
    "ICU_TIMEZONE_FILES_DIR",
    // The dynamic linker's: a tool run around the whole browser (a
    // profiler that preloads itself) reaches its renderers too. machopo
    // itself needs neither.
    #[cfg(target_os = "linux")]
    "LD_PRELOAD",
    #[cfg(target_os = "linux")]
    "LD_LIBRARY_PATH",
    // dyld's, for the same: on macOS what `LD_PRELOAD` and
    // `LD_LIBRARY_PATH` are on Linux.
    #[cfg(target_os = "macos")]
    "DYLD_INSERT_LIBRARIES",
    #[cfg(target_os = "macos")]
    "DYLD_LIBRARY_PATH",
    // CoreFoundation's and libxpc's initializers read the user's text
    // encoding here (the user's id, an encoding's and a region's
    // numbers); where it isn't, CoreFoundation looks the user up in the
    // directory service and reads `~/.CFUserTextEncoding`, before the
    // renderer is in its sandbox.
    #[cfg(target_os = "macos")]
    "__CF_USER_TEXT_ENCODING",
    // samply's (the profiler `bench.py speedometer --sample` runs machopo
    // under): the name of its Mach service, which the library it
    // preloads gives the process's task port to before `main`, so before
    // the renderer is in its sandbox.
    #[cfg(target_os = "macos")]
    "SAMPLY_BOOTSTRAP_SERVER_NAME",
];

/// The variables a renderer process is given by what their names start
/// with: machopo's own switches (a new one reaches the renderer without
/// being listed), mimalloc's options (it reads them itself), and the
/// locale's categories.
const RENDERER_PREFIXES: &[&str] = &["MACHOPO_", "MIMALLOC_", "LC_"];

/// Whether the variable `name` of the browser's environment is given to
/// a renderer process. A name that isn't Unicode isn't any of them.
pub fn passes_to_renderer(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    RENDERER_VARIABLES.contains(&name)
        || RENDERER_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

/// What of `environment` a renderer process is given.
pub fn renderer_environment(
    environment: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    environment
        .into_iter()
        .filter(|(name, _)| passes_to_renderer(name))
        .collect()
}

/// Starts `command` (a renderer process's) with only what of this
/// process's environment a renderer is given, where it is limited: what
/// a renderer is to know is told it on its command line and its channel,
/// and the rest of a user's environment (tokens, keys, sockets' paths)
/// would be in its memory, which no sandbox keeps it from. What is set on
/// `command` after this (the channel's variable) goes too.
pub fn limit_environment(command: &mut Command) {
    if !LIMITS_ENVIRONMENT {
        return;
    }
    command.env_clear();
    command.envs(renderer_environment(std::env::vars_os()));
}

/// What a renderer process is told on its command line, after
/// `--type=renderer`: only what it has to know before it enters its
/// sandbox, which is before it has a channel to be told anything on.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ChildSwitches {
    pub no_sandbox: bool,
    pub test_fonts: bool,
}

impl ChildSwitches {
    /// For a renderer of a browser with `test_fonts`, and the browser's
    /// `--no-sandbox`.
    pub fn for_renderer(test_fonts: bool) -> Self {
        Self {
            no_sandbox: disabled(),
            test_fonts,
        }
    }

    pub fn append_to(&self, command: &mut Command) {
        if self.no_sandbox {
            command.arg(NO_SANDBOX_SWITCH);
        }
        if self.test_fonts {
            command.arg(TEST_FONTS_SWITCH);
        }
    }

    /// From a renderer process's arguments after `--type=renderer`. What
    /// it doesn't know it leaves: the browser is this same executable.
    pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Self {
        let mut switches = Self::default();
        for argument in arguments {
            if argument == NO_SANDBOX_SWITCH {
                switches.no_sandbox = true;
            } else if argument == TEST_FONTS_SWITCH {
                switches.test_fonts = true;
            }
        }
        switches
    }
}

/// A renderer process enters its sandbox, unless told not to: nothing
/// comes first, whatever its fonts (the browser hands it the system's).
/// Called before the process has a second thread.
///
/// An error is why the process isn't in a sandbox, and it is not to go on.
pub fn enter(switches: &ChildSwitches) -> Result<(), String> {
    if switches.no_sandbox || !SUPPORTED {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    sunaba::linux::enter(&linux::policy()).map_err(|error| error.to_string())?;
    #[cfg(target_os = "macos")]
    {
        let browser = std::os::unix::process::parent_id();
        sunaba::macos::enter(&macos::profile(browser)).map_err(|error| error.to_string())?;
    }
    // The renderer was started in its sandbox; it gives up what it could
    // only be started with.
    #[cfg(windows)]
    sunaba::windows::lower().map_err(|error| error.to_string())?;
    Ok(())
}

/// The executable as [`PROBE_SWITCH`] has it: `arguments` are a renderer's
/// switches, `--`, and the attempt. Exits with 2 where there is no
/// sandbox to test.
pub fn probe(arguments: impl IntoIterator<Item = OsString>) -> ! {
    let mut arguments = arguments.into_iter();
    let switches = ChildSwitches::parse(arguments.by_ref().take_while(|argument| argument != "--"));
    #[cfg(target_os = "linux")]
    {
        sunaba::linux::probe::no_core_dumps();
        match enter(&switches) {
            Ok(_) if !switches.no_sandbox => sunaba::linux::probe::attempt_and_exit(
                arguments.map(|argument| argument.to_string_lossy().into_owned()),
            ),
            Ok(_) => eprintln!("machopo: {NO_SANDBOX_SWITCH}: there is no sandbox to probe"),
            Err(error) => eprintln!("machopo: the sandbox wasn't entered: {error}"),
        }
    }
    #[cfg(target_os = "macos")]
    match enter(&switches) {
        Ok(_) if !switches.no_sandbox => sunaba::macos::probe::attempt_and_exit(
            arguments.map(|argument| argument.to_string_lossy().into_owned()),
        ),
        Ok(_) => eprintln!("machopo: {NO_SANDBOX_SWITCH}: there is no sandbox to probe"),
        Err(error) => eprintln!("machopo: the sandbox wasn't entered: {error}"),
    }
    // The sandbox is the starter's to give: the executable starts itself
    // as its browser starts a renderer, and that process lowers itself
    // and makes the attempt.
    #[cfg(windows)]
    {
        let attempt: Vec<OsString> = arguments.collect();
        if attempt.first().is_some_and(|word| word == windows::INSIDE) {
            match enter(&switches) {
                Ok(_) => sunaba::windows::probe::attempt_and_exit(
                    attempt
                        .iter()
                        .skip(1)
                        .map(|argument| argument.to_string_lossy().into_owned()),
                ),
                Err(error) => eprintln!("machopo: the sandbox wasn't entered: {error}"),
            }
        } else if switches.no_sandbox {
            eprintln!("machopo: {NO_SANDBOX_SWITCH}: there is no sandbox to probe");
        } else {
            let mut command =
                sunaba::windows::Command::new(std::env::current_exe().unwrap_or_default());
            command.arg(PROBE_SWITCH);
            if switches.test_fonts {
                command.arg(TEST_FONTS_SWITCH);
            }
            command.arg("--").arg(windows::INSIDE);
            for word in attempt {
                command.arg(word);
            }
            match command.spawn(sandbox().as_ref()) {
                Ok(mut child) => {
                    let code = child.wait().ok().and_then(|status| status.code());
                    std::process::exit(code.unwrap_or(2));
                }
                Err(error) => eprintln!("machopo: the probe wasn't started: {error}"),
            }
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (switches, arguments);
        eprintln!("machopo: this build has no renderer sandbox to probe");
    }
    std::process::exit(2);
}

#[cfg(windows)]
pub mod windows {
    use std::ffi::{OsStr, OsString};
    use std::fs::File;
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use std::path::PathBuf;

    /// The file a renderer process's trace goes to, open, by its handle's
    /// value in the next argument: a renderer started in its sandbox can't
    /// open it, so its browser does and hands it over.
    pub const TRACE_FILE_SWITCH: &str = "--trace-file";
    /// Likewise the file its census goes to.
    pub const CENSUS_FILE_SWITCH: &str = "--census-file";

    /// The values after [`TRACE_FILE_SWITCH`] and [`CENSUS_FILE_SWITCH`]
    /// among a renderer process's arguments.
    #[derive(Debug, Default, PartialEq, Eq)]
    pub struct HandedFiles {
        pub trace: Option<usize>,
        pub census: Option<usize>,
    }

    impl HandedFiles {
        pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Self {
            let mut files = Self::default();
            let mut arguments = arguments.into_iter();
            while let Some(argument) = arguments.next() {
                let slot = if argument == TRACE_FILE_SWITCH {
                    &mut files.trace
                } else if argument == CENSUS_FILE_SWITCH {
                    &mut files.census
                } else {
                    continue;
                };
                *slot = arguments
                    .next()
                    .and_then(|value| value.to_str()?.parse().ok());
            }
            files
        }
    }

    /// A file a renderer process is to write, opened for it before it is
    /// started: `<path>.<the renderer's id>` is what it is to be called,
    /// and the id isn't known until the process is, so it is made under
    /// another name and given its own ([`Self::name_for`]) once it is.
    pub struct Handed {
        pub file: File,
        made_as: PathBuf,
        path: OsString,
    }

    impl Handed {
        /// A new file beside `path`, open for writing, that may be renamed
        /// while it is open.
        pub fn make(path: &OsStr) -> std::io::Result<Self> {
            // Told apart from this browser's others in the making.
            static NEXT: base::AtomicSequenceNumber<u64> = base::AtomicSequenceNumber::<u64>::new();
            let number = NEXT.get_next();
            let mut made_as = path.to_owned();
            made_as.push(format!(".starting.{}.{number}", std::process::id()));
            let made_as = PathBuf::from(made_as);
            // `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`: the
            // last is what lets it be renamed with the renderer writing it.
            let file = File::options()
                .write(true)
                .create(true)
                .truncate(true)
                .share_mode(0x1 | 0x2 | 0x4)
                .open(&made_as)?;
            Ok(Self {
                file,
                made_as,
                path: path.to_owned(),
            })
        }

        /// The handle's value, as the renderer is told it: the same in the
        /// renderer, which inherits the handle.
        pub fn value(&self) -> usize {
            self.file.as_raw_handle().expose_provenance()
        }

        /// What gives the file its name once the renderer has an id.
        pub fn name_for(&self) -> impl FnOnce(u32) + Send + 'static {
            let (made_as, path) = (self.made_as.clone(), self.path.clone());
            move |renderer| {
                let mut named = path;
                named.push(format!(".{renderer}"));
                if let Err(error) = std::fs::rename(&made_as, &named) {
                    eprintln!(
                        "machopo: {} couldn't be named {}: {error}",
                        made_as.display(),
                        PathBuf::from(named).display()
                    );
                }
            }
        }
    }

    /// A renderer started in its sandbox is handed the files it writes:
    /// its trace's when `MACHOPO_TRACE` names one, and its census's when
    /// `MACHOPO_CENSUS` does and this build keeps one.
    pub fn hand_files(process: &mut browser::RendererProcess) {
        let mut named: Vec<Box<dyn FnOnce(u32) + Send>> = Vec::new();
        let census = cfg!(feature = "census").then_some(("MACHOPO_CENSUS", CENSUS_FILE_SWITCH));
        for (variable, switch) in [Some(("MACHOPO_TRACE", TRACE_FILE_SWITCH)), census]
            .into_iter()
            .flatten()
        {
            let Some(path) = std::env::var_os(variable) else {
                continue;
            };
            match Handed::make(&path) {
                Ok(handed) => {
                    process.command.arg(switch).arg(handed.value().to_string());
                    named.push(Box::new(handed.name_for()));
                    process.files.push(handed.file);
                }
                Err(error) => eprintln!(
                    "machopo: {variable}: no file for a renderer beside {}: {error}",
                    PathBuf::from(path).display()
                ),
            }
        }
        if !named.is_empty() {
            process.started = Some(Box::new(move |renderer| {
                for name in named {
                    name(renderer);
                }
            }));
        }
    }
    /// The first word of a probe's attempt in the process the probe
    /// started: it is inside, and is to make the attempt itself.
    pub const INSIDE: &str = "inside";

    /// The name of the app container of the renderer `number` of the
    /// browser `browser`: one no other renderer has, of this browser or
    /// of another running beside it.
    pub fn app_container(browser: u32, number: u64) -> String {
        format!("machopo.renderer.{browser}.{number}")
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    use sunaba::macos::Profile;

    /// The renderer's profile (Chromium's `sandbox/policy/mac`:
    /// `common.sb` and `renderer.sb`, of which this is the part a
    /// renderer with no Cocoa, no GPU and no preferences needs). Nothing
    /// is allowed but what is named here: no file is written, no socket
    /// or process made, no process signalled, and of the system's
    /// services none is spoken to: the one service looked up is its
    /// browser's.
    const SOURCE: &str = r#"(version 1)
(deny default)
; What `(deny default)` leaves allowed.
(deny iokit-get-properties process-info* nvram*)
(deny distributed-notification-post)

; Its threads' names, for whoever looks at the process from outside.
(allow process-info-setcontrol (target self))

; Its end of the channel waits at its browser's rendezvous server.
(allow mach-lookup
  (global-name (string-append "org.machopo.MachPortRendezvousServer." (param "BROWSER_PID"))))

; The time zone (ICU reads the link and the zone's file; `/etc` and
; `/var` are links themselves).
(allow file-read-metadata (literal "/etc") (literal "/var"))
(allow file-read*
  (literal "/private/etc/localtime")
  (subpath "/private/var/db/timezone"))

; The number of CPUs (V8's and the standard library's thread counts) and
; what the CPU can do (which instructions the JITs and the standard
; library pick).
(allow sysctl-read
  (sysctl-name "hw.ncpu")
  (sysctl-name-prefix "hw.optional."))
"#;

    /// The profile of a renderer started by the process `browser`. The
    /// same whatever its fonts: the system's are its browser's to open
    /// and hand it, and the built-in ones are in the executable.
    pub fn profile(browser: u32) -> Profile {
        let mut profile = Profile::new(SOURCE);
        profile.parameter("BROWSER_PID", browser.to_string());
        profile
    }
}

#[cfg(target_os = "linux")]
pub mod linux {
    use sunaba::linux::{Arg, Policy, Test};

    fn int(value: libc::c_int) -> u64 {
        u64::from(value.unsigned_abs())
    }

    /// The renderer's policy: the few files whose lack would change what a
    /// page sees (no font: the browser opens the system's and hands them
    /// over); the system calls V8, wasmtime, the allocators, the channel
    /// and the standard library make. The rest ends the process. The same
    /// whatever its fonts.
    pub fn policy() -> Policy {
        let mut policy = Policy::new();
        policy
            // The number of CPUs (glibc's `get_nprocs`, for V8's and
            // abseil's thread counts), V8's random seed, the time zone.
            .read_only("/sys/devices/system/cpu/online")
            .read_only("/dev/urandom")
            .read_only("/etc/localtime")
            .read_only("/usr/share/zoneinfo");

        policy.allow(&[
            // Memory.
            libc::SYS_mmap,
            libc::SYS_munmap,
            libc::SYS_mprotect,
            libc::SYS_mremap,
            libc::SYS_brk,
            // V8 keeps its code pages behind a protection key.
            libc::SYS_pkey_alloc,
            libc::SYS_pkey_free,
            libc::SYS_pkey_mprotect,
            // wasmtime's image of a module's memory: anonymous, its own.
            libc::SYS_memfd_create,
            libc::SYS_ftruncate,
            // Threads and their ends.
            libc::SYS_futex,
            libc::SYS_set_robust_list,
            libc::SYS_rseq,
            libc::SYS_set_tid_address,
            libc::SYS_sched_yield,
            libc::SYS_sched_getaffinity,
            libc::SYS_sched_getparam,
            libc::SYS_sched_getscheduler,
            libc::SYS_gettid,
            libc::SYS_getpid,
            libc::SYS_exit,
            libc::SYS_exit_group,
            // Signals within the process.
            libc::SYS_rt_sigaction,
            libc::SYS_rt_sigprocmask,
            libc::SYS_rt_sigreturn,
            libc::SYS_sigaltstack,
            // Clocks.
            libc::SYS_clock_gettime,
            libc::SYS_clock_getres,
            libc::SYS_clock_nanosleep,
            libc::SYS_nanosleep,
            libc::SYS_gettimeofday,
            libc::SYS_getrandom,
            // What is already open, and the channel.
            libc::SYS_read,
            libc::SYS_write,
            libc::SYS_readv,
            libc::SYS_writev,
            libc::SYS_pread64,
            libc::SYS_pwrite64,
            libc::SYS_close,
            libc::SYS_dup,
            libc::SYS_fcntl,
            libc::SYS_lseek,
            libc::SYS_fstat,
            libc::SYS_fstatfs,
            libc::SYS_fdatasync,
            libc::SYS_sendmsg,
            libc::SYS_recvmsg,
            libc::SYS_shutdown,
            libc::SYS_getsockopt,
            libc::SYS_epoll_create1,
            libc::SYS_epoll_ctl,
            libc::SYS_epoll_pwait,
            libc::SYS_eventfd2,
            libc::SYS_ppoll,
            // About the machine and the process.
            libc::SYS_uname,
            libc::SYS_sysinfo,
            libc::SYS_getuid,
            libc::SYS_geteuid,
            libc::SYS_getgid,
            libc::SYS_getegid,
            // Paths: Landlock decides which open, and opening is failed
            // below unless it only reads. Their metadata is read for any.
            libc::SYS_openat,
            libc::SYS_newfstatat,
            libc::SYS_statx,
            libc::SYS_faccessat,
            libc::SYS_faccessat2,
            libc::SYS_readlinkat,
            libc::SYS_getdents64,
        ]);
        #[cfg(target_arch = "x86_64")]
        policy.allow(&[
            libc::SYS_open,
            libc::SYS_stat,
            libc::SYS_lstat,
            libc::SYS_access,
            libc::SYS_readlink,
            libc::SYS_poll,
            libc::SYS_epoll_wait,
        ]);

        policy.allow_threads();

        // A signal only to itself (`abort`).
        let this = u64::from(std::process::id());
        policy
            .allow_when(libc::SYS_tgkill, &[Arg::int(0, Test::Equals(this))])
            .allow_when(libc::SYS_kill, &[Arg::int(0, Test::Equals(this))]);

        // A thread's name (set, and read by the trace), a mapping's name,
        // the auxiliary vector, and no core dump of a crash that was asked
        // for.
        for option in [
            libc::PR_SET_NAME,
            libc::PR_GET_NAME,
            libc::PR_SET_VMA,
            PR_GET_AUXV,
        ] {
            policy.allow_when(libc::SYS_prctl, &[Arg::int(0, Test::Equals(int(option)))]);
        }
        policy.allow_when(
            libc::SYS_prctl,
            &[
                Arg::int(0, Test::Equals(int(libc::PR_SET_DUMPABLE))),
                Arg::long(1, Test::Equals(0)),
            ],
        );

        // Whether a descriptor is a terminal.
        for request in [libc::TCGETS, libc::TCGETS2] {
            policy.allow_when(libc::SYS_ioctl, &[Arg::int(1, Test::Equals(request))]);
        }

        // Advice the allocators and V8 give of their own memory.
        for advice in [
            libc::MADV_NORMAL,
            libc::MADV_RANDOM,
            libc::MADV_SEQUENTIAL,
            libc::MADV_WILLNEED,
            libc::MADV_DONTNEED,
            libc::MADV_FREE,
            libc::MADV_HUGEPAGE,
            libc::MADV_NOHUGEPAGE,
            libc::MADV_DONTFORK,
            libc::MADV_DOFORK,
            libc::MADV_DONTDUMP,
            libc::MADV_DODUMP,
            MADV_GUARD_INSTALL,
            MADV_GUARD_REMOVE,
        ] {
            policy.allow_when(libc::SYS_madvise, &[Arg::int(2, Test::Equals(int(advice)))]);
        }

        // Its own limits, read.
        policy.allow_when(
            libc::SYS_prlimit64,
            &[Arg::int(0, Test::Equals(0)), Arg::long(2, Test::Equals(0))],
        );

        // Opening only to read. Landlock refuses the rest by path too, but
        // not, before its third ABI, an open that truncates what can be
        // read. `openat2`'s flags are in memory: libraries fall back.
        for flag in [libc::O_WRONLY, libc::O_RDWR, libc::O_CREAT, libc::O_TRUNC] {
            let set = Test::MaskedEquals {
                mask: int(flag),
                value: int(flag),
            };
            policy.fail_when(libc::SYS_openat, &[Arg::int(2, set)], libc::EACCES);
            #[cfg(target_arch = "x86_64")]
            policy.fail_when(libc::SYS_open, &[Arg::int(1, set)], libc::EACCES);
        }
        policy.fail(libc::SYS_openat2, libc::ENOSYS);

        // Changing the file system is refused, not fatal: a library that
        // tries (a cache it would refresh) goes on without. Landlock would
        // refuse most of these by path; a file's mode, owner and times it
        // doesn't decide.
        for call in [
            libc::SYS_unlinkat,
            libc::SYS_renameat,
            libc::SYS_renameat2,
            libc::SYS_mkdirat,
            libc::SYS_symlinkat,
            libc::SYS_linkat,
            libc::SYS_fchmodat,
            libc::SYS_fchmod,
            libc::SYS_fchownat,
            libc::SYS_fchown,
            libc::SYS_utimensat,
            libc::SYS_truncate,
        ] {
            policy.fail(call, libc::EACCES);
        }
        #[cfg(target_arch = "x86_64")]
        for call in [
            libc::SYS_unlink,
            libc::SYS_rename,
            libc::SYS_mkdir,
            libc::SYS_rmdir,
            libc::SYS_symlink,
            libc::SYS_link,
            libc::SYS_chmod,
            libc::SYS_chown,
            libc::SYS_lchown,
            libc::SYS_creat,
        ] {
            policy.fail(call, libc::EACCES);
        }
        policy
    }

    /// glibc reads the auxiliary vector with it (Linux 6.4): newer than
    /// `libc`'s constants.
    const PR_GET_AUXV: libc::c_int = 0x4155_5856;

    /// `madvise`'s guard pages (Linux 6.13; glibc's thread stacks): newer
    /// than `libc`'s constants.
    const MADV_GUARD_INSTALL: libc::c_int = 102;
    const MADV_GUARD_REMOVE: libc::c_int = 103;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(arguments: &[&str]) -> ChildSwitches {
        ChildSwitches::parse(arguments.iter().map(OsString::from))
    }

    #[test]
    fn a_renderers_switches_come_back_from_its_command_line() {
        let switches = ChildSwitches::default();
        let mut command = Command::new("machopo");
        switches.append_to(&mut command);
        assert_eq!(command.get_args().count(), 0);

        let switches = ChildSwitches {
            no_sandbox: true,
            test_fonts: true,
        };
        let mut command = Command::new("machopo");
        switches.append_to(&mut command);
        let arguments: Vec<OsString> = command.get_args().map(OsString::from).collect();
        assert_eq!(arguments, ["--no-sandbox", "--test-fonts"]);
        assert_eq!(ChildSwitches::parse(arguments), switches);
    }

    #[test]
    fn a_renderer_is_told_its_browsers_fonts_and_sandbox() {
        assert_eq!(
            ChildSwitches::for_renderer(true),
            ChildSwitches {
                no_sandbox: false,
                test_fonts: true,
            }
        );
        assert_eq!(ChildSwitches::for_renderer(false), ChildSwitches::default());
    }

    #[test]
    fn a_renderer_is_given_the_variables_listed_and_no_other() {
        for listed in [
            "RUST_LOG",
            "RUST_BACKTRACE",
            "RUST_MIN_STACK",
            "NO_COLOR",
            "LANG",
            "TZ",
            "ICU_DATA",
            // By what they start with.
            "MACHOPO_V8_FLAGS",
            "MACHOPO_SOMETHING_NEW",
            "MIMALLOC_PURGE_DELAY",
            "LC_ALL",
            "LC_TIME",
        ] {
            assert!(passes_to_renderer(listed.as_ref()), "{listed}");
        }
        for other in [
            "HOME",
            "PATH",
            "USER",
            "SSH_AUTH_SOCK",
            "DBUS_SESSION_BUS_ADDRESS",
            "XAUTHORITY",
            "SOME_API_TOKEN",
            // Not by a part of their name.
            "RUST_LOG_STYLE",
            "XMACHOPO_V8_FLAGS",
            "MACHOPO",
            "LANGUAGE",
            "rust_log",
            "",
        ] {
            assert!(!passes_to_renderer(other.as_ref()), "{other}");
        }
        assert_eq!(
            passes_to_renderer("LD_PRELOAD".as_ref()),
            cfg!(target_os = "linux")
        );
        for macos in [
            "DYLD_INSERT_LIBRARIES",
            "DYLD_LIBRARY_PATH",
            "__CF_USER_TEXT_ENCODING",
            "SAMPLY_BOOTSTRAP_SERVER_NAME",
        ] {
            assert_eq!(
                passes_to_renderer(macos.as_ref()),
                cfg!(target_os = "macos"),
                "{macos}"
            );
        }
        for not_even_on_macos in [
            "DYLD_FRAMEWORK_PATH",
            "TMPDIR",
            "XPC_FLAGS",
            "MallocNanoZone",
        ] {
            assert!(
                !passes_to_renderer(not_even_on_macos.as_ref()),
                "{not_even_on_macos}"
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let not_unicode = std::ffi::OsStr::from_bytes(b"MACHOPO_\xff");
            assert!(!passes_to_renderer(not_unicode));
        }
    }

    #[test]
    fn a_renderers_environment_is_the_listed_part_of_the_browsers() {
        let pair = |name: &str, value: &str| (OsString::from(name), OsString::from(value));
        let browsers = [
            pair("HOME", "/home/someone"),
            pair("LANG", "ja_JP.UTF-8"),
            pair("SOME_API_TOKEN", "secret"),
            pair("MACHOPO_V8_FLAGS", "--trace-gc"),
            pair("TZ", "Asia/Tokyo"),
        ];
        assert_eq!(
            renderer_environment(browsers),
            [
                pair("LANG", "ja_JP.UTF-8"),
                pair("MACHOPO_V8_FLAGS", "--trace-gc"),
                pair("TZ", "Asia/Tokyo"),
            ]
        );
    }

    #[test]
    fn a_renderer_with_no_switches_is_sandboxed_with_the_systems_fonts() {
        assert_eq!(parsed(&[]), ChildSwitches::default());
        assert!(!parsed(&["--something-else"]).no_sandbox);
    }

    #[cfg(windows)]
    #[test]
    fn a_renderers_app_container_is_its_own() {
        assert_ne!(windows::app_container(1, 2), windows::app_container(1, 3));
        assert_ne!(windows::app_container(1, 2), windows::app_container(2, 2));
        // One for each renderer this browser starts.
        let name = |policy: Option<ipc::platform::Sandbox>| format!("{policy:?}");
        assert_ne!(name(sandbox()), name(sandbox()));
        assert!(sandbox().is_some());
    }

    #[cfg(windows)]
    #[test]
    fn a_renderer_is_told_the_files_it_is_handed_by_their_handles_values() {
        use windows::HandedFiles;
        let parsed = |arguments: &[&str]| HandedFiles::parse(arguments.iter().map(OsString::from));
        assert_eq!(parsed(&[]), HandedFiles::default());
        assert_eq!(
            parsed(&["--test-fonts", "--trace-file", "612", "--census-file", "88"]),
            HandedFiles {
                trace: Some(612),
                census: Some(88)
            }
        );
        assert_eq!(
            parsed(&["--census-file", "4"]),
            HandedFiles {
                trace: None,
                census: Some(4)
            }
        );
        // No value, or one that is no number, is no file.
        assert_eq!(parsed(&["--trace-file"]), HandedFiles::default());
        assert_eq!(parsed(&["--trace-file", "x"]), HandedFiles::default());
    }

    #[cfg(windows)]
    #[test]
    fn a_file_handed_to_a_renderer_is_named_after_it_once_it_has_an_id() {
        use std::io::Write as _;
        let dir = std::env::temp_dir().join(format!("machopo-handed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("trace.pftrace");
        let mut handed = windows::Handed::make(path.as_os_str()).unwrap();
        assert_ne!(handed.value(), 0);
        // Made beside the path, under another name.
        let made: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(made.len(), 1);
        assert!(made[0].starts_with("trace.pftrace.starting."), "{made:?}");
        // Renamed with the file open and written to, before and after.
        handed.file.write_all(b"before ").unwrap();
        (handed.name_for())(4321);
        handed.file.write_all(b"after").unwrap();
        drop(handed);
        let named = dir.join("trace.pftrace.4321");
        assert_eq!(std::fs::read(&named).unwrap(), b"before after");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        // Another for the same path is another file.
        let first = windows::Handed::make(path.as_os_str()).unwrap();
        let second = windows::Handed::make(path.as_os_str()).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
        drop((first, second));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_renderers_profile_is_told_its_browser_alone() {
        let profile = macos::profile(42);
        assert!(
            profile
                .source()
                .starts_with("(version 1)\n(deny default)\n")
        );
        assert_eq!(
            profile.parameters(),
            [("BROWSER_PID".to_owned(), "42".to_owned())]
        );
    }
}
