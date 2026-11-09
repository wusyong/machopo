//! `CharacterData` on a text: its data (null is the empty string) and its
//! length in UTF-16 code units.

use crate::{Arg, Ret, TestDom, document, get, method, rc, set};

#[test]
fn a_texts_data_and_its_length_in_utf16() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let text = rc(method(
        &mut dom,
        &doc,
        "Document",
        "createTextNode",
        &[Arg::Str("abc")],
    )
    .unwrap());
    assert_eq!(
        get(&mut dom, &text, "CharacterData", "data").string(),
        "abc"
    );
    assert!(matches!(
        get(&mut dom, &text, "CharacterData", "length"),
        Ret::U32(3)
    ));
    // One code unit each, two past the BMP.
    set(&mut dom, &text, "CharacterData", "data", Arg::Str("é文😀")).unwrap();
    assert!(matches!(
        get(&mut dom, &text, "CharacterData", "length"),
        Ret::U32(4)
    ));
    assert_eq!(
        get(&mut dom, &text, "Node", "textContent").string(),
        "é文😀"
    );
    // `[LegacyNullToEmptyString]`.
    set(&mut dom, &text, "CharacterData", "data", Arg::Null).unwrap();
    assert_eq!(get(&mut dom, &text, "CharacterData", "data").string(), "");
    assert!(matches!(
        get(&mut dom, &text, "CharacterData", "length"),
        Ret::U32(0)
    ));
    // Undefined isn't null: it is a string.
    set(&mut dom, &text, "CharacterData", "data", Arg::Undefined).unwrap();
    assert_eq!(
        get(&mut dom, &text, "CharacterData", "data").string(),
        "undefined"
    );
}
