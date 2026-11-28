//! A frame's document as it loads, the engine's side of it (Blink's
//! `DocumentLoader`, and its parser's `HTMLParserScriptRunner`): its
//! body's bytes as the host reads them from the browser's pipe decoded
//! (HTML's encoding sniffing, `encoding`), turned into markup by its type
//! (`MarkupFilter`), and given to its parser a slice at a time; the parser
//! run till a script must wait, the input is spent, or its run's budget
//! is (`parse`), each script it stops at prepared and run: an inline one at
//! once, an external one once fetched (a pending script of the DOM's,
//! fetched as the others are, `Cx::queue_parser_script`), neither while a
//! sheet blocks scripts; the deferred ones after parsing
//! (`run_deferred_scripts`). The engine keeps one with each frame's
//! document, and lends itself to the parser for each run: the parser
//! takes its host by value, as its sink holds it across the tree
//! builder's calls.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use dom::{Dom, NodeRoot};
use html::parser::{Parser, ParserState};

use crate::encoding::DocumentDecoder;
use crate::{ClassicScript, FrameId, Prepared, ScriptEngine};

/// A frame's document's loading: its decoder, its markup's filter, what
/// its parser hasn't been given yet, its parser, and its text so far; the
/// parser's script it waits for, and those to run after parsing.
pub struct DocumentLoader {
    document: NodeRoot,
    decoder: DocumentDecoder,
    markup: MarkupFilter,
    input: ParserInput,
    /// None while a run has it, and once it is finished.
    parser: Option<Parser<dyn ScriptEngine>>,
    /// The body has all come (its load ended): once its input is spent,
    /// the parser finishes; aborted if it stopped short (an error, or its
    /// load was stopped).
    ended: bool,
    aborted: bool,
    /// Its scripts are wasm components (a wasm document's).
    wasm: bool,
    /// The parser has made the body: a run lets other work go only after.
    body_parsed: bool,
    /// HTML's "pending parsing-blocking script", and its "list of scripts
    /// that will execute when the document has finished parsing".
    blocking: Option<ParserScript>,
    deferred: VecDeque<ParserScript>,
    /// The document's source so far (DevTools' `Page.getResourceContent`).
    text: String,
    /// How many times its parser let other work run, and the task it posted
    /// to parse on in (`Cx::queue_parse_on`) while it is to.
    yields: u32,
    parse_task: Option<u64>,
    /// The title last told, and the tree's version it was read at
    /// (`tell_title`).
    told_title: Option<String>,
    title_version: u64,
}

/// A script of the parser's: an inline one's element and text (the line
/// it starts on, its base URL as it was prepared), or an external one's
/// element, whose pending script the DOM has.
enum ParserScript {
    Inline {
        element: NodeRoot,
        source: String,
        line: u32,
        base_url: Option<String>,
    },
    Pending {
        element: NodeRoot,
        kind: PendingKind,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingKind {
    Classic,
    Module,
    Wasm,
}

impl ParserScript {
    fn element(&self) -> &NodeRoot {
        match self {
            Self::Inline { element, .. } | Self::Pending { element, .. } => element,
        }
    }
}

/// A run of the parser's limits, the host's: its budget after so many
/// times it let other work run (Blink's `HTMLParserScheduler`'s), what is
/// left of it after a script in the body runs, the most bytes given to
/// the parser at a time, and whether input waits (the run lets it go
/// first, once the body is: the host's scheduler's, Blink's
/// `ShouldYieldForHighPriorityWork`).
pub struct ParseRun<'a> {
    pub budget: &'a dyn Fn(u32) -> Duration,
    pub after_script: Duration,
    pub slice: usize,
    pub input_waits: &'a dyn Fn() -> bool,
}

/// One of this module's functions that run a frame's document's parser
/// (`parse`, `parse_unless_due`), the engine lent to it and given back.
pub type ParseEntry =
    fn(Box<dyn ScriptEngine>, FrameId, &ParseRun<'_>) -> (ParseOutcome, Box<dyn ScriptEngine>);

/// Where a run of the parser stopped (`parse`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseOutcome {
    /// Nothing to parse: no parser, or it finished before.
    Stopped,
    /// The script it waits for isn't fetched yet, or a sheet blocks it.
    WaitsForScript,
    /// Its input is spent, the rest of its body still to come.
    WaitsForBody,
    /// Its budget is spent, or input waits: it parses on in a task of its
    /// own, posted (Blink's `SchedulePumpTokenizer`: `Cx::queue_parse_on`).
    Yielded,
    /// It finished now: the document is parsed.
    Parsed,
}

impl DocumentLoader {
    /// The loading of `document` (a new one, its frame's, `srcdoc`: an
    /// iframe's `srcdoc` document) of `mime_type`, whose transport said
    /// `charset`: decoded as its body's `start` says (`more`: more may come
    /// after), none of it given yet.
    #[must_use]
    pub fn new(
        dom: &Dom,
        document: &NodeRoot,
        srcdoc: bool,
        mime_type: &str,
        charset: Option<&str>,
        start: &[u8],
        more: bool,
    ) -> Self {
        // Scripting as the document is sandboxed (its host sandboxes it
        // before it is loaded).
        let scripting = dom.read(|cx| !crate::frame::scripting_disabled(cx, document.get(cx.gc)));
        let parser = Parser::with_scripting(dom, document, srcdoc, scripting);
        Self {
            document: document.clone(),
            decoder: DocumentDecoder::new(mime_type, charset, start, more),
            markup: MarkupFilter::new(mime_type),
            input: ParserInput::default(),
            parser: Some(parser),
            ended: false,
            aborted: false,
            wasm: false,
            body_parsed: false,
            blocking: None,
            deferred: VecDeque::new(),
            text: String::new(),
            yields: 0,
            parse_task: None,
            told_title: None,
            title_version: 0,
        }
    }

    /// The same, whose scripts are wasm components (a wasm document's).
    #[must_use]
    pub fn with_wasm_scripts(self) -> Self {
        Self { wasm: true, ..self }
    }

    /// What its text is read in: its sheets' and scripts' default.
    #[must_use]
    pub fn encoding(&self) -> &'static encoding_rs::Encoding {
        self.decoder.encoding()
    }

    /// The body's next `bytes` (`last`: the end of it), decoded and kept:
    /// its markup at the end of the parser's input.
    pub fn body_data(&mut self, bytes: &[u8], last: bool) {
        let text = self.decoder.decode(bytes, last);
        let markup = self.markup.push(&text, last);
        self.text.push_str(&text);
        self.input.push(markup);
    }

    /// The body has all come and its load ended (Blink's
    /// `DocumentLoader::BodyLoadingFinished`), stopped short if `aborted`:
    /// the parser finishes once it parsed what came, then the document is
    /// parsed or its parser aborted (HTML's "abort a parser").
    pub fn body_ended(&mut self, aborted: bool) {
        self.ended = true;
        self.aborted = aborted;
    }

    /// The document's source so far.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether the parser waits for a script (one a sheet may block).
    #[must_use]
    pub fn waits_for_script(&self) -> bool {
        self.blocking.is_some()
    }
}

/// `frame`'s document's parser run (`engine` lent to it, and given back)
/// as far as `run` lets it: the sheets of the `<link>`s it made asked for
/// before each script it stopped at runs (so they block it), the script it
/// waits for run once it is ready, then what came parsed a slice at a
/// time, each script it stops at prepared. Where it stopped.
pub fn parse(
    mut engine: Box<dyn ScriptEngine>,
    frame: FrameId,
    run: &ParseRun<'_>,
) -> (ParseOutcome, Box<dyn ScriptEngine>) {
    let started = Instant::now();
    // This run parses whatever its task was to: the task, as it comes,
    // finds it parsed.
    let Some(loader) = engine.document_loader(frame) else {
        return (ParseOutcome::Stopped, engine);
    };
    loader.parse_task = None;
    let mut budget = (run.budget)(loader.yields);
    // Whether the run gave the parser a slice: it lets other work go only
    // after (Blink's, after a token).
    let mut gave = false;
    loop {
        engine.send_sheet_fetches();
        let Some(loader) = engine.document_loader(frame) else {
            return (ParseOutcome::Stopped, engine);
        };
        let document = loader.document.clone();
        if let Some(script) = &loader.blocking {
            let element = script.element().clone();
            let inline = matches!(script, ParserScript::Inline { .. });
            if !script_ready(engine.dom_mut(), &document, &element, inline) {
                return (ParseOutcome::WaitsForScript, engine);
            }
            let Some(script) = engine
                .document_loader(frame)
                .and_then(|loader| loader.blocking.take())
            else {
                return (ParseOutcome::Stopped, engine);
            };
            execute(engine.as_mut(), frame, &document, script);
            // Having run a script in the body, it is to let other work go
            // soon (Blink's `kHaveTokensAfterScript`).
            if body_parsed(engine.as_mut(), frame) {
                budget = budget.min(started.elapsed().saturating_add(run.after_script));
            }
            continue;
        }
        let Some(mut parser) = loader.parser.take() else {
            return (ParseOutcome::Stopped, engine);
        };
        // The engine lent to the parser for its run (it constructs the
        // custom elements it defines at once), and given back.
        let (state, lent) = parser.resume(engine);
        engine = lent;
        let Some(loader) = engine.document_loader(frame) else {
            return (ParseOutcome::Stopped, engine);
        };
        loader.parser = Some(parser);
        match state {
            ParserState::NeedsInput => {
                let has_input = !loader.input.is_empty();
                let ended = loader.ended;
                if has_input {
                    // Its run's budget spent, or input waiting, it lets other
                    // work go, once it has its body (Blink's `PumpTokenizer`);
                    // else the next slice.
                    if gave
                        && body_parsed(engine.as_mut(), frame)
                        && (started.elapsed() >= budget || (run.input_waits)())
                    {
                        let task = engine
                            .dom_mut()
                            .mutate(|cx| cx.queue_parse_on(document.get(cx.gc)));
                        if let Some(loader) = engine.document_loader(frame) {
                            loader.yields = loader.yields.saturating_add(1);
                            loader.parse_task = task;
                        }
                        return (ParseOutcome::Yielded, engine);
                    }
                    gave = true;
                    if let Some(loader) = engine.document_loader(frame)
                        && let Some(parser) = &mut loader.parser
                    {
                        loader.input.give(run.slice, |text| parser.push(text));
                    }
                    continue;
                }
                if !ended {
                    return (ParseOutcome::WaitsForBody, engine);
                }
                let Some(parser) = engine
                    .document_loader(frame)
                    .and_then(|loader| loader.parser.take())
                else {
                    return (ParseOutcome::Stopped, engine);
                };
                let mut engine = parser.finish(engine);
                parsed(engine.as_mut(), frame, &document);
                return (ParseOutcome::Parsed, engine);
            }
            ParserState::Script { element, line } => {
                let element = element.root().clone();
                // The title parsed so far goes before the script runs
                // (Blink tells it as its `<title>` ends; a script may set
                // another at once).
                engine.tell_title();
                // HTML's "an end tag whose tag name is script": a microtask
                // checkpoint first, so the mutation observers hear of what
                // the parser inserted before the script is prepared
                // (Blink's `HTMLParserScriptRunner::ProcessScriptElement`).
                engine.perform_microtask_checkpoint();
                let line = u32::try_from(line).unwrap_or(u32::MAX);
                stopped_at_script(engine.as_mut(), frame, &document, element, line);
            }
        }
    }
}

/// `frame`'s document's parser run (`parse`) unless it is to parse on in a
/// task it posted (Blink's parser, once scheduled, isn't pumped by what
/// comes): what came of its body, a script it waited for.
pub fn parse_unless_due(
    engine: Box<dyn ScriptEngine>,
    frame: FrameId,
    run: &ParseRun<'_>,
) -> (ParseOutcome, Box<dyn ScriptEngine>) {
    let mut engine = engine;
    if engine
        .document_loader(frame)
        .is_some_and(|loader| loader.parse_task.is_some())
    {
        return (ParseOutcome::Stopped, engine);
    }
    parse(engine, frame, run)
}

/// The DOM's task numbered `number`, as the host's scheduler took its token
/// (`ScriptEngine::run_dom_task`): a document's parser's task to parse on
/// in, run here (`parse`, the engine lent to the parser), the task taken
/// out; any other, the engine's. Whether it ran.
pub fn run_dom_task(
    mut engine: Box<dyn ScriptEngine>,
    number: u64,
    run: &ParseRun<'_>,
) -> (bool, Box<dyn ScriptEngine>) {
    // Every DOM task comes here: the engine's list of frames, not a walk of
    // the documents' trees.
    let parser = engine.frames().into_iter().find(|&frame| {
        engine
            .document_loader(frame)
            .is_some_and(|loader| loader.parse_task == Some(number))
    });
    let Some(frame) = parser else {
        let ran = engine.run_dom_task(number);
        return (ran, engine);
    };
    if let Some(document) = engine
        .document_loader(frame)
        .map(|loader| loader.document.clone())
    {
        engine.dom_mut().mutate(|cx| {
            let document = document.get(cx.gc);
            if let Some(window) = cx.window_of(document) {
                cx.take_element_task_numbered(cx.window_context(window), number);
            }
        });
    }
    let _task = trace_event::trace_event!("HTMLDocumentParser::PumpTokenizer");
    let (_, engine) = parse(engine, frame, run);
    (true, engine)
}

/// The response of the script the DOM queued whose request is
/// `request_id` (`dom::SentScript`), `at` the frame showing its document
/// and that document's encoding (none: no frame shows it, and a classic
/// one's fails): a module's to the engine's module map, after which a
/// deferred module script's turn may come in any frame; a classic one's
/// or a wasm one's decoded and handed to its pending script
/// (`Cx::script_fetched`), and the parser's then runs on
/// (`parse_unless_due`) or its deferred scripts do. One that failed, or
/// was of no use, noted in its document's failed loads. Whether it was
/// the DOM's.
pub fn script_fetched(
    mut engine: Box<dyn ScriptEngine>,
    request_id: u64,
    result: &crate::subresource::SubresourceResult<'_>,
    at: Option<(FrameId, &'static encoding_rs::Encoding)>,
    run: &ParseRun<'_>,
) -> (bool, Box<dyn ScriptEngine>) {
    use crate::subresource;
    let Some(sent) = engine.dom_mut().take_sent_script(request_id) else {
        return (false, engine);
    };
    let document = sent.document;
    let note_failed = |engine: &mut dyn ScriptEngine| {
        if at.is_some() {
            note_load_failed(engine.dom_mut(), document, subresource::failed_url(result));
        }
    };
    if sent.module {
        let module = subresource::module_loaded(result);
        if module.is_none() {
            note_failed(engine.as_mut());
        }
        engine.module_fetched(sent.key, module);
        // A deferred module script's graph may be in.
        for frame in engine.frames() {
            run_deferred_scripts(engine.as_mut(), frame);
        }
        return (true, engine);
    }
    let parsers = if sent.wasm {
        let loaded = subresource::wasm_loaded(result);
        if loaded.is_none() {
            note_failed(engine.as_mut());
        }
        engine
            .dom_mut()
            .mutate(|cx| cx.wasm_script_fetched(sent.key, loaded))
    } else {
        let loaded = at.and_then(|(_, encoding)| {
            subresource::script_loaded(result, sent.charset.as_deref(), encoding)
        });
        if loaded.is_none() {
            note_failed(engine.as_mut());
        }
        engine
            .dom_mut()
            .mutate(|cx| cx.script_fetched(sent.key, loaded))
    };
    // The parser's: the one it waits for (unless it parses on in a task of
    // its own already), or a deferred one.
    if parsers && let Some((frame, _)) = at {
        let (_, lent) = parse_unless_due(engine, frame, run);
        engine = lent;
        run_deferred_scripts(engine.as_mut(), frame);
    }
    (true, engine)
}

/// The response of the `<link>`'s sheet whose request is `request_id`
/// (`dom::SentSheet`), `at` the frame showing its document and that
/// document's encoding (none: no frame shows it any more, and nothing
/// waits): its document's `load` waits for it no more; decoded as a sheet
/// (`stylesheet_text`), one of no use noted as failed; parsed against
/// where it came from and set as the `<link>`'s sheet, origin-clean if the
/// document's origin or CORS allowed it (for a network error, if it asked
/// its own origin or a `data:` URL); then its `load` or `error`; the
/// parser's scripts it blocked may run on, the document's load may finish,
/// a frame is asked for. Whether it was one, and its text with what it was
/// parsed into if `keep_text` (DevTools', Chrome's from its cache).
pub fn sheet_fetched(
    mut engine: Box<dyn ScriptEngine>,
    request_id: u64,
    result: &crate::subresource::SubresourceResult<'_>,
    at: Option<(FrameId, &'static encoding_rs::Encoding)>,
    keep_text: bool,
    run: &ParseRun<'_>,
) -> (bool, KeptSheetText, Box<dyn ScriptEngine>) {
    use crate::subresource;
    let taken = engine.dom_mut().mutate(|cx| {
        cx.take_sent_sheet(request_id)
            .map(|(sent, link)| (sent, link.map(|link| cx.root(link))))
    });
    let Some((sent, link)) = taken else {
        return (false, None, engine);
    };
    let (Some((frame, encoding)), Some(link), Ok(url)) = (at, link, url::Url::parse(&sent.url))
    else {
        return (true, None, engine);
    };
    let Some((document, quirks, document_url)) = engine.dom_mut().read(|cx| {
        let document = cx.node_by_id(sent.document)?;
        let quirks = cx.quirks_mode(document) == dom::QuirksMode::Quirks;
        let url = cx
            .document_url(document)
            .and_then(|url| url::Url::parse(&url.to_string()).ok());
        Some((cx.root(document), quirks, url))
    }) else {
        return (true, None, engine);
    };
    // Script may read the rules of one of the document's origin or that
    // CORS allowed; of a network error (a CORS failure among them), if it
    // asked its own origin.
    let origin_clean = match result {
        Ok(response) => !response.opaque,
        Err(_) => {
            url.scheme() == "data"
                || document_url.is_some_and(|document| url.origin() == document.origin())
        }
    };
    let css = subresource::stylesheet_text(result, quirks, encoding);
    if css.is_none() {
        note_load_failed(
            engine.dom_mut(),
            sent.document,
            subresource::failed_url(result),
        );
    }
    let base = result
        .as_ref()
        .ok()
        .and_then(|response| url::Url::parse(response.url).ok())
        .map(|url| {
            let referrer = url.to_string();
            css::UrlContext::new(url, &referrer)
        });
    let contents = std::sync::Arc::new(css::StyleSheetContents::parse(
        css.as_deref().unwrap_or_default(),
        base,
    ));
    let kept = css
        .as_ref()
        .filter(|_| keep_text)
        .map(|css| (contents.clone(), css.clone()));
    engine.dom_mut().mutate(|cx| {
        cx.set_link_sheet(link.get(cx.gc), contents, url.as_str(), origin_clean);
    });
    engine.fire_event(
        &link,
        if css.is_some() {
            dom::names::event::LOAD
        } else {
            dom::names::event::ERROR
        },
    );
    // The parser's scripts it blocked may run now.
    if sent.blocks_scripts {
        let (_, lent) = parse(engine, frame, run);
        engine = lent;
        run_deferred_scripts(engine.as_mut(), frame);
    }
    finish_load(engine.as_mut(), &document);
    schedule_frame(engine.as_mut());
    (true, kept, engine)
}

/// A sheet's text and what it was parsed into, kept for DevTools
/// (`sheet_fetched`'s).
pub type KeptSheetText = Option<(std::sync::Arc<css::StyleSheetContents>, String)>;

/// `url`, of a subresource of the document `document`, noted as failed.
fn note_load_failed(dom: &mut Dom, document: dom::NodeId, url: String) {
    dom.mutate(|cx| {
        if let Some(document) = cx.node_by_id(document) {
            cx.note_load_failed(document, url);
        }
    });
}

/// The parser stopped at the end tag of `element`, which starts on `line`:
/// HTML's "prepare the script element" for it, in the document the parser
/// is for (one script moved to another meanwhile doesn't run); the parser
/// waits for it, or it runs after parsing, or it waits for no parser.
fn stopped_at_script(
    engine: &mut dyn ScriptEngine,
    frame: FrameId,
    document: &NodeRoot,
    element: NodeRoot,
    line: u32,
) {
    let wasm = engine
        .document_loader(frame)
        .is_some_and(|loader| loader.wasm);
    let prepared = engine.dom_mut().mutate(|cx| {
        let script = element.get(cx.gc);
        if cx.node_id(cx.document_of(script)) != cx.node_id(document.get(cx.gc)) {
            return Prepared::Nothing;
        }
        let prepared = if wasm {
            crate::prepare_wasm_script(cx, script)
        } else {
            crate::prepare_script(cx, script)
        };
        let queued = match &prepared {
            Prepared::Blocking(request) | Prepared::Defer(request) => {
                Some((request.clone(), false))
            }
            Prepared::Wasm(request) => Some((request.clone(), true)),
            _ => None,
        };
        if let Some((request, wasm)) = queued {
            cx.queue_parser_script(script, request, wasm);
        }
        prepared
    });
    let pending = |kind| ParserScript::Pending {
        element: element.clone(),
        kind,
    };
    let script = match prepared {
        Prepared::Nothing => return,
        Prepared::Queued => {
            engine.send_script_fetches();
            return;
        }
        Prepared::Inline { source, base_url } => {
            let blocking = ParserScript::Inline {
                element: element.clone(),
                source,
                line,
                base_url,
            };
            if let Some(loader) = engine.document_loader(frame) {
                loader.blocking = Some(blocking);
            }
            return;
        }
        Prepared::Blocking(_) => {
            engine.send_script_fetches();
            if let Some(loader) = engine.document_loader(frame) {
                loader.blocking = Some(pending(PendingKind::Classic));
            }
            return;
        }
        Prepared::Defer(_) => {
            engine.send_script_fetches();
            pending(PendingKind::Classic)
        }
        Prepared::Wasm(_) => {
            engine.send_script_fetches();
            pending(PendingKind::Wasm)
        }
        Prepared::Module(mut start) => {
            if let crate::ModuleSource::Inline { line: at, .. } = &mut start.source {
                *at = line;
            }
            let deferred = start.deferred;
            engine.start_module_graph(document, start);
            if !deferred {
                return;
            }
            pending(PendingKind::Module)
        }
    };
    if let Some(loader) = engine.document_loader(frame) {
        loader.deferred.push_back(script);
    }
}

/// Whether the parser's script `element` (`inline`, or whose pending
/// script the DOM has) is ready to run: fetched (a module script's graph),
/// and no sheet that blocks scripts is loading.
fn script_ready(dom: &mut Dom, document: &NodeRoot, element: &NodeRoot, inline: bool) -> bool {
    dom.read(|cx| {
        !cx.sheets_block_scripts(document.get(cx.gc))
            && (inline || cx.pending_script_fetched(element.get(cx.gc)))
    })
}

/// Whether the parser has made `frame`'s document's body: once it has, a
/// run lets other work go when its budget is spent.
fn body_parsed(engine: &mut dyn ScriptEngine, frame: FrameId) -> bool {
    let Some(loader) = engine.document_loader(frame) else {
        return false;
    };
    if loader.body_parsed {
        return true;
    }
    let document = loader.document.clone();
    let parsed = engine
        .dom_mut()
        .read(|cx| cx.body(document.get(cx.gc)).is_some());
    if parsed && let Some(loader) = engine.document_loader(frame) {
        loader.body_parsed = true;
    }
    parsed
}

/// HTML's "execute the script element" for the parser's `script` of
/// `document`: none if script moved it to another document meanwhile (an
/// external classic or module one's, `dom_api::run_script_task` sees to);
/// its `error` if it failed to load; else it runs, a microtask checkpoint
/// before (the parser's script nesting level is zero) and after; an
/// external one from its response's URL, then its `load`.
fn execute(
    engine: &mut dyn ScriptEngine,
    frame: FrameId,
    document: &NodeRoot,
    script: ParserScript,
) {
    match script {
        ParserScript::Pending {
            element,
            kind: PendingKind::Module,
        } => engine.run_ready_script(&element),
        ParserScript::Pending {
            element,
            kind: PendingKind::Classic,
        } => {
            engine.perform_microtask_checkpoint();
            engine.run_ready_script(&element);
        }
        ParserScript::Pending {
            element,
            kind: PendingKind::Wasm,
        } => {
            let gone = moved(engine.dom_mut(), document, &element);
            let ready = engine.dom_mut().mutate(|cx| {
                cx.take_ready_script(element.get(cx.gc))
                    .map(|ready| ready.loaded)
            });
            if gone {
                return;
            }
            match ready.flatten() {
                Some(dom::ScriptResult::Wasm { bytes, url }) => {
                    engine.run_wasm_script(
                        frame,
                        &crate::WasmScript {
                            bytes: &bytes,
                            url: &url,
                        },
                    );
                    engine.fire_event(&element, dom::names::event::LOAD);
                }
                _ => engine.fire_event(&element, dom::names::event::ERROR),
            }
        }
        ParserScript::Inline {
            element,
            source,
            line,
            base_url,
        } => {
            if moved(engine.dom_mut(), document, &element) {
                return;
            }
            run_inline(engine, frame, &element, &source, line, base_url.as_deref());
        }
    }
}

/// Whether script moved the parser's `element` out of `document`.
fn moved(dom: &mut Dom, document: &NodeRoot, element: &NodeRoot) -> bool {
    dom.read(|cx| cx.node_id(cx.document_of(element.get(cx.gc))) != cx.node_id(document.get(cx.gc)))
}

/// The parser's inline classic `script`, `source` from `line` (its base
/// URL `base_url`), run from its document's URL: a microtask checkpoint
/// before it (the tree builder's at a script's end tag: what the parser
/// changed reaches the mutation observers first) and after it (HTML's
/// "clean up after running script"), the document's `currentScript` it
/// meanwhile (none for one in a shadow tree; none again after: the
/// parser's scripts run with no other running), its credentials mode its
/// element's as it runs.
fn run_inline(
    engine: &mut dyn ScriptEngine,
    frame: FrameId,
    script: &NodeRoot,
    source: &str,
    line: u32,
    base_url: Option<&str>,
) {
    engine.perform_microtask_checkpoint();
    // The document it was set on gets none back, if the script moved the
    // element meanwhile.
    let (document, url, use_credentials) = engine.dom_mut().mutate(|cx| {
        let script = script.get(cx.gc);
        let document = cx.document_of(script);
        let current = cx
            .containing_shadow_root(script)
            .is_none()
            .then_some(script);
        cx.set_current_script(document, current);
        let url = cx
            .document_url(document)
            .map_or_else(|| "about:blank".to_owned(), |url| url.to_string());
        (cx.root(document), url, crate::uses_credentials(cx, script))
    });
    engine.run_classic_script(
        frame,
        &ClassicScript {
            source,
            url: &url,
            base_url,
            line,
            muted: false,
            use_credentials,
        },
    );
    engine.dom_mut().mutate(|cx| {
        cx.set_current_script(document.get(cx.gc), None);
    });
    engine.perform_microtask_checkpoint();
}

/// HTML's "the end" for `frame`'s `document`, its parser finished now:
/// interactive, then its deferred scripts (`run_deferred_scripts`); or,
/// its body stopped short, its parser aborted (`aborted`).
fn parsed(engine: &mut dyn ScriptEngine, frame: FrameId, document: &NodeRoot) {
    // The `<link>`s the last of the input brought asked for first: the
    // deferred scripts wait for their sheets.
    engine.send_sheet_fetches();
    if engine
        .document_loader(frame)
        .is_some_and(|loader| loader.aborted)
    {
        aborted(engine, frame, document);
        return;
    }
    engine.document_ready(frame, crate::DocumentReadiness::Interactive);
    run_deferred_scripts(engine, frame);
}

/// HTML's "abort a parser", once what came of the document's body is
/// parsed (it stopped short): interactive, then complete, with no deferred
/// scripts, `DOMContentLoaded` or `load` (Blink's `Document::CancelParsing`,
/// as a load stopped is). Its load is over nonetheless: a child frame's
/// parent no longer waits for it, the top-level one's `DidFinishLoad`
/// goes with the next frame (its host reads the load ended). (The parser
/// ended what it had as at the end of its input: html5ever has no abort,
/// which would only pop the elements open.)
fn aborted(engine: &mut dyn ScriptEngine, frame: FrameId, document: &NodeRoot) {
    abort_scripts(engine, frame);
    // Its load over, before its `readystatechange`: no one waits for it.
    engine
        .dom_mut()
        .mutate(|cx| cx.set_load_ended(document.get(cx.gc)));
    let parent = engine.frame_parent(frame);
    engine.document_ready(frame, crate::DocumentReadiness::Interactive);
    engine.document_ready(frame, crate::DocumentReadiness::Aborted);
    if let Some((_, iframe)) = parent {
        let parent_document = engine
            .dom_mut()
            .read(|cx| cx.root(cx.document_of(iframe.get(cx.gc))));
        finish_load(engine, &parent_document);
    }
    schedule_frame(engine);
}

/// `frame`'s document's deferred scripts run in order, each once fetched
/// and no sheet blocks scripts (its host asks again as they come, while it
/// defers: `dom::LoadProgress::Deferring`, which none of them changes);
/// once none is left, `DOMContentLoaded` (`content_loaded`). Whether none
/// was left.
pub fn run_deferred_scripts(engine: &mut dyn ScriptEngine, frame: FrameId) -> bool {
    let Some(document) = engine
        .document_loader(frame)
        .map(|loader| loader.document.clone())
    else {
        return false;
    };
    if engine
        .dom_mut()
        .read(|cx| cx.load_progress(document.get(cx.gc)))
        != dom::LoadProgress::Deferring
    {
        return false;
    }
    if !run_deferred(engine, frame) {
        return false;
    }
    content_loaded(engine, frame, &document);
    true
}

/// HTML's "the end" once the deferred scripts ran: `DOMContentLoaded`, the
/// document scrolled to its URL's fragment once laid out (HTML's "scroll
/// to the fragment"), its title and lifecycle told, then complete and its
/// window's `load` once nothing delays it (`finish_load`).
fn content_loaded(engine: &mut dyn ScriptEngine, frame: FrameId, document: &NodeRoot) {
    engine.document_ready(frame, crate::DocumentReadiness::ContentLoaded);
    let (dom, layout) = engine.dom_and_layout();
    let target = dom.mutate(|cx| {
        let document = document.get(cx.gc);
        let url = url::Url::parse(&cx.document_url(document)?.to_string()).ok()?;
        let element = crate::fragment_target(cx, document, &url)?;
        Some((cx.node_id(document), element))
    });
    if let Some(layout) = layout {
        if let Some((document, element)) = target {
            layout.scroll_to_fragment(document, element);
        }
        layout.schedule_frame();
    }
    engine.tell_lifecycle(frame, crate::DocumentLifecycle::DomContentLoaded);
    finish_load(engine, document);
}

/// HTML's "the end" for `document` (a frame's) as far as it can go: its
/// sheets asked for first (the `<link>`s the last of the input brought,
/// before the style update below takes the flag that says the sheets
/// changed); then complete, its window's `load` task queued, once nothing
/// delays it (`check_completed`); once only images delay it, those its
/// style found fetched, and again (one that fails as it is asked for, a
/// revoked blob URL's, delays it no more). Its window's `load` task ends
/// it.
pub fn finish_load(engine: &mut dyn ScriptEngine, document: &NodeRoot) {
    engine.send_sheet_fetches();
    let check = |engine: &mut dyn ScriptEngine| {
        let (dom, layout) = engine.dom_and_layout();
        dom.mutate(|cx| crate::check_completed(cx, layout, document.get(cx.gc)))
    };
    if check(engine) == crate::LoadCheck::WaitsForImages {
        engine.send_image_fetches();
        check(engine);
    }
}

/// A frame asked for of the frame's layout: what the end changed is drawn
/// (and the next frame tells the load it ended).
fn schedule_frame(engine: &mut dyn ScriptEngine) {
    if let (_, Some(layout)) = engine.dom_and_layout() {
        layout.schedule_frame();
    }
}

/// The deferred scripts of `frame`'s document run in order while each is
/// ready: whether none is left.
fn run_deferred(engine: &mut dyn ScriptEngine, frame: FrameId) -> bool {
    loop {
        let Some(loader) = engine.document_loader(frame) else {
            return false;
        };
        let document = loader.document.clone();
        let Some(script) = loader.deferred.front() else {
            return true;
        };
        let element = script.element().clone();
        let inline = matches!(script, ParserScript::Inline { .. });
        if !script_ready(engine.dom_mut(), &document, &element, inline) {
            return false;
        }
        let Some(script) = engine
            .document_loader(frame)
            .and_then(|loader| loader.deferred.pop_front())
        else {
            return false;
        };
        execute(engine, frame, &document, script);
    }
}

/// The title of `loader`'s document (`document_title`) told to `client`
/// if it isn't the one told last: read again only once the tree changed
/// since (its first is told, whatever it is).
pub fn tell_title(dom: &Dom, loader: &mut DocumentLoader, client: &mut dyn crate::FrameClient) {
    let version = dom.read(|cx| cx.tree_version());
    if loader.told_title.is_some() && version == loader.title_version {
        return;
    }
    loader.title_version = version;
    let document = &loader.document;
    let title = dom.read(|cx| crate::document_title(&crate::ApiCx::new(cx), document.get(cx.gc)));
    if loader.told_title.as_deref() != Some(title.as_str()) {
        client.title_changed(&title);
        loader.told_title = Some(title);
    }
}

/// HTML's "abort a parser" for `frame`'s document: its deferred scripts
/// (and any it waits for) never run, their pending scripts let go (their
/// document's load no longer waits for them).
pub fn abort_scripts(engine: &mut dyn ScriptEngine, frame: FrameId) {
    let Some(loader) = engine.document_loader(frame) else {
        return;
    };
    let scripts: Vec<ParserScript> = loader
        .blocking
        .take()
        .into_iter()
        .chain(loader.deferred.drain(..))
        .collect();
    engine.dom_mut().mutate(|cx| {
        for script in &scripts {
            if let ParserScript::Pending { element, .. } = script {
                cx.take_ready_script(element.get(cx.gc));
            }
        }
    });
}

/// A document's text as the HTML parser reads it, a piece at a time as
/// it comes: HTML as it is; XHTML through the HTML parser too (there is no
/// XML parser for documents yet, and the WPT tests we run are well-formed
/// documents it reads the same, except for CDATA sections, usually around
/// `<style>` contents, whose markers an XML parser drops: so they are
/// dropped first); any other type as text in a `<pre>` (Blink's
/// `TextDocumentParser`).
pub(crate) enum MarkupFilter {
    Html,
    /// The end of what came that may be the start of a CDATA marker, held
    /// for what comes next.
    Xhtml {
        held: String,
    },
    /// Whether the `<pre>` has gone.
    Text {
        started: bool,
    },
}

const CDATA_START: &str = "<![CDATA[";
const CDATA_END: &str = "]]>";

impl MarkupFilter {
    pub(crate) fn new(mime_type: &str) -> Self {
        match mime_type {
            "text/html" => Self::Html,
            "application/xhtml+xml" => Self::Xhtml {
                held: String::new(),
            },
            _ => Self::Text { started: false },
        }
    }

    /// The markup of the text's next piece, `text` (`last`: the end of
    /// it).
    pub(crate) fn push(&mut self, text: &str, last: bool) -> String {
        match self {
            Self::Html => text.to_owned(),
            Self::Xhtml { held } => {
                let mut markup = std::mem::take(held);
                markup.push_str(text);
                let mut markup = markup.replace(CDATA_START, "").replace(CDATA_END, "");
                if !last {
                    let keep = marker_start_at_end(&markup);
                    *held = markup.split_off(markup.len().saturating_sub(keep));
                }
                markup
            }
            Self::Text { started } => {
                let escaped = text
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                if std::mem::replace(started, true) {
                    escaped
                } else {
                    // The parser drops a newline right after `<pre>`, so
                    // add one.
                    format!("<pre>\n{escaped}")
                }
            }
        }
    }
}

/// How long the end of `markup` is that starts a CDATA marker (and
/// isn't one): 0 if none does. The markers are ASCII, so it ends at a
/// character's boundary.
fn marker_start_at_end(markup: &str) -> usize {
    [CDATA_START, CDATA_END]
        .iter()
        .flat_map(|marker| (1..marker.len()).filter(|&len| markup.ends_with(&marker[..len])))
        .max()
        .unwrap_or(0)
}

/// A document's markup its parser hasn't been given yet, in the pieces it
/// came in: given a slice at a time, so the parser can stop between them
/// (Blink's parser takes its input a token at a time).
#[derive(Default)]
struct ParserInput {
    pieces: std::collections::VecDeque<String>,
    /// How much of the first piece was given.
    at: usize,
}

impl ParserInput {
    fn push(&mut self, markup: String) {
        if !markup.is_empty() {
            self.pieces.push_back(markup);
        }
    }

    fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// `give` the next slice: at most `max` bytes of whole characters (one
    /// character, if it is longer). False if there is none.
    fn give(&mut self, max: usize, give: impl FnOnce(&str)) -> bool {
        let Some(first) = self.pieces.front() else {
            return false;
        };
        let mut end = self.at.saturating_add(max).min(first.len());
        while !first.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        if end <= self.at {
            end = first[self.at..]
                .chars()
                .next()
                .map_or(first.len(), |c| self.at.saturating_add(c.len_utf8()));
        }
        give(&first[self.at..end]);
        if end == first.len() {
            self.pieces.pop_front();
            self.at = 0;
        } else {
            self.at = end;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_not_parsed_as_markup() {
        let mut dom = dom::Dom::new();
        let mut filter = MarkupFilter::new("text/plain");
        let markup = [
            filter.push("<title>t", false),
            filter.push("</title>\nx", true),
        ]
        .concat();
        let document = html::parser::parse_document(&mut dom, &markup, true);
        let dump = html::parser::dump_children(&mut dom, &document.root());
        assert!(
            dump.contains("<pre>\n|       \"<title>t</title>\nx\""),
            "{dump}"
        );
    }

    #[test]
    fn xhtml_drops_cdata_markers_cut_between_pieces() {
        let mut filter = MarkupFilter::new("application/xhtml+xml");
        let pieces = ["<style><![CD", "ATA[p{}]", "]></style>]", "]"];
        let markup: String = pieces
            .iter()
            .enumerate()
            .map(|(at, piece)| filter.push(piece, at + 1 == pieces.len()))
            .collect();
        // What only looked like a marker's start comes out at the end.
        assert_eq!(markup, "<style>p{}</style>]]");
    }

    /// Given in slices of whole characters, the pieces' text as it was.
    #[test]
    fn input_is_given_in_slices_of_whole_characters() {
        let mut input = ParserInput::default();
        input.push("ab中".to_owned());
        input.push(String::new());
        input.push("cdef".to_owned());
        let mut slices = Vec::new();
        while input.give(3, |slice| slices.push(slice.to_owned())) {}
        assert_eq!(slices, ["ab", "中", "cde", "f"]);
        assert!(input.is_empty());
        // Narrower than a character: the character.
        input.push("中".to_owned());
        assert!(input.give(1, |slice| assert_eq!(slice, "中")));
        assert!(!input.give(1, |_| unreachable!()));
    }
}
