//! The names: the fixed ones (the DOM's own, the HTML, SVG and MathML local
//! names), found and read with no table, and the others in the thread's
//! table, one for every DOM on the thread.

use dom::names::{self, FIXED_COUNT, NameId, local};
use dom::{Dom, NameIds, QualName, QualifiedName};

/// Runs `f` on a thread of its own: a table no other test touched.
fn on_new_thread<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
    std::thread::spawn(f).join().unwrap()
}

/// Every fixed number reads its name back, and finds it.
#[test]
fn every_fixed_name_is_found_by_its_text() {
    for id in 0..NameId::try_from(FIXED_COUNT).unwrap() {
        let name = names::get(id);
        assert_eq!(names::find(&name), Some(id), "{name}");
        assert_eq!(names::fixed(&name), Some(id), "{name}");
    }
}

/// A name one byte off a fixed one, or one longer or shorter, isn't found
/// as it: the perfect hash's slot holds a fixed name, compared whole.
#[test]
fn a_name_off_a_fixed_one_isnt_it() {
    for id in 1..NameId::try_from(FIXED_COUNT).unwrap() {
        let name = String::from(&*names::get(id));
        for at in 0..name.len() {
            let mut off = name.clone().into_bytes();
            off[at] = off[at].wrapping_add(1);
            if let Ok(off) = String::from_utf8(off) {
                assert_ne!(names::find(&off), Some(id), "{off} for {name}");
            }
        }
        assert_ne!(names::find(&format!("{name}x")), Some(id));
        assert_ne!(names::find(&name[..name.len() - 1]), Some(id));
    }
}

/// The constants are their names', the DOM's own at the numbers they had.
#[test]
fn constants_name_their_names() {
    for (id, name) in [
        (names::NONE, ""),
        (names::HTML_NAMESPACE, "http://www.w3.org/1999/xhtml"),
        (names::XMLNS_NAMESPACE, "http://www.w3.org/2000/xmlns/"),
        (names::ID, "id"),
        (names::CLASS, "class"),
        (local::TEMPLATE, "template"),
        (names::CONTENT, "content"),
        (local::DIV, "div"),
        (local::STAR, "*"),
        (local::ACCEPT_CHARSET, "accept-charset"),
        (local::XLINK_HREF, "xlink:href"),
        // SVG's mixed case, and the attribute that would be the element's.
        (local::CLIP_PATH, "clipPath"),
        (local::CLIP_PATH_ATTR, "clip-path"),
        (local::SOLID_COLOR, "solidColor"),
        (local::SOLID_COLOR_ATTR, "solid-color"),
    ] {
        assert_eq!(names::get(id), name);
        assert_eq!(names::find(name), Some(id));
    }
    // Ids 0 to 16 as before, the local names after.
    assert_eq!(names::CONTENT, 16);
    assert!(usize::try_from(local::DIV).unwrap() > 16);
}

/// A name with no fixed number gets the next one past them, the same each
/// time; a fixed one takes none of the table's.
#[test]
fn other_names_get_numbers_past_the_fixed_ones() {
    on_new_thread(|| {
        assert_eq!(names::intern("div"), local::DIV);
        assert_eq!(names::intern("clip-path"), local::CLIP_PATH_ATTR);
        assert_eq!(names::find("x-widget"), None);
        let first = names::intern("x-widget");
        assert_eq!(usize::try_from(first).unwrap(), FIXED_COUNT);
        assert_eq!(names::intern("x-widget"), first);
        assert_eq!(names::find("x-widget"), Some(first));
        assert_eq!(names::intern("x-other"), first + 1);
        assert_eq!(names::get(first), "x-widget");
        // Its string is one, given again.
        let string = names::string(first);
        assert_eq!(string, "x-widget");
        assert!(dom::DomString::ptr_eq(&string, &names::string(first)));
        // So is a fixed one's, made the first time.
        assert!(dom::DomString::ptr_eq(
            &names::string(local::SPAN),
            &names::string(local::SPAN)
        ));
        assert_eq!(names::string(local::SPAN), "span");
    });
}

/// Every DOM on a thread shares its names; another thread has its own.
#[test]
fn a_threads_doms_share_its_names() {
    let make = || {
        let mut dom = Dom::new();
        let id = dom.mutate(|cx| {
            let doc = cx.create_document(true);
            let element = cx.create_element(doc, QualName::html("x-shared"));
            cx.element_name(element).unwrap().local
        });
        (dom, id)
    };
    let (_a, a) = make();
    let (_b, b) = make();
    assert_eq!(a, b);
    assert_eq!(names::find("x-shared"), Some(a));
    on_new_thread(|| assert_eq!(names::find("x-shared"), None));
}

/// A fixed local name's uppercased one (an HTML element's `tagName`) is
/// fixed too: no number of the table's taken.
#[test]
fn a_fixed_names_node_name_is_fixed() {
    on_new_thread(|| {
        let mut dom = Dom::new();
        dom.mutate(|cx| {
            let doc = cx.create_document(true);
            for local in ["p", "div", "annotation-xml", "foreignObject"] {
                let element = cx.create_element(doc, QualName::html(local));
                let id = cx.node_name_id(element);
                assert_eq!(&*names::get(id), local.to_ascii_uppercase());
                assert_eq!(names::find(&local.to_ascii_uppercase()), Some(id));
                assert!(usize::try_from(id).unwrap() < FIXED_COUNT, "{local}");
            }
        });
        assert_eq!(
            usize::try_from(names::intern("x-first")).unwrap(),
            FIXED_COUNT
        );
    });
}

/// A fixed name that isn't a local name of the list (an event type, a
/// dictionary's member) has no uppercased one fixed: an HTML element so
/// named has its `tagName` uppercased all the same, a name of the table.
#[test]
fn a_fixed_name_that_is_no_local_name_is_uppercased_too() {
    on_new_thread(|| {
        let mut dom = Dom::new();
        dom.mutate(|cx| {
            let doc = cx.create_document(true);
            for local in ["click", "bubbles", "c"] {
                assert!(names::fixed(local).is_some(), "{local}");
                let element = cx.create_element(doc, QualName::html(local));
                let id = cx.node_name_id(element);
                assert_eq!(&*names::get(id), local.to_ascii_uppercase());
                assert!(usize::try_from(id).unwrap() >= FIXED_COUNT, "{local}");
            }
        });
    });
}

/// `nodeName` that isn't the local name is made once, a name of the table.
#[test]
fn an_uppercased_node_name_is_made_once() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let one = cx.create_element(doc, QualName::html("x-name"));
        let two = cx.create_element(doc, QualName::html("x-name"));
        let id = cx.node_name_id(one);
        assert_eq!(cx.node_name_id(two), id);
        assert_eq!(names::get(id), "X-NAME");
        assert_eq!(names::find("X-NAME"), Some(id));
    });
}

/// A name with no prefix in no namespace or the HTML, SVG or MathML one is
/// its qualified name's bits, a fixed local name or not: made and read
/// with no table.
#[test]
fn a_plain_qualified_name_is_in_its_bits() {
    on_new_thread(|| {
        let custom = names::intern("x-custom");
        for namespace in [
            names::NONE,
            names::HTML_NAMESPACE,
            names::SVG_NAMESPACE,
            names::MATHML_NAMESPACE,
        ] {
            for local in [local::DIV, custom] {
                let ids = NameIds {
                    namespace,
                    prefix: names::NONE,
                    local,
                };
                let name = QualifiedName::new(ids);
                assert_eq!(name.ids(), ids);
                assert!(name.is(namespace, local));
                assert!(!name.is(names::XLINK_NAMESPACE, local));
                assert!(!name.is(namespace, local::SPAN));
            }
        }
        assert_eq!(names::tabled_qualified_names(), 0);
        assert_eq!(
            QualifiedName::html(local::DIV),
            QualifiedName::new(NameIds {
                namespace: names::HTML_NAMESPACE,
                prefix: names::NONE,
                local: local::DIV,
            })
        );
        assert_eq!(QualifiedName::NONE.ids().local, names::NONE);
    });
}

/// A name with a prefix, or in another namespace, is numbered in the
/// thread's table, once: the same name the same number. `is` compares its
/// namespace and local name, whatever its prefix.
#[test]
fn another_qualified_name_is_numbered_in_the_table() {
    on_new_thread(|| {
        let href = NameIds {
            namespace: names::XLINK_NAMESPACE,
            prefix: names::intern("xlink"),
            local: names::intern("href"),
        };
        let lang = NameIds {
            namespace: names::XML_NAMESPACE,
            prefix: names::NONE,
            local: names::intern("lang"),
        };
        let a = QualifiedName::new(href);
        let b = QualifiedName::new(lang);
        assert_eq!(names::tabled_qualified_names(), 2);
        assert_eq!(QualifiedName::new(href), a, "numbered once");
        assert_eq!(names::tabled_qualified_names(), 2);
        assert_ne!(a, b);
        assert_eq!((a.ids(), b.ids()), (href, lang));
        assert_eq!(a.prefix(), href.prefix);
        assert!(a.is(names::XLINK_NAMESPACE, href.local));
        assert!(!a.is(names::NONE, href.local));
        // A prefixed name in the HTML namespace too.
        let prefixed = NameIds {
            namespace: names::HTML_NAMESPACE,
            prefix: names::intern("h"),
            local: local::DIV,
        };
        let name = QualifiedName::new(prefixed);
        assert_eq!(name.ids(), prefixed);
        assert!(name.is(names::HTML_NAMESPACE, local::DIV));
        assert_ne!(name, QualifiedName::html(local::DIV));
    });
}

/// Elements and attributes keep their names as one number each: a prefixed
/// one reads back whole and is found by its qualified name and by its
/// namespace and local name.
#[test]
fn prefixed_names_on_nodes_read_back() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(false);
        let svg = QualName {
            namespace: Some("http://www.w3.org/2000/svg"),
            prefix: Some("s"),
            local: "rect",
        };
        let element = cx.create_element(doc, svg);
        let name = cx.element_name(element).unwrap();
        assert_eq!(
            (
                &*names::get(name.namespace),
                &*names::get(name.prefix),
                &*names::get(name.local)
            ),
            ("http://www.w3.org/2000/svg", "s", "rect")
        );
        assert_eq!(cx.node_name(element), "s:rect");
        let href = QualName {
            namespace: Some("http://www.w3.org/1999/xlink"),
            prefix: Some("xlink"),
            local: "href",
        };
        cx.set_attribute_ns(element, href, "#a");
        cx.set_attribute(element, "width", "1");
        let attribute = cx.attribute_at(element, 0).unwrap();
        assert_eq!(attribute.name.prefix, names::find("xlink").unwrap());
        assert_eq!(cx.get_attribute(element, "xlink:href").unwrap(), "#a");
        assert!(cx.get_attribute(element, "href").is_none());
        let found = cx.get_attribute_ns(element, Some("http://www.w3.org/1999/xlink"), "href");
        assert_eq!(found.unwrap(), "#a");
        assert_eq!(cx.get_attribute(element, "width").unwrap(), "1");
        assert!(
            cx.get_attribute_ns(element, Some("http://www.w3.org/1999/xlink"), "width")
                .is_none()
        );
    });
}
