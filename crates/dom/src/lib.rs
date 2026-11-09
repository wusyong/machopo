//! The DOM on machopo's own GC (elise): the node tree, its names, attributes and text, and
//! the DOM's algorithms that the parser uses too (inserting, removing,
//! setting an attribute); the API that engines reach, with its arguments'
//! checks, is `dom_api`'s (Blink's `core/dom`, its bindings apart).
//!
//! A DOM (`Dom`) is a heap, one thread's; an operation of it (`Dom::mutate`)
//! is a binding's call, at whose start the heap may collect. Nodes are one
//! type (`Node`), its kind a field, as Blink's tree links are its `Node`'s:
//! walking the tree reads no type. Names are interned (`names`, the thread's
//! table), a node holding numbers; attribute values are interned too (the
//! thread's table, as Blink's `AtomicString`s), and they and text
//! are the DOM's strings (`DomString`, the thread's), outside the heap,
//! each in a `LocalExternal` that the heap's thread frees. Script holds
//! events and plain event targets too (`event`), each of a type of its own.
//!
//! No `unsafe`: the DOM is built on elise's safe API alone.

#![forbid(unsafe_code)]
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

use std::cell::{Cell, RefCell};
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Instant;

use elise::{
    Erased, External, Gc, GcObject, Heap, HeapOptions, HeapStats, HeapVector, JsEdge, JsEdgeSink,
    LocalExternal, Member, Mutation, Persistent, Platform, Site, WrapperMarker,
};

mod abort;
mod animation_objects;
mod animations;
mod attr;
mod base;
pub use attr::{DomParser, NamedNodeMap};
mod blob;
mod bloom;
mod canvas;
mod collection;
mod crypto;
mod cssom;
mod custom_element;
mod directionality;
mod editing;
mod event;
mod fetch;
mod file_reader;
mod flat;
mod form;
mod form_state;
mod frame;
mod geolocation;
mod headers;
mod history;
mod idle;
mod ids;
mod image;
mod initiator;
mod input;
mod intersection;
mod language;
mod lists;
mod media_query;
mod message;
mod mutation;
pub mod names;
mod navigator;
mod numbers;
mod performance;
mod range;
mod reactions;
mod resize;
mod script;
mod select;
mod selection;
mod shadow;
mod slot;
mod step_range;
mod string;
mod style;
mod style_sheet;
mod svg;
mod svg_use;
mod tasks;
mod text_control;
mod traversal;
mod tree;
mod ua_shadow;
mod ui_event;
mod url;
mod user_action;
mod wake_lock;
mod window;

pub use abort::{AbortAlgorithm, AbortController, AbortSignalData, DependentRef};
pub use animation_objects::{
    AnimationClass, AnimationObjectData, AnimationObjectRef, AnimationPlaybackEventData,
    DocumentTimeline, FreeEffect, KeyframeEffectObject, KeyframeEffectRef,
};
pub use animations::{
    AnimationEventData, AnimationEventFields, AnimationPlace, ElementAnimationsData,
    PendingAnimationEvent,
};
pub use blob::{Blob, BlobData, BlobRead, FileInfo};
pub use bloom::{attribute_bloom, class_bloom};
pub use canvas::{
    CanvasBlobCallback, CanvasEncode, CanvasGradient, CanvasGradientData, CanvasPattern,
    CanvasRenderingContext2d, CanvasStyleObject, ImageData, MAX_CACHED_FONTS, Path2d, TextMetrics,
};
pub use collection::HtmlCollection;
pub use collection::KeptCollection;
pub use collection::collection_steps_for_testing;
pub use crypto::Crypto;
pub use cssom::{
    CssRule, CssRuleList, MediaList, MediaOwner, RuleKind, RuleParent, Rules, StyleSheetList,
};
pub use custom_element::{
    Candidate, CandidateList, ConstructionTop, CustomElementDefinition, CustomElementRegistry,
    CustomElementState, DefinitionEdges, DefinitionInit, WhenDefined, has_hyphen,
    is_valid_custom_element_name,
};
pub use editing::ContentEditable;
pub use event::{
    CustomEventData, ErrorEventData, ErrorInfo, Event, EventFlags, EventState, EventTargetImpl,
    Listener, ListenerFlags, ObjectRef, ObjectRoot, PromiseRejectionEventData, ShadowEntry,
    ShadowPath, TargetRef, TargetRoot,
};
pub use fetch::{
    FetchHostRequest, FetchRecord, FetchRequest, FetchState, Referrer, Request, RequestCache,
    RequestCredentials, RequestData, RequestMode, RequestPriority, RequestRedirect, Response,
    ResponseData, ResponseType,
};
pub use file_reader::{
    FileReaderData, FileReaderResult, FileReaderState, ProgressEventData, ReadAs, ReadStep,
};
pub use flat::FlatChildren;
pub use form::{FormAssociation, FormItem, PastNames, RadioList, disabled_walks, form_walks};
pub use frame::{
    BlobUrlRequest, ElementChange, FrameNavigation, FrameOwnerChange, NavigationRequest,
};
pub use geolocation::{GeoRequest, Geolocation, GeolocationPositionError, GeolocationStep};
pub use headers::{Headers, HeadersGuard};
pub use history::{HashChangeEventData, History, PopStateEventData, SessionHistory};
pub use idle::{IdleCallback, IdleDeadline};
pub use ids::{WindowNamed, id_table_changes_for_testing, id_walks};
pub use image::{
    Image, ImageDecode, ImageEvent, ImageFetch, ImageForCanvas, ImageLoader, ImageRequestState,
    ImageState, ImageTaskAction,
};
pub use initiator::{LoadInitiator, StackAsker};
pub use input::{
    ClickState, InputKind, InputType, SpinState, ValueMode, input_type_changes_for_testing,
};
pub use intersection::{
    IntersectionEntryValues, IntersectionObservationState, IntersectionObserver,
    IntersectionObserverEntry, IntersectionObserverOptions, IntersectionObserverRoot,
};
pub use lists::{
    CssStyleDeclaration, DeclarationOwner, DomImplementation, DomRect, DomRectList, DomStringMap,
    DomTokenList, NodeList, StyleTarget,
};
pub use message::{
    MessageChannel, MessageEventData, MessageEventInit, MessagePort, PostedMessage,
    SerializedMessage, TakenMessage,
};
pub use mutation::{MutationKind, MutationObserver, MutationRecord, ObserveOptions};
pub use navigator::{Navigator, Scheduling, UserActivation};
pub use numbers::{number_to_string, parse_float};
pub use performance::{
    NavigationMilestone, NavigationTiming, NavigationType, PerformanceData, PerformanceEntry,
    PerformanceEntryType, PerformanceObserver, PerformanceObserverEntryList,
    PerformanceObserverMode, PerformanceTiming, PerformanceTimingKind,
};
pub use range::{BoundaryPoint, Range};
pub use reactions::{Callback, CallbackArgs, CeReactionsScope, Reaction, ReactionRef};
pub use resize::{
    BoxSize, ResizeObservationState, ResizeObserver, ResizeObserverBox, ResizeObserverEntry,
    ResizeObserverEntryValues, ResizeObserverSize,
};
pub use script::{
    LoadedScript, PendingScript, ReadyScript, ScriptCors, ScriptFetch, ScriptRequest, ScriptResult,
};
pub use select::{ListBox, ListBoxState, Output, PopupRequest};
pub use selection::Selection;
pub use shadow::{
    ShadowHostError, ShadowRootData, ShadowRootInit, ShadowRootMode, SlotAssignmentMode,
    retargets_for_testing,
};
pub use slot::SlotData;
pub use step_range::{RangeKey, StepError};
pub use style_sheet::{CssStyleSheet, SheetContents, SheetMedia};
pub use svg::{SvgAnimatedString, SvgMatrix, SvgPoint, SvgRect};
pub use svg_use::MAX_COPIES;
pub use tasks::{NextTask, TaskQueue};
pub use text_control::{SelectionDirection, SelectionMode, TextArea, TextSelection};
pub use traversal::{NodeIterator, Traversal, TraversalRoot, TreeWalker};
pub use tree::DOCUMENT_POSITION_FOLLOWING;
pub use ui_event::{
    FocusEventData, InputEventData, InputEventInfo, KeyboardEventData, KeyboardEventInfo,
    MouseEventData, MouseEventInfo, TextEventData, UiEventData, WheelEventData, WheelEventInfo,
    modifiers,
};
pub use url::{Url, UrlSearchParams};
pub use wake_lock::{WakeLock, WakeLockSentinelData};
pub use window::{
    AnimationFrame, ElementTask, ElementTaskKind, Location, NativeAction, NativeTimer, Timer,
    TimerState, Window,
};

pub use elise::LocalToken;
pub use gfx::{NodeId, NodeIdHashMap, NodeIdHashSet};
pub use names::{NameId, QualifiedName};
pub use string::{DomString, find_value, intern_value};
pub use style::{
    AttributeChanges, ClassList, ClassName, ElementData, ElementStyle, LayoutTreeFlags, StyleFlags,
    StyleNames, StyleNamesData,
};

/// A node of any kind (`NodeKind`), and its links in the tree.
/// The kind's data hangs off it, so that a node with none (an element
/// without attributes) has no drop glue to sweep.
#[derive(GcObject)]
pub struct Node {
    parent: Member<Node>,
    first_child: Member<Node>,
    /// Its previous sibling; the first child's is the last child, the
    /// children a ring that way (Blink's `previous_`, so that a node has
    /// no `last_child`): `Cx::previous_sibling` gives none for it, as the
    /// last child has no next sibling.
    previous_sibling: Member<Node>,
    next_sibling: Member<Node>,
    /// Its tree scope's root (Blink's `tree_scope_`): its node document,
    /// or in a shadow tree its shadow root (`shadow::IN_SHADOW_TREE` in
    /// `scope`), whose owner is the document; none for a document.
    owner: Member<Node>,
    /// Where it is among shadow trees (`shadow`'s bits: in one, a shadow
    /// root, a host), an element's custom element state
    /// (`custom_element`'s three), whether it has a `childNodes`
    /// (`lists::HAS_CHILD_LIST`), whether it is a textarea
    /// (`text_control::TEXTAREA_CHILDREN`): the byte is full. In the
    /// padding after `name`: the flag word is full.
    scope: Cell<u8>,
    /// The layout tree's flags past the flag word's
    /// (`LayoutTreeFlags::CONTROL_STATE_CHANGED`), in the padding too: a
    /// node is 54 bytes of fields in its 56.
    layout_more: Cell<u8>,
    /// The bits of its subtree's attribute names and classes (`bloom`,
    /// Blink's `attribute_or_class_bloom_`): room the ring of previous
    /// siblings left.
    bloom: Cell<u32>,
    /// Its kind (`NodeKind`'s, the low bits), what a kind keeps in a bit
    /// or two, and what changed for style and layout (Blink's
    /// `Node::node_flags_`): with its name one number, a node stays 56
    /// bytes, a 64-byte slot with its header.
    flags: Cell<u32>,
    /// An element's name; a processing instruction's target and a doctype's
    /// name as a local name; `QualifiedName::NONE` otherwise. One number,
    /// not three (Blink's `QualifiedName`): `QualifiedName::raw`.
    name: Cell<u32>,
    /// An element's attributes, made at the first.
    attributes: Member<HeapVector<Attribute>>,
    /// The kind's data, made when it has some (Blink's node subclasses'
    /// fields, and `NodeRareData`): a text's, a comment's or a processing
    /// instruction's `CharacterData`, a doctype's `DoctypeData`, an
    /// element's `ElementData` (a template's, a styled one's, one with
    /// listeners), a fragment's `NodeRareData`, a document's
    /// `DocumentData`.
    data: Member<Erased>,
    /// Its id in the heap's id table (`elise::Id::get`) once something off the
    /// heap asked for one (`Cx::node_id`: layout, paint); 0 until then
    /// (Blink's `DOMNodeIds`, which gives ids only to the nodes that need
    /// one).
    id: Cell<u32>,
    /// Its JS engine's wrapper (Blink's `ScriptWrappable::main_world_wrapper_`):
    /// the engine's own reference, which the DOM never reads
    /// (`Cx::wrapper_edge`). In a heap the engine drives, it makes the node
    /// old (`elise::JsEdge`).
    wrapper: JsEdge,
}

impl Node {
    #[inline]
    fn qualified_name(&self) -> QualifiedName {
        QualifiedName::from_raw(self.name.get())
    }
}

impl Attribute {
    #[inline]
    fn qualified_name(&self) -> QualifiedName {
        QualifiedName::from_raw(self.name.get())
    }
}

/// An attribute: its name and value (an interned string).
#[derive(GcObject)]
pub struct Attribute {
    /// `QualifiedName::raw`, as a node's.
    name: Cell<u32>,
    value: LocalExternal<RefCell<DomString>>,
}

/// A text's, a comment's or a processing instruction's data, and its
/// listeners.
#[derive(GcObject)]
pub struct CharacterData {
    data: LocalExternal<RefCell<DomString>>,
    /// Its listeners and observers', made at the first.
    pub(crate) rare: Member<NodeRareData>,
}

/// A doctype's public and system ids (its name is the node's `local`), and
/// its listeners.
#[derive(GcObject)]
pub struct DoctypeData {
    public_id: LocalExternal<DomString>,
    system_id: LocalExternal<DomString>,
    pub(crate) rare: Member<NodeRareData>,
}

/// A node's rarely used data (Blink's `NodeRareData`): its listeners and
/// the mutation observers registered on it. Made when it has one: a
/// fragment's is its data, another kind's is in its data (so that a
/// text's data stays as small).
#[derive(GcObject)]
pub struct NodeRareData {
    pub(crate) listeners: Member<HeapVector<Listener>>,
    pub(crate) observers: Member<HeapVector<mutation::Registration>>,
    /// A slottable's: its slot as its shadow root's assignment last worked
    /// it out, and the slot `assign()` gave it (`slot`).
    pub(crate) assigned_slot: Member<Node>,
    pub(crate) manual_slot: Member<Node>,
    /// Its `childNodes` and `children`, once script asked for them.
    pub(crate) child_nodes: Member<NodeList>,
    pub(crate) children: Member<HtmlCollection>,
    /// The other collections it is the root of that are the same each time
    /// (a table's `rows`, a document's `images`...), one of each kind.
    pub(crate) collections: Member<HeapVector<HtmlCollection>>,
    /// The collections by qualified name it is the root of that script
    /// still holds (`getElementsByTagName`'s, `getElementsByTagNameNS`'s),
    /// found again for the same name (`collection::NamedCollections`).
    pub(crate) named_collections: Member<collection::NamedCollections>,
    /// An element's CSS animations and transitions, once it has had one
    /// (`animations`).
    pub(crate) animations: Member<animations::ElementAnimationsData>,
}

/// A template's contents' data (Blink's `TemplateContentDocumentFragment`):
/// its template, which DOM's "host-including" walks go on to, and its rare
/// data (another fragment's is its data).
#[derive(GcObject)]
pub struct TemplateContentData {
    host: Member<Node>,
    pub(crate) rare: Member<NodeRareData>,
}

/// A document's: its inert document for its templates' contents (HTML's
/// "appropriate template contents owner document"), made at the first,
/// its listeners, its window, its URL and its readiness.
#[derive(GcObject)]
pub struct DocumentData {
    template_document: Member<Node>,
    pub(crate) rare: Member<NodeRareData>,
    window: Member<Window>,
    /// For a document with no window of its own (`createHTMLDocument`'s,
    /// `new Document()`'s, a template's contents' document): the window
    /// whose script made it, its relevant global (Blink's: it shares that
    /// window's `ExecutionContext`).
    relevant_window: Member<Window>,
    /// Empty for none.
    url: LocalExternal<RefCell<DomString>>,
    /// Its content type and character encoding's name as its host gave
    /// them; empty: the defaults (`document_content_type`).
    content_type: LocalExternal<RefCell<DomString>>,
    character_set: LocalExternal<RefCell<DomString>>,
    /// HTML's "currently executing script": the classic script element its
    /// host runs now (`set_current_script`).
    current_script: Member<Node>,
    /// `DocumentReadyState` as a number.
    ready_state: Cell<u8>,
    /// Its tree's elements by id (`ids`).
    ids: External<ids::IdTableCell>,
    /// The elements at the bottom of its `:hover` and `:active` chains
    /// (`user_action`).
    hover: Member<Node>,
    active: Member<Node>,
    /// Its focused element (`user_action`), none: its viewport.
    focused: Member<Node>,
    /// Its `implementation`, once script asked for it.
    pub(crate) implementation: Member<DomImplementation>,
    /// What style reads of its elements (`style::StyleNames`), none until
    /// it is first styled.
    pub(crate) style_names: Member<StyleNamesData>,
    /// Its adopted sheets (`adoptedStyleSheets`), once it has had some.
    pub(crate) adopted: Member<HeapVector<style_sheet::CssStyleSheet>>,
    /// Its shadow roots whose sheets may have changed (`style`), and the
    /// constructed sheets it made whose contents or `disabled` changed
    /// (`style_sheet`), since its style took them: its own (Blink's
    /// `StyleEngine::dirty_tree_scopes_`), as each frame's document is
    /// styled by itself, and a document no frame styles has them die
    /// with it.
    pub(crate) sheet_changed_roots: Member<HeapVector<Node>>,
    pub(crate) changed_sheets: Member<HeapVector<style_sheet::CssStyleSheet>>,
    /// Its `forms`, once script asked for them.
    pub(crate) forms: Member<HtmlCollection>,
    /// How many of its `<img>`s' requests and external scripts delay its
    /// load event (HTML's "delay the load event"; `image`, `script`).
    pub(crate) load_delay: Cell<u32>,
    /// Its `<img>`s that select again when its environment changes
    /// (`image`'s `responsive_images`), weakly.
    pub(crate) responsive_images: Member<HeapVector<image::ResponsiveImage>>,
    /// The intersection observer its lazy `<img>`s wait for (Blink's
    /// `LazyLoadImageObserver`), and the resize observer its lazy ones with
    /// `sizes="auto"` select again by (`Document::
    /// GetLazyLoadedAutoSizedImgObserver`): made the first time one is.
    pub(crate) lazy_load_observer: Member<IntersectionObserver>,
    pub(crate) auto_sizes_observer: Member<ResizeObserver>,
    /// The fonts its canvases were given, parsed (`canvas`'s
    /// `CanvasFontCache`); taken and put back, none until the first.
    pub(crate) canvas_fonts: External<Cell<Option<Box<canvas::CanvasFontCache>>>>,
    /// The `<iframe>` whose content document it is (`frame`).
    pub(crate) frame_owner: Member<Node>,
    /// Discarded: its browsing context went (`frame`).
    pub(crate) discarded: Cell<bool>,
    /// A top-level document's: the document of its frame tree's focused
    /// frame (`frame`; none: its own).
    pub(crate) focused_frame: Member<Node>,
    /// Its live ranges, those whose start is in it, and its node
    /// iterators, those whose root is, weakly (`range`, `traversal`).
    pub(crate) live_ranges: Member<HeapVector<range::LiveRangeRef>>,
    pub(crate) node_iterators: Member<HeapVector<traversal::NodeIteratorRef>>,
    /// Its `Selection`, once asked for (`selection`).
    pub(crate) selection: Member<selection::Selection>,
    /// Its list of scripts that will execute in order as soon as possible
    /// (their elements; `script`), once it has had one.
    pub(crate) in_order_scripts: Member<HeapVector<Node>>,
    /// Its `styleSheets`, once script asked for it, and its `<style>`s'
    /// contents by their text (`cssom`; taken and put back, none until
    /// the first).
    pub(crate) style_sheets: Member<cssom::StyleSheetList>,
    /// Its `MediaQueryList`s, weakly, in the order made; those with a
    /// listener, kept; the environment they were last evaluated in
    /// (`media_query`).
    pub(crate) media_query_lists: Member<HeapVector<media_query::MediaQueryListRef>>,
    pub(crate) kept_media_query_lists: Member<HeapVector<EventTargetImpl>>,
    pub(crate) media_environment: External<Cell<Option<css::media::MediaEnvironment>>>,
    pub(crate) sheet_texts: External<Cell<Option<Box<cssom::SheetTextCache>>>>,
    /// Its timeline's time (milliseconds, as of its last frame), the
    /// elements with animations (kept: one that left it has them canceled
    /// at the next frame), and the events its animations queued for the
    /// rendering update (`animations`).
    pub(crate) animation_time: Cell<Option<f64>>,
    pub(crate) animated: Member<HeapVector<Node>>,
    pub(crate) animation_events: External<RefCell<Vec<animations::PendingAnimationEvent>>>,
    /// Its animations' and effects' objects, weakly, and those kept while
    /// their animations can be reached; its `timeline`, once asked for; the
    /// animations off any element (no target, or style's that left theirs)
    /// script's objects still hold (`animation_objects`, `animations`).
    pub(crate) animation_objects: Member<HeapVector<animation_objects::AnimationObjectRef>>,
    pub(crate) kept_animation_objects: Member<HeapVector<EventTargetImpl>>,
    pub(crate) effect_objects: Member<HeapVector<animation_objects::KeyframeEffectRef>>,
    pub(crate) kept_effect_objects: Member<HeapVector<animation_objects::KeyframeEffectObject>>,
    pub(crate) timeline: Member<animation_objects::DocumentTimeline>,
    pub(crate) detached_animations: External<RefCell<Vec<animation::Animation>>>,
    /// Its target element (`:target`; `history`), which a fragment sets.
    pub(crate) target_element: Member<Node>,
    /// Its `load` event fired (`history`'s `is_completely_loaded`).
    pub(crate) completely_loaded: Cell<bool>,
    /// Its base URL as last worked out (`base`).
    pub(crate) base_url: LocalExternal<base::BaseUrlCell>,
    /// An `about:blank` document's about base URL (HTML's), its creator's
    /// base URL as it was made; a srcdoc document's, its container's as its
    /// iframe let it go (`base`).
    pub(crate) about_base_url: LocalExternal<RefCell<Option<::url::Url>>>,
    /// Its content language: its response's `Content-Language`, or its
    /// `<meta http-equiv="content-language">`'s, the last set (`language`).
    pub(crate) content_language: LocalExternal<RefCell<Option<Box<str>>>>,
    /// A top-level document's autofocus candidates (HTML's), by id (weak:
    /// one collected meanwhile was out of its document, and couldn't take
    /// the focus anyway), and its autofocus processed flag (`frame`).
    pub(crate) autofocus_candidates: External<Cell<Vec<NodeId>>>,
    pub(crate) autofocus_processed: Cell<bool>,
}

/// A document's readiness (HTML's "current document readiness",
/// `document.readyState`): a document the parser makes is loading until
/// it is parsed, then interactive, then complete once loaded; any other is
/// complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentReadyState {
    Loading,
    Interactive,
    Complete,
}

/// What a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Document,
    DocumentType,
    DocumentFragment,
    Element,
    Text,
    Comment,
    ProcessingInstruction,
    /// An attribute as a node (`attr`): in no tree.
    Attr,
}

impl NodeKind {
    fn from_raw(raw: u32) -> Self {
        match raw & KIND_MASK {
            0 => Self::Document,
            1 => Self::DocumentType,
            2 => Self::DocumentFragment,
            3 => Self::Element,
            4 => Self::Text,
            5 => Self::Comment,
            6 => Self::ProcessingInstruction,
            _ => Self::Attr,
        }
    }

    fn raw(self) -> u32 {
        match self {
            Self::Document => 0,
            Self::DocumentType => 1,
            Self::DocumentFragment => 2,
            Self::Element => 3,
            Self::Text => 4,
            Self::Comment => 5,
            Self::ProcessingInstruction => 6,
            Self::Attr => 7,
        }
    }

    /// Whether it is DOM's `CharacterData`: it has data.
    pub fn is_character_data(self) -> bool {
        matches!(
            self,
            Self::Text | Self::Comment | Self::ProcessingInstruction
        )
    }
}

/// `Node::flags`: the kind in the low bits; then a document's and an
/// element's few bits.
const KIND_MASK: u32 = 0b111;
/// A document's: it is an HTML document.
const HTML_DOCUMENT: u32 = 1 << 3;
/// A document's quirks mode (`QuirksMode`'s, 2 bits).
const QUIRKS_SHIFT: u32 = 4;
const QUIRKS_MASK: u32 = 0b11 << QUIRKS_SHIFT;
/// A document's: it is a document's inert one for its templates' contents.
const TEMPLATE_DOCUMENT: u32 = 1 << 6;
/// A text's: it is a `CDATASection` (Blink's, a `Text` of its own type:
/// DOM's CDATA section is a text node but for its type and name).
const CDATA_SECTION: u32 = 1 << 3;
/// A document's: its interface is `XMLDocument` (`createDocument`'s; DOM's
/// "XML document" with that interface, not `new Document()`'s).
const XML_DOCUMENT_INTERFACE: u32 = 1 << 7;
/// A script's (HTML's script element's state): the parser inserted it, it
/// already started (an option's selectedness and dirtiness take the bits
/// of `ALREADY_STARTED` and `INTEGRATION_POINT`: `select`). A listed element's `PARSER_INSERTED` is HTML's parser
/// inserted flag of a form-associated element: the parser gave it its
/// form owner, which its insertion keeps (`form`).
const PARSER_INSERTED: u32 = 1 << 7;
const ALREADY_STARTED: u32 = 1 << 8;
/// A MathML `annotation-xml` element the parser treats as an HTML
/// integration point (its `encoding`).
const INTEGRATION_POINT: u32 = 1 << 9;
/// An HTML script element's: HTML's "non-blocking" (true for one the
/// parser didn't make, until its `async` is set or added), in the bit a
/// MathML element's `INTEGRATION_POINT` takes.
const NON_BLOCKING: u32 = INTEGRATION_POINT;
/// Any node's: it has had a listener (its kind's data holds them), so a
/// dispatch looks at it; none read nothing more of a node.
const HAS_LISTENERS: u32 = 1 << 10;

/// A document's quirks mode (DOM's "mode").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuirksMode {
    NoQuirks,
    LimitedQuirks,
    Quirks,
}

/// A script element's flags (HTML's "parser document", "already
/// started" and "non-blocking"; a `<link>`'s `parser_inserted`: its
/// document's parser made it, its sheet blocks the parser's scripts). The
/// default is none set: a new script element script makes is
/// non-blocking (`Cx::create_element`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScriptFlags {
    pub parser_inserted: bool,
    pub already_started: bool,
    pub non_blocking: bool,
}

/// A doctype's name and ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doctype {
    pub name: DomString,
    pub public_id: DomString,
    pub system_id: DomString,
}

/// A name as an operation gives it: its namespace (none for the null
/// namespace), prefix (none for no prefix) and local name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualName<'a> {
    pub namespace: Option<&'a str>,
    pub prefix: Option<&'a str>,
    pub local: &'a str,
}

impl<'a> QualName<'a> {
    /// A name in no namespace, with no prefix.
    pub fn local(local: &'a str) -> Self {
        Self {
            namespace: None,
            prefix: None,
            local,
        }
    }

    /// An HTML element's name, with no prefix.
    pub fn html(local: &'a str) -> Self {
        Self {
            namespace: Some("http://www.w3.org/1999/xhtml"),
            prefix: None,
            local,
        }
    }
}

/// A name in no namespace, with no prefix: a doctype's, a processing
/// instruction's target.
fn local_name(local: NameId) -> QualifiedName {
    QualifiedName::new(NameIds {
        namespace: names::NONE,
        prefix: names::NONE,
        local,
    })
}

/// A name's numbers in its thread's names (`names::get`): a
/// `QualifiedName`'s, apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NameIds {
    pub namespace: NameId,
    pub prefix: NameId,
    pub local: NameId,
}

/// An attribute's name and value (`Cx::attribute_at`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeData {
    pub name: NameIds,
    pub value: DomString,
}

/// `Node::scope`'s bits under which a change of the node's children has
/// more to do (`Cx::children_changed`): one read on every insertion and
/// removal.
const CHILDREN_HOOKS: u8 =
    slot::SLOT_SCOPES | lists::HAS_CHILD_LIST | text_control::TEXTAREA_CHILDREN;

/// A DOM operation's failure: WebIDL's `DOMException` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomError {
    HierarchyRequest,
    NotFound,
    /// An `Attr` of another element's (`setAttributeNode`).
    InUseAttribute,
}

impl DomError {
    /// Its `DOMException` name.
    pub fn name(self) -> &'static str {
        match self {
            Self::HierarchyRequest => "HierarchyRequestError",
            Self::NotFound => "NotFoundError",
            Self::InUseAttribute => "InUseAttributeError",
        }
    }
}

impl fmt::Display for DomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl std::error::Error for DomError {}

/// A node in an operation: a raw pointer, as long as the operation.
pub type NodeRef<'gc> = Gc<'gc, Node>;

/// A string of the DOM's where it is (an attribute's value, a text's
/// data), for an operation: one pointer, read with no clone (an engine
/// that finds its own string for it cached counts no reference, nor moves
/// the string's three words about).
#[derive(Clone, Copy)]
pub struct StrRef<'gc>(&'gc RefCell<DomString>);

impl<'gc> StrRef<'gc> {
    /// The string, borrowed (a setter replaces it only in another
    /// operation's step, never while this is read).
    pub fn borrow(&self) -> std::cell::Ref<'gc, DomString> {
        self.0.borrow()
    }

    /// The string, kept: a reference counted.
    pub fn to_owned(&self) -> DomString {
        self.0.borrow().clone()
    }
}

/// A node kept past an operation: a root of the heap.
pub type NodeRoot = Persistent<Node>;

/// A DOM: its heap. Its names and attribute values are its thread's
/// (`names`): its nodes' name numbers mean names on that thread alone.
pub struct Dom {
    /// What its operations count and note of its changes: dropped before
    /// the heap (its mutation observers' state roots the pending ones).
    changes: Changes,
    heap: Heap,
    /// Not `Send`, whatever the heap is: it stays on the thread whose names
    /// its nodes hold.
    thread: PhantomData<*const ()>,
    /// When it was made: what its events' times count from.
    origin: Instant,
}

/// A DOM's changes as its operations see them, behind one pointer (each
/// change reads both): the count of its trees' changes
/// (`Cx::tree_version`), and its mutation observers' state.
#[derive(Default)]
struct Changes {
    version: Cell<u64>,
    /// The counts of the changes that clear collections' index caches
    /// (`collection`).
    collections: collection::CollectionVersions,
    /// The Unix time (milliseconds) at the DOM's `origin`: what a window's
    /// `performance.timeOrigin` counts from.
    unix_origin: Cell<f64>,
    mutations: mutation::MutationState,
    /// The line the parser sets attributes or inserts nodes at (0 when it
    /// isn't): where an event handler's content attribute's source is
    /// (Blink's parser's `TextPosition`), and that the loads its elements
    /// ask for are the parser's, not script's (`initiator`).
    parser_line: Cell<u32>,
    /// How many `wheel` and `mousewheel` listeners there are (those of
    /// targets collected since too: they are counted as they are removed,
    /// a target that dies has them still), and a count of their changes
    /// (Blink's `EventHandlerRegistry`, which tells its frame host): what
    /// the compositor's wheel event handler region is made from.
    wheel_listeners: Cell<u32>,
    wheel_listeners_version: Cell<u64>,
    /// Whether a document was ever discarded (`frame`): until then every
    /// target's relevant global is fully active, which a dispatch looks
    /// up before each listener only once one was.
    discarded_documents: Cell<bool>,
    /// The shadow roots whose slot assignment is out of date (`slot`).
    stale_slot_roots: Cell<Option<Persistent<HeapVector<Node>>>>,
    /// The constructed sheets made so far (their numbers; `style_sheet`).
    sheets_made: Cell<u64>,
    /// The `whenDefined` promises made so far (their numbers).
    promises_made: Cell<u64>,
    /// The custom element reactions stack and the backup element queue
    /// (`reactions`).
    reactions: reactions::ReactionStack,
    /// The microtasks the engine has yet to queue (`Microtasks`' bits),
    /// one byte read after each call.
    microtasks: Cell<u8>,
    /// The page's session history, as its host gives it (`history`).
    session_history: Cell<history::SessionHistory>,
    /// Whether a form was seen, and the elements `form` attributes name
    /// forms by (`form`).
    forms: form::FormState,
    /// The external scripts' fetches (`script`).
    scripts: script::ScriptFetches,
    /// The list of available images and the fetches it waits for
    /// (`image`).
    images: std::cell::RefCell<image::ImageCache>,
    /// The `<img>`s whose image data's microtask is queued (`image`).
    image_updates: Cell<Option<Persistent<HeapVector<Node>>>>,
    /// Intersection observers made, targets observed or unobserved
    /// (`intersection`'s `intersection_version`).
    intersection_version: Cell<u64>,
    /// Targets observed by resize observers (`resize`'s `resize_version`).
    resize_version: Cell<u64>,
    /// The canvases that drew since the renderer last drew them, and how
    /// many times any did (`canvas`).
    canvas_draws: std::cell::RefCell<Vec<NodeId>>,
    canvas_draws_version: Cell<u64>,
    /// The `toBlob` calls made so far (their numbers), those waiting for
    /// their files, and what they are to encode, which the host takes
    /// (`canvas`).
    canvas_blobs_made: Cell<u64>,
    canvas_blobs: Cell<Option<Persistent<HeapVector<canvas::CanvasBlobCallback>>>>,
    canvas_encodes: Cell<Vec<canvas::CanvasEncode>>,
    /// The `<use>`s and what each copied (`svg_use`).
    svg_uses: svg_use::UseState,
    /// What happened to the `<iframe>`s (`frame`).
    frames: frame::FrameState,
    /// The animations' count and the subtrees removed since they were
    /// styled (`animations`).
    animations: animations::AnimationState,
    /// Who asked for loads, for DevTools (`initiator`).
    stacks: initiator::RequestStacks,
    /// The console warnings script's calls earned, until the host takes
    /// them (`Dom::take_console_warnings`).
    console_warnings: Cell<Vec<String>>,
    /// The next task's number, whether a task that is due now may be
    /// queued, and the first number a pass of the host's holds back
    /// (`tasks`).
    task_sequence: Cell<u64>,
    tasks_queued: Cell<bool>,
    task_limit: Cell<Option<u64>>,
    /// The documents whose window's `load` task ran, until the host takes
    /// them (`Dom::take_loaded_documents`).
    loaded_documents: Cell<Vec<NodeId>>,
    /// The browser's user agent and whether automation controls it, its
    /// host's (`Dom::set_agent`, `navigator`).
    user_agent: std::cell::RefCell<String>,
    webdriver: Cell<bool>,
}

/// The microtasks an operation asked the engine to queue
/// (`Dom::take_microtasks`): HTML's mutation observer microtask, the
/// backup element queue's, and the `<img>`s' image data's
/// (`Cx::take_image_updates`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Microtasks(u8);

impl Microtasks {
    pub(crate) const MUTATIONS: u8 = 1 << 0;
    pub(crate) const REACTIONS: u8 = 1 << 1;
    pub(crate) const IMAGES: u8 = 1 << 2;
    /// Not a microtask: an `<iframe>` came, went, or changed its `src` or
    /// `srcdoc`, or a script element is to be prepared, and the engine has
    /// steps to run before the call returns (`Cx::take_element_changes`).
    pub(crate) const ELEMENTS: u8 = 1 << 3;
    /// Not a microtask either: an animation's playback settled or replaced
    /// its promises (`Cx::take_objects_with_animation_notices`), which the
    /// engine settles before the call returns.
    pub(crate) const ANIMATIONS: u8 = 1 << 4;
    /// Not a microtask either: script asked for loads whose stacks the
    /// host wants (`Cx::asked_for_load`), which the engine captures before
    /// the call returns (`Dom::note_request_stacks`).
    pub(crate) const STACKS: u8 = 1 << 5;
    /// Not a microtask either: `fetch()`es and their aborts asked for, which
    /// the engine hands its host before the call returns (Blink's
    /// `FetchManager` starts its loader in the call;
    /// `Cx::take_fetch_requests`).
    pub(crate) const FETCHES: u8 = 1 << 6;

    #[inline]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn mutations(self) -> bool {
        self.0 & Self::MUTATIONS != 0
    }

    pub fn reactions(self) -> bool {
        self.0 & Self::REACTIONS != 0
    }

    pub fn images(self) -> bool {
        self.0 & Self::IMAGES != 0
    }

    pub fn elements(self) -> bool {
        self.0 & Self::ELEMENTS != 0
    }

    pub fn animations(self) -> bool {
        self.0 & Self::ANIMATIONS != 0
    }

    pub fn stacks(self) -> bool {
        self.0 & Self::STACKS != 0
    }

    pub fn fetches(self) -> bool {
        self.0 & Self::FETCHES != 0
    }
}

impl Dom {
    /// A DOM whose GC works on elise's pool of workers.
    pub fn new() -> Self {
        Self::with_options(HeapOptions::default())
    }

    pub fn with_options(options: HeapOptions) -> Self {
        Self::from_heap(Heap::new(options))
    }

    /// A DOM whose GC works on `platform`.
    pub fn with_platform(options: HeapOptions, platform: Arc<dyn Platform>) -> Self {
        Self::from_heap(Heap::with_platform(options, platform))
    }

    fn from_heap(heap: Heap) -> Self {
        let changes = Changes::default();
        changes.unix_origin.set(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0.0, |since| since.as_secs_f64() * 1000.0),
        );
        Self {
            changes,
            heap,
            thread: PhantomData,
            origin: Instant::now(),
        }
    }

    /// Runs an operation: a binding's call. The heap may collect first.
    pub fn mutate<R>(&mut self, f: impl for<'gc> FnOnce(&Cx<'_, 'gc>) -> R) -> R {
        let (origin, changes) = (self.origin, &self.changes);
        self.heap.mutate(|gc| {
            f(&Cx {
                gc,
                origin,
                changes,
            })
        })
    }

    /// The microtasks operations asked for since the last time, taken
    /// (the engine queues them now): the mutation observer microtask once
    /// an operation queued the first record since the last delivery; the
    /// backup element queue's once an element went in it with its
    /// processing not queued. One read: most calls find none, and write
    /// nothing.
    /// The warnings for the page's console that script's calls earned
    /// since the last ask, in order (`Cx::warn_console`): the host logs
    /// them.
    pub fn take_console_warnings(&self) -> Vec<String> {
        self.changes.console_warnings.take()
    }

    #[inline]
    pub fn take_microtasks(&self) -> Microtasks {
        let cell = &self.changes.microtasks;
        let bits = cell.get();
        if bits != 0 {
            cell.set(0);
        }
        Microtasks(bits)
    }

    /// Whether the mutation observer microtask is to be queued: taken
    /// (the backup element queue's left for `take_microtasks`).
    pub fn take_mutation_microtask(&self) -> bool {
        self.take_one(Microtasks::MUTATIONS)
    }

    /// Whether the backup element queue's microtask is to be queued:
    /// taken.
    pub fn take_reactions_microtask(&self) -> bool {
        self.take_one(Microtasks::REACTIONS)
    }

    fn take_one(&self, bit: u8) -> bool {
        let cell = &self.changes.microtasks;
        let bits = cell.get();
        cell.set(bits & !bit);
        bits & bit != 0
    }

    /// One operation that allocates nothing (a binding's getter): no
    /// safepoint first, so no GC's work (`elise::Heap::read`). Shared: such
    /// operations may overlap, not one of `mutate` or a GC.
    pub fn read<R>(&self, f: impl for<'gc> FnOnce(&Cx<'_, 'gc>) -> R) -> R {
        let (origin, changes) = (self.origin, &self.changes);
        self.heap.read(|gc| {
            f(&Cx {
                gc,
                origin,
                changes,
            })
        })
    }

    /// Its heap, lent to an engine whose GC objects live in it beside the
    /// nodes (wasmtime's, through a `GcHeap` that borrows it): their
    /// allocations and writes through bits (`elise::raw`), the frames that
    /// hold them (`enter_frames`), the roots its stores hold between calls
    /// (`set_roots_source`), and the GCs given its stacks' roots
    /// (`safepoint_with_roots`). Blink's wasm GC objects are V8's, beside
    /// cppgc's heap of the DOM's; here the DOM's heap holds both. Lent, not
    /// to be replaced: its nodes are the DOM's.
    pub fn heap_mut(&mut self) -> &mut Heap {
        &mut self.heap
    }

    /// A full GC, at this safepoint.
    ///
    /// # Panics
    ///
    /// In a heap a JS engine drives (`HeapOptions::driven`): its full GCs
    /// are the engine's.
    pub fn collect(&mut self) {
        self.heap.collect();
    }

    pub fn stats(&self) -> HeapStats {
        self.heap.stats()
    }

    /// Whether its heap is due to run a GC (`elise::Heap::should_collect`):
    /// a scavenge, a full GC, a marking's start or end.
    pub fn should_collect(&self) -> bool {
        self.heap.should_collect()
    }

    /// One operation over every object of its heap, for a heap snapshot
    /// (`elise::Heap::walk`: the last GC's sweep finished first, no GC's
    /// work started).
    pub fn walk<R>(&mut self, f: impl for<'gc> FnOnce(&Cx<'_, 'gc>, &elise::Walk<'gc>) -> R) -> R {
        let (origin, changes) = (self.origin, &self.changes);
        self.heap.walk(|gc, walk| {
            f(
                &Cx {
                    gc,
                    origin,
                    changes,
                },
                walk,
            )
        })
    }

    /// The bytes its objects use: what a JS engine that drives the heap is
    /// told as it grows (`elise::Heap::used_bytes`).
    pub fn used_bytes(&self) -> usize {
        self.heap.used_bytes()
    }

    /// `used_bytes` with the rest of the allocation buffers it counts as it
    /// takes them, read at once (`elise::Heap::counted_bytes`): for how far it
    /// has moved since it was told.
    pub fn counted_bytes(&self) -> usize {
        self.heap.counted_bytes()
    }

    /// The JS engine's start of the heap's marking, between operations
    /// (`elise::Heap::start_marking`).
    pub fn start_marking(&mut self, js: Arc<dyn JsEdgeSink>) -> WrapperMarker {
        self.heap.start_marking(js)
    }

    /// A step of the marking the engine drives (`elise::Heap::advance_marking`):
    /// whether nothing is left.
    pub fn advance_marking(&mut self, atomic: bool, budget: std::time::Duration) -> bool {
        self.heap.advance_marking(atomic, budget)
    }

    /// The engine's end of the marking, in its pause
    /// (`elise::Heap::finish_marking`): the bytes the heap uses after it, its
    /// live ones, for the engine's next limit.
    pub fn finish_marking(&mut self) -> usize {
        self.heap.finish_marking()
    }

    /// How many attribute values the thread's table of interned ones holds
    /// (some may be held by no attribute until it next drops them).
    pub fn interned_values(&self) -> usize {
        string::interned_values()
    }
}

impl Default for Dom {
    fn default() -> Self {
        Self::new()
    }
}

/// An operation of the DOM: what makes, reads and changes nodes.
pub struct Cx<'a, 'gc> {
    /// The heap's operation: for roots and ids.
    pub gc: &'a Mutation<'gc>,
    origin: Instant,
    changes: &'a Changes,
}

/// Where the DOM allocates (V8's `AllocationSite`): the heap pretenures a
/// site whose objects mostly survive their first scavenge (a tree built and
/// kept), not one whose die young (nodes made and dropped).
/// `data`'s length in UTF-16 code units.
fn utf16_len(data: &str) -> usize {
    // At most its length in bytes: no overflow.
    data.chars()
        .fold(0usize, |len, c| len.saturating_add(c.len_utf16()))
}

/// A length or an offset in UTF-16 code units as the DOM's `unsigned
/// long`: saturated (data past 4 GiB units is past what a range counts).
fn utf16_u32(units: usize) -> u32 {
    u32::try_from(units).unwrap_or(u32::MAX)
}

mod sites {
    use elise::Site;

    pub(crate) static DOCUMENT: Site = Site::new();
    pub(crate) static ELEMENT: Site = Site::new();
    pub(crate) static TEXT: Site = Site::new();
    pub(crate) static TEXT_DATA: Site = Site::new();
    pub(crate) static ATTRIBUTE: Site = Site::new();
    /// What the parser makes besides elements and texts, and their data.
    pub(crate) static OTHER: Site = Site::new();
}

impl<'gc> Cx<'_, 'gc> {
    /// The microtask `bit` (`Microtasks`') for the engine to queue.
    #[inline]
    pub(crate) fn microtask_to_queue(&self, bit: u8) {
        let cell = &self.changes.microtasks;
        cell.set(cell.get() | bit);
    }

    /// A warning for the page's console about what script asked (Blink's
    /// `ExecutionContext::AddConsoleMessage`, a JavaScript one at the
    /// warning level), for the host to log (`Dom::take_console_warnings`).
    pub fn warn_console(&self, text: String) {
        let warnings = &self.changes.console_warnings;
        let mut taken = warnings.take();
        taken.push(text);
        warnings.set(taken);
    }

    fn node(&self, kind: NodeKind, owner: Option<NodeRef<'gc>>, site: &Site) -> NodeRef<'gc> {
        self.gc.alloc_in(
            site,
            NodeInit {
                parent: None,
                first_child: None,
                previous_sibling: None,
                next_sibling: None,
                owner,
                // Never styled.
                flags: Cell::new(kind.raw() | style::NEW),
                scope: Cell::new(0),
                bloom: Cell::new(0),
                layout_more: Cell::new(0),
                name: Cell::new(QualifiedName::NONE.raw()),
                attributes: None,
                data: None,
                id: Cell::new(0),
                wrapper: (),
            },
        )
    }

    fn flag(&self, node: NodeRef<'gc>, flag: u32) -> bool {
        node.flags.get() & flag != 0
    }

    fn set_flag(&self, node: NodeRef<'gc>, flag: u32, on: bool) {
        let flags = node.flags.get();
        node.flags
            .set(if on { flags | flag } else { flags & !flag });
    }

    /// A new document: an HTML document or an XML one (`html`), which
    /// decides how `nodeName` cases an HTML element's name; no quirks.
    pub fn create_document(&self, html: bool) -> NodeRef<'gc> {
        let document = self.node(NodeKind::Document, None, &sites::DOCUMENT);
        self.set_flag(document, HTML_DOCUMENT, html);
        self.set_flag(document, style::CONNECTED, true);
        document
    }

    /// `document`'s inert document for its templates' contents (HTML's
    /// "appropriate template contents owner document"): made at the first,
    /// as HTML as `document`; itself for such a document.
    pub fn template_document(&self, document: NodeRef<'gc>) -> NodeRef<'gc> {
        let document = self.document_of(document);
        if self.flag(document, TEMPLATE_DOCUMENT) {
            return document;
        }
        if let Some(data) = self.document_data(document)
            && let Some(inert) = data.template_document.get(self.gc)
        {
            return inert;
        }
        let inert = self.create_document(self.is_html_document(document));
        self.set_flag(inert, TEMPLATE_DOCUMENT, true);
        self.set_document_relevant_window(inert, self.document_relevant_window(document));
        let data = match self.document_data(document) {
            Some(data) => data,
            None => {
                let data =
                    self.gc
                        .alloc_beside(document, &sites::OTHER, window::new_document_data());
                document.data.set(self.gc, Some(data.erase()));
                data
            }
        };
        data.template_document.set(self.gc, Some(inert));
        inert
    }

    fn document_data(&self, document: NodeRef<'gc>) -> Option<Gc<'gc, DocumentData>> {
        document.data.get(self.gc)?.downcast()
    }

    /// A document's elements by id (`ids`).
    pub(crate) fn document_ids(&self, document: NodeRef<'gc>) -> Option<&'gc ids::IdTableCell> {
        Some(&*self.document_data(document)?.into_ref().ids)
    }

    /// A document's quirks mode.
    pub fn quirks_mode(&self, document: NodeRef<'gc>) -> QuirksMode {
        match (document.flags.get() & QUIRKS_MASK) >> QUIRKS_SHIFT {
            0 => QuirksMode::NoQuirks,
            1 => QuirksMode::LimitedQuirks,
            _ => QuirksMode::Quirks,
        }
    }

    pub fn set_quirks_mode(&self, document: NodeRef<'gc>, mode: QuirksMode) {
        let bits = match mode {
            QuirksMode::NoQuirks => 0,
            QuirksMode::LimitedQuirks => 1,
            QuirksMode::Quirks => 2,
        };
        document
            .flags
            .set((document.flags.get() & !QUIRKS_MASK) | (bits << QUIRKS_SHIFT));
        // What a class matches changes with it: the caches of what matched.
        self.tree_changed();
        self.collections_classes_changed();
    }

    /// `node` if it is a document, else its node document.
    pub fn document_of(&self, node: NodeRef<'gc>) -> NodeRef<'gc> {
        match node.owner.get(self.gc) {
            // In a shadow tree: the shadow root's document.
            Some(root) if node.scope.get() & shadow::IN_SHADOW_TREE != 0 => {
                root.owner.get(self.gc).expect("a shadow root's document")
            }
            Some(document) => document,
            None => node,
        }
    }

    /// A new element named `name` in `document`'s node document (DOM's
    /// "create an element", its name already checked). An HTML `<template>`
    /// gets its contents, a fragment of `document`'s template document.
    /// An HTML element whose name is a valid custom element name is
    /// undefined (HTML's "create an element" with no definition: one with
    /// a definition is the caller's to construct or upgrade).
    ///
    /// Small and inlined, the rest out of line: inlined into a caller
    /// naming the HTML namespace, its name's hash is the compiler's
    /// (`names::intern` of a constant), 100 instructions a `createElement`
    /// not run.
    #[inline]
    pub fn create_element(&self, document: NodeRef<'gc>, name: QualName<'_>) -> NodeRef<'gc> {
        let ids = self.intern_name(name);
        let element = self.create_element_named(document, ids);
        if ids.namespace == names::HTML_NAMESPACE && custom_element::has_hyphen(name.local) {
            self.undefined_if_custom_name(element, name.local);
        }
        element
    }

    #[cold]
    #[inline(never)]
    fn undefined_if_custom_name(&self, element: NodeRef<'gc>, local: &str) {
        if custom_element::is_valid_custom_element_name(local) {
            self.set_custom_element_state(element, CustomElementState::Undefined);
        }
    }

    /// `create_element` as the HTML parser makes an element (HTML's
    /// "create an element for a token"): a script element is the parser's
    /// to flag (`set_script_flags`), not one that may run as it is
    /// inserted.
    pub fn create_element_by_parser(
        &self,
        document: NodeRef<'gc>,
        name: QualName<'_>,
    ) -> NodeRef<'gc> {
        let ids = self.intern_name(name);
        let element = self.make_element(document, ids);
        if ids.namespace == names::HTML_NAMESPACE && custom_element::has_hyphen(name.local) {
            self.undefined_if_custom_name(element, name.local);
        }
        element
    }

    /// An HTML element named `local` with its attributes, as the parser's
    /// fast path makes one (Blink's `HTMLFastPathParser`, then
    /// `Element::ParserSetAttributes`): no name interned from its string,
    /// none of the attributes looked for first (the caller gives each name
    /// once), their list made its size, and an input made of its type
    /// (Blink's `HTMLInputElement::InitializeTypeInParsing`), not a text
    /// input changed to it. Each attribute is still appended as DOM's
    /// "append an attribute" appends it, its change steps run.
    ///
    /// Not for a name that may be a custom element's or a script's: those
    /// are `create_element_by_parser`'s.
    pub fn create_html_element_with_attributes(
        &self,
        document: NodeRef<'gc>,
        local: NameId,
        attributes: &[(NameId, &str)],
    ) -> NodeRef<'gc> {
        debug_assert!(
            local != names::local::SCRIPT && !custom_element::has_hyphen(&names::get(local)),
            "a name the fast path knows"
        );
        let element = self.make_element(
            document,
            NameIds {
                namespace: names::HTML_NAMESPACE,
                prefix: names::NONE,
                local,
            },
        );
        if attributes.is_empty() {
            return element;
        }
        if local == names::local::INPUT
            && let Some(&(_, kind)) = attributes
                .iter()
                .find(|&&(name, _)| name == names::local::TYPE)
        {
            self.input_made_of_type(element, kind);
        }
        let list = HeapVector::new_beside(self.gc, element, attributes.len());
        element.attributes.set(self.gc, Some(list));
        for &(local, value) in attributes {
            let name = QualifiedName::new(NameIds {
                namespace: names::NONE,
                prefix: names::NONE,
                local,
            });
            let attribute = self.gc.alloc_beside(
                element,
                &sites::ATTRIBUTE,
                AttributeInit {
                    name: Cell::new(name.raw()),
                    value: LocalExternal::new(RefCell::new(string::intern_value(value))),
                },
            );
            list.push(self.gc, attribute);
            self.attribute_added(element, name);
            self.attribute_changed(element, name, None, Some(value));
        }
        element
    }

    /// `create_element` of a name of this DOM's. A script element is
    /// non-blocking, and may run as it is inserted.
    #[inline]
    pub fn create_element_named(&self, document: NodeRef<'gc>, name: NameIds) -> NodeRef<'gc> {
        let element = self.make_element(document, name);
        if name.namespace == names::HTML_NAMESPACE && name.local == names::local::SCRIPT {
            self.script_made(element);
        }
        element
    }

    #[inline]
    fn make_element(&self, document: NodeRef<'gc>, name: NameIds) -> NodeRef<'gc> {
        let document = self.document_of(document);
        let element = self.node(NodeKind::Element, Some(document), &sites::ELEMENT);
        element.name.set(QualifiedName::new(name).raw());
        if name.namespace == names::HTML_NAMESPACE && name.local == names::TEMPLATE {
            let content = self.create_document_fragment(self.template_document(document));
            let data = self.gc.alloc_beside(
                content,
                &sites::OTHER,
                TemplateContentDataInit {
                    host: Some(element),
                    rare: None,
                },
            );
            content.data.set(self.gc, Some(data.erase()));
            self.element_data(element)
                .template_content
                .set(self.gc, Some(content));
        }
        // From the first form (fieldset) on, subtrees inserted and removed
        // are looked at for form owners (disabled controls): a page without
        // one, nearly all of Speedometer's, looks at nothing more. An input
        // has its type from the start, a textarea its state.
        if name.namespace == names::HTML_NAMESPACE {
            // `CONTROL_ATTRIBUTE` as an element's name's: a `<picture>`
            // (and a `<form>`, a `<label>`, whose names are attributes'
            // too: looked at, nothing done).
            let kind = names::form_kind(name.local)
                & (form::FORM
                    | form::FIELDSET
                    | form::INPUT
                    | form::TEXTAREA
                    | form::DISABLABLE
                    | form::LISTED
                    | form::CONTROL_ATTRIBUTE);
            if kind != 0 {
                self.form_element_made(element, kind);
            }
        } else if name.namespace == names::SVG_NAMESPACE && name.local == names::local::USE {
            self.svg_use_made();
        }
        if name.namespace == names::HTML_NAMESPACE && name.local == names::local::IFRAME {
            self.iframe_made();
        }
        if name.namespace == names::HTML_NAMESPACE && name.local == names::local::BASE {
            self.base_made();
        }
        element
    }

    /// `template`'s contents made `contents` (HTML's, as the parser
    /// attaches a declarative shadow root: its template's contents are the
    /// shadow root, the template never inserted).
    pub fn set_template_content(&self, template: NodeRef<'gc>, contents: NodeRef<'gc>) {
        self.element_data(template)
            .template_content
            .set(self.gc, Some(contents));
    }

    /// The template whose contents `fragment` is; none for another node
    /// (a declarative shadow root, its template's contents as the parser
    /// builds it, is a shadow root: its host is its own).
    pub fn template_content_host(&self, fragment: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        fragment
            .data
            .get(self.gc)?
            .downcast::<TemplateContentData>()?
            .host
            .get(self.gc)
    }

    /// A template's contents; none for another node.
    pub fn template_content(&self, element: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        element
            .data
            .get(self.gc)?
            .downcast::<ElementData>()?
            .template_content
            .get(self.gc)
    }

    fn character_data(
        &self,
        kind: NodeKind,
        document: NodeRef<'gc>,
        data: &str,
        site: &Site,
    ) -> NodeRef<'gc> {
        let node = self.node(kind, Some(self.document_of(document)), site);
        let data = self.gc.alloc_beside(
            node,
            &sites::TEXT_DATA,
            CharacterDataInit {
                data: LocalExternal::new(RefCell::new(DomString::new(data))),
                rare: None,
            },
        );
        node.data.set(self.gc, Some(data.erase()));
        node
    }

    /// A new text of `data` in `document`'s node document.
    pub fn create_text(&self, document: NodeRef<'gc>, data: &str) -> NodeRef<'gc> {
        self.character_data(NodeKind::Text, document, data, &sites::TEXT)
    }

    /// A new CDATA section of `data` in `document`'s node document
    /// (`createCDATASection`).
    pub fn create_cdata_section(&self, document: NodeRef<'gc>, data: &str) -> NodeRef<'gc> {
        let node = self.create_text(document, data);
        node.flags.set(node.flags.get() | CDATA_SECTION);
        node
    }

    /// Whether `node` is a CDATA section (a text of that type).
    pub fn is_cdata_section(&self, node: NodeRef<'gc>) -> bool {
        self.kind(node) == NodeKind::Text && self.flag(node, CDATA_SECTION)
    }

    /// Whether `node` is one of DOM's "exclusive Text nodes": a text that
    /// isn't a CDATA section (`normalize()`'s).
    pub fn is_exclusive_text(&self, node: NodeRef<'gc>) -> bool {
        self.kind(node) == NodeKind::Text && !self.flag(node, CDATA_SECTION)
    }

    /// Whether `document`'s interface is `XMLDocument`.
    pub fn is_xml_document_interface(&self, document: NodeRef<'gc>) -> bool {
        self.kind(document) == NodeKind::Document && self.flag(document, XML_DOCUMENT_INTERFACE)
    }

    /// `document`'s interface `XMLDocument` (`createDocument`'s).
    pub fn set_xml_document_interface(&self, document: NodeRef<'gc>) {
        document
            .flags
            .set(document.flags.get() | XML_DOCUMENT_INTERFACE);
    }

    /// A new comment of `data` in `document`'s node document.
    pub fn create_comment(&self, document: NodeRef<'gc>, data: &str) -> NodeRef<'gc> {
        self.character_data(NodeKind::Comment, document, data, &sites::OTHER)
    }

    /// A new processing instruction in `document`'s node document (its
    /// target already checked).
    pub fn create_processing_instruction(
        &self,
        document: NodeRef<'gc>,
        target: &str,
        data: &str,
    ) -> NodeRef<'gc> {
        let node = self.character_data(
            NodeKind::ProcessingInstruction,
            document,
            data,
            &sites::OTHER,
        );
        node.name.set(local_name(names::intern(target)).raw());
        node
    }

    /// A new doctype in `document`'s node document.
    pub fn create_doctype(
        &self,
        document: NodeRef<'gc>,
        name: &str,
        public_id: &str,
        system_id: &str,
    ) -> NodeRef<'gc> {
        let node = self.node(
            NodeKind::DocumentType,
            Some(self.document_of(document)),
            &sites::OTHER,
        );
        node.name.set(local_name(names::intern(name)).raw());
        let data = self.gc.alloc_beside(
            node,
            &sites::OTHER,
            DoctypeDataInit {
                public_id: LocalExternal::new(DomString::new(public_id)),
                system_id: LocalExternal::new(DomString::new(system_id)),
                rare: None,
            },
        );
        node.data.set(self.gc, Some(data.erase()));
        node
    }

    /// A new document fragment in `document`'s node document.
    pub fn create_document_fragment(&self, document: NodeRef<'gc>) -> NodeRef<'gc> {
        self.node(
            NodeKind::DocumentFragment,
            Some(self.document_of(document)),
            &sites::OTHER,
        )
    }

    /// A doctype's name and ids; none for another node.
    pub fn doctype(&self, node: NodeRef<'gc>) -> Option<Doctype> {
        let data = node.data.get(self.gc)?.downcast::<DoctypeData>()?;
        let data = data.into_ref();
        Some(Doctype {
            name: names::string(node.qualified_name().local()),
            public_id: data.public_id.get().clone(),
            system_id: data.system_id.get().clone(),
        })
    }

    /// A processing instruction's target; none for another node.
    pub fn target(&self, node: NodeRef<'gc>) -> Option<DomString> {
        (self.kind(node) == NodeKind::ProcessingInstruction)
            .then(|| names::string(node.qualified_name().local()))
    }

    /// A script element's flags (none set for another element; a
    /// `<link>`'s `parser_inserted`).
    pub fn script_flags(&self, element: NodeRef<'gc>) -> ScriptFlags {
        let script = self.is_html_script(element);
        ScriptFlags {
            parser_inserted: self.flag(element, PARSER_INSERTED),
            already_started: script && self.flag(element, ALREADY_STARTED),
            non_blocking: script && self.flag(element, NON_BLOCKING),
        }
    }

    /// `element`'s script flags set: a script element left neither
    /// parser-inserted nor already started may run as it is inserted
    /// (`script`).
    pub fn set_script_flags(&self, element: NodeRef<'gc>, flags: ScriptFlags) {
        self.set_flag(element, PARSER_INSERTED, flags.parser_inserted);
        if !self.is_html_script(element) {
            return;
        }
        self.set_flag(element, ALREADY_STARTED, flags.already_started);
        self.set_flag(element, NON_BLOCKING, flags.non_blocking);
        if !flags.parser_inserted && !flags.already_started {
            self.see(form::SCRIPTS_SEEN);
        }
    }

    /// Whether the parser made `element` a MathML `annotation-xml` HTML
    /// integration point.
    pub fn is_integration_point(&self, element: NodeRef<'gc>) -> bool {
        self.flag(element, INTEGRATION_POINT)
    }

    pub fn set_integration_point(&self, element: NodeRef<'gc>) {
        self.set_flag(element, INTEGRATION_POINT, true);
    }

    /// `name`'s numbers. An empty namespace or prefix is none: the empty
    /// string is `names::NONE`.
    fn intern_name(&self, name: QualName<'_>) -> NameIds {
        NameIds {
            namespace: names::intern(name.namespace.unwrap_or("")),
            prefix: names::intern(name.prefix.unwrap_or("")),
            local: names::intern(name.local),
        }
    }

    pub fn kind(&self, node: NodeRef<'gc>) -> NodeKind {
        NodeKind::from_raw(node.flags.get())
    }

    /// `node`'s reference to its JS engine's wrapper, which only the engine
    /// reads and writes (`Node::wrapper`).
    pub fn wrapper_edge(&self, node: NodeRef<'gc>) -> &'gc JsEdge {
        &node.into_ref().wrapper
    }

    /// In a DOM no JS engine drives (a wasm document's), the same 8 bytes
    /// are its engine's word (`elise::JsEdge::embedder_word`): 0 until set.
    ///
    /// # Panics
    ///
    /// In a DOM a JS engine drives.
    pub fn engine_word(&self, node: NodeRef<'gc>) -> u64 {
        node.into_ref().wrapper.embedder_word(self.gc)
    }

    /// # Panics
    ///
    /// In a DOM a JS engine drives.
    pub fn set_engine_word(&self, node: NodeRef<'gc>, word: u64) {
        node.into_ref().wrapper.set_embedder_word(self.gc, word);
    }

    /// `nodeType`.
    pub fn node_type(&self, node: NodeRef<'gc>) -> u16 {
        match self.kind(node) {
            NodeKind::Element => 1,
            NodeKind::Attr => 2,
            NodeKind::Text if self.flag(node, CDATA_SECTION) => 4,
            NodeKind::Text => 3,
            NodeKind::ProcessingInstruction => 7,
            NodeKind::Comment => 8,
            NodeKind::Document => 9,
            NodeKind::DocumentType => 10,
            NodeKind::DocumentFragment => 11,
        }
    }

    /// Whether `document` is an HTML document (`create_document`).
    pub fn is_html_document(&self, document: NodeRef<'gc>) -> bool {
        self.flag(document, HTML_DOCUMENT)
    }

    /// An element's name; none for other kinds.
    pub fn element_name(&self, node: NodeRef<'gc>) -> Option<NameIds> {
        (self.kind(node) == NodeKind::Element).then(|| node.qualified_name().ids())
    }

    /// An element's name as one number (style compares it); none for
    /// other kinds.
    pub fn qualified_name(&self, node: NodeRef<'gc>) -> Option<QualifiedName> {
        (self.kind(node) == NodeKind::Element).then(|| node.qualified_name())
    }

    /// `nodeName`: an HTML element's qualified name uppercased in an HTML
    /// document, another element's as it is.
    pub fn node_name(&self, node: NodeRef<'gc>) -> DomString {
        names::string(self.node_name_id(node))
    }

    /// `nodeName`'s number in the thread's names: made once for each element
    /// name (Blink's `QualifiedName::LocalNameUpper`), not a string each
    /// call.
    pub fn node_name_id(&self, node: NodeRef<'gc>) -> NameId {
        match self.kind(node) {
            NodeKind::Document => names::DOCUMENT_NAME,
            NodeKind::Text if self.flag(node, CDATA_SECTION) => names::intern("#cdata-section"),
            NodeKind::Text => names::TEXT_NAME,
            NodeKind::Comment => names::COMMENT_NAME,
            NodeKind::DocumentFragment => names::FRAGMENT_NAME,
            // Their name or target.
            NodeKind::DocumentType | NodeKind::ProcessingInstruction => {
                node.qualified_name().local()
            }
            // Its qualified name, as it is.
            NodeKind::Attr => {
                let name = self.attr_name(node);
                if name.prefix == names::NONE {
                    return name.local;
                }
                names::node_name(name.prefix, name.local, false)
            }
            NodeKind::Element => {
                let name = self.element_name(node).expect("an element's name");
                let uppercase = name.namespace == names::HTML_NAMESPACE
                    && self.is_html_document(self.document_of(node));
                if name.prefix == names::NONE && !uppercase {
                    return name.local;
                }
                names::node_name(name.prefix, name.local, uppercase)
            }
        }
    }

    /// `ownerDocument`: none for a document.
    pub fn owner_document(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        node.owner.get(self.gc).map(|_| self.document_of(node))
    }

    pub fn parent(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        node.parent.get(self.gc)
    }

    pub fn first_child(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        node.first_child.get(self.gc)
    }

    /// The first child's previous sibling, round the ring.
    pub fn last_child(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        node.first_child.get(self.gc)?.previous_sibling.get(self.gc)
    }

    /// None for a first child, whose ring's previous is the last child,
    /// with no next sibling.
    pub fn previous_sibling(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        node.previous_sibling
            .get(self.gc)
            .filter(|previous| !previous.next_sibling.is_null())
    }

    pub fn next_sibling(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        node.next_sibling.get(self.gc)
    }

    /// Whether one of `node`'s children is of `kind`.
    fn has_child_of(&self, node: NodeRef<'gc>, kind: NodeKind) -> bool {
        self.children_of(node, kind) > 0
    }

    /// How many of `node`'s children are of `kind`.
    fn children_of(&self, node: NodeRef<'gc>, kind: NodeKind) -> usize {
        let mut count = 0usize;
        let mut child = self.first_child(node);
        while let Some(at) = child {
            if self.kind(at) == kind {
                // At most the children there are, which memory bounds.
                count = count.saturating_add(1);
            }
            child = self.next_sibling(at);
        }
        count
    }

    /// Whether a sibling of `kind` comes after `child` (`following`) or
    /// before it.
    fn sibling_of(&self, child: NodeRef<'gc>, kind: NodeKind, following: bool) -> bool {
        let step = |at| {
            if following {
                self.next_sibling(at)
            } else {
                self.previous_sibling(at)
            }
        };
        let mut at = step(child);
        while let Some(node) = at {
            if self.kind(node) == kind {
                return true;
            }
            at = step(node);
        }
        false
    }

    /// DOM's "ensure pre-insertion validity" of `node` in `parent` before
    /// `child` (`insertNode` checks it before it splits a text).
    pub fn ensure_pre_insertion_validity(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        child: Option<NodeRef<'gc>>,
    ) -> Result<(), DomError> {
        let parent_kind = self.kind(parent);
        if !matches!(
            parent_kind,
            NodeKind::Document | NodeKind::DocumentFragment | NodeKind::Element
        ) {
            return Err(DomError::HierarchyRequest);
        }
        if self.is_host_including_inclusive_ancestor(node, parent) {
            return Err(DomError::HierarchyRequest);
        }
        if let Some(child) = child
            && !self.parent(child).is_some_and(|of| Gc::ptr_eq(of, parent))
        {
            return Err(DomError::NotFound);
        }
        let kind = self.kind(node);
        match kind {
            // A document or an `Attr` is no child.
            NodeKind::Document | NodeKind::Attr => return Err(DomError::HierarchyRequest),
            NodeKind::Text if parent_kind == NodeKind::Document => {
                return Err(DomError::HierarchyRequest);
            }
            NodeKind::DocumentType if parent_kind != NodeKind::Document => {
                return Err(DomError::HierarchyRequest);
            }
            _ => {}
        }
        if parent_kind != NodeKind::Document {
            return Ok(());
        }
        // An element after a doctype, at most one of each.
        let doctype_after = |child: Option<NodeRef<'gc>>| {
            child.is_some_and(|child| {
                self.kind(child) == NodeKind::DocumentType
                    || self.sibling_of(child, NodeKind::DocumentType, true)
            })
        };
        let element_rule = || {
            if self.has_child_of(parent, NodeKind::Element) || doctype_after(child) {
                Err(DomError::HierarchyRequest)
            } else {
                Ok(())
            }
        };
        match kind {
            NodeKind::DocumentFragment => {
                let elements = self.children_of(node, NodeKind::Element);
                if elements > 1 || self.has_child_of(node, NodeKind::Text) {
                    return Err(DomError::HierarchyRequest);
                }
                if elements == 1 {
                    element_rule()?;
                }
            }
            NodeKind::Element => element_rule()?,
            NodeKind::DocumentType => {
                let element_before = match child {
                    Some(child) => self.sibling_of(child, NodeKind::Element, false),
                    None => self.has_child_of(parent, NodeKind::Element),
                };
                if self.has_child_of(parent, NodeKind::DocumentType) || element_before {
                    return Err(DomError::HierarchyRequest);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// DOM's "pre-insert" (`insertBefore`, and `appendChild` with no
    /// `child`): `node` inserted in `parent` before `child`, once
    /// "ensure pre-insertion validity" holds.
    pub fn pre_insert(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        child: Option<NodeRef<'gc>>,
    ) -> Result<NodeRef<'gc>, DomError> {
        self.ensure_pre_insertion_validity(parent, node, child)?;
        // The reference child is the node itself: insert before its next.
        let child = match child {
            Some(child) if Gc::ptr_eq(child, node) => self.next_sibling(node),
            child => child,
        };
        self.insert(parent, node, child);
        Ok(node)
    }

    /// `appendChild`: `pre_insert` last.
    pub fn append_child(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
    ) -> Result<NodeRef<'gc>, DomError> {
        self.pre_insert(parent, node, None)
    }

    /// DOM's "replace"'s checks of `child` (of `parent`) replaced by
    /// `node`: "ensure pre-insertion validity"'s, `child` not counted.
    fn ensure_replace_validity(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        child: NodeRef<'gc>,
    ) -> Result<(), DomError> {
        let parent_kind = self.kind(parent);
        if !matches!(
            parent_kind,
            NodeKind::Document | NodeKind::DocumentFragment | NodeKind::Element
        ) || self.is_host_including_inclusive_ancestor(node, parent)
        {
            return Err(DomError::HierarchyRequest);
        }
        if !self.parent(child).is_some_and(|of| Gc::ptr_eq(of, parent)) {
            return Err(DomError::NotFound);
        }
        let kind = self.kind(node);
        match kind {
            // A document or an `Attr` is no child.
            NodeKind::Document | NodeKind::Attr => return Err(DomError::HierarchyRequest),
            NodeKind::Text if parent_kind == NodeKind::Document => {
                return Err(DomError::HierarchyRequest);
            }
            NodeKind::DocumentType if parent_kind != NodeKind::Document => {
                return Err(DomError::HierarchyRequest);
            }
            _ => {}
        }
        if parent_kind != NodeKind::Document {
            return Ok(());
        }
        // Another of `kind` than `child`.
        let other =
            |kind: NodeKind| self.children_of(parent, kind) > usize::from(self.kind(child) == kind);
        let element_fits =
            !other(NodeKind::Element) && !self.sibling_of(child, NodeKind::DocumentType, true);
        let fits = match kind {
            NodeKind::DocumentFragment => {
                if self.has_child_of(node, NodeKind::Text) {
                    return Err(DomError::HierarchyRequest);
                }
                match self.children_of(node, NodeKind::Element) {
                    0 => true,
                    1 => element_fits,
                    _ => false,
                }
            }
            NodeKind::Element => element_fits,
            NodeKind::DocumentType => {
                !other(NodeKind::DocumentType) && !self.sibling_of(child, NodeKind::Element, false)
            }
            _ => true,
        };
        if fits {
            Ok(())
        } else {
            Err(DomError::HierarchyRequest)
        }
    }

    /// DOM's "replace" (`replaceChild`): `child` of `parent` replaced by
    /// `node` (a fragment's children), once the checks hold: one record
    /// of what came and went, `node` leaving its old parent its own.
    pub fn replace_child(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        child: NodeRef<'gc>,
    ) -> Result<NodeRef<'gc>, DomError> {
        self.ensure_replace_validity(parent, node, child)?;
        // Where it goes: around `child`, `node` itself skipped.
        let skip = |at: Option<NodeRef<'gc>>, next: bool| match at {
            Some(at) if Gc::ptr_eq(at, node) => {
                if next {
                    self.next_sibling(at)
                } else {
                    self.previous_sibling(at)
                }
            }
            at => at,
        };
        let reference = skip(self.next_sibling(child), true);
        let previous = skip(self.previous_sibling(child), false);
        let observed = self.observed();
        let nodes = if observed {
            self.as_nodes(node)
        } else {
            Vec::new()
        };
        // Out of its old parent first (its own record), as "adopt" takes it.
        self.remove(node);
        let removed = self.parent(child).is_some();
        if removed {
            self.remove_noting(child, false);
        }
        self.insert_noting(parent, node, reference, false);
        if observed {
            let removed: &[NodeRef<'gc>] = if removed { &[child] } else { &[] };
            self.queue_tree_record(parent, &nodes, removed, previous, reference);
        }
        Ok(child)
    }

    /// `replaceChildren`: `parent`'s children replaced by `node` (DOM's
    /// "replace all"), once "ensure pre-insertion validity" holds.
    pub fn replace_children(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
    ) -> Result<(), DomError> {
        self.ensure_pre_insertion_validity(parent, node, None)?;
        self.replace_all(parent, Some(node));
        Ok(())
    }

    /// DOM's "insert", with no checks (the parser's): `node` taken from
    /// its parent, adopted into `parent`'s node document, and put in
    /// `parent`'s children before `child` (one of them), or last; a
    /// fragment's children instead, in order, the fragment left empty.
    pub fn insert(&self, parent: NodeRef<'gc>, node: NodeRef<'gc>, child: Option<NodeRef<'gc>>) {
        self.insert_noting(parent, node, child, true);
    }

    /// `insert`, its record at `parent` queued if `note` (else the
    /// caller's: DOM's "suppress observers flag").
    fn insert_noting(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        child: Option<NodeRef<'gc>>,
        note: bool,
    ) {
        let observed = self.observed();
        if self.kind(node) == NodeKind::DocumentFragment {
            let (nodes, previous) = if observed {
                (self.children(node), self.previous_of(parent, child))
            } else {
                (Vec::new(), None)
            };
            let scope = self.shadow_scope(parent);
            while let Some(first) = self.first_child(node) {
                self.remove_noting(first, false);
                self.adopt(first, self.document_of(parent));
                self.set_shadow_scope(first, scope);
                if let Some(child) = child
                    && self.has_seen_live()
                {
                    self.live_ranges_inserting(parent, child);
                }
                self.link(parent, first, child);
                if parent.scope.get() & CHILDREN_HOOKS != 0 {
                    self.children_changed(parent, first, true);
                }
            }
            if observed {
                self.queue_tree_record(node, &[], &nodes, None, None);
                if note {
                    self.queue_tree_record(parent, &nodes, &[], previous, child);
                }
            }
            return;
        }
        self.remove(node);
        self.adopt(node, self.document_of(parent));
        self.set_shadow_scope(node, self.shadow_scope(parent));
        let previous = if observed {
            self.previous_of(parent, child)
        } else {
            None
        };
        if let Some(child) = child
            && self.has_seen_live()
        {
            self.live_ranges_inserting(parent, child);
        }
        self.link(parent, node, child);
        if parent.scope.get() & CHILDREN_HOOKS != 0 {
            self.children_changed(parent, node, true);
        }
        if observed && note {
            self.queue_tree_record(parent, &[node], &[], previous, child);
        }
    }

    /// What will be before a node put in `parent`'s children before
    /// `child` (last for none).
    fn previous_of(
        &self,
        parent: NodeRef<'gc>,
        child: Option<NodeRef<'gc>>,
    ) -> Option<NodeRef<'gc>> {
        match child {
            Some(child) => self.previous_sibling(child),
            None => self.last_child(parent),
        }
    }

    /// `node`'s children, in order.
    pub(crate) fn children(&self, node: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
        let mut children = Vec::new();
        let mut at = self.first_child(node);
        while let Some(child) = at {
            children.push(child);
            at = self.next_sibling(child);
        }
        children
    }

    /// DOM's "pre-remove" (`removeChild`): `child` removed from `parent`,
    /// whose child it must be.
    pub fn pre_remove(
        &self,
        parent: NodeRef<'gc>,
        child: NodeRef<'gc>,
    ) -> Result<NodeRef<'gc>, DomError> {
        if !self.parent(child).is_some_and(|of| Gc::ptr_eq(of, parent)) {
            return Err(DomError::NotFound);
        }
        self.remove(child);
        Ok(child)
    }

    /// DOM's "remove", with no checks: `node` taken out of its parent's
    /// children, if it has a parent.
    pub fn remove(&self, node: NodeRef<'gc>) {
        self.remove_noting(node, true);
    }

    /// `remove`, its record queued if `note` (DOM's "suppress observers
    /// flag" unset); the transient registrations made either way.
    fn remove_noting(&self, node: NodeRef<'gc>, note: bool) {
        let Some(parent) = self.parent(node) else {
            return;
        };
        let previous = self.previous_sibling(node);
        let next = self.next_sibling(node);
        if self.observed() {
            self.add_transient_registrations(parent, node);
            if note {
                self.queue_tree_record(parent, &[], &[node], previous, next);
            }
        }
        if self.has_seen_live() {
            self.live_removing(parent, node);
        }
        // Its link round the ring: the last child if it is the first, its
        // previous sibling (the new last) if it is the last.
        let ring = node.previous_sibling.get(self.gc);
        match previous {
            Some(previous) => previous.next_sibling.set(self.gc, next),
            None => parent.first_child.set(self.gc, next),
        }
        if let Some(next) = next {
            next.previous_sibling.set(self.gc, previous);
        }
        // The ring: a new first or last child, its first's previous the last.
        if (previous.is_none() || next.is_none())
            && let Some(first) = parent.first_child.get(self.gc)
        {
            first.previous_sibling.set(self.gc, ring);
        }
        node.parent.set(self.gc, None);
        node.previous_sibling.set(self.gc, None);
        node.next_sibling.set(self.gc, None);
        self.removed(parent, node, previous, next);
        // Out of a shadow tree: into its document's.
        self.set_shadow_scope(node, None);
        if parent.scope.get() & CHILDREN_HOOKS != 0 {
            self.children_changed(parent, node, false);
        }
    }

    /// `parent`'s children changed (`node` came, `inserted`, or went), and
    /// `parent` has `CHILDREN_HOOKS`: its `childNodes`' cache cleared, the
    /// slot steps (`slot`), a textarea's value (`text_control`).
    #[cold]
    fn children_changed(&self, parent: NodeRef<'gc>, node: NodeRef<'gc>, inserted: bool) {
        if parent.scope.get() & lists::HAS_CHILD_LIST != 0 {
            self.child_list_changed(parent);
        }
        if parent.scope.get() & slot::SLOT_SCOPES != 0 {
            if inserted {
                self.slots_inserted(parent, node);
            } else {
                self.slots_removed(parent, node);
            }
        }
        if parent.scope.get() & text_control::TEXTAREA_CHILDREN != 0 {
            self.textarea_children_changed(parent);
        }
    }

    /// DOM's "host-including inclusive ancestor": up the tree, and from a
    /// fragment with a host (a shadow root, a template's contents) on to
    /// it.
    fn is_host_including_inclusive_ancestor(
        &self,
        ancestor: NodeRef<'gc>,
        node: NodeRef<'gc>,
    ) -> bool {
        let mut at = node;
        loop {
            if Gc::ptr_eq(at, ancestor) {
                return true;
            }
            match self.parent(at) {
                Some(parent) => at = parent,
                // On through the host only at a fragment: out of the way
                // of the walk (a read of its kind at the root).
                None if self.kind(at) == NodeKind::DocumentFragment => {
                    return self.is_host_including_ancestor_of_host(ancestor, at);
                }
                None => return false,
            }
        }
    }

    /// `is_host_including_inclusive_ancestor` on from `root`'s host.
    #[cold]
    #[inline(never)]
    fn is_host_including_ancestor_of_host(
        &self,
        ancestor: NodeRef<'gc>,
        root: NodeRef<'gc>,
    ) -> bool {
        self.shadow_host(root)
            .or_else(|| self.template_content_host(root))
            .is_some_and(|host| self.is_host_including_inclusive_ancestor(ancestor, host))
    }

    /// `adoptNode`'s: `node` taken out of its parent, and DOM's "adopt"
    /// into `document` (the caller checks it isn't a document or a shadow
    /// root).
    pub fn adopt_node(&self, node: NodeRef<'gc>, document: NodeRef<'gc>) {
        self.remove(node);
        self.adopt(node, document);
    }

    /// DOM's "adopt": `node` and its descendants moved to `document`, if
    /// they are of another; a template's contents to its template document
    /// (HTML's template adopting steps).
    fn adopt(&self, node: NodeRef<'gc>, document: NodeRef<'gc>) {
        let old = self.document_of(node);
        if Gc::ptr_eq(old, document) {
            return;
        }
        self.adopt_tree(node, old, document);
        if self.has_seen_live() {
            self.live_adopted(old, document);
        }
    }

    /// `adopt`'s walk of `root`'s tree: the shadow trees of its hosts too,
    /// whose nodes keep their shadow root as their owner (their document
    /// is the shadow root's); each custom element's `adoptedCallback`.
    fn adopt_tree(&self, root: NodeRef<'gc>, old: NodeRef<'gc>, document: NodeRef<'gc>) {
        self.for_each_inclusive_descendant(root, |at| {
            if at.scope.get() & shadow::IN_SHADOW_TREE == 0 {
                at.owner.set(self.gc, Some(document));
            }
            if self.is_custom_element(at) {
                self.enqueue_callback_reaction(at, CallbackArgs::Adopted(old, document));
            }
            self.adopt_attr_nodes(at, document);
            self.image_adopted(at);
            if let Some(content) = self.template_content(at) {
                self.adopt(content, self.template_document(document));
            }
            if let Some(shadow) = self.shadow_root(at) {
                self.adopt_tree(shadow, old, document);
                // In its new document by now.
                self.shadow_root_adopted(shadow, old, document);
            }
        });
    }

    /// `f` of `root` and its descendants, in tree order.
    fn for_each_inclusive_descendant(&self, root: NodeRef<'gc>, mut f: impl FnMut(NodeRef<'gc>)) {
        let mut at = Some(root);
        while let Some(current) = at {
            f(current);
            // Down, else across, else up and across, never past `root`.
            at = self.first_child(current).or_else(|| {
                let mut up = Some(current);
                while let Some(from) = up.filter(|from| !Gc::ptr_eq(*from, root)) {
                    if let Some(next) = self.next_sibling(from) {
                        return Some(next);
                    }
                    up = self.parent(from);
                }
                None
            });
        }
    }

    /// Puts `node`, with no parent, in `parent`'s children before `child`
    /// (one of them), or last. The caller runs `children_changed` (a
    /// clone's copies, new, have none of its bits).
    fn link(&self, parent: NodeRef<'gc>, node: NodeRef<'gc>, child: Option<NodeRef<'gc>>) {
        // `child`'s link round the ring is the last child if it is the
        // first: no read of the first child for that.
        let (previous, last) = match child {
            Some(child) => {
                let ring = child.previous_sibling.get(self.gc);
                (
                    ring.filter(|previous| !previous.next_sibling.is_null()),
                    ring,
                )
            }
            None => {
                let last = self.last_child(parent);
                (last, last)
            }
        };
        node.parent.set(self.gc, Some(parent));
        node.previous_sibling.set(self.gc, previous);
        node.next_sibling.set(self.gc, child);
        match previous {
            Some(previous) => previous.next_sibling.set(self.gc, Some(node)),
            None => parent.first_child.set(self.gc, Some(node)),
        }
        if let Some(child) = child {
            child.previous_sibling.set(self.gc, Some(node));
        }
        // Its subtree's attribute names and classes are its new ancestors'.
        let bloom = node.bloom.get();
        if bloom != 0 {
            self.add_to_bloom(parent, bloom);
        }
        // The ring: a new first or last child, its first's previous the last.
        if previous.is_none() || child.is_none() {
            let first = parent.first_child.get(self.gc).expect("`node` at least");
            let last = if child.is_none() { Some(node) } else { last };
            first.previous_sibling.set(self.gc, last);
        }
        self.inserted(parent, node, previous, child);
    }

    /// `element.attributes.length`: none for another kind.
    pub fn attribute_count(&self, element: NodeRef<'gc>) -> usize {
        self.synchronize_attributes(element);
        element
            .attributes
            .get(self.gc)
            .map_or(0, |attributes| attributes.len())
    }

    /// The attribute at `index` in `element`'s list, in the order they were
    /// added.
    pub fn attribute_at(&self, element: NodeRef<'gc>, index: usize) -> Option<AttributeData> {
        self.synchronize_attributes(element);
        let attribute = element.attributes.get(self.gc)?.get(self.gc, index)?;
        Some(AttributeData {
            name: attribute.qualified_name().ids(),
            value: attribute.value.get().borrow().clone(),
        })
    }

    /// The name of the attribute at `index` (`attribute_at`'s), as one
    /// number: what style compares, with no string.
    pub fn attribute_name_at(&self, element: NodeRef<'gc>, index: usize) -> Option<QualifiedName> {
        self.synchronize_attributes(element);
        let attribute = element.attributes.get(self.gc)?.get(self.gc, index)?;
        Some(attribute.qualified_name())
    }

    /// The value of the attribute at `index`, where it is (no clone).
    pub fn attribute_value_at(&self, element: NodeRef<'gc>, index: usize) -> Option<StrRef<'gc>> {
        self.synchronize_attributes(element);
        let attribute = element.attributes.get(self.gc)?.get(self.gc, index)?;
        Some(StrRef(attribute.into_ref().value.get()))
    }

    /// `element`'s attributes as they are: its `style` attribute written
    /// from its declarations if CSSOM changed them (one bit looked at).
    #[inline]
    fn synchronize_attributes(&self, element: NodeRef<'gc>) {
        if element.flags.get() & style::STYLE_ATTRIBUTE_DIRTY != 0 {
            self.synchronize_style_attribute(element);
        }
    }

    /// `element`'s attribute `name` set to `value` (added if it has none),
    /// with no notice and no synchronizing: a lazily written attribute's
    /// (`style`).
    pub(crate) fn write_attribute_quietly(
        &self,
        element: NodeRef<'gc>,
        name: QualifiedName,
        value: &str,
    ) {
        let value = string::intern_value(value);
        if let Some(attributes) = element.attributes.get(self.gc) {
            for index in 0..attributes.len() {
                if let Some(attribute) = attributes.get(self.gc, index)
                    && attribute.qualified_name() == name
                {
                    *attribute.value.get().borrow_mut() = value;
                    return;
                }
            }
        }
        let attributes = match element.attributes.get(self.gc) {
            Some(attributes) => attributes,
            None => {
                let attributes = HeapVector::new_beside(self.gc, element, 1);
                element.attributes.set(self.gc, Some(attributes));
                attributes
            }
        };
        let attribute = self.gc.alloc_beside(
            element,
            &sites::ATTRIBUTE,
            AttributeInit {
                name: Cell::new(name.raw()),
                value: LocalExternal::new(RefCell::new(value)),
            },
        );
        attributes.push(self.gc, attribute);
    }

    /// The index of the first of `element`'s attributes `matches` takes.
    fn find_attribute(
        &self,
        element: NodeRef<'gc>,
        mut matches: impl FnMut(&Attribute) -> bool,
    ) -> Option<(usize, Gc<'gc, Attribute>)> {
        self.synchronize_attributes(element);
        let attributes = element.attributes.get(self.gc)?;
        (0..attributes.len())
            .filter_map(|index| Some((index, attributes.get(self.gc, index)?)))
            .find(|(_, attribute)| matches(attribute))
    }

    /// The attribute in `namespace` (none for the null namespace) named
    /// `local`.
    fn attribute_ns(
        &self,
        element: NodeRef<'gc>,
        namespace: Option<&str>,
        local: &str,
    ) -> Option<(usize, Gc<'gc, Attribute>)> {
        // An empty namespace is none, `names::NONE`, as in `intern_name`.
        let (namespace, local) = (names::find(namespace.unwrap_or(""))?, names::find(local)?);
        self.find_attribute(element, |attribute| {
            attribute.qualified_name().is(namespace, local)
        })
    }

    /// The first attribute whose qualified name is `qualified` (DOM's "get
    /// an attribute by name", the name already lowercased if need be).
    fn attribute_by_name(
        &self,
        element: NodeRef<'gc>,
        qualified: &str,
    ) -> Option<(usize, Gc<'gc, Attribute>)> {
        // Every attribute's name is interned: one the table doesn't have is
        // no attribute's, and the rest compare as numbers. Split at a colon
        // only for an attribute with a prefix (rare).
        let whole = names::find(qualified);
        self.find_attribute(element, |attribute| {
            let name = attribute.qualified_name().ids();
            match name.prefix {
                names::NONE => Some(name.local) == whole,
                prefix => qualified.split_once(':').is_some_and(|(p, l)| {
                    names::find(p) == Some(prefix) && names::find(l) == Some(name.local)
                }),
            }
        })
    }

    /// `getAttributeNS`: the value of the attribute in `namespace` (none or
    /// empty for the null namespace) named `local`.
    pub fn get_attribute_ns(
        &self,
        element: NodeRef<'gc>,
        namespace: Option<&str>,
        local: &str,
    ) -> Option<DomString> {
        self.attribute_value_ns(element, namespace, local)
            .map(|value| value.to_owned())
    }

    /// `get_attribute_ns`'s value where it is.
    pub fn attribute_value_ns(
        &self,
        element: NodeRef<'gc>,
        namespace: Option<&str>,
        local: &str,
    ) -> Option<StrRef<'gc>> {
        self.attribute_ns(element, namespace, local)
            .map(|(_, attribute)| StrRef(attribute.into_ref().value.get()))
    }

    /// `attribute_value_ns` of an attribute in no namespace whose local
    /// name is the table's own `local` (`names::ID`, `names::CLASS`): no
    /// look in the table.
    pub fn attribute_value_by_id(
        &self,
        element: NodeRef<'gc>,
        local: NameId,
    ) -> Option<StrRef<'gc>> {
        self.find_attribute(element, |attribute| {
            attribute.qualified_name().is(names::NONE, local)
        })
        .map(|(_, attribute)| StrRef(attribute.into_ref().value.get()))
    }

    /// `getAttribute`: the value of the first attribute whose qualified name
    /// is `qualified` (lowercased first by the caller in an HTML document).
    pub fn get_attribute(&self, element: NodeRef<'gc>, qualified: &str) -> Option<DomString> {
        self.attribute_value(element, qualified)
            .map(|value| value.to_owned())
    }

    /// `get_attribute`'s value where it is.
    pub fn attribute_value(&self, element: NodeRef<'gc>, qualified: &str) -> Option<StrRef<'gc>> {
        self.attribute_by_name(element, qualified)
            .map(|(_, attribute)| StrRef(attribute.into_ref().value.get()))
    }

    /// DOM's "set an attribute value": the attribute in `name`'s namespace
    /// with its local name gets `value`, its prefix unchanged; if there is
    /// none, one is appended with `name`.
    ///
    /// # Panics
    ///
    /// If `element` isn't an element.
    pub fn set_attribute_ns(&self, element: NodeRef<'gc>, name: QualName<'_>, value: &str) {
        if let Some((_, attribute)) = self.attribute_ns(element, name.namespace, name.local) {
            self.change_attribute(element, attribute, value);
            return;
        }
        let name = self.intern_name(name);
        self.append_attribute(element, name, value);
    }

    /// `setAttribute`: the first attribute whose qualified name is
    /// `qualified` (lowercased first by the caller in an HTML document) gets
    /// `value`; if there is none, one with that local name and no namespace
    /// or prefix is appended.
    ///
    /// # Panics
    ///
    /// If `element` isn't an element.
    pub fn set_attribute(&self, element: NodeRef<'gc>, qualified: &str, value: &str) {
        if let Some((_, attribute)) = self.attribute_by_name(element, qualified) {
            self.change_attribute(element, attribute, value);
            return;
        }
        let name = self.intern_name(QualName::local(qualified));
        self.append_attribute(element, name, value);
    }

    fn change_attribute(&self, element: NodeRef<'gc>, attribute: Gc<'gc, Attribute>, value: &str) {
        let old = std::mem::replace(
            &mut *attribute.value.get().borrow_mut(),
            string::intern_value(value),
        );
        self.attribute_changed(
            element,
            attribute.qualified_name(),
            Some(&old.to_str()),
            Some(value),
        );
    }

    fn append_attribute(&self, element: NodeRef<'gc>, name: NameIds, value: &str) {
        let name = QualifiedName::new(name);
        let attribute = self.gc.alloc_beside(
            element,
            &sites::ATTRIBUTE,
            AttributeInit {
                name: Cell::new(name.raw()),
                value: LocalExternal::new(RefCell::new(string::intern_value(value))),
            },
        );
        self.append_attribute_object(element, attribute);
    }

    /// DOM's "append an attribute": `attribute` last of `element`'s.
    fn append_attribute_object(&self, element: NodeRef<'gc>, attribute: Gc<'gc, Attribute>) {
        assert_eq!(
            self.kind(element),
            NodeKind::Element,
            "only an element has attributes"
        );
        let attributes = match element.attributes.get(self.gc) {
            Some(attributes) => attributes,
            None => {
                let attributes = HeapVector::new_beside(self.gc, element, 1);
                element.attributes.set(self.gc, Some(attributes));
                attributes
            }
        };
        attributes.push(self.gc, attribute);
        let name = attribute.qualified_name();
        let value = attribute.value.get().borrow().clone();
        self.attribute_added(element, name);
        self.attribute_changed(element, name, None, Some(&value.to_str()));
    }

    /// `removeAttributeNS`: the attribute in `namespace` named `local`
    /// removed; its value, if there was one.
    pub fn remove_attribute_ns(
        &self,
        element: NodeRef<'gc>,
        namespace: Option<&str>,
        local: &str,
    ) -> Option<DomString> {
        let (index, _) = self.attribute_ns(element, namespace, local)?;
        Some(self.remove_attribute_at(element, index))
    }

    /// `removeAttribute`: the first attribute whose qualified name is
    /// `qualified` removed; its value, if there was one.
    pub fn remove_attribute(&self, element: NodeRef<'gc>, qualified: &str) -> Option<DomString> {
        let (index, _) = self.attribute_by_name(element, qualified)?;
        Some(self.remove_attribute_at(element, index))
    }

    fn remove_attribute_at(&self, element: NodeRef<'gc>, index: usize) -> DomString {
        let attributes = element.attributes.get(self.gc).expect("its attributes");
        let attribute = attributes.remove(self.gc, index);
        self.detach_attr_node(element, attribute);
        let value = attribute.value.get().borrow().clone();
        self.attribute_changed(
            element,
            attribute.qualified_name(),
            Some(&value.to_str()),
            None,
        );
        value
    }

    /// A character data node's data (a text's, a comment's, a processing
    /// instruction's); none for another kind.
    pub fn data(&self, node: NodeRef<'gc>) -> Option<DomString> {
        self.text_data(node).map(|data| data.to_owned())
    }

    /// `data` where it is.
    pub fn text_data(&self, node: NodeRef<'gc>) -> Option<StrRef<'gc>> {
        let data = node.data.get(self.gc)?.downcast::<CharacterData>()?;
        Some(StrRef(data.into_ref().data.get()))
    }

    fn character_data_of(&self, node: NodeRef<'gc>) -> Gc<'gc, CharacterData> {
        node.data
            .get(self.gc)
            .and_then(|data| data.downcast::<CharacterData>())
            .expect("a character data node's data")
    }

    /// A character data node's data replaced by `data`.
    ///
    /// # Panics
    ///
    /// If `node` isn't a character data node.
    pub fn set_data(&self, node: NodeRef<'gc>, data: &str) {
        if self.observed() {
            let old = self
                .character_data_of(node)
                .data
                .get()
                .borrow()
                .to_str()
                .into_owned();
            self.queue_character_data_record(node, &old);
        }
        if self.has_seen_live() {
            // DOM's "replace data" of all of it.
            let old = self.character_data_of(node).data.get().borrow().utf16_len();
            self.live_ranges_data_replaced(node, 0, utf16_u32(old), utf16_u32(utf16_len(data)));
        }
        *self.character_data_of(node).data.get().borrow_mut() = DomString::new(data);
        self.data_changed(node);
    }

    /// DOM's "replace data": `count` UTF-16 code units of a character data
    /// node's data from `offset` (fewer at its end) replaced by `data`, its
    /// record queued (`DomString::utf16_splice`: half a surrogate pair
    /// left at an edge becomes U+FFFD).
    ///
    /// # Panics
    ///
    /// If `node` isn't a character data node.
    pub fn replace_data(&self, node: NodeRef<'gc>, offset: usize, count: usize, data: &str) {
        let old = self.character_data_of(node).data.get().borrow().clone();
        if self.observed() {
            self.queue_character_data_record(node, &old.to_str());
        }
        if self.has_seen_live() {
            // A `count` past the end needs no clamping here: no point is
            // past the length.
            self.live_ranges_data_replaced(
                node,
                utf16_u32(offset),
                utf16_u32(count),
                utf16_u32(utf16_len(data)),
            );
        }
        let new = old.utf16_splice(offset, count, data);
        *self.character_data_of(node).data.get().borrow_mut() = DomString::from(new);
        self.data_changed(node);
    }

    /// `more` appended to a character data node's data (the parser's text
    /// that continues a text node).
    ///
    /// # Panics
    ///
    /// If `node` isn't a character data node.
    pub fn append_data(&self, node: NodeRef<'gc>, more: &str) {
        let data = self.character_data_of(node);
        let mut data = data.data.get().borrow_mut();
        let joined = format!("{}{more}", data.to_str());
        *data = DomString::from(joined);
        drop(data);
        self.data_changed(node);
    }

    /// `textContent`'s getter: a character data node's data; an element's
    /// or a fragment's descendant texts' joined; none for a document or a
    /// doctype.
    pub fn text_content(&self, node: NodeRef<'gc>) -> Option<DomString> {
        match self.kind(node) {
            NodeKind::Document | NodeKind::DocumentType => None,
            NodeKind::Text | NodeKind::Comment | NodeKind::ProcessingInstruction => self.data(node),
            NodeKind::Attr => Some(self.attr_value_string(node)),
            NodeKind::Element | NodeKind::DocumentFragment => {
                let mut joined = String::new();
                self.for_each_inclusive_descendant(node, |at| {
                    if self.kind(at) == NodeKind::Text
                        && let Some(data) = self.text_data(at)
                    {
                        joined.push_str(&data.borrow().to_str());
                    }
                });
                Some(DomString::from(joined))
            }
        }
    }

    /// `textContent`'s setter: a character data node's data; an element's
    /// or a fragment's children replaced by a text of `value` (none if it
    /// is empty); nothing for a document or a doctype.
    pub fn set_text_content(&self, node: NodeRef<'gc>, value: &str) {
        match self.kind(node) {
            NodeKind::Document | NodeKind::DocumentType => {}
            NodeKind::Text | NodeKind::Comment | NodeKind::ProcessingInstruction => {
                self.set_data(node, value);
            }
            NodeKind::Attr => self.set_attr_value(node, value),
            NodeKind::Element | NodeKind::DocumentFragment => {
                if self.observed() {
                    // DOM's "replace all": one record.
                    let text = (!value.is_empty()).then(|| self.create_text(node, value));
                    self.replace_all(node, text);
                    return;
                }
                // A select's options' steps wait for the end, as `replace_all`'s.
                let options = self.options_replacing(node);
                while let Some(child) = self.first_child(node) {
                    self.remove(child);
                }
                if options {
                    self.options_replaced(node);
                }
                if !value.is_empty() {
                    // New, and of `node`'s document: linked, not inserted.
                    let text = self.create_text(node, value);
                    self.link(node, text, None);
                    if node.scope.get() & CHILDREN_HOOKS != 0 {
                        self.children_changed(node, text, true);
                    }
                }
            }
        }
    }

    /// DOM's "replace all": `parent`'s children removed, then `node` (a
    /// fragment's children) inserted, if there is one.
    pub fn replace_all(&self, parent: NodeRef<'gc>, node: Option<NodeRef<'gc>>) {
        let observed = self.observed();
        let (removed, added) = if observed {
            (
                self.children(parent),
                node.map_or_else(Vec::new, |node| self.as_nodes(node)),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        let options = self.options_replacing(parent);
        while let Some(child) = self.first_child(parent) {
            self.remove_noting(child, false);
        }
        if let Some(node) = node {
            self.insert_noting(parent, node, None, false);
        }
        if options {
            self.options_replaced(parent);
        }
        if observed {
            self.queue_tree_record(parent, &added, &removed, None, None);
        }
    }

    /// The nodes inserting `node` inserts: a fragment's children, or it.
    fn as_nodes(&self, node: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
        if self.kind(node) == NodeKind::DocumentFragment {
            self.children(node)
        } else {
            vec![node]
        }
    }

    /// `child` (one of its parent's children) replaced by `node` (a
    /// fragment's children) where it was, with no checks: DOM's "replace"
    /// of a node the caller made (`outerHTML`'s fragment), which can be
    /// any parent's child.
    pub fn replace_with_new(&self, child: NodeRef<'gc>, node: NodeRef<'gc>) {
        let parent = self.parent(child).expect("a child replaced has a parent");
        let next = self.next_sibling(child);
        let previous = self.previous_sibling(child);
        let observed = self.observed();
        let added = if observed {
            self.as_nodes(node)
        } else {
            Vec::new()
        };
        self.remove_noting(child, false);
        self.insert_noting(parent, node, next, false);
        if observed {
            self.queue_tree_record(parent, &added, &[child], previous, next);
        }
    }

    /// A root of `node`, kept past the operation.
    pub fn root(&self, node: NodeRef<'gc>) -> NodeRoot {
        Persistent::new(self.gc, node)
    }

    /// DOM's "clone a node": a copy of `node` in `document` (its node
    /// document if none), with its attributes or data; its descendants too
    /// if `deep`, and a template's contents (HTML's cloning steps).
    pub fn clone_node(
        &self,
        node: NodeRef<'gc>,
        document: Option<NodeRef<'gc>>,
        deep: bool,
    ) -> NodeRef<'gc> {
        let document = document.unwrap_or_else(|| self.document_of(node));
        let copy = match self.kind(node) {
            NodeKind::Document => {
                let copy = self.create_document(self.is_html_document(node));
                self.set_document_relevant_window(copy, self.document_relevant_window(node));
                self.set_quirks_mode(copy, self.quirks_mode(node));
                // Its type, content type and encoding (DOM's "clone").
                if self.is_xml_document_interface(node) {
                    self.set_xml_document_interface(copy);
                }
                self.set_document_content_type(copy, &self.document_content_type(node).to_str());
                self.set_document_character_set(copy, &self.document_character_set(node).to_str());
                copy
            }
            NodeKind::DocumentType => {
                let doctype = self.doctype(node).expect("a doctype's");
                self.create_doctype(
                    document,
                    &doctype.name.to_str(),
                    &doctype.public_id.to_str(),
                    &doctype.system_id.to_str(),
                )
            }
            NodeKind::DocumentFragment => self.create_document_fragment(document),
            NodeKind::Attr => self.clone_attr(node, document),
            NodeKind::Element => {
                let copy = self.create_element_named(
                    document,
                    self.element_name(node).expect("an element's name"),
                );
                // A custom element's name: undefined, and an upgrade
                // reaction if it is defined (HTML's "create an element"
                // with no synchronous custom elements flag).
                if self.custom_element_state(node) != CustomElementState::Uncustomized {
                    self.set_custom_element_state(copy, CustomElementState::Undefined);
                    if let Some(definition) = self.custom_element_definition_for(copy) {
                        self.enqueue_upgrade_reaction(copy, definition);
                    }
                }
                for index in 0..self.attribute_count(node) {
                    let attribute = self
                        .attribute_at(node, index)
                        .expect("an attribute below the count");
                    self.append_attribute(copy, attribute.name, &attribute.value.to_str());
                }
                self.input_cloned(node, copy);
                self.textarea_cloned(node, copy);
                self.script_cloned(node, copy);
                self.option_cloned(node, copy);
                if deep
                    && let (Some(from), Some(to)) =
                        (self.template_content(node), self.template_content(copy))
                {
                    let mut child = self.first_child(from);
                    while let Some(at) = child {
                        let cloned = self.clone_node(at, Some(self.document_of(to)), true);
                        self.link(to, cloned, None);
                        child = self.next_sibling(at);
                    }
                }
                copy
            }
            NodeKind::Text => {
                let copy = self.create_text(document, &self.data(node).expect("a text's").to_str());
                if self.flag(node, CDATA_SECTION) {
                    copy.flags.set(copy.flags.get() | CDATA_SECTION);
                }
                copy
            }
            NodeKind::Comment => {
                self.create_comment(document, &self.data(node).expect("a comment's").to_str())
            }
            NodeKind::ProcessingInstruction => self.create_processing_instruction(
                document,
                &self.target(node).expect("a target").to_str(),
                &self.data(node).expect("data").to_str(),
            ),
        };
        if deep {
            let copy_document = self.document_of(copy);
            let mut child = self.first_child(node);
            while let Some(at) = child {
                let cloned = self.clone_node(at, Some(copy_document), true);
                self.link(copy, cloned, None);
                child = self.next_sibling(at);
            }
        }
        // DOM's: a clonable shadow root comes with its host, deep or not.
        if let Some(shadow) = self.shadow_root(node)
            && let Some(init) = self.shadow_root_init(shadow).filter(|init| init.clonable)
        {
            let root = self.attach_shadow(copy, init);
            let mut child = self.first_child(shadow);
            while let Some(at) = child {
                let cloned = self.clone_node(at, Some(self.document_of(root)), true);
                self.insert(root, cloned, None);
                child = self.next_sibling(at);
            }
        }
        copy
    }
}
