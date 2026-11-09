//! Headless mode: run the full pipeline once and write the frame Viz would
//! have presented to a PNG, drawn by the software `DirectRenderer`.
//! End-to-end tests build on this.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base::numerics::saturated_cast;
use browser::{ContentsEvent, NetworkContext, NetworkContextParams, RendererFlags, WebContents};
use crossbeam_channel::unbounded;
use gfx::Size;
use protocol::TabId;
use protocol::input::InputEvent;
use protocol::network::{LoadResult, RedirectInfo};
use protocol::renderer::RendererHostMsg;
use protocol::viz::{CompositorFrameSinkMsg, CopyOutputRequest, SharedImageInterface, VizMsg};
use viz::{BeginFrameSource, Display, GpuSharedImages, WgpuRenderer};

#[derive(Debug, PartialEq)]
pub struct Options {
    pub out: PathBuf,
    /// Physical pixels.
    pub viewport_size: Size,
    pub device_scale_factor: f32,
    pub test_fonts: bool,
    /// `RendererFlags::num_raster_threads`.
    pub num_raster_threads: Option<usize>,
}

/// The first frame that shows a navigation fully loaded.
pub struct Frame {
    /// Physical pixels.
    pub size: Size,
    /// RGBA8, premultiplied, tightly packed.
    pub pixels: Vec<u8>,
    /// The last title the renderer reported, if any.
    pub title: Option<String>,
    /// The URL shown: the one committed, or the one a link to a fragment
    /// changed it to.
    pub url: Option<String>,
    /// Frames Viz drew for the navigation, this one included. More than
    /// one means the page showed before it finished loading.
    pub frames_drawn: usize,
    /// The subresources that failed to load (see `DidFinishLoad`).
    pub failed_subresources: Vec<String>,
}

const FRAME_TIMEOUT: Duration = Duration::from_secs(30);

/// Renders `url` and writes the frame to `opts.out`.
pub fn run(url: &str, opts: Options) -> Result<(), String> {
    let (frame, _) = drive(
        url,
        opts.viewport_size,
        opts.device_scale_factor,
        opts.test_fonts,
        &[],
        crate::SCRIPT_ENGINES,
        None,
        REFTEST_WAIT_TIMEOUT,
        NetworkContextParams::default(),
        opts.num_raster_threads,
    )?;
    if let Some(title) = &frame.title {
        tracing::info!(title = %title, "page title");
    }
    write_png(&opts.out, frame.size, &frame.pixels)
        .map_err(|e| format!("writing {}: {e}", opts.out.display()))?;
    tracing::info!(path = %opts.out.display(), size = ?frame.size, "wrote frame");
    Ok(())
}

/// What the headless loop waits on: frames for Viz, and messages from the
/// renderer. They come on two channels; forwarding both into one keeps
/// their order and lets one `recv` wait for either.
enum Event {
    Viz(VizMsg),
    Host(RendererHostMsg),
    /// The navigation reached a redirect.
    Redirected(u64, RedirectInfo),
    /// The navigation loaded, or failed.
    Completed(u64, LoadResult),
}

/// Navigates a new tab to `url` and returns the frame Viz draws once the
/// page and its subresources have loaded (`DidFinishLoad`). Each call gets
/// its own renderer (and so its own V8 isolate). `test_fonts`: see
/// `RendererFlags`.
pub fn render(
    url: &str,
    viewport_size: Size,
    device_scale_factor: f32,
    test_fonts: bool,
) -> Result<Frame, String> {
    render_after_input(url, viewport_size, device_scale_factor, test_fonts, &[])
}

/// A reftest's (or its reference's) screenshot, as machopo's reftests and
/// WPT's are taken: 800×600 CSS px at a scale of 1, the test fonts,
/// `reftest-wait` waited for up to `reftest_wait_timeout`.
pub fn screenshot(
    url: &str,
    network: NetworkContextParams,
    reftest_wait_timeout: Duration,
) -> Result<Frame, String> {
    drive(
        url,
        Size::new(800, 600),
        1.0,
        true,
        &[],
        crate::SCRIPT_ENGINES,
        None,
        reftest_wait_timeout,
        network,
        None,
    )
    .map(|(frame, _)| frame)
}

/// How many frames a loaded page may take to remove `reftest-wait`: ten
/// seconds of (virtual) animation time.
const MAX_REFTEST_WAIT_FRAMES: u32 = 600;

/// How long, in real time, a loaded page may take to remove
/// `reftest-wait`: WPT's timeout for a reftest (wptrunner's). The frames'
/// limit only counts while the page animates; one that has stopped asking
/// for frames may still remove it from a timer or an event, so it gets
/// this long. (Chromium's web tests likewise time out a test that never
/// finishes, after 6 s in a release build.)
pub const REFTEST_WAIT_TIMEOUT: Duration = Duration::from_secs(10);

/// Like [`render`], with `reftest_wait_timeout` instead of
/// [`REFTEST_WAIT_TIMEOUT`]: for tests of the timeout that don't wait as
/// long.
pub fn render_with_reftest_wait_timeout(
    url: &str,
    viewport_size: Size,
    reftest_wait_timeout: Duration,
) -> Result<Frame, String> {
    drive(
        url,
        viewport_size,
        1.0,
        true,
        &[],
        crate::SCRIPT_ENGINES,
        None,
        reftest_wait_timeout,
        NetworkContextParams::default(),
        None,
    )
    .map(|(frame, _)| frame)
}

/// Like [`render`], then sends `input` once the page has loaded and drawn,
/// and returns the frame that shows what it did, once the renderer has
/// handled it all (acknowledged every event, after sending what they led
/// to): the frame the last ack that waited for one names, or the frame
/// already drawn if none did; if the input started a navigation (a click
/// on a link), the first frame with that page loaded.
pub fn render_after_input(
    url: &str,
    viewport_size: Size,
    device_scale_factor: f32,
    test_fonts: bool,
    input: &[InputEvent],
) -> Result<Frame, String> {
    drive(
        url,
        viewport_size,
        device_scale_factor,
        test_fonts,
        input,
        crate::SCRIPT_ENGINES,
        None,
        REFTEST_WAIT_TIMEOUT,
        NetworkContextParams::default(),
        None,
    )
    .map(|(frame, _)| frame)
}

/// Navigates a new tab to the test page at `url` and returns what it
/// reported (`testRunner.reportHarnessResults`, through
/// `RendererHostMsg::TestharnessResults`), for an end-to-end test to check.
/// An error if it doesn't come within `deadline`, or the renderer goes.
/// `network` is how the tab's network service reaches the test server.
pub fn run_testharness(
    url: &str,
    deadline: Duration,
    network: NetworkContextParams,
) -> Result<String, String> {
    let (_, results) = drive(
        url,
        Size::new(800, 600),
        1.0,
        true,
        &[],
        crate::SCRIPT_ENGINES,
        Some(deadline),
        REFTEST_WAIT_TIMEOUT,
        network,
        None,
    )?;
    Ok(results.expect("waited for the results"))
}

/// What [`run_testharness`] fails with when no results came by the
/// deadline: a test that timed out.
pub const NO_TESTHARNESS_RESULTS: &str = "no testharness.js results before the deadline";

/// Like [`run_testharness`] (at `viewport_size`), with `input` sent once
/// the page has loaded and drawn, as [`render_after_input`] sends it: for
/// what the renderer does after the input's frames, which that doesn't
/// wait for.
pub fn run_testharness_after_input(
    url: &str,
    viewport_size: Size,
    input: &[InputEvent],
    deadline: Duration,
) -> Result<String, String> {
    let (_, results) = drive(
        url,
        viewport_size,
        1.0,
        true,
        input,
        crate::SCRIPT_ENGINES,
        Some(deadline),
        REFTEST_WAIT_TIMEOUT,
        NetworkContextParams::default(),
        None,
    )?;
    Ok(results.expect("waited for the results"))
}

/// The pipeline, driven until the page has loaded, drawn and handled
/// `input`; or, with `testharness` (a deadline), until it reports its
/// testharness.js results instead (after `input`, sent once it has loaded
/// and drawn). A loaded page gets
/// `reftest_wait_timeout` to remove `reftest-wait`.
#[allow(clippy::too_many_arguments)]
fn drive(
    url: &str,
    viewport_size: Size,
    device_scale_factor: f32,
    test_fonts: bool,
    input: &[InputEvent],
    script_engine: browser::ScriptEngineFactory,
    testharness: Option<Duration>,
    reftest_wait_timeout: Duration,
    network: NetworkContextParams,
    num_raster_threads: Option<usize>,
) -> Result<(Frame, Option<String>), String> {
    let deadline = testharness.map(crate::automation::deadline);
    let mut results = None;
    // Drawn on the process's GPU device, into a texture read back at the
    // end; tiles go straight onto the device (shared images).
    let shared_images = GpuSharedImages::new();
    let renderer = WgpuRenderer::offscreen(viewport_size, shared_images.clone())?;
    let (events_tx, events) = unbounded::<Event>();
    let (viz_tx, viz_rx) = unbounded::<VizMsg>();
    let forward = {
        let events_tx = events_tx.clone();
        std::thread::Builder::new()
            .name("HeadlessViz".into())
            .spawn(move || {
                for msg in viz_rx {
                    if events_tx.send(Event::Viz(msg)).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| format!("failed to start the frame forwarder: {e}"))?
    };
    let user_agent = browser::user_agent();
    let network = NetworkContext::start(NetworkContextParams {
        user_agent: user_agent.clone(),
        ..network
    })
    .map_err(|e| format!("failed to start the network service: {e}"))?;
    let mut web_contents = WebContents::new(
        TabId(0),
        RendererFlags {
            test_fonts,
            // Tests that use `reftest-wait` are captured once it goes.
            web_tests: true,
            // Each frame has the main thread's update: the same every run.
            wait_for_all_pipeline_stages_before_draw: true,
            automation: false,
            user_agent,
            // A web test's (content_shell's): the same on every platform.
            system_colors: gfx::SystemColors::DEFAULT,
            num_raster_threads,
            // A web test's: no blinking, so each frame comes out the same.
            caret_blink_interval: None,
        },
        script_engine,
        viz_tx,
        Some(shared_images as Arc<dyn SharedImageInterface>),
        network,
        Box::new(move |_tab, event| {
            let _ = events_tx.send(match event {
                ContentsEvent::Renderer(msg) => Event::Host(msg),
                ContentsEvent::NavigationRedirected {
                    navigation_id,
                    redirect,
                } => Event::Redirected(navigation_id, redirect),
                ContentsEvent::NavigationCompleted {
                    navigation_id,
                    result,
                } => Event::Completed(navigation_id, result),
            });
        }),
    )
    .map_err(|e| format!("failed to start renderer: {e}"))?;

    web_contents.resize(viewport_size, device_scale_factor);
    // As in Chrome's headless mode, the page has the focus.
    web_contents.set_focus(true);
    web_contents.navigate(url);

    let mut display = Display::new(renderer, web_contents.frame_sink_id());
    // `BeginFrame`s go out back to back: the next as soon as the renderer
    // has answered the last. Their times are virtual, 60 frames a second,
    // so animations come out the same on every run.
    let mut begin_frames = BeginFrameSource::new();
    begin_frames.add_client(
        web_contents.frame_sink_id(),
        Box::new(web_contents.begin_frame_sender()),
    );
    let mut frame_time = 0.0;
    let mut title = None;
    // The navigation `DidFinishLoad` was for and the token of the frame it
    // pointed at, once it came (with `reftest-wait`, once that ended).
    let mut loaded = None;
    // While the loaded page waits for `reftest-wait` to go: the
    // `BeginFrame`s sent since (an animation that never removes it would
    // run forever), and when its time runs out (a page that stops asking
    // for frames would wait for nothing).
    let mut reftest_waiting: Option<(u32, Instant)> = None;
    let mut failed_subresources = Vec::new();
    // The newest frame drawn so far.
    let mut drawn_token = None;
    let mut frames_drawn = 0;
    // Once the loaded page is drawn: the input goes out, and then every ack,
    // and the newest frame they name.
    let mut input_sent = input.is_empty();
    let mut acks = 0;
    let mut acked_frame = None;
    let result = loop {
        if let Some(deadline) = deadline
            && Instant::now() >= deadline
        {
            break Err(NO_TESTHARNESS_RESULTS.into());
        }
        if reftest_waiting.is_some_and(|(_, d)| Instant::now() >= d) {
            break Err(format!(
                "reftest-wait was still on after {} s",
                reftest_wait_timeout.as_secs_f32()
            ));
        }
        // Loaded and drawn, unless a newer navigation has started since.
        if let (Some((navigation_id, loaded)), Some(drawn)) = (loaded, drawn_token)
            && navigation_id == web_contents.latest_navigation_id()
            && drawn >= loaded
        {
            if !input_sent {
                for event in input {
                    web_contents.forward_input_event(event.clone());
                }
                input_sent = true;
            }
            // A wait for results goes on after the input.
            if deadline.is_none()
                && acks == input.len()
                && acked_frame.is_none_or(|acked| drawn >= acked)
            {
                break Ok(());
            }
        }
        if begin_frames.wants_begin_frame() {
            begin_frames.issue(frame_time);
            frame_time += 1000.0 / 60.0;
            if let Some((frames, _)) = &mut reftest_waiting {
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "it stops one past MAX_REFTEST_WAIT_FRAMES, below"
                )]
                {
                    *frames += 1;
                }
                if *frames > MAX_REFTEST_WAIT_FRAMES {
                    break Err(format!(
                        "reftest-wait was still on after {MAX_REFTEST_WAIT_FRAMES} frames"
                    ));
                }
            }
        }
        let wait = [deadline, reftest_waiting.map(|(_, d)| d)]
            .into_iter()
            .flatten()
            .fold(FRAME_TIMEOUT, |wait, d| {
                wait.min(d.saturating_duration_since(Instant::now()))
            });
        let event = events.recv_timeout(wait);
        if let Ok(Event::Viz(VizMsg::FrameSink(sink, msg))) = &event {
            begin_frames.did_receive(*sink, msg);
        }
        match event {
            Ok(Event::Viz(VizMsg::FrameSink(
                _,
                CompositorFrameSinkMsg::SetNeedsBeginFrame(_)
                | CompositorFrameSinkMsg::DidNotProduceFrame,
            ))) => {}
            Ok(Event::Viz(VizMsg::FrameSink(
                sink,
                CompositorFrameSinkMsg::SubmitCompositorFrame {
                    local_surface_id,
                    frame,
                },
            ))) => {
                let token = frame.metadata.frame_token;
                display.submit_compositor_frame(sink, local_surface_id, frame);
                if display.needs_draw() && display.draw_and_swap() {
                    drawn_token = Some(token);
                    // A count for tests: wrapping only miscounts.
                    frames_drawn = usize::wrapping_add(frames_drawn, 1);
                }
            }
            // Headless has one tab, its client from the start.
            Ok(Event::Viz(
                VizMsg::Display(_)
                | VizMsg::RegisterFrameSinkId(..)
                | VizMsg::InvalidateFrameSinkId(_),
            )) => {}
            Ok(Event::Redirected(navigation_id, redirect)) => {
                web_contents.on_navigation_redirected(navigation_id, &redirect);
            }
            Ok(Event::Completed(navigation_id, result)) => {
                web_contents.on_navigation_completed(navigation_id, result);
            }
            Ok(Event::Host(msg)) => {
                web_contents.on_renderer_message(&msg);
                match msg {
                    RendererHostMsg::UpdateTitle(t) => title = Some(t),
                    RendererHostMsg::InputEventAck { frame_token } => {
                        // Only compared with the inputs sent.
                        acks = usize::saturating_add(acks, 1);
                        acked_frame = acked_frame.max(frame_token);
                    }
                    RendererHostMsg::DidFinishLoad {
                        navigation_id,
                        frame_token: Some(token),
                        failed_subresources: failed,
                        reftest_wait,
                    } => {
                        failed_subresources = failed;
                        if reftest_wait {
                            reftest_waiting =
                                Some((0, crate::automation::deadline(reftest_wait_timeout)));
                        } else {
                            loaded = Some((navigation_id, token));
                        }
                    }
                    RendererHostMsg::ReftestWaitEnded {
                        navigation_id,
                        frame_token: Some(token),
                    } => {
                        reftest_waiting = None;
                        loaded = Some((navigation_id, token));
                    }
                    RendererHostMsg::DidFinishLoad {
                        frame_token: None, ..
                    }
                    | RendererHostMsg::ReftestWaitEnded {
                        frame_token: None, ..
                    } => break Err("the renderer drew no frame for the loaded page".into()),
                    RendererHostMsg::TestharnessResults {
                        navigation_id,
                        results: reported,
                    } if deadline.is_some()
                        && navigation_id == web_contents.latest_navigation_id() =>
                    {
                        results = Some(reported);
                        break Ok(());
                    }
                    RendererHostMsg::RendererGone => {
                        break Err("the renderer exited before the page loaded".into());
                    }
                    _ => {}
                }
            }
            // Only the deadline ends a wait for results.
            Err(_) if deadline.is_some() => {}
            // A wait for `reftest-wait` ends at the top of the loop, once
            // its time is up.
            Err(e) if e.is_timeout() && reftest_waiting.is_some() => {}
            Err(e) => {
                break Err(format!(
                    "the page didn't finish loading and drawing: {e} (loaded: {loaded:?}, drawn: {drawn_token:?})"
                ));
            }
        }
    };
    let url = web_contents.url().map(str::to_owned);
    web_contents.shutdown();
    // The renderer has gone, so the frame forwarder's channel closes.
    drop(web_contents);
    drop(events);
    let _ = forward.join();
    result?;

    // The last frame drawn, drawn again and read back; transparent if
    // there was none.
    let copy = Arc::new(Mutex::new(None));
    let into = copy.clone();
    display.request_copy_of_output(CopyOutputRequest::new(move |result| {
        *into.lock().unwrap_or_else(|e| e.into_inner()) = result;
    }));
    display.draw_and_swap();
    let copied = copy.lock().unwrap_or_else(|e| e.into_inner()).take();
    let (size, pixels) = match copied {
        Some(copied) => (copied.size, copied.pixels),
        None => (viewport_size, vec![0; transparent_len(viewport_size)]),
    };
    let frame = Frame {
        size,
        pixels,
        title,
        url,
        frames_drawn,
        failed_subresources,
    };
    Ok((frame, results))
}

/// Writes premultiplied RGBA8 as a (straight-alpha) PNG.
pub fn write_png(path: &Path, size: Size, premul: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    encode_png(BufWriter::new(File::create(path)?), size, premul)
}

/// Premultiplied RGBA8 as a (straight-alpha) PNG, into `out`.
pub fn encode_png(
    out: impl std::io::Write,
    size: Size,
    premul: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut straight = premul.to_vec();
    for px in straight.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                // c <= a for a premultiplied channel, so this is at most 255.
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "c, a <= 255: c * 255 + a / 2 fits a u32, and a > 0"
                )]
                let straight = (*c as u32 * 255 + a / 2) / a;
                *c = saturated_cast(straight);
            }
        }
    }
    let mut encoder = png::Encoder::new(out, size.width, size.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&straight)?;
    writer.finish()?;
    Ok(())
}
/// The bytes of a transparent frame of `size`: one Viz could draw, its
/// texture no bigger than its device takes.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "a viewport Viz drew into a texture: its sides are within the device's limit"
)]
fn transparent_len(size: Size) -> usize {
    size.width as usize * size.height as usize * 4
}
