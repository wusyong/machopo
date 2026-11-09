//! Which fonts a `font-family` list asks the font collection for, in
//! order, as Chrome finds them: each name, then what Blink and Skia take
//! for it when it isn't there, and the generic families as Chrome's
//! default font settings name them. fontique looks a name up only among
//! the families the fonts themselves name, so what Chrome takes in place
//! of a name is spelled out here.
//!
//! Three layers, as in Chromium:
//! - Blink's alternate names (`AlternateFamilyName`): Arial and
//!   Helvetica, Times and Times New Roman, Courier and Courier New stand
//!   for each other, on every platform.
//! - Skia's metric-compatible fonts, on Linux only (`SkFontConfigInterface`
//!   takes what fontconfig gives for a name only if it is in the name's
//!   class of `kFontEquivMap`): Arial's class is Arimo and Liberation Sans,
//!   and so on, those fontconfig gives (see [`METRIC_COMPATIBLE`]). In the
//!   order fontconfig's `30-metric-aliases.conf` accepts them, which picks
//!   among those installed.
//! - The generic families on Linux, as Chrome's defaults name them
//!   (`locale_settings_linux.grd`): `serif` and the standard font are Times
//!   New Roman, `sans-serif` Arial, `cursive` Comic Sans MS, `fantasy`
//!   Impact. fontique's own (fontconfig's `serif`...) only come last, when
//!   none of those is there. On macOS and Windows fontique's generic
//!   families are already Chrome's defaults.

use std::borrow::Cow;

use computed_style::{FontFamily, GenericFamily};
use parley::FontFamilyName;

/// What a family list resolves differently on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    Linux,
    MacOs,
    Windows,
}

impl Platform {
    pub(crate) const CURRENT: Self = if cfg!(target_os = "windows") {
        Self::Windows
    } else if cfg!(target_os = "macos") {
        Self::MacOs
    } else {
        Self::Linux
    };
}

/// Skia's `kFontEquivMap`: each font with its class of metric-compatible
/// ones, in Skia's order (a name's class is its first entry's). Only the
/// classes fontconfig's own `30-metric-aliases.conf` maps names to: Skia
/// takes a font of the class only if fontconfig gives it first, which for
/// the Japanese and Chinese classes (MS Gothic, SimSun...) it doesn't
/// unless a distribution adds the aliases, so Chrome doesn't take them.
const METRIC_COMPATIBLE: &[(&str, &str)] = &[
    ("sans", "Arial"),
    ("sans", "Arimo"),
    ("sans", "Liberation Sans"),
    ("serif", "Times New Roman"),
    ("serif", "Tinos"),
    ("serif", "Liberation Serif"),
    ("mono", "Courier New"),
    ("mono", "Cousine"),
    ("mono", "Liberation Mono"),
    ("symbol", "Symbol"),
    ("symbol", "Symbol Neu"),
    ("cambria", "Cambria"),
    ("cambria", "Caladea"),
    ("calibri", "Calibri"),
    ("calibri", "Carlito"),
];

/// The standard font, Chrome's default on Linux.
const STANDARD: &str = "Times New Roman";

/// `name`'s class of metric-compatible fonts (Skia's `GetFontEquivClass`:
/// the first entry that is `name`, ASCII case-insensitive).
fn class_of(name: &str) -> Option<&'static str> {
    METRIC_COMPATIBLE
        .iter()
        .find(|(_, font)| font.eq_ignore_ascii_case(name))
        .map(|&(class, _)| class)
}

/// The other fonts Skia takes for `name`: those whose class is `name`'s.
fn metric_compatible(name: &str) -> impl Iterator<Item = &'static str> {
    let class = class_of(name);
    METRIC_COMPATIBLE
        .iter()
        .map(|&(_, font)| font)
        .filter(move |font| {
            class.is_some() && class_of(font) == class && !font.eq_ignore_ascii_case(name)
        })
}

/// Blink's `AlternateFamilyName`: the name tried when `name` isn't there.
/// On Windows Courier New has none, Courier being a bitmap font there.
fn alternate(name: &str, platform: Platform) -> Option<&'static str> {
    let pairs: &[(&str, &str)] = &[
        ("Courier", "Courier New"),
        ("Courier New", "Courier"),
        ("Times", "Times New Roman"),
        ("Times New Roman", "Times"),
        ("Arial", "Helvetica"),
        ("Helvetica", "Arial"),
    ];
    pairs
        .iter()
        .find(|(from, _)| from.eq_ignore_ascii_case(name))
        .map(|&(_, to)| to)
        .filter(|to| !(platform == Platform::Windows && *to == "Courier"))
}

/// The families the shaper is to try for `families`, in order, on
/// `platform`: the list ends in the UA's standard font.
pub(crate) fn font_stack(
    families: &[FontFamily],
    platform: Platform,
) -> Vec<FontFamilyName<'static>> {
    let mut stack = Stack::default();
    for family in families {
        match family {
            FontFamily::Named(name) => stack.named(name, platform),
            FontFamily::Generic(generic) => stack.generic(*generic, platform),
        }
    }
    // The UA's standard font when none of the listed ones is available,
    // then fontique's `serif` should that be missing too.
    if platform == Platform::Linux {
        stack.named(STANDARD, platform);
    }
    stack.push_generic(parley::GenericFamily::Serif);
    stack.out
}

#[derive(Default)]
struct Stack {
    out: Vec<FontFamilyName<'static>>,
}

impl Stack {
    /// `name` and what Chrome takes for it, each name once.
    fn named(&mut self, name: &str, platform: Platform) {
        self.push_with_metric_compatible(name, platform);
        if let Some(alternate) = alternate(name, platform) {
            self.push_with_metric_compatible(alternate, platform);
        }
    }

    fn push_with_metric_compatible(&mut self, name: &str, platform: Platform) {
        self.push(name);
        if platform == Platform::Linux {
            for font in metric_compatible(name) {
                self.push(font);
            }
        }
    }

    fn push(&mut self, name: &str) {
        let seen = self.out.iter().any(|family| {
            matches!(family, FontFamilyName::Named(seen) if seen.eq_ignore_ascii_case(name))
        });
        if !seen {
            self.out
                .push(FontFamilyName::Named(Cow::Owned(name.to_owned())));
        }
    }

    /// `generic`: on Linux the font Chrome's defaults name, as a name (not
    /// there, the list goes on: Chrome's `cursive` with no Comic Sans MS
    /// is the standard font at the end, not fontconfig's cursive font).
    fn generic(&mut self, generic: GenericFamily, platform: Platform) {
        let named = match generic {
            GenericFamily::Serif => Some(STANDARD),
            GenericFamily::SansSerif => Some("Arial"),
            GenericFamily::Cursive => Some("Comic Sans MS"),
            GenericFamily::Fantasy => Some("Impact"),
            // Chrome's default is "Monospace", which fontconfig resolves as
            // fontique does; `system-ui` is the platform's.
            GenericFamily::Monospace | GenericFamily::SystemUi => None,
        };
        if platform == Platform::Linux
            && let Some(named) = named
        {
            self.named(named, platform);
            return;
        }
        self.push_generic(match generic {
            GenericFamily::Serif => parley::GenericFamily::Serif,
            GenericFamily::SansSerif => parley::GenericFamily::SansSerif,
            GenericFamily::Monospace => parley::GenericFamily::Monospace,
            GenericFamily::Cursive => parley::GenericFamily::Cursive,
            GenericFamily::Fantasy => parley::GenericFamily::Fantasy,
            GenericFamily::SystemUi => parley::GenericFamily::SystemUi,
        });
    }

    fn push_generic(&mut self, generic: parley::GenericFamily) {
        let family = FontFamilyName::Generic(generic);
        if !self.out.contains(&family) {
            self.out.push(family);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stack for `css` (a `font-family` list, names unquoted), names
    /// as they are and generics as `<name>`.
    fn stack(css: &[&str], platform: Platform) -> Vec<String> {
        let families: Vec<FontFamily> = css
            .iter()
            .map(|name| match *name {
                "serif" => FontFamily::Generic(GenericFamily::Serif),
                "sans-serif" => FontFamily::Generic(GenericFamily::SansSerif),
                "monospace" => FontFamily::Generic(GenericFamily::Monospace),
                "cursive" => FontFamily::Generic(GenericFamily::Cursive),
                "fantasy" => FontFamily::Generic(GenericFamily::Fantasy),
                "system-ui" => FontFamily::Generic(GenericFamily::SystemUi),
                name => FontFamily::Named(name.into()),
            })
            .collect();
        font_stack(&families, platform)
            .into_iter()
            .map(|family| match family {
                FontFamilyName::Named(name) => name.into_owned(),
                FontFamilyName::Generic(generic) => format!("<{generic:?}>"),
            })
            .collect()
    }

    /// What Linux ends every list in: the standard font, Times New Roman,
    /// what Skia and Blink take for it, and fontique's serif.
    const LINUX_END: [&str; 5] = [
        "Times New Roman",
        "Tinos",
        "Liberation Serif",
        "Times",
        "<Serif>",
    ];

    fn linux(css: &[&str], expected: &[&str]) {
        let end = LINUX_END.iter().filter(|end| !expected.contains(end));
        let expected: Vec<&str> = expected.iter().chain(end).copied().collect();
        assert_eq!(stack(css, Platform::Linux), expected, "{css:?}");
    }

    /// On Linux, as Chrome 152 picks the fonts there: Blink's alternate
    /// names, Skia's metric-compatible fonts in fontconfig's order (each
    /// name once, ASCII case-insensitive), and the generic families as
    /// Chrome's defaults name them.
    #[test]
    fn linux_takes_chromes_names_for_each() {
        linux(
            &["Helvetica Neue", "Helvetica", "Verdana", "sans-serif"],
            &[
                "Helvetica Neue",
                "Helvetica",
                "Arial",
                "Arimo",
                "Liberation Sans",
                "Verdana",
            ],
        );
        linux(
            &["ARIAL"],
            &["ARIAL", "Arimo", "Liberation Sans", "Helvetica"],
        );
        // Blink's alternate is of the name itself: Arial's, not its class's.
        linux(&["Liberation Sans"], &["Liberation Sans", "Arial", "Arimo"]);
        linux(
            &["Times"],
            &["Times", "Times New Roman", "Tinos", "Liberation Serif"],
        );
        linux(
            &["Courier"],
            &["Courier", "Courier New", "Cousine", "Liberation Mono"],
        );
        linux(
            &["Calibri", "Cambria"],
            &["Calibri", "Carlito", "Cambria", "Caladea"],
        );
        // fontconfig maps none of the Japanese and Chinese fonts to
        // Skia's classes for them: Chrome 152 takes none.
        linux(&["SimSun", "ＭＳ 明朝"], &["SimSun", "ＭＳ 明朝"]);
        linux(&["Symbol"], &["Symbol", "Symbol Neu"]);
        linux(&["Palatino"], &["Palatino"]);
        linux(
            &["sans-serif"],
            &["Arial", "Arimo", "Liberation Sans", "Helvetica"],
        );
        linux(&["serif"], &[]);
        linux(&["cursive", "fantasy"], &["Comic Sans MS", "Impact"]);
        linux(&["monospace"], &["<Monospace>"]);
        linux(&["system-ui"], &["<SystemUi>"]);
    }

    /// On macOS and Windows: Blink's alternate names only, the generic
    /// families fontique's (already Chrome's defaults there); Courier New
    /// has none on Windows, Courier being a bitmap font there.
    #[test]
    fn macos_and_windows_take_the_alternate_names() {
        assert_eq!(
            stack(&["Helvetica", "sans-serif"], Platform::MacOs),
            ["Helvetica", "Arial", "<SansSerif>", "<Serif>"]
        );
        assert_eq!(
            stack(&["Liberation Sans", "cursive"], Platform::MacOs),
            ["Liberation Sans", "<Cursive>", "<Serif>"]
        );
        assert_eq!(
            stack(&["Courier New"], Platform::MacOs),
            ["Courier New", "Courier", "<Serif>"]
        );
        assert_eq!(
            stack(&["Courier New"], Platform::Windows),
            ["Courier New", "<Serif>"]
        );
        assert_eq!(
            stack(&["Courier", "Times"], Platform::Windows),
            [
                "Courier",
                "Courier New",
                "Times",
                "Times New Roman",
                "<Serif>"
            ]
        );
    }
}
