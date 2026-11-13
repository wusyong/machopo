//! Shadow trees' members (https://dom.spec.whatwg.org/#shadow-trees):
//! `Element`'s `attachShadow` and `shadowRoot`, `ShadowRoot`'s, `Node`'s
//! `getRootNode` (Blink's `Element::attachShadow`, `ShadowRoot`). The
//! trees are `dom`'s (`attach_shadow`, `retarget`).

use dom::{NodeRef, ShadowHostError, ShadowRootInit, ShadowRootMode, SlotAssignmentMode};

use crate::exception::{DomException, ExceptionName};
use crate::interfaces::{GetRootNodeOptions, ShadowRootInit as ShadowRootInitDict};
use crate::typed::DomStr;
use crate::{ApiCx, ApiError};

fn not_supported(message: &str) -> ApiError {
    ApiError::Dom(DomException {
        name: ExceptionName::NotSupported,
        message: message.to_owned(),
    })
}

/// `attachShadow(init)`: DOM's "attach a shadow root" to an HTML element
/// of a valid custom element name or one of the list (`span` too), with
/// none yet (else a `NotSupportedError`); `init`'s enums as strings, a
/// bad one a `TypeError`. No custom element definitions yet to disable
/// it, no declarative roots yet to reuse.
pub(crate) fn attach_shadow<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    init: &ShadowRootInitDict,
) -> Result<NodeRef<'gc>, ApiError> {
    let mode = match init.mode.as_str() {
        "open" => ShadowRootMode::Open,
        "closed" => ShadowRootMode::Closed,
        "" => {
            return Err(ApiError::type_error(
                "Failed to read the 'mode' property from 'ShadowRootInit': Required member is undefined.",
            ));
        }
        other => {
            return Err(ApiError::type_error(format!(
                "The provided value '{other}' is not a valid enum value of type ShadowRootMode."
            )));
        }
    };
    let slot_assignment = match init.slotAssignment.as_str() {
        "named" => SlotAssignmentMode::Named,
        "manual" => SlotAssignmentMode::Manual,
        other => {
            return Err(ApiError::type_error(format!(
                "The provided value '{other}' is not a valid enum value of type SlotAssignmentMode."
            )));
        }
    };
    match cx.shadow_host_check(this) {
        Ok(()) => {}
        Err(ShadowHostError::NotAHost) => {
            return Err(not_supported("This element does not support attachShadow"));
        }
        Err(ShadowHostError::Disabled) => {
            return Err(not_supported(
                "attachShadow() is disabled by disabledFeatures static field.",
            ));
        }
    }
    // A declarative shadow root of the same mode is taken over: emptied,
    // not declarative any more.
    if let Some(root) = cx.shadow_root(this) {
        let same_mode = cx
            .shadow_root_init(root)
            .is_some_and(|init| init.mode == mode);
        if !cx.is_declarative_shadow_root(root) || !same_mode {
            return Err(not_supported(
                "Shadow root cannot be created on a host which already hosts a shadow tree.",
            ));
        }
        while let Some(child) = cx.first_child(root) {
            cx.pre_remove(root, child).expect("a child of its parent");
        }
        cx.set_shadow_root_declarative(root, false);
        return Ok(root);
    }
    Ok(cx.attach_shadow(
        this,
        ShadowRootInit {
            mode,
            delegates_focus: init.delegatesFocus,
            slot_assignment,
            clonable: init.clonable,
            serializable: init.serializable,
        },
    ))
}

/// `shadowRoot`: an open one; none for a closed one (or a user agent's).
pub(crate) fn shadow_root<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.shadow_root(this).filter(|&root| {
        cx.shadow_root_init(root)
            .is_some_and(|init| init.mode == ShadowRootMode::Open)
    }))
}

/// `getRootNode(options)`: its root, or its shadow-including root with
/// `composed`.
pub(crate) fn get_root_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    options: &GetRootNodeOptions,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.root_node(this, options.composed))
}

/// A shadow root's settings, which its `this` is (the front end's checks).
fn init<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> ShadowRootInit {
    cx.shadow_root_init(this).expect("`this` is a shadow root")
}

pub(crate) fn mode<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    let mode = match init(cx, this).mode {
        ShadowRootMode::Open => "open",
        ShadowRootMode::Closed | ShadowRootMode::UserAgent => "closed",
    };
    Ok(DomStr::fresh(mode))
}

pub(crate) fn delegates_focus<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<bool, ApiError> {
    Ok(init(cx, this).delegates_focus)
}

pub(crate) fn slot_assignment<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    let mode = match init(cx, this).slot_assignment {
        SlotAssignmentMode::Named => "named",
        SlotAssignmentMode::Manual => "manual",
    };
    Ok(DomStr::fresh(mode))
}

pub(crate) fn clonable<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<bool, ApiError> {
    Ok(init(cx, this).clonable)
}

pub(crate) fn serializable<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<bool, ApiError> {
    Ok(init(cx, this).serializable)
}

pub(crate) fn host<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.shadow_host(this).expect("a shadow root has its host"))
}
