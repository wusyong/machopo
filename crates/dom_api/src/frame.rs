//! An `<iframe>`'s child browsing context, as its steps make it (HTML's
//! post-connection, removing and attribute change steps of an iframe;
//! Blink's `HTMLFrameOwnerElement` and `HTMLIFrameElement`): connected in a
//! document shown in a browsing context, it gets one of its own (the
//! engine's realm, a window of an initial `about:blank`); its `srcdoc` or
//! `src` then navigates it (the host loads the document), an initial
//! `about:blank` firing its `load` at once; disconnected, it loses it, its
//! document discarded. The DOM records what happened to the iframes, and
//! the script elements to prepare, in order (`Cx::take_element_changes`);
//! the engine runs these steps for them once each call is over
//! (`update_elements`), as Blink does as the operation ends
//! (`DidNotifySubtreeInsertionsToDocument`).

use dom::{ElementChange, FrameOwnerChange, NodeRoot, QualName, QuirksMode, TargetRoot, names};

pub use protocol::FrameToken;

use crate::ScriptHost;

/// A browsing context's number, the engine's and its host's: the window
/// its realm has changes as it navigates, the number stays. Outside the
/// renderer a frame is known by its token instead (`FrameToken`), which
/// the engine keeps with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameId(pub u64);

/// A new child frame's token: 128 random bits from the operating system,
/// as the browser's main frame token is (Chromium's frame tokens are
/// `base::UnguessableToken`s), so that a page can't guess another frame's.
pub fn new_frame_token() -> FrameToken {
    let mut bytes = [0u8; 16];
    // Chromium can't go on without them either (BoringSSL aborts).
    getrandom::fill(&mut bytes).expect("the operating system gives random numbers");
    FrameToken(bytes)
}

/// What a child browsing context navigates to (HTML's "process the iframe
/// attributes").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildNavigation {
    /// `src`, resolved against its iframe's document's URL; one that
    /// doesn't resolve is `about:blank`.
    Src(String),
    /// `srcdoc`'s markup, for an `about:srcdoc` document.
    Srcdoc(String),
    /// A new `about:blank` (an empty or missing `src`, after the first).
    AboutBlank,
}

/// A frame's navigation to another document its host sent the browser
/// (`FrameClient::navigate`, `navigate_child`), booked on the frame as it
/// was sent, till the browser commits it or drops it
/// (`ScriptEngine::take_navigation`): the host's number for it, whether its
/// iframe's referrer policy is `no-referrer` (what its navigation timing
/// reads of its response's `Timing-Allow-Origin`; never the top-level
/// frame's), and, for one to `about:blank`, the document that asked, which
/// its document has its origin from (HTML's initiator origin; Chromium's
/// `origin_to_commit`, taken as the navigation starts: an opaque one is
/// that document's alone, which a value through the browser and back
/// wouldn't be). A frame has one at a time: one asked for after another
/// takes its place.
#[derive(Debug, Clone)]
pub struct NavigationRequest {
    pub begin_id: u64,
    pub no_referrer: bool,
    pub origin_source: Option<dom::NodeRoot>,
}

impl NavigationRequest {
    /// The navigation numbered `begin_id` to `url` (none: a reload), of the
    /// frame whose iframe is `owner` (none: the top-level frame's), as
    /// script in `initiator` asked.
    pub fn new<'gc>(
        cx: &dom::Cx<'_, 'gc>,
        begin_id: u64,
        owner: Option<dom::NodeRef<'gc>>,
        initiator: dom::NodeRef<'gc>,
        url: Option<&str>,
    ) -> Self {
        let no_referrer = owner.is_some_and(|owner| {
            cx.attribute_value_by_id(owner, dom::names::local::REFERRERPOLICY)
                .is_some_and(|policy| policy.borrow().to_str().eq_ignore_ascii_case("no-referrer"))
        });
        let blank = url.is_some_and(|url| {
            url::Url::parse(url).is_ok_and(|url| url.scheme() == "about" && url.path() == "blank")
        });
        Self {
            begin_id,
            no_referrer,
            origin_source: blank.then(|| cx.root(initiator)),
        }
    }
}

/// How long an iframe's `load` is put off after a navigation within its
/// frame's document that another origin asked for (Blink's 100 ms
/// `DispatchLoad`, crbug.com/1248444).
const OWNER_LOAD_DELAY_MS: f64 = 100.0;

/// An iframe's navigation that goes nowhere its host sends the browser
/// (`child_navigation_here`): the engine's to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationHere {
    /// To a fragment of the document its frame shows: its URL that,
    /// scrolled there (`navigate_child_to_fragment`; HTML's navigate to a
    /// fragment, but for `hashchange` and a history entry).
    Fragment(url::Url),
    /// To a `javascript:` URL: an empty document commits, its script not
    /// run (as its load failed when its host fetched it), in a task of its
    /// host's (`FrameClient::commit_here`).
    JavaScript(url::Url),
}

/// What an iframe's navigation of its frame, which shows `document`, is,
/// if it goes nowhere its host sends the browser: none for one that does
/// (one to another document, `srcdoc`, `about:blank`).
pub fn child_navigation_here<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    document: dom::NodeRef<'gc>,
    navigation: &ChildNavigation,
) -> Option<NavigationHere> {
    let ChildNavigation::Src(src) = navigation else {
        return None;
    };
    let url = url::Url::parse(src).ok()?;
    if url.scheme() == "javascript" {
        return Some(NavigationHere::JavaScript(url));
    }
    url.fragment()?;
    let current = cx
        .document_url(document)
        .and_then(|current| url::Url::parse(&current.to_str()).ok())
        .unwrap_or_else(|| url::Url::parse("about:blank").expect("about:blank"));
    let without = |url: &url::Url| {
        let mut url = url.clone();
        url.set_fragment(None);
        url
    };
    (without(&current) == without(&url)).then_some(NavigationHere::Fragment(url))
}

/// `owner`'s frame navigated to `url`, a fragment of the document it
/// shows (`NavigationHere::Fragment`): the document's URL that, scrolled
/// to the fragment once laid out (the iframe's attribute that changed asks
/// for the frame that shows it). If its document
/// is completely loaded and of another origin than `owner`'s (whose
/// script asked), `owner` fires `load` a while after (the last of such
/// navigations in a row's), as it would for another document, so that
/// script can't tell which the URL was (Blink's
/// `DocumentLoader::CommitSameDocumentNavigation`).
pub fn navigate_child_to_fragment<H: ScriptHost>(host: &mut H, owner: &NodeRoot, url: &url::Url) {
    host.mutate(|cx| {
        let owner = owner.get(cx.gc);
        let Some(document) = cx.content_document(owner) else {
            return;
        };
        cx.set_document_url(document, url.as_str());
        let id = cx.node_id(document);
        if let Some(target) = crate::history::fragment_target(cx, document, url) {
            cx.lend_layout(document, |layout| layout.scroll_to_fragment(id, target));
        }
        if cx.is_completely_loaded(document) && !same_origin(cx, document, cx.document_of(owner)) {
            let number = cx.next_owner_load(document);
            cx.add_native_timer(
                owner,
                dom::NativeAction::OwnerLoad,
                number,
                OWNER_LOAD_DELAY_MS,
                None,
            );
        }
    });
}

/// `owner`'s `load` put off as `number` is due (`navigate_child_to_fragment`):
/// fired, unless another was put off since, or its frame went.
pub(crate) fn owner_load_due<H: ScriptHost>(host: &mut H, owner: &NodeRoot, number: u64) {
    let latest = host.read(|cx| {
        cx.content_document(owner.get(cx.gc))
            .is_some_and(|document| cx.owner_load(document) == number)
    });
    if latest {
        crate::fire_event(
            host,
            &TargetRoot::Node(owner.clone()),
            names::event::LOAD,
            false,
            false,
        );
    }
}

/// What the engine asks of its host for its frames, after the call that
/// made it so (`ScriptEngine::take_frame_requests`), in order. Frames made
/// and gone are told as they come and go (`FrameClient`, `FrameLayout`),
/// and are the engine's to ask of (`ScriptEngine::child_frames`,
/// `take_frames_changed`).
#[derive(Debug, Clone)]
pub enum FrameRequest {
    /// Script in `frame`'s realm made `url` a blob URL for `data`
    /// (`URL.createObjectURL`): the host keeps it, until it is revoked or
    /// the realm goes (the frame's next document, or the frame). The
    /// browser takes it through `loader`, the realm's document's.
    RegisterBlobUrl {
        frame: FrameId,
        loader: u64,
        url: String,
        data: dom::BlobData,
    },
    /// `url` stands for nothing now (`URL.revokeObjectURL`).
    RevokeBlobUrl { url: String },
}

/// The steps for what happened to the iframes, and the script elements
/// to prepare, since the last time (`Cx::take_element_changes`), in order:
/// a script runs after the iframes before it in tree order got their
/// browsing contexts. A host with no frames
/// (`ScriptHost::create_child_window` makes none) gives no iframe one.
pub fn update_elements<H: ScriptHost>(host: &mut H) {
    // The blob URLs registered and revoked, before the loads the iframes'
    // and scripts' steps may start of them.
    for request in host.mutate(|cx| cx.take_blob_url_requests()) {
        match request {
            dom::BlobUrlRequest::Register {
                document,
                url,
                data,
            } => match document {
                Some(document) => {
                    let document =
                        host.read(|cx| cx.node_by_id(document).map(|document| cx.root(document)));
                    if let Some(document) = document {
                        host.register_blob_url(Some(&document), &url, &data);
                    }
                }
                // A worker's global's.
                None => host.register_blob_url(None, &url, &data),
            },
            dom::BlobUrlRequest::Revoke { url } => host.revoke_blob_url(&url),
        }
    }
    update_element_changes(host);
    // The navigations script asked for: the host's (a fragment's
    // `popstate` first).
    crate::history::update_navigations(host);
}

fn update_element_changes<H: ScriptHost>(host: &mut H) {
    for change in host.mutate(|cx| cx.take_element_changes()) {
        let change = match change {
            ElementChange::Frame(change) => change,
            ElementChange::Script(script) => {
                crate::script::update_script(host, script);
                continue;
            }
            ElementChange::Autofocus(element) => {
                crate::focus::autofocus_inserted(host, element);
                continue;
            }
            ElementChange::ModulePreload(link) => {
                let link = host.read(|cx| {
                    let link = cx.node_by_id(link)?;
                    // Still one, still connected (the steps before it may have
                    // changed it).
                    (cx.is_connected(link) && cx.is_module_preload(link)).then(|| cx.root(link))
                });
                if let Some(link) = link {
                    host.preload_module(&link);
                }
                continue;
            }
        };
        match change {
            FrameOwnerChange::Inserted(owner) => {
                if let Some(owner) = create_child_frame(host, owner) {
                    process_iframe_attributes(host, &owner, true);
                }
            }
            FrameOwnerChange::Removed(owner) => remove_child_frame(host, owner),
            // HTML's attribute change steps: `src` counts only without a
            // `srcdoc`.
            FrameOwnerChange::SrcChanged(owner) | FrameOwnerChange::SrcdocChanged(owner) => {
                let owner = host.read(|cx| {
                    let owner = cx.node_by_id(owner)?;
                    let has_frame = cx.content_document(owner).is_some();
                    let srcdoc = cx
                        .attribute_value_by_id(owner, names::local::SRCDOC)
                        .is_some();
                    (has_frame && (matches!(change, FrameOwnerChange::SrcdocChanged(_)) || !srcdoc))
                        .then(|| cx.root(owner))
                });
                if let Some(owner) = owner {
                    process_iframe_attributes(host, &owner, false);
                }
            }
            // Its frame's host told, for the frame's next navigation (HTML:
            // the flags count from then on).
            FrameOwnerChange::SandboxChanged(owner) => {
                let changed = host.read(|cx| {
                    let owner = cx.node_by_id(owner)?;
                    cx.content_document(owner)?;
                    Some((cx.root(owner), iframe_sandbox(cx, owner)))
                });
                if let Some((owner, sandbox)) = changed {
                    host.frame_policy_changed(&owner, sandbox);
                }
            }
        }
    }
}

/// HTML's "destroy a child navigable" for `owner`, removed: its document
/// and those of the frames in it unload, script allowed (Blink's
/// `LocalFrame::DetachImpl`: no frame made, no navigation, meanwhile:
/// `Cx::unloading`), then they are discarded with their frames; then its
/// document, whose `load` may have waited for it, may be loaded
/// (`check_completed`; Blink's `FrameLoader::Detach` checks the parent as
/// the child goes). The unload's handlers may have changed anything: its
/// content document is looked for again.
fn remove_child_frame<H: ScriptHost>(host: &mut H, owner: dom::NodeId) {
    let Some(document) = host.read(|cx| {
        let owner = cx.node_by_id(owner)?;
        cx.content_document(owner).map(|document| cx.root(document))
    }) else {
        return;
    };
    host.unload_child(&document);
    let gone = host.mutate(|cx| {
        let owner = cx.node_by_id(owner)?;
        let current = cx.content_document(owner)?;
        if !elise::Gc::ptr_eq(current, document.get(cx.gc)) {
            return None;
        }
        cx.discard_content_document(owner)?;
        Some(cx.root(cx.document_of(owner)))
    });
    let Some(parent) = gone else {
        return;
    };
    host.discard_child(&document);
    check_parent_completed(host, &parent);
}

/// `check_completed` of `parent`, whose child frame's load ended or which
/// it went from, its style brought up to date through its frame's layout,
/// lent only if nothing else delays it.
pub(crate) fn check_parent_completed<H: ScriptHost>(host: &mut H, parent: &NodeRoot) {
    host.mutate(|cx| {
        let parent = parent.get(cx.gc);
        if cx.load_progress(parent) != dom::LoadProgress::Parsed {
            return;
        }
        let checked = cx.lend_layout(parent, |layout| {
            crate::lifecycle::check_completed(cx, Some(layout), parent)
        });
        if checked.is_none() {
            crate::lifecycle::check_completed(cx, None, parent);
        }
    });
}

/// HTML's "create a new child navigable" for `owner`, if it is still
/// connected and its document is shown in a browsing context (has a
/// window, not discarded, not unloading): its initial `about:blank` (`<html>`, `<head>`, `<body>`, in
/// quirks mode) in a realm the engine makes. The iframe, if it got one.
fn create_child_frame<H: ScriptHost>(host: &mut H, owner: dom::NodeId) -> Option<NodeRoot> {
    let owner = host.read(|cx| {
        let owner = cx
            .node_by_id(owner)
            .filter(|owner| cx.is_connected(*owner))?;
        let document = cx.document_of(owner);
        cx.window_of(document)?;
        // Not while it unloads (Blink's `SubframeLoadingDisabler`): it is
        // to go, and its frames with it.
        (!cx.is_discarded(document) && !cx.unloading(document)).then(|| cx.root(owner))
    })?;
    let document = host.create_child_window(&owner)?;
    host.mutate(|cx| {
        let document = document.get(cx.gc);
        cx.set_quirks_mode(document, QuirksMode::Quirks);
        let html = cx.create_element(document, QualName::html("html"));
        let head = cx.create_element(document, QualName::html("head"));
        let body = cx.create_element(document, QualName::html("body"));
        cx.append_child(document, html)
            .expect("an element goes in a new document");
        cx.append_child(html, head)
            .expect("an element goes in a new element");
        cx.append_child(html, body)
            .expect("an element goes in a new element");
        cx.set_content_document(owner.get(cx.gc), Some(document));
    });
    Some(owner)
}

/// HTML's "process the iframe attributes": `srcdoc` if it has one, else
/// `src` (`about:blank` when empty, missing or about:blank). Inserted
/// (`initial`) with `about:blank`, the iframe doesn't navigate: its `load`
/// fires at once, while the insertion's call runs (Blink does the same).
fn process_iframe_attributes<H: ScriptHost>(host: &mut H, owner: &NodeRoot, initial: bool) {
    let (src, srcdoc) = host.read(|cx| {
        let owner = owner.get(cx.gc);
        let value = |local| {
            cx.attribute_value_by_id(owner, local)
                .map(|value| value.borrow().to_string())
        };
        (value(names::local::SRC), value(names::local::SRCDOC))
    });
    let navigation = match (srcdoc, src) {
        (Some(srcdoc), _) => ChildNavigation::Srcdoc(srcdoc),
        (None, Some(src)) if !src.is_empty() && !matches_about_blank(&src) => {
            let url = host.read(|cx| cx.resolve_url(cx.document_of(owner.get(cx.gc)), src.trim()));
            match url.ok_or(()) {
                Ok(url) => ChildNavigation::Src(url.to_string()),
                Err(_) => ChildNavigation::AboutBlank,
            }
        }
        (None, _) if initial => {
            crate::fire_event(
                host,
                &TargetRoot::Node(owner.clone()),
                names::event::LOAD,
                false,
                false,
            );
            return;
        }
        (None, _) => ChildNavigation::AboutBlank,
    };
    host.navigate_child(owner, navigation);
}

/// URL's "matches about:blank" for `src` as written: absolute, so no base
/// URL is needed (a relative one never resolves to it from a document of
/// another scheme).
fn matches_about_blank(src: &str) -> bool {
    url::Url::parse(src.trim()).is_ok_and(|url| {
        url.scheme() == "about"
            && url.path() == "blank"
            && url.username().is_empty()
            && url.password().is_none()
            && url.host().is_none()
    })
}

/// `contentDocument`: the iframe's content document, if it has a
/// browsing context.
pub(crate) fn content_document<'gc>(
    cx: &crate::ApiCx<'_, 'gc>,
    this: dom::NodeRef<'gc>,
) -> Result<Option<dom::NodeRef<'gc>>, crate::ApiError> {
    // HTML's: none unless of the current realm's origin (the iframe's
    // document's: its own realm's getter is the one called).
    Ok(cx
        .content_document(this)
        .filter(|&document| same_origin(cx, document, cx.document_of(this))))
}

/// `contentWindow`: its content document's window.
pub(crate) fn content_window<'gc>(
    cx: &crate::ApiCx<'_, 'gc>,
    this: dom::NodeRef<'gc>,
) -> Result<Option<elise::Gc<'gc, dom::Window>>, crate::ApiError> {
    Ok(cx
        .content_document(this)
        .and_then(|document| cx.window_of(document)))
}

/// The window of the browsing context `window`'s is the child of: its
/// iframe's document's.
fn parent_window<'gc>(
    cx: &crate::ApiCx<'_, 'gc>,
    window: elise::Gc<'gc, dom::Window>,
) -> Option<elise::Gc<'gc, dom::Window>> {
    let owner = cx.frame_owner(cx.window_document(window))?;
    cx.window_of(cx.document_of(owner))
}

/// Whether `window`'s document was discarded: it has no browsing context.
fn is_discarded<'gc>(cx: &crate::ApiCx<'_, 'gc>, window: elise::Gc<'gc, dom::Window>) -> bool {
    cx.is_discarded(cx.window_document(window))
}

/// `parent`: the parent browsing context's window; a top-level one's is
/// itself; none without a browsing context.
pub(crate) fn parent<'gc>(
    cx: &crate::ApiCx<'_, 'gc>,
    this: elise::Gc<'gc, dom::Window>,
) -> Result<Option<elise::Gc<'gc, dom::Window>>, crate::ApiError> {
    if is_discarded(cx, this) {
        return Ok(None);
    }
    Ok(Some(parent_window(cx, this).unwrap_or(this)))
}

/// `top`: the top-level browsing context's window, up the parents; none
/// without a browsing context.
pub(crate) fn top<'gc>(
    cx: &crate::ApiCx<'_, 'gc>,
    this: elise::Gc<'gc, dom::Window>,
) -> Result<Option<elise::Gc<'gc, dom::Window>>, crate::ApiError> {
    if is_discarded(cx, this) {
        return Ok(None);
    }
    let mut window = this;
    while let Some(parent) = parent_window(cx, window) {
        window = parent;
    }
    Ok(Some(window))
}

/// `frameElement`: the iframe whose browsing context's window it is; none
/// for a top-level one.
pub(crate) fn frame_element<'gc>(
    cx: &crate::ApiCx<'_, 'gc>,
    this: elise::Gc<'gc, dom::Window>,
) -> Result<Option<dom::NodeRef<'gc>>, crate::ApiError> {
    // HTML's: none unless its document is of the current realm's origin
    // (the window's).
    let document = cx.window_document(this);
    Ok(cx
        .frame_owner(document)
        .filter(|&owner| same_origin(cx, cx.document_of(owner), document)))
}

/// `close()`: a browsing context script didn't open isn't one script may
/// close (HTML's script-closable), and a child's never is.
pub(crate) fn close<'gc>(
    _: &crate::ApiCx<'_, 'gc>,
    _: elise::Gc<'gc, dom::Window>,
) -> Result<(), crate::ApiError> {
    Ok(())
}

/// `closed`: whether it has no browsing context (its iframe gone).
pub(crate) fn closed<'gc>(
    cx: &crate::ApiCx<'_, 'gc>,
    this: elise::Gc<'gc, dom::Window>,
) -> Result<bool, crate::ApiError> {
    Ok(is_discarded(cx, this))
}

/// `opener`: none (no browsing context is opened by another).
pub(crate) fn opener<'gc>(
    _: &crate::ApiCx<'_, 'gc>,
    _: elise::Gc<'gc, dom::Window>,
) -> Result<Option<elise::Gc<'gc, dom::Window>>, crate::ApiError> {
    Ok(None)
}

/// `blur()`: HTML's does nothing.
pub(crate) fn blur<'gc>(
    _: &crate::ApiCx<'_, 'gc>,
    _: elise::Gc<'gc, dom::Window>,
) -> Result<(), crate::ApiError> {
    Ok(())
}

/// The windows of `window`'s document-tree child browsing contexts: its
/// document's iframes' that have one, in tree order (not in shadow trees).
pub fn child_windows<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    window: elise::Gc<'gc, dom::Window>,
) -> Vec<elise::Gc<'gc, dom::Window>> {
    let mut windows = Vec::new();
    let mut stack: Vec<_> = cx
        .first_child(cx.window_document(window))
        .into_iter()
        .collect();
    while let Some(node) = stack.pop() {
        if cx.is_iframe(node)
            && let Some(child) = cx
                .content_document(node)
                .and_then(|document| cx.window_of(document))
        {
            windows.push(child);
        }
        stack.extend(cx.next_sibling(node));
        stack.extend(cx.first_child(node));
    }
    windows
}

/// The names of `window`'s child windows (HTML's document-tree child
/// navigable target name property set: their iframes' `name`s, each once,
/// in tree order, none empty), with the first window of each: what
/// another origin reaches them by.
pub fn named_child_windows<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    window: elise::Gc<'gc, dom::Window>,
) -> Vec<(String, elise::Gc<'gc, dom::Window>)> {
    let mut named: Vec<(String, elise::Gc<'gc, dom::Window>)> = Vec::new();
    for child in child_windows(cx, window) {
        let Some(owner) = cx.frame_owner(cx.window_document(child)) else {
            continue;
        };
        let Some(name) = cx.attribute_value_by_id(owner, names::local::NAME) else {
            continue;
        };
        let name = name.borrow().to_str().into_owned();
        if !name.is_empty() && !named.iter().any(|(other, _)| *other == name) {
            named.push((name, child));
        }
    }
    named
}

/// `length`: how many child browsing contexts its document has.
pub(crate) fn length<'gc>(
    cx: &crate::ApiCx<'_, 'gc>,
    this: elise::Gc<'gc, dom::Window>,
) -> Result<u32, crate::ApiError> {
    Ok(u32::try_from(child_windows(cx, this).len()).unwrap_or(u32::MAX))
}

/// `window[index]`: the window of its document's `index`th child browsing
/// context, if it has that many.
pub(crate) fn child_window<'gc>(
    cx: &crate::ApiCx<'_, 'gc>,
    this: elise::Gc<'gc, dom::Window>,
    index: u32,
) -> Result<Option<elise::Gc<'gc, dom::Window>>, crate::ApiError> {
    Ok(child_windows(cx, this).get(index as usize).copied())
}

/// HTML's activation notification steps, for user input that fires an
/// activation-triggering event in `document` (before it is dispatched):
/// its window activated, the windows of the frames above it whatever their
/// origin, and those of the frames below it of its origin, however deep
/// (Blink's `FrameTreeNode::NotifyUserActivation`).
pub(crate) fn notify_activation<'gc>(cx: &dom::Cx<'_, 'gc>, document: dom::NodeRef<'gc>) {
    let mut at = Some(document);
    while let Some(above) = at {
        if let Some(window) = cx.window_of(above) {
            cx.notify_activation(window);
        }
        at = cx.frame_owner(above).map(|owner| cx.document_of(owner));
    }
    let mut below = cx.child_content_documents(document);
    while let Some(child) = below.pop() {
        below.extend(cx.child_content_documents(child));
        if same_origin(cx, child, document)
            && let Some(window) = cx.window_of(child)
        {
            cx.notify_activation(window);
        }
    }
}

/// HTML's "consume user activation" of `document`'s window: if it has
/// transient activation, every window of its page (its top-level
/// document's frame tree) loses its transient activation, keeping its
/// sticky one.
pub(crate) fn consume_activation<'gc>(cx: &dom::Cx<'_, 'gc>, document: dom::NodeRef<'gc>) {
    if !cx
        .window_of(document)
        .is_some_and(|window| cx.has_transient_activation(window))
    {
        return;
    }
    for document in documents_in_tree_order(cx, cx.top_document(document)) {
        if let Some(window) = cx.window_of(document) {
            cx.consume_activation(window);
        }
    }
}

thread_local! {
    /// How many times, on this thread, `documents_in_tree_order` walked a
    /// frame tree's documents. Counted in debug builds only.
    static FRAME_TREE_WALKS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many times, on this thread, the documents of a frame tree were
/// walked for their iframes (`documents_in_tree_order`): what tells a
/// rendering update walks them once, and the host's loop not at all.
/// `None` in release builds. For tests.
pub fn frame_tree_walks_for_testing() -> Option<u64> {
    cfg!(debug_assertions).then(|| FRAME_TREE_WALKS.get())
}

/// The documents of `top`'s frame tree, `top` first, each followed by
/// those of its iframes in shadow-including tree order (HTML's order of
/// the documents the rendering update goes through: one after its
/// container document; Chrome walks its frame tree). Each document's tree
/// is walked for its iframes: what a rendering update does once
/// (`ScriptEngine::begin_rendering_update`).
pub fn documents_in_tree_order<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    top: dom::NodeRef<'gc>,
) -> Vec<dom::NodeRef<'gc>> {
    if cfg!(debug_assertions) {
        FRAME_TREE_WALKS.set(FRAME_TREE_WALKS.get().wrapping_add(1));
    }
    let mut documents = Vec::new();
    let mut stack = vec![top];
    while let Some(document) = stack.pop() {
        documents.push(document);
        stack.extend(cx.child_content_documents(document).into_iter().rev());
    }
    documents
}

/// `owner`'s `sandbox` flags (an iframe's, HTML's "parse a sandboxing
/// directive" of its attribute, as Chromium's `ParseWebSandboxPolicy`):
/// none without the attribute.
pub fn iframe_sandbox<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    owner: dom::NodeRef<'gc>,
) -> protocol::sandbox::SandboxFlags {
    cx.attribute_value_by_id(owner, names::local::SANDBOX)
        .map_or(protocol::sandbox::SandboxFlags::NONE, |value| {
            protocol::sandbox::parse(&value.borrow().to_string()).flags
        })
}

/// HTML's "scripting is disabled" for `document`: it is shown in no
/// browsing context (no window, or discarded), or is sandboxed without
/// `allow-scripts` (Blink's `CanExecuteScripts`, of the sandboxed scripts
/// flag).
pub fn scripting_disabled<'gc>(cx: &dom::Cx<'_, 'gc>, document: dom::NodeRef<'gc>) -> bool {
    cx.window_of(document).is_none()
        || cx.is_discarded(document)
        || document_sandbox(cx, document).contains(protocol::sandbox::SandboxFlags::SCRIPTS)
}

/// `document` is sandboxed by `flags`, as its host committed it: the DOM
/// keeps them, its origin a new opaque one with the origin flag, scripting
/// disabled for it with the scripts flag.
pub fn set_document_sandbox<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    document: dom::NodeRef<'gc>,
    flags: protocol::sandbox::SandboxFlags,
) {
    use protocol::sandbox::SandboxFlags;
    cx.set_sandbox(
        document,
        flags.bits(),
        flags.contains(SandboxFlags::ORIGIN),
        flags.contains(SandboxFlags::SCRIPTS),
    );
}

/// What `document` is sandboxed by, as its host committed it (the DOM's
/// bits, read as flags).
pub fn document_sandbox<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    document: dom::NodeRef<'gc>,
) -> protocol::sandbox::SandboxFlags {
    protocol::sandbox::SandboxFlags::from_bits(cx.sandbox_flags(document))
}

/// `document`'s origin, as its URL's (`dom`'s `document_origin`): none
/// without one.
pub fn origin<'gc>(cx: &dom::Cx<'_, 'gc>, document: dom::NodeRef<'gc>) -> Option<url::Origin> {
    cx.document_origin(document)
}

/// Whether `a` and `b` are of the same origin: their origins' documents
/// (`origin_document`) the same one, or their URLs' origins the same tuple
/// (an opaque one is no other document's).
pub fn same_origin<'gc>(cx: &dom::Cx<'_, 'gc>, a: dom::NodeRef<'gc>, b: dom::NodeRef<'gc>) -> bool {
    let (a, b) = (cx.origin_document(a), cx.origin_document(b));
    if elise::Gc::ptr_eq(a, b) {
        return true;
    }
    match (origin(cx, a), origin(cx, b)) {
        (Some(a), Some(b)) => a.is_tuple() && a == b,
        _ => false,
    }
}
