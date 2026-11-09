// The page `bench.py speedometer --inventory` has machopo load to find out which
// of what Chrome saw the suites use machopo has: each recorded API key, CSS
// declaration, selector and kind of at-rule, and what `createElement` and
// `createElementNS` make. It reads the list from `window.__machopoProbe`
// (written into the page by the server) and posts what machopo lacks to the
// server.
(() => {
  "use strict";
  const { api, declarations, selectors, elements } = window.__machopoProbe;
  const atRules = window.__machopoProbe.atRules ?? {};
  const missing = { api: [], declarations: [], selectors: [], culprits: {}, atRules: [], elements: [] };

  // `I.m`, `I.m=`: on the interface's prototype (its chain) or the
  // interface itself (static), or on a namespace; `new I`: the interface;
  // `window.m`: the global's own.
  const has = (key) => {
    let m = /^new (.+)$/.exec(key);
    if (m) return typeof window[m[1]] === "function";
    m = /^([^.]+)\.(.+?)=?$/.exec(key);
    if (!m) return true;
    const [, owner, member] = m;
    if (owner === "window") return member in window;
    if (!(owner in window)) return false;
    const object = window[owner];
    if (typeof object === "function") return member in object.prototype || member in object;
    return member in object;
  };
  for (const key of api) {
    if (/^(listen|dispatch|context):/.test(key)) continue;
    let ok;
    try {
      ok = has(key);
    } catch {
      ok = false;
    }
    if (!ok) missing.api.push(key);
  }

  // A declaration machopo keeps is one it parses: set on an element's style, it
  // reads back.
  const probe = document.createElement("div");
  for (const declaration of declarations) {
    const at = declaration.indexOf(":");
    const property = declaration.slice(0, at).trim();
    const value = declaration.slice(at + 1).trim();
    probe.removeAttribute("style");
    let ok = false;
    try {
      probe.style.setProperty(property, value);
      ok = probe.style.getPropertyValue(property) !== "";
    } catch {}
    if (!ok) missing.declarations.push(declaration);
  }

  const parses = (selector) => {
    try {
      document.querySelector(selector);
      return true;
    } catch {
      return false;
    }
  };
  // A selector list's items, split at its top-level commas.
  const items = (list) => {
    const out = [];
    let depth = 0, quote = null, start = 0;
    for (let i = 0; i <= list.length; i++) {
      const c = list[i];
      if (quote) {
        if (c === "\\") i++;
        else if (c === quote) quote = null;
      } else if (c === '"' || c === "'") quote = c;
      else if (c === "(" || c === "[") depth++;
      else if (c === ")" || c === "]") depth--;
      else if ((c === "," || c === undefined) && depth === 0) {
        out.push(list.slice(start, i).trim());
        start = i + 1;
      }
    }
    return out;
  };
  const pseudo = /::?[-\w]+(\((?:[^()]|\([^()]*\))*\))?/g;
  // What makes each selector machopo can't parse fail: the pseudo-classes and
  // pseudo-elements machopo can't parse on their own (`*::placeholder`), not
  // the others written with them; the item itself when none fails alone.
  for (const selector of selectors) {
    if (parses(selector)) continue;
    missing.selectors.push(selector);
    const culprits = new Set();
    for (const item of items(selector)) {
      if (parses(item)) continue;
      const alone = (item.match(pseudo) ?? []).filter((p) => !parses(`*${p}`));
      for (const p of alone.length ? alone : [item]) culprits.add(p);
    }
    missing.culprits[selector] = [...culprits];
  }

  // A kind of at-rule machopo keeps: a sheet of the rule's text has it.
  for (const [kind, text] of Object.entries(atRules)) {
    const kinds = new Set();
    const walk = (list) => {
      for (const rule of list) {
        if (rule.selectorText !== undefined && rule.style) {
          if (rule.cssRules?.length) kinds.add("nesting");
        } else kinds.add(rule.constructor.name);
        if (rule.cssRules) walk(rule.cssRules);
      }
    };
    try {
      const sheet = new CSSStyleSheet();
      sheet.replaceSync(text);
      walk(sheet.cssRules);
    } catch {}
    if (!kinds.has(kind)) missing.atRules.push(kind);
  }

  // `element:<tag>:<interface>`, `element:<namespace>|<name>:<interface>`:
  // machopo's element is another interface (often `HTMLElement` for one it
  // has no interface of).
  const namespaces = {
    svg: "http://www.w3.org/2000/svg",
    mathml: "http://www.w3.org/1998/Math/MathML",
    html: "http://www.w3.org/1999/xhtml",
  };
  for (const key of elements) {
    const [, tag, name] = key.split(":");
    let got;
    try {
      const bar = tag.indexOf("|");
      let element;
      if (bar < 0) element = document.createElement(tag);
      else {
        const prefix = tag.slice(0, bar);
        // No namespace for any other (its elements are all `Element`).
        element = document.createElementNS(namespaces[prefix], tag.slice(bar + 1));
      }
      got = element.constructor.name;
    } catch (e) {
      got = String(e);
    }
    if (got !== name) missing.elements.push(`${tag}: ${name} (machopo: ${got})`);
  }

  fetch("/__machopo__/results", { method: "POST", body: JSON.stringify(missing) });
})();
