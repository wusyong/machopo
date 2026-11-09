//! The DOM's core members past the first ones: `CharacterData`'s edits by
//! UTF-16 offsets (a surrogate pair cut is U+FFFD), `Text`'s `splitText`
//! and `wholeText`, the node constructors (`Text`, `Comment`,
//! `DocumentFragment`, `Document`), `Element`'s `getAttributeNames`,
//! `toggleAttribute`, `insertAdjacentElement` / `Text`,
//! `getElementsByTagNameNS`; `Document`'s `getElementsByName`, `links`,
//! `anchors` (live), `characterSet`, `contentType`, `dir`, its visibility;
//! `baseURI`; a shadow root's `activeElement`.

use crate::env::Env;

fn window() -> Env {
    let mut env = Env::window("https://example.test/a/page.html");
    env.eval(
        "var html = document.appendChild(document.createElement('html')); \
         var body = html.appendChild(document.createElement('body')); \
         var error = (f) => { try { f(); return 'ok' } catch (e) { return e.name } }; 0",
    );
    env
}

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
    }
}

#[test]
fn character_data_is_edited_by_utf16_offsets() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var t = new Text('hello'); t.appendData(' world'); t.data",
                "hello world",
            ),
            ("t.insertData(5, ','); t.data", "hello, world"),
            ("t.deleteData(5, 100); t.data", "hello"),
            ("t.replaceData(1, 3, 'ipp'); t.data", "hippo"),
            (
                "[t.substringData(1, 2), t.substringData(3, 100), t.substringData(5, 1)].join()",
                "ip,po,",
            ),
            (
                "[error(() => t.substringData(6, 1)), error(() => t.insertData(6, 'x')), error(() => t.deleteData(9, 0))].join()",
                "IndexSizeError,IndexSizeError,IndexSizeError",
            ),
            // UTF-16 code units: a pair is two; one cut is U+FFFD.
            (
                "var e = new Comment('a😀b'); [e.length, e.substringData(1, 2), e.substringData(1, 1) === '\\ufffd'].join()",
                "4,😀,true",
            ),
            (
                "e.replaceData(2, 1, ''); [e.data === 'a\\ufffdb', e instanceof Comment].join()",
                "true,true",
            ),
            // A record of each edit, with the old value.
            (
                "var records = []; var mo = new MutationObserver(r => records.push(...r)); mo.observe(t, {characterData: true, characterDataOldValue: true}); \
              t.appendData('!'); mo.takeRecords().map(r => r.type + ':' + r.oldValue).join()",
                "characterData:hippo",
            ),
        ],
    );
}

#[test]
fn a_text_is_split_and_its_whole_text_joined() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var a = body.appendChild(new Text('abcdef')); var b = a.splitText(2); \
              [a.data, b.data, a.nextSibling === b, b.parentNode === body, b.ownerDocument === document].join()",
                "ab,cdef,true,true,true",
            ),
            (
                "body.appendChild(document.createComment('c')); body.appendChild(new Text('z')); \
              [a.wholeText, b.wholeText, body.lastChild.wholeText].join()",
                "abcdef,abcdef,z",
            ),
            (
                "var lone = new Text('xy'); var rest = lone.splitText(1); [lone.data, rest.data, rest.parentNode].join()",
                "x,y,",
            ),
            ("error(() => lone.splitText(5))", "IndexSizeError"),
        ],
    );
}

#[test]
fn nodes_are_constructed_in_the_realms_document() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "[new Text().data === '', new Text('a').ownerDocument === document, new Comment('c').nodeType].join()",
                "true,true,8",
            ),
            (
                "var f = new DocumentFragment(); [f.nodeType, f.ownerDocument === document].join()",
                "11,true",
            ),
            (
                "var d = new Document(); [d.nodeType, d.URL, d.contentType, d.documentElement, d.createElement('A').localName, d instanceof Document].join()",
                "9,about:blank,application/xml,,A,true",
            ),
        ],
    );
}

/// The qualified name `getAttribute` and the like are given is lowercased
/// for an HTML element in an HTML document only: an SVG element's, and an
/// HTML element's in an XML document, are as given.
#[test]
fn attribute_names_are_lowercased_for_html_elements_in_html_documents() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var el = body.appendChild(document.createElement('div')); el.setAttribute('DATA-A', '1'); \
              [el.getAttributeNames().join(), el.getAttribute('Data-A'), el.hasAttribute('DATA-a'), el.getAttribute('data-a')].join()",
                "data-a,1,true,1",
            ),
            (
                "el.removeAttribute('DATA-A'); el.hasAttribute('data-a')",
                "false",
            ),
            (
                "var svg = body.appendChild(document.createElementNS('http://www.w3.org/2000/svg', 'svg')); \
              svg.setAttribute('viewBox', '0 0 1 1'); [svg.getAttributeNames().join(), svg.getAttribute('viewbox'), svg.getAttribute('viewBox')].join()",
                "viewBox,,0 0 1 1",
            ),
            (
                "var x = document.implementation.createDocument(null, 'r', null).createElementNS('http://www.w3.org/1999/xhtml', 'div'); \
              x.setAttribute('Aa', '2'); [x.getAttributeNames().join(), x.getAttribute('aa'), x.getAttribute('Aa')].join()",
                "Aa,,2",
            ),
        ],
    );
}

#[test]
fn an_elements_attributes_are_listed_and_toggled() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var el = body.appendChild(document.createElement('div')); el.setAttribute('b', '1'); el.setAttributeNS('urn:x', 'p:a', '2'); \
              el.getAttributeNames().join()",
                "b,p:a",
            ),
            (
                "[el.toggleAttribute('HIDDEN'), el.hasAttribute('hidden'), el.getAttribute('hidden')].join()",
                "true,true,",
            ),
            (
                "[el.toggleAttribute('hidden'), el.hasAttribute('hidden')].join()",
                "false,false",
            ),
            (
                "[el.toggleAttribute('x', true), el.toggleAttribute('x', true), el.toggleAttribute('y', false), el.hasAttribute('y'), el.toggleAttribute('x', false)].join()",
                "true,true,false,false,false",
            ),
            (
                "error(() => el.toggleAttribute('a b'))",
                "InvalidCharacterError",
            ),
        ],
    );
}

#[test]
fn elements_and_texts_are_inserted_adjacent() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var t = body.appendChild(document.createElement('p')); \
              var n = (s) => document.createElement(s); \
              [t.insertAdjacentElement('beforeBegin', n('a')).localName, t.insertAdjacentElement('afterbegin', n('b')).localName, \
               t.insertAdjacentElement('beforeend', n('c')).localName, t.insertAdjacentElement('afterend', n('d')).localName].join()",
                "a,b,c,d",
            ),
            (
                "Array.from(body.childNodes, c => c.localName).join() + '/' + Array.from(t.childNodes, c => c.localName).join()",
                "a,p,d/b,c",
            ),
            (
                "t.insertAdjacentText('afterbegin', 'x'); t.firstChild.data",
                "x",
            ),
            (
                "var loose = n('i'); [loose.insertAdjacentElement('beforebegin', n('u')), error(() => loose.insertAdjacentText('nowhere', 'x'))].join()",
                ",SyntaxError",
            ),
        ],
    );
}

#[test]
fn documents_find_elements_by_namespace_and_name() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var svg = body.appendChild(document.createElementNS('http://www.w3.org/2000/svg', 'svg')); \
              var all = document.getElementsByTagNameNS('*', 'svg'); var html_ns = document.getElementsByTagNameNS('http://www.w3.org/1999/xhtml', '*'); \
              [all.length, all[0] === svg, html_ns.length, document.getElementsByTagNameNS(null, 'svg').length].join()",
                "1,true,2,0",
            ),
            (
                "var named = document.getElementsByName('n'); var i = body.appendChild(document.createElement('input')); i.name = 'n'; \
              [named.length, named[0] === i, named instanceof NodeList].join()",
                "1,true,true",
            ),
            ("i.name = 'm'; named.length", "0"),
            (
                "var links = document.links, anchors = document.anchors; var a = body.appendChild(document.createElement('a')); \
              [links.length, anchors.length].join()",
                "0,0",
            ),
            (
                "a.setAttribute('href', '#'); a.setAttribute('name', 'x'); body.appendChild(document.createElement('area')).setAttribute('href', '#'); \
              [links.length, anchors.length].join()",
                "2,1",
            ),
        ],
    );
}

#[test]
fn a_documents_metadata_and_visibility() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "[document.characterSet, document.charset, document.inputEncoding, document.contentType].join()",
                "UTF-8,UTF-8,UTF-8,text/html",
            ),
            (
                "[document.dir, (html.setAttribute('dir', 'RTL'), document.dir), (document.dir = 'ltr', html.getAttribute('dir'))].join()",
                ",rtl,ltr",
            ),
            ("html.setAttribute('dir', 'sideways'); document.dir", ""),
            (
                "[document.hidden, document.visibilityState, document.prerendering].join()",
                "false,visible,false",
            ),
            // One no window shows is hidden (HTML's visibility state starts
            // hidden; Blink's `IsPageVisible` is false without a frame).
            (
                "[new Document().hidden, document.implementation.createHTMLDocument('').visibilityState].join()",
                "true,hidden",
            ),
            (
                "[document.baseURI, body.baseURI, new Document().baseURI].join()",
                "https://example.test/a/page.html,https://example.test/a/page.html,about:blank",
            ),
            ("document.currentScript", "null"),
        ],
    );
}

/// With nothing focused, none (the focused case is `dom_api`'s, whose
/// tests lay out).
#[test]
fn a_shadow_roots_active_element_is_its_own() {
    let mut env = window();
    check(
        &mut env,
        &[(
            "var host = body.appendChild(document.createElement('div')); var root = host.attachShadow({mode: 'open'}); \
              var input = root.appendChild(document.createElement('input')); [root.activeElement, document.activeElement === body].join()",
            ",true",
        )],
    );
}

/// `Attr`s: the same one each time, its members, its value its element's
/// attribute's while it is one, kept once taken off; `attributes` (live,
/// by index and name); `setAttributeNode` replacing and giving back the
/// one before, another element's an `InUseAttributeError`;
/// `removeAttributeNode`; `createAttribute(NS)`; an `Attr` as a node.
#[test]
fn attributes_are_attr_nodes() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var el = body.appendChild(document.createElement('div')); el.setAttribute('a', '1'); \
              var a = el.getAttributeNode('A'); \
              [a === el.getAttributeNode('a'), a === el.attributes[0], a === el.attributes.a, a === el.attributes.getNamedItem('a'), \
               a.nodeType, a.nodeName, a.name, a.localName, a.value, a.ownerElement === el, a.specified, a instanceof Attr, a.parentNode].join()",
                "true,true,true,true,2,a,a,a,1,true,true,true,",
            ),
            (
                "a.value = '2'; [el.getAttribute('a'), a.nodeValue, a.textContent].join()",
                "2,2,2",
            ),
            (
                "el.setAttribute('a', '3'); [a.value, a.ownerElement === el].join()",
                "3,true",
            ),
            (
                "el.removeAttribute('a'); [a.value, a.ownerElement, el.hasAttribute('a'), el.attributes.length].join()",
                "3,,false,0",
            ),
            (
                "a.value = '4'; [a.value, el.hasAttribute('a')].join()",
                "4,false",
            ),
            // Put back: the same Attr.
            (
                "[el.setAttributeNode(a), el.getAttribute('a'), el.getAttributeNode('a') === a, a.ownerElement === el].join()",
                ",4,true,true",
            ),
            (
                "var b = document.createAttribute('A'); b.value = 'new'; var old = el.setAttributeNode(b); \
              [old === a, a.ownerElement, b.ownerElement === el, el.getAttribute('a'), a.value, el.attributes.length].join()",
                "true,,true,new,4,1",
            ),
            (
                "var other = document.createElement('p'); error(() => other.setAttributeNode(b))",
                "InUseAttributeError",
            ),
            (
                "[el.setAttributeNode(b) === b, error(() => other.removeAttributeNode(b))].join()",
                "true,NotFoundError",
            ),
            (
                "[el.removeAttributeNode(b) === b, el.attributes.length, b.ownerElement].join()",
                "true,0,",
            ),
            (
                "var n = document.createAttributeNS('urn:x', 'p:q'); [n.namespaceURI, n.prefix, n.localName, n.name, error(() => document.createAttribute('a b'))].join()",
                "urn:x,p,q,p:q,InvalidCharacterError",
            ),
            (
                "el.setAttributeNodeNS(n); [el.getAttributeNS('urn:x', 'q'), el.getAttributeNodeNS('urn:x', 'q') === n, el.attributes.getNamedItemNS('urn:x', 'q') === n].join()",
                ",true,true",
            ),
            (
                "var map = el.attributes; [map === el.attributes, map.length, map.item(0) === n, map.item(1), map instanceof NamedNodeMap].join()",
                "true,1,true,,true",
            ),
            (
                "[map.removeNamedItemNS('urn:x', 'q') === n, map.length, error(() => map.removeNamedItem('zz'))].join()",
                "true,0,NotFoundError",
            ),
            (
                "map.setNamedItem(a); [el.getAttribute('a'), map.length].join()",
                "4,1",
            ),
            // Put on another document's element: of that document; its
            // namespaces looked up from its element.
            (
                "var od = new Document(); var oe = od.createElement('o'); el.removeAttributeNode(a); oe.setAttributeNode(a); \
              [a.ownerDocument === od, a.lookupNamespaceURI('xml'), document.createAttribute('z').lookupNamespaceURI('xml')].join()",
                "true,http://www.w3.org/XML/1998/namespace,",
            ),
            // As a node: cloned, compared, in no tree.
            (
                "var c = a.cloneNode(); [c.value, c.ownerElement, c.isEqualNode(a), c === a, error(() => body.appendChild(a)), \
               error(() => body.replaceChild(a, body.firstChild))].join()",
                "4,,true,false,HierarchyRequestError,HierarchyRequestError",
            ),
            // An adopted element's Attrs go with it.
            (
                "var ad = new Document(); var held = body.appendChild(document.createElement('b')); held.setAttribute('k', 'v'); \
              var k = held.getAttributeNode('k'); ad.adoptNode(held); [k.ownerDocument === ad, k.ownerElement === held].join()",
                "true,true",
            ),
        ],
    );
}

/// A CDATA section: only in an XML document, without `]]>`; a text of
/// its own type and name (a `Text`, not one of `wholeText`'s or
/// `normalize()`'s exclusive texts), cloned and split as one;
/// `createDocument`'s document an `XMLDocument` of its namespace's content
/// type, cloned as one, an XHTML one's `createElement` HTML's.
#[test]
fn cdata_sections_and_xml_documents() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var x = document.implementation.createDocument(null, 'r', null); \
              [x instanceof XMLDocument, x.contentType, new Document() instanceof XMLDocument, document instanceof XMLDocument].join()",
                "true,application/xml,false,false",
            ),
            (
                "[document.implementation.createDocument('http://www.w3.org/1999/xhtml', 'html', null).contentType, \
               document.implementation.createDocument('http://www.w3.org/2000/svg', 'svg', null).contentType, \
               x.cloneNode() instanceof XMLDocument, x.cloneNode().contentType, \
               document.implementation.createDocument('http://www.w3.org/2000/svg', 'svg', null).cloneNode().contentType].join()",
                "application/xhtml+xml,image/svg+xml,true,application/xml,image/svg+xml",
            ),
            // `createElement` of an XHTML document: HTML's namespace, the
            // name as given; of another XML document's, none.
            (
                "var xh = document.implementation.createDocument('http://www.w3.org/1999/xhtml', 'html', null); \
                 var e = xh.createElement('Div'); [e.namespaceURI, e.localName, e instanceof HTMLElement, \
                 x.createElement('div').namespaceURI].join()",
                "http://www.w3.org/1999/xhtml,Div,true,",
            ),
            (
                "[error(() => document.createCDATASection('a')), error(() => x.createCDATASection('a]]>b'))].join()",
                "NotSupportedError,InvalidCharacterError",
            ),
            (
                "var c = x.createCDATASection('ab'); [c.nodeType, c.nodeName, c instanceof CDATASection, c instanceof Text, c.data].join()",
                "4,#cdata-section,true,true,ab",
            ),
            (
                "var r = x.documentElement; r.appendChild(x.createTextNode('t')); r.appendChild(c); r.appendChild(x.createTextNode('u')); \
              [r.firstChild.wholeText, c.wholeText, r.textContent].join()",
                "tabu,tabu,tabu",
            ),
            (
                "[c.cloneNode() instanceof CDATASection, c.isEqualNode(x.createTextNode('ab')), c.splitText(1) instanceof CDATASection].join()",
                "true,false,true",
            ),
            (
                "r.normalize(); Array.from(r.childNodes, n => n.nodeType + ':' + n.data).join()",
                "3:t,4:a,4:b,3:u",
            ),
        ],
    );
}

/// `DOMParser`: HTML parsed into a new document (no scripts run,
/// declarative shadow roots stay templates) at the realm's document's URL;
/// XML's types a `NotSupportedError`, another a `TypeError`.
#[test]
fn a_dom_parser_parses_html() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var p = new DOMParser(); var d = p.parseFromString('<p id=x>hi<script>window.ran = 1</script><div><template shadowrootmode=open>s</template></div>', 'text/html'); \
              [d instanceof Document, d.getElementById('x').firstChild.data, d.URL, d.contentType, typeof window.ran, \
               d.querySelector('div').shadowRoot, d.querySelector('template') !== null, d.defaultView].join()",
                "true,hi,https://example.test/a/page.html,text/html,undefined,,true,",
            ),
            (
                "[error(() => p.parseFromString('<a/>', 'text/xml')), error(() => p.parseFromString('', 'text/plain'))].join()",
                "NotSupportedError,TypeError",
            ),
        ],
    );
}
