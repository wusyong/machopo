//! What a process started under a policy can't do, tried from inside one:
//! each test runs `sunaba_probe`, which starts itself under the whole
//! policy (less what the test says), and the process it starts lowers
//! itself, tries one thing and prints what came of it.

use std::io::{Read as _, Write as _};
use std::os::windows::io::AsHandle as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use sunaba::windows::{AccessToken, LOW, UNTRUSTED};

/// What a probe did: its one line (`ok N`, `error CODE`), or what the
/// started process exited with when it printed none.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Ok(u64),
    Error(i32),
    Exit(i32),
}

/// `ERROR_ACCESS_DENIED`: what the sandbox fails a call with.
const DENIED: Outcome = Outcome::Error(5);

/// `WSASYSCALLFAILURE`: Winsock doesn't start in a process that opens
/// nothing of the system's.
const NO_WINSOCK: Outcome = Outcome::Error(10107);

fn probe(options: &[&str], attempt: &[&str]) -> (Outcome, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_sunaba_probe"))
        .args(options)
        .arg("--")
        .args(attempt)
        .stdin(Stdio::null())
        .output()
        .expect("the probe runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next().unwrap_or("").trim();
    let outcome = if let Some(number) = line.strip_prefix("ok ") {
        Outcome::Ok(number.parse().unwrap())
    } else if let Some(code) = line.strip_prefix("error ") {
        Outcome::Error(code.parse().unwrap())
    } else if output.status.code() != Some(0) {
        Outcome::Exit(output.status.code().unwrap_or(-1))
    } else {
        panic!("the probe printed {stdout:?} ({stderr})");
    };
    (outcome, stderr)
}

fn outcome(options: &[&str], attempt: &[&str]) -> Outcome {
    probe(options, attempt).0
}

/// The process started as a sandboxed one is, and lowered.
fn inside(attempt: &[&str]) -> Outcome {
    outcome(&[], attempt)
}

/// The process started with no policy and left as it started.
fn outside(attempt: &[&str]) -> Outcome {
    outcome(&["--no-policy", "--no-lower"], attempt)
}

/// A file of this test's, under the user's own directories (where the
/// build's output is), and the directory it is in.
fn file(name: &str) -> (String, String) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("sunaba")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, b"the user's").unwrap();
    (
        file.to_str().unwrap().to_owned(),
        dir.to_str().unwrap().to_owned(),
    )
}

fn system_root() -> String {
    std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())
}

fn system32(name: &str) -> String {
    format!(r"{}\System32\{name}", system_root())
}

/// The probe started under the policy, with the app container `name`
/// (of this test's), sleeping once it has lowered itself.
fn lowered(name: &str) -> sunaba::windows::Child {
    let mut command = sunaba::windows::Command::new(env!("CARGO_BIN_EXE_sunaba_probe"));
    command.arg("--inside").arg("sleep").arg("20");
    let policy = sunaba::windows::Policy::new(format!("sunaba.test.{}.{name}", std::process::id()));
    let child = command.spawn(Some(&policy)).unwrap();
    let process = sunaba::windows::open(child.id()).unwrap();
    let lowered = (0..500).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(10));
        sunaba::windows::integrity(process.as_handle()).unwrap() == UNTRUSTED
    });
    assert!(lowered);
    child
}

#[test]
fn a_process_started_with_no_policy_is_as_its_starter() {
    assert_eq!(outside(&["nothing"]), Outcome::Ok(0));
    assert_eq!(outside(&["container"]), Outcome::Ok(0));
    let own = sunaba::windows::integrity(sunaba::windows::this_process()).unwrap();
    assert_eq!(outside(&["integrity"]), Outcome::Ok(u64::from(own)));
    assert!(own > LOW, "the tests run at low integrity themselves");
}

#[test]
fn a_process_under_the_policy_is_in_an_app_container() {
    assert_eq!(inside(&["nothing"]), Outcome::Ok(0));
    let mut child = lowered("container");
    let process = sunaba::windows::open(child.id()).unwrap();
    assert!(sunaba::windows::in_app_container(process.as_handle()).unwrap());
    child.kill().unwrap();
    // Its first thread starts with a token of the same app container's,
    // which may read itself.
    assert_eq!(outcome(&["--no-lower"], &["container"]), Outcome::Ok(1));
    // The process's own, which every other thread has from the start
    // and the first once it has lowered itself, can't even be read.
    assert_eq!(outcome(&["--no-lower"], &["thread", "container"]), DENIED);
    assert_eq!(inside(&["container"]), DENIED);
    assert_eq!(inside(&["thread", "container"]), DENIED);
}

#[test]
fn it_starts_at_low_integrity_and_lowers_itself_to_untrusted() {
    assert_eq!(
        outcome(&["--no-lower"], &["integrity"]),
        Outcome::Ok(u64::from(LOW))
    );
    let mut child = lowered("integrity");
    let process = sunaba::windows::open(child.id()).unwrap();
    assert_eq!(
        sunaba::windows::integrity(process.as_handle()).unwrap(),
        UNTRUSTED
    );
    child.kill().unwrap();
}

/// From outside: its token is an app container's made from a lockdown
/// one, whose every group only denies and which grants only what is
/// granted to the null SID, as its starter's isn't; and what it makes is
/// for the user and its own app container, not its logon session's or
/// any restricted token's.
#[test]
fn the_process_has_a_lockdown_app_containers_token() {
    let mut child = lowered("token");
    let process = sunaba::windows::open(child.id()).unwrap();
    let token = AccessToken::of(process.as_handle()).unwrap();
    child.kill().unwrap();
    assert!(token.user.deny_only, "{token:?}");
    assert!(
        token
            .groups
            .iter()
            .all(|group| group.integrity || group.deny_only),
        "{token:?}"
    );
    assert!(token.groups.iter().any(|group| group.logon), "{token:?}");
    assert_eq!(token.restricting, ["S-1-0-0"]);
    let container = token.app_container.clone().expect("an app container's");
    assert!(container.starts_with("S-1-15-2-"), "{container}");
    let dacl = token.default_dacl.clone().expect("a default DACL");
    let granted: Vec<&str> = dacl.iter().map(|ace| ace.sid.as_str()).collect();
    assert!(dacl.iter().all(|ace| ace.kind == 0), "{dacl:?}");
    assert!(granted.contains(&container.as_str()), "{dacl:?}");
    assert!(granted.contains(&token.user.sid.as_str()), "{dacl:?}");
    assert!(
        !granted.iter().any(|sid| sid.starts_with("S-1-5-5-")),
        "{dacl:?}"
    );
    // RESTRICTED, and every app container (ALL APPLICATION PACKAGES).
    assert!(!granted.contains(&"S-1-5-12"), "{dacl:?}");
    assert!(!granted.contains(&"S-1-15-2-1"), "{dacl:?}");

    let own = AccessToken::of(sunaba::windows::this_process()).unwrap();
    assert!(!own.user.deny_only && own.restricting.is_empty());
    assert_eq!(own.app_container, None);
    assert_eq!(own.user.sid, token.user.sid);
}

/// What one process under a policy makes (its process, whose security
/// comes from its token's default DACL) another can't open, by its
/// lockdown token or by the one it starts with, which isn't the first
/// one's app container's; the user's other processes can.
#[test]
fn what_a_process_makes_another_under_a_policy_cant_open() {
    let mut child = lowered("made");
    let id = child.id().to_string();
    assert_eq!(inside(&["process", &id]), DENIED);
    assert_eq!(outcome(&["--no-lower"], &["process", &id]), DENIED);
    assert_eq!(outside(&["process", &id]), Outcome::Ok(0));
    child.kill().unwrap();
}

/// The loader maps the DLLs a process under a policy starts with on its
/// first thread alone, the only one that may open them.
#[test]
fn the_loader_of_a_process_under_the_policy_has_one_thread() {
    let mut child = lowered("loader");
    assert_eq!(sunaba::windows::loader_threads(child.id()), Some(1));
    child.kill().unwrap();
    let mut command = sunaba::windows::Command::new(env!("CARGO_BIN_EXE_sunaba_probe"));
    command
        .arg("--inside")
        .arg("--no-lower")
        .arg("sleep")
        .arg("20");
    let mut child = command.spawn(None).unwrap();
    // The system's default.
    assert_eq!(sunaba::windows::loader_threads(child.id()), Some(0));
    child.kill().unwrap();
}

#[test]
fn lowering_isnt_only_for_a_process_in_a_sandbox() {
    // A process as its starter can lower its integrity too; it is then
    // refused what a low-integrity process may write.
    assert_eq!(
        outcome(&["--no-policy"], &["integrity"]),
        Outcome::Ok(u64::from(UNTRUSTED))
    );
}

#[test]
fn the_users_files_arent_read_listed_or_looked_at() {
    let (path, dir) = file("read");
    assert_eq!(outside(&["read", &path]), Outcome::Ok(10));
    assert_eq!(inside(&["read", &path]), DENIED);
    assert_eq!(inside(&["stat", &path]), DENIED);
    assert_eq!(inside(&["list", &dir]), DENIED);
    assert_eq!(inside(&["thread", "read", &path]), DENIED);
    // Not by lowering alone, and not by the app container alone either:
    // each is asked.
    assert_eq!(outcome(&["--no-lower"], &["read", &path]), DENIED);
}

#[test]
fn no_file_is_made_changed_or_removed() {
    let (path, dir) = file("write");
    let new = format!(r"{dir}\new.txt");
    assert_eq!(inside(&["write", &new]), DENIED);
    assert!(!PathBuf::from(&new).exists());
    assert_eq!(inside(&["write", &path]), DENIED);
    assert_eq!(inside(&["remove", &path]), DENIED);
    assert_eq!(std::fs::read(&path).unwrap(), b"the user's");
    // Nor where the system lets any process of the user's write.
    let temp = std::env::temp_dir().join(format!("sunaba-{}.txt", std::process::id()));
    assert_eq!(inside(&["write", temp.to_str().unwrap()]), DENIED);
    assert!(!temp.exists());
    assert_eq!(outside(&["write", &new]), Outcome::Ok(0));
}

/// What the system lets every application read, an app container
/// reads: a lockdown one doesn't. Only its first thread, until the
/// process has lowered itself, may (which is how its DLLs are loaded).
#[test]
fn nothing_of_the_systems_is_read_once_the_process_has_lowered_itself() {
    let dll = system32("kernel32.dll");
    let root = system_root();
    let fonts = format!(r"{root}\Fonts");
    let ini = format!(r"{root}\win.ini");
    let programs = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    assert!(matches!(outside(&["read", &dll]), Outcome::Ok(bytes) if bytes > 0));
    assert!(matches!(outside(&["list", &fonts]), Outcome::Ok(files) if files > 0));
    assert_eq!(inside(&["read", &dll]), DENIED);
    assert_eq!(inside(&["read", &ini]), DENIED);
    assert_eq!(inside(&["list", &fonts]), DENIED);
    assert_eq!(inside(&["list", &programs]), DENIED);
    assert_eq!(inside(&["stat", &system32("cmd.exe")]), DENIED);
    assert!(matches!(
        outcome(&["--no-lower"], &["read", &dll]),
        Outcome::Ok(bytes) if bytes > 0
    ));
    assert_eq!(outcome(&["--no-lower"], &["thread", "read", &dll]), DENIED);
}

#[test]
fn no_socket_is_made() {
    // An address no machine has (TEST-NET-1), and a port of the
    // machine's to listen on: Winsock doesn't even start.
    assert_eq!(inside(&["connect", "192.0.2.1:80"]), NO_WINSOCK);
    assert_eq!(inside(&["listen"]), NO_WINSOCK);
    assert_eq!(outside(&["listen"]), Outcome::Ok(0));
}

#[test]
fn nothing_is_connected_to_on_the_machine() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let answering = std::thread::spawn(move || {
        // One connection, answered: the unsandboxed probe's.
        let (mut stream, _) = listener.accept().unwrap();
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).unwrap();
        stream.write_all(b"y").unwrap();
        // And none after it, for as long as the sandboxed one tries.
        listener.set_nonblocking(true).unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut more = 0;
        while std::time::Instant::now() < until {
            if listener.accept().is_ok() {
                more += 1;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        more
    });
    assert_eq!(outside(&["connect", &address]), Outcome::Ok(1));
    assert_eq!(inside(&["connect", &address]), NO_WINSOCK);
    assert_eq!(answering.join().unwrap(), 0, "it reached the listener");
}

#[test]
fn no_program_is_started() {
    let cmd = system32("cmd.exe");
    assert_eq!(outside(&["run", &cmd, "/c", "exit 0"]), Outcome::Ok(0));
    // The program can't be opened.
    assert_eq!(inside(&["run", &cmd, "/c", "exit 0"]), DENIED);
    assert_eq!(inside(&["thread", "run", &cmd, "/c", "exit 0"]), DENIED);
    // And where it can, by the token the first thread starts with:
    // `ERROR_CHILD_PROCESS_BLOCKED`, the process's own attribute.
    assert_eq!(
        outcome(&["--no-lower"], &["run", &cmd, "/c", "exit 0"]),
        Outcome::Error(367)
    );
}

#[test]
fn no_other_process_is_opened() {
    let this = std::process::id().to_string();
    assert_eq!(outside(&["process", &this]), Outcome::Ok(0));
    assert_eq!(inside(&["process", &this]), DENIED);
}

/// The flags of mitigation policy `number` in a process started with
/// `options`.
fn mitigation(options: &[&str], number: &str) -> u64 {
    match outcome(options, &["mitigation", number]) {
        Outcome::Ok(flags) => flags,
        other => panic!("mitigation {number}: {other:?}"),
    }
}

#[test]
fn the_process_has_the_policys_mitigations() {
    // No win32k system calls, no file system control codes.
    assert_eq!(mitigation(&[], "4") & 0b101, 0b101);
    // No extension points.
    assert_eq!(mitigation(&[], "6") & 1, 1);
    // Microsoft-signed DLLs only.
    assert_eq!(mitigation(&[], "8") & 1, 1);
    // No fonts but the system's.
    assert_eq!(mitigation(&[], "9") & 1, 1);
    // No remote images, no low-integrity images.
    assert_eq!(mitigation(&[], "10") & 0b11, 0b11);
    // No child processes.
    assert_eq!(mitigation(&[], "13") & 1, 1);
    // Indirect branch prediction kept to its own domain.
    assert_eq!(mitigation(&[], "14") & 1, 1);
    // Invalid handles raise.
    assert_eq!(mitigation(&[], "3") & 0b11, 0b11);
}

#[test]
fn a_process_with_no_policy_has_none_of_them() {
    let none = ["--no-policy", "--no-lower"];
    for (number, bits) in [
        ("4", 0b101),
        ("6", 1),
        ("8", 1),
        ("9", 1),
        ("10", 0b11),
        ("13", 1),
        ("14", 1),
        ("3", 0b11),
    ] {
        assert_eq!(mitigation(&none, number) & bits, 0, "mitigation {number}");
    }
}

#[test]
fn win32k_and_other_peoples_dlls_are_allowed_only_when_asked() {
    // Each leaves the other, and what else is in the same word.
    assert_eq!(mitigation(&["--allow-win32k"], "4") & 0b101, 0b100);
    assert_eq!(mitigation(&["--allow-win32k"], "8") & 1, 1);
    assert_eq!(mitigation(&["--allow-third-party-modules"], "8") & 1, 0);
    assert_eq!(
        mitigation(&["--allow-third-party-modules"], "4") & 0b101,
        0b101
    );
}

#[test]
fn only_the_handles_named_are_inherited() {
    // With no policy too: this is how any process is started here.
    assert_eq!(outside(&["handle", "{named}"]), Outcome::Ok(1));
    // `ERROR_INVALID_HANDLE`: the other inheritable handle isn't there.
    assert_eq!(outside(&["handle", "{unnamed}"]), Outcome::Error(6));
    assert_eq!(
        outcome(&["--no-lower"], &["handle", "{named}"]),
        Outcome::Ok(1)
    );
}

#[test]
fn a_handle_that_isnt_one_ends_a_sandboxed_process() {
    // `STATUS_INVALID_HANDLE`, as the process's exit code.
    let ended = Outcome::Exit(0xC000_0008_u32.cast_signed());
    // A number no handle has.
    assert_eq!(inside(&["handle", "2147483632"]), ended);
    assert_eq!(outside(&["handle", "2147483632"]), Outcome::Error(6));
}

#[test]
fn a_sandboxed_process_ends_when_its_starter_lets_go_of_it() {
    assert_eq!(outcome(&["--abandon"], &["sleep", "20"]), Outcome::Ok(0));
    // One with no policy runs on.
    assert_eq!(
        outcome(&["--no-policy", "--abandon"], &["sleep", "2"]),
        Outcome::Ok(1)
    );
}

#[test]
fn arguments_and_the_environment_arrive_as_they_were_given() {
    let awkward = [
        "plain",
        "two words",
        "",
        r#"a "quoted" word"#,
        r"ends with a backslash\",
        r#"\"both\""#,
        "tab\there",
        r"C:\Program Files\x\",
    ];
    let mut words = vec!["words"];
    words.extend(awkward);
    assert_eq!(outside(&words), Outcome::Ok(awkward.len() as u64));
    for word in awkward {
        assert_eq!(
            outside(&["length", word]),
            Outcome::Ok(word.len() as u64),
            "{word:?}"
        );
    }
    // The starter's environment, with what was set and without what was
    // removed; a name is one whatever its case.
    assert!(matches!(outside(&["env", "PATH"]), Outcome::Ok(bytes) if bytes > 0));
    assert_eq!(
        outcome(
            &["--no-policy", "--env", "Sunaba_X", "abc"],
            &["env", "SUNABA_x"]
        ),
        Outcome::Ok(3)
    );
    assert_eq!(
        outcome(&["--no-policy", "--env-remove", "path"], &["env", "PATH"]),
        // `ERROR_ENVVAR_NOT_FOUND`.
        Outcome::Error(203)
    );
    // Under the policy as well.
    assert_eq!(
        outcome(&["--env", "SUNABA_X", "abcd"], &["env", "SUNABA_X"]),
        Outcome::Ok(4)
    );
}

/// Its starter reads what a sandboxed process was given from outside:
/// with only the variables set after its environment was cleared, and
/// once it has lowered itself.
#[test]
fn a_sandboxed_processs_environment_is_seen_from_outside() {
    let mut command = sunaba::windows::Command::new(env!("CARGO_BIN_EXE_sunaba_probe"));
    command
        .arg("--inside")
        .arg("sleep")
        .arg("20")
        .env("SUNABA_BEFORE", "cleared")
        .env_clear()
        .env("SOME_NAME", "a value with spaces")
        .env("OTHER", "x=y")
        .env("EMPTY", "");
    let policy = sunaba::windows::Policy::new(format!("sunaba.test.{}.env", std::process::id()));
    let mut child = command.spawn(Some(&policy)).unwrap();
    let process = sunaba::windows::open(child.id()).unwrap();
    let lowered = (0..500).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(10));
        sunaba::windows::integrity(process.as_handle()).unwrap() == UNTRUSTED
    });
    assert!(lowered);
    let mut environment = sunaba::windows::environment(child.id()).expect("its environment");
    environment.sort();
    let pair = |name: &str, value: &str| (name.into(), value.into());
    assert_eq!(
        environment,
        [
            pair("EMPTY", ""),
            pair("OTHER", "x=y"),
            pair("SOME_NAME", "a value with spaces"),
        ]
    );
    let id = child.id();
    child.kill().unwrap();
    child.wait().unwrap();
    drop((child, process));
    assert_eq!(sunaba::windows::environment(id), None);
}

#[test]
fn the_process_ends_as_a_program_that_gives_up_does() {
    // `STATUS_STACK_BUFFER_OVERRUN`: a fast fail, which `abort` is.
    let aborted = Outcome::Exit(0xC000_0409_u32.cast_signed());
    assert_eq!(inside(&["abort"]), aborted);
}

#[test]
fn a_program_that_isnt_there_starts_nothing() {
    let error = sunaba::windows::Command::new(r"C:\no\such\program.exe")
        .spawn(Some(&sunaba::windows::Policy::new("sunaba.test.missing")))
        .expect_err("started");
    // `ERROR_PATH_NOT_FOUND`.
    assert_eq!(error.code(), Some(3), "{error}");
}

#[test]
fn each_process_can_have_an_app_container_of_its_own() {
    // Two at once, by different names, with nothing registered for
    // either.
    let run = |name: &str| {
        let mut command = sunaba::windows::Command::new(env!("CARGO_BIN_EXE_sunaba_probe"));
        command.arg("--inside").arg("sleep").arg("1");
        command
            .spawn(Some(&sunaba::windows::Policy::new(name)))
            .unwrap()
    };
    let id = std::process::id();
    let mut first = run(&format!("sunaba.test.{id}.1"));
    let mut second = run(&format!("sunaba.test.{id}.2"));
    assert!(first.ends_with_this() && second.ends_with_this());
    for child in [&mut first, &mut second] {
        let process = sunaba::windows::open(child.id()).unwrap();
        assert!(sunaba::windows::in_app_container(process.as_handle()).unwrap());
        assert_eq!(child.wait().unwrap().code(), Some(0));
        assert!(child.try_wait().unwrap().is_some());
    }
}
