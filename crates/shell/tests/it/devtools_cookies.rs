//! DevTools' and automation's cookies (CDP's `Network` and `Storage`
//! cookie commands, Chromium's `NetworkHandler` and `StorageHandler`): the
//! profile's, every one of any context, `HttpOnly` ones too.

use net::test_server::Route;
use serde_json::{Value, json};

use crate::automation_pipe::server;
use crate::devtools::{evaluate, load, page};
use crate::remote_debugging::{Client, Machopo};

fn ok(client: &mut Client, method: &str, params: Value) -> Value {
    let answer = client.call(None, method, params);
    assert!(answer.get("result").is_some(), "{method}: {answer}");
    answer["result"].clone()
}

fn error(client: &mut Client, method: &str, params: Value) -> String {
    let answer = client.call(None, method, params);
    assert!(answer.get("error").is_some(), "{method}: {answer}");
    answer["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// The cookies' names, sorted.
fn names(answer: &Value) -> Vec<String> {
    let mut names: Vec<String> = answer["cookies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cookie| cookie["name"].as_str().unwrap().to_owned())
        .collect();
    names.sort_unstable();
    names
}

fn named<'a>(answer: &'a Value, name: &str) -> &'a Value {
    answer["cookies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|cookie| cookie["name"] == name)
        .unwrap_or_else(|| panic!("no {name} in {answer}"))
}

/// `Network.getCookies` gives the page's (the tab's frames' URLs, or those
/// named), `HttpOnly` and every `SameSite` too, as `Network.Cookie`s;
/// `setCookie` sets one script then reads, `HttpOnly` one it doesn't;
/// `deleteCookies` deletes by name and the URL's host; the parts that
/// make no cookie are errors.
#[test]
fn a_pages_cookies_read_set_and_deleted() {
    let server = server(&[(
        "/",
        Route::ok("text/html", "<!DOCTYPE html>page")
            .with_header("Set-Cookie", "hidden=1; HttpOnly")
            .with_header("Set-Cookie", "strict=2; SameSite=Strict; Max-Age=3600"),
    )]);
    let url = server.url("/");
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &url);

    let cookies = ok(&mut page, "Network.getCookies", json!({}));
    assert_eq!(names(&cookies), ["hidden", "strict"]);
    let hidden = named(&cookies, "hidden");
    let host = url::Url::parse(&url)
        .unwrap()
        .host_str()
        .unwrap()
        .to_owned();
    assert_eq!(hidden["domain"], host.as_str());
    assert_eq!(hidden["path"], "/");
    assert_eq!(hidden["httpOnly"], true);
    assert_eq!(hidden["session"], true);
    assert_eq!(hidden["expires"], -1.0);
    assert_eq!(hidden["size"], 7);
    assert!(hidden.get("sameSite").is_none(), "{hidden}");
    let strict = named(&cookies, "strict");
    assert_eq!(strict["sameSite"], "Strict");
    assert_eq!(strict["session"], false);
    assert!(strict["expires"].as_f64().unwrap() > 0.0);
    let other = ok(
        &mut page,
        "Network.getCookies",
        json!({ "urls": ["http://example.com/"] }),
    );
    assert_eq!(names(&other), Vec::<String>::new());

    // In 100 days (at most 400 are kept).
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 100 * 86_400;
    let set = ok(
        &mut page,
        "Network.setCookie",
        json!({ "name": "set", "value": "3", "url": url, "expires": expires }),
    );
    assert_eq!(set, json!({ "success": true }));
    ok(
        &mut page,
        "Network.setCookie",
        json!({ "name": "only", "value": "4", "url": url, "httpOnly": true }),
    );
    let set = named(&ok(&mut page, "Network.getCookies", json!({})), "set").clone();
    assert_eq!(set["expires"].as_f64(), Some(expires as f64));
    assert_eq!(
        evaluate(
            &mut page,
            "document.cookie.split('; ').sort().join(' ')",
            None
        ),
        "set=3 strict=2"
    );

    ok(
        &mut page,
        "Network.deleteCookies",
        json!({ "name": "hidden", "url": url }),
    );
    assert_eq!(
        names(&ok(&mut page, "Network.getCookies", json!({}))),
        ["only", "set", "strict"]
    );

    assert!(
        error(
            &mut page,
            "Network.setCookie",
            json!({ "name": "a", "value": "1" })
        )
        .contains("url or domain")
    );
    assert_eq!(
        error(
            &mut page,
            "Network.setCookie",
            json!({ "name": "a;b", "value": "1", "url": url })
        ),
        "Sanitizing cookie failed"
    );
    error(
        &mut page,
        "Network.setCookie",
        json!({ "name": "a", "value": "1", "url": "ftp://example.com/" }),
    );
    error(&mut page, "Network.deleteCookies", json!({ "name": "a" }));
}

/// The browser's own: `Storage.getCookies` and `Network.getAllCookies`
/// give every cookie, `setCookies` sets them all (a domain with a dot is a
/// domain cookie's) or none if one makes no cookie, and
/// `clearBrowserCookies`, `Storage.clearCookies` delete them all. A
/// partitioned one is given with its partition, and deleted by it.
#[test]
fn the_browsers_cookies_all_at_once() {
    let machopo = Machopo::start();
    let mut browser = machopo.connect_browser();
    let cookies = json!([
        { "name": "a", "value": "1", "domain": "a.example", "secure": true },
        { "name": "b", "value": "2", "domain": ".b.example", "path": "/p", "secure": true },
        {
            "name": "c", "value": "3", "domain": "c.example", "secure": true,
            "partitionKey": { "topLevelSite": "https://top.example", "hasCrossSiteAncestor": true },
        },
    ]);
    ok(
        &mut browser,
        "Storage.setCookies",
        json!({ "cookies": cookies }),
    );
    let all = ok(&mut browser, "Storage.getCookies", json!({}));
    assert_eq!(names(&all), ["a", "b", "c"]);
    // A domain without a dot: a host-only cookie of it.
    assert_eq!(named(&all, "a")["domain"], "a.example");
    assert_eq!(named(&all, "b")["domain"], ".b.example");
    assert_eq!(named(&all, "b")["path"], "/p");
    assert_eq!(
        named(&all, "c")["partitionKey"],
        json!({ "topLevelSite": "https://top.example", "hasCrossSiteAncestor": true })
    );
    assert_eq!(ok(&mut browser, "Network.getAllCookies", json!({})), all);

    // An `https:` URL's is set as from a secure URL, `Secure` or not.
    ok(
        &mut browser,
        "Network.setCookie",
        json!({ "name": "s", "value": "1", "url": "https://s.example/" }),
    );
    let s_cookie = named(&ok(&mut browser, "Storage.getCookies", json!({})), "s").clone();
    assert_eq!(s_cookie["sourceScheme"], "Secure");
    assert_eq!(s_cookie["secure"], true);
    ok(
        &mut browser,
        "Network.deleteCookies",
        json!({ "name": "s", "url": "https://s.example/" }),
    );

    // One that makes no cookie: none set.
    let bad = json!([
        { "name": "d", "value": "4", "domain": "d.example" },
        { "name": "e", "value": "5" },
    ]);
    error(
        &mut browser,
        "Storage.setCookies",
        json!({ "cookies": bad }),
    );
    assert_eq!(
        names(&ok(&mut browser, "Storage.getCookies", json!({}))),
        ["a", "b", "c"]
    );

    // A partitioned one goes with its partition alone.
    ok(
        &mut browser,
        "Network.deleteCookies",
        json!({ "name": "c", "domain": "c.example" }),
    );
    assert_eq!(
        names(&ok(&mut browser, "Storage.getCookies", json!({}))),
        ["a", "b", "c"]
    );
    ok(
        &mut browser,
        "Network.deleteCookies",
        json!({
            "name": "c", "domain": "c.example",
            "partitionKey": { "topLevelSite": "https://top.example", "hasCrossSiteAncestor": true },
        }),
    );
    assert_eq!(
        names(&ok(&mut browser, "Storage.getCookies", json!({}))),
        ["a", "b"]
    );

    ok(&mut browser, "Network.clearBrowserCookies", json!({}));
    assert_eq!(
        names(&ok(&mut browser, "Storage.getCookies", json!({}))),
        Vec::<String>::new()
    );
    ok(
        &mut browser,
        "Storage.setCookies",
        json!({ "cookies": [cookies[0].clone()] }),
    );
    ok(&mut browser, "Storage.clearCookies", json!({}));
    assert_eq!(
        names(&ok(&mut browser, "Network.getAllCookies", json!({}))),
        Vec::<String>::new()
    );
}
