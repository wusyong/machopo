//! `BeginFrameSource`: who gets `BeginFrame`s, and when the next may go out
//! (cc's `BeginFrameSource` with Viz's `CompositorFrameSinkSupport` side
//! of it).

use std::collections::HashMap;

use base::SequenceNumber;
use protocol::viz::{BeginFrameArgs, CompositorFrameSinkMsg, FrameSinkId};

/// Delivers a `BeginFrame` to one client: the browser wires it to the
/// renderer's compositor thread (`WebContents::begin_frame_sender`), so
/// Viz needs no renderer types.
pub type BeginFrameClient = Box<dyn FnMut(BeginFrameArgs) + Send>;

struct Client {
    send: BeginFrameClient,
    /// It asked for `BeginFrame`s (`SetNeedsBeginFrame(true)`).
    needs: bool,
    /// It got one and hasn't answered yet (with a frame or
    /// `DidNotProduceFrame`): it gets no other until it does.
    pending: bool,
}

/// The clients that may ask for `BeginFrame`s. What paces them is up to
/// the owner: a window sends one per display frame, headless and tests
/// send the next as soon as every client has answered, on a clock of their
/// own.
#[derive(Default)]
pub struct BeginFrameSource {
    clients: HashMap<FrameSinkId, Client>,
    /// The next `BeginFrame`'s number, from 0: one a frame, a `u64` lasts.
    sequence: SequenceNumber<u64>,
}

impl BeginFrameSource {
    pub fn new() -> Self {
        Self::default()
    }

    /// `sink` may ask for `BeginFrame`s; they go to `send`.
    pub fn add_client(&mut self, sink: FrameSinkId, send: BeginFrameClient) {
        self.clients.insert(
            sink,
            Client {
                send,
                needs: false,
                pending: false,
            },
        );
    }

    /// `sink` has gone: it gets no more `BeginFrame`s, and an answer it
    /// owed holds up no other.
    pub fn remove_client(&mut self, sink: FrameSinkId) {
        self.clients.remove(&sink);
    }

    /// What `sink` sent Viz: whether it wants `BeginFrame`s, or its answer
    /// to the last one. Messages from other sinks are ignored.
    pub fn did_receive(&mut self, sink: FrameSinkId, msg: &CompositorFrameSinkMsg) {
        let Some(client) = self.clients.get_mut(&sink) else {
            return;
        };
        match msg {
            CompositorFrameSinkMsg::SetNeedsBeginFrame(needs) => client.needs = *needs,
            CompositorFrameSinkMsg::SubmitCompositorFrame { .. }
            | CompositorFrameSinkMsg::DidNotProduceFrame => {
                client.pending = false;
            }
        }
    }

    /// Whether some client wants a `BeginFrame` and has answered the last.
    pub fn wants_begin_frame(&self) -> bool {
        self.clients.values().any(|c| c.needs && !c.pending)
    }

    /// Whether a client is still working on a `BeginFrame`.
    pub fn is_pending(&self) -> bool {
        self.clients.values().any(|c| c.pending)
    }

    /// Sends a `BeginFrame` for `frame_time` (ms) to every client that
    /// wants one and has answered the last. Returns how many got it.
    pub fn issue(&mut self, frame_time: f64) -> usize {
        let args = BeginFrameArgs {
            sequence: self.sequence.peek(),
            frame_time,
        };
        let mut sent: usize = 0;
        for client in self.clients.values_mut().filter(|c| c.needs && !c.pending) {
            (client.send)(args);
            client.pending = true;
            sent = sent.saturating_add(1);
        }
        if sent > 0 {
            self.sequence.get_next();
        }
        sent
    }
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::unbounded;

    use super::*;

    fn sink(id: u32) -> FrameSinkId {
        FrameSinkId {
            client_id: id,
            sink_id: 1,
        }
    }

    #[test]
    fn begin_frames_go_to_clients_that_ask_once_they_have_answered() {
        let (tx, rx) = unbounded();
        let mut source = BeginFrameSource::new();
        source.add_client(sink(1), Box::new(move |args| tx.send(args).unwrap()));
        assert!(!source.wants_begin_frame(), "nobody asked");

        source.did_receive(sink(1), &CompositorFrameSinkMsg::SetNeedsBeginFrame(true));
        assert!(source.wants_begin_frame());
        assert_eq!(source.issue(0.0), 1);
        assert_eq!(rx.try_recv().unwrap().sequence, 0);
        assert!(!source.wants_begin_frame(), "the first is unanswered");
        assert_eq!(source.issue(16.0), 0);

        source.did_receive(sink(1), &CompositorFrameSinkMsg::DidNotProduceFrame);
        assert_eq!(source.issue(16.0), 1);
        assert_eq!(
            rx.try_recv().unwrap(),
            BeginFrameArgs {
                sequence: 1,
                frame_time: 16.0
            }
        );

        source.did_receive(sink(1), &CompositorFrameSinkMsg::SetNeedsBeginFrame(false));
        source.did_receive(sink(1), &CompositorFrameSinkMsg::DidNotProduceFrame);
        assert!(!source.wants_begin_frame(), "it stopped asking");
        // Another sink's messages change nothing.
        source.did_receive(sink(2), &CompositorFrameSinkMsg::SetNeedsBeginFrame(true));
        assert!(!source.wants_begin_frame());
    }
}
