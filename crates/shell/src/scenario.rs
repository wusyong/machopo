//! Scenarios: what a person would do in a browser window and check, one
//! command a line, run against a window by [`automation`](crate::automation).
//! The format is described for authors in `tests/scenarios/README.md`.
//!
//! ```text
//! # A comment.
//! window scale=2
//! navigate about:demo
//! wait-loaded
//! click #clicker
//! wait-text #count "Clicked 1 time (onclick)."
//! ```

use std::time::Duration;

use gfx::PointF;
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// A scenario: the window it wants, and what to do in it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scenario {
    pub window: WindowSettings,
    pub steps: Vec<Step>,
}

/// What `window` lines ask of the window; `None` leaves it as the runner
/// has it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WindowSettings {
    /// Device pixels per CSS px.
    pub scale: Option<f32>,
    /// CSS px.
    pub size: Option<(f32, f32)>,
    /// `RendererFlags::caret_blink_interval`: `caret-blink=<ms>`; the
    /// caret doesn't blink without it.
    pub caret_blink: Option<std::time::Duration>,
}

impl Scenario {
    /// Whether it can go differently with macOS's keyboard shortcuts than
    /// with the others': it presses a key (`key`, `type`), whose meaning
    /// the shortcuts decide, or has a `platform` block. Nothing else the
    /// window does depends on them, so a scenario without either runs the
    /// same both ways.
    pub fn depends_on_shortcuts(&self) -> bool {
        self.steps
            .iter()
            .any(|step| matches!(step.command, Command::Key { .. } | Command::Platform(_)))
    }
}

/// Whose keyboard shortcuts the window uses: what a `platform` block's
/// steps run for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Mac,
    /// Windows and Linux.
    Other,
    All,
}

/// One command, and the line it is on (from 1).
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub line: usize,
    pub command: Command,
}

/// Where a pointer command goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// The center of the first element matching the selector.
    Element(String),
    /// A point, CSS px of the window.
    Point(PointF),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// As if typed in the address field: a URL, or a path relative to the
    /// scenario's file.
    Navigate(String),
    /// Until the latest navigation started has loaded.
    WaitLoaded,
    /// The primary button down and up.
    Click(Target),
    /// Twice, the second the double click (its `click_count` 2).
    DoubleClick(Target),
    /// A wheel at the target, scrolling by `(dx, dy)` CSS px.
    Wheel { target: Target, dx: f32, dy: f32 },
    /// A key with modifiers held, and with Cmd on macOS or Ctrl elsewhere
    /// if `primary`; `text` is what it types, if anything.
    Key {
        key: Key,
        modifiers: ModifiersState,
        primary: bool,
        text: Option<String>,
    },
    /// The steps after it, up to the next `platform`, run only with that
    /// platform's shortcuts.
    Platform(Platform),
    /// Until the element's `textContent` is `text`.
    WaitText { selector: String, text: String },
    /// Until the active tab's title, as its tab shows it (its URL while
    /// the page has none), is this (Chromium's `TitleWatcher`).
    WaitTitle(String),
    /// Until the element's text or box is other than when the wait began,
    /// or, right after input, than before the input.
    WaitChange(String),
    /// Scrolls the element into view if its center is out of it, as a
    /// link to `#fragment` would (its top at the top of the page), and
    /// waits until that is drawn: before a `capture` of what is below the
    /// window. A `click` or `wheel` at an element does it first too.
    ScrollTo(String),
    /// Until the renderer's main thread is busy: a query goes unanswered
    /// for a while.
    WaitBusy,
    /// The pointer moves to the target, no button down.
    Hover(Target),
    /// Until the pointer looks as the CSS keyword says, where it is.
    WaitCursor(String),
    /// Keeps a frame by name: one showing what the page has changed so far,
    /// or, `now`, the next drawn, while script keeps the page busy.
    Capture { name: String, now: bool },
    /// Until a frame differs from the one captured as `name`: all of it, or
    /// within the element's box.
    ExpectDiffers {
        name: String,
        selector: Option<String>,
    },
    /// Saves a frame as `<name>.png`, as `Capture` picks it.
    Screenshot { name: String, now: bool },
    /// The window's size, CSS px.
    Resize { width: f32, height: f32 },
    /// How long each later wait may take.
    Timeout(Duration),
    /// Which document later selectors pick in: the child frame of the
    /// first iframe matching the selector in the one picked so far, or,
    /// `None`, the document shown again.
    Frame(Option<String>),
}

/// What is wrong with a scenario's text.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// The scenario `text` is; empty lines and `#` comments are skipped.
pub fn parse(text: &str) -> Result<Scenario, ParseError> {
    let mut scenario = Scenario::default();
    for (line, line_number) in text.lines().zip(1..) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let error = |message| ParseError {
            line: line_number,
            message,
        };
        if name == "window" {
            if !scenario.steps.is_empty() {
                return Err(error("`window` comes before the other commands".into()));
            }
            window(rest.trim(), &mut scenario.window).map_err(error)?;
            continue;
        }
        let command = parse_command(name, rest.trim()).map_err(error)?;
        scenario
            .steps
            .extend(command.into_iter().map(|command| Step {
                line: line_number,
                command,
            }));
    }
    Ok(scenario)
}

/// `window` settings: `scale=<n>`, `size=<w>x<h>`, `caret-blink=<ms>`.
fn window(rest: &str, settings: &mut WindowSettings) -> Result<(), String> {
    if rest.is_empty() {
        return Err("`window` needs settings, like `scale=2`".into());
    }
    for setting in rest.split_whitespace() {
        let (name, value) = setting
            .split_once('=')
            .ok_or_else(|| format!("{setting:?} isn't `name=value`"))?;
        let positive = |text: &str| {
            text.parse::<f32>()
                .ok()
                .filter(|n| n.is_finite() && *n > 0.0)
                .ok_or_else(|| format!("{text:?} isn't a positive number"))
        };
        match name {
            "scale" => settings.scale = Some(positive(value)?),
            "size" => {
                let (width, height) = value
                    .split_once('x')
                    .ok_or_else(|| format!("`size` is `<width>x<height>`, not {value:?}"))?;
                settings.size = Some((positive(width)?, positive(height)?));
            }
            "caret-blink" => {
                let ms = positive(value)?;
                settings.caret_blink = Some(std::time::Duration::from_secs_f32(ms / 1000.0));
            }
            _ => {
                return Err(format!(
                    "unknown setting `{name}`: script, scale, size or caret-blink"
                ));
            }
        }
    }
    Ok(())
}

/// One line's commands (`type` is one key each).
fn parse_command(name: &str, rest: &str) -> Result<Vec<Command>, String> {
    let nonempty = |what: &str| {
        if rest.is_empty() {
            Err(format!("`{name}` needs {what}"))
        } else {
            Ok(rest.to_owned())
        }
    };
    let no_arguments = || {
        if rest.is_empty() {
            Ok(())
        } else {
            Err(format!("`{name}` takes no arguments"))
        }
    };
    let command = match name {
        "navigate" => Command::Navigate(nonempty("a URL or a path")?),
        "wait-loaded" => {
            no_arguments()?;
            Command::WaitLoaded
        }
        "click" => Command::Click(Target::Element(nonempty("a selector")?)),
        "click-at" => {
            let [x, y] = numbers(rest)?;
            Command::Click(Target::Point(PointF::new(x, y)))
        }
        "hover" => Command::Hover(Target::Element(nonempty("a selector")?)),
        "hover-at" => {
            let [x, y] = numbers(rest)?;
            Command::Hover(Target::Point(PointF::new(x, y)))
        }
        "wait-cursor" => {
            if !CURSORS.contains(&rest) {
                return Err(format!(
                    "`wait-cursor` needs a cursor's keyword (`default`, `pointer`, `text`...), not {rest:?}"
                ));
            }
            Command::WaitCursor(rest.to_owned())
        }
        "dblclick" => Command::DoubleClick(Target::Element(nonempty("a selector")?)),
        "dblclick-at" => {
            let [x, y] = numbers(rest)?;
            Command::DoubleClick(Target::Point(PointF::new(x, y)))
        }
        "wheel" => {
            let (selector, [dx, dy]) = trailing_numbers(rest)?;
            if selector.is_empty() {
                return Err("`wheel` needs a selector".into());
            }
            Command::Wheel {
                target: Target::Element(selector.to_owned()),
                dx,
                dy,
            }
        }
        "wheel-at" => {
            let [x, y, dx, dy] = numbers(rest)?;
            Command::Wheel {
                target: Target::Point(PointF::new(x, y)),
                dx,
                dy,
            }
        }
        "key" => key(rest)?,
        "type" => {
            let text = quoted(rest)?;
            return Ok(text.chars().map(typed).collect());
        }
        "wait-text" => {
            let (selector, text) = trailing_quoted(rest)?;
            if selector.is_empty() {
                return Err("`wait-text` needs a selector".into());
            }
            Command::WaitText {
                selector: selector.to_owned(),
                text,
            }
        }
        "wait-title" => Command::WaitTitle(quoted(rest)?),
        "wait-change" => Command::WaitChange(nonempty("a selector")?),
        "scroll-to" => Command::ScrollTo(nonempty("a selector")?),
        "wait-busy" => {
            no_arguments()?;
            Command::WaitBusy
        }
        "platform" => Command::Platform(match rest {
            "mac" => Platform::Mac,
            "other" => Platform::Other,
            "all" => Platform::All,
            _ => {
                return Err(format!(
                    "`platform` is `mac`, `other` or `all`, not {rest:?}"
                ));
            }
        }),
        "capture" => {
            let (name, now) = frame_argument(rest)?;
            Command::Capture { name, now }
        }
        "expect-differs" => {
            let (name, selector) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let selector = selector.trim();
            Command::ExpectDiffers {
                name: name_argument(name)?,
                selector: (!selector.is_empty()).then(|| selector.to_owned()),
            }
        }
        "screenshot" => {
            let (name, now) = frame_argument(rest)?;
            Command::Screenshot { name, now }
        }
        "resize" => {
            let [width, height] = numbers(rest)?;
            if width < 1.0 || height < 1.0 {
                return Err("`resize` needs a size of at least 1×1".into());
            }
            Command::Resize { width, height }
        }
        "timeout" => {
            let [seconds] = numbers(rest)?;
            if seconds <= 0.0 {
                return Err("`timeout` needs a positive number of seconds".into());
            }
            Command::Timeout(Duration::from_secs_f32(seconds))
        }
        "frame" => match nonempty("a selector or `top`")?.as_str() {
            "top" => Command::Frame(None),
            selector => Command::Frame(Some(selector.to_owned())),
        },
        _ => return Err(format!("unknown command `{name}`")),
    };
    Ok(vec![command])
}

/// The pointers `wait-cursor` tells apart, by their CSS keywords
/// (`all-scroll` looks as `move` does, `auto` is one of the others).
const CURSORS: [&str; 34] = [
    "default",
    "none",
    "context-menu",
    "help",
    "pointer",
    "progress",
    "wait",
    "cell",
    "crosshair",
    "text",
    "vertical-text",
    "alias",
    "copy",
    "move",
    "no-drop",
    "not-allowed",
    "grab",
    "grabbing",
    "e-resize",
    "n-resize",
    "ne-resize",
    "nw-resize",
    "s-resize",
    "se-resize",
    "sw-resize",
    "w-resize",
    "ew-resize",
    "ns-resize",
    "nesw-resize",
    "nwse-resize",
    "col-resize",
    "row-resize",
    "zoom-in",
    "zoom-out",
];

/// `<name> [now]`, for a capture or a screenshot.
fn frame_argument(rest: &str) -> Result<(String, bool), String> {
    match rest.split_once(char::is_whitespace) {
        None => Ok((name_argument(rest)?, false)),
        Some((name, "now")) => Ok((name_argument(name)?, true)),
        Some((_, other)) => Err(format!(
            "after the name comes `now` or nothing, not {:?}",
            other.trim()
        )),
    }
}

/// A name for a capture or a screenshot: a file name, so no path.
fn name_argument(rest: &str) -> Result<String, String> {
    let valid = !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !rest.starts_with('.');
    if valid {
        Ok(rest.to_owned())
    } else {
        Err(format!(
            "{rest:?} isn't a name: letters, digits, `-`, `_` and `.` only"
        ))
    }
}

fn numbers<const N: usize>(rest: &str) -> Result<[f32; N], String> {
    let parsed: Vec<f32> = rest
        .split_whitespace()
        .map(|word| {
            word.parse::<f32>()
                .ok()
                .filter(|n| n.is_finite())
                .ok_or_else(|| format!("{word:?} isn't a number"))
        })
        .collect::<Result<_, _>>()?;
    parsed
        .try_into()
        .map_err(|parsed: Vec<f32>| format!("needs {N} numbers, not {}", parsed.len()))
}

/// `<anything> <n1> … <nN>`: what comes before the last `N` words, which
/// are numbers.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "a white space's index plus its length is the string's at most"
)]
fn trailing_numbers<const N: usize>(rest: &str) -> Result<(&str, [f32; N]), String> {
    let mut end = rest.len();
    for _ in 0..N {
        // After the white space before the number, which may be more than
        // a byte (U+3000).
        end = rest[..end]
            .trim_end()
            .char_indices()
            .rfind(|(_, c)| c.is_whitespace())
            .map_or(0, |(space, c)| space + c.len_utf8());
    }
    Ok((rest[..end].trim(), numbers(&rest[end..])?))
}

/// A string in double quotes, with `\"` and `\\` escapes, that is all of
/// `text`.
fn quoted(text: &str) -> Result<String, String> {
    let inner = text
        .strip_prefix('"')
        .ok_or_else(|| format!("expected a quoted string, not {text:?}"))?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(escaped @ ('"' | '\\')) => out.push(escaped),
                _ => return Err(r#"only \" and \\ are escapes"#.into()),
            },
            '"' if chars.as_str().is_empty() => return Ok(out),
            '"' => return Err(format!("text after the closing quote in {text:?}")),
            c => out.push(c),
        }
    }
    Err(format!("no closing quote in {text:?}"))
}

/// `<selector> "<text>"`: the quoted string that ends the line, starting
/// at a word (a selector may hold quotes of its own, `[title="a"]`).
fn trailing_quoted(rest: &str) -> Result<(&str, String), String> {
    let starts = rest
        .char_indices()
        .filter(|&(i, c)| c == '"' && (i == 0 || rest[..i].ends_with(char::is_whitespace)));
    for (i, _) in starts {
        if let Ok(text) = quoted(&rest[i..]) {
            return Ok((rest[..i].trim(), text));
        }
    }
    Err(format!(
        "expected a selector and a quoted text, not {rest:?}"
    ))
}

/// `[modifier+]…key`: `ctrl+l`, `shift+Space`, `PageDown`.
fn key(rest: &str) -> Result<Command, String> {
    if rest.is_empty() || rest.contains(char::is_whitespace) {
        return Err(format!("`key` needs one key, like `ctrl+l`, not {rest:?}"));
    }
    let mut parts: Vec<&str> = rest.split('+').collect();
    // `key +` and `key shift++` are the plus key.
    if rest.ends_with("++") || rest == "+" {
        // At least two: the string has a `+`.
        parts.truncate(parts.len().saturating_sub(2));
        parts.push("+");
    }
    let name = parts.pop().unwrap_or_default();
    let mut modifiers = ModifiersState::empty();
    let mut primary = false;
    for modifier in parts {
        modifiers |= match modifier {
            "shift" => ModifiersState::SHIFT,
            "ctrl" => ModifiersState::CONTROL,
            "alt" => ModifiersState::ALT,
            "cmd" => ModifiersState::SUPER,
            "primary" => {
                primary = true;
                ModifiersState::empty()
            }
            _ => {
                return Err(format!(
                    "unknown modifier `{modifier}`: shift, ctrl, alt, cmd or primary"
                ));
            }
        };
    }
    let (key, text) = key_named(name)?;
    // Held, Ctrl, Alt or Cmd make a key a shortcut, which types nothing.
    let shortcut = primary
        || modifiers
            .intersects(ModifiersState::CONTROL | ModifiersState::ALT | ModifiersState::SUPER);
    let text = text.filter(|_| !shortcut).map(|text| {
        if modifiers.shift_key() {
            text.to_uppercase()
        } else {
            text
        }
    });
    Ok(Command::Key {
        key,
        modifiers,
        primary,
        text,
    })
}

/// The keys known by name (winit's `NamedKey`'s), as scenarios and
/// `Machopo.dispatchWindowInput` write them.
const NAMED_KEYS: &[(&str, NamedKey)] = &[
    ("Enter", NamedKey::Enter),
    ("Escape", NamedKey::Escape),
    ("Backspace", NamedKey::Backspace),
    ("Delete", NamedKey::Delete),
    ("Tab", NamedKey::Tab),
    ("Space", NamedKey::Space),
    ("ArrowUp", NamedKey::ArrowUp),
    ("ArrowDown", NamedKey::ArrowDown),
    ("ArrowLeft", NamedKey::ArrowLeft),
    ("ArrowRight", NamedKey::ArrowRight),
    ("PageUp", NamedKey::PageUp),
    ("PageDown", NamedKey::PageDown),
    ("Home", NamedKey::Home),
    ("End", NamedKey::End),
    ("F5", NamedKey::F5),
    ("F6", NamedKey::F6),
];

/// A key by its name (winit's `NamedKey`), or a character; and what it
/// types, as the platform says: Space a space, Enter a carriage return.
pub(crate) fn key_named(name: &str) -> Result<(Key, Option<String>), String> {
    if let Some((_, named)) = NAMED_KEYS.iter().find(|(known, _)| *known == name) {
        let text = match named {
            NamedKey::Space => Some(" ".to_owned()),
            NamedKey::Enter => Some("\r".to_owned()),
            _ => None,
        };
        return Ok((Key::Named(*named), text));
    }
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok((Key::Character(c.to_string().into()), Some(c.to_string()))),
        _ => Err(format!("unknown key `{name}`")),
    }
}

/// `key`'s name, as [`key_named`] reads it; `None` for one it doesn't know.
pub(crate) fn key_name(key: &Key) -> Option<String> {
    match key {
        Key::Named(named) => NAMED_KEYS
            .iter()
            .find(|(_, known)| known == named)
            .map(|(name, _)| (*name).to_owned()),
        Key::Character(c) => Some(c.to_string()),
        _ => None,
    }
}

/// The key that types `c`, alone.
fn typed(c: char) -> Command {
    let key = if c == ' ' {
        Key::Named(NamedKey::Space)
    } else {
        Key::Character(c.to_string().into())
    };
    Command::Key {
        key,
        modifiers: ModifiersState::empty(),
        primary: false,
        text: Some(c.to_string()),
    }
}

/// The scenarios a list of known failures names (`tests/scenarios/
/// known-failures.txt`): one a line, the file's name without `.txt`, `#`
/// comments.
pub fn known_failures(text: &str) -> Vec<&str> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .collect()
}

/// What fails a run of the scenarios `names` (those there are) whose
/// `runs` (each a scenario's name and whether it passed) `known` lets
/// fail: a run that failed though `known` doesn't name its scenario, a
/// scenario `known` names that passed every run (to be taken off), and a
/// name in `known` no scenario has. Empty if none.
pub fn judge_runs(names: &[&str], runs: &[(&str, bool)], known: &[&str]) -> Vec<String> {
    let mut problems = Vec::new();
    for name in known {
        if !names.contains(name) {
            problems.push(format!(
                "known-failures.txt names {name}, which no scenario is"
            ));
        }
    }
    for (name, passed) in runs {
        if !passed && !known.contains(name) {
            problems.push(format!("failed: {name}"));
        }
    }
    for name in known {
        let mut ran = runs.iter().filter(|(run, _)| run == name).peekable();
        if ran.peek().is_some() && ran.all(|(_, passed)| *passed) {
            problems.push(format!(
                "passes though known-failures.txt names it: take it off the list: {name}"
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Known failures: one a line, comments and blank lines skipped.
    #[test]
    fn known_failures_are_names_a_line() {
        assert_eq!(
            known_failures("# about\n\nsvg # why\n  cursor\n"),
            ["svg", "cursor"]
        );
    }

    /// A failure the list doesn't name fails the run, one it names
    /// doesn't; a scenario it names that passed every run fails it (one
    /// that failed in one of its runs doesn't), as does a name no scenario
    /// has; one it names that didn't run (filtered out) doesn't.
    #[test]
    fn a_run_is_judged_by_the_known_failures() {
        let names = ["a", "b", "c", "d"];
        assert!(judge_runs(&names, &[("a", true), ("b", false)], &["b"]).is_empty());
        assert_eq!(judge_runs(&names, &[("a", false)], &[]), ["failed: a"]);
        assert_eq!(
            judge_runs(&names, &[("b", true), ("b", true)], &["b"]),
            ["passes though known-failures.txt names it: take it off the list: b"]
        );
        assert!(judge_runs(&names, &[("b", true), ("b", false)], &["b"]).is_empty());
        assert!(judge_runs(&names, &[("a", true)], &["c"]).is_empty());
        assert_eq!(
            judge_runs(&names, &[], &["z"]),
            ["known-failures.txt names z, which no scenario is"]
        );
    }

    /// The numbers at the end come after white space of any width, a
    /// full-width space (U+3000, three bytes) too.
    #[test]
    fn trailing_numbers_after_any_white_space() {
        assert_eq!(trailing_numbers::<2>("#a 1 2"), Ok(("#a", [1.0, 2.0])));
        assert_eq!(
            trailing_numbers::<2>("#a\u{3000}1\u{3000}2"),
            Ok(("#a", [1.0, 2.0]))
        );
    }

    fn commands(text: &str) -> Vec<Command> {
        parse(text)
            .unwrap()
            .steps
            .into_iter()
            .map(|s| s.command)
            .collect()
    }

    fn error(text: &str) -> ParseError {
        parse(text).unwrap_err()
    }

    #[test]
    fn steps_keep_their_lines() {
        let steps = parse("# comment\n\nnavigate about:demo\n  wait-loaded  \n")
            .unwrap()
            .steps;
        assert_eq!(
            steps,
            [
                Step {
                    line: 3,
                    command: Command::Navigate("about:demo".into())
                },
                Step {
                    line: 4,
                    command: Command::WaitLoaded
                },
            ]
        );
    }

    #[test]
    fn pointer_commands() {
        assert_eq!(
            commands(
                "click #a .b\nclick-at 10 20.5\ndblclick #c\ndblclick-at 1 2\n\
                 wheel #box > p 0 -40\nwheel-at 1 2 3 4\nhover a[href]\nhover-at 5 6\n\
                 wait-cursor ew-resize"
            ),
            [
                Command::Click(Target::Element("#a .b".into())),
                Command::Click(Target::Point(PointF::new(10.0, 20.5))),
                Command::DoubleClick(Target::Element("#c".into())),
                Command::DoubleClick(Target::Point(PointF::new(1.0, 2.0))),
                Command::Wheel {
                    target: Target::Element("#box > p".into()),
                    dx: 0.0,
                    dy: -40.0
                },
                Command::Wheel {
                    target: Target::Point(PointF::new(1.0, 2.0)),
                    dx: 3.0,
                    dy: 4.0
                },
                Command::Hover(Target::Element("a[href]".into())),
                Command::Hover(Target::Point(PointF::new(5.0, 6.0))),
                Command::WaitCursor("ew-resize".into()),
            ]
        );
    }

    #[test]
    fn waits_and_captures() {
        assert_eq!(
            commands(concat!(
                "wait-text #count \"Clicked 1 time (onclick).\"\n",
                "wait-text a[title=\"x y\"] \"say \\\"hi\\\" \\\\ \"\n",
                "wait-text #empty \"\"\n",
                "wait-title \"A \\\"page\\\"\"\n",
                "wait-change #clock\n",
                "scroll-to #far .item\n",
                "capture before\n",
                "expect-differs before\n",
                "expect-differs before #a b\n",
                "screenshot demo-1.png\n",
                "resize 640 480\n",
                "timeout 0.5\n",
            )),
            [
                Command::WaitText {
                    selector: "#count".into(),
                    text: "Clicked 1 time (onclick).".into()
                },
                Command::WaitText {
                    selector: "a[title=\"x y\"]".into(),
                    text: "say \"hi\" \\ ".into()
                },
                Command::WaitText {
                    selector: "#empty".into(),
                    text: "".into()
                },
                Command::WaitTitle("A \"page\"".into()),
                Command::WaitChange("#clock".into()),
                Command::ScrollTo("#far .item".into()),
                Command::Capture {
                    name: "before".into(),
                    now: false
                },
                Command::ExpectDiffers {
                    name: "before".into(),
                    selector: None
                },
                Command::ExpectDiffers {
                    name: "before".into(),
                    selector: Some("#a b".into())
                },
                Command::Screenshot {
                    name: "demo-1.png".into(),
                    now: false
                },
                Command::Resize {
                    width: 640.0,
                    height: 480.0
                },
                Command::Timeout(Duration::from_millis(500)),
            ]
        );
    }

    #[test]
    fn keys_and_typing() {
        let key = |key: Key, modifiers, text: Option<&str>| Command::Key {
            key,
            modifiers,
            primary: false,
            text: text.map(str::to_owned),
        };
        let char = |c: &str| Key::Character(c.into());
        assert_eq!(
            commands(
                "key PageDown\nkey shift+Space\nkey ctrl+l\nkey cmd+shift+[\nkey a\nkey shift+a\nkey +\nkey alt++"
            ),
            [
                key(
                    Key::Named(NamedKey::PageDown),
                    ModifiersState::empty(),
                    None
                ),
                key(
                    Key::Named(NamedKey::Space),
                    ModifiersState::SHIFT,
                    Some(" ")
                ),
                key(char("l"), ModifiersState::CONTROL, None),
                key(
                    char("["),
                    ModifiersState::SUPER | ModifiersState::SHIFT,
                    None
                ),
                key(char("a"), ModifiersState::empty(), Some("a")),
                key(char("a"), ModifiersState::SHIFT, Some("A")),
                key(char("+"), ModifiersState::empty(), Some("+")),
                key(char("+"), ModifiersState::ALT, None),
            ]
        );
        assert_eq!(
            commands("type \"a b\""),
            [
                key(char("a"), ModifiersState::empty(), Some("a")),
                key(
                    Key::Named(NamedKey::Space),
                    ModifiersState::empty(),
                    Some(" ")
                ),
                key(char("b"), ModifiersState::empty(), Some("b")),
            ]
        );
    }

    /// `primary` is Cmd or Ctrl, chosen when the scenario runs; it types
    /// nothing.
    #[test]
    fn the_primary_modifier() {
        assert_eq!(
            commands("key primary+l\nkey primary+shift+a"),
            [
                Command::Key {
                    key: Key::Character("l".into()),
                    modifiers: ModifiersState::empty(),
                    primary: true,
                    text: None,
                },
                Command::Key {
                    key: Key::Character("a".into()),
                    modifiers: ModifiersState::SHIFT,
                    primary: true,
                    text: None,
                },
            ]
        );
    }

    /// `now`: the next frame drawn, not one showing the page's changes.
    #[test]
    fn frames_now() {
        assert_eq!(
            commands("capture busy now\nscreenshot shot now"),
            [
                Command::Capture {
                    name: "busy".into(),
                    now: true
                },
                Command::Screenshot {
                    name: "shot".into(),
                    now: true
                },
            ]
        );
        assert!(parse("capture busy later").is_err());
        assert!(parse("screenshot a now now").is_err());
    }

    #[test]
    fn frames() {
        assert_eq!(
            commands("frame iframe#a\nframe [name=\"b c\"]\nframe top\n"),
            [
                Command::Frame(Some("iframe#a".into())),
                Command::Frame(Some("[name=\"b c\"]".into())),
                Command::Frame(None),
            ]
        );
    }

    #[test]
    fn platforms_and_busy() {
        assert_eq!(
            commands("platform mac\nplatform other\nplatform all\nwait-busy"),
            [
                Command::Platform(Platform::Mac),
                Command::Platform(Platform::Other),
                Command::Platform(Platform::All),
                Command::WaitBusy,
            ]
        );
    }

    #[test]
    fn what_depends_on_shortcuts() {
        let depends = |text| parse(text).unwrap().depends_on_shortcuts();
        assert!(!depends(
            "window scale=2\nnavigate a.html\nclick #a\ndblclick #a\nwheel #a 0 10\nwait-text #a \"x\""
        ));
        assert!(depends("click #a\nkey Tab"));
        assert!(depends("type \"a\""));
        assert!(depends("platform all\nclick #a"));
    }

    /// `window` lines come first; later ones add to earlier ones.
    #[test]
    fn window_settings() {
        let scenario =
            parse("# first\nwindow scale=2\nwindow size=640x480.5 caret-blink=250\nwait-loaded")
                .unwrap();
        assert_eq!(
            scenario.window,
            WindowSettings {
                scale: Some(2.0),
                size: Some((640.0, 480.5)),
                caret_blink: Some(std::time::Duration::from_millis(250)),
            }
        );
        assert_eq!(scenario.steps.len(), 1);
        assert_eq!(
            parse("wait-loaded").unwrap().window,
            WindowSettings::default()
        );
        assert_eq!(error("wait-loaded\nwindow scale=2").line, 2);
        for bad in [
            "window",
            "window scale",
            "window scale=0",
            "window scale=x",
            "window script=js",
            "window size=640",
            "window caret-blink=0",
            "window size=0x10",
            "window colour=red",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn errors_name_the_line() {
        assert_eq!(error("wait-loaded\nfly away").line, 2);
        assert_eq!(error("fly").message, "unknown command `fly`");
        for bad in [
            "navigate",
            "wait-loaded now",
            "click",
            "click-at 1",
            "dblclick",
            "dblclick-at 1",
            "click-at 1 x",
            "click-at 1 inf",
            "wheel 1 2",
            "wheel-at 1 2 3",
            "hover",
            "hover-at 1",
            "wait-cursor",
            "wait-cursor auto",
            "wait-cursor hand",
            "wait-cursor all-scroll",
            "key",
            "key ctrl+",
            "key hyper+a",
            "key Fly",
            "key a b",
            "type abc",
            "type \"abc",
            "type \"a\"b\"",
            "type \"\\n\"",
            "wait-text #a",
            "wait-text \"x\"",
            "wait-text #a\"b\"",
            "wait-change",
            "scroll-to",
            "frame",
            "wait-busy now",
            "platform",
            "platform linux",
            "key primary",
            "key super+a",
            "capture",
            "capture ../x",
            "capture a b",
            "screenshot .hidden",
            "expect-differs",
            "resize 0 10",
            "timeout 0",
            "timeout -1",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
