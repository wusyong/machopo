# RFC 0076 的細節

## Chrome 152 的探測

2026-10-23，Fedora 44（AMD Ryzen AI 9 HX 370），Chrome for Testing 152.0.7977.82（`--headless=new`），經過 `scripts/bench/common.py` 的 `ChromePipe`（CDP）。kun 是 `main` 的 `9357e4d9`。

### `image-set()` 的解析和序列化

每個值設給一個元素的 `style.backgroundImage`，讀回 `element.style.backgroundImage`、`getComputedStyle(…).backgroundImage` 和 `CSS.supports('background-image', 值)`。網址是測試的 server 的，寫成 `…`。

| 寫的 | `element.style` 讀回 | `getComputedStyle` 讀回 | `CSS.supports` |
| --- | --- | --- | --- |
| `image-set(url(a.png) 1x, url(b.png) 2x)` | `image-set(url("a.png") 1x, url("b.png") 2x)` | `image-set(url("…/a.png") 1dppx, url("…/b.png") 2dppx)` | 是 |
| `-webkit-image-set(url(a.png) 1x, url(b.png) 2x)` | `image-set(url("a.png") 1x, url("b.png") 2x)` | `image-set(url("…/a.png") 1dppx, url("…/b.png") 2dppx)` | 是 |
| `image-set("a.png" 1x, "b.png" 2x)` | `image-set(url("a.png") 1x, url("b.png") 2x)` | `image-set(url("…/a.png") 1dppx, url("…/b.png") 2dppx)` | 是 |
| `-webkit-image-set("a.png" 1x, "b.png" 2x)` | `image-set(url("a.png") 1x, url("b.png") 2x)` | `image-set(url("…/a.png") 1dppx, url("…/b.png") 2dppx)` | 是 |
| `image-set(url(a.png))` | `image-set(url("a.png") 1x)` | `image-set(url("…/a.png") 1dppx)` | 是 |
| `image-set(url(a.png) 1dppx, url(b.png) 192dpi)` | `image-set(url("a.png") 1dppx, url("b.png") 192dpi)` | `image-set(url("…/a.png") 1dppx, url("…/b.png") 2dppx)` | 是 |
| `image-set(url(a.png) 2x, url(b.png) 1x)` | `image-set(url("a.png") 2x, url("b.png") 1x)` | `image-set(url("…/a.png") 2dppx, url("…/b.png") 1dppx)` | 是 |
| `image-set(url(a.png) 1x type("image/png"), url(b.png) 1x type("image/x-nope"))` | `image-set(url("a.png") 1x type("image/png"), url("b.png") 1x type("image/x-nope"))` | `image-set(url("…/a.png") 1dppx type("image/png"), url("…/b.png") 1dppx type("image/x-nope"))` | 是 |
| `image-set(url(a.png) type("image/png") 1x)` | `image-set(url("a.png") 1x type("image/png"))` | `image-set(url("…/a.png") 1dppx type("image/png"))` | 是 |
| `image-set(url(a.png) 1x, linear-gradient(red, blue) 2x)` | `image-set(url("a.png") 1x, linear-gradient(red, blue) 2x)` | `image-set(url("…/a.png") 1dppx, linear-gradient(rgb(255, 0, 0), rgb(0, 0, 255)) 2dppx)` | 是 |
| `image-set(linear-gradient(red, blue) 1x)` | `image-set(linear-gradient(red, blue) 1x)` | `image-set(linear-gradient(rgb(255, 0, 0), rgb(0, 0, 255)) 1dppx)` | 是 |
| `image-set(url(a.png) 0x)` | `image-set(url("a.png") 0x)` | `image-set(url("…/a.png") 0dppx)` | 是 |
| `image-set(url(a.png) -1x)` | `（空字串）` | `none` | 否 |
| `image-set(url(a.png) 1x, url(b.png) 1x)` | `image-set(url("a.png") 1x, url("b.png") 1x)` | `image-set(url("…/a.png") 1dppx, url("…/b.png") 1dppx)` | 是 |
| `image-set()` | `（空字串）` | `none` | 否 |
| `image-set(url(a.png) 1x,)` | `（空字串）` | `none` | 否 |
| `image-set(image-set(url(a.png) 1x) 1x)` | `（空字串）` | `none` | 否 |
| `image-set(url(a.png) 1.5x, url(b.png) 3x)` | `image-set(url("a.png") 1.5x, url("b.png") 3x)` | `image-set(url("…/a.png") 1.5dppx, url("…/b.png") 3dppx)` | 是 |
| `image-set(url(a.png) 96dpi)` | `image-set(url("a.png") 96dpi)` | `image-set(url("…/a.png") 1dppx)` | 是 |
| `image-set(url(a.png) 1dpcm)` | `image-set(url("a.png") 1dpcm)` | `image-set(url("…/a.png") 0.0264583dppx)` | 是 |
| `image-set(url(a.png) calc(1x + 1x))` | `image-set(url("a.png") calc(2dppx))` | `image-set(url("…/a.png") 2dppx)` | 是 |
| `image-set(url(a.png) 1X)` | `image-set(url("a.png") 1x)` | `image-set(url("…/a.png") 1dppx)` | 是 |
| `IMAGE-SET(url(a.png) 1x)` | `image-set(url("a.png") 1x)` | `image-set(url("…/a.png") 1dppx)` | 是 |
| `image-set(url(a.png) 1x), url(c.png)` | `image-set(url("a.png") 1x), url("c.png")` | `image-set(url("…/a.png") 1dppx), url("…/c.png")` | 是 |
| `image-set(url(a.png) 1x, "b.png")` | `image-set(url("a.png") 1x, url("b.png") 1x)` | `image-set(url("…/a.png") 1dppx, url("…/b.png") 1dppx)` | 是 |
| `image-set(url(a.png) 1x, url(b.png) 2x, url(c.png) 3x)` | `image-set(url("a.png") 1x, url("b.png") 2x, url("c.png") 3x)` | `image-set(url("…/a.png") 1dppx, url("…/b.png") 2dppx, url("…/c.png") 3dppx)` | 是 |
| `image-set(url(a.png) 1x type("image/webp"))` | `image-set(url("a.png") 1x type("image/webp"))` | `image-set(url("…/a.png") 1dppx type("image/webp"))` | 是 |
| `image-set(url(a.png) 1x type("IMAGE/PNG"))` | `image-set(url("a.png") 1x type("IMAGE/PNG"))` | `image-set(url("…/a.png") 1dppx type("IMAGE/PNG"))` | 是 |
| `image-set(url(a.png) type("image/avif"), url(b.png) type("image/png"))` | `image-set(url("a.png") 1x type("image/avif"), url("b.png") 1x type("image/png"))` | `image-set(url("…/a.png") 1dppx type("image/avif"), url("…/b.png") 1dppx type("image/png"))` | 是 |

整理：

- `-webkit-image-set()` 和名稱的大小寫都讀回 `image-set()`；只寫字串的選項讀回 `url("…")`。
- 解析度可以省略（`1x`），單位保留到 specified value（`x`、`dppx`、`dpi`、`dpcm`、`calc()` 化簡成 `calc(2dppx)`），computed value 一律 `dppx`（`1dpcm` 是 `0.0264583dppx`、`192dpi` 是 `2dppx`）；`X` 讀回 `x`。
- `type()` 在解析度前後都可以，讀回時放在解析度後面；字串原樣保留（`IMAGE/PNG` 不轉小寫）。
- 無效：負的解析度、空的 `image-set()`、結尾的逗號、`image-set()` 裡再放 `image-set()`。`0x` 有效。
- 選項可以是漸層；`image-set()` 可以和其他圖一起放在 `background-image` 的清單裡。

### 選哪一張、畫多大

三張 PNG：`a.png` 40×20 紅、`b.png` 80×40 藍、`c.png` 120×60 綠。每個元素 `background-repeat: no-repeat`，`--force-device-scale-factor` 1、2、3 各截圖一次，看畫了哪一張、CSS px 的大小，和 Resource Timing 裡下載了哪些。

| 值 | 1x | 2x |
| --- | --- | --- |
| `image-set(url(a.png) 1x, url(b.png) 2x)` | a，40×20 | b，40×20 |
| `image-set(url(b.png) 2x)` | b，40×20 | b，40×20 |
| `image-set(url(a.png) 1.5x, url(c.png) 3x)` | a，26.7×13.3 | c，40×20 |
| `image-set(url(b.png) 0.5x)` | b，160×80 | b，160×80 |
| `image-set(url(a.png) 1x type("image/x-nope"), url(b.png) 2x)` | b，40×20 | b，40×20 |
| `image-set(url(a.png) 1x type("image/x-nope"))` | 不畫 | 不畫 |
| `image-set(url(a.png) 1x type("IMAGE/PNG"))` | a，40×20 | a，40×20 |
| `image-set(url(a.png) 1x type("image/webp"), url(b.png) 2x)` | a，40×20 | b，40×20 |
| `image-set(url(a.png) 0x)` | 不畫 | 不畫 |
| `image-set(url(a.png) 1x, url(b.png) 1x)` | a，40×20 | a，40×20 |
| `image-set(url(c.png) 3x, url(b.png) 2x)` | b，40×20 | b，40×20 |

規則：跳過 `type()` 不支援的選項（MIME type 不分大小寫）；在剩下的選項裡，挑解析度大於等於 device scale factor 的最小的，沒有的話挑最大的；一樣時挑先寫的。畫的大小是圖的自然大小除以選中的解析度。`0x` 的選項不畫。只下載選中的圖：1x 時只有 `a.png`、`b.png`，`c.png` 在 2x 和 3x 才下載。

### 字型：Chrome 和 kun 的對照

每個 `font-family` 一個 20px 的 `<div>`，內容是 `Hamburgefonstiv 0123456789`。Chrome 的字型是 CDP 的 `CSS.getPlatformFontsForNode`；kun 沒有這個方法，用 `Range` 量文字的寬度，和已知字型的寬度比對（Liberation Sans 267.969、Nimbus Sans 267.047、Noto Sans 284.109、Liberation Serif 244.609、Noto Serif 284.74、Liberation Mono 312.063、Noto Sans Mono 312、Open Sans 281.531、Carlito 245.156）。

| `font-family` | Chrome | 寬度 | kun 的寬度 | 一樣 |
| --- | --- | ---: | ---: | --- |
| `"Helvetica Neue"` | Liberation Serif | 244.609 | 284.74 | 否 |
| `Helvetica` | Liberation Sans | 267.969 | 284.74 | 否 |
| `Arial` | Liberation Sans | 267.969 | 284.74 | 否 |
| `Verdana` | Liberation Serif | 244.609 | 284.74 | 否 |
| `Futura` | Liberation Serif | 244.609 | 284.74 | 否 |
| `Futura-CondensedMedium` | Liberation Serif | 244.609 | 284.74 | 否 |
| `"Times New Roman"` | Liberation Serif | 244.609 | 284.74 | 否 |
| `Times` | Liberation Serif | 244.609 | 284.74 | 否 |
| `Courier` | Liberation Mono | 312.063 | 284.74 | 否 |
| `"Courier New"` | Liberation Mono | 312.063 | 284.74 | 否 |
| `Georgia` | Liberation Serif | 244.609 | 284.74 | 否 |
| `Tahoma` | Liberation Serif | 244.609 | 284.74 | 否 |
| `"Segoe UI"` | Liberation Serif | 244.609 | 284.74 | 否 |
| `Roboto` | Liberation Serif | 244.609 | 284.74 | 否 |
| `Menlo` | Liberation Serif | 244.609 | 284.74 | 否 |
| `Monaco` | Liberation Serif | 244.609 | 284.74 | 否 |
| `Consolas` | Liberation Serif | 244.609 | 284.74 | 否 |
| `system-ui` | Noto Sans | 284.109 | 270.12 | 否 |
| `"Liberation Sans"` | Liberation Sans | 267.969 | 267.969 | 是 |
| `"Nimbus Sans"` | Nimbus Sans | 267.047 | 267.04 | 是 |
| `Arimo` | Liberation Sans | 267.969 | 284.74 | 否 |
| `sans-serif` | Liberation Sans | 267.969 | 284.1 | 否 |
| `serif` | Liberation Serif | 244.609 | 284.74 | 否 |
| `monospace` | Noto Sans Mono | 312 | 312 | 是 |
| `"No Such Font", serif` | Liberation Serif | 244.609 | 284.74 | 否 |
| `"Helvetica Neue", Helvetica, Verdana, sans-serif` | Liberation Sans | 267.969 | 284.1 | 否 |
| `Calibri` | Carlito | 245.156 | 284.74 | 否 |
| `Cambria` | Caladea | 263.313 | 284.74 | 否 |
| `"Open Sans"` | Open Sans | 281.531 | 281.523 | 是 |
| `"DejaVu Sans"` | Liberation Serif | 244.609 | 284.74 | 否 |
| `"Noto Sans"` | Noto Sans | 284.109 | 284.1 | 是 |
| `Palatino` | Liberation Serif | 244.609 | 284.74 | 否 |
| `"Book Antiqua"` | Liberation Serif | 244.609 | 284.74 | 否 |
| `"Century Schoolbook"` | Liberation Serif | 244.609 | 284.74 | 否 |
| `"ITC Avant Garde Gothic"` | Liberation Serif | 244.609 | 284.74 | 否 |
| `"Zapf Chancery"` | Liberation Serif | 244.609 | 284.74 | 否 |
| `Symbol` | Liberation Serif | 244.609 | 284.74 | 否 |
| `"MS Gothic"` | Liberation Serif | 244.609 | 284.74 | 否 |
| `SimSun` | Liberation Serif | 244.609 | 284.74 | 否 |
| `"Comic Sans MS"` | Liberation Serif | 244.609 | 284.74 | 否 |
| `Impact` | Liberation Serif | 244.609 | 284.74 | 否 |
| `"Trebuchet MS"` | Liberation Serif | 244.609 | 284.74 | 否 |
| `"Lucida Grande"` | Liberation Serif | 244.609 | 284.74 | 否 |
| `-apple-system` | Liberation Serif | 244.609 | 284.74 | 否 |
| `BlinkMacSystemFont` | Liberation Serif | 244.609 | 284.74 | 否 |

另外，Chrome 的 `cursive`、`fantasy`、`math`、`"Comic Sans MS"`、`Impact`、`Tinos`、`"MS PGothic"` 都是 Liberation Serif（這台沒有對應的字型，`Tinos` 也沒有，但它和 Liberation Serif 在同一個等價類），`Cousine` 是 Liberation Mono，`Arial, monospace` 和 `Helvetica, monospace` 是 Liberation Sans。

整理：

- **kun 一樣的**：真的裝了的字型（Liberation Sans、Nimbus Sans、Open Sans、Noto Sans）和 `monospace`。
- **kun 不同的**：
  - 沒有指定通用字型、名稱又都找不到時：Chrome 是 Liberation Serif（預設的 Times New Roman 的等價字型），kun 是 Noto Serif（`serif` 的 fontconfig 預設）。
  - `sans-serif`：Chrome 是 Liberation Sans（預設的 Arial），kun 是 Noto Sans。
  - Arial、Helvetica、Arimo、Times、Times New Roman、Courier、Courier New、Calibri、Cambria：Chrome 是尺寸相容的替代，kun 找不到、換下一個。
  - fontconfig 會替代、但 Chrome 不接受的（Helvetica → Nimbus Sans、Palatino → P052、Century Schoolbook → C059、Verdana → Noto Sans）：Chrome 都不用，kun 也沒用（兩邊一樣不接受，但最後落到的預設字型不同）。
  - `system-ui`：Chrome 是 Noto Sans，kun 是寬度 270.12 的字型（不在這份範圍）。

## kun 現在怎麼找字型

- `layout` 的 `text_style()`（`crates/layout/src/text.rs`）把 `font-family` 轉成 parley 的清單，最後一律接 `serif`；`canvas_text.rs` 一樣。測試模式（`TextContext::new(true)`）只登記 Ahem 和 Lato，通用字型都是 Lato。
- parley 的 `ResolveContext::resolve_font_family` 對每個名稱問 fontique 的 `Collection::family_by_name`：照名稱（不分大小寫）在字型檔自己的 family 名稱裡找，找不到就跳過。
- fontique 的 Linux backend 連結 libfontconfig：名稱表只來自字型檔自己的 `FC_FAMILY`，fontconfig 設定裡的別名（`<alias>`）不在表裡；通用字型在啟動時照 fontconfig 的設定（`FcConfigSubstitute`、`FcFontSort`）解出來。macOS 和 Windows 的 backend 的通用字型是寫死的表（macOS：Times、Helvetica、Courier；Windows：Times New Roman、Arial、Consolas），和 Chrome 的預設一樣。
- fontique 快取每個名稱的結果，包括「找不到」；parley 快取解好的字型清單。

## Chromium 怎麼做

### 字型

- **Blink 的替代**（`third_party/blink/renderer/platform/fonts/alternate_font_family.h` 的 `AlternateFamilyName`）：Courier ↔ Courier New、Times ↔ Times New Roman、Arial ↔ Helvetica；Windows 上 Courier New 不換回 Courier（Courier 是 bitmap 字型），另有 `AdjustFamilyNameToAvoidUnsupportedFonts` 把 bitmap 字型換成 TrueType 的。名稱找不到時 `FontCache` 再用替代的名稱找一次。
- **通用字型**：`GetFallbackFontFamily` 和 Chrome 的字型設定（`webkit.webprefs.fonts.*`）。Linux 的出廠值：standard 和 serif 是 Times New Roman、sans-serif 是 Arial、fixed 是 Monospace（和上面的探測一致）；cursive、fantasy 的出廠值還沒查（這台上 Chrome 都落到 Liberation Serif）。
- **Skia 的 fontconfig**（`src/ports/SkFontConfigInterface_direct.cpp` 的 `matchFamilyName`）：先 `FcFontMatch`，不行再 `FcFontSort` 的第一個；結果只在 `isAcceptableMatch` 時接受：名稱是 `sans`、`serif`、`monospace`（`IsFallbackFontAllowed`），或結果的 family 和設定之後的名稱、要求的名稱相同，或兩者在同一個等價類（`IsMetricCompatibleReplacement`）。
- **等價類**（`kFontEquivMap`）：SANS：Arial、Arimo、Liberation Sans；SERIF：Times New Roman、Tinos、Liberation Serif；MONO：Courier New、Cousine、Liberation Mono；SYMBOL；CAMBRIA：Cambria、Caladea；CALIBRI：Calibri、Carlito；日文的 PGOTHIC、GOTHIC、PMINCHO、MINCHO（MS 的字型和 IPA、VL 等）；簡體中文的 SIMSUN、NSIMSUN、SIMHEI；繁體中文的 PMINGLIU、MINGLIU、PMINGLIUHK、MINGLIUHK。kun 只收 fontconfig 的 `30-metric-aliases.conf` 有對應的 SANS、SERIF、MONO、SYMBOL、CAMBRIA、CALIBRI（`README.md` 的「詳細設計」第 3 節）。

### `image-set()`

- `CSSImageSetValue`（`third_party/blink/renderer/core/css/css_image_set_value.cc`）：解析、序列化、照 device scale factor 選最好的選項。
- `ElementStyleResources`：算完 style 之後才選圖、開始下載；matched properties cache 不看 device scale factor。
- `StyleImageSet`：保留整個 set 給 `getComputedStyle`，畫的是選中的圖，自然大小照它的解析度縮放。
