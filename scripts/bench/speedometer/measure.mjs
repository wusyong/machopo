// Injected into the runner's page after `main.mjs` when `bench.py speedometer`
// measures a suite, in Chrome and in machopo alike: after each test (once it is
// measured, so outside the times) takes a summary of the suite's document,
// and with each iteration the times the runner measured (`didRunSuites`);
// at the end, the runner's marks, by which a profile finds the windows the
// runner timed (on this page's clock). With `--profile`, holds the runner's
// start until the tool, the profile started, calls `__machopoStart()`: the
// profile has the suite's code from before it is compiled.
// Posts it all, with the exceptions `errors.js` collects, to the server once
// the runner finishes, gives up (`handleError`), stalls after an exception
// (nothing more for STALL ms), or runs out of time (DEADLINE ms).
import { summarize } from "/__machopo__/summary.mjs";

const DEADLINE = __DEADLINE__;
const HOLD = __HOLD__;
const STALL = 5000;
const client = globalThis.benchmarkClient;
const started = performance.now();
// Each iteration's tests' times, `{suite: {tests: {test: {tests: {Sync,
// Async}, total}}, total}}`, as the runner gives them.
const iterations = [];
// Each iteration's summaries, by `suite/test`.
const summaries = [{}];
let current = null;
let completed = 0;
let reported = false;
let lastProgress = started;

async function report(status) {
  if (reported) return;
  reported = true;
  const body = JSON.stringify({
    status,
    current,
    completed,
    iterations,
    summaries: summaries.filter((s) => Object.keys(s).length),
    errors: (window.__machopoErrors ?? []).slice(0, 20),
    marks: performance.getEntriesByType("mark").map((mark) => [mark.startTime, mark.name]),
    runnerStarted,
    seconds: (performance.now() - started) / 1000,
  });
  await fetch("/__machopo__/results", { method: "POST", body });
}

function frameDocument() {
  return document.querySelector("iframe.test-runner")?.contentDocument;
}

// The runner didn't load (its modules failed): what the page says why.
if (!client) {
  setTimeout(() => report("no runner"), 1000);
  throw new Error("no benchmarkClient");
}

// When the runner started, if held (on this page's clock).
let runnerStarted = null;
if (HOLD) {
  const start = client.start.bind(client);
  let release;
  const released = new Promise((resolve) => (release = resolve));
  globalThis.__machopoStart = () => release();
  client.start = () =>
    released.then(() => {
      runnerStarted = performance.now();
      return start();
    });
}

const willRunTest = client.willRunTest.bind(client);
client.willRunTest = (suite, test) => {
  current = `${suite.name}/${test.name}`;
  lastProgress = performance.now();
  return willRunTest(suite, test);
};

const didRunTest = client.didRunTest.bind(client);
client.didRunTest = (...args) => {
  const doc = frameDocument();
  if (doc && current) summaries[summaries.length - 1][current] = summarize(doc);
  completed++;
  current = null;
  lastProgress = performance.now();
  return didRunTest(...args);
};

const didRunSuites = client.didRunSuites.bind(client);
client.didRunSuites = (measuredValues) => {
  iterations.push(JSON.parse(JSON.stringify(measuredValues.tests)));
  summaries.push({});
  return didRunSuites(measuredValues);
};

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

setInterval(() => {
  const now = performance.now();
  if ((window.__machopoErrors ?? []).length && now - lastProgress > STALL) report("stalled");
  else if (now - started > DEADLINE) report("timeout");
}, 500);
