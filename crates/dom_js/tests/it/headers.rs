//! `Headers`: its init (a sequence of pairs, a record; not a string),
//! `append` (a name keeps its first case), `get` (the values joined),
//! `getSetCookie`, `has`, `set`, `delete`, the pair iterator (sorted,
//! combined, `set-cookie` once per value), the errors (an invalid name or
//! value, a `ByteString` past U+00FF, a pair not of two), and a record's
//! conversion in WebIDL's order (each key's descriptor, then its value; a
//! symbol key's a `TypeError` only if it is enumerable).

use crate::history::Page;

#[test]
fn a_header_list() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var h = new Headers([['Accept', 'a'], ['X-B', ' 1 ']]); h.append('accept', 'b'); h.append('Set-Cookie', 'c=1'); h.append('set-cookie', 'd=2');
             var r = [h instanceof Headers, h.get('ACCEPT'), h.get('x-b'), String(h.get('nope')), h.has('X-b'), h.has('nope'), h.getSetCookie().join(';'), h.get('set-cookie')];
             h.set('accept', 'c'); h.delete('x-b'); r.push(h.get('accept'), h.has('x-b'));
             r.push([...h].map(p => p.join('=')).join(' '), [...h.keys()].join(), [...h.values()].join('|'));
             var seen = []; h.forEach((value, name, self) => seen.push(name + (self === h))); r.push(seen.join());
             r.push([...new Headers({ b: '2', A: '1' })].join(' '), [...new Headers()].length, [...new Headers(undefined)].length);
             r.join(' ; ')"
        ),
        "true ; a, b ; 1 ; null ; true ; false ; c=1;d=2 ; c=1, d=2 ; c ; false ; accept=c set-cookie=c=1 set-cookie=d=2 ; accept,set-cookie,set-cookie ; c|c=1|d=2 ; accepttrue,set-cookietrue,set-cookietrue ; a,1 b,2 ; 0 ; 0"
    );
    assert_eq!(
        page.eval(
            "var r = []; var tries = [() => new Headers('a=b'), () => new Headers([['a']]), () => new Headers([['a', 'b', 'c']]), () => new Headers({ 'bad name': 'x' }),
             () => new Headers().append('a', 'x\\ny'), () => new Headers().append('a', '\\u0100'), () => new Headers().get('bad name'), () => new Headers().has(''),
             () => new Headers().set('ok', 'fine'), () => new Headers().delete('a b'), () => new Headers([['a', '\\u00ff']]).get('a').length];
             for (var t of tries) { try { var v = t(); r.push(v === undefined ? 'ok' : v); } catch (e) { r.push(e.name); } } r.join()"
        ),
        "TypeError,TypeError,TypeError,TypeError,TypeError,TypeError,TypeError,TypeError,ok,TypeError,1"
    );
}

#[test]
fn a_record_converts_in_webidls_order() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var log = []; var p = new Proxy({ a: '1', b: '2' }, {
                 ownKeys(t) { log.push('keys'); return Reflect.ownKeys(t); },
                 getOwnPropertyDescriptor(t, k) { log.push('desc ' + k); return Reflect.getOwnPropertyDescriptor(t, k); },
                 get(t, k) { log.push('get ' + String(k)); return Reflect.get(t, k); } });
             var h = new Headers(p); var r = [log.join(), [...h].join(' ')];
             var hidden = {}; Object.defineProperty(hidden, Symbol('s'), { value: 'x', enumerable: false }); Object.defineProperty(hidden, 'n', { value: 'y', enumerable: false });
             r.push([...new Headers(hidden)].length);
             try { new Headers({ [Symbol('s')]: 'x' }); } catch (e) { r.push(e.name); } r.join(' ; ')"
        ),
        "get Symbol(Symbol.iterator),keys,desc a,get a,desc b,get b ; a,1 b,2 ; 0 ; TypeError",
        "the union looks for a sequence first"
    );
}
