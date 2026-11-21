//! macOS: Mach ports. A region of shared memory is a memory entry, a port
//! that stands for the memory, with no name and no descriptor.

pub mod memory;
pub mod message;
pub mod port;
pub mod rendezvous;
