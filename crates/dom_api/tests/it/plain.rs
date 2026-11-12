//! What `PLAIN` promises of a type, checked against the crate's own
//! source: a front end given only `PLAIN` members has
//! `PLAIN_FRONT_METHODS` and `PLAIN_OP_METHODS`, and a method past them
//! may be `unreachable!` in it, found only as a member is called. So the
//! types marked `PLAIN` are read for the front end's methods they call
//! (by their text: `front.name(`, `op.name(`), which must all be among
//! those. (What a member's function calls of the front end's side in the
//! operation isn't checked here: each of those methods every front end
//! has, or it has no default and the front end doesn't compile.)

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use dom_api::{PLAIN_FRONT_METHODS, PLAIN_OP_METHODS};

/// The crate's sources by module, and the table's macros'.
fn sources() -> BTreeMap<String, String> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = BTreeMap::new();
    for entry in std::fs::read_dir(&src).expect("the crate's sources") {
        let path = entry.expect("an entry").path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            let module = path.file_stem().unwrap().to_string_lossy().into_owned();
            sources.insert(
                module,
                code(&std::fs::read_to_string(&path).expect("a source")),
            );
        }
    }
    sources
}

/// `text` with its comments', strings' and character literals' insides
/// blanked: their braces and names aren't the code's.
fn code(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
        for byte in &mut out[from..to] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    };
    let mut at = 0;
    while at < bytes.len() {
        let rest = &bytes[at..];
        if rest.starts_with(b"//") {
            let end = rest
                .iter()
                .position(|&byte| byte == b'\n')
                .map_or(bytes.len(), |end| at + end);
            blank(&mut out, at, end);
            at = end;
        } else if rest.starts_with(b"/*") {
            let end = text[at..]
                .find("*/")
                .map_or(bytes.len(), |end| at + end + 2);
            blank(&mut out, at, end);
            at = end;
        } else if rest[0] == b'"' {
            // A raw string's hashes before it close it too.
            let hashes = bytes[..at]
                .iter()
                .rev()
                .take_while(|&&byte| byte == b'#')
                .count();
            let raw = bytes.get(at.wrapping_sub(hashes + 1)) == Some(&b'r') && at > hashes;
            let mut end = at + 1;
            loop {
                match bytes.get(end) {
                    None => break,
                    Some(b'\\') if !raw => end += 2,
                    Some(b'"')
                        if !raw
                            || bytes[end + 1..]
                                .iter()
                                .take(hashes)
                                .all(|&byte| byte == b'#') =>
                    {
                        break;
                    }
                    Some(_) => end += 1,
                }
            }
            let end = end.min(bytes.len());
            blank(&mut out, at + 1, end);
            at = end + 1;
        } else if rest[0] == b'\'' {
            // A character literal, not a lifetime: closed after one
            // character or an escape.
            let end = if rest.get(1) == Some(&b'\\') {
                rest[2..]
                    .iter()
                    .position(|&byte| byte == b'\'')
                    .map(|end| end + 2)
            } else {
                let width = text[at + 1..].chars().next().map_or(1, char::len_utf8);
                (rest.get(1 + width) == Some(&b'\'')).then_some(1 + width)
            };
            match end {
                Some(end) => {
                    blank(&mut out, at + 1, at + end);
                    at += end + 1;
                }
                None => at += 1,
            }
        } else {
            at += 1;
        }
    }
    String::from_utf8(out).expect("blanked at characters' boundaries")
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Where the block opened at `open` (a `{`) closes.
fn block_end(text: &str, open: usize) -> usize {
    let mut depth = 0usize;
    for (at, byte) in text.bytes().enumerate().skip(open) {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return at;
                }
            }
            _ => {}
        }
    }
    panic!("an unclosed block")
}

/// The methods called on `receiver` in `text`: `receiver.name(` or
/// `receiver.name::<..>(`, the dot on the next line too.
fn calls_on(text: &str, receiver: &str) -> BTreeSet<String> {
    let bytes = text.as_bytes();
    let mut calls = BTreeSet::new();
    let mut from = 0;
    while let Some(found) = text[from..].find(receiver) {
        let at = from + found;
        from = at + receiver.len();
        if at > 0 && (is_ident(bytes[at - 1]) || bytes[at - 1] == b'.') {
            continue;
        }
        let mut next = from;
        while next < bytes.len() && bytes[next].is_ascii_whitespace() {
            next += 1;
        }
        if bytes.get(next) != Some(&b'.') {
            continue;
        }
        next += 1;
        let start = next;
        while next < bytes.len() && is_ident(bytes[next]) {
            next += 1;
        }
        if next > start && (bytes.get(next) == Some(&b'(') || text[next..].starts_with("::<")) {
            calls.insert(text[start..next].to_owned());
        }
    }
    calls
}

/// The front end's methods each block of `text` headed by `header` calls
/// on `front` and `op`, of the blocks `wanted`.
fn blocks_calls<'a>(
    text: &'a str,
    header: &str,
    wanted: impl Fn(&str, &str) -> bool,
) -> Vec<(String, BTreeSet<String>)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find(header).map(|at| from + at) {
        from = at + header.len();
        // Its body: the first `{` before a `;` (a bound's mention has
        // none).
        let Some(open) = text[at..].find(['{', ';']).map(|open| at + open) else {
            continue;
        };
        if text.as_bytes()[open] == b';' {
            continue;
        }
        let body = &text[open..=block_end(text, open)];
        let head = text[at..open].trim();
        if !wanted(head, body) {
            continue;
        }
        let mut calls = calls_on(body, "front");
        calls.extend(calls_on(body, "op"));
        found.push((head.to_owned(), calls));
    }
    found
}

/// A type marked `PLAIN` (an argument's, a result's, a dictionary
/// member's), and every object type (an interface's `this` and
/// arguments), reach the front end through `PLAIN_FRONT_METHODS` and
/// `PLAIN_OP_METHODS` alone; so do the functions they share and the
/// types the table's macros make.
#[test]
fn a_plain_type_calls_only_the_plain_methods() {
    let sources = sources();
    let typed = &sources["typed"];
    let allowed = |call: &String| {
        PLAIN_FRONT_METHODS.contains(&call.as_str()) || PLAIN_OP_METHODS.contains(&call.as_str())
    };
    let plain = |_: &str, body: &str| body.contains("const PLAIN: bool = true;");
    let mut blocks = Vec::new();
    for header in [" ArgType for ", " RetType for ", " DictMember for "] {
        blocks.extend(blocks_calls(typed, header, plain));
    }
    assert!(blocks.len() > 60, "{} plain types read", blocks.len());
    blocks.extend(blocks_calls(typed, " ObjectType for ", |_, _| true));
    // A dictionary's members that are the front end's handles: plain,
    // all.
    let handles = blocks_calls(typed, " DictHandle<F> for ", |_, _| true);
    assert!(!handles.is_empty());
    blocks.extend(handles);
    // What the plain types call that isn't a method of theirs.
    for function in [
        "fn node_param<",
        "fn take_node<",
        "fn take_object<",
        "fn give_object<",
    ] {
        let found = blocks_calls(typed, function, |_, _| true);
        assert!(!found.is_empty(), "{function}");
        blocks.extend(found);
    }
    let mut wrong: Vec<String> = blocks
        .iter()
        .filter_map(|(head, calls)| {
            let past: Vec<&String> = calls.iter().filter(|call| !allowed(call)).collect();
            (!past.is_empty()).then(|| format!("{head}: {past:?}"))
        })
        .collect();
    // The macros' types: a node interface's, an object interface's, a
    // callback interface's, a dictionary's (whose members' types say
    // whether it is plain).
    let macros = code(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../dom_api_macros/src/lib.rs"),
        )
        .expect("the macros' source"),
    );
    for function in [
        "fn node_types(",
        "fn object_types(",
        "fn callback_items(",
        "fn dictionary_items(",
    ] {
        let at = macros
            .find(function)
            .unwrap_or_else(|| panic!("{function}"));
        let open = at + macros[at..].find('{').expect("its body");
        let body = &macros[open..=block_end(&macros, open)];
        let mut calls = calls_on(body, "front");
        calls.extend(calls_on(body, "op"));
        assert!(!calls.is_empty(), "{function}");
        let past: Vec<String> = calls
            .into_iter()
            // A dictionary returned isn't `PLAIN`.
            .filter(|call| !allowed(call) && call != "ret_dictionary")
            .collect();
        if !past.is_empty() {
            wrong.push(format!("{function}: {past:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "plain types that call more:\n{}",
        wrong.join("\n")
    );
}
