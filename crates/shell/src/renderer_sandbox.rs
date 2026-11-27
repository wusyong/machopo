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

/// What a renderer process is started in, where some of its sandbox is
/// its starter's to give: on Windows an app container of its own, by a
/// name no other process of this browser's is given; on Linux user and
/// network namespaces of its own, where this machine lets a process that
/// isn't privileged make them (else it is started as it was before,
/// which is said once). `None` where a renderer enters its own, or the
/// browser was told `--no-sandbox`.
pub fn sandbox() -> Option<ipc::platform::Sandbox> {
    if disabled() {
        return None;
    }
    #[cfg(target_os = "linux")]
    {
        linux::namespaces().cloned()
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
    #[cfg(not(any(windows, target_os = "linux")))]
    None
}

/// Whether this build puts a renderer process in a sandbox at all.
pub const SUPPORTED: bool = cfg!(any(target_os = "linux", target_os = "macos", windows));

/// Whether a renderer process is started with only the variables of the
/// browser's environment that [`passes_to_renderer`] lets through, in a
/// sandbox or out of one. Where it isn't (a platform whose list hasn't
/// been measured on it), it inherits the browser's whole environment.
pub const LIMITS_ENVIRONMENT: bool = cfg!(any(target_os = "linux", target_os = "macos", windows));

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
    // its data is when it isn't built in. On Windows ICU takes the locale
    // and the zone from the system's settings, not these: they are given
    // there all the same, as one list.
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
    // Where Windows is: the system's components expand paths in the
    // registry with them (`%SystemRoot%\system32\...`), and a DLL found
    // so isn't loaded where they aren't, without a word. Neither is the
    // user's; `Path`, `TEMP`, `TMP` and `LOCALAPPDATA`, which are, aren't
    // given: once lowered, a renderer looks for DLLs in the system's
    // directories and its own alone, and writes nowhere.
    #[cfg(windows)]
    "SystemRoot",
    #[cfg(windows)]
    "SystemDrive",
];

/// The variables a renderer process is given by what their names start
/// with: machopo's own switches (a new one reaches the renderer without
/// being listed), mimalloc's options (it reads them itself), and the
/// locale's categories.
const RENDERER_PREFIXES: &[&str] = &["MACHOPO_", "MIMALLOC_", "LC_"];

/// Whether the variable `name` of the browser's environment is given to
/// a renderer process. A name that isn't Unicode isn't any of them.
///
/// On Windows a name is the same whatever its case (`Path` is `PATH`,
/// and mimalloc asks for its options in lower case), so it is compared
/// so. Only ASCII's case is: a name the system would take for a listed
/// one by another letter's isn't given, which a renderer can do without.
pub fn passes_to_renderer(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    if cfg!(windows) {
        RENDERER_VARIABLES
            .iter()
            .any(|listed| listed.eq_ignore_ascii_case(name))
            || RENDERER_PREFIXES.iter().any(|prefix| {
                name.get(..prefix.len())
                    .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
            })
    } else {
        RENDERER_VARIABLES.contains(&name)
            || RENDERER_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
    }
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
    sunaba::linux::enter(&linux::policy(linux::reads_time_zone_files(
        std::env::var_os("TZ").as_deref(),
    )))
    .map_err(|error| error.to_string())?;
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
    use std::ffi::OsStr;
    use std::path::Path;

    use sunaba::linux::{Arg, Policy, Test};

    fn int(value: libc::c_int) -> u64 {
        u64::from(value.unsigned_abs())
    }

    /// The namespaces each renderer process is started in, where this
    /// machine allows them: tried once for the browser, in a child of its
    /// own. Where it doesn't, the renderer goes without (Landlock and
    /// seccomp alone, as before), and the browser says so once, with how
    /// to allow them.
    pub fn namespaces() -> Option<&'static sunaba::linux::Namespaces> {
        static NAMESPACES: std::sync::OnceLock<Option<sunaba::linux::Namespaces>> =
            std::sync::OnceLock::new();
        NAMESPACES
            .get_or_init(|| {
                // Its chroot is the renderer's own to make (`policy`).
                let namespaces = sunaba::linux::Namespaces::new().keeping_chroot();
                if namespaces.available() {
                    return Some(namespaces);
                }
                eprintln!(
                    "machopo: renderers are sandboxed without user and network namespaces of \
                     their own: this machine doesn't let a program that isn't privileged make \
                     them (on Ubuntu, `sysctl kernel.apparmor_restrict_unprivileged_userns=0`, \
                     or an AppArmor profile for machopo that allows `userns`, does)"
                );
                None
            })
            .as_ref()
    }

    /// Where the system says its time zone is: a link into the zone files
    /// (`../usr/share/zoneinfo/Asia/Tokyo`), or a copy of one.
    const LOCALTIME: &str = "/etc/localtime";

    /// The zone the time zone file `target` (where `/etc/localtime` links)
    /// is, by its name in the zone files (`Asia/Tokyo`): what ICU makes of
    /// the same link. `None` when it isn't in any zone files' directory,
    /// or isn't a name.
    pub fn zone_from_link(target: &Path) -> Option<String> {
        let target = target.to_str()?;
        let (_, zone) = target.rsplit_once("zoneinfo/")?;
        // The same zones, with or without leap seconds: ICU's by name.
        let zone = zone
            .strip_prefix("posix/")
            .or_else(|| zone.strip_prefix("right/"))
            .unwrap_or(zone);
        let named = zone.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_+-".contains(&b))
        });
        (named && icu_takes_as_a_zone(zone)).then(|| zone.to_owned())
    }

    /// The time zone a renderer process is told (its `TZ`) where the user
    /// told the browser none: this machine's, by name, which ICU finds in
    /// the zone data built into V8 without reading a file. Where
    /// `/etc/localtime` links into the zone files, by where it links; where
    /// it is a copy of one (some containers), by which it is a copy of, as
    /// ICU finds it (found once for the browser). `None` when the user's
    /// `TZ` stands, or this machine's zone can't be named (a zone of its
    /// own making, or one ICU wouldn't take from `TZ`): ICU then finds it
    /// from the files itself, and the renderer reads them.
    pub fn renderer_time_zone() -> Option<String> {
        if std::env::var_os("TZ").is_some() {
            return None;
        }
        match std::fs::read_link(LOCALTIME) {
            Ok(target) => zone_from_link(&target),
            Err(_) => {
                static COPIED: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
                COPIED
                    .get_or_init(|| zone_copied(Path::new(LOCALTIME), Path::new(ZONEINFO)))
                    .clone()
            }
        }
    }

    /// The zone files' directory.
    const ZONEINFO: &str = "/usr/share/zoneinfo";

    /// The name in `zoneinfo` of the zone whose file `localtime` is a copy
    /// of (ICU's `searchForTZFile`: the same bytes): of the files with
    /// them, the first by name of those in an area's directory
    /// (`Asia/Tokyo`, not `Japan`), else the first. Not `posix/` or
    /// `right/` (the same zones again), nor `posixrules`, `localtime` or
    /// `Factory`. `None` when no file has them, or none is a name ICU would
    /// take from `TZ`.
    pub fn zone_copied(localtime: &Path, zoneinfo: &Path) -> Option<String> {
        let wanted = std::fs::read(localtime).ok()?;
        let mut found = Vec::new();
        let mut directories = vec![(zoneinfo.to_path_buf(), String::new())];
        while let Some((directory, prefix)) = directories.pop() {
            let Ok(entries) = std::fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                let zone = format!("{prefix}{name}");
                if prefix.is_empty()
                    && ["posix", "right", "posixrules", "localtime", "Factory"]
                        .contains(&name.as_str())
                {
                    continue;
                }
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if kind.is_dir() {
                    directories.push((entry.path(), format!("{zone}/")));
                } else if kind.is_file()
                    && entry.metadata().is_ok_and(|metadata| {
                        usize::try_from(metadata.len()).is_ok_and(|len| len == wanted.len())
                    })
                    && std::fs::read(entry.path()).is_ok_and(|bytes| bytes == wanted)
                {
                    found.push(zone);
                }
            }
        }
        found.retain(|zone| zone_from_link(&Path::new("zoneinfo").join(zone)).is_some());
        found.sort();
        let in_an_area = found.iter().find(|zone| zone.contains('/'));
        in_an_area.or(found.first()).cloned()
    }

    /// Whether ICU takes `tz` (the `TZ` variable's value) as its time zone
    /// and so reads no file to find one, as ICU's `isValidOlsonID` decides
    /// (`putil.cpp`): no digit and no comma, but for at most two digits at
    /// the end (`Etc/GMT+11`), or one of four zones POSIX spells with
    /// digits. Anything else (`AST4ADT`, `CST6CDT5,J129,J131/19:30`) ICU
    /// leaves for `/etc/localtime` and the zone files.
    pub fn icu_takes_as_a_zone(tz: &str) -> bool {
        // What follows the part with no digit and no comma.
        let rest = tz.trim_start_matches(|c: char| !c.is_ascii_digit() && c != ',');
        (rest.len() <= 2 && rest.bytes().all(|b| b.is_ascii_digit()))
            || ["PST8PDT", "MST7MDT", "CST6CDT", "EST5EDT"].contains(&tz)
    }

    /// Whether a renderer whose `TZ` is `tz` reads the system's time zone
    /// files to find its zone: where ICU doesn't take `tz` as its zone (or
    /// there is none), and reads `/etc/localtime` and the zone files. Where
    /// ICU takes it, ICU checks a name of three or four letters (`CET`)
    /// against glibc's offset (`TimeZone::detectHostTimeZone`), which
    /// glibc reads from the zone's file: `sunaba`'s `enter` has glibc read
    /// it first (`tzset`), while it can.
    pub fn reads_time_zone_files(tz: Option<&OsStr>) -> bool {
        !tz.and_then(OsStr::to_str).is_some_and(icu_takes_as_a_zone)
    }

    /// The renderer's policy: no file is read but the system's time zone's,
    /// where `reads_time_zone` (its `TZ` names no zone: ICU reads
    /// `/etc/localtime` and the zone files to find it); where it doesn't,
    /// none at all, and its root is a directory that no longer is, where
    /// the renderer may chroot (in namespaces of its own, given
    /// `CAP_SYS_CHROOT`). The number of CPUs
    /// isn't read (glibc counts the ones this process may run on), nor
    /// `/dev/urandom` (V8's seeds are the system's, given it), nor any
    /// font (the browser opens the system's and hands them over). The
    /// system calls V8, wasmtime, the allocators, the channel and the
    /// standard library make; the rest ends the process. The same whatever
    /// its fonts.
    pub fn policy(reads_time_zone: bool) -> Policy {
        let mut policy = Policy::new();
        if reads_time_zone {
            policy.read_only(LOCALTIME).read_only(ZONEINFO);
        } else {
            policy.chroot();
        }

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
            "",
        ] {
            assert!(!passes_to_renderer(other.as_ref()), "{other}");
        }
        // Whatever their case, where the system's names are (Windows).
        for cased in [
            "rust_log",
            "Lang",
            "Machopo_V8_Flags",
            "mimalloc_purge_delay",
        ] {
            assert_eq!(passes_to_renderer(cased.as_ref()), cfg!(windows), "{cased}");
        }
        for windows in ["SystemRoot", "SYSTEMDRIVE"] {
            assert_eq!(
                passes_to_renderer(windows.as_ref()),
                cfg!(windows),
                "{windows}"
            );
        }
        for not_even_on_windows in [
            "PATH",
            "Path",
            "TEMP",
            "TMP",
            "LOCALAPPDATA",
            "USERPROFILE",
            "APPDATA",
            "SystemRootX",
        ] {
            assert!(
                !passes_to_renderer(not_even_on_windows.as_ref()),
                "{not_even_on_windows}"
            );
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

    #[cfg(target_os = "linux")]
    #[test]
    fn a_zone_is_named_by_where_the_systems_time_zone_links() {
        let zone = |target: &str| linux::zone_from_link(std::path::Path::new(target));
        assert_eq!(
            zone("../usr/share/zoneinfo/Asia/Tokyo").as_deref(),
            Some("Asia/Tokyo")
        );
        assert_eq!(zone("/usr/share/zoneinfo/UTC").as_deref(), Some("UTC"));
        assert_eq!(
            zone("/usr/share/zoneinfo/America/Argentina/Buenos_Aires").as_deref(),
            Some("America/Argentina/Buenos_Aires")
        );
        assert_eq!(
            zone("/usr/share/zoneinfo/Etc/GMT+11").as_deref(),
            Some("Etc/GMT+11")
        );
        // The same zones, with or without leap seconds.
        assert_eq!(
            zone("/usr/share/zoneinfo/posix/Europe/Paris").as_deref(),
            Some("Europe/Paris")
        );
        assert_eq!(
            zone("/usr/share/zoneinfo/right/Europe/Paris").as_deref(),
            Some("Europe/Paris")
        );
        // Another system's zone files.
        assert_eq!(
            zone("/usr/lib/zoneinfo/Asia/Taipei").as_deref(),
            Some("Asia/Taipei")
        );
        // Not a zone's name.
        for target in [
            "/etc/my-own-zone",
            "/usr/share/zoneinfo/",
            "/usr/share/zoneinfo/Asia//Tokyo",
            "/usr/share/zoneinfo/../etc/passwd",
            "/usr/share/zoneinfo/Asia/To kyo",
            // One ICU wouldn't take from `TZ`.
            "/usr/share/zoneinfo/Etc/GMT+123",
        ] {
            assert_eq!(zone(target), None, "{target}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_copied_zone_is_named_by_the_file_it_is_a_copy_of() {
        let dir = std::env::temp_dir().join(format!("machopo-zoneinfo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let zoneinfo = dir.join("zoneinfo");
        let write = |zone: &str, bytes: &[u8]| {
            let path = zoneinfo.join(zone);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        };
        let tokyo = b"TZif tokyo";
        write("Asia/Tokyo", tokyo);
        write("Japan", tokyo);
        write("posix/Asia/Tokyo", tokyo);
        write("posixrules", tokyo);
        write("Europe/Paris", b"TZif paris");
        // An older name that sorts before its area's.
        write("Eire", b"TZif dublin");
        write("Europe/Dublin", b"TZif dublin");
        write("posix/Asia/Seoul", b"TZif seoul");
        write("UTC", b"TZif utc");
        // Not a name ICU would take from `TZ`.
        write("Etc/GMT+123", b"TZif odd");
        let localtime = dir.join("localtime");
        let named = |bytes: &[u8]| {
            std::fs::write(&localtime, bytes).unwrap();
            linux::zone_copied(&localtime, &zoneinfo)
        };
        // An area's, before an older name for it; not `posix/`'s.
        assert_eq!(named(tokyo).as_deref(), Some("Asia/Tokyo"));
        assert_eq!(named(b"TZif paris").as_deref(), Some("Europe/Paris"));
        assert_eq!(named(b"TZif dublin").as_deref(), Some("Europe/Dublin"));
        // `posix/`'s alone: the same zones again, not looked in.
        assert_eq!(named(b"TZif seoul"), None);
        assert_eq!(named(b"TZif utc").as_deref(), Some("UTC"));
        assert_eq!(named(b"TZif of its own"), None);
        assert_eq!(named(b"TZif odd"), None);
        assert_eq!(linux::zone_copied(&dir.join("none"), &zoneinfo), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn icu_takes_a_tz_as_its_zone_as_it_decides() {
        for zone in [
            "Asia/Tokyo",
            "UTC",
            "Etc/GMT+11",
            "GMT-9",
            // Taken, if not known: its name is no zone's.
            "JST-9",
            "EST5EDT",
            ":Asia/Tokyo",
            "",
        ] {
            assert!(linux::icu_takes_as_a_zone(zone), "{zone}");
        }
        for posix in ["AST4ADT", "CST6CDT5,J129,J131/19:30", "GMT+123"] {
            assert!(!linux::icu_takes_as_a_zone(posix), "{posix}");
        }
        assert!(linux::reads_time_zone_files(None));
        assert!(linux::reads_time_zone_files(Some("AST4ADT".as_ref())));
        assert!(!linux::reads_time_zone_files(Some("Asia/Tokyo".as_ref())));
        assert!(!linux::reads_time_zone_files(Some(":Etc/UTC".as_ref())));
        // ICU checks a short name against glibc's offset, which glibc has
        // read as the renderer entered.
        for short in ["CET", "EST", "UTC", ":HST", "posix/CET", "WET0"] {
            assert!(
                !linux::reads_time_zone_files(Some(short.as_ref())),
                "{short}"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_renderer_reads_no_file_but_the_time_zones_it_has_to_find() {
        let read = |reads_time_zone| linux::policy(reads_time_zone).read_only_paths().to_vec();
        assert_eq!(read(false), Vec::<std::path::PathBuf>::new());
        // Reading none, its root is nothing, where it may chroot.
        assert!(linux::policy(false).chroots());
        assert!(!linux::policy(true).chroots());
        assert_eq!(
            read(true),
            [
                std::path::PathBuf::from("/etc/localtime"),
                "/usr/share/zoneinfo".into()
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
