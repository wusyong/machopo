//! A script element's triggers to be prepared, recorded in tree order
//! with the iframes' (`take_element_changes`): connected, a node inserted
//! into it while connected, a `src` set while connected; not for one the
//! parser made, nor one already started, nor before one that may run is
//! made. Its flags: non-blocking until `async` is added, already started
//! copied by a clone. An external one queued: its fetch for the host,
//! an element task as its turn comes (as soon as it is fetched, or in
//! order), what it fetched taken then; its document's load delayed till.

use dom::ElementChange::{Frame, Script};
use dom::{
    Cx, Dom, ElementTaskKind, FrameOwnerChange, LoadedScript, NodeRef, QualName, ScriptFlags,
    ScriptRequest, ScriptResult,
};

/// A document with a window and a `<body>`: the `<body>`.
fn page<'gc>(cx: &Cx<'_, 'gc>) -> NodeRef<'gc> {
    let document = cx.create_document(true);
    cx.create_window(document);
    let html = cx.create_element(document, QualName::html("html"));
    cx.append_child(document, html).unwrap();
    let body = cx.create_element(document, QualName::html("body"));
    cx.append_child(html, body).unwrap();
    body
}

#[test]
fn script_elements_to_prepare_are_recorded_in_tree_order() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx);
        let document = cx.document_of(body);
        // The parser's: nothing looked at yet.
        let parsers = cx.create_element_by_parser(document, QualName::html("script"));
        cx.set_script_flags(
            parsers,
            ScriptFlags {
                parser_inserted: true,
                ..ScriptFlags::default()
            },
        );
        cx.append_child(body, parsers).unwrap();
        let text = cx.create_text(document, "a()");
        cx.append_child(parsers, text).unwrap();
        assert!(!cx.has_element_changes(), "the parser's");

        let div = cx.create_element(document, QualName::html("div"));
        let iframe = cx.create_element(document, QualName::html("iframe"));
        let script = cx.create_element(document, QualName::html("script"));
        let inner = cx.create_element(document, QualName::html("script"));
        cx.append_child(div, iframe).unwrap();
        cx.append_child(div, script).unwrap();
        cx.append_child(script, inner).unwrap();
        cx.set_attribute(script, "src", "a.js");
        assert!(!cx.has_element_changes(), "not connected");
        cx.append_child(body, div).unwrap();
        let (iframe_id, script_id, inner_id) =
            (cx.node_id(iframe), cx.node_id(script), cx.node_id(inner));
        assert_eq!(
            cx.take_element_changes(),
            [
                Frame(FrameOwnerChange::Inserted(iframe_id)),
                Script(script_id),
                Script(inner_id)
            ]
        );
        // Connected: a node inserted into it, a `src` set (not removed); a
        // text inserted into another element, nothing.
        let text = cx.create_text(document, "b()");
        cx.append_child(script, text).unwrap();
        cx.set_attribute(script, "src", "b.js");
        cx.remove_attribute(script, "src");
        cx.set_attribute(script, "type", "module");
        let other = cx.create_text(document, "c");
        cx.append_child(div, other).unwrap();
        assert_eq!(
            cx.take_element_changes(),
            [Script(script_id), Script(script_id)]
        );
        // The parser's, not yet prepared (its end tag not parsed yet),
        // once one that may run is made: nothing.
        let text = cx.create_text(document, "e()");
        cx.append_child(parsers, text).unwrap();
        assert!(cx.take_element_changes().is_empty(), "the parser's, seen");
        // Already started: nothing.
        cx.set_script_flags(
            script,
            ScriptFlags {
                already_started: true,
                ..cx.script_flags(script)
            },
        );
        let text = cx.create_text(document, "d()");
        cx.append_child(script, text).unwrap();
        cx.remove(div);
        cx.append_child(body, div).unwrap();
        assert_eq!(
            cx.take_element_changes(),
            [
                Frame(FrameOwnerChange::Removed(iframe_id)),
                Frame(FrameOwnerChange::Inserted(iframe_id)),
                Script(inner_id)
            ]
        );
    });
}

/// The parser's script left unprepared (empty, or of a type that doesn't
/// run) loses its parser document: from then on, what script does to it
/// is looked at.
#[test]
fn a_script_the_parser_left_unprepared_is_looked_at() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx);
        let document = cx.document_of(body);
        let parsers = cx.create_element_by_parser(document, QualName::html("script"));
        cx.set_script_flags(
            parsers,
            ScriptFlags {
                parser_inserted: true,
                ..ScriptFlags::default()
            },
        );
        cx.append_child(body, parsers).unwrap();
        cx.set_script_flags(parsers, ScriptFlags::default());
        let text = cx.create_text(document, "a()");
        cx.append_child(parsers, text).unwrap();
        assert_eq!(cx.take_element_changes(), [Script(cx.node_id(parsers))]);
    });
}

/// Non-blocking until its `async` is added (removed, it stays so); a
/// clone already started if the original is.
#[test]
fn a_script_elements_flags() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx);
        let document = cx.document_of(body);
        let script = cx.create_element(document, QualName::html("script"));
        assert!(cx.script_flags(script).non_blocking);
        cx.set_attribute(script, "async", "");
        cx.remove_attribute(script, "async");
        assert!(!cx.script_flags(script).non_blocking);
        let clone = cx.clone_node(script, None, false);
        assert_eq!(
            cx.script_flags(clone),
            ScriptFlags {
                non_blocking: true,
                ..ScriptFlags::default()
            }
        );
        cx.set_script_flags(
            script,
            ScriptFlags {
                already_started: true,
                ..ScriptFlags::default()
            },
        );
        let clone = cx.clone_node(script, None, false);
        assert!(cx.script_flags(clone).already_started);
        let div = cx.create_element(document, QualName::html("div"));
        cx.set_attribute(div, "async", "");
        assert_eq!(cx.script_flags(div), ScriptFlags::default());
    });
}

#[test]
fn queued_scripts_take_their_turns() {
    let mut dom = Dom::new();
    let (scripts, document) = dom.mutate(|cx| {
        let body = page(cx);
        let document = cx.document_of(body);
        let scripts: Vec<_> = ["a", "b", "c", "d"]
            .iter()
            .map(|name| {
                let script = cx.create_element(document, QualName::html("script"));
                cx.append_child(body, script).unwrap();
                let request = ScriptRequest {
                    url: format!("https://example.test/{name}.js"),
                    cors: None,
                    charset: None,
                };
                // `a` as soon as it can; the rest in order.
                cx.queue_script(script, request, *name != "a");
                cx.root(script)
            })
            .collect();
        assert!(cx.load_event_delayed(document));
        (scripts, cx.root(document))
    });
    let fetches = dom.take_script_fetches();
    assert_eq!(
        fetches
            .iter()
            .map(|fetch| fetch.request.url.as_str())
            .collect::<Vec<_>>(),
        [
            "https://example.test/a.js",
            "https://example.test/b.js",
            "https://example.test/c.js",
            "https://example.test/d.js"
        ]
    );
    assert!(dom.take_script_fetches().is_empty(), "taken");
    let loaded = |source: &str| {
        Some(LoadedScript {
            source: source.to_owned(),
            url: String::new(),
            muted: false,
        })
    };
    let turns = |dom: &mut Dom| {
        dom.mutate(|cx| {
            let window = cx.window_of(document.get(cx.gc)).expect("a window");
            let mut turns = Vec::new();
            while let Some((element, kind, _)) = cx.take_element_task(window) {
                assert_eq!(kind, ElementTaskKind::Script);
                let ready = cx.take_ready_script(element).expect("its pending script");
                assert_eq!(cx.node_id(ready.document), cx.node_id(document.get(cx.gc)));
                turns.push(ready.loaded.map(|loaded| match loaded {
                    ScriptResult::Classic(loaded) => loaded.source,
                    ScriptResult::Module(root) => format!("module {root}"),
                }));
            }
            turns
        })
    };
    // `c` waits for `b`; `a` runs as it comes.
    dom.mutate(|cx| {
        cx.script_fetched(fetches[2].key, loaded("c"));
        cx.script_fetched(fetches[0].key, loaded("a"));
    });
    assert_eq!(turns(&mut dom), [Some("a".to_owned())]);
    dom.mutate(|cx| cx.script_fetched(fetches[1].key, None));
    assert_eq!(turns(&mut dom), [None, Some("c".to_owned())]);
    assert!(
        dom.read(|cx| cx.load_event_delayed(document.get(cx.gc))),
        "d not yet"
    );
    // Twice, or once more after its turn: nothing.
    dom.mutate(|cx| cx.script_fetched(fetches[2].key, loaded("again")));
    assert!(turns(&mut dom).is_empty());
    dom.mutate(|cx| cx.script_fetched(fetches[3].key, loaded("d")));
    assert_eq!(turns(&mut dom), [Some("d".to_owned())]);
    dom.read(|cx| {
        assert!(!cx.load_event_delayed(document.get(cx.gc)));
        assert!(
            cx.take_ready_script(scripts[3].get(cx.gc)).is_none(),
            "taken"
        );
    });
}

/// One whose element moved to a document with no window: its task in the
/// window of the document it was prepared in, which it no longer delays
/// once taken.
#[test]
fn a_moved_scripts_turn_is_its_first_documents() {
    let mut dom = Dom::new();
    let (script, document) = dom.mutate(|cx| {
        let body = page(cx);
        let document = cx.document_of(body);
        let script = cx.create_element(document, QualName::html("script"));
        cx.append_child(body, script).unwrap();
        let request = ScriptRequest {
            url: "https://example.test/a.js".to_owned(),
            cors: None,
            charset: None,
        };
        cx.queue_script(script, request, false);
        let other = cx.create_document(true);
        cx.append_child(other, script).unwrap();
        (cx.root(script), cx.root(document))
    });
    let fetches = dom.take_script_fetches();
    dom.mutate(|cx| {
        cx.script_fetched(fetches[0].key, None);
        let window = cx.window_of(document.get(cx.gc)).expect("a window");
        let (element, kind, _) = cx.take_element_task(window).expect("its turn");
        assert_eq!(kind, ElementTaskKind::Script);
        assert_eq!(cx.node_id(element), cx.node_id(script.get(cx.gc)));
        assert!(cx.load_event_delayed(document.get(cx.gc)));
        let ready = cx.take_ready_script(element).expect("its pending script");
        assert_eq!(cx.node_id(ready.document), cx.node_id(document.get(cx.gc)));
        assert!(!cx.load_event_delayed(document.get(cx.gc)));
    });
}
