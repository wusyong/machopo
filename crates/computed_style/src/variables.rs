//! Custom properties' computed values (Blink's `StyleVariables`): a
//! persistent hash array-mapped trie from name to value.
//!
//! Every custom property inherits, so an element's map starts as its
//! parent's: copying one is copying the root `Arc`, and setting a value
//! copies only the nodes on the way to it (a node shared with another map
//! is copied, one this map owns is changed in place). Pages declare a
//! thousand properties on one element near the root and a few dozen on
//! each component below it; each of those costs a few short paths, not a
//! copy of the thousand.
//!
//! Each node has 16 slots, chosen by the next 4 bits of the name's hash,
//! and only the used ones are stored (a bitmap says which). A slot is
//! either entries (more than one only if their whole hashes are equal)
//! or a child node. The map also keeps the XOR of every entry's hash, so
//! that two maps that differ mostly compare unequal without a walk.
//!
//! There is no removal: a property that is invalid at computed-value time
//! is stored with no value, which `get` treats as absent but the names
//! still list (as Blink's map stores a null value).

use std::fmt;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct StyleVariables {
    root: Option<Arc<Node>>,
    len: usize,
    /// XOR of every entry's `mix`.
    hash: u64,
}

#[derive(Clone)]
struct Node {
    /// Which of the 16 slots are used: `slots` holds them in order.
    bitmap: u16,
    slots: Vec<Slot>,
}

#[derive(Clone)]
enum Slot {
    /// Entries with the same hash (almost always one).
    Entries(Vec<Entry>),
    Child(Arc<Node>),
}

#[derive(Clone, PartialEq)]
struct Entry {
    hash: u64,
    name: Arc<str>,
    value: Option<Arc<str>>,
}

impl Entry {
    /// What the map's hash keeps of this entry: its name and value.
    fn mix(&self) -> u64 {
        let value = self.value.as_deref().map_or(0, |v| hash(v) | 1);
        self.hash ^ value.rotate_left(29)
    }
}

const BITS: u32 = 4;

/// The slot of `hash` at `shift`.
fn slot_index(hash: u64, shift: u32) -> u32 {
    ((hash >> shift) & ((1 << BITS) - 1)) as u32
}

/// FNV-1a, finished with SplitMix64's mixing so that every 4 bits of it
/// depend on the whole name.
fn hash(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        h ^= u64::from(byte);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h ^= h >> 30;
    h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^ (h >> 31)
}

impl Node {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "a slot index is below 2^BITS, so `bit` is 1 at least"
    )]
    fn position(&self, index: u32) -> Option<usize> {
        let bit = 1u16 << index;
        (self.bitmap & bit != 0).then(|| (self.bitmap & (bit - 1)).count_ones() as usize)
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "two different hashes differ in some level below 64 bits, so `shift + BITS` stays below 64"
    )]
    fn get(&self, hash: u64, name: &str, shift: u32) -> Option<&Entry> {
        let position = self.position(slot_index(hash, shift))?;
        match &self.slots[position] {
            Slot::Entries(entries) => entries.iter().find(|e| e.hash == hash && &*e.name == name),
            Slot::Child(child) => child.get(hash, name, shift + BITS),
        }
    }

    /// Sets `entry`; returns the entry it replaced.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "a slot index is below 2^BITS, so `bit` is 1 at least; two different hashes \
                  differ in some level below 64 bits, so `shift + BITS` stays below 64"
    )]
    fn insert(&mut self, entry: Entry, shift: u32) -> Option<Entry> {
        let index = slot_index(entry.hash, shift);
        let Some(position) = self.position(index) else {
            let bit = 1u16 << index;
            let position = (self.bitmap & (bit - 1)).count_ones() as usize;
            self.bitmap |= bit;
            self.slots.insert(position, Slot::Entries(vec![entry]));
            return None;
        };
        match &mut self.slots[position] {
            Slot::Child(child) => Arc::make_mut(child).insert(entry, shift + BITS),
            Slot::Entries(entries) if entries[0].hash == entry.hash => {
                match entries.iter_mut().find(|e| e.name == entry.name) {
                    Some(old) => Some(std::mem::replace(old, entry)),
                    None => {
                        entries.push(entry);
                        None
                    }
                }
            }
            Slot::Entries(entries) => {
                // Another hash in this slot: both go one level down,
                // where their next bits tell them apart (the whole
                // hashes differ, so some level does).
                let mut child = Node {
                    bitmap: 0,
                    slots: Vec::new(),
                };
                for old in std::mem::take(entries) {
                    child.insert(old, shift + BITS);
                }
                child.insert(entry, shift + BITS);
                self.slots[position] = Slot::Child(Arc::new(child));
                None
            }
        }
    }

    fn for_each<'a>(&'a self, f: &mut impl FnMut(&'a Entry)) {
        for slot in &self.slots {
            match slot {
                Slot::Entries(entries) => entries.iter().for_each(&mut *f),
                Slot::Child(child) => child.for_each(f),
            }
        }
    }
}

impl StyleVariables {
    pub fn new() -> Self {
        Self::default()
    }

    fn entry(&self, name: &str) -> Option<&Entry> {
        self.root.as_ref()?.get(hash(name), name, 0)
    }

    /// The value of custom property `name`; `None` if it has none (not
    /// set, or invalid at computed-value time).
    pub fn get(&self, name: &str) -> Option<&Arc<str>> {
        self.entry(name)?.value.as_ref()
    }

    /// Whether `name` is in the map, even with no value.
    pub fn contains(&self, name: &str) -> bool {
        self.entry(name).is_some()
    }

    /// Sets custom property `name` to `value`, `None` for invalid at
    /// computed-value time.
    pub fn set(&mut self, name: Arc<str>, value: Option<Arc<str>>) {
        let entry = Entry {
            hash: hash(&name),
            name,
            value,
        };
        if self.entry(&entry.name) == Some(&entry) {
            return;
        }
        let mix = entry.mix();
        let root = self.root.get_or_insert_with(|| {
            Arc::new(Node {
                bitmap: 0,
                slots: Vec::new(),
            })
        });
        match Arc::make_mut(root).insert(entry, 0) {
            Some(old) => self.hash ^= old.mix(),
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "one per custom property set, each in memory"
            )]
            None => self.len += 1,
        }
        self.hash ^= mix;
    }

    /// Whether the two are copies of one map, unchanged since: they
    /// compare equal without a walk.
    pub fn same_map(&self, other: &Self) -> bool {
        match (&self.root, &other.root) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// The number of names, with a value or not.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Every name with its value (`None` for invalid at computed-value
    /// time), in no particular order.
    pub fn entries(&self) -> Vec<(&Arc<str>, Option<&Arc<str>>)> {
        let mut entries = Vec::with_capacity(self.len);
        if let Some(root) = &self.root {
            root.for_each(&mut |e| entries.push((&e.name, e.value.as_ref())));
        }
        entries
    }
}

impl PartialEq for StyleVariables {
    fn eq(&self, other: &Self) -> bool {
        match (&self.root, &other.root) {
            (None, None) => return true,
            (Some(a), Some(b)) if Arc::ptr_eq(a, b) => return true,
            _ => {}
        }
        if self.len != other.len || self.hash != other.hash {
            return false;
        }
        match (&self.root, &other.root) {
            (Some(a), Some(b)) => Node::same(a, b),
            _ => true,
        }
    }
}

impl Node {
    /// Whether two tries hold the same entries. Only insertions build
    /// them, so the same names make the same shape whatever the order:
    /// slot by slot, skipping the subtrees they share.
    fn same(a: &Self, b: &Self) -> bool {
        a.bitmap == b.bitmap
            && a.slots.iter().zip(&b.slots).all(|pair| match pair {
                (Slot::Child(x), Slot::Child(y)) => Arc::ptr_eq(x, y) || Self::same(x, y),
                (Slot::Entries(x), Slot::Entries(y)) => {
                    x.len() == y.len() && x.iter().all(|e| y.contains(e))
                }
                _ => false,
            })
    }
}

impl fmt::Debug for StyleVariables {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut entries = self.entries();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        f.debug_map().entries(entries).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> Option<Arc<str>> {
        Some(text.into())
    }

    #[test]
    fn sets_gets_and_replaces() {
        let mut vars = StyleVariables::new();
        assert!(vars.is_empty());
        assert_eq!(vars.get("--a"), None);
        vars.set("--a".into(), value("1"));
        vars.set("--b".into(), value("2"));
        vars.set("--a".into(), value("3"));
        assert_eq!(vars.get("--a").map(|v| &**v), Some("3"));
        assert_eq!(vars.get("--b").map(|v| &**v), Some("2"));
        assert_eq!(vars.len(), 2);
        vars.set("--c".into(), None);
        assert_eq!(vars.get("--c"), None);
        assert!(vars.contains("--c"));
        assert!(!vars.contains("--d"));
        assert_eq!(vars.len(), 3);
    }

    #[test]
    fn copies_share_until_changed() {
        let mut parent = StyleVariables::new();
        for i in 0..2000 {
            parent.set(format!("--p{i}").into(), value(&i.to_string()));
        }
        let mut child = parent.clone();
        assert_eq!(child, parent);
        child.set("--p7".into(), value("seven"));
        child.set("--own".into(), value("x"));
        assert_ne!(child, parent);
        // The parent didn't change.
        assert_eq!(parent.get("--p7").map(|v| &**v), Some("7"));
        assert_eq!(parent.get("--own"), None);
        assert_eq!(child.get("--p7").map(|v| &**v), Some("seven"));
        assert_eq!(child.get("--p1999").map(|v| &**v), Some("1999"));
        assert_eq!(child.len(), 2001);
        for i in 0..2000 {
            assert!(child.contains(&format!("--p{i}")));
        }
        // The same value again changes nothing.
        let before = child.clone();
        child.set("--own".into(), value("x"));
        assert!(Arc::ptr_eq(
            child.root.as_ref().unwrap(),
            before.root.as_ref().unwrap()
        ));
    }

    #[test]
    fn equality_ignores_how_a_map_was_built() {
        let mut a = StyleVariables::new();
        let mut b = StyleVariables::new();
        for i in 0..100 {
            a.set(format!("--v{i}").into(), value("1"));
        }
        for i in (0..100).rev() {
            b.set(format!("--v{i}").into(), value("1"));
        }
        assert_eq!(a, b);
        b.set("--v50".into(), value("2"));
        assert_ne!(a, b);
        b.set("--v50".into(), value("1"));
        assert_eq!(a, b);
        // A name with no value isn't the same as no name.
        let mut c = a.clone();
        c.set("--none".into(), None);
        assert_ne!(a, c);
    }

    #[test]
    fn entries_list_every_name_once() {
        let mut vars = StyleVariables::new();
        for i in 0..300 {
            vars.set(format!("--v{i}").into(), value(&i.to_string()));
        }
        vars.set("--v3".into(), None);
        let mut names: Vec<String> = vars.entries().iter().map(|(n, _)| n.to_string()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 300);
        assert_eq!(
            vars.entries()
                .iter()
                .filter(|(_, value)| value.is_none())
                .count(),
            1
        );
    }

    #[test]
    fn equal_hashes_share_a_slot() {
        // Entries with the same whole hash: inserted by hand, since no two
        // short names are known to collide.
        let mut node = Node {
            bitmap: 0,
            slots: Vec::new(),
        };
        let entry = |name: &str, v: &str| Entry {
            hash: 42,
            name: name.into(),
            value: value(v),
        };
        assert!(node.insert(entry("--a", "1"), 0).is_none());
        assert!(node.insert(entry("--b", "2"), 0).is_none());
        assert_eq!(node.get(42, "--a", 0).unwrap().value, value("1"));
        assert_eq!(node.get(42, "--b", 0).unwrap().value, value("2"));
        assert!(node.insert(entry("--a", "3"), 0).is_some());
        assert_eq!(node.get(42, "--a", 0).unwrap().value, value("3"));
        assert!(node.get(42, "--c", 0).is_none());
    }
}
