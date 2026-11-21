//! What only Linux's channel has, between real processes (what every
//! platform's has is in `process_channel.rs`): how a child comes by its
//! end. It inherits one descriptor, named by an environment variable,
//! and nothing else its host has open.

use std::net::{TcpListener, UdpSocket};
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::process::Stdio;

use ipc::core::channel::Gone;
use ipc::platform::linux::channel::Channel;
use ipc::platform::linux::memory;
use ipc::platform::{self};
use rustix::io::{FdFlags, fcntl_setfd};

use super::process_channel::{Host, ROLE, SETUP, command};

/// What a child finds its end by.
const INHERITED_FD: &str = "MACHOPO_IPC_FD";

const HAS_NONE: i32 = 9;
const HAS_ONE: i32 = 10;

/// What this process has open, but for the three it starts with: each
/// descriptor's number and what `/proc` says it is.
fn open_descriptors() -> Vec<(i32, String)> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir("/proc/self/fd").unwrap() {
        let entry = entry.unwrap();
        let number: i32 = entry.file_name().to_str().unwrap().parse().unwrap();
        // The directory being read is one too, gone by the time it is
        // asked about.
        if let Ok(target) = std::fs::read_link(entry.path())
            && number > 2
        {
            found.push((number, target.to_string_lossy().into_owned()));
        }
    }
    found
}

/// Whether descriptor `number` is closed when this process starts
/// another.
fn close_on_exec(number: i32) -> bool {
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{number}")).unwrap();
    let flags = info.lines().find_map(|line| line.strip_prefix("flags:"));
    let flags = u32::from_str_radix(flags.unwrap().trim(), 8).unwrap();
    flags & 0o2_000_000 != 0
}

/// The child's side: is what `ROLE` says and exits with what came of it.
/// Without `ROLE` (the test run itself) it does nothing.
#[test]
fn child() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    let outcome = match role.as_str() {
        // What it has open once it took its end: exits with 0 if that
        // is its end alone, kept from whatever it starts itself.
        "fds" => {
            let before = open_descriptors();
            let Some(_channel) = platform::inherited() else {
                std::process::exit(SETUP);
            };
            let after = open_descriptors();
            let sockets: Vec<_> = after
                .iter()
                .filter(|(_, target)| target.starts_with("socket:"))
                .collect();
            let regions = after.iter().filter(|(_, t)| t.contains("memfd:")).count();
            if platform::inherited().is_some() {
                41
            } else if before.len() != after.len() {
                // Taking the end opens and closes nothing.
                42
            } else if sockets.len() != 1 {
                43
            } else if regions != 0 {
                44
            } else if !close_on_exec(sockets[0].0) {
                45
            } else {
                0
            }
        }
        // Whether there is an end for it.
        "any" => match platform::inherited() {
            Some(_) => HAS_ONE,
            None => HAS_NONE,
        },
        _ => SETUP,
    };
    std::process::exit(outcome);
}

#[test]
fn a_child_has_its_end_and_nothing_else_of_its_hosts() {
    // What a host has open when it starts a child: other children's
    // ends, regions.
    let _other = Channel::pair().unwrap();
    let _region = memory::create(64 << 10, true).unwrap();
    let (mut child, _channel) =
        platform::spawn(&mut command("linux_channel::child", "fds")).unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(0));
}

#[test]
fn a_host_hears_its_child_go_while_what_started_it_is_kept() {
    // The command outlives the child: nothing it holds is the child's
    // end, which would keep the host from reading the stream's end.
    let mut started_by = command("process_channel::child", "die");
    let (child, channel) = platform::spawn(&mut started_by).unwrap();
    assert_eq!(Host::over(channel, child).end(), (Gone::PeerClosed, 7));
    drop(started_by);
}

/// What a child started with `fd` named as its end exits with; `fd` is
/// left open in it.
fn child_told_of(fd: Option<&OwnedFd>, named: Option<&str>) -> Option<i32> {
    let mut command = command("linux_channel::child", "any");
    command.stdin(Stdio::null());
    match (fd, named) {
        (Some(fd), _) => {
            // For this child (and, harmlessly, any another test starts
            // meanwhile).
            fcntl_setfd(fd.as_fd(), FdFlags::empty()).unwrap();
            command.env(INHERITED_FD, fd.as_raw_fd().to_string());
        }
        (None, Some(named)) => {
            command.env(INHERITED_FD, named);
        }
        (None, None) => {
            command.env_remove(INHERITED_FD);
        }
    }
    let code = command.status().unwrap().code();
    if let Some(fd) = fd {
        fcntl_setfd(fd.as_fd(), FdFlags::CLOEXEC).unwrap();
    }
    code
}

#[test]
fn a_process_told_of_no_end_or_of_what_isnt_one_has_none() {
    assert_eq!(child_told_of(None, None), Some(HAS_NONE));
    for named in ["", "abc", "-1", "0", "1", "2", "99999", "3.5"] {
        assert_eq!(child_told_of(None, Some(named)), Some(HAS_NONE), "{named}");
    }
    // Open in the child, and not a Unix stream socket.
    let (_mapping, region) = memory::create(64 << 10, true).unwrap();
    assert_eq!(child_told_of(Some(&region), None), Some(HAS_NONE));
    let (datagram, _other) = UnixDatagram::pair().unwrap();
    assert_eq!(child_told_of(Some(&datagram.into()), None), Some(HAS_NONE));
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    assert_eq!(child_told_of(Some(&tcp.into()), None), Some(HAS_NONE));
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    assert_eq!(child_told_of(Some(&udp.into()), None), Some(HAS_NONE));
    // A Unix stream socket as one of the three a process starts with
    // isn't an end either.
    let (stdin, _other) = UnixStream::pair().unwrap();
    let mut command = command("linux_channel::child", "any");
    command.stdin(OwnedFd::from(stdin)).env(INHERITED_FD, "0");
    assert_eq!(command.status().unwrap().code(), Some(HAS_NONE));
    // And what is one, however it came to be open there.
    let (stream, _other) = UnixStream::pair().unwrap();
    assert_eq!(child_told_of(Some(&stream.into()), None), Some(HAS_ONE));
}
