# RFC 0068 的細節

這份是 `README.md` 的補充：盤點的結果、每個 property 在 Speedometer 裡的值，和每個 property 的初始值、繼承、Chrome 的行為。

## 盤點

2026-10-21，`python3 scripts/bench.py speedometer --inventory`，kun `b80ebbc3`，Chrome for Testing 152.0.7977.82。20 個 default suite 在 kun 上都跑得完、沒有例外。probe 和 RFC 0064 的一樣：把 Chrome 讀到的宣告設在一個元素的 `style` 上，讀不回來的就是 kun 丟掉的；它只看解析。

丟掉的宣告依類別：

- **只要解析（這份 RFC）**：43 個 property，見下一節。
- **選取和游標（另一份 RFC）**：`caret-color`（2 個 suite：Editor-CodeMirror、Editor-TipTap）；選擇器 `::selection`（同樣兩個）。
- **DOM 和 Web API（另一份 RFC）**：HTML 元素各自的 interface（`div` 14 個 suite、`li` 11、`ul` 10、`span` 9、`h1` 7……）、`XMLHttpRequest`、`window.opener`、`FileReader`、`PerformanceObserver` 和 Navigation、Resource Timing、Trusted Types、`navigator.geolocation` / `connection` / `scheduling`、`console.createTask`、`MouseEvent.fromElement`、`document.all`、`document.prerendering`、`IntersectionObserver` 的 v2、`HTMLLIElement.type`、`HTMLUListElement.type`、`HTMLBodyElement.onfocus`、runner 的 `navigator.wakeLock`。

RFC 0064 那一類（畫出來會不一樣的）都不見了，也沒有新的類別。

## 每個 suite 丟掉的宣告

default suite 依 CSS 分成幾群（每一群丟掉的 property 一樣）：

| 群 | suite | 丟掉的 property |
| --- | --- | --- |
| TodoMVC | JavaScript-ES5、WebComponents、React-Redux、Backbone、Vue、jQuery（6） | `border-image-*` 的 5 個 longhand、`cursor`、`-webkit-font-smoothing`、`text-rendering`，`font` shorthand 帶出來的 `font-*`（`font-feature-settings`、`-kerning`、`-language-override`、`-optical-sizing`、`-size-adjust`、`-stretch`、`-variant-alternates`、`-caps`、`-east-asian`、`-emoji`、`-ligatures`、`-numeric`、`-position`、`font-variation-settings`） |
| TodoMVC 的 Complex-DOM | JavaScript-ES6-Webpack、React、Angular、Preact、Svelte、Lit（6） | TodoMVC 的那些，加上 Spectrum 的 `border-start-start-radius` 等 4 個邏輯圓角、`border-image` shorthand、`forced-color-adjust`、`-webkit-tap-highlight-color`、`-webkit-border-horizontal-spacing`、`-vertical-spacing`、`user-select`、`hyphens`、`resize` |
| NewsSite | Next、Nuxt（2） | `border-image-*`、`cursor`、`font` 帶出來的 `font-*`、`scroll-behavior` |
| Editor-CodeMirror | 1 | `border-image-*`、`cursor`、`-webkit-user-modify`、`animation-timeline`、`animation-range-start`、`-end`、`font-synthesis-weight`、`-style`、`-small-caps`、`text-rendering`、`text-size-adjust`（另有 `caret-color`） |
| Editor-TipTap | 1 | `border-image-*`、`font-feature-settings`、`font-variant-ligatures`、`animation-timeline`、`animation-range-*`、`font-synthesis-*`、`text-rendering`、`text-size-adjust`（另有 `caret-color`） |
| Charts-chartjs | 1 | `user-select` |
| React-Stockcharts-SVG | 1 | `cursor`（7 個值） |
| Charts-observable-plot、Perf-Dashboard | 2 | 沒有 |

## 逐項

「值」是盤點時 Chrome 讀到、kun 丟掉的宣告值。初始值和繼承照 css 的規範和 Blink 的 `css_properties.json5`；介面的 property、`border-image`、`border-spacing`、`font-stretch` 的值 2026-10-21 在 Chrome 152 上用 `getComputedStyle` 和 `el.style` 確認過（「Chrome 上確認的事」）。**標了「要查」的還沒確認**。

### 字型

都繼承；`font` shorthand 重設它們（css-fonts 4：`font` 能設的是 `font-style`、`font-variant-caps` 的 `normal` / `small-caps`、`font-weight`、`font-stretch` 的關鍵字、`font-size`、`line-height`、`font-family`，其他重設成初始值）。

| property | 初始值 | 能插值 | Speedometer 的值 | 效果怎麼做（B2） |
| --- | --- | --- | --- | --- |
| `font-feature-settings` | `normal` | 否 | `normal`、`inherit`、`"liga" 0`（TipTap） | 交給 parley 的 `FontFeatures`；在 `font-variant-*` 的 feature 之後套用（css-fonts 4 的「Feature precedence」） |
| `font-variant-ligatures` | `normal` | 否 | `normal`、`none`（TipTap）、`inherit` | `liga`、`clig`、`dlig`、`hlig`、`calt` |
| `font-variant-caps` | `normal` | 否 | `normal` | `smcp`、`c2sc`、`pcap`、`c2pc`、`unic`、`titl`；字型沒有時不合成 |
| `font-variant-numeric` | `normal` | 否 | `normal` | `lnum`、`onum`、`pnum`、`tnum`、`frac`、`afrc`、`ordn`、`zero` |
| `font-variant-east-asian` | `normal` | 否 | `normal` | `jp78` 等、`fwid`、`pwid`、`ruby` |
| `font-variant-position` | `normal` | 否 | `normal` | `sups`、`subs`；字型沒有時不合成（Chrome 合成，Blink 的 `FontDescription::VariantPosition`，要查哪個版本開始） |
| `font-variant-alternates` | `normal` | 否 | `normal` | 只解析（大部分值要 `@font-feature-values`；`historical-forms` 是 `hist`，可以一起交給 parley） |
| `font-variant-emoji` | `normal` | 否 | `normal` | 只解析 |
| `font-kerning` | `auto` | 否 | `auto` | `none` 關掉 `kern` |
| `font-stretch` | `normal`（`100%`） | 是 | `normal` | fontique 選字的 width；computed value 讀回百分比（`condensed` 讀回 `75%`），宣告值留著關鍵字 |
| `font-optical-sizing` | `auto` | 否 | `auto` | `auto` 時把字級設成 `opsz` 軸（使用者沒在 `font-variation-settings` 給 `opsz` 時） |
| `font-variation-settings` | `normal` | 是 | `normal` | 交給 parley 的 `FontVariations` |
| `font-size-adjust` | `none` | 是 | `none` | 只解析（要量字型的 x-height 再調字級） |
| `font-language-override` | `normal` | 否 | `normal` | 只解析 |
| `font-synthesis-weight` | `auto` | 否 | `none`（Editor 的 2 個） | `none` 時不合成粗體 |
| `font-synthesis-style` | `auto` | 否 | `none` | `none` 時不合成斜體 |
| `font-synthesis-small-caps` | `auto` | 否 | `none` | kun 不合成 small caps，`none` 沒有差別 |
| `text-rendering` | `auto` | 否 | `optimizelegibility` | Chrome 的 `optimizeSpeed` 關掉 kerning 和連字，其他值和 `auto` 一樣 |
| `-webkit-font-smoothing` | `auto` | 否 | `antialiased`、`subpixel-antialiased` | 只解析（Chrome 只在 macOS 上有作用） |

`font-variant` shorthand 寫 7 個 `font-variant-*`；`font-synthesis` shorthand 寫 3 個 `font-synthesis-*`（`weight`、`style`、`small-caps` 的清單，或 `none`）。

kun 的 parley fork：`StyleProperty::FontFeatures`、`FontVariations`、`FontWidth` 都有；fork 裡的 `ShapeCache` 照 run 的樣式（`RunStyle`）做 key，新的 feature 和 variation 要進 key，不然同一個字串不同 feature 會拿到另一份的 shaping。kun 的合成在 `layout/src/text.rs`（粗體看 `needs_synthetic_bold`，斜體看 parley 的 `run.synthesis().skew()`）。

### `border-image`

都不繼承；`border` shorthand 重設 `border-image`（css-backgrounds 3）。

| property | 初始值 | 插值 | Speedometer 的值 |
| --- | --- | --- | --- |
| `border-image-source` | `none` | image | `none` |
| `border-image-slice` | `100%` | 數字和百分比（`fill` 一樣才插值） | `100%` |
| `border-image-width` | `1` | 數字、長度、百分比 | `1` |
| `border-image-outset` | `0` | 數字、長度 | `0` |
| `border-image-repeat` | `stretch` | 離散 | `stretch` |

16 個 suite 裡全是初始值（`border` shorthand 和 Spectrum 的 `border-image: none` 帶出來的）。Chrome 讀 `border-image` shorthand 時寫回 `none`。

### 邏輯圓角和 `border-spacing`

| property | 初始值 | 繼承 | Speedometer 的值 |
| --- | --- | --- | --- |
| `border-start-start-radius`、`-start-end-`、`-end-start-`、`-end-end-` | `0` | 否 | `0px`、`var(--spectrum-…)`（Spectrum 的按鈕、textfield 等；Complex-DOM 的 6 個 suite） |
| `-webkit-border-horizontal-spacing`、`-vertical-spacing` | `0` | 是 | `0px`（Spectrum 的 `border-spacing: 0`） |

`horizontal-tb`、`ltr` 時：start-start 是 top-left、start-end 是 top-right、end-start 是 bottom-left、end-end 是 bottom-right。

### 介面和其他

| property | 初始值 | 繼承 | Speedometer 的值 | kun 的效果 |
| --- | --- | --- | --- | --- |
| `cursor` | `auto` | 是 | `pointer`、`default`（多數 suite）；Stockcharts 另有 `crosshair`、`move`、`grabbing`、`ew-resize`、`ns-resize` | 照關鍵字換游標，`url()` 先不做 |
| `user-select` | `auto` | **Chrome 是繼承的**（規範不繼承，靠 `auto` 的計算看 parent）：parent 是 `none` 時 child 讀回 `none` | `none`（Spectrum、Chart.js） | 只解析；選第 5 條 E1 時，文字上的 I 形游標要看它 |
| `-webkit-user-modify` | `read-only` | 是 | `read-write-plaintext-only`（CodeMirror） | 只解析 |
| `resize` | `none` | 否 | `none` | 只解析（UA stylesheet 的 `textarea { resize: auto }` 照 Chrome 加） |
| `scroll-behavior` | `auto` | 否 | `auto`（NewsSite） | 只解析 |
| `hyphens` | `manual` | 是 | `auto`（Spectrum） | 只解析；`manual` 的 U+00AD 見 `README.md` 的待決定第 1 條 |
| `forced-color-adjust` | `auto` | 是 | `auto`、`none`（Spectrum） | 只解析 |
| `-webkit-tap-highlight-color` | `rgba(0, 0, 0, 0.18)`（Chrome 的） | 是 | `rgba(0, 0, 0, 0)`（Spectrum） | 只解析 |
| `text-size-adjust` | `auto` | 是 | `none`（Editor 的 2 個） | 只解析 |
| `animation-timeline` | `auto` | 否 | `auto`（`animation` shorthand 帶出來的） | 只解析 |
| `animation-range-start`、`-end` | `normal` | 否 | `normal`（同上） | 只解析 |

`animation-timeline`、`animation-range-*` 是清單，和其他 `animation-*` 一樣照 `animation-name` 的個數對齊。Chrome 的 `animation` shorthand 重設 `animation-timeline` 和 `animation-range-*`，但不收它們的值（css-animations 2：shorthand 裡寫不出 timeline）；序列化時它們不是初始值，shorthand 寫回空字串。

## Chrome 上確認的事

2026-10-21，Chrome 152.0.7977（`getComputedStyle` 和 `el.style`）：

- **別名**：`-webkit-user-select` 是 `user-select` 的別名，`-webkit-text-size-adjust` 是 `text-size-adjust` 的（`cssText` 寫沒有前綴的名字）；沒有 `-webkit-hyphens`。
- **不收的值**：`user-select: contain`、`element`；`cursor: hand`、只有 `url()` 沒有關鍵字的 `cursor`；`-webkit-font-smoothing: grayscale`；`fill` 不在頭尾的 `border-image-slice`。收 `-webkit-grab`、`-webkit-zoom-in` 這類舊的 `cursor` 關鍵字，`forced-color-adjust: preserve-parent-color`，`resize: block`、`inline`。
- **computed value**：`resize: auto` 讀回 `both`（任何元素；Chrome 的 textarea 能調整大小的設定），`<textarea>` 讀回 `both`；`text-size-adjust: none` 讀回 `100%`；`text-rendering` 的關鍵字讀回小寫（`optimizelegibility`）；`cursor` 的 `url()` 讀回絕對網址；`font-synthesis` 的初始值讀回 `weight style small-caps`，只有 `font-synthesis-weight: none` 時讀回 `style small-caps`。
- **清單**：`animation-timeline` 等清單的 computed value 不照 `animation-name` 的個數對齊（`animation-duration: 1s, 2s` 配三個名字讀回 `1s, 2s`）。
- **shorthand**：
  - `border` 重設 `border-image`；`border-image` 不是初始值時，`border` 讀回空字串，`cssText` 拆成 `border-width`、`border-style`、`border-color`、`border-image`。
  - `animation` 重設 `animation-timeline` 和 `animation-range`，讀回時不寫它們；它們不是初始值時，`animation` 讀回空字串，`cssText` 拆成 longhand 和 `animation-range` shorthand。
  - `border-image: url(a.png) 30 round` 寫回 `url("a.png") 30 / 1 / 0 round`；`border-spacing: 3px 5px` 讀回 `3px 5px`，兩個 longhand 一樣時寫成一個值；只有一個 longhand 時 `border-spacing` 讀回空字串。
  - 宣告裡的邏輯圓角照原本的名字留著（`border-start-start-radius: 4px` 時 `borderTopLeftRadius` 讀回空字串，`border-radius` 和它同時在時 `cssText` 兩個都寫）。
  - `font: small-caps condensed 12px serif` 寫回原樣，`font-feature-settings` 這些重設成 `normal`。

## 軟連字號

2026-10-21，`kun --headless`（`main` 在 `b63cd58f`）：kun 在 U+00AD 斷行（parley 的換行照 UAX #14，U+00AD 是 `BA`），沒斷的地方不畫（default ignorable），但斷了之後行尾沒有連字號；Chrome 畫連字號（`hyphens: manual` 是初始值），而且決定一行放不放得下時算進它的寬度（Blink 的 `NGLineBreaker` 的 `HyphenResult`）。`hyphens: none` 時 Chrome 不在 U+00AD 斷行。WPT 的 `css/css-text/hyphens/hyphens-manual-*` 在 kun 上全部失敗，但它們用 `ch` 定寬度（kun 沒有 `ch`），不只是連字號的問題。

## Chrome 的游標

Blink 的 `EventHandler::SelectCursor`（`third_party/blink/renderer/core/input/event_handler.cc`）：

1. 元素的 style 的 `cursor` 清單裡有 `url()` 的圖片而且載入了，用圖片（kun 先不做）。
2. `cursor` 是 `auto` 時：在可編輯的地方或選得到的文字上是 I 形（`SelectAutoCursor`：hit test 的 node 是文字、`user-select` 不是 `none`，或在可編輯的元素裡），連結上是手指（`pointer`），其他是箭頭。
3. 其他的關鍵字照對應的平台游標（`ui::mojom::CursorType`）。

winit 的 `CursorIcon` 照 CSS 的關鍵字命名（`Default`、`Pointer`、`Text`、`Crosshair`、`Move`、`Grab`、`Grabbing`、`EwResize`、`NsResize`、`NotAllowed`、`Wait`、`Progress`、`Help`、`ContextMenu`、`Cell`、`VerticalText`、`Alias`、`Copy`、`NoDrop`、`AllScroll`、`ZoomIn`、`ZoomOut` 和各方向的 resize），`none` 用 `Window::set_cursor_visible(false)`。

## 參考資料

- css-fonts 4：<https://drafts.csswg.org/css-fonts-4/>
- css-backgrounds 3 的 `border-image`：<https://drafts.csswg.org/css-backgrounds-3/#border-images>
- css-logical 1 的圓角：<https://drafts.csswg.org/css-logical-1/#border-radius-properties>
- css-ui 4（`cursor`、`user-select`、`resize`）：<https://drafts.csswg.org/css-ui-4/>
- css-text 3 的 `hyphens`：<https://drafts.csswg.org/css-text-3/#hyphenation>
- css-animations 2 和 scroll-animations 1：<https://drafts.csswg.org/css-animations-2/>、<https://drafts.csswg.org/scroll-animations-1/>
- Blink：`third_party/blink/renderer/core/css/css_properties.json5`、`platform/fonts/font_description.h`、`platform/fonts/shaping/font_features.cc`、`core/paint/nine_piece_image_painter.cc`、`core/input/event_handler.cc`
