//! The parser (`html::parser`) beyond what the tree-construction tests
//! cover: the document's quirks mode, the fragment's context's (its
//! quirks mode, an `annotation-xml` one's integration point), the flags
//! the parser sets on scripts and links, `<selectedcontent>`, custom
//! elements, declarative shadow roots and the fast path.

use std::cell::RefCell;
use std::rc::Rc;

use dom::{Dom, NodeKind, NodeRoot, QualName, QuirksMode, ScriptFlags, names};
use html::parser::SinkHandle;

use crate::harness::{TestHost, context, dom};

fn quirks_mode(dom: &Rc<RefCell<Dom>>, document: &SinkHandle) -> QuirksMode {
    dom.borrow_mut()
        .read(|cx| cx.quirks_mode(document.root().get(cx.gc)))
}

#[test]
fn records_quirks_mode() {
    let dom = dom();
    for (input, mode) in [
        ("<!DOCTYPE html><p>", QuirksMode::NoQuirks),
        ("<p>no doctype", QuirksMode::Quirks),
        (
            r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN" "">"#,
            QuirksMode::LimitedQuirks,
        ),
    ] {
        let document = html::parser::parse_document(&mut dom.borrow_mut(), input, true);
        assert_eq!(quirks_mode(&dom, &document), mode, "{input}");
    }
}

/// The fragment parser builds into a new fragment of the context's
/// document, in its quirks mode (in quirks mode a `<table>` doesn't close a
/// `<p>`), and leaves the context and its document as they were.
#[test]
fn a_fragment_takes_the_contexts_quirks_mode() {
    let dom = dom();
    for (quirks, expected) in [
        (QuirksMode::Quirks, "| <p>\n|   <table>\n"),
        (QuirksMode::NoQuirks, "| <p>\n| <table>\n"),
    ] {
        let (div, document) = context(&dom, quirks);
        let fragment =
            html::parser::parse_fragment(&mut dom.borrow_mut(), &div, "<p><table></table>", true);
        assert_eq!(
            html::parser::dump_children(&mut dom.borrow_mut(), &fragment.root()),
            expected
        );
        dom.borrow_mut().read(|cx| {
            let (div, document) = (div.get(cx.gc), document.get(cx.gc));
            let fragment = fragment.root().get(cx.gc);
            assert_eq!(cx.kind(fragment), NodeKind::DocumentFragment);
            assert!(cx.parent(fragment).is_none());
            assert!(elise::Gc::ptr_eq(cx.document_of(fragment), document));
            assert!(cx.first_child(div).is_none() && cx.first_child(document).is_none());
            assert_eq!(cx.quirks_mode(document), quirks);
        });
    }
}

/// The first element named `local` under `root`, in tree order, and its
/// flags.
fn flags_of(dom: &Rc<RefCell<Dom>>, root: &NodeRoot, local: &str) -> ScriptFlags {
    dom.borrow_mut().read(|cx| {
        let mut stack = vec![root.get(cx.gc)];
        while let Some(node) = stack.pop() {
            if let Some(name) = cx.element_name(node)
                && names::get(name.local) == local
            {
                return cx.script_flags(node);
            }
            let mut child = cx.last_child(node);
            while let Some(at) = child {
                stack.push(at);
                child = cx.previous_sibling(at);
            }
        }
        panic!("no <{local}>");
    })
}

/// HTML's "create an element for a token": a document's parser's scripts
/// are parser-inserted, and already started if scripting is disabled (they
/// never run); a fragment's always are. A document's parser's `<link>` is
/// parser-inserted (its stylesheet blocks the scripts), a fragment's isn't.
#[test]
fn scripts_and_links_have_the_parsers_flags() {
    let dom = dom();
    let input = "<link rel=stylesheet href=a.css><script></script><div></div>";
    let inserted = ScriptFlags {
        parser_inserted: true,
        already_started: false,
        non_blocking: false,
    };
    let never_runs = ScriptFlags {
        parser_inserted: true,
        already_started: true,
        non_blocking: false,
    };
    let on = html::parser::parse_document(&mut dom.borrow_mut(), input, true);
    assert_eq!(flags_of(&dom, &on.root(), "script"), inserted);
    assert_eq!(flags_of(&dom, &on.root(), "link"), inserted);
    assert_eq!(flags_of(&dom, &on.root(), "div"), ScriptFlags::default());
    let off = html::parser::parse_document(&mut dom.borrow_mut(), input, false);
    assert_eq!(flags_of(&dom, &off.root(), "script"), never_runs);
    assert_eq!(flags_of(&dom, &off.root(), "link"), inserted);

    let (div, _) = context(&dom, QuirksMode::NoQuirks);
    let fragment = html::parser::parse_fragment(&mut dom.borrow_mut(), &div, input, true);
    assert_eq!(flags_of(&dom, &fragment.root(), "script"), never_runs);
    assert_eq!(
        flags_of(&dom, &fragment.root(), "link"),
        ScriptFlags::default()
    );
}

/// html5ever only asks the sink to update `<selectedcontent>` on an
/// explicit `</option>` (servo/html5ever#712), so the tree-construction
/// tests, which close options implicitly, can't cover it: the selected
/// option's contents, copied; the first option's if none is selected;
/// none in a `multiple` select.
#[test]
fn explicit_option_close_fills_selectedcontent() {
    let dom = dom();
    let dump = |input: &str| {
        let document = html::parser::parse_document(&mut dom.borrow_mut(), input, true);
        html::parser::dump_children(&mut dom.borrow_mut(), &document.root())
    };
    assert_eq!(
        dump(
            "<select><button><selectedcontent>old</selectedcontent></button>\
             <option>X</option><option selected>Y<b>!</b><!--c--></option></select>"
        ),
        "\
| <html>
|   <head>
|   <body>
|     <select>
|       <button>
|         <selectedcontent>
|           \"Y\"
|           <b>
|             \"!\"
|           <!-- c -->
|       <option>
|         \"X\"
|       <option>
|         selected=\"\"
|         \"Y\"
|         <b>
|           \"!\"
|         <!-- c -->
"
    );
    let first = dump(
        "<select><button><selectedcontent></button><option>X</option><option>Z</option></select>",
    );
    assert!(
        first.contains("<selectedcontent>\n|           \"X\"\n|       <option>"),
        "{first}"
    );
    let multiple =
        dump("<select multiple><button><selectedcontent></button><option>X</option></select>");
    assert!(
        multiple.contains("<selectedcontent>\n|       <option>"),
        "{multiple}"
    );
}

/// A MathML `annotation-xml` context is an HTML integration point by its
/// `encoding` (`text/html` or `application/xhtml+xml`, any case): there
/// `<style>` is HTML's, raw text; without it, MathML's, and the `<img>`
/// in it breaks out as HTML.
#[test]
fn an_annotation_xml_context_is_an_integration_point_by_its_encoding() {
    let dom = dom();
    let parse = |encoding: Option<&str>| {
        let context = dom.borrow_mut().mutate(|cx| {
            let document = cx.create_document(true);
            let name = QualName {
                namespace: Some("http://www.w3.org/1998/Math/MathML"),
                prefix: None,
                local: "annotation-xml",
            };
            let context = cx.create_element(document, name);
            if let Some(encoding) = encoding {
                cx.set_attribute(context, "encoding", encoding);
            }
            cx.root(context)
        });
        let root = html::parser::parse_fragment(
            &mut dom.borrow_mut(),
            &context,
            "<style><img></style>",
            true,
        );
        html::parser::dump_children(&mut dom.borrow_mut(), &root.root())
    };
    let html = "| <style>\n|   \"<img>\"\n";
    assert_eq!(parse(Some("application/xhtml+xml")), html);
    assert_eq!(parse(Some("TEXT/HTML")), html);
    assert_eq!(parse(None), "| <math style>\n| <img>\n");
    assert_eq!(parse(Some("text/plain")), "| <math style>\n| <img>\n");
}

/// The document's parser stops at every `</script>`: the script element
/// is whole, what follows isn't parsed yet, and the line its text starts
/// on is known; input pushed in pieces parses as one; at the end every
/// element is closed. Its engine is lent to it for each run, given back.
#[test]
fn the_document_parser_pauses_at_each_script() {
    use html::parser::{Parser, ParserState};
    // First: the DOM goes after every root.
    let mut host = Some(TestHost::new());
    let document = host
        .as_mut()
        .unwrap()
        .dom
        .mutate(|cx| cx.root(cx.create_document(true)));
    let mut parser = Parser::<TestHost>::new(&host.as_ref().unwrap().dom, &document);
    let resume = |parser: &mut Parser<TestHost>, host: &mut Option<Box<TestHost>>| {
        let (state, back) = parser.resume(host.take().unwrap());
        *host = Some(back);
        state
    };
    parser.push("<!DOCTYPE html><title>t</title>\n<script>one()</scr");
    assert!(matches!(
        resume(&mut parser, &mut host),
        ParserState::NeedsInput
    ));
    parser.push("ipt><p id=a>\n\n<script>\ntwo()</script><p id=b>");
    let text_and_rest = |host: &mut Option<Box<TestHost>>, element: &SinkHandle| {
        host.as_mut().unwrap().dom.read(|cx| {
            let script = element.root().get(cx.gc);
            let text = cx.text_content(script).unwrap().to_string();
            let paragraphs =
                cx.collection_length(cx.elements_by_tag_name(document.get(cx.gc), "p"));
            (text, paragraphs)
        })
    };
    let ParserState::Script { element, line } = resume(&mut parser, &mut host) else {
        panic!("stops at the first script");
    };
    assert_eq!(
        (text_and_rest(&mut host, &element), line),
        (("one()".to_owned(), 0), 2)
    );
    let ParserState::Script { element, line } = resume(&mut parser, &mut host) else {
        panic!("stops at the second script");
    };
    assert_eq!(
        (text_and_rest(&mut host, &element), line),
        (("\ntwo()".to_owned(), 1), 4)
    );
    assert!(matches!(
        resume(&mut parser, &mut host),
        ParserState::NeedsInput
    ));
    host = Some(parser.finish(host.take().unwrap()));
    assert_eq!(
        host.as_mut()
            .unwrap()
            .dom
            .read(|cx| cx.collection_length(cx.elements_by_tag_name(document.get(cx.gc), "p"))),
        2
    );
}

/// The document's parser knows the line each element that loads
/// something starts on (`<link>`, `<img>`, `<script>`, `<iframe>`; not
/// another element, nor one in another namespace), taken as it goes (and
/// at the end); what it connects, it asked for, not script (the line is
/// set as it inserts).
#[test]
fn the_document_parser_knows_where_its_loads_start() {
    use html::parser::{Parser, ParserState};
    let mut host = TestHost::new();
    let document = host.dom.mutate(|cx| {
        let document = cx.create_document(true);
        cx.create_window(document);
        cx.root(document)
    });
    host.dom.set_stacks_wanted(true, true);
    let mut parser = Parser::<TestHost>::new(&host.dom, &document);
    parser.push(
        "<!DOCTYPE html>\n<link rel=stylesheet href=s.css>\n<p>\n<img src=a.png>\
         <svg><script></script></svg>\n<script>0</script>\n<iframe></iframe>",
    );
    let (state, back) = parser.resume(host);
    assert!(matches!(state, ParserState::Script { .. }));
    // Its script element's root, before the DOM.
    drop(state);
    let lines = parser.take_load_lines();
    let (state, back) = parser.resume(back);
    assert!(matches!(state, ParserState::NeedsInput));
    let rest = parser.take_load_lines();
    let (back, end) = parser.finish_taking_load_lines(back);
    assert!(end.is_empty(), "taken");
    let named = |lines: &dom::NodeIdHashMap<u32>| {
        let mut named: Vec<(String, u32)> = back.dom.read(|cx| {
            lines
                .iter()
                .map(|(&id, &line)| {
                    let node = cx.node_by_id(id).expect("a node");
                    (cx.node_name(node).to_string(), line)
                })
                .collect()
        });
        named.sort_by_key(|(_, line)| *line);
        named
    };
    assert_eq!(
        named(&lines),
        [
            ("LINK".to_owned(), 2),
            ("IMG".to_owned(), 4),
            ("SCRIPT".to_owned(), 5)
        ]
    );
    assert_eq!(named(&rest), [("IFRAME".to_owned(), 6)]);
    assert!(!back.dom.take_microtasks().stacks(), "none script's");
    // The roots before their DOM.
    drop(document);
}

/// The document's parser notes the line each `<style>`'s start tag ends
/// on, where its text starts (DevTools tells where its sheet's text is
/// from it): of a tag over two lines, the second; its own only, not one
/// script makes, nor an SVG `<style>`.
#[test]
fn the_document_parser_notes_where_styles_start() {
    let mut dom = Dom::new();
    let document = html::parser::parse_document(
        &mut dom,
        "<!DOCTYPE html>\n<style>a {}</style>\n\n<p>\n<style\n media=all>b {}</style>\
         <svg><style>c {}</style></svg>",
        true,
    );
    let lines: Vec<Option<u32>> = dom.mutate(|cx| {
        let document = document.root().get(cx.gc);
        let made = cx.create_element(document, QualName::html("style"));
        let mut styles = Vec::new();
        let mut at = cx.first_child(document);
        while let Some(node) = at {
            if cx.kind(node) == NodeKind::Element
                && cx.node_name(node).to_str().eq_ignore_ascii_case("style")
            {
                styles.push(node);
            }
            at = cx.first_child(node).or_else(|| {
                let mut up = Some(node);
                while let Some(here) = up {
                    if let Some(next) = cx.next_sibling(here) {
                        return Some(next);
                    }
                    up = cx.parent(here);
                }
                None
            });
        }
        styles.push(made);
        styles.into_iter().map(|style| cx.style_parser_line(style)).collect()
    });
    assert_eq!(lines, [Some(2), Some(6), None, None]);
}

/// An HTML element whose name the document's window's registry defines,
/// the document's parser asks its engine to construct (with its
/// attributes) as it inserts it in the document, and puts the element it
/// gives in the tree; another custom element's name, a defined one in
/// another namespace or in a template's contents, it makes itself.
#[test]
fn the_document_parser_asks_its_engine_to_construct_defined_elements() {
    use html::parser::Parser;
    let mut host = TestHost::new();
    // The DOM goes after every root: the host taken back into `host`.
    let document = host.dom.mutate(|cx| {
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        let registry = cx.custom_elements(window);
        let name = dom::names::intern("x-a");
        cx.add_custom_element_definition(
            registry,
            dom::DefinitionInit {
                name,
                local_name: name,
                observed: Box::new([]),
                disable_internals: false,
                disable_shadow: false,
            },
        );
        cx.root(document)
    });
    let mut parser = Parser::<TestHost>::new(&host.dom, &document);
    parser.push(
        "<!DOCTYPE html><p id=a></p><x-a id=x b=1><i></i></x-a><x-b></x-b><svg><x-a></x-a></svg>\
         <template><x-a id=inert></x-a></template><table><x-a id=fostered></x-a></table>",
    );
    let (_, back) = parser.resume(host);
    host = parser.finish(back);
    // Its insertion left an element queue (the test's element is undefined:
    // an upgrade reaction): the engine runs it.
    // A template's contents' (their document has no browsing context): not
    // constructed; one foster parented out of a table: constructed.
    assert_eq!(
        host.constructed,
        ["x-a id=x b=1", "reactions", "x-a id=fostered", "reactions"]
    );
    let (child, before) = host.dom.read(|cx| {
        let x = cx
            .element_by_id(document.get(cx.gc), "x")
            .expect("the constructed element in the tree");
        (
            cx.first_child(x)
                .map(|child| cx.node_name(child).to_string()),
            cx.previous_sibling(x)
                .map(|before| cx.node_name(before).to_string()),
        )
    });
    assert_eq!(
        (child.as_deref(), before.as_deref()),
        (Some("I"), Some("P"))
    );
}

/// What the fast path knows it builds itself (the same tree as the
/// fragment parsing algorithm's, which debug builds check), counted; what
/// it doesn't, or a context it doesn't take, goes to the tree builder.
#[test]
fn the_fast_path_takes_plain_markup() {
    let dom = dom();
    let (div, _) = context(&dom, QuirksMode::NoQuirks);
    let parse = |input: &str| {
        let before = html::fast_parses_for_testing();
        let fragment = html::parser::parse_fragment(&mut dom.borrow_mut(), &div, input, true);
        let fast = html::fast_parses_for_testing() != before;
        (
            html::parser::dump_children(&mut dom.borrow_mut(), &fragment.root()),
            fast,
        )
    };
    let (dump, fast) = parse("<p class=a id=\"x\">a &amp; <b>b</b></p>t<br>u");
    assert_eq!(
        dump,
        "| <p>\n|   class=\"a\"\n|   id=\"x\"\n|   \"a & \"\n|   <b>\n|     \"b\"\n| \"t\"\n| <br>\n| \"u\"\n"
    );
    // Release builds don't count.
    assert!(fast || html::fast_parses_for_testing().is_none());
    for input in [
        "<table><td>x",
        "<p><div>closes p</div>",
        "a &copy; b",
        "<P>upper</P>",
    ] {
        let (_, fast) = parse(input);
        assert!(!fast, "{input}");
    }
    let table = dom.borrow_mut().mutate(|cx| {
        let document = cx.create_document(true);
        cx.root(cx.create_element(document, QualName::html("table")))
    });
    let before = html::fast_parses_for_testing();
    html::parser::parse_fragment(&mut dom.borrow_mut(), &table, "<p>x</p>", true);
    assert_eq!(
        html::fast_parses_for_testing(),
        before,
        "not a context it takes"
    );
}

/// A document's `<template shadowrootmode>` gives its parent a declarative
/// shadow root with its settings, its contents parsed into it, the
/// template never inserted; a second one for the same host, one whose
/// parent can't host, or a mode that isn't one, is a template; the
/// fragment parsing algorithm's (`innerHTML`) are templates.
#[test]
fn declarative_shadow_roots() {
    let dom = dom();
    let document = html::parser::parse_document(
        &mut dom.borrow_mut(),
        "<!DOCTYPE html><div id=h><template shadowrootmode=open shadowrootclonable shadowrootdelegatesfocus>\
         <span id=in>in</span></template><template shadowrootmode=open><i></i></template><p>light</p></div>\
         <a id=a><template shadowrootmode=open><b></b></template></a>\
         <div id=c><template shadowrootmode=closed shadowrootserializable></template></div>\
         <div id=x><template shadowrootmode=bogus></template></div>",
        true,
    );
    dom.borrow_mut().read(|cx| {
        let document = document.root().get(cx.gc);
        let host = cx.element_by_id(document, "h").unwrap();
        let root = cx.shadow_root(host).expect("a shadow root");
        let init = cx.shadow_root_init(root).unwrap();
        assert_eq!(init.mode, dom::ShadowRootMode::Open);
        assert!(init.clonable && init.delegates_focus && !init.serializable);
        assert!(cx.is_declarative_shadow_root(root));
        let span = cx.first_child(root).expect("its contents");
        assert_eq!(cx.node_name(span).to_string(), "SPAN");
        // The second template: a template, among the light children.
        let names: Vec<String> =
            std::iter::successors(cx.first_child(host), |at| cx.next_sibling(*at))
                .map(|at| cx.node_name(at).to_string())
                .collect();
        assert_eq!(names, ["TEMPLATE", "P"]);
        for (id, shadow) in [("a", false), ("x", false)] {
            let element = cx.element_by_id(document, id).unwrap();
            assert_eq!(cx.shadow_root(element).is_some(), shadow, "{id}");
            assert_eq!(
                cx.node_name(cx.first_child(element).unwrap()).to_string(),
                "TEMPLATE",
                "{id}"
            );
        }
        let closed = cx
            .shadow_root(cx.element_by_id(document, "c").unwrap())
            .unwrap();
        let init = cx.shadow_root_init(closed).unwrap();
        assert_eq!(init.mode, dom::ShadowRootMode::Closed);
        assert!(init.serializable && !init.clonable);
    });
    let (div, _) = context(&dom, QuirksMode::NoQuirks);
    let fragment = html::parser::parse_fragment(
        &mut dom.borrow_mut(),
        &div,
        "<div><template shadowrootmode=open><i></i></template></div>",
        true,
    );
    dom.borrow_mut().read(|cx| {
        let inner = cx.first_child(fragment.root().get(cx.gc)).unwrap();
        assert!(cx.shadow_root(inner).is_none());
        assert_eq!(
            cx.node_name(cx.first_child(inner).unwrap()).to_string(),
            "TEMPLATE"
        );
    });
}

/// An iframe's `srcdoc` document's parser (HTML's iframe srcdoc document)
/// doesn't put it in quirks mode for lack of a doctype; another document's
/// does.
#[test]
fn a_srcdoc_document_is_never_quirks_for_lack_of_a_doctype() {
    use html::parser::Parser;
    for (srcdoc, expected) in [(false, QuirksMode::Quirks), (true, QuirksMode::NoQuirks)] {
        let mut host = TestHost::new();
        let document = host.dom.mutate(|cx| cx.root(cx.create_document(true)));
        let mut parser = if srcdoc {
            Parser::<TestHost>::new_srcdoc(&host.dom, &document)
        } else {
            Parser::<TestHost>::new(&host.dom, &document)
        };
        parser.push("<p>no doctype");
        let (_, back) = parser.resume(host);
        host = parser.finish(back);
        let mode = host.dom.read(|cx| cx.quirks_mode(document.get(cx.gc)));
        assert_eq!(mode, expected, "srcdoc: {srcdoc}");
    }
}
