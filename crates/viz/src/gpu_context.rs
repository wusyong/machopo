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

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use gfx::Size;
use protocol::viz::MAX_TEXTURE_SIZE;

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
}

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
            .map(|i| adapters[i].clone())
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
            [&sampler, &linear_sampler],
            Size::new(1, 1),
            &[255, 255, 255, 255],
        );

        let lost = Arc::new(AtomicBool::new(false));
        let flag = lost.clone();
        device.set_device_lost_callback(move |reason, message| {
            tracing::warn!(?reason, %message, generation, "viz: the GPU device was lost");
            flag.store(true, Ordering::Release);
        });

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
        })
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
    pub(crate) fn upload(&self, size: Size, pixels: &[u8]) -> UploadedResource {
        upload(
            &self.device,
            &self.queue,
            &self.bind_group_layout,
            [&self.sampler, &self.linear_sampler],
            size,
            pixels,
        )
    }
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    [nearest, linear]: [&wgpu::Sampler; 2],
    size: Size,
    pixels: &[u8],
) -> UploadedResource {
    let extent = wgpu::Extent3d {
        width: size.width,
        height: size.height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("viz resource"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "every caller's pixels fit their size (`pixels_fit`, or a 1x1 white): \
                  no wider than MAX_TEXTURE_SIZE, so 4 bytes a pixel fit a u32"
    )]
    let bytes_per_row = size.width * 4;
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
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
        extent,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = |sampler: &wgpu::Sampler| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viz resource bg"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    };
    UploadedResource {
        size,
        bind_group: bind_group(nearest),
        linear_bind_group: bind_group(linear),
        _texture: texture,
    }
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
