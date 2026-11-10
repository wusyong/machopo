# Site snapshots

Offline copies of real pages, used to check that `kun` can browse simple
document sites (the P2 goal). They are loaded from disk, so the checks don't
depend on the network or on the sites changing.

These pages can't be compared pixel for pixel with a real browser (the fonts
differ), so they are not reftests. `crates/shell/tests/it/sites.rs` checks each
one for things that must hold: its title, that it loads and draws, that
its subresources load, and that clicking a link at a given point (with the
test fonts) leads where it should: CERN's to another page, Gutenberg's
contents to a chapter in the same document.

Run the checks with:

```sh
cargo test -p shell --test it sites::
```

Each directory keeps the site's own paths, so relative links between the
pages still work.

| Directory | Source | Fetched | Pages | Why it's here |
| --- | --- | --- | --- | --- |
| `cern/` | `http://info.cern.ch/hypertext/WWW/` | 2026-09-30 | `TheProject.html` and four pages it links to: `WhatIs.html`, `Summary.html`, `Status.html`, `History.html` | The first website: plain HTTP, no CSS, many links |
| `sqlite/` | `https://sqlite.org/` | 2026-10-01 | `about.html`, `sqlite.css`, `images/sqlite370_banner.svg` | A linked stylesheet and inline boxes. The logo is an SVG, which kun doesn't decode, so it fails and takes the room `sqlite.css` gives it; the floats (logo, menu) are laid out as blocks |
| `gutenberg-28885/` | `https://www.gutenberg.org/cache/epub/28885/` | 2026-10-01 | `pg28885-images.html` and its images: 42 `<img>` (25 PNG, 17 JPEG) showing 37 files | A long document with many images: decoding, sizes from `width` / `height`, scrolling and `#` links |
| `todomvc/` | Speedometer 3.1 (the `dist` of its vanilla ES5 TodoMVC, under `resources` in https://github.com/WebKit/Speedometer at `1386415`) | 2026-11-10 | `index.html`, `base.css`, `index.css` and its seven scripts | TodoMVC in plain JS, as Speedometer runs it: the app a wasm guest is written to match. `tests/scenarios/pages/todomvc-wasm.html` uses its markup and style sheets with the guest in place of its scripts |

## Licenses

- **`cern/`**: the World Wide Web project's own pages from 1992, which CERN
  keeps online at info.cern.ch as a historical record. On 30 April 1993 CERN
  put the WWW software in the public domain; the statement is about the
  software, and doesn't name these pages. The files are unmodified.
- **`sqlite/`**: "All of the code and documentation in SQLite has been
  dedicated to the public domain by the authors"
  (https://sqlite.org/copyright.html, checked 2026-10-01). Unmodified.
- **`todomvc/`**: TodoMVC, MIT License, "Copyright (c) Addy Osmani, Sindre
  Sorhus, Pascal Hartig, Stephen Sawchuk": the license is `license.md`
  there (the one Speedometer keeps beside its TodoMVC apps). The files are
  unmodified.
- **`gutenberg-28885/`**: *Alice's Adventures in Wonderland* with Arthur
  Rackham's illustrations, a Project Gutenberg eBook of a work in the
  public domain in the United States (the Project Gutenberg Literary
  Archive Foundation holds a compilation copyright in the collection, §1.C
  of its license). A copy that carries the "Project Gutenberg" name has to
  keep the full Project Gutenberg License with it (§1.E.1–1.E.2); the HTML
  here is unmodified and ends with that license.
