//! How a child whose host is gone ends on macOS and Linux
//! (`terminate_current_process_immediately`): at once, from whichever
//! thread says so, with nothing of an ordinary exit run. The test starts
//! itself again as the child, as `process_channel` does.

use std::io::Read;
use std::process::Stdio;
use std::thread;

use ipc::platform::terminate_current_process_immediately;

use crate::process_channel::{ROLE, SOON, command};

const STATUS: i32 = 26;
/// What the child leaves in its standard output's buffer.
const UNWRITTEN: &str = "left in the buffer";

/// The child's side. Without `ROLE` (the test run itself) it does
/// nothing.
#[test]
fn child() {
    if std::env::var(ROLE).as_deref() != Ok("ends at once") {
        return;
    }
    // Should the process not be ended, it ends anyway, and says so by
    // its status.
    thread::spawn(|| {
        thread::sleep(SOON);
        std::process::abort();
    });
    // No newline: to a pipe, it stays in the buffer until one comes or
    // the process exits as processes do.
    print!("{UNWRITTEN}");
    thread::spawn(|| terminate_current_process_immediately(STATUS));
    // This thread never ends, as one in a script that never returns.
    loop {
        std::hint::spin_loop();
    }
}

#[test]
fn a_process_ended_at_once_is_whatever_its_threads_do_and_flushes_nothing() {
    let mut child = command("unix_exit::child", "ends at once")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut said = String::new();
    // To the end: the process is over, its end of the pipe closed.
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut said)
        .unwrap();
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(STATUS), "{status:?}");
    assert!(!said.contains(UNWRITTEN), "{said}");
    // It was running: what it wrote whole is there.
    assert!(said.contains("running 1 test"), "{said}");
}
