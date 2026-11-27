//! The sad tab (Chromium's `SadTabView`): what a web view shows once its
//! renderer has gone, drawn by the browser, with a button that loads the
//! page again in a new renderer. It knows its size and where the button
//! is; what the button does is the view's. Its text is laid out with the
//! fonts the browser UI's tree lends.

use std::f32::consts::PI;

use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
use gfx::{Color, PointF, RRectF, RectF, SizeF};
use ui::TextContexts;

const TITLE: &str = "This page crashed";
const MESSAGE: &str = "Something went wrong while showing this page.";
const RELOAD: &str = "Reload";

/// Why a renderer has gone, as far as the sad tab tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Crashed,
    /// Its process couldn't enter the operating system's sandbox and
    /// didn't start: another renderer wouldn't either.
    NoSandbox,
    /// Its process ended before it ran any of its own code, at loading
    /// what it is made of (Windows): something other than the system's
    /// may have been put in its way.
    NotLoaded,
    /// It asked for more memory than its sandbox lets it have (Chromium's
    /// out-of-memory sad tab): reloading may run out again.
    OutOfMemory,
}

/// What Windows ends a process with when a DLL it needs isn't found,
/// isn't signed as the process requires, or fails as it starts
/// (`STATUS_DLL_NOT_FOUND`, `STATUS_INVALID_IMAGE_HASH`,
/// `STATUS_DLL_INIT_FAILED`), as exit codes.
const NOT_LOADED: [u32; 3] = [0xC000_0135, 0xC000_0428, 0xC000_0142];

impl Reason {
    /// From how the renderer's process ended, if it has.
    pub fn of(exit: Option<browser::ChildProcessTerminationInfo>) -> Self {
        match exit {
            Some(exit) if exit.out_of_memory => Self::OutOfMemory,
            _ => Self::of_exit(exit.and_then(|exit| exit.status).and_then(|status| status.code())),
        }
    }

    /// From what the renderer's process exited with, if it has.
    pub fn of_exit(code: Option<i32>) -> Self {
        if code == Some(i32::from(crate::renderer_sandbox::NO_SANDBOX_EXIT)) {
            Self::NoSandbox
        } else if code.is_some_and(|code| NOT_LOADED.contains(&code.cast_unsigned())) {
            Self::NotLoaded
        } else {
            Self::Crashed
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Crashed => TITLE,
            Self::NoSandbox | Self::NotLoaded => "This page can't be shown",
            Self::OutOfMemory => "This page ran out of memory",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::Crashed => MESSAGE,
            Self::NoSandbox => "Its renderer couldn't enter the system's sandbox: see the log.",
            Self::NotLoaded => {
                "Its renderer didn't start. If other software loads into every program, try \
                 --allow-third-party-modules."
            }
            Self::OutOfMemory => "It needed more memory than a page is given, and was ended.",
        }
    }
}

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
}

impl SadTab {
    pub fn new() -> Self {
        Self { pressed: false }
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

    /// The sad tab for a web view `size` CSS px whose renderer has gone
    /// for `reason`.
    pub fn paint(
        &mut self,
        text: &mut TextContexts,
        size: SizeF,
        reason: Reason,
    ) -> DisplayItemList {
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
        label(
            text,
            &mut list,
            reason.title(),
            TITLE_SIZE,
            PointF::new(x, Self::title_top(size)),
            TEXT,
        );
        label(
            text,
            &mut list,
            reason.message(),
            MESSAGE_SIZE,
            PointF::new(x, Self::message_top(size)),
            SECONDARY,
        );
        let button = Self::reload_rect(size);
        list.push_op(PaintOp::DrawRRect {
            rrect: RRectF::uniform(button, 4.0),
            flags: PaintFlags::fill(BUTTON_COLOR),
        });
        let layout = text.label(RELOAD, MESSAGE_SIZE);
        let at = PointF::new(
            button.x + ((button.width - layout.width()) / 2.0).round(),
            button.y + ((button.height - layout.height()) / 2.0).round(),
        );
        ui::text::paint_glyphs(&mut list, &layout, at, Color::WHITE);
        list.finalize();
        list
    }
}

impl Default for SadTab {
    fn default() -> Self {
        Self::new()
    }
}

fn label(
    contexts: &mut TextContexts,
    list: &mut DisplayItemList,
    text: &str,
    size: f32,
    at: PointF,
    color: Color,
) {
    let layout = contexts.label(text, size);
    ui::text::paint_glyphs(list, &layout, at, color);
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
    fn a_renderer_that_couldnt_enter_its_sandbox_is_told_apart_from_a_crash() {
        let no_sandbox = i32::from(crate::renderer_sandbox::NO_SANDBOX_EXIT);
        assert_eq!(Reason::of_exit(Some(no_sandbox)), Reason::NoSandbox);
        // Still running, killed by a signal, or exited otherwise.
        assert_eq!(Reason::of_exit(None), Reason::Crashed);
        assert_eq!(Reason::of_exit(Some(4)), Reason::Crashed);
        assert_ne!(Reason::NoSandbox.title(), Reason::Crashed.title());
        assert!(Reason::NoSandbox.message().contains("sandbox"));
        // A process Windows ended at loading its DLLs.
        for status in [0xC000_0135_u32, 0xC000_0428, 0xC000_0142] {
            assert_eq!(
                Reason::of_exit(Some(status.cast_signed())),
                Reason::NotLoaded,
                "{status:#x}"
            );
        }
        // One that crashed is no such thing (a fast fail).
        assert_eq!(
            Reason::of_exit(Some(0xC000_0409_u32.cast_signed())),
            Reason::Crashed
        );
        assert!(
            Reason::NotLoaded
                .message()
                .contains(crate::renderer_sandbox::ALLOW_THIRD_PARTY_MODULES_SWITCH)
        );
        // Each paints its own words: other glyphs.
        let glyphs = |reason| {
            let list = SadTab::new().paint(&mut TextContexts::new(true), SIZE, reason);
            format!("{:?}", list.ops())
        };
        assert_ne!(glyphs(Reason::NoSandbox), glyphs(Reason::Crashed));
    }

    /// A process's status as having exited with `code`.
    fn exited_with(code: u32) -> std::process::ExitStatus {
        #[cfg(windows)]
        {
            use std::os::windows::process::ExitStatusExt as _;
            std::process::ExitStatus::from_raw(code)
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt as _;
            // A wait status: the code is its second byte.
            let code = i32::try_from(code).unwrap().checked_mul(256).unwrap();
            std::process::ExitStatus::from_raw(code)
        }
    }

    #[test]
    fn a_renderer_that_ran_out_of_memory_is_told_apart_whatever_it_exited_with() {
        let ended = |code: Option<u32>, out_of_memory| {
            Reason::of(Some(browser::ChildProcessTerminationInfo {
                status: code.map(exited_with),
                out_of_memory,
            }))
        };
        // Crashed, killed, or exited of what an allocation that failed
        // does (on Windows: a stack that couldn't grow, Rust's abort).
        let mut codes = vec![Some(1), Some(4), None];
        if cfg!(windows) {
            codes.extend([Some(0xC000_00FD), Some(0xC000_0409)]);
        }
        for &code in &codes {
            assert_eq!(ended(code, true), Reason::OutOfMemory, "{code:?}");
            assert_eq!(ended(code, false), Reason::Crashed, "{code:?}");
        }
        let no_sandbox = u32::from(crate::renderer_sandbox::NO_SANDBOX_EXIT);
        assert_eq!(ended(Some(no_sandbox), false), Reason::NoSandbox);
        assert_eq!(Reason::of(None), Reason::Crashed);
        assert!(Reason::OutOfMemory.message().contains("memory"));
        let glyphs = |reason| {
            let list = SadTab::new().paint(&mut TextContexts::new(true), SIZE, reason);
            format!("{:?}", list.ops())
        };
        assert_ne!(glyphs(Reason::OutOfMemory), glyphs(Reason::Crashed));
    }

    #[test]
    fn the_button_reloads_on_down_and_up_on_it() {
        let mut sad = SadTab::new();
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
        let mut sad = SadTab::new();
        let list = sad.paint(&mut TextContexts::new(true), SIZE, Reason::Crashed);
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
