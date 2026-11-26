//! The renderer process's sandbox: a real renderer's threads are in it by
//! the kernel's account, and what its policy refuses is refused, tried
//! from inside by the executable itself (`--type=renderer-sandbox-probe`:
//! it enters as a renderer process does, then tries one thing).

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Command;

use crate::automation_pipe::{Browser, script};
use crate::renderer_process::{MACHOPO, RunningRenderer};

/// `name`'s number in every thread's `/proc/<pid>/task/<tid>/status`.
fn of_every_thread(pid: u32, name: &str) -> Vec<u32> {
    let tasks = std::fs::read_dir(format!("/proc/{pid}/task")).expect("the process's threads");
    tasks
        .map(|task| {
            let status = std::fs::read_to_string(task.unwrap().path().join("status")).unwrap();
            status
                .lines()
                .find_map(|line| {
                    line.strip_prefix(name)?
                        .strip_prefix(':')?
                        .trim()
                        .parse()
                        .ok()
                })
                .unwrap_or_else(|| panic!("no {name} in {status}"))
        })
        .collect()
}

#[test]
fn every_thread_of_a_renderer_process_is_in_the_sandbox() {
    let renderer = RunningRenderer::start(&["--test-fonts"]);
    let pid = renderer.child.id();
    let filtered = of_every_thread(pid, "Seccomp");
    // The main thread, the IO thread, the compositor, and V8's.
    assert!(filtered.len() > 3, "{filtered:?}");
    // 2: a filter.
    assert!(filtered.iter().all(|mode| *mode == 2), "{filtered:?}");
    let no_new_privs = of_every_thread(pid, "NoNewPrivs");
    assert!(no_new_privs.iter().all(|set| *set == 1), "{no_new_privs:?}");
    assert_eq!(renderer.browser_goes(), Some(0));
}

#[test]
fn a_renderer_process_told_no_sandbox_is_in_none() {
    let renderer = RunningRenderer::start(&["--no-sandbox"]);
    let filtered = of_every_thread(renderer.child.id(), "Seccomp");
    assert!(filtered.len() > 3, "{filtered:?}");
    assert!(filtered.iter().all(|mode| *mode == 0), "{filtered:?}");
    assert_eq!(renderer.browser_goes(), Some(0));
}

/// The system's fonts are found before the process enters and handed to
/// the renderer: in the sandbox it lays text out as wide as outside it,
/// with the same fonts. (Found inside, there would be none: the system's
/// list of them can't be read there.)
#[test]
fn a_sandboxed_renderer_lays_text_out_with_the_systems_fonts() {
    if shell::renderer_sandbox::font_directories().is_empty() {
        // A machine with no fonts has nothing to tell the two apart.
        return;
    }
    let page = "data:text/html,<span id=s style='font: 40px serif'>Sandbox mmmm</span>\
        <span id=t style='font: 40px monospace'>Sandbox mmmm</span>";
    let widths = |args: &[&str]| {
        let mut browser = Browser::start_with_system_fonts(args);
        let session = browser.attach();
        browser.navigate(&session, page);
        let read = "return [s.offsetWidth, t.offsetWidth]";
        script(&mut browser, &session, read, serde_json::json!([])).unwrap()
    };
    let inside = widths(&["--process-per-tab"]);
    assert!(inside[0].as_f64().unwrap() > 100.0, "{inside}");
    assert!(inside[1].as_f64().unwrap() > 100.0, "{inside}");
    assert_eq!(inside, widths(&["--process-per-tab", "--no-sandbox"]));
}

/// What the probe did: its line (`ok N`, `error ERRNO`) or the signal that
/// ended it.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Ok(u64),
    Errno(i32),
    Signal(i32),
}

const SIGSYS: Outcome = Outcome::Signal(libc::SIGSYS);
const EACCES: Outcome = Outcome::Errno(libc::EACCES);

fn probe_with(switches: &[&str], attempt: &[&str]) -> (Outcome, String) {
    let output = Command::new(MACHOPO)
        .arg(shell::renderer_sandbox::PROBE_SWITCH)
        .args(switches)
        .arg("--")
        .args(attempt)
        .output()
        .expect("machopo runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let outcome = if let Some(signal) = output.status.signal() {
        Outcome::Signal(signal)
    } else if let Some(number) = stdout.trim().strip_prefix("ok ") {
        Outcome::Ok(number.parse().unwrap())
    } else if let Some(errno) = stdout.trim().strip_prefix("error ") {
        Outcome::Errno(errno.parse().unwrap())
    } else {
        panic!(
            "the probe printed {stdout:?}, exited {:?} ({stderr})",
            output.status
        );
    };
    (outcome, stderr)
}

/// With the built-in fonts alone: no font directory.
fn probe(attempt: &[&str]) -> Outcome {
    probe_with(&["--test-fonts"], attempt).0
}

fn syscall(call: libc::c_long, arguments: &[i64]) -> Outcome {
    let mut attempt = vec!["syscall".to_owned(), call.to_string()];
    attempt.extend(arguments.iter().map(i64::to_string));
    let attempt: Vec<&str> = attempt.iter().map(String::as_str).collect();
    probe(&attempt)
}

/// A directory of this test's, with `file.txt` in it.
fn directory(name: &str) -> (PathBuf, String) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("renderer-sandbox")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("file.txt");
    std::fs::write(&file, b"contents").unwrap();
    (dir, file.to_str().unwrap().to_owned())
}

#[test]
fn the_probe_enters_and_makes_a_thread() {
    assert_eq!(probe(&["nothing"]), Outcome::Ok(0));
    assert_eq!(probe(&["thread", "nothing"]), Outcome::Ok(0));
    // With the system's fonts found first, as a renderer for a window.
    assert_eq!(probe_with(&[], &["nothing"]).0, Outcome::Ok(0));
}

#[test]
fn a_renderer_reads_no_file_of_the_users() {
    let (dir, file) = directory("read");
    assert_eq!(probe(&["read", &file]), EACCES);
    assert_eq!(probe(&["list", dir.to_str().unwrap()]), EACCES);
    assert_eq!(probe(&["read", "/etc/passwd"]), EACCES);
    assert_eq!(probe(&["list", "/"]), EACCES);
    if let Some(home) = std::env::var_os("HOME") {
        assert_eq!(probe(&["list", home.to_str().unwrap()]), EACCES);
    }
    assert_eq!(probe(&["read", "/proc/self/environ"]), EACCES);
    // On a thread made after entering, too.
    assert_eq!(probe(&["thread", "read", &file]), EACCES);
}

#[test]
fn a_renderer_reads_the_font_directories_it_was_given_and_the_time_zone() {
    let (dir, file) = directory("fonts");
    let fonts = ["--font-directory", dir.to_str().unwrap()];
    assert_eq!(probe_with(&fonts, &["read", &file]).0, Outcome::Ok(8));
    assert_eq!(
        probe_with(&fonts, &["list", dir.to_str().unwrap()]).0,
        Outcome::Ok(1)
    );
    // Not without being given it; and not with only the built-in fonts.
    assert_eq!(probe_with(&[], &["read", &file]).0, EACCES);
    if std::path::Path::new("/etc/localtime").exists() {
        assert!(matches!(probe(&["read", "/etc/localtime"]), Outcome::Ok(_)));
    }
}

#[test]
fn a_renderer_changes_nothing_in_the_file_system() {
    let (dir, file) = directory("write");
    // Where it can read as much as where it can't.
    for switches in [
        &["--font-directory", dir.to_str().unwrap()][..],
        &["--test-fonts"][..],
    ] {
        let new = dir.join("new.txt");
        let attempts: [&[&str]; 5] = [
            &["write", new.to_str().unwrap()],
            &["write", &file],
            &["remove", &file],
            &["chmod", &file],
            &["truncate", &file],
        ];
        for attempt in attempts {
            assert_eq!(probe_with(switches, attempt).0, EACCES, "{attempt:?}");
        }
        assert!(!new.exists());
        assert_eq!(std::fs::read(&file).unwrap(), b"contents");
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_ne!(mode & 0o777, 0o600, "its mode is as it was made");
    }
    // By the calls themselves, for where the standard library doesn't go.
    for call in [
        libc::SYS_mkdirat,
        libc::SYS_renameat2,
        libc::SYS_symlinkat,
        libc::SYS_linkat,
        libc::SYS_fchownat,
        libc::SYS_utimensat,
        libc::SYS_truncate,
    ] {
        assert_eq!(syscall(call, &[]), EACCES, "{call}");
    }
    // Its flags are in memory, where a filter can't read them.
    assert_eq!(
        syscall(libc::SYS_openat2, &[]),
        Outcome::Errno(libc::ENOSYS)
    );
}

#[test]
fn a_renderer_that_reaches_outside_its_process_is_ended() {
    let (outcome, stderr) = probe_with(
        &["--test-fonts"],
        &["syscall", &libc::SYS_socket.to_string(), "2", "1", "0"],
    );
    assert_eq!(outcome, SIGSYS);
    assert!(
        stderr.contains("(socket) isn't allowed in this sandbox"),
        "{stderr}"
    );
    for call in [
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_execve,
        libc::SYS_execveat,
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_io_uring_setup,
        libc::SYS_bpf,
        libc::SYS_mount,
        libc::SYS_unshare,
        libc::SYS_setns,
        libc::SYS_chroot,
        libc::SYS_chdir,
        libc::SYS_setuid,
        libc::SYS_seccomp,
        libc::SYS_landlock_restrict_self,
        libc::SYS_perf_event_open,
        libc::SYS_userfaultfd,
        libc::SYS_clock_settime,
        libc::SYS_kexec_load,
        libc::SYS_pidfd_open,
    ] {
        assert_eq!(syscall(call, &[]), SIGSYS, "{call}");
    }
}

#[test]
fn a_renderer_makes_threads_and_no_process() {
    assert_eq!(syscall(libc::SYS_clone3, &[]), Outcome::Errno(libc::ENOSYS));
    assert_eq!(
        syscall(libc::SYS_clone, &[i64::from(libc::SIGCHLD)]),
        SIGSYS
    );
    #[cfg(target_arch = "x86_64")]
    for call in [libc::SYS_fork, libc::SYS_vfork] {
        assert_eq!(syscall(call, &[]), SIGSYS, "{call}");
    }
}

#[test]
fn a_renderer_signals_itself_alone() {
    let other = i64::from(std::process::id());
    assert_eq!(syscall(libc::SYS_kill, &[other, 0]), SIGSYS);
    assert_eq!(syscall(libc::SYS_tgkill, &[other, other, 0]), SIGSYS);
    // Every process and its own group.
    assert_eq!(syscall(libc::SYS_kill, &[-1, 0]), SIGSYS);
    assert_eq!(syscall(libc::SYS_kill, &[0, 0]), SIGSYS);
    // Itself: `abort`, which ends it as `abort` does anywhere.
    let (outcome, _) = probe_with(&["--test-fonts"], &["abort"]);
    assert_eq!(outcome, Outcome::Signal(libc::SIGABRT));
}

#[test]
fn a_renderers_calls_are_allowed_for_the_arguments_it_uses() {
    // `prctl`: a crash's core dump turned off, and nothing else of it.
    let dumpable = i64::from(libc::PR_SET_DUMPABLE);
    assert_eq!(syscall(libc::SYS_prctl, &[dumpable, 0]), Outcome::Ok(0));
    assert_eq!(syscall(libc::SYS_prctl, &[dumpable, 1]), SIGSYS);
    assert_eq!(
        syscall(libc::SYS_prctl, &[i64::from(libc::PR_SET_PDEATHSIG), 0]),
        SIGSYS
    );
    assert_eq!(
        syscall(libc::SYS_prctl, &[i64::from(libc::PR_SET_SECCOMP), 0]),
        SIGSYS
    );
    // `ioctl`: whether a descriptor is a terminal (its standard input
    // here isn't: the call is made and says so).
    let tcgets = i64::try_from(libc::TCGETS).unwrap();
    assert!(matches!(
        syscall(libc::SYS_ioctl, &[0, tcgets, 0]),
        Outcome::Errno(_)
    ));
    let tiocsti = i64::try_from(libc::TIOCSTI).unwrap();
    assert_eq!(syscall(libc::SYS_ioctl, &[0, tiocsti, 0]), SIGSYS);
    // `madvise`: the advice the allocators give, not `MADV_REMOVE`.
    assert!(matches!(
        syscall(libc::SYS_madvise, &[0, 0, i64::from(libc::MADV_DONTNEED)]),
        Outcome::Ok(_) | Outcome::Errno(_)
    ));
    assert_eq!(
        syscall(libc::SYS_madvise, &[0, 0, i64::from(libc::MADV_REMOVE)]),
        SIGSYS
    );
    // `prlimit64`: its own limits read, not set (a pointer the filter
    // refuses before the kernel reads it), nor another process's.
    let stack = i64::from(libc::RLIMIT_STACK as i32);
    assert!(matches!(
        syscall(libc::SYS_prlimit64, &[0, stack, 0, 0]),
        Outcome::Ok(0)
    ));
    assert_eq!(syscall(libc::SYS_prlimit64, &[0, stack, 1, 0]), SIGSYS);
    assert_eq!(syscall(libc::SYS_prlimit64, &[1, stack, 0, 0]), SIGSYS);
}
