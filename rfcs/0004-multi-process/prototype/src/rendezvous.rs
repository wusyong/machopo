//! How a child gets its ports (Chromium's `MachPortRendezvous`): the host
//! checks a server in with launchd's bootstrap under a name of its own pid;
//! a child looks the name of its parent's pid up and asks; the server
//! answers each child, known by the kernel's audit token, with the rights
//! registered for its pid, once.

use std::collections::HashMap;
use std::ffi::CString;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use mach2::bootstrap::{bootstrap_check_in, bootstrap_look_up, bootstrap_port};
use mach2::kern_return::{KERN_SUCCESS, kern_return_t};
use mach2::message::{MACH_MSG_TYPE_MAKE_SEND_ONCE, MACH_MSG_TYPE_MOVE_SEND_ONCE, mach_msg_type_name_t};
use mach2::port::{MACH_PORT_NULL, mach_port_t};

use crate::msg::{self, MsgBuf, Outgoing};

const REQUEST_ID: i32 = 0x5245_5131;
const REPLY_ID: i32 = 0x5245_5132;

pub fn name_for(host_pid: u32) -> String {
    format!("org.machopo.mp-prototype.rendezvous.{host_pid}")
}

type Rights = Vec<(mach_port_t, mach_msg_type_name_t)>;

#[derive(Default)]
struct Registry {
    by_pid: Mutex<HashMap<i32, Rights>>,
    registered: Condvar,
    /// Requests refused: from a pid with nothing registered.
    refused: Mutex<Vec<i32>>,
}

pub struct Server {
    registry: Arc<Registry>,
}

impl Server {
    /// Checks the server in under this process's name and serves on a
    /// thread of its own.
    pub fn start() -> Result<Self, kern_return_t> {
        let name = CString::new(name_for(std::process::id())).unwrap();
        let mut port = MACH_PORT_NULL;
        // SAFETY: a call with a C string and a place for one name.
        let kr = unsafe { bootstrap_check_in(bootstrap_port, name.as_ptr(), &mut port) };
        if kr != KERN_SUCCESS {
            return Err(kr);
        }
        let registry = Arc::new(Registry::default());
        let serving = registry.clone();
        std::thread::Builder::new()
            .name("rendezvous".into())
            .spawn(move || serve(port, &serving))
            .unwrap();
        Ok(Self { registry })
    }

    /// The rights the child `pid` gets when it asks.
    pub fn register(&self, pid: u32, rights: Rights) {
        self.registry.by_pid.lock().unwrap().insert(pid.cast_signed(), rights);
        self.registry.registered.notify_all();
    }

    pub fn refused(&self) -> Vec<i32> {
        self.registry.refused.lock().unwrap().clone()
    }
}

fn serve(port: mach_port_t, registry: &Registry) {
    let mut buf = MsgBuf::with_capacity(1024);
    loop {
        let Ok(request) = msg::recv(&mut buf, port, None) else {
            continue;
        };
        for &port in &request.ports {
            msg::deallocate(port);
        }
        if request.id != REQUEST_ID || request.reply == MACH_PORT_NULL {
            msg::deallocate(request.reply);
            continue;
        }
        // The child may ask before its pid is registered (the pid is known
        // only once it has been spawned): wait a little for it.
        let rights = {
            let by_pid = registry.by_pid.lock().unwrap();
            let (mut by_pid, _) = registry
                .registered
                .wait_timeout_while(by_pid, Duration::from_millis(500), |by_pid| {
                    !by_pid.contains_key(&request.sender_pid)
                })
                .unwrap();
            by_pid.remove(&request.sender_pid)
        };
        let Some(rights) = rights else {
            registry.refused.lock().unwrap().push(request.sender_pid);
            // Dropping the send-once right tells the asker (a send-once
            // notification on its reply port).
            msg::deallocate(request.reply);
            continue;
        };
        let reply = Outgoing {
            id: REPLY_ID,
            destination: 0,
            payload: &[],
            ports: &rights,
            ool: None,
        };
        let sent = msg::send(&mut buf, request.reply, MACH_MSG_TYPE_MOVE_SEND_ONCE, None, &reply, None);
        if let Err(kr) = sent {
            eprintln!("rendezvous: the reply to {} failed: {kr:#x}", request.sender_pid);
        }
    }
}

#[derive(Debug)]
pub enum FetchError {
    LookUp(kern_return_t),
    Send(kern_return_t),
    /// The server had nothing for this process.
    Refused,
}

/// A child's side: the rights registered for this process with the server
/// of `host_pid`, in the order registered.
pub fn fetch(host_pid: u32) -> Result<Vec<mach_port_t>, FetchError> {
    let name = CString::new(name_for(host_pid)).unwrap();
    let mut server = MACH_PORT_NULL;
    // SAFETY: a call with a C string and a place for one name.
    let kr = unsafe { bootstrap_look_up(bootstrap_port, name.as_ptr(), &mut server) };
    if kr != KERN_SUCCESS {
        return Err(FetchError::LookUp(kr));
    }
    let reply_port = msg::allocate_receive();
    let mut buf = MsgBuf::with_capacity(1024);
    let request = Outgoing {
        id: REQUEST_ID,
        destination: 0,
        payload: &[],
        ports: &[],
        ool: None,
    };
    let sent = msg::send(
        &mut buf,
        server,
        msg::COPY_SEND,
        Some((reply_port, MACH_MSG_TYPE_MAKE_SEND_ONCE)),
        &request,
        None,
    );
    msg::deallocate(server);
    if let Err(kr) = sent {
        msg::drop_receive(reply_port);
        return Err(FetchError::Send(kr));
    }
    let reply = msg::recv(&mut buf, reply_port, Some(2000));
    msg::drop_receive(reply_port);
    match reply {
        Ok(reply) if reply.id == REPLY_ID => Ok(reply.ports),
        _ => Err(FetchError::Refused),
    }
}
