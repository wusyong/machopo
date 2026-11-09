//! machopo's spans in the trace (Chromium's `base/trace_event`): each made
//! and entered only when the trace takes spans of its level, as
//! `TRACE_EVENT0` looks at its category's enabled flag first. A
//! `tracing::trace_span!` that the trace doesn't take is still an empty
//! span built, entered and dropped, tens of instructions on every call of
//! the code it is in (a DOM call's dispatch is one); with these, one
//! relaxed load and a branch. machopo's code makes its spans with these
//! (`sanity.py spans` checks it names no `tracing` span macro); the span
//! is `tracing`'s, so filters, fields and the trace's layers are as
//! before.

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

#[doc(hidden)]
pub use tracing;

/// `tracing::trace_span!(…)` (the same arguments: a name, then fields)
/// entered until the value it gives is dropped, if the trace takes
/// `TRACE` spans; `None` otherwise, with no span made. Bind it to a name
/// (`let _span = trace_event!("Layout");`): `let _ = …` drops it at once.
#[macro_export]
macro_rules! trace_event {
    ($($span:tt)+) => {
        if $crate::tracing::level_enabled!($crate::tracing::Level::TRACE) {
            let span = $crate::tracing::trace_span!($($span)+);
            ::core::option::Option::Some(span.entered())
        } else {
            ::core::option::Option::None
        }
    };
}

/// `trace_event!` at `INFO` (`tracing::info_span!`): a span the log's
/// filter may take too (a navigation's commit, with its URL).
#[macro_export]
macro_rules! info_event {
    ($($span:tt)+) => {
        if $crate::tracing::level_enabled!($crate::tracing::Level::INFO) {
            let span = $crate::tracing::info_span!($($span)+);
            ::core::option::Option::Some(span.entered())
        } else {
            ::core::option::Option::None
        }
    };
}

/// `f` run in `span` (a `trace_event!`'s, made first), which ends as `f`
/// returns: `tracing::Span::in_scope`'s.
pub fn in_scope<R>(span: Option<tracing::span::EnteredSpan>, f: impl FnOnce() -> R) -> R {
    let _span = span;
    f()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing::Subscriber;
    use tracing::span::{Attributes, Id};
    use tracing_subscriber::filter::LevelFilter;
    use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

    /// The names of the spans made, in order.
    #[derive(Clone, Default)]
    struct Made(Arc<Mutex<Vec<String>>>);

    impl<S: Subscriber> Layer<S> for Made {
        fn on_new_span(&self, attrs: &Attributes<'_>, _: &Id, _: Context<'_, S>) {
            self.0
                .lock()
                .unwrap()
                .push(attrs.metadata().name().to_owned());
        }
    }

    /// No span made under no trace, nor one of a level the trace doesn't
    /// take; one of a level it takes, made, entered while the value lives,
    /// with its fields; `in_scope`'s, around its function. In one test, in
    /// order: the level `tracing` checks first is the most any subscriber
    /// takes (`LevelFilter::current`), and it doesn't go down again as a
    /// subscriber goes.
    #[test]
    fn spans_are_made_only_at_the_levels_the_trace_takes() {
        assert!(trace_event!("Layout").is_none());
        assert!(info_event!("commit_navigation", url = "a").is_none());

        let made = Made::default();
        let info = tracing_subscriber::registry().with(made.clone().with_filter(LevelFilter::INFO));
        tracing::subscriber::with_default(info, || {
            assert!(trace_event!("Layout").is_none());
            let commit = info_event!("commit_navigation", url = "a");
            assert!(commit.is_some());
        });
        assert_eq!(*made.0.lock().unwrap(), ["commit_navigation"]);

        let made = Made::default();
        let trace =
            tracing_subscriber::registry().with(made.clone().with_filter(LevelFilter::TRACE));
        tracing::subscriber::with_default(trace, || {
            let outer = trace_event!("ProxyMain::BeginMainFrame");
            let entered = tracing::Span::current();
            assert_eq!(
                entered.metadata().map(|meta| meta.name()),
                Some("ProxyMain::BeginMainFrame")
            );
            {
                let _inner = trace_event!("Layout", nodes = 3);
                assert_eq!(
                    tracing::Span::current().metadata().map(|meta| meta.name()),
                    Some("Layout")
                );
            }
            assert_eq!(
                tracing::Span::current().metadata().map(|meta| meta.name()),
                Some("ProxyMain::BeginMainFrame")
            );
            drop(outer);
            assert!(tracing::Span::current().is_none());
            // `in_scope` runs its function in the span, which ends as it
            // returns.
            let name = crate::in_scope(trace_event!("canvas::flush_shown"), || {
                tracing::Span::current().metadata().map(|meta| meta.name())
            });
            assert_eq!(name, Some("canvas::flush_shown"));
            assert!(tracing::Span::current().is_none());
        });
        assert_eq!(
            *made.0.lock().unwrap(),
            ["ProxyMain::BeginMainFrame", "Layout", "canvas::flush_shown"]
        );
    }
}
