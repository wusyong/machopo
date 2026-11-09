//! SVG's geometry members, as Chrome has them: `SVGMatrix`, `SVGPoint` and
//! `SVGRect` (interfaces of their own, where the specs make them aliases of
//! `DOMMatrix`, `DOMPoint` and `DOMRect`; their numbers WebIDL's `double`
//! and `float`, not `unrestricted`), `SVGSVGElement`'s `createSVG*`,
//! `SVGGraphicsElement`'s `getBBox()`, `getCTM()` and `getScreenCTM()`,
//! `SVGGeometryElement`'s and `SVGTextContentElement`'s methods. The
//! element's read the layout, brought up to date first (Blink's
//! `SVGGraphicsElement` after `UpdateStyleAndLayoutForNode`); what they
//! compute is `gfx::svg`'s.

use base::numerics::f32_from_f64;
use dom::{NodeRef, SvgMatrix, SvgPoint, SvgRect, names};
use elise::Gc;
use gfx::svg::{MatrixValues, SvgTextChar, matrix, text, webidl_float};
use gfx::{Path, PointF, RectF, SvgGeometry, Transform};

use crate::exception::{DomException, ExceptionName};
use crate::interfaces::DOMPointInit;
use crate::{ApiCx, ApiError};

fn exception(name: ExceptionName, message: &str) -> ApiError {
    ApiError::Dom(DomException {
        name,
        message: message.to_owned(),
    })
}

/// A new matrix: `this` × `by`.
fn then<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgMatrix>,
    by: MatrixValues,
) -> Result<Gc<'gc, SvgMatrix>, ApiError> {
    Ok(cx.create_svg_matrix(matrix::multiply(cx.svg_matrix(this), by)))
}

macro_rules! matrix_attributes {
    ($($get:ident, $set:ident, $index:literal;)*) => {
        $(
            pub(crate) fn $get<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, SvgMatrix>) -> Result<f64, ApiError> {
                Ok(cx.svg_matrix(this)[$index])
            }

            pub(crate) fn $set<'gc>(
                cx: &ApiCx<'_, 'gc>,
                this: Gc<'gc, SvgMatrix>,
                value: f64,
            ) -> Result<(), ApiError> {
                let mut values = cx.svg_matrix(this);
                values[$index] = value;
                cx.set_svg_matrix(this, values);
                Ok(())
            }
        )*
    };
}

matrix_attributes! {
    a, set_a, 0;
    b, set_b, 1;
    c, set_c, 2;
    d, set_d, 3;
    e, set_e, 4;
    f, set_f, 5;
}

pub(crate) fn multiply<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgMatrix>,
    second: Gc<'gc, SvgMatrix>,
) -> Result<Gc<'gc, SvgMatrix>, ApiError> {
    then(cx, this, cx.svg_matrix(second))
}

pub(crate) fn inverse<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgMatrix>,
) -> Result<Gc<'gc, SvgMatrix>, ApiError> {
    let values = matrix::inverse(cx.svg_matrix(this))
        .ok_or_else(|| exception(ExceptionName::InvalidState, "The matrix is not invertible."))?;
    Ok(cx.create_svg_matrix(values))
}

pub(crate) fn translate<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgMatrix>,
    x: f64,
    y: f64,
) -> Result<Gc<'gc, SvgMatrix>, ApiError> {
    then(cx, this, matrix::translate(x, y))
}

pub(crate) fn scale<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgMatrix>,
    factor: f64,
) -> Result<Gc<'gc, SvgMatrix>, ApiError> {
    then(cx, this, matrix::scale(factor))
}

pub(crate) fn scale_non_uniform<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgMatrix>,
    x: f64,
    y: f64,
) -> Result<Gc<'gc, SvgMatrix>, ApiError> {
    then(cx, this, matrix::scale_non_uniform(x, y))
}

pub(crate) fn rotate<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgMatrix>,
    angle: f64,
) -> Result<Gc<'gc, SvgMatrix>, ApiError> {
    then(cx, this, matrix::rotate(angle))
}

pub(crate) fn rotate_from_vector<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgMatrix>,
    x: f64,
    y: f64,
) -> Result<Gc<'gc, SvgMatrix>, ApiError> {
    let by = matrix::rotate_from_vector(x, y)
        .ok_or_else(|| exception(ExceptionName::InvalidAccess, "Arguments cannot be zero."))?;
    then(cx, this, by)
}

pub(crate) fn flip_x<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgMatrix>,
) -> Result<Gc<'gc, SvgMatrix>, ApiError> {
    then(cx, this, matrix::FLIP_X)
}

pub(crate) fn flip_y<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgMatrix>,
) -> Result<Gc<'gc, SvgMatrix>, ApiError> {
    then(cx, this, matrix::FLIP_Y)
}

pub(crate) fn skew_x<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgMatrix>,
    angle: f64,
) -> Result<Gc<'gc, SvgMatrix>, ApiError> {
    then(cx, this, matrix::skew_x(angle))
}

pub(crate) fn skew_y<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgMatrix>,
    angle: f64,
) -> Result<Gc<'gc, SvgMatrix>, ApiError> {
    then(cx, this, matrix::skew_y(angle))
}

pub(crate) fn point_x<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, SvgPoint>) -> Result<f64, ApiError> {
    Ok(cx.svg_point(this)[0])
}

pub(crate) fn point_y<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, SvgPoint>) -> Result<f64, ApiError> {
    Ok(cx.svg_point(this)[1])
}

pub(crate) fn set_point_x<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgPoint>,
    value: f64,
) -> Result<(), ApiError> {
    let [_, y] = cx.svg_point(this);
    cx.set_svg_point(this, [webidl_float(value), y]);
    Ok(())
}

pub(crate) fn set_point_y<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgPoint>,
    value: f64,
) -> Result<(), ApiError> {
    let [x, _] = cx.svg_point(this);
    cx.set_svg_point(this, [x, webidl_float(value)]);
    Ok(())
}

/// The point through `matrix`, as a new one.
pub(crate) fn matrix_transform<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgPoint>,
    by: Gc<'gc, SvgMatrix>,
) -> Result<Gc<'gc, SvgPoint>, ApiError> {
    let point = matrix::transform_point(cx.svg_matrix(by), cx.svg_point(this));
    Ok(cx.create_svg_point(point))
}

macro_rules! rect_attributes {
    ($($get:ident, $set:ident, $index:literal;)*) => {
        $(
            pub(crate) fn $get<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, SvgRect>) -> Result<f64, ApiError> {
                Ok(cx.svg_rect(this)[$index])
            }

            pub(crate) fn $set<'gc>(
                cx: &ApiCx<'_, 'gc>,
                this: Gc<'gc, SvgRect>,
                value: f64,
            ) -> Result<(), ApiError> {
                let mut values = cx.svg_rect(this);
                values[$index] = webidl_float(value);
                cx.set_svg_rect(this, values);
                Ok(())
            }
        )*
    };
}

rect_attributes! {
    rect_x, set_rect_x, 0;
    rect_y, set_rect_y, 1;
    rect_width, set_rect_width, 2;
    rect_height, set_rect_height, 3;
}

fn new_rect<'gc>(cx: &ApiCx<'_, 'gc>, rect: RectF) -> Gc<'gc, SvgRect> {
    cx.create_svg_rect([rect.x, rect.y, rect.width, rect.height].map(f64::from))
}

fn new_point<'gc>(cx: &ApiCx<'_, 'gc>, point: PointF) -> Gc<'gc, SvgPoint> {
    cx.create_svg_point([f64::from(point.x), f64::from(point.y)])
}

fn new_matrix<'gc>(cx: &ApiCx<'_, 'gc>, Transform(values): Transform) -> Gc<'gc, SvgMatrix> {
    cx.create_svg_matrix(values.map(f64::from))
}

/// `createSVGPoint()`: (0, 0).
pub(crate) fn create_svg_point<'gc>(
    cx: &ApiCx<'_, 'gc>,
    _this: NodeRef<'gc>,
) -> Result<Gc<'gc, SvgPoint>, ApiError> {
    Ok(cx.create_svg_point([0.0, 0.0]))
}

/// `createSVGMatrix()`: the identity.
pub(crate) fn create_svg_matrix<'gc>(
    cx: &ApiCx<'_, 'gc>,
    _this: NodeRef<'gc>,
) -> Result<Gc<'gc, SvgMatrix>, ApiError> {
    Ok(cx.create_svg_matrix(matrix::IDENTITY))
}

/// `createSVGRect()`: all 0.
pub(crate) fn create_svg_rect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    _this: NodeRef<'gc>,
) -> Result<Gc<'gc, SvgRect>, ApiError> {
    Ok(cx.create_svg_rect([0.0; 4]))
}

/// The element's geometry, its document laid out first; the default (no
/// box, identity matrices) if no frame shows it.
pub(crate) fn geometry<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> SvgGeometry {
    cx.with_layout(element, |layout| layout.svg_geometry(cx, element))
        .unwrap_or_default()
}

/// `getBBox()`: its object bounding box, in its user space; all 0 if it
/// isn't rendered (Chrome has no exception for that).
pub(crate) fn get_bbox<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, SvgRect>, ApiError> {
    Ok(new_rect(cx, geometry(cx, this).bbox))
}

/// `getCTM()`: from its user space to its nearest viewport's; a new
/// matrix each call. Never null in Chrome, so not here either.
pub(crate) fn get_ctm<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, SvgMatrix>, ApiError> {
    Ok(new_matrix(cx, geometry(cx, this).ctm))
}

/// `getScreenCTM()`: from its user space to its frame's viewport.
pub(crate) fn get_screen_ctm<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, SvgMatrix>, ApiError> {
    Ok(new_matrix(cx, geometry(cx, this).screen_ctm))
}

/// The shape's path, its document laid out first; `None` for one with no
/// style (not in a document a frame shows), unless it is a `<path>`, whose
/// path data is its own (Blink's `SVGPathElement` reads its `d` detached
/// too).
fn shape_path<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<Path, ApiError> {
    cx.with_layout(this, |layout| layout.svg_shape_path(cx, this))
        .flatten()
        .or_else(|| {
            is_path(cx, this).then(|| {
                let data = cx
                    .attribute_value_by_id(this, names::local::D)
                    .map(|value| value.borrow().to_string())
                    .unwrap_or_default();
                css::svg::path_from_string(&data)
            })
        })
        .ok_or_else(|| exception(ExceptionName::InvalidState, "The element has no style."))
}

/// Whether it is a `<path>`, measured along its path data.
fn is_path<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> bool {
    cx.qualified_name(this)
        .is_some_and(|name| name.is(names::SVG_NAMESPACE, names::local::PATH))
}

/// `getTotalLength()`: its path's length (`gfx::svg::shape_length`).
pub(crate) fn get_total_length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<f64, ApiError> {
    let path = shape_path(cx, this)?;
    Ok(f64::from(gfx::svg::shape_length(&path, is_path(cx, this))))
}

/// `getPointAtLength()`: the point `distance` along its path, the distance
/// clamped to it; an empty path has none.
pub(crate) fn get_point_at_length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    distance: f64,
) -> Result<Gc<'gc, SvgPoint>, ApiError> {
    let path = shape_path(cx, this)?;
    let point = gfx::svg::point_at_length(&path, is_path(cx, this), distance)
        .ok_or_else(|| exception(ExceptionName::InvalidState, "The element's path is empty."))?;
    Ok(new_point(cx, point))
}

/// A `DOMPointInit`'s `x` and `y` (unrestricted doubles, 0 when missing)
/// as floats; `None` for one not finite.
fn finite_point(point: &DOMPointInit) -> Option<PointF> {
    let (x, y) = (
        f32_from_f64(point.x.unwrap_or(0.0)),
        f32_from_f64(point.y.unwrap_or(0.0)),
    );
    (x.is_finite() && y.is_finite()).then(|| PointF::new(x, y))
}

/// Whether `point`, in its user space, is in its fill area or stroke area,
/// as laid out; one with no layout node, or a point not finite, isn't.
fn point_in<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    point: &DOMPointInit,
    stroke: bool,
) -> bool {
    let Some(point) = finite_point(point) else {
        return false;
    };
    cx.with_layout(this, |layout| {
        layout.svg_point_in_shape(cx, this, point, stroke)
    })
    .unwrap_or(false)
}

pub(crate) fn is_point_in_fill<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    point: &DOMPointInit,
) -> Result<bool, ApiError> {
    Ok(point_in(cx, this, point, false))
}

pub(crate) fn is_point_in_stroke<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    point: &DOMPointInit,
) -> Result<bool, ApiError> {
    Ok(point_in(cx, this, point, true))
}

/// Its characters, as laid out; none if it isn't rendered.
fn chars<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Vec<SvgTextChar> {
    cx.with_layout(this, |layout| layout.svg_text_chars(cx, this))
        .unwrap_or_default()
}

fn out_of_range() -> ApiError {
    exception(
        ExceptionName::IndexSize,
        "The character number is out of range.",
    )
}

/// The character that `charnum` is a code unit of.
fn char_at<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    charnum: u32,
) -> Result<SvgTextChar, ApiError> {
    text::char_at(&chars(cx, this), charnum as usize).ok_or_else(out_of_range)
}

pub(crate) fn get_number_of_chars<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<i32, ApiError> {
    Ok(i32::try_from(text::count(&chars(cx, this))).unwrap_or(i32::MAX))
}

/// `getComputedTextLength()`: its characters' advances together.
pub(crate) fn get_computed_text_length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<f64, ApiError> {
    Ok(f64::from(text::computed_length(&chars(cx, this))))
}

/// `getSubStringLength()`: the advances of the characters with any of the
/// numbers from `charnum`, `nchars` of them (cut at the end).
pub(crate) fn get_sub_string_length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    charnum: u32,
    nchars: u32,
) -> Result<f64, ApiError> {
    text::sub_string_length(&chars(cx, this), charnum as usize, nchars as usize)
        .map(f64::from)
        .ok_or_else(out_of_range)
}

pub(crate) fn get_start_position_of_char<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    charnum: u32,
) -> Result<Gc<'gc, SvgPoint>, ApiError> {
    Ok(new_point(cx, char_at(cx, this, charnum)?.start))
}

pub(crate) fn get_end_position_of_char<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    charnum: u32,
) -> Result<Gc<'gc, SvgPoint>, ApiError> {
    Ok(new_point(cx, char_at(cx, this, charnum)?.end))
}

/// `getExtentOfChar()`: the bounds of its cell, turned with it.
pub(crate) fn get_extent_of_char<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    charnum: u32,
) -> Result<Gc<'gc, SvgRect>, ApiError> {
    Ok(new_rect(cx, text::extent(&char_at(cx, this, charnum)?)))
}

pub(crate) fn get_rotation_of_char<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    charnum: u32,
) -> Result<f64, ApiError> {
    Ok(f64::from(char_at(cx, this, charnum)?.rotation))
}

/// `getCharNumAtPosition()`: the number of the first character whose cell
/// holds `point`, in the `<text>`'s user space; -1 if none does.
pub(crate) fn get_char_num_at_position<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    point: &DOMPointInit,
) -> Result<i32, ApiError> {
    let at = PointF::new(
        f32_from_f64(point.x.unwrap_or(0.0)),
        f32_from_f64(point.y.unwrap_or(0.0)),
    );
    let found = text::char_num_at(&chars(cx, this), at);
    Ok(found.map_or(-1, |n| i32::try_from(n).unwrap_or(i32::MAX)))
}

/// `selectSubString()`: checks the range; Chrome selects the characters,
/// machopo doesn't.
pub(crate) fn select_sub_string<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    charnum: u32,
    _nchars: u32,
) -> Result<(), ApiError> {
    if charnum as usize >= text::count(&chars(cx, this)) {
        return Err(out_of_range());
    }
    Ok(())
}
