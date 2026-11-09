//! HTML's images (https://html.spec.whatwg.org/#images-processing-model):
//! an `<img>`'s current and pending requests (Blink's `ImageLoader`),
//! "update the image data", and the list of available images, one for the
//! DOM's documents (Blink's `MemoryCache`: by URL, an image still loading
//! counts, so another `<img>` that selects it takes it at once).
//!
//! Updating the image data, when an attribute it reads changes
//! (`image_attribute_changed`) or the element is adopted, does the
//! synchronous part here: an image the list has is the current request at
//! once, `load` queued. Otherwise the rest waits for a microtask
//! (`Microtasks::images`, `run_image_update`): the URL resolved against the
//! node document's then, the request made (the pending one if the current
//! has its image, which it keeps showing till the new one comes: Chrome's
//! too), and the image taken from the list or fetched. The host fetches
//! what `Dom::take_image_fetches` gives and hands each back
//! (`image_fetched`), which settles the requests waiting for it. `load`
//! and `error` fire in tasks of their own (`ImageTaskAction`), and a
//! request delays its node document's load event till its event has
//! fired. An update since makes an older step do nothing (the loader's
//! generation).
//!
//! The DOM keeps the file and its natural size, which the host read from
//! its header; raster decodes it when it draws it. An element in a
//! document no window shows doesn't load (`createHTMLDocument`'s).

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;

use elise::{External, Gc, GcObject, HeapVector, Member, Persistent};

use crate::{Cx, Dom, NodeId, NodeRef, QualifiedName, names, sites};

/// An image request's state (HTML's; Blink has no partially available
/// state for a still image either).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImageRequestState {
    /// Being fetched, or waiting to be.
    #[default]
    Unavailable,
    CompletelyAvailable,
    Broken,
}

/// An image the DOM has: a file the host decodes and its natural size, or
/// an SVG image the host made of one (Blink's `SVGImage`), by its number,
/// which layout and paint ask the host for, with its natural dimensions.
#[derive(Debug, Clone)]
pub enum Image {
    Raster(gfx::ImageData, gfx::Size),
    Svg(u64, gfx::svg::SvgNaturalSize),
}

impl Image {
    /// Its natural size, as `naturalWidth` and `naturalHeight` say it (an
    /// SVG image's, `SvgNaturalSize::dom_size`).
    pub fn size(&self) -> gfx::Size {
        self.size_given().0
    }

    /// Its natural size, and which of its dimensions the image gave (an
    /// SVG image's may be the default object size's), which a density
    /// corrects.
    fn size_given(&self) -> (gfx::Size, [bool; 2]) {
        match self {
            Self::Raster(_, size) => (*size, [true; 2]),
            Self::Svg(_, natural) => natural.dom_size(),
        }
    }
}

/// An image request: its state, its URL (empty for none), its image once
/// available, the density its source has (`srcset`'s, else 1: its natural
/// size is the image's over it), and the `decode()` promises waiting for
/// it to load (their numbers, the engine's to settle).
struct ImageRequest {
    state: ImageRequestState,
    url: String,
    image: Option<Image>,
    density: f64,
    decodes: Vec<u64>,
    /// It waits for its lazy load (HTML's lazy load resumption steps): not
    /// fetched until its element nears the viewport (`resume_lazy_image`).
    deferred: bool,
}

impl Default for ImageRequest {
    fn default() -> Self {
        Self::new(ImageRequestState::default(), String::new(), None)
    }
}

impl ImageRequest {
    fn new(state: ImageRequestState, url: String, image: Option<Image>) -> Self {
        Self {
            state,
            url,
            image,
            density: 1.0,
            decodes: Vec::new(),
            deferred: false,
        }
    }

    /// Its image's natural size over its density, rounded: what layout
    /// sizes its box by (Blink's is fractional; machopo's boxes take whole
    /// pixels of it).
    fn natural_size(&self) -> Option<gfx::Size> {
        self.corrected_size(base::numerics::clamp_round::<u32>)
    }

    /// Its image's natural size over its density, truncated: what
    /// `naturalWidth` and `naturalHeight` give (Blink's
    /// `HTMLImageElement::naturalWidth`, the fractional size made an
    /// integer).
    fn natural_size_truncated(&self) -> Option<gfx::Size> {
        self.corrected_size(base::numerics::clamp_floor::<u32>)
    }

    fn corrected_size(&self, to_integer: fn(f64) -> u32) -> Option<gfx::Size> {
        let (size, [width_given, height_given]) = self.image.as_ref()?.size_given();
        if self.density == 1.0 {
            return Some(size);
        }
        let corrected = |length: u32, given: bool| {
            if given {
                to_integer(f64::from(length) / self.density)
            } else {
                length
            }
        };
        Some(gfx::Size {
            width: corrected(size.width, width_given),
            height: corrected(size.height, height_given),
        })
    }
}

/// An element's requests; the `decode()` promises asked while its update's
/// microtask is queued, for the request it makes (`upcoming`); and those
/// asked when the current request was available, with the current
/// request's count then (`serial`, how many times the current request was
/// replaced: one replaced since rejects them).
#[derive(Default)]
struct Requests {
    current: ImageRequest,
    pending: Option<ImageRequest>,
    upcoming: Vec<u64>,
    ready: Vec<(u64, u64)>,
    serial: u64,
}

impl Requests {
    /// `request` as the current request: the old one's `decode()` promises
    /// waiting for it, returned to reject.
    fn replace_current(&mut self, request: ImageRequest) -> Vec<u64> {
        let old = std::mem::replace(&mut self.current, request);
        self.serial = self.serial.wrapping_add(1);
        old.decodes
    }
}

/// An `<img>`'s requests (Blink's `ImageLoader`), made at its first update.
#[derive(GcObject)]
pub struct ImageLoader {
    /// Taken and put back (a move, no borrow).
    requests: External<Cell<Option<Box<Requests>>>>,
    /// How many times its image data was updated: a microtask, a fetch's
    /// step or a task for an older update does nothing.
    generation: Cell<u64>,
    /// Its microtask is queued (`run_image_update`): one at a time, which
    /// runs for the latest update.
    microtask_queued: Cell<bool>,
    /// The microtask is its lazy load's resumption (`resume_lazy_image`).
    resuming: Cell<bool>,
    /// Its latest update is HTML's "react to environment changes"
    /// (`reselect_image`): one that fails fires no `error`.
    reacting: Cell<bool>,
    /// Its latest update began lazy (its `loading` lazy, a window): its
    /// requests don't delay the load event, though `loading` changed since
    /// (Chrome's `lazy_image_load_state_`, set as the update begins).
    lazy_update: Cell<bool>,
    /// The document whose list of `<img>`s that select again when the
    /// environment changes it is in (`responsive_images`), if any.
    listed_in: Member<crate::Node>,
    /// The document whose load event it delays, if it does.
    delaying: Member<crate::Node>,
}

/// An `<img>` in its document's list of those that select again when the
/// environment changes, held weakly (`Cx::responsive_images`).
#[derive(GcObject)]
pub struct ResponsiveImage {
    element: elise::WeakMember<crate::Node>,
}

/// The list of available images, the DOM's documents' (by URL), and the
/// URLs the host is yet to fetch, each with the document that asked first
/// (whose requests a frame's network idleness counts: Blink's document's
/// `ResourceFetcher` has the request, another document's finds it in the
/// memory cache). A `RefCell`: the DOM's operations reach
/// it through `&Cx`, borrowed within one step, only on an image's path.
#[derive(Default)]
pub(crate) struct ImageCache {
    entries: HashMap<String, CacheEntry>,
    /// The documents that use each URL's image (an `<img>`'s request that
    /// fetched it or took it from the list, a background), by id: an
    /// image no document uses leaves the list (Blink's memory cache is
    /// weak, each document's `ResourceFetcher` keeping what it used till
    /// it shuts down; `release_document_images`). A request that holds
    /// the image keeps it still.
    users: HashMap<String, Vec<NodeId>>,
    fetches: Vec<ImageFetch>,
    /// The URLs loading for a background: their documents' load waits for
    /// them (an element's request delays it by its count instead).
    backgrounds: std::collections::HashSet<String>,
    /// The URLs whose image came from another origin without CORS (an
    /// opaque response): a canvas that draws it isn't origin-clean after.
    opaque: std::collections::HashSet<String>,
}

impl ImageCache {
    /// `document` uses the image at `url` (once).
    fn use_image(&mut self, url: &str, document: NodeId) {
        let documents = self.users.entry(url.to_owned()).or_default();
        if !documents.contains(&document) {
            documents.push(document);
        }
    }
}

enum CacheEntry {
    /// Fetching: the elements whose requests wait for it, by id.
    Loading(Vec<NodeId>),
    Loaded(Image),
}

/// An image the host is to fetch (`Dom::take_image_fetches`): its URL, the
/// document that asked first, and who in it asked (an `<img>`, or a
/// stylesheet for a background).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageFetch {
    pub url: String,
    pub document: NodeId,
    pub initiator: crate::LoadInitiator,
}

/// What an element's image task does (`Cx::queue_image_task`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageTaskAction {
    /// `load` (or `error`) fires for the request of the generation.
    Event { load: bool, generation: u64 },
    /// A `decode()` promise of an image available when it was asked:
    /// resolved if the current request is still the one it was.
    Decode { promise: u64 },
    /// A `decode()` promise rejected (its request was replaced).
    Reject { promise: u64 },
}

/// What an image's event task found (`Cx::take_image_event`): the event to
/// fire (none for an `error` of a reaction to environment changes), and
/// the `decode()` promises to resolve (`load`) or reject.
pub struct ImageEvent {
    pub load: bool,
    pub fire: bool,
    pub decodes: Vec<u64>,
}

/// What `decode()` does (`Cx::image_decode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageDecode {
    /// Its promise, numbered, settled in a task or by the current
    /// request's event.
    Pending(u64),
    /// Rejected now: the element's document isn't fully active.
    Reject,
}

/// What an `<img>` is as a canvas's image source (`drawImage`,
/// `createPattern`; Blink's `GetSourceImageForCanvas`).
pub enum ImageForCanvas {
    /// Nothing to draw yet: no request, its image not loaded, or of no
    /// size; or its image data being updated (Chrome's `ImageComplete()`
    /// is false: an update's microtask queued, or a pending request).
    Nothing,
    /// Its current request is broken: an `InvalidStateError`.
    Broken,
    /// Its image: the file, its natural size (the image's own, not over
    /// the density), its source's density, and whether a canvas that draws
    /// it stays origin-clean.
    Image {
        data: gfx::ImageData,
        size: gfx::Size,
        density: f64,
        origin_clean: bool,
    },
}

/// What layout draws for an element (`Cx::image_state`): an image its
/// natural size over its source's density.
pub enum ImageState {
    Loading,
    Loaded(Image),
    Failed,
}

/// The attributes whose change updates an `<img>`'s image data (HTML's
/// relevant mutations: `srcset` and `sizes` select among sources).
fn is_relevant(local: names::NameId) -> bool {
    matches!(
        local,
        names::local::SRC
            | names::local::SRCSET
            | names::local::SIZES
            | names::local::CROSSORIGIN
            | names::local::REFERRERPOLICY
    )
}

impl Dom {
    /// The URLs the DOM's images wait for, with the document that asked
    /// first, taken: the host fetches each and hands it back
    /// (`Cx::image_fetched`).
    pub fn take_image_fetches(&self) -> Vec<ImageFetch> {
        std::mem::take(&mut self.changes.images.borrow_mut().fetches)
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// Whether `element` is an HTML `<img>`.
    pub fn is_img(&self, element: NodeRef<'gc>) -> bool {
        self.qualified_name(element) == Some(QualifiedName::html(names::local::IMG))
    }

    /// An attribute of `element` named `local` changed (`form::kind`'s
    /// control attributes, which the relevant ones are among): an `<img>`'s,
    /// or a `<source>`'s in a `<picture>` (its `srcset`, `sizes`, `media`,
    /// `type`), whose later `<img>`s select again.
    pub(crate) fn image_attribute_changed(&self, element: NodeRef<'gc>, local: names::NameId) {
        if is_relevant(local) && self.is_img(element) {
            self.update_image_data(element);
        } else if matches!(local, names::local::WIDTH | names::local::HEIGHT)
            && self.is_canvas(element)
        {
            self.canvas_size_changed(element);
        } else if local == names::local::LOADING && self.is_img(element) {
            // An update that hasn't reached its microtask begins as lazy as
            // `loading` is now (the parser sets the attributes one by one;
            // Blink's sets them all before it hears of any).
            if let Some(loader) = self.image_loader(element)
                && loader.microtask_queued.get()
            {
                loader.lazy_update.set(
                    self.is_lazy_image(element)
                        && self.window_of(self.document_of(element)).is_some(),
                );
            }
            // HTML: a `loading` now eager resumes a lazy load that waits.
            if !self.is_lazy_image(element) {
                self.resume_lazy_image(element);
            }
        } else if matches!(
            local,
            names::local::SRCSET | names::local::SIZES | names::local::MEDIA | names::local::TYPE
        ) && self.is_html_named(element, names::local::SOURCE)
            && let Some(parent) = self.parent(element)
            && self.is_html_named(parent, names::local::PICTURE)
        {
            self.update_images_after(self.next_sibling(element));
        }
    }

    /// `node` came into or left `parent`'s children, before `next` (HTML's
    /// relevant mutations of a `<picture>`'s): an `<img>` updates (its
    /// parent changed), a `<source>`'s later `<img>`s do. Nothing for
    /// another parent.
    pub(crate) fn picture_children_changed(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        next: Option<NodeRef<'gc>>,
    ) {
        if !self.is_html_named(parent, names::local::PICTURE) {
            return;
        }
        if self.is_img(node) {
            self.update_image_data(node);
        } else if self.is_html_named(node, names::local::SOURCE) {
            self.update_images_after(next);
        }
    }

    /// The `<img>`s from `first` on among its siblings update.
    fn update_images_after(&self, first: Option<NodeRef<'gc>>) {
        let mut at = first;
        while let Some(node) = at {
            if self.is_img(node) {
                self.update_image_data(node);
            }
            at = self.next_sibling(node);
        }
    }

    /// `element`, an `<img>`, was adopted into another document: its image
    /// data updated (HTML's relevant mutations).
    pub(crate) fn image_adopted(&self, element: NodeRef<'gc>) {
        if self.is_img(element) {
            self.update_image_data(element);
        }
    }

    /// Whether `element`'s `loading` is in the Lazy state.
    pub fn is_lazy_image(&self, element: NodeRef<'gc>) -> bool {
        self.attribute_value_by_id(element, names::local::LOADING)
            .is_some_and(|loading| loading.borrow().to_str().eq_ignore_ascii_case("lazy"))
    }

    /// HTML's "allows auto-sizes": lazy, its `sizes` `auto` or starting
    /// with `auto,` (ASCII case-insensitive; Blink's `AllowAutoSizes`).
    pub fn allows_auto_sizes(&self, element: NodeRef<'gc>) -> bool {
        self.is_lazy_image(element)
            && self
                .attribute_value_by_id(element, names::local::SIZES)
                .is_some_and(|sizes| {
                    let sizes = sizes.borrow().to_str().to_ascii_lowercase();
                    sizes == "auto" || sizes.starts_with("auto,")
                })
    }

    /// HTML's lazy load resumption steps of `element`'s request that waits
    /// for its lazy load, if one does (it neared the viewport, or its
    /// `loading` is eager now): it no longer waits for its document's lazy
    /// load observer, and its update's microtask fetches it.
    pub fn resume_lazy_image(&self, element: NodeRef<'gc>) {
        let Some(loader) = self.image_loader(element) else {
            return;
        };
        let deferred = self.with_requests(loader, |requests| {
            requests.current.deferred
                || requests
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.deferred)
        });
        if !deferred {
            return;
        }
        let document = self.document_of(element);
        if let Some(observer) = self
            .document_data(document)
            .and_then(|data| data.lazy_load_observer.get(self.gc))
        {
            self.unobserve_intersection(observer, element);
        }
        loader.resuming.set(true);
        if !loader.microtask_queued.replace(true) {
            self.queue_image_microtask(element);
        }
    }

    /// Whether `element`'s request waits for its lazy load.
    pub fn image_waits_for_lazy_load(&self, element: NodeRef<'gc>) -> bool {
        self.image_loader(element).is_some_and(|loader| {
            self.with_requests(loader, |requests| {
                requests.current.deferred
                    || requests
                        .pending
                        .as_ref()
                        .is_some_and(|pending| pending.deferred)
            })
        })
    }

    fn image_loader(&self, element: NodeRef<'gc>) -> Option<Gc<'gc, ImageLoader>> {
        self.existing_element_data(element)?.image.get(self.gc)
    }

    fn image_loader_or_new(&self, element: NodeRef<'gc>) -> Gc<'gc, ImageLoader> {
        let data = self.element_data(element);
        if let Some(loader) = data.image.get(self.gc) {
            return loader;
        }
        let loader = self.gc.alloc_in(
            &sites::OTHER,
            ImageLoaderInit {
                requests: External::new(Cell::new(None)),
                generation: Cell::new(0),
                microtask_queued: Cell::new(false),
                resuming: Cell::new(false),
                reacting: Cell::new(false),
                lazy_update: Cell::new(false),
                listed_in: None,
                delaying: None,
            },
        );
        data.image.set(self.gc, Some(loader));
        loader
    }

    /// `loader`'s requests changed by `f`.
    fn with_requests<R>(
        &self,
        loader: Gc<'gc, ImageLoader>,
        f: impl FnOnce(&mut Requests) -> R,
    ) -> R {
        let cell = loader.into_ref().requests.get();
        let mut requests = cell.take().unwrap_or_default();
        let result = f(&mut requests);
        cell.set(Some(requests));
        result
    }

    /// `element`'s `src`, if not empty, at density 1: its source when it
    /// doesn't use `srcset` or `<picture>`.
    pub fn image_src(&self, element: NodeRef<'gc>) -> Option<(String, f64)> {
        let src = self.attribute_value_by_id(element, names::local::SRC)?;
        let src = src.borrow().to_str().into_owned();
        (!src.is_empty()).then_some((src, 1.0))
    }

    /// `url` resolved against `element`'s node document's base URL: `Err`
    /// with it if it doesn't resolve.
    pub fn resolve_image_url(&self, element: NodeRef<'gc>, url: &str) -> Result<String, String> {
        self.resolve_url(self.document_of(element), url)
            .map(String::from)
            .ok_or_else(|| url.to_owned())
    }

    /// `element`'s `src` resolved (`image_src`, `resolve_image_url`): none
    /// without one.
    fn image_source(&self, element: NodeRef<'gc>) -> Option<Result<String, String>> {
        let (src, _) = self.image_src(element)?;
        Some(self.resolve_image_url(element, &src))
    }

    /// Whether `element`, an `<img>`, uses `srcset` or `<picture>` (HTML's):
    /// its source depends on the environment, selected in its update's
    /// microtask.
    pub fn image_uses_srcset_or_picture(&self, element: NodeRef<'gc>) -> bool {
        self.attribute_value_by_id(element, names::local::SRCSET)
            .is_some()
            || self
                .parent(element)
                .is_some_and(|parent| self.is_html_named(parent, names::local::PICTURE))
    }

    /// HTML's "update the image data" for `element` again (its environment
    /// changed: the engine selected another source). As HTML's "react to
    /// environment changes", an image that fails fires no `error` (Chrome's
    /// neither).
    pub fn reselect_image(&self, element: NodeRef<'gc>) {
        self.update_image_data(element);
        if let Some(loader) = self.image_loader(element) {
            loader.reacting.set(true);
        }
    }

    /// `element`'s current request, for `url` (resolved), shows its image at
    /// `density` now: the same image at another density changes only its
    /// natural size (Blink's `HTMLImageElement::SelectSourceURL`; HTML
    /// fetches it again: whatwg/html#4646). Whether it was that request.
    pub fn set_image_density(&self, element: NodeRef<'gc>, url: &str, density: f64) -> bool {
        let Some(loader) = self.image_loader(element) else {
            return false;
        };
        let changed = self.with_requests(loader, |requests| {
            let current = &mut requests.current;
            if current.url != url || requests.pending.is_some() {
                return None;
            }
            Some(std::mem::replace(&mut current.density, density) != density)
        });
        if changed == Some(true) {
            self.set_needs_reattach(element);
        }
        changed.is_some()
    }

    /// `element`, which uses `srcset` or `<picture>`, in its node
    /// document's list of those to select again when the environment
    /// changes (once a document: an adopted one goes in its new document's
    /// too; held weakly: one collected leaves it).
    fn list_responsive_image(&self, loader: Gc<'gc, ImageLoader>, element: NodeRef<'gc>) {
        let document = self.document_of(element);
        if loader
            .listed_in
            .get(self.gc)
            .is_some_and(|listed| Gc::ptr_eq(listed, document))
        {
            return;
        }
        let Some(data) = self.document_data(document) else {
            return;
        };
        loader.listed_in.set(self.gc, Some(document));
        let list = match data.responsive_images.get(self.gc) {
            Some(list) => list,
            None => {
                let list = HeapVector::new(self.gc, 1);
                data.responsive_images.set(self.gc, Some(list));
                list
            }
        };
        let entry = self.gc.alloc_in(
            &sites::OTHER,
            ResponsiveImageInit {
                element: Some(element),
            },
        );
        list.push(self.gc, entry);
        // The collected ones let go of now and then, as `define`'s
        // candidates are.
        if list.len().is_power_of_two() && list.len() >= 16 {
            let mut index = 0;
            while let Some(entry) = list.get(self.gc, index) {
                if entry.element.get(self.gc).is_some() {
                    index = index.saturating_add(1);
                } else {
                    list.remove(self.gc, index);
                }
            }
        }
    }

    /// `document`'s `<img>`s that use `srcset` or `<picture>` and select
    /// their source again when its environment changes (HTML's "react to
    /// environment changes"): those alive, still its, still using them.
    pub fn responsive_images(&self, document: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
        let Some(list) = self
            .document_data(document)
            .and_then(|data| data.responsive_images.get(self.gc))
        else {
            return Vec::new();
        };
        (0..list.len())
            .filter_map(|index| list.get(self.gc, index)?.element.get(self.gc))
            .filter(|&element| {
                Gc::ptr_eq(self.document_of(element), document)
                    && self.image_uses_srcset_or_picture(element)
            })
            .collect()
    }

    /// HTML's "update the image data", up to its microtask: an image the
    /// list of available images has is the current request at once (its
    /// `load` queued); else the microtask is queued, unless the image the
    /// element selects is the one it is fetching already.
    fn update_image_data(&self, element: NodeRef<'gc>) {
        // Script that changed it asked for its load (the request is made in
        // the microtask, where no script runs).
        self.element_asked_for_load(element, false);
        let loader = self.image_loader_or_new(element);
        let generation = loader.generation.get().wrapping_add(1);
        loader.generation.set(generation);
        loader.reacting.set(false);
        loader.lazy_update.set(
            self.is_lazy_image(element) && self.window_of(self.document_of(element)).is_some(),
        );
        // Those asked since an earlier update reject: the request they
        // were for won't be (Chrome rejects them as the `src` changes).
        let stale = self.with_requests(loader, |requests| std::mem::take(&mut requests.upcoming));
        self.reject_decodes(element, &stale);
        // With `srcset` or `<picture>`, the source is selected in the
        // microtask (HTML's synchronous part is for a `src` alone).
        let responsive = self.image_uses_srcset_or_picture(element);
        let selected = if responsive {
            None
        } else {
            self.image_source(element).and_then(Result::ok)
        };
        let available = selected
            .as_ref()
            .and_then(|url| self.take_available_image(url, element));
        if let (Some(url), Some(image)) = (selected.clone(), available) {
            // An update's microtask queued before is done with.
            loader.microtask_queued.set(false);
            let (rejected, ready) = self.with_requests(loader, |requests| {
                // The image it has already: its `decode()` promises stand
                // (Blink keeps its `ImageResourceContent`).
                let same = requests.current.url == url
                    && requests.current.state == ImageRequestState::CompletelyAvailable;
                let mut rejected = requests
                    .pending
                    .take()
                    .map(|pending| pending.decodes)
                    .unwrap_or_default();
                let request =
                    ImageRequest::new(ImageRequestState::CompletelyAvailable, url, Some(image));
                if !same {
                    rejected.extend(requests.replace_current(request));
                }
                // Those asked since the update: of an image available now.
                let serial = requests.serial;
                let upcoming = std::mem::take(&mut requests.upcoming);
                requests
                    .ready
                    .extend(upcoming.iter().map(|&promise| (promise, serial)));
                (rejected, upcoming)
            });
            self.reject_decodes(element, &rejected);
            for promise in ready {
                self.queue_image_task(element, ImageTaskAction::Decode { promise });
            }
            self.set_needs_reattach(element);
            self.set_delaying(loader, element, true);
            self.queue_image_task(
                element,
                ImageTaskAction::Event {
                    load: true,
                    generation,
                },
            );
            return;
        }
        // One the list is loading: waited for at once too (Blink's
        // `ShouldLoadImmediately`: its memory cache has it), the current
        // request this one, its `load` once it comes.
        if let Some(url) = selected.clone().filter(|url| self.is_image_loading(url)) {
            // (A microtask queued before finds this request waiting.)
            let waiting = self.with_requests(loader, |requests| {
                let current = &requests.current;
                current.url == url && current.state == ImageRequestState::Unavailable
            });
            if !waiting {
                let rejected = self.with_requests(loader, |requests| {
                    let mut rejected = requests
                        .pending
                        .take()
                        .map(|pending| pending.decodes)
                        .unwrap_or_default();
                    rejected.extend(requests.replace_current(ImageRequest::new(
                        ImageRequestState::Unavailable,
                        url.clone(),
                        None,
                    )));
                    rejected
                });
                self.reject_decodes(element, &rejected);
                self.wait_for_image(element, url);
                self.set_needs_reattach(element);
            }
            self.set_delaying(loader, element, true);
            return;
        }
        // No source: the image is gone at once, as Blink's loader clears
        // it for an empty URL (the microtask fires `error` for an empty
        // `src`).
        if !responsive && self.image_source(element).is_none() {
            let rejected = self.with_requests(loader, |requests| {
                let mut rejected = requests
                    .pending
                    .take()
                    .map(|pending| pending.decodes)
                    .unwrap_or_default();
                if !requests.current.url.is_empty() || requests.current.image.is_some() {
                    rejected.extend(requests.replace_current(ImageRequest::default()));
                }
                rejected
            });
            self.reject_decodes(element, &rejected);
            self.set_needs_reattach(element);
        }
        // The one it is fetching: it goes on.
        let fetching = self.with_requests(loader, |requests| {
            let waiting = requests.pending.as_ref().unwrap_or(&requests.current);
            waiting.state == ImageRequestState::Unavailable
                && !waiting.url.is_empty()
                && selected.as_ref() == Some(&waiting.url)
        });
        if fetching {
            return;
        }
        if !loader.microtask_queued.replace(true) {
            self.queue_image_microtask(element);
        }
    }

    /// Whether the list of available images is loading `url`.
    fn is_image_loading(&self, url: &str) -> bool {
        matches!(
            self.changes.images.borrow().entries.get(url),
            Some(CacheEntry::Loading(_))
        )
    }

    /// The image the list of available images has for `url`, if loaded.
    fn available_image(&self, url: &str) -> Option<Image> {
        match self.changes.images.borrow().entries.get(url)? {
            CacheEntry::Loaded(image) => Some(image.clone()),
            CacheEntry::Loading(_) => None,
        }
    }

    /// [`available_image`](Self::available_image), taken for `element`'s
    /// request: its document uses it from now on.
    fn take_available_image(&self, url: &str, element: NodeRef<'gc>) -> Option<Image> {
        let image = self.available_image(url)?;
        let document = self.node_id(self.document_of(element));
        self.changes.images.borrow_mut().use_image(url, document);
        Some(image)
    }

    /// `document` shut down (navigated away from, or discarded): the
    /// images it used that no other document does leave the list of
    /// available images (Blink's `ResourceFetcher` letting its resources
    /// go; its memory cache is weak).
    pub fn release_document_images(&self, document: NodeRef<'gc>) {
        let id = self.node_id(document);
        let mut cache = self.changes.images.borrow_mut();
        let ImageCache {
            entries,
            users,
            opaque,
            ..
        } = &mut *cache;
        users.retain(|url, documents| {
            documents.retain(|&user| user != id);
            if !documents.is_empty() {
                return true;
            }
            // One still loading stays: its requests wait for it.
            if matches!(entries.get(url), Some(CacheEntry::Loaded(_))) {
                entries.remove(url);
                opaque.remove(url);
            }
            false
        });
    }

    fn queue_image_microtask(&self, element: NodeRef<'gc>) {
        let updates = &self.changes.image_updates;
        let list = match updates.take() {
            Some(list) => list,
            None => Persistent::new(self.gc, HeapVector::new(self.gc, 1)),
        };
        list.get(self.gc).push(self.gc, element);
        updates.set(Some(list));
        self.changes
            .microtasks
            .set(self.changes.microtasks.get() | crate::Microtasks::IMAGES);
    }

    /// The `<img>`s whose image data's microtask is due, taken (the
    /// engine's microtask runs `run_image_update` for each).
    pub fn take_image_updates(&self) -> Vec<NodeRef<'gc>> {
        let Some(list) = self.changes.image_updates.take() else {
            return Vec::new();
        };
        let updates = list.get(self.gc);
        let taken: Vec<NodeRef<'gc>> = (0..updates.len())
            .filter_map(|index| updates.get(self.gc, index))
            .collect();
        updates.clear(self.gc);
        self.changes.image_updates.set(Some(list));
        taken
    }

    /// The rest of "update the image data", in its microtask, with the
    /// source the engine selected (`selected`: its URL as written and its
    /// density; `image_src` for an element that doesn't use `srcset` or
    /// `<picture>`): none is no image (an error with a `src`, an empty one,
    /// as in Chrome: HTML says `srcset` and `<picture>` are errors too);
    /// one that doesn't resolve is a broken image and `error`; any other is
    /// a new request (the pending one if the current has its image), its
    /// image taken from the list of available images or fetched.
    pub fn run_image_update(&self, element: NodeRef<'gc>, selected: Option<(String, f64)>) {
        let Some(loader) = self.image_loader(element) else {
            return;
        };
        // One its update found an image for at once since does nothing.
        if !loader.microtask_queued.replace(false) {
            return;
        }
        let generation = loader.generation.get();
        let document = self.document_of(element);
        let has_window = self.window_of(document).is_some();
        let resuming = loader.resuming.replace(false);
        if self.image_uses_srcset_or_picture(element) {
            self.list_responsive_image(loader, element);
        }
        // HTML's lazy load resumption steps fetch the URL the update
        // resolved then; one whose source size is its width
        // (`sizes="auto"`) selects again, laid out now.
        let selected = match resuming && !self.allows_auto_sizes(element) {
            true => self
                .with_requests(loader, |requests| {
                    [requests.pending.as_ref(), Some(&requests.current)]
                        .into_iter()
                        .flatten()
                        .find(|request| request.deferred)
                        .map(|request| (request.url.clone(), request.density))
                })
                .or(selected),
            false => selected,
        };
        // HTML's "will lazy load element steps": lazy, scripting enabled
        // (a window).
        let lazy = self.is_lazy_image(element) && has_window;
        let density = selected.as_ref().map_or(1.0, |(_, density)| *density);
        let source = selected.map(|(url, _)| self.resolve_image_url(element, &url));
        match source {
            None | Some(Err(_)) => {
                let rejected = self.with_requests(loader, |requests| {
                    let mut rejected = std::mem::take(&mut requests.upcoming);
                    if let Some(pending) = requests.pending.take() {
                        rejected.extend(pending.decodes);
                    }
                    let url = match &source {
                        Some(Err(src)) => src.clone(),
                        _ => String::new(),
                    };
                    rejected.extend(requests.replace_current(ImageRequest::new(
                        ImageRequestState::Broken,
                        url,
                        None,
                    )));
                    rejected
                });
                self.reject_decodes(element, &rejected);
                self.set_needs_reattach(element);
                // No image is an error with a `src` (an empty one).
                let has_src = self
                    .attribute_value_by_id(element, names::local::SRC)
                    .is_some();
                if has_src && has_window {
                    self.set_delaying(loader, element, true);
                    self.queue_image_task(
                        element,
                        ImageTaskAction::Event {
                            load: false,
                            generation,
                        },
                    );
                } else {
                    self.set_delaying(loader, element, false);
                }
            }
            Some(Ok(url)) => {
                // The pending request is for it already: it takes those
                // asked since. With no window to load it, they reject.
                let (fetching, upcoming) = self.with_requests(loader, |requests| {
                    let upcoming = std::mem::take(&mut requests.upcoming);
                    match requests
                        .pending
                        .as_mut()
                        .filter(|pending| pending.url == url)
                    {
                        Some(pending) if has_window => {
                            pending.decodes.extend(upcoming);
                            // One that waits for its lazy load, resumed:
                            // fetched now.
                            let resumed = resuming && std::mem::take(&mut pending.deferred);
                            (if resumed { Some(true) } else { Some(false) }, Vec::new())
                        }
                        _ => (None, upcoming),
                    }
                });
                match fetching {
                    Some(true) => return self.fetch_request(element, loader, url),
                    Some(false) => return,
                    None => {}
                }
                if !has_window {
                    self.reject_decodes(element, &upcoming);
                    return;
                }
                // The image the current request is waiting for: it goes on,
                // those asked since its.
                let waiting = self.with_requests(loader, |requests| {
                    let current = &mut requests.current;
                    let waiting = current.url == url
                        && current.state == ImageRequestState::Unavailable
                        && requests.pending.is_none();
                    if waiting {
                        current.density = density;
                        current.decodes.extend(upcoming.iter().copied());
                    }
                    (
                        waiting,
                        waiting && resuming && std::mem::take(&mut current.deferred),
                    )
                });
                match waiting {
                    (true, true) => return self.fetch_request(element, loader, url),
                    (true, false) => return,
                    _ => {}
                }
                // The image the current request has: kept (Blink keeps its
                // `ImageResourceContent`, its `decode()`s standing), `load`
                // again, at the density selected now.
                let kept = self.with_requests(loader, |requests| {
                    let current = &mut requests.current;
                    if current.url != url || current.state != ImageRequestState::CompletelyAvailable
                    {
                        return None;
                    }
                    current.density = density;
                    let rejected = requests
                        .pending
                        .take()
                        .map(|pending| pending.decodes)
                        .unwrap_or_default();
                    let serial = requests.serial;
                    requests
                        .ready
                        .extend(upcoming.iter().map(|&promise| (promise, serial)));
                    Some(rejected)
                });
                if let Some(rejected) = kept {
                    self.reject_decodes(element, &rejected);
                    for &promise in &upcoming {
                        self.queue_image_task(element, ImageTaskAction::Decode { promise });
                    }
                    self.set_needs_reattach(element);
                    self.set_delaying(loader, element, true);
                    self.queue_image_task(
                        element,
                        ImageTaskAction::Event {
                            load: true,
                            generation,
                        },
                    );
                    return;
                }
                let mut request =
                    ImageRequest::new(ImageRequestState::Unavailable, url.clone(), None);
                request.density = density;
                request.decodes = upcoming;
                // A lazy one the list of available images doesn't have
                // waits for its element to near the viewport (Chrome's
                // `ShouldLoadImmediately` takes one its memory cache has).
                request.deferred = lazy && !resuming && self.available_image(&url).is_none();
                let deferred = request.deferred;
                // HTML: a current request that has its image keeps it
                // while the new one loads (the pending request).
                let rejected = self.with_requests(loader, |requests| {
                    if requests.current.state == ImageRequestState::CompletelyAvailable {
                        requests
                            .pending
                            .replace(request)
                            .map(|old| old.decodes)
                            .unwrap_or_default()
                    } else {
                        let mut rejected = requests
                            .pending
                            .take()
                            .map(|old| old.decodes)
                            .unwrap_or_default();
                        rejected.extend(requests.replace_current(request));
                        rejected
                    }
                });
                self.reject_decodes(element, &rejected);
                if deferred {
                    // HTML's "start intersection-observing a lazy loading
                    // element".
                    self.set_delaying(loader, element, false);
                    if let Some(observer) = self.lazy_load_observer(document) {
                        self.observe_intersection(observer, element);
                    }
                    self.set_needs_reattach(element);
                    return;
                }
                self.fetch_request(element, loader, url);
            }
        }
    }

    /// `element`'s request for `url` (not one that waits for its lazy
    /// load, or one resumed): taken from the list of available images, or
    /// fetched.
    fn fetch_request(&self, element: NodeRef<'gc>, loader: Gc<'gc, ImageLoader>, url: String) {
        match self.take_available_image(&url, element) {
            Some(image) => self.image_request_done(element, loader, &url, Some(image)),
            None => {
                self.set_delaying(loader, element, true);
                self.wait_for_image(element, url);
                self.set_needs_reattach(element);
            }
        }
    }

    /// `element`'s request waits for `url`: fetched unless it is already.
    fn wait_for_image(&self, element: NodeRef<'gc>, url: String) {
        self.hold_pending_image(element);
        let id = self.node_id(element);
        let document = self.node_id(self.document_of(element));
        let mut cache = self.changes.images.borrow_mut();
        cache.use_image(&url, document);
        match cache.entries.get_mut(&url) {
            Some(CacheEntry::Loading(waiting)) => {
                if !waiting.contains(&id) {
                    waiting.push(id);
                }
            }
            Some(CacheEntry::Loaded(_)) => unreachable!("available images are taken at once"),
            None => {
                cache
                    .entries
                    .insert(url.clone(), CacheEntry::Loading(vec![id]));
                cache.fetches.push(ImageFetch {
                    url,
                    document,
                    initiator: crate::LoadInitiator::Element(id),
                });
            }
        }
    }

    /// The image at `url` for a background (not an element's request) in
    /// `document`, if the list of available images has it; else fetched
    /// (the stylesheet at `referrer` asking), unless it is being already.
    /// Whether it is available.
    pub fn background_image(
        &self,
        url: &str,
        document: NodeId,
        referrer: Option<&Arc<str>>,
    ) -> Option<Image> {
        let mut cache = self.changes.images.borrow_mut();
        cache.use_image(url, document);
        match cache.entries.get(url) {
            Some(CacheEntry::Loaded(image)) => Some(image.clone()),
            Some(CacheEntry::Loading(_)) => {
                cache.backgrounds.insert(url.to_owned());
                None
            }
            None => {
                cache
                    .entries
                    .insert(url.to_owned(), CacheEntry::Loading(Vec::new()));
                cache.fetches.push(ImageFetch {
                    url: url.to_owned(),
                    document,
                    initiator: referrer.map_or(crate::LoadInitiator::Other, |referrer| {
                        crate::LoadInitiator::Stylesheet(referrer.clone())
                    }),
                });
                cache.backgrounds.insert(url.to_owned());
                None
            }
        }
    }

    /// Whether an image a background waits for is loading: what the load
    /// event waits for besides the requests that delay it
    /// (`load_event_delayed`; a lazy `<img>`'s doesn't).
    pub fn background_images_loading(&self) -> bool {
        !self.changes.images.borrow().backgrounds.is_empty()
    }

    /// The image at `url`, if the list of available images has it loaded
    /// (what paint draws for a background).
    pub fn cached_image(&self, url: &str) -> Option<Image> {
        self.available_image(url)
    }

    /// The host fetched `url` (`Dom::take_image_fetches`): its image, or
    /// none for one that failed to load or decode, which leaves the list
    /// (Blink drops an errored resource from its memory cache); whether it
    /// is `origin_clean` (not another origin's without CORS). The requests
    /// waiting for it are done.
    pub fn image_fetched(&self, url: &str, image: Option<Image>, origin_clean: bool) {
        let waiting = {
            let mut cache = self.changes.images.borrow_mut();
            let waiting = match cache.entries.remove(url) {
                Some(CacheEntry::Loading(waiting)) => waiting,
                _ => Vec::new(),
            };
            cache.backgrounds.remove(url);
            // Kept while a document uses it (a failed one leaves the list).
            let used = image.is_some() && cache.users.contains_key(url);
            if !used {
                cache.users.remove(url);
            }
            if used && !origin_clean {
                cache.opaque.insert(url.to_owned());
            } else {
                cache.opaque.remove(url);
            }
            if let Some(image) = image.as_ref().filter(|_| used) {
                cache
                    .entries
                    .insert(url.to_owned(), CacheEntry::Loaded(image.clone()));
            }
            waiting
        };
        for id in waiting {
            let Some(element) = self.node_by_id(id) else {
                continue;
            };
            // Its task (queued now, if its request is still for `url`)
            // holds it from here; unless another of its requests still
            // waits (its `src` changed meanwhile).
            self.let_go_pending_image(element);
            if let Some(loader) = self.image_loader(element) {
                self.image_request_done(element, loader, url, image.clone());
                let waiting = self.with_requests(loader, |requests| {
                    let waits = |request: &ImageRequest| {
                        !request.url.is_empty()
                            && !request.deferred
                            && request.state == ImageRequestState::Unavailable
                    };
                    waits(&requests.current) || requests.pending.as_ref().is_some_and(waits)
                });
                if waiting {
                    self.hold_pending_image(element);
                }
            }
        }
    }

    /// `element`'s request for `url` (the pending one, or the current) got
    /// its image, or failed: it is the current request (a pending one
    /// replaces the current), its event queued. Nothing if neither is for
    /// `url` and waiting by now.
    fn image_request_done(
        &self,
        element: NodeRef<'gc>,
        loader: Gc<'gc, ImageLoader>,
        url: &str,
        image: Option<Image>,
    ) {
        let load = image.is_some();
        let done = self.with_requests(loader, |requests| {
            let waiting = |request: &ImageRequest| {
                request.url == url && request.state == ImageRequestState::Unavailable
            };
            let finished = |request: &mut ImageRequest| {
                request.state = if load {
                    ImageRequestState::CompletelyAvailable
                } else {
                    ImageRequestState::Broken
                };
                request.image = image.clone();
            };
            if requests.pending.as_ref().is_some_and(waiting) {
                // HTML's "upgrade the pending request to the current
                // request": the old one's `decode()` promises reject.
                let mut pending = requests.pending.take().expect("checked above");
                finished(&mut pending);
                Some(requests.replace_current(pending))
            } else if waiting(&requests.current) {
                finished(&mut requests.current);
                Some(Vec::new())
            } else {
                None
            }
        });
        let Some(rejected) = done else {
            return;
        };
        self.reject_decodes(element, &rejected);
        self.set_needs_reattach(element);
        let generation = loader.generation.get();
        self.queue_image_task(element, ImageTaskAction::Event { load, generation });
    }

    /// Queues `action` for `element` in its document's window, a task of
    /// its own (HTML's DOM manipulation task source).
    /// `element`, waiting for an image, held by its document's window
    /// (`Window::pending_images`) till its image comes: none without a
    /// window (nothing waits there).
    fn hold_pending_image(&self, element: NodeRef<'gc>) {
        let Some(window) = self.window_of(self.document_of(element)) else {
            return;
        };
        let list = match window.pending_images.get(self.gc) {
            Some(list) => list,
            None => {
                let list = HeapVector::new(self.gc, 1);
                window.pending_images.set(self.gc, Some(list));
                list
            }
        };
        let held = (0..list.len()).any(|index| {
            list.get(self.gc, index)
                .is_some_and(|other| Gc::ptr_eq(other, element))
        });
        if !held {
            list.push(self.gc, element);
        }
    }

    /// `element` no longer held for its image (`hold_pending_image`).
    fn let_go_pending_image(&self, element: NodeRef<'gc>) {
        let Some(list) = self
            .window_of(self.document_of(element))
            .and_then(|window| window.pending_images.get(self.gc))
        else {
            return;
        };
        if let Some(index) = (0..list.len()).find(|&index| {
            list.get(self.gc, index)
                .is_some_and(|other| Gc::ptr_eq(other, element))
        }) {
            list.remove(self.gc, index);
        }
    }

    /// How many `<img>`s `window` holds for their images (the tests').
    pub fn pending_images_for_testing(&self, window: Gc<'gc, crate::Window>) -> usize {
        window
            .pending_images
            .get(self.gc)
            .map_or(0, |list| list.len())
    }

    fn queue_image_task(&self, element: NodeRef<'gc>, action: ImageTaskAction) {
        self.queue_element_task_with(element, action);
    }

    fn reject_decodes(&self, element: NodeRef<'gc>, promises: &[u64]) {
        for &promise in promises {
            self.queue_image_task(element, ImageTaskAction::Reject { promise });
        }
    }

    /// Whether `loader`'s requests delay `element`'s node document's load
    /// event (they do from the update that fetches or queues an event till
    /// the event has fired), its count kept.
    fn set_delaying(&self, loader: Gc<'gc, ImageLoader>, element: NodeRef<'gc>, delays: bool) {
        let document = self.document_of(element);
        // HTML's "delay load event": not for an update that began lazy
        // (Chrome's neither).
        let delays = delays && !loader.lazy_update.get();
        let current = loader.delaying.get(self.gc);
        if delays && current.is_some_and(|delayed| Gc::ptr_eq(delayed, document)) {
            return;
        }
        if let Some(delayed) = current {
            self.change_load_delay(delayed, false);
            loader.delaying.set(self.gc, None);
        }
        if delays {
            self.change_load_delay(document, true);
            loader.delaying.set(self.gc, Some(document));
        }
    }

    /// The image event task `generation` queued for `element` is due: its
    /// `decode()` promises taken, its document's load no longer delayed by
    /// it; none if another update came since.
    pub fn take_image_event(
        &self,
        element: NodeRef<'gc>,
        load: bool,
        generation: u64,
    ) -> Option<ImageEvent> {
        let loader = self.image_loader(element)?;
        if loader.generation.get() != generation {
            return None;
        }
        let decodes = self.with_requests(loader, |requests| {
            std::mem::take(&mut requests.current.decodes)
        });
        // No pending request loads now: one is made only once the current
        // has its image, by an update, which makes this task an older one.
        self.set_delaying(loader, element, false);
        Some(ImageEvent {
            load,
            fire: load || !loader.reacting.get(),
            decodes,
        })
    }

    /// Whether the `decode()` promise `promise`, asked when the current
    /// request was available, resolves (its task is due): the current
    /// request is the same one and has its image.
    pub fn take_ready_decode(&self, element: NodeRef<'gc>, promise: u64) -> bool {
        self.image_loader(element).is_some_and(|loader| {
            self.with_requests(loader, |requests| {
                let Some(at) = requests.ready.iter().position(|&(id, _)| id == promise) else {
                    return false;
                };
                let (_, serial) = requests.ready.remove(at);
                serial == requests.serial
                    && requests.current.state == ImageRequestState::CompletelyAvailable
            })
        })
    }

    /// HTML's `decode()` for `element`: a promise, numbered (the engine
    /// keeps its resolver), settled in a task for a current request that
    /// is available or broken, or by its event for one that loads.
    pub fn image_decode(&self, element: NodeRef<'gc>) -> ImageDecode {
        let document = self.document_of(element);
        if self.window_of(document).is_none() {
            return ImageDecode::Reject;
        }
        let promise = self.new_promise_number();
        let loader = self.image_loader_or_new(element);
        let queued = loader.microtask_queued.get();
        // For the request its update's microtask makes, the pending one,
        // or the current.
        let action = self.with_requests(loader, |requests| {
            if queued {
                requests.upcoming.push(promise);
                return None;
            }
            if let Some(pending) = &mut requests.pending {
                pending.decodes.push(promise);
                return None;
            }
            let current = &mut requests.current;
            match current.state {
                ImageRequestState::Unavailable if !current.url.is_empty() => {
                    current.decodes.push(promise);
                    None
                }
                ImageRequestState::CompletelyAvailable => {
                    requests.ready.push((promise, requests.serial));
                    Some(ImageTaskAction::Decode { promise })
                }
                _ => Some(ImageTaskAction::Reject { promise }),
            }
        });
        if let Some(action) = action {
            self.queue_image_task(element, action);
        }
        ImageDecode::Pending(promise)
    }

    /// What `element`'s current request shows (layout's): none for an
    /// element with no image to show (no `src`).
    pub fn image_state(&self, element: NodeRef<'gc>) -> Option<ImageState> {
        let loader = self.image_loader(element)?;
        self.with_requests(loader, |requests| {
            let current = &requests.current;
            if current.url.is_empty() {
                return None;
            }
            Some(
                match (current.state, &current.image, current.natural_size()) {
                    (
                        ImageRequestState::CompletelyAvailable,
                        Some(Image::Raster(data, _)),
                        Some(size),
                    ) => ImageState::Loaded(Image::Raster(data.clone(), size)),
                    (
                        ImageRequestState::CompletelyAvailable,
                        Some(Image::Svg(image, natural)),
                        _,
                    ) => {
                        let corrected = |length: f32| {
                            base::numerics::f32_from_f64(f64::from(length) / current.density)
                        };
                        ImageState::Loaded(Image::Svg(
                            *image,
                            gfx::svg::SvgNaturalSize {
                                width: natural.width.map(corrected),
                                height: natural.height.map(corrected),
                                ratio: natural.ratio,
                            },
                        ))
                    }
                    (ImageRequestState::Unavailable, ..) => ImageState::Loading,
                    _ => ImageState::Failed,
                },
            )
        })
    }

    /// `element`'s current request as a canvas's image source (HTML's
    /// "check the usability of the image argument").
    pub fn image_for_canvas(&self, element: NodeRef<'gc>) -> ImageForCanvas {
        let Some(loader) = self.image_loader(element) else {
            return ImageForCanvas::Nothing;
        };
        if loader.microtask_queued.get() {
            return ImageForCanvas::Nothing;
        }
        let found = self.with_requests(loader, |requests| {
            let current = &requests.current;
            if requests.pending.is_some() {
                return Err(false);
            }
            match (current.state, &current.image) {
                (ImageRequestState::Broken, _) => Err(true),
                // An SVG image isn't drawn (Chrome draws it
                // at its natural size).
                (ImageRequestState::CompletelyAvailable, Some(Image::Raster(data, size)))
                    if size.width > 0 && size.height > 0 =>
                {
                    Ok((data.clone(), *size, current.density, current.url.clone()))
                }
                _ => Err(false),
            }
        });
        match found {
            Ok((data, size, density, url)) => ImageForCanvas::Image {
                data,
                size,
                density,
                origin_clean: !self.changes.images.borrow().opaque.contains(&url),
            },
            Err(true) => ImageForCanvas::Broken,
            Err(false) => ImageForCanvas::Nothing,
        }
    }

    /// `element`'s current request's state and URL (`complete`,
    /// `currentSrc`): none before its first update.
    pub fn image_request(&self, element: NodeRef<'gc>) -> Option<(ImageRequestState, String)> {
        let loader = self.image_loader(element)?;
        Some(self.with_requests(loader, |requests| {
            (requests.current.state, requests.current.url.clone())
        }))
    }

    /// `element`'s current request's image's natural size, 0 by 0 without
    /// one (`naturalWidth`, `naturalHeight`).
    pub fn image_natural_size(&self, element: NodeRef<'gc>) -> gfx::Size {
        self.image_loader(element)
            .and_then(|loader| {
                self.with_requests(loader, |requests| requests.current.natural_size_truncated())
            })
            .unwrap_or_default()
    }

    /// Whether `element` has a pending request (its image data being
    /// updated while the current keeps its image): `complete` is false.
    pub fn image_has_pending_request(&self, element: NodeRef<'gc>) -> bool {
        self.image_loader(element)
            .is_some_and(|loader| self.with_requests(loader, |requests| requests.pending.is_some()))
    }

    /// Whether `element`'s image data's microtask is queued (`complete` is
    /// false while it is, as Blink's pending update).
    pub fn image_update_queued(&self, element: NodeRef<'gc>) -> bool {
        self.image_loader(element)
            .is_some_and(|loader| loader.microtask_queued.get())
    }
}
