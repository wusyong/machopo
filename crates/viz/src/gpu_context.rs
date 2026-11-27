//! The GPU a process draws with: one wgpu instance, and one device on it
//! that every renderer and every client's shared images share (Chromium's
//! GPU process, whose one `GpuChannelManager` and shared context serve
//! every compositor). Made the first time something draws, kept until the
//! process ends: making it takes about 180 ms, and a test binary opens a
//! browser many times. When the device is lost it is made again, as a new
//! generation, and whoever drew on the old one moves to the new one.
//!
//! The adapter is a GPU wherever there is one, else the CPU one wgpu falls
//! back to (WARP on Windows, Mesa's lavapipe or llvmpipe on Linux), as
//! Chrome takes the GPU, else SwiftShader. `MACHOPO_GPU_ADAPTER=cpu` takes the
//! CPU one even with a GPU: to reproduce a machine without one, and to
//! test that path.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use gfx::Size;
use protocol::SharedBuffer;
use protocol::viz::MAX_TEXTURE_SIZE;

use crate::display::Pixels;
use crate::wgpu_renderer::{SHADER, UploadedResource, Vertex};

/// The environment variable that picks the adapter: `cpu` for the CPU
/// one; unset, empty or `gpu` for a GPU when there is one.
pub const ADAPTER_ENV: &str = "MACHOPO_GPU_ADAPTER";

/// Which adapter to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterPreference {
    /// A GPU, the CPU adapter if there is none.
    Gpu,
    /// Only the CPU adapter.
    Cpu,
}

impl AdapterPreference {
    /// What [`ADAPTER_ENV`] asks for.
    pub fn from_env() -> Self {
        Self::parse(std::env::var(ADAPTER_ENV).ok().as_deref())
    }

    fn parse(value: Option<&str>) -> Self {
        match value {
            Some("cpu") => Self::Cpu,
            None | Some("" | "gpu") => Self::Gpu,
            Some(other) => {
                tracing::warn!(
                    value = other,
                    "viz: {ADAPTER_ENV} is `cpu` or `gpu`; taking a GPU"
                );
                Self::Gpu
            }
        }
    }
}

/// Of adapters of `types` (in the order they were found), the one to take
/// for `preference`: the GPU most likely the fastest (a discrete one, an
/// integrated one, one of unknown type, a virtual one), the first of each
/// kind; else the first CPU adapter. With [`AdapterPreference::Cpu`], only
/// a CPU adapter.
pub(crate) fn pick_adapter(
    types: &[wgpu::DeviceType],
    preference: AdapterPreference,
) -> Option<usize> {
    let rank = |t: wgpu::DeviceType| match (preference, t) {
        (AdapterPreference::Cpu, wgpu::DeviceType::Cpu) => Some(0),
        (AdapterPreference::Cpu, _) => None,
        (_, wgpu::DeviceType::DiscreteGpu) => Some(0),
        (_, wgpu::DeviceType::IntegratedGpu) => Some(1),
        (_, wgpu::DeviceType::Other) => Some(2),
        (_, wgpu::DeviceType::VirtualGpu) => Some(3),
        (_, wgpu::DeviceType::Cpu) => Some(4),
    };
    types
        .iter()
        .enumerate()
        .filter_map(|(i, &t)| Some((rank(t)?, i)))
        .min()
        .map(|(_, i)| i)
}

/// The backends drawn with: each platform's own, with its CPU adapter.
/// Windows' Direct3D 12, as Chromium draws with Direct3D there (WARP is a
/// Direct3D adapter); Metal on macOS; Vulkan on Linux, with GL where there
/// is no Vulkan driver (Mesa's lavapipe is Vulkan, its llvmpipe GL). Every
/// backend wgpu has takes about 190 ms to list on Windows, Direct3D 12
/// alone about 117.
pub const BACKENDS: wgpu::Backends = if cfg!(windows) {
    wgpu::Backends::DX12
} else if cfg!(target_os = "macos") {
    wgpu::Backends::METAL
} else {
    wgpu::Backends::VULKAN.union(wgpu::Backends::GL)
};

static INSTANCE: OnceLock<wgpu::Instance> = OnceLock::new();

/// The process's wgpu instance, made with `descriptor` (on [`BACKENDS`])
/// by the first caller: a window's, with its display (which some backends
/// need for its surfaces).
pub fn instance_with(descriptor: impl FnOnce() -> wgpu::InstanceDescriptor) -> wgpu::Instance {
    INSTANCE
        .get_or_init(|| {
            let mut descriptor = descriptor();
            descriptor.backends = BACKENDS;
            wgpu::Instance::new(descriptor)
        })
        .clone()
}

/// The process's wgpu instance, made with no display if there is none yet.
pub fn instance() -> wgpu::Instance {
    instance_with(wgpu::InstanceDescriptor::new_without_display_handle)
}

/// The adapter the process draws with, once something has drawn.
pub fn adapter_info() -> Option<wgpu::AdapterInfo> {
    let shared = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    shared.as_ref().map(|context| context.adapter.get_info())
}

/// The device every renderer draws with, and what drawing needs on it.
pub(crate) struct GpuContext {
    /// Which device this is, counting from 0: what was put on another
    /// isn't drawn.
    pub(crate) generation: u64,
    /// Set when the device is lost, by wgpu's callback (on whichever thread
    /// polls it) or by `lose`.
    pub(crate) lost: Arc<AtomicBool>,
    pub(crate) adapter: wgpu::Adapter,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) sampler: wgpu::Sampler,
    pub(crate) linear_sampler: wgpu::Sampler,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    /// The quads' pipeline for each output format drawn to.
    pipelines: Mutex<HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>>,
    /// 1x1 opaque white, used for solid color quads.
    pub(crate) white: UploadedResource,
    /// Held by whoever submits, and by a renderer while it configures its
    /// surface: wgpu fails `Surface::configure` if another thread submits
    /// while it waits for the GPU to go idle. (Chromium's GPU scheduler
    /// runs shared image and display work in turn, never at once.)
    pub(crate) submit_lock: Arc<Mutex<()>>,
    /// The copies into textures of pixels that came in shared memory,
    /// waiting for the next submit ([`GpuContext::take_staged`]): each
    /// from a buffer the pixels were copied into, once, straight out of
    /// the memory they were drawn in.
    staged: Mutex<Option<wgpu::CommandEncoder>>,
    /// The textures of shared images let go, for the next ones.
    pub(crate) pool: Arc<TexturePool>,
}

/// The kinds of wgpu's errors, as Viz handles them (Chromium's GPU process
/// loses the context on a client's error); the worst last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum GpuError {
    /// Something Viz handed wgpu that it should have checked first
    /// (`CompositorFrameSinkMsg::check`): machopo's bug. What it was of is
    /// dropped; debug builds panic, so that tests see the check missing.
    Validation,
    /// No memory for it: dropped, and the pool's textures let go, so the
    /// next may fit.
    OutOfMemory,
    /// wgpu's or the driver's own: the device is taken as lost, and the
    /// next draw makes another, as `about:gpucrash` does.
    Internal,
}

/// Handles `error` of `what` (a scope's, or one no scope took): logs it,
/// and does what its kind asks ([`GpuError`]).
pub(crate) fn report(
    error: &wgpu::Error,
    what: &str,
    lost: &AtomicBool,
    pool: &std::sync::Weak<TexturePool>,
) -> GpuError {
    match error {
        wgpu::Error::Validation { .. } => {
            tracing::error!(%error, what, "viz: wgpu's validation error: dropped");
            assert!(
                !cfg!(debug_assertions) || tolerating_validation(),
                "viz: wgpu's validation error, of {what}: a check missing before it: {error}"
            );
            GpuError::Validation
        }
        wgpu::Error::OutOfMemory { .. } => {
            tracing::error!(%error, what, "viz: the GPU's memory is out: dropped");
            if let Some(pool) = pool.upgrade() {
                pool.clear();
            }
            GpuError::OutOfMemory
        }
        wgpu::Error::Internal { .. } => {
            tracing::error!(%error, what, "viz: wgpu's internal error: the device is taken as lost");
            lost.store(true, Ordering::Release);
            GpuError::Internal
        }
    }
}

/// Whether a validation error is taken without a panic in a debug build:
/// only by Viz's tests of catching one.
#[cfg(not(test))]
fn tolerating_validation() -> bool {
    false
}

#[cfg(test)]
pub(crate) static TOLERATE_VALIDATION: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
fn tolerating_validation() -> bool {
    TOLERATE_VALIDATION.load(Ordering::Acquire)
}

/// cc's `ResourcePool`, on Viz's side: the textures of shared images their
/// clients and Viz have let go of, kept to be written over by the next
/// shared image of their size rather than made anew (making a tile's
/// texture and what draws it costs about as much as copying its pixels).
/// It keeps the newest, up to `POOL_BYTES` and `POOL_TEXTURES`: a window
/// resized leaves tiles of many sizes that are never asked for again.
#[derive(Default)]
pub(crate) struct TexturePool {
    free: Mutex<FreeTextures>,
}

#[derive(Default)]
struct FreeTextures {
    /// The oldest first.
    textures: VecDeque<UploadedResource>,
    bytes: usize,
}

/// How much the pool keeps: about a screen's worth of tiles twice over at
/// a device scale factor of 2 (cc's pool keeps what its tile manager's
/// memory limit leaves).
pub(crate) const POOL_BYTES: usize = 32 * 1024 * 1024;
/// And how many, whatever their size: taking one looks through them.
pub(crate) const POOL_TEXTURES: usize = 256;

impl TexturePool {
    fn free(&self) -> std::sync::MutexGuard<'_, FreeTextures> {
        self.free.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A texture of `size` to write over, the one put last.
    pub(crate) fn take(&self, size: Size) -> Option<UploadedResource> {
        let mut free = self.free();
        let at = free.textures.iter().rposition(|t| t.size == size)?;
        let texture = free.textures.remove(at)?;
        free.bytes = free.bytes.saturating_sub(texture_bytes(size));
        Some(texture)
    }

    /// Keeps `texture`, which nothing draws any more, letting the oldest
    /// go to stay within what the pool keeps.
    pub(crate) fn put(&self, texture: UploadedResource) {
        let bytes = texture_bytes(texture.size);
        // Not one that would push everything else out.
        if bytes > POOL_BYTES {
            return;
        }
        let mut free = self.free();
        free.bytes = free.bytes.saturating_add(bytes);
        free.textures.push_back(texture);
        while free.bytes > POOL_BYTES || free.textures.len() > POOL_TEXTURES {
            let Some(oldest) = free.textures.pop_front() else {
                break;
            };
            free.bytes = free.bytes.saturating_sub(texture_bytes(oldest.size));
        }
    }

    /// Lets every texture go (the GPU's memory is out).
    pub(crate) fn clear(&self) {
        let mut free = self.free();
        free.textures.clear();
        free.bytes = 0;
    }

    #[cfg(test)]
    pub(crate) fn sizes(&self) -> Vec<Size> {
        self.free().textures.iter().map(|t| t.size).collect()
    }
}

fn texture_bytes(size: Size) -> usize {
    size.checked_byte_size().unwrap_or(usize::MAX)
}

/// The process's device. A static is never dropped, so the Viz thread lets
/// it go as it ends (`release_shared`), having waited for the GPU: a device
/// still working as the process exits runs under the C++ destructors
/// `exit` runs. Mesa's lavapipe compiles shaders with LLVM on its own
/// threads as it draws, and LLVM's globals torn down under them crashed it.
static SHARED: Mutex<Option<Arc<GpuContext>>> = Mutex::new(None);

impl GpuContext {
    /// The process's device, made now if there is none yet. `surface`, a
    /// window's, must be one its adapter can draw to: the first context is
    /// made for it; if a later one can't be (a process has one window, or
    /// none, so this shouldn't happen), it gets a device of its own.
    pub(crate) fn shared(surface: Option<&wgpu::Surface<'_>>) -> Result<Arc<Self>, String> {
        let mut shared = SHARED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(context) = shared.as_ref() {
            match surface {
                Some(surface) if !context.adapter.is_surface_supported(surface) => {
                    tracing::error!(
                        "viz: the process's GPU adapter can't draw to this window; \
                         it gets a device of its own"
                    );
                    return Self::new(Some(surface), 0).map(Arc::new);
                }
                _ => return Ok(context.clone()),
            }
        }
        let context = Arc::new(Self::new(surface, 0)?);
        *shared = Some(context.clone());
        Ok(context)
    }

    /// The device that comes after `lost`'s (lost): made now, unless
    /// another renderer has made it already.
    pub(crate) fn replace_lost(
        lost: &GpuContext,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Arc<Self>, String> {
        let mut shared = SHARED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(context) = shared.as_ref()
            && context.generation > lost.generation
        {
            return Ok(context.clone());
        }
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "one generation a lost device: a u64 never runs out"
        )]
        let generation = lost.generation + 1;
        let context = Arc::new(Self::new(surface, generation)?);
        *shared = Some(context.clone());
        Ok(context)
    }

    /// The process is done drawing on `context`: it is no longer the
    /// process's device, and goes with the last renderer that holds it. A
    /// renderer made after this makes a new one.
    pub(crate) fn release_shared(context: &Arc<Self>) {
        let mut shared = SHARED.lock().unwrap_or_else(|e| e.into_inner());
        if shared
            .as_ref()
            .is_some_and(|shared| Arc::ptr_eq(shared, context))
        {
            *shared = None;
        }
    }

    /// Waits until the GPU has done everything submitted to the device.
    pub(crate) fn finish(&self) {
        if let Err(err) = self.device.poll(wgpu::PollType::wait_indefinitely()) {
            // A lost device has nothing more to do.
            tracing::debug!(%err, "viz: waiting for the GPU to finish");
        }
    }

    /// A new device, on the adapter [`AdapterPreference::from_env`] picks
    /// (one that can draw to `surface`, if any). Blocks while it is made.
    fn new(surface: Option<&wgpu::Surface<'_>>, generation: u64) -> Result<Self, String> {
        let instance = instance();
        let preference = AdapterPreference::from_env();
        let adapters: Vec<wgpu::Adapter> =
            pollster::block_on(instance.enumerate_adapters(BACKENDS))
                .into_iter()
                .filter(|adapter| surface.is_none_or(|s| adapter.is_surface_supported(s)))
                .collect();
        let types: Vec<_> = adapters.iter().map(|a| a.get_info().device_type).collect();
        let adapter = pick_adapter(&types, preference)
            .and_then(|i| adapters.get(i).cloned())
            .ok_or_else(|| match preference {
                AdapterPreference::Gpu => "no GPU adapter, and no CPU one either \
                    (WARP on Windows; on Linux, Mesa's lavapipe or llvmpipe)"
                    .to_owned(),
                AdapterPreference::Cpu => format!(
                    "{ADAPTER_ENV}=cpu, but there is no CPU adapter \
                     (WARP on Windows; on Linux, Mesa's lavapipe or llvmpipe)"
                ),
            })?;
        let info = adapter.get_info();
        tracing::info!(adapter = ?info, generation, "viz: using adapter");
        // Clients make textures up to this (a canvas's layer), and the
        // device would fail one past its own limit.
        let max_texture = adapter.limits().max_texture_dimension_2d;
        if max_texture < MAX_TEXTURE_SIZE {
            return Err(format!(
                "{} takes textures up to {max_texture} px, less than the {MAX_TEXTURE_SIZE} \
                 machopo needs",
                info.name
            ));
        }
        if info.device_type == wgpu::DeviceType::Cpu && preference == AdapterPreference::Gpu {
            tracing::warn!(
                name = info.name,
                "viz: no GPU; drawing on the CPU adapter, much slower"
            );
        }

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("viz"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| format!("request_device failed: {e}"))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viz quad shader"),
            source: wgpu::ShaderSource::Wgsl(
                format!("{SHADER}{}", crate::rounded_corner::ROUNDED_CLIP_WGSL).into(),
            ),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viz quad bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viz quad layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        // Tiles are drawn 1:1, so nearest keeps them pixel exact; a
        // canvas's layer is scaled, bilinearly.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("viz nearest"),
            ..Default::default()
        });
        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("viz linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = upload(
            &device,
            &queue,
            &bind_group_layout,
            &sampler,
            Size::new(1, 1),
            &[255, 255, 255, 255],
        );

        let lost = Arc::new(AtomicBool::new(false));
        let flag = lost.clone();
        device.set_device_lost_callback(move |reason, message| {
            tracing::warn!(?reason, %message, generation, "viz: the GPU device was lost");
            flag.store(true, Ordering::Release);
        });
        // What no error scope took (wgpu panics on it by default): handled
        // as a scope's would be, the operation it was of not known.
        let pool: Arc<TexturePool> = Arc::default();
        let (flag, freed) = (lost.clone(), Arc::downgrade(&pool));
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            report(&error, "something no scope was around", &flag, &freed);
        }));

        Ok(Self {
            generation,
            lost,
            adapter,
            device,
            queue,
            bind_group_layout,
            sampler,
            linear_sampler,
            shader,
            pipeline_layout,
            pipelines: Mutex::default(),
            white,
            submit_lock: Arc::new(Mutex::new(())),
            staged: Mutex::new(None),
            pool,
        })
    }

    /// Does `f` in wgpu's error scopes (on this thread: wgpu's scopes are
    /// each thread's own), and handles what they caught, as `what`'s
    /// ([`GpuError`]): the worst of it, if any, for the caller to drop
    /// what `f` made.
    pub(crate) fn scoped<T>(
        &self,
        what: &'static str,
        f: impl FnOnce() -> T,
    ) -> (T, Option<GpuError>) {
        let scopes = [
            wgpu::ErrorFilter::Validation,
            wgpu::ErrorFilter::OutOfMemory,
            wgpu::ErrorFilter::Internal,
        ]
        .map(|filter| self.device.push_error_scope(filter));
        let made = f();
        let mut worst = None;
        // A stack: the last pushed first. The native backend has the
        // errors at once (the future is ready).
        for scope in scopes.into_iter().rev() {
            if let Some(error) = pollster::block_on(scope.pop()) {
                worst = worst.max(Some(report(
                    &error,
                    what,
                    &self.lost,
                    &Arc::downgrade(&self.pool),
                )));
            }
        }
        (made, worst)
    }

    pub(crate) fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    /// Loses the device as a driver reset would (`about:gpucrash`): every
    /// renderer on it makes a new one at its next draw.
    pub(crate) fn lose(&self) {
        self.device.destroy();
        self.lost.store(true, Ordering::Release);
    }

    /// The quads' pipeline for drawing to `format`.
    pub(crate) fn pipeline(&self, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        let mut pipelines = self.pipelines.lock().unwrap_or_else(|e| e.into_inner());
        pipelines
            .entry(format)
            .or_insert_with(|| {
                self.device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("viz quad pipeline"),
                        layout: Some(&self.pipeline_layout),
                        vertex: wgpu::VertexState {
                            module: &self.shader,
                            entry_point: Some("vs_main"),
                            compilation_options: Default::default(),
                            buffers: &[Some(wgpu::VertexBufferLayout {
                                array_stride: std::mem::size_of::<Vertex>() as u64,
                                step_mode: wgpu::VertexStepMode::Vertex,
                                attributes: &wgpu::vertex_attr_array![
                                    0 => Float32x2,
                                    1 => Float32x2,
                                    2 => Float32x4,
                                    3 => Float32x4,
                                    4 => Float32x4,
                                    5 => Float32x4
                                ],
                            })],
                        },
                        primitive: wgpu::PrimitiveState::default(),
                        depth_stencil: None,
                        multisample: wgpu::MultisampleState::default(),
                        fragment: Some(wgpu::FragmentState {
                            module: &self.shader,
                            entry_point: Some("fs_main"),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format,
                                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                        }),
                        multiview_mask: None,
                        cache: None,
                    })
            })
            .clone()
    }

    /// `pixels` (premultiplied RGBA, `size`) as a texture to draw.
    pub(crate) fn upload<'a>(&self, size: Size, pixels: impl Into<Pixels<'a>>) -> UploadedResource {
        match pixels.into() {
            Pixels::Own(pixels) => upload(
                &self.device,
                &self.queue,
                &self.bind_group_layout,
                &self.sampler,
                size,
                pixels,
            ),
            Pixels::Shared(pixels) => {
                let texture = texture(&self.device, size);
                self.stage(&texture, size, pixels);
                resource(
                    &self.device,
                    &self.bind_group_layout,
                    &self.sampler,
                    size,
                    texture,
                )
            }
        }
    }

    /// `pixels` over what `resource`, of their size, had.
    pub(crate) fn write<'a>(&self, resource: &UploadedResource, pixels: impl Into<Pixels<'a>>) {
        match pixels.into() {
            Pixels::Own(pixels) => write(&self.queue, &resource.texture, resource.size, pixels),
            Pixels::Shared(pixels) => self.stage(&resource.texture, resource.size, pixels),
        }
    }

    /// `pixels` (of `size`, which they fit) on their way to `texture`,
    /// from memory whoever drew them shares and may still write: copied
    /// once, into a buffer of the GPU's mapped for it, which the next
    /// submit copies to the texture. `Queue::write_texture` copies the
    /// same once, but out of a slice, which that memory can't be.
    fn stage(&self, texture: &wgpu::Texture, size: Size, pixels: &SharedBuffer) {
        let _write = trace_event::trace_event!("SharedImage::StageTexture");
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "the pixels fit their size (`pixels_fit`): no wider than MAX_TEXTURE_SIZE, \
                      so 4 bytes a pixel, rounded up to the next 256, fit a u32"
        )]
        let (row, padded_row) = {
            let row = size.width * 4;
            (
                row,
                row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
            )
        };
        let making = trace_event::trace_event!("SharedImage::StageBuffer");
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viz shared pixels"),
            size: u64::from(padded_row).saturating_mul(u64::from(size.height)),
            usage: wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        drop(making);
        // A device that was lost makes a buffer that isn't one: nothing
        // is mapped, and there is nothing to unmap (which wgpu takes as
        // a fatal error). The device's textures went with it; the client
        // makes its images again on the next.
        let Ok(mut mapped) = buffer.get_mapped_range_mut(..) else {
            tracing::debug!("viz: no buffer for shared pixels: the device is lost");
            return;
        };
        let copying = trace_event::trace_event!("SharedImage::StageCopy");
        let copied = copy_rows(pixels, mapped.slice(..), row, padded_row);
        drop(copying);
        let unmapping = trace_event::trace_event!("SharedImage::StageUnmap");
        drop(mapped);
        buffer.unmap();
        drop(unmapping);
        if !copied {
            // Not all there (the caller checked they were): the texture
            // keeps what it had.
            return;
        }
        let _encoding = trace_event::trace_event!("SharedImage::StageEncode");
        let mut staged = self.staged.lock().unwrap_or_else(|e| e.into_inner());
        let encoder = staged.get_or_insert_with(|| {
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("viz shared pixels"),
                })
        });
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(size.height),
                },
            },
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// The copies staged since the last call, for whoever submits next
    /// to submit first: what it draws samples those textures.
    pub(crate) fn take_staged(&self) -> Option<wgpu::CommandBuffer> {
        let staged = self.staged.lock().unwrap_or_else(|e| e.into_inner()).take();
        staged.map(wgpu::CommandEncoder::finish)
    }

    /// What `resource` (of this device) is drawn with sampled bilinearly,
    /// made the first time it is.
    pub(crate) fn linear<'a>(&self, resource: &'a UploadedResource) -> &'a wgpu::BindGroup {
        resource.linear_bind_group.get_or_init(|| {
            bind_group(
                &self.device,
                &self.bind_group_layout,
                &resource.view,
                &self.linear_sampler,
            )
        })
    }
}

/// The pixels' rows, `row` bytes each, copied to `into` `padded_row`
/// bytes apart (a GPU copies rows out of a buffer only at multiples of
/// 256 bytes). `false`, with `into` partly written, when the buffer
/// hasn't that many rows or `into` no room for them.
///
/// `into` is memory that may only be written (a mapped buffer may be
/// write-combining), so it gives no slice to copy into: the bytes go by
/// value, 256 at a time, which costs what a copy does.
fn copy_rows(
    pixels: &SharedBuffer,
    mut into: wgpu::WriteOnly<'_, [u8]>,
    row: u32,
    padded_row: u32,
) -> bool {
    const RUN: usize = 256;
    let (Ok(row), Ok(padded_row)) = (usize::try_from(row), usize::try_from(padded_row)) else {
        return false;
    };
    if padded_row < row || !pixels.len().is_multiple_of(row) {
        return false;
    }
    let Some(rows) = pixels.len().checked_div(row) else {
        return false;
    };
    if rows.checked_mul(padded_row) != Some(into.len()) {
        return false;
    }
    // Rows with nothing between them (a tile's, 256 pixels wide): all
    // of them as one.
    let (row, padded_row, rows) = if row == padded_row {
        (pixels.len(), pixels.len(), usize::from(rows > 0))
    } else {
        (row, padded_row, rows)
    };
    let mut offset = 0usize;
    for _ in 0..rows {
        let Some(mut padded) = into.split_off(..padded_row) else {
            return false;
        };
        let (runs, rest) = padded.slice(..row).into_chunks::<RUN>();
        let whole = runs.len().saturating_mul(RUN);
        // A row is whole pixels, so what is left after the runs is too.
        let (pixels_left, _) = rest.into_chunks::<4>();
        let (Some(from_runs), Some(from_rest)) = (
            pixels.chunks::<RUN>(offset, runs.len()),
            pixels.chunks::<4>(offset.saturating_add(whole), pixels_left.len()),
        ) else {
            return false;
        };
        runs.write_iter(from_runs);
        pixels_left.write_iter(from_rest);
        offset = offset.saturating_add(row);
    }
    true
}

/// A texture for a resource of `size`, with nothing in it yet.
fn texture(device: &wgpu::Device, size: Size) -> wgpu::Texture {
    let _create = trace_event::trace_event!("SharedImage::CreateTexture");
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("viz resource"),
        size: wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    nearest: &wgpu::Sampler,
    size: Size,
    pixels: &[u8],
) -> UploadedResource {
    let texture = texture(device, size);
    write(queue, &texture, size, pixels);
    resource(device, layout, nearest, size, texture)
}

/// `texture`, of `size`, as what quads of it are drawn with.
fn resource(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    nearest: &wgpu::Sampler,
    size: Size,
    texture: wgpu::Texture,
) -> UploadedResource {
    let _bind = trace_event::trace_event!("SharedImage::BindGroups");
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    UploadedResource {
        size,
        bind_group: bind_group(device, layout, &view, nearest),
        linear_bind_group: Arc::default(),
        view,
        texture,
    }
}

/// `pixels` (premultiplied RGBA, `size`) as the whole of `texture`, which
/// is that size.
fn write(queue: &wgpu::Queue, texture: &wgpu::Texture, size: Size, pixels: &[u8]) {
    let _write = trace_event::trace_event!("SharedImage::WriteTexture");
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "every caller's pixels fit their size (`pixels_fit`, or a 1x1 white): \
                  no wider than MAX_TEXTURE_SIZE, so 4 bytes a pixel fit a u32"
    )]
    let bytes_per_row = size.width * 4;
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: Some(size.height),
        },
        wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
    );
}

/// What a quad of a resource is drawn with: its texture, sampled by
/// `sampler`.
fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("viz resource bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::DeviceType::*;

    #[test]
    fn the_adapter_is_a_gpu_wherever_there_is_one() {
        let gpu = AdapterPreference::Gpu;
        // The discrete GPU first, wherever it was found.
        assert_eq!(
            pick_adapter(&[Cpu, IntegratedGpu, DiscreteGpu], gpu),
            Some(2)
        );
        assert_eq!(pick_adapter(&[Cpu, IntegratedGpu], gpu), Some(1));
        assert_eq!(pick_adapter(&[Cpu, VirtualGpu, Other], gpu), Some(2));
        assert_eq!(pick_adapter(&[Cpu, VirtualGpu], gpu), Some(1));
        // Of two alike, the first found.
        assert_eq!(pick_adapter(&[DiscreteGpu, DiscreteGpu], gpu), Some(0));
        // No GPU: the CPU adapter.
        assert_eq!(pick_adapter(&[Cpu, Cpu], gpu), Some(0));
        assert_eq!(pick_adapter(&[], gpu), None);
    }

    #[test]
    fn asked_for_the_cpu_adapter_only_it_is_taken() {
        let cpu = AdapterPreference::Cpu;
        assert_eq!(
            pick_adapter(&[DiscreteGpu, Cpu, IntegratedGpu], cpu),
            Some(1)
        );
        assert_eq!(pick_adapter(&[DiscreteGpu, IntegratedGpu], cpu), None);
    }

    #[test]
    fn the_environment_asks_for_the_cpu_adapter_by_name() {
        assert_eq!(
            AdapterPreference::parse(Some("cpu")),
            AdapterPreference::Cpu
        );
        for gpu in [None, Some(""), Some("gpu"), Some("CPU"), Some("warp")] {
            assert_eq!(AdapterPreference::parse(gpu), AdapterPreference::Gpu);
        }
    }
}
