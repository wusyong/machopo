//! Compositing types (`viz/public/mojom/compositing/*.mojom`).
//!
//! A [`CompositorFrame`] is what a client (the renderer's cc) submits to Viz:
//! a list of render passes, each a list of [`DrawQuad`]s, plus the resources
//! (textures) those quads reference. Viz aggregates the frames of every
//! surface into one and draws it.

use std::sync::{Arc, Weak};

use crate::BigBuffer;
use crate::control_channel::{ChannelLink, send_over, to_browser};
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

/// A GPU image its client made (`CompositorFrameSinkMsg::CreateSharedImage`;
/// Chromium's `gpu::ClientSharedImage`), as a frame names it: by the
/// client's number for it. Viz draws it as it is, with no pixels sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedImage {
    pub mailbox: Mailbox,
}

impl SharedImage {
    pub fn from_mailbox(mailbox: Mailbox) -> Self {
        Self { mailbox }
    }
}

/// Viz's GPU as the browser sees it (Chromium's `viz::ContextProvider`):
/// whether there is one for a renderer's shared images, and where to hear
/// that it was lost. The browser's alone: a renderer hears through a
/// message.
pub trait ContextProvider: Send + Sync {
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
    pub pixels: BigBuffer,
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

/// The most quads a frame may draw, in all its render passes. Viz puts a
/// frame's vertices in one buffer (six of 80 bytes a quad: 120 MiB, under
/// the 256 MiB a buffer may have on any device it takes) and draws every
/// client's frames on one thread: a frame of millions would hold up every
/// tab's. A page's compositor draws a quad a visible tile, a few thousand
/// at most, and draws no more than this (it leaves the rest out).
pub const MAX_QUADS: usize = 1 << 18;

/// The most render passes a frame may have (Viz draws the last).
pub const MAX_RENDER_PASSES: usize = 1024;

/// What makes a client's message to Viz wrong whatever Viz holds (Chromium's
/// checks as the mojo traits read a `CompositorFrame`, and as
/// `SharedImageStub` validates a shared image's pixels): the client's bad
/// message. What depends on Viz (a shared image it no longer has after a
/// lost device, a sink no longer registered) isn't one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidFrameSinkMsg {
    /// A frame without a render pass.
    NoRenderPass,
    /// A frame whose root pass draws nothing: an output that is empty, or
    /// no rect at all (NaN).
    EmptyOutput,
    /// More than [`MAX_RENDER_PASSES`].
    TooManyRenderPasses,
    /// More than [`MAX_QUADS`] in all the passes.
    TooManyQuads,
    /// More shared quad states than [`MAX_QUADS`] and one (a layer's clip
    /// is a state, whether any of its tiles is drawn or not).
    TooManySharedQuadStates,
    /// A quad of a shared quad state its pass hasn't.
    SharedQuadStateOutOfRange,
    /// A quad of a resource its frame hasn't.
    UnknownResource,
    /// Two resources of one id.
    DuplicateResource,
    /// A resource's pixels that aren't those of a texture of its size Viz
    /// can make, or pixels besides its shared image.
    ResourcePixels,
    /// A surface embedded by a client that may embed none (a renderer's
    /// compositor embeds no other's).
    EmbedsSurface,
    /// A `CreateSharedImage` of no size, or pixels that aren't those of a
    /// texture of its size Viz can make.
    SharedImagePixels,
}

/// Whether `pixels` bytes are the pixels of an RGBA8 texture of `size`
/// Viz can make: no side past [`MAX_TEXTURE_SIZE`], and as many as the
/// size has (Chromium's `CheckedNumeric` size of a shared bitmap).
pub fn pixels_fit(size: Size, pixels: usize) -> bool {
    size.width <= MAX_TEXTURE_SIZE
        && size.height <= MAX_TEXTURE_SIZE
        && size.checked_byte_size() == Some(pixels)
}

impl CompositorFrame {
    /// Whether the frame can be right, from a client that `may_embed`
    /// others' surfaces (the browser's own UI) or not (a renderer's).
    /// Coordinates of NaN or infinity are not checked: they draw nothing,
    /// and wgpu takes them (vertices aren't validated).
    pub fn check(&self, may_embed: bool) -> Result<(), InvalidFrameSinkMsg> {
        let Some(root) = self.root_pass() else {
            return Err(InvalidFrameSinkMsg::NoRenderPass);
        };
        if !(root.output_rect.width > 0.0 && root.output_rect.height > 0.0) {
            return Err(InvalidFrameSinkMsg::EmptyOutput);
        }
        if self.render_pass_list.len() > MAX_RENDER_PASSES {
            return Err(InvalidFrameSinkMsg::TooManyRenderPasses);
        }
        let (mut quads, mut states) = (0_usize, 0_usize);
        for pass in &self.render_pass_list {
            quads = quads.saturating_add(pass.quad_list.len());
            states = states.saturating_add(pass.shared_quad_state_list.len());
        }
        if quads > MAX_QUADS {
            return Err(InvalidFrameSinkMsg::TooManyQuads);
        }
        if states > MAX_QUADS.saturating_add(1) {
            return Err(InvalidFrameSinkMsg::TooManySharedQuadStates);
        }
        let mut resources = std::collections::HashSet::with_capacity(self.resource_list.len());
        for resource in &self.resource_list {
            if !resources.insert(resource.id) {
                return Err(InvalidFrameSinkMsg::DuplicateResource);
            }
            let fits = match resource.shared_image {
                // Its size is what the quads sample in.
                Some(_) => {
                    resource.pixels.is_empty()
                        && resource.size.width <= MAX_TEXTURE_SIZE
                        && resource.size.height <= MAX_TEXTURE_SIZE
                }
                None => pixels_fit(resource.size, resource.pixels.len()),
            };
            if !fits {
                return Err(InvalidFrameSinkMsg::ResourcePixels);
            }
        }
        for pass in &self.render_pass_list {
            for quad in &pass.quad_list {
                if quad.shared_quad_state >= pass.shared_quad_state_list.len() {
                    return Err(InvalidFrameSinkMsg::SharedQuadStateOutOfRange);
                }
                match quad.material {
                    Material::SolidColor { .. } => {}
                    Material::Texture { resource_id, .. } => {
                        if !resources.contains(&resource_id) {
                            return Err(InvalidFrameSinkMsg::UnknownResource);
                        }
                    }
                    Material::Surface { .. } => {
                        if !may_embed {
                            return Err(InvalidFrameSinkMsg::EmbedsSurface);
                        }
                    }
                }
            }
        }
        Ok(())
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
    /// Pixels to put on the GPU as the shared image `mailbox`, a number of
    /// the client's own (`SharedImageInterface::CreateSharedImage` with
    /// pixels; Chromium's client makes the mailbox too): tightly packed
    /// RGBA, premultiplied, `size` of them. Sent as soon as they are drawn,
    /// before the frame that shows them, which names the mailbox alone; the
    /// channel keeps the two in order, so there are no sync tokens. Viz
    /// hands the buffer back once the pixels are on the GPU
    /// ([`BeginFrameObserver::on_pixels_returned`]): the client draws its next
    /// into it. The pixels are shared with whoever drew them, not copied
    /// into the message: the client's own buffer within a process, and a
    /// region of the channel's it drew in from a process of its own
    /// (which is its to draw in again when Viz lets the pixels go).
    CreateSharedImage {
        mailbox: Mailbox,
        size: Size,
        pixels: crate::SharedBuffer,
    },
    /// The client is done with the shared image `mailbox`
    /// (`DestroySharedImage`). Viz lets its texture go once no frame it
    /// holds draws it.
    DestroySharedImage { mailbox: Mailbox },
}

/// Whether a `CreateSharedImage` of `size` and `bytes` of pixels can be
/// right: some size, and the pixels of it ([`pixels_fit`]).
pub fn check_shared_image(size: Size, bytes: usize) -> Result<(), InvalidFrameSinkMsg> {
    if size.is_empty() || !pixels_fit(size, bytes) {
        Err(InvalidFrameSinkMsg::SharedImagePixels)
    } else {
        Ok(())
    }
}

impl CompositorFrameSinkMsg {
    /// Whether the message can be right, from a client that `may_embed`
    /// others' surfaces or not ([`CompositorFrame::check`]).
    pub fn check(&self, may_embed: bool) -> Result<(), InvalidFrameSinkMsg> {
        match self {
            Self::SubmitCompositorFrame { frame, .. } => frame.check(may_embed),
            Self::CreateSharedImage { size, pixels, .. } => check_shared_image(*size, pixels.len()),
            Self::SetNeedsBeginFrame(_)
            | Self::DidNotProduceFrame
            | Self::DestroySharedImage { .. } => Ok(()),
        }
    }
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
    #[serde(skip)]
    pixels_returned: Option<Arc<dyn Fn(Mailbox) + Send + Sync>>,
}

impl BeginFrameObserver {
    pub fn new(send: impl Fn(BeginFrameArgs) + Send + Sync + 'static) -> Self {
        Self {
            send: Some(Arc::new(send)),
            pixels_returned: None,
        }
    }

    /// Also told when the pixels of a shared image the client made have
    /// gone to the GPU, and their buffer is the client's again.
    #[must_use]
    pub fn with_pixels_returned(
        mut self,
        returned: impl Fn(Mailbox) + Send + Sync + 'static,
    ) -> Self {
        self.pixels_returned = Some(Arc::new(returned));
        self
    }

    /// The pixels `CreateSharedImage` brought for `mailbox` are on the GPU
    /// (or weren't taken): Viz holds their buffer no more.
    pub fn on_pixels_returned(&self, mailbox: Mailbox) {
        if let Some(returned) = &self.pixels_returned {
            returned(mailbox);
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
/// it speaks for no other. A renderer in a process of its own names no
/// sink at all: the browser's IO thread adds the id of the one it bound
/// that renderer's channel to.
#[derive(Clone)]
pub struct CompositorFrameSinkSender(Way);

#[derive(Clone)]
enum Way {
    /// A client in the browser's process: the message is moved.
    Local {
        frame_sink_id: FrameSinkId,
        viz: crossbeam_channel::Sender<VizMsg>,
    },
    /// A renderer in a process of its own: the message is written to its
    /// channel.
    Channel(ChannelLink),
}

impl CompositorFrameSinkSender {
    /// For the browser: `frame_sink_id`'s way to Viz, at `viz`.
    pub fn new(frame_sink_id: FrameSinkId, viz: crossbeam_channel::Sender<VizMsg>) -> Self {
        Self(Way::Local { frame_sink_id, viz })
    }

    /// For a renderer in a process of its own, whose channel `link` is.
    pub fn over(link: ChannelLink) -> Self {
        Self(Way::Channel(link))
    }

    /// A region of the channel's to draw `len` bytes of pixels in, for a
    /// `CreateSharedImage` sent by this sender: the pixels cross without
    /// being copied. `None` within a process (the client's own buffer is
    /// shared as it is), and when the channel has no region free: the
    /// pixels are then drawn in a buffer of the client's own, and sending
    /// them asks the browser for one more region, for the next time.
    pub fn take_region(&self, len: usize) -> Option<crate::SharedBufferWriter> {
        match &self.0 {
            Way::Local { .. } => None,
            Way::Channel(link) => link.take_region(len),
        }
    }

    /// Sends `msg` to Viz, from this sink. Fails once Viz has gone, or the
    /// way to it; the message is dropped.
    pub fn send(&self, msg: CompositorFrameSinkMsg) -> Result<(), VizGone> {
        match &self.0 {
            Way::Local { frame_sink_id, viz } => viz
                .send(VizMsg::FrameSink(*frame_sink_id, msg))
                .map_err(|_| VizGone),
            Way::Channel(link) => {
                send_over(link, to_browser::FRAME_SINK, &msg).map_err(|_| VizGone)
            }
        }
    }
}

impl std::fmt::Debug for CompositorFrameSinkSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Way::Local { frame_sink_id, .. } => f
                .debug_tuple("CompositorFrameSinkSender")
                .field(frame_sink_id)
                .finish(),
            Way::Channel(_) => f.write_str("CompositorFrameSinkSender(channel)"),
        }
    }
}

/// Viz has gone, and its queue with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VizGone;

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(shared_quad_state: usize, material: Material) -> DrawQuad {
        DrawQuad {
            rect: RectF::new(0.0, 0.0, 4.0, 4.0),
            shared_quad_state,
            material,
        }
    }

    fn solid() -> Material {
        Material::SolidColor {
            color: Color::WHITE,
        }
    }

    fn texture(id: u64) -> Material {
        Material::Texture {
            resource_id: ResourceId(id),
            tex_coord_rect: RectF::new(0.0, 0.0, 2.0, 2.0),
            nearest_neighbor: true,
        }
    }

    fn resource(
        id: u64,
        size: Size,
        bytes: usize,
        shared_image: Option<u64>,
    ) -> TransferableResource {
        TransferableResource {
            id: ResourceId(id),
            size,
            format: ResourceFormat::Rgba8Premul,
            pixels: BigBuffer::from(vec![0; bytes]),
            shared_image: shared_image.map(|m| SharedImage::from_mailbox(Mailbox(m))),
        }
    }

    fn pass(states: usize, quads: Vec<DrawQuad>) -> CompositorRenderPass {
        let area = RectF::new(0.0, 0.0, 8.0, 8.0);
        CompositorRenderPass {
            id: RenderPassId(1),
            output_rect: area,
            damage_rect: area,
            shared_quad_state_list: vec![SharedQuadState::default(); states],
            quad_list: quads,
        }
    }

    /// What a page's compositor sends: tiles of shared images and of
    /// pixels, a solid one, and a clipped layer with no tile drawn (its
    /// state is there all the same).
    fn page() -> CompositorFrame {
        CompositorFrame {
            metadata: CompositorFrameMetadata {
                device_scale_factor: 2.0,
                frame_token: 1,
                root_background_color: Color::WHITE,
            },
            resource_list: vec![
                resource(1, Size::new(2, 2), 0, Some(9)),
                resource(2, Size::new(2, 2), 16, None),
                // An empty layer's snapshot.
                resource(3, Size::new(0, 3), 0, None),
            ],
            render_pass_list: vec![pass(
                3,
                vec![
                    quad(0, texture(1)),
                    quad(1, texture(2)),
                    quad(1, texture(3)),
                    quad(0, solid()),
                ],
            )],
        }
    }

    #[track_caller]
    fn refused(frame: &CompositorFrame, why: InvalidFrameSinkMsg) {
        assert_eq!(frame.check(false), Err(why));
        assert_eq!(frame.check(true), Err(why));
    }

    #[test]
    fn a_pages_frame_can_be_right() {
        assert_eq!(page().check(false), Ok(()));
    }

    /// The root pass's output is the frame's size: one without a pass,
    /// or whose root draws no rect, can't be.
    #[test]
    fn a_frame_draws_somewhere() {
        let mut frame = page();
        frame.render_pass_list.clear();
        refused(&frame, InvalidFrameSinkMsg::NoRenderPass);
        for (width, height) in [
            (0.0, 8.0),
            (8.0, 0.0),
            (-1.0, 8.0),
            (f32::NAN, 8.0),
            (8.0, f32::NAN),
        ] {
            let mut frame = page();
            frame.render_pass_list[0].output_rect = RectF::new(0.0, 0.0, width, height);
            refused(&frame, InvalidFrameSinkMsg::EmptyOutput);
        }
        // Only the root's counts: Viz draws only it.
        let mut frame = page();
        frame.render_pass_list.insert(0, pass(1, Vec::new()));
        frame.render_pass_list[0].output_rect = RectF::default();
        assert_eq!(frame.check(false), Ok(()));
    }

    #[test]
    fn a_frame_has_so_many_passes_quads_and_states_at_most() {
        let mut frame = page();
        frame.render_pass_list = vec![pass(1, Vec::new()); MAX_RENDER_PASSES];
        assert_eq!(frame.check(false), Ok(()));
        frame.render_pass_list.push(pass(1, Vec::new()));
        refused(&frame, InvalidFrameSinkMsg::TooManyRenderPasses);

        // Quads of every pass, together.
        let mut frame = page();
        let others = MAX_QUADS - frame.render_pass_list[0].quad_list.len();
        frame
            .render_pass_list
            .insert(0, pass(1, vec![quad(0, solid()); others]));
        assert_eq!(frame.check(false), Ok(()));
        frame.render_pass_list[1].quad_list.push(quad(0, solid()));
        refused(&frame, InvalidFrameSinkMsg::TooManyQuads);

        // States too, though no quad is of them.
        let mut frame = page();
        let others = MAX_QUADS + 1 - frame.render_pass_list[0].shared_quad_state_list.len();
        frame.render_pass_list.insert(0, pass(others, Vec::new()));
        assert_eq!(frame.check(false), Ok(()));
        frame.render_pass_list[0]
            .shared_quad_state_list
            .push(SharedQuadState::default());
        refused(&frame, InvalidFrameSinkMsg::TooManySharedQuadStates);
    }

    /// A quad's state and resource are its frame's: one of none it has
    /// can't be right (they used to draw with the default state, and not
    /// at all).
    #[test]
    fn a_quad_is_of_its_frames_state_and_resource() {
        let mut frame = page();
        frame.render_pass_list[0].quad_list[1].shared_quad_state = 3;
        refused(&frame, InvalidFrameSinkMsg::SharedQuadStateOutOfRange);
        frame.render_pass_list[0].quad_list[1].shared_quad_state = usize::MAX;
        refused(&frame, InvalidFrameSinkMsg::SharedQuadStateOutOfRange);
        // Each pass's own states.
        let mut frame = page();
        frame
            .render_pass_list
            .insert(0, pass(1, vec![quad(2, solid())]));
        refused(&frame, InvalidFrameSinkMsg::SharedQuadStateOutOfRange);

        let mut frame = page();
        frame.render_pass_list[0].quad_list[0].material = texture(4);
        refused(&frame, InvalidFrameSinkMsg::UnknownResource);

        let mut frame = page();
        frame
            .resource_list
            .push(resource(2, Size::new(1, 1), 4, None));
        refused(&frame, InvalidFrameSinkMsg::DuplicateResource);
    }

    #[test]
    fn a_resources_pixels_are_those_of_its_size() {
        let wide = (MAX_TEXTURE_SIZE as usize + 1) * 4;
        for bad in [
            resource(2, Size::new(2, 2), 15, None),
            resource(2, Size::new(2, 2), 17, None),
            resource(2, Size::new(MAX_TEXTURE_SIZE + 1, 1), wide, None),
            resource(2, Size::new(1, MAX_TEXTURE_SIZE + 1), wide, None),
            // A shared image's are on the GPU: none come with it.
            resource(1, Size::new(2, 2), 16, Some(9)),
            resource(1, Size::new(MAX_TEXTURE_SIZE + 1, 1), 0, Some(9)),
            resource(1, Size::new(1, MAX_TEXTURE_SIZE + 1), 0, Some(9)),
        ] {
            let mut frame = page();
            let at = frame
                .resource_list
                .iter()
                .position(|r| r.id == bad.id)
                .unwrap();
            frame.resource_list[at] = bad;
            refused(&frame, InvalidFrameSinkMsg::ResourcePixels);
        }
        let mut frame = page();
        frame.resource_list[1] = resource(
            2,
            Size::new(MAX_TEXTURE_SIZE, 1),
            MAX_TEXTURE_SIZE as usize * 4,
            None,
        );
        frame.resource_list[0] =
            resource(1, Size::new(MAX_TEXTURE_SIZE, MAX_TEXTURE_SIZE), 0, Some(9));
        assert_eq!(frame.check(false), Ok(()));
    }

    /// Only a client that may embed others' surfaces (the browser's UI)
    /// sends a surface's quad: a renderer's compositor embeds none.
    #[test]
    fn only_a_client_that_may_embed_embeds() {
        let mut frame = page();
        frame.render_pass_list[0].quad_list.push(quad(
            0,
            Material::Surface {
                surface_id: SurfaceId {
                    frame_sink_id: FrameSinkId {
                        client_id: 2,
                        sink_id: 1,
                    },
                    local_surface_id: LocalSurfaceId::default(),
                },
                default_background_color: Color::WHITE,
            },
        ));
        assert_eq!(frame.check(false), Err(InvalidFrameSinkMsg::EmbedsSurface));
        assert_eq!(frame.check(true), Ok(()));
    }

    /// Coordinates aren't checked: NaN and infinity draw nothing, and
    /// wgpu takes them.
    #[test]
    fn odd_coordinates_can_be_right() {
        for odd in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
            let mut frame = page();
            let pass = &mut frame.render_pass_list[0];
            pass.quad_list[0].rect = RectF::new(odd, odd, odd, odd);
            pass.shared_quad_state_list[1].clip_rect = Some(RectF::new(odd, 0.0, odd, 1.0));
            pass.shared_quad_state_list[1].opacity = odd;
            pass.damage_rect = RectF::new(odd, odd, odd, odd);
            frame.metadata.device_scale_factor = odd;
            assert_eq!(frame.check(false), Ok(()));
        }
    }

    #[test]
    fn a_shared_images_pixels_are_those_of_its_size() {
        let wide = (MAX_TEXTURE_SIZE as usize + 1) * 4;
        for (size, bytes) in [
            (Size::new(0, 0), 0),
            (Size::new(0, 4), 0),
            (Size::new(2, 2), 15),
            (Size::new(2, 2), 17),
            (Size::new(MAX_TEXTURE_SIZE + 1, 1), wide),
            (Size::new(1, MAX_TEXTURE_SIZE + 1), wide),
        ] {
            assert_eq!(
                check_shared_image(size, bytes),
                Err(InvalidFrameSinkMsg::SharedImagePixels),
                "{size:?} {bytes}"
            );
        }
        assert_eq!(check_shared_image(Size::new(2, 2), 16), Ok(()));
        let widest = MAX_TEXTURE_SIZE as usize * 4;
        assert_eq!(
            check_shared_image(Size::new(MAX_TEXTURE_SIZE, 1), widest),
            Ok(())
        );
    }

    /// A message checks what it carries: a frame, or nothing.
    #[test]
    fn a_message_is_checked_for_what_it_carries() {
        for fine in [
            CompositorFrameSinkMsg::SetNeedsBeginFrame(true),
            CompositorFrameSinkMsg::DidNotProduceFrame,
            CompositorFrameSinkMsg::DestroySharedImage {
                mailbox: Mailbox(1),
            },
        ] {
            assert_eq!(fine.check(false), Ok(()));
        }
        let submitted = |frame| CompositorFrameSinkMsg::SubmitCompositorFrame {
            local_surface_id: LocalSurfaceId::default(),
            frame,
        };
        assert_eq!(submitted(page()).check(false), Ok(()));
        let none = CompositorFrame {
            render_pass_list: Vec::new(),
            ..page()
        };
        assert_eq!(
            submitted(none).check(true),
            Err(InvalidFrameSinkMsg::NoRenderPass)
        );
    }

    /// What a client sends goes as its own sink's: it names no other.
    #[test]
    fn a_frame_sink_sender_speaks_for_its_sink() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let sink = FrameSinkId {
            client_id: 3,
            sink_id: 1,
        };
        let sender = CompositorFrameSinkSender::new(sink, tx);
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
