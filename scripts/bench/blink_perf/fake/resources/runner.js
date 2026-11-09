// A stand-in for Blink's `perf_tests/resources/runner.js`, for the tool's
// tests: the same `PerfTestRunner` calls the hook takes, a few iterations,
// and as Blink's, `logFatalError` ends the test at once (`finish`) and the
// measured iterations are timed with `console.time("blink_perf")`.
(function () {
  var PerfTestRunner = (window.PerfTestRunner = {});
  var results = [];
  var test = null;
  var done = false;
  var ITERATIONS = 4;

  PerfTestRunner.log = function (text) {
    var pre = document.getElementById("log") || document.body.appendChild(document.createElement("pre"));
    pre.id = "log";
    pre.textContent += text + "\n";
  };
  PerfTestRunner.logStatistics = function (values, unit, title) {
    PerfTestRunner.log(title);
    PerfTestRunner.log("values " + values.join(", ") + " " + unit);
  };
  function finish() {
    if (done) return;
    done = true;
    console.timeEnd("blink_perf");
    PerfTestRunner.logStatistics(results, PerfTestRunner.unit, "Time:");
  }
  PerfTestRunner.logFatalError = function (text) {
    PerfTestRunner.log("FATAL: " + text);
    finish();
  };
  // Blink's collects garbage with `GCController` when there is one.
  PerfTestRunner.gc = function () {};
  PerfTestRunner.logException = function (text, exception) {
    PerfTestRunner.logFatalError("Got an exception while " + text + ": " + exception.message);
  };
  function start(t, measure) {
    test = t;
    var iteration = -1;
    function next() {
      if (done) return;
      if (iteration >= 0) console.time("blink_perf");
      try {
        var value = measure();
      } catch (exception) {
        PerfTestRunner.logException("running test.run", exception);
        return;
      }
      if (iteration >= 0) results.push(value);
      PerfTestRunner.log(value + " " + PerfTestRunner.unit);
      if (++iteration < ITERATIONS) setTimeout(next, 0);
      else finish();
    }
    setTimeout(next, 0);
  }
  PerfTestRunner.measureTime = function (t) {
    PerfTestRunner.unit = "ms";
    start(t, function () {
      var begin = performance.now();
      test.run();
      return performance.now() - begin + 1;
    });
  };
  PerfTestRunner.measureRunsPerSecond = function (t) {
    PerfTestRunner.unit = "runs/s";
    start(t, function () {
      var begin = performance.now();
      for (var i = 0; i < 100; i++) test.run();
      return 100000 / (performance.now() - begin + 1);
    });
  };
})();
