//! A pipe a process is driven through, between real processes: the test
//! starts itself again as the child (this file's `child`, told what to be
//! by an environment variable), which takes its ends as a driven process
//! does. What the child finds, and what it starts itself, is told by what
//! it exits with.

use std::io::{BufRead as _, BufReader, PipeReader, PipeWriter, Write as _};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use ipc::platform::{self, InheritError, PipeEnd};

const ROLE: &str = "MACHOPO_IPC_PIPE_CHILD";

// What a child exits with: what it found.
const SETUP: i32 = 24;
const MISSING_INPUT: i32 = 30;
const MISSING_OUTPUT: i32 = 31;
const INPUT_NOT_A_PIPE: i32 = 32;
const OUTPUT_NOT_A_PIPE: i32 = 33;
const NO_HANDLES: i32 = 34;
const BAD_HANDLES: i32 = 35;
const TAKEN: i32 = 37;
const IO: i32 = 38;
const TAKEN_TWICE: i32 = 39;

/// What a child that found no pipe exits with, by why.
fn code(error: &InheritError) -> i32 {
    match error {
        InheritError::Missing(PipeEnd::Input) => MISSING_INPUT,
        InheritError::Missing(PipeEnd::Output) => MISSING_OUTPUT,
        InheritError::NotAPipe(PipeEnd::Input) => INPUT_NOT_A_PIPE,
        InheritError::NotAPipe(PipeEnd::Output) => OUTPUT_NOT_A_PIPE,
        InheritError::NoHandles => NO_HANDLES,
        InheritError::BadHandles => BAD_HANDLES,
        InheritError::Taken => TAKEN,
        InheritError::Io(_) => IO,
    }
}

/// The handles' switch's value among this process's arguments, as a
/// driven process's caller would find it (Windows).
fn io_pipes() -> Option<String> {
    let prefix = format!("{}=", platform::IO_PIPES_SWITCH);
    std::env::args().find_map(|arg| arg.strip_prefix(&prefix).map(str::to_owned))
}

/// The child's side: is what `ROLE` says and exits with what came of it.
/// Without `ROLE` (the test run itself) it does nothing.
#[test]
fn child() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    // A child nothing tells to end (its host failed and is gone) ends by
    // itself: one left behind keeps the test run's output open.
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(60));
        std::process::exit(9);
    });
    if role == "linger" {
        thread::sleep(Duration::from_secs(4));
        std::process::exit(0);
    }
    // Is there until it is killed (or the limit above).
    if role == "stay" {
        thread::sleep(Duration::from_secs(50));
        std::process::exit(0);
    }
    if role == "launcher" {
        std::process::exit(launch());
    }
    // Asks with its output named otherwise than it was: by a value that
    // names no handle, or by its input's own (Windows).
    if role == "output-no-pipe" || role == "one-handle" {
        let named = io_pipes();
        let Some((input, _)) = named.as_deref().and_then(|named| named.split_once(',')) else {
            std::process::exit(SETUP);
        };
        let output = if role == "one-handle" { input } else { "1" };
        let outcome = match platform::inherited_pipe(Some(&format!("{input},{output}"))) {
            Ok(_) => 0,
            Err(error) => code(&error),
        };
        std::process::exit(outcome);
    }
    let (input, output) = match platform::inherited_pipe(io_pipes().as_deref()) {
        Ok(ends) => ends,
        Err(error) => std::process::exit(code(&error)),
    };
    let outcome = match role.as_str() {
        // Says back each line it reads, until its input ends.
        "echo" => echo(input, output),
        // Takes its ends, and asks for them again.
        "twice" => match platform::inherited_pipe(io_pipes().as_deref()) {
            Err(InheritError::Taken) => 0,
            Err(error) => code(&error),
            Ok(_) => TAKEN_TWICE,
        },
        // Takes its ends, starts a child of its own that outlives it, and
        // goes without closing them: were they inherited again, the
        // pipe would have an end yet.
        "parent" => {
            if command("linger").stdin(Stdio::null()).spawn().is_err() {
                std::process::exit(SETUP);
            }
            std::mem::forget((input, output));
            0
        }
        _ => SETUP,
    };
    std::process::exit(outcome);
}

/// Starts an echoing child with a pipe, and talks to it: 0 if it said
/// back what it was told.
fn launch() -> i32 {
    let Ok((child, mut writer, reader)) = platform::spawn_with_pipe(&mut command("echo")) else {
        return SETUP;
    };
    let mut line = String::new();
    if writeln!(writer, "from a launcher").is_err()
        || BufReader::new(reader).read_line(&mut line).is_err()
    {
        return SETUP;
    }
    drop(writer);
    match (line == "echo: from a launcher\n", exit_code(child)) {
        (true, Some(0)) => 0,
        (true, Some(code)) => code,
        _ => SETUP,
    }
}

fn echo(input: PipeReader, mut output: PipeWriter) -> i32 {
    for line in BufReader::new(input).lines() {
        let Ok(line) = line else { return SETUP };
        if writeln!(output, "echo: {line}").is_err() {
            return SETUP;
        }
    }
    0
}

/// The test started again as the child `role` (without a pipe: with one
/// through [`platform::spawn_with_pipe`]). The arguments end with
/// `--`: the handles' switch, put after them on Windows, is then one more
/// name of a test to run, which no test has, not an option the test
/// harness refuses.
fn command(role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "inherited_pipe::child",
            "--nocapture",
            "--test-threads=1",
            "--",
        ])
        .env(ROLE, role)
        .stdout(Stdio::null());
    command
}

fn exit_code(mut child: Child) -> Option<i32> {
    child.wait().unwrap().code()
}

/// Whether `reader` reads its end, with nothing before it, within two
/// seconds: none of the child's ends are held anywhere else.
fn ends_soon(mut reader: impl std::io::Read + Send + 'static) -> bool {
    let (read, done) = mpsc::channel();
    thread::spawn(move || {
        let mut rest = Vec::new();
        let _ = read.send(reader.read_to_end(&mut rest).map(|_| rest.is_empty()));
    });
    matches!(done.recv_timeout(Duration::from_secs(2)), Ok(Ok(true)))
}

#[test]
fn a_child_reads_and_writes_its_ends() {
    let (child, mut writer, reader) = platform::spawn_with_pipe(&mut command("echo")).unwrap();
    let mut reader = BufReader::new(reader);
    for said in ["one", "two"] {
        writeln!(writer, "{said}").unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, format!("echo: {said}\n"));
    }
    // The input's end is the child's end.
    drop(writer);
    assert_eq!(exit_code(child), Some(0));
    assert!(ends_soon(reader));
}

#[test]
fn the_ends_are_taken_once() {
    let (child, _writer, _reader) = platform::spawn_with_pipe(&mut command("twice")).unwrap();
    assert_eq!(exit_code(child), Some(0));
}

#[test]
fn a_childs_own_child_has_no_end_of_the_pipe() {
    // The child goes while its own child lives on for seconds: the
    // reader reads the end at once only if the child's output was the
    // child's alone.
    let (child, writer, reader) = platform::spawn_with_pipe(&mut command("parent")).unwrap();
    assert_eq!(exit_code(child), Some(0));
    assert!(ends_soon(reader));
    drop(writer);
}

#[test]
fn children_started_at_once_each_have_their_own_ends_alone() {
    // Eight at a time, from as many threads: each says back what was
    // said to it alone, and each one's reader reads the end once that
    // child goes, which it wouldn't if another child had its ends too.
    let started: Vec<_> = (0..8)
        .map(|_| thread::spawn(|| platform::spawn_with_pipe(&mut command("echo")).unwrap()))
        .collect();
    for (n, thread) in started.into_iter().enumerate() {
        let (child, mut writer, reader) = thread.join().unwrap();
        let mut reader = BufReader::new(reader);
        writeln!(writer, "child {n}").unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, format!("echo: child {n}\n"));
        drop(writer);
        assert_eq!(exit_code(child), Some(0));
        assert!(ends_soon(reader));
    }
}

/// The child `role` started by `sh`, which first does `redirections` to
/// 3 and 4 (as Chrome's clients may hand them over, by a shell).
#[cfg(unix)]
fn by_shell(role: &str, redirections: &str) -> Command {
    let plain = command(role);
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(format!(r#"exec "$0" "$@" {redirections}"#))
        .arg(plain.get_program())
        .args(plain.get_args())
        .env(ROLE, role)
        .stdout(Stdio::null());
    command
}

/// Has `command` (a [`by_shell`] one) take `input` and `output` as its
/// standard input and output, which its shell puts at 3 and 4, then gives
/// back to nothing: the test harness's output isn't the pipe's.
#[cfg(unix)]
fn over_stdio(command: &mut Command, input: impl Into<Stdio>, output: impl Into<Stdio>) -> Child {
    command.stdin(input).stdout(output).spawn().unwrap()
}

/// What moves standard input and output to 3 and 4.
#[cfg(unix)]
const STDIO_TO_PIPE: &str = "3<&0 4>&1 0</dev/null 1>/dev/null";

#[cfg(unix)]
#[test]
fn ends_a_shell_puts_at_3_and_4_are_taken() {
    // As Python's `pass_fds` and a shell hand them to Chrome.
    let (to_child, mut writer) = std::io::pipe().unwrap();
    let (reader, from_child) = std::io::pipe().unwrap();
    let child = over_stdio(&mut by_shell("echo", STDIO_TO_PIPE), to_child, from_child);
    let mut reader = BufReader::new(reader);
    writeln!(writer, "by hand").unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert_eq!(line, "echo: by hand\n");
    drop(writer);
    assert_eq!(exit_code(child), Some(0));
}

#[cfg(unix)]
#[test]
fn a_socket_pair_at_3_and_4_is_taken() {
    // Node's stdio pipes are a socket pair, which Playwright hands
    // Chrome.
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    let (ours, theirs) = UnixStream::pair().unwrap();
    let input = OwnedFd::from(theirs.try_clone().unwrap());
    let child = over_stdio(
        &mut by_shell("echo", STDIO_TO_PIPE),
        input,
        OwnedFd::from(theirs),
    );
    let mut writer = ours.try_clone().unwrap();
    let mut reader = BufReader::new(ours);
    writeln!(writer, "by socket").unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert_eq!(line, "echo: by socket\n");
    writer.shutdown(std::net::Shutdown::Write).unwrap();
    assert_eq!(exit_code(child), Some(0));
}

#[cfg(unix)]
#[test]
fn a_child_started_without_a_pipe_says_what_is_missing() {
    // Closed by the shell, whatever the test run has open there.
    let missing = |redirections| exit_code(by_shell("echo", redirections).spawn().unwrap());
    assert_eq!(missing("3<&- 4>&-"), Some(MISSING_INPUT));
    assert_eq!(missing("3</dev/null 4>&-"), Some(INPUT_NOT_A_PIPE));
    // A pipe at 3 (a shell's own), nothing at 4.
    let only_input = by_shell("echo", "4>&- 3<&0")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    assert_eq!(exit_code(only_input), Some(MISSING_OUTPUT));
    let file_output = by_shell("echo", "4>/dev/null 3<&0")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    assert_eq!(exit_code(file_output), Some(OUTPUT_NOT_A_PIPE));
}

#[cfg(unix)]
#[test]
fn a_process_with_3_and_4_free_starts_a_child_with_a_pipe() {
    // Its own pipes are made at 3 and 4, where the child's ends go.
    let launcher = by_shell("launcher", "3<&- 4>&-").spawn().unwrap();
    assert_eq!(exit_code(launcher), Some(0));
}

#[cfg(unix)]
#[test]
fn handles_named_on_macos_and_linux_are_let_be() {
    // As Chromium, which reads the switch on Windows alone: the child's
    // ends are 3 and 4 whatever it names.
    let mut command = command("echo");
    command.arg(format!("{}=1,2", platform::IO_PIPES_SWITCH));
    let (child, mut writer, reader) = platform::spawn_with_pipe(&mut command).unwrap();
    let mut reader = BufReader::new(reader);
    writeln!(writer, "named").unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert_eq!(line, "echo: named\n");
    drop(writer);
    assert_eq!(exit_code(child), Some(0));
}

#[cfg(windows)]
#[test]
fn a_child_named_no_pipe_says_why() {
    let with = |value: Option<&str>| {
        let mut command = command("echo");
        if let Some(value) = value {
            command.arg(format!("{}={value}", platform::IO_PIPES_SWITCH));
        }
        exit_code(command.spawn().unwrap())
    };
    assert_eq!(with(None), Some(NO_HANDLES));
    assert_eq!(with(Some("12")), Some(BAD_HANDLES));
    assert_eq!(with(Some("a,b")), Some(BAD_HANDLES));
    // Values that name no handle of the child's.
    assert_eq!(with(Some("1,2")), Some(INPUT_NOT_A_PIPE));
}

#[cfg(windows)]
#[test]
fn a_channels_child_started_meanwhile_has_no_end_of_a_pipe() {
    // Children with a channel, which stay, and children with a pipe are
    // started at once, several from each of a few threads. A handle is
    // inheritable while its child is started, by any child started then:
    // one of the channel's that had a pipe's ends would keep that pipe's
    // reader from its end once the pipe's own child is gone.
    const THREADS: usize = 4;
    const EACH: usize = 6;
    let together = std::sync::Arc::new(std::sync::Barrier::new(2 * THREADS));
    let (mut with_channel, mut with_pipe) = (Vec::new(), Vec::new());
    for _ in 0..THREADS {
        let start = together.clone();
        with_channel.push(thread::spawn(move || {
            start.wait();
            let started = (0..EACH).map(|_| platform::spawn(&mut command("stay")).unwrap());
            started.collect::<Vec<_>>()
        }));
        let start = together.clone();
        with_pipe.push(thread::spawn(move || {
            start.wait();
            let started =
                (0..EACH).map(|_| platform::spawn_with_pipe(&mut command("echo")).unwrap());
            started.collect::<Vec<_>>()
        }));
    }
    let staying: Vec<_> = with_channel
        .into_iter()
        .flat_map(|t| t.join().unwrap())
        .collect();
    for (child, writer, reader) in with_pipe.into_iter().flat_map(|t| t.join().unwrap()) {
        drop(writer);
        assert_eq!(exit_code(child), Some(0));
        assert!(ends_soon(reader));
    }
    for (mut child, _channel) in staying {
        // Still there: what the readers' ends were read beside.
        assert_eq!(child.try_wait().unwrap(), None);
        child.kill().unwrap();
        child.wait().unwrap();
    }
}

#[cfg(windows)]
#[test]
fn a_childs_output_is_looked_at_as_its_input_is() {
    // The child has a pipe for its input, and asks with another value
    // for its output.
    let found = |role| {
        let (child, _writer, _reader) = platform::spawn_with_pipe(&mut command(role)).unwrap();
        exit_code(child)
    };
    assert_eq!(found("output-no-pipe"), Some(OUTPUT_NOT_A_PIPE));
    // One handle for both ends would be owned, and closed, twice.
    assert_eq!(found("one-handle"), Some(BAD_HANDLES));
}
