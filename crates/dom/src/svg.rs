//! SVG's `SVGAnimatedString` (Blink's, an `SVGAnimatedProperty` tear-off):
//! an SVG element's attribute as a base and an animated value. It holds its
//! element and which attribute; the element's data keeps it, so it is the
//! same object each time (`[SameObject]`). The value is the attribute's
//! (`dom_api`), as there are no animations.
//!
//! `SVGMatrix`, `SVGPoint` and `SVGRect` (Blink's `SVGMatrixTearOff`,
//! `SVGPointTearOff`, `SVGRectTearOff`, interfaces of their own as in
//! Chrome): numbers each, of no element and no realm, as those the
//! geometry members and `createSVG*` make are in Chrome.

use std::cell::Cell;

use elise::{External, Gc, GcObject, JsEdge, Member};
use gfx::svg::MatrixValues;

use crate::{Cx, Node, NodeRef, QualifiedName, sites};

#[derive(GcObject)]
pub struct SvgAnimatedString {
    element: Member<Node>,
    /// The attribute's qualified name (`class` for `className`), as its
    /// number.
    attribute: u32,
    wrapper: JsEdge,
}

impl<'gc> Cx<'_, 'gc> {
    /// `element`'s `className`: the same each time.
    pub fn svg_class_name(&self, element: NodeRef<'gc>) -> Gc<'gc, SvgAnimatedString> {
        let data = self.element_data(element);
        if let Some(animated) = data.svg_class_name.get(self.gc) {
            return animated;
        }
        let animated = self.gc.alloc_in(
            &sites::OTHER,
            SvgAnimatedStringInit {
                element: Some(element),
                attribute: QualifiedName::plain(crate::names::CLASS).raw(),
                wrapper: (),
            },
        );
        data.svg_class_name.set(self.gc, Some(animated));
        animated
    }

    /// The element and the attribute `animated` is of.
    pub fn svg_animated_string_attribute(
        &self,
        animated: Gc<'gc, SvgAnimatedString>,
    ) -> (NodeRef<'gc>, QualifiedName) {
        let animated = animated.into_ref();
        (
            animated
                .element
                .get(self.gc)
                .expect("an animated string's element"),
            QualifiedName::from_raw(animated.attribute),
        )
    }

    pub(crate) fn svg_animated_string_wrapper_edge(
        &self,
        animated: Gc<'gc, SvgAnimatedString>,
    ) -> &'gc JsEdge {
        &animated.into_ref().wrapper
    }
}

#[derive(GcObject)]
pub struct SvgMatrix {
    values: External<Cell<MatrixValues>>,
    wrapper: JsEdge,
}

/// An `SVGPoint`'s `x` and `y`.
#[derive(GcObject)]
pub struct SvgPoint {
    values: External<Cell<[f64; 2]>>,
    wrapper: JsEdge,
}

/// An `SVGRect`'s `x`, `y`, `width` and `height`.
#[derive(GcObject)]
pub struct SvgRect {
    values: External<Cell<[f64; 4]>>,
    wrapper: JsEdge,
}

impl<'gc> Cx<'_, 'gc> {
    pub fn create_svg_matrix(&self, values: MatrixValues) -> Gc<'gc, SvgMatrix> {
        self.gc.alloc_in(
            &sites::OTHER,
            SvgMatrixInit {
                values: External::new(Cell::new(values)),
                wrapper: (),
            },
        )
    }

    pub fn svg_matrix(&self, matrix: Gc<'gc, SvgMatrix>) -> MatrixValues {
        matrix.into_ref().values.get().get()
    }

    pub fn set_svg_matrix(&self, matrix: Gc<'gc, SvgMatrix>, values: MatrixValues) {
        matrix.into_ref().values.get().set(values);
    }

    pub fn create_svg_point(&self, values: [f64; 2]) -> Gc<'gc, SvgPoint> {
        self.gc.alloc_in(
            &sites::OTHER,
            SvgPointInit {
                values: External::new(Cell::new(values)),
                wrapper: (),
            },
        )
    }

    pub fn svg_point(&self, point: Gc<'gc, SvgPoint>) -> [f64; 2] {
        point.into_ref().values.get().get()
    }

    pub fn set_svg_point(&self, point: Gc<'gc, SvgPoint>, values: [f64; 2]) {
        point.into_ref().values.get().set(values);
    }

    pub fn create_svg_rect(&self, values: [f64; 4]) -> Gc<'gc, SvgRect> {
        self.gc.alloc_in(
            &sites::OTHER,
            SvgRectInit {
                values: External::new(Cell::new(values)),
                wrapper: (),
            },
        )
    }

    pub fn svg_rect(&self, rect: Gc<'gc, SvgRect>) -> [f64; 4] {
        rect.into_ref().values.get().get()
    }

    pub fn set_svg_rect(&self, rect: Gc<'gc, SvgRect>, values: [f64; 4]) {
        rect.into_ref().values.get().set(values);
    }

    pub(crate) fn svg_matrix_wrapper_edge(&self, matrix: Gc<'gc, SvgMatrix>) -> &'gc JsEdge {
        &matrix.into_ref().wrapper
    }

    pub(crate) fn svg_point_wrapper_edge(&self, point: Gc<'gc, SvgPoint>) -> &'gc JsEdge {
        &point.into_ref().wrapper
    }

    pub(crate) fn svg_rect_wrapper_edge(&self, rect: Gc<'gc, SvgRect>) -> &'gc JsEdge {
        &rect.into_ref().wrapper
    }
}
