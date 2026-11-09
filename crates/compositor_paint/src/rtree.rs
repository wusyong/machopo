//! `cc::RTree`: a static R-tree over a display list's visual rects, built
//! once when the list is finalized and searched for each tile it is
//! rastered into.

use gfx::RectF;

/// Children a node has at most (cc's `RTree::kMaxChildren`).
const MAX_CHILDREN: usize = 11;

/// A rect and what it stands for: at the leaves, an op's index (`start`);
/// above, the node's children, `start..end` in the level below.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Entry {
    rect: RectF,
    start: u32,
    end: u32,
}

/// Built bottom up by sort-tile-recursive packing (STR): each level's
/// entries are sorted into vertical slices by their centers' x, each slice
/// by y, and cut into runs of at most `MAX_CHILDREN`, each run a node of
/// the level above, until one level fits in a node. Static: a finalized
/// list doesn't change, so nothing is ever inserted or removed.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RTree {
    /// `levels[0]` is the leaves; the last level, the root's children.
    levels: Vec<Vec<Entry>>,
}

impl RTree {
    /// A tree of `rects`, each with its index; empty rects, which nothing
    /// meets, are left out.
    pub(crate) fn new(rects: impl IntoIterator<Item = RectF>) -> Self {
        let mut level: Vec<Entry> = rects
            .into_iter()
            .enumerate()
            .filter(|(_, rect)| !rect.is_empty())
            .map(|(index, rect)| Entry {
                rect,
                start: u32::try_from(index).expect("fewer than 2^32 ops"),
                end: 0,
            })
            .collect();
        let mut levels = Vec::new();
        while level.len() > MAX_CHILDREN {
            let firsts = pack(&mut level);
            // Each node's entries end where the next node's start.
            let ends = firsts.iter().skip(1).copied().chain([level.len()]);
            let above = firsts
                .iter()
                .zip(ends)
                .map(|(&first, end)| {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "first <= end <= level.len(), no more entries than the \
                                  ops, fewer than 2^32 (checked as the leaves were made)"
                    )]
                    let (start, end_index) = (first as u32, end as u32);
                    Entry {
                        rect: level[first..end]
                            .iter()
                            .fold(RectF::default(), |union, entry| union.union(&entry.rect)),
                        start,
                        end: end_index,
                    }
                })
                .collect();
            levels.push(level);
            level = above;
        }
        if !level.is_empty() {
            levels.push(level);
        }
        Self { levels }
    }

    /// The indices of the rects that meet `rect` (overlap it with some
    /// area), in increasing order.
    pub(crate) fn search(&self, rect: RectF) -> Vec<usize> {
        let mut found = Vec::new();
        if let Some(top) = self.levels.len().checked_sub(1) {
            self.search_in(top, 0..self.levels[top].len(), rect, &mut found);
        }
        found.sort_unstable();
        found
    }

    fn search_in(
        &self,
        level: usize,
        entries: std::ops::Range<usize>,
        rect: RectF,
        found: &mut Vec<usize>,
    ) {
        let this = &self.levels[level];
        for i in entries {
            let entry = this[i];
            if !meets(&entry.rect, &rect) {
                continue;
            }
            match level.checked_sub(1) {
                None => found.push(entry.start as usize),
                Some(below) => {
                    self.search_in(below, entry.start as usize..entry.end as usize, rect, found)
                }
            }
        }
    }
}

/// Whether `a` and `b` share some area: rects that only touch don't.
pub(crate) fn meets(a: &RectF, b: &RectF) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

/// Sorts `entries` into STR's runs, as the nodes of the level above will
/// have them, and returns where each run starts.
fn pack(entries: &mut [Entry]) -> Vec<usize> {
    let center = |rect: &RectF, axis: bool| {
        if axis {
            rect.x + rect.width / 2.0
        } else {
            rect.y + rect.height / 2.0
        }
    };
    let nodes = entries.len().div_ceil(MAX_CHILDREN);
    let slices: usize = base::numerics::clamp_ceil((nodes as f64).sqrt());
    let per_slice = entries.len().div_ceil(slices);
    entries.sort_by(|a, b| center(&a.rect, true).total_cmp(&center(&b.rect, true)));
    // Only a capacity: no more than both, which fit as `entries` does.
    let mut firsts = Vec::with_capacity(nodes.saturating_add(slices));
    for (slice, run) in entries.chunks_mut(per_slice).enumerate() {
        run.sort_by(|a, b| center(&a.rect, false).total_cmp(&center(&b.rect, false)));
        // As many nodes as it takes, of sizes as even as they can be.
        let count = run.len().div_ceil(MAX_CHILDREN);
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "where run `slice` starts in `entries`; and k < count, so \
                      run.len() * k / count < run.len(), and run.len() * k is about \
                      run.len()^2 / MAX_CHILDREN, far within a usize"
        )]
        let first_of = |k: usize| slice * per_slice + run.len() * k / count;
        firsts.extend((0..count).map(first_of));
    }
    firsts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// xorshift64*: the same rects for the same seed, no dependencies.
    fn rects(seed: u64, count: usize) -> Vec<RectF> {
        let mut state = seed;
        let mut next = move |range: f32| {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / (1u64 << 24) as f32 * range
        };
        (0..count)
            .map(|_| RectF::new(next(1000.0), next(1000.0), next(120.0), next(120.0)))
            .collect()
    }

    #[test]
    fn a_search_finds_what_a_scan_does_in_order() {
        for count in [0, 1, 11, 12, 121, 122, 2000] {
            let all = rects(count as u64 + 1, count);
            let tree = RTree::new(all.iter().copied());
            for (i, query) in rects(7, 50).into_iter().enumerate() {
                let query = RectF::new(query.x, query.y, query.width * 3.0, query.height * 3.0);
                let scanned: Vec<usize> =
                    (0..all.len()).filter(|&j| meets(&all[j], &query)).collect();
                assert_eq!(tree.search(query), scanned, "{count} rects, query {i}");
            }
        }
    }

    #[test]
    fn its_nodes_have_at_most_eleven_children_and_hold_them() {
        let all = rects(3, 2000);
        let tree = RTree::new(all.iter().copied());
        assert!(tree.levels.len() >= 3, "{}", tree.levels.len());
        assert!(tree.levels.last().unwrap().len() <= MAX_CHILDREN);
        for level in 1..tree.levels.len() {
            let (nodes, below) = (&tree.levels[level], &tree.levels[level - 1]);
            let mut children_seen = 0;
            for node in nodes {
                let children = &below[node.start as usize..node.end as usize];
                children_seen += children.len();
                assert!(
                    (1..=MAX_CHILDREN).contains(&children.len()),
                    "{}",
                    children.len()
                );
                for child in children {
                    assert_eq!(child.rect.union(&node.rect), node.rect);
                }
            }
            assert_eq!(children_seen, below.len(), "each child has one parent");
        }
    }

    #[test]
    fn touching_and_empty_rects_meet_nothing() {
        let tree = RTree::new([
            RectF::new(0.0, 0.0, 10.0, 10.0),
            RectF::new(10.0, 0.0, 0.0, 10.0),
            RectF::new(20.0, 0.0, 10.0, 10.0),
        ]);
        assert_eq!(
            tree.search(RectF::new(10.0, 0.0, 10.0, 10.0)),
            Vec::<usize>::new()
        );
        assert_eq!(tree.search(RectF::new(9.5, 0.0, 11.0, 1.0)), vec![0, 2]);
        assert_eq!(
            tree.search(RectF::new(0.0, 0.0, 0.0, 0.0)),
            Vec::<usize>::new()
        );
    }
}
