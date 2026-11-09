//! An HTML element's presentational hints in the DOM, and an SVG
//! element's presentation attributes: `style_resolver`'s
//! (`style_resolver::html_hints`, `style_resolver::svg_hint`, which parse
//! them), its attributes read here. They
//! cascade as author declarations with specificity 0, before every author
//! rule. An SVG attribute's declarations are kept by its name and value
//! (`style_resolver::PresentationAttributeCache`), so elements alike parse each
//! once; a path's `d` isn't (rarely the same twice).

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use css::CssPropertyValueSet;
use dom::{ContentEditable, Cx, NameId, NodeRef, QualifiedName, names};
use style_resolver::{HintAttribute, HintElement, HintSource};

/// An element of the DOM as its hints read it.
struct Source<'c, 'a, 'gc> {
    cx: &'c Cx<'a, 'gc>,
    element: NodeRef<'gc>,
}

/// An attribute's local name: those not among the fixed names are found
/// in the thread's names, none if no element has had one.
fn name(attribute: HintAttribute) -> Option<NameId> {
    use names::local;
    Some(match attribute {
        HintAttribute::Align => local::ALIGN,
        HintAttribute::Background => local::BACKGROUND,
        HintAttribute::Bgcolor => local::BGCOLOR,
        HintAttribute::Border => local::BORDER,
        HintAttribute::Bordercolor => return names::find("bordercolor"),
        HintAttribute::Cellpadding => local::CELLPADDING,
        HintAttribute::Cellspacing => local::CELLSPACING,
        HintAttribute::Color => local::COLOR,
        HintAttribute::Face => local::FACE,
        HintAttribute::Frameborder => local::FRAMEBORDER,
        HintAttribute::Height => local::HEIGHT,
        HintAttribute::Leftmargin => return names::find("leftmargin"),
        HintAttribute::Marginheight => local::MARGINHEIGHT,
        HintAttribute::Marginwidth => local::MARGINWIDTH,
        HintAttribute::Noshade => local::NOSHADE,
        HintAttribute::Nowrap => local::NOWRAP,
        HintAttribute::Reversed => local::REVERSED,
        HintAttribute::Size => local::SIZE,
        HintAttribute::Start => local::START,
        HintAttribute::Text => local::TEXT,
        HintAttribute::Topmargin => return names::find("topmargin"),
        HintAttribute::Type => local::TYPE,
        HintAttribute::Valign => local::VALIGN,
        HintAttribute::Value => local::VALUE,
        HintAttribute::Width => local::WIDTH,
    })
}

/// The attributes any element's hints are of (each as `name` finds it,
/// interned, and `contenteditable`, any HTML element's): what the DOM
/// notes changes of whatever the rules.
pub(crate) fn attribute_names() -> impl Iterator<Item = NameId> {
    use HintAttribute::*;
    [
        Align,
        Background,
        Bgcolor,
        Border,
        Bordercolor,
        Cellpadding,
        Cellspacing,
        Color,
        Face,
        Frameborder,
        Height,
        Leftmargin,
        Marginheight,
        Marginwidth,
        Noshade,
        Nowrap,
        Reversed,
        Size,
        Start,
        Text,
        Topmargin,
        Type,
        Valign,
        Value,
        Width,
    ]
    .into_iter()
    .map(|attribute| match attribute {
        Bordercolor => names::intern("bordercolor"),
        Leftmargin => names::intern("leftmargin"),
        Topmargin => names::intern("topmargin"),
        other => name(other).expect("a fixed name"),
    })
    .chain([names::local::CONTENTEDITABLE])
}

impl HintSource for Source<'_, '_, '_> {
    fn attribute(&self, attribute: HintAttribute) -> Option<String> {
        self.cx
            .attribute_value_by_id(self.element, name(attribute)?)
            .map(|value| value.borrow().to_string())
    }

    fn in_order(&self, attributes: &[HintAttribute]) -> Vec<(HintAttribute, String)> {
        let wanted: Vec<_> = attributes
            .iter()
            .filter_map(|&attribute| Some((QualifiedName::plain(name(attribute)?), attribute)))
            .collect();
        (0..self.cx.attribute_count(self.element))
            .filter_map(|at| {
                let name = self.cx.attribute_name_at(self.element, at)?;
                let &(_, attribute) = wanted.iter().find(|(wanted, _)| *wanted == name)?;
                let value = self.cx.attribute_value_at(self.element, at)?;
                Some((attribute, value.borrow().to_string()))
            })
            .collect()
    }

    fn cell_table(&self) -> Option<Self> {
        use names::local;
        let cx = self.cx;
        let is = |node, locals: &[NameId]| {
            locals
                .iter()
                .any(|&local| cx.qualified_name(node) == Some(QualifiedName::html(local)))
        };
        let row = cx
            .parent(self.element)
            .filter(|&row| is(row, &[local::TR]))?;
        let mut parent = cx.parent(row)?;
        if is(parent, &[local::THEAD, local::TBODY, local::TFOOT]) {
            parent = cx.parent(parent)?;
        }
        is(parent, &[local::TABLE]).then_some(Self {
            cx,
            element: parent,
        })
    }
}

/// What of `element` its hints are of, if it is an HTML element that may
/// have some.
fn hint_element<'gc>(cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> Option<HintElement> {
    use names::local;
    let name = cx.qualified_name(element)?;
    if name.namespace() != names::HTML_NAMESPACE {
        return None;
    }
    Some(match name.local() {
        local::OL => HintElement::Ol,
        local::UL => HintElement::Ul,
        local::LI => HintElement::Li,
        local::TABLE => HintElement::Table,
        local::TD => HintElement::Td,
        local::TH => HintElement::Th,
        local::TR => HintElement::Tr,
        local::THEAD => HintElement::Thead,
        local::TBODY => HintElement::Tbody,
        local::TFOOT => HintElement::Tfoot,
        local::COL => HintElement::Col,
        local::COLGROUP => HintElement::Colgroup,
        local::BODY => HintElement::Body,
        local::FONT => HintElement::Font,
        local::HR => HintElement::Hr,
        local::IMG => HintElement::Img,
        local::IFRAME => HintElement::Iframe,
        _ => return None,
    })
}

/// `element`'s presentational hints, as declarations, and a digest of the
/// attributes they are of (the matched properties cache's key); `None` if
/// it has none. Worked out each time it is styled, as few elements have
/// them (a cell's are its table's too).
pub(crate) fn declarations<'gc>(
    cx: &Cx<'_, 'gc>,
    element: NodeRef<'gc>,
    base: Option<&css::UrlContext>,
    cache: &mut style_resolver::PresentationAttributeCache<NameId>,
) -> Option<(Arc<CssPropertyValueSet>, u64)> {
    let mut hints = match hint_element(cx, element) {
        Some(kind) => style_resolver::html_hints(kind, &Source { cx, element }),
        None => svg_declarations(cx, element, cache).unwrap_or_default(),
    };
    // Blink's `HTMLElement::CollectStyleForPresentationAttribute`: an
    // editing host's long words wrap (its `-webkit-user-modify` and
    // `-webkit-line-break` machopo hasn't).
    if matches!(
        cx.content_editable(element),
        Some(ContentEditable::True | ContentEditable::PlaintextOnly)
    ) {
        hints.push(css::properties::Declaration {
            id: css::properties::LonghandId::OverflowWrap,
            value: css::properties::Value::OverflowWrap(computed_style::OverflowWrap::BreakWord),
        });
    }
    (!hints.is_empty()).then(|| {
        let mut set = CssPropertyValueSet::from_declarations(hints);
        // A `background`'s `url()` against the document's URL.
        if set.has_unresolved_urls()
            && let Some(base) = base
        {
            set.resolve_urls(base);
        }
        (Arc::new(set), digest(cx, element))
    })
}

/// An SVG element's presentation attributes as declarations (`None` for
/// an element that isn't SVG's), each attribute in no namespace that is
/// one of its (`style_resolver::svg_hint`) by way of `cache`.
fn svg_declarations<'gc>(
    cx: &Cx<'_, 'gc>,
    element: NodeRef<'gc>,
    cache: &mut style_resolver::PresentationAttributeCache<NameId>,
) -> Option<Vec<css::properties::Declaration>> {
    let name = cx.qualified_name(element)?;
    if name.namespace() != names::SVG_NAMESPACE {
        return None;
    }
    let local = names::string(name.local());
    let local = local.to_str();
    let mut out = Vec::new();
    for at in 0..cx.attribute_count(element) {
        let Some(attribute) = cx.attribute_name_at(element, at) else {
            continue;
        };
        if attribute.namespace() != names::NONE {
            continue;
        }
        let attribute_name = names::string(attribute.local());
        let attribute_name = attribute_name.to_str();
        let Some(hint) = style_resolver::svg_hint(&local, &attribute_name) else {
            continue;
        };
        let Some(value) = cx.attribute_value_at(element, at) else {
            continue;
        };
        let value = value.borrow().to_str().into_owned();
        let parse = || style_resolver::svg_hint_declarations(hint, &attribute_name, &value);
        if hint == style_resolver::SvgHint::D {
            out.extend(parse());
        } else {
            out.extend_from_slice(&cache.get(&attribute.local(), &value, parse));
        }
    }
    Some(out)
}

/// A digest of the attributes `element`'s hints are of: all of them but
/// its `id`, `class` and `style`.
fn digest<'gc>(cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    cx.qualified_name(element).hash(&mut hasher);
    for at in 0..cx.attribute_count(element) {
        let Some(name) = cx.attribute_name_at(element, at) else {
            continue;
        };
        if [names::ID, names::CLASS, names::local::STYLE]
            .into_iter()
            .any(|local| name == QualifiedName::plain(local))
        {
            continue;
        }
        name.hash(&mut hasher);
        if let Some(value) = cx.attribute_value_at(element, at) {
            value.borrow().to_str().hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Whether `changes` to `element`'s attributes change the hints of
/// elements below it: a table's `cellpadding`, `border` and
/// `bordercolor`, which its cells take.
pub(crate) fn for_descendants<'gc>(
    cx: &Cx<'_, 'gc>,
    element: NodeRef<'gc>,
    changes: &dom::AttributeChanges,
) -> bool {
    hint_element(cx, element) == Some(HintElement::Table)
        && changes.attributes.iter().any(|&changed| {
            [
                HintAttribute::Cellpadding,
                HintAttribute::Border,
                HintAttribute::Bordercolor,
            ]
            .into_iter()
            .any(|attribute| name(attribute) == Some(changed))
        })
}

/// A document's link colors: its body element's `link` and `alink`, legacy
/// colors (Blink's `HTMLBodyElement` sets `TextLinkColors` from them),
/// else the defaults. `vlink` is the visited links', which machopo never has.
pub(crate) fn text_link_colors<'gc>(
    cx: &Cx<'_, 'gc>,
    document: NodeRef<'gc>,
) -> css::values::TextLinkColors {
    use names::local;
    let children =
        |node| std::iter::successors(cx.first_child(node), |&child| cx.next_sibling(child));
    let is = |node, local| cx.qualified_name(node) == Some(QualifiedName::html(local));
    let body = children(document)
        .find(|&node| is(node, local::HTML))
        .and_then(|html| children(html).find(|&node| is(node, local::BODY)));
    let defaults = css::values::TextLinkColors::DEFAULT;
    let Some(body) = body else {
        return defaults;
    };
    let color = |name| {
        cx.attribute_value_by_id(body, name)
            .and_then(|value| css::values::parse_legacy_color(&value.borrow().to_str()))
    };
    css::values::TextLinkColors {
        link: color(local::LINK).unwrap_or(defaults.link),
        active_link: color(local::ALINK).unwrap_or(defaults.active_link),
    }
}
