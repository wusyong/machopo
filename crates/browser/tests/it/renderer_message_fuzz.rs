//! Whatever a renderer sends, the browser's side of a tab takes or
//! refuses, and goes on: the checks the fuzzer runs
//! (`fuzz/fuzz_targets/renderer_message.rs`), which are these, so what it
//! finds is replayed here (`fuzz/regressions/`).
//!
//! The renderer has nothing at its end (`RendererLauncher::Detached`): an
//! input is the messages it "sends", each handled as one off its channel
//! is (the IO thread's object first, then the tab), with what a browser
//! does in between: navigate somewhere, lose its renderer. The network
//! reaches nothing, so no input makes a request that leaves the process.
//!
//! What it looks for is a panic: a renderer that can end the browser ends
//! every tab with it. A refused message ends the renderer, which is right.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use base::AtomicSequenceNumber;
use browser::{
    BadMessageReason, ContentsEvent, NetworkContextParams, Profile, RendererLauncher,
    StoragePartition, WebContents,
};
use crossbeam_channel::{Receiver, unbounded};
use ipc::codec::{self, NoRegions};
use protocol::TabId;
use protocol::renderer::RendererHostMsg;
use protocol::viz::VizMsg;

/// What an input came to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// How many of its messages were messages.
    pub delivered: usize,
    /// What the browser found wrong with them, in order.
    pub refused: Vec<BadMessageReason>,
}

/// The profile's storage every input shares: a network that reaches
/// nothing, in memory. Shared because starting one is most of what a run
/// would cost; an input's cookies and blob URLs are there for the next,
/// which makes a find depend on the inputs before it only if it is about
/// those.
fn partition() -> StoragePartition {
    static PARTITION: OnceLock<StoragePartition> = OnceLock::new();
    PARTITION
        .get_or_init(|| {
            let params = NetworkContextParams {
                host_resolver_rules: browser::HostMappingRules::parse("MAP * ^NOTFOUND")
                    .expect("the rule parses"),
                ..Default::default()
            };
            StoragePartition::new(&Profile::in_memory(), params).expect("the storage starts")
        })
        .clone()
}

static TABS: AtomicSequenceNumber<u32> = AtomicSequenceNumber::<u32>::starting_at(1 << 20);

/// A tab whose renderer has nothing at its end, and its events.
fn tab() -> (WebContents, Receiver<ContentsEvent>, Receiver<VizMsg>) {
    let (viz, viz_rx) = unbounded();
    let (events, events_rx) = unbounded();
    let events = Arc::new(Mutex::new(events));
    let web_contents = WebContents::new(
        TabId(TABS.get_next()),
        browser::RendererFlags {
            test_fonts: true,
            web_tests: false,
            wait_for_all_pipeline_stages_before_draw: true,
            automation: false,
            user_agent: browser::user_agent(),
            system_colors: gfx::SystemColors::DEFAULT,
            num_raster_threads: None,
            caret_blink_interval: None,
            parser_budget: Default::default(),
            new_content_rendering_delay: browser::NEW_CONTENT_RENDERING_DELAY,
        },
        RendererLauncher::Detached,
        viz,
        None,
        partition(),
        Box::new(move |_tab, event| {
            let _ = events.lock().unwrap_or_else(|e| e.into_inner()).send(event);
        }),
    )
    .expect("a tab with no renderer starts");
    (web_contents, events_rx, viz_rx)
}

/// Does with an event what an embedder does.
fn handle(web_contents: &mut WebContents, event: ContentsEvent, outcome: &mut Outcome) {
    match event {
        ContentsEvent::Renderer { renderer, msg } => {
            if web_contents.is_current_renderer(renderer) {
                web_contents.on_renderer_message(&msg);
            }
        }
        ContentsEvent::BadMessage { renderer, reason } => {
            if web_contents.is_current_renderer(renderer) {
                outcome.refused.push(reason);
                web_contents.received_bad_message(reason);
            }
        }
        ContentsEvent::NavigationStarted(_) => {}
        event => web_contents.on_navigation_event(event),
    }
}

/// One of a few places a browser navigates a tab to, picked by `bytes`:
/// never a file, and never a URL that does something instead of loading.
fn url(bytes: &[u8]) -> String {
    let text: String = bytes
        .iter()
        .map(|b| char::from(b'a' + b % 26))
        .take(24)
        .collect();
    match bytes.first().map_or(0, |b| b % 4) {
        0 => "about:blank".to_owned(),
        1 => format!("data:text/html,<title>{text}</title>"),
        2 => format!("http://{text}.test/{text}"),
        _ => format!("https://127.0.0.1:1/{text}"),
    }
}

/// One input: steps of an operation, a length (two bytes) and that many
/// bytes.
pub fn run(data: &[u8]) -> Outcome {
    let (mut web_contents, events, _viz) = tab();
    let mut outcome = Outcome::default();
    let mut rest = data;
    while let [op, low, high, after @ ..] = rest {
        let len = usize::from(u16::from_le_bytes([*low, *high])).min(after.len());
        let (bytes, after) = after.split_at(len);
        rest = after;
        match op % 16 {
            // The browser navigates.
            12 | 13 => web_contents.navigate(url(bytes)),
            // The renderer goes, as one whose channel is over.
            14 => web_contents.deliver_renderer_gone(),
            // What the network was asked comes back (that nothing is
            // there): waited for a moment, since it comes from its thread.
            15 => {
                if let Ok(event) = events.recv_timeout(Duration::from_millis(20)) {
                    handle(&mut web_contents, event, &mut outcome);
                }
            }
            // The renderer sends: what isn't a message its channel would
            // have refused, which is the channel's to do (`ipc`'s fuzzers).
            _ => {
                if let Ok(msg) = codec::decode::<RendererHostMsg>(bytes, &mut NoRegions) {
                    outcome.delivered += 1;
                    web_contents.deliver_from_renderer(msg);
                }
            }
        }
        while let Ok(event) = events.try_recv() {
            handle(&mut web_contents, event, &mut outcome);
        }
    }
    web_contents.shutdown();
    outcome
}

/// An input of the renderer's messages `msgs`, one after another.
fn sends(msgs: &[RendererHostMsg]) -> Vec<u8> {
    let mut input = Vec::new();
    for msg in msgs {
        let mut payload = Vec::new();
        codec::encode(msg, &mut NoRegions, &mut payload).expect("the message serializes");
        input.push(0);
        input.extend_from_slice(&u16::try_from(payload.len()).unwrap().to_le_bytes());
        input.extend_from_slice(&payload);
    }
    input
}

#[test]
fn an_inputs_messages_reach_the_tab() {
    // The harness does what it says: a message goes through the checks
    // where a channel is read and on to the tab, which has its say.
    let title = RendererHostMsg::UpdateTitle("from nowhere".into());
    assert_eq!(
        run(&sends(&[title])),
        Outcome {
            delivered: 1,
            refused: vec![]
        }
    );
    // Refused where the channel is read: cookies through a loader the
    // browser never gave.
    let cookie = RendererHostMsg::SetCookie {
        loader: 999,
        url: "https://example.test/".into(),
        line: "a=b".into(),
    };
    assert_eq!(
        run(&sends(&[cookie])).refused,
        [BadMessageReason::UnknownLoader]
    );
    // Bytes that are no message are none delivered.
    assert_eq!(run(&[0, 3, 0, 0xff, 0xff, 0xff]), Outcome::default());
    assert_eq!(run(&[]), Outcome::default());
    // The browser's own steps, and a length past the input's end.
    assert_eq!(
        run(&[12, 1, 0, 2, 15, 0, 0, 13, 0, 0, 14, 0, 0, 12, 200, 0, 1]),
        Outcome::default()
    );
}

#[test]
fn regressions_run() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/regressions");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "bin") {
            run(&std::fs::read(&path).unwrap());
        }
    }
}
