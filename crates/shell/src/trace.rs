//! `MACHOPO_TRACE=<file>`: a Perfetto trace of the process, written to `file`
//! as Perfetto's protobuf, which Perfetto's UI and trace processor open, as
//! they do Chrome's own traces (Chromium's `--trace-startup-file`). Every
//! span, on every thread, is a track event on that thread, next to V8's
//! own events (GC phases, deopts), as Chrome's trace has
//! Blink's and V8's. The spans are named as Chrome's trace events are
//! where machopo has their counterpart (`FunctionCall`, `Layout`), so that the
//! two read alike. The page's `performance.mark()` and `measure()`, which
//! `dom_api` sends as events of the target `blink.user_timing`, are
//! Chrome's User Timing events. Other log events stay the log's.

use std::path::Path;

use dom_js::trace;
use tracing::Event;
use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing::span::Id;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

/// The target of `dom_api`'s User Timing events, as Blink's category.
const USER_TIMING: &str = "blink.user_timing";

/// Starts the trace into `file` (before V8 starts: V8's events are
/// recorded only then), and gives the layer that records the spans and
/// User Timing, and what ends the trace once dropped (it is written as it goes; the end
/// writes the rest).
pub fn layer<S>(
    file: &Path,
) -> Result<(impl Layer<S> + Send + Sync + 'static, TraceEnd), trace::StartError>
where
    S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
{
    trace::start(file)?;
    let writer = TraceWriter {
        begin: trace::begin,
        end: trace::end,
        mark: trace::mark,
        measure: trace::measure,
    };
    Ok((writer.with_filter(filter_fn(is_traced)), TraceEnd(())))
}

/// Ends the trace when dropped.
pub struct TraceEnd(());

impl Drop for TraceEnd {
    fn drop(&mut self) {
        trace::stop();
    }
}

/// What the trace takes: every span, and the User Timing events.
fn is_traced(metadata: &tracing::Metadata<'_>) -> bool {
    metadata.is_span() || metadata.target() == USER_TIMING
}

/// Each span, while it is entered, as a span of the trace on its thread
/// (Perfetto's `TRACE_EVENT_BEGIN` and `TRACE_EVENT_END`); each User
/// Timing event as a mark or a measure.
struct TraceWriter {
    begin: fn(&str),
    end: fn(),
    mark: fn(&str, i64),
    measure: fn(&str, i64, i64),
}

impl<S> Layer<S> for TraceWriter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            (self.begin)(span.name());
        }
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        if ctx.span(id).is_some() {
            (self.end)();
        }
    }

    // Only User Timing's events get here (`is_traced`).
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let mut fields = UserTiming::default();
        event.record(&mut fields);
        match fields {
            UserTiming {
                mark: Some(name),
                ago: Some(ago),
                ..
            } => (self.mark)(&name, ago),
            UserTiming {
                measure: Some(name),
                start_ago: Some(start),
                end_ago: Some(end),
                ..
            } => (self.measure)(&name, start, end),
            _ => {}
        }
    }
}

/// A User Timing event's fields: a mark's name and how long ago it was, or
/// a measure's name and how long ago it started and ended (ns).
#[derive(Default)]
struct UserTiming {
    mark: Option<String>,
    ago: Option<i64>,
    measure: Option<String>,
    start_ago: Option<i64>,
    end_ago: Option<i64>,
}

impl Visit for UserTiming {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "mark" => self.mark = Some(value.to_owned()),
            "measure" => self.measure = Some(value.to_owned()),
            _ => {}
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        match field.name() {
            "ago_ns" => self.ago = Some(value),
            "start_ago_ns" => self.start_ago = Some(value),
            "end_ago_ns" => self.end_ago = Some(value),
            _ => {}
        }
    }

    fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use tracing_subscriber::Layer;
    use tracing_subscriber::filter::filter_fn;
    use tracing_subscriber::layer::SubscriberExt;

    use super::{TraceWriter, is_traced};

    thread_local! {
        static RECORDED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn begin(name: &str) {
        RECORDED.with(|recorded| recorded.borrow_mut().push(format!("B {name}")));
    }

    fn end() {
        RECORDED.with(|recorded| recorded.borrow_mut().push("E".into()));
    }

    fn mark(name: &str, ago: i64) {
        RECORDED.with(|recorded| recorded.borrow_mut().push(format!("M {name} {ago}")));
    }

    fn measure(name: &str, start: i64, end: i64) {
        RECORDED.with(|recorded| {
            recorded
                .borrow_mut()
                .push(format!("S {name} {start} {end}"))
        });
    }

    fn writer() -> TraceWriter {
        TraceWriter {
            begin,
            end,
            mark,
            measure,
        }
    }

    /// A span begins and ends on its thread each time it is entered and
    /// exited, under its name; log events aren't in the trace.
    #[test]
    fn spans_are_recorded_while_entered() {
        let subscriber =
            tracing_subscriber::registry().with(writer().with_filter(filter_fn(is_traced)));
        tracing::subscriber::with_default(subscriber, || {
            let outer = tracing::trace_span!("FunctionCall").entered();
            tracing::info!("a log line");
            let layout = tracing::trace_span!("Layout");
            layout.in_scope(|| {});
            layout.in_scope(|| {});
            drop(outer);
        });
        RECORDED.with(|recorded| {
            assert_eq!(
                *recorded.borrow(),
                ["B FunctionCall", "B Layout", "E", "B Layout", "E", "E"]
            );
        });
    }

    /// `dom_api`'s User Timing events are the trace's marks and measures,
    /// with their names and how long ago (ns), among the spans; events of
    /// another target with the same fields aren't, nor is a log line.
    #[test]
    fn user_timing_events_are_marks_and_measures() {
        let subscriber =
            tracing_subscriber::registry().with(writer().with_filter(filter_fn(is_traced)));
        tracing::subscriber::with_default(subscriber, || {
            let call = tracing::trace_span!("FunctionCall").entered();
            tracing::trace!(target: "blink.user_timing", mark = "a-start", ago_ns = 300_000i64);
            tracing::trace!(
                target: "blink.user_timing",
                measure = "a-sync",
                start_ago_ns = 2_000_000i64,
                end_ago_ns = -5i64,
            );
            tracing::trace!(target: "dom_api", mark = "not-this", ago_ns = 1i64);
            tracing::info!(mark = "nor-this", ago_ns = 1i64);
            drop(call);
        });
        RECORDED.with(|recorded| {
            assert_eq!(
                *recorded.borrow(),
                [
                    "B FunctionCall",
                    "M a-start 300000",
                    "S a-sync 2000000 -5",
                    "E"
                ]
            );
        });
    }
}
