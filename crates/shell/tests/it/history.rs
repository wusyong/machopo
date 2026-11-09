//! The classic history API in a whole browser: what
//! the page reports through `testRunner` (`headless::run_testharness`).

use std::path::PathBuf;
use std::time::Duration;

use shell::headless;

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-history");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    net::file_url(&path).expect("an absolute path")
}

/// What the page at `url` reports.
fn report(url: &str) -> String {
    headless::run_testharness(url, Duration::from_secs(30), Default::default())
        .expect("the page reports")
}

/// `pushState` to a URL with a fragment changes the URL and doesn't
/// scroll; a navigation to a fragment does, with the next frame.
#[test]
fn push_state_does_not_scroll() {
    let url = page(
        "no-scroll.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 }</style>
<div style="height: 3000px"></div>
<div id="far" style="height: 10px"></div>
<div id="far2" style="height: 3000px"></div>
<script>
const frames = (n, then) => n ? requestAnimationFrame(() => frames(n - 1, then)) : then();
onload = () => {
  const log = [];
  history.pushState({n: 1}, '', '#far');
  log.push(location.hash, history.state.n);
  frames(2, () => {
    log.push(scrollY);
    location.hash = '#far2';
    frames(2, () => {
      log.push(scrollY > 0);
      testRunner.reportHarnessResults(log.join(' / '));
    });
  });
};
</script>"#,
    );
    assert_eq!(report(&url), "#far / 1 / 0 / true");
}

/// A traversal to an entry with a fragment scrolls there, unless its
/// scroll restoration mode is manual (no position is restored);
/// `history.back()` goes through the browser.
#[test]
fn a_traversal_scrolls_to_the_fragment_unless_manual() {
    for (mode, scrolled) in [("auto", "true"), ("manual", "false")] {
        let url = page(
            &format!("traversal-{mode}.html"),
            &format!(
                r#"<!DOCTYPE html>
<style>html, body {{ margin: 0 }}</style>
<div style="height: 3000px"></div>
<div id="far" style="height: 3000px"></div>
<script>
const frames = (n, then) => n ? requestAnimationFrame(() => frames(n - 1, then)) : then();
onload = () => {{
  history.scrollRestoration = '{mode}';
  history.pushState(null, '', '#far');
  history.pushState(null, '', '#elsewhere');
  addEventListener('popstate', () => frames(2, () => {{
    testRunner.reportHarnessResults([location.hash, scrollY > 0].join(' / '));
  }}));
  history.back();
}};
</script>"#
            ),
        );
        assert_eq!(report(&url), format!("#far / {scrolled}"), "{mode}");
    }
}

/// The document's target element (`:target`) is the element its URL's
/// fragment names once it is parsed (after `DOMContentLoaded`, as Blink
/// processes the fragment), and moves with a navigation to a fragment (at
/// once) and a traversal; not with `pushState`.
#[test]
fn the_target_element_follows_the_fragment() {
    let url = page(
        "target.html",
        r#"<!DOCTYPE html>
<div id="a"></div><div id="b"></div><div id="c"></div>
<script>
const log = [];
const target = () => document.querySelector(':target')?.id ?? 'none';
addEventListener('DOMContentLoaded', () => log.push(target()));
onload = () => {
  log.push(target());
  history.pushState(null, '', '#c');
  log.push(target());
  location.hash = '#b';
  log.push(target());
  addEventListener('popstate', () => {
    log.push(target());
    testRunner.reportHarnessResults(log.join(' / '));
  });
  // Before the document is completely loaded, `#b` replaces `#c`.
  history.back();
};
</script>"#,
    );
    assert_eq!(report(&format!("{url}#a")), "none / a / a / b / a");
}
