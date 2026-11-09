//! Making nodes and documents: `createComment`,
//! `createDocumentFragment`, `createProcessingInstruction` (its checks),
//! `cloneNode`, `importNode` and `adoptNode` (theirs), and
//! `DOMImplementation`'s documents (no window: their events stop at
//! them) and doctypes; fragments through the tree's members.

use crate::env::Env;

fn window() -> Env {
    let mut env = Env::window("about:blank");
    env.eval(
        "var html = document.documentElement || document.appendChild(document.createElement('html')); \
         var names = (node) => Array.from(node.childNodes).map((c) => c.nodeType == 3 ? c.data : c.nodeName).join(); \
         var error = (f) => { try { f(); return 'ok' } catch (e) { return e.name } }; 0",
    );
    env
}

/// Each script's value, a microtask checkpoint after it (the observers'
/// records delivered).
fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
        env.checkpoint();
    }
}

#[test]
fn documents_make_comments_fragments_and_processing_instructions() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var c = document.createComment('x'); [c.nodeType, c.data, c instanceof Comment, c.ownerDocument === document].join()",
                "8,x,true,true",
            ),
            (
                "var f = document.createDocumentFragment(); [f.nodeType, f instanceof DocumentFragment, f.childNodes.length].join()",
                "11,true,0",
            ),
            (
                "var pi = document.createProcessingInstruction('xml-stylesheet', 'href=a'); [pi.nodeType, pi.target, pi.data].join()",
                "7,xml-stylesheet,href=a",
            ),
            (
                "['1a', 'a b', ''].map((t) => error(() => document.createProcessingInstruction(t, 'd'))).join() + ',' + \
                 error(() => document.createProcessingInstruction('a', 'x?>'))",
                "InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError",
            ),
        ],
    );
}

#[test]
fn clones_copy_names_attributes_data_and_with_deep_children() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var d = document.createElement('div'); d.id = 'x'; d.innerHTML = '<b>1</b>2'; \
                 var shallow = d.cloneNode(); var deep = d.cloneNode(true); \
                 [shallow.id, shallow.childNodes.length, deep.innerHTML, deep.ownerDocument === document, deep.parentNode, \
                  deep.isEqualNode(d), deep === d].join()",
                "x,0,<b>1</b>2,true,,true,false",
            ),
            (
                "var heard = 0; d.addEventListener('e', () => heard++); d.cloneNode(true).dispatchEvent(new Event('e')); heard",
                "0",
            ),
            (
                "var dc = document.cloneNode(true); [dc.nodeType, dc === document, dc.documentElement.nodeName, \
                  dc.documentElement.ownerDocument === dc].join()",
                "9,false,HTML,true",
            ),
            (
                "error(() => document.createElement('div').attachShadow({ mode: 'open' }).cloneNode())",
                "NotSupportedError",
            ),
        ],
    );
}

#[test]
fn implementations_make_documents_without_windows() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var h = document.implementation.createHTMLDocument('T'); \
                 [h.nodeType, h instanceof Document, names(h), h.documentElement.innerHTML, \
                  h.implementation === h.implementation, h.implementation === document.implementation, \
                  document.implementation instanceof DOMImplementation, document.implementation.hasFeature()].join('|')",
                "9|true|html,HTML|<head><title>T</title></head><body></body>|true|false|true|true",
            ),
            (
                "document.implementation.createHTMLDocument().documentElement.innerHTML",
                "<head></head><body></body>",
            ),
            (
                "var log = []; h.addEventListener('e', () => log.push('document')); window.addEventListener('e', () => log.push('window')); \
                 h.documentElement.dispatchEvent(new Event('e', { bubbles: true })); [log.join(), h.documentElement.isConnected].join('|')",
                "document|true",
            ),
            // Between documents.
            (
                "var a2 = h.createElement('a'); h.documentElement.appendChild(a2); var adopted = document.adoptNode(a2); \
                 [adopted === a2, a2.ownerDocument === document, a2.parentNode].join()",
                "true,true,",
            ),
            (
                "var imported = document.importNode(h.documentElement, true); var flat = document.importNode(h.documentElement); \
                 [imported.ownerDocument === document, imported.innerHTML, h.documentElement.parentNode === h, flat.childNodes.length].join('|')",
                "true|<head><title>T</title></head><body></body>|true|0",
            ),
            (
                "var sr = document.createElement('div').attachShadow({ mode: 'open' }); \
                 [error(() => document.adoptNode(h)), error(() => document.adoptNode(sr)), error(() => document.importNode(h)), \
                  error(() => document.importNode(sr))].join()",
                "NotSupportedError,HierarchyRequestError,NotSupportedError,NotSupportedError",
            ),
            (
                "var dt = document.implementation.createDocumentType('svg', 'pub', 'sys'); \
                 [dt.nodeType, dt.name, dt.publicId, dt.systemId, dt.ownerDocument === document].join()",
                "10,svg,pub,sys,true",
            ),
            (
                "['a b', 'a>', 'a\\0'].map((n) => error(() => document.implementation.createDocumentType(n, '', ''))).join() + ',' + \
                 error(() => document.implementation.createDocumentType('', '', ''))",
                "InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,ok",
            ),
            (
                "var x = document.implementation.createDocument('http://www.w3.org/2000/svg', 'svg:svg', dt); \
                 [x.nodeType, names(x), x.documentElement.namespaceURI, x.documentElement.prefix, dt.ownerDocument === x, \
                  x.createElement('Ab').nodeName].join()",
                "9,svg,svg:svg,http://www.w3.org/2000/svg,svg,true,Ab",
            ),
            (
                "[document.implementation.createDocument(null, '').childNodes.length, \
                  document.implementation.createDocument(null, null).childNodes.length, \
                  document.implementation.createDocument(null, 'a').documentElement.nodeName, \
                  error(() => document.implementation.createDocument('', 'a:b'))].join()",
                "0,0,a,NamespaceError",
            ),
        ],
    );
}

/// The tree's members with fragments: their children go in, in order;
/// one node isn't put in a fragment first.
#[test]
fn fragments_go_in_as_their_children() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var q = document.createElement('div'); q.innerHTML = '<a></a><b></b>'; \
                 var frag = document.createDocumentFragment(); frag.append('1', '2'); \
                 q.replaceChild(frag, q.lastChild); [names(q), frag.childNodes.length].join('|')",
                "A,1,2|0",
            ),
            (
                "var two = document.createDocumentFragment(); two.append(document.createElement('a'), document.createElement('b')); \
                 [error(() => document.replaceChild(two, document.documentElement)), error(() => document.append(two)), \
                  two.childNodes.length].join()",
                "HierarchyRequestError,HierarchyRequestError,2",
            ),
            // A doctype prepended alone: no fragment (a fragment can't hold it).
            (
                "var d2 = document.implementation.createHTMLDocument(); d2.removeChild(d2.firstChild); \
                 d2.prepend(d2.implementation.createDocumentType('html', '', '')); [d2.firstChild.nodeType, names(d2)].join('|')",
                "10|html,HTML",
            ),
            (
                "var f2 = document.createDocumentFragment(); f2.append('x'); var t2 = document.createElement('p'); \
                 t2.replaceChildren(f2, 'y'); names(t2)",
                "x,y",
            ),
        ],
    );
}

/// HTML's body and head elements, DOM's doctype, and a template's
/// contents.
#[test]
fn a_documents_body_head_doctype_and_a_templates_contents() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var h = document.implementation.createHTMLDocument('t'); \
                 [h.body.nodeName, h.head.nodeName, h.doctype.name, h.doctype === h.firstChild, h.body instanceof HTMLElement].join()",
                "BODY,HEAD,html,true,true",
            ),
            // Its title a `DOMString`: null is "null"; undefined, none.
            (
                "[document.implementation.createHTMLDocument(null).title, \
                  document.implementation.createHTMLDocument(undefined).querySelector('title'), \
                  document.implementation.createHTMLDocument().querySelector('title')].join()",
                "null,,",
            ),
            // The `<html>`'s children only, the first body or frameset.
            (
                "var f = h.createElement('frameset'); h.documentElement.insertBefore(f, h.head); \
                 [h.body === f, h.head.nodeName].join()",
                "true,HEAD",
            ),
            (
                "h.documentElement.removeChild(f); var nested = h.body.appendChild(h.createElement('body')); \
                 [h.body === nested, h.body.parentNode === h.documentElement].join()",
                "false,true",
            ),
            // Set: in place of the body, or appended; else an error.
            (
                "var b2 = h.createElement('body'); var old = h.body; h.body = b2; \
                 [h.body === b2, old.parentNode, h.documentElement.lastChild === b2].join()",
                "true,,true",
            ),
            (
                "var records = 0; new MutationObserver((r) => records += r.length).observe(h.documentElement, { childList: true }); \
              h.body = h.body; h.body === b2",
                "true",
            ),
            ("records", "0"),
            (
                "h.documentElement.removeChild(b2); h.body = b2; [h.body === b2, h.documentElement.lastChild === b2].join()",
                "true,true",
            ),
            (
                "[error(() => { h.body = h.createElement('div') }), error(() => { h.body = null }), \
                  error(() => { var bare = document.implementation.createHTMLDocument(); bare.removeChild(bare.documentElement); \
                    bare.body = bare.createElement('body') })].join()",
                "HierarchyRequestError,HierarchyRequestError,HierarchyRequestError",
            ),
            // Not an HTML `<html>`: none.
            (
                "var x = document.implementation.createDocument('urn:x', 'html'); x.documentElement.appendChild(x.createElementNS('http://www.w3.org/1999/xhtml', 'body')); \
                 [x.body, x.head, x.doctype].join()",
                ",,",
            ),
            (
                "var t = document.createElement('template'); t.innerHTML = '<b>1</b><i>2</i>'; \
                 [t instanceof HTMLTemplateElement, t.content instanceof DocumentFragment, t.content === t.content, \
                  t.childNodes.length, t.content.childNodes.length, t.content.ownerDocument === document].join()",
                "true,true,true,0,2,false",
            ),
            (
                "var copy = document.importNode(t.content, true); document.documentElement.appendChild(copy); \
                 [copy.childNodes.length, document.documentElement.lastChild.nodeName].join()",
                "0,I",
            ),
            // Its contents' host is the template: its ancestors, across
            // templates, can't go into them; a copy of the contents has
            // none; a listener on the contents is kept.
            (
                "var outer = document.createElement('div'); var t2 = outer.appendChild(document.createElement('template')); \
                 var inner = t2.content.appendChild(document.createElement('template')); var name = (f) => { try { f(); return 'ok'; } catch (e) { return e.name; } }; \
                 var heard = 0; t2.content.addEventListener('x', () => heard++); t2.content.dispatchEvent(new Event('x')); \
                 [name(() => t2.content.appendChild(outer)), name(() => inner.content.appendChild(outer)), name(() => inner.content.appendChild(t2)), \
                  name(() => t2.content.cloneNode().appendChild(outer)), heard].join()",
                "HierarchyRequestError,HierarchyRequestError,HierarchyRequestError,ok,1",
            ),
        ],
    );
}
