//! HTML's `document.all` (`HTMLAllCollection`) from script, as Chrome 152
//! has it: undetectable ([[IsHTMLDDA]]: `typeof` `"undefined"`, `== null`,
//! falsy, yet an object), callable as its `item`, the elements by index
//! (a string that is an array index too) and by name (any element's `id`,
//! some HTML elements' `name`; several, a live `HTMLCollection`), the same
//! object each time, on every kind of document.

use crate::env::Env;

/// The script `probes/all_test.js` runs in Chrome 152, which gives the
/// same but for the error messages, which are machopo's.
const SCRIPT: &str = r#"var log = [];
document.appendChild(document.createElement('html'));
document.documentElement.innerHTML = '<head></head><body><div id=a></div><img name=a><div name=b></div>' +
  '<form name=f id=g></form><p id=0x></p><span id=5></span><svg><circle id=c></circle></svg>' +
  '<meta name=m><input name=a></body>';
const all = document.all;
const show = v => v === undefined ? 'undefined' : v === null ? 'null'
  : v === all ? 'all'
  : v instanceof HTMLCollection ? `HTMLCollection(${[...v].map(e => e.localName).join(',')})`
  : v instanceof Element ? v.localName + (v.id ? '#' + v.id : '')
  : String(v);
const attempt = f => { try { return show(f()); } catch (e) { return `${e.name}: ${e.message}`; } };
log.push(
  typeof all, all == null, all == undefined, all === undefined, all === null, !all, Boolean(all),
  all ? 'truthy' : 'falsy', show(all ?? 1), all?.length, all === document.all,
  Object.prototype.toString.call(all), String(all), all instanceof HTMLCollection,
  Object.getPrototypeOf(all) === HTMLAllCollection.prototype,
  Object.getOwnPropertyNames(HTMLAllCollection.prototype).join(),
  all.item.length, all.namedItem.length, typeof all.item);
log.push(...[
  () => all[0], () => all(0), () => all('0'), () => all.item(0), () => all.item('3'),
  () => all.item(), () => all(), () => all(null), () => all(undefined), () => all.item(-1),
  () => all.item(1e10), () => all.item(4294967295), () => all.item('5'), () => all['5'],
  () => all('a'), () => all.a, () => all.b, () => all.f, () => all.g, () => all['0x'],
  () => all.c, () => all.m, () => all.nope, () => all.item('nope'), () => all.namedItem('nope'),
  () => all.namedItem(''), () => all.namedItem(null), () => all.namedItem(),
  () => all.a === all.a, () => all.item('a') === all.namedItem('a'),
  () => new all(0), () => Function.prototype.call.call(all, null, 1),
  () => Reflect.apply(all, undefined, ['g']), () => [...all].length,
  () => Object.keys(all).length, () => 'a' in all, () => 'b' in all,
  () => HTMLAllCollection.prototype.item.call(document.forms, 0),
  () => new HTMLAllCollection(),
].map(attempt));
const a = all.a, before = all.length;
document.body.append(Object.assign(document.createElement('i'), {id: 'a'}));
log.push(all.length - before, a.length, show(document.implementation.createHTMLDocument().all.length),
  typeof new Document().all, new Document().all.length);
log.join(' / ')"#;

#[test]
fn document_all() {
    let mut env = Env::window("about:blank");
    assert_eq!(
        env.eval(SCRIPT),
        "undefined / true / true / false / false / true / false / falsy / all / 13 / true / \
         [object HTMLAllCollection] / [object HTMLAllCollection] / false / true / \
         length,item,namedItem,constructor / 0 / 1 / function / html / html / html / html / \
         div#a / null / null / null / null / null / null / null / div / div / \
         HTMLCollection(div,img,input) / HTMLCollection(div,img,input) / undefined / \
         form#g / form#g / p#0x / circle#c / meta / undefined / null / null / null / null / \
         TypeError: Failed to execute 'namedItem': 1 argument required, but only 0 present. / \
         true / true / TypeError: all is not a constructor / head / form#g / 13 / 13 / true / \
         false / TypeError: Illegal invocation / TypeError: Illegal constructor / 1 / 4 / 3 / \
         undefined / 0"
    );
    // The empty string names the elements whose `id` or `name` is empty
    // (Chrome 152's `probes/all3.html`), unlike `HTMLCollection`'s.
    assert_eq!(
        env.eval(
            "log = [];
             document.body.innerHTML = '<a name></a><div name></div>';
             const name = v => v === null ? 'null' : v === undefined ? 'undefined' : v.localName;
             log.push(...[all.namedItem(''), all.item(''), all[''], all('')].map(name));
             document.querySelector('a').remove();
             document.body.append(Object.assign(document.createElement('p'), {id: ''}));
             document.body.append(Object.assign(document.createElement('p'), {id: 'x'}));
             log.push(name(all.namedItem('')), name(document.getElementsByTagName('p').namedItem('')));
             log.join(' / ')"
        ),
        "a / a / a / a / p / null"
    );
}

/// Only `document.all` is callable: other collections' and lists' `item`
/// is no legacy caller.
#[test]
fn other_collections_are_not_callable() {
    let mut env = Env::window("about:blank");
    assert_eq!(
        env.eval(
            "document.appendChild(document.createElement('html'));
             const kinds = [document.children, document.childNodes, document.forms,
                            document.documentElement.classList, document.styleSheets];
             kinds.map(v => typeof v + ' ' + (() => { try { v(0); return 'called'; }
                                                       catch (e) { return e.name; } })()).join(' / ')"
        ),
        "object TypeError / object TypeError / object TypeError / object TypeError / object TypeError"
    );
}
