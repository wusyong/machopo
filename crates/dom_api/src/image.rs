//! HTML's `<img>` (https://html.spec.whatwg.org/#the-img-element):
//! `HTMLImageElement`'s members, the `Image()` legacy factory function,
//! and the steps of "update the image data" the DOM leaves to the engine
//! (Blink's `HTMLImageElement`, `ImageLoader`): its microtask
//! (`run_image_updates`) and its tasks, `load` and `error`, and the
//! `decode()` promises settled (`run_image_task`). The DOM keeps the
//! requests (`dom`'s `image`).

use std::cell::Cell;

use base::numerics::clamp_round;
use css::media::{self, MediaEnvironment};
use dom::{
    ImageDecode, ImageRequestState, ImageTaskAction, NodeRef, QualifiedName, TargetRoot, names,
};
use elise::Persistent;
use html::srcset;

use crate::exception::DomException;
use crate::typed::{OpFront, PromiseOf, ScriptHost};
use crate::{ApiCx, ApiError, DomStr, ExceptionName};

/// HTML's microtask of "update the image data", for each `<img>` whose is
/// queued: its source selected (its `src`, or among its `srcset` and its
/// `<picture>`'s `<source>`s: `select_source`), the request made, the
/// image taken from the list of available images or fetched.
pub fn run_image_updates<H: ScriptHost>(host: &mut H) {
    host.mutate(|cx| {
        for element in cx.take_image_updates() {
            let selected = if cx.image_uses_srcset_or_picture(element) {
                select_source(cx, element)
            } else {
                cx.image_src(element)
            };
            cx.run_image_update(element, selected);
        }
    });
}

/// HTML's "react to environment changes" for `document`, whose viewport or
/// device pixel ratio changed: each `<img>` of it that uses `srcset` or a
/// `<picture>` selects again, and updates its image data if it selects
/// another image. The same image at another density only changes its
/// density, as Blink does (`HTMLImageElement::SelectSourceURL`, against
/// HTML, to spare a fetch and an event: whatwg/html#4646).
pub fn react_to_environment_changes<H: ScriptHost>(host: &mut H, document: &dom::NodeRoot) {
    host.mutate(|cx| {
        for element in cx.responsive_images(document.get(cx.gc)) {
            select_again(cx, element);
        }
    });
}

/// `element`, an `<img>` that uses `srcset` or a `<picture>`, selects its
/// source again (its environment, or its width for `sizes="auto"`,
/// changed): another image updates its image data; the same at another
/// density only changes its natural size (Blink's `SelectSourceURL` with
/// `kUpdateSizeChanged`: no fetch, no event).
pub(crate) fn select_again<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) {
    let same = match select_source(cx, element) {
        Some((url, density)) => cx
            .resolve_image_url(element, &url)
            .is_ok_and(|url| cx.set_image_density(element, &url, density)),
        None => cx
            .image_request(element)
            .is_none_or(|(_, url)| url.is_empty()),
    };
    if !same {
        cx.reselect_image(element);
    }
}

/// The image types machopo decodes (`raster`'s) or draws (SVG's), as
/// `<source type>` names them.
const SUPPORTED_TYPES: &[&str] = &[
    "image/png",
    "image/apng",
    "image/jpeg",
    "image/jpg",
    "image/pjpeg",
    "image/gif",
    "image/svg+xml",
];

/// What media queries and `sizes` are evaluated against: `element`'s
/// document's frame's viewport and device pixel ratio (a document no frame
/// shows: none, at 1). A child frame never laid out is laid out first, at
/// its iframe's content box (Blink's child frame has its size as its
/// iframe is inserted); its viewport isn't known before.
fn environment<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> MediaEnvironment {
    let read = |layout: &dyn crate::FrameLayout| {
        let viewport = layout.viewport();
        MediaEnvironment {
            width: viewport.size.width,
            height: viewport.size.height,
            device_pixel_ratio: viewport.device_scale_factor,
        }
    };
    let known = cx.peek_layout(element, |layout| layout.has_viewport());
    match known {
        Some(false) => cx.with_layout(element, |layout| read(layout)),
        _ => cx.peek_layout(element, read),
    }
    .unwrap_or(MediaEnvironment {
        width: 0.0,
        height: 0.0,
        device_pixel_ratio: 1.0,
    })
}

/// HTML's "select an image source": a `<source>` of the `<picture>` parent,
/// before the element, with a `srcset` that has a candidate and whose
/// `media` matches and `type` is one machopo decodes; else the element's own
/// `srcset` and `src`. Of those, the candidate Blink picks for the
/// viewport and device pixel ratio (`html::srcset::pick`, `sizes` the
/// source size, 100vw without one). `sizes="auto"`, where the element
/// allows auto-sizes (lazy): its width (`auto_width`), and its document's
/// observer of those watches it (Blink's `HTMLImageElement::SourceSize`);
/// elsewhere, and not rendered, 100vw (Chrome's: HTML goes on to the next
/// source size). A `<source>` with no `sizes` takes the element's `auto`
/// (Blink's `SourceSizeValue`). The URL as written and its density; `None`
/// if there is none.
fn select_source<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> Option<(String, f64)> {
    let environment = environment(cx, element);
    let allows_auto = cx.allows_auto_sizes(element);
    let auto = Cell::new(false);
    let size = |sizes: Option<String>| match sizes {
        Some(sizes) if media::sizes_start_with_auto(&sizes) => {
            auto.set(allows_auto);
            allows_auto
                .then(|| auto_width(cx, element))
                .flatten()
                .unwrap_or_else(|| media::default_source_size(&environment))
        }
        sizes => sizes
            .and_then(|sizes| media::source_size(&sizes, &environment))
            .unwrap_or_else(|| media::default_source_size(&environment)),
    };
    let selected = select_among(cx, element, &environment, &size, allows_auto);
    cx.observe_auto_sizes(element, auto.get());
    selected
}

/// `select_source`'s choice, `size` the source size of a `sizes`.
fn select_among<'gc>(
    cx: &ApiCx<'_, 'gc>,
    element: NodeRef<'gc>,
    environment: &MediaEnvironment,
    size: &dyn Fn(Option<String>) -> f32,
    allows_auto: bool,
) -> Option<(String, f64)> {
    let cached = |url: &str| {
        cx.resolve_image_url(element, url)
            .is_ok_and(|url| cx.cached_image(&url).is_some())
    };
    let attribute = |node: NodeRef<'gc>, local| {
        cx.attribute_value_by_id(node, local)
            .map(|value| value.borrow().to_str().into_owned())
    };
    let picture = cx.parent(element).filter(|&parent| {
        cx.qualified_name(parent) == Some(QualifiedName::html(names::local::PICTURE))
    });
    if let Some(picture) = picture {
        let mut child = cx.first_child(picture);
        while let Some(source) = child {
            child = cx.next_sibling(source);
            if elise::Gc::ptr_eq(source, element) {
                break;
            }
            if cx.qualified_name(source) != Some(QualifiedName::html(names::local::SOURCE)) {
                continue;
            }
            let Some(srcset) = attribute(source, names::local::SRCSET).filter(|s| !s.is_empty())
            else {
                continue;
            };
            if let Some(query) =
                attribute(source, names::local::MEDIA).filter(|m| !m.trim().is_empty())
                && !media::MediaQueryList::parse(&query).matches(environment)
            {
                continue;
            }
            if let Some(mime) =
                attribute(source, names::local::TYPE).filter(|t| !t.trim().is_empty())
            {
                let essence = mime
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase();
                if !SUPPORTED_TYPES.contains(&essence.as_str()) {
                    continue;
                }
            }
            let candidates = srcset::parse(&srcset);
            if candidates.is_empty() {
                continue;
            }
            // Without its own `sizes`, the `<img>`'s `auto` holds for it.
            let sizes = attribute(source, names::local::SIZES)
                .or_else(|| allows_auto.then(|| "auto".to_owned()));
            let source_size = size(sizes);
            return srcset::pick(
                &candidates,
                None,
                source_size,
                environment.device_pixel_ratio,
                cached,
            );
        }
    }
    let src = attribute(element, names::local::SRC);
    let candidates = attribute(element, names::local::SRCSET)
        .map(|srcset| srcset::parse(&srcset))
        .unwrap_or_default();
    if candidates.is_empty() {
        return src.map(|src| (src, 1.0));
    }
    let source_size = size(attribute(element, names::local::SIZES));
    srcset::pick(
        &candidates,
        src.as_deref(),
        source_size,
        environment.device_pixel_ratio,
        cached,
    )
}

/// The width `sizes="auto"` stands for: the element's concrete object
/// size's, its content box as last laid out, or laid out first if it has
/// no box yet (script set its `srcset` before the first layout: HTML
/// selects a lazy image's source as its load starts, once laid out);
/// `None` if it has none then (it isn't being rendered). One not loaded
/// yet (waiting for its lazy load) takes only the room its style gives, 0
/// without a `width`: HTML's concrete object size is then the default
/// object size's 300px.
fn auto_width<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> Option<f32> {
    let id = cx.node_id(element);
    let element_box = cx
        .peek_layout(element, |layout| layout.element_box(id))
        .flatten()
        .or_else(|| {
            cx.with_layout(element, |layout| layout.element_box(id))
                .flatten()
        })?;
    let (border, padding) = element_box
        .block
        .map_or(([0.0; 4], [0.0; 4]), |b| (b.border, b.padding));
    let laid_out =
        (element_box.border_box.width - border[1] - border[3] - padding[1] - padding[3]).max(0.0);
    let no_size = cx
        .attribute_value_by_id(element, names::local::WIDTH)
        .is_none()
        && cx.image_natural_size(element) == gfx::Size::default();
    Some(if laid_out == 0.0 && no_size {
        300.0
    } else {
        laid_out
    })
}

/// An `<img>`'s task (`dom::ImageTaskAction`): `load` or `error`
/// fired at it (trusted, not bubbling), then its `decode()` promises
/// settled; a `decode()` promise resolved or rejected.
pub(crate) fn run_image_task<H: ScriptHost>(
    host: &mut H,
    element: &Persistent<dom::Node>,
    action: ImageTaskAction,
) {
    match action {
        ImageTaskAction::Event { load, generation } => {
            let Some(event) =
                host.mutate(|cx| cx.take_image_event(element.get(cx.gc), load, generation))
            else {
                return;
            };
            let event_type = if load {
                names::event::LOAD
            } else {
                names::event::ERROR
            };
            if event.fire {
                crate::fire_event(
                    host,
                    &TargetRoot::Node(element.clone()),
                    event_type,
                    false,
                    false,
                );
            }
            let error = (!event.load).then(decoding_error);
            host.settle_promises(&event.decodes, error);
        }
        ImageTaskAction::Decode { promise } => {
            let decodable = host.mutate(|cx| cx.take_ready_decode(element.get(cx.gc), promise));
            host.settle_promises(&[promise], (!decodable).then(decoding_error));
        }
        ImageTaskAction::Reject { promise } => {
            host.settle_promises(&[promise], Some(decoding_error()));
        }
    }
}

/// `decode()`'s rejection: an `EncodingError` (with Blink's message).
fn decoding_error() -> ApiError {
    ApiError::Dom(DomException {
        name: ExceptionName::Encoding,
        message: "The source image cannot be decoded.".to_owned(),
    })
}

// --- HTMLImageElement (https://html.spec.whatwg.org/#htmlimageelement) -------

/// `new Image(width, height)`: an `<img>` of the current window's document,
/// with `width` and `height` set if given.
pub(crate) fn construct_image<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<NodeRef<'gc>, ApiError> {
    let window = op
        .current_window(cx)
        .ok_or_else(|| ApiError::type_error("Illegal constructor"))?;
    let document = cx.window_document(window);
    let image = cx.create_element(document, dom::QualName::html("img"));
    if let Some(width) = width {
        cx.set_attribute(image, "width", &width.to_string());
    }
    if let Some(height) = height {
        cx.set_attribute(image, "height", &height.to_string());
    }
    Ok(image)
}

pub(crate) fn natural_width<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<u32, ApiError> {
    Ok(cx.image_natural_size(this).width)
}

pub(crate) fn natural_height<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<u32, ApiError> {
    Ok(cx.image_natural_size(this).height)
}

/// `complete`: true with neither `src` nor `srcset` (or only an empty
/// `src`), or a current request completely available or broken and no
/// pending request; false while it loads (or its update's microtask is
/// queued).
pub(crate) fn complete<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<bool, ApiError> {
    let src = cx.attribute_value_by_id(this, names::local::SRC);
    let srcset = cx.attribute_value_by_id(this, names::local::SRCSET);
    if srcset.is_none() && src.is_none_or(|src| src.borrow().is_empty()) {
        return Ok(true);
    }
    if cx.image_has_pending_request(this) || cx.image_update_queued(this) {
        return Ok(false);
    }
    Ok(cx
        .image_request(this)
        .is_none_or(|(state, _)| state != ImageRequestState::Unavailable))
}

/// `currentSrc`: the current request's URL once it is available or
/// broken; empty while it loads, as in Chrome (Blink's
/// `HTMLImageElement::currentSrc` reads the image the loader has).
pub(crate) fn current_src<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    let url = cx
        .image_request(this)
        .filter(|(state, _)| *state != ImageRequestState::Unavailable)
        .map(|(_, url)| url)
        .unwrap_or_default();
    Ok(DomStr::Owned(dom::DomString::new(&url)))
}

/// `width` and `height`: the content box's, laid out, for an image being
/// rendered; else the attribute's valid non-negative integer, else the
/// natural size (0 without an image).
fn dimension<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>, width: bool) -> u32 {
    let laid_out = cx
        .with_layout(this, |layout| layout.element_box(cx.node_id(this)))
        .flatten();
    if let Some(element_box) = laid_out {
        let (border, padding) = element_box
            .block
            .map_or(([0.0; 4], [0.0; 4]), |b| (b.border, b.padding));
        let size = if width {
            element_box.border_box.width - border[1] - border[3] - padding[1] - padding[3]
        } else {
            element_box.border_box.height - border[0] - border[2] - padding[0] - padding[2]
        };
        let size: u32 = clamp_round(size.max(0.0));
        return size;
    }
    let local = if width {
        names::local::WIDTH
    } else {
        names::local::HEIGHT
    };
    let attribute = cx
        .attribute_value_by_id(this, local)
        .and_then(|value| crate::reflect::parse_integer(&value.borrow().to_str()))
        .and_then(|value| u32::try_from(value).ok());
    if let Some(value) = attribute {
        return value;
    }
    let natural = cx.image_natural_size(this);
    if width { natural.width } else { natural.height }
}

pub(crate) fn width<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<u32, ApiError> {
    Ok(dimension(cx, this, true))
}

pub(crate) fn height<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<u32, ApiError> {
    Ok(dimension(cx, this, false))
}

/// `decode()`: resolves once the image is completely available (in a
/// task, as `load` fires), rejects with an `EncodingError` if it is
/// broken, there is none, the document isn't fully active, or the current
/// request is replaced first.
pub(crate) fn decode<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<PromiseOf<'gc>, ApiError> {
    match cx.image_decode(this) {
        ImageDecode::Pending(promise) => Ok(PromiseOf::Pending(promise)),
        ImageDecode::Reject => Err(decoding_error()),
    }
}

/// `referrerPolicy`'s keywords (HTML's referrer policy attribute).
const REFERRER_POLICIES: &[&str] = &[
    "",
    "no-referrer",
    "no-referrer-when-downgrade",
    "same-origin",
    "origin",
    "strict-origin",
    "origin-when-cross-origin",
    "strict-origin-when-cross-origin",
    "unsafe-url",
];

macro_rules! enumerated {
    ($get:ident, $local:ident, $keywords:expr, $default:expr) => {
        pub(crate) fn $get<'gc>(
            cx: &ApiCx<'_, 'gc>,
            this: NodeRef<'gc>,
        ) -> Result<DomStr<'gc>, ApiError> {
            Ok(crate::reflect::enumerated(
                cx,
                this,
                names::local::$local,
                $keywords,
                Some($default),
                $default,
            ))
        }
    };
}

enumerated!(referrer_policy, REFERRERPOLICY, REFERRER_POLICIES, "");
enumerated!(decoding, DECODING, &["sync", "async", "auto"], "auto");
enumerated!(loading, LOADING, &["lazy", "eager"], "eager");
enumerated!(
    fetch_priority,
    FETCHPRIORITY,
    &["high", "low", "auto"],
    "auto"
);

/// `crossOrigin`: null without the attribute, else its CORS settings
/// keyword (an invalid value, the empty string too, is `anonymous`).
pub(crate) fn cross_origin<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<dom::DomString>, ApiError> {
    let Some(value) = cx.attribute_value_by_id(this, names::local::CROSSORIGIN) else {
        return Ok(None);
    };
    let keyword = if value
        .borrow()
        .to_str()
        .eq_ignore_ascii_case("use-credentials")
    {
        "use-credentials"
    } else {
        "anonymous"
    };
    Ok(Some(dom::DomString::new(keyword)))
}

/// Setting `crossOrigin`: null removes the attribute.
pub(crate) fn set_cross_origin<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: Option<&str>,
) -> Result<(), ApiError> {
    match value {
        Some(value) => cx.set_attribute(this, "crossorigin", value),
        None => {
            cx.remove_attribute(this, "crossorigin");
        }
    }
    Ok(())
}
