// What a page loads that swallows an exception: a browser without the API
// it calls does less, and nothing says so.
function tryToCall() {
  try {
    document.createElement("span").notAnApiAnywhere();
  } catch (exception) {}
}
