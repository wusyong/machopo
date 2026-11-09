//! `URL` and `URLSearchParams` (their parts and setters, the params
//! following the query and the query the params, the pair iterator),
//! `crypto` and `structuredClone`.

use crate::history::Page;

#[test]
fn urls_and_their_parts() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var u = new URL('b/c?x=1#h', 'https://user:pw@example.test:8080/a/'); \
             [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash, String(u), JSON.stringify(u)].join(' ')"
        ),
        "https://user:pw@example.test:8080/a/b/c?x=1#h https://example.test:8080 https: user pw example.test:8080 example.test 8080 /a/b/c ?x=1 #h \
         https://user:pw@example.test:8080/a/b/c?x=1#h \"https://user:pw@example.test:8080/a/b/c?x=1#h\""
    );
    assert_eq!(
        page.eval(
            "var r = []; try { new URL('nope'); } catch (e) { r.push(e.name); } try { new URL('a', 'nope'); } catch (e) { r.push(e.name); } \
             r.push(URL.canParse('nope'), URL.canParse('a', 'https://x.test/'), URL.parse('nope'), URL.parse('/p', 'https://x.test/q').href); \
             u.protocol = 'http'; u.port = '80'; u.hostname = 'other.test'; u.pathname = 'd'; u.hash = ''; r.push(u.href); \
             u.port = 'nope'; r.push(u.port); try { u.href = 'nope'; } catch (e) { r.push(e.name); } r.join()"
        ),
        "TypeError,TypeError,false,true,,https://x.test/p,http://user:pw@other.test/d?x=1,,TypeError"
    );
}

#[test]
fn search_params_follow_the_query() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var u = new URL('https://x.test/?a=1&b=2&a=3'); var p = u.searchParams; \
             var r = [p === u.searchParams, p.size, p.get('a'), p.getAll('a').join('|'), p.has('b'), p.has('a', '3'), p.has('a', '4'), p.get('z')]; \
             p.append('c', 'x y'); r.push(u.search); p.delete('a', '1'); r.push(u.search); p.append('a', '4'); p.set('a', '9'); p.sort(); r.push(u.search, String(p)); \
             u.search = '?q=1'; r.push(p.get('q'), p.size); u.href = 'https://x.test/'; r.push(p.size); p.append('k', 'v'); r.push(u.href); \
             p.delete('k'); r.push(u.href); r.join()"
        ),
        "true,3,1,1|3,true,true,false,,?a=1&b=2&a=3&c=x+y,?b=2&a=3&c=x+y,?a=9&b=2&c=x+y,a=9&b=2&c=x+y,1,1,0,https://x.test/?k=v,https://x.test/"
    );
    // The constructor's three forms; a pair that isn't two strings.
    assert_eq!(
        page.eval(
            "var r = [new URLSearchParams('?a=1&b').toString(), new URLSearchParams([['x', '1'], ['y', 2]]).toString(), \
             new URLSearchParams({ m: 'n', o: 'p' }).toString(), new URLSearchParams().size, new URLSearchParams(undefined).size, \
             new URLSearchParams(null).toString(), new URLSearchParams({ '\\uD835x': '1', xx: '2', '\\uD83Dx': '3' }).toString()]; \
             try { new URLSearchParams([['x']]); } catch (e) { r.push(e.name); } r.join()"
        ),
        "a=1&b=,x=1&y=2,m=n&o=p,0,0,null=,%EF%BF%BDx=3&xx=2,TypeError"
    );
}

#[test]
fn search_params_iterate_live() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var p = new URLSearchParams('a=1&b=2'); var r = [[...p].join(';'), [...p.keys()].join(), [...p.values()].join(), [...p.entries()].join(';')]; \
             var it = p[Symbol.iterator](); r.push(Object.prototype.toString.call(it), Object.getPrototypeOf(Object.getPrototypeOf(it)) === Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))); \
             r.push(JSON.stringify(it.next())); p.append('c', '3'); p.delete('a'); r.push(JSON.stringify(it.next()), JSON.stringify(it.next())); \
             var seen = []; p.forEach(function (v, k, o) { seen.push(k + v, o === p, this.t); if (k === 'b') p.append('d', '4'); }, { t: 'x' }); r.push(seen.join()); \
             r.push(p[Symbol.iterator] === p.entries, typeof p.iterableEntry); try { p.entries.call({}); } catch (e) { r.push(e.name); } r.join(' ')"
        ),
        "a,1;b,2 a,b 1,2 a,1;b,2 [object URLSearchParams Iterator] true {\"value\":[\"a\",\"1\"],\"done\":false} \
         {\"value\":[\"c\",\"3\"],\"done\":false} {\"done\":true} b2,true,x,c3,true,x,d4,true,x true undefined TypeError"
    );
}

#[test]
fn crypto_and_structured_clone() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var a = new Uint32Array(8); var r = [crypto === crypto, crypto.getRandomValues(a) === a, a.some(x => x !== 0)]; \
             var b = new Uint8Array(new ArrayBuffer(8), 2, 4); crypto.getRandomValues(b); r.push(new Uint8Array(b.buffer).slice(0, 2).join(), new Uint8Array(b.buffer).slice(6).join()); \
             for (var bad of [new Float32Array(1), new DataView(new ArrayBuffer(1)), new Uint8Array(65537)]) { try { crypto.getRandomValues(bad); } catch (e) { r.push(e.name); } } \
             try { crypto.getRandomValues([]); } catch (e) { r.push(e.name); } \
             r.push(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(crypto.randomUUID()), crypto.randomUUID() !== crypto.randomUUID()); r.join()"
        ),
        "true,true,true,0,0,0,0,TypeMismatchError,TypeMismatchError,QuotaExceededError,TypeError,true,true"
    );
    assert_eq!(
        page.eval(
            "var o = { a: [1, { b: 'c' }], d: new Date(0), m: new Map([[1, 2]]) }; var c = structuredClone(o); \
             var r = [c !== o, JSON.stringify(c.a), c.d.getTime(), c.m.get(1)]; \
             var buf = new ArrayBuffer(4); var moved = structuredClone(buf, { transfer: [buf] }); r.push(buf.byteLength, moved.byteLength); \
             var ch = new MessageChannel(); var port = structuredClone(ch.port1, { transfer: [ch.port1] }); r.push(port instanceof MessagePort, port !== ch.port1); \
             try { structuredClone(function () {}); } catch (e) { r.push(e.name); } try { structuredClone(ch.port2); } catch (e) { r.push(e.name); } r.join()"
        ),
        "true,[1,{\"b\":\"c\"}],0,2,0,4,true,true,DataCloneError,DataCloneError"
    );
    // In `this`'s realm: another window's method called on this one.
    assert_eq!(
        page.eval(
            "var w = document.body.appendChild(document.createElement('iframe')).contentWindow; \
             [w.structuredClone.call(window, new w.Array()) instanceof Array, structuredClone.call(w, {}) instanceof w.Object].join()"
        ),
        "true,true"
    );
}
