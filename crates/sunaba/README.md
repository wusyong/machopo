# sunaba

A process's sandbox, entered by the process itself as it starts. A process that is about to handle what it can't trust (a web page, a file from elsewhere) says what it will still need of the kernel, a policy, and enters it; from then on the kernel refuses the rest, whatever the process's own code has been made to do.

The crate is the mechanism alone and knows no program's policy (Chromium's split: `sandbox/` and `sandbox/policy/`). It depends on nothing of the workspace it sits in.

| Operating system | Mechanism | Module |
| --- | --- | --- |
| Linux | Landlock (which files open), seccomp-bpf (which system calls are made) | `sunaba::linux` |
| macOS | The kernel's sandbox (Seatbelt) under a profile: which files, Mach services and `sysctl`s, and no sockets or processes | `sunaba::macos` |

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
- **`allow_threads()`**: threads and nothing else. `clone3` returns `ENOSYS` (its arguments are in memory), and `clone` is allowed with `CLONE_THREAD` and no new namespace.
- **Anything else ends the process**, by `SIGSYS`, after one line on stderr: `sunaba: system call 41 (socket) isn't allowed in this sandbox`. The line is missing when the call was made with signals blocked (libc does so around `clone`): the process ends by `SIGSYS` all the same.

The file system calls (`openat`, `unlinkat`…) are for the policy to allow: Landlock then decides by path, and a refusal there is `EACCES`, not the process's end. Not all of them: Landlock doesn't decide a file's mode, owner, times or extended attributes (`chmod`, `chown`, `utimensat`, `setxattr`…), and before its third ABI (Linux 6.2) not truncation either (`truncate`, and `open` with `O_TRUNC` of a file that can be read). A policy leaves those calls out, or fails them (`fail`, `fail_when`).

### Entering

`enter` is for the first thing in `main`:

- **One thread.** Both restrictions are the calling thread's and those of what it makes later. With a second thread already running, `enter` restricts nothing and returns `Error::Threads`.
- **For good.** Nothing takes a restriction off; a policy is everything the process will ever need.
- **An error means end the process.** `Error::Seccomp` can come after the files were restricted.

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

