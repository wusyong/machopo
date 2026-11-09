//! What capturing an `<svg>`'s content (`svg`, `svg_text`, `svg_gradient`)
//! reads of the DOM ([`SvgElement`]): its nodes, their names among the SVG
//! elements the capture knows ([`SvgName`]), the attributes it reads
//! ([`SvgAttr`], each a name's number: none is looked up by its string),
//! their computed styles.

use std::sync::Arc;

use computed_style::ComputedStyle;
use dom::{NameId, NodeKind, NodeRef, names};
use gfx::NodeId;

use crate::tree_builder::Reader;

/// The SVG elements the capture draws or looks into, by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgName {
    Defs,
    G,
    A,
    Switch,
    Svg,
    Use,
    Symbol,
    Rect,
    Circle,
    Ellipse,
    Line,
    Polyline,
    Polygon,
    Path,
    Text,
    Tspan,
    Stop,
    LinearGradient,
    RadialGradient,
    ClipPath,
}

/// The attributes the capture reads (in no namespace, but for `XmlSpace`
/// and `XlinkHref`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgAttr {
    Id,
    ViewBox,
    PreserveAspectRatio,
    SystemLanguage,
    RequiredExtensions,
    Width,
    Height,
    Points,
    X1,
    Y1,
    X2,
    Y2,
    X,
    Y,
    Dx,
    Dy,
    Rotate,
    /// `xml:space`.
    XmlSpace,
    Href,
    /// `xlink:href`.
    XlinkHref,
    Offset,
    GradientUnits,
    GradientTransform,
    SpreadMethod,
    Cx,
    Cy,
    R,
    Fx,
    Fy,
    Fr,
    ClipPathUnits,
}

/// An element (or text) of the DOM as the SVG capture and the geometry
/// queries read it, its style as last styled.
#[derive(Clone, Copy)]
pub struct SvgElement<'c, 'gc> {
    pub(crate) dom: Reader<'c, 'gc>,
    pub(crate) node: NodeRef<'gc>,
}

/// `SvgName`'s elements, by their local names' numbers.
const SVG_NAMES: [(NameId, SvgName); 20] = {
    use names::local;
    [
        (local::DEFS, SvgName::Defs),
        (local::G, SvgName::G),
        (local::A, SvgName::A),
        (local::SWITCH, SvgName::Switch),
        (local::SVG, SvgName::Svg),
        (local::USE, SvgName::Use),
        (local::SYMBOL, SvgName::Symbol),
        (local::RECT, SvgName::Rect),
        (local::CIRCLE, SvgName::Circle),
        (local::ELLIPSE, SvgName::Ellipse),
        (local::LINE, SvgName::Line),
        (local::POLYLINE, SvgName::Polyline),
        (local::POLYGON, SvgName::Polygon),
        (local::PATH, SvgName::Path),
        (local::TEXT, SvgName::Text),
        (local::TSPAN, SvgName::Tspan),
        (local::STOP, SvgName::Stop),
        (local::LINEAR_GRADIENT, SvgName::LinearGradient),
        (local::RADIAL_GRADIENT, SvgName::RadialGradient),
        (local::CLIP_PATH, SvgName::ClipPath),
    ]
};

/// `name`'s local name's number (in no namespace, but for `xml:space` and
/// `xlink:href`, which `SvgElement::attribute` reads by namespace).
fn svg_attribute(name: SvgAttr) -> NameId {
    use names::local;
    match name {
        SvgAttr::Id => local::ID,
        SvgAttr::ViewBox => local::VIEW_BOX,
        SvgAttr::PreserveAspectRatio => local::PRESERVE_ASPECT_RATIO,
        SvgAttr::SystemLanguage => local::SYSTEM_LANGUAGE,
        SvgAttr::RequiredExtensions => local::REQUIRED_EXTENSIONS,
        SvgAttr::Width => local::WIDTH,
        SvgAttr::Height => local::HEIGHT,
        SvgAttr::Points => local::POINTS,
        SvgAttr::X1 => local::X1,
        SvgAttr::Y1 => local::Y1,
        SvgAttr::X2 => local::X2,
        SvgAttr::Y2 => local::Y2,
        SvgAttr::X => local::X,
        SvgAttr::Y => local::Y,
        SvgAttr::Dx => local::DX,
        SvgAttr::Dy => local::DY,
        SvgAttr::Rotate => local::ROTATE,
        SvgAttr::XmlSpace => local::SPACE,
        SvgAttr::Href | SvgAttr::XlinkHref => local::HREF,
        SvgAttr::Offset => local::OFFSET,
        SvgAttr::GradientUnits => local::GRADIENT_UNITS,
        SvgAttr::GradientTransform => local::GRADIENT_TRANSFORM,
        SvgAttr::SpreadMethod => local::SPREAD_METHOD,
        SvgAttr::Cx => local::CX,
        SvgAttr::Cy => local::CY,
        SvgAttr::R => local::R,
        SvgAttr::Fx => local::FX,
        SvgAttr::Fy => local::FY,
        SvgAttr::Fr => local::FR,
        SvgAttr::ClipPathUnits => local::CLIP_PATH_UNITS,
    }
}

impl<'gc> SvgElement<'_, 'gc> {
    pub(crate) fn id(self) -> NodeId {
        self.dom.id(self.node)
    }

    pub(crate) fn is_element(self) -> bool {
        self.dom.is_element(self.node)
    }

    pub(crate) fn is_text(self) -> bool {
        self.dom.dom.kind(self.node) == NodeKind::Text
    }

    pub(crate) fn is_svg_element(self) -> bool {
        self.dom.is_svg(self.node)
    }

    pub(crate) fn svg_name(self) -> Option<SvgName> {
        let name = self.dom.dom.qualified_name(self.node)?;
        SVG_NAMES
            .iter()
            .find(|(local, _)| name.is(names::SVG_NAMESPACE, *local))
            .map(|(_, svg_name)| *svg_name)
    }

    pub(crate) fn attribute(self, name: SvgAttr) -> Option<String> {
        let dom = self.dom.dom;
        let value = match name {
            SvgAttr::XmlSpace => dom.attribute_value_ns(
                self.node,
                Some("http://www.w3.org/XML/1998/namespace"),
                "space",
            ),
            SvgAttr::XlinkHref => {
                dom.attribute_value_ns(self.node, Some("http://www.w3.org/1999/xlink"), "href")
            }
            _ => dom.attribute_value_by_id(self.node, svg_attribute(name)),
        }?;
        Some(value.borrow().to_string())
    }

    pub(crate) fn computed_style(self) -> Option<Arc<ComputedStyle>> {
        self.dom.style(self.node)
    }

    pub(crate) fn text(self) -> Option<String> {
        self.dom
            .dom
            .text_data(self.node)
            .map(|data| data.borrow().to_string())
    }

    pub(crate) fn parent(self) -> Option<Self> {
        Some(Self {
            node: self.dom.dom.parent(self.node)?,
            ..self
        })
    }

    pub(crate) fn first_child(self) -> Option<Self> {
        Some(Self {
            node: self.dom.dom.first_child(self.node)?,
            ..self
        })
    }

    pub(crate) fn next_sibling(self) -> Option<Self> {
        Some(Self {
            node: self.dom.dom.next_sibling(self.node)?,
            ..self
        })
    }

    pub(crate) fn node_by_id(self, id: NodeId) -> Option<Self> {
        Some(Self {
            node: self.dom.dom.node_by_id(id)?,
            ..self
        })
    }

    pub(crate) fn use_copy(self) -> Option<Self> {
        Some(Self {
            node: self.dom.dom.use_copy(self.node)?,
            ..self
        })
    }

    /// Its children, in order.
    pub(crate) fn children(self) -> impl Iterator<Item = Self> {
        std::iter::successors(self.first_child(), |child| child.next_sibling())
    }
}
