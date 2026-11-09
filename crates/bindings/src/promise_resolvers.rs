//! A realm's promises that the embedder settles later, by number (the
//! table of Blink's `ScriptPromiseResolver`s, which each keep a resolver
//! alive while the work it waits for is on its way): a DOM member's that
//! returns one, and `import()`'s. Held strongly in the context's slot,
//! until settled and handed out; a kept one (a promise attribute's, Blink's
//! `ScriptPromiseProperty`: `Animation.ready`) until released, the same
//! object each time. The realm's [`PromiseResolvers`] drops them when it
//! goes, so none outlives the realm, and one of a realm that went never
//! settles (as HTML says).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use base::SequenceNumber;

struct Entry {
    resolver: v8::Global<v8::PromiseResolver>,
    /// Its promise was given to script ([`hand_out`]).
    handed_out: bool,
    /// It was resolved or rejected ([`take_resolver`]).
    settled: bool,
    /// It stays until [`release`]d, settled and handed out or not.
    kept: bool,
}

/// The context's slot.
struct Table {
    entries: HashMap<u64, Entry>,
    /// From 1.
    next: SequenceNumber<u64>,
}

/// A realm's table of promises: made with the realm, dropped with it.
pub struct PromiseResolvers {
    table: Rc<RefCell<Table>>,
}

impl PromiseResolvers {
    /// The (empty) table of the context `scope` is in.
    pub fn new(scope: &mut v8::PinScope<'_, '_>) -> Self {
        let table = Rc::new(RefCell::new(Table {
            entries: HashMap::new(),
            next: SequenceNumber::starting_at(1),
        }));
        let previous = scope.get_current_context().set_slot(table.clone());
        assert!(previous.is_none(), "a context has one table of promises");
        Self { table }
    }
}

impl Drop for PromiseResolvers {
    /// The promises still waiting go with the realm: nothing in the
    /// context's slot keeps a V8 value alive once the realm has gone.
    fn drop(&mut self) {
        self.table.borrow_mut().entries.clear();
    }
}

fn table(scope: &v8::PinScope<'_, '_>) -> Option<Rc<RefCell<Table>>> {
    scope.get_current_context().get_slot::<RefCell<Table>>()
}

/// WebIDL's "a new promise" in the current context, kept under a number
/// until it is settled and handed out. `None` if the context has no table
/// ([`PromiseResolvers::new`]), or no promise could be made (an exception
/// is then pending).
pub fn create_promise<'s>(
    scope: &mut v8::PinScope<'s, '_>,
) -> Option<(u64, v8::Local<'s, v8::PromiseResolver>)> {
    make_promise(scope, false)
}

/// As [`create_promise`], kept until [`release`]d: handed out as the same
/// object as often as asked, before and after it is settled. Its rejection
/// is never an unhandled one: an attribute's promise rejects because what
/// it waits for was canceled (web-animations 1 sets `[[PromiseIsHandled]]`
/// as it rejects `ready` and `finished`; Blink's `Animation` marks them
/// handled when it makes them).
pub fn create_kept_promise<'s>(
    scope: &mut v8::PinScope<'s, '_>,
) -> Option<(u64, v8::Local<'s, v8::PromiseResolver>)> {
    let (number, resolver) = make_promise(scope, true)?;
    resolver.get_promise(scope).mark_as_handled();
    Some((number, resolver))
}

/// Forgets the kept promise numbered `number` of the current context.
pub fn release(scope: &v8::PinScope<'_, '_>, number: u64) {
    if let Some(table) = table(scope) {
        table.borrow_mut().entries.remove(&number);
    }
}

fn make_promise<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    kept: bool,
) -> Option<(u64, v8::Local<'s, v8::PromiseResolver>)> {
    let table = table(scope)?;
    let resolver = v8::PromiseResolver::new(scope)?;
    let mut table = table.borrow_mut();
    let number = table.next.get_next();
    table.entries.insert(
        number,
        Entry {
            resolver: v8::Global::new(scope, resolver),
            handed_out: false,
            settled: false,
            kept,
        },
    );
    Some((number, resolver))
}

/// The promise numbered `number` of the current context, to give to
/// script (once): it is forgotten once also settled. `None` if there is
/// none (its realm is going).
pub fn hand_out<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    number: u64,
) -> Option<v8::Local<'s, v8::Promise>> {
    let table = table(scope)?;
    let mut table = table.borrow_mut();
    let entry = table.entries.get_mut(&number)?;
    entry.handed_out = true;
    let promise = v8::Local::new(scope, &entry.resolver).get_promise(scope);
    if entry.settled && !entry.kept {
        table.entries.remove(&number);
    }
    Some(promise)
}

/// Whether the promise numbered `number` of the current context waits to
/// be settled.
pub fn is_waiting(scope: &v8::PinScope<'_, '_>, number: u64) -> bool {
    table(scope).is_some_and(|table| {
        table
            .borrow()
            .entries
            .get(&number)
            .is_some_and(|entry| !entry.settled)
    })
}

/// The resolver of the promise numbered `number` of the current context,
/// to settle it (once): it is forgotten once also handed out. `None` if it
/// isn't waiting.
pub fn take_resolver<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    number: u64,
) -> Option<v8::Local<'s, v8::PromiseResolver>> {
    let table = table(scope)?;
    let mut table = table.borrow_mut();
    let entry = table.entries.get_mut(&number)?;
    if entry.settled {
        return None;
    }
    entry.settled = true;
    let resolver = v8::Local::new(scope, &entry.resolver);
    if entry.handed_out && !entry.kept {
        table.entries.remove(&number);
    }
    Some(resolver)
}
