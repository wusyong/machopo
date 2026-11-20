//! Web Storage's areas in the DOM: one copy of an area for every window
//! of it; what changes here queued for the host; the browser's news of
//! changes taken in, but not over a change made here and not told back
//! yet; `storage` events at the windows of the area but the source's.

use dom::{Dom, ElementTaskKind, StorageKind, StorageMutation};

fn set(key: &str, value: &str) -> StorageMutation {
    StorageMutation::Set {
        key: key.to_owned(),
        value: value.to_owned(),
    }
}

/// The `storage` events queued at each of `windows`: each event's key,
/// new value and kind.
fn events<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    windows: &[elise::Gc<'gc, dom::Window>],
) -> Vec<Vec<(Option<String>, Option<String>, StorageKind)>> {
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
        );
        let second = cx.set_window_storage(windows[1], StorageKind::Local, 1, None);
        assert_eq!(cx.storage_get(second, "a").as_deref(), Some("1"));
        cx.storage_set(first, "b", "2");
        cx.storage_set(first, "b", "2");
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
        let storage = cx.set_window_storage(window, StorageKind::Local, 3, Some(Vec::new()));
        cx.storage_set(storage, "mine", "here");
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
        let first = cx.set_window_storage(windows[0], StorageKind::Session, 5, Some(Vec::new()));
        cx.set_window_storage(windows[1], StorageKind::Session, 5, None);
        cx.set_window_storage(windows[2], StorageKind::Session, 6, Some(Vec::new()));
        cx.storage_set(first, "s", "1");
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
        );
        cx.storage_clear(storage);
        cx.storage_area_changed(4, set("b", "theirs"), None, true, String::new(), None);
        assert_eq!(cx.storage_length(storage), 0);
    });
}
