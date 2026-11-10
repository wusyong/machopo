//! `MACHOPO_TRACE` in a whole browser: the page's `performance.mark()` and
//! `measure()` are in the trace as Chrome's are, under
//! `blink.user_timing`: a mark an instant on the renderer's main thread, a
//! measure a slice on a track of its own under that thread's, at the
//! entries' times rather than the calls'; a mark the constructor makes
//! isn't. The document's navigation timing is there too, a mark at each
//! time, named as Chrome's are. cppgc's phases (`CppGC.*`, as Oilpan's in
//! Chrome's trace) are there under `cppgc`, a JS document's and a wasm
//! document's heap alike. A trace is once a process, so machopo runs as one of
//! its own.

use std::collections::HashMap;
use std::path::PathBuf;

use net::test_server::Route;

use crate::automation_pipe::{Browser, server};

#[test]
fn marks_and_measures_are_chromes_user_timing_events() {
    // `t0` is 20 ms before `now`; the measures are given in its terms.
    let page = r#"<!DOCTYPE html>
<script>
const t0 = performance.now();
while (performance.now() - t0 < 20) {}
performance.mark('past', { startTime: t0 });
performance.mark('now');
performance.measure('m', { start: t0, end: t0 + 10 });
performance.measure('m', { start: t0 + 5, end: t0 + 15 });
new PerformanceMark('made');
</script>"#;
    let server = server(&[("/marks", Route::ok("text/html", page))]);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-trace");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("user-timing.pftrace");
    let _ = std::fs::remove_file(&file);
    let mut browser = Browser::start_with_env(&[("MACHOPO_TRACE", file.as_os_str())]);
    let session = browser.attach();
    browser.navigate(&session, &server.url("/marks"));
    assert!(browser.end().success());

    let trace = Trace::read(&std::fs::read(&file).expect("machopo wrote the trace"));
    let main = trace.thread_track("CrRendererMain");
    let (navigation, user_timing): (Vec<&TrackEvent>, Vec<&TrackEvent>) = trace
        .events
        .iter()
        .filter(|event| event.categories.iter().any(|c| c == "blink.user_timing"))
        .partition(|event| NAVIGATION.contains(&event.name.as_str()));
    // The page's document's: the last of each, in order, each an instant
    // on the thread's track, at their times.
    let start = navigation
        .iter()
        .rposition(|event| event.name == "navigationStart")
        .expect("the document's navigationStart");
    let navigation = &navigation[start..];
    let names: Vec<&str> = navigation.iter().map(|event| event.name.as_str()).collect();
    assert_eq!(names, NAVIGATION);
    for pair in navigation.windows(2) {
        assert!(
            pair[0].time <= pair[1].time,
            "{} after {}",
            pair[0].name,
            pair[1].name
        );
        assert_eq!((pair[0].kind, pair[0].track), (INSTANT, Some(main)));
    }
    let names: Vec<(u64, &str)> = user_timing
        .iter()
        .map(|event| (event.kind, event.name.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            (INSTANT, "past"),
            (INSTANT, "now"),
            (BEGIN, "m"),
            (BEGIN, "m")
        ]
    );
    let [past, now, first, second] = user_timing[..] else {
        unreachable!()
    };
    // The marks on the thread's own track, at their times.
    assert_eq!(past.track, Some(main));
    assert_eq!(now.track, Some(main));
    let ms = |from: &TrackEvent, to: &TrackEvent| (to.time as f64 - from.time as f64) / 1e6;
    assert!(ms(past, now) >= 19.9, "{}", ms(past, now));
    // Each measure on a track of its own under the thread's, from its
    // start to its end; the second overlaps the first.
    assert_ne!(first.track, second.track);
    for measure in [first, second] {
        let track = measure.track.unwrap();
        assert_eq!(trace.tracks[&track].parent, Some(main));
    }
    let end = |begin: &TrackEvent| {
        trace
            .events
            .iter()
            .find(|event| event.kind == END && event.track == begin.track)
            .expect("the measure ends")
    };
    // Its times and the mark's come from two clocks read a call apart.
    assert!(ms(past, first).abs() < 0.05, "{}", ms(past, first));
    assert!(
        (ms(past, second) - 5.0).abs() < 0.05,
        "{}",
        ms(past, second)
    );
    assert!((ms(first, end(first)) - 10.0).abs() < 1e-5);
    assert!((ms(second, end(second)) - 10.0).abs() < 1e-5);
}

/// Navigation Timing's marks of a document, in the order they come
/// (Blink's `DocumentLoadTiming` and `DocumentTiming`).
const NAVIGATION: [&str; 11] = [
    "navigationStart",
    "fetchStart",
    "responseEnd",
    "commitNavigationEnd",
    "domLoading",
    "domInteractive",
    "domContentLoadedEventStart",
    "domContentLoadedEventEnd",
    "domComplete",
    "loadEventStart",
    "loadEventEnd",
];

/// A JS document's heap, collected by a forced GC (V8's with the CppHeap
/// in it): its atomic pause is on the renderer's main thread.
#[test]
fn a_js_documents_cppgc_phases_are_traced() {
    let page = r#"<!DOCTYPE html>
<script>
for (let i = 0; i < 1000; i++) document.createElement('div');
gc();
</script>"#;
    let trace = traced(
        "cppgc-js",
        &[("/gc", Route::ok("text/html", page))],
        "/gc",
        &[("MACHOPO_V8_FLAGS", "--expose-gc".as_ref())],
    );
    let phases = main_thread_cppgc_phases(&trace);
    assert!(phases.contains(&"CppGC.AtomicMark"), "{phases:?}");
    // The DOM's heap's own marking in that pause: its platform's flag says
    // the trace records.
    let main = trace.thread_track("CrRendererMain");
    assert!(
        trace.events.iter().any(|event| event.kind == BEGIN
            && event.track == Some(main)
            && event.name == "Gc.Mark"),
        "no Gc.Mark on the main thread"
    );
}

/// The trace of machopo loading `path` from `routes`, with `env`.
fn traced(
    name: &str,
    routes: &[(&str, Route)],
    path: &str,
    env: &[(&str, &std::ffi::OsStr)],
) -> Trace {
    let server = server(routes);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-trace");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{name}.pftrace"));
    let _ = std::fs::remove_file(&file);
    let mut env = env.to_vec();
    env.push(("MACHOPO_TRACE", file.as_os_str()));
    let mut browser = Browser::start_with_env(&env);
    let session = browser.attach();
    browser.navigate(&session, &server.url(path));
    assert!(browser.end().success());
    Trace::read(&std::fs::read(&file).expect("machopo wrote the trace"))
}

/// The names of the `cppgc` slices begun on the renderer's main thread,
/// each checked to be one of cppgc's (`CppGC.`).
fn main_thread_cppgc_phases(trace: &Trace) -> Vec<&str> {
    let main = trace.thread_track("CrRendererMain");
    let names: Vec<&str> = trace
        .events
        .iter()
        .filter(|event| {
            event.kind == BEGIN
                && event.track == Some(main)
                && event.categories.iter().any(|c| c == "cppgc")
        })
        .map(|event| event.name.as_str())
        .collect();
    assert!(
        names.iter().all(|name| name.starts_with("CppGC.")),
        "{names:?}"
    );
    names
}

const BEGIN: u64 = 1;
const END: u64 = 2;
const INSTANT: u64 = 3;

/// The track events of a Perfetto trace as Perfetto's SDK writes them,
/// and its tracks.
struct Trace {
    events: Vec<TrackEvent>,
    tracks: HashMap<u64, Track>,
}

struct TrackEvent {
    /// `TrackEvent.type`: [`BEGIN`], [`END`], [`INSTANT`].
    kind: u64,
    name: String,
    categories: Vec<String>,
    /// Its own, or its sequence's default (its thread's).
    track: Option<u64>,
    /// ns, on the clock it was written with.
    time: u64,
}

struct Track {
    parent: Option<u64>,
    thread: Option<String>,
}

/// A sequence's state: what it interned, its default track, and where its
/// incremental clock is.
#[derive(Default)]
struct Sequence {
    names: HashMap<u64, String>,
    categories: HashMap<u64, String>,
    track: Option<u64>,
    clock: Option<u64>,
    time: u64,
}

/// `TracePacket`'s clock for times relative to the sequence's last one.
const INCREMENTAL_CLOCK: u64 = 64;

impl Trace {
    fn read(bytes: &[u8]) -> Self {
        let mut sequences: HashMap<u64, Sequence> = HashMap::new();
        let mut events = Vec::new();
        let mut tracks = HashMap::new();
        for packet in fields(bytes).into_iter().filter(|(n, _)| *n == 1) {
            let packet = fields(packet.1.bytes());
            let first = |number| packet.iter().find(|(n, _)| *n == number).map(|(_, v)| v);
            let sequence = sequences
                .entry(first(10).map_or(0, Value::int))
                .or_default();
            if first(13).is_some_and(|flags| flags.int() & 1 != 0) {
                sequence.names.clear();
                sequence.categories.clear();
            }
            if let Some(defaults) = first(59) {
                let defaults = fields(defaults.bytes());
                sequence.clock = field(&defaults, 58).map(Value::int);
                if let Some(track_event) = field(&defaults, 11) {
                    sequence.track = field(&fields(track_event.bytes()), 11).map(Value::int);
                }
            }
            if let Some(snapshot) = first(6) {
                for (_, clock) in fields(snapshot.bytes()).iter().filter(|(n, _)| *n == 1) {
                    let clock = fields(clock.bytes());
                    if field(&clock, 1).map(Value::int) == Some(INCREMENTAL_CLOCK) {
                        sequence.time = field(&clock, 2).map_or(0, Value::int);
                    }
                }
            }
            if let Some(interned) = first(12) {
                for (number, entry) in fields(interned.bytes()) {
                    let entry = fields(entry.bytes());
                    let iid = field(&entry, 1).map_or(0, Value::int);
                    let name = field(&entry, 2).map_or(String::new(), Value::string);
                    match number {
                        1 => sequence.categories.insert(iid, name),
                        2 => sequence.names.insert(iid, name),
                        _ => None,
                    };
                }
            }
            if let Some(descriptor) = first(60) {
                let descriptor = fields(descriptor.bytes());
                let thread = field(&descriptor, 4)
                    .and_then(|thread| field(&fields(thread.bytes()), 5).map(Value::string));
                tracks.insert(
                    field(&descriptor, 1).map_or(0, Value::int),
                    Track {
                        parent: field(&descriptor, 5).map(Value::int),
                        thread,
                    },
                );
            }
            if let Some(event) = first(11) {
                let event = fields(event.bytes());
                let mut time = first(8).map_or(0, Value::int);
                if first(58).map(Value::int).or(sequence.clock) == Some(INCREMENTAL_CLOCK) {
                    sequence.time += time;
                    time = sequence.time;
                }
                let name = match field(&event, 23) {
                    Some(name) => name.string(),
                    None => field(&event, 10)
                        .and_then(|iid| sequence.names.get(&iid.int()).cloned())
                        .unwrap_or_default(),
                };
                let categories = event
                    .iter()
                    .filter_map(|(number, value)| match number {
                        3 => sequence.categories.get(&value.int()).cloned(),
                        22 => Some(value.string()),
                        _ => None,
                    })
                    .collect();
                events.push(TrackEvent {
                    kind: field(&event, 9).map_or(0, Value::int),
                    name,
                    categories,
                    track: field(&event, 11).map(Value::int).or(sequence.track),
                    time,
                });
            }
        }
        Self { events, tracks }
    }

    /// The track of the thread named `name`.
    fn thread_track(&self, name: &str) -> u64 {
        *self
            .tracks
            .iter()
            .find(|(_, track)| track.thread.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("a thread {name}"))
            .0
    }
}

/// A protobuf field's value: a varint or fixed-size number, or bytes.
enum Value<'a> {
    Int(u64),
    Bytes(&'a [u8]),
}

impl<'a> Value<'a> {
    fn int(&self) -> u64 {
        match self {
            Value::Int(n) => *n,
            Value::Bytes(_) => panic!("a number"),
        }
    }

    fn bytes(&self) -> &'a [u8] {
        match self {
            Value::Bytes(bytes) => bytes,
            Value::Int(_) => panic!("bytes"),
        }
    }

    fn string(&self) -> String {
        String::from_utf8_lossy(self.bytes()).into_owned()
    }
}

/// A protobuf message's fields in order.
fn fields(mut bytes: &[u8]) -> Vec<(u64, Value<'_>)> {
    fn varint(bytes: &mut &[u8]) -> u64 {
        let mut value = 0;
        for shift in (0..64).step_by(7) {
            let byte = bytes[0];
            *bytes = &bytes[1..];
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                break;
            }
        }
        value
    }
    let mut found = Vec::new();
    while !bytes.is_empty() {
        let key = varint(&mut bytes);
        let value = match key & 7 {
            0 => Value::Int(varint(&mut bytes)),
            1 => {
                let (number, rest) = bytes.split_at(8);
                bytes = rest;
                Value::Int(u64::from_le_bytes(number.try_into().unwrap()))
            }
            2 => {
                let len = varint(&mut bytes) as usize;
                let (value, rest) = bytes.split_at(len);
                bytes = rest;
                Value::Bytes(value)
            }
            5 => {
                let (number, rest) = bytes.split_at(4);
                bytes = rest;
                Value::Int(u64::from(u32::from_le_bytes(number.try_into().unwrap())))
            }
            wire => panic!("wire type {wire}"),
        };
        found.push((key >> 3, value));
    }
    found
}

/// The first field `number` of a message's `fields`.
fn field<'a, 'b>(fields: &'b [(u64, Value<'a>)], number: u64) -> Option<&'b Value<'a>> {
    fields
        .iter()
        .find(|(n, _)| *n == number)
        .map(|(_, value)| value)
}
