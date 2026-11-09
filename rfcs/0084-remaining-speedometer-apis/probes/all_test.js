const log = [];
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
