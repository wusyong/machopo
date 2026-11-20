//! What a frame's storage is kept by: its origin, the top-level site, and
//! whether a frame of another site is between them.

use site::{AncestorChainBit, Site, StorageKey};
use url::{Origin, Url};

fn origin(url: &str) -> Origin {
    Url::parse(url).unwrap().origin()
}

/// A top-level frame's key is its own origin's and site's, first party.
#[test]
fn a_top_level_frames_key_is_first_party() {
    let a = origin("https://www.a.example/");
    let key = StorageKey::first_party(&a);
    assert_eq!(key.origin(), &a);
    assert_eq!(key.top_level_site(), &Site::of(&a));
    assert_eq!(key.ancestor_chain_bit(), AncestorChainBit::SameSite);
    assert!(!key.is_third_party());
    assert_eq!(key, StorageKey::of_frame(&a, []));
}

/// A frame of the top-level site, another origin of it too, is first
/// party; one of another site is a third party's, and has a key of its own
/// for each top-level site it is in.
#[test]
fn a_frame_of_another_site_is_a_third_partys() {
    let top = origin("https://a.example/");
    let same_site = StorageKey::of_frame(&origin("https://sub.a.example/"), [&top]);
    assert!(!same_site.is_third_party());
    assert_eq!(same_site.top_level_site(), &Site::of(&top));
    let chat = origin("https://chat.example/");
    let in_a = StorageKey::of_frame(&chat, [&top]);
    assert!(in_a.is_third_party());
    assert_eq!(in_a.origin(), &chat);
    assert_eq!(in_a.top_level_site(), &Site::of(&top));
    let in_b = StorageKey::of_frame(&chat, [&origin("https://b.example/")]);
    assert_ne!(in_a, in_b);
    assert_ne!(in_a, StorageKey::first_party(&chat));
    assert_eq!(
        in_a,
        StorageKey::of_frame(&chat, [&origin("https://www.a.example/")])
    );
}

/// A frame of the top-level site in a frame of another site is a third
/// party's: that one can control it. Only the frames it is in count, the
/// top-level one's site the one compared with.
#[test]
fn a_frame_of_another_site_between_makes_it_a_third_partys() {
    let a = origin("https://a.example/");
    let b = origin("https://b.example/");
    let inner = StorageKey::of_frame(&a, [&b, &a]);
    assert_eq!(inner.ancestor_chain_bit(), AncestorChainBit::CrossSite);
    assert_eq!(inner.top_level_site(), &Site::of(&a));
    assert_ne!(inner, StorageKey::of_frame(&a, [&a]));
    let deep = StorageKey::of_frame(&a, [&origin("https://x.a.example/"), &a]);
    assert_eq!(deep.ancestor_chain_bit(), AncestorChainBit::SameSite);
}

/// An opaque origin's key, or one under an opaque top-level frame, is a
/// third party's.
#[test]
fn opaque_origins_are_third_parties() {
    let opaque = origin("data:text/html,a");
    assert!(StorageKey::first_party(&opaque).is_third_party());
    let a = origin("https://a.example/");
    let under_opaque = StorageKey::of_frame(&a, [&opaque]);
    assert!(under_opaque.is_third_party());
    assert!(under_opaque.top_level_site().is_opaque());
    assert!(StorageKey::of_frame(&opaque, [&a]).is_third_party());
}
