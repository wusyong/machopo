//! Web Storage's areas in the DOM: one copy of an area for every window
//! of it; what changes here queued for the host; the browser's news of
//! changes taken in, but not over a change made here and not told back
//! yet; `storage` events at the windows of the area but the source's.

use dom::{Dom, ElementTaskKind, StorageKind, StorageMutation};

/// The quota the host gives each area.
const QUOTA: usize = 10 * 1024 * 1024;

fn set(key: &str, value: &str) -> StorageMutation {
    StorageMutation::Set {
        key: key.to_owned(),
        value: value.to_owned(),
    }
}

/// A queued `storage` event's key, new value and kind.
type Event = (Option<String>, Option<String>, StorageKind);

/// The `storage` events queued at each of `windows`.
fn events<'gc>(cx: &dom::Cx<'_, 'gc>, windows: &[elise::Gc<'gc, dom::Window>]) -> Vec<Vec<Event>> {
    windows
        .iter()
        .map(|&window| {
            std::iter::from_fn(|| cx.take_element_task(cx.window_context(window)))
                .filter_map(|(_, kind, _)| match kind {
                    ElementTaskKind::Storage(info) => Some((info.key, info.new_value, info.kind)),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

/// Two windows of an area share its items: what one sets the other reads;
/// each change is queued for the host once, by its `Storage`, with its
/// document's URL; one that changes nothing isn't.
#[test]
fn windows_of_an_area_share_its_items() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let windows: Vec<_> = (0..2)
            .map(|_| {
                let document = cx.create_document(true);
                cx.set_document_url(document, "https://a.example/page");
                cx.create_window(document)
            })
            .collect();
        let first = cx.set_window_storage(
            windows[0],
            StorageKind::Local,
            1,
            Some(vec![("a".to_owned(), "1".to_owned())]),
            QUOTA,
        );
        let second = cx.set_window_storage(windows[1], StorageKind::Local, 1, None, QUOTA);
        assert_eq!(cx.storage_get(second, "a").as_deref(), Some("1"));
        assert!(cx.storage_set(first, "b", "2"));
        assert!(cx.storage_set(first, "b", "2"));
        cx.storage_remove(first, "missing");
        assert_eq!(cx.storage_keys(second), ["a", "b"]);
        let changes = cx.take_storage_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].area, 1);
        assert_eq!(changes[0].mutation, set("b", "2"));
        assert_eq!(changes[0].url, "https://a.example/page");
        // A `localStorage` change fires nothing until the browser tells
        // it back.
        assert_eq!(events(cx, &windows), [vec![], vec![]]);
        cx.storage_area_changed(
            1,
            set("b", "2"),
            None,
            true,
            "https://a.example/page".to_owned(),
            Some(changes[0].source),
        );
        assert_eq!(
            events(cx, &windows),
            [
                vec![],
                vec![(
                    Some("b".to_owned()),
                    Some("2".to_owned()),
                    StorageKind::Local
                )]
            ]
        );
    });
}

/// An item may grow only within the area's quota, 2 bytes a UTF-16 code
/// unit of its key and value: past it nothing changes and nothing is
/// queued; one that doesn't grow always may.
#[test]
fn an_item_grows_within_the_areas_quota() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let window = cx.create_window(cx.create_document(true));
        // 20 bytes: "ab" and "😀😀😀😀" (four characters, eight code units).
        let storage = cx.set_window_storage(window, StorageKind::Local, 7, Some(Vec::new()), 20);
        assert!(cx.storage_set(storage, "ab", "😀😀😀😀"));
        assert!(!cx.storage_set(storage, "c", ""));
        assert!(!cx.storage_set(storage, "ab", "😀😀😀😀x"));
        assert_eq!(cx.storage_get(storage, "ab").as_deref(), Some("😀😀😀😀"));
        assert_eq!(cx.take_storage_changes().len(), 1);
        // Smaller, or the same size, always.
        assert!(cx.storage_set(storage, "ab", "😀"));
        assert!(cx.storage_set(storage, "c", "d"));
        assert!(cx.storage_set(storage, "ab", "x"));
        // A removed item's bytes are free again.
        cx.storage_remove(storage, "ab");
        assert!(cx.storage_set(storage, "e", "1234567"));
        assert!(!cx.storage_set(storage, "f", ""));
        cx.storage_clear(storage);
        assert!(cx.storage_set(storage, "g", "123456789"));
        // Items past the quota as the browser gave them (its allowance),
        // 30 bytes of 20: one may still shrink, none grow.
        let over = cx.set_window_storage(
            cx.create_window(cx.create_document(true)),
            StorageKind::Local,
            9,
            Some(vec![("k".to_owned(), "01234567890123".to_owned())]),
            20,
        );
        assert!(!cx.storage_set(over, "k", "012345678901234"));
        // 26 bytes, past the quota still, but smaller.
        assert!(cx.storage_set(over, "k", "012345678901"));
    });
}

/// A change the browser refused: the first made here not told back goes,
/// and its key is the browser's value again, unless a later change made
/// here is of it.
#[test]
fn a_refused_change_takes_the_browsers_value() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let window = cx.create_window(cx.create_document(true));
        let storage = cx.set_window_storage(
            window,
            StorageKind::Local,
            8,
            Some(vec![("k".to_owned(), "old".to_owned())]),
            QUOTA,
        );
        assert!(cx.storage_set(storage, "k", "big"));
        assert!(cx.storage_set(storage, "j", "1"));
        assert!(cx.storage_set(storage, "j", "2"));
        cx.storage_change_failed(8, "k", Some("old".to_owned()));
        assert_eq!(cx.storage_get(storage, "k").as_deref(), Some("old"));
        // The next refused is `j`'s first: its second is still to come.
        cx.storage_change_failed(8, "j", None);
        assert_eq!(cx.storage_get(storage, "j").as_deref(), Some("2"));
        cx.storage_change_failed(8, "j", None);
        assert_eq!(cx.storage_get(storage, "j"), None);
        // Another key's news is taken: nothing of it is pending.
        cx.storage_area_changed(8, set("k", "theirs"), None, true, String::new(), None);
        assert_eq!(cx.storage_get(storage, "k").as_deref(), Some("theirs"));
    });
}

/// Another renderer's change of a key changed here, and not told back
/// yet, isn't taken: the browser made it before this one's, which wins.
/// One of another key is; a clear of theirs leaves the keys changed here.
/// Each fires `storage` at every window of the area; one that changed
/// nothing fires none.
#[test]
fn a_change_made_here_wins_over_one_the_browser_made_before() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        let storage = cx.set_window_storage(window, StorageKind::Local, 3, Some(Vec::new()), QUOTA);
        assert!(cx.storage_set(storage, "mine", "here"));
        let theirs = |mutation, changed| {
            cx.storage_area_changed(3, mutation, None, changed, String::new(), None);
        };
        theirs(set("mine", "theirs"), true);
        theirs(set("other", "theirs"), true);
        theirs(set("quiet", "x"), false);
        assert_eq!(cx.storage_get(storage, "mine").as_deref(), Some("here"));
        assert_eq!(cx.storage_get(storage, "other").as_deref(), Some("theirs"));
        theirs(StorageMutation::Clear, true);
        assert_eq!(cx.storage_keys(storage), ["mine"]);
        // Told back: theirs is taken from now on.
        let source = cx.take_storage_changes()[0].source;
        cx.storage_area_changed(
            3,
            set("mine", "here"),
            None,
            true,
            String::new(),
            Some(source),
        );
        theirs(set("mine", "later"), true);
        assert_eq!(cx.storage_get(storage, "mine").as_deref(), Some("later"));
        let kinds: Vec<_> = events(cx, &[window])[0]
            .iter()
            .map(|(key, _, _)| key.clone())
            .collect();
        assert_eq!(
            kinds,
            [
                Some("mine".to_owned()),
                Some("other".to_owned()),
                None,
                Some("mine".to_owned())
            ]
        );
    });
}

/// A `sessionStorage` change fires `storage` at once at the other windows
/// of the area, none of `localStorage`'s area of the same number.
#[test]
fn a_session_storage_change_fires_at_once() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let windows: Vec<_> = (0..3)
            .map(|_| cx.create_window(cx.create_document(true)))
            .collect();
        let first =
            cx.set_window_storage(windows[0], StorageKind::Session, 5, Some(Vec::new()), QUOTA);
        cx.set_window_storage(windows[1], StorageKind::Session, 5, None, QUOTA);
        cx.set_window_storage(windows[2], StorageKind::Session, 6, Some(Vec::new()), QUOTA);
        assert!(cx.storage_set(first, "s", "1"));
        assert_eq!(
            events(cx, &windows),
            [
                vec![],
                vec![(
                    Some("s".to_owned()),
                    Some("1".to_owned()),
                    StorageKind::Session
                )],
                vec![]
            ]
        );
        // The browser's news is of `localStorage` areas alone.
        cx.storage_area_changed(5, set("x", "y"), None, true, String::new(), None);
        assert_eq!(cx.storage_get(first, "x"), None);
        assert_eq!(events(cx, &windows), [vec![], vec![], vec![]]);
    });
}

/// A `storage` listener on a window (an event handler too), not on
/// another target, asks for its areas to be opened.
#[test]
fn a_storage_listener_asks_for_its_windows_areas() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        cx.add_listener(
            dom::TargetRef::Node(document),
            dom::names::event::STORAGE,
            dom::ListenerFlags::default(),
        );
        cx.add_listener(
            dom::TargetRef::Window(window),
            dom::names::event::LOAD,
            dom::ListenerFlags::default(),
        );
        assert!(cx.take_storage_listened().is_empty());
        cx.add_listener(
            dom::TargetRef::Window(window),
            dom::names::event::STORAGE,
            dom::ListenerFlags::default(),
        );
        let listened = cx.take_storage_listened();
        assert_eq!(listened.len(), 1);
        assert!(elise::Gc::ptr_eq(listened[0].get(cx.gc), window));
    });
}

/// A clear made here, not told back yet, holds back every change of
/// another renderer's the browser made before it.
#[test]
fn a_clear_made_here_holds_back_anothers_changes() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let window = cx.create_window(cx.create_document(true));
        let storage = cx.set_window_storage(
            window,
            StorageKind::Local,
            4,
            Some(vec![("a".to_owned(), "1".to_owned())]),
            QUOTA,
        );
        cx.storage_clear(storage);
        cx.storage_area_changed(4, set("b", "theirs"), None, true, String::new(), None);
        assert_eq!(cx.storage_length(storage), 0);
    });
}
