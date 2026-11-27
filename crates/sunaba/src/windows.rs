//! Windows: a process is put in its sandbox by whoever starts it
//! (Chromium's `sandbox/win`: the broker's side), and lowers itself the
//! rest of the way as it starts (the target's side).
//!
//! What Windows restricts a process by is given when the process is
//! made: its token (here an app container's, with no capabilities), a
//! job, its mitigations, and which of its starter's handles it inherits.
//! So this module starts processes: [`Command::spawn`] with a [`Policy`]
//! starts one that is in its sandbox from its first instruction, and
//! without one starts one as usual, except that it inherits only the
//! handles named. The started process calls [`lower`] first thing in
//! `main`.

pub mod probe;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString, c_void};
use std::fmt;
use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{
    AsHandle, AsRawHandle as _, BorrowedHandle, FromRawHandle as _, OwnedHandle, RawHandle,
};
use std::os::windows::process::ExitStatusExt as _;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::ptr::{null, null_mut};

use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
use windows_sys::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Isolation::DeriveAppContainerSidFromAppContainerName;
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, CreateRestrictedToken, CreateWellKnownSid,
    DISABLE_MAX_PRIVILEGE, DuplicateTokenEx, FreeSid, GetAce, GetSidIdentifierAuthority,
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, PSID, RevertToSelf,
    SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES, SecurityImpersonation, SetTokenInformation,
    TOKEN_ADJUST_DEFAULT, TOKEN_APPCONTAINER_INFORMATION, TOKEN_ASSIGN_PRIMARY, TOKEN_DEFAULT_DACL,
    TOKEN_DUPLICATE, TOKEN_GROUPS, TOKEN_IMPERSONATE, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    TOKEN_USER, TokenAppContainerSid, TokenDefaultDacl, TokenGroups, TokenImpersonation,
    TokenIntegrityLevel, TokenIsAppContainer, TokenRestrictedSids, TokenUser, WinNullSid,
    WinUntrustedLabelSid,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_TYPE_DISK, FILE_TYPE_PIPE, GetFileType};
use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
use windows_sys::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows_sys::Win32::System::StationsAndDesktops::{
    CloseWindowStation, CreateDesktopW, CreateWindowStationW, DESKTOP_CREATEWINDOW,
    DESKTOP_READOBJECTS, GetProcessWindowStation, GetUserObjectInformationW, HWINSTA,
    SetProcessWindowStation, UOI_NAME,
};
use windows_sys::Win32::System::IO::{CreateIoCompletionPort, GetQueuedCompletionStatus};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    JOBOBJECT_ASSOCIATE_COMPLETION_PORT, JOBOBJECT_BASIC_UI_RESTRICTIONS,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectAssociateCompletionPortInformation,
    JobObjectBasicUIRestrictions, JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, CreateProcessW,
    DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess,
    GetExitCodeProcess, GetProcessMitigationPolicy, INFINITE, InitializeProcThreadAttributeList,
    OpenProcess, OpenProcessToken, PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY,
    PROC_THREAD_ATTRIBUTE_COMPONENT_FILTER, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY,
    PROCESS_BASIC_INFORMATION, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE as SYNCHRONIZE, PROCESS_VM_READ, ResumeThread, STARTF_USESTDHANDLES,
    STARTUPINFOEXW, SetProcessMitigationPolicy, SetThreadToken, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject,
};

/// The most a SID takes (`SECURITY_MAX_SID_SIZE`).
const SID_MAX: usize = 68;
/// A group's attribute: it is the token's integrity level.
const SE_GROUP_INTEGRITY: u32 = 0x20;
/// A group's attribute: it only ever denies access, never grants it.
const SE_GROUP_USE_FOR_DENY_ONLY: u32 = 0x10;
/// A group's attributes: it is the logon session's.
const SE_GROUP_LOGON_ID: u32 = 0xC000_0000;
/// What a job's completion port is told when a process of the job's asks
/// for more memory than its limit lets it commit.
const JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT: u32 = 9;
/// A window station's right to have desktops made on it.
const WINSTA_CREATEDESKTOP: u32 = 0x8;
/// An object's right to have its security descriptor read.
const READ_CONTROL: u32 = 0x0002_0000;
/// The untrusted integrity level's last number.
pub const UNTRUSTED: u32 = 0;
/// The low integrity level's last number: what an app container's token
/// starts at.
pub const LOW: u32 = 0x1000;
/// `SetDefaultDllDirectories`: the application's directory, System32 and
/// what `AddDllDirectory` named, and not the current directory.
const LOAD_LIBRARY_SEARCH_DEFAULT_DIRS: u32 = 0x1000;

/// Every user interface restriction a job has (`JOB_OBJECT_UILIMIT_*`):
/// other processes' window handles, the clipboard both ways, the
/// system's parameters and display settings, global atoms, desktops, and
/// logging the user off.
const UI_RESTRICTIONS: u32 = 0xFF;

/// `PROCESS_CREATION_MITIGATION_POLICY_*`'s first word (`<winbase.h>`).
mod creation {
    /// Data isn't executed, and ATL's thunks aren't emulated.
    pub const DEP: u64 = 0x01 | 0x02;
    /// Exception handler chains are checked.
    pub const SEHOP: u64 = 0x04;
    /// A corrupt heap ends the process.
    pub const HEAP_TERMINATE: u64 = 0x1 << 12;
    pub const BOTTOM_UP_ASLR: u64 = 0x1 << 16;
    /// No calls into win32k: the window manager and GDI, in the kernel.
    pub const WIN32K_DISABLE: u64 = 0x1 << 28;
    /// No AppInit DLLs, input methods' or hooks' DLLs loaded into it.
    pub const EXTENSION_POINT_DISABLE: u64 = 0x1 << 32;
    /// Only DLLs Microsoft signed are loaded.
    pub const MICROSOFT_SIGNED_ONLY: u64 = 0x1 << 44;
    /// The kernel parses no font but the system's.
    pub const NON_SYSTEM_FONT_DISABLE: u64 = 0x1 << 48;
    /// No DLL from another machine.
    pub const IMAGE_LOAD_NO_REMOTE: u64 = 0x1 << 52;
    /// No DLL a low-integrity process could have written.
    pub const IMAGE_LOAD_NO_LOW_LABEL: u64 = 0x1 << 56;
}

/// `PROCESS_CREATION_MITIGATION_POLICY2_*`: the second word.
mod creation2 {
    /// Indirect branches aren't predicted from what another security
    /// domain trained.
    pub const RESTRICT_INDIRECT_BRANCH_PREDICTION: u64 = 0x1 << 16;
    /// No file system control codes (Windows 10 22H2; ignored before).
    pub const FSCTL_DISABLE: u64 = 0x1 << 56;
}

/// A mitigation policy of a process, as [`mitigation`] reads it
/// (`PROCESS_MITIGATION_POLICY`'s numbers). Each is a word of flags, of
/// which the first bits are the policy's own switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Mitigation {
    /// Bit 0: invalid handles raise; bit 1: for good.
    StrictHandleCheck = 3,
    /// Bit 0: no win32k system calls; bit 2: no file system control
    /// codes.
    SystemCallDisable = 4,
    /// Bit 0: no extension points.
    ExtensionPointDisable = 6,
    /// Bit 0: Microsoft-signed DLLs only.
    Signature = 8,
    /// Bit 0: no fonts but the system's.
    FontDisable = 9,
    /// Bit 0: no remote images; bit 1: no low-integrity images.
    ImageLoad = 10,
    /// Bit 0: no child processes.
    ChildProcess = 13,
    /// Bit 0: indirect branches aren't predicted from what another
    /// security domain trained.
    SideChannelIsolation = 14,
}

/// Why a process wasn't started in its sandbox, or didn't lower itself:
/// the call that failed, and the system's error. Either way the process
/// isn't to run.
#[derive(Debug)]
pub struct Error {
    what: &'static str,
    source: io::Error,
}

impl Error {
    fn last(what: &'static str) -> Self {
        Self {
            what,
            source: io::Error::last_os_error(),
        }
    }

    fn other(what: &'static str, why: &'static str) -> Self {
        Self {
            what,
            source: io::Error::other(why),
        }
    }

    /// The system's error code, when the system gave one.
    pub fn code(&self) -> Option<i32> {
        self.source.raw_os_error()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.what, self.source)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

impl From<Error> for io::Error {
    fn from(error: Error) -> Self {
        Self::new(error.source.kind(), error.to_string())
    }
}

fn check(what: &'static str, ok: bool) -> Result<(), Error> {
    if ok { Ok(()) } else { Err(Error::last(what)) }
}

fn size32(what: &'static str, size: usize) -> Result<u32, Error> {
    u32::try_from(size).map_err(|_| Error::other(what, "too long"))
}

fn owned(handle: HANDLE) -> OwnedHandle {
    // SAFETY: only ever called with a handle a call that succeeded just
    // made for this process, which nothing else has: it is closed once,
    // by this value.
    unsafe { OwnedHandle::from_raw_handle(handle) }
}

/// What a process started under it can't do: open a file, the user's,
/// the system's or another program's, or make one; use a socket; open
/// another process, or what another process under a policy made; start a
/// program; use a window, the clipboard or a hook; or load a DLL once it
/// has lowered itself ([`lower`]). Until then its first thread may read
/// what the system lets every application read: that is how the process
/// loads the DLLs it starts with, and has the system make the random
/// numbers it will need.
///
/// The process's token is an app container's with no capabilities, one
/// of its own if `app_container` is a name no other process is given,
/// made from a restricted token that has every group of its starter's
/// only to deny access by and grants nothing but what is granted to the
/// null SID (Chromium's `USER_LOCKDOWN`). Its first thread starts with
/// the same app container's token made from one that is restricted to
/// all of those groups, so lets the thread do what an app container may
/// (Chromium's `USER_RESTRICTED_SAME_ACCESS`), which [`lower`] gives
/// back. The process is in a job of one process with every user
/// interface restriction, and has the mitigations Chromium gives a
/// renderer at its creation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    app_container: String,
    win32k: bool,
    third_party_modules: bool,
    memory_limit: Option<usize>,
}

impl Policy {
    /// `app_container` names the app container (nothing is registered
    /// under the name: its identity is computed from it).
    pub fn new(app_container: impl Into<String>) -> Self {
        Self {
            app_container: app_container.into(),
            win32k: false,
            third_party_modules: false,
            memory_limit: None,
        }
    }

    /// The process may call into win32k: for a program that loads
    /// `user32.dll` or `gdi32.dll` as it starts, which fails to start
    /// otherwise.
    pub fn allow_win32k(&mut self) -> &mut Self {
        self.win32k = true;
        self
    }

    /// DLLs that Microsoft didn't sign may be loaded: for a machine
    /// where other software puts its own into every process, and the
    /// process wouldn't start without.
    pub fn allow_third_party_modules(&mut self) -> &mut Self {
        self.third_party_modules = true;
        self
    }

    /// The process commits at most `bytes` of memory (the job's
    /// `ProcessMemoryLimit`): what it asks for beyond that it isn't given,
    /// as when the machine has no more, and its [`Child`] says it asked
    /// ([`Child::exceeded_memory_limit`]). Memory it only reserves isn't
    /// counted. With none, it commits what the machine lets it.
    pub fn limit_memory(&mut self, bytes: usize) -> &mut Self {
        self.memory_limit = Some(bytes);
        self
    }

    /// The most the process may commit, if [`Self::limit_memory`] said.
    pub fn memory_limit(&self) -> Option<usize> {
        self.memory_limit
    }

    /// The two words of `PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY`.
    fn creation_mitigations(&self) -> [u64; 2] {
        let mut first = creation::DEP
            | creation::SEHOP
            | creation::HEAP_TERMINATE
            | creation::BOTTOM_UP_ASLR
            | creation::EXTENSION_POINT_DISABLE
            | creation::NON_SYSTEM_FONT_DISABLE
            | creation::IMAGE_LOAD_NO_REMOTE
            | creation::IMAGE_LOAD_NO_LOW_LABEL;
        if !self.win32k {
            first |= creation::WIN32K_DISABLE;
        }
        if !self.third_party_modules {
            first |= creation::MICROSOFT_SIGNED_ONLY;
        }
        [
            first,
            creation2::RESTRICT_INDIRECT_BRANCH_PREDICTION | creation2::FSCTL_DISABLE,
        ]
    }
}

/// A program to start: its arguments, its whole environment, where it
/// runs, and the handles of this process's it inherits. Where this
/// process's output or errors are a file or a pipe, those are its output
/// and errors, and its input is nothing (a console's handle can't be
/// handed on by name, so one that is a console's is nothing to it then);
/// where neither is, it has its console's, as any program started from
/// one.
#[derive(Clone, Debug)]
pub struct Command {
    program: OsString,
    arguments: Vec<OsString>,
    /// By the name's upper case, as the system compares and orders them.
    environment: BTreeMap<OsString, (OsString, OsString)>,
    directory: Option<PathBuf>,
    inherited: Vec<RawHandle>,
    /// In place of this process's own.
    errors: Option<RawHandle>,
}

impl Command {
    /// `program` with this process's environment and directory.
    pub fn new(program: impl Into<OsString>) -> Self {
        let mut command = Self {
            program: program.into(),
            arguments: Vec::new(),
            environment: BTreeMap::new(),
            directory: None,
            inherited: Vec::new(),
            errors: None,
        };
        for (name, value) in std::env::vars_os() {
            command.env(name, value);
        }
        command
    }

    /// The program, arguments, environment and directory of a standard
    /// library's command. What it says of the standard streams can't be
    /// read from it and isn't followed.
    pub fn from_std(std: &std::process::Command) -> Self {
        let mut command = Self::new(std.get_program());
        command.arguments = std.get_args().map(OsStr::to_owned).collect();
        for (name, value) in std.get_envs() {
            match value {
                Some(value) => command.env(name, value),
                None => command.env_remove(name),
            };
        }
        command.directory = std.get_current_dir().map(PathBuf::from);
        command
    }

    pub fn arg(&mut self, argument: impl Into<OsString>) -> &mut Self {
        self.arguments.push(argument.into());
        self
    }

    pub fn env(&mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> &mut Self {
        let name = name.into();
        self.environment.insert(upper(&name), (name, value.into()));
        self
    }

    pub fn env_remove(&mut self, name: impl AsRef<OsStr>) -> &mut Self {
        self.environment.remove(&upper(name.as_ref()));
        self
    }

    /// Nothing of this process's environment: the program has only the
    /// variables set after this (the standard library's `env_clear`).
    pub fn env_clear(&mut self) -> &mut Self {
        self.environment.clear();
        self
    }

    /// The process inherits `handle`, by the same value. It has to be
    /// inheritable and open when the process is started; only processes
    /// started with it named get it.
    pub fn inherit(&mut self, handle: BorrowedHandle<'_>) -> &mut Self {
        self.inherited.push(handle.as_raw_handle());
        self
    }

    /// The process's errors go to `handle` (a pipe's or a file's end to
    /// write to), not where this process's go. It has to be open when
    /// the process is started; the process gets a copy of its own.
    pub fn stderr(&mut self, handle: BorrowedHandle<'_>) -> &mut Self {
        self.errors = Some(handle.as_raw_handle());
        self
    }

    /// Starts the program: under `policy`, in its sandbox from the
    /// start, or with none. Either way the process has no handle of this
    /// process's but those named with [`Self::inherit`] and its output
    /// and errors.
    ///
    /// An error is why nothing was started: a process that couldn't be
    /// given all of its policy isn't started with less.
    pub fn spawn(&self, policy: Option<&Policy>) -> Result<Child, Error> {
        let application = wide(&self.program)?;
        let mut line = Vec::new();
        quote(&self.program, &mut line);
        for argument in &self.arguments {
            line.push(u16::from(b' '));
            quote(argument, &mut line);
        }
        if line.contains(&0) {
            return Err(Error::other("the command line", "a NUL in it"));
        }
        line.push(0);
        let mut block = Vec::new();
        for (name, value) in self.environment.values() {
            let entry: Vec<u16> = name
                .encode_wide()
                .chain([u16::from(b'=')])
                .chain(value.encode_wide())
                .collect();
            if entry.contains(&0) {
                return Err(Error::other("the environment", "a NUL in it"));
            }
            block.extend(entry);
            block.push(0);
        }
        // An empty block is two NULs; one more ends any.
        block.extend([0, 0]);
        let directory = match &self.directory {
            Some(directory) => Some(wide(directory.as_os_str())?),
            None => None,
        };

        // Its output and errors: copies that are inheritable, whatever
        // the originals are, closed here once it has them.
        let mut handles: Vec<HANDLE> = self.inherited.clone();
        let mut standard = [null_mut(); 2];
        let mut copies = Vec::new();
        let theirs = [
            standard_handle(STD_OUTPUT_HANDLE),
            self.errors.or_else(|| standard_handle(STD_ERROR_HANDLE)),
        ];
        for (slot, handle) in standard.iter_mut().zip(theirs) {
            if let Some(copy) = handle.and_then(inheritable_copy) {
                *slot = copy.as_raw_handle();
                handles.push(copy.as_raw_handle());
                copies.push(copy);
            }
        }

        let tokens = match policy {
            Some(policy) => Some(Tokens::new(&policy.app_container)?),
            None => None,
        };
        // A process that may call into win32k is left on this process's
        // desktop: the other one grants it nothing to connect with.
        let mut desktop = match policy {
            Some(policy) if !policy.win32k => Some(alternate_desktop()?),
            _ => None,
        };
        let (job, port) = match policy {
            Some(policy) => {
                let (job, port) = lockdown_job(policy.memory_limit)?;
                (Some(job), port)
            }
            None => (None, None),
        };
        let jobs: Vec<HANDLE> = job.iter().map(|job| job.as_raw_handle()).collect();
        let mitigations = policy.map(Policy::creation_mitigations);
        // `COMPONENT_KTM`, and `PROCESS_CREATION_CHILD_PROCESS_RESTRICTED`.
        let one: u32 = 1;

        let mut attributes = Attributes::new(5)?;
        if !handles.is_empty() {
            attributes.add(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, handles.as_slice())?;
        }
        if let Some(mitigations) = &mitigations {
            attributes.add(
                PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY,
                mitigations.as_slice(),
            )?;
            attributes.add(PROC_THREAD_ATTRIBUTE_JOB_LIST, jobs.as_slice())?;
            attributes.add(
                PROC_THREAD_ATTRIBUTE_COMPONENT_FILTER,
                std::slice::from_ref(&one),
            )?;
            attributes.add(
                PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY,
                std::slice::from_ref(&one),
            )?;
        }

        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size32("STARTUPINFOEXW", size_of::<STARTUPINFOEXW>())?;
        // With neither handed on, the process is left to its console's,
        // if it has a console.
        if !copies.is_empty() {
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdOutput = standard[0];
            startup.StartupInfo.hStdError = standard[1];
        }
        startup.lpAttributeList = attributes.list();
        if let Some(desktop) = &mut desktop {
            startup.StartupInfo.lpDesktop = desktop.as_mut_ptr();
        }
        let mut flags = CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT;
        // Its first thread is given its token, and the loader told to
        // load on that thread alone, before it runs.
        if tokens.is_some() {
            flags |= CREATE_SUSPENDED;
        }
        let inherit = i32::from(!handles.is_empty());
        let directory = directory.as_ref().map_or(null(), |wide| wide.as_ptr());
        let mut made = PROCESS_INFORMATION::default();
        // SAFETY: `application`, `line` and `directory` are NUL-terminated
        // and alive for the call, and `line` is this function's to be
        // written to; `block` is NUL-terminated strings and a last NUL,
        // which the flags say are UTF-16; `startup` is the extended
        // structure the flags say it is, its size in `cb`, and its
        // attribute list and every value the list points to (`handles`,
        // `mitigations`, `jobs`, `one`) are alive until the list is
        // dropped, after the call, as the desktop's name (NUL-terminated)
        // is; `made` is written. The token, when
        // there is one, is a primary token open for the call. Only the
        // handles in the list are inherited, and each of them is the
        // caller's to hand on or a copy made here.
        let created = unsafe {
            match &tokens {
                Some(tokens) => CreateProcessAsUserW(
                    tokens.primary.as_raw_handle(),
                    application.as_ptr(),
                    line.as_mut_ptr(),
                    null(),
                    null(),
                    inherit,
                    flags,
                    block.as_ptr().cast(),
                    directory,
                    &raw const startup.StartupInfo,
                    &raw mut made,
                ),
                None => CreateProcessW(
                    application.as_ptr(),
                    line.as_mut_ptr(),
                    null(),
                    null(),
                    inherit,
                    flags,
                    block.as_ptr().cast(),
                    directory,
                    &raw const startup.StartupInfo,
                    &raw mut made,
                ),
            }
        };
        check("CreateProcess", created != 0)?;
        let process = owned(made.hProcess);
        let thread = owned(made.hThread);
        // On an error the process, which hasn't run, ends with the job,
        // whose only handle is dropped on the way out.
        if let Some(tokens) = &tokens {
            start(&process, &thread, &tokens.initial)?;
        }
        drop((thread, copies));
        Ok(Child {
            process,
            id: made.dwProcessId,
            job,
            port,
            exceeded_memory_limit: false,
        })
    }
}

/// A name as the system compares environment variables' names.
fn upper(name: &OsStr) -> OsString {
    name.to_string_lossy().to_uppercase().into()
}

fn wide(text: &OsStr) -> Result<Vec<u16>, Error> {
    let mut wide: Vec<u16> = text.encode_wide().collect();
    if wide.contains(&0) {
        return Err(Error::other("a path", "a NUL in it"));
    }
    wide.push(0);
    Ok(wide)
}

/// `argument` as the C runtime's parser reads it back whole
/// (`CommandLineToArgvW`'s rules): in quotes when it is empty or has a
/// space, a tab or a quote in it; a quote escaped by a backslash, and the
/// backslashes before a quote doubled.
fn quote(argument: &OsStr, line: &mut Vec<u16>) {
    const QUOTE: u16 = b'"' as u16;
    const BACKSLASH: u16 = b'\\' as u16;
    let units: Vec<u16> = argument.encode_wide().collect();
    let plain = !units.is_empty()
        && !units
            .iter()
            .any(|&unit| unit == u16::from(b' ') || unit == u16::from(b'\t') || unit == QUOTE);
    if plain {
        line.extend(units);
        return;
    }
    line.push(QUOTE);
    let mut backslashes = 0usize;
    for unit in units {
        if unit == BACKSLASH {
            backslashes = backslashes.saturating_add(1);
        } else {
            if unit == QUOTE {
                // Those before it again, and one for the quote itself.
                line.extend(std::iter::repeat_n(
                    BACKSLASH,
                    backslashes.saturating_add(1),
                ));
            }
            backslashes = 0;
        }
        line.push(unit);
    }
    // Those before the closing quote again, so that it closes.
    line.extend(std::iter::repeat_n(BACKSLASH, backslashes));
    line.push(QUOTE);
}

/// This process's standard handle `which`, when it has one.
fn standard_handle(which: u32) -> Option<RawHandle> {
    // SAFETY: asks for one of the process's standard handles.
    let handle = unsafe { GetStdHandle(which) };
    (!handle.is_null() && handle != INVALID_HANDLE_VALUE).then_some(handle)
}

/// An inheritable copy of `handle`, when it is a file's or a pipe's.
fn inheritable_copy(handle: RawHandle) -> Option<OwnedHandle> {
    // SAFETY: asks what the handle is; one that isn't open is an error,
    // which is neither kind.
    let kind = unsafe { GetFileType(handle) };
    if kind != FILE_TYPE_DISK && kind != FILE_TYPE_PIPE {
        return None;
    }
    let mut copy = null_mut();
    // SAFETY: both processes are this one, whose pseudo-handle is always
    // open; `copy` is written. A handle that has been closed since is an
    // error.
    let copied = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle,
            GetCurrentProcess(),
            &raw mut copy,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        )
    };
    (copied != 0).then(|| owned(copy))
}

/// A process's attribute list (`PROC_THREAD_ATTRIBUTE_LIST`), and the
/// lifetime of what its attributes point to.
struct Attributes<'a> {
    /// Aligned for the list, which is the system's own structure.
    buffer: Vec<u64>,
    values: std::marker::PhantomData<&'a ()>,
}

impl<'a> Attributes<'a> {
    /// A list with room for `count` attributes.
    fn new(count: u32) -> Result<Self, Error> {
        let mut size = 0usize;
        // SAFETY: with no list the call only writes the size one needs.
        unsafe { InitializeProcThreadAttributeList(null_mut(), count, 0, &raw mut size) };
        let mut buffer = vec![0u64; size.div_ceil(size_of::<u64>()).max(1)];
        // SAFETY: the buffer is at least `size` bytes, aligned for any
        // structure, and stays where it is until `drop` deletes the list.
        let made = unsafe {
            InitializeProcThreadAttributeList(buffer.as_mut_ptr().cast(), count, 0, &raw mut size)
        };
        check("InitializeProcThreadAttributeList", made != 0)?;
        Ok(Self {
            buffer,
            values: std::marker::PhantomData,
        })
    }

    fn list(&mut self) -> *mut c_void {
        self.buffer.as_mut_ptr().cast()
    }

    /// `attribute` is `value`, which the list points to from here on.
    fn add<T>(&mut self, attribute: u32, value: &'a [T]) -> Result<(), Error> {
        // SAFETY: the list was initialized with room for as many
        // attributes as are added; `value` is `size_of_val` bytes of
        // plain numbers or handles, the shape each attribute used here
        // takes, and outlives the list (`'a`).
        let added = unsafe {
            UpdateProcThreadAttribute(
                self.list(),
                0,
                attribute as usize,
                value.as_ptr().cast(),
                size_of_val(value),
                null_mut(),
                null(),
            )
        };
        check("UpdateProcThreadAttribute", added != 0)
    }
}

impl Drop for Attributes<'_> {
    fn drop(&mut self) {
        // SAFETY: initialized in `new`, deleted once.
        unsafe { DeleteProcThreadAttributeList(self.list()) };
    }
}

#[link(name = "kernelbase", kind = "raw-dylib")]
unsafe extern "system" {
    /// In no header; Chromium's `AccessToken::CreateAppContainer` calls
    /// it the same way.
    fn CreateAppContainerToken(
        token: HANDLE,
        capabilities: *const SECURITY_CAPABILITIES,
        app_container_token: *mut HANDLE,
    ) -> i32;
}

fn process_token(access: u32) -> Result<OwnedHandle, Error> {
    let mut token = null_mut();
    // SAFETY: the process's pseudo-handle is always open; `token` is
    // written.
    let opened = unsafe { OpenProcessToken(GetCurrentProcess(), access, &raw mut token) };
    check("OpenProcessToken", opened != 0)?;
    Ok(owned(token))
}

/// The tokens of a process started under a policy: its primary token, a
/// lockdown app container's, and its first thread's to start with, an
/// app container's that may do what any app container may (Chromium's
/// target's lockdown token and initial token, both made app containers').
struct Tokens {
    primary: OwnedHandle,
    initial: OwnedHandle,
}

impl Tokens {
    /// Made from this process's token, for the app container `name`.
    fn new(name: &str) -> Result<Self, Error> {
        let base = process_token(TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY)?;
        let sids = Sids::of(&base)?;
        let null_sid = well_known(WinNullSid)?;
        let restricting_null = [SID_AND_ATTRIBUTES {
            Sid: null_sid.as_ptr().cast_mut().cast(),
            Attributes: 0,
        }];
        // Every group only denies, and nothing is granted but what is
        // granted to the null SID, which nothing is: an app container's
        // token made from it opens nothing, and loads no DLL that isn't
        // a known one (those are mapped for every process). With no
        // privileges, but for the one to traverse.
        let lockdown = restricted(&base, DISABLE_MAX_PRIVILEGE, &sids.list, &restricting_null)?;
        // Restricted to every group it has: what it is granted is what
        // the unrestricted token would be. A process whose primary token
        // is restricted can't impersonate one that isn't (the system
        // takes that for a token to identify by), so its first thread's
        // is restricted too.
        let same_access = restricted(&base, 0, &[], &sids.list)?;
        let primary = app_container_token(&lockdown, name)?;
        let initial = app_container_token(&same_access, name)?;
        let mut impersonating = null_mut();
        // SAFETY: `initial` is open with the right to be duplicated; no
        // attributes; `impersonating` is written.
        let made = unsafe {
            DuplicateTokenEx(
                initial.as_raw_handle(),
                TOKEN_IMPERSONATE | TOKEN_QUERY,
                null(),
                SecurityImpersonation,
                TokenImpersonation,
                &raw mut impersonating,
            )
        };
        check("DuplicateTokenEx", made != 0)?;
        Ok(Self {
            primary,
            initial: owned(impersonating),
        })
    }
}

/// The SIDs of a token a restricted token is made of: its user's and
/// each of its groups' but its integrity level's (what Chromium's
/// `RestrictedToken::AddAllSidsForDenyOnly` and
/// `AddRestrictingSidAllSids` take).
struct Sids {
    /// The token's answers, which the list points into.
    _user: Vec<u64>,
    _groups: Vec<u64>,
    list: Vec<SID_AND_ATTRIBUTES>,
}

impl Sids {
    fn of(token: &OwnedHandle) -> Result<Self, Error> {
        let what = "GetTokenInformation";
        let user = information(token, TokenUser).map_err(|source| Error { what, source })?;
        let groups = information(token, TokenGroups).map_err(|source| Error { what, source })?;
        let mut list = Vec::new();
        // SAFETY: the buffer holds the structure the class returns,
        // aligned; its SID points inside the buffer.
        let sid = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        list.push(SID_AND_ATTRIBUTES {
            Sid: sid,
            Attributes: 0,
        });
        for group in groups_in(&groups).map_err(|source| Error { what, source })? {
            if group.Attributes & SE_GROUP_INTEGRITY == 0 {
                list.push(SID_AND_ATTRIBUTES {
                    Sid: group.Sid,
                    Attributes: 0,
                });
            }
        }
        Ok(Self {
            _user: user,
            _groups: groups,
            list,
        })
    }
}

/// The groups of the `TOKEN_GROUPS` a token's answer holds.
fn groups_in(answer: &[u64]) -> io::Result<&[SID_AND_ATTRIBUTES]> {
    let groups = answer.as_ptr().cast::<TOKEN_GROUPS>();
    // SAFETY: the buffer is at least one word, the first of which is the
    // count.
    let count = unsafe { (*groups).GroupCount };
    let count = usize::try_from(count).map_err(io::Error::other)?;
    let needed = size_of::<SID_AND_ATTRIBUTES>()
        .checked_mul(count)
        .and_then(|bytes| bytes.checked_add(std::mem::offset_of!(TOKEN_GROUPS, Groups)))
        .ok_or_else(|| io::Error::other("too many groups"))?;
    if needed > size_of_val(answer) {
        return Err(io::Error::other("fewer groups than counted"));
    }
    // SAFETY: the groups follow the count inside the buffer, which holds
    // all of them (checked above), aligned as the structure is; they are
    // borrowed for as long as the buffer is.
    Ok(unsafe {
        std::slice::from_raw_parts(
            (&raw const (*groups).Groups).cast::<SID_AND_ATTRIBUTES>(),
            count,
        )
    })
}

/// A restricted token made from `base`: `deny` only deny access, and if
/// any are `restricting`, what is granted is only what is granted to one
/// of them as well.
fn restricted(
    base: &OwnedHandle,
    flags: u32,
    deny: &[SID_AND_ATTRIBUTES],
    restricting: &[SID_AND_ATTRIBUTES],
) -> Result<OwnedHandle, Error> {
    let what = "CreateRestrictedToken";
    let mut token = null_mut();
    // SAFETY: `base` is open with the right to be duplicated; each list
    // is as long as said, and the SIDs it points to are alive for the
    // call (the caller's); no privilege is named; `token` is written.
    let made = unsafe {
        CreateRestrictedToken(
            base.as_raw_handle(),
            flags,
            size32(what, deny.len())?,
            deny.as_ptr(),
            0,
            null(),
            size32(what, restricting.len())?,
            restricting.as_ptr(),
            &raw mut token,
        )
    };
    check(what, made != 0)?;
    Ok(owned(token))
}

/// A well-known SID (`WELL_KNOWN_SID_TYPE`), in a buffer the most a SID
/// takes.
fn well_known(kind: i32) -> Result<[u8; SID_MAX], Error> {
    let mut sid = [0u8; SID_MAX];
    let mut len = size32("a SID", SID_MAX)?;
    // SAFETY: the buffer is the most a SID takes, `len` long.
    let made =
        unsafe { CreateWellKnownSid(kind, null_mut(), sid.as_mut_ptr().cast(), &raw mut len) };
    check("CreateWellKnownSid", made != 0)?;
    Ok(sid)
}

/// `base` as the app container `name`'s, with no capabilities: a primary
/// token for a new process.
fn app_container_token(base: &OwnedHandle, name: &str) -> Result<OwnedHandle, Error> {
    let name = wide(OsStr::new(name))?;
    let mut sid: PSID = null_mut();
    // SAFETY: `name` is NUL-terminated; `sid` is written with a SID the
    // system allocated, freed below.
    let derived = unsafe { DeriveAppContainerSidFromAppContainerName(name.as_ptr(), &raw mut sid) };
    if derived != 0 {
        return Err(Error {
            what: "DeriveAppContainerSidFromAppContainerName",
            source: io::Error::from_raw_os_error(derived),
        });
    }
    let capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: sid,
        Capabilities: null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };
    let mut token = null_mut();
    // SAFETY: `base` is open, `capabilities` and the SID it points to
    // are alive, and `token` is written.
    let made = unsafe {
        CreateAppContainerToken(
            base.as_raw_handle(),
            &raw const capabilities,
            &raw mut token,
        )
    };
    let result = check("CreateAppContainerToken", made != 0).map(|()| owned(token));
    // SAFETY: allocated by the call that derived it, freed once.
    unsafe { FreeSid(sid) };
    result
}

/// Lets a process made suspended under a policy run: its first thread
/// with `initial` until it lowers itself, and the loader mapping the DLLs
/// it starts with on that thread alone. The loader's own threads (four,
/// by default) would have only the primary token, which opens no DLL
/// that isn't a known one: one given to them fails the process's start.
fn start(process: &OwnedHandle, thread: &OwnedHandle, initial: &OwnedHandle) -> Result<(), Error> {
    one_loader_thread(process)?;
    let thread = thread.as_raw_handle();
    // SAFETY: `thread` is an open handle of the first thread of a
    // process made just now, with every access; `initial` is an
    // impersonation token open with the right to impersonate.
    let set = unsafe { SetThreadToken(&raw const thread, initial.as_raw_handle()) };
    check("SetThreadToken", set != 0)?;
    // SAFETY: as above; the thread was made suspended, and is resumed
    // once.
    let resumed = unsafe { ResumeThread(thread) };
    check("ResumeThread", resumed != u32::MAX)
}

/// Writes 1 to a suspended process's `LoaderThreads` (in its
/// `RTL_USER_PROCESS_PARAMETERS`, not in any public header): the loader
/// reads it as the process's first thread starts, and with 1 makes no
/// threads of its own.
#[cfg(target_pointer_width = "64")]
fn one_loader_thread(process: &OwnedHandle) -> Result<(), Error> {
    let what = "the process's LoaderThreads";
    let unread = || Error::other(what, "the process's parameters weren't read");
    let parameters = parameters_of(process).ok_or_else(unread)?;
    // The structure's first word is how long it is: a field past its end
    // is not one to write.
    let mut length = [0_u8; 4];
    read_from(process, parameters, &mut length).ok_or_else(unread)?;
    let end = peb::LOADER_THREADS.saturating_add(size_of::<u32>());
    if usize::try_from(u32::from_ne_bytes(length)).map_or(true, |length| length < end) {
        return Err(Error::other(
            what,
            "past the end of the process's parameters",
        ));
    }
    let address = parameters
        .checked_add(peb::LOADER_THREADS)
        .ok_or_else(|| Error::other(what, "out of the address space"))?;
    let one = 1_u32.to_ne_bytes();
    let mut written = 0_usize;
    // SAFETY: `process` is open with every access and hasn't run; the
    // four bytes written are this process's own and alive for the call;
    // they go to the other process's memory, where the field is a word
    // inside its parameters (checked above) that nothing there has read
    // yet.
    let done = unsafe {
        WriteProcessMemory(
            process.as_raw_handle(),
            std::ptr::without_provenance(address),
            one.as_ptr().cast(),
            one.len(),
            &raw mut written,
        )
    };
    check(what, done != 0 && written == one.len())
}

#[cfg(not(target_pointer_width = "64"))]
fn one_loader_thread(_process: &OwnedHandle) -> Result<(), Error> {
    Err(Error::other(
        "the process's LoaderThreads",
        "known only on 64-bit Windows",
    ))
}

/// A job of one process that ends it when the job's handle closes or an
/// exception goes unhandled, with every user interface restriction
/// (Chromium's `JobLevel::kLockdown`), and that commits at most
/// `memory_limit`, when there is one. A job with a limit has a completion
/// port, which is told when the process asks for more: nothing waits on
/// it, and what it was told is read once the process has ended
/// ([`Child::exceeded_memory_limit`]).
fn lockdown_job(memory_limit: Option<usize>) -> Result<(OwnedHandle, Option<OwnedHandle>), Error> {
    // SAFETY: no attributes (so it isn't inherited) and no name.
    let job = unsafe { CreateJobObjectW(null(), null()) };
    check("CreateJobObjectW", !job.is_null())?;
    let job = owned(job);
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
    if let Some(limit) = memory_limit {
        limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
        limits.ProcessMemoryLimit = limit;
    }
    let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
        UIRestrictionsClass: UI_RESTRICTIONS,
    };
    let limits_len = size32("the job's limits", size_of_val(&limits))?;
    let ui_len = size32("the job's restrictions", size_of_val(&ui))?;
    // SAFETY: the job is open, and each structure is the one its class
    // reads, as long as said and alive for its call.
    let (limited, restricted) = unsafe {
        (
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                limits_len,
            ),
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectBasicUIRestrictions,
                (&raw const ui).cast(),
                ui_len,
            ),
        )
    };
    check("SetInformationJobObject", limited != 0 && restricted != 0)?;
    let port = match memory_limit {
        Some(_) => Some(completion_port(&job)?),
        None => None,
    };
    Ok((job, port))
}

/// A completion port that `job` tells of what its processes do. It is
/// told before any process is in the job, so it hears of all of them.
fn completion_port(job: &OwnedHandle) -> Result<OwnedHandle, Error> {
    // SAFETY: no file, so a new port; no key; one thread, which no thread
    // ever is.
    let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1) };
    check("CreateIoCompletionPort", !port.is_null())?;
    let port = owned(port);
    let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
        CompletionKey: job.as_raw_handle(),
        CompletionPort: port.as_raw_handle(),
    };
    let length = size32("the job's port", size_of_val(&association))?;
    // SAFETY: the job and the port are open; the structure is the one
    // the class reads, as long as said and alive for the call. The key is
    // only ever given back, never followed.
    let associated = unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectAssociateCompletionPortInformation,
            (&raw const association).cast(),
            length,
        )
    };
    check("SetInformationJobObject", associated != 0)?;
    Ok(port)
}

/// The window station and desktop a process under a policy is started on
/// (Chromium's alternate window station and desktop): made by the first
/// such start, and kept, their handles never closed, until this process
/// ends, so that every later one is started on them by name. A start that
/// couldn't make them is refused, and the next tries again. The name is
/// NUL-terminated.
static ALTERNATE_DESKTOP: std::sync::Mutex<Option<Vec<u16>>> = std::sync::Mutex::new(None);

/// The full name (`station\desktop`) of the window station and desktop a
/// process under a policy is started on, made if they aren't yet.
fn alternate_desktop() -> Result<Vec<u16>, Error> {
    // A start that panicked while holding it left either a name or none.
    let mut made = ALTERNATE_DESKTOP
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(name) = &*made {
        return Ok(name.clone());
    }
    let name = make_alternate_desktop()?;
    *made = Some(name.clone());
    Ok(name)
}

/// A window station with no name, and a desktop on it named after this
/// process. Neither is given a security descriptor: their DACLs are this
/// process's token's default DACL (its user, the administrators and the
/// system), which grants a process in an app container nothing, so one
/// that did reach win32k couldn't connect to either. A window station
/// with no name is the logon session's one (`Service-0x0-<session>$`),
/// whoever asks; the desktop's name is this process's alone.
fn make_alternate_desktop() -> Result<Vec<u16>, Error> {
    // SAFETY: takes nothing; the handle it returns isn't to be closed.
    let current = unsafe { GetProcessWindowStation() };
    check("GetProcessWindowStation", !current.is_null())?;
    // SAFETY: no name, no flags, no attributes (so not inherited).
    let station =
        unsafe { CreateWindowStationW(null(), 0, GENERIC_READ | WINSTA_CREATEDESKTOP, null()) };
    check("CreateWindowStationW", !station.is_null())?;
    let made = desktop_on(station, current);
    if made.is_err() {
        // SAFETY: the handle was made above and is closed once.
        unsafe { CloseWindowStation(station) };
    }
    made
}

/// Makes this process's desktop on `station`: a desktop is made only on
/// the window station of the process that makes it, so this process's is
/// `station` for the call, then `current` again. A thread of this
/// process's that is connected to its desktop already stays on it; one
/// that first calls into win32k meanwhile would connect to `station`.
fn desktop_on(station: HWINSTA, current: HWINSTA) -> Result<Vec<u16>, Error> {
    let desktop = OsString::from(format!("sunaba-{}", std::process::id()));
    let desktop_wide = wide(&desktop)?;
    let mut name = object_name(station)?;
    name.push("\\");
    name.push(&desktop);
    let name = wide(&name)?;
    // SAFETY: the handle is an open window station.
    let switched = unsafe { SetProcessWindowStation(station) };
    check("SetProcessWindowStation", switched != 0)?;
    // SAFETY: the name is NUL-terminated and alive for the call; no
    // device, mode, flags or attributes.
    let made = unsafe {
        CreateDesktopW(
            desktop_wide.as_ptr(),
            null(),
            null(),
            0,
            DESKTOP_CREATEWINDOW | DESKTOP_READOBJECTS | READ_CONTROL,
            null(),
        )
    };
    let made = check("CreateDesktopW", !made.is_null());
    // SAFETY: the handle is this process's window station as it was.
    let back = unsafe { SetProcessWindowStation(current) };
    made?;
    check("SetProcessWindowStation", back != 0)?;
    Ok(name)
}

/// A window station's or desktop's name.
fn object_name(object: HANDLE) -> Result<OsString, Error> {
    use std::os::windows::ffi::OsStringExt as _;
    // More than a window station's or desktop's name is.
    let mut buffer = [0_u16; 256];
    let mut needed = 0;
    let length = size32("a name", size_of_val(&buffer))?;
    // SAFETY: `object` is open; the buffer is writable for the length
    // given, which the call writes at most of, and `needed` is written.
    let read = unsafe {
        GetUserObjectInformationW(
            object,
            UOI_NAME,
            buffer.as_mut_ptr().cast(),
            length,
            &raw mut needed,
        )
    };
    check("GetUserObjectInformationW", read != 0)?;
    let end = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    Ok(OsString::from_wide(&buffer[..end]))
}

/// The full name (`station\desktop`) of the window station and desktop
/// this process starts a process under a policy on, once it has started
/// one; `None` before.
pub fn alternate_desktop_name() -> Option<OsString> {
    use std::os::windows::ffi::OsStringExt as _;
    let made = ALTERNATE_DESKTOP
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    made.as_ref()
        .map(|name| OsString::from_wide(name.strip_suffix(&[0]).unwrap_or(name)))
}

/// A process [`Command::spawn`] started. One started under a policy is
/// ended when this is dropped (its job closes); one started with none
/// runs on.
#[derive(Debug)]
pub struct Child {
    process: OwnedHandle,
    id: u32,
    job: Option<OwnedHandle>,
    /// The job's completion port, when the process's memory is limited.
    port: Option<OwnedHandle>,
    /// Its port has said so.
    exceeded_memory_limit: bool,
}

impl Child {
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Whether the process is in a job that ends it with this value.
    pub fn ends_with_this(&self) -> bool {
        self.job.is_some()
    }

    /// Whether the process has asked for more memory than its policy's
    /// limit lets it commit ([`Policy::limit_memory`]), as far as its job
    /// has said: what the job said by the time the process ended is all
    /// said once it has. It may have gone on after it was refused (a
    /// program that takes a failed allocation in its stride), and ended
    /// for another reason. `false` for a process whose memory isn't
    /// limited.
    pub fn exceeded_memory_limit(&mut self) -> bool {
        let Some(port) = &self.port else {
            return false;
        };
        loop {
            let mut message = 0;
            let mut key = 0;
            let mut process = null_mut();
            // SAFETY: the port is open; the three are written; no wait.
            let got = unsafe {
                GetQueuedCompletionStatus(
                    port.as_raw_handle(),
                    &raw mut message,
                    &raw mut key,
                    &raw mut process,
                    0,
                )
            };
            // Nothing more to read: a job's messages are never failures.
            if got == 0 {
                break;
            }
            if message == JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT {
                self.exceeded_memory_limit = true;
            }
        }
        self.exceeded_memory_limit
    }

    /// Ends the process, which is not to be confused with its having
    /// ended: [`Self::wait`] says when it has.
    pub fn kill(&mut self) -> io::Result<()> {
        // SAFETY: the process's handle is open.
        if unsafe { TerminateProcess(self.process.as_raw_handle(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.waited(INFINITE)?
            .ok_or_else(|| io::Error::other("the wait ended with the process running"))
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.waited(0)
    }

    fn waited(&self, milliseconds: u32) -> io::Result<Option<ExitStatus>> {
        // SAFETY: the process's handle is open, and the wait writes
        // nothing.
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), milliseconds) } {
            WAIT_OBJECT_0 => {
                let mut code = 0;
                // SAFETY: the handle is open; `code` is written.
                let read =
                    unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &raw mut code) };
                if read == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(Some(ExitStatus::from_raw(code)))
            }
            WAIT_TIMEOUT => Ok(None),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

impl AsHandle for Child {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.process.as_handle()
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetDefaultDllDirectories(flags: u32) -> i32;
}

#[link(name = "advapi32", kind = "raw-dylib")]
unsafe extern "system" {
    /// `RtlGenRandom`, by its exported name: what the C runtime's
    /// `rand_s` calls, and V8 with it.
    #[link_name = "SystemFunction036"]
    fn RtlGenRandom(buffer: *mut c_void, length: u32) -> u8;
}

/// How much physical memory the machine has, in bytes, by the system's
/// account; `None` if it gives none.
pub fn physical_memory() -> Option<u64> {
    let mut status = MEMORYSTATUSEX {
        dwLength: u32::try_from(size_of::<MEMORYSTATUSEX>()).ok()?,
        ..MEMORYSTATUSEX::default()
    };
    // SAFETY: the structure is as long as its first field says, and is
    // written.
    let read = unsafe { GlobalMemoryStatusEx(&raw mut status) };
    (read != 0).then_some(status.ullTotalPhys)
}

/// Whether this process has lowered itself: a DLL it loads from then on
/// fails for that.
static LOWERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `delayimp.h`'s `DelayLoadInfo`: what the linker's delay-load helper
/// says of a DLL, or a function of one, it couldn't load.
#[cfg(target_env = "msvc")]
#[repr(C)]
struct DelayLoadInfo {
    size: u32,
    descriptor: *const c_void,
    slot: *mut c_void,
    dll: *const std::ffi::c_char,
    /// `DelayLoadProc`: whether the function is asked for by name, and
    /// its name or its ordinal.
    by_name: i32,
    name_or_ordinal: *const std::ffi::c_char,
    module: *mut c_void,
    function: *mut c_void,
    last_error: u32,
}

/// The hook the delay-load helper calls when it fails (it reads this
/// variable): an executable linked with `/DELAYLOAD` loads such a DLL
/// when it is first called, which a process that has lowered itself
/// can't, and the helper then raises an exception that ends it with a
/// code and nothing else. This says on its errors which DLL and function
/// it was first, then lets it end so (Chromium's `chrome.exe` has a hook
/// of its own for the same).
#[cfg(target_env = "msvc")]
#[unsafe(no_mangle)]
static __pfnDliFailureHook2: Option<
    unsafe extern "system" fn(u32, *const DelayLoadInfo) -> *mut c_void,
> = Some(delay_load_failed);

/// What [`__pfnDliFailureHook2`] calls: says what failed and returns
/// nothing, for the helper to raise its exception.
///
/// # Safety
///
/// `info` is the helper's, valid for the call.
#[cfg(target_env = "msvc")]
unsafe extern "system" fn delay_load_failed(
    notification: u32,
    info: *const DelayLoadInfo,
) -> *mut c_void {
    use std::io::Write as _;
    // `dliFailLoadLib`, and `dliFailGetProc` for a function the DLL
    // doesn't have.
    const DLL: u32 = 3;
    let text = |name: *const std::ffi::c_char| {
        if name.is_null() {
            "?".into()
        } else {
            // SAFETY: the helper's strings are NUL-terminated, alive for
            // the call.
            unsafe { std::ffi::CStr::from_ptr(name) }.to_string_lossy()
        }
    };
    // SAFETY: the caller's.
    let Some(info) = (unsafe { info.as_ref() }) else {
        return null_mut();
    };
    let function = if info.by_name != 0 {
        text(info.name_or_ordinal).into_owned()
    } else {
        format!("#{}", info.name_or_ordinal.addr() & 0xFFFF_FFFF)
    };
    let what = if notification == DLL {
        "couldn't be loaded"
    } else {
        "has no such function"
    };
    let why = if LOWERED.load(std::sync::atomic::Ordering::Relaxed) {
        ": no DLL is loaded once a process in a sandbox has lowered itself".to_owned()
    } else {
        format!(" (error {})", info.last_error)
    };
    let _ = writeln!(
        std::io::stderr(),
        "sunaba: {} {what}, for {function} when it was first called{why}",
        text(info.dll)
    );
    null_mut()
}

/// A process started under a [`Policy`] gives up what it could only be
/// started with, for good: first thing in `main`, on its first thread.
/// Its integrity level goes from low to untrusted (it can no longer
/// write what low-integrity processes may); DLLs are looked for in the
/// system's directories and its own, not the current one; a handle that
/// isn't one ends the process instead of being an error it might go on
/// from; and the first thread gives back the token it started with,
/// after which the process opens no file and loads no DLL. Before that
/// it has the system make a random number once, which the first time
/// loads what makes them (Chromium's `WarmupRandomnessInfrastructure`).
///
/// A process started with no policy may call it too: it lowers its
/// integrity level, and the rest is the same, but it has no token to
/// give back.
///
/// On an error the process is not to go on.
pub fn lower() -> Result<(), Error> {
    let token = process_token(TOKEN_ADJUST_DEFAULT)?;
    let mut sid = well_known(WinUntrustedLabelSid)?;
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: sid.as_mut_ptr().cast(),
            Attributes: SE_GROUP_INTEGRITY,
        },
    };
    let label_len = size32(
        "the integrity label",
        size_of::<TOKEN_MANDATORY_LABEL>().saturating_add(SID_MAX),
    )?;
    // SAFETY: `token` is open with the right to set this; `label` is the
    // structure the class reads, and the SID it points to is alive. The
    // length is the structure's and a SID's, as the class expects of a
    // label whose SID follows it or not.
    let set = unsafe {
        SetTokenInformation(
            token.as_raw_handle(),
            TokenIntegrityLevel,
            (&raw const label).cast(),
            label_len,
        )
    };
    check("SetTokenInformation", set != 0)?;
    // SAFETY: takes flags alone.
    let searched = unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) };
    check("SetDefaultDllDirectories", searched != 0)?;
    // Invalid handles raise, for good.
    let strict: u32 = 0b11;
    // SAFETY: the policy's structure is one word of flags, four bytes.
    let set = unsafe {
        SetProcessMitigationPolicy(
            Mitigation::StrictHandleCheck as i32,
            (&raw const strict).cast(),
            size_of::<u32>(),
        )
    };
    check("SetProcessMitigationPolicy", set != 0)?;
    let mut random = [0_u8; 16];
    // SAFETY: the buffer is this function's, as long as said.
    let made = unsafe {
        RtlGenRandom(
            random.as_mut_ptr().cast(),
            size32("a number", random.len())?,
        )
    };
    check("RtlGenRandom", made != 0)?;
    // SAFETY: takes no argument; on a thread that impersonates nothing
    // it does nothing.
    let reverted = unsafe { RevertToSelf() };
    check("RevertToSelf", reverted != 0)?;
    LOWERED.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// The process `id`, open to be asked about ([`integrity`],
/// [`in_app_container`], [`mitigation`], [`running`]): for whoever started a process
/// to see what it is in.
pub fn open(id: u32) -> io::Result<OwnedHandle> {
    // SAFETY: opens a handle or fails; nothing is read or written.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, id) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(owned(process))
}

/// Whether `process` hasn't ended.
pub fn running(process: BorrowedHandle<'_>) -> bool {
    // SAFETY: `process` is open, and the wait writes nothing.
    unsafe { WaitForSingleObject(process.as_raw_handle(), 0) == WAIT_TIMEOUT }
}

/// This process, to be asked about as [`open`]'s.
pub fn this_process() -> BorrowedHandle<'static> {
    // SAFETY: the pseudo-handle is a constant that is always open and
    // never closed.
    unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess()) }
}

/// `class` of `process`'s token, in memory aligned for its structure.
fn token_information(process: BorrowedHandle<'_>, class: i32) -> io::Result<Vec<u64>> {
    information(&token_of(process)?, class)
}

/// `process`'s token, open to be asked about.
fn token_of(process: BorrowedHandle<'_>) -> io::Result<OwnedHandle> {
    let mut token = null_mut();
    // SAFETY: `process` is open; `token` is written.
    if unsafe { OpenProcessToken(process.as_raw_handle(), TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(owned(token))
}

/// `class` of `token`, in memory aligned for its structure.
fn information(token: &OwnedHandle, class: i32) -> io::Result<Vec<u64>> {
    let mut len = 0u32;
    // SAFETY: with no buffer the call only writes the length it needs.
    unsafe { GetTokenInformation(token.as_raw_handle(), class, null_mut(), 0, &raw mut len) };
    let words = usize::try_from(len)
        .map_err(io::Error::other)?
        .div_ceil(size_of::<u64>())
        .max(1);
    let mut buffer = vec![0u64; words];
    // SAFETY: the buffer is at least `len` bytes.
    let read = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            buffer.as_mut_ptr().cast(),
            len,
            &raw mut len,
        )
    };
    if read == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(buffer)
}

/// The integrity level of `process`: [`LOW`], [`UNTRUSTED`], `0x2000`
/// for an ordinary process.
pub fn integrity(process: BorrowedHandle<'_>) -> io::Result<u32> {
    let label = token_information(process, TokenIntegrityLevel)?;
    // SAFETY: the buffer holds the label the class returns, aligned, and
    // the SID it points to is inside the buffer and valid; a valid SID
    // has a count of numbers, and that many numbers, the last of which is
    // read.
    unsafe {
        let sid = (*label.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid;
        let count = *GetSidSubAuthorityCount(sid);
        let last = u32::from(count)
            .checked_sub(1)
            .ok_or_else(|| io::Error::other("an integrity level with no numbers"))?;
        Ok(*GetSidSubAuthority(sid, last))
    }
}

/// Whether `process` is in an app container.
pub fn in_app_container(process: BorrowedHandle<'_>) -> io::Result<bool> {
    let answer = token_information(process, TokenIsAppContainer)?;
    // The class returns one word, in the buffer's first.
    Ok(answer.first().is_some_and(|word| *word & 0xFFFF_FFFF != 0))
}

/// What a process's token is, read from outside (Chromium's
/// `base::win::AccessToken`): for whoever started a process to see what
/// it was started with. SIDs are in their text form (`S-1-5-32-544`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessToken {
    /// The user's SID, as a group is.
    pub user: Group,
    pub groups: Vec<Group>,
    /// What access is also checked against, when there are any: the
    /// token is a restricted one.
    pub restricting: Vec<String>,
    /// Its app container's SID, when it is one's.
    pub app_container: Option<String>,
    /// What an object it makes with no security of its own is given:
    /// `None` when that is no DACL at all (everyone may).
    pub default_dacl: Option<Vec<Ace>>,
}

/// A SID of a token's, and how it counts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub sid: String,
    /// Access is only ever denied by it, never granted.
    pub deny_only: bool,
    /// It is the token's integrity level.
    pub integrity: bool,
    /// It is the logon session's.
    pub logon: bool,
}

/// An entry of an access control list: whom it grants (or denies) what.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ace {
    /// `ACCESS_ALLOWED_ACE_TYPE` (0), `ACCESS_DENIED_ACE_TYPE` (1), or
    /// another, whose SID isn't read (empty).
    pub kind: u8,
    pub mask: u32,
    pub sid: String,
}

impl AccessToken {
    /// `process`'s token.
    pub fn of(process: BorrowedHandle<'_>) -> io::Result<Self> {
        let token = token_of(process)?;
        let user = information(&token, TokenUser)?;
        // SAFETY: the buffer holds the structure the class returns,
        // aligned, and its SID points inside the buffer.
        let user = unsafe { group(&(*user.as_ptr().cast::<TOKEN_USER>()).User) };
        let groups = information(&token, TokenGroups)?;
        // SAFETY: each group's SID points inside the buffer.
        let groups = groups_in(&groups)?
            .iter()
            .map(|each| unsafe { group(each) })
            .collect();
        let restricting = information(&token, TokenRestrictedSids)?;
        // SAFETY: as above.
        let restricting = groups_in(&restricting)?
            .iter()
            .map(|each| unsafe { sid_text(each.Sid) })
            .collect();
        let container = information(&token, TokenAppContainerSid)?;
        // SAFETY: the buffer holds the structure the class returns, whose
        // SID is null or points inside the buffer.
        let app_container = unsafe {
            let sid =
                (*container.as_ptr().cast::<TOKEN_APPCONTAINER_INFORMATION>()).TokenAppContainer;
            (!sid.is_null()).then(|| sid_text(sid))
        };
        let dacl = information(&token, TokenDefaultDacl)?;
        // SAFETY: the buffer holds the structure the class returns, whose
        // ACL is null or a valid one inside the buffer.
        let default_dacl = unsafe {
            let acl = (*dacl.as_ptr().cast::<TOKEN_DEFAULT_DACL>()).DefaultDacl;
            if acl.is_null() {
                None
            } else {
                Some(aces(acl)?)
            }
        };
        Ok(Self {
            user,
            groups,
            restricting,
            app_container,
            default_dacl,
        })
    }
}

/// # Safety
///
/// `each`'s SID is a valid one, alive for the call.
unsafe fn group(each: &SID_AND_ATTRIBUTES) -> Group {
    Group {
        // SAFETY: the caller's.
        sid: unsafe { sid_text(each.Sid) },
        deny_only: each.Attributes & SE_GROUP_USE_FOR_DENY_ONLY != 0,
        integrity: each.Attributes & SE_GROUP_INTEGRITY != 0,
        logon: each.Attributes & SE_GROUP_LOGON_ID == SE_GROUP_LOGON_ID,
    }
}

/// The entries of `acl`.
///
/// # Safety
///
/// `acl` is a valid ACL, alive for the call.
unsafe fn aces(acl: *const ACL) -> io::Result<Vec<Ace>> {
    // SAFETY: the caller's.
    let count = unsafe { (*acl).AceCount };
    (0..u32::from(count))
        .map(|index| {
            let mut ace = null_mut();
            // SAFETY: the ACL is valid and has `count` entries; `ace` is
            // written with one of them, inside the ACL.
            if unsafe { GetAce(acl, index, &raw mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: every entry starts with its header; an allowed or
            // a denied entry (the two kinds whose layout this is) has
            // its mask after it and its SID last, inside the entry.
            Ok(unsafe {
                let kind = (*ace.cast::<ACE_HEADER>()).AceType;
                let allowed = ace.cast::<ACCESS_ALLOWED_ACE>();
                let mask = (*allowed).Mask;
                let sid = if kind <= 1 {
                    sid_text((&raw const (*allowed).SidStart).cast_mut().cast())
                } else {
                    String::new()
                };
                Ace { kind, mask, sid }
            })
        })
        .collect()
}

/// `sid` in its text form: `S-`, its revision, its authority, and each
/// of its numbers, after a `-`.
///
/// # Safety
///
/// `sid` is a valid SID, alive for the call.
unsafe fn sid_text(sid: PSID) -> String {
    use std::fmt::Write as _;
    // SAFETY: a valid SID's first byte is its revision; it has an
    // authority of six bytes and as many numbers as it counts.
    unsafe {
        let revision = *sid.cast::<u8>();
        let authority = (*GetSidIdentifierAuthority(sid)).Value;
        let authority = authority
            .iter()
            .fold(0_u64, |value, &byte| (value << 8) | u64::from(byte));
        // As the system writes it: in hexadecimal when it doesn't fit 32
        // bits.
        let mut text = if authority >> 32 == 0 {
            format!("S-{revision}-{authority}")
        } else {
            format!("S-{revision}-0x{authority:012X}")
        };
        for index in 0..u32::from(*GetSidSubAuthorityCount(sid)) {
            let _ = write!(text, "-{}", *GetSidSubAuthority(sid, index));
        }
        text
    }
}

/// Where in a 64-bit process its parameters are
/// (`RTL_USER_PROCESS_PARAMETERS`, which its PEB points to), and in
/// those its environment block's address and size in bytes, and how many
/// threads its loader is to use. The last three aren't in the public
/// `winternl.h`; the first two have been where they are since Windows
/// Vista, the third since Windows 10.
#[cfg(target_pointer_width = "64")]
mod peb {
    pub const PROCESS_PARAMETERS: usize = 0x20;
    pub const ENVIRONMENT: usize = 0x80;
    /// `DesktopInfo`, a `UNICODE_STRING`: its length in bytes, the most it
    /// may be, padding, and where the string is.
    pub const DESKTOP_INFO: usize = 0xC0;
    pub const ENVIRONMENT_SIZE: usize = 0x3F0;
    pub const LOADER_THREADS: usize = 0x40C;
    /// More than any environment is: a size read past it is taken for a
    /// misread.
    pub const MOST: usize = 1 << 24;
}

/// The process `id`, open to read its memory, when it is there and this
/// user's.
fn open_to_read(id: u32) -> Option<OwnedHandle> {
    // SAFETY: opens a handle or fails; nothing is read or written.
    let process =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, id) };
    (!process.is_null()).then(|| owned(process))
}

/// Where `process`'s parameters are, in its memory.
#[cfg(target_pointer_width = "64")]
fn parameters_of(process: &OwnedHandle) -> Option<usize> {
    // SAFETY: all-zero is a valid `PROCESS_BASIC_INFORMATION` (numbers
    // and a pointer that is only read as a number).
    let mut basic: PROCESS_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let length = u32::try_from(size_of::<PROCESS_BASIC_INFORMATION>()).ok()?;
    // SAFETY: `process` is open; `basic` is the structure the class
    // writes, of the length given, alive across the call.
    let status = unsafe {
        NtQueryInformationProcess(
            process.as_raw_handle(),
            ProcessBasicInformation,
            (&raw mut basic).cast(),
            length,
            null_mut(),
        )
    };
    if status != 0 {
        return None;
    }
    let peb = basic.PebBaseAddress.addr();
    word_at(process, peb.checked_add(peb::PROCESS_PARAMETERS)?)
}

/// How many threads the process `id`'s loader was told to map its DLLs
/// with, as it started: 1 for a process started under a policy, 0 (the
/// system's default, four) for one with none. `None` for a process that
/// isn't there, isn't this user's, or can't be read.
#[cfg(target_pointer_width = "64")]
pub fn loader_threads(id: u32) -> Option<u32> {
    let process = open_to_read(id)?;
    let parameters = parameters_of(&process)?;
    let mut threads = [0_u8; 4];
    read_from(
        &process,
        parameters.checked_add(peb::LOADER_THREADS)?,
        &mut threads,
    )?;
    Some(u32::from_ne_bytes(threads))
}

/// The window station and desktop the process `id` was started on
/// (`station\desktop`), by the name it was given: its parameters'
/// `DesktopInfo`, empty where it was given none (it is then on its
/// starter's). `None` for a process that isn't there, isn't this user's,
/// or can't be read.
#[cfg(target_pointer_width = "64")]
pub fn desktop(id: u32) -> Option<OsString> {
    use std::os::windows::ffi::OsStringExt as _;
    let process = open_to_read(id)?;
    let parameters = parameters_of(&process)?;
    let mut string = [0_u8; 16];
    read_from(
        &process,
        parameters.checked_add(peb::DESKTOP_INFO)?,
        &mut string,
    )?;
    let length = usize::from(u16::from_le_bytes([string[0], string[1]]));
    let address = usize::from_ne_bytes(*string.last_chunk::<8>()?);
    let mut bytes = vec![0_u8; length];
    if length > 0 {
        read_from(&process, address, &mut bytes)?;
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .collect();
    Some(OsString::from_wide(&units))
}

/// The environment of the process `id` as it is now, by the system's
/// account of it (its PEB's): for whoever started a process to see what
/// it was given, in a sandbox or out of one. What the process has removed
/// from it since isn't seen. `None` for a process that isn't there, isn't
/// this user's, or can't be read.
#[cfg(target_pointer_width = "64")]
pub fn environment(id: u32) -> Option<Vec<(OsString, OsString)>> {
    let process = open_to_read(id)?;
    let parameters = parameters_of(&process)?;
    let block = word_at(&process, parameters.checked_add(peb::ENVIRONMENT)?)?;
    let size = word_at(&process, parameters.checked_add(peb::ENVIRONMENT_SIZE)?)?;
    if size > peb::MOST {
        return None;
    }
    let mut bytes = vec![0_u8; size];
    read_from(&process, block, &mut bytes)?;
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .collect();
    Some(environment_in(&units))
}

/// `buffer.len()` bytes of `process`'s memory at `address`, all of them
/// or `None`.
fn read_from(process: &OwnedHandle, address: usize, buffer: &mut [u8]) -> Option<()> {
    let mut read = 0_usize;
    // SAFETY: `process` is open; the buffer is this process's, writable
    // for its length, and alive across the call, which writes at most
    // that many bytes to it and how many to `read`. What is read is
    // another process's memory, so it can't alias anything here.
    let done = unsafe {
        ReadProcessMemory(
            process.as_raw_handle(),
            std::ptr::without_provenance(address),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &raw mut read,
        )
    };
    (done != 0 && read == buffer.len()).then_some(())
}

/// The pointer-sized number at `address` in `process`.
fn word_at(process: &OwnedHandle, address: usize) -> Option<usize> {
    let mut word = [0_u8; size_of::<usize>()];
    read_from(process, address, &mut word)?;
    Some(usize::from_ne_bytes(word))
}

/// The variables in an environment block: `NAME=value` strings, each
/// ended by a NUL, until an empty one (what follows isn't the
/// environment). A name may start with `=` (the per-drive directories a
/// shell keeps), so it ends at the first `=` after its first unit.
fn environment_in(block: &[u16]) -> Vec<(OsString, OsString)> {
    use std::os::windows::ffi::OsStringExt as _;
    const EQUALS: u16 = b'=' as u16;
    block
        .split(|&unit| unit == 0)
        .take_while(|variable| !variable.is_empty())
        .map(|variable| {
            let at = variable
                .iter()
                .skip(1)
                .position(|&unit| unit == EQUALS)
                .map_or(variable.len(), |at| at.saturating_add(1));
            let value = variable.get(at.saturating_add(1)..).unwrap_or_default();
            (
                OsString::from_wide(&variable[..at]),
                OsString::from_wide(value),
            )
        })
        .collect()
}

/// The flags of `process`'s mitigation policy `which`.
pub fn mitigation(process: BorrowedHandle<'_>, which: Mitigation) -> io::Result<u32> {
    let mut flags = 0u32;
    // SAFETY: `process` is open; each policy asked for here is one word
    // of flags, which is written.
    let read = unsafe {
        GetProcessMitigationPolicy(
            process.as_raw_handle(),
            which as i32,
            (&raw mut flags).cast(),
            size_of::<u32>(),
        )
    };
    if read == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quoted(argument: &str) -> String {
        let mut line = Vec::new();
        quote(OsStr::new(argument), &mut line);
        String::from_utf16(&line).unwrap()
    }

    #[test]
    fn an_argument_is_quoted_as_the_c_runtime_reads_it_back() {
        assert_eq!(quoted("plain"), "plain");
        assert_eq!(quoted(r"C:\a\b"), r"C:\a\b");
        assert_eq!(quoted(""), r#""""#);
        assert_eq!(quoted("two words"), r#""two words""#);
        assert_eq!(quoted("a\tb"), "\"a\tb\"");
        assert_eq!(quoted(r#"say "hi""#), r#""say \"hi\"""#);
        // Backslashes count only before a quote: doubled there, and
        // before the closing one.
        assert_eq!(quoted(r"C:\a b\"), r#""C:\a b\\""#);
        assert_eq!(quoted(r#"a\"b"#), r#""a\\\"b""#);
        assert_eq!(quoted(r"a\\b c"), r#""a\\b c""#);
    }

    #[test]
    fn making_the_desktop_leaves_this_process_on_its_own_window_station() {
        use std::os::windows::ffi::OsStringExt as _;
        let station = || {
            // SAFETY: takes nothing; the handle isn't to be closed.
            let station = unsafe { GetProcessWindowStation() };
            object_name(station).unwrap()
        };
        let before = station();
        let made = alternate_desktop().unwrap();
        assert_eq!(station(), before);
        let made = OsString::from_wide(made.strip_suffix(&[0]).unwrap());
        assert_ne!(made.to_str().unwrap().split_once('\\').unwrap().0, before);
        // Made once.
        assert_eq!(alternate_desktop_name(), Some(made));
    }

    #[test]
    fn a_policy_asks_for_every_mitigation_but_what_it_was_told_to_allow() {
        let all = Policy::new("x").creation_mitigations();
        assert_eq!(
            all[0],
            0x03 | 0x04
                | (1 << 12)
                | (1 << 16)
                | (1 << 28)
                | (1 << 32)
                | (1 << 44)
                | (1 << 48)
                | (1 << 52)
                | (1 << 56)
        );
        assert_eq!(all[1], (1 << 16) | (1 << 56));
        let mut policy = Policy::new("x");
        policy.allow_win32k();
        assert_eq!(policy.creation_mitigations()[0], all[0] & !(1 << 28));
        let mut policy = Policy::new("x");
        policy.allow_third_party_modules();
        assert_eq!(policy.creation_mitigations()[0], all[0] & !(1 << 44));
    }

    #[test]
    fn a_commands_environment_is_this_processs_with_what_was_set_and_removed() {
        let mut command = Command::new("x");
        assert!(command.environment.contains_key(OsStr::new("PATH")));
        // Names are one whatever their case, and keep the last spelling.
        command.env("Sunaba_Test", "1").env("SUNABA_TEST", "2");
        let entry = &command.environment[OsStr::new("SUNABA_TEST")];
        assert_eq!(entry, &("SUNABA_TEST".into(), "2".into()));
        command.env_remove("sunaba_test");
        assert!(!command.environment.contains_key(OsStr::new("SUNABA_TEST")));
    }

    #[test]
    fn a_cleared_commands_environment_is_only_what_is_set_after() {
        let mut command = Command::new("x");
        command.env("SUNABA_BEFORE", "1").env_clear();
        assert!(command.environment.is_empty());
        command.env("SUNABA_AFTER", "2");
        assert_eq!(
            command.environment.values().collect::<Vec<_>>(),
            [&("SUNABA_AFTER".into(), "2".into())]
        );
    }

    #[test]
    fn an_environment_block_is_read_up_to_its_empty_string() {
        let block = |strings: &[&str]| -> Vec<u16> {
            strings
                .iter()
                .flat_map(|string| string.encode_utf16().chain([0]))
                .collect()
        };
        let pair = |name: &str, value: &str| (OsString::from(name), OsString::from(value));
        assert_eq!(
            environment_in(&block(&[
                "Path=C:\\Windows",
                "SPACED=a b",
                "EQUALS=a=b",
                "=C:=C:\\x",
                "EMPTY=",
                "",
                "AFTER=the end",
            ])),
            [
                pair("Path", "C:\\Windows"),
                pair("SPACED", "a b"),
                pair("EQUALS", "a=b"),
                pair("=C:", "C:\\x"),
                pair("EMPTY", ""),
            ]
        );
        // Without its empty string, or its last NUL; without an `=`.
        assert_eq!(environment_in(&block(&["A=1"])), [pair("A", "1")]);
        assert_eq!(environment_in(&block(&["A=1"])[..3]), [pair("A", "1")]);
        assert_eq!(environment_in(&block(&["A"])), [pair("A", "")]);
        assert!(environment_in(&[]).is_empty());
    }

    #[test]
    fn a_standard_commands_program_arguments_and_environment_are_taken() {
        let mut std = std::process::Command::new("program");
        std.args(["a", "b c"])
            .env("SUNABA_SET", "v")
            .env_remove("PATH");
        std.current_dir(r"C:\Windows");
        let command = Command::from_std(&std);
        assert_eq!(command.program, "program");
        assert_eq!(command.arguments, ["a", "b c"]);
        assert_eq!(command.environment[OsStr::new("SUNABA_SET")].1, "v");
        assert!(!command.environment.contains_key(OsStr::new("PATH")));
        assert_eq!(command.directory, Some(PathBuf::from(r"C:\Windows")));
    }

    #[test]
    fn a_nul_in_what_a_command_says_starts_nothing() {
        let error = Command::new("x")
            .arg("a\0b")
            .spawn(None)
            .expect_err("started");
        assert!(error.to_string().contains("NUL"), "{error}");
        let error = Command::new("x\0y").spawn(None).expect_err("started");
        assert!(error.to_string().contains("NUL"), "{error}");
        let error = Command::new("x")
            .env("A", "b\0c")
            .spawn(None)
            .expect_err("started");
        assert!(error.to_string().contains("NUL"), "{error}");
    }
}
