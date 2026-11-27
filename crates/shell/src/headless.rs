//! The full pipeline, headless, for end-to-end tests: a page loaded in a
//! tab of its own, and the frame Viz would have presented read back, drawn
//! on the process's GPU device. Its `BeginFrame`s are virtual, so a page
//! comes out the same on every run (as content_shell's web test mode).

use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base::numerics::saturated_cast;
use browser::{ContentsEvent, NetworkContextParams, RendererFlags, StoragePartition, WebContents};
use crossbeam_channel::unbounded;
use gfx::Size;
use protocol::TabId;
use protocol::input::InputEvent;
use protocol::renderer::RendererHostMsg;
use protocol::viz::{CompositorFrameSinkMsg, ContextProvider, CopyOutputRequest, VizMsg};
use viz::{BeginFrameSource, Display, GpuSharedImages, WgpuRenderer};

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
    /// one means the page showed before it finished loading; how many it
    /// takes to load depends on when the begin frames come (a main frame
    /// is asked for as each task ends), so a test of what was drawn before
    /// the load reads `first_pixels`.
    pub frames_drawn: usize,
    /// Of them, those after the frame `DidFinishLoad` named (the page's
    /// `load` and what came of it): what the page did once loaded.
    pub frames_drawn_after_load: usize,
    /// The first frame drawn, as `pixels` (empty if none was): what showed
    /// before anything else did; and how long after the navigation began.
    pub first_pixels: Vec<u8>,
    pub first_drawn_after: Option<Duration>,
    /// The subresources that failed to load (see `DidFinishLoad`).
    pub failed_subresources: Vec<String>,
}

const FRAME_TIMEOUT: Duration = Duration::from_secs(30);

/// What the headless loop waits on: frames for Viz, and messages from the
/// renderer. They come on two channels; forwarding both into one keeps
/// their order and lets one `recv` wait for either.
#[expect(
    clippy::large_enum_variant,
    reason = "every renderer message is a `Host`: boxing it would allocate for each, to save \
              room only in the few others"
)]
enum Event {
    Viz(VizMsg),
    /// From a renderer: the tab's, or one it ended.
    Host(browser::RendererId, RendererHostMsg),
    /// What a renderer's IO thread found bad.
    Bad(browser::RendererId, browser::BadMessageReason),
    /// A frame's navigation's redirect, response, body's data or end.
    /// Boxed: a navigation's response is much bigger than the other events.
    Navigation(Box<ContentsEvent>),
    /// What headless doesn't look at.
    Other,
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
/// to) and run the tasks it left: a frame asked for once the renderer has
/// no task and wants no frame (`RendererMsg::RequestIdleFrame`), or the
/// frame already drawn if none went; if the input started a navigation (a
/// click on a link), one with that page loaded.
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
    let network = NetworkContextParams {
        user_agent: user_agent.clone(),
        ..network
    };
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
            // The compositor's default: a web test's viewport (800x600) is
            // given the least anyway.
            tile_memory_limit: None,
            // A web test's: no blinking, so each frame comes out the same.
            caret_blink_interval: None,
            parser_budget: Default::default(),
            new_content_rendering_delay: browser::NEW_CONTENT_RENDERING_DELAY,
        },
        browser::RendererLauncher::InProcess(script_engine),
        viz_tx,
        Some(shared_images as Arc<dyn ContextProvider>),
        StoragePartition::new(&browser::Profile::in_memory(), network)
            .map_err(|e| format!("failed to start the storage and the network service: {e}"))?,
        Box::new(move |_tab, event| {
            let _ = events_tx.send(match event {
                ContentsEvent::Renderer { renderer, msg } => Event::Host(renderer, msg),
                ContentsEvent::BadMessage { renderer, reason } => Event::Bad(renderer, reason),
                ContentsEvent::NavigationStarted(_) => Event::Other,
                event => Event::Navigation(Box::new(event)),
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
    // Each renderer's sink's way back to it, for the pixels of its shared
    // images: the tab has the one it was made with, and another for each
    // renderer that takes the tab's place (a navigation to another site).
    // Those whose first frame has come, which each is told once.
    let mut first_frames = std::collections::HashSet::new();
    let mut clients = HashMap::from([(
        web_contents.frame_sink_id(),
        web_contents.begin_frame_observer(),
    )]);
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
    // Each frame drawn's token, the one `DidFinishLoad` named, and the
    // first frame read back as it is drawn.
    let mut drawn_tokens = Vec::new();
    let mut load_token = None;
    let first_copy = Arc::new(Mutex::new(None));
    let started = Instant::now();
    let mut first_drawn_after = None;
    // Once the loaded page is drawn: the input goes out, and then every ack,
    // and the newest frame they name.
    let mut input_sent = input.is_empty();
    let mut acks = 0;
    let mut acked_frame = None;
    // Then a frame asked for once the renderer is idle (for which loaded
    // navigation), and its answer: the frame it names, if one went.
    let mut idle_frame_asked = None;
    let mut idle_frame: Option<Option<u64>> = None;
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
                if input.is_empty() {
                    break Ok(());
                }
                // What the input led to may go on in tasks after the frame
                // its ack names (a link's `hashchange`): a frame once the
                // renderer has none left.
                if idle_frame_asked != Some(navigation_id) {
                    web_contents.request_idle_frame();
                    idle_frame_asked = Some(navigation_id);
                    idle_frame = None;
                }
                if idle_frame.is_some_and(|frame| frame.is_none_or(|frame| drawn >= frame)) {
                    break Ok(());
                }
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
                if let Some(client) = clients.get(&sink)
                    && first_frames.insert(sink)
                {
                    client.on_first_frame();
                }
                if frames_drawn == 0 && display.needs_draw() {
                    let into = first_copy.clone();
                    display.request_copy_of_output(CopyOutputRequest::new(move |result| {
                        *into.lock().unwrap_or_else(|e| e.into_inner()) = result;
                    }));
                }
                if display.needs_draw() && display.draw_and_swap() {
                    drawn_token = Some(token);
                    drawn_tokens.push(token);
                    first_drawn_after.get_or_insert_with(|| started.elapsed());
                    // A count for tests: wrapping only miscounts.
                    frames_drawn = usize::wrapping_add(frames_drawn, 1);
                }
            }
            Ok(Event::Viz(VizMsg::FrameSink(
                sink,
                CompositorFrameSinkMsg::CreateSharedImage {
                    mailbox,
                    size,
                    pixels,
                },
            ))) => {
                display.create_shared_image(sink, mailbox, size, &pixels);
                display.flush_shared_images();
                // The buffer is the renderer's again.
                drop(pixels);
                if let Some(client) = clients.get(&sink) {
                    client.on_pixels_returned(mailbox);
                }
            }
            Ok(Event::Viz(VizMsg::FrameSink(
                sink,
                CompositorFrameSinkMsg::DestroySharedImage { mailbox },
            ))) => display.destroy_shared_image(sink, mailbox),
            // Headless has one tab, whose surface fills the output: a
            // renderer that takes its place (a navigation to another site)
            // has a sink of its own, which the output shows from then on,
            // and the one it replaced goes (a renderer gone ends the run).
            Ok(Event::Viz(VizMsg::RegisterFrameSinkId(sink, observer))) => {
                display.set_root_frame_sink(sink);
                let begin_frame = observer.clone();
                begin_frames
                    .add_client(sink, Box::new(move |args| begin_frame.on_begin_frame(args)));
                clients.insert(sink, observer);
            }
            Ok(Event::Viz(VizMsg::InvalidateFrameSinkId(sink))) => {
                begin_frames.remove_client(sink);
                clients.remove(&sink);
                display.remove_frame_sink(sink);
            }
            Ok(Event::Viz(VizMsg::Display(_))) => {}
            Ok(Event::Navigation(event)) => web_contents.on_navigation_event(*event),
            Ok(Event::Bad(renderer, reason)) => {
                if web_contents.is_current_renderer(renderer) {
                    web_contents.received_bad_message(reason);
                }
            }
            Ok(Event::Other) => {}
            // What a renderer the tab ended still sent counts for nothing.
            Ok(Event::Host(renderer, _)) if !web_contents.is_current_renderer(renderer) => {}
            Ok(Event::Host(_, msg)) => {
                web_contents.on_renderer_message(&msg);
                match msg {
                    RendererHostMsg::UpdateTitle(t) => title = Some(t),
                    RendererHostMsg::IdleFrameReady { frame_token } => {
                        idle_frame = Some(frame_token);
                    }
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
                        load_token = Some(token);
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
    let url = web_contents.last_committed_url().map(str::to_owned);
    // Its script stopped if it runs on, and waited for with machopo's
    // limit: a test of a page that never returns ends all the same.
    let asked = std::time::Instant::now();
    web_contents.ask_to_shut_down();
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "seconds from now: within any clock's range"
    )]
    let deadline = asked + browser::shutdown::LIMIT;
    let left = web_contents.wait_for_shutdown(asked, deadline);
    // The renderer has gone, so the frame forwarder's channel closes; one
    // left running keeps it open, and the forwarder with it.
    drop(web_contents);
    drop(events);
    if left.is_none() {
        let _ = forward.join();
    }
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
    let first_pixels = first_copy
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .map(|copied| copied.pixels)
        .unwrap_or_default();
    let frames_drawn_after_load = load_token.map_or(0, |load| {
        drawn_tokens.iter().filter(|&&token| token > load).count()
    });
    let frame = Frame {
        size,
        pixels,
        title,
        url,
        frames_drawn,
        frames_drawn_after_load,
        first_pixels,
        first_drawn_after,
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
