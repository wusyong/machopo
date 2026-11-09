// Injected into the runner's page after `main.mjs` for `bench.py speedometer
// --inventory` in Chrome: after each test, takes what the suite's page used
// (`record.js`'s counts) and the CSS it has (every rule's selectors,
// declarations and at-rules, in its document, its open shadow roots and
// their adopted sheets, and the `style` attributes); at the end, posts it
// all, by suite, to the server. Each kind of at-rule comes with the text of
// the first top-level rule seen that has it, for machopo's probe to try.
const client = globalThis.benchmarkClient;
const suites = Object.create(null);
let current = null;

const suiteOf = (name) =>
  (suites[name] ??= { api: {}, selectors: {}, declarations: {}, atRules: {}, atRuleTexts: {}, tests: 0 });

function merge(into, counts) {
  for (const [key, n] of Object.entries(counts)) into[key] = (into[key] || 0) + n;
}

function frameWindow() {
  return document.querySelector("iframe.test-runner")?.contentWindow;
}

function collectCss(doc, out) {
  const add = (map, key) => (map[key] = (map[key] || 0) + 1);
  const declarations = (style) => {
    let pending = false;
    for (let i = 0; i < style.length; i++) {
      const property = style[i];
      // Custom properties are kept as written by machopo; their uses show in
      // the values that `var()` them.
      if (property.startsWith("--")) continue;
      const value = style.getPropertyValue(property);
      // A shorthand's longhand reads empty while the shorthand `var()`s
      // (pending substitution): the shorthand is what was written.
      if (value === "") pending = true;
      else add(out.declarations, `${property}: ${value}`);
    }
    if (!pending) return;
    // The names in its text (the longhands it has again: a count more): a
    // `;` in a value (`url(data:...;base64,...)`) makes one that isn't a
    // property, which reads empty.
    for (const name of style.cssText.split(";").map((d) => d.split(":")[0].trim())) {
      if (name.startsWith("--")) continue;
      const value = style.getPropertyValue(name);
      if (value !== "") add(out.declarations, `${name}: ${value}`);
    }
  };
  // `top`: the text of the top-level rule `list` is in, which machopo's probe
  // can put in a sheet of its own.
  const rules = (list, top) => {
    for (const rule of list) {
      const text = top ?? rule.cssText;
      if (rule.selectorText !== undefined && rule.style) {
        add(out.selectors, rule.selectorText);
        declarations(rule.style);
        if (rule.cssRules?.length) {
          add(out.atRules, "nesting");
          out.atRuleTexts.nesting ??= text;
          rules(rule.cssRules, text);
        }
      } else {
        const kind = rule.constructor.name;
        add(out.atRules, kind);
        out.atRuleTexts[kind] ??= text;
        if (rule.cssRules) rules(rule.cssRules, text);
      }
    }
  };
  const roots = [doc];
  for (const element of doc.querySelectorAll("*")) if (element.shadowRoot) roots.push(element.shadowRoot);
  for (let i = 1; i < roots.length; i++)
    for (const element of roots[i].querySelectorAll("*")) if (element.shadowRoot) roots.push(element.shadowRoot);
  for (const root of roots) {
    for (const sheet of [...root.styleSheets, ...(root.adoptedStyleSheets ?? [])]) {
      let list;
      try {
        list = sheet.cssRules;
      } catch {
        continue;
      }
      rules(list);
    }
    for (const element of root.querySelectorAll("[style]")) declarations(element.style);
  }
}

function takeFrame() {
  const win = frameWindow();
  if (!win || !current) return;
  const suite = suiteOf(current);
  win.__machopoRecord?.quiet(() => {
    merge(suite.api, win.__machopoRecord.take());
    collectCss(win.document, suite);
  });
}

const willRunTest = client.willRunTest.bind(client);
client.willRunTest = (suite, test) => {
  // The runner has loaded the suite's page by now: what it did while
  // loading is taken with its first test's, as this suite's.
  current = suite.name;
  return willRunTest(suite, test);
};

const didRunTest = client.didRunTest.bind(client);
client.didRunTest = (...args) => {
  takeFrame();
  if (current) suiteOf(current).tests++;
  return didRunTest(...args);
};

async function report(status) {
  takeFrame();
  const runner = window.__machopoRecord ? window.__machopoRecord.take() : {};
  const body = JSON.stringify({ status, suites, runner, errors: window.__machopoErrors ?? [] });
  await fetch("/__machopo__/inventory", { method: "POST", body });
}

const didFinishLastIteration = client.didFinishLastIteration.bind(client);
client.didFinishLastIteration = async (metrics) => {
  await report("done");
  return didFinishLastIteration(metrics);
};

const handleError = client.handleError.bind(client);
client.handleError = async (error) => {
  (window.__machopoErrors ??= []).push({ page: "runner", kind: "handleError", message: String(error?.stack ?? error) });
  await report("error");
  return handleError(error);
};
