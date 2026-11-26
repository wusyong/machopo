//! A process's sandbox, entered by the process itself as it starts
//! (Chromium's `sandbox/`: the mechanism, with no process's policy in it).
//! A process that is about to handle what it can't trust builds a policy,
//! what it will still need of the kernel, and enters it; from then on the
//! kernel refuses the rest, whatever the process's own code has become.
//!
//! One module for each operating system, since what a policy says is the
//! operating system's: `linux` (Landlock for the files, seccomp-bpf for
//! the system calls) and `macos` (the kernel's sandbox under a profile).
//!
//! `unsafe` is allowed here: the signal handler that says which system
//! call was refused, the system's function a profile is entered with, and
//! the raw calls a policy's test makes. Each
//! use says why it is sound; `soundness.md` has the whole argument.

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]
#![allow(
    unsafe_code,
    reason = "the signal handler for a refused system call; entering a profile; a policy's test makes raw system calls"
)]

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
