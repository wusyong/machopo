//! A V8 string's characters read in place, not copied (Blink reads a
//! string's characters through V8's header, inline): an ASCII one-byte
//! string's bytes, lent as a `&str` ([`LentString`]) while no GC runs, as
//! a GC may move the string. That is checked, not assumed: every isolate
//! machopo makes (`V8PerIsolateData`) has a GC prologue that aborts while any
//! string has lent its text on its thread, as V8's `ValueView` disallows
//! GCs while it lives (but counted, so that loans needn't nest).

use std::cell::Cell;
use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::{self, NonNull};

/// The isolate data slot (after rusty_v8's own and the state's) marking
/// an isolate whose GCs check that no text is borrowed (`guard_gcs`).
const GUARDED_SLOT: u32 = 1;

thread_local! {
    /// The texts borrowed on this thread: V8's GC prologue on it aborts
    /// unless none.
    static BORROWED: Cell<usize> = const { Cell::new(0) };
    /// The strings' characters scanned for ASCII on this thread, for the
    /// tests. Counted in debug builds only.
    static ASCII_SCANS: Cell<u64> = const { Cell::new(0) };
}

/// Gives `isolate` the GC prologue that aborts while a text is borrowed
/// on its thread, and marks it: [`borrow_ascii`] borrows none of an
/// isolate without it.
pub(crate) fn guard_gcs(isolate: &mut v8::Isolate) {
    isolate.add_gc_prologue_callback(no_text_borrowed, ptr::null_mut(), v8::GCType::kGCTypeAll);
    isolate.set_data(GUARDED_SLOT, NonNull::<c_void>::dangling().as_ptr());
}

/// V8's GC prologue (any GC: a scavenge moves strings too), on the
/// isolate's thread, before anything moves.
unsafe extern "C" fn no_text_borrowed(
    _: v8::UnsafeRawIsolatePtr,
    _: v8::GCType,
    _: v8::GCCallbackFlags,
    _: *mut c_void,
) {
    if BORROWED.get() != 0 {
        eprintln!("V8 collects garbage while a string's text is borrowed in place");
        std::process::abort();
    }
}

/// A V8 string whose text may be lent in place ([`LentString::ascii`]):
/// from the first loan until it drops, V8's GCs abort.
pub struct LentString<'s> {
    string: v8::Local<'s, v8::String>,
    /// Whether it lent its text (counted in `BORROWED`).
    lent: Cell<bool>,
    /// Whether its characters were found ASCII, on a guarded isolate: a V8
    /// string's characters never change (a GC may move them, or make it
    /// thin, forwarding to the same ones), so later reads find where they
    /// are and scan them no more.
    ascii: Cell<bool>,
    /// The count is this thread's.
    _thread: PhantomData<*const ()>,
}

impl<'s> LentString<'s> {
    /// `string`, which must be the isolate's that it is later read with (as
    /// `v8::ValueView::new` takes it): machopo's bindings read a callback's
    /// arguments on its isolate.
    pub fn new(string: v8::Local<'s, v8::String>) -> Self {
        Self {
            string,
            lent: Cell::new(false),
            ascii: Cell::new(false),
            _thread: PhantomData,
        }
    }

    pub fn string(&self) -> v8::Local<'s, v8::String> {
        self.string
    }

    /// Its text in place, if it is a flat one-byte ASCII string of an
    /// isolate whose GCs check loans (`V8PerIsolateData`'s); else `None`,
    /// and the caller reads it otherwise (V8's `ValueView`, a copy). V8's
    /// GCs abort from now until it drops.
    #[inline]
    pub fn ascii(&self, isolate: &v8::Isolate) -> Option<&str> {
        let chars = self.chars(isolate)?;
        if !self.lent.get() {
            BORROWED.set(
                BORROWED
                    .get()
                    .checked_add(1)
                    .expect("fewer than usize::MAX loans"),
            );
            self.lent.set(true);
        }
        // SAFETY: the characters live while no GC runs (`chars`), which
        // the count makes so until `self` drops, past the borrow of it.
        Some(unsafe { &*chars })
    }

    /// `f` of its text in place, lent for the call alone (the count up
    /// meanwhile: a GC in `f` aborts), if `ascii` would lend it: a peek
    /// before what may collect (a wrapper made), with no loan kept.
    #[inline]
    pub fn peek_ascii<R>(&self, isolate: &v8::Isolate, f: impl FnOnce(&str) -> R) -> Option<R> {
        let chars = self.chars(isolate)?;
        BORROWED.set(
            BORROWED
                .get()
                .checked_add(1)
                .expect("fewer than usize::MAX loans"),
        );
        // SAFETY: the characters live while no GC runs (`chars`), which the
        // count makes so until it goes down below, after `f` (which the
        // borrow can't outlive) returns; a panic in `f` aborts with the
        // count up, as any GC on the thread would.
        let result = f(unsafe { &*chars });
        BORROWED.set(BORROWED.get().saturating_sub(1));
        Some(result)
    }

    /// Its text as UTF-8, a copy, if it is a flat one-byte string of an
    /// isolate whose GCs check loans: its Latin-1 characters read where
    /// they are and converted (simdutf), instead of through V8's
    /// `ValueView`. For a string that isn't ASCII (which `ascii` won't
    /// lend: Latin-1 isn't UTF-8).
    pub fn latin1_to_utf8(&self, isolate: &v8::Isolate) -> Option<String> {
        if isolate.get_data(GUARDED_SLOT).is_null() {
            return None;
        }
        // SAFETY: as `chars`': the string is `isolate`'s, rooted by its
        // handle; the slice is read below, before anything of V8's is
        // called (nothing allocates between: no GC).
        let chars = unsafe { v8::machopo::one_byte_chars(isolate, &self.string) }?;
        let capacity = chars.len().checked_mul(2)?;
        let mut utf8 = vec![0; capacity];
        // SAFETY: the output has twice the input's length, simdutf's
        // bound for Latin-1 (a character is one or two bytes of UTF-8).
        let written = unsafe { v8::simdutf::convert_latin1_to_utf8(chars, &mut utf8) };
        utf8.truncate(written);
        // SAFETY: simdutf wrote `written` bytes of valid UTF-8 (any
        // Latin-1 input converts).
        Some(unsafe { String::from_utf8_unchecked(utf8) })
    }

    /// Whether `ascii` would lend its text: lending none yet.
    pub fn lends(&self, isolate: &v8::Isolate) -> bool {
        self.chars(isolate).is_some()
    }

    /// Its ASCII characters in place: valid while no GC runs. Found ASCII
    /// once (`lends`, before the operation), they are found again with no
    /// scan (Blink reads a string's characters once it knows them).
    #[inline]
    fn chars(&self, isolate: &v8::Isolate) -> Option<*const str> {
        let known = self.ascii.get();
        if !known && isolate.get_data(GUARDED_SLOT).is_null() {
            return None;
        }
        // SAFETY: the string is `isolate`'s (`new`'s contract), rooted by
        // its handle for `'s`; the slice is used only to make a pointer,
        // dereferenced while no GC runs (`ascii`).
        let chars = unsafe { v8::machopo::one_byte_chars(isolate, &self.string) }?;
        if !known {
            if cfg!(debug_assertions) {
                ASCII_SCANS.set(ASCII_SCANS.get().wrapping_add(1));
            }
            if !chars.is_ascii() {
                return None;
            }
            self.ascii.set(true);
        }
        // SAFETY: ASCII is UTF-8: these characters, or the same ones moved
        // or forwarded to (`ascii`'s field), were found ASCII.
        Some(unsafe { std::str::from_utf8_unchecked(chars) } as *const str)
    }
}

impl Drop for LentString<'_> {
    #[inline]
    fn drop(&mut self) {
        if self.lent.get() {
            BORROWED.set(BORROWED.get().saturating_sub(1));
        }
    }
}

/// How many texts are borrowed on this thread, for the tests.
pub fn borrowed_texts() -> usize {
    BORROWED.get()
}

/// How many times, on this thread, a string's characters were scanned for
/// ASCII: what tells one found ASCII is not scanned again. `None` in
/// release builds. For tests.
pub fn ascii_scans_for_testing() -> Option<u64> {
    cfg!(debug_assertions).then(|| ASCII_SCANS.get())
}
