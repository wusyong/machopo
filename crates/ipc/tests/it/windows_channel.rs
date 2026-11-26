//! What only Windows's channel has, between real processes: how a child
//! comes by its end, and that it doesn't outlive its host. It inherits
//! the end's handle and is told the handle's value by a variable of its
//! environment; once it has taken the end, neither is there for a child
//! of its own.
//!
//! The test is the host and starts itself again as the child (this
//! file's `child`), as `process_channel` does.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use ipc::endpoint::{Event, Platform, Waited, Waiter};
use ipc::platform::windows::channel::CHANNEL_VARIABLE;
use ipc::platform::{self, Channel};

use crate::process_channel::{ROLE, SOON, command};

const HAD_NO_END: i32 = 40;
const VARIABLE_LEFT: i32 = 41;
const TAKEN_TWICE: i32 = 42;
const GOT_AN_END: i32 = 43;
const SETUP: i32 = 44;

/// Where a child that stays says it runs: the test's listener's address.
const REPORT_TO: &str = "MACHOPO_IPC_TEST_REPORT_TO";
/// What it says it is: a child of `spawn`'s, or one started without it.
const SPAWNED: u8 = b's';
const STARTED_PLAINLY: u8 = b'p';

/// The test started again as `role`, by nothing of the platform's: it is
/// given no end.
fn plain(role: &str) -> Command {
    let mut command = command("windows_channel::child", role);
    command.stderr(Stdio::null());
    command
}

/// The child's side: is what `ROLE` says and exits with what came of it.
/// Without `ROLE` (the test run itself) it does nothing.
#[test]
fn child() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    thread::spawn(|| {
        thread::sleep(SOON * 3);
        std::process::exit(9);
    });
    let outcome = match role.as_str() {
        // Takes its end, and looks at what is left of how it came.
        "tidy" => match platform::inherited() {
            None => HAD_NO_END,
            Some(_end) if std::env::var_os(CHANNEL_VARIABLE).is_some() => VARIABLE_LEFT,
            Some(_end) if platform::inherited().is_some() => TAKEN_TWICE,
            Some(_end) => 0,
        },
        // Takes its end, starts a child of its own that outlives it, and
        // goes.
        "parent" => match platform::inherited() {
            None => HAD_NO_END,
            Some(end) => {
                // (Its own output isn't the grandchild's to hold open.)
                if plain("linger").stdin(Stdio::null()).spawn().is_err() {
                    std::process::exit(SETUP);
                }
                // Gone without a word, its end with it.
                std::mem::forget(end);
                0
            }
        },
        "linger" => {
            thread::sleep(Duration::from_secs(4));
            0
        }
        // A host of two children that never look at their channels, and
        // of one started without the platform.
        "host" => host(),
        // Says it runs and stays, hearing nothing of any channel, for as
        // long as the test holds the connection.
        "stays" => stays(SPAWNED),
        "stays-plainly" => stays(STARTED_PLAINLY),
        // Was given no end, whatever its environment says.
        "none" => no_end(),
        _ => SETUP,
    };
    std::process::exit(outcome);
}

fn no_end() -> i32 {
    if platform::inherited().is_some() {
        GOT_AN_END
    } else {
        0
    }
}

fn host() -> i32 {
    let mut kept = Vec::new();
    for _ in 0..2 {
        let mut command = command("windows_channel::child", "stays");
        match platform::spawn(command.stderr(Stdio::null())) {
            Ok(started) => kept.push(started),
            Err(_) => return SETUP,
        }
    }
    let Ok(_other) = plain("stays-plainly").spawn() else {
        return SETUP;
    };
    // Until it is killed.
    thread::sleep(SOON * 2);
    SETUP
}

fn stays(as_what: u8) -> i32 {
    let Some(mut test) = std::env::var(REPORT_TO)
        .ok()
        .and_then(|address| TcpStream::connect(address).ok())
    else {
        return SETUP;
    };
    if test.write_all(&[as_what]).is_err() {
        return SETUP;
    }
    let _ = test.read(&mut [0]);
    0
}

/// A child however it was started: by this layer, or by the standard
/// library.
trait Exited {
    fn code(self) -> Option<i32>;
}

impl Exited for ipc::platform::Child {
    fn code(mut self) -> Option<i32> {
        self.wait().unwrap().code()
    }
}

impl Exited for std::process::Child {
    fn code(mut self) -> Option<i32> {
        self.wait().unwrap().code()
    }
}

fn exit_code(child: impl Exited) -> Option<i32> {
    child.code()
}

#[test]
fn a_child_takes_its_end_once_and_leaves_no_word_of_it() {
    let (child, _channel) =
        platform::spawn(&mut command("windows_channel::child", "tidy")).unwrap();
    assert_eq!(exit_code(child), Some(0));
}

#[test]
fn a_command_is_left_as_it_was() {
    // Started by the platform, then by itself: the second child is told
    // of no end.
    let mut command = command("windows_channel::child", "none");
    let (first, _channel) = platform::spawn(&mut command).unwrap();
    // (The first had an end, and says so.)
    assert_eq!(exit_code(first), Some(GOT_AN_END));
    let told = command
        .get_envs()
        .any(|(name, value)| name == CHANNEL_VARIABLE && value.is_some());
    assert!(!told, "the command still names the first child's end");
    assert_eq!(exit_code(command.spawn().unwrap()), Some(0));
}

#[test]
fn a_variable_that_names_no_pipe_gives_no_end() {
    for value in [
        "",
        "0",
        "nothing",
        "-1",
        "99999999999999999999999999",
        "4294967292",
    ] {
        let child = plain("none").env(CHANNEL_VARIABLE, value).spawn().unwrap();
        assert_eq!(exit_code(child), Some(0), "{value:?}");
    }
    assert_eq!(exit_code(plain("none").spawn().unwrap()), Some(0), "unset");
}

/// Whether the host's end of `channel` hears its peer gone within
/// `soon`.
fn hears_its_peer_gone(channel: Channel, soon: Duration) -> bool {
    let (told, heard) = mpsc::channel();
    thread::spawn(move || {
        let mut waiter = <Channel as Platform>::Waiter::new().unwrap();
        assert!(waiter.add(0, &channel));
        loop {
            let gone = waiter.wait(|waited| {
                matches!(
                    waited,
                    Waited::Channel {
                        event: Event::PeerGone,
                        ..
                    }
                )
            });
            if gone {
                let _ = told.send(());
                return;
            }
        }
    });
    heard.recv_timeout(soon).is_ok()
}

#[test]
fn a_childs_own_child_has_no_end_of_the_channel() {
    // The child goes while its own child lives on: were the end
    // inherited again, the pipe would have an end yet, and the host
    // would hear nothing until the grandchild went too.
    let (child, channel) =
        platform::spawn(&mut command("windows_channel::child", "parent")).unwrap();
    assert_eq!(exit_code(child), Some(0));
    assert!(hears_its_peer_gone(channel, Duration::from_secs(2)));
}

#[test]
fn a_process_started_after_has_no_end_of_the_channel() {
    // By this process, without the platform, once the child has its
    // end: the end isn't this process's to hand on any more.
    let (mut child, channel) =
        platform::spawn(&mut command("process_channel::child", "echo")).unwrap();
    let mut other = plain("linger").spawn().unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(hears_its_peer_gone(channel, Duration::from_secs(2)));
    other.kill().unwrap();
    other.wait().unwrap();
}

#[test]
fn children_started_at_once_each_have_their_own_end_alone() {
    // Eight at a time, from as many threads: each child killed is heard
    // by its host at once, which it wouldn't be if another child had
    // inherited its end.
    let started: Vec<_> = (0..8)
        .map(|_| {
            thread::spawn(|| {
                platform::spawn(&mut command("process_channel::child", "echo")).unwrap()
            })
        })
        .collect();
    let mut started: Vec<_> = started
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    let mut left = Vec::new();
    while let Some((mut child, channel)) = started.pop() {
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(
            hears_its_peer_gone(channel, Duration::from_secs(2)),
            "with {} others alive",
            started.len()
        );
        left.push(child);
    }
}

/// Whether the process at the other end of `connection` ends within
/// `soon`: a socket isn't inherited, so nothing else holds it open.
fn ends(connection: &mut TcpStream, soon: Duration) -> bool {
    connection.set_read_timeout(Some(soon)).unwrap();
    match connection.read(&mut [0]) {
        Ok(read) => read == 0,
        Err(error) => !matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
    }
}

#[test]
fn a_hosts_children_go_with_it_whatever_they_are_doing() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let mut host = plain("host")
        .env(REPORT_TO, address)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let (came, connections) = mpsc::channel();
    thread::spawn(move || {
        for _ in 0..3 {
            let (mut connection, _) = listener.accept().unwrap();
            let mut as_what = [0];
            connection.read_exact(&mut as_what).unwrap();
            if came.send((as_what[0], connection)).is_err() {
                return;
            }
        }
    });
    let mut spawned = Vec::new();
    let mut plainly = Vec::new();
    for _ in 0..3 {
        match connections.recv_timeout(SOON).expect("its children run") {
            (SPAWNED, connection) => spawned.push(connection),
            (_, connection) => plainly.push(connection),
        }
    }
    assert_eq!((spawned.len(), plainly.len()), (2, 1));

    // While it lives, so do they.
    for child in &mut spawned {
        assert!(!ends(child, Duration::from_millis(300)), "gone before it");
    }
    host.kill().unwrap();
    host.wait().unwrap();
    for child in &mut spawned {
        assert!(ends(child, Duration::from_secs(5)), "left behind");
    }
    // What it started by itself is none of the platform's.
    assert!(
        !ends(&mut plainly[0], Duration::from_millis(300)),
        "a process spawn didn't start went with it"
    );
}

/// A measurement, not a check: a host and echoing children over the real
/// channel, what each kind of message costs.
/// `cargo test -p ipc --test it -- --ignored --exact windows_channel::measure --nocapture`
#[test]
#[ignore = "a measurement, run by hand"]
fn measure() {
    use std::time::Instant;

    use bytes::Bytes;
    use ipc::core::frame::BadMessage;
    use ipc::endpoint::{self, Handler, IoLoop, Link, Received};

    use crate::process_channel::{Msg, config};

    struct Count(mpsc::Sender<usize>, usize);
    impl Handler<Channel> for Count {
        fn message(&mut self, mut message: Received<'_, Channel>) -> Result<(), BadMessage> {
            let _ = message.decode::<Msg>()?;
            let _ = self.0.send(self.1);
            Ok(())
        }
    }

    let (io, handle) = IoLoop::<Channel>::new().unwrap();
    thread::spawn(move || io.run());
    let (told, came) = mpsc::channel();
    let mut children = Vec::new();
    let links: Vec<Link<Channel>> = (0..4)
        .map(|name| {
            let (child, channel) =
                platform::spawn(&mut command("process_channel::child", "echo")).unwrap();
            let (link, endpoint) = endpoint::endpoint(channel, config(true));
            handle.serve(endpoint, Count(told.clone(), name));
            link.grant_to_peer(8 << 20);
            children.push(child);
            link
        })
        .collect();
    let wait = |count: usize| {
        for _ in 0..count {
            came.recv_timeout(SOON).expect("an echo");
        }
    };
    // Warm: regions made, threads awake.
    for link in &links {
        link.send(0, &Msg::Body(Bytes::from(vec![1u8; 8 << 20])))
            .unwrap();
    }
    wait(4);
    let median = |mut runs: Vec<f64>| {
        runs.sort_by(f64::total_cmp);
        (runs[0], runs[runs.len() / 2])
    };
    let report = |what: &str, per: &str, runs: Vec<f64>| {
        let (least, middle) = median(runs);
        println!("MEASURE {what:<44} {middle:>9.2} {per} (least {least:.2})");
    };
    let small = Msg::Text("ping".into());
    let one = &links[0];

    report(
        "small, one at a time (round trip)",
        "us",
        (0..7)
            .map(|_| {
                let began = Instant::now();
                for _ in 0..20_000 {
                    one.send(0, &small).unwrap();
                    wait(1);
                }
                began.elapsed().as_secs_f64() * 1e6 / 20_000.0
            })
            .collect(),
    );

    report(
        "small, 20000 sent at once (each, there and back)",
        "us",
        (0..7)
            .map(|_| {
                let began = Instant::now();
                for _ in 0..20_000 {
                    one.send(0, &small).unwrap();
                }
                wait(20_000);
                began.elapsed().as_secs_f64() * 1e6 / 20_000.0
            })
            .collect(),
    );

    report(
        "small, 4 children, 5000 each at once (each)",
        "us",
        (0..7)
            .map(|_| {
                let began = Instant::now();
                thread::scope(|scope| {
                    for link in &links {
                        scope.spawn(|| {
                            for _ in 0..5_000 {
                                link.send(0, &small).unwrap();
                            }
                        });
                    }
                });
                wait(20_000);
                began.elapsed().as_secs_f64() * 1e6 / 20_000.0
            })
            .collect(),
    );

    for (kib, rounds) in [(48usize, 400usize), (1024, 60), (8192, 12)] {
        let string = Msg::Text("s".repeat(kib << 10));
        report(
            &format!("{kib} KiB in the message (round trip)"),
            "us",
            (0..5)
                .map(|_| {
                    let began = Instant::now();
                    for _ in 0..rounds {
                        one.send(0, &string).unwrap();
                        wait(1);
                    }
                    began.elapsed().as_secs_f64() * 1e6 / rounds as f64
                })
                .collect(),
        );
    }
    for (kib, rounds) in [(64usize, 400usize), (1024, 100), (8192, 20)] {
        let body = Msg::Body(Bytes::from(vec![7u8; kib << 10]));
        report(
            &format!("{kib} KiB by a region (round trip)"),
            "us",
            (0..5)
                .map(|_| {
                    let began = Instant::now();
                    for _ in 0..rounds {
                        one.send(0, &body).unwrap();
                        wait(1);
                    }
                    began.elapsed().as_secs_f64() * 1e6 / rounds as f64
                })
                .collect(),
        );
    }
    handle.stop();
    for mut child in children {
        let _ = child.wait();
    }
}
