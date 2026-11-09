// Injected first into every page `bench.py speedometer` serves (the runner's
// and each suite's, in Chrome and in machopo): collects the exceptions nobody
// caught, `error` and `unhandledrejection`, into the runner's window. A
// suite's page is in the runner's iframe, and its errors don't reach the
// runner on their own.
(() => {
  "use strict";
  let top;
  try {
    top = window.top;
    top.__machopoErrors ??= [];
  } catch {
    return;
  }
  const where = location.pathname;
  const push = (kind, message) => {
    top.__machopoErrors.push({ page: where, kind, message: String(message) });
  };
  window.addEventListener("error", (event) => {
    const e = event.error;
    push("error", e && e.stack ? e.stack : `${event.message} (${event.filename}:${event.lineno})`);
  });
  window.addEventListener("unhandledrejection", (event) => {
    const r = event.reason;
    push("unhandledrejection", r && r.stack ? r.stack : r);
  });
})();
