//! Where a sheet's rules and declarations are in its text, for DevTools
//! (Blink's `CSSRuleSourceData`, which `InspectorStyleSheet` builds with a
//! `CSSParserObserver`): the page's parser, told where each rule it keeps
//! is, so that the rules found are the page's, in its order. Byte ranges
//! into the text throughout.

use std::ops::Range;

use cssparser::{ParseErrorKind, Parser, ParserInput, RuleBodyParser};

use crate::properties::DeclarationError;
use crate::stylesheet::{DeclarationListParser, ParseLog, parse_rule_list};

/// A rule the parser kept, where it is in its sheet's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRule {
    pub kind: SourceRuleKind,
    /// Its prelude, white space at either end left out: a style rule's
    /// selector list, a group rule's condition.
    pub prelude: Range<usize>,
    /// A style rule's selectors, each from its first token to its comma
    /// (the last to the prelude's end), as Blink's observer has them.
    pub selectors: Vec<Range<usize>>,
    /// Its block's contents, between its braces.
    pub block: Range<usize>,
    /// A group rule's rules (`@media`'s, `@supports`'), a `@keyframes`'
    /// keyframes, those kept.
    pub rules: Vec<SourceRule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRuleKind {
    Style,
    Media,
    Supports,
    Keyframes,
    /// One of a `@keyframes`' keyframes: its prelude its selectors.
    Keyframe,
}

/// What the parser tells of the rules it keeps, as it parses (Blink's
/// `CSSParserObserver`).
#[derive(Default)]
pub(crate) struct Recorder {
    /// Where the prelude of the rule being parsed starts.
    pub(crate) prelude_start: usize,
    /// The rules kept of the list being parsed.
    pub(crate) rules: Vec<SourceRule>,
    /// The rule whose block was just parsed, for its list to keep if the
    /// rule is.
    pub(crate) done: Option<SourceRule>,
}

impl Recorder {
    /// The rule of `kind` whose prelude started last and whose block is
    /// from `start` (just after its `{`) to `end`.
    pub(crate) fn rule(&self, kind: SourceRuleKind, start: usize, end: usize) -> SourceRule {
        SourceRule {
            kind,
            // To its `{`; trimmed once the text is at hand.
            prelude: self.prelude_start..start.saturating_sub(1),
            selectors: Vec::new(),
            block: start..end,
            rules: Vec::new(),
        }
    }
}

/// Where the rules of `text`, parsed as an author's sheet, are: those the
/// parser keeps, in its order (what `StyleSheetContents::parse` makes of
/// it, rule for rule).
pub fn rules_for_inspector(text: &str) -> Vec<SourceRule> {
    let mut log = ParseLog::default();
    let mut recorder = Recorder::default();
    let mut input = ParserInput::new(text);
    let mut input = Parser::new(&mut input);
    parse_rule_list(&mut input, &mut log, false, Some(&mut recorder));
    let mut rules = recorder.rules;
    finish(text, &mut rules);
    rules
}

/// `rules`' preludes trimmed, a style rule's split into its selectors.
fn finish(text: &str, rules: &mut [SourceRule]) {
    for rule in rules {
        rule.prelude = trimmed(text, rule.prelude.clone());
        if rule.kind == SourceRuleKind::Style {
            rule.selectors = selectors(text, rule.prelude.clone());
        }
        finish(text, &mut rule.rules);
    }
}

/// `range` of `text` without the white space at either end.
fn trimmed(text: &str, range: Range<usize>) -> Range<usize> {
    let Some(slice) = text.get(range.clone()) else {
        return range;
    };
    // Offsets into a text: none past its length, nothing to overflow.
    let start = range
        .start
        .saturating_add(slice.len().saturating_sub(slice.trim_start().len()));
    let end = range
        .end
        .saturating_sub(slice.len().saturating_sub(slice.trim_end().len()));
    start..end.max(start)
}

/// The selectors of the selector list at `prelude` of `text`: split at its
/// commas but those in a function or a block.
fn selectors(text: &str, prelude: Range<usize>) -> Vec<Range<usize>> {
    let Some(slice) = text.get(prelude.clone()) else {
        return Vec::new();
    };
    let mut input = ParserInput::new(slice);
    let mut input = Parser::new(&mut input);
    let mut found = Vec::new();
    loop {
        let start = input.position().byte_index();
        let _: Result<(), cssparser::ParseError<'_, ()>> =
            input.parse_until_before(cssparser::Delimiter::Comma, |input| {
                while input.next_including_whitespace_and_comments().is_ok() {}
                Ok(())
            });
        let end = input.position().byte_index();
        // White space after a comma left out; before one, kept (Blink's).
        let (start, end) = (
            prelude.start.saturating_add(start),
            prelude.start.saturating_add(end),
        );
        let range = trimmed(text, start..end);
        found.push(range.start..end);
        if input.next().is_err() {
            return found;
        }
    }
}

/// A declaration as written in a block (Blink's `CSSPropertySourceData`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDeclaration {
    /// As written.
    pub name: String,
    /// As written, `!important` and all, white space at either end left
    /// out.
    pub value: String,
    pub important: bool,
    /// Whether the parser kept it.
    pub parsed_ok: bool,
    /// A declaration in a comment (`/* color: red; */`), as DevTools writes
    /// one it turns off.
    pub disabled: bool,
    /// The declaration with its `;` (the last one without one, to its
    /// block's end), or the comment.
    pub range: Range<usize>,
}

/// The declarations written in `block` of `text` (a rule's block's
/// contents, or a `style` attribute's whole value), in order, with those in
/// comments between them: each declaration as the page's parser parses it.
pub fn declarations_for_inspector(text: &str, block: Range<usize>) -> Vec<SourceDeclaration> {
    let Some(slice) = text.get(block.clone()) else {
        return Vec::new();
    };
    let mut log = ParseLog::default();
    let mut parser = DeclarationListParser {
        parsed: Vec::new(),
        log: &mut log,
        keyframe: false,
    };
    let mut input = ParserInput::new(slice);
    let mut input = Parser::new(&mut input);
    let mut items = RuleBodyParser::new(&mut input, &mut parser);
    let mut found = Vec::new();
    loop {
        let before = items.input.position().byte_index();
        let result = items.next();
        let after = items.input.position().byte_index();
        // What the parser skipped to it: white space, `;`s and comments.
        let start = skipped(slice, before, after, |range| {
            if let Some(declaration) = commented(slice, range) {
                found.push(declaration);
            }
        });
        let Some(result) = result else {
            break;
        };
        let range = start..after;
        let Some((name, value)) = slice[range.clone()].split_once(':') else {
            continue;
        };
        let value = value.trim().trim_end_matches(';').trim_end();
        found.push(SourceDeclaration {
            name: name.trim().to_owned(),
            value: value.to_owned(),
            important: is_important(value),
            parsed_ok: result.is_ok(),
            disabled: false,
            range,
        });
    }
    for declaration in &mut found {
        declaration.range = block.start.saturating_add(declaration.range.start)
            ..block.start.saturating_add(declaration.range.end);
    }
    found
}

/// Skips what the parser skips from `from` in `text` (to `to` at most):
/// white space, `;`s and comments, each comment's range given to `comment`.
/// Where it stopped.
fn skipped(text: &str, from: usize, to: usize, mut comment: impl FnMut(Range<usize>)) -> usize {
    let mut at = from;
    while at < to {
        let rest = &text[at..to];
        if let Some(after) = rest.strip_prefix("/*") {
            // Past the `*/`, if there is one.
            let end = after
                .find("*/")
                .map_or(to, |end| at.saturating_add(end).saturating_add(4));
            comment(at..end);
            at = end;
        } else if rest.starts_with(|c: char| c.is_ascii_whitespace() || c == ';') {
            at = at.saturating_add(1);
        } else {
            break;
        }
    }
    at
}

/// The comment at `range` of `text` as a declaration DevTools turned off: a
/// declaration of a property the parser knows, valid or not; none for
/// another comment.
fn commented(text: &str, range: Range<usize>) -> Option<SourceDeclaration> {
    let inner = text
        .get(range.start.saturating_add(2)..range.end.checked_sub(2)?)?
        .trim();
    let (name, value) = inner.split_once(':')?;
    let name = name.trim();
    let value = value.trim().trim_end_matches(';').trim_end();
    let mut log = ParseLog::default();
    let mut parser = DeclarationListParser {
        parsed: Vec::new(),
        log: &mut log,
        keyframe: false,
    };
    let declaration = format!("{name}: {value}");
    let mut input = ParserInput::new(&declaration);
    let mut input = Parser::new(&mut input);
    let mut items = RuleBodyParser::new(&mut input, &mut parser);
    if let Err((error, _)) = items.next()?
        && matches!(
            error.kind,
            ParseErrorKind::Custom(DeclarationError::UnsupportedProperty)
        )
    {
        return None;
    }
    // One declaration, nothing after it.
    if items.next().is_some() {
        return None;
    }
    // Whether its value parses isn't told of one turned off (Blink's).
    Some(SourceDeclaration {
        name: name.to_owned(),
        value: value.to_owned(),
        important: is_important(value),
        parsed_ok: true,
        disabled: true,
        range,
    })
}

/// Whether a value as written ends in `!important`.
fn is_important(value: &str) -> bool {
    value
        .rsplit_once('!')
        .is_some_and(|(_, after)| after.trim().eq_ignore_ascii_case("important"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at<'t>(text: &'t str, range: &Range<usize>) -> &'t str {
        &text[range.clone()]
    }

    /// The rules kept, in the parser's order, nested in their group rules
    /// (a `@keyframes`' keyframes in it): each prelude, selector and block
    /// where it is; a rule the parser drops (an at-rule it doesn't know, a
    /// selector it can't parse) none.
    #[test]
    fn rules_are_found_where_they_are() {
        let text = "/* c */ .a ,  b > i { color: red }\n@font-face { font-family: x }\n\
                    @media  (min-width: 1px) {\n  :unknown-thing { top: 0 }\n  @supports (display: grid) { p:is(.x, .y) {} }\n}\n\
                    @keyframes k { from { top: 0 } }\n.z{}";
        let rules = rules_for_inspector(text);
        let kinds: Vec<SourceRuleKind> = rules.iter().map(|rule| rule.kind).collect();
        use SourceRuleKind::*;
        assert_eq!(kinds, [Style, Media, Keyframes, Style]);
        let a = &rules[0];
        assert_eq!(at(text, &a.prelude), ".a ,  b > i");
        let selectors: Vec<&str> = a.selectors.iter().map(|range| at(text, range)).collect();
        // The first to its comma.
        assert_eq!(selectors, [".a ", "b > i"]);
        assert_eq!(at(text, &a.block), " color: red ");
        let media = &rules[1];
        assert_eq!(at(text, &media.prelude), "(min-width: 1px)");
        assert_eq!(media.rules.len(), 1);
        let supports = &media.rules[0];
        assert_eq!(supports.kind, Supports);
        assert_eq!(at(text, &supports.prelude), "(display: grid)");
        // A comma in a function doesn't split.
        let nested = &supports.rules[0];
        let selectors: Vec<&str> = nested
            .selectors
            .iter()
            .map(|range| at(text, range))
            .collect();
        assert_eq!(selectors, ["p:is(.x, .y)"]);
        assert_eq!(at(text, &nested.block), "");
        assert!(at(text, &media.block).ends_with("{} }\n"));
        assert_eq!(at(text, &rules[2].prelude), "k");
        // Its keyframes.
        let keyframes = &rules[2].rules;
        assert_eq!(keyframes.len(), 1);
        assert_eq!(keyframes[0].kind, Keyframe);
        assert_eq!(at(text, &keyframes[0].prelude), "from");
        assert_eq!(at(text, &keyframes[0].block), " top: 0 ");
        assert_eq!(at(text, &rules[3].prelude), ".z");
        // As many as the page's parse keeps.
        let contents = crate::StyleSheetContents::parse(text, None);
        assert_eq!(contents.rules().len(), rules.len());
    }

    /// Each declaration as written, in order: its value with `!important`,
    /// those the parser drops (a property it doesn't know, a value it can't
    /// parse) not kept, those in comments turned off; the last one without
    /// a `;`.
    #[test]
    fn declarations_are_found_as_written() {
        let text = ".a {\n  margin: 1px 2px;\n  color: red ! IMPORTANT;\n  foo: bar;;\n  color: blu;\n  \
                    /* gap: 1px; */ /* not one */ /* foo: 1 */\n  --x: { a };\n  top: 0 }";
        let rules = rules_for_inspector(text);
        let found = declarations_for_inspector(text, rules[0].block.clone());
        let listed: Vec<(&str, &str, bool, bool, bool, &str)> = found
            .iter()
            .map(|declaration| {
                (
                    declaration.name.as_str(),
                    declaration.value.as_str(),
                    declaration.important,
                    declaration.parsed_ok,
                    declaration.disabled,
                    at(text, &declaration.range),
                )
            })
            .collect();
        assert_eq!(
            listed,
            [
                ("margin", "1px 2px", false, true, false, "margin: 1px 2px;"),
                (
                    "color",
                    "red ! IMPORTANT",
                    true,
                    true,
                    false,
                    "color: red ! IMPORTANT;"
                ),
                ("foo", "bar", false, false, false, "foo: bar;"),
                ("color", "blu", false, false, false, "color: blu;"),
                ("gap", "1px", false, true, true, "/* gap: 1px; */"),
                ("--x", "{ a }", false, true, false, "--x: { a };"),
                // The last to the block's end.
                ("top", "0", false, true, false, "top: 0 "),
            ]
        );
        // A `style` attribute's: all of it.
        let inline = "color: red;  zz: 1";
        let found = declarations_for_inspector(inline, 0..inline.len());
        let ranges: Vec<&str> = found
            .iter()
            .map(|declaration| at(inline, &declaration.range))
            .collect();
        assert_eq!(ranges, ["color: red;", "zz: 1"]);
    }

    /// Non-ASCII text before: byte ranges still.
    #[test]
    fn ranges_are_bytes() {
        let text = "/* é */ .é { content: 'ü' }";
        let rules = rules_for_inspector(text);
        assert_eq!(at(text, &rules[0].prelude), ".é");
        let found = declarations_for_inspector(text, rules[0].block.clone());
        assert_eq!(at(text, &found[0].range), "content: 'ü' ");
    }
}
