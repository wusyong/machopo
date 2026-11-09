// Parser-blocking: `last` isn't parsed yet, so this box goes before it.
const box = document.createElement("div");
box.setAttribute(
  "style",
  document.getElementById("last") === null ? "background-color: green" : "background-color: red",
);
document.querySelector("body").appendChild(box);
