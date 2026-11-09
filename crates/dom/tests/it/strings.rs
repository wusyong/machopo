//! The DOM's strings: what they know of themselves, equality by content and
//! by identity, and the thread's table of interned values, shared by its
//! DOMs, dropping those no attribute holds as it grows.

use dom::{Dom, DomString, QualName};

#[test]
fn a_string_knows_whether_it_is_ascii() {
    let ascii = DomString::new("plain text-1");
    let latin = DomString::new("café");
    let cjk = DomString::from(String::from("文字"));
    assert!(ascii.is_ascii());
    assert!(!latin.is_ascii());
    assert!(!cjk.is_ascii());
    assert!(DomString::new("").is_ascii());
    assert_eq!(latin.len(), 5);
    // UTF-16 code units: one each, two for what is past the BMP.
    assert_eq!(ascii.utf16_len(), 12);
    assert_eq!(latin.utf16_len(), 4);
    assert_eq!(cjk.utf16_len(), 2);
    assert_eq!(DomString::new("a😀b").utf16_len(), 4);
    assert!(DomString::new("").is_empty());
    assert!(!ascii.is_empty());
}

#[test]
fn strings_are_equal_by_content_and_the_same_by_identity() {
    let a = DomString::new("x");
    let b = DomString::new("x");
    let copy = a.clone();
    assert_eq!(a, b);
    assert!(!DomString::ptr_eq(&a, &b));
    assert!(DomString::ptr_eq(&a, &copy));
    assert_ne!(a, DomString::new("y"));
    assert_eq!(a, "x");
    assert_eq!(&*a.to_str(), "x");
    assert!(matches!(a.to_str(), std::borrow::Cow::Borrowed(_)));
    assert_eq!(a.to_string(), "x");
    // The address is a string's, its clones' too.
    assert_eq!(a.addr(), copy.addr());
    assert_ne!(a.addr(), b.addr());
    // A clone counts a reference; a drop takes it away.
    assert_eq!(a.reference_count(), 2);
    drop(copy);
    assert_eq!(a.reference_count(), 1);
    assert_eq!(format!("{a:?}"), "\"x\"");
}

/// Many values held: the table looks again only once it has twice what
/// it kept, so that interning costs a constant per value, not a walk of the
/// table each time.
#[test]
fn a_table_of_many_held_values_looks_again_only_at_twice_its_size() {
    let mut dom = Dom::new();
    let _kept = dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let element = cx.create_element(doc, QualName::html("div"));
        // At 64 the table looks, keeps all: it looks next at 128.
        for i in 0..100 {
            cx.set_attribute(element, &format!("a{i}"), &format!("v{i}"));
        }
        // Each replaced value stays until then.
        for i in 0..20 {
            cx.set_attribute(element, "changing", &format!("w{i}"));
        }
        cx.root(element)
    });
    assert_eq!(dom.interned_values(), 120);
}

/// The table keeps what attributes hold; what none holds goes when the
/// table grows past twice what it last kept (at least 64).
#[test]
fn values_no_attribute_holds_leave_the_table_as_it_grows() {
    let mut dom = Dom::new();
    let kept = dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let kept = cx.create_element(doc, QualName::html("div"));
        cx.set_attribute(kept, "id", "kept");
        let dropped = cx.create_element(doc, QualName::html("div"));
        for i in 0..40 {
            cx.set_attribute(dropped, &format!("a{i}"), &format!("v{i}"));
        }
        cx.root(kept)
    });
    assert_eq!(dom.interned_values(), 41);
    dom.collect();
    // Below the table's limit: nothing dropped yet.
    assert_eq!(dom.interned_values(), 41);
    dom.mutate(|cx| {
        let element = kept.get(cx.gc);
        for i in 0..40 {
            cx.set_attribute(element, "changing", &format!("w{i}"));
        }
    });
    // At 64 (w0 to w22 added), the table looked: the 40 values of the
    // collected element and the 22 replaced ones went; `kept` and w22 (which
    // `changing` still held) stayed. w23 to w39 came after: 2 + 17.
    assert_eq!(dom.interned_values(), 19);
    dom.mutate(|cx| {
        let element = kept.get(cx.gc);
        assert_eq!(cx.get_attribute(element, "id").unwrap(), "kept");
        assert_eq!(cx.get_attribute(element, "changing").unwrap(), "w39");
    });
}

/// Every DOM on a thread interns into its table: an equal value is one
/// string, whichever DOM's attribute holds it.
#[test]
fn a_threads_doms_share_its_values() {
    let value = || {
        let mut dom = Dom::new();
        let value = dom.mutate(|cx| {
            let doc = cx.create_document(true);
            let element = cx.create_element(doc, QualName::html("div"));
            cx.set_attribute(element, "title", "one of the thread's");
            cx.get_attribute(element, "title").unwrap()
        });
        (dom, value)
    };
    let (_a, a) = value();
    let (_b, b) = value();
    assert!(DomString::ptr_eq(&a, &b));
}
