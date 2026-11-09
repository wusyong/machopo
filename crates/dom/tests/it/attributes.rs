//! Attributes: setting, getting and removing by qualified name and by
//! namespace, their order, a change keeping the prefix, and values
//! interned.

use dom::{DomString, QualName, names};

const XLINK: &str = "http://www.w3.org/1999/xlink";

fn xlink(prefix: &'static str, local: &'static str) -> QualName<'static> {
    QualName {
        namespace: Some(XLINK),
        prefix: Some(prefix),
        local,
    }
}

#[test]
fn attributes_set_by_name_are_found_by_name_and_in_the_null_namespace() {
    let mut dom = dom::Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let div = cx.create_element(doc, QualName::html("div"));
        assert_eq!(cx.attribute_count(div), 0);
        assert_eq!(cx.get_attribute(div, "id"), None);
        cx.set_attribute(div, "id", "main");
        cx.set_attribute(div, "class", "a b");
        assert_eq!(cx.get_attribute(div, "id").unwrap(), "main");
        assert_eq!(cx.get_attribute_ns(div, None, "class").unwrap(), "a b");
        assert_eq!(cx.get_attribute_ns(div, Some(""), "class").unwrap(), "a b");
        assert_eq!(cx.get_attribute_ns(div, Some(XLINK), "class"), None);
        // No case folding here: the API lowercases in an HTML document.
        assert_eq!(cx.get_attribute(div, "ID"), None);
        // Setting again changes the value in place.
        cx.set_attribute(div, "id", "other");
        assert_eq!(cx.attribute_count(div), 2);
        let first = cx.attribute_at(div, 0).unwrap();
        assert_eq!(names::get(first.name.local), "id");
        assert_eq!(first.name.namespace, names::NONE);
        assert_eq!(first.value, "other");
        assert_eq!(cx.attribute_at(div, 1).unwrap().value, "a b");
        assert_eq!(cx.attribute_at(div, 2), None);
    });
}

#[test]
fn namespaced_attributes_keep_their_prefix_and_are_found_both_ways() {
    let mut dom = dom::Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let a = cx.create_element(doc, QualName::html("a"));
        cx.set_attribute_ns(a, xlink("xl", "href"), "#one");
        cx.set_attribute(a, "href", "plain");
        assert_eq!(cx.attribute_count(a), 2);
        assert_eq!(cx.get_attribute_ns(a, Some(XLINK), "href").unwrap(), "#one");
        assert_eq!(cx.get_attribute_ns(a, None, "href").unwrap(), "plain");
        // By qualified name: the prefix, a colon and the local name.
        assert_eq!(cx.get_attribute(a, "xl:href").unwrap(), "#one");
        assert_eq!(cx.get_attribute(a, "href").unwrap(), "plain");
        assert_eq!(cx.get_attribute(a, "other:href"), None);
        assert_eq!(cx.get_attribute(a, "xl:other"), None);
        // Setting it again with another prefix changes the value, not the
        // prefix.
        cx.set_attribute_ns(a, xlink("other", "href"), "#two");
        assert_eq!(cx.attribute_count(a), 2);
        let attribute = cx.attribute_at(a, 0).unwrap();
        assert_eq!(names::get(attribute.name.prefix), "xl");
        assert_eq!(attribute.name.namespace, names::XLINK_NAMESPACE);
        assert_eq!(attribute.value, "#two");
        // `setAttribute` with the qualified name finds it too.
        cx.set_attribute(a, "xl:href", "#three");
        assert_eq!(cx.attribute_count(a), 2);
        assert_eq!(
            cx.get_attribute_ns(a, Some(XLINK), "href").unwrap(),
            "#three"
        );
    });
}

#[test]
fn removed_attributes_are_gone_and_give_their_value() {
    let mut dom = dom::Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let div = cx.create_element(doc, QualName::html("div"));
        cx.set_attribute(div, "a", "1");
        cx.set_attribute_ns(div, xlink("x", "b"), "2");
        cx.set_attribute(div, "c", "3");
        assert_eq!(cx.remove_attribute(div, "a").unwrap(), "1");
        assert_eq!(cx.remove_attribute(div, "a"), None);
        assert_eq!(cx.remove_attribute_ns(div, Some(XLINK), "b").unwrap(), "2");
        assert_eq!(cx.remove_attribute_ns(div, Some(XLINK), "b"), None);
        assert_eq!(cx.remove_attribute_ns(div, None, "nothing"), None);
        assert_eq!(cx.attribute_count(div), 1);
        assert_eq!(cx.attribute_at(div, 0).unwrap().value, "3");
        assert_eq!(cx.remove_attribute(div, "c").unwrap(), "3");
        assert_eq!(cx.attribute_count(div), 0);
        // An element with no attributes yet.
        let empty = cx.create_element(doc, QualName::html("p"));
        assert_eq!(cx.remove_attribute(empty, "a"), None);
    });
}

/// Equal values are one string, on any element (an engine finds its own
/// string of a value by its address).
#[test]
fn equal_attribute_values_are_one_string() {
    let mut dom = dom::Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let [a, b] = ["a", "b"].map(|name| cx.create_element(doc, QualName::html(name)));
        cx.set_attribute(a, "class", "item");
        cx.set_attribute(b, "class", "item");
        cx.set_attribute(b, "title", "item");
        cx.set_attribute(b, "id", "other");
        let one = cx.get_attribute(a, "class").unwrap();
        assert!(DomString::ptr_eq(
            &one,
            &cx.get_attribute(b, "class").unwrap()
        ));
        assert!(DomString::ptr_eq(
            &one,
            &cx.get_attribute(b, "title").unwrap()
        ));
        assert!(!DomString::ptr_eq(
            &one,
            &cx.get_attribute(b, "id").unwrap()
        ));
        // A change interns too.
        cx.set_attribute(b, "id", "item");
        assert!(DomString::ptr_eq(&one, &cx.get_attribute(b, "id").unwrap()));
    });
    assert_eq!(dom.interned_values(), 2);
}

#[test]
#[should_panic(expected = "only an element has attributes")]
fn only_elements_have_attributes() {
    let mut dom = dom::Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let text = cx.create_text(doc, "t");
        cx.set_attribute(text, "a", "b");
    });
}

/// An element the parser's fast path makes with its attributes is the one
/// set one by one makes: the same attributes in order, and what their
/// change steps do (an id, classes, the subtree's bloom filter, an event
/// handler, an input's type, checkedness and value). The input is made of
/// its type, not a text input changed to it.
#[test]
fn an_element_made_with_its_attributes_is_one_given_them_one_by_one() {
    use dom::{InputKind, TargetRef};

    let mut dom = dom::Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let html = cx.create_element(doc, QualName::html("html"));
        cx.insert(doc, html, None);
        let id = names::intern("id");
        let made = |local: &str, attributes: &[(&str, &str)]| {
            let numbered: Vec<_> = attributes
                .iter()
                .map(|&(name, value)| (names::intern(name), value))
                .collect();
            let before = dom::input_type_changes_for_testing().expect("a debug build");
            let element =
                cx.create_html_element_with_attributes(doc, names::intern(local), &numbered);
            let changes = dom::input_type_changes_for_testing().unwrap() - before;
            let one_by_one = cx.create_element(doc, QualName::html(local));
            for &(name, value) in attributes {
                cx.set_attribute(one_by_one, name, value);
            }
            (element, one_by_one, changes)
        };
        let div_attributes = [
            ("id", "x"),
            ("class", "a  b"),
            ("onclick", "f()"),
            ("data-n", "1"),
        ];
        let (div, other, changes) = made("div", &div_attributes);
        assert_eq!(changes, 0);
        assert_eq!(cx.attribute_count(div), 4);
        for index in 0..4 {
            assert_eq!(cx.attribute_at(div, index), cx.attribute_at(other, index));
        }
        assert_eq!(cx.attribute_at(div, 0).unwrap().name.local, id);
        assert_eq!(cx.class_names(div), cx.class_names(other));
        assert_eq!(cx.class_names(div).len(), 2);
        assert_eq!(cx.subtree_bloom(div), cx.subtree_bloom(other));
        assert!(
            cx.event_handler(TargetRef::Node(div), names::event::CLICK)
                .is_some()
        );
        cx.insert(html, div, None);
        assert!(
            cx.element_by_id(doc, "x")
                .is_some_and(|found| elise::Gc::ptr_eq(found, div))
        );

        let input_attributes = [("checked", ""), ("type", "checkbox"), ("value", "v")];
        let (input, other, changes) = made("input", &input_attributes);
        assert_eq!(changes, 0, "made a checkbox");
        assert_eq!(cx.input_kind(input), Some(InputKind::Checkbox));
        assert_eq!(cx.input_kind(other), Some(InputKind::Checkbox));
        assert!(cx.checkedness(input) && cx.checkedness(other));
        assert_eq!(cx.input_value(input), cx.input_value(other));
        assert_eq!(cx.input_value(input), "v");
        // The one given its type after it was made changed to it.
        let before = dom::input_type_changes_for_testing().unwrap();
        let later = cx.create_element(doc, QualName::html("input"));
        cx.set_attribute(later, "type", "checkbox");
        assert_eq!(dom::input_type_changes_for_testing().unwrap() - before, 1);

        let (radio, _, _) = made("input", &[("type", "Radio"), ("name", "r")]);
        assert_eq!(cx.input_kind(radio), Some(InputKind::Radio));
        let (plain, _, _) = made("input", &[("value", "w")]);
        assert_eq!(cx.input_kind(plain), Some(InputKind::Text));
        assert_eq!(cx.input_value(plain), "w");
        let (empty, _, _) = made("span", &[]);
        assert_eq!(cx.attribute_count(empty), 0);
    });
}
