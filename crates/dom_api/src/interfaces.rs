//! The table's interfaces (`declare!`): the DOM standard's tree and
//! events, as Blink keeps them in `core/dom`. `ParentNode`'s members are in `Document`,
//! `DocumentFragment` and `Element`, which include it.

use dom::DomString;
use dom::names::event as event_type;
use dom::names::local;

use crate::history::{HistoryState, SerializedState};
use crate::message::{PortMessage, WindowMessage};
use crate::typed::{
    AbortReason, AbortSignalObject, AbortSignals, AllCollectionObject, AlwaysNull,
    AnimationEventObject, AnimationObject, AnimationOptions, AnimationPlaybackEventObject, Any,
    AnyArg, AnyOrNull, AnyOrUndefined, ArrayView, BlobCallback, BlobParts, BodyMember, ByteString,
    CanvasImageSource, CanvasStyleValue, CloneValue, Cloned, ConfidenceObject,
    CustomElementConstructor, CustomEventObject, DictOrBool, DictionaryOut, Doubles, EffectOptions,
    ElementLocalName, EmptyStringDefault, EnforcedI32, EnforcedU64, EntryDetail, ErrorEventObject,
    EventHandler, EventOrUndefined, FileObject, FileReaderObject, FilledView, FocusEventObject,
    FormControlsCollectionObject, FrameRequestCallback, FrozenArray, FrozenStrings,
    GetHtmlOptionsArg, HTMLCollectionOrElement, HashChangeEventObject, HeaderPairs, HeadersMember,
    Hidden, IdleRequestCallback, InputEventObject, IntersectionObserverCallback,
    IntersectionObserverEntryInitArg, IntersectionObserverInitArg, IsNull, JsonData,
    KeyboardEventObject, Keyframes, LegacyNullToEmptyString, LongDefaultMinusOne, LongDefaultOne,
    MeasureStart, MediaQueryListEventObject, MediaQueryListObject, MessageEventObject,
    MessagePortObject, MessagePorts, MouseEventObject, MutationCallback, NamespaceObject,
    NavigationTimingObject, NetworkInformationObject, NewElement, NewHtmlElement, NewImageData,
    NewNodeLikeThis, NewObject, NoObject, NoPositionObject, NodesOrStrings, NullableBody,
    NullableElementOrLong, NullableU32, NumberIfNumber, Numbers, ObservableArray, OptionOrOptGroup,
    OptionalClampedI64, OptionalPath2dOrString, OptionalString, OptionalTimingArg,
    OptionalUnrestrictedDouble, OptionsCollectionObject, PendingPromise, PerformanceEntryObject,
    PerformanceMarkObject, PerformanceMeasureObject, PerformanceNavigationObject,
    PerformanceObject, PerformanceObserverCallback, PerformanceTimingObject, PopStateEventObject,
    PositionCallback, PositionErrorCallback, ProgressEventObject, Promise, PromiseOfAnimation,
    PromiseOfAny, PromiseOfArrayBuffer, PromiseOfJson, PromiseOfString, PromiseOfUint8Array,
    PromiseOfUndefined, PromiseRejectionEventObject, RadioNodeListObject, RadioNodeListOrElement,
    ReaderResult, Records, RelatedTarget, RequestInfo, ResizeObserverCallback, Rest,
    RoundRectRadii, Sequence, Serialized, SignalMember, SignalObject, Slottables,
    StaticRangeInitArg, StringOrNumber, StringPairs, Strings, TargetObject, TextEventObject,
    TimelineArg, TimerHandler, TransitionEventObject, U16Default302, UiEventObject,
    Uint8ClampedArray, UnrestrictedDouble, ViewObject, WakeLockSentinelObject, WhatToShow,
    WheelEventObject,
};
use crate::window::NavigationUrl;
use crate::{
    abort, activation, animation, animation_frame, animations, attr, blob, canvas, character_data,
    child_node, collection, computed_style, crypto, css_namespace, css_style, css_style_sheet,
    cssom, custom_elements, dataset, document, document_type, dom_implementation, effect, element,
    event, event_target, fetch, file_reader, focus, forms, frame, geolocation, geometry, handlers,
    headers, history, html, hyperlink, idle, image, intersection_observer, location, markup,
    media_query_list, menu_list, message, mutation_observer, navigator, node, parent_node,
    performance, query, range, reflect, request, resize_observer, response, script, scroll, select,
    selection, shadow_root, slot, svg, svg_geometry, table, text_control, timer, token_list,
    traversal, ui_event, url, wake_lock, window,
};

crate::declare! {
    interface EventTarget [type = TargetObject] {
        #[front]
        constructor() = event_target::construct_event_target;
        #[front]
        method addEventListener(
            r#type: DomString,
            callback: Option<EventListener>,
            options?: DictOrBool<AddEventListenerOptions>,
        ) = event_target::add_event_listener;
        #[front]
        method removeEventListener(
            r#type: DomString,
            callback: Option<EventListener>,
            options?: DictOrBool<EventListenerOptions>,
        ) = event_target::remove_event_listener;
        #[calls_script]
        method dispatchEvent(event: Event) -> bool = event_target::dispatch_event;
    }

    callback EventListener;

    dictionary EventListenerOptions {
        capture: bool = false,
    }

    // `EventListenerOptions`' members first, as WebIDL reads an inherited
    // dictionary's (`capture` sorts first anyway).
    // `passive` has no default: DOM's "default passive value" then.
    // `signal` an `AbortSignal` (the front end's handle, checked as it
    // is added).
    dictionary AddEventListenerOptions {
        capture: bool = false,
        once: bool = false,
        passive: Option<bool> = None,
        signal: Option<SignalObject<T::Node>> = None,
    }

    interface Event [type = dom::Event] {
        attribute r#type: DomString, get = event::event_type;
        attribute target: Option<EventTarget>, get = event::target;
        attribute srcElement: Option<EventTarget>, get = event::target;
        attribute currentTarget: Option<EventTarget>, get = event::current_target;
        method composedPath() -> Sequence<EventTarget> = event::composed_path;
        const NONE: u32 = 0;
        const CAPTURING_PHASE: u32 = 1;
        const AT_TARGET: u32 = 2;
        const BUBBLING_PHASE: u32 = 3;
        #[front]
        constructor(r#type: DomString, eventInitDict?: EventInit) = event::construct_event;
        attribute eventPhase: u16, get = event::event_phase;
        method stopPropagation() = event::stop_propagation;
        attribute cancelBubble: bool, get = event::cancel_bubble, set = event::set_cancel_bubble;
        method stopImmediatePropagation() = event::stop_immediate_propagation;
        attribute bubbles: bool, get = event::bubbles;
        attribute cancelable: bool, get = event::cancelable;
        attribute returnValue: bool, get = event::return_value, set = event::set_return_value;
        method preventDefault() = event::prevent_default;
        attribute defaultPrevented: bool, get = event::default_prevented;
        attribute composed: bool, get = event::composed;
        #[unforgeable]
        attribute isTrusted: bool, get = event::is_trusted;
        attribute timeStamp: f64, get = event::time_stamp;
        method initEvent(r#type: DomString, bubbles?: bool, cancelable?: bool) = event::init_event;
    }

    dictionary EventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
    }

    // DOM's: an event with a value of script's.
    interface CustomEvent : Event [type = CustomEventObject] {
        #[front]
        constructor(r#type: DomString, eventInitDict?: CustomEventInit) = event::construct_custom_event;
        attribute detail: AnyOrNull, get = event::detail;
        // Legacy: for `createEvent`'s events.
        #[front]
        method initCustomEvent(r#type: DomString, bubbles?: bool, cancelable?: bool, detail?: Any) =
            event::init_custom_event;
    }

    dictionary CustomEventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        detail: Option<AnyArg<T::Any>> = None,
    }

    // HTML's messages (`message.rs`).
    interface PopStateEvent : Event [type = PopStateEventObject] {
        #[front]
        constructor(r#type: DomString, eventInitDict?: PopStateEventInit) = history::construct_pop_state_event;
        attribute state: AnyOrNull, get = history::pop_state_state;
        attribute hasUAVisualTransition: bool, get = history::has_ua_visual_transition;
    }

    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary PopStateEventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        state: Option<AnyArg<T::Any>> = None,
        hasUAVisualTransition: bool = false,
    }

    interface HashChangeEvent : Event [type = HashChangeEventObject] {
        #[front]
        constructor(r#type: DomString, eventInitDict?: HashChangeEventInit) = history::construct_hash_change_event;
        attribute oldURL: DomString, get = history::hash_change_old_url;
        attribute newURL: DomString, get = history::hash_change_new_url;
    }

    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary HashChangeEventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        oldURL: String = String::new(),
        newURL: String = String::new(),
    }

    interface MessageEvent : Event [type = MessageEventObject] {
        #[front]
        constructor(r#type: DomString, eventInitDict?: MessageEventInit) = message::construct_message_event;
        attribute data: AnyOrNull, get = message::data;
        attribute origin: DomString, get = message::origin;
        attribute lastEventId: DomString, get = message::last_event_id;
        attribute source: Option<EventTarget>, get = message::source;
        attribute ports: FrozenArray<MessagePort>, get = message::ports;
        // Legacy. Its `source` a window (Blink's `MessageEventSource`, a
        // port too).
        #[front]
        method initMessageEvent(
            r#type: DomString,
            bubbles?: bool,
            cancelable?: bool,
            data?: Any,
            origin?: EmptyStringDefault,
            lastEventId?: EmptyStringDefault,
            source?: Option<Window>,
            ports?: MessagePorts,
        ) = message::init_message_event;
    }

    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary MessageEventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        data: Option<AnyArg<T::Any>> = None,
        origin: String = String::new(),
        lastEventId: String = String::new(),
    }

    // `postMessage`'s options: its `transfer` the front end's to read with
    // the message.
    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary WindowPostMessageOptions {
        targetOrigin: String = "/".to_owned(),
    }

    interface MessageChannel [type = dom::MessageChannel] {
        #[front]
        constructor() = message::construct_message_channel;
        attribute port1: MessagePort, get = message::port1;
        attribute port2: MessagePort, get = message::port2;
    }

    interface MessagePort : EventTarget [type = MessagePortObject] {
        method postMessage(message: PortMessage) = message::port_post_message;
        method start() = message::start;
        method close() = message::close;
        #[front]
        attribute onmessage: EventHandler, get = handlers::port_handler::<{ event_type::MESSAGE }>, set = handlers::set_port_message_handler::<_, { event_type::MESSAGE }>;
        #[front]
        attribute onmessageerror: EventHandler, get = handlers::port_handler::<{ event_type::MESSAGEERROR }>, set = handlers::set_port_handler::<_, { event_type::MESSAGEERROR }>;
    }

    // CSSOM's: JS's only (a wasm guest has no namespaces).
    #[namespace]
    interface CSS [type = NamespaceObject] {
        #[static_method]
        method supports(property: DomString, value: DomString) -> bool = css_namespace::supports_declaration;
        #[static_method]
        method supports(conditionText: DomString) -> bool = css_namespace::supports_condition;
        #[static_method]
        method escape(ident: DomString) -> DomString = css_namespace::escape;
    }

    // CSSOM View's: what `matchMedia()` makes, an event target of its own
    // kind.
    interface MediaQueryList : EventTarget [type = MediaQueryListObject] {
        attribute media: DomString, get = media_query_list::media;
        attribute matches: bool, get = media_query_list::matches;
        #[front]
        method addListener(callback: Option<EventListener>) = media_query_list::add_listener;
        #[front]
        method removeListener(callback: Option<EventListener>) = media_query_list::remove_listener;
        #[front]
        attribute onchange: EventHandler, get = handlers::port_handler::<{ event_type::CHANGE }>, set = handlers::set_port_handler::<_, { event_type::CHANGE }>;
    }

    // What a `MediaQueryList` fires when its result changes.
    interface MediaQueryListEvent : Event [type = MediaQueryListEventObject] {
        #[front]
        constructor(r#type: DomString, eventInitDict?: MediaQueryListEventInit) = media_query_list::construct_event;
        attribute media: DomString, get = media_query_list::event_media;
        attribute matches: bool, get = media_query_list::event_matches;
    }

    // What a CSS transition fires at its element as it runs, starts, ends
    // and is canceled (css-transitions 1).
    interface TransitionEvent : Event [type = TransitionEventObject] {
        #[front]
        constructor(r#type: DomString, transitionEventInitDict?: TransitionEventInit) = animations::construct_transition_event;
        attribute propertyName: DomString, get = animations::event_name;
        attribute elapsedTime: f64, get = animations::elapsed_time;
        attribute pseudoElement: DomString, get = animations::pseudo_element;
    }

    // What a CSS animation fires at its element as it starts, repeats, ends
    // and is canceled (css-animations 1).
    interface AnimationEvent : Event [type = AnimationEventObject] {
        #[front]
        constructor(r#type: DomString, animationEventInitDict?: AnimationEventInit) = animations::construct_animation_event;
        attribute animationName: DomString, get = animations::event_name;
        attribute elapsedTime: f64, get = animations::elapsed_time;
        attribute pseudoElement: DomString, get = animations::pseudo_element;
    }

    // Web Animations' (web-animations 1): the timelines (a document's only:
    // no constructor), the effects (their members come with
    // `KeyframeEffect`'s constructor), the animations.
    interface AnimationTimeline [type = dom::DocumentTimeline] {
        attribute currentTime: Option<f64>, get = animation::timeline_current_time;
    }

    interface DocumentTimeline : AnimationTimeline [type = dom::DocumentTimeline] {}

    interface AnimationEffect [type = dom::KeyframeEffectObject] {
        method getTiming() -> DictionaryOut = effect::get_timing;
        method getComputedTiming() -> DictionaryOut = effect::get_computed_timing;
        method updateTiming(timing?: OptionalTimingArg) = effect::update_timing;
    }

    // Its keyframes are script objects (JS's only); no copy constructor.
    interface KeyframeEffect : AnimationEffect [type = dom::KeyframeEffectObject] {
        #[front]
        constructor(target: Option<Element>, keyframes: Keyframes, options?: EffectOptions) = effect::construct;
        attribute target: Option<Element>, get = effect::target, set = effect::set_target;
        attribute pseudoElement: Option<DomString>, get = effect::pseudo_element, set = effect::set_pseudo_element;
        attribute composite: DomString, get = effect::composite, set = effect::set_composite;
        method getKeyframes() -> Records = effect::get_keyframes;
        method setKeyframes(keyframes: Keyframes) = effect::set_keyframes;
    }

    interface Animation : EventTarget [type = AnimationObject] {
        #[front]
        constructor(effect?: Option<KeyframeEffect>, timeline?: TimelineArg) = animation::construct;
        attribute id: DomString, get = animation::id, set = animation::set_id;
        attribute effect: Option<KeyframeEffect>, get = animation::effect, set = animation::set_effect;
        attribute timeline: Option<DocumentTimeline>, get = animation::timeline, set = animation::set_timeline;
        attribute startTime: Option<f64>, get = animation::start_time, set = animation::set_start_time;
        attribute currentTime: Option<f64>, get = animation::current_time, set = animation::set_current_time;
        attribute playbackRate: f64, get = animation::playback_rate, set = animation::set_playback_rate;
        attribute playState: DomString, get = animation::play_state;
        attribute replaceState: DomString, get = animation::replace_state;
        attribute pending: bool, get = animation::pending;
        // The same promise until the animation replaces it.
        attribute ready: PromiseOfAnimation, get = animation::ready;
        attribute finished: PromiseOfAnimation, get = animation::finished;
        #[front]
        attribute onfinish: EventHandler, get = handlers::port_handler::<{ event_type::FINISH }>, set = handlers::set_port_handler::<_, { event_type::FINISH }>;
        #[front]
        attribute oncancel: EventHandler, get = handlers::port_handler::<{ event_type::CANCEL }>, set = handlers::set_port_handler::<_, { event_type::CANCEL }>;
        #[front]
        attribute onremove: EventHandler, get = handlers::port_handler::<{ event_type::REMOVE }>, set = handlers::set_port_handler::<_, { event_type::REMOVE }>;
        method cancel() = animation::cancel;
        method finish() = animation::finish;
        method play() = animation::play;
        method pause() = animation::pause;
        method updatePlaybackRate(playbackRate: f64) = animation::update_playback_rate;
        method reverse() = animation::reverse;
        method persist() = animation::persist;
        #[ce_reactions]
        method commitStyles() = animation::commit_styles;
    }

    interface CSSAnimation : Animation [type = AnimationObject] {
        attribute animationName: DomString, get = animation::animation_name;
    }

    interface CSSTransition : Animation [type = AnimationObject] {
        attribute transitionProperty: DomString, get = animation::transition_property;
    }

    // An animation's `finish` and `cancel`.
    interface AnimationPlaybackEvent : Event [type = AnimationPlaybackEventObject] {
        #[front]
        constructor(r#type: DomString, eventInitDict?: AnimationPlaybackEventInit) = animation::construct_playback_event;
        attribute currentTime: Option<f64>, get = animation::event_current_time;
        attribute timelineTime: Option<f64>, get = animation::event_timeline_time;
    }

    dictionary GetAnimationsOptions {
        subtree: bool = false,
    }

    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary AnimationPlaybackEventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        currentTime: Option<f64> = None,
        timelineTime: Option<f64> = None,
    }

    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary TransitionEventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        elapsedTime: f64 = 0.0,
        propertyName: String = String::new(),
        pseudoElement: String = String::new(),
    }

    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary AnimationEventInit {
        animationName: String = String::new(),
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        elapsedTime: f64 = 0.0,
        pseudoElement: String = String::new(),
    }

    // CSSOM View's: the enumerations' members as strings, checked.
    dictionary ScrollToOptions {
        behavior: String = String::from("auto"),
        left: Option<f64> = None,
        top: Option<f64> = None,
    }

    dictionary ScrollIntoViewOptions {
        behavior: String = String::from("auto"),
        block: String = String::from("start"),
        container: String = String::from("all"),
        inline: String = String::from("nearest"),
    }

    dictionary MediaQueryListEventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        matches: bool = false,
        media: String = String::new(),
    }

    // UI Events'.
    interface UIEvent : Event [type = UiEventObject] {
        #[front]
        constructor(r#type: DomString, eventInitDict?: UIEventInit) = ui_event::construct_ui_event;
        attribute view: Option<Window>, get = ui_event::view;
        attribute detail: i32, get = ui_event::detail;
        // WebIDL `unsigned long`, legacy.
        attribute which: u32, get = ui_event::which;
        // Legacy: for `createEvent`'s events.
        method initUIEvent(r#type: DomString, bubbles?: bool, cancelable?: bool, view?: Option<Window>, detail?: i32) =
            ui_event::init_ui_event;
    }

    // WebIDL's names, which the table reads its members by, in its order:
    // the inherited dictionaries' first. `view` is the front end's handle
    // (`ViewObject`), as `relatedTarget` (`RelatedTarget`) below.
    dictionary UIEventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        detail: i32 = 0,
        view: Option<ViewObject<T::Node>> = None,
        which: u32 = 0,
    }

    // With CSSOM View's coordinates, Pointer Lock's `movementX` and the
    // legacy `layerX` and `fromElement`.
    interface MouseEvent : UIEvent [type = MouseEventObject] {
        #[front]
        constructor(r#type: DomString, eventInitDict?: MouseEventInit) = ui_event::construct_mouse_event;
        attribute screenX: f64, get = ui_event::screen_x;
        attribute screenY: f64, get = ui_event::screen_y;
        attribute clientX: f64, get = ui_event::client_x;
        attribute clientY: f64, get = ui_event::client_y;
        attribute ctrlKey: bool, get = ui_event::ctrl_key;
        attribute shiftKey: bool, get = ui_event::shift_key;
        attribute altKey: bool, get = ui_event::alt_key;
        attribute metaKey: bool, get = ui_event::meta_key;
        // WebIDL `short`.
        attribute button: i32, get = ui_event::button;
        // WebIDL `unsigned short`.
        attribute buttons: u16, get = ui_event::buttons;
        attribute relatedTarget: Option<EventTarget>, get = ui_event::related_target;
        method getModifierState(keyArg: DomString) -> bool = ui_event::get_modifier_state;
        attribute pageX: f64, get = ui_event::page_x;
        attribute pageY: f64, get = ui_event::page_y;
        attribute x: f64, get = ui_event::client_x;
        attribute y: f64, get = ui_event::client_y;
        attribute offsetX: f64, get = ui_event::offset_x;
        attribute offsetY: f64, get = ui_event::offset_y;
        // Legacy, no spec: WebIDL `long`s, as Chrome's.
        attribute layerX: i32, get = ui_event::layer_x;
        attribute layerY: i32, get = ui_event::layer_y;
        // Legacy, no spec: Chrome's `Node?`s.
        attribute fromElement: Option<Node>, get = ui_event::from_element;
        attribute toElement: Option<Node>, get = ui_event::to_element;
        attribute movementX: f64, get = ui_event::movement_x;
        attribute movementY: f64, get = ui_event::movement_y;
        // Legacy: for `createEvent`'s events. WebIDL `long`s, and a
        // `short` button.
        method initMouseEvent(
            r#type: DomString,
            bubbles?: bool,
            cancelable?: bool,
            view?: Option<Window>,
            detail?: i32,
            screenX?: i32,
            screenY?: i32,
            clientX?: i32,
            clientY?: i32,
            ctrlKey?: bool,
            altKey?: bool,
            shiftKey?: bool,
            metaKey?: bool,
            button?: i32,
            relatedTarget?: Option<EventTarget>,
        ) = ui_event::init_mouse_event;
    }

    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary MouseEventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        detail: i32 = 0,
        view: Option<ViewObject<T::Node>> = None,
        which: u32 = 0,
        altKey: bool = false,
        ctrlKey: bool = false,
        metaKey: bool = false,
        modifierAltGraph: bool = false,
        modifierCapsLock: bool = false,
        modifierFn: bool = false,
        modifierFnLock: bool = false,
        modifierHyper: bool = false,
        modifierNumLock: bool = false,
        modifierScrollLock: bool = false,
        modifierSuper: bool = false,
        modifierSymbol: bool = false,
        modifierSymbolLock: bool = false,
        shiftKey: bool = false,
        button: i16 = 0,
        buttons: u16 = 0,
        clientX: f64 = 0.0,
        clientY: f64 = 0.0,
        movementX: f64 = 0.0,
        movementY: f64 = 0.0,
        relatedTarget: Option<RelatedTarget<T::Node>> = None,
        screenX: f64 = 0.0,
        screenY: f64 = 0.0,
    }

    // With Chrome's `wheelDelta`s (`mousewheel`'s, its legacy type).
    interface WheelEvent : MouseEvent [type = WheelEventObject] {
        const DOM_DELTA_PIXEL: u32 = 0;
        const DOM_DELTA_LINE: u32 = 1;
        const DOM_DELTA_PAGE: u32 = 2;
        #[front]
        constructor(r#type: DomString, eventInitDict?: WheelEventInit) = ui_event::construct_wheel_event;
        attribute deltaX: f64, get = ui_event::delta_x;
        attribute deltaY: f64, get = ui_event::delta_y;
        attribute deltaZ: f64, get = ui_event::delta_z;
        // WebIDL `unsigned long`.
        attribute deltaMode: u32, get = ui_event::delta_mode;
        // WebIDL `long`s.
        attribute wheelDelta: i32, get = ui_event::wheel_delta;
        attribute wheelDeltaX: i32, get = ui_event::wheel_delta_x;
        attribute wheelDeltaY: i32, get = ui_event::wheel_delta_y;
    }

    // `MouseEventInit`'s members, then its own.
    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary WheelEventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        detail: i32 = 0,
        view: Option<ViewObject<T::Node>> = None,
        which: u32 = 0,
        altKey: bool = false,
        ctrlKey: bool = false,
        metaKey: bool = false,
        modifierAltGraph: bool = false,
        modifierCapsLock: bool = false,
        modifierFn: bool = false,
        modifierFnLock: bool = false,
        modifierHyper: bool = false,
        modifierNumLock: bool = false,
        modifierScrollLock: bool = false,
        modifierSuper: bool = false,
        modifierSymbol: bool = false,
        modifierSymbolLock: bool = false,
        shiftKey: bool = false,
        button: i16 = 0,
        buttons: u16 = 0,
        clientX: f64 = 0.0,
        clientY: f64 = 0.0,
        movementX: f64 = 0.0,
        movementY: f64 = 0.0,
        relatedTarget: Option<RelatedTarget<T::Node>> = None,
        screenX: f64 = 0.0,
        screenY: f64 = 0.0,
        deltaMode: u32 = 0,
        deltaX: f64 = 0.0,
        deltaY: f64 = 0.0,
        deltaZ: f64 = 0.0,
    }

    // The focus events'.
    interface FocusEvent : UIEvent [type = FocusEventObject] {
        #[front]
        constructor(r#type: DomString, eventInitDict?: FocusEventInit) = focus::construct_focus_event;
        attribute relatedTarget: Option<EventTarget>, get = focus::focus_related_target;
    }

    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary FocusEventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        detail: i32 = 0,
        view: Option<ViewObject<T::Node>> = None,
        which: u32 = 0,
        relatedTarget: Option<RelatedTarget<T::Node>> = None,
    }

    // `focus()`'s; `focusVisible` with no default (the window's then).
    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary FocusOptions {
        focusVisible: Option<bool> = None,
        preventScroll: bool = false,
    }

    // With the legacy `charCode` and `keyCode`.
    interface KeyboardEvent : UIEvent [type = KeyboardEventObject] {
        const DOM_KEY_LOCATION_STANDARD: u32 = 0;
        const DOM_KEY_LOCATION_LEFT: u32 = 1;
        const DOM_KEY_LOCATION_RIGHT: u32 = 2;
        const DOM_KEY_LOCATION_NUMPAD: u32 = 3;
        #[front]
        constructor(r#type: DomString, eventInitDict?: KeyboardEventInit) = ui_event::construct_keyboard_event;
        attribute key: DomString, get = ui_event::key;
        attribute code: DomString, get = ui_event::code;
        // WebIDL `unsigned long`.
        attribute location: u32, get = ui_event::location;
        attribute ctrlKey: bool, get = ui_event::ctrl_key;
        attribute shiftKey: bool, get = ui_event::shift_key;
        attribute altKey: bool, get = ui_event::alt_key;
        attribute metaKey: bool, get = ui_event::meta_key;
        attribute repeat: bool, get = ui_event::repeat;
        attribute isComposing: bool, get = ui_event::is_composing;
        method getModifierState(keyArg: DomString) -> bool = ui_event::get_modifier_state;
        // Legacy: for `createEvent`'s events.
        method initKeyboardEvent(
            r#type: DomString,
            bubbles?: bool,
            cancelable?: bool,
            view?: Option<Window>,
            keyArg?: EmptyStringDefault,
            // WebIDL `unsigned long`.
            locationArg?: u32,
            ctrlKey?: bool,
            altKey?: bool,
            shiftKey?: bool,
            metaKey?: bool,
        ) = ui_event::init_keyboard_event;
        attribute charCode: u32, get = ui_event::char_code;
        attribute keyCode: u32, get = ui_event::key_code;
    }

    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary KeyboardEventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        detail: i32 = 0,
        view: Option<ViewObject<T::Node>> = None,
        which: u32 = 0,
        altKey: bool = false,
        ctrlKey: bool = false,
        metaKey: bool = false,
        modifierAltGraph: bool = false,
        modifierCapsLock: bool = false,
        modifierFn: bool = false,
        modifierFnLock: bool = false,
        modifierHyper: bool = false,
        modifierNumLock: bool = false,
        modifierScrollLock: bool = false,
        modifierSuper: bool = false,
        modifierSymbol: bool = false,
        modifierSymbolLock: bool = false,
        shiftKey: bool = false,
        charCode: u32 = 0,
        code: String = String::new(),
        isComposing: bool = false,
        key: String = String::new(),
        keyCode: u32 = 0,
        location: u32 = 0,
        repeat: bool = false,
    }

    // What editing fires (`beforeinput`, `input`); no `dataTransfer` or
    // `getTargetRanges()` (no `DataTransfer`, `StaticRange` or editing
    // yet).
    interface InputEvent : UIEvent [type = InputEventObject] {
        #[front]
        constructor(r#type: DomString, eventInitDict?: InputEventInit) = ui_event::construct_input_event;
        attribute data: Option<DomString>, get = ui_event::input_data;
        attribute isComposing: bool, get = ui_event::input_is_composing;
        attribute inputType: DomString, get = ui_event::input_type;
    }

    // The legacy `textInput`'s (Blink's `TextEvent`): no constructor,
    // `createEvent('TextEvent')`'s.
    interface TextEvent : UIEvent [type = TextEventObject] {
        attribute data: DomString, get = ui_event::text_data;
        method initTextEvent(
            r#type: DomString,
            bubbles?: bool,
            cancelable?: bool,
            view?: Option<Window>,
            data?: Option<DomString>
        ) = ui_event::init_text_event;
    }

    // No `dataTransfer` or `targetRanges` yet.
    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary InputEventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        detail: i32 = 0,
        view: Option<ViewObject<T::Node>> = None,
        which: u32 = 0,
        data: Option<String> = None,
        inputType: String = String::new(),
        isComposing: bool = false,
    }

    // No constructor yet: its dictionary has an `any` member.
    interface ErrorEvent : Event [type = ErrorEventObject] {
        #[front]
        constructor(r#type: DomString, eventInitDict?: ErrorEventInit) = event::construct_error_event;
        attribute message: DomString, get = event::message;
        attribute filename: DomString, get = event::filename;
        // WebIDL `unsigned long`.
        attribute lineno: u32, get = event::lineno;
        attribute colno: u32, get = event::colno;
        attribute error: Any, get = event::error;
    }

    dictionary ErrorEventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        colno: u32 = 0,
        error: Option<AnyArg<T::Any>> = None,
        filename: String = String::new(),
        lineno: u32 = 0,
        message: String = String::new(),
    }

    // No constructor yet: its dictionary has `any` members.
    interface PromiseRejectionEvent : Event [type = PromiseRejectionEventObject] {
        attribute promise: Any, get = event::promise;
        attribute reason: Any, get = event::reason;
    }

    #[global]
    interface Window : EventTarget [type = dom::Window] {
        #[unforgeable]
        #[cross_origin]
        attribute window: Window, get = window::window;
        #[replaceable]
        #[cross_origin]
        attribute self: Window, get = window::window;
        #[replaceable]
        #[cross_origin]
        attribute frames: Window, get = window::window;
        // Its child browsing contexts' windows, in tree order.
        #[replaceable]
        #[cross_origin]
        attribute length: u32, get = frame::length;
        #[indexed(getter)]
        method indexedGetter(index: u32) -> Option<Window> = frame::child_window;
        #[unforgeable]
        #[cross_origin]
        attribute top: Option<Window>, get = frame::top;
        #[replaceable]
        #[cross_origin]
        attribute opener: Option<Window>, get = frame::opener;
        #[calls_script]
        #[cross_origin]
        method focus() = focus::window_focus;
        // HTML's: does nothing.
        #[cross_origin]
        method blur() = frame::blur;
        #[replaceable]
        #[cross_origin]
        attribute parent: Option<Window>, get = frame::parent;
        attribute frameElement: Option<Element>, get = frame::frame_element;
        // HTML's legacy current event.
        #[replaceable]
        attribute event: EventOrUndefined, get = window::event;
        #[unforgeable]
        attribute document: Document, get = window::document;

        // HTML's `GlobalEventHandlers`.
        #[front]
        attribute onabort: EventHandler, get = handlers::window_handler::<{ event_type::ABORT }>, set = handlers::set_window_handler::<_, { event_type::ABORT }>;
        #[front]
        attribute onanimationcancel: EventHandler, get = handlers::window_handler::<{ event_type::ANIMATIONCANCEL }>, set = handlers::set_window_handler::<_, { event_type::ANIMATIONCANCEL }>;
        #[front]
        attribute onanimationend: EventHandler, get = handlers::window_handler::<{ event_type::ANIMATIONEND }>, set = handlers::set_window_handler::<_, { event_type::ANIMATIONEND }>;
        #[front]
        attribute onanimationiteration: EventHandler, get = handlers::window_handler::<{ event_type::ANIMATIONITERATION }>, set = handlers::set_window_handler::<_, { event_type::ANIMATIONITERATION }>;
        #[front]
        attribute onanimationstart: EventHandler, get = handlers::window_handler::<{ event_type::ANIMATIONSTART }>, set = handlers::set_window_handler::<_, { event_type::ANIMATIONSTART }>;
        #[front]
        attribute onauxclick: EventHandler, get = handlers::window_handler::<{ event_type::AUXCLICK }>, set = handlers::set_window_handler::<_, { event_type::AUXCLICK }>;
        #[front]
        attribute onbeforeinput: EventHandler, get = handlers::window_handler::<{ event_type::BEFOREINPUT }>, set = handlers::set_window_handler::<_, { event_type::BEFOREINPUT }>;
        #[front]
        attribute onbeforematch: EventHandler, get = handlers::window_handler::<{ event_type::BEFOREMATCH }>, set = handlers::set_window_handler::<_, { event_type::BEFOREMATCH }>;
        #[front]
        attribute onbeforetoggle: EventHandler, get = handlers::window_handler::<{ event_type::BEFORETOGGLE }>, set = handlers::set_window_handler::<_, { event_type::BEFORETOGGLE }>;
        #[front]
        attribute onblur: EventHandler, get = handlers::window_handler::<{ event_type::BLUR }>, set = handlers::set_window_handler::<_, { event_type::BLUR }>;
        #[front]
        attribute oncancel: EventHandler, get = handlers::window_handler::<{ event_type::CANCEL }>, set = handlers::set_window_handler::<_, { event_type::CANCEL }>;
        #[front]
        attribute oncanplay: EventHandler, get = handlers::window_handler::<{ event_type::CANPLAY }>, set = handlers::set_window_handler::<_, { event_type::CANPLAY }>;
        #[front]
        attribute oncanplaythrough: EventHandler, get = handlers::window_handler::<{ event_type::CANPLAYTHROUGH }>, set = handlers::set_window_handler::<_, { event_type::CANPLAYTHROUGH }>;
        #[front]
        attribute onchange: EventHandler, get = handlers::window_handler::<{ event_type::CHANGE }>, set = handlers::set_window_handler::<_, { event_type::CHANGE }>;
        #[front]
        attribute onclick: EventHandler, get = handlers::window_handler::<{ event_type::CLICK }>, set = handlers::set_window_handler::<_, { event_type::CLICK }>;
        #[front]
        attribute onclose: EventHandler, get = handlers::window_handler::<{ event_type::CLOSE }>, set = handlers::set_window_handler::<_, { event_type::CLOSE }>;
        #[front]
        attribute oncommand: EventHandler, get = handlers::window_handler::<{ event_type::COMMAND }>, set = handlers::set_window_handler::<_, { event_type::COMMAND }>;
        #[front]
        attribute oncontentvisibilityautostatechange: EventHandler, get = handlers::window_handler::<{ event_type::CONTENTVISIBILITYAUTOSTATECHANGE }>, set = handlers::set_window_handler::<_, { event_type::CONTENTVISIBILITYAUTOSTATECHANGE }>;
        #[front]
        attribute oncontextlost: EventHandler, get = handlers::window_handler::<{ event_type::CONTEXTLOST }>, set = handlers::set_window_handler::<_, { event_type::CONTEXTLOST }>;
        #[front]
        attribute oncontextmenu: EventHandler, get = handlers::window_handler::<{ event_type::CONTEXTMENU }>, set = handlers::set_window_handler::<_, { event_type::CONTEXTMENU }>;
        #[front]
        attribute oncontextrestored: EventHandler, get = handlers::window_handler::<{ event_type::CONTEXTRESTORED }>, set = handlers::set_window_handler::<_, { event_type::CONTEXTRESTORED }>;
        #[front]
        attribute oncuechange: EventHandler, get = handlers::window_handler::<{ event_type::CUECHANGE }>, set = handlers::set_window_handler::<_, { event_type::CUECHANGE }>;
        #[front]
        attribute ondblclick: EventHandler, get = handlers::window_handler::<{ event_type::DBLCLICK }>, set = handlers::set_window_handler::<_, { event_type::DBLCLICK }>;
        #[front]
        attribute ondrag: EventHandler, get = handlers::window_handler::<{ event_type::DRAG }>, set = handlers::set_window_handler::<_, { event_type::DRAG }>;
        #[front]
        attribute ondragend: EventHandler, get = handlers::window_handler::<{ event_type::DRAGEND }>, set = handlers::set_window_handler::<_, { event_type::DRAGEND }>;
        #[front]
        attribute ondragenter: EventHandler, get = handlers::window_handler::<{ event_type::DRAGENTER }>, set = handlers::set_window_handler::<_, { event_type::DRAGENTER }>;
        #[front]
        attribute ondragleave: EventHandler, get = handlers::window_handler::<{ event_type::DRAGLEAVE }>, set = handlers::set_window_handler::<_, { event_type::DRAGLEAVE }>;
        #[front]
        attribute ondragover: EventHandler, get = handlers::window_handler::<{ event_type::DRAGOVER }>, set = handlers::set_window_handler::<_, { event_type::DRAGOVER }>;
        #[front]
        attribute ondragstart: EventHandler, get = handlers::window_handler::<{ event_type::DRAGSTART }>, set = handlers::set_window_handler::<_, { event_type::DRAGSTART }>;
        #[front]
        attribute ondrop: EventHandler, get = handlers::window_handler::<{ event_type::DROP }>, set = handlers::set_window_handler::<_, { event_type::DROP }>;
        #[front]
        attribute ondurationchange: EventHandler, get = handlers::window_handler::<{ event_type::DURATIONCHANGE }>, set = handlers::set_window_handler::<_, { event_type::DURATIONCHANGE }>;
        #[front]
        attribute onemptied: EventHandler, get = handlers::window_handler::<{ event_type::EMPTIED }>, set = handlers::set_window_handler::<_, { event_type::EMPTIED }>;
        #[front]
        attribute onended: EventHandler, get = handlers::window_handler::<{ event_type::ENDED }>, set = handlers::set_window_handler::<_, { event_type::ENDED }>;
        #[front]
        attribute onerror: EventHandler, get = handlers::window_handler::<{ event_type::ERROR }>, set = handlers::set_window_handler::<_, { event_type::ERROR }>;
        #[front]
        attribute onfocus: EventHandler, get = handlers::window_handler::<{ event_type::FOCUS }>, set = handlers::set_window_handler::<_, { event_type::FOCUS }>;
        #[front]
        attribute onformdata: EventHandler, get = handlers::window_handler::<{ event_type::FORMDATA }>, set = handlers::set_window_handler::<_, { event_type::FORMDATA }>;
        #[front]
        attribute oninput: EventHandler, get = handlers::window_handler::<{ event_type::INPUT }>, set = handlers::set_window_handler::<_, { event_type::INPUT }>;
        #[front]
        attribute oninvalid: EventHandler, get = handlers::window_handler::<{ event_type::INVALID }>, set = handlers::set_window_handler::<_, { event_type::INVALID }>;
        #[front]
        attribute onkeydown: EventHandler, get = handlers::window_handler::<{ event_type::KEYDOWN }>, set = handlers::set_window_handler::<_, { event_type::KEYDOWN }>;
        #[front]
        attribute onkeypress: EventHandler, get = handlers::window_handler::<{ event_type::KEYPRESS }>, set = handlers::set_window_handler::<_, { event_type::KEYPRESS }>;
        #[front]
        attribute onkeyup: EventHandler, get = handlers::window_handler::<{ event_type::KEYUP }>, set = handlers::set_window_handler::<_, { event_type::KEYUP }>;
        #[front]
        attribute onload: EventHandler, get = handlers::window_handler::<{ event_type::LOAD }>, set = handlers::set_window_handler::<_, { event_type::LOAD }>;
        #[front]
        attribute onloadeddata: EventHandler, get = handlers::window_handler::<{ event_type::LOADEDDATA }>, set = handlers::set_window_handler::<_, { event_type::LOADEDDATA }>;
        #[front]
        attribute onloadedmetadata: EventHandler, get = handlers::window_handler::<{ event_type::LOADEDMETADATA }>, set = handlers::set_window_handler::<_, { event_type::LOADEDMETADATA }>;
        #[front]
        attribute onloadstart: EventHandler, get = handlers::window_handler::<{ event_type::LOADSTART }>, set = handlers::set_window_handler::<_, { event_type::LOADSTART }>;
        #[front]
        attribute onmousedown: EventHandler, get = handlers::window_handler::<{ event_type::MOUSEDOWN }>, set = handlers::set_window_handler::<_, { event_type::MOUSEDOWN }>;
        #[front]
        attribute onmouseenter: EventHandler, get = handlers::window_handler::<{ event_type::MOUSEENTER }>, set = handlers::set_window_handler::<_, { event_type::MOUSEENTER }>;
        #[front]
        attribute onmouseleave: EventHandler, get = handlers::window_handler::<{ event_type::MOUSELEAVE }>, set = handlers::set_window_handler::<_, { event_type::MOUSELEAVE }>;
        #[front]
        attribute onmousemove: EventHandler, get = handlers::window_handler::<{ event_type::MOUSEMOVE }>, set = handlers::set_window_handler::<_, { event_type::MOUSEMOVE }>;
        #[front]
        attribute onmouseout: EventHandler, get = handlers::window_handler::<{ event_type::MOUSEOUT }>, set = handlers::set_window_handler::<_, { event_type::MOUSEOUT }>;
        #[front]
        attribute onmouseover: EventHandler, get = handlers::window_handler::<{ event_type::MOUSEOVER }>, set = handlers::set_window_handler::<_, { event_type::MOUSEOVER }>;
        #[front]
        attribute onmouseup: EventHandler, get = handlers::window_handler::<{ event_type::MOUSEUP }>, set = handlers::set_window_handler::<_, { event_type::MOUSEUP }>;
        #[front]
        attribute onmousewheel: EventHandler, get = handlers::window_handler::<{ event_type::MOUSEWHEEL }>, set = handlers::set_window_handler::<_, { event_type::MOUSEWHEEL }>;
        #[front]
        attribute onpause: EventHandler, get = handlers::window_handler::<{ event_type::PAUSE }>, set = handlers::set_window_handler::<_, { event_type::PAUSE }>;
        #[front]
        attribute onplay: EventHandler, get = handlers::window_handler::<{ event_type::PLAY }>, set = handlers::set_window_handler::<_, { event_type::PLAY }>;
        #[front]
        attribute onplaying: EventHandler, get = handlers::window_handler::<{ event_type::PLAYING }>, set = handlers::set_window_handler::<_, { event_type::PLAYING }>;
        #[front]
        attribute onprogress: EventHandler, get = handlers::window_handler::<{ event_type::PROGRESS }>, set = handlers::set_window_handler::<_, { event_type::PROGRESS }>;
        #[front]
        attribute onratechange: EventHandler, get = handlers::window_handler::<{ event_type::RATECHANGE }>, set = handlers::set_window_handler::<_, { event_type::RATECHANGE }>;
        #[front]
        attribute onreset: EventHandler, get = handlers::window_handler::<{ event_type::RESET }>, set = handlers::set_window_handler::<_, { event_type::RESET }>;
        #[front]
        attribute onresize: EventHandler, get = handlers::window_handler::<{ event_type::RESIZE }>, set = handlers::set_window_handler::<_, { event_type::RESIZE }>;
        #[front]
        attribute onscroll: EventHandler, get = handlers::window_handler::<{ event_type::SCROLL }>, set = handlers::set_window_handler::<_, { event_type::SCROLL }>;
        #[front]
        attribute onscrollend: EventHandler, get = handlers::window_handler::<{ event_type::SCROLLEND }>, set = handlers::set_window_handler::<_, { event_type::SCROLLEND }>;
        #[front]
        attribute onsecuritypolicyviolation: EventHandler, get = handlers::window_handler::<{ event_type::SECURITYPOLICYVIOLATION }>, set = handlers::set_window_handler::<_, { event_type::SECURITYPOLICYVIOLATION }>;
        #[front]
        attribute onseeked: EventHandler, get = handlers::window_handler::<{ event_type::SEEKED }>, set = handlers::set_window_handler::<_, { event_type::SEEKED }>;
        #[front]
        attribute onseeking: EventHandler, get = handlers::window_handler::<{ event_type::SEEKING }>, set = handlers::set_window_handler::<_, { event_type::SEEKING }>;
        #[front]
        attribute onselect: EventHandler, get = handlers::window_handler::<{ event_type::SELECT }>, set = handlers::set_window_handler::<_, { event_type::SELECT }>;
        #[front]
        attribute onselectionchange: EventHandler, get = handlers::window_handler::<{ event_type::SELECTIONCHANGE }>, set = handlers::set_window_handler::<_, { event_type::SELECTIONCHANGE }>;
        #[front]
        attribute onselectstart: EventHandler, get = handlers::window_handler::<{ event_type::SELECTSTART }>, set = handlers::set_window_handler::<_, { event_type::SELECTSTART }>;
        #[front]
        attribute onslotchange: EventHandler, get = handlers::window_handler::<{ event_type::SLOTCHANGE }>, set = handlers::set_window_handler::<_, { event_type::SLOTCHANGE }>;
        #[front]
        attribute onstalled: EventHandler, get = handlers::window_handler::<{ event_type::STALLED }>, set = handlers::set_window_handler::<_, { event_type::STALLED }>;
        #[front]
        attribute onsubmit: EventHandler, get = handlers::window_handler::<{ event_type::SUBMIT }>, set = handlers::set_window_handler::<_, { event_type::SUBMIT }>;
        #[front]
        attribute onsuspend: EventHandler, get = handlers::window_handler::<{ event_type::SUSPEND }>, set = handlers::set_window_handler::<_, { event_type::SUSPEND }>;
        #[front]
        attribute ontimeupdate: EventHandler, get = handlers::window_handler::<{ event_type::TIMEUPDATE }>, set = handlers::set_window_handler::<_, { event_type::TIMEUPDATE }>;
        #[front]
        attribute ontoggle: EventHandler, get = handlers::window_handler::<{ event_type::TOGGLE }>, set = handlers::set_window_handler::<_, { event_type::TOGGLE }>;
        #[front]
        attribute ontransitioncancel: EventHandler, get = handlers::window_handler::<{ event_type::TRANSITIONCANCEL }>, set = handlers::set_window_handler::<_, { event_type::TRANSITIONCANCEL }>;
        #[front]
        attribute ontransitionend: EventHandler, get = handlers::window_handler::<{ event_type::TRANSITIONEND }>, set = handlers::set_window_handler::<_, { event_type::TRANSITIONEND }>;
        #[front]
        attribute ontransitionrun: EventHandler, get = handlers::window_handler::<{ event_type::TRANSITIONRUN }>, set = handlers::set_window_handler::<_, { event_type::TRANSITIONRUN }>;
        #[front]
        attribute ontransitionstart: EventHandler, get = handlers::window_handler::<{ event_type::TRANSITIONSTART }>, set = handlers::set_window_handler::<_, { event_type::TRANSITIONSTART }>;
        #[front]
        attribute onvolumechange: EventHandler, get = handlers::window_handler::<{ event_type::VOLUMECHANGE }>, set = handlers::set_window_handler::<_, { event_type::VOLUMECHANGE }>;
        #[front]
        attribute onwaiting: EventHandler, get = handlers::window_handler::<{ event_type::WAITING }>, set = handlers::set_window_handler::<_, { event_type::WAITING }>;
        #[front]
        attribute onwebkitanimationend: EventHandler, get = handlers::window_handler::<{ event_type::WEBKIT_ANIMATION_END }>, set = handlers::set_window_handler::<_, { event_type::WEBKIT_ANIMATION_END }>;
        #[front]
        attribute onwebkitanimationiteration: EventHandler, get = handlers::window_handler::<{ event_type::WEBKIT_ANIMATION_ITERATION }>, set = handlers::set_window_handler::<_, { event_type::WEBKIT_ANIMATION_ITERATION }>;
        #[front]
        attribute onwebkitanimationstart: EventHandler, get = handlers::window_handler::<{ event_type::WEBKIT_ANIMATION_START }>, set = handlers::set_window_handler::<_, { event_type::WEBKIT_ANIMATION_START }>;
        #[front]
        attribute onwebkittransitionend: EventHandler, get = handlers::window_handler::<{ event_type::WEBKIT_TRANSITION_END }>, set = handlers::set_window_handler::<_, { event_type::WEBKIT_TRANSITION_END }>;
        #[front]
        attribute onwheel: EventHandler, get = handlers::window_handler::<{ event_type::WHEEL }>, set = handlers::set_window_handler::<_, { event_type::WHEEL }>;

        // HTML's `WindowEventHandlers`.
        #[front]
        attribute onafterprint: EventHandler, get = handlers::window_handler::<{ event_type::AFTERPRINT }>, set = handlers::set_window_handler::<_, { event_type::AFTERPRINT }>;
        #[front]
        attribute onbeforeprint: EventHandler, get = handlers::window_handler::<{ event_type::BEFOREPRINT }>, set = handlers::set_window_handler::<_, { event_type::BEFOREPRINT }>;
        #[front]
        attribute onbeforeunload: EventHandler, get = handlers::window_handler::<{ event_type::BEFOREUNLOAD }>, set = handlers::set_window_handler::<_, { event_type::BEFOREUNLOAD }>;
        #[front]
        attribute onhashchange: EventHandler, get = handlers::window_handler::<{ event_type::HASHCHANGE }>, set = handlers::set_window_handler::<_, { event_type::HASHCHANGE }>;
        #[front]
        attribute onlanguagechange: EventHandler, get = handlers::window_handler::<{ event_type::LANGUAGECHANGE }>, set = handlers::set_window_handler::<_, { event_type::LANGUAGECHANGE }>;
        #[front]
        attribute onmessage: EventHandler, get = handlers::window_handler::<{ event_type::MESSAGE }>, set = handlers::set_window_handler::<_, { event_type::MESSAGE }>;
        #[front]
        attribute onmessageerror: EventHandler, get = handlers::window_handler::<{ event_type::MESSAGEERROR }>, set = handlers::set_window_handler::<_, { event_type::MESSAGEERROR }>;
        #[front]
        attribute onoffline: EventHandler, get = handlers::window_handler::<{ event_type::OFFLINE }>, set = handlers::set_window_handler::<_, { event_type::OFFLINE }>;
        #[front]
        attribute ononline: EventHandler, get = handlers::window_handler::<{ event_type::ONLINE }>, set = handlers::set_window_handler::<_, { event_type::ONLINE }>;
        #[front]
        attribute onpagehide: EventHandler, get = handlers::window_handler::<{ event_type::PAGEHIDE }>, set = handlers::set_window_handler::<_, { event_type::PAGEHIDE }>;
        #[front]
        attribute onpagereveal: EventHandler, get = handlers::window_handler::<{ event_type::PAGEREVEAL }>, set = handlers::set_window_handler::<_, { event_type::PAGEREVEAL }>;
        #[front]
        attribute onpageshow: EventHandler, get = handlers::window_handler::<{ event_type::PAGESHOW }>, set = handlers::set_window_handler::<_, { event_type::PAGESHOW }>;
        #[front]
        attribute onpageswap: EventHandler, get = handlers::window_handler::<{ event_type::PAGESWAP }>, set = handlers::set_window_handler::<_, { event_type::PAGESWAP }>;
        #[front]
        attribute onpopstate: EventHandler, get = handlers::window_handler::<{ event_type::POPSTATE }>, set = handlers::set_window_handler::<_, { event_type::POPSTATE }>;
        #[front]
        attribute onrejectionhandled: EventHandler, get = handlers::window_handler::<{ event_type::REJECTIONHANDLED }>, set = handlers::set_window_handler::<_, { event_type::REJECTIONHANDLED }>;
        #[front]
        attribute onstorage: EventHandler, get = handlers::window_handler::<{ event_type::STORAGE }>, set = handlers::set_window_handler::<_, { event_type::STORAGE }>;
        #[front]
        attribute onunhandledrejection: EventHandler, get = handlers::window_handler::<{ event_type::UNHANDLEDREJECTION }>, set = handlers::set_window_handler::<_, { event_type::UNHANDLEDREJECTION }>;
        #[front]
        attribute onunload: EventHandler, get = handlers::window_handler::<{ event_type::UNLOAD }>, set = handlers::set_window_handler::<_, { event_type::UNLOAD }>;
        // `[PutForwards=href]`.
        #[unforgeable]
        #[front]
        #[cross_origin(getter, setter)]
        attribute location: Location, get = window::location, set: NavigationUrl = window::set_location;
        attribute history: History, get = history::window_history;
        attribute navigator: Navigator, get = navigator::window_navigator;
        attribute customElements: CustomElementRegistry, get = custom_elements::registry;
        attribute crypto: Crypto, get = crypto::window_crypto;
        #[front]
        method setTimeout(handler: TimerHandler, timeout?: i32, arguments?: Rest) -> i32 =
            timer::set_timeout;
        #[front]
        method setInterval(handler: TimerHandler, timeout?: i32, arguments?: Rest) -> i32 =
            timer::set_interval;
        method clearTimeout(id?: i32) = timer::clear_timer;
        method clearInterval(id?: i32) = timer::clear_timer;

        // HTML's animation frames. WebIDL `unsigned long` handles, given as
        // a `long` (they go up from 1).
        #[front]
        method requestAnimationFrame(callback: FrameRequestCallback) -> i32 =
            animation_frame::request_animation_frame;
        method cancelAnimationFrame(handle: i32) = animation_frame::cancel_animation_frame;

        // requestIdleCallback's.
        #[front]
        method requestIdleCallback(callback: IdleRequestCallback, options?: IdleRequestOptions) -> u32 =
            idle::request_idle_callback;
        method cancelIdleCallback(handle: u32) = idle::cancel_idle_callback;

        // Fetch's.
        #[front]
        method fetch(input: RequestInfo, init?: RequestInit) -> PendingPromise<Response> = fetch::fetch;
        // HTML's window post message steps (`message.rs`).
        #[front]
        #[cross_origin]
        method postMessage(message: WindowMessage) = message::window_post_message;
        // HTML's: the value serialized with its options' transfer list (the
        // argument after it, which `CloneValue` reads), then deserialized
        // in this realm.
        method structuredClone(value: CloneValue) -> Cloned = crypto::structured_clone;
        // No auxiliary browsing contexts: none opened by script, so none
        // that script may close (HTML's script-closable), and no opener.
        #[cross_origin]
        method close() = frame::close;
        #[cross_origin]
        attribute closed: bool, get = frame::closed;
        #[replaceable]
        attribute performance: Performance, get = performance::window_performance;

        // CSSOM View's: the viewport. WebIDL `long`.
        #[replaceable]
        attribute innerWidth: i32, get = geometry::inner_width;
        #[replaceable]
        attribute innerHeight: i32, get = geometry::inner_height;
        #[replaceable]
        attribute scrollX: f64, get = geometry::scroll_x;
        #[replaceable]
        attribute pageXOffset: f64, get = geometry::scroll_x;
        #[replaceable]
        attribute scrollY: f64, get = geometry::scroll_y;
        #[replaceable]
        attribute pageYOffset: f64, get = geometry::scroll_y;
        #[replaceable]
        attribute screenX: i32, get = geometry::screen_x;
        #[replaceable]
        attribute screenLeft: i32, get = geometry::screen_x;
        #[replaceable]
        attribute screenY: i32, get = geometry::screen_y;
        #[replaceable]
        attribute screenTop: i32, get = geometry::screen_y;
        // The browser window's.
        #[replaceable]
        attribute outerWidth: i32, get = geometry::outer_width;
        #[replaceable]
        attribute outerHeight: i32, get = geometry::outer_height;
        #[replaceable]
        attribute devicePixelRatio: f64, get = geometry::device_pixel_ratio;
        // CSSOM View's: a new list each call.
        method matchMedia(query: DomString) -> MediaQueryList = media_query_list::match_media;
        // CSSOM's: a new declaration each call, live.
        method getComputedStyle(elt: Element, pseudoElt?: Option<DomString>) -> CSSStyleProperties =
            computed_style::get_computed_style;
        method scroll(options?: ScrollToOptions) -> PromiseOfUndefined = scroll::window_scroll_to;
        method scroll(x: UnrestrictedDouble, y: UnrestrictedDouble) -> PromiseOfUndefined = scroll::window_scroll_to_xy;
        method scrollTo(options?: ScrollToOptions) -> PromiseOfUndefined = scroll::window_scroll_to;
        method scrollTo(x: UnrestrictedDouble, y: UnrestrictedDouble) -> PromiseOfUndefined = scroll::window_scroll_to_xy;
        method scrollBy(options?: ScrollToOptions) -> PromiseOfUndefined = scroll::window_scroll_by;
        method scrollBy(x: UnrestrictedDouble, y: UnrestrictedDouble) -> PromiseOfUndefined = scroll::window_scroll_by_xy;
        // The Selection API's.
        method getSelection() -> Option<Selection> = selection::window_get_selection;
    }

    interface Location [type = dom::Location] {
        #[unforgeable]
        #[front]
        #[cross_origin(setter)]
        attribute href: DomString, get = location::href, set: NavigationUrl = location::set_href;
        #[unforgeable]
        attribute origin: DomString, get = location::origin;
        #[unforgeable]
        attribute protocol: DomString, get = location::protocol, set = location::set_protocol;
        #[unforgeable]
        attribute host: DomString, get = location::host, set = location::set_host;
        #[unforgeable]
        attribute hostname: DomString, get = location::hostname, set = location::set_hostname;
        #[unforgeable]
        attribute port: DomString, get = location::port, set = location::set_port;
        #[unforgeable]
        attribute pathname: DomString, get = location::pathname, set = location::set_pathname;
        #[unforgeable]
        attribute search: DomString, get = location::search, set = location::set_search;
        #[unforgeable]
        attribute hash: DomString, get = location::hash, set = location::set_hash;
        #[unforgeable]
        #[front]
        method assign(url: NavigationUrl) = location::set_href;
        #[unforgeable]
        #[front]
        #[cross_origin]
        method replace(url: NavigationUrl) = location::replace;
        #[unforgeable]
        method reload() = location::reload;
        #[unforgeable]
        method toString() -> DomString = location::href;
    }

    interface Navigator [type = dom::Navigator] {
        // NavigatorID
        attribute appCodeName: DomString, get = navigator::app_code_name;
        attribute appName: DomString, get = navigator::app_name;
        attribute appVersion: DomString, get = navigator::app_version;
        attribute platform: DomString, get = navigator::platform;
        attribute product: DomString, get = navigator::product;
        attribute productSub: DomString, get = navigator::product_sub;
        attribute userAgent: DomString, get = navigator::user_agent;
        attribute vendor: DomString, get = navigator::vendor;
        attribute vendorSub: DomString, get = navigator::vendor_sub;
        // NavigatorLanguage
        attribute language: DomString, get = navigator::language;
        attribute languages: Strings, get = navigator::languages;
        // NavigatorOnLine
        attribute onLine: bool, get = navigator::on_line;
        // NavigatorCookies
        attribute cookieEnabled: bool, get = navigator::cookie_enabled;
        // NavigatorConcurrentHardware
        attribute hardwareConcurrency: u32, get = navigator::hardware_concurrency;
        // NavigatorAutomationInformation (WebDriver's)
        attribute webdriver: bool, get = navigator::webdriver;
        // HTML's `NavigatorUserActivation`; `[SameObject]`.
        attribute userActivation: UserActivation, get = navigator::user_activation;
        // The other specs' partials Chrome has on the desktop, each
        // `[SameObject]`: Network Information's, Geolocation's, Screen Wake
        // Lock's, and `scheduling` (`isInputPending`'s).
        attribute connection: NetworkInformation, get = navigator::connection;
        attribute geolocation: Geolocation, get = geolocation::navigator_geolocation;
        attribute wakeLock: WakeLock, get = wake_lock::navigator_wake_lock;
        attribute scheduling: Scheduling, get = navigator::scheduling;
    }

    // DOM's: a controller makes its signal and aborts it.
    interface AbortController [type = dom::AbortController] {
        #[front]
        constructor() = abort::construct;
        attribute signal: AbortSignal, get = abort::signal;
        #[front]
        #[then_script]
        method abort(reason?: AbortReason) = abort::abort;
    }

    // Its static members first, as in Blink's `abort_signal.idl`.
    interface AbortSignal : EventTarget [type = AbortSignalObject] {
        #[static_method]
        #[front]
        method abort(reason?: AbortReason) -> AbortSignal = abort::abort_signal;
        #[static_method]
        #[front]
        method timeout(milliseconds: EnforcedU64) -> AbortSignal = abort::timeout;
        #[static_method]
        #[front]
        method any(signals: AbortSignals) -> AbortSignal = abort::any;
        attribute aborted: bool, get = abort::aborted;
        attribute reason: AnyOrUndefined, get = abort::reason;
        method throwIfAborted() = abort::throw_if_aborted;
        #[front]
        attribute onabort: EventHandler, get = handlers::port_handler::<{ event_type::ABORT }>, set = handlers::set_port_handler::<_, { event_type::ABORT }>;
    }

    // Fetch's: a header list and its guard, its pairs sorted and combined.
    interface Headers [type = dom::Headers] {
        constructor(init?: HeaderPairs) = headers::construct;
        method append(name: ByteString, value: ByteString) = headers::append;
        method delete(name: ByteString) = headers::delete;
        method get(name: ByteString) -> Option<DomString> = headers::get;
        method getSetCookie() -> Strings = headers::get_set_cookie;
        method has(name: ByteString) -> bool = headers::has;
        method set(name: ByteString, value: ByteString) = headers::set;
        #[pairs]
        method iterableEntry(index: u32) -> Strings = headers::pair;
    }

    // Fetch's: each enum member a string, checked as `Request`'s
    // constructor reads it; `window` must be null.
    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary RequestInit {
        method: Option<String> = None,
        headers: Option<HeadersMember> = None,
        body: Option<BodyMember<T::Node>> = None,
        referrer: Option<String> = None,
        referrerPolicy: Option<String> = None,
        mode: Option<String> = None,
        credentials: Option<String> = None,
        cache: Option<String> = None,
        redirect: Option<String> = None,
        integrity: Option<String> = None,
        keepalive: Option<bool> = None,
        signal: Option<SignalMember<T::Node>> = None,
        duplex: Option<String> = None,
        priority: Option<String> = None,
        window: Option<IsNull> = None,
    }

    // `statusText` a `ByteString`, checked as a reason phrase.
    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary ResponseInit {
        status: u16 = 200,
        statusText: String = String::new(),
        headers: Option<HeadersMember> = None,
    }

    // Fetch's; the body mixin's members after its own, as in Blink's
    // `request.idl`.
    interface Request [type = dom::Request] {
        #[front]
        constructor(input: RequestInfo, init?: RequestInit) = request::construct;
        attribute method: DomString, get = request::method;
        attribute url: DomString, get = request::url;
        attribute headers: Headers, get = request::headers;
        attribute destination: DomString, get = request::destination;
        attribute referrer: DomString, get = request::referrer;
        attribute referrerPolicy: DomString, get = request::referrer_policy;
        attribute mode: DomString, get = request::mode;
        attribute credentials: DomString, get = request::credentials;
        attribute cache: DomString, get = request::cache;
        attribute redirect: DomString, get = request::redirect;
        attribute integrity: DomString, get = request::integrity;
        attribute keepalive: bool, get = request::keepalive;
        attribute isReloadNavigation: bool, get = request::not_navigation;
        attribute isHistoryNavigation: bool, get = request::not_navigation;
        attribute signal: AbortSignal, get = request::signal;
        attribute duplex: DomString, get = request::duplex;
        method clone() -> Request = request::clone;
        attribute bodyUsed: bool, get = request::body_used;
        method arrayBuffer() -> PromiseOfArrayBuffer = request::array_buffer;
        method blob() -> Promise<Blob> = request::blob;
        method bytes() -> PromiseOfUint8Array = request::bytes;
        method json() -> PromiseOfJson = request::json;
        method text() -> PromiseOfString = request::text;
    }

    interface Response [type = dom::Response] {
        #[front]
        constructor(body?: NullableBody, init?: ResponseInit) = response::construct;
        #[static_method]
        #[front]
        method error() -> Response = response::error;
        #[static_method]
        #[front]
        method redirect(url: DomString, status?: U16Default302) -> Response = response::redirect;
        #[static_method]
        #[front]
        method json(data: JsonData, init?: ResponseInit) -> Response = response::json_static;
        attribute r#type: DomString, get = response::response_type;
        attribute url: DomString, get = response::url;
        attribute redirected: bool, get = response::redirected;
        attribute status: u16, get = response::status;
        attribute ok: bool, get = response::ok;
        attribute statusText: DomString, get = response::status_text;
        attribute headers: Headers, get = response::headers;
        method clone() -> Response = response::clone;
        attribute bodyUsed: bool, get = response::body_used;
        method arrayBuffer() -> PromiseOfArrayBuffer = response::array_buffer;
        method blob() -> Promise<Blob> = response::blob;
        method bytes() -> PromiseOfUint8Array = response::bytes;
        method json() -> PromiseOfJson = response::json;
        method text() -> PromiseOfString = response::text;
    }

    // requestIdleCallback's: 0 is no timeout.
    dictionary IdleRequestOptions {
        timeout: u32 = 0,
    }

    // What an idle callback gets.
    interface IdleDeadline [type = dom::IdleDeadline] {
        method timeRemaining() -> f64 = idle::time_remaining;
        attribute didTimeout: bool, get = idle::did_timeout;
    }

    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary IsInputPendingOptions {
        includeContinuous: bool = false,
    }

    // A navigator's `scheduling`, which tells whether input waits that its
    // window may know of.
    interface Scheduling [type = dom::Scheduling] {
        #[calls_script]
        method isInputPending(isInputPendingOptions?: IsInputPendingOptions) -> bool = navigator::is_input_pending;
    }

    // Network Information's `connection`, with the members Chrome has on
    // the desktop (not `type`, `downlinkMax` and `ontypechange`, which it
    // has on Android and ChromeOS only), `onchange` first as in Blink's
    // `network_information.idl`.
    interface NetworkInformation : EventTarget [type = NetworkInformationObject] {
        #[front]
        attribute onchange: EventHandler, get = handlers::port_handler::<{ event_type::CHANGE }>, set = handlers::set_port_handler::<_, { event_type::CHANGE }>;
        // WebIDL's `EffectiveConnectionType`, as a string.
        attribute effectiveType: DomString, get = navigator::effective_type;
        // WebIDL's `Millisecond`, an `unsigned long long`: always 0.
        attribute rtt: u32, get = navigator::rtt;
        // WebIDL's `Megabit`, an `unrestricted double`.
        attribute downlink: f64, get = navigator::downlink;
        attribute saveData: bool, get = navigator::save_data;
    }

    // Geolocation's: `timeout` and `maximumAge` are `[Clamp] unsigned
    // long`s, clamped by `geolocation`.
    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary PositionOptions {
        enableHighAccuracy: bool = false,
        timeout: Option<f64> = None,
        maximumAge: Option<f64> = None,
    }

    // A navigator's `geolocation`, which answers every request as Chrome
    // does when the user denies the permission; its operations in
    // Chrome's order (by name).
    interface Geolocation [type = dom::Geolocation] {
        method clearWatch(watchId: i32) = geolocation::clear_watch;
        #[front]
        method getCurrentPosition(successCallback: PositionCallback, errorCallback?: PositionErrorCallback, options?: PositionOptions) =
            geolocation::get_current_position;
        #[front]
        method watchPosition(successCallback: PositionCallback, errorCallback?: PositionErrorCallback, options?: PositionOptions) -> i32 =
            geolocation::watch_position;
    }

    // None is ever made (machopo has no position), but the interfaces are
    // there, as Chrome's.
    interface GeolocationPosition [type = NoPositionObject] {
        attribute coords: GeolocationCoordinates, get = geolocation::no_coords;
        // WebIDL's `EpochTimeStamp`, an `unsigned long long`.
        attribute timestamp: f64, get = geolocation::no_position;
        method toJSON() -> AnyOrNull = geolocation::no_json;
    }

    interface GeolocationCoordinates [type = NoPositionObject] {
        attribute latitude: f64, get = geolocation::no_position;
        attribute longitude: f64, get = geolocation::no_position;
        attribute altitude: Option<f64>, get = geolocation::no_optional;
        attribute accuracy: f64, get = geolocation::no_position;
        attribute altitudeAccuracy: Option<f64>, get = geolocation::no_optional;
        attribute heading: Option<f64>, get = geolocation::no_optional;
        attribute speed: Option<f64>, get = geolocation::no_optional;
        method toJSON() -> AnyOrNull = geolocation::no_json;
    }

    // The attributes before the constants, in Chrome's order.
    interface GeolocationPositionError [type = dom::GeolocationPositionError] {
        attribute code: u16, get = geolocation::error_code;
        attribute message: DomString, get = geolocation::error_message;
        const PERMISSION_DENIED: u32 = 1;
        const POSITION_UNAVAILABLE: u32 = 2;
        const TIMEOUT: u32 = 3;
    }

    // A navigator's `wakeLock`, which grants every request of a visible
    // page; `screen` locks only.
    interface WakeLock [type = dom::WakeLock] {
        method request(r#type?: OptionalString) -> PendingPromise<WakeLockSentinel> = wake_lock::request;
    }

    // Its members in Chrome's order.
    interface WakeLockSentinel : EventTarget [type = WakeLockSentinelObject] {
        #[front]
        attribute onrelease: EventHandler, get = handlers::port_handler::<{ event_type::RELEASE }>, set = handlers::set_port_handler::<_, { event_type::RELEASE }>;
        attribute released: bool, get = wake_lock::released;
        // WebIDL's `WakeLockType`, as a string.
        attribute r#type: DomString, get = wake_lock::sentinel_type;
        #[calls_script]
        method release() -> PromiseOfUndefined = wake_lock::release;
    }

    // HR-Time's, a window's: an event target of its own, on its document's
    // timeline; Performance Timeline's and User Timing's partials.
    interface Performance : EventTarget [type = PerformanceObject] {
        method now() -> f64 = performance::now;
        attribute timeOrigin: f64, get = performance::time_origin;
        method getEntries() -> Sequence<PerformanceEntry> = performance::get_entries;
        method getEntriesByType(r#type: DomString) -> Sequence<PerformanceEntry> = performance::get_entries_by_type;
        method getEntriesByName(name: DomString, r#type?: OptionalString) -> Sequence<PerformanceEntry> = performance::get_entries_by_name;
        method mark(markName: DomString, markOptions?: PerformanceMarkOptions) -> PerformanceMark = performance::mark;
        method clearMarks(markName?: OptionalString) = performance::clear_marks;
        method measure(measureName: DomString, startOrMeasureOptions?: MeasureStart, endMark?: OptionalString) -> PerformanceMeasure = performance::measure;
        method clearMeasures(measureName?: OptionalString) = performance::clear_measures;
        // Navigation Timing's, the same each time.
        attribute timing: PerformanceTiming, get = performance::timing;
        attribute navigation: PerformanceNavigation, get = performance::navigation;
    }

    // One `dom` type for every kind, as `Event` is.
    interface PerformanceEntry [type = PerformanceEntryObject] {
        attribute name: DomString, get = performance::name;
        attribute entryType: DomString, get = performance::entry_type;
        attribute startTime: f64, get = performance::start_time;
        attribute duration: f64, get = performance::duration;
    }

    interface PerformanceMark : PerformanceEntry [type = PerformanceMarkObject] {
        #[front]
        constructor(markName: DomString, markOptions?: PerformanceMarkOptions) = performance::construct_mark;
        // `any`: structured-serialized, each read the same object.
        attribute detail: EntryDetail, get = performance::detail;
    }

    interface PerformanceMeasure : PerformanceEntry [type = PerformanceMeasureObject] {
        attribute detail: EntryDetail, get = performance::detail;
    }

    // A missing `detail` is `null`.
    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary PerformanceMarkOptions {
        detail: Option<Serialized> = None,
        startTime: Option<f64> = None,
    }

    dictionary PerformanceMeasureOptions {
        detail: Option<Serialized> = None,
        start: Option<StringOrNumber> = None,
        duration: Option<f64> = None,
        end: Option<StringOrNumber> = None,
    }

    // JS's only (a guest reads the timeline). The methods in Chrome's
    // order.
    interface PerformanceObserver [type = dom::PerformanceObserver] {
        #[front]
        constructor(callback: PerformanceObserverCallback) = performance::construct_observer;
        method disconnect() = performance::disconnect;
        method observe(options?: PerformanceObserverInit) = performance::observe;
        method takeRecords() -> Sequence<PerformanceEntry> = performance::take_records;
        #[static_method]
        attribute supportedEntryTypes: FrozenStrings, get = performance::supported_entry_types;
    }

    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary PerformanceObserverInit {
        entryTypes: Option<Vec<String>> = None,
        r#type: Option<String> = None,
        buffered: Option<bool> = None,
    }

    // What an observer delivers, in `startTime` order.
    interface PerformanceObserverEntryList [type = dom::PerformanceObserverEntryList] {
        method getEntries() -> Sequence<PerformanceEntry> = performance::list_entries;
        method getEntriesByName(name: DomString, r#type?: OptionalString) -> Sequence<PerformanceEntry> = performance::list_entries_by_name;
        method getEntriesByType(r#type: DomString) -> Sequence<PerformanceEntry> = performance::list_entries_by_type;
    }

    // A navigation entry's times (machopo has no resource entries). The members
    // in Chrome 152's order; the service worker's are 0 or empty.
    interface PerformanceResourceTiming : PerformanceEntry [type = NavigationTimingObject] {
        attribute initiatorType: DomString, get = performance::initiator_type;
        attribute nextHopProtocol: DomString, get = performance::next_hop_protocol;
        attribute deliveryType: DomString, get = performance::empty;
        attribute workerStart: f64, get = performance::zero;
        attribute redirectStart: f64, get = performance::redirect_start;
        attribute redirectEnd: f64, get = performance::redirect_end;
        attribute fetchStart: f64, get = performance::fetch_start;
        attribute domainLookupStart: f64, get = performance::domain_lookup_start;
        attribute domainLookupEnd: f64, get = performance::domain_lookup_end;
        attribute connectStart: f64, get = performance::connect_start;
        attribute connectEnd: f64, get = performance::connect_end;
        attribute secureConnectionStart: f64, get = performance::secure_connection_start;
        attribute requestStart: f64, get = performance::request_start;
        attribute responseStart: f64, get = performance::response_start;
        attribute responseEnd: f64, get = performance::response_end;
        attribute transferSize: f64, get = performance::transfer_size;
        attribute encodedBodySize: f64, get = performance::encoded_body_size;
        attribute decodedBodySize: f64, get = performance::decoded_body_size;
        // `FrozenArray<PerformanceServerTiming>`: always empty.
        attribute serverTiming: Numbers, get = performance::server_timing;
        attribute renderBlockingStatus: DomString, get = performance::render_blocking_status;
        attribute responseStatus: u32, get = performance::response_status;
        attribute contentType: DomString, get = performance::content_type;
        attribute contentEncoding: DomString, get = performance::content_encoding;
        attribute finalResponseHeadersStart: f64, get = performance::final_response_headers_start;
        attribute firstInterimResponseStart: f64, get = performance::zero;
        attribute workerRouterEvaluationStart: f64, get = performance::zero;
        attribute workerCacheLookupStart: f64, get = performance::zero;
        attribute workerMatchedSourceType: DomString, get = performance::empty;
        attribute workerFinalSourceType: DomString, get = performance::empty;
    }

    // A document's navigation entry, its times filled in as they happen.
    interface PerformanceNavigationTiming : PerformanceResourceTiming [type = NavigationTimingObject] {
        attribute unloadEventStart: f64, get = performance::unload_event_start;
        attribute unloadEventEnd: f64, get = performance::unload_event_end;
        attribute domInteractive: f64, get = performance::dom_interactive;
        attribute domContentLoadedEventStart: f64, get = performance::dom_content_loaded_event_start;
        attribute domContentLoadedEventEnd: f64, get = performance::dom_content_loaded_event_end;
        attribute domComplete: f64, get = performance::dom_complete;
        attribute loadEventStart: f64, get = performance::load_event_start;
        attribute loadEventEnd: f64, get = performance::load_event_end;
        attribute r#type: DomString, get = performance::navigation_type;
        attribute redirectCount: u32, get = performance::redirect_count;
        attribute confidence: PerformanceTimingConfidence, get = performance::confidence;
        attribute criticalCHRestart: f64, get = performance::zero;
        attribute activationStart: f64, get = performance::zero;
        // `NotRestoredReasons?`: machopo has no back-forward cache.
        attribute notRestoredReasons: AlwaysNull, get = performance::not_restored_reasons;
    }

    // None is ever made: machopo has no server timing (`serverTiming` is
    // empty) and no back-forward cache (`notRestoredReasons` is null). The
    // interfaces are there, as Chrome's.
    interface PerformanceServerTiming [type = NoObject] {}

    interface NotRestoredReasons [type = NoObject] {}

    // Chromium's randomized response.
    interface PerformanceTimingConfidence [type = ConfidenceObject] {
        attribute randomizedTriggerRate: f64, get = performance::randomized_trigger_rate;
        attribute value: DomString, get = performance::confidence_value;
    }

    // `performance.timing`: whole milliseconds since the Unix epoch.
    interface PerformanceTiming [type = PerformanceTimingObject] {
        attribute navigationStart: f64, get = performance::legacy::<0>;
        attribute unloadEventStart: f64, get = performance::legacy::<1>;
        attribute unloadEventEnd: f64, get = performance::legacy::<2>;
        attribute redirectStart: f64, get = performance::legacy::<3>;
        attribute redirectEnd: f64, get = performance::legacy::<4>;
        attribute fetchStart: f64, get = performance::legacy::<5>;
        attribute domainLookupStart: f64, get = performance::legacy::<6>;
        attribute domainLookupEnd: f64, get = performance::legacy::<7>;
        attribute connectStart: f64, get = performance::legacy::<8>;
        attribute connectEnd: f64, get = performance::legacy::<9>;
        attribute secureConnectionStart: f64, get = performance::legacy::<10>;
        attribute requestStart: f64, get = performance::legacy::<11>;
        attribute responseStart: f64, get = performance::legacy::<12>;
        attribute responseEnd: f64, get = performance::legacy::<13>;
        attribute domLoading: f64, get = performance::legacy::<14>;
        attribute domInteractive: f64, get = performance::legacy::<15>;
        attribute domContentLoadedEventStart: f64, get = performance::legacy::<16>;
        attribute domContentLoadedEventEnd: f64, get = performance::legacy::<17>;
        attribute domComplete: f64, get = performance::legacy::<18>;
        attribute loadEventStart: f64, get = performance::legacy::<19>;
        attribute loadEventEnd: f64, get = performance::legacy::<20>;
    }

    // `performance.navigation`; the constants after the attributes, as
    // Chrome 152 has them.
    interface PerformanceNavigation [type = PerformanceNavigationObject] {
        attribute r#type: u32, get = performance::legacy_type;
        attribute redirectCount: u32, get = performance::legacy_redirect_count;
        const TYPE_NAVIGATE: u32 = 0;
        const TYPE_RELOAD: u32 = 1;
        const TYPE_BACK_FORWARD: u32 = 2;
        const TYPE_RESERVED: u32 = 255;
    }

    // URL Standard's API. `toString()` is the stringifier.
    interface URL [type = dom::Url] {
        constructor(url: DomString, base?: OptionalString) = url::construct;
        #[static_method]
        method parse(url: DomString, base?: OptionalString) -> Option<URL> = url::parse_static;
        #[static_method]
        method canParse(url: DomString, base?: OptionalString) -> bool = url::can_parse;
        // File API's: the host keeps the blob URLs.
        #[static_method]
        #[front]
        method createObjectURL(obj: Blob) -> DomString = url::create_object_url;
        #[static_method]
        #[front]
        method revokeObjectURL(url: DomString) = url::revoke_object_url;
        attribute href: DomString, get = url::href, set = url::set_href;
        method toString() -> DomString = url::href;
        attribute origin: DomString, get = url::origin;
        attribute protocol: DomString, get = url::protocol, set = url::set_protocol;
        attribute username: DomString, get = url::username, set = url::set_username;
        attribute password: DomString, get = url::password, set = url::set_password;
        attribute host: DomString, get = url::host, set = url::set_host;
        attribute hostname: DomString, get = url::hostname, set = url::set_hostname;
        attribute port: DomString, get = url::port, set = url::set_port;
        attribute pathname: DomString, get = url::pathname, set = url::set_pathname;
        attribute search: DomString, get = url::search, set = url::set_search;
        attribute searchParams: URLSearchParams, get = url::search_params;
        attribute hash: DomString, get = url::hash, set = url::set_hash;
        method toJSON() -> DomString = url::href;
    }

    // WebIDL's `iterable<USVString, USVString>`: its pairs (`#[pairs]`).
    interface URLSearchParams [type = dom::UrlSearchParams] {
        constructor(init?: StringPairs) = url::construct_search_params;
        attribute size: u32, get = url::size;
        method append(name: DomString, value: DomString) = url::append;
        method delete(name: DomString, value?: OptionalString) = url::delete;
        method get(name: DomString) -> Option<DomString> = url::get_value;
        method getAll(name: DomString) -> Strings = url::get_all;
        method has(name: DomString, value?: OptionalString) -> bool = url::has;
        method set(name: DomString, value: DomString) = url::set_value;
        method sort() = url::sort;
        method toString() -> DomString = url::to_string;
        #[pairs]
        method iterableEntry(index: u32) -> Strings = url::pair;
    }

    // WebCrypto's, without `subtle`.
    interface Crypto [type = dom::Crypto] {
        method getRandomValues(array: ArrayView) -> FilledView = crypto::get_random_values;
        method randomUUID() -> DomString = crypto::random_uuid;
    }

    // The window's user activation, read live.
    interface UserActivation [type = dom::UserActivation] {
        attribute hasBeenActive: bool, get = navigator::has_been_active;
        attribute isActive: bool, get = navigator::is_active;
    }

    // `scrollRestoration` is an enumeration (`auto`, `manual`): another
    // value set is ignored.
    interface History [type = dom::History] {
        attribute length: u32, get = history::length;
        attribute scrollRestoration: DomString, get = history::scroll_restoration, set = history::set_scroll_restoration;
        attribute state: HistoryState, get = history::state;
        method go(delta?: i32) = history::go;
        method back() = history::back;
        method forward() = history::forward;
        method pushState(data: SerializedState, unused: DomString, url?: Option<DomString>) = history::push_state;
        method replaceState(data: SerializedState, unused: DomString, url?: Option<DomString>) = history::replace_state;
    }

    interface Node : EventTarget [] {
        const ELEMENT_NODE: u32 = 1;
        const ATTRIBUTE_NODE: u32 = 2;
        const TEXT_NODE: u32 = 3;
        const CDATA_SECTION_NODE: u32 = 4;
        // Historical.
        const ENTITY_REFERENCE_NODE: u32 = 5;
        // Historical.
        const ENTITY_NODE: u32 = 6;
        const PROCESSING_INSTRUCTION_NODE: u32 = 7;
        const COMMENT_NODE: u32 = 8;
        const DOCUMENT_NODE: u32 = 9;
        const DOCUMENT_TYPE_NODE: u32 = 10;
        const DOCUMENT_FRAGMENT_NODE: u32 = 11;
        // Historical.
        const NOTATION_NODE: u32 = 12;
        // WebIDL `unsigned short`.
        attribute nodeType: u32, get = node::node_type;
        attribute nodeName: DomString, get = node::node_name;

        attribute isConnected: bool, get = node::is_connected;
        method getRootNode(options?: GetRootNodeOptions) -> Node = shadow_root::get_root_node;
        attribute parentNode: Option<Node>, get = node::parent_node;
        attribute baseURI: DomString, get = node::base_uri;

        attribute ownerDocument: Option<Document>, get = node::owner_document;
        attribute parentElement: Option<Element>, get = node::parent_element;
        method hasChildNodes() -> bool = node::has_child_nodes;
        attribute firstChild: Option<Node>, get = node::first_child;
        attribute lastChild: Option<Node>, get = node::last_child;
        attribute previousSibling: Option<Node>, get = node::previous_sibling;
        attribute nextSibling: Option<Node>, get = node::next_sibling;

        attribute childNodes: NodeList, get = node::child_nodes;
        #[ce_reactions]
        attribute nodeValue: Option<DomString>, get = node::node_value, set = node::set_node_value;
        #[ce_reactions]
        attribute textContent: Option<DomString>, get = node::text_content, set = node::set_text_content;
        #[ce_reactions]
        method appendChild(node: Node) -> Node = node::append_child;

        #[ce_reactions]
        method insertBefore(node: Node, child: Option<Node>) -> Node = node::insert_before;
        #[ce_reactions]
        method replaceChild(node: Node, child: Node) -> Node = node::replace_child;
        #[ce_reactions]
        method removeChild(child: Node) -> Node = node::remove_child;
        #[ce_reactions]
        method cloneNode(deep?: bool) -> NewNodeLikeThis = document::clone_node;

        method isEqualNode(otherNode: Option<Node>) -> bool = node::is_equal_node;
        method isSameNode(otherNode: Option<Node>) -> bool = node::is_same_node;

        const DOCUMENT_POSITION_DISCONNECTED: u32 = 0x01;
        const DOCUMENT_POSITION_PRECEDING: u32 = 0x02;
        const DOCUMENT_POSITION_FOLLOWING: u32 = 0x04;
        const DOCUMENT_POSITION_CONTAINS: u32 = 0x08;
        const DOCUMENT_POSITION_CONTAINED_BY: u32 = 0x10;
        const DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC: u32 = 0x20;
        method compareDocumentPosition(other: Node) -> u16 = node::compare_document_position;
        method contains(other: Option<Node>) -> bool = node::contains;
        #[ce_reactions]
        method normalize() = node::normalize;

        method lookupPrefix(namespace: Option<DomString>) -> Option<DomString> = node::lookup_prefix;
        method lookupNamespaceURI(prefix: Option<DomString>) -> Option<DomString> = node::lookup_namespace_uri;
        method isDefaultNamespace(namespace: Option<DomString>) -> bool = node::is_default_namespace;
    }

    dictionary GetRootNodeOptions {
        composed: bool = false,
    }

    interface Document : Node [Document] {
        // An XML document (DOM's `new Document()`).
        #[front]
        constructor() = document::construct_document;
        #[static_method]
        #[front]
        method parseHTMLUnsafe(html: DomString) -> NewObject<Document> = markup::parse_html_unsafe;
        attribute implementation: DOMImplementation, get = document::implementation;
        attribute URL: DomString, get = document::url;
        attribute documentURI: DomString, get = document::url;
        attribute compatMode: DomString, get = document::compat_mode;
        attribute defaultView: Option<Window>, get = document::default_view;
        // Its window's, `[PutForwards=href]`; none without one.
        #[unforgeable]
        #[front]
        attribute location: Option<Location>, get = window::document_location, set: NavigationUrl = window::set_document_location;
        attribute characterSet: DomString, get = document::character_set;
        attribute charset: DomString, get = document::character_set;
        attribute inputEncoding: DomString, get = document::character_set;
        attribute contentType: DomString, get = document::content_type;
        attribute doctype: Option<DocumentType>, get = document::doctype;
        attribute head: Option<HTMLElement>, get = document::head;
        attribute documentElement: Option<Element>, get = document::document_element;
        // HTML's.
        #[ce_reactions]
        attribute body: Option<HTMLElement>, get = document::body, set = document::set_body;
        // HTML's.
        attribute forms: HTMLCollection, get = forms::document_forms;
        attribute links: HTMLCollection, get = document::links;
        attribute anchors: HTMLCollection, get = document::anchors;
        // Obsolete: every element, an `HTMLAllCollection` (undetectable).
        attribute all: HTMLAllCollection, get = collection::document_all;
        attribute images: HTMLCollection, get = document::images;
        attribute embeds: HTMLCollection, get = document::embeds;
        attribute plugins: HTMLCollection, get = document::embeds;
        attribute scripts: HTMLCollection, get = document::scripts;
        attribute currentScript: Option<Element>, get = document::current_script;
        attribute readyState: DomString, get = document::ready_state;
        #[ce_reactions]
        attribute dir: DomString, get = document::dir, set = document::set_dir;
        attribute hidden: bool, get = document::hidden;
        // WebIDL's `DocumentVisibilityState`, as a string.
        attribute visibilityState: DomString, get = document::visibility_state;
        #[front]
        attribute onvisibilitychange: EventHandler, get = handlers::document_handler::<{ event_type::VISIBILITYCHANGE }>, set = handlers::set_document_handler::<_, { event_type::VISIBILITYCHANGE }>;

        // HTML's `GlobalEventHandlers`.
        #[front]
        attribute onabort: EventHandler, get = handlers::document_handler::<{ event_type::ABORT }>, set = handlers::set_document_handler::<_, { event_type::ABORT }>;
        #[front]
        attribute onanimationcancel: EventHandler, get = handlers::document_handler::<{ event_type::ANIMATIONCANCEL }>, set = handlers::set_document_handler::<_, { event_type::ANIMATIONCANCEL }>;
        #[front]
        attribute onanimationend: EventHandler, get = handlers::document_handler::<{ event_type::ANIMATIONEND }>, set = handlers::set_document_handler::<_, { event_type::ANIMATIONEND }>;
        #[front]
        attribute onanimationiteration: EventHandler, get = handlers::document_handler::<{ event_type::ANIMATIONITERATION }>, set = handlers::set_document_handler::<_, { event_type::ANIMATIONITERATION }>;
        #[front]
        attribute onanimationstart: EventHandler, get = handlers::document_handler::<{ event_type::ANIMATIONSTART }>, set = handlers::set_document_handler::<_, { event_type::ANIMATIONSTART }>;
        #[front]
        attribute onauxclick: EventHandler, get = handlers::document_handler::<{ event_type::AUXCLICK }>, set = handlers::set_document_handler::<_, { event_type::AUXCLICK }>;
        #[front]
        attribute onbeforeinput: EventHandler, get = handlers::document_handler::<{ event_type::BEFOREINPUT }>, set = handlers::set_document_handler::<_, { event_type::BEFOREINPUT }>;
        #[front]
        attribute onbeforematch: EventHandler, get = handlers::document_handler::<{ event_type::BEFOREMATCH }>, set = handlers::set_document_handler::<_, { event_type::BEFOREMATCH }>;
        #[front]
        attribute onbeforetoggle: EventHandler, get = handlers::document_handler::<{ event_type::BEFORETOGGLE }>, set = handlers::set_document_handler::<_, { event_type::BEFORETOGGLE }>;
        #[front]
        attribute onblur: EventHandler, get = handlers::document_handler::<{ event_type::BLUR }>, set = handlers::set_document_handler::<_, { event_type::BLUR }>;
        #[front]
        attribute oncancel: EventHandler, get = handlers::document_handler::<{ event_type::CANCEL }>, set = handlers::set_document_handler::<_, { event_type::CANCEL }>;
        #[front]
        attribute oncanplay: EventHandler, get = handlers::document_handler::<{ event_type::CANPLAY }>, set = handlers::set_document_handler::<_, { event_type::CANPLAY }>;
        #[front]
        attribute oncanplaythrough: EventHandler, get = handlers::document_handler::<{ event_type::CANPLAYTHROUGH }>, set = handlers::set_document_handler::<_, { event_type::CANPLAYTHROUGH }>;
        #[front]
        attribute onchange: EventHandler, get = handlers::document_handler::<{ event_type::CHANGE }>, set = handlers::set_document_handler::<_, { event_type::CHANGE }>;
        #[front]
        attribute onclick: EventHandler, get = handlers::document_handler::<{ event_type::CLICK }>, set = handlers::set_document_handler::<_, { event_type::CLICK }>;
        #[front]
        attribute onclose: EventHandler, get = handlers::document_handler::<{ event_type::CLOSE }>, set = handlers::set_document_handler::<_, { event_type::CLOSE }>;
        #[front]
        attribute oncommand: EventHandler, get = handlers::document_handler::<{ event_type::COMMAND }>, set = handlers::set_document_handler::<_, { event_type::COMMAND }>;
        #[front]
        attribute oncontentvisibilityautostatechange: EventHandler, get = handlers::document_handler::<{ event_type::CONTENTVISIBILITYAUTOSTATECHANGE }>, set = handlers::set_document_handler::<_, { event_type::CONTENTVISIBILITYAUTOSTATECHANGE }>;
        #[front]
        attribute oncontextlost: EventHandler, get = handlers::document_handler::<{ event_type::CONTEXTLOST }>, set = handlers::set_document_handler::<_, { event_type::CONTEXTLOST }>;
        #[front]
        attribute oncontextmenu: EventHandler, get = handlers::document_handler::<{ event_type::CONTEXTMENU }>, set = handlers::set_document_handler::<_, { event_type::CONTEXTMENU }>;
        #[front]
        attribute oncontextrestored: EventHandler, get = handlers::document_handler::<{ event_type::CONTEXTRESTORED }>, set = handlers::set_document_handler::<_, { event_type::CONTEXTRESTORED }>;
        #[front]
        attribute oncuechange: EventHandler, get = handlers::document_handler::<{ event_type::CUECHANGE }>, set = handlers::set_document_handler::<_, { event_type::CUECHANGE }>;
        #[front]
        attribute ondblclick: EventHandler, get = handlers::document_handler::<{ event_type::DBLCLICK }>, set = handlers::set_document_handler::<_, { event_type::DBLCLICK }>;
        #[front]
        attribute ondrag: EventHandler, get = handlers::document_handler::<{ event_type::DRAG }>, set = handlers::set_document_handler::<_, { event_type::DRAG }>;
        #[front]
        attribute ondragend: EventHandler, get = handlers::document_handler::<{ event_type::DRAGEND }>, set = handlers::set_document_handler::<_, { event_type::DRAGEND }>;
        #[front]
        attribute ondragenter: EventHandler, get = handlers::document_handler::<{ event_type::DRAGENTER }>, set = handlers::set_document_handler::<_, { event_type::DRAGENTER }>;
        #[front]
        attribute ondragleave: EventHandler, get = handlers::document_handler::<{ event_type::DRAGLEAVE }>, set = handlers::set_document_handler::<_, { event_type::DRAGLEAVE }>;
        #[front]
        attribute ondragover: EventHandler, get = handlers::document_handler::<{ event_type::DRAGOVER }>, set = handlers::set_document_handler::<_, { event_type::DRAGOVER }>;
        #[front]
        attribute ondragstart: EventHandler, get = handlers::document_handler::<{ event_type::DRAGSTART }>, set = handlers::set_document_handler::<_, { event_type::DRAGSTART }>;
        #[front]
        attribute ondrop: EventHandler, get = handlers::document_handler::<{ event_type::DROP }>, set = handlers::set_document_handler::<_, { event_type::DROP }>;
        #[front]
        attribute ondurationchange: EventHandler, get = handlers::document_handler::<{ event_type::DURATIONCHANGE }>, set = handlers::set_document_handler::<_, { event_type::DURATIONCHANGE }>;
        #[front]
        attribute onemptied: EventHandler, get = handlers::document_handler::<{ event_type::EMPTIED }>, set = handlers::set_document_handler::<_, { event_type::EMPTIED }>;
        #[front]
        attribute onended: EventHandler, get = handlers::document_handler::<{ event_type::ENDED }>, set = handlers::set_document_handler::<_, { event_type::ENDED }>;
        #[front]
        attribute onerror: EventHandler, get = handlers::document_handler::<{ event_type::ERROR }>, set = handlers::set_document_handler::<_, { event_type::ERROR }>;
        #[front]
        attribute onfocus: EventHandler, get = handlers::document_handler::<{ event_type::FOCUS }>, set = handlers::set_document_handler::<_, { event_type::FOCUS }>;
        #[front]
        attribute onformdata: EventHandler, get = handlers::document_handler::<{ event_type::FORMDATA }>, set = handlers::set_document_handler::<_, { event_type::FORMDATA }>;
        #[front]
        attribute oninput: EventHandler, get = handlers::document_handler::<{ event_type::INPUT }>, set = handlers::set_document_handler::<_, { event_type::INPUT }>;
        #[front]
        attribute oninvalid: EventHandler, get = handlers::document_handler::<{ event_type::INVALID }>, set = handlers::set_document_handler::<_, { event_type::INVALID }>;
        #[front]
        attribute onkeydown: EventHandler, get = handlers::document_handler::<{ event_type::KEYDOWN }>, set = handlers::set_document_handler::<_, { event_type::KEYDOWN }>;
        #[front]
        attribute onkeypress: EventHandler, get = handlers::document_handler::<{ event_type::KEYPRESS }>, set = handlers::set_document_handler::<_, { event_type::KEYPRESS }>;
        #[front]
        attribute onkeyup: EventHandler, get = handlers::document_handler::<{ event_type::KEYUP }>, set = handlers::set_document_handler::<_, { event_type::KEYUP }>;
        #[front]
        attribute onload: EventHandler, get = handlers::document_handler::<{ event_type::LOAD }>, set = handlers::set_document_handler::<_, { event_type::LOAD }>;
        #[front]
        attribute onloadeddata: EventHandler, get = handlers::document_handler::<{ event_type::LOADEDDATA }>, set = handlers::set_document_handler::<_, { event_type::LOADEDDATA }>;
        #[front]
        attribute onloadedmetadata: EventHandler, get = handlers::document_handler::<{ event_type::LOADEDMETADATA }>, set = handlers::set_document_handler::<_, { event_type::LOADEDMETADATA }>;
        #[front]
        attribute onloadstart: EventHandler, get = handlers::document_handler::<{ event_type::LOADSTART }>, set = handlers::set_document_handler::<_, { event_type::LOADSTART }>;
        #[front]
        attribute onmousedown: EventHandler, get = handlers::document_handler::<{ event_type::MOUSEDOWN }>, set = handlers::set_document_handler::<_, { event_type::MOUSEDOWN }>;
        #[front]
        attribute onmouseenter: EventHandler, get = handlers::document_handler::<{ event_type::MOUSEENTER }>, set = handlers::set_document_handler::<_, { event_type::MOUSEENTER }>;
        #[front]
        attribute onmouseleave: EventHandler, get = handlers::document_handler::<{ event_type::MOUSELEAVE }>, set = handlers::set_document_handler::<_, { event_type::MOUSELEAVE }>;
        #[front]
        attribute onmousemove: EventHandler, get = handlers::document_handler::<{ event_type::MOUSEMOVE }>, set = handlers::set_document_handler::<_, { event_type::MOUSEMOVE }>;
        #[front]
        attribute onmouseout: EventHandler, get = handlers::document_handler::<{ event_type::MOUSEOUT }>, set = handlers::set_document_handler::<_, { event_type::MOUSEOUT }>;
        #[front]
        attribute onmouseover: EventHandler, get = handlers::document_handler::<{ event_type::MOUSEOVER }>, set = handlers::set_document_handler::<_, { event_type::MOUSEOVER }>;
        #[front]
        attribute onmouseup: EventHandler, get = handlers::document_handler::<{ event_type::MOUSEUP }>, set = handlers::set_document_handler::<_, { event_type::MOUSEUP }>;
        #[front]
        attribute onmousewheel: EventHandler, get = handlers::document_handler::<{ event_type::MOUSEWHEEL }>, set = handlers::set_document_handler::<_, { event_type::MOUSEWHEEL }>;
        #[front]
        attribute onpause: EventHandler, get = handlers::document_handler::<{ event_type::PAUSE }>, set = handlers::set_document_handler::<_, { event_type::PAUSE }>;
        #[front]
        attribute onplay: EventHandler, get = handlers::document_handler::<{ event_type::PLAY }>, set = handlers::set_document_handler::<_, { event_type::PLAY }>;
        #[front]
        attribute onplaying: EventHandler, get = handlers::document_handler::<{ event_type::PLAYING }>, set = handlers::set_document_handler::<_, { event_type::PLAYING }>;
        #[front]
        attribute onprogress: EventHandler, get = handlers::document_handler::<{ event_type::PROGRESS }>, set = handlers::set_document_handler::<_, { event_type::PROGRESS }>;
        #[front]
        attribute onratechange: EventHandler, get = handlers::document_handler::<{ event_type::RATECHANGE }>, set = handlers::set_document_handler::<_, { event_type::RATECHANGE }>;
        #[front]
        attribute onreset: EventHandler, get = handlers::document_handler::<{ event_type::RESET }>, set = handlers::set_document_handler::<_, { event_type::RESET }>;
        #[front]
        attribute onresize: EventHandler, get = handlers::document_handler::<{ event_type::RESIZE }>, set = handlers::set_document_handler::<_, { event_type::RESIZE }>;
        #[front]
        attribute onscroll: EventHandler, get = handlers::document_handler::<{ event_type::SCROLL }>, set = handlers::set_document_handler::<_, { event_type::SCROLL }>;
        #[front]
        attribute onscrollend: EventHandler, get = handlers::document_handler::<{ event_type::SCROLLEND }>, set = handlers::set_document_handler::<_, { event_type::SCROLLEND }>;
        #[front]
        attribute onsecuritypolicyviolation: EventHandler, get = handlers::document_handler::<{ event_type::SECURITYPOLICYVIOLATION }>, set = handlers::set_document_handler::<_, { event_type::SECURITYPOLICYVIOLATION }>;
        #[front]
        attribute onseeked: EventHandler, get = handlers::document_handler::<{ event_type::SEEKED }>, set = handlers::set_document_handler::<_, { event_type::SEEKED }>;
        #[front]
        attribute onseeking: EventHandler, get = handlers::document_handler::<{ event_type::SEEKING }>, set = handlers::set_document_handler::<_, { event_type::SEEKING }>;
        #[front]
        attribute onselect: EventHandler, get = handlers::document_handler::<{ event_type::SELECT }>, set = handlers::set_document_handler::<_, { event_type::SELECT }>;
        #[front]
        attribute onselectionchange: EventHandler, get = handlers::document_handler::<{ event_type::SELECTIONCHANGE }>, set = handlers::set_document_handler::<_, { event_type::SELECTIONCHANGE }>;
        #[front]
        attribute onselectstart: EventHandler, get = handlers::document_handler::<{ event_type::SELECTSTART }>, set = handlers::set_document_handler::<_, { event_type::SELECTSTART }>;
        #[front]
        attribute onslotchange: EventHandler, get = handlers::document_handler::<{ event_type::SLOTCHANGE }>, set = handlers::set_document_handler::<_, { event_type::SLOTCHANGE }>;
        #[front]
        attribute onstalled: EventHandler, get = handlers::document_handler::<{ event_type::STALLED }>, set = handlers::set_document_handler::<_, { event_type::STALLED }>;
        #[front]
        attribute onsubmit: EventHandler, get = handlers::document_handler::<{ event_type::SUBMIT }>, set = handlers::set_document_handler::<_, { event_type::SUBMIT }>;
        #[front]
        attribute onsuspend: EventHandler, get = handlers::document_handler::<{ event_type::SUSPEND }>, set = handlers::set_document_handler::<_, { event_type::SUSPEND }>;
        #[front]
        attribute ontimeupdate: EventHandler, get = handlers::document_handler::<{ event_type::TIMEUPDATE }>, set = handlers::set_document_handler::<_, { event_type::TIMEUPDATE }>;
        #[front]
        attribute ontoggle: EventHandler, get = handlers::document_handler::<{ event_type::TOGGLE }>, set = handlers::set_document_handler::<_, { event_type::TOGGLE }>;
        #[front]
        attribute ontransitioncancel: EventHandler, get = handlers::document_handler::<{ event_type::TRANSITIONCANCEL }>, set = handlers::set_document_handler::<_, { event_type::TRANSITIONCANCEL }>;
        #[front]
        attribute ontransitionend: EventHandler, get = handlers::document_handler::<{ event_type::TRANSITIONEND }>, set = handlers::set_document_handler::<_, { event_type::TRANSITIONEND }>;
        #[front]
        attribute ontransitionrun: EventHandler, get = handlers::document_handler::<{ event_type::TRANSITIONRUN }>, set = handlers::set_document_handler::<_, { event_type::TRANSITIONRUN }>;
        #[front]
        attribute ontransitionstart: EventHandler, get = handlers::document_handler::<{ event_type::TRANSITIONSTART }>, set = handlers::set_document_handler::<_, { event_type::TRANSITIONSTART }>;
        #[front]
        attribute onvolumechange: EventHandler, get = handlers::document_handler::<{ event_type::VOLUMECHANGE }>, set = handlers::set_document_handler::<_, { event_type::VOLUMECHANGE }>;
        #[front]
        attribute onwaiting: EventHandler, get = handlers::document_handler::<{ event_type::WAITING }>, set = handlers::set_document_handler::<_, { event_type::WAITING }>;
        #[front]
        attribute onwebkitanimationend: EventHandler, get = handlers::document_handler::<{ event_type::WEBKIT_ANIMATION_END }>, set = handlers::set_document_handler::<_, { event_type::WEBKIT_ANIMATION_END }>;
        #[front]
        attribute onwebkitanimationiteration: EventHandler, get = handlers::document_handler::<{ event_type::WEBKIT_ANIMATION_ITERATION }>, set = handlers::set_document_handler::<_, { event_type::WEBKIT_ANIMATION_ITERATION }>;
        #[front]
        attribute onwebkitanimationstart: EventHandler, get = handlers::document_handler::<{ event_type::WEBKIT_ANIMATION_START }>, set = handlers::set_document_handler::<_, { event_type::WEBKIT_ANIMATION_START }>;
        #[front]
        attribute onwebkittransitionend: EventHandler, get = handlers::document_handler::<{ event_type::WEBKIT_TRANSITION_END }>, set = handlers::set_document_handler::<_, { event_type::WEBKIT_TRANSITION_END }>;
        #[front]
        attribute onwheel: EventHandler, get = handlers::document_handler::<{ event_type::WHEEL }>, set = handlers::set_document_handler::<_, { event_type::WHEEL }>;

        attribute children: HTMLCollection, get = parent_node::children;
        attribute firstElementChild: Option<Element>, get = parent_node::first_element_child;
        attribute lastElementChild: Option<Element>, get = parent_node::last_element_child;
        // WebIDL `unsigned long`.
        attribute childElementCount: u32, get = parent_node::child_element_count;
        method getElementById(elementId: DomString) -> Option<Element> = query::get_element_by_id;
        method createEvent(interface: DomString) -> Event = document::create_event;
        // `whatToShow`'s default is `NodeFilter.SHOW_ALL`.
        #[front]
        method createNodeIterator(root: Node, whatToShow?: WhatToShow, filter?: Option<NodeFilter>) -> NodeIterator =
            traversal::create_node_iterator;
        #[front]
        method createTreeWalker(root: Node, whatToShow?: WhatToShow, filter?: Option<NodeFilter>) -> TreeWalker =
            traversal::create_tree_walker;
        method createRange() -> Range = range::create_range;
        // The Selection API's.
        method getSelection() -> Option<Selection> = selection::get_selection;

        method getElementsByTagName(qualifiedName: DomString) -> HTMLCollection =
            collection::get_elements_by_tag_name;
        method getElementsByTagNameNS(namespace: Option<DomString>, localName: DomString) -> HTMLCollection =
            element::get_elements_by_tag_name_ns;
        method getElementsByClassName(classNames: DomString) -> HTMLCollection =
            collection::get_elements_by_class_name;
        method getElementsByName(elementName: DomString) -> NodeList = document::get_elements_by_name;
        method querySelector(selectors: DomString) -> Option<Element> = query::query_selector;
        method querySelectorAll(selectors: DomString) -> NodeList = query::query_selector_all;
        #[ce_reactions]
        method append(nodes?: NodesOrStrings) = parent_node::append;
        #[ce_reactions]
        method prepend(nodes?: NodesOrStrings) = parent_node::prepend;
        #[ce_reactions]
        method replaceChildren(nodes?: NodesOrStrings) = parent_node::replace_children;

        // Most an HTML document makes are HTML elements of no interface of
        // their own: their wrapper made ahead.
        method createElement(localName: ElementLocalName) -> NewHtmlElement = document::create_element;
        method createElementNS(namespace: Option<DomString>, qualifiedName: DomString) -> NewElement<Element> =
            document::create_element_ns;
        method createAttribute(localName: DomString) -> NewObject<Attr> = attr::create_attribute;
        method createAttributeNS(namespace: Option<DomString>, qualifiedName: DomString) -> NewObject<Attr> =
            attr::create_attribute_ns;
        method createTextNode(data: DomString) -> NewObject<Text> = document::create_text_node;
        method createComment(data: DomString) -> NewObject<Comment> = document::create_comment;
        method createCDATASection(data: DomString) -> NewObject<CDATASection> = document::create_cdata_section;
        method createProcessingInstruction(target: DomString, data: DomString) -> NewObject<ProcessingInstruction> =
            document::create_processing_instruction;
        method createDocumentFragment() -> NewObject<DocumentFragment> = document::create_document_fragment;
        #[ce_reactions]
        method importNode(node: Node, deep?: bool) -> Node = document::import_node;
        #[ce_reactions]
        method adoptNode(node: Node) -> Node = document::adopt_node;
        attribute scrollingElement: Option<Element>, get = geometry::scrolling_element;

        // CSSOM View's.
        method elementFromPoint(x: f64, y: f64) -> Option<Element> = geometry::element_from_point;
        method elementsFromPoint(x: f64, y: f64) -> Sequence<Element> = geometry::elements_from_point;

        #[ce_reactions]
        attribute title: DomString, get = document::title, set = document::set_title;
        // HTML's focus.
        attribute activeElement: Option<Element>, get = focus::active_element;
        method hasFocus() -> bool = focus::has_focus;
        // DocumentOrShadowRoot's (CSSOM's).
        attribute styleSheets: StyleSheetList, get = css_style_sheet::style_sheets;
        attribute adoptedStyleSheets: ObservableArray<CSSStyleSheet>, get = css_style_sheet::adopted;
        #[observable(set, adoptedStyleSheets)]
        method setAdoptedStyleSheet(index: u32, sheet: CSSStyleSheet) = css_style_sheet::set_adopted;
        #[observable(delete, adoptedStyleSheets)]
        method deleteAdoptedStyleSheet() = css_style_sheet::delete_adopted;
        // Web Animations'.
        attribute timeline: DocumentTimeline, get = animation::document_timeline;
        method getAnimations() -> Sequence<Animation> = animation::tree_get_animations;

        // Prerendering Revamped: machopo never prerenders, so nothing fires
        // `prerenderingchange`.
        attribute prerendering: bool, get = document::prerendering;
        #[front]
        attribute onprerenderingchange: EventHandler, get = handlers::document_handler::<{ event_type::PRERENDERINGCHANGE }>, set = handlers::set_document_handler::<_, { event_type::PRERENDERINGCHANGE }>;
    }

    interface DocumentType : Node [DocumentType] {
        attribute name: DomString, get = document_type::name;
        attribute publicId: DomString, get = document_type::public_id;
        attribute systemId: DomString, get = document_type::system_id;
        // `ChildNode`'s.
        #[ce_reactions]
        method before(nodes?: NodesOrStrings) = child_node::before;
        #[ce_reactions]
        method after(nodes?: NodesOrStrings) = child_node::after;
        #[ce_reactions]
        method replaceWith(nodes?: NodesOrStrings) = child_node::replace_with;
        #[ce_reactions]
        method remove() = child_node::remove;
    }

    interface DocumentFragment : Node [DocumentFragment] {
        #[front]
        constructor() = document::construct_document_fragment;

        attribute children: HTMLCollection, get = parent_node::children;
        attribute firstElementChild: Option<Element>, get = parent_node::first_element_child;
        attribute lastElementChild: Option<Element>, get = parent_node::last_element_child;
        attribute childElementCount: u32, get = parent_node::child_element_count;
        method querySelector(selectors: DomString) -> Option<Element> = query::query_selector;
        method querySelectorAll(selectors: DomString) -> NodeList = query::query_selector_all;
        #[ce_reactions]
        method append(nodes?: NodesOrStrings) = parent_node::append;
        #[ce_reactions]
        method prepend(nodes?: NodesOrStrings) = parent_node::prepend;
        #[ce_reactions]
        method replaceChildren(nodes?: NodesOrStrings) = parent_node::replace_children;
        method getElementById(elementId: DomString) -> Option<Element> = query::get_element_by_id;
    }

    // A document fragment of no kind of its own: the most derived
    // interface of the fragments that are shadow roots (`typed`'s
    // `node_interface_of`), as Blink's `ShadowRoot` is a `DocumentFragment`.
    // No `activeElement` or `getHTML()` yet.
    interface ShadowRoot : DocumentFragment [] {
        // DocumentOrShadowRoot's.
        attribute activeElement: Option<Element>, get = focus::shadow_active_element;
        method elementFromPoint(x: f64, y: f64) -> Option<Element> = geometry::shadow_element_from_point;
        method elementsFromPoint(x: f64, y: f64) -> Sequence<Element> = geometry::elements_from_point;
        // DocumentOrShadowRoot's (CSSOM's).
        attribute styleSheets: StyleSheetList, get = css_style_sheet::style_sheets;
        attribute adoptedStyleSheets: ObservableArray<CSSStyleSheet>, get = css_style_sheet::adopted;
        #[observable(set, adoptedStyleSheets)]
        method setAdoptedStyleSheet(index: u32, sheet: CSSStyleSheet) = css_style_sheet::set_adopted;
        #[observable(delete, adoptedStyleSheets)]
        method deleteAdoptedStyleSheet() = css_style_sheet::delete_adopted;
        // `mode` and `slotAssignment`: WebIDL enums, as strings.
        attribute mode: DomString, get = shadow_root::mode;
        attribute delegatesFocus: bool, get = shadow_root::delegates_focus;
        attribute slotAssignment: DomString, get = shadow_root::slot_assignment;
        attribute clonable: bool, get = shadow_root::clonable;
        attribute serializable: bool, get = shadow_root::serializable;
        attribute host: Element, get = shadow_root::host;
        #[ce_reactions]
        attribute innerHTML: LegacyNullToEmptyString, get = markup::inner_html, set = markup::set_shadow_inner_html;
        method getHTML(options?: GetHtmlOptionsArg) -> DomString = markup::get_html;
        #[ce_reactions]
        method setHTMLUnsafe(html: DomString) = markup::set_shadow_html_unsafe;
        method getAnimations() -> Sequence<Animation> = animation::tree_get_animations;
    }

    // `mode` is required: empty here when missing, which `attachShadow`
    // throws for. WebIDL enums as strings.
    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary ShadowRootInit {
        clonable: bool = false,
        delegatesFocus: bool = false,
        mode: String = String::new(),
        serializable: bool = false,
        slotAssignment: String = String::from("named"),
    }

    interface Element : Node [Element] {
        attribute namespaceURI: Option<DomString>, get = element::namespace_uri;
        attribute prefix: Option<DomString>, get = element::prefix;
        attribute localName: DomString, get = element::local_name;
        attribute tagName: DomString, get = element::tag_name;

        #[ce_reactions]
        attribute id: DomString, get = element::id, set = element::set_id;
        #[ce_reactions]
        attribute className: DomString, get = element::class_name, set = element::set_class_name;
        // `Slottable`'s, and its `slot`.
        #[ce_reactions]
        attribute slot: DomString, get = slot::slot, set = slot::set_slot;

        attribute children: HTMLCollection, get = parent_node::children;
        attribute firstElementChild: Option<Element>, get = parent_node::first_element_child;
        attribute lastElementChild: Option<Element>, get = parent_node::last_element_child;
        attribute childElementCount: u32, get = parent_node::child_element_count;
        // `NonDocumentTypeChildNode`'s.
        attribute previousElementSibling: Option<Element>, get = child_node::previous_element_sibling;
        attribute nextElementSibling: Option<Element>, get = child_node::next_element_sibling;

        method getAttribute(qualifiedName: DomString) -> Option<DomString> = element::get_attribute;
        #[ce_reactions]
        method setAttribute(qualifiedName: DomString, value: DomString) = element::set_attribute;
        #[ce_reactions]
        method removeAttribute(qualifiedName: DomString) = element::remove_attribute;
        method hasAttribute(qualifiedName: DomString) -> bool = element::has_attribute;
        method hasAttributes() -> bool = element::has_attributes;
        method getAttributeNames() -> Strings = element::get_attribute_names;
        attribute attributes: NamedNodeMap, get = attr::attributes;
        method getAttributeNode(qualifiedName: DomString) -> Option<Attr> = attr::get_attribute_node;
        method getAttributeNodeNS(namespace: Option<DomString>, localName: DomString) -> Option<Attr> =
            attr::get_attribute_node_ns;
        #[ce_reactions]
        method setAttributeNode(attr: Attr) -> Option<Attr> = attr::set_attribute_node;
        #[ce_reactions]
        method setAttributeNodeNS(attr: Attr) -> Option<Attr> = attr::set_attribute_node;
        #[ce_reactions]
        method removeAttributeNode(attr: Attr) -> Attr = attr::remove_attribute_node;
        method getAttributeNS(namespace: Option<DomString>, localName: DomString) -> Option<DomString> =
            element::get_attribute_ns;
        #[ce_reactions]
        method setAttributeNS(namespace: Option<DomString>, qualifiedName: DomString, value: DomString) =
            element::set_attribute_ns;
        #[ce_reactions]
        method removeAttributeNS(namespace: Option<DomString>, localName: DomString) =
            element::remove_attribute_ns;
        #[ce_reactions]
        method toggleAttribute(qualifiedName: DomString, force?: Option<bool>) -> bool = element::toggle_attribute;
        method hasAttributeNS(namespace: Option<DomString>, localName: DomString) -> bool =
            element::has_attribute_ns;
        method matches(selectors: DomString) -> bool = query::matches;
        method webkitMatchesSelector(selectors: DomString) -> bool = query::matches;
        method closest(selectors: DomString) -> Option<Element> = query::closest;
        method getElementsByTagName(qualifiedName: DomString) -> HTMLCollection =
            collection::get_elements_by_tag_name;
        method getElementsByTagNameNS(namespace: Option<DomString>, localName: DomString) -> HTMLCollection =
            element::get_elements_by_tag_name_ns;
        method getElementsByClassName(classNames: DomString) -> HTMLCollection =
            collection::get_elements_by_class_name;
        method querySelector(selectors: DomString) -> Option<Element> = query::query_selector;
        method querySelectorAll(selectors: DomString) -> NodeList = query::query_selector_all;
        #[ce_reactions]
        method append(nodes?: NodesOrStrings) = parent_node::append;
        #[ce_reactions]
        method prepend(nodes?: NodesOrStrings) = parent_node::prepend;
        #[ce_reactions]
        method replaceChildren(nodes?: NodesOrStrings) = parent_node::replace_children;
        // `[PutForwards=value]`.
        #[ce_reactions]
        attribute classList: DOMTokenList, get = token_list::class_list,
            set: DomString = token_list::set_class_list;
        #[ce_reactions]
        attribute innerHTML: LegacyNullToEmptyString, get = markup::inner_html, set = markup::set_inner_html;
        #[ce_reactions]
        attribute outerHTML: LegacyNullToEmptyString, get = markup::outer_html, set = markup::set_outer_html;
        #[ce_reactions]
        method insertAdjacentHTML(position: DomString, string: DomString) = markup::insert_adjacent_html;
        #[ce_reactions]
        method insertAdjacentElement(r#where: DomString, element: Element) -> Option<Element> =
            element::insert_adjacent_element;
        method insertAdjacentText(r#where: DomString, data: DomString) = element::insert_adjacent_text;
        method getHTML(options?: GetHtmlOptionsArg) -> DomString = markup::get_html;
        #[ce_reactions]
        method setHTMLUnsafe(html: DomString) = markup::set_html_unsafe;
        method attachShadow(init: ShadowRootInit) -> ShadowRoot = shadow_root::attach_shadow;
        attribute shadowRoot: Option<ShadowRoot>, get = shadow_root::shadow_root;
        attribute assignedSlot: Option<HTMLSlotElement>, get = slot::assigned_slot;
        // `ChildNode`'s.
        #[ce_reactions]
        method before(nodes?: NodesOrStrings) = child_node::before;
        #[ce_reactions]
        method after(nodes?: NodesOrStrings) = child_node::after;
        #[ce_reactions]
        method replaceWith(nodes?: NodesOrStrings) = child_node::replace_with;
        #[ce_reactions]
        method remove() = child_node::remove;

        // CSSOM View's: its boxes as laid out, and scrolling them (every
        // scroll instant).
        method getClientRects() -> DOMRectList = geometry::get_client_rects;
        method getBoundingClientRect() -> DOMRect = geometry::get_bounding_client_rect;
        attribute scrollTop: UnrestrictedDouble, get = geometry::scroll_top, set = scroll::set_scroll_top;
        attribute scrollLeft: UnrestrictedDouble, get = geometry::scroll_left, set = scroll::set_scroll_left;
        method scroll(options?: ScrollToOptions) -> PromiseOfUndefined = scroll::element_scroll_to;
        method scroll(x: UnrestrictedDouble, y: UnrestrictedDouble) -> PromiseOfUndefined = scroll::element_scroll_to_xy;
        method scrollTo(options?: ScrollToOptions) -> PromiseOfUndefined = scroll::element_scroll_to;
        method scrollTo(x: UnrestrictedDouble, y: UnrestrictedDouble) -> PromiseOfUndefined = scroll::element_scroll_to_xy;
        method scrollBy(options?: ScrollToOptions) -> PromiseOfUndefined = scroll::element_scroll_by;
        method scrollBy(x: UnrestrictedDouble, y: UnrestrictedDouble) -> PromiseOfUndefined = scroll::element_scroll_by_xy;
        method scrollIntoView(arg?: DictOrBool<ScrollIntoViewOptions>) -> PromiseOfUndefined = scroll::scroll_into_view;
        // WebIDL `long`.
        attribute scrollWidth: i32, get = geometry::scroll_width;
        attribute scrollHeight: i32, get = geometry::scroll_height;
        attribute clientTop: i32, get = geometry::client_top;
        attribute clientLeft: i32, get = geometry::client_left;
        attribute clientWidth: i32, get = geometry::client_width;
        attribute clientHeight: i32, get = geometry::client_height;
        // Web Animations' `Animatable`.
        method animate(keyframes: Keyframes, options?: AnimationOptions) -> Animation = animation::animate;
        method getAnimations(options?: GetAnimationsOptions) -> Sequence<Animation> = animation::element_get_animations;
    }

    // HTML's elements: an HTML element's interface by its local name.
    interface HTMLElement : Element [element html] {
        // `ElementCSSInlineStyle`'s; `[PutForwards=cssText]`.
        attribute style: CSSStyleProperties, get = css_style::style, set: DomString = css_style::set_style;

        // CSSOM View's. WebIDL `long`.
        attribute offsetParent: Option<Element>, get = geometry::offset_parent;
        attribute offsetTop: i32, get = geometry::offset_top;
        attribute offsetLeft: i32, get = geometry::offset_left;
        attribute offsetWidth: i32, get = geometry::offset_width;
        attribute offsetHeight: i32, get = geometry::offset_height;

        // HTML's `GlobalEventHandlers`: a `<body>`'s or `<frameset>`'s window-reflecting ones are its window's.
        #[front]
        attribute onabort: EventHandler, get = handlers::element_handler::<{ event_type::ABORT }>, set = handlers::set_element_handler::<_, { event_type::ABORT }>;
        #[front]
        attribute onanimationcancel: EventHandler, get = handlers::element_handler::<{ event_type::ANIMATIONCANCEL }>, set = handlers::set_element_handler::<_, { event_type::ANIMATIONCANCEL }>;
        #[front]
        attribute onanimationend: EventHandler, get = handlers::element_handler::<{ event_type::ANIMATIONEND }>, set = handlers::set_element_handler::<_, { event_type::ANIMATIONEND }>;
        #[front]
        attribute onanimationiteration: EventHandler, get = handlers::element_handler::<{ event_type::ANIMATIONITERATION }>, set = handlers::set_element_handler::<_, { event_type::ANIMATIONITERATION }>;
        #[front]
        attribute onanimationstart: EventHandler, get = handlers::element_handler::<{ event_type::ANIMATIONSTART }>, set = handlers::set_element_handler::<_, { event_type::ANIMATIONSTART }>;
        #[front]
        attribute onauxclick: EventHandler, get = handlers::element_handler::<{ event_type::AUXCLICK }>, set = handlers::set_element_handler::<_, { event_type::AUXCLICK }>;
        #[front]
        attribute onbeforeinput: EventHandler, get = handlers::element_handler::<{ event_type::BEFOREINPUT }>, set = handlers::set_element_handler::<_, { event_type::BEFOREINPUT }>;
        #[front]
        attribute onbeforematch: EventHandler, get = handlers::element_handler::<{ event_type::BEFOREMATCH }>, set = handlers::set_element_handler::<_, { event_type::BEFOREMATCH }>;
        #[front]
        attribute onbeforetoggle: EventHandler, get = handlers::element_handler::<{ event_type::BEFORETOGGLE }>, set = handlers::set_element_handler::<_, { event_type::BEFORETOGGLE }>;
        #[front]
        attribute onblur: EventHandler, get = handlers::element_handler::<{ event_type::BLUR }>, set = handlers::set_element_handler::<_, { event_type::BLUR }>;
        #[front]
        attribute oncancel: EventHandler, get = handlers::element_handler::<{ event_type::CANCEL }>, set = handlers::set_element_handler::<_, { event_type::CANCEL }>;
        #[front]
        attribute oncanplay: EventHandler, get = handlers::element_handler::<{ event_type::CANPLAY }>, set = handlers::set_element_handler::<_, { event_type::CANPLAY }>;
        #[front]
        attribute oncanplaythrough: EventHandler, get = handlers::element_handler::<{ event_type::CANPLAYTHROUGH }>, set = handlers::set_element_handler::<_, { event_type::CANPLAYTHROUGH }>;
        #[front]
        attribute onchange: EventHandler, get = handlers::element_handler::<{ event_type::CHANGE }>, set = handlers::set_element_handler::<_, { event_type::CHANGE }>;
        #[front]
        attribute onclick: EventHandler, get = handlers::element_handler::<{ event_type::CLICK }>, set = handlers::set_element_handler::<_, { event_type::CLICK }>;
        #[front]
        attribute onclose: EventHandler, get = handlers::element_handler::<{ event_type::CLOSE }>, set = handlers::set_element_handler::<_, { event_type::CLOSE }>;
        #[front]
        attribute oncommand: EventHandler, get = handlers::element_handler::<{ event_type::COMMAND }>, set = handlers::set_element_handler::<_, { event_type::COMMAND }>;
        #[front]
        attribute oncontentvisibilityautostatechange: EventHandler, get = handlers::element_handler::<{ event_type::CONTENTVISIBILITYAUTOSTATECHANGE }>, set = handlers::set_element_handler::<_, { event_type::CONTENTVISIBILITYAUTOSTATECHANGE }>;
        #[front]
        attribute oncontextlost: EventHandler, get = handlers::element_handler::<{ event_type::CONTEXTLOST }>, set = handlers::set_element_handler::<_, { event_type::CONTEXTLOST }>;
        #[front]
        attribute oncontextmenu: EventHandler, get = handlers::element_handler::<{ event_type::CONTEXTMENU }>, set = handlers::set_element_handler::<_, { event_type::CONTEXTMENU }>;
        #[front]
        attribute oncontextrestored: EventHandler, get = handlers::element_handler::<{ event_type::CONTEXTRESTORED }>, set = handlers::set_element_handler::<_, { event_type::CONTEXTRESTORED }>;
        #[front]
        attribute oncuechange: EventHandler, get = handlers::element_handler::<{ event_type::CUECHANGE }>, set = handlers::set_element_handler::<_, { event_type::CUECHANGE }>;
        #[front]
        attribute ondblclick: EventHandler, get = handlers::element_handler::<{ event_type::DBLCLICK }>, set = handlers::set_element_handler::<_, { event_type::DBLCLICK }>;
        #[front]
        attribute ondrag: EventHandler, get = handlers::element_handler::<{ event_type::DRAG }>, set = handlers::set_element_handler::<_, { event_type::DRAG }>;
        #[front]
        attribute ondragend: EventHandler, get = handlers::element_handler::<{ event_type::DRAGEND }>, set = handlers::set_element_handler::<_, { event_type::DRAGEND }>;
        #[front]
        attribute ondragenter: EventHandler, get = handlers::element_handler::<{ event_type::DRAGENTER }>, set = handlers::set_element_handler::<_, { event_type::DRAGENTER }>;
        #[front]
        attribute ondragleave: EventHandler, get = handlers::element_handler::<{ event_type::DRAGLEAVE }>, set = handlers::set_element_handler::<_, { event_type::DRAGLEAVE }>;
        #[front]
        attribute ondragover: EventHandler, get = handlers::element_handler::<{ event_type::DRAGOVER }>, set = handlers::set_element_handler::<_, { event_type::DRAGOVER }>;
        #[front]
        attribute ondragstart: EventHandler, get = handlers::element_handler::<{ event_type::DRAGSTART }>, set = handlers::set_element_handler::<_, { event_type::DRAGSTART }>;
        #[front]
        attribute ondrop: EventHandler, get = handlers::element_handler::<{ event_type::DROP }>, set = handlers::set_element_handler::<_, { event_type::DROP }>;
        #[front]
        attribute ondurationchange: EventHandler, get = handlers::element_handler::<{ event_type::DURATIONCHANGE }>, set = handlers::set_element_handler::<_, { event_type::DURATIONCHANGE }>;
        #[front]
        attribute onemptied: EventHandler, get = handlers::element_handler::<{ event_type::EMPTIED }>, set = handlers::set_element_handler::<_, { event_type::EMPTIED }>;
        #[front]
        attribute onended: EventHandler, get = handlers::element_handler::<{ event_type::ENDED }>, set = handlers::set_element_handler::<_, { event_type::ENDED }>;
        #[front]
        attribute onerror: EventHandler, get = handlers::element_handler::<{ event_type::ERROR }>, set = handlers::set_element_handler::<_, { event_type::ERROR }>;
        #[front]
        attribute onfocus: EventHandler, get = handlers::element_handler::<{ event_type::FOCUS }>, set = handlers::set_element_handler::<_, { event_type::FOCUS }>;
        #[front]
        attribute onformdata: EventHandler, get = handlers::element_handler::<{ event_type::FORMDATA }>, set = handlers::set_element_handler::<_, { event_type::FORMDATA }>;
        #[front]
        attribute oninput: EventHandler, get = handlers::element_handler::<{ event_type::INPUT }>, set = handlers::set_element_handler::<_, { event_type::INPUT }>;
        #[front]
        attribute oninvalid: EventHandler, get = handlers::element_handler::<{ event_type::INVALID }>, set = handlers::set_element_handler::<_, { event_type::INVALID }>;
        #[front]
        attribute onkeydown: EventHandler, get = handlers::element_handler::<{ event_type::KEYDOWN }>, set = handlers::set_element_handler::<_, { event_type::KEYDOWN }>;
        #[front]
        attribute onkeypress: EventHandler, get = handlers::element_handler::<{ event_type::KEYPRESS }>, set = handlers::set_element_handler::<_, { event_type::KEYPRESS }>;
        #[front]
        attribute onkeyup: EventHandler, get = handlers::element_handler::<{ event_type::KEYUP }>, set = handlers::set_element_handler::<_, { event_type::KEYUP }>;
        #[front]
        attribute onload: EventHandler, get = handlers::element_handler::<{ event_type::LOAD }>, set = handlers::set_element_handler::<_, { event_type::LOAD }>;
        #[front]
        attribute onloadeddata: EventHandler, get = handlers::element_handler::<{ event_type::LOADEDDATA }>, set = handlers::set_element_handler::<_, { event_type::LOADEDDATA }>;
        #[front]
        attribute onloadedmetadata: EventHandler, get = handlers::element_handler::<{ event_type::LOADEDMETADATA }>, set = handlers::set_element_handler::<_, { event_type::LOADEDMETADATA }>;
        #[front]
        attribute onloadstart: EventHandler, get = handlers::element_handler::<{ event_type::LOADSTART }>, set = handlers::set_element_handler::<_, { event_type::LOADSTART }>;
        #[front]
        attribute onmousedown: EventHandler, get = handlers::element_handler::<{ event_type::MOUSEDOWN }>, set = handlers::set_element_handler::<_, { event_type::MOUSEDOWN }>;
        #[front]
        attribute onmouseenter: EventHandler, get = handlers::element_handler::<{ event_type::MOUSEENTER }>, set = handlers::set_element_handler::<_, { event_type::MOUSEENTER }>;
        #[front]
        attribute onmouseleave: EventHandler, get = handlers::element_handler::<{ event_type::MOUSELEAVE }>, set = handlers::set_element_handler::<_, { event_type::MOUSELEAVE }>;
        #[front]
        attribute onmousemove: EventHandler, get = handlers::element_handler::<{ event_type::MOUSEMOVE }>, set = handlers::set_element_handler::<_, { event_type::MOUSEMOVE }>;
        #[front]
        attribute onmouseout: EventHandler, get = handlers::element_handler::<{ event_type::MOUSEOUT }>, set = handlers::set_element_handler::<_, { event_type::MOUSEOUT }>;
        #[front]
        attribute onmouseover: EventHandler, get = handlers::element_handler::<{ event_type::MOUSEOVER }>, set = handlers::set_element_handler::<_, { event_type::MOUSEOVER }>;
        #[front]
        attribute onmouseup: EventHandler, get = handlers::element_handler::<{ event_type::MOUSEUP }>, set = handlers::set_element_handler::<_, { event_type::MOUSEUP }>;
        #[front]
        attribute onmousewheel: EventHandler, get = handlers::element_handler::<{ event_type::MOUSEWHEEL }>, set = handlers::set_element_handler::<_, { event_type::MOUSEWHEEL }>;
        #[front]
        attribute onpause: EventHandler, get = handlers::element_handler::<{ event_type::PAUSE }>, set = handlers::set_element_handler::<_, { event_type::PAUSE }>;
        #[front]
        attribute onplay: EventHandler, get = handlers::element_handler::<{ event_type::PLAY }>, set = handlers::set_element_handler::<_, { event_type::PLAY }>;
        #[front]
        attribute onplaying: EventHandler, get = handlers::element_handler::<{ event_type::PLAYING }>, set = handlers::set_element_handler::<_, { event_type::PLAYING }>;
        #[front]
        attribute onprogress: EventHandler, get = handlers::element_handler::<{ event_type::PROGRESS }>, set = handlers::set_element_handler::<_, { event_type::PROGRESS }>;
        #[front]
        attribute onratechange: EventHandler, get = handlers::element_handler::<{ event_type::RATECHANGE }>, set = handlers::set_element_handler::<_, { event_type::RATECHANGE }>;
        #[front]
        attribute onreset: EventHandler, get = handlers::element_handler::<{ event_type::RESET }>, set = handlers::set_element_handler::<_, { event_type::RESET }>;
        #[front]
        attribute onresize: EventHandler, get = handlers::element_handler::<{ event_type::RESIZE }>, set = handlers::set_element_handler::<_, { event_type::RESIZE }>;
        #[front]
        attribute onscroll: EventHandler, get = handlers::element_handler::<{ event_type::SCROLL }>, set = handlers::set_element_handler::<_, { event_type::SCROLL }>;
        #[front]
        attribute onscrollend: EventHandler, get = handlers::element_handler::<{ event_type::SCROLLEND }>, set = handlers::set_element_handler::<_, { event_type::SCROLLEND }>;
        #[front]
        attribute onsecuritypolicyviolation: EventHandler, get = handlers::element_handler::<{ event_type::SECURITYPOLICYVIOLATION }>, set = handlers::set_element_handler::<_, { event_type::SECURITYPOLICYVIOLATION }>;
        #[front]
        attribute onseeked: EventHandler, get = handlers::element_handler::<{ event_type::SEEKED }>, set = handlers::set_element_handler::<_, { event_type::SEEKED }>;
        #[front]
        attribute onseeking: EventHandler, get = handlers::element_handler::<{ event_type::SEEKING }>, set = handlers::set_element_handler::<_, { event_type::SEEKING }>;
        #[front]
        attribute onselect: EventHandler, get = handlers::element_handler::<{ event_type::SELECT }>, set = handlers::set_element_handler::<_, { event_type::SELECT }>;
        #[front]
        attribute onselectionchange: EventHandler, get = handlers::element_handler::<{ event_type::SELECTIONCHANGE }>, set = handlers::set_element_handler::<_, { event_type::SELECTIONCHANGE }>;
        #[front]
        attribute onselectstart: EventHandler, get = handlers::element_handler::<{ event_type::SELECTSTART }>, set = handlers::set_element_handler::<_, { event_type::SELECTSTART }>;
        #[front]
        attribute onslotchange: EventHandler, get = handlers::element_handler::<{ event_type::SLOTCHANGE }>, set = handlers::set_element_handler::<_, { event_type::SLOTCHANGE }>;
        #[front]
        attribute onstalled: EventHandler, get = handlers::element_handler::<{ event_type::STALLED }>, set = handlers::set_element_handler::<_, { event_type::STALLED }>;
        #[front]
        attribute onsubmit: EventHandler, get = handlers::element_handler::<{ event_type::SUBMIT }>, set = handlers::set_element_handler::<_, { event_type::SUBMIT }>;
        #[front]
        attribute onsuspend: EventHandler, get = handlers::element_handler::<{ event_type::SUSPEND }>, set = handlers::set_element_handler::<_, { event_type::SUSPEND }>;
        #[front]
        attribute ontimeupdate: EventHandler, get = handlers::element_handler::<{ event_type::TIMEUPDATE }>, set = handlers::set_element_handler::<_, { event_type::TIMEUPDATE }>;
        #[front]
        attribute ontoggle: EventHandler, get = handlers::element_handler::<{ event_type::TOGGLE }>, set = handlers::set_element_handler::<_, { event_type::TOGGLE }>;
        #[front]
        attribute ontransitioncancel: EventHandler, get = handlers::element_handler::<{ event_type::TRANSITIONCANCEL }>, set = handlers::set_element_handler::<_, { event_type::TRANSITIONCANCEL }>;
        #[front]
        attribute ontransitionend: EventHandler, get = handlers::element_handler::<{ event_type::TRANSITIONEND }>, set = handlers::set_element_handler::<_, { event_type::TRANSITIONEND }>;
        #[front]
        attribute ontransitionrun: EventHandler, get = handlers::element_handler::<{ event_type::TRANSITIONRUN }>, set = handlers::set_element_handler::<_, { event_type::TRANSITIONRUN }>;
        #[front]
        attribute ontransitionstart: EventHandler, get = handlers::element_handler::<{ event_type::TRANSITIONSTART }>, set = handlers::set_element_handler::<_, { event_type::TRANSITIONSTART }>;
        #[front]
        attribute onvolumechange: EventHandler, get = handlers::element_handler::<{ event_type::VOLUMECHANGE }>, set = handlers::set_element_handler::<_, { event_type::VOLUMECHANGE }>;
        #[front]
        attribute onwaiting: EventHandler, get = handlers::element_handler::<{ event_type::WAITING }>, set = handlers::set_element_handler::<_, { event_type::WAITING }>;
        #[front]
        attribute onwebkitanimationend: EventHandler, get = handlers::element_handler::<{ event_type::WEBKIT_ANIMATION_END }>, set = handlers::set_element_handler::<_, { event_type::WEBKIT_ANIMATION_END }>;
        #[front]
        attribute onwebkitanimationiteration: EventHandler, get = handlers::element_handler::<{ event_type::WEBKIT_ANIMATION_ITERATION }>, set = handlers::set_element_handler::<_, { event_type::WEBKIT_ANIMATION_ITERATION }>;
        #[front]
        attribute onwebkitanimationstart: EventHandler, get = handlers::element_handler::<{ event_type::WEBKIT_ANIMATION_START }>, set = handlers::set_element_handler::<_, { event_type::WEBKIT_ANIMATION_START }>;
        #[front]
        attribute onwebkittransitionend: EventHandler, get = handlers::element_handler::<{ event_type::WEBKIT_TRANSITION_END }>, set = handlers::set_element_handler::<_, { event_type::WEBKIT_TRANSITION_END }>;
        #[front]
        attribute onwheel: EventHandler, get = handlers::element_handler::<{ event_type::WHEEL }>, set = handlers::set_element_handler::<_, { event_type::WHEEL }>;
        #[ce_reactions]
        attribute title: DomString, get = reflect::string::<{ local::TITLE }>, set = reflect::set_string::<{ local::TITLE }>;
        #[ce_reactions]
        attribute lang: DomString, get = reflect::string::<{ local::LANG }>, set = reflect::set_string::<{ local::LANG }>;
        #[ce_reactions]
        attribute translate: bool, get = html::translate, set = html::set_translate;
        #[ce_reactions]
        attribute dir: DomString, get = html::dir, set = reflect::set_string::<{ local::DIR }>;
        #[ce_reactions]
        attribute hidden: Hidden, get = html::hidden, set = html::set_hidden;
        #[ce_reactions]
        attribute inert: bool, get = reflect::boolean::<{ local::INERT }>, set = reflect::set_boolean::<{ local::INERT }>;
        // HTML's user interaction.
        #[calls_script]
        method click() = activation::click;
        #[ce_reactions]
        attribute accessKey: DomString, get = reflect::string::<{ local::ACCESSKEY }>, set = reflect::set_string::<{ local::ACCESSKEY }>;
        #[ce_reactions]
        attribute draggable: bool, get = html::draggable, set = html::set_draggable;
        #[ce_reactions]
        attribute spellcheck: bool, get = html::spellcheck, set = html::set_spellcheck;
        #[ce_reactions]
        attribute autocapitalize: DomString, get = html::autocapitalize, set = html::set_autocapitalize;
        // `ElementContentEditable`'s.
        #[ce_reactions]
        attribute contentEditable: DomString, get = html::content_editable, set = html::set_content_editable;
        #[ce_reactions]
        attribute enterKeyHint: DomString, get = html::enter_key_hint, set = html::set_enter_key_hint;
        attribute isContentEditable: bool, get = html::is_content_editable;
        #[ce_reactions]
        attribute inputMode: DomString, get = html::input_mode, set = reflect::set_string::<{ local::INPUTMODE }>;

        #[ce_reactions]
        attribute autofocus: bool, get = reflect::boolean::<{ local::AUTOFOCUS }>, set = reflect::set_boolean::<{ local::AUTOFOCUS }>;

        // `[HTMLConstructor]`: a custom element's `super()`.
        #[front]
        constructor() = custom_elements::html_constructor;

        // `HTMLOrSVGElement`'s focus, which dispatches the focus events.
        #[ce_reactions]
        attribute tabIndex: i32, get = focus::tab_index, set = focus::set_tab_index;
        #[calls_script]
        method focus(options?: FocusOptions) = focus::focus;
        #[calls_script]
        method blur() = focus::blur;
        // `HTMLOrSVGElement`'s.
        attribute dataset: DOMStringMap, get = dataset::dataset;
    }

    dictionary AssignedNodesOptions {
        flatten: bool = false,
    }

    interface HTMLSlotElement : HTMLElement [element html slot] {
        #[ce_reactions]
        attribute name: DomString, get = html::name, set = html::set_name;
        method assignedNodes(options?: AssignedNodesOptions) -> Sequence<Node> = slot::assigned_nodes;
        method assignedElements(options?: AssignedNodesOptions) -> Sequence<Element> = slot::assigned_elements;
        method assign(nodes?: Slottables) = slot::assign;
    }

    interface HTMLTemplateElement : HTMLElement [element html template] {
        attribute content: DocumentFragment, get = html::template_content;
        #[ce_reactions]
        attribute shadowRootMode: DomString, get = html::shadow_root_mode, set = html::set_shadow_root_mode;
        #[ce_reactions]
        attribute shadowRootDelegatesFocus: bool, get = html::shadow_root_delegates_focus,
            set = html::set_shadow_root_delegates_focus;
        #[ce_reactions]
        attribute shadowRootClonable: bool, get = html::shadow_root_clonable, set = html::set_shadow_root_clonable;
        #[ce_reactions]
        attribute shadowRootSerializable: bool, get = html::shadow_root_serializable,
            set = html::set_shadow_root_serializable;
    }

    interface HTMLMetaElement : HTMLElement [element html meta] {
        #[ce_reactions]
        attribute name: DomString, get = html::name, set = html::set_name;
        #[ce_reactions]
        attribute content: DomString, get = html::content, set = html::set_content;
    }

    // `LinkStyle`'s `sheet` (CSSOM's); no `blocking`.
    interface HTMLStyleElement : HTMLElement [element html style] {
        attribute disabled: bool, get = css_style_sheet::style_disabled, set = css_style_sheet::set_style_disabled;
        attribute sheet: Option<CSSStyleSheet>, get = css_style_sheet::sheet;
        #[ce_reactions]
        attribute media: DomString, get = reflect::string::<{ local::MEDIA }>, set = reflect::set_string::<{ local::MEDIA }>;
        // Obsolete.
        #[ce_reactions]
        attribute type: DomString, get = reflect::string::<{ local::TYPE }>, set = reflect::set_string::<{ local::TYPE }>;
    }

    interface HTMLHtmlElement : HTMLElement [element html html] {
        #[ce_reactions]
        attribute version: DomString, get = reflect::string::<{ local::VERSION }>, set = reflect::set_string::<{ local::VERSION }>;
    }

    interface HTMLHeadElement : HTMLElement [element html head] {
    }

    interface HTMLTitleElement : HTMLElement [element html title] {
        #[ce_reactions]
        attribute text: DomString, get = hyperlink::text, set = hyperlink::set_text;
    }

    interface HTMLBodyElement : HTMLElement [element html body] {
        #[ce_reactions]
        attribute text: DomString, get = reflect::string::<{ local::TEXT }>, set: LegacyNullToEmptyString = reflect::set_string::<{ local::TEXT }>;
        #[ce_reactions]
        attribute link: DomString, get = reflect::string::<{ local::LINK }>, set: LegacyNullToEmptyString = reflect::set_string::<{ local::LINK }>;
        #[ce_reactions]
        attribute vLink: DomString, get = reflect::string::<{ local::VLINK }>, set: LegacyNullToEmptyString = reflect::set_string::<{ local::VLINK }>;
        #[ce_reactions]
        attribute aLink: DomString, get = reflect::string::<{ local::ALINK }>, set: LegacyNullToEmptyString = reflect::set_string::<{ local::ALINK }>;
        #[ce_reactions]
        attribute bgColor: DomString, get = reflect::string::<{ local::BGCOLOR }>, set: LegacyNullToEmptyString = reflect::set_string::<{ local::BGCOLOR }>;
        #[ce_reactions]
        attribute background: DomString, get = reflect::string::<{ local::BACKGROUND }>, set = reflect::set_string::<{ local::BACKGROUND }>;
        // HTML's `WindowEventHandlers`: its window's.
        #[front]
        attribute onafterprint: EventHandler, get = handlers::element_handler::<{ event_type::AFTERPRINT }>, set = handlers::set_element_handler::<_, { event_type::AFTERPRINT }>;
        #[front]
        attribute onbeforeprint: EventHandler, get = handlers::element_handler::<{ event_type::BEFOREPRINT }>, set = handlers::set_element_handler::<_, { event_type::BEFOREPRINT }>;
        #[front]
        attribute onbeforeunload: EventHandler, get = handlers::element_handler::<{ event_type::BEFOREUNLOAD }>, set = handlers::set_element_handler::<_, { event_type::BEFOREUNLOAD }>;
        #[front]
        attribute onhashchange: EventHandler, get = handlers::element_handler::<{ event_type::HASHCHANGE }>, set = handlers::set_element_handler::<_, { event_type::HASHCHANGE }>;
        #[front]
        attribute onlanguagechange: EventHandler, get = handlers::element_handler::<{ event_type::LANGUAGECHANGE }>, set = handlers::set_element_handler::<_, { event_type::LANGUAGECHANGE }>;
        #[front]
        attribute onmessage: EventHandler, get = handlers::element_handler::<{ event_type::MESSAGE }>, set = handlers::set_element_handler::<_, { event_type::MESSAGE }>;
        #[front]
        attribute onmessageerror: EventHandler, get = handlers::element_handler::<{ event_type::MESSAGEERROR }>, set = handlers::set_element_handler::<_, { event_type::MESSAGEERROR }>;
        #[front]
        attribute onoffline: EventHandler, get = handlers::element_handler::<{ event_type::OFFLINE }>, set = handlers::set_element_handler::<_, { event_type::OFFLINE }>;
        #[front]
        attribute ononline: EventHandler, get = handlers::element_handler::<{ event_type::ONLINE }>, set = handlers::set_element_handler::<_, { event_type::ONLINE }>;
        #[front]
        attribute onpagehide: EventHandler, get = handlers::element_handler::<{ event_type::PAGEHIDE }>, set = handlers::set_element_handler::<_, { event_type::PAGEHIDE }>;
        #[front]
        attribute onpagereveal: EventHandler, get = handlers::element_handler::<{ event_type::PAGEREVEAL }>, set = handlers::set_element_handler::<_, { event_type::PAGEREVEAL }>;
        #[front]
        attribute onpageshow: EventHandler, get = handlers::element_handler::<{ event_type::PAGESHOW }>, set = handlers::set_element_handler::<_, { event_type::PAGESHOW }>;
        #[front]
        attribute onpageswap: EventHandler, get = handlers::element_handler::<{ event_type::PAGESWAP }>, set = handlers::set_element_handler::<_, { event_type::PAGESWAP }>;
        #[front]
        attribute onpopstate: EventHandler, get = handlers::element_handler::<{ event_type::POPSTATE }>, set = handlers::set_element_handler::<_, { event_type::POPSTATE }>;
        #[front]
        attribute onrejectionhandled: EventHandler, get = handlers::element_handler::<{ event_type::REJECTIONHANDLED }>, set = handlers::set_element_handler::<_, { event_type::REJECTIONHANDLED }>;
        #[front]
        attribute onstorage: EventHandler, get = handlers::element_handler::<{ event_type::STORAGE }>, set = handlers::set_element_handler::<_, { event_type::STORAGE }>;
        #[front]
        attribute onunhandledrejection: EventHandler, get = handlers::element_handler::<{ event_type::UNHANDLEDREJECTION }>, set = handlers::set_element_handler::<_, { event_type::UNHANDLEDREJECTION }>;
        #[front]
        attribute onunload: EventHandler, get = handlers::element_handler::<{ event_type::UNLOAD }>, set = handlers::set_element_handler::<_, { event_type::UNLOAD }>;
    }

    interface HTMLHeadingElement : HTMLElement [element html h1 h2 h3 h4 h5 h6] {
        #[ce_reactions]
        attribute align: DomString, get = reflect::string::<{ local::ALIGN }>, set = reflect::set_string::<{ local::ALIGN }>;
    }

    interface HTMLParagraphElement : HTMLElement [element html p] {
        #[ce_reactions]
        attribute align: DomString, get = reflect::string::<{ local::ALIGN }>, set = reflect::set_string::<{ local::ALIGN }>;
    }

    interface HTMLHRElement : HTMLElement [element html hr] {
        #[ce_reactions]
        attribute align: DomString, get = reflect::string::<{ local::ALIGN }>, set = reflect::set_string::<{ local::ALIGN }>;
        #[ce_reactions]
        attribute color: DomString, get = reflect::string::<{ local::COLOR }>, set = reflect::set_string::<{ local::COLOR }>;
        #[ce_reactions]
        attribute noShade: bool, get = reflect::boolean::<{ local::NOSHADE }>, set = reflect::set_boolean::<{ local::NOSHADE }>;
        #[ce_reactions]
        attribute size: DomString, get = reflect::string::<{ local::SIZE }>, set = reflect::set_string::<{ local::SIZE }>;
        #[ce_reactions]
        attribute width: DomString, get = reflect::string::<{ local::WIDTH }>, set = reflect::set_string::<{ local::WIDTH }>;
    }

    interface HTMLPreElement : HTMLElement [element html pre listing xmp] {
        #[ce_reactions]
        attribute width: i32, get = reflect::long::<{ local::WIDTH }, 0>, set = reflect::set_long::<{ local::WIDTH }>;
    }

    interface HTMLQuoteElement : HTMLElement [element html blockquote q] {
        #[ce_reactions]
        attribute cite: DomString, get = reflect::url::<{ local::CITE }>, set = reflect::set_string::<{ local::CITE }>;
    }

    interface HTMLOListElement : HTMLElement [element html ol] {
        #[ce_reactions]
        attribute reversed: bool, get = reflect::boolean::<{ local::REVERSED }>, set = reflect::set_boolean::<{ local::REVERSED }>;
        #[ce_reactions]
        attribute start: i32, get = reflect::long::<{ local::START }, 1>, set = reflect::set_long::<{ local::START }>;
        #[ce_reactions]
        attribute type: DomString, get = reflect::string::<{ local::TYPE }>, set = reflect::set_string::<{ local::TYPE }>;
        #[ce_reactions]
        attribute compact: bool, get = reflect::boolean::<{ local::COMPACT }>, set = reflect::set_boolean::<{ local::COMPACT }>;
    }

    interface HTMLUListElement : HTMLElement [element html ul] {
        #[ce_reactions]
        attribute compact: bool, get = reflect::boolean::<{ local::COMPACT }>, set = reflect::set_boolean::<{ local::COMPACT }>;
        #[ce_reactions]
        attribute type: DomString, get = reflect::string::<{ local::TYPE }>, set = reflect::set_string::<{ local::TYPE }>;
    }

    interface HTMLMenuElement : HTMLElement [element html menu] {
        #[ce_reactions]
        attribute compact: bool, get = reflect::boolean::<{ local::COMPACT }>, set = reflect::set_boolean::<{ local::COMPACT }>;
    }

    interface HTMLLIElement : HTMLElement [element html li] {
        #[ce_reactions]
        attribute value: i32, get = reflect::long::<{ local::VALUE }, 0>, set = reflect::set_long::<{ local::VALUE }>;
        #[ce_reactions]
        attribute type: DomString, get = reflect::string::<{ local::TYPE }>, set = reflect::set_string::<{ local::TYPE }>;
    }

    interface HTMLDListElement : HTMLElement [element html dl] {
        #[ce_reactions]
        attribute compact: bool, get = reflect::boolean::<{ local::COMPACT }>, set = reflect::set_boolean::<{ local::COMPACT }>;
    }

    interface HTMLDivElement : HTMLElement [element html div] {
        #[ce_reactions]
        attribute align: DomString, get = reflect::string::<{ local::ALIGN }>, set = reflect::set_string::<{ local::ALIGN }>;
    }

    interface HTMLDataElement : HTMLElement [element html data] {
        #[ce_reactions]
        attribute value: DomString, get = reflect::string::<{ local::VALUE }>, set = reflect::set_string::<{ local::VALUE }>;
    }

    interface HTMLTimeElement : HTMLElement [element html time] {
        #[ce_reactions]
        attribute dateTime: DomString, get = reflect::string::<{ local::DATETIME }>, set = reflect::set_string::<{ local::DATETIME }>;
    }

    interface HTMLSpanElement : HTMLElement [element html span] {
    }

    interface HTMLBRElement : HTMLElement [element html br] {
        #[ce_reactions]
        attribute clear: DomString, get = reflect::string::<{ local::CLEAR }>, set = reflect::set_string::<{ local::CLEAR }>;
    }

    interface HTMLModElement : HTMLElement [element html ins del] {
        #[ce_reactions]
        attribute cite: DomString, get = reflect::url::<{ local::CITE }>, set = reflect::set_string::<{ local::CITE }>;
        #[ce_reactions]
        attribute dateTime: DomString, get = reflect::string::<{ local::DATETIME }>, set = reflect::set_string::<{ local::DATETIME }>;
    }

    interface HTMLDirectoryElement : HTMLElement [element html dir] {
        #[ce_reactions]
        attribute compact: bool, get = reflect::boolean::<{ local::COMPACT }>, set = reflect::set_boolean::<{ local::COMPACT }>;
    }

    interface HTMLFontElement : HTMLElement [element html font] {
        #[ce_reactions]
        attribute color: DomString, get = reflect::string::<{ local::COLOR }>, set: LegacyNullToEmptyString = reflect::set_string::<{ local::COLOR }>;
        #[ce_reactions]
        attribute face: DomString, get = reflect::string::<{ local::FACE }>, set = reflect::set_string::<{ local::FACE }>;
        #[ce_reactions]
        attribute size: DomString, get = reflect::string::<{ local::SIZE }>, set = reflect::set_string::<{ local::SIZE }>;
    }

    interface HTMLParamElement : HTMLElement [element html param] {
        #[ce_reactions]
        attribute name: DomString, get = reflect::string::<{ local::NAME }>, set = reflect::set_string::<{ local::NAME }>;
        #[ce_reactions]
        attribute value: DomString, get = reflect::string::<{ local::VALUE }>, set = reflect::set_string::<{ local::VALUE }>;
        #[ce_reactions]
        attribute type: DomString, get = reflect::string::<{ local::TYPE }>, set = reflect::set_string::<{ local::TYPE }>;
        #[ce_reactions]
        attribute valueType: DomString, get = reflect::string::<{ local::VALUETYPE }>, set = reflect::set_string::<{ local::VALUETYPE }>;
    }

    interface HTMLMapElement : HTMLElement [element html map] {
        #[ce_reactions]
        attribute name: DomString, get = reflect::string::<{ local::NAME }>, set = reflect::set_string::<{ local::NAME }>;
        attribute areas: HTMLCollection, get = table::map_areas;
    }

    interface HTMLTableElement : HTMLElement [element html table] {
        #[ce_reactions]
        attribute caption: Option<HTMLTableCaptionElement>, get = table::caption, set = table::set_caption;
        method createCaption() -> HTMLTableCaptionElement = table::create_caption;
        #[ce_reactions]
        method deleteCaption() = table::delete_caption;
        #[ce_reactions]
        attribute tHead: Option<HTMLTableSectionElement>, get = table::t_head, set = table::set_t_head;
        method createTHead() -> HTMLTableSectionElement = table::create_t_head;
        #[ce_reactions]
        method deleteTHead() = table::delete_t_head;
        #[ce_reactions]
        attribute tFoot: Option<HTMLTableSectionElement>, get = table::t_foot, set = table::set_t_foot;
        method createTFoot() -> HTMLTableSectionElement = table::create_t_foot;
        #[ce_reactions]
        method deleteTFoot() = table::delete_t_foot;
        attribute tBodies: HTMLCollection, get = table::t_bodies;
        method createTBody() -> HTMLTableSectionElement = table::create_t_body;
        attribute rows: HTMLCollection, get = table::table_rows;
        method insertRow(index?: LongDefaultMinusOne) -> HTMLTableRowElement = table::insert_row;
        #[ce_reactions]
        method deleteRow(index: i32) = table::delete_row;
        // Obsolete.
        #[ce_reactions]
        attribute align: DomString, get = reflect::string::<{ local::ALIGN }>, set = reflect::set_string::<{ local::ALIGN }>;
        #[ce_reactions]
        attribute border: DomString, get = reflect::string::<{ local::BORDER }>, set = reflect::set_string::<{ local::BORDER }>;
        #[ce_reactions]
        attribute frame: DomString, get = reflect::string::<{ local::FRAME }>, set = reflect::set_string::<{ local::FRAME }>;
        #[ce_reactions]
        attribute rules: DomString, get = reflect::string::<{ local::RULES }>, set = reflect::set_string::<{ local::RULES }>;
        #[ce_reactions]
        attribute summary: DomString, get = reflect::string::<{ local::SUMMARY }>, set = reflect::set_string::<{ local::SUMMARY }>;
        #[ce_reactions]
        attribute width: DomString, get = reflect::string::<{ local::WIDTH }>, set = reflect::set_string::<{ local::WIDTH }>;
        #[ce_reactions]
        attribute bgColor: DomString, get = reflect::string::<{ local::BGCOLOR }>, set: LegacyNullToEmptyString = reflect::set_string::<{ local::BGCOLOR }>;
        #[ce_reactions]
        attribute cellPadding: DomString, get = reflect::string::<{ local::CELLPADDING }>, set: LegacyNullToEmptyString = reflect::set_string::<{ local::CELLPADDING }>;
        #[ce_reactions]
        attribute cellSpacing: DomString, get = reflect::string::<{ local::CELLSPACING }>, set: LegacyNullToEmptyString = reflect::set_string::<{ local::CELLSPACING }>;
    }

    interface HTMLTableCaptionElement : HTMLElement [element html caption] {
        #[ce_reactions]
        attribute align: DomString, get = reflect::string::<{ local::ALIGN }>, set = reflect::set_string::<{ local::ALIGN }>;
    }

    interface HTMLTableColElement : HTMLElement [element html col colgroup] {
        #[ce_reactions]
        attribute span: u32, get = reflect::clamped::<{ local::SPAN }, 1, 1, 1000>, set = reflect::set_unsigned::<{ local::SPAN }, 1>;
        // Obsolete.
        #[ce_reactions]
        attribute align: DomString, get = reflect::string::<{ local::ALIGN }>, set = reflect::set_string::<{ local::ALIGN }>;
        #[ce_reactions]
        attribute ch: DomString, get = reflect::string::<{ local::CHAR }>, set = reflect::set_string::<{ local::CHAR }>;
        #[ce_reactions]
        attribute chOff: DomString, get = reflect::string::<{ local::CHAROFF }>, set = reflect::set_string::<{ local::CHAROFF }>;
        #[ce_reactions]
        attribute vAlign: DomString, get = reflect::string::<{ local::VALIGN }>, set = reflect::set_string::<{ local::VALIGN }>;
        #[ce_reactions]
        attribute width: DomString, get = reflect::string::<{ local::WIDTH }>, set = reflect::set_string::<{ local::WIDTH }>;
    }

    interface HTMLTableSectionElement : HTMLElement [element html thead tbody tfoot] {
        attribute rows: HTMLCollection, get = table::section_rows;
        method insertRow(index?: LongDefaultMinusOne) -> HTMLTableRowElement = table::section_insert_row;
        #[ce_reactions]
        method deleteRow(index: i32) = table::section_delete_row;
        // Obsolete.
        #[ce_reactions]
        attribute align: DomString, get = reflect::string::<{ local::ALIGN }>, set = reflect::set_string::<{ local::ALIGN }>;
        #[ce_reactions]
        attribute ch: DomString, get = reflect::string::<{ local::CHAR }>, set = reflect::set_string::<{ local::CHAR }>;
        #[ce_reactions]
        attribute chOff: DomString, get = reflect::string::<{ local::CHAROFF }>, set = reflect::set_string::<{ local::CHAROFF }>;
        #[ce_reactions]
        attribute vAlign: DomString, get = reflect::string::<{ local::VALIGN }>, set = reflect::set_string::<{ local::VALIGN }>;
    }

    interface HTMLTableRowElement : HTMLElement [element html tr] {
        attribute rowIndex: i32, get = table::row_index;
        attribute sectionRowIndex: i32, get = table::section_row_index;
        attribute cells: HTMLCollection, get = table::cells;
        method insertCell(index?: LongDefaultMinusOne) -> HTMLTableCellElement = table::insert_cell;
        #[ce_reactions]
        method deleteCell(index: i32) = table::delete_cell;
        // Obsolete.
        #[ce_reactions]
        attribute align: DomString, get = reflect::string::<{ local::ALIGN }>, set = reflect::set_string::<{ local::ALIGN }>;
        #[ce_reactions]
        attribute ch: DomString, get = reflect::string::<{ local::CHAR }>, set = reflect::set_string::<{ local::CHAR }>;
        #[ce_reactions]
        attribute chOff: DomString, get = reflect::string::<{ local::CHAROFF }>, set = reflect::set_string::<{ local::CHAROFF }>;
        #[ce_reactions]
        attribute vAlign: DomString, get = reflect::string::<{ local::VALIGN }>, set = reflect::set_string::<{ local::VALIGN }>;
        #[ce_reactions]
        attribute bgColor: DomString, get = reflect::string::<{ local::BGCOLOR }>, set: LegacyNullToEmptyString = reflect::set_string::<{ local::BGCOLOR }>;
    }

    interface HTMLTableCellElement : HTMLElement [element html td th] {
        #[ce_reactions]
        attribute colSpan: u32, get = reflect::clamped::<{ local::COLSPAN }, 1, 1, 1000>, set = reflect::set_unsigned::<{ local::COLSPAN }, 1>;
        #[ce_reactions]
        attribute rowSpan: u32, get = reflect::clamped::<{ local::ROWSPAN }, 1, 0, 65534>, set = reflect::set_unsigned::<{ local::ROWSPAN }, 1>;
        #[ce_reactions]
        attribute headers: DomString, get = reflect::string::<{ local::HEADERS }>, set = reflect::set_string::<{ local::HEADERS }>;
        attribute cellIndex: i32, get = table::cell_index;
        #[ce_reactions]
        attribute scope: DomString, get = table::scope, set = reflect::set_string::<{ local::SCOPE }>;
        #[ce_reactions]
        attribute abbr: DomString, get = reflect::string::<{ local::ABBR }>, set = reflect::set_string::<{ local::ABBR }>;
        // Obsolete.
        #[ce_reactions]
        attribute align: DomString, get = reflect::string::<{ local::ALIGN }>, set = reflect::set_string::<{ local::ALIGN }>;
        #[ce_reactions]
        attribute axis: DomString, get = reflect::string::<{ local::AXIS }>, set = reflect::set_string::<{ local::AXIS }>;
        #[ce_reactions]
        attribute height: DomString, get = reflect::string::<{ local::HEIGHT }>, set = reflect::set_string::<{ local::HEIGHT }>;
        #[ce_reactions]
        attribute width: DomString, get = reflect::string::<{ local::WIDTH }>, set = reflect::set_string::<{ local::WIDTH }>;
        #[ce_reactions]
        attribute ch: DomString, get = reflect::string::<{ local::CHAR }>, set = reflect::set_string::<{ local::CHAR }>;
        #[ce_reactions]
        attribute chOff: DomString, get = reflect::string::<{ local::CHAROFF }>, set = reflect::set_string::<{ local::CHAROFF }>;
        #[ce_reactions]
        attribute noWrap: bool, get = reflect::boolean::<{ local::NOWRAP }>, set = reflect::set_boolean::<{ local::NOWRAP }>;
        #[ce_reactions]
        attribute vAlign: DomString, get = reflect::string::<{ local::VALIGN }>, set = reflect::set_string::<{ local::VALIGN }>;
        #[ce_reactions]
        attribute bgColor: DomString, get = reflect::string::<{ local::BGCOLOR }>, set: LegacyNullToEmptyString = reflect::set_string::<{ local::BGCOLOR }>;
    }

    // With `HTMLHyperlinkElementUtils` (not `hrefTranslate` and
    // `interestForElement`, Chrome's experiments).
    interface HTMLAnchorElement : HTMLElement [element html a] {
        #[ce_reactions]
        attribute target: DomString, get = reflect::string::<{ local::TARGET }>, set = reflect::set_string::<{ local::TARGET }>;
        #[ce_reactions]
        attribute download: DomString, get = reflect::string::<{ local::DOWNLOAD }>, set = reflect::set_string::<{ local::DOWNLOAD }>;
        #[ce_reactions]
        attribute ping: DomString, get = reflect::string::<{ local::PING }>, set = reflect::set_string::<{ local::PING }>;
        #[ce_reactions]
        attribute rel: DomString, get = reflect::string::<{ local::REL }>, set = reflect::set_string::<{ local::REL }>;
        attribute relList: DOMTokenList, get = token_list::rel_list, set: DomString = token_list::set_rel_list;
        #[ce_reactions]
        attribute hreflang: DomString, get = reflect::string::<{ local::HREFLANG }>, set = reflect::set_string::<{ local::HREFLANG }>;
        #[ce_reactions]
        attribute type: DomString, get = reflect::string::<{ local::TYPE }>, set = reflect::set_string::<{ local::TYPE }>;
        #[ce_reactions]
        attribute text: DomString, get = hyperlink::text, set = hyperlink::set_text;
        #[ce_reactions]
        attribute referrerPolicy: DomString, get = image::referrer_policy,
            set = reflect::set_string::<{ local::REFERRERPOLICY }>;
        // Obsolete.
        #[ce_reactions]
        attribute coords: DomString, get = reflect::string::<{ local::COORDS }>, set = reflect::set_string::<{ local::COORDS }>;
        #[ce_reactions]
        attribute charset: DomString, get = reflect::string::<{ local::CHARSET }>, set = reflect::set_string::<{ local::CHARSET }>;
        #[ce_reactions]
        attribute name: DomString, get = reflect::string::<{ local::NAME }>, set = reflect::set_string::<{ local::NAME }>;
        #[ce_reactions]
        attribute rev: DomString, get = reflect::string::<{ local::REV }>, set = reflect::set_string::<{ local::REV }>;
        #[ce_reactions]
        attribute shape: DomString, get = reflect::string::<{ local::SHAPE }>, set = reflect::set_string::<{ local::SHAPE }>;
        // `HTMLHyperlinkElementUtils`.
        #[ce_reactions]
        attribute href: DomString, get = hyperlink::href, set = reflect::set_string::<{ local::HREF }>;
        method toString() -> DomString = hyperlink::href;
        attribute origin: DomString, get = hyperlink::origin;
        #[ce_reactions]
        attribute protocol: DomString, get = hyperlink::protocol, set = hyperlink::set_protocol;
        #[ce_reactions]
        attribute username: DomString, get = hyperlink::username, set = hyperlink::set_username;
        #[ce_reactions]
        attribute password: DomString, get = hyperlink::password, set = hyperlink::set_password;
        #[ce_reactions]
        attribute host: DomString, get = hyperlink::host, set = hyperlink::set_host;
        #[ce_reactions]
        attribute hostname: DomString, get = hyperlink::hostname, set = hyperlink::set_hostname;
        #[ce_reactions]
        attribute port: DomString, get = hyperlink::port, set = hyperlink::set_port;
        #[ce_reactions]
        attribute pathname: DomString, get = hyperlink::pathname, set = hyperlink::set_pathname;
        #[ce_reactions]
        attribute search: DomString, get = hyperlink::search, set = hyperlink::set_search;
        #[ce_reactions]
        attribute hash: DomString, get = hyperlink::hash, set = hyperlink::set_hash;
    }

    // With `HTMLHyperlinkElementUtils`: the members only, no image map.
    interface HTMLAreaElement : HTMLElement [element html area] {
        #[ce_reactions]
        attribute alt: DomString, get = reflect::string::<{ local::ALT }>, set = reflect::set_string::<{ local::ALT }>;
        #[ce_reactions]
        attribute coords: DomString, get = reflect::string::<{ local::COORDS }>, set = reflect::set_string::<{ local::COORDS }>;
        #[ce_reactions]
        attribute shape: DomString, get = reflect::string::<{ local::SHAPE }>, set = reflect::set_string::<{ local::SHAPE }>;
        #[ce_reactions]
        attribute target: DomString, get = reflect::string::<{ local::TARGET }>, set = reflect::set_string::<{ local::TARGET }>;
        #[ce_reactions]
        attribute download: DomString, get = reflect::string::<{ local::DOWNLOAD }>, set = reflect::set_string::<{ local::DOWNLOAD }>;
        #[ce_reactions]
        attribute ping: DomString, get = reflect::string::<{ local::PING }>, set = reflect::set_string::<{ local::PING }>;
        #[ce_reactions]
        attribute rel: DomString, get = reflect::string::<{ local::REL }>, set = reflect::set_string::<{ local::REL }>;
        attribute relList: DOMTokenList, get = token_list::rel_list, set: DomString = token_list::set_rel_list;
        #[ce_reactions]
        attribute referrerPolicy: DomString, get = image::referrer_policy,
            set = reflect::set_string::<{ local::REFERRERPOLICY }>;
        // Obsolete.
        #[ce_reactions]
        attribute noHref: bool, get = reflect::boolean::<{ local::NOHREF }>, set = reflect::set_boolean::<{ local::NOHREF }>;
        // `HTMLHyperlinkElementUtils`.
        #[ce_reactions]
        attribute href: DomString, get = hyperlink::href, set = reflect::set_string::<{ local::HREF }>;
        method toString() -> DomString = hyperlink::href;
        attribute origin: DomString, get = hyperlink::origin;
        #[ce_reactions]
        attribute protocol: DomString, get = hyperlink::protocol, set = hyperlink::set_protocol;
        #[ce_reactions]
        attribute username: DomString, get = hyperlink::username, set = hyperlink::set_username;
        #[ce_reactions]
        attribute password: DomString, get = hyperlink::password, set = hyperlink::set_password;
        #[ce_reactions]
        attribute host: DomString, get = hyperlink::host, set = hyperlink::set_host;
        #[ce_reactions]
        attribute hostname: DomString, get = hyperlink::hostname, set = hyperlink::set_hostname;
        #[ce_reactions]
        attribute port: DomString, get = hyperlink::port, set = hyperlink::set_port;
        #[ce_reactions]
        attribute pathname: DomString, get = hyperlink::pathname, set = hyperlink::set_pathname;
        #[ce_reactions]
        attribute search: DomString, get = hyperlink::search, set = hyperlink::set_search;
        #[ce_reactions]
        attribute hash: DomString, get = hyperlink::hash, set = hyperlink::set_hash;
    }

    interface HTMLBaseElement : HTMLElement [element html base] {
        #[ce_reactions]
        attribute href: DomString, get = hyperlink::base_href, set = reflect::set_string::<{ local::HREF }>;
        #[ce_reactions]
        attribute target: DomString, get = reflect::string::<{ local::TARGET }>, set = reflect::set_string::<{ local::TARGET }>;
    }

    // `LinkStyle`'s `sheet` (CSSOM's); no `sizes`, `imageSrcset`,
    // `imageSizes`, `blocking` or `fetchPriority`.
    interface HTMLLinkElement : HTMLElement [element html link] {
        #[ce_reactions]
        attribute href: DomString, get = reflect::url::<{ local::HREF }>, set = reflect::set_string::<{ local::HREF }>;
        #[ce_reactions]
        attribute crossOrigin: Option<DomString>, get = image::cross_origin, set = image::set_cross_origin;
        #[ce_reactions]
        attribute rel: DomString, get = reflect::string::<{ local::REL }>, set = reflect::set_string::<{ local::REL }>;
        #[ce_reactions]
        attribute as: DomString, get = html::link_as, set = reflect::set_string::<{ local::AS }>;
        attribute relList: DOMTokenList, get = token_list::rel_list, set: DomString = token_list::set_rel_list;
        #[ce_reactions]
        attribute integrity: DomString, get = reflect::string::<{ local::INTEGRITY }>,
            set = reflect::set_string::<{ local::INTEGRITY }>;
        #[ce_reactions]
        attribute hreflang: DomString, get = reflect::string::<{ local::HREFLANG }>,
            set = reflect::set_string::<{ local::HREFLANG }>;
        #[ce_reactions]
        attribute media: DomString, get = reflect::string::<{ local::MEDIA }>, set = reflect::set_string::<{ local::MEDIA }>;
        #[ce_reactions]
        attribute type: DomString, get = reflect::string::<{ local::TYPE }>, set = reflect::set_string::<{ local::TYPE }>;
        #[ce_reactions]
        attribute referrerPolicy: DomString, get = image::referrer_policy,
            set = reflect::set_string::<{ local::REFERRERPOLICY }>;
        #[ce_reactions]
        attribute disabled: bool, get = reflect::boolean::<{ local::DISABLED }>,
            set = reflect::set_boolean::<{ local::DISABLED }>;
        attribute sheet: Option<CSSStyleSheet>, get = css_style_sheet::sheet;
        // Obsolete.
        #[ce_reactions]
        attribute charset: DomString, get = reflect::string::<{ local::CHARSET }>,
            set = reflect::set_string::<{ local::CHARSET }>;
        #[ce_reactions]
        attribute rev: DomString, get = reflect::string::<{ local::REV }>, set = reflect::set_string::<{ local::REV }>;
        #[ce_reactions]
        attribute target: DomString, get = reflect::string::<{ local::TARGET }>,
            set = reflect::set_string::<{ local::TARGET }>;
    }

    // HTML's scripting (`core/html/`, `core/script/`): no `fetchPriority`,
    // `blocking` or `attributionSrc`.
    interface HTMLScriptElement : HTMLElement [element html script] {
        #[static_method]
        method supports(r#type: DomString) -> bool = script::script_supports;
        #[ce_reactions]
        attribute type: DomString, get = reflect::string::<{ local::TYPE }>, set = reflect::set_string::<{ local::TYPE }>;
        #[ce_reactions]
        attribute src: DomString, get = reflect::url::<{ local::SRC }>, set = reflect::set_string::<{ local::SRC }>;
        #[ce_reactions]
        attribute noModule: bool, get = reflect::boolean::<{ local::NOMODULE }>,
            set = reflect::set_boolean::<{ local::NOMODULE }>;
        #[ce_reactions]
        attribute async: bool, get = script::script_async, set = script::set_script_async;
        #[ce_reactions]
        attribute defer: bool, get = reflect::boolean::<{ local::DEFER }>, set = reflect::set_boolean::<{ local::DEFER }>;
        #[ce_reactions]
        attribute crossOrigin: Option<DomString>, get = image::cross_origin, set = image::set_cross_origin;
        #[ce_reactions]
        attribute text: DomString, get = script::script_text, set = script::set_script_text;
        #[ce_reactions]
        attribute integrity: DomString, get = reflect::string::<{ local::INTEGRITY }>,
            set = reflect::set_string::<{ local::INTEGRITY }>;
        #[ce_reactions]
        attribute referrerPolicy: DomString, get = image::referrer_policy,
            set = reflect::set_string::<{ local::REFERRERPOLICY }>;
        // Obsolete.
        #[ce_reactions]
        attribute charset: DomString, get = reflect::string::<{ local::CHARSET }>,
            set = reflect::set_string::<{ local::CHARSET }>;
        #[ce_reactions]
        attribute event: DomString, get = reflect::string::<{ local::EVENT }>, set = reflect::set_string::<{ local::EVENT }>;
        #[ce_reactions]
        attribute htmlFor: DomString, get = reflect::string::<{ local::FOR }>, set = reflect::set_string::<{ local::FOR }>;
    }

    // HTML's forms (`core/html/forms/`): the form controls' interfaces and
    // their reflected attributes; the rest of their members as the form
    // segments make them.

    // An HTML element whose name HTML doesn't define, or a failed custom element.
    interface HTMLUnknownElement : HTMLElement [element html] {
    }

    // HTML's embedded content (`core/html/`): `<img>`.
    // No `loading`, `sandbox` or `allow`.
    interface HTMLIFrameElement : HTMLElement [element html iframe] {
        #[ce_reactions]
        attribute src: DomString, get = reflect::url::<{ local::SRC }>, set = reflect::set_string::<{ local::SRC }>;
        #[ce_reactions]
        attribute srcdoc: DomString, get = reflect::string::<{ local::SRCDOC }>, set = reflect::set_string::<{ local::SRCDOC }>;
        #[ce_reactions]
        attribute name: DomString, get = reflect::string::<{ local::NAME }>, set = reflect::set_string::<{ local::NAME }>;
        #[ce_reactions]
        attribute width: DomString, get = reflect::string::<{ local::WIDTH }>, set = reflect::set_string::<{ local::WIDTH }>;
        #[ce_reactions]
        attribute height: DomString, get = reflect::string::<{ local::HEIGHT }>, set = reflect::set_string::<{ local::HEIGHT }>;
        // Its navigation timing reads it (a redirect's `Timing-Allow-Origin`).
        #[ce_reactions]
        attribute referrerPolicy: DomString, get = image::referrer_policy, set = reflect::set_string::<{ local::REFERRERPOLICY }>;
        attribute contentDocument: Option<Document>, get = frame::content_document;
        attribute contentWindow: Option<Window>, get = frame::content_window;
    }

    interface HTMLImageElement : HTMLElement [element html img] {
        #[front]
        #[legacy_factory]
        method Image(width?: Option<u32>, height?: Option<u32>) -> HTMLImageElement = image::construct_image;
        #[ce_reactions]
        attribute alt: DomString, get = reflect::string::<{ local::ALT }>, set = reflect::set_string::<{ local::ALT }>;
        #[ce_reactions]
        attribute src: DomString, get = reflect::url::<{ local::SRC }>, set = reflect::set_string::<{ local::SRC }>;
        #[ce_reactions]
        attribute srcset: DomString, get = reflect::string::<{ local::SRCSET }>, set = reflect::set_string::<{ local::SRCSET }>;
        #[ce_reactions]
        attribute sizes: DomString, get = reflect::string::<{ local::SIZES }>, set = reflect::set_string::<{ local::SIZES }>;
        #[ce_reactions]
        attribute crossOrigin: Option<DomString>, get = image::cross_origin, set = image::set_cross_origin;
        #[ce_reactions]
        attribute useMap: DomString, get = reflect::string::<{ local::USEMAP }>, set = reflect::set_string::<{ local::USEMAP }>;
        #[ce_reactions]
        attribute isMap: bool, get = reflect::boolean::<{ local::ISMAP }>, set = reflect::set_boolean::<{ local::ISMAP }>;
        #[ce_reactions]
        attribute width: u32, get = image::width, set = reflect::set_unsigned::<{ local::WIDTH }, 0>;
        #[ce_reactions]
        attribute height: u32, get = image::height, set = reflect::set_unsigned::<{ local::HEIGHT }, 0>;
        attribute naturalWidth: u32, get = image::natural_width;
        attribute naturalHeight: u32, get = image::natural_height;
        attribute complete: bool, get = image::complete;
        attribute currentSrc: DomString, get = image::current_src;
        #[ce_reactions]
        attribute referrerPolicy: DomString, get = image::referrer_policy, set = reflect::set_string::<{ local::REFERRERPOLICY }>;
        #[ce_reactions]
        attribute decoding: DomString, get = image::decoding, set = reflect::set_string::<{ local::DECODING }>;
        #[ce_reactions]
        attribute loading: DomString, get = image::loading, set = reflect::set_string::<{ local::LOADING }>;
        #[ce_reactions]
        attribute fetchPriority: DomString, get = image::fetch_priority, set = reflect::set_string::<{ local::FETCHPRIORITY }>;
        method decode() -> PromiseOfAny = image::decode;
        // HTML's obsolete members.
        #[ce_reactions]
        attribute name: DomString, get = reflect::string::<{ local::NAME }>, set = reflect::set_string::<{ local::NAME }>;
        #[ce_reactions]
        attribute lowsrc: DomString, get = reflect::url::<{ local::LOWSRC }>, set = reflect::set_string::<{ local::LOWSRC }>;
        #[ce_reactions]
        attribute align: DomString, get = reflect::string::<{ local::ALIGN }>, set = reflect::set_string::<{ local::ALIGN }>;
        #[ce_reactions]
        attribute hspace: u32, get = reflect::unsigned::<{ local::HSPACE }, 0>, set = reflect::set_unsigned::<{ local::HSPACE }, 0>;
        #[ce_reactions]
        attribute vspace: u32, get = reflect::unsigned::<{ local::VSPACE }, 0>, set = reflect::set_unsigned::<{ local::VSPACE }, 0>;
        #[ce_reactions]
        attribute longDesc: DomString, get = reflect::url::<{ local::LONGDESC }>, set = reflect::set_string::<{ local::LONGDESC }>;
        #[ce_reactions]
        attribute border: DomString, get = reflect::string::<{ local::BORDER }>, set = reflect::set_string::<{ local::BORDER }>;
    }

    // `<picture>`: no members of its own.
    interface HTMLPictureElement : HTMLElement [element html picture] {
    }

    // `<source>` (a `<picture>`'s, which its `<img>` selects among).
    interface HTMLSourceElement : HTMLElement [element html source] {
        #[ce_reactions]
        attribute src: DomString, get = reflect::url::<{ local::SRC }>, set = reflect::set_string::<{ local::SRC }>;
        #[ce_reactions]
        attribute r#type: DomString, get = reflect::string::<{ local::TYPE }>, set = reflect::set_string::<{ local::TYPE }>;
        #[ce_reactions]
        attribute srcset: DomString, get = reflect::string::<{ local::SRCSET }>, set = reflect::set_string::<{ local::SRCSET }>;
        #[ce_reactions]
        attribute sizes: DomString, get = reflect::string::<{ local::SIZES }>, set = reflect::set_string::<{ local::SIZES }>;
        #[ce_reactions]
        attribute media: DomString, get = reflect::string::<{ local::MEDIA }>, set = reflect::set_string::<{ local::MEDIA }>;
        #[ce_reactions]
        attribute width: u32, get = reflect::unsigned::<{ local::WIDTH }, 0>, set = reflect::set_unsigned::<{ local::WIDTH }, 0>;
        #[ce_reactions]
        attribute height: u32, get = reflect::unsigned::<{ local::HEIGHT }, 0>, set = reflect::set_unsigned::<{ local::HEIGHT }, 0>;
    }

    // `<canvas>`: its 2D context (no others, no `toDataURL` or `toBlob`
    // yet).
    interface HTMLCanvasElement : HTMLElement [element html canvas] {
        #[ce_reactions]
        attribute width: u32, get = reflect::unsigned::<{ local::WIDTH }, 300>, set = reflect::set_unsigned::<{ local::WIDTH }, 300>;
        #[ce_reactions]
        attribute height: u32, get = reflect::unsigned::<{ local::HEIGHT }, 150>, set = reflect::set_unsigned::<{ local::HEIGHT }, 150>;
        method getContext(
            contextId: DomString,
            options?: CanvasRenderingContext2DSettings,
        ) -> Option<CanvasRenderingContext2D> = canvas::get_context;
        // WebIDL `USVString`: the URL is ASCII.
        method toDataURL(r#type?: OptionalString, quality?: NumberIfNumber) -> DomString =
            canvas::to_data_url;
        #[front]
        method toBlob(callback: BlobCallback, r#type?: OptionalString, quality?: NumberIfNumber) =
            canvas::to_blob;
    }

    // `colorSpace` and `colorType`: their enumerations' values, checked
    // by `getContext`.
    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary CanvasRenderingContext2DSettings {
        alpha: bool = true,
        colorSpace: String = String::from("srgb"),
        colorType: String = String::from("unorm8"),
        desynchronized: bool = false,
        willReadFrequently: bool = false,
    }

    // `[LegacyOverrideBuiltIns]`; its named properties aren't listed (as
    // `[LegacyUnenumerableNamedProperties]` has them not enumerable, but
    // not in the own keys either yet).
    #[override_builtins]
    interface HTMLFormElement : HTMLElement [element html form] {
        #[ce_reactions]
        attribute acceptCharset: DomString, get = reflect::string::<{ local::ACCEPT_CHARSET }>, set = reflect::set_string::<{ local::ACCEPT_CHARSET }>;
        #[ce_reactions]
        attribute action: DomString, get = reflect::action_url::<{ local::ACTION }>, set = reflect::set_string::<{ local::ACTION }>;
        #[ce_reactions]
        attribute autocomplete: DomString, get = forms::form_autocomplete, set = reflect::set_string::<{ local::AUTOCOMPLETE }>;
        #[ce_reactions]
        attribute enctype: DomString, get = forms::enctype, set = reflect::set_string::<{ local::ENCTYPE }>;
        #[ce_reactions]
        attribute encoding: DomString, get = forms::enctype, set = reflect::set_string::<{ local::ENCTYPE }>;
        #[ce_reactions]
        attribute method: DomString, get = forms::method, set = reflect::set_string::<{ local::METHOD }>;
        #[ce_reactions]
        attribute name: DomString, get = reflect::string::<{ local::NAME }>, set = reflect::set_string::<{ local::NAME }>;
        #[ce_reactions]
        attribute noValidate: bool, get = reflect::boolean::<{ local::NOVALIDATE }>, set = reflect::set_boolean::<{ local::NOVALIDATE }>;
        #[ce_reactions]
        attribute target: DomString, get = reflect::string::<{ local::TARGET }>, set = reflect::set_string::<{ local::TARGET }>;
        #[ce_reactions]
        attribute rel: DomString, get = reflect::string::<{ local::REL }>, set = reflect::set_string::<{ local::REL }>;
        // `[PutForwards=value]`.
        #[ce_reactions]
        attribute relList: DOMTokenList, get = token_list::rel_list, set: DomString = token_list::set_rel_list;
        attribute elements: HTMLFormControlsCollection, get = forms::elements;
        // WebIDL `unsigned long`.
        attribute length: u32, get = forms::length;
        #[indexed(getter)]
        method indexedGetter(index: u32) -> Option<Element> = forms::item;
        #[named(getter)]
        method namedGetter(name: DomString) -> Option<RadioNodeListOrElement> = forms::named_item;
        #[ce_reactions]
        #[calls_script]
        method reset() = forms::reset;
    }

    interface HTMLInputElement : HTMLElement [element html input] {
        #[ce_reactions]
        attribute accept: DomString, get = reflect::string::<{ local::ACCEPT }>, set = reflect::set_string::<{ local::ACCEPT }>;
        #[ce_reactions]
        attribute alt: DomString, get = reflect::string::<{ local::ALT }>, set = reflect::set_string::<{ local::ALT }>;
        #[ce_reactions]
        attribute autocomplete: DomString, get = reflect::string::<{ local::AUTOCOMPLETE }>, set = reflect::set_string::<{ local::AUTOCOMPLETE }>;
        #[ce_reactions]
        attribute defaultChecked: bool, get = reflect::boolean::<{ local::CHECKED }>, set = reflect::set_boolean::<{ local::CHECKED }>;
        #[ce_reactions]
        attribute dirName: DomString, get = reflect::string::<{ local::DIRNAME }>, set = reflect::set_string::<{ local::DIRNAME }>;
        #[ce_reactions]
        attribute disabled: bool, get = reflect::boolean::<{ local::DISABLED }>, set = reflect::set_boolean::<{ local::DISABLED }>;
        attribute form: Option<HTMLFormElement>, get = forms::form;
        #[ce_reactions]
        attribute formAction: DomString, get = reflect::action_url::<{ local::FORMACTION }>, set = reflect::set_string::<{ local::FORMACTION }>;
        #[ce_reactions]
        attribute formEnctype: DomString, get = forms::form_enctype, set = reflect::set_string::<{ local::FORMENCTYPE }>;
        #[ce_reactions]
        attribute formMethod: DomString, get = forms::form_method, set = reflect::set_string::<{ local::FORMMETHOD }>;
        #[ce_reactions]
        attribute formNoValidate: bool, get = reflect::boolean::<{ local::FORMNOVALIDATE }>, set = reflect::set_boolean::<{ local::FORMNOVALIDATE }>;
        #[ce_reactions]
        attribute formTarget: DomString, get = reflect::string::<{ local::FORMTARGET }>, set = reflect::set_string::<{ local::FORMTARGET }>;
        #[ce_reactions]
        attribute height: u32, get = reflect::unsigned::<{ local::HEIGHT }, 0>, set = reflect::set_unsigned::<{ local::HEIGHT }, 0>;
        #[ce_reactions]
        attribute max: DomString, get = reflect::string::<{ local::MAX }>, set = reflect::set_string::<{ local::MAX }>;
        #[ce_reactions]
        attribute maxLength: i32, get = reflect::non_negative::<{ local::MAXLENGTH }, { -1 }>,
            set = reflect::set_non_negative::<{ local::MAXLENGTH }>;
        #[ce_reactions]
        attribute min: DomString, get = reflect::string::<{ local::MIN }>, set = reflect::set_string::<{ local::MIN }>;
        #[ce_reactions]
        attribute minLength: i32, get = reflect::non_negative::<{ local::MINLENGTH }, { -1 }>,
            set = reflect::set_non_negative::<{ local::MINLENGTH }>;
        #[ce_reactions]
        attribute multiple: bool, get = reflect::boolean::<{ local::MULTIPLE }>, set = reflect::set_boolean::<{ local::MULTIPLE }>;
        #[ce_reactions]
        attribute name: DomString, get = reflect::string::<{ local::NAME }>, set = reflect::set_string::<{ local::NAME }>;
        #[ce_reactions]
        attribute pattern: DomString, get = reflect::string::<{ local::PATTERN }>, set = reflect::set_string::<{ local::PATTERN }>;
        #[ce_reactions]
        attribute placeholder: DomString, get = reflect::string::<{ local::PLACEHOLDER }>, set = reflect::set_string::<{ local::PLACEHOLDER }>;
        #[ce_reactions]
        attribute readOnly: bool, get = reflect::boolean::<{ local::READONLY }>, set = reflect::set_boolean::<{ local::READONLY }>;
        #[ce_reactions]
        attribute required: bool, get = reflect::boolean::<{ local::REQUIRED }>, set = reflect::set_boolean::<{ local::REQUIRED }>;
        #[ce_reactions]
        attribute size: u32, get = reflect::positive::<{ local::SIZE }, 20>,
            set = reflect::set_positive::<{ local::SIZE }, 20, false>;
        #[ce_reactions]
        attribute src: DomString, get = reflect::url::<{ local::SRC }>, set = reflect::set_string::<{ local::SRC }>;
        #[ce_reactions]
        attribute step: DomString, get = reflect::string::<{ local::STEP }>, set = reflect::set_string::<{ local::STEP }>;
        #[ce_reactions]
        attribute r#type: DomString, get = forms::input_type, set = reflect::set_string::<{ local::TYPE }>;
        #[ce_reactions]
        attribute defaultValue: DomString, get = reflect::string::<{ local::VALUE }>, set = reflect::set_string::<{ local::VALUE }>;
        #[ce_reactions]
        attribute width: u32, get = reflect::unsigned::<{ local::WIDTH }, 0>, set = reflect::set_unsigned::<{ local::WIDTH }, 0>;
        attribute labels: Option<NodeList>, get = forms::input_labels;
        attribute checked: bool, get = forms::checked, set = forms::set_checked;
        attribute selectionStart: NullableU32, get = text_control::selection_start, set = text_control::set_selection_start;
        attribute selectionEnd: NullableU32, get = text_control::selection_end, set = text_control::set_selection_end;
        attribute selectionDirection: Option<DomString>,
            get = text_control::selection_direction, set = text_control::set_selection_direction;
        #[calls_script]
        method select() = text_control::select;
        method setSelectionRange(start: u32, end: u32, direction?: Option<DomString>) = text_control::set_selection_range_method;
        method setRangeText(replacement: DomString, start?: Option<u32>, end?: Option<u32>, selectionMode?: Option<DomString>) =
            text_control::set_range_text;
        attribute indeterminate: bool, get = forms::indeterminate, set = forms::set_indeterminate;
        #[ce_reactions]
        attribute value: LegacyNullToEmptyString, get = forms::input_value, set = forms::set_input_value;
        attribute valueAsNumber: UnrestrictedDouble, get = forms::value_as_number, set = forms::set_value_as_number;
        method stepUp(n?: LongDefaultOne) = forms::step_up;
        method stepDown(n?: LongDefaultOne) = forms::step_down;
    }

    interface HTMLButtonElement : HTMLElement [element html button] {
        #[ce_reactions]
        attribute disabled: bool, get = reflect::boolean::<{ local::DISABLED }>, set = reflect::set_boolean::<{ local::DISABLED }>;
        attribute form: Option<HTMLFormElement>, get = forms::form;
        #[ce_reactions]
        attribute formAction: DomString, get = reflect::action_url::<{ local::FORMACTION }>, set = reflect::set_string::<{ local::FORMACTION }>;
        #[ce_reactions]
        attribute formEnctype: DomString, get = forms::form_enctype, set = reflect::set_string::<{ local::FORMENCTYPE }>;
        #[ce_reactions]
        attribute formMethod: DomString, get = forms::form_method, set = reflect::set_string::<{ local::FORMMETHOD }>;
        #[ce_reactions]
        attribute formNoValidate: bool, get = reflect::boolean::<{ local::FORMNOVALIDATE }>, set = reflect::set_boolean::<{ local::FORMNOVALIDATE }>;
        #[ce_reactions]
        attribute formTarget: DomString, get = reflect::string::<{ local::FORMTARGET }>, set = reflect::set_string::<{ local::FORMTARGET }>;
        #[ce_reactions]
        attribute name: DomString, get = reflect::string::<{ local::NAME }>, set = reflect::set_string::<{ local::NAME }>;
        #[ce_reactions]
        attribute r#type: DomString, get = forms::button_type, set = reflect::set_string::<{ local::TYPE }>;
        #[ce_reactions]
        attribute value: DomString, get = reflect::string::<{ local::VALUE }>, set = reflect::set_string::<{ local::VALUE }>;
        attribute labels: NodeList, get = forms::labels;
    }

    interface HTMLSelectElement : HTMLElement [element html select] {
        #[ce_reactions]
        attribute autocomplete: DomString, get = reflect::string::<{ local::AUTOCOMPLETE }>, set = reflect::set_string::<{ local::AUTOCOMPLETE }>;
        #[ce_reactions]
        attribute disabled: bool, get = reflect::boolean::<{ local::DISABLED }>, set = reflect::set_boolean::<{ local::DISABLED }>;
        attribute form: Option<HTMLFormElement>, get = forms::form;
        #[ce_reactions]
        attribute multiple: bool, get = reflect::boolean::<{ local::MULTIPLE }>, set = reflect::set_boolean::<{ local::MULTIPLE }>;
        #[ce_reactions]
        attribute name: DomString, get = reflect::string::<{ local::NAME }>, set = reflect::set_string::<{ local::NAME }>;
        #[ce_reactions]
        attribute required: bool, get = reflect::boolean::<{ local::REQUIRED }>, set = reflect::set_boolean::<{ local::REQUIRED }>;
        #[ce_reactions]
        attribute size: u32, get = reflect::unsigned::<{ local::SIZE }, 0>, set = reflect::set_unsigned::<{ local::SIZE }, 0>;
        attribute r#type: DomString, get = select::select_type;
        attribute options: HTMLOptionsCollection, get = select::options;
        // WebIDL `unsigned long`.
        #[ce_reactions]
        attribute length: u32, get = select::length, set = select::set_length;
        #[indexed]
        method item(index: u32) -> Option<HTMLOptionElement> = select::item;
        method namedItem(name: DomString) -> Option<HTMLOptionElement> = select::named_item;
        #[ce_reactions]
        method add(element: OptionOrOptGroup, before?: NullableElementOrLong) = select::add;
        // `remove()` and `remove(index)`.
        #[ce_reactions]
        method remove(index?: Option<i32>) = select::remove;
        #[ce_reactions]
        #[indexed(setter)]
        method indexedSetter(index: u32, option: Option<HTMLOptionElement>) = select::set_item;
        attribute selectedOptions: HTMLCollection, get = select::selected_options;
        attribute selectedIndex: i32, get = select::selected_index, set = select::set_selected_index;
        attribute value: DomString, get = select::value, set = select::set_value;
        attribute labels: NodeList, get = forms::labels;
        #[calls_script]
        method showPicker() = menu_list::show_picker;
    }

    interface HTMLOptionElement : HTMLElement [element html option] {
        #[front]
        #[legacy_factory]
        method Option(
            text?: EmptyStringDefault,
            value?: OptionalString,
            defaultSelected?: bool,
            selected?: bool,
        ) -> HTMLOptionElement = select::construct_option;
        #[ce_reactions]
        attribute disabled: bool, get = reflect::boolean::<{ local::DISABLED }>, set = reflect::set_boolean::<{ local::DISABLED }>;
        attribute form: Option<HTMLFormElement>, get = select::option_form;
        #[ce_reactions]
        attribute label: DomString, get = select::label, set = reflect::set_string::<{ local::LABEL }>;
        #[ce_reactions]
        attribute defaultSelected: bool, get = reflect::boolean::<{ local::SELECTED }>, set = reflect::set_boolean::<{ local::SELECTED }>;
        attribute selected: bool, get = select::selected, set = select::set_selected;
        #[ce_reactions]
        attribute value: DomString, get = select::option_value, set = reflect::set_string::<{ local::VALUE }>;
        #[ce_reactions]
        attribute text: DomString, get = select::text, set = select::set_text;
        attribute index: i32, get = select::index;
    }

    interface HTMLOptGroupElement : HTMLElement [element html optgroup] {
        #[ce_reactions]
        attribute disabled: bool, get = reflect::boolean::<{ local::DISABLED }>, set = reflect::set_boolean::<{ local::DISABLED }>;
        #[ce_reactions]
        attribute label: DomString, get = reflect::string::<{ local::LABEL }>, set = reflect::set_string::<{ local::LABEL }>;
    }

    interface HTMLTextAreaElement : HTMLElement [element html textarea] {
        #[ce_reactions]
        attribute autocomplete: DomString, get = reflect::string::<{ local::AUTOCOMPLETE }>, set = reflect::set_string::<{ local::AUTOCOMPLETE }>;
        #[ce_reactions]
        attribute cols: u32, get = reflect::positive::<{ local::COLS }, 20>,
            set = reflect::set_positive::<{ local::COLS }, 20, true>;
        #[ce_reactions]
        attribute dirName: DomString, get = reflect::string::<{ local::DIRNAME }>, set = reflect::set_string::<{ local::DIRNAME }>;
        #[ce_reactions]
        attribute disabled: bool, get = reflect::boolean::<{ local::DISABLED }>, set = reflect::set_boolean::<{ local::DISABLED }>;
        attribute form: Option<HTMLFormElement>, get = forms::form;
        #[ce_reactions]
        attribute maxLength: i32, get = reflect::non_negative::<{ local::MAXLENGTH }, { -1 }>,
            set = reflect::set_non_negative::<{ local::MAXLENGTH }>;
        #[ce_reactions]
        attribute minLength: i32, get = reflect::non_negative::<{ local::MINLENGTH }, { -1 }>,
            set = reflect::set_non_negative::<{ local::MINLENGTH }>;
        #[ce_reactions]
        attribute name: DomString, get = reflect::string::<{ local::NAME }>, set = reflect::set_string::<{ local::NAME }>;
        #[ce_reactions]
        attribute placeholder: DomString, get = reflect::string::<{ local::PLACEHOLDER }>, set = reflect::set_string::<{ local::PLACEHOLDER }>;
        #[ce_reactions]
        attribute readOnly: bool, get = reflect::boolean::<{ local::READONLY }>, set = reflect::set_boolean::<{ local::READONLY }>;
        #[ce_reactions]
        attribute required: bool, get = reflect::boolean::<{ local::REQUIRED }>, set = reflect::set_boolean::<{ local::REQUIRED }>;
        #[ce_reactions]
        attribute rows: u32, get = reflect::positive::<{ local::ROWS }, 2>,
            set = reflect::set_positive::<{ local::ROWS }, 2, true>;
        #[ce_reactions]
        attribute wrap: DomString, get = reflect::string::<{ local::WRAP }>, set = reflect::set_string::<{ local::WRAP }>;
        attribute r#type: DomString, get = text_control::textarea_type;
        #[ce_reactions]
        attribute defaultValue: DomString, get = text_control::textarea_default_value, set = text_control::set_textarea_default_value;
        attribute labels: NodeList, get = forms::labels;
        attribute value: LegacyNullToEmptyString, get = text_control::textarea_value, set = text_control::set_textarea_value;
        attribute textLength: u32, get = text_control::text_length;
        attribute selectionStart: NullableU32, get = text_control::selection_start, set = text_control::set_selection_start;
        attribute selectionEnd: NullableU32, get = text_control::selection_end, set = text_control::set_selection_end;
        attribute selectionDirection: Option<DomString>,
            get = text_control::selection_direction, set = text_control::set_selection_direction;
        #[calls_script]
        method select() = text_control::select;
        method setSelectionRange(start: u32, end: u32, direction?: Option<DomString>) = text_control::set_selection_range_method;
        method setRangeText(replacement: DomString, start?: Option<u32>, end?: Option<u32>, selectionMode?: Option<DomString>) =
            text_control::set_range_text;
    }

    interface HTMLLabelElement : HTMLElement [element html label] {
        attribute form: Option<HTMLFormElement>, get = forms::label_form;
        #[ce_reactions]
        attribute htmlFor: DomString, get = reflect::string::<{ local::FOR }>, set = reflect::set_string::<{ local::FOR }>;
        attribute control: Option<HTMLElement>, get = forms::control;
    }

    interface HTMLFieldSetElement : HTMLElement [element html fieldset] {
        #[ce_reactions]
        attribute disabled: bool, get = reflect::boolean::<{ local::DISABLED }>, set = reflect::set_boolean::<{ local::DISABLED }>;
        attribute form: Option<HTMLFormElement>, get = forms::form;
        #[ce_reactions]
        attribute name: DomString, get = reflect::string::<{ local::NAME }>, set = reflect::set_string::<{ local::NAME }>;
        attribute r#type: DomString, get = forms::fieldset_type;
        attribute elements: HTMLCollection, get = forms::fieldset_elements;
    }

    interface HTMLLegendElement : HTMLElement [element html legend] {
        attribute form: Option<HTMLFormElement>, get = forms::legend_form;
    }

    interface HTMLOutputElement : HTMLElement [element html output] {
        // `[PutForwards=value]`.
        #[ce_reactions]
        attribute htmlFor: DOMTokenList, get = token_list::html_for, set: DomString = token_list::set_html_for;
        attribute form: Option<HTMLFormElement>, get = forms::form;
        #[ce_reactions]
        attribute name: DomString, get = reflect::string::<{ local::NAME }>, set = reflect::set_string::<{ local::NAME }>;
        attribute r#type: DomString, get = select::output_type;
        #[ce_reactions]
        attribute defaultValue: DomString, get = select::output_default_value, set = select::set_output_default_value;
        #[ce_reactions]
        attribute value: DomString, get = select::output_value, set = select::set_output_value;
        attribute labels: NodeList, get = forms::labels;
    }

    interface HTMLDataListElement : HTMLElement [element html datalist] {
        attribute options: HTMLCollection, get = select::datalist_options;
    }

    interface HTMLProgressElement : HTMLElement [element html progress] {
        #[ce_reactions]
        attribute value: f64, get = select::progress_value, set = select::set_progress_value;
        #[ce_reactions]
        attribute max: f64, get = select::progress_max, set = select::set_progress_max;
        attribute position: f64, get = select::progress_position;
        attribute labels: NodeList, get = forms::labels;
    }

    interface HTMLMeterElement : HTMLElement [element html meter] {
        #[ce_reactions]
        attribute value: f64, get = select::meter_value, set = select::set_meter_value;
        #[ce_reactions]
        attribute min: f64, get = select::meter_min, set = select::set_meter_min;
        #[ce_reactions]
        attribute max: f64, get = select::meter_max, set = select::set_meter_max;
        #[ce_reactions]
        attribute low: f64, get = select::meter_low, set = select::set_meter_low;
        #[ce_reactions]
        attribute high: f64, get = select::meter_high, set = select::set_meter_high;
        #[ce_reactions]
        attribute optimum: f64, get = select::meter_optimum, set = select::set_meter_optimum;
        attribute labels: NodeList, get = forms::labels;
    }
    // A document's `implementation`.
    interface CustomElementRegistry [type = dom::CustomElementRegistry] {
        #[ce_reactions]
        #[calls_script]
        method define(name: DomString, constructor: CustomElementConstructor, options?: ElementDefinitionOptions) =
            custom_elements::define;
        method get(name: DomString) -> AnyOrUndefined = custom_elements::get;
        #[front]
        method getName(constructor: CustomElementConstructor) -> Option<DomString> = custom_elements::get_name;
        method whenDefined(name: DomString) -> PromiseOfAny = custom_elements::when_defined;
        #[ce_reactions]
        #[calls_script]
        method upgrade(root: Node) = custom_elements::upgrade_tree;
    }

    dictionary ElementDefinitionOptions {
        extends: Option<String> = None,
    }

    // A constructed sheet, or a `<style>`'s or `<link>`'s. There are no
    // `@import` rules, so a parent sheet is null.
    interface StyleSheet [type = dom::CssStyleSheet] {
        attribute type: DomString, get = css_style_sheet::type_;
        attribute href: Option<DomString>, get = css_style_sheet::href;
        attribute ownerNode: Option<Element>, get = css_style_sheet::owner_node;
        attribute parentStyleSheet: Option<CSSStyleSheet>, get = css_style_sheet::parent_style_sheet;
        attribute title: Option<DomString>, get = css_style_sheet::title;
        attribute media: MediaList, get = css_style_sheet::media, set: DomString = css_style_sheet::set_media;
        attribute disabled: bool, get = css_style_sheet::disabled, set = css_style_sheet::set_disabled;
    }

    // With the legacy `rules`, `addRule` and `removeRule`.
    interface CSSStyleSheet : StyleSheet [type = dom::CssStyleSheet] {
        #[front]
        constructor(options?: CSSStyleSheetInit) = css_style_sheet::construct;
        attribute ownerRule: Option<CSSRule>, get = css_style_sheet::owner_rule;
        attribute cssRules: CSSRuleList, get = css_style_sheet::css_rules;
        method insertRule(rule: DomString, index?: u32) -> u32 = css_style_sheet::insert_rule;
        method deleteRule(index: u32) = css_style_sheet::delete_rule;
        method replace(text: DomString) -> Promise<CSSStyleSheet> = css_style_sheet::replace;
        method replaceSync(text: DomString) = css_style_sheet::replace_sync;
        // The same list as `cssRules`.
        attribute rules: CSSRuleList, get = css_style_sheet::css_rules;
        // `selector` and `style` "undefined" if not given, as WebIDL's
        // defaults are.
        method addRule(selector?: Option<DomString>, style?: Option<DomString>, index?: Option<u32>) -> i32 =
            css_style_sheet::add_rule;
        method removeRule(index?: Option<u32>) = css_style_sheet::remove_rule;
    }

    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary CSSStyleSheetInit {
        baseURL: Option<String> = None,
        media: String = String::new(),
        disabled: bool = false,
    }

    // A document's or a shadow root's sheets, live.
    interface StyleSheetList [type = dom::StyleSheetList] {
        attribute length: u32, get = css_style_sheet::style_sheet_list_length;
        #[indexed]
        method item(index: u32) -> Option<CSSStyleSheet> = css_style_sheet::style_sheet_list_item;
    }

    // A sheet's or a media rule's media, live.
    interface MediaList [type = dom::MediaList] {
        attribute mediaText: LegacyNullToEmptyString, get = cssom::media_text, set = cssom::set_media_text;
        method toString() -> DomString = cssom::media_text;
        attribute length: u32, get = cssom::media_length;
        #[indexed]
        method item(index: u32) -> Option<DomString> = cssom::media_item;
        method appendMedium(medium: DomString) = cssom::append_medium;
        method deleteMedium(medium: DomString) = cssom::delete_medium;
    }

    // A sheet's or a group rule's `cssRules`, live.
    interface CSSRuleList [type = dom::CssRuleList] {
        attribute length: u32, get = cssom::rule_list_length;
        #[indexed]
        method item(index: u32) -> Option<CSSRule> = cssom::rule_list_item;
    }

    // Style, media, supports, keyframes and keyframe rules exist.
    interface CSSRule [type = dom::CssRule] {
        attribute cssText: DomString, get = cssom::css_text, set = cssom::set_css_text;
        attribute parentRule: Option<CSSRule>, get = cssom::parent_rule;
        attribute parentStyleSheet: Option<CSSStyleSheet>, get = cssom::parent_style_sheet;
        attribute type: u32, get = cssom::rule_type;
        const STYLE_RULE: u32 = 1;
        const CHARSET_RULE: u32 = 2;
        const IMPORT_RULE: u32 = 3;
        const MEDIA_RULE: u32 = 4;
        const FONT_FACE_RULE: u32 = 5;
        const PAGE_RULE: u32 = 6;
        const MARGIN_RULE: u32 = 9;
        const NAMESPACE_RULE: u32 = 10;
        // CSS Animations'.
        const KEYFRAMES_RULE: u32 = 7;
        const KEYFRAME_RULE: u32 = 8;
        // CSS Conditional's.
        const SUPPORTS_RULE: u32 = 12;
    }

    // Its parent is `CSSRule`, not CSS Nesting's `CSSGroupingRule`: machopo
    // has no nesting, so a style rule has no rules of its own.
    interface CSSStyleRule : CSSRule [type = dom::CssRule] {
        attribute selectorText: DomString, get = cssom::selector_text, set = cssom::set_selector_text;
        attribute style: CSSStyleProperties, get = cssom::style, set: DomString = cssom::set_style;
    }

    interface CSSGroupingRule : CSSRule [type = dom::CssRule] {
        attribute cssRules: CSSRuleList, get = cssom::css_rules;
        method insertRule(rule: DomString, index?: u32) -> u32 = cssom::insert_rule;
        method deleteRule(index: u32) = cssom::delete_rule;
    }

    interface CSSConditionRule : CSSGroupingRule [type = dom::CssRule] {
        attribute conditionText: DomString, get = cssom::condition_text;
    }

    interface CSSMediaRule : CSSConditionRule [type = dom::CssRule] {
        attribute media: MediaList, get = cssom::rule_media, set: DomString = cssom::set_rule_media;
    }

    interface CSSSupportsRule : CSSConditionRule [type = dom::CssRule] {}

    interface CSSKeyframesRule : CSSRule [type = dom::CssRule] {
        attribute name: DomString, get = cssom::keyframes_name, set = cssom::set_keyframes_name;
        attribute cssRules: CSSRuleList, get = cssom::css_rules;
        attribute length: u32, get = cssom::keyframes_length;
        #[indexed(getter)]
        method indexedGetter(index: u32) -> Option<CSSKeyframeRule> = cssom::keyframes_item;
        method appendRule(rule: DomString) = cssom::append_rule;
        method deleteRule(select: DomString) = cssom::keyframes_delete_rule;
        method findRule(select: DomString) -> Option<CSSKeyframeRule> = cssom::find_rule;
    }

    interface CSSKeyframeRule : CSSRule [type = dom::CssRule] {
        attribute keyText: DomString, get = cssom::key_text, set = cssom::set_key_text;
        attribute style: CSSStyleProperties, get = cssom::style, set: DomString = cssom::set_style;
    }

    interface DOMImplementation [type = dom::DomImplementation] {
        method createDocumentType(qualifiedName: DomString, publicId: DomString, systemId: DomString) -> DocumentType =
            dom_implementation::create_document_type;
        method createDocument(namespace: Option<DomString>, qualifiedName: LegacyNullToEmptyString, doctype?: Option<DocumentType>) -> Document =
            dom_implementation::create_document;
        // `title` optional: none given is no `<title>` (null is too, here).
        method createHTMLDocument(title?: OptionalString) -> Document = dom_implementation::create_html_document;
        method hasFeature() -> bool = dom_implementation::has_feature;
    }

    // A live collection: its elements found at each ask. Its named
    // properties aren't listed (`[LegacyUnenumerableNamedProperties]`
    // has them not enumerable, but not in the own keys either yet).
    interface HTMLCollection [type = dom::HtmlCollection] {
        // WebIDL `unsigned long`.
        attribute length: u32, get = collection::length;
        #[indexed]
        method item(index: u32) -> Option<Element> = collection::item;
        method namedItem(name: DomString) -> Option<Element> = collection::named_item;
        #[named(getter)]
        method namedGetter(name: DomString) -> Option<Element> = collection::named_item;
    }

    // A form's `elements`: its listed elements but image buttons, in tree
    // order, as it keeps them. Its named properties aren't listed.
    // `document.all`: every element of the document. Not an
    // `HTMLCollection`: an undetectable object in JS (`typeof` is
    // `"undefined"`), and callable as its `item`. A name several elements
    // have is an `HTMLCollection` of them.
    #[is_html_dda]
    interface HTMLAllCollection [type = AllCollectionObject] {
        // WebIDL `unsigned long`.
        attribute length: u32, get = collection::length;
        #[indexed(getter)]
        method indexedGetter(index: u32) -> Option<Element> = collection::item;
        // `item` first, as Chrome has them.
        method item(nameOrIndex?: OptionalString) -> Option<HTMLCollectionOrElement> = collection::all_item;
        method namedItem(name: DomString) -> Option<HTMLCollectionOrElement> = collection::all_named_item;
        #[named(getter)]
        method namedGetter(name: DomString) -> Option<HTMLCollectionOrElement> = collection::all_named_item;
    }

    interface HTMLFormControlsCollection : HTMLCollection [type = FormControlsCollectionObject] {
        method namedItem(name: DomString) -> Option<RadioNodeListOrElement> = forms::controls_named_item;
        #[named(getter)]
        method namedGetter(name: DomString) -> Option<RadioNodeListOrElement> = forms::controls_named_item;
    }

    // A select's `options`. Its named properties aren't listed.
    interface HTMLOptionsCollection : HTMLCollection [type = OptionsCollectionObject] {
        // WebIDL `unsigned long`.
        #[ce_reactions]
        attribute length: u32, get = collection::length, set = select::options_set_length;
        #[named(getter)]
        method namedGetter(name: DomString) -> Option<Element> = collection::named_item;
        #[ce_reactions]
        #[indexed(setter)]
        method indexedSetter(index: u32, option: Option<HTMLOptionElement>) = select::options_set_item;
        #[ce_reactions]
        method add(element: OptionOrOptGroup, before?: NullableElementOrLong) = select::options_add;
        #[ce_reactions]
        method remove(index: i32) = select::options_remove;
        attribute selectedIndex: i32, get = select::options_selected_index, set = select::options_set_selected_index;
    }

    // A static list (`querySelectorAll`'s).
    #[iterable]
    interface NodeList [type = dom::NodeList] {
        // WebIDL `unsigned long`.
        attribute length: u32, get = query::node_list_length;
        #[indexed]
        method item(index: u32) -> Option<Node> = query::node_list_item;
    }

    // A form's `elements` of a name, live (`elements.namedItem`, `form[name]`
    // with several). Its `value` comes with the checkedness of radio
    // buttons.
    interface RadioNodeList : NodeList [type = RadioNodeListObject] {
        attribute value: DomString, get = forms::radio_value, set = forms::set_radio_value;
    }

    // An element's `class` as a set of tokens (`classList`).
    #[iterable]
    interface DOMTokenList [type = dom::DomTokenList] {
        attribute length: u32, get = token_list::length;
        #[indexed]
        method item(index: u32) -> Option<DomString> = token_list::item;
        method contains(token: DomString) -> bool = token_list::contains;
        #[ce_reactions]
        method add(tokens?: Strings) = token_list::add;
        #[ce_reactions]
        method remove(tokens?: Strings) = token_list::remove;
        #[ce_reactions]
        method toggle(token: DomString, force?: Option<bool>) -> bool = token_list::toggle;
        #[ce_reactions]
        method replace(token: DomString, newToken: DomString) -> bool = token_list::replace;
        method supports(token: DomString) -> bool = token_list::supports;
        #[ce_reactions]
        attribute value: DomString, get = token_list::value, set = token_list::set_value;
        method toString() -> DomString = token_list::value;
    }

    // An element's `data-*` attributes, as named properties.
    #[override_builtins]
    interface DOMStringMap [type = dom::DomStringMap] {
        #[named(getter)]
        method getter(name: DomString) -> Option<DomString> = dataset::get;
        #[ce_reactions]
        #[named(setter)]
        method setter(name: DomString, value: DomString) = dataset::set;
        #[ce_reactions]
        #[named(deleter)]
        method deleter(name: DomString) = dataset::delete;
        #[named(names)]
        method names() -> Strings = dataset::names;
    }

    // An element's `style` attribute's declarations; the camel-cased and
    // dashed property names are named properties (Blink's).
    interface CSSStyleDeclaration [type = dom::CssStyleDeclaration] {
        #[ce_reactions]
        attribute cssText: DomString, get = css_style::css_text, set = css_style::set_css_text;
        attribute length: u32, get = css_style::length;
        attribute parentRule: Option<CSSRule>, get = css_style::parent_rule;
        #[indexed]
        method item(index: u32) -> DomString = css_style::item;
        method getPropertyValue(property: DomString) -> DomString = css_style::get_property_value;
        method getPropertyPriority(property: DomString) -> DomString = css_style::get_property_priority;
        #[ce_reactions]
        method setProperty(property: DomString, value: LegacyNullToEmptyString, priority?: Option<DomString>) =
            css_style::set_property;
        #[ce_reactions]
        method removeProperty(property: DomString) -> DomString = css_style::remove_property;
    }

    // What `element.style`, `getComputedStyle()` and a style rule's `style`
    // are (CSSOM's): `cssFloat`, and each property's
    // camel-cased and dashed attributes as named properties, as in Blink
    // (not on the prototype).
    interface CSSStyleProperties : CSSStyleDeclaration [type = dom::CssStyleDeclaration] {
        #[ce_reactions]
        attribute cssFloat: LegacyNullToEmptyString, get = css_style::css_float, set = css_style::set_css_float;
        #[named(getter)]
        method getter(name: DomString) -> Option<DomString> = css_style::named_property;
        #[ce_reactions]
        #[named(setter)]
        method setter(name: DomString, value: LegacyNullToEmptyString) -> bool = css_style::set_named_property;
        #[named(names)]
        method names() -> Strings = css_style::supported_property_names;
    }

    interface MutationObserver [type = dom::MutationObserver] {
        #[front]
        constructor(callback: MutationCallback) = mutation_observer::construct;
        method observe(target: Node, options?: MutationObserverInit) = mutation_observer::observe;
        method disconnect() = mutation_observer::disconnect;
        method takeRecords() -> Sequence<MutationRecord> = mutation_observer::take_records;
    }

    // WebIDL's names, which the table reads its members by; no default
    // where whether it is there matters.
    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary MutationObserverInit {
        attributeFilter: Option<Vec<String>> = None,
        attributeOldValue: Option<bool> = None,
        attributes: Option<bool> = None,
        characterData: Option<bool> = None,
        characterDataOldValue: Option<bool> = None,
        childList: bool = false,
        subtree: bool = false,
    }

    interface MutationRecord [type = dom::MutationRecord] {
        attribute r#type: DomString, get = mutation_observer::record_type;
        attribute target: Node, get = mutation_observer::target;
        attribute addedNodes: NodeList, get = mutation_observer::added_nodes;
        attribute removedNodes: NodeList, get = mutation_observer::removed_nodes;
        attribute previousSibling: Option<Node>, get = mutation_observer::previous_sibling;
        attribute nextSibling: Option<Node>, get = mutation_observer::next_sibling;
        attribute attributeName: Option<DomString>, get = mutation_observer::attribute_name;
        attribute attributeNamespace: Option<DomString>, get = mutation_observer::attribute_namespace;
        attribute oldValue: Option<DomString>, get = mutation_observer::old_value;
    }

    // Intersection Observer's, v2 included (`trackVisibility`, `delay`,
    // `isVisible`: Chromium's). No `IntersectionObserverEntry` constructor
    // yet.
    interface IntersectionObserver [type = dom::IntersectionObserver] {
        #[front]
        constructor(callback: IntersectionObserverCallback, options?: IntersectionObserverInitArg) = intersection_observer::construct;
        attribute root: Option<Node>, get = intersection_observer::root;
        attribute rootMargin: DomString, get = intersection_observer::root_margin;
        attribute scrollMargin: DomString, get = intersection_observer::scroll_margin;
        attribute thresholds: Numbers, get = intersection_observer::thresholds;
        attribute delay: f64, get = intersection_observer::delay;
        attribute trackVisibility: bool, get = intersection_observer::track_visibility;
        method disconnect() = intersection_observer::disconnect;
        method observe(target: Element) = intersection_observer::observe;
        method takeRecords() -> Sequence<IntersectionObserverEntry> = intersection_observer::take_records;
        method unobserve(target: Element) = intersection_observer::unobserve;
    }

    interface IntersectionObserverEntry [type = dom::IntersectionObserverEntry] {
        constructor(init: IntersectionObserverEntryInitArg) = intersection_observer::construct_entry;
        attribute time: f64, get = intersection_observer::time;
        attribute rootBounds: Option<DOMRectReadOnly>, get = intersection_observer::root_bounds;
        attribute boundingClientRect: DOMRectReadOnly, get = intersection_observer::bounding_client_rect;
        attribute intersectionRect: DOMRectReadOnly, get = intersection_observer::intersection_rect;
        attribute isIntersecting: bool, get = intersection_observer::is_intersecting;
        attribute isVisible: bool, get = intersection_observer::is_visible;
        attribute intersectionRatio: f64, get = intersection_observer::intersection_ratio;
        attribute target: Element, get = intersection_observer::target;
    }

    // Resize Observer's (Blink's `core/resize_observer`).
    interface ResizeObserver [type = dom::ResizeObserver] {
        #[front]
        constructor(callback: ResizeObserverCallback) = resize_observer::construct;
        method observe(target: Element, options?: ResizeObserverOptions) = resize_observer::observe;
        method unobserve(target: Element) = resize_observer::unobserve;
        method disconnect() = resize_observer::disconnect;
    }

    // `box`: `ResizeObserverBoxOptions`' value, checked by `observe`.
    dictionary ResizeObserverOptions {
        r#box: String = String::from("content-box"),
    }

    // Its lists are the same objects every time (the entry keeps them).
    interface ResizeObserverEntry [type = dom::ResizeObserverEntry] {
        attribute target: Element, get = resize_observer::target;
        attribute contentRect: DOMRectReadOnly, get = resize_observer::content_rect;
        attribute borderBoxSize: FrozenArray<ResizeObserverSize>, get = resize_observer::border_box_size;
        attribute contentBoxSize: FrozenArray<ResizeObserverSize>, get = resize_observer::content_box_size;
        attribute devicePixelContentBoxSize: FrozenArray<ResizeObserverSize>,
            get = resize_observer::device_pixel_content_box_size;
    }

    interface ResizeObserverSize [type = dom::ResizeObserverSize] {
        attribute inlineSize: UnrestrictedDouble, get = resize_observer::inline_size;
        attribute blockSize: UnrestrictedDouble, get = resize_observer::block_size;
    }

    // HTML's 2D context: its state, transforms, compositing, colors and
    // gradients (no patterns yet), shadows, line styles, rects, paths and
    // pixels (no `Path2D`, text or images yet). Its numbers are
    // `unrestricted double`s: a member ignores what isn't finite.
    interface CanvasRenderingContext2D [type = dom::CanvasRenderingContext2d] {
        attribute canvas: HTMLCanvasElement, get = canvas::canvas;
        method getContextAttributes() -> CanvasRenderingContext2DSettings = canvas::get_context_attributes;
        method save() = canvas::save;
        method restore() = canvas::restore;
        method reset() = canvas::reset;
        method isContextLost() -> bool = canvas::is_context_lost;
        method scale(x: UnrestrictedDouble, y: UnrestrictedDouble) = canvas::scale;
        method rotate(angle: UnrestrictedDouble) = canvas::rotate;
        method translate(x: UnrestrictedDouble, y: UnrestrictedDouble) = canvas::translate;
        method transform(
            a: UnrestrictedDouble,
            b: UnrestrictedDouble,
            c: UnrestrictedDouble,
            d: UnrestrictedDouble,
            e: UnrestrictedDouble,
            f: UnrestrictedDouble,
        ) = canvas::transform;
        method setTransform(
            a: UnrestrictedDouble,
            b: UnrestrictedDouble,
            c: UnrestrictedDouble,
            d: UnrestrictedDouble,
            e: UnrestrictedDouble,
            f: UnrestrictedDouble,
        ) = canvas::set_transform;
        method setTransform(transform?: DOMMatrix2DInit) = canvas::set_transform_matrix;
        method resetTransform() = canvas::reset_transform;
        attribute globalAlpha: UnrestrictedDouble, get = canvas::global_alpha, set = canvas::set_global_alpha;
        attribute globalCompositeOperation: DomString,
            get = canvas::global_composite_operation, set = canvas::set_global_composite_operation;
        attribute strokeStyle: CanvasStyleValue, get = canvas::stroke_style, set = canvas::set_stroke_style;
        attribute fillStyle: CanvasStyleValue, get = canvas::fill_style, set = canvas::set_fill_style;
        method createLinearGradient(
            x0: f64,
            y0: f64,
            x1: f64,
            y1: f64,
        ) -> CanvasGradient = canvas::create_linear_gradient;
        method createRadialGradient(
            x0: f64,
            y0: f64,
            r0: f64,
            x1: f64,
            y1: f64,
            r1: f64,
        ) -> CanvasGradient = canvas::create_radial_gradient;
        method createConicGradient(startAngle: f64, x: f64, y: f64) -> CanvasGradient =
            canvas::create_conic_gradient;
        method createPattern(image: CanvasImageSource, repetition: LegacyNullToEmptyString) -> Option<CanvasPattern> =
            canvas::create_pattern;
        attribute imageSmoothingEnabled: bool,
            get = canvas::image_smoothing_enabled, set = canvas::set_image_smoothing_enabled;
        attribute imageSmoothingQuality: DomString,
            get = canvas::image_smoothing_quality, set = canvas::set_image_smoothing_quality;
        attribute shadowOffsetX: UnrestrictedDouble, get = canvas::shadow_offset_x, set = canvas::set_shadow_offset_x;
        attribute shadowOffsetY: UnrestrictedDouble, get = canvas::shadow_offset_y, set = canvas::set_shadow_offset_y;
        attribute shadowBlur: UnrestrictedDouble, get = canvas::shadow_blur, set = canvas::set_shadow_blur;
        attribute shadowColor: DomString, get = canvas::shadow_color, set = canvas::set_shadow_color;
        method drawImage(image: CanvasImageSource, dx: UnrestrictedDouble, dy: UnrestrictedDouble) = canvas::draw_image_at;
        method drawImage(
            image: CanvasImageSource,
            dx: UnrestrictedDouble,
            dy: UnrestrictedDouble,
            dw: UnrestrictedDouble,
            dh: UnrestrictedDouble,
        ) = canvas::draw_image_scaled;
        method drawImage(
            image: CanvasImageSource,
            sx: UnrestrictedDouble,
            sy: UnrestrictedDouble,
            sw: UnrestrictedDouble,
            sh: UnrestrictedDouble,
            dx: UnrestrictedDouble,
            dy: UnrestrictedDouble,
            dw: UnrestrictedDouble,
            dh: UnrestrictedDouble,
        ) = canvas::draw_image_part;
        method clearRect(x: UnrestrictedDouble, y: UnrestrictedDouble, w: UnrestrictedDouble, h: UnrestrictedDouble) = canvas::clear_rect;
        method fillRect(x: UnrestrictedDouble, y: UnrestrictedDouble, w: UnrestrictedDouble, h: UnrestrictedDouble) = canvas::fill_rect;
        method strokeRect(x: UnrestrictedDouble, y: UnrestrictedDouble, w: UnrestrictedDouble, h: UnrestrictedDouble) = canvas::stroke_rect;
        method createImageData(sw: EnforcedI32, sh: EnforcedI32, settings?: ImageDataSettings) -> NewImageData =
            canvas::create_image_data;
        method createImageData(imagedata: ImageData) -> NewImageData = canvas::create_image_data_of;
        method getImageData(
            sx: EnforcedI32,
            sy: EnforcedI32,
            sw: EnforcedI32,
            sh: EnforcedI32,
            settings?: ImageDataSettings,
        ) -> NewImageData = canvas::get_image_data;
        #[front]
        method putImageData(imagedata: ImageData, dx: EnforcedI32, dy: EnforcedI32) = canvas::put_image_data;
        #[front]
        method putImageData(
            imagedata: ImageData,
            dx: EnforcedI32,
            dy: EnforcedI32,
            dirtyX: EnforcedI32,
            dirtyY: EnforcedI32,
            dirtyWidth: EnforcedI32,
            dirtyHeight: EnforcedI32,
        ) = canvas::put_image_data_part;
        method beginPath() = canvas::begin_path;
        method fill(fillRule?: OptionalString) = canvas::fill;
        method fill(path: Path2D, fillRule?: OptionalString) = canvas::fill_path;
        method stroke() = canvas::stroke;
        method stroke(path: Path2D) = canvas::stroke_path;
        method clip(fillRule?: OptionalString) = canvas::clip;
        method clip(path: Path2D, fillRule?: OptionalString) = canvas::clip_path;
        method isPointInPath(x: UnrestrictedDouble, y: UnrestrictedDouble, fillRule?: OptionalString) -> bool =
            canvas::is_point_in_path;
        method isPointInPath(
            path: Path2D,
            x: UnrestrictedDouble,
            y: UnrestrictedDouble,
            fillRule?: OptionalString,
        ) -> bool = canvas::is_point_in_path_of;
        method isPointInStroke(x: UnrestrictedDouble, y: UnrestrictedDouble) -> bool =
            canvas::is_point_in_stroke;
        method isPointInStroke(path: Path2D, x: UnrestrictedDouble, y: UnrestrictedDouble) -> bool =
            canvas::is_point_in_stroke_of;
        method measureText(text: DomString) -> TextMetrics = canvas::measure_text;
        method fillText(text: DomString, x: UnrestrictedDouble, y: UnrestrictedDouble, maxWidth?: OptionalUnrestrictedDouble) =
            canvas::fill_text;
        method strokeText(text: DomString, x: UnrestrictedDouble, y: UnrestrictedDouble, maxWidth?: OptionalUnrestrictedDouble) =
            canvas::stroke_text;
        // CanvasTextDrawingStyles: the keywords' attributes `DOMString`s,
        // an invalid value ignored (WebIDL's enumerations).
        attribute font: DomString, get = canvas::font, set = canvas::set_font;
        attribute textAlign: DomString, get = canvas::text_align, set = canvas::set_text_align;
        attribute textBaseline: DomString, get = canvas::text_baseline, set = canvas::set_text_baseline;
        attribute direction: DomString, get = canvas::direction, set = canvas::set_direction;
        attribute letterSpacing: DomString, get = canvas::letter_spacing, set = canvas::set_letter_spacing;
        attribute fontKerning: DomString, get = canvas::font_kerning, set = canvas::set_font_kerning;
        attribute fontStretch: DomString, get = canvas::font_stretch, set = canvas::set_font_stretch;
        attribute fontVariantCaps: DomString, get = canvas::font_variant_caps, set = canvas::set_font_variant_caps;
        attribute textRendering: DomString, get = canvas::text_rendering, set = canvas::set_text_rendering;
        attribute wordSpacing: DomString, get = canvas::word_spacing, set = canvas::set_word_spacing;
        attribute lineWidth: UnrestrictedDouble, get = canvas::line_width, set = canvas::set_line_width;
        attribute lineCap: DomString, get = canvas::line_cap, set = canvas::set_line_cap;
        attribute lineJoin: DomString, get = canvas::line_join, set = canvas::set_line_join;
        attribute miterLimit: UnrestrictedDouble, get = canvas::miter_limit, set = canvas::set_miter_limit;
        method setLineDash(segments: Doubles) = canvas::set_line_dash;
        method getLineDash() -> Doubles = canvas::get_line_dash;
        attribute lineDashOffset: UnrestrictedDouble,
            get = canvas::line_dash_offset, set = canvas::set_line_dash_offset;
        method closePath() = canvas::close_path;
        method moveTo(x: UnrestrictedDouble, y: UnrestrictedDouble) = canvas::move_to;
        method lineTo(x: UnrestrictedDouble, y: UnrestrictedDouble) = canvas::line_to;
        method quadraticCurveTo(
            cpx: UnrestrictedDouble,
            cpy: UnrestrictedDouble,
            x: UnrestrictedDouble,
            y: UnrestrictedDouble,
        ) = canvas::quadratic_curve_to;
        method bezierCurveTo(
            cp1x: UnrestrictedDouble,
            cp1y: UnrestrictedDouble,
            cp2x: UnrestrictedDouble,
            cp2y: UnrestrictedDouble,
            x: UnrestrictedDouble,
            y: UnrestrictedDouble,
        ) = canvas::bezier_curve_to;
        method arcTo(
            x1: UnrestrictedDouble,
            y1: UnrestrictedDouble,
            x2: UnrestrictedDouble,
            y2: UnrestrictedDouble,
            radius: UnrestrictedDouble,
        ) = canvas::arc_to;
        method rect(x: UnrestrictedDouble, y: UnrestrictedDouble, w: UnrestrictedDouble, h: UnrestrictedDouble) = canvas::rect;
        method roundRect(
            x: UnrestrictedDouble,
            y: UnrestrictedDouble,
            w: UnrestrictedDouble,
            h: UnrestrictedDouble,
            radii?: RoundRectRadii,
        ) = canvas::round_rect;
        method arc(
            x: UnrestrictedDouble,
            y: UnrestrictedDouble,
            radius: UnrestrictedDouble,
            startAngle: UnrestrictedDouble,
            endAngle: UnrestrictedDouble,
            counterclockwise?: bool,
        ) = canvas::arc;
        method ellipse(
            x: UnrestrictedDouble,
            y: UnrestrictedDouble,
            radiusX: UnrestrictedDouble,
            radiusY: UnrestrictedDouble,
            rotation: UnrestrictedDouble,
            startAngle: UnrestrictedDouble,
            endAngle: UnrestrictedDouble,
            counterclockwise?: bool,
        ) = canvas::ellipse;
    }

    // `colorStop`'s offset a (finite) `double`.
    interface CanvasGradient [type = dom::CanvasGradient] {
        method addColorStop(offset: f64, color: DomString) = canvas::add_color_stop;
    }

    // SVG's: what an SVG element of a name SVG gives no interface is; the
    // members that need no layout, and `HTMLOrSVGElement`'s (`HTMLElement`'s
    // functions).
    interface SVGElement : Element [element svg] {
        attribute className: SVGAnimatedString, get = svg::class_name;
        attribute ownerSVGElement: Option<SVGSVGElement>, get = svg::owner_svg_element;
        attribute viewportElement: Option<SVGElement>, get = svg::viewport_element;
        attribute style: CSSStyleProperties, get = css_style::style, set: DomString = css_style::set_style;
        #[ce_reactions]
        attribute tabIndex: i32, get = focus::tab_index, set = focus::set_tab_index;
        #[calls_script]
        method focus(options?: FocusOptions) = focus::focus;
        #[calls_script]
        method blur() = focus::blur;
        #[ce_reactions]
        attribute autofocus: bool, get = reflect::boolean::<{ local::AUTOFOCUS }>, set = reflect::set_boolean::<{ local::AUTOFOCUS }>;

        // HTML's `GlobalEventHandlers`, as `HTMLElement`'s.
        #[front]
        attribute onabort: EventHandler, get = handlers::element_handler::<{ event_type::ABORT }>, set = handlers::set_element_handler::<_, { event_type::ABORT }>;
        #[front]
        attribute onanimationcancel: EventHandler, get = handlers::element_handler::<{ event_type::ANIMATIONCANCEL }>, set = handlers::set_element_handler::<_, { event_type::ANIMATIONCANCEL }>;
        #[front]
        attribute onanimationend: EventHandler, get = handlers::element_handler::<{ event_type::ANIMATIONEND }>, set = handlers::set_element_handler::<_, { event_type::ANIMATIONEND }>;
        #[front]
        attribute onanimationiteration: EventHandler, get = handlers::element_handler::<{ event_type::ANIMATIONITERATION }>, set = handlers::set_element_handler::<_, { event_type::ANIMATIONITERATION }>;
        #[front]
        attribute onanimationstart: EventHandler, get = handlers::element_handler::<{ event_type::ANIMATIONSTART }>, set = handlers::set_element_handler::<_, { event_type::ANIMATIONSTART }>;
        #[front]
        attribute onauxclick: EventHandler, get = handlers::element_handler::<{ event_type::AUXCLICK }>, set = handlers::set_element_handler::<_, { event_type::AUXCLICK }>;
        #[front]
        attribute onbeforeinput: EventHandler, get = handlers::element_handler::<{ event_type::BEFOREINPUT }>, set = handlers::set_element_handler::<_, { event_type::BEFOREINPUT }>;
        #[front]
        attribute onbeforematch: EventHandler, get = handlers::element_handler::<{ event_type::BEFOREMATCH }>, set = handlers::set_element_handler::<_, { event_type::BEFOREMATCH }>;
        #[front]
        attribute onbeforetoggle: EventHandler, get = handlers::element_handler::<{ event_type::BEFORETOGGLE }>, set = handlers::set_element_handler::<_, { event_type::BEFORETOGGLE }>;
        #[front]
        attribute onblur: EventHandler, get = handlers::element_handler::<{ event_type::BLUR }>, set = handlers::set_element_handler::<_, { event_type::BLUR }>;
        #[front]
        attribute oncancel: EventHandler, get = handlers::element_handler::<{ event_type::CANCEL }>, set = handlers::set_element_handler::<_, { event_type::CANCEL }>;
        #[front]
        attribute oncanplay: EventHandler, get = handlers::element_handler::<{ event_type::CANPLAY }>, set = handlers::set_element_handler::<_, { event_type::CANPLAY }>;
        #[front]
        attribute oncanplaythrough: EventHandler, get = handlers::element_handler::<{ event_type::CANPLAYTHROUGH }>, set = handlers::set_element_handler::<_, { event_type::CANPLAYTHROUGH }>;
        #[front]
        attribute onchange: EventHandler, get = handlers::element_handler::<{ event_type::CHANGE }>, set = handlers::set_element_handler::<_, { event_type::CHANGE }>;
        #[front]
        attribute onclick: EventHandler, get = handlers::element_handler::<{ event_type::CLICK }>, set = handlers::set_element_handler::<_, { event_type::CLICK }>;
        #[front]
        attribute onclose: EventHandler, get = handlers::element_handler::<{ event_type::CLOSE }>, set = handlers::set_element_handler::<_, { event_type::CLOSE }>;
        #[front]
        attribute oncommand: EventHandler, get = handlers::element_handler::<{ event_type::COMMAND }>, set = handlers::set_element_handler::<_, { event_type::COMMAND }>;
        #[front]
        attribute oncontentvisibilityautostatechange: EventHandler, get = handlers::element_handler::<{ event_type::CONTENTVISIBILITYAUTOSTATECHANGE }>, set = handlers::set_element_handler::<_, { event_type::CONTENTVISIBILITYAUTOSTATECHANGE }>;
        #[front]
        attribute oncontextlost: EventHandler, get = handlers::element_handler::<{ event_type::CONTEXTLOST }>, set = handlers::set_element_handler::<_, { event_type::CONTEXTLOST }>;
        #[front]
        attribute oncontextmenu: EventHandler, get = handlers::element_handler::<{ event_type::CONTEXTMENU }>, set = handlers::set_element_handler::<_, { event_type::CONTEXTMENU }>;
        #[front]
        attribute oncontextrestored: EventHandler, get = handlers::element_handler::<{ event_type::CONTEXTRESTORED }>, set = handlers::set_element_handler::<_, { event_type::CONTEXTRESTORED }>;
        #[front]
        attribute oncuechange: EventHandler, get = handlers::element_handler::<{ event_type::CUECHANGE }>, set = handlers::set_element_handler::<_, { event_type::CUECHANGE }>;
        #[front]
        attribute ondblclick: EventHandler, get = handlers::element_handler::<{ event_type::DBLCLICK }>, set = handlers::set_element_handler::<_, { event_type::DBLCLICK }>;
        #[front]
        attribute ondrag: EventHandler, get = handlers::element_handler::<{ event_type::DRAG }>, set = handlers::set_element_handler::<_, { event_type::DRAG }>;
        #[front]
        attribute ondragend: EventHandler, get = handlers::element_handler::<{ event_type::DRAGEND }>, set = handlers::set_element_handler::<_, { event_type::DRAGEND }>;
        #[front]
        attribute ondragenter: EventHandler, get = handlers::element_handler::<{ event_type::DRAGENTER }>, set = handlers::set_element_handler::<_, { event_type::DRAGENTER }>;
        #[front]
        attribute ondragleave: EventHandler, get = handlers::element_handler::<{ event_type::DRAGLEAVE }>, set = handlers::set_element_handler::<_, { event_type::DRAGLEAVE }>;
        #[front]
        attribute ondragover: EventHandler, get = handlers::element_handler::<{ event_type::DRAGOVER }>, set = handlers::set_element_handler::<_, { event_type::DRAGOVER }>;
        #[front]
        attribute ondragstart: EventHandler, get = handlers::element_handler::<{ event_type::DRAGSTART }>, set = handlers::set_element_handler::<_, { event_type::DRAGSTART }>;
        #[front]
        attribute ondrop: EventHandler, get = handlers::element_handler::<{ event_type::DROP }>, set = handlers::set_element_handler::<_, { event_type::DROP }>;
        #[front]
        attribute ondurationchange: EventHandler, get = handlers::element_handler::<{ event_type::DURATIONCHANGE }>, set = handlers::set_element_handler::<_, { event_type::DURATIONCHANGE }>;
        #[front]
        attribute onemptied: EventHandler, get = handlers::element_handler::<{ event_type::EMPTIED }>, set = handlers::set_element_handler::<_, { event_type::EMPTIED }>;
        #[front]
        attribute onended: EventHandler, get = handlers::element_handler::<{ event_type::ENDED }>, set = handlers::set_element_handler::<_, { event_type::ENDED }>;
        #[front]
        attribute onerror: EventHandler, get = handlers::element_handler::<{ event_type::ERROR }>, set = handlers::set_element_handler::<_, { event_type::ERROR }>;
        #[front]
        attribute onfocus: EventHandler, get = handlers::element_handler::<{ event_type::FOCUS }>, set = handlers::set_element_handler::<_, { event_type::FOCUS }>;
        #[front]
        attribute onformdata: EventHandler, get = handlers::element_handler::<{ event_type::FORMDATA }>, set = handlers::set_element_handler::<_, { event_type::FORMDATA }>;
        #[front]
        attribute oninput: EventHandler, get = handlers::element_handler::<{ event_type::INPUT }>, set = handlers::set_element_handler::<_, { event_type::INPUT }>;
        #[front]
        attribute oninvalid: EventHandler, get = handlers::element_handler::<{ event_type::INVALID }>, set = handlers::set_element_handler::<_, { event_type::INVALID }>;
        #[front]
        attribute onkeydown: EventHandler, get = handlers::element_handler::<{ event_type::KEYDOWN }>, set = handlers::set_element_handler::<_, { event_type::KEYDOWN }>;
        #[front]
        attribute onkeypress: EventHandler, get = handlers::element_handler::<{ event_type::KEYPRESS }>, set = handlers::set_element_handler::<_, { event_type::KEYPRESS }>;
        #[front]
        attribute onkeyup: EventHandler, get = handlers::element_handler::<{ event_type::KEYUP }>, set = handlers::set_element_handler::<_, { event_type::KEYUP }>;
        #[front]
        attribute onload: EventHandler, get = handlers::element_handler::<{ event_type::LOAD }>, set = handlers::set_element_handler::<_, { event_type::LOAD }>;
        #[front]
        attribute onloadeddata: EventHandler, get = handlers::element_handler::<{ event_type::LOADEDDATA }>, set = handlers::set_element_handler::<_, { event_type::LOADEDDATA }>;
        #[front]
        attribute onloadedmetadata: EventHandler, get = handlers::element_handler::<{ event_type::LOADEDMETADATA }>, set = handlers::set_element_handler::<_, { event_type::LOADEDMETADATA }>;
        #[front]
        attribute onloadstart: EventHandler, get = handlers::element_handler::<{ event_type::LOADSTART }>, set = handlers::set_element_handler::<_, { event_type::LOADSTART }>;
        #[front]
        attribute onmousedown: EventHandler, get = handlers::element_handler::<{ event_type::MOUSEDOWN }>, set = handlers::set_element_handler::<_, { event_type::MOUSEDOWN }>;
        #[front]
        attribute onmouseenter: EventHandler, get = handlers::element_handler::<{ event_type::MOUSEENTER }>, set = handlers::set_element_handler::<_, { event_type::MOUSEENTER }>;
        #[front]
        attribute onmouseleave: EventHandler, get = handlers::element_handler::<{ event_type::MOUSELEAVE }>, set = handlers::set_element_handler::<_, { event_type::MOUSELEAVE }>;
        #[front]
        attribute onmousemove: EventHandler, get = handlers::element_handler::<{ event_type::MOUSEMOVE }>, set = handlers::set_element_handler::<_, { event_type::MOUSEMOVE }>;
        #[front]
        attribute onmouseout: EventHandler, get = handlers::element_handler::<{ event_type::MOUSEOUT }>, set = handlers::set_element_handler::<_, { event_type::MOUSEOUT }>;
        #[front]
        attribute onmouseover: EventHandler, get = handlers::element_handler::<{ event_type::MOUSEOVER }>, set = handlers::set_element_handler::<_, { event_type::MOUSEOVER }>;
        #[front]
        attribute onmouseup: EventHandler, get = handlers::element_handler::<{ event_type::MOUSEUP }>, set = handlers::set_element_handler::<_, { event_type::MOUSEUP }>;
        #[front]
        attribute onmousewheel: EventHandler, get = handlers::element_handler::<{ event_type::MOUSEWHEEL }>, set = handlers::set_element_handler::<_, { event_type::MOUSEWHEEL }>;
        #[front]
        attribute onpause: EventHandler, get = handlers::element_handler::<{ event_type::PAUSE }>, set = handlers::set_element_handler::<_, { event_type::PAUSE }>;
        #[front]
        attribute onplay: EventHandler, get = handlers::element_handler::<{ event_type::PLAY }>, set = handlers::set_element_handler::<_, { event_type::PLAY }>;
        #[front]
        attribute onplaying: EventHandler, get = handlers::element_handler::<{ event_type::PLAYING }>, set = handlers::set_element_handler::<_, { event_type::PLAYING }>;
        #[front]
        attribute onprogress: EventHandler, get = handlers::element_handler::<{ event_type::PROGRESS }>, set = handlers::set_element_handler::<_, { event_type::PROGRESS }>;
        #[front]
        attribute onratechange: EventHandler, get = handlers::element_handler::<{ event_type::RATECHANGE }>, set = handlers::set_element_handler::<_, { event_type::RATECHANGE }>;
        #[front]
        attribute onreset: EventHandler, get = handlers::element_handler::<{ event_type::RESET }>, set = handlers::set_element_handler::<_, { event_type::RESET }>;
        #[front]
        attribute onresize: EventHandler, get = handlers::element_handler::<{ event_type::RESIZE }>, set = handlers::set_element_handler::<_, { event_type::RESIZE }>;
        #[front]
        attribute onscroll: EventHandler, get = handlers::element_handler::<{ event_type::SCROLL }>, set = handlers::set_element_handler::<_, { event_type::SCROLL }>;
        #[front]
        attribute onscrollend: EventHandler, get = handlers::element_handler::<{ event_type::SCROLLEND }>, set = handlers::set_element_handler::<_, { event_type::SCROLLEND }>;
        #[front]
        attribute onsecuritypolicyviolation: EventHandler, get = handlers::element_handler::<{ event_type::SECURITYPOLICYVIOLATION }>, set = handlers::set_element_handler::<_, { event_type::SECURITYPOLICYVIOLATION }>;
        #[front]
        attribute onseeked: EventHandler, get = handlers::element_handler::<{ event_type::SEEKED }>, set = handlers::set_element_handler::<_, { event_type::SEEKED }>;
        #[front]
        attribute onseeking: EventHandler, get = handlers::element_handler::<{ event_type::SEEKING }>, set = handlers::set_element_handler::<_, { event_type::SEEKING }>;
        #[front]
        attribute onselect: EventHandler, get = handlers::element_handler::<{ event_type::SELECT }>, set = handlers::set_element_handler::<_, { event_type::SELECT }>;
        #[front]
        attribute onselectionchange: EventHandler, get = handlers::element_handler::<{ event_type::SELECTIONCHANGE }>, set = handlers::set_element_handler::<_, { event_type::SELECTIONCHANGE }>;
        #[front]
        attribute onselectstart: EventHandler, get = handlers::element_handler::<{ event_type::SELECTSTART }>, set = handlers::set_element_handler::<_, { event_type::SELECTSTART }>;
        #[front]
        attribute onslotchange: EventHandler, get = handlers::element_handler::<{ event_type::SLOTCHANGE }>, set = handlers::set_element_handler::<_, { event_type::SLOTCHANGE }>;
        #[front]
        attribute onstalled: EventHandler, get = handlers::element_handler::<{ event_type::STALLED }>, set = handlers::set_element_handler::<_, { event_type::STALLED }>;
        #[front]
        attribute onsubmit: EventHandler, get = handlers::element_handler::<{ event_type::SUBMIT }>, set = handlers::set_element_handler::<_, { event_type::SUBMIT }>;
        #[front]
        attribute onsuspend: EventHandler, get = handlers::element_handler::<{ event_type::SUSPEND }>, set = handlers::set_element_handler::<_, { event_type::SUSPEND }>;
        #[front]
        attribute ontimeupdate: EventHandler, get = handlers::element_handler::<{ event_type::TIMEUPDATE }>, set = handlers::set_element_handler::<_, { event_type::TIMEUPDATE }>;
        #[front]
        attribute ontoggle: EventHandler, get = handlers::element_handler::<{ event_type::TOGGLE }>, set = handlers::set_element_handler::<_, { event_type::TOGGLE }>;
        #[front]
        attribute ontransitioncancel: EventHandler, get = handlers::element_handler::<{ event_type::TRANSITIONCANCEL }>, set = handlers::set_element_handler::<_, { event_type::TRANSITIONCANCEL }>;
        #[front]
        attribute ontransitionend: EventHandler, get = handlers::element_handler::<{ event_type::TRANSITIONEND }>, set = handlers::set_element_handler::<_, { event_type::TRANSITIONEND }>;
        #[front]
        attribute ontransitionrun: EventHandler, get = handlers::element_handler::<{ event_type::TRANSITIONRUN }>, set = handlers::set_element_handler::<_, { event_type::TRANSITIONRUN }>;
        #[front]
        attribute ontransitionstart: EventHandler, get = handlers::element_handler::<{ event_type::TRANSITIONSTART }>, set = handlers::set_element_handler::<_, { event_type::TRANSITIONSTART }>;
        #[front]
        attribute onvolumechange: EventHandler, get = handlers::element_handler::<{ event_type::VOLUMECHANGE }>, set = handlers::set_element_handler::<_, { event_type::VOLUMECHANGE }>;
        #[front]
        attribute onwaiting: EventHandler, get = handlers::element_handler::<{ event_type::WAITING }>, set = handlers::set_element_handler::<_, { event_type::WAITING }>;
        #[front]
        attribute onwebkitanimationend: EventHandler, get = handlers::element_handler::<{ event_type::WEBKIT_ANIMATION_END }>, set = handlers::set_element_handler::<_, { event_type::WEBKIT_ANIMATION_END }>;
        #[front]
        attribute onwebkitanimationiteration: EventHandler, get = handlers::element_handler::<{ event_type::WEBKIT_ANIMATION_ITERATION }>, set = handlers::set_element_handler::<_, { event_type::WEBKIT_ANIMATION_ITERATION }>;
        #[front]
        attribute onwebkitanimationstart: EventHandler, get = handlers::element_handler::<{ event_type::WEBKIT_ANIMATION_START }>, set = handlers::set_element_handler::<_, { event_type::WEBKIT_ANIMATION_START }>;
        #[front]
        attribute onwebkittransitionend: EventHandler, get = handlers::element_handler::<{ event_type::WEBKIT_TRANSITION_END }>, set = handlers::set_element_handler::<_, { event_type::WEBKIT_TRANSITION_END }>;
        #[front]
        attribute onwheel: EventHandler, get = handlers::element_handler::<{ event_type::WHEEL }>, set = handlers::set_element_handler::<_, { event_type::WHEEL }>;
        attribute dataset: DOMStringMap, get = dataset::dataset;
    }

    // An attribute as its value and, with no animations, the same
    // animated value.
    interface SVGAnimatedString [type = dom::SvgAnimatedString] {
        attribute baseVal: DomString, get = svg::base_val, set = svg::set_base_val;
        attribute animVal: DomString, get = svg::anim_val;
    }

    // Chrome's `SVGMatrix` (Blink's `svg_matrix.idl`), its own interface
    // where SVG 2 makes it `DOMMatrix`'s alias: doubles, the operations'
    // arguments floats, finite.
    interface SVGMatrix [type = dom::SvgMatrix] {
        attribute a: f64, get = svg_geometry::a, set = svg_geometry::set_a;
        attribute b: f64, get = svg_geometry::b, set = svg_geometry::set_b;
        attribute c: f64, get = svg_geometry::c, set = svg_geometry::set_c;
        attribute d: f64, get = svg_geometry::d, set = svg_geometry::set_d;
        attribute e: f64, get = svg_geometry::e, set = svg_geometry::set_e;
        attribute f: f64, get = svg_geometry::f, set = svg_geometry::set_f;
        method multiply(second_matrix: SVGMatrix) -> SVGMatrix = svg_geometry::multiply;
        method inverse() -> SVGMatrix = svg_geometry::inverse;
        method translate(x: f64, y: f64) -> SVGMatrix = svg_geometry::translate;
        method scale(scale_factor: f64) -> SVGMatrix = svg_geometry::scale;
        method scaleNonUniform(scale_factor_x: f64, scale_factor_y: f64) -> SVGMatrix =
            svg_geometry::scale_non_uniform;
        method rotate(angle: f64) -> SVGMatrix = svg_geometry::rotate;
        method rotateFromVector(x: f64, y: f64) -> SVGMatrix = svg_geometry::rotate_from_vector;
        method flipX() -> SVGMatrix = svg_geometry::flip_x;
        method flipY() -> SVGMatrix = svg_geometry::flip_y;
        method skewX(angle: f64) -> SVGMatrix = svg_geometry::skew_x;
        method skewY(angle: f64) -> SVGMatrix = svg_geometry::skew_y;
    }

    // Chrome's `SVGPoint` and `SVGRect`: floats, finite.
    interface SVGPoint [type = dom::SvgPoint] {
        attribute x: f64, get = svg_geometry::point_x, set = svg_geometry::set_point_x;
        attribute y: f64, get = svg_geometry::point_y, set = svg_geometry::set_point_y;
        method matrixTransform(matrix: SVGMatrix) -> SVGPoint = svg_geometry::matrix_transform;
    }
    interface SVGRect [type = dom::SvgRect] {
        attribute x: f64, get = svg_geometry::rect_x, set = svg_geometry::set_rect_x;
        attribute y: f64, get = svg_geometry::rect_y, set = svg_geometry::set_rect_y;
        attribute width: f64, get = svg_geometry::rect_width, set = svg_geometry::set_rect_width;
        attribute height: f64, get = svg_geometry::rect_height, set = svg_geometry::set_rect_height;
    }

    // Geometry's: unrestricted doubles, read in their names' order.
    dictionary DOMPointInit {
        w: Option<f64> = None,
        x: Option<f64> = None,
        y: Option<f64> = None,
        z: Option<f64> = None,
    }

    // The element interfaces; no `transform` (an `SVGAnimatedTransformList`).
    interface SVGGraphicsElement : SVGElement [] {
        method getBBox() -> SVGRect = svg_geometry::get_bbox;
        method getCTM() -> SVGMatrix = svg_geometry::get_ctm;
        method getScreenCTM() -> SVGMatrix = svg_geometry::get_screen_ctm;
    }
    // No `pathLength` (an `SVGAnimatedNumber`); the numbers floats.
    interface SVGGeometryElement : SVGGraphicsElement [] {
        method isPointInFill(point?: DOMPointInit) -> bool = svg_geometry::is_point_in_fill;
        method isPointInStroke(point?: DOMPointInit) -> bool = svg_geometry::is_point_in_stroke;
        method getTotalLength() -> f64 = svg_geometry::get_total_length;
        method getPointAtLength(distance: f64) -> SVGPoint = svg_geometry::get_point_at_length;
    }
    interface SVGSVGElement : SVGGraphicsElement [element svg svg] {
        method createSVGPoint() -> SVGPoint = svg_geometry::create_svg_point;
        method createSVGMatrix() -> SVGMatrix = svg_geometry::create_svg_matrix;
        method createSVGRect() -> SVGRect = svg_geometry::create_svg_rect;
    }
    interface SVGGElement : SVGGraphicsElement [element svg g] {}
    interface SVGDefsElement : SVGGraphicsElement [element svg defs] {}
    interface SVGDescElement : SVGElement [element svg desc] {}
    interface SVGMetadataElement : SVGElement [element svg metadata] {}
    interface SVGTitleElement : SVGElement [element svg title] {}
    interface SVGSymbolElement : SVGGraphicsElement [element svg symbol] {}
    interface SVGUseElement : SVGGraphicsElement [element svg r#use] {}
    interface SVGSwitchElement : SVGGraphicsElement [element svg switch] {}
    // `LinkStyle`'s `sheet` (CSSOM's), as HTML's `<style>`.
    interface SVGStyleElement : SVGElement [element svg style] {
        #[ce_reactions]
        attribute type: DomString, get = reflect::string::<{ local::TYPE }>, set = reflect::set_string::<{ local::TYPE }>;
        #[ce_reactions]
        attribute media: DomString, get = reflect::string::<{ local::MEDIA }>, set = reflect::set_string::<{ local::MEDIA }>;
        #[ce_reactions]
        attribute title: DomString, get = reflect::string::<{ local::TITLE }>, set = reflect::set_string::<{ local::TITLE }>;
        attribute disabled: bool, get = css_style_sheet::style_disabled, set = css_style_sheet::set_style_disabled;
        attribute sheet: Option<CSSStyleSheet>, get = css_style_sheet::sheet;
    }
    interface SVGPathElement : SVGGeometryElement [element svg path] {}
    interface SVGRectElement : SVGGeometryElement [element svg rect] {}
    interface SVGCircleElement : SVGGeometryElement [element svg circle] {}
    interface SVGEllipseElement : SVGGeometryElement [element svg ellipse] {}
    interface SVGLineElement : SVGGeometryElement [element svg line] {}
    interface SVGPolylineElement : SVGGeometryElement [element svg polyline] {}
    interface SVGPolygonElement : SVGGeometryElement [element svg polygon] {}
    // Character numbers are WebIDL `unsigned long`s (a negative one wraps,
    // out of range); the lengths and the angle floats.
    interface SVGTextContentElement : SVGGraphicsElement [] {
        method getNumberOfChars() -> i32 = svg_geometry::get_number_of_chars;
        method getComputedTextLength() -> f64 = svg_geometry::get_computed_text_length;
        method getSubStringLength(charnum: u32, nchars: u32) -> f64 =
            svg_geometry::get_sub_string_length;
        method getStartPositionOfChar(charnum: u32) -> SVGPoint =
            svg_geometry::get_start_position_of_char;
        method getEndPositionOfChar(charnum: u32) -> SVGPoint =
            svg_geometry::get_end_position_of_char;
        method getExtentOfChar(charnum: u32) -> SVGRect = svg_geometry::get_extent_of_char;
        method getRotationOfChar(charnum: u32) -> f64 = svg_geometry::get_rotation_of_char;
        method getCharNumAtPosition(point?: DOMPointInit) -> i32 =
            svg_geometry::get_char_num_at_position;
        method selectSubString(charnum: u32, nchars: u32) = svg_geometry::select_sub_string;
    }
    interface SVGTextPositioningElement : SVGTextContentElement [] {}
    interface SVGTextElement : SVGTextPositioningElement [element svg text] {}
    interface SVGTSpanElement : SVGTextPositioningElement [element svg tspan] {}
    interface SVGTextPathElement : SVGTextContentElement [element svg textPath] {}
    interface SVGImageElement : SVGGraphicsElement [element svg image] {}
    interface SVGForeignObjectElement : SVGGraphicsElement [element svg foreignObject] {}
    interface SVGMarkerElement : SVGElement [element svg marker] {}
    interface SVGGradientElement : SVGElement [] {}
    interface SVGLinearGradientElement : SVGGradientElement [element svg linearGradient] {}
    interface SVGRadialGradientElement : SVGGradientElement [element svg radialGradient] {}
    interface SVGStopElement : SVGElement [element svg stop] {}
    interface SVGPatternElement : SVGElement [element svg pattern] {}
    interface SVGScriptElement : SVGElement [element svg script] {}
    interface SVGAElement : SVGGraphicsElement [element svg a] {}
    interface SVGViewElement : SVGElement [element svg view] {}
    interface SVGClipPathElement : SVGElement [element svg clipPath] {}
    interface SVGMaskElement : SVGElement [element svg mask] {}
    interface SVGFilterElement : SVGElement [element svg filter] {}
    interface SVGFEBlendElement : SVGElement [element svg feBlend] {}
    interface SVGFEColorMatrixElement : SVGElement [element svg feColorMatrix] {}
    interface SVGFEComponentTransferElement : SVGElement [element svg feComponentTransfer] {}
    interface SVGComponentTransferFunctionElement : SVGElement [] {}
    interface SVGFEFuncRElement : SVGComponentTransferFunctionElement [element svg feFuncR] {}
    interface SVGFEFuncGElement : SVGComponentTransferFunctionElement [element svg feFuncG] {}
    interface SVGFEFuncBElement : SVGComponentTransferFunctionElement [element svg feFuncB] {}
    interface SVGFEFuncAElement : SVGComponentTransferFunctionElement [element svg feFuncA] {}
    interface SVGFECompositeElement : SVGElement [element svg feComposite] {}
    interface SVGFEConvolveMatrixElement : SVGElement [element svg feConvolveMatrix] {}
    interface SVGFEDiffuseLightingElement : SVGElement [element svg feDiffuseLighting] {}
    interface SVGFEDistantLightElement : SVGElement [element svg feDistantLight] {}
    interface SVGFEPointLightElement : SVGElement [element svg fePointLight] {}
    interface SVGFESpotLightElement : SVGElement [element svg feSpotLight] {}
    interface SVGFEDisplacementMapElement : SVGElement [element svg feDisplacementMap] {}
    interface SVGFEDropShadowElement : SVGElement [element svg feDropShadow] {}
    interface SVGFEFloodElement : SVGElement [element svg feFlood] {}
    interface SVGFEGaussianBlurElement : SVGElement [element svg feGaussianBlur] {}
    interface SVGFEImageElement : SVGElement [element svg feImage] {}
    interface SVGFEMergeElement : SVGElement [element svg feMerge] {}
    interface SVGFEMergeNodeElement : SVGElement [element svg feMergeNode] {}
    interface SVGFEMorphologyElement : SVGElement [element svg feMorphology] {}
    interface SVGFEOffsetElement : SVGElement [element svg feOffset] {}
    interface SVGFESpecularLightingElement : SVGElement [element svg feSpecularLighting] {}
    interface SVGFETileElement : SVGElement [element svg feTile] {}
    interface SVGFETurbulenceElement : SVGElement [element svg feTurbulence] {}
    interface SVGAnimationElement : SVGElement [] {}
    interface SVGAnimateElement : SVGAnimationElement [element svg animate] {}
    interface SVGSetElement : SVGAnimationElement [element svg set] {}
    interface SVGAnimateMotionElement : SVGAnimationElement [element svg animateMotion] {}
    interface SVGMPathElement : SVGElement [element svg mpath] {}
    interface SVGAnimateTransformElement : SVGAnimationElement [element svg animateTransform] {}

    // `measureText`'s numbers as they were; no `emHeightAscent` and the
    // other experimental ones.
    interface TextMetrics [type = dom::TextMetrics] {
        attribute width: f64, get = canvas::text_metrics::width;
        attribute actualBoundingBoxLeft: f64, get = canvas::text_metrics::actual_bounding_box_left;
        attribute actualBoundingBoxRight: f64, get = canvas::text_metrics::actual_bounding_box_right;
        attribute fontBoundingBoxAscent: f64, get = canvas::text_metrics::font_bounding_box_ascent;
        attribute fontBoundingBoxDescent: f64, get = canvas::text_metrics::font_bounding_box_descent;
        attribute actualBoundingBoxAscent: f64, get = canvas::text_metrics::actual_bounding_box_ascent;
        attribute actualBoundingBoxDescent: f64, get = canvas::text_metrics::actual_bounding_box_descent;
        attribute hangingBaseline: f64, get = canvas::text_metrics::hanging_baseline;
        attribute alphabeticBaseline: f64, get = canvas::text_metrics::alphabetic_baseline;
        attribute ideographicBaseline: f64, get = canvas::text_metrics::ideographic_baseline;
    }

    // A path its `CanvasPath` members build (the context's, the same
    // functions); no `DOMMatrix` for `addPath`'s transform (a
    // `DOMMatrix2DInit`).
    interface Path2D [type = dom::Path2d] {
        constructor(path?: OptionalPath2dOrString) = canvas::construct_path_2d;
        method addPath(path: Path2D, transform?: DOMMatrix2DInit) = canvas::add_path;
        method closePath() = canvas::close_path;
        method moveTo(x: UnrestrictedDouble, y: UnrestrictedDouble) = canvas::move_to;
        method lineTo(x: UnrestrictedDouble, y: UnrestrictedDouble) = canvas::line_to;
        method quadraticCurveTo(
            cpx: UnrestrictedDouble,
            cpy: UnrestrictedDouble,
            x: UnrestrictedDouble,
            y: UnrestrictedDouble,
        ) = canvas::quadratic_curve_to;
        method bezierCurveTo(
            cp1x: UnrestrictedDouble,
            cp1y: UnrestrictedDouble,
            cp2x: UnrestrictedDouble,
            cp2y: UnrestrictedDouble,
            x: UnrestrictedDouble,
            y: UnrestrictedDouble,
        ) = canvas::bezier_curve_to;
        method arcTo(
            x1: UnrestrictedDouble,
            y1: UnrestrictedDouble,
            x2: UnrestrictedDouble,
            y2: UnrestrictedDouble,
            radius: UnrestrictedDouble,
        ) = canvas::arc_to;
        method rect(x: UnrestrictedDouble, y: UnrestrictedDouble, w: UnrestrictedDouble, h: UnrestrictedDouble) = canvas::rect;
        method roundRect(
            x: UnrestrictedDouble,
            y: UnrestrictedDouble,
            w: UnrestrictedDouble,
            h: UnrestrictedDouble,
            radii?: RoundRectRadii,
        ) = canvas::round_rect;
        method arc(
            x: UnrestrictedDouble,
            y: UnrestrictedDouble,
            radius: UnrestrictedDouble,
            startAngle: UnrestrictedDouble,
            endAngle: UnrestrictedDouble,
            counterclockwise?: bool,
        ) = canvas::arc;
        method ellipse(
            x: UnrestrictedDouble,
            y: UnrestrictedDouble,
            radiusX: UnrestrictedDouble,
            radiusY: UnrestrictedDouble,
            rotation: UnrestrictedDouble,
            startAngle: UnrestrictedDouble,
            endAngle: UnrestrictedDouble,
            counterclockwise?: bool,
        ) = canvas::ellipse;
    }

    // Its image as it was when made.
    interface CanvasPattern [type = dom::CanvasPattern] {
        method setTransform(transform?: DOMMatrix2DInit) = canvas::set_pattern_transform;
    }

    // File API's: bytes that never change. `size` a WebIDL `unsigned long
    // long`, given as a `double` (no blob is past 2^53 bytes); no `stream()`.
    interface Blob [type = dom::Blob] {
        constructor(blobParts?: BlobParts, options?: BlobPropertyBag) = blob::construct;
        attribute size: f64, get = blob::size;
        attribute r#type: DomString, get = blob::blob_type;
        method slice(
            start?: OptionalClampedI64,
            end?: OptionalClampedI64,
            contentType?: OptionalString,
        ) -> Blob = blob::slice;
        #[realm]
        method text() -> PromiseOfAny = blob::text;
        #[realm]
        method arrayBuffer() -> PromiseOfAny = blob::array_buffer;
        #[realm]
        method bytes() -> PromiseOfAny = blob::bytes;
    }

    interface File : Blob [type = FileObject] {
        #[front]
        constructor(fileBits: BlobParts, fileName: DomString, options?: FilePropertyBag) = file_reader::construct_file;
        attribute name: DomString, get = file_reader::name;
        attribute lastModified: f64, get = file_reader::last_modified;
        attribute webkitRelativePath: DomString, get = file_reader::webkit_relative_path;
    }

    // `BlobPropertyBag`'s and `lastModified` (a `long long`).
    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary FilePropertyBag {
        r#type: String = String::new(),
        endings: String = String::from("transparent"),
        lastModified: Option<f64> = None,
    }

    // Its members in Chrome's order.
    interface FileReader : EventTarget [type = FileReaderObject] {
        #[front]
        constructor() = file_reader::construct;
        attribute readyState: u16, get = file_reader::ready_state;
        attribute result: ReaderResult, get = file_reader::result;
        // Always `null`: reading bytes in memory doesn't fail.
        attribute error: AlwaysNull, get = file_reader::error;
        #[front]
        attribute onloadstart: EventHandler, get = handlers::port_handler::<{ event_type::LOADSTART }>, set = handlers::set_port_handler::<_, { event_type::LOADSTART }>;
        #[front]
        attribute onprogress: EventHandler, get = handlers::port_handler::<{ event_type::PROGRESS }>, set = handlers::set_port_handler::<_, { event_type::PROGRESS }>;
        #[front]
        attribute onload: EventHandler, get = handlers::port_handler::<{ event_type::LOAD }>, set = handlers::set_port_handler::<_, { event_type::LOAD }>;
        #[front]
        attribute onabort: EventHandler, get = handlers::port_handler::<{ event_type::ABORT }>, set = handlers::set_port_handler::<_, { event_type::ABORT }>;
        #[front]
        attribute onerror: EventHandler, get = handlers::port_handler::<{ event_type::ERROR }>, set = handlers::set_port_handler::<_, { event_type::ERROR }>;
        #[front]
        attribute onloadend: EventHandler, get = handlers::port_handler::<{ event_type::LOADEND }>, set = handlers::set_port_handler::<_, { event_type::LOADEND }>;
        const EMPTY: u32 = 0;
        const LOADING: u32 = 1;
        const DONE: u32 = 2;
        #[calls_script]
        method abort() = file_reader::abort;
        method readAsArrayBuffer(blob: Blob) = file_reader::read_as_array_buffer;
        method readAsBinaryString(blob: Blob) = file_reader::read_as_binary_string;
        method readAsDataURL(blob: Blob) = file_reader::read_as_data_url;
        method readAsText(blob: Blob, encoding?: OptionalString) = file_reader::read_as_text;
    }

    interface ProgressEvent : Event [type = ProgressEventObject] {
        #[front]
        constructor(r#type: DomString, eventInitDict?: ProgressEventInit) = file_reader::construct_progress_event;
        attribute lengthComputable: bool, get = file_reader::length_computable;
        attribute loaded: f64, get = file_reader::loaded;
        attribute total: f64, get = file_reader::total;
    }

    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary ProgressEventInit {
        bubbles: bool = false,
        cancelable: bool = false,
        composed: bool = false,
        lengthComputable: bool = false,
        loaded: f64 = 0.0,
        total: f64 = 0.0,
    }

    // `endings`: `EndingType`'s value, checked by the constructor.
    dictionary BlobPropertyBag {
        r#type: String = String::new(),
        endings: String = String::from("transparent"),
    }

    // Its pixels the engine's `Uint8ClampedArray`, the same each time; no
    // `pixelFormat`.
    interface ImageData [type = dom::ImageData] {
        constructor(sw: u32, sh: u32, settings?: ImageDataSettings) = canvas::construct_image_data;
        #[front]
        constructor(
            data: Uint8ClampedArray,
            sw: u32,
            sh?: Option<u32>,
            settings?: ImageDataSettings,
        ) = canvas::construct_image_data_of;
        attribute width: u32, get = canvas::image_data_width;
        attribute height: u32, get = canvas::image_data_height;
        attribute data: Uint8ClampedArray, get = canvas::image_data_data;
        attribute colorSpace: DomString, get = canvas::image_data_color_space;
    }

    // `colorSpace`: `PredefinedColorSpace`'s value, checked by its member.
    #[expect(non_snake_case, reason = "WebIDL's member names")]
    dictionary ImageDataSettings {
        colorSpace: Option<String> = None,
    }

    // Geometry's, `setTransform`'s: each value with no default.
    dictionary DOMMatrix2DInit {
        a: Option<f64> = None,
        b: Option<f64> = None,
        c: Option<f64> = None,
        d: Option<f64> = None,
        e: Option<f64> = None,
        f: Option<f64> = None,
        m11: Option<f64> = None,
        m12: Option<f64> = None,
        m21: Option<f64> = None,
        m22: Option<f64> = None,
        m41: Option<f64> = None,
        m42: Option<f64> = None,
    }

    // Geometry's rects: what the layout gives script. No constructors or
    // `toJSON()` yet.
    interface DOMRectReadOnly [type = dom::DomRect] {
        constructor(
            x?: OptionalUnrestrictedDouble,
            y?: OptionalUnrestrictedDouble,
            width?: OptionalUnrestrictedDouble,
            height?: OptionalUnrestrictedDouble,
        ) = geometry::construct_read_only_rect;
        #[static_method]
        method fromRect(other?: DOMRectInit) -> DOMRectReadOnly = geometry::read_only_rect_from_rect;
        attribute x: UnrestrictedDouble, get = geometry::x;
        attribute y: UnrestrictedDouble, get = geometry::y;
        attribute width: UnrestrictedDouble, get = geometry::width;
        attribute height: UnrestrictedDouble, get = geometry::height;
        attribute top: UnrestrictedDouble, get = geometry::top;
        attribute right: UnrestrictedDouble, get = geometry::right;
        attribute bottom: UnrestrictedDouble, get = geometry::bottom;
        attribute left: UnrestrictedDouble, get = geometry::left;
        method toJSON() -> DictionaryOut = geometry::to_json;
    }

    // Its own `x`, `y`, `width` and `height` are settable.
    interface DOMRect : DOMRectReadOnly [type = dom::DomRect] {
        constructor(
            x?: OptionalUnrestrictedDouble,
            y?: OptionalUnrestrictedDouble,
            width?: OptionalUnrestrictedDouble,
            height?: OptionalUnrestrictedDouble,
        ) = geometry::construct_rect;
        #[static_method]
        method fromRect(other?: DOMRectInit) -> DOMRect = geometry::rect_from_rect;
        attribute x: UnrestrictedDouble, get = geometry::x, set = geometry::set_x;
        attribute y: UnrestrictedDouble, get = geometry::y, set = geometry::set_y;
        attribute width: UnrestrictedDouble, get = geometry::width, set = geometry::set_width;
        attribute height: UnrestrictedDouble, get = geometry::height, set = geometry::set_height;
    }

    // Its members unrestricted doubles, 0 if missing.
    dictionary DOMRectInit {
        x: Option<f64> = None,
        y: Option<f64> = None,
        width: Option<f64> = None,
        height: Option<f64> = None,
    }

    // `getClientRects`'s: not `iterable<T>`, only `@@iterator`, as WebIDL
    // gives any interface with an indexed getter and a `length`.
    interface DOMRectList [type = dom::DomRectList] {
        attribute length: u32, get = geometry::rect_list_length;
        #[indexed]
        method item(index: u32) -> Option<DOMRect> = geometry::rect_list_item;
    }

    interface CharacterData : Node [] {
        attribute data: LegacyNullToEmptyString, get = character_data::data, set = character_data::set_data;
        // WebIDL `unsigned long`.
        attribute length: u32, get = character_data::length;
        // Offsets and counts WebIDL `unsigned long`, in UTF-16 code units.
        method substringData(offset: u32, count: u32) -> DomString = character_data::substring_data;
        method appendData(data: DomString) = character_data::append_data;
        method insertData(offset: u32, data: DomString) = character_data::insert_data;
        method deleteData(offset: u32, count: u32) = character_data::delete_data;
        method replaceData(offset: u32, count: u32, data: DomString) = character_data::replace_data;
        // `NonDocumentTypeChildNode`'s.
        attribute previousElementSibling: Option<Element>, get = child_node::previous_element_sibling;
        attribute nextElementSibling: Option<Element>, get = child_node::next_element_sibling;
        // `ChildNode`'s.
        #[ce_reactions]
        method before(nodes?: NodesOrStrings) = child_node::before;
        #[ce_reactions]
        method after(nodes?: NodesOrStrings) = child_node::after;
        #[ce_reactions]
        method replaceWith(nodes?: NodesOrStrings) = child_node::replace_with;
        #[ce_reactions]
        method remove() = child_node::remove;
    }

    interface Text : CharacterData [Text] {
        #[front]
        constructor(data?: OptionalString) = character_data::construct_text;
        method splitText(offset: u32) -> NewObject<Text> = character_data::split_text;
        attribute wholeText: DomString, get = character_data::whole_text;
        // `Slottable`'s.
        attribute assignedSlot: Option<HTMLSlotElement>, get = slot::assigned_slot;
    }

    interface Attr : Node [Attr] {
        attribute namespaceURI: Option<DomString>, get = attr::namespace_uri;
        attribute prefix: Option<DomString>, get = attr::prefix;
        attribute localName: DomString, get = attr::local_name;
        attribute name: DomString, get = attr::name;
        #[ce_reactions]
        attribute value: DomString, get = attr::value, set = attr::set_value;
        attribute ownerElement: Option<Element>, get = attr::owner_element;
        attribute specified: bool, get = attr::specified;
    }

    // `[LegacyUnenumerableNamedProperties]`: its names aren't own keys.
    interface NamedNodeMap [type = dom::NamedNodeMap] {
        // WebIDL `unsigned long`.
        attribute length: u32, get = attr::map_length;
        #[indexed]
        method item(index: u32) -> Option<Attr> = attr::map_item;
        method getNamedItem(qualifiedName: DomString) -> Option<Attr> = attr::get_named_item;
        #[named(getter)]
        method namedGetter(qualifiedName: DomString) -> Option<Attr> = attr::get_named_item;
        method getNamedItemNS(namespace: Option<DomString>, localName: DomString) -> Option<Attr> =
            attr::get_named_item_ns;
        #[ce_reactions]
        method setNamedItem(attr: Attr) -> Option<Attr> = attr::set_named_item;
        #[ce_reactions]
        method setNamedItemNS(attr: Attr) -> Option<Attr> = attr::set_named_item;
        #[ce_reactions]
        method removeNamedItem(qualifiedName: DomString) -> Attr = attr::remove_named_item;
        #[ce_reactions]
        method removeNamedItemNS(namespace: Option<DomString>, localName: DomString) -> Attr =
            attr::remove_named_item_ns;
    }

    interface DOMParser [type = dom::DomParser] {
        #[front]
        constructor() = attr::construct_dom_parser;
        // `DOMParserSupportedType`, as a string (`attr::parse_from_string`).
        method parseFromString(string: DomString, r#type: DomString) -> Document = attr::parse_from_string;
    }

    interface CDATASection : Text [] {}

    interface AbstractRange [type = dom::Range] {
        attribute startContainer: Node, get = range::start_container;
        attribute startOffset: u32, get = range::start_offset;
        attribute endContainer: Node, get = range::end_container;
        attribute endOffset: u32, get = range::end_offset;
        attribute collapsed: bool, get = range::collapsed;
    }

    interface StaticRange : AbstractRange [type = dom::Range] {
        constructor(init: StaticRangeInitArg) = range::construct_static;
    }

    interface Range : AbstractRange [type = dom::Range] {
        #[front]
        constructor() = range::construct;
        const START_TO_START: u32 = 0;
        const START_TO_END: u32 = 1;
        const END_TO_END: u32 = 2;
        const END_TO_START: u32 = 3;
        attribute commonAncestorContainer: Node, get = range::common_ancestor_container;
        method setStart(node: Node, offset: u32) = range::set_start;
        method setEnd(node: Node, offset: u32) = range::set_end;
        method setStartBefore(node: Node) = range::set_start_before;
        method setStartAfter(node: Node) = range::set_start_after;
        method setEndBefore(node: Node) = range::set_end_before;
        method setEndAfter(node: Node) = range::set_end_after;
        method collapse(toStart?: Option<bool>) = range::collapse;
        method selectNode(node: Node) = range::select_node;
        method selectNodeContents(node: Node) = range::select_node_contents;
        method compareBoundaryPoints(how: u16, sourceRange: Range) -> i16 = range::compare_boundary_points;
        #[ce_reactions]
        method deleteContents() = range::delete_contents;
        #[ce_reactions]
        method extractContents() -> NewObject<DocumentFragment> = range::extract_contents;
        #[ce_reactions]
        method cloneContents() -> NewObject<DocumentFragment> = range::clone_contents;
        #[ce_reactions]
        method insertNode(node: Node) = range::insert_node;
        #[ce_reactions]
        method surroundContents(newParent: Node) = range::surround_contents;
        // HTML's (DOM Parsing's).
        #[ce_reactions]
        method createContextualFragment(string: DomString) -> NewObject<DocumentFragment> =
            range::create_contextual_fragment;

        method cloneRange() -> Range = range::clone_range;
        method detach() = range::detach;
        method isPointInRange(node: Node, offset: u32) -> bool = range::is_point_in_range;
        method comparePoint(node: Node, offset: u32) -> i16 = range::compare_point;
        method intersectsNode(node: Node) -> bool = range::intersects_node;
        // Its stringifier.
        method toString() -> DomString = range::to_string;
        // CSSOM View's.
        method getClientRects() -> DOMRectList = range::get_client_rects;
        method getBoundingClientRect() -> DOMRect = range::get_bounding_client_rect;
    }

    // A callback interface with constants: its legacy callback interface
    // object has them.
    callback NodeFilter {
        const FILTER_ACCEPT: u32 = 1;
        const FILTER_REJECT: u32 = 2;
        const FILTER_SKIP: u32 = 3;
        const SHOW_ALL: u32 = 0xFFFF_FFFF;
        const SHOW_ELEMENT: u32 = 0x1;
        const SHOW_ATTRIBUTE: u32 = 0x2;
        const SHOW_TEXT: u32 = 0x4;
        const SHOW_CDATA_SECTION: u32 = 0x8;
        const SHOW_ENTITY_REFERENCE: u32 = 0x10;
        const SHOW_ENTITY: u32 = 0x20;
        const SHOW_PROCESSING_INSTRUCTION: u32 = 0x40;
        const SHOW_COMMENT: u32 = 0x80;
        const SHOW_DOCUMENT: u32 = 0x100;
        const SHOW_DOCUMENT_TYPE: u32 = 0x200;
        const SHOW_DOCUMENT_FRAGMENT: u32 = 0x400;
        const SHOW_NOTATION: u32 = 0x800;
    }

    interface Selection [type = dom::Selection] {
        attribute anchorNode: Option<Node>, get = selection::anchor_node;
        attribute anchorOffset: u32, get = selection::anchor_offset;
        attribute focusNode: Option<Node>, get = selection::focus_node;
        attribute focusOffset: u32, get = selection::focus_offset;
        attribute isCollapsed: bool, get = selection::is_collapsed;
        attribute rangeCount: u32, get = selection::range_count;
        attribute r#type: DomString, get = selection::selection_type;
        attribute direction: DomString, get = selection::direction;
        method getRangeAt(index: u32) -> Range = selection::get_range_at;
        // A method that puts the selection in an editing host gives it the
        // focus, out of its operation.
        #[then_script]
        method addRange(range: Range) = selection::add_range;
        method removeRange(range: Range) = selection::remove_range;
        method removeAllRanges() = selection::remove_all_ranges;
        method empty() = selection::remove_all_ranges;
        #[then_script]
        method collapse(node: Option<Node>, offset?: u32) = selection::collapse;
        #[then_script]
        method setPosition(node: Option<Node>, offset?: u32) = selection::collapse;
        #[then_script]
        method collapseToStart() = selection::collapse_to_start;
        #[then_script]
        method collapseToEnd() = selection::collapse_to_end;
        #[then_script]
        method extend(node: Node, offset?: u32) = selection::extend;
        #[then_script]
        method setBaseAndExtent(anchorNode: Node, anchorOffset: u32, focusNode: Node, focusOffset: u32) =
            selection::set_base_and_extent;
        #[then_script]
        method selectAllChildren(node: Node) = selection::select_all_children;
        #[ce_reactions]
        method deleteFromDocument() = selection::delete_from_document;
        method containsNode(node: Node, allowPartialContainment?: Option<bool>) -> bool = selection::contains_node;
        // Its stringifier.
        method toString() -> DomString = selection::to_string;
    }

    interface NodeIterator [type = dom::NodeIterator] {
        attribute root: Node, get = traversal::node_iterator_root;
        attribute referenceNode: Node, get = traversal::reference_node;
        attribute pointerBeforeReferenceNode: bool, get = traversal::pointer_before_reference_node;
        attribute whatToShow: u32, get = traversal::node_iterator_what_to_show;
        attribute filter: AnyOrNull, get = traversal::node_iterator_filter;
        #[calls_script]
        method nextNode() -> Option<Node> = traversal::next_node;
        #[calls_script]
        method previousNode() -> Option<Node> = traversal::previous_node;
        method detach() = traversal::detach;
    }

    interface TreeWalker [type = dom::TreeWalker] {
        attribute root: Node, get = traversal::tree_walker_root;
        attribute whatToShow: u32, get = traversal::tree_walker_what_to_show;
        attribute filter: AnyOrNull, get = traversal::tree_walker_filter;
        attribute currentNode: Node, get = traversal::current_node, set = traversal::set_current_node;
        #[calls_script]
        method parentNode() -> Option<Node> = traversal::parent_node;
        #[calls_script]
        method firstChild() -> Option<Node> = traversal::first_child;
        #[calls_script]
        method lastChild() -> Option<Node> = traversal::last_child;
        #[calls_script]
        method previousSibling() -> Option<Node> = traversal::previous_sibling;
        #[calls_script]
        method nextSibling() -> Option<Node> = traversal::next_sibling;
        #[calls_script]
        method previousNode() -> Option<Node> = traversal::walker_previous_node;
        #[calls_script]
        method nextNode() -> Option<Node> = traversal::walker_next_node;
    }

    interface XMLDocument : Document [] {}

    interface Comment : CharacterData [Comment] {
        #[front]
        constructor(data?: OptionalString) = character_data::construct_comment;
    }

    interface ProcessingInstruction : CharacterData [ProcessingInstruction] {
        attribute target: DomString, get = document_type::name;
    }
}
