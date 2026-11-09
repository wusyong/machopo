//! Ranges and traversal: `Range`'s boundary points (set with DOM's checks,
//! compared in tree order), `StaticRange`, a live range adjusted as its
//! tree changes (inserting, removing, replacing data, `splitText`,
//! `normalize`, another document's nodes, adoption); `NodeIterator`,
//! `TreeWalker` and their filters (a function or an object's
//! `acceptNode`, what they throw thrown on, the "active" flag), the
//! iterators' pre-removing steps; `NodeFilter`'s interface object.

use crate::env::Env;

fn window() -> Env {
    let mut env = Env::window("https://example.test/a/page.html");
    env.eval(
        "var html = document.appendChild(document.createElement('html')); \
         var body = html.appendChild(document.createElement('body')); \
         var error = (f) => { try { f(); return 'ok' } catch (e) { return e.name } }; \
         var names = (step) => { var all = []; var n; while ((n = step())) all.push(n.nodeName); return all.join() }; 0",
    );
    env
}

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
    }
}

#[test]
fn a_ranges_boundary_points_are_set_with_doms_checks() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var p = body.appendChild(document.createElement('p')); p.append('ab', document.createElement('i'), 'cd'); \
              var r = document.createRange(); \
              [r.startContainer === document, r.startOffset, r.collapsed, r instanceof Range, r instanceof AbstractRange, Range.END_TO_START].join()",
                "true,0,true,true,true,3",
            ),
            (
                "var made = new Range(); [made.startContainer === document, made.endOffset].join()",
                "true,0",
            ),
            (
                "r.setStart(p.firstChild, 1); r.setEnd(p, 3); \
              [r.startContainer === p.firstChild, r.startOffset, r.endContainer === p, r.endOffset, r.collapsed, r.commonAncestorContainer === p].join()",
                "true,1,true,3,false,true",
            ),
            // An end before the start takes the start with it; a point in
            // another tree, both.
            (
                "r.setEnd(p.firstChild, 0); [r.startContainer === p.firstChild, r.startOffset, r.collapsed].join()",
                "true,0,true",
            ),
            (
                "r.setEnd(p, 2); r.setStart(p, 3); [r.endContainer === p, r.endOffset].join()",
                "true,3",
            ),
            (
                "var lone = document.createElement('div'); r.setStart(lone, 0); [r.startContainer === lone, r.endContainer === lone, r.commonAncestorContainer === lone].join()",
                "true,true,true",
            ),
            (
                "r.selectNodeContents(p); var other = document.createElement('div'); r.setEnd(other, 0); \
              [r.startContainer === other, r.endContainer === other].join()",
                "true,true",
            ),
            (
                "[error(() => r.setStart(document.implementation.createDocumentType('html', '', ''), 0)), error(() => r.setEnd(p, 4)), \
               error(() => r.setStartBefore(document)), error(() => r.selectNode(document)), \
               error(() => r.selectNodeContents(document.implementation.createDocumentType('html', '', '')))].join()",
                "InvalidNodeTypeError,IndexSizeError,InvalidNodeTypeError,InvalidNodeTypeError,InvalidNodeTypeError",
            ),
            (
                "r.setStartBefore(p.childNodes[1]); r.setEndAfter(p.childNodes[1]); [r.startContainer === p, r.startOffset, r.endOffset].join()",
                "true,1,2",
            ),
            (
                "r.setStartAfter(p.firstChild); r.setEndBefore(p.lastChild); [r.startOffset, r.endOffset].join()",
                "1,2",
            ),
            (
                "r.selectNode(p); [r.startContainer === body, r.startOffset, r.endOffset].join()",
                "true,0,1",
            ),
            (
                "r.selectNodeContents(p.firstChild); [r.startOffset, r.endOffset].join()",
                "0,2",
            ),
            (
                "r.collapse(true); [r.endOffset, r.collapsed].join()",
                "0,true",
            ),
            (
                "r.selectNodeContents(p); r.collapse(); [r.startContainer === p, r.startOffset].join()",
                "true,3",
            ),
            (
                "var c = r.cloneRange(); r.detach(); [c !== r, c instanceof Range, c.startContainer === p, c.startOffset].join()",
                "true,true,true,3",
            ),
            // An attribute's length is 0: a point in it, at 0.
            (
                "var a = document.createAttribute('x'); r.setStart(a, 0); [r.startContainer === a, error(() => r.setStart(a, 1))].join()",
                "true,IndexSizeError",
            ),
        ],
    );
}

#[test]
fn ranges_and_points_are_compared_in_tree_order() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var p = body.appendChild(document.createElement('p')); p.append('abc', document.createElement('b'), 'def'); \
              var r = document.createRange(); r.setStart(p.firstChild, 1); r.setEnd(p.lastChild, 2); \
              var s = document.createRange(); s.selectNode(p.childNodes[1]); \
              [r.compareBoundaryPoints(Range.START_TO_START, s), r.compareBoundaryPoints(Range.START_TO_END, s), \
               r.compareBoundaryPoints(Range.END_TO_END, s), r.compareBoundaryPoints(Range.END_TO_START, s), \
               s.compareBoundaryPoints(Range.START_TO_START, r), r.compareBoundaryPoints(Range.START_TO_START, r)].join()",
                "-1,1,1,-1,1,0",
            ),
            // `unsigned short`: modulo 2^16.
            (
                "[r.compareBoundaryPoints(65536 + 1, s), error(() => r.compareBoundaryPoints(4, s))].join()",
                "1,NotSupportedError",
            ),
            // A string's negative number wraps too (V8's `Uint32Value`
            // doesn't).
            (
                "[error(() => r.compareBoundaryPoints('-1', s)), r.compareBoundaryPoints('-65535', s), r.compareBoundaryPoints('1.9', s), \
               r.compareBoundaryPoints('x', s), document.createNodeIterator(p, '-1').whatToShow].join()",
                "NotSupportedError,1,1,-1,4294967295",
            ),
            (
                "var o = document.createRange(); o.selectNodeContents(document.createElement('div')); error(() => r.compareBoundaryPoints(0, o))",
                "WrongDocumentError",
            ),
            (
                "var st = new StaticRange({startContainer: p, startOffset: 0, endContainer: p, endOffset: 0}); \
              [error(() => r.compareBoundaryPoints(0, st)), error(() => r.compareBoundaryPoints(0, null)), error(() => r.compareBoundaryPoints(0, p))].join()",
                "TypeError,TypeError,TypeError",
            ),
            (
                "[r.isPointInRange(p, 1), r.isPointInRange(p.firstChild, 0), r.isPointInRange(p.firstChild, 1), r.isPointInRange(p.lastChild, 2), \
               r.isPointInRange(p.lastChild, 3), r.isPointInRange(document.createElement('div'), 0)].join()",
                "true,false,true,true,false,false",
            ),
            (
                "[error(() => r.isPointInRange(p, 4)), error(() => r.comparePoint(p, 9))].join()",
                "IndexSizeError,IndexSizeError",
            ),
            (
                "[r.comparePoint(p.firstChild, 0), r.comparePoint(p, 2), r.comparePoint(p.lastChild, 3), \
               error(() => r.comparePoint(document.createElement('div'), 0))].join()",
                "-1,0,1,WrongDocumentError",
            ),
            (
                "var after = body.appendChild(document.createElement('s')); \
              [r.intersectsNode(p.childNodes[1]), r.intersectsNode(body), r.intersectsNode(after), r.intersectsNode(document), \
               r.intersectsNode(document.createElement('div'))].join()",
                "true,true,false,true,false",
            ),
            // Ending where the node starts: not intersecting it.
            (
                "var e2 = document.createRange(); e2.setStart(p, 0); e2.setEnd(p, 1); [e2.intersectsNode(p.childNodes[1]), e2.intersectsNode(p.firstChild)].join()",
                "false,true",
            ),
        ],
    );
}

#[test]
fn a_static_range_is_taken_as_it_is_and_never_adjusted() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var t = body.appendChild(new Text('hello')); \
              var sr = new StaticRange({startContainer: t, startOffset: 1, endContainer: t, endOffset: 9}); \
              [sr.startContainer === t, sr.startOffset, sr.endOffset, sr.collapsed, sr instanceof AbstractRange, sr instanceof StaticRange, \
               sr instanceof Range, typeof sr.setStart].join()",
                "true,1,9,false,true,true,false,undefined",
            ),
            (
                "t.data = 'x'; t.before('y'); [sr.startOffset, sr.endOffset].join()",
                "1,9",
            ),
            (
                "var two = new StaticRange({startContainer: body, startOffset: 0, endContainer: t, endOffset: 2}); \
              [two.startContainer === body, two.endContainer === t, two.startOffset, two.endOffset].join()",
                "true,true,0,2",
            ),
            (
                "var doctype = document.implementation.createDocumentType('a', '', ''); \
              [error(() => new StaticRange({startContainer: doctype, startOffset: 0, endContainer: t, endOffset: 0})), \
               error(() => new StaticRange({startContainer: t, startOffset: 0, endContainer: document.createAttribute('a'), endOffset: 0})), \
               error(() => new StaticRange({startContainer: t, startOffset: 0, endContainer: t})), \
               error(() => new StaticRange({startContainer: {}, startOffset: 0, endContainer: t, endOffset: 0})), \
               error(() => new StaticRange()), error(() => StaticRange({})), error(() => Range.prototype.setStart.call(sr, t, 0))].join()",
                "InvalidNodeTypeError,InvalidNodeTypeError,TypeError,TypeError,TypeError,TypeError,TypeError",
            ),
        ],
    );
}

#[test]
fn a_live_range_follows_insertions_and_removals() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var p = body.appendChild(document.createElement('p')); p.append('a', 'b', 'c'); \
              var r = document.createRange(); r.setStart(p, 1); r.setEnd(p, 3); \
              p.insertBefore(document.createElement('i'), p.firstChild); [r.startOffset, r.endOffset].join()",
                "2,4",
            ),
            // Before the child at the start: past it only.
            (
                "p.insertBefore(document.createElement('u'), p.childNodes[2]); [r.startOffset, r.endOffset].join()",
                "2,5",
            ),
            (
                "p.appendChild(document.createElement('s')); [r.startOffset, r.endOffset].join()",
                "2,5",
            ),
            // A fragment's children: their count.
            (
                "var f = document.createDocumentFragment(); f.append('x', 'y'); p.insertBefore(f, p.firstChild); [r.startOffset, r.endOffset].join()",
                "4,7",
            ),
            (
                "p.firstChild.remove(); p.removeChild(p.lastChild); [r.startOffset, r.endOffset].join()",
                "3,6",
            ),
            // Moved within its parent: out, then in.
            (
                "p.insertBefore(p.lastChild, p.firstChild); [r.startOffset, r.endOffset].join()",
                "4,6",
            ),
            // A point in what goes moves to where it was.
            (
                "var d = body.appendChild(document.createElement('div')); d.innerHTML = '<b>xy</b><i>z</i>'; \
              var q = document.createRange(); q.setStart(d.firstChild.firstChild, 1); q.setEnd(d.lastChild.firstChild, 1); \
              d.firstChild.remove(); [q.startContainer === d, q.startOffset, q.endContainer === d.firstChild.firstChild, q.endOffset].join()",
                "true,0,true,1",
            ),
            (
                "d.replaceChildren(); [q.startContainer === d, q.startOffset, q.endContainer === d, q.endOffset].join()",
                "true,0,true,0",
            ),
            (
                "d.innerHTML = 'a<b>xy</b>c'; q.setStart(d.childNodes[1].firstChild, 1); q.setEnd(d, 3); d.childNodes[1].remove(); \
              [q.startContainer === d, q.startOffset, q.endOffset].join()",
                "true,1,2",
            ),
            // A fragment's own: its children leave it.
            (
                "var frag = document.createDocumentFragment(); frag.append('m', 'n'); var fr = document.createRange(); fr.setStart(frag, 2); \
              d.append(frag); [fr.startContainer === frag, fr.startOffset].join()",
                "true,0",
            ),
            // `textContent` replaces all.
            (
                "q.setStart(d, 2); d.textContent = 'all'; [q.startContainer === d, q.startOffset].join()",
                "true,0",
            ),
        ],
    );
}

#[test]
fn a_live_range_follows_its_datas_changes() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var t = body.appendChild(new Text('abcdef')); var r = document.createRange(); r.setStart(t, 2); r.setEnd(t, 5); \
              t.insertData(1, 'XY'); [r.startOffset, r.endOffset].join()",
                "4,7",
            ),
            // In the part replaced: to its start; past it: with the rest.
            (
                "t.deleteData(3, 2); [r.startOffset, r.endOffset, t.data].join()",
                "3,5,aXYdef",
            ),
            (
                "t.replaceData(4, 1, '123'); [r.startOffset, r.endOffset].join()",
                "3,4",
            ),
            (
                "r.setEnd(t, 6); t.replaceData(4, 1, ''); [r.startOffset, r.endOffset].join()",
                "3,5",
            ),
            (
                "t.replaceData(0, 100, 'pq'); [r.startOffset, r.endOffset].join()",
                "0,0",
            ),
            (
                "r.setStart(t, 1); r.setEnd(t, 2); t.appendData('rs'); [r.startOffset, r.endOffset].join()",
                "1,2",
            ),
            ("t.data = 'zz'; [r.startOffset, r.endOffset].join()", "0,0"),
            (
                "r.setStart(t, 0); r.setEnd(t, 2); t.nodeValue = 'abc'; t.textContent = 'abcd'; [r.startOffset, r.endOffset].join()",
                "0,0",
            ),
            // UTF-16 code units.
            (
                "t.data = '😀a'; r.setStart(t, 3); r.collapse(true); t.insertData(0, '😀'); r.startOffset",
                "5",
            ),
        ],
    );
}

#[test]
fn a_live_range_follows_split_and_normalized_texts() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var p = body.appendChild(document.createElement('p')); var t = p.appendChild(new Text('abcdef')); \
              var r = document.createRange(); r.setStart(t, 1); r.setEnd(t, 5); \
              var after = document.createRange(); after.setStart(p, 1); \
              var at = document.createRange(); at.setStart(t, 3); \
              var u = t.splitText(3); \
              [r.startContainer === t, r.startOffset, r.endContainer === u, r.endOffset, after.startContainer === p, after.startOffset, \
               at.startContainer === t, at.startOffset].join()",
                "true,1,true,2,true,2,true,3",
            ),
            // Between the two: into the first, at its end.
            (
                "var between = document.createRange(); between.setStart(p, 1); p.normalize(); \
              [r.startContainer === t, r.startOffset, r.endContainer === t, r.endOffset, between.startContainer === t, between.startOffset, \
               after.startContainer === p, after.startOffset, t.data, p.childNodes.length].join()",
                "true,1,true,5,true,3,true,1,abcdef,1",
            ),
            // An empty text goes; the ones after a run's first join it.
            (
                "p.append('', 'gh', '', 'i'); var e = document.createRange(); e.setStart(p.childNodes[4], 1); \
              var g = document.createRange(); g.setStart(p.childNodes[2], 2); p.normalize(); \
              [p.childNodes.length, t.data, e.startContainer === t, e.startOffset, g.startContainer === t, g.startOffset].join()",
                "1,abcdefghi,true,9,true,8",
            ),
        ],
    );
}

#[test]
fn a_live_range_moves_to_the_document_its_start_is_in() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var other = document.implementation.createHTMLDocument(''); \
              var div = other.createElement('div'); div.append('abc'); \
              var r = document.createRange(); r.setStart(div.firstChild, 1); r.setEnd(div.firstChild, 2); \
              div.firstChild.insertData(0, 'XX'); [r.startOffset, r.endOffset].join()",
                "3,4",
            ),
            // Adopted with the nodes it is in.
            (
                "document.adoptNode(div); div.firstChild.insertData(0, 'Y'); [r.startOffset, r.endOffset].join()",
                "4,5",
            ),
            // Many collected, the live one still adjusted.
            (
                "for (var i = 0; i < 100; i++) document.createRange().selectNodeContents(div); gc(); \
              for (var i = 0; i < 40; i++) document.createRange(); \
              div.firstChild.insertData(0, 'Z'); [r.startOffset, r.endOffset].join()",
                "5,6",
            ),
        ],
    );
}

#[test]
fn a_node_iterator_walks_its_roots_subtree() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var root = body.appendChild(document.createElement('div')); root.innerHTML = '<a><b></b></a>t<c></c>'; \
              body.append(document.createElement('after')); body.prepend(document.createElement('before')); \
              var it = document.createNodeIterator(root); names(() => it.nextNode())",
                "DIV,A,B,#text,C",
            ),
            (
                "[it.referenceNode.nodeName, it.pointerBeforeReferenceNode, it.root === root, it.whatToShow, it.filter, it instanceof NodeIterator].join()",
                "C,false,true,4294967295,,true",
            ),
            ("names(() => it.previousNode())", "C,#text,B,A,DIV"),
            (
                "[it.referenceNode === root, it.pointerBeforeReferenceNode, it.previousNode(), it.detach()].join()",
                "true,true,,",
            ),
            (
                "var elements = document.createNodeIterator(root, NodeFilter.SHOW_ELEMENT); names(() => elements.nextNode())",
                "DIV,A,B,C",
            ),
            (
                "[document.createNodeIterator(root, undefined).whatToShow, document.createNodeIterator(root, 1).whatToShow, \
               document.createNodeIterator(root, -1).whatToShow].join()",
                "4294967295,1,4294967295",
            ),
        ],
    );
}

#[test]
fn a_traversals_filter_is_a_function_or_an_objects_accept_node() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var root = body.appendChild(document.createElement('div')); root.innerHTML = '<a><b></b></a>t<c></c>'; \
              var skip = (n) => n.nodeName == 'B' ? NodeFilter.FILTER_SKIP : NodeFilter.FILTER_ACCEPT; \
              var it = document.createNodeIterator(root, NodeFilter.SHOW_ALL, skip); [names(() => it.nextNode()), it.filter === skip].join()",
                "DIV,A,#text,C,true",
            ),
            (
                "var f = { calls: 0, acceptNode(n) { this.calls++; return n.nodeType == 1 ? 1 : 3 } }; \
              var it2 = document.createNodeIterator(root, -1, f); [names(() => it2.nextNode()), f.calls, it2.filter === f].join()",
                "DIV,A,B,C,5,true",
            ),
            // Not shown: never filtered.
            (
                "var seen = []; var it3 = document.createNodeIterator(root, NodeFilter.SHOW_TEXT, (n) => { seen.push(n.nodeName); return 1 }); \
              [names(() => it3.nextNode()), seen.join()].join(';')",
                "#text;#text",
            ),
            // `unsigned short`: any value converted.
            (
                "var string = document.createNodeIterator(root, 1, () => '1'); var wrapped = document.createNodeIterator(root, 1, () => 65537); \
              var skipped = document.createNodeIterator(root, 1, () => 65539); \
              [names(() => string.nextNode()), names(() => wrapped.nextNode()), names(() => skipped.nextNode())].join(';')",
                "DIV,A,B,C;DIV,A,B,C;",
            ),
            // What it throws is thrown, and it can run again.
            (
                "var thrower = document.createNodeIterator(root, -1, () => { throw new RangeError('boom') }); \
              [error(() => thrower.nextNode()), error(() => thrower.nextNode()), thrower.referenceNode === root].join()",
                "RangeError,RangeError,true",
            ),
            (
                "[error(() => document.createNodeIterator(root, -1, {}).nextNode()), \
               error(() => document.createNodeIterator(root, -1, {acceptNode: 1}).nextNode()), \
               error(() => document.createNodeIterator(root, -1, 1))].join()",
                "TypeError,TypeError,TypeError",
            ),
            (
                "var getter = { get acceptNode() { throw new SyntaxError('get') } }; error(() => document.createNodeIterator(root, -1, getter).nextNode())",
                "SyntaxError",
            ),
            // Its filter using it: an `InvalidStateError`.
            (
                "var inner = ''; var nested = document.createNodeIterator(root, -1, (n) => { inner = error(() => nested.nextNode()); return 1 }); \
              [nested.nextNode().nodeName, inner].join()",
                "DIV,InvalidStateError",
            ),
            (
                "var wInner = []; var w = document.createTreeWalker(root, -1, (n) => { wInner.push(error(() => w.firstChild()), String(w.parentNode())); return 1 }); \
              [w.firstChild().nodeName, wInner.join()].join(';')",
                "A;InvalidStateError,null",
            ),
        ],
    );
}

#[test]
fn a_node_iterator_stays_in_the_tree_as_nodes_are_removed() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var root = body.appendChild(document.createElement('div')); root.innerHTML = '<a><b></b></a>t'; \
              var it = document.createNodeIterator(root); it.nextNode(); it.nextNode(); it.nextNode(); \
              root.firstChild.remove(); [it.referenceNode === root, it.pointerBeforeReferenceNode, it.nextNode().nodeName].join()",
                "true,false,#text",
            ),
            // The pointer before it: to the node after the one removed.
            (
                "root.innerHTML = '<a></a><b></b><c></c>'; var it2 = document.createNodeIterator(root); \
              it2.nextNode(); it2.nextNode(); it2.nextNode(); it2.previousNode(); root.childNodes[1].remove(); \
              [it2.referenceNode.nodeName, it2.pointerBeforeReferenceNode, it2.nextNode().nodeName].join()",
                "C,true,C",
            ),
            // None after: before it, the last of its previous sibling.
            (
                "root.innerHTML = '<a><i></i></a><b></b>'; body.append(document.createElement('after')); var it3 = document.createNodeIterator(root); \
              while (it3.nextNode()); it3.previousNode(); root.lastChild.remove(); \
              [it3.referenceNode.nodeName, it3.pointerBeforeReferenceNode].join()",
                "I,false",
            ),
            // Removing the root, or above it: nothing moves.
            (
                "var held = it3.referenceNode; root.remove(); [it3.referenceNode === held, it3.root === root].join()",
                "true,true",
            ),
            // A filter removing the node it is given: the walk goes on from
            // where the position was moved.
            (
                "body.append(root); root.innerHTML = '<a></a><b></b>'; \
              var it4 = document.createNodeIterator(root, NodeFilter.SHOW_ELEMENT, (n) => { if (n.nodeName == 'A') n.remove(); return 1 }); \
              [it4.nextNode().nodeName, it4.nextNode().nodeName, it4.referenceNode === root, it4.nextNode().nodeName].join()",
                "DIV,A,true,B",
            ),
            // Its root adopted: the iterator goes with it.
            (
                "var other = document.implementation.createHTMLDocument(''); var outer = other.createElement('div'); outer.innerHTML = '<a></a><b></b>'; \
              var it5 = document.createNodeIterator(outer); it5.nextNode(); it5.nextNode(); it5.nextNode(); \
              document.adoptNode(outer); outer.lastChild.remove(); it5.referenceNode.nodeName",
                "A",
            ),
            (
                "for (var i = 0; i < 100; i++) document.createNodeIterator(root); gc(); \
              var kept = document.createNodeIterator(root); kept.nextNode(); kept.nextNode(); root.firstChild.remove(); kept.referenceNode === root",
                "true",
            ),
        ],
    );
}

#[test]
fn a_tree_walker_walks_as_its_filter_says() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var root = body.appendChild(document.createElement('div')); root.innerHTML = '<a><b></b><c></c></a><d></d>'; \
              var w = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT); \
              [w.currentNode === root, w.firstChild().nodeName, w.firstChild().nodeName, w.nextSibling().nodeName, w.nextSibling(), \
               w.parentNode().nodeName, w.nextSibling().nodeName, w.previousSibling().nodeName, w.lastChild().nodeName, w instanceof TreeWalker].join()",
                "true,A,B,C,,A,D,A,C,true",
            ),
            (
                "[w.nextNode().nodeName, w.previousNode().nodeName, w.previousNode().nodeName, w.previousNode().nodeName, \
               w.previousNode() === root, w.previousNode(), w.parentNode(), w.nextSibling()].join()",
                "D,C,B,A,true,,,",
            ),
            (
                "[w.root === root, w.whatToShow, w.filter].join()",
                "true,1,",
            ),
            // A skipped node's children are looked at; a rejected one's
            // aren't.
            (
                "var skip = document.createTreeWalker(root, -1, (n) => n.nodeName == 'A' ? NodeFilter.FILTER_SKIP : 1); \
              [names(() => skip.nextNode()), skip.filter instanceof Function].join(';')",
                "B,C,D;true",
            ),
            (
                "var reject = document.createTreeWalker(root, -1, (n) => n.nodeName == 'A' ? NodeFilter.FILTER_REJECT : 1); \
              [names(() => reject.nextNode()), reject.firstChild(), names(() => reject.previousNode())].join(';')",
                "D;;DIV",
            ),
            (
                "skip.currentNode = root; [skip.firstChild().nodeName, skip.nextSibling().nodeName, skip.lastChild()].join()",
                "B,C,",
            ),
            (
                "skip.currentNode = root.firstChild.lastChild; [skip.nextSibling().nodeName, skip.parentNode().nodeName].join()",
                "D,DIV",
            ),
            (
                "reject.currentNode = root.firstChild.firstChild; [reject.nextSibling().nodeName, reject.parentNode().nodeName].join()",
                "C,DIV",
            ),
            // A rejected sibling's children aren't looked at; a skipped
            // one's are.
            (
                "reject.currentNode = root.lastChild; var none = reject.previousSibling(); skip.currentNode = root.lastChild; \
              [none, skip.previousNode().nodeName].join()",
                ",C",
            ),
            // Its current node anywhere; what the filter throws is thrown.
            (
                "var lone = document.createElement('p'); w.currentNode = lone; [w.currentNode === lone, w.parentNode(), w.nextNode()].join()",
                "true,,",
            ),
            (
                "var thrower = document.createTreeWalker(root, -1, () => { throw new TypeError('no') }); \
              [error(() => thrower.firstChild()), thrower.currentNode === root, error(() => thrower.nextNode())].join()",
                "TypeError,true,TypeError",
            ),
        ],
    );
}

#[test]
fn node_filter_is_a_legacy_callback_interface_object() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "[typeof NodeFilter, NodeFilter.name, NodeFilter.FILTER_SKIP, NodeFilter.SHOW_ALL, NodeFilter.SHOW_NOTATION, \
               'prototype' in NodeFilter, error(() => NodeFilter()), error(() => new NodeFilter())].join()",
                "function,NodeFilter,3,4294967295,2048,false,TypeError,TypeError",
            ),
            (
                "var d = Object.getOwnPropertyDescriptor(NodeFilter, 'SHOW_TEXT'); [d.value, d.writable, d.enumerable, d.configurable, \
               Object.getOwnPropertyDescriptor(window, 'NodeFilter').enumerable].join()",
                "4,false,true,false,false",
            ),
        ],
    );
}

#[test]
fn a_ranges_contents_are_cloned_extracted_and_deleted() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var p = body.appendChild(document.createElement('p')); p.innerHTML = 'ab<b>cd</b><i>ef</i>gh'; \
              var r = document.createRange(); r.setStart(p.firstChild, 1); r.setEnd(p.childNodes[2].firstChild, 1); \
              var c = r.cloneContents(); var div = document.createElement('div'); var isFragment = c instanceof DocumentFragment; div.append(c); \
              [div.innerHTML, p.innerHTML, isFragment, c.ownerDocument === document].join('|')",
                "b<b>cd</b><i>e</i>|ab<b>cd</b><i>ef</i>gh|true|true",
            ),
            (
                "var e = r.extractContents(); div.replaceChildren(e); [div.innerHTML, p.innerHTML, r.startContainer === p, r.startOffset, r.collapsed].join('|')",
                "b<b>cd</b><i>e</i>|a<i>f</i>gh|true|1|true",
            ),
            (
                "p.innerHTML = 'ab<b>cd</b><i>ef</i>gh'; r.setStart(p.firstChild, 1); r.setEnd(p.lastChild, 1); r.deleteContents(); \
              [p.innerHTML, r.startContainer === p, r.startOffset, r.collapsed].join('|')",
                "ah|true|1|true",
            ),
            // In one text: its data.
            (
                "p.innerHTML = 'hello'; r.setStart(p.firstChild, 1); r.setEnd(p.firstChild, 3); var one = r.cloneContents(); \
              r.deleteContents(); [one.firstChild.data, p.innerHTML, r.startOffset, r.endOffset].join('|')",
                "el|hlo|1|1",
            ),
            (
                "[r.extractContents().childNodes.length, r.cloneContents().childNodes.length, p.innerHTML].join('|')",
                "0|0|hlo",
            ),
            // A node the range starts at the start of isn't contained; one it
            // starts in, partly, gives a copy of its part.
            (
                "p.innerHTML = '<b>x</b>y'; r.setStart(p.firstChild, 0); r.setEnd(p, 2); div.replaceChildren(r.cloneContents()); div.innerHTML",
                "<b>x</b>y",
            ),
            (
                "p.innerHTML = '<b>cd</b>ef'; r.setStart(p.firstChild.firstChild, 1); r.setEnd(p.lastChild, 1); div.replaceChildren(r.cloneContents()); div.innerHTML",
                "<b>d</b>e",
            ),
            // What goes keeps its own children.
            (
                "p.innerHTML = 'a<b><i>c</i></b>d'; var gone = p.childNodes[1]; r.setStart(p.firstChild, 0); r.setEnd(p.lastChild, 1); r.deleteContents(); \
              [p.innerHTML, gone.innerHTML].join('|')",
                "|<i>c</i>",
            ),
            // The end in the start: what is after it in the start goes.
            (
                "p.innerHTML = '<b>xy</b>z'; r.setStart(p, 0); r.setEnd(p.firstChild.firstChild, 1); r.deleteContents(); \
              [p.innerHTML, r.startContainer === p, r.startOffset].join('|')",
                "<b>y</b>z|true|0",
            ),
            (
                "document.insertBefore(document.implementation.createDocumentType('html', '', ''), document.firstChild); \
              var whole = document.createRange(); whole.selectNodeContents(document); \
              [error(() => whole.cloneContents()), error(() => whole.extractContents()), document.childNodes.length].join()",
                "HierarchyRequestError,HierarchyRequestError,2",
            ),
            // Checked before anything moves.
            (
                "document.insertBefore(document.createComment('first'), document.firstChild); whole.setStart(document, 0); whole.setEnd(document, 2); \
              [error(() => whole.extractContents()), document.childNodes.length, document.firstChild.data].join()",
                "HierarchyRequestError,3,first",
            ),
        ],
    );
}

#[test]
fn a_ranges_text_is_its_texts_data() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var p = body.appendChild(document.createElement('p')); p.innerHTML = 'ab<b>cd</b><!--x-->ef'; \
              var r = document.createRange(); r.setStart(p.firstChild, 1); r.setEnd(p.lastChild, 1); [r.toString(), String(r), r + ''].join('|')",
                "bcde|bcde|bcde",
            ),
            (
                "r.selectNodeContents(p.childNodes[1].firstChild); var a = r.toString(); r.collapse(); [a, r.toString(), new Range().toString()].join('|')",
                "cd||",
            ),
            ("r.selectNodeContents(p); r.toString()", "abcdef"),
            (
                "r.setStart(p.firstChild, 1); r.setEnd(p.firstChild, 2); r.toString()",
                "b",
            ),
        ],
    );
}

#[test]
fn a_node_is_inserted_at_a_ranges_start() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var p = body.appendChild(document.createElement('p')); p.innerHTML = 'abcd'; \
              var r = document.createRange(); r.setStart(p.firstChild, 2); r.collapse(true); r.insertNode(document.createElement('u')); \
              [p.innerHTML, r.startContainer === p.firstChild, r.startOffset, r.endContainer === p, r.endOffset].join('|')",
                "ab<u></u>cd|true|2|true|2",
            ),
            // A fragment's children; a range not collapsed keeps its end.
            (
                "r.setStart(p, 0); r.collapse(true); var f = document.createDocumentFragment(); f.append('x', 'y'); r.insertNode(f); \
              [p.firstChild.data, r.endOffset].join('|')",
                "x|2",
            ),
            (
                "r.setStart(p, 0); r.setEnd(p, 1); r.insertNode(document.createElement('s')); [p.firstChild.nodeName, r.startOffset, r.endOffset].join('|')",
                "S|0|2",
            ),
            // Already in the tree: moved.
            (
                "var last = p.lastChild; r.setStart(p, 0); r.collapse(true); r.insertNode(last); p.firstChild === last",
                "true",
            ),
            // The child at the start itself: where it is.
            (
                "var before = p.innerHTML; r.setStart(p, 0); r.collapse(true); r.insertNode(p.firstChild); [p.innerHTML === before, r.endOffset].join()",
                "true,1",
            ),
            (
                "var comment = p.appendChild(document.createComment('c')); var at = document.createRange(); \
              var tries = [[comment, 0], [new Text('lone'), 0], [document.createAttribute('a'), 0]].map(([node, offset]) => { \
                at.setStart(node, offset); return error(() => at.insertNode(document.createElement('x'))) }); \
              at.setStart(p, 0); tries.push(error(() => at.insertNode(body)), error(() => at.insertNode(p)), error(() => at.insertNode(document.createAttribute('b')))); \
              tries.join()",
                "HierarchyRequestError,HierarchyRequestError,HierarchyRequestError,HierarchyRequestError,HierarchyRequestError,HierarchyRequestError",
            ),
            // Checked before the text splits.
            (
                "p.innerHTML = 'ab'; at.setStart(p.firstChild, 1); [error(() => at.insertNode(body)), p.childNodes.length].join()",
                "HierarchyRequestError,1",
            ),
        ],
    );
}

#[test]
fn a_ranges_contents_are_surrounded() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var p = body.appendChild(document.createElement('p')); p.innerHTML = 'abcd'; \
              var r = document.createRange(); r.setStart(p.firstChild, 1); r.setEnd(p.firstChild, 3); \
              var s = document.createElement('s'); s.textContent = 'old'; r.surroundContents(s); \
              [p.innerHTML, r.startContainer === p, r.startOffset, r.endOffset].join('|')",
                "a<s>bc</s>d|true|1|2",
            ),
            (
                "p.innerHTML = 'a<b>c</b>'; r.setStart(p.firstChild, 0); r.setEnd(p.lastChild.firstChild, 1); \
              [error(() => r.surroundContents(document.createElement('s'))), p.innerHTML].join('|')",
                "InvalidStateError|a<b>c</b>",
            ),
            (
                "r.selectNodeContents(p); [document.createDocumentFragment(), document, document.implementation.createDocumentType('a', '', '')] \
              .map((parent) => error(() => r.surroundContents(parent))).join()",
                "InvalidNodeTypeError,InvalidNodeTypeError,InvalidNodeTypeError",
            ),
            (
                "var wrap = document.createElement('i'); r.surroundContents(wrap); [p.innerHTML, r.startContainer === p, r.startOffset, r.endOffset].join('|')",
                "<i>a<b>c</b></i>|true|0|1",
            ),
        ],
    );
}

#[test]
fn a_contextual_fragment_is_parsed_in_the_ranges_element() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var p = body.appendChild(document.createElement('p')); p.textContent = 'x'; var r = document.createRange(); r.selectNodeContents(p); \
              var f = r.createContextualFragment('<td>x</td><b>y</b>'); \
              [f.childNodes.length, f.firstChild.data, f.lastChild.nodeName, f instanceof DocumentFragment, f.ownerDocument === document, p.innerHTML].join()",
                "2,x,B,true,true,x",
            ),
            (
                "var t = body.appendChild(document.createElement('table')); r.selectNodeContents(t); r.createContextualFragment('<tr><td>1</td></tr>').firstChild.nodeName",
                "TBODY",
            ),
            (
                "t.append('in'); r.setStart(t.lastChild, 0); r.createContextualFragment('<tr><td>1</td></tr>').firstChild.nodeName",
                "TBODY",
            ),
            // A text's parent element; a new body for none or the root.
            (
                "r.setStart(p.firstChild, 0); var tf = r.createContextualFragment('<td>z</td>'); r.selectNodeContents(document); \
              var df = r.createContextualFragment('<td>w</td>'); r.selectNodeContents(html); \
              var hf = r.createContextualFragment('<head></head><p>v</p>'); [tf.firstChild.data, df.firstChild.data, hf.firstChild.nodeName].join()",
                "z,w,P",
            ),
            (
                "var made = []; customElements.define('x-up', class extends HTMLElement { constructor() { super(); made.push('made') } }); \
              var uf = r.createContextualFragment('<x-up></x-up>'); [made.join(), uf.firstChild instanceof customElements.get('x-up')].join()",
                "made,true",
            ),
            (
                "var xml = document.implementation.createDocument(null, 'root'); var xr = xml.createRange(); xr.selectNodeContents(xml.documentElement); \
              error(() => xr.createContextualFragment('<a/>'))",
                "NotSupportedError",
            ),
        ],
    );
}
