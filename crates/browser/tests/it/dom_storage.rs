//! `localStorage`: the profile's areas, each storage key's, which a
//! renderer opens once and keeps a copy of; on disk in the profile's
//! folder; and pages end to end.

use std::path::PathBuf;
use std::sync::Arc;

use browser::{
    DomStorageContext, NetworkContext, NetworkContextParams, Profile, StoragePartition, ToRenderer,
};
use net::test_server::Route;
use protocol::TabId;
use protocol::renderer::{
    OpenedStorageArea, RendererHostMsg, RendererMsg, StorageChange, StorageKind,
};
use site::StorageKey;
use url::{Origin, Url};

use crate::navigation::{TIMEOUT, next_message, server, tab_in};

/// A new, empty folder for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("browser-dom-storage")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn origin(url: &str) -> Origin {
    Url::parse(url).unwrap().origin()
}

fn first_party(url: &str) -> StorageKey {
    StorageKey::first_party(&origin(url))
}

/// A renderer as the context sees it: its number, and what it is told.
struct Renderer {
    id: u64,
    to: ToRenderer,
    told: crossbeam_channel::Receiver<RendererMsg>,
    /// What it was told before an answer it waited for.
    kept: std::cell::RefCell<Vec<RendererMsg>>,
}

impl Renderer {
    fn new(id: u64) -> Self {
        let (tell, told) = crossbeam_channel::unbounded();
        Self {
            id,
            to: Arc::new(move |msg| {
                let _ = tell.send(msg);
            }),
            told,
            kept: std::cell::RefCell::default(),
        }
    }

    /// What the context answers its frame of `key` in tab `tab`'s storage
    /// of `kind`; what it was told before, kept.
    fn open_kind(
        &self,
        context: &DomStorageContext,
        kind: StorageKind,
        tab: TabId,
        key: &StorageKey,
    ) -> Option<OpenedStorageArea> {
        context.open(self.id, self.to.clone(), kind, tab, key.clone(), 77);
        loop {
            match self
                .told
                .recv_timeout(TIMEOUT)
                .expect("the context answers")
            {
                RendererMsg::StorageAreaOpened {
                    request_id: 77,
                    area,
                } => return area,
                msg => self.kept.borrow_mut().push(msg),
            }
        }
    }

    fn open(&self, context: &DomStorageContext, key: &StorageKey) -> Option<OpenedStorageArea> {
        self.open_kind(context, StorageKind::Local, TabId(0), key)
    }

    /// The changes it has been told of so far: each `StorageAreaChanged`'s
    /// area, change, old value, whether it changed, and source.
    fn changes(&self) -> Vec<(u64, StorageChange, Option<String>, bool, Option<u64>)> {
        let kept: Vec<RendererMsg> = self.kept.borrow_mut().drain(..).collect();
        kept.into_iter()
            .chain(self.told.try_iter())
            .filter_map(|msg| match msg {
                RendererMsg::StorageAreaChanged {
                    area,
                    change,
                    old_value,
                    changed,
                    url,
                    source,
                } => {
                    assert_eq!(url, "https://a.example/page");
                    Some((area, change, old_value, changed, source))
                }
                _ => None,
            })
            .collect()
    }
}

/// What the context answers renderer `renderer`'s frame of `key`.
fn open(context: &DomStorageContext, renderer: u64, key: &StorageKey) -> Option<OpenedStorageArea> {
    Renderer::new(renderer).open(context, key)
}

/// Renderer `renderer`'s change, by its `Storage` 5 of a page.
fn change(context: &DomStorageContext, renderer: u64, area: u64, made: StorageChange) {
    context.change(renderer, area, made, 5, "https://a.example/page".to_owned());
}

fn set(key: &str, value: &str) -> StorageChange {
    StorageChange::Set {
        key: key.to_owned(),
        value: value.to_owned(),
    }
}

fn items(pairs: &[(&str, &str)]) -> Option<Vec<(String, String)>> {
    Some(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    )
}

/// A renderer gets an area once, with its items, and the same number
/// again without them; what it changes the next renderer's area has, in
/// the order of the keys; a change of an area that isn't the renderer's
/// changes nothing.
#[test]
fn a_renderer_gets_an_areas_items_once() {
    let context = DomStorageContext::new(&Profile::in_memory()).unwrap();
    let key = first_party("https://a.example/");
    let first = open(&context, 1, &key).unwrap();
    assert_eq!(first.items, items(&[]));
    change(&context, 1, first.area, set("b", "2"));
    change(&context, 1, first.area, set("a", "1"));
    change(&context, 1, first.area, set("c", "3"));
    change(
        &context,
        1,
        first.area,
        StorageChange::Remove {
            key: "c".to_owned(),
        },
    );
    assert_eq!(
        open(&context, 1, &key),
        Some(OpenedStorageArea {
            area: first.area,
            items: None
        })
    );
    // Not renderer 2's area, nor one there is.
    change(&context, 2, first.area, set("x", "no"));
    change(&context, 1, first.area.wrapping_add(100), set("x", "no"));
    let second = open(&context, 2, &key).unwrap();
    assert_ne!(second.area, first.area);
    assert_eq!(second.items, items(&[("a", "1"), ("b", "2")]));
    change(&context, 2, second.area, StorageChange::Clear);
    assert_eq!(open(&context, 3, &key).unwrap().items, items(&[]));
}

/// Each key has items of its own: another origin's, the same origin's
/// in another top-level site, a frame of another site between. An opaque
/// origin's key has no area.
#[test]
fn each_key_has_items_of_its_own() {
    let context = DomStorageContext::new(&Profile::in_memory()).unwrap();
    let chat = origin("https://chat.example/");
    let top = origin("https://a.example/");
    let keys = [
        StorageKey::first_party(&chat),
        StorageKey::of_frame(&chat, [&top]),
        StorageKey::of_frame(&chat, [&origin("https://b.example/")]),
        StorageKey::of_frame(&top, [&chat, &top]),
        StorageKey::first_party(&top),
    ];
    for (index, key) in keys.iter().enumerate() {
        let area = open(&context, 1, key).unwrap();
        assert_eq!(area.items, items(&[]), "{key:?}");
        change(&context, 1, area.area, set("k", &index.to_string()));
    }
    for (index, key) in keys.iter().enumerate() {
        assert_eq!(
            open(&context, 2, key).unwrap().items,
            items(&[("k", &index.to_string())])
        );
    }
    let opaque = Url::parse("data:text/html,").unwrap().origin();
    assert_eq!(open(&context, 1, &StorageKey::first_party(&opaque)), None);
}

/// A renderer that has forgotten its areas (its DOM replaced) gets the
/// items again; one that has gone gets them as a new one.
#[test]
fn a_renderer_that_forgot_its_areas_gets_them_again() {
    let context = DomStorageContext::new(&Profile::in_memory()).unwrap();
    let key = first_party("https://a.example/");
    let area = open(&context, 1, &key).unwrap().area;
    change(&context, 1, area, set("k", "v"));
    context.forget(1);
    let again = open(&context, 1, &key).unwrap();
    assert_ne!(again.area, area);
    assert_eq!(again.items, items(&[("k", "v")]));
    // The old number is no area of the renderer's now.
    change(&context, 1, area, set("k", "old"));
    context.forget(1);
    assert_eq!(open(&context, 1, &key).unwrap().items, items(&[("k", "v")]));
}

/// The profile's folder keeps the items: a new context of it has them,
/// once the one before has gone, which waits for its thread to write what
/// it was told (many changes, still being written as it goes). A key of
/// an opaque top-level site's isn't kept.
#[test]
fn the_profiles_folder_keeps_the_items() {
    let dir = scratch("keeps");
    let key = first_party("https://a.example/");
    let in_opaque = StorageKey::of_frame(
        &origin("https://a.example/"),
        [&Url::parse("data:text/html,").unwrap().origin()],
    );
    {
        let profile = Profile::open(&dir).unwrap();
        let context = DomStorageContext::new(&profile).unwrap();
        let area = open(&context, 1, &key).unwrap().area;
        change(&context, 1, area, set("kept", "yes"));
        change(&context, 1, area, set("gone", "soon"));
        change(
            &context,
            1,
            area,
            StorageChange::Remove {
                key: "gone".to_owned(),
            },
        );
        let opaque = open(&context, 1, &in_opaque).unwrap();
        change(&context, 1, opaque.area, set("kept", "no"));
        assert_eq!(
            open(&context, 2, &in_opaque).unwrap().items,
            items(&[("kept", "no")])
        );
        for index in 0..500 {
            change(&context, 1, area, set(&format!("many {index:03}"), "v"));
        }
    }
    assert!(dir.join("LocalStorage").is_file());
    let profile = Profile::open(&dir).unwrap();
    let context = DomStorageContext::new(&profile).unwrap();
    let kept = open(&context, 1, &key).unwrap().items.unwrap();
    assert_eq!(kept.len(), 501);
    assert_eq!(kept.first(), Some(&("kept".to_owned(), "yes".to_owned())));
    assert_eq!(kept.last(), Some(&("many 499".to_owned(), "v".to_owned())));
    let other = StorageKey::of_frame(
        &origin("https://a.example/"),
        [&Url::parse("data:text/html,").unwrap().origin()],
    );
    assert_eq!(open(&context, 1, &other).unwrap().items, items(&[]));
    drop(context);
    drop(profile);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every renderer with a `localStorage` area is told each change, in
/// order: the value before, whether it changed anything; the one that
/// made it with its `Storage`'s number. One that set a value it had, or
/// removed or cleared nothing, is told it changed nothing.
#[test]
fn each_renderer_with_an_area_is_told_its_changes() {
    let context = DomStorageContext::new(&Profile::in_memory()).unwrap();
    let key = first_party("https://a.example/");
    let (one, two, other) = (Renderer::new(1), Renderer::new(2), Renderer::new(3));
    let area = one.open(&context, &key).unwrap().area;
    let theirs = two.open(&context, &key).unwrap().area;
    other
        .open(&context, &first_party("https://b.example/"))
        .unwrap();
    change(&context, 1, area, set("k", "v"));
    change(&context, 1, area, set("k", "v"));
    change(&context, 1, area, set("k", "w"));
    change(
        &context,
        1,
        area,
        StorageChange::Remove {
            key: "k".to_owned(),
        },
    );
    change(
        &context,
        1,
        area,
        StorageChange::Remove {
            key: "k".to_owned(),
        },
    );
    change(&context, 1, area, StorageChange::Clear);
    // Its answer comes after the changes before it.
    two.open(&context, &key);
    let told = |area, source| {
        vec![
            (area, set("k", "v"), None, true, source),
            (area, set("k", "v"), Some("v".to_owned()), false, source),
            (area, set("k", "w"), Some("v".to_owned()), true, source),
            (
                area,
                StorageChange::Remove {
                    key: "k".to_owned(),
                },
                Some("w".to_owned()),
                true,
                source,
            ),
            (
                area,
                StorageChange::Remove {
                    key: "k".to_owned(),
                },
                None,
                false,
                source,
            ),
            (area, StorageChange::Clear, None, false, source),
        ]
    };
    assert_eq!(two.changes(), told(theirs, None));
    assert_eq!(one.changes(), told(area, Some(5)));
    assert_eq!(other.changes(), []);
}

/// Each tab has a `sessionStorage` of its own, a key's area in it apart
/// from its `localStorage`; no renderer is told its changes; it lasts as
/// long as its tab (a renderer launched again has it), not on disk.
#[test]
fn each_tab_has_a_session_storage_of_its_own() {
    let dir = scratch("session");
    let key = first_party("https://a.example/");
    {
        let profile = Profile::open(&dir).unwrap();
        let context = DomStorageContext::new(&profile).unwrap();
        let (one, two) = (Renderer::new(1), Renderer::new(2));
        let session = |renderer: &Renderer, tab| {
            renderer
                .open_kind(&context, StorageKind::Session, TabId(tab), &key)
                .unwrap()
        };
        let area = session(&one, 1).area;
        change(&context, 1, area, set("tab", "one"));
        assert_eq!(one.open(&context, &key).unwrap().items, items(&[]));
        assert_eq!(session(&two, 2).items, items(&[]));
        // Tab 1's renderer launched again.
        context.forget(1);
        let again = session(&Renderer::new(3), 1);
        assert_eq!(again.items, items(&[("tab", "one")]));
        assert_eq!(one.changes(), []);
        context.forget_namespace(TabId(1));
        assert_eq!(session(&Renderer::new(4), 1).items, items(&[]));
        assert_eq!(session(&Renderer::new(5), 2).items, items(&[]));
        let opaque = Url::parse("data:text/html,").unwrap().origin();
        assert_eq!(
            Renderer::new(6).open_kind(
                &context,
                StorageKind::Session,
                TabId(1),
                &StorageKey::first_party(&opaque)
            ),
            None
        );
        change(&context, 3, again.area, set("written", "no"));
    }
    let profile = Profile::open(&dir).unwrap();
    let context = DomStorageContext::new(&profile).unwrap();
    assert_eq!(
        Renderer::new(1)
            .open_kind(&context, StorageKind::Session, TabId(1), &key)
            .unwrap()
            .items,
        items(&[])
    );
    assert_eq!(open(&context, 1, &key).unwrap().items, items(&[]));
    drop(context);
    drop(profile);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The renderer's messages until the title is `done` (not empty), each
/// handed to the tab; the titles before it.
fn titles_until(
    web_contents: &mut browser::WebContents,
    host_rx: &crossbeam_channel::Receiver<browser::ContentsEvent>,
    done: impl Fn(&str) -> bool,
) -> String {
    loop {
        let msg = next_message(web_contents, host_rx, TIMEOUT).expect("renderer answers");
        web_contents.on_renderer_message(&msg);
        if let RendererHostMsg::UpdateTitle(title) = msg
            && done(&title)
        {
            return title;
        }
    }
}

fn partition(profile: &Profile) -> StoragePartition {
    StoragePartition::new(
        profile,
        NetworkContext::start(NetworkContextParams {
            user_agent: browser::user_agent(),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap()
}

/// The page's members read and change its area: `length`, `key()` in the
/// order of the keys, the items as named properties (not over the
/// members: an item `getItem` leaves the method), `setItem` turning values to strings, `removeItem`, `clear`;
/// the same `Storage` each time.
const MEMBERS: &str = r#"<!DOCTYPE html><script>
const s = localStorage;
s.setItem('b', 2);
s.a = 'one';
s.setItem('getItem', 'shadowed?');
const out = [
  s === localStorage, s instanceof Storage, s.length, s.key(0), s.key(1), s.key(2), s.key(9),
  s.getItem('b'), s.b, s.missing, s.getItem('missing'), typeof s.getItem,
  Object.keys(s).join(','), 'a' in s,
];
delete s.a;
s.removeItem('b');
out.push(s.length, s.getItem('getItem'));
s.clear();
out.push(s.length);
s.setItem('gone', 'soon');
s.setItem('kept', 'yes');
s.removeItem('gone');
document.title = JSON.stringify(out);
</script>"#;

/// A page's `localStorage`: its members on its area; another tab of the
/// profile, its renderer another, reads what it left; a page of another
/// origin doesn't; a page of an opaque origin's is denied.
#[test]
fn a_pages_local_storage_is_its_origins() {
    let first_server = server(&[
        ("/members", Route::ok("text/html", MEMBERS)),
        (
            "/read",
            Route::ok(
                "text/html",
                "<script>document.title = 'read ' + localStorage.getItem('kept') + ' ' + localStorage.length</script>",
            ),
        ),
    ]);
    let other = server(&[(
        "/read",
        Route::ok(
            "text/html",
            "<script>document.title = 'read ' + localStorage.getItem('kept') + ' ' + localStorage.length</script>",
        ),
    )]);
    let partition = partition(&Profile::in_memory());
    let (mut first, host_rx) = tab_in(partition.clone());
    first.navigate(first_server.url("/members"));
    assert_eq!(
        titles_until(&mut first, &host_rx, |title| title.starts_with('[')),
        r#"[true,true,3,"a","b","getItem",null,"2","2",null,null,"function","a,b,getItem",true,1,"shadowed?",0]"#
    );
    let (mut second, host_rx) = tab_in(partition.clone());
    second.navigate(first_server.url("/read"));
    assert_eq!(
        titles_until(&mut second, &host_rx, |title| title.starts_with("read")),
        "read yes 1"
    );
    second.navigate(other.url("/read"));
    assert_eq!(
        titles_until(&mut second, &host_rx, |title| title.starts_with("read")),
        "read null 0"
    );
    second.navigate(
        "data:text/html,<script>try { localStorage; document.title = 'allowed' } catch (e) { document.title = e.name }</script>",
    );
    assert_eq!(
        titles_until(&mut second, &host_rx, |title| !title.is_empty()),
        "SecurityError"
    );
    first.shutdown();
    second.shutdown();
}

/// Documents of one key in a renderer share its copy: what one changes
/// the other reads at once, through its own `Storage`; a child frame of
/// another origin has its own.
#[test]
fn documents_of_a_key_share_the_renderers_copy() {
    let other = server(&[(
        "/child",
        Route::ok(
            "text/html",
            "<script>localStorage.child = 'other'; parent.postMessage(localStorage.length, '*')</script>",
        ),
    )]);
    let page = format!(
        r#"<!DOCTYPE html>
<iframe srcdoc="<p>same origin"></iframe>
<script>
onload = () => {{
  // Opened by the parent first: the child's open is of the area the
  // renderer has.
  localStorage.setItem('from', 'parent');
  const child = frames[0].localStorage;
  const read = child.getItem('from');
  child.removeItem('from');
  const out = [child !== localStorage, read, localStorage.getItem('from')];
  onmessage = (e) => {{
    out.push(e.data, localStorage.getItem('child'));
    document.title = JSON.stringify(out);
  }};
  const iframe = document.createElement('iframe');
  iframe.src = '{}';
  document.body.append(iframe);
}};
</script>"#,
        other.url("/child")
    );
    let server = server(&[("/", Route::ok("text/html", page))]);
    let (mut web_contents, host_rx) = tab_in(partition(&Profile::in_memory()));
    web_contents.navigate(server.url("/"));
    assert_eq!(
        titles_until(&mut web_contents, &host_rx, |title| title.starts_with('[')),
        r#"[true,"parent",null,1,null]"#
    );
    web_contents.shutdown();
}

/// What a page leaves is there for the next machopo of the profile's
/// folder.
#[test]
fn a_pages_local_storage_lasts_in_the_profiles_folder() {
    let dir = scratch("page");
    let server = server(&[
        (
            "/write",
            Route::ok(
                "text/html",
                "<script>localStorage.visits = 1; document.title = 'written'</script>",
            ),
        ),
        (
            "/read",
            Route::ok(
                "text/html",
                "<script>document.title = 'visits ' + localStorage.visits</script>",
            ),
        ),
    ]);
    {
        let (mut web_contents, host_rx) = tab_in(partition(&Profile::open(&dir).unwrap()));
        web_contents.navigate(server.url("/write"));
        titles_until(&mut web_contents, &host_rx, |title| title == "written");
        web_contents.shutdown();
    }
    let (mut web_contents, host_rx) = tab_in(partition(&Profile::open(&dir).unwrap()));
    web_contents.navigate(server.url("/read"));
    assert_eq!(
        titles_until(&mut web_contents, &host_rx, |title| title
            .starts_with("visits")),
        "visits 1"
    );
    web_contents.shutdown();
    drop(web_contents);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The window of a frame that has gone has no `localStorage` (null, as
/// Chrome's with no frame): nothing is asked.
#[test]
fn a_gone_frames_window_has_no_local_storage() {
    let page = r#"<!DOCTYPE html><iframe srcdoc="<p>child"></iframe><script>
onload = () => {
  const child = frames[0];
  document.querySelector('iframe').remove();
  let result;
  try { result = String(child.localStorage); } catch (e) { result = e.name; }
  document.title = 'done ' + result;
};
</script>"#;
    let server = server(&[("/", Route::ok("text/html", page))]);
    let (mut web_contents, host_rx) = tab_in(partition(&Profile::in_memory()));
    web_contents.navigate(server.url("/"));
    assert_eq!(
        titles_until(&mut web_contents, &host_rx, |title| title
            .starts_with("done")),
        "done null"
    );
    web_contents.shutdown();
}

/// A page that writes what `storage` events its window gets to its title,
/// as JSON: each event's key, old and new value, URL's path, and which
/// storage it is of.
const LISTENER: &str = r#"<!DOCTYPE html><script>
const seen = [];
addEventListener('storage', (e) => {
  seen.push([e.key, e.oldValue, e.newValue, new URL(e.url).pathname,
    e.storageArea === localStorage ? 'local' : e.storageArea === sessionStorage ? 'session' : null,
    e instanceof StorageEvent, e.isTrusted, e.bubbles, e.cancelable]);
  document.title = JSON.stringify(seen);
});
document.title = 'listening';
</script>"#;

/// A `localStorage` change fires `storage` at the windows of its key in
/// another tab (with the key, the values, the URL of the page that made
/// it and its own `localStorage`), not at the window that made it; a
/// change that changes nothing fires none; a clear's key and values are
/// null.
#[test]
fn a_local_storage_change_fires_storage_in_another_tab() {
    let server = server(&[
        ("/listen", Route::ok("text/html", LISTENER)),
        (
            "/write",
            Route::ok(
                "text/html",
                r#"<!DOCTYPE html><script>
let own = 0;
addEventListener('storage', () => own++);
localStorage.k = 'v';
localStorage.k = 'v';
localStorage.setItem('k', 'w');
localStorage.removeItem('missing');
localStorage.clear();
setTimeout(() => document.title = 'own ' + own, 300);
</script>"#,
            ),
        ),
    ]);
    let partition = partition(&Profile::in_memory());
    let (mut listener, listener_rx) = tab_in(partition.clone());
    listener.navigate(server.url("/listen"));
    titles_until(&mut listener, &listener_rx, |title| title == "listening");
    let (mut writer, writer_rx) = tab_in(partition);
    writer.navigate(server.url("/write"));
    assert_eq!(
        titles_until(&mut writer, &writer_rx, |title| title.starts_with("own")),
        "own 0"
    );
    let event = |key: &str, old: &str, new: &str| {
        format!(r#"[{key},{old},{new},"/write","local",true,true,false,false]"#)
    };
    let expected = format!(
        "[{},{},{}]",
        event(r#""k""#, "null", r#""v""#),
        event(r#""k""#, r#""v""#, r#""w""#),
        event("null", "null", "null"),
    );
    assert_eq!(
        titles_until(&mut listener, &listener_rx, |title| title.len()
            >= expected.len()),
        expected
    );
    listener.shutdown();
    writer.shutdown();
}

/// Within a tab, a window of the key that never read its storage hears of
/// a change all the same, its `storage` listener (here a `<body
/// onstorage>`) having opened it: of `localStorage` through the browser,
/// of `sessionStorage` at once; the window that made the change doesn't.
#[test]
fn a_listener_opens_its_windows_storage_to_hear_of_changes() {
    let page = r#"<!DOCTYPE html>
<iframe srcdoc="<body onstorage=&quot;parent.seen.push([event.key, event.newValue, event.storageArea === localStorage ? 'local' : 'session'])&quot;>"></iframe>
<script>
window.seen = [];
let own = 0;
onstorage = () => own++;
onload = () => {
  localStorage.l = '1';
  sessionStorage.s = '2';
  setTimeout(() => document.title = JSON.stringify([seen, own]), 300);
};
</script>"#;
    let server = server(&[("/", Route::ok("text/html", page))]);
    let (mut web_contents, host_rx) = tab_in(partition(&Profile::in_memory()));
    web_contents.navigate(server.url("/"));
    let title = titles_until(&mut web_contents, &host_rx, |title| title.starts_with('['));
    // The session one at once, the local one once the browser told back.
    assert_eq!(title, r#"[[["s","2","session"],["l","1","local"]],0]"#);
    web_contents.shutdown();
}

/// A tab's `sessionStorage` is its own: its next page of the origin has
/// it, another tab of the profile doesn't (one opened after the change
/// too), and a change in it fires no `storage` in another tab.
#[test]
fn a_tabs_session_storage_is_its_own() {
    let server = server(&[
        ("/listen", Route::ok("text/html", LISTENER)),
        (
            "/write",
            Route::ok(
                "text/html",
                "<script>sessionStorage.mine = 'yes'; localStorage.done = '1'; document.title = 'written'</script>",
            ),
        ),
        (
            "/read",
            Route::ok(
                "text/html",
                "<script>document.title = 'read ' + sessionStorage.mine + ' ' + sessionStorage.length</script>",
            ),
        ),
    ]);
    let partition = partition(&Profile::in_memory());
    let partition_of_test = partition.clone();
    let (mut listener, listener_rx) = tab_in(partition.clone());
    listener.navigate(server.url("/listen"));
    titles_until(&mut listener, &listener_rx, |title| title == "listening");
    let (mut writer, writer_rx) = tab_in(partition.clone());
    writer.navigate(server.url("/write"));
    titles_until(&mut writer, &writer_rx, |title| title == "written");
    writer.navigate(server.url("/read"));
    assert_eq!(
        titles_until(&mut writer, &writer_rx, |title| title.starts_with("read")),
        "read yes 1"
    );
    // The listener hears of the `localStorage` change alone.
    assert_eq!(
        titles_until(&mut listener, &listener_rx, |title| title.starts_with('[')),
        r#"[["done",null,"1","/write","local",true,true,false,false]]"#
    );
    listener.navigate(server.url("/read"));
    assert_eq!(
        titles_until(&mut listener, &listener_rx, |title| title
            .starts_with("read")),
        "read undefined 0"
    );
    let (mut later, later_rx) = tab_in(partition);
    later.navigate(server.url("/read"));
    assert_eq!(
        titles_until(&mut later, &later_rx, |title| title.starts_with("read")),
        "read undefined 0"
    );
    listener.shutdown();
    let tab = writer.tab();
    writer.shutdown();
    later.shutdown();
    // The closed tab's `sessionStorage` is gone with it.
    let session = Renderer::new(1_000)
        .open_kind(
            partition_of_test.dom_storage(),
            StorageKind::Session,
            tab,
            &StorageKey::first_party(&Url::parse(&server.url("/")).unwrap().origin()),
        )
        .unwrap();
    assert_eq!(session.items, items(&[]));
}
