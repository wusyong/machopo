//! DevTools' `CSS` domain, which the tab's renderer answers for the
//! Elements panel's Computed and Styles: `DOM` first; an element's
//! computed values as `getComputedStyle` reads them (a text node's its
//! parent's); the rules it matches in cascade order with the selectors
//! that match, its inline style and presentational hints, its layout
//! parent; the fonts of its text; its style followed while Computed is
//! open; the overlay label's styles; all of it while script is paused too;
//! the style sheets, with their texts, told as they come and go, and the
//! sheet each rule is of.

use net::test_server::Route;
use serde_json::{Value, json};

use crate::automation_pipe::{pixel, png, server};
use crate::devtools::{evaluate, load, page};
use crate::devtools_overlay::{non_white, rgba};
use crate::remote_debugging::{Client, Machopo};

fn result(answer: Value) -> Value {
    assert!(answer.get("error").is_none(), "{answer}");
    answer["result"].clone()
}

fn error(answer: Value) -> String {
    answer["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("an error: {answer}"))
        .to_owned()
}

/// `html` loaded, `DOM` and `CSS` on: the session and the document's node.
fn css_page(html: &str) -> (Machopo, Client, Value, net::test_server::TestServer) {
    css_site(&[("/", Route::ok("text/html", html))])
}

/// `css_page` of a site with `routes`, its page at `/`.
fn css_site(routes: &[(&str, Route)]) -> (Machopo, Client, Value, net::test_server::TestServer) {
    let server = server(routes);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    result(page.call(None, "DOM.enable", json!({})));
    let root = result(page.call(None, "DOM.getDocument", json!({})))["root"]["nodeId"].clone();
    result(page.call(None, "CSS.enable", json!({})));
    (machopo, page, root, server)
}

fn node(page: &mut Client, document: &Value, selector: &str) -> Value {
    result(page.call(
        None,
        "DOM.querySelector",
        json!({ "nodeId": document, "selector": selector }),
    ))["nodeId"]
        .clone()
}

/// The first child of `element`, pushed to the session: its number.
fn first_child(page: &mut Client, element: &Value) -> Value {
    let described = result(page.call(
        None,
        "DOM.describeNode",
        json!({ "nodeId": element, "depth": 1 }),
    ));
    let backend = described["node"]["children"][0]["backendNodeId"].clone();
    result(page.call(
        None,
        "DOM.pushNodesByBackendIdsToFrontend",
        json!({ "backendNodeIds": [backend] }),
    ))["nodeIds"][0]
        .clone()
}

/// `getComputedStyleForNode`'s values by name.
fn computed(page: &mut Client, node: &Value) -> serde_json::Map<String, Value> {
    let answer = result(page.call(
        None,
        "CSS.getComputedStyleForNode",
        json!({ "nodeId": node }),
    ));
    answer["computedStyle"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["name"].as_str().unwrap().to_owned(),
                entry["value"].clone(),
            )
        })
        .collect()
}

/// A `CSS.CSSStyle`'s declarations, as `name: value` (`!` when important,
/// `?` when implicit).
fn declarations(style: &Value) -> Vec<String> {
    style["cssProperties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|property| {
            let mut text = format!(
                "{}: {}",
                property["name"].as_str().unwrap(),
                property["value"].as_str().unwrap()
            );
            if property["important"] == true {
                text.push('!');
            }
            if property["implicit"] == true {
                text.push('?');
            }
            text
        })
        .collect()
}

/// `CSS` needs `DOM`, and answers nothing else until it is on; off again,
/// it doesn't.
#[test]
fn css_needs_dom_and_to_be_on() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html><p>p"))]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    assert_eq!(
        error(page.call(None, "CSS.enable", json!({}))),
        "DOM agent needs to be enabled first."
    );
    result(page.call(None, "DOM.enable", json!({})));
    let root = result(page.call(None, "DOM.getDocument", json!({})))["root"]["nodeId"].clone();
    let p = node(&mut page, &root, "p");
    let not_on = page.call(None, "CSS.getComputedStyleForNode", json!({ "nodeId": p }));
    assert_eq!(error(not_on), "CSS agent was not enabled");
    result(page.call(None, "CSS.enable", json!({})));
    assert!(!computed(&mut page, &p).is_empty());
    result(page.call(None, "CSS.disable", json!({})));
    let off = page.call(None, "CSS.getMatchedStylesForNode", json!({ "nodeId": p }));
    assert_eq!(error(off), "CSS agent was not enabled");
}

/// Computed values: every property `getComputedStyle` lists, with its
/// value, layout's where it decides it; a text node's are its parent's; an
/// element with no box has its computed values; one no frame shows, none.
#[test]
fn computed_styles_are_read() {
    let (_machopo, mut page, document, _server) = css_page(
        "<!DOCTYPE html><style>body { margin: 0; width: 400px } \
         #a { width: 50%; color: rgb(1, 2, 3); --x: 1 } #n { display: none; width: 50% }</style>\
         <div id=a>text</div><div id=n></div><template><i>t</i></template>",
    );
    let a = node(&mut page, &document, "#a");
    let values = computed(&mut page, &a);
    let length = evaluate(&mut page, "getComputedStyle(a).length", None);
    assert_eq!(json!(values.len()), length);
    assert_eq!(values["width"], "200px");
    assert_eq!(values["color"], "rgb(1, 2, 3)");
    assert_eq!(values["--x"], "1");
    let extra = result(page.call(None, "CSS.getComputedStyleForNode", json!({ "nodeId": a })))["extraFields"].clone();
    assert_eq!(extra, json!({ "isAppearanceBase": false }));
    let text = first_child(&mut page, &a);
    assert_eq!(computed(&mut page, &text)["color"], "rgb(1, 2, 3)");
    let hidden = node(&mut page, &document, "#n");
    assert_eq!(computed(&mut page, &hidden)["width"], "50%");
    // Script's changes, as they are now.
    evaluate(&mut page, "a.style.width = '10px'", None);
    assert_eq!(computed(&mut page, &a)["width"], "10px");
    let object = page.call(
        None,
        "Runtime.evaluate",
        json!({ "expression": "document.querySelector('template').content.firstChild" }),
    );
    let object = object["result"]["result"]["objectId"].clone();
    let i =
        result(page.call(None, "DOM.requestNode", json!({ "objectId": object })))["nodeId"].clone();
    assert!(computed(&mut page, &i).is_empty());
}

/// The rules an element matches: the UA's first, then the author's by
/// specificity and where they come, each with the selectors of its list
/// that match and their specificity, its declarations as parsed (a
/// shorthand's longhands, with the shorthand apart); its inline style and
/// presentational hints apart; its layout parent (past `display:
/// contents`), sent before the answer; a shadow tree's `:host` rule; a text
/// node is no element.
#[test]
fn matched_rules_are_listed() {
    let (_machopo, mut page, document, _server) = css_page(
        "<!DOCTYPE html><style>#x { color: red !important } div { color: green } \
         .a, p, div.a { margin: 1px 2px } span { color: gray } div::before { content: 'b' }</style>\
         <section style='display: contents'><div id=x class=a style='border-top: solid'>x</div></section>\
         <font id=f color=red>f</font><div id=host></div>",
    );
    let x = node(&mut page, &document, "#x");
    let matched = result(page.call(None, "CSS.getMatchedStylesForNode", json!({ "nodeId": x })));
    let rules = matched["matchedCSSRules"].as_array().unwrap();
    let first_author = rules
        .iter()
        .position(|rule| rule["rule"]["origin"] == "regular")
        .unwrap();
    assert!(first_author > 0);
    assert!(
        rules[..first_author]
            .iter()
            .all(|rule| rule["rule"]["origin"] == "user-agent")
    );
    let author: Vec<(String, Value)> = rules[first_author..]
        .iter()
        .map(|rule| {
            (
                rule["rule"]["selectorList"]["text"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
                rule["matchingSelectors"].clone(),
            )
        })
        .collect();
    assert_eq!(
        author,
        [
            ("div".to_owned(), json!([0])),
            (".a, p, div.a".to_owned(), json!([0, 2])),
            ("#x".to_owned(), json!([0])),
        ]
    );
    let margin = &rules[first_author + 1]["rule"];
    assert_eq!(
        margin["selectorList"]["selectors"][2],
        json!({ "text": "div.a", "specificity": { "a": 0, "b": 1, "c": 1 } })
    );
    assert_eq!(
        declarations(&margin["style"]),
        [
            "margin-top: 1px",
            "margin-right: 2px",
            "margin-bottom: 1px",
            "margin-left: 2px"
        ]
    );
    assert_eq!(
        margin["style"]["shorthandEntries"],
        json!([{ "name": "margin", "value": "1px 2px" }])
    );
    assert_eq!(
        declarations(&rules[first_author + 2]["rule"]["style"]),
        ["color: red!"]
    );
    assert_eq!(
        declarations(&matched["inlineStyle"]),
        [
            "border-top-width: medium?",
            "border-top-style: solid",
            "border-top-color: currentcolor?"
        ]
    );
    assert!(matched.get("attributesStyle").is_none());
    // The `<section>` is `display: contents`: the `<body>`, pushed first.
    let body = node(&mut page, &document, "body");
    assert_eq!(matched["parentLayoutNodeId"], body);

    let f = node(&mut page, &document, "#f");
    let matched = result(page.call(None, "CSS.getMatchedStylesForNode", json!({ "nodeId": f })));
    assert_eq!(
        declarations(&matched["attributesStyle"]),
        ["color: rgb(255, 0, 0)"]
    );
    assert_eq!(
        matched["inlineStyle"],
        json!({ "cssProperties": [], "shorthandEntries": [] })
    );
    let inline = result(page.call(None, "CSS.getInlineStylesForNode", json!({ "nodeId": f })));
    assert_eq!(inline["attributesStyle"], matched["attributesStyle"]);
    assert_eq!(inline["inlineStyle"], matched["inlineStyle"]);

    evaluate(
        &mut page,
        "host.attachShadow({ mode: 'open' }).innerHTML = '<style>:host { color: blue }</style>'",
        None,
    );
    let host = node(&mut page, &document, "#host");
    let matched = result(page.call(
        None,
        "CSS.getMatchedStylesForNode",
        json!({ "nodeId": host }),
    ));
    let selectors: Vec<&str> = matched["matchedCSSRules"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|rule| rule["rule"]["origin"] == "regular")
        .map(|rule| rule["rule"]["selectorList"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(selectors, [":host", "div"]);

    let text = first_child(&mut page, &x);
    let refused = page.call(
        None,
        "CSS.getMatchedStylesForNode",
        json!({ "nodeId": text }),
    );
    assert_eq!(error(refused), "Node is not an Element");
    let refused = page.call(
        None,
        "CSS.getMatchedStylesForNode",
        json!({ "nodeId": 999_999 }),
    );
    assert_eq!(error(refused), "Could not find node with given id");
}

/// The fonts of an element's text, its children's and generated
/// content's, by glyph; not its grandchildren's; a text node's own.
#[test]
fn fonts_are_counted() {
    let (_machopo, mut page, document, _server) = css_page(
        "<!DOCTYPE html><style>div { font: 10px Ahem } div::before { content: 'xy' } \
         b { font: 10px Lato } i { font: 10px serif }</style>\
         <div id=a>abc <b>de</b> <span><i>far</i></span></div>",
    );
    let a = node(&mut page, &document, "#a");
    let fonts =
        result(page.call(None, "CSS.getPlatformFontsForNode", json!({ "nodeId": a })))["fonts"]
            .clone();
    assert_eq!(
        fonts,
        json!([
            // `xy`, `abc `, ` ` (between the `<b>` and the `<span>`).
            { "familyName": "Ahem", "postScriptName": "Ahem", "isCustomFont": false, "glyphCount": 7 },
            { "familyName": "Lato", "postScriptName": "Lato-Medium", "isCustomFont": false, "glyphCount": 2 },
        ])
    );
    let text = first_child(&mut page, &a);
    let fonts = result(page.call(
        None,
        "CSS.getPlatformFontsForNode",
        json!({ "nodeId": text }),
    ))["fonts"]
        .clone();
    assert_eq!(fonts[0]["glyphCount"], 4);
    assert_eq!(fonts.as_array().unwrap().len(), 1);
}

/// Two frames from now: what a frame tells has come.
fn two_frames(page: &mut Client) {
    result(page.call(
        None,
        "Runtime.evaluate",
        json!({
            "expression": "new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))",
            "awaitPromise": true,
        }),
    ));
}

/// The node Computed follows: each change of its style told, one of its
/// parent's it inherits too; not another node's; none once it stops.
#[test]
fn computed_style_updates_are_told() {
    let (_machopo, mut page, document, _server) = css_page(
        "<!DOCTYPE html><style>.red { color: red }</style><div id=p><div id=x>x</div></div><div id=o></div>",
    );
    let x = node(&mut page, &document, "#x");
    result(page.call(
        None,
        "CSS.trackComputedStyleUpdatesForNode",
        json!({ "nodeId": x }),
    ));
    evaluate(&mut page, "x.style.paddingLeft = '7px'", None);
    let told = page.event("CSS.computedStyleUpdated");
    assert_eq!(told["params"]["nodeId"], x);
    evaluate(&mut page, "p.className = 'red'", None);
    assert_eq!(
        page.event("CSS.computedStyleUpdated")["params"]["nodeId"],
        x
    );
    // Another's: none.
    two_frames(&mut page);
    page.take_events("CSS.computedStyleUpdated");
    evaluate(&mut page, "o.style.color = 'blue'", None);
    two_frames(&mut page);
    assert!(page.take_events("CSS.computedStyleUpdated").is_empty());
    result(page.call(None, "CSS.trackComputedStyleUpdatesForNode", json!({})));
    evaluate(&mut page, "x.style.paddingLeft = '9px'", None);
    two_frames(&mut page);
    assert!(page.take_events("CSS.computedStyleUpdated").is_empty());
}

/// How many of the page's pixels from `from` to `to` (corners, exclusive)
/// are about black.
fn black(page: &mut Client, from: (u32, u32), to: (u32, u32)) -> usize {
    let shot = result(page.call(None, "Page.captureScreenshot", json!({})));
    let ((width, _), pixels) = png(shot["data"].as_str().unwrap());
    (from.1..to.1)
        .flat_map(|y| (from.0..to.0).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            pixel(&pixels, width, x, y)
                .iter()
                .all(|&channel| channel < 50)
        })
        .count()
}

/// The overlay's label with styles: an element with text its color (as
/// `#RRGGBBAA` whatever the format asked, which the overlay writes it in,
/// and as fractions), font, padding, margin and background; one without
/// text none of the color and font.
#[test]
fn labels_show_styles() {
    let (_machopo, mut page, document, _server) = css_page(
        "<!DOCTYPE html><style>#a { color: rgb(255, 0, 0); font: 12px/20px Ahem; padding: 3px; \
         background-color: rgba(0, 0, 255, 0.5) } #e { margin: 4px }</style>\
         <div id=a>a</div><div id=e><b></b></div>",
    );
    result(page.call(None, "Overlay.enable", json!({})));
    let a = node(&mut page, &document, "#a");
    let highlight = result(page.call(
        None,
        "Overlay.getHighlightObjectForTest",
        json!({ "nodeId": a, "includeStyle": true }),
    ))["highlight"]
        .clone();
    assert_eq!(
        highlight["elementInfo"]["style"],
        json!({
            "color": "#FF0000FF",
            "color-unclamped-rgba": [1.0, 0.0, 0.0, 1.0],
            "font-family": "Ahem",
            "font-size": "12px",
            "line-height": "20px",
            "padding": "3px",
            "margin": "0px",
            "background-color": "#0000FF80",
            "background-color-unclamped-rgba": [0.0, 0.0, 1.0, 128.0 / 255.0],
        })
    );
    let e = node(&mut page, &document, "#e");
    let highlight = result(page.call(
        None,
        "Overlay.getHighlightObjectForTest",
        json!({ "nodeId": e, "includeStyle": true, "colorFormat": "rgb" }),
    ))["highlight"]
        .clone();
    assert_eq!(
        highlight["elementInfo"]["style"],
        json!({
            "padding": "0px",
            "margin": "4px",
            "background-color": "#00000000",
            "background-color-unclamped-rgba": [0.0, 0.0, 0.0, 0.0],
        })
    );
    // Without styles asked for, none.
    let plain = result(page.call(
        None,
        "Overlay.getHighlightObjectForTest",
        json!({ "nodeId": a }),
    ))["highlight"]
        .clone();
    assert!(plain["elementInfo"].get("style").is_none());

    // Drawn: the rows under the label's first line, there only with them;
    // their values black (the names are gray, the header purple, blue and
    // gray).
    let mut config = json!({ "contentColor": rgba(255, 0, 0), "showInfo": true });
    result(page.call(
        None,
        "Overlay.highlightNode",
        json!({ "nodeId": a, "highlightConfig": config }),
    ));
    let below = |page: &mut Client| non_white(page, (0, 70), (400, 200));
    let without = below(&mut page);
    assert_eq!(black(&mut page, (0, 40), (400, 200)), 0);
    config["showStyles"] = true.into();
    result(page.call(
        None,
        "Overlay.highlightNode",
        json!({ "nodeId": a, "highlightConfig": config }),
    ));
    let with = below(&mut page);
    assert!(with > without + 100, "{with} against {without}");
    assert!(black(&mut page, (0, 40), (400, 200)) > 20);
}

/// While script is paused: the computed values and the rules as the
/// script left them.
#[test]
fn styles_are_there_while_paused() {
    let (_machopo, mut page, document, _server) =
        css_page("<!DOCTYPE html><style>.wide { width: 30px }</style><div id=x>x</div>");
    let x = node(&mut page, &document, "#x");
    page.call(None, "Debugger.enable", json!({}));
    let evaluated = page.send(
        None,
        "Runtime.evaluate",
        json!({ "expression": "x.className = 'wide'; debugger; 1" }),
    );
    page.event("Debugger.paused");
    assert_eq!(computed(&mut page, &x)["width"], "30px");
    let matched = result(page.call(None, "CSS.getMatchedStylesForNode", json!({ "nodeId": x })));
    assert!(
        matched["matchedCSSRules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule["rule"]["selectorList"]["text"] == ".wide")
    );
    page.call(None, "Debugger.resume", json!({}));
    page.answer(evaluated);
}

/// The `CSS.styleSheetAdded` events that came already, unread: their
/// headers.
fn added(page: &mut Client) -> Vec<Value> {
    page.take_events("CSS.styleSheetAdded")
        .into_iter()
        .map(|event| event["params"]["header"].clone())
        .collect()
}

/// `getStyleSheetText` of the sheet `id`.
fn sheet_text(page: &mut Client, id: &Value) -> String {
    let answer = result(page.call(
        None,
        "CSS.getStyleSheetText",
        json!({ "styleSheetId": id }),
    ));
    answer["text"].as_str().unwrap().to_owned()
}

/// A header's place: its start's line and column, its end's, its length.
fn place(header: &Value) -> [&Value; 5] {
    [
        &header["startLine"],
        &header["startColumn"],
        &header["endLine"],
        &header["endColumn"],
        &header["length"],
    ]
}

const SHEETS: &str = "<!DOCTYPE html>
<link rel=stylesheet href=/style.css>
<style media=screen title=t>
p { color: red }
</style>
<p class=a>p</p><div id=host></div>
<script>
  const sheet = new CSSStyleSheet();
  sheet.replaceSync('.a { margin: 1px }');
  document.adoptedStyleSheets = [sheet];
  host.attachShadow({ mode: 'open' }).innerHTML = '<style>span { color: blue }</style><span>s</span>';
</script>";

/// Each sheet told as `CSS` comes on, before its answer: the document's
/// (a `<link>`'s, a `<style>`'s, an adopted one's) and a shadow root's;
/// with its URL, owner, title and place (a `<style>`'s text where it is in
/// the markup); its text, a `<link>`'s as it came; and the sheet each rule
/// an element matches is of.
#[test]
fn style_sheets_are_told_with_their_texts() {
    let link = "/* as it came */\np { padding: 2px }\n";
    let (_machopo, mut page, document, server) = css_site(&[
        ("/", Route::ok("text/html", SHEETS)),
        ("/style.css", Route::ok("text/css", link)),
    ]);
    let headers = added(&mut page);
    assert_eq!(headers.len(), 4, "{headers:?}");
    let [from_link, from_style, adopted, shadow] = &headers[..] else {
        unreachable!()
    };
    let backend = |page: &mut Client, selector: &str| {
        let element = node(page, &document, selector);
        result(page.call(None, "DOM.describeNode", json!({ "nodeId": element })))["node"]
            ["backendNodeId"]
            .clone()
    };
    let page_url = server.url("/");

    assert_eq!(from_link["sourceURL"], server.url("/style.css"));
    assert_eq!(from_link["ownerNode"], backend(&mut page, "link"));
    assert_eq!(
        (&from_link["isInline"], &from_link["loadingFailed"]),
        (&json!(false), &json!(false))
    );
    assert_eq!(sheet_text(&mut page, &from_link["styleSheetId"]), link);
    // Kept as it came: not fetched again.
    let fetched = server.requests().iter().filter(|request| request.target == "/style.css").count();
    assert_eq!(fetched, 1);
    assert_eq!(place(from_link), [&json!(0), &json!(0), &json!(2), &json!(0), &json!(36)]);

    assert_eq!(from_style["sourceURL"], page_url);
    assert_eq!(from_style["ownerNode"], backend(&mut page, "style"));
    assert_eq!(
        (&from_style["isInline"], &from_style["title"], &from_style["isMutable"]),
        (&json!(true), &json!("t"), &json!(false))
    );
    assert!(from_style.get("loadingFailed").is_none(), "{from_style}");
    assert_eq!(
        sheet_text(&mut page, &from_style["styleSheetId"]),
        "\np { color: red }\n"
    );
    // After `<style media=screen title=t>`, on the markup's third line.
    assert_eq!(place(from_style), [&json!(2), &json!(28), &json!(4), &json!(0), &json!(18)]);

    assert_eq!(
        (&adopted["sourceURL"], &adopted["isConstructed"], &adopted["isMutable"]),
        (&json!(""), &json!(true), &json!(true))
    );
    assert!(adopted.get("ownerNode").is_none(), "{adopted}");
    assert_eq!(
        sheet_text(&mut page, &adopted["styleSheetId"]),
        ".a { margin: 1px; }\n"
    );

    assert_eq!(
        (&shadow["sourceURL"], &shadow["isInline"]),
        (&json!(page_url), &json!(true))
    );
    assert_eq!(
        sheet_text(&mut page, &shadow["styleSheetId"]),
        "span { color: blue }"
    );
    let ids: Vec<&Value> = headers.iter().map(|header| &header["styleSheetId"]).collect();
    assert!(
        ids.iter().enumerate().all(|(at, id)| !ids[..at].contains(id)),
        "{ids:?}"
    );
    assert!(
        headers.iter().all(|header| header["frameId"] == headers[0]["frameId"]),
        "{headers:?}"
    );

    // The rules `p` matches, each with its sheet's id.
    let p = node(&mut page, &document, "p");
    let matched = result(page.call(None, "CSS.getMatchedStylesForNode", json!({ "nodeId": p })));
    let of = |selector: &str, property: &str| {
        let rule = matched["matchedCSSRules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|matched| &matched["rule"])
            .find(|rule| {
                rule["selectorList"]["text"] == selector
                    && rule["style"]["cssProperties"][0]["name"] == property
            })
            .unwrap_or_else(|| panic!("{selector} {{ {property} }}: {matched}"));
        assert_eq!(rule["style"]["styleSheetId"], rule["styleSheetId"]);
        rule["styleSheetId"].clone()
    };
    assert_eq!(of("p", "padding-top"), from_link["styleSheetId"]);
    assert_eq!(of("p", "color"), from_style["styleSheetId"]);
    assert_eq!(of(".a", "margin-top"), adopted["styleSheetId"]);
    // The UA's: of none.
    let ua = matched["matchedCSSRules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|matched| matched["rule"]["origin"] == "user-agent")
        .unwrap_or_else(|| panic!("a UA rule: {matched}"));
    assert!(ua["rule"].get("styleSheetId").is_none(), "{ua}");

    assert_eq!(
        error(page.call(
            None,
            "CSS.getStyleSheetText",
            json!({ "styleSheetId": "style-sheet-0-0" })
        )),
        "No style sheet with given id found"
    );
}

/// A sheet that goes is told gone, one that comes told come: a `<style>`'s
/// whose text changes both, as a new sheet; a `<link>` removed gone; a
/// sheet adopted come; a shadow root's and a frame's too. A session with `CSS` off is told
/// nothing.
#[test]
fn sheets_coming_and_going_are_told() {
    let (_machopo, mut page, _document, _server) = css_site(&[
        ("/", Route::ok("text/html", SHEETS)),
        ("/style.css", Route::ok("text/css", "p { padding: 2px }")),
    ]);
    let headers = added(&mut page);
    let id = |at: usize| headers[at]["styleSheetId"].clone();

    evaluate(&mut page, "document.querySelector('style').textContent = 'p { color: blue }'", None);
    let removed = page.event("CSS.styleSheetRemoved");
    assert_eq!(removed["params"]["styleSheetId"], id(1));
    let come = page.event("CSS.styleSheetAdded")["params"]["header"].clone();
    assert_ne!(come["styleSheetId"], id(1));
    assert_eq!(come["isInline"], true);
    assert_eq!(sheet_text(&mut page, &come["styleSheetId"]), "p { color: blue }");

    evaluate(&mut page, "document.querySelector('link').remove()", None);
    assert_eq!(
        page.event("CSS.styleSheetRemoved")["params"]["styleSheetId"],
        id(0)
    );
    assert_eq!(
        error(page.call(None, "CSS.getStyleSheetText", json!({ "styleSheetId": id(0) }))),
        "No style sheet with given id found"
    );

    evaluate(
        &mut page,
        "const more = new CSSStyleSheet(); more.replaceSync('b { color: red }'); \
         document.adoptedStyleSheets = [...document.adoptedStyleSheets, more]",
        None,
    );
    let adopted = page.event("CSS.styleSheetAdded")["params"]["header"].clone();
    assert_eq!(adopted["isConstructed"], true);
    assert_eq!(sheet_text(&mut page, &adopted["styleSheetId"]), "b { color: red; }\n");
    two_frames(&mut page);
    assert!(page.take_events("CSS.styleSheet").is_empty());

    // A shadow root's.
    evaluate(
        &mut page,
        "host.shadowRoot.querySelector('style').textContent = 'span {}'",
        None,
    );
    assert_eq!(
        page.event("CSS.styleSheetRemoved")["params"]["styleSheetId"],
        id(3)
    );
    let shadow = page.event("CSS.styleSheetAdded")["params"]["header"].clone();
    assert_eq!(sheet_text(&mut page, &shadow["styleSheetId"]), "span {}");

    // Off: nothing told.
    result(page.call(None, "CSS.disable", json!({})));
    evaluate(&mut page, "document.querySelector('style').remove()", None);
    two_frames(&mut page);
    assert!(page.take_events("CSS.styleSheet").is_empty());
    // On again: what there is now.
    result(page.call(None, "CSS.enable", json!({})));
    assert_eq!(added(&mut page).len(), 3);

    // A frame's: its sheet come with it, gone and come as its text changes.
    evaluate(
        &mut page,
        "const frame = document.createElement('iframe'); \
         frame.srcdoc = '<style>i { color: red }</style>'; document.body.append(frame)",
        None,
    );
    let framed = page.event("CSS.styleSheetAdded")["params"]["header"].clone();
    assert_ne!(framed["frameId"], headers[0]["frameId"]);
    evaluate(
        &mut page,
        "document.querySelector('iframe').contentDocument.querySelector('style').textContent = 'i {}'",
        None,
    );
    assert_eq!(
        page.event("CSS.styleSheetRemoved")["params"]["styleSheetId"],
        framed["styleSheetId"]
    );
    let again = page.event("CSS.styleSheetAdded")["params"]["header"].clone();
    assert_eq!(sheet_text(&mut page, &again["styleSheetId"]), "i {}");
}

/// A `<link>`'s sheet loaded before DevTools came is fetched again as its
/// text is first asked for, as its `<link>` fetched it (with CORS for one
/// with `crossorigin`): the text as it came now if it has the sheet's
/// rules, else the rules as CSSOM writes them; once, however many ask.
#[test]
fn link_sheets_loaded_before_devtools_are_fetched_again() {
    let a = "/* a */\np { padding: 1px }\n";
    let other = server(&[(
        "/c.css",
        Route::ok("text/css", "/* c */ p { color: blue }")
            .with_header("Access-Control-Allow-Origin", "*"),
    )]);
    let html = format!(
        "<!DOCTYPE html><link rel=stylesheet href=/a.css><link rel=stylesheet href=/b.css>\
         <link rel=stylesheet crossorigin href={}>\
         <p>p</p><script>onload = () => {{\
           document.styleSheets[1].insertRule('p {{ margin: 0 }}');\
           fetch('/loaded');\
         }};</script>",
        other.url("/c.css")
    );
    let server = server(&[
        ("/", Route::ok("text/html", html)),
        ("/a.css", Route::ok("text/css", a)),
        ("/b.css", Route::ok("text/css", "/* b */ p { color: red }")),
        ("/loaded", Route::ok("text/plain", "")),
    ]);
    let machopo = Machopo::start();
    // A tab of its own, which no session has as it loads.
    let (status, body) = machopo.http("PUT", &format!("/json/new?{}", server.url("/")));
    assert_eq!(status, 200, "{body}");
    let requests = |target: &str| {
        server
            .requests()
            .iter()
            .filter(|request| request.target == target)
            .count()
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while requests("/loaded") == 0 {
        assert!(std::time::Instant::now() < deadline, "the page loads");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let opened: Value = serde_json::from_str(&body).unwrap();
    let url = format!(
        "ws://127.0.0.1:{}/devtools/page/{}",
        machopo.port,
        opened["id"].as_str().unwrap()
    );
    let mut page = machopo.connect(&url, None).expect("connects");
    result(page.call(None, "DOM.enable", json!({})));
    result(page.call(None, "CSS.enable", json!({})));
    let headers = added(&mut page);
    let id = |url: &str| {
        headers
            .iter()
            .find(|header| header["sourceURL"] == url)
            .unwrap_or_else(|| panic!("{url}: {headers:?}"))["styleSheetId"]
            .clone()
    };
    let (a_id, b_id) = (id(&server.url("/a.css")), id(&server.url("/b.css")));
    assert_eq!(requests("/a.css"), 1);
    // Asked twice before it comes: both answered, of one fetch.
    let asked = [a_id.clone(), a_id.clone()].map(|id| {
        page.send(None, "CSS.getStyleSheetText", json!({ "styleSheetId": id }))
    });
    for asked in asked {
        assert_eq!(result(page.answer(asked))["text"], a);
    }
    assert_eq!(requests("/a.css"), 2);
    // Its rules changed since: they.
    let b = "p { margin: 0px; }\np { color: red; }\n";
    assert_eq!(sheet_text(&mut page, &b_id), b);
    assert_eq!(requests("/b.css"), 2);
    // Asked again: kept.
    assert_eq!(sheet_text(&mut page, &a_id), a);
    assert_eq!(sheet_text(&mut page, &b_id), b);
    assert_eq!((requests("/a.css"), requests("/b.css")), (2, 2));
    // With CORS, as its `<link>`'s: the page's origin told.
    assert_eq!(
        sheet_text(&mut page, &id(&other.url("/c.css"))),
        "/* c */ p { color: blue }"
    );
    let origin = server.url("");
    let origins: Vec<Option<String>> = other
        .requests()
        .iter()
        .map(|request| request.header("origin").map(str::to_owned))
        .collect();
    assert_eq!(origins, [Some(origin.clone()), Some(origin)]);
}
