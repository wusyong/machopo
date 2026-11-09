//! The parser's spans in a trace: each run of it is a `ParseHTML`, as
//! Blink's parser's trace event is named.

use std::process::Command;
use std::sync::{Arc, Mutex};

use html::parser::ParserState;
use tracing::span::{Attributes, Id};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::{Context, SubscriberExt};

/// Set in the child process `alone` runs a test in.
const ALONE: &str = "MACHOPO_HTML_SPANS_ALONE";

/// Whether this is the child process running the test `name` by itself;
/// if not, runs it so and checks it passed. Whether a callsite makes spans
/// is cached for the whole process, and tracing-core registers a callsite
/// by listing it and then computing that from the dispatchers of that
/// moment: another test's thread reaching the parser's callsite first,
/// with no subscriber, could store "never" after this test's subscriber
/// was registered, and the spans from then on would not be made (a few
/// runs of the binary in a hundred, sometimes partway through a test).
fn alone(name: &str) -> bool {
    if std::env::var_os(ALONE).is_some() {
        return true;
    }
    let output = Command::new(std::env::current_exe().expect("the test binary"))
        .args([name, "--exact", "--test-threads=1"])
        .env(ALONE, "1")
        .output()
        .expect("the test binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{name}, alone:\n{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

/// The names of the spans made while `f` runs.
fn spans_of(f: impl FnOnce()) -> Vec<&'static str> {
    struct Made(Arc<Mutex<Vec<&'static str>>>);
    impl<S: tracing::Subscriber> Layer<S> for Made {
        fn on_new_span(&self, attrs: &Attributes<'_>, _: &Id, _: Context<'_, S>) {
            self.0.lock().unwrap().push(attrs.metadata().name());
        }
    }
    let made = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(Made(made.clone()));
    tracing::subscriber::with_default(subscriber, f);
    made.lock().unwrap().clone()
}

/// A document parsed as it comes: each run to a script or to the end of
/// the input so far (Blink's `PumpTokenizer`), and its end.
#[test]
fn each_run_of_the_parser() {
    if !alone("spans::each_run_of_the_parser") {
        return;
    }
    let spans = spans_of(|| {
        let mut parser = crate::harness::DocumentParser::new();
        parser.push("<p>a<script>1</script><p>b");
        assert!(matches!(parser.resume(), ParserState::Script { .. }));
        assert!(matches!(parser.resume(), ParserState::NeedsInput));
        parser.finish();
    });
    assert_eq!(spans, ["ParseHTML"; 3]);
}

/// A whole document parsed at once (`DOMParser`'s).
#[test]
fn a_document_at_once() {
    if !alone("spans::a_document_at_once") {
        return;
    }
    let spans = spans_of(|| {
        let mut dom = dom::Dom::new();
        html::parser::parse_document(&mut dom, "<p>a", true);
    });
    assert_eq!(spans, ["ParseHTML"]);
}
