//! What a process in a sandbox can't do, tried from inside one: each test
//! runs `sunaba_probe`, which enters `(deny default)` and the rules the
//! test adds to it, tries one thing and prints what came of it.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// What a probe did: its one line (`ok N`, `error ERRNO`), or its own
/// exit code (2 when it didn't enter, 3 when it didn't enter again).
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Ok(u64),
    Errno(i32),
    Exit(i32),
}

/// What the sandbox fails a call with.
const REFUSED: Outcome = Outcome::Errno(libc::EPERM);

fn probe(profile: &[&str], attempt: &[&str]) -> (Outcome, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_sunaba_probe"))
        .args(profile)
        .arg("--")
        .args(attempt)
        .output()
        .expect("the probe runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let outcome = if output.status.code() != Some(0) {
        Outcome::Exit(output.status.code().unwrap_or(-1))
    } else if let Some(number) = stdout.trim().strip_prefix("ok ") {
        Outcome::Ok(number.parse().unwrap())
    } else if let Some(errno) = stdout.trim().strip_prefix("error ") {
        Outcome::Errno(errno.parse().unwrap())
    } else {
        panic!("the probe printed {stdout:?} ({stderr})");
    };
    (outcome, stderr)
}

fn outcome(profile: &[&str], attempt: &[&str]) -> Outcome {
    probe(profile, attempt).0
}

/// A directory of this test's with `inside/a.txt` and `outside/b.txt`, by
/// the paths the sandbox knows them by (no symbolic link on the way).
fn files(name: &str) -> (PathBuf, String, String) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("sunaba")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("inside")).unwrap();
    std::fs::create_dir_all(dir.join("outside")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("inside/a.txt"), b"inside").unwrap();
    std::fs::write(dir.join("outside/b.txt"), b"outside!").unwrap();
    let inside = dir.join("inside").to_str().unwrap().to_owned();
    let outside = dir.join("outside").to_str().unwrap().to_owned();
    (dir, inside, outside)
}

const READ_DIRECTORY: &str = r#"(allow file-read* (subpath (param "DIRECTORY")))"#;

/// launchd's answer for a service the sandbox doesn't let be looked up
/// (`BOOTSTRAP_NOT_PRIVILEGED`), and for one that isn't there
/// (`BOOTSTRAP_UNKNOWN_SERVICE`).
const NOT_PRIVILEGED: Outcome = Outcome::Errno(1100);
const UNKNOWN_SERVICE: Outcome = Outcome::Errno(1102);

#[test]
fn a_process_enters_and_goes_on() {
    assert_eq!(outcome(&[], &["nothing"]), Outcome::Ok(0));
    assert_eq!(outcome(&[], &["entered"]), Outcome::Ok(1));
    // This one hasn't.
    assert!(!sunaba::macos::entered());
}

#[test]
fn a_sandboxed_process_is_seen_to_be_from_outside() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sunaba_probe"))
        .args(["--", "wait"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    // It has entered by the time it reads: until then, it may not have.
    let entered = (0..500).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(10));
        sunaba::macos::sandboxed(child.id())
    });
    assert!(entered);
    assert!(!sunaba::macos::sandboxed(std::process::id()));
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
}

#[test]
fn nothing_of_the_file_system_is_reached_by_default() {
    let (_, _, outside) = files("default");
    let file = format!("{outside}/b.txt");
    assert_eq!(outcome(&[], &["read", &file]), REFUSED);
    assert_eq!(outcome(&[], &["list", &outside]), REFUSED);
    assert_eq!(outcome(&[], &["stat", &file]), REFUSED);
    assert_eq!(outcome(&[], &["read", "/etc/passwd"]), REFUSED);
}

/// Not a guarantee, a limit to know of: the sandbox refuses what is
/// there, and what isn't there is said not to be.
#[test]
fn whether_a_path_is_there_can_still_be_told() {
    let (_, _, outside) = files("there");
    assert_eq!(
        outcome(&[], &["stat", &format!("{outside}/missing.txt")]),
        Outcome::Errno(libc::ENOENT)
    );
}

#[test]
fn what_is_beneath_an_allowed_directory_is_read_and_the_rest_isnt() {
    let (_, inside, outside) = files("read");
    let profile = [
        "--rule",
        READ_DIRECTORY,
        "--parameter",
        "DIRECTORY",
        &inside,
    ];
    assert_eq!(
        outcome(&profile, &["read", &format!("{inside}/a.txt")]),
        Outcome::Ok(6)
    );
    assert_eq!(outcome(&profile, &["list", &inside]), Outcome::Ok(1));
    assert_eq!(
        outcome(&profile, &["read", &format!("{outside}/b.txt")]),
        REFUSED
    );
    assert_eq!(outcome(&profile, &["list", &outside]), REFUSED);
}

#[test]
fn nothing_is_written_removed_or_changed_where_it_can_be_read() {
    let (dir, inside, _) = files("write");
    let profile = [
        "--rule",
        READ_DIRECTORY,
        "--parameter",
        "DIRECTORY",
        &inside,
    ];
    let file = format!("{inside}/a.txt");
    assert_eq!(
        outcome(&profile, &["write", &format!("{inside}/new.txt")]),
        REFUSED
    );
    assert_eq!(outcome(&profile, &["write", &file]), REFUSED);
    assert_eq!(outcome(&profile, &["remove", &file]), REFUSED);
    assert_eq!(outcome(&profile, &["chmod", &file]), REFUSED);
    assert!(!dir.join("inside/new.txt").exists());
    assert_eq!(std::fs::read(dir.join("inside/a.txt")).unwrap(), b"inside");
}

/// What Linux's Landlock can't refuse, this does: a path's being there.
#[test]
fn metadata_is_its_own_permission() {
    let (_, inside, _) = files("metadata");
    let file = format!("{inside}/a.txt");
    let profile = [
        "--rule",
        r#"(allow file-read-metadata (subpath (param "DIRECTORY")))"#,
        "--parameter",
        "DIRECTORY",
        &inside,
    ];
    assert_eq!(outcome(&profile, &["stat", &file]), Outcome::Ok(0));
    assert_eq!(outcome(&profile, &["read", &file]), REFUSED);
}

/// A parameter's value is a string to the profile, whatever is in it.
#[test]
fn a_parameters_value_is_never_read_as_rules() {
    let (_, inside, outside) = files("parameter");
    let value = format!(r#"{inside}")) (allow default) (deny file-write* (subpath "/nowhere"#);
    let profile = ["--rule", READ_DIRECTORY, "--parameter", "DIRECTORY", &value];
    assert_eq!(
        outcome(&profile, &["read", &format!("{outside}/b.txt")]),
        REFUSED
    );
    assert_eq!(
        outcome(&profile, &["read", &format!("{inside}/a.txt")]),
        REFUSED
    );
}

#[test]
fn no_socket_is_made_or_connected() {
    assert_eq!(outcome(&[], &["listen"]), REFUSED);
    assert_eq!(outcome(&[], &["connect", "127.0.0.1:9"]), REFUSED);
    assert_eq!(outcome(&[], &["unix", "/var/run/syslog"]), REFUSED);
}

#[test]
fn no_process_is_made() {
    assert_eq!(outcome(&[], &["fork"]), REFUSED);
    assert_eq!(outcome(&[], &["run", "/usr/bin/true"]), REFUSED);
}

#[test]
fn a_signal_goes_to_no_one_unless_the_profile_says_so() {
    let this = std::process::id().to_string();
    assert_eq!(outcome(&[], &["signal", &this]), REFUSED);
    let to_itself = ["--rule", "(allow signal (target self))"];
    assert_eq!(outcome(&to_itself, &["signal", &this]), REFUSED);
    // Giving up takes no such permission: `abort` ends the process as it
    // does outside a sandbox.
    let output = Command::new(env!("CARGO_BIN_EXE_sunaba_probe"))
        .args(["--", "abort"])
        .output()
        .unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(output.status.signal(), Some(libc::SIGABRT));
}

#[test]
fn another_process_is_asked_about_unless_the_profile_denies_it() {
    let this = std::process::id().to_string();
    assert!(matches!(outcome(&[], &["process", &this]), Outcome::Ok(size) if size > 0));
    let profile = ["--rule", "(deny process-info*)"];
    assert_eq!(outcome(&profile, &["process", &this]), REFUSED);
}

#[test]
fn a_mach_service_is_looked_up_only_by_a_name_the_profile_allows() {
    let service = "com.apple.system.notification_center";
    assert_eq!(outcome(&[], &["lookup", service]), NOT_PRIVILEGED);
    let profile = [
        "--rule",
        r#"(allow mach-lookup (global-name (param "SERVICE")))"#,
        "--parameter",
        "SERVICE",
        service,
    ];
    assert_eq!(outcome(&profile, &["lookup", service]), Outcome::Ok(0));
    assert_eq!(
        outcome(&profile, &["lookup", "com.apple.logd"]),
        NOT_PRIVILEGED
    );
    // Allowed and not there is another answer.
    let missing = "org.sunaba.test.no-such-service";
    let profile = [
        "--rule",
        r#"(allow mach-lookup (global-name (param "SERVICE")))"#,
        "--parameter",
        "SERVICE",
        missing,
    ];
    assert_eq!(outcome(&profile, &["lookup", missing]), UNKNOWN_SERVICE);
}

#[test]
fn a_sysctl_is_read_only_by_a_name_the_profile_allows() {
    assert_eq!(outcome(&[], &["sysctl", "hw.ncpu"]), REFUSED);
    let profile = ["--rule", r#"(allow sysctl-read (sysctl-name "hw.ncpu"))"#];
    assert_eq!(outcome(&profile, &["sysctl", "hw.ncpu"]), Outcome::Ok(4));
    assert_eq!(outcome(&profile, &["sysctl", "kern.ostype"]), REFUSED);
}

#[test]
fn no_shared_memory_is_made() {
    let name = format!("/sunaba-{}", std::process::id());
    assert_eq!(outcome(&[], &["shm", &name]), REFUSED);
}

#[test]
fn a_thread_is_inside_whether_made_before_or_after() {
    let (_, _, outside) = files("thread");
    let file = format!("{outside}/b.txt");
    assert_eq!(outcome(&[], &["thread", "read", &file]), REFUSED);
    assert_eq!(outcome(&["--thread-first"], &["read", &file]), REFUSED);
    assert_eq!(outcome(&["--thread-first"], &["entered"]), Outcome::Ok(1));
}

#[test]
fn a_second_profile_doesnt_replace_the_first() {
    let (_, _, outside) = files("again");
    let file = format!("{outside}/b.txt");
    let (outcome, stderr) = probe(&["--again", "(version 1)(allow default)"], &["read", &file]);
    assert_eq!(outcome, Outcome::Exit(3));
    assert!(
        stderr.contains("sunaba: again: the system refused"),
        "{stderr}"
    );
}

#[test]
fn a_profile_that_doesnt_compile_isnt_entered() {
    let (outcome, stderr) = probe(&["--profile", "(version 1)(deny default"], &["nothing"]);
    assert_eq!(outcome, Outcome::Exit(2));
    assert!(
        stderr.contains("sunaba: the system refused the sandbox's profile"),
        "{stderr}"
    );
    // A NUL is said before the system is asked.
    let profile = sunaba::macos::Profile::new("(version 1)\0(allow default)");
    assert!(matches!(
        sunaba::macos::enter(&profile),
        Err(sunaba::macos::Error::Nul)
    ));
    let mut profile = sunaba::macos::Profile::new("(version 1)(allow default)");
    profile.parameter("A", "b\0c");
    assert!(matches!(
        sunaba::macos::enter(&profile),
        Err(sunaba::macos::Error::Nul)
    ));
    assert!(!sunaba::macos::entered());
}
