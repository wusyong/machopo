// Runs `change` once a frame or two has been drawn (so tiles exist to keep
// or raster again), then ends the test with the next frame.
function afterFrames(change) {
  requestAnimationFrame(() => requestAnimationFrame(() => {
    change();
    requestAnimationFrame(() => document.documentElement.classList.remove("reftest-wait"));
  }));
}
