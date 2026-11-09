// Added after Blink's `resources/runner.js` (`PerfTestRunner`) when
// `bench.py blink-perf` serves it: posts the page's result to the server
// once, as the first of: the runner's statistics of its iterations
// (`logStatistics` with "Time:", what every way of measuring ends with), a
// fatal error the runner logs, or an exception nothing caught; each with
// the last lines the runner logged (why a page measured nothing) and a
// summary of the document (`summary.mjs`'s, which the server puts before
// this as `__machopoSummarize`: whether both browsers did the same work). The
// runner's `console.time("blink_perf")`, from the first measured iteration
// to the end, becomes User Timing: a mark when it starts and a measure
// from it at the end, which `bench.py blink-perf --gc` finds in the
// trace.
(function () {
  var posted = false;
  var lines = [];
  function post(body) {
    if (posted) return;
    posted = true;
    body.log = lines.slice(-10);
    try {
      body.summary = __machopoSummarize(document);
      // Not what the runner logs in the page: its times differ.
      var log = document.getElementById("log");
      if (log) body.summary.text -= log.textContent.length;
    } catch (error) {
      body.summary_error = String(error);
    }
    fetch("/__machopo__/results", { method: "POST", body: JSON.stringify(body) });
  }
  var log = PerfTestRunner.log;
  PerfTestRunner.log = function (text) {
    lines.push(String(text));
    return log.apply(this, arguments);
  };
  // The runner's `logFatalError` ends the test at once (`finish`, which
  // logs the statistics of what it has): the error goes instead.
  var fatal = null;
  var logStatistics = PerfTestRunner.logStatistics;
  PerfTestRunner.logStatistics = function (values, unit, title) {
    logStatistics.apply(this, arguments);
    if (title === "Time:") post(fatal === null ? { unit: unit, values: values } : { error: "fatal: " + fatal });
  };
  var logFatalError = PerfTestRunner.logFatalError;
  PerfTestRunner.logFatalError = function (text) {
    if (fatal === null) fatal = String(text);
    logFatalError.apply(this, arguments);
    post({ error: "fatal: " + fatal });
  };
  var timing = false;
  var time = console.time;
  console.time = function (label) {
    if (label === "blink_perf" && !timing) {
      timing = true;
      performance.mark("blink_perf");
    }
    return time.apply(this, arguments);
  };
  var timeEnd = console.timeEnd;
  console.timeEnd = function (label) {
    if (label === "blink_perf" && timing) performance.measure("blink_perf", "blink_perf");
    return timeEnd.apply(this, arguments);
  };
  window.addEventListener("error", function (event) {
    post({ error: String(event.message) + " (" + event.filename + ":" + event.lineno + ")" });
  });
  window.addEventListener("unhandledrejection", function (event) {
    post({ error: "unhandled rejection: " + String(event.reason) });
  });
})();
