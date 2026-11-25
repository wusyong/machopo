//! A thread's task queues (Chromium's `base::sequence_manager::
//! SequenceManager`, with its `ThreadController`'s wait): any thread posts,
//! the one thread that owns them takes, a task at a time, the earliest
//! posted among the queues it asks for. Each thread that runs tasks so has
//! one of its own (the renderer's main thread, each dedicated worker's).
//!
//! What a user says of its queues is a [`Spec`]: their kinds, its tasks,
//! the answers its thread's calls wait for ([`SequenceManager::sync_wait`]),
//! and what it keeps of its own under the same lock.
//!
//! The order: tasks are numbered as they are queued, and the earliest
//! numbered among the queues asked for is taken. A task posted for later
//! ([`SequenceManager::post_at`]) is queued once it is due, as the next
//! task is taken, not as it comes due (Chromium moves its ready delayed
//! tasks as it selects the next one): a task posted while another ran past
//! a timer's time comes before the timer.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// What a [`SequenceManager`]'s user says of it.
pub trait Spec {
    /// Which queue a task is in.
    type Queue: Copy + Eq + 'static;
    /// A task.
    type Task;
    /// An answer a call of the thread's waits for.
    type Reply;
    /// What the user keeps under the queues' lock.
    type Extra: Default;

    /// Every queue.
    const QUEUES: &'static [Self::Queue];

    /// `queue`'s place in [`QUEUES`](Self::QUEUES).
    fn index(queue: Self::Queue) -> usize;

    /// `task` was taken to run: what `extra` keeps of it may go.
    fn taken(extra: &mut Self::Extra, task: &Self::Task) {
        let _ = (extra, task);
    }
}

/// The thread that took a sequence manager's tasks has gone: nothing is
/// posted any more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Closed;

/// A task as queued, and its number.
struct Queued<T> {
    sequence: u64,
    task: T,
}

/// A task posted to come due at `at`, into `queue`; `order` the order it
/// was posted in, for those due at once.
struct Delayed<Q, T> {
    at: Instant,
    order: u64,
    queue: Q,
    task: T,
}

impl<Q, T> PartialEq for Delayed<Q, T> {
    fn eq(&self, other: &Self) -> bool {
        (self.at, self.order) == (other.at, other.order)
    }
}

impl<Q, T> Eq for Delayed<Q, T> {}

impl<Q, T> PartialOrd for Delayed<Q, T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<Q, T> Ord for Delayed<Q, T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.at, self.order).cmp(&(other.at, other.order))
    }
}

struct State<S: Spec> {
    queues: Vec<VecDeque<Queued<S::Task>>>,
    /// The tasks not due yet: each queued as it comes due, numbered then.
    delayed: BinaryHeap<Reverse<Delayed<S::Queue, S::Task>>>,
    /// The last delayed task's place among those due at once.
    delayed_order: u64,
    /// The last task's number: 0 is what was put back first
    /// ([`SequenceManager::put_back`]).
    next: u64,
    /// The thread has gone: nothing is posted any more.
    closed: bool,
    /// The answers to the thread's calls that wait
    /// ([`SequenceManager::sync_wait`]), by number: not tasks.
    replies: HashMap<u64, S::Reply>,
    extra: S::Extra,
}

impl<S: Spec> Default for State<S> {
    fn default() -> Self {
        Self {
            queues: S::QUEUES.iter().map(|_| VecDeque::new()).collect(),
            delayed: BinaryHeap::new(),
            delayed_order: 0,
            next: 0,
            closed: false,
            replies: HashMap::new(),
            extra: S::Extra::default(),
        }
    }
}

impl<S: Spec> State<S> {
    /// The next task's number.
    fn next_sequence(&mut self) -> u64 {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "one per task a thread is given: a u64 lasts centuries"
        )]
        {
            self.next += 1;
        }
        self.next
    }

    fn push(&mut self, queue: S::Queue, task: S::Task) {
        let sequence = self.next_sequence();
        if let Some(queue) = self.queues.get_mut(S::index(queue)) {
            queue.push_back(Queued { sequence, task });
        }
    }

    /// `task` into `queue` as `at` comes.
    fn push_delayed(&mut self, at: Instant, queue: S::Queue, task: S::Task) {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "one per delayed task a thread is given: a u64 lasts centuries"
        )]
        {
            self.delayed_order += 1;
        }
        let order = self.delayed_order;
        self.delayed.push(Reverse(Delayed {
            at,
            order,
            queue,
            task,
        }));
    }

    /// `task` into `queue`, now or as `at` comes.
    fn push_at(&mut self, queue: S::Queue, task: S::Task, at: Option<Instant>) {
        match at {
            Some(at) if at > Instant::now() => self.push_delayed(at, queue, task),
            _ => self.push(queue, task),
        }
    }

    /// The tasks due by now, queued in their turn: after what was posted
    /// before they were, the earliest due first.
    fn queue_due(&mut self) {
        let now = Instant::now();
        while self
            .delayed
            .peek()
            .is_some_and(|Reverse(delayed)| delayed.at <= now)
        {
            let Some(Reverse(delayed)) = self.delayed.pop() else {
                break;
            };
            self.push(delayed.queue, delayed.task);
        }
    }

    /// The queue among those `wanted` whose first task was posted earliest.
    fn earliest(&self, wanted: &impl Fn(S::Queue) -> bool) -> Option<S::Queue> {
        S::QUEUES
            .iter()
            .copied()
            .filter(|&queue| wanted(queue))
            .filter_map(|queue| Some((self.queues.get(S::index(queue))?.front()?.sequence, queue)))
            .min_by_key(|&(sequence, _)| sequence)
            .map(|(_, queue)| queue)
    }

    /// `queue`'s first task, taken.
    fn take(&mut self, queue: S::Queue) -> Option<S::Task> {
        let task = self.queues.get_mut(S::index(queue))?.pop_front()?.task;
        S::taken(&mut self.extra, &task);
        Some(task)
    }

    /// When the first of the tasks not due yet of a queue `wanted` is.
    fn next_delayed(&self, wanted: &impl Fn(S::Queue) -> bool) -> Option<Instant> {
        self.delayed
            .iter()
            .filter(|Reverse(delayed)| wanted(delayed.queue))
            .map(|Reverse(delayed)| delayed.at)
            .min()
    }
}

/// A thread's task queues: one lock and one condition for them all.
pub struct SequenceManager<S: Spec> {
    state: Mutex<State<S>>,
    ready: Condvar,
}

impl<S: Spec> Default for SequenceManager<S> {
    fn default() -> Self {
        Self {
            state: Mutex::new(State::default()),
            ready: Condvar::new(),
        }
    }
}

impl<S: Spec> SequenceManager<S> {
    fn lock(&self) -> MutexGuard<'_, State<S>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Posts `task` to `queue`, after every task posted before.
    pub fn post(&self, queue: S::Queue, task: S::Task) -> Result<(), Closed> {
        {
            let mut state = self.lock();
            if state.closed {
                return Err(Closed);
            }
            state.push(queue, task);
        }
        self.ready.notify_one();
        Ok(())
    }

    /// Posts `task` to `queue` as `at` comes (now, if it has).
    pub fn post_at(&self, queue: S::Queue, task: S::Task, at: Instant) -> Result<(), Closed> {
        {
            let mut state = self.lock();
            if state.closed {
                return Err(Closed);
            }
            state.push_at(queue, task, Some(at));
        }
        self.ready.notify_one();
        Ok(())
    }

    /// Posts what `decide` says, deciding it with the user's state under
    /// the queues' lock: a task, its queue, and when (now, if none), or
    /// nothing. Nothing once the thread has gone.
    pub fn post_with(
        &self,
        decide: impl FnOnce(&mut S::Extra) -> Option<(S::Queue, S::Task, Option<Instant>)>,
    ) {
        {
            let mut state = self.lock();
            if state.closed {
                return;
            }
            let Some((queue, task, at)) = decide(&mut state.extra) else {
                return;
            };
            state.push_at(queue, task, at);
        }
        self.ready.notify_one();
    }

    /// Puts `tasks` first in `queue`, in order, before every task: what
    /// waited for something to end.
    pub fn put_back(&self, queue: S::Queue, tasks: Vec<S::Task>) {
        if tasks.is_empty() {
            return;
        }
        {
            let mut state = self.lock();
            if let Some(queued) = state.queues.get_mut(S::index(queue)) {
                for task in tasks.into_iter().rev() {
                    queued.push_front(Queued { sequence: 0, task });
                }
            }
        }
        self.ready.notify_one();
    }

    /// The tasks of `queue` that `take` says to, taken out in order; the
    /// others stay, in order.
    pub fn take_matching(
        &self,
        queue: S::Queue,
        mut take: impl FnMut(&S::Task) -> bool,
    ) -> Vec<S::Task> {
        let mut state = self.lock();
        let Some(queued) = state.queues.get_mut(S::index(queue)) else {
            return Vec::new();
        };
        let mut taken = Vec::new();
        let mut kept = VecDeque::new();
        for item in queued.drain(..) {
            if take(&item.task) {
                taken.push(item.task);
            } else {
                kept.push_back(item);
            }
        }
        *queued = kept;
        taken
    }

    /// Reads or changes the user's state under the queues' lock.
    pub fn with_extra<R>(&self, f: impl FnOnce(&mut S::Extra) -> R) -> R {
        let result = f(&mut self.lock().extra);
        // A call waiting may give up on what was changed.
        self.ready.notify_one();
        result
    }

    /// The earliest task of the queues `wanted`, waiting for one until
    /// `timeout` has passed (forever if none).
    pub fn next(
        &self,
        wanted: impl Fn(S::Queue) -> bool,
        timeout: Option<Duration>,
    ) -> Option<S::Task> {
        let deadline = timeout.and_then(|timeout| Instant::now().checked_add(timeout));
        let mut state = self.lock();
        loop {
            state.queue_due();
            if let Some(queue) = state.earliest(&wanted) {
                return state.take(queue);
            }
            if timeout.is_some() && deadline.is_some_and(|deadline| deadline <= Instant::now()) {
                return None;
            }
            // Woken for the next delayed task of the queues wanted too.
            let delayed = state.next_delayed(&wanted);
            let wake = match (deadline.filter(|_| timeout.is_some()), delayed) {
                (Some(deadline), Some(delayed)) => Some(deadline.min(delayed)),
                (deadline, delayed) => deadline.or(delayed),
            };
            state = match wake {
                Some(wake) => {
                    let left = wake.saturating_duration_since(Instant::now());
                    self.ready
                        .wait_timeout(state, left)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
                // None, or a wait too long to count: forever.
                None => self
                    .ready
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner),
            };
        }
    }

    /// The earliest task of the queues `wanted`, if there is one.
    pub fn try_next(&self, wanted: impl Fn(S::Queue) -> bool) -> Option<S::Task> {
        let mut state = self.lock();
        state.queue_due();
        let queue = state.earliest(&wanted)?;
        state.take(queue)
    }

    /// Which of the queues `wanted` the earliest task is in, waiting for
    /// one; the task left queued. Delayed tasks aren't looked at: only
    /// queues that have none are waited for so.
    pub fn wait_for(&self, wanted: impl Fn(S::Queue) -> bool) -> S::Queue {
        let mut state = self.lock();
        loop {
            if let Some(queue) = state.earliest(&wanted) {
                return queue;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Whether a task waits in the queues `wanted`.
    pub fn has_task(&self, wanted: impl Fn(S::Queue) -> bool) -> bool {
        let mut state = self.lock();
        state.queue_due();
        state.earliest(&wanted).is_some()
    }

    /// Waits for the answer numbered `id` (a mojo `[Sync]` call's), every
    /// task left queued, in order. None once the thread has gone, or as
    /// `give_up` says of the user's state: nothing will answer.
    pub fn sync_wait(&self, id: u64, give_up: impl Fn(&S::Extra) -> bool) -> Option<S::Reply> {
        let mut state = self.lock();
        loop {
            if let Some(reply) = state.replies.remove(&id) {
                return Some(reply);
            }
            if state.closed || give_up(&state.extra) {
                return None;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// The answer to the call numbered `id` ([`sync_wait`](Self::sync_wait)),
    /// for that call alone.
    pub fn post_reply(&self, id: u64, reply: S::Reply) -> Result<(), Closed> {
        {
            let mut state = self.lock();
            if state.closed {
                return Err(Closed);
            }
            state.replies.insert(id, reply);
        }
        self.ready.notify_one();
        Ok(())
    }

    /// The thread has gone: what is queued goes, and what is posted from
    /// now on fails.
    pub fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        for queue in &mut state.queues {
            queue.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Queue {
        A,
        B,
    }

    /// Two queues; it counts the tasks taken, and a call may give up.
    struct Test;

    #[derive(Default)]
    struct Extra {
        taken: Vec<u32>,
        give_up: bool,
    }

    impl Spec for Test {
        type Queue = Queue;
        type Task = u32;
        type Reply = &'static str;
        type Extra = Extra;

        const QUEUES: &'static [Queue] = &[Queue::A, Queue::B];

        fn index(queue: Queue) -> usize {
            queue as usize
        }

        fn taken(extra: &mut Extra, task: &u32) {
            extra.taken.push(*task);
        }
    }

    fn all(_: Queue) -> bool {
        true
    }

    fn drain(manager: &SequenceManager<Test>) -> Vec<u32> {
        std::iter::from_fn(|| manager.try_next(all)).collect()
    }

    /// The earliest posted of the queues asked for, whichever queue it is
    /// in; a queue not asked for keeps its tasks, in order.
    #[test]
    fn tasks_are_taken_in_the_order_posted_across_queues() {
        let manager = SequenceManager::<Test>::default();
        for (queue, task) in [(Queue::A, 1), (Queue::B, 2), (Queue::A, 3), (Queue::B, 4)] {
            manager.post(queue, task).unwrap();
        }
        assert_eq!(manager.try_next(|queue| queue == Queue::B), Some(2));
        assert_eq!(drain(&manager), [1, 3, 4]);
        assert_eq!(
            manager.with_extra(|extra| extra.taken.clone()),
            [2, 1, 3, 4]
        );
        assert!(!manager.has_task(all));
    }

    /// A delayed task takes its place as the next task is taken once it is
    /// due, not as it comes due: one posted after its time, before the
    /// take, comes first. Those due at once come in the order posted.
    #[test]
    fn a_delayed_task_takes_its_place_as_the_next_is_taken() {
        let manager = SequenceManager::<Test>::default();
        let at = Instant::now() + Duration::from_millis(20);
        manager.post_at(Queue::A, 1, at).unwrap();
        manager.post_at(Queue::A, 2, at).unwrap();
        assert!(!manager.has_task(all));
        std::thread::sleep(Duration::from_millis(30));
        manager.post(Queue::B, 3).unwrap();
        assert_eq!(drain(&manager), [3, 1, 2]);
        // One already due is posted at once.
        manager.post_at(Queue::A, 4, Instant::now()).unwrap();
        assert_eq!(drain(&manager), [4]);
    }

    /// A wait ends with a delayed task as it comes due, with one posted
    /// from another thread, or at its timeout.
    #[test]
    fn waits_end_as_a_task_comes_or_time_is_up() {
        let manager = Arc::new(SequenceManager::<Test>::default());
        let started = Instant::now();
        manager
            .post_at(Queue::A, 1, started + Duration::from_millis(30))
            .unwrap();
        // Woken as it comes due, not at the wait's end.
        assert_eq!(manager.next(all, Some(Duration::from_secs(1))), Some(1));
        let waited = started.elapsed();
        assert!(waited >= Duration::from_millis(30), "{waited:?}");
        assert!(waited < Duration::from_millis(500), "{waited:?}");
        let posting = manager.clone();
        let poster = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            posting.post(Queue::B, 2).unwrap();
        });
        assert_eq!(manager.next(all, Some(Duration::from_secs(5))), Some(2));
        poster.join().unwrap();
        assert_eq!(manager.next(all, Some(Duration::from_millis(10))), None);
        // A delayed task of a queue not waited for doesn't end the wait.
        manager
            .post_at(Queue::A, 3, Instant::now() + Duration::from_millis(5))
            .unwrap();
        let waited = Instant::now();
        assert_eq!(
            manager.next(|queue| queue == Queue::B, Some(Duration::from_millis(30))),
            None
        );
        assert!(waited.elapsed() >= Duration::from_millis(30));
        assert_eq!(manager.try_next(all), Some(3));
    }

    /// A call that waits takes its answer alone, the tasks staying queued;
    /// it gives up as the user's state says, or as the thread goes.
    #[test]
    fn a_call_takes_its_answer_alone_or_gives_up() {
        let manager = Arc::new(SequenceManager::<Test>::default());
        manager.post(Queue::A, 1).unwrap();
        let answering = manager.clone();
        let answer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            answering.post_reply(8, "other").unwrap();
            answering.post_reply(7, "yes").unwrap();
        });
        assert_eq!(manager.sync_wait(7, |extra| extra.give_up), Some("yes"));
        answer.join().unwrap();
        assert_eq!(drain(&manager), [1]);
        let giving_up = manager.clone();
        let giver = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            giving_up.with_extra(|extra| extra.give_up = true);
        });
        // On a thread of its own: one that doesn't give up fails the test
        // rather than hang it.
        let waiting = manager.clone();
        let (gave, given_up) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = gave.send(waiting.sync_wait(9, |extra| extra.give_up));
        });
        assert_eq!(given_up.recv_timeout(Duration::from_secs(5)), Ok(None));
        giver.join().unwrap();
        manager.with_extra(|extra| extra.give_up = false);
        manager.close();
        assert_eq!(manager.sync_wait(9, |extra| extra.give_up), None);
    }

    /// Tasks taken out of a queue come in order, the rest staying in
    /// order; tasks put back come before every other.
    #[test]
    fn tasks_are_taken_out_and_put_back_in_order() {
        let manager = SequenceManager::<Test>::default();
        for task in [1, 2, 3, 4] {
            manager.post(Queue::A, task).unwrap();
        }
        manager.post(Queue::B, 5).unwrap();
        assert_eq!(
            manager.take_matching(Queue::A, |&task| task % 2 == 0),
            [2, 4]
        );
        manager.put_back(Queue::B, vec![6, 7]);
        assert_eq!(drain(&manager), [6, 7, 1, 3, 5]);
    }

    /// What is posted is decided with the user's state under the lock: a
    /// task now, one for later, or none.
    #[test]
    fn a_post_is_decided_under_the_lock() {
        let manager = SequenceManager::<Test>::default();
        manager.post_with(|extra| {
            assert!(extra.taken.is_empty());
            Some((Queue::A, 1, None))
        });
        manager.post_with(|_| None);
        manager.post_with(|_| {
            Some((
                Queue::B,
                2,
                Some(Instant::now() + Duration::from_millis(10)),
            ))
        });
        assert_eq!(drain(&manager), [1]);
        assert_eq!(manager.next(all, Some(Duration::from_secs(1))), Some(2));
    }

    /// Once the thread has gone, what was queued goes and nothing is
    /// posted.
    #[test]
    fn a_closed_manager_takes_nothing() {
        let manager = SequenceManager::<Test>::default();
        manager.post(Queue::A, 1).unwrap();
        manager.close();
        assert!(!manager.has_task(all));
        assert_eq!(manager.post(Queue::A, 2), Err(Closed));
        assert_eq!(manager.post_at(Queue::A, 3, Instant::now()), Err(Closed));
        assert_eq!(manager.post_reply(1, "late"), Err(Closed));
        manager.post_with(|_| Some((Queue::A, 4, None)));
        assert!(!manager.has_task(all));
    }
}
