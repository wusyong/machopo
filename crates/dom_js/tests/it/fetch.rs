//! Fetch: `Request` (its constructor's steps: the URL, the init's members
//! and their errors, a body moved from its input, the signal followed),
//! `Response` (its constructor, `error()`, `redirect()`, `json()`), the
//! body mixin (each reading once; a `Content-Type` of the body's), and
//! `fetch()`: its request the engine's frame request once the script is
//! done, its response given back by its number (a basic, CORS, opaque or
//! opaque-redirect `Response`, or a network error), and its abort (the
//! promise rejected with the reason, the request cancelled; a body after
//! it unreadable).

use std::sync::Arc;

use dom_api::{FetchedResponse, FrameRequest, ScriptEngine};
use protocol::network::ResponseTainting;

use crate::history::Page;

fn fetches(page: &mut Page) -> Vec<FrameRequest> {
    page.engine
        .take_frame_requests()
        .into_iter()
        .filter(|request| {
            matches!(
                request,
                FrameRequest::Fetch { .. } | FrameRequest::CancelFetch { .. }
            )
        })
        .collect()
}

fn response(
    tainting: ResponseTainting,
    status: u16,
    headers: &[(&str, &str)],
    body: &str,
) -> FetchedResponse {
    FetchedResponse {
        tainting,
        url: "https://example.test/data#frag".into(),
        redirected: true,
        status,
        status_text: "Fine".into(),
        headers: headers
            .iter()
            .map(|&(name, value)| (name.to_owned(), value.to_owned()))
            .collect(),
        body: Arc::from(body.as_bytes()),
    }
}

#[test]
fn a_request_and_its_init() {
    let mut page = Page::new("https://example.test/dir/page");
    assert_eq!(
        page.eval(
            "var r = new Request('a?x', { method: 'post', headers: { 'X-A': '1' }, body: 'hi', mode: 'same-origin', credentials: 'include', cache: 'no-store',
                 redirect: 'manual', integrity: 'sha', keepalive: true, referrer: '', referrerPolicy: 'origin', priority: 'high' });
             [r.url, r.method, r.headers.get('x-a'), r.headers.get('content-type'), r.mode, r.credentials, r.cache, r.redirect, r.integrity, r.keepalive,
              JSON.stringify(r.referrer), r.referrerPolicy, r.destination, r.duplex, r.signal instanceof AbortSignal, r.bodyUsed, r.isReloadNavigation].join()"
        ),
        "https://example.test/dir/a?x,POST,1,text/plain;charset=UTF-8,same-origin,include,no-store,manual,sha,true,\"\",origin,,half,true,false,false"
    );
    assert_eq!(
        page.eval(
            "var d = new Request('https://other.test/'); var r = [d.method, d.mode, d.credentials, d.referrer, d.headers.has('a')];
             var tries = [() => new Request('https://u:p@x.test/'), () => new Request('http://[bad'), () => new Request('x', { method: 'TRACE' }),
                 () => new Request('x', { method: 'bad method' }), () => new Request('x', { mode: 'navigate' }), () => new Request('x', { mode: 'nope' }),
                 () => new Request('x', { body: 'b' }), () => new Request('x', { cache: 'only-if-cached' }), () => new Request('x', { window: {} }),
                 () => new Request('x', { mode: 'no-cors', method: 'PUT' }), () => new Request('x', { signal: {} }), () => new Request('x', { window: null })];
             for (var t of tries) { try { t(); r.push('ok'); } catch (e) { r.push(e.name); } } r.join()"
        ),
        "GET,cors,same-origin,about:client,false,TypeError,TypeError,TypeError,TypeError,TypeError,TypeError,TypeError,TypeError,TypeError,TypeError,TypeError,ok"
    );
    assert_eq!(
        page.eval(
            "var c = new AbortController(); var a = new Request('x', { method: 'POST', body: 'moved', signal: c.signal });
             var b = new Request(a); var r = [a.bodyUsed, b.method, b.url.endsWith('/dir/x'), b.signal !== a.signal];
             c.abort('why'); r.push(b.signal.aborted, b.signal.reason);
             try { new Request(a); } catch (e) { r.push(e.name); }
             var u = new Request('x', { method: 'POST', body: new URLSearchParams('a=1&b=2') });
             r.push(u.headers.get('content-type'), new Request('x', { method: 'POST', body: new Blob(['z'], { type: 'text/z' }) }).headers.get('content-type'),
                 String(new Request('x', { method: 'POST', body: new Uint8Array([1]) }).headers.get('content-type')));
             var none = new Request('x', { signal: null }); r.push(none.signal.aborted); r.join()"
        ),
        "true,POST,true,true,true,why,TypeError,application/x-www-form-urlencoded;charset=UTF-8,text/z,null,false"
    );
}

#[test]
fn a_response_and_its_body() {
    let mut page = Page::new("https://example.test/dir/page");
    let setup = page.eval(
        "var log = []; var out = (p, name) => p.then(v => log.push(name + ' ' + (v instanceof Blob ? v.type + ':' + v.size : v instanceof ArrayBuffer ? 'buffer ' + v.byteLength : v instanceof Uint8Array ? 'view ' + v.length : JSON.stringify(v))), e => log.push(name + ' ' + e.name));
         var r = new Response('{\"a\": [1]}', { status: 201, statusText: 'Made', headers: [['X-B', '2']] });
         log.push([r.status, r.ok, r.statusText, r.type, JSON.stringify(r.url), r.redirected, r.headers.get('content-type'), r.headers.get('x-b')].join());
         var c = r.clone(); out(r.json(), 'json'); log.push(r.bodyUsed); out(r.text(), 'again'); out(c.text(), 'clone');
         out(new Response(new Uint8Array([1, 2])).arrayBuffer(), 'buffer'); out(new Response('abc').bytes(), 'bytes');
         out(new Response('x', { headers: { 'Content-Type': 'Text/Plain' } }).blob(), 'blob'); out(new Response().text(), 'null');
         out(new Response('not json').json(), 'bad'); out(new Response(new Uint8Array([0xEF, 0xBB, 0xBF, 0x61])).text(), 'bom'); 0",
    );
    assert_eq!(setup, "0");
    assert_eq!(
        page.eval("log.join(' | ')"),
        "201,true,Made,default,\"\",false,text/plain;charset=UTF-8,2 | true | json {\"a\":[1]} | again TypeError | clone \"{\\\"a\\\": [1]}\" | buffer buffer 2 | bytes view 3 | blob text/plain:1 | null \"\" | bad SyntaxError | bom \"a\""
    );
    assert_eq!(
        page.eval(
            "var e = Response.error(), d = Response.redirect('b', 307), j = Response.json({ x: 1 }, { status: 202 });
             var r = [e.type, e.status, d.status, d.headers.get('location'), j.status, j.headers.get('content-type')];
             var tries = [() => new Response('', { status: 100 }), () => new Response('b', { status: 204 }), () => new Response('', { statusText: 'a\\nb' }),
                 () => Response.redirect('b', 200), () => Response.json(undefined), () => e.headers.set('a', 'b'), () => new Response(null, { status: 204 })];
             for (var t of tries) { try { t(); r.push('ok'); } catch (e) { r.push(e.name); } } r.join()"
        ),
        "error,0,307,https://example.test/dir/b,202,application/json,RangeError,TypeError,TypeError,RangeError,TypeError,TypeError,ok"
    );
}

#[test]
fn fetch_goes_to_the_host_as_the_script_ends() {
    let mut page = Page::new("https://example.test/dir/page");
    page.eval(
        "var log = []; var done = p => p.then(r => log.push([r.type, r.status, r.statusText, r.url, r.redirected, r.headers.get('x-a'), String(r.headers.get('set-cookie'))].join() + ' ' + r.bodyUsed),
                                       e => log.push(e.name + ' ' + e.message));
         done(fetch('data', { method: 'POST', headers: [['X-A', '1'], ['x-a', '2']], body: 'hi', redirect: 'manual' })); 0",
    );
    let requests = fetches(&mut page);
    let [FrameRequest::Fetch { request, .. }] = requests.as_slice() else {
        panic!("{requests:?}");
    };
    assert_eq!(
        (
            request.url.as_str(),
            request.method.as_str(),
            request.headers.clone(),
            request.body.as_deref(),
            request.initiator.as_str()
        ),
        (
            "https://example.test/dir/data",
            "POST",
            vec![
                ("X-A".to_owned(), "1, 2".to_owned()),
                (
                    "Content-Type".to_owned(),
                    "text/plain;charset=UTF-8".to_owned()
                )
            ],
            Some(&b"hi"[..]),
            "https://example.test"
        )
    );
    let id = request.id;
    assert!(
        page.engine
            .fetch_response(id, Ok(response(ResponseTainting::Basic, 302, &[], "")))
    );
    assert!(!page.engine.fetch_response(id, Err(())), "answered once");
    // Each kind of response, and a network error.
    page.eval(
        "done(fetch('a')); done(fetch('b')); done(fetch('c')); done(fetch('d')); done(fetch('e')); 0",
    );
    let ids: Vec<u64> = fetches(&mut page)
        .into_iter()
        .map(|request| match request {
            FrameRequest::Fetch { request, .. } => request.id,
            other => panic!("{other:?}"),
        })
        .collect();
    let headers = [
        ("X-A", "1"),
        ("Set-Cookie", "c=1"),
        ("Content-Type", "text/plain"),
    ];
    page.engine.fetch_response(
        ids[0],
        Ok(response(ResponseTainting::Basic, 200, &headers, "body")),
    );
    page.engine.fetch_response(
        ids[1],
        Ok(response(ResponseTainting::Cors, 200, &headers, "")),
    );
    page.engine.fetch_response(
        ids[2],
        Ok(response(ResponseTainting::Opaque, 200, &headers, "x")),
    );
    page.engine.fetch_response(ids[3], Err(()));
    page.engine.fetch_response(
        ids[4],
        Ok(response(ResponseTainting::Basic, 204, &[], "dropped")),
    );
    assert_eq!(
        page.eval("log.join(' | ')"),
        "opaqueredirect,0,,https://example.test/data,false,,null false | basic,200,Fine,https://example.test/data,true,1,null false | \
         cors,200,Fine,https://example.test/data,true,,null false | opaque,0,,,false,,null false | TypeError Failed to fetch | basic,204,Fine,https://example.test/data,true,,null false"
    );
    // A null body status's body is none (read as empty, not used).
    page.eval("fetch('f').then(r => r.text().then(t => log.push('204 body ' + JSON.stringify(t) + ' ' + r.bodyUsed))); 0");
    let id = match fetches(&mut page).as_slice() {
        [FrameRequest::Fetch { request, .. }] => request.id,
        other => panic!("{other:?}"),
    };
    page.engine.fetch_response(
        id,
        Ok(response(ResponseTainting::Basic, 204, &[], "dropped")),
    );
    assert_eq!(page.eval("log[log.length - 1]"), "204 body \"\" false");
    assert_eq!(
        page.eval("var r = []; for (var u of ['file:///x', 'about:blank', 'javascript:1']) fetch(u).catch(e => r.push(e.name)); 0"),
        "0"
    );
    assert_eq!(
        page.eval("r.join()"),
        "TypeError,TypeError,TypeError",
        "rejected at once"
    );
    assert!(fetches(&mut page).is_empty(), "none goes");
}

#[test]
fn an_aborted_fetch() {
    let mut page = Page::new("https://example.test/");
    page.eval(
        "var log = []; var c = new AbortController();
         fetch('a', { signal: c.signal }).then(() => log.push('resolved'), e => log.push('rejected ' + e));
         fetch('b', { signal: AbortSignal.abort('early') }).catch(e => log.push('at once ' + e)); 0",
    );
    let requests = fetches(&mut page);
    let [FrameRequest::Fetch { request, .. }] = requests.as_slice() else {
        panic!("one goes: {requests:?}");
    };
    let id = request.id;
    page.eval("c.abort('why'); 0");
    assert_eq!(page.eval("log.join(' | ')"), "at once early | rejected why");
    assert_eq!(
        fetches(&mut page)
            .into_iter()
            .map(|request| format!("{request:?}"))
            .collect::<Vec<_>>(),
        [format!("{:?}", FrameRequest::CancelFetch { id })]
    );
    assert!(
        !page.engine.fetch_response(id, Err(())),
        "no longer in flight"
    );
    // Aborted once the response came: its body can't be read.
    page.eval(
        "var d = new AbortController(); var got; fetch('c', { signal: d.signal }).then(r => { got = r; }); 0",
    );
    let id = match fetches(&mut page).as_slice() {
        [FrameRequest::Fetch { request, .. }] => request.id,
        other => panic!("{other:?}"),
    };
    page.engine
        .fetch_response(id, Ok(response(ResponseTainting::Basic, 200, &[], "text")));
    page.eval("d.abort('late'); got.text().catch(e => log.push('body ' + e)); 0");
    assert_eq!(page.eval("log.slice(2).join()"), "body late");
    assert!(fetches(&mut page).is_empty(), "nothing to cancel");
}

/// A signal timing out (in its native timer's task) rejects the fetch
/// first and fires `abort` after (DOM's "signal abort"); the rejection's
/// reaction runs as the listener's callback is cleaned up, after it (the
/// settled promise runs no checkpoint of its own).
#[test]
fn a_timed_out_fetch_reacts_after_the_abort_listeners() {
    let mut page = Page::new("https://example.test/");
    page.eval(
        "var log = []; var s = AbortSignal.timeout(0);
         fetch('t', { signal: s }).catch(e => log.push('rejected ' + e.name));
         s.onabort = () => log.push('abort'); 0",
    );
    fetches(&mut page);
    assert!(page.engine.run_timers());
    assert_eq!(
        page.eval("log.join(' | ')"),
        "abort | rejected TimeoutError"
    );
}

/// A `Request` made in the realm of an iframe removed: its initial
/// `about:blank`'s base URL is its creator's still (its about base URL).
#[test]
fn a_request_of_a_removed_frames_realm() {
    let mut page = Page::new("https://example.test/dir/page");
    assert_eq!(
        page.eval(
            "var f = document.body.appendChild(document.createElement('iframe')); var R = f.contentWindow.Request; f.remove();
             var r1 = new R('resource', { method: 'POST', body: 'string' }); var r2 = new R(r1); [r1.bodyUsed, r2.bodyUsed, r2.url].join()"
        ),
        "true,false,https://example.test/dir/resource"
    );
}
