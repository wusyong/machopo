//! The grid properties' values (css-grid 2): track lists with their line
//! names, track sizes, `grid-template-areas`, `grid-auto-flow` and
//! `<grid-line>`.

use std::collections::HashMap;
use std::sync::Arc;

use base::numerics::saturated_cast;
use computed_style::{
    GridArea, GridAutoFlow, GridLine, GridTemplateAreas, GridTrack, LineNames, RepeatCount,
    TrackBreadth, TrackList, TrackSize,
};
use cssparser::{Parser, Token, match_ignore_ascii_case};

use crate::values::{ComputeContext, LengthPercentage, ParseResult, write_number};

/// A track list as declared: lengths may still be in `em`.
pub(crate) type SpecifiedTrackList = TrackList<LengthPercentage>;

/// `none | <track-list> | <auto-track-list>`.
pub(crate) fn parse_track_list<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, SpecifiedTrackList> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(TrackList::none());
    }
    parse_tracks(input, true)
}

/// Tracks and `repeat()`s with the line names around them; an `auto-fill`
/// / `auto-fit` one only if `auto_repeat`.
fn parse_tracks<'i>(
    input: &mut Parser<'i, '_>,
    auto_repeat: bool,
) -> ParseResult<'i, SpecifiedTrackList> {
    let (mut tracks, mut names) = (Vec::new(), Vec::new());
    loop {
        names.push(
            input
                .try_parse(parse_line_names)
                .unwrap_or_else(|_| Arc::new([])),
        );
        let track = input.try_parse(|i| {
            if let Ok(size) = i.try_parse(parse_track_size) {
                return Ok(GridTrack::Size(size));
            }
            parse_repeat(i)
        });
        match track {
            Ok(track) => tracks.push(track),
            Err(e) if tracks.is_empty() => return Err(e),
            Err(_) => break,
        }
    }
    // At most one `auto-fill` / `auto-fit`, and then every other track a
    // fixed size (`<auto-track-list>`).
    let auto = |t: &GridTrack<LengthPercentage>| {
        matches!(
            t,
            GridTrack::Repeat(RepeatCount::AutoFill | RepeatCount::AutoFit, _, _)
        )
    };
    let autos = tracks.iter().filter(|t| auto(t)).count();
    if autos > usize::from(auto_repeat)
        || (autos == 1
            && !tracks.iter().all(|t| match t {
                GridTrack::Size(size) => is_fixed(size),
                GridTrack::Repeat(_, sizes, _) => sizes.iter().all(is_fixed),
            }))
    {
        return Err(input.new_custom_error(()));
    }
    Ok(TrackList {
        tracks: tracks.into(),
        line_names: named_or_none(names),
    })
}

/// `<explicit-track-list>`, `grid-template`'s columns after areas: tracks
/// and line names, without `repeat()`.
pub(crate) fn parse_explicit_track_list<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, SpecifiedTrackList> {
    let list = parse_tracks(input, false)?;
    if list
        .tracks
        .iter()
        .any(|t| matches!(t, GridTrack::Repeat(..)))
    {
        return Err(input.new_custom_error(()));
    }
    Ok(list)
}

/// Each line's names, or none at all if no line has one.
fn named_or_none(names: Vec<LineNames>) -> Arc<[LineNames]> {
    if names.iter().all(|names| names.is_empty()) {
        Arc::new([])
    } else {
        names.into()
    }
}

/// `<line-names>`: `[ <custom-ident>* ]`.
pub(crate) fn parse_line_names<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, LineNames> {
    input.expect_square_bracket_block()?;
    input.parse_nested_block(|i| {
        let mut names = Vec::new();
        while !i.is_exhausted() {
            names.push(parse_custom_ident(i)?);
        }
        Ok(names.into())
    })
}

/// A `<custom-ident>` naming a grid line or area: not a CSS-wide keyword,
/// `default`, nor `span`, which `<grid-line>` takes as itself.
fn parse_custom_ident<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Arc<str>> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?.clone();
    match_ignore_ascii_case! { &ident,
        "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "default" | "span" | "auto" => {
            Err(location.new_custom_error(()))
        },
        _ => Ok(Arc::from(&*ident)),
    }
}

/// `<track-size>+`: `grid-auto-columns` / `-rows`.
pub(crate) fn parse_track_sizes<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Arc<[TrackSize<LengthPercentage>]>> {
    let mut sizes = vec![parse_track_size(input)?];
    while let Ok(size) = input.try_parse(parse_track_size) {
        sizes.push(size);
    }
    Ok(sizes.into())
}

/// `repeat( [ <integer [1,∞]> | auto-fill | auto-fit ] , [ <line-names>?
/// <track-size> ]+ <line-names>? )`.
fn parse_repeat<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, GridTrack<LengthPercentage>> {
    input.expect_function_matching("repeat")?;
    input.parse_nested_block(|i| {
        let location = i.current_source_location();
        let count = match i.next()?.clone() {
            Token::Number {
                int_value: Some(n), ..
            } if n >= 1 => RepeatCount::Count(saturated_cast(n)),
            Token::Ident(ident) => match_ignore_ascii_case! { &ident,
                "auto-fill" => RepeatCount::AutoFill,
                "auto-fit" => RepeatCount::AutoFit,
                _ => return Err(location.new_custom_error(())),
            },
            _ => return Err(location.new_custom_error(())),
        };
        i.expect_comma()?;
        let (mut sizes, mut names) = (Vec::new(), Vec::new());
        loop {
            names.push(
                i.try_parse(parse_line_names)
                    .unwrap_or_else(|_| Arc::new([])),
            );
            match i.try_parse(parse_track_size) {
                Ok(size) => sizes.push(size),
                Err(e) if sizes.is_empty() => return Err(e),
                Err(_) => break,
            }
        }
        // `<auto-repeat>` takes fixed sizes only.
        if matches!(count, RepeatCount::AutoFill | RepeatCount::AutoFit)
            && !sizes.iter().all(is_fixed)
        {
            return Err(i.new_custom_error(()));
        }
        Ok(GridTrack::Repeat(count, sizes.into(), named_or_none(names)))
    })
}

/// `<track-breadth> | minmax( <inflexible-breadth> , <track-breadth> ) |
/// fit-content( <length-percentage [0,∞]> )`.
pub(crate) fn parse_track_size<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, TrackSize<LengthPercentage>> {
    if let Ok(breadth) = input.try_parse(parse_track_breadth) {
        return Ok(TrackSize::Breadth(breadth));
    }
    let location = input.current_source_location();
    let name = match input.next()? {
        Token::Function(name) => name.clone(),
        _ => return Err(location.new_custom_error(())),
    };
    input.parse_nested_block(|i| {
        match_ignore_ascii_case! { &name,
            "minmax" => {
                let min = parse_track_breadth(i)?;
                // The minimum can't be flexible.
                if matches!(min, TrackBreadth::Flex(_)) {
                    return Err(i.new_custom_error(()));
                }
                i.expect_comma()?;
                Ok(TrackSize::MinMax(min, parse_track_breadth(i)?))
            },
            "fit-content" => Ok(TrackSize::FitContent(LengthPercentage::parse(i, false)?)),
            _ => Err(location.new_custom_error(())),
        }
    })
}

/// `<length-percentage [0,∞]> | <flex [0,∞]> | min-content | max-content |
/// auto`.
fn parse_track_breadth<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, TrackBreadth<LengthPercentage>> {
    let keyword: ParseResult<'i, _> = input.try_parse(|i| {
        let location = i.current_source_location();
        match *i.next()? {
            Token::Dimension {
                value, ref unit, ..
            } if unit.eq_ignore_ascii_case("fr") && value >= 0.0 => Ok(TrackBreadth::Flex(value)),
            Token::Ident(ref ident) => Ok(match_ignore_ascii_case! { ident,
                "auto" => TrackBreadth::Auto,
                "min-content" => TrackBreadth::MinContent,
                "max-content" => TrackBreadth::MaxContent,
                _ => return Err(location.new_custom_error(())),
            }),
            _ => Err(location.new_custom_error(())),
        }
    });
    if let Ok(breadth) = keyword {
        return Ok(breadth);
    }
    LengthPercentage::parse(input, false).map(TrackBreadth::LengthPercentage)
}

/// `<fixed-size>`: a track whose size doesn't depend on its content or the
/// free space, which `auto-fill` / `auto-fit` need to count the tracks.
fn is_fixed(size: &TrackSize<LengthPercentage>) -> bool {
    let fixed = |b: &TrackBreadth<LengthPercentage>| matches!(b, TrackBreadth::LengthPercentage(_));
    match size {
        TrackSize::Breadth(b) => fixed(b),
        TrackSize::MinMax(min, max) => {
            fixed(min) || (fixed(max) && !matches!(min, TrackBreadth::Flex(_)))
        }
        TrackSize::FitContent(_) => false,
    }
}

/// `[ row | column ] || dense`.
pub(crate) fn parse_auto_flow<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, GridAutoFlow> {
    let (mut axis, mut dense) = (None, false);
    loop {
        let parsed: ParseResult<'i, ()> = input.try_parse(|i| {
            let location = i.current_source_location();
            let ident = i.expect_ident()?.clone();
            match_ignore_ascii_case! { &ident,
                "row" if axis.is_none() => axis = Some(false),
                "column" if axis.is_none() => axis = Some(true),
                "dense" if !dense => dense = true,
                _ => return Err(location.new_custom_error(())),
            }
            Ok(())
        });
        if parsed.is_err() {
            break;
        }
    }
    Ok(match (axis, dense) {
        (None, false) => return Err(input.new_custom_error(())),
        (Some(false), false) => GridAutoFlow::Row,
        (Some(true), false) => GridAutoFlow::Column,
        (Some(false) | None, true) => GridAutoFlow::RowDense,
        (Some(true), true) => GridAutoFlow::ColumnDense,
    })
}

/// `<grid-line>`: `auto | <custom-ident> | [ <integer> && <custom-ident>? ]
/// | [ span && [ <integer [1,∞]> || <custom-ident> ] ]`; a line number of
/// 0 is invalid.
pub(crate) fn parse_grid_line<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, GridLine> {
    parse_grid_line_and_name(input).map(|(line, _)| line)
}

/// [`parse_grid_line`], with the name if it was only a `<custom-ident>`,
/// which the shorthands give an omitted end too.
pub(crate) fn parse_grid_line_and_name<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, (GridLine, Option<Arc<str>>)> {
    if input.try_parse(|i| i.expect_ident_matching("auto")).is_ok() {
        return Ok((GridLine::Auto, None));
    }
    let location = input.current_source_location();
    let (mut span, mut n, mut name) = (false, None, None);
    for i in 0..3 {
        if !span && input.try_parse(|i| i.expect_ident_matching("span")).is_ok() {
            // `span && [ <integer> || <custom-ident> ]`: the number and the
            // name go together, so `span` can't come between them.
            if i == 1 && n.is_some() != name.is_some() {
                let state = input.state();
                let next = input.next().ok().cloned();
                input.reset(&state);
                if matches!(next, Some(Token::Number { .. } | Token::Ident(_))) {
                    return Err(location.new_custom_error(()));
                }
            }
            span = true;
        } else if let Some(v) = n
            .is_none()
            .then(|| input.try_parse(crate::values::parse_integer).ok())
            .flatten()
        {
            n = Some(v);
        } else if let Some(v) = name
            .is_none()
            .then(|| input.try_parse(parse_custom_ident).ok())
            .flatten()
        {
            name = Some(v);
        } else {
            break;
        }
    }
    Ok(match (span, n, name) {
        (_, None, None) => return Err(location.new_custom_error(())),
        (true, n, name) => {
            let n = n.unwrap_or(1);
            if n < 1 {
                return Err(location.new_custom_error(()));
            }
            // At least 1.
            let n = n.unsigned_abs();
            (
                name.map_or(GridLine::Span(n), |name| GridLine::NamedSpan(name, n)),
                None,
            )
        }
        (false, Some(0), _) => return Err(location.new_custom_error(())),
        (false, Some(n), name) => (
            name.map_or(GridLine::Line(n), |name| GridLine::Named(name, n)),
            None,
        ),
        (false, None, Some(name)) => (GridLine::Ident(name.clone()), Some(name)),
    })
}

/// `none | <string>+`.
pub(crate) fn parse_template_areas<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Option<Arc<GridTemplateAreas>>> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(None);
    }
    let mut rows = Vec::new();
    while let Ok(row) = input.try_parse(|i| i.expect_string().map(|s| s.to_string())) {
        rows.push(row);
    }
    areas_from_strings(&rows)
        .map(Some)
        .ok_or_else(|| input.new_custom_error(()))
}

/// The areas `rows` name (css-grid 2, "grid-template-areas"): `None` if
/// there are no rows, the rows have different numbers of cells, a row
/// holds anything but names and `.`s, or a name's cells don't make a
/// rectangle (or the template has more than `u32::MAX` rows or columns).
#[expect(
    clippy::arithmetic_side_effects,
    reason = "r0 <= r1 and c0 <= c1 are indices of the cells: an area's extent is at most \
              the template's, the product of two at most its cells, and so its count; \
              one past an index fits a usize"
)]
pub(crate) fn areas_from_strings(rows: &[String]) -> Option<Arc<GridTemplateAreas>> {
    let cells: Vec<Vec<Option<&str>>> = rows
        .iter()
        .map(|row| area_cells(row))
        .collect::<Option<_>>()?;
    let columns = cells.first()?.len();
    if columns == 0 || cells.iter().any(|row| row.len() != columns) {
        return None;
    }
    // Grid lines are numbered from 1, past the last cell too.
    let line_after = |cells: usize| u32::try_from(cells).ok()?.checked_add(1);
    line_after(rows.len())?;
    line_after(columns)?;
    // In the order they first come, each with the rows and columns it
    // spans and how many cells it has; found by name (Blink's
    // `NamedGridAreaMap`), as a template can have many.
    let mut areas: Vec<(&str, [usize; 4], usize)> = Vec::new();
    let mut by_name: HashMap<&str, usize> = HashMap::new();
    for (r, row) in cells.iter().enumerate() {
        for (c, name) in row.iter().enumerate() {
            let Some(name) = *name else { continue };
            match by_name.get(name) {
                Some(&index) => {
                    let (_, [r0, r1, c0, c1], count) = &mut areas[index];
                    (*r0, *r1, *c0, *c1) = ((*r0).min(r), (*r1).max(r), (*c0).min(c), (*c1).max(c));
                    *count += 1;
                }
                None => {
                    by_name.insert(name, areas.len());
                    areas.push((name, [r, r, c, c], 1));
                }
            }
        }
    }
    let areas = areas
        .into_iter()
        .map(|(name, [r0, r1, c0, c1], count)| {
            // Not a rectangle: the template is invalid.
            if (r1 - r0 + 1) * (c1 - c0 + 1) != count {
                return None;
            }
            Some(GridArea {
                name: Arc::from(name),
                row_start: line_after(r0)?,
                row_end: line_after(r1 + 1)?,
                column_start: line_after(c0)?,
                column_end: line_after(c1 + 1)?,
            })
        })
        .collect::<Option<Arc<[GridArea]>>>()?;
    Some(Arc::new(GridTemplateAreas {
        areas,
        rows: u32::try_from(rows.len()).ok()?,
        columns: u32::try_from(columns).ok()?,
    }))
}

/// One row's cells: names, and `None` for each run of `.`s; `None` if it
/// holds anything else (a trash token).
fn area_cells(row: &str) -> Option<Vec<Option<&str>>> {
    let name_char = |c: char| c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii();
    let mut cells = Vec::new();
    let mut rest = row.trim_start_matches(|c: char| c.is_ascii_whitespace());
    while let Some(c) = rest.chars().next() {
        let end = if c == '.' {
            cells.push(None);
            rest.find(|c| c != '.').unwrap_or(rest.len())
        } else if name_char(c) {
            let end = rest.find(|c| !name_char(c)).unwrap_or(rest.len());
            cells.push(Some(&rest[..end]));
            end
        } else {
            return None;
        };
        rest = rest[end..].trim_start_matches(|c: char| c.is_ascii_whitespace());
    }
    Some(cells)
}

/// A specified track list, computed.
pub(crate) fn compute_track_list(tracks: &SpecifiedTrackList, cx: &ComputeContext) -> TrackList {
    tracks.map(|v| v.compute(cx))
}

/// Specified track sizes, computed.
pub(crate) fn compute_track_sizes(
    sizes: &[TrackSize<LengthPercentage>],
    cx: &ComputeContext,
) -> Arc<[TrackSize]> {
    sizes
        .iter()
        .map(|size| size.map(|v| v.compute(cx)))
        .collect()
}

/// How a track's length is written: declared (`crate::values`) or
/// computed (`computed_style`), so a track list serializes the same way
/// either side of computing it.
pub(crate) trait LengthToCss {
    fn to_css(&self, dest: &mut String);
}

impl LengthToCss for LengthPercentage {
    fn to_css(&self, dest: &mut String) {
        LengthPercentage::to_css(self, dest);
    }
}

impl LengthToCss for computed_style::LengthPercentage {
    fn to_css(&self, dest: &mut String) {
        match *self {
            computed_style::LengthPercentage::Px(px) => {
                crate::values::write_number(dest, px.into());
                dest.push_str("px");
            }
            computed_style::LengthPercentage::Percent(percent) => {
                crate::values::write_number(dest, percent.into());
                dest.push('%');
            }
            computed_style::LengthPercentage::Calc { px, percent, .. } => {
                crate::calc::write_computed(px, percent, dest);
            }
        }
    }
}

/// A track list: `none`, or its tracks and `repeat()`s with their line
/// names (empty `[]`s left out).
pub(crate) fn track_list_to_css<L: LengthToCss>(list: &TrackList<L>, dest: &mut String) {
    if list.tracks.is_empty() {
        dest.push_str("none");
        return;
    }
    let mut parts = Vec::new();
    for (i, track) in list.tracks.iter().enumerate() {
        line_names_part(list.line_names.get(i), &mut parts);
        let mut part = String::new();
        match track {
            GridTrack::Size(size) => track_size_to_css(size, &mut part),
            GridTrack::Repeat(count, sizes, names) => {
                part.push_str("repeat(");
                match count {
                    RepeatCount::Count(n) => part.push_str(&n.to_string()),
                    RepeatCount::AutoFill => part.push_str("auto-fill"),
                    RepeatCount::AutoFit => part.push_str("auto-fit"),
                }
                part.push_str(", ");
                let mut inner = Vec::new();
                for (j, size) in sizes.iter().enumerate() {
                    line_names_part(names.get(j), &mut inner);
                    let mut size_css = String::new();
                    track_size_to_css(size, &mut size_css);
                    inner.push(size_css);
                }
                line_names_part(names.get(sizes.len()), &mut inner);
                part.push_str(&inner.join(" "));
                part.push(')');
            }
        }
        parts.push(part);
    }
    line_names_part(list.line_names.get(list.tracks.len()), &mut parts);
    dest.push_str(&parts.join(" "));
}

/// `[a b]` as one part, if there are names.
fn line_names_part(names: Option<&LineNames>, parts: &mut Vec<String>) {
    let Some(names) = names.filter(|names| !names.is_empty()) else {
        return;
    };
    let mut part = String::from("[");
    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            part.push(' ');
        }
        cssparser::serialize_identifier(name, &mut part).expect("writing to a String");
    }
    part.push(']');
    parts.push(part);
}

/// `grid-auto-columns` / `-rows`: the sizes separated by spaces.
pub(crate) fn track_sizes_to_css<L: LengthToCss>(sizes: &[TrackSize<L>], dest: &mut String) {
    for (i, size) in sizes.iter().enumerate() {
        if i > 0 {
            dest.push(' ');
        }
        track_size_to_css(size, dest);
    }
}

fn track_size_to_css<L: LengthToCss>(size: &TrackSize<L>, dest: &mut String) {
    match size {
        TrackSize::Breadth(breadth) => track_breadth_to_css(breadth, dest),
        TrackSize::MinMax(min, max) => {
            dest.push_str("minmax(");
            track_breadth_to_css(min, dest);
            dest.push_str(", ");
            track_breadth_to_css(max, dest);
            dest.push(')');
        }
        TrackSize::FitContent(v) => {
            dest.push_str("fit-content(");
            v.to_css(dest);
            dest.push(')');
        }
    }
}

fn track_breadth_to_css<L: LengthToCss>(breadth: &TrackBreadth<L>, dest: &mut String) {
    match breadth {
        TrackBreadth::LengthPercentage(v) => v.to_css(dest),
        TrackBreadth::Flex(v) => {
            write_number(dest, (*v).into());
            dest.push_str("fr");
        }
        TrackBreadth::Auto => dest.push_str("auto"),
        TrackBreadth::MinContent => dest.push_str("min-content"),
        TrackBreadth::MaxContent => dest.push_str("max-content"),
    }
}

/// `grid-template-areas`: `none`, or each row as a string of its cells'
/// names and `.`s separated by single spaces.
pub(crate) fn template_areas_to_css(areas: Option<&GridTemplateAreas>, dest: &mut String) {
    let Some(areas) = areas else {
        dest.push_str("none");
        return;
    };
    for (i, row) in template_area_rows(areas).iter().enumerate() {
        if i > 0 {
            dest.push(' ');
        }
        cssparser::serialize_string(row, dest).expect("writing to a String");
    }
}

/// Each row of the template as its cells' names and `.`s separated by
/// single spaces: each area's name put in its cells, once, rather than each
/// cell looking through the areas.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "lines are numbered from 1, and an area's cells are within the template's \
              rows times columns, which `cells` has"
)]
pub(crate) fn template_area_rows(areas: &GridTemplateAreas) -> Vec<String> {
    let columns = areas.columns as usize;
    let mut cells: Vec<&str> = vec!["."; (areas.rows as usize).saturating_mul(columns)];
    // Lines are numbered from 1.
    let index = |line: u32| line as usize - 1;
    for area in areas.areas.iter() {
        for row in index(area.row_start)..index(area.row_end) {
            for column in index(area.column_start)..index(area.column_end) {
                cells[row * columns + column] = &area.name;
            }
        }
    }
    cells
        .chunks(columns.max(1))
        .map(|row| row.join(" "))
        .collect()
}

pub(crate) fn auto_flow_to_css(flow: GridAutoFlow, dest: &mut String) {
    dest.push_str(match flow {
        GridAutoFlow::Row => "row",
        GridAutoFlow::Column => "column",
        GridAutoFlow::RowDense => "dense",
        GridAutoFlow::ColumnDense => "column dense",
    });
}

/// A `<grid-line>` in canonical order: `span`, the number, the name.
pub(crate) fn grid_line_to_css(line: &GridLine, dest: &mut String) {
    let ident = |name: &str, dest: &mut String| {
        cssparser::serialize_identifier(name, dest).expect("writing to a String");
    };
    match line {
        GridLine::Auto => dest.push_str("auto"),
        GridLine::Line(n) => dest.push_str(&n.to_string()),
        GridLine::Span(n) => dest.push_str(&format!("span {n}")),
        GridLine::Ident(name) => ident(name, dest),
        GridLine::Named(name, n) => {
            dest.push_str(&format!("{n} "));
            ident(name, dest);
        }
        GridLine::NamedSpan(name, 1) => {
            dest.push_str("span ");
            ident(name, dest);
        }
        GridLine::NamedSpan(name, n) => {
            dest.push_str(&format!("span {n} "));
            ident(name, dest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cssparser::ParserInput;

    fn parse<T>(
        css: &str,
        f: impl for<'i, 't> FnOnce(&mut Parser<'i, 't>) -> ParseResult<'i, T>,
    ) -> Option<T> {
        let mut input = ParserInput::new(css);
        let mut parser = Parser::new(&mut input);
        let result = f(&mut parser).ok()?;
        parser.expect_exhausted().ok()?;
        Some(result)
    }

    #[test]
    fn auto_repeat_needs_fixed_sizes_and_only_one() {
        let list = |css| parse(css, parse_track_list).is_some();
        assert!(list("repeat(auto-fill, 100px) 20px minmax(10px, 1fr)"));
        // With an auto repeat, the other tracks are fixed too.
        assert!(!list("repeat(auto-fill, 100px) 1fr"));
        assert!(list("repeat(auto-fit, minmax(100px, 1fr))"));
        assert!(!list("repeat(auto-fit, 1fr)"));
        assert!(!list("repeat(auto-fill, 10px) repeat(auto-fit, 10px)"));
        assert!(list("repeat(2, 1fr) repeat(3, auto)"));
        assert!(!list("repeat(0, 10px)"));
        assert!(!list("minmax(1fr, 10px)"));
    }

    #[test]
    fn line_names() {
        let list = |css| parse(css, parse_track_list).unwrap();
        let names = |names: &[&str]| -> LineNames { names.iter().map(|n| Arc::from(*n)).collect() };
        let l = list("[a] 10px [b c] repeat(2, [d] 1fr [e]) [f]");
        assert_eq!(
            *l.line_names,
            [names(&["a"]), names(&["b", "c"]), names(&["f"])]
        );
        let GridTrack::Repeat(_, _, repeated) = &l.tracks[1] else {
            panic!("{l:?}")
        };
        assert_eq!(**repeated, [names(&["d"]), names(&["e"])]);
        // No names: none at all.
        assert!(list("10px repeat(2, 1fr)").line_names.is_empty());
        assert!(parse("[a] [b] 10px", parse_track_list).is_none());
        assert!(parse("[span] 10px", parse_track_list).is_none());
        assert!(parse("[a]", parse_track_list).is_none());
    }

    #[test]
    fn areas() {
        let areas = |rows: &[&str]| {
            areas_from_strings(&rows.iter().map(|r| r.to_string()).collect::<Vec<_>>())
        };
        let a = areas(&["head head", "nav  main", ".    main"]).unwrap();
        assert_eq!((a.rows, a.columns), (3, 2));
        let area = |name: &str| {
            let a = a.areas.iter().find(|a| &*a.name == name).unwrap();
            (a.row_start, a.row_end, a.column_start, a.column_end)
        };
        assert_eq!(area("head"), (1, 2, 1, 3));
        assert_eq!(area("nav"), (2, 3, 1, 2));
        assert_eq!(area("main"), (2, 4, 2, 3));
        // `...` is one unnamed cell.
        assert_eq!(areas(&["a ... b"]).unwrap().columns, 3);
        // Not a rectangle, rows of different lengths, trash.
        assert!(areas(&["a b", "b a"]).is_none());
        assert!(areas(&["a a", "a"]).is_none());
        assert!(areas(&["a #"]).is_none());
        assert!(areas(&[""]).is_none());
    }

    /// A template keeps as many rows and columns as it has, past 16 bits,
    /// and reads back the same, as Chrome 152's computed value does; many
    /// names take as long as many cells.
    #[test]
    fn big_templates() {
        let row = vec!["a"; 70_000].join(" ");
        let a = areas_from_strings(std::slice::from_ref(&row)).unwrap();
        assert_eq!((a.rows, a.columns), (1, 70_000));
        let area = &a.areas[0];
        assert_eq!(
            (
                area.row_start,
                area.row_end,
                area.column_start,
                area.column_end
            ),
            (1, 2, 1, 70_001)
        );
        assert_eq!(template_area_rows(&a), [row]);

        let names: Vec<String> = (0..300).map(|i| format!("n{i}")).collect();
        let rows: Vec<String> = (0..300).map(|_| names.join(" ")).collect();
        let a = areas_from_strings(&rows).unwrap();
        assert_eq!(a.areas.len(), 300);
        assert_eq!(a.areas[299].column_start, 300);
        assert_eq!(a.areas[299].row_end, 301);
        assert_eq!(template_area_rows(&a), rows);
        let mut out = String::new();
        template_areas_to_css(
            Some(&areas_from_strings(&["a .".into(), "b b".into()]).unwrap()),
            &mut out,
        );
        assert_eq!(out, r#""a ." "b b""#);
    }

    #[test]
    fn grid_lines() {
        let line = |css| parse(css, parse_grid_line);
        assert_eq!(line("auto"), Some(GridLine::Auto));
        assert_eq!(line("-2"), Some(GridLine::Line(-2)));
        assert_eq!(line("span 3"), Some(GridLine::Span(3)));
        assert_eq!(line("3 span"), Some(GridLine::Span(3)));
        assert_eq!(line("0"), None);
        assert_eq!(line("span 0"), None);
        assert_eq!(line("span -1"), None);
        let name = |n: &str| -> Arc<str> { Arc::from(n) };
        assert_eq!(line("a"), Some(GridLine::Ident(name("a"))));
        assert_eq!(line("2 a"), Some(GridLine::Named(name("a"), 2)));
        assert_eq!(line("a -1"), Some(GridLine::Named(name("a"), -1)));
        assert_eq!(line("span a"), Some(GridLine::NamedSpan(name("a"), 1)));
        assert_eq!(line("a 2 span"), Some(GridLine::NamedSpan(name("a"), 2)));
        // The number and the name go together, `span` before or after them.
        for css in ["2 span a", "a span 2", "a span 2 b"] {
            assert_eq!(line(css), None, "{css}");
        }
        assert_eq!(line("a 0"), None);
        assert_eq!(line("span"), None);
        assert_eq!(line("a b"), None);
        assert_eq!(line("inherit 2"), None);
    }

    #[test]
    fn auto_flow() {
        let flow = |css| parse(css, parse_auto_flow);
        assert_eq!(flow("row"), Some(GridAutoFlow::Row));
        assert_eq!(flow("dense"), Some(GridAutoFlow::RowDense));
        assert_eq!(flow("dense column"), Some(GridAutoFlow::ColumnDense));
        assert_eq!(flow("column"), Some(GridAutoFlow::Column));
        assert_eq!(flow("row column"), None);
        assert_eq!(flow("dense dense"), None);
    }
}
