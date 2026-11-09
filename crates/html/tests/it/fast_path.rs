//! The fragment parser's fast path: what it takes, and what it leaves to
//! the tokenizer and the tree builder. Debug builds parse each fragment the
//! fast path takes the full way too and check the trees are the same, so
//! each case here checks its tree as well.

use dom::{Cx, Dom, NodeRef, QualName, QuirksMode, TargetRef, names};

use crate::harness::{context_named, dump};

/// Whether the fast path parsed `input` in a `context` element, and what it
/// gave.
fn parse_in(context: &str, input: &str) -> (bool, String) {
    let mut dom = Dom::new();
    let (context, _) = context_named(&mut dom, QualName::html(context), QuirksMode::NoQuirks);
    let before = html::fast_parses_for_testing().expect("a debug build");
    let fragment = html::parser::parse_fragment(&mut dom, &context, input, true);
    let fast = html::fast_parses_for_testing().unwrap() > before;
    (fast, dump(&mut dom, &fragment))
}

fn fast(input: &str) -> bool {
    parse_in("div", input).0
}

#[test]
fn common_markup_takes_the_fast_path() {
    for input in [
        "",
        "text alone",
        "<li data-id=\"1\" class=\"completed\"><div class=\"view\">\
         <input class=\"toggle\" type=\"checkbox\" checked>\
         <label>Buy &amp; sell</label><button class=\"destroy\"></button></div>\
         <input class=\"edit\" value=\"Buy &amp; sell\"></li>",
        "<p>one<br>two<br/>three</p>",
        "<ul>\n  <li>a<ul><li>b</li></ul></li>\n  <li>c</li>\n</ul>",
        "<h1><span>x</span></h1><h2>y</h2>",
        "it&#39;s &#x27;quoted&#x27; &lt;b&gt; &quot;x&quot; &apos;y&apos;",
        "<img src=a.png alt=\"\"><input value=x/>",
        "<a href='#/'>all</a> <a href='#/active' class=selected>active</a>",
        "<section><header><h1>t</h1></header><footer><p>f</p></footer></section>",
        "<p><b><i>nested</i></b> <strong>well</strong></p>",
        "<button type=button><span>b</span></button>",
        "<div\n  class=\"a\"\n  id=\"b\">lines</div>",
    ] {
        assert!(fast(input), "{input:?}");
    }
}

/// What the tree builder would nest otherwise than as written, or the fast
/// path doesn't know: left to the full parser (which the tree, as dumped,
/// shows it gave).
#[test]
fn other_markup_is_left_to_the_full_parser() {
    for input in [
        "<table><tr><td>x</td></tr></table>",
        "<p>a<div>b</div></p>",
        "<li>a<li>b",
        "<b><i>x</b></i>",
        "<a href=1>x<a href=2>y</a></a>",
        "<button>a<button>b</button></button>",
        "<h1>a<h2>b</h2></h1>",
        "<DIV>caps</DIV>",
        "<div CLASS=a></div>",
        "a&nbsp;b",
        "a &amp b",
        "a&#0;b",
        "a&#x80;b",
        "<!-- comment -->",
        "<div/>",
        "a\r\nb",
        "<p>x</div>",
        "a < b",
        "<custom-element></custom-element>",
        "<script>x</script>",
        "<div class=a class=b></div>",
        "<div a =b></div>",
        "<div a=\"1\"b=\"2\"></div>",
        "</p>",
        "<br></br>",
    ] {
        assert!(!fast(input), "{input:?}");
    }
    // Contexts other than "in body" with nothing open.
    assert!(!parse_in("tr", "<td>x</td>").0);
    assert!(!parse_in("select", "<option>x</option>").0);
    assert!(!parse_in("template", "<p>x</p>").0);
    assert!(parse_in("ul", "<li>x</li>").0);
}

/// The inline event handlers under `node`, in tree order: each one's
/// attribute and the line its source is on.
fn handler_lines<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> Vec<(String, u32)> {
    let mut lines = Vec::new();
    for index in 0..cx.attribute_count(node) {
        let Some(attribute) = cx.attribute_at(node, index) else {
            continue;
        };
        let Some(handler) = names::handler_attribute(attribute.name.local) else {
            continue;
        };
        let listener = cx
            .event_handler(TargetRef::Node(node), handler.event_type)
            .expect("the attribute's handler");
        let (_, line, _) = cx.uncompiled_handler(listener).expect("not compiled");
        lines.push((names::get(attribute.name.local).to_string(), line));
    }
    let mut child = cx.first_child(node);
    while let Some(at) = child {
        lines.extend(handler_lines(cx, at));
        child = cx.next_sibling(at);
    }
    lines
}

/// The line each element's attributes are on, as its inline event
/// handlers take it (where their errors count from): the line of the
/// tag's `>`, as the tokenizer's when it gives the tag. A carriage return,
/// which the input stream turns into a line feed, is left to the full
/// parser.
#[test]
fn inline_handlers_get_each_tags_line() {
    let lines = |input: &str| {
        let mut dom = Dom::new();
        let (context, _) = context_named(&mut dom, QualName::html("div"), QuirksMode::NoQuirks);
        let before = html::fast_parses_for_testing().expect("a debug build");
        let fragment = html::parser::parse_fragment(&mut dom, &context, input, true);
        let fast = html::fast_parses_for_testing().unwrap() > before;
        let lines = dom.read(|cx| handler_lines(cx, fragment.root().get(cx.gc)));
        (fast, lines)
    };
    let input = "<p\n  onclick=\"a\nb\"\n  onkeydown=t>x</p>\n<span\tonclick=c>y</span>";
    let expected = vec![
        ("onclick".to_owned(), 4),
        ("onkeydown".to_owned(), 4),
        ("onclick".to_owned(), 5),
    ];
    assert_eq!(lines(input), (true, expected.clone()));
    // The full parser's are the same.
    assert_eq!(
        lines(&input.replace("<p", "<P").replace("</p>", "</P>")),
        (false, expected)
    );
    let (fast, _) = lines("<p\r\nonclick=a>x</p>");
    assert!(!fast, "a carriage return");
    // A document's, the lines before it skipped counted too.
    let mut dom = Dom::new();
    let document = html::parser::parse_document(&mut dom, "\n\n<p onclick=a>x</p>", false);
    let lines = dom.read(|cx| handler_lines(cx, document.root().get(cx.gc)));
    assert_eq!(lines, [("onclick".to_owned(), 3)]);
}

/// A whole document of what the fast path takes (`DOMParser`'s): with no
/// DOCTYPE, in quirks mode, the white space before it skipped, in the
/// `<body>` of an `<html>` with an empty `<head>`, as the tree builder
/// makes it. A DOCTYPE, or what goes in the head, is the full parser's.
#[test]
fn a_simple_document_takes_the_fast_path() {
    let parse = |input: &str| {
        let mut dom = Dom::new();
        let before = html::fast_parses_for_testing().expect("a debug build");
        let document = html::parser::parse_document(&mut dom, input, false);
        let fast = html::fast_parses_for_testing().unwrap() > before;
        let quirks = dom.read(|cx| cx.quirks_mode(document.root().get(cx.gc)));
        (fast, quirks, dump(&mut dom, &document))
    };
    let (fast, quirks, dump) = parse("\n  <li class=a>x</li>\n");
    assert!(fast);
    assert_eq!(quirks, QuirksMode::Quirks);
    assert_eq!(
        dump,
        "\
| <html>
|   <head>
|   <body>
|     <li>
|       class=\"a\"
|       \"x\"
|     \"
\"
"
    );
    assert!(parse("").0);
    assert!(parse("just text").0);
    assert!(!parse("<!DOCTYPE html><p>x</p>").0);
    assert!(!parse("<title>t</title><p>x</p>").0);
    assert!(!parse("<html><body>x</body></html>").0);
}

/// The inputs the fast path makes are made of their types (as Blink's
/// parser gives an input its type before its other attributes), not text
/// inputs changed to them, and are the full parser's: each type,
/// checkedness and value (two checked radio buttons of a name both
/// checked: in no document they are in no group, as in Blink).
#[test]
fn inputs_are_made_of_their_types() {
    let states = |input: &str| {
        let mut dom = Dom::new();
        let (context, _) = context_named(&mut dom, QualName::html("div"), QuirksMode::NoQuirks);
        let parses = html::fast_parses_for_testing().expect("a debug build");
        let changes = dom::input_type_changes_for_testing().expect("a debug build");
        let fragment = html::parser::parse_fragment(&mut dom, &context, input, true);
        let fast = html::fast_parses_for_testing().unwrap() > parses;
        let changes = dom::input_type_changes_for_testing().unwrap() - changes;
        let states = dom.read(|cx| {
            let mut states = Vec::new();
            let mut child = cx.first_child(fragment.root().get(cx.gc));
            while let Some(at) = child {
                states.push((
                    cx.input_kind(at),
                    cx.checkedness(at),
                    cx.input_value(at).to_string(),
                ));
                child = cx.next_sibling(at);
            }
            states
        });
        (fast, changes, states)
    };
    let input = "<input type=checkbox checked><input checked type=radio name=r>\
                 <input type=RADIO name=r checked><input value=v><input type=number value=12>";
    let (fast, changes, fast_states) = states(input);
    assert!(fast);
    let (full, full_changes, full_states) = states(&input.replace("<input", "<INPUT"));
    assert!(!full);
    assert_eq!(fast_states, full_states);
    assert_eq!(
        fast_states
            .iter()
            .map(|&(kind, checked, _)| (kind, checked))
            .collect::<Vec<_>>(),
        [
            (Some(dom::InputKind::Checkbox), true),
            (Some(dom::InputKind::Radio), true),
            (Some(dom::InputKind::Radio), true),
            (Some(dom::InputKind::Text), false),
            (Some(dom::InputKind::Number), false),
        ]
    );
    // The full parser changes the four given a type to it; a debug build
    // parses what the fast path takes the full way too, so those four are
    // all the fast path's parse counts.
    assert_eq!(full_changes, 4);
    assert_eq!(changes, full_changes);
}

/// Each text and attribute value is decoded once, as the scan checks it:
/// building takes what the scan decoded.
#[test]
fn each_text_and_value_is_decoded_once() {
    let mut dom = Dom::new();
    let (context, _) = context_named(&mut dom, QualName::html("div"), QuirksMode::NoQuirks);
    let before = html::decodes_for_testing().expect("a debug build");
    let fragment = html::parser::parse_fragment(
        &mut dom,
        &context,
        "<p class=\"a &amp; b\" id=x>one &lt; two</p>three",
        true,
    );
    assert_eq!(html::decodes_for_testing().unwrap() - before, 4);
    assert_eq!(
        dump(&mut dom, &fragment),
        "\
| <p>
|   class=\"a & b\"
|   id=\"x\"
|   \"one < two\"
| \"three\"
"
    );
}
