//! Linux: a channel is a Unix stream socket, and a region of shared
//! memory a memfd, sealed so that nobody who holds it can change its
//! length.

pub mod channel;
pub mod memory;
