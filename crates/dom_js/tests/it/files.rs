//! `File` (a blob with a name and a time), `ProgressEvent`, `FileReader`'s
//! reads (each event a task of the window's: `loadstart`, `progress`,
//! `load`, `loadend`; what it makes of the bytes; `abort()`; a read while
//! one goes on), and the blob URLs `URL.createObjectURL` gives the host.

use dom_api::{FrameRequest, ScriptEngine};

use crate::history::Page;

#[test]
fn files_and_progress_events() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var f = new File(['ab', new Uint8Array([99])], 'n.txt', { type: 'Text/Plain', lastModified: 1234.9 }); \
             var r = [f instanceof File, f instanceof Blob, Object.prototype.toString.call(f), f.name, f.lastModified, f.size, f.type, JSON.stringify(f.webkitRelativePath)]; \
             var s = f.slice(1); r.push(s instanceof File, s.size); var now = new File([], 'x').lastModified; r.push(Math.abs(now - Date.now()) < 10000); \
             r.push(new File([], 'y', { lastModified: 1e20 }).lastModified); try { Object.getOwnPropertyDescriptor(File.prototype, 'name').get.call(new Blob()); } catch (e) { r.push(e.name); } \
             var e = new ProgressEvent('progress', { lengthComputable: true, loaded: 2, total: 3 }); r.push(e instanceof Event, e.lengthComputable, e.loaded, e.total, new ProgressEvent('x').total); r.join()"
        ),
        "true,true,[object File],n.txt,1234,3,text/plain,\"\",false,2,true,9211727563254776,TypeError,true,true,2,3,0"
    );
}

#[test]
fn a_readers_events_are_tasks() {
    let mut page = Page::new("https://example.test/");
    page.eval(
        "var log = []; var reader = new FileReader(); \
         for (var type of ['loadstart', 'progress', 'load', 'loadend', 'abort']) \
         reader.addEventListener(type, e => log.push(e.type + ':' + e.loaded + '/' + e.total + ':' + reader.readyState + ':' + (e instanceof ProgressEvent))); \
         reader.readAsText(new Blob(['h\\u00e9'])); log.push('sync:' + reader.readyState + ':' + reader.result); \
         try { reader.readAsText(new Blob([])); } catch (e) { log.push(e.name); } 0",
    );
    while page.engine.run_element_tasks() {}
    assert_eq!(
        page.eval("log.join(' ') + ' ' + reader.result + ' ' + reader.error"),
        "sync:1:null InvalidStateError loadstart:0/3:1:true progress:3/3:1:true load:3/3:2:true loadend:3/3:2:true hé null"
    );
    // What a read makes of the bytes; the same buffer for a read.
    page.eval(
        "var results = []; var blob = new Blob([new Uint8Array([0x41, 0xff])], { type: 'a/b' }); \
         var reads = [['readAsDataURL'], ['readAsBinaryString'], ['readAsArrayBuffer'], ['readAsText', 'windows-1252']]; \
         var next = () => { var read = reads.shift(); if (!read) return; var r = new FileReader(); \
         r.onload = () => { var v = r.result; results.push(v instanceof ArrayBuffer ? [...new Uint8Array(v)].join('|') + ':' + (r.result === v) : JSON.stringify(v)); next(); }; \
         r[read[0]](blob, read[1]); }; next(); 0",
    );
    while page.engine.run_element_tasks() {}
    assert_eq!(
        page.eval("results.join(' ')"),
        "\"data:a/b;base64,Qf8=\" \"Aÿ\" 65|255:true \"Aÿ\""
    );
    // `abort()`: `abort` and `loadend` at once, the read's tasks doing
    // nothing; an empty blob has no `progress`.
    page.eval(
        "log = []; var r2 = new FileReader(); for (var type of ['loadstart', 'progress', 'load', 'loadend', 'abort']) r2.addEventListener(type, e => log.push(e.type + ':' + e.lengthComputable)); \
         r2.readAsArrayBuffer(new Blob(['x'])); r2.abort(); log.push(r2.readyState + ':' + r2.result); r2.abort(); \
         var r3 = new FileReader(); r3.onloadstart = r3.onprogress = r3.onloadend = e => log.push('r3 ' + e.type); r3.readAsText(new Blob()); 0",
    );
    while page.engine.run_element_tasks() {}
    assert_eq!(
        page.eval("log.join(' ') + ' ' + [FileReader.EMPTY, FileReader.LOADING, FileReader.DONE, new FileReader().readyState].join()"),
        "abort:false loadend:false 2:null r3 loadstart r3 loadend 0,1,2,0"
    );
    // A `load` handler that begins another read: no `loadend` for the
    // first.
    page.eval(
        "log = []; var r4 = new FileReader(); var first = true; \
         r4.onload = () => { log.push('load ' + r4.result); if (first) { first = false; r4.readAsText(new Blob(['b'])); } }; \
         r4.onloadend = () => log.push('loadend ' + r4.result); r4.readAsText(new Blob(['a'])); 0",
    );
    while page.engine.run_element_tasks() {}
    assert_eq!(page.eval("log.join(' ')"), "load a load b loadend b");
}

#[test]
fn blob_urls_go_to_the_host() {
    let mut page = Page::new("https://example.test/");
    page.engine.take_frame_requests();
    let url = page.eval("var u = URL.createObjectURL(new Blob(['hi'], { type: 'text/x' })); u");
    assert!(
        url.starts_with("blob:https://example.test/")
            && url.len() == "blob:https://example.test/".len() + 36,
        "{url}"
    );
    // Another origin's and what isn't a blob URL: nothing. The host
    // revokes exact matches only (File API's store is by the URL's
    // serialization): the one with a fragment revokes nothing there.
    page.eval(&format!(
        "URL.revokeObjectURL('blob:https://other.test/x'); URL.revokeObjectURL('nope'); URL.revokeObjectURL('{url}#f'); \
         URL.revokeObjectURL('{url}'); 0"
    ));
    let requests: Vec<String> = page
        .engine
        .take_frame_requests()
        .into_iter()
        .map(|request| match request {
            FrameRequest::RegisterBlobUrl { frame, url, data } => {
                assert_eq!(frame, page.engine.top_frame());
                format!("register {url} {} {:?}", data.type_(), data.bytes())
            }
            FrameRequest::RevokeBlobUrl { url } => format!("revoke {url}"),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        requests,
        [
            format!("register {url} text/x [104, 105]"),
            format!("revoke {url}#f"),
            format!("revoke {url}")
        ]
    );
}
