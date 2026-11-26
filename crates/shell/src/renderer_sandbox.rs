//! The renderer process's sandbox (Chromium's `sandbox/policy`, the
//! renderer's): what a renderer still needs of the operating system once
//! it handles a page, and the switches its browser starts it with for
//! that. The mechanism is `sunaba`'s; this is machopo's list.
//!
//! A renderer process finds the system's fonts (the one thing it reads
//! through the file system that it can't be handed), then enters, then
//! takes its channel and starts its threads. Linux alone so far: elsewhere
//! [`enter`] does nothing.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// The renderer isn't put in a sandbox: the browser's switch, and the one
/// it hands its renderer processes.
pub const NO_SANDBOX_SWITCH: &str = "--no-sandbox";
/// Only the built-in fonts: nothing of the system's is to be read.
const TEST_FONTS_SWITCH: &str = "--test-fonts";
/// A directory of the system's font files, in the next argument.
const FONT_DIRECTORY_SWITCH: &str = "--font-directory";

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

/// Whether this build puts a renderer process in a sandbox at all.
pub const SUPPORTED: bool = cfg!(target_os = "linux");

/// The directories the system's font files are in, found once for the
/// browser: finding them opens every font file, which each renderer
/// process would otherwise do as it starts. Fonts installed later are seen
/// by the next browser.
pub fn font_directories() -> &'static [PathBuf] {
    static DIRECTORIES: OnceLock<Vec<PathBuf>> = OnceLock::new();
    DIRECTORIES.get_or_init(|| browser::SystemFonts::find().directories())
}

/// What a renderer process is told on its command line, after
/// `--type=renderer`: only what it has to know before it enters its
/// sandbox, which is before it has a channel to be told anything on.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ChildSwitches {
    pub no_sandbox: bool,
    pub test_fonts: bool,
    pub font_directories: Vec<PathBuf>,
}

impl ChildSwitches {
    /// For a renderer of a browser with `test_fonts`: the browser's
    /// `--no-sandbox`, and the font directories where they will be read.
    pub fn for_renderer(test_fonts: bool) -> Self {
        let no_sandbox = disabled();
        Self {
            no_sandbox,
            test_fonts,
            font_directories: if SUPPORTED && !no_sandbox && !test_fonts {
                font_directories().to_vec()
            } else {
                Vec::new()
            },
        }
    }

    pub fn append_to(&self, command: &mut Command) {
        if self.no_sandbox {
            command.arg(NO_SANDBOX_SWITCH);
        }
        if self.test_fonts {
            command.arg(TEST_FONTS_SWITCH);
        }
        for directory in &self.font_directories {
            command.arg(FONT_DIRECTORY_SWITCH).arg(directory);
        }
    }

    /// From a renderer process's arguments after `--type=renderer`. What
    /// it doesn't know it leaves: the browser is this same executable.
    pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Self {
        let mut switches = Self::default();
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            if argument == NO_SANDBOX_SWITCH {
                switches.no_sandbox = true;
            } else if argument == TEST_FONTS_SWITCH {
                switches.test_fonts = true;
            } else if argument == FONT_DIRECTORY_SWITCH
                && let Some(directory) = arguments.next()
            {
                switches.font_directories.push(directory.into());
            }
        }
        switches
    }
}

/// A renderer process enters its sandbox, unless told not to: first the
/// system's fonts are found (their list and what the system says of each,
/// read from files the sandbox then closes off), which the renderer's
/// main thread is handed in place of finding them itself. Called before
/// the process has a second thread.
///
/// An error is why the process isn't in a sandbox, and it is not to go on.
pub fn enter(switches: &ChildSwitches) -> Result<Option<browser::SystemFonts>, String> {
    if switches.no_sandbox || !SUPPORTED {
        return Ok(None);
    }
    let fonts = (!switches.test_fonts).then(browser::SystemFonts::find);
    #[cfg(target_os = "linux")]
    sunaba::linux::enter(&linux::policy(&switches.font_directories))
        .map_err(|error| error.to_string())?;
    Ok(fonts)
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
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (switches, arguments);
        eprintln!("machopo: this build has no renderer sandbox to probe");
    }
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
pub mod linux {
    use std::path::PathBuf;

    use sunaba::linux::{Arg, Policy, Test};

    fn int(value: libc::c_int) -> u64 {
        u64::from(value.unsigned_abs())
    }

    /// The renderer's policy: the font files and three files whose lack
    /// would change what a page sees; the system calls V8, wasmtime, the
    /// allocators, the channel and the standard library make. The rest
    /// ends the process.
    pub fn policy(font_directories: &[PathBuf]) -> Policy {
        let mut policy = Policy::new();
        for directory in font_directories {
            policy.read_only(directory);
        }
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

        // A thread's name, a mapping's name, the auxiliary vector, and no
        // core dump of a crash that was asked for.
        for option in [libc::PR_SET_NAME, libc::PR_SET_VMA, PR_GET_AUXV] {
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
        let switches = ChildSwitches {
            no_sandbox: false,
            test_fonts: false,
            font_directories: vec!["/usr/share/fonts/a b".into(), "/home/u/.fonts".into()],
        };
        let mut command = Command::new("machopo");
        switches.append_to(&mut command);
        let arguments: Vec<OsString> = command.get_args().map(OsString::from).collect();
        assert_eq!(ChildSwitches::parse(arguments), switches);

        let switches = ChildSwitches {
            no_sandbox: true,
            test_fonts: true,
            font_directories: Vec::new(),
        };
        let mut command = Command::new("machopo");
        switches.append_to(&mut command);
        let arguments: Vec<OsString> = command.get_args().map(OsString::from).collect();
        assert_eq!(arguments, ["--no-sandbox", "--test-fonts"]);
        assert_eq!(ChildSwitches::parse(arguments), switches);
    }

    #[test]
    fn a_renderer_is_handed_the_font_directories_unless_it_reads_none() {
        assert_eq!(
            ChildSwitches::for_renderer(true),
            ChildSwitches {
                no_sandbox: false,
                test_fonts: true,
                font_directories: Vec::new(),
            }
        );
        let system = ChildSwitches::for_renderer(false);
        assert!(!system.test_fonts);
        let expected = if SUPPORTED { font_directories() } else { &[] };
        assert_eq!(system.font_directories, expected);
    }

    #[test]
    fn a_renderer_with_no_switches_is_sandboxed_with_the_systems_fonts() {
        assert_eq!(parsed(&[]), ChildSwitches::default());
        assert!(!parsed(&["--something-else"]).no_sandbox);
        // A directory switch with nothing after it names none.
        assert_eq!(
            parsed(&["--font-directory"]).font_directories,
            [PathBuf::new(); 0]
        );
    }
}
