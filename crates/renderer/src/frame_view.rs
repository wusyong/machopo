//! The DOM's `FrameView` (`blink::LocalFrameView`): a document of the
//! new DOM and the lifecycle output built from it, brought up to date a
//! frame at a time. Its style is `style`'s, its box tree `layout`'s
//! from the DOM (`BoxTree::build`); from the fragments down,
//! `paint` pre-paints and paints, and `compositing` makes the compositor's
//! layers.
//!
//! It is the frame's layout as script reaches it too (`Frame`, a
//! `dom_api::FrameLayout`): the engine holds it, with the thread's
//! token and text, and lends it to each operation; the renderer borrows it
//! back for a frame.
//!
//! It draws the focused text control's selection and caret (blinking,
//! while the page has the focus), or else the document's selection and
//! its caret in the focused editing host (`selection`), and the
//! images of its backgrounds, which it asks the DOM's list of available
//! images for as style finds them (an `<img>`'s is its element's, which
//! layout reads). Its child frames' views are its `children`, laid out at
//! their iframes' content boxes and painted with it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use compositor::{CompositorScrollDelta, DocumentId, Layer, ScrollKey, ScrollNode};
use dom::{Cx, Dom, LocalToken, NodeId, NodeRef, NodeRoot};
use gfx::{ElementBox, PointF, RectF, SizeF, SvgGeometry, SvgTextChar, Transform, Viewport};
use layout::svg_dom::SvgElement;
use layout::{BoxFragment, BoxTree, TextContext};
use paint::PrePaint;

use crate::compositing;
use crate::scroll::{self, ScrollState};
use crate::svg_image::SvgImage;

pub(crate) struct FrameView {
    document: NodeRoot,
    /// Its document's id.
    document_node: NodeId,
    document_id: DocumentId,
    /// A child frame's iframe (in its parent's document), and its browsing
    /// context; none for the top-level frame's view.
    owner: Option<(NodeId, dom_api::FrameId)>,
    /// A child frame whose iframe's `scrolling` keeps the user from
    /// scrolling it (as its iframe was when it was last laid out).
    scrolling_off: bool,
    /// A child frame whose document waits for a sheet that blocks its
    /// rendering: neither styled nor laid out meanwhile, so it doesn't
    /// show unstyled (and its transitions don't start from that style),
    /// shown as it last was. Its viewport follows its iframe all the same,
    /// so its media queries see its size (Blink's child frame has it from
    /// its owner's layout before it is styled). The top-level frame's
    /// rendering waits as a whole (`RenderThread::render_blocked`).
    render_blocked: bool,
    /// Given another viewport while blocked: laid out again once it isn't.
    needs_layout: bool,
    /// The child frames' views, as their iframes are in its document: laid
    /// out at their iframes' content boxes after it, pre-painted and
    /// painted with it.
    children: Vec<FrameView>,
    /// The child frames below it laid out at another size since they were
    /// last taken (`take_resized_frames`): their environment changed.
    resized: Vec<dom_api::FrameId>,
    style: style::StyleEngine,
    box_tree: Option<BoxTree>,
    viewport: SizeF,
    /// Device pixels per CSS px: what pre-paint and paint snap to.
    device_scale_factor: f32,
    /// The last layout, and the pre-paint of it.
    fragment: Option<Arc<BoxFragment>>,
    pre: Option<(Arc<BoxFragment>, PrePaint)>,
    document_size: SizeF,
    scroll: ScrollState,
    /// What its pre-paint is made from changed: its layout, or the device
    /// scale factor (its scroll offsets mark `ScrollState::needs_pre_paint`).
    needs_pre_paint: bool,
    /// What it paints changed since the last paint.
    paint_dirty: bool,
    /// The scroll containers that were layers at the last paint.
    composited: dom::NodeIdHashSet,
    /// How far the sticky boxes stuck when it was last painted.
    painted_sticky: paint::StickyOffsets,
    /// The DOM's count of its wheel listeners' changes the compositor's
    /// wheel event handler region was last made at.
    wheel_listeners_version: Option<u64>,
    compositor: compositing::PaintArtifactCompositor,
    paint_controller: paint::PaintController,
    output: Option<Arc<PaintOutput>>,
    /// The element to scroll into view once next laid out, as a fragment
    /// is.
    scroll_target: Option<NodeId>,
    /// Script, or a key, scrolled something since last asked
    /// (`take_scrolled_by_script`): the next frame paints or commits it (no
    /// DOM changed).
    scrolled_by_script: bool,
    /// The elements to scroll into view in their containers once laid out
    /// (a list box's option), by container.
    container_scrolls: Vec<(NodeId, NodeId)>,
    /// What is drawn as selected: the focused text control's selection,
    /// in its inner editor's text, or else the document's selection
    /// (`selection`).
    selected: HashMap<NodeId, paint::TextSelection>,
    /// The focused text control's caret while the page has the focus, and
    /// whether it is shown now (it blinks) and came or moved since asked.
    caret: Option<paint::Caret>,
    caret_shown: bool,
    caret_moved: bool,
    /// The images its backgrounds show that the DOM's list of available
    /// images has, by URL; those it is yet to have, with the elements whose
    /// backgrounds show them (looked for again once an image came:
    /// `images_changed`; those elements paint anew, as Blink's
    /// `ImageChanged`).
    background_images: HashMap<String, dom::Image>,
    awaited_backgrounds: HashMap<String, Vec<NodeId>>,
    images_came: bool,
    /// How many times it was pre-painted (`pre_paints`).
    pre_paints: u64,
    /// Each canvas's snapshot as it was last drawn (`canvases_drawn`), and
    /// whether it is opaque: what paint draws, and a canvas's layer shows
    /// (the snapshots are in the DOM, which paint doesn't reach).
    canvases: HashMap<NodeId, (gfx::ImageData, bool)>,
    /// The canvases that are layers and drew since the last paint: their
    /// layers take their new snapshots without a paint.
    canvas_textures: Vec<NodeId>,
}

/// A frame's background images (`images`) and those of the frames below
/// it (`children`): borrowed apart from the rest of the view, which paint
/// writes.
fn background_tables<'a>(
    images: &'a HashMap<String, dom::Image>,
    children: &'a [FrameView],
) -> Vec<&'a HashMap<String, dom::Image>> {
    let mut tables = vec![images];
    for child in children {
        tables.extend(child.background_tables());
    }
    tables
}

/// The selected text of a frame (`selected`) and of the frames in it.
fn selection_tables<'a>(
    selected: &'a HashMap<NodeId, paint::TextSelection>,
    children: &'a [FrameView],
) -> Vec<&'a HashMap<NodeId, paint::TextSelection>> {
    let mut tables = vec![selected];
    for child in children {
        tables.extend(selection_tables(&child.selected, &child.children));
    }
    tables
}

/// What paints again for a frame's selected text going from `old` to
/// `new`: the elements of the texts whose selection changed (a `<br>`
/// itself: the line it ends knows it).
fn selection_changes(
    cx: &Cx<'_, '_>,
    old: &HashMap<NodeId, paint::TextSelection>,
    new: &HashMap<NodeId, paint::TextSelection>,
) -> Vec<NodeId> {
    new.iter()
        .filter(|(node, selection)| old.get(node) != Some(selection))
        .map(|(node, _)| *node)
        .chain(old.keys().filter(|node| !new.contains_key(node)).copied())
        .filter_map(|node| {
            let node = cx.node_by_id(node)?;
            if cx.kind(node) == dom::NodeKind::Element {
                return Some(cx.node_id(node));
            }
            cx.parent(node).map(|parent| cx.node_id(parent))
        })
        .collect()
}

/// The SVG images the page's DOM made of what it fetched
/// (`svg_image::SvgImage`), by the numbers the DOM's images and layout
/// know them by: the frame's, kept as it shows another document, as the
/// DOM's list of available images is.
#[derive(Default)]
pub(crate) struct SvgImages {
    images: HashMap<u64, SvgImage>,
    next: u64,
}

impl SvgImages {
    /// `image` kept: its number.
    pub(crate) fn add(&mut self, image: SvgImage) -> u64 {
        let id = self.next;
        // One per SVG image the DOM fetches: fewer than u64::MAX.
        self.next = self.next.saturating_add(1);
        self.images.insert(id, image);
        id
    }
}

/// What paint reads of the document besides the fragments: the images of
/// its backgrounds, the SVG images (those it asks for at a size not painted
/// yet noted, `missing`), its canvases' snapshots, the selection and the
/// caret.
struct FrameImages<'a> {
    /// Each frame's background images, the tree's (one paint for all).
    images: Vec<&'a HashMap<String, dom::Image>>,
    svg: &'a SvgImages,
    device_scale_factor: f32,
    missing: RefCell<Vec<(u64, SizeF, Option<NodeId>)>>,
    canvases: &'a HashMap<NodeId, (gfx::ImageData, bool)>,
    /// Each frame's selected text, the tree's.
    selected: Vec<&'a HashMap<NodeId, paint::TextSelection>>,
    caret: Option<paint::Caret>,
    system_colors: gfx::SystemColors,
}

impl paint::BackgroundImages for FrameImages<'_> {
    fn image(&self, url: &str) -> Option<(gfx::ImageData, gfx::Size)> {
        match self.images.iter().find_map(|images| images.get(url))? {
            dom::Image::Raster(data, size) => Some((data.clone(), *size)),
            dom::Image::Svg(..) => None,
        }
    }

    fn svg_url(&self, url: &str) -> Option<(u64, layout::SvgNaturalSize)> {
        match self.images.iter().find_map(|images| images.get(url))? {
            dom::Image::Svg(image, natural) => Some((*image, *natural)),
            dom::Image::Raster(..) => None,
        }
    }

    /// As painted for that size; one not yet is noted, for the frame to
    /// paint and then paint again.
    fn svg_image(
        &self,
        image: u64,
        size: SizeF,
        client: Option<NodeId>,
    ) -> Option<(Arc<compositor_paint::DisplayItemList>, SizeF)> {
        let record = self
            .svg
            .images
            .get(&image)?
            .record(size, self.device_scale_factor);
        if record.is_none() {
            self.missing.borrow_mut().push((image, size, client));
        }
        record
    }

    fn canvas(&self, element: NodeId) -> Option<gfx::ImageData> {
        self.canvases
            .get(&element)
            .map(|(snapshot, _)| snapshot.clone())
    }

    fn selection(&self, text: NodeId) -> Option<paint::TextSelection> {
        self.selected
            .iter()
            .find_map(|selected| selected.get(&text))
            .cloned()
    }

    fn caret(&self) -> Option<paint::Caret> {
        self.caret
    }

    fn selection_colors(&self, active: bool) -> paint::SelectionColors {
        if active {
            paint::SelectionColors::active(&self.system_colors)
        } else {
            paint::SelectionColors::inactive(&self.system_colors)
        }
    }
}

impl FrameView {
    /// A view of `document`, which the compositor knows as `document_id`,
    /// with the platform's `system_colors` (`RendererFlags::system_colors`):
    /// everything to do.
    pub(crate) fn new(
        document: NodeRoot,
        document_node: NodeId,
        document_id: DocumentId,
        system_colors: gfx::SystemColors,
    ) -> Self {
        let mut style = style::StyleEngine::new();
        style.set_system_colors(system_colors);
        Self {
            document,
            document_node,
            document_id,
            owner: None,
            scrolling_off: false,
            render_blocked: false,
            needs_layout: false,
            children: Vec::new(),
            resized: Vec::new(),
            style,
            box_tree: None,
            viewport: SizeF::default(),
            device_scale_factor: 1.0,
            fragment: None,
            pre: None,
            document_size: SizeF::default(),
            scroll: ScrollState::default(),
            needs_pre_paint: true,
            paint_dirty: true,
            composited: dom::NodeIdHashSet::default(),
            painted_sticky: paint::StickyOffsets::default(),
            wheel_listeners_version: None,
            scroll_target: None,
            scrolled_by_script: false,
            container_scrolls: Vec::new(),
            selected: HashMap::new(),
            background_images: HashMap::new(),
            awaited_backgrounds: HashMap::new(),
            images_came: false,
            pre_paints: 0,
            canvases: HashMap::new(),
            canvas_textures: Vec::new(),
            caret: None,
            caret_shown: true,
            caret_moved: false,
            compositor: Default::default(),
            paint_controller: Default::default(),
            output: None,
        }
    }

    pub(crate) fn document_id(&self) -> DocumentId {
        self.document_id
    }

    /// The child frames below it laid out at another size since the last
    /// time (their first layout too), taken: HTML's environment of their
    /// documents changed.
    pub(crate) fn take_resized_frames(&mut self) -> Vec<dom_api::FrameId> {
        std::mem::take(&mut self.resized)
    }

    /// Whether a sheet blocks this child frame's rendering (its document
    /// waits for one): it isn't styled or laid out till none does.
    pub(crate) fn set_render_blocked(&mut self, blocked: bool) {
        self.render_blocked = blocked;
    }

    /// This view as `frame`'s, the child frame of iframe `owner`.
    pub(crate) fn into_child(mut self, frame: dom_api::FrameId, owner: NodeId) -> Self {
        self.owner = Some((owner, frame));
        self
    }

    /// The view, this one or one below, of child frame `frame`.
    pub(crate) fn child_mut(&mut self, frame: dom_api::FrameId) -> Option<&mut Self> {
        self.children.iter_mut().find_map(|child| {
            if child.owner.is_some_and(|(_, id)| id == frame) {
                Some(child)
            } else {
                child.child_mut(frame)
            }
        })
    }

    /// The child frame, this view's or one's below, whose iframe is
    /// `owner`.
    pub(crate) fn frame_owned_by(&self, owner: NodeId) -> Option<dom_api::FrameId> {
        self.children.iter().find_map(|child| match child.owner {
            Some((of, frame)) if of == owner => Some(frame),
            _ => child.frame_owned_by(owner),
        })
    }

    /// The child frame this view is of; none for the top-level one's.
    pub(crate) fn child_frame(&self) -> Option<dom_api::FrameId> {
        self.owner.map(|(_, frame)| frame)
    }

    /// The child frames this view and those below show.
    pub(crate) fn child_frames_shown(&self, frames: &mut Vec<dom_api::FrameId>) {
        for child in &self.children {
            frames.extend(child.child_frame());
            child.child_frames_shown(frames);
        }
    }

    /// The view, this one or one below, showing the document whose id is
    /// `document`.
    pub(crate) fn view_of(&self, document: NodeId) -> Option<&Self> {
        if self.document_node == document {
            return Some(self);
        }
        self.children
            .iter()
            .find_map(|child| child.view_of(document))
    }

    pub(crate) fn view_of_mut(&mut self, document: NodeId) -> Option<&mut Self> {
        if self.document_node == document {
            return Some(self);
        }
        self.children
            .iter_mut()
            .find_map(|child| child.view_of_mut(document))
    }

    /// `view` (a child frame's: `into_child`) shown in the frame, this one
    /// or one below, that shows the document whose id is `parent`, if it
    /// has none for that frame yet.
    pub(crate) fn show_new_child(&mut self, parent: NodeId, view: Self) {
        let Some((_, frame)) = view.owner else {
            return;
        };
        if self.child_mut(frame).is_some() {
            return;
        }
        if let Some(at) = self.view_of_mut(parent) {
            at.children.push(view);
            at.paint_dirty = true;
        }
    }

    /// The compositor's id of the document this view shows.
    pub(crate) fn set_document_id(&mut self, document_id: DocumentId) {
        self.document_id = document_id;
    }

    /// `view` (a child frame's: `into_child`) shown in this view's frame
    /// or the one below that is `parent` (none: this one), replacing the one
    /// it had for the same frame.
    pub(crate) fn show_child(&mut self, parent: Option<dom_api::FrameId>, view: Self) {
        let Some((_, frame)) = view.owner else {
            return;
        };
        let at = match parent {
            Some(parent) => match self.child_mut(parent) {
                Some(at) => at,
                None => return,
            },
            None => self,
        };
        match at
            .children
            .iter_mut()
            .find(|child| child.owner.is_some_and(|(_, id)| id == frame))
        {
            Some(old) => *old = view,
            None => at.children.push(view),
        }
        at.paint_dirty = true;
    }

    /// Child frame `frame`'s view gone, with those below it.
    pub(crate) fn remove_child(&mut self, frame: dom_api::FrameId) -> bool {
        let before = self.children.len();
        self.children
            .retain(|child| child.owner.is_none_or(|(_, id)| id != frame));
        if self.children.len() != before {
            self.paint_dirty = true;
            self.needs_pre_paint = true;
            return true;
        }
        self.children
            .iter_mut()
            .any(|child| child.remove_child(frame))
    }

    /// The child frames below this one as last laid out, by iframe, for
    /// pre-paint and paint to go into. One never laid out isn't painted.
    fn child_frames(&self) -> paint::ChildFrames {
        let mut frames = paint::ChildFrames::default();
        self.collect_child_frames(&mut frames);
        frames
    }

    fn collect_child_frames(&self, frames: &mut paint::ChildFrames) {
        for child in &self.children {
            if let (Some((owner, _)), Some(fragment)) = (child.owner, &child.fragment) {
                frames.insert(
                    owner,
                    paint::ChildFrame {
                        layout_view: fragment.clone(),
                        document: child.document_node,
                        viewport_scroll: child.scroll.viewport,
                        user_scrollable_x: !child.scrolling_off
                            && child
                                .scroll
                                .viewport_node
                                .is_some_and(|node| node.user_scrollable_horizontal),
                        user_scrollable_y: !child.scrolling_off
                            && child
                                .scroll
                                .viewport_node
                                .is_some_and(|node| node.user_scrollable_vertical),
                        scroll: child.scroll.containers.clone(),
                        document_size: child.document_size,
                    },
                );
            }
            child.collect_child_frames(frames);
        }
    }

    /// An image the DOM's list of available images fetched came: the
    /// backgrounds that wait for one look again (an `<img>`'s boxes are
    /// built again as its element's request says).
    pub(crate) fn images_changed(&mut self) {
        self.images_came = true;
        for child in &mut self.children {
            child.images_changed();
        }
    }

    /// The background images of this frame and the ones below it.
    fn background_tables(&self) -> Vec<&HashMap<String, dom::Image>> {
        background_tables(&self.background_images, &self.children)
    }

    pub(crate) fn document(&self) -> &NodeRoot {
        &self.document
    }

    /// Style, layout, pre-paint and paint of the document, as each is due
    /// (Blink's `LocalFrameView::UpdateAllLifecyclePhases`): what the
    /// compositor gets. Style is written while `token` is lent whole.
    pub(crate) fn update_all_lifecycle_phases(
        &mut self,
        dom: &mut Dom,
        token: &mut LocalToken,
        text: &mut TextContext,
        svg: &SvgImages,
        viewport: SizeF,
        device_scale_factor: f32,
    ) -> Arc<PaintOutput> {
        let _span = trace_event::trace_event!("LocalFrameView::UpdateLifecyclePhases");
        let wheel = dom.mutate(|cx| {
            self.update_style_and_layout(cx, token, text, viewport, device_scale_factor);
            self.update_selection_and_caret(cx, token, text);
            WheelListeners {
                any: cx.may_have_wheel_listeners(),
                whole_document: cx.document_blocks_wheel(self.document.get(cx.gc)),
                version: cx.wheel_listeners_version(),
            }
        });
        self.update_pre_paint();
        self.paint(&wheel, dom, token, text, svg);
        self.output.clone().expect("just painted")
    }

    /// Pre-paint of the last layout, if it is out of date.
    pub(crate) fn update_pre_paint(&mut self) {
        if self.pre.is_none() || self.needs_pre_paint || self.scroll.needs_pre_paint {
            let fragment = self.fragment.clone().expect("just laid out");
            let previous = self.pre.take().map(|(_, pre)| pre);
            let children = self.child_frames();
            let pre = paint::pre_paint_frame_tree(
                &fragment,
                &self.scroll.containers,
                &self.stuck(),
                &children,
                self.device_scale_factor,
                self.scroll.viewport,
                previous,
            );
            self.pre = Some((fragment, pre));
            self.needs_pre_paint = false;
            self.scroll.needs_pre_paint = false;
            self.pre_paints = self.pre_paints.wrapping_add(1);
        }
    }

    /// How many times it was pre-painted: the intersections it gives
    /// (`intersections`) hold while this and the observations stay the
    /// same.
    pub(crate) fn pre_paints(&self) -> u64 {
        self.pre_paints
    }

    /// Whether something scrolled in this frame or those under it that the
    /// scroll steps haven't seen (`take_scroll_events`).
    pub(crate) fn has_scroll_events(&self) -> bool {
        self.scroll.has_scrolled() || self.children.iter().any(FrameView::has_scroll_events)
    }

    /// The `scroll` events due (HTML's pending scroll event targets): what
    /// moved in each frame of the tree under this one since last asked,
    /// this frame's first, then its child frames' in order; each the
    /// document (for its viewport) or the container's element.
    pub(crate) fn take_scroll_events(&mut self, out: &mut Vec<NodeId>) {
        for target in self.scroll.take_scrolled() {
            out.push(target.unwrap_or(self.document_node));
        }
        for child in &mut self.children {
            child.take_scroll_events(out);
        }
    }

    /// Whether script scrolled anything since last asked.
    pub(crate) fn take_scrolled_by_script(&mut self) -> bool {
        std::mem::take(&mut self.scrolled_by_script)
    }

    /// Script scrolls `target` (a scroll container of the frame showing
    /// `document`, or `document` for its viewport) to `offset` (CSS px),
    /// clamped to what it can scroll as last laid out
    /// (`ScrollState::scroll_to`); what moved is painted again, or (a
    /// layer) moved by the compositor at the commit.
    pub(crate) fn scroll_to(&mut self, document: NodeId, target: NodeId, offset: PointF) {
        let before = self.scroll_offsets();
        let Some(frame) = self.view_of_mut(document) else {
            return;
        };
        let Some(fragment) = frame.fragment.clone() else {
            return;
        };
        let target = (target != document).then_some(target);
        if frame.scroll.scroll_to(&fragment, target, offset) {
            self.containers_moved(&before);
            self.scrolled_by_script = true;
        }
    }

    /// Script scrolls `element` into view (`ScrollState::scroll_into_view`)
    /// in the frame showing it, then, unless only the nearest scrolling box
    /// is asked for, in each frame around that one, where its iframe shows
    /// the element now (Blink's `ScrollRectToVisible` going on to the owner
    /// frame), up to this frame, the root of the tree. Its documents' as
    /// `document_of` finds them.
    pub(crate) fn scroll_into_view(
        &mut self,
        element: NodeId,
        document_of: &dyn Fn(NodeId) -> Option<NodeId>,
        params: gfx::ScrollIntoViewParams,
    ) {
        self.scroll_rect_into_view(element, None, document_of, params);
    }

    /// `scroll_into_view` of `element`, or of `inner`, a rect in its
    /// content box (a caret's).
    pub(crate) fn scroll_rect_into_view(
        &mut self,
        element: NodeId,
        inner: Option<RectF>,
        document_of: &dyn Fn(NodeId) -> Option<NodeId>,
        params: gfx::ScrollIntoViewParams,
    ) {
        let before = self.scroll_offsets();
        let mut moved = false;
        let (mut target, mut inner) = (element, inner);
        while let Some(frame) = document_of(target).and_then(|document| self.view_of_mut(document))
        {
            let Some(fragment) = frame.fragment.clone() else {
                break;
            };
            let viewport = frame.scroll.viewport;
            let Some(rect) = frame
                .scroll
                .scroll_into_view(&fragment, target, inner, params)
            else {
                break;
            };
            moved |= frame.scroll.viewport != viewport;
            match frame.owner {
                Some((iframe, _)) if !params.nearest_only => (target, inner) = (iframe, Some(rect)),
                _ => break,
            }
        }
        if self.containers_moved(&before) || moved {
            self.scrolled_by_script = true;
        }
    }

    /// Where `element`'s border box starts in its transform space, as last
    /// pre-painted (`paint::paint_offset`).
    pub(crate) fn paint_offset(&self, element: NodeId) -> Option<PointF> {
        let (fragment, pre) = self.pre.as_ref()?;
        paint::paint_offset(fragment, pre, element)
    }

    /// The last paint's number, if it painted.
    pub(crate) fn last_paint(&self) -> Option<paint::PaintId> {
        self.paint_controller
            .last_artifact()
            .map(|artifact| artifact.id)
    }

    /// Each request's intersection (`paint::intersections`), as the frame's
    /// last pre-paint placed its document: none before the first, and no
    /// target visible before the first paint. `contains`: whether the
    /// second node is the first or in it, in the flat tree.
    pub(crate) fn intersections(
        &self,
        document: NodeId,
        requests: &[paint::IntersectionRequest],
        contains: &dyn Fn(NodeId, NodeId) -> bool,
    ) -> Vec<paint::IntersectionGeometry> {
        let Some((fragment, pre)) = self.pre.as_ref() else {
            return vec![paint::IntersectionGeometry::default(); requests.len()];
        };
        let occlusion = self
            .paint_controller
            .last_artifact()
            .map(|artifact| paint::Occlusion { artifact, contains });
        paint::intersections(
            fragment,
            pre,
            document,
            self.viewport,
            requests,
            occlusion.as_ref(),
        )
    }

    /// Scrolls `element` into view as a fragment is, once the document is
    /// next laid out (an automated check's `ScrollIntoViewIfNeeded`); the
    /// document itself: its viewport to the top.
    pub(crate) fn scroll_into_view_after_layout(&mut self, element: NodeId) {
        self.scroll_target = Some(element);
    }

    /// Scrolls `element` into view in its scroll container `container`
    /// only, once the document is next laid out (the last asked of each
    /// container).
    pub(crate) fn scroll_into_view_in_after_layout(&mut self, container: NodeId, element: NodeId) {
        self.container_scrolls.retain(|(c, _)| *c != container);
        self.container_scrolls.push((container, element));
    }

    /// Style and layout of the document, as each is due, in the operation
    /// `cx` (a frame's, or a member's that reads layout); then the element
    /// to scroll into view, if one waits.
    pub(crate) fn update_style_and_layout(
        &mut self,
        cx: &Cx<'_, '_>,
        token: &mut LocalToken,
        text: &mut TextContext,
        viewport: SizeF,
        device_scale_factor: f32,
    ) {
        self.update_style_and_layout_only(cx, token, text, viewport, device_scale_factor);
        // Each child frame's, at its iframe's content box (0×0 where the
        // iframe has no box: Blink still styles and lays out such a frame
        // of the same process); laid out again, the frames are pre-painted
        // again.
        let mut children = std::mem::take(&mut self.children);
        for child in &mut children {
            let child_viewport = child_viewport(self.fragment.as_deref(), child.owner);
            child.scrolling_off = child.owner.is_some_and(|(owner, _)| {
                cx.node_by_id(owner).is_some_and(|iframe| {
                    cx.attribute_value_ns(iframe, None, "scrolling")
                        .is_some_and(|value| scrolling_off(&value.borrow().to_str()))
                })
            });
            let before = child.fragment.clone();
            if child.viewport != child_viewport
                && let Some((_, frame)) = child.owner
            {
                self.resized.push(frame);
            }
            if child.render_blocked {
                child.set_viewport_unstyled(child_viewport, device_scale_factor);
                continue;
            }
            child.update_style_and_layout(cx, token, text, child_viewport, device_scale_factor);
            self.resized.append(&mut child.resized);
            let laid_out = match (&before, &child.fragment) {
                (Some(before), Some(after)) => !Arc::ptr_eq(before, after),
                (None, None) => false,
                _ => true,
            };
            if laid_out || child.paint_dirty {
                self.needs_pre_paint = true;
                self.paint_dirty = true;
            }
            // Scrolled alone: pre-painted again; painted again only if what
            // moved isn't a layer (`containers_moved`,
            // `apply_compositor_scroll`) or a sticky box in it stuck
            // elsewhere (`paint`), as the top-level frame's scrolls are.
            if child.needs_pre_paint || child.scroll.needs_pre_paint {
                self.needs_pre_paint = true;
            }
            // It is pre-painted and painted with this frame (`child_frames`),
            // never on its own: what it needed, this frame does now.
            child.paint_dirty = false;
            child.needs_pre_paint = false;
            child.scroll.needs_pre_paint = false;
        }
        self.children = children;
        if let Some(fragment) = &self.fragment {
            for (_, element) in std::mem::take(&mut self.container_scrolls) {
                if self
                    .scroll
                    .scroll_into_view(fragment, element, None, scroll::IN_CONTAINER)
                    .is_some()
                {
                    self.paint_dirty = true;
                }
            }
        }
        match self.scroll_target.take() {
            // HTML's "scroll to the beginning of the document": the
            // viewport, as its scroll offsets do (the compositor takes it
            // at the commit).
            Some(target) if target == self.document_node => {
                self.scroll.set_viewport(PointF::new(0.0, 0.0));
            }
            Some(target)
                if let Some(fragment) = &self.fragment
                    && self
                        .scroll
                        .scroll_into_view(fragment, target, None, scroll::FRAGMENT_ANCHOR)
                        .is_some() =>
            {
                self.paint_dirty = true;
            }
            _ => {}
        }
    }

    /// The frames around the one showing `document` laid out, and its
    /// size from its iframe's box given it, its own document not styled or
    /// laid out (Blink's `LocalDOMWindow::GetViewportSize` lays out the
    /// parent only): whether this view or one below shows it. Nothing of
    /// this view's if it is the one.
    pub(crate) fn update_frames_around(
        &mut self,
        cx: &Cx<'_, '_>,
        token: &mut LocalToken,
        text: &mut TextContext,
        viewport: SizeF,
        device_scale_factor: f32,
        document: NodeId,
    ) -> bool {
        if self.document_node == document {
            return true;
        }
        let Some(index) = self
            .children
            .iter()
            .position(|child| child.view_of(document).is_some())
        else {
            return false;
        };
        self.update_style_and_layout_only(cx, token, text, viewport, device_scale_factor);
        let child = &self.children[index];
        let child_viewport = child_viewport(self.fragment.as_deref(), child.owner);
        if child.viewport != child_viewport
            && let Some((_, frame)) = child.owner
        {
            self.resized.push(frame);
        }
        let child = &mut self.children[index];
        if child.document_node == document {
            child.set_viewport_unstyled(child_viewport, device_scale_factor);
        } else {
            child.update_frames_around(
                cx,
                token,
                text,
                child_viewport,
                device_scale_factor,
                document,
            );
            self.resized.append(&mut child.resized);
        }
        true
    }

    /// Style of the document brought up to date, nothing laid out (Blink's
    /// `Document::UpdateStyleAndLayoutTree`; `getComputedStyle` of what
    /// layout doesn't decide): at the viewport it was last given.
    pub(crate) fn update_style(&mut self, cx: &Cx<'_, '_>, token: &mut LocalToken) {
        // The controls connected since get their UA shadow trees, which
        // style sees (Blink's `ProcessScheduledShadowTreeCreationsNow`).
        cx.create_scheduled_shadow_trees();
        // The `<use>`s whose copies may be out of date copy again (Blink's
        // `SVGUseElement::BuildPendingResource`, before style).
        cx.update_use_trees();
        let Self {
            document, style, ..
        } = self;
        // The document's URL, which its `<style>`s' `url()`s resolve
        // against: known by its first style update.
        if !style.has_url_context()
            && let Some(url) = cx.document_url(document.get(cx.gc))
            && let Ok(base) = url::Url::parse(&url.to_str())
        {
            let referrer = base.to_string();
            style.set_url_context(Some(css::UrlContext::new(base, &referrer)));
        }
        trace_event::in_scope(trace_event::trace_event!("Document::UpdateStyle"), || {
            style.recalc_style(cx, token, document.get(cx.gc));
        });
        // The images the backgrounds styled since show: those the list of
        // available images has drawn, the others fetched (Blink's style
        // loads its pending images), and looked for again once some came.
        let document_id = cx.node_id(document.get(cx.gc));
        for (element, image) in style.take_background_image_urls() {
            let url = String::from(&*image.url);
            if self.background_images.contains_key(&url) {
                continue;
            }
            // Its stylesheet asked, for DevTools.
            match cx.background_image(&url, document_id, image.referrer.as_ref()) {
                Some(image) => {
                    self.background_images.insert(url, image);
                }
                None => self
                    .awaited_backgrounds
                    .entry(url)
                    .or_default()
                    .push(element),
            }
        }
        if std::mem::take(&mut self.images_came) {
            let Self {
                awaited_backgrounds,
                background_images,
                paint_controller,
                ..
            } = self;
            let mut showing = Vec::new();
            awaited_backgrounds.retain(|url, elements| match cx.cached_image(url) {
                Some(image) => {
                    background_images.insert(url.clone(), image);
                    showing.append(elements);
                    false
                }
                None => true,
            });
            if !showing.is_empty() {
                paint_controller.invalidate(showing);
                self.paint_dirty = true;
            }
        }
    }

    /// The viewport a frame not styled yet is given (one whose rendering is
    /// blocked): its style's environment, which its first style takes, and
    /// what its media queries see meanwhile.
    fn set_viewport_unstyled(&mut self, viewport: SizeF, device_scale_factor: f32) {
        if viewport != self.viewport || device_scale_factor != self.device_scale_factor {
            if device_scale_factor != self.device_scale_factor {
                self.paint_dirty = true;
                self.needs_pre_paint = true;
            }
            self.needs_layout = true;
            self.viewport = viewport;
            self.device_scale_factor = device_scale_factor;
            self.style.set_viewport(viewport, device_scale_factor);
        }
    }

    fn update_style_and_layout_only(
        &mut self,
        cx: &Cx<'_, '_>,
        token: &mut LocalToken,
        text: &mut TextContext,
        viewport: SizeF,
        device_scale_factor: f32,
    ) {
        let _span = trace_event::trace_event!("LocalFrameView::RunStyleAndLayoutLifecyclePhases");
        let mut layout_dirty = self.fragment.is_none() || std::mem::take(&mut self.needs_layout);
        if viewport != self.viewport || device_scale_factor != self.device_scale_factor {
            if device_scale_factor != self.device_scale_factor {
                self.paint_dirty = true;
                self.needs_pre_paint = true;
            }
            layout_dirty |= viewport != self.viewport;
            self.viewport = viewport;
            self.device_scale_factor = device_scale_factor;
            self.style.set_viewport(viewport, device_scale_factor);
        }
        self.update_style(cx, token);
        let Self {
            document, box_tree, ..
        } = self;
        let token = &*token;
        let updated = {
            let document = document.get(cx.gc);
            match box_tree {
                Some(tree) => {
                    // Anything flagged for the boxes: a change that only took
                    // content out (built nothing, shaped nothing) counts too.
                    let flagged = cx.layout_tree_flags(document) != dom::LayoutTreeFlags::CLEAN;
                    let update = tree.update(cx, token, document, text);
                    flagged
                        || update.full
                        || update.built > 0
                        || update.shaped > 0
                        || update.restyled > 0
                }
                None => {
                    *box_tree = Some(BoxTree::build(cx, token, document, text));
                    true
                }
            }
        };
        layout_dirty |= updated;
        if !layout_dirty {
            return;
        }
        let box_tree = self.box_tree.as_mut().expect("built with the style");
        let anchoring = box_tree.take_scroll_anchoring_changes();
        if let Some(last) = &self.fragment {
            self.scroll.pick_scroll_anchors(
                cx,
                token,
                self.document.get(cx.gc),
                last,
                viewport,
                &anchoring,
            );
        }
        let fragment = box_tree.layout(viewport);
        self.document_size = layout::document_size(&fragment);
        // What can be scrolled may have changed.
        self.scroll.viewport_node = Some(scroll::viewport_scroll_node(
            viewport,
            self.document_size,
            box_tree.viewport_overflow(),
        ));
        self.scroll.clamp(&fragment);
        self.scroll
            .set_sticky(layout::StickyPositions::new(&fragment, viewport));
        // What the user was reading stays where it was: the compositor
        // takes the offsets at the next commit, as a scroll by script's.
        if !self.scroll.anchors.is_empty()
            && self.scroll.adjust_for_scroll_anchors(
                cx,
                &fragment,
                viewport,
                &anchoring.disabled,
                device_scale_factor,
            )
        {
            self.scroll.clamp(&fragment);
            self.scrolled_by_script = true;
        }
        if self
            .fragment
            .as_ref()
            .is_none_or(|last| !Arc::ptr_eq(last, &fragment))
        {
            self.paint_dirty = true;
            self.needs_pre_paint = true;
        }
        self.fragment = Some(fragment);
    }

    /// Paints the pre-painted fragments if anything they show changed, and
    /// makes the compositor's layers of them, with its wheel event handler
    /// region.
    fn paint(
        &mut self,
        wheel: &WheelListeners,
        dom: &Dom,
        token: &LocalToken,
        text: &mut TextContext,
        svg: &SvgImages,
    ) {
        let (fragment, pre) = self.pre.as_ref().expect("just pre-painted");
        let layers = paint::composited_scrollers(&pre.trees);
        let moved = composited_sticky_keys(&pre.trees, &layers);
        let mut sticky = self.stuck().offsets;
        sticky.retain(|key, _| !moved.contains(key));
        if !(self.paint_dirty || self.output.is_none() || sticky != self.painted_sticky) {
            trace_event::in_scope(
                trace_event::trace_event!("FrameView::take_canvas_textures"),
                || {
                    swap_canvas_textures(
                        std::mem::take(&mut self.canvas_textures),
                        &self.compositor,
                        &self.canvases,
                        &mut self.output,
                    );
                },
            );
            if self.wheel_listeners_version != Some(wheel.version) {
                // Only the listeners changed: the same layers, with the
                // region made again.
                let last = self.output.clone().expect("painted");
                let mut property_trees = last.property_trees.clone();
                let artifact = self
                    .paint_controller
                    .last_artifact()
                    .expect("painted")
                    .clone();
                trace_event::in_scope(
                    trace_event::trace_event!("FrameView::set_wheel_event_handler_region"),
                    || {
                        set_wheel_event_handler_region(
                            pre,
                            &layers,
                            &artifact,
                            &mut property_trees,
                            wheel,
                        );
                    },
                );
                self.wheel_listeners_version = Some(wheel.version);
                self.output = Some(Arc::new(PaintOutput {
                    layers: last.layers.clone(),
                    property_trees,
                }));
            }
            return;
        }
        // The paint alone: what goes to the compositor after it is the
        // lifecycle's own step, as Chrome's `PushPaintArtifactToCompositor`.
        let paint_tree = trace_event::trace_event!("LocalFrameView::PaintTree");
        let composited: dom::NodeIdHashSet = pre
            .trees
            .scroll_nodes()
            .filter(|(id, _)| layers[id.0])
            .map(|(_, node)| node.element)
            .collect();
        // The containers that are layers are painted unscrolled: the
        // compositor moves them by their offsets; so are the sticky boxes
        // it moves.
        let unscrolled =
            (!composited.is_empty() || !moved.is_empty()).then(|| pre.unscrolled(&composited));
        let painted = unscrolled.as_ref().unwrap_or(pre);
        let images = FrameImages {
            images: background_tables(&self.background_images, &self.children),
            svg,
            device_scale_factor: self.device_scale_factor,
            missing: RefCell::default(),
            canvases: &self.canvases,
            selected: selection_tables(&self.selected, &self.children),
            caret: self.caret.filter(|_| self.caret_shown),
            system_colors: self.style.system_colors(),
        };
        let mut artifact =
            self.paint_controller
                .paint(fragment, painted, self.document_size, &images);
        // The SVG images paint asked for at a size not painted yet (just
        // loaded, or in a box or tile of a new size): painted now, and the
        // boxes showing them painted again.
        let missing = images.missing.take();
        if !missing.is_empty() {
            let mut clients = Vec::new();
            dom.read(|cx| {
                for (image, size, client) in missing {
                    if let Some(image) = svg.images.get(&image) {
                        image.prepare(cx, token, text, size, self.device_scale_factor);
                    }
                    clients.extend(client);
                }
            });
            self.paint_controller.invalidate(clients);
            artifact = self
                .paint_controller
                .paint(fragment, painted, self.document_size, &images);
        }
        drop(paint_tree);
        let region = trace_event::in_scope(
            trace_event::trace_event!("paint::main_thread_scroll_hit_test_region"),
            || paint::main_thread_scroll_hit_test_region(&artifact, pre, &layers),
        );
        let attribution = trace_event::in_scope(
            trace_event::trace_event!("paint::frame_attribution_region"),
            || paint::frame_attribution_region(&artifact, pre, &layers),
        );
        let sticky_boxes = self.sticky_boxes();
        let (painted_layers, mut property_trees) = trace_event::in_scope(
            trace_event::trace_event!("PaintArtifactCompositor::Update"),
            || {
                self.compositor.update(
                    &artifact,
                    &painted.trees,
                    self.document_size,
                    &region,
                    &attribution,
                    &sticky_boxes,
                    &|canvas| self.canvases.get(&canvas).cloned(),
                )
            },
        );
        trace_event::in_scope(
            trace_event::trace_event!("FrameView::set_wheel_event_handler_region"),
            || {
                set_wheel_event_handler_region(pre, &layers, &artifact, &mut property_trees, wheel);
            },
        );
        self.wheel_listeners_version = Some(wheel.version);
        self.output = Some(Arc::new(PaintOutput {
            layers: painted_layers,
            property_trees,
        }));
        self.painted_sticky = sticky;
        self.composited = composited;
        self.paint_dirty = false;
        self.canvas_textures.clear();
    }

    /// The canvases drawn before this frame (the renderer flushed them):
    /// each one's new snapshot, none for one that shows nothing (reset,
    /// transparent). One that is a layer, with a snapshot, takes it in its
    /// layer without a paint (Blink's `TextureLayer` of an accelerated
    /// canvas); the others are painted again.
    pub(crate) fn canvases_drawn(&mut self, drawn: Vec<(NodeId, Option<(gfx::ImageData, bool)>)>) {
        for (canvas, snapshot) in drawn {
            let layer = snapshot.is_some() && self.compositor.canvas_layer(canvas).is_some();
            match snapshot {
                Some(snapshot) => self.canvases.insert(canvas, snapshot),
                None => self.canvases.remove(&canvas),
            };
            if layer {
                self.canvas_textures.push(canvas);
            } else {
                self.paint_controller.invalidate([canvas]);
                self.paint_dirty = true;
            }
        }
    }

    /// The snapshots of the canvases that are gone, let go of.
    pub(crate) fn forget_canvases(&mut self, gone: impl Fn(NodeId) -> bool) {
        self.canvases.retain(|&canvas, _| !gone(canvas));
    }

    /// Finds again what is drawn as selected and the caret (Blink's
    /// `FrameSelection` of a text control: `TextControlElement`'s selection
    /// in its inner editor), after layout: the focused text control's
    /// selection, in the active colors while the page has the focus; its
    /// caret where the selection is collapsed, while the page has the
    /// focus, unless it is read-only (Chrome draws none there). Without
    /// one, the document's selection, and its caret in the focused
    /// editing host. What
    /// changed paints its blocks again (`invalidate`, `invalidate_caret`).
    fn update_selection_and_caret(
        &mut self,
        cx: &Cx<'_, '_>,
        token: &LocalToken,
        text: &mut TextContext,
    ) {
        let Some(box_tree) = self.box_tree.as_ref() else {
            return;
        };
        let document = self.document.get(cx.gc);
        let page = cx
            .window_of(document)
            .is_some_and(|window| cx.page_focused(window));
        let mut selected = HashMap::new();
        let mut caret = None;
        let control = cx
            .focused_element(document)
            .filter(|control| cx.selection_applies(*control));
        if control.is_none() {
            // The document's selection, in the active colors in the
            // focused frame of a page with the focus (Blink's
            // `FrameSelection::FrameIsFocusedAndActive`), and its caret
            // there only.
            let active =
                page && dom::NodeRef::ptr_eq(cx.focused_frame(cx.top_document(document)), document);
            crate::selection::add_document(
                cx,
                document,
                active,
                &|node| box_tree.is_rendered(node),
                &mut selected,
            );
            if active {
                caret = crate::selection::caret(cx, token, document, box_tree, text);
            }
        }
        if let Some(control) = control
            && let Some(selection) = cx.text_selection(control)
        {
            let editor_text = cx.inner_editor_text(control);
            if selection.start < selection.end {
                if let Some(node) = editor_text {
                    selected.insert(
                        cx.node_id(node),
                        paint::TextSelection {
                            range: selection.start..selection.end,
                            active: page,
                        },
                    );
                }
            } else if page
                && cx
                    .attribute_value_by_id(control, dom::names::local::READONLY)
                    .is_none()
            {
                caret = control_caret(cx, token, control, selection.start, box_tree, text);
            }
        }
        let mut changed = selection_changes(cx, &self.selected, &selected);
        self.selected = selected;
        // The child frames' document selections, painted with this frame.
        let mut children = std::mem::take(&mut self.children);
        for child in &mut children {
            child.update_document_selection(cx, page, &mut changed);
        }
        self.children = children;
        if !changed.is_empty() {
            self.paint_controller.invalidate(changed);
            self.paint_dirty = true;
        }
        if caret != self.caret {
            let blocks = [self.caret, caret].into_iter().flatten().map(|c| c.block);
            self.paint_controller.invalidate_caret(blocks);
            self.paint_dirty = true;
            self.caret = caret;
            self.caret_shown = true;
            self.caret_moved = true;
        }
    }

    /// A child frame's document selection, and its child frames', in the
    /// active colors in the focused frame of a page with the focus (`page`)
    /// and the inactive ones elsewhere (Blink draws an unfocused frame's
    /// selection inactive); what changed added to `changed`, for the
    /// top-level frame, which paints them, to paint again.
    fn update_document_selection(
        &mut self,
        cx: &Cx<'_, '_>,
        page: bool,
        changed: &mut Vec<NodeId>,
    ) {
        let mut selected = HashMap::new();
        if let Some(box_tree) = self.box_tree.as_ref() {
            let document = self.document.get(cx.gc);
            let active =
                page && dom::NodeRef::ptr_eq(cx.focused_frame(cx.top_document(document)), document);
            crate::selection::add_document(
                cx,
                document,
                active,
                &|node| box_tree.is_rendered(node),
                &mut selected,
            );
        }
        changed.extend(selection_changes(cx, &self.selected, &selected));
        self.selected = selected;
        for child in &mut self.children {
            child.update_document_selection(cx, page, changed);
        }
    }

    /// Whether the caret came, went or moved since this was last asked:
    /// its blinking starts again, shown (Blink's `FrameCaret::
    /// StartBlinkCaret`).
    pub(crate) fn take_caret_moved(&mut self) -> bool {
        std::mem::take(&mut self.caret_moved)
    }

    /// Whether there is a caret to blink.
    pub(crate) fn has_caret(&self) -> bool {
        self.caret.is_some()
    }

    /// The caret blinks: shown if it was hidden, else hidden; only its
    /// block paints again.
    pub(crate) fn blink_caret(&mut self) {
        let Some(caret) = self.caret else {
            return;
        };
        self.caret_shown = !self.caret_shown;
        self.paint_controller.invalidate_caret([caret.block]);
        self.paint_dirty = true;
    }

    /// The sticky boxes as layout has them, for the compositor's nodes.
    fn sticky_boxes(&self) -> compositing::StickyBoxes {
        let mut boxes = compositing::StickyBoxes {
            offsets: self.scroll.stuck.offsets.clone(),
            ..Default::default()
        };
        boxes
            .scroll
            .insert(compositor::ElementId::VIEWPORT, self.scroll.viewport);
        for (element, offset) in self.scroll_offsets() {
            boxes
                .scroll
                .insert(compositing::element_id(element), offset);
        }
        self.collect_sticky(&mut boxes);
        boxes
    }

    /// The frame tree's sticky boxes: this frame's constraints and
    /// offsets, then its children's.
    fn collect_sticky(&self, boxes: &mut compositing::StickyBoxes) {
        let positions = &self.scroll.sticky.boxes;
        for c in positions {
            let around = c
                .shifting_containing_block
                .map(|index| positions[index].key);
            boxes.constraints.insert(c.key, (c.clone(), around));
        }
        for child in &self.children {
            boxes.offsets.extend(&child.scroll.stuck.offsets);
            child.collect_sticky(boxes);
        }
    }

    /// How far each sticky box in the frame tree sticks and may move, the
    /// child frames' too: pre-paint offsets their boxes in the one paint of
    /// the tree.
    fn stuck(&self) -> paint::StickyState {
        let mut stuck = self.scroll.stuck.clone();
        self.collect_child_stuck(&mut stuck);
        stuck
    }

    fn collect_child_stuck(&self, stuck: &mut paint::StickyState) {
        for child in &self.children {
            stuck.offsets.extend(&child.scroll.stuck.offsets);
            stuck.reaches.extend(&child.scroll.stuck.reaches);
            child.collect_child_stuck(stuck);
        }
    }

    /// Every scroll offset in the frame tree but this frame's viewport's:
    /// its containers, and each child frame's viewport (by its document)
    /// and containers.
    fn scroll_offsets(&self) -> paint::ScrollOffsets {
        let mut offsets = self.scroll.containers.clone();
        self.collect_child_scroll_offsets(&mut offsets);
        offsets
    }

    fn collect_child_scroll_offsets(&self, offsets: &mut paint::ScrollOffsets) {
        for child in &self.children {
            offsets.insert(child.document_node, child.scroll.viewport);
            offsets.extend(&child.scroll.containers);
            child.collect_child_scroll_offsets(offsets);
        }
    }

    /// `element`'s box as last laid out.
    pub(crate) fn element_box(&self, element: NodeId) -> Option<ElementBox> {
        let mut found = layout::element_box(
            self.fragment.as_ref()?,
            &self.scroll.containers,
            &self.scroll.stuck.offsets,
            element,
        )?;
        // A sticky box's insets are resolved against its scrollport,
        // which only its constraints know.
        if let Some(constraints) = self.scroll.sticky.get((element, None))
            && let Some(block) = &mut found.block
        {
            for (inset, resolved) in block.inset.iter_mut().zip(constraints.resolved_inset) {
                *inset = resolved.unwrap_or(0.0);
            }
        }
        Some(found)
    }

    /// `element`'s boxes' border boxes as last laid out, CSS px of the
    /// viewport.
    pub(crate) fn client_rects(&self, element: NodeId) -> Vec<RectF> {
        let Some(fragment) = &self.fragment else {
            return Vec::new();
        };
        // A box fixed to the viewport is laid out against it, so layout
        // already placed it where it shows.
        let at = if layout::is_fixed(fragment, element) {
            PointF::new(0.0, 0.0)
        } else {
            self.scroll.viewport
        };
        layout::client_rects(
            fragment,
            &self.scroll.containers,
            &self.scroll.stuck.offsets,
            element,
        )
        .into_iter()
        .map(|rect| rect.offset(-at.x, -at.y))
        .collect()
    }

    /// `element`'s box model as last laid out, CSS px of the viewport, as
    /// `client_rects` places it (`layout::box_quads`).
    /// Each document's (this view's and those below) style's count of its
    /// sheets' collections (`style::StyleEngine::sheets_version`).
    pub(crate) fn sheets_versions(&self, versions: &mut Vec<(NodeId, u64)>) {
        versions.push((self.document_node, self.style.sheets_version()));
        for child in &self.children {
            child.sheets_versions(versions);
        }
    }

    /// The fonts of the text runs `counts` picks, as last laid out
    /// (`layout::platform_fonts`).
    pub(crate) fn platform_fonts(
        &self,
        counts: &dyn Fn(&layout::TextFragment) -> bool,
    ) -> Vec<layout::PlatformFont> {
        self.fragment
            .as_ref()
            .map(|fragment| layout::platform_fonts(fragment, counts))
            .unwrap_or_default()
    }

    /// What `element`'s own style is cascaded from, as its document's
    /// last style recalc matched it (`style::StyleEngine::explain`).
    pub(crate) fn explain_style<'gc>(
        &mut self,
        cx: &Cx<'_, 'gc>,
        element: NodeRef<'gc>,
    ) -> Option<style::Explained> {
        self.style.explain(cx, element)
    }

    /// The `@keyframes` named `name` that `element`'s animations find
    /// (`style::StyleEngine::keyframes_rule`).
    pub(crate) fn keyframes_rule<'gc>(
        &self,
        cx: &Cx<'_, 'gc>,
        element: NodeRef<'gc>,
        name: &str,
    ) -> Option<Arc<css::KeyframesRule>> {
        self.style.keyframes_rule(cx, element, name)
    }

    /// `element`'s pseudo-element `pseudo`'s box model (its own, if none),
    /// as `layout::box_quads` has it, in the viewport.
    pub(crate) fn pseudo_box_quads(
        &self,
        element: NodeId,
        pseudo: Option<computed_style::PseudoId>,
    ) -> Option<layout::BoxQuads> {
        let fragment = self.fragment.as_ref()?;
        let quads = layout::pseudo_box_quads(
            fragment,
            &self.scroll.containers,
            &self.scroll.stuck.offsets,
            element,
            pseudo,
        )?;
        let at = self.viewport_origin_of(fragment, element);
        let moved = |quad: layout::Quad| quad.map(|p| PointF::new(p.x - at.x, p.y - at.y));
        Some(layout::BoxQuads {
            content: moved(quads.content),
            padding: moved(quads.padding),
            border: moved(quads.border),
            margin: moved(quads.margin),
        })
    }

    /// `element`'s border boxes as quads as last laid out, CSS px of the
    /// viewport (`layout::border_quads`).
    pub(crate) fn border_quads(&self, element: NodeId) -> Vec<layout::Quad> {
        let Some(fragment) = &self.fragment else {
            return Vec::new();
        };
        let at = self.viewport_origin_of(fragment, element);
        layout::border_quads(
            fragment,
            &self.scroll.containers,
            &self.scroll.stuck.offsets,
            element,
        )
        .into_iter()
        .map(|quad| quad.map(|p| PointF::new(p.x - at.x, p.y - at.y)))
        .collect()
    }

    /// Where the viewport is in the document for `element`'s boxes: its
    /// scroll offset, or nowhere for a box fixed to it, which layout
    /// placed where it shows.
    fn viewport_origin_of(&self, fragment: &layout::BoxFragment, element: NodeId) -> PointF {
        if layout::is_fixed(fragment, element) {
            PointF::new(0.0, 0.0)
        } else {
            self.scroll.viewport
        }
    }

    /// Whether `element` is a box the user scrolls: a scroll container
    /// with more than its padding box to show (Blink's
    /// `LayoutBox::IsUserScrollable`).
    pub(crate) fn is_user_scrollable(&self, element: NodeId) -> bool {
        let Some(found) = self.element_box(element) else {
            return false;
        };
        let Some(block) = found.block.filter(|block| block.scroll.is_some()) else {
            return false;
        };
        let border = found.border_box;
        let width = border.width - block.border[1] - block.border[3];
        let height = border.height - block.border[0] - block.border[2];
        block.scroll_size.width > width || block.scroll_size.height > height
    }

    /// The boxes of text `text`'s data from `start` to `end`, CSS px of the
    /// viewport (`layout::text_rects` less the viewport's scroll offset,
    /// unless `element`, the one it is in, is fixed to it, as
    /// `client_rects` has it); empty without a box or a layout.
    pub(crate) fn text_rects(
        &self,
        text: NodeId,
        element: Option<NodeId>,
        start: u32,
        end: u32,
    ) -> Vec<RectF> {
        let Some(fragment) = &self.fragment else {
            return Vec::new();
        };
        let at = if element.is_some_and(|element| layout::is_fixed(fragment, element)) {
            PointF::new(0.0, 0.0)
        } else {
            self.scroll.viewport
        };
        layout::text_rects(
            fragment,
            &self.scroll.containers,
            &self.scroll.stuck.offsets,
            text,
            start,
            end,
        )
        .into_iter()
        .map(|rect| rect.offset(-at.x, -at.y))
        .collect()
    }

    /// The first place a caret can be in editing host `host`, as last laid
    /// out (`layout::first_caret_position`); its start without a layout.
    pub(crate) fn first_caret_position<'gc>(
        &self,
        cx: &Cx<'_, 'gc>,
        host: NodeRef<'gc>,
    ) -> (NodeRef<'gc>, u32) {
        match (&self.box_tree, &self.fragment) {
            (Some(box_tree), Some(fragment)) => {
                layout::first_caret_position(cx, box_tree, fragment, host)
            }
            _ => (host, 0),
        }
    }

    /// An SVG element's geometry, CSS px of the viewport, as last laid out
    /// (`layout::svg_geometry` less the viewport's scroll offset); the
    /// default if no `<svg>` around it has a box.
    pub(crate) fn svg_geometry(&self, element: SvgElement<'_, '_>) -> SvgGeometry {
        let Some(fragment) = &self.fragment else {
            return SvgGeometry::default();
        };
        let Some(mut geometry) = layout::svg_geometry(
            fragment,
            &self.scroll.containers,
            &self.scroll.stuck.offsets,
            element,
        ) else {
            return SvgGeometry::default();
        };
        let at = self.scroll.viewport;
        geometry.screen_ctm = Transform::translate(-at.x, -at.y).then(&geometry.screen_ctm);
        geometry.client_rect = geometry.client_rect.map(|rect| rect.offset(-at.x, -at.y));
        geometry
    }

    /// An SVG shape's path for its length, as laid out
    /// (`layout::svg_shape_path`).
    pub(crate) fn svg_shape_path(&self, element: SvgElement<'_, '_>) -> Option<gfx::Path> {
        layout::svg_shape_path(self.fragment.as_ref()?, element)
    }

    /// Whether `point` is in an SVG shape's fill or stroke area, as laid
    /// out (`paint::svg_shape_contains`); `false` without a layout node.
    pub(crate) fn svg_point_in_shape(
        &self,
        element: SvgElement<'_, '_>,
        point: PointF,
        stroke: bool,
    ) -> bool {
        self.fragment
            .as_ref()
            .and_then(|fragment| layout::svg_node(fragment, element))
            .is_some_and(|node| paint::svg_shape_contains(node, point, stroke))
    }

    /// An SVG text element's characters, as laid out.
    pub(crate) fn svg_text_chars(&self, element: SvgElement<'_, '_>) -> Vec<SvgTextChar> {
        self.fragment
            .as_ref()
            .map(|fragment| layout::svg_text_chars(fragment, element))
            .unwrap_or_default()
    }

    /// The viewport as last laid out.
    pub(crate) fn script_viewport(&self) -> Viewport {
        Viewport {
            size: self.viewport,
            device_scale_factor: self.device_scale_factor,
            scroll: self.scroll.viewport,
            content_size: self.document_size,
        }
    }

    /// The topmost element at `point` (CSS px of the viewport) as laid
    /// out (a hit test), pre-painted first if it isn't; none before the
    /// first layout or outside every box.
    pub(crate) fn element_at(&mut self, point: PointF) -> Option<NodeId> {
        self.hit_test(point).element
    }

    /// `element_at`, and whether what it hit there is a run of the
    /// element's text.
    pub(crate) fn hit_test(&mut self, point: PointF) -> dom_api::Hit {
        if self.fragment.is_none() {
            return dom_api::Hit::default();
        }
        self.update_pre_paint();
        let Some((fragment, pre)) = self.pre.as_ref() else {
            return dom_api::Hit::default();
        };
        let viewport = self.scroll.viewport;
        let hit = paint::hit_test(
            fragment,
            pre,
            PointF::new(point.x + viewport.x, point.y + viewport.y),
        );
        dom_api::Hit {
            element: hit.node,
            text: hit.text,
        }
    }

    /// A wheel at `point` (CSS px of the viewport) scrolling by `delta` on
    /// the main thread (the compositor sent it here: a container under it
    /// isn't a layer, or a listener there may have canceled it):
    /// pre-painted first; the innermost container under it that can move
    /// takes what it can, those around it the rest, through the child
    /// frames' viewports out to their iframes' frames, then the viewport.
    /// Whether anything moved; a container that isn't a layer is painted
    /// where it scrolled to.
    pub(crate) fn wheel(&mut self, point: PointF, delta: PointF) -> bool {
        if self.fragment.is_none() {
            return false;
        }
        self.update_pre_paint();
        let Some((fragment, pre)) = &self.pre else {
            return false;
        };
        let before = self.scroll_offsets();
        let children = &mut self.children;
        let scrolled = self.scroll.wheel(
            pre,
            fragment,
            point,
            delta,
            &mut |iframe, element, scroller, rest| {
                let Some(child) = child_view_mut(children, iframe) else {
                    return false;
                };
                let target = (child.document_node != element).then_some(element);
                child.scroll.scroll_by(target, scroller, rest)
            },
        );
        let moved = self.containers_moved(&before);
        scrolled.viewport || moved
    }

    /// A scrolling key (`ScrollState::keyboard`) from the boxes around
    /// `chain` (the focused element, or what the mouse last went down on,
    /// and its ancestors, each frame's document among them, out through
    /// the frames), in this frame or a child frame: pre-painted first.
    /// Whether anything moved; what moved is painted again or (a layer)
    /// moved by the compositor at the commit, as script's scrolling is.
    pub(crate) fn keyboard_scroll(
        &mut self,
        chain: &[NodeId],
        key: ScrollKey,
        shift: bool,
    ) -> bool {
        if self.fragment.is_none() {
            return false;
        }
        self.update_pre_paint();
        let Some((_, pre)) = &self.pre else {
            return false;
        };
        let before = self.scroll_offsets();
        let children = &mut self.children;
        let scrolled = self.scroll.keyboard(
            pre,
            chain,
            key,
            shift,
            &mut |iframe, element, scroller, step| {
                let Some(child) = child_view_mut(children, iframe) else {
                    return false;
                };
                let target = (child.document_node != element).then_some(element);
                child.scroll.scroll_by(target, scroller, step)
            },
        );
        let moved = self.containers_moved(&before) || scrolled.viewport;
        self.scrolled_by_script |= moved;
        moved
    }

    /// Whether a scroll offset in the frame tree moved since `before`
    /// (`scroll_offsets`'s), a container that isn't a layer painted where
    /// it scrolled to.
    fn containers_moved(&mut self, before: &paint::ScrollOffsets) -> bool {
        let after = self.scroll_offsets();
        let moved: Vec<NodeId> = after
            .iter()
            .filter(|(element, offset)| {
                before.get(element).copied().unwrap_or_default() != **offset
            })
            .map(|(element, _)| *element)
            .collect();
        self.paint_dirty |= moved
            .iter()
            .any(|element| !self.composited.contains(element));
        !moved.is_empty()
    }

    /// `point` (CSS px of this frame's viewport) in the viewport of the
    /// frame below showing `document` (Blink's `ConvertFromRootFrame`):
    /// through its iframes' transforms and scroll offsets, pre-painted
    /// first. None if no frame below shows it, or before the first layout.
    pub(crate) fn point_in_frame(&mut self, document: NodeId, point: PointF) -> Option<PointF> {
        if document == self.document_node {
            return Some(point);
        }
        let (iframe, _) = self.view_of(document)?.owner?;
        self.fragment.as_ref()?;
        self.update_pre_paint();
        let (_, pre) = self.pre.as_ref()?;
        let viewport = self.scroll.viewport;
        pre.point_in_child_frame(
            iframe,
            PointF::new(point.x + viewport.x, point.y + viewport.y),
        )
    }

    /// `point_in_frame` the other way: `point` in the viewport of the frame
    /// showing `document` in this frame's.
    pub(crate) fn point_from_frame(&mut self, document: NodeId, point: PointF) -> Option<PointF> {
        if document == self.document_node {
            return Some(point);
        }
        let (iframe, _) = self.view_of(document)?.owner?;
        self.fragment.as_ref()?;
        self.update_pre_paint();
        let (_, pre) = self.pre.as_ref()?;
        let viewport = self.scroll.viewport;
        let at = pre.point_from_child_frame(iframe, point)?;
        Some(PointF::new(at.x - viewport.x, at.y - viewport.y))
    }

    /// Every element at `point` (CSS px of the viewport), the topmost
    /// first (a hit test), pre-painted first; none before the first
    /// layout.
    pub(crate) fn elements_at(&mut self, point: PointF) -> Vec<NodeId> {
        if self.fragment.is_none() {
            return Vec::new();
        }
        self.update_pre_paint();
        let Some((fragment, pre)) = self.pre.as_ref() else {
            return Vec::new();
        };
        let viewport = self.scroll.viewport;
        paint::hit_test_all(
            fragment,
            pre,
            PointF::new(point.x + viewport.x, point.y + viewport.y),
        )
    }

    /// The viewport's scroll offset, for the compositor.
    pub(crate) fn viewport_scroll_offset(&self) -> PointF {
        self.scroll.viewport
    }

    /// The viewport as a scroller, for the compositor; `None` before the
    /// first layout.
    pub(crate) fn viewport_scroll_node(&self) -> Option<ScrollNode> {
        self.scroll.viewport_node
    }

    /// The offsets of the scroll containers that are layers, for the
    /// compositor, which moves them.
    pub(crate) fn compositor_scroll_offsets(&self) -> compositor::ScrollOffsets {
        self.scroll_offsets()
            .into_iter()
            .filter(|(element, _)| self.composited.contains(element))
            .map(|(element, offset)| (compositing::element_id(element), offset))
            .collect()
    }

    /// What the compositor scrolled: the viewport, and the containers that
    /// were layers in the last commit it had. Whether a main frame is due:
    /// a container no longer a layer is painted where it scrolled to.
    pub(crate) fn apply_compositor_scroll(&mut self, scroll: &CompositorScrollDelta) -> bool {
        let Some(fragment) = self.fragment.clone() else {
            return false;
        };
        let before = self.scroll_offsets();
        self.scroll
            .apply_compositor_delta(&fragment, scroll.viewport, &scroll.containers);
        apply_child_compositor_deltas(&mut self.children, &scroll.containers);
        let after = self.scroll_offsets();
        let repaint = after
            .iter()
            .filter(|(element, offset)| {
                before.get(element).copied().unwrap_or_default() != **offset
            })
            .any(|(element, _)| !self.composited.contains(element));
        self.paint_dirty |= repaint;
        repaint
    }
}

/// The child frame view, at any depth in `children`, of iframe `iframe`
/// (`frame_view`'s `child_frame_mut`).
fn child_view_mut(children: &mut [FrameView], iframe: NodeId) -> Option<&mut FrameView> {
    children.iter_mut().find_map(|child| {
        if child.owner.is_some_and(|(owner, _)| owner == iframe) {
            Some(child)
        } else {
            child_view_mut(&mut child.children, iframe)
        }
    })
}

/// The compositor's deltas of the child frames' viewports (by their
/// documents) and containers, each applied in its frame.
fn apply_child_compositor_deltas(
    children: &mut [FrameView],
    containers: &[(compositor::ElementId, PointF)],
) {
    for child in children {
        if let Some(fragment) = child.fragment.clone() {
            let document = compositing::element_id(child.document_node);
            let viewport = containers
                .iter()
                .find(|(element, _)| *element == document)
                .map_or(PointF::new(0.0, 0.0), |(_, delta)| *delta);
            child
                .scroll
                .apply_compositor_delta(&fragment, viewport, containers);
        }
        apply_child_compositor_deltas(&mut child.children, containers);
    }
}

/// What the compositor's wheel event handler region is made from besides
/// paint: whether the DOM may have `wheel` listeners, whether the whole
/// document waits (its root element, it or its window has a blocking one),
/// and the DOM's count of their changes.
struct WheelListeners {
    any: bool,
    whole_document: bool,
    version: u64,
}

/// Puts the wheel event handler region of `artifact` (paint's hit test
/// data, `pre` its pre-paint, `composited` the containers that are layers)
/// in `trees`.
fn set_wheel_event_handler_region(
    pre: &PrePaint,
    composited: &[bool],
    artifact: &paint::PaintArtifact,
    trees: &mut compositor::PropertyTrees,
    wheel: &WheelListeners,
) {
    let composited_sticky = paint::composited_sticky(&pre.trees, composited);
    let region = paint::wheel_event_handler_region(artifact, pre, composited, wheel.whole_document);
    compositing::set_wheel_event_handler_region(
        trees,
        composited,
        &composited_sticky,
        &region,
        wheel.any,
    );
}

/// The caret of text control `control` with its selection collapsed at
/// `offset` of its value, in its inner editor's text; with an empty value
/// (no line), where the inner editor's line would be.
fn control_caret<'gc>(
    cx: &Cx<'_, 'gc>,
    token: &LocalToken,
    control: NodeRef<'gc>,
    offset: u32,
    box_tree: &BoxTree,
    text: &mut TextContext,
) -> Option<paint::Caret> {
    let id = cx.node_id(cx.inner_editor_text(control)?);
    if let Some(block) = box_tree.block_of(id) {
        return Some(paint::Caret {
            at: paint::CaretPosition::Text { text: id, offset },
            block,
        });
    }
    let editor = cx.inner_editor(control)?;
    let style = cx.computed_style(editor, token)?;
    let (ascent, descent, line_height) = layout::empty_caret_metrics(text, style);
    Some(paint::Caret {
        at: paint::CaretPosition::Empty {
            element: cx.node_id(editor),
            ascent,
            descent,
            line_height,
        },
        block: cx.node_id(editor),
    })
}

/// `document`'s caret where the user could type, whether or not the page
/// has the focus: its focused text control's (its selection collapsed,
/// not read-only), else the collapsed selection's in its focused editing
/// host (`selection::caret`). What `focus()` scrolls into view.
fn focused_caret<'gc>(
    cx: &Cx<'_, 'gc>,
    token: &LocalToken,
    document: NodeRef<'gc>,
    box_tree: &BoxTree,
    text: &mut TextContext,
) -> Option<paint::Caret> {
    let control = cx
        .focused_element(document)
        .filter(|control| cx.selection_applies(*control));
    let Some(control) = control else {
        return crate::selection::caret(cx, token, document, box_tree, text);
    };
    let selection = cx.text_selection(control)?;
    if selection.start < selection.end
        || cx
            .attribute_value_by_id(control, dom::names::local::READONLY)
            .is_some()
    {
        return None;
    }
    control_caret(cx, token, control, selection.start, box_tree, text)
}

/// The frame's layout as the engine holds it (`dom_api::FrameLayout`):
/// its view, the thread's token (style writes with it, all else reads) and
/// text, and the viewport the renderer last gave it.
pub(crate) struct Frame {
    pub(crate) view: FrameView,
    document: NodeId,
    pub(crate) token: LocalToken,
    pub(crate) text: TextContext,
    viewport: SizeF,
    device_scale_factor: f32,
    window_screen_rect: Option<RectF>,
    /// Where its canvases' bitmaps are drawn off the main thread
    /// (`canvas_raster`), its own.
    pub(crate) canvas_raster: crate::canvas_raster::CanvasRaster,
    pub(crate) svg_images: SvgImages,
    /// The document whose frame the queries are of (`shows`).
    selected: NodeId,
    /// The frame's widget (the thread's until the first frame takes it).
    pub(crate) widget: crate::frame_widget::FrameWidget,
}

impl Frame {
    /// `view`, of the document whose id is `document`, with the thread's
    /// token, text and widget.
    pub(crate) fn new(
        view: FrameView,
        document: NodeId,
        token: LocalToken,
        text: TextContext,
        widget: crate::frame_widget::FrameWidget,
    ) -> Self {
        Self {
            view,
            document,
            token,
            text,
            widget,
            viewport: SizeF::default(),
            device_scale_factor: 1.0,
            window_screen_rect: None,
            canvas_raster: crate::canvas_raster::CanvasRaster::default(),
            svg_images: SvgImages::default(),
            selected: document,
        }
    }

    /// The thread's text and widget back, as the page takes another engine
    /// (another DOM): the view, of the DOM that goes, and the token go.
    pub(crate) fn into_thread_parts(self) -> (TextContext, crate::frame_widget::FrameWidget) {
        (self.text, self.widget)
    }

    /// The view the queries are of (`FrameLayout::shows`).
    fn selected(&self) -> &FrameView {
        self.view.view_of(self.selected).unwrap_or(&self.view)
    }

    fn selected_mut(&mut self) -> &mut FrameView {
        let selected = self.selected;
        if self.view.view_of(selected).is_none() {
            return &mut self.view;
        }
        self.view.view_of_mut(selected).expect("just found")
    }

    /// Draws what `canvas`'s context recorded onto its bitmap, or sends it
    /// to the canvas raster thread, as `flush` says (`canvas::flush_data`);
    /// nothing for an element with no context. Its new snapshot, if it has
    /// one, and whether it is opaque.
    pub(crate) fn flush_canvas(
        &mut self,
        cx: &Cx<'_, '_>,
        canvas: NodeId,
        flush: canvas::CanvasFlush,
        items: &mut Vec<crate::canvas_raster::Item>,
    ) -> Option<(gfx::ImageData, bool)> {
        let element = cx.node_by_id(canvas)?;
        let context = cx.canvas_context(element)?;
        let settings = cx.canvas_context_settings(context);
        let raster = &mut self.canvas_raster;
        cx.with_canvas_data(context, |data| {
            crate::canvas::flush_data(data, settings, flush, raster, items);
            data.snapshot(canvas::background(settings))
                .map(|snapshot| (snapshot, !settings.alpha))
        })
    }

    /// Before a frame is painted: draws the canvases with something
    /// recorded (or reset) that are in the frame's document, and gives the
    /// view their snapshots, to be painted again or swapped into their
    /// layers; the others (not in a document, or in another) keep their
    /// recordings until script reads their pixels, or records too much.
    /// What is for the thread goes as one batch, with the bitmaps no
    /// context holds any more let go of; the snapshots of canvases that are
    /// gone too.
    pub(crate) fn flush_shown_canvases(&mut self, dom: &mut Dom) {
        let mut items = self.canvas_raster.releases();
        let drawn = dom.mutate(|cx| {
            self.view
                .forget_canvases(|canvas| cx.node_by_id(canvas).is_none());
            let draws = cx.take_canvas_draws();
            if draws.is_empty() {
                return Vec::new();
            }
            let mut kept = Vec::new();
            let mut drawn = Vec::new();
            for canvas in draws {
                // In a document of the frame tree: the top-level view,
                // which paints them all, keeps its snapshot.
                let shown = cx.node_by_id(canvas).is_some_and(|node| {
                    cx.is_connected(node)
                        && self
                            .view
                            .view_of(cx.node_id(cx.document_of(node)))
                            .is_some()
                });
                if shown {
                    drawn.push((
                        canvas,
                        self.flush_canvas(cx, canvas, canvas::CanvasFlush::Read, &mut items),
                    ));
                } else {
                    kept.push(canvas);
                }
            }
            cx.keep_canvas_draws(kept);
            drawn
        });
        self.canvas_raster.send(items, true);
        self.view.canvases_drawn(drawn);
    }

    /// Another document's view, the token and the text kept.
    pub(crate) fn show(&mut self, view: FrameView, document: NodeId) {
        self.view = view;
        self.document = document;
        self.selected = document;
    }

    /// The viewport and device scale factor layout is at from now on
    /// (script's and the next frame's).
    /// The frame's viewport and device scale factor: whether either
    /// changed (the document's environment did).
    /// What the media queries of `document`'s frame see now: the window's
    /// viewport for the top-level frame's, a child frame's iframe's size
    /// (from its parent's layout); none for a child frame not given one
    /// yet, or a document no frame shows.
    pub(crate) fn media_environment(
        &self,
        document: NodeId,
    ) -> Option<css::media::MediaEnvironment> {
        let (viewport, device_pixel_ratio) = if document == self.view.document_node {
            (self.viewport, self.device_scale_factor)
        } else {
            let view = self.view.view_of(document)?;
            if view.fragment.is_none() && !view.needs_layout {
                return None;
            }
            (view.viewport, view.device_scale_factor)
        };
        Some(css::media::MediaEnvironment {
            width: viewport.width,
            height: viewport.height,
            device_pixel_ratio,
        })
    }

    pub(crate) fn set_viewport(&mut self, viewport: SizeF, device_scale_factor: f32) -> bool {
        let changed = (viewport, device_scale_factor) != (self.viewport, self.device_scale_factor);
        self.viewport = viewport;
        self.device_scale_factor = device_scale_factor;
        changed
    }

    /// The browser window's rect on the screen, as the browser last told
    /// it (`RendererMsg::UpdateScreenRects`).
    pub(crate) fn set_window_screen_rect(&mut self, rect: Option<RectF>) {
        self.window_screen_rect = rect;
    }

    /// The frame's lifecycle (`FrameView::update_all_lifecycle_phases`)
    /// at the viewport it was last given.
    pub(crate) fn update_all_lifecycle_phases(&mut self, dom: &mut Dom) -> Arc<PaintOutput> {
        let Self {
            view,
            token,
            text,
            svg_images,
            viewport,
            device_scale_factor,
            ..
        } = self;
        view.update_all_lifecycle_phases(
            dom,
            token,
            text,
            svg_images,
            *viewport,
            *device_scale_factor,
        )
    }
}

impl Frame {
    /// Style, layout and pre-paint brought up to date, in the operation
    /// `cx` (a round of the resize observer loop: every entry's
    /// device-pixel size takes the paint offset).
    pub(crate) fn update_lifecycle_to_pre_paint(&mut self, cx: &Cx<'_, '_>) {
        dom_api::FrameLayout::update_style_and_layout(self, cx);
        self.view.update_pre_paint();
    }

    /// `element`'s pseudo-element `pseudo`'s box model (its own, if none),
    /// of `document`'s frame, in the top-level
    /// frame's viewport (Blink's `FrameQuadToViewport`): as last laid out
    /// and pre-painted; none without a box or a frame showing `document`.
    pub(crate) fn pseudo_box_quads_in_top(
        &mut self,
        document: NodeId,
        element: NodeId,
        pseudo: Option<computed_style::PseudoId>,
    ) -> Option<layout::BoxQuads> {
        let quads = self
            .view
            .view_of(document)?
            .pseudo_box_quads(element, pseudo)?;
        Some(layout::BoxQuads {
            content: self.quad_in_top(document, quads.content)?,
            padding: self.quad_in_top(document, quads.padding)?,
            border: self.quad_in_top(document, quads.border)?,
            margin: self.quad_in_top(document, quads.margin)?,
        })
    }

    /// `element`'s border boxes, of `document`'s frame, as quads in the
    /// top-level frame's viewport.
    pub(crate) fn border_quads_in_top(
        &mut self,
        document: NodeId,
        element: NodeId,
    ) -> Vec<layout::Quad> {
        let quads = self
            .view
            .view_of(document)
            .map(|view| view.border_quads(element))
            .unwrap_or_default();
        quads
            .into_iter()
            .filter_map(|quad| self.quad_in_top(document, quad))
            .collect()
    }

    /// `quad`, in the viewport of the frame showing `document`, in the
    /// top-level frame's.
    fn quad_in_top(&mut self, document: NodeId, quad: layout::Quad) -> Option<layout::Quad> {
        let [a, b, c, d] = quad;
        Some([
            self.view.point_from_frame(document, a)?,
            self.view.point_from_frame(document, b)?,
            self.view.point_from_frame(document, c)?,
            self.view.point_from_frame(document, d)?,
        ])
    }

    /// Whether `element`, of `document`'s frame, is a box the user scrolls.
    pub(crate) fn is_user_scrollable(&self, document: NodeId, element: NodeId) -> bool {
        self.view
            .view_of(document)
            .is_some_and(|view| view.is_user_scrollable(element))
    }

    /// What is at `point`, CSS px of the top-level frame's viewport, through
    /// the frames in it: pre-painted first.
    pub(crate) fn hit_test_in_top(&mut self, point: PointF) -> dom_api::Hit {
        self.view.hit_test(point)
    }

    /// The top-level frame's viewport's scroll offset.
    pub(crate) fn top_viewport_scroll(&self) -> PointF {
        self.view.viewport_scroll_offset()
    }

    /// The top-level frame's viewport's size, CSS px.
    pub(crate) fn viewport_size(&self) -> SizeF {
        self.viewport
    }

    /// `text` on one line in the UI's font (sans-serif), `size` px, bold
    /// or not: DevTools' overlay's labels.
    pub(crate) fn shape_label(
        &mut self,
        text: &str,
        size: f32,
        bold: bool,
    ) -> Option<gfx::ShapedText> {
        let style = computed_style::CanvasTextStyle {
            font: computed_style::CanvasFont {
                family: Arc::new([computed_style::FontFamily::Generic(
                    computed_style::GenericFamily::SansSerif,
                )]),
                size,
                weight: if bold { 600.0 } else { 400.0 },
                ..computed_style::CanvasFont::default()
            },
            direction: computed_style::Direction::Ltr,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            kerning: computed_style::FontKerning::Auto,
        };
        Some(layout::shape_canvas_text(&mut self.text, text, &style))
    }

    /// `FrameView::wheel`, style and layout brought up to date first
    /// (the `wheel` event's listeners may have changed the DOM).
    pub(crate) fn wheel(&mut self, dom: &mut Dom, point: PointF, delta: PointF) -> bool {
        dom.mutate(|cx| dom_api::FrameLayout::update_style_and_layout(self, cx));
        self.view.wheel(point, delta)
    }
}

impl dom_api::FrameLayout for Frame {
    /// The frame whose view shows it is the one queries are of from now
    /// on.
    fn shows(&mut self, document: NodeId) -> bool {
        if self.view.view_of(document).is_none() {
            return false;
        }
        self.selected = document;
        true
    }

    fn update_style_and_layout(&mut self, cx: &Cx<'_, '_>) {
        let Self {
            view,
            token,
            text,
            viewport,
            device_scale_factor,
            ..
        } = self;
        view.update_style_and_layout(cx, token, text, *viewport, *device_scale_factor);
    }

    /// The shown document's frame's style only: its own (a child frame's at
    /// the size it was last laid out at).
    fn update_style(&mut self, cx: &Cx<'_, '_>) {
        let selected = self.selected;
        let Self {
            view,
            token,
            viewport,
            device_scale_factor,
            document,
            ..
        } = self;
        // The top-level frame's size is the window's at once (Blink's frame
        // view has it before any layout): its media queries go by it.
        if selected == *document {
            view.set_viewport_unstyled(*viewport, *device_scale_factor);
        }
        let frame = if view.view_of(selected).is_some() {
            view.view_of_mut(selected).expect("just found")
        } else {
            view
        };
        frame.update_style(cx, token);
    }

    fn element_box(&self, node: NodeId) -> Option<ElementBox> {
        self.selected().element_box(node)
    }

    /// The top-level frame's pre-paint's, which went into the child
    /// frames: a child frame isn't pre-painted on its own.
    fn paint_offset(&self, node: NodeId) -> Option<PointF> {
        self.view.paint_offset(node)
    }

    fn system_colors(&self) -> gfx::SystemColors {
        self.view.style.system_colors()
    }

    fn flush_canvas(&mut self, cx: &Cx<'_, '_>, canvas: NodeId, flush: canvas::CanvasFlush) {
        let mut items = Vec::new();
        self.flush_canvas(cx, canvas, flush, &mut items);
        self.canvas_raster.send(items, false);
    }

    fn shape_canvas_text(
        &mut self,
        text: &str,
        style: &computed_style::CanvasTextStyle,
        ink: bool,
    ) -> Option<gfx::ShapedText> {
        let mut shaped = layout::shape_canvas_text(&mut self.text, text, style);
        if ink {
            shaped.ink = raster::ink_bounds(&shaped.runs);
        }
        Some(shaped)
    }

    fn encode_image(
        &mut self,
        bitmap: &canvas::export::ExportBitmap,
        encoding: canvas::export::ImageEncoding,
    ) -> Option<Vec<u8>> {
        crate::canvas_encoder::encode_bitmap(bitmap, encoding)
    }

    fn client_rects(&self, node: NodeId) -> Vec<RectF> {
        self.selected().client_rects(node)
    }

    fn text_rects(
        &self,
        text: NodeId,
        element: Option<NodeId>,
        start: u32,
        end: u32,
    ) -> Vec<RectF> {
        self.selected().text_rects(text, element, start, end)
    }

    fn first_caret_position<'gc>(
        &self,
        cx: &Cx<'_, 'gc>,
        host: NodeRef<'gc>,
    ) -> (NodeRef<'gc>, u32) {
        self.selected().first_caret_position(cx, host)
    }

    /// The window's size before its first layout, the document laid out
    /// (Blink's lays out a main frame's own while it has none, as the
    /// initial scale may follow from it), as it is after; a child frame's,
    /// the frames around it laid out.
    fn viewport_size(&mut self, cx: &Cx<'_, '_>) -> SizeF {
        if self.selected == self.document {
            if self.view.fragment.is_none() {
                dom_api::FrameLayout::update_style_and_layout(self, cx);
            }
            return self.viewport;
        }
        let Self {
            view,
            token,
            text,
            viewport,
            device_scale_factor,
            selected,
            ..
        } = self;
        view.update_frames_around(cx, token, text, *viewport, *device_scale_factor, *selected);
        self.selected().viewport
    }

    fn child_frame_created(
        &mut self,
        frame: dom_api::FrameId,
        parent: NodeId,
        owner: NodeId,
        document: NodeRoot,
        document_node: NodeId,
    ) {
        let system_colors = self.view.style.system_colors();
        let view = FrameView::new(
            document,
            document_node,
            DocumentId::default(),
            system_colors,
        )
        .into_child(frame, owner);
        self.view.show_new_child(parent, view);
    }

    /// The window's size and scale as they are, before a layout takes
    /// them, as Blink's frame view has its size at once.
    /// A child frame's, as its iframe last laid it out.
    fn viewport(&self) -> Viewport {
        if self.selected != self.document {
            return self.selected().script_viewport();
        }
        Viewport {
            size: self.viewport,
            device_scale_factor: self.device_scale_factor,
            ..self.view.script_viewport()
        }
    }

    fn has_viewport(&self) -> bool {
        self.selected == self.document || self.selected().fragment.is_some()
    }

    fn window_screen_rect(&self) -> Option<RectF> {
        self.window_screen_rect
    }

    fn computed_style<'gc>(
        &self,
        cx: &Cx<'_, 'gc>,
        node: NodeRef<'gc>,
    ) -> Option<Arc<computed_style::ComputedStyle>> {
        cx.computed_style(node, &self.token).cloned()
    }

    fn pseudo_style<'gc>(
        &self,
        cx: &Cx<'_, 'gc>,
        element: NodeRef<'gc>,
        pseudo: computed_style::PseudoId,
    ) -> Option<Arc<computed_style::ComputedStyle>> {
        cx.element_style(element, &self.token)?
            .pseudo
            .as_ref()?
            .get(pseudo)
            .cloned()
    }

    /// A child frame's point hit through the top-level frame's pre-paint,
    /// which its frame is part of (it has none of its own).
    /// The shown document's frame's own hit test, through the frames in
    /// it: not what its parent paints over its iframe (Blink hit tests
    /// the frame of the document asked).
    fn element_at(&mut self, point: PointF) -> Option<NodeId> {
        self.selected_mut().element_at(point)
    }

    fn hit_test(&mut self, point: PointF) -> dom_api::Hit {
        self.selected_mut().hit_test(point)
    }

    fn point_in_frame(&mut self, document: NodeId, point: PointF) -> Option<PointF> {
        self.view.point_in_frame(document, point)
    }

    fn point_from_frame(&mut self, document: NodeId, point: PointF) -> Option<PointF> {
        self.view.point_from_frame(document, point)
    }

    fn svg_geometry<'gc>(&self, cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> SvgGeometry {
        self.selected()
            .svg_geometry(layout::svg_element(cx, &self.token, element))
    }

    /// One not styled (not in the document) is no shape: a `<path>` still
    /// has its data, as Blink's `SVGPathElement` reads it (relative
    /// commands made absolute, what comes before an error).
    fn svg_shape_path<'gc>(&self, cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> Option<gfx::Path> {
        if let Some(path) =
            self.selected()
                .svg_shape_path(layout::svg_element(cx, &self.token, element))
        {
            return Some(path);
        }
        if cx.computed_style(element, &self.token).is_some()
            || cx.qualified_name(element) != Some(dom::QualifiedName::svg(dom::names::local::PATH))
        {
            return None;
        }
        let data = cx
            .attribute_value_by_id(element, dom::names::local::D)
            .map(|value| value.borrow().to_string())
            .unwrap_or_default();
        Some(css::path_from_string(&data))
    }

    fn svg_point_in_shape<'gc>(
        &self,
        cx: &Cx<'_, 'gc>,
        element: NodeRef<'gc>,
        point: PointF,
        stroke: bool,
    ) -> bool {
        self.selected().svg_point_in_shape(
            layout::svg_element(cx, &self.token, element),
            point,
            stroke,
        )
    }

    fn svg_text_chars<'gc>(&self, cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> Vec<SvgTextChar> {
        self.selected()
            .svg_text_chars(layout::svg_element(cx, &self.token, element))
    }

    fn elements_at(&mut self, point: PointF) -> Vec<NodeId> {
        self.selected_mut().elements_at(point)
    }

    fn scroll_into_view_in(&mut self, container: NodeId, element: NodeId) {
        self.selected_mut()
            .scroll_into_view_in_after_layout(container, element);
    }

    fn scroll_to(&mut self, document: NodeId, scroller: NodeId, offset: PointF) {
        self.view.scroll_to(document, scroller, offset);
    }

    fn keyboard_scroll(&mut self, chain: &[NodeId], key: &str, shift: bool) -> bool {
        ScrollKey::from_dom_key(key).is_some_and(|key| self.view.keyboard_scroll(chain, key, shift))
    }

    fn scroll_into_view<'gc>(
        &mut self,
        cx: &Cx<'_, 'gc>,
        element: NodeRef<'gc>,
        params: gfx::ScrollIntoViewParams,
    ) {
        let document_of = |node: NodeId| {
            cx.node_by_id(node)
                .map(|node| cx.node_id(cx.document_of(node)))
        };
        self.view
            .scroll_into_view(cx.node_id(element), &document_of, params);
    }

    fn scroll_caret_into_view<'gc>(
        &mut self,
        cx: &Cx<'_, 'gc>,
        document: NodeRef<'gc>,
        params: gfx::ScrollIntoViewParams,
    ) -> bool {
        let Some(view) = self.view.view_of(cx.node_id(document)) else {
            return false;
        };
        let (Some(box_tree), Some(fragment)) = (view.box_tree.as_ref(), view.fragment.as_ref())
        else {
            return false;
        };
        let Some(caret) = focused_caret(cx, &self.token, document, box_tree, &mut self.text) else {
            return false;
        };
        let Some(rect) = caret_rect(fragment, caret) else {
            return false;
        };
        let document_of = |node: NodeId| {
            cx.node_by_id(node)
                .map(|node| cx.node_id(cx.document_of(node)))
        };
        self.view
            .scroll_rect_into_view(caret.block, Some(rect), &document_of, params);
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Not painted again: the same layers, the textures of `drawn` (canvases
/// that drew, `FrameView::canvases_drawn`) with their new snapshots.
fn swap_canvas_textures(
    drawn: Vec<NodeId>,
    compositor: &compositing::PaintArtifactCompositor,
    snapshots: &HashMap<NodeId, (gfx::ImageData, bool)>,
    output: &mut Option<Arc<PaintOutput>>,
) {
    if drawn.is_empty() {
        return;
    }
    let Some(last) = output.clone() else {
        return;
    };
    let textures: HashMap<compositor::LayerId, (gfx::ImageData, bool)> = drawn
        .into_iter()
        .filter_map(|canvas| {
            Some((
                compositor.canvas_layer(canvas)?,
                snapshots.get(&canvas)?.clone(),
            ))
        })
        .collect();
    let layers = last
        .layers
        .iter()
        .map(|layer| match layer {
            Layer::Texture(texture) => match textures.get(&texture.id) {
                Some((snapshot, opaque)) => Layer::Texture(compositor::TextureLayer {
                    snapshot: snapshot.clone(),
                    contents_opaque: *opaque,
                    ..texture.clone()
                }),
                None => layer.clone(),
            },
            Layer::Picture(_) => layer.clone(),
        })
        .collect();
    *output = Some(Arc::new(PaintOutput {
        layers,
        property_trees: last.property_trees.clone(),
    }));
}

/// What a paint gives the compositor: the document's layers (each with
/// what to raster again, keeping its display list when nothing) and the
/// property trees they are in, with where a wheel needs the main thread.
/// A new `Arc` at each paint, so the renderer hands it over only when it
/// is one.
pub(crate) struct PaintOutput {
    pub(crate) layers: Vec<Layer>,
    pub(crate) property_trees: compositor::PropertyTrees,
}

/// The keys of the sticky boxes the compositor moves, with the
/// containers that are layers `composited` (by scroll node).
pub(crate) fn composited_sticky_keys(
    trees: &paint::PropertyTrees,
    composited: &[bool],
) -> std::collections::HashSet<layout::StickyKey> {
    paint::composited_sticky(trees, composited)
        .into_iter()
        .zip(&trees.transform)
        .filter_map(|(moved, node)| moved.then_some(node.sticky?.key))
        .collect()
}

/// Whether an iframe's `scrolling` keeps the user from scrolling its child
/// frame (Blink's `HTMLFrameElementBase::ScrollbarMode`): script and
/// fragments still scroll it.
fn scrolling_off(scrolling: &str) -> bool {
    ["no", "noscroll", "off"]
        .iter()
        .any(|off| scrolling.eq_ignore_ascii_case(off))
}

/// The first fragment `node` generated, in `root`.
fn find_fragment(root: &BoxFragment, node: NodeId) -> Option<&BoxFragment> {
    if root.node == Some(node) {
        return Some(root);
    }
    root.children
        .iter()
        .find_map(|child| find_fragment(&child.fragment, node))
}

/// The size a child frame of iframe `owner` (none: no iframe) is laid out
/// at: the iframe's content box in `fragment`, its frame's layout; 0×0
/// where the iframe has no box (Blink still styles and lays out such a
/// frame of the same process).
fn child_viewport(
    fragment: Option<&BoxFragment>,
    owner: Option<(NodeId, dom_api::FrameId)>,
) -> SizeF {
    owner
        .zip(fragment)
        .and_then(|((owner, _), fragment)| find_fragment(fragment, owner))
        .map_or(SizeF::new(0.0, 0.0), content_box_size)
}

/// A box's content box size: its border box less its borders and padding.
fn content_box_size(fragment: &BoxFragment) -> SizeF {
    let (b, p) = (&fragment.border, &fragment.padding);
    SizeF::new(
        (fragment.size.width - b.left - b.right - p.left - p.right).max(0.0),
        (fragment.size.height - b.top - b.bottom - p.top - p.bottom).max(0.0),
    )
}

/// Where `caret` is drawn in its block, `fragment`'s or one in it: from
/// the block's content box (on its lines, or those of an anonymous block
/// in it, as paint finds it); `None` if it is on none.
fn caret_rect(fragment: &BoxFragment, caret: paint::Caret) -> Option<RectF> {
    if fragment.node == Some(caret.block) {
        let rect = in_anonymous_blocks(fragment, caret.at)?;
        let (b, p) = (&fragment.border, &fragment.padding);
        return Some(rect.offset(-(b.left + p.left), -(b.top + p.top)));
    }
    fragment
        .children
        .iter()
        .find_map(|child| caret_rect(&child.fragment, caret))
}

/// The caret at `at` on `fragment`'s lines or an anonymous block's in it,
/// from `fragment`'s border box.
fn in_anonymous_blocks(fragment: &BoxFragment, at: paint::CaretPosition) -> Option<RectF> {
    paint::caret_rect(fragment, at).or_else(|| {
        fragment
            .children
            .iter()
            .filter(|child| child.fragment.role == layout::BoxRole::AnonymousBlock)
            .find_map(|child| {
                in_anonymous_blocks(&child.fragment, at)
                    .map(|rect| rect.offset(child.offset.x, child.offset.y))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Page {
        token: LocalToken,
        text: TextContext,
        view: FrameView,
        svg: SvgImages,
        /// Last: fields drop in order, and the view's root goes before
        /// its heap.
        dom: Dom,
    }

    impl Page {
        fn new(html: &str) -> Self {
            Self::with_system_colors(html, gfx::SystemColors::DEFAULT)
        }

        fn with_system_colors(html: &str, system_colors: gfx::SystemColors) -> Self {
            let mut dom = Dom::new();
            let document = html::parser::parse_document(&mut dom, html, true)
                .root()
                .clone();
            let node = dom.read(|cx| cx.node_id(document.get(cx.gc)));
            Self {
                token: LocalToken::new(),
                text: TextContext::new(true),
                view: FrameView::new(document, node, DocumentId::default(), system_colors),
                svg: SvgImages::default(),
                dom,
            }
        }

        fn update(&mut self, viewport: SizeF, scale: f32) -> Arc<PaintOutput> {
            let Self {
                dom,
                token,
                text,
                view,
                svg,
            } = self;
            view.update_all_lifecycle_phases(dom, token, text, svg, viewport, scale)
        }

        /// The width of the `<body>`'s child `child`.
        fn width(&self, child: usize) -> f32 {
            // The `LayoutView`'s, `<html>`'s, `<body>`'s.
            let fragment = self.view.fragment.as_ref().expect("laid out");
            let body = &fragment.children[0].fragment.children[0].fragment;
            body.children[child].fragment.size.width
        }
    }

    const SIZE: SizeF = SizeF::new(200.0, 100.0);

    /// How many backgrounds the last paint filled with an image.
    fn images_drawn(page: &Page) -> usize {
        use compositor_paint::{PaintOp, PaintShader, ShaderImage};
        let artifact = page.view.paint_controller.last_artifact().expect("painted");
        artifact
            .items
            .iter()
            .filter(|item| match &item.op {
                PaintOp::DrawRect { flags, .. } | PaintOp::DrawRRect { flags, .. } => matches!(
                    flags.shader.as_deref(),
                    Some(PaintShader::Tiled(tiled)) if matches!(tiled.image, ShaderImage::Image(_))
                ),
                _ => false,
            })
            .count()
    }

    /// The platform's system colors the view was made with: CSS's are
    /// resolved with them, and the selection (in a document with no
    /// window: not focused) is drawn in their inactive ones.
    #[test]
    fn system_colors_are_the_platforms() {
        let colors = gfx::SystemColors {
            canvas: gfx::Color::rgb(1, 2, 3),
            inactive_highlight: gfx::Color::rgb(4, 5, 6),
            ..gfx::SystemColors::DEFAULT
        };
        let mut page = Page::with_system_colors(
            "<!doctype html><body style='margin: 0'>\
             <div style='width: 20px; height: 10px; background: Canvas'></div><p>text</p>",
            colors,
        );
        let document = page.view.document.clone();
        page.dom.mutate(|cx| {
            let document = document.get(cx.gc);
            let body = cx.last_child(cx.last_child(document).unwrap()).unwrap();
            let text = cx.first_child(cx.last_child(body).unwrap()).unwrap();
            let range = cx.create_range((text, 0), (text, 4));
            cx.set_selection(cx.document_selection(document), Some(range), false);
        });
        page.update(SIZE, 1.0);
        let artifact = page.view.paint_controller.last_artifact().expect("painted");
        let filled: Vec<gfx::Color> = artifact
            .items
            .iter()
            .filter_map(|item| match &item.op {
                compositor_paint::PaintOp::DrawRect { flags, .. } => Some(flags.color),
                _ => None,
            })
            .collect();
        assert!(filled.contains(&colors.canvas), "{filled:?}");
        assert!(filled.contains(&colors.inactive_highlight), "{filled:?}");
    }

    /// A background's image: asked of the DOM's list of available images
    /// as style finds it (fetched once), drawn once it came and the view
    /// was told (its element painted again); one the list has already is
    /// drawn at once.
    #[test]
    fn a_backgrounds_image_is_drawn_once_it_comes() {
        let mut page = Page::new(
            "<!doctype html><style>.b { width: 20px; height: 10px; background-image: url(a.png) }</style>\
             <body style='margin: 0'><div class=b></div>",
        );
        let document = page.view.document.clone();
        page.dom.mutate(|cx| {
            cx.set_document_url(document.get(cx.gc), "https://example.test/");
        });
        let first = page.update(SIZE, 1.0);
        assert_eq!(
            page.dom
                .take_image_fetches()
                .into_iter()
                .map(|fetch| fetch.url)
                .collect::<Vec<_>>(),
            ["https://example.test/a.png"]
        );
        assert_eq!(images_drawn(&page), 0);
        let image = dom::Image::Raster(
            gfx::ImageData::new(vec![0u8; 4]),
            gfx::Size {
                width: 2,
                height: 1,
            },
        );
        page.dom
            .mutate(|cx| cx.image_fetched("https://example.test/a.png", Some(image), true));
        assert!(Arc::ptr_eq(&first, &page.update(SIZE, 1.0)), "not told yet");
        page.view.images_changed();
        let drawn = page.update(SIZE, 1.0);
        assert!(!Arc::ptr_eq(&first, &drawn));
        assert_eq!(images_drawn(&page), 1);
        // Another element of the same image, by its `style` attribute
        // (its `url()` against the document's URL too): drawn at once, no
        // fetch.
        page.dom.mutate(|cx| {
            let body = cx
                .last_child(cx.last_child(document.get(cx.gc)).unwrap())
                .unwrap();
            let other = cx.create_element(document.get(cx.gc), dom::QualName::html("div"));
            cx.set_attribute(
                other,
                "style",
                "width: 20px; height: 10px; background-image: url(a.png)",
            );
            cx.append_child(body, other).unwrap();
        });
        page.update(SIZE, 1.0);
        assert_eq!(images_drawn(&page), 2);
        assert!(page.dom.take_image_fetches().is_empty());
    }

    /// How many backgrounds the last paint filled with an SVG image's paint.
    fn svg_images_drawn(page: &Page) -> usize {
        use compositor_paint::{PaintOp, PaintShader, ShaderImage};
        let artifact = page.view.paint_controller.last_artifact().expect("painted");
        artifact
            .items
            .iter()
            .filter(|item| match &item.op {
                PaintOp::DrawRect { flags, .. } | PaintOp::DrawRRect { flags, .. } => matches!(
                    flags.shader.as_deref(),
                    Some(PaintShader::Tiled(tiled)) if matches!(tiled.image, ShaderImage::Record { .. })
                ),
                _ => false,
            })
            .count()
    }

    /// An SVG image, made of a response in the page's DOM (a document of
    /// its own, styled), drawn as a background once it came: painted for
    /// its tile's size when paint first asks, then drawn; another tile
    /// size painted again. A response whose root isn't an `<svg>` is no
    /// image.
    #[test]
    fn an_svg_background_is_painted_for_its_tile() {
        let mut page = Page::new(
            "<!doctype html><style>.b { width: 20px; height: 10px; background-image: url(a.svg) }</style>\
             <body style='margin: 0'><div class=b></div>",
        );
        let document = page.view.document.clone();
        page.dom.mutate(|cx| {
            cx.set_document_url(document.get(cx.gc), "https://example.test/");
        });
        page.update(SIZE, 1.0);
        assert_eq!(
            page.dom
                .take_image_fetches()
                .into_iter()
                .map(|fetch| fetch.url)
                .collect::<Vec<_>>(),
            ["https://example.test/a.svg"]
        );
        assert!(
            SvgImage::new(&mut page.dom, &mut page.token, None, b"<html/>").is_none(),
            "not an <svg>"
        );
        let image = SvgImage::new(
            &mut page.dom,
            &mut page.token,
            None,
            b"<svg xmlns='http://www.w3.org/2000/svg' width='4' height='2'><rect width='4' height='2' fill='lime'/></svg>",
        )
        .expect("an image");
        assert_eq!(
            image.natural(),
            layout::SvgNaturalSize {
                width: Some(4.0),
                height: Some(2.0),
                ratio: None
            }
        );
        let natural = image.natural();
        let id = page.svg.add(image);
        page.dom.mutate(|cx| {
            cx.image_fetched(
                "https://example.test/a.svg",
                Some(dom::Image::Svg(id, natural)),
                true,
            )
        });
        page.view.images_changed();
        page.update(SIZE, 1.0);
        assert_eq!(svg_images_drawn(&page), 1, "painted, then drawn");
        assert!(
            page.svg.images[&id]
                .record(SizeF::new(4.0, 2.0), 1.0)
                .is_some()
        );
        assert!(
            page.svg.images[&id]
                .record(SizeF::new(4.0, 2.0), 2.0)
                .is_none()
        );
        page.update(SIZE, 2.0);
        assert_eq!(svg_images_drawn(&page), 1);
        assert!(
            page.svg.images[&id]
                .record(SizeF::new(4.0, 2.0), 2.0)
                .is_some(),
            "at the new scale"
        );
    }

    /// An SVG image's `<use>`s draw their copies: its one style update
    /// has them made first.
    #[test]
    fn an_svg_images_uses_are_drawn() {
        let mut page = Page::new(
            "<!doctype html><style>.b { width: 4px; height: 2px; background-image: url(a.svg) }</style>\
             <body style='margin: 0'><div class=b></div>",
        );
        let document = page.view.document.clone();
        page.dom.mutate(|cx| {
            cx.set_document_url(document.get(cx.gc), "https://example.test/");
        });
        page.update(SIZE, 1.0);
        drop(page.dom.take_image_fetches());
        let image = SvgImage::new(
            &mut page.dom,
            &mut page.token,
            None,
            b"<svg xmlns='http://www.w3.org/2000/svg' width='4' height='2'>\
              <symbol id='s'><rect width='4' height='2' fill='lime'/></symbol><use href='#s'/></svg>",
        )
        .expect("an image");
        let natural = image.natural();
        let id = page.svg.add(image);
        page.dom.mutate(|cx| {
            cx.image_fetched(
                "https://example.test/a.svg",
                Some(dom::Image::Svg(id, natural)),
                true,
            )
        });
        page.view.images_changed();
        page.update(SIZE, 1.0);
        let (list, _) = page.svg.images[&id]
            .record(SizeF::new(4.0, 2.0), 1.0)
            .expect("painted");
        assert!(
            list.ops().iter().any(|op| matches!(
                op,
                compositor_paint::PaintOp::DrawPath { flags, .. }
                    if flags.color == gfx::Color::rgba(0, 255, 0, 255)
            )),
            "the copy's rect"
        );
    }

    /// Nothing changed, nothing done: the same layers; a change to the
    /// DOM, the viewport (which `vw` sees) or the device scale factor
    /// paints again.
    #[test]
    fn what_changed_is_painted_again() {
        let mut page = Page::new(
            "<!doctype html><body style='margin: 0'><div style='width: 50vw'>a</div><p>b</p>",
        );
        let first = page.update(SIZE, 1.0);
        assert_eq!(page.width(0), 100.0, "the viewport's width in vw");
        assert_eq!(page.width(1), 200.0, "the viewport's width");
        assert!(
            Arc::ptr_eq(&first, &page.update(SIZE, 1.0)),
            "nothing changed"
        );
        let wider = page.update(SizeF::new(400.0, 100.0), 1.0);
        assert!(!Arc::ptr_eq(&first, &wider));
        assert_eq!(page.width(0), 200.0);
        assert_eq!(page.width(1), 400.0, "laid out at the new width");
        let scaled = page.update(SizeF::new(400.0, 100.0), 2.0);
        assert!(!Arc::ptr_eq(&wider, &scaled), "another scale");
        let document = page.view.document.clone();
        page.dom.mutate(|cx| {
            let body = cx
                .last_child(cx.last_child(document.get(cx.gc)).unwrap())
                .unwrap();
            let div = cx.first_child(body).unwrap();
            cx.set_attribute(div, "style", "width: 30px");
        });
        let changed = page.update(SizeF::new(400.0, 100.0), 2.0);
        assert!(!Arc::ptr_eq(&scaled, &changed), "the DOM changed");
        assert_eq!(page.width(0), 30.0);
    }

    /// A change that only takes content out (it builds and shapes
    /// nothing): laid out again all the same.
    #[test]
    fn content_taken_out_lays_out_again() {
        let mut page = Page::new(
            "<!doctype html><body style='margin: 0'><div style='font: 20px/1 Ahem'>a</div>",
        );
        page.update(SIZE, 1.0);
        let height = |page: &Page| {
            let fragment = page.view.fragment.as_ref().expect("laid out");
            fragment.children[0].fragment.children[0].fragment.children[0]
                .fragment
                .size
                .height
        };
        assert_eq!(height(&page), 20.0);
        let document = page.view.document.clone();
        page.dom.mutate(|cx| {
            let body = cx
                .last_child(cx.last_child(document.get(cx.gc)).unwrap())
                .unwrap();
            let div = cx.first_child(body).unwrap();
            cx.remove(cx.first_child(div).unwrap());
        });
        page.update(SIZE, 1.0);
        assert_eq!(height(&page), 0.0);
    }

    /// A blocking `wheel` listener's boxes make the compositor's wheel
    /// event handler region (a paint of the style it changes), a passive
    /// one's none; the root's, the document's or the window's make the
    /// whole document; a listener that styles nothing (a plain target's)
    /// still tells the compositor there may be listeners, the same layers
    /// kept.
    #[test]
    fn blocking_wheel_listeners_make_the_region() {
        use dom::names::event;
        use dom::{ListenerFlags, TargetRef};
        let mut page = Page::new(
            "<!doctype html><body style='margin: 0'><div style='height: 30px; width: 50px'></div><p style='margin: 0; height: 20px'>p</p>",
        );
        let first = page.update(SIZE, 1.0);
        assert!(first.property_trees.wheel_event_handler_region.is_empty());
        assert!(!first.property_trees.wheel_event_listeners);
        let document = page.view.document.clone();
        let plain = page.dom.mutate(|cx| {
            let plain = cx.create_event_target();
            cx.add_listener(
                TargetRef::EventTarget(plain),
                event::WHEEL,
                ListenerFlags::default(),
            );
            cx.root_target(TargetRef::EventTarget(plain))
        });
        let told = page.update(SIZE, 1.0);
        assert!(told.property_trees.wheel_event_listeners);
        assert!(told.property_trees.wheel_event_handler_region.is_empty());
        assert_eq!(told.layers.len(), first.layers.len());
        let in_region = |output: &PaintOutput, x: f32, y: f32| {
            output
                .property_trees
                .wheel_event_handler_region
                .iter()
                .any(|rect| rect.contains(PointF::new(x, y)))
        };
        let (div, p) = page.dom.mutate(|cx| {
            let body = cx
                .last_child(cx.last_child(document.get(cx.gc)).unwrap())
                .unwrap();
            let div = cx.first_child(body).unwrap();
            let p = cx.next_sibling(div).unwrap();
            cx.add_listener(TargetRef::Node(div), event::WHEEL, ListenerFlags::default());
            let passive = ListenerFlags {
                passive: true,
                ..ListenerFlags::default()
            };
            cx.add_listener(TargetRef::Node(p), event::WHEEL, passive);
            (cx.root(div), cx.root(p))
        });
        let blocking = page.update(SIZE, 1.0);
        assert!(in_region(&blocking, 10.0, 10.0), "the div's box");
        assert!(!in_region(&blocking, 10.0, 40.0), "the passive one's");
        assert!(!in_region(&blocking, 100.0, 10.0), "beside the div");
        page.dom.mutate(|cx| {
            cx.add_listener(
                TargetRef::Node(document.get(cx.gc)),
                event::WHEEL,
                ListenerFlags::default(),
            );
        });
        let whole = page.update(SIZE, 1.0);
        assert!(in_region(&whole, 150.0, 90.0), "anywhere");
        drop((div, p, plain));
    }

    /// A wheel the main thread scrolls: the container under it that can
    /// move takes it (painted again where it scrolled to, if it isn't a
    /// layer), then the viewport; nothing to scroll, nothing moved.
    #[test]
    fn a_wheel_scrolls_on_the_main_thread() {
        let mut page = Page::new(
            "<!doctype html><body style='margin: 0'>\
             <div style='overflow: auto; height: 50px; width: 100px'><div style='height: 200px'></div></div>",
        );
        page.update(SIZE, 1.0);
        assert!(
            page.view
                .wheel(PointF::new(10.0, 10.0), PointF::new(0.0, 30.0))
        );
        let offsets: Vec<f32> = page
            .view
            .scroll
            .containers
            .values()
            .map(|offset| offset.y)
            .collect();
        assert_eq!(offsets, [30.0]);
        // The document is shorter than the viewport: nothing to scroll.
        assert!(
            !page
                .view
                .wheel(PointF::new(150.0, 80.0), PointF::new(0.0, 30.0))
        );
        // One token a thread: the first page goes first.
        drop(page);
        let mut tall =
            Page::new("<!doctype html><body style='margin: 0'><div style='height: 300px'></div>");
        tall.update(SIZE, 1.0);
        assert!(
            tall.view
                .wheel(PointF::new(10.0, 10.0), PointF::new(0.0, 40.0))
        );
        assert_eq!(tall.view.viewport_scroll_offset(), PointF::new(0.0, 40.0));
    }

    /// A child frame below a tall page, `src` its document's markup: the
    /// page, the child frame's document's id. The iframe is at (10, 20), its
    /// content box 100×50.
    fn page_with_child(child: &str) -> (Page, NodeId) {
        page_with_child_of("", child)
    }

    /// `page_with_child`, the iframe with the attributes `attributes`.
    fn page_with_child_of(attributes: &str, child: &str) -> (Page, NodeId) {
        let mut page = Page::new(&format!(
            "<!doctype html><body style='margin: 0'>\
             <iframe {attributes} style='position: absolute; left: 10px; top: 20px; width: 100px; height: 50px; border: 0'></iframe>\
             <div style='height: 300px'></div>"
        ));
        let top = page.view.document.clone();
        let iframe = page.dom.read(|cx| {
            let body = cx
                .last_child(cx.last_child(top.get(cx.gc)).unwrap())
                .unwrap();
            cx.node_id(cx.first_child(body).unwrap())
        });
        let document = html::parser::parse_document(&mut page.dom, child, true)
            .root()
            .clone();
        let node = page.dom.read(|cx| cx.node_id(document.get(cx.gc)));
        page.view.show_child(
            None,
            FrameView::new(
                document,
                node,
                DocumentId::default(),
                gfx::SystemColors::DEFAULT,
            )
            .into_child(dom_api::FrameId(1), iframe),
        );
        page.update(SIZE, 1.0);
        (page, node)
    }

    /// The sticky boxes of the frame tree, in the tree's one pre-paint:
    /// a child frame's and its own child frame's stick (each 10 px high at
    /// 100 px, `bottom: 0`, in a viewport 50 and 40 px high) and may move
    /// as their viewports scroll.
    #[test]
    fn child_frames_sticky_boxes_are_in_the_trees_pre_paint() {
        let sticky = "<div style='height: 100px'></div>\
                      <div style='position: sticky; bottom: 0; height: 10px'></div>";
        let (mut page, _) = page_with_child(&format!(
            "<!doctype html><body style='margin: 0'>\
             <iframe style='position: absolute; left: 0; top: 0; width: 50px; height: 40px; border: 0'></iframe>{sticky}"
        ));
        let iframe = page.dom.read(|cx| {
            let child = page.view.children[0].document.get(cx.gc);
            let body = cx.last_child(cx.last_child(child).unwrap()).unwrap();
            cx.node_id(cx.first_child(body).unwrap())
        });
        let document = html::parser::parse_document(
            &mut page.dom,
            &format!("<!doctype html><body style='margin: 0'>{sticky}"),
            true,
        )
        .root()
        .clone();
        let node = page.dom.read(|cx| cx.node_id(document.get(cx.gc)));
        page.view.show_child(
            Some(dom_api::FrameId(1)),
            FrameView::new(
                document,
                node,
                DocumentId::default(),
                gfx::SystemColors::DEFAULT,
            )
            .into_child(dom_api::FrameId(2), iframe),
        );
        page.update(SIZE, 1.0);
        let (_, pre) = page.view.pre.as_ref().expect("pre-painted");
        let mut stuck: Vec<(f32, bool)> = pre
            .trees
            .transform
            .iter()
            .filter_map(|node| {
                let sticky = node.sticky?;
                let at = node.matrix.as_translation().unwrap_or_default();
                Some((at.y, sticky.reach != RectF::default()))
            })
            .collect();
        stuck.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(stuck, [(-70.0, true), (-60.0, true)]);
    }

    /// A sticky box in a child frame the compositor can't move (under an
    /// opacity): as the compositor scrolls the child frame's viewport (a
    /// layer), nothing is painted until the box sticks elsewhere, then it
    /// is painted again where it sticks.
    #[test]
    fn a_child_frames_sticky_box_is_painted_again_where_it_sticks() {
        let (mut page, child) = page_with_child(
            "<!doctype html><body style='margin: 0'><div style='opacity: 0.5'>\
             <div style='height: 20px'></div>\
             <div style='position: sticky; top: 0; height: 10px; background: green'></div>\
             <div style='height: 300px'></div></div>",
        );
        let first = page.update(SIZE, 1.0);
        let scroll = |page: &mut Page, by: f32| {
            page.view.apply_compositor_scroll(&CompositorScrollDelta {
                document: page.view.document_id(),
                viewport: PointF::new(0.0, 0.0),
                containers: vec![(compositing::element_id(child), PointF::new(0.0, by))],
            });
        };
        // 4 px: the box, at 20 px, doesn't stick yet.
        scroll(&mut page, 4.0);
        let second = page.update(SIZE, 1.0);
        assert!(Arc::ptr_eq(&first, &second), "nothing painted");
        scroll(&mut page, 36.0);
        let second = page.update(SIZE, 1.0);
        assert!(!Arc::ptr_eq(&first, &second), "painted again");
        let (_, pre) = page.view.pre.as_ref().expect("pre-painted");
        let at = pre
            .trees
            .transform
            .iter()
            .find(|node| node.sticky.is_some())
            .and_then(|node| node.matrix.as_translation());
        assert_eq!(at, Some(PointF::new(0.0, 20.0)));
    }

    /// A wheel over a child frame scrolls it first, then, at its end, the
    /// page; the compositor's delta of its viewport is its. Points map
    /// between the top-level frame's viewport and the child frame's, its
    /// scrolling too, and a child frame's hit test goes through the page's.
    #[test]
    fn wheels_and_points_go_into_child_frames() {
        let (mut page, child) = page_with_child(
            "<!doctype html><body style='margin: 0'><div style='height: 100px'></div>",
        );
        let at = PointF::new(20.0, 30.0);
        assert_eq!(
            page.view.point_in_frame(child, at),
            Some(PointF::new(10.0, 10.0))
        );
        assert_eq!(
            page.view.point_from_frame(child, PointF::new(10.0, 10.0)),
            Some(at)
        );
        assert!(page.view.wheel(at, PointF::new(0.0, 20.0)));
        let child_scroll = |page: &Page| page.view.view_of(child).unwrap().scroll.viewport;
        assert_eq!(child_scroll(&page), PointF::new(0.0, 20.0));
        assert_eq!(
            page.view.viewport_scroll_offset(),
            PointF::new(0.0, 0.0),
            "the child's"
        );
        assert_eq!(
            page.view.point_in_frame(child, at),
            Some(PointF::new(10.0, 10.0)),
            "its viewport's"
        );
        // Past the child frame's end (it scrolls 50 px): the page takes the
        // rest.
        page.update(SIZE, 1.0);
        assert!(page.view.wheel(at, PointF::new(0.0, 60.0)));
        assert_eq!(child_scroll(&page), PointF::new(0.0, 50.0));
        assert_eq!(
            page.view.viewport_scroll_offset(),
            PointF::new(0.0, 30.0),
            "the rest the page's"
        );
        // The page scrolled 30 px: the iframe is 30 px higher.
        assert_eq!(
            page.view.point_in_frame(child, at),
            Some(PointF::new(10.0, 40.0))
        );
        assert_eq!(
            page.view.point_from_frame(child, PointF::new(10.0, 40.0)),
            Some(at)
        );
        // The compositor scrolled the child frame's viewport back.
        let back = CompositorScrollDelta {
            document: page.view.document_id(),
            viewport: PointF::new(0.0, 0.0),
            containers: vec![(compositing::element_id(child), PointF::new(0.0, -5.0))],
        };
        let before = child_scroll(&page).y;
        page.view.apply_compositor_scroll(&back);
        assert_eq!(child_scroll(&page).y, before - 5.0);
    }

    /// Another viewport lays out again, though no style changed (no
    /// viewport unit, no media query).
    #[test]
    fn another_viewport_lays_out_again() {
        let mut page = Page::new("<!doctype html><body style='margin: 0'><p>a</p>");
        page.update(SIZE, 1.0);
        assert_eq!(page.width(0), 200.0);
        page.update(SizeF::new(300.0, 100.0), 1.0);
        assert_eq!(page.width(0), 300.0);
    }

    /// A child frame whose rendering a sheet blocks is neither styled nor
    /// laid out, though the page is, its viewport (what its media queries
    /// see) its iframe's all the same; once nothing blocks it, it is, at
    /// that size; blocked again, it keeps its last layout, and takes the
    /// iframe's new size when it goes on.
    #[test]
    fn a_child_frame_whose_rendering_is_blocked_waits() {
        use dom_api::FrameId;
        let mut page = Page::new(
            "<!doctype html><body style='margin: 0'>\
             <iframe style='width: 120px; height: 60px; border: 0'></iframe>",
        );
        let top = page.view.document.clone();
        let iframe = page.dom.read(|cx| {
            let body = cx
                .last_child(cx.last_child(top.get(cx.gc)).unwrap())
                .unwrap();
            cx.node_id(cx.first_child(body).unwrap())
        });
        let document = html::parser::parse_document(
            &mut page.dom,
            "<!doctype html><body style='margin: 0'><div>a</div>",
            true,
        )
        .root()
        .clone();
        let node = page.dom.read(|cx| cx.node_id(document.get(cx.gc)));
        let mut view = FrameView::new(
            document.clone(),
            node,
            DocumentId::default(),
            gfx::SystemColors::DEFAULT,
        )
        .into_child(FrameId(1), iframe);
        view.set_render_blocked(true);
        page.view.show_child(None, view);
        page.update(SIZE, 1.0);
        let child = page.view.view_of(node).unwrap();
        assert!(child.fragment.is_none(), "not laid out while blocked");
        assert_eq!(child.viewport, SizeF::new(120.0, 60.0), "its iframe's size");
        let div_styled = |page: &mut Page| {
            let token = &page.token;
            page.dom.read(|cx| {
                let body = cx
                    .last_child(cx.last_child(document.get(cx.gc)).unwrap())
                    .unwrap();
                cx.computed_style(cx.first_child(body).unwrap(), token)
                    .is_some()
            })
        };
        assert!(!div_styled(&mut page), "not styled while blocked");
        page.view
            .child_mut(FrameId(1))
            .unwrap()
            .set_render_blocked(false);
        page.update(SIZE, 1.0);
        assert!(div_styled(&mut page));
        let laid_out = page.view.view_of(node).unwrap().fragment.clone();
        assert!(laid_out.is_some(), "laid out once nothing blocks it");
        // Blocked again while its iframe grows: its last layout stays, at
        // the new size once it goes on.
        page.view
            .child_mut(FrameId(1))
            .unwrap()
            .set_render_blocked(true);
        page.dom.mutate(|cx| {
            let iframe = cx.node_by_id(iframe).unwrap();
            cx.set_attribute(iframe, "style", "width: 150px; height: 60px; border: 0");
        });
        page.update(SIZE, 1.0);
        let child = page.view.view_of(node).unwrap();
        assert_eq!(child.viewport, SizeF::new(150.0, 60.0));
        assert!(Arc::ptr_eq(
            child.fragment.as_ref().unwrap(),
            laid_out.as_ref().unwrap()
        ));
        page.view
            .child_mut(FrameId(1))
            .unwrap()
            .set_render_blocked(false);
        page.update(SIZE, 1.0);
        let child = page.view.view_of(node).unwrap();
        assert!(!Arc::ptr_eq(
            child.fragment.as_ref().unwrap(),
            laid_out.as_ref().unwrap()
        ));
    }

    /// A child frame's view: laid out at its iframe's content box (again
    /// as that changes), painted into its iframe, the page painted again as
    /// its document changes; one below it at its own iframe's, 0×0 without
    /// one; gone, not painted.
    #[test]
    fn a_child_frame_is_laid_out_at_its_iframes_content_box() {
        use dom_api::FrameId;
        let mut page = Page::new(
            "<!doctype html><body style='margin: 0'>\
             <iframe style='width: 120px; height: 60px; border: 5px solid; padding: 3px'></iframe>",
        );
        let top = page.view.document.clone();
        let iframe = page.dom.read(|cx| {
            let body = cx
                .last_child(cx.last_child(top.get(cx.gc)).unwrap())
                .unwrap();
            cx.node_id(cx.first_child(body).unwrap())
        });
        let child = |page: &mut Page, html: &str| {
            let document = html::parser::parse_document(&mut page.dom, html, true)
                .root()
                .clone();
            let node = page.dom.read(|cx| cx.node_id(document.get(cx.gc)));
            (
                FrameView::new(
                    document,
                    node,
                    DocumentId::default(),
                    gfx::SystemColors::DEFAULT,
                ),
                node,
            )
        };
        let (view, node) = child(
            &mut page,
            "<!doctype html><body style='margin: 0'><div>a</div>",
        );
        page.view
            .show_child(None, view.into_child(FrameId(1), iframe));
        let first = page.update(SIZE, 1.0);
        let body_width = |view: &FrameView| {
            let fragment = view.fragment.as_ref().expect("laid out");
            fragment.children[0].fragment.children[0]
                .fragment
                .size
                .width
        };
        assert_eq!(
            body_width(page.view.view_of(node).unwrap()),
            120.0,
            "the content box's width"
        );
        assert!(page.view.child_frames().contains_key(&iframe));
        assert!(
            Arc::ptr_eq(&first, &page.update(SIZE, 1.0)),
            "nothing changed"
        );
        // Its document changed: the page painted again.
        let document = page.view.view_of(node).unwrap().document.clone();
        page.dom.mutate(|cx| {
            let body = cx
                .last_child(cx.last_child(document.get(cx.gc)).unwrap())
                .unwrap();
            let div = cx.first_child(body).unwrap();
            cx.set_attribute(div, "style", "height: 30px");
        });
        let changed = page.update(SIZE, 1.0);
        assert!(
            !Arc::ptr_eq(&first, &changed),
            "the child's document changed"
        );
        // Scrolled, not laid out again: painted again all the same.
        let target = page.dom.mutate(|cx| {
            let body = cx
                .last_child(cx.last_child(document.get(cx.gc)).unwrap())
                .unwrap();
            let div = cx.first_child(body).unwrap();
            cx.set_attribute(div, "style", "height: 300px");
            let p = cx.create_element(cx.document_of(body), dom::QualName::html("p"));
            cx.append_child(body, p).unwrap();
            cx.append_child(p, cx.create_text(cx.document_of(body), "b"))
                .unwrap();
            cx.node_id(p)
        });
        let tall = page.update(SIZE, 1.0);
        page.view
            .view_of_mut(node)
            .unwrap()
            .scroll_into_view_after_layout(target);
        let scrolled = page.update(SIZE, 1.0);
        assert!(page.view.view_of(node).unwrap().scroll.viewport.y > 0.0);
        assert!(!Arc::ptr_eq(&tall, &scrolled), "painted at its offset");
        // Its iframe narrower: laid out again at its width.
        page.dom.mutate(|cx| {
            let body = cx
                .last_child(cx.last_child(top.get(cx.gc)).unwrap())
                .unwrap();
            let iframe = cx.first_child(body).unwrap();
            cx.set_attribute(iframe, "style", "width: 80px; height: 60px; border: 0");
        });
        page.update(SIZE, 1.0);
        assert_eq!(body_width(page.view.view_of(node).unwrap()), 80.0);
        // Ones below it (their iframes not boxes of their parents'): 0×0,
        // painted with it.
        let (inner, inner_node) =
            child(&mut page, "<!doctype html><body style='margin: 0'><p>b</p>");
        page.view
            .show_child(Some(FrameId(1)), inner.into_child(FrameId(2), node));
        let (deepest, deepest_node) = child(&mut page, "<!doctype html><p>c</p>");
        page.view
            .show_child(Some(FrameId(2)), deepest.into_child(FrameId(3), inner_node));
        page.update(SIZE, 1.0);
        assert_eq!(body_width(page.view.view_of(inner_node).unwrap()), 0.0);
        assert!(page.view.view_of(deepest_node).is_some());
        let frames = page.view.child_frames();
        assert!(frames.contains_key(&node) && frames.contains_key(&inner_node));
        assert!(page.view.remove_child(FrameId(3)), "one two below");
        assert!(page.view.view_of(deepest_node).is_none());
        // Gone: not painted, nor the one below it.
        let shown = page.update(SIZE, 1.0);
        assert!(page.view.remove_child(FrameId(1)));
        let removed = page.update(SIZE, 1.0);
        assert!(!Arc::ptr_eq(&shown, &removed));
        assert!(page.view.child_frames().is_empty());
        assert!(page.view.view_of(inner_node).is_none());
        assert!(!page.view.remove_child(FrameId(2)));
    }

    /// A canvas that is a layer and is drawn on takes its new snapshot in
    /// its layer without a paint (Blink's `TextureLayer` of a composited
    /// canvas); one that isn't (under a scale) is painted again; one that
    /// is gone is let go of.
    #[test]
    fn a_canvas_layer_takes_its_new_snapshot_without_a_paint() {
        use compositor_paint::{PaintFlags, PaintOp};
        use gfx::{Color, Transform};

        let mut page = Page::new(
            "<!doctype html><body style='margin: 0'>\
             <div style='transform: translate(1px, 0)'>\
             <canvas id=c width=2 height=1 style='display: block'></canvas></div>",
        );
        let document = page.view.document.clone();
        let canvas = page.dom.mutate(|cx| {
            let canvas = cx.element_by_id(document.get(cx.gc), "c").unwrap();
            cx.create_canvas_context(canvas, canvas::CanvasSettings::default());
            cx.node_id(canvas)
        });
        let mut raster = crate::canvas_raster::CanvasRaster::default();
        // Fills the bitmap with `color`, as a frame's flush of a `fillRect`
        // does: its new snapshot.
        let mut fill = |dom: &mut Dom, color| {
            let mut items = Vec::new();
            let snapshot = dom.mutate(|cx| {
                let context = cx.canvas_context(cx.node_by_id(canvas).unwrap()).unwrap();
                let settings = cx.canvas_context_settings(context);
                cx.with_canvas_data(context, |data| {
                    data.ops.push(PaintOp::SetMatrix {
                        transform: Transform::IDENTITY,
                    });
                    data.ops.push(PaintOp::DrawRect {
                        rect: RectF::new(0.0, 0.0, 2.0, 1.0),
                        flags: PaintFlags::fill(color),
                    });
                    data.matrix = Some(Transform::IDENTITY);
                    crate::canvas::flush_data(
                        data,
                        settings,
                        canvas::CanvasFlush::Read,
                        &mut raster,
                        &mut items,
                    );
                    data.snapshot(canvas::background(settings))
                        .map(|snapshot| (snapshot, false))
                })
            });
            raster.send(items, true);
            snapshot
        };
        let texture = |output: &PaintOutput| {
            output.layers.iter().find_map(|layer| match layer {
                Layer::Texture(texture) => Some(texture.snapshot.clone()),
                Layer::Picture(_) => None,
            })
        };
        let pictures = |output: &PaintOutput| {
            output
                .layers
                .iter()
                .filter_map(|layer| layer.as_picture())
                .map(|picture| picture.display_list.clone())
                .collect::<Vec<_>>()
        };
        let red = fill(&mut page.dom, Color::rgb(255, 0, 0));
        page.view.canvases_drawn(vec![(canvas, red)]);
        let first = page.update(SIZE, 1.0);
        let red = texture(&first).expect("the canvas is a layer");

        let blue = fill(&mut page.dom, Color::rgb(0, 0, 255));
        page.view.canvases_drawn(vec![(canvas, blue)]);
        assert!(!page.view.paint_dirty);
        let second = page.update(SIZE, 1.0);
        let blue = texture(&second).expect("still a layer");
        assert_ne!(blue.id, red.id);
        assert!(blue.wait_ready());
        assert_eq!(&blue.as_bytes()[..4], [0, 0, 255, 255]);
        // Nothing painted again: the same display lists.
        let (before, after) = (pictures(&first), pictures(&second));
        assert_eq!(before.len(), after.len());
        assert!(before.iter().zip(&after).all(|(a, b)| Arc::ptr_eq(a, b)));

        // Under a scale, painted into the page: painted again, with the
        // snapshot.
        page.dom.mutate(|cx| {
            let wrapper = cx.parent(cx.node_by_id(canvas).unwrap()).unwrap();
            cx.set_attribute(wrapper, "style", "transform: scale(2)");
        });
        let third = page.update(SIZE, 1.0);
        assert!(texture(&third).is_none(), "under a scale, no layer");
        let green = fill(&mut page.dom, Color::rgb(0, 128, 0));
        page.view.canvases_drawn(vec![(canvas, green)]);
        assert!(page.view.paint_dirty, "painted again");
        let fourth = page.update(SIZE, 1.0);
        let drawn: Vec<u64> = pictures(&fourth)
            .iter()
            .flat_map(|list| list.ops().to_vec())
            .filter_map(|op| match op {
                PaintOp::DrawImage { image, .. } => Some(image.id),
                _ => None,
            })
            .collect();
        assert_eq!(drawn, [page.view.canvases[&canvas].0.id]);

        page.view.forget_canvases(|id| id == canvas);
        assert!(page.view.canvases.is_empty());
    }

    /// The id of the element `id` names in `document`, then its
    /// ancestors' and the document's: a key's chain from it.
    fn chain(page: &Page, document: &NodeRoot, id: &str) -> Vec<NodeId> {
        page.dom.read(|cx| {
            let document = document.get(cx.gc);
            let element = cx.element_by_id(document, id).expect("there");
            std::iter::successors(Some(element), |&node| cx.parent(node))
                .map(|node| cx.node_id(node))
                .collect()
        })
    }

    /// A key scrolls one box: the first around the chain that can move
    /// that way (by a line: 40 px for an arrow), not split as a wheel's
    /// delta is; at its end, the viewport; another key's step is its own
    /// (End: to the end). Nothing to scroll: nothing moved.
    #[test]
    fn a_key_scrolls_the_first_box_around_the_chain_that_moves() {
        let mut page = Page::new(
            "<!doctype html><body style='margin: 0'>\
             <div id=box style='overflow: auto; height: 50px'><div id=in style='height: 70px'></div></div>\
             <div style='height: 300px'></div>",
        );
        page.update(SIZE, 1.0);
        let document = page.view.document.clone();
        let chain = chain(&page, &document, "in");
        let container = chain[1];
        let offset = |page: &Page| page.view.scroll.containers.get(&container).copied();
        assert!(
            page.view
                .keyboard_scroll(&chain, ScrollKey::ArrowDown, false)
        );
        assert_eq!(
            offset(&page),
            Some(PointF::new(0.0, 20.0)),
            "as far as it can"
        );
        assert_eq!(page.view.viewport_scroll_offset(), PointF::new(0.0, 0.0));
        assert!(page.view.take_scrolled_by_script(), "a frame commits it");
        // At its end: the viewport.
        assert!(
            page.view
                .keyboard_scroll(&chain, ScrollKey::ArrowDown, false)
        );
        assert_eq!(page.view.viewport_scroll_offset(), PointF::new(0.0, 40.0));
        assert!(page.view.keyboard_scroll(&chain, ScrollKey::End, false));
        assert_eq!(
            page.view.viewport_scroll_offset(),
            PointF::new(0.0, 250.0),
            "350 high in 100"
        );
        assert!(!page.view.keyboard_scroll(&chain, ScrollKey::End, false));
        // Shift+Space goes up a page: the container can.
        assert!(page.view.keyboard_scroll(&chain, ScrollKey::Space, true));
        assert_eq!(offset(&page), Some(PointF::new(0.0, 0.0)));
        assert_eq!(page.view.viewport_scroll_offset(), PointF::new(0.0, 250.0));
    }

    /// A key in a child frame scrolls its viewport, then (at its end) the
    /// page's, through its iframe; `scrolling=no` keeps the user from
    /// scrolling a child frame, by keys and by wheels (either way: the
    /// scroll tree's node for its viewport isn't user-scrollable on either
    /// axis): the page scrolls.
    #[test]
    fn keys_and_scrolling_no_in_child_frames() {
        let child = "<!doctype html><body style='margin: 0'><div id=x style='height: 70px'></div>";
        let (mut page, child_document) = page_with_child(child);
        // The child frame's document, its iframe and the iframe's
        // ancestors.
        let iframe_chain = |page: &Page, child_document: NodeId| {
            let mut chain = vec![child_document];
            chain.extend(page.dom.read(|cx| {
                let top = page.view.document.get(cx.gc);
                let body = cx.last_child(cx.last_child(top).unwrap()).unwrap();
                let iframe = cx.first_child(body).unwrap();
                std::iter::successors(Some(iframe), |&node| cx.parent(node))
                    .map(|node| cx.node_id(node))
                    .collect::<Vec<_>>()
            }));
            chain
        };
        let chain = iframe_chain(&page, child_document);
        let child_scroll = |page: &Page| page.view.view_of(child_document).unwrap().scroll.viewport;
        assert!(
            page.view
                .keyboard_scroll(&chain, ScrollKey::ArrowDown, false)
        );
        assert_eq!(child_scroll(&page), PointF::new(0.0, 20.0));
        assert_eq!(page.view.viewport_scroll_offset(), PointF::new(0.0, 0.0));
        page.update(SIZE, 1.0);
        assert!(
            page.view
                .keyboard_scroll(&chain, ScrollKey::ArrowDown, false)
        );
        assert_eq!(page.view.viewport_scroll_offset(), PointF::new(0.0, 40.0));
        drop(page);

        for scrolling in ["no", "NoScroll", "off"] {
            let (mut page, child_document) =
                page_with_child_of(&format!("scrolling={scrolling}"), child);
            let chain = iframe_chain(&page, child_document);
            let child_scroll =
                |page: &Page| page.view.view_of(child_document).unwrap().scroll.viewport;
            assert!(
                page.view
                    .keyboard_scroll(&chain, ScrollKey::ArrowDown, false)
            );
            assert_eq!(child_scroll(&page), PointF::new(0.0, 0.0), "{scrolling}");
            assert_eq!(page.view.viewport_scroll_offset(), PointF::new(0.0, 40.0));
            assert!(
                page.view
                    .wheel(PointF::new(20.0, 30.0), PointF::new(0.0, 10.0))
            );
            assert_eq!(child_scroll(&page), PointF::new(0.0, 0.0), "{scrolling}");
            assert_eq!(page.view.viewport_scroll_offset(), PointF::new(0.0, 50.0));
            let frames = page.view.child_frames();
            let frame = frames.values().next().expect("the child frame");
            assert_eq!(
                (frame.user_scrollable_x, frame.user_scrollable_y),
                (false, false),
                "{scrolling}"
            );
        }
        let (mut page, child_document) = page_with_child_of("scrolling=auto", child);
        assert!(
            page.view
                .wheel(PointF::new(20.0, 30.0), PointF::new(0.0, 10.0))
        );
        assert_eq!(
            page.view.view_of(child_document).unwrap().scroll.viewport,
            PointF::new(0.0, 10.0)
        );
        let frames = page.view.child_frames();
        let frame = frames.values().next().expect("the child frame");
        assert_eq!(
            (frame.user_scrollable_x, frame.user_scrollable_y),
            (true, true)
        );
    }

    /// A hit test says whether what it hit is a run of the element's text
    /// (the cursor's I-beam), not beside it.
    #[test]
    fn a_hit_test_tells_text_from_its_box() {
        let mut page = Page::new(
            "<!doctype html><body style='margin: 0'><p style='margin: 0; font: 20px/1 Ahem'>XX</p>",
        );
        page.update(SIZE, 1.0);
        let over = page.view.hit_test(PointF::new(10.0, 10.0));
        let beside = page.view.hit_test(PointF::new(150.0, 10.0));
        assert!(over.element.is_some() && over.text.is_some());
        assert_eq!(over.element, beside.element, "the paragraph");
        assert!(beside.text.is_none());
    }

    /// The document scrolled into view once laid out: its viewport to the
    /// top (a fragment of `top`, or empty).
    #[test]
    fn the_top_of_the_document() {
        let mut page =
            Page::new("<!doctype html><body style='margin: 0'><div style='height: 300px'></div>");
        page.update(SIZE, 1.0);
        assert!(
            page.view
                .wheel(PointF::new(10.0, 10.0), PointF::new(0.0, 40.0))
        );
        let document = page.view.document_node;
        page.view.scroll_into_view_after_layout(document);
        page.update(SIZE, 1.0);
        assert_eq!(page.view.viewport_scroll_offset(), PointF::new(0.0, 0.0));
    }

    /// A page for scroll anchoring: laid out at 800x600, its elements by
    /// `id`.
    struct Anchoring {
        token: LocalToken,
        text: TextContext,
        view: FrameView,
        ids: HashMap<String, NodeId>,
        device_scale_factor: f32,
        /// Last: the view's root goes before its heap.
        dom: Dom,
    }

    /// Three 100px blocks, `a`, `b` and `c`, and 2000px after them.
    const ANCHORING: &str = "<style>html, body { margin: 0 } div { height: 100px }</style>\
        <div id=a></div><div id=b></div><div id=c></div><div id=filler style='height: 2000px'></div>";

    impl Anchoring {
        fn new(html: &str) -> Self {
            Self::at_scale(html, 1.0)
        }

        fn at_scale(html: &str, device_scale_factor: f32) -> Self {
            let mut dom = Dom::new();
            let document = html::parser::parse_document(&mut dom, html, true)
                .root()
                .clone();
            let (node, ids) = dom.read(|cx| {
                let mut ids = HashMap::new();
                let mut stack = vec![document.get(cx.gc)];
                while let Some(node) = stack.pop() {
                    if let Some(id) = cx.attribute_value(node, "id") {
                        ids.insert(id.borrow().to_string(), cx.node_id(node));
                    }
                    stack.extend(std::iter::successors(cx.first_child(node), |&child| {
                        cx.next_sibling(child)
                    }));
                }
                (cx.node_id(document.get(cx.gc)), ids)
            });
            let mut page = Self {
                token: LocalToken::new(),
                text: TextContext::new(true),
                view: FrameView::new(
                    document,
                    node,
                    DocumentId::default(),
                    gfx::SystemColors::DEFAULT,
                ),
                ids,
                device_scale_factor,
                dom,
            };
            page.lay_out();
            page
        }

        fn lay_out(&mut self) {
            let Self {
                token,
                text,
                view,
                device_scale_factor,
                dom,
                ..
            } = self;
            dom.mutate(|cx| {
                view.update_style_and_layout(
                    cx,
                    token,
                    text,
                    SizeF::new(800.0, 600.0),
                    *device_scale_factor,
                );
            });
        }

        /// Scrolls `scroller` (`None`: the viewport) to `y` as script does.
        fn scroll_to(&mut self, scroller: Option<&str>, y: f32) {
            let fragment = self.view.fragment.clone().unwrap();
            let target = scroller.map(|name| self.ids[name]);
            self.view
                .scroll
                .scroll_to(&fragment, target, PointF::new(0.0, y));
        }

        fn offset(&self, scroller: Option<&str>) -> f32 {
            match scroller {
                None => self.view.scroll.viewport.y,
                Some(name) => self
                    .view
                    .scroll
                    .containers
                    .get(&self.ids[name])
                    .map_or(0.0, |offset| offset.y),
            }
        }

        /// `f` with the element whose `id` is `name`, in a DOM operation.
        fn with(&mut self, name: &str, f: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>)) {
            let id = self.ids[name];
            self.dom
                .mutate(|cx| f(cx, cx.node_by_id(id).expect("in the page")));
        }

        fn set_style(&mut self, name: &str, style: &str) {
            self.with(name, |cx, element| {
                cx.set_attribute(element, "style", style)
            });
        }

        /// Puts a block `height` high before `name`.
        fn insert_before(&mut self, name: &str, height: &str) {
            self.with(name, |cx, reference| {
                let parent = cx.parent(reference).unwrap();
                let block =
                    cx.create_element(cx.document_of(reference), dom::QualName::html("div"));
                cx.set_attribute(block, "style", &format!("height: {height}"));
                cx.insert(parent, block, Some(reference));
            });
        }

        fn remove(&mut self, name: &str) {
            self.with(name, |cx, element| cx.remove(element));
        }

        /// Gives `name` its document's focus.
        fn focus(&mut self, name: &str) {
            self.with(name, |cx, element| {
                cx.set_focused_element(cx.document_of(element), Some(element), true);
            });
        }
    }

    /// Content added above what the viewport shows, or growing there,
    /// scrolls it by as much: what was shown stays where it was (`b`,
    /// partly in view, is the anchor). The move is a scroll for its
    /// event.
    #[test]
    fn content_above_the_anchor_scrolls_the_viewport() {
        let mut page = Anchoring::new(ANCHORING);
        page.scroll_to(None, 150.0);
        page.view.scroll.take_scrolled();
        page.insert_before("a", "30px");
        page.lay_out();
        assert_eq!(page.offset(None), 180.0);
        assert_eq!(page.view.scroll.take_scrolled(), [None]);
        page.set_style("a", "height: 150px");
        page.lay_out();
        assert_eq!(page.offset(None), 230.0);
        // Less above: back up.
        page.set_style("a", "height: 0");
        page.lay_out();
        assert_eq!(page.offset(None), 80.0);
        // Below: nothing.
        page.set_style("filler", "height: 3000px");
        page.lay_out();
        assert_eq!(page.offset(None), 80.0);
    }

    /// At the top, nothing is anchored: content added above shows.
    #[test]
    fn at_the_top_nothing_is_anchored() {
        let mut page = Anchoring::new(ANCHORING);
        page.insert_before("a", "30px");
        page.lay_out();
        assert_eq!(page.offset(None), 0.0);
        // Scrolled back to the top: the anchor goes.
        page.scroll_to(None, 150.0);
        page.lay_out();
        page.scroll_to(None, 0.0);
        page.insert_before("a", "30px");
        page.lay_out();
        assert_eq!(page.offset(None), 0.0);
    }

    /// A scroll other than anchoring's picks the anchor again, where it
    /// scrolled to; the clamp keeps it.
    #[test]
    fn a_scroll_picks_the_anchor_again() {
        let mut page = Anchoring::new(ANCHORING);
        page.scroll_to(None, 150.0);
        page.insert_before("a", "30px");
        page.lay_out();
        assert_eq!(page.offset(None), 180.0);
        // `b` (at 130) was 50px above the top: from 260, it is 130.
        page.scroll_to(None, 260.0);
        page.set_style("filler", "height: 3000px");
        page.lay_out();
        assert_eq!(page.offset(None), 260.0);
        // The document shrinks: the clamp moves up, to 830 - 600, and the
        // anchor (`b` 130px above the top) stays. With room again, it is
        // shown where it was.
        page.set_style("filler", "height: 500px");
        page.lay_out();
        assert_eq!(page.offset(None), 230.0);
        page.set_style("filler", "height: 3000px");
        page.lay_out();
        assert_eq!(page.offset(None), 260.0);
    }

    /// A container keeps what it shows as the viewport does. The
    /// viewport's anchor is in it too, `s2` (the search goes into scroll
    /// containers), and is measured first, before the container moves: the
    /// viewport moves as far.
    #[test]
    fn a_scroll_container_keeps_what_it_shows() {
        let mut page = Anchoring::new(
            "<style>html, body { margin: 0 } div { height: 100px }</style>\
             <div id=s style='height: 200px; overflow: auto'>\
             <div id=s1></div><div id=s2></div><div id=s3></div><div id=s4></div></div>\
             <div style='height: 2000px'></div>",
        );
        page.scroll_to(Some("s"), 150.0);
        page.scroll_to(None, 10.0);
        page.insert_before("s1", "30px");
        page.lay_out();
        assert_eq!(page.offset(Some("s")), 180.0);
        assert_eq!(page.offset(None), 40.0);
    }

    /// How far the anchor moved, in device pixels: 10.3px at 2x is 21
    /// device pixels from where `b` was, -100 device pixels (Blink rounds
    /// both ends).
    #[test]
    fn the_scroll_is_in_device_pixels() {
        let mut page = Anchoring::at_scale(ANCHORING, 2.0);
        page.scroll_to(None, 150.0);
        page.insert_before("a", "10.3px");
        page.lay_out();
        assert_eq!(page.offset(None), 160.5);
    }

    /// A change on the way from the anchor to its scroller to what moves
    /// boxes (the anchor's margin here) isn't scrolled for: the page meant
    /// it. The anchor goes, and is picked again: `b`, 10px above the top.
    /// A change elsewhere, or to what doesn't move boxes (a border), is.
    #[test]
    fn a_change_on_the_way_to_the_scroller_is_not_scrolled_for() {
        let mut page = Anchoring::new(ANCHORING);
        page.scroll_to(None, 150.0);
        page.set_style("b", "margin-top: 40px");
        page.lay_out();
        assert_eq!(page.offset(None), 150.0);
        page.insert_before("a", "30px");
        page.lay_out();
        assert_eq!(page.offset(None), 180.0);

        drop(page);
        let mut page = Anchoring::new(
            "<style>html, body { margin: 0 } div { height: 100px }</style>\
             <div id=a></div><div id=w style='height: auto'><div id=b></div></div>\
             <div id=filler style='height: 2000px'></div>",
        );
        page.scroll_to(None, 150.0);
        page.set_style("w", "height: auto; padding-top: 20px");
        page.lay_out();
        assert_eq!(page.offset(None), 150.0, "padding");
        page.scroll_to(None, 160.0);
        page.set_style(
            "w",
            "height: auto; padding-top: 20px; border-top: 20px solid",
        );
        page.lay_out();
        assert_eq!(page.offset(None), 180.0, "a border");
    }

    /// A box going out of the flow, or into it, anywhere in the scroller
    /// isn't scrolled for (the spec's second suppression trigger): `a`,
    /// above the anchor, not on its way up.
    #[test]
    fn a_box_leaving_the_flow_is_not_scrolled_for() {
        let mut page = Anchoring::new(ANCHORING);
        page.scroll_to(None, 150.0);
        page.set_style("a", "position: absolute; width: 10px");
        page.lay_out();
        assert_eq!(page.offset(None), 150.0);
        // In a container: its own anchor, not the viewport's.
        drop(page);
        let mut page = Anchoring::new(
            "<style>html, body { margin: 0 } div { height: 100px }</style>\
             <div id=s style='height: 200px; overflow: auto'>\
             <div id=s1></div><div id=s2></div><div id=s3></div><div id=s4></div>\
             <div id=s5></div></div><div id=a></div><div style='height: 2000px'></div>",
        );
        page.scroll_to(Some("s"), 150.0);
        page.scroll_to(None, 250.0);
        page.set_style("s1", "position: absolute; width: 10px");
        page.lay_out();
        assert_eq!(page.offset(Some("s")), 150.0);
        page.set_style("s", "height: 300px; overflow: auto");
        page.lay_out();
        assert_eq!(page.offset(None), 350.0, "the viewport's anchor, `a`");
        // A scroll container itself: noted for the scroller around it.
        drop(page);
        let read = run(
            "<style>html, body { margin: 0 } div { height: 100px }</style>\
             <div id=a></div><div id=s style='overflow: auto'></div><div id=b></div>\
             <div id=filler style='height: 2000px'></div>",
            &[
                Step::Scroll(None, 210.0),
                Step::Style("s", "position: absolute; overflow: auto; width: 10px"),
                Step::Read(None),
            ],
        );
        assert_eq!(read, [210.0], "a scroll container");
    }

    /// The whole box tree built again for the viewport's `overflow` keeps
    /// the anchor, and what keeps a move from being followed: `b`'s
    /// margin; for the root element's box, it drops the anchors. As
    /// Chrome 152 does.
    #[test]
    fn the_whole_box_tree_built_again() {
        let html = "<style>html, body { margin: 0 } div { height: 100px }</style>\
            <body id=body><div id=a></div><div id=b></div><div id=c></div>\
            <div id=filler style='height: 2000px'></div></body>";
        let read = run(
            html,
            &[
                Step::Scroll(None, 150.0),
                Step::Insert("a", "30px"),
                Step::Style("body", "overflow-y: scroll"),
                Step::Read(None),
            ],
        );
        assert_eq!(read, [180.0], "followed");
        let read = run(
            html,
            &[
                Step::Scroll(None, 150.0),
                Step::Style("b", "margin-top: 40px"),
                Step::Style("body", "overflow-y: scroll"),
                Step::Read(None),
            ],
        );
        assert_eq!(read, [150.0], "not followed");
        // The root element's box built again: every box is new, and
        // nothing is followed.
        let html = "<style>html, body { margin: 0 } div { height: 100px }</style>\
            <html id=html><body><div id=a></div><div id=b></div><div id=c></div>\
            <div id=filler style='height: 2000px'></div></body></html>";
        let read = run(
            html,
            &[
                Step::Scroll(None, 150.0),
                Step::Insert("a", "30px"),
                Step::Style("html", "display: flow-root"),
                Step::Read(None),
            ],
        );
        assert_eq!(read, [150.0], "the root's box");
    }

    /// An anchor that has no box any more is picked again, before layout,
    /// from what is left where it was: `c`, 50px down, now 50px up.
    #[test]
    fn an_anchor_without_a_box_is_picked_again() {
        for gone in ["remove", "display: none"] {
            let mut page = Anchoring::new(ANCHORING);
            page.scroll_to(None, 150.0);
            match gone {
                "remove" => page.remove("b"),
                style => page.set_style("b", style),
            }
            page.lay_out();
            assert_eq!(page.offset(None), 50.0, "{gone}");
        }
    }

    /// `overflow-anchor: none` on the anchor picks another: `c`, which
    /// `b` growing moves (on `b`'s way up, not `c`'s). On an inline
    /// element, its text isn't picked: the next line's is, which moves
    /// 20px down in its taller line. As Chrome 152 does.
    #[test]
    fn overflow_anchor_none_picks_another() {
        let mut page = Anchoring::new(ANCHORING);
        page.scroll_to(None, 150.0);
        page.set_style("b", "overflow-anchor: none; height: 130px");
        page.lay_out();
        assert_eq!(page.offset(None), 180.0);

        let text = "<style>html, body { margin: 0 } div { height: 100px }</style>\
            <div id=a></div><div id=t style='height: auto; line-height: 20px'>\
            <span id=s>Text</span><br><span id=u>Text</span></div>\
            <div id=filler style='height: 2000px'></div>";
        let taller = Step::Style("u", "line-height: 60px");
        drop(page);
        let read = run(text, &[Step::Scroll(None, 105.0), taller, Step::Read(None)]);
        assert_eq!(read, [105.0], "the first line's text");
        let read = run(
            text,
            &[
                Step::Scroll(None, 105.0),
                Step::Style("s", "overflow-anchor: none"),
                Step::Style("u", "line-height: 60px"),
                Step::Read(None),
            ],
        );
        assert_eq!(read, [125.0], "the next line's");
    }

    /// What a scroll anchoring case does, as the Chrome 152 probe for it
    /// does it.
    #[derive(Clone, Copy)]
    enum Step {
        /// Scrolls the viewport (`None`) or a container to a y, as script.
        Scroll(Option<&'static str>, f32),
        /// Puts a block that high before an element.
        Insert(&'static str, &'static str),
        Style(&'static str, &'static str),
        Remove(&'static str),
        /// Gives an element the document's focus.
        Focus(&'static str),
        /// Lays out and reads the viewport's or a container's offset.
        Read(Option<&'static str>),
    }

    fn run(html: &str, steps: &[Step]) -> Vec<f32> {
        let mut page = Anchoring::new(html);
        let mut read = Vec::new();
        for step in steps {
            match *step {
                Step::Scroll(scroller, y) => page.scroll_to(scroller, y),
                Step::Insert(before, height) => page.insert_before(before, height),
                Step::Style(name, style) => page.set_style(name, style),
                Step::Remove(name) => page.remove(name),
                Step::Focus(name) => page.focus(name),
                Step::Read(scroller) => {
                    page.lay_out();
                    read.push(page.offset(scroller));
                }
            }
        }
        read
    }

    /// The anchor stays until it has no box as it had: removed, or built
    /// again (another `display`, its parent's too), when one is picked
    /// again before layout, where things were (`c`, which moves). Styled
    /// again in place, or with `overflow-anchor: none` on it (picked
    /// again), as Chrome 152 does. One not moved by a change to it (no
    /// height) stays, though the change would have kept a move from
    /// being followed: content added under it moves nothing.
    #[test]
    fn an_anchor_stays_until_its_box_goes() {
        use Step::*;
        let anchored = [Scroll(None, 150.0), Insert("a", "30px"), Read(None)];
        let wrapped = "<style>html, body { margin: 0 } div { height: 100px }</style>\
            <div id=a></div><div id=w style='height: auto'><div id=b></div></div>\
            <div id=filler style='height: 2000px'></div>";
        let cases: [(&str, &str, Vec<Step>, Vec<f32>); 6] = [
            (
                "removed",
                ANCHORING,
                vec![Remove("b"), Read(None)],
                vec![180.0, 80.0],
            ),
            (
                "another display",
                ANCHORING,
                vec![Style("b", "display: flex"), Insert("c", "30px"), Read(None)],
                vec![180.0, 210.0],
            ),
            (
                "its parent's display",
                wrapped,
                vec![
                    Style("w", "height: auto; display: flex; flex-direction: column"),
                    Insert("filler", "30px"),
                    Read(None),
                ],
                vec![180.0, 210.0],
            ),
            (
                "painted again",
                ANCHORING,
                vec![
                    Style("b", "background: red"),
                    Insert("c", "30px"),
                    Read(None),
                ],
                vec![180.0, 180.0],
            ),
            (
                "overflow-anchor: none",
                ANCHORING,
                vec![
                    Style("b", "overflow-anchor: none; height: 130px"),
                    Read(None),
                ],
                vec![180.0, 210.0],
            ),
            (
                "no height",
                ANCHORING,
                vec![
                    Style("b", "height: 0"),
                    Read(None),
                    Insert("c", "30px"),
                    Read(None),
                ],
                vec![180.0, 180.0, 180.0],
            ),
        ];
        for (name, html, steps, expected) in cases {
            let steps: Vec<Step> = anchored.iter().copied().chain(steps).collect();
            assert_eq!(run(html, &steps), expected, "{name}");
        }
    }

    /// Each kind of property the suppression triggers name, changed on
    /// `b`'s parent so that `b` moves 20px down, isn't followed; a border
    /// (not one of them) is. A change outside a container doesn't keep its
    /// own anchoring from following. As Chrome 152 does.
    #[test]
    fn what_keeps_a_move_from_being_followed() {
        let page = |w: &str| {
            format!(
                "<style>html, body {{ margin: 0 }} div {{ height: 100px }}</style>\
                 <div id=a></div><div id=w style='{w}'><div id=b style='flex: none'></div>\
                 </div><div id=filler style='height: 2000px'></div>"
            )
        };
        // `b` at its parent's bottom.
        macro_rules! flex {
            ($sizes:literal) => {
                concat!(
                    $sizes,
                    "; display: flex; flex-direction: column; justify-content: flex-end"
                )
            };
        }
        let cases = [
            ("height: auto", "height: auto; padding-top: 20px", 150.0),
            ("height: auto", "height: auto; margin-top: 20px", 150.0),
            (
                "height: auto; position: relative",
                "height: auto; position: relative; top: 20px",
                150.0,
            ),
            (
                "height: auto",
                "height: auto; transform: translateY(20px)",
                150.0,
            ),
            (flex!("height: 100px"), flex!("height: 120px"), 150.0),
            (
                flex!("height: 100px"),
                flex!("height: 100px; min-height: 120px"),
                150.0,
            ),
            (
                flex!("height: 140px; max-height: 100px"),
                flex!("height: 140px; max-height: 120px"),
                150.0,
            ),
            (
                "height: auto",
                "height: auto; border-top: 20px solid",
                170.0,
            ),
        ];
        for (before, after, expected) in cases {
            let read = run(
                &page(before),
                &[
                    Step::Scroll(None, 150.0),
                    Step::Style("w", after),
                    Step::Read(None),
                ],
            );
            assert_eq!(read, [expected], "{after}");
        }
        // Two 300px inline-blocks before `b`: on one line at 800px, on two
        // at 400px, which moves `b` 20px down.
        let wide = |w: &str| {
            format!(
                "<style>html, body {{ margin: 0 }} div {{ height: 100px }}</style>\
                 <div id=a></div><div id=w style='height: auto; {w}'>\
                 <span style='display: inline-block; width: 300px; height: 20px'></span>\
                 <span style='display: inline-block; width: 300px; height: 20px'></span>\
                 <div id=b></div></div><div id=filler style='height: 2000px'></div>"
            )
        };
        for (before, after, expected) in [
            ("width: 800px", "height: auto; width: 400px", 150.0),
            (
                "width: 400px",
                "height: auto; width: 400px; min-width: 800px",
                150.0,
            ),
            (
                "width: 800px",
                "height: auto; width: 800px; max-width: 400px",
                150.0,
            ),
            (
                "width: 800px",
                "height: auto; width: 800px; border-top: 20px solid",
                170.0,
            ),
        ] {
            let read = run(
                &wide(before),
                &[
                    Step::Scroll(None, 150.0),
                    Step::Style("w", after),
                    Step::Read(None),
                ],
            );
            assert_eq!(read, [expected], "{after}");
        }
        // An inline element on the way up from the anchor, its text.
        let text = "<style>html, body { margin: 0 } div { height: 100px }</style>\
            <div id=a></div><div id=t style='height: auto'><span id=s>Text</span></div>\
            <div id=filler style='height: 2000px'></div>";
        let read = run(
            text,
            &[
                Step::Scroll(None, 105.0),
                Step::Style("s", "position: relative; top: 20px"),
                Step::Read(None),
            ],
        );
        assert_eq!(read, [105.0], "an inline element");
        let read = run(
            text,
            &[
                Step::Scroll(None, 105.0),
                Step::Insert("a", "20px"),
                Step::Read(None),
            ],
        );
        assert_eq!(read, [125.0], "the text followed");
        // `position` on the way up, with something above moving.
        let read = run(
            &page("height: auto"),
            &[
                Step::Scroll(None, 150.0),
                Step::Style("w", "height: auto; position: relative"),
                Step::Style("a", "height: 130px"),
                Step::Read(None),
            ],
        );
        assert_eq!(read, [150.0], "position");
        let read = run(
            "<style>html, body { margin: 0 } div { height: 100px }</style>\
             <div id=o style='height: auto'><div id=s style='height: 200px; overflow: auto'>\
             <div id=s1></div><div id=s2></div><div id=s3></div><div id=s4></div></div></div>\
             <div style='height: 2000px'></div>",
            &[
                Step::Scroll(Some("s"), 150.0),
                Step::Style("o", "height: auto; padding-top: 20px"),
                Step::Insert("s1", "30px"),
                Step::Read(Some("s")),
            ],
        );
        assert_eq!(read, [180.0], "outside the container");
    }

    /// A container's anchor goes as the viewport's does: with a scroll by
    /// script, then picked again there (`s3`, which nothing moves); not
    /// with the clamp, so it comes back as there is room. As Chrome 152
    /// does.
    #[test]
    fn a_containers_anchor_goes_as_the_viewports() {
        let html = "<style>html, body { margin: 0 } div { height: 100px }</style>\
            <div id=s style='height: 200px; overflow: auto'><div id=s1></div><div id=s2></div>\
            <div id=s3></div><div id=s4 style='height: 300px'></div></div>\
            <div style='height: 2000px'></div>";
        let anchored = || {
            vec![
                Step::Scroll(Some("s"), 150.0),
                Step::Insert("s1", "30px"),
                Step::Read(Some("s")),
            ]
        };
        let mut steps = anchored();
        steps.extend([
            Step::Scroll(Some("s"), 260.0),
            Step::Style("s4", "height: 400px"),
            Step::Read(Some("s")),
        ]);
        assert_eq!(run(html, &steps), [180.0, 260.0], "a scroll");
        let mut steps = anchored();
        steps.extend([
            Step::Style("s4", "height: 0"),
            Step::Read(Some("s")),
            Step::Style("s4", "height: 300px"),
            Step::Read(Some("s")),
        ]);
        assert_eq!(run(html, &steps), [180.0, 130.0, 180.0], "the clamp");
    }

    /// The focused element in editable content is the anchor first (the
    /// priority candidate), where it is in view, even partly: it stays
    /// where it was as what is above grows by 2900px. Not a text control,
    /// nor an element focusable but not editable; not one in a box with
    /// `overflow-anchor: none` or a sticky one. An inline editing host's
    /// is the box around it. As Chrome 152 does.
    #[test]
    fn the_focused_editable_element_first() {
        let page = |focused: &str| {
            format!(
                "<style>html, body {{ margin: 0 }} body {{ height: 4000px }} .s {{ height: 100px }}</style>\
                 <div class=s></div><div class=s></div><div class=s></div><div class=s></div>\
                 <div id=growing class=s></div><div class=s></div>{focused}<div class=s></div>"
            )
        };
        for (focused, expected) in [
            (
                "<div id=focused contenteditable style='height: 10px'></div>",
                3050.0,
            ),
            (
                "<div contenteditable style='height: auto'>\
                 <p id=focused style='margin: 0; height: 10px'>x</p></div>",
                3050.0,
            ),
            (
                "<div id=focused contenteditable style='height: 300px'></div>",
                3050.0,
            ),
            (
                "<div id=box style='height: auto'><span id=focused contenteditable>x</span></div>",
                3050.0,
            ),
            (
                "<textarea id=focused style='height: 10px; display: block'></textarea>",
                150.0,
            ),
            (
                "<input id=focused style='height: 10px; display: block'>",
                150.0,
            ),
            (
                "<div id=focused tabindex=0 style='height: 10px'></div>",
                150.0,
            ),
            (
                "<div style='overflow-anchor: none; height: auto'>\
                 <div id=focused contenteditable style='height: 10px'></div></div>",
                150.0,
            ),
            (
                "<div style='position: sticky; top: 0; height: auto'>\
                 <div id=focused contenteditable style='height: 10px'></div></div>",
                150.0,
            ),
        ] {
            let read = run(
                &page(focused),
                &[
                    Step::Scroll(None, 150.0),
                    Step::Focus("focused"),
                    Step::Style("growing", "height: 3000px"),
                    Step::Read(None),
                ],
            );
            assert_eq!(read, [expected], "{focused}");
        }
        // Partly in view, it is searched: `p`, all in view in it, is the
        // anchor, and moves 50px down.
        let read = run(
            &page(
                "<div id=focused contenteditable style='height: auto'>\
                 <div id=growing2 style='height: 50px'></div>\
                 <p style='margin: 0; height: 10px'>x</p><div style='height: 200px'></div></div>",
            ),
            &[
                Step::Focus("focused"),
                Step::Scroll(None, 650.0),
                Step::Style("growing2", "height: 100px"),
                Step::Read(None),
            ],
        );
        assert_eq!(read, [700.0], "what is in it");
    }

    /// A container with `overflow-anchor: none` isn't anchored, though
    /// the viewport is; set later, its anchor goes; set around it, its
    /// anchoring goes on. An inline element with
    /// it isn't gone through: its text, though an element in it is `auto`.
    /// As Chrome 152 does (WPT's `opt-out*` and `exclude-inline`).
    #[test]
    fn overflow_anchor_none_on_the_scroller_and_an_inline() {
        let html = "<style>html, body { margin: 0 } div { height: 100px }</style>\
            <div id=s style='height: 200px; overflow: auto'><div id=s1></div><div id=s2></div>\
            <div id=s3></div><div id=s4 style='height: 300px'></div></div>\
            <div style='height: 2000px'></div>";
        let read = run(
            html,
            &[
                Step::Scroll(Some("s"), 150.0),
                Step::Style("s", "height: 200px; overflow: auto; overflow-anchor: none"),
                Step::Insert("s1", "30px"),
                Step::Read(Some("s")),
            ],
        );
        assert_eq!(read, [150.0], "set first");
        let read = run(
            html,
            &[
                Step::Scroll(Some("s"), 150.0),
                Step::Insert("s1", "30px"),
                Step::Read(Some("s")),
                Step::Style("s", "height: 200px; overflow: auto; overflow-anchor: none"),
                Step::Insert("s1", "30px"),
                Step::Read(Some("s")),
            ],
        );
        assert_eq!(read, [180.0, 180.0], "set later");
        // On `<body>`, around the container: the container's own anchoring
        // goes on (WPT's `opt-out`).
        let read = run(
            &html.replace("</style>", "body { overflow-anchor: none }</style>"),
            &[
                Step::Scroll(Some("s"), 150.0),
                Step::Insert("s1", "30px"),
                Step::Read(Some("s")),
            ],
        );
        assert_eq!(read, [180.0], "around the container");
        let read = run(
            "<style>html, body { margin: 0 } #expander { margin-bottom: 50px }\
             #no { overflow-anchor: none } #spacing { margin-bottom: 1800px }</style>\
             <span>out of view</span><div id=expander></div>\
             <span id=no>excluded subtree <span>[nested inline]</span></span>\
             <div id=spacing></div>",
            &[
                Step::Scroll(None, 50.0),
                Step::Style("expander", "margin-bottom: 100px"),
                Step::Read(None),
            ],
        );
        assert_eq!(read, [50.0], "an inline element");
    }

    /// The move scroll anchoring makes is clamped: `c` moves 30px down
    /// with the document no longer, at its end.
    #[test]
    fn the_move_is_clamped() {
        let read = run(
            "<style>html, body { margin: 0 } div { height: 100px }</style>\
             <div id=a></div><div id=b style='height: 1000px'></div>\
             <div id=c style='height: 1000px'></div><div id=d></div>",
            &[
                Step::Scroll(None, 1600.0),
                Step::Read(None),
                Step::Style("a", "height: 130px"),
                Step::Style("d", "height: 70px"),
                Step::Read(None),
            ],
        );
        assert_eq!(read, [1600.0, 1600.0]);
    }
}
