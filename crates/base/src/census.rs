//! A census of what each binding call makes on its way: the scopes of
//! V8's it opens, the DOM heap's roots it registers and the operations it
//! runs, by the callback that was called. Kept with the `census` feature
//! alone, and written to the file `MACHOPO_CENSUS` names; without the
//! feature every function here is empty, so the callers call them
//! unconditionally.
//!
//! A callback's numbers are its own: what the callbacks called within it
//! made (a listener's calls within `dispatchEvent`) is theirs.

/// What a call makes that the census counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A scope a callback given V8's info alone opened for itself.
    Scope,
    /// The scope rusty_v8's trampoline opens before the callback.
    TrampolineScope,
    /// A scope opened only to tell V8 how the DOM's heap grew.
    ReportScope,
    /// An operation with no safepoint.
    Read,
    /// An operation with a safepoint first.
    Mutate,
    /// A root of the DOM's heap.
    Root,
}

/// The kinds, in the order of a row's columns.
pub const KINDS: [Kind; 6] = [
    Kind::Scope,
    Kind::TrampolineScope,
    Kind::ReportScope,
    Kind::Read,
    Kind::Mutate,
    Kind::Root,
];

/// One more of `kind` on this thread: the running callback's.
#[inline(always)]
pub fn count(kind: Kind) {
    #[cfg(feature = "census")]
    imp::count(kind);
    #[cfg(not(feature = "census"))]
    let _ = kind;
}

/// Names the running callback's member (`Interface.member`, and whether
/// it is a getter, a setter, a method or a constructor): a callback that
/// is one generic function for every member tells them apart by it.
#[inline(always)]
pub fn label(interface: &'static str, member: &'static str, kind: &'static str) {
    #[cfg(feature = "census")]
    imp::label(interface, member, kind);
    #[cfg(not(feature = "census"))]
    let _ = (interface, member, kind);
}

/// A callback's run: from `enter` to its drop.
#[must_use]
pub struct Call {
    #[cfg(feature = "census")]
    inner: imp::Running,
}

impl Call {
    /// A callback begins: `entry` names it (its type), and `roots` tells
    /// how many roots the thread made so far (the heap's own count, which
    /// this crate can't reach).
    #[inline(always)]
    pub fn enter(entry: fn() -> &'static str, roots: fn() -> u64) -> Self {
        #[cfg(not(feature = "census"))]
        let _ = (entry, roots);
        Self {
            #[cfg(feature = "census")]
            inner: imp::Running::enter(entry, roots),
        }
    }
}

#[cfg(feature = "census")]
impl Drop for Call {
    fn drop(&mut self) {
        self.inner.exit();
    }
}

#[cfg(feature = "census")]
mod imp {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::io::Write as _;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use super::{KINDS, Kind};

    type Counts = [u64; KINDS.len()];
    type Label = (&'static str, &'static str, &'static str);

    const fn index(kind: Kind) -> usize {
        match kind {
            Kind::Scope => 0,
            Kind::TrampolineScope => 1,
            Kind::ReportScope => 2,
            Kind::Read => 3,
            Kind::Mutate => 4,
            Kind::Root => 5,
        }
    }

    fn add(a: Counts, b: Counts) -> Counts {
        std::array::from_fn(|i| a[i].wrapping_add(b[i]))
    }

    fn sub(a: Counts, b: Counts) -> Counts {
        std::array::from_fn(|i| a[i].saturating_sub(b[i]))
    }

    thread_local! {
        /// What this thread made so far (its roots are read of the heap).
        static MADE: Cell<Counts> = const { Cell::new([0; KINDS.len()]) };
        /// What the callbacks called within the running one made.
        static WITHIN: Cell<Counts> = const { Cell::new([0; KINDS.len()]) };
        /// What this thread's outermost callbacks made, with all within.
        static IN_CALLS: Cell<Counts> = const { Cell::new([0; KINDS.len()]) };
        static LABEL: Cell<Option<Label>> = const { Cell::new(None) };
        static DEPTH: Cell<u32> = const { Cell::new(0) };
        static WRITTEN: Cell<Option<Instant>> = const { Cell::new(None) };
        /// What this thread made outside any callback, as of the last
        /// write: the table holds each thread's by its difference.
        static OUTSIDE: Cell<Counts> = const { Cell::new([0; KINDS.len()]) };
        static AT_END: AtEnd = const { AtEnd };
    }

    /// Writes the table as its thread ends: a short page ends before the
    /// next write is due.
    struct AtEnd;

    impl Drop for AtEnd {
        fn drop(&mut self) {
            write(None);
        }
    }

    #[derive(Default)]
    struct Row {
        calls: u64,
        /// How many of each kind its calls made.
        made: Counts,
        /// How many of its calls made any of each kind.
        with: Counts,
    }

    type Table = HashMap<(&'static str, Option<Label>), Row>;

    fn table() -> &'static Mutex<Table> {
        static TABLE: OnceLock<Mutex<Table>> = OnceLock::new();
        TABLE.get_or_init(Mutex::default)
    }

    pub(super) fn count(kind: Kind) {
        MADE.with(|made| {
            let mut counts = made.get();
            counts[index(kind)] = counts[index(kind)].wrapping_add(1);
            made.set(counts);
        });
    }

    pub(super) fn label(interface: &'static str, member: &'static str, kind: &'static str) {
        LABEL.set(Some((interface, member, kind)));
    }

    fn now(roots: fn() -> u64) -> Counts {
        let mut counts = MADE.get();
        counts[index(Kind::Root)] = roots();
        counts
    }

    pub(super) struct Running {
        entry: fn() -> &'static str,
        roots: fn() -> u64,
        start: Counts,
        outer_within: Counts,
        outer_label: Option<Label>,
    }

    impl Running {
        pub(super) fn enter(entry: fn() -> &'static str, roots: fn() -> u64) -> Self {
            AT_END.with(|_| ());
            DEPTH.set(DEPTH.get().wrapping_add(1));
            Self {
                entry,
                roots,
                start: now(roots),
                outer_within: WITHIN.replace([0; KINDS.len()]),
                outer_label: LABEL.take(),
            }
        }

        pub(super) fn exit(&mut self) {
            let whole = sub(now(self.roots), self.start);
            let own = sub(whole, WITHIN.get());
            WITHIN.set(add(self.outer_within, whole));
            let label = LABEL.replace(self.outer_label);
            {
                let mut table = table().lock().expect("the census's table");
                let row = table.entry(((self.entry)(), label)).or_default();
                row.calls = row.calls.wrapping_add(1);
                row.made = add(row.made, own);
                row.with = add(row.with, own.map(|n| u64::from(n != 0)));
            }
            let depth = DEPTH.get().wrapping_sub(1);
            DEPTH.set(depth);
            if depth == 0 {
                IN_CALLS.set(add(IN_CALLS.get(), whole));
                let due = WRITTEN
                    .get()
                    .is_none_or(|at| at.elapsed() > Duration::from_millis(200));
                if due {
                    WRITTEN.set(Some(Instant::now()));
                    write(Some(now(self.roots)));
                }
            }
        }
    }

    /// The row of `entry` with no label, as (calls, made): for the tests.
    #[cfg(test)]
    pub(super) fn row(entry: &'static str) -> Option<(u64, Counts)> {
        let table = table().lock().expect("the census's table");
        table.get(&(entry, None)).map(|row| (row.calls, row.made))
    }

    /// Writes the whole table over the file: five times a second at most, as an
    /// outermost callback ends, since a renderer's end isn't one place.
    /// `made` is what the thread made so far, for what it made outside any
    /// callback; none as the thread ends (its counts may be gone).
    fn write(made: Option<Counts>) {
        static PATH: OnceLock<Option<std::ffi::OsString>> = OnceLock::new();
        let Some(path) = PATH.get_or_init(|| std::env::var_os("MACHOPO_CENSUS")) else {
            return;
        };
        let mut table = table().lock().expect("the census's table");
        if let Some(made) = made {
            let outside = sub(made, IN_CALLS.get());
            let more = sub(outside, OUTSIDE.replace(outside));
            let row = table.entry(("(outside a callback)", None)).or_default();
            row.made = add(row.made, more);
        }
        let mut text = String::from(
            "entry\tinterface\tmember\tkind\tcalls\tscopes\ttrampoline_scopes\treport_scopes\t\
             reads\tmutates\troots\twith_scope\twith_trampoline_scope\twith_report_scope\t\
             with_read\twith_mutate\twith_root\n",
        );
        for ((entry, label), row) in table.iter() {
            let (interface, member, kind) = label.unwrap_or_default();
            text.push_str(&format!("{entry}\t{interface}\t{member}\t{kind}\t{}", row.calls));
            for n in row.made.iter().chain(&row.with) {
                text.push_str(&format!("\t{n}"));
            }
            text.push('\n');
        }
        drop(table);
        let mut partial = path.clone();
        partial.push(".partial");
        let written = std::fs::File::create(&partial)
            .and_then(|mut file| file.write_all(text.as_bytes()))
            .and_then(|()| std::fs::rename(&partial, path));
        if let Err(e) = written {
            eprintln!("machopo: MACHOPO_CENSUS: can't write {}: {e}", path.display());
        }
    }
}

#[cfg(all(test, feature = "census"))]
mod tests {
    use super::{Call, Kind, count, imp};

    fn no_roots() -> u64 {
        0
    }

    /// A callback's row holds what it made itself: what a callback called
    /// within it made is that one's.
    #[test]
    fn a_callback_is_counted_what_it_made_itself() {
        {
            let _outer = Call::enter(|| "outer", no_roots);
            count(Kind::Scope);
            count(Kind::Read);
            {
                let _inner = Call::enter(|| "inner", no_roots);
                count(Kind::Read);
                count(Kind::Mutate);
            }
            count(Kind::Read);
        }
        // Scope, trampoline scope, report scope, read, mutate, root.
        assert_eq!(imp::row("outer"), Some((1, [1, 0, 0, 2, 0, 0])));
        assert_eq!(imp::row("inner"), Some((1, [0, 0, 0, 1, 1, 0])));
    }
}
