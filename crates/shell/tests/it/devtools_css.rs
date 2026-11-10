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
use crate::devtools_overlay::{non_white, rgba, screen};
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

/// A `CSS.CSSStyle`'s declarations as parsed (those as written, with
/// ranges, left out), as `name: value` (`!` when important, `?` when
/// implicit).
fn declarations(style: &Value) -> Vec<String> {
    style["cssProperties"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|property| property.get("range").is_none())
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
    let selector = &margin["selectorList"]["selectors"][2];
    assert_eq!(
        (&selector["text"], &selector["specificity"]),
        (&json!("div.a"), &json!({ "a": 0, "b": 1, "c": 1 }))
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
        ["color: red !important!"]
    );
    assert_eq!(
        declarations(&matched["inlineStyle"]),
        [
            "border-top-width: medium",
            "border-top-style: solid",
            "border-top-color: currentcolor"
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
    let inline_style = &matched["inlineStyle"];
    assert_eq!(
        (&inline_style["cssProperties"], &inline_style["cssText"]),
        (&json!([]), &json!(""))
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
    let answer = result(page.call(None, "CSS.getStyleSheetText", json!({ "styleSheetId": id })));
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
    let fetched = server
        .requests()
        .iter()
        .filter(|request| request.target == "/style.css")
        .count();
    assert_eq!(fetched, 1);
    assert_eq!(
        place(from_link),
        [&json!(0), &json!(0), &json!(2), &json!(0), &json!(36)]
    );

    assert_eq!(from_style["sourceURL"], page_url);
    assert_eq!(from_style["ownerNode"], backend(&mut page, "style"));
    assert_eq!(
        (
            &from_style["isInline"],
            &from_style["title"],
            &from_style["isMutable"]
        ),
        (&json!(true), &json!("t"), &json!(false))
    );
    assert!(from_style.get("loadingFailed").is_none(), "{from_style}");
    assert_eq!(
        sheet_text(&mut page, &from_style["styleSheetId"]),
        "\np { color: red }\n"
    );
    // After `<style media=screen title=t>`, on the markup's third line.
    assert_eq!(
        place(from_style),
        [&json!(2), &json!(28), &json!(4), &json!(0), &json!(18)]
    );

    assert_eq!(
        (
            &adopted["sourceURL"],
            &adopted["isConstructed"],
            &adopted["isMutable"]
        ),
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
    let ids: Vec<&Value> = headers
        .iter()
        .map(|header| &header["styleSheetId"])
        .collect();
    assert!(
        ids.iter()
            .enumerate()
            .all(|(at, id)| !ids[..at].contains(id)),
        "{ids:?}"
    );
    assert!(
        headers
            .iter()
            .all(|header| header["frameId"] == headers[0]["frameId"]),
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
    assert_eq!(of("p", "padding"), from_link["styleSheetId"]);
    assert_eq!(of("p", "color"), from_style["styleSheetId"]);
    assert_eq!(of(".a", "margin"), adopted["styleSheetId"]);
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

    evaluate(
        &mut page,
        "document.querySelector('style').textContent = 'p { color: blue }'",
        None,
    );
    let removed = page.event("CSS.styleSheetRemoved");
    assert_eq!(removed["params"]["styleSheetId"], id(1));
    let come = page.event("CSS.styleSheetAdded")["params"]["header"].clone();
    assert_ne!(come["styleSheetId"], id(1));
    assert_eq!(come["isInline"], true);
    assert_eq!(
        sheet_text(&mut page, &come["styleSheetId"]),
        "p { color: blue }"
    );

    evaluate(&mut page, "document.querySelector('link').remove()", None);
    assert_eq!(
        page.event("CSS.styleSheetRemoved")["params"]["styleSheetId"],
        id(0)
    );
    assert_eq!(
        error(page.call(
            None,
            "CSS.getStyleSheetText",
            json!({ "styleSheetId": id(0) })
        )),
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
    assert_eq!(
        sheet_text(&mut page, &adopted["styleSheetId"]),
        "b { color: red; }\n"
    );
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
/// rules (not if nothing came), else the rules as CSSOM writes them; once,
/// however many ask. One script edited is its rules as CSSOM writes them,
/// not fetched.
#[test]
fn link_sheets_loaded_before_devtools_are_fetched_again() {
    let a = "/* a */\np { padding: 1px }\n";
    let other = server(&[(
        "/c.css",
        Route::ok("text/css", "/* c */ p { color: blue }")
            .with_header("Access-Control-Allow-Origin", "*"),
    )]);
    let gone = server(&[("/d.css", Route::ok("text/css", "/* d */ p { top: 0 }"))]);
    let html = format!(
        "<!DOCTYPE html><link rel=stylesheet href=/a.css><link rel=stylesheet href=/b.css>\
         <link rel=stylesheet crossorigin href={}><link rel=stylesheet href={}>\
         <p>p</p><script>onload = () => {{\
           document.styleSheets[1].insertRule('p {{ margin: 0 }}');\
           fetch('/loaded');\
         }};</script>",
        other.url("/c.css"),
        gone.url("/d.css")
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
    let d_url = gone.url("/d.css");
    drop(gone);
    let id = |url: &str| {
        headers
            .iter()
            .find(|header| header["sourceURL"] == url)
            .unwrap_or_else(|| panic!("{url}: {headers:?}"))["styleSheetId"]
            .clone()
    };
    let (a_id, b_id) = (id(&server.url("/a.css")), id(&server.url("/b.css")));
    assert_eq!(requests("/a.css"), 1);
    // Where `a.css`'s rule is: in its rules as CSSOM writes them, until its
    // text came again.
    let document = result(page.call(None, "DOM.getDocument", json!({})))["root"]["nodeId"].clone();
    let padding_at = |page: &mut Client| {
        let (rules, _) = author_rules(page, &document, "p");
        let rule = rules
            .iter()
            .find(|rule| rule["styleSheetId"] == a_id)
            .unwrap_or_else(|| panic!("{rules:?}"))
            .clone();
        range(&rule["selectorList"]["selectors"][0]["range"])
    };
    assert_eq!(padding_at(&mut page), (0, 0, 0, 1));
    // Asked twice before it comes: both answered, of one fetch.
    let asked = [a_id.clone(), a_id.clone()]
        .map(|id| page.send(None, "CSS.getStyleSheetText", json!({ "styleSheetId": id })));
    for asked in asked {
        assert_eq!(result(page.answer(asked))["text"], a);
    }
    assert_eq!(requests("/a.css"), 2);
    assert_eq!(padding_at(&mut page), (1, 0, 1, 1));
    // Script edited its rules since: they, not fetched.
    let b = "p { margin: 0px; }\np { color: red; }\n";
    assert_eq!(sheet_text(&mut page, &b_id), b);
    assert_eq!(requests("/b.css"), 1);
    // Asked again: kept.
    assert_eq!(sheet_text(&mut page, &a_id), a);
    assert_eq!(sheet_text(&mut page, &b_id), b);
    assert_eq!((requests("/a.css"), requests("/b.css")), (2, 1));
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
    // Nothing came: the rules.
    assert_eq!(sheet_text(&mut page, &id(&d_url)), "p { top: 0px; }\n");
}

/// A `CSS.SourceRange` as (start line, start column, end line, end column).
type SourceRange = (u64, u64, u64, u64);

fn range(value: &Value) -> SourceRange {
    let at = |key: &str| {
        value[key]
            .as_u64()
            .unwrap_or_else(|| panic!("{key}: {value}"))
    };
    (
        at("startLine"),
        at("startColumn"),
        at("endLine"),
        at("endColumn"),
    )
}

/// The author rules `selector` finds an element matching.
fn author_rules(page: &mut Client, document: &Value, selector: &str) -> (Vec<Value>, Value) {
    let element = node(page, document, selector);
    let matched = result(page.call(
        None,
        "CSS.getMatchedStylesForNode",
        json!({ "nodeId": element }),
    ));
    let rules = matched["matchedCSSRules"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|rule| rule["rule"]["origin"] == "regular")
        .map(|rule| rule["rule"].clone())
        .collect();
    (rules, matched["inlineStyle"].clone())
}

/// Where a rule is in its sheet's text (lines from 0, columns in UTF-16
/// code units): each selector (to its comma) and the selector list as
/// written; its block; each declaration as written, in order, with its
/// text, a shorthand's longhands, `!important`, one that doesn't parse, one
/// in a comment, then those parsed; the `@media` and `@supports` it is in,
/// the innermost first, the sheet's own media last.
#[test]
fn rules_have_their_places() {
    let (_machopo, mut page, document, _server) = css_page(
        "<!DOCTYPE html><style media=screen>\n/* é */ .a ,\n p > i.a { margin: 1px 2px; color: red !important; \
         foo: bar; /* gap: 1px; */ top: 0 }\n@media (min-width: 1px) {\n  @supports (display: grid) {\n    \
         @media (max-width: 9999px) { .a { left: 1px } }\n  }\n}\n</style><p><i class=a>i</i></p>",
    );
    let (rules, _) = author_rules(&mut page, &document, "i");
    assert_eq!(rules.len(), 2, "{rules:?}");
    // In cascade order: the nested one's selector is less specific.
    let [nested, first] = &rules[..] else {
        unreachable!()
    };
    let selectors = first["selectorList"]["selectors"].as_array().unwrap();
    let selectors: Vec<(&str, SourceRange)> = selectors
        .iter()
        .map(|selector| {
            (
                selector["text"].as_str().unwrap(),
                range(&selector["range"]),
            )
        })
        .collect();
    // `é` is one code unit; the first runs to its comma.
    assert_eq!(
        selectors,
        [(".a", (1, 8, 1, 11)), ("p > i.a", (2, 1, 2, 8))]
    );
    assert_eq!(first["selectorList"]["text"], ".a, p > i.a");
    let style = &first["style"];
    assert_eq!(range(&style["range"]), (2, 10, 2, 84));
    assert!(
        style["cssText"]
            .as_str()
            .unwrap()
            .starts_with(" margin: 1px 2px;")
    );
    let written: Vec<Value> = style["cssProperties"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|property| property.get("range").is_some())
        .map(|property| {
            let mut property = property.clone();
            let place = range(&property["range"]);
            property["range"] = json!([place.1, place.3]);
            property
        })
        .collect();
    let longhands = json!([
        { "name": "margin-top", "value": "1px" },
        { "name": "margin-right", "value": "2px" },
        { "name": "margin-bottom", "value": "1px" },
        { "name": "margin-left", "value": "2px" },
    ]);
    assert_eq!(
        written,
        [
            json!({ "name": "margin", "value": "1px 2px", "text": "margin: 1px 2px;", "range": [11, 27],
                    "implicit": false, "disabled": false, "longhandProperties": longhands }),
            json!({ "name": "color", "value": "red !important", "text": "color: red !important;",
                    "range": [28, 50], "implicit": false, "disabled": false, "important": true }),
            json!({ "name": "foo", "value": "bar", "text": "foo: bar;", "range": [51, 60],
                    "implicit": false, "disabled": false, "parsedOk": false }),
            json!({ "name": "gap", "value": "1px", "text": "/* gap: 1px; */", "range": [61, 76],
                    "disabled": true, "longhandProperties": [
                        { "name": "row-gap", "value": "1px" }, { "name": "column-gap", "value": "1px" },
                    ] }),
            json!({ "name": "top", "value": "0", "text": "top: 0 ", "range": [77, 84],
                    "implicit": false, "disabled": false }),
        ]
    );
    // Then as parsed.
    assert_eq!(
        declarations(style),
        [
            "margin-top: 1px",
            "margin-right: 2px",
            "margin-bottom: 1px",
            "margin-left: 2px",
            "top: 0px",
            "color: red !important!"
        ]
    );
    assert_eq!(first["ruleTypes"], json!(["MediaRule"]));
    assert_eq!(first["supports"], json!([]));
    let own = &first["media"];
    assert_eq!(
        (&own[0]["text"], &own[0]["source"], own[0].get("range")),
        (&json!("screen"), &json!("linkedSheet"), None)
    );

    assert_eq!(
        nested["ruleTypes"],
        json!(["MediaRule", "SupportsRule", "MediaRule", "MediaRule"])
    );
    let supports = &nested["supports"][0];
    assert_eq!(
        (
            &supports["text"],
            &supports["active"],
            range(&supports["range"])
        ),
        (&json!("(display: grid)"), &json!(true), (4, 12, 4, 27))
    );
    assert_eq!(supports["styleSheetId"], first["styleSheetId"]);
    let media = &nested["media"];
    assert_eq!(media[0]["text"], "(max-width: 9999px)");
    assert_eq!(
        (
            &media[1]["text"],
            &media[1]["source"],
            range(&media[1]["range"])
        ),
        (
            &json!("(min-width: 1px)"),
            &json!("mediaRule"),
            (3, 7, 3, 23)
        )
    );
    assert_eq!(media[2]["text"], "screen");
    assert_eq!(
        range(&nested["selectorList"]["selectors"][0]["range"]),
        (5, 33, 5, 35)
    );
}

/// An element's `style` attribute is a sheet of its own, not told: an id
/// that stays the element's, its text the attribute's, its declarations
/// where they are in it; with no attribute, empty. Once script changes it,
/// that is told.
#[test]
fn inline_styles_are_sheets_of_their_own() {
    let (_machopo, mut page, document, _server) = css_page(
        "<!DOCTYPE html><div id=a style='color: pink;  zz: 1'>a</div><div id=b>b</div>\
         <div id=c style='margin: 1px; background: var(--x)'>c</div>",
    );
    let (_, inline) = author_rules(&mut page, &document, "#a");
    let id = inline["styleSheetId"].clone();
    assert!(id.is_string(), "{inline}");
    assert_eq!(inline["cssText"], "color: pink;  zz: 1");
    assert_eq!(range(&inline["range"]), (0, 0, 0, 19));
    let written: Vec<(&str, &Value, SourceRange)> = inline["cssProperties"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|property| property.get("range").is_some())
        .map(|property| {
            (
                property["text"].as_str().unwrap(),
                &property["parsedOk"],
                range(&property["range"]),
            )
        })
        .collect();
    assert_eq!(
        written,
        [
            ("color: pink;", &Value::Null, (0, 0, 0, 12)),
            ("zz: 1", &json!(false), (0, 14, 0, 19))
        ]
    );
    assert_eq!(sheet_text(&mut page, &id), "color: pink;  zz: 1");
    let a = node(&mut page, &document, "#a");
    let again = result(page.call(None, "CSS.getInlineStylesForNode", json!({ "nodeId": a })));
    assert_eq!(again["inlineStyle"]["styleSheetId"], id);
    assert!(page.take_events("CSS.styleSheetAdded").is_empty());

    let (_, none) = author_rules(&mut page, &document, "#b");
    assert_ne!(none["styleSheetId"], id);
    assert_eq!(
        (&none["cssText"], &none["cssProperties"]),
        (&json!(""), &json!([]))
    );
    assert_eq!(sheet_text(&mut page, &none["styleSheetId"]), "");
    // A shorthand's longhands; none while a `var()` is in it.
    let (_, c) = author_rules(&mut page, &document, "#c");
    let longhands: Vec<(&Value, usize)> = c["cssProperties"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|property| property.get("range").is_some())
        .map(|property| {
            (
                &property["name"],
                property["longhandProperties"]
                    .as_array()
                    .map_or(0, Vec::len),
            )
        })
        .collect();
    assert_eq!(
        longhands,
        [(&json!("margin"), 4), (&json!("background"), 0)]
    );

    evaluate(&mut page, "a.style.color = 'blue'", None);
    let changed = page.event("CSS.styleSheetChanged");
    assert_eq!(changed["params"]["styleSheetId"], id);
    assert_eq!(sheet_text(&mut page, &id), "color: blue;");
    two_frames(&mut page);
    assert!(page.take_events("CSS.styleSheetChanged").is_empty());
}

/// A sheet script edits is told changed, and its text is its rules as
/// CSSOM writes them from then on (a `<link>`'s too), where its rules are
/// found; a `<style>` script filled with `insertRule` (CSS-in-JS's) has them
/// for its text.
#[test]
fn edited_sheets_are_their_rules() {
    let (_machopo, mut page, document, _server) = css_site(&[
        (
            "/",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><style>/* c */ p { color: red }</style><style id=js></style>\
                 <script>js.sheet.insertRule('p { top: 0 }')</script><p>p</p>\
                 <link rel=stylesheet href=/l.css>",
            ),
        ),
        ("/l.css", Route::ok("text/css", "/* l */ p { left: 1px }")),
    ]);
    let headers = added(&mut page);
    // A `<link>`'s as it came, until script edits it.
    let link = headers[2]["styleSheetId"].clone();
    assert_eq!(sheet_text(&mut page, &link), "/* l */ p { left: 1px }");
    evaluate(&mut page, "document.styleSheets[2].deleteRule(0)", None);
    assert_eq!(
        page.event("CSS.styleSheetChanged")["params"]["styleSheetId"],
        link
    );
    assert_eq!(sheet_text(&mut page, &link), "");
    assert_eq!(
        sheet_text(&mut page, &headers[1]["styleSheetId"]),
        "p { top: 0px; }\n"
    );
    assert_eq!(headers[1]["length"], 16);
    let id = headers[0]["styleSheetId"].clone();
    assert_eq!(sheet_text(&mut page, &id), "/* c */ p { color: red }");
    evaluate(
        &mut page,
        "document.styleSheets[0].insertRule('p { left: 0 }', 0)",
        None,
    );
    let changed = page.event("CSS.styleSheetChanged");
    assert_eq!(changed["params"]["styleSheetId"], id);
    assert!(page.take_events("CSS.styleSheetAdded").is_empty());
    assert_eq!(
        sheet_text(&mut page, &id),
        "p { left: 0px; }\np { color: red; }\n"
    );
    let (rules, _) = author_rules(&mut page, &document, "p");
    let color = rules
        .iter()
        .find(|rule| rule["styleSheetId"] == id && rule["style"]["cssText"] == " color: red; ")
        .unwrap_or_else(|| panic!("{rules:?}"));
    assert_eq!(
        range(&color["selectorList"]["selectors"][0]["range"]),
        (1, 0, 1, 1)
    );
}

/// The author rules of `matches` (`CSS.RuleMatch`es), by selector.
fn selectors(matches: &Value) -> Vec<&str> {
    matches
        .as_array()
        .unwrap()
        .iter()
        .filter(|matched| matched["rule"]["origin"] == "regular")
        .map(|matched| matched["rule"]["selectorList"]["text"].as_str().unwrap())
        .collect()
}

const PSEUDO: &str = "<!DOCTYPE html><style>\
    body { color: red } .p { padding: 1px } .p::selection { color: navy } .p::before { color: gray }\
    .x::before { content: 'b' } .x::after { content: 'a' } .x::marker { color: green }\
    .x::selection { color: pink } li::marker { color: blue }\
    </style><div class=p style='letter-spacing: 1px'><div class=x id=x>x</div><ul><li id=li>l</li></ul></div>";

/// An element's pseudo-elements' rules, those it can have (a marker a list
/// item's) with any; its ancestors' rules, the nearest first, with their
/// inline styles, and their highlight pseudo-elements' rules.
#[test]
fn pseudo_elements_and_ancestors_rules_are_listed() {
    let (_machopo, mut page, document, _server) = css_page(PSEUDO);
    let x = node(&mut page, &document, "#x");
    let matched = result(page.call(None, "CSS.getMatchedStylesForNode", json!({ "nodeId": x })));
    let pseudo: Vec<(&str, Vec<&str>)> = matched["pseudoElements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|of| {
            (
                of["pseudoType"].as_str().unwrap(),
                selectors(&of["matches"]),
            )
        })
        .collect();
    assert_eq!(
        pseudo,
        [
            ("before", vec![".x::before"]),
            ("after", vec![".x::after"]),
            ("selection", vec![".x::selection"])
        ]
    );
    let inherited: Vec<(Vec<&str>, &Value)> = matched["inherited"]
        .as_array()
        .unwrap()
        .iter()
        .map(|of| {
            (
                selectors(&of["matchedCSSRules"]),
                &of["inlineStyle"]["cssText"],
            )
        })
        .collect();
    assert_eq!(
        inherited,
        [
            (vec![".p"], &json!("letter-spacing: 1px")),
            (vec!["body"], &Value::Null),
            (vec![], &Value::Null)
        ]
    );
    let highlights: Vec<usize> = matched["inheritedPseudoElements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|of| of["pseudoElements"].as_array().unwrap().len())
        .collect();
    assert_eq!(highlights, [1, 0, 0]);
    assert_eq!(
        matched["inheritedPseudoElements"][0]["pseudoElements"][0]["pseudoType"],
        "selection"
    );
    // A list item has a marker.
    let li = node(&mut page, &document, "#li");
    let matched = result(page.call(None, "CSS.getMatchedStylesForNode", json!({ "nodeId": li })));
    let kinds: Vec<&Value> = matched["pseudoElements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|of| &of["pseudoType"])
        .collect();
    assert!(kinds.contains(&&json!("marker")), "{kinds:?}");
}

/// A pseudo-element node is styled as itself: its rules, its element's
/// first among those it inherits, no inline style, its element its layout
/// parent; its computed values, fonts, box and label; no inline styles to
/// ask for.
#[test]
fn a_pseudo_element_is_styled_as_itself() {
    let (_machopo, mut page, document, _server) = css_page(PSEUDO);
    let x = node(&mut page, &document, "#x");
    let described = result(page.call(None, "DOM.describeNode", json!({ "nodeId": x })));
    let before = described["node"]["pseudoElements"][0]["nodeId"].clone();
    let matched = result(page.call(
        None,
        "CSS.getMatchedStylesForNode",
        json!({ "nodeId": before }),
    ));
    assert_eq!(selectors(&matched["matchedCSSRules"]), [".x::before"]);
    assert_eq!(matched["pseudoElements"], json!([]));
    assert!(matched.get("inlineStyle").is_none(), "{matched}");
    // Its element (no author rule of its own), `.p`, `body`, `html`.
    assert_eq!(matched["inherited"].as_array().unwrap().len(), 4);
    assert_eq!(
        selectors(&matched["inherited"][1]["matchedCSSRules"]),
        [".p"]
    );
    assert_eq!(matched["parentLayoutNodeId"], x);
    let values = computed(&mut page, &before);
    assert_eq!(values["content"], "\"b\"");
    assert_eq!(computed(&mut page, &x)["content"], "normal");
    let fonts = result(page.call(
        None,
        "CSS.getPlatformFontsForNode",
        json!({ "nodeId": before }),
    ));
    assert_eq!(fonts["fonts"][0]["glyphCount"], 1, "{fonts}");
    assert_eq!(
        error(page.call(
            None,
            "CSS.getInlineStylesForNode",
            json!({ "nodeId": before })
        )),
        "Element is not a style sheet"
    );
    // Its box: narrower than its element's.
    let model = result(page.call(None, "DOM.getBoxModel", json!({ "nodeId": before })));
    let width = model["model"]["width"].as_f64().unwrap();
    let element = result(page.call(None, "DOM.getBoxModel", json!({ "nodeId": x })));
    assert!(
        width > 0.0 && width < element["model"]["width"].as_f64().unwrap(),
        "{model}"
    );
    let label = result(page.call(
        None,
        "Overlay.getHighlightObjectForTest",
        json!({ "nodeId": before }),
    ));
    let info = &label["highlight"]["elementInfo"];
    assert_eq!(
        (&info["tagName"], &info["className"]),
        (&json!("div"), &json!(".x::before"))
    );
    // Its boxes drawn: its border box where its box model's is.
    let border = &label["highlight"]["paths"].as_array().unwrap()[2];
    assert_eq!(border["name"], "border");
    assert_eq!(
        (&border["path"][1], &border["path"][2]),
        (&model["model"]["border"][0], &model["model"]["border"][1])
    );
    // Highlighted: its content box, not its element's.
    result(page.call(None, "Overlay.enable", json!({})));
    result(page.call(
        None,
        "Overlay.highlightNode",
        json!({ "nodeId": before, "highlightConfig": { "contentColor": rgba(255, 0, 0) } }),
    ));
    let at = |model: &Value, corner: usize| {
        let coordinate = |index: usize| {
            base::numerics::clamp_round::<u32>(model["model"]["content"][index].as_f64().unwrap())
        };
        (coordinate(corner * 2), coordinate(corner * 2 + 1))
    };
    let ((left, top), (right, bottom)) = (at(&model, 0), at(&model, 2));
    assert_eq!(
        screen(&mut page, left.midpoint(right), top.midpoint(bottom)),
        [255, 0, 0]
    );
    let (element_right, _) = at(&element, 2);
    assert_ne!(
        screen(
            &mut page,
            element_right.saturating_sub(5),
            top.midpoint(bottom)
        ),
        [255, 0, 0]
    );
}

/// An element's animations' values, each alone, by name, of the properties
/// each animates (those in effect); its transitions' together (empty with
/// none running); its ancestors' too. The `@keyframes` its animations run, with where their
/// names, keyframes and declarations are.
#[test]
fn animations_are_listed() {
    let (_machopo, mut page, document, _server) = css_page(
        "<!DOCTYPE html><style>\n\
         @keyframes fade { from { opacity: 0 } to { opacity: 1 } }\n\
         @keyframes move { to { margin-left: 100px } }\n\
         #x { animation: fade 100s linear -50s paused, move 100s linear -25s paused, fade 1s -2s;\n\
              transition: left 1000s linear; position: relative; left: 0 }\n\
         </style><div id=p><div id=x>x</div></div>",
    );
    let x = node(&mut page, &document, "#x");
    let animated = result(page.call(None, "CSS.getAnimatedStylesForNode", json!({ "nodeId": x })));
    let styles = |of: &Value| -> Vec<(Value, Vec<(String, String)>)> {
        of["animationStyles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                let properties = entry["style"]["cssProperties"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|property| {
                        (
                            property["name"].as_str().unwrap().to_owned(),
                            property["value"].as_str().unwrap().to_owned(),
                        )
                    })
                    .collect();
                (entry["name"].clone(), properties)
            })
            .collect()
    };
    let own = styles(&animated);
    assert_eq!(
        own[0],
        (
            json!("fade"),
            vec![("opacity".to_owned(), "0.5".to_owned())]
        )
    );
    assert_eq!(
        own[1],
        (
            json!("move"),
            vec![("margin-left".to_owned(), "25px".to_owned())]
        )
    );
    // The last over before it started: none.
    assert_eq!(own.len(), 2, "{own:?}");
    assert_eq!(animated["transitionsStyle"]["cssProperties"], json!([]));
    // Its ancestors': none.
    let inherited = animated["inherited"].as_array().unwrap();
    assert_eq!(inherited.len(), 3);
    assert!(
        inherited
            .iter()
            .all(|of| of["animationStyles"] == json!([]) && of.get("transitionsStyle").is_none())
    );
    // A transition running.
    evaluate(&mut page, "x.style.left = '1000px'", None);
    two_frames(&mut page);
    let animated = result(page.call(None, "CSS.getAnimatedStylesForNode", json!({ "nodeId": x })));
    let transitions = &animated["transitionsStyle"]["cssProperties"];
    assert_eq!(transitions[0]["name"], "left", "{animated}");

    // The `@keyframes`: each name once, where it is.
    let matched = result(page.call(None, "CSS.getMatchedStylesForNode", json!({ "nodeId": x })));
    let keyframes = matched["cssKeyframesRules"].as_array().unwrap();
    let names: Vec<&Value> = keyframes
        .iter()
        .map(|rule| &rule["animationName"]["text"])
        .collect();
    assert_eq!(names, [&json!("fade"), &json!("move")]);
    let fade = &keyframes[0];
    assert_eq!(range(&fade["animationName"]["range"]), (1, 11, 1, 15));
    let keys: Vec<(&Value, SourceRange)> = fade["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|keyframe| {
            (
                &keyframe["keyText"]["text"],
                range(&keyframe["keyText"]["range"]),
            )
        })
        .collect();
    assert_eq!(
        keys,
        [
            (&json!("0%"), (1, 18, 1, 22)),
            (&json!("100%"), (1, 38, 1, 40))
        ]
    );
    let style = &fade["keyframes"][0]["style"];
    assert_eq!(style["cssText"], " opacity: 0 ");
    assert_eq!(style["cssProperties"][0]["text"], "opacity: 0 ");
    assert_eq!(fade["keyframes"][0]["styleSheetId"], style["styleSheetId"]);
    assert!(fade["keyframes"][0]["styleSheetId"].is_string());
}

const FORCED: &str = "<!DOCTYPE html><style>\
     #x:hover { color: rgb(0, 128, 0) } .p:hover #x { background-color: red }\
     #x:focus { margin-left: 3px } #x:focus-within { margin-top: 3px }\
     .p:focus-within { margin-left: 4px } .p:has(#x:hover) { margin-right: 8px }\
     #x:target { padding-left: 5px } #x:active { padding-top: 1px }\
     #i:disabled { margin-left: 6px } #i:enabled { margin-top: 2px } #i:read-only { text-indent: 7px }\
     #r:required { margin-left: 9px } #r:optional { margin-top: 9px }\
     a:visited { color: rgb(255, 0, 0) }\
     #b { width: 40px; height: 40px } #b:hover { background: black }\
     </style><div class=p><div id=x tabindex=0>x</div><input id=i><input id=r><a id=l href=/>l</a></div>\
     <div id=b></div>";

/// `getComputedStyle(<selector>)`'s `properties`, as script reads them.
fn read_styles(page: &mut Client, selector: &str, properties: &[&str]) -> Vec<String> {
    let expression = format!(
        "(s => {properties:?}.map(p => s.getPropertyValue(p)))(getComputedStyle(document.querySelector('{selector}')))"
    );
    let values = evaluate(page, &expression, None);
    serde_json::from_value(values.clone()).unwrap_or_else(|_| panic!("{expression}: {values}"))
}

fn force(page: &mut Client, node: &Value, classes: &[&str]) -> Value {
    page.call(
        None,
        "CSS.forcePseudoState",
        json!({ "nodeId": node, "forcedPseudoClasses": classes }),
    )
}

/// `forcePseudoState`: the pseudo-classes forced match on the element
/// alone (not its ancestors' `:hover`), in its rules, for script and on
/// screen; a forced focus makes its ancestors match `:focus-within`, not
/// it; a forced form state its opposite not match, the others as they
/// are; `:visited` is taken and matches nothing; unknown names are passed
/// over; each call puts the element's in place of those before, and none
/// leaves them as the DOM is.
#[test]
fn forced_pseudo_classes_match() {
    let (_machopo, mut page, document, _server) = css_page(FORCED);
    let x = node(&mut page, &document, "#x");
    let style = [
        "color",
        "background-color",
        "margin-left",
        "margin-top",
        "padding-left",
        "padding-top",
    ];
    let parent = ["margin-left", "margin-right"];
    assert_eq!(
        read_styles(&mut page, "#x", &style),
        [
            "rgb(0, 0, 0)",
            "rgba(0, 0, 0, 0)",
            "0px",
            "0px",
            "0px",
            "0px"
        ]
    );
    assert_eq!(
        result(force(
            &mut page,
            &x,
            &["hover", "focus", "target", "nope", "link"]
        )),
        json!({})
    );
    assert_eq!(
        read_styles(&mut page, "#x", &style),
        [
            "rgb(0, 128, 0)",
            "rgba(0, 0, 0, 0)",
            "3px",
            "0px",
            "5px",
            "0px"
        ]
    );
    assert_eq!(read_styles(&mut page, ".p", &parent), ["4px", "8px"]);
    assert_eq!(
        evaluate(
            &mut page,
            "[x.matches(':hover'), x.matches(':focus-within'), document.querySelector('.p').matches(':focus-within, :hover')]",
            None
        ),
        json!([true, false, true])
    );
    let matched = result(page.call(None, "CSS.getMatchedStylesForNode", json!({ "nodeId": x })));
    let rules = selectors(&matched["matchedCSSRules"]);
    assert!(
        rules.contains(&"#x:hover") && rules.contains(&"#x:focus"),
        "{rules:?}"
    );
    // Computed too.
    assert_eq!(computed(&mut page, &x)["color"], "rgb(0, 128, 0)");

    // In place of those before.
    result(force(&mut page, &x, &["active"]));
    assert_eq!(
        read_styles(&mut page, "#x", &style),
        [
            "rgb(0, 0, 0)",
            "rgba(0, 0, 0, 0)",
            "0px",
            "0px",
            "0px",
            "1px"
        ]
    );
    assert_eq!(read_styles(&mut page, ".p", &parent), ["0px", "0px"]);
    // `:focus-within` forced: its ancestors' too.
    result(force(&mut page, &x, &["focus-within"]));
    assert_eq!(read_styles(&mut page, "#x", &["margin-top"]), ["3px"]);
    assert_eq!(read_styles(&mut page, ".p", &parent), ["4px", "0px"]);
    result(force(&mut page, &x, &[]));
    assert_eq!(read_styles(&mut page, ".p", &parent), ["0px", "0px"]);
    assert_eq!(read_styles(&mut page, "#x", &["padding-top"]), ["0px"]);

    // A form state and its opposite; the others as the DOM is.
    let form = ["margin-left", "margin-top", "text-indent"];
    let i = node(&mut page, &document, "#i");
    assert_eq!(read_styles(&mut page, "#i", &form), ["0px", "2px", "0px"]);
    result(force(&mut page, &i, &["disabled"]));
    assert_eq!(read_styles(&mut page, "#i", &form), ["6px", "0px", "0px"]);
    let r = node(&mut page, &document, "#r");
    assert_eq!(read_styles(&mut page, "#r", &form[..2]), ["0px", "9px"]);
    result(force(&mut page, &r, &["required"]));
    assert_eq!(read_styles(&mut page, "#r", &form[..2]), ["9px", "0px"]);
    result(force(&mut page, &r, &["optional"]));
    assert_eq!(read_styles(&mut page, "#r", &form[..2]), ["0px", "9px"]);

    // Taken, matching nothing.
    let l = node(&mut page, &document, "#l");
    let link = read_styles(&mut page, "#l", &["color"]);
    assert_eq!(result(force(&mut page, &l, &["visited"])), json!({}));
    assert_eq!(read_styles(&mut page, "#l", &["color"]), link);

    // Drawn.
    let b = node(&mut page, &document, "#b");
    assert_eq!(black(&mut page, (8, 30), (48, 120)), 0);
    result(force(&mut page, &b, &["hover"]));
    two_frames(&mut page);
    assert!(black(&mut page, (8, 30), (48, 120)) > 1000);

    // An element's only.
    assert_eq!(
        error(force(&mut page, &document, &["hover"])),
        "Node is not an Element"
    );
    let text = first_child(&mut page, &x);
    assert_eq!(
        error(force(&mut page, &text, &["hover"])),
        "Node is not an Element"
    );
    assert_eq!(
        page.call(
            None,
            "CSS.forcePseudoState",
            json!({ "nodeId": x, "forcedPseudoClasses": "hover" })
        )["error"]["code"],
        -32602
    );
}

/// What was forced goes once the element leaves its tree (put back, it
/// matches as the DOM is, as Chrome's), with the focus forced on it the
/// `:focus-within` above; once the session turns `CSS` or `DOM` off; and
/// with the session. Two sessions' are both forced, each its own.
#[test]
fn forced_pseudo_classes_go() {
    let (machopo, mut page, document, _server) = css_page(FORCED);
    let x = node(&mut page, &document, "#x");
    result(force(&mut page, &x, &["hover", "focus"]));
    assert_eq!(read_styles(&mut page, ".p", &["margin-left"]), ["4px"]);
    evaluate(&mut page, "document.body.append(x)", None);
    assert_eq!(read_styles(&mut page, "#x", &["color"]), ["rgb(0, 0, 0)"]);
    assert_eq!(read_styles(&mut page, ".p", &["margin-left"]), ["0px"]);
    // A subtree's.
    evaluate(&mut page, "document.querySelector('.p').append(x)", None);
    let x = node(&mut page, &document, "#x");
    result(force(&mut page, &x, &["hover"]));
    assert_eq!(read_styles(&mut page, "#x", &["color"]), ["rgb(0, 128, 0)"]);
    evaluate(
        &mut page,
        "(e => { document.querySelector('.p').remove(); document.body.prepend(e) })(x)",
        None,
    );
    assert_eq!(read_styles(&mut page, "#x", &["color"]), ["rgb(0, 0, 0)"]);

    let x = node(&mut page, &document, "#x");
    result(force(&mut page, &x, &["hover"]));
    result(page.call(None, "CSS.disable", json!({})));
    assert_eq!(read_styles(&mut page, "#x", &["color"]), ["rgb(0, 0, 0)"]);
    result(page.call(None, "CSS.enable", json!({})));
    result(force(&mut page, &x, &["hover"]));
    result(page.call(None, "DOM.disable", json!({})));
    assert_eq!(read_styles(&mut page, "#x", &["color"]), ["rgb(0, 0, 0)"]);

    // Two sessions.
    result(page.call(None, "DOM.enable", json!({})));
    let document = result(page.call(None, "DOM.getDocument", json!({})))["root"]["nodeId"].clone();
    let x = node(&mut page, &document, "#x");
    let (mut other, _) = crate::devtools::page(&machopo);
    result(other.call(None, "DOM.enable", json!({})));
    let other_document =
        result(other.call(None, "DOM.getDocument", json!({})))["root"]["nodeId"].clone();
    result(other.call(None, "CSS.enable", json!({})));
    let other_x = node(&mut other, &other_document, "#x");
    result(force(&mut page, &x, &["hover"]));
    result(force(&mut other, &other_x, &["active"]));
    assert_eq!(
        read_styles(&mut page, "#x", &["color", "padding-top"]),
        ["rgb(0, 128, 0)", "1px"]
    );
    // One's none: the other's stay.
    result(force(&mut other, &other_x, &[]));
    assert_eq!(
        read_styles(&mut page, "#x", &["color", "padding-top"]),
        ["rgb(0, 128, 0)", "0px"]
    );
    result(force(&mut other, &other_x, &["active"]));
    result(other.call(None, "CSS.disable", json!({})));
    assert_eq!(
        read_styles(&mut page, "#x", &["color", "padding-top"]),
        ["rgb(0, 128, 0)", "0px"]
    );
    // Gone with its session.
    result(force(&mut page, &x, &[]));
    result(other.call(None, "CSS.enable", json!({})));
    result(force(&mut other, &other_x, &["hover"]));
    assert_eq!(read_styles(&mut page, "#x", &["color"]), ["rgb(0, 128, 0)"]);
    drop(other);
    let start = std::time::Instant::now();
    while read_styles(&mut page, "#x", &["color"]) != ["rgb(0, 0, 0)"] {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "the session's hover stays"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// What a forced pseudo-class changes is restyled where no `:has()` makes
/// every change restyle the document: a rule that came while it was forced
/// (its state looked at all the same), `:required`'s, an ancestor's
/// `:focus-within` above the parent, as the focus is forced and as the
/// element forced it leaves the tree. `:visited` doesn't match restyled.
#[test]
fn forced_pseudo_classes_restyle_what_they_change() {
    let (_machopo, mut page, document, _server) = css_page(
        "<!DOCTYPE html><style>#r:required { margin-left: 9px } .p:focus-within { margin-left: 4px }\
         a:visited { color: rgb(255, 0, 0) } a:hover { padding-left: 1px }</style>\
         <div class=p><div class=c><div id=x tabindex=0>x</div></div></div><div id=y>y</div>\
         <input id=r><a id=l href=/>l</a>",
    );
    let y = node(&mut page, &document, "#y");
    result(force(&mut page, &y, &["hover"]));
    evaluate(
        &mut page,
        "document.head.append(Object.assign(document.createElement('style'), { textContent: '#y:hover { color: rgb(0, 128, 0) }' }))",
        None,
    );
    assert_eq!(read_styles(&mut page, "#y", &["color"]), ["rgb(0, 128, 0)"]);
    result(force(&mut page, &y, &[]));
    assert_eq!(read_styles(&mut page, "#y", &["color"]), ["rgb(0, 0, 0)"]);

    let r = node(&mut page, &document, "#r");
    result(force(&mut page, &r, &["required"]));
    assert_eq!(read_styles(&mut page, "#r", &["margin-left"]), ["9px"]);

    let x = node(&mut page, &document, "#x");
    result(force(&mut page, &x, &["focus"]));
    assert_eq!(read_styles(&mut page, ".p", &["margin-left"]), ["4px"]);
    evaluate(&mut page, "x.remove()", None);
    assert_eq!(read_styles(&mut page, ".p", &["margin-left"]), ["0px"]);

    let l = node(&mut page, &document, "#l");
    let link = read_styles(&mut page, "#l", &["color"]);
    result(force(&mut page, &l, &["visited", "hover"]));
    assert_eq!(read_styles(&mut page, "#l", &["padding-left"]), ["1px"]);
    assert_eq!(read_styles(&mut page, "#l", &["color"]), link);
}
