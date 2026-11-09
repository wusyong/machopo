//! A V8 string's text lent in place (`LentString`): a flat one-byte ASCII
//! string's, on an isolate whose GCs check loans (or peeked at, lent for
//! a call); a GC while one is lent aborts.

use bindings::{LentString, V8PerIsolateData, borrowed_texts};

/// `source`'s value, run in a new context of `scope`'s isolate.
fn eval<'s>(scope: &mut v8::PinScope<'s, '_>, source: &str) -> v8::Local<'s, v8::String> {
    let source = v8::String::new(scope, source).unwrap();
    let value = v8::Script::compile(scope, source, None)
        .unwrap()
        .run(scope)
        .unwrap();
    value.to_string(scope).unwrap()
}

#[test]
fn a_flat_ascii_string_lends_its_text() {
    let mut data = V8PerIsolateData::new();
    let isolate = data.isolate();
    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let before = borrowed_texts();
    // A literal (internalized, sequential), a string made at run time, and
    // a property key made of parts (a cons string, made a forward to the
    // internalized one once it is a key).
    for (source, text) in [
        ("'class-name'", "class-name"),
        ("String.fromCharCode(105, 100)", "id"),
        (
            "var key = 'thin-key-' + 'abc'.repeat(3); ({})[key] = 1; key",
            "thin-key-abcabcabc",
        ),
    ] {
        let string = LentString::new(eval(scope, source));
        assert!(string.lends(scope), "{source}");
        assert_eq!(borrowed_texts(), before, "asking lends nothing");
        assert_eq!(string.ascii(scope), Some(text), "{source}");
        assert_eq!(string.ascii(scope), Some(text), "{source}, again");
        assert_eq!(
            borrowed_texts(),
            before + 1,
            "lent once, however often read"
        );
        drop(string);
        assert_eq!(borrowed_texts(), before);
    }
}

/// A peek lends the text for its call alone: counted while it runs, not
/// after; none for a string that lends nothing.
#[test]
fn a_peek_lends_for_its_call() {
    let mut data = V8PerIsolateData::new();
    let isolate = data.isolate();
    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let before = borrowed_texts();
    let string = LentString::new(eval(scope, "'div'"));
    let peeked = string.peek_ascii(scope, |text| (text.to_owned(), borrowed_texts()));
    assert_eq!(peeked, Some(("div".to_owned(), before + 1)));
    assert_eq!(borrowed_texts(), before, "lent no longer");
    let other = LentString::new(eval(scope, "'文字'"));
    assert_eq!(other.peek_ascii(scope, |_| ()), None);
    assert_eq!(borrowed_texts(), before);
}

/// A string found ASCII (`lends`, as an argument is converted) lends its
/// characters after with no scan again: where they are is read anew, as a
/// GC may have moved them.
#[test]
fn a_string_is_scanned_for_ascii_once() {
    let mut data = V8PerIsolateData::new();
    let isolate = data.isolate();
    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let scans = bindings::ascii_scans_for_testing().expect("a debug build");
    let string = LentString::new(eval(scope, "'scanned-once'"));
    assert!(string.lends(scope));
    assert!(string.lends(scope));
    assert_eq!(string.ascii(scope), Some("scanned-once"));
    assert_eq!(bindings::ascii_scans_for_testing().unwrap() - scans, 1);
    // One not ASCII is scanned each time, and lends nothing.
    let other = LentString::new(eval(scope, "'caf\\u00e9'"));
    assert!(!other.lends(scope));
    assert_eq!(other.ascii(scope), None);
    assert_eq!(bindings::ascii_scans_for_testing().unwrap() - scans, 3);
}

#[test]
fn another_string_lends_nothing() {
    let mut data = V8PerIsolateData::new();
    let isolate = data.isolate();
    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let before = borrowed_texts();
    for source in [
        // Two-byte; one-byte but Latin-1; a cons string not yet flat.
        "'文字'",
        "'caf\\u00e9'",
        "'cons-string-' + String.fromCharCode(97).repeat(20)",
    ] {
        let string = LentString::new(eval(scope, source));
        assert!(!string.lends(scope), "{source}");
        assert_eq!(string.ascii(scope), None, "{source}");
        drop(string);
        assert_eq!(borrowed_texts(), before, "{source}");
    }
}

/// An isolate machopo didn't make (`V8PerIsolateData` gives its isolates the
/// GC prologue that checks loans) lends nothing.
#[test]
fn an_unguarded_isolate_lends_nothing() {
    bindings::initialize();
    let mut isolate = v8::Isolate::new(v8::CreateParams::default());
    v8::scope!(let scope, &mut isolate);
    let string = LentString::new(v8::String::new(scope, "ascii").unwrap());
    assert!(!string.lends(scope));
    assert_eq!(string.ascii(scope), None);
}

/// A GC while a text is lent aborts (in a process of its own); once the
/// string drops, GCs run.
#[test]
fn a_gc_while_a_text_is_lent_aborts() {
    if std::env::var_os("MACHOPO_LENT_GC").is_some() {
        let mut data = V8PerIsolateData::new();
        let isolate = data.isolate();
        v8::scope!(let scope, isolate);
        let string = LentString::new(v8::String::new(scope, "lent").unwrap());
        assert_eq!(string.ascii(scope), Some("lent"));
        if std::env::var_os("MACHOPO_LENT_GC_DROP").is_some() {
            drop(string);
        }
        scope.request_garbage_collection_for_testing(v8::GarbageCollectionType::Minor);
        return;
    }
    let run = |drop_first: bool| {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "v8_text::a_gc_while_a_text_is_lent_aborts",
                "--nocapture",
            ])
            .env("MACHOPO_LENT_GC", "1");
        if drop_first {
            command.env("MACHOPO_LENT_GC_DROP", "1");
        }
        command.output().unwrap()
    };
    let lent = run(false);
    assert!(!lent.status.success());
    assert!(
        String::from_utf8_lossy(&lent.stderr)
            .contains("V8 collects garbage while a string's text is borrowed in place"),
        "{}",
        String::from_utf8_lossy(&lent.stderr)
    );
    let dropped = run(true);
    assert!(
        dropped.status.success(),
        "{}",
        String::from_utf8_lossy(&dropped.stderr)
    );
}
