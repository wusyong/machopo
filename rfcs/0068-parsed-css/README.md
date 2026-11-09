# RFC 0068：Speedometer 用到、kun 丟掉的其他 CSS：字型的 property、`border-image`、`cursor` 和介面的 property

- 開始日期：2026-10-21
- 狀態：✅ 已完成：2026-10-21
- 進度和驗收：`progress.md`
- 細節：`details.md`（2026-10-21 的盤點：每個宣告的值、用在哪個 suite；每個 property 的初始值、繼承和 Chrome 的行為）

## 摘要

RFC 0064 補上了 Speedometer 用到、會改變畫面的 CSS；2026-10-21 重跑盤點，剩下被 kun 丟掉的宣告有 43 個 property。它們大多是 shorthand 帶出來的初始值（`border` 帶出 `border-image: none`，`font` 帶出十幾個 `font-*`，`animation` 帶出 `animation-timeline: auto`），少數是頁面真的寫了、而且 Chrome 有效果的值（`cursor: move`、Spectrum 用 `var()` 給的邏輯圓角、`font-variant-ligatures: none`、`font-feature-settings: "liga" 0`、`font-synthesis-*: none`）。這份 RFC 讓這些 property 都能解析、cascade、`getComputedStyle` 讀得回來，shorthand 照 Chrome 展開和寫回；kun 已經有對應機制的，照 Chrome 生效（字型的 OpenType feature、可變字型的軸和 `opsz`、字型的合成、軟連字號的連字號、邏輯圓角、`cursor`），kun 還沒有對應功能的（文字選取、contenteditable 的編輯、平滑捲動、scroll-driven animation、`border-image` 的繪製）只解析，記進限制。做的時候順帶修好了可變字型一律畫成預設 instance 的問題，並讓 cascade 的成本不再跟著 longhand 的總數長。

## 動機

RFC 0064 的「盤點」把 Speedometer 用到、kun 缺的分成四類：畫出來會不一樣的 CSS（RFC 0064，做完了）、只要解析的 CSS（這份）、選取和游標（另一份）、DOM 和 Web API（另一份）。2026-10-21 在 `main`（`b80ebbc3`）上重跑 `bench.py speedometer --inventory`（`details.md` 的「盤點」）：20 個 default suite 都跑得完、沒有例外；RFC 0064 那一類全部消失，剩下的正是這三類，沒有新的。

這一類的宣告被丟掉，量測上有兩種影響：

- **大多不改變畫面，但改變 kun 做的事**：kun 丟掉宣告的 parse 比 Chrome 照存少做事；`getComputedStyle` 讀這些 property 時，kun 回傳空字串，Chrome 回傳值。框架很少讀它們，但和 Chrome 不一樣的路徑都可能讓比較失真。
- **少數改變畫面或行為**：Complex-DOM 的 6 個 suite 的 Spectrum 元件用 `border-*-*-radius`（邏輯圓角）畫圓角，kun 畫成直角；CodeMirror、TipTap 關掉連字（`"liga" 0`、`font-variant-ligatures: none`），kun 照樣連；Stockcharts 的圖表用 `cursor: crosshair`、`move`、`ew-resize` 這些，kun 一律是箭頭（只有連結上是手指）。

依 default suite 數（`details.md`）：

| property | suite | 值 |
| --- | --- | --- |
| `border-image` 和它的 5 個 longhand | 16 | 都是初始值（`border` shorthand 帶出來的） |
| `cursor` | 16 | `pointer`、`default`、`move`、`grabbing`、`crosshair`、`ew-resize`、`ns-resize` |
| `font-feature-settings`、`font-variant-ligatures` | 15 | 多是初始值和 `inherit`；`"liga" 0`、`none` |
| `font-kerning`、`font-stretch`、`font-variant-*` 的其他 5 個、`font-variation-settings`、`font-optical-sizing`、`font-size-adjust`、`font-language-override` | 14 | 都是初始值或 `inherit`（`font` shorthand 帶出來的） |
| `text-rendering` | 14 | `optimizelegibility` |
| `-webkit-font-smoothing` | 12 | `antialiased`、`subpixel-antialiased` |
| `user-select` | 7 | `none` |
| 邏輯圓角（`border-start-start-radius` 等 4 個） | 6 | `0px`、`var(--spectrum-…)` |
| `-webkit-border-horizontal-spacing`、`-vertical-spacing` | 6 | `0px` |
| `forced-color-adjust`、`-webkit-tap-highlight-color`、`resize`、`hyphens` | 6 | `none` / `auto`、`transparent`、`none`、`auto` |
| `scroll-behavior` | 2 | `auto` |
| `animation-timeline`、`animation-range-start`、`-end` | 2 | 初始值（`animation` shorthand 帶出來的） |
| `font-synthesis-weight`、`-style`、`-small-caps` | 2 | `none` |
| `text-size-adjust` | 2 | `none` |
| `-webkit-user-modify` | 1 | `read-write-plaintext-only` |

目標：

- 上表的每個 property（和它所在的 shorthand、同一組的其他 longhand）照 Chrome 解析、cascade（初始值、繼承、CSS-wide 關鍵字、`var()`）、計算、序列化，`getComputedStyle` 讀得回 Chrome 的值；能動畫的照 Chrome 插值。
- `font`、`border`、`animation`、`font-synthesis`、`border-image`、`font-variant`、`border-spacing` 這些 shorthand 照 Chrome 展開（重設它們包含的 longhand）、寫回。
- kun 已經有對應機制的照 Chrome 生效（已決定的事項第 2 到 5 條）。
- 不讓沒用到這些 property 的頁面變慢：多數元素是初始值的放進 `rare` 組。

**不在範圍內**：

- 選取和游標（`::selection`、`caret-color`）：另一份 RFC（RFC 0064 第 2 條）。
- `hyphens: auto` 的斷字：要斷字的字典，另一份 RFC（RFC 0064 第 4 條）；這份只解析。
- 使用者的文字選取（`user-select` 的效果）、contenteditable 的編輯（`-webkit-user-modify`）、`<textarea>` 的 resize 把手（`resize`）、平滑捲動（`scroll-behavior`）、scroll-driven animation（`animation-timeline`、`animation-range-*`）、forced colors 模式（`forced-color-adjust`）、觸控和行動版（`-webkit-tap-highlight-color`、`text-size-adjust`）：kun 還沒有對應的功能，這份只解析，記進限制，等做那些功能的 RFC。
- `border-image` 的繪製（已決定的事項第 4 條）。
- `@font-feature-values`（`font-variant-alternates` 的大部分值要它）。

要解決的 `docs/limitations.md` 裡的限制：

- 「Style」的支援的 property 那一條的「沒有邏輯的圓角（`border-start-start-radius` 這些）」。
- 「Style」的 table 的 property 那一條的「`border-spacing` 是一個 longhand（Chrome 拆成 `-webkit-border-horizontal-spacing`、`-webkit-border-vertical-spacing`，kun 沒有這兩個名字）」。
- 「Style」的「Transition 和 CSS animation」那一條的「沒有 `animation-timeline`、`animation-range-*`」：改成「只解析，沒有 scroll-driven animation」。
- 「連結導航和 hit testing」的「網頁上只有箭頭和手指，文字上沒有 I 形游標」（已決定的事項第 5 條）。

## 設計概要

網頁的作者看到的是：

```css
.button { font: small-caps 600 condensed 14px/1.2 system-ui; }   /* small-caps 和 condensed 不再被丟掉 */
.code { font-variant-ligatures: none; font-feature-settings: "liga" 0; }  /* 不連字 */
.title { font-synthesis: none; }               /* 字型沒有粗體時不合成 */
.chart { cursor: crosshair; }                  /* 游標照 CSS 換 */
.spectrum-button { border-start-start-radius: 4px; }  /* 對到 border-top-left-radius */
.panel { border: 1px solid; }                  /* 照 Chrome 也重設 border-image */
.hide { user-select: none; }                   /* 讀得回來，還沒有使用者的選取，沒有效果 */
```

`getComputedStyle(el).fontVariantCaps` 讀得回 `small-caps`，`el.style.font` 寫回時照 Chrome 合成 shorthand。

給之後改這部分程式的人：

- 新的 longhand 照 `docs/architecture.md` 的「快取和增量：加新東西時要跟著改的地方」加：多數元素是初始值的放進 `rare` 組，少見的繼承 property 放進 `rare_inherited`（`RareInherited`），字型的放進 `font`（`FontDescription`）；只影響 paint 或游標的放進 `StyleDifference::between` 的 paint 那一串；影響 shaping 的交給 parley，而且要在 parley fork 的 `ShapeCache` 的 key 裡。
- 被 shorthand 重設、多數元素是初始值的 longhand（`font` 重設的字型 longhand、`border` 重設的 `border-image`），apply 時值一樣就不寫（`set_font!`、`set_rare!`）：否則每個被選到的元素都複製一份組。
- 繼承靠共用的組（子元素拿 parent 的 `Arc`），`is_inherited` 只決定 `unset` 的行為：測試要包含 `unset`。
- 只解析的 property 也是 `ComputedStyle` 的欄位：`getComputedStyle` 從那裡讀，之後做它的效果的 RFC 直接用。

## 詳細設計

每一項列出選項和 2026-10-21 的決定（「已決定的事項」）。每個 property 的值、初始值和繼承在 `details.md` 的「逐項」。

### A. 只解析的 property

`user-select`、`-webkit-user-modify`、`resize`、`scroll-behavior`、`forced-color-adjust`、`-webkit-tap-highlight-color`、`text-size-adjust`、`hyphens`、`animation-timeline`、`animation-range-start`、`animation-range-end`（`animation-range` shorthand）、`text-rendering`、`-webkit-font-smoothing`：解析、計算、繼承、序列化、`getComputedStyle`；能動畫的照 Chrome（多是離散）。`animation` shorthand 照 Chrome 重設 `animation-timeline` 和 `animation-range-*`（css-animations 2）。

- `text-rendering`：Chrome 的 `optimizeSpeed` 關掉 kerning 和連字（Blink 的 `FontDescription::SetTextRendering`），其他值和 `auto` 一樣。照 B2 一起照做（已決定的事項第 2 條）。
- `-webkit-font-smoothing`：Chrome 只在 macOS 上有效果（`none`、`antialiased`、`subpixel-antialiased`），macOS 從 10.14 起沒有 subpixel 的反鋸齒，`antialiased` 和 `auto` 畫出來一樣；kun 只解析。
- `hyphens`：`none`、`manual`（初始值）、`auto`。斷在軟連字號（U+00AD）時行尾畫連字號，`none` 不在它斷（已決定的事項第 11、14 條，見下面的「軟連字號」）；`auto` 照 `manual`（斷字是另一份 RFC）。
- `animation-timeline` 只收 `auto`，`animation-range-*` 只收 `normal`（已決定的事項第 8 條）。

### B. 字型的 property

`font-variant-ligatures`、`-caps`、`-numeric`、`-east-asian`、`-position`、`-alternates`、`-emoji`（`font-variant` shorthand）、`font-feature-settings`、`font-variation-settings`、`font-kerning`、`font-stretch`、`font-optical-sizing`、`font-size-adjust`、`font-language-override`、`font-synthesis-weight`、`-style`、`-small-caps`（`font-synthesis` shorthand）：解析、計算、繼承、序列化、插值（`font-stretch`、`font-variation-settings`、`font-size-adjust` 能插值）。`font` shorthand 照 Chrome 收 `font-variant-caps` 的 `small-caps` 和 `font-stretch` 的關鍵字（kun 現在收下了卻丟掉），並重設其他的 longhand（`font-variant-*`、`font-feature-settings`、`font-kerning`、`font-optical-sizing`、`font-size-adjust`、`font-language-override`、`font-variation-settings`；Chrome 152 的 `font` 不重設 `font-synthesis-*`，照它）。寫回時照 Chrome：那些 longhand 都是 `font` 重設的值（`font-variant-caps` 是 `normal` 或 `small-caps`，`font-stretch` 是一個關鍵字）才寫成 `font`；`getComputedStyle` 的 `font` 照 Chrome 省略 400 的粗細。`font-variant-*` 的值照 css-fonts 4 的順序寫（Chrome 的宣告值照寫的順序，`font-variant-alternates` 的 computed value 把 `styleset()`、`character-variant()` 放在最後；WPT 照 css-fonts 4）。

要不要生效（已決定的事項第 2 條）：

- **B1**：都只解析，字照舊 shape。
- **B2**：能直接對到 OpenType 的照 Chrome 交給 parley：`font-feature-settings` 和 `font-variant-ligatures`、`-caps`、`-numeric`、`-east-asian`、`-position`、`font-kerning` 成為 feature（Blink 的 `FontFeatures`，交給 HarfBuzz）；`font-variation-settings`、`font-optical-sizing: auto`（`opsz` 軸）成為 variation；`font-stretch` 成為 fontique 選字的 width。`font-size-adjust`、`font-variant-alternates`（要 `@font-feature-values`）、`font-variant-emoji`、`font-language-override` 只解析，記進限制。`font-variant-caps` 的 `small-caps` 在字型沒有 `smcp` 時，Chrome 用縮小的大寫合成；kun 照 B2 只開 feature，沒有合成，記進限制（B3 才做）。
- **B3**：B2 加上 `font-size-adjust` 和 small caps 的合成。

決定：B2。Speedometer 看得到的只有關掉連字（CodeMirror、TipTap），但一旦解析了，頁面上寫的 `font-variant-numeric: tabular-nums`、`small-caps` 這類就該生效，否則是新的「解析了、沒效果」的落差。parley 已經有 `FontFeatures`、`FontVariations`、`FontWidth`，kun 這邊只是把 computed value 轉過去；parley fork 的 `ShapeCache` 的 key 要包含它們（`RunStyle`）。B3 的合成要改 parley 的 shaping（或在 kun 這邊拆 run），Speedometer 沒用到。

`font-synthesis`（已決定的事項第 3 條）：kun 照 Blink 合成粗體（Skia 的 embolden）和斜體（skew）。`font-synthesis-weight: none`、`-style: none` 讓它不合成（Blink 的 `FontDescription::SyntheticBoldAllowed`、`SyntheticItalicAllowed`）。決定：照做，只是合成前多看一個值；`-small-caps` 只解析。

字型的值整組放在 `FontDescription`（已決定的事項第 13 條），繼承時跟著 parent。`layout` 的 `font_features` 模組照 Blink 的 `FontFeatures::Initialize` 和 `UpdateTypesettingFeatures` 的順序把它變成 feature（`font-feature-settings` 最後），連同字寬、`font-variation-settings` 交給 parley；`font-optical-sizing: auto` 由 parley fork 在 shaping 時把 `opsz` 設成字級（第 15 條）。畫字時 `TextFragment`、SVG 的文字和 canvas 帶著 shaping 用的軸的值，raster 照字型和這組值建 Skia 的 typeface 並快取（第 18 條：開始做時發現 kun 畫字一律用可變字型的預設 instance）。

**軟連字號**（第 11、14 條）：parley fork 的 `StyleProperty::SoftHyphens` 讓換行器在軟連字號之後的斷點照它的 style：`hyphens: none` 不斷（最多是 `overflow-wrap` 的緊急斷點），其他的帶著連字號的前進量，連字號放得下、或這一行前面沒有別的斷點時才用，用了就算進這一行的 `advance`（`LineMetrics::hyphen_advance`）；min/max-content 的量測也算進它。kun 只在文字有 U+00AD 時為有軟連字號的 run shape 連字號（U+2010，第一個字型沒有它時是 `-`，Blink 的 `HyphenString`；和省略號共用一個做法），排成片段時放在行的文字的末端（段落的方向）。

### C. `border-image`

`border-image-source`、`-slice`、`-width`、`-outset`、`-repeat` 和 `border-image` shorthand：解析、計算、序列化、插值（`-source` 是 image，照 Chrome 的 image 插值，其他照數值和長度）；`border` shorthand 照 Chrome 重設 `border-image`。

要不要畫（已決定的事項第 4 條）：

- **C1**：只解析，邊框照舊畫成 `border-style` 的；記進限制。
- **C2**：照 Blink 的 `NinePieceImagePainter` 畫九宮格（圖片和漸層、`stretch`、`repeat`、`round`、`space`，`outset` 往外擴、算進 visual rect）。

決定：C1。Speedometer 裡只有初始值；C2 是新的繪製（九宮格的切法和重複、和背景圖共用的圖片載入、raster invalidation），工作量接近 RFC 0064 的一段，之後有頁面用到時另開。

### D. 邏輯圓角和 `border-spacing`

- `border-start-start-radius`、`border-start-end-radius`、`border-end-start-radius`、`border-end-end-radius`：照 kun 其他邏輯 property 的做法（`LonghandId::physical`，`writing-mode: horizontal-tb`、`direction: ltr`）對到 `border-top-left-radius` 這些。`getComputedStyle` 照 Chrome 讀得回邏輯的名字，值和對到的物理 property 一樣。
- `-webkit-border-horizontal-spacing`、`-webkit-border-vertical-spacing`：照 Chrome 把 `border-spacing` 拆成這兩個 longhand，`border-spacing` 是它們的 shorthand（寫回時兩個一樣只寫一個）。

### E. `cursor`

做之前 renderer 照 hit test 的結果送 `CursorType`，只有箭頭（`Pointer`）和連結上的手指（`Hand`），文字上沒有 I 形游標。

- **E1（甲）**：`cursor` 照規範解析（關鍵字，和 `url()` 加座標加 fallback 的清單），computed value 存關鍵字和圖片的網址；renderer 照 Blink 的 `EventHandler::SelectCursor` 選游標：元素的 `cursor` 不是 `auto` 時用它，`auto` 時連結上是手指、可編輯的文字和選得到的文字上是 I 形；`CursorType` 擴成 CSS 的關鍵字，shell 對到 winit 的 `CursorIcon`。`url()` 的游標圖片先不做（照它的 fallback 關鍵字），記進限制。
- **E2（乙）**：只解析，游標照舊。

決定：E1。Stockcharts 的 `crosshair`、`move`，Spectrum 的 `pointer`、`ew-resize` 在 kun 的視窗裡看得到；winit 的 `CursorIcon` 正好對應 CSS 的關鍵字。動到視窗那一層（`CLAUDE.md`：要跑 `test.py automation`），游標的樣子情境做不到、要請人看（`docs/test-gaps.md` 的「只做過手動確認」）；renderer 選了哪個游標可以自動測（看它送出的 `CursorType`）。`auto` 時文字上的 I 形會順便解掉「文字上沒有 I 形游標」的限制，但要判斷「選得到的文字」：`user-select` 不是 `none`（這份解析了它）。

做法（第 19 到 24 條）：`cursor` 放在 `RareInherited`，收 `url()` 加座標的清單和最後的關鍵字（座標照 Chrome 取整數），只用關鍵字；renderer 的 `FrameView::cursor_at` 照 `SelectCursor` 選，`protocol` 的 `CursorType` 照 `ui::mojom::CursorType` 有 34 種，shell 對到 winit 的 `CursorIcon`。連結的手指、文字欄位的 I 形、按鈕和其他控制項的箭頭照 Chrome 的 `html.css` 放進 UA stylesheet（`auto` 在連結上照樣是手指）。滑鼠不動、pre-paint 之後底下的東西變了時也重算（Blink 的 `ScheduleCursorUpdate`）。情境的語言加上 `hover`、`hover-at` 和 `wait-cursor`（自動化協定的 `move` 輸入和 `Kun.getCursor`）。

### 測試

- 每個 property：`css` 的解析和序列化、`style` 的 cascade 和 `getComputedStyle`（`cssom::`、`computed::`、`cascade::`）、插值（`interpolation::`）；shorthand 的展開和寫回（`font`、`border`、`animation` 重設的 longhand）。
- WPT：`css/css-fonts/parsing/`、`css/css-fonts/` 的 `font-variant-*`、`font-feature-settings`、`font-synthesis-*`、`font-stretch` 的 reftest（B2）、`css/css-backgrounds/parsing/border-image-*`、`css/css-logical/` 的圓角、`css/css-tables/` 的 `border-spacing`、`css/css-ui/parsing/`（`cursor`、`user-select`、`resize`）、`css/css-text/parsing/hyphens-*`、`css/cssom-view/parsing/scroll-behavior-*`：用 `--all` 試過、挑通過大半的加進清單。
- 字型的效果（B2）：`layout` 的測試用 WPT 的有 feature 和軸的字型（第 16 條）看 shaping 的結果（連字、caps、數字、上下標、字寬、軸、`opsz`），`raster` 的測試看可變字型照軸畫；WPT 的 `css/css-fonts/` 的 reftest。
- 軟連字號：`layout` 的測試看連字號的位置、斷行、`none`、min-content；WPT 的 `css/css-text/hyphens/` 通過的那些。
- 邏輯圓角：WPT 和情境 `rounded`（點弧外打到頁面）。
- `cursor`（E1）：`browser`、`shell` 的測試看 renderer 送出、shell 選定的 `CursorType`；情境 `cursor`；`test.py automation`；游標的樣子請人看。
- 效能：開始前和做完用 `--against` 在同一時段輪流比 Speedometer 和 blink-perf 的 `css/`。
- 不重排的：`cursor`、`user-select` 這類只影響游標和選取的改變不重排（`StyleDifference`），字型的改變重排並重新 shape。
- mutation test 照 `docs/testing.md`。

### 分階段

每段開始前再決定細節（「已決定的事項」照段落分開）：

1. **只解析的 property 和 shorthand**（A、C、D）：1a 少見的繼承 property 放進共用的組；1b 介面的 property；1c `border-image`；1d 邏輯圓角、`border-spacing` 的 longhand；1e `animation-timeline` 和 `animation-range-*`；`border`、`animation` 的重設。
2. **字型**（B）：2a 字型組和全部的 `font-*` longhand、`font`、`font-variant`、`font-synthesis` shorthand；2b 生效（feature、variation、字寬、`opsz`、合成）和可變字型照軸畫；2c 軟連字號。
3. **`cursor`**（E）：動到 renderer 和視窗。

收尾時查整份的效能，改了 cascade（第 25 條）。第 1 段的改動在 `css`、`computed_style`、`style` 和 `dom_api` 的 `getComputedStyle`；第 2 段加上 `layout` 的文字、`paint`、`raster` 和 parley fork；第 3 段加上 `renderer`、`protocol` 和 `shell`。

## 缺點

- **多一批只解析、沒有效果的 property**：`getComputedStyle` 讀得回 Chrome 的值，頁面看起來像是支援了，行為卻不同（例如 `scroll-behavior: smooth` 照樣跳著捲）；要在 `docs/limitations.md` 寫清楚哪些沒有效果。
- **`ComputedStyle` 多了 41 個 longhand**：大多放進共用的組（主體反而從 88 個欄位變成 72 個），但 cascade 和 `getComputedStyle` 要處理的 property 變多；cascade 只走有宣告的 longhand（第 25 條）之後，Speedometer 和開始前在雜訊範圍內（TodoMVC-Lit-Complex-DOM 慢約 2%）。
- **CSSOM 設讀 `border`、`font`、`font-variant` 慢約 20%**（blink-perf 的 `CSSPropertySetterGetter*`）：照 Chrome 展開成全部的 longhand，宣告區塊的查找是線性的（`docs/performance-gaps.md`）。
- **字型的 feature 和軸讓 shaping 的快取多幾個 key，raster 多一個 instance 的快取**（B2）：instance 的快取跟著 compositor，不會變小（每種字型、每組軸的值一個；macOS 的系統字型有 `opsz`，每個字級一個）。

## 理由和其他做法

- **為什麼不維持丟掉**：丟掉的宣告讓 kun 少做 parse 和 cascade 的工作，`getComputedStyle` 也和 Chrome 不同；而且邏輯圓角、連字、游標這幾個是看得到的差別。
- **為什麼大多只解析**：效果要的功能（文字選取、編輯、平滑捲動、scroll-driven animation、`border-image` 的繪製）各自是一份 RFC 的量，Speedometer 裡用到的值大多是初始值；先讓 cascade 和 `getComputedStyle` 對，之後做功能的 RFC 直接讀 computed value。
- **為什麼和 RFC 0064 分開**：0064 每一項都改畫面、要 reftest；這份大多只改 style，可以和 RFC 0067 同時做。

## 先例

- **Blink**：`css_properties.json5`（每個 property 的初始值、繼承、能不能動畫、在哪個 shorthand）；`FontDescription`（`font-variant-*`、`font-feature-settings`、`font-synthesis` 到 `FontFeatures` 和合成的旗標，交給 HarfBuzz）；`NinePieceImagePainter`（`border-image`）；`CSSDirectionAwareResolver`（邏輯 property）；`EventHandler::SelectCursor`（`cursor`）。
- **Stylo**（Firefox）：同樣的 property 都是 longhand，`font` shorthand 重設 `font-variant-*` 這些；`font-feature-settings` 交給 HarfBuzz。

## 已決定的事項

2026-10-21，草稿之後（使用者）：

1. **範圍**：上表的 property，加上它們所在的 shorthand 和同一組的其他 longhand（例如 `font-variant` 的 7 個、`font-synthesis` 的 3 個），讓 shorthand 的展開和寫回完整。不選只做盤點列出的：`font-variant`、`font-synthesis` 這些 shorthand 少了一個 longhand 就寫不回來。
2. **字型的 property 照 B2 生效**：能直接對到 OpenType 的交給 parley（feature、variation、`font-stretch` 的 width），`text-rendering: optimizeSpeed` 照 Chrome 關掉 kerning 和連字；`font-size-adjust`、`font-variant-alternates`、`font-variant-emoji`、`font-language-override` 只解析，small caps 不合成，記進限制。不選 B1：解析了卻不生效是新的落差，而 parley 已經有這些輸入。不選 B3：合成要改 parley 的 shaping，Speedometer 沒用到。
3. **`font-synthesis`**：`-weight`、`-style` 是 `none` 時不合成粗體和斜體；`-small-caps` 只解析（kun 照第 2 條不合成 small caps）。
4. **`border-image` 照 C1 只解析**，不畫。不選 C2：Speedometer 裡只有初始值，九宮格的繪製工作量接近 RFC 0064 的一段，之後有頁面用到時另開。
5. **`cursor` 照 E1**：CSS 的關鍵字換視窗的游標，`auto` 照 Blink 的 `SelectCursor`（連結上是手指、選得到的文字和可編輯的地方是 I 形、其他是箭頭）；`url()` 的游標圖片先不做，照它的 fallback 關鍵字。不選 E2：Speedometer 的圖表和 Spectrum 用的游標在視窗裡看得到，winit 的 `CursorIcon` 正好對應 CSS 的關鍵字。
6. **分三段**：只解析的 property 和 shorthand（A、C、D）、字型（B）、`cursor`（E），每段做完、經過審核可以先 merge 回 `main`。

2026-10-21，第 1 段開始前（使用者）：

7. **只解析的 property 變了只重畫，不重新 layout**：它們加進 `StyleDifference::between` 的 paint 那一串（Blink 的這些 property 變了也不觸發 layout）。不選維持預設（重新 layout）：`user-select` 這類常被 script 切換的會白做 layout。
8. **`animation-timeline` 只收 `auto`，`animation-range-start`、`-end` 和 `animation-range` 只收 `normal`**（加上 CSS-wide 關鍵字和 `var()`），其他的值當成無效。不選 Chrome 的完整文法（`scroll()`、`view()`、`--name`、`entry 10%`）：頁面用 `@supports (animation-timeline: scroll())` 判斷支不支援，收了它，kun 卻照時間跑動畫（`animation-duration: auto` 是 0 秒，直接跳到最後），比不支援還糟；只收 kun 的行為和 Chrome 一樣的值，`@supports` 照實回答，頁面走 fallback。Speedometer 只有 `animation` shorthand 帶出來的 `auto` 和 `normal`。
9. **`border-image-source: url()` 不下載圖片**：只解析，`getComputedStyle` 讀回絕對網址（Chrome 下載，會延後 `load` 事件、佔圖片快取，kun 不畫它，沒有差別），記進限制。
10. **`user-select` 照 Chrome 繼承**（規範不繼承，靠 `auto` 的計算看 parent），**收 `-webkit-user-select`、`-webkit-text-size-adjust` 兩個別名**。不選照規範：parent 是 `none` 時 child 讀回的值和 Chrome 不同。
11. **軟連字號在第 2 段做**：第 1 段只解析 `hyphens`；斷在 U+00AD 時畫連字號、放不放得下算進它的寬度（Blink 的 `HyphenResult`），`hyphens: none` 時不在 U+00AD 斷行，和字型一起在第 2 段改 parley fork。不選記進限制、留給 `hyphens: auto` 的 RFC：`manual` 是初始值，任何用 `&shy;` 的頁面都看得出差別。

12. **少見的繼承 property 放進共用的 `Arc<RareInherited>`**（Blink 的 `StyleRareInheritedData`）：這段新增的 8 個，加上現有的 `letter-spacing`、`word-spacing`、`tab-size`、`text-indent`、`word-break`、`overflow-wrap`、套用的裝飾線、`text-shadow`、`-webkit-text-fill-color`、`text-underline-offset`、`text-underline-position`、`-webkit-text-security`、`border-spacing`、`list-style-type`；`color`、字型、`line-height` 和一個 byte 的關鍵字留在外面（Blink 的 inherited flags）。繼承時只複製一個 `Arc`，`same_inherited` 先比指標。第 1 段的第一步只搬欄位、不改行為。第 2 段的字型 property 另開字型的組（Blink 的 `FontDescription`），到時再定。不選只放新的 8 個：兩種放法並存。不選全部的繼承 property 都進一組：設了 `color`、`font-size` 的元素很多，每個都要複製整組。

2026-10-21，第 2 段開始前（使用者）：

13. **字型的 property 放進一個共用的字型組**（Blink 的 `FontDescription`）：這段新增的字型 longhand，加上現有的 `font-family`、`font-size`、`font-weight`、`font-style` 搬進同一個 `Arc` 的組，繼承時整組跟著 parent；`line-height` 留在外面（Blink 也不在 `FontDescription` 裡）。apply 時值和現在的一樣就不寫，不然 `font` shorthand（和 `font: inherit`）重設的十幾個 longhand 會讓每個被它選到的元素複製一份。不選放進 `RareInherited`：字型的 property 在 Blink 自成一組，`font` shorthand 一次設它們，分開放的話它們和其他少見的繼承 property 互相拖累。
14. **軟連字號改 parley fork**：換行時在 U+00AD 的斷點把連字號的寬度算進放不放得下（Blink 的 `NGLineBreaker` 的 `HyphenResult`），記下這一行斷在軟連字號，kun 照補省略號的做法在行尾畫連字號（字型有 U+2010 就用它，沒有就用 `-`，Blink 的 `HyphenString`）；`hyphens: none` 用 kun 自己的 `StyleProperty`（前例是 tab stops）讓 parley 不在 U+00AD 斷行。不選只在 kun 補畫、不預留寬度：行尾會超出約 0.3em。
15. **`font-optical-sizing: auto` 生效**：fork 的 shaping 對有 `opsz` 軸的字型設 `opsz` 等於字級（px），`font-variation-settings` 寫了 `opsz` 時照它（css-fonts 4、Blink）。不選只解析：macOS 的系統字型 SF 有 `opsz` 軸，Chrome 會套。
16. **測 feature 和可變軸的字型只在測試裡從 WPT 的 checkout 讀**（`css/css-fonts/support/fonts/` 的 `FontWithFeatures2.otf`、`RobotoExtremo-VF.subset.ttf`），不打包進執行檔：兩個字型都沒有寫授權（RobotoExtremo 只寫 Google 的著作權，WPT 的 README 也沒有說明），確認不了能不能隨 kun 散布，所以不選放進 `resources/test_fonts/`。layout 的測試能用它們，kun 自己的 reftest 不能（只有 Lato 有的 feature 寫成 reftest）。
17. **第 2 段分三小段**：2a 解析（全部的字型 longhand 和 `font`、`font-variant`、`font-synthesis`，`font` 收 `small-caps` 和字寬並重設其他 longhand，system font；字型組的搬家在這裡），2b 字型生效（feature 照 Blink 的順序合併、variation、字寬、`opsz`、`font-synthesis`、`text-rendering: optimizeSpeed`），2c 軟連字號。

2026-10-21，2b 做到一半（使用者）：

18. **可變字型照 shaping 用的軸的值畫**：2b 開始時發現 kun 畫字一律用字型的預設 instance，軸的值只給了 shaping（macOS 的 `system-ui` 是可變字型，`bold` 畫成一般的粗細、字距卻是粗體的）。`TextFragment` 帶著 shaping 用的軸的值（fontique 合成的 `wght` 這些，加上 `font-variation-settings` 和 `opsz`），raster 用它們建 Skia 的 typeface（`clone_with_arguments`，Blink 的 `FontPlatformData` 帶著 variation 建 `SkTypeface`），照字型和這組值快取；在 2b 裡做，順便修好既有的粗體。不選只讓 variation 影響 shaping（`opsz` 只改字距不改字形，比不改還糟）或另開一份 RFC（2b 的 variation 和 `opsz` 要等它）。

2026-10-21，第 3 段開始前（使用者）：

19. **`cursor` 照規範收 `url()` 的清單**：`[<url> [<x> <y>]?,]* <關鍵字>`，computed value 存絕對網址和座標，`getComputedStyle` 照 Chrome 寫回；kun 只用最後的關鍵字（第 5 條）。收 Chrome 的別名（`-webkit-grab`、`-webkit-grabbing`、`-webkit-zoom-in`、`-webkit-zoom-out`）。不選只收關鍵字：網頁常寫 `url(x.cur), pointer`，整條宣告無效會連 `pointer` 都丟掉。
20. **`cursor` 放進 `RareInherited`**（它繼承；Blink 的 `StyleRareInheritedData` 有 `cursor_data` 和 `cursor`），變了不重新排版（`StyleDifference` 的 paint 那一串）。不選放在本體：每個 style 多一個欄位。
21. **`protocol` 的 `CursorType` 照 Chromium 的 `ui::mojom::CursorType` 擴充**（`IBeam`、`Cross`、`Move`、`EastWestResize` 這些），shell 對到 winit 的 `CursorIcon`，`none` 藏起游標。不選用 CSS 的名字：protocol 的型別照 Chromium 命名。
22. **滑鼠不動、底下的內容變了也更新游標**（Blink 的 `EventHandler::ScheduleCursorUpdate`，排版、捲動、style 改變之後的 50ms timer）：滑鼠在頁面上、一個 frame 的 pre-paint 變了時，renderer 用上次的滑鼠位置重新選一次。不選只在滑鼠移動時：script 改了 `cursor`、捲動之後，要等滑鼠再動才換。
23. **`auto` 時，`user-select` 不是 `none` 的文字上是 I 形**，照 Blink 的 `SelectAutoCursor`，雖然 kun 還不能選取頁面的文字（記進限制）。不選只有可編輯的地方是 I 形：和 Chrome 不同，等選取的 RFC 時還要再改。
24. **游標用情境測**：情境的語言加上檢查游標的步驟（`shell` 的 `scenario.rs`，它在 renderer 送出、shell 選定的游標上檢查），renderer 送出的 `SetCursor` 另外由 `browser`、`shell` 的測試檢查；游標的樣子請人看。不選只有 renderer 的測試：視窗看得到的行為要寫成情境。

2026-10-21，收尾時查變慢的原因（使用者）：

25. **cascade 只走有宣告的 longhand，`border` 重設 `border-image` 時值一樣就不寫**：longhand 從 206 個變成 247 個，cascade 對每個元素跑三趟長度是全部 longhand 數的迴圈（套用勝出的宣告、找作者設的背景和邊框），成本跟著總數長；改成另外記下哪幾格有勝出的宣告（Blink 的 `CascadeMap::native_property_bits_`），只走那幾格。`border` 照 Chrome 重設 `border-image` 之後，每個設了 `border` 的元素都複製一份 `rare` 組，改成和 `set_font!` 一樣值相同時不寫。不選接受（Speedometer 平均 +1% 在雜訊裡，但 blink-perf 的 custom property 繼承慢 7% 到 8%，原因明確，之後每加一個 property 都會再慢一點）。CSSOM 設讀 `border`、`font` 這些 shorthand 變慢約 20%（展開成更多 longhand，宣告區塊的查找是線性的）不在這份 RFC 裡改：那是照 Chrome 展開之後的工作量，改它要換宣告區塊的結構，記在 `docs/performance-gaps.md`。

## 待決定

沒有。

## 之後的可能

- `border-image` 的繪製。
- 使用者的文字選取（`user-select` 生效）、contenteditable 的編輯（`-webkit-user-modify`、`contenteditable="plaintext-only"`）、`<textarea>` 的 resize 把手（`resize`）、平滑捲動（`scroll-behavior` 和 `ScrollToOptions` 的 `behavior: 'smooth'`）、scroll-driven animation（`animation-timeline`、`animation-range-*`、`ScrollTimeline`、`ViewTimeline`）。
- `font-size-adjust`、`@font-feature-values` 和 `font-variant-alternates`、small caps 和上下標（`font-variant-position`）的合成：字型沒有 `smcp`、`sups`、`subs` 時，Chrome 縮小大寫、縮小並移動字，kun 照樣用原本的字形（記在限制）。
- CSSOM 的宣告區塊照 longhand 查找（設讀展開成很多 longhand 的 shorthand）。
- `cursor: url()` 的游標圖片。
- `hyphens: auto` 的斷字（另一份 RFC）。
