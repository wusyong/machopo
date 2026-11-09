//! Declaration blocks (Blink's `CSSPropertyValueSet`): what a style rule
//! or a `style` attribute declares, each longhand and custom property
//! once. The cascade reads them; CSSOM reads, changes and serializes them
//! (the algorithms of Blink's `AbstractPropertySetCSSStyleDeclaration` and
//! `MutableCSSPropertyValueSet`, and `StylePropertySerializer::AsText`).

use crate::css_url_data::UrlContext;
use std::collections::HashSet;
use std::sync::Arc;

use cssparser::{
    Delimiter, ParseError, Parser, ParserInput, match_ignore_ascii_case, parse_important,
};

use crate::properties::{
    Declaration, DeclarationError, LonghandId, Shorthand, Value, parse_declaration,
};
use crate::serialize::{background_computed_to_css, css_wide_name, shorthand_to_css, value_text};
use crate::stylesheet::{ParseLog, parse_declaration_list};

/// One declaration of a block (Blink's `CSSPropertyValue`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PropertyValue {
    pub(crate) declaration: PropertyDeclaration,
    pub(crate) important: bool,
    /// The shorthand that set it left it out, and so set it to its initial
    /// value (Blink's implicit initial value): serializing the shorthand
    /// leaves it out too.
    pub(crate) implicit: bool,
    /// The shorthand that set it, if any (Blink's
    /// `CSSPropertyValueMetadata`).
    pub(crate) shorthand: Option<Shorthand>,
}

/// A declaration as DevTools lists it
/// (`CssPropertyValueSet::inspector_declarations`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectorDeclaration {
    pub name: String,
    pub value: String,
    pub important: bool,
    pub implicit: bool,
    pub shorthand: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PropertyDeclaration {
    Longhand(Declaration),
    /// `--name: value`, with the value as written, without the white space
    /// around it: the cascade computes it (`variables`), CSSOM reads it.
    Custom {
        name: Arc<str>,
        value: Arc<str>,
    },
}

impl PropertyDeclaration {
    fn key(&self) -> PropertyKey<'_> {
        match self {
            Self::Longhand(declaration) => PropertyKey::Longhand(declaration.id),
            Self::Custom { name, .. } => PropertyKey::Custom(name),
        }
    }
}

/// What a block holds at most one of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PropertyKey<'a> {
    Longhand(LonghandId),
    Custom(&'a str),
}

/// A property as CSSOM names it.
#[derive(Debug, Clone, Copy)]
enum PropertyName<'a> {
    Longhand(LonghandId),
    Shorthand(Shorthand),
    Custom(&'a str),
}

impl<'a> PropertyName<'a> {
    /// A custom property's name as given (they are case-sensitive), any
    /// other one ignoring ASCII case; `None` if not one we support.
    fn parse(name: &'a str) -> Option<Self> {
        if is_custom_property_name(name) {
            return Some(Self::Custom(name));
        }
        Shorthand::from_name(name)
            .map(Self::Shorthand)
            .or_else(|| LonghandId::from_name(name).map(Self::Longhand))
    }
}

/// The property a `CSSStyleDeclaration` attribute name stands for (Blink's
/// `CssPropertyInfo`): a camel-cased name (`backgroundColor`) or a dashed
/// one (`background-color`); `None` for a name mixing dashes and capitals,
/// a custom property's, or one of a property we don't support. The result
/// is the property's standard name.
///
/// A name starting with a capital comes out starting with `-`: `WebkitFoo`
/// is `-webkit-foo`, and so is `webkitFoo` (Blink's
/// `CssPropertyInfo::ParseCssPropertyId` adds the dash to a `webkit` it
/// starts with).
pub fn css_property_for_attribute(name: &str) -> Option<&'static str> {
    // `float` has an attribute of its own too (CSSOM, "cssFloat").
    if name == "cssFloat" {
        return Some(LonghandId::Float.name());
    }
    // Only room asked for: a few dashes more than the name.
    let mut dashed = String::with_capacity(name.len().saturating_add(4));
    let (mut dash, mut upper) = (false, false);
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            upper = true;
            dashed.push('-');
            dashed.push(c.to_ascii_lowercase());
        } else {
            dash |= c == '-';
            dashed.push(c);
        }
    }
    if (dash && upper) || is_custom_property_name(&dashed) {
        return None;
    }
    if upper && dashed.starts_with("webkit-") {
        dashed.insert(0, '-');
    }
    // Every capital became a dash and a lowercase letter, so `from_name`
    // ignoring case doesn't let any other spelling through.
    Shorthand::from_name(&dashed)
        .map(Shorthand::name)
        .or_else(|| LonghandId::from_name(&dashed).map(LonghandId::name))
}

/// The camel-cased attribute name of every property we support
/// (`backgroundColor`), each once, by its standard name: what
/// `CSSStyleDeclaration` lists as its named properties (Blink's
/// `NamedPropertyEnumerator`).
pub fn css_property_attributes() -> Vec<String> {
    let camel = |name: &str| {
        let mut out = String::with_capacity(name.len());
        let mut upper = false;
        for c in name.chars() {
            match c {
                '-' => upper = true,
                c if upper => {
                    out.push(c.to_ascii_uppercase());
                    upper = false;
                }
                c => out.push(c),
            }
        }
        out
    };
    let mut names: Vec<String> = LonghandId::ALL
        .iter()
        .map(|id| id.name())
        .chain(Shorthand::ALL.iter().map(|shorthand| shorthand.name()))
        .map(camel)
        .chain(["cssFloat".to_owned()])
        .collect();
    names.sort_unstable();
    names
}

/// `--` and at least one more character (`--` alone is reserved).
pub(crate) fn is_custom_property_name(name: &str) -> bool {
    name.len() > 2 && name.starts_with("--")
}

/// A declaration block (Blink's `CSSPropertyValueSet`, mutable or not:
/// machopo parses a `style` attribute again for every CSSOM change, so one type
/// serves both).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CssPropertyValueSet {
    values: Vec<PropertyValue>,
}

impl CssPropertyValueSet {
    /// A declaration list on its own: a `style` attribute, or `cssText`.
    pub fn parse(css: &str) -> Self {
        Self::parse_logged(css, &mut ParseLog::default())
    }

    /// Property `name`'s `value` alone (`Element.animate()`'s keyframe
    /// values): its longhands, a shorthand's expanded; `None` if `name` is
    /// no property machopo has, or `value` isn't one of its.
    pub fn parse_value(name: &str, value: &str) -> Option<Self> {
        let mut input = ParserInput::new(value);
        let mut parser = Parser::new(&mut input);
        let (shorthand, declarations) = parser
            .parse_entirely(|input| {
                crate::properties::parse_declaration(name, input)
                    .map_err(|_| input.new_custom_error::<(), ()>(()))
            })
            .ok()?;
        // As a declaration in a block has them: a shorthand reads back
        // without the longhands it left out.
        let values = declarations
            .into_iter()
            .map(|(declaration, implicit)| PropertyValue {
                declaration: PropertyDeclaration::Longhand(declaration),
                important: false,
                implicit,
                shorthand,
            })
            .collect();
        Some(Self { values })
    }

    pub fn parse_logged(css: &str, log: &mut ParseLog) -> Self {
        let mut input = ParserInput::new(css);
        parse_declaration_list(&mut Parser::new(&mut input), log)
    }

    /// The block for declarations in source order: for each property, the
    /// last `!important` one, or the last one if none is (the cascade's
    /// order within a block). Like Blink's `FilterProperties`, the normal
    /// ones come first and then the important ones, each in the order of
    /// the ones kept.
    pub(crate) fn from_parsed(parsed: Vec<PropertyValue>) -> Self {
        let mut keep = vec![false; parsed.len()];
        let mut seen = HashSet::new();
        for important in [true, false] {
            for (i, value) in parsed.iter().enumerate().rev() {
                if value.important == important && seen.insert(value.declaration.key()) {
                    keep[i] = true;
                }
            }
        }
        drop(seen);
        let (important, mut values): (Vec<_>, Vec<_>) = parsed
            .into_iter()
            .zip(keep)
            .filter_map(|(value, keep)| keep.then_some(value))
            .partition(|value| value.important);
        values.extend(important);
        Self { values }
    }

    /// Normal declarations, each of a different longhand.
    pub fn from_declarations(declarations: Vec<Declaration>) -> Self {
        let values = declarations
            .into_iter()
            .map(|declaration| PropertyValue {
                declaration: PropertyDeclaration::Longhand(declaration),
                important: false,
                implicit: false,
                shorthand: None,
            })
            .collect();
        Self { values }
    }

    /// Resolves its `url()`s against `base`, those not resolved yet: the
    /// base URL of where it was parsed (Blink's parser context's), which a
    /// later change of the base URL doesn't change.
    pub fn resolve_urls(&mut self, base: &UrlContext) {
        let mut unparsed = Vec::new();
        for value in &mut self.values {
            if let PropertyDeclaration::Longhand(declaration) = &mut value.declaration {
                declaration.resolve_urls(base, &mut unparsed);
            }
        }
    }

    /// Whether it has a `url()` not resolved yet: one parsed with no base
    /// URL at hand.
    pub fn has_unresolved_urls(&self) -> bool {
        self.values.iter().any(|value| match &value.declaration {
            PropertyDeclaration::Longhand(declaration) => declaration.has_unresolved_urls(),
            PropertyDeclaration::Custom { .. } => false,
        })
    }

    /// Itself with its `url()`s as if not resolved yet: for comparing what
    /// was written, whatever base URL it was resolved against.
    pub fn with_urls_as_written(&self) -> Self {
        let mut written = self.clone();
        for value in &mut written.values {
            if let PropertyDeclaration::Longhand(declaration) = &mut value.declaration {
                *declaration = declaration.as_written();
            }
        }
        written
    }

    /// The longhand declarations of one importance, for the cascade.
    pub fn declarations(&self, important: bool) -> impl DoubleEndedIterator<Item = &Declaration> {
        self.values
            .iter()
            .filter_map(move |value| match &value.declaration {
                PropertyDeclaration::Longhand(declaration) if value.important == important => {
                    Some(declaration)
                }
                _ => None,
            })
    }

    /// The custom property declarations of one importance, for the
    /// cascade: each name and value as written.
    pub fn custom_declarations(
        &self,
        important: bool,
    ) -> impl Iterator<Item = (&Arc<str>, &Arc<str>)> {
        self.values
            .iter()
            .filter_map(move |value| match &value.declaration {
                PropertyDeclaration::Custom { name, value: v } if value.important == important => {
                    Some((name, v))
                }
                _ => None,
            })
    }

    pub fn has_important(&self) -> bool {
        self.values.iter().any(|value| value.important)
    }

    /// The number of declarations (`CSSStyleDeclaration.length`):
    /// longhands, not shorthands.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The name of the `index`th declaration (`item()`).
    pub fn item(&self, index: usize) -> Option<&str> {
        self.values
            .get(index)
            .map(|value| match &value.declaration {
                PropertyDeclaration::Longhand(declaration) => declaration.id.name(),
                PropertyDeclaration::Custom { name, .. } => name,
            })
    }

    fn find(&self, key: PropertyKey<'_>) -> Option<&PropertyValue> {
        self.values
            .iter()
            .find(|value| value.declaration.key() == key)
    }

    fn find_longhand(&self, id: LonghandId) -> Option<&PropertyValue> {
        self.find(PropertyKey::Longhand(id))
    }

    /// `getPropertyValue()`: the empty string if the property isn't set
    /// (or, for a shorthand, can't be written from its longhands).
    pub fn property_value(&self, name: &str) -> String {
        let text = match PropertyName::parse(name) {
            None => None,
            Some(PropertyName::Custom(name)) => {
                self.find(PropertyKey::Custom(name))
                    .map(|value| match &value.declaration {
                        PropertyDeclaration::Custom { value, .. } => value.to_string(),
                        PropertyDeclaration::Longhand(_) => unreachable!("found by name"),
                    })
            }
            Some(PropertyName::Longhand(id)) => self.find_longhand(id).map(|value| {
                let PropertyDeclaration::Longhand(declaration) = &value.declaration else {
                    unreachable!("found by id")
                };
                value_text(&declaration.value)
            }),
            Some(PropertyName::Shorthand(shorthand)) => self.shorthand_value(shorthand),
        };
        text.unwrap_or_default()
    }

    /// Its declarations as DevTools lists a block it has no text of
    /// (Blink's `InspectorStyle::PopulateAllProperties` without source
    /// data): each longhand and custom property in `item()`'s order, with its value,
    /// whether it is important, whether a shorthand set it to its initial
    /// value by leaving it out, and the shorthand that set it.
    pub fn inspector_declarations(&self) -> Vec<InspectorDeclaration> {
        self.values
            .iter()
            .map(|value| {
                let (name, text) = match &value.declaration {
                    PropertyDeclaration::Custom { name, value } => {
                        (name.to_string(), value.to_string())
                    }
                    PropertyDeclaration::Longhand(declaration) => (
                        declaration.id.name().to_owned(),
                        value_text(&declaration.value),
                    ),
                };
                InspectorDeclaration {
                    name,
                    value: text,
                    important: value.important,
                    implicit: value.implicit,
                    shorthand: value.shorthand.map(Shorthand::name),
                }
            })
            .collect()
    }

    /// `getPropertyPriority()`: `important`, or the empty string. A
    /// shorthand is important if all of its longhands are.
    pub fn property_priority(&self, name: &str) -> &'static str {
        let important = match PropertyName::parse(name) {
            None => false,
            Some(PropertyName::Custom(name)) => self
                .find(PropertyKey::Custom(name))
                .is_some_and(|value| value.important),
            Some(PropertyName::Longhand(id)) => {
                self.find_longhand(id).is_some_and(|value| value.important)
            }
            Some(PropertyName::Shorthand(shorthand)) => shorthand
                .longhands()
                .into_iter()
                .all(|id| self.find_longhand(id).is_some_and(|value| value.important)),
        };
        if important { "important" } else { "" }
    }

    /// `setProperty()` (after the read-only check): parses `value` for the
    /// property and sets it, or removes the property if `value` is empty.
    /// Whether the block changed: nothing happens for a property we don't
    /// support, a priority other than `important` or empty, or a value we
    /// can't parse.
    pub fn set_property(&mut self, name: &str, value: &str, priority: &str) -> bool {
        self.set_property_resolved(name, value, priority, || None)
    }

    /// `set_property`, the `url()`s of what `value` parses into resolved
    /// against `base`'s (where it is set: an element's document's base URL,
    /// a rule's sheet's), so that the same text set again is the same.
    /// `base` is asked only of a value with a `url()` (most have none, and
    /// an element's is its document's URL parsed).
    pub fn set_property_resolved(
        &mut self,
        name: &str,
        value: &str,
        priority: &str,
        base: impl FnOnce() -> Option<UrlContext>,
    ) -> bool {
        let Some(property) = PropertyName::parse(name) else {
            return false;
        };
        let important = match_ignore_ascii_case! { priority,
            "" => false,
            "important" => true,
            _ => return false,
        };
        if value.is_empty() {
            return self.remove_property(name).1;
        }
        let mut input = ParserInput::new(value);
        let mut input = Parser::new(&mut input);
        if let PropertyName::Custom(name) = property {
            let Ok(value) = parse_custom_property_value::<()>(&mut input) else {
                return false;
            };
            return self.set(PropertyValue {
                declaration: PropertyDeclaration::Custom {
                    name: name.into(),
                    value: value.into(),
                },
                important,
                implicit: false,
                shorthand: None,
            });
        }
        let Ok((shorthand, declarations)) = input.parse_entirely(|input| {
            parse_declaration(name, input)
                .map_err(|e| input.new_custom_error::<_, DeclarationError>(e))
        }) else {
            return false;
        };
        let mut changed = false;
        let mut unparsed = Vec::new();
        let mut base = Some(base);
        let mut resolved: Option<UrlContext> = None;
        for (mut declaration, implicit) in declarations {
            // One with no `url()` resolves to itself.
            if declaration.has_unresolved_urls() {
                if let Some(base) = base.take() {
                    resolved = base();
                }
                if let Some(base) = &resolved {
                    declaration.resolve_urls(base, &mut unparsed);
                }
            }
            changed |= self.set(PropertyValue {
                declaration: PropertyDeclaration::Longhand(declaration),
                important,
                implicit,
                shorthand,
            });
        }
        changed
    }

    /// Sets one declaration where the property's is, or at the end
    /// (Blink's `SetLonghandProperty`). Whether it changed: the same value
    /// and importance is left as it was. A declaration with one after it
    /// of the same logical property group and the other mapping logic
    /// (`margin-left` after `margin-inline-start`) moves to the end, so
    /// that it wins as the last set (CSSOM's "set a CSS declaration";
    /// Blink's `FindInsertionPointForID`).
    fn set(&mut self, value: PropertyValue) -> bool {
        let key = value.declaration.key();
        let Some(i) = self
            .values
            .iter()
            .position(|old| old.declaration.key() == key)
        else {
            self.values.push(value);
            return true;
        };
        let old = &self.values[i];
        let same = (&old.declaration, old.important, old.implicit)
            == (&value.declaration, value.important, value.implicit);
        let overridden = match key {
            PropertyKey::Longhand(id) => self.values[i..].iter().skip(1).any(|later| {
                matches!(later.declaration.key(), PropertyKey::Longhand(other)
                    if id.is_in_same_logical_group_with_other_mapping(other))
            }),
            PropertyKey::Custom(_) => false,
        };
        if overridden {
            self.values.remove(i);
            self.values.push(value);
        } else if same {
            return false;
        } else {
            self.values[i] = value;
        }
        true
    }

    /// `removeProperty()`: removes the property (a shorthand's longhands)
    /// and returns what `getPropertyValue()` gave for it, and whether the
    /// block changed.
    pub fn remove_property(&mut self, name: &str) -> (String, bool) {
        let Some(property) = PropertyName::parse(name) else {
            return (String::new(), false);
        };
        let old = self.property_value(name);
        let before = self.values.len();
        match property {
            PropertyName::Custom(name) => self
                .values
                .retain(|value| value.declaration.key() != PropertyKey::Custom(name)),
            PropertyName::Longhand(id) => self
                .values
                .retain(|value| value.declaration.key() != PropertyKey::Longhand(id)),
            PropertyName::Shorthand(shorthand) => {
                let longhands = shorthand.longhands();
                self.values.retain(|value| {
                    !matches!(value.declaration.key(), PropertyKey::Longhand(id) if longhands.contains(&id))
                });
            }
        }
        (old, self.values.len() != before)
    }

    /// `background`'s resolved value from its longhands' resolved values
    /// (see `shorthand_value` in `computed.rs`).
    pub(crate) fn background_computed_text(&self) -> Option<String> {
        let values: Vec<(&Value, bool)> = Shorthand::Background
            .longhands()
            .into_iter()
            .map(|id| match &self.find_longhand(id)?.declaration {
                PropertyDeclaration::Longhand(declaration) => Some((&declaration.value, false)),
                PropertyDeclaration::Custom { .. } => None,
            })
            .collect::<Option<_>>()?;
        background_computed_to_css(&values)
    }

    /// The shorthand's value made from its longhands (Blink's
    /// `SerializeShorthand`): `None` unless they are all set with the same
    /// importance, and either all the same CSS-wide keyword or none.
    fn shorthand_value(&self, shorthand: Shorthand) -> Option<String> {
        let values: Vec<&PropertyValue> = shorthand
            .longhands()
            .into_iter()
            .map(|id| self.find_longhand(id))
            .collect::<Option<_>>()?;
        if values.iter().any(|v| v.important != values[0].important) {
            return None;
        }
        if shorthand.is_partial() && !values.iter().any(|v| v.shorthand == Some(shorthand)) {
            return None;
        }
        // A value with `var()`: the shorthand's own, if every longhand
        // still has it.
        let unparsed = |v: &PropertyValue| match &v.declaration {
            PropertyDeclaration::Longhand(Declaration {
                value: Value::Unparsed(unparsed),
                ..
            }) => Some(unparsed.clone()),
            _ => None,
        };
        if values.iter().any(|v| unparsed(v).is_some()) {
            let first = unparsed(values[0])?;
            return (first.shorthand == Some(shorthand)
                && values
                    .iter()
                    .all(|v| unparsed(v).is_some_and(|u| Arc::ptr_eq(&u, &first))))
            .then(|| first.css.to_string());
        }
        let declared: Vec<(&Value, bool)> = values
            .iter()
            .map(|v| match &v.declaration {
                PropertyDeclaration::Longhand(declaration) => (&declaration.value, v.implicit),
                PropertyDeclaration::Custom { .. } => unreachable!("found by id"),
            })
            .collect();
        let wide = |value: &Value| match value {
            Value::CssWide(keyword) => Some(*keyword),
            _ => None,
        };
        if let Some(keyword) = wide(declared[0].0) {
            return declared
                .iter()
                .all(|(value, _)| wide(value) == Some(keyword))
                .then(|| css_wide_name(keyword).to_owned());
        }
        if declared.iter().any(|(value, _)| wide(value).is_some()) {
            return None;
        }
        shorthand_to_css(shorthand, &declared)
    }

    /// `cssText` (CSSOM's "serialize a CSS declaration block"; Blink's
    /// `StylePropertySerializer::AsText`): each declaration in order, a
    /// group of longhands as their shorthand where the first of them is,
    /// the larger shorthand first, each tried once. A shorthand's `var()`
    /// value of which some longhands were set since can't be written: the
    /// others are written with empty values, as Chrome does, which don't
    /// parse back (an inline style keeps its declarations, not the text).
    pub fn css_text(&self) -> String {
        let mut shorthands_tried = HashSet::new();
        let mut serialized: HashSet<LonghandId> = HashSet::new();
        let mut parts = Vec::new();
        for value in &self.values {
            let (name, text) = match &value.declaration {
                PropertyDeclaration::Custom { name, value } => {
                    (name.to_string(), value.to_string())
                }
                PropertyDeclaration::Longhand(declaration) => {
                    if serialized.contains(&declaration.id) {
                        continue;
                    }
                    let as_shorthand =
                        shorthands_for(declaration.id)
                            .into_iter()
                            .find_map(|shorthand| {
                                if !shorthands_tried.insert(shorthand) {
                                    return None;
                                }
                                let longhands = shorthand.longhands();
                                if longhands.iter().any(|id| serialized.contains(id)) {
                                    return None;
                                }
                                let text = self.shorthand_value(shorthand)?;
                                serialized.extend(longhands);
                                Some((shorthand.name().to_owned(), text))
                            });
                    as_shorthand.unwrap_or_else(|| {
                        serialized.insert(declaration.id);
                        (
                            declaration.id.name().to_owned(),
                            value_text(&declaration.value),
                        )
                    })
                }
            };
            let important = if value.important { " !important" } else { "" };
            parts.push(format!("{name}: {text}{important};"));
        }
        parts.join(" ")
    }
}

/// The shorthands that set `id`, the one with the most longhands first.
fn shorthands_for(id: LonghandId) -> Vec<Shorthand> {
    let mut shorthands: Vec<(Shorthand, usize)> = Shorthand::ALL
        .into_iter()
        .filter_map(|shorthand| {
            let longhands = shorthand.longhands();
            longhands
                .contains(&id)
                .then_some((shorthand, longhands.len()))
        })
        .collect();
    shorthands.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    shorthands
        .into_iter()
        .map(|(shorthand, _)| shorthand)
        .collect()
}

/// A custom property's value (`<declaration-value>?`): any tokens but bad
/// strings and URLs, unmatched closing brackets, and `;` or `!` outside
/// blocks. Returns its text without the white space around it.
pub(crate) fn parse_custom_property_value<'i, E>(
    input: &mut Parser<'i, '_>,
) -> Result<String, ParseError<'i, E>> {
    let start = input.position();
    crate::variables::check_declaration_value(input, true)?;
    Ok(input
        .slice_from(start)
        .trim_matches(is_css_white_space)
        .to_owned())
}

pub(crate) fn is_css_white_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C')
}

/// One declaration of a declaration list (`name: value [!important]`), as
/// longhands, or a custom property.
pub(crate) fn parse_property<'i>(
    name: &str,
    input: &mut Parser<'i, '_>,
) -> Result<Vec<PropertyValue>, ParseError<'i, DeclarationError>> {
    if is_custom_property_name(name) {
        let value = input.parse_until_before(Delimiter::Bang, parse_custom_property_value)?;
        let important = input.try_parse(parse_important).is_ok();
        input.expect_exhausted()?;
        return Ok(vec![PropertyValue {
            declaration: PropertyDeclaration::Custom {
                name: name.into(),
                value: value.into(),
            },
            important,
            implicit: false,
            shorthand: None,
        }]);
    }
    let (shorthand, declarations) = input.parse_until_before(Delimiter::Bang, |input| {
        parse_declaration(name, input).map_err(|e| input.new_custom_error(e))
    })?;
    let important = input.try_parse(parse_important).is_ok();
    input.expect_exhausted()?;
    Ok(declarations
        .into_iter()
        .map(|(declaration, implicit)| PropertyValue {
            declaration: PropertyDeclaration::Longhand(declaration),
            important,
            implicit,
            shorthand,
        })
        .collect())
}
