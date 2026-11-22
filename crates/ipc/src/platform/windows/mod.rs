//! Windows. A channel's end is a named pipe's; a region of shared memory
//! is a section of the page file, with no name. A child started with a
//! channel is in a job that ends it with this process.

pub mod channel;
mod job;
pub mod memory;

/// Held by a test that counts this process's handles, or has hundreds
/// of its own at once: the count is the whole process's, and another
/// such test's handles would be taken for this one's.
#[cfg(test)]
static COUNTING_HANDLES: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
fn counting_handles() -> std::sync::MutexGuard<'static, ()> {
    COUNTING_HANDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
