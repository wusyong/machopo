// Injected first into every page Chrome loads for `bench.py speedometer
// --inventory`, before any of the page's own scripts: records which Web API
// members the page uses. Every interface object's prototype and static
// members, the window's own members, and the namespace objects (`CSS`) are
// replaced with wrappers that count each call, get and set; an interface
// object itself with a proxy that counts `new`. The wrappers' `toString`
// is the original's (frameworks test it to tell native functions apart).
//
// Keys: `I.m` (a method or getter, on the prototype or static, or of a
// namespace: `console.assert`), `I.m=` (a
// setter), `new I`, `window.m` (the global's own), and a few with the
// arguments that matter: `element:<tag>:<interface>` (what `createElement`
// made), `element:<namespace>|<name>:<interface>` (what `createElementNS`
// made: `svg`, `mathml`, `html`, or nothing for any other, whose elements
// are all `Element`),
// `listen:<type>`, `dispatch:<type>`, `context:<kind>`.
//
// `window.__machopoRecord.take()` hands the counts over and starts again; the
// harness's own calls aren't counted while `quiet()` runs. JavaScript's own
// globals (`Array`, `Promise`...) aren't wrapped: machopo has V8's.
(() => {
  "use strict";
  if (window.__machopoRecord) return;

  let counts = Object.create(null);
  let quiet = 0;
  const hit = (key) => {
    if (quiet === 0) counts[key] = (counts[key] || 0) + 1;
  };

  const R = Reflect;
  const gopd = Object.getOwnPropertyDescriptor;
  const dp = Object.defineProperty;
  const ownNames = Object.getOwnPropertyNames;
  const originals = new WeakMap();
  const fnToString = Function.prototype.toString;

  // ECMAScript's (and V8's) globals, which machopo has as Chrome does.
  const JS = new Set([
    "Object", "Function", "Array", "Number", "Boolean", "String", "Symbol", "Date", "Promise",
    "RegExp", "Error", "AggregateError", "EvalError", "RangeError", "ReferenceError", "SyntaxError",
    "TypeError", "URIError", "SuppressedError", "JSON", "Math", "Intl", "ArrayBuffer",
    "SharedArrayBuffer", "Atomics", "Uint8Array", "Int8Array", "Uint16Array", "Int16Array",
    "Uint32Array", "Int32Array", "BigUint64Array", "BigInt64Array", "Float16Array", "Float32Array",
    "Float64Array", "Uint8ClampedArray", "DataView", "Map", "BigInt", "Set", "WeakMap", "WeakSet",
    "Proxy", "Reflect", "FinalizationRegistry", "WeakRef", "Iterator", "DisposableStack",
    "AsyncDisposableStack", "WebAssembly", "globalThis",
  ]);

  const namespaces = {
    "http://www.w3.org/2000/svg": "svg",
    "http://www.w3.org/1998/Math/MathML": "mathml",
    "http://www.w3.org/1999/xhtml": "html",
  };

  // What a call's arguments add: which element, event or context.
  const extra = {
    "Document.createElement": (args, result) =>
      `element:${String(args[0]).toLowerCase()}:${result?.constructor?.name}`,
    // The namespace too: an SVG element's name made by `createElement` is
    // an HTML element's. Its local name only (no prefix, no `:`).
    "Document.createElementNS": (args, result) => {
      const local = String(args[1]).split(":").pop();
      return `element:${namespaces[args[0]] ?? ""}|${local}:${result?.constructor?.name}`;
    },
    "EventTarget.addEventListener": (args) => `listen:${args[0]}`,
    "EventTarget.dispatchEvent": (args) => `dispatch:${args[0]?.type}`,
    "HTMLCanvasElement.getContext": (args) => `context:${args[0]}`,
  };

  function wrapFunction(f, key) {
    const more = extra[key];
    const name = f.name;
    // A method, not a constructor, as the native ones are.
    const w = {
      [name](...args) {
        hit(key);
        const result = R.apply(f, this, args);
        // What the arguments add, worked out without counting what that
        // reads (`constructor`), and counted only when the call is.
        if (more && quiet === 0) {
          quiet++;
          let extraKey;
          try {
            extraKey = more(args, result);
          } finally {
            quiet--;
          }
          hit(extraKey);
        }
        return result;
      },
    }[name];
    try {
      dp(w, "length", { value: f.length });
    } catch {}
    originals.set(w, f);
    return w;
  }

  function wrapMembers(target, prefix) {
    for (const member of ownNames(target)) {
      if (member === "constructor" || member === "prototype") continue;
      const d = gopd(target, member);
      if (!d || !d.configurable) continue;
      const key = `${prefix}.${member}`;
      try {
        if (typeof d.value === "function") {
          dp(target, member, { ...d, value: wrapFunction(d.value, key) });
        } else if (d.get || d.set) {
          dp(target, member, {
            ...d,
            get: d.get && wrapFunction(d.get, key),
            set: d.set && wrapFunction(d.set, `${key}=`),
          });
        }
      } catch {}
    }
  }

  for (const name of ownNames(window)) {
    if (JS.has(name) || name.startsWith("__machopo")) continue;
    const d = gopd(window, name);
    if (!d) continue;
    const value = d.value;
    const isInterface =
      typeof value === "function" && /^[A-Z]/.test(name) && value.prototype && d.configurable;
    if (isInterface) {
      wrapMembers(value.prototype, name);
      wrapMembers(value, name);
      const proxy = new Proxy(value, {
        construct(target, args, newTarget) {
          hit(`new ${name}`);
          return R.construct(target, args, newTarget === proxy ? target : newTarget);
        },
      });
      try {
        dp(window, name, { ...d, value: proxy });
      } catch {}
    } else if (value && typeof value === "object" && (/^[A-Z]/.test(name) || name === "console")) {
      // A namespace: `CSS`, `console`.
      wrapMembers(value, name);
    } else if (d.configurable) {
      // The global's own: `setTimeout`, `innerWidth`, `onload`...
      const key = `window.${name}`;
      try {
        if (typeof value === "function") dp(window, name, { ...d, value: wrapFunction(value, key) });
        else if (d.get || d.set)
          dp(window, name, {
            ...d,
            get: d.get && wrapFunction(d.get, key),
            set: d.set && wrapFunction(d.set, `${key}=`),
          });
      } catch {}
    }
  }

  const toString = function toString() {
    return R.apply(fnToString, originals.get(this) ?? this, []);
  };
  originals.set(toString, fnToString);
  dp(Function.prototype, "toString", { ...gopd(Function.prototype, "toString"), value: toString });

  dp(window, "__machopoRecord", {
    value: Object.freeze({
      take() {
        const taken = counts;
        counts = Object.create(null);
        return taken;
      },
      quiet(f) {
        quiet++;
        try {
          return f();
        } finally {
          quiet--;
        }
      },
    }),
  });
})();
