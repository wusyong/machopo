//! Shadow trees from script: `attachShadow` (which elements, its init,
//! one root a host), `shadowRoot` and `ShadowRoot`'s members, the tree
//! scope of a node in a shadow tree (`getRootNode`, `isConnected`, ids,
//! `innerHTML`), and an event's dispatch through hosts: its path, the
//! target and related target each listener sees, `composedPath()` with
//! closed trees, and the targets it leaves.

use crate::env::Env;

/// A window whose document has `<html><body><div id=host>`, `host` an
/// open shadow root's host with `<span id=inner>` in it, `root` the root.
fn page() -> Env {
    let mut env = Env::window("about:blank");
    env.eval(
        "var html = document.documentElement || document.appendChild(document.createElement('html')); \
         var body = html.appendChild(document.createElement('body')); \
         var host = body.appendChild(document.createElement('div')); host.id = 'host'; \
         var root = host.attachShadow({ mode: 'open' }); \
         var inner = root.appendChild(document.createElement('span')); inner.id = 'inner'; 0",
    );
    env
}

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
    }
}

#[test]
fn attach_shadow_makes_a_root_for_its_host() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "[root instanceof ShadowRoot, root instanceof DocumentFragment, root.nodeType, root.nodeName, \
                  root.mode, root.host === host, host.shadowRoot === root, root.delegatesFocus, root.slotAssignment, \
                  root.clonable, root.serializable, Object.getPrototypeOf(ShadowRoot.prototype) === DocumentFragment.prototype].join()",
                "true,true,11,#document-fragment,open,true,true,false,named,false,false,true",
            ),
            (
                "var c = document.createElement('x-y').attachShadow({ mode: 'closed', delegatesFocus: true, \
                   slotAssignment: 'manual', clonable: true, serializable: true }); \
                 [c.mode, c.host.shadowRoot, c.delegatesFocus, c.slotAssignment, c.clonable, c.serializable].join()",
                "closed,,true,manual,true,true",
            ),
            // One root a host.
            (
                "try { host.attachShadow({ mode: 'open' }); 'attached' } catch (e) { e.name }",
                "NotSupportedError",
            ),
            // Of the elements DOM lists, and custom ones, in HTML.
            (
                "['span', 'p', 'body', 'h6', 'my-element', 'img', 'button', 'a', 'font-face'].map((n) => { \
                   try { document.createElement(n).attachShadow({ mode: 'open' }); return 'ok' } catch (e) { return e.name } }).join()",
                "ok,ok,ok,ok,ok,NotSupportedError,NotSupportedError,NotSupportedError,NotSupportedError",
            ),
            (
                "['g', 'div', 'x-y'].map((n) => { \
                   try { document.createElementNS('http://www.w3.org/2000/svg', n).attachShadow({ mode: 'open' }); return 'ok' } \
                   catch (e) { return e.name } }).join()",
                "NotSupportedError,NotSupportedError,NotSupportedError",
            ),
            // `mode` is required; the enums are checked.
            (
                "['{}', \"{ mode: 'nope' }\", \"{ mode: 'open', slotAssignment: 'auto' }\"].map((init) => { \
                   try { document.createElement('div').attachShadow(eval('(' + init + ')')); return 'ok' } \
                   catch (e) { return e instanceof TypeError } }).join()",
                "true,true,true",
            ),
            (
                "try { Object.getOwnPropertyDescriptor(ShadowRoot.prototype, 'mode').get.call(document.createDocumentFragment()); 'read' } \
                 catch (e) { e instanceof TypeError }",
                "true",
            ),
        ],
    );
}

#[test]
fn a_shadow_tree_is_its_nodes_tree_scope() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "[inner.getRootNode() === root, inner.getRootNode({ composed: true }) === document, \
                  root.getRootNode() === root, inner.isConnected, root.isConnected, inner.ownerDocument === document, \
                  inner.parentNode === root, root.parentNode, inner.parentElement].join()",
                "true,true,true,true,true,true,true,,",
            ),
            // Ids are the tree scope's.
            (
                "[root.getElementById('inner') === inner, document.getElementById('inner'), \
                  document.getElementById('host') === host, root.querySelector('#inner') === inner].join()",
                "true,,true,true",
            ),
            // Out of the document with its host, and back.
            (
                "body.removeChild(host); var out = [inner.isConnected, root.isConnected, inner.getRootNode({ composed: true }) === host]; \
                 body.appendChild(host); out.push(inner.isConnected); out.join()",
                "false,false,true,true",
            ),
            // Out of the shadow tree: its own root.
            (
                "var moved = root.appendChild(document.createElement('b')); var deep = moved.appendChild(document.createElement('i')); \
                 var before = deep.getRootNode() === root; root.removeChild(moved); \
                 [before, deep.getRootNode() === moved, moved.getRootNode() === moved, deep.isConnected, \
                  root.getElementById('inner') === inner].join()",
                "true,true,true,false,true",
            ),
            // Into another host's tree, one in a shadow tree.
            (
                "var nested = inner.attachShadow({ mode: 'open' }); var leaf = nested.appendChild(document.createElement('em')); \
                 [leaf.getRootNode() === nested, leaf.getRootNode({ composed: true }) === document, leaf.isConnected, \
                  nested.host === inner].join()",
                "true,true,true,true",
            ),
            (
                "root.innerHTML = '<b id=q>x</b><i></i>'; [root.getElementById('q').textContent, \
                 root.innerHTML, inner.isConnected, root.firstChild.getRootNode() === root].join()",
                "x,<b id=\"q\">x</b><i></i>,false,true",
            ),
        ],
    );
}

#[test]
fn events_go_through_hosts_retargeted() {
    let mut env = page();
    env.eval(
        "var log = []; \
         var note = (name) => (e) => log.push(name + ':' + e.eventPhase + ':' + (e.target && (e.target.id || e.target.nodeName))); \
         inner.addEventListener('e', note('inner')); root.addEventListener('e', note('root')); \
         host.addEventListener('e', note('host')); body.addEventListener('e', note('body')); \
         document.addEventListener('e', note('document'), true); 0",
    );
    check(
        &mut env,
        &[
            // Composed: out through the host, each seeing its own target.
            (
                "log = []; var ev = new Event('e', { bubbles: true, composed: true }); inner.dispatchEvent(ev); \
                 [log.join(' '), ev.target === host, ev.composedPath().length].join(' | ')",
                "document:1:host inner:2:inner root:3:inner host:2:host body:3:host | true | 0",
            ),
            // Not composed: the shadow tree alone; its target forgotten.
            (
                "log = []; var ev2 = new Event('e', { bubbles: true }); inner.dispatchEvent(ev2); [log.join(' '), ev2.target].join(' | ')",
                "inner:2:inner root:3:inner | ",
            ),
            // Not bubbling: at the target and at each host.
            (
                "log = []; inner.dispatchEvent(new Event('e', { composed: true })); log.join(' ')",
                "document:1:host inner:2:inner host:2:host",
            ),
            // At the shadow root itself: its host is the target outside.
            (
                "log = []; root.dispatchEvent(new Event('e', { bubbles: true, composed: true })); log.join(' ')",
                "document:1:host root:2:#document-fragment host:2:host body:3:host",
            ),
        ],
    );
}

#[test]
fn composed_path_hides_closed_trees() {
    let mut env = page();
    env.eval(
        "var closedHost = body.appendChild(document.createElement('section')); \
         var closedRoot = closedHost.attachShadow({ mode: 'closed' }); \
         var hidden = closedRoot.appendChild(document.createElement('p')); \
         var paths = {}; \
         var record = (name) => (e) => paths[name] = e.composedPath().map((t) => t.nodeName || 'window').join(','); \
         hidden.addEventListener('c', record('hidden')); closedHost.addEventListener('c', record('host')); \
         body.addEventListener('c', record('body')); \
         var ev = new Event('c', { bubbles: true, composed: true }); hidden.dispatchEvent(ev); 0",
    );
    check(
        &mut env,
        &[
            (
                "paths.hidden",
                "P,#document-fragment,SECTION,BODY,HTML,#document,window",
            ),
            ("paths.host", "SECTION,BODY,HTML,#document,window"),
            ("paths.body", "SECTION,BODY,HTML,#document,window"),
            ("ev.target === closedHost", "true"),
            // An open tree's path shows it all.
            (
                "var open = []; inner.addEventListener('o', (e) => open = e.composedPath().map((t) => t.nodeName || 'window')); \
                 host.addEventListener('o', (e) => open.push('|', ...e.composedPath().map((t) => t.nodeName || 'window'))); \
                 inner.dispatchEvent(new Event('o', { bubbles: true, composed: true })); open.join()",
                "SPAN,#document-fragment,DIV,BODY,HTML,#document,window,|,SPAN,#document-fragment,DIV,BODY,HTML,#document,window",
            ),
        ],
    );
}

/// The related target is retargeted against each entry; a host the
/// retargeted related target is ends the path; what is left after the
/// dispatch is what the last entry saw.
#[test]
fn related_targets_are_retargeted() {
    let mut env = page();
    env.eval(
        "var other = root.appendChild(document.createElement('i')); other.id = 'other'; \
         var seen = []; \
         var see = (name) => (e) => seen.push(name + ':' + (e.relatedTarget && (e.relatedTarget.id || e.relatedTarget.nodeName))); \
         inner.addEventListener('mouseover', see('inner')); root.addEventListener('mouseover', see('root')); \
         host.addEventListener('mouseover', see('host')); body.addEventListener('mouseover', see('body')); \
         var over = (related) => { var e = document.createEvent('MouseEvent'); \
           e.initMouseEvent('mouseover', true, true, null, 0, 0, 0, 0, 0, false, false, false, false, 0, related); return e; }; 0",
    );
    check(
        &mut env,
        &[
            // From one node of the tree to another: not past the host.
            (
                "seen = []; var e1 = over(other); inner.dispatchEvent(e1); [seen.join(' '), e1.relatedTarget, e1.target].join(' | ')",
                "inner:other root:other |  | ",
            ),
            // From outside to inside: outside, the host is the related target.
            (
                "seen = []; var e2 = over(inner); body.dispatchEvent(e2); [seen.join(' '), e2.relatedTarget === host].join(' | ')",
                "body:host | true",
            ),
            // The target its own retargeted related target: no dispatch.
            (
                "seen = []; host.dispatchEvent(over(inner)); seen.length",
                "0",
            ),
        ],
    );
}

/// A dispatch through nested shadow trees retargets its related target and
/// its target once a tree scope its path goes through, not once a node
/// (what DOM's retarget reads of a node is its tree scope, Blink's
/// `EventPath::BuildRelatedNodeMap`): 5 shadow trees each 20 deep, about
/// a hundred nodes, a dozen retargets; and the related target each
/// listener sees is the one DOM says.
#[test]
fn a_path_through_shadow_trees_retargets_once_a_tree() {
    let mut env = page();
    env.eval(
        "var build = (root, depth) => { var node = root; \
           for (var i = 0; i < 20; i++) node = node.appendChild(document.createElement('div')); \
           if (depth == 0) return node; \
           var host = node.appendChild(document.createElement('div')); host.id = 'h' + depth; \
           return build(host.attachShadow({ mode: 'open' }), depth - 1); }; \
         var outer = document.body.appendChild(document.createElement('div')); \
         var leaf = build(outer, 5); \
         var other = outer.appendChild(document.createElement('i')); other.id = 'other'; \
         var seen = []; \
         outer.addEventListener('mousemove', (e) => seen.push('outer:' + e.relatedTarget.id)); \
         leaf.addEventListener('mousemove', (e) => seen.push('leaf:' + e.relatedTarget.id)); 0",
    );
    let made = env.eval("typeof leaf");
    assert_eq!(made, "object");
    let before = dom::retargets_for_testing().expect("a debug build");
    let seen = env.eval(
        "leaf.dispatchEvent(new MouseEvent('mousemove', { relatedTarget: other, composed: true, bubbles: true })); seen.join(' ')",
    );
    let retargets = dom::retargets_for_testing().unwrap() - before;
    assert_eq!(seen, "leaf:other outer:other");
    assert!(retargets <= 30, "{retargets} retargets");
}

/// A window whose document's `host` has `<span id=a slot=a>`, `<b id=b>`,
/// a text and a comment, and an open shadow root `root` with
/// `<slot id=sa name=a>` and `<slot id=sd>`; `ids` names a list's nodes.
/// What each listener sees, and what the dispatch leaves, are as the
/// path was made, whatever a listener moves meanwhile: a target moved out
/// of its shadow tree is still cleared at the end, one moved into one
/// isn't, `window.event` is still set at it, and a related target moved
/// out of its shadow tree is still its host to the listeners after.
#[test]
fn a_dispatch_sees_its_path_as_it_was_made() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "var out = root.appendChild(document.createElement('i')); var seen = []; \
                 out.addEventListener('x', () => host.append(out)); \
                 out.addEventListener('x', e => seen.push(window.event === e, e.target === out)); \
                 var e1 = new MouseEvent('x', { relatedTarget: host }); out.dispatchEvent(e1); \
                 [seen.join(), e1.target, e1.relatedTarget].join('|')",
                "false,true||",
            ),
            (
                "var into = host.appendChild(document.createElement('b')); seen = []; \
                 into.addEventListener('x', () => root.append(into)); \
                 into.addEventListener('x', e => seen.push(window.event === e)); \
                 var e2 = new MouseEvent('x', { relatedTarget: host }); into.dispatchEvent(e2); \
                 [seen.join(), e2.target === into, e2.relatedTarget === host].join('|')",
                "true|true|true",
            ),
            (
                "var away = root.appendChild(document.createElement('u')); seen = []; \
                 body.addEventListener('y', () => body.append(away), { once: true }); \
                 html.addEventListener('y', e => seen.push(e.relatedTarget === host), { once: true }); \
                 var e3 = new FocusEvent('y', { relatedTarget: away, bubbles: true }); body.dispatchEvent(e3); \
                 [seen.join(), e3.target === body, e3.relatedTarget === host].join('|')",
                "true|true|true",
            ),
        ],
    );
}

fn slotted() -> Env {
    let mut env = Env::window("about:blank");
    env.eval(
        "var html = document.documentElement || document.appendChild(document.createElement('html')); \
         var body = html.appendChild(document.createElement('body')); \
         var host = body.appendChild(document.createElement('div')); \
         var a = host.appendChild(document.createElement('span')); a.id = 'a'; a.slot = 'a'; \
         var b = host.appendChild(document.createElement('b')); b.id = 'b'; \
         var t = host.appendChild(document.createTextNode('t')); \
         host.insertAdjacentHTML('beforeend', '<!--c-->'); var c = host.lastChild; \
         var root = host.attachShadow({ mode: 'open' }); \
         var sa = root.appendChild(document.createElement('slot')); sa.id = 'sa'; sa.name = 'a'; \
         var sd = root.appendChild(document.createElement('slot')); sd.id = 'sd'; \
         var ids = (list) => list.map((n) => n.id || n.nodeName).join(); 0",
    );
    env
}

#[test]
fn slots_get_the_hosts_children_by_name() {
    let mut env = slotted();
    check(
        &mut env,
        &[
            (
                "[ids(sa.assignedNodes()), ids(sd.assignedNodes()), ids(sd.assignedElements()), sa.name, a.slot, \
                  sa instanceof HTMLSlotElement].join('|')",
                "a|b,#text|b|a|a|true",
            ),
            (
                "[a.assignedSlot === sa, b.assignedSlot === sd, t.assignedSlot === sd, c.assignedSlot === undefined, \
                  sa.assignedSlot, root.firstChild.assignedSlot].join()",
                "true,true,true,true,,",
            ),
            // A slottable renamed, then a slot.
            (
                "b.slot = 'a'; [ids(sa.assignedNodes()), ids(sd.assignedNodes())].join('|')",
                "a,b|#text",
            ),
            (
                "sa.name = 'x'; [ids(sa.assignedNodes()), ids(sd.assignedNodes()), String(a.assignedSlot)].join('|')",
                "|#text|null",
            ),
            // The first slot of a name in tree order; out of the tree, none.
            (
                "var first = root.insertBefore(document.createElement('slot'), sa); first.name = 'a'; \
                 var second = root.appendChild(document.createElement('slot')); second.setAttribute('name', 'a'); \
                 [ids(first.assignedNodes()), second.assignedNodes().length, a.assignedSlot === first].join('|')",
                "a,b|0|true",
            ),
            (
                "root.removeChild(first); [first.assignedNodes().length, a.assignedSlot === second, ids(second.assignedNodes())].join('|')",
                "0|true|a,b",
            ),
            // Moved out of the host: no slot's.
            (
                "body.appendChild(t); [String(t.assignedSlot), sd.assignedNodes().length].join()",
                "null,0",
            ),
            ("document.createElement('slot').assignedNodes().length", "0"),
            // A closed tree's slot is hidden from `assignedSlot`.
            (
                "var ch = body.appendChild(document.createElement('div')); var cc = ch.appendChild(document.createElement('i')); \
                 var cs = ch.attachShadow({ mode: 'closed' }).appendChild(document.createElement('slot')); \
                 [String(cc.assignedSlot), cs.assignedNodes()[0] === cc].join()",
                "null,true",
            ),
        ],
    );
}

#[test]
fn flattened_slottables_go_through_slots_and_fallbacks() {
    let mut env = slotted();
    check(
        &mut env,
        &[
            (
                "var outer = body.appendChild(document.createElement('div')); var x = outer.appendChild(document.createElement('p')); x.id = 'x'; \
                 var inner = outer.attachShadow({ mode: 'open' }).appendChild(document.createElement('div')); \
                 var s1 = inner.appendChild(document.createElement('slot')); s1.id = 's1'; \
                 var innerRoot = inner.attachShadow({ mode: 'open' }); var s2 = innerRoot.appendChild(document.createElement('slot')); \
                 [ids(s2.assignedNodes()), ids(s2.assignedNodes({ flatten: true })), ids(s1.assignedNodes({ flatten: true }))].join('|')",
                "s1|x|x",
            ),
            // No assigned nodes: its slottable children instead.
            (
                "var fallback = innerRoot.appendChild(document.createElement('slot')); fallback.name = 'none'; \
                 var f = fallback.appendChild(document.createElement('em')); f.id = 'f'; fallback.insertAdjacentHTML('beforeend', '<!--x-->'); \
                 [ids(fallback.assignedNodes()), ids(fallback.assignedNodes({ flatten: true })), \
                  ids(fallback.assignedElements({ flatten: true }))].join('|')",
                "|f|f",
            ),
            // A slot in the fallback: its own (its fallback here).
            (
                "var s3 = fallback.appendChild(document.createElement('slot')); s3.name = 'none2'; \
                 var g = s3.appendChild(document.createElement('em')); g.id = 'g'; ids(fallback.assignedNodes({ flatten: true }))",
                "f,g",
            ),
            // Moved into another tree: that host's children (none).
            (
                "fallback.insertBefore(s1, s3); ids(fallback.assignedNodes({ flatten: true }))",
                "f,g",
            ),
        ],
    );
}

/// After `mutation`, at the microtask checkpoint: the `slotchange`s `log`
/// has.
fn slot_changes(env: &mut Env, cases: &[(&str, &str)]) {
    for (mutation, expected) in cases {
        env.eval(&format!("log = []; {mutation}; 0"));
        env.checkpoint();
        assert_eq!(env.eval("log.join()"), *expected, "{mutation}");
    }
}

#[test]
fn slotchange_fires_at_the_slots_whose_nodes_change() {
    let mut env = Env::window("about:blank");
    env.eval(
        "var html = document.documentElement || document.appendChild(document.createElement('html')); \
         var body = html.appendChild(document.createElement('body')); \
         var host = body.appendChild(document.createElement('div')); \
         var a = host.appendChild(document.createElement('span')); a.slot = 'a'; \
         var root = host.attachShadow({ mode: 'open' }); var log = []; \
         root.addEventListener('slotchange', (e) => log.push(e.target.id + (e.bubbles ? '' : '!') + (e.composed ? '+' : ''))); \
         host.addEventListener('slotchange', () => log.push('host')); 0",
    );
    slot_changes(
        &mut env,
        &[
            // A slot that gets nodes; one that gets none.
            (
                "var sa = document.createElement('slot'); sa.id = 'sa'; sa.name = 'a'; root.appendChild(sa); \
                 var sd = document.createElement('slot'); sd.id = 'sd'; root.appendChild(sd)",
                "sa",
            ),
            // Once a microtask, however many changes.
            (
                "host.appendChild(document.createElement('b')); host.appendChild(document.createTextNode('t'))",
                "sd",
            ),
            // Its old slot and its new.
            ("a.slot = ''", "sa,sd"),
            ("a.slot = ''", ""),
            ("host.removeChild(host.lastChild)", "sd"),
            ("host.insertAdjacentHTML('beforeend', '<!--c-->')", ""),
            // A slot's fallback content, while it has no assigned nodes.
            (
                "var sz = root.appendChild(document.createElement('slot')); sz.name = 'z'; sz.id = 'sz'",
                "",
            ),
            ("sz.appendChild(document.createElement('i'))", "sz"),
            ("sd.appendChild(document.createElement('i'))", ""),
            // A slot renamed: its nodes and the others'.
            ("sz.name = ''", ""),
            ("sd.name = 'q'", "sd,sz"),
            // A slot that goes with nodes: at it, out of the tree.
            (
                "sz.addEventListener('slotchange', () => log.push('gone')); root.removeChild(sz)",
                "gone",
            ),
            ("root.removeChild(sa)", ""),
        ],
    );
}

#[test]
fn manual_slots_get_the_nodes_assigned() {
    let mut env = slotted();
    env.eval(
        "var mh = body.appendChild(document.createElement('div')); \
         var m1 = mh.appendChild(document.createElement('i')); m1.id = 'm1'; \
         var m2 = mh.appendChild(document.createTextNode('m2')); \
         var m3 = body.appendChild(document.createElement('u')); m3.id = 'm3'; \
         var mr = mh.attachShadow({ mode: 'open', slotAssignment: 'manual' }); \
         var ms = mr.appendChild(document.createElement('slot')); ms.id = 'ms'; \
         var mt = mr.appendChild(document.createElement('slot')); mt.id = 'mt'; \
         var log = []; mr.addEventListener('slotchange', (e) => log.push(e.target.id)); 0",
    );
    check(
        &mut env,
        &[
            // None by name.
            (
                "[ms.assignedNodes().length, mt.assignedNodes().length, String(m1.assignedSlot)].join()",
                "0,0,null",
            ),
            // In the order given; one not the host's child left out until it is.
            (
                "ms.assign(m2, m3, m1, m2); [ids(ms.assignedNodes()), m1.assignedSlot === ms, String(m3.assignedSlot)].join('|')",
                "#text,m1|true|null",
            ),
            ("mh.appendChild(m3); ids(ms.assignedNodes())", "#text,m3,m1"),
            // Assigned to another: no longer the first's.
            (
                "mt.assign(m1); [ids(ms.assignedNodes()), ids(mt.assignedNodes())].join('|')",
                "#text,m3|m1",
            ),
            (
                "ms.assign(); [ms.assignedNodes().length, String(m2.assignedSlot)].join()",
                "0,null",
            ),
            (
                "try { ms.assign(document); 'assigned' } catch (e) { e instanceof TypeError }",
                "true",
            ),
            (
                "try { ms.assign(null); 'assigned' } catch (e) { e instanceof TypeError }",
                "true",
            ),
        ],
    );
    env.checkpoint();
    env.eval("log = []; ms.assign(m2); 0");
    env.checkpoint();
    assert_eq!(env.eval("log.join()"), "ms");
}

/// A slotted node's events go through its slot, into the shadow tree and
/// out through the host, the target the node throughout.
#[test]
fn events_go_through_slots() {
    let mut env = slotted();
    env.eval(
        "var log = []; var note = (name) => (e) => log.push(name + ':' + e.eventPhase + ':' + (e.target.id || e.target.nodeName)); \
         for (var [name, at] of [['a', a], ['sa', sa], ['root', root], ['host', host], ['body', body]]) at.addEventListener('e', note(name)); \
         var ch = body.appendChild(document.createElement('div')); var cc = ch.appendChild(document.createElement('i')); \
         var cs = ch.attachShadow({ mode: 'closed' }).appendChild(document.createElement('slot')); 0",
    );
    check(
        &mut env,
        &[
            (
                "log = []; var ev = new Event('e', { bubbles: true }); a.dispatchEvent(ev); \
                 [log.join(' '), ev.composedPath().length, ev.target === a].join(' | ')",
                "a:2:a sa:3:a root:3:a host:3:a body:3:a | 0 | true",
            ),
            (
                "var path = []; sa.addEventListener('p', (e) => path.push(e.composedPath().map((t) => t.id || t.nodeName || 'window').join())); \
                 a.dispatchEvent(new Event('p', { bubbles: true })); path.join()",
                "a,sa,#document-fragment,DIV,BODY,HTML,#document,window",
            ),
            // Through a slot: its listener's invocation target is in a
            // shadow tree (no `window.event`), the host's isn't.
            (
                "var seen = []; var ew = new Event('w', { bubbles: true }); \
                 sa.addEventListener('w', () => seen.push(window.event === undefined)); \
                 host.addEventListener('w', () => seen.push(window.event === ew)); \
                 a.dispatchEvent(ew); seen.join()",
                "true,true",
            ),
            // A closed tree's slot and root hidden from those outside.
            (
                "var hidden = []; cc.addEventListener('h', (e) => hidden.push(e.composedPath().map((t) => t.nodeName || 'window').join())); \
                 ch.addEventListener('h', (e) => hidden.push(e.composedPath().map((t) => t.nodeName || 'window').join())); \
                 cs.addEventListener('h', (e) => hidden.push(e.composedPath().map((t) => t.nodeName || 'window').join())); \
                 cc.dispatchEvent(new Event('h', { bubbles: true })); hidden.join(' | ')",
                "I,DIV,BODY,HTML,#document,window | I,SLOT,#document-fragment,DIV,BODY,HTML,#document,window | I,DIV,BODY,HTML,#document,window",
            ),
            // A closed slot's fallback content: its slot is its parent, not
            // a slot it is assigned to.
            (
                "var shown = []; var fb = cs.appendChild(document.createElement('em')); \
                 ch.addEventListener('f', (e) => shown.push(e.composedPath().map((t) => t.nodeName || 'window').join())); \
                 fb.dispatchEvent(new Event('f', { bubbles: true, composed: true })); shown.join()",
                "DIV,BODY,HTML,#document,window",
            ),
            // A slotted node in a closed tree, its own closed tree before
            // it: each sees as deep as it is, the slot of an open tree
            // shown, a closed tree's slot leading out of what is before it.
            (
                "var trees = (mode) => { var outer = body.appendChild(document.createElement('div')); \
                   var outerRoot = outer.attachShadow({ mode: 'closed' }); var host = outerRoot.appendChild(document.createElement('section')); \
                   var slotted = host.appendChild(document.createElement('p')); var root = host.attachShadow({ mode }); \
                   root.appendChild(document.createElement('slot')); \
                   var deep = slotted.attachShadow({ mode: 'closed' }).appendChild(document.createElement('em')); \
                   var seen = []; var see = (e) => seen.push(e.composedPath().map((t) => t.nodeName || 'window').join()); \
                   for (var at of [deep, slotted, root, outerRoot, outer]) at.addEventListener('d', see); \
                   deep.dispatchEvent(new Event('d', { bubbles: true, composed: true })); return seen.join(' | ') }; \
                 [trees('open'), trees('closed')].join(' || ')",
                "EM,#document-fragment,P,SLOT,#document-fragment,SECTION,#document-fragment,DIV,BODY,HTML,#document,window | \
                 P,SLOT,#document-fragment,SECTION,#document-fragment,DIV,BODY,HTML,#document,window | \
                 P,SLOT,#document-fragment,SECTION,#document-fragment,DIV,BODY,HTML,#document,window | \
                 P,SLOT,#document-fragment,SECTION,#document-fragment,DIV,BODY,HTML,#document,window | \
                 DIV,BODY,HTML,#document,window || \
                 EM,#document-fragment,P,SECTION,#document-fragment,DIV,BODY,HTML,#document,window | \
                 P,SECTION,#document-fragment,DIV,BODY,HTML,#document,window | \
                 P,SLOT,#document-fragment,SECTION,#document-fragment,DIV,BODY,HTML,#document,window | \
                 P,SECTION,#document-fragment,DIV,BODY,HTML,#document,window | \
                 DIV,BODY,HTML,#document,window",
            ),
        ],
    );
}

#[test]
fn a_templates_shadow_root_attributes_reflect() {
    let mut env = Env::window("about:blank");
    let cases = [
        (
            "var t = document.createElement('template'); [JSON.stringify(t.shadowRootMode), t.shadowRootClonable].join()",
            "\"\",false",
        ),
        (
            "t.setAttribute('shadowrootmode', 'OPEN'); t.shadowRootMode",
            "open",
        ),
        (
            "t.shadowRootMode = 'closed'; [t.shadowRootMode, t.getAttribute('shadowrootmode')].join()",
            "closed,closed",
        ),
        (
            "t.shadowRootMode = 'bogus'; [JSON.stringify(t.shadowRootMode), t.getAttribute('shadowrootmode')].join()",
            "\"\",bogus",
        ),
        (
            "t.shadowRootDelegatesFocus = true; t.shadowRootClonable = true; t.shadowRootSerializable = true; \
             [t.hasAttribute('shadowrootdelegatesfocus'), t.getAttribute('shadowrootclonable'), \
              t.shadowRootSerializable].join()",
            "true,,true",
        ),
        (
            "t.shadowRootDelegatesFocus = false; t.shadowRootClonable = false; t.shadowRootSerializable = false; \
             [t.hasAttribute('shadowrootdelegatesfocus'), t.hasAttribute('shadowrootclonable'), \
              t.hasAttribute('shadowrootserializable')].join()",
            "false,false,false",
        ),
    ];
    for (script, expected) in cases {
        assert_eq!(env.eval(script), expected, "{script}");
    }
}

#[test]
fn set_html_unsafe_and_get_html_carry_declarative_shadow_roots() {
    let mut env = Env::window("about:blank");
    let cases = [
        (
            "var d = document.createElement('div'); \
             d.setHTMLUnsafe('<p id=h><template shadowrootmode=open shadowrootserializable><b>s</b></template>l</p>'); \
             var h = d.firstChild; [h.shadowRoot.mode, h.shadowRoot.innerHTML, h.innerHTML].join('|')",
            "open|<b>s</b>|l",
        ),
        // `innerHTML` keeps them templates.
        (
            "var e = document.createElement('div'); \
             e.innerHTML = '<p><template shadowrootmode=open></template></p>'; \
             [e.firstChild.shadowRoot, e.firstChild.firstChild.nodeName].join()",
            ",TEMPLATE",
        ),
        // Not written by default; serializable ones with the option; listed
        // ones whatever they are; the host's own too.
        ("d.getHTML()", "<p id=\"h\">l</p>"),
        (
            "d.getHTML({ serializableShadowRoots: true })",
            "<p id=\"h\"><template shadowrootmode=\"open\" shadowrootserializable=\"\"><b>s</b></template>l</p>",
        ),
        (
            "var c = document.createElement('span'); var closedRoot = c.attachShadow({ mode: 'closed', delegatesFocus: true, clonable: true }); \
             closedRoot.setHTMLUnsafe('<i>c</i>'); d.append(c); \
             [d.getHTML({ serializableShadowRoots: true }).endsWith('<span></span>'), \
              d.getHTML({ shadowRoots: [closedRoot] }).endsWith( \
                '<span><template shadowrootmode=\"closed\" shadowrootdelegatesfocus=\"\" shadowrootclonable=\"\"><i>c</i></template></span>')].join()",
            "true,true",
        ),
        // Each setting in HTML's order.
        (
            "var all = document.createElement('span'); \
             all.attachShadow({ mode: 'open', delegatesFocus: true, clonable: true, serializable: true }); \
             var box = document.createElement('div'); box.append(all); box.getHTML({ serializableShadowRoots: true })",
            "<span><template shadowrootmode=\"open\" shadowrootdelegatesfocus=\"\" shadowrootserializable=\"\" shadowrootclonable=\"\"></template></span>",
        ),
        (
            "h.getHTML({ serializableShadowRoots: true })",
            "<template shadowrootmode=\"open\" shadowrootserializable=\"\"><b>s</b></template>l",
        ),
        ("h.shadowRoot.getHTML()", "<b>s</b>"),
        (
            "var error; try { d.getHTML({ shadowRoots: [d] }); } catch (x) { error = x.name; } \
             var error2; try { d.getHTML({ shadowRoots: [1] }); } catch (x) { error2 = x.name; } \
             var error3; try { d.getHTML({ shadowRoots: [{}] }); } catch (x) { error3 = x.name; } [error, error2, error3].join()",
            "TypeError,TypeError,TypeError",
        ),
        // A template's contents.
        (
            "var t = document.createElement('template'); t.setHTMLUnsafe('<u>x</u>'); [t.childNodes.length, t.content.firstChild.nodeName].join()",
            "0,U",
        ),
    ];
    for (script, expected) in cases {
        assert_eq!(env.eval(script), expected, "{script}");
    }
}

#[test]
fn parse_html_unsafe_makes_a_document() {
    let mut env = Env::window("about:blank");
    let cases = [
        (
            "var doc = Document.parseHTMLUnsafe('<!DOCTYPE html><title>t</title><div id=h><template shadowrootmode=open><b></b></template></div><script>window.ran = 1</script>'); \
             [doc instanceof Document, doc !== document, doc.doctype.name, doc.head.firstChild.nodeName, \
              doc.getElementById('h').shadowRoot.firstChild.nodeName, typeof window.ran].join()",
            "true,true,html,TITLE,B,undefined",
        ),
        (
            "Object.getOwnPropertyDescriptor(Document.prototype, 'parseHTMLUnsafe') === undefined && Document.parseHTMLUnsafe.length",
            "1",
        ),
        ("Document.parseHTMLUnsafe('x').body.textContent", "x"),
    ];
    for (script, expected) in cases {
        assert_eq!(env.eval(script), expected, "{script}");
    }
}
