//! html5lib tree-construction tests (the `.dat` files in WPT's
//! `html/syntax/parsing/resources/`, in the WPT checkout next to the repo,
//! `../third_party/wpt`): parse each document, dump the DOM and compare.
//!
//! Tests listed in `tests/wpt/tree-construction-expectations.txt` are
//! known failures. The run fails on any other failure, and on a listed
//! test that passes (so the list shrinks as things get fixed).
//! Fragment-parsing tests (`#document-fragment`) parse with a context
//! element made from their `#document-fragment` line.

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::{Path, PathBuf};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn dat_dir() -> PathBuf {
    workspace().join("../third_party/wpt/html/syntax/parsing/resources")
}

struct Test {
    /// `file.dat#N`, N counting from 1.
    id: String,
    data: String,
    /// The context element of a fragment test: `td`, or `svg path` for
    /// one in another namespace.
    fragment: Option<String>,
    /// `Some` if the test is for one scripting mode only.
    scripting: Option<bool>,
    document: String,
}

/// Splits a `.dat` file into tests. A test starts at a `#data` line that
/// begins the file or follows an empty line (text in `#document` may itself
/// contain empty lines).
fn parse_dat(file: &str, text: &str) -> Vec<Test> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut starts: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if *line == "#data" && (i == 0 || lines[i - 1].is_empty()) {
            starts.push(i);
        }
    }
    starts.push(lines.len() + 1);

    let mut tests = Vec::new();
    for (n, window) in starts.windows(2).enumerate() {
        let (start, end) = (window[0], window[1] - 1);
        let body = &lines[start + 1..end.min(lines.len())];
        let section = |name: &str| body.iter().position(|l| *l == name);

        let errors = section("#errors").expect("#errors");
        let doc = section("#document").expect("#document");
        let mut document: Vec<&str> = body[doc + 1..].to_vec();
        // The empty line separating tests (and the file's final newline).
        while document.last() == Some(&"") {
            document.pop();
        }
        tests.push(Test {
            id: format!("{file}#{}", n + 1),
            data: body[..errors].join("\n"),
            fragment: section("#document-fragment").map(|i| body[i + 1].to_owned()),
            scripting: if section("#script-off").is_some() {
                Some(false)
            } else if section("#script-on").is_some() {
                Some(true)
            } else {
                None
            },
            document: document.iter().map(|l| format!("{l}\n")).collect(),
        });
    }
    tests
}

/// `id [ Failure ]` lines; lines starting with `#` are comments (ids
/// themselves contain `#`).
fn expectations() -> BTreeSet<String> {
    let text =
        std::fs::read_to_string(workspace().join("tests/wpt/tree-construction-expectations.txt"))
            .unwrap_or_default();
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let id = l.strip_suffix("[ Failure ]").expect("`<id> [ Failure ]`");
            id.trim().to_owned()
        })
        .collect()
}

fn dat_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dat_dir())
        .expect("WPT's html/syntax/parsing/resources (`python3 scripts/ci.py third-party --clone`)")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "dat"))
        .collect();
    files.sort();
    files
}

/// Each test, once in each scripting mode it applies to: its id, whether
/// scripting is enabled.
fn each_run(test: &Test) -> Vec<(String, bool)> {
    match test.scripting {
        Some(mode) => vec![(test.id.clone(), mode)],
        None => vec![
            (format!("{}/script-on", test.id), true),
            (format!("{}/script-off", test.id), false),
        ],
    }
}

/// A failure's message: the input, what was expected and what came out.
fn failure_message(test: &Test, actual: &str) -> String {
    let mut message = String::new();
    let _ = write!(
        message,
        "#data\n{}\n#expected\n{}#actual\n{}",
        test.data, test.document, actual
    );
    message
}

fn check(documents: usize, fragments: usize, failures: Vec<(String, String)>) {
    let expected = expectations();
    let failed: BTreeSet<&String> = failures.iter().map(|(id, _)| id).collect();
    let unexpected: Vec<&(String, String)> = failures
        .iter()
        .filter(|(id, _)| !expected.contains(id))
        .collect();
    let fixed: Vec<&String> = expected.iter().filter(|id| !failed.contains(id)).collect();

    eprintln!(
        "{documents} document and {fragments} fragment runs, {} failed ({} expected)",
        failures.len(),
        failures.len() - unexpected.len()
    );
    for (id, message) in unexpected.iter().take(10) {
        eprintln!("=== unexpected failure: {id}\n{message}");
    }
    if unexpected.len() > 10 {
        eprintln!("... and {} more", unexpected.len() - 10);
    }
    for id in &fixed {
        eprintln!("=== listed as failing but passes: {id}");
    }
    if std::env::var_os("MACHOPO_PRINT_FAILURES").is_some() {
        for (id, _) in &failures {
            println!("{id} [ Failure ]");
        }
    }
    assert!(
        unexpected.is_empty() && fixed.is_empty(),
        "{} unexpected failures, {} unexpected passes (update tests/wpt/tree-construction-expectations.txt; \
         MACHOPO_PRINT_FAILURES=1 prints every failing id)",
        unexpected.len(),
        fixed.len()
    );
}

/// A fragment test's context element (`td`, `svg path`, `math mi`), in a
/// new no-quirks document.
fn context_element(dom: &mut dom::Dom, context: &str) -> dom::NodeRoot {
    let (namespace, local) = match context.split_once(' ') {
        Some(("svg", local)) => ("http://www.w3.org/2000/svg", local),
        Some(("math", local)) => ("http://www.w3.org/1998/Math/MathML", local),
        Some((prefix, _)) => panic!("context namespace `{prefix}`"),
        None => ("http://www.w3.org/1999/xhtml", context),
    };
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let name = dom::QualName {
            namespace: Some(namespace),
            prefix: None,
            local,
        };
        cx.root(cx.create_element(document, name))
    })
}

/// Runs every test, in each scripting mode it applies to, on a DOM with
/// `options`. Returns (document tests, fragment tests, failures as `(id,
/// message)`).
fn run_all(options: elise::HeapOptions) -> (usize, usize, Vec<(String, String)>) {
    let mut dom = dom::Dom::with_options(options);
    let (mut documents, mut fragments) = (0, 0);
    let mut failures = Vec::new();
    for path in dat_files() {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).unwrap();
        for test in parse_dat(&file, &text) {
            for (id, scripting) in each_run(&test) {
                let actual = match &test.fragment {
                    Some(context) => {
                        fragments += 1;
                        let context = context_element(&mut dom, context);
                        let root =
                            html::parser::parse_fragment(&mut dom, &context, &test.data, scripting);
                        html::parser::dump_children(&mut dom, &root.root())
                    }
                    None => {
                        documents += 1;
                        let document =
                            html::parser::parse_document(&mut dom, &test.data, scripting);
                        html::parser::dump_children(&mut dom, &document.root())
                    }
                };
                if actual != test.document {
                    failures.push((id, failure_message(&test, &actual)));
                }
            }
        }
        dom.collect();
    }
    (documents, fragments, failures)
}

#[test]
fn tree_construction() {
    let (documents, fragments, failures) = run_all(elise::HeapOptions::default());
    check(documents, fragments, failures);
}

/// The same with a GC at every safepoint (each sink call's operation
/// starts at one): the sink's handles keep every node html5ever still
/// refers to, as the nodes move in scavenges.
#[test]
fn tree_construction_under_gc_stress() {
    let (documents, fragments, failures) = run_all(elise::HeapOptions {
        stress: true,
        ..elise::HeapOptions::default()
    });
    check(documents, fragments, failures);
}
