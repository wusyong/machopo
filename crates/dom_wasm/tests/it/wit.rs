//! `wit/machopo-dom.wit` is what the table generates.

use std::path::Path;

/// `MACHOPO_UPDATE_GOLDEN=1` writes it.
#[test]
fn the_wit_is_the_tables() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("wit/machopo-dom.wit");
    let wit = dom_wasm::wit::describe();
    if std::env::var_os("MACHOPO_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, wit).unwrap();
        return;
    }
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(
        written == wit,
        "wit/machopo-dom.wit isn't the table's: run MACHOPO_UPDATE_GOLDEN=1 cargo test -p dom_wasm --test it wit"
    );
}

/// `for_each_member!`'s plain members are `wit::is_plain`'s (the WIT's
/// are the frozen ones of them: `the_wit_is_the_frozen_members`): none of the interfaces whose objects aren't
/// nodes (`EventTarget`, `Event`, `Window`, `Location`, the collections and
/// lists), nor an element interface's.
#[test]
fn a_guest_gets_the_plain_members() {
    let mut rows: Vec<(&str, &str, bool)> = Vec::new();
    macro_rules! row {
        ($interface:ident, $member:ident, $kind:ident, $path:ty, $class:ident) => {
            rows.push((
                stringify!($interface),
                stringify!($member),
                stringify!($class) == "plain",
            ));
        };
    }
    dom_api::for_each_member!(row);
    let mut skipped = Vec::new();
    for (interface, member, plain) in rows {
        let member = member.trim_start_matches("r#");
        let desc = desc_of(interface);
        // An anonymous indexed getter's description has no name.
        let named = desc
            .members
            .iter()
            .any(|member_desc| member_desc.name == member);
        let wit = desc
            .members
            .iter()
            .filter(|member_desc| member_desc.name == if named { member } else { "" })
            .all(|member_desc| dom_wasm::wit::is_plain(desc, member_desc));
        assert_eq!(plain, wit, "{interface}.{member}");
        if !plain {
            skipped.push(format!("{interface}.{member}"));
        }
    }
    // Event handlers' IDL attributes are JS's alone for now: a guest has
    // no callbacks to set.
    skipped.retain(|member| {
        let name = member.split('.').nth(1).unwrap();
        dom::names::find(name)
            .and_then(dom::names::handler_attribute)
            .is_none()
    });
    // Whole interfaces whose objects aren't nodes, and element interfaces
    // (a guest's `this` is checked by its kind alone); of the other node
    // interfaces, the members giving an object that isn't a node (a
    // collection).
    let mut interfaces: Vec<&str> = skipped
        .iter()
        .map(|member| member.split('.').next().unwrap())
        .collect();
    interfaces.dedup();
    assert_eq!(
        interfaces,
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
            "HTMLIFrameElement",
            "HTMLImageElement",
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
            "SVGStyleElement",
            "SVGTextContentElement",
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
            "NamedNodeMap",
            "DOMParser",
            "AbstractRange",
            "StaticRange",
            "Range",
            "Selection",
            "NodeIterator",
            "TreeWalker",
            "Comment"
        ]
    );
    for interface in interfaces {
        let desc = desc_of(interface);
        if desc.node && desc.element.is_none() {
            let members: Vec<_> = skipped
                .iter()
                .filter(|member| member.split('.').next() == Some(interface))
                .collect();
            let expected: &[&str] = match interface {
                // A live list; a dictionary.
                "Node" => &["childNodes", "getRootNode"],
                // The constructor (the caller's realm's document), a static
                // operation (no `this`), an observable array and its
                // setting operation (whose sheet isn't a node), the
                // sheet list, collections, a sequence of nodes, the variadic
                // `(Node or DOMString)...` (`ParentNode`'s and
                // `ChildNode`'s), a range, the selection and the traversals
                // (no node's, and the filters are callbacks).
                "Document" => &[
                    "constructor",
                    "parseHTMLUnsafe",
                    "styleSheets",
                    "adoptedStyleSheets",
                    "setAdoptedStyleSheet",
                    "defaultView",
                    // Its getter and its setter (the attribute's whole).
                    "location",
                    "location",
                    "createRange",
                    "getSelection",
                    "timeline",
                    "getAnimations",
                    "createNodeIterator",
                    "createTreeWalker",
                    "implementation",
                    "forms",
                    "createEvent",
                    "getElementsByTagNameNS",
                    "getElementsByTagName",
                    "getElementsByClassName",
                    "children",
                    "prepend",
                    "append",
                    "replaceChildren",
                    "querySelectorAll",
                    "elementsFromPoint",
                    "getElementsByName",
                    "links",
                    "anchors",
                    "images",
                    "embeds",
                    "plugins",
                    "scripts",
                    "all",
                    // The document's own event handlers, no content
                    // attribute's (getter and setter each).
                    "onvisibilitychange",
                    "onvisibilitychange",
                    "onprerenderingchange",
                    "onprerenderingchange",
                ],
                "DocumentType" | "CharacterData" => &["before", "after", "replaceWith"],
                // The constructors (the caller's realm's document).
                "Text" | "Comment" => &["constructor"],
                // Whole: its kind, a fragment's, doesn't tell a shadow root.
                "ShadowRoot" => &[
                    "styleSheets",
                    "adoptedStyleSheets",
                    "setAdoptedStyleSheet",
                    "deleteAdoptedStyleSheet",
                    "mode",
                    "delegatesFocus",
                    "slotAssignment",
                    "clonable",
                    "serializable",
                    "host",
                    "activeElement",
                    "elementFromPoint",
                    "elementsFromPoint",
                    "getAnimations",
                    "innerHTML",
                    "innerHTML",
                    "setHTMLUnsafe",
                    "getHTML",
                ],
                // The constructor (the caller's realm's document).
                "DocumentFragment" => &[
                    "constructor",
                    "children",
                    "prepend",
                    "append",
                    "replaceChildren",
                    "querySelectorAll",
                ],
                // `classList`'s getter and setter, a sequence of strings and
                // the attributes' map.
                "Element" => &[
                    "attachShadow",
                    "shadowRoot",
                    "before",
                    "after",
                    "replaceWith",
                    "classList",
                    "classList",
                    "getAttributeNames",
                    "attributes",
                    "getHTML",
                    "getElementsByTagNameNS",
                    "getElementsByTagName",
                    "getElementsByClassName",
                    "children",
                    "prepend",
                    "append",
                    "replaceChildren",
                    "querySelectorAll",
                    "getClientRects",
                    "getBoundingClientRect",
                    // Promises, a dictionary or a boolean (`scrollIntoView`).
                    "scroll",
                    "scroll",
                    "scrollTo",
                    "scrollTo",
                    "scrollBy",
                    "scrollBy",
                    "scrollIntoView",
                    // Animations, keyframes (script objects).
                    "animate",
                    "getAnimations",
                ],
                // SVG's values (`SVGRect`, `SVGMatrix`, `SVGPoint`), and
                // a `DOMPointInit`.
                "SVGGraphicsElement" => &["getBBox", "getCTM", "getScreenCTM"],
                "SVGGeometryElement" => &["isPointInFill", "isPointInStroke", "getPointAtLength"],
                "SVGTextContentElement" => &[
                    "getStartPositionOfChar",
                    "getEndPositionOfChar",
                    "getExtentOfChar",
                    "getCharNumAtPosition",
                ],
                other => panic!("{other} has members giving objects"),
            };
            // In any order: the table's is the prototype's.
            let mut members: Vec<&String> = members;
            members.sort();
            let mut expected: Vec<String> = expected
                .iter()
                .map(|member| format!("{interface}.{member}"))
                .collect();
            expected.sort();
            assert_eq!(members, expected.iter().collect::<Vec<_>>(), "{interface}");
        }
    }
    let wit = dom_wasm::wit::describe();
    assert!(!wit.contains("event"), "no event in the WIT");
}

fn desc_of(name: &str) -> &'static dom_api::InterfaceDesc {
    dom_api::INTERFACES
        .iter()
        .find(|desc| desc.name == name)
        .unwrap()
}

/// The WIT's members are the frozen ones (`wit::FROZEN`), each plain
/// still: a member the table changes out of plain shows here, not as a
/// guest's missing import.
#[test]
fn the_wit_is_the_frozen_members() {
    let mut in_wit = Vec::new();
    for interface in dom_api::INTERFACES {
        for member in interface.members {
            if !matches!(member.kind, dom_api::MemberKind::Constant(_))
                && dom_wasm::wit::in_wit(interface, member)
            {
                in_wit.push(dom_wasm::wit::function_name(
                    interface.name,
                    member.name,
                    member.kind,
                ));
            }
        }
    }
    in_wit.sort();
    let mut frozen: Vec<String> = dom_wasm::wit::FROZEN
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    frozen.sort();
    assert_eq!(in_wit, frozen);
}
