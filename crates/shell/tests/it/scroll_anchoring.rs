//! Scroll anchoring in a whole browser: content added above what the
//! viewport shows scrolls it by as much, already when script reads the
//! layout, with a `scroll` event; what shows stays. As Chrome 152 does it.

use std::path::PathBuf;
use std::time::Duration;

use shell::headless;

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-scroll-anchoring");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    net::file_url(&path).expect("an absolute path")
}

/// Scrolled to 150 (`b` 50px above the top), 30px added at the top: at
/// 180 when script asks, `b` where it was; one `scroll` event; `b` still
/// shown at the top. A scroll by script picks the anchor again where it
/// went: from 260, growing what was added by 30 more shows `c` there.
#[test]
fn content_added_above_keeps_what_shows() {
    let url = page(
        "added.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } div { height: 100px }</style>
<div id="a"></div><div id="b"></div><div id="c"></div><div style="height: 2000px"></div>
<script>
const frame = () => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
onload = async () => {
  const log = [];
  let scrolls = [];
  addEventListener('scroll', () => scrolls.push(scrollY));
  scrollTo(0, 150);
  await frame();
  log.push(scrolls.join(','));
  scrolls = [];
  const shown = document.elementFromPoint(10, 10).id;
  const added = document.createElement('div');
  added.style.height = '30px';
  document.body.prepend(added);
  log.push(scrollY, document.getElementById('b').getBoundingClientRect().top);
  await frame();
  log.push(scrolls.join(','), shown, document.elementFromPoint(10, 10).id);
  scrollTo(0, 260);
  added.style.height = '60px';
  await frame();
  log.push(scrollY, document.elementFromPoint(10, 10).id);
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    assert_eq!(
        headless::run_testharness(&url, Duration::from_secs(30), Default::default())
            .expect("the page reports"),
        "150 / 180 / -50 / 180 / b / b / 290 / c"
    );
}
