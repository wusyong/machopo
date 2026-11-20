//! `localStorage`: the profile's areas, each storage key's, which a
//! renderer opens once and keeps a copy of; on disk in the profile's
//! folder; and pages end to end.

use std::path::PathBuf;

use browser::{DomStorageContext, NetworkContext, NetworkContextParams, Profile, StoragePartition};
use net::test_server::Route;
use protocol::renderer::{OpenedStorageArea, RendererHostMsg, StorageChange};
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

/// What the context answers renderer `renderer`'s frame of `key`.
fn open(context: &DomStorageContext, renderer: u64, key: &StorageKey) -> Option<OpenedStorageArea> {
    let (answer, answered) = crossbeam_channel::bounded(1);
    context.open(
        renderer,
        key.clone(),
        Box::new(move |area| {
            let _ = answer.send(area);
        }),
    );
    answered.recv_timeout(TIMEOUT).expect("the context answers")
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
    context.change(1, first.area, set("b", "2"));
    context.change(1, first.area, set("a", "1"));
    context.change(1, first.area, set("c", "3"));
    context.change(
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
    context.change(2, first.area, set("x", "no"));
    context.change(1, first.area.wrapping_add(100), set("x", "no"));
    let second = open(&context, 2, &key).unwrap();
    assert_ne!(second.area, first.area);
    assert_eq!(second.items, items(&[("a", "1"), ("b", "2")]));
    context.change(2, second.area, StorageChange::Clear);
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
        context.change(1, area.area, set("k", &index.to_string()));
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
    context.change(1, area, set("k", "v"));
    context.forget(1);
    let again = open(&context, 1, &key).unwrap();
    assert_ne!(again.area, area);
    assert_eq!(again.items, items(&[("k", "v")]));
    // The old number is no area of the renderer's now.
    context.change(1, area, set("k", "old"));
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
        context.change(1, area, set("kept", "yes"));
        context.change(1, area, set("gone", "soon"));
        context.change(
            1,
            area,
            StorageChange::Remove {
                key: "gone".to_owned(),
            },
        );
        let opaque = open(&context, 1, &in_opaque).unwrap();
        context.change(1, opaque.area, set("kept", "no"));
        assert_eq!(
            open(&context, 2, &in_opaque).unwrap().items,
            items(&[("kept", "no")])
        );
        for index in 0..500 {
            context.change(1, area, set(&format!("many {index:03}"), "v"));
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
