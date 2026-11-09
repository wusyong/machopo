# Scenarios

What a person would do in a browser window and check, written down so it
runs by itself. Each `*.txt` here is one scenario, but `known-failures.txt`
(below). Run them with:

```sh
python3 scripts/test.py scenarios            # all of them
python3 scripts/test.py scenarios demo       # file names containing "demo"
python3 scripts/test.py automation           # each in a real window (see below)
```

**In a real window**: `machopo --automation <scenario> [--out <dir>]` runs one
in `machopo`'s own window, winit and wgpu included, and exits 0 if every check
passed, 1 if one failed, 2 if the scenario is wrong; `scripts/test.py
automation` runs each this way, with screenshots in
`target/automation-results/`. The window stays on top and ignores the mouse
and keyboard while it runs. It uses the system's keyboard shortcuts (only
that platform's `platform` blocks run), the display's scale (`window scale=`
is ignored) and the system's fonts, so its screenshots look like what a
person sees. It needs a display and a GPU.

- **The window**: a browser window without the platform's window
  (`shell::windowless`): the toolbar and the tab, Viz on its own thread
  sending `BeginFrame`s every 16.7 ms, the compositor drawing without waiting
  for the main thread, as in `machopo`'s window; drawn on the GPU (the CPU adapter
  without one, or with `MACHOPO_GPU_ADAPTER=cpu`) into a texture read back from,
  800×600 CSS px at 1x, test fonts only. Time is real: checks wait for what they expect,
  up to a timeout.
- **Twice if it presses keys**: a scenario with `key`, `type` or a
  `platform` block runs with macOS's keyboard shortcuts and with the
  others' (Windows and Linux), on any system: which shortcuts the window
  uses is only a setting of it here. Nothing else depends on them, so the
  rest run once, with the others'.
- **Screenshots** go to `target/scenario-results/<scenario>/other/<name>.png`
  (and `…/mac/…` for those that run twice). A screenshot is for people to
  look at; only the checks decide pass or fail.
- **Four at a time**, each in a new window with its own renderer and Viz
  thread; the results print in the scenarios' order. Time is real, so a
  check's timeout must leave room for the others running beside it (a
  `timeout 1` that tests what the compositor does while the main thread is
  busy for 3 seconds still does).
- **Pages** written for a scenario go in `pages/`; most scenarios use the
  built-in `about:demo`, which people look at in the
  window too.
- **Known failures**: those `known-failures.txt` names (one a line, the
  file's name without `.txt`, a `#` comment saying why) may fail; one of
  them that passes every run fails the run (take it off the list), as does
  a name there no scenario has. `test.py automation` reads the list too.

## Format

One command a line. Empty lines and lines starting with `#` are skipped.
Elements are picked by CSS selector: the first element in the document that
matches, as `querySelector`; the page's document, or a child frame's after
`frame`. Points and sizes are CSS px of the window, the
toolbar included.

A command that asks the page about an element (a selector), and `capture` and
`screenshot` without `now`, first wait until the renderer has handled the
input given it so far: one that changed the page, with the frame that shows
the change (`RenderWidgetHostImpl`'s in-flight events). So a `click` right
after a click that moves its element goes where the element is now. `wait-busy`,
`click-at`, `dblclick-at`, `wheel-at`, `hover-at`, `key`, `type`, `wait-cursor`
and `capture … now` don't wait.

| Command | What it does |
| --- | --- |
| `window <setting>…` | The window the scenario wants, before any other command: `scale=<n>` (device pixels per CSS px), `size=<width>x<height>`, `caret-blink=<ms>` (the caret shows and hides for that long each; without it, it doesn't blink, so every frame shows it). A real window is no taller than the screen (macOS keeps it on the screen): write scenarios for a window that fits one, 800x600 by default, and scroll to what is below it. |
| `navigate <url>` | As if typed in the address field. A path is relative to the scenario's file. |
| `wait-loaded` | Waits until the latest navigation started has loaded (`DidFinishLoad`). A link clicked starts its navigation later: wait for something on the new page instead. |
| `click <selector>` | The primary button down and up at the center of the element's box, once it has one. If the center is out of the page as the window shows it, the element is scrolled into view first, as `scroll-to` does (WebDriver's Element Click does the same); if it still is (the element is taller than the page), the step fails. |
| `click-at <x> <y>` | The same at a point. |
| `dblclick <selector>` / `dblclick-at <x> <y>` | Two clicks there, the second the double click (its `click_count` 2: a `dblclick` follows its `click`). A `click` is always the first of a series, however soon it comes after the last. |
| `wheel <selector> <dx> <dy>` | A wheel over the element's center, scrolling by `dx`, `dy` CSS px (positive: right and down). The element is scrolled into view first, as for `click`. |
| `wheel-at <x> <y> <dx> <dy>` | The same at a point. |
| `hover <selector>` / `hover-at <x> <y>` | The pointer moves to the element's center (scrolled into view first, as for `click`) or to a point, no button down. |
| `wait-cursor <keyword>` | Waits until the pointer, where it is, looks as `cursor`'s keyword says: `default` (the arrow), `pointer` (the hand), `text` (the I-beam), `move`, `ew-resize`, `none` (hidden)... (`all-scroll` looks as `move` does). Over a page it is what the page's renderer asked for, over the toolbar the toolbar's (the address field's `text`). A real window shows it too: what it looks like there is for a person to check. |
| `scroll-to <selector>` | Once the element has a box, scrolls it into view if its center is out of the page, as a link to `#fragment` would (its top at the top of the page; DevTools' `DOM.scrollIntoViewIfNeeded`), and waits until a frame shows that. Before a `capture` or `screenshot` of what is below the window: a `click` scrolls only when it comes, after the capture. |
| `key [modifier+]…<key>` | A key with modifiers held: `shift`, `ctrl`, `alt`, `cmd`, and `primary`, which is Cmd with macOS's shortcuts and Ctrl with the others'. Keys are one character, or `Enter`, `Escape`, `Backspace`, `Delete`, `Tab`, `Space`, `ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight`, `PageUp`, `PageDown`, `Home`, `End`, `F5`, `F6`. Space and Enter type what the platform's do (a space, a carriage return: a line break in a textarea). Examples: `key F6`, `key primary+l`, `key shift+Space`. |
| `platform mac` / `other` / `all` | The commands after it, up to the next `platform`, run only with macOS's shortcuts, only with the others', or with both (the default). |
| `type "<text>"` | A key for each character, typing it. |
| `wait-text <selector> "<text>"` | Waits until the element's `textContent` is exactly `<text>`. In quotes, `\"` and `\\` are escapes. |
| `wait-title "<text>"` | Waits until the active tab's title, as its tab shows it (the page's title, or its URL while it has none), is exactly `<text>`. Escapes as for `wait-text`. |
| `wait-change <selector>` | Waits until the element's text or box differs from when the wait began; right after input (`platform` lines between don't count), from before the input, which has changed it by the time the page is looked at. |
| `wait-busy` | Waits until the renderer's main thread is busy: a query goes unanswered for 300 ms. |
| `capture <name> [now]` | Keeps a frame as `<name>`: one that shows what the page has changed so far (it waits for the frame the renderer already wants, and forces none, so a change that asked for no frame stays undrawn, as for the user). With `now`, the next frame drawn, whatever the main thread is doing: while script keeps the page busy. |
| `expect-differs <name> [<selector>]` | Waits until a frame drawn differs from the one captured as `<name>`: anywhere, or within the element's box as it is now. Frames come from Viz, so this works while script keeps the page busy, when queries about elements can't. The timeout counts from the first frame, which it waits for as long as the default timeout, however short the scenario's: the timeout is for the change to show, and a busy machine can take longer than a short one to draw any frame. |
| `screenshot <name> [now]` | Saves a frame as `<name>.png`, picked as `capture` picks it. |
| `resize <width> <height>` | The window's new size. |
| `timeout <seconds>` | How long each later wait may take; 10 by default (`expect-differs` counts it from its first frame). |
| `frame <selector>` / `frame top` | Later selectors pick in the document of the first iframe matching the selector, in the document picked so far (nested frames: one `frame` each), or, `top`, in the page's again. An element's box is still in the window. `scroll-to`, and `click` and `wheel` at an element, scroll it into view in its frame, then its iframe in the frame around it, out to the page (as Chrome's `scrollIntoView`), and point at its center in the window. |

Names (`capture`, `screenshot`) are letters, digits, `-`, `_` and `.`.

A failure names the line, and what was seen instead:

```text
scenario demo-script (other) ... FAILED: line 16: #count never had the text "Clicked 1 time (onclick)."; it had "Not clicked yet."
```

## Writing one

- Check what a person would look at, by its text where it has one: it
  doesn't depend on fonts or on where the page puts it.
- Never wait a fixed time; wait for what should happen.
- **Shortcuts that differ between platforms**: `primary` where only Cmd and
  Ctrl differ (reload, the address field, select all and the clipboard);
  a `platform` block where the modifier or the key itself differs (a word
  at a time is Option+arrow on macOS and Ctrl+arrow elsewhere; back is
  Cmd+[ or Cmd+← on macOS and Alt+← elsewhere; Alt+D is only off macOS).
- **Input goes only to a page that has been drawn**: after a navigation
  (history, reload, the address field), `wait-loaded` before clicking or
  scrolling in the new page. A query answers as soon as the document is
  parsed, before its first frame.
- **`wait-change` sees a box move** when something before it changes size:
  check an element right after what changes it, or by its text with
  `wait-text`. An empty element has a box too.
- **A box may change before the wait starts**: what a click changes can be
  drawn before `wait-change` looks. `capture` before the click, wait for
  its text, then `expect-differs` the box (`flex.txt`). Hit testing uses
  the last frame drawn, so after a change that moves what is under the
  pointer, wait for it to be drawn before clicking there.
- While script keeps the renderer's main thread busy, it answers no query
  and draws nothing new of the page, and the input that made it busy isn't
  handled yet: `wait-busy`, then point with `click-at` and `wheel-at`, and
  check frames with `capture <name> now` and `expect-differs`
  (`demo-busy.txt`).
- A frame can look the same after a change: scrolling a pattern by its
  period, say. Pick offsets that show.
- An element's box is where the main thread last laid it out and scrolled
  it: right after a wheel the compositor has handled alone, it may not have
  heard of the scroll yet.
