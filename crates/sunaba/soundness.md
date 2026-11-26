# sunaba's soundness

What the crate promises, what it trusts, what it doesn't promise, each `unsafe`, and what to review when changing it. The API is in `README.md`.

## What is promised (Linux)

After `sunaba::linux::enter(&policy)` returns `Ok`, for the calling thread and every thread and process it makes from then on, whatever code they run:

1. **Files.** No file or directory is opened except for reading, at or beneath a path of the policy's `read_only` that existed at the time. Nothing is written, truncated, made, removed, renamed or linked through a path, and no file is executed.
2. **System calls.** A system call is made only if the policy allows it (for its arguments, where it tests them). One the policy fails returns its `errno` and isn't made. Any other isn't made, and the thread gets `SIGSYS`.
3. **For good.** No later call loosens either: Landlock rules and seccomp filters only stack, and the process has `no_new_privs`.

If `enter` returns `Err`, nothing is promised, and the caller is to end the process (`Error::Threads`, `Error::Policy`, `Error::Signal` and `Error::NoLandlock` are returned before anything is restricted; `Error::Landlock` and `Error::Seccomp` may come after part of it).

## What is promised (macOS)

After `sunaba::macos::enter(&profile)` returns `Ok`, for every thread of the process and every process it starts from then on, whatever code they run:

1. **The profile.** An operation the sandbox mediates is done only if the profile allows it; a refused one fails (`EPERM`, or launchd's refusal of a lookup) and isn't done.
2. **For good.** No later call replaces the profile with another.

If `enter` returns `Err`, the process isn't restricted at all, and the caller is to end it.

## What is trusted

- **The kernel**: that Landlock and seccomp do what their documentation says. In particular: a Landlock domain and a seccomp filter are inherited by `clone`, `fork` and across `execve` and can't be removed; when several filters are installed all run and the strictest answer wins (`SECCOMP_RET_TRAP` over `SECCOMP_RET_ERRNO` over `SECCOMP_RET_ALLOW`); a filter is run before the system call's number is looked up, so a number the kernel doesn't have is refused as any other; a `SECCOMP_RET_TRAP` that can't be delivered because `SIGSYS` is blocked ends the process.
- **The `landlock` and `seccompiler` crates**: that the rules and the BPF program they build are the ones asked for. The crate's tests run real processes in real sandboxes for this reason: they test the kernel's answer, not the builder's output.
- **macOS's sandbox** (the kernel's `Sandbox.kext` and `libsystem_sandbox`): that a profile is compiled and enforced as its language says, for the whole process and its children, and that a second, different profile is refused. `sandbox_init_with_parameters` is undocumented; the tests run real processes under real profiles for this reason.
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

## How it holds

- **Order.** `enter` checks the thread count, builds every filter (a policy that makes none fails here, with nothing restricted), installs the `SIGSYS` handler, restricts the files, then installs the filters: the `errno` ones first, since each allows `seccomp`, and the one that traps last, since it refuses `seccomp` and `landlock_restrict_self` (unless the policy allows them, which only lets the process restrict itself further).
- **Allowed by list.** The trapping filter's default is the trap, so a system call added by a later kernel, or an architecture's other ABI (x32's numbers on x86-64; another `AUDIT_ARCH`, which `seccompiler`'s filter checks first), is refused without the crate knowing of it.
- **`errno` under the trap.** A call the policy fails is allowed by the trapping filter and failed by its own, and the stricter of the two answers, `errno`, is the result; a call in neither is trapped by the first whatever the others say. For a call failed only for some arguments (`fail_when`) and not otherwise named, the trapping filter allows it for those arguments alone, so that its other uses are still trapped.
- **`clone3`.** A filter can't read its arguments, so `allow_threads` fails it with `ENOSYS` and tests `clone`'s flags: `CLONE_THREAD` set and no `CLONE_NEW*`. With `CLONE_THREAD` the kernel requires `CLONE_SIGHAND` and `CLONE_VM`: the result is a thread of this process, under the same filters.
- **The handler isn't load-bearing.** `SECCOMP_RET_TRAP` skips the system call before the signal is sent. A process that blocks `SIGSYS` is ended by the kernel at the next trap; one that installs a handler that returns gets back a system call that did nothing.
- **Landlock's minimum.** The first ABI's file accesses are a hard requirement (`CompatLevel::HardRequirement`); what later ABIs add is best effort. `enter` then checks the status it gets back and returns `Error::NoLandlock` when nothing was enforced.

## Each `unsafe`

| Where | What | Why it is sound |
| --- | --- | --- |
| `linux/sigsys.rs`, `install` | `mem::zeroed` for a `libc::sigaction`; `libc::sigaction` | All-zero is a valid `sigaction`. The handler has the `SA_SIGINFO` signature and makes only async-signal-safe calls; the old action isn't read. |
| `linux/sigsys.rs`, `on_sigsys` | Reads `si_code` and `si_syscall` through a `repr(C)` view of `siginfo_t` | The kernel passes an `SA_SIGINFO` handler a valid `siginfo_t`; the view is the kernel's layout of `_sigsys` on 64-bit Linux (three `int`s, padding, a pointer, an `int`, an `unsigned`), within `siginfo_t`'s size. Only integers are read. A `SIGSYS` sent by `kill` has other fields there: `si_code` is checked first and the number then isn't used. |
| `linux/sigsys.rs`, `on_sigsys` | `libc::write` of the message; `libc::syscall` of a number no kernel has; `libc::abort` | The buffer is a local array and the length never passes its end (`Message::text` stops at it). The raw system call takes no arguments and is refused by the filter. All three are async-signal-safe; the handler allocates nothing and takes no lock, and never returns. |
| `linux/probe.rs`, `no_core_dumps` | `libc::prctl(PR_SET_DUMPABLE, 0)` | Integer arguments only. |
| `linux/probe.rs`, `system_call` | `libc::syscall` with the caller's number and arguments | Not sound for arbitrary arguments, and not meant to be: it is a test's process, which makes the one call and ends. The tests pass numbers and flags, and no pointer but null. It is public only through `probe::run`'s command line. |

| `macos.rs`, `enter` | `sandbox_init_with_parameters`; `CStr::from_ptr` and `sandbox_free_error` on its message | The profile and each parameter are `CString`s that outlive the call; the parameter array is name, value, name, value, null, as the function reads it. On failure the function leaves a string it allocated: copied, then freed once with the function meant for it. The declaration is Chromium's and Firefox's (`const char*`, `uint64_t`, `const char* const[]`, `char**`). |
| `macos.rs`, `sandboxed` | `sandbox_check(pid, NULL, 0)` | With no operation it reads only the pid; no pointer is passed but null. |
| `macos/probe.rs`, `attempt` | `fork`, `kill` with signal 0, `proc_pidinfo`, `bootstrap_look_up`, `sysctlbyname`, `shm_open` | A test's process, one call each: integers, C strings that outlive the call, and out-parameters of the size passed. The forked child calls `_exit` alone. |

`names::name`, which the handler calls, reads `static` tables and nothing else.

## What the tests check, and don't

`tests/it/linux.rs` runs `sunaba_probe` in a real sandbox for each claim above that a process can see from inside: files inside and outside the policy, writing and removing, a single file, a path made late, a system call not named, one failed (two `errno`s), argument tests (either of two; 32 and 64 bits), a call failed for some arguments, threads but no processes or new namespaces, a thread made later being inside, the kernel's own `Seccomp`, `Seccomp_filters` and `NoNewPrivs`, and a second thread refusing entry. Three tests state limits: `stat` outside the policy succeeds; a policy that allows `chmod` allows it anywhere; an `open` with `O_TRUNC` is for the policy to fail.

Not tested, checked by review: the handler's async-signal-safety; a kernel without Landlock, or with it turned off (the path is `Error::NoLandlock`; it needs such a kernel); the best-effort Landlock restrictions (they are behind system calls the tests' policy refuses anyway); architectures other than x86-64.

`tests/it/macos.rs` does the same on macOS: the file system refused by default (read, list, metadata), a directory allowed for reading and the rest still refused, nothing written, removed or changed where it can be read, metadata as its own permission, a parameter's value not read as rules, no socket, no process, signals, another process's information, a Mach service by name, a `sysctl` by name, no shared memory, a thread made before or after entering, a second profile refused, a profile that doesn't compile, and a sandboxed process seen from outside. Two tests state limits: a missing path is told from a refused one, and `(deny default)` alone lets another process be asked about.

Not tested on macOS, checked by review: operations a profile could allow that the probe has no attempt for (IOKit, NVRAM, distributed notifications, `process-exec` of an allowed path keeping the profile).

## Reviewing a change

- A new `unsafe`: a `// SAFETY:` comment and a row in the table above.
- Anything in `on_sigsys` or what it calls: still no allocation, no lock, no formatting machinery, and it still never returns.
- The order in `enter`: nothing that needs a system call the trapping filter refuses may come after that filter, and nothing is restricted before the checks that can fail cleanly.
- A change to how filters are built: the trapping filter's default must stay the trap, and every failed call must stay allowed in it.
- Raising the `landlock` crate's version: `NEWEST` follows its newest ABI, and what that ABI adds is read to see that best effort is still right for it.
- macOS: the declaration of `sandbox_init_with_parameters` against Chromium's `sandbox/mac/seatbelt.cc` when a new macOS is supported; the parameter array stays null-terminated and its strings alive across the call.
