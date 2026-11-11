//! The DOM's strings to and from V8 (Blink's `V8StringResource` and
//! `StringCache`).
//!
//! To V8: a copy, V8's own string. Not an external string of the DOM's
//! buffer (Blink's way): the DOM's strings are their thread's, and V8
//! frees an external string's buffer on its GC's workers; and copying was
//! faster (a new value read with `getAttribute` 22% faster than lent, even
//! of 100 bytes: an external string costs V8 an object, an entry in its
//! table and a call back as it collects it). A string's V8 string is
//! cached (`StringCache`) by the string's address: an interned
//! attribute value is one string on every element, so its V8 string is made
//! once. An entry keeps its string, so that its address is no other's
//! while it lives, and a weak handle of the V8 string; when V8 collects
//! that, its finalizer marks the entry dead, and the cache drops dead
//! entries as it is next used (not in the finalizer, which mustn't drop
//! its own handle).
//!
//! From V8: an argument converted by WebIDL's `ToString`, which may run
//! script, unless it is a string already (a member's: `arg_string`); V8's one-byte strings by their bytes when ASCII (rusty_v8's
//! `ValueView`). A lone surrogate becomes U+FFFD, until the DOM's strings
//! keep them (WTF-8). A member's string argument (`ArgString`) is read in
//! the DOM's operation: an ASCII one in place, V8's own characters
//! (`bindings::LentString`), so that a name or a value the DOM holds
//! already is found with no copy, and compared with bytes V8 wrote long
//! ago, not ones copied just now (which a wide read can't forward from).

use std::cell::RefCell;
use std::rc::Rc;

use dom::DomString;
use rustc_hash::FxHashMap;

/// A string's V8 string, made now, a copy; none if it is longer than V8's
/// strings may be.
pub fn new_string<'s>(
    scope: &v8::PinScope<'s, '_, ()>,
    string: &DomString,
) -> Option<v8::Local<'s, v8::String>> {
    v8::String::new_from_utf8(scope, string.to_str().as_bytes(), v8::NewStringType::Normal)
}

/// The V8 strings made of the DOM's strings, by their addresses (Blink's
/// `StringCache`, one an isolate). In the isolate's state, it is read in an
/// operation (`find`, its handle opened through `bindings::NoGc`), and an
/// entry made out of one, the state let go while V8 makes the string
/// (`make`, then `insert`): V8's GC reaches the state.
#[derive(Default)]
pub struct StringCache {
    entries: FxHashMap<usize, Entry>,
    /// The entries whose V8 strings V8 collected, by their keys: their
    /// finalizers' (which hold this weakly), not the state's.
    dead: Rc<RefCell<Vec<usize>>>,
}

struct Entry {
    /// Keeps the key: no other string has this address while it lives.
    _string: DomString,
    v8: v8::Weak<v8::String>,
}

impl StringCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// `string`'s V8 string: the cached one if V8 still has it, else made
    /// now (`make`) and cached; none if it is too long for V8. For a cache
    /// V8's GC doesn't reach (not the state's: `make`, then `insert`).
    pub fn get<'s>(
        &mut self,
        scope: &mut v8::PinScope<'s, '_, ()>,
        string: &DomString,
    ) -> Option<v8::Local<'s, v8::String>> {
        if let Some(local) = self.peek(scope, string) {
            return Some(local);
        }
        let (local, weak) = Self::make(scope, self.dead(), string)?;
        self.insert(string, weak);
        Some(local)
    }

    /// `string`'s cached V8 string, if V8 still has it: no allocation.
    pub fn peek<'s>(
        &self,
        scope: &v8::PinScope<'s, '_, ()>,
        string: &DomString,
    ) -> Option<v8::Local<'s, v8::String>> {
        self.find(string).and_then(|weak| weak.to_local(scope))
    }

    /// `string`'s entry's handle, if it has one (V8 may have collected its
    /// string): read in an operation.
    #[inline]
    pub fn find(&self, string: &DomString) -> Option<&v8::Weak<v8::String>> {
        self.entries.get(&string.addr()).map(|entry| &entry.v8)
    }

    /// What a new entry's finalizer marks it dead in (`make`).
    pub fn dead(&self) -> std::rc::Weak<RefCell<Vec<usize>>> {
        Rc::downgrade(&self.dead)
    }

    /// `string`'s V8 string made now (`new_string`), and its weak handle,
    /// whose finalizer marks its entry dead in `dead` (`dead`); none if it
    /// is too long for V8.
    pub fn make<'s>(
        scope: &mut v8::PinScope<'s, '_, ()>,
        dead: std::rc::Weak<RefCell<Vec<usize>>>,
        string: &DomString,
    ) -> Option<(v8::Local<'s, v8::String>, v8::Weak<v8::String>)> {
        let local = new_string(scope, string)?;
        let key = string.addr();
        let weak = v8::Weak::with_finalizer(
            scope,
            local,
            Box::new(move |_| {
                if let Some(dead) = dead.upgrade() {
                    dead.borrow_mut().push(key);
                }
            }),
        );
        Some((local, weak))
    }

    /// `string`'s entry, its handle `weak` (`make`'s), the dead ones
    /// dropped first. An entry whose V8 string died before its finalizer
    /// ran is replaced (its handle dropped here, not in its finalizer).
    pub fn insert(&mut self, string: &DomString, weak: v8::Weak<v8::String>) {
        self.prune();
        self.entries.insert(
            string.addr(),
            Entry {
                _string: string.clone(),
                v8: weak,
            },
        );
    }

    /// Drops the entries whose V8 strings V8 collected: not one made again
    /// since, whose handle has its string.
    fn prune(&mut self) {
        let dead = std::mem::take(&mut *self.dead.borrow_mut());
        for key in dead {
            if self
                .entries
                .get(&key)
                .is_some_and(|entry| entry.v8.is_empty())
            {
                self.entries.remove(&key);
            }
        }
    }

    /// How many entries it holds (dead ones too, until it is next used).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Argument `value` as a `DOMString`: WebIDL's `ToString` (which may run
/// script: none if it threw).
pub fn string_arg(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<'_, v8::Value>,
) -> Option<String> {
    let string = value.to_string(scope)?;
    Some(string.to_rust_string_lossy(scope))
}

/// A member's string argument (`dom_api::Front::Str`): converted by
/// `ToString` before the operation, its text read in the operation
/// (`text`): an ASCII flat one-byte string's in place, V8's GCs aborting
/// while it is lent (the call drops its arguments once the operation is
/// over, before V8 allocates again); another's copied before the
/// operation, as flattening a cons string allocates.
pub struct ArgString<'s> {
    string: bindings::LentString<'s>,
    copied: Option<String>,
}

/// Argument `value` as a member's `DOMString`: itself if it is a string
/// (most are; told inline, as Blink's bindings tell with V8's
/// `Value::IsString`), else WebIDL's `ToString` (which may run script:
/// none if it threw), a call into V8 and its context found first.
pub fn arg_string<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
) -> Option<ArgString<'s>> {
    let string = match v8::machopo::as_string(value) {
        Some(string) => string,
        None => {
            if let Some(state) = bindings::isolate_state_mut::<crate::state::State>(scope) {
                state.string_conversions = state.string_conversions.wrapping_add(1);
            }
            value.to_string(scope)?
        }
    };
    let string = bindings::LentString::new(string);
    let copied = (!string.lends(scope)).then(|| string.string().to_rust_string_lossy(scope));
    Some(ArgString { string, copied })
}

impl<'s> ArgString<'s> {
    /// `string`, which lends its text in place (`LentString::lends`).
    pub(crate) fn lent(string: bindings::LentString<'s>) -> Self {
        Self {
            string,
            copied: None,
        }
    }

    /// `text`, for a holder of the state borrowed from the callback's
    /// info.
    pub(crate) fn text_lent(&self, no_gc: &mut bindings::InfoNoGc<'_, '_>) -> &str {
        match &self.copied {
            Some(copied) => copied,
            // A flat string stays flat, and stays V8's that lends it.
            None => no_gc
                .ascii(&self.string)
                .expect("a string that lent its text lends it again"),
        }
    }

    /// `f` of its text if it lends it in place (an ASCII one-byte one),
    /// lent for the call alone (`bindings::LentString::peek_ascii`): before
    /// what may collect.
    pub fn peek_ascii<R>(&self, isolate: &v8::Isolate, f: impl FnOnce(&str) -> R) -> Option<R> {
        self.string.peek_ascii(isolate, f)
    }

    /// Its text, in the operation.
    pub fn text(&self, no_gc: &mut bindings::NoGc<'_, '_, '_>) -> &str {
        match &self.copied {
            Some(copied) => copied,
            // A flat string stays flat, and stays V8's that lends it.
            None => no_gc
                .ascii(&self.string)
                .expect("a string that lent its text lends it again"),
        }
    }
}

/// Throws a `RangeError` of `message`.
pub fn throw_range_error(scope: &mut v8::PinScope<'_, '_>, message: &str) {
    let Some(message) = v8::String::new(scope, message) else {
        return;
    };
    let error = v8::Exception::range_error(scope, message);
    scope.throw_exception(error);
}

/// Throws a `TypeError` of `message`.
pub fn throw_type_error(scope: &mut v8::PinScope<'_, '_>, message: &str) {
    let Some(message) = v8::String::new(scope, message) else {
        return;
    };
    let error = v8::Exception::type_error(scope, message);
    scope.throw_exception(error);
}

#[cfg(test)]
mod tests {
    use dom::DomString;

    use super::StringCache;

    /// A key marked dead whose entry was made again since (its V8 string
    /// collected, the string asked for before the old finalizer ran): the
    /// entry stays at the next insert's pruning, its V8 string alive.
    #[test]
    fn a_dead_key_keeps_an_entry_made_again() {
        bindings::initialize();
        let mut isolate = v8::Isolate::new(v8::CreateParams::default());
        v8::scope!(let scope, &mut isolate);
        let mut cache = StringCache::new();
        let string = DomString::new("again");
        let references = || string.reference_count();
        let _made = cache.get(scope, &string).unwrap();
        // The string's and the entry's.
        assert_eq!(references(), 2);
        cache.dead.borrow_mut().push(string.addr());
        cache.get(scope, &DomString::new("other")).unwrap();
        assert!(cache.dead.borrow().is_empty(), "pruned");
        assert_eq!(cache.len(), 2, "the entry stayed");
        cache.get(scope, &string).unwrap();
        // Found, not made again: no other entry's reference.
        assert_eq!(references(), 2);
        assert_eq!(cache.len(), 2);
    }
}
