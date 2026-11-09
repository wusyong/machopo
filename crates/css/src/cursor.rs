//! `cursor` (css-ui 4, "Pointing Devices and Keyboards"): parsing,
//! computing and serializing it as Chrome does. machopo shows the keyword: it
//! loads no cursor images.

use std::sync::Arc;

use base::numerics::saturated_cast;
use computed_style::{Cursor, CursorImage, CursorKeyword};
use cssparser::{Parser, match_ignore_ascii_case};

use crate::background::{SpecifiedImage, image_to_css, parse_image_value};
use crate::css_url_data::CssUrlData;
use crate::values::{ParseResult, parse_number};

/// `cursor`'s declared value: the images with their hot spots, and the
/// keyword, with whether it was written with Chrome's `-webkit-` prefix
/// (`-webkit-grab` reads back as written, computes to `grab`).
#[derive(Debug, Clone, PartialEq)]
pub struct SpecifiedCursor {
    pub images: Arc<[SpecifiedCursorImage]>,
    pub keyword: CursorKeyword,
    pub prefixed: bool,
}

/// An image in `cursor`'s list as written: a `url()`, or an `image-set()`
/// of them only (as Chrome takes it), and its hot spot.
pub type SpecifiedCursorImage = (SpecifiedImage, Option<(i32, i32)>);

/// `[<image> [<x> <y>]?,]* <keyword>`.
pub(crate) fn parse<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SpecifiedCursor> {
    let mut images = Vec::new();
    while let Ok(url) = input.try_parse(parse_image) {
        // The hot spot, both numbers or neither: whole numbers, toward 0
        // and within an `i32` (Blink's `ConsumeCursor` keeps an `int`).
        let hot_spot = input
            .try_parse(|i| -> ParseResult<'i, (i32, i32)> {
                let x = parse_number(i)?;
                let y = parse_number(i)?;
                Ok((saturated_cast(x), saturated_cast(y)))
            })
            .ok();
        input.expect_comma()?;
        images.push((url, hot_spot));
    }
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    let (keyword, prefixed) = keyword(ident).ok_or_else(|| location.new_custom_error(()))?;
    Ok(SpecifiedCursor {
        images: images.into(),
        keyword,
        prefixed,
    })
}

fn parse_image<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SpecifiedImage> {
    let location = input.current_source_location();
    let image = parse_image_value(input, true)?;
    if !image.is_urls() {
        return Err(location.new_custom_error(()));
    }
    Ok(image)
}

/// The keyword `ident` names, and whether it is one of Chrome's `-webkit-`
/// aliases.
fn keyword(ident: &str) -> Option<(CursorKeyword, bool)> {
    use CursorKeyword::*;
    Some(match_ignore_ascii_case! { ident,
        "-webkit-grab" => (Grab, true),
        "-webkit-grabbing" => (Grabbing, true),
        "-webkit-zoom-in" => (ZoomIn, true),
        "-webkit-zoom-out" => (ZoomOut, true),
        _ => {
            let keyword = match_ignore_ascii_case! { ident,
                "auto" => Auto,
                "default" => Default,
                "none" => None,
                "context-menu" => ContextMenu,
                "help" => Help,
                "pointer" => Pointer,
                "progress" => Progress,
                "wait" => Wait,
                "cell" => Cell,
                "crosshair" => Crosshair,
                "text" => Text,
                "vertical-text" => VerticalText,
                "alias" => Alias,
                "copy" => Copy,
                "move" => Move,
                "no-drop" => NoDrop,
                "not-allowed" => NotAllowed,
                "grab" => Grab,
                "grabbing" => Grabbing,
                "e-resize" => EResize,
                "n-resize" => NResize,
                "ne-resize" => NeResize,
                "nw-resize" => NwResize,
                "s-resize" => SResize,
                "se-resize" => SeResize,
                "sw-resize" => SwResize,
                "w-resize" => WResize,
                "ew-resize" => EwResize,
                "ns-resize" => NsResize,
                "nesw-resize" => NeswResize,
                "nwse-resize" => NwseResize,
                "col-resize" => ColResize,
                "row-resize" => RowResize,
                "all-scroll" => AllScroll,
                "zoom-in" => ZoomIn,
                "zoom-out" => ZoomOut,
                _ => return Option::None,
            };
            (keyword, false)
        }
    })
}

pub(crate) fn keyword_name(keyword: CursorKeyword) -> &'static str {
    use CursorKeyword::*;
    match keyword {
        Auto => "auto",
        Default => "default",
        None => "none",
        ContextMenu => "context-menu",
        Help => "help",
        Pointer => "pointer",
        Progress => "progress",
        Wait => "wait",
        Cell => "cell",
        Crosshair => "crosshair",
        Text => "text",
        VerticalText => "vertical-text",
        Alias => "alias",
        Copy => "copy",
        Move => "move",
        NoDrop => "no-drop",
        NotAllowed => "not-allowed",
        Grab => "grab",
        Grabbing => "grabbing",
        EResize => "e-resize",
        NResize => "n-resize",
        NeResize => "ne-resize",
        NwResize => "nw-resize",
        SResize => "s-resize",
        SeResize => "se-resize",
        SwResize => "sw-resize",
        WResize => "w-resize",
        EwResize => "ew-resize",
        NsResize => "ns-resize",
        NeswResize => "nesw-resize",
        NwseResize => "nwse-resize",
        ColResize => "col-resize",
        RowResize => "row-resize",
        AllScroll => "all-scroll",
        ZoomIn => "zoom-in",
        ZoomOut => "zoom-out",
    }
}

impl SpecifiedCursor {
    /// Its URLs resolved ([`computed_url`]).
    pub(crate) fn compute(&self) -> Cursor {
        Cursor {
            images: self
                .images
                .iter()
                .map(|(image, hot_spot)| CursorImage {
                    image: image.compute_urls(&computed_url),
                    hot_spot: *hot_spot,
                })
                .collect(),
            keyword: self.keyword,
        }
    }

    pub(crate) fn has_unresolved_urls(&self) -> bool {
        self.images
            .iter()
            .any(|(image, _)| image.has_unresolved_urls())
    }

    pub(crate) fn resolve_urls(&mut self, base: &crate::css_url_data::UrlContext) {
        let mut images = self.images.to_vec();
        for (image, _) in &mut images {
            for url in image.urls_mut() {
                url.resolve(base);
            }
        }
        self.images = images.into();
    }

    pub(crate) fn as_written(&self) -> Self {
        Self {
            images: self
                .images
                .iter()
                .map(|(image, hot_spot)| (image.as_written(), *hot_spot))
                .collect(),
            ..self.clone()
        }
    }

    pub(crate) fn to_css(&self, dest: &mut String) {
        for (image, hot_spot) in self.images.iter() {
            image.to_css(dest);
            hot_spot_to_css(*hot_spot, dest);
            dest.push_str(", ");
        }
        if self.prefixed {
            dest.push_str("-webkit-");
        }
        dest.push_str(keyword_name(self.keyword));
    }
}

fn hot_spot_to_css(hot_spot: Option<(i32, i32)>, dest: &mut String) {
    if let Some((x, y)) = hot_spot {
        dest.push_str(&format!(" {x} {y}"));
    }
}

/// A cursor image's URL resolved, but an empty one and a fragment, which
/// stay as written (Chrome's).
fn computed_url(url: &CssUrlData) -> Arc<str> {
    if url.specified().is_empty() || url.is_local() {
        url.specified().clone()
    } else {
        url.computed()
    }
}

/// The computed value, as `getComputedStyle` has it.
pub(crate) fn to_css(cursor: &Cursor, dest: &mut String) {
    for image in cursor.images.iter() {
        image_to_css(None, &image.image, dest);
        hot_spot_to_css(image.hot_spot, dest);
        dest.push_str(", ");
    }
    dest.push_str(keyword_name(cursor.keyword));
}
