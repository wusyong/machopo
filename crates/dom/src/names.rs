//! The names: elements' and attributes' local names, prefixes and
//! namespaces, and events' types, interned, the same name the same number
//! (Blink's `QualifiedName` and event types hold `AtomicString`s). A node
//! or an event holds numbers: no allocation, no finalizer, and comparing
//! names compares numbers.
//!
//! One table a thread, as Blink's `AtomicStringTable`: the parser, the DOM
//! and style intern into it directly, with no DOM at hand, and a number
//! means one name on its thread, whichever DOM holds it. The names known at
//! compile time (`build.rs`: the DOM's own, the HTML, SVG and MathML
//! local names, and Blink's event types) have fixed numbers, constants
//! (`local::DIV`, Blink's `html_names::kDivTag`; `event::CLICK`, its
//! `event_type_names::kClick`), found by a perfect hash made at compile time
//! (`build.rs`) and read from an array by number: no table borrowed. Only the others go
//! through the thread's table.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::ops::Deref;
use std::sync::atomic::{AtomicU32, Ordering};

use rustc_hash::{FxBuildHasher, FxHashMap};

use crate::NameIds;
use crate::string::DomString;

/// A name's number in its thread's table: 0 is none (no namespace, no
/// prefix).
pub type NameId = u32;

/// No namespace or no prefix (the table's empty string).
pub const NONE: NameId = 0;

/// The bits a name's number takes at most: a listener keeps its flags in
/// the others of its type's word (2^27 names, past what any page makes).
pub const NAME_BITS: u32 = 27;

/// `name`'s entry in the event handlers' table (0: not a handler's
/// attribute): what an attribute's change reads, one load.
#[inline]
pub(crate) fn handler_entry(name: NameId) -> u16 {
    usize::try_from(name)
        .ok()
        .and_then(|index| HANDLER_ATTRIBUTES.get(index))
        .copied()
        .unwrap_or(0)
}

/// What an HTML element of the local name `local` is to forms
/// (`form::LISTED` and the others' bits; 0 for a name not fixed): one
/// load.
#[inline]
pub(crate) fn form_kind(local: NameId) -> u8 {
    usize::try_from(local)
        .ok()
        .and_then(|index| FORM_KINDS.get(index))
        .copied()
        .unwrap_or(0)
}

/// What an attribute is as an event handler's content attribute: the
/// event type its handler handles, and the sets it is in (HTML's
/// `GlobalEventHandlers`, its `WindowEventHandlers`, and the global ones
/// a `<body>` or `<frameset>` has for its window).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandlerAttribute {
    pub event_type: NameId,
    pub global: bool,
    pub window: bool,
    pub reflecting: bool,
}

/// `name`, a plain attribute's (no namespace, no prefix: its
/// `QualifiedName` is its number), as an event handler's content
/// attribute, if it is one: a read of a table made at compile time
/// (`build.rs`), as an attribute's every change asks.
pub fn handler_attribute(name: NameId) -> Option<HandlerAttribute> {
    let entry = handler_entry(name);
    (entry != 0).then(|| HandlerAttribute {
        event_type: NameId::from(entry & 0x1fff),
        global: entry & 1 << 13 != 0,
        window: entry & 1 << 14 != 0,
        reflecting: entry & 1 << 15 != 0,
    })
}

/// The namespaces with fixed numbers, as Blink's
/// `html_names::xhtmlNamespaceURI` and the others are constants.
pub const HTML_NAMESPACE: NameId = 1;
pub const SVG_NAMESPACE: NameId = 2;
pub const MATHML_NAMESPACE: NameId = 3;
pub const XLINK_NAMESPACE: NameId = 4;
pub const XML_NAMESPACE: NameId = 5;
pub const XMLNS_NAMESPACE: NameId = 6;

/// What `nodeName` gives a document and a text: fixed too, so that it is
/// one string a thread, not one a call.
pub(crate) const DOCUMENT_NAME: NameId = 7;
pub(crate) const TEXT_NAME: NameId = 8;

/// The attributes `id` and `className` reflect: their getters find the
/// attribute by number, with no look in the table (Blink's
/// `html_names::kIdAttr` and `kClassAttr`).
pub const ID: NameId = 9;
pub const CLASS: NameId = 10;

/// What `nodeName` gives a comment and a document fragment.
pub(crate) const COMMENT_NAME: NameId = 11;
pub(crate) const FRAGMENT_NAME: NameId = 12;

/// The local name an HTML element has its contents for (`<template>`):
/// making an element compares a number.
pub(crate) const TEMPLATE: NameId = 13;

/// An HTML element's local name with an interface of its own (Blink's
/// `html_names`): a wrapper's interface is a table read by number
/// (`fixed`).
pub const META: NameId = 14;

/// The attributes reflected by number, as `ID` and `CLASS` are (`name` is
/// `namedItem`'s too).
pub const NAME: NameId = 15;
pub const CONTENT: NameId = 16;

include!(concat!(env!("OUT_DIR"), "/fixed_names.rs"));
include!("name_hash.rs");

/// The threads' serials given so far (`thread_serial`).
static SERIALS: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static SERIAL: Cell<u32> = const { Cell::new(0) };
}

/// The thread's serial, from 1, given the first time asked for: what tells
/// its names' numbers from another thread's where both may read them (a
/// selector's `css::IdentAtom`, in a sheet two threads share).
///
/// # Panics
///
/// Once 2^32 - 1 threads asked.
pub fn thread_serial() -> u32 {
    SERIAL.with(|serial| match serial.get() {
        0 => {
            let given = SERIALS
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
                    last.checked_add(1)
                })
                .ok()
                .and_then(|last| last.checked_add(1))
                .expect("fewer than 2^32 threads");
            serial.set(given);
            given
        }
        given => given,
    })
}

/// The number `name` has fixed, if it has one: for a table worked out at
/// compile time (the element interfaces by local name).
#[expect(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "at < FIXED_COUNT, which build.rs keeps far below 2^32: it fits a NameId"
)]
pub const fn fixed(name: &str) -> Option<NameId> {
    let mut at = 0;
    while at < FIXED.len() {
        let fixed = FIXED[at].as_bytes();
        let name = name.as_bytes();
        if fixed.len() == name.len() {
            let mut i = 0;
            while i < name.len() && fixed[i] == name[i] {
                i += 1;
            }
            if i == name.len() {
                return Some(at as NameId);
            }
        }
        at += 1;
    }
    None
}

/// The names with no fixed number, the thread's (Blink's
/// `AtomicStringTable`). In a `RefCell`: a thread's, reached from the
/// parser, the DOM and style with nothing to borrow it from; no borrow
/// outlives a call here, and none calls out while it holds one, so none
/// overlaps. Only names with no fixed one borrow it.
struct Table {
    numbers: FxHashMap<DomString, NameId>,
    /// By number less `FIXED_COUNT`.
    strings: Vec<DomString>,
    /// The fixed names' strings, made the first time asked for one
    /// (`string`), by number.
    fixed_strings: Vec<Option<DomString>>,
    /// An element's `nodeName` that isn't its local name (a prefix, or
    /// uppercased in an HTML document), by its prefix, local name and
    /// whether uppercased: made once, a name of the table (Blink's
    /// `QualifiedName`, which keeps its uppercased local name).
    node_names: FxHashMap<(NameId, NameId, bool), NameId>,
    /// The qualified names a `QualifiedName` can't hold in its bits (a
    /// prefix, another namespace), by number less `TABLED`, and their
    /// numbers.
    qualified: Vec<NameIds>,
    qualified_numbers: FxHashMap<NameIds, u32>,
}

thread_local! {
    // `const`: no check for a first use on each access.
    static TABLE: RefCell<Table> = const {
        RefCell::new(Table {
            numbers: FxHashMap::with_hasher(FxBuildHasher),
            strings: Vec::new(),
            fixed_strings: Vec::new(),
            node_names: FxHashMap::with_hasher(FxBuildHasher),
            qualified: Vec::new(),
            qualified_numbers: FxHashMap::with_hasher(FxBuildHasher),
        })
    };
}

/// A dynamic name's index in `Table::strings`, if `number` isn't fixed.
fn dynamic_index(number: NameId) -> Option<usize> {
    usize::try_from(number).ok()?.checked_sub(FIXED_COUNT)
}

impl Table {
    fn find(&self, name: &str) -> Option<NameId> {
        self.numbers.get(name).copied()
    }

    fn add(&mut self, name: &str) -> NameId {
        let number = self
            .strings
            .len()
            .checked_add(FIXED_COUNT)
            .and_then(|number| NameId::try_from(number).ok())
            .filter(|&number| number < 1 << NAME_BITS)
            .expect("fewer than 2^27 names");
        let string = DomString::new(name);
        self.numbers.insert(string.clone(), number);
        self.strings.push(string);
        number
    }

    fn dynamic(&self, index: usize) -> &DomString {
        self.strings.get(index).expect("a name the table gave")
    }
}

/// `name`'s number, given it now if it has none.
pub fn intern(name: &str) -> NameId {
    if let Some(number) = find_fixed(name) {
        return number;
    }
    TABLE.with_borrow_mut(|table| match table.find(name) {
        Some(number) => number,
        None => table.add(name),
    })
}

/// `name`'s number, if it has one.
pub fn find(name: &str) -> Option<NameId> {
    find_fixed(name).or_else(|| TABLE.with_borrow(|table| table.find(name)))
}

/// `name`'s fixed number, if it has one. The empty string (no namespace,
/// no prefix: most looks) is `NONE` with no look in the map: comparing it
/// there calls `memcmp` with length 0 on the empty slice's dangling
/// address, which glibc's AVX-512 `memcmp` still loads from (masked), at
/// page 0, a fault-suppression assist of hundreds of cycles on AMD's Zen 4
/// and 5.
fn find_fixed(name: &str) -> Option<NameId> {
    if name.is_empty() {
        return Some(NONE);
    }
    let (bucket, f1, f2) = name_hash_parts(name_hash(name.as_bytes(), SEED));
    let number = SLOTS[name_hash_slot(f1, f2, DISPLACEMENTS[bucket], SLOT_MASK)];
    name_eq(FIXED[usize::from(number)].as_bytes(), name.as_bytes()).then_some(NameId::from(number))
}

/// Whether `a` and `b` are the same bytes, read whole as `name_hash` reads
/// them: a name's few bytes compared with no call to `bcmp` (a few dozen
/// instructions for 2 bytes).
#[inline]
fn name_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let (mut a, mut b) = (a, b);
    while let (Some((x, a_tail)), Some((y, b_tail))) =
        (a.split_first_chunk::<8>(), b.split_first_chunk::<8>())
    {
        if x != y {
            return false;
        }
        (a, b) = (a_tail, b_tail);
    }
    match (
        a.first_chunk::<4>(),
        b.first_chunk::<4>(),
        a.last_chunk::<4>(),
        b.last_chunk::<4>(),
    ) {
        (Some(a_first), Some(b_first), Some(a_last), Some(b_last)) => {
            a_first == b_first && a_last == b_last
        }
        _ => a.iter().zip(b).all(|(x, y)| x == y),
    }
}

/// The name numbered `number`: a fixed one's text, or a dynamic one's
/// string (a reference counted), so that no borrow of the table outlives
/// the call.
///
/// # Panics
///
/// If the thread's table gave no such number.
pub fn get(number: NameId) -> Name {
    match dynamic_index(number) {
        None => Name::Fixed(FIXED[usize::try_from(number).expect("a fixed number fits a usize")]),
        Some(index) => Name::Dynamic(TABLE.with_borrow(|table| table.dynamic(index).clone())),
    }
}

/// The name numbered `number` as a DOM string: a fixed one's made the first
/// time on the thread.
///
/// # Panics
///
/// If the thread's table gave no such number.
pub fn string(number: NameId) -> DomString {
    TABLE.with_borrow_mut(|table| match dynamic_index(number) {
        Some(index) => table.dynamic(index).clone(),
        None => {
            let index = usize::try_from(number).expect("a fixed number fits a usize");
            if table.fixed_strings.is_empty() {
                table.fixed_strings.resize(FIXED_COUNT, None);
            }
            table.fixed_strings[index]
                .get_or_insert_with(|| DomString::new(FIXED[index]))
                .clone()
        }
    })
}

/// The number of `nodeName` of an element named `prefix` and `local`
/// (`uppercase`d in an HTML document), made the first time.
pub(crate) fn node_name(prefix: NameId, local: NameId, uppercase: bool) -> NameId {
    // A fixed local name's uppercased one is fixed too: no table.
    if prefix == NONE
        && uppercase
        && let Some(&upper) = usize::try_from(local)
            .ok()
            .and_then(|local| UPPERCASE.get(local))
    {
        return NameId::from(upper);
    }
    if let Some(number) =
        TABLE.with_borrow(|table| table.node_names.get(&(prefix, local, uppercase)).copied())
    {
        return number;
    }
    let mut name = String::from(&*get(local));
    if prefix != NONE {
        name = format!("{}:{name}", get(prefix));
    }
    if uppercase {
        name.make_ascii_uppercase();
    }
    let number = intern(&name);
    TABLE.with_borrow_mut(|table| table.node_names.insert((prefix, local, uppercase), number));
    number
}

/// An element's or an attribute's name, its namespace, prefix and local
/// name, as one number (Blink's `QualifiedName`, one pointer): a node holds
/// it in 4 bytes, not 12. A name with no prefix in no namespace or the
/// HTML, SVG or MathML one (whose numbers are 0 to 3: nearly every name) is
/// in the bits, the namespace's number above the local name's: made and
/// read with no table. Only the others are numbered in the thread's table.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct QualifiedName(u32);

/// A `QualifiedName` numbered in the table, not in its bits.
const TABLED: u32 = 1 << 31;
/// Where a `QualifiedName`'s namespace is in its bits, and its local name.
const NAMESPACE_SHIFT: u32 = 29;
const LOCAL_MASK: u32 = (1 << NAMESPACE_SHIFT) - 1;

impl QualifiedName {
    /// No name (a node that isn't an element, a doctype or a processing
    /// instruction).
    pub const NONE: Self = Self(0);

    /// Its number, as a node holds it (elise's plain data is numbers).
    #[inline]
    pub(crate) fn raw(self) -> u32 {
        self.0
    }

    /// The name `raw` gave.
    #[inline]
    pub(crate) fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// The name in its bits, if it fits them: no prefix, a namespace
    /// numbered 0 to 3, a local name below 2^29.
    const fn in_bits(name: NameIds) -> Option<Self> {
        if name.prefix == NONE && name.namespace <= MATHML_NAMESPACE && name.local <= LOCAL_MASK {
            Some(Self((name.namespace << NAMESPACE_SHIFT) | name.local))
        } else {
            None
        }
    }

    /// An HTML element's name with no prefix, `local` (a fixed one: for
    /// constants).
    pub const fn html(local: NameId) -> Self {
        match Self::in_bits(NameIds {
            namespace: HTML_NAMESPACE,
            prefix: NONE,
            local,
        }) {
            Some(name) => name,
            None => panic!("a fixed local name fits a qualified name's bits"),
        }
    }

    /// An SVG element's name with no prefix, `local` (a fixed one: for
    /// constants).
    pub const fn svg(local: NameId) -> Self {
        match Self::in_bits(NameIds {
            namespace: SVG_NAMESPACE,
            prefix: NONE,
            local,
        }) {
            Some(name) => name,
            None => panic!("a fixed local name fits a qualified name's bits"),
        }
    }

    /// A name with no prefix in no namespace, `local` (a fixed one: for
    /// constants).
    pub const fn plain(local: NameId) -> Self {
        match Self::in_bits(NameIds {
            namespace: NONE,
            prefix: NONE,
            local,
        }) {
            Some(name) => name,
            None => panic!("a fixed local name fits a qualified name's bits"),
        }
    }

    /// `name`'s number, given it now if it needs the table and has none.
    pub fn new(name: NameIds) -> Self {
        if let Some(name) = Self::in_bits(name) {
            return name;
        }
        TABLE.with_borrow_mut(|table| {
            if let Some(&number) = table.qualified_numbers.get(&name) {
                return Self(number);
            }
            let number = u32::try_from(table.qualified.len())
                .ok()
                .filter(|index| index & TABLED == 0)
                .expect("fewer than 2^31 qualified names")
                | TABLED;
            table.qualified.push(name);
            table.qualified_numbers.insert(name, number);
            Self(number)
        })
    }

    /// Its namespace, prefix and local name.
    #[inline]
    pub fn ids(self) -> NameIds {
        if self.0 & TABLED == 0 {
            return NameIds {
                namespace: self.0 >> NAMESPACE_SHIFT,
                prefix: NONE,
                local: self.0 & LOCAL_MASK,
            };
        }
        self.tabled()
    }

    #[cold]
    fn tabled(self) -> NameIds {
        let index = usize::try_from(self.0 & !TABLED).expect("a u32 fits a usize");
        TABLE.with_borrow(|table| *table.qualified.get(index).expect("a name the table gave"))
    }

    #[inline]
    pub fn local(self) -> NameId {
        self.ids().local
    }

    #[inline]
    pub fn namespace(self) -> NameId {
        self.ids().namespace
    }

    #[inline]
    pub fn prefix(self) -> NameId {
        self.ids().prefix
    }

    /// Whether it is `local` in `namespace`, whatever its prefix: in its
    /// bits, one comparison.
    #[inline]
    pub fn is(self, namespace: NameId, local: NameId) -> bool {
        if self.0 & TABLED == 0 {
            return Self::in_bits(NameIds {
                namespace,
                prefix: NONE,
                local,
            }) == Some(self);
        }
        let name = self.tabled();
        name.namespace == namespace && name.local == local
    }
}

/// How many qualified names the thread's table numbers (not in their
/// bits): for tests.
#[doc(hidden)]
pub fn tabled_qualified_names() -> usize {
    TABLE.with_borrow(|table| table.qualified.len())
}

impl fmt::Debug for QualifiedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.ids();
        write!(f, "{{{}}}", get(name.namespace))?;
        if name.prefix != NONE {
            write!(f, "{}:", get(name.prefix))?;
        }
        write!(f, "{}", get(name.local))
    }
}

/// A name's text (`get`): a fixed one's, static, or a dynamic one's string.
#[derive(Clone)]
pub enum Name {
    Fixed(&'static str),
    Dynamic(DomString),
}

impl Deref for Name {
    type Target = str;

    fn deref(&self) -> &str {
        match self {
            Self::Fixed(text) => text,
            Self::Dynamic(string) => std::borrow::Borrow::borrow(string),
        }
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self)
    }
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl PartialEq<str> for Name {
    fn eq(&self, other: &str) -> bool {
        **self == *other
    }
}

impl PartialEq<&str> for Name {
    fn eq(&self, other: &&str) -> bool {
        **self == **other
    }
}
