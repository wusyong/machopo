//! What crosses the process boundary between the browser, the renderer and
//! Viz can be serialized, so the renderer could be a process of its own
//! later: checked when this compiles. The messages may
//! still carry what `#[serde(skip)]` drops; `python3 scripts/sanity.py
//! boundary` lists those exceptions.

use serde::Serialize;
use serde::de::DeserializeOwned;

use protocol::renderer::{CompositorMsg, RendererHostMsg, RendererMsg};
use protocol::viz::{CompositorFrameSinkMsg, VizMsg};

/// A message that may go to another process.
fn crosses<T: Serialize + DeserializeOwned + Send + 'static>() {}

#[test]
fn the_messages_across_the_boundary_can_be_serialized() {
    // Browser -> renderer's main thread, and back.
    crosses::<RendererMsg>();
    crosses::<RendererHostMsg>();
    // Browser and Viz -> renderer's compositor thread.
    crosses::<CompositorMsg>();
    // Compositor -> Viz.
    crosses::<CompositorFrameSinkMsg>();
    crosses::<VizMsg>();
}
