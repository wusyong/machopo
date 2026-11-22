//! What crosses the process boundary between the browser, the renderer and
//! Viz can be serialized, so the renderer could be a process of its own
//! later: checked when this compiles. A data pipe in a message is one of
//! them (it crosses with the message, by `ipc`'s codec alone). What a
//! message between the browser and Viz, which share a process, may still
//! carry that `#[serde(skip)]` drops is listed by `python3
//! scripts/sanity.py boundary`.

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
