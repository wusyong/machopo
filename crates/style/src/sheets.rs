//! Which of the DOM's sheets apply (`dom::Cx::owns_sheet` says
//! which elements have one): one with a title applies if it is of the
//! preferred set, an alternate `<link>` only then.

use dom::{Cx, NodeRef};

/// Whether `owner`'s sheet is of the preferred set (`preferred`, set by
/// the first title that isn't an alternate `<link>`'s): one without a
/// title is, but an alternate `<link>`.
pub(crate) fn of_preferred_set<'gc>(
    cx: &Cx<'_, 'gc>,
    owner: NodeRef<'gc>,
    preferred: &mut Option<String>,
) -> bool {
    let title = cx
        .get_attribute(owner, "title")
        .map(|title| title.to_str().into_owned())
        .filter(|title| !title.is_empty());
    let alternate = cx.is_alternate_stylesheet(owner);
    match title {
        Some(title) => {
            if preferred.is_none() && !alternate {
                *preferred = Some(title.clone());
            }
            preferred.as_deref() == Some(title.as_str())
        }
        None => !alternate,
    }
}
