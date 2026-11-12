//! What `PLAIN` promises, checked against the crate's own source: a front
//! end given only `PLAIN` members has `PLAIN_FRONT_METHODS` and
//! `PLAIN_OP_METHODS` alone, and a method past them is `unreachable!` in
//! it, found only as a member is called. So the types marked `PLAIN`, and
//! the functions of the members that are, are read for the front end's
//! methods they call (by their text: `front.name(`, `op.name(`), which
//! must all be among those.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use dom_api::{
    INTERFACES, MemberKind, PLAIN_FRONT_METHODS, PLAIN_OP_METHODS, TypedMember, TypedVisitor,
};

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

/// A module's functions (its items at any depth: `fn name`), each with
/// its body.
fn functions(text: &str) -> Vec<(String, &str)> {
    let bytes = text.as_bytes();
    let mut functions = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find("fn ") {
        let at = from + found;
        from = at + 3;
        if at > 0 && is_ident(bytes[at - 1]) {
            continue;
        }
        let mut end = from;
        while end < bytes.len() && is_ident(bytes[end]) {
            end += 1;
        }
        let name = &text[from..end];
        // Its body: the first `{` before a `;` (a trait's method with no
        // default has none).
        let Some(open) = text[end..].find(['{', ';']).map(|open| end + open) else {
            continue;
        };
        if bytes[open] == b';' || name.is_empty() {
            continue;
        }
        functions.push((name.to_owned(), &text[open..=block_end(text, open)]));
    }
    functions
}

/// The functions `body` names: `name(`, `name::<`, `module::name`.
fn callees(body: &str) -> BTreeSet<(Option<String>, String)> {
    let bytes = body.as_bytes();
    let mut callees = BTreeSet::new();
    let mut at = 0;
    while at < bytes.len() {
        if !is_ident(bytes[at]) || (at > 0 && (is_ident(bytes[at - 1]) || bytes[at - 1] == b'.')) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && is_ident(bytes[at]) {
            at += 1;
        }
        let name = &body[start..at];
        if !(bytes.get(at) == Some(&b'(') || body[at..].starts_with("::<")) {
            continue;
        }
        let module = body[..start].strip_suffix("::").map(|before| {
            let begin = before
                .bytes()
                .rposition(|byte| !is_ident(byte))
                .map_or(0, |before| before + 1);
            before[begin..].to_owned()
        });
        callees.insert((module, name.to_owned()));
    }
    callees
}

/// Every function of the crate (by module and name): the `OpFront`
/// methods it calls on `op`, itself or through the crate's functions it
/// names.
fn op_calls(sources: &BTreeMap<String, String>) -> BTreeMap<(String, String), BTreeSet<String>> {
    let mut direct: BTreeMap<
        (String, String),
        (BTreeSet<String>, BTreeSet<(Option<String>, String)>),
    > = BTreeMap::new();
    for (module, text) in sources {
        if module == "typed" || module == "interfaces" {
            continue;
        }
        for (name, body) in functions(text) {
            let entry = direct.entry((module.clone(), name)).or_default();
            entry.0.extend(calls_on(body, "op"));
            entry.1.extend(callees(body));
        }
    }
    let by_name: BTreeMap<&str, Vec<&(String, String)>> =
        direct.keys().fold(BTreeMap::new(), |mut by, key| {
            by.entry(key.1.as_str()).or_insert_with(Vec::new).push(key);
            by
        });
    // To a fixed point: a function's calls are its callees' too.
    let mut all: BTreeMap<(String, String), BTreeSet<String>> = direct
        .iter()
        .map(|(key, (calls, _))| (key.clone(), calls.clone()))
        .collect();
    loop {
        let mut grew = false;
        for (key, (_, callees)) in &direct {
            let mut more = BTreeSet::new();
            for (module, name) in callees {
                // A qualified name's module's; else this module's, or
                // (imported) any module's of the name.
                let targets: Vec<(String, String)> = match module {
                    Some(module) if direct.contains_key(&(module.clone(), name.clone())) => {
                        vec![(module.clone(), name.clone())]
                    }
                    Some(_) => Vec::new(),
                    None if direct.contains_key(&(key.0.clone(), name.clone())) => {
                        vec![(key.0.clone(), name.clone())]
                    }
                    None => by_name
                        .get(name.as_str())
                        .map(|keys| keys.iter().map(|key| (*key).clone()).collect())
                        .unwrap_or_default(),
                };
                for target in targets {
                    more.extend(all[&target].iter().cloned());
                }
            }
            let calls = all.get_mut(key).expect("every function");
            let before = calls.len();
            calls.extend(more);
            grew |= calls.len() > before;
        }
        if !grew {
            return all;
        }
    }
}

/// The table's members' functions: by interface, member name and kind,
/// the function's module and name (`get = module::function`).
fn table_functions(table: &str) -> BTreeMap<(String, String, &'static str), Vec<(String, String)>> {
    let mut functions: BTreeMap<_, Vec<_>> = BTreeMap::new();
    let mut interface = String::new();
    let mut declaration = String::new();
    let path = |text: &str| -> Option<(String, String)> {
        // `module::function`, its generics dropped.
        let text = text.trim();
        let text = text.split("::<").next().unwrap_or(text);
        let end = text
            .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
            .unwrap_or(text.len());
        let mut segments = text[..end].rsplit("::");
        let function = segments.next()?.to_owned();
        Some((segments.next()?.to_owned(), function))
    };
    for line in table.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("interface ") {
            interface = rest
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()
                .unwrap_or_default()
                .to_owned();
            continue;
        }
        if declaration.is_empty()
            && !(line.starts_with("method ") || line.starts_with("attribute "))
        {
            continue;
        }
        declaration.push_str(line);
        declaration.push(' ');
        if !line.ends_with(';') {
            continue;
        }
        let text = std::mem::take(&mut declaration);
        let name = |after: &str| {
            text[after.len()..]
                .trim_start_matches("r#")
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()
                .unwrap_or_default()
                .to_owned()
        };
        if text.starts_with("method ") {
            if let Some(function) = text.rsplit(" = ").next().and_then(path) {
                functions
                    .entry((interface.clone(), name("method "), "method"))
                    .or_default()
                    .push(function);
            }
        } else {
            for (kind, marks) in [
                ("getter", &["get = "][..]),
                ("setter", &["set = ", "set: "][..]),
            ] {
                let Some(at) = marks
                    .iter()
                    .find_map(|mark| text.find(mark).map(|at| at + mark.len()))
                else {
                    continue;
                };
                // `set: Type = module::function`.
                let rest = &text[at..];
                let rest = if text[..at].ends_with("set: ") {
                    rest.split_once(" = ").map_or(rest, |(_, rest)| rest)
                } else {
                    rest
                };
                if let Some(function) = path(rest) {
                    functions
                        .entry((interface.clone(), name("attribute "), kind))
                        .or_default()
                        .push(function);
                }
            }
        }
    }
    functions
}

struct PlainMembers(Vec<(&'static str, &'static str, &'static str)>);

impl TypedVisitor for PlainMembers {
    fn any<M: TypedMember>(&mut self) {
        let kind = match M::KIND {
            MemberKind::Getter => "getter",
            MemberKind::Setter => "setter",
            MemberKind::Method => "method",
            _ => return,
        };
        if M::PLAIN {
            self.0.push((INTERFACES[M::INTERFACE].name, M::NAME, kind));
        }
    }
}

/// A `PLAIN` member's function, if it is given the front end's side in
/// the operation, calls only `PLAIN_OP_METHODS` on it, itself or through
/// the functions it calls: not one that has the front end keep something
/// for after the operation (`rethrow`, `set_image_data_pixels`), which
/// or reads through it what such a front end doesn't (`with_array_bytes`),
/// which such a member's types must say (`Rethrown`, `NewImageData`,
/// `ReadImageData`).
#[test]
fn a_plain_members_function_calls_only_the_plain_methods() {
    let sources = sources();
    let calls = op_calls(&sources);
    let functions = table_functions(&sources["interfaces"]);
    let mut members = PlainMembers(Vec::new());
    dom_api::visit_typed(&mut members);
    let (mut checked, mut wrong) = (0, Vec::new());
    for (interface, name, kind) in &members.0 {
        let Some(functions) = functions.get(&((*interface).to_owned(), (*name).to_owned(), *kind))
        else {
            continue;
        };
        for function in functions {
            let Some(calls) = calls.get(function) else {
                continue;
            };
            checked += 1;
            let past: Vec<&String> = calls
                .iter()
                .filter(|call| !PLAIN_OP_METHODS.contains(&call.as_str()))
                .collect();
            if !past.is_empty() {
                wrong.push(format!(
                    "{interface}.{name} ({}::{}): {past:?}",
                    function.0, function.1
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "plain members whose functions call more:\n{}",
        wrong.join("\n")
    );
    // Most of them were found (a few are named otherwise than the table's
    // text is read).
    assert!(
        checked * 10 >= members.0.len() * 9,
        "{checked} of {} checked",
        members.0.len()
    );
}

/// The functions that have the front end keep something are found by the
/// check above: the ones known to, through a member's function.
#[test]
fn the_functions_that_keep_something_are_seen() {
    let sources = sources();
    let calls = op_calls(&sources);
    for (function, method) in [
        (("abort", "throw_if_aborted"), "rethrow"),
        (("canvas", "get_image_data"), "set_image_data_pixels"),
        (("canvas", "create_image_data_of"), "set_image_data_pixels"),
        (("canvas", "put_image_data"), "with_array_bytes"),
    ] {
        let key = (function.0.to_owned(), function.1.to_owned());
        assert!(calls[&key].contains(method), "{function:?} calls {method}");
    }
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
