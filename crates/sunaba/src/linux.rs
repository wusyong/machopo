//! Linux: Landlock decides which files can be opened, seccomp-bpf which
//! system calls can be made (Chromium's `sandbox/linux`, which builds
//! namespaces and a chroot where this uses Landlock); a child may be
//! started in user and network namespaces of its own ([`Namespaces`]).
//!
//! [`enter`] is for the start of `main`, while the process is one thread:
//! both restrictions are the calling thread's and what it makes later, so
//! a thread already running would stay outside.

mod names;
mod namespaces;
pub mod probe;
mod sigsys;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use landlock::{
    ABI, Access, AccessFs, AccessNet, CompatLevel, Compatible, LandlockStatus, Ruleset,
    RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope, path_beneath_rules,
};
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule, TargetArch,
};

pub use names::name;
pub use namespaces::Namespaces;

/// What a process may still ask of the kernel once it has entered:
/// the paths it can read, and each system call it can make. A system call
/// the policy doesn't name ends the process (`SIGSYS`), after saying
/// which it was.
#[derive(Clone, Debug, Default)]
pub struct Policy {
    read_only: Vec<PathBuf>,
    calls: BTreeMap<i64, Call>,
    /// Calls that return an `errno` for some arguments.
    failing_when: Vec<(i64, Vec<Arg>, u32)>,
}

#[derive(Clone, Debug)]
enum Call {
    Allow,
    /// Allowed when every [`Arg`] of one of these holds.
    AllowWhen(Vec<Vec<Arg>>),
    /// Not made: the caller gets this `errno`.
    Fail(u32),
}

/// A test of one of a system call's arguments (the first is 0).
#[derive(Clone, Copy, Debug)]
pub struct Arg {
    index: u8,
    wide: bool,
    test: Test,
}

#[derive(Clone, Copy, Debug)]
pub enum Test {
    Equals(u64),
    /// The argument's bits under `mask` are `value`'s.
    MaskedEquals {
        mask: u64,
        value: u64,
    },
}

impl Arg {
    /// An argument the kernel reads as an `int`: its low 32 bits, which is
    /// all the kernel looks at.
    pub fn int(index: u8, test: Test) -> Self {
        Self {
            index,
            wide: false,
            test,
        }
    }

    /// An argument the kernel reads as a `long` or a pointer: all 64 bits.
    pub fn long(index: u8, test: Test) -> Self {
        Self {
            index,
            wide: true,
            test,
        }
    }
}

impl Policy {
    pub fn new() -> Self {
        Self::default()
    }

    /// `path` can be opened for reading: the file, or everything beneath
    /// the directory. One that isn't there when the process enters is left
    /// out (it can't be made later either: nothing can be written).
    pub fn read_only(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.read_only.push(path.into());
        self
    }

    /// These system calls (`libc::SYS_*`), whatever their arguments.
    pub fn allow(&mut self, calls: &[i64]) -> &mut Self {
        for call in calls {
            self.calls.insert(*call, Call::Allow);
        }
        self
    }

    /// `call` when all of `args` hold. Said again for the same call, it is
    /// allowed when either holds.
    pub fn allow_when(&mut self, call: i64, args: &[Arg]) -> &mut Self {
        match self.calls.get_mut(&call) {
            Some(Call::AllowWhen(any)) => any.push(args.to_vec()),
            _ => {
                self.calls
                    .insert(call, Call::AllowWhen(vec![args.to_vec()]));
            }
        }
        self
    }

    /// `call` isn't made and returns `errno` (`libc::ENOSYS`…), for what a
    /// library tries and has another way without.
    pub fn fail(&mut self, call: i64, errno: i32) -> &mut Self {
        self.calls.insert(call, Call::Fail(errno.unsigned_abs()));
        self
    }

    /// `call` isn't made and returns `errno` when all of `args` hold, and
    /// is otherwise as the policy says elsewhere (not named there, the
    /// rest of its uses end the process). For a call that is fine but for
    /// some flags: opening a file, but not to write it.
    pub fn fail_when(&mut self, call: i64, args: &[Arg], errno: i32) -> &mut Self {
        self.failing_when
            .push((call, args.to_vec(), errno.unsigned_abs()));
        self
    }

    /// Threads can be made and nothing else: `clone3` returns `ENOSYS`
    /// (its arguments are in memory, where a filter can't read them; libc
    /// then makes the thread with `clone`), and `clone` is allowed with
    /// `CLONE_THREAD` and no new namespace. A new process is what's left,
    /// and ends this one.
    pub fn allow_threads(&mut self) -> &mut Self {
        let namespaces = libc::CLONE_NEWNS
            | libc::CLONE_NEWCGROUP
            | libc::CLONE_NEWUTS
            | libc::CLONE_NEWIPC
            | libc::CLONE_NEWUSER
            | libc::CLONE_NEWPID
            | libc::CLONE_NEWNET;
        let mask = u64::from((libc::CLONE_THREAD | namespaces).unsigned_abs());
        let thread = u64::from(libc::CLONE_THREAD.unsigned_abs());
        self.fail(libc::SYS_clone3, libc::ENOSYS).allow_when(
            libc::SYS_clone,
            &[Arg::long(
                0,
                Test::MaskedEquals {
                    mask,
                    value: thread,
                },
            )],
        )
    }

    /// The paths that can be read.
    pub fn read_only_paths(&self) -> &[PathBuf] {
        &self.read_only
    }

    /// The filters to install, in order. seccomp runs every installed
    /// filter and takes the strictest answer, and one filter has one
    /// answer for what it matches: so a filter for each `errno` (matched:
    /// that `errno`; the rest: allowed), then the one that traps what the
    /// policy doesn't name. That one is last because it refuses `seccomp`
    /// itself.
    fn filters(&self) -> Result<Vec<BpfProgram>, Error> {
        let policy = |error: seccompiler::BackendError| Error::Policy(error.to_string());
        let arch: TargetArch = std::env::consts::ARCH.try_into().map_err(policy)?;
        let rule = |args: &[Arg]| {
            let conditions = args
                .iter()
                .map(|arg| {
                    let length = if arg.wide {
                        SeccompCmpArgLen::Qword
                    } else {
                        SeccompCmpArgLen::Dword
                    };
                    let (operator, value) = match arg.test {
                        Test::Equals(value) => (SeccompCmpOp::Eq, value),
                        Test::MaskedEquals { mask, value } => (SeccompCmpOp::MaskedEq(mask), value),
                    };
                    SeccompCondition::new(arg.index, length, operator, value)
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(policy)?;
            SeccompRule::new(conditions).map_err(policy)
        };
        // For each errno, its calls: with no rule, whatever the arguments.
        let mut failing: BTreeMap<u32, BTreeMap<i64, Vec<SeccompRule>>> = BTreeMap::new();
        let mut always: Vec<(u32, i64)> = Vec::new();
        let mut named: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
        for (number, call) in &self.calls {
            let mut rules = Vec::new();
            match call {
                Call::Allow => {}
                Call::Fail(errno) => {
                    failing
                        .entry(*errno)
                        .or_default()
                        .insert(*number, Vec::new());
                    always.push((*errno, *number));
                }
                Call::AllowWhen(any) => {
                    for all in any {
                        rules.push(rule(all)?);
                    }
                }
            }
            named.insert(*number, rules);
        }
        for (number, args, errno) in &self.failing_when {
            // One that always fails with this errno already does.
            if always.contains(&(*errno, *number)) {
                continue;
            }
            failing
                .entry(*errno)
                .or_default()
                .entry(*number)
                .or_default()
                .push(rule(args)?);
            // The trap is stricter than an errno, so the last filter has
            // to let these arguments through for the errno to be the
            // answer: unless it lets the call through whatever they are.
            let whatever = matches!(self.calls.get(number), Some(Call::Allow | Call::Fail(_)));
            if !whatever {
                named.entry(*number).or_default().push(rule(args)?);
            }
        }
        let mut filters = Vec::new();
        for (errno, calls) in failing {
            let filter = SeccompFilter::new(
                calls,
                SeccompAction::Allow,
                SeccompAction::Errno(errno),
                arch,
            )
            .map_err(policy)?;
            filters.push(filter.try_into().map_err(policy)?);
        }
        let filter = SeccompFilter::new(named, SeccompAction::Trap, SeccompAction::Allow, arch)
            .map_err(policy)?;
        filters.push(filter.try_into().map_err(policy)?);
        Ok(filters)
    }
}

/// What [`enter`] found: for a log, not for deciding anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entered {
    /// The kernel's Landlock ABI. The files are restricted from the first;
    /// later ones add what seccomp refuses here as well (TCP, abstract
    /// sockets, signals to other processes).
    pub landlock_abi: i32,
}

/// Why the process isn't in the sandbox. A process that gets one of these
/// is to end, not to go on outside: [`Error::Seccomp`] can come after the
/// files were restricted.
#[derive(Debug)]
pub enum Error {
    /// The process has this many threads, and only the calling one would
    /// be restricted.
    Threads(usize),
    /// The kernel has no Landlock, or it is off (it has to be built in,
    /// `CONFIG_SECURITY_LANDLOCK`, and among the boot's `lsm=`).
    NoLandlock(String),
    Landlock(String),
    Seccomp(String),
    /// The handler that says which system call was refused.
    Signal(std::io::Error),
    /// The policy doesn't make a filter (too many system calls, an
    /// argument past the sixth).
    Policy(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Threads(count) => write!(
                f,
                "the process has {count} threads: a sandbox is entered while there is one"
            ),
            Self::NoLandlock(why) => write!(
                f,
                "this kernel has no Landlock ({why}): it comes with Linux 5.13, built with \
                 CONFIG_SECURITY_LANDLOCK and named in the boot's lsm="
            ),
            Self::Landlock(why) => write!(f, "Landlock refused the rules: {why}"),
            Self::Seccomp(why) => write!(f, "seccomp refused the filter: {why}"),
            Self::Signal(error) => write!(f, "the SIGSYS handler wasn't installed: {error}"),
            Self::Policy(why) => write!(f, "the policy doesn't make a seccomp filter: {why}"),
        }
    }
}

impl std::error::Error for Error {}

/// Enters `policy`'s sandbox, for good: the calling thread and every
/// thread and process it makes from here on. Call it first thing in
/// `main`: with another thread running it does nothing and says so.
///
/// On an error the process may be partly restricted and is to end.
pub fn enter(policy: &Policy) -> Result<Entered, Error> {
    let threads = threads();
    if threads != 1 {
        return Err(Error::Threads(threads));
    }
    // Before anything is restricted: a policy that makes no filter leaves
    // the process as it was.
    let filters = policy.filters()?;
    sigsys::install().map_err(Error::Signal)?;
    // Landlock first: seccomp refuses its system calls.
    let landlock_abi = restrict_files(&policy.read_only)?;
    for filter in &filters {
        seccompiler::apply_filter(filter).map_err(|error| Error::Seccomp(error.to_string()))?;
    }
    Ok(Entered { landlock_abi })
}

/// How many threads the process has; 0 when the kernel wouldn't say (no
/// `/proc`), which [`enter`] takes as not one.
fn threads() -> usize {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("Threads:"))
                .and_then(|count| count.trim().parse().ok())
        })
        .unwrap_or(0)
}

/// The newest Landlock ABI the `landlock` crate knows: what it adds to the
/// first is asked for and taken where the kernel has it.
const NEWEST: ABI = ABI::V10;

/// Every access to a file Landlock knows is refused except reading what is
/// at or beneath `read_only`; where the kernel has them, TCP's `bind` and
/// `connect`, abstract Unix sockets and signals to other processes too.
/// The kernel's ABI.
fn restrict_files(read_only: &[PathBuf]) -> Result<i32, Error> {
    let refused = |error: landlock::RulesetError| Error::Landlock(error.to_string());
    let created = Ruleset::default()
        // The first ABI's file accesses are the least there has to be.
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI::V1))
        .and_then(|ruleset| {
            ruleset
                .set_compatibility(CompatLevel::BestEffort)
                .handle_access(AccessFs::from_all(NEWEST))?
                .handle_access(AccessNet::from_all(NEWEST))?
                .scope(Scope::from_all(NEWEST))?
                .create()
        });
    let mut ruleset = match created {
        Ok(ruleset) => ruleset,
        Err(error) => return Err(Error::NoLandlock(error.to_string())),
    };
    // Reading only: not `Execute`, which `AccessFs::from_read` has.
    let read = AccessFs::ReadFile | AccessFs::ReadDir;
    // `path_beneath_rules` leaves out a path that doesn't open, and asks
    // of a file only what a file has.
    ruleset = ruleset
        .add_rules(path_beneath_rules(
            read_only
                .iter()
                .map(PathBuf::as_path)
                .map(Path::to_path_buf),
            read,
        ))
        .map_err(refused)?;
    let status = ruleset.restrict_self().map_err(refused)?;
    match (status.ruleset, status.landlock) {
        (RulesetStatus::NotEnforced, landlock) => Err(Error::NoLandlock(format!("{landlock:?}"))),
        (
            _,
            LandlockStatus::Available {
                effective_abi,
                kernel_abi,
            },
        ) => Ok(kernel_abi.unwrap_or(effective_abi as i32)),
        (_, landlock) => Err(Error::NoLandlock(format!("{landlock:?}"))),
    }
}
