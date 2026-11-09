// What of a document shows whether a page did what it was asked, the same
// in machopo and in Chrome: the server of each benchmark serves this file as
// `/__machopo__/summary.mjs`.
//
// What of a document shows whether the app did what the test asked: how
// many elements of each name, how many with each class, and how long its
// text is (not in `<style>` and `<script>`, where a page writes the class
// names it makes up), through its open shadow roots. Not the whole tree:
// dates, random numbers and the order of attributes differ between runs.
export function summarize(doc) {
  const elements = {};
  const classes = {};
  let text = 0;
  const roots = [doc];
  for (let i = 0; i < roots.length; i++) {
    for (const element of roots[i].querySelectorAll("*")) {
      elements[element.localName] = (elements[element.localName] || 0) + 1;
      for (const name of element.classList) classes[name] = (classes[name] || 0) + 1;
      if (element.shadowRoot) roots.push(element.shadowRoot);
    }
    const walker = doc.createTreeWalker(roots[i], NodeFilter.SHOW_TEXT);
    while (walker.nextNode()) {
      const parent = walker.currentNode.parentNode?.localName;
      if (parent !== "style" && parent !== "script") text += walker.currentNode.data.length;
    }
  }
  return { elements, classes, text };
}
