//! `MutationObserver` through its members: its constructor, the options'
//! checks, the records taken and delivered.

use std::rc::Rc;

use dom::{ObjectRoot, QualName};
use dom_api::MemberKind;

use crate::{Arg, Ret, TestDom, This, call_on, rc, with_host};

fn new_observer(dom: &mut TestDom, callback: u64) -> Rc<ObjectRoot> {
    match call_on(
        dom,
        This::None,
        "MutationObserver",
        "constructor",
        MemberKind::Constructor,
        &[Arg::Callback(callback)],
    )
    .0
    .unwrap()
    {
        Ret::Object(object) => Rc::new(object),
        other => panic!("{other:?}"),
    }
}

fn observe(
    dom: &mut TestDom,
    observer: &Rc<ObjectRoot>,
    target: Arg,
    options: Vec<(&'static str, Option<bool>)>,
) -> Result<Ret, dom_api::ApiError> {
    call_on(
        dom,
        This::Object(observer.clone()),
        "MutationObserver",
        "observe",
        MemberKind::Method,
        &[target, Arg::Dict(options)],
    )
    .0
}

#[test]
fn the_constructor_takes_a_function() {
    let mut dom = TestDom::new();
    let (result, _) = call_on(
        &mut dom,
        This::None,
        "MutationObserver",
        "constructor",
        MemberKind::Constructor,
        &[Arg::Str("f")],
    );
    assert!(matches!(result, Err(dom_api::ApiError::Type(_))));
}

#[test]
fn the_options_are_checked() {
    let mut dom = TestDom::new();
    let div = rc(Ret::Node(dom.mutate(|cx| {
        let doc = cx.create_document(true);
        cx.root(cx.create_element(doc, QualName::html("div")))
    })));
    let observer = new_observer(&mut dom, 1);
    let target = || Arg::Node(div.clone());
    for (options, ok) in [
        (vec![], false),
        (vec![("subtree", Some(true))], false),
        (vec![("childList", Some(true))], true),
        // An old value asked for implies what it is of.
        (vec![("attributeOldValue", Some(true))], true),
        (vec![("characterDataOldValue", Some(true))], true),
        (
            vec![
                ("attributes", Some(false)),
                ("attributeOldValue", Some(true)),
            ],
            false,
        ),
        (
            vec![
                ("characterData", Some(false)),
                ("characterDataOldValue", Some(true)),
            ],
            false,
        ),
        (
            vec![
                ("attributes", Some(false)),
                ("attributeOldValue", Some(false)),
                ("childList", Some(true)),
            ],
            true,
        ),
    ] {
        let result = observe(&mut dom, &observer, target(), options.clone());
        assert_eq!(result.is_ok(), ok, "{options:?}");
        if !ok {
            assert!(
                matches!(result, Err(dom_api::ApiError::Type(_))),
                "{options:?}"
            );
        }
    }
}

#[test]
fn records_are_taken_or_delivered() {
    let mut dom = TestDom::new();
    let div = rc(Ret::Node(dom.mutate(|cx| {
        let doc = cx.create_document(true);
        cx.root(cx.create_element(doc, QualName::html("div")))
    })));
    let observer = new_observer(&mut dom, 7);
    observe(
        &mut dom,
        &observer,
        Arg::Node(div.clone()),
        vec![("attributes", Some(true)), ("childList", Some(true))],
    )
    .unwrap();
    dom.mutate(|cx| {
        let div = div.get(cx.gc);
        cx.set_attribute(div, "a", "1");
        let p = cx.create_element(cx.document_of(div), QualName::html("p"));
        cx.append_child(div, p).unwrap();
    });
    match call_on(
        &mut dom,
        This::Object(observer.clone()),
        "MutationObserver",
        "takeRecords",
        MemberKind::Method,
        &[],
    )
    .0
    .unwrap()
    {
        Ret::Objects(records) => assert_eq!(records.len(), 2),
        other => panic!("{other:?}"),
    }
    // Taken: the delivery has none for it.
    with_host(&dom, |host| dom_api::deliver_mutations(host));
    assert!(dom.1.mutation_log.borrow().is_empty());
    dom.mutate(|cx| cx.set_attribute(div.get(cx.gc), "a", "2"));
    with_host(&dom, |host| dom_api::deliver_mutations(host));
    assert_eq!(
        *dom.1.mutation_log.borrow(),
        [(7, vec!["attributes".to_owned()])]
    );
    with_host(&dom, |host| dom_api::deliver_mutations(host));
    assert_eq!(dom.1.mutation_log.borrow().len(), 1, "nothing more");
}

/// An observer with transient registrations and no records isn't called.
#[test]
fn no_records_no_call() {
    let mut dom = TestDom::new();
    let (div, child) = dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let div = cx.create_element(doc, QualName::html("div"));
        let child = cx.create_element(doc, QualName::html("p"));
        cx.append_child(div, child).unwrap();
        (cx.root(div), cx.root(child))
    });
    let div = rc(Ret::Node(div));
    let observer = new_observer(&mut dom, 3);
    observe(
        &mut dom,
        &observer,
        Arg::Node(div.clone()),
        vec![("attributes", Some(true)), ("subtree", Some(true))],
    )
    .unwrap();
    dom.mutate(|cx| cx.remove(child.get(cx.gc)));
    with_host(&dom, |host| dom_api::deliver_mutations(host));
    assert!(dom.1.mutation_log.borrow().is_empty());
}
