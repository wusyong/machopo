//! File API in a whole browser: a `File` made now is of now, as
//! `Date.now()` has it; a `FileReader`'s read goes through the renderer's
//! event loop, its events each in a task, after the script that began it.

use std::path::PathBuf;
use std::time::Duration;

use shell::headless;

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-file-reader");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    net::file_url(&path).expect("an absolute path")
}

#[test]
fn a_file_of_now_and_a_read() {
    let url = page(
        "read.html",
        r#"<!DOCTYPE html>
<script>
const log = [];
const f = new File(['h', new Uint8Array([0xc3, 0xa9])], 'x.txt', {type: 'text/plain'});
log.push(Math.abs(f.lastModified - Date.now()) < 1000);
const r = new FileReader();
for (const t of ['loadstart', 'progress', 'load', 'loadend'])
  r.addEventListener(t, e => log.push(`${t} ${e.loaded}/${e.total}`));
r.onloadend = () => testRunner.reportHarnessResults(log.concat(r.result).join(', '));
r.readAsText(f);
log.push('after the script');
</script>"#,
    );
    let report = headless::run_testharness(&url, Duration::from_secs(30), Default::default())
        .expect("the page reports");
    assert_eq!(
        report,
        "true, after the script, loadstart 0/3, progress 3/3, load 3/3, loadend 3/3, h\u{e9}"
    );
}
