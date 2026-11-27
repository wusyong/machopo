//! GPU `DirectRenderer` on wgpu. Plays the role of Chromium's `SkiaRenderer`
//! plus its output device: every `DrawQuad` becomes a textured quad drawn
//! into a window's swap chain image (`SkiaOutputDevice`), or, with no
//! window (`--headless`, tests), into a texture of its own
//! that frames are read back from (`SkiaOutputDeviceOffscreen`).
//!
//! It draws with the process's one device ([`GpuContext`]). A resource is
//! either a shared image a client made on that device
//! ([`GpuSharedImages`]), drawn as it is, or CPU pixels (a client with no
//! shared image interface, such as one made before the device was),
//! uploaded once per `ResourceId`. Resources a frame no longer references
//! are let go; a shared image lives on while its client holds it.
//!
//! When the device is lost (a driver reset, or `about:gpucrash`) the
//! renderer moves to the next one, as Chromium restarts its GPU process,
//! and tells the clients, which make their shared images again; until they
//! do, what was on the old device isn't drawn.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};
use std::time::{Duration, Instant};

use base::numerics::saturated_cast;
use gfx::{RectF, Size};
use protocol::viz::{
    ContextLostObserver, ContextProvider, CopyOutputResult, Material, ResourceId,
    TransferableResource,
};

use crate::display::{CopyFailure, DirectRenderer, Pixels, SharedImageHandle};
use crate::gpu_context::GpuContext;
use crate::surface_aggregator::AggregatedFrame;

/// The quads' shader; `rounded_corner::ROUNDED_CLIP_WGSL` is added to it.
pub(crate) const SHADER: &str = r#"
struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) clip: vec4<f32>,
    @location(3) @interpolate(flat) clip_rx: vec4<f32>,
    @location(4) @interpolate(flat) clip_ry: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) clip: vec4<f32>,
    @location(4) clip_rx: vec4<f32>,
    @location(5) clip_ry: vec4<f32>,
) -> VsOut {
    var out: VsOut;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    out.clip = clip;
    out.clip_rx = clip_rx;
    out.clip_ry = clip_ry;
    return out;
}

@group(0) @binding(0) var quad_texture: texture_2d<f32>;
@group(0) @binding(1) var quad_sampler: sampler;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Texture and color are both premultiplied; a rounded clip takes off
    // what is outside its curves.
    let coverage = rounded_clip_coverage(in.position.xy, in.clip, in.clip_rx, in.clip_ry);
    return textureSample(quad_texture, quad_sampler, in.uv) * in.color * coverage;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    /// The quad's rounded clip, in the output's pixels: its rect as (x0,
    /// y0, x1, y1), none when `x1 < x0`, and its corners' radii.
    clip: [f32; 4],
    clip_rx: [f32; 4],
    clip_ry: [f32; 4],
}

#[derive(Clone)]
pub(crate) struct UploadedResource {
    pub(crate) size: Size,
    /// Sampled at the nearest texel (a tile), or bilinearly (a canvas's
    /// layer, `nearest_neighbor: false`).
    pub(crate) bind_group: wgpu::BindGroup,
    /// Made when first drawn so: few are (a tile never is), and making
    /// one costs the thread that rasters as much as the other did.
    pub(crate) linear_bind_group: Arc<OnceLock<wgpu::BindGroup>>,
    pub(crate) view: wgpu::TextureView,
    /// Kept for the bind groups, and written over when it is used again.
    pub(crate) texture: wgpu::Texture,
}

/// What a shared image holds: its texture, and the device it is on.
struct SharedImageBacking {
    generation: u64,
    uploaded: UploadedResource,
    /// Where the texture goes when the shared image is let go: its
    /// device's pool, while there is that device.
    pool: Weak<crate::gpu_context::TexturePool>,
}

/// The last of a shared image is gone: its client's (a tile rastered
/// again, or far from the viewport) and the frames' that drew it. A
/// renderer may still hold the resource for a frame it has drawn, which it
/// forgets at its next; it draws only what a frame holds, so the texture
/// is free to be another shared image's. What was drawn from it and is
/// still on its way to the screen was submitted before the write that
/// follows, and the queue keeps their order.
impl Drop for SharedImageBacking {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.upgrade() {
            pool.put(self.uploaded.clone());
        }
    }
}

/// How many times, and after how long, a lost device is made again before
/// giving up: there is no software output to fall back to (Chromium's
/// desktop GPU process, crashing 3 times, falls back to software
/// compositing).
const RECREATE_WAITS: [Duration; 3] = [
    Duration::ZERO,
    Duration::from_millis(500),
    Duration::from_secs(2),
];

/// The GPU side of [`SharedImageInterface`] (Chromium's `SharedImageStub`
/// over the `SharedImageManager`): clients put their tiles on the
/// renderer's device from their own thread, as Chromium rasters into GPU
/// memory, and keep no pixels. While the renderer has no device (not yet,
/// or a lost one not made again yet), it makes none, and clients keep
/// theirs.
#[derive(Default)]
pub struct GpuSharedImages {
    gpu: RwLock<Option<Arc<GpuContext>>>,
    observers: Mutex<Vec<Weak<dyn ContextLostObserver>>>,
}

impl GpuSharedImages {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn set_gpu(&self, gpu: Option<Arc<GpuContext>>) {
        *self.gpu.write().unwrap_or_else(|e| e.into_inner()) = gpu;
    }

    /// Tells every client still there that the device was lost.
    fn context_lost(&self) {
        let observers: Vec<_> = {
            let observers = self.observers.lock().unwrap_or_else(|e| e.into_inner());
            observers.iter().filter_map(Weak::upgrade).collect()
        };
        for observer in observers {
            observer.on_context_lost();
        }
    }
}

impl GpuSharedImages {
    /// `pixels` on the device as a shared image's texture, if there is a
    /// device and they are a texture's of `size` it can make.
    fn upload(&self, size: Size, pixels: Pixels<'_>) -> Option<SharedImageBacking> {
        let gpu = self.gpu.read().unwrap_or_else(|e| e.into_inner()).clone()?;
        // Not one Viz can make (the GPU process checks a client's too).
        if size.is_empty() || !protocol::viz::pixels_fit(size, pixels.len()) {
            return None;
        }
        let _submitting = gpu.submit_lock.lock().unwrap_or_else(|e| e.into_inner());
        let write = trace_event::trace_event!("SharedImage::Write");
        let (uploaded, error) =
            gpu.scoped("a shared image's upload", || match gpu.pool.take(size) {
                Some(texture) => {
                    gpu.write(&texture, pixels);
                    texture
                }
                None => gpu.upload(size, pixels),
            });
        drop(write);
        // As one never made: a frame that draws it draws nothing of it.
        if error.is_some() {
            return None;
        }
        Some(SharedImageBacking {
            generation: gpu.generation,
            uploaded,
            pool: Arc::downgrade(&gpu.pool),
        })
    }
}

impl GpuSharedImages {
    /// `pixels` on the device as a shared image, as a renderer keeps one
    /// for its `Display`.
    pub fn create_shared_image<'a>(
        &self,
        size: Size,
        pixels: impl Into<Pixels<'a>>,
    ) -> Option<SharedImageHandle> {
        let backing: SharedImageHandle = Arc::new(self.upload(size, pixels.into())?);
        Some(backing)
    }

    /// Sends the GPU what the shared images made since the last call were
    /// made from: after those there are to make for now, not each (until
    /// then their pixels are held as copies waiting to go).
    pub fn flush(&self) {
        let Some(gpu) = self.gpu.read().unwrap_or_else(|e| e.into_inner()).clone() else {
            return;
        };
        // The writes go out at the next submit, which the renderer doesn't
        // make while it can't draw (an occluded window): send them now, so
        // they don't pile up holding their staging copies.
        let _submitting = gpu.submit_lock.lock().unwrap_or_else(|e| e.into_inner());
        let _submit = trace_event::trace_event!("SharedImage::Submit");
        // What went wrong is handled as it is caught: nothing else of it.
        let ((), _) = gpu.scoped("the shared images' copies", || {
            gpu.queue.submit(gpu.take_staged());
        });
    }
}

impl ContextProvider for GpuSharedImages {
    fn add_context_lost_observer(&self, observer: Weak<dyn ContextLostObserver>) {
        let mut observers = self.observers.lock().unwrap_or_else(|e| e.into_inner());
        observers.retain(|o| o.strong_count() > 0);
        observers.push(observer);
    }
}

/// One draw call's worth of state.
struct DrawCmd {
    resource: Option<ResourceId>,
    /// Sampled bilinearly rather than at the nearest texel.
    linear: bool,
    scissor: Option<(u32, u32, u32, u32)>,
    /// Which of the frame's vertex buffers, and where in it.
    buffer: usize,
    first_vertex: u32,
}

/// The most vertices in one buffer: the most quads a client's frame has,
/// six each (120 MiB, under the 256 MiB a buffer may have on any device
/// Viz takes). A frame aggregated from several clients' is drawn from
/// several.
const VERTICES_PER_BUFFER: usize = protocol::viz::MAX_QUADS * 6;

/// Makes a window's surface again, on the process's instance.
pub type MakeSurface = Box<dyn Fn() -> Result<wgpu::Surface<'static>, String> + Send>;

/// What a renderer draws into.
enum Output {
    /// A window's swap chain.
    Surface {
        surface: wgpu::Surface<'static>,
        /// For a new device that can't take this surface's swap chain.
        make_surface: MakeSurface,
        config: wgpu::SurfaceConfiguration,
        /// The surface's textures can be copied from, so frames can be
        /// read back (asked for, and the surface allows it).
        readback: bool,
        needs_reconfigure: bool,
        /// Frames skipped in a row because the surface had no texture to
        /// draw into (the window occluded, a timeout...): logged when it
        /// starts and when it ends, not for each.
        skipped: u32,
    },
    /// A texture of its own, which copies are always read back from.
    Offscreen { texture: wgpu::Texture },
}

/// The format an offscreen output is drawn in, and read back as.
const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl Output {
    fn offscreen(context: &GpuContext, size: Size) -> Self {
        Output::Offscreen {
            texture: context.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("viz offscreen output"),
                size: wgpu::Extent3d {
                    width: size.width.max(1),
                    height: size.height.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: OFFSCREEN_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            }),
        }
    }

    /// `surface`, configured for `context`'s device at `size`, so its
    /// textures can be read back too if `readback` and the surface allows.
    fn surface(
        context: &GpuContext,
        surface: wgpu::Surface<'static>,
        make_surface: MakeSurface,
        size: Size,
        readback: bool,
    ) -> Result<Self, String> {
        let caps = surface.get_capabilities(&context.adapter);
        let format = swap_chain_format(&caps.formats).ok_or("surface reports no formats")?;
        let mut config = surface
            .get_default_config(&context.adapter, size.width.max(1), size.height.max(1))
            .ok_or("surface is not supported by the adapter")?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        // Only when asked: copying from the surface costs some drivers an
        // optimization (Metal's `framebufferOnly`).
        let readback = readback && {
            // Four bytes a pixel: what `Readback` copies out.
            let supported = caps.usages.contains(wgpu::TextureUsages::COPY_SRC)
                && format.block_copy_size(None) == Some(4);
            if !supported {
                tracing::error!(
                    usages = ?caps.usages,
                    ?format,
                    "viz: the surface can't be read back"
                );
            }
            supported
        };
        if readback {
            config.usage |= wgpu::TextureUsages::COPY_SRC;
        }
        {
            let _no_submits = context
                .submit_lock
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            surface.configure(&context.device, &config);
        }
        Ok(Output::Surface {
            surface,
            make_surface,
            config,
            readback,
            needs_reconfigure: false,
            skipped: 0,
        })
    }

    fn size(&self) -> Size {
        match self {
            Output::Surface { config, .. } => Size::new(config.width, config.height),
            Output::Offscreen { texture } => Size::new(texture.width(), texture.height()),
        }
    }

    /// Frames can be read back from it.
    fn readback(&self) -> bool {
        match self {
            Output::Surface { readback, .. } => *readback,
            Output::Offscreen { .. } => true,
        }
    }
}

pub struct WgpuRenderer {
    /// `None` only while a lost device's next one is being made, or once
    /// it has failed to be.
    output: Option<Output>,
    shared_images: Arc<GpuSharedImages>,
    context: Arc<GpuContext>,
    resources: HashMap<ResourceId, UploadedResource>,
    /// Told, once, that a lost device's next one can't draw
    /// ([`on_failure`](Self::on_failure)).
    on_failure: Option<Box<dyn FnOnce(String) + Send>>,
    /// That has happened: nothing is drawn any more.
    failed: bool,
}

impl WgpuRenderer {
    /// Draws into a window's `surface` (made on the process's instance,
    /// `gpu_context::instance_with`); `make_surface` makes another for the
    /// window, when a lost device's next one can't take this one. Must be
    /// called on the thread that will draw (the Viz thread); blocks while
    /// the process's device is made, if it isn't yet. Clients' shared
    /// images go on the device, through `shared_images`. With `readback`,
    /// what it draws can be copied (`CopyOutputRequest`s), if the surface
    /// allows.
    pub fn new(
        surface: wgpu::Surface<'static>,
        make_surface: MakeSurface,
        size: Size,
        shared_images: Arc<GpuSharedImages>,
        readback: bool,
    ) -> Result<Self, String> {
        let context = GpuContext::shared(Some(&surface))?;
        let output = Output::surface(&context, surface, make_surface, size, readback)?;
        Ok(Self::with_output(output, context, shared_images))
    }

    /// Draws into a texture of its own, `size` physical pixels, which every
    /// copy request reads back from. As [`Self::new`] otherwise.
    pub fn offscreen(size: Size, shared_images: Arc<GpuSharedImages>) -> Result<Self, String> {
        let context = GpuContext::shared(None)?;
        let output = Output::offscreen(&context, size);
        Ok(Self::with_output(output, context, shared_images))
    }

    fn with_output(
        output: Output,
        context: Arc<GpuContext>,
        shared_images: Arc<GpuSharedImages>,
    ) -> Self {
        let renderer = Self {
            output: Some(output),
            shared_images,
            context,
            resources: HashMap::new(),
            on_failure: None,
            failed: false,
        };
        renderer.share_device();
        renderer
    }

    /// What is told, once, if the device is lost and no next one can draw
    /// (none can be made, or the window's surface can't take it): nothing
    /// is drawn after. Chromium's GPU process ends itself so, and the
    /// browser decides what next; Viz here is a thread of the browser's,
    /// which is to end as closing it does, its profile and trace written.
    #[must_use]
    pub fn on_failure(mut self, told: impl FnOnce(String) + Send + 'static) -> Self {
        self.on_failure = Some(Box::new(told));
        self
    }

    fn output(&self) -> &Output {
        self.output.as_ref().expect("an output")
    }

    /// The size it draws, in physical pixels (none once it has failed
    /// without one).
    pub fn size(&self) -> Size {
        self.output.as_ref().map_or(Size::default(), Output::size)
    }

    /// Draws no more, and says why.
    fn fail(&mut self, error: String) {
        tracing::error!(%error, "viz: can't draw any more");
        self.failed = true;
        if let Some(told) = self.on_failure.take() {
            told(error);
        }
    }

    /// The adapter it draws with (for logs and reports).
    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.context.adapter.get_info()
    }

    /// Lets clients put shared images on the device.
    fn share_device(&self) {
        self.shared_images.set_gpu(Some(self.context.clone()));
    }

    /// The device was lost: moves to the next one (made now, unless
    /// another renderer has), and tells the clients, whose shared images
    /// were on the old one. An error if it can't: there is nothing to draw
    /// with.
    fn recreate_context(&mut self) -> Result<(), String> {
        self.shared_images.set_gpu(None);
        self.resources.clear();
        let size = self.output().size();
        // A Direct3D 12 swap chain belongs to its device, and wgpu would
        // resize the old one for the new device, which fails: the old
        // surface goes first, taking its swap chain with it, and the window
        // gets a new one (as Chromium's GPU process, restarted, makes new
        // swap chains). Vulkan's and Metal's surfaces take a new device.
        if self.context.adapter.get_info().backend == wgpu::Backend::Dx12
            && let Some(Output::Surface {
                surface,
                make_surface,
                ..
            }) = &mut self.output
        {
            let new =
                make_surface().map_err(|err| format!("no new surface for the window: {err}"))?;
            // The old one goes before the new one is configured: a window
            // has one swap chain.
            drop(std::mem::replace(surface, new));
        }
        for (attempt, wait) in RECREATE_WAITS.into_iter().enumerate() {
            std::thread::sleep(wait);
            let surface = match self.output() {
                Output::Surface { surface, .. } => Some(surface),
                Output::Offscreen { .. } => None,
            };
            let context = match GpuContext::replace_lost(&self.context, surface) {
                Ok(context) => context,
                Err(err) => {
                    tracing::warn!(attempt, %err, "viz: no new GPU device");
                    continue;
                }
            };
            // The old device goes first: what is still on it (a swap
            // chain's buffers, say) keeps its swap chain alive, and a window
            // takes no other while it is.
            self.context = context;
            let output = match self.output.take().expect("an output") {
                Output::Surface {
                    surface,
                    make_surface,
                    readback,
                    ..
                } => Output::surface(&self.context, surface, make_surface, size, readback),
                Output::Offscreen { .. } => Ok(Output::offscreen(&self.context, size)),
            };
            match output {
                Ok(output) => {
                    tracing::info!(
                        generation = self.context.generation,
                        "viz: moved to a new GPU device"
                    );
                    self.output = Some(output);
                    self.share_device();
                    self.shared_images.context_lost();
                    return Ok(());
                }
                // The surface went with the failed configuration.
                Err(err) => {
                    return Err(format!(
                        "the new GPU device can't draw to the window: {err}"
                    ));
                }
            }
        }
        Err(format!(
            "the GPU device was lost and no new one could be made in {} attempts",
            RECREATE_WAITS.len()
        ))
    }

    fn sync_resources(&mut self, frame: &AggregatedFrame<'_>) {
        self.resources
            .retain(|id, _| frame.resources.contains_key(id));
        for (id, res) in &frame.resources {
            if self.resources.contains_key(id) {
                continue;
            }
            // A shared image: what the frame holds of Viz's table, if its
            // client made it. One made on a lost device isn't drawn: its
            // client is making it again on this one.
            if res.shared_image.is_some() {
                if let Some(backing) = frame
                    .images
                    .get(id)
                    .and_then(|image| image.downcast_ref::<SharedImageBacking>())
                    && backing.generation == self.context.generation
                {
                    self.resources.insert(*id, backing.uploaded.clone());
                }
                continue;
            }
            let TransferableResource { size, pixels, .. } = res;
            if size.is_empty() {
                continue;
            }
            let uploaded = self.context.upload(*size, &pixels[..]);
            self.resources.insert(*id, uploaded);
        }
    }

    fn reconfigure(&self) {
        if let Some(Output::Surface {
            surface, config, ..
        }) = &self.output
        {
            let _no_submits = self
                .context
                .submit_lock
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            surface.configure(&self.context.device, config);
        }
    }

    /// Target pixels -> clip space for an output of `size`.
    fn to_ndc(size: Size, x: f32, y: f32) -> [f32; 2] {
        [
            x / size.width as f32 * 2.0 - 1.0,
            1.0 - y / size.height as f32 * 2.0,
        ]
    }

    fn scissor(size: Size, clip: Option<RectF>) -> Option<(u32, u32, u32, u32)> {
        let c = clip?.intersect(&RectF::new(0.0, 0.0, size.width as f32, size.height as f32));
        let x = saturated_cast(c.x.max(0.0));
        let y = saturated_cast(c.y.max(0.0));
        Some((
            x,
            y,
            saturated_cast(c.width.max(0.0)),
            saturated_cast(c.height.max(0.0)),
        ))
    }
}

impl DirectRenderer for WgpuRenderer {
    fn resize(&mut self, size: Size) {
        if size.is_empty() || self.failed {
            return; // minimised; keep the old configuration
        }
        match self.output.as_mut().expect("an output") {
            Output::Surface { config, .. } => {
                config.width = size.width;
                config.height = size.height;
                // A lost device's next one is configured for it.
                if !self.context.is_lost() {
                    self.reconfigure();
                }
            }
            Output::Offscreen { texture } => {
                if Size::new(texture.width(), texture.height()) != size {
                    self.output = Some(Output::offscreen(&self.context, size));
                }
            }
        }
    }

    fn create_shared_image(&mut self, size: Size, pixels: Pixels<'_>) -> Option<SharedImageHandle> {
        self.shared_images.create_shared_image(size, pixels)
    }

    fn flush_shared_images(&mut self) {
        self.shared_images.flush();
    }

    fn lose_context(&mut self) {
        tracing::warn!("viz: losing the GPU device on request");
        self.context.lose();
    }

    /// Clients put no more shared images on the device, the GPU finishes
    /// what this renderer gave it, and the device is no longer the
    /// process's: it goes when this renderer does, unless another holds it.
    fn shutdown(&mut self) {
        self.shared_images.set_gpu(None);
        self.context.finish();
        GpuContext::release_shared(&self.context);
    }

    fn draw_frame(&mut self, frame: &AggregatedFrame<'_>) {
        let _ = self.draw(frame, false);
    }

    fn draw_frame_and_copy(
        &mut self,
        frame: &AggregatedFrame<'_>,
    ) -> Result<CopyOutputResult, CopyFailure> {
        if self.failed {
            return Err(CopyFailure::Unsupported);
        }
        // The device may be made again by the draw, and its output made as
        // it was.
        if !self.output().readback() {
            self.draw_frame(frame);
            return Err(CopyFailure::Unsupported);
        }
        // The device lost while the copy was made (by another renderer
        // too: they share it) fails the copy, not the request: the next
        // draw moves to the new device and copies again, as Chromium's
        // copy requests are made again after its GPU process restarts.
        for _ in 0..RECREATE_WAITS.len() {
            match self.draw(frame, true)? {
                Some(copy) => return Ok(copy),
                None if self.context.is_lost() => continue,
                None => break,
            }
        }
        Err(CopyFailure::Unsupported)
    }
}

/// Where a frame is drawn: a surface's texture (presented after), or the
/// offscreen output's.
enum Target {
    Surface(wgpu::SurfaceTexture),
    Offscreen(wgpu::Texture),
}

impl Target {
    fn texture(&self) -> &wgpu::Texture {
        match self {
            Target::Surface(t) => &t.texture,
            Target::Offscreen(t) => t,
        }
    }
}

impl WgpuRenderer {
    /// The texture to draw the next frame into. `NotDrawn` if a surface has
    /// none: the window hidden or minimised (macOS says so at once), a
    /// timeout...
    fn target(&mut self) -> Result<Target, CopyFailure> {
        let Some(Output::Surface {
            surface,
            needs_reconfigure,
            skipped,
            ..
        }) = &mut self.output
        else {
            let Some(Output::Offscreen { texture }) = &self.output else {
                unreachable!("an output")
            };
            return Ok(Target::Offscreen(texture.clone()));
        };
        // Getting one may wait for one to come free (the display's
        // refresh); logged when that is long.
        let asked = Instant::now();
        let current = surface.get_current_texture();
        let waited = asked.elapsed();
        if waited > Duration::from_millis(100) {
            tracing::debug!(?waited, "viz: waited for the surface's next texture");
        }
        let status = surface_status(&current);
        let mut skip = || {
            if *skipped == 0 {
                tracing::info!(status, "viz: can't draw, skipping frames until it can");
            }
            // Only told apart from 0, and logged: saturating keeps that.
            *skipped = skipped.saturating_add(1);
        };
        let texture = match current {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                // Draw this one, reconfigure for the next.
                tracing::debug!("viz: suboptimal surface");
                *needs_reconfigure = true;
                t
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                skip();
                self.reconfigure();
                return Err(CopyFailure::NotDrawn);
            }
            _ => {
                skip();
                return Err(CopyFailure::NotDrawn);
            }
        };
        if *skipped > 0 {
            tracing::info!(skipped = *skipped, "viz: drawing again");
            *skipped = 0;
        }
        Ok(Target::Surface(texture))
    }

    /// Draws `frame` (and presents it, to a surface); first copies it, if
    /// `copy` and the output can be copied from. `NotDrawn` if there was
    /// nothing to draw into.
    fn draw(
        &mut self,
        frame: &AggregatedFrame<'_>,
        copy: bool,
    ) -> Result<Option<CopyOutputResult>, CopyFailure> {
        if self.failed {
            return Ok(None);
        }
        if self.context.is_lost()
            && let Err(error) = self.recreate_context()
        {
            self.fail(error);
            return Ok(None);
        }
        if let Some(Output::Surface {
            needs_reconfigure, ..
        }) = &mut self.output
            && std::mem::take(needs_reconfigure)
        {
            self.reconfigure();
        }
        // A texture to draw into first: without one the frame is skipped
        // whole, and nothing is uploaded. Uploads wait in wgpu until the
        // next submit, which a skipped frame never makes: uploading a new
        // frame's tiles at every BeginFrame while the window is hidden
        // piles them up, gigabytes a minute with an animation running. The
        // next frame drawn uploads what it needs.
        let target = self.target()?;
        // What wgpu reports of the frame is caught before it is presented:
        // one it reported an error of isn't (the last stays on the screen),
        // nor copied.
        let context = self.context.clone();
        let (readback, error) = context.scoped("a frame", || self.record(frame, &target, copy));
        if error.is_some() {
            return Ok(None);
        }
        if let Target::Surface(surface_texture) = target {
            self.context.queue.present(surface_texture);
        }
        Ok(readback.and_then(|readback| readback.read(&self.context.device)))
    }

    /// Uploads what `frame` needs, draws it into `target` and submits it,
    /// with a copy if `copy` and the output can be copied from. Nothing is
    /// submitted if its vertices have no buffer.
    fn record(
        &mut self,
        frame: &AggregatedFrame<'_>,
        target: &Target,
        copy: bool,
    ) -> Option<Readback> {
        self.sync_resources(frame);
        let size = self.output().size();

        // Build vertices back-to-front (quad lists are front-to-back), in
        // buffers of VERTICES_PER_BUFFER at most.
        let mut vertices: Vec<Vec<Vertex>> = vec![Vec::new()];
        let mut cmds: Vec<DrawCmd> = Vec::new();
        if let Some(root) = frame.render_pass_list.last() {
            for quad in root.quad_list.iter().rev() {
                let sqs = root
                    .shared_quad_state_list
                    .get(quad.shared_quad_state)
                    .copied()
                    .unwrap_or_default();
                let linear = matches!(
                    quad.material,
                    Material::Texture {
                        nearest_neighbor: false,
                        ..
                    }
                );
                let (resource, uv, color) = match quad.material {
                    Material::SolidColor { color } => {
                        let c = color.premultiplied_f32().map(|v| v * sqs.opacity);
                        (None, RectF::new(0.0, 0.0, 1.0, 1.0), c)
                    }
                    Material::Texture {
                        resource_id,
                        tex_coord_rect: t,
                        ..
                    } => {
                        let Some(res) = self.resources.get(&resource_id) else {
                            continue;
                        };
                        let (w, h) = (res.size.width as f32, res.size.height as f32);
                        (
                            Some(resource_id),
                            RectF::new(t.x / w, t.y / h, t.width / w, t.height / h),
                            [sqs.opacity; 4],
                        )
                    }
                    // The aggregator has replaced it with the surface's quads.
                    Material::Surface { .. } => continue,
                };
                let r = quad.rect;
                let corners = [
                    (r.x, r.y, uv.x, uv.y),
                    (r.right(), r.y, uv.right(), uv.y),
                    (r.x, r.bottom(), uv.x, uv.bottom()),
                    (r.right(), r.bottom(), uv.right(), uv.bottom()),
                ];
                if vertices
                    .last()
                    .is_none_or(|v| v.len() >= VERTICES_PER_BUFFER)
                {
                    vertices.push(Vec::with_capacity(6));
                }
                let buffer = vertices.len().saturating_sub(1);
                let Some(vertices) = vertices.last_mut() else {
                    continue;
                };
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "fewer than VERTICES_PER_BUFFER (1.5 million) in a buffer"
                )]
                let first_vertex = vertices.len() as u32;
                let (clip, clip_rx, clip_ry) = match sqs.rounded_clip {
                    Some(rr) => (
                        [rr.rect.x, rr.rect.y, rr.rect.right(), rr.rect.bottom()],
                        rr.radii.map(|r| r.width),
                        rr.radii.map(|r| r.height),
                    ),
                    None => ([0.0, 0.0, -1.0, -1.0], [0.0; 4], [0.0; 4]),
                };
                for i in [0, 1, 2, 2, 1, 3] {
                    let (x, y, u, v) = corners[i];
                    vertices.push(Vertex {
                        position: Self::to_ndc(size, x, y),
                        uv: [u, v],
                        color,
                        clip,
                        clip_rx,
                        clip_ry,
                    });
                }
                cmds.push(DrawCmd {
                    resource,
                    linear,
                    scissor: Self::scissor(size, sqs.clip_rect),
                    buffer,
                    first_vertex,
                });
            }
        }

        let texture = target.texture();
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let pipeline = self.context.pipeline(texture.format());

        let buffers = vertices
            .iter()
            .filter(|vertices| !vertices.is_empty())
            .map(|vertices| self.vertex_buffer(vertices))
            .collect::<Option<Vec<_>>>()?;

        let bg = frame.background_color.premultiplied_f32();
        let mut encoder = self
            .context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("viz") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viz root pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg[0] as f64,
                            g: bg[1] as f64,
                            b: bg[2] as f64,
                            a: bg[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !buffers.is_empty() {
                pass.set_pipeline(&pipeline);
                let full = (0, 0, size.width, size.height);
                let mut bound = None;
                for cmd in &cmds {
                    let (x, y, w, h) = cmd.scissor.unwrap_or(full);
                    if w == 0 || h == 0 {
                        continue;
                    }
                    if bound != Some(cmd.buffer) {
                        let Some(buffer) = buffers.get(cmd.buffer) else {
                            continue;
                        };
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        bound = Some(cmd.buffer);
                    }
                    pass.set_scissor_rect(x, y, w, h);
                    let bind_group = match cmd.resource {
                        Some(id) if cmd.linear => self.context.linear(&self.resources[&id]),
                        Some(id) => &self.resources[&id].bind_group,
                        None => &self.context.white.bind_group,
                    };
                    pass.set_bind_group(0, bind_group, &[]);
                    #[expect(
                        clippy::arithmetic_side_effects,
                        reason = "the quad's six vertices are in the buffer, whose length \
                                  `first_vertex` fits (see where it is taken)"
                    )]
                    let vertices = cmd.first_vertex..cmd.first_vertex + 6;
                    pass.draw(vertices, 0..1);
                }
            }
        }
        let readback = (copy && self.output().readback())
            .then(|| Readback::record(&self.context.device, &mut encoder, texture));
        {
            let _submitting = self
                .context
                .submit_lock
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            // What the frame draws may be in pixels staged since the last
            // flush: their copies go first.
            let staged = self.context.take_staged();
            self.context
                .queue
                .submit(staged.into_iter().chain([encoder.finish()]));
        }
        readback
    }

    /// A buffer of `vertices` to draw from, made mapped and written into
    /// (as `DeviceExt::create_buffer_init` does, which panics when the
    /// buffer can't be made: past the device's limit, out of memory, a lost
    /// device). `None` then: what went wrong is the frame's error scope's.
    fn vertex_buffer(&self, vertices: &[Vertex]) -> Option<wgpu::Buffer> {
        let bytes: &[u8] = bytemuck::cast_slice(vertices);
        let buffer = self.context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viz quads"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: true,
        });
        // Not mapped, there is nothing to unmap (which wgpu would take as
        // another error).
        let Ok(mut mapped) = buffer.get_mapped_range_mut(..) else {
            tracing::debug!(bytes = bytes.len(), "viz: no buffer for a frame's vertices");
            return None;
        };
        mapped.slice(..).copy_from_slice(bytes);
        drop(mapped);
        buffer.unmap();
        Some(buffer)
    }
}

/// Which of the formats a surface offers (in its own order of preference)
/// to configure it with: one with eight bits a channel and no sRGB
/// encoding. Bitmaps hold sRGB-encoded bytes and must be written out
/// unchanged, as Chromium does, and eight bits a channel is what the
/// shader writes and what reading the surface back copies out; Chromium
/// picks 8-bit BGRA or RGBA too. A Wayland surface on Mesa offers
/// `Rgba16Float` before any 8-bit format, so "the first that isn't sRGB"
/// isn't enough. Without one, any format that isn't sRGB, else the
/// surface's own first choice.
fn swap_chain_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    let eight_bit = |f: &wgpu::TextureFormat| {
        matches!(
            f,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
        )
    };
    let mut formats = formats.iter().copied();
    formats
        .clone()
        .find(eight_bit)
        .or_else(|| formats.clone().find(|f| !f.is_srgb()))
        .or_else(|| formats.next())
}

/// A frame being copied from the surface into a buffer the CPU can map
/// (what Skia's `readPixels` does for a `CopyOutputRequest` in Chromium).
struct Readback {
    buffer: wgpu::Buffer,
    size: Size,
    /// Each row's bytes in the buffer: whole multiples of
    /// `COPY_BYTES_PER_ROW_ALIGNMENT`.
    stride: u32,
    /// The surface is BGRA, not RGBA.
    bgra: bool,
}

impl Readback {
    /// Records copying `texture` (a surface texture, with `COPY_SRC`) to a
    /// new buffer, before the frame is presented.
    fn record(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> Self {
        let size = Size::new(texture.width(), texture.height());
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "a texture's side is at most the device's limit (32768 at the most \
                      any allows): 4 bytes a pixel, rounded up to 256, fit a u32; and two \
                      u32s' product a u64"
        )]
        let (stride, bytes) = {
            let stride = (size.width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
            (stride, u64::from(stride) * u64::from(size.height))
        };
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viz readback"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(size.height),
                },
            },
            texture.size(),
        );
        let bgra = matches!(
            texture.format(),
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        Self {
            buffer,
            size,
            stride,
            bgra,
        }
    }

    /// Waits for the copy and reads it, once submitted. `None` if the
    /// device failed it (it was lost, say).
    fn read(self, device: &wgpu::Device) -> Option<CopyOutputResult> {
        let (sender, receiver) = std::sync::mpsc::channel();
        self.buffer
            .map_async(wgpu::MapMode::Read, .., move |result| {
                let _ = sender.send(result);
            });
        if let Err(err) = device.poll(wgpu::PollType::wait_indefinitely()) {
            tracing::warn!(%err, "viz: waiting for a readback");
            return None;
        }
        if let Err(err) = receiver.recv().ok()? {
            tracing::warn!(%err, "viz: mapping a readback");
            return None;
        }
        let view = self.buffer.get_mapped_range(..).ok()?;
        let pixels = tightly_packed_rgba(&view, self.size, self.stride, self.bgra);
        drop(view);
        self.buffer.unmap();
        Some(CopyOutputResult {
            size: self.size,
            pixels,
        })
    }
}

/// `size`'s pixels, as RGBA rows with nothing between them, from `rows`
/// that are `stride` bytes apart and BGRA if `bgra`.
fn tightly_packed_rgba(rows: &[u8], size: Size, stride: u32, bgra: bool) -> Vec<u8> {
    let row_bytes = size.width as usize * 4;
    // Only a hint: a size whose bytes don't fit takes none (a texture's do).
    let mut pixels = Vec::with_capacity(size.checked_byte_size().unwrap_or(0));
    for row in rows.chunks(stride as usize).take(size.height as usize) {
        pixels.extend_from_slice(&row[..row_bytes]);
    }
    if bgra {
        for px in pixels.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
    }
    pixels
}

/// What `get_current_texture` said, for the log.
fn surface_status(current: &wgpu::CurrentSurfaceTexture) -> &'static str {
    match current {
        wgpu::CurrentSurfaceTexture::Success(_) => "success",
        wgpu::CurrentSurfaceTexture::Suboptimal(_) => "suboptimal",
        wgpu::CurrentSurfaceTexture::Timeout => "timeout",
        wgpu::CurrentSurfaceTexture::Occluded => "occluded",
        wgpu::CurrentSurfaceTexture::Outdated => "outdated",
        wgpu::CurrentSurfaceTexture::Lost => "lost",
        wgpu::CurrentSurfaceTexture::Validation => "validation error",
    }
}

#[cfg(test)]
mod tests {
    //! What needs no device; drawing is in `drawing_tests`.

    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    /// Counts the times it is told.
    struct Observer(Arc<AtomicU64>);

    impl ContextLostObserver for Observer {
        fn on_context_lost(&self) {
            #[expect(
                clippy::disallowed_methods,
                reason = "a test's count: far from wrapping"
            )]
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn observer() -> (Arc<dyn ContextLostObserver>, Arc<AtomicU64>) {
        let told = Arc::new(AtomicU64::new(0));
        (Arc::new(Observer(told.clone())), told)
    }

    /// The surface is configured with an 8-bit, non-sRGB format wherever
    /// one is offered, whatever the surface puts first.
    #[test]
    fn the_swap_chain_format_has_eight_bits_a_channel() {
        use wgpu::TextureFormat::*;
        // A Wayland surface on Mesa (an AMD laptop, 2026-10-08): a float
        // format comes before the 8-bit ones, and reading it back would
        // need four bytes a pixel to hold eight.
        assert_eq!(
            swap_chain_format(&[
                Bgra8UnormSrgb,
                Rgba8UnormSrgb,
                Rgba16Float,
                Rgba16Unorm,
                Rgb10a2Unorm,
                Bgra8Unorm,
                Rgba8Unorm,
            ]),
            Some(Bgra8Unorm)
        );
        // Where the 8-bit format comes first (macOS), the same one.
        assert_eq!(
            swap_chain_format(&[Bgra8Unorm, Bgra8UnormSrgb]),
            Some(Bgra8Unorm)
        );
        assert_eq!(
            swap_chain_format(&[Rgba8UnormSrgb, Rgba8Unorm]),
            Some(Rgba8Unorm)
        );
        // Without one: anything that isn't sRGB, then the first offered.
        assert_eq!(
            swap_chain_format(&[Bgra8UnormSrgb, Rgba16Float]),
            Some(Rgba16Float)
        );
        assert_eq!(swap_chain_format(&[Bgra8UnormSrgb]), Some(Bgra8UnormSrgb));
        assert_eq!(swap_chain_format(&[]), None);
    }

    /// Rows lose the padding the copy needed, and BGRA becomes RGBA.
    #[test]
    fn readbacks_are_tightly_packed_rgba() {
        // 2x2, rows 12 bytes apart (8 of pixels, 4 of padding).
        let rows = [
            1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0, //
            9, 10, 11, 12, 13, 14, 15, 16, 0, 0, 0, 0,
        ];
        let size = Size::new(2, 2);
        assert_eq!(
            tightly_packed_rgba(&rows, size, 12, false),
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
        assert_eq!(
            tightly_packed_rgba(&rows, size, 12, true),
            [3, 2, 1, 4, 7, 6, 5, 8, 11, 10, 9, 12, 15, 14, 13, 16]
        );
    }

    #[test]
    fn with_no_device_no_shared_image_is_made() {
        let images = GpuSharedImages::new();
        assert!(
            images
                .create_shared_image(Size::new(1, 1), &[0; 4])
                .is_none()
        );
    }

    #[test]
    fn the_clients_still_there_hear_the_device_was_lost() {
        let images = GpuSharedImages::new();
        let (kept, kept_told) = observer();
        let (dropped, dropped_told) = observer();
        images.add_context_lost_observer(Arc::downgrade(&kept));
        images.add_context_lost_observer(Arc::downgrade(&dropped));
        drop(dropped);
        images.context_lost();
        assert_eq!(kept_told.load(Ordering::Relaxed), 1);
        assert_eq!(dropped_told.load(Ordering::Relaxed), 0);
        // The one gone is forgotten when another comes.
        let (another, _) = observer();
        images.add_context_lost_observer(Arc::downgrade(&another));
        assert_eq!(images.observers.lock().unwrap().len(), 2);
    }
}

#[cfg(test)]
mod shader_tests {
    use super::SHADER;

    /// The quads' shader, rounded clip included, parses and validates as
    /// WGSL (the drawing itself needs a GPU, which the tests don't have).
    #[test]
    fn the_shader_is_valid_wgsl() {
        use wgpu::naga;
        let source = format!("{SHADER}{}", crate::rounded_corner::ROUNDED_CLIP_WGSL);
        let module = naga::front::wgsl::parse_str(&source).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validates");
    }
}

#[cfg(test)]
mod drawing_tests {
    //! Drawing on the process's adapter: a GPU where there is one, the CPU
    //! adapter with `MACHOPO_GPU_ADAPTER=cpu`. A pixel that blends or samples
    //! between texels may be 1 off from another adapter's (each rounds its
    //! own way), so those are compared within 1; the rest exactly.

    use std::sync::Mutex;

    use gfx::{Color, RRectF, RectF, Size, SizeF};
    use protocol::viz::*;

    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::Display;

    const SINK: FrameSinkId = FrameSinkId {
        client_id: 1,
        sink_id: 1,
    };

    /// Held by each test: one loses the process's device, which the others
    /// draw with.
    static GPU: Mutex<()> = Mutex::new(());

    fn gpu() -> std::sync::MutexGuard<'static, ()> {
        GPU.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn frame(size: Size, sqs: SharedQuadState, quads: Vec<DrawQuad>) -> CompositorFrame {
        let area = RectF::new(0.0, 0.0, size.width as f32, size.height as f32);
        CompositorFrame {
            metadata: CompositorFrameMetadata {
                device_scale_factor: 1.0,
                frame_token: 1,
                root_background_color: Color::WHITE,
            },
            resource_list: Vec::new(),
            render_pass_list: vec![CompositorRenderPass {
                id: RenderPassId(1),
                output_rect: area,
                damage_rect: area,
                shared_quad_state_list: vec![sqs],
                quad_list: quads,
            }],
        }
    }

    fn solid(color: Color, rect: RectF) -> DrawQuad {
        DrawQuad {
            rect,
            shared_quad_state: 0,
            material: Material::SolidColor { color },
        }
    }

    fn display(size: Size) -> Display<WgpuRenderer> {
        let renderer = WgpuRenderer::offscreen(size, GpuSharedImages::new()).expect("an adapter");
        Display::new(renderer, SINK)
    }

    /// Draws what `display` has and reads it back.
    fn draw(display: &mut Display<WgpuRenderer>) -> CopyOutputResult {
        let copy = Arc::new(Mutex::new(None));
        let into = copy.clone();
        display.request_copy_of_output(CopyOutputRequest::new(move |result| {
            *into.lock().unwrap() = Some(result);
        }));
        assert!(display.draw_and_swap());
        let result = copy.lock().unwrap().take().expect("answered");
        result.expect("read back")
    }

    fn px(copy: &CopyOutputResult, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * copy.size.width + x) * 4) as usize;
        copy.pixels[i..i + 4].try_into().unwrap()
    }

    #[track_caller]
    fn assert_within_1(actual: [u8; 4], expected: [u8; 4]) {
        assert!(
            actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 1),
            "{actual:?} is not within 1 of {expected:?}"
        );
    }

    /// A renderer whose lost device's next one can't draw says so, once,
    /// and draws nothing after (machopo then ends as closing it does):
    /// what it is asked then neither draws nor panics, its output gone or
    /// not.
    #[test]
    #[expect(clippy::disallowed_methods, reason = "a test's counter")]
    fn a_renderer_that_cant_draw_says_so_once_and_draws_no_more() {
        let _gpu = gpu();
        let size = Size::new(4, 4);
        let told = Arc::new(AtomicU64::new(0));
        let counted = told.clone();
        let mut renderer = WgpuRenderer::offscreen(size, GpuSharedImages::new())
            .expect("an adapter")
            .on_failure(move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
            });
        renderer.fail("the test's".into());
        renderer.fail("again".into());
        assert_eq!(told.load(Ordering::Relaxed), 1);
        let mut display = Display::new(renderer, SINK);
        // A frame with a texture: drawn, its pixels would be uploaded.
        let mut f = frame(
            size,
            SharedQuadState::default(),
            vec![DrawQuad {
                rect: RectF::new(0.0, 0.0, 2.0, 2.0),
                shared_quad_state: 0,
                material: Material::Texture {
                    resource_id: ResourceId(7),
                    tex_coord_rect: RectF::new(0.0, 0.0, 2.0, 2.0),
                    nearest_neighbor: true,
                },
            }],
        );
        f.resource_list.push(TransferableResource {
            id: ResourceId(7),
            size: Size::new(2, 2),
            format: ResourceFormat::Rgba8Premul,
            pixels: protocol::BigBuffer::from([0, 0, 255, 255].repeat(4)),
            shared_image: None,
        });
        display.submit_compositor_frame(SINK, LocalSurfaceId::default(), f.clone());
        display.draw_and_swap();
        assert!(display.renderer().resources.is_empty());
        // As a failed configuration leaves it.
        display.renderer_mut().output = None;
        assert_eq!(display.renderer().size(), Size::default());
        display.resize(Size::new(8, 8));
        display.submit_compositor_frame(SINK, LocalSurfaceId::default(), f);
        let copy = Arc::new(Mutex::new(None));
        let into = copy.clone();
        display.request_copy_of_output(CopyOutputRequest::new(move |result| {
            *into.lock().unwrap() = Some(result);
        }));
        display.draw_and_swap();
        assert!(matches!(copy.lock().unwrap().take(), Some(None)));
    }

    #[test]
    fn draws_texture_and_solid_quads_in_order() {
        let _gpu = gpu();
        let size = Size::new(4, 4);
        // 2x2 blue texture at (0,0); a half-transparent red solid quad on
        // top of it (front-most, so first in the list) covering column 1.
        let mut f = frame(
            size,
            SharedQuadState::default(),
            vec![
                solid(Color::rgba(255, 0, 0, 128), RectF::new(1.0, 0.0, 1.0, 4.0)),
                DrawQuad {
                    rect: RectF::new(0.0, 0.0, 2.0, 2.0),
                    shared_quad_state: 0,
                    material: Material::Texture {
                        resource_id: ResourceId(7),
                        tex_coord_rect: RectF::new(0.0, 0.0, 2.0, 2.0),
                        nearest_neighbor: true,
                    },
                },
            ],
        );
        f.resource_list.push(TransferableResource {
            id: ResourceId(7),
            size: Size::new(2, 2),
            format: ResourceFormat::Rgba8Premul,
            pixels: protocol::BigBuffer::from([0, 0, 255, 255].repeat(4)),
            shared_image: None,
        });
        let mut display = display(size);
        display.submit_compositor_frame(SINK, LocalSurfaceId::default(), f);
        let copy = draw(&mut display);
        assert_eq!(copy.size, size);
        assert_eq!(px(&copy, 0, 0), [0, 0, 255, 255]); // texture only
        assert_within_1(px(&copy, 1, 0), [128, 0, 127, 255]); // red@50% over blue
        assert_within_1(px(&copy, 1, 3), [255, 127, 127, 255]); // red@50% over white
        assert_eq!(px(&copy, 3, 3), [255, 255, 255, 255]); // background
    }

    /// A shared image a client made by a message is drawn by the frame
    /// that names its mailbox, with no pixels in the frame.
    #[test]
    fn a_shared_image_made_by_a_message_is_drawn() {
        let _gpu = gpu();
        let size = Size::new(2, 2);
        let mut f = frame(
            size,
            SharedQuadState::default(),
            vec![DrawQuad {
                rect: RectF::new(0.0, 0.0, 2.0, 2.0),
                shared_quad_state: 0,
                material: Material::Texture {
                    resource_id: ResourceId(7),
                    tex_coord_rect: RectF::new(0.0, 0.0, 2.0, 2.0),
                    nearest_neighbor: true,
                },
            }],
        );
        f.resource_list.push(TransferableResource {
            id: ResourceId(7),
            size,
            format: ResourceFormat::Rgba8Premul,
            pixels: protocol::BigBuffer::from([]),
            shared_image: Some(SharedImage::from_mailbox(Mailbox(3))),
        });
        let mut display = display(size);
        display.create_shared_image(SINK, Mailbox(3), size, &[0, 0, 255, 255].repeat(4));
        display.flush_shared_images();
        display.submit_compositor_frame(SINK, LocalSurfaceId::default(), f);
        let copy = draw(&mut display);
        assert_eq!(px(&copy, 0, 0), [0, 0, 255, 255]);
        assert_eq!(px(&copy, 1, 1), [0, 0, 255, 255]);
    }

    /// A resource naming a shared image its client never made draws
    /// nothing (a lost device's, or a renderer's that is wrong), whatever
    /// pixels it carries besides (which a renderer process's can't: the IO
    /// thread's `CompositorFrame::check`): a client can't get its own bytes
    /// drawn in place of an image it doesn't have.
    #[test]
    fn a_shared_image_nobody_made_isnt_drawn() {
        let _gpu = gpu();
        let size = Size::new(2, 2);
        let mut f = frame(
            size,
            SharedQuadState::default(),
            vec![DrawQuad {
                rect: RectF::new(0.0, 0.0, 2.0, 2.0),
                shared_quad_state: 0,
                material: Material::Texture {
                    resource_id: ResourceId(7),
                    tex_coord_rect: RectF::new(0.0, 0.0, 2.0, 2.0),
                    nearest_neighbor: true,
                },
            }],
        );
        f.resource_list.push(TransferableResource {
            id: ResourceId(7),
            size,
            format: ResourceFormat::Rgba8Premul,
            pixels: protocol::BigBuffer::from([255, 0, 0, 255].repeat(4)),
            shared_image: Some(SharedImage::from_mailbox(Mailbox(4))),
        });
        let mut display = display(size);
        display.submit_compositor_frame(SINK, LocalSurfaceId::default(), f);
        let copy = draw(&mut display);
        assert_eq!(px(&copy, 0, 0), [255, 255, 255, 255]);
        assert_eq!(px(&copy, 1, 1), [255, 255, 255, 255]);
    }

    /// Viz doesn't check what it is handed (the IO thread has, of a
    /// renderer process's; the rest are machopo's own compositors'), and
    /// draws what it can of a frame that can't be right, with no wgpu
    /// error (which panics in this debug build): a quad of a state its pass
    /// hasn't is the default state's, one of a resource the frame hasn't
    /// isn't drawn, and a client that isn't the root embedding the root
    /// gets the surface's default background where it would embed itself.
    #[test]
    fn what_cant_be_right_is_drawn_as_far_as_it_can_be() {
        let _gpu = gpu();
        let size = Size::new(4, 4);
        let tab = FrameSinkId {
            client_id: 2,
            sink_id: 1,
        };
        let full = RectF::new(0.0, 0.0, 4.0, 4.0);
        let mut display = display(size);
        display.add_frame_sink(tab);
        let embeds = |sink| DrawQuad {
            rect: full,
            shared_quad_state: 0,
            material: Material::Surface {
                surface_id: SurfaceId {
                    frame_sink_id: sink,
                    local_surface_id: LocalSurfaceId::default(),
                },
                default_background_color: Color::WHITE,
            },
        };
        let tabs = frame(
            size,
            SharedQuadState::default(),
            vec![
                DrawQuad {
                    material: Material::Texture {
                        resource_id: ResourceId(99),
                        tex_coord_rect: full,
                        nearest_neighbor: true,
                    },
                    ..solid(Color::WHITE, full)
                },
                DrawQuad {
                    shared_quad_state: 5,
                    ..solid(Color::rgb(0, 0, 255), RectF::new(0.0, 0.0, 2.0, 2.0))
                },
                solid(Color::rgb(255, 0, 0), full),
                // Under the rest: the root's, which embeds this one (drawn as
                // its default background, where it would embed itself).
                embeds(SINK),
            ],
        );
        display.submit_compositor_frame(tab, LocalSurfaceId::default(), tabs);
        display.submit_compositor_frame(
            SINK,
            LocalSurfaceId::default(),
            frame(size, SharedQuadState::default(), vec![embeds(tab)]),
        );
        let copy = draw(&mut display);
        assert_eq!(px(&copy, 0, 0), [0, 0, 255, 255], "of the default state");
        assert_eq!(
            px(&copy, 3, 3),
            [255, 0, 0, 255],
            "the resource it hasn't drew nothing"
        );
    }

    /// A texture scaled up is sampled bilinearly (a canvas's), clamped at
    /// its edges, unless it asks for its nearest texel (a tile's).
    #[test]
    fn a_texture_is_sampled_bilinearly_unless_it_asks_for_the_nearest_texel() {
        let _gpu = gpu();
        let size = Size::new(4, 4);
        let draw_row = |nearest_neighbor| {
            let mut f = frame(
                size,
                SharedQuadState::default(),
                vec![DrawQuad {
                    rect: RectF::new(0.0, 0.0, 4.0, 1.0),
                    shared_quad_state: 0,
                    material: Material::Texture {
                        resource_id: ResourceId(7),
                        tex_coord_rect: RectF::new(0.0, 0.0, 2.0, 1.0),
                        nearest_neighbor,
                    },
                }],
            );
            f.resource_list.push(TransferableResource {
                id: ResourceId(7),
                size: Size::new(2, 1),
                format: ResourceFormat::Rgba8Premul,
                pixels: protocol::BigBuffer::from([255, 0, 0, 255, 0, 0, 255, 255]),
                shared_image: None,
            });
            let mut display = display(size);
            display.submit_compositor_frame(SINK, LocalSurfaceId::default(), f);
            let copy = draw(&mut display);
            [0, 1, 2, 3].map(|x| px(&copy, x, 0))
        };
        let (red, blue) = ([255, 0, 0, 255], [0, 0, 255, 255]);
        assert_eq!(draw_row(true), [red, red, blue, blue]);
        // A quarter and three quarters of the way between the centres;
        // the outer pixels are past them, at the edge texels.
        let linear = draw_row(false);
        assert_eq!(linear[0], red);
        assert_within_1(linear[1], [191, 0, 64, 255]);
        assert_within_1(linear[2], [64, 0, 191, 255]);
        assert_eq!(linear[3], blue);
    }

    #[test]
    fn a_rounded_clip_leaves_its_corners_out() {
        let _gpu = gpu();
        let size = Size::new(40, 40);
        let area = RectF::new(0.0, 0.0, 40.0, 40.0);
        let sqs = SharedQuadState {
            clip_rect: Some(area),
            rounded_clip: Some(RRectF::new(area, [SizeF::new(20.0, 20.0); 4])),
            opacity: 1.0,
        };
        let mut display = display(size);
        display.submit_compositor_frame(
            SINK,
            LocalSurfaceId::default(),
            frame(size, sqs, vec![solid(Color::rgb(0, 128, 0), area)]),
        );
        let copy = draw(&mut display);
        // A circle: past its curve the background, inside it the quad.
        assert_eq!(px(&copy, 1, 1), [255, 255, 255, 255]);
        assert_eq!(px(&copy, 38, 38), [255, 255, 255, 255]);
        assert_eq!(px(&copy, 20, 20), [0, 128, 0, 255]);
        assert_eq!(px(&copy, 20, 1), [0, 128, 0, 255]);
    }

    #[test]
    fn a_clip_and_an_opacity_apply_to_their_quads() {
        let _gpu = gpu();
        let size = Size::new(4, 4);
        let sqs = SharedQuadState {
            clip_rect: Some(RectF::new(0.0, 0.0, 2.0, 4.0)),
            rounded_clip: None,
            opacity: 0.5,
        };
        let mut display = display(size);
        display.submit_compositor_frame(
            SINK,
            LocalSurfaceId::default(),
            frame(
                size,
                sqs,
                vec![solid(Color::rgb(0, 0, 0), RectF::new(0.0, 0.0, 4.0, 4.0))],
            ),
        );
        let copy = draw(&mut display);
        assert_within_1(px(&copy, 0, 0), [128, 128, 128, 255]);
        assert_eq!(px(&copy, 3, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn draws_an_embedded_surface() {
        let _gpu = gpu();
        let size = Size::new(4, 4);
        let tab = FrameSinkId {
            client_id: 2,
            sink_id: 1,
        };
        let mut display = display(size);
        let root = frame(
            size,
            SharedQuadState::default(),
            vec![
                solid(Color::rgba(255, 0, 0, 255), RectF::new(0.0, 0.0, 4.0, 1.0)),
                DrawQuad {
                    rect: RectF::new(0.0, 1.0, 4.0, 3.0),
                    shared_quad_state: 0,
                    material: Material::Surface {
                        surface_id: SurfaceId {
                            frame_sink_id: tab,
                            local_surface_id: LocalSurfaceId::default(),
                        },
                        default_background_color: Color::WHITE,
                    },
                },
            ],
        );
        display.add_frame_sink(tab);
        display.submit_compositor_frame(SINK, LocalSurfaceId::default(), root);
        display.submit_compositor_frame(
            tab,
            LocalSurfaceId::default(),
            frame(
                size,
                SharedQuadState::default(),
                vec![solid(
                    Color::rgba(0, 0, 255, 255),
                    RectF::new(0.0, 0.0, 4.0, 4.0),
                )],
            ),
        );
        let copy = draw(&mut display);
        assert_eq!(px(&copy, 0, 0), [255, 0, 0, 255]);
        assert_eq!(px(&copy, 0, 1), [0, 0, 255, 255]);
        assert_eq!(px(&copy, 3, 3), [0, 0, 255, 255]);
    }

    #[test]
    fn a_resized_output_is_read_back_at_its_new_size() {
        let _gpu = gpu();
        let mut display = display(Size::new(4, 4));
        display.submit_compositor_frame(
            SINK,
            LocalSurfaceId::default(),
            frame(Size::new(6, 2), SharedQuadState::default(), vec![]),
        );
        display.resize(Size::new(6, 2));
        let copy = draw(&mut display);
        assert_eq!(copy.size, Size::new(6, 2));
        assert_eq!(copy.pixels.len(), 6 * 2 * 4);
        assert_eq!(px(&copy, 5, 1), [255, 255, 255, 255]);
    }

    /// The adapter is the one `MACHOPO_GPU_ADAPTER` asks for (the CPU one with
    /// `cpu`, else a GPU wherever the machine has one), on the platform's
    /// backend.
    #[test]
    fn the_adapter_is_the_one_asked_for() {
        use crate::gpu_context::AdapterPreference;
        let _gpu = gpu();
        let renderer = WgpuRenderer::offscreen(Size::new(1, 1), GpuSharedImages::new()).unwrap();
        let info = renderer.adapter_info();
        // On the platform's own backend.
        #[cfg(windows)]
        assert_eq!(info.backend, wgpu::Backend::Dx12);
        #[cfg(target_os = "macos")]
        assert_eq!(info.backend, wgpu::Backend::Metal);
        #[cfg(not(any(windows, target_os = "macos")))]
        assert!(
            matches!(info.backend, wgpu::Backend::Vulkan | wgpu::Backend::Gl),
            "{:?}",
            info.backend
        );
        let used = info.device_type;
        match AdapterPreference::from_env() {
            AdapterPreference::Cpu => assert_eq!(used, wgpu::DeviceType::Cpu),
            AdapterPreference::Gpu => {
                let instance = crate::gpu_context::instance_with(
                    wgpu::InstanceDescriptor::new_without_display_handle,
                );
                let has_gpu =
                    pollster::block_on(instance.enumerate_adapters(crate::gpu_context::BACKENDS))
                        .iter()
                        .any(|a| a.get_info().device_type != wgpu::DeviceType::Cpu);
                assert_eq!(used != wgpu::DeviceType::Cpu, has_gpu, "{used:?}");
            }
        }
    }

    #[test]
    fn renderers_share_the_process_device() {
        let _gpu = gpu();
        let a = WgpuRenderer::offscreen(Size::new(1, 1), GpuSharedImages::new()).unwrap();
        let b = WgpuRenderer::offscreen(Size::new(2, 2), GpuSharedImages::new()).unwrap();
        assert!(Arc::ptr_eq(&a.context, &b.context));
    }

    /// Shut down as Viz ends, a renderer is done with the process's device:
    /// its clients put nothing more on it, the next renderer makes another,
    /// and the device goes with the last renderer that holds it.
    #[test]
    fn a_renderer_shut_down_lets_the_process_device_go() {
        let _gpu = gpu();
        let images = GpuSharedImages::new();
        let mut a = WgpuRenderer::offscreen(Size::new(1, 1), images.clone()).unwrap();
        let b = WgpuRenderer::offscreen(Size::new(1, 1), GpuSharedImages::new()).unwrap();
        let device = Arc::downgrade(&a.context);
        a.shutdown();
        assert!(
            images
                .create_shared_image(Size::new(1, 1), &[0; 4])
                .is_none()
        );
        let c = WgpuRenderer::offscreen(Size::new(1, 1), GpuSharedImages::new()).unwrap();
        assert!(!Arc::ptr_eq(&b.context, &c.context), "a new device");
        drop(a);
        assert!(device.upgrade().is_some(), "the other renderer's still");
        drop(b);
        assert!(device.upgrade().is_none(), "gone with the last renderer");
    }

    /// Counts the times it is told.
    struct Observer(Arc<AtomicU64>);

    impl ContextLostObserver for Observer {
        fn on_context_lost(&self) {
            #[expect(
                clippy::disallowed_methods,
                reason = "a test's count: far from wrapping"
            )]
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// A lost device is made again once, by the first renderer to draw;
    /// every renderer moves to it, and their clients hear of it.
    #[test]
    fn a_lost_device_is_made_again_for_every_renderer() {
        let _gpu = gpu();
        let size = Size::new(2, 2);
        let images = [GpuSharedImages::new(), GpuSharedImages::new()];
        let told = [Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0))];
        let observers: Vec<Arc<dyn ContextLostObserver>> = told
            .iter()
            .map(|t| Arc::new(Observer(t.clone())) as Arc<dyn ContextLostObserver>)
            .collect();
        for (images, observer) in images.iter().zip(&observers) {
            images.add_context_lost_observer(Arc::downgrade(observer));
        }
        let mut displays = images
            .clone()
            .map(|images| Display::new(WgpuRenderer::offscreen(size, images).unwrap(), SINK));
        let before = displays[0].renderer().context.generation;
        displays[0].lose_context();
        for display in &mut displays {
            display.submit_compositor_frame(
                SINK,
                LocalSurfaceId::default(),
                frame(
                    size,
                    SharedQuadState::default(),
                    vec![solid(Color::rgb(0, 0, 255), RectF::new(0.0, 0.0, 2.0, 2.0))],
                ),
            );
            assert_eq!(px(&draw(display), 1, 1), [0, 0, 255, 255]);
        }
        let [a, b] = &displays;
        assert_eq!(a.renderer().context.generation, before + 1);
        assert!(Arc::ptr_eq(&a.renderer().context, &b.renderer().context));
        assert!(!a.renderer().context.is_lost());
        assert_eq!(told.map(|t| t.load(Ordering::Relaxed)), [1, 1]);
        // Shared images go on the new device.
        assert!(
            images[0]
                .create_shared_image(Size::new(1, 1), &[0; 4])
                .is_some()
        );
    }
    /// What draws a texture bilinearly is made the first time it is drawn
    /// so, and kept: a tile, never drawn so, has none.
    #[test]
    fn a_texture_has_what_draws_it_bilinearly_once_it_is_drawn_so() {
        let _gpu = gpu();
        let images = GpuSharedImages::new();
        let renderer = WgpuRenderer::offscreen(Size::new(1, 1), images.clone()).unwrap();
        let image = images
            .create_shared_image(Size::new(1, 1), &[0; 4])
            .unwrap();
        images.flush();
        let uploaded = &image.downcast_ref::<SharedImageBacking>().unwrap().uploaded;
        assert!(uploaded.linear_bind_group.get().is_none());
        let first: *const wgpu::BindGroup = renderer.context.linear(uploaded);
        assert!(uploaded.linear_bind_group.get().is_some());
        // The renderer's own copy of the resource shares it.
        let copy = uploaded.clone();
        assert!(std::ptr::eq(first, renderer.context.linear(&copy)));
    }

    /// A shared image let go by everything that held it leaves its
    /// texture for the next shared image of its size, which is drawn with
    /// its own pixels.
    #[test]
    fn a_shared_image_let_go_gives_its_texture_to_the_next_of_its_size() {
        let _gpu = gpu();
        // A size no other test's shared images have: the device, and so
        // its pool, is the process's.
        let size = Size::new(3, 5);
        let images = GpuSharedImages::new();
        let renderer = WgpuRenderer::offscreen(size, images.clone()).unwrap();
        let pool = renderer.context.pool.clone();
        let pooled = || pool.sizes().iter().filter(|&&s| s == size).count();
        let mut display = Display::new(renderer, SINK);
        let filled = |color: [u8; 4]| {
            let pixels: Vec<u8> = std::iter::repeat_n(color, 15).flatten().collect();
            let image = images.create_shared_image(size, &pixels).unwrap();
            images.flush();
            image
        };
        let before = pooled();
        let red = filled([255, 0, 0, 255]);
        assert_eq!(pooled(), before);
        drop(red);
        assert_eq!(pooled(), before + 1);
        // One of another size takes none of this size.
        drop(images.create_shared_image(Size::new(5, 3), &[0; 60]));
        assert_eq!(pooled(), before + 1);

        // The next of this size takes it, and is what it was made of.
        let blue: Vec<u8> = std::iter::repeat_n([0, 0, 255, 255], 15)
            .flatten()
            .collect();
        display.create_shared_image(SINK, Mailbox(9), size, &blue);
        display.flush_shared_images();
        assert_eq!(pooled(), before);
        let mut f = frame(
            size,
            SharedQuadState::default(),
            vec![DrawQuad {
                rect: RectF::new(0.0, 0.0, 3.0, 5.0),
                shared_quad_state: 0,
                material: Material::Texture {
                    resource_id: ResourceId(7),
                    tex_coord_rect: RectF::new(0.0, 0.0, 3.0, 5.0),
                    nearest_neighbor: true,
                },
            }],
        );
        f.resource_list.push(TransferableResource {
            id: ResourceId(7),
            size,
            format: ResourceFormat::Rgba8Premul,
            pixels: protocol::BigBuffer::from([]),
            shared_image: Some(SharedImage::from_mailbox(Mailbox(9))),
        });
        display.submit_compositor_frame(SINK, LocalSurfaceId::default(), f);
        let copy = draw(&mut display);
        for (x, y) in [(0, 0), (2, 4), (1, 2)] {
            assert_eq!(px(&copy, x, y), [0, 0, 255, 255], "{x},{y}");
        }
        // The frame still holds it: not let go while anything does.
        display.destroy_shared_image(SINK, Mailbox(9));
        assert_eq!(pooled(), before);
        drop(display);
        assert_eq!(pooled(), before + 1);
    }

    /// Pixels that came in memory shared with whoever drew them are what
    /// is drawn, whatever their rows' length is to the 256 bytes the GPU
    /// copies rows at: a tile's (256 pixels wide, nothing between its
    /// rows), a few pixels', and rows longer than 256 bytes that aren't a
    /// multiple of it. On a texture made for them and on one of the
    /// pool's, as the frame that draws them is submitted and after a
    /// flush. (Sizes no other test's shared images have: the pool is the
    /// process's.)
    #[test]
    fn pixels_in_shared_memory_are_what_is_drawn() {
        let _gpu = gpu();
        // Each pixel says where it is, so one in the wrong place shows.
        let pixels_of = |size: Size, salt: u8| -> Vec<u8> {
            (0..size.height)
                .flat_map(|y| (0..size.width).map(move |x| (x, y)))
                .flat_map(|(x, y)| [x as u8, y as u8, salt, 255])
                .collect()
        };
        for (n, size) in [
            Size::new(256, 3),
            Size::new(7, 3),
            Size::new(70, 2),
            Size::new(1, 6),
        ]
        .into_iter()
        .enumerate()
        {
            let images = GpuSharedImages::new();
            let renderer = WgpuRenderer::offscreen(size, images.clone()).unwrap();
            let pool = renderer.context.pool.clone();
            let pooled = || pool.sizes().iter().filter(|&&s| s == size).count();
            let context = renderer.context.clone();
            let mut display = Display::new(renderer, SINK);
            // A texture of the size let go: the first image is written
            // over it, and the second (the first's is still drawn) on one
            // made for it.
            let own = vec![9; pixels_of(size, 0).len()];
            drop(images.create_shared_image(size, &own));
            let before = pooled();
            assert!(before > 0);
            for (round, flush) in [(0u8, true), (1, false)] {
                let salt = 40 + round;
                let mailbox = Mailbox(u64::from(round) + 1);
                let pixels = pixels_of(size, salt);
                let shared = ipc::local::shared_buffer(&pixels).unwrap();
                assert!(shared.as_bytes().is_none());
                display.create_shared_image(SINK, mailbox, size, &shared);
                drop(shared);
                assert_eq!(pooled(), before - 1, "round {round}");
                if flush {
                    display.flush_shared_images();
                    // Sent to the GPU now: nothing waits for a frame to
                    // be drawn, holding what it was copied into.
                    assert!(context.take_staged().is_none());
                }
                let (w, h) = (size.width as f32, size.height as f32);
                let mut f = frame(
                    size,
                    SharedQuadState::default(),
                    vec![DrawQuad {
                        rect: RectF::new(0.0, 0.0, w, h),
                        shared_quad_state: 0,
                        material: Material::Texture {
                            resource_id: ResourceId(7 + u64::from(round)),
                            tex_coord_rect: RectF::new(0.0, 0.0, w, h),
                            nearest_neighbor: true,
                        },
                    }],
                );
                f.resource_list.push(TransferableResource {
                    id: ResourceId(7 + u64::from(round)),
                    size,
                    format: ResourceFormat::Rgba8Premul,
                    pixels: protocol::BigBuffer::from([]),
                    shared_image: Some(SharedImage::from_mailbox(mailbox)),
                });
                display.submit_compositor_frame(SINK, LocalSurfaceId::default(), f);
                let copy = draw(&mut display);
                for y in 0..size.height {
                    for x in 0..size.width {
                        assert_eq!(
                            px(&copy, x, y),
                            [x as u8, y as u8, salt, 255],
                            "size {n}, round {round}: {x},{y}"
                        );
                    }
                }
                display.destroy_shared_image(SINK, mailbox);
            }
        }
        // On a device that was lost nothing is copied and nothing goes
        // wrong, with pixels staged before it was lost or after; the next
        // device draws what is made on it.
        let size = Size::new(9, 2);
        let images = GpuSharedImages::new();
        let renderer = WgpuRenderer::offscreen(size, images.clone()).unwrap();
        let mut display = Display::new(renderer, SINK);
        let pixels = pixels_of(size, 50);
        let shared = ipc::local::shared_buffer(&pixels).unwrap();
        display.create_shared_image(SINK, Mailbox(1), size, &shared);
        display.lose_context();
        display.create_shared_image(SINK, Mailbox(2), size, &shared);
        display.flush_shared_images();
        let blue = solid(Color::rgb(0, 0, 255), RectF::new(0.0, 0.0, 9.0, 2.0));
        let f = frame(size, SharedQuadState::default(), vec![blue]);
        display.submit_compositor_frame(SINK, LocalSurfaceId::default(), f);
        assert_eq!(px(&draw(&mut display), 1, 1), [0, 0, 255, 255]);
        display.create_shared_image(SINK, Mailbox(3), size, &shared);
        display.flush_shared_images();

        // Pixels that aren't all there make no image, as any that don't
        // fit their size.
        let images = GpuSharedImages::new();
        let _renderer = WgpuRenderer::offscreen(Size::new(2, 2), images.clone()).unwrap();
        let short = ipc::local::shared_buffer(&[0; 15]).unwrap();
        assert!(
            images
                .create_shared_image(Size::new(2, 2), &short)
                .is_none()
        );
        let whole = ipc::local::shared_buffer(&[0; 16]).unwrap();
        assert!(
            images
                .create_shared_image(Size::new(2, 2), &whole)
                .is_some()
        );
    }

    /// The pool keeps the textures put last, up to so many and so many
    /// bytes, and none that alone is more than it keeps.
    #[test]
    fn the_texture_pool_keeps_the_newest_within_its_limits() {
        use crate::gpu_context::{POOL_BYTES, POOL_TEXTURES, TexturePool};
        let _gpu = gpu();
        let renderer = WgpuRenderer::offscreen(Size::new(1, 1), GpuSharedImages::new()).unwrap();
        let texture = |size: Size| {
            let pixels = vec![0; size.checked_byte_size().unwrap()];
            renderer.context.upload(size, &pixels)
        };
        // So many: the oldest go.
        let pool = TexturePool::default();
        let count = u32::try_from(POOL_TEXTURES).unwrap();
        for height in 1..=count + 2 {
            pool.put(texture(Size::new(1, height)));
        }
        let sizes = pool.sizes();
        assert_eq!(sizes.len(), POOL_TEXTURES);
        assert_eq!(sizes[0], Size::new(1, 3));
        assert_eq!(sizes.last(), Some(&Size::new(1, count + 2)));
        // Taken: the one put last of that size, and no other.
        assert!(pool.take(Size::new(1, 1)).is_none());
        assert!(pool.take(Size::new(1, 3)).is_some());
        assert!(pool.take(Size::new(1, 3)).is_none());
        assert_eq!(pool.sizes().len(), POOL_TEXTURES - 1);

        // So many bytes: three textures of over a third of them, the first
        // let go when the third comes; room again once one is taken.
        let pool = TexturePool::default();
        let side = 1900;
        let third = Size::new(side, side);
        assert!(third.checked_byte_size().unwrap() * 3 > POOL_BYTES);
        assert!(third.checked_byte_size().unwrap() * 2 <= POOL_BYTES);
        pool.put(texture(Size::new(1, 1)));
        pool.put(texture(third));
        pool.put(texture(third));
        assert_eq!(pool.sizes().len(), 3);
        pool.put(texture(third));
        assert_eq!(pool.sizes(), [third, third]);
        assert!(pool.take(third).is_some());
        pool.put(texture(Size::new(1, 1)));
        pool.put(texture(third));
        assert_eq!(pool.sizes(), [third, Size::new(1, 1), third]);

        // One larger than all it keeps isn't kept, and pushes nothing out.
        let large = Size::new(2900, 2900);
        assert!(large.checked_byte_size().unwrap() > POOL_BYTES);
        pool.put(texture(large));
        assert_eq!(pool.sizes(), [third, Size::new(1, 1), third]);
        // Of two of a size, the one put last is taken: the other, unused
        // the longest, is the next to go.
        assert!(pool.take(third).is_some());
        assert_eq!(pool.sizes(), [third, Size::new(1, 1)]);
    }

    /// A shared image is made only of pixels that fit its size, no bigger
    /// than a texture Viz takes: as the GPU process checks a client's.
    #[test]
    fn a_shared_image_of_pixels_that_dont_fit_isnt_made() {
        let _gpu = gpu();
        let images = GpuSharedImages::new();
        let _renderer = WgpuRenderer::offscreen(Size::new(1, 1), images.clone()).unwrap();
        let wide = Size::new(MAX_TEXTURE_SIZE + 1, 1);
        assert!(
            images
                .create_shared_image(Size::new(2, 2), &[0; 15])
                .is_none()
        );
        assert!(
            images
                .create_shared_image(Size::new(2, 2), &[0; 17])
                .is_none()
        );
        assert!(
            images
                .create_shared_image(wide, &vec![0; (MAX_TEXTURE_SIZE as usize + 1) * 4])
                .is_none()
        );
        assert!(
            images
                .create_shared_image(Size::new(2, 2), &[0; 16])
                .is_some()
        );
        let widest = Size::new(MAX_TEXTURE_SIZE, 1);
        assert!(
            images
                .create_shared_image(widest, &vec![0; MAX_TEXTURE_SIZE as usize * 4])
                .is_some()
        );
    }

    /// Lets Viz's tests of catching a validation error make one without a
    /// panic in a debug build, while it is held.
    struct Tolerating;

    impl Tolerating {
        fn new() -> Self {
            crate::gpu_context::TOLERATE_VALIDATION
                .store(true, std::sync::atomic::Ordering::Release);
            Self
        }
    }

    impl Drop for Tolerating {
        fn drop(&mut self) {
            crate::gpu_context::TOLERATE_VALIDATION
                .store(false, std::sync::atomic::Ordering::Release);
        }
    }

    /// A frame aggregated from several clients' may have more vertices
    /// than a buffer takes: it is drawn from several, in order. Here the
    /// root's quads fill one buffer but for one, and the tab it embeds
    /// draws two over them, the front one from a second buffer.
    #[test]
    fn a_frame_of_more_vertices_than_a_buffer_takes_is_drawn_from_several() {
        let _gpu = gpu();
        let size = Size::new(4, 4);
        let tab = FrameSinkId {
            client_id: 2,
            sink_id: 1,
        };
        let full = RectF::new(0.0, 0.0, 4.0, 4.0);
        let mut quads = vec![DrawQuad {
            rect: full,
            shared_quad_state: 0,
            material: Material::Surface {
                surface_id: SurfaceId {
                    frame_sink_id: tab,
                    local_surface_id: LocalSurfaceId::default(),
                },
                default_background_color: Color::WHITE,
            },
        }];
        quads.extend(std::iter::repeat_n(
            solid(Color::rgb(0, 255, 0), full),
            protocol::viz::MAX_QUADS - 1,
        ));
        let mut display = display(size);
        display.add_frame_sink(tab);
        display.submit_compositor_frame(
            tab,
            LocalSurfaceId::default(),
            frame(
                size,
                SharedQuadState::default(),
                vec![
                    solid(Color::rgb(0, 0, 255), RectF::new(0.0, 0.0, 1.0, 1.0)),
                    solid(Color::rgb(255, 0, 0), full),
                ],
            ),
        );
        display.submit_compositor_frame(
            SINK,
            LocalSurfaceId::default(),
            frame(size, SharedQuadState::default(), quads),
        );
        let copy = draw(&mut display);
        assert_eq!(px(&copy, 0, 0), [0, 0, 255, 255], "the second buffer's");
        assert_eq!(
            px(&copy, 2, 2),
            [255, 0, 0, 255],
            "the first's, over the root's"
        );
    }

    /// What wgpu reports is handled by its kind: a validation error is
    /// only told of (and panics in a debug build, unless a test of it
    /// tolerates it); out of memory lets the pool's textures go; an
    /// internal error takes the device as lost.
    #[test]
    fn wgpus_errors_are_handled_by_their_kind() {
        use crate::gpu_context::{GpuError, report};
        let _gpu = gpu();
        let context = GpuContext::shared(None).expect("a device");
        let source = || -> wgpu::ErrorSource { Box::new(std::io::Error::other("a test's")) };
        let pool = Arc::new(crate::gpu_context::TexturePool::default());
        pool.put(context.upload(Size::new(2, 2), &[0; 16]));
        assert_eq!(pool.sizes(), [Size::new(2, 2)]);
        let lost = std::sync::atomic::AtomicBool::new(false);
        let weak = Arc::downgrade(&pool);

        let tolerating = Tolerating::new();
        let validation = wgpu::Error::Validation {
            source: source(),
            description: "a test's".into(),
        };
        assert_eq!(
            report(&validation, "a test", &lost, &weak),
            GpuError::Validation
        );
        drop(tolerating);
        assert_eq!(pool.sizes().len(), 1);
        assert!(!lost.load(std::sync::atomic::Ordering::Acquire));

        let memory = wgpu::Error::OutOfMemory { source: source() };
        assert_eq!(
            report(&memory, "a test", &lost, &weak),
            GpuError::OutOfMemory
        );
        assert_eq!(pool.sizes(), Vec::<Size>::new());
        assert!(!lost.load(std::sync::atomic::Ordering::Acquire));

        let internal = wgpu::Error::Internal {
            source: source(),
            description: "a test's".into(),
        };
        assert_eq!(
            report(&internal, "a test", &lost, &weak),
            GpuError::Internal
        );
        assert!(lost.load(std::sync::atomic::Ordering::Acquire));
        // Worst last: a scope's caller is told the worst.
        assert!(GpuError::Validation < GpuError::OutOfMemory);
        assert!(GpuError::OutOfMemory < GpuError::Internal);
    }

    /// A debug build panics on a validation error: a check before it is
    /// missing, and every test that gets there says so.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "a check missing before it")]
    fn a_validation_error_panics_in_a_debug_build() {
        let _gpu = gpu();
        let context = GpuContext::shared(None).expect("a device");
        let too_large = context.device.limits().max_buffer_size + 4;
        let _ = context.scoped("a test", || {
            context.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("too large"),
                size: too_large,
                usage: wgpu::BufferUsages::VERTEX,
                mapped_at_creation: false,
            })
        });
    }

    /// Error scopes catch what they are around, on the thread they are
    /// on, and only that: the device draws on after.
    #[test]
    fn an_error_scope_catches_what_it_is_around() {
        let _gpu = gpu();
        let context = GpuContext::shared(None).expect("a device");
        let _tolerating = Tolerating::new();
        let too_large = context.device.limits().max_buffer_size + 4;
        let ((), error) = context.scoped("a test", || {
            let _ = context.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("too large"),
                size: too_large,
                usage: wgpu::BufferUsages::VERTEX,
                mapped_at_creation: false,
            });
        });
        assert_eq!(error, Some(crate::gpu_context::GpuError::Validation));
        let ((), error) = context.scoped("a test", || {
            let _ = context.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("fine"),
                size: 64,
                usage: wgpu::BufferUsages::VERTEX,
                mapped_at_creation: false,
            });
        });
        assert_eq!(error, None);
        assert!(!context.is_lost());
    }

    /// An error no scope is around goes to Viz's handler, as a scope's
    /// would: told of, not wgpu's panic.
    #[test]
    fn an_error_no_scope_is_around_is_handled() {
        let _gpu = gpu();
        let context = GpuContext::shared(None).expect("a device");
        let _tolerating = Tolerating::new();
        let too_large = context.device.limits().max_buffer_size + 4;
        let _ = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("too large, in no scope"),
            size: too_large,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });
        assert!(!context.is_lost());
    }

    /// A frame wgpu reports an error of (here pixels shorter than their
    /// resource's size, which Viz's checks would have stopped: the frame
    /// goes to the renderer around them) isn't presented, nor copied, and
    /// the frame after is drawn as usual.
    #[test]
    fn a_frame_wgpu_reports_an_error_of_isnt_copied_and_the_next_is_drawn() {
        let _gpu = gpu();
        let _tolerating = Tolerating::new();
        let size = Size::new(2, 2);
        let mut renderer =
            WgpuRenderer::offscreen(size, GpuSharedImages::new()).expect("an adapter");
        let short = TransferableResource {
            id: ResourceId(1),
            size,
            format: ResourceFormat::Rgba8Premul,
            pixels: protocol::BigBuffer::from([255, 0, 0, 255]),
            shared_image: None,
        };
        fn aggregated<'a>(
            size: Size,
            quads: Vec<DrawQuad>,
            resources: HashMap<ResourceId, &'a TransferableResource>,
        ) -> AggregatedFrame<'a> {
            let f = frame(size, SharedQuadState::default(), quads);
            AggregatedFrame {
                render_pass_list: f.render_pass_list,
                resources,
                images: HashMap::new(),
                background_color: Color::WHITE,
                size_in_pixels: size,
                embedded: Default::default(),
            }
        }
        let bad = aggregated(
            size,
            vec![DrawQuad {
                rect: RectF::new(0.0, 0.0, 2.0, 2.0),
                shared_quad_state: 0,
                material: Material::Texture {
                    resource_id: ResourceId(1),
                    tex_coord_rect: RectF::new(0.0, 0.0, 2.0, 2.0),
                    nearest_neighbor: true,
                },
            }],
            HashMap::from([(ResourceId(1), &short)]),
        );
        assert_eq!(
            renderer.draw_frame_and_copy(&bad).err(),
            Some(CopyFailure::Unsupported)
        );
        assert!(!renderer.context.is_lost());
        let good = aggregated(
            size,
            vec![solid(Color::rgb(0, 0, 255), RectF::new(0.0, 0.0, 2.0, 2.0))],
            HashMap::new(),
        );
        let copy = renderer.draw_frame_and_copy(&good).expect("drawn");
        assert_eq!(px(&copy, 1, 1), [0, 0, 255, 255]);
        renderer.shutdown();
    }
}
