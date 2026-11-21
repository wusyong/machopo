//! Linux: a region of shared memory is a memfd, sealed so that nobody
//! who holds it can change its length.

pub mod memory;
