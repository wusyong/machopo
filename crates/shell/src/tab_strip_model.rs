//! `TabStripModel` (Chromium's, in `chrome/browser/ui/tabs`): a window's
//! tabs in order, and which one is shown. It owns each tab's contents; the
//! view shows the active one.

/// The tabs, never none: closing the last one closes the window instead.
pub struct TabStripModel<C> {
    tabs: Vec<C>,
    active: usize,
}

impl<C> TabStripModel<C> {
    pub fn new(first: C) -> Self {
        Self {
            tabs: vec![first],
            active: 0,
        }
    }

    pub fn count(&self) -> usize {
        self.tabs.len()
    }

    /// The last tab's index.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "there is a tab at least (closing the last closes the window)"
    )]
    pub fn last_index(&self) -> usize {
        self.tabs.len() - 1
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active(&self) -> &C {
        &self.tabs[self.active]
    }

    pub fn active_mut(&mut self) -> &mut C {
        &mut self.tabs[self.active]
    }

    pub fn iter(&self) -> impl Iterator<Item = &C> {
        self.tabs.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut C> {
        self.tabs.iter_mut()
    }

    pub fn position(&self, is: impl FnMut(&C) -> bool) -> Option<usize> {
        self.tabs.iter().position(is)
    }

    /// Adds `tab` last, as a new tab is (`AddTabAt` at the end); it isn't
    /// made active. Its index.
    pub fn append(&mut self, tab: C) -> usize {
        let index = self.tabs.len();
        self.tabs.push(tab);
        index
    }

    /// Puts `tab` at `index` (at most the end); it isn't made active, and
    /// the active one stays the same tab.
    pub fn insert(&mut self, index: usize, tab: C) -> usize {
        let index = index.min(self.tabs.len());
        self.tabs.insert(index, tab);
        if index <= self.active {
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "the active tab is still one of them: active < len"
            )]
            {
                self.active += 1;
            }
        }
        index
    }

    /// Makes the tab at `index` the active one, if there is one there.
    /// Whether that changed which.
    pub fn activate(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() || index == self.active {
            return false;
        }
        self.active = index;
        true
    }

    /// The tab after the active one, the first after the last
    /// (`SelectNextTab`).
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "active < len, and len is a tab at least"
    )]
    pub fn next_index(&self) -> usize {
        (self.active + 1) % self.tabs.len()
    }

    /// The tab before the active one, the last before the first.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "active < len, and len is a tab at least: active + len - 1 is \
                  within twice a Vec's length"
    )]
    pub fn previous_index(&self) -> usize {
        (self.active + self.tabs.len() - 1) % self.tabs.len()
    }

    /// Takes out the tab at `index`, unless it is the only one. Closing
    /// the active tab makes the one to its right active, or else the one
    /// to its left (`DetermineNewSelectedIndex`, with no opener).
    pub fn close(&mut self, index: usize) -> Option<C> {
        if self.tabs.len() <= 1 || index >= self.tabs.len() {
            return None;
        }
        let tab = self.tabs.remove(index);
        if index < self.active || self.active == self.tabs.len() {
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "a tab before it, or it was the last of two or more: active > 0"
            )]
            {
                self.active -= 1;
            }
        }
        Some(tab)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(tabs: &[&'static str], active: usize) -> TabStripModel<&'static str> {
        let mut model = TabStripModel::new(tabs[0]);
        for tab in &tabs[1..] {
            model.append(*tab);
        }
        model.activate(active);
        model
    }

    #[test]
    fn next_and_previous_wrap_around() {
        let model = model(&["a", "b", "c"], 2);
        assert_eq!(model.next_index(), 0);
        assert_eq!(model.previous_index(), 1);
        let model = TabStripModel::new("a");
        assert_eq!((model.next_index(), model.previous_index()), (0, 0));
    }

    #[test]
    fn a_new_tab_goes_last_and_waits_to_be_activated() {
        let mut model = model(&["a", "b"], 0);
        assert_eq!(model.append("c"), 2);
        assert_eq!(*model.active(), "a");
        assert!(model.activate(2));
        assert!(!model.activate(2), "already");
        assert!(!model.activate(3), "no tab there");
        assert_eq!(*model.active(), "c");
    }

    #[test]
    fn closing_the_active_tab_activates_the_one_to_its_right_or_left() {
        let mut model = model(&["a", "b", "c"], 1);
        assert_eq!(model.close(1), Some("b"));
        assert_eq!(*model.active(), "c");
        assert_eq!(model.close(1), Some("c"));
        assert_eq!(*model.active(), "a", "none to its right");
        assert_eq!(model.close(0), None, "the last one stays");
        assert_eq!(model.count(), 1);
    }

    #[test]
    fn a_tab_put_before_the_active_one_keeps_it_active() {
        let mut model = model(&["a", "b"], 1);
        assert_eq!(model.insert(1, "c"), 1);
        assert_eq!(*model.active(), "b");
        assert_eq!(model.insert(9, "d"), 3, "at most the end");
        assert_eq!(*model.active(), "b");
        assert_eq!(
            model.iter().copied().collect::<Vec<_>>(),
            ["a", "c", "b", "d"]
        );
    }

    #[test]
    fn closing_another_tab_keeps_the_active_one() {
        let mut model = model(&["a", "b", "c"], 1);
        assert_eq!(model.close(0), Some("a"));
        assert_eq!(*model.active(), "b");
        assert_eq!(model.close(1), Some("c"));
        assert_eq!(*model.active(), "b");
        assert_eq!(model.close(5), None);
    }
}
