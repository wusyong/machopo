//! SVG images (Blink's `SVGImage`): an `image/svg+xml` response parsed
//! into a document of its own, in the page's DOM but with no window and
//! nothing that runs script, styled once, and laid out and painted for
//! each size it is drawn at (`SVGImageForContainer`: an `<img>`'s box, a
//! background's tile) into one display list, which raster replays as
//! vectors there (`SvgImagePaint`).
//! Paint asks for a size; one not painted yet is painted after, and what
//! asked painted again.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use compositor_paint::DisplayItemList;
use dom::{Cx, Dom, LocalToken, NodeKind, NodeRef, NodeRoot};
use gfx::SizeF;
use layout::{BoxFragment, ScrollOffsets, SvgNaturalSize, TextContext};

/// A container size and a device scale factor, by their bits.
type RecordKey = (u32, u32, u32);

/// What an SVG image is drawn from: its natural dimensions, and its paint
/// for each container size and scale it was prepared for.
pub(crate) struct SvgImagePaint {
    natural: SvgNaturalSize,
    records: RefCell<HashMap<RecordKey, Arc<DisplayItemList>>>,
}

impl SvgImagePaint {
    fn new(natural: SvgNaturalSize) -> Self {
        Self {
            natural,
            records: RefCell::default(),
        }
    }

    /// What it is laid out at to be drawn into a box `size` big: each
    /// natural dimension, one missing through the other and the ratio, else
    /// the box's; the box then scales it (Chrome 152 stretches a 20×10
    /// image with no `viewBox` to fill a 40×40 box, and lays one with only
    /// a `viewBox` out at the box's size).
    fn layout_size(&self, size: SizeF) -> SizeF {
        let n = self.natural;
        let width = n.width.or_else(|| Some(n.height? * n.ratio?));
        let height = n.height.or_else(|| Some(n.width? / n.ratio?));
        SizeF::new(width.unwrap_or(size.width), height.unwrap_or(size.height))
    }

    /// Paints its document laid out to be drawn into a box `size` big
    /// (`lay_out` lays it out at a size), CSS px, at
    /// `device_scale_factor`, unless it already was.
    fn prepare(
        &self,
        size: SizeF,
        device_scale_factor: f32,
        lay_out: impl FnOnce(SizeF) -> Option<Arc<BoxFragment>>,
    ) {
        let size = self.layout_size(size);
        let key = key(size, device_scale_factor);
        if self.records.borrow().contains_key(&key) {
            return;
        }
        let Some(fragment) = lay_out(size) else {
            return;
        };
        let pre = paint::pre_paint(&fragment, &ScrollOffsets::default(), device_scale_factor);
        let artifact = paint::paint(&fragment, &pre, size, &());
        let mut list = paint::to_display_list(&artifact, &pre.trees);
        list.finalize();
        self.records.borrow_mut().insert(key, Arc::new(list));
    }

    /// Its paint to be drawn into a box `size` big at
    /// `device_scale_factor`, if prepared, and the size it was laid out at.
    fn record(
        &self,
        size: SizeF,
        device_scale_factor: f32,
    ) -> Option<(Arc<DisplayItemList>, SizeF)> {
        let size = self.layout_size(size);
        let record = self
            .records
            .borrow()
            .get(&key(size, device_scale_factor))
            .cloned()?;
        Some((record, size))
    }
}

/// An SVG image whose document is in the DOM: in the page's DOM, with
/// no window, styled once (a `style` engine of its own, the UA index
/// the thread's).
pub(crate) struct SvgImage {
    document: NodeRoot,
    paint: SvgImagePaint,
}

impl SvgImage {
    /// `body`, an `image/svg+xml` response's, as an image of `dom`: its
    /// text parsed into a document of its own and styled, its style written
    /// while `token` is lent whole; `None` if its root isn't an SVG `<svg>`
    /// (a broken image, as in Chrome).
    pub(crate) fn new(
        dom: &mut Dom,
        token: &mut LocalToken,
        charset: Option<&str>,
        body: &[u8],
    ) -> Option<Self> {
        let text = crate::encoding::decode_xml(charset, body);
        let document = html::parser::parse_xml_document(dom, &text).root().clone();
        let natural = dom.mutate(|cx| {
            let root = svg_root(cx, &document)?;
            // Its `<use>`s' copies, which a frame makes before each style
            // update: this is its only one.
            cx.update_use_trees();
            style::StyleEngine::new().recalc_style(cx, token, document.get(cx.gc));
            Some(layout::svg_natural_size(layout::svg_element(
                cx, token, root,
            )))
        })?;
        Some(Self {
            document,
            paint: SvgImagePaint::new(natural),
        })
    }

    pub(crate) fn natural(&self) -> SvgNaturalSize {
        self.paint.natural
    }

    /// Lays it out and paints it to be drawn into a box `size` big, CSS
    /// px, at `device_scale_factor`, unless it already was.
    pub(crate) fn prepare(
        &self,
        cx: &Cx<'_, '_>,
        token: &LocalToken,
        text: &mut TextContext,
        size: SizeF,
        device_scale_factor: f32,
    ) {
        self.paint.prepare(size, device_scale_factor, |size| {
            let root = svg_root(cx, &self.document)?;
            Some(layout::svg_image_fragment(
                layout::svg_element(cx, token, root),
                text,
                size,
            ))
        });
    }

    pub(crate) fn record(
        &self,
        size: SizeF,
        device_scale_factor: f32,
    ) -> Option<(Arc<DisplayItemList>, SizeF)> {
        self.paint.record(size, device_scale_factor)
    }
}

/// The root of `document`, if it is an SVG `<svg>`.
fn svg_root<'gc>(cx: &Cx<'_, 'gc>, document: &NodeRoot) -> Option<NodeRef<'gc>> {
    let document = document.get(cx.gc);
    let mut child = cx.first_child(document);
    while let Some(node) = child {
        if cx.kind(node) == NodeKind::Element {
            return (cx.qualified_name(node)
                == Some(dom::QualifiedName::svg(dom::names::local::SVG)))
            .then_some(node);
        }
        child = cx.next_sibling(node);
    }
    None
}

fn key(size: SizeF, device_scale_factor: f32) -> RecordKey {
    (
        size.width.to_bits(),
        size.height.to_bits(),
        device_scale_factor.to_bits(),
    )
}
