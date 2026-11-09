// After parsing: `last` is there, and this box goes after it.
const late = document.createElement("div");
late.setAttribute(
  "style",
  document.getElementById("last") !== null ? "background-color: green" : "background-color: red",
);
document.querySelector("body").appendChild(late);
