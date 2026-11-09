//! `gc_platform`: the elise heap's jobs on V8's workers. A job runs on the
//! calling thread, its seed's, even asking for no worker, and on the
//! workers it asks for; `run` returns once their runs have; a posted job
//! runs on workers alone until cancelled (or its handle dropped); a run's
//! panic reaches `run`'s caller, or `cancel`'s.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use elise::{Job, JobDelegate};

/// Counts its runs, asking for `want` workers until one has run.
struct Counting {
    want: usize,
    runs: AtomicUsize,
    workers: AtomicUsize,
    panic_on_worker: bool,
}

impl Job for Counting {
    #[expect(clippy::disallowed_methods, reason = "tests' counters")]
    fn run(&self, delegate: &dyn JobDelegate) {
        self.runs.fetch_add(1, Ordering::Relaxed);
        if !delegate.is_joining_thread() {
            self.workers.fetch_add(1, Ordering::Relaxed);
            assert!(!self.panic_on_worker, "a worker's panic");
            return;
        }
        // The joining thread waits for a worker, if it asked for one: a
        // while, not forever.
        wait_until("a worker", || {
            self.want == 0 || self.workers.load(Ordering::Relaxed) > 0
        });
    }

    fn max_concurrency(&self, active: usize) -> usize {
        if self.workers.load(Ordering::Relaxed) > 0 {
            active
        } else {
            self.want
        }
    }
}

fn counting(want: usize) -> Counting {
    Counting {
        want,
        runs: AtomicUsize::new(0),
        workers: AtomicUsize::new(0),
        panic_on_worker: false,
    }
}

/// Waits until `done`, a while, not forever.
fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "{what} never came");
        std::thread::yield_now();
    }
}

#[test]
fn a_job_runs_on_the_caller_and_the_workers_it_asks_for() {
    let platform = bindings::gc_platform();
    let job = counting(1);
    platform.run(&job);
    assert!(job.workers.load(Ordering::Relaxed) >= 1);
    // Asking for no worker (its work the caller's alone), it runs on the
    // caller all the same.
    let job = counting(0);
    platform.run(&job);
    assert_eq!(job.runs.load(Ordering::Relaxed), 1);
    assert_eq!(job.workers.load(Ordering::Relaxed), 0);
}

/// A worker still inside its run when the joining thread's returns.
struct Slow {
    started: AtomicUsize,
    inside: AtomicUsize,
}

impl Job for Slow {
    #[expect(clippy::disallowed_methods, reason = "tests' counters")]
    fn run(&self, delegate: &dyn JobDelegate) {
        if delegate.is_joining_thread() {
            wait_until("a worker", || self.started.load(Ordering::SeqCst) > 0);
            return;
        }
        self.inside.fetch_add(1, Ordering::SeqCst);
        self.started.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(50));
        self.inside.fetch_sub(1, Ordering::SeqCst);
    }

    fn max_concurrency(&self, active: usize) -> usize {
        if self.started.load(Ordering::SeqCst) > 0 {
            active
        } else {
            1
        }
    }
}

/// `run` returns only once its workers' runs have: the job may be gone
/// right after.
#[test]
fn run_waits_for_its_workers() {
    let job = Slow {
        started: AtomicUsize::new(0),
        inside: AtomicUsize::new(0),
    };
    bindings::gc_platform().run(&job);
    assert_eq!(
        job.inside.load(Ordering::SeqCst),
        0,
        "a worker is still in the job"
    );
}

#[test]
#[should_panic = "a worker's panic"]
fn a_workers_panic_reaches_the_caller() {
    let mut job = counting(1);
    job.panic_on_worker = true;
    bindings::gc_platform().run(&job);
}

/// A posted job: each run spins until it should yield; it wants `want`
/// workers.
struct Spinning {
    want: usize,
    runs: AtomicUsize,
    inside: AtomicUsize,
    panic: bool,
    stop: AtomicBool,
}

impl Job for Spinning {
    #[expect(clippy::disallowed_methods, reason = "tests' counters")]
    fn run(&self, delegate: &dyn JobDelegate) {
        assert!(!delegate.is_joining_thread(), "a posted job has no caller");
        self.runs.fetch_add(1, Ordering::SeqCst);
        assert!(!self.panic, "a posted job's panic");
        self.inside.fetch_add(1, Ordering::SeqCst);
        while !delegate.should_yield() && !self.stop.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
        self.inside.fetch_sub(1, Ordering::SeqCst);
    }

    fn max_concurrency(&self, active: usize) -> usize {
        if self.stop.load(Ordering::SeqCst) {
            0
        } else {
            self.want.max(active)
        }
    }
}

fn spinning(want: usize, panic: bool) -> Arc<Spinning> {
    Arc::new(Spinning {
        want,
        runs: AtomicUsize::new(0),
        inside: AtomicUsize::new(0),
        panic,
        stop: AtomicBool::new(false),
    })
}

#[test]
fn a_posted_job_runs_on_workers_until_cancelled() {
    let platform = bindings::gc_platform();
    assert!(platform.posts_jobs());
    let job = spinning(1, false);
    let handle = platform
        .post_job(job.clone())
        .expect("V8's platform runs posted jobs");
    // The caller goes on while a worker runs it.
    wait_until("a run", || job.inside.load(Ordering::SeqCst) == 1);
    handle.cancel();
    assert_eq!(
        job.inside.load(Ordering::SeqCst),
        0,
        "cancel waits for its runs"
    );
    let runs = job.runs.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(job.runs.load(Ordering::SeqCst), runs, "no run after cancel");
}

#[test]
fn dropping_a_posted_jobs_handle_cancels_it() {
    let job = spinning(1, false);
    let handle = bindings::gc_platform().post_job(job.clone()).unwrap();
    wait_until("a run", || job.inside.load(Ordering::SeqCst) == 1);
    drop(handle);
    assert_eq!(job.inside.load(Ordering::SeqCst), 0);
}

#[test]
#[should_panic = "a posted job's panic"]
fn a_posted_jobs_panic_reaches_cancel() {
    let job = spinning(1, true);
    let handle = bindings::gc_platform().post_job(job.clone()).unwrap();
    wait_until("a run", || job.runs.load(Ordering::SeqCst) > 0);
    handle.cancel();
}
