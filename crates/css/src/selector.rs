//! `selectors`' `SelectorImpl` for machopo and its selector parser: what a
//! selector is made of and how it parses and serializes. Matching selectors
//! against the DOM (`selectors::Element`) is `style`'s.
//!
//! Besides what `selectors` implements itself (tree-structural
//! pseudo-classes, `:not()`, `:is()`, `:where()`, `:has()`), only the link,
//! user-action, form, `:defined`, `:target` and `:lang()` pseudo-classes
//! and the `::before` and `::after` pseudo-elements are parsed; a rule
//! using any other pseudo-class or pseudo-element is dropped as invalid.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use cssparser::{CowRcStr, Parser, ParserInput, SourceLocation, ToCss};
use precomputed_hash::PrecomputedHash;

use selectors::SelectorImpl;
use selectors::parser::{Component, ParseRelative, SelectorList, SelectorParseErrorKind};

#[derive(Debug, Clone)]
pub struct Impl;

impl SelectorImpl for Impl {
    type ExtraMatchingData<'a> = ();
    type AttrValue = CssString;
    type Identifier = CssIdent;
    type LocalName = CssIdent;
    type NamespaceUrl = CssIdent;
    type NamespacePrefix = CssIdent;
    type BorrowedLocalName = CssIdent;
    type BorrowedNamespaceUrl = CssIdent;
    type NonTSPseudoClass = PseudoClass;
    type PseudoElement = PseudoElement;
}

/// An identifier, name or namespace URL in a selector, and what an
/// embedder keeps of it on its thread (`IdentAtom`): equal by its text.
#[derive(Debug, Clone, Default)]
pub struct CssIdent(pub String, pub IdentAtom);

impl PartialEq for CssIdent {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for CssIdent {}

impl From<&str> for CssIdent {
    fn from(s: &str) -> Self {
        Self(s.to_owned(), IdentAtom::default())
    }
}

/// A number an embedder gives an identifier on one thread (the DOM's
/// style: the name's in its thread's table, Blink's selector holding an
/// `AtomicString`), with that thread's serial: so that matching compares
/// numbers, not text, and a thread that isn't the one that set it finds
/// none (a sheet's rules are shared, and each thread numbers names its
/// own way). The serial and the number are one atomic word: no thread
/// reads one's serial with another's number.
#[derive(Debug, Default)]
pub struct IdentAtom(AtomicU64);

impl IdentAtom {
    /// Its number on the thread of serial `thread` (not 0), if that thread
    /// set one.
    #[inline]
    pub fn get(&self, thread: u32) -> Option<u32> {
        let word = self.0.load(Ordering::Relaxed);
        (thread != 0 && word >> 32 == u64::from(thread))
            .then(|| u32::try_from(word & u64::from(u32::MAX)).expect("the low 32 bits"))
    }

    /// `number` is its number on the thread of serial `thread`.
    pub fn set(&self, thread: u32, number: u32) {
        self.0.store(
            (u64::from(thread) << 32) | u64::from(number),
            Ordering::Relaxed,
        );
    }
}

impl Clone for IdentAtom {
    /// The same number on the same thread: a copy of the identifier is it.
    fn clone(&self) -> Self {
        Self(AtomicU64::new(self.0.load(Ordering::Relaxed)))
    }
}

impl ToCss for CssIdent {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        cssparser::serialize_identifier(&self.0, dest)
    }
}

impl PrecomputedHash for CssIdent {
    /// What the ancestor bloom filter takes it by (`name_hash`), worked out
    /// when a rule set is made.
    fn precomputed_hash(&self) -> u32 {
        name_hash(&self.0)
    }
}

/// The hash of `name` ASCII lowercase: what an id, a class, an element's
/// local name or a namespace goes by in the ancestor bloom filter and in a
/// rule set's buckets (where Blink has an `AtomicString`'s hash), the same
/// whatever its case: a quirks mode document matches ids and classes
/// ASCII case-insensitively, and one rule set serves both modes. Never 0
/// in the bits the bloom filter uses, which `selectors` takes as no hash.
pub fn name_hash(name: &str) -> u32 {
    use std::hash::Hasher;
    let mut hasher = rustc_hash::FxHasher::default();
    for byte in name.bytes() {
        hasher.write_u8(byte.to_ascii_lowercase());
    }
    let hash = hasher.finish();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "folds the hash's two halves into one"
    )]
    let hash = (hash ^ (hash >> 32)) as u32;
    if hash & selectors::bloom::BLOOM_HASH_MASK == 0 {
        1
    } else {
        hash
    }
}

/// An attribute value in a selector.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CssString(String);

impl From<&str> for CssString {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl AsRef<str> for CssString {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl ToCss for CssString {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        cssparser::serialize_string(&self.0, dest)
    }
}

/// The non-tree-structural pseudo-classes we parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PseudoClass {
    Link,
    AnyLink,
    Visited,
    Hover,
    Active,
    Focus,
    FocusVisible,
    FocusWithin,
    Checked,
    Indeterminate,
    Disabled,
    Enabled,
    Default,
    PlaceholderShown,
    ReadOnly,
    ReadWrite,
    Required,
    Optional,
    Defined,
    Target,
    /// `:open`: an open `<details>` or `<dialog>`, a select whose dropdown
    /// is shown.
    Open,
    /// `:window-inactive`: the page doesn't have the focus. Chrome's, on
    /// any element and after `::selection` (Blink's `kPseudoWindowInactive`).
    WindowInactive,
    /// `:lang()`'s language ranges, as written.
    Lang(Box<[LangRange]>),
    /// `:-internal-list-box`: a `<select>` shown as a list box, not a menu
    /// list. The UA stylesheet's only (Blink's `kPseudoListBox`).
    InternalListBox,
    /// `:-internal-multi-select-focus`: the `<option>` of a `multiple`
    /// list box the keys moved to without selecting it. The UA
    /// stylesheet's only (Blink's `kPseudoMultiSelectFocus`).
    InternalMultiSelectFocus,
}

impl PseudoClass {
    const ALL: [(Self, &'static str); 22] = [
        (Self::Link, "link"),
        (Self::AnyLink, "any-link"),
        (Self::Visited, "visited"),
        (Self::Hover, "hover"),
        (Self::Active, "active"),
        (Self::Focus, "focus"),
        (Self::FocusVisible, "focus-visible"),
        (Self::FocusWithin, "focus-within"),
        (Self::Checked, "checked"),
        (Self::Indeterminate, "indeterminate"),
        (Self::Disabled, "disabled"),
        (Self::Enabled, "enabled"),
        (Self::Default, "default"),
        (Self::PlaceholderShown, "placeholder-shown"),
        (Self::ReadOnly, "read-only"),
        (Self::ReadWrite, "read-write"),
        (Self::Required, "required"),
        (Self::Optional, "optional"),
        (Self::Defined, "defined"),
        (Self::Target, "target"),
        (Self::Open, "open"),
        (Self::WindowInactive, "window-inactive"),
    ];
}

impl ToCss for PseudoClass {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        match self {
            Self::InternalListBox => return dest.write_str(":-internal-list-box"),
            Self::InternalMultiSelectFocus => {
                return dest.write_str(":-internal-multi-select-focus");
            }
            _ => {}
        }
        if let Self::Lang(ranges) = self {
            dest.write_str(":lang(")?;
            for (i, range) in ranges.iter().enumerate() {
                if i > 0 {
                    dest.write_str(", ")?;
                }
                range.to_css(dest)?;
            }
            return dest.write_str(")");
        }
        let name = Self::ALL.iter().find(|(pc, _)| pc == self).unwrap().1;
        write!(dest, ":{name}")
    }
}

/// One of `:lang()`'s language ranges (selectors-4): an identifier or a
/// string, which serializes as it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LangRange {
    pub range: String,
    pub quoted: bool,
}

impl ToCss for LangRange {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        if self.quoted {
            cssparser::serialize_string(&self.range, dest)
        } else {
            cssparser::serialize_identifier(&self.range, dest)
        }
    }
}

impl LangRange {
    /// Whether this range matches the language tag `tag` (RFC 4647's
    /// extended filtering, as selectors-4 has `:lang()` match): subtag by
    /// subtag, ASCII case-insensitively, `*` matching any one, a subtag of
    /// the range missing from the tag skipped over unless a singleton comes
    /// first. The first subtag has to match (or be `*`), so an empty range
    /// matches nothing; nor does anything match a tag that isn't
    /// well-formed (a subtag empty or longer than eight characters, BCP
    /// 47's limit), such as the empty one of an unknown language.
    pub fn matches(&self, tag: &str) -> bool {
        if tag
            .split('-')
            .any(|subtag| subtag.is_empty() || subtag.len() > 8)
        {
            return false;
        }
        let mut range = self.range.split('-');
        let mut tag = tag.split('-').peekable();
        let (Some(first), Some(tag_first)) = (range.next(), tag.next()) else {
            return false;
        };
        if first != "*" && !first.eq_ignore_ascii_case(tag_first) {
            return false;
        }
        for subtag in range {
            if subtag == "*" {
                continue;
            }
            loop {
                let Some(next) = tag.peek() else {
                    return false;
                };
                if next.eq_ignore_ascii_case(subtag) {
                    tag.next();
                    break;
                }
                if next.len() == 1 {
                    return false;
                }
                tag.next();
            }
        }
        true
    }
}

impl selectors::parser::NonTSPseudoClass for PseudoClass {
    type Impl = Impl;

    fn is_active_or_hover(&self) -> bool {
        matches!(self, Self::Active | Self::Hover)
    }

    /// What may follow a pseudo-element that takes state pseudo-classes:
    /// the user action ones, and `:window-inactive` (Chrome takes it after
    /// `::selection` and a UA part such as `::-webkit-scrollbar`; after
    /// `::selection`, nothing else, [`check_list`]).
    fn is_user_action_state(&self) -> bool {
        matches!(
            self,
            Self::Active
                | Self::Hover
                | Self::Focus
                | Self::FocusVisible
                | Self::FocusWithin
                | Self::WindowInactive
        )
    }
}

/// Pseudo-elements: those whose style is computed with their element's
/// (`computed_style::PseudoId`), `::selection` (a highlight: its style is
/// the element's `HighlightStyle`, with nothing of its own to lay out), and
/// those that are an element of the element's UA shadow tree (Blink's shadow pseudo id): `::placeholder`,
/// any `::-webkit-` one, and in the UA stylesheet any `::-internal-` one
/// (the parts no author's selector names), by its name in ASCII lowercase
/// (Blink's
/// `kPseudoWebKitCustomElement`: `::-webkit-input-placeholder`,
/// `::-webkit-inner-spin-button`; one machopo makes no element for matches
/// nothing, and doesn't make the rule invalid).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PseudoElement {
    Before,
    After,
    Marker,
    Selection,
    Placeholder,
    Webkit(Box<str>),
}

impl PseudoElement {
    /// The pseudo-element styled with its element; `None` for one that is
    /// an element of its own.
    pub fn id(&self) -> Option<computed_style::PseudoId> {
        match self {
            Self::Before => Some(computed_style::PseudoId::Before),
            Self::After => Some(computed_style::PseudoId::After),
            Self::Marker => Some(computed_style::PseudoId::Marker),
            Self::Selection | Self::Placeholder | Self::Webkit(_) => None,
        }
    }

    /// The shadow pseudo id of the UA shadow tree's element it is (Blink's
    /// `ShadowPseudoId`), which that element has as its `pseudo` attribute.
    pub fn shadow_pseudo_id(&self) -> Option<&str> {
        match self {
            Self::Placeholder => Some(PLACEHOLDER_PSEUDO_ID),
            Self::Webkit(name) => Some(name),
            Self::Before | Self::After | Self::Marker | Self::Selection => None,
        }
    }
}

/// The placeholder element's shadow pseudo id (Blink's
/// `shadow_element_names::kPseudoInputPlaceholder`).
pub const PLACEHOLDER_PSEUDO_ID: &str = "-webkit-input-placeholder";

/// The `::-webkit-` pseudo-elements Chrome knows (Blink's
/// `FeatureForWebKitCustomPseudoElement`), in order: `@supports
/// selector()` takes only these, though a style rule takes any.
const KNOWN_WEBKIT_PSEUDO_ELEMENTS: [&str; 52] = [
    "-webkit-calendar-picker-indicator",
    "-webkit-color-swatch",
    "-webkit-color-swatch-wrapper",
    "-webkit-date-and-time-value",
    "-webkit-datetime-edit",
    "-webkit-datetime-edit-ampm-field",
    "-webkit-datetime-edit-day-field",
    "-webkit-datetime-edit-fields-wrapper",
    "-webkit-datetime-edit-hour-field",
    "-webkit-datetime-edit-millisecond-field",
    "-webkit-datetime-edit-minute-field",
    "-webkit-datetime-edit-month-field",
    "-webkit-datetime-edit-second-field",
    "-webkit-datetime-edit-text",
    "-webkit-datetime-edit-week-field",
    "-webkit-datetime-edit-year-field",
    "-webkit-file-upload-button",
    "-webkit-inner-spin-button",
    "-webkit-input-placeholder",
    "-webkit-media-controls",
    "-webkit-media-controls-current-time-display",
    "-webkit-media-controls-enclosure",
    "-webkit-media-controls-fullscreen-button",
    "-webkit-media-controls-mute-button",
    "-webkit-media-controls-overlay-enclosure",
    "-webkit-media-controls-overlay-play-button",
    "-webkit-media-controls-panel",
    "-webkit-media-controls-play-button",
    "-webkit-media-controls-time-remaining-display",
    "-webkit-media-controls-timeline",
    "-webkit-media-controls-timeline-container",
    "-webkit-media-controls-toggle-closed-captions-button",
    "-webkit-media-controls-volume-slider",
    "-webkit-media-slider-container",
    "-webkit-media-slider-thumb",
    "-webkit-media-text-track-container",
    "-webkit-media-text-track-display",
    "-webkit-media-text-track-region",
    "-webkit-media-text-track-region-container",
    "-webkit-meter-bar",
    "-webkit-meter-even-less-good-value",
    "-webkit-meter-inner-element",
    "-webkit-meter-optimum-value",
    "-webkit-meter-suboptimum-value",
    "-webkit-progress-bar",
    "-webkit-progress-inner-element",
    "-webkit-progress-value",
    "-webkit-search-cancel-button",
    "-webkit-slider-container",
    "-webkit-slider-runnable-track",
    "-webkit-slider-thumb",
    "-webkit-textfield-decoration-container",
];

impl ToCss for PseudoElement {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        dest.write_str(match self {
            Self::Before => "::before",
            Self::After => "::after",
            Self::Marker => "::marker",
            Self::Selection => "::selection",
            Self::Placeholder => "::placeholder",
            Self::Webkit(name) => return write!(dest, "::{name}"),
        })
    }
}

impl selectors::parser::PseudoElement for PseudoElement {
    type Impl = Impl;

    fn is_before_or_after(&self) -> bool {
        matches!(self, Self::Before | Self::After)
    }

    /// A UA part (`::-webkit-inner-spin-button:hover`), as Chrome, and
    /// `::selection` (`:window-inactive` only, [`check_list`]); not
    /// `::placeholder`.
    fn accepts_state_pseudo_classes(&self) -> bool {
        matches!(self, Self::Webkit(_) | Self::Selection)
    }

    /// Each of them but `::selection`, as Chrome (css-scoping 1:
    /// `::slotted()` takes the tree-abiding pseudo-elements; Chrome takes
    /// `::placeholder` too).
    fn valid_after_slotted(&self) -> bool {
        !matches!(self, Self::Selection)
    }
}

/// Parses selectors, with `:is()` / `:where()` / `:has()` and `of S` in
/// `:nth-child()`, and shadow DOM's `:host`, `:host()` and `::slotted()`;
/// no nesting (`&`), `::part()` or `:host-context()`. The UA stylesheet's
/// also takes the `:-internal-` pseudo-classes and the `svg|` namespace
/// prefix, and HTML's namespace is its default, which a page's doesn't
/// (Blink's `CSSParserMode` `kUASheetMode`; a page's has no `@namespace`).
pub struct SelectorParser {
    user_agent: bool,
}

impl SelectorParser {
    pub const AUTHOR: Self = Self { user_agent: false };
    pub const USER_AGENT: Self = Self { user_agent: true };
}

impl<'i> selectors::parser::Parser<'i> for SelectorParser {
    type Impl = Impl;
    type Error = SelectorParseErrorKind<'i>;

    fn parse_host(&self) -> bool {
        true
    }

    fn parse_slotted(&self) -> bool {
        true
    }

    fn parse_is_and_where(&self) -> bool {
        true
    }

    fn parse_has(&self) -> bool {
        true
    }

    fn parse_nth_child_of(&self) -> bool {
        true
    }

    /// The UA stylesheet's selectors are HTML's, as Chrome's `html.css`
    /// declares it the default namespace: an element of another with an
    /// HTML element's name (`createElementNS("not-html", "button")`) takes
    /// none of its rules. A page's stylesheet has no `@namespace`, so any.
    fn default_namespace(&self) -> Option<CssIdent> {
        self.user_agent
            .then(|| CssIdent::from("http://www.w3.org/1999/xhtml"))
    }

    /// In the UA stylesheet, `svg|` is SVG's namespace, for the rules of
    /// Chrome's `svg.css` (which declares it the default namespace). A
    /// page's stylesheet has no `@namespace`, so no prefix.
    fn namespace_for_prefix(&self, prefix: &CssIdent) -> Option<CssIdent> {
        (self.user_agent && prefix.0 == "svg").then(|| CssIdent::from("http://www.w3.org/2000/svg"))
    }

    /// `::before`, `::after`, `::marker`, `::selection`, `::placeholder`,
    /// any `::-webkit-` one, and in the UA stylesheet any `::-internal-`
    /// one; `selectors` sends the CSS 2 single-colon forms of the first two
    /// here too (not `:marker`). Not `::-moz-selection`: as in Chrome, it
    /// makes its rule invalid.
    fn parse_pseudo_element(
        &self,
        location: SourceLocation,
        name: CowRcStr<'i>,
    ) -> Result<PseudoElement, cssparser::ParseError<'i, Self::Error>> {
        if name.eq_ignore_ascii_case("before") {
            Ok(PseudoElement::Before)
        } else if name.eq_ignore_ascii_case("after") {
            Ok(PseudoElement::After)
        } else if name.eq_ignore_ascii_case("marker") {
            Ok(PseudoElement::Marker)
        } else if name.eq_ignore_ascii_case("selection") {
            Ok(PseudoElement::Selection)
        } else if name.eq_ignore_ascii_case("placeholder") {
            Ok(PseudoElement::Placeholder)
        } else if ["-webkit-", "-internal-"]
            .into_iter()
            .filter(|prefix| self.user_agent || *prefix == "-webkit-")
            .any(|prefix| {
                name.len() > prefix.len()
                    && name
                        .get(..prefix.len())
                        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
            })
        {
            Ok(PseudoElement::Webkit(name.to_ascii_lowercase().into()))
        } else {
            Err(
                location.new_custom_error(SelectorParseErrorKind::UnsupportedPseudoClassOrElement(
                    name,
                )),
            )
        }
    }

    fn parse_non_ts_pseudo_class(
        &self,
        location: SourceLocation,
        name: CowRcStr<'i>,
    ) -> Result<PseudoClass, cssparser::ParseError<'i, Self::Error>> {
        if self.user_agent {
            if name.eq_ignore_ascii_case("-internal-list-box") {
                return Ok(PseudoClass::InternalListBox);
            }
            if name.eq_ignore_ascii_case("-internal-multi-select-focus") {
                return Ok(PseudoClass::InternalMultiSelectFocus);
            }
        }
        PseudoClass::ALL
            .iter()
            .find(|(_, n)| name.eq_ignore_ascii_case(n))
            .map(|(pc, _)| pc.clone())
            .ok_or_else(|| {
                location.new_custom_error(SelectorParseErrorKind::UnsupportedPseudoClassOrElement(
                    name,
                ))
            })
    }

    /// `:lang()`, with a comma-separated list of identifiers and strings
    /// (selectors-4; Blink's `CSSSelectorParser::ConsumePseudo` takes the
    /// list too).
    fn parse_non_ts_functional_pseudo_class<'t>(
        &self,
        name: CowRcStr<'i>,
        parser: &mut Parser<'i, 't>,
        _after_part: bool,
    ) -> Result<PseudoClass, cssparser::ParseError<'i, Self::Error>> {
        if !name.eq_ignore_ascii_case("lang") {
            return Err(parser.new_custom_error(
                SelectorParseErrorKind::UnsupportedPseudoClassOrElement(name),
            ));
        }
        let ranges = parser.parse_comma_separated(|input| {
            let location = input.current_source_location();
            let (range, quoted) = match input.next()? {
                cssparser::Token::Ident(range) => (range, false),
                cssparser::Token::QuotedString(range) => (range, true),
                token => return Err(location.new_unexpected_token_error(token.clone())),
            };
            Ok(LangRange {
                range: range.as_ref().to_owned(),
                quoted,
            })
        })?;
        Ok(PseudoClass::Lang(ranges.into()))
    }
}

/// Whether `text` is one complex selector that `@supports selector()`
/// takes (css-conditional 4): not a list, and none of its own
/// `::-webkit-` pseudo-elements unknown to Chrome (Blink's
/// `CSSSelectorParser::ContainsUnknownWebkitPseudoElements`, which looks
/// past no `:is()`).
pub fn supports_selector(text: &str) -> bool {
    let Some(list) = parse_selector_list(text) else {
        return false;
    };
    let [selector] = list.slice() else {
        return false;
    };
    selector
        .iter_raw_match_order()
        .all(|component| match component {
            Component::PseudoElement(PseudoElement::Webkit(name)) => {
                KNOWN_WEBKIT_PSEUDO_ELEMENTS.binary_search(&&**name).is_ok()
            }
            _ => true,
        })
}

/// A selector list, as `querySelector` and `selectorText` take it: `None`
/// unless all of `text` is a valid selector list (the DOM's
/// `SyntaxError`).
pub fn parse_selector_list(text: &str) -> Option<SelectorList<Impl>> {
    let mut input = ParserInput::new(text);
    let mut parser = Parser::new(&mut input);
    let list = parse_list(&SelectorParser::AUTHOR, &mut parser).ok()?;
    parser.expect_exhausted().ok()?;
    Some(list)
}

/// A selector list as `parser` takes it, [`check_list`]ed: what a style
/// rule's prelude and [`parse_selector_list`] parse.
pub fn parse_list<'i>(
    parser: &SelectorParser,
    input: &mut Parser<'i, '_>,
) -> Result<SelectorList<Impl>, cssparser::ParseError<'i, SelectorParseErrorKind<'i>>> {
    let location = input.current_source_location();
    let list = SelectorList::parse(parser, input, ParseRelative::No)?;
    if !check_list(&list) {
        return Err(location.new_custom_error(SelectorParseErrorKind::InvalidState));
    }
    Ok(list)
}

/// What `selectors` takes but Chrome doesn't: a pseudo-class after
/// `::selection` other than `:window-inactive` (`::selection:hover`;
/// `selectors` lets any user action state follow a pseudo-element that
/// takes states).
fn check_list(list: &SelectorList<Impl>) -> bool {
    list.slice().iter().all(|selector| {
        let mut iter = selector.iter();
        loop {
            let compound: Vec<&Component<Impl>> = iter.by_ref().collect();
            let selection = compound
                .iter()
                .any(|c| matches!(c, Component::PseudoElement(PseudoElement::Selection)));
            if selection
                && compound.iter().any(|c| {
                    matches!(c, Component::NonTSPseudoClass(class) if *class != PseudoClass::WindowInactive)
                })
            {
                return false;
            }
            if iter.next_sequence().is_none() {
                return true;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{CssIdent, IdentAtom};

    /// A number is the thread's that set it: another serial (another
    /// thread's) finds none, nor does 0 (no thread), and a copy keeps it.
    #[test]
    fn an_ident_atom_is_one_threads() {
        let atom = IdentAtom::default();
        assert_eq!(atom.get(1), None);
        atom.set(3, 42);
        assert_eq!(atom.get(3), Some(42));
        assert_eq!(atom.get(4), None);
        assert_eq!(atom.get(0), None);
        atom.set(4, 7);
        assert_eq!((atom.get(3), atom.get(4)), (None, Some(7)));
        assert_eq!(atom.clone().get(4), Some(7));
        // Equal by its text alone.
        let numbered = CssIdent::from("a");
        numbered.1.set(1, 5);
        assert_eq!(numbered, CssIdent::from("a"));
    }
}
