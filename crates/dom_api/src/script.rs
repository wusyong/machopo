//! HTML's "prepare the script element" (Blink's `ScriptLoader::PrepareScript`):
//! whether a script element runs, and when, for the parser's scripts (the
//! renderer prepares them as the parser stops at one) and for those script
//! inserts (`update_script`, once the operation that inserted it is over:
//! the DOM recorded it, `dom`'s `script`). It reads and writes the
//! element's flags (`ScriptFlags`); what it decides is the caller's to do:
//! run, fetch, or (one that waits for no parser) the DOM's queue
//! (`Cx::queue_script`), whose scripts run in element tasks of their own
//! (`run_script_task`). Classic scripts only: a module script's type is
//! one that doesn't run yet. And `HTMLScriptElement`'s members.

use dom::{
    Cx, NodeId, NodeRef, NodeRoot, ScriptCors, ScriptFlags, ScriptRequest, ScriptResult,
    TargetRoot, names,
};

use crate::typed::DomStr;
use crate::{ApiCx, ApiError, ClassicScript, ScriptHost};

/// What "prepare the script element" makes of a script element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prepared {
    /// Nothing runs: already started, not connected, empty, not a classic
    /// script, `nomodule`, `for` and `event` not `window`'s `onload`, its
    /// document shown in no browsing context; or its `src` was empty or
    /// didn't parse (an `error` event queued).
    Nothing,
    /// An inline classic script: runs now (the parser's, once no
    /// stylesheet blocks scripts), its text, and its base URL, its
    /// document's as it was prepared (what an `import()` in it resolves
    /// against; none if the document has none).
    Inline {
        source: String,
        base_url: Option<String>,
    },
    /// The parser's external script without `defer` or `async`: the parser
    /// waits for it ("parser-blocking").
    Blocking(ScriptRequest),
    /// The parser's with `defer`: after parsing, in document order.
    Defer(ScriptRequest),
    /// An external one that waits for no parser (script inserted it, or
    /// it is the parser's `async` one): queued in the DOM to fetch and to
    /// run as soon as it can, or in order (`Cx::queue_script`).
    Queued,
    /// A module script, queued in the DOM (`Cx::queue_module_script`):
    /// its graph for the engine to fetch (`ScriptHost::start_module_graph`).
    Module(ModuleStart),
}

/// A module script's graph to fetch: its pending script's number (whose
/// fetch it is), its root, its modules' credentials mode, and whether it
/// is the parser's deferred one (the parser runs it, in its list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleStart {
    pub key: u64,
    pub source: ModuleSource,
    pub credentials: ScriptCors,
    pub deferred: bool,
}

/// A module graph's root: from a URL (resolved), or inline (its text, its
/// document's URL, the line it starts on; its base URL the document's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleSource {
    External(String),
    Inline {
        source: String,
        url: String,
        line: u32,
    },
}

/// HTML's "prepare the script element" for `script`, in an operation: it
/// sets the element's flags as HTML says (one prepared, of a type that
/// runs, is already started and never prepared again; a parser-inserted
/// one that turns out empty, or of a type that doesn't run, loses its
/// parser document, so that what script later inserts into it runs, as
/// soon as it can), and queues an `error` event for an empty `src` or one
/// that doesn't parse. The parser's document is the one its scripts are
/// in (the renderer prepares a parser's scripts in its document).
pub fn prepare_script<'gc>(cx: &Cx<'_, 'gc>, script: NodeRef<'gc>) -> Prepared {
    let mut flags = cx.script_flags(script);
    if flags.already_started {
        return Prepared::Nothing;
    }
    let prepared = prepare_with(cx, script, &mut flags);
    cx.set_script_flags(script, flags);
    prepared
}

/// The steps of `prepare_script` past "already started", on `flags`.
fn prepare_with<'gc>(cx: &Cx<'_, 'gc>, script: NodeRef<'gc>, flags: &mut ScriptFlags) -> Prepared {
    let attr = |name| {
        cx.attribute_value_by_id(script, name)
            .map(|value| value.borrow().to_str().into_owned())
    };
    // The parser document goes, for good if this returns before the type
    // is known: script changing the element prepares it then.
    let parser_inserted = std::mem::take(&mut flags.parser_inserted);
    let is_async = attr(names::local::ASYNC).is_some();
    if parser_inserted && !is_async {
        flags.non_blocking = true;
    }
    let src = attr(names::local::SRC);
    let source = cx.child_text_content(script);
    if src.is_none() && source.is_empty() {
        return Prepared::Nothing;
    }
    // A template's contents aren't connected, so their scripts are inert.
    if !cx.is_connected(script) {
        return Prepared::Nothing;
    }
    let type_attr = attr(names::local::TYPE);
    let module = !is_classic(
        type_attr.as_deref(),
        attr(names::local::LANGUAGE).as_deref(),
    );
    if module && !type_attr.as_deref().is_some_and(is_module_type) {
        return Prepared::Nothing;
    }
    if parser_inserted {
        flags.parser_inserted = true;
        flags.non_blocking = false;
    }
    flags.already_started = true;
    // Scripting is disabled for a document shown in no browsing context.
    let document = cx.document_of(script);
    if cx.window_of(document).is_none() || cx.is_discarded(document) {
        return Prepared::Nothing;
    }
    // A browser that runs modules leaves the fallback for those that
    // don't.
    if !module && attr(names::local::NOMODULE).is_some() {
        return Prepared::Nothing;
    }
    // The old way of running a classic script only for `window`'s `load`.
    if !module
        && let (Some(event), Some(target)) = (attr(names::local::EVENT), attr(names::local::FOR))
    {
        let trim = |value: &str| {
            value
                .trim_matches(|c: char| c.is_ascii_whitespace())
                .to_owned()
        };
        let (event, target) = (trim(&event), trim(&target));
        if !target.eq_ignore_ascii_case("window")
            || !(event.eq_ignore_ascii_case("onload") || event.eq_ignore_ascii_case("onload()"))
        {
            return Prepared::Nothing;
        }
    }
    // HTML's "module script credentials mode", and a classic script's
    // CORS settings.
    let cors = attr(names::local::CROSSORIGIN).map(|value| {
        if value.eq_ignore_ascii_case("use-credentials") {
            ScriptCors::UseCredentials
        } else {
            ScriptCors::Anonymous
        }
    });
    let Some(src) = src else {
        if !module {
            return Prepared::Inline {
                source: source.to_string(),
                base_url: cx.document_base_url(document).map(String::from),
            };
        }
        let url = cx
            .document_url(document)
            .map(|url| url.to_string())
            .unwrap_or_default();
        return queue_module(
            cx,
            script,
            flags,
            is_async,
            ModuleSource::Inline {
                source: source.to_string(),
                url,
                line: 1,
            },
            cors.unwrap_or(ScriptCors::Anonymous),
        );
    };
    // HTML queues an `error` event for an empty `src` (stripped of ASCII
    // whitespace first, as Blink's `ScriptLoader::PrepareScript` does), or
    // one that doesn't parse against the document's base URL.
    let src = src.trim_matches(|c: char| c.is_ascii_whitespace());
    let url = (!src.is_empty())
        .then(|| cx.resolve_url(document, src))
        .flatten();
    let Some(url) = url else {
        cx.queue_element_task(script, names::event::ERROR);
        return Prepared::Nothing;
    };
    if module {
        return queue_module(
            cx,
            script,
            flags,
            is_async,
            ModuleSource::External(url.into()),
            cors.unwrap_or(ScriptCors::Anonymous),
        );
    }
    let request = ScriptRequest {
        url: url.into(),
        cors,
        charset: attr(names::local::CHARSET),
    };
    // HTML's step that picks the list it waits in.
    if flags.parser_inserted && !is_async {
        return if attr(names::local::DEFER).is_some() {
            Prepared::Defer(request)
        } else {
            Prepared::Blocking(request)
        };
    }
    cx.queue_script(script, request, !is_async && !flags.non_blocking);
    Prepared::Queued
}

/// Whether `script`'s fetch options' credentials mode is include (HTML's
/// CORS settings attribute credentials mode: its `crossorigin` is
/// `use-credentials`), what an `import()` in its classic script fetches
/// with.
pub fn uses_credentials<'gc>(cx: &Cx<'_, 'gc>, script: NodeRef<'gc>) -> bool {
    cx.attribute_value_by_id(script, names::local::CROSSORIGIN)
        .is_some_and(|value| {
            value
                .borrow()
                .to_str()
                .eq_ignore_ascii_case("use-credentials")
        })
}

/// A module script queued in the DOM (`Cx::queue_module_script`): the
/// parser's without `async` is deferred (its list the parser's), any other
/// runs as soon as it can, or in order; its graph the engine's to fetch.
fn queue_module<'gc>(
    cx: &Cx<'_, 'gc>,
    script: NodeRef<'gc>,
    flags: &ScriptFlags,
    is_async: bool,
    source: ModuleSource,
    credentials: ScriptCors,
) -> Prepared {
    let external = matches!(source, ModuleSource::External(_));
    let deferred = flags.parser_inserted && !is_async;
    let in_order = !deferred && !is_async && !flags.non_blocking;
    let key = cx.queue_module_script(script, in_order, deferred, external);
    Prepared::Module(ModuleStart {
        key,
        source,
        credentials,
        deferred,
    })
}

/// A module script's type: `module`, ASCII case-insensitively, without
/// surrounding whitespace.
fn is_module_type(type_attr: &str) -> bool {
    type_attr
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .eq_ignore_ascii_case("module")
}

/// The script's type is a classic script's: HTML's "the script block's
/// type string" is a JavaScript MIME type essence match. An empty `type`
/// (or `language` without `type`, or neither) is JavaScript; a `type` of
/// whitespace isn't: stripped, it is empty, no MIME type (Blink strips it
/// only once it isn't empty, too).
fn is_classic(type_attr: Option<&str>, language: Option<&str>) -> bool {
    let type_string = match (type_attr, language) {
        (Some(""), _) | (None, Some("") | None) => return true,
        (Some(kind), _) => kind
            .trim_matches(|c: char| c.is_ascii_whitespace())
            .to_owned(),
        (None, Some(language)) => format!("text/{language}"),
    };
    is_javascript_mime_type(&type_string)
}

/// MIME Sniffing's JavaScript MIME type essences, ASCII case-insensitively.
pub fn is_javascript_mime_type(essence: &str) -> bool {
    const ESSENCES: &[&str] = &[
        "application/ecmascript",
        "application/javascript",
        "application/x-ecmascript",
        "application/x-javascript",
        "text/ecmascript",
        "text/javascript",
        "text/javascript1.0",
        "text/javascript1.1",
        "text/javascript1.2",
        "text/javascript1.3",
        "text/javascript1.4",
        "text/javascript1.5",
        "text/jscript",
        "text/livescript",
        "text/x-ecmascript",
        "text/x-javascript",
    ];
    ESSENCES
        .iter()
        .any(|known| known.eq_ignore_ascii_case(essence))
}

/// A script element script inserted, or changed, prepared (the DOM
/// recorded it: `ElementChange::Script`); an inline classic one runs now
/// (HTML's "execute the script element": in its document's browsing
/// context, from the document's URL, its line 1 as Blink counts them,
/// `currentScript` it meanwhile unless it is in a shadow tree, what it
/// throws reported there). An external one was queued in the DOM.
pub(crate) fn update_script<H: ScriptHost>(host: &mut H, script: NodeId) {
    let ran = host.mutate(|cx| {
        let script = cx.node_by_id(script)?;
        let (source, base_url) = match prepare_script(cx, script) {
            Prepared::Inline { source, base_url } => (source, base_url),
            Prepared::Module(start) => return Some(Err((cx.root(cx.document_of(script)), start))),
            _ => return None,
        };
        let document = cx.document_of(script);
        let url = cx
            .document_url(document)
            .map(|url| url.to_string())
            .unwrap_or_default();
        let current = cx
            .containing_shadow_root(script)
            .is_none()
            .then_some(script);
        let before = cx
            .set_current_script(document, current)
            .map(|before| cx.root(before));
        Some(Ok((
            cx.root(document),
            before,
            source,
            url,
            base_url,
            uses_credentials(cx, script),
        )))
    });
    let (document, before, source, url, base_url, use_credentials) = match ran {
        Some(Ok(inline)) => inline,
        Some(Err((document, start))) => {
            host.start_module_graph(&document, start);
            return;
        }
        None => return,
    };
    host.run_classic_script(
        &document,
        &ClassicScript {
            source: &source,
            url: &url,
            base_url: base_url.as_deref(),
            line: 1,
            muted: false,
            use_credentials,
        },
    );
    // The document it was set on gets it back, if the script moved the
    // element meanwhile.
    host.mutate(|cx| {
        cx.set_current_script(
            document.get(cx.gc),
            before.as_ref().map(|before| before.get(cx.gc)),
        );
    });
}

/// An external script whose turn came (`ElementTaskKind::Script`),
/// executed (HTML's "execute the script element"): nothing if its element
/// moved to another document since it was prepared; its `error` if the
/// fetch failed; else run in its document's browsing context (a classic
/// one from its response's URL, `currentScript` it meanwhile unless in a
/// shadow tree; a module's graph), a microtask checkpoint, then its
/// `load` if it is external.
pub fn run_script_task<H: ScriptHost>(host: &mut H, element: &NodeRoot) {
    let ready = host.mutate(|cx| {
        let script = element.get(cx.gc);
        let ready = cx.take_ready_script(script)?;
        let document = cx.document_of(script);
        (cx.node_id(ready.document) == cx.node_id(document))
            .then(|| (cx.root(document), ready.loaded, ready.external))
    });
    let Some((document, loaded, external)) = ready else {
        return;
    };
    let target = TargetRoot::Node(element.clone());
    let Some(loaded) = loaded else {
        crate::fire_event(host, &target, names::event::ERROR, false, false);
        return;
    };
    match loaded {
        // HTML's "run a module script" (`currentScript` stays null).
        ScriptResult::Module(root) => host.run_module_script(&document, root),
        ScriptResult::Classic(loaded) => {
            let (before, use_credentials) = host.mutate(|cx| {
                let script = element.get(cx.gc);
                let current = cx
                    .containing_shadow_root(script)
                    .is_none()
                    .then_some(script);
                let before = cx
                    .set_current_script(document.get(cx.gc), current)
                    .map(|before| cx.root(before));
                (before, uses_credentials(cx, script))
            });
            host.run_classic_script(
                &document,
                &ClassicScript {
                    source: &loaded.source,
                    url: &loaded.url,
                    base_url: None,
                    line: 1,
                    muted: loaded.muted,
                    use_credentials,
                },
            );
            host.mutate(|cx| {
                cx.set_current_script(
                    document.get(cx.gc),
                    before.as_ref().map(|before| before.get(cx.gc)),
                );
            });
        }
    }
    // HTML's "clean up after running script": the stack is empty in a
    // task, so a microtask checkpoint (a module's rejection reported in
    // it), before its `load`.
    host.perform_microtask_checkpoint();
    if external {
        crate::fire_event(host, &target, names::event::LOAD, false, false);
    }
}

// --- HTMLScriptElement (https://html.spec.whatwg.org/#htmlscriptelement)

/// `async`: true while the element is non-blocking (script made it, and
/// it runs as soon as it can), or has the attribute.
pub(crate) fn script_async<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<bool, ApiError> {
    Ok(cx.script_flags(this).non_blocking
        || cx
            .attribute_value_by_id(this, names::local::ASYNC)
            .is_some())
}

/// Setting `async` ends its being non-blocking, either way: `async =
/// false` makes a script that script inserts wait for those inserted
/// before it.
pub(crate) fn set_script_async<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: bool,
) -> Result<(), ApiError> {
    let flags = cx.script_flags(this);
    cx.set_script_flags(
        this,
        ScriptFlags {
            non_blocking: false,
            ..flags
        },
    );
    crate::reflect::set_boolean::<{ names::local::ASYNC }>(cx, this, value)
}

/// `text`: the element's child text content.
pub(crate) fn script_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(cx.child_text_content(this)))
}

/// Setting `text` is "string replace all": as `textContent`.
pub(crate) fn set_script_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_text_content(this, value);
    Ok(())
}

/// `HTMLScriptElement.supports()`: the script types that run (classic and
/// module scripts; no import maps, nor speculation rules).
pub(crate) fn script_supports(_: &ApiCx<'_, '_>, kind: &str) -> Result<bool, ApiError> {
    Ok(kind == "classic" || kind == "module")
}
