//! `machopo --run-web-tests`'s runner (`shell::web_tests`): a line a test (its
//! timeout and URL), a record a line of JSON: the page's report (parsed when it is JSON, the text otherwise), a
//! timeout, a line that isn't a test; a reftest page's screenshot, written
//! as a PNG.

use std::path::PathBuf;

use shell::web_tests;

fn page(name: &str, contents: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-web-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    net::file_url(&path).expect("an absolute path")
}

fn run(input: &str) -> Vec<serde_json::Value> {
    let mut output = Vec::new();
    web_tests::run(&Default::default(), input.as_bytes(), &mut output).expect("runs");
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("a line of JSON"))
        .collect()
}

#[test]
fn each_test_is_a_record() {
    let json = page(
        "json.html",
        "<script>testRunner.reportHarnessResults(JSON.stringify({status: 0}))</script>",
    );
    let text = page(
        "text.html",
        "<script>testRunner.reportHarnessResults('plain text')</script>",
    );
    let silent = page("silent.html", "<p>nothing reported</p>");
    let records = run(&format!(
        "10000 {json}\n\n10000 {text}\n500 {silent}\nnot a test\n"
    ));
    assert_eq!(records.len(), 4, "{records:?}");
    assert_eq!(
        records[0],
        serde_json::json!({"url": json, "status": "OK", "results": {"status": 0}})
    );
    assert_eq!(
        records[1],
        serde_json::json!({"url": text, "status": "OK", "results": "plain text"})
    );
    assert_eq!(
        records[2],
        serde_json::json!({"url": silent, "status": "TIMEOUT"})
    );
    assert_eq!(records[3]["status"], "ERROR");
    assert_eq!(records[3]["url"], "not a test");
}

/// `screenshot TIMEOUT URL PATH`: the page at 800×600, written to `PATH`
/// once `reftest-wait` is off; `TIMEOUT` if it stays on; `ERROR` for a
/// line short of a path.
#[test]
fn a_screenshot_is_written() {
    let green = page(
        "green.html",
        "<!doctype html><html class=reftest-wait><style>body { margin: 0 }</style>\
         <div style='width: 30px; height: 30px; background: green'></div>\
         <script>setTimeout(() => document.documentElement.removeAttribute('class'), 30)</script>",
    );
    let waiting = page("waiting.html", "<!doctype html><html class=reftest-wait>");
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-web-tests");
    let out = dir.join("green.png");
    let _ = std::fs::remove_file(&out);
    let records = run(&format!(
        "screenshot 10000 {green} {}\nscreenshot 300 {waiting} {}\nscreenshot 100 {green}\n",
        out.display(),
        dir.join("waiting.png").display(),
    ));
    assert_eq!(
        records[0],
        serde_json::json!({"url": green, "status": "OK"})
    );
    assert_eq!(
        records[1],
        serde_json::json!({"url": waiting, "status": "TIMEOUT"})
    );
    assert_eq!(records[2]["status"], "ERROR");
    let png = std::fs::read(&out).expect("written");
    assert_eq!(&png[1..4], b"PNG");
}
