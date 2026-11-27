# sunaba's soundness

What the crate promises, what it trusts, what it doesn't promise, each `unsafe`, and what to review when changing it. The API is in `README.md`.

## What is promised (Linux)

After `sunaba::linux::enter(&policy)` returns `Ok`, for the calling thread and every thread and process it makes from then on, whatever code they run:

1. **Files.** No file or directory is opened except for reading, at or beneath a path of the policy's `read_only` that existed at the time. Nothing is written, truncated, made, removed, renamed or linked through a path, and no file is executed.
2. **System calls.** A system call is made only if the policy allows it (for its arguments, where it tests them). One the policy fails returns its `errno` and isn't made. Any other isn't made, and the thread gets `SIGSYS`.
3. **For good.** No later call loosens either: Landlock rules and seccomp filters only stack, and the process has `no_new_privs`.

If `enter` returns `Err`, nothing is promised, and the caller is to end the process (`Error::Threads`, `Error::Policy`, `Error::Signal` and `Error::NoLandlock` are returned before anything is restricted; `Error::Landlock` and `Error::Seccomp` may come after part of it).

A child whose starter called `Namespaces::enter` in it between the fork and the exec, and whose exec followed `Ok`, is in a user namespace and a network namespace of its own: no network interface but loopback, the same user and group inside as out (not root), and no capability once it has exec'd a program with none of its own. Nothing else of its namespaces changes (its PID, mount, IPC and UTS namespaces are its starter's). If `enter` returns `Err`, the child is to end (the starter's `spawn` fails), and the starter may start it again without.

## What is promised (macOS)

After `sunaba::macos::enter(&profile)` returns `Ok`, for every thread of the process and every process it starts from then on, whatever code they run:

1. **The profile.** An operation the sandbox mediates is done only if the profile allows it; a refused one fails (`EPERM`, or launchd's refusal of a lookup) and isn't done.
2. **For good.** No later call replaces the profile with another.

If `enter` returns `Err`, the process isn't restricted at all, and the caller is to end it.

## What is promised (Windows)

For a process `Command::spawn(Some(&policy))` returned `Ok` for, from its first instruction, whatever code it runs:

1. **The app container.** Its token is an app container's with no capabilities: an object is opened only if its security descriptor grants the container (or all application packages) the access. So no file of the user's is read, nothing is written anywhere the user alone may write, no socket reaches another machine or a port of this one, and no other process of the user's is opened.
2. **The job.** It is alone in a job of one process, with every user interface restriction; the job ends it when its handle closes (the `Child` dropped, or its starter gone) or an exception goes unhandled.
3. **The mitigations**, in the kernel's account of the process from its creation: no win32k system calls (unless `allow_win32k`), no file system control codes, no extension points, Microsoft-signed DLLs only (unless `allow_third_party_modules`), the system's fonts only, no remote or low-integrity images, no child processes.
4. **Its handles.** It has none of its starter's but those named with `inherit` and copies of the two standard streams. This holds for `spawn(None)` too.
5. **For good.** A process can't leave its app container or its job, raise its integrity level, or turn a mitigation off.

After `lower()` returns `Ok` in that process: its integrity level is untrusted, and a handle value that is no handle ends it.

If `spawn` returns `Err`, no process was started. If `lower` returns `Err`, the process has what `spawn` gave it and maybe part of the rest, and is to end.

## What is trusted

- **The kernel**: that Landlock and seccomp do what their documentation says. In particular: a Landlock domain and a seccomp filter are inherited by `clone`, `fork` and across `execve` and can't be removed; when several filters are installed all run and the strictest answer wins (`SECCOMP_RET_TRAP` over `SECCOMP_RET_ERRNO` over `SECCOMP_RET_ALLOW`); a filter is run before the system call's number is looked up, so a number the kernel doesn't have is refused as any other; a `SECCOMP_RET_TRAP` that can't be delivered because `SIGSYS` is blocked ends the process.
- **The `landlock` and `seccompiler` crates**: that the rules and the BPF program they build are the ones asked for. The crate's tests run real processes in real sandboxes for this reason: they test the kernel's answer, not the builder's output.
- **macOS's sandbox** (the kernel's `Sandbox.kext` and `libsystem_sandbox`): that a profile is compiled and enforced as its language says, for the whole process and its children, and that a second, different profile is refused. `sandbox_init_with_parameters` is undocumented; the tests run real processes under real profiles for this reason.
- **Windows**: that an app container's token, a job's limits and a process's mitigation policies are enforced as documented, and that `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` limits inheritance to the list. `CreateAppContainerToken` is exported by `kernelbase.dll` and in no header (Chromium's `AccessToken::CreateAppContainer` declares it the same way): that it returns a primary token of the container named, from the token given. The tests run real processes for this reason, and ask the kernel what each is in.
- **The caller's policy.** The crate enforces what the policy says, and a policy can say too much: allowing `ptrace`, `process_vm_writev`, `socket` or `execve`, or making a path readable that holds secrets, is the policy's doing.
- **The process before it enters.** What is already open stays open and usable: file descriptors, mapped memory, sockets. Entering first thing in `main` is what keeps this small; what is inherited from the parent is the parent's to choose.

## What is not promised

- **Metadata.** Landlock decides opens. `stat`, `access`, `readlink` and their kin answer for any path the process's user could see, so the process learns which files exist, their sizes and times. Only a file system the process can't see at all (a mount namespace, `chroot`) takes this away.
- **What Landlock doesn't decide.** A file's mode, owner, times and extended attributes change without an open (`chmod`, `chown`, `utimensat`, `setxattr`): promise 1 covers them only as far as the policy's system calls do. The same for truncation on a kernel before Landlock's third ABI (Linux 6.2): `truncate`, and `open` with `O_TRUNC` of a file the policy lets be read. A policy that names none of these calls, and fails `O_TRUNC`, is covered on every kernel; the tests show both limits.
- **Executable memory.** The crate doesn't look at `mmap`'s and `mprotect`'s protections unless the policy tests them, and a policy for a JIT can't refuse them: such a process, once subverted, runs the attacker's code, inside the sandbox.
- **The kernel's own bugs.** Every allowed system call is kernel code the process can reach. A shorter list is less of it.
- **What the allowed system calls do.** `write` to an inherited descriptor, `sendmsg` on an inherited socket, signals to itself, as much memory and CPU as the process's limits allow.
- **Other threads.** A thread running when `enter` is called would be outside; `enter` refuses to go on when the process has more than one, by `/proc/self/status`. A process with no `/proc` isn't entered at all (the count reads as 0).
- **The line on stderr.** It is a convenience. It is missing when the call was made with `SIGSYS` blocked, when stderr is closed, or when the process has replaced the handler.

### On macOS

- **What a profile doesn't name isn't always refused.** `(deny default)` leaves some operations allowed (another process's information; by Chromium's account also IOKit properties, NVRAM and distributed notifications): a profile is to deny them by name. The crate doesn't add these denials: the profile is the caller's whole text.
- **System calls.** The profile used here decides by kind of operation. Every system call and Mach trap is still reachable kernel code; what the sandbox doesn't mediate (memory, threads, clocks, what is already open) is as outside.
- **Whether a path exists.** A refused path fails with `EPERM`, one that isn't there with `ENOENT`.
- **What is already held.** Open descriptors and Mach ports the process has (its bootstrap port among them: lookups through it are what the profile decides) stay usable.
- **Executable memory**, as on Linux.

### On Windows

- **What every application may read.** An app container reads what the system grants all application packages: the system's directories, its fonts, other programs' installed files. It learns what is installed; it reads nothing of the user's.
- **Listening.** A socket may be bound and listened on; nothing can connect to it.
- **Whether a path exists**, where the directory it is in may be listed by all application packages.
- **Another desktop.** The process is on its starter's window station and desktop. With win32k's calls refused it can't use either; a process started with `allow_win32k` can send what the job's restrictions and its integrity level leave it.
- **Before `lower`.** From its start until it calls `lower`, a process is at low integrity, not untrusted: it may write what low-integrity processes may, where the app container doesn't already refuse it.
- **Which handles are named.** Each handle given to `inherit` is the starter's to hand on, with whatever access it has.
- **A console.** A process whose starter's output and errors are both a console's is attached to that console.
- **Executable memory**, as on Linux: dynamic code isn't prohibited (a JIT needs it).
- **The process's environment and command line** are what the starter gave it: the starter's whole environment unless it removed from it or cleared it (`env_clear`). That its starter can read its environment from outside (`environment`) is a way to check what it was given, not something kept from the process.

## How it holds

- **Order.** `enter` checks the thread count, builds every filter (a policy that makes none fails here, with nothing restricted), installs the `SIGSYS` handler, restricts the files, then installs the filters: the `errno` ones first, since each allows `seccomp`, and the one that traps last, since it refuses `seccomp` and `landlock_restrict_self` (unless the policy allows them, which only lets the process restrict itself further).
- **Allowed by list.** The trapping filter's default is the trap, so a system call added by a later kernel, or an architecture's other ABI (x32's numbers on x86-64; another `AUDIT_ARCH`, which `seccompiler`'s filter checks first), is refused without the crate knowing of it.
- **`errno` under the trap.** A call the policy fails is allowed by the trapping filter and failed by its own, and the stricter of the two answers, `errno`, is the result; a call in neither is trapped by the first whatever the others say. For a call failed only for some arguments (`fail_when`) and not otherwise named, the trapping filter allows it for those arguments alone, so that its other uses are still trapped.
- **`clone3`.** A filter can't read its arguments, so `allow_threads` fails it with `ENOSYS` and tests `clone`'s flags: `CLONE_THREAD` set and no `CLONE_NEW*`. With `CLONE_THREAD` the kernel requires `CLONE_SIGHAND` and `CLONE_VM`: the result is a thread of this process, under the same filters.
- **The handler isn't load-bearing.** `SECCOMP_RET_TRAP` skips the system call before the signal is sent. A process that blocks `SIGSYS` is ended by the kernel at the next trap; one that installs a handler that returns gets back a system call that did nothing.
- **Landlock's minimum.** The first ABI's file accesses are a hard requirement (`CompatLevel::HardRequirement`); what later ABIs add is best effort. `enter` then checks the status it gets back and returns `Error::NoLandlock` when nothing was enforced.

### On Windows

- **All at creation, or no process.** The token, the job, the mitigations and the handle list are arguments of the one `CreateProcessAsUserW` call; every one of them is built first, and a failure to build any returns before the call. There is no window in which the process runs with part of them, and no later step whose failure would leave it running with less: the process isn't suspended and patched, it is made whole.
- **The job, by the attribute.** The process is put in the job by `PROC_THREAD_ATTRIBUTE_JOB_LIST`, as it is made, not assigned after: it never runs outside. The job's only handle is the `Child`'s.
- **Only the list.** `bInheritHandles` is true only with a `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, which the system then takes as the whole of what is inherited. The standard streams are copies made inheritable for the call and closed after it; the originals are never made inheritable.
- **The command line.** Arguments are quoted by the C runtime's rules (`CommandLineToArgvW`'s), so that each arrives as one argument whatever is in it; a NUL in the program, an argument or the environment is refused, since it would end the string early.
- **`lower`.** The integrity level is lowered on the process's own primary token, which the system lets a process do and not undo.

## Each `unsafe`

| Where | What | Why it is sound |
| --- | --- | --- |
| `linux/sigsys.rs`, `install` | `mem::zeroed` for a `libc::sigaction`; `libc::sigaction` | All-zero is a valid `sigaction`. The handler has the `SA_SIGINFO` signature and makes only async-signal-safe calls; the old action isn't read. |
| `linux/sigsys.rs`, `on_sigsys` | Reads `si_code` and `si_syscall` through a `repr(C)` view of `siginfo_t` | The kernel passes an `SA_SIGINFO` handler a valid `siginfo_t`; the view is the kernel's layout of `_sigsys` on 64-bit Linux (three `int`s, padding, a pointer, an `int`, an `unsigned`), within `siginfo_t`'s size. Only integers are read. A `SIGSYS` sent by `kill` has other fields there: `si_code` is checked first and the number then isn't used. |
| `linux/sigsys.rs`, `on_sigsys` | `libc::write` of the message; `libc::syscall` of a number no kernel has; `libc::abort` | The buffer is a local array and the length never passes its end (`Message::text` stops at it). The raw system call takes no arguments and is refused by the filter. All three are async-signal-safe; the handler allocates nothing and takes no lock, and never returns. |
| `linux/probe.rs`, `no_core_dumps` | `libc::prctl(PR_SET_DUMPABLE, 0)` | Integer arguments only. |
| `linux/namespaces.rs`, `Namespaces::new` | `libc::getuid`, `libc::getgid` | They can't fail and take no argument. |
| `linux/namespaces.rs`, `Namespaces::enter`, `write_all` | `libc::unshare`; `libc::open`, `libc::write`, `libc::close` | Flags alone; a C string literal alive for the call; the bytes of a slice `self` holds, as far as its length; the descriptor opened just before, closed once. Nothing allocates and nothing takes a lock (`io::Error::last_os_error` holds the number alone), which is what the starter's child, between a fork of a process of many threads and the exec, may do. |
| `linux/namespaces.rs`, `Namespaces::available` | `libc::fork`; in the child `enter` and `libc::_exit`; `libc::waitpid` | The child makes only `enter`'s system calls and `_exit`, which runs nothing of the process's (no destructor, no exit handler): safe in a child of a process whose other threads may have held any lock. The parent waits for the pid it was just given, into a local. |
| `linux/probe.rs`, `system_call` | `libc::syscall` with the caller's number and arguments | Not sound for arbitrary arguments, and not meant to be: it is a test's process, which makes the one call and ends. The tests pass numbers and flags, and no pointer but null. It is public only through `probe::run`'s command line. |

| `macos.rs`, `enter` | `sandbox_init_with_parameters`; `CStr::from_ptr` and `sandbox_free_error` on its message | The profile and each parameter are `CString`s that outlive the call; the parameter array is name, value, name, value, null, as the function reads it. On failure the function leaves a string it allocated: copied, then freed once with the function meant for it. The declaration is Chromium's and Firefox's (`const char*`, `uint64_t`, `const char* const[]`, `char**`). |
| `macos.rs`, `sandboxed` | `sandbox_check(pid, NULL, 0)` | With no operation it reads only the pid; no pointer is passed but null. |
| `macos.rs`, `environment` | `sysctl` of `KERN_PROCARGS2` | The name is three integers and its length is theirs; the first call has no buffer and writes only the size; the second's buffer is that many bytes, alive across the call, and the kernel writes at most that many and how many it wrote. The answer is then read as bytes, by safe code (`environment_in`). |
| `macos/probe.rs`, `attempt` | `fork`, `kill` with signal 0, `proc_pidinfo`, `bootstrap_look_up`, `sysctlbyname`, `shm_open` | A test's process, one call each: integers, C strings that outlive the call, and out-parameters of the size passed. The forked child calls `_exit` alone. |
| `windows.rs`, `owned` | `OwnedHandle::from_raw_handle` | Called only with a handle a call that succeeded just returned for this process: nothing else has it, and it is closed once. |
| `windows.rs`, `Command::spawn` | `CreateProcessAsUserW`, `CreateProcessW` | Every string is NUL-terminated (a NUL inside is refused before) and alive for the call; the command line is this function's buffer to be written to; the environment block ends with an empty string; the startup structure is the extended one and says so in `cb` and the flags; the attribute list and each value it points to outlive the call (`Attributes<'a>`); the token is a primary token, open. |
| `windows.rs`, `Attributes` | `InitializeProcThreadAttributeList`, `UpdateProcThreadAttribute`, `DeleteProcThreadAttributeList` | The buffer is the size the first call asked for, aligned to 8, and never moves (a `Vec` that isn't grown). Each value is a slice of plain numbers or handles, `size_of_val` bytes, borrowed for `'a`, which the list doesn't outlive. Deleted once, in `drop`. |
| `windows.rs`, `standard_handle`, `inheritable_copy` | `GetStdHandle`, `GetFileType`, `DuplicateHandle` | No pointer but the out-parameter; a handle that isn't open is an error the code takes as "none". |
| `windows.rs`, `app_container_token` | `DeriveAppContainerSidFromAppContainerName`, `CreateAppContainerToken`, `FreeSid` | The name is NUL-terminated; the SID is the system's allocation, alive until freed once, after the token is made; the capabilities structure has a count of zero and a null list. |
| `windows.rs`, `lockdown_job` | `CreateJobObjectW`, `SetInformationJobObject` | No name or attributes; each structure is the one its information class reads, with its own size. |
| `windows.rs`, `Child` | `TerminateProcess`, `WaitForSingleObject`, `GetExitCodeProcess` | On the process's handle, which the `Child` owns. |
| `windows.rs`, `lower` | `CreateWellKnownSid`, `SetTokenInformation`, `SetDefaultDllDirectories`, `SetProcessMitigationPolicy` | The SID's buffer is the largest a SID is; the label points to it and both live across the call; the mitigation's structure is one word of flags. |
| `windows.rs`, `open`, `running`, `this_process`, `token_information`, `integrity`, `in_app_container`, `mitigation` | `OpenProcess`, `OpenProcessToken`, `GetTokenInformation`, `GetSidSubAuthority`, `GetProcessMitigationPolicy` | Buffers are sized by the call's own first answer and aligned to 8; the SID read is inside the buffer the call filled, and its last number is read only when its count is at least one. The pseudo-handle borrowed for `'static` is a constant. |
| `windows.rs`, `environment`, `read_from` | `OpenProcess`; `mem::zeroed` for a `PROCESS_BASIC_INFORMATION` and `NtQueryInformationProcess` of `ProcessBasicInformation`; `ReadProcessMemory` | All-zero is a valid structure of numbers and a pointer, and the pointer (the PEB's address in the other process) is only used as a number. The call writes the structure its class names, of the length passed. Each read's buffer is this process's own, writable for its length and alive across the call, which writes at most that many bytes and how many it wrote; the bytes are another process's, so they alias nothing here, and they are read as plain numbers by safe code (`environment_in`). A read short of the length is taken as none. The offsets into the parameters aren't documented: a wrong one reads the wrong numbers, which the size limit and the block's parsing take as data, not as anything trusted. |
| `windows/probe.rs`, `attempt` | `OpenProcess`; `GetHandleInformation` and `GetFileType` on a number from the command line | A test's process. The second is the test's point (is this number a handle here): the calls read nothing through it. |
| `windows/probe.rs`, `set_inheritable` | `SetHandleInformation` | On a borrowed, open handle; one flag. |

`names::name`, which the handler calls, reads `static` tables and nothing else.

## What the tests check, and don't

`tests/it/linux.rs` runs `sunaba_probe` in a real sandbox for each claim above that a process can see from inside: files inside and outside the policy, writing and removing, a single file, a path made late, a system call not named, one failed (two `errno`s), argument tests (either of two; 32 and 64 bits), a call failed for some arguments, threads but no processes or new namespaces, a thread made later being inside, the kernel's own `Seccomp`, `Seccomp_filters` and `NoNewPrivs`, and a second thread refusing entry. Three tests state limits: `stat` outside the policy succeeds; a policy that allows `chmod` allows it anywhere; an `open` with `O_TRUNC` is for the policy to fail.

`linux/namespaces.rs`'s own tests check the maps and that a process of many threads is refused; machopo's tests start a real child in its namespaces (it is in user and network namespaces other than its starter's, with loopback alone and no capability, the same user) and, in a user namespace whose processes may make none, see `available` say no.

Not tested, checked by review: the handler's async-signal-safety; that `Namespaces::enter` allocates nothing between a fork and an exec; a kernel without Landlock, or with it turned off (the path is `Error::NoLandlock`; it needs such a kernel); the best-effort Landlock restrictions (they are behind system calls the tests' policy refuses anyway); architectures other than x86-64.

`tests/it/macos.rs` does the same on macOS: the file system refused by default (read, list, metadata), a directory allowed for reading and the rest still refused, nothing written, removed or changed where it can be read, metadata as its own permission, a parameter's value not read as rules, no socket, no process, signals, another process's information, a Mach service by name, a `sysctl` by name, no shared memory, a thread made before or after entering, a second profile refused, a profile that doesn't compile, and a sandboxed process seen from outside (that it is in a sandbox; the environment it was started with, and none for one that has ended). Two tests state limits: a missing path is told from a refused one, and `(deny default)` alone lets another process be asked about.

Not tested on macOS, checked by review: operations a profile could allow that the probe has no attempt for (IOKit, NVRAM, distributed notifications, `process-exec` of an allowed path keeping the profile).

`tests/it/windows.rs` does the same on Windows, where the probe starts itself under the policy: the app container and both integrity levels; the user's files not read, listed or looked at, none made, changed or removed; the system's read; no connection off the machine or to it (a listener that hears nothing); no program started; no other process opened; each mitigation by the kernel's account, absent with no policy, and the two a policy may allow; only the named handle inherited; a bad handle fatal once lowered; the process ended with its starter's `Child`; arguments and environment as given; a sandboxed process's environment seen from outside (only what was set after it was cleared, and none once it has ended); two containers at once.

Not tested on Windows, checked by review: the transaction manager's filter (nothing reads it back); DEP, SEHOP, heap termination and ASLR (on in any 64-bit process, or with no account to read); that the job's user interface restrictions are set (the process can't call win32k to try them); a failure of `CreateAppContainerToken` or of an attribute (it needs a Windows without them); the console path of the standard streams (a test's are pipes); Windows 10.

## Reviewing a change

- A new `unsafe`: a `// SAFETY:` comment and a row in the table above.
- Anything in `on_sigsys` or what it calls: still no allocation, no lock, no formatting machinery, and it still never returns.
- The order in `enter`: nothing that needs a system call the trapping filter refuses may come after that filter, and nothing is restricted before the checks that can fail cleanly.
- A change to how filters are built: the trapping filter's default must stay the trap, and every failed call must stay allowed in it.
- Raising the `landlock` crate's version: `NEWEST` follows its newest ABI, and what that ABI adds is read to see that best effort is still right for it.
- macOS: the declaration of `sandbox_init_with_parameters` against Chromium's `sandbox/mac/seatbelt.cc` when a new macOS is supported; the parameter array stays null-terminated and its strings alive across the call.
- Windows: nothing may be added between making the process and its being whole (no suspend-and-patch step that can fail with the process alive); a new attribute's value must be borrowed for the list's lifetime; a new standard stream or handle source must go through a copy, never by making the original inheritable; the declaration of `CreateAppContainerToken` against Chromium's `base/win/access_token.cc` when a new Windows is supported.
