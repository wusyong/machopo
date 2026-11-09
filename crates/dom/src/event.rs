//! The objects script holds besides nodes (Blink's `ScriptWrappable`s that
//! aren't `Node`s): an event (`Event`, https://dom.spec.whatwg.org/#interface-event)
//! and the event target `new EventTarget()` makes (`EventTargetImpl`,
//! Blink's), and what an operation makes and reads of them; event targets'
//! listeners (`Listener`), and an event's state as it is dispatched. Each
//! holds its JS engine's reference (a `JsEdge`: a wrapper, a listener's
//! callback), so it is old in a heap a JS engine drives, as a node is.
//!
//! A node's listeners are in its rare data (`NodeRareData`: in its
//! kind's data, a fragment's its data, made at its first listener), a flag
//! of the node saying it has some: a dispatch reads nothing more of a node
//! without.

use std::cell::{Cell, RefCell};

use elise::{Erased, Gc, GcObject, HeapVector, JsEdge, LocalExternal, Member, Persistent};

use crate::{
    CharacterData, CssStyleDeclaration, Cx, DoctypeData, DocumentData, DomImplementation, DomRect,
    DomRectList, DomString, DomStringMap, DomTokenList, ElementData, HAS_LISTENERS, HtmlCollection,
    Location, MutationObserver, MutationRecord, NameId, Node, NodeKind, NodeList, NodeRareData,
    NodeRareDataInit, NodeRef, NodeRoot, Window, names, sites,
};

/// An event: its type, its flags, when it was made, and what a dispatch
/// keeps on it (its target, its path, the listeners being called).
#[derive(GcObject)]
pub struct Event {
    /// Interned with the attribute values, as Blink's event types are
    /// `AtomicString`s: equal types are one string.
    event_type: Cell<NameId>,
    /// When it was made, in milliseconds since the DOM was (`Dom::origin`),
    /// at full precision: `timeStamp` coarsens it.
    time_stamp: Cell<f64>,
    /// Its flags (`BUBBLES` and the others), and its phase.
    flags: Cell<u16>,
    /// A node or an `EventTargetImpl` (`TargetRef`).
    target: Member<Erased>,
    current_target: Member<Erased>,
    /// The dispatch's path, its target first (DOM's event path, Blink's
    /// `EventPath`, which is the event's too), each a target as `target`
    /// is; and the listeners of the target being invoked (DOM's "clone
    /// listeners", Blink's `EventListenerVectorSnapshot`). Each is cleared
    /// and refilled: a dispatch allocates them once, an event dispatched
    /// again not at all. On the event, so that a dispatch, which runs
    /// script between operations, roots the event alone.
    path: Member<HeapVector<Erased>>,
    snapshot: Member<HeapVector<Listener>>,
    /// A dispatch's path with an entry in a shadow tree: what each entry
    /// sees, worked out before any listener (none for another dispatch).
    shadow: Member<ShadowPath>,
    /// A subclass's data (an `ErrorEvent`'s `ErrorEventData`), none for a
    /// plain event: what interface it is, without making every event
    /// bigger.
    pub(crate) data: Member<Erased>,
    wrapper: JsEdge,
}

/// A dispatch's path with an entry in a shadow tree, as it was when the
/// path was made (DOM's path's structs, Blink's `EventPath` and its
/// `NodeEventContext`s): each entry's shadow-adjusted target, its related
/// target (if the event has one), whether it was in a shadow tree (DOM's
/// "invocation-target-in-shadow-tree"), and whether the event's targets
/// are cleared at the end (DOM's "clear targets"). What a listener moves
/// meanwhile changes none of it.
#[derive(GcObject)]
pub struct ShadowPath {
    targets: Member<HeapVector<Erased>>,
    related: Member<HeapVector<Erased>>,
    in_shadow: LocalExternal<RefCell<Vec<bool>>>,
    clear: Cell<bool>,
    /// The entry being invoked.
    current: Cell<u32>,
}

/// One entry of a `ShadowPath`.
#[derive(Clone, Copy)]
pub struct ShadowEntry<'gc> {
    pub target: TargetRef<'gc>,
    pub related: Option<TargetRef<'gc>>,
    pub in_shadow: bool,
}

/// An `ErrorEvent`'s data (HTML's, Blink's `ErrorEvent`): where the error
/// was thrown, and the thrown value, the JS engine's (any value), which
/// only the engine reads and writes.
#[derive(GcObject)]
pub struct ErrorEventData {
    message: LocalExternal<DomString>,
    filename: LocalExternal<DomString>,
    lineno: Cell<u32>,
    colno: Cell<u32>,
    error: JsEdge,
}

/// A `PromiseRejectionEvent`'s data (HTML's, Blink's
/// `PromiseRejectionEvent`): the promise rejected with no handler and what
/// it was rejected with, the JS engine's, which only it reads and writes.
#[derive(GcObject)]
pub struct PromiseRejectionEventData {
    promise: JsEdge,
    reason: JsEdge,
}

/// A `CustomEvent`'s data (DOM's, Blink's `CustomEvent`): its `detail`,
/// any value of the JS engine's, which only it reads and writes (empty is
/// null, the default).
#[derive(GcObject)]
pub struct CustomEventData {
    detail: JsEdge,
}

/// Where an error was thrown: an `ErrorEvent`'s `message`, `filename`,
/// `lineno` and `colno`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorInfo {
    pub message: DomString,
    pub filename: DomString,
    pub lineno: u32,
    pub colno: u32,
}

/// The target `new EventTarget()` makes: an event target that is nothing
/// else; or a message port (its `port`).
#[derive(GcObject)]
pub struct EventTargetImpl {
    listeners: Member<HeapVector<Listener>>,
    /// A message port's data (`message`): none for `new EventTarget()`'s.
    pub(crate) port: Member<crate::MessagePort>,
    /// A `MediaQueryList`'s data (`media_query`).
    pub(crate) media_query: Member<crate::media_query::MediaQueryListData>,
    /// An animation's object's data (`animation_objects`).
    pub(crate) animation: Member<crate::animation_objects::AnimationObjectData>,
    /// A `FileReader`'s data (`file_reader`).
    pub(crate) file_reader: Member<crate::file_reader::FileReaderData>,
    /// A window's `performance`'s data (`performance`).
    pub(crate) performance: Member<crate::PerformanceData>,
    /// A `WakeLockSentinel`'s data (`wake_lock`).
    pub(crate) wake_lock: Member<crate::WakeLockSentinelData>,
    /// A navigator's `connection`'s window (`navigator`).
    pub(crate) connection: Member<Window>,
    /// An `AbortSignal`'s state (`abort`).
    pub(crate) abort: Member<crate::AbortSignalData>,
    /// `new EventTarget()`'s: the window whose script made it, its
    /// relevant global.
    window: Member<Window>,
    wrapper: JsEdge,
}

/// An event listener (DOM's, Blink's `RegisteredEventListener` and its
/// `JSEventListener`): its type, its flags, and its callback, the JS
/// engine's object (a function, or an object with `handleEvent`), which
/// only the engine reads and writes.
///
/// An event handler (HTML's) is a listener of its own too, `HANDLER` in its
/// flags (Blink's `JSEventHandler`): its callback is the compiled
/// function; one of a content attribute not compiled yet has its source
/// in an `UncompiledHandler` instead.
#[derive(GcObject)]
pub struct Listener {
    /// Its type (a name's number, `names::NAME_BITS` bits) and above it its
    /// flags: one word, so that with the handler's edge it is 16 bytes.
    type_and_flags: Cell<u32>,
    /// An event handler's source while it isn't compiled.
    uncompiled: Member<UncompiledHandler>,
    callback: JsEdge,
}

/// An event handler of a content attribute not compiled yet (Blink's
/// `JSEventHandlerForContentAttribute`): its source (the attribute's
/// value, a reference counted), its line, and its element (whose scope,
/// with its document's, it is compiled in; a `<body>`'s for its window).
#[derive(GcObject)]
pub struct UncompiledHandler {
    source: LocalExternal<DomString>,
    line: Cell<u32>,
    element: Member<Node>,
}

/// `Event::flags`.
const BUBBLES: u16 = 1 << 0;
const CANCELABLE: u16 = 1 << 1;
const COMPOSED: u16 = 1 << 2;
/// The user agent made it (DOM's "isTrusted").
const TRUSTED: u16 = 1 << 3;
const STOP_PROPAGATION: u16 = 1 << 4;
const STOP_IMMEDIATE_PROPAGATION: u16 = 1 << 5;
const CANCELED: u16 = 1 << 6;
const DISPATCH: u16 = 1 << 7;
const IN_PASSIVE_LISTENER: u16 = 1 << 8;
/// Its phase (`eventPhase`, 2 bits).
const PHASE_SHIFT: u16 = 9;
const PHASE_MASK: u16 = 0b11 << PHASE_SHIFT;
/// DOM's "initialized flag": unset only for `createEvent`'s events, until
/// an `init*Event` (a dispatch of one is an `InvalidStateError`).
const INITIALIZED: u16 = 1 << 11;

/// `Listener::type_and_flags`: its type below, its flags above.
const LISTENER_TYPE: u32 = (1 << names::NAME_BITS) - 1;
const CAPTURE: u32 = 1 << names::NAME_BITS;
const ONCE: u32 = CAPTURE << 1;
const PASSIVE: u32 = CAPTURE << 2;
/// DOM's "removed": set as it is removed, so that a dispatch that had it
/// in its snapshot skips it.
const REMOVED: u32 = CAPTURE << 3;
/// An event handler's listener.
const HANDLER: u32 = CAPTURE << 4;

/// What an event is made with (DOM's `EventInit`), and whether the user
/// agent made it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EventFlags {
    pub bubbles: bool,
    pub cancelable: bool,
    pub composed: bool,
    pub trusted: bool,
}

/// An event's flags a dispatch reads and sets (DOM's "stop propagation
/// flag" and the others).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EventState {
    pub stop_propagation: bool,
    pub stop_immediate_propagation: bool,
    pub canceled: bool,
    pub dispatching: bool,
    pub in_passive_listener: bool,
}

/// A listener's flags (`addEventListener`'s options).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListenerFlags {
    pub capture: bool,
    pub once: bool,
    pub passive: bool,
}

/// An event target, of whichever type (DOM's `EventTarget`).
#[derive(Clone, Copy)]
pub enum TargetRef<'gc> {
    Node(NodeRef<'gc>),
    Window(Gc<'gc, Window>),
    EventTarget(Gc<'gc, EventTargetImpl>),
}

impl<'gc> TargetRef<'gc> {
    /// As an edge to any type (an event's target, its path).
    pub fn erase(self) -> Gc<'gc, Erased> {
        match self {
            Self::Node(node) => node.erase(),
            Self::Window(window) => window.erase(),
            Self::EventTarget(target) => target.erase(),
        }
    }

    /// An erased target read back.
    ///
    /// # Panics
    ///
    /// If it is no target.
    pub fn from_erased(object: Gc<'gc, Erased>) -> Self {
        if let Some(node) = object.downcast::<Node>() {
            return Self::Node(node);
        }
        match object.downcast::<Window>() {
            Some(window) => Self::Window(window),
            None => Self::EventTarget(object.downcast().expect("an event target")),
        }
    }

    pub fn ptr_eq(self, other: Self) -> bool {
        Gc::ptr_eq(self.erase(), other.erase())
    }
}

/// A root of a `TargetRef` (`Cx::root_target`).
#[derive(Debug)]
pub enum TargetRoot {
    Node(NodeRoot),
    Window(Persistent<Window>),
    EventTarget(Persistent<EventTargetImpl>),
}

impl TargetRoot {
    pub fn get<'gc>(&self, cx: &elise::Mutation<'gc>) -> TargetRef<'gc> {
        match self {
            Self::Node(root) => TargetRef::Node(root.get(cx)),
            Self::Window(root) => TargetRef::Window(root.get(cx)),
            Self::EventTarget(root) => TargetRef::EventTarget(root.get(cx)),
        }
    }
}

/// An object script holds, of whichever type (Blink's `ScriptWrappable`):
/// what a binding gives back or wraps.
#[derive(Clone, Copy)]
pub enum ObjectRef<'gc> {
    Node(NodeRef<'gc>),
    Event(Gc<'gc, Event>),
    EventTarget(Gc<'gc, EventTargetImpl>),
    Window(Gc<'gc, Window>),
    Location(Gc<'gc, Location>),
    History(Gc<'gc, crate::History>),
    Navigator(Gc<'gc, crate::Navigator>),
    UserActivation(Gc<'gc, crate::UserActivation>),
    Url(Gc<'gc, crate::Url>),
    Crypto(Gc<'gc, crate::Crypto>),
    Request(Gc<'gc, crate::Request>),
    Response(Gc<'gc, crate::Response>),
    AbortController(Gc<'gc, crate::AbortController>),
    Headers(Gc<'gc, crate::Headers>),
    Geolocation(Gc<'gc, crate::Geolocation>),
    GeolocationPositionError(Gc<'gc, crate::GeolocationPositionError>),
    WakeLock(Gc<'gc, crate::WakeLock>),
    Scheduling(Gc<'gc, crate::Scheduling>),
    IdleDeadline(Gc<'gc, crate::IdleDeadline>),
    PerformanceTiming(Gc<'gc, crate::PerformanceTiming>),
    PerformanceObserverEntryList(Gc<'gc, crate::PerformanceObserverEntryList>),
    PerformanceObserver(Gc<'gc, crate::PerformanceObserver>),
    PerformanceEntry(Gc<'gc, crate::PerformanceEntry>),
    UrlSearchParams(Gc<'gc, crate::UrlSearchParams>),
    HtmlCollection(Gc<'gc, HtmlCollection>),
    NodeList(Gc<'gc, NodeList>),
    DomTokenList(Gc<'gc, DomTokenList>),
    DomStringMap(Gc<'gc, DomStringMap>),
    NamedNodeMap(Gc<'gc, crate::NamedNodeMap>),
    DomParser(Gc<'gc, crate::DomParser>),
    Range(Gc<'gc, crate::Range>),
    NodeIterator(Gc<'gc, crate::NodeIterator>),
    TreeWalker(Gc<'gc, crate::TreeWalker>),
    Selection(Gc<'gc, crate::Selection>),
    CssStyleDeclaration(Gc<'gc, CssStyleDeclaration>),
    MutationObserver(Gc<'gc, MutationObserver>),
    MutationRecord(Gc<'gc, MutationRecord>),
    IntersectionObserver(Gc<'gc, crate::IntersectionObserver>),
    IntersectionObserverEntry(Gc<'gc, crate::IntersectionObserverEntry>),
    ResizeObserver(Gc<'gc, crate::ResizeObserver>),
    ResizeObserverEntry(Gc<'gc, crate::ResizeObserverEntry>),
    ResizeObserverSize(Gc<'gc, crate::ResizeObserverSize>),
    CanvasRenderingContext2d(Gc<'gc, crate::CanvasRenderingContext2d>),
    CanvasGradient(Gc<'gc, crate::CanvasGradient>),
    CanvasPattern(Gc<'gc, crate::CanvasPattern>),
    ImageData(Gc<'gc, crate::ImageData>),
    Blob(Gc<'gc, crate::Blob>),
    Path2d(Gc<'gc, crate::Path2d>),
    TextMetrics(Gc<'gc, crate::TextMetrics>),
    MessageChannel(Gc<'gc, crate::MessageChannel>),
    SvgAnimatedString(Gc<'gc, crate::SvgAnimatedString>),
    SvgMatrix(Gc<'gc, crate::SvgMatrix>),
    SvgPoint(Gc<'gc, crate::SvgPoint>),
    SvgRect(Gc<'gc, crate::SvgRect>),
    DomRect(Gc<'gc, DomRect>),
    DomRectList(Gc<'gc, DomRectList>),
    DomImplementation(Gc<'gc, DomImplementation>),
    CssStyleSheet(Gc<'gc, crate::CssStyleSheet>),
    CssRule(Gc<'gc, crate::CssRule>),
    CssRuleList(Gc<'gc, crate::CssRuleList>),
    MediaList(Gc<'gc, crate::MediaList>),
    StyleSheetList(Gc<'gc, crate::StyleSheetList>),
    KeyframeEffect(Gc<'gc, crate::KeyframeEffectObject>),
    DocumentTimeline(Gc<'gc, crate::DocumentTimeline>),
    CustomElementRegistry(Gc<'gc, crate::CustomElementRegistry>),
}

impl<'gc> From<NodeRef<'gc>> for ObjectRef<'gc> {
    fn from(node: NodeRef<'gc>) -> Self {
        Self::Node(node)
    }
}

impl<'gc> From<Gc<'gc, Event>> for ObjectRef<'gc> {
    fn from(event: Gc<'gc, Event>) -> Self {
        Self::Event(event)
    }
}

impl<'gc> From<Gc<'gc, EventTargetImpl>> for ObjectRef<'gc> {
    fn from(target: Gc<'gc, EventTargetImpl>) -> Self {
        Self::EventTarget(target)
    }
}

impl<'gc> From<TargetRef<'gc>> for ObjectRef<'gc> {
    fn from(target: TargetRef<'gc>) -> Self {
        match target {
            TargetRef::Node(node) => Self::Node(node),
            TargetRef::Window(window) => Self::Window(window),
            TargetRef::EventTarget(target) => Self::EventTarget(target),
        }
    }
}

impl<'gc> From<Gc<'gc, Window>> for ObjectRef<'gc> {
    fn from(window: Gc<'gc, Window>) -> Self {
        Self::Window(window)
    }
}

impl<'gc> From<Gc<'gc, Location>> for ObjectRef<'gc> {
    fn from(location: Gc<'gc, Location>) -> Self {
        Self::Location(location)
    }
}

impl<'gc> From<Gc<'gc, crate::Navigator>> for ObjectRef<'gc> {
    fn from(navigator: Gc<'gc, crate::Navigator>) -> Self {
        Self::Navigator(navigator)
    }
}

impl<'gc> From<Gc<'gc, crate::PerformanceEntry>> for ObjectRef<'gc> {
    fn from(object: Gc<'gc, crate::PerformanceEntry>) -> Self {
        Self::PerformanceEntry(object)
    }
}

impl<'gc> From<Gc<'gc, crate::PerformanceObserver>> for ObjectRef<'gc> {
    fn from(object: Gc<'gc, crate::PerformanceObserver>) -> Self {
        Self::PerformanceObserver(object)
    }
}

impl<'gc> From<Gc<'gc, crate::PerformanceObserverEntryList>> for ObjectRef<'gc> {
    fn from(object: Gc<'gc, crate::PerformanceObserverEntryList>) -> Self {
        Self::PerformanceObserverEntryList(object)
    }
}

impl<'gc> From<Gc<'gc, crate::PerformanceTiming>> for ObjectRef<'gc> {
    fn from(object: Gc<'gc, crate::PerformanceTiming>) -> Self {
        Self::PerformanceTiming(object)
    }
}

impl<'gc> From<Gc<'gc, crate::Geolocation>> for ObjectRef<'gc> {
    fn from(object: Gc<'gc, crate::Geolocation>) -> Self {
        Self::Geolocation(object)
    }
}

impl<'gc> From<Gc<'gc, crate::GeolocationPositionError>> for ObjectRef<'gc> {
    fn from(object: Gc<'gc, crate::GeolocationPositionError>) -> Self {
        Self::GeolocationPositionError(object)
    }
}

impl<'gc> From<Gc<'gc, crate::WakeLock>> for ObjectRef<'gc> {
    fn from(object: Gc<'gc, crate::WakeLock>) -> Self {
        Self::WakeLock(object)
    }
}

impl<'gc> From<Gc<'gc, crate::Scheduling>> for ObjectRef<'gc> {
    fn from(object: Gc<'gc, crate::Scheduling>) -> Self {
        Self::Scheduling(object)
    }
}

impl<'gc> From<Gc<'gc, crate::IdleDeadline>> for ObjectRef<'gc> {
    fn from(object: Gc<'gc, crate::IdleDeadline>) -> Self {
        Self::IdleDeadline(object)
    }
}

impl<'gc> From<Gc<'gc, crate::AbortController>> for ObjectRef<'gc> {
    fn from(object: Gc<'gc, crate::AbortController>) -> Self {
        Self::AbortController(object)
    }
}

impl<'gc> From<Gc<'gc, crate::Headers>> for ObjectRef<'gc> {
    fn from(object: Gc<'gc, crate::Headers>) -> Self {
        Self::Headers(object)
    }
}

impl<'gc> From<Gc<'gc, crate::Request>> for ObjectRef<'gc> {
    fn from(object: Gc<'gc, crate::Request>) -> Self {
        Self::Request(object)
    }
}

impl<'gc> From<Gc<'gc, crate::Response>> for ObjectRef<'gc> {
    fn from(object: Gc<'gc, crate::Response>) -> Self {
        Self::Response(object)
    }
}

impl<'gc> From<Gc<'gc, crate::Crypto>> for ObjectRef<'gc> {
    fn from(crypto: Gc<'gc, crate::Crypto>) -> Self {
        Self::Crypto(crypto)
    }
}

impl<'gc> From<Gc<'gc, crate::Url>> for ObjectRef<'gc> {
    fn from(url: Gc<'gc, crate::Url>) -> Self {
        Self::Url(url)
    }
}

impl<'gc> From<Gc<'gc, crate::UrlSearchParams>> for ObjectRef<'gc> {
    fn from(params: Gc<'gc, crate::UrlSearchParams>) -> Self {
        Self::UrlSearchParams(params)
    }
}

impl<'gc> From<Gc<'gc, crate::UserActivation>> for ObjectRef<'gc> {
    fn from(activation: Gc<'gc, crate::UserActivation>) -> Self {
        Self::UserActivation(activation)
    }
}

impl<'gc> From<Gc<'gc, crate::History>> for ObjectRef<'gc> {
    fn from(history: Gc<'gc, crate::History>) -> Self {
        Self::History(history)
    }
}

impl<'gc> From<Gc<'gc, HtmlCollection>> for ObjectRef<'gc> {
    fn from(collection: Gc<'gc, HtmlCollection>) -> Self {
        Self::HtmlCollection(collection)
    }
}

impl<'gc> From<Gc<'gc, NodeList>> for ObjectRef<'gc> {
    fn from(list: Gc<'gc, NodeList>) -> Self {
        Self::NodeList(list)
    }
}

impl<'gc> From<Gc<'gc, DomTokenList>> for ObjectRef<'gc> {
    fn from(list: Gc<'gc, DomTokenList>) -> Self {
        Self::DomTokenList(list)
    }
}

impl<'gc> From<Gc<'gc, DomRect>> for ObjectRef<'gc> {
    fn from(rect: Gc<'gc, DomRect>) -> Self {
        Self::DomRect(rect)
    }
}

impl<'gc> From<Gc<'gc, crate::CustomElementRegistry>> for ObjectRef<'gc> {
    fn from(registry: Gc<'gc, crate::CustomElementRegistry>) -> Self {
        Self::CustomElementRegistry(registry)
    }
}

impl<'gc> From<Gc<'gc, crate::CssStyleSheet>> for ObjectRef<'gc> {
    fn from(sheet: Gc<'gc, crate::CssStyleSheet>) -> Self {
        Self::CssStyleSheet(sheet)
    }
}

impl<'gc> From<Gc<'gc, crate::CssRule>> for ObjectRef<'gc> {
    fn from(rule: Gc<'gc, crate::CssRule>) -> Self {
        Self::CssRule(rule)
    }
}

impl<'gc> From<Gc<'gc, crate::CssRuleList>> for ObjectRef<'gc> {
    fn from(list: Gc<'gc, crate::CssRuleList>) -> Self {
        Self::CssRuleList(list)
    }
}

impl<'gc> From<Gc<'gc, crate::MediaList>> for ObjectRef<'gc> {
    fn from(list: Gc<'gc, crate::MediaList>) -> Self {
        Self::MediaList(list)
    }
}

impl<'gc> From<Gc<'gc, crate::StyleSheetList>> for ObjectRef<'gc> {
    fn from(list: Gc<'gc, crate::StyleSheetList>) -> Self {
        Self::StyleSheetList(list)
    }
}

impl<'gc> From<Gc<'gc, DomImplementation>> for ObjectRef<'gc> {
    fn from(implementation: Gc<'gc, DomImplementation>) -> Self {
        Self::DomImplementation(implementation)
    }
}

impl<'gc> From<Gc<'gc, crate::KeyframeEffectObject>> for ObjectRef<'gc> {
    fn from(effect: Gc<'gc, crate::KeyframeEffectObject>) -> Self {
        Self::KeyframeEffect(effect)
    }
}

impl<'gc> From<Gc<'gc, DomRectList>> for ObjectRef<'gc> {
    fn from(list: Gc<'gc, DomRectList>) -> Self {
        Self::DomRectList(list)
    }
}

impl<'gc> From<Gc<'gc, MutationObserver>> for ObjectRef<'gc> {
    fn from(observer: Gc<'gc, MutationObserver>) -> Self {
        Self::MutationObserver(observer)
    }
}

impl<'gc> From<Gc<'gc, crate::IntersectionObserver>> for ObjectRef<'gc> {
    fn from(observer: Gc<'gc, crate::IntersectionObserver>) -> Self {
        Self::IntersectionObserver(observer)
    }
}

impl<'gc> From<Gc<'gc, crate::IntersectionObserverEntry>> for ObjectRef<'gc> {
    fn from(entry: Gc<'gc, crate::IntersectionObserverEntry>) -> Self {
        Self::IntersectionObserverEntry(entry)
    }
}

impl<'gc> From<Gc<'gc, crate::ResizeObserver>> for ObjectRef<'gc> {
    fn from(observer: Gc<'gc, crate::ResizeObserver>) -> Self {
        Self::ResizeObserver(observer)
    }
}

impl<'gc> From<Gc<'gc, crate::ResizeObserverEntry>> for ObjectRef<'gc> {
    fn from(entry: Gc<'gc, crate::ResizeObserverEntry>) -> Self {
        Self::ResizeObserverEntry(entry)
    }
}

impl<'gc> From<Gc<'gc, crate::ResizeObserverSize>> for ObjectRef<'gc> {
    fn from(size: Gc<'gc, crate::ResizeObserverSize>) -> Self {
        Self::ResizeObserverSize(size)
    }
}

impl<'gc> From<Gc<'gc, crate::CanvasRenderingContext2d>> for ObjectRef<'gc> {
    fn from(context: Gc<'gc, crate::CanvasRenderingContext2d>) -> Self {
        Self::CanvasRenderingContext2d(context)
    }
}

impl<'gc> From<Gc<'gc, crate::CanvasGradient>> for ObjectRef<'gc> {
    fn from(gradient: Gc<'gc, crate::CanvasGradient>) -> Self {
        Self::CanvasGradient(gradient)
    }
}

impl<'gc> From<Gc<'gc, crate::CanvasPattern>> for ObjectRef<'gc> {
    fn from(pattern: Gc<'gc, crate::CanvasPattern>) -> Self {
        Self::CanvasPattern(pattern)
    }
}

impl<'gc> From<Gc<'gc, crate::ImageData>> for ObjectRef<'gc> {
    fn from(image_data: Gc<'gc, crate::ImageData>) -> Self {
        Self::ImageData(image_data)
    }
}

impl<'gc> From<Gc<'gc, crate::SvgAnimatedString>> for ObjectRef<'gc> {
    fn from(animated: Gc<'gc, crate::SvgAnimatedString>) -> Self {
        Self::SvgAnimatedString(animated)
    }
}

impl<'gc> From<Gc<'gc, crate::SvgMatrix>> for ObjectRef<'gc> {
    fn from(matrix: Gc<'gc, crate::SvgMatrix>) -> Self {
        Self::SvgMatrix(matrix)
    }
}

impl<'gc> From<Gc<'gc, crate::SvgPoint>> for ObjectRef<'gc> {
    fn from(point: Gc<'gc, crate::SvgPoint>) -> Self {
        Self::SvgPoint(point)
    }
}

impl<'gc> From<Gc<'gc, crate::SvgRect>> for ObjectRef<'gc> {
    fn from(rect: Gc<'gc, crate::SvgRect>) -> Self {
        Self::SvgRect(rect)
    }
}

impl<'gc> From<Gc<'gc, crate::MessageChannel>> for ObjectRef<'gc> {
    fn from(channel: Gc<'gc, crate::MessageChannel>) -> Self {
        Self::MessageChannel(channel)
    }
}

impl<'gc> From<Gc<'gc, crate::TextMetrics>> for ObjectRef<'gc> {
    fn from(metrics: Gc<'gc, crate::TextMetrics>) -> Self {
        Self::TextMetrics(metrics)
    }
}

impl<'gc> From<Gc<'gc, crate::Path2d>> for ObjectRef<'gc> {
    fn from(path: Gc<'gc, crate::Path2d>) -> Self {
        Self::Path2d(path)
    }
}

impl<'gc> From<Gc<'gc, crate::Blob>> for ObjectRef<'gc> {
    fn from(blob: Gc<'gc, crate::Blob>) -> Self {
        Self::Blob(blob)
    }
}

impl<'gc> From<Gc<'gc, MutationRecord>> for ObjectRef<'gc> {
    fn from(record: Gc<'gc, MutationRecord>) -> Self {
        Self::MutationRecord(record)
    }
}

impl<'gc> From<Gc<'gc, CssStyleDeclaration>> for ObjectRef<'gc> {
    fn from(declaration: Gc<'gc, CssStyleDeclaration>) -> Self {
        Self::CssStyleDeclaration(declaration)
    }
}

impl<'gc> From<Gc<'gc, crate::DomParser>> for ObjectRef<'gc> {
    fn from(parser: Gc<'gc, crate::DomParser>) -> Self {
        Self::DomParser(parser)
    }
}

impl<'gc> From<Gc<'gc, crate::Range>> for ObjectRef<'gc> {
    fn from(range: Gc<'gc, crate::Range>) -> Self {
        Self::Range(range)
    }
}

impl<'gc> From<Gc<'gc, crate::NodeIterator>> for ObjectRef<'gc> {
    fn from(iterator: Gc<'gc, crate::NodeIterator>) -> Self {
        Self::NodeIterator(iterator)
    }
}

impl<'gc> From<Gc<'gc, crate::Selection>> for ObjectRef<'gc> {
    fn from(selection: Gc<'gc, crate::Selection>) -> Self {
        Self::Selection(selection)
    }
}

impl<'gc> From<Gc<'gc, crate::TreeWalker>> for ObjectRef<'gc> {
    fn from(walker: Gc<'gc, crate::TreeWalker>) -> Self {
        Self::TreeWalker(walker)
    }
}

impl<'gc> From<Gc<'gc, DomStringMap>> for ObjectRef<'gc> {
    fn from(map: Gc<'gc, DomStringMap>) -> Self {
        Self::DomStringMap(map)
    }
}

/// A root of an `ObjectRef`, kept past the operation (`Cx::root_object`).
#[derive(Debug)]
pub enum ObjectRoot {
    Node(NodeRoot),
    Event(Persistent<Event>),
    EventTarget(Persistent<EventTargetImpl>),
    Window(Persistent<Window>),
    Location(Persistent<Location>),
    History(Persistent<crate::History>),
    Navigator(Persistent<crate::Navigator>),
    UserActivation(Persistent<crate::UserActivation>),
    Url(Persistent<crate::Url>),
    Crypto(Persistent<crate::Crypto>),
    Request(Persistent<crate::Request>),
    Response(Persistent<crate::Response>),
    AbortController(Persistent<crate::AbortController>),
    Headers(Persistent<crate::Headers>),
    Geolocation(Persistent<crate::Geolocation>),
    GeolocationPositionError(Persistent<crate::GeolocationPositionError>),
    WakeLock(Persistent<crate::WakeLock>),
    Scheduling(Persistent<crate::Scheduling>),
    IdleDeadline(Persistent<crate::IdleDeadline>),
    PerformanceTiming(Persistent<crate::PerformanceTiming>),
    PerformanceObserverEntryList(Persistent<crate::PerformanceObserverEntryList>),
    PerformanceObserver(Persistent<crate::PerformanceObserver>),
    PerformanceEntry(Persistent<crate::PerformanceEntry>),
    UrlSearchParams(Persistent<crate::UrlSearchParams>),
    HtmlCollection(Persistent<HtmlCollection>),
    NodeList(Persistent<NodeList>),
    DomTokenList(Persistent<DomTokenList>),
    DomStringMap(Persistent<DomStringMap>),
    NamedNodeMap(Persistent<crate::NamedNodeMap>),
    DomParser(Persistent<crate::DomParser>),
    Range(Persistent<crate::Range>),
    NodeIterator(Persistent<crate::NodeIterator>),
    TreeWalker(Persistent<crate::TreeWalker>),
    Selection(Persistent<crate::Selection>),
    CssStyleDeclaration(Persistent<CssStyleDeclaration>),
    MutationObserver(Persistent<MutationObserver>),
    MutationRecord(Persistent<MutationRecord>),
    IntersectionObserver(Persistent<crate::IntersectionObserver>),
    IntersectionObserverEntry(Persistent<crate::IntersectionObserverEntry>),
    ResizeObserver(Persistent<crate::ResizeObserver>),
    ResizeObserverEntry(Persistent<crate::ResizeObserverEntry>),
    ResizeObserverSize(Persistent<crate::ResizeObserverSize>),
    CanvasRenderingContext2d(Persistent<crate::CanvasRenderingContext2d>),
    CanvasGradient(Persistent<crate::CanvasGradient>),
    CanvasPattern(Persistent<crate::CanvasPattern>),
    ImageData(Persistent<crate::ImageData>),
    Blob(Persistent<crate::Blob>),
    Path2d(Persistent<crate::Path2d>),
    TextMetrics(Persistent<crate::TextMetrics>),
    MessageChannel(Persistent<crate::MessageChannel>),
    SvgAnimatedString(Persistent<crate::SvgAnimatedString>),
    SvgMatrix(Persistent<crate::SvgMatrix>),
    SvgPoint(Persistent<crate::SvgPoint>),
    SvgRect(Persistent<crate::SvgRect>),
    DomRect(Persistent<DomRect>),
    DomRectList(Persistent<DomRectList>),
    DomImplementation(Persistent<DomImplementation>),
    CssStyleSheet(Persistent<crate::CssStyleSheet>),
    CssRule(Persistent<crate::CssRule>),
    CssRuleList(Persistent<crate::CssRuleList>),
    MediaList(Persistent<crate::MediaList>),
    StyleSheetList(Persistent<crate::StyleSheetList>),
    KeyframeEffect(Persistent<crate::KeyframeEffectObject>),
    DocumentTimeline(Persistent<crate::DocumentTimeline>),
    CustomElementRegistry(Persistent<crate::CustomElementRegistry>),
}

impl ObjectRoot {
    pub fn get<'gc>(&self, cx: &elise::Mutation<'gc>) -> ObjectRef<'gc> {
        match self {
            Self::Node(root) => ObjectRef::Node(root.get(cx)),
            Self::Event(root) => ObjectRef::Event(root.get(cx)),
            Self::EventTarget(root) => ObjectRef::EventTarget(root.get(cx)),
            Self::Window(root) => ObjectRef::Window(root.get(cx)),
            Self::Location(root) => ObjectRef::Location(root.get(cx)),
            Self::History(root) => ObjectRef::History(root.get(cx)),
            Self::Navigator(root) => ObjectRef::Navigator(root.get(cx)),
            Self::UserActivation(root) => ObjectRef::UserActivation(root.get(cx)),
            Self::Url(root) => ObjectRef::Url(root.get(cx)),
            Self::Crypto(root) => ObjectRef::Crypto(root.get(cx)),
            Self::Request(root) => ObjectRef::Request(root.get(cx)),
            Self::Response(root) => ObjectRef::Response(root.get(cx)),
            Self::AbortController(root) => ObjectRef::AbortController(root.get(cx)),
            Self::Headers(root) => ObjectRef::Headers(root.get(cx)),
            Self::Geolocation(root) => ObjectRef::Geolocation(root.get(cx)),
            Self::GeolocationPositionError(root) => {
                ObjectRef::GeolocationPositionError(root.get(cx))
            }
            Self::WakeLock(root) => ObjectRef::WakeLock(root.get(cx)),
            Self::Scheduling(root) => ObjectRef::Scheduling(root.get(cx)),
            Self::IdleDeadline(root) => ObjectRef::IdleDeadline(root.get(cx)),
            Self::PerformanceTiming(root) => ObjectRef::PerformanceTiming(root.get(cx)),
            Self::PerformanceObserverEntryList(root) => {
                ObjectRef::PerformanceObserverEntryList(root.get(cx))
            }
            Self::PerformanceObserver(root) => ObjectRef::PerformanceObserver(root.get(cx)),
            Self::PerformanceEntry(root) => ObjectRef::PerformanceEntry(root.get(cx)),
            Self::UrlSearchParams(root) => ObjectRef::UrlSearchParams(root.get(cx)),
            Self::HtmlCollection(root) => ObjectRef::HtmlCollection(root.get(cx)),
            Self::NodeList(root) => ObjectRef::NodeList(root.get(cx)),
            Self::DomTokenList(root) => ObjectRef::DomTokenList(root.get(cx)),
            Self::DomStringMap(root) => ObjectRef::DomStringMap(root.get(cx)),
            Self::NamedNodeMap(root) => ObjectRef::NamedNodeMap(root.get(cx)),
            Self::DomParser(root) => ObjectRef::DomParser(root.get(cx)),
            Self::Range(root) => ObjectRef::Range(root.get(cx)),
            Self::NodeIterator(root) => ObjectRef::NodeIterator(root.get(cx)),
            Self::TreeWalker(root) => ObjectRef::TreeWalker(root.get(cx)),
            Self::Selection(root) => ObjectRef::Selection(root.get(cx)),
            Self::CssStyleDeclaration(root) => ObjectRef::CssStyleDeclaration(root.get(cx)),
            Self::MutationObserver(root) => ObjectRef::MutationObserver(root.get(cx)),
            Self::MutationRecord(root) => ObjectRef::MutationRecord(root.get(cx)),
            Self::IntersectionObserver(root) => ObjectRef::IntersectionObserver(root.get(cx)),
            Self::IntersectionObserverEntry(root) => {
                ObjectRef::IntersectionObserverEntry(root.get(cx))
            }
            Self::ResizeObserver(root) => ObjectRef::ResizeObserver(root.get(cx)),
            Self::ResizeObserverEntry(root) => ObjectRef::ResizeObserverEntry(root.get(cx)),
            Self::ResizeObserverSize(root) => ObjectRef::ResizeObserverSize(root.get(cx)),
            Self::CanvasRenderingContext2d(root) => {
                ObjectRef::CanvasRenderingContext2d(root.get(cx))
            }
            Self::CanvasGradient(root) => ObjectRef::CanvasGradient(root.get(cx)),
            Self::CanvasPattern(root) => ObjectRef::CanvasPattern(root.get(cx)),
            Self::ImageData(root) => ObjectRef::ImageData(root.get(cx)),
            Self::Blob(root) => ObjectRef::Blob(root.get(cx)),
            Self::Path2d(root) => ObjectRef::Path2d(root.get(cx)),
            Self::MessageChannel(root) => ObjectRef::MessageChannel(root.get(cx)),
            Self::TextMetrics(root) => ObjectRef::TextMetrics(root.get(cx)),
            Self::SvgAnimatedString(root) => ObjectRef::SvgAnimatedString(root.get(cx)),
            Self::SvgMatrix(root) => ObjectRef::SvgMatrix(root.get(cx)),
            Self::SvgPoint(root) => ObjectRef::SvgPoint(root.get(cx)),
            Self::SvgRect(root) => ObjectRef::SvgRect(root.get(cx)),
            Self::DomRect(root) => ObjectRef::DomRect(root.get(cx)),
            Self::DomRectList(root) => ObjectRef::DomRectList(root.get(cx)),
            Self::DomImplementation(root) => ObjectRef::DomImplementation(root.get(cx)),
            Self::CssStyleSheet(root) => ObjectRef::CssStyleSheet(root.get(cx)),
            Self::CssRule(root) => ObjectRef::CssRule(root.get(cx)),
            Self::CssRuleList(root) => ObjectRef::CssRuleList(root.get(cx)),
            Self::MediaList(root) => ObjectRef::MediaList(root.get(cx)),
            Self::KeyframeEffect(root) => ObjectRef::KeyframeEffect(root.get(cx)),
            Self::DocumentTimeline(root) => ObjectRef::DocumentTimeline(root.get(cx)),
            Self::StyleSheetList(root) => ObjectRef::StyleSheetList(root.get(cx)),
            Self::CustomElementRegistry(root) => ObjectRef::CustomElementRegistry(root.get(cx)),
        }
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// A new event of `event_type` (DOM's "create an event", then its
    /// initialization), made now: its time since its relevant global's
    /// time origin (`window`'s; the DOM's start with none).
    pub fn create_event(
        &self,
        event_type: NameId,
        flags: EventFlags,
        window: Option<Gc<'gc, Window>>,
    ) -> Gc<'gc, Event> {
        self.gc.alloc_in(
            &sites::OTHER,
            EventInit {
                event_type: Cell::new(event_type),
                time_stamp: Cell::new(self.now_since(window)),
                flags: Cell::new(flag_bits(flags) | INITIALIZED),
                target: None,
                current_target: None,
                path: None,
                snapshot: None,
                shadow: None,
                data: None,
                wrapper: (),
            },
        )
    }

    /// A new event of no type, not initialized (DOM's "create an event" of
    /// `createEvent`): an `init*Event` initializes it.
    pub fn create_uninitialized_event(&self, window: Option<Gc<'gc, Window>>) -> Gc<'gc, Event> {
        let event = self.create_event(names::NONE, EventFlags::default(), window);
        set_bits(&event.flags, INITIALIZED, false);
        event
    }

    /// Whether `event` is initialized (DOM's "initialized flag").
    pub fn event_initialized(&self, event: Gc<'gc, Event>) -> bool {
        event.flags.get() & INITIALIZED != 0
    }

    /// Makes `event` a `CustomEvent`, its `detail` null (the engine's to
    /// set).
    pub fn set_custom_event_data(&self, event: Gc<'gc, Event>) {
        let data = self
            .gc
            .alloc_in(&sites::OTHER, CustomEventDataInit { detail: () });
        event.data.set(self.gc, Some(data.erase()));
    }

    /// `event`'s `CustomEvent` data, if it is one.
    pub fn custom_event_data(&self, event: Gc<'gc, Event>) -> Option<Gc<'gc, CustomEventData>> {
        event.data.get(self.gc)?.downcast::<CustomEventData>()
    }

    /// A `CustomEvent`'s reference to its `detail`, which only the JS
    /// engine reads and writes.
    pub fn detail_edge(&self, data: Gc<'gc, CustomEventData>) -> &'gc JsEdge {
        &data.into_ref().detail
    }

    /// A new `ErrorEvent` (HTML's "report an exception"'s): an `error`
    /// event, cancelable, not bubbling, made by the user agent, of `info`;
    /// its error value is the engine's to set.
    pub fn create_error_event(
        &self,
        info: &ErrorInfo,
        window: Option<Gc<'gc, Window>>,
    ) -> Gc<'gc, Event> {
        self.create_error_event_of(
            names::event::ERROR,
            EventFlags {
                cancelable: true,
                trusted: true,
                ..EventFlags::default()
            },
            info,
            window,
        )
    }

    /// A new `ErrorEvent` of `event_type` with `flags` and `info` (`new
    /// ErrorEvent()`'s); its error is the engine's to set.
    pub fn create_error_event_of(
        &self,
        event_type: NameId,
        flags: EventFlags,
        info: &ErrorInfo,
        window: Option<Gc<'gc, Window>>,
    ) -> Gc<'gc, Event> {
        let event = self.create_event(event_type, flags, window);
        let data = self.gc.alloc_in(
            &sites::OTHER,
            ErrorEventDataInit {
                message: LocalExternal::new(info.message.clone()),
                filename: LocalExternal::new(info.filename.clone()),
                lineno: Cell::new(info.lineno),
                colno: Cell::new(info.colno),
                error: (),
            },
        );
        event.data.set(self.gc, Some(data.erase()));
        event
    }

    /// A new `PromiseRejectionEvent` of `event_type` (`unhandledrejection`:
    /// HTML's "notify about rejected promises"), cancelable, not bubbling,
    /// made by the user agent; its promise and reason are the engine's to
    /// set.
    pub fn create_promise_rejection_event(
        &self,
        event_type: NameId,
        window: Option<Gc<'gc, Window>>,
    ) -> Gc<'gc, Event> {
        let event = self.create_event(
            event_type,
            EventFlags {
                cancelable: true,
                trusted: true,
                ..EventFlags::default()
            },
            window,
        );
        let data = self.gc.alloc_in(
            &sites::OTHER,
            PromiseRejectionEventDataInit {
                promise: (),
                reason: (),
            },
        );
        event.data.set(self.gc, Some(data.erase()));
        event
    }

    /// `event`'s `PromiseRejectionEvent` data, if it is one.
    pub fn promise_rejection_data(
        &self,
        event: Gc<'gc, Event>,
    ) -> Option<Gc<'gc, PromiseRejectionEventData>> {
        event
            .data
            .get(self.gc)?
            .downcast::<PromiseRejectionEventData>()
    }

    /// A `PromiseRejectionEvent`'s references to its promise and its
    /// reason, which only the JS engine reads and writes.
    pub fn promise_edge(&self, data: Gc<'gc, PromiseRejectionEventData>) -> &'gc JsEdge {
        &data.into_ref().promise
    }

    pub fn reason_edge(&self, data: Gc<'gc, PromiseRejectionEventData>) -> &'gc JsEdge {
        &data.into_ref().reason
    }

    /// `event`'s `ErrorEvent` data, if it is one.
    pub fn error_event_data(&self, event: Gc<'gc, Event>) -> Option<Gc<'gc, ErrorEventData>> {
        event.data.get(self.gc)?.downcast::<ErrorEventData>()
    }

    pub fn error_info(&self, data: Gc<'gc, ErrorEventData>) -> ErrorInfo {
        let data = data.into_ref();
        ErrorInfo {
            message: data.message.get().clone(),
            filename: data.filename.get().clone(),
            lineno: data.lineno.get(),
            colno: data.colno.get(),
        }
    }

    /// An `ErrorEvent`'s reference to its error value, which only the JS
    /// engine reads and writes.
    pub fn error_edge(&self, data: Gc<'gc, ErrorEventData>) -> &'gc JsEdge {
        &data.into_ref().error
    }

    /// `event`'s type: a name's number (Blink's event types are
    /// `AtomicString`s), the fixed ones `names::event`'s.
    pub fn event_type(&self, event: Gc<'gc, Event>) -> NameId {
        event.event_type.get()
    }

    pub fn event_flags(&self, event: Gc<'gc, Event>) -> EventFlags {
        let bits = event.flags.get();
        EventFlags {
            bubbles: bits & BUBBLES != 0,
            cancelable: bits & CANCELABLE != 0,
            composed: bits & COMPOSED != 0,
            trusted: bits & TRUSTED != 0,
        }
    }

    /// DOM's "initialize" (`initEvent`): initialized, a new type and flags,
    /// its propagation and cancellation flags cleared, not trusted, no
    /// target.
    pub fn init_event(
        &self,
        event: Gc<'gc, Event>,
        event_type: NameId,
        bubbles: bool,
        cancelable: bool,
    ) {
        event.event_type.set(event_type);
        let kept = event.flags.get() & (COMPOSED | PHASE_MASK);
        event.flags.set(
            kept | INITIALIZED
                | flag_bits(EventFlags {
                    bubbles,
                    cancelable,
                    ..EventFlags::default()
                }),
        );
        event.target.set(self.gc, None);
    }

    /// Sets `event`'s type, as DOM's legacy event types do while the
    /// listeners of a legacy one are called.
    pub fn set_event_type(&self, event: Gc<'gc, Event>, event_type: NameId) {
        event.event_type.set(event_type);
    }

    /// Sets whether the user agent made `event` (`dispatchEvent` clears it).
    pub fn set_event_trusted(&self, event: Gc<'gc, Event>, trusted: bool) {
        set_bits(&event.flags, TRUSTED, trusted);
    }

    pub fn event_state(&self, event: Gc<'gc, Event>) -> EventState {
        let bits = event.flags.get();
        EventState {
            stop_propagation: bits & STOP_PROPAGATION != 0,
            stop_immediate_propagation: bits & STOP_IMMEDIATE_PROPAGATION != 0,
            canceled: bits & CANCELED != 0,
            dispatching: bits & DISPATCH != 0,
            in_passive_listener: bits & IN_PASSIVE_LISTENER != 0,
        }
    }

    pub fn set_event_state(&self, event: Gc<'gc, Event>, state: EventState) {
        for (on, bit) in [
            (state.stop_propagation, STOP_PROPAGATION),
            (state.stop_immediate_propagation, STOP_IMMEDIATE_PROPAGATION),
            (state.canceled, CANCELED),
            (state.dispatching, DISPATCH),
            (state.in_passive_listener, IN_PASSIVE_LISTENER),
        ] {
            set_bits(&event.flags, bit, on);
        }
    }

    /// `eventPhase`: 0 (`NONE`) to 3 (`BUBBLING_PHASE`).
    pub fn event_phase(&self, event: Gc<'gc, Event>) -> u16 {
        (event.flags.get() & PHASE_MASK) >> PHASE_SHIFT
    }

    /// # Panics
    ///
    /// If `phase` is past `BUBBLING_PHASE` (3).
    pub fn set_event_phase(&self, event: Gc<'gc, Event>, phase: u16) {
        assert!(phase <= 3, "an event phase");
        event
            .flags
            .set((event.flags.get() & !PHASE_MASK) | (phase << PHASE_SHIFT));
    }

    pub fn event_target(&self, event: Gc<'gc, Event>) -> Option<TargetRef<'gc>> {
        event.target.get(self.gc).map(TargetRef::from_erased)
    }

    pub fn set_event_target(&self, event: Gc<'gc, Event>, target: Option<TargetRef<'gc>>) {
        event.target.set(self.gc, target.map(TargetRef::erase));
    }

    pub fn event_current_target(&self, event: Gc<'gc, Event>) -> Option<TargetRef<'gc>> {
        event
            .current_target
            .get(self.gc)
            .map(TargetRef::from_erased)
    }

    pub fn set_event_current_target(&self, event: Gc<'gc, Event>, target: Option<TargetRef<'gc>>) {
        event
            .current_target
            .set(self.gc, target.map(TargetRef::erase));
    }

    /// What each entry of `event`'s path sees (`ShadowPath`), and whether
    /// its targets are cleared at the end.
    pub fn set_event_shadow_path(
        &self,
        event: Gc<'gc, Event>,
        entries: &[ShadowEntry<'gc>],
        clear: bool,
    ) {
        let targets = HeapVector::new(self.gc, entries.len());
        let related = HeapVector::new(self.gc, entries.len());
        for entry in entries {
            targets.push(self.gc, entry.target.erase());
            if let Some(entry) = entry.related {
                related.push(self.gc, entry.erase());
            }
        }
        let path = self.gc.alloc_in(
            &sites::OTHER,
            ShadowPathInit {
                targets: Some(targets),
                related: Some(related),
                in_shadow: LocalExternal::new(RefCell::new(
                    entries.iter().map(|entry| entry.in_shadow).collect(),
                )),
                clear: Cell::new(clear),
                current: Cell::new(0),
            },
        );
        event.shadow.set(self.gc, Some(path));
    }

    /// The `ShadowPath` of `event`'s dispatch forgotten (at its end).
    pub fn clear_event_shadow_path(&self, event: Gc<'gc, Event>) {
        event.shadow.set(self.gc, None);
    }

    /// What `event`'s path's entry `index` sees, if its dispatch has a
    /// `ShadowPath`.
    pub fn event_shadow_entry(
        &self,
        event: Gc<'gc, Event>,
        index: usize,
    ) -> Option<ShadowEntry<'gc>> {
        let path = event.shadow.get(self.gc)?.into_ref();
        let target = path.targets.get(self.gc)?.get(self.gc, index)?;
        let related = path.related.get(self.gc)?;
        Some(ShadowEntry {
            target: TargetRef::from_erased(target),
            // All or none of the entries have one.
            related: related.get(self.gc, index).map(TargetRef::from_erased),
            in_shadow: path
                .in_shadow
                .get()
                .borrow()
                .get(index)
                .copied()
                .unwrap_or(false),
        })
    }

    /// The entry of `event`'s `ShadowPath` being invoked made `index`.
    pub fn set_event_shadow_current(&self, event: Gc<'gc, Event>, index: usize) {
        if let Some(path) = event.shadow.get(self.gc) {
            path.current.set(u32::try_from(index).unwrap_or(u32::MAX));
        }
    }

    /// What the entry of `event`'s path being invoked sees, if its
    /// dispatch has a `ShadowPath`.
    pub fn event_shadow_current(&self, event: Gc<'gc, Event>) -> Option<ShadowEntry<'gc>> {
        let current = event.shadow.get(self.gc)?.current.get();
        self.event_shadow_entry(event, usize::try_from(current).ok()?)
    }

    /// Whether `event`'s dispatch clears its targets at its end (its
    /// `ShadowPath`'s).
    pub fn event_shadow_clears(&self, event: Gc<'gc, Event>) -> bool {
        event
            .shadow
            .get(self.gc)
            .is_some_and(|path| path.clear.get())
    }

    /// `event`'s path made `targets`, in its own list, refilled.
    pub fn set_event_path(&self, event: Gc<'gc, Event>, targets: &[TargetRef<'gc>]) {
        let path = match event.path.get(self.gc) {
            Some(path) => {
                path.clear(self.gc);
                path
            }
            None => {
                let path = HeapVector::new(self.gc, targets.len());
                event.path.set(self.gc, Some(path));
                path
            }
        };
        for target in targets {
            path.push(self.gc, target.erase());
        }
    }

    /// `event`'s path, its target first; empty out of a dispatch.
    pub fn event_path(&self, event: Gc<'gc, Event>) -> Vec<TargetRef<'gc>> {
        let Some(path) = event.path.get(self.gc) else {
            return Vec::new();
        };
        (0..path.len())
            .filter_map(|index| path.get(self.gc, index))
            .map(TargetRef::from_erased)
            .collect()
    }

    pub fn event_path_len(&self, event: Gc<'gc, Event>) -> usize {
        event.path.get(self.gc).map_or(0, |path| path.len())
    }

    pub fn event_path_at(&self, event: Gc<'gc, Event>, index: usize) -> Option<TargetRef<'gc>> {
        event
            .path
            .get(self.gc)?
            .get(self.gc, index)
            .map(TargetRef::from_erased)
    }

    /// `event`'s path emptied, its list kept for its next dispatch.
    pub fn clear_event_path(&self, event: Gc<'gc, Event>) {
        if let Some(path) = event.path.get(self.gc) {
            path.clear(self.gc);
        }
    }

    /// `event`'s snapshot made `target`'s listeners of its type, in their
    /// order (DOM's "clone listeners"): how many.
    pub fn snapshot_listeners(&self, event: Gc<'gc, Event>, target: TargetRef<'gc>) -> usize {
        let snapshot = match event.snapshot.get(self.gc) {
            Some(snapshot) => {
                snapshot.clear(self.gc);
                snapshot
            }
            None => {
                let snapshot = HeapVector::new(self.gc, 1);
                event.snapshot.set(self.gc, Some(snapshot));
                snapshot
            }
        };
        if let Some(listeners) = self.listeners(target) {
            let event_type = event.event_type.get();
            for index in 0..listeners.len() {
                let listener = listeners.get(self.gc, index).expect("below the length");
                if listener.type_and_flags.get() & LISTENER_TYPE == event_type {
                    snapshot.push(self.gc, listener);
                }
            }
        }
        snapshot.len()
    }

    /// As `snapshot_listeners`, `target`'s listeners of `legacy_type`
    /// (DOM's legacy event types: a trusted event where there are none of
    /// its own type). Its own loop: the one every target of a dispatch
    /// takes stays as it is.
    pub fn snapshot_legacy_listeners(
        &self,
        event: Gc<'gc, Event>,
        target: TargetRef<'gc>,
        legacy_type: NameId,
    ) -> usize {
        let snapshot = event.snapshot.get(self.gc).expect("snapshot taken first");
        snapshot.clear(self.gc);
        if let Some(listeners) = self.listeners(target) {
            for index in 0..listeners.len() {
                let listener = listeners.get(self.gc, index).expect("below the length");
                if listener.type_and_flags.get() & LISTENER_TYPE == legacy_type {
                    snapshot.push(self.gc, listener);
                }
            }
        }
        snapshot.len()
    }

    /// The listener at `index` of `event`'s snapshot.
    pub fn snapshot_at(&self, event: Gc<'gc, Event>, index: usize) -> Option<Gc<'gc, Listener>> {
        event.snapshot.get(self.gc)?.get(self.gc, index)
    }

    /// When `event` was made, in milliseconds since its relevant global's
    /// time origin (the DOM's start with none), at full precision.
    pub fn event_time_stamp(&self, event: Gc<'gc, Event>) -> f64 {
        event.time_stamp.get()
    }

    /// A new event target that is nothing else (`new EventTarget()`).
    /// `target`, made by `new EventTarget()`, of `window`'s script.
    pub fn set_event_target_window(
        &self,
        target: Gc<'gc, EventTargetImpl>,
        window: Option<Gc<'gc, Window>>,
    ) {
        target.window.set(self.gc, window);
    }

    pub fn create_event_target(&self) -> Gc<'gc, EventTargetImpl> {
        self.gc.alloc_in(
            &sites::OTHER,
            EventTargetImplInit {
                listeners: None,
                port: None,
                media_query: None,
                animation: None,
                file_reader: None,
                performance: None,
                wake_lock: None,
                connection: None,
                abort: None,
                window: None,
                wrapper: (),
            },
        )
    }

    /// `target`'s listeners, in the order they were added; none if it never
    /// had one.
    pub fn listeners(&self, target: TargetRef<'gc>) -> Option<Gc<'gc, HeapVector<Listener>>> {
        match target {
            TargetRef::Node(node) => {
                if !self.flag(node, HAS_LISTENERS) {
                    return None;
                }
                self.node_listeners(node, false)?.get(self.gc)
            }
            TargetRef::Window(window) => window.listeners.get(self.gc),
            TargetRef::EventTarget(target) => target.listeners.get(self.gc),
        }
    }

    /// `node`'s rare data (its listeners, its observers' registrations):
    /// in its kind's data, a fragment's its data (a template's contents'
    /// in theirs); made first, with its
    /// kind's data, if `make` and it has none.
    pub(crate) fn node_rare(
        &self,
        node: NodeRef<'gc>,
        make: bool,
    ) -> Option<Gc<'gc, NodeRareData>> {
        let data = node.data.get(self.gc);
        let edge: &'gc Member<NodeRareData> = match self.kind(node) {
            NodeKind::Text | NodeKind::Comment | NodeKind::ProcessingInstruction => {
                &data?.downcast::<CharacterData>()?.into_ref().rare
            }
            NodeKind::DocumentType => &data?.downcast::<DoctypeData>()?.into_ref().rare,
            NodeKind::Attr => &data?.downcast::<crate::attr::AttrData>()?.into_ref().rare,
            NodeKind::Document => {
                let data = match data.and_then(|data| data.downcast::<DocumentData>()) {
                    Some(data) => data,
                    None if make => self.ensure_document_data(node),
                    None => return None,
                };
                &data.into_ref().rare
            }
            NodeKind::Element => {
                let data = match data.and_then(|data| data.downcast::<ElementData>()) {
                    Some(data) => data,
                    None if make => self.element_data(node),
                    None => return None,
                };
                &data.into_ref().rare
            }
            NodeKind::DocumentFragment if self.is_shadow_root(node) => {
                self.shadow_rare(node).expect("a shadow root's data")
            }
            NodeKind::DocumentFragment
                if let Some(content) =
                    data.and_then(|data| data.downcast::<crate::TemplateContentData>()) =>
            {
                &content.into_ref().rare
            }
            NodeKind::DocumentFragment => {
                return match data.and_then(|data| data.downcast::<NodeRareData>()) {
                    Some(data) => Some(data),
                    None if make => {
                        let data = self.new_rare_data(node);
                        node.data.set(self.gc, Some(data.erase()));
                        Some(data)
                    }
                    None => None,
                };
            }
        };
        if let Some(rare) = edge.get(self.gc) {
            return Some(rare);
        }
        if !make {
            return None;
        }
        let rare = self.new_rare_data(node);
        edge.set(self.gc, Some(rare));
        Some(rare)
    }

    /// Rare data for `node`, beside it.
    fn new_rare_data(&self, node: NodeRef<'gc>) -> Gc<'gc, NodeRareData> {
        self.gc.alloc_beside(
            node,
            &sites::OTHER,
            NodeRareDataInit {
                listeners: None,
                observers: None,
                assigned_slot: None,
                manual_slot: None,
                child_nodes: None,
                children: None,
                collections: None,
                named_collections: None,
                animations: None,
            },
        )
    }

    /// `node`'s listeners' edge, in its rare data; that made first if
    /// `make` and it has none.
    fn node_listeners(
        &self,
        node: NodeRef<'gc>,
        make: bool,
    ) -> Option<&'gc Member<HeapVector<Listener>>> {
        Some(&self.node_rare(node, make)?.into_ref().listeners)
    }

    /// `target`'s listeners, made if it has none.
    fn ensure_listeners(&self, target: TargetRef<'gc>) -> Gc<'gc, HeapVector<Listener>> {
        let edge = match target {
            TargetRef::Node(node) => {
                self.set_flag(node, HAS_LISTENERS, true);
                self.node_listeners(node, true)
                    .expect("every kind of node has a place for listeners")
            }
            TargetRef::Window(window) => &window.into_ref().listeners,
            TargetRef::EventTarget(target) => &target.into_ref().listeners,
        };
        match edge.get(self.gc) {
            Some(listeners) => listeners,
            None => {
                let listeners = HeapVector::new(self.gc, 1);
                edge.set(self.gc, Some(listeners));
                listeners
            }
        }
    }

    /// A new listener of `event_type`, added last to `target`'s: its
    /// callback is the engine's to set (`callback_edge`).
    pub fn add_listener(
        &self,
        target: TargetRef<'gc>,
        event_type: NameId,
        flags: ListenerFlags,
    ) -> Gc<'gc, Listener> {
        let bits = [
            (flags.capture, CAPTURE),
            (flags.once, ONCE),
            (flags.passive, PASSIVE),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .fold(0, |bits, (_, bit)| bits | bit);
        self.push_listener(target, event_type | bits)
    }

    fn push_listener(&self, target: TargetRef<'gc>, mut type_and_flags: u32) -> Gc<'gc, Listener> {
        let event_type = type_and_flags & LISTENER_TYPE;
        // An event handler's listener is passive where DOM's default is
        // (HTML adds it with no `passive`).
        if type_and_flags & HANDLER != 0 && self.default_passive(target, event_type) {
            type_and_flags |= PASSIVE;
        }
        if is_wheel_type(event_type) {
            self.wheel_listeners_changed(target, 1);
        }
        let listener = self.gc.alloc_in(
            &sites::OTHER,
            ListenerInit {
                type_and_flags: Cell::new(type_and_flags),
                uncompiled: None,
                callback: (),
            },
        );
        self.ensure_listeners(target).push(self.gc, listener);
        self.abort_listeners_changed(target, event_type);
        if let TargetRef::EventTarget(target) = target {
            self.media_query_list_listened(target);
        }
        listener
    }

    /// `target`'s listener of `event_type` and `capture` whose callback
    /// `same` says is the one asked for (the engine compares callbacks), if
    /// it has one.
    pub fn find_listener(
        &self,
        target: TargetRef<'gc>,
        event_type: NameId,
        capture: bool,
        mut same: impl FnMut(&'gc JsEdge) -> bool,
    ) -> Option<Gc<'gc, Listener>> {
        let listeners = self.listeners(target)?;
        (0..listeners.len())
            .filter_map(|index| listeners.get(self.gc, index))
            .find(|listener| {
                let listener = listener.into_ref();
                let bits = listener.type_and_flags.get();
                // An event handler's callback is no listener's.
                bits & LISTENER_TYPE == event_type
                    && bits & HANDLER == 0
                    && (bits & CAPTURE != 0) == capture
                    && same(&listener.callback)
            })
    }

    /// `target`'s event handler of `event_type` (its listener), if it has
    /// one.
    pub fn event_handler(
        &self,
        target: TargetRef<'gc>,
        event_type: NameId,
    ) -> Option<Gc<'gc, Listener>> {
        let listeners = self.listeners(target)?;
        (0..listeners.len())
            .filter_map(|index| listeners.get(self.gc, index))
            .find(|listener| {
                listener.type_and_flags.get() & (LISTENER_TYPE | HANDLER) == event_type | HANDLER
            })
    }

    /// HTML's setting of `target`'s event handler of `event_type` to an
    /// internal raw uncompiled handler (a content attribute's `source` at
    /// `line`, of `element`): its listener kept where it is if it has
    /// one, else added last ("activate"); compiled when first needed.
    pub fn set_uncompiled_handler(
        &self,
        target: TargetRef<'gc>,
        event_type: NameId,
        source: DomString,
        line: u32,
        element: NodeRef<'gc>,
    ) {
        let listener = self.activate_handler(target, event_type);
        let uncompiled = self.gc.alloc_in(
            &sites::OTHER,
            UncompiledHandlerInit {
                source: LocalExternal::new(source),
                line: Cell::new(line),
                element: Some(element),
            },
        );
        listener.uncompiled.set(self.gc, Some(uncompiled));
    }

    /// `target`'s event handler of `event_type` made one whose callback is
    /// compiled (an IDL attribute's function, or its source just
    /// compiled): its listener, whose callback the engine sets
    /// (`callback_edge`).
    pub fn set_compiled_handler(
        &self,
        target: TargetRef<'gc>,
        event_type: NameId,
    ) -> Gc<'gc, Listener> {
        let listener = self.activate_handler(target, event_type);
        listener.uncompiled.set(self.gc, None);
        listener
    }

    fn activate_handler(&self, target: TargetRef<'gc>, event_type: NameId) -> Gc<'gc, Listener> {
        match self.event_handler(target, event_type) {
            Some(listener) => listener,
            None => self.push_listener(target, event_type | HANDLER),
        }
    }

    /// HTML's "deactivate an event handler": its listener removed (set
    /// again, it is added last); nothing if it has none.
    pub fn remove_event_handler(&self, target: TargetRef<'gc>, event_type: NameId) {
        if let Some(listener) = self.event_handler(target, event_type) {
            listener.uncompiled.set(self.gc, None);
            self.remove_listener(target, listener);
        }
    }

    /// An event handler's listener's source, line and element, if it isn't
    /// compiled yet.
    pub fn uncompiled_handler(
        &self,
        listener: Gc<'gc, Listener>,
    ) -> Option<(DomString, u32, NodeRef<'gc>)> {
        let uncompiled = listener.uncompiled.get(self.gc)?.into_ref();
        Some((
            uncompiled.source.get().clone(),
            uncompiled.line.get(),
            uncompiled
                .element
                .get(self.gc)
                .expect("an uncompiled handler's element"),
        ))
    }

    /// HTML's attribute change steps of `element`'s event handler content
    /// attribute (`handler`'s, named for its type): set, the handler made
    /// an uncompiled one of the attribute's value, at the parser's line
    /// (1 for script's); removed, deactivated. An HTML or SVG element's global
    /// handler is its own; a `<body>`'s or `<frameset>`'s window and
    /// window-reflecting ones are its document's window's, if it has one.
    /// Out of line and cold: an attribute's change pays the table's read
    /// alone.
    #[cold]
    #[inline(never)]
    pub(crate) fn handler_attribute_changed(&self, element: NodeRef<'gc>, attribute: NameId) {
        let Some(handler) = names::handler_attribute(attribute) else {
            return;
        };
        let Some(name) = self.element_name(element) else {
            return;
        };
        // HTML's and SVG's elements have the event handlers.
        let html = name.namespace == names::HTML_NAMESPACE;
        if !html && name.namespace != names::SVG_NAMESPACE {
            return;
        }
        let body =
            html && (name.local == names::local::BODY || name.local == names::local::FRAMESET);
        let target = if body && (handler.window || handler.reflecting) {
            match self.window_of(self.document_of(element)) {
                Some(window) => TargetRef::Window(window),
                None => return,
            }
        } else if handler.global {
            TargetRef::Node(element)
        } else {
            return;
        };
        match self.attribute_value_by_id(element, attribute) {
            Some(value) => {
                let line = self.changes.parser_line.get().max(1);
                self.set_uncompiled_handler(
                    target,
                    handler.event_type,
                    value.to_owned(),
                    line,
                    element,
                );
            }
            None => self.remove_event_handler(target, handler.event_type),
        }
    }

    /// The line the parser sets attributes or inserts nodes at from now on
    /// (0: it doesn't).
    pub fn set_parser_line(&self, line: u32) {
        self.changes.parser_line.set(line);
    }

    /// Whether `listener` is an event handler's whose source isn't
    /// compiled yet.
    pub fn handler_uncompiled(&self, listener: Gc<'gc, Listener>) -> bool {
        listener.uncompiled.get(self.gc).is_some()
    }

    /// Whether `listener` is an event handler's.
    pub fn listener_is_handler(&self, listener: Gc<'gc, Listener>) -> bool {
        listener.type_and_flags.get() & HANDLER != 0
    }

    /// `listener`'s type.
    pub fn listener_type(&self, listener: Gc<'gc, Listener>) -> NameId {
        listener.type_and_flags.get() & LISTENER_TYPE
    }

    /// `listener` taken out of `target`'s and marked removed (a dispatch
    /// holding it skips it); nothing if it isn't one of them.
    pub fn remove_listener(&self, target: TargetRef<'gc>, listener: Gc<'gc, Listener>) {
        let Some(listeners) = self.listeners(target) else {
            return;
        };
        let found = (0..listeners.len()).find(|&index| {
            listeners
                .get(self.gc, index)
                .is_some_and(|other| Gc::ptr_eq(other, listener))
        });
        if let Some(index) = found {
            listeners.remove(self.gc, index);
            let bits = listener.type_and_flags.get();
            listener.type_and_flags.set(bits | REMOVED);
            if is_wheel_type(bits & LISTENER_TYPE) {
                self.wheel_listeners_changed(target, -1);
            }
            self.abort_listeners_changed(target, bits & LISTENER_TYPE);
        }
    }

    /// An abort signal's `abort` listeners changed: whether its sources
    /// keep it (`abort`'s `update_kept`).
    fn abort_listeners_changed(&self, target: TargetRef<'gc>, event_type: NameId) {
        if let TargetRef::EventTarget(signal) = target
            && event_type == names::event::ABORT
            && self.is_abort_signal(signal)
        {
            self.update_kept(signal);
        }
    }

    /// DOM's "default passive value" of a listener of `event_type` on
    /// `target`: a touch or wheel listener on a window, a document, its
    /// document element or its body is passive unless it says otherwise
    /// (the intervention Chrome made first): scrolling needn't wait for
    /// listeners that almost never cancel it.
    pub fn default_passive(&self, target: TargetRef<'gc>, event_type: NameId) -> bool {
        if !matches!(
            event_type,
            names::event::TOUCHSTART
                | names::event::TOUCHMOVE
                | names::event::WHEEL
                | names::event::MOUSEWHEEL
        ) {
            return false;
        }
        let node = match target {
            TargetRef::Window(_) => return true,
            TargetRef::EventTarget(_) => return false,
            TargetRef::Node(node) => node,
        };
        if self.kind(node) == NodeKind::Document {
            return true;
        }
        let document = self.document_of(node);
        let Some(root) = self.document_element(document) else {
            return false;
        };
        Gc::ptr_eq(node, root)
            || self
                .body(document)
                .is_some_and(|body| Gc::ptr_eq(node, body))
    }

    /// `document`'s document element: its element child.
    pub(crate) fn document_element(&self, document: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        std::iter::successors(self.first_child(document), |&child| {
            self.next_sibling(child)
        })
        .find(|&child| self.kind(child) == NodeKind::Element)
    }

    /// HTML's "the body element" of `document`: its `<html>` document
    /// element's first `<body>` or `<frameset>` child.
    fn body(&self, document: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let is_html = |node: NodeRef<'gc>, locals: &[NameId]| {
            self.element_name(node).is_some_and(|name| {
                name.namespace == names::HTML_NAMESPACE && locals.contains(&name.local)
            })
        };
        let root = self
            .document_element(document)
            .filter(|&root| is_html(root, &[names::local::HTML]))?;
        std::iter::successors(self.first_child(root), |&child| self.next_sibling(child))
            .find(|&child| is_html(child, &[names::local::BODY, names::local::FRAMESET]))
    }

    /// A `wheel` or `mousewheel` listener came (`delta` 1) or went (-1) on
    /// `target`: counted, and the element whose style says it is inside a
    /// blocking one (`has_own_blocking_wheel_listener`: the root element
    /// for its document's and window's) styled again.
    fn wheel_listeners_changed(&self, target: TargetRef<'gc>, delta: i32) {
        let changes = self.changes;
        changes
            .wheel_listeners
            .set(changes.wheel_listeners.get().saturating_add_signed(delta));
        changes.wheel_listeners_version.set(
            changes
                .wheel_listeners_version
                .get()
                .checked_add(1)
                .expect("fewer than 2^64 changes"),
        );
        let element = match target {
            TargetRef::Node(node) if self.kind(node) == NodeKind::Element => Some(node),
            TargetRef::Node(node) if self.kind(node) == NodeKind::Document => {
                self.document_element(node)
            }
            TargetRef::Window(window) => self.document_element(self.window_document(window)),
            // A shadow root's is over its tree: its elements at the top.
            TargetRef::Node(root) if self.is_shadow_root(root) => {
                let mut child = self.first_child(root);
                while let Some(at) = child {
                    if self.kind(at) == NodeKind::Element {
                        self.wheel_listener_styled(at);
                    }
                    child = self.next_sibling(at);
                }
                None
            }
            TargetRef::Node(_) | TargetRef::EventTarget(_) => None,
        };
        if let Some(element) = element {
            self.wheel_listener_styled(element);
        }
    }

    /// Whether there may be a `wheel` or `mousewheel` listener: false only
    /// if there is none (Blink's `EventHandlerRegistry::HasEventHandlers`).
    pub fn may_have_wheel_listeners(&self) -> bool {
        self.changes.wheel_listeners.get() > 0
    }

    /// Goes up each time a `wheel` or `mousewheel` listener is added or
    /// removed: the compositor's wheel event handler region may have
    /// changed.
    pub fn wheel_listeners_version(&self) -> u64 {
        self.changes.wheel_listeners_version.get()
    }

    /// Whether `document`'s root element has a blocking wheel listener, or
    /// it or its window has (`has_own_blocking_wheel_listener`): a wheel
    /// anywhere on it waits for the main thread.
    pub fn document_blocks_wheel(&self, document: NodeRef<'gc>) -> bool {
        self.document_element(document)
            .is_some_and(|root| self.has_own_blocking_wheel_listener(root))
    }

    /// Whether `element` has a blocking (not passive) `wheel` or
    /// `mousewheel` listener of its own, or, the root element, its
    /// document or its window has, or, a shadow root's child, the shadow
    /// root has (it is over its tree): what style adds to what the element
    /// inherits of `inside_blocking_wheel_event_handler` (Blink's
    /// `LayoutObject::InsideBlockingWheelEventHandler` from its node's).
    pub fn has_own_blocking_wheel_listener(&self, element: NodeRef<'gc>) -> bool {
        if self.changes.wheel_listeners.get() == 0 {
            return false;
        }
        if self.has_blocking_wheel_listener(TargetRef::Node(element)) {
            return true;
        }
        let Some(parent) = self.parent(element) else {
            return false;
        };
        if self.kind(parent) != NodeKind::Document {
            return self.is_shadow_root(parent)
                && self.has_blocking_wheel_listener(TargetRef::Node(parent));
        }
        self.has_blocking_wheel_listener(TargetRef::Node(parent))
            || self
                .window_of(parent)
                .is_some_and(|window| self.has_blocking_wheel_listener(TargetRef::Window(window)))
    }

    fn has_blocking_wheel_listener(&self, target: TargetRef<'gc>) -> bool {
        let Some(listeners) = self.listeners(target) else {
            return false;
        };
        (0..listeners.len()).any(|index| {
            let bits = listeners
                .get(self.gc, index)
                .expect("below the length")
                .type_and_flags
                .get();
            is_wheel_type(bits & LISTENER_TYPE) && bits & PASSIVE == 0
        })
    }

    pub fn listener_flags(&self, listener: Gc<'gc, Listener>) -> ListenerFlags {
        let bits = listener.type_and_flags.get();
        ListenerFlags {
            capture: bits & CAPTURE != 0,
            once: bits & ONCE != 0,
            passive: bits & PASSIVE != 0,
        }
    }

    pub fn listener_removed(&self, listener: Gc<'gc, Listener>) -> bool {
        listener.type_and_flags.get() & REMOVED != 0
    }

    /// `listener`'s reference to its callback, which only the JS engine
    /// reads and writes.
    pub fn callback_edge(&self, listener: Gc<'gc, Listener>) -> &'gc JsEdge {
        &listener.into_ref().callback
    }

    /// `object`'s reference to its JS engine's wrapper, which only the
    /// engine reads and writes.
    pub fn object_wrapper_edge(&self, object: ObjectRef<'gc>) -> &'gc JsEdge {
        match object {
            ObjectRef::Node(node) => self.wrapper_edge(node),
            ObjectRef::Event(event) => &event.into_ref().wrapper,
            ObjectRef::EventTarget(target) => &target.into_ref().wrapper,
            ObjectRef::Window(window) => self.window_wrapper_edge(window),
            ObjectRef::Location(location) => self.location_wrapper_edge(location),
            ObjectRef::History(history) => self.history_wrapper_edge(history),
            ObjectRef::Navigator(navigator) => self.navigator_wrapper_edge(navigator),
            ObjectRef::UserActivation(activation) => self.user_activation_wrapper_edge(activation),
            ObjectRef::Url(url) => self.url_wrapper_edge(url),
            ObjectRef::Crypto(crypto) => self.crypto_wrapper_edge(crypto),
            ObjectRef::Request(request) => self.request_wrapper_edge(request),
            ObjectRef::Response(response) => self.response_wrapper_edge(response),
            ObjectRef::AbortController(controller) => {
                self.abort_controller_wrapper_edge(controller)
            }
            ObjectRef::Headers(headers) => self.headers_wrapper_edge(headers),
            ObjectRef::Geolocation(geolocation) => self.geolocation_wrapper_edge(geolocation),
            ObjectRef::GeolocationPositionError(error) => {
                self.geolocation_position_error_wrapper_edge(error)
            }
            ObjectRef::WakeLock(wake_lock) => self.wake_lock_wrapper_edge(wake_lock),
            ObjectRef::Scheduling(scheduling) => self.scheduling_wrapper_edge(scheduling),
            ObjectRef::IdleDeadline(deadline) => self.idle_deadline_wrapper_edge(deadline),
            ObjectRef::PerformanceTiming(object) => self.performance_timing_wrapper_edge(object),
            ObjectRef::PerformanceObserverEntryList(object) => self.entry_list_wrapper_edge(object),
            ObjectRef::PerformanceObserver(object) => {
                self.performance_observer_wrapper_edge(object)
            }
            ObjectRef::PerformanceEntry(object) => self.performance_entry_wrapper_edge(object),
            ObjectRef::UrlSearchParams(params) => self.search_params_wrapper_edge(params),
            ObjectRef::HtmlCollection(collection) => self.collection_wrapper_edge(collection),
            ObjectRef::NodeList(list) => self.node_list_wrapper_edge(list),
            ObjectRef::DomTokenList(list) => self.token_list_wrapper_edge(list),
            ObjectRef::DomStringMap(map) => self.string_map_wrapper_edge(map),
            ObjectRef::NamedNodeMap(map) => self.attribute_map_wrapper_edge(map),
            ObjectRef::DomParser(parser) => self.dom_parser_wrapper_edge(parser),
            ObjectRef::Range(range) => self.range_wrapper_edge(range),
            ObjectRef::NodeIterator(iterator) => self.node_iterator_wrapper_edge(iterator),
            ObjectRef::TreeWalker(walker) => self.tree_walker_wrapper_edge(walker),
            ObjectRef::Selection(selection) => self.selection_wrapper_edge(selection),
            ObjectRef::CssStyleDeclaration(declaration) => {
                self.style_declaration_wrapper_edge(declaration)
            }
            ObjectRef::MutationObserver(observer) => self.observer_wrapper_edge(observer),
            ObjectRef::MutationRecord(record) => self.record_wrapper_edge(record),
            ObjectRef::IntersectionObserver(observer) => {
                self.intersection_observer_wrapper_edge(observer)
            }
            ObjectRef::IntersectionObserverEntry(entry) => {
                self.intersection_entry_wrapper_edge(entry)
            }
            ObjectRef::ResizeObserver(observer) => self.resize_observer_wrapper_edge(observer),
            ObjectRef::ResizeObserverEntry(entry) => self.resize_entry_wrapper_edge(entry),
            ObjectRef::ResizeObserverSize(size) => self.resize_size_wrapper_edge(size),
            ObjectRef::CanvasRenderingContext2d(context) => {
                self.canvas_context_wrapper_edge(context)
            }
            ObjectRef::CanvasGradient(gradient) => self.canvas_gradient_wrapper_edge(gradient),
            ObjectRef::CanvasPattern(pattern) => self.canvas_pattern_wrapper_edge(pattern),
            ObjectRef::ImageData(image_data) => self.image_data_wrapper_edge(image_data),
            ObjectRef::Blob(blob) => self.blob_wrapper_edge(blob),
            ObjectRef::Path2d(path) => self.path_2d_wrapper_edge(path),
            ObjectRef::TextMetrics(metrics) => self.text_metrics_wrapper_edge(metrics),
            ObjectRef::MessageChannel(channel) => self.message_channel_wrapper_edge(channel),
            ObjectRef::SvgAnimatedString(animated) => {
                self.svg_animated_string_wrapper_edge(animated)
            }
            ObjectRef::SvgMatrix(matrix) => self.svg_matrix_wrapper_edge(matrix),
            ObjectRef::SvgPoint(point) => self.svg_point_wrapper_edge(point),
            ObjectRef::SvgRect(rect) => self.svg_rect_wrapper_edge(rect),
            ObjectRef::DomRect(rect) => self.rect_wrapper_edge(rect),
            ObjectRef::DomRectList(list) => self.rect_list_wrapper_edge(list),
            ObjectRef::DomImplementation(implementation) => {
                self.dom_implementation_wrapper_edge(implementation)
            }
            ObjectRef::CssStyleSheet(sheet) => self.style_sheet_wrapper_edge(sheet),
            ObjectRef::CssRule(rule) => self.css_rule_wrapper_edge(rule),
            ObjectRef::CssRuleList(list) => self.css_rule_list_wrapper_edge(list),
            ObjectRef::MediaList(list) => self.media_list_wrapper_edge(list),
            ObjectRef::KeyframeEffect(effect) => self.effect_wrapper_edge(effect),
            ObjectRef::DocumentTimeline(timeline) => self.timeline_wrapper_edge(timeline),
            ObjectRef::StyleSheetList(list) => self.style_sheet_list_wrapper_edge(list),
            ObjectRef::CustomElementRegistry(registry) => {
                self.custom_elements_wrapper_edge(registry)
            }
        }
    }

    /// A root of `object`, kept past the operation.
    pub fn root_object(&self, object: ObjectRef<'gc>) -> ObjectRoot {
        match object {
            ObjectRef::Node(node) => ObjectRoot::Node(self.root(node)),
            ObjectRef::Event(event) => ObjectRoot::Event(Persistent::new(self.gc, event)),
            ObjectRef::EventTarget(target) => {
                ObjectRoot::EventTarget(Persistent::new(self.gc, target))
            }
            ObjectRef::Window(window) => ObjectRoot::Window(Persistent::new(self.gc, window)),
            ObjectRef::Location(location) => {
                ObjectRoot::Location(Persistent::new(self.gc, location))
            }
            ObjectRef::History(history) => ObjectRoot::History(Persistent::new(self.gc, history)),
            ObjectRef::Navigator(navigator) => {
                ObjectRoot::Navigator(Persistent::new(self.gc, navigator))
            }
            ObjectRef::UserActivation(activation) => {
                ObjectRoot::UserActivation(Persistent::new(self.gc, activation))
            }
            ObjectRef::Url(url) => ObjectRoot::Url(Persistent::new(self.gc, url)),
            ObjectRef::Crypto(crypto) => ObjectRoot::Crypto(Persistent::new(self.gc, crypto)),
            ObjectRef::Request(request) => ObjectRoot::Request(Persistent::new(self.gc, request)),
            ObjectRef::Response(response) => {
                ObjectRoot::Response(Persistent::new(self.gc, response))
            }
            ObjectRef::AbortController(controller) => {
                ObjectRoot::AbortController(Persistent::new(self.gc, controller))
            }
            ObjectRef::Headers(headers) => ObjectRoot::Headers(Persistent::new(self.gc, headers)),
            ObjectRef::Geolocation(geolocation) => {
                ObjectRoot::Geolocation(Persistent::new(self.gc, geolocation))
            }
            ObjectRef::GeolocationPositionError(error) => {
                ObjectRoot::GeolocationPositionError(Persistent::new(self.gc, error))
            }
            ObjectRef::WakeLock(wake_lock) => {
                ObjectRoot::WakeLock(Persistent::new(self.gc, wake_lock))
            }
            ObjectRef::Scheduling(scheduling) => {
                ObjectRoot::Scheduling(Persistent::new(self.gc, scheduling))
            }
            ObjectRef::IdleDeadline(deadline) => {
                ObjectRoot::IdleDeadline(Persistent::new(self.gc, deadline))
            }
            ObjectRef::PerformanceTiming(object) => {
                ObjectRoot::PerformanceTiming(Persistent::new(self.gc, object))
            }
            ObjectRef::PerformanceObserverEntryList(object) => {
                ObjectRoot::PerformanceObserverEntryList(Persistent::new(self.gc, object))
            }
            ObjectRef::PerformanceObserver(object) => {
                ObjectRoot::PerformanceObserver(Persistent::new(self.gc, object))
            }
            ObjectRef::PerformanceEntry(object) => {
                ObjectRoot::PerformanceEntry(Persistent::new(self.gc, object))
            }
            ObjectRef::UrlSearchParams(params) => {
                ObjectRoot::UrlSearchParams(Persistent::new(self.gc, params))
            }
            ObjectRef::HtmlCollection(collection) => {
                ObjectRoot::HtmlCollection(Persistent::new(self.gc, collection))
            }
            ObjectRef::NodeList(list) => ObjectRoot::NodeList(Persistent::new(self.gc, list)),
            ObjectRef::DomTokenList(list) => {
                ObjectRoot::DomTokenList(Persistent::new(self.gc, list))
            }
            ObjectRef::DomStringMap(map) => ObjectRoot::DomStringMap(Persistent::new(self.gc, map)),
            ObjectRef::NamedNodeMap(map) => ObjectRoot::NamedNodeMap(Persistent::new(self.gc, map)),
            ObjectRef::DomParser(parser) => ObjectRoot::DomParser(Persistent::new(self.gc, parser)),
            ObjectRef::Range(range) => ObjectRoot::Range(Persistent::new(self.gc, range)),
            ObjectRef::NodeIterator(iterator) => {
                ObjectRoot::NodeIterator(Persistent::new(self.gc, iterator))
            }
            ObjectRef::TreeWalker(walker) => {
                ObjectRoot::TreeWalker(Persistent::new(self.gc, walker))
            }
            ObjectRef::Selection(selection) => {
                ObjectRoot::Selection(Persistent::new(self.gc, selection))
            }
            ObjectRef::CssStyleDeclaration(declaration) => {
                ObjectRoot::CssStyleDeclaration(Persistent::new(self.gc, declaration))
            }
            ObjectRef::MutationObserver(observer) => {
                ObjectRoot::MutationObserver(Persistent::new(self.gc, observer))
            }
            ObjectRef::MutationRecord(record) => {
                ObjectRoot::MutationRecord(Persistent::new(self.gc, record))
            }
            ObjectRef::IntersectionObserver(observer) => {
                ObjectRoot::IntersectionObserver(Persistent::new(self.gc, observer))
            }
            ObjectRef::IntersectionObserverEntry(entry) => {
                ObjectRoot::IntersectionObserverEntry(Persistent::new(self.gc, entry))
            }
            ObjectRef::ResizeObserver(observer) => {
                ObjectRoot::ResizeObserver(Persistent::new(self.gc, observer))
            }
            ObjectRef::ResizeObserverEntry(entry) => {
                ObjectRoot::ResizeObserverEntry(Persistent::new(self.gc, entry))
            }
            ObjectRef::ResizeObserverSize(size) => {
                ObjectRoot::ResizeObserverSize(Persistent::new(self.gc, size))
            }
            ObjectRef::CanvasRenderingContext2d(context) => {
                ObjectRoot::CanvasRenderingContext2d(Persistent::new(self.gc, context))
            }
            ObjectRef::CanvasGradient(gradient) => {
                ObjectRoot::CanvasGradient(Persistent::new(self.gc, gradient))
            }
            ObjectRef::CanvasPattern(pattern) => {
                ObjectRoot::CanvasPattern(Persistent::new(self.gc, pattern))
            }
            ObjectRef::ImageData(image_data) => {
                ObjectRoot::ImageData(Persistent::new(self.gc, image_data))
            }
            ObjectRef::Blob(blob) => ObjectRoot::Blob(Persistent::new(self.gc, blob)),
            ObjectRef::Path2d(path) => ObjectRoot::Path2d(Persistent::new(self.gc, path)),
            ObjectRef::TextMetrics(metrics) => {
                ObjectRoot::TextMetrics(Persistent::new(self.gc, metrics))
            }
            ObjectRef::MessageChannel(channel) => {
                ObjectRoot::MessageChannel(Persistent::new(self.gc, channel))
            }
            ObjectRef::SvgAnimatedString(animated) => {
                ObjectRoot::SvgAnimatedString(Persistent::new(self.gc, animated))
            }
            ObjectRef::SvgMatrix(matrix) => ObjectRoot::SvgMatrix(Persistent::new(self.gc, matrix)),
            ObjectRef::SvgPoint(point) => ObjectRoot::SvgPoint(Persistent::new(self.gc, point)),
            ObjectRef::SvgRect(rect) => ObjectRoot::SvgRect(Persistent::new(self.gc, rect)),
            ObjectRef::DomRect(rect) => ObjectRoot::DomRect(Persistent::new(self.gc, rect)),
            ObjectRef::DomRectList(list) => ObjectRoot::DomRectList(Persistent::new(self.gc, list)),
            ObjectRef::DomImplementation(implementation) => {
                ObjectRoot::DomImplementation(Persistent::new(self.gc, implementation))
            }
            ObjectRef::CssStyleSheet(sheet) => {
                ObjectRoot::CssStyleSheet(Persistent::new(self.gc, sheet))
            }
            ObjectRef::CssRule(rule) => ObjectRoot::CssRule(Persistent::new(self.gc, rule)),
            ObjectRef::CssRuleList(list) => ObjectRoot::CssRuleList(Persistent::new(self.gc, list)),
            ObjectRef::MediaList(list) => ObjectRoot::MediaList(Persistent::new(self.gc, list)),
            ObjectRef::KeyframeEffect(effect) => {
                ObjectRoot::KeyframeEffect(Persistent::new(self.gc, effect))
            }
            ObjectRef::DocumentTimeline(timeline) => {
                ObjectRoot::DocumentTimeline(Persistent::new(self.gc, timeline))
            }
            ObjectRef::StyleSheetList(list) => {
                ObjectRoot::StyleSheetList(Persistent::new(self.gc, list))
            }
            ObjectRef::CustomElementRegistry(registry) => {
                ObjectRoot::CustomElementRegistry(Persistent::new(self.gc, registry))
            }
        }
    }

    /// A root of `target`, kept past the operation.
    pub fn root_target(&self, target: TargetRef<'gc>) -> TargetRoot {
        match target {
            TargetRef::Node(node) => TargetRoot::Node(self.root(node)),
            TargetRef::Window(window) => TargetRoot::Window(Persistent::new(self.gc, window)),
            TargetRef::EventTarget(target) => {
                TargetRoot::EventTarget(Persistent::new(self.gc, target))
            }
        }
    }

    /// The window `target` belongs to (its relevant global, as the DOM
    /// knows it): a window itself, a node's document's; none for a plain
    /// target, or a node whose document has none.
    pub fn target_window(&self, target: TargetRef<'gc>) -> Option<Gc<'gc, Window>> {
        match target {
            TargetRef::Window(window) => Some(window),
            TargetRef::Node(node) => self.window_of(self.document_of(node)),
            // A message port's: its owner's; a `FileReader`'s and an abort
            // signal's: the window it was made in; a wake lock's sentinel's
            // and a `connection`'s: their navigator's.
            TargetRef::EventTarget(target) => target
                .window
                .get(self.gc)
                .or_else(|| self.message_port_owner(target))
                .or_else(|| self.abort_signal_window(target))
                .or_else(|| self.file_reader_window(target))
                .or_else(|| self.wake_lock_sentinel_window(target))
                .or_else(|| self.network_information_window(target)),
        }
    }
}

/// `flags` as `Event::flags`' bits.
/// Whether `event_type` is a wheel's (`wheel`, and Chrome's legacy
/// `mousewheel`): its listeners make the compositor wait.
fn is_wheel_type(event_type: NameId) -> bool {
    matches!(event_type, names::event::WHEEL | names::event::MOUSEWHEEL)
}

fn flag_bits(flags: EventFlags) -> u16 {
    [
        (flags.bubbles, BUBBLES),
        (flags.cancelable, CANCELABLE),
        (flags.composed, COMPOSED),
        (flags.trusted, TRUSTED),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .fold(0, |bits, (_, bit)| bits | bit)
}

fn set_bits(flags: &Cell<u16>, bit: u16, on: bool) {
    let bits = flags.get();
    flags.set(if on { bits | bit } else { bits & !bit });
}
