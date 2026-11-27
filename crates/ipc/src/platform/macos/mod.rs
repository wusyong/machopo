//! macOS: Mach ports. A region of shared memory is a memory entry, a port
//! that stands for the memory, with no name and no descriptor; an open
//! file is a fileport, a port that stands for the open file.

pub mod channel;
pub mod fileport;
pub mod memory;
pub mod message;
pub mod port;
pub mod rendezvous;
