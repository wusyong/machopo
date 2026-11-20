//! Cookies through the browser: a page's response sets them, its
//! subresources and later navigations send them, by the trusted params the
//! browser gives each request (a navigation's by its frame, a document's
//! loads by its loader). A cross-site child frame's are a third party's:
//! neither kept nor sent.

use browser::{NetworkContextParams, Profile, StoragePartition};
use net::test_server::{Route, TestServer};

use crate::dom_storage::titles_until;
use crate::navigation::{server, tab_in};

fn partition() -> StoragePartition {
    partition_of(&Profile::in_memory())
}

fn partition_of(profile: &Profile) -> StoragePartition {
    StoragePartition::new(
        profile,
        NetworkContextParams {
            user_agent: browser::user_agent(),
            host_resolver_rules: net::HostMappingRules::parse("MAP *.localhost 127.0.0.1").unwrap(),
            ..Default::default()
        },
    )
    .unwrap()
}

/// The `Cookie` header `server` last had with a request for `target`.
fn sent(server: &TestServer, target: &str) -> Option<String> {
    server
        .requests()
        .into_iter()
        .rfind(|request| request.target == target)
        .and_then(|request| request.header("cookie").map(str::to_owned))
}

/// A page sets a cookie with its response; its image sends it. Its child
/// frame of another site sets one too, which a page of that site doesn't
/// get later: a third party's cookies are neither sent nor kept.
#[test]
fn a_pages_cookies_and_a_third_partys() {
    let server = server(&[
        (
            "/page",
            Route::ok(
                "text/html",
                r#"<!DOCTYPE html><img src="/image"><script>
let loads = 0;
const loaded = () => { if (++loads == 3) document.title = 'loaded'; };
document.querySelector('img').onerror = loaded;
const frame = document.createElement('iframe');
frame.onload = loaded;
frame.src = `http://b.localhost:${location.port}/frame`;
document.body.append(frame);
onload = loaded;
</script>"#,
            )
            .with_header("Set-Cookie", "first=1"),
        ),
        ("/image", Route::ok("text/plain", "not an image")),
        (
            "/frame",
            Route::ok("text/html", "frame")
                .with_header("Set-Cookie", "third=1; SameSite=None; Secure"),
        ),
        (
            "/check",
            Route::ok("text/html", "<script>document.title = 'checked'</script>"),
        ),
    ]);
    let (mut web_contents, host_rx) = tab_in(partition());
    web_contents.navigate(server.url_for_host("a.localhost", "/page"));
    titles_until(&mut web_contents, &host_rx, |title| title == "loaded");
    assert_eq!(sent(&server, "/image").as_deref(), Some("first=1"));
    web_contents.navigate(server.url_for_host("b.localhost", "/check"));
    titles_until(&mut web_contents, &host_rx, |title| title == "checked");
    assert_eq!(sent(&server, "/check").as_deref(), None);
    web_contents.shutdown();
}

/// A profile's folder keeps its persistent cookies: the storage of the
/// profile opened again, once the one before has gone, sends them.
#[test]
fn a_profiles_cookies_outlast_its_storage() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("browser-cookies")
        .join("outlast");
    let _ = std::fs::remove_dir_all(&dir);
    let server = server(&[
        (
            "/set",
            Route::ok("text/html", "<script>document.title = 'set'</script>")
                .with_header("Set-Cookie", "kept=1; Max-Age=3600"),
        ),
        (
            "/check",
            Route::ok("text/html", "<script>document.title = 'checked'</script>"),
        ),
    ]);
    {
        let profile = Profile::open(&dir).unwrap();
        let (mut web_contents, host_rx) = tab_in(partition_of(&profile));
        web_contents.navigate(server.url_for_host("a.localhost", "/set"));
        titles_until(&mut web_contents, &host_rx, |title| title == "set");
        web_contents.shutdown();
    }
    let profile = Profile::open(&dir).unwrap();
    let (mut web_contents, host_rx) = tab_in(partition_of(&profile));
    web_contents.navigate(server.url_for_host("a.localhost", "/check"));
    titles_until(&mut web_contents, &host_rx, |title| title == "checked");
    assert_eq!(sent(&server, "/check").as_deref(), Some("kept=1"));
    web_contents.shutdown();
}

/// `document.cookie`: script reads the page's cookies but `HttpOnly` ones,
/// and what it sets goes with the page's next request; an `about:blank`
/// iframe's document reads its creator's; a document of no browsing
/// context reads none, nor one of a frame taken out of the page.
/// `navigator.cookieEnabled` is true at the top, false in a third party's
/// iframe, whose `document.cookie` keeps no cookie it sets.
#[test]
fn document_cookie_and_cookie_enabled() {
    let server = server(&[
        (
            "/page",
            Route::ok(
                "text/html",
                r#"<!DOCTYPE html><script>
const results = [];
try {
results.push(document.cookie);
document.cookie = 'script=1';
document.cookie = 'hidden=2';
results.push(document.cookie);
const blank = document.createElement('iframe');
document.documentElement.append(blank);
results.push(blank.contentDocument.cookie);
results.push(document.implementation.createHTMLDocument('').cookie);
results.push(navigator.cookieEnabled);
fetch('/echo').then(() => {
  const same = document.createElement('iframe');
  same.src = '/echo';
  same.onload = () => {
    const detached = same.contentDocument;
    results.push(detached.cookie);
    same.remove();
    results.push(detached.cookie);
    const frame = document.createElement('iframe');
    frame.src = `http://b.localhost:${location.port}/frame`;
    frame.onload = () => { document.title = results.join('|'); };
    document.documentElement.append(frame);
  };
  document.documentElement.append(same);
});
} catch (error) { document.title = `error|${error}`; }
</script>"#,
            )
            .with_header("Set-Cookie", "http=1")
            .with_header("Set-Cookie", "hidden=1; HttpOnly"),
        ),
        ("/echo", Route::ok("text/plain", "echo")),
        ("/report", Route::ok("text/plain", "report")),
        (
            "/frame",
            Route::ok(
                "text/html",
                r#"<script>
let report;
try {
  document.cookie = 'third=1; SameSite=None; Secure';
  report = `${navigator.cookieEnabled}:${document.cookie}`;
} catch (error) { report = `error:${error}`; }
const image = new Image();
image.src = `/report?${report}`;
document.documentElement.append(image);
</script>"#,
            ),
        ),
    ]);
    let (mut web_contents, host_rx) = tab_in(partition());
    web_contents.navigate(server.url_for_host("a.localhost", "/page"));
    let title = titles_until(&mut web_contents, &host_rx, |title| title.contains('|'));
    assert_eq!(
        title,
        "http=1|http=1; script=1|http=1; script=1||true|http=1; script=1|"
    );
    let report = server
        .requests()
        .into_iter()
        .find(|request| request.target.starts_with("/report?"))
        .map(|request| request.target);
    assert_eq!(report.as_deref(), Some("/report?false:"));
    assert_eq!(
        sent(&server, "/echo").as_deref(),
        Some("http=1; hidden=1; script=1")
    );
    web_contents.shutdown();
}

/// A `file:` page's document is cookie-averse: it reads no cookies and
/// throws nothing, and `navigator.cookieEnabled` is true.
#[test]
fn a_file_pages_document_has_no_cookies() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("browser-cookies")
        .join("file");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let page = dir.join("page.html");
    std::fs::write(
        &page,
        r#"<script>
try {
  document.cookie = 'a=1';
  document.title = `[${document.cookie}]:${navigator.cookieEnabled}`;
} catch (error) { document.title = `error:${error}`; }
</script>"#,
    )
    .unwrap();
    let (mut web_contents, host_rx) = tab_in(partition());
    web_contents.navigate(url::Url::from_file_path(&page).unwrap().to_string());
    let title = titles_until(&mut web_contents, &host_rx, |title| !title.is_empty());
    assert_eq!(title, "[]:true");
    web_contents.shutdown();
}
