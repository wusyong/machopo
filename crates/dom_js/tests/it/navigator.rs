//! `navigator`: the same object each time, Chrome's fixed values, the
//! user agent and webdriver flag the frame gives (none in a test: empty,
//! false), and its `userActivation`, the window's user activation read
//! live.

use crate::history::Page;

#[test]
fn the_navigators_values() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var n = navigator; [n === window.navigator, n instanceof Navigator, n.appCodeName, n.appName, n.product, \
             n.productSub, n.vendor, JSON.stringify(n.vendorSub), n.language, JSON.stringify(n.languages), \
             n.languages === n.languages, n.onLine, n.cookieEnabled, n.hardwareConcurrency >= 1, \
             JSON.stringify(n.userAgent), JSON.stringify(n.appVersion), n.webdriver, typeof n.platform].join()"
        ),
        "true,true,Mozilla,Netscape,Gecko,20030107,Google Inc.,\"\",en-US,[\"en-US\",\"en\"],false,true,false,true,\"\",\"\",false,string"
    );
    assert_eq!(
        page.eval(
            "var a = navigator.userActivation; [a === navigator.userActivation, a instanceof UserActivation, \
             a.hasBeenActive, a.isActive, Object.prototype.toString.call(a)].join()"
        ),
        "true,true,false,false,[object UserActivation]"
    );
}
