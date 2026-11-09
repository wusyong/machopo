//! Compositing types (`viz/public/mojom/compositing/*.mojom`).
//!
//! A [`CompositorFrame`] is what a client (the renderer's cc) submits to Viz:
//! a list of render passes, each a list of [`DrawQuad`]s, plus the resources
//! (textures) those quads reference. Viz aggregates the frames of every
//! surface into one and draws it.

use std::any::Any;
use std::sync::{Arc, Weak};

use base::numerics::saturated_cast;
use gfx::{Color, RRectF, RectF, Size};
use serde::{Deserialize, Serialize};

/// The widest and tallest texture a client may hand Viz (a canvas's
/// layer), in pixels (Chromium's `ContextCapabilities::max_texture_size`):
/// what WebGPU guarantees of `max_texture_dimension_2d`, and Viz takes no
/// device with less. Fixed rather than the device's own: a renderer starts
/// before Viz has a device, and a lost device is replaced by another.
pub const MAX_TEXTURE_SIZE: u32 = 8192;

/// `viz::FrameSinkId`: identifies one frame producer (one cc instance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FrameSinkId {
    pub client_id: u32,
    pub sink_id: u32,
}

/// `viz::LocalSurfaceId`: bumped by the embedder whenever visual properties
/// (size, scale) change, so Viz knows which frames match the current geometry.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct LocalSurfaceId {
    pub parent_sequence_number: u32,
    pub child_sequence_number: u32,
}

/// `viz::SurfaceId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SurfaceId {
    pub frame_sink_id: FrameSinkId,
    pub local_surface_id: LocalSurfaceId,
}

/// `viz::ResourceId`, unique per frame sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceId(pub u64);

/// Pixel layout of a [`TransferableResource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceFormat {
    /// 8-bit RGBA, premultiplied alpha, sRGB-encoded (no linearisation).
    Rgba8Premul,
}

impl ResourceFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            ResourceFormat::Rgba8Premul => 4,
        }
    }
}

/// `gpu::Mailbox`: names a [`SharedImage`] across threads (and, once the
/// renderer is its own process, processes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Mailbox(pub u64);

/// A GPU image a client made through a [`SharedImageInterface`] (Chromium's
/// `gpu::ClientSharedImage`): Viz draws it as it is, with no pixels sent.
/// The backing keeps it alive while the client's tile or a frame holds it,
/// standing in, in one process, for `DestroySharedImage` once Viz has
/// returned it (`ReturnResources`); it isn't sent across processes.
#[derive(Clone, Serialize, Deserialize)]
pub struct SharedImage {
    pub mailbox: Mailbox,
    #[serde(skip)]
    backing: Option<Arc<dyn Any + Send + Sync>>,
}

impl SharedImage {
    pub fn new(mailbox: Mailbox, backing: Arc<dyn Any + Send + Sync>) -> Self {
        Self {
            mailbox,
            backing: Some(backing),
        }
    }

    /// What the interface that made it put behind it.
    pub fn backing<T: Any>(&self) -> Option<&T> {
        self.backing.as_deref()?.downcast_ref()
    }
}

impl std::fmt::Debug for SharedImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SharedImage").field(&self.mailbox).finish()
    }
}

/// `gpu::SharedImageInterface`: how a client puts pixels on the GPU Viz
/// draws with, once, instead of sending them with every frame that shows
/// them. Viz implements it; the client sees only [`SharedImage`]s.
pub trait SharedImageInterface: Send + Sync {
    /// `CreateSharedImage` with `pixels` (tightly packed RGBA,
    /// premultiplied) as its contents. `None` if there is no GPU to put it
    /// on (not yet, or a lost one not made again yet): the client keeps
    /// the pixels.
    fn create_shared_image(&self, size: Size, pixels: &[u8]) -> Option<SharedImage>;

    /// Tells `observer` when the GPU is lost. Held weakly: it is told until
    /// its client drops it (Chromium's clients remove theirs).
    fn add_context_lost_observer(&self, observer: Weak<dyn ContextLostObserver>);
}

/// `viz::ContextLostObserver`: the GPU the shared images were on was lost
/// (a driver reset, say) and Viz has a new one. Every shared image made
/// before is gone: the client drops them and makes new ones. Called on
/// Viz's thread, so it posts to the client's own.
pub trait ContextLostObserver: Send + Sync {
    fn on_context_lost(&self);
}

/// `viz::TransferableResource`.
///
/// In Chromium this names a GPU `SharedImage` (mailbox + sync token). With
/// a [`SharedImage`] the pixels are on the GPU and `pixels` is empty;
/// without one (no GPU, or software drawing) they live in CPU memory, the
/// `Arc` standing in for the shared-memory region so handing a frame to
/// Viz never copies them.
#[derive(Clone, Serialize, Deserialize)]
pub struct TransferableResource {
    pub id: ResourceId,
    pub size: Size,
    pub format: ResourceFormat,
    /// Tightly packed rows (`stride == size.width * bytes_per_pixel`);
    /// empty with a shared image.
    pub pixels: Arc<[u8]>,
    pub shared_image: Option<SharedImage>,
}

impl std::fmt::Debug for TransferableResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransferableResource")
            .field("id", &self.id)
            .field("size", &self.size)
            .field("format", &self.format)
            .field("bytes", &self.pixels.len())
            .field("shared_image", &self.shared_image)
            .finish()
    }
}

/// `viz::CompositorRenderPassId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RenderPassId(pub u64);

/// `viz::SharedQuadState`: state shared by consecutive quads from one layer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SharedQuadState {
    /// Clip in render pass (target) space, if any.
    pub clip_rect: Option<RectF>,
    /// A clip with rounded corners, in the same space, inside `clip_rect`
    /// (`viz::SharedQuadState::mask_filter_info`, Chromium's fast rounded
    /// corners): a scroll container's `border-radius`. The renderer draws
    /// only what is inside its curves, anti-aliased.
    pub rounded_clip: Option<RRectF>,
    pub opacity: f32,
}

impl Default for SharedQuadState {
    fn default() -> Self {
        Self {
            clip_rect: None,
            rounded_clip: None,
            opacity: 1.0,
        }
    }
}

/// `viz::DrawQuad` material.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Material {
    /// `viz::SolidColorDrawQuad`.
    SolidColor { color: Color },
    /// `viz::TileDrawQuad` / `TextureDrawQuad`: samples `resource_id` over
    /// `tex_coord_rect` (in texels of the resource): the nearest texel
    /// (`TextureDrawQuad::nearest_neighbor`; a tile, drawn 1:1), or
    /// bilinearly (a canvas's layer, which may be scaled).
    Texture {
        resource_id: ResourceId,
        tex_coord_rect: RectF,
        nearest_neighbor: bool,
    },
    /// `viz::SurfaceDrawQuad`: another client's surface, embedded here.
    /// Viz's aggregator replaces it with that surface's quads, moved to
    /// the quad's origin and clipped to it, so a `DirectRenderer` never
    /// sees one.
    Surface {
        surface_id: SurfaceId,
        /// Shown while the surface has no frame yet.
        default_background_color: Color,
    },
}

/// `viz::DrawQuad`. Geometry is already in render pass (target) space:
/// there is no `quad_to_target_transform` yet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DrawQuad {
    pub rect: RectF,
    /// Index into [`CompositorRenderPass::shared_quad_state_list`].
    pub shared_quad_state: usize,
    pub material: Material,
}

/// `viz::CompositorRenderPass`. Quads are ordered front-to-back, as in
/// Chromium, so renderers draw them in reverse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositorRenderPass {
    pub id: RenderPassId,
    pub output_rect: RectF,
    pub damage_rect: RectF,
    pub shared_quad_state_list: Vec<SharedQuadState>,
    pub quad_list: Vec<DrawQuad>,
}

/// `viz::CompositorFrameMetadata`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositorFrameMetadata {
    pub device_scale_factor: f32,
    /// Monotonic per sink; lets the client match presentation feedback.
    pub frame_token: u64,
    /// Background painted behind all quads (`root_background_color`).
    pub root_background_color: Color,
}

/// `viz::CompositorFrame`. The last render pass is the root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositorFrame {
    pub metadata: CompositorFrameMetadata,
    pub resource_list: Vec<TransferableResource>,
    pub render_pass_list: Vec<CompositorRenderPass>,
}

impl CompositorFrame {
    pub fn root_pass(&self) -> Option<&CompositorRenderPass> {
        self.render_pass_list.last()
    }

    /// Size of the root render pass in physical pixels.
    pub fn size_in_pixels(&self) -> Size {
        self.root_pass()
            .map(|p| {
                Size::new(
                    saturated_cast(p.output_rect.width),
                    saturated_cast(p.output_rect.height),
                )
            })
            .unwrap_or_default()
    }
}

/// Client -> Viz (`viz.mojom.CompositorFrameSink`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompositorFrameSinkMsg {
    /// A frame, in answer to a `BeginFrame` (the browser's own UI submits
    /// without one).
    SubmitCompositorFrame {
        local_surface_id: LocalSurfaceId,
        frame: CompositorFrame,
    },
    /// Whether the client wants `BeginFrame`s (`SetNeedsBeginFrame`): it has
    /// something to draw, or animation frame callbacks to run.
    SetNeedsBeginFrame(bool),
    /// The answer to a `BeginFrame` that produced no frame
    /// (`DidNotProduceFrame`), so the source may send the next.
    DidNotProduceFrame,
}

/// `viz::BeginFrameArgs`: a frame is due. Viz sends it to the clients that
/// asked (`CompositorFrameSinkClient::OnBeginFrame`), and each answers with
/// a frame or `DidNotProduceFrame` before it gets the next.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BeginFrameArgs {
    /// Increases by one with every `BeginFrame` the source sends.
    pub sequence: u64,
    /// When the frame is for, in milliseconds on the source's clock (real
    /// time in a window; frames times 1000/60 when headless, so tests see
    /// the same times every run). `requestAnimationFrame` callbacks get it.
    pub frame_time: f64,
}

/// Browser -> Viz (`viz.mojom.DisplayPrivate`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DisplayMsg {
    /// The output surface (window) changed size, in physical pixels.
    Resize(Size),
    /// Draw again even without new frames (e.g. the window was exposed).
    ForceRedraw,
    /// Loses the GPU device, as a driver reset would: `about:gpucrash`
    /// (Chromium's `chrome://gpucrash`, which crashes the GPU process).
    LoseContext,
    /// A copy of the next frame drawn, even if nothing changed
    /// (`viz::Surface::RequestCopyOfOutput`, which DevTools' screenshots
    /// and web tests' pixel dumps use).
    RequestCopyOfOutput(CopyOutputRequest),
    Shutdown,
}

/// `viz::CopyOutputRequest`: asks for a copy of what Viz draws next, which
/// it hands to the callback once drawn. The callback stands in, in one
/// process, for the pipe the result would go back on; it isn't sent across
/// processes. Dropped unanswered if Viz goes first.
#[derive(Clone, Serialize, Deserialize)]
pub struct CopyOutputRequest {
    #[serde(skip)]
    callback: Option<Arc<dyn Fn(Option<CopyOutputResult>) + Send + Sync>>,
}

impl CopyOutputRequest {
    /// `callback` gets the copy, or `None` if the output can't be read
    /// back.
    pub fn new(callback: impl Fn(Option<CopyOutputResult>) + Send + Sync + 'static) -> Self {
        Self {
            callback: Some(Arc::new(callback)),
        }
    }

    pub fn send_result(self, result: Option<CopyOutputResult>) {
        if let Some(callback) = self.callback {
            callback(result);
        }
    }
}

impl std::fmt::Debug for CopyOutputRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CopyOutputRequest")
    }
}

/// Where Viz sends a client's `BeginFrame`s
/// (`CompositorFrameSinkClient::OnBeginFrame`): the browser makes it, for
/// the client's compositor. The callback stands in, in one process, for
/// the pipe to the client; it isn't sent across processes (Viz stays in
/// the browser process, which forwards it).
#[derive(Clone, Serialize, Deserialize)]
pub struct BeginFrameObserver {
    #[serde(skip)]
    send: Option<Arc<dyn Fn(BeginFrameArgs) + Send + Sync>>,
}

impl BeginFrameObserver {
    pub fn new(send: impl Fn(BeginFrameArgs) + Send + Sync + 'static) -> Self {
        Self {
            send: Some(Arc::new(send)),
        }
    }

    pub fn on_begin_frame(&self, args: BeginFrameArgs) {
        if let Some(send) = &self.send {
            send(args);
        }
    }
}

impl std::fmt::Debug for BeginFrameObserver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BeginFrameObserver")
    }
}

/// `viz::CopyOutputResult`: the output as drawn, in physical pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopyOutputResult {
    pub size: Size,
    /// RGBA8, premultiplied, tightly packed.
    pub pixels: Vec<u8>,
}

/// Everything the Viz thread receives. In Chromium these arrive on separate
/// mojo pipes; here they share one channel so the Viz thread has a single
/// place to block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VizMsg {
    FrameSink(FrameSinkId, CompositorFrameSinkMsg),
    Display(DisplayMsg),
    /// `HostFrameSinkManager::RegisterFrameSinkId`, from the browser: a new
    /// client (a tab's renderer) may submit frames for this sink and ask
    /// for `BeginFrame`s, which go to the observer.
    RegisterFrameSinkId(FrameSinkId, BeginFrameObserver),
    /// `HostFrameSinkManager::InvalidateFrameSinkId`: the client has gone
    /// (its tab was closed). No more `BeginFrame`s, and its surface goes.
    InvalidateFrameSinkId(FrameSinkId),
}

/// A client's way to its `CompositorFrameSink` in Viz (a
/// `mojom::CompositorFrameSink` remote, which the browser's
/// `FrameSinkManager` binds to the client's `FrameSinkId`): the browser
/// makes it for the client's sink, and the client can't change which, so
/// it speaks for no other. Once the renderer is a process of its own, the
/// browser's IO thread adds the id instead.
#[derive(Clone)]
pub struct CompositorFrameSinkSender {
    frame_sink_id: FrameSinkId,
    viz: crossbeam_channel::Sender<VizMsg>,
}

impl CompositorFrameSinkSender {
    /// For the browser: `frame_sink_id`'s way to Viz, at `viz`.
    pub fn new(frame_sink_id: FrameSinkId, viz: crossbeam_channel::Sender<VizMsg>) -> Self {
        Self { frame_sink_id, viz }
    }

    pub fn frame_sink_id(&self) -> FrameSinkId {
        self.frame_sink_id
    }

    /// Sends `msg` to Viz, from this sink. Fails once Viz has gone; the
    /// message is dropped.
    pub fn send(&self, msg: CompositorFrameSinkMsg) -> Result<(), VizGone> {
        self.viz
            .send(VizMsg::FrameSink(self.frame_sink_id, msg))
            .map_err(|_| VizGone)
    }
}

impl std::fmt::Debug for CompositorFrameSinkSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("CompositorFrameSinkSender")
            .field(&self.frame_sink_id)
            .finish()
    }
}

/// Viz has gone, and its queue with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VizGone;

#[cfg(test)]
mod tests {
    use super::*;

    /// What a client sends goes as its own sink's: it names no other.
    #[test]
    fn a_frame_sink_sender_speaks_for_its_sink() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let sink = FrameSinkId {
            client_id: 3,
            sink_id: 1,
        };
        let sender = CompositorFrameSinkSender::new(sink, tx);
        assert_eq!(sender.frame_sink_id(), sink);
        sender
            .send(CompositorFrameSinkMsg::DidNotProduceFrame)
            .unwrap();
        sender
            .send(CompositorFrameSinkMsg::SetNeedsBeginFrame(true))
            .unwrap();
        let sent: Vec<_> = rx
            .try_iter()
            .map(|msg| match msg {
                VizMsg::FrameSink(id, msg) => (id, format!("{msg:?}")),
                msg => panic!("not a frame sink's: {msg:?}"),
            })
            .collect();
        assert_eq!(
            sent,
            [
                (sink, "DidNotProduceFrame".to_owned()),
                (sink, "SetNeedsBeginFrame(true)".to_owned())
            ]
        );
        drop(rx);
        assert_eq!(
            sender.send(CompositorFrameSinkMsg::DidNotProduceFrame),
            Err(VizGone)
        );
    }
}
