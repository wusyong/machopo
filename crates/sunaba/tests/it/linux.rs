//! What a process in a sandbox can't do, tried from inside one: each test
//! runs `sunaba_probe`, which enters the probe's baseline policy and what
//! the test adds to it, tries one thing and prints what came of it.

use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Command;

/// What a probe did: its one line (`ok N`, `error ERRNO`), or the signal
/// that ended it, and its stderr.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Ok(u64),
    Errno(i32),
    Signal(i32),
    /// The probe's own exit code: 2 when it didn't enter.
    Exit(i32),
}

fn probe(policy: &[&str], attempt: &[&str]) -> (Outcome, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_sunaba_probe"))
        .args(policy)
        .arg("--")
        .args(attempt)
        .output()
        .expect("the probe runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let outcome = if let Some(signal) = output.status.signal() {
        Outcome::Signal(signal)
    } else if output.status.code() != Some(0) {
        Outcome::Exit(output.status.code().unwrap())
    } else if let Some(number) = stdout.trim().strip_prefix("ok ") {
        Outcome::Ok(number.parse().unwrap())
    } else if let Some(errno) = stdout.trim().strip_prefix("error ") {
        Outcome::Errno(errno.parse().unwrap())
    } else {
        panic!("the probe printed {stdout:?} ({stderr})");
    };
    (outcome, stderr)
}

fn outcome(policy: &[&str], attempt: &[&str]) -> Outcome {
    probe(policy, attempt).0
}

/// A directory of this test's with `inside/a.txt` and `outside/b.txt`.
fn files(name: &str) -> (PathBuf, String, String) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("sunaba")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("inside")).unwrap();
    std::fs::create_dir_all(dir.join("outside")).unwrap();
    std::fs::write(dir.join("inside/a.txt"), b"inside").unwrap();
    std::fs::write(dir.join("outside/b.txt"), b"outside!").unwrap();
    let inside = dir.join("inside").to_str().unwrap().to_owned();
    let outside = dir.join("outside").to_str().unwrap().to_owned();
    (dir, inside, outside)
}

fn number(call: libc::c_long) -> String {
    call.to_string()
}

const SIGSYS: Outcome = Outcome::Signal(libc::SIGSYS);

#[test]
fn a_process_enters_and_goes_on() {
    assert_eq!(outcome(&[], &["nothing"]), Outcome::Ok(0));
}

#[test]
fn what_is_beneath_a_read_only_directory_is_read_and_listed() {
    let (_, inside, _) = files("read");
    let policy = ["--read-only", &inside];
    assert_eq!(
        outcome(&policy, &["read", &format!("{inside}/a.txt")]),
        Outcome::Ok(6)
    );
    assert_eq!(outcome(&policy, &["list", &inside]), Outcome::Ok(1));
}

#[test]
fn a_file_outside_the_policy_isnt_read_nor_its_directory_listed() {
    let (_, inside, outside) = files("outside");
    let policy = ["--read-only", &inside];
    let file = format!("{outside}/b.txt");
    assert_eq!(
        outcome(&policy, &["read", &file]),
        Outcome::Errno(libc::EACCES)
    );
    assert_eq!(
        outcome(&policy, &["list", &outside]),
        Outcome::Errno(libc::EACCES)
    );
    // With no path at all, too.
    assert_eq!(outcome(&[], &["read", &file]), Outcome::Errno(libc::EACCES));
}

#[test]
fn a_read_only_file_is_read_and_its_neighbours_arent() {
    let (dir, inside, _) = files("one-file");
    std::fs::write(dir.join("inside/other.txt"), b"other").unwrap();
    let file = format!("{inside}/a.txt");
    let policy = ["--read-only", &file];
    assert_eq!(outcome(&policy, &["read", &file]), Outcome::Ok(6));
    assert_eq!(
        outcome(&policy, &["read", &format!("{inside}/other.txt")]),
        Outcome::Errno(libc::EACCES)
    );
    assert_eq!(
        outcome(&policy, &["list", &inside]),
        Outcome::Errno(libc::EACCES)
    );
}

#[test]
fn nothing_is_written_or_removed_even_where_it_can_be_read() {
    let (dir, inside, _) = files("write");
    let policy = ["--read-only", &inside];
    assert_eq!(
        outcome(&policy, &["write", &format!("{inside}/new.txt")]),
        Outcome::Errno(libc::EACCES)
    );
    assert_eq!(
        outcome(&policy, &["write", &format!("{inside}/a.txt")]),
        Outcome::Errno(libc::EACCES)
    );
    assert_eq!(
        outcome(&policy, &["remove", &format!("{inside}/a.txt")]),
        Outcome::Errno(libc::EACCES)
    );
    assert!(!dir.join("inside/new.txt").exists());
    assert_eq!(std::fs::read(dir.join("inside/a.txt")).unwrap(), b"inside");
}

#[test]
fn a_path_that_isnt_there_is_left_out_of_the_policy() {
    let (dir, inside, _) = files("missing");
    let missing = format!("{inside}/missing");
    let policy = ["--read-only", &missing, "--read-only", &inside];
    assert_eq!(
        outcome(&policy, &["read", &format!("{inside}/a.txt")]),
        Outcome::Ok(6)
    );
    // Made after the process entered, it is as any other path outside.
    let late = dir.join("late.txt");
    let late_path = late.to_str().unwrap();
    let policy = ["--read-only", late_path];
    std::fs::write(&late, b"late").unwrap();
    std::fs::remove_file(&late).unwrap();
    assert_eq!(
        outcome(&policy, &["read", late_path]),
        Outcome::Errno(libc::ENOENT)
    );
}

/// Not a guarantee, a limit to know of: Landlock decides opens, and a
/// path's metadata is read without one.
#[test]
fn a_file_outside_the_policy_can_still_be_statted() {
    let (_, _, outside) = files("stat");
    assert_eq!(
        outcome(&[], &["stat", &format!("{outside}/b.txt")]),
        Outcome::Ok(0)
    );
}

#[test]
fn a_system_call_the_policy_doesnt_name_ends_the_process_and_is_named() {
    let (outcome, stderr) = probe(&[], &["syscall", &number(libc::SYS_socket), "2", "1", "0"]);
    assert_eq!(outcome, SIGSYS);
    assert_eq!(
        stderr,
        format!(
            "sunaba: system call {} (socket) isn't allowed in this sandbox\n",
            libc::SYS_socket
        )
    );
    // One with no arguments and no effect, too: the list is of what is
    // allowed.
    let (outcome, stderr) = probe(&[], &["syscall", &number(libc::SYS_getpid)]);
    assert_eq!(outcome, SIGSYS);
    assert!(stderr.contains("(getpid)"), "{stderr}");
}

#[test]
fn an_allowed_system_call_is_made() {
    let pid = number(libc::SYS_getpid);
    assert!(matches!(outcome(&["--allow", &pid], &["syscall", &pid]), Outcome::Ok(pid) if pid > 1));
}

#[test]
fn a_failing_system_call_returns_its_errno_and_isnt_made() {
    let socket = number(libc::SYS_socket);
    let attempt = ["syscall", &socket, "2", "1", "0"];
    assert_eq!(
        outcome(&["--fail", &socket, "13"], &attempt),
        Outcome::Errno(13)
    );
    // Two calls with two errnos are two filters, and each keeps its own.
    let pid = number(libc::SYS_getpid);
    let policy = ["--fail", &socket, "13", "--fail", &pid, "38"];
    assert_eq!(outcome(&policy, &attempt), Outcome::Errno(13));
    assert_eq!(outcome(&policy, &["syscall", &pid]), Outcome::Errno(38));
    // And the rest is as it was.
    assert_eq!(outcome(&policy, &["nothing"]), Outcome::Ok(0));
    assert_eq!(
        outcome(&policy, &["syscall", &number(libc::SYS_getuid)]),
        SIGSYS
    );
}

/// `getpid` reads no argument, so the registers can hold anything: the
/// filter sees them all the same.
#[test]
fn a_system_call_allowed_for_some_arguments_ends_the_process_for_others() {
    let pid = number(libc::SYS_getpid);
    let policy = ["--allow-int", &pid, "1", "5"];
    assert!(matches!(
        outcome(&policy, &["syscall", &pid, "0", "5"]),
        Outcome::Ok(_)
    ));
    assert_eq!(outcome(&policy, &["syscall", &pid, "0", "6"]), SIGSYS);
    assert_eq!(outcome(&policy, &["syscall", &pid, "5", "0"]), SIGSYS);
    // Either of two.
    let policy = ["--allow-int", &pid, "1", "5", "--allow-int", &pid, "1", "7"];
    assert!(matches!(
        outcome(&policy, &["syscall", &pid, "0", "5"]),
        Outcome::Ok(_)
    ));
    assert!(matches!(
        outcome(&policy, &["syscall", &pid, "0", "7"]),
        Outcome::Ok(_)
    ));
    assert_eq!(outcome(&policy, &["syscall", &pid, "0", "6"]), SIGSYS);
}

#[test]
fn an_int_argument_is_its_low_half_and_a_long_all_of_it() {
    let pid = number(libc::SYS_getpid);
    let high = ((1u64 << 32) + 5).to_string();
    let policy = ["--allow-int", &pid, "0", "5"];
    assert!(matches!(
        outcome(&policy, &["syscall", &pid, &high]),
        Outcome::Ok(_)
    ));
    let policy = ["--allow-long", &pid, "0", "5"];
    assert!(matches!(
        outcome(&policy, &["syscall", &pid, "5"]),
        Outcome::Ok(_)
    ));
    assert_eq!(outcome(&policy, &["syscall", &pid, &high]), SIGSYS);
}

#[test]
fn a_call_fails_for_some_arguments_and_is_made_for_the_rest() {
    let pid = number(libc::SYS_getpid);
    // Fails when the second argument has bit 4 set.
    let policy = ["--allow", &pid, "--fail-masked", &pid, "13", "1", "4", "4"];
    assert!(matches!(
        outcome(&policy, &["syscall", &pid, "0", "3"]),
        Outcome::Ok(_)
    ));
    assert_eq!(
        outcome(&policy, &["syscall", &pid, "0", "4"]),
        Outcome::Errno(13)
    );
    assert_eq!(
        outcome(&policy, &["syscall", &pid, "0", "7"]),
        Outcome::Errno(13)
    );
    // Either of two tests, and with a call that always fails with the
    // same errno beside it.
    let uid = number(libc::SYS_getuid);
    let policy = [
        "--allow",
        &pid,
        "--fail-masked",
        &pid,
        "13",
        "1",
        "4",
        "4",
        "--fail-masked",
        &pid,
        "13",
        "1",
        "1",
        "1",
        "--fail",
        &uid,
        "13",
    ];
    assert!(matches!(
        outcome(&policy, &["syscall", &pid, "0", "2"]),
        Outcome::Ok(_)
    ));
    assert_eq!(
        outcome(&policy, &["syscall", &pid, "0", "4"]),
        Outcome::Errno(13)
    );
    assert_eq!(
        outcome(&policy, &["syscall", &pid, "0", "1"]),
        Outcome::Errno(13)
    );
    assert_eq!(outcome(&policy, &["syscall", &uid]), Outcome::Errno(13));
    // A call that always fails with an errno does, whatever is said of
    // some of its arguments.
    let policy = [
        "--fail",
        &pid,
        "13",
        "--fail-masked",
        &pid,
        "13",
        "1",
        "4",
        "4",
    ];
    assert_eq!(
        outcome(&policy, &["syscall", &pid, "0", "3"]),
        Outcome::Errno(13)
    );
    // Not allowed elsewhere, the rest of its uses end the process.
    let policy = ["--fail-masked", &pid, "13", "1", "4", "4"];
    assert_eq!(
        outcome(&policy, &["syscall", &pid, "0", "4"]),
        Outcome::Errno(13)
    );
    assert_eq!(outcome(&policy, &["syscall", &pid, "0", "3"]), SIGSYS);
}

/// Limits to know of, for a policy to cover with its system calls:
/// Landlock doesn't decide `chmod` (nor `chown`, `utime`, extended
/// attributes), so a policy that allows the call allows it anywhere the
/// user could.
#[test]
fn a_policy_that_allows_chmod_allows_it_outside_its_paths() {
    let (dir, _, outside) = files("chmod");
    let file = format!("{outside}/b.txt");
    let chmod = number(libc::SYS_fchmodat);
    let old = number(libc::SYS_chmod);
    assert_eq!(outcome(&[], &["chmod", &file]), SIGSYS);
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let policy = ["--allow", &chmod, "--allow", &old];
    assert_eq!(outcome(&policy, &["chmod", &file]), Outcome::Ok(0));
    let mode = std::fs::metadata(dir.join("outside/b.txt"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

/// Opening a readable file with `O_TRUNC` empties it on a kernel before
/// Landlock's third ABI; from the third on it is refused. Either way a
/// policy can fail the flag itself.
#[test]
fn a_policy_can_fail_an_open_that_truncates() {
    let (dir, inside, _) = files("truncate");
    let file = format!("{inside}/a.txt");
    let open = number(libc::SYS_openat);
    let trunc = libc::O_TRUNC.to_string();
    let policy = [
        "--read-only",
        &inside,
        "--fail-masked",
        &open,
        "13",
        "2",
        &trunc,
        &trunc,
    ];
    assert_eq!(outcome(&policy, &["truncate", &file]), Outcome::Errno(13));
    assert_eq!(outcome(&policy, &["read", &file]), Outcome::Ok(6));
    assert_eq!(std::fs::read(dir.join("inside/a.txt")).unwrap(), b"inside");
}

#[test]
fn a_thread_is_made_and_a_process_isnt() {
    assert_eq!(outcome(&[], &["thread", "nothing"]), Outcome::Ok(0));
    // `clone3` is refused politely, so libc falls back to `clone`.
    assert_eq!(
        outcome(&[], &["syscall", &number(libc::SYS_clone3)]),
        Outcome::Errno(libc::ENOSYS)
    );
    // `fork`'s `clone`: no `CLONE_THREAD`.
    let fork = libc::SIGCHLD.to_string();
    assert_eq!(
        outcome(&[], &["syscall", &number(libc::SYS_clone), &fork]),
        SIGSYS
    );
    // A thread in a new user namespace (the kernel refuses the pair, but
    // the filter is what's asked here).
    let flags = (libc::CLONE_THREAD | libc::CLONE_SIGHAND | libc::CLONE_VM | libc::CLONE_NEWUSER)
        .to_string();
    assert_eq!(
        outcome(&[], &["syscall", &number(libc::SYS_clone), &flags]),
        SIGSYS
    );
    assert_eq!(
        outcome(&[], &["syscall", &number(libc::SYS_execve)]),
        SIGSYS
    );
}

#[test]
fn a_thread_made_after_entering_is_inside_too() {
    let (_, inside, outside) = files("thread");
    let policy = ["--read-only", &inside];
    assert_eq!(
        outcome(&policy, &["thread", "read", &format!("{inside}/a.txt")]),
        Outcome::Ok(6)
    );
    assert_eq!(
        outcome(&policy, &["thread", "read", &format!("{outside}/b.txt")]),
        Outcome::Errno(libc::EACCES)
    );
    assert_eq!(
        outcome(
            &policy,
            &[
                "thread",
                "syscall",
                &number(libc::SYS_socket),
                "2",
                "1",
                "0"
            ]
        ),
        SIGSYS
    );
}

/// The kernel's own account, on the main thread and on one made later:
/// `Seccomp: 2` is a filter, and the process can't gain privileges by
/// `execve` (which a filter needs of an unprivileged process).
#[test]
fn the_kernel_says_the_process_and_its_threads_are_filtered() {
    let policy = ["--read-only", "/proc"];
    for thread in [&[][..], &["thread"][..]] {
        let field = |name: &str| {
            let attempt = [thread, &["field", "/proc/thread-self/status", name]].concat();
            outcome(&policy, &attempt)
        };
        assert_eq!(field("Seccomp"), Outcome::Ok(2));
        assert_eq!(field("NoNewPrivs"), Outcome::Ok(1));
    }
    // One filter for what isn't named, one for `clone3`'s errno.
    let filters = ["field", "/proc/thread-self/status", "Seccomp_filters"];
    assert_eq!(outcome(&policy, &filters), Outcome::Ok(2));
}

#[test]
fn a_process_with_a_second_thread_doesnt_enter() {
    let (outcome, stderr) = probe(&["--thread-first"], &["nothing"]);
    assert_eq!(outcome, Outcome::Exit(2));
    assert!(stderr.contains("2 threads"), "{stderr}");
}
