//! The engine's browsing contexts (Blink's `LocalFrame`s, each with its
//! `LocalWindowProxy`'s context): a top-level one, and a child of a frame
//! for each `<iframe>` connected in its document (`dom_api`'s frame
//! steps, as each call ends: `state::report`). Each has its realm and its
//! active document, by `FrameId`, the number the host knows it by; the
//! host is told what to do for them (`ScriptEngine::take_frame_requests`). In
//! the isolate's state: a call that inserts an iframe makes its realm.

use dom::{NodeId, NodeRoot, Window};
use dom_api::{ChildNavigation, FrameId, FrameRequest, FrameToken};

use crate::state::Scope;
use crate::window::create_window_realm;

/// The top-level browsing context's number.
pub(crate) const TOP: FrameId = FrameId(0);

/// A browsing context: its realm, its active document, its parent and its
/// iframe (none for the top-level one).
pub(crate) struct Frame {
    pub(crate) id: FrameId,
    /// Its token, drawn as it was made: none for the top-level frame, whose
    /// is the browser's (`Frames::main_token`).
    token: Option<FrameToken>,
    pub(crate) context: v8::Global<v8::Context>,
    pub(crate) document: NodeRoot,
    document_id: NodeId,
    pub(crate) parent: Option<FrameId>,
    pub(crate) owner: Option<NodeRoot>,
    /// Its document is the initial `about:blank` it was made with.
    pub(crate) initial: bool,
    /// Its realm's modules, from its first module script (`modules`).
    pub(crate) modules: Option<Box<crate::modules::RealmModules>>,
}

#[derive(Default)]
pub(crate) struct Frames {
    frames: Vec<Frame>,
    /// The last number given (the top-level one's is 0).
    last: u64,
    /// The top-level frame's token, once the browser told it
    /// (`ScriptEngine::set_main_frame_token`).
    pub(crate) main_token: Option<FrameToken>,
    /// The opaque origins made, numbered.
    opaque: u64,
    /// What the host is to do, in order.
    pub(crate) requests: Vec<FrameRequest>,
    /// What sends the loads script asks for as it asks, the host's
    /// (`ScriptEngine::set_load_sink`); none: they wait in `requests`.
    pub(crate) load_sink: Option<Box<dyn dom_api::LoadSink>>,
}

impl Frames {
    pub(crate) fn get(&self, id: FrameId) -> Option<&Frame> {
        self.frames.iter().find(|frame| frame.id == id)
    }

    /// The token of frame `id`, the browser's for the top-level one.
    pub(crate) fn token(&self, id: FrameId) -> Option<FrameToken> {
        if id == TOP {
            return self.main_token;
        }
        self.get(id)?.token
    }

    /// The frames, in the order they were made (the top-level one first):
    /// for what doesn't go in tree order, without walking the trees.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &Frame> {
        self.frames.iter()
    }

    /// The frame whose active document is `document`.
    pub(crate) fn of_document(&self, document: NodeId) -> Option<&Frame> {
        self.frames
            .iter()
            .find(|frame| frame.document_id == document)
    }

    pub(crate) fn of_document_mut(&mut self, document: NodeId) -> Option<&mut Frame> {
        self.frames
            .iter_mut()
            .find(|frame| frame.document_id == document)
    }

    /// `frame`'s realm and document are these now (a new one's, or one it
    /// navigated to): its token the one it had (a frame's own as it
    /// navigates), its modules the realm's, kept if the realm is (the
    /// initial `about:blank`'s window reused).
    pub(crate) fn set(&mut self, mut frame: Frame) {
        match self.frames.iter_mut().find(|old| old.id == frame.id) {
            Some(old) => {
                frame.token = old.token;
                if old.context == frame.context {
                    frame.modules = old.modules.take();
                }
                *old = frame;
            }
            None => self.frames.push(frame),
        }
    }

    /// The child frames gone (the top-level frame has a new document): the
    /// host told of those of its document.
    pub(crate) fn clear_children(&mut self) {
        for child in self.frames.iter().filter(|frame| frame.parent == Some(TOP)) {
            if let Some(token) = child.token {
                self.requests.push(FrameRequest::Removed {
                    frame: child.id,
                    token,
                });
            }
        }
        self.frames.retain(|frame| frame.id == TOP);
    }

    fn next_id(&mut self) -> FrameId {
        // One for each iframe's browsing context: fewer than u64::MAX.
        self.last = self.last.saturating_add(1);
        FrameId(self.last)
    }

    /// `frame` and the frames in it, gone: taken out.
    fn remove(&mut self, frame: FrameId) -> Vec<Frame> {
        self.remove_below(frame, true)
    }

    /// The frames inside `frame`'s document, taken out (and those inside
    /// theirs): `frame` itself stays.
    fn remove_descendants(&mut self, frame: FrameId) -> Vec<Frame> {
        self.remove_below(frame, false)
    }

    /// `frame` (if `with_it`) and every frame below it, taken out.
    fn remove_below(&mut self, frame: FrameId, with_it: bool) -> Vec<Frame> {
        let mut gone = vec![frame];
        let mut at = 0;
        while let Some(&parent) = gone.get(at) {
            gone.extend(
                self.frames
                    .iter()
                    .filter(|frame| frame.parent == Some(parent))
                    .map(|frame| frame.id),
            );
            at = at.saturating_add(1);
        }
        if !with_it {
            gone.remove(0);
        }
        let (removed, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.frames)
            .into_iter()
            .partition(|frame| gone.contains(&frame.id));
        self.frames = kept;
        removed
    }
}

impl Frame {
    /// A frame with no token of its own yet: the top-level one, or one
    /// navigating (`Frames::set` keeps its token).
    pub(crate) fn new(
        id: FrameId,
        context: v8::Global<v8::Context>,
        document: NodeRoot,
        document_id: NodeId,
        parent: Option<FrameId>,
        owner: Option<NodeRoot>,
        initial: bool,
    ) -> Self {
        Self {
            id,
            token: None,
            context,
            document,
            document_id,
            parent,
            owner,
            initial,
            modules: None,
        }
    }
}

/// The document of `context`'s window, with its id.
pub(crate) fn window_document(
    scope: &mut Scope<'_, '_, '_>,
    context: v8::Local<'_, v8::Context>,
) -> (NodeRoot, NodeId) {
    let global = context.global(scope.scope());
    let (state, mut no_gc) = scope.state();
    state.dom.read(|cx| {
        let window = no_gc
            .unwrap::<Window>(cx.gc, global)
            .expect("a realm's window");
        let document = cx.window_document(window);
        (cx.root(document), cx.node_id(document))
    })
}

/// HTML's "create a new child navigable" for `owner`: a realm whose global
/// is a window of a new HTML document at `about:blank`, its time origin
/// now, the child of the frame showing `owner`'s document; the host told.
/// None if no frame shows that document.
pub(crate) fn create_child(scope: &mut Scope<'_, '_, '_>, owner: &NodeRoot) -> Option<NodeRoot> {
    let parent = {
        let (state, _) = scope.state();
        let document = state
            .dom
            .read(|cx| cx.node_id(cx.document_of(owner.get(cx.gc))));
        state.frames.of_document(document)?.id
    };
    // Script on the stack made it, not the parser (Blink's
    // `is_created_by_script`), before any of its own realm's.
    let created_by_script = v8::StackTrace::current_stack_trace(scope.scope(), 1)
        .is_some_and(|stack| stack.get_frame_count() > 0);
    let context = create_window_realm(scope.scope(), "about:blank", None);
    // Its initial `about:blank`'s origin, its parent's.
    let token = {
        let (state, _) = scope.state();
        state.frames.get(parent).map(|frame| frame.context.clone())
    }
    .map(|parent| v8::Local::new(scope.scope(), parent).get_security_token(scope.scope()));
    if let Some(token) = token {
        context.set_security_token(token);
    }
    let global = v8::Global::new(scope.scope(), context);
    let (document, document_id) = window_document(scope, context);
    let id = scope.state_mut().0.frames.next_id();
    let token = dom_api::new_frame_token();
    // The inspector hears of it, of its parent's origin.
    let parent_document = {
        let (state, _) = scope.state();
        state
            .dom
            .read(|cx| cx.root(cx.document_of(owner.get(cx.gc))))
    };
    crate::devtools::realm_created(scope, context, id, Some(token), &parent_document);
    let (state, _) = scope.state_mut();
    // The page's system focus, its parent's (as `create_window`'s window
    // gets the engine's): a window that stays when it navigates to its
    // origin gets the focus events too.
    state.dom.mutate(|cx| {
        let window = cx.window_of(document.get(cx.gc)).expect("a realm's window");
        let parent = cx.window_of(cx.document_of(owner.get(cx.gc)));
        let focused = parent.is_some_and(|parent| cx.page_focused(parent));
        cx.set_page_focused(window, focused);
        // Its page's visibility too.
        cx.set_page_hidden(window, parent.is_some_and(|parent| cx.page_hidden(parent)));
        cx.set_initial_about_blank(document.get(cx.gc));
    });
    state.frames.set(Frame {
        token: Some(token),
        ..Frame::new(
            id,
            global,
            document.clone(),
            document_id,
            Some(parent),
            Some(owner.clone()),
            true,
        )
    });
    // Its name, its iframe's now (HTML gives the attribute to the
    // navigable as it is made alone); whether script made it, as Blink's
    // `is_created_by_script` (script on the stack).
    let name = state.dom.read(|cx| {
        cx.attribute_value_by_id(owner.get(cx.gc), dom::names::local::NAME)
            .map(|name| name.borrow().to_string())
            .unwrap_or_default()
    });
    state.frames.requests.push(FrameRequest::Created {
        frame: id,
        token,
        parent,
        owner: owner.clone(),
        document: document.clone(),
        name,
        created_by_script,
    });
    // Its layout is there for what script reads of it next.
    if let Some(mut layout) = state.layout.take() {
        let (parent_document, owner_id) = state.dom.read(|cx| {
            let owner = owner.get(cx.gc);
            (cx.node_id(cx.document_of(owner)), cx.node_id(owner))
        });
        layout.child_frame_created(id, parent_document, owner_id, document.clone(), document_id);
        state.layout.set(Some(layout));
    }
    Some(document)
}

/// The frame `owner`'s content document is shown in is to navigate: the
/// host told.
pub(crate) fn navigate_child(
    scope: &mut Scope<'_, '_, '_>,
    owner: &NodeRoot,
    navigation: ChildNavigation,
) {
    let (state, _) = scope.state_mut();
    let document = state.dom.read(|cx| {
        cx.content_document(owner.get(cx.gc))
            .map(|document| cx.node_id(document))
    });
    let Some(frame) = document.and_then(|document| state.frames.of_document(document)) else {
        return;
    };
    let frame = frame.id;
    state
        .frames
        .requests
        .push(FrameRequest::Navigate { frame, navigation });
}

/// The frame showing `document` navigated, or is to (`FrameNavigation`),
/// as the frame showing `initiator` asked: the host told. One to another
/// document that a document no frame shows now asked for goes nowhere
/// (HTML navigates from a fully active document alone).
pub(crate) fn navigate(
    scope: &mut Scope<'_, '_, '_>,
    document: &NodeRoot,
    initiator: &NodeRoot,
    navigation: dom::FrameNavigation,
) {
    let (state, _) = scope.state_mut();
    let (id, by) = state.dom.read(|cx| {
        (
            cx.node_id(document.get(cx.gc)),
            cx.node_id(initiator.get(cx.gc)),
        )
    });
    let Some(frame) = state.frames.of_document(id).map(|frame| frame.id) else {
        return;
    };
    let initiator = match state.frames.of_document(by) {
        Some(initiator) => initiator.id,
        None if matches!(navigation, dom::FrameNavigation::To { .. }) => return,
        None => frame,
    };
    state.frames.requests.push(FrameRequest::Navigation {
        frame,
        initiator,
        navigation,
    });
}

/// A blob URL of the realm of the frame showing `document`: the host told.
pub(crate) fn register_blob_url(
    scope: &mut Scope<'_, '_, '_>,
    document: Option<&NodeRoot>,
    url: &str,
    data: &dom::BlobData,
) {
    let (state, _) = scope.state_mut();
    // A worker's global's: its host's.
    let Some(document) = document else {
        if let Some(host) = state.caches.worker_global_host.as_deref() {
            host.register_blob_url(url, data);
        }
        return;
    };
    let id = state.dom.read(|cx| cx.node_id(document.get(cx.gc)));
    let Some(frame) = state.frames.of_document(id).map(|frame| frame.id) else {
        return;
    };
    match &state.frames.load_sink {
        Some(sink) => sink.register_blob_url(frame, url, data),
        None => state.frames.requests.push(FrameRequest::RegisterBlobUrl {
            frame,
            url: url.to_owned(),
            data: data.clone(),
        }),
    }
}

/// The `localStorage` or `sessionStorage` (`kind`) of the frame showing
/// `document`, which the host's storage host opens, waiting for the
/// browser; none without one, or for a document no frame shows.
pub(crate) fn open_storage_area(
    scope: &mut Scope<'_, '_, '_>,
    document: &NodeRoot,
    kind: dom::StorageKind,
) -> Option<dom_api::OpenedStorageArea> {
    let (state, _) = scope.state();
    let id = state.dom.read(|cx| cx.node_id(document.get(cx.gc)));
    let frame = state.frames.of_document(id)?.id;
    let token = state.frames.token(frame)?;
    state.storage_host.as_ref()?.open_storage_area(token, kind)
}

/// `call` on the cookies of `document` at its cookie URL `url`, by the
/// host's cookie host, which waits for the browser; none without one.
pub(crate) fn cookie_call(
    scope: &mut Scope<'_, '_, '_>,
    document: &NodeRoot,
    url: &str,
    call: dom_api::CookieCall,
) -> dom_api::CookieReply {
    let (state, _) = scope.state();
    let id = state.dom.read(|cx| cx.node_id(document.get(cx.gc)));
    state
        .cookie_host
        .as_ref()
        .map_or(dom_api::CookieReply::None, |host| {
            call.on(host.as_ref(), id, url)
        })
}

/// What the operation changed in Web Storage, and the cookies script set:
/// the hosts told, in order; then the windows given `storage` listeners
/// get their areas.
pub(crate) fn send_storage_changes(scope: &mut Scope<'_, '_, '_>) {
    let (state, _) = scope.state_mut();
    let (changes, cookies) = state
        .dom
        .mutate(|cx| (cx.take_storage_changes(), cx.take_cookie_writes()));
    if let Some(host) = &state.storage_host {
        for change in changes {
            host.storage_changed(change);
        }
    }
    if let Some(host) = &state.cookie_host {
        for write in cookies {
            host.set_cookie(write.document, &write.url, &write.line);
        }
    }
    dom_api::open_for_listeners(&mut crate::listeners::Host { scope });
}

/// A blob URL revoked: the host told.
pub(crate) fn revoke_blob_url(scope: &mut Scope<'_, '_, '_>, url: &str) {
    let (state, _) = scope.state_mut();
    // A worker's: its host's.
    if let Some(host) = state.caches.worker_global_host.as_deref() {
        host.revoke_blob_url(url);
        return;
    }
    match &state.frames.load_sink {
        Some(sink) => sink.revoke_blob_url(url),
        None => state.frames.requests.push(FrameRequest::RevokeBlobUrl {
            url: url.to_owned(),
        }),
    }
}

/// The `fetch()`es and aborts the operation asked for, handed to the host
/// before the call returns: each `fetch()` sent by its load sink now,
/// what it sent told after (`FrameRequest::FetchSent`); without a sink,
/// they wait for the host's next look (`take_frame_requests`).
pub(crate) fn send_fetches(scope: &mut Scope<'_, '_, '_>) {
    let (state, _) = scope.state_mut();
    // A worker's: sent by its host now.
    if let Some(host) = state.caches.worker_global_host.as_deref() {
        for fetch in state.dom.mutate(|cx| cx.take_fetch_requests()) {
            match fetch {
                dom::FetchHostRequest::Start(request) => host.start_fetch(&request),
                dom::FetchHostRequest::Cancel(id) => host.cancel_fetch(id),
            }
        }
        return;
    }
    if state.frames.load_sink.is_none() {
        return;
    }
    let fetches = state.dom.mutate(|cx| cx.take_fetch_requests());
    for fetch in fetches {
        let request = match fetch {
            dom::FetchHostRequest::Start(request) => {
                let Some(frame) = request
                    .document
                    .and_then(|document| state.frames.of_document(document))
                    .map(|frame| frame.id)
                else {
                    continue;
                };
                let request_id = state
                    .frames
                    .load_sink
                    .as_ref()
                    .and_then(|sink| sink.start_fetch(frame, &request));
                FrameRequest::FetchSent {
                    frame,
                    request,
                    request_id,
                }
            }
            dom::FetchHostRequest::Cancel(id) => FrameRequest::CancelFetch { id },
        };
        state.frames.requests.push(request);
    }
}

/// The frame showing `document` went, with those in it: the host told.
pub(crate) fn discard_child(scope: &mut Scope<'_, '_, '_>, document: &NodeRoot) {
    let (state, _) = scope.state_mut();
    let id = state.dom.read(|cx| cx.node_id(document.get(cx.gc)));
    let Some((frame, token)) = state
        .frames
        .of_document(id)
        .and_then(|frame| Some((frame.id, frame.token?)))
    else {
        return;
    };
    let removed = state.frames.remove(frame);
    terminate_workers(state, removed.iter().map(|frame| &frame.document));
    if let Some(sink) = &state.frames.load_sink {
        for removed in &removed {
            sink.frame_gone(removed.id);
        }
    }
    state
        .frames
        .requests
        .push(FrameRequest::Removed { frame, token });
    // Their realms have gone for the inspector too.
    for removed in &removed {
        crate::devtools::realm_destroyed(scope.scope(), &removed.context);
    }
}

/// The dedicated workers of `documents`' windows, terminated: their
/// documents gone (HTML terminates a discarded document's workers, Blink's
/// `DedicatedWorker::ContextDestroyed`); their `Worker`s get nothing more.
pub(crate) fn terminate_workers<'a>(
    state: &mut crate::state::State,
    documents: impl IntoIterator<Item = &'a NodeRoot>,
) {
    let crate::state::State { caches, dom, .. } = state;
    let host = caches.worker_host.as_deref();
    dom.mutate(|cx| {
        for document in documents {
            let Some(window) = cx.window_of(document.get(cx.gc)) else {
                continue;
            };
            for worker in cx.context_workers(cx.window_context(window)) {
                if let Some(host) = host {
                    host.terminate_worker(cx.worker_id(worker));
                }
                cx.terminate_worker_object(worker);
            }
        }
    });
}

/// The frames inside child frame `frame`'s document, as it navigates to
/// another (Blink's `FrameLoader::DetachChildren`): gone as a removed
/// iframe's are, `frame` itself kept.
pub(crate) fn discard_descendants(scope: &mut Scope<'_, '_, '_>, frame: FrameId) {
    let (state, _) = scope.state_mut();
    let removed = state.frames.remove_descendants(frame);
    terminate_workers(state, removed.iter().map(|frame| &frame.document));
    if let Some(sink) = &state.frames.load_sink {
        for removed in &removed {
            sink.frame_gone(removed.id);
        }
    }
    // The renderer takes each child's frames out with it.
    for child in removed
        .iter()
        .filter(|removed| removed.parent == Some(frame))
    {
        if let Some(token) = child.token {
            state.frames.requests.push(FrameRequest::Removed {
                frame: child.id,
                token,
            });
        }
    }
    for removed in &removed {
        crate::devtools::realm_destroyed(scope.scope(), &removed.context);
    }
}

/// A realm's V8 security token: its document's origin, so that realms of
/// the same origin reach each other's objects and the others get a
/// `TypeError` from V8's access check (as across processes later; Blink
/// sets the token from the document's security origin). A URL with an
/// opaque origin gets a new one; an `about:` document of a child frame
/// inherits its parent's (`parent`, that realm).
pub(crate) fn set_security_token(
    scope: &mut Scope<'_, '_, '_>,
    context: v8::Local<'_, v8::Context>,
    url: &str,
    parent: Option<&v8::Global<v8::Context>>,
) {
    let inherits = url.starts_with("about:");
    if inherits && let Some(parent) = parent {
        let parent = v8::Local::new(scope.scope(), parent);
        let token = parent.get_security_token(scope.scope());
        context.set_security_token(token);
        return;
    }
    set_origin_token(
        scope,
        context,
        url::Url::parse(url).ok().map(|url| url.origin()),
    );
}

/// `context`'s security token: `origin`'s (a new one for an opaque
/// origin, as for none).
pub(crate) fn set_origin_token(
    scope: &mut Scope<'_, '_, '_>,
    context: v8::Local<'_, v8::Context>,
    origin: Option<url::Origin>,
) {
    let text = match origin {
        Some(origin) if origin.is_tuple() => origin.ascii_serialization(),
        // An opaque origin's text has a space, which no tuple origin's has.
        _ => {
            let (state, _) = scope.state_mut();
            // One for each document with an opaque origin: fewer than
            // u64::MAX.
            state.frames.opaque = state.frames.opaque.saturating_add(1);
            format!("opaque {}", state.frames.opaque)
        }
    };
    let scope = scope.scope();
    // V8 compares tokens by identity: an internalized string is the same
    // object for the same text, in every context of the isolate.
    let token = v8::String::new_from_utf8(scope, text.as_bytes(), v8::NewStringType::Internalized)
        .expect("a string V8 can hold");
    context.set_security_token(token.into());
}

/// `script` run in the realm of the frame `document` is the active
/// document of (HTML's "run a classic script", as a script script inserted
/// runs: in its document's browsing context, which may not be the caller's
/// realm), what it throws reported at that frame's window; nothing if no
/// frame shows `document`.
pub(crate) fn run_classic_script(
    scope: &mut Scope<'_, '_, '_>,
    document: &NodeRoot,
    script: &dom_api::ClassicScript<'_>,
) {
    if crate::listeners::stopped(scope.scope()) {
        return;
    }
    let realm = {
        let (state, _) = scope.state();
        let id = state.dom.read(|cx| cx.node_id(document.get(cx.gc)));
        state
            .frames
            .of_document(id)
            .map(|frame| frame.context.clone())
    };
    let Some(realm) = realm else {
        return;
    };
    let scope = scope.scope();
    let context = v8::Local::new(scope, &realm);
    let scope = &mut v8::ContextScope::new(scope, context);
    let global = context.global(scope);
    v8::tc_scope!(let scope, scope);
    let (Some(source), Some(name)) = (
        v8::String::new(scope, script.source),
        v8::String::new(scope, script.url),
    ) else {
        return;
    };
    let line_offset = i32::try_from(script.line.saturating_sub(1)).unwrap_or(i32::MAX);
    // Muted, it isn't "shared cross-origin": V8's messages of its errors
    // say so, and they are reported sanitized (`listeners::Thrown`).
    // Its base URL and fetch options, which an `import()` in it takes.
    let options =
        bindings::script_options(scope, import_base_url(script), Some(credentials_of(script)));
    let origin = v8::ScriptOrigin::new(
        scope,
        name.into(),
        line_offset,
        0,
        !script.muted,
        0,
        None,
        false,
        false,
        false,
        options,
    );
    // Chrome's trace's span of a classic script (Blink's
    // `ClassicScript::RunScript`).
    let span = trace_event::trace_event!("EvaluateScript");
    let ran =
        v8::Script::compile(scope, source, Some(&origin)).and_then(|script| script.run(scope));
    drop(span);
    if ran.is_none() {
        crate::listeners::report_caught(scope, crate::listeners::ReportAt::Global(global));
    }
}

/// The base URL a classic script's `import()` resolves against: muted,
/// `about:blank` (HTML's "create a classic script": nothing of another
/// origin's URL to resolve against); else its base URL.
pub(crate) fn import_base_url<'a>(script: &dom_api::ClassicScript<'a>) -> &'a str {
    if script.muted {
        "about:blank"
    } else {
        script.base_url.unwrap_or(script.url)
    }
}

/// A classic script's fetch options' credentials mode, as its host-defined
/// options carry it (`modules::import_dynamically` reads it back).
pub(crate) fn credentials_of(script: &dom_api::ClassicScript<'_>) -> &'static str {
    if script.use_credentials {
        "include"
    } else {
        "same-origin"
    }
}
