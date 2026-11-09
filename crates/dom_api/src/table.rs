//! HTML's tables (https://html.spec.whatwg.org/#tables; Blink's
//! `HTMLTableElement`, `HTMLTableSectionElement`, `HTMLTableRowElement`,
//! `HTMLTableCellElement`): a table's caption,
//! sections and rows and the collections of them, made and removed;
//! `<map>`'s `areas`.

use dom::names::local;
use dom::{KeptCollection, NameId, NodeRef, QualName, QualifiedName, names};
use elise::Gc;

use crate::typed::DomStr;
use crate::{ApiCx, ApiError, DomException, ExceptionName};

fn is<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>, name: NameId) -> bool {
    cx.qualified_name(node) == Some(QualifiedName::html(name))
}

fn is_row<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> bool {
    is(cx, node, local::TR)
}

fn is_cell<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> bool {
    is(cx, node, local::TD) || is(cx, node, local::TH)
}

fn is_section<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> bool {
    is(cx, node, local::THEAD) || is(cx, node, local::TBODY) || is(cx, node, local::TFOOT)
}

fn children<'a, 'gc>(
    cx: &'a ApiCx<'_, 'gc>,
    node: NodeRef<'gc>,
) -> impl Iterator<Item = NodeRef<'gc>> + 'a {
    std::iter::successors(cx.first_child(node), |&child| cx.next_sibling(child))
}

fn children_backwards<'a, 'gc>(
    cx: &'a ApiCx<'_, 'gc>,
    node: NodeRef<'gc>,
) -> impl Iterator<Item = NodeRef<'gc>> + 'a {
    std::iter::successors(cx.last_child(node), |&child| cx.previous_sibling(child))
}

/// A table's rows, in `rows`' order: its `thead`s', its own and its
/// `tbody`s', its `tfoot`s'.
fn rows<'gc>(cx: &ApiCx<'_, 'gc>, table: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
    let collection = cx.kept_collection(table, KeptCollection::TableRows);
    let length = cx.collection_length(collection);
    (0..length)
        .filter_map(|index| cx.collection_item(collection, index))
        .collect()
}

/// The table's last row (`insertRow(-1)`'s): its last `tfoot`'s, else its
/// last own row or last `tbody`'s, else its last `thead`'s.
fn last_row<'gc>(cx: &ApiCx<'_, 'gc>, table: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
    let last_of =
        |section: NodeRef<'gc>| children_backwards(cx, section).find(|&row| is_row(cx, row));
    children_backwards(cx, table)
        .filter(|&child| is(cx, child, local::TFOOT))
        .find_map(last_of)
        .or_else(|| {
            children_backwards(cx, table).find_map(|child| {
                if is_row(cx, child) {
                    Some(child)
                } else if is(cx, child, local::TBODY) {
                    last_of(child)
                } else {
                    None
                }
            })
        })
        .or_else(|| {
            children_backwards(cx, table)
                .filter(|&child| is(cx, child, local::THEAD))
                .find_map(last_of)
        })
}

fn index_size_error(message: &str) -> ApiError {
    ApiError::Dom(DomException {
        name: ExceptionName::IndexSize,
        message: message.to_owned(),
    })
}

fn create<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>, name: &str) -> NodeRef<'gc> {
    cx.create_element(cx.document_of(node), QualName::html(name))
}

fn first_child<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>, name: NameId) -> Option<NodeRef<'gc>> {
    children(cx, node).find(|&child| is(cx, child, name))
}

fn delete_first<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>, name: NameId) {
    if let Some(child) = first_child(cx, node, name) {
        cx.remove(child);
    }
}

fn insert<'gc>(
    cx: &ApiCx<'_, 'gc>,
    parent: NodeRef<'gc>,
    node: NodeRef<'gc>,
    before: Option<NodeRef<'gc>>,
) -> Result<NodeRef<'gc>, ApiError> {
    cx.pre_insert(parent, node, before).map_err(ApiError::from)
}

/// Where a `thead` goes: before the first element child that isn't a
/// `caption` or a `colgroup`.
fn head_position<'gc>(cx: &ApiCx<'_, 'gc>, table: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
    children(cx, table).find(|&child| {
        cx.kind(child) == dom::NodeKind::Element
            && !is(cx, child, local::CAPTION)
            && !is(cx, child, local::COLGROUP)
    })
}

pub(crate) fn caption<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(first_child(cx, this, local::CAPTION))
}

/// Setting `caption`: the first one removed, the new one first.
pub(crate) fn set_caption<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    caption: Option<NodeRef<'gc>>,
) -> Result<(), ApiError> {
    delete_first(cx, this, local::CAPTION);
    if let Some(caption) = caption {
        insert(cx, this, caption, cx.first_child(this))?;
    }
    Ok(())
}

pub(crate) fn create_caption<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<NodeRef<'gc>, ApiError> {
    if let Some(caption) = first_child(cx, this, local::CAPTION) {
        return Ok(caption);
    }
    let caption = create(cx, this, "caption");
    insert(cx, this, caption, cx.first_child(this))
}

pub(crate) fn delete_caption<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<(), ApiError> {
    delete_first(cx, this, local::CAPTION);
    Ok(())
}

pub(crate) fn t_head<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(first_child(cx, this, local::THEAD))
}

pub(crate) fn set_t_head<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    head: Option<NodeRef<'gc>>,
) -> Result<(), ApiError> {
    set_section(cx, this, head, local::THEAD)
}

pub(crate) fn t_foot<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(first_child(cx, this, local::TFOOT))
}

pub(crate) fn set_t_foot<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    foot: Option<NodeRef<'gc>>,
) -> Result<(), ApiError> {
    set_section(cx, this, foot, local::TFOOT)
}

/// Setting `tHead` or `tFoot`: not a section of that name a
/// `HierarchyRequestError`; the first one removed, the new one where it
/// goes (a `thead` before the first other element child, a `tfoot` last).
fn set_section<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    section: Option<NodeRef<'gc>>,
    name: NameId,
) -> Result<(), ApiError> {
    if let Some(section) = section
        && !is(cx, section, name)
    {
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::HierarchyRequest,
            message: format!("not a <{}>", names::get(name)),
        }));
    }
    delete_first(cx, this, name);
    if let Some(section) = section {
        let before = if name == local::THEAD {
            head_position(cx, this)
        } else {
            None
        };
        insert(cx, this, section, before)?;
    }
    Ok(())
}

pub(crate) fn create_t_head<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<NodeRef<'gc>, ApiError> {
    if let Some(head) = first_child(cx, this, local::THEAD) {
        return Ok(head);
    }
    let head = create(cx, this, "thead");
    insert(cx, this, head, head_position(cx, this))
}

pub(crate) fn delete_t_head<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<(), ApiError> {
    delete_first(cx, this, local::THEAD);
    Ok(())
}

pub(crate) fn create_t_foot<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<NodeRef<'gc>, ApiError> {
    if let Some(foot) = first_child(cx, this, local::TFOOT) {
        return Ok(foot);
    }
    let foot = create(cx, this, "tfoot");
    insert(cx, this, foot, None)
}

pub(crate) fn delete_t_foot<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<(), ApiError> {
    delete_first(cx, this, local::TFOOT);
    Ok(())
}

pub(crate) fn t_bodies<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, dom::HtmlCollection>, ApiError> {
    Ok(cx.kept_collection(this, KeptCollection::TableBodies))
}

/// `createTBody()`: a new `tbody` after the last one (else last).
pub(crate) fn create_t_body<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<NodeRef<'gc>, ApiError> {
    let body = create(cx, this, "tbody");
    let before = children_backwards(cx, this)
        .find(|&child| is(cx, child, local::TBODY))
        .and_then(|last| cx.next_sibling(last));
    insert(cx, this, body, before)
}

pub(crate) fn table_rows<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, dom::HtmlCollection>, ApiError> {
    Ok(cx.kept_collection(this, KeptCollection::TableRows))
}

/// `insertRow(index)`: below -1, or past the rows, an `IndexSizeError`;
/// -1 or the number of rows, after the last row (in its parent), or in
/// the last `tbody` (one made, last, if there is none) without rows;
/// else before the row at `index`.
pub(crate) fn insert_row<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    index: i32,
) -> Result<NodeRef<'gc>, ApiError> {
    if index < -1 {
        return Err(index_size_error("the index is below -1"));
    }
    let rows = rows(cx, this);
    let (before, last) = if index == -1 {
        (None, last_row(cx, this))
    } else {
        let index = usize::try_from(index).unwrap_or(usize::MAX);
        match rows.get(index) {
            Some(&before) => (Some(before), None),
            None if index == rows.len() => (None, rows.last().copied()),
            None => return Err(index_size_error("the index is past the number of rows")),
        }
    };
    let row = create(cx, this, "tr");
    match (before, last) {
        (Some(before), _) => {
            let parent = cx.parent(before).expect("a row has its parent");
            insert(cx, parent, row, Some(before))
        }
        (None, Some(last)) => {
            let parent = cx.parent(last).expect("a row has its parent");
            insert(cx, parent, row, None)
        }
        (None, None) => {
            match children_backwards(cx, this).find(|&child| is(cx, child, local::TBODY)) {
                Some(body) => insert(cx, body, row, None),
                None => {
                    let body = create(cx, this, "tbody");
                    insert(cx, body, row, None)?;
                    insert(cx, this, body, None)?;
                    Ok(row)
                }
            }
        }
    }
}

/// `deleteRow(index)`: -1 the last row (none: nothing); else the row at
/// `index`, an `IndexSizeError` without one.
pub(crate) fn delete_row<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    index: i32,
) -> Result<(), ApiError> {
    let row = if index == -1 {
        last_row(cx, this)
    } else {
        usize::try_from(index)
            .ok()
            .and_then(|index| rows(cx, this).get(index).copied())
    };
    match row {
        Some(row) => {
            cx.remove(row);
            Ok(())
        }
        None if index == -1 => Ok(()),
        None => Err(index_size_error("no row at that index")),
    }
}

pub(crate) fn section_rows<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, dom::HtmlCollection>, ApiError> {
    Ok(cx.kept_collection(this, KeptCollection::SectionRows))
}

pub(crate) fn cells<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, dom::HtmlCollection>, ApiError> {
    Ok(cx.kept_collection(this, KeptCollection::RowCells))
}

/// A section's `insertRow` and a row's `insertCell`: a new child of `name`
/// before the `index`th one `picks` picks (-1, or their number: last);
/// below -1 or past them an `IndexSizeError`.
fn insert_child<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    index: i32,
    picks: fn(&ApiCx<'_, 'gc>, NodeRef<'gc>) -> bool,
    name: &str,
) -> Result<NodeRef<'gc>, ApiError> {
    if index < -1 {
        return Err(index_size_error("the index is below -1"));
    }
    let picked: Vec<NodeRef<'gc>> = children(cx, this)
        .filter(|&child| picks(cx, child))
        .collect();
    let before = match usize::try_from(index) {
        Err(_) => None,
        Ok(index) => match picked.get(index) {
            Some(&before) => Some(before),
            None if index == picked.len() => None,
            None => return Err(index_size_error("the index is past the number of children")),
        },
    };
    let child = create(cx, this, name);
    insert(cx, this, child, before)
}

/// A section's `deleteRow` and a row's `deleteCell`.
fn delete_child<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    index: i32,
    picks: fn(&ApiCx<'_, 'gc>, NodeRef<'gc>) -> bool,
) -> Result<(), ApiError> {
    let child = if index == -1 {
        children_backwards(cx, this).find(|&child| picks(cx, child))
    } else {
        usize::try_from(index).ok().and_then(|index| {
            children(cx, this)
                .filter(|&child| picks(cx, child))
                .nth(index)
        })
    };
    match child {
        Some(child) => {
            cx.remove(child);
            Ok(())
        }
        None if index == -1 => Ok(()),
        None => Err(index_size_error("no child at that index")),
    }
}

pub(crate) fn section_insert_row<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    index: i32,
) -> Result<NodeRef<'gc>, ApiError> {
    insert_child(cx, this, index, is_row, "tr")
}

pub(crate) fn section_delete_row<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    index: i32,
) -> Result<(), ApiError> {
    delete_child(cx, this, index, is_row)
}

pub(crate) fn insert_cell<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    index: i32,
) -> Result<NodeRef<'gc>, ApiError> {
    insert_child(cx, this, index, is_cell, "td")
}

pub(crate) fn delete_cell<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    index: i32,
) -> Result<(), ApiError> {
    delete_child(cx, this, index, is_cell)
}

fn position<'gc>(found: impl IntoIterator<Item = NodeRef<'gc>>, node: NodeRef<'gc>) -> i32 {
    found
        .into_iter()
        .position(|at| Gc::ptr_eq(at, node))
        .map_or(-1, |index| i32::try_from(index).unwrap_or(i32::MAX))
}

/// `rowIndex`: its place in its table's `rows` (its parent's, or its
/// section's parent's); -1 in none.
pub(crate) fn row_index<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<i32, ApiError> {
    let table = cx.parent(this).and_then(|parent| {
        if is(cx, parent, local::TABLE) {
            Some(parent)
        } else if is_section(cx, parent) {
            cx.parent(parent).filter(|&up| is(cx, up, local::TABLE))
        } else {
            None
        }
    });
    Ok(table.map_or(-1, |table| position(rows(cx, table), this)))
}

/// `sectionRowIndex`: its place among its parent's rows (a table's
/// `rows`, a section's own); -1 in neither.
pub(crate) fn section_row_index<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<i32, ApiError> {
    Ok(match cx.parent(this) {
        Some(parent) if is(cx, parent, local::TABLE) => position(rows(cx, parent), this),
        Some(parent) if is_section(cx, parent) => position(
            children(cx, parent).filter(|&child| is_row(cx, child)),
            this,
        ),
        _ => -1,
    })
}

/// `cellIndex`: its place among its row's cells; -1 out of a row.
pub(crate) fn cell_index<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<i32, ApiError> {
    Ok(match cx.parent(this) {
        Some(row) if is_row(cx, row) => {
            position(children(cx, row).filter(|&child| is_cell(cx, child)), this)
        }
        _ => -1,
    })
}

/// A cell's `scope`: limited to its keywords, the empty string otherwise.
pub(crate) fn scope<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(crate::reflect::enumerated(
        cx,
        this,
        local::SCOPE,
        &["row", "col", "rowgroup", "colgroup"],
        None,
        "",
    ))
}

/// `<map>`'s `areas`.
pub(crate) fn map_areas<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, dom::HtmlCollection>, ApiError> {
    Ok(cx.kept_collection(this, KeptCollection::MapAreas))
}
