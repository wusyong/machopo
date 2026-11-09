// Injected into the runner's page after `main.mjs` when `bench.py speedometer
// --inventory` runs a suite in machopo: follows the runner (each test started
// and finished) and the exceptions `errors.js` collects, and posts what
// happened to the server once the runner finishes, gives up
// (`handleError`), stalls after an exception (nothing more for STALL ms),
// or runs out of time (DEADLINE ms).
const DEADLINE = __DEADLINE__;
const STALL = 5000;
const client = globalThis.benchmarkClient;
const started = performance.now();
const completed = [];
let current = null;
let reported = false;
let lastProgress = started;

async function report(status) {
  if (reported) return;
  reported = true;
  const body = JSON.stringify({
    status,
    current,
    completed,
    errors: (window.__machopoErrors ?? []).slice(0, 20),
    seconds: (performance.now() - started) / 1000,
  });
  await fetch("/__machopo__/results", { method: "POST", body });
}

// The runner didn't load (its modules failed): what the page says why.
if (!client) {
  setTimeout(() => report("no runner"), 1000);
  throw new Error("no benchmarkClient");
}

const willRunTest = client.willRunTest.bind(client);
client.willRunTest = (suite, test) => {
  current = `${suite.name}/${test.name}`;
  lastProgress = performance.now();
  return willRunTest(suite, test);
};

const didRunTest = client.didRunTest.bind(client);
client.didRunTest = (...args) => {
  completed.push(current);
  current = null;
  lastProgress = performance.now();
  return didRunTest(...args);
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
