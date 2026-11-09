//! `calc()` lengths for taffy. A computed `calc()` of a length and a
//! percentage (`LengthPercentage::Calc`) has no taffy value of its own:
//! taffy takes an opaque handle for it and asks the tree to resolve the
//! handle against a basis (`LayoutPartialTree::resolve_calc_value`). The
//! handles are indexes into the box tree's table of the values, each once.

use std::cell::RefCell;
use std::collections::HashMap;

/// The `calc()` values of a box tree's taffy styles.
#[derive(Debug, Default)]
pub(crate) struct CalcValues {
    values: RefCell<Vec<Calc>>,
    /// Each value's index, by its bits.
    index: RefCell<HashMap<(u32, u32, bool), usize>>,
}

#[derive(Debug, Clone, Copy)]
struct Calc {
    px: f32,
    percent: f32,
    non_negative: bool,
}

/// taffy keeps its tag in a handle's low 3 bits.
const SHIFT: u32 = 3;

impl CalcValues {
    /// The handle for `px` plus `percent` of the basis: never null, its
    /// low bits clear, as taffy needs.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "index is one of the values kept, far fewer than usize::MAX >> SHIFT"
    )]
    pub(crate) fn handle(&self, px: f32, percent: f32, non_negative: bool) -> *const () {
        let key = (px.to_bits(), percent.to_bits(), non_negative);
        let index = *self.index.borrow_mut().entry(key).or_insert_with(|| {
            let mut values = self.values.borrow_mut();
            let index = values.len();
            values.push(Calc {
                px,
                percent,
                non_negative,
            });
            index
        });
        std::ptr::without_provenance((index + 1) << SHIFT)
    }

    /// The length `handle` stands for, its percentage of `basis`.
    pub(crate) fn resolve(&self, handle: *const (), basis: f32) -> f32 {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "a handle `handle` made is at least 1 << SHIFT"
        )]
        let index = (handle.addr() >> SHIFT) - 1;
        let calc = self.values.borrow()[index];
        let v = calc.px + calc.percent / 100.0 * basis;
        if calc.non_negative { v.max(0.0) } else { v }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_are_aligned_shared_and_resolve() {
        let values = CalcValues::default();
        let a = values.handle(10.0, 50.0, false);
        let b = values.handle(-20.0, 100.0, true);
        assert_eq!(values.handle(10.0, 50.0, false), a, "the same value, once");
        assert_ne!(a, b);
        for handle in [a, b] {
            assert!(!handle.is_null());
            assert_eq!(handle.addr() & 0b111, 0);
        }
        assert_eq!(values.resolve(a, 200.0), 110.0);
        assert_eq!(values.resolve(b, 200.0), 180.0);
        // Clamped at 0 when it takes no negative value.
        assert_eq!(values.resolve(b, 10.0), 0.0);
    }
}
