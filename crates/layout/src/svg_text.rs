//! SVG's text (Blink's `LayoutSVGText`, `LayoutSVGInlineText` and
//! `SvgTextLayoutAlgorithm`): a `<text>` and its `<tspan>`s (and `<a>`s)
//! are captured with the `<svg>`: their text, its white space processed
//! across them, shaped as one line, and each addressable character's
//! `x`, `y`, `dx`, `dy` and `rotate` from the innermost element that gives
//! it one. Layout then places each character where those say, in the
//! `<svg>`'s viewport, and each text chunk (from a character with an
//! absolute position to the next) by its `text-anchor`.

use std::sync::Arc;

use computed_style::{ComputedStyle, Display, LengthPercentage, TextAnchor};
use gfx::NodeId;
use gfx::{FontData, PointF, RectF, SizeF};

use crate::svg_dom::{SvgAttr, SvgElement, SvgName};
use crate::svg_gradient::{Capture, SourceServer, SvgPaintServer};
use crate::text::{needs_synthetic_bold, shape_line};

/// A `<text>`'s content, as captured.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SourceText {
    /// The `<text>`, then the elements in it, in tree order.
    elements: Vec<TextElement>,
    /// Each addressable character's position attributes.
    chars: Vec<CharPosition>,
    runs: Vec<SourceRun>,
    /// In logical order.
    clusters: Vec<SourceCluster>,
}

#[derive(Debug, Clone, PartialEq)]
struct TextElement {
    /// As drawn (a copy's, its `<use>`).
    node: NodeId,
    style: Arc<ComputedStyle>,

    servers: [Option<SourceServer>; 2],
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct CharPosition {
    x: Option<LengthPercentage>,
    y: Option<LengthPercentage>,
    dx: Option<LengthPercentage>,
    dy: Option<LengthPercentage>,
    /// Degrees, clockwise.
    rotate: Option<f32>,
}

/// One font's run of the shaped line.
#[derive(Debug, Clone, PartialEq)]
struct SourceRun {
    font: FontData,
    size: f32,
    embolden: bool,
    skew: f32,
    variations: Arc<[gfx::FontVariation]>,
    ascent: f32,
    descent: f32,
}

/// A grapheme cluster: what one position places.
#[derive(Debug, Clone, PartialEq)]
struct SourceCluster {
    run: usize,
    element: usize,
    /// Its first addressable character.
    char_index: usize,
    /// Its characters' UTF-16 code units.
    units: usize,
    advance: f32,
    /// Its glyphs, from its origin.
    glyphs: Vec<(u16, PointF)>,
}

/// The position attributes of one element.
#[derive(Default)]
struct Lists {
    x: Vec<LengthPercentage>,
    y: Vec<LengthPercentage>,
    dx: Vec<LengthPercentage>,
    dy: Vec<LengthPercentage>,
    rotate: Vec<f32>,
}

/// Gathers a `<text>`'s characters (white space processed: SVG 2, as CSS
/// for one line, across element boundaries) and elements.
struct Collector<'c, 't, 'd, 'gc> {
    cx: &'c Capture<'t, 'd, 'gc>,
    shown_as: Option<NodeId>,
    text: String,
    /// Each character's element.
    owners: Vec<usize>,
    elements: Vec<TextElement>,
    parents: Vec<Option<usize>>,
    lists: Vec<Lists>,
    /// A space collapsed into one, kept until something follows it, with
    /// its element.
    pending: Option<usize>,
}

/// CSS's document white space.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

impl SourceText {
    /// `text` (as drawn: in a `<use>`'s copy, `shown_as` that `<use>`),
    /// styled `style`; `None` with no characters.
    pub(crate) fn capture<'d, 'gc>(
        cx: &Capture<'_, 'd, 'gc>,
        text: SvgElement<'d, 'gc>,
        shown_as: Option<NodeId>,
        style: Arc<ComputedStyle>,
    ) -> Option<Self> {
        let mut collector = Collector {
            cx,
            shown_as,
            text: String::new(),
            owners: Vec::new(),
            elements: Vec::new(),
            parents: Vec::new(),
            lists: Vec::new(),
            pending: None,
        };
        collector.element(text, style, None, false);
        let Collector {
            text: content,
            owners,
            elements,
            parents,
            lists,
            ..
        } = collector;
        if owners.is_empty() {
            return None;
        }
        // Each element's characters, in order, and the position lists'
        // values on them; an inner element's win, as it comes later.
        let mut chars = vec![CharPosition::default(); owners.len()];
        let within = |mut e: usize, ancestor: usize| loop {
            if e == ancestor {
                return true;
            }
            match parents[e] {
                Some(parent) => e = parent,
                None => return false,
            }
        };
        for (e, lists) in lists.iter().enumerate() {
            let mine: Vec<usize> = (0..owners.len())
                .filter(|&i| within(owners[i], e))
                .collect();
            for (k, &i) in mine.iter().enumerate() {
                let c = &mut chars[i];
                c.x = lists.x.get(k).copied().or(c.x);
                c.y = lists.y.get(k).copied().or(c.y);
                c.dx = lists.dx.get(k).copied().or(c.dx);
                c.dy = lists.dy.get(k).copied().or(c.dy);
                // Past the list, its last value goes on.
                if let Some(&last) = lists.rotate.last() {
                    c.rotate = Some(lists.rotate.get(k).copied().unwrap_or(last));
                }
            }
        }
        // Shaped as one line, a span for each run of one element's
        // characters.
        let byte_of: Vec<usize> = content.char_indices().map(|(b, _)| b).collect();
        let mut spans: Vec<(std::ops::Range<usize>, usize)> = Vec::new();
        for (i, &owner) in owners.iter().enumerate() {
            #[expect(clippy::arithmetic_side_effects, reason = "i < owners.len(), a Vec's")]
            let end = byte_of.get(i + 1).copied().unwrap_or(content.len());
            match spans.last_mut() {
                Some((range, e)) if *e == owner => range.end = end,
                _ => spans.push((byte_of[i]..end, owner)),
            }
        }
        let styled: Vec<(std::ops::Range<usize>, &ComputedStyle)> = spans
            .iter()
            .map(|(range, e)| (range.clone(), &*elements[*e].style))
            .collect();
        let layout = shape_line(&mut cx.text(), &content, &styled);
        let char_of = |byte: usize| byte_of.partition_point(|&b| b < byte);
        let mut runs = Vec::new();
        let mut clusters = Vec::new();
        for line in layout.lines() {
            for run in line.runs() {
                let font = run.font();
                let metrics = run.font_metrics();
                let synthesis = run.synthesis();
                let mut run_clusters = run.clusters().peekable();
                let Some(first) = run_clusters.peek() else {
                    continue;
                };
                let style = &elements[spans[first.style().brush as usize].1].style;
                let run_index = runs.len();
                runs.push(SourceRun {
                    font: FontData {
                        bytes: Arc::new(font.font.data.clone()),
                        index: font.font.index,
                        id: font.font.data.id(),
                    },
                    size: run.font_size(),
                    embolden: crate::font_features::embolden(
                        &style.font,
                        synthesis.embolden() || needs_synthetic_bold(style.font.weight, &font.font),
                    ),
                    skew: crate::font_features::skew(&style.font, synthesis.skew()),
                    variations: crate::font_features::run_variations(&run, Some(&style.font)),
                    ascent: metrics.ascent,
                    descent: metrics.descent,
                });
                for cluster in run_clusters {
                    let mut x = 0.0;
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "an OpenType glyph id is 16 bits, which parley holds in a u32"
                    )]
                    let glyphs = cluster
                        .glyphs()
                        .map(|g| {
                            let at = PointF::new(x + g.x, g.y);
                            x += g.advance;
                            (g.id as u16, at)
                        })
                        .collect();
                    clusters.push(SourceCluster {
                        run: run_index,
                        element: spans[cluster.style().brush as usize].1,
                        char_index: char_of(cluster.text_range().start),
                        units: content[cluster.text_range()].encode_utf16().count(),
                        advance: cluster.advance(),
                        glyphs,
                    });
                }
            }
        }
        clusters.sort_by_key(|c| c.char_index);
        clusters.dedup_by_key(|c| c.char_index);
        Some(Self {
            elements,
            chars,
            runs,
            clusters,
        })
    }
}

impl<'d, 'gc> Collector<'_, '_, 'd, 'gc> {
    /// `element` (`<text>`, `<tspan>` or `<a>`) and what is in it.
    fn element(
        &mut self,
        element: SvgElement<'d, 'gc>,
        style: Arc<ComputedStyle>,
        parent: Option<usize>,
        preserve: bool,
    ) {
        let index = self.elements.len();
        let node = self.shown_as.unwrap_or(element.id());
        let servers = [
            self.cx.server(&style.svg.fill),
            self.cx.server(&style.svg.stroke),
        ];
        self.elements.push(TextElement {
            node,
            style: style.clone(),
            servers,
        });
        self.parents.push(parent);
        let positioned = matches!(element.svg_name(), Some(SvgName::Text | SvgName::Tspan));
        self.lists.push(if positioned {
            lists(element)
        } else {
            Lists::default()
        });
        // `xml:space` (SVG 1.1's), else `white-space`.
        let preserve = match element.attribute(SvgAttr::XmlSpace).as_deref() {
            Some("preserve") => true,
            Some("default") => false,
            _ => preserve || !style.white_space_collapse.collapses_spaces(),
        };
        for child in element.children() {
            if child.is_text() {
                for c in child.text().unwrap_or_default().chars() {
                    self.push(c, index, preserve);
                }
            } else if child.is_element() {
                if !matches!(child.svg_name(), Some(SvgName::Tspan | SvgName::A)) {
                    continue;
                }
                let Some(child_style) = child.computed_style() else {
                    continue;
                };
                if child_style.display == Display::None || !crate::svg::conditions_hold(child) {
                    continue;
                }
                self.element(child, child_style, Some(index), preserve);
            }
        }
    }

    fn push(&mut self, c: char, element: usize, preserve: bool) {
        if preserve {
            if let Some(owner) = self.pending.take() {
                self.text.push(' ');
                self.owners.push(owner);
            }
            self.text.push(if is_space(c) { ' ' } else { c });
            self.owners.push(element);
            return;
        }
        if is_space(c) {
            // Collapsed into one, and none at the start.
            if !self.owners.is_empty() && self.pending.is_none() {
                self.pending = Some(element);
            }
            return;
        }
        if let Some(owner) = self.pending.take() {
            self.text.push(' ');
            self.owners.push(owner);
        }
        self.text.push(c);
        self.owners.push(element);
    }
}

/// `element`'s `x`, `y`, `dx`, `dy` (lists of lengths) and `rotate` (of
/// numbers); an invalid list is none.
fn lists(element: SvgElement<'_, '_>) -> Lists {
    let lengths = |name: SvgAttr| -> Vec<LengthPercentage> {
        let Some(text) = element.attribute(name) else {
            return Vec::new();
        };
        let font_size = element
            .computed_style()
            .map_or(16.0, |style| style.font.size);
        let mut out = Vec::new();
        for length in svgtypes::LengthListParser::from(text.as_str()) {
            let Ok(length) = length else {
                return Vec::new();
            };
            out.push(crate::svg::length(length, font_size));
        }
        out
    };
    let rotate = element
        .attribute(SvgAttr::Rotate)
        .map(|text| {
            svgtypes::NumberListParser::from(text.as_str())
                .map(|n| n.map(base::numerics::f32_from_f64))
                .collect::<Result<Vec<f32>, _>>()
                .unwrap_or_default()
        })
        .unwrap_or_default();
    Lists {
        x: lengths(SvgAttr::X),
        y: lengths(SvgAttr::Y),
        dx: lengths(SvgAttr::Dx),
        dy: lengths(SvgAttr::Dy),
        rotate,
    }
}

/// A `<text>` laid out: its glyphs, placed.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgText {
    /// In paint order.
    pub runs: Vec<SvgTextRun>,
    /// Each grapheme cluster's cell, in logical order: what hit testing
    /// finds.
    pub cells: Vec<SvgTextCell>,
    /// The cells' bounds, in the `<text>`'s space: its object bounding
    /// box.
    pub bounds: Option<RectF>,
}

/// Glyphs of one font in one element's style.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgTextRun {
    /// The element they are in (or the `<use>` a copy is drawn as).
    pub node: NodeId,
    pub style: Arc<ComputedStyle>,
    pub font: FontData,
    pub font_size: f32,
    pub embolden: bool,
    pub skew: f32,
    pub variations: Arc<[gfx::FontVariation]>,
    pub glyphs: Vec<u16>,
    /// Each glyph's origin, on the baseline.
    pub positions: Vec<PointF>,
    /// Each glyph's turn about its origin, in degrees clockwise; empty if
    /// none turns.
    pub rotations: Vec<f32>,
    /// `stroke-width`, `stroke-dasharray` and `stroke-dashoffset` in user
    /// units.
    pub stroke_width: f32,
    pub dashes: Vec<f32>,
    pub dash_offset: f32,
    pub fill_server: Option<SvgPaintServer>,
    pub stroke_server: Option<SvgPaintServer>,
    /// The cells' bounds, past which only ink goes.
    pub bounds: RectF,
}

/// A character's cell: from its origin along its advance, from its font's
/// ascent above the baseline to its descent below, turned by `rotation`
/// degrees about the origin.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgTextCell {
    pub origin: PointF,
    pub rotation: f32,
    pub advance: f32,
    pub ascent: f32,
    pub descent: f32,
    /// Its characters' UTF-16 code units: how many character numbers
    /// script's text methods give it (`getNumberOfChars()`).
    pub units: usize,
    /// What is found there: the element it is in, innermost (the
    /// `<text>`'s own, `<tspan>`'s, `<a>`'s; or the `<use>` a copy is
    /// drawn as). Only it: `elementsFromPoint` lists an element for its
    /// own text, not for the text of what is in it, as Chrome does.
    pub node: NodeId,
    /// Its element's: what `pointer-events` and `visibility` it goes by.
    pub style: Arc<ComputedStyle>,
}

impl SvgTextCell {
    /// Its corners, in the `<text>`'s space.
    pub fn corners(&self) -> [PointF; 4] {
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let at = |x: f32, y: f32| {
            PointF::new(
                self.origin.x + x * cos - y * sin,
                self.origin.y + x * sin + y * cos,
            )
        };
        [
            at(0.0, -self.ascent),
            at(self.advance, -self.ascent),
            at(self.advance, self.descent),
            at(0.0, self.descent),
        ]
    }

    /// Whether `point`, in the `<text>`'s space, is in it.
    pub fn contains(&self, point: PointF) -> bool {
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let (x, y) = (point.x - self.origin.x, point.y - self.origin.y);
        // Turned back.
        let (x, y) = (x * cos + y * sin, -x * sin + y * cos);
        (0.0..self.advance).contains(&x) && (-self.ascent..self.descent).contains(&y)
    }
}

fn union(a: Option<RectF>, points: &[PointF]) -> Option<RectF> {
    points.iter().fold(a, |r, p| {
        Some(match r {
            None => RectF::new(p.x, p.y, 0.0, 0.0),
            Some(r) => {
                let (x0, y0) = (r.x.min(p.x), r.y.min(p.y));
                let (x1, y1) = (r.right().max(p.x), r.bottom().max(p.y));
                RectF::new(x0, y0, x1 - x0, y1 - y0)
            }
        })
    })
}

impl SourceText {
    /// Placed in a viewport `viewport` big (SVG 2's text layout
    /// algorithm, for one line): each cluster at the current position,
    /// moved to its first character's `x` and `y` and by its `dx` and
    /// `dy`; a character with an `x` or `y` starts a text chunk, which its
    /// first cluster's `text-anchor` places against its start.
    pub(crate) fn lay_out(&self, viewport: SizeF) -> SvgText {
        let resolve = |value: LengthPercentage, base: f32| value.resolve(base);
        let mut pen = PointF::default();
        let mut placed: Vec<PointF> = Vec::with_capacity(self.clusters.len());
        let mut chunks: Vec<usize> = Vec::new();
        for (i, cluster) in self.clusters.iter().enumerate() {
            let c = &self.chars[cluster.char_index];
            let mut starts = i == 0;
            if let Some(x) = c.x {
                pen.x = resolve(x, viewport.width);
                starts = true;
            }
            if let Some(y) = c.y {
                pen.y = resolve(y, viewport.height);
                starts = true;
            }
            if let Some(dx) = c.dx {
                pen.x += resolve(dx, viewport.width);
            }
            if let Some(dy) = c.dy {
                pen.y += resolve(dy, viewport.height);
            }
            if starts {
                chunks.push(i);
            }
            placed.push(pen);
            pen.x += cluster.advance;
        }
        // Each chunk moved by its anchor: by half its advance or all of it.
        for (k, &start) in chunks.iter().enumerate() {
            #[expect(clippy::arithmetic_side_effects, reason = "k < chunks.len(), a Vec's")]
            let end = chunks.get(k + 1).copied().unwrap_or(self.clusters.len());
            let anchor = self.elements[self.clusters[start].element]
                .style
                .svg
                .text_anchor;
            let shift = match anchor {
                TextAnchor::Start => continue,
                TextAnchor::Middle => 0.5,
                TextAnchor::End => 1.0,
            };
            let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
            for (at, cluster) in placed[start..end].iter().zip(&self.clusters[start..end]) {
                min = min.min(at.x);
                max = max.max(at.x + cluster.advance);
            }
            let by = (max - min) * shift;
            for p in &mut placed[start..end] {
                p.x -= by;
            }
        }
        let rotation = |i: usize| {
            self.chars[self.clusters[i].char_index]
                .rotate
                .unwrap_or(0.0)
        };
        let cells: Vec<SvgTextCell> = self
            .clusters
            .iter()
            .enumerate()
            .map(|(i, cluster)| {
                let run = &self.runs[cluster.run];
                let element = &self.elements[cluster.element];
                SvgTextCell {
                    origin: placed[i],
                    rotation: rotation(i),
                    advance: cluster.advance,
                    ascent: run.ascent,
                    descent: run.descent,
                    units: cluster.units,
                    node: element.node,
                    style: element.style.clone(),
                }
            })
            .collect();
        let bounds = cells.iter().fold(None, |r, cell| union(r, &cell.corners()));
        // The runs: clusters next to each other of one font and element.
        let diagonal =
            ((viewport.width * viewport.width + viewport.height * viewport.height) / 2.0).sqrt();
        let mut runs: Vec<SvgTextRun> = Vec::new();
        // Each run's bounds so far.
        let mut run_bounds: Vec<Option<RectF>> = Vec::new();
        let mut key = None;
        for (i, cluster) in self.clusters.iter().enumerate() {
            let source = &self.runs[cluster.run];
            if key != Some((cluster.run, cluster.element)) {
                key = Some((cluster.run, cluster.element));
                let element = &self.elements[cluster.element];
                let style = &element.style;
                let server = |s: &Option<SourceServer>| s.as_ref()?.lay_out(bounds, viewport);
                runs.push(SvgTextRun {
                    node: element.node,
                    style: style.clone(),
                    font: source.font.clone(),
                    font_size: source.size,
                    embolden: source.embolden,
                    skew: source.skew,
                    variations: source.variations.clone(),
                    glyphs: Vec::new(),
                    positions: Vec::new(),
                    rotations: Vec::new(),
                    stroke_width: resolve(style.svg.stroke_width, diagonal).max(0.0),
                    dashes: style
                        .svg
                        .stroke_dasharray
                        .iter()
                        .map(|&dash| resolve(dash, diagonal))
                        .collect(),
                    dash_offset: resolve(style.svg.stroke_dashoffset, diagonal),
                    fill_server: server(&element.servers[0]),
                    stroke_server: server(&element.servers[1]),
                    bounds: RectF::default(),
                });
                run_bounds.push(None);
            }
            let run = runs.last_mut().unwrap();
            let degrees = rotation(i);
            let (sin, cos) = degrees.to_radians().sin_cos();
            for &(glyph, at) in &cluster.glyphs {
                run.glyphs.push(glyph);
                run.positions.push(PointF::new(
                    placed[i].x + at.x * cos - at.y * sin,
                    placed[i].y + at.x * sin + at.y * cos,
                ));
                run.rotations.push(degrees);
            }
            run.bounds = union(run_bounds.pop().flatten(), &cells[i].corners()).unwrap_or_default();
            run_bounds.push(Some(run.bounds));
        }
        for run in &mut runs {
            if run.rotations.iter().all(|&r| r == 0.0) {
                run.rotations.clear();
            }
        }
        SvgText {
            runs,
            cells,
            bounds,
        }
    }
}
