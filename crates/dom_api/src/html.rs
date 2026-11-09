//! HTML's element interfaces' members
//! (https://html.spec.whatwg.org/#the-meta-element): attributes reflected
//! by their fixed name numbers, as `Element`'s `id` is.

use dom::{ContentEditable, NodeRef, QualName, names};

use crate::element::reflected;
use crate::exception::{DomException, ExceptionName};
use crate::typed::DomStr;
use crate::{ApiCx, ApiError};

/// `HTMLMetaElement`'s `name` and `content`.
pub(crate) fn name<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(reflected(cx, this, names::NAME))
}

pub(crate) fn set_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_attribute_ns(this, QualName::local("name"), value);
    Ok(())
}

pub(crate) fn content<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(reflected(cx, this, names::CONTENT))
}

pub(crate) fn set_content<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_attribute_ns(this, QualName::local("content"), value);
    Ok(())
}

/// `HTMLTemplateElement`'s `content`: its contents, a fragment of its
/// template contents owner document, made with it.
pub(crate) fn template_content<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx
        .template_content(this)
        .expect("a template has its contents"))
}

/// `HTMLTemplateElement`'s `shadowRootMode`: its `shadowrootmode`
/// attribute, limited to its known values (`open`, `closed`, ASCII case
/// insensitively; else the empty string).
pub(crate) fn shadow_root_mode<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    let value = cx.get_attribute(this, "shadowrootmode");
    let keyword = match value.as_ref().map(|value| value.to_str()) {
        Some(value) if value.eq_ignore_ascii_case("open") => "open",
        Some(value) if value.eq_ignore_ascii_case("closed") => "closed",
        _ => "",
    };
    Ok(DomStr::Owned(dom::DomString::new(keyword)))
}

pub(crate) fn set_shadow_root_mode<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_attribute_ns(this, QualName::local("shadowrootmode"), value);
    Ok(())
}

/// A boolean attribute reflected (HTML's): whether `this` has it; set: it
/// added (empty) or removed.
fn has<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>, name: &str) -> bool {
    cx.get_attribute(this, name).is_some()
}

fn set_flag<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>, name: &str, value: bool) {
    if value {
        cx.set_attribute_ns(this, QualName::local(name), "");
    } else {
        cx.remove_attribute(this, name);
    }
}

/// `HTMLTemplateElement`'s `shadowRootDelegatesFocus`,
/// `shadowRootClonable` and `shadowRootSerializable`.
pub(crate) fn shadow_root_delegates_focus<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<bool, ApiError> {
    Ok(has(cx, this, "shadowrootdelegatesfocus"))
}

pub(crate) fn set_shadow_root_delegates_focus<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: bool,
) -> Result<(), ApiError> {
    set_flag(cx, this, "shadowrootdelegatesfocus", value);
    Ok(())
}

pub(crate) fn shadow_root_clonable<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<bool, ApiError> {
    Ok(has(cx, this, "shadowrootclonable"))
}

pub(crate) fn set_shadow_root_clonable<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: bool,
) -> Result<(), ApiError> {
    set_flag(cx, this, "shadowrootclonable", value);
    Ok(())
}

pub(crate) fn shadow_root_serializable<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<bool, ApiError> {
    Ok(has(cx, this, "shadowrootserializable"))
}

pub(crate) fn set_shadow_root_serializable<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: bool,
) -> Result<(), ApiError> {
    set_flag(cx, this, "shadowrootserializable", value);
    Ok(())
}

/// `HTMLElement`'s `contentEditable`: its `contenteditable`'s state, as
/// its keyword.
pub(crate) fn content_editable<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    let state = cx
        .content_editable(this)
        .unwrap_or(ContentEditable::Inherit);
    Ok(DomStr::Owned(dom::DomString::new(state.keyword())))
}

/// Setting it: `inherit` removes the attribute; `true`, `false` and
/// `plaintext-only` set it, lowercased; anything else is a `SyntaxError`
/// (with Blink's message).
pub(crate) fn set_content_editable<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    let keyword = [
        ContentEditable::True,
        ContentEditable::False,
        ContentEditable::PlaintextOnly,
        ContentEditable::Inherit,
    ]
    .into_iter()
    .find(|state| value.eq_ignore_ascii_case(state.keyword()));
    match keyword {
        Some(ContentEditable::Inherit) => {
            cx.remove_attribute(this, "contenteditable");
        }
        Some(state) => {
            cx.set_attribute_ns(this, QualName::local("contenteditable"), state.keyword())
        }
        None => {
            return Err(ApiError::Dom(DomException {
                name: ExceptionName::Syntax,
                message: format!(
                    "The value provided ('{value}') is not one of 'true', 'false', \
                     'plaintext-only', or 'inherit'."
                ),
            }));
        }
    }
    Ok(())
}

/// `isContentEditable`: whether it is editable or an editing host, from
/// the DOM (as Chrome's): the same out of the document and in
/// `display: none`.
pub(crate) fn is_content_editable<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<bool, ApiError> {
    Ok(cx.is_editable(this))
}

/// What a `<link>`'s `as` may be (HTML's potential destinations, and
/// Chrome's `fetch`), its `as` limited to them.
const LINK_AS_KEYWORDS: &[&str] = &[
    "audio",
    "audioworklet",
    "document",
    "embed",
    "fetch",
    "font",
    "frame",
    "iframe",
    "image",
    "json",
    "manifest",
    "object",
    "paintworklet",
    "report",
    "script",
    "serviceworker",
    "sharedworker",
    "style",
    "text",
    "track",
    "video",
    "webidentity",
    "worker",
    "xslt",
];

/// `HTMLLinkElement`'s `as`: its keyword, the empty string for none or
/// any other.
pub(crate) fn link_as<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(crate::reflect::enumerated(
        cx,
        this,
        names::local::AS,
        LINK_AS_KEYWORDS,
        Some(""),
        "",
    ))
}

// --- `HTMLElement`'s global attributes.

/// The value of `name` on the nearest inclusive ancestor HTML element
/// whose value `state` maps to a state other than the inherited one
/// (`None`): how `translate` and `spellcheck` inherit.
fn inherited_state<'gc>(
    cx: &ApiCx<'_, 'gc>,
    node: NodeRef<'gc>,
    name: dom::NameId,
    state: impl Fn(&str) -> Option<bool>,
) -> Option<bool> {
    std::iter::successors(Some(node), |&at| cx.parent(at))
        .filter(|&at| {
            cx.qualified_name(at)
                .is_some_and(|name| name.namespace() == names::HTML_NAMESPACE)
        })
        .find_map(|at| {
            cx.attribute_value_by_id(at, name)
                .and_then(|value| state(&value.borrow().to_str()))
        })
}

/// `translate`: whether it is translate-enabled: the nearest HTML element
/// with the attribute in the yes (`""`, `yes`) or no state decides; the
/// root's is translate-enabled (Blink's `HTMLElement::translate`).
pub(crate) fn translate<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<bool, ApiError> {
    let state = |value: &str| match value {
        "" => Some(true),
        _ if value.eq_ignore_ascii_case("yes") => Some(true),
        _ if value.eq_ignore_ascii_case("no") => Some(false),
        _ => None,
    };
    Ok(inherited_state(cx, this, names::local::TRANSLATE, state).unwrap_or(true))
}

pub(crate) fn set_translate<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: bool,
) -> Result<(), ApiError> {
    crate::reflect::set_string::<{ names::local::TRANSLATE }>(
        cx,
        this,
        if value { "yes" } else { "no" },
    )
}

/// `spellcheck`: the nearest HTML element with the attribute in the true
/// (`""`, `true`) or false state decides; without one it is enabled,
/// Chrome's default (Blink's `HTMLElement::IsSpellCheckingEnabled`).
pub(crate) fn spellcheck<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<bool, ApiError> {
    let state = |value: &str| match value {
        "" => Some(true),
        _ if value.eq_ignore_ascii_case("true") => Some(true),
        _ if value.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    };
    Ok(inherited_state(cx, this, names::local::SPELLCHECK, state).unwrap_or(true))
}

pub(crate) fn set_spellcheck<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: bool,
) -> Result<(), ApiError> {
    crate::reflect::set_string::<{ names::local::SPELLCHECK }>(
        cx,
        this,
        if value { "true" } else { "false" },
    )
}

/// `draggable`: the attribute's true or false state (any case), else its
/// auto state's default: true for an `<img>` and an `<a>` with an `href`
/// (Blink's `HTMLImageElement::draggable`, `HTMLAnchorElement::draggable`).
pub(crate) fn draggable<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<bool, ApiError> {
    let value = cx
        .attribute_value_by_id(this, names::local::DRAGGABLE)
        .map(|value| value.borrow().to_str().into_owned());
    Ok(match value.as_deref() {
        Some(value) if value.eq_ignore_ascii_case("true") => true,
        Some(value) if value.eq_ignore_ascii_case("false") => false,
        _ => {
            let name = cx.qualified_name(this);
            name == Some(dom::QualifiedName::html(names::local::IMG))
                || (name == Some(dom::QualifiedName::html(names::local::A))
                    && cx.attribute_value_by_id(this, names::local::HREF).is_some())
        }
    })
}

pub(crate) fn set_draggable<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: bool,
) -> Result<(), ApiError> {
    crate::reflect::set_string::<{ names::local::DRAGGABLE }>(
        cx,
        this,
        if value { "true" } else { "false" },
    )
}

/// `hidden`: `"until-found"` in the hidden until found state, else
/// whether the attribute is there.
pub(crate) fn hidden<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<crate::typed::HiddenState, ApiError> {
    Ok(match cx.attribute_value_by_id(this, names::local::HIDDEN) {
        Some(value) if value.borrow().to_str().eq_ignore_ascii_case("until-found") => {
            crate::typed::HiddenState::UntilFound
        }
        value => crate::typed::HiddenState::Hidden(value.is_some()),
    })
}

/// Setting `hidden`: `"until-found"` (any case) is that state; `false`,
/// `null`, the empty string, 0 and NaN remove the attribute; anything
/// else makes it empty.
pub(crate) fn set_hidden<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: Option<crate::typed::HiddenValue<&str>>,
) -> Result<(), ApiError> {
    use crate::typed::HiddenValue;
    let value = match value {
        None | Some(HiddenValue::Bool(false)) => None,
        Some(HiddenValue::Bool(true)) => Some(""),
        Some(HiddenValue::Number(number)) => (number != 0.0 && !number.is_nan()).then_some(""),
        Some(HiddenValue::String("")) => None,
        Some(HiddenValue::String(text)) if text.eq_ignore_ascii_case("until-found") => {
            Some("until-found")
        }
        Some(HiddenValue::String(_)) => Some(""),
    };
    match value {
        Some(value) => crate::reflect::set_string::<{ names::local::HIDDEN }>(cx, this, value),
        None => crate::reflect::set_boolean::<{ names::local::HIDDEN }>(cx, this, false),
    }
}

/// `dir`, `inputMode` and `enterKeyHint`: limited to their keywords.
pub(crate) fn dir<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(crate::reflect::enumerated(
        cx,
        this,
        names::local::DIR,
        &["ltr", "rtl", "auto"],
        None,
        "",
    ))
}

pub(crate) fn input_mode<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(crate::reflect::enumerated(
        cx,
        this,
        names::local::INPUTMODE,
        &[
            "none", "text", "tel", "url", "email", "numeric", "decimal", "search",
        ],
        None,
        "",
    ))
}

/// A global attribute whose name isn't one of the fixed ones, by its
/// name: its value.
fn unfixed_attribute<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>, name: &str) -> Option<String> {
    cx.get_attribute(this, name)
        .map(|value| value.to_str().into_owned())
}

pub(crate) fn enter_key_hint<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    const KEYWORDS: [&str; 7] = ["enter", "done", "go", "next", "previous", "search", "send"];
    let keyword = unfixed_attribute(cx, this, "enterkeyhint")
        .and_then(|value| {
            KEYWORDS
                .iter()
                .find(|keyword| keyword.eq_ignore_ascii_case(&value))
                .copied()
        })
        .unwrap_or("");
    Ok(DomStr::Owned(dom::DomString::new(keyword)))
}

pub(crate) fn set_enter_key_hint<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_attribute_ns(this, QualName::local("enterkeyhint"), value);
    Ok(())
}

/// The elements whose autocapitalization hint is their form owner's when
/// they have none (HTML's autocapitalize-and-autocorrect inheriting
/// elements).
const AUTOCAPITALIZE_INHERITING: [dom::NameId; 6] = [
    names::local::BUTTON,
    names::local::FIELDSET,
    names::local::INPUT,
    names::local::OUTPUT,
    names::local::SELECT,
    names::local::TEXTAREA,
];

/// HTML's own autocapitalization hint, as `autocapitalize` gives it: `""`
/// for the default state; an invalid value `"sentences"`, as in Chrome and
/// WPT.
fn own_autocapitalization_hint<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> &'static str {
    match unfixed_attribute(cx, element, "autocapitalize").as_deref() {
        None | Some("") => {
            let inherits = cx.qualified_name(element).is_some_and(|name| {
                name.namespace() == names::HTML_NAMESPACE
                    && AUTOCAPITALIZE_INHERITING.contains(&name.local())
            });
            match cx.form_owner(element) {
                Some(form) if inherits => own_autocapitalization_hint(cx, form),
                _ => "",
            }
        }
        Some(value) if value.eq_ignore_ascii_case("none") || value.eq_ignore_ascii_case("off") => {
            "none"
        }
        Some(value) if value.eq_ignore_ascii_case("characters") => "characters",
        Some(value) if value.eq_ignore_ascii_case("words") => "words",
        Some(_) => "sentences",
    }
}

pub(crate) fn autocapitalize<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(dom::DomString::new(
        own_autocapitalization_hint(cx, this),
    )))
}

pub(crate) fn set_autocapitalize<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_attribute_ns(this, QualName::local("autocapitalize"), value);
    Ok(())
}
