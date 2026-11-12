//! The DOM's strings (Blink's `String` and `AtomicString`): immutable,
//! shared by a reference count, outside the GC's heap, so that an engine
//! can hold one past an operation. The count isn't atomic (Blink's
//! `StringImpl`'s isn't either): a string is its thread's, not `Send`, so
//! the compiler keeps every clone and drop on it. Where one ends is on it
//! too: the heap objects that hold strings finalize on the heap's thread
//! (`elise::LocalExternal`, which a `Send` value's `External` doesn't, its
//! finalizer on the sweep's worker), and an engine copies a string into its
//! own rather than keeping this one's buffer (V8 frees an external string's
//! buffer on its GC's workers; and lending was slower: a new string read
//! from JS 22% slower than copying, even of 100 bytes).
//!
//! The DOM's strings are 16-bit code units (WebIDL's `DOMString`); these
//! keep them as UTF-8, what machopo's parser, style, text layout and wasm
//! guests read, and give them out as `Cow<str>`. A string from JS may hold
//! a lone surrogate, which UTF-8 can't: such strings will be kept as WTF-8
//! (UTF-8 that may encode a lone surrogate) once JS can write them, and only
//! those would give out a copy with U+FFFD for each. A string knows whether
//! it is ASCII, whose bytes are its Latin-1 and whose UTF-16 offsets are its
//! byte offsets.
//!
//! Attribute values are interned (`Values`, Blink's `AtomicString` table):
//! equal values are one string, which an engine's cache of the strings it
//! made of them finds by its address.

use std::borrow::{Borrow, Cow};
use std::cell::RefCell;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use rustc_hash::{FxBuildHasher, FxHashSet};

/// An immutable string of the DOM, shared: cloning it counts a reference,
/// not atomically. Not `Send`, checked as the crate builds (below).
#[derive(Clone)]
pub struct DomString {
    text: Rc<str>,
    /// What it knows of its text (`ASCII`). A whole word, not a `bool`: a
    /// string moves as three words, and a byte written then the word
    /// around it read (a result handed on) can't forward from the store,
    /// which waits for the cache (an `Option<DomString>` returned by a
    /// member, 6 such conflicts a `getAttribute` from JS).
    flags: usize,
}

/// `DomString` isn't `Send`, checked as the crate builds (`static_assertions`'
/// `assert_not_impl_any!`): were it, `some_item` would be both impls' and
/// the call below ambiguous. Its count isn't atomic, and where it ends
/// relies on it (the heap objects that hold strings finalize on the heap's
/// thread; an engine copies one rather than keeping its buffer).
trait AmbiguousIfSend<A> {
    fn some_item() {}
}
impl<T: ?Sized> AmbiguousIfSend<()> for T {}
struct IsSend;
impl<T: ?Sized + Send> AmbiguousIfSend<IsSend> for T {}
const _: fn() = || {
    let _ = <DomString as AmbiguousIfSend<_>>::some_item;
};

/// `DomString::flags`: its text is ASCII.
const ASCII: usize = 1;

impl DomString {
    pub fn new(text: &str) -> Self {
        Self {
            text: Rc::from(text),
            flags: if text.is_ascii() { ASCII } else { 0 },
        }
    }

    /// Its text: borrowed, unless it holds a lone surrogate (none can yet).
    pub fn to_str(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.text)
    }

    /// Whether it is ASCII: its bytes are its Latin-1 and its UTF-16 offsets
    /// are its byte offsets.
    pub fn is_ascii(&self) -> bool {
        self.flags & ASCII != 0
    }

    /// Its length in UTF-16 code units (the DOM's `length`): its length in
    /// bytes if it is ASCII, else counted.
    pub fn utf16_len(&self) -> usize {
        if self.is_ascii() {
            return self.text.len();
        }
        // At most its length in bytes: no overflow.
        self.text
            .chars()
            .fold(0usize, |len, c| len.saturating_add(c.len_utf16()))
    }

    /// Its `count` UTF-16 code units from `offset` (fewer at its end; none
    /// past it): half a surrogate pair at an edge becomes U+FFFD (the
    /// DOM's strings keep no lone surrogates).
    pub fn utf16_substring(&self, offset: usize, count: usize) -> String {
        if self.is_ascii() {
            let start = offset.min(self.text.len());
            let end = start.saturating_add(count).min(self.text.len());
            return self.text[start..end].to_owned();
        }
        let units: Vec<u16> = self.text.encode_utf16().collect();
        let start = offset.min(units.len());
        let end = start.saturating_add(count).min(units.len());
        String::from_utf16_lossy(&units[start..end])
    }

    /// It with `count` UTF-16 code units from `offset` (fewer at its end)
    /// replaced by `data`, as `utf16_substring` cuts it.
    pub fn utf16_splice(&self, offset: usize, count: usize, data: &str) -> String {
        if self.is_ascii() {
            let start = offset.min(self.text.len());
            let end = start.saturating_add(count).min(self.text.len());
            return [&self.text[..start], data, &self.text[end..]].concat();
        }
        let units: Vec<u16> = self.text.encode_utf16().collect();
        let start = offset.min(units.len());
        let end = start.saturating_add(count).min(units.len());
        let mut spliced = String::from_utf16_lossy(&units[..start]);
        spliced.push_str(data);
        spliced.push_str(&String::from_utf16_lossy(&units[end..]));
        spliced
    }

    /// Its length in bytes.
    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Whether `a` and `b` are the same string, not only equal (an
    /// interned value is the same string as every equal one).
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Rc::ptr_eq(&a.text, &b.text)
    }

    /// Its address, the same for every clone of it and no other string's
    /// while one lives: what an engine keeps the strings it made of it by
    /// (Blink's `StringCache`, by `StringImpl*`).
    pub fn addr(&self) -> usize {
        Rc::as_ptr(&self.text).cast::<u8>().addr()
    }

    /// How many hold its string (tests: what an operation clones).
    #[doc(hidden)]
    pub fn reference_count(&self) -> usize {
        Rc::strong_count(&self.text)
    }

    /// Whether only `self` holds its string (an interned value only the
    /// table holds; a result only the call that computed it holds).
    pub fn is_unique(&self) -> bool {
        Rc::strong_count(&self.text) == 1
    }
}

impl From<&str> for DomString {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<String> for DomString {
    fn from(text: String) -> Self {
        Self::new(&text)
    }
}

impl PartialEq for DomString {
    fn eq(&self, other: &Self) -> bool {
        Self::ptr_eq(self, other) || self.text == other.text
    }
}

impl Eq for DomString {}

impl PartialEq<str> for DomString {
    fn eq(&self, other: &str) -> bool {
        &*self.text == other
    }
}

impl PartialEq<&str> for DomString {
    fn eq(&self, other: &&str) -> bool {
        &*self.text == *other
    }
}

impl Hash for DomString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.hash(state);
    }
}

impl Borrow<str> for DomString {
    fn borrow(&self) -> &str {
        &self.text
    }
}

impl fmt::Debug for DomString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.text, f)
    }
}

impl fmt::Display for DomString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// The interned attribute values, the thread's (Blink's `AtomicStringTable`):
/// the same value, the same string. A value only the table holds
/// is dropped as the table grows past twice what was left the last time it
/// looked, so that looking costs a constant per value interned: the values
/// die in the GC's finalizers, and a string doesn't know it is interned to
/// tell the table (Blink's `AtomicString` does, a flag on its `StringImpl`).
struct Values {
    strings: FxHashSet<DomString>,
    /// What the table holds at most before it drops the values only it
    /// holds.
    limit: usize,
}

/// The fewest values the table drops nothing below.
const MIN_LIMIT: usize = 64;

thread_local! {
    /// In a `RefCell`: a thread's, reached from the DOM and the parser with
    /// nothing to borrow it from; borrowed once an intern, which calls out
    /// to nothing.
    static VALUES: RefCell<Values> = const {
        RefCell::new(Values {
            strings: FxHashSet::with_hasher(FxBuildHasher),
            limit: MIN_LIMIT,
        })
    };
}

/// The thread's table's string of the value `text`, if it has one: what
/// no attribute's value is otherwise (they are the table's).
pub fn find_value(text: &str) -> Option<DomString> {
    VALUES.with_borrow(|values| values.strings.get(text).cloned())
}

/// The string of the value `text`: the thread's table's if it has one, else
/// a new one it keeps.
pub fn intern_value(text: &str) -> DomString {
    VALUES.with_borrow_mut(|values| {
        if let Some(string) = values.strings.get(text) {
            return string.clone();
        }
        if values.strings.len() >= values.limit {
            values.strings.retain(|string| !string.is_unique());
            values.limit = values.strings.len().saturating_mul(2).max(MIN_LIMIT);
        }
        let string = DomString::new(text);
        values.strings.insert(string.clone());
        string
    })
}

/// How many values the thread's table holds.
pub(crate) fn interned_values() -> usize {
    VALUES.with_borrow(|values| values.strings.len())
}
