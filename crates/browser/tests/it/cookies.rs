//! Cookies through the browser: a page's response sets them, its
//! subresources and later navigations send them, by the trusted params the
//! browser gives each request (a navigation's by its frame, a document's
//! loads by its loader). A cross-site child frame's are a third party's:
//! kept and sent only with Chromium's cookie policy.

use browser::{NetworkContext, NetworkContextParams, Profile, StoragePartition};
use net::test_server::{Route, TestServer};

use crate::dom_storage::titles_until;
use crate::navigation::{server, tab_in};

fn partition(allow_third_party_cookies: bool) -> StoragePartition {
    StoragePartition::new(
        &Profile::in_memory(),
        NetworkContext::start(NetworkContextParams {
            user_agent: browser::user_agent(),
            host_resolver_rules: net::HostMappingRules::parse("MAP *.localhost 127.0.0.1").unwrap(),
            allow_third_party_cookies,
            ..Default::default()
        })
        .unwrap(),
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
/// frame of another site sets one too, which a page of that site gets
/// later only with Chromium's cookie policy.
#[test]
fn a_pages_cookies_and_a_third_partys() {
    for allow in [false, true] {
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
        let (mut web_contents, host_rx) = tab_in(partition(allow));
        web_contents.navigate(server.url_for_host("a.localhost", "/page"));
        titles_until(&mut web_contents, &host_rx, |title| title == "loaded");
        assert_eq!(
            sent(&server, "/image").as_deref(),
            Some("first=1"),
            "allow {allow}"
        );
        web_contents.navigate(server.url_for_host("b.localhost", "/check"));
        titles_until(&mut web_contents, &host_rx, |title| title == "checked");
        assert_eq!(
            sent(&server, "/check").as_deref(),
            allow.then_some("third=1"),
            "allow {allow}"
        );
        web_contents.shutdown();
    }
}
