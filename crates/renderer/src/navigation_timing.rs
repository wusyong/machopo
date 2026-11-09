//! A document's navigation timing (Blink's `DocumentLoadTiming`, which
//! `PerformanceNavigationTiming` reads), as the renderer works it out: the
//! network's times, from the browser's commit (the main frame's) or the
//! renderer's own load (a child frame's), on the renderer's clock; then, as
//! the DOM keeps it, since the window's time origin (`document_timing`).

use dom::NavigationType;
use ipc::TimeTicks;
use protocol::network::LoadTiming;
use protocol::renderer::RedirectHop;
use url::Url;

/// What the renderer knows of a document's navigation (Blink's
/// `DocumentLoadTiming` and the response's): times in milliseconds on the
/// renderer's clock, `None` for what didn't happen (no redirect, no
/// connection opened).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct NavigationTiming {
    pub(crate) navigation_type: NavigationType,
    /// The redirects followed, when every one was of the document's
    /// origin (0 otherwise, as their times are none).
    pub(crate) redirect_count: u32,
    /// When the first redirect's request started, and the last one's
    /// response ended.
    pub(crate) redirect_start: Option<f64>,
    pub(crate) redirect_end: Option<f64>,
    /// Whether a redirect of another origin than the document's came
    /// before it (Fetch's "has cross-origin redirects"): the `unload` of
    /// the document before isn't told then.
    pub(crate) has_cross_origin_redirects: bool,
    /// The `unload` of the document before it in its frame, if told
    /// (`with_unload`).
    pub(crate) unload_event_start: Option<f64>,
    pub(crate) unload_event_end: Option<f64>,
    /// When the document's own request started (after the redirects).
    pub(crate) fetch_start: Option<f64>,
    pub(crate) domain_lookup_start: Option<f64>,
    pub(crate) domain_lookup_end: Option<f64>,
    pub(crate) connect_start: Option<f64>,
    pub(crate) connect_end: Option<f64>,
    pub(crate) secure_connection_start: Option<f64>,
    pub(crate) request_start: Option<f64>,
    /// When the response's head came.
    pub(crate) response_start: Option<f64>,
    pub(crate) response_end: Option<f64>,
    /// `http/1.1`, `h2`...; empty for none.
    pub(crate) next_hop_protocol: String,
    /// Over the network: the body's bytes, before and after
    /// `Content-Encoding` was undone (0 for a document that didn't come
    /// over it).
    pub(crate) encoded_body_size: u64,
    pub(crate) decoded_body_size: u64,
    /// Whether it came over the network (its `transferSize` counts).
    pub(crate) transferred: bool,
    pub(crate) response_status: u32,
    pub(crate) content_type: String,
    pub(crate) content_encoding: String,
    /// `confidence.value`: `high`, or `low` (randomized, as Chrome's).
    pub(crate) confidence_high: bool,
}

/// A navigation's `confidence`, as Chromium randomizes it: machopo's is
/// `high` (it has no signal of a busy start, which makes Chromium's
/// `low`), told as it is unless a roll below
/// `dom_api::RANDOMIZED_TRIGGER_RATE` makes it a coin's toss. Rolled
/// once per navigation.
pub(crate) fn randomized_confidence() -> bool {
    let mut bytes = [0; 9];
    getrandom::fill(&mut bytes).expect("the operating system gives random numbers");
    let roll = u64::from_le_bytes(bytes[..8].try_into().expect("eight bytes"));
    randomize(true, roll, bytes[8] & 1 == 1)
}

/// `high`, told as it is, or (`roll` below the rate, as a fraction of
/// 2^64) `coin`.
fn randomize(high: bool, roll: u64, coin: bool) -> bool {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a fraction: 53 bits of it are plenty"
    )]
    let fraction = roll as f64 / 18_446_744_073_709_551_616.0;
    if fraction < dom_api::RANDOMIZED_TRIGGER_RATE {
        coin
    } else {
        high
    }
}

/// What a document's load took, as Navigation Timing reads it.
pub(crate) struct DocumentLoad<'a> {
    /// Its final URL, after the redirects.
    pub(crate) url: &'a Url,
    pub(crate) timing: Option<LoadTiming>,
    pub(crate) response_end: TimeTicks,
    pub(crate) redirects: &'a [RedirectHop],
    /// Whether its request's referrer policy is `no-referrer`: its origin
    /// then serializes as `null` (Fetch's "serializing a request origin"),
    /// which no redirect's `Timing-Allow-Origin` names but `*`.
    pub(crate) no_referrer: bool,
    pub(crate) protocol: &'a str,
    pub(crate) status: u16,
    pub(crate) mime_type: &'a str,
    pub(crate) content_encoding: Option<&'a str>,
    pub(crate) encoded_body_size: u64,
    pub(crate) decoded_body_size: u64,
}

/// The network's part of a document's navigation timing, on the frame's
/// clock (`at` puts a reading of the shared clock on it), from its load:
/// Navigation Timing's steps, with what machopo can't measure as decided (a
/// connection's TLS isn't apart from it: `secureConnectionStart` is
/// `connectStart` for HTTPS; the request is sent as the connection is
/// made, or as the fetch starts on one reused). A name it didn't resolve,
/// or a connection it reused, is at the fetch's start, as in Chrome.
/// Redirects count if each was of the document's origin; with one of
/// another, only if every one opted in to it (its `Timing-Allow-Origin` is
/// `*` or the document's origin), the same-origin ones too, as Chrome 152
/// counts them (`probes/tao_probe.py`). A document
/// that wasn't loaded (`about:blank`, an error page) has every network
/// time at its time origin, `origin`.
pub(crate) fn network_timing(
    load: Option<DocumentLoad<'_>>,
    navigation_type: NavigationType,
    origin: f64,
    at: impl Fn(TimeTicks) -> f64,
    confidence_high: bool,
) -> NavigationTiming {
    let mut timing = NavigationTiming {
        navigation_type,
        confidence_high,
        ..NavigationTiming::default()
    };
    let Some(load) = load else {
        for time in [
            &mut timing.fetch_start,
            &mut timing.domain_lookup_start,
            &mut timing.domain_lookup_end,
            &mut timing.connect_start,
            &mut timing.connect_end,
            &mut timing.request_start,
            &mut timing.response_start,
            &mut timing.response_end,
        ] {
            *time = Some(origin);
        }
        return timing;
    };
    let fetch_start = load.timing.map_or(origin, |t| at(t.request_start));
    let (lookup_start, lookup_end) = load
        .timing
        .and_then(|t| t.dns)
        .map_or((fetch_start, fetch_start), |dns| {
            (at(dns.start), at(dns.end))
        });
    let connect = load.timing.and_then(|t| t.connect);
    let (connect_start, connect_end) =
        connect.map_or((lookup_end, lookup_end), |c| (at(c.start), at(c.end)));
    timing.fetch_start = Some(fetch_start);
    timing.domain_lookup_start = Some(lookup_start);
    timing.domain_lookup_end = Some(lookup_end);
    timing.connect_start = Some(connect_start);
    timing.connect_end = Some(connect_end);
    timing.secure_connection_start = (load.url.scheme() == "https").then_some(connect_start);
    timing.request_start = Some(if connect.is_some() {
        connect_end
    } else {
        fetch_start
    });
    timing.response_start = Some(
        load.timing
            .map_or_else(|| at(load.response_end), |t| at(t.receive_headers_end)),
    );
    timing.response_end = Some(at(load.response_end));
    let destination = load.url.origin();
    let same_origin =
        |hop: &RedirectHop| Url::parse(&hop.url).is_ok_and(|url| url.origin() == destination);
    let opted_in = |hop: &RedirectHop| {
        hop.timing_allow_origin.as_deref().is_some_and(|values| {
            values.split(',').map(str::trim).any(|value| {
                value == "*" || (!load.no_referrer && value == destination.ascii_serialization())
            })
        })
    };
    timing.has_cross_origin_redirects = !load.redirects.iter().all(same_origin);
    let exposed = load.redirects.iter().all(same_origin) || load.redirects.iter().all(opted_in);
    if !load.redirects.is_empty() && exposed {
        timing.redirect_count = u32::try_from(load.redirects.len()).unwrap_or(u32::MAX);
        timing.redirect_start = load
            .redirects
            .first()
            .and_then(|hop| hop.timing)
            .map(|t| at(t.request_start));
        timing.redirect_end = load
            .redirects
            .last()
            .and_then(|hop| hop.timing)
            .map(|t| at(t.response_end));
    }
    timing.next_hop_protocol = load.protocol.to_owned();
    timing.transferred = load.timing.is_some();
    timing.encoded_body_size = load.encoded_body_size;
    timing.decoded_body_size = load.decoded_body_size;
    timing.response_status = u32::from(load.status);
    timing.content_type = load.mime_type.to_owned();
    timing.content_encoding = load.content_encoding.unwrap_or_default().to_owned();
    timing
}

/// `timing` with the `unload` of the document before it in its frame, at
/// `unloaded` (on the renderer's clock), if that one was of `previous`, an
/// origin, the same as the document's (at `url`), and no redirect of
/// another origin came between (HTML's "unload timing info", kept only
/// then).
pub(crate) fn with_unload(
    mut timing: NavigationTiming,
    unloaded: Option<(f64, f64)>,
    previous: Option<&Url>,
    url: &Url,
) -> NavigationTiming {
    if let Some((start, end)) = unloaded
        && previous.is_some_and(|previous| previous.origin() == url.origin())
        && !timing.has_cross_origin_redirects
    {
        timing.unload_event_start = Some(start);
        timing.unload_event_end = Some(end);
    }
    timing
}

/// `timing` (on the renderer's clock) as the DOM keeps it: its times
/// since `time_origin` (the window's, on the same clock), `dom_loading` the
/// document's start (now, as it commits), never before its fetch started
/// or its response ended (Chrome makes the document once the response is
/// in; two readings of the clock, the browser's and the renderer's, may
/// say otherwise under load). In machopo's trace too, as Chrome's mark.
pub(crate) fn document_timing(
    timing: &NavigationTiming,
    time_origin: f64,
    dom_loading: f64,
) -> dom::NavigationTiming {
    let since = |time: Option<f64>| time.map(|time| time - time_origin);
    let committed = dom_loading;
    let dom_loading = [timing.fetch_start, timing.response_end]
        .into_iter()
        .flatten()
        .fold(dom_loading, f64::max);
    tracing::trace!(
        target: "blink.user_timing",
        mark = "domLoading",
        ago_ns = base::numerics::saturated_cast::<i64, _>((committed - dom_loading) * 1e6)
    );
    dom::NavigationTiming {
        navigation_type: timing.navigation_type,
        redirect_count: timing.redirect_count,
        redirect_start: since(timing.redirect_start),
        redirect_end: since(timing.redirect_end),
        unload_event_start: since(timing.unload_event_start),
        unload_event_end: since(timing.unload_event_end),
        fetch_start: since(timing.fetch_start),
        domain_lookup_start: since(timing.domain_lookup_start),
        domain_lookup_end: since(timing.domain_lookup_end),
        connect_start: since(timing.connect_start),
        connect_end: since(timing.connect_end),
        secure_connection_start: since(timing.secure_connection_start),
        request_start: since(timing.request_start),
        response_start: since(timing.response_start),
        response_end: since(timing.response_end),
        next_hop_protocol: timing.next_hop_protocol.clone(),
        encoded_body_size: timing.encoded_body_size,
        decoded_body_size: timing.decoded_body_size,
        transferred: timing.transferred,
        response_status: timing.response_status,
        content_type: timing.content_type.clone(),
        content_encoding: timing.content_encoding.clone(),
        dom_loading: Some(dom_loading - time_origin),
        confidence_high: timing.confidence_high,
        ..dom::NavigationTiming::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::network::TimeSpan;
    use std::time::Duration;

    /// A reading `ms` after `base`.
    fn tick(base: TimeTicks, ms: u64) -> TimeTicks {
        base.checked_add(Duration::from_millis(ms)).unwrap()
    }

    /// The `unload` of the document before is told if it was of the
    /// document's origin and no redirect of another came between.
    #[test]
    fn the_unload_before_of_one_origin() {
        let url = Url::parse("https://a.test/page").unwrap();
        let same = Url::parse("https://a.test/before").unwrap();
        let other = Url::parse("https://b.test/before").unwrap();
        let told = |previous: Option<&Url>, cross: bool| {
            let timing = NavigationTiming {
                has_cross_origin_redirects: cross,
                ..NavigationTiming::default()
            };
            let timing = with_unload(timing, Some((3.0, 4.0)), previous, &url);
            (timing.unload_event_start, timing.unload_event_end)
        };
        assert_eq!(told(Some(&same), false), (Some(3.0), Some(4.0)));
        assert_eq!(told(Some(&other), false), (None, None));
        assert_eq!(told(Some(&same), true), (None, None));
        assert_eq!(told(None, false), (None, None));
        // Fetch's "has cross-origin redirects": one of another origin.
        let base = TimeTicks::now();
        let hops = |url: &str| {
            vec![RedirectHop {
                url: url.into(),
                timing: Some(LoadTiming {
                    request_start: base,
                    dns: None,
                    connect: None,
                    receive_headers_end: base,
                    response_end: base,
                }),
                timing_allow_origin: None,
            }]
        };
        for (hop, cross) in [("https://a.test/hop", false), ("https://b.test/hop", true)] {
            let hops = hops(hop);
            let load = DocumentLoad {
                url: &url,
                timing: None,
                response_end: base,
                redirects: &hops,
                no_referrer: false,
                protocol: "h2",
                status: 200,
                mime_type: "text/html",
                content_encoding: None,
                encoded_body_size: 0,
                decoded_body_size: 0,
            };
            let timing = network_timing(Some(load), NavigationType::Navigate, 0.0, |_| 1.0, true);
            assert_eq!(timing.has_cross_origin_redirects, cross, "{hop}");
        }
    }

    /// `domLoading` is when the document commits, but never before its
    /// fetch started or its response ended (the browser's clock and the
    /// renderer's read apart): a commit read earlier takes the latest of
    /// them; the other times count from the time origin.
    #[test]
    fn dom_loading_is_after_the_response() {
        let timing = NavigationTiming {
            fetch_start: Some(12.0),
            response_end: Some(20.0),
            ..NavigationTiming::default()
        };
        let early = document_timing(&timing, 10.0, 15.0);
        assert_eq!(early.dom_loading, Some(10.0));
        assert_eq!(
            (early.fetch_start, early.response_end),
            (Some(2.0), Some(10.0))
        );
        assert_eq!(document_timing(&timing, 10.0, 25.0).dom_loading, Some(15.0));
        // An `about:` document, fetched nowhere: when it commits.
        let none = NavigationTiming::default();
        assert_eq!(document_timing(&none, 10.0, 11.0).dom_loading, Some(1.0));
    }

    #[test]
    fn the_networks_times() {
        let base = TimeTicks::now();
        let at = |t: TimeTicks| t.saturating_since(base).as_secs_f64() * 1000.0;
        let url = Url::parse("https://a.test/page").unwrap();
        let load_timing = |start, dns: bool, connect: bool, headers, end| LoadTiming {
            request_start: tick(base, start),
            dns: dns.then(|| TimeSpan {
                start: tick(base, start + 1),
                end: tick(base, start + 2),
            }),
            connect: connect.then(|| TimeSpan {
                start: tick(base, start + 2),
                end: tick(base, start + 4),
            }),
            receive_headers_end: tick(base, headers),
            response_end: tick(base, end),
        };
        let redirects = [
            RedirectHop {
                url: "https://a.test/one".into(),
                timing: Some(load_timing(10, true, true, 18, 19)),
                timing_allow_origin: None,
            },
            RedirectHop {
                url: "https://a.test/two".into(),
                timing: Some(load_timing(20, false, false, 24, 25)),
                timing_allow_origin: None,
            },
        ];
        let load = |redirects| DocumentLoad {
            url: &url,
            timing: Some(load_timing(30, true, true, 40, 50)),
            response_end: tick(base, 50),
            redirects,
            no_referrer: false,
            protocol: "h2",
            status: 200,
            mime_type: "text/html",
            content_encoding: Some("gzip"),
            encoded_body_size: 100,
            decoded_body_size: 400,
        };
        let timing = network_timing(
            Some(load(&redirects)),
            NavigationType::Reload,
            5.0,
            at,
            true,
        );
        assert_eq!(timing.navigation_type, NavigationType::Reload);
        assert_eq!(timing.fetch_start, Some(30.0));
        assert_eq!(
            (timing.domain_lookup_start, timing.domain_lookup_end),
            (Some(31.0), Some(32.0))
        );
        assert_eq!(
            (timing.connect_start, timing.connect_end),
            (Some(32.0), Some(34.0))
        );
        assert_eq!(timing.secure_connection_start, Some(32.0));
        assert_eq!(timing.request_start, Some(34.0));
        assert_eq!(
            (timing.response_start, timing.response_end),
            (Some(40.0), Some(50.0))
        );
        assert_eq!(timing.redirect_count, 2);
        assert_eq!(
            (timing.redirect_start, timing.redirect_end),
            (Some(10.0), Some(25.0))
        );
        assert_eq!(timing.next_hop_protocol, "h2");
        assert!(timing.transferred);
        assert_eq!(
            (timing.encoded_body_size, timing.decoded_body_size),
            (100, 400)
        );
        assert_eq!(timing.content_encoding, "gzip");
        // A redirect of another origin: none counted, unless it opted in
        // to the document's origin, or to any (only to any if the request's
        // origin is `null`: its referrer policy `no-referrer`).
        let elsewhere = |tao: Option<&str>| {
            vec![RedirectHop {
                url: "https://b.test/".into(),
                timing: Some(load_timing(10, true, true, 18, 19)),
                timing_allow_origin: tao.map(String::from),
            }]
        };
        for (tao, no_referrer, count) in [
            (None, false, 0),
            (Some("https://c.test"), false, 0),
            (Some("https://c.test, https://a.test"), false, 1),
            (Some("*"), false, 1),
            (Some("https://a.test"), true, 0),
            (Some("*"), true, 1),
        ] {
            let hops = elsewhere(tao);
            let redirected = DocumentLoad {
                redirects: &hops,
                no_referrer,
                ..load(&[])
            };
            let timing = network_timing(Some(redirected), NavigationType::Navigate, 5.0, at, true);
            assert_eq!(timing.redirect_count, count, "{tao:?}");
            assert_eq!(timing.redirect_start.is_some(), count > 0, "{tao:?}");
        }
        // With one of another origin, every one must opt in: one of the
        // document's own before it that didn't hides them all.
        let mut chain = redirects.to_vec();
        chain.extend(elsewhere(Some("*")));
        let redirected = DocumentLoad {
            redirects: &chain,
            ..load(&[])
        };
        let timing = network_timing(Some(redirected), NavigationType::Navigate, 5.0, at, true);
        assert_eq!(timing.redirect_count, 0);
        for hop in &mut chain {
            hop.timing_allow_origin = Some("https://a.test".into());
        }
        let redirected = DocumentLoad {
            redirects: &chain,
            ..load(&[])
        };
        let timing = network_timing(Some(redirected), NavigationType::Navigate, 5.0, at, true);
        assert_eq!(timing.redirect_count, 3);
        // A reused connection, a name not resolved: at the fetch's start,
        // the request sent then; HTTP has no secure connection.
        let http = Url::parse("http://a.test/").unwrap();
        let reused = DocumentLoad {
            url: &http,
            timing: Some(load_timing(30, false, false, 40, 50)),
            ..load(&[])
        };
        let timing = network_timing(Some(reused), NavigationType::Navigate, 5.0, at, false);
        assert_eq!(
            [
                timing.domain_lookup_start,
                timing.domain_lookup_end,
                timing.connect_start,
                timing.connect_end,
                timing.request_start
            ],
            [Some(30.0); 5]
        );
        assert_eq!(timing.secure_connection_start, None);
        assert!(!timing.confidence_high);
        // Not loaded: every network time at the origin.
        let timing = network_timing(None, NavigationType::Navigate, 5.0, at, true);
        assert_eq!(
            (timing.fetch_start, timing.response_end),
            (Some(5.0), Some(5.0))
        );
        assert!(!timing.transferred);
    }

    /// Below the rate, the coin's; at or above it, the value itself.
    #[test]
    fn confidence_is_randomized_below_the_rate() {
        let below = (0.49 * 18_446_744_073_709_551_616.0) as u64;
        let above = (0.5 * 18_446_744_073_709_551_616.0) as u64;
        assert!(!randomize(true, 0, false));
        assert!(!randomize(true, below, false));
        assert!(randomize(false, below, true));
        assert!(randomize(true, above, false));
        assert!(!randomize(false, above, true));
    }
}
