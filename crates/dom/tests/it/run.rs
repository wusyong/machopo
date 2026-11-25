//! The DOM's tasks run as a host's scheduler runs them: each numbered
//! task's token goes into the test thread's `TaskTokens` (each test is a
//! thread of its own), and those due are taken by their numbers
//! (`Cx::task_numbered` and the numbered takes), in the scheduler's order.

use dom::{
    Cx, Dom, ElementTaskKind, EventTargetImpl, ExecutionContext, NodeRef, TakenMessage, TaskQueue,
    TaskTokens, Timer,
};
use elise::Gc;

thread_local! {
    static TOKENS: TaskTokens = TaskTokens::new();
}

/// A DOM whose tasks' tokens go to this thread's.
pub fn dom() -> Dom {
    let mut dom = Dom::new();
    tokens(|tokens| dom.set_task_waker(tokens.waker()));
    dom
}

/// This thread's tokens.
pub fn tokens<R>(f: impl FnOnce(&TaskTokens) -> R) -> R {
    TOKENS.with(f)
}

/// A task taken by its number.
pub enum Taken<'gc> {
    Timer(Gc<'gc, Timer>),
    NativeTimer,
    IdleTimeout,
    Element(
        Option<NodeRef<'gc>>,
        ElementTaskKind,
        Option<Gc<'gc, EventTargetImpl>>,
    ),
    Message(TakenMessage<'gc>),
}

impl Taken<'_> {
    pub fn queue(&self) -> TaskQueue {
        match self {
            Self::Timer(_) => TaskQueue::Timer,
            Self::NativeTimer => TaskQueue::NativeTimer,
            Self::IdleTimeout => TaskQueue::IdleTimeout,
            Self::Element(..) => TaskQueue::Element,
            Self::Message(_) => TaskQueue::Message,
        }
    }
}

/// The tasks of `contexts` whose tokens are due now, each taken by its
/// number, in the tokens' order (one queued as they are waits for the next
/// call); a token whose task is no more (cancelled, taken) takes nothing.
pub fn take_due<'gc>(cx: &Cx<'_, 'gc>, contexts: &[Gc<'gc, ExecutionContext>]) -> Vec<Taken<'gc>> {
    take_due_in(cx, contexts)
        .into_iter()
        .map(|(_, taken)| taken)
        .collect()
}

/// `take_due`'s, each with the index of its context in `contexts`.
pub fn take_due_in<'gc>(
    cx: &Cx<'_, 'gc>,
    contexts: &[Gc<'gc, ExecutionContext>],
) -> Vec<(usize, Taken<'gc>)> {
    tokens(TaskTokens::take_due)
        .into_iter()
        .filter_map(|number| {
            let (index, queue) = contexts
                .iter()
                .enumerate()
                .find_map(|(index, &context)| Some((index, cx.task_numbered(context, number)?)))?;
            Some((index, take(cx, contexts[index], number, queue)?))
        })
        .collect()
}

fn take<'gc>(
    cx: &Cx<'_, 'gc>,
    context: Gc<'gc, ExecutionContext>,
    number: u64,
    queue: TaskQueue,
) -> Option<Taken<'gc>> {
    Some(match queue {
        TaskQueue::Timer => {
            let now = cx.now_in(Some(context));
            Taken::Timer(cx.take_timer_numbered(context, number, now)?.0)
        }
        TaskQueue::NativeTimer => {
            cx.take_native_timer_numbered(context, number)?;
            Taken::NativeTimer
        }
        TaskQueue::IdleTimeout => {
            let window = cx.context_window(context)?;
            cx.take_idle_timeout_numbered(window, number)?;
            Taken::IdleTimeout
        }
        TaskQueue::Element => {
            let (element, kind, target) = cx.take_element_task_numbered(context, number)?;
            Taken::Element(element, kind, target)
        }
        TaskQueue::Message => Taken::Message(cx.take_posted_message_numbered(context, number)?),
    })
}

/// The messages of `contexts` due now, taken (`take_due`'s; those of other
/// kinds taken too, dropped).
pub fn take_messages<'gc>(
    cx: &Cx<'_, 'gc>,
    contexts: &[Gc<'gc, ExecutionContext>],
) -> Vec<TakenMessage<'gc>> {
    take_due(cx, contexts)
        .into_iter()
        .filter_map(|taken| match taken {
            Taken::Message(message) => Some(message),
            _ => None,
        })
        .collect()
}

/// The element tasks of `contexts` due now, taken (`take_due`'s).
pub fn take_element_tasks<'gc>(
    cx: &Cx<'_, 'gc>,
    contexts: &[Gc<'gc, ExecutionContext>],
) -> Vec<(
    Option<NodeRef<'gc>>,
    ElementTaskKind,
    Option<Gc<'gc, EventTargetImpl>>,
)> {
    take_due(cx, contexts)
        .into_iter()
        .filter_map(|taken| match taken {
            Taken::Element(element, kind, target) => Some((element, kind, target)),
            _ => None,
        })
        .collect()
}
