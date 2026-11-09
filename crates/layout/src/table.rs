//! Tables (css-tables 3; Blink's `LayoutTable` and its parts, laid out by
//! `TableLayoutAlgorithm` with `table_layout_utils`): the fixup that makes
//! the anonymous table boxes, and the table formatting context.
//!
//! The box tree holds a table as a tree, as Blink's does: the table's
//! children are its captions and row groups, a row group's are its rows,
//! a row's are its cells (each part's out-of-flow children are among
//! them). There is no table wrapper box: a caption is the table's child,
//! above or below the grid, inside the table's border box but outside its
//! border and padding (Blink's).
//!
//! The table is a leaf to taffy, as an inline formatting context is: its
//! layout sizes the columns from the cells' min-content and max-content
//! widths, lays each cell out at its column's width (taffy lays a cell out
//! as any block container), sizes the rows from the cells, and places the
//! row groups, rows and cells itself. Where css-tables 3 leaves things
//! open, it does what Chrome (Blink's LayoutNG) does, as the comments say.

use std::sync::Arc;

use computed_style::{
    BorderCollapse, BorderStyle, BoxSizing, CaptionSide, ComputedStyle, Display, LengthPercentage,
    LengthPercentageOrAuto, LengthPercentageOrNone, Size, TableDisplay, TableLayout, VerticalAlign,
    Visibility,
};
use dom::{Cx, NodeRef, QualifiedName};
use gfx::{PointF, RectF};
use taffy::{
    AvailableSpace, AxisStaticPosition, LayoutInput, LayoutOutput, LayoutPartialTree, OofCandidate,
    OofCandidates, OofPositioningArea, RequestedAxis, ResolveOrZero, RunMode, SizingMode,
    compute_leaf_layout, compute_oof_layout,
};

use crate::text::{AtomicInlines, Edge, InlineItem, relative_offset};
use crate::{
    BoxRole, BoxTree, Child, CollapsedBorder, CollapsedBorders, ContentContext, LayoutBox,
    TableColumn, border_rect, content_box_inset,
};

/// The boxes of a table, by their `display`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TablePart {
    /// `table` and `inline-table`.
    Table,
    /// `table-row-group`, `table-header-group` and `table-footer-group`.
    RowGroup,
    Row,
    Cell,
    Caption,
    /// `table-column-group` and `table-column`: boxes only in a table (a
    /// column in a column group or a table), with no fragments (Blink's
    /// `LayoutTableColumn`): the table reads their widths, backgrounds and
    /// `visibility`.
    ColumnGroup,
    Column,
}

impl TablePart {
    pub(crate) fn of(display: Display) -> Option<Self> {
        Some(match display {
            Display::Table(TableDisplay::Table) | Display::InlineTable => Self::Table,
            Display::Table(
                TableDisplay::RowGroup | TableDisplay::HeaderGroup | TableDisplay::FooterGroup,
            ) => Self::RowGroup,
            Display::Table(TableDisplay::Row) => Self::Row,
            Display::Table(TableDisplay::Cell) => Self::Cell,
            Display::Table(TableDisplay::Caption) => Self::Caption,
            Display::Table(TableDisplay::ColumnGroup) => Self::ColumnGroup,
            Display::Table(TableDisplay::Column) => Self::Column,
            _ => return None,
        })
    }
}

/// The most columns a cell spans, and rows (HTML's "processing model" of
/// tables; Blink's `kMaxColSpan` and `kMaxRowSpan`).
const MAX_COLSPAN: u32 = 1000;
const MAX_ROWSPAN: u32 = 65534;

/// `element`'s span: a `<td>`'s or `<th>`'s `colspan` (1 if it isn't a
/// positive integer) and `rowspan` (1 if it isn't a non-negative integer;
/// 0 goes to the end of the row group); a `<col>`'s or `<colgroup>`'s
/// `span` (1 if it isn't a positive integer), and 1; others', 1 by 1.
pub(crate) fn span<'gc>(cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> (u32, u32) {
    use dom::names::local;
    let html = |name| cx.qualified_name(element) == Some(QualifiedName::html(name));
    let attribute = |name| {
        cx.attribute_value_by_id(element, name)
            .and_then(|value| parse_non_negative_integer(&value.borrow().to_str()))
    };
    if html(local::COL) || html(local::COLGROUP) {
        let span = match attribute(local::SPAN) {
            Some(0) | None => 1,
            Some(n) => n.min(MAX_COLSPAN),
        };
        return (span, 1);
    }
    if !(html(local::TD) || html(local::TH)) {
        return (1, 1);
    }
    let colspan = match attribute(local::COLSPAN) {
        Some(0) | None => 1,
        Some(n) => n.min(MAX_COLSPAN),
    };
    let rowspan = attribute(local::ROWSPAN).map_or(1, |n| n.min(MAX_ROWSPAN));
    (colspan, rowspan)
}

/// HTML's "rules for parsing non-negative integers": white space, an
/// optional `+`, then the digits up to the first that isn't one.
fn parse_non_negative_integer(value: &str) -> Option<u32> {
    let value = value.trim_start_matches(is_html_space);
    let value = value.strip_prefix('+').unwrap_or(value);
    let digits = value.bytes().take_while(u8::is_ascii_digit);
    let mut any = false;
    let mut n: u64 = 0;
    for digit in digits {
        any = true;
        // Held at u32::MAX each step, so the u64 never saturates: exact.
        n = n
            .saturating_mul(10)
            .saturating_add(u64::from(digit.saturating_sub(b'0')))
            .min(u64::from(u32::MAX));
    }
    any.then_some(u32::try_from(n).unwrap_or(u32::MAX))
}

/// Whether `child` is a text node of only white space: between table
/// parts, it makes nothing, whatever `white-space` says (css-tables 3,
/// "Remove irrelevant boxes"; Blink keeps no such text in a table, row
/// group or row); with other content, it is content.
fn is_space_text(child: &Child) -> bool {
    matches!(child, Child::Inline(InlineItem::Text(text, _, _, Some(_)))
        if text.chars().all(is_html_space))
}

fn is_html_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

/// An anonymous table box's style: `display`, inheriting from `parent`
/// (Blink's `ComputedStyle::CreateAnonymousStyleWithDisplay`).
fn anonymous_style(parent: &ComputedStyle, display: Display) -> Arc<ComputedStyle> {
    Arc::new(ComputedStyle {
        display,
        original_display: display,
        ..ComputedStyle::inherit(parent)
    })
}

impl BoxTree {
    /// Gives table, row group or row box `index` its content, fixed up
    /// (css-tables 3, "Fixup"; Blink's `AddChild` of `LayoutTable`,
    /// `LayoutTableSection` and `LayoutTableRow`): its proper children
    /// (a table's captions and row groups, a row group's rows, a row's
    /// cells) as they are, and each run of other content between them
    /// wrapped in an anonymous row group, row or cell, which goes on down
    /// to a cell. An out-of-flow box goes with the run it is in, if one is
    /// open; otherwise it is a child of its own. Text nodes of only white
    /// space directly in it make nothing unless other content is with them
    /// ([`is_space_text`]).
    pub(crate) fn set_table_content(
        &mut self,
        cx: &mut ContentContext<'_>,
        index: usize,
        content: Vec<Child>,
    ) {
        let part = TablePart::of(self.boxes[index].style.display).expect("a table box");
        let proper = |tree: &Self, id: taffy::NodeId| {
            let child = TablePart::of(tree.boxes[usize::from(id)].style.display);
            match part {
                TablePart::Table => matches!(
                    child,
                    Some(
                        TablePart::RowGroup
                            | TablePart::Caption
                            | TablePart::ColumnGroup
                            | TablePart::Column
                    )
                ),
                TablePart::RowGroup => child == Some(TablePart::Row),
                TablePart::Row => child == Some(TablePart::Cell),
                TablePart::Cell
                | TablePart::Caption
                | TablePart::ColumnGroup
                | TablePart::Column => unreachable!("not a table box"),
            }
        };
        let mut children = Vec::new();
        let mut run = Vec::new();
        // How many inline elements are open: a part in one isn't this box's.
        let mut depth = 0usize;
        for child in content {
            match &child {
                Child::Block(id) if depth == 0 && proper(self, *id) => {
                    self.wrap_table_run(cx, index, &mut run, &mut children);
                    children.push(*id);
                }
                Child::Inline(InlineItem::OutOfFlow(id, _)) if run.iter().all(is_space_text) => {
                    children.push(*id);
                }
                _ => {
                    match &child {
                        Child::Inline(InlineItem::Open(..)) => depth = depth.saturating_add(1),
                        Child::Inline(InlineItem::Close(..)) => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                    run.push(child);
                }
            }
        }
        self.wrap_table_run(cx, index, &mut run, &mut children);
        for &child in &children {
            self.boxes[usize::from(child)].parent = Some(index);
        }
        self.forget_texts(index);
        let b = &mut self.boxes[index];
        b.inline = None;
        b.children = children;
    }

    /// Wraps `run`, content of table box `parent` that isn't one of its
    /// proper children, in an anonymous row group, row or cell, as the
    /// parent's part says, and appends it to `children`.
    fn wrap_table_run(
        &mut self,
        cx: &mut ContentContext<'_>,
        parent: usize,
        run: &mut Vec<Child>,
        children: &mut Vec<taffy::NodeId>,
    ) {
        if run.iter().all(is_space_text) {
            run.clear();
            return;
        }
        let content = std::mem::take(run);
        let parent_style = self.boxes[parent].style.clone();
        let display = match TablePart::of(parent_style.display) {
            Some(TablePart::Table) => TableDisplay::RowGroup,
            Some(TablePart::RowGroup) => TableDisplay::Row,
            _ => TableDisplay::Cell,
        };
        let style = anonymous_style(&parent_style, Display::Table(display));
        cx.stats.built = cx.stats.built.wrapping_add(1);
        let anonymous = self.allocate(LayoutBox::new(
            None,
            BoxRole::AnonymousTable,
            style,
            &self.calcs,
        ));
        self.boxes[anonymous].parent = Some(parent);
        // A cell is a block container: its content is set as any other's.
        self.set_content(cx, anonymous, content);
        children.push(anonymous.into());
    }

    /// `content` of non-table box `index`, with each run of table parts in
    /// it (row groups, rows, cells and captions), and the white space
    /// between them, wrapped in an anonymous table (css-tables 3, "Fixup";
    /// Blink's `LayoutObject::AddChild`): an inline-table if they are in
    /// an inline element (an atomic inline there), otherwise a table.
    pub(crate) fn wrap_misparented(
        &mut self,
        cx: &mut ContentContext<'_>,
        index: usize,
        content: Vec<Child>,
    ) -> Vec<Child> {
        let misparented = |tree: &Self, child: &Child| {
            matches!(child, Child::Block(id) if matches!(
                TablePart::of(tree.boxes[usize::from(*id)].style.display),
                Some(TablePart::RowGroup | TablePart::Row | TablePart::Cell | TablePart::Caption)
            ))
        };
        if !content.iter().any(|child| misparented(self, child)) {
            return content;
        }
        let mut out = Vec::new();
        // The styles of the inline elements open, the innermost last.
        let mut open: Vec<Arc<ComputedStyle>> = Vec::new();
        let mut group = Vec::new();
        for child in content {
            let space = matches!(&child, Child::Inline(item @ InlineItem::Text(..))
                if InlineItem::is_collapsible_space_only(std::slice::from_ref(item)));
            if misparented(self, &child) || (space && !group.is_empty()) {
                group.push(child);
                continue;
            }
            self.wrap_in_table(cx, index, &open, &mut group, &mut out);
            match &child {
                Child::Inline(InlineItem::Open(_, style)) => open.push(style.clone()),
                Child::Inline(InlineItem::Close(..)) => {
                    open.pop();
                }
                _ => {}
            }
            out.push(child);
        }
        self.wrap_in_table(cx, index, &open, &mut group, &mut out);
        out
    }

    /// Wraps `group`, table parts out of a table and the white space
    /// between them, in an anonymous table (or inline-table, in the
    /// innermost of the `open` inline elements), appended to `out`; the
    /// white space after the last one goes after it.
    fn wrap_in_table(
        &mut self,
        cx: &mut ContentContext<'_>,
        index: usize,
        open: &[Arc<ComputedStyle>],
        group: &mut Vec<Child>,
        out: &mut Vec<Child>,
    ) {
        let Some(last) = group.iter().rposition(|c| matches!(c, Child::Block(_))) else {
            out.append(group);
            return;
        };
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "last < group.len(), so last + 1 <= len"
        )]
        let after = group.split_off(last + 1);
        let content = std::mem::take(group);
        let (parent_style, display) = match open.last() {
            Some(style) => (style.clone(), Display::InlineTable),
            None => (
                self.boxes[index].style.clone(),
                Display::Table(TableDisplay::Table),
            ),
        };
        let style = anonymous_style(&parent_style, display);
        cx.stats.built = cx.stats.built.wrapping_add(1);
        let table = self.allocate(LayoutBox::new(
            None,
            BoxRole::AnonymousTable,
            style.clone(),
            &self.calcs,
        ));
        self.set_table_content(cx, table, content);
        out.push(if open.is_empty() {
            Child::Block(table.into())
        } else {
            Child::Inline(InlineItem::Atomic(table.into(), style))
        });
        out.extend(after);
    }
}

/// The `len` slots from `start`: a cell's columns, or its rows in its row
/// group.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "a cell's columns end where they were counted, in a usize, as it was put in \
              its slots; its rows within its group's (its rowspan is cut to the rows left)"
)]
fn slots(start: usize, len: usize) -> std::ops::Range<usize> {
    start..start + len
}

/// A cell where its table's grid has it.
#[derive(Debug, Clone, Copy)]
struct GridCell {
    index: usize,
    column: usize,
    colspan: usize,
    /// At least 1, and no further than its row group's last row.
    rowspan: usize,
}

/// The cells spanning rows down into the next, while a row group's cells
/// are put in their slots (Blink's `ColspanCellTabulator`): only those,
/// so finding the free columns costs as much as the row's cells and these,
/// however many columns the table has.
#[derive(Default)]
struct RowspanCellTabulator {
    /// By the column they start in. Two may overlap (a cell spanning
    /// columns over one from above): a column is taken while either is.
    cells: Vec<RowspanCell>,
}

struct RowspanCell {
    column: usize,
    colspan: usize,
    /// The rows it is in from the current one on, this one included.
    rows: usize,
}

impl RowspanCellTabulator {
    /// The current row's free columns, found left to right.
    fn free_columns(&self) -> FreeColumns<'_> {
        FreeColumns {
            above: &self.cells,
            next: 0,
            added: Vec::new(),
        }
    }

    /// The row ends, with `added` its cells that span rows into the next:
    /// each cell has a row fewer to go (Blink's `EndRow`).
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "a cell kept has a row to go (rows > 0), and one added spans more than one"
    )]
    fn end_row(&mut self, added: Vec<RowspanCell>) {
        self.cells.retain_mut(|cell| {
            cell.rows -= 1;
            cell.rows > 0
        });
        self.cells.extend(added.into_iter().map(|cell| RowspanCell {
            rows: cell.rows - 1,
            ..cell
        }));
        self.cells.sort_by_key(|cell| cell.column);
    }
}

/// [`RowspanCellTabulator::free_columns`]: the cells from the rows above,
/// passed left to right as the row's cells are put down.
struct FreeColumns<'a> {
    above: &'a [RowspanCell],
    /// The first of `above` that may still take a column to the right.
    next: usize,
    /// The row's own cells that span rows.
    added: Vec<RowspanCell>,
}

impl FreeColumns<'_> {
    /// The first column from `column` on that no cell from above takes
    /// (Blink's `FindNextFreeColumn`); `column` only grows along a row.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "a cell's columns end where they were counted, in a usize; next < above.len()"
    )]
    fn next_free(&mut self, mut column: usize) -> usize {
        while let Some(cell) = self.above.get(self.next) {
            if cell.column > column {
                break;
            }
            // Over this column, or ended before it: past it either way.
            column = column.max(cell.column + cell.colspan);
            self.next += 1;
        }
        column
    }

    /// A cell of the row at `column` spanning `rowspan` rows, more than one.
    fn add(&mut self, column: usize, colspan: usize, rowspan: usize) {
        self.added.push(RowspanCell {
            column,
            colspan,
            rows: rowspan,
        });
    }
}

struct GridRow {
    index: usize,
    cells: Vec<GridCell>,
}

struct GridSection {
    index: usize,
    rows: Vec<GridRow>,
}

/// A table's structure: its captions, and its row groups in the order
/// they are laid out, each cell in its slots (HTML's "forming a table";
/// Blink's `TableGroupedChildren` and `TableNode::GetColumnCount`).
struct Grid {
    captions: Vec<usize>,
    sections: Vec<GridSection>,
    /// Whether a cell starts in each column: one where none does is merged
    /// away, with no width or border spacing of its own, unless a column
    /// with a width defines it (Blink's mergeable columns).
    starts: Vec<bool>,
    /// The column or column group that defines each column, if one does.
    defs: Vec<Option<ColumnDef>>,
    /// The column groups and columns, in tree order, with the columns each
    /// spans (a group after its columns).
    column_boxes: Vec<ColumnBox>,
}

/// What defines a column: a column (or a column group without any),
/// `index`, in column group `group` if it is in one.
#[derive(Debug, Clone, Copy)]
struct ColumnDef {
    index: usize,
    group: Option<usize>,
}

/// A column group or column, `index`, and the columns it spans.
struct ColumnBox {
    index: usize,
    group: bool,
    columns: std::ops::Range<usize>,
}

impl Grid {
    fn cells(&self) -> impl Iterator<Item = &GridCell> {
        self.sections
            .iter()
            .flat_map(|section| &section.rows)
            .flat_map(|row| &row.cells)
    }
}

/// A cell's widths as its column sizing sees them (Blink's
/// `TableTypes::CellInlineConstraint`), of the border box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CellConstraint {
    pub(crate) min: f32,
    pub(crate) max: f32,
    /// A percentage `width`.
    pub(crate) percent: Option<f32>,
    /// A fixed `width`.
    pub(crate) constrained: bool,
}

/// A column's widths (Blink's `TableTypes::Column`), from the cells in it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Column {
    pub(crate) min: f32,
    pub(crate) max: f32,
    pub(crate) percent: Option<f32>,
    pub(crate) constrained: bool,
    /// No cell starts in it: no width of its own.
    pub(crate) mergeable: bool,
    /// A fixed table's percentage column's cell's padding and border, with
    /// `box-sizing: content-box`: its width is the percentage and these
    /// (Blink's `percentage_border_padding`).
    pub(crate) percent_padding: f32,
    /// `visibility: collapse`: sized as the others, then 0 wide, with no
    /// border spacing (css-tables 3; Blink's `is_collapsed`).
    pub(crate) collapsed: bool,
    /// A cell or a column with a fixed width gave it its widths.
    set: bool,
}

impl Column {
    /// Takes in a cell that spans only this column (Blink's
    /// `Column::Encompass`): a fixed width replaces the content's widths,
    /// and only fixed widths grow a column that has one.
    pub(crate) fn encompass(&mut self, cell: CellConstraint) {
        if self.set {
            self.min = self.min.max(cell.min);
            match (self.constrained, cell.constrained) {
                (true, true) | (false, false) => self.max = self.max.max(cell.max),
                (true, false) => {}
                (false, true) => self.max = cell.max,
            }
        } else {
            self.min = cell.min;
            self.max = cell.max;
            self.set = true;
        }
        self.max = self.max.max(self.min);
        self.constrained |= cell.constrained;
        if let Some(percent) = cell.percent {
            self.percent = Some(self.percent.map_or(percent, |p| p.max(percent)));
        }
    }

    /// A percentage column's width out of `target`, not under its minimum
    /// (Blink's `Column::ResolvePercentInlineSize`).
    fn percent_width(&self, target: f32) -> f32 {
        self.min.max(self.percent.unwrap_or(0.0) * target / 100.0)
    }
}

/// Distributes `target` to `columns` (Blink's
/// `DistributeInlineSizeToComputedInlineSizeAuto`; css-tables 3,
/// "Distributing width to columns"): at least each column's minimum; then,
/// guess by guess, the percentage columns up to their percentage, the
/// fixed ones up to their width, the auto ones up to their maximum; what
/// is left grows the auto columns, or without any the fixed ones (unless
/// the target isn't `constrained`: a spanning cell without a fixed width
/// doesn't widen fixed columns), or else the percentage ones, in
/// proportion to their maximum (or percentage). A mergeable column gets 0.
pub(crate) fn distribute(target: f32, columns: &[Column], constrained: bool) -> Vec<f32> {
    const MIN: usize = 0;
    const PERCENT: usize = 1;
    const SPECIFIED: usize = 2;
    const MAX: usize = 3;
    let mut guesses = [0.0f32; 4];
    let mut increases = [0.0f32; 4];
    let (mut percent_count, mut fixed_count, mut auto_count) = (0usize, 0usize, 0usize);
    let (mut total_percent, mut auto_max, mut fixed_max) = (0.0f32, 0.0f32, 0.0f32);
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "each count is of columns: at most columns.len()"
    )]
    for column in columns.iter().filter(|c| !c.mergeable) {
        guesses[MIN] += column.min;
        if column.percent.is_some() {
            percent_count += 1;
            total_percent += column.percent.unwrap_or(0.0);
            let width = column.percent_width(target);
            guesses[PERCENT] += width;
            guesses[SPECIFIED] += width;
            guesses[MAX] += width;
            increases[PERCENT] += width - column.min;
        } else if column.constrained {
            fixed_count += 1;
            fixed_max += column.max;
            guesses[PERCENT] += column.min;
            guesses[SPECIFIED] += column.max;
            guesses[MAX] += column.max;
            increases[SPECIFIED] += column.max - column.min;
        } else {
            auto_count += 1;
            auto_max += column.max;
            guesses[PERCENT] += column.min;
            guesses[SPECIFIED] += column.min;
            guesses[MAX] += column.max;
            increases[MAX] += column.max - column.min;
        }
    }
    // Never below the minimums (a spanning cell may be narrower).
    let target = target.max(guesses[MIN]);
    let guess = (MIN..=MAX).find(|&i| guesses[i] >= target);
    let share = |room: f32, part: f32, whole: f32, count: usize| {
        if whole > 0.0 {
            room * part / whole
        } else {
            room / count as f32
        }
    };
    columns
        .iter()
        .map(|column| {
            if column.mergeable {
                return 0.0;
            }
            let percent = column.percent.is_some();
            match guess {
                Some(MIN) => column.min,
                // The percentage columns grow toward their percentage, in
                // proportion to how far it is.
                Some(PERCENT) => {
                    if percent {
                        let room = target - guesses[MIN];
                        column.min
                            + share(
                                room,
                                column.percent_width(target) - column.min,
                                increases[PERCENT],
                                percent_count,
                            )
                    } else {
                        column.min
                    }
                }
                // The fixed columns grow toward their width.
                Some(SPECIFIED) => {
                    if percent {
                        column.percent_width(target)
                    } else if column.constrained {
                        let room = target - guesses[PERCENT];
                        column.min
                            + share(
                                room,
                                column.max - column.min,
                                increases[SPECIFIED],
                                fixed_count,
                            )
                    } else {
                        column.min
                    }
                }
                // The auto columns grow toward their maximum; exactly the
                // maxima (a table of `auto` width) are taken as they are,
                // not worked out, which could wrap a line by rounding.
                Some(_) => {
                    if percent {
                        column.percent_width(target)
                    } else if column.constrained || target == guesses[MAX] {
                        column.max
                    } else {
                        let room = target - guesses[SPECIFIED];
                        column.min
                            + share(room, column.max - column.min, increases[MAX], auto_count)
                    }
                }
                // Past every maximum.
                None => {
                    let room = target - guesses[MAX];
                    if auto_count > 0 {
                        if percent {
                            column.percent_width(target)
                        } else if column.constrained {
                            column.max
                        } else {
                            column.max + share(room, column.max, auto_max, auto_count)
                        }
                    } else if fixed_count > 0 && constrained {
                        if percent {
                            column.percent_width(target)
                        } else {
                            column.max + share(room, column.max, fixed_max, fixed_count)
                        }
                    } else if percent {
                        column.percent_width(target)
                            + share(
                                room,
                                column.percent.unwrap_or(0.0),
                                total_percent,
                                percent_count,
                            )
                    } else {
                        column.max
                    }
                }
            }
        })
        .collect()
}

/// Distributes `target` to the columns of a fixed table (Blink's
/// `DistributeInlineSizeToComputedInlineSizeFixed`): the fixed ones their
/// width, the percentage ones their percentage of it (and their padding,
/// with `box-sizing: content-box`); what is left evenly to the `auto`
/// ones, or without any to the fixed ones in proportion to their widths
/// (a 0 one doesn't grow), or else to the percentage ones in proportion to
/// theirs. Too narrow for them all, the percentage columns share what the
/// fixed ones leave, in proportion. A mergeable column gets 0.
pub(crate) fn distribute_fixed(target: f32, columns: &[Column]) -> Vec<f32> {
    let live = || columns.iter().filter(|c| !c.mergeable);
    let percent_width = |c: &Column| c.percent.unwrap_or(0.0) * target / 100.0 + c.percent_padding;
    let auto = |c: &Column| c.percent.is_none() && !c.constrained;
    let fixed = |c: &Column| c.percent.is_none() && c.constrained;
    let fixed_total: f32 = live().filter(|c| fixed(c)).map(|c| c.max).sum();
    let percent_total: f32 = live()
        .filter(|c| c.percent.is_some())
        .map(percent_width)
        .sum();
    let auto_count = live().filter(|c| auto(c)).count();
    let fixed_count = live().filter(|c| fixed(c)).count();
    let room = target - fixed_total - percent_total;
    columns
        .iter()
        .map(|c| {
            if c.mergeable {
                0.0
            } else if room < 0.0 {
                // The fixed columns fit (the table is no narrower); the
                // percentage ones share what they leave.
                if c.percent.is_some() {
                    (target - fixed_total).max(0.0) * percent_width(c) / percent_total
                } else if auto(c) {
                    0.0
                } else {
                    c.max
                }
            } else if c.percent.is_some() {
                let grows = auto_count == 0 && fixed_total == 0.0;
                percent_width(c)
                    + if grows {
                        room * percent_width(c) / percent_total
                    } else {
                        0.0
                    }
            } else if auto(c) {
                room / auto_count as f32
            } else if auto_count > 0 {
                c.max
            } else if fixed_total > 0.0 {
                c.max + room * c.max / fixed_total
            } else if percent_total > 0.0 {
                0.0
            } else {
                room / fixed_count as f32
            }
        })
        .collect()
}

/// Spreads a cell spanning `columns` over them (Blink's
/// `DistributeColspanCellToColumnsAuto`): its percentage beyond theirs to
/// those without one, in proportion to their maximum; then its minimum and
/// maximum as a table's width is, each column taking what it gets if that
/// is more. The border spacing between the columns isn't theirs.
pub(crate) fn distribute_colspan(columns: &mut [Column], cell: CellConstraint, spacing: f32) {
    let effective = columns.iter().filter(|c| !c.mergeable).count();
    let inner = spacing * effective.saturating_sub(1) as f32;
    for column in columns.iter_mut() {
        column.set = true;
    }
    if let Some(cell_percent) = cell.percent {
        let mut columns_percent = 0.0;
        let (mut others, mut others_max) = (0usize, 0.0f32);
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "a count of columns: at most columns.len()"
        )]
        for column in columns.iter().filter(|c| !c.mergeable) {
            match column.percent {
                Some(percent) => columns_percent += percent,
                None => {
                    others += 1;
                    others_max += column.max;
                }
            }
        }
        let surplus = cell_percent - columns_percent;
        if surplus > 0.0 && others > 0 {
            for column in columns
                .iter_mut()
                .filter(|c| !c.mergeable && c.percent.is_none())
            {
                column.percent = Some(if others_max > 0.0 {
                    surplus * column.max / others_max
                } else {
                    surplus / others as f32
                });
            }
        }
    }
    let widths = distribute((cell.min - inner).max(0.0), columns, true);
    for (column, width) in columns.iter_mut().zip(widths) {
        column.min = column.min.max(width);
    }
    // An unconstrained cell's maximum doesn't grow fixed columns.
    let widths = distribute((cell.max - inner).max(0.0), columns, cell.constrained);
    for (column, width) in columns.iter_mut().zip(widths) {
        column.max = column.max.max(width).max(column.min);
    }
}

/// The grid's min-content and max-content widths, without border
/// spacing (Blink's `ComputeGridInlineMinMax`): the columns' minima; their
/// maxima, which is what the table contributes to its container's
/// intrinsic sizes; and the maximum its own layout fits into its room,
/// with percentage columns as wide as it takes for each to be its
/// percentage of the table and the others to fit in the rest.
pub(crate) fn grid_widths(columns: &[Column]) -> (f32, f32, f32) {
    // As wide as Blink lets a table's maximum go (`kTableMaxInlineSize`).
    const LIMIT: f32 = 1_000_000.0;
    let (mut min, mut max) = (0.0f32, 0.0f32);
    let plain: f32 = columns.iter().map(|c| c.max).sum();
    let mut remaining = 100.0f32;
    let mut percent_estimate: Option<f32> = None;
    for column in columns {
        min += column.min;
        match column.percent {
            Some(percent) => {
                let estimate = if percent > 0.0 && column.max > 0.0 {
                    column.max * 100.0 / percent
                } else {
                    0.0
                };
                percent_estimate = Some(percent_estimate.unwrap_or(0.0).max(estimate));
                remaining -= percent;
            }
            None => max += column.max,
        }
    }
    if let Some(percent_estimate) = percent_estimate {
        let remaining = remaining.max(0.0);
        let others = if remaining > 0.0 {
            max * 100.0 / remaining
        } else if max > 0.0 {
            LIMIT
        } else {
            0.0
        };
        max = percent_estimate.max(others).min(LIMIT);
    }
    (min, plain.max(min), max.max(plain).max(min))
}

/// Where each column is in the grid's content box: its left edge and
/// width (Blink's `ComputeLocationsFromColumns`); a merged one, or a
/// collapsed one if `shrink_collapsed`, is where the one before it ends, 0
/// wide.
fn column_locations(
    widths: &[f32],
    columns: &[Column],
    spacing: f32,
    shrink_collapsed: bool,
) -> Vec<(f32, f32)> {
    let (mut x, mut end) = (spacing, spacing);
    widths
        .iter()
        .zip(columns)
        .map(|(&width, column)| {
            if column.mergeable || (shrink_collapsed && column.collapsed) {
                return (end, 0.0);
            }
            let at = (x, width);
            end = x + width;
            x = end + spacing;
            at
        })
        .collect()
}

/// What laying a table's grid out found, from its grid's border box.
struct GridLayout {
    /// The grid's content box's height.
    height: f32,
    /// How much taller the collapsed rows would have made it: the table is
    /// sized with them, then shorter by them (Blink's).
    collapsed: f32,
    /// The first row's baseline, from the grid content box's top.
    baseline: Option<f32>,
}

/// A cell as its row lays it out.
struct LaidOutCell {
    cell: GridCell,
    width: f32,
    /// Its border box's height laid out, its content's (Blink's intrinsic
    /// block size), and its style's `height` (a border box's).
    content_height: f32,
    css_height: f32,
    /// Its first line's baseline from its border box's top; its content
    /// box's bottom without one.
    baseline: f32,
    has_line: bool,
    output: LayoutOutput,
}

impl BoxTree {
    /// Table `index`'s structure.
    fn grid(&self, index: usize) -> Grid {
        let parts = |of: usize, part: TablePart| -> Vec<usize> {
            self.boxes[of]
                .children
                .iter()
                .map(|&child| usize::from(child))
                .filter(|&child| {
                    let style = &self.boxes[child].style;
                    !style.is_out_of_flow() && TablePart::of(style.display) == Some(part)
                })
                .collect()
        };
        let captions = parts(index, TablePart::Caption);
        let mut sections = parts(index, TablePart::RowGroup);
        // The first header group goes first, the first footer group last
        // (css-tables 3, "Table layout algorithm").
        let first = |sections: &[usize], display| {
            sections
                .iter()
                .position(|&s| self.boxes[s].style.display == Display::Table(display))
        };
        if let Some(header) = first(&sections, TableDisplay::HeaderGroup) {
            let header = sections.remove(header);
            sections.insert(0, header);
        }
        if let Some(footer) = first(&sections, TableDisplay::FooterGroup) {
            let footer = sections.remove(footer);
            sections.push(footer);
        }
        let mut starts: Vec<bool> = Vec::new();
        let sections = sections
            .into_iter()
            .map(|section| {
                let rows = parts(section, TablePart::Row);
                let count = rows.len();
                let mut spanning = RowspanCellTabulator::default();
                let rows = rows
                    .into_iter()
                    .enumerate()
                    .map(|(r, row)| {
                        let mut column = 0;
                        let mut free = spanning.free_columns();
                        let cells = parts(row, TablePart::Cell)
                            .into_iter()
                            .map(|index| {
                                column = free.next_free(column);
                                let (colspan, rowspan) = self.boxes[index].span;
                                let colspan = colspan.max(1) as usize;
                                #[expect(
                                    clippy::arithmetic_side_effects,
                                    reason = "r < count (enumerated); a cell ends at most \
                                              MAX_COLSPAN columns past the row's last, so the \
                                              columns count no further than the cells do"
                                )]
                                let (left, end) = (count - r, column + colspan);
                                let rowspan = match rowspan as usize {
                                    0 => left,
                                    n => n.min(left),
                                };
                                if rowspan > 1 {
                                    free.add(column, colspan, rowspan);
                                }
                                if starts.len() < end {
                                    starts.resize(end, false);
                                }
                                starts[column] = true;
                                let cell = GridCell {
                                    index,
                                    column,
                                    colspan,
                                    rowspan,
                                };
                                column = end;
                                cell
                            })
                            .collect();
                        let added = free.added;
                        spanning.end_row(added);
                        GridRow { index: row, cells }
                    })
                    .collect();
                GridSection {
                    index: section,
                    rows,
                }
            })
            .collect();
        // The columns the column groups and columns define, in tree order
        // (Blink's `TableNode`'s `ColumnVisitor`): a column `span` times,
        // or a column group without columns `span` times; there are as
        // many columns as they or the cells make, whichever is more.
        let mut defs: Vec<Option<ColumnDef>> = Vec::new();
        let mut column_boxes = Vec::new();
        let columns = |of: usize| -> Vec<usize> {
            self.boxes[of]
                .children
                .iter()
                .map(|&child| usize::from(child))
                .filter(|&child| {
                    TablePart::of(self.boxes[child].style.display) == Some(TablePart::Column)
                })
                .collect()
        };
        for &child in &self.boxes[index].children {
            let child = usize::from(child);
            let span = self.boxes[child].span.0.max(1) as usize;
            let start = defs.len();
            match TablePart::of(self.boxes[child].style.display) {
                Some(TablePart::Column) => {
                    defs.extend((0..span).map(|_| {
                        Some(ColumnDef {
                            index: child,
                            group: None,
                        })
                    }));
                }
                Some(TablePart::ColumnGroup) => {
                    let inside = columns(child);
                    if inside.is_empty() {
                        defs.extend((0..span).map(|_| {
                            Some(ColumnDef {
                                index: child,
                                group: None,
                            })
                        }));
                    }
                    for column in inside {
                        let from = defs.len();
                        let span = self.boxes[column].span.0.max(1) as usize;
                        defs.extend((0..span).map(|_| {
                            Some(ColumnDef {
                                index: column,
                                group: Some(child),
                            })
                        }));
                        column_boxes.push(ColumnBox {
                            index: column,
                            group: false,
                            columns: from..defs.len(),
                        });
                    }
                    column_boxes.push(ColumnBox {
                        index: child,
                        group: true,
                        columns: start..defs.len(),
                    });
                    continue;
                }
                _ => continue,
            }
            column_boxes.push(ColumnBox {
                index: child,
                group: false,
                columns: start..defs.len(),
            });
        }
        let count = starts.len().max(defs.len());
        starts.resize(count, false);
        defs.resize(count, None);
        Grid {
            captions,
            sections,
            starts,
            defs,
            column_boxes,
        }
    }

    /// Cell `index`'s widths for column sizing (Blink's
    /// `CreateCellInlineConstraint`): its content's min-content and
    /// max-content widths, its own `width` replacing the maximum (no
    /// narrower than the minimum), `min-width` and `max-width` holding
    /// them; a percentage `width` is kept as one.
    fn cell_constraint(&mut self, index: usize) -> CellConstraint {
        let style = self.boxes[index].style.clone();
        // Its percentages (its padding's) of 0: the table's width isn't
        // known yet (Blink's); a `calc()`'s length counts.
        let mut content = |space| {
            self.compute_child_layout(
                index.into(),
                LayoutInput {
                    known_dimensions: taffy::Size::NONE,
                    known_dimensions_are_definite: taffy::Size {
                        width: true,
                        height: true,
                    },
                    parent_size: taffy::Size {
                        width: Some(0.0),
                        height: None,
                    },
                    available_space: taffy::Size {
                        width: space,
                        height: AvailableSpace::MaxContent,
                    },
                    sizing_mode: SizingMode::ContentSize,
                    run_mode: RunMode::ComputeSize,
                    axis: RequestedAxis::Horizontal,
                    vertical_margins_are_collapsible: taffy::Line::FALSE,
                },
            )
            .size
            .width
        };
        let min_content = content(AvailableSpace::MinContent);
        let max_content = content(AvailableSpace::MaxContent);
        let edge = self.cell_edge(index, 0.0);
        let inset = edge.padding.left + edge.padding.right + edge.border.left + edge.border.right;
        let border_box = |px: f32| match style.box_sizing {
            BoxSizing::ContentBox => px + inset,
            BoxSizing::BorderBox => px.max(inset),
        };
        let (width, percent) = match style.width {
            Size::LengthPercentage(v) if let Some(p) = percent_width(v) => (None, Some(p.max(0.0))),
            Size::LengthPercentage(v) => (v.px().map(border_box), None),
            _ => (None, None),
        };
        let min_width = match style.min_width {
            LengthPercentageOrAuto::LengthPercentage(v) => v.px().map(border_box),
            LengthPercentageOrAuto::Auto => None,
        };
        // A `max-width` under the `min-width` is the `min-width` (Blink's
        // `InlineSizesFromStyle`).
        let max_width = match style.max_width {
            LengthPercentageOrNone::LengthPercentage(v) => v
                .px()
                .map(border_box)
                .map(|max| max.max(min_width.unwrap_or(0.0))),
            LengthPercentageOrNone::None => None,
        };
        let mut min = min_content.max(min_width.unwrap_or(0.0));
        let mut max = width.unwrap_or(max_content);
        if let Some(max_width) = max_width {
            max = max.min(max_width);
            min = min.min(max_width);
        }
        CellConstraint {
            min,
            max: max.max(min),
            percent,
            constrained: width.is_some(),
        }
    }

    /// The columns of `grid` sized from its cells (Blink's
    /// `ComputeColumnInlineConstraints`): the cells that span one column
    /// first, then the others, narrower spans first; the percentages
    /// held to 100 in all.
    fn column_constraints(&mut self, grid: &Grid, spacing: f32) -> Vec<Column> {
        let mut columns: Vec<Column> = grid
            .starts
            .iter()
            .zip(&grid.defs)
            .map(|(&starts, def)| {
                let mut column = def.map_or_else(Column::default, |def| self.defined_column(def));
                column.mergeable = !starts && column.max == 0.0 && column.percent.is_none();
                column
            })
            .collect();
        let mut spanning = Vec::new();
        let cells: Vec<GridCell> = grid.cells().copied().collect();
        for cell in cells {
            let constraint = self.cell_constraint(cell.index);
            if cell.colspan == 1 {
                columns[cell.column].encompass(constraint);
            } else {
                spanning.push((cell, constraint));
            }
        }
        spanning.sort_by_key(|(cell, _)| cell.colspan);
        for (cell, constraint) in spanning {
            distribute_colspan(
                &mut columns[slots(cell.column, cell.colspan)],
                constraint,
                spacing,
            );
        }
        let mut total = 0.0f32;
        for column in &mut columns {
            if let Some(percent) = column.percent {
                let percent = percent.min(100.0 - total).max(0.0);
                column.percent = Some(percent);
                total += percent;
            }
        }
        columns
    }

    /// Where `grid`'s column groups and columns are, the groups first (they
    /// are painted under the columns), its columns at `locations` in the
    /// grid's content box, which is at `origin` in the table: across its
    /// row groups, which are placed.
    fn table_columns(
        &self,
        grid: &Grid,
        locations: &[(f32, f32)],
        origin: PointF,
    ) -> Vec<TableColumn> {
        let (mut top, mut bottom) = (f32::INFINITY, f32::NEG_INFINITY);
        for section in &grid.sections {
            let layout = &self.boxes[section.index].layout;
            top = top.min(layout.location.y);
            bottom = bottom.max(layout.location.y + layout.size.height);
        }
        if grid.column_boxes.is_empty() || top > bottom {
            return Vec::new();
        }
        let mut out: Vec<TableColumn> = grid
            .column_boxes
            .iter()
            .filter_map(|column| {
                let spanned = locations.get(column.columns.clone())?;
                let left = spanned.first()?.0;
                let right = spanned.iter().map(|l| l.0 + l.1).fold(left, f32::max);
                let b = &self.boxes[column.index];
                Some(TableColumn {
                    node: b.node,
                    style: b.style.clone(),
                    group: column.group,
                    rect: RectF::new(origin.x + left, top, right - left, bottom - top),
                })
            })
            .collect();
        out.sort_by_key(|column| !column.group);
        out
    }

    /// The `width` column definition `def` gives its columns: its own, or
    /// its column group's fixed one if it has none (Blink's
    /// `default_inline_size`).
    fn column_width(&self, def: ColumnDef) -> Size {
        let own = self.boxes[def.index].style.width;
        match (own, def.group) {
            (Size::Auto, Some(group)) => match self.boxes[group].style.width {
                width @ Size::LengthPercentage(v) if v.px().is_some() => width,
                _ => own,
            },
            _ => own,
        }
    }

    /// The column that definition `def` makes for auto layout (Blink's
    /// `TableTypes::CreateColumn`): a fixed width is its maximum, which
    /// its cells' maxima don't change (but fixed ones), a percentage its
    /// percentage, a fixed `min-width` its minimum (not `max-width`); a 0
    /// width counts as none.
    fn defined_column(&self, def: ColumnDef) -> Column {
        let style = &self.boxes[def.index].style;
        let mut column = Column {
            collapsed: style.visibility == Visibility::Collapse,
            ..Column::default()
        };
        if let LengthPercentageOrAuto::LengthPercentage(v) = style.min_width
            && let Some(px) = v.px().filter(|&px| px > 0.0)
        {
            column.min = px;
            column.set = true;
        }
        if let Size::LengthPercentage(v) = self.column_width(def) {
            if let Some(px) = v.px().filter(|&px| px > 0.0) {
                column.max = px.max(column.min);
                column.constrained = true;
                column.set = true;
            } else if let Some(p) = percent_width(v).filter(|&p| p > 0.0) {
                column.percent = Some(p);
            }
        }
        column
    }

    /// The columns of `grid` for fixed table layout (Blink's
    /// `ComputeColumnInlineConstraints` for a fixed table): the columns'
    /// `width`s, then its first row's cells' for the others, a spanning
    /// cell's shared out evenly, no content measured.
    fn fixed_column_constraints(&self, grid: &Grid, spacing: f32) -> Vec<Column> {
        // No column is merged away: every column a cell spans has its
        // share of its width (Blink's).
        let mut defined = vec![false; grid.defs.len()];
        let mut columns: Vec<Column> = grid
            .defs
            .iter()
            .zip(&mut defined)
            .map(|(def, defined)| {
                let mut column = Column {
                    set: true,
                    ..Column::default()
                };
                let Some(def) = *def else {
                    return column;
                };
                column.collapsed = self.boxes[def.index].style.visibility == Visibility::Collapse;
                if let Size::LengthPercentage(v) = self.column_width(def) {
                    if let Some(p) = percent_width(v) {
                        column.percent = Some(p.max(0.0));
                        *defined = true;
                    } else if let Some(px) = v.px() {
                        column.constrained = true;
                        column.min = px.max(0.0);
                        column.max = px.max(0.0);
                        *defined = true;
                    }
                }
                column
            })
            .collect();
        let Some(row) = grid.sections.iter().find_map(|s| s.rows.first()) else {
            return columns;
        };
        for cell in &row.cells {
            let style = &self.boxes[cell.index].style;
            let span = cell.colspan;
            // A column with a `width` keeps it (Blink's
            // `DistributeColspanCellToColumnsFixed` fills the others).
            let spanned = columns[slots(cell.column, span)]
                .iter_mut()
                .zip(&defined[slots(cell.column, span)])
                .filter(|(_, defined)| !**defined)
                .map(|(column, _)| column);
            let edge = self.cell_edge(cell.index, 0.0);
            let inset =
                edge.padding.left + edge.padding.right + edge.border.left + edge.border.right;
            match style.width {
                Size::LengthPercentage(v) if let Some(p) = percent_width(v) => {
                    // A spanning cell shares out its percentage, not its
                    // padding (Blink's).
                    let padding = match style.box_sizing {
                        BoxSizing::ContentBox if span == 1 => inset,
                        _ => 0.0,
                    };
                    for column in spanned {
                        column.percent = Some(p.max(0.0) / span as f32);
                        column.percent_padding = padding;
                    }
                }
                Size::LengthPercentage(v) if let Some(px) = v.px() => {
                    let width = match style.box_sizing {
                        BoxSizing::ContentBox => px + inset,
                        BoxSizing::BorderBox => px.max(inset),
                    };
                    #[expect(
                        clippy::arithmetic_side_effects,
                        reason = "a cell spans a column at least"
                    )]
                    let each = ((width - spacing * (span - 1) as f32) / span as f32).max(0.0);
                    for column in spanned {
                        column.constrained = true;
                        column.min = each;
                        column.max = each;
                    }
                }
                _ => {}
            }
        }
        columns
    }

    /// Lays table `index` out: a leaf to taffy, whose measure sizes the
    /// columns and, unless only its width is asked for, the rows, and,
    /// laid out for real, places its parts. The table's own `width`,
    /// `height` and their limits are worked out here, not by the leaf:
    /// the grid's content keeps a table from being narrower or shorter
    /// than it (css-tables 3, "Computing the table width").
    pub(crate) fn compute_table(&mut self, index: usize, inputs: LayoutInput) -> LayoutOutput {
        let style = self.boxes[index].style.clone();
        let grid = self.grid(index);
        // Collapsed borders: each cell's are the half of its edges, the
        // table's the half of its outermost ones; the table has no padding
        // and no border spacing (css-tables 3; Blink's `TableBorders`).
        let borders = (style.border_collapse == BorderCollapse::Collapse)
            .then(|| self.table_borders(index, &grid));
        let rows = grid
            .sections
            .iter()
            .flat_map(|section| section.rows.iter().map(move |row| (section.index, row)));
        for (r, (section, row)) in rows.enumerate() {
            for cell in &row.cells {
                let at = (r, cell.column);
                let border = borders
                    .as_ref()
                    .map(|b| b.border(at, (cell.rowspan, cell.colspan)));
                self.set_cell_border(cell.index, border);
                let layered = [section, row.index, cell.index]
                    .iter()
                    .any(|&i| self.boxes[i].style.is_stacked());
                let b = &mut self.boxes[cell.index];
                let inside = border.is_some() && layered;
                if b.background_inside_border != inside {
                    b.background_inside_border = inside;
                    b.needs_fragment = true;
                }
            }
        }
        // Its sizes are worked out here, not by the leaf.
        let mut leaf_style = self.boxes[index].taffy_style.clone();
        leaf_style.size = taffy::Size::auto();
        leaf_style.min_size = taffy::Size::auto();
        leaf_style.max_size = taffy::Size::auto();
        let table_border = borders.as_ref().map(TableBorders::table_border);
        // (Its padding is 0 in its taffy style already.)
        if let Some(border) = table_border {
            leaf_style.border = border_rect(border);
        }
        let inset = content_box_inset(&leaf_style, inputs.parent_size.width);
        let (inset_x, inset_y) = (inset.left + inset.right, inset.top + inset.bottom);
        let spacing = if borders.is_some() {
            computed_style::BorderSpacing::ZERO
        } else {
            style.rare_inherited.border_spacing
        };
        // `table-layout: fixed` with a `width` (css-tables 3, "Fixed table
        // layout"; Blink's `IsFixedTableLayout`).
        let fixed = style.table_layout == TableLayout::Fixed && style.width != Size::Auto;
        let columns = if fixed {
            self.fixed_column_constraints(&grid, spacing.horizontal)
        } else {
            self.column_constraints(&grid, spacing.horizontal)
        };
        let count = columns.iter().filter(|c| !c.mergeable).count();
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "count <= columns.len(), so count + 1 fits"
        )]
        let spacing_x = if count > 0 {
            spacing.horizontal * (count + 1) as f32
        } else {
            0.0
        };
        let (grid_min, grid_intrinsic_max, grid_max) = if fixed {
            // As wide as its fixed columns at least.
            let fixed: f32 = columns
                .iter()
                .filter(|c| c.constrained)
                .map(|c| c.max)
                .sum();
            (fixed, fixed, fixed)
        } else {
            grid_widths(&columns)
        };
        // No narrower than a caption's min-content width (Blink's
        // `ComputeCaptionMinMax`).
        let mut caption_min = 0.0f32;
        for &caption in &grid.captions {
            caption_min = caption_min.max(self.width(caption.into(), AvailableSpace::MinContent));
        }
        let table_min = (grid_min + spacing_x + inset_x).max(caption_min);
        let table_max = (grid_max + spacing_x + inset_x).max(table_min);
        // What it contributes to its container's intrinsic sizes, and its
        // `max-content` width: no percentage widens it (Blink's
        // `ComputeGridInlineMinMax` out of a layout pass).
        let intrinsic_max = (grid_intrinsic_max + spacing_x + inset_x).max(table_min);
        // Its own sizes, of the border box.
        let border_box = |px: f32, inset: f32| match style.box_sizing {
            BoxSizing::ContentBox => px + inset,
            BoxSizing::BorderBox => px.max(inset),
        };
        let resolve = |v: LengthPercentage, base: Option<f32>| match v.px() {
            Some(px) => Some(px),
            None => base.map(|base| v.resolve(base)),
        };
        let (parent_width, parent_height) = (inputs.parent_size.width, inputs.parent_size.height);
        let css_width = match style.width {
            Size::LengthPercentage(v) => resolve(v, parent_width).map(|w| border_box(w, inset_x)),
            Size::MinContent => Some(table_min),
            Size::MaxContent => Some(intrinsic_max),
            Size::Auto | Size::FitContent => None,
        };
        let min_width = match style.min_width {
            LengthPercentageOrAuto::LengthPercentage(v) => {
                resolve(v, parent_width).map(|w| border_box(w, inset_x))
            }
            LengthPercentageOrAuto::Auto => None,
        };
        let max_width = match style.max_width {
            LengthPercentageOrNone::LengthPercentage(v) => {
                resolve(v, parent_width).map(|w| border_box(w, inset_x))
            }
            LengthPercentageOrNone::None => None,
        };
        let css_height = match style.height {
            Size::LengthPercentage(v) => resolve(v, parent_height).map(|h| border_box(h, inset_y)),
            _ => None,
        };
        let min_height = match style.min_height {
            LengthPercentageOrAuto::LengthPercentage(v) => {
                resolve(v, parent_height).map(|h| border_box(h, inset_y))
            }
            LengthPercentageOrAuto::Auto => None,
        };
        let grid_min_height =
            (css_height.unwrap_or(0.0).max(min_height.unwrap_or(0.0)) - inset_y).max(0.0);
        let known = inputs.known_dimensions;
        let run_mode = inputs.run_mode;
        let perform = run_mode == RunMode::PerformLayout;
        let only_width =
            run_mode == RunMode::ComputeSize && inputs.axis == RequestedAxis::Horizontal;
        let mut baseline = None;
        let mut candidates = OofCandidates::new();
        let mut table_columns = Vec::new();
        let mut collapsed_borders = None;
        // Its border box as its content has it, which a size it is given
        // (an out-of-flow table's `width`, a flex item's) can't shrink.
        let mut measured = taffy::Size::ZERO;
        let mut output = compute_leaf_layout(
            inputs,
            &leaf_style,
            |_, _| 0.0,
            |_, available| {
                // The border box's width: the one it is given, or its own, or
                // as wide as its content in the room it has; never narrower
                // than its content's minimum.
                let width = known
                    .width
                    .map(|width| width.max(table_min))
                    .unwrap_or_else(|| {
                        let width = css_width.unwrap_or(match available.width {
                            AvailableSpace::Definite(room) => {
                                (room + inset_x).max(0.0).min(table_max)
                            }
                            AvailableSpace::MinContent => table_min,
                            AvailableSpace::MaxContent => intrinsic_max,
                        });
                        let width = max_width.map_or(width, |max| width.min(max));
                        min_width.map_or(width, |min| width.max(min)).max(table_min)
                    });
                let target = (width - inset_x - spacing_x).max(0.0);
                let widths = if fixed {
                    distribute_fixed(target, &columns)
                } else {
                    distribute(target, &columns, true)
                };
                // Collapsed columns are sized, then take no room: the table
                // is narrower by them and their border spacing (Blink's).
                let removed: f32 = columns
                    .iter()
                    .zip(&widths)
                    .filter(|(c, _)| c.collapsed && !c.mergeable)
                    .map(|(_, &w)| w + spacing.horizontal)
                    .sum();
                let width = width - removed;
                let content_width = (width - inset_x).max(0.0);
                measured.width = width;
                if only_width {
                    return taffy::Size {
                        width: content_width,
                        height: 0.0,
                    };
                }
                // The rows are sized with the collapsed columns, the cells
                // placed without them (Blink's).
                let sizing = column_locations(&widths, &columns, spacing.horizontal, false);
                let locations = column_locations(&widths, &columns, spacing.horizontal, true);
                // The captions above the grid, the grid, the captions below.
                let mut y = 0.0;
                for side in [CaptionSide::Top, CaptionSide::Bottom] {
                    if side == CaptionSide::Bottom {
                        let origin = PointF::new(inset.left, y + inset.top);
                        let layout = self.lay_out_grid(
                            &grid,
                            &sizing,
                            &locations,
                            content_width,
                            spacing.vertical,
                            grid_min_height,
                            css_height.is_some(),
                            perform.then_some((origin, &mut candidates)),
                        );
                        baseline = layout.baseline.map(|baseline| origin.y + baseline);
                        if perform {
                            table_columns = self.table_columns(&grid, &locations, origin);
                            collapsed_borders = borders.as_ref().map(|borders| {
                                Arc::new(self.collapsed_borders(borders, &grid, &locations, origin))
                            });
                            // A cell all of whose columns are collapsed is
                            // hidden (Blink's `is_hidden_due_to_layout`).
                            for cell in grid.cells() {
                                let spanned = &columns[slots(cell.column, cell.colspan)];
                                let hidden = spanned.iter().all(|c| c.collapsed);
                                let b = &mut self.boxes[cell.index];
                                if b.hidden != hidden {
                                    b.hidden = hidden;
                                    b.needs_fragment = true;
                                }
                            }
                            // The table has read its columns: they have no
                            // layout of their own (Blink's
                            // `ClearNeedsLayout` of each column).
                            for column in &grid.column_boxes {
                                let b = &mut self.boxes[column.index];
                                b.needs_layout = false;
                                b.needs_fragment = false;
                            }
                        }
                        y += css_height
                            .unwrap_or(0.0)
                            .max(min_height.unwrap_or(0.0))
                            .max(inset_y + layout.height + layout.collapsed)
                            - layout.collapsed;
                    }
                    for &caption in &grid.captions {
                        if self.boxes[caption].style.caption_side == side {
                            y += self.lay_out_caption(caption, width, y, perform, &mut candidates);
                        }
                    }
                }
                measured.height = y;
                taffy::Size {
                    width: content_width,
                    height: (y - inset_y).max(0.0),
                }
            },
        );
        output.size = output.size.f32_max(measured);
        // A table's margins don't collapse through it, empty or not.
        output.margins_can_collapse_through = false;
        output.baselines.first = baseline;
        if !perform {
            return output;
        }
        self.boxes[index].table_baseline = baseline;
        self.boxes[index].table_columns = table_columns;
        self.boxes[index].collapsed_border = table_border;
        self.boxes[index].collapsed_borders = collapsed_borders;
        // Its own out-of-flow children, at the start of its content box.
        let children = self.boxes[index].children.clone();
        for (order, &child) in children.iter().enumerate() {
            if self.boxes[usize::from(child)].style.is_out_of_flow() {
                candidates.push(self.static_candidate(
                    index,
                    child,
                    order,
                    PointF::new(inset.left, inset.top),
                ));
            }
        }
        output.oof_candidates = candidates;
        // Its padding box, which it lays out the out-of-flow boxes it is
        // the containing block of against.
        let border = leaf_style
            .border
            .resolve_or_zero(inputs.parent_size.width, |_, _| 0.0);
        output.oof_positioning_area = Some(OofPositioningArea {
            size: taffy::Size {
                width: (output.size.width - border.left - border.right).max(0.0),
                height: (output.size.height - border.top - border.bottom).max(0.0),
            },
            offset: taffy::Point {
                x: border.left,
                y: border.top,
            },
        });
        output
    }

    /// Lays caption `index` out as wide as its table, `width`, at `y`
    /// (placed there if `perform`, passing up its out-of-flow boxes into
    /// `candidates`), and returns its margin box's height.
    fn lay_out_caption(
        &mut self,
        index: usize,
        width: f32,
        y: f32,
        perform: bool,
        candidates: &mut OofCandidates,
    ) -> f32 {
        let style = self.boxes[index].style.clone();
        // `auto` margins are 0.
        let edge = Edge::of(&style, width);
        let m = edge.margin;
        let room = (width - m.left - m.right).max(0.0);
        let mut output = self.compute_child_layout(
            index.into(),
            LayoutInput {
                known_dimensions: taffy::Size {
                    width: (style.width == Size::Auto).then_some(room),
                    height: None,
                },
                known_dimensions_are_definite: taffy::Size {
                    width: true,
                    height: true,
                },
                parent_size: taffy::Size {
                    width: Some(width),
                    height: None,
                },
                available_space: taffy::Size {
                    width: AvailableSpace::Definite(room),
                    height: AvailableSpace::MaxContent,
                },
                sizing_mode: SizingMode::InherentSize,
                run_mode: if perform {
                    RunMode::PerformLayout
                } else {
                    RunMode::ComputeSize
                },
                axis: RequestedAxis::Both,
                vertical_margins_are_collapsible: taffy::Line::FALSE,
            },
        );
        if perform {
            let by = relative_offset(&style, width, None);
            let location = taffy::Point {
                x: m.left + by.x,
                y: y + m.top + by.y,
            };
            self.place(index, location, output.size, &edge);
            let mut theirs = output.oof_candidates.take();
            theirs.translate(location);
            candidates.append(&mut theirs);
        }
        output.size.height + m.top + m.bottom
    }

    /// Gives table part `index` its place and size, and `edge`'s padding
    /// and border (no margins but a caption's): its fragment is made
    /// again if they changed.
    fn place(
        &mut self,
        index: usize,
        location: taffy::Point<f32>,
        size: taffy::Size<f32>,
        edge: &Edge,
    ) {
        let rect = |s: computed_style::Sides<f32>| taffy::Rect {
            top: s.top,
            right: s.right,
            bottom: s.bottom,
            left: s.left,
        };
        let b = &mut self.boxes[index];
        let layout = taffy::Layout {
            location,
            size,
            padding: rect(edge.padding),
            border: rect(edge.border),
            margin: rect(edge.margin),
            ..b.layout
        };
        if layout != b.layout {
            b.layout = layout;
            b.needs_fragment = true;
        }
    }

    /// Lays out `grid`'s rows and cells in its columns, `locations` in its
    /// content box, `width` wide (Blink's `ComputeRowBlockSize` and
    /// `TableLayoutAlgorithm::GenerateFragment`): each row as tall as its
    /// cells (and its own `height`), each row group as its rows (and its
    /// own `height`), the grid as its row groups, at least `min_height`
    /// (its table's `height`: what is over goes to the row groups and
    /// their rows, Blink's `DistributeTableBlockSizeToSections`), each
    /// cell's content aligned in it by its `vertical-align`. Laid out for
    /// real (`place`: where the grid's content box is in the table, and the
    /// out-of-flow boxes passed up), the row groups, rows and cells are
    /// placed; a cell whose content needs its final height is laid out
    /// again at it (`definite`: the table has a `height`).
    #[allow(clippy::too_many_arguments)]
    fn lay_out_grid(
        &mut self,
        grid: &Grid,
        locations: &[(f32, f32)],
        placed: &[(f32, f32)],
        width: f32,
        spacing: f32,
        min_height: f32,
        definite: bool,
        mut place: Option<(PointF, &mut OofCandidates)>,
    ) -> GridLayout {
        // The rows go from the first column's left edge to the last one's
        // right edge (Blink's: the outer border spacing isn't in them).
        // A table without columns has its rows as wide as its content box
        // (css-tables 3; csswg-drafts#10132).
        let (left, row_width) = match placed.first() {
            Some(first) => {
                let right = placed.iter().map(|l| l.0 + l.1).fold(first.0, f32::max);
                (first.0, right - first.0)
            }
            None => (0.0, width),
        };
        // What the cells' percentages are of: the table's content box but
        // the border spacing at its edges (Blink's and Gecko's), once the
        // columns are sized (against 0).
        let cell_basis = (width - 2.0 * left).max(0.0);
        // Each row group's rows' heights, baselines and cells.
        let mut sections = Vec::with_capacity(grid.sections.len());
        // Each row group's own fixed `height` (a percentage only takes
        // what the table's `height` leaves, below).
        let own_height = |tree: &Self, index: usize| match tree.boxes[index].style.height {
            Size::LengthPercentage(v) => v.px(),
            _ => None,
        };
        for section in &grid.sections {
            let mut rows = self.row_heights(section, locations, cell_basis, spacing);
            // Its own `height` is a minimum, spread over its rows.
            if let Some(px) = own_height(self, section.index) {
                if rows.heights.is_empty() {
                    rows.empty_height = px;
                } else {
                    let have = section_height(&rows.heights, spacing);
                    rows.grow(px - have);
                }
            }
            sections.push(rows);
        }
        let with_rows = sections.iter().filter(|s| !s.heights.is_empty()).count();
        let content = if with_rows > 0 {
            spacing
                + sections
                    .iter()
                    .filter(|s| !s.heights.is_empty())
                    .map(|s| section_height(&s.heights, spacing) + spacing)
                    .sum::<f32>()
        } else {
            0.0
        };
        // The table's `height` beyond its rows goes to its row groups (an
        // empty one too), and in each to its rows (Blink's
        // `DistributeTableBlockSizeToSections`): first to those without a
        // `height` of their own or a row's (in proportion to their heights,
        // to the bodies if all are empty), then to those with a fixed one,
        // then to those with a percentage; those two to the bodies if there
        // are any.
        let content = content
            + sections
                .iter()
                .filter(|s| s.heights.is_empty())
                .map(|s| s.empty_height)
                .sum::<f32>();
        // A row group with a percentage `height` first grows toward it,
        // as far as the table's `height` goes.
        let mut content = content;
        if definite {
            for (section, rows) in grid.sections.iter().zip(sections.iter_mut()) {
                let Size::LengthPercentage(v) = self.boxes[section.index].style.height else {
                    continue;
                };
                if v.px().is_some() {
                    continue;
                }
                let have = section_height(&rows.heights, spacing) + rows.empty_height;
                let grow = (v.resolve(min_height) - have).min(min_height - content);
                if grow <= 0.0 {
                    continue;
                }
                content += grow;
                if rows.heights.is_empty() {
                    rows.empty_height += grow;
                } else {
                    rows.grow(grow);
                }
            }
        }
        if min_height > content && !sections.is_empty() {
            let heights: Vec<f32> = sections
                .iter()
                .map(|s| section_height(&s.heights, spacing) + s.empty_height)
                .collect();
            let tier: Vec<u8> = grid
                .sections
                .iter()
                .zip(&sections)
                .map(
                    |(section, rows)| match self.boxes[section.index].style.height {
                        Size::LengthPercentage(v) if v.px().is_none() => 2,
                        Size::LengthPercentage(_) => 1,
                        _ if rows.constrained.iter().any(|&c| c) => 1,
                        _ => 0,
                    },
                )
                .collect();
            let bodies: Vec<bool> = grid
                .sections
                .iter()
                .map(|s| {
                    self.boxes[s.index].style.display == Display::Table(TableDisplay::RowGroup)
                })
                .collect();
            let lowest = tier.iter().copied().min().unwrap_or(0);
            let in_tier: Vec<usize> = (0..sections.len()).filter(|&i| tier[i] == lowest).collect();
            let takers: Vec<usize> = if lowest > 0 && in_tier.iter().any(|&i| bodies[i]) {
                in_tier.iter().copied().filter(|&i| bodies[i]).collect()
            } else {
                in_tier
            };
            let picked: Vec<f32> = takers.iter().map(|&i| heights[i]).collect();
            let preferred: Vec<bool> = takers.iter().map(|&i| bodies[i]).collect();
            let added = shares(
                &picked,
                &vec![false; picked.len()],
                &preferred,
                min_height - content,
            );
            for (&i, added) in takers.iter().zip(added) {
                let rows = &mut sections[i];
                if rows.heights.is_empty() {
                    rows.empty_height += added;
                } else {
                    rows.grow(added);
                }
            }
        }
        // A collapsed row (`visibility: collapse`, its own or its row
        // group's) is sized as the others, then 0 tall, with no border
        // spacing (css-tables 3; Blink's `is_collapsed`).
        let collapsed: Vec<Vec<bool>> = grid
            .sections
            .iter()
            .map(|section| {
                let group = self.boxes[section.index].style.visibility == Visibility::Collapse;
                section
                    .rows
                    .iter()
                    .map(|row| {
                        group || self.boxes[row.index].style.visibility == Visibility::Collapse
                    })
                    .collect()
            })
            .collect();
        let shown = collapsed
            .iter()
            .zip(&sections)
            .filter(|(c, s)| !s.heights.is_empty() && c.iter().any(|&c| !c))
            .count();
        let mut y = if shown > 0 { spacing } else { 0.0 };
        // Where the grid would end with nothing collapsed.
        let mut y_all = if with_rows > 0 { spacing } else { 0.0 };
        let mut baseline = None;
        for ((section, rows), collapsed) in grid.sections.iter().zip(sections).zip(&collapsed) {
            let SectionRows {
                mut heights,
                baselines,
                lineless,
                cells,
                empty_height,
                ..
            } = rows;
            y_all += section_height(&heights, spacing)
                + empty_height
                + if heights.is_empty() { 0.0 } else { spacing };
            for (height, &collapsed) in heights.iter_mut().zip(collapsed) {
                if collapsed {
                    *height = 0.0;
                }
            }
            let mut row_y = Vec::with_capacity(heights.len());
            let mut ry = 0.0;
            for (&height, &collapsed) in heights.iter().zip(collapsed) {
                row_y.push(ry);
                if !collapsed {
                    ry += height + spacing;
                }
            }
            let shown_rows = collapsed.iter().any(|&c| !c);
            let height = shown_height(&heights, collapsed, spacing) + empty_height;
            if baseline.is_none()
                && let Some((r, b)) = baselines
                    .iter()
                    .enumerate()
                    .find_map(|(r, b)| b.map(|b| (r, b)))
            {
                // A cell without lines: its content box's bottom at its
                // final height (Blink reports the row's baseline from its
                // laid out cells).
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "a cell spans a row at least"
                )]
                let b = lineless[r].map_or(b, |i| {
                    let laid = &cells[i].laid;
                    let layout = &self.boxes[laid.cell.index].layout;
                    heights[slots(r, laid.cell.rowspan)].iter().sum::<f32>()
                        + spacing * (laid.cell.rowspan - 1) as f32
                        - layout.padding.bottom
                        - layout.border.bottom
                });
                baseline = Some(y + row_y[r] + b);
            }
            if let Some((origin, candidates)) = place.as_mut() {
                let mut section_candidates = OofCandidates::new();
                for (r, row) in section.rows.iter().enumerate() {
                    let mut row_candidates = OofCandidates::new();
                    for laid in cells.iter().filter(|c| c.row == r).map(|c| &c.laid) {
                        // One in a collapsed row is 0 tall, whatever it
                        // spans (Blink's `ComputeCellBlockSize`).
                        let span = slots(r, laid.cell.rowspan);
                        let cell_height = if collapsed[r] {
                            0.0
                        } else {
                            shown_height(&heights[span.clone()], &collapsed[span], spacing)
                        };
                        let mut theirs = self.place_cell(
                            laid,
                            placed_span(placed, &laid.cell, left),
                            cell_height,
                            baselines[r],
                            cell_basis,
                            definite,
                        );
                        row_candidates.append(&mut theirs);
                    }
                    let size = taffy::Size {
                        width: row_width,
                        height: heights[r],
                    };
                    let location =
                        self.place_part(row.index, PointF::new(0.0, row_y[r]), size, row_width);
                    self.boxes[row.index].hidden = collapsed[r];
                    let mut unclaimed = self.claim_out_of_flow(row.index, row_candidates, size);
                    unclaimed.translate(location);
                    section_candidates.append(&mut unclaimed);
                }
                let size = taffy::Size {
                    width: row_width,
                    height,
                };
                let location = self.place_part(
                    section.index,
                    PointF::new(origin.x + left, origin.y + y),
                    size,
                    width,
                );
                let mut unclaimed = self.claim_out_of_flow(section.index, section_candidates, size);
                unclaimed.translate(location);
                candidates.append(&mut unclaimed);
            }
            if shown_rows {
                y += height + spacing;
            } else {
                y += height;
            }
        }
        let height = if shown > 0 { y } else { 0.0 };
        let all = if with_rows > 0 { y_all } else { 0.0 };
        GridLayout {
            height,
            collapsed: all - height,
            baseline,
        }
    }

    /// Places cell `laid` in its row at `(x, cell_width)` (narrower than it
    /// was laid out if it spans collapsed columns), `height` tall (its
    /// row's, or its rows'), its content aligned by its `vertical-align`
    /// (`baseline` is its row's), its percentages of `width`; returns the
    /// out-of-flow boxes it passes up, from its row. A narrower one is
    /// measured again at its width and aligned by that (its rows keep the
    /// heights from before). Laid out again at its final size if its
    /// content needs it (Blink lays every cell out at its final size): a
    /// positioned cell, the containing block of the out-of-flow boxes in
    /// it, and one with a child with a percentage height, which is of the
    /// cell's if the cell's height is `definite` (its table's or its own
    /// `height` is set; not its row's, as in Chrome).
    #[allow(clippy::too_many_arguments)]
    fn place_cell(
        &mut self,
        laid: &LaidOutCell,
        (x, cell_width): (f32, f32),
        height: f32,
        baseline: Option<f32>,
        width: f32,
        definite: bool,
    ) -> OofCandidates {
        let index = laid.cell.index;
        let style = self.boxes[index].style.clone();
        let definite = definite || laid.css_height > 0.0;
        let percent_children = definite
            && self.boxes[index].children.iter().any(|&child| {
                let style = &self.boxes[usize::from(child)].style;
                !style.is_out_of_flow()
                    && (matches!(style.height, Size::LengthPercentage(v) if v.has_percent())
                        || matches!(style.min_height,
                            LengthPercentageOrAuto::LengthPercentage(v) if v.has_percent()))
            });
        let narrowed;
        let laid = if cell_width != laid.width {
            narrowed = self.lay_out_cell(laid.cell, cell_width, width);
            &narrowed
        } else {
            laid
        };
        let mut output = None;
        // One that is the containing block of the out-of-flow boxes in it
        // (positioned, or a transform or containment makes it one) lays
        // them out against its final height.
        if (style.is_positioned() || crate::contains_out_of_flow(&style, false) || percent_children)
            && height != laid.content_height
        {
            output = Some(self.compute_child_layout(
                index.into(),
                LayoutInput {
                    known_dimensions: taffy::Size {
                        width: Some(cell_width),
                        height: Some(height),
                    },
                    known_dimensions_are_definite: taffy::Size {
                        width: true,
                        height: percent_children,
                    },
                    parent_size: taffy::Size {
                        width: Some(width),
                        height: None,
                    },
                    available_space: taffy::Size {
                        width: AvailableSpace::Definite(cell_width),
                        height: AvailableSpace::Definite(height),
                    },
                    sizing_mode: SizingMode::ContentSize,
                    run_mode: RunMode::PerformLayout,
                    axis: RequestedAxis::Both,
                    vertical_margins_are_collapsible: taffy::Line::FALSE,
                },
            ));
        }
        let shift = match style.vertical_align {
            VerticalAlign::Top => 0.0,
            VerticalAlign::Middle => (height - laid.content_height) / 2.0,
            VerticalAlign::Bottom => height - laid.content_height,
            // Every other value is `baseline` in a cell.
            _ => baseline.map_or(0.0, |b| b - laid.baseline),
        }
        .max(0.0);
        let by = relative_offset(&style, width, None);
        let location = taffy::Point {
            x: x + by.x,
            y: by.y,
        };
        let edge = Edge {
            margin: computed_style::Sides::all(0.0),
            ..self.cell_edge(index, width)
        };
        self.place(
            index,
            location,
            taffy::Size {
                width: cell_width,
                height,
            },
            &edge,
        );
        self.shift_content(index, shift);
        let mut theirs = match output {
            Some(mut output) => output.oof_candidates.take(),
            None => laid.output.oof_candidates.clone(),
        };
        // The static positions move with the content they follow; in a
        // cell with nothing in flow, they stay at its top (Blink's).
        let b = &self.boxes[index];
        let in_flow = b.inline.as_ref().is_some_and(|inline| inline.has_content())
            || b.children
                .iter()
                .any(|&child| !self.boxes[usize::from(child)].style.is_out_of_flow());
        theirs.translate(taffy::Point {
            x: location.x,
            y: location.y + if in_flow { shift } else { 0.0 },
        });
        theirs
    }

    /// Gives row group or row `index` its place, `at` moved by its own
    /// `position: relative` (in a containing block `width` wide), and its
    /// `size`, and returns the place. Its fragment is made again: its cells
    /// or rows may have moved.
    fn place_part(
        &mut self,
        index: usize,
        at: PointF,
        size: taffy::Size<f32>,
        width: f32,
    ) -> taffy::Point<f32> {
        let by = relative_offset(&self.boxes[index].style, width, None);
        let location = taffy::Point {
            x: at.x + by.x,
            y: at.y + by.y,
        };
        let b = &mut self.boxes[index];
        b.layout = taffy::Layout {
            location,
            size,
            padding: taffy::Rect::zero(),
            border: taffy::Rect::zero(),
            margin: taffy::Rect::zero(),
            ..b.layout
        };
        b.needs_fragment = true;
        location
    }

    /// The heights of `section`'s rows (Blink's `ComputeRowBlockSize`):
    /// each its cells' and its own `height`'s, room for its baseline-
    /// aligned cells on one baseline, and a cell spanning rows given what
    /// it needs beyond them in its last row. With each row's baseline and
    /// its cells laid out, their percentages of `width`.
    fn row_heights(
        &mut self,
        section: &GridSection,
        locations: &[(f32, f32)],
        width: f32,
        spacing: f32,
    ) -> SectionRows {
        let mut heights = Vec::with_capacity(section.rows.len());
        let mut constrained = Vec::with_capacity(section.rows.len());
        let mut baselines = Vec::with_capacity(section.rows.len());
        let mut lineless = Vec::with_capacity(section.rows.len());
        let mut cells = Vec::new();
        for (r, row) in section.rows.iter().enumerate() {
            let row_style = self.boxes[row.index].style.clone();
            let own = match row_style.height {
                Size::LengthPercentage(v) => v.px(),
                _ => None,
            };
            // A fixed height, its own or a cell's, constrains it (Blink's
            // `TableTypes::Row::is_constrained`): it takes more height only
            // when no other row can.
            let mut fixed = own.is_some();
            let mut height = own.unwrap_or(0.0);
            let (mut ascent, mut descent) = (None::<f32>, 0.0f32);
            let (mut with_line, mut first_aligned) = (false, None);
            let first = cells.len();
            for &cell in &row.cells {
                let (_, cell_width) = placed_span(locations, &cell, 0.0);
                let laid = self.lay_out_cell(cell, cell_width, width);
                let needs = laid.content_height.max(laid.css_height);
                let aligned = !matches!(
                    self.boxes[cell.index].style.vertical_align,
                    VerticalAlign::Top | VerticalAlign::Middle | VerticalAlign::Bottom
                );
                // Below the baseline, its content: its `height` makes the
                // row no taller than that (Blink's).
                if aligned {
                    with_line |= laid.has_line;
                    first_aligned.get_or_insert(cells.len());
                    ascent = Some(ascent.unwrap_or(0.0).max(laid.baseline));
                    if cell.rowspan == 1 {
                        descent = descent.max(laid.content_height - laid.baseline);
                    }
                }
                if cell.rowspan == 1 {
                    height = height.max(needs);
                    fixed |= laid.css_height > 0.0;
                }
                cells.push(RowCell { row: r, laid });
            }
            if let Some(ascent) = ascent {
                height = height.max(ascent + descent);
            }
            // A row without baseline-aligned cells has its first cell's
            // baseline.
            let baseline =
                ascent.or_else(|| cells.iter().find(|c| c.row == r).map(|c| c.laid.baseline));
            let from = if ascent.is_some() {
                first_aligned.filter(|_| !with_line)
            } else {
                Some(first).filter(|&i| cells.get(i).is_some_and(|c| !c.laid.has_line))
            };
            lineless.push(from);
            heights.push(height);
            constrained.push(fixed);
            baselines.push(baseline);
        }
        // A cell spanning rows gets what it needs beyond them, the narrower
        // spans first (Blink's `DistributeRowspanCellToRows`): with every
        // row it spans empty, all in the last.
        let mut spanning: Vec<&RowCell> =
            cells.iter().filter(|c| c.laid.cell.rowspan > 1).collect();
        spanning.sort_by_key(|c| c.laid.cell.rowspan);
        for c in spanning {
            let span = c.laid.cell.rowspan;
            let rows = slots(c.row, span);
            let have = section_height(&heights[rows.clone()], spacing);
            let needs = c.laid.content_height.max(c.laid.css_height);
            let last: Vec<bool> = (1..=span).map(|i| i == span).collect();
            let added = shares(
                &heights[rows.clone()],
                &constrained[rows.clone()],
                &last,
                needs - have,
            );
            for (height, added) in heights[rows].iter_mut().zip(added) {
                *height += added;
            }
        }
        SectionRows {
            heights,
            constrained,
            baselines,
            lineless,
            cells,
            empty_height: 0.0,
        }
    }

    /// Lays cell `cell` out `width` wide, its percentages of
    /// `table_width`, as tall as its content.
    fn lay_out_cell(&mut self, cell: GridCell, width: f32, table_width: f32) -> LaidOutCell {
        let index = cell.index;
        let output = self.compute_child_layout(
            index.into(),
            LayoutInput {
                known_dimensions: taffy::Size {
                    width: Some(width),
                    height: None,
                },
                known_dimensions_are_definite: taffy::Size {
                    width: true,
                    height: true,
                },
                parent_size: taffy::Size {
                    width: Some(table_width),
                    height: None,
                },
                available_space: taffy::Size {
                    width: AvailableSpace::Definite(width),
                    height: AvailableSpace::MaxContent,
                },
                // Its `height` is a minimum, which the row applies.
                sizing_mode: SizingMode::ContentSize,
                run_mode: RunMode::PerformLayout,
                axis: RequestedAxis::Both,
                vertical_margins_are_collapsible: taffy::Line::FALSE,
            },
        );
        let style = self.boxes[index].style.clone();
        let edge = self.cell_edge(index, table_width);
        let (p, b) = (edge.padding, edge.border);
        // Its padding and border first: its lines are placed inside them.
        {
            let rect = |s: computed_style::Sides<f32>| taffy::Rect {
                top: s.top,
                right: s.right,
                bottom: s.bottom,
                left: s.left,
            };
            let layout = &mut self.boxes[index].layout;
            layout.padding = rect(p);
            layout.border = rect(b);
            layout.margin = taffy::Rect::zero();
        }
        let content_height = output.size.height;
        let inset_y = p.top + p.bottom + b.top + b.bottom;
        let css_height = match style.height {
            Size::LengthPercentage(v) => v.px().map_or(0.0, |px| match style.box_sizing {
                BoxSizing::ContentBox => px + inset_y,
                BoxSizing::BorderBox => px.max(inset_y),
            }),
            _ => 0.0,
        };
        // Without a line, the bottom of its content box as laid out, not
        // at its `height` (Blink's: the row's baseline isn't pushed down).
        let shift = self.boxes[index].content_shift;
        let line = self.first_line(index).map(|line| line.y - shift);
        let baseline = line.unwrap_or(content_height - p.bottom - b.bottom);
        LaidOutCell {
            cell,
            width,
            content_height,
            css_height,
            baseline,
            has_line: line.is_some(),
            output,
        }
    }

    /// Moves cell `index`'s content down by `shift` from where its layout
    /// put it: its in-flow children by what they aren't moved already, its
    /// lines when its fragment is made.
    pub(crate) fn shift_content(&mut self, index: usize, shift: f32) {
        let delta = shift - self.boxes[index].content_shift;
        if delta == 0.0 {
            return;
        }
        let children = self.boxes[index].children.clone();
        for child in children {
            let b = &mut self.boxes[usize::from(child)];
            if !b.style.is_out_of_flow() {
                b.layout.location.y += delta;
            }
        }
        let b = &mut self.boxes[index];
        b.content_shift = shift;
        b.needs_fragment = true;
    }

    /// Lays out the out-of-flow boxes among `candidates` that row group or
    /// row `index`, `size` big, is the containing block of (a positioned
    /// one), and returns the others.
    fn claim_out_of_flow(
        &mut self,
        index: usize,
        mut candidates: OofCandidates,
        size: taffy::Size<f32>,
    ) -> OofCandidates {
        let children = self.boxes[index].children.clone();
        for (order, &child) in children.iter().enumerate() {
            if self.boxes[usize::from(child)].style.is_out_of_flow() {
                candidates.push(self.static_candidate(index, child, order, PointF::new(0.0, 0.0)));
            }
        }
        let mut output = LayoutOutput::from_outer_size(size);
        output.oof_candidates = candidates;
        output.oof_positioning_area = Some(OofPositioningArea {
            size,
            offset: taffy::Point::ZERO,
        });
        compute_oof_layout(self, index.into(), &mut output);
        output.oof_candidates
    }

    /// Out-of-flow child `child` of table box `parent` with its static
    /// position at `at`, aligned there as taffy's block layout aligns a
    /// block's out-of-flow children.
    fn static_candidate(
        &self,
        parent: usize,
        child: taffy::NodeId,
        order: usize,
        at: PointF,
    ) -> OofCandidate {
        let style = &self.boxes[usize::from(child)].taffy_style;
        let justify_items = self.boxes[parent].taffy_style.justify_items;
        let ltr = taffy::Direction::Ltr;
        let justify = style
            .justify_self
            .unwrap_or(justify_items)
            .resolve_self_relative(ltr, ltr, true);
        let align = style
            .align_self
            .unwrap_or(taffy::AlignItems::NORMAL)
            .resolve_self_relative(ltr, ltr, false);
        let line = |v: f32| taffy::Line { start: v, end: v };
        #[expect(
            clippy::cast_possible_truncation,
            reason = "one per child of the table: fewer than 2^32"
        )]
        let order = order as u32;
        OofCandidate {
            node: child,
            order,
            position: style.position,
            static_position: taffy::Point {
                x: AxisStaticPosition::from_alignment(justify, line(at.x), false),
                y: AxisStaticPosition::from_alignment(align, line(at.y), false),
            },
        }
    }
}

/// A cell laid out in row `row` of its row group.
struct RowCell {
    row: usize,
    laid: LaidOutCell,
}

/// A cell's or column's `width` as a percentage: a `calc()` of one and
/// 0px is the percentage alone, any other `calc()` with one isn't one.
fn percent_width(v: LengthPercentage) -> Option<f32> {
    match v {
        LengthPercentage::Percent(p) => Some(p),
        LengthPercentage::Calc {
            px: 0.0, percent, ..
        } => Some(percent),
        _ => None,
    }
}

/// Where cell `cell` is in its row with the columns at `placed`, whose
/// first starts at `left`: its left edge and width.
fn placed_span(placed: &[(f32, f32)], cell: &GridCell, left: f32) -> (f32, f32) {
    let spanned = &placed[slots(cell.column, cell.colspan)];
    let first = spanned[0];
    // A cell spans a column at least: the first, if no other.
    let last = spanned.last().copied().unwrap_or(first);
    (first.0 - left, last.0 + last.1 - first.0)
}

/// How tall rows of `heights` are together, `spacing` apart.
fn section_height(heights: &[f32], spacing: f32) -> f32 {
    heights.iter().sum::<f32>() + spacing * heights.len().saturating_sub(1) as f32
}

/// How tall rows of `heights` are together, `spacing` apart, but for the
/// `collapsed` ones, which take no room.
fn shown_height(heights: &[f32], collapsed: &[bool], spacing: f32) -> f32 {
    let shown = collapsed.iter().filter(|&&c| !c).count();
    heights.iter().sum::<f32>() + spacing * shown.saturating_sub(1) as f32
}

/// What `extra` (if over 0) adds to each of `heights` (Blink's
/// `DistributeExcessBlockSizeToRows` and
/// `DistributeTableBlockSizeToSections`, but for percentage heights): the
/// unconstrained ones (not `constrained`) take it in proportion to their
/// heights, or evenly among the `preferred` of them if those are all 0;
/// without any unconstrained, the constrained ones the same way.
fn shares(heights: &[f32], constrained: &[bool], preferred: &[bool], extra: f32) -> Vec<f32> {
    let mut out = vec![0.0; heights.len()];
    if extra <= 0.0 || heights.is_empty() {
        return out;
    }
    let mut group: Vec<usize> = (0..heights.len()).filter(|&i| !constrained[i]).collect();
    if group.is_empty() {
        group = (0..heights.len()).collect();
    }
    let total: f32 = group.iter().map(|&i| heights[i]).sum();
    if total > 0.0 {
        for &i in &group {
            out[i] = extra * heights[i] / total;
        }
        return out;
    }
    let picked: Vec<usize> = group.iter().copied().filter(|&i| preferred[i]).collect();
    let picked = if picked.is_empty() { group } else { picked };
    for &i in &picked {
        out[i] = extra / picked.len() as f32;
    }
    out
}

/// A row group's rows as the table lays them out.
struct SectionRows {
    heights: Vec<f32>,
    /// Whether each row has a fixed height (its own or a cell's).
    constrained: Vec<bool>,
    baselines: Vec<Option<f32>>,
    /// The cell (in `cells`) whose content box's bottom is each row's
    /// baseline when no line gives it one: what the table reports, at the
    /// cell's final height.
    lineless: Vec<Option<usize>>,
    cells: Vec<RowCell>,
    /// A row group without rows: the height the table gave it.
    empty_height: f32,
}

impl SectionRows {
    /// Its rows take `extra`, all of them evenly if all are empty.
    fn grow(&mut self, extra: f32) {
        let all = vec![true; self.heights.len()];
        let added = shares(&self.heights, &self.constrained, &all, extra);
        for (height, added) in self.heights.iter_mut().zip(added) {
            *height += added;
        }
    }
}

/// Which side of its source's style an edge of a table's collapsed borders
/// takes (Blink's `TableBorders::EdgeSide`); `DoNotFill`, an edge inside a
/// cell spanning rows or columns, which no other part may fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeSide {
    Top,
    Right,
    Bottom,
    Left,
    DoNotFill,
}

/// One edge of a table's collapsed borders: the style whose border won it,
/// and which of that style's sides (Blink's `TableBorders::Edge`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CollapsedEdge {
    pub(crate) style: Option<Arc<ComputedStyle>>,
    pub(crate) side: EdgeSide,
    /// When its box was visited: an edge of a lower order is painted over
    /// one of a higher (Blink's `box_order`).
    pub(crate) box_order: u32,
}

impl CollapsedEdge {
    pub(crate) fn width(&self) -> f32 {
        self.style
            .as_deref()
            .map_or(0.0, |s| side_width(s, self.side))
    }

    pub(crate) fn border_style(&self) -> BorderStyle {
        self.style
            .as_deref()
            .map_or(BorderStyle::None, |s| side_style(s, self.side))
    }

    /// Whether it has a border to draw (Blink's `CanPaint`).
    pub(crate) fn can_paint(&self) -> bool {
        !matches!(self.border_style(), BorderStyle::None | BorderStyle::Hidden)
            && self.width() > 0.0
    }
}

fn side_width(style: &ComputedStyle, side: EdgeSide) -> f32 {
    match side {
        EdgeSide::Top => style.border_width.top,
        EdgeSide::Right => style.border_width.right,
        EdgeSide::Bottom => style.border_width.bottom,
        EdgeSide::Left => style.border_width.left,
        EdgeSide::DoNotFill => 0.0,
    }
}

/// A side's style as collapsed borders have it: `inset` is `ridge` and
/// `outset` is `groove` there (CSS 2 § 17.6.2.1; Blink's
/// `ComputedStyle::CollapsedBorderStyle`).
fn side_style(style: &ComputedStyle, side: EdgeSide) -> BorderStyle {
    let s = match side {
        EdgeSide::Top => style.border_style.top,
        EdgeSide::Right => style.border_style.right,
        EdgeSide::Bottom => style.border_style.bottom,
        EdgeSide::Left => style.border_style.left,
        EdgeSide::DoNotFill => BorderStyle::None,
    };
    match s {
        BorderStyle::Inset => BorderStyle::Ridge,
        BorderStyle::Outset => BorderStyle::Groove,
        s => s,
    }
}

/// How a style ranks against another of the same width (css-tables 3,
/// "Border specificity"; Blink's `EBorderStyle` order): `double` over
/// `solid` over `dashed`, `dotted`, `ridge`, `outset`, `groove`, `inset`.
fn style_rank(style: BorderStyle) -> u8 {
    match style {
        BorderStyle::None => 0,
        BorderStyle::Hidden => 1,
        BorderStyle::Inset => 2,
        BorderStyle::Groove => 3,
        BorderStyle::Outset => 4,
        BorderStyle::Ridge => 5,
        BorderStyle::Dotted => 6,
        BorderStyle::Dashed => 7,
        BorderStyle::Solid => 8,
        BorderStyle::Double => 9,
    }
}

/// Whether a border of `style` and `width` wins `edge` (css-tables 3,
/// "Border conflict resolution"; Blink's `IsSourceMoreSpecificThanEdge`):
/// `hidden` always does, and nothing wins over it; then the wider; then the
/// higher style. On a tie, the edge keeps the one it has: the cell's over
/// the row's over the row group's, the column's, the column group's and the
/// table's, as they are merged in that order.
fn wins(style: BorderStyle, width: f32, edge: &CollapsedEdge) -> bool {
    if edge.side == EdgeSide::DoNotFill {
        return false;
    }
    if edge.style.is_none() || style == BorderStyle::Hidden {
        return true;
    }
    let edge_style = edge.border_style();
    if edge_style == BorderStyle::Hidden {
        return false;
    }
    let edge_width = edge.width();
    if width != edge_width {
        return width > edge_width;
    }
    style_rank(style) > style_rank(edge_style)
}

/// A table's collapsed borders (css-tables 3, "Border conflict
/// resolution"; Blink's `TableBorders`): for each slot of its grid, the
/// edge on its left and the edge on its top, with one more row and column
/// for the right and bottom edges of the last ones.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableBorders {
    pub(crate) rows: usize,
    pub(crate) columns: usize,
    edges: Vec<CollapsedEdge>,
}

impl TableBorders {
    fn new(rows: usize, columns: usize) -> Self {
        let edge = CollapsedEdge {
            style: None,
            side: EdgeSide::Top,
            box_order: 0,
        };
        // Two edges for each slot and the line after the last row and
        // column: as many as memory holds, or it ends here.
        let len = rows
            .checked_add(1)
            .zip(columns.checked_add(1))
            .and_then(|(rows, columns)| rows.checked_mul(columns)?.checked_mul(2))
            .expect("more collapsed border edges than a usize counts");
        Self {
            rows,
            columns,
            edges: vec![edge; len],
        }
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "row <= rows and column <= columns: within edges, whose length is a usize"
    )]
    fn index(&self, row: usize, column: usize) -> usize {
        (row * (self.columns + 1) + column) * 2
    }

    /// The edge on the left of the slot at `row`, `column`.
    pub(crate) fn left(&self, row: usize, column: usize) -> &CollapsedEdge {
        &self.edges[self.index(row, column)]
    }

    /// The edge on the top of the slot at `row`, `column`.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "an index of a slot's two edges, the second within edges too"
    )]
    pub(crate) fn top(&self, row: usize, column: usize) -> &CollapsedEdge {
        &self.edges[self.index(row, column) + 1]
    }

    /// Takes in the borders of `style`, the box over `rowspan` rows and
    /// `colspan` columns from `row`, `column`, visited `box_order`th (Blink's
    /// `MergeBorders`): each of its edges that it wins. A cell spanning
    /// rows or columns leaves the edges inside it unfilled.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "the spans are cut to the table, so rows and columns end within its own; \
                  an index's second edge is within edges too"
    )]
    fn merge(
        &mut self,
        (row, column): (usize, usize),
        (rowspan, colspan): (usize, usize),
        style: &Arc<ComputedStyle>,
        box_order: u32,
        cell: bool,
    ) {
        let rowspan = rowspan.min(self.rows.saturating_sub(row));
        let colspan = colspan.min(self.columns.saturating_sub(column));
        if rowspan == 0 || colspan == 0 {
            return;
        }
        let take = |tree: &mut Self, index: usize, side: EdgeSide| {
            let (s, width) = (side_style(style, side), side_width(style, side));
            if s != BorderStyle::None && wins(s, width, &tree.edges[index]) {
                tree.edges[index] = CollapsedEdge {
                    style: Some(style.clone()),
                    side,
                    box_order,
                };
            }
        };
        for c in column..column + colspan {
            let top = self.index(row, c) + 1;
            take(self, top, EdgeSide::Top);
            let bottom = self.index(row + rowspan, c) + 1;
            take(self, bottom, EdgeSide::Bottom);
        }
        for r in row..row + rowspan {
            let left = self.index(r, column);
            take(self, left, EdgeSide::Left);
            let right = self.index(r, column + colspan);
            take(self, right, EdgeSide::Right);
        }
        if cell && (rowspan > 1 || colspan > 1) {
            let mut inner = Vec::new();
            for r in row..row + rowspan {
                for c in column + 1..column + colspan {
                    inner.push(self.index(r, c));
                }
            }
            for r in row + 1..row + rowspan {
                for c in column..column + colspan {
                    inner.push(self.index(r, c) + 1);
                }
            }
            for index in inner {
                if self.edges[index].style.is_none() {
                    self.edges[index].side = EdgeSide::DoNotFill;
                }
            }
        }
    }

    /// The border widths of the box over `rowspan` rows and `colspan`
    /// columns from `row`, `column`: half of the widest edge on each side
    /// that has a border to draw (Blink's `GetCellBorders`); the other half
    /// is its neighbor's.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "the spans are cut to the table, so rows and columns end within its own"
    )]
    pub(crate) fn border(
        &self,
        (row, column): (usize, usize),
        (rowspan, colspan): (usize, usize),
    ) -> computed_style::Sides<f32> {
        let rowspan = rowspan.min(self.rows.saturating_sub(row));
        let colspan = colspan.min(self.columns.saturating_sub(column));
        let width = |edge: &CollapsedEdge| if edge.can_paint() { edge.width() } else { 0.0 };
        let mut border = computed_style::Sides::all(0.0f32);
        for r in row..row + rowspan {
            border.left = border.left.max(width(self.left(r, column)));
            border.right = border.right.max(width(self.left(r, column + colspan)));
        }
        for c in column..column + colspan {
            border.top = border.top.max(width(self.top(row, c)));
            border.bottom = border.bottom.max(width(self.top(row + rowspan, c)));
        }
        computed_style::Sides {
            top: border.top / 2.0,
            right: border.right / 2.0,
            bottom: border.bottom / 2.0,
            left: border.left / 2.0,
        }
    }

    /// The table's borders, the half of its outermost edges.
    pub(crate) fn table_border(&self) -> computed_style::Sides<f32> {
        self.border((0, 0), (self.rows, self.columns))
    }
}

impl BoxTree {
    /// Gives cell `index` its collapsed `border` widths (`None`: its
    /// style's): in its taffy style, which lays it out, its cache cleared
    /// when they change (Blink passes them in the constraint space).
    fn set_cell_border(&mut self, index: usize, border: Option<computed_style::Sides<f32>>) {
        let b = &mut self.boxes[index];
        if b.collapsed_border == border {
            return;
        }
        b.collapsed_border = border;
        b.taffy_style.border = border_rect(border.unwrap_or(b.style.border_width));
        b.cache.clear();
    }

    /// Cell `index`'s edges at `width` (what its percentages are of), its
    /// border collapsed if its table's is.
    fn cell_edge(&self, index: usize, width: f32) -> Edge {
        let b = &self.boxes[index];
        let mut edge = Edge::of(&b.style, width);
        if let Some(border) = b.collapsed_border {
            edge.border = border;
        }
        edge
    }

    /// `borders` as paint draws them, `grid`'s, its columns at `locations`
    /// in its content box, which is at `origin` in the table, its row
    /// groups and rows placed (Blink's `CollapsedTableBordersGeometry`).
    fn collapsed_borders(
        &self,
        borders: &TableBorders,
        grid: &Grid,
        locations: &[(f32, f32)],
        origin: PointF,
    ) -> CollapsedBorders {
        // Only room asked for: saturating changes nothing it holds.
        let mut row_offsets = Vec::with_capacity(borders.rows.saturating_add(1));
        for section in &grid.sections {
            let at = self.boxes[section.index].layout.location.y;
            for row in &section.rows {
                let layout = &self.boxes[row.index].layout;
                row_offsets.push(at + layout.location.y);
                if row_offsets.len() == borders.rows {
                    row_offsets.push(at + layout.location.y + layout.size.height);
                }
            }
        }
        let mut column_offsets: Vec<f32> = locations.iter().map(|l| origin.x + l.0).collect();
        if let Some(last) = locations.last() {
            column_offsets.push(origin.x + last.0 + last.1);
        }
        let edges = borders
            .edges
            .iter()
            .map(|edge| {
                let color = edge.style.as_deref().map_or(gfx::Color::TRANSPARENT, |s| {
                    let c = &s.border_color;
                    s.resolve(match edge.side {
                        EdgeSide::Top => c.top,
                        EdgeSide::Right => c.right,
                        EdgeSide::Bottom => c.bottom,
                        EdgeSide::Left | EdgeSide::DoNotFill => c.left,
                    })
                });
                CollapsedBorder {
                    width: edge.width(),
                    style: edge.border_style(),
                    color,
                    box_order: edge.box_order,
                }
            })
            .collect();
        CollapsedBorders {
            rows: borders.rows,
            columns: borders.columns,
            edges,
            row_offsets,
            column_offsets,
        }
    }

    /// `grid`'s collapsed borders, table `index`'s, merged from its cells
    /// (in tree order), rows, row groups, columns, column groups and its own
    /// (Blink's `TableBorders::ComputeTableBorders`).
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "`order` counts the table's boxes, each in memory: fewer than 2^32; \
                  `start` sums its rows"
    )]
    fn table_borders(&self, index: usize, grid: &Grid) -> TableBorders {
        let rows = grid.sections.iter().map(|s| s.rows.len()).sum();
        let columns = grid.starts.len();
        let mut borders = TableBorders::new(rows, columns);
        let mut order = 0;
        let style = |i: usize| self.boxes[i].style.clone();
        let mut start = 0;
        for section in &grid.sections {
            for (r, row) in section.rows.iter().enumerate() {
                for cell in &row.cells {
                    order += 1;
                    borders.merge(
                        (start + r, cell.column),
                        (cell.rowspan, cell.colspan),
                        &style(cell.index),
                        order,
                        true,
                    );
                }
            }
            start += section.rows.len();
        }
        start = 0;
        for section in &grid.sections {
            for (r, row) in section.rows.iter().enumerate() {
                order += 1;
                borders.merge(
                    (start + r, 0),
                    (1, columns),
                    &style(row.index),
                    order,
                    false,
                );
            }
            start += section.rows.len();
        }
        start = 0;
        for section in &grid.sections {
            order += 1;
            let span = (section.rows.len(), columns);
            borders.merge((start, 0), span, &style(section.index), order, false);
            start += section.rows.len();
        }
        // A column's over its group's.
        for group in [false, true] {
            order += 1;
            for column in grid.column_boxes.iter().filter(|c| c.group == group) {
                let span = column.columns.len();
                if group {
                    let at = (0, column.columns.start);
                    borders.merge(at, (rows, span), &style(column.index), order, false);
                } else {
                    for c in column.columns.clone() {
                        borders.merge((0, c), (rows, 1), &style(column.index), order, false);
                    }
                }
            }
        }
        order += 1;
        borders.merge((0, 0), (rows, columns), &style(index), order, false);
        borders
    }
}
