//! The sad tab (Chromium's `SadTabView`): what a web view shows once its
//! renderer has gone, drawn by the browser, with a button that loads the
//! page again in a new renderer. It knows its size and where the button
//! is; what the button does is the view's.
//!
//! Transitional, with the rest of the native UI (see `toolbar`).

use std::f32::consts::PI;

use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
use gfx::{Color, PointF, RRectF, RectF, SizeF};
use parley::{FontContext, LayoutContext};

use crate::ui_text;

const TITLE: &str = "This page crashed";
const MESSAGE: &str = "Something went wrong while showing this page.";
const RELOAD: &str = "Reload";

/// Where the content starts: a column at most this wide, centered, a
/// quarter of the way down.
const COLUMN: f32 = 400.0;
const MARGIN: f32 = 24.0;
const ICON: f32 = 48.0;
const TITLE_SIZE: f32 = 20.0;
const MESSAGE_SIZE: f32 = 14.0;
const BUTTON: SizeF = SizeF::new(88.0, 32.0);

pub const BACKGROUND: Color = Color::rgb(248, 249, 250);
const TEXT: Color = Color::rgb(32, 33, 36);
const SECONDARY: Color = Color::rgb(95, 99, 104);
const BUTTON_COLOR: Color = Color::rgb(26, 115, 232);

pub struct SadTab {
    /// The primary button went down on the reload button: going up on it
    /// too reloads.
    pressed: bool,
    fonts: FontContext,
    layouts: LayoutContext<()>,
}

impl SadTab {
    /// `test_fonts` as for the toolbar.
    pub fn new(test_fonts: bool) -> Self {
        Self {
            pressed: false,
            fonts: ui_text::font_context(test_fonts),
            layouts: LayoutContext::new(),
        }
    }

    /// Where the content's column starts in a web view `size` CSS px.
    fn origin(size: SizeF) -> PointF {
        PointF::new(
            ((size.width - COLUMN) / 2.0).max(MARGIN),
            (size.height / 4.0).max(MARGIN),
        )
    }

    fn icon_rect(size: SizeF) -> RectF {
        let at = Self::origin(size);
        RectF::new(at.x, at.y, ICON, ICON)
    }

    fn title_top(size: SizeF) -> f32 {
        Self::origin(size).y + ICON + 16.0
    }

    fn message_top(size: SizeF) -> f32 {
        Self::title_top(size) + TITLE_SIZE + 16.0
    }

    /// The reload button, in a web view `size` CSS px.
    pub fn reload_rect(size: SizeF) -> RectF {
        let at = Self::origin(size);
        RectF::new(
            at.x,
            Self::message_top(size) + MESSAGE_SIZE + 28.0,
            BUTTON.width,
            BUTTON.height,
        )
    }

    /// The primary button went down at `point` in a web view `size` CSS px.
    pub fn mouse_down(&mut self, point: PointF, size: SizeF) {
        self.pressed = Self::reload_rect(size).contains(point);
    }

    /// The primary button went up at `point`: whether that pressed the
    /// reload button, down and up on it.
    pub fn mouse_up(&mut self, point: PointF, size: SizeF) -> bool {
        std::mem::take(&mut self.pressed) && Self::reload_rect(size).contains(point)
    }

    /// The sad tab for a web view `size` CSS px.
    pub fn paint(&mut self, size: SizeF) -> DisplayItemList {
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::from_size(size),
            flags: PaintFlags::fill(BACKGROUND),
        });
        let icon = Self::icon_rect(size);
        list.push_op(PaintOp::DrawPath {
            path: gfx::Path::polygons(sad_face(PointF::new(
                icon.x + icon.width / 2.0,
                icon.y + icon.height / 2.0,
            ))),
            flags: PaintFlags::fill(SECONDARY),
        });
        let x = Self::origin(size).x;
        self.label(
            &mut list,
            TITLE,
            TITLE_SIZE,
            PointF::new(x, Self::title_top(size)),
            TEXT,
        );
        self.label(
            &mut list,
            MESSAGE,
            MESSAGE_SIZE,
            PointF::new(x, Self::message_top(size)),
            SECONDARY,
        );
        let button = Self::reload_rect(size);
        list.push_op(PaintOp::DrawRRect {
            rrect: RRectF::uniform(button, 4.0),
            flags: PaintFlags::fill(BUTTON_COLOR),
        });
        let layout = ui_text::label(&mut self.fonts, &mut self.layouts, RELOAD, MESSAGE_SIZE);
        let at = PointF::new(
            button.x + ((button.width - layout.width()) / 2.0).round(),
            button.y + ((button.height - layout.height()) / 2.0).round(),
        );
        ui_text::paint_glyphs(&mut list, &layout, at, Color::WHITE);
        list.finalize();
        list
    }

    fn label(
        &mut self,
        list: &mut DisplayItemList,
        text: &str,
        size: f32,
        at: PointF,
        color: Color,
    ) {
        let layout = ui_text::label(&mut self.fonts, &mut self.layouts, text, size);
        ui_text::paint_glyphs(list, &layout, at, color);
    }
}

/// A frowning face around `c`, as filled polygons: a ring, two eyes and a
/// mouth curving down.
fn sad_face(c: PointF) -> Vec<Vec<PointF>> {
    let arc = |radius: f32, width: f32, start: f32, sweep: f32, center: PointF| {
        let steps = 32;
        let at = |r: f32, i: usize| {
            let a = start + sweep * i as f32 / steps as f32;
            PointF::new(center.x + r * a.cos(), center.y + r * a.sin())
        };
        let mut band: Vec<PointF> = (0..=steps).map(|i| at(radius + width / 2.0, i)).collect();
        band.extend((0..=steps).rev().map(|i| at(radius - width / 2.0, i)));
        band
    };
    let eye = |dx: f32| {
        let (x, y) = (c.x + dx, c.y - 6.0);
        vec![
            PointF::new(x - 2.5, y - 2.5),
            PointF::new(x + 2.5, y - 2.5),
            PointF::new(x + 2.5, y + 2.5),
            PointF::new(x - 2.5, y + 2.5),
        ]
    };
    // The mouth: the top of a circle below the face's center.
    let mouth = arc(10.0, 3.0, PI * 1.2, PI * 0.6, PointF::new(c.x, c.y + 14.0));
    vec![arc(22.0, 3.0, 0.0, 2.0 * PI, c), eye(-8.0), eye(8.0), mouth]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: SizeF = SizeF::new(800.0, 600.0);

    fn center(rect: RectF) -> PointF {
        PointF::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
    }

    #[test]
    fn the_content_is_a_centered_column_a_quarter_down() {
        assert_eq!(SadTab::origin(SIZE), PointF::new(200.0, 150.0));
        // Narrow or short: the margins.
        assert_eq!(
            SadTab::origin(SizeF::new(300.0, 40.0)),
            PointF::new(MARGIN, MARGIN)
        );
        let button = SadTab::reload_rect(SIZE);
        assert!(button.y > SadTab::message_top(SIZE) + MESSAGE_SIZE);
        assert_eq!(button.x, 200.0);
    }

    #[test]
    fn the_button_reloads_on_down_and_up_on_it() {
        let mut sad = SadTab::new(true);
        let button = center(SadTab::reload_rect(SIZE));
        let elsewhere = PointF::new(10.0, 10.0);
        sad.mouse_down(button, SIZE);
        assert!(sad.mouse_up(button, SIZE));
        assert!(!sad.mouse_up(button, SIZE), "once");
        sad.mouse_down(button, SIZE);
        assert!(!sad.mouse_up(elsewhere, SIZE), "moved off");
        sad.mouse_down(elsewhere, SIZE);
        assert!(!sad.mouse_up(button, SIZE), "not down on it");
    }

    #[test]
    fn it_paints_the_page_the_face_the_words_and_the_button() {
        let mut sad = SadTab::new(true);
        let list = sad.paint(SIZE);
        let ops = list.ops();
        assert!(matches!(
            ops.first(),
            Some(PaintOp::DrawRect { rect, flags }) if *rect == RectF::from_size(SIZE) && flags.color == BACKGROUND
        ));
        let glyph_runs = ops
            .iter()
            .filter(|op| matches!(op, PaintOp::DrawGlyphs { .. }))
            .count();
        assert!(glyph_runs >= 3, "the title, the message and the button's");
        let button = SadTab::reload_rect(SIZE);
        assert!(ops.iter().any(|op| matches!(
            op,
            PaintOp::DrawRRect { rrect, flags } if rrect.rect == button && flags.color == BUTTON_COLOR
        )));
        assert!(ops.iter().any(|op| matches!(op, PaintOp::DrawPath { .. })));
    }
}
