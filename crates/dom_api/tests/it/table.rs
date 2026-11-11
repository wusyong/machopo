//! The table: interfaces, their parents and kinds, members and types, and
//! one typed path for each member that has one.

use dom::NodeKind;
use dom_api::{INTERFACES, Interface, MemberKind, TypeDesc, TypedMember, TypedVisitor};

fn interface(name: &str) -> &'static dom_api::InterfaceDesc {
    INTERFACES
        .iter()
        .find(|desc| desc.name == name)
        .unwrap_or_else(|| panic!("no interface {name}"))
}

#[test]
fn interfaces_have_their_parents_and_kinds_parents_first() {
    let names: Vec<_> = INTERFACES.iter().map(|desc| desc.name).collect();
    assert_eq!(
        names,
        [
            "EventTarget",
            "Event",
            "CustomEvent",
            "PopStateEvent",
            "HashChangeEvent",
            "MessageEvent",
            "MessageChannel",
            "MessagePort",
            "CSS",
            "MediaQueryList",
            "MediaQueryListEvent",
            "TransitionEvent",
            "AnimationEvent",
            "AnimationTimeline",
            "DocumentTimeline",
            "AnimationEffect",
            "KeyframeEffect",
            "Animation",
            "CSSAnimation",
            "CSSTransition",
            "AnimationPlaybackEvent",
            "UIEvent",
            "MouseEvent",
            "WheelEvent",
            "FocusEvent",
            "KeyboardEvent",
            "InputEvent",
            "TextEvent",
            "ErrorEvent",
            "PromiseRejectionEvent",
            "Window",
            "Location",
            "Navigator",
            "AbortController",
            "AbortSignal",
            "Headers",
            "Request",
            "Response",
            "IdleDeadline",
            "Scheduling",
            "NetworkInformation",
            "Geolocation",
            "GeolocationPosition",
            "GeolocationCoordinates",
            "GeolocationPositionError",
            "WakeLock",
            "WakeLockSentinel",
            "Performance",
            "PerformanceEntry",
            "PerformanceMark",
            "PerformanceMeasure",
            "PerformanceObserver",
            "PerformanceObserverEntryList",
            "PerformanceResourceTiming",
            "PerformanceNavigationTiming",
            "PerformanceServerTiming",
            "NotRestoredReasons",
            "PerformanceTimingConfidence",
            "PerformanceTiming",
            "PerformanceNavigation",
            "URL",
            "URLSearchParams",
            "Crypto",
            "UserActivation",
            "History",
            "Node",
            "Document",
            "DocumentType",
            "DocumentFragment",
            "ShadowRoot",
            "Element",
            "HTMLElement",
            "HTMLSlotElement",
            "HTMLTemplateElement",
            "HTMLMetaElement",
            "HTMLStyleElement",
            "HTMLHtmlElement",
            "HTMLHeadElement",
            "HTMLTitleElement",
            "HTMLBodyElement",
            "HTMLHeadingElement",
            "HTMLParagraphElement",
            "HTMLHRElement",
            "HTMLPreElement",
            "HTMLQuoteElement",
            "HTMLOListElement",
            "HTMLUListElement",
            "HTMLMenuElement",
            "HTMLLIElement",
            "HTMLDListElement",
            "HTMLDivElement",
            "HTMLDataElement",
            "HTMLTimeElement",
            "HTMLSpanElement",
            "HTMLBRElement",
            "HTMLModElement",
            "HTMLDirectoryElement",
            "HTMLFontElement",
            "HTMLParamElement",
            "HTMLMapElement",
            "HTMLTableElement",
            "HTMLTableCaptionElement",
            "HTMLTableColElement",
            "HTMLTableSectionElement",
            "HTMLTableRowElement",
            "HTMLTableCellElement",
            "HTMLAnchorElement",
            "HTMLAreaElement",
            "HTMLBaseElement",
            "HTMLLinkElement",
            "HTMLScriptElement",
            "HTMLUnknownElement",
            "HTMLIFrameElement",
            "HTMLImageElement",
            "HTMLPictureElement",
            "HTMLSourceElement",
            "HTMLCanvasElement",
            "HTMLFormElement",
            "HTMLInputElement",
            "HTMLButtonElement",
            "HTMLSelectElement",
            "HTMLOptionElement",
            "HTMLOptGroupElement",
            "HTMLTextAreaElement",
            "HTMLLabelElement",
            "HTMLFieldSetElement",
            "HTMLLegendElement",
            "HTMLOutputElement",
            "HTMLDataListElement",
            "HTMLProgressElement",
            "HTMLMeterElement",
            "CustomElementRegistry",
            "StyleSheet",
            "CSSStyleSheet",
            "StyleSheetList",
            "MediaList",
            "CSSRuleList",
            "CSSRule",
            "CSSStyleRule",
            "CSSGroupingRule",
            "CSSConditionRule",
            "CSSMediaRule",
            "CSSSupportsRule",
            "CSSKeyframesRule",
            "CSSKeyframeRule",
            "DOMImplementation",
            "HTMLCollection",
            "HTMLAllCollection",
            "HTMLFormControlsCollection",
            "HTMLOptionsCollection",
            "NodeList",
            "RadioNodeList",
            "DOMTokenList",
            "DOMStringMap",
            "CSSStyleDeclaration",
            "CSSStyleProperties",
            "MutationObserver",
            "MutationRecord",
            "IntersectionObserver",
            "IntersectionObserverEntry",
            "ResizeObserver",
            "ResizeObserverEntry",
            "ResizeObserverSize",
            "CanvasRenderingContext2D",
            "CanvasGradient",
            "SVGElement",
            "SVGAnimatedString",
            "SVGMatrix",
            "SVGPoint",
            "SVGRect",
            "SVGGraphicsElement",
            "SVGGeometryElement",
            "SVGSVGElement",
            "SVGGElement",
            "SVGDefsElement",
            "SVGDescElement",
            "SVGMetadataElement",
            "SVGTitleElement",
            "SVGSymbolElement",
            "SVGUseElement",
            "SVGSwitchElement",
            "SVGStyleElement",
            "SVGPathElement",
            "SVGRectElement",
            "SVGCircleElement",
            "SVGEllipseElement",
            "SVGLineElement",
            "SVGPolylineElement",
            "SVGPolygonElement",
            "SVGTextContentElement",
            "SVGTextPositioningElement",
            "SVGTextElement",
            "SVGTSpanElement",
            "SVGTextPathElement",
            "SVGImageElement",
            "SVGForeignObjectElement",
            "SVGMarkerElement",
            "SVGGradientElement",
            "SVGLinearGradientElement",
            "SVGRadialGradientElement",
            "SVGStopElement",
            "SVGPatternElement",
            "SVGScriptElement",
            "SVGAElement",
            "SVGViewElement",
            "SVGClipPathElement",
            "SVGMaskElement",
            "SVGFilterElement",
            "SVGFEBlendElement",
            "SVGFEColorMatrixElement",
            "SVGFEComponentTransferElement",
            "SVGComponentTransferFunctionElement",
            "SVGFEFuncRElement",
            "SVGFEFuncGElement",
            "SVGFEFuncBElement",
            "SVGFEFuncAElement",
            "SVGFECompositeElement",
            "SVGFEConvolveMatrixElement",
            "SVGFEDiffuseLightingElement",
            "SVGFEDistantLightElement",
            "SVGFEPointLightElement",
            "SVGFESpotLightElement",
            "SVGFEDisplacementMapElement",
            "SVGFEDropShadowElement",
            "SVGFEFloodElement",
            "SVGFEGaussianBlurElement",
            "SVGFEImageElement",
            "SVGFEMergeElement",
            "SVGFEMergeNodeElement",
            "SVGFEMorphologyElement",
            "SVGFEOffsetElement",
            "SVGFESpecularLightingElement",
            "SVGFETileElement",
            "SVGFETurbulenceElement",
            "SVGAnimationElement",
            "SVGAnimateElement",
            "SVGSetElement",
            "SVGAnimateMotionElement",
            "SVGMPathElement",
            "SVGAnimateTransformElement",
            "TextMetrics",
            "Path2D",
            "CanvasPattern",
            "Blob",
            "File",
            "FileReader",
            "ProgressEvent",
            "ImageData",
            "DOMRectReadOnly",
            "DOMRect",
            "DOMRectList",
            "CharacterData",
            "Text",
            "Attr",
            "NamedNodeMap",
            "DOMParser",
            "CDATASection",
            "AbstractRange",
            "StaticRange",
            "Range",
            "Selection",
            "NodeIterator",
            "TreeWalker",
            "XMLDocument",
            "Comment",
            "ProcessingInstruction"
        ]
    );
    for (index, desc) in INTERFACES.iter().enumerate() {
        if let Some(parent) = desc.parent {
            let parent = names.iter().position(|name| *name == parent).unwrap();
            assert!(parent < index, "{} after its parent", desc.name);
        }
    }
    assert_eq!(interface("EventTarget").parent, None);
    assert_eq!(interface("Node").parent, Some("EventTarget"));
    assert_eq!(interface("Text").parent, Some("CharacterData"));
    assert_eq!(interface("Element").kinds, [NodeKind::Element]);
    assert!(interface("CharacterData").kinds.is_empty());
    // Node interfaces' objects are nodes; the others' aren't, and have no
    // kinds.
    for desc in INTERFACES {
        let node = !matches!(
            desc.name,
            "EventTarget"
                | "Event"
                | "CustomEvent"
                | "MessageEvent"
                | "MessageChannel"
                | "MessagePort"
                | "UIEvent"
                | "MouseEvent"
                | "WheelEvent"
                | "FocusEvent"
                | "KeyboardEvent"
                | "InputEvent"
                | "TextEvent"
                | "ErrorEvent"
                | "PromiseRejectionEvent"
                | "Window"
                | "Location"
                | "History"
                | "PopStateEvent"
                | "HashChangeEvent"
                | "HTMLCollection"
                | "HTMLAllCollection"
                | "HTMLFormControlsCollection"
                | "HTMLOptionsCollection"
                | "NodeList"
                | "RadioNodeList"
                | "DOMTokenList"
                | "DOMStringMap"
                | "CSSStyleDeclaration"
                | "CSSStyleProperties"
                | "MutationObserver"
                | "MutationRecord"
                | "IntersectionObserver"
                | "IntersectionObserverEntry"
                | "ResizeObserver"
                | "ResizeObserverEntry"
                | "ResizeObserverSize"
                | "CanvasRenderingContext2D"
                | "CanvasGradient"
                | "CanvasPattern"
                | "SVGAnimatedString"
                | "SVGMatrix"
                | "SVGPoint"
                | "SVGRect"
                | "TextMetrics"
                | "Path2D"
                | "Blob"
                | "ImageData"
                | "DOMRectReadOnly"
                | "DOMRect"
                | "DOMRectList"
                | "DOMImplementation"
                | "CustomElementRegistry"
                | "Navigator"
                | "UserActivation"
                | "AbortController"
                | "AbortSignal"
                | "Headers"
                | "Request"
                | "Response"
                | "IdleDeadline"
                | "Scheduling"
                | "NetworkInformation"
                | "Geolocation"
                | "GeolocationPosition"
                | "GeolocationCoordinates"
                | "GeolocationPositionError"
                | "WakeLock"
                | "WakeLockSentinel"
                | "Performance"
                | "PerformanceEntry"
                | "PerformanceMark"
                | "PerformanceMeasure"
                | "PerformanceObserver"
                | "PerformanceObserverEntryList"
                | "PerformanceResourceTiming"
                | "PerformanceNavigationTiming"
                | "PerformanceServerTiming"
                | "NotRestoredReasons"
                | "PerformanceTimingConfidence"
                | "PerformanceTiming"
                | "PerformanceNavigation"
                | "URL"
                | "URLSearchParams"
                | "Crypto"
                | "File"
                | "FileReader"
                | "ProgressEvent"
                | "CSS"
                | "MediaQueryList"
                | "MediaQueryListEvent"
                | "TransitionEvent"
                | "AnimationEvent"
                | "AnimationTimeline"
                | "DocumentTimeline"
                | "AnimationEffect"
                | "KeyframeEffect"
                | "Animation"
                | "CSSAnimation"
                | "CSSTransition"
                | "AnimationPlaybackEvent"
                | "StyleSheet"
                | "CSSStyleSheet"
                | "StyleSheetList"
                | "MediaList"
                | "CSSRuleList"
                | "CSSRule"
                | "CSSStyleRule"
                | "CSSGroupingRule"
                | "CSSConditionRule"
                | "CSSMediaRule"
                | "CSSSupportsRule"
                | "CSSKeyframesRule"
                | "CSSKeyframeRule"
                | "NamedNodeMap"
                | "DOMParser"
                | "AbstractRange"
                | "StaticRange"
                | "Range"
                | "NodeIterator"
                | "TreeWalker"
                | "Selection"
        );
        assert_eq!(desc.node, node, "{}", desc.name);
        assert!(node || desc.kinds.is_empty(), "{}", desc.name);
    }
}

/// An interface accepts its own kinds and its descendants'.
#[test]
fn interfaces_accept_their_kinds_and_their_descendants() {
    use dom_api::interfaces::{
        CharacterData, Comment, Document, DocumentFragment, DocumentType, Element, Node,
        ProcessingInstruction, Text,
    };
    let every = [
        NodeKind::Document,
        NodeKind::DocumentType,
        NodeKind::DocumentFragment,
        NodeKind::Element,
        NodeKind::Text,
        NodeKind::Comment,
        NodeKind::ProcessingInstruction,
        NodeKind::Attr,
    ];
    for kind in every {
        assert!(Node::accepts(kind), "{kind:?}");
        // A node is an event target.
        assert!(dom_api::interfaces::EventTarget::accepts(kind), "{kind:?}");
        assert!(!dom_api::interfaces::Event::accepts(kind), "{kind:?}");
        // Each kind's most derived interface is one of the table's.
        assert!(
            INTERFACES.iter().any(|desc| desc.kinds.contains(&kind)),
            "{kind:?}"
        );
    }
    for kind in [
        NodeKind::Text,
        NodeKind::Comment,
        NodeKind::ProcessingInstruction,
    ] {
        assert!(CharacterData::accepts(kind), "{kind:?}");
    }
    assert!(Comment::accepts(NodeKind::Comment) && !Comment::accepts(NodeKind::Text));
    assert!(ProcessingInstruction::accepts(
        NodeKind::ProcessingInstruction
    ));
    assert!(
        DocumentType::accepts(NodeKind::DocumentType) && !DocumentType::accepts(NodeKind::Document)
    );
    assert!(DocumentFragment::accepts(NodeKind::DocumentFragment));
    assert!(!Element::accepts(NodeKind::DocumentFragment));
    assert!(CharacterData::accepts(NodeKind::Text));
    assert!(Text::accepts(NodeKind::Text));
    assert!(!CharacterData::accepts(NodeKind::Element));
    assert!(Element::accepts(NodeKind::Element));
    assert!(!Element::accepts(NodeKind::Text));
    assert!(!Element::accepts(NodeKind::Document));
    assert!(Document::accepts(NodeKind::Document));
    assert!(!Document::accepts(NodeKind::Element));
    assert!(dom_api::interfaces::Attr::accepts(NodeKind::Attr));
    assert!(!Element::accepts(NodeKind::Attr) && !CharacterData::accepts(NodeKind::Attr));
}

#[test]
fn members_are_described_with_their_kinds_and_types() {
    let node = interface("Node");
    let member = |name: &str, kind: MemberKind| {
        node.members
            .iter()
            .find(|m| {
                m.name == name && std::mem::discriminant(&m.kind) == std::mem::discriminant(&kind)
            })
            .unwrap_or_else(|| panic!("no {name}"))
    };
    assert_eq!(
        member("TEXT_NODE", MemberKind::Constant(0)).kind,
        MemberKind::Constant(3)
    );
    assert_eq!(
        member("DOCUMENT_NODE", MemberKind::Constant(0)).kind,
        MemberKind::Constant(9)
    );
    let insert = member("insertBefore", MemberKind::Method);
    assert_eq!(insert.required(), 2);
    assert_eq!(insert.params[0].name, "node");
    assert_eq!(insert.params[0].ty, TypeDesc::Node("Node"));
    assert_eq!(insert.params[1].ty, TypeDesc::NullableNode("Node"));
    assert_eq!(insert.returns, TypeDesc::Node("Node"));
    assert_eq!(member("hasChildNodes", MemberKind::Method).required(), 0);
    assert_eq!(
        member("nodeType", MemberKind::Getter).returns,
        TypeDesc::U32
    );
    assert_eq!(
        member("parentNode", MemberKind::Getter).returns,
        TypeDesc::NullableNode("Node")
    );
    let set = member("textContent", MemberKind::Setter);
    assert_eq!(set.params[0].ty, TypeDesc::NullableString);
    assert_eq!(set.returns, TypeDesc::Undefined);
    let element = interface("Element");
    let set_attribute = element
        .members
        .iter()
        .find(|m| m.name == "setAttribute")
        .unwrap();
    assert_eq!(set_attribute.returns, TypeDesc::Undefined);
    assert_eq!(
        set_attribute
            .params
            .iter()
            .map(|p| p.ty)
            .collect::<Vec<_>>(),
        [TypeDesc::String, TypeDesc::String]
    );
    let data = interface("CharacterData")
        .members
        .iter()
        .find(|m| m.name == "data")
        .unwrap();
    // `[LegacyNullToEmptyString]`: read as a string.
    assert_eq!(data.returns, TypeDesc::String);
    // `[PutForwards=value]`: its setter takes a string.
    let class_list = |kind| {
        element
            .members
            .iter()
            .find(|m| m.name == "classList" && m.kind == kind)
            .unwrap()
    };
    assert_eq!(
        class_list(MemberKind::Getter).returns,
        TypeDesc::Node("DOMTokenList")
    );
    assert_eq!(
        class_list(MemberKind::Setter).params[0].ty,
        TypeDesc::String
    );
    // Value iterators, and named properties' anonymous operations.
    assert!(interface("NodeList").iterable && interface("DOMTokenList").iterable);
    assert!(!interface("HTMLCollection").iterable);
    let map = interface("DOMStringMap");
    assert!(map.override_builtins && !interface("NodeList").override_builtins);
    let named: Vec<_> = map.members.iter().map(|m| m.named).collect();
    assert_eq!(
        named,
        [
            Some(dom_api::Named::Getter),
            Some(dom_api::Named::Setter),
            Some(dom_api::Named::Deleter),
            Some(dom_api::Named::Names),
        ]
    );
    assert!(element.members.iter().all(|m| m.named.is_none()));
    let add = interface("DOMTokenList")
        .members
        .iter()
        .find(|m| m.name == "add")
        .unwrap();
    assert_eq!(add.params[0].ty, TypeDesc::Strings);
    assert_eq!(add.required(), 0);
}

struct Paths(Vec<(usize, usize, &'static str, MemberKind)>);

impl TypedVisitor for Paths {
    fn any<M: TypedMember>(&mut self) {
        self.0.push((M::INTERFACE, M::MEMBER, M::NAME, M::KIND));
    }
}

/// Each typed path's interface's and member's names, and the
/// `TypedVisitor` method it was visited by.
struct ByKind(Vec<(&'static str, &'static str, &'static str)>);

impl ByKind {
    fn push<M: TypedMember>(&mut self, method: &'static str) {
        self.0
            .push((INTERFACES[M::INTERFACE].name, M::NAME, method));
    }
}

impl TypedVisitor for ByKind {
    fn any<M: TypedMember>(&mut self) {
        unreachable!("every member's kind has its method");
    }
    fn getter<M: TypedMember>(&mut self) {
        self.push::<M>("getter");
    }
    fn setter<M: TypedMember>(&mut self) {
        self.push::<M>("setter");
    }
    fn method<M: TypedMember>(&mut self) {
        self.push::<M>("method");
    }
    fn static_member<M: TypedMember>(&mut self) {
        self.push::<M>("static_member");
    }
    fn cross_origin<M: TypedMember>(&mut self) {
        let accessor = match M::KIND {
            MemberKind::Getter => "cross_origin getter",
            MemberKind::Setter => "cross_origin setter",
            _ => "cross_origin method",
        };
        self.push::<M>(accessor);
    }
    fn constructor<M: TypedMember>(&mut self) {
        self.push::<M>("constructor");
    }
    fn legacy_factory<M: TypedMember>(&mut self) {
        self.push::<M>("legacy_factory");
    }
    fn indexed<M: TypedMember>(&mut self) {
        self.push::<M>("indexed");
    }
    fn indexed_setter<M: TypedMember>(&mut self) {
        self.push::<M>("indexed_setter");
    }
    fn named<M: TypedMember>(&mut self) {
        self.push::<M>("named");
    }
    fn pairs<M: TypedMember>(&mut self) {
        self.push::<M>("pairs");
    }
    fn observable<M: TypedMember>(&mut self) {
        self.push::<M>("observable");
    }
}

/// Each typed path is visited by its kind's method, as its description
/// says; the cross-origin ones are HTML's CrossOriginProperties' members
/// (a window's and a location's), no others.
#[test]
fn typed_paths_are_visited_by_their_kinds() {
    let mut by_kind = ByKind(Vec::new());
    dom_api::visit_typed(&mut by_kind);
    let mut expected = Vec::new();
    for desc in INTERFACES {
        for member in desc.members {
            let method = if member.pairs {
                "pairs"
            } else if member.named.is_some() {
                "named"
            } else if member.observable.is_some() {
                "observable"
            } else if member.indexed {
                "indexed"
            } else {
                match member.kind {
                    MemberKind::Constant(_) => continue,
                    MemberKind::Constructor => "constructor",
                    MemberKind::LegacyFactory => "legacy_factory",
                    MemberKind::IndexedSetter => "indexed_setter",
                    _ if member.is_static => "static_member",
                    MemberKind::Getter => "getter",
                    MemberKind::Setter => "setter",
                    MemberKind::Method => "method",
                }
            };
            expected.push((desc.name, member.name, method));
        }
    }
    let cross_origin: Vec<_> = by_kind
        .0
        .iter()
        .filter(|(_, _, method)| method.starts_with("cross_origin"))
        .copied()
        .collect();
    assert_eq!(
        cross_origin,
        [
            ("Window", "window", "cross_origin getter"),
            ("Window", "self", "cross_origin getter"),
            ("Window", "frames", "cross_origin getter"),
            ("Window", "length", "cross_origin getter"),
            ("Window", "top", "cross_origin getter"),
            ("Window", "opener", "cross_origin getter"),
            ("Window", "focus", "cross_origin method"),
            ("Window", "blur", "cross_origin method"),
            ("Window", "parent", "cross_origin getter"),
            ("Window", "location", "cross_origin getter"),
            ("Window", "location", "cross_origin setter"),
            ("Window", "postMessage", "cross_origin method"),
            ("Window", "close", "cross_origin method"),
            ("Window", "closed", "cross_origin getter"),
            ("Location", "href", "cross_origin setter"),
            ("Location", "replace", "cross_origin method"),
        ]
    );
    // The others, as their descriptions say.
    let regular: Vec<_> = by_kind
        .0
        .iter()
        .map(|&(interface, name, method)| {
            (
                interface,
                name,
                method.strip_prefix("cross_origin ").unwrap_or(method),
            )
        })
        .collect();
    assert_eq!(regular, expected);
}

/// Every member but a constant has one typed path, its interface and index
/// those of its description.
#[test]
fn every_member_but_constants_has_one_typed_path() {
    let mut paths = Paths(Vec::new());
    dom_api::visit_typed(&mut paths);
    let mut expected = Vec::new();
    for (interface, desc) in INTERFACES.iter().enumerate() {
        for (index, member) in desc.members.iter().enumerate() {
            if !matches!(member.kind, MemberKind::Constant(_)) {
                expected.push((interface, index, member.name, member.kind));
            }
        }
    }
    assert_eq!(paths.0, expected);
}

/// What a member's typed path says it takes and gives (`TypedMember::Args`,
/// `Ret`), as the table's descriptions of types.
trait ArgDescs {
    fn descs() -> Vec<TypeDesc>;
}

macro_rules! arg_descs {
    ($(($($t:ident),*))*) => {$(
        impl<$($t: dom_api::ArgType),*> ArgDescs for ($($t,)*) {
            fn descs() -> Vec<TypeDesc> {
                vec![$($t::DESC),*]
            }
        }
    )*};
}

// Up to `initMouseEvent`'s fifteen.
arg_descs! {
    ()
    (A0)
    (A0, A1)
    (A0, A1, A2)
    (A0, A1, A2, A3)
    (A0, A1, A2, A3, A4)
    (A0, A1, A2, A3, A4, A5)
    (A0, A1, A2, A3, A4, A5, A6)
    (A0, A1, A2, A3, A4, A5, A6, A7)
    (A0, A1, A2, A3, A4, A5, A6, A7, A8)
    (A0, A1, A2, A3, A4, A5, A6, A7, A8, A9)
    (A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10)
    (A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11)
    (A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12)
    (A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13)
    (A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14)
    (A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15)
}

/// One row `for_each_member!` gave: the interface's and member's names,
/// the kind, the typed path's own constants and types, and the row's
/// class (`plain` or `object`).
type Row = (
    &'static str,
    &'static str,
    MemberKind,
    (usize, usize, &'static str, MemberKind),
    Vec<TypeDesc>,
    TypeDesc,
    &'static str,
);

fn row<M: TypedMember>(
    interface: &'static str,
    member: &'static str,
    kind: MemberKind,
    class: &'static str,
) -> Row
where
    M::Args: ArgDescs,
{
    (
        interface,
        member,
        kind,
        (M::INTERFACE, M::MEMBER, M::NAME, M::KIND),
        <M::Args as ArgDescs>::descs(),
        <M::Ret as dom_api::RetType>::DESC,
        class,
    )
}

/// Whether a type a plain member may take or give: a node, a string, a
/// number, a boolean.
fn is_plain(ty: TypeDesc) -> bool {
    match ty {
        TypeDesc::Node(interface) | TypeDesc::NullableNode(interface) => {
            interface_desc_node(interface)
        }
        // A function (an event handler's) a guest can't hold either.
        TypeDesc::Dictionary(_)
        | TypeDesc::DictOrBool(_)
        | TypeDesc::Callback(_)
        | TypeDesc::Function(_)
        | TypeDesc::Sequence(_)
        | TypeDesc::FrozenArray(_)
        | TypeDesc::Doubles
        | TypeDesc::Numbers
        | TypeDesc::FrozenStrings
        | TypeDesc::ObservableArray(_)
        | TypeDesc::Promise(_)
        | TypeDesc::Strings
        | TypeDesc::Slottables
        | TypeDesc::NodesOrStrings
        | TypeDesc::StringPairs => false,
        _ => true,
    }
}

/// A node interface's objects, which a guest holds: not a shadow root's,
/// which its kind (a fragment's) doesn't tell.
fn interface_desc_node(name: &str) -> bool {
    interface(name).node && name != "ShadowRoot"
}

/// `for_each_member!` names every typed path `visit_typed` visits, in the
/// same order, by its own interface, name and kind; each one's argument and
/// result types are its description's.
#[test]
fn for_each_member_lists_every_typed_path_with_its_types() {
    let mut rows: Vec<Row> = Vec::new();
    macro_rules! collect {
        ($interface:ident, $member:ident, $kind:ident, $path:ty, $class:ident) => {
            rows.push(row::<$path>(
                stringify!($interface),
                stringify!($member),
                MemberKind::$kind,
                stringify!($class),
            ));
        };
    }
    dom_api::for_each_member!(collect);

    let mut paths = Paths(Vec::new());
    dom_api::visit_typed(&mut paths);
    assert_eq!(rows.iter().map(|row| row.3).collect::<Vec<_>>(), paths.0);
    for (interface, member, kind, (index, at, name, path_kind), args, ret, class) in rows {
        let desc = &INTERFACES[index].members[at];
        // A raw name (`r#type`) is the table's own.
        let member = member.trim_start_matches("r#");
        // An anonymous indexed getter's (`#[indexed(getter)]`) or setter's
        // (`#[indexed(setter)]`) is empty.
        let name = if name.is_empty() {
            assert!(
                (desc.indexed || desc.kind == MemberKind::IndexedSetter) && desc.name.is_empty(),
                "{interface}.{member}"
            );
            member
        } else {
            name
        };
        assert_eq!(
            (interface, member, kind),
            (INTERFACES[index].name, name, path_kind)
        );
        // Plain: a node interface's member (not an element interface's),
        // not a constructor nor a static operation, of plain types only; an
        // attribute's setter its getter's type too (`[PutForwards]`).
        let getter = (kind == MemberKind::Setter).then(|| {
            INTERFACES[index]
                .members
                .iter()
                .find(|other| other.name == member && other.kind == MemberKind::Getter)
                .expect("a setter's getter")
                .returns
        });
        let plain = INTERFACES[index].node
            && INTERFACES[index].element.is_none()
            && interface != "ShadowRoot"
            && kind != MemberKind::Constructor
            && !desc.is_static
            && args
                .iter()
                .chain([&ret])
                .chain(getter.as_ref())
                .all(|ty| is_plain(*ty));
        assert_eq!(
            class,
            if plain { "plain" } else { "object" },
            "{interface}.{member}"
        );
        assert_eq!(
            args,
            desc.params.iter().map(|param| param.ty).collect::<Vec<_>>(),
            "{interface}.{member}"
        );
        // A setter's description says it returns nothing; its path's
        // result is `()`, which is.
        assert_eq!(ret, desc.returns, "{interface}.{member}");
    }
}

/// The `DOMException`s' names and legacy codes (WebIDL's table), and the
/// DOM's errors as them.
#[test]
fn exceptions_have_their_names_and_codes() {
    use dom_api::{ApiError, ExceptionName};
    let table = [
        (ExceptionName::HierarchyRequest, "HierarchyRequestError", 3),
        (ExceptionName::InvalidCharacter, "InvalidCharacterError", 5),
        (ExceptionName::NotFound, "NotFoundError", 8),
        (ExceptionName::Namespace, "NamespaceError", 14),
    ];
    for (name, string, code) in table {
        assert_eq!((name.name(), name.code()), (string, code));
    }
    assert_eq!(
        ApiError::from(dom::DomError::HierarchyRequest),
        ApiError::dom(ExceptionName::HierarchyRequest)
    );
    assert_eq!(
        ApiError::from(dom::DomError::NotFound),
        ApiError::dom(ExceptionName::NotFound)
    );
}

/// The tables worked out at compile time are the table's: an interface
/// accepts a kind exactly when some interface it is an ancestor of (or
/// is) lists it; a kind's wrapper is the interface listing it.
#[test]
fn the_compile_time_tables_are_the_tables() {
    let every = [
        NodeKind::Document,
        NodeKind::DocumentType,
        NodeKind::DocumentFragment,
        NodeKind::Element,
        NodeKind::Text,
        NodeKind::Comment,
        NodeKind::ProcessingInstruction,
    ];
    let ancestor_or_self = |ancestor: usize, mut at: usize| loop {
        if at == ancestor {
            return true;
        }
        match INTERFACES[at].parent {
            Some(parent) => {
                at = INTERFACES
                    .iter()
                    .position(|desc| desc.name == parent)
                    .unwrap()
            }
            None => return false,
        }
    };
    for (interface, desc) in INTERFACES.iter().enumerate() {
        for kind in every {
            let listed = INTERFACES.iter().enumerate().any(|(at, other)| {
                let has = other.kinds.contains(&kind)
                    || (kind == NodeKind::Element && other.element.is_some());
                has && ancestor_or_self(interface, at)
            });
            assert_eq!(
                dom_api::typed::accepts(interface, kind),
                listed,
                "{} {kind:?}",
                desc.name
            );
        }
    }
    for kind in every {
        let listing = INTERFACES
            .iter()
            .position(|desc| desc.kinds.contains(&kind))
            .unwrap();
        assert_eq!(dom_api::node_interface(kind), listing, "{kind:?}");
    }
}

/// A static operation and a regular one of the same name (`Response.json()`,
/// `response.json()`) are no overload's forms.
#[test]
fn a_static_and_a_regular_operation_are_no_overload() {
    let response = dom_api::declare::interface_index("Response");
    let members = dom_api::INTERFACES[response].members;
    let jsons: Vec<usize> = (0..members.len())
        .filter(|&index| members[index].name == "json")
        .collect();
    assert_eq!(jsons.len(), 2, "the static and the regular one");
    for index in jsons {
        assert!(
            !dom_api::declare::is_overload(response, index),
            "{}",
            members[index].is_static
        );
    }
}

/// Each typed path's interface's and member's names, its kind, and
/// whether it is plain.
struct Plain(Vec<(&'static str, &'static str, MemberKind, bool)>);

impl TypedVisitor for Plain {
    fn any<M: TypedMember>(&mut self) {
        self.0
            .push((INTERFACES[M::INTERFACE].name, M::NAME, M::KIND, M::PLAIN));
    }
}

/// A member is plain (`TypedMember::PLAIN`) when it is an object's getter,
/// setter or method whose arguments are strings, numbers, booleans, nodes
/// and other objects of the DOM's, and whose result is one of those, null
/// or nothing (a new node too, its wrapper made ahead); not when it takes
/// or gives anything else (a callback, a sequence), is static, or its
/// function is given the front end.
#[test]
fn members_of_strings_nodes_numbers_and_booleans_are_plain() {
    let mut plain = Plain(Vec::new());
    dom_api::visit_typed(&mut plain);
    let is_plain = |interface: &str, name: &str, kind: MemberKind| {
        let found: Vec<bool> = plain
            .0
            .iter()
            .filter(|member| (member.0, member.1, member.2) == (interface, name, kind))
            .map(|member| member.3)
            .collect();
        assert_eq!(found.len(), 1, "{interface}.{name}: one typed path");
        found[0]
    };
    for (interface, name, kind) in [
        ("Node", "nodeType", MemberKind::Getter),
        ("Node", "firstChild", MemberKind::Getter),
        ("Node", "parentNode", MemberKind::Getter),
        ("Node", "textContent", MemberKind::Getter),
        ("Node", "textContent", MemberKind::Setter),
        ("Node", "appendChild", MemberKind::Method),
        ("Node", "contains", MemberKind::Method),
        ("Element", "tagName", MemberKind::Getter),
        ("Element", "id", MemberKind::Getter),
        ("Element", "id", MemberKind::Setter),
        ("Element", "className", MemberKind::Setter),
        ("Element", "getAttribute", MemberKind::Method),
        ("Element", "setAttribute", MemberKind::Method),
        ("Element", "hasAttribute", MemberKind::Method),
        ("Element", "removeAttribute", MemberKind::Method),
        ("Event", "timeStamp", MemberKind::Getter),
        ("Event", "type", MemberKind::Getter),
        // Numbers and booleans as arguments; objects that aren't nodes.
        ("Node", "cloneNode", MemberKind::Method),
        ("Node", "childNodes", MemberKind::Getter),
        ("Element", "classList", MemberKind::Getter),
        ("Element", "toggleAttribute", MemberKind::Method),
        ("CharacterData", "substringData", MemberKind::Method),
        ("NodeList", "item", MemberKind::Method),
        ("Event", "target", MemberKind::Getter),
        // New nodes, whose wrappers are made ahead of the operation.
        ("Document", "createElement", MemberKind::Method),
        ("Document", "createTextNode", MemberKind::Method),
        ("Document", "createDocumentFragment", MemberKind::Method),
        ("Text", "splitText", MemberKind::Method),
        // Callbacks (the front end's handle, handed back to it), event
        // handlers, a dictionary of booleans; their functions are given
        // the front end's side in the operation.
        ("EventTarget", "removeEventListener", MemberKind::Method),
        ("Window", "requestAnimationFrame", MemberKind::Method),
        ("Window", "cancelAnimationFrame", MemberKind::Method),
        ("HTMLElement", "onclick", MemberKind::Setter),
        ("Window", "onload", MemberKind::Setter),
    ] {
        assert!(is_plain(interface, name, kind), "{interface}.{name}");
    }
    for (interface, name, kind) in [
        // A dictionary with an object (`signal`); a method that leaves
        // script to run; rest arguments.
        ("EventTarget", "addEventListener", MemberKind::Method),
        ("EventTarget", "dispatchEvent", MemberKind::Method),
        ("Window", "setTimeout", MemberKind::Method),
        // An event handler's getter may compile its source: script.
        ("HTMLElement", "onclick", MemberKind::Getter),
        // Their functions hand the front end what it keeps for after the
        // operation (a value to throw, an `ImageData`'s pixels): their
        // result types say so.
        ("AbortSignal", "throwIfAborted", MemberKind::Method),
        (
            "CanvasRenderingContext2D",
            "getImageData",
            MemberKind::Method,
        ),
        // A sequence.
        ("Element", "getAttributeNames", MemberKind::Method),
        // Static; its function given the front end.
        // Static.
        ("CSS", "escape", MemberKind::Method),
    ] {
        assert!(!is_plain(interface, name, kind), "{interface}.{name}");
    }
    // Only getters, setters and methods are (an indexed setter is
    // declared as a method: an engine calls it as an interceptor).
    assert!(plain.0.iter().all(|member| {
        !member.3
            || matches!(
                member.2,
                MemberKind::Getter
                    | MemberKind::Setter
                    | MemberKind::Method
                    | MemberKind::IndexedSetter
            )
    }));
    let count = plain.0.iter().filter(|member| member.3).count();
    assert!(count > plain.0.len() / 4, "{count} plain members");
}
