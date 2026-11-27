// Stage 0's probe of how wgpu fails under Viz's own drawing: tests to be
// included at the end of `crates/viz/src/wgpu_renderer.rs`'s last test
// module (`include!("../../../rfcs/0123-browser-survives-renderer/probe.rs");`),
// each run alone (`cargo test -p viz --lib probe_NAME -- --nocapture`), and
// taken out again. Not part of the build.

fn probe_device() -> Arc<GpuContext> {
    GpuContext::shared(None).expect("a device")
}

/// What an uncaptured error is, said and counted in place of the panic.
fn probe_count_uncaptured(device: &wgpu::Device) -> Arc<Mutex<Vec<String>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let into = seen.clone();
    device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
        into.lock().unwrap().push(format!("{error:?}").chars().take(160).collect());
    }));
    seen
}

fn probe_scopes(device: &wgpu::Device) -> [wgpu::ErrorScopeGuard; 3] {
    [
        device.push_error_scope(wgpu::ErrorFilter::Validation),
        device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
        device.push_error_scope(wgpu::ErrorFilter::Internal),
    ]
}

fn probe_pop(scopes: [wgpu::ErrorScopeGuard; 3]) -> Vec<String> {
    let mut errors = Vec::new();
    for scope in scopes.into_iter().rev() {
        if let Some(error) = pollster::block_on(scope.pop()) {
            errors.push(format!("{error:?}").chars().take(200).collect());
        }
    }
    errors
}

#[test]
fn probe_limits() {
    let _gpu = gpu();
    let context = probe_device();
    let limits = context.device.limits();
    eprintln!(
        "PROBE adapter={:?} max_buffer_size={} max_texture_dimension_2d={} vertex={}B",
        context.adapter.get_info().name,
        limits.max_buffer_size,
        limits.max_texture_dimension_2d,
        std::mem::size_of::<Vertex>(),
    );
}

/// One quad more than a vertex buffer of the device's largest holds.
#[test]
fn probe_many_quads_as_it_is() {
    let _gpu = gpu();
    let context = probe_device();
    let per_quad = 6 * std::mem::size_of::<Vertex>() as u64;
    let quads = (context.device.limits().max_buffer_size / per_quad + 1) as usize;
    eprintln!("PROBE {quads} quads, {} bytes of vertices", quads as u64 * per_quad);
    let size = Size::new(8, 8);
    let quad = solid(Color::WHITE, RectF::new(0.0, 0.0, 1.0, 1.0));
    let mut display = display(size);
    display.submit_compositor_frame(
        SINK,
        LocalSurfaceId::default(),
        frame(size, SharedQuadState::default(), vec![quad; quads]),
    );
    let drawn = display.draw_and_swap();
    eprintln!("PROBE drew: {drawn}");
}

/// The same, with the errors caught: does anything still panic?
#[test]
fn probe_many_quads_with_errors_caught() {
    let _gpu = gpu();
    let context = probe_device();
    let uncaptured = probe_count_uncaptured(&context.device);
    let per_quad = 6 * std::mem::size_of::<Vertex>() as u64;
    let quads = (context.device.limits().max_buffer_size / per_quad + 1) as usize;
    let size = Size::new(8, 8);
    let quad = solid(Color::WHITE, RectF::new(0.0, 0.0, 1.0, 1.0));
    let mut display = display(size);
    display.submit_compositor_frame(
        SINK,
        LocalSurfaceId::default(),
        frame(size, SharedQuadState::default(), vec![quad; quads]),
    );
    let scopes = probe_scopes(&context.device);
    let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| display.draw_and_swap()));
    eprintln!("PROBE drew: {drawn:?}");
    eprintln!("PROBE scoped errors: {:?}", probe_pop(scopes));
    eprintln!("PROBE uncaptured: {:?}", uncaptured.lock().unwrap());
}

/// A buffer too large, made as the upload path makes its own (no
/// `create_buffer_init`): what the scope says, and whether the device
/// draws after.
#[test]
fn probe_error_scope_and_the_device_after() {
    let _gpu = gpu();
    let context = probe_device();
    let uncaptured = probe_count_uncaptured(&context.device);
    let too_large = context.device.limits().max_buffer_size + 4;
    let scopes = probe_scopes(&context.device);
    let buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("probe"),
        size: too_large,
        usage: wgpu::BufferUsages::VERTEX,
        mapped_at_creation: true,
    });
    let mapped = buffer.get_mapped_range_mut(..).is_ok();
    eprintln!("PROBE too large a buffer: mapped={mapped} scoped={:?}", probe_pop(scopes));
    // A texture past the limit, and one of no size.
    for (width, height) in [(context.device.limits().max_texture_dimension_2d + 1, 1), (0, 1)] {
        let scopes = probe_scopes(&context.device);
        let _texture = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("probe"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        eprintln!("PROBE texture {width}x{height}: scoped={:?}", probe_pop(scopes));
    }
    eprintln!("PROBE uncaptured: {:?}", uncaptured.lock().unwrap());
    // The device after: a frame drawn and read back.
    let size = Size::new(2, 2);
    let mut display = display(size);
    display.submit_compositor_frame(
        SINK,
        LocalSurfaceId::default(),
        frame(size, SharedQuadState::default(), vec![solid(Color::rgb(0, 0, 255), RectF::new(0.0, 0.0, 2.0, 2.0))]),
    );
    let copy = draw(&mut display);
    eprintln!("PROBE the device after: pixel {:?}", px(&copy, 0, 0));
}

/// Quads, clips and texture coordinates of NaN and infinity: any error?
#[test]
fn probe_nan_and_infinity() {
    let _gpu = gpu();
    let context = probe_device();
    let uncaptured = probe_count_uncaptured(&context.device);
    let size = Size::new(4, 4);
    let odd = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, -f32::MAX, -0.0];
    let mut errors = Vec::new();
    let mut drawn = 0;
    for &a in &odd {
        for &b in &odd {
            let sqs = SharedQuadState {
                clip_rect: Some(RectF::new(a, b, b, a)),
                rounded_clip: Some(RRectF::new(RectF::new(b, a, a, b), [SizeF::new(a, b); 4])),
                opacity: a,
            };
            let mut f = frame(
                size,
                sqs,
                vec![
                    solid(Color::WHITE, RectF::new(a, b, a, b)),
                    DrawQuad {
                        rect: RectF::new(b, a, 1.0, 1.0),
                        shared_quad_state: 0,
                        material: Material::Texture {
                            resource_id: ResourceId(1),
                            tex_coord_rect: RectF::new(a, b, b, a),
                            nearest_neighbor: false,
                        },
                    },
                ],
            );
            f.metadata.device_scale_factor = a;
            f.render_pass_list[0].output_rect = RectF::new(a, b, a, b);
            f.resource_list.push(TransferableResource {
                id: ResourceId(1),
                size: Size::new(1, 1),
                format: ResourceFormat::Rgba8Premul,
                pixels: protocol::BigBuffer::from([1, 2, 3, 4]),
                shared_image: None,
            });
            let mut display = display(size);
            display.submit_compositor_frame(SINK, LocalSurfaceId::default(), f);
            let scopes = probe_scopes(&context.device);
            if display.draw_and_swap() {
                drawn += 1;
            }
            errors.extend(probe_pop(scopes));
        }
    }
    eprintln!("PROBE {drawn} of {} frames drew; scoped errors: {errors:?}", odd.len() * odd.len());
    eprintln!("PROBE uncaptured: {:?}", uncaptured.lock().unwrap());
}

/// What three error scopes around a frame's drawing cost.
#[test]
fn probe_error_scope_cost() {
    let _gpu = gpu();
    let context = probe_device();
    let size = Size::new(256, 256);
    let quads: Vec<DrawQuad> = (0..200)
        .map(|i| solid(Color::rgb(i as u8, 0, 0), RectF::new(i as f32, 0.0, 8.0, 8.0)))
        .collect();
    let mut display = display(size);
    let mut time = |scoped: bool| {
        let started = Instant::now();
        for _ in 0..2000 {
            display.submit_compositor_frame(
                SINK,
                LocalSurfaceId::default(),
                frame(size, SharedQuadState::default(), quads.clone()),
            );
            if scoped {
                let scopes = probe_scopes(&context.device);
                assert!(display.draw_and_swap());
                assert!(probe_pop(scopes).is_empty());
            } else {
                assert!(display.draw_and_swap());
            }
        }
        started.elapsed().as_secs_f64() * 1e6 / 2000.0
    };
    let (a, b, c, d) = (time(false), time(true), time(false), time(true));
    eprintln!("PROBE a frame of 200 quads: {a:.1} us and {c:.1} us without scopes, {b:.1} us and {d:.1} us with three");
    // The scopes alone.
    let started = Instant::now();
    for _ in 0..100_000 {
        assert!(probe_pop(probe_scopes(&context.device)).is_empty());
    }
    eprintln!("PROBE three scopes pushed and popped: {:.2} us", started.elapsed().as_secs_f64() * 1e6 / 100_000.0);
}

/// Textures until the adapter has no memory for one more. Run on the CPU
/// adapter under `ulimit -v` (a GPU's memory is the machine's own on an
/// integrated one): `PROBE_TEXTURES` is how many to try.
#[test]
fn probe_out_of_memory() {
    let _gpu = gpu();
    let context = probe_device();
    let uncaptured = probe_count_uncaptured(&context.device);
    let most: usize = std::env::var("PROBE_TEXTURES").ok().and_then(|n| n.parse().ok()).unwrap_or(0);
    let side = 4096;
    let pixels = vec![0x7f_u8; side as usize * side as usize * 4];
    let mut kept = Vec::new();
    let mut first = None;
    for i in 0..most {
        let scopes = probe_scopes(&context.device);
        let texture = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("probe"),
            size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        context.queue.write_texture(
            texture.as_image_copy(),
            &pixels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(side * 4), rows_per_image: None },
            wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
        );
        context.queue.submit([]);
        let errors = probe_pop(scopes);
        if !errors.is_empty() {
            first = Some((i, errors));
            break;
        }
        kept.push(texture);
    }
    eprintln!("PROBE {} textures of {side} made; then: {first:?}", kept.len());
    eprintln!("PROBE uncaptured: {:?}", uncaptured.lock().unwrap());
    eprintln!("PROBE lost: {}", context.is_lost());
    drop(kept);
    // The device after: a frame drawn and read back.
    let size = Size::new(2, 2);
    let mut display = display(size);
    display.submit_compositor_frame(
        SINK,
        LocalSurfaceId::default(),
        frame(size, SharedQuadState::default(), vec![solid(Color::rgb(0, 0, 255), RectF::new(0.0, 0.0, 2.0, 2.0))]),
    );
    let copy = draw(&mut display);
    eprintln!("PROBE the device after: pixel {:?}", px(&copy, 0, 0));
}

/// The same through Viz's own shared images (a buffer mapped when made,
/// copied to the texture), as a client's `CreateSharedImage`s get there.
#[test]
fn probe_out_of_memory_shared_images() {
    let _gpu = gpu();
    let context = probe_device();
    let uncaptured = probe_count_uncaptured(&context.device);
    let most: u64 = std::env::var("PROBE_TEXTURES").ok().and_then(|n| n.parse().ok()).unwrap_or(0);
    let size = Size::new(4096, 4096);
    let pixels = vec![0x7f_u8; 4096 * 4096 * 4];
    let mut display = display(Size::new(2, 2));
    let mut first = None;
    for i in 0..most {
        let scopes = probe_scopes(&context.device);
        display.create_shared_image(SINK, Mailbox(i + 1), size, &pixels);
        display.flush_shared_images();
        let errors = probe_pop(scopes);
        if !errors.is_empty() {
            first = Some((i, errors));
            break;
        }
    }
    eprintln!("PROBE shared images, then: {first:?}");
    eprintln!("PROBE uncaptured: {:?}", uncaptured.lock().unwrap());
    eprintln!("PROBE lost: {}", context.is_lost());
    display.submit_compositor_frame(
        SINK,
        LocalSurfaceId::default(),
        frame(Size::new(2, 2), SharedQuadState::default(), vec![solid(Color::rgb(0, 0, 255), RectF::new(0.0, 0.0, 2.0, 2.0))]),
    );
    let copy = draw(&mut display);
    eprintln!("PROBE the device after: pixel {:?}", px(&copy, 0, 0));
}

