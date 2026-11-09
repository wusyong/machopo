//! `DocumentType` (https://dom.spec.whatwg.org/#interface-documenttype) and
//! `ProcessingInstruction`'s `target`
//! (https://dom.spec.whatwg.org/#interface-processinginstruction).

use dom::NodeRef;

use crate::typed::DomStr;
use crate::{ApiCx, ApiError};

/// A doctype's `name`, a processing instruction's `target`: its node name,
/// one of the DOM's names (no string made).
pub(crate) fn name<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Name(cx.node_name_id(this)))
}

pub(crate) fn public_id<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(
        cx.doctype(this).expect("a doctype").public_id,
    ))
}

pub(crate) fn system_id<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(
        cx.doctype(this).expect("a doctype").system_id,
    ))
}
