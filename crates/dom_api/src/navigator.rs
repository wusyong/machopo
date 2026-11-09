//! `Navigator` (https://html.spec.whatwg.org/#the-navigator-object): what a
//! page learns of the browser, with Chrome's values: its user agent, the
//! browser's (the frame's, `FrameLayout::user_agent`), and the constants
//! Blink's `NavigatorID` and its kin answer. machopo has no language setting,
//! network state or cookies: those are fixed. And its `userActivation`
//! (HTML's `UserActivation`): the window's user activation, read live; its
//! `scheduling` (https://wicg.github.io/is-input-pending/, Blink's
//! `core/frame/scheduling.cc`), whose `isInputPending()` asks the host what
//! the input waiting for the main thread is for (`InputMonitor`); and its
//! `connection` (https://wicg.github.io/netinfo/, Blink's
//! `modules/netinfo`): machopo has no estimate of the network's quality
//! (Chromium's `NetworkQualityEstimator`), so it answers what Chrome does
//! without one, and never fires `change`.

use dom::{DomString, EventTargetImpl, Navigator, NodeId, Scheduling, UserActivation, Window};
use elise::{Gc, Persistent};

use crate::interfaces::IsInputPendingOptions;
use crate::typed::{DomStr, ScriptHost};
use crate::{ApiCx, ApiError};

/// What an input event waiting for the main thread is for, as
/// `isInputPending()` tells them apart (Blink's attribution of
/// `PendingUserInput`'s events).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingInput {
    /// A key's: the frame with the focus.
    FocusedFrame,
    /// A pointer's: the document the event is aimed at.
    Document(NodeId),
}

/// The input waiting for the main thread (Blink's
/// `ThreadScheduler::GetPendingUserInputInfo`): the host's, counted as the
/// thread that takes the input in hands it over, read when script asks.
pub trait InputMonitor {
    /// What the events waiting are for, each once; a continuous one's (a
    /// move, a wheel) only if `include_continuous`.
    fn pending_input(&self, include_continuous: bool) -> Vec<PendingInput>;
}

pub(crate) fn window_navigator<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<Gc<'gc, Navigator>, ApiError> {
    Ok(cx.window_navigator(this))
}

fn string<'gc>(text: &str) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::new(text)))
}

pub(crate) fn app_code_name<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Navigator>,
) -> Result<DomStr<'gc>, ApiError> {
    string("Mozilla")
}

pub(crate) fn app_name<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Navigator>,
) -> Result<DomStr<'gc>, ApiError> {
    string("Netscape")
}

/// The user agent past its `Mozilla/` (Blink's `NavigatorID::appVersion`).
pub(crate) fn app_version<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Navigator>,
) -> Result<DomStr<'gc>, ApiError> {
    let agent = agent(cx, this);
    string(agent.strip_prefix("Mozilla/").unwrap_or(&agent))
}

/// What Chrome says on the platform machopo was built for (Blink's
/// `NavigatorBase::platform`): `MacIntel` on every Mac.
pub(crate) fn platform<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Navigator>,
) -> Result<DomStr<'gc>, ApiError> {
    string(if cfg!(target_os = "macos") {
        "MacIntel"
    } else if cfg!(target_os = "windows") {
        "Win32"
    } else if cfg!(target_arch = "aarch64") {
        "Linux aarch64"
    } else {
        "Linux x86_64"
    })
}

pub(crate) fn product<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Navigator>,
) -> Result<DomStr<'gc>, ApiError> {
    string("Gecko")
}

/// Chrome's (HTML's when the navigator compatibility mode is Chrome's).
pub(crate) fn product_sub<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Navigator>,
) -> Result<DomStr<'gc>, ApiError> {
    string("20030107")
}

/// The browser's user agent, the DOM's (`Dom::set_agent`: its host's,
/// the one its requests send; every frame's, a new iframe's `about:blank`
/// too, as Blink's `LocalFrameClient::UserAgent` is there from the
/// frame's start); empty if the host gave none (a test's).
fn agent<'gc>(cx: &ApiCx<'_, 'gc>, _: Gc<'gc, Navigator>) -> String {
    cx.user_agent()
}

pub(crate) fn user_agent<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Navigator>,
) -> Result<DomStr<'gc>, ApiError> {
    string(&agent(cx, this))
}

pub(crate) fn vendor<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Navigator>,
) -> Result<DomStr<'gc>, ApiError> {
    string("Google Inc.")
}

pub(crate) fn vendor_sub<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Navigator>,
) -> Result<DomStr<'gc>, ApiError> {
    string("")
}

/// machopo has no language setting: what Chrome says in English.
pub(crate) fn language<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Navigator>,
) -> Result<DomStr<'gc>, ApiError> {
    string("en-US")
}

/// A new array each time (Chrome's is too).
pub(crate) fn languages<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Navigator>,
) -> Result<Vec<DomString>, ApiError> {
    Ok(vec![DomString::new("en-US"), DomString::new("en")])
}

/// machopo isn't told of the network going away.
pub(crate) fn on_line<'gc>(_: &ApiCx<'_, 'gc>, _: Gc<'gc, Navigator>) -> Result<bool, ApiError> {
    Ok(true)
}

/// machopo keeps no cookies.
pub(crate) fn cookie_enabled<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Navigator>,
) -> Result<bool, ApiError> {
    Ok(false)
}

/// The machine's logical cores, as Chrome reports them (WebIDL's
/// `unsigned long long`, never past `u32`'s).
pub(crate) fn hardware_concurrency<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Navigator>,
) -> Result<u32, ApiError> {
    Ok(std::thread::available_parallelism()
        .map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX)))
}

/// WebDriver's webdriver-active flag: whether automation controls the
/// browser, the DOM's (`Dom::set_agent`, Blink's from
/// `--enable-automation`); `false` if the host said nothing.
pub(crate) fn webdriver<'gc>(cx: &ApiCx<'_, 'gc>, _: Gc<'gc, Navigator>) -> Result<bool, ApiError> {
    Ok(cx.webdriver())
}

/// `userActivation`: the same object each time, which the navigator keeps.
pub(crate) fn user_activation<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Navigator>,
) -> Result<Gc<'gc, UserActivation>, ApiError> {
    Ok(cx.navigator_user_activation(this))
}

/// `hasBeenActive`: the window's sticky activation.
pub(crate) fn has_been_active<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UserActivation>,
) -> Result<bool, ApiError> {
    Ok(cx.has_sticky_activation(cx.user_activation_window(this)))
}

/// `isActive`: the window's transient activation.
pub(crate) fn is_active<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, UserActivation>,
) -> Result<bool, ApiError> {
    Ok(cx.has_transient_activation(cx.user_activation_window(this)))
}

/// `scheduling`: the same object each time, which the navigator keeps.
pub(crate) fn scheduling<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Navigator>,
) -> Result<Gc<'gc, Scheduling>, ApiError> {
    Ok(cx.navigator_scheduling(this))
}

/// `isInputPending(options)` (Blink's `Scheduling::isInputPending`):
/// whether an input event waits that this window may know of, a
/// continuous one only if `includeContinuous`; none in a document that
/// isn't fully active.
pub(crate) fn is_input_pending<H: ScriptHost>(
    host: &mut H,
    this: Persistent<Scheduling>,
    options: IsInputPendingOptions,
) -> Result<bool, ApiError> {
    let active = host.read(|cx| {
        crate::history::is_fully_active(
            cx,
            cx.window_document(cx.scheduling_window(this.get(cx.gc))),
        )
    });
    if !active {
        return Ok(false);
    }
    let pending = host.pending_input(options.includeContinuous);
    if pending.is_empty() {
        return Ok(false);
    }
    Ok(host.read(|cx| {
        let document = cx.window_document(cx.scheduling_window(this.get(cx.gc)));
        pending
            .iter()
            .any(|&input| can_access_event(cx, document, input))
    }))
}

/// Blink's `LocalFrame::CanAccessEvent`: a key's if this document is the
/// focused frame's; a pointer's if this document may reach the target's
/// (its origin is the same).
fn can_access_event<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: dom::NodeRef<'gc>,
    input: PendingInput,
) -> bool {
    match input {
        PendingInput::FocusedFrame => {
            Gc::ptr_eq(cx.focused_frame(cx.top_document(document)), document)
        }
        PendingInput::Document(target) => cx
            .node_by_id(target)
            .filter(|&target| {
                cx.kind(target) == dom::NodeKind::Document && cx.window_of(target).is_some()
            })
            .is_some_and(|target| crate::frame::same_origin(cx, target, document)),
    }
}

/// `connection`: the same event target each time, which the navigator
/// keeps.
pub(crate) fn connection<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Navigator>,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    Ok(cx.navigator_connection(this))
}

/// `effectiveType`: Blink's `EffectiveConnectionTypeToEnum` makes an
/// unknown type `4g`.
pub(crate) fn effective_type<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, EventTargetImpl>,
) -> Result<DomStr<'gc>, ApiError> {
    string("4g")
}

/// `rtt`, in ms: Blink's `NetworkStateNotifier::RoundRtt` is 0 without an
/// estimate.
pub(crate) fn rtt<'gc>(_: &ApiCx<'_, 'gc>, _: Gc<'gc, EventTargetImpl>) -> Result<u32, ApiError> {
    Ok(0)
}

/// `downlink`, in Mbit/s: without an estimate Blink's
/// `NetworkStateNotifier::RoundMbps` gives the fastest it reports, 10.
pub(crate) fn downlink<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, EventTargetImpl>,
) -> Result<f64, ApiError> {
    Ok(10.0)
}

/// `saveData`: machopo has no Data Saver (Chrome's desktop no longer has it).
pub(crate) fn save_data<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, EventTargetImpl>,
) -> Result<bool, ApiError> {
    Ok(false)
}
