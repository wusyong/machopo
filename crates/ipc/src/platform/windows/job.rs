//! The jobs that end this process's children with it (Chromium's
//! `CreateUnsandboxedJob` in `sandbox/policy/win/sandbox_win.cc`, with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`). A child that hears its channel
//! is over ends by itself only if the thread that has to act on it is
//! free to; a job needs nothing of the child. Its only handle is this
//! process's and is never closed: the kernel closes it when the process
//! ends, however that comes about, and ends what is in the job.
//!
//! One job is enough (as Chromium has one), until this process is itself
//! put in another job after its first child was: children started from
//! then on can't join the job the earlier ones are in, and get a new one.

use std::io;
use std::os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::ptr::null;
use std::sync::{Mutex, PoisonError};

use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows_sys::Win32::System::Threading::WaitForSingleObject;

/// How long a child that no job takes is given to turn out to be one
/// that is exiting. The kernel refuses such a child from a moment before
/// it says the process is over (16 ms at most, measured).
const EXITING_MS: u32 = 1000;

/// Jobs that each end their processes when their handle closes, the one
/// that takes children now last.
struct Jobs(Mutex<Vec<OwnedHandle>>);

/// Makes `child` one that ends when this process does. It is an error
/// when `child` runs on and can't be made one; one that is exiting
/// needn't be.
pub(super) fn end_with_this_process(child: BorrowedHandle<'_>) -> io::Result<()> {
    // Never dropped, as statics aren't.
    static OF_THIS_PROCESS: Jobs = Jobs::new();
    OF_THIS_PROCESS.add(child)
}

impl Jobs {
    const fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }

    fn add(&self, child: BorrowedHandle<'_>) -> io::Result<()> {
        let Err(refused) = self.assign(child) else {
            return Ok(());
        };
        if exits_within(child, EXITING_MS) {
            Ok(())
        } else {
            Err(refused)
        }
    }

    fn assign(&self, child: BorrowedHandle<'_>) -> io::Result<()> {
        // Nothing panics while holding it.
        let mut jobs = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(job) = jobs.last()
            && assign_to(job, child).is_ok()
        {
            return Ok(());
        }
        let job = kill_on_close()?;
        assign_to(&job, child)?;
        // The ones before are kept: closing one ends the children in it.
        jobs.push(job);
        Ok(())
    }
}

/// A new job, without a name, that no child inherits and that ends what
/// is in it when its handle closes.
fn kill_on_close() -> io::Result<OwnedHandle> {
    // SAFETY: no attributes (so the handle isn't inherited) and no name:
    // nothing is read through either pointer.
    let job = unsafe { CreateJobObjectW(null(), null()) };
    if job.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a new handle, checked, that nothing else has.
    let job = unsafe { OwnedHandle::from_raw_handle(job) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let len = u32::try_from(size_of_val(&limits)).map_err(io::Error::other)?;
    // SAFETY: the job is open for the call, and `limits` is the structure
    // this class reads, `len` long and alive for the call.
    let set = unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            len,
        )
    };
    if set == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

fn assign_to(job: &OwnedHandle, child: BorrowedHandle<'_>) -> io::Result<()> {
    // SAFETY: both handles are open for the call; the kernel checks what
    // they are and what they allow.
    let assigned = unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) };
    if assigned == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn exits_within(child: BorrowedHandle<'_>, milliseconds: u32) -> bool {
    // SAFETY: the handle is open for the call, which writes nothing.
    unsafe { WaitForSingleObject(child.as_raw_handle(), milliseconds) == WAIT_OBJECT_0 }
}

#[cfg(test)]
mod tests {
    use std::os::windows::io::AsHandle;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{
        DuplicateHandle, GetHandleInformation, HANDLE_FLAG_INHERIT,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };

    use super::*;

    /// A child that runs until its input closes.
    fn waiting() -> Child {
        Command::new("cmd.exe")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn exiting() -> Child {
        Command::new("cmd.exe")
            .args(["/c", "exit 7"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn ended(child: &mut Child) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() > deadline {
                let _ = child.kill();
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        true
    }

    fn count(jobs: &Jobs) -> usize {
        jobs.0.lock().unwrap().len()
    }

    #[test]
    fn children_share_a_job_and_end_when_it_closes() {
        let jobs = Jobs::new();
        let mut children = [waiting(), waiting()];
        for child in &children {
            jobs.add(child.as_handle()).unwrap();
        }
        assert_eq!(count(&jobs), 1);
        for child in &mut children {
            assert!(child.try_wait().unwrap().is_none(), "ended with it open");
        }
        drop(jobs);
        for child in &mut children {
            assert!(ended(child), "left when its job closed");
        }
    }

    #[test]
    fn a_job_isnt_inherited() {
        let job = kill_on_close().unwrap();
        let mut flags = 0;
        // SAFETY: the job is open for the call, which writes `flags`.
        assert!(unsafe { GetHandleInformation(job.as_raw_handle(), &raw mut flags) } != 0);
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
    }

    #[test]
    fn a_child_the_job_cant_take_gets_another_and_the_first_stays() {
        let jobs = Jobs::new();
        let mut first = waiting();
        jobs.add(first.as_handle()).unwrap();
        // In a job the first child isn't in, as every child is once this
        // process has been put in one: the job with the first child in
        // it can't be nested in that.
        let elsewhere = kill_on_close().unwrap();
        let mut second = waiting();
        assign_to(&elsewhere, second.as_handle()).unwrap();
        jobs.add(second.as_handle()).unwrap();
        assert_eq!(count(&jobs), 2);
        assert!(first.try_wait().unwrap().is_none(), "its job was closed");
        drop(jobs);
        assert!(ended(&mut first), "the first left");
        assert!(ended(&mut second), "the second left");
    }

    #[test]
    fn a_child_that_has_exited_needs_no_job() {
        let jobs = Jobs::new();
        let mut child = exiting();
        child.wait().unwrap();
        jobs.add(child.as_handle()).unwrap();
        assert_eq!(count(&jobs), 0);
        assert_eq!(child.wait().unwrap().code(), Some(7));
    }

    #[test]
    fn a_child_that_is_exiting_needs_no_job() {
        // Asked over and over until it is over, so also in the moment in
        // which it is refused and not over yet.
        let jobs = Jobs::new();
        for _ in 0..20 {
            let mut child = exiting();
            loop {
                jobs.add(child.as_handle()).unwrap();
                if child.try_wait().unwrap().is_some() {
                    break;
                }
            }
            assert_eq!(child.wait().unwrap().code(), Some(7));
        }
    }

    #[test]
    fn a_child_that_runs_on_and_cant_join_is_an_error() {
        let jobs = Jobs::new();
        let mut child = waiting();
        let mut limited = std::ptr::null_mut();
        // SAFETY: the child's handle is open for the call, both processes
        // are this one, and the new handle is written to `limited`.
        let duplicated = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                child.as_raw_handle(),
                GetCurrentProcess(),
                &raw mut limited,
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                0,
            )
        };
        assert!(duplicated != 0);
        // SAFETY: a new handle that nothing else has.
        let limited = unsafe { OwnedHandle::from_raw_handle(limited) };
        let added = jobs.add(limited.as_handle());
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(added.is_err());
    }
}
