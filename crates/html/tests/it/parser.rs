//! `Parser`, the parser that pauses at scripts: where it stops, and
//! that the DOM is the caller's while it waits.

use dom::Dom;
use html::parser::ParserState;

use crate::harness::DocumentParser;

#[test]
fn stops_at_each_script_before_parsing_what_follows() {
    let mut parser = DocumentParser::new();
    parser.push("<p>a</p><script>one()</script><p>b</p><script>two()</script><p>c");

    let ParserState::Script {
        element: first,
        line: 1,
    } = parser.resume()
    else {
        panic!("no stop at the first script, on line 1");
    };
    assert_eq!(parser.script_text(&first), "one()");
    let so_far = parser.dump();
    assert!(so_far.contains("\"a\""), "{so_far}");
    assert!(
        !so_far.contains("\"b\""),
        "parsed past the script:\n{so_far}"
    );

    let ParserState::Script {
        element: second, ..
    } = parser.resume()
    else {
        panic!("no stop at the second script");
    };
    assert_eq!(parser.script_text(&second), "two()");
    assert!(parser.dump().contains("\"b\""));
    assert!(matches!(parser.resume(), ParserState::NeedsInput));

    parser.finish();
    assert!(parser.dump().contains("\"c\""));
}

/// While the parser waits, the DOM is free: what "the script" changes is
/// there, and parsing carries on after it.
#[test]
fn the_dom_is_free_while_the_parser_waits() {
    let mut parser = DocumentParser::new();
    parser.push("<body><script>s</script><p>after");
    let ParserState::Script {
        element: script, ..
    } = parser.resume()
    else {
        panic!("no stop at the script");
    };
    let document = parser.document.clone();
    parser.dom().mutate(|cx| {
        let body = cx.parent(script.root().get(cx.gc)).unwrap();
        let added = cx.create_text(document.get(cx.gc), "added");
        cx.append_child(body, added).unwrap();
    });
    assert!(matches!(parser.resume(), ParserState::NeedsInput));
    parser.finish();
    let tree = parser.dump();
    let added = tree.find("\"added\"").expect("the script's change is gone");
    let after = tree.find("\"after\"").expect("parsing stopped");
    assert!(added < after, "{tree}");
}

/// Input in pieces, split anywhere, even inside the script.
#[test]
fn input_may_come_in_pieces() {
    let mut parser = DocumentParser::new();
    let mut scripts = Vec::new();
    for piece in ["<p>x</p><scr", "ipt>let a = '</sc", "';</script><p", ">y"] {
        parser.push(piece);
        scripts.extend(parser.scripts());
    }
    parser.finish();
    assert_eq!(scripts, ["let a = '</sc';"]);
    assert!(parser.dump().contains("\"y\""));
}

/// HTML: a script the input ends inside of isn't run, so the parser doesn't
/// stop for it.
#[test]
fn a_script_cut_off_by_the_end_is_not_returned() {
    let mut parser = DocumentParser::new();
    parser.push("<p>x<script>never()");
    assert!(parser.scripts().is_empty());
    parser.finish();
    assert!(parser.dump().contains("never()"));
}

/// The same tree as parsing in one go.
#[test]
fn builds_the_same_tree_as_parse_document() {
    let input = "<!DOCTYPE html><title>t</title><table><tr><td>a<script>x</script>b</table><template><i>c</i></template>";
    let mut parser = DocumentParser::new();
    parser.push(input);
    parser.scripts();
    parser.finish();
    let incremental = parser.dump();

    let mut other = Dom::new();
    let whole = html::parser::parse_document(&mut other, input, true);
    let at_once = html::parser::dump_children(&mut other, &whole.root());
    assert_eq!(incremental, at_once);
}

/// The line each script's start tag ends on, where its text begins, for
/// error messages: a tag split over lines counts where it ends.
#[test]
fn reports_the_line_each_script_starts_on() {
    let mut parser = DocumentParser::new();
    parser.push("<p>a\n\n<script>\nx()</script>\n<p>b<script\n>y()</script>");
    let mut lines = Vec::new();
    while let ParserState::Script { line, .. } = parser.resume() {
        lines.push(line);
    }
    parser.finish();
    assert_eq!(lines, [3, 6]);
}

/// A control foster parented out of a table that script removed (it has no
/// parent) is in the tree of the element before the table, which is the
/// form element pointer's: the parser associates it with that form.
#[test]
fn a_control_foster_parented_from_a_removed_table_gets_the_form() {
    let mut parser = DocumentParser::new();
    parser.push("<form id=f><table><script>remove()</script><input id=i>");
    let ParserState::Script { element, .. } = parser.resume() else {
        panic!("no stop at the script");
    };
    parser.dom().mutate(|cx| {
        let table = cx.parent(element.root().get(cx.gc)).unwrap();
        cx.remove(table);
    });
    while let ParserState::Script { .. } = parser.resume() {}
    parser.finish();
    let document = parser.document.clone();
    parser.dom().read(|cx| {
        let document = document.get(cx.gc);
        let input = cx.element_by_id(document, "i").expect("the input");
        let form = cx.element_by_id(document, "f").expect("the form");
        assert!(
            cx.parent(input)
                .is_some_and(|parent| elise::Gc::ptr_eq(parent, form))
        );
        assert!(
            cx.form_owner(input)
                .is_some_and(|owner| elise::Gc::ptr_eq(owner, form))
        );
    });
}

/// The form element pointer's form, removed from the document by script
/// (`<div><form></div>`, so it isn't an open element), is in another tree
/// than the controls the parser makes next: they aren't associated with it.
#[test]
fn a_form_script_removed_owns_no_later_control() {
    let mut parser = DocumentParser::new();
    parser.push("<div><form id=f></div><script>remove()</script><input id=i>");
    let ParserState::Script { .. } = parser.resume() else {
        panic!("no stop at the script");
    };
    let document = parser.document.clone();
    let form = parser.dom().mutate(|cx| {
        let form = cx
            .element_by_id(document.get(cx.gc), "f")
            .expect("the form");
        cx.remove(form);
        cx.root(form)
    });
    while let ParserState::Script { .. } = parser.resume() {}
    parser.finish();
    parser.dom().read(|cx| {
        let body = cx
            .first_child(document.get(cx.gc))
            .and_then(|html| cx.last_child(html))
            .expect("a body");
        let input = cx.last_child(body).expect("the input");
        assert_eq!(
            cx.get_attribute(input, "id").map(|id| id.to_string()),
            Some("i".to_owned())
        );
        assert!(cx.form_owner(input).is_none());
    });
    drop(form);
}
