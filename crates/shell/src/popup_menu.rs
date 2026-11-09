//! A collapsed `<select>`'s dropdown, drawn by the browser (Chromium's
//! `PopupMenuHelper` showing an `ExternalPopupMenu`, as on a Mac; the
//! look of Windows' and Linux' `list_picker`): the select's items, one row
//! each, below the select or above it where there is more room, as wide as
//! its widest item or the select. The browser view puts it over everything
//! in the window, gives it the mouse over it and every key while it is
//! open, and tells the renderer how it closed.
//!
//! Transitional, with the rest of the native UI (see `toolbar`).

use base::numerics::{clamp_ceil, clamp_floor, saturated_cast};
use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
use gfx::{Color, PointF, RectF, SizeF};
use parley::{FontContext, LayoutContext, StyleProperty};
use protocol::TabId;
use protocol::renderer::{MenuItem, MenuItemKind, PopupMenuResult};

use crate::ui_text;

/// At most this many rows show; the rest scroll (Chrome's list picker's
/// size).
const MAX_ROWS: usize = 20;
/// Around a row's text (Chrome's list box option's padding).
const PADDING_X: f32 = 4.0;
const PADDING_Y: f32 = 1.0;
/// How far an option in a group is in from its label.
const GROUP_INDENT: f32 = 16.0;
const BORDER: f32 = 1.0;

const BACKGROUND: Color = Color::WHITE;
const BORDER_COLOR: Color = Color::rgb(118, 118, 118);
/// `SelectedItem` and `SelectedItemText`: the highlighted row.
const HIGHLIGHT: Color = Color::rgb(25, 103, 210);
const HIGHLIGHT_TEXT: Color = Color::WHITE;
/// `GrayText`: a disabled item's.
const DISABLED_TEXT: Color = Color::rgb(128, 128, 128);
const SEPARATOR: Color = Color::rgb(204, 204, 204);

/// What a key did to the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    /// It is still open (the highlight may have moved).
    Open,
    /// It closed.
    Closed(PopupMenuResult),
}

pub struct PopupMenu {
    /// The tab whose select it is.
    tab: TabId,
    items: Vec<MenuItem>,
    /// Whether each item is in a group (after a group's label, before the
    /// next one or a separator).
    grouped: Vec<bool>,
    font_size: f32,
    color: Color,
    /// CSS px of the window, its border included.
    rect: RectF,
    row_height: f32,
    rows: usize,
    /// The first item shown.
    scroll: usize,
    /// The item the pointer or the keys are on: an option that can be
    /// picked.
    highlighted: Option<usize>,
    fonts: FontContext,
    layouts: LayoutContext<()>,
}

impl PopupMenu {
    /// `tab`'s select's dropdown, its items in `menu`, below `anchor` (the
    /// select's border box) in a window `window` big, CSS px both.
    /// `test_fonts` as for the toolbar.
    pub fn new(
        tab: TabId,
        menu: protocol::renderer::PopupMenu,
        anchor: RectF,
        window: SizeF,
        test_fonts: bool,
    ) -> Self {
        let mut fonts = ui_text::font_context(test_fonts);
        let mut layouts = LayoutContext::new();
        let mut grouped = Vec::with_capacity(menu.items.len());
        let mut in_group = false;
        for item in &menu.items {
            match item.kind {
                MenuItemKind::Group => in_group = true,
                MenuItemKind::Separator => in_group = false,
                MenuItemKind::Option => {}
            }
            grouped.push(in_group && item.kind == MenuItemKind::Option);
        }
        let probe = ui_text::label(&mut fonts, &mut layouts, "Xy", menu.font_size);
        let row_height = probe.height().ceil() + 2.0 * PADDING_Y;
        let widest = menu
            .items
            .iter()
            .zip(&grouped)
            .map(|(item, grouped)| {
                let layout = label(&mut fonts, &mut layouts, item, menu.font_size);
                layout.width() + if *grouped { GROUP_INDENT } else { 0.0 }
            })
            .fold(0.0f32, f32::max);
        let width = (widest.ceil() + 2.0 * PADDING_X + 2.0 * BORDER).max(anchor.width);
        let rows = menu.items.len().clamp(1, MAX_ROWS);
        let wanted = rows as f32 * row_height + 2.0 * BORDER;
        // Below the select, or above it where there is more room; as high
        // as there is room for.
        let below = (window.height - anchor.bottom()).max(0.0);
        let above = anchor.y.max(0.0);
        let (y, height) = if wanted <= below || below >= above {
            (
                anchor.bottom(),
                wanted.min(below.max(row_height + 2.0 * BORDER)),
            )
        } else {
            let height = wanted.min(above);
            (anchor.y - height, height)
        };
        let rows = clamp_floor::<usize>((height - 2.0 * BORDER) / row_height).clamp(1, rows);
        let height = rows as f32 * row_height + 2.0 * BORDER;
        let x = if menu.right_aligned {
            anchor.right() - width
        } else {
            anchor.x
        };
        let x = x.min(window.width - width).max(0.0);
        let mut popup = Self {
            tab,
            highlighted: menu
                .selected
                .map(|i| i as usize)
                .filter(|i| menu.items.get(*i).is_some_and(pickable)),
            items: menu.items,
            grouped,
            font_size: menu.font_size,
            color: menu.color,
            rect: RectF::new(x, y, width, height),
            row_height,
            rows,
            scroll: 0,
            fonts,
            layouts,
        };
        popup.scroll_to_highlighted();
        popup
    }

    pub fn tab(&self) -> TabId {
        self.tab
    }

    /// Where it is, CSS px of the window.
    pub fn rect(&self) -> RectF {
        self.rect
    }

    pub fn contains(&self, point: PointF) -> bool {
        self.rect.contains(point)
    }

    /// The item highlighted, if any.
    pub fn highlighted(&self) -> Option<usize> {
        self.highlighted
    }

    /// The first item shown.
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// A row's height, CSS px: how far a line of a wheel scrolls.
    pub fn row_height_px(&self) -> f32 {
        self.row_height
    }

    /// The item of the row at `point`, CSS px of the window.
    fn item_at(&self, point: PointF) -> Option<usize> {
        if !self.contains(point) {
            return None;
        }
        let row = ((point.y - self.rect.y - BORDER) / self.row_height).floor();
        // The point is in it: the row is one of its rows, or just past the
        // last, and an item's index at most a row past the last.
        let index = self.scroll.saturating_add(saturated_cast(row.max(0.0)));
        (index < self.items.len() && row < self.rows as f32).then_some(index)
    }

    /// The pointer moved to `point`: the option under it is highlighted.
    /// Whether it shows differently.
    pub fn mouse_move(&mut self, point: PointF) -> bool {
        match self.item_at(point).filter(|i| pickable(&self.items[*i])) {
            Some(index) if self.highlighted != Some(index) => {
                self.highlighted = Some(index);
                true
            }
            _ => false,
        }
    }

    /// The primary button went up at `point`: on an option that can be
    /// picked, it is.
    pub fn mouse_up(&mut self, point: PointF) -> Option<PopupMenuResult> {
        let index = self.item_at(point).filter(|i| pickable(&self.items[*i]))?;
        // The renderer numbered the items with u32s.
        Some(PopupMenuResult::Accepted {
            index: u32::try_from(index).ok()?,
            by_keyboard: false,
        })
    }

    /// A wheel over it, `dy` CSS px down: the rows scroll, a row for each
    /// row's height or part of one. Whether it shows differently.
    pub fn wheel(&mut self, dy: f32) -> bool {
        let rows = clamp_ceil::<usize>(dy.abs() / self.row_height).max(1);
        let before = self.scroll;
        let last = self.items.len().saturating_sub(self.rows);
        // A wheel far past the end (as many rows as a usize holds) scrolls
        // to it.
        self.scroll = if dy > 0.0 {
            self.scroll.saturating_add(rows).min(last)
        } else {
            self.scroll.saturating_sub(rows)
        };
        self.scroll != before
    }

    /// A key went down (Chrome's list picker's): ↑ and ↓ highlight the
    /// option before and after, PageUp and PageDown a page away, Home and
    /// End the first and the last; Enter, Tab, Alt+↑, Alt+↓ and F4 pick the
    /// highlighted one; Escape closes it with none. Any other key does
    /// nothing (no type-ahead).
    pub fn key(&mut self, key: &str, alt: bool) -> KeyOutcome {
        // The renderer numbered the items with u32s.
        let picked = |popup: &Self| match popup.highlighted.map(u32::try_from) {
            Some(Ok(index)) => PopupMenuResult::Accepted {
                index,
                by_keyboard: true,
            },
            _ => PopupMenuResult::Canceled { by_press: false },
        };
        let page = self.rows.saturating_sub(1).max(1);
        let next = match key {
            "ArrowDown" | "ArrowUp" if alt => return KeyOutcome::Closed(picked(self)),
            "Enter" | "Tab" | "F4" => return KeyOutcome::Closed(picked(self)),
            "Escape" => return KeyOutcome::Closed(PopupMenuResult::Canceled { by_press: false }),
            // An item's index, one past it at most.
            "ArrowDown" => {
                self.pickable_from(self.highlighted.map_or(0, |i| i.saturating_add(1)), 1, true)
            }
            "ArrowUp" => self
                .highlighted
                .and_then(|i| self.pickable_from(i.checked_sub(1)?, 1, false)),
            "PageDown" => self.pickable_from(
                self.highlighted.map_or(0, |i| i.saturating_add(1)),
                page,
                true,
            ),
            "PageUp" => self
                .highlighted
                .and_then(|i| self.pickable_from(i.checked_sub(1)?, page, false)),
            "Home" => self.pickable_from(0, 1, true),
            "End" => self.pickable_from(self.items.len().saturating_sub(1), 1, false),
            _ => None,
        };
        if let Some(next) = next {
            self.highlighted = Some(next);
            self.scroll_to_highlighted();
        }
        KeyOutcome::Open
    }

    /// The `count`th option that can be picked from `start` (itself the
    /// first) going forwards or backwards, or the last one there is.
    fn pickable_from(&self, start: usize, count: usize, forwards: bool) -> Option<usize> {
        let indices: Box<dyn Iterator<Item = usize>> = if forwards {
            Box::new(start..self.items.len())
        } else {
            Box::new((0..=start.min(self.items.len().saturating_sub(1))).rev())
        };
        let mut found = None;
        for (n, index) in indices
            .filter(|i| self.items.get(*i).is_some_and(pickable))
            .enumerate()
        {
            found = Some(index);
            // The `count`th: `n + 1 >= count`.
            if n >= count.saturating_sub(1) {
                break;
            }
        }
        found
    }

    /// The highlighted row scrolled into view.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "scroll is the last page's first row at most, so scroll + rows is an \
                  item's count; an index past that is rows or more"
    )]
    fn scroll_to_highlighted(&mut self) {
        let Some(index) = self.highlighted else {
            return;
        };
        if index < self.scroll {
            self.scroll = index;
        } else if index >= self.scroll + self.rows {
            self.scroll = index + 1 - self.rows;
        }
    }

    /// Its picture, from its top left.
    pub fn paint(&mut self) -> DisplayItemList {
        let mut list = DisplayItemList::new();
        let size = self.rect.size();
        fill(&mut list, RectF::from_size(size), BORDER_COLOR);
        fill(
            &mut list,
            RectF::new(
                BORDER,
                BORDER,
                size.width - 2.0 * BORDER,
                size.height - 2.0 * BORDER,
            ),
            BACKGROUND,
        );
        let end = self.scroll.saturating_add(self.rows).min(self.items.len());
        for (shown, index) in (self.scroll..end).enumerate() {
            let row = RectF::new(
                BORDER,
                BORDER + shown as f32 * self.row_height,
                size.width - 2.0 * BORDER,
                self.row_height,
            );
            let item = &self.items[index];
            if item.kind == MenuItemKind::Separator {
                let y = (row.y + row.height / 2.0).floor();
                fill(
                    &mut list,
                    RectF::new(row.x + PADDING_X, y, row.width - 2.0 * PADDING_X, 1.0),
                    SEPARATOR,
                );
                continue;
            }
            let highlighted = self.highlighted == Some(index);
            if highlighted {
                fill(&mut list, row, HIGHLIGHT);
            }
            let color = if highlighted {
                HIGHLIGHT_TEXT
            } else if item.kind == MenuItemKind::Option && !item.enabled {
                DISABLED_TEXT
            } else {
                self.color
            };
            let indent = if self.grouped[index] {
                GROUP_INDENT
            } else {
                0.0
            };
            let layout = label(&mut self.fonts, &mut self.layouts, item, self.font_size);
            let top = row.y + ((row.height - layout.height()) / 2.0).round();
            ui_text::paint_glyphs(
                &mut list,
                &layout,
                PointF::new(row.x + PADDING_X + indent, top),
                color,
            );
        }
        list.finalize();
        list
    }
}

/// Whether the list's keys and the pointer can stop on `item`: an option
/// that can be picked.
fn pickable(item: &MenuItem) -> bool {
    item.kind == MenuItemKind::Option && item.enabled
}

/// An item's label on one line: a group's in bold.
fn label(
    fonts: &mut FontContext,
    layouts: &mut LayoutContext<()>,
    item: &MenuItem,
    size: f32,
) -> parley::Layout<()> {
    let text = &item.label;
    let mut builder = layouts.ranged_builder(fonts, text, 1.0, true);
    builder.push_default(ui_text::font_family());
    builder.push_default(StyleProperty::FontSize(size));
    if item.kind == MenuItemKind::Group {
        builder.push_default(StyleProperty::FontWeight(parley::FontWeight::BOLD));
    }
    let mut layout = builder.build(text);
    layout.break_all_lines(None);
    layout
}

fn fill(list: &mut DisplayItemList, rect: RectF, color: Color) {
    list.push_op(PaintOp::DrawRect {
        rect,
        flags: PaintFlags::fill(color),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, kind: MenuItemKind, enabled: bool) -> MenuItem {
        MenuItem {
            label: label.to_owned(),
            kind,
            enabled,
        }
    }

    fn option(label: &str) -> MenuItem {
        item(label, MenuItemKind::Option, true)
    }

    fn menu(items: Vec<MenuItem>, selected: Option<u32>) -> protocol::renderer::PopupMenu {
        protocol::renderer::PopupMenu {
            bounds: RectF::new(0.0, 0.0, 0.0, 0.0),
            font_size: 13.0,
            color: Color::BLACK,
            items,
            selected,
            right_aligned: false,
        }
    }

    const WINDOW: SizeF = SizeF::new(800.0, 600.0);

    fn popup(items: Vec<MenuItem>, selected: Option<u32>, anchor: RectF) -> PopupMenu {
        PopupMenu::new(TabId(1), menu(items, selected), anchor, WINDOW, true)
    }

    fn accepted(index: u32, by_keyboard: bool) -> PopupMenuResult {
        PopupMenuResult::Accepted { index, by_keyboard }
    }

    /// Below the select, at its left, as wide as it at least; above it
    /// where there is more room there, as high as there is room for; at
    /// most 20 rows; kept in the window sideways.
    #[test]
    fn it_goes_below_or_above_the_select() {
        let anchor = RectF::new(100.0, 50.0, 300.0, 20.0);
        let three = vec![option("a"), option("b"), option("c")];
        let p = popup(three.clone(), None, anchor);
        let rect = p.rect();
        assert_eq!((rect.x, rect.y, rect.width), (100.0, 70.0, 300.0));
        assert_eq!(rect.height, 3.0 * p.row_height_px() + 2.0);

        let low = RectF::new(100.0, 560.0, 50.0, 20.0);
        let p = popup(three.clone(), None, low);
        assert_eq!(p.rect().bottom(), 560.0, "above it");
        assert!(p.rect().width > 50.0 - 1.0);

        let many: Vec<MenuItem> = (0..30).map(|i| option(&format!("item {i}"))).collect();
        let p = popup(many.clone(), None, anchor);
        assert_eq!(p.rect().height, 20.0 * p.row_height_px() + 2.0);
        // Where only part fits: as many rows as there is room for.
        let middle = RectF::new(100.0, 290.0, 50.0, 20.0);
        let p = popup(many, None, middle);
        assert!(p.rect().y >= 0.0 && p.rect().bottom() <= 600.0);

        let right = RectF::new(780.0, 50.0, 300.0, 20.0);
        let p = popup(three, None, right);
        assert_eq!(p.rect().right(), 800.0, "in the window");
    }

    /// The keys: the selected option is highlighted first; ↓ and ↑ go to
    /// the next and the one before that can be picked (not a disabled
    /// one, a group's label or a separator), Home and End to the ends,
    /// PageDown and PageUp a page; Enter, Tab, F4 and Alt+↓ pick it (with
    /// none highlighted, a cancel), Escape cancels; other keys do nothing.
    #[test]
    fn its_keys() {
        let items = vec![
            option("a"),
            item("G", MenuItemKind::Group, true),
            item("b", MenuItemKind::Option, false),
            option("c"),
            item("", MenuItemKind::Separator, false),
            option("d"),
        ];
        let anchor = RectF::new(0.0, 0.0, 50.0, 20.0);
        let mut p = popup(items.clone(), Some(0), anchor);
        assert_eq!(p.highlighted(), Some(0));
        assert_eq!(p.key("ArrowDown", false), KeyOutcome::Open);
        assert_eq!(p.highlighted(), Some(3));
        p.key("ArrowDown", false);
        assert_eq!(p.highlighted(), Some(5));
        p.key("ArrowDown", false);
        assert_eq!(p.highlighted(), Some(5), "nothing after");
        p.key("ArrowUp", false);
        assert_eq!(p.highlighted(), Some(3));
        p.key("Home", false);
        assert_eq!(p.highlighted(), Some(0));
        p.key("End", false);
        assert_eq!(p.highlighted(), Some(5));
        p.key("PageUp", false);
        assert_eq!(p.highlighted(), Some(0));
        p.key("PageDown", false);
        assert_eq!(p.highlighted(), Some(5));
        p.key("a", false);
        assert_eq!(p.highlighted(), Some(5));
        for (key, alt) in [
            ("Enter", false),
            ("Tab", false),
            ("F4", false),
            ("ArrowDown", true),
        ] {
            assert_eq!(
                p.key(key, alt),
                KeyOutcome::Closed(accepted(5, true)),
                "{key}"
            );
        }
        assert_eq!(
            p.key("Escape", false),
            KeyOutcome::Closed(PopupMenuResult::Canceled { by_press: false })
        );
        // A disabled option selected isn't highlighted.
        let mut p = popup(items, Some(2), anchor);
        assert_eq!(p.highlighted(), None);
        assert_eq!(
            p.key("Enter", false),
            KeyOutcome::Closed(PopupMenuResult::Canceled { by_press: false })
        );
    }

    /// The pointer highlights the option under it (not a disabled one); a
    /// release on one picks it, by the mouse; outside, or on one that
    /// can't be picked, nothing.
    #[test]
    fn its_mouse() {
        let items = vec![
            option("a"),
            item("b", MenuItemKind::Option, false),
            option("c"),
        ];
        let anchor = RectF::new(10.0, 10.0, 50.0, 20.0);
        let mut p = popup(items, None, anchor);
        let row = p.row_height_px();
        let at = |n: f32| PointF::new(20.0, 30.0 + 1.0 + row * n + row / 2.0);
        assert!(p.mouse_move(at(2.0)));
        assert_eq!(p.highlighted(), Some(2));
        assert!(!p.mouse_move(at(1.0)), "a disabled one");
        assert_eq!(p.highlighted(), Some(2));
        assert_eq!(p.mouse_up(at(0.0)), Some(accepted(0, false)));
        assert_eq!(p.mouse_up(at(1.0)), None);
        assert_eq!(p.mouse_up(PointF::new(500.0, 500.0)), None);
    }

    /// A wheel over it scrolls its rows, up to the last, however far it
    /// goes; the keys scroll the highlighted row into view.
    #[test]
    fn its_rows_scroll() {
        let items: Vec<MenuItem> = (0..30).map(|i| option(&format!("item {i}"))).collect();
        let anchor = RectF::new(0.0, 0.0, 50.0, 20.0);
        let mut p = popup(items.clone(), None, anchor);
        assert!(p.wheel(p.row_height_px() * 3.0));
        assert_eq!(p.scroll(), 3);
        assert!(p.wheel(10_000.0));
        assert_eq!(p.scroll(), 10, "the last 20 rows");
        assert!(!p.wheel(10_000.0));
        assert!(p.wheel(-1.0));
        assert_eq!(p.scroll(), 9);
        // As many rows as a usize holds, or more: to the last, too.
        assert!(p.wheel(f32::MAX));
        assert_eq!(p.scroll(), 10);
        assert!(p.wheel(-1.0));
        assert!(p.wheel(f32::INFINITY));
        assert_eq!(p.scroll(), 10);
        let mut p = popup(items, Some(25), anchor);
        assert_eq!(p.scroll(), 6, "the selected one shown");
        p.key("Home", false);
        assert_eq!(p.scroll(), 0);
    }

    /// Its picture: the border, the rows' text, the highlighted row.
    #[test]
    fn it_paints_its_rows() {
        let anchor = RectF::new(0.0, 0.0, 50.0, 20.0);
        let mut p = popup(vec![option("a"), option("b")], Some(1), anchor);
        let list = p.paint();
        let ops: Vec<&PaintOp> = list.ops().iter().collect();
        let fills: Vec<Color> = ops
            .iter()
            .filter_map(|op| match op {
                PaintOp::DrawRect { flags, .. } => Some(flags.color),
                _ => None,
            })
            .collect();
        assert_eq!(fills, [BORDER_COLOR, BACKGROUND, HIGHLIGHT]);
        let glyphs = ops
            .iter()
            .filter(|op| matches!(op, PaintOp::DrawGlyphs { .. }))
            .count();
        assert_eq!(glyphs, 2);
    }
}
