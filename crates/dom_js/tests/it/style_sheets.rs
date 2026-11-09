//! Style sheets from script: `new CSSStyleSheet(options)`, its members
//! (the legacy ones too), `replaceSync`, `replace`'s promise, `disabled`;
//! `cssRules` and its rules (the same objects, keeping their expandos
//! through GC; `CSSStyleRule` straight under `CSSRule`: machopo has no
//! nesting), the errors as `DOMException`s, a rule's `style` with
//! `[PutForwards=cssText]`, group rules and `MediaList`s, `@keyframes`;
//! `adoptedStyleSheets`, an observable array (an array, the same one each
//! time, changed in place through `push`, an index, `length`, `splice`,
//! assigned a sequence), taking only the document's own constructed
//! sheets; a `<style>`'s and a `<link>`'s `sheet` and a tree scope's
//! `styleSheets`, as Chrome has them.

use std::time::Instant;

use dom_api::{ClassicScript, EngineSettings, ScriptEngine};
use dom_js::V8Engine;

use crate::env::Env;

fn page() -> Env {
    let mut env = Env::window("about:blank");
    env.eval(
        "var html = document.documentElement || document.appendChild(document.createElement('html')); \
         var host = html.appendChild(document.createElement('div')); \
         var root = host.attachShadow({ mode: 'open' }); \
         var a = new CSSStyleSheet(); var b = new CSSStyleSheet({ disabled: true, media: 'print' }); 0",
    );
    env
}

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
    }
}

#[test]
fn a_constructed_sheet_replaces_its_rules() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "[a instanceof CSSStyleSheet, a instanceof StyleSheet, a.disabled, b.disabled, \
                  Object.getPrototypeOf(CSSStyleSheet.prototype) === StyleSheet.prototype].join()",
                "true,true,false,true,true",
            ),
            (
                "a.disabled = true; b.disabled = false; [a.disabled, b.disabled].join()",
                "true,false",
            ),
            ("String(a.replaceSync('p { color: red }'))", "undefined"),
            (
                "var replaced; var promise = a.replace('i {}'); promise.then(s => { replaced = s === a; }); \
                 promise instanceof Promise",
                "true",
            ),
        ],
    );
    env.checkpoint();
    check(&mut env, &[("String(replaced)", "true")]);
    // `this` of another interface.
    check(
        &mut env,
        &[(
            "try { CSSStyleSheet.prototype.replaceSync.call(document, ''); 'no' } catch (e) { e.name }",
            "TypeError",
        )],
    );
}

#[test]
fn adopted_style_sheets_is_an_observable_array() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "var list = document.adoptedStyleSheets; \
                 [Array.isArray(list), list === document.adoptedStyleSheets, list.length].join()",
                "true,true,0",
            ),
            (
                "list.push(a, b); [list.length, list[0] === a, list[1] === b].join()",
                "2,true,true",
            ),
            ("list[0] = b; [list.length, list[0] === b].join()", "2,true"),
            ("list.length = 1; [list.length, list[1]].join()", "1,"),
            ("list.pop() === b && list.length", "0"),
            (
                "document.adoptedStyleSheets = [a, b, a]; [list.length, list[2] === a].join()",
                "3,true",
            ),
            (
                "document.adoptedStyleSheets = [b]; [list.length, list[0] === b].join()",
                "1,true",
            ),
            (
                "[Reflect.set(list, 'length', 5), Reflect.set(list, 2, a), list.length, \
                  Reflect.preventExtensions(list)].join()",
                "false,false,1,false",
            ),
            ("document.adoptedStyleSheets = [a, b, a]; list.length", "3"),
            (
                "list.splice(1, 1); [list.length, list[0] === a, list[1] === a].join()",
                "2,true,true",
            ),
            (
                "[Object.keys(list).join(), 'length' in list, 2 in list].join()",
                "0,1,true,false",
            ),
            ("list.length = 0; list.length", "0"),
            // Each tree scope its own.
            (
                "root.adoptedStyleSheets.push(b); [root.adoptedStyleSheets.length, list.length].join()",
                "1,0",
            ),
            // Its backing list's operations aren't members.
            (
                "['setAdoptedStyleSheet' in document, 'deleteAdoptedStyleSheet' in root].join()",
                "false,false",
            ),
        ],
    );
}

/// A shadow root's adopted sheets go when its host moves to another
/// document (the same array, emptied), but not into or out of a
/// `<template>`'s contents (its document's template document), nested
/// ones too.
#[test]
fn a_shadow_roots_adopted_sheets_and_adoption() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "root.adoptedStyleSheets = [a]; var list = root.adoptedStyleSheets; \
                 var t = html.appendChild(document.createElement('template')); \
                 var inner = t.content.appendChild(document.createElement('template')); \
                 t.content.append(host); var one = root.adoptedStyleSheets.length; \
                 html.append(host); var two = root.adoptedStyleSheets.length; \
                 inner.content.append(host); var three = root.adoptedStyleSheets.length; \
                 html.append(host); [one, two, three, root.adoptedStyleSheets.length, \
                 host.ownerDocument === document].join()",
                "1,1,1,1,true",
            ),
            (
                "var other = document.implementation.createHTMLDocument(''); other.body.append(host); \
                 [root.adoptedStyleSheets.length, list === root.adoptedStyleSheets].join()",
                "0,true",
            ),
        ],
    );
}

#[test]
fn adopted_style_sheets_takes_the_documents_own_sheets() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "try { document.adoptedStyleSheets.push(document); 'no' } catch (e) { e.name }",
                "TypeError",
            ),
            (
                "try { document.adoptedStyleSheets = [a, 1]; 'no' } catch (e) { e.name }",
                "TypeError",
            ),
            ("document.adoptedStyleSheets.length", "0"),
            (
                "var other = document.implementation.createHTMLDocument(''); \
                 try { other.adoptedStyleSheets.push(a); 'no' } catch (e) { e.name }",
                "NotAllowedError",
            ),
            (
                "try { Object.preventExtensions(document.adoptedStyleSheets); 'no' } catch (e) { e.name }",
                "TypeError",
            ),
        ],
    );
}

/// A window at `https://example.test/` whose document has `<html>`, as the
/// renderer makes one; `report(value)` gives what a test reads.
struct Page {
    document: dom::NodeRoot,
    engine: V8Engine,
}

impl Page {
    fn new() -> Self {
        let mut engine = V8Engine::new(EngineSettings { web_tests: true });
        let document = engine
            .create_window(engine.top_frame(), "https://example.test/", Instant::now())
            .expect("a window");
        let mut page = Self { document, engine };
        page.eval("document.appendChild(document.createElement('html')); 0");
        page
    }

    /// `source`'s completion value as a string (a global script's: its
    /// `var`s stay), or the name of what it throws, `!` first; then a
    /// microtask checkpoint.
    fn eval(&mut self, source: &str) -> String {
        let source = format!(
            "testRunner.reportHarnessResults((() => {{ try {{ return String((0, eval)({source:?})); }} \
             catch (e) {{ return '!' + e.name; }} }})())"
        );
        self.engine.run_classic_script(
            self.engine.top_frame(),
            &ClassicScript {
                source: &source,
                url: "https://example.test/",
                base_url: None,
                line: 1,
                muted: false,
                use_credentials: false,
            },
        );
        self.engine.perform_microtask_checkpoint();
        self.engine.take_harness_results().expect("reported")
    }

    fn check(&mut self, cases: &[(&str, &str)]) {
        for (script, expected) in cases {
            assert_eq!(self.eval(script), *expected, "{script}");
        }
    }

    /// `css` as the loaded sheet of the `<link>` whose id is `id`, from
    /// `href`, origin-clean or not.
    fn give_link_sheet(&mut self, id: &str, css: &str, href: &str, origin_clean: bool) {
        let document = &self.document;
        self.engine.dom_mut().mutate(|cx| {
            let link = cx.element_by_id(document.get(cx.gc), id).expect("the link");
            let contents = std::sync::Arc::new(css::StyleSheetContents::parse(css, None));
            cx.set_link_sheet(link, contents, href, origin_clean);
        });
    }
}

#[test]
fn a_constructed_sheets_members() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "var sheet = new CSSStyleSheet({ disabled: true, media: 'screen' }); \
                 [sheet.type, sheet.href, sheet.ownerNode, sheet.parentStyleSheet, sheet.title, \
                  sheet.ownerRule, sheet.disabled, sheet.cssRules.length, sheet.media.mediaText].join()",
                "text/css,,,,,,true,0,screen",
            ),
            // The legacy members, as Chrome's: `rules` is `cssRules`,
            // `addRule` returns -1.
            (
                "[sheet.rules === sheet.cssRules, sheet.addRule('.x', 'color: red'), \
                  sheet.cssRules[0].cssText, sheet.addRule('.y'), sheet.cssRules[1].cssText, \
                  sheet.addRule('.z', '', 0), sheet.cssRules[0].cssText, \
                  (sheet.removeRule(), sheet.cssRules[0].cssText), sheet.addRule()].join(' | ')",
                "true | -1 | .x { color: red; } | -1 | .y { } | -1 | .z { } | .x { color: red; } | -1",
            ),
            ("sheet.cssRules[2].cssText", "undefined { }"),
            (
                "[CSSStyleSheet.length, CSSStyleSheet.prototype.insertRule.length, String(sheet)].join()",
                "0,1,[object CSSStyleSheet]",
            ),
            (
                "try { new CSSStyleSheet({ baseURL: 'https://[' }); 'no' } catch (e) { e.name }",
                "NotAllowedError",
            ),
        ],
    );
}

#[test]
fn css_rules_and_their_rules() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "var sheet = new CSSStyleSheet(); \
                 sheet.replaceSync('.a { color: red } @font-face { font-family: x } .c {}'); \
                 var rules = sheet.cssRules; rules.mark = 'kept'; \
                 var a = rules[0]; a.mark = 'kept'; a.style.mark = 'kept'; \
                 [rules === sheet.cssRules, rules.length, rules.item(1).selectorText, \
                  rules[2], rules.item(2), String(rules[0])].join()",
                "true,2,.c,,,[object CSSStyleRule]",
            ),
            (
                "[a instanceof CSSStyleRule, a instanceof CSSRule, \
                  Object.getPrototypeOf(CSSStyleRule.prototype) === CSSRule.prototype, \
                  a instanceof CSSGroupingRule, 'insertRule' in a, 'cssRules' in a].join()",
                "true,true,true,false,false,false",
            ),
            (
                "[a.type, CSSRule.STYLE_RULE, a.STYLE_RULE, CSSRule.KEYFRAMES_RULE, \
                  a.parentStyleSheet === sheet, a.parentRule, a.style.parentRule === a, \
                  a.cssText, document.body ? 1 : 0].join()",
                "1,1,1,7,true,,true,.a { color: red; },0",
            ),
        ],
    );
    env.full_gc();
    env.full_gc();
    check(
        &mut env,
        &[(
            "[sheet.cssRules.mark, sheet.cssRules[0].mark, sheet.cssRules[0].style.mark, \
              sheet.cssRules[0] === a].join()",
            "kept,kept,kept,true",
        )],
    );
}

#[test]
fn editing_rules() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "var sheet = new CSSStyleSheet(); sheet.replaceSync('.a {} .c {}'); \
                 var rules = sheet.cssRules; var a = rules[0]; \
                 a.style = 'margin: 1px'; a.style.backgroundColor = 'blue'; \
                 a.selectorText = 'p > .x'; a.selectorText = '!'; a.cssText = 'ignored {}'; \
                 var index = sheet.insertRule('.new { width: 2px }', 1); \
                 [a.cssText, index, rules.length, rules[1].cssText, rules[2].selectorText, \
                  sheet.insertRule('.first {}'), rules[1] === a].join('|')",
                "p > .x { margin: 1px; background-color: blue; }|1|3|.new { width: 2px; }|.c|0|true",
            ),
            (
                "try { sheet.insertRule('.x {}', 9); 'no' } catch (e) { e.name }",
                "IndexSizeError",
            ),
            (
                "try { sheet.insertRule('@font-face { font-family: x }'); 'no' } catch (e) { e.name }",
                "SyntaxError",
            ),
            (
                "try { sheet.insertRule('.x {} .y {}'); 'no' } catch (e) { e.name }",
                "SyntaxError",
            ),
            (
                "try { sheet.deleteRule(9); 'no' } catch (e) { e.name }",
                "IndexSizeError",
            ),
            (
                "sheet.deleteRule(1); \
                 [rules.length, a.parentStyleSheet, a.selectorText, rules[1].selectorText].join()",
                "3,,p > .x,.new",
            ),
            // An edit of a rule taken out is its own.
            (
                "a.style.color = 'red'; [a.cssText, sheet.cssRules.length].join('|')",
                "p > .x { margin: 1px; background-color: blue; color: red; }|3",
            ),
        ],
    );
}

/// Group rules are `CSSGroupingRule`s and `CSSConditionRule`s with their
/// own `cssRules`, and their rules know them; media lists are live,
/// stringify as their text, and take it through the attribute that holds
/// them.
#[test]
fn group_rules_and_media_lists() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "var sheet = new CSSStyleSheet({ media: 'SCREEN' }); \
                 sheet.replaceSync('@media screen { .a {} } @supports (color: red) { .b {} }'); \
                 var media = sheet.cssRules[0]; var supports = sheet.cssRules[1]; \
                 [String(media), String(supports), media.type, supports.type, \
                  CSSRule.SUPPORTS_RULE, media.conditionText, supports.conditionText].join()",
                "[object CSSMediaRule],[object CSSSupportsRule],4,12,12,screen,(color: red)",
            ),
            (
                "[Object.getPrototypeOf(CSSMediaRule.prototype) === CSSConditionRule.prototype, \
                  Object.getPrototypeOf(CSSSupportsRule.prototype) === CSSConditionRule.prototype, \
                  Object.getPrototypeOf(CSSConditionRule.prototype) === CSSGroupingRule.prototype, \
                  Object.getPrototypeOf(CSSGroupingRule.prototype) === CSSRule.prototype, \
                  'media' in supports].join()",
                "true,true,true,true,false",
            ),
            (
                "var inner = media.cssRules[0]; media.insertRule('.c { color: red }', 1); \
                 inner.style.color = 'blue'; \
                 [media.cssRules === media.cssRules, media.cssRules.length, \
                  inner.parentRule === media, inner.parentStyleSheet === sheet, \
                  media.parentRule, JSON.stringify(sheet.cssRules[0].cssText), \
                  sheet.cssRules[0] === media, media.cssRules[0] === inner].join('|')",
                "true|2|true|true||\"@media screen {\\n  .a { color: blue; }\\n  .c { color: red; }\\n}\"|true|true",
            ),
            (
                "try { media.insertRule('.x {}', 9); 'no' } catch (e) { e.name }",
                "IndexSizeError",
            ),
            (
                "try { media.deleteRule(9); 'no' } catch (e) { e.name }",
                "IndexSizeError",
            ),
            (
                "var list = sheet.media; list.appendMedium('print'); var appended = String(list); \
                 sheet.media = 'tv, (color)'; \
                 [appended, list === sheet.media, String(list), list.length, list[1], list.item(5), \
                  list instanceof MediaList].join('|')",
                "screen, print|true|tv, (color)|2|(color)||true",
            ),
            (
                "try { sheet.media.deleteMedium('print'); 'no' } catch (e) { e.name }",
                "NotFoundError",
            ),
            (
                "sheet.media.deleteMedium('tv'); var deleted = sheet.media.mediaText; \
                 media.media = 'print'; sheet.media.mediaText = null; \
                 [deleted, media.conditionText, media.media.length, sheet.media.length, \
                  sheet.cssRules[0] === media].join()",
                "(color),print,1,0,true",
            ),
            // A rule in a group rule taken out: no sheet, nor are its own.
            (
                "media.deleteRule(0); [inner.parentRule === media, inner.parentStyleSheet].join()",
                "false,",
            ),
            (
                "var nested = new CSSStyleSheet(); nested.replaceSync('@media print { @supports (color: red) { .d {} } }'); \
                 var outer = nested.cssRules[0]; var deep = outer.cssRules[0].cssRules[0]; \
                 nested.deleteRule(0); [deep.parentStyleSheet, deep.parentRule.parentRule === outer].join()",
                ",true",
            ),
        ],
    );
}

#[test]
fn keyframes_rules() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "var sheet = new CSSStyleSheet(); \
                 sheet.insertRule('@keyframes cm-blink { 0% {} 50% { opacity: 0 } 100% {} }'); \
                 var keyframes = sheet.cssRules[0]; var half = keyframes.cssRules[1]; \
                 [String(keyframes), String(half), keyframes.type, half.type, keyframes.name, \
                  keyframes.cssRules.length, half.keyText, half.parentRule === keyframes, \
                  keyframes.cssRules === keyframes.cssRules].join()",
                "[object CSSKeyframesRule],[object CSSKeyframeRule],7,8,cm-blink,3,50%,true,true",
            ),
            (
                "[Object.getPrototypeOf(CSSKeyframesRule.prototype) === CSSRule.prototype, \
                  Object.getPrototypeOf(CSSKeyframeRule.prototype) === CSSRule.prototype, \
                  half.style === half.style, half.style.opacity, half.style.parentRule === half, \
                  'insertRule' in keyframes, 'appendRule' in keyframes].join()",
                "true,true,true,0,true,false,true",
            ),
            (
                "half.style.opacity = '1'; half.cssText",
                "50% { opacity: 1; }",
            ),
            (
                "half.style = 'color: red'; half.cssText",
                "50% { color: red; }",
            ),
            (
                "keyframes.name = 'b'; keyframes.appendRule('25% { top: 1px }'); \
                 keyframes.appendRule('nope'); keyframes.findRule('25%').keyText = '30%'; \
                 keyframes.deleteRule('100%'); \
                 [keyframes.length, keyframes[2].keyText, keyframes[3], keyframes.name, \
                  keyframes.findRule('50%') === half, keyframes.findRule('90%'), \
                  sheet.cssRules[0].cssRules[2] === keyframes[2]].join()",
                "3,30%,,b,true,,true",
            ),
            (
                "try { half.keyText = 'x'; 'no' } catch (e) { [e.name, half.keyText].join() }",
                "SyntaxError,50%",
            ),
        ],
    );
}

/// `replace` resolves with the sheet; the rules it took out keep theirs,
/// with no sheet; an element's sheet can't be replaced, nor adopted.
#[test]
fn replace_takes_the_rules_out() {
    let mut env = page();
    check(
        &mut env,
        &[(
            "var sheet = new CSSStyleSheet(); sheet.replaceSync('.a {}'); var old = sheet.cssRules[0]; \
             sheet.replaceSync('.d { margin: 0 }'); \
             [sheet.cssRules.length, sheet.cssRules[0].cssText, old.parentStyleSheet, old.cssText, \
              sheet.cssRules[0] === old].join()",
            "1,.d { margin: 0px; },,.a { },false",
        )],
    );
}

/// A `<style>`'s `sheet` and a tree scope's `styleSheets` (Chrome's):
/// `null` until connected; the same objects while the sheet is the
/// element's (`media` and `title` keep it, and an edit stays); the text
/// changing or the element going makes a new one, the old one with no
/// `ownerNode`; an element's sheet can't be replaced or adopted; adopted
/// sheets and a shadow tree's aren't the document's; an SVG `<style>` has
/// one, a `<style>` of another `type` none.
#[test]
fn element_sheets_and_style_sheets() {
    let mut page = Page::new();
    page.check(&[
        (
            "var s = document.createElement('style'); s.textContent = 'p { color: red } q {}'; s.sheet",
            "null",
        ),
        (
            "document.documentElement.append(s); [s.sheet instanceof CSSStyleSheet, s.sheet === s.sheet, \
             s.sheet.ownerNode === s, s.sheet.href, s.sheet.title, s.sheet.cssRules.length, s.sheet.type, \
             s.sheet.media.mediaText].map(String).join()",
            "true,true,true,null,null,2,text/css,",
        ),
        (
            "var l = document.styleSheets; [l instanceof StyleSheetList, l === document.styleSheets, l.length, \
             l[0] === s.sheet, l.item(0) === s.sheet, l.item(5), l[5], [...l].length].map(String).join()",
            "true,true,1,true,true,null,undefined,1",
        ),
        (
            "var sheet = s.sheet; s.media = 'print'; s.setAttribute('title', 't'); var titled = sheet.title; \
             s.setAttribute('title', ''); [s.sheet === sheet, sheet.media.mediaText, titled, String(sheet.title)].join()",
            "true,print,t,null",
        ),
        (
            "s.sheet.disabled = true; var d = s.disabled; s.disabled = false; [d, s.sheet.disabled].join()",
            "true,false",
        ),
        (
            "var sheet = s.sheet; sheet.insertRule('div {}', 0); var kept = s.sheet.cssRules.length; \
             s.append(' '); [kept, s.sheet === sheet, sheet.ownerNode, s.sheet.cssRules.length, \
             sheet.cssRules.length, s.sheet.media.mediaText].map(String).join()",
            "3,false,null,2,3,print",
        ),
        // A comment child's data changing: the element's children changed
        // steps, a new sheet; a comment outside it, none.
        (
            "var c = s.appendChild(new Comment()); var sheet = s.sheet; c.appendData('x'); var once = s.sheet; \
             var out = document.documentElement.appendChild(new Comment()); out.appendData('y'); out.remove(); \
             [sheet === once, once === s.sheet].join()",
            "false,true",
        ),
        (
            "var sheet = s.sheet; s.remove(); document.documentElement.append(s); \
             [s.sheet === sheet, sheet.ownerNode, s.sheet.ownerNode === s].map(String).join()",
            "false,null,true",
        ),
        (
            "var sheet = s.sheet; s.remove(); [s.sheet, sheet.ownerNode, document.styleSheets.length].map(String).join()",
            "null,null,0",
        ),
        ("document.documentElement.append(s); s.sheet.replaceSync('')", "!NotAllowedError"),
        // A rejected promise.
        (
            "var rejected; s.sheet.replace('').then(null, (e) => { rejected = e.name; }) instanceof Promise",
            "true",
        ),
        ("rejected", "NotAllowedError"),
        ("document.adoptedStyleSheets = [s.sheet]", "!NotAllowedError"),
        (
            "var c = new CSSStyleSheet(); document.adoptedStyleSheets = [c]; \
             [document.styleSheets.length, c.ownerNode, c.href, c.title].map(String).join()",
            "1,null,null,null",
        ),
        (
            "var host = document.createElement('div'); document.documentElement.append(host); \
             var root = host.attachShadow({mode: 'open'}); root.innerHTML = '<style>p {}</style>'; \
             [root.styleSheets.length, root.styleSheets[0].ownerNode === root.firstChild, \
              document.styleSheets.length, root.styleSheets === root.styleSheets].map(String).join()",
            "1,true,1,true",
        ),
        (
            "var svg = document.createElementNS('http://www.w3.org/2000/svg', 'style'); \
             document.documentElement.append(svg); var plain = document.createElement('style'); \
             plain.type = 'text/plain'; document.documentElement.append(plain); \
             [svg.sheet instanceof CSSStyleSheet, String(plain.sheet), document.styleSheets.length, \
              document.styleSheets[1] === svg.sheet].join()",
            "true,null,2,true",
        ),
    ]);
}

/// A `<link>`'s sheet once loaded: its `href` as asked for, the same
/// sheet while it is its, a new one of the same rules once it went and
/// came back; one from another origin without CORS isn't origin-clean;
/// `HTMLLinkElement`'s reflected attributes.
#[test]
fn link_sheets() {
    let mut page = Page::new();
    page.check(&[(
        "var link = document.createElement('link'); link.id = 'l'; link.rel = 'stylesheet'; \
         link.href = 'a.css'; document.documentElement.append(link); \
         [link.sheet, document.styleSheets.length, link instanceof HTMLLinkElement].map(String).join()",
        "null,0,true",
    )]);
    page.give_link_sheet(
        "l",
        "p { color: green }",
        "https://example.test/a.css",
        true,
    );
    page.check(&[
        (
            "var sheet = link.sheet; [sheet.href, sheet.ownerNode === link, sheet.cssRules[0].cssText, \
             document.styleSheets[0] === sheet, link.sheet === sheet].join()",
            "https://example.test/a.css,true,p { color: green; },true,true",
        ),
        (
            "sheet.insertRule('q {}'); link.remove(); document.documentElement.append(link); \
             [link.sheet === sheet, sheet.ownerNode, link.sheet.cssRules.length, link.sheet.href].map(String).join()",
            "false,null,1,https://example.test/a.css",
        ),
        (
            "var before = link.sheet; link.rel = 'icon'; var none = link.sheet; link.rel = 'stylesheet'; \
             [none, link.sheet === before, before.ownerNode].map(String).join()",
            "null,false,null",
        ),
        (
            "link.media = 'print'; link.sheet.media.mediaText",
            "print",
        ),
    ]);
    page.give_link_sheet("l", "p {}", "https://other.test/b.css", false);
    page.check(&[
        ("link.sheet.cssRules", "!SecurityError"),
        ("link.sheet.rules", "!SecurityError"),
        ("link.sheet.insertRule('p {}')", "!SecurityError"),
        ("link.sheet.deleteRule(0)", "!SecurityError"),
        ("link.sheet.addRule('p', '')", "!SecurityError"),
        ("link.sheet.href", "https://other.test/b.css"),
        (
            "link.crossOrigin = 'USE-credentials'; link.as = 'STYLE'; link.relList.add('preload'); \
             [link.crossOrigin, link.as, link.rel, link.href, link.disabled, link.integrity].join()",
            "use-credentials,style,stylesheet preload,https://example.test/a.css,false,",
        ),
        ("link.as = 'nope'; link.as", ""),
        (
            "[link.relList.supports('MODULEPRELOAD'), link.relList.supports('stylesheet'), \
              link.relList.supports('icon')].join()",
            "true,true,false",
        ),
    ]);
}
