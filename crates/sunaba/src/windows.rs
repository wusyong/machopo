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

use windows_sys::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Isolation::DeriveAppContainerSidFromAppContainerName;
use windows_sys::Win32::Security::{
    CreateWellKnownSid, FreeSid, GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation,
    PSID, SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES, SetTokenInformation, TOKEN_ADJUST_DEFAULT,
    TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TokenIntegrityLevel,
    TokenIsAppContainer, WinUntrustedLabelSid,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_TYPE_DISK, FILE_TYPE_PIPE, GetFileType};
use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_UI_RESTRICTIONS,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicUIRestrictions,
    JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, CreateProcessW,
    DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess,
    GetExitCodeProcess, GetProcessMitigationPolicy, INFINITE, InitializeProcThreadAttributeList,
    OpenProcess, OpenProcessToken, PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY,
    PROC_THREAD_ATTRIBUTE_COMPONENT_FILTER, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY, PROCESS_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE as SYNCHRONIZE, STARTF_USESTDHANDLES,
    STARTUPINFOEXW, SetProcessMitigationPolicy, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject,
};

/// The most a SID takes (`SECURITY_MAX_SID_SIZE`).
const SID_MAX: usize = 68;
/// A group's attribute: it is the token's integrity level.
const SE_GROUP_INTEGRITY: u32 = 0x20;
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

/// What a process started under it can't do: read or write the user's
/// files or make any, reach the network, open another process, start a
/// program, use a window, the clipboard or a hook, or load a DLL that
/// isn't the system's. It still reads what the system lets every
/// application read.
///
/// The process is in an app container with no capabilities, one of its
/// own if `app_container` is a name no other process is given; in a job
/// of one process with every user interface restriction; and with the
/// mitigations Chromium gives a renderer at its creation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    app_container: String,
    win32k: bool,
    third_party_modules: bool,
}

impl Policy {
    /// `app_container` names the app container (nothing is registered
    /// under the name: its identity is computed from it).
    pub fn new(app_container: impl Into<String>) -> Self {
        Self {
            app_container: app_container.into(),
            win32k: false,
            third_party_modules: false,
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

        let token = match policy {
            Some(policy) => Some(app_container_token(&policy.app_container)?),
            None => None,
        };
        let job = match policy {
            Some(_) => Some(lockdown_job()?),
            None => None,
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
        let flags = CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT;
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
        // dropped, after the call; `made` is written. The token, when
        // there is one, is a primary token open for the call. Only the
        // handles in the list are inherited, and each of them is the
        // caller's to hand on or a copy made here.
        let created = unsafe {
            match &token {
                Some(token) => CreateProcessAsUserW(
                    token.as_raw_handle(),
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
        drop(owned(made.hThread));
        drop(copies);
        Ok(Child {
            process: owned(made.hProcess),
            id: made.dwProcessId,
            job,
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

/// This process's token as the app container `name`'s, with no
/// capabilities: a primary token for a new process.
fn app_container_token(name: &str) -> Result<OwnedHandle, Error> {
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
    let result =
        process_token(TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY).and_then(|base| {
            let mut token = null_mut();
            // SAFETY: `base` is open, `capabilities` and the SID it
            // points to are alive, and `token` is written.
            let made = unsafe {
                CreateAppContainerToken(
                    base.as_raw_handle(),
                    &raw const capabilities,
                    &raw mut token,
                )
            };
            check("CreateAppContainerToken", made != 0)?;
            Ok(owned(token))
        });
    // SAFETY: allocated by the call that derived it, freed once.
    unsafe { FreeSid(sid) };
    result
}

/// A job of one process that ends it when the job's handle closes or an
/// exception goes unhandled, with every user interface restriction
/// (Chromium's `JobLevel::kLockdown`).
fn lockdown_job() -> Result<OwnedHandle, Error> {
    // SAFETY: no attributes (so it isn't inherited) and no name.
    let job = unsafe { CreateJobObjectW(null(), null()) };
    check("CreateJobObjectW", !job.is_null())?;
    let job = owned(job);
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
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
    Ok(job)
}

/// A process [`Command::spawn`] started. One started under a policy is
/// ended when this is dropped (its job closes); one started with none
/// runs on.
#[derive(Debug)]
pub struct Child {
    process: OwnedHandle,
    id: u32,
    job: Option<OwnedHandle>,
}

impl Child {
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Whether the process is in a job that ends it with this value.
    pub fn ends_with_this(&self) -> bool {
        self.job.is_some()
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

/// A process started under a [`Policy`] gives up what it could only be
/// started with, for good: first thing in `main`. Its integrity level
/// goes from low to untrusted (it can no longer write what low-integrity
/// processes may); DLLs are looked for in the system's directories and
/// its own, not the current one; and a handle that isn't one ends the
/// process instead of being an error it might go on from.
///
/// On an error the process is not to go on.
pub fn lower() -> Result<(), Error> {
    let token = process_token(TOKEN_ADJUST_DEFAULT)?;
    let mut sid = [0u8; SID_MAX];
    let mut len = size32("a SID", SID_MAX)?;
    // SAFETY: the buffer is the most a SID takes, `len` long.
    let made = unsafe {
        CreateWellKnownSid(
            WinUntrustedLabelSid,
            null_mut(),
            sid.as_mut_ptr().cast(),
            &raw mut len,
        )
    };
    check("CreateWellKnownSid", made != 0)?;
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
    check("SetProcessMitigationPolicy", set != 0)
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
    let mut token = null_mut();
    // SAFETY: `process` is open; `token` is written.
    if unsafe { OpenProcessToken(process.as_raw_handle(), TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = owned(token);
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
