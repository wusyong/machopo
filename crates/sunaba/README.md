# sunaba

A process's sandbox, entered by the process itself as it starts. A process that is about to handle what it can't trust (a web page, a file from elsewhere) says what it will still need of the kernel, a policy, and enters it; from then on the kernel refuses the rest, whatever the process's own code has been made to do. On Windows the sandbox is given to a process by whoever starts it, so there the crate starts the process.

The crate is the mechanism alone and knows no program's policy (Chromium's split: `sandbox/` and `sandbox/policy/`). It depends on nothing of the workspace it sits in.

| Operating system | Mechanism | Module |
| --- | --- | --- |
| Linux | Landlock (which files open), seccomp-bpf (which system calls are made) | `sunaba::linux` |
| macOS | The kernel's sandbox (Seatbelt) under a profile: which files, Mach services and `sysctl`s, and no sockets or processes | `sunaba::macos` |
| Windows | An app container with no capabilities, made from a lockdown restricted token (no files of the user's or the system's, no sockets, no other process), a job (one process, no user interface), another window station and desktop, the process's mitigations (no win32k, no DLL but the system's); given as the process is started | `sunaba::windows` |

Why it holds, what it doesn't promise, and what to review when changing it: `soundness.md`.

## Linux

```rust
use sunaba::linux::{Arg, Policy, Test, enter};

fn main() {
    let mut policy = Policy::new();
    policy
        .read_only("/usr/share/fonts")
        .allow(&[libc::SYS_read, libc::SYS_write, libc::SYS_openat, libc::SYS_mmap /* ... */])
        .allow_when(libc::SYS_ioctl, &[Arg::int(1, Test::Equals(libc::TCGETS as u64))])
        .fail(libc::SYS_statx, libc::ENOSYS)
        .allow_threads();
    if let Err(error) = enter(&policy) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    // From here on: the policy.
}
```

### The policy

- **`read_only(path)`**: the file, or everything beneath the directory, can be opened for reading. Nothing else can be opened, and nothing can be written, made, removed or renamed anywhere. A path that isn't there when the process enters is left out. What was opened before entering stays open: a file the process is to write is opened first (or handed to it).
- **`allow(calls)`**: these system calls, whatever their arguments.
- **`allow_when(call, args)`**: the call when every `Arg` holds; said again for the same call, when either set holds. `Arg::int` tests an argument's low 32 bits (what the kernel reads of an `int`), `Arg::long` all 64. A filter reads the arguments' values and never the memory a pointer leads to.
- **`fail(call, errno)`**: the call isn't made and returns `errno`: for what a library tries first and has another way without.
- **`fail_when(call, args, errno)`**: the same, only when every `Arg` holds; the call's other uses are as the policy says elsewhere. For a call that is fine but for some flags: `openat`, but not with `O_TRUNC`.
- **`chroot()`**: where the process may (it holds `CAP_SYS_CHROOT`: a child started in `Namespaces::keeping_chroot()`), its root becomes a directory that no longer exists, as Chromium's `ChrootToSafeEmptyDir` makes it: a thread chroots into its own `/proc/thread-self/fdinfo` and ends, and the directory with it. No path leads anywhere, not even to say whether a file is there (`ESRCH`, for any path but `/`), and `/proc` is out of reach. Where it may not, the process goes without. A policy that chroots reads no path (`enter` returns `Error::Policy`).
- **`allow_threads()`**: threads and nothing else. `clone3` returns `ENOSYS` (its arguments are in memory), and `clone` is allowed with `CLONE_THREAD` and no new namespace.
- **Anything else ends the process**, by `SIGSYS`, after one line on stderr: `sunaba: system call 41 (socket) isn't allowed in this sandbox`. The line is missing when the call was made with signals blocked (libc does so around `clone`): the process ends by `SIGSYS` all the same.

The file system calls (`openat`, `unlinkat`…) are for the policy to allow: Landlock then decides by path, and a refusal there is `EACCES`, not the process's end. Not all of them: Landlock doesn't decide a file's mode, owner, times or extended attributes (`chmod`, `chown`, `utimensat`, `setxattr`…), and before its third ABI (Linux 6.2) not truncation either (`truncate`, and `open` with `O_TRUNC` of a file that can be read). A policy leaves those calls out, or fails them (`fail`, `fail_when`).

### Entering

`enter` is for the first thing in `main`. In order: it checks there is one thread, has glibc read the time zone (`tzset`: glibc reads the zone `TZ` names, or `/etc/localtime`, once, and not again while `TZ` stays, so it has it whatever can be read later), makes the policy's chroot, gives up every capability the process has (ambient ones too), then restricts the files and the system calls.

- **One thread.** Both restrictions are the calling thread's and those of what it makes later. With a second thread already running, `enter` restricts nothing and returns `Error::Threads`.
- **For good.** Nothing takes a restriction off; a policy is everything the process will ever need.
- **An error means end the process.** `Error::Seccomp` can come after the files were restricted.

### Namespaces of a child's own

A process may also be started in user and network namespaces of its own, by whoever starts it: `Namespaces::new()` before the fork (it writes the maps then, so that nothing is allocated after), `namespaces.enter()` in the child between the fork and the exec (`CommandExt::pre_exec`). The child has no network interface but loopback. Its user and group ids map to themselves, so it is the same user inside, not root, and the exec takes every capability the new user namespace gave it; with `keeping_chroot()`, all but `CAP_SYS_CHROOT`, kept as an ambient capability for its policy's `chroot()`, which `enter` then drops with the rest.

Not every machine allows a program that isn't privileged to make them: a kernel may be built or set without them, and Ubuntu (since 23.10) lets only the programs its AppArmor profiles name. `namespaces.available()` tries in a child of its own, which ends at once, and says whether a child started with them would start. A starter that is told no goes without them: the process's own `enter` restricts it all the same.

### What the kernel has to have

Landlock (Linux 5.13, built with `CONFIG_SECURITY_LANDLOCK` and named in the boot's `lsm=`) and seccomp filters. Without Landlock `enter` returns `Error::NoLandlock` and restricts nothing: there is no weaker sandbox to fall back to. A newer kernel's Landlock also refuses TCP's `bind` and `connect`, abstract Unix sockets and signals to other processes; these are asked for and taken where they are, and a policy's seccomp list is what refuses them everywhere.

### Testing a policy

What a sandbox refuses can only be seen from inside one, and a process can't leave to try the next thing. `sunaba::linux::probe::run(&policy, arguments)` is a `main` that enters the policy, tries the one thing its arguments say (read, list, write, remove or stat a path; a raw system call; any of these on a new thread) and prints `ok N` or `error ERRNO`. A test runs it once for each thing and reads the line or the signal. `probe::baseline()` is the policy the probe's own code needs, for a test to add to.

The crate's own tests (`tests/it/linux.rs`) run `sunaba_probe`, the baseline with what its arguments add:

```sh
cargo test -p sunaba
target/debug/sunaba_probe --read-only /etc -- read /etc/hostname      # ok 7
target/debug/sunaba_probe -- read /etc/hostname                       # error 13
target/debug/sunaba_probe -- syscall 41 2 1 0                         # killed by SIGSYS
```

## macOS

```rust
use sunaba::macos::{Profile, enter};

fn main() {
    let mut profile = Profile::new(
        r#"(version 1)
(deny default)
(allow signal (target self))
(allow file-read* (subpath (param "FONTS")))
(allow mach-lookup (global-name "com.apple.fonts"))
(allow sysctl-read (sysctl-name "hw.ncpu"))
"#,
    );
    profile.parameter("FONTS", "/System/Library/Fonts");
    if let Err(error) = enter(&profile) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    // From here on: the profile.
}
```

### The profile

A profile is a program in the sandbox's own language (`sandbox(7)`'s, a Scheme: Apple's own profiles are in `/System/Library/Sandbox/Profiles`). It allows by kind of operation, not by system call: files by path (`file-read*`, `file-read-metadata`, `file-write*`), Mach services by name (`mach-lookup`), `sysctl`s by name, signals by target, sockets (`network*`), processes (`process-fork`, `process-exec`), shared memory (`ipc-posix-shm*`).

- **Start from `(deny default)`** and allow what the process needs.
- **`(deny default)` leaves some operations allowed.** What the kernel says of another process of the same user is one (`process-info*`); Chromium's profiles also name `iokit-get-properties`, `nvram*` and `distributed-notification-post`. A profile denies these by name, before what it allows of them.
- **`parameter(name, value)`** is what `(param "name")` reads in the profile: for what is only known as the process runs (a directory, a process's id). A value is a string to the profile whatever is in it, so it is never spliced into the profile's text.
- **Paths are as the kernel resolves them**: `/etc/hosts` is `/private/etc/hosts` to a rule, and following the link `/etc` takes `file-read-metadata` of `/etc` itself.
- **A refusal is an error, not the process's end**: `EPERM` from the call, and from launchd `BOOTSTRAP_NOT_PRIVILEGED` (1100) for a service's lookup. The kernel logs each: `log stream --predicate 'sender == "Sandbox"'` shows the process, the operation and what it was on, which is how a profile is written (run under `(deny default)`, read what was refused, decide of each).
- **A path that isn't there is `ENOENT`**, a refused one `EPERM`: a process told nothing of a path's metadata can still tell whether the path exists.

### Entering

`enter` is for the first thing in `main`, and unlike Linux's it is the whole process's, whatever threads it has already.

- **For good.** Entering again with another profile is refused, looser or stricter (the same profile again is taken, and changes nothing).
- **An error means end the process**: the profile didn't compile (`Error::Refused`, with the system's message) or held a NUL (`Error::Nul`). Nothing was restricted.
- **`entered()`** says whether this process is in a sandbox; **`sandboxed(pid)`** whether a running process is, for whoever started it to check.
- **`environment(pid)`** is the environment a running process was started with, by the kernel's account (`KERN_PROCARGS2`), in a sandbox or out of one: for whoever started it to check what it was given. What the process changed since isn't seen; the system's own executables' is empty (the kernel keeps it from other processes).

`sandbox_init_with_parameters` is in no public header, and `sandbox_init` is deprecated in the ones it is in: the system's own daemons, Chromium and Firefox all enter this way, and there is no other for a process that isn't an App Store app.

### Testing a profile

`sunaba::macos::probe::run(&profile, arguments)` is a `main` that enters the profile, tries the one thing its arguments say (read, list, write, remove, stat or chmod a path; a TCP or Unix socket; a program or a fork; a signal; another process's information; a Mach service's lookup; a `sysctl`; POSIX shared memory; any of these on a new thread) and prints `ok N` or `error ERRNO`. `probe::attempt_and_exit` is the same for a program that has entered its own way.

The crate's own tests (`tests/it/macos.rs`) run `sunaba_probe`, `(deny default)` with the rules its arguments add:

```sh
cargo test -p sunaba
target/debug/sunaba_probe -- read /private/etc/hosts                                               # error 1
target/debug/sunaba_probe --rule '(allow file-read* (subpath "/private/etc"))' -- read /private/etc/hosts   # ok 213
target/debug/sunaba_probe -- lookup com.apple.fonts                                                # error 1100
```


## Windows

What restricts a process on Windows is given when the process is made: its token, its job, its mitigations, the handles it inherits. So the starter does it, and the started process only lowers itself the rest of the way.

```rust
use sunaba::windows::{Command, Policy};

// The starter.
let mut command = Command::new(std::env::current_exe()?);
command.arg("--worker").inherit(pipe.as_handle());
let mut child = command.spawn(Some(&Policy::new("example.worker.1")))?;

// The started process, first thing in `main`.
if let Err(error) = sunaba::windows::lower() {
    eprintln!("{error}");
    std::process::exit(1);
}
```

### Starting a process

`Command` is a program, its arguments, its whole environment (this process's, with what `env` and `env_remove` change; after `env_clear`, only what is set after it), its directory, and the handles of this process's it inherits (`inherit`; each has to be inheritable). `Command::from_std` takes those from a standard library's command. `spawn(None)` starts it as usual, except that it has no handle of the starter's but the ones named, where the standard library's lets it have every inheritable one: that is all the crate adds for a process with no sandbox, and a reason to start one here.

The started process's output and errors are the starter's where those are a file or a pipe (`stderr` hands it another); its input is nothing. Where neither is, it is left to its console's.

`Child` is the process: `id`, `kill`, `wait`, `try_wait`, `exceeded_memory_limit`. One started under a policy ends when its `Child` is dropped.

### The policy

`Policy::new(name)` is the whole of it; the name is the app container's, and a process given a name no other has is in a container of its own. Nothing is registered under the name. A process started under it:

- is in an **app container** with no capabilities, whose token is made from a **lockdown** restricted token (Chromium's `USER_LOCKDOWN`: every group only denies, and only what is granted to the null SID is granted): it opens no file, the user's or the system's (not `C:\Windows` or its fonts either), makes no socket (Winsock doesn't start), opens no other process and nothing another process under a policy made, and can't read its own token. What it makes is its user's and its app container's alone, not its logon session's, on any of its threads (the default DACL of both its tokens is Chromium's lockdown one, set before they are made app containers');
- has a first thread that starts with a token of the same app container's that may do what an app container may (Chromium's `USER_RESTRICTED_SAME_ACCESS`), until `lower` gives it back: the loader maps the DLLs the program starts with on that thread alone (the process's `LoaderThreads` is 1), since the lockdown token opens no DLL that isn't a known one;
- is in a **job** of one process with every user interface restriction, which ends it when the job's handle closes or an exception goes unhandled, and, with `limit_memory(bytes)`, commits no more than that for it: what it asks for beyond is refused (an allocation fails, as when the machine has no more), and its `Child` says so once it has ended (`exceeded_memory_limit`, from the job's completion port; it may have gone on, and ended for another reason). Only committed memory counts, not what it reserves;
- is on **another window station and desktop** (Chromium's alternate ones): the starter's first start under a policy makes them, and keeps them until it ends, for every later one. Their DACLs are the starter's token's default, which grants an app container nothing: a process that did reach win32k couldn't connect to them. The window station has no name, so it is the logon session's one; the desktop is named after the starter (`sunaba-<pid>`);
- has **no compatibility shim**: the system's choice for the executable (`pShimData`, in its PEB) is written 0 before it runs, so the loader maps neither `apphelp.dll` nor a shim's DLL (Chromium's zero app shim; not where the executable runs emulated, x64 on ARM64);
- has, from its first instruction, Chromium's **mitigations** for a renderer: no calls into win32k, no file system control codes, no extension points, only DLLs Microsoft signed (and module tampering protection: a clean copy of the executable if its imports were changed before it started), only the system's fonts in the kernel, no DLL from another machine or from where a low-integrity process could write, no child processes, branch prediction kept to itself, no transaction manager, and DEP, SEHOP, heap termination and bottom-up ASLR.

Two things a program may need of it: `allow_win32k` for one that loads `user32.dll` or `gdi32.dll` as it starts (it doesn't start otherwise; link them to be loaded on first use instead, `/DELAYLOAD`), which is then left on its starter's desktop, and `allow_third_party_modules` for a machine where other software puts its DLL into every process.

### Lowering

`lower()`, first thing in the started process's `main`, on its first thread: its integrity level goes from low to untrusted, DLLs are looked for in the system's directories and its own, the system makes one random number (the first time loads what makes them), the handles that opened to the kernel's security device (`\Device\KsecDD`) are closed (Chromium's handle closer), a handle that isn't one then ends the process, and the first thread gives back the token it started with. A closed handle's slot holds a copy of an event's handle, kept for the process's life, so that code that kept the number reaches an event and not a handle made later. None of it can be undone. On an error the process is to end.

From then on the process loads no DLL. A program linked to load some DLLs on first use (`/DELAYLOAD`) that calls into one ends with the delay-load helper's exception (`0xC06D007E`); the crate defines the helper's failure hook (`__pfnDliFailureHook2`, MSVC's), which first says on the process's errors which DLL and function it was. Whatever the process needs of a DLL is to be loaded before `lower`.

### Looking from outside

`open(id)` and then `in_app_container`, `integrity`, `mitigation` and `running` say what a process is in, for the starter's tests of the real thing; `AccessToken::of` is its token's user, groups (which only deny), restricting SIDs, app container and default DACL. `environment(id)` is a process's environment as it is now, by its PEB (64-bit processes): what its starter gave it, less what the process removed since; `loader_threads(id)` is how many threads its loader was told to use; `shim_data(id)` is its `pShimData`; `desktop(id)` is the window station and desktop it was started on (`station\desktop`); `handles(id)` is the handles it has: each one's value, kind (`File`, `Event`…), name (a file's is its device's; not asked of a pipe) and how many handles its object has; `None` for one that has ended or can't be read. `physical_memory()` is how much the machine has, for a starter's limit. `alternate_desktop_name()` is the starter's own: the window station and desktop it starts processes under a policy on, once it has started one.

The crate calls `user32.dll` in the starter, to make those. A program that links the crate and is itself started under a policy is to load `user32.dll` on first use (`/DELAYLOAD`), as `sunaba_probe` does (`build.rs`), or it doesn't start with win32k's calls turned off.

### Testing a policy

`sunaba::windows::probe::attempt_and_exit(arguments)` is the started process's `main` after `lower`: it tries the one thing its arguments say (read, list, write, remove or stat a path; a TCP connection or a listener; a program; another process; whether a number is a handle; committing memory; a random number; its own integrity level, container and mitigations; any of these on a new thread) and prints `ok N` or `error CODE`.

The crate's own tests (`tests/it/windows.rs`) run `sunaba_probe`, which starts itself under the whole policy, less what its arguments say:

```sh
cargo test -p sunaba
target/debug/sunaba_probe -- read C:\Users\me\notes.txt                # error 5
target/debug/sunaba_probe --no-policy -- read C:\Users\me\notes.txt    # ok 120
target/debug/sunaba_probe -- connect 192.0.2.1:80                       # error 10107
target/debug/sunaba_probe -- integrity                                  # error 5
target/debug/sunaba_probe --no-lower -- integrity                       # ok 4096
```
