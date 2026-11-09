// A stand-in for Speedometer's runner, for `scripts/speedometer_test.py`:
// the same `benchmarkClient` hooks, called in the same order, around the
// tests of the suites `suites=` names (`/suites/<name>.html`, each a page
// with `window.tests`, `[{name, run(document)}]`), `iterationCount=` times
// in a row. Each iteration loads each suite's page into a new frame, and
// times each test, as the real runner does.
const params = new URLSearchParams(location.search);
const iterationCount = Number(params.get("iterationCount") ?? 1);
const suites = (params.get("suites") ?? "").split(",").filter(Boolean);

globalThis.benchmarkClient = {
  willStartFirstIteration() {},
  willAddTestFrame() {},
  willRunTest() {},
  didRunTest() {},
  didRunSuites() {},
  didFinishLastIteration() {},
  handleError() {},
  // As the real runner's, which starts itself through it.
  start() {
    run();
  },
};

function loadFrame(src) {
  const frame = document.createElement("iframe");
  frame.className = "test-runner";
  frame.style.width = "800px";
  frame.style.height = "600px";
  const loaded = new Promise((resolve) => frame.addEventListener("load", resolve, { once: true }));
  frame.src = src;
  document.body.prepend(frame);
  return loaded.then(() => frame);
}

// Speedometer's `RAFTestInvoker`: the test runs in an animation frame's
// callback (its sync time), and a timer that the next callback of the same
// frame sets ends its async time. With the runner's marks around them
// (`_runTestAndRecordResults`), which traces find the timed windows by,
// and its two measures. A test of these fakes may return a promise: its
// sync time ends, and the timer waits, when the promise settles.
function runTest(suite, test, document) {
  const label = `${suite.name}.${test.name}`;
  return new Promise((resolve, reject) => {
    let start;
    let ran;
    requestAnimationFrame(() => {
      performance.mark(`${label}-start`);
      start = performance.now();
      try {
        ran = Promise.resolve(test.run(document)).then(() => {
          const syncEnd = performance.now();
          performance.mark(`${label}-sync-end`, { startTime: syncEnd });
          performance.mark(`${label}-async-start`, { startTime: syncEnd });
          return syncEnd;
        });
      } catch (error) {
        ran = Promise.reject(error);
      }
      // What the test threw is the runner's, as the timer takes it: not a
      // rejection left unhandled meanwhile.
      ran.catch(() => {});
    });
    requestAnimationFrame(() =>
      setTimeout(async () => {
        try {
          const syncEnd = await ran;
          const asyncEnd = performance.now();
          performance.mark(`${label}-async-end`, { startTime: asyncEnd });
          performance.measure(`${label}-sync`, `${label}-start`, `${label}-sync-end`);
          performance.measure(`${label}-async`, `${label}-async-start`, `${label}-async-end`);
          resolve({ Sync: syncEnd - start, Async: asyncEnd - syncEnd });
        } catch (error) {
          reject(error);
        }
      })
    );
  });
}

async function run() {
  // Through `globalThis.benchmarkClient` each time: the injected module has
  // wrapped its hooks by now.
  const client = globalThis.benchmarkClient;
  try {
    await client.willStartFirstIteration(iterationCount);
    for (let i = 0; i < iterationCount; i++) {
      const tests = {};
      for (const name of suites) {
        const frame = await loadFrame(`/suites/${name}.html`);
        const suite = { name };
        const results = { tests: {}, total: 0 };
        for (const test of frame.contentWindow.tests) {
          await client.willRunTest(suite, test);
          const times = await runTest(suite, test, frame.contentDocument);
          const time = times.Sync + times.Async;
          results.tests[test.name] = { tests: times, total: time };
          results.total += time;
          await client.didRunTest(suite, test);
        }
        tests[name] = results;
        frame.remove();
      }
      await client.didRunSuites({ tests });
    }
  } catch (error) {
    await client.handleError(error);
    return;
  }
  await client.didFinishLastIteration({});
}

// `load` comes after every module script, the injected one too.
if (params.has("startAutomatically")) window.addEventListener("load", () => globalThis.benchmarkClient.start());
