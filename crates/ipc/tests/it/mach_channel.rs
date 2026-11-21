//! What only macOS's channel has, between real processes (what every
//! platform's has is in `process_channel.rs`): the kernel's
//! notifications, which a child can forge the ids of; bodies out of
//! line, which a child can make longer than any message; and the audit
//! token, by which a message from another process is nothing.

use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use ipc::core::channel::Gone;
use ipc::core::frame::Encoder;
use ipc::platform::macos::channel::{Channel, MESSAGE_ID};
use ipc::platform::macos::message::{self, Buffer};
use ipc::platform::macos::port::{ReceiveRight, Right, SendRight};
use ipc::platform::macos::rendezvous::{self, Server};

use super::process_channel::{
    Host, KIB, OTHER, PEER_CLOSED, ROLE, SETUP, SOON, command, frame, text,
};

/// The rights of the child's end, for a child that sends Mach messages
/// by hand.
fn raw_rights() -> Option<(ReceiveRight, SendRight)> {
    let host = std::os::unix::process::parent_id();
    let mut rights = rendezvous::fetch(host, SOON).ok()?.into_iter();
    match (rights.next(), rights.next()) {
        (Some(Right::Receive(mine)), Some(Right::Send(to_host))) => Some((mine, to_host)),
        _ => None,
    }
}

/// The child's side: is what `ROLE` says and exits with what came of it.
/// Without `ROLE` (the test run itself) it does nothing.
#[test]
fn child() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    let outcome = raw_rights().map_or(SETUP, |(mine, to_host)| {
        let mut buffer = Buffer::with_out_of_line(64 * KIB, 256 << 20);
        let mut send =
            |id: i32, body: &[u8]| message::try_send(&mut buffer, &to_host, id, body, &[]).is_ok();
        match role.as_str() {
            // Messages with the ids of the kernel's notifications, and
            // one of no id the channel knows, each with a body that
            // would be a good message were its id the core's; then a
            // good message.
            "forged" => {
                let forged = frame(&mut Encoder::new(), 0, &text("forged"), 0);
                for id in [0o110, 0o106, 0o102, 0o107, 12345] {
                    assert!(send(id, &forged));
                }
                let good = frame(&mut Encoder::new(), 1, &text("still here"), 0);
                assert!(send(MESSAGE_ID, &good));
            }
            // More out of line than any message of the core's is long.
            "huge" => assert!(send(MESSAGE_ID, &vec![0u8; (128 << 20) + 64 * KIB])),
            _ => return SETUP,
        }
        wait_for_the_host_to_go(&mine)
    });
    std::process::exit(outcome);
}

/// Waits for the host's end to go: the kernel says so on this port (the
/// last right to it is let go). What the host says meanwhile is let go.
fn wait_for_the_host_to_go(mine: &ReceiveRight) -> i32 {
    assert!(mine.tell_when_no_senders(1));
    let mut buffer = Buffer::with_out_of_line(64 * KIB, 256 << 20);
    loop {
        match message::receive(&mut buffer, mine.name(), Some(SOON)) {
            Ok(told) if told.sender.pid() == 0 && told.id == 0o106 => return PEER_CLOSED,
            Ok(_) => {}
            Err(_) => return OTHER,
        }
    }
}

/// Starts the child `role` of `entry`, with a channel to it made by
/// hand: `known_as` is the pid the host takes the child to have (its
/// own, unless the test says).
fn start(entry: &str, role: &str, known_as: Option<u32>) -> Host {
    let child = command(entry, role).spawn().unwrap();
    let (channel, rights) = Channel::pair(known_as.unwrap_or(child.id())).unwrap();
    Server::global().unwrap().register(child.id(), rights);
    Host::over(channel, child)
}

#[test]
fn a_notification_the_kernel_didnt_send_is_nothing() {
    let host = start("mach_channel::child", "forged", None);
    assert_eq!(host.next(), (1, text("still here")));
    // The channel is as it was: a pause, and nothing ended it.
    assert_eq!(
        host.got.recv_timeout(Duration::from_millis(300)),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(!host.io.is_finished());
    host.link.close();
    assert_eq!(host.end(), (Gone::Closed, PEER_CLOSED));
}

#[test]
fn more_out_of_line_than_a_message_is_long_is_no_message() {
    let host = start("mach_channel::child", "huge", None);
    assert_eq!(host.end(), (Gone::PeerClosed, PEER_CLOSED));
}

#[test]
fn messages_from_a_process_that_isnt_the_peer_are_nothing() {
    // The host takes its peer to be another process (launchd): what the
    // child sends comes from the wrong one.
    let mut host = start("process_channel::child", "echo", Some(1));
    host.link.send(0, &text("hello")).unwrap();
    assert_eq!(
        host.got.recv_timeout(Duration::from_millis(700)),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(!host.io.is_finished(), "nor do they end the channel");
    host.child.kill().unwrap();
    let (gone, _) = host.end();
    assert_eq!(gone, Gone::PeerClosed);
}
