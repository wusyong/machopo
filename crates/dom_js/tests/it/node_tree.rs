//! The tree's members of `Node`, `ParentNode` and `ChildNode`:
//! `childNodes` (live, its cache kept right as the children change),
//! `children`, `getElementsByClassName`, `append`, `prepend`, `before`,
//! `after`, `replaceWith`, `remove`, `replaceChildren`, `replaceChild` (its
//! checks and its record), `nodeValue`, `normalize`,
//! `compareDocumentPosition`, `isEqualNode`, `isSameNode`, the namespace
//! lookups and the element siblings.

use crate::env::Env;

/// A window with `names(node)`: its children, a text's data, another's
/// name.
fn window() -> Env {
    let mut env = Env::window("about:blank");
    env.eval(
        "var names = (node) => Array.from(node.childNodes).map((c) => c.nodeType == 3 ? c.data : c.nodeName).join(); 0",
    );
    env
}

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
    }
}

#[test]
fn child_nodes_is_live_and_walked_from_where_it_was() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var p = document.createElement('div'); var l = p.childNodes; \
                 [l === p.childNodes, l.length, l instanceof NodeList].join()",
                "true,0,true",
            ),
            (
                "for (var i = 0; i < 5; i++) p.appendChild(document.createElement('i')).id = 'c' + i; \
                 [l.length, l[0].id, l[4].id, l[2].id, l.item(1).id, l[5] === undefined, l.item(5)].join()",
                "5,c0,c4,c2,c1,true,",
            ),
            (
                "p.removeChild(l[2]); [l.length, l[2].id, l[3].id].join()",
                "4,c3,c4",
            ),
            // Past the end first, then the length.
            (
                "var fresh = document.createElement('p'); fresh.append('a', 'b', 'c'); var fl = fresh.childNodes; [fl[5], fl.length].join()",
                ",3",
            ),
            // A middle one first, then the length: counted on from it.
            (
                "var mid = document.createElement('p'); mid.append('a', 'b', 'c', 'd'); var ml = mid.childNodes; [ml[2].data, ml.length].join()",
                "c,4",
            ),
            (
                "p.insertBefore(document.createElement('b'), p.firstChild); [l.length, l[0].nodeName, l[1].id].join()",
                "5,B,c0",
            ),
            (
                "var ids = []; for (var i = l.length - 1; i >= 0; i--) ids.push(l[i].id || l[i].nodeName); ids.join()",
                "c4,c3,c1,c0,B",
            ),
            // Its length known, then a child more at the end.
            (
                "l.length; p.appendChild(document.createElement('u')); [l.length, l[5].nodeName].join()",
                "6,U",
            ),
            ("Array.from(l).length + ',' + [...l].length", "6,6"),
            (
                "p.textContent = 'x'; [l.length, l[0].nodeName, l[1]].join()",
                "1,#text,",
            ),
            ("p.innerHTML = '<a></a><b></b>'; names(p)", "A,B"),
        ],
    );
}

#[test]
fn children_and_classes_are_element_collections() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var d = document.createElement('div'); \
                 d.innerHTML = '<p class=\"a b\">1</p>text<span class=\"b\">2</span><p class=\"c\"><i class=\"a b\"></i></p>'; \
                 [d.children === d.children, d.children.length, d.children[1].nodeName, d.children instanceof HTMLCollection, \
                  d.getElementsByClassName('b').length, d.getElementsByClassName(' a  b ').length, \
                  d.getElementsByClassName('').length, d.getElementsByClassName('a z').length].join()",
                "true,3,SPAN,true,3,2,0,0",
            ),
            (
                "var byB = d.getElementsByClassName('b'); d.appendChild(document.createElement('b')).className = 'b'; \
                 [d.children.length, byB.length, d.lastElementChild.nodeName].join()",
                "4,4,B",
            ),
        ],
    );
}

#[test]
fn parent_and_child_node_insert_where_they_say() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var r = document.createElement('div'); var a = r.appendChild(document.createElement('a')); \
                 r.append('x', document.createElement('b'), 'y'); names(r)",
                "A,x,B,y",
            ),
            ("r.prepend('p'); names(r)", "p,A,x,B,y"),
            ("a.before('1', '2'); names(r)", "p,1,2,A,x,B,y"),
            // The first child: before it.
            ("r.firstChild.before('0'); names(r)", "0,p,1,2,A,x,B,y"),
            ("r.firstChild.remove(); names(r)", "p,1,2,A,x,B,y"),
            // After itself, past the nodes given.
            (
                "var b = a.nextSibling.nextSibling; a.after(b); names(r)",
                "p,1,2,A,B,x,y",
            ),
            ("a.after(a.nextSibling, 'z'); names(r)", "p,1,2,A,B,z,x,y"),
            (
                "a.replaceWith('r'); [names(r), a.parentNode].join('|')",
                "p,1,2,r,B,z,x,y|",
            ),
            ("r.lastChild.remove(); names(r)", "p,1,2,r,B,z,x"),
            ("b.replaceWith(b); names(r)", "p,1,2,r,B,z,x"),
            // Itself among the nodes: out of the parent, then where it was.
            ("b.replaceWith('q', b); names(r)", "p,1,2,r,q,B,z,x"),
            (
                "r.lastChild.replaceWith('w', r.lastChild); names(r)",
                "p,1,2,r,q,B,z,w,x",
            ),
            ("r.replaceChildren('only'); names(r)", "only"),
            ("r.replaceChildren(); r.childNodes.length", "0"),
            // Not in a tree: nothing.
            (
                "var lone = document.createElement('i'); lone.before('x'); lone.after('y'); lone.replaceWith('z'); lone.remove(); 'ok'",
                "ok",
            ),
            (
                "['document.append(\"t\")', 'r.append(r)', 'r.firstChild || r.append(document)', 'r.replaceChildren(r)'].map((s) => { \
                   try { eval(s); return 'ok' } catch (e) { return e.name } }).join()",
                "HierarchyRequestError,HierarchyRequestError,HierarchyRequestError,HierarchyRequestError",
            ),
            // A host into its own shadow tree.
            (
                "var h = document.createElement('div'); var sr = h.attachShadow({ mode: 'open' }); \
                 var deep = sr.appendChild(document.createElement('p')); \
                 var outer = document.createElement('section'); outer.appendChild(h); \
                 [() => sr.appendChild(h), () => deep.append(h), () => deep.replaceChildren(h), () => deep.append(outer)].map((f) => { \
                   try { f(); return 'ok' } catch (e) { return e.name } }).join()",
                "HierarchyRequestError,HierarchyRequestError,HierarchyRequestError,HierarchyRequestError",
            ),
            (
                "var s = document.createElement('div'); s.innerHTML = 'a<b></b>c<i></i>'; \
                 var t = s.firstChild; [t.previousElementSibling, t.nextElementSibling.nodeName, s.lastChild.previousElementSibling.nodeName, \
                  s.childNodes[2].nextElementSibling.nodeName, s.lastChild.nextElementSibling, s.childNodes[1].nextElementSibling.nodeName, \
                  s.lastChild.previousElementSibling.previousElementSibling].join()",
                ",B,B,I,,I,",
            ),
        ],
    );
}

#[test]
fn replace_child_checks_and_records_one_change() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var q = document.createElement('div'); q.innerHTML = '<a></a><b></b><i></i>'; \
                 var old = q.childNodes[1]; var ret = q.replaceChild(document.createElement('u'), old); \
                 [ret === old, names(q), old.parentNode].join('|')",
                "true|A,U,I|",
            ),
            ("q.replaceChild(q.firstChild, q.lastChild); names(q)", "U,A"),
            // Its next sibling, then its previous, in its place.
            (
                "q.append(document.createElement('b')); q.replaceChild(q.childNodes[1], q.firstChild); names(q)",
                "A,B",
            ),
            ("q.replaceChild(q.firstChild, q.lastChild); names(q)", "A"),
            ("q.append(document.createElement('u')); 0", "0"),
            (
                "q.replaceChild(q.firstChild, q.firstChild); names(q)",
                "A,U",
            ),
            (
                "[() => q.replaceChild(document.createElement('x'), document.createElement('y')), \
                  () => q.firstChild.appendChild(document.createElement('z')) && q.firstChild.replaceChild(q, q.firstChild.firstChild), \
                  () => document.replaceChild(document.createTextNode('t'), \
                        document.documentElement || document.appendChild(document.createElement('html')))].map((f) => { \
                   try { f(); return 'ok' } catch (e) { return e.name } }).join()",
                "NotFoundError,HierarchyRequestError,HierarchyRequestError",
            ),
            // The element a document has: replaced by one.
            (
                "var html = document.documentElement || document.appendChild(document.createElement('html')); \
                 document.replaceChild(document.createElement('html'), html); [document.documentElement === html, html.parentNode].join()",
                "false,",
            ),
        ],
    );
    // One record: what came, what went, where.
    env.eval(
        "var seen = []; new MutationObserver((records) => records.forEach((r) => seen.push(r.target.nodeName + ':' + \
           names({ childNodes: r.addedNodes }) + '/' + names({ childNodes: r.removedNodes }) + '/' + \
           (r.previousSibling && r.previousSibling.nodeName) + '/' + (r.nextSibling && (r.nextSibling.nodeName || r.nextSibling.data))))) \
           .observe(q, { childList: true }); \
         q.replaceChild(document.createElement('s'), q.firstChild); 0",
    );
    env.checkpoint();
    assert_eq!(env.eval("seen.join(' ')"), "DIV:S/A/null/U");
    // A node in its own place: its removal recorded, then its insertion.
    env.eval("seen = []; q.replaceChild(q.lastChild, q.lastChild); 0");
    env.checkpoint();
    assert_eq!(env.eval("seen.join(' ')"), "DIV:/U/S/null DIV:U//S/null");
    // Its previous sibling in its place: the record's previous is the one
    // before them.
    env.eval("seen = []; q.replaceChild(q.firstChild, q.lastChild); 0");
    env.checkpoint();
    assert_eq!(
        env.eval("seen.join(' ')"),
        "DIV:/S/null/U DIV:S/U/null/null"
    );
}

#[test]
fn node_value_and_normalize() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var t = document.createTextNode('a'); [t.nodeValue, document.createElement('p').nodeValue, document.nodeValue].join()",
                "a,,",
            ),
            ("t.nodeValue = null; [t.data, t.length].join()", ",0"),
            ("t.nodeValue = 'v'; t.data", "v"),
            (
                "var e = document.createElement('p'); e.nodeValue = 'x'; e.childNodes.length",
                "0",
            ),
            (
                "var n = document.createElement('p'); n.append('a', '', 'b'); n.appendChild(document.createElement('i')).append('c', 'd'); \
                 n.append('', 'e'); var first = n.firstChild; n.normalize(); \
                 [names(n), names(n.childNodes[1]), n.firstChild === first].join('|')",
                "ab,I,e|cd|true",
            ),
            (
                "var lone = document.createTextNode(''); lone.normalize(); lone.data.length",
                "0",
            ),
            // Of a text itself: nothing (only descendants).
            (
                "var m = document.createElement('p'); m.append('a', 'b'); m.firstChild.normalize(); names(m)",
                "a,b",
            ),
            // An empty one between elements: gone.
            (
                "var o = document.createElement('p'); o.append(document.createElement('a'), '', document.createElement('b')); \
                 o.normalize(); names(o)",
                "A,B",
            ),
        ],
    );
}

#[test]
fn positions_equality_and_namespaces() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var c = document.createElement('div'); c.innerHTML = '<a><b></b></a><i></i>'; \
                 var A = c.firstChild, B = A.firstChild, I = c.lastChild; \
                 [c.compareDocumentPosition(B), B.compareDocumentPosition(c), A.compareDocumentPosition(I), \
                  I.compareDocumentPosition(B), B.compareDocumentPosition(B), Node.DOCUMENT_POSITION_CONTAINED_BY].join()",
                "20,10,4,2,0,16",
            ),
            (
                "var x = document.createElement('x'); var r1 = c.compareDocumentPosition(x), r2 = x.compareDocumentPosition(c); \
                 [r1 & 1, r1 & 32, (r1 ^ r2) & 6, c.compareDocumentPosition(x) === r1].join()",
                "1,32,6,true",
            ),
            (
                "try { c.compareDocumentPosition(null) } catch (e) { e instanceof TypeError }",
                "true",
            ),
            (
                "var e1 = document.createElement('p'); e1.setAttribute('a', '1'); e1.setAttribute('b', '2'); e1.append('t'); \
                 var e2 = document.createElement('p'); e2.setAttribute('b', '2'); e2.setAttribute('a', '1'); e2.append('t'); \
                 [e1.isEqualNode(e2), e1.isEqualNode(null), e1.isSameNode(e1), e1.isSameNode(e2), e1.isSameNode(null)].join()",
                "true,false,true,false,false",
            ),
            (
                "e2.append('u'); \
                 var e4 = document.createElement('p'); e4.setAttribute('a', '1'); e4.setAttribute('b', '3'); e4.append('t'); \
                 var e5 = document.createElement('p'); e5.setAttribute('a', '1'); e5.append('t'); \
                 [e1.isEqualNode(e2), e1.isEqualNode(e4), document.createTextNode('a').isEqualNode(document.createTextNode('a')), \
                  document.createElement('p').isEqualNode(document.createElementNS('urn:x', 'p')), e5.isEqualNode(e1), \
                  document.createTextNode('a').isEqualNode(document.createTextNode('b'))].join()",
                "false,false,true,false,false,false",
            ),
            (
                "var g = document.createElementNS('http://www.w3.org/2000/svg', 'svg:g'); \
                 g.setAttributeNS('http://www.w3.org/2000/xmlns/', 'xmlns:x', 'urn:x'); \
                 var k = g.appendChild(document.createElementNS('urn:y', 'k')); var kt = k.appendChild(document.createTextNode('t')); \
                 [k.lookupNamespaceURI('svg'), k.lookupNamespaceURI('x'), k.lookupNamespaceURI(null), kt.lookupNamespaceURI(''), \
                  k.lookupNamespaceURI('nope'), k.lookupPrefix('urn:x'), k.lookupPrefix('http://www.w3.org/2000/svg'), k.lookupPrefix(''), \
                  k.isDefaultNamespace('urn:y'), k.isDefaultNamespace(''), g.isDefaultNamespace(null), \
                  document.createElement('p').lookupNamespaceURI('xml'), document.createElement('p').lookupNamespaceURI('xmlns')].join('|')",
                "http://www.w3.org/2000/svg|urn:x|urn:y|urn:y||x|svg||true|false|true|\
                 http://www.w3.org/XML/1998/namespace|http://www.w3.org/2000/xmlns/",
            ),
            // An empty declaration unbinds; the empty namespace has no prefix
            // and is no namespace; a document's are its element's.
            (
                "var z = document.createElementNS('urn:z', 'z:a'); z.setAttributeNS('http://www.w3.org/2000/xmlns/', 'xmlns:e', ''); \
                 var none = document.createElementNS(null, 'q'); \
                 var root = document.documentElement || document.appendChild(document.createElement('html')); \
                 [z.lookupNamespaceURI('e') === null, z.lookupPrefix(''), none.isDefaultNamespace(''), none.isDefaultNamespace(null), \
                  document.lookupNamespaceURI(null), document.isDefaultNamespace('http://www.w3.org/1999/xhtml')].join('|')",
                "true||true|true|http://www.w3.org/1999/xhtml|true",
            ),
        ],
    );
}
