# RFC 0076：`image-set()` 和字型名稱的對應照 Chrome

- 開始日期：2026-10-23
- 狀態：✅ 已完成：2026-10-23（驗收和做過的檢查在 `progress.md`）
- 細節：`details.md`（Chrome 152 的探測結果、kun 現在怎麼找字型、Chromium 的程式）

## 摘要

兩件讓 Speedometer 的結果頁和 Chrome 畫得不一樣的事，一起補上：

1. **`image-set()`**：照 Chrome 152 解析、計算、序列化（連同 `-webkit-image-set()`、只寫字串的寫法、`type()`），照 device scale factor 選一張圖，圖的自然大小除以它的解析度。
2. **字型名稱的對應**：照 Blink 和 Skia，Helvetica 和 Arial、Times 和 Times New Roman、Courier 和 Courier New 互為替代；Linux 上沒有的字型接受尺寸相容的替代（Arial → Liberation Sans、Calibri → Carlito……）；Linux 上 `serif`、`sans-serif` 等通用字型照 Chrome 的預設設定（Times New Roman、Arial……）。

## 動機

**現況**：2026-10-22 第一次在 kun 裡用官方的方式跑完整的 Speedometer 3.1（同一個 process、20 個 suite、10 個 iteration），能跑完、分數有效，但結果頁和 Chrome 的截圖不同：

- 分數上面的儀表板只剩指針，沒有刻度和數字：它是 `background-image: image-set(url("gauge@2x.png") 2x, url("gauge.png") 1x)`，kun 沒有 `image-set()`，整條宣告無效。
- 字不一樣、框的高度也不一樣：頁面要 `"Helvetica Neue", Helvetica, Verdana, sans-serif`。Chrome 在 Linux 上畫成 Liberation Sans，kun 畫成 Noto Sans。

再查下去，字型的差異不只 Helvetica（`details.md` 的「字型：Chrome 和 kun 的對照」，這台 Fedora 44 上 45 個名稱只有 5 個一樣）：

- kun 對有名字的字型只找「真的叫這個名字」的字型檔：Arial、Helvetica、Times New Roman、Courier New、Calibri 這些在 Linux 上大多沒有的字型都找不到，直接換下一個名稱。Chrome 接受 fontconfig 給的尺寸相容的替代（Liberation Sans、Liberation Serif、Liberation Mono、Carlito、Caladea）。
- kun 的通用字型在 Linux 上照 fontconfig 的預設（這台是 `sans-serif` → Noto Sans、`serif` → Noto Serif），Chrome 照自己的設定（`sans-serif` → Arial、`serif` → Times New Roman，再經過上一條變成 Liberation Sans、Liberation Serif）。kun 在字型清單最後一律接 `serif`，所以**沒有指定通用字型、或指定的字型都找不到的文字，在 Linux 上 kun 是 Noto Serif、Chrome 是 Liberation Serif**：寬度差一成多（`details.md`）。
- macOS 和 Windows 上，fontique 的通用字型表本來就和 Chrome 的預設一樣（macOS：Times、Helvetica、Courier；Windows：Times New Roman、Arial、Consolas），差的只有 Blink 的 Helvetica ↔ Arial 那幾對替代。

**目標**：

- `image-set()` 在 `background-image` 照 Chrome 畫、`getComputedStyle` 和 CSSOM 讀回和 Chrome 一樣的字串、`CSS.supports` 一樣的答案；只下載選中的那一張。
- 同一個 `font-family`，kun 在 Linux 上選到的字型和 Chrome 相同（`details.md` 的對照表全部一樣）；macOS、Windows 上加上 Blink 的替代。
- Speedometer 的結果頁和 Chrome 的截圖一樣（字型、儀表板）。

**要解決的 `docs/limitations.md` 的限制**：

- 「背景」那一條的「沒有 … `image-set()`」。
- 游標那一條的「沒有 `image-set()`」（有它的宣告整條無效）。
- 字型的對應原本沒有記在 `docs/limitations.md`：剩下和 Chrome 不同的地方（`system-ui`、`emoji` 和 `math`、使用者的字型設定、系統的 fontconfig 設定和預設不同時）寫進「Style」的字型那幾條。

**不在範圍內**：

- `image()`、`cross-fade()`、`conic-gradient()`、`light-dark()` 等其他沒有的 `<image>`。
- `list-style-image`、`content: url()`、`border-image` 的圖片（kun 還沒有畫它們）：`image-set()` 在這些地方和 `url()` 一樣只是解析或不支援。
- 使用者改預設字型的設定（Chrome 的「自訂字型」）：照 Chrome 的出廠值寫死。
- `system-ui`：這台 Chrome 是 Noto Sans、kun 是另一個字型（寬度 270.12，還沒查是哪個），範圍不同（Chrome 從 GTK 的設定拿），另外處理。
- web font（`@font-face`）：kun 還沒有。

## 設計概要

**`image-set()`**：

```css
.gauge { background-image: image-set(url("gauge@2x.png") 2x, url("gauge.png") 1x); }
```

- 1x 的螢幕選 `gauge.png`，2x 的選 `gauge@2x.png`，畫出來的大小都是 `gauge.png` 的大小（2x 的圖自然大小除以 2）。只下載選中的那一張。
- 規則照 Chrome 152：從解析度大於等於 device scale factor 的選項裡挑最小的；都比它小時挑最大的；一樣大時挑先寫的；`type()` 不支援的選項跳過，全部跳過時什麼都不畫。
- `getComputedStyle` 讀回整個 set，網址變成絕對網址、解析度換成 `dppx`：`image-set(url("https://…/gauge@2x.png") 2dppx, url("https://…/gauge.png") 1dppx)`；`element.style` 讀回寫的單位（`2x`），`-webkit-image-set()` 讀回 `image-set()`。
- 視窗移到解析度不同的螢幕時，重新選、下載新的那張。

**字型**：

```css
body { font-family: "Helvetica Neue", Helvetica, Verdana, sans-serif; }
```

- Linux 上：沒有 Helvetica Neue；Helvetica 也沒有，照 Blink 試它的替代 Arial，Arial 也沒有，接受尺寸相容的 Liberation Sans。和 Chrome 一樣。
- 沒有指定通用字型的文字，Linux 上的最後一個字型是 Times New Roman（找不到時是 Liberation Serif），和 Chrome 一樣。

## 詳細設計

### 1. `image-set()` 的解析和計算

- `css` 的 `parse_image`（`crates/css/src/background.rs`）多一種 `SpecifiedImage::Set`：選項是 `<image>`（`url()`、漸層，不能是另一個 `image-set()`）或字串（當成 `url()`），加上可以省略的解析度（`x`、`dppx`、`dpi`、`dpcm`、`calc()`，省略是 `1x`，負數無效、但 `calc()` 算出負數照寫的讀回、計算成 `0dppx`，`0x` 有效）和 `type(<string>)`，兩者的順序不拘。名稱不分大小寫，`-webkit-image-set` 是同一個。
- 序列化照 Chrome：specified value 保留單位、`type()` 放在解析度後面、字串寫成 `url("…")`；computed value 的解析度換成 `dppx`（`1dpcm` 是 `0.0264583dppx`，照 Chrome 的位數）。
- `computed_style` 的 `BackgroundImage` 多一種 `Set`：每個選項是計算好的圖（絕對網址或漸層）、解析度（dppx）、type。computed value 不依 device scale factor：選圖在 renderer（下一節）。
- 用到 `parse_image` 的地方都跟著接受：`background-image`、`background`、`border-image-source`（還是只解析）。`cursor` 的圖片清單也接受 `image-set()`（照舊不載入，用後面的關鍵字），不再讓整條宣告無效。
- `CSS.supports` 和 `@supports` 跟著 parser，不用另外改。
- 動畫：`background-image` 之間本來就是離散的，`Set` 一樣。

### 2. 選圖

- `computed_style::BackgroundImage::shown(device_scale_factor)`：照 Blink 的 `CSSImageSetValue::GetBestOption` 選，回傳選中的圖和它的解析度（不是 set 的圖是自己、1dppx）；`shown_url` 是它的網址。
- 三個地方用它，都用同一個 scale：`style` 收集背景圖的網址（`ComputedStyle::background_image_urls(scale)`，style engine 的 device pixel ratio）、renderer 的 `elements_with_background`（圖到時找顯示它的元素，`FrameView` 的 scale）、`paint` 的 `layer_paint`（`dsf`）。
- 自然大小：`layer_paint` 用圖的像素大小建 `Natural` 的地方，除以選中的解析度。漸層的選項沒有自然大小，和直接寫漸層一樣。
- 第一次收集網址之前，`FrameView::start_loads` 先拿視窗的 device scale factor（每個 frame 一樣），所以 2x 的螢幕一開始就只下載 2x 的圖。
- device scale factor 改變時（`FrameView::take_device_scale_factor`）：下一次 `start_loads` 重新收集有 `Set` 的元素選中的網址、下載還沒有的（`UpdateVisualProperties` 本來就接著跑 `start_loads`）。paint 不用另外標記：scale 改變時 `PaintController` 丟掉整份快取、全部重畫；新的圖下載完時照舊經過 `elements_with_background`（用同一個 scale 找顯示它的元素）讓它們重畫。

### 3. 字型名稱的對應

照 Chromium 的分層，各放一層：

- **Blink 的替代**（`AlternateFamilyName`，所有平台）：`font-family` 的每個名稱找不到時，試它的替代：Helvetica ↔ Arial、Times ↔ Times New Roman、Courier ↔ Courier New（Windows 上 Courier New 不換回 Courier）。
- **Skia 的尺寸相容**（`SkFontConfigInterface_direct.cpp`，只有 Linux）：名稱和它的替代都找不到時，接受同一個等價類裡的字型：Arial、Arimo、Liberation Sans；Times New Roman、Tinos、Liberation Serif；Courier New、Cousine、Liberation Mono；Symbol、Symbol Neu；Calibri、Carlito；Cambria、Caladea。Skia 的表還有日文、中文的 MS 字型那幾類（`details.md`），kun 不收：Skia 只看 fontconfig 給的第一個結果，fontconfig 自己的 `30-metric-aliases.conf` 不把 MS Gothic、SimSun 這些對到 Noto CJK 等字型，所以一般的系統上 Chrome 也拿不到它們（2026-10-23 實作中改的：本來照 Skia 整張表，驗收的探測裡 `SimSun` 選到 Noto Serif CJK SC、Chrome 是 Liberation Serif）。
- **通用字型**（只有 Linux 和 macOS、Windows 不同）：照 Chrome 在 Linux 的出廠值（`locale_settings_linux.grd`）：`serif` 和最後接的 standard 字型是 Times New Roman、`sans-serif` 是 Arial、`cursive` 是 Comic Sans MS、`fantasy` 是 Impact，都再經過上面兩層；找不到時接著試下一個名稱，最後是 standard 字型（這台上 Chrome 的 `cursive`、`fantasy` 都畫成 Liberation Serif），再找不到才用 fontique 的通用字型。`monospace` 照 Chrome 是 fontconfig 的 `monospace`、`system-ui` 照舊（現在的做法）。等價類裡的順序照 fontconfig 的設定（`30-metric-aliases.conf`：Arial 先接受 Arimo、再 Liberation Sans），一個名稱屬於哪個等價類照 Skia：表裡第一個符合的那一筆。

放在哪一層：「已決定的事項」第 3 條。展開在 `layout` 的 `font_families`（`font_stack`，`Platform` 決定平台的規則），頁面的文字（`text.rs` 的 `text_style`）和 canvas 的文字（`canvas_text.rs`）都用它；每個名稱只出現一次。

### 測試

（每一項的指令在 `progress.md` 的「驗收」。）

- `style`：`image-set()` 的解析、CSSOM 讀回、computed value、無效的寫法、`url()` 的解析，照 Chrome 152 的探測（`details.md`）；`computed_style`：選圖照 Chrome 的 11 個例子在 1x、2x、3x 的結果。
- `shell` 的整合測試：1x 和 2x 下畫出來的圖和大小、只下載選中的那張；scale 改變之後載入新的圖、回到原本的 scale 時已經有的圖馬上顯示。情境 `image-set`（2x 的 `about:demo`）。
- WPT 的 `css/css-images/image-set/` 加進清單；`cursor-valid.html` 的 `image-set()` 變成通過。
- 字型：展開的規則是 `layout` 的 `font_families` 的單元測試（三個平台）；真的選到的字型照「已決定的事項」第 5 條，在驗收時用 `details.md` 的探測方法和 Chrome 比（`docs/test-gaps.md` 的「只做過手動確認」）。
- 驗收：重跑官方的 Speedometer，結果頁的截圖和 Chrome 比。

### 分階段

1. `image-set()`：解析、計算、序列化、選圖、畫、scale 改變。
2. 字型名稱的對應。

兩段互不依賴。

## 缺點

- `image-set()` 的選圖不在 computed value 裡，style 以外的兩個地方（收集網址、paint）都要記得用同一個函式；漏了會下載一張、畫另一張（或畫不出來）。
- 字型的替代表是寫死的，Chromium 改了它要跟著改；系統的 fontconfig 設定和預設不同時，kun 不跟著變（`docs/limitations.md`）。Linux 上沒有指定字型、或指定的都找不到的文字換了字型（Noto Serif 變成 Liberation Serif），kun 自己的測試和情境用測試字型，不受影響。

## 理由和其他做法

見「已決定的事項」第 2 到 4 條。Chrome 的行為和 Chromium 的程式在 `details.md`。

## 先例

- **Chromium**：
  - `image-set()`：`CSSImageSetValue`（解析和序列化）、`CSSImageSetValue::GetBestOption`（照 device scale factor 選）、`ElementStyleResources` 在算完 style 之後才選圖、下載（不放進 matched properties cache 的 key），`StyleImageSet` 保留整個 set 給 `getComputedStyle`，`StyleImage::ImageScaleFactor` 讓自然大小除以解析度。
  - 字型：Blink 的 `alternate_font_family.h`（`AlternateFamilyName`、`GetFallbackFontFamily`）；Skia 的 `SkFontConfigInterface_direct.cpp`（`matchFamilyName` 只接受同名、通用字型、或 `IsMetricCompatibleReplacement` 的 fontconfig 結果）；通用字型的預設是 Chrome 的設定（`locale_settings_linux.grd` 等）。

## 已決定的事項

2026-10-23 確認（使用者：「都照建議」）：

1. **範圍和分段**：`image-set()` 和字型名稱的對應放在同一份 RFC，分兩段（「分階段」），互不依賴，`image-set()` 先做。
2. **`image-set()` 在 renderer 選圖**：computed value 保留整個 set（絕對網址或漸層、解析度換成 dppx、type），renderer 收集要下載的網址和 paint 時，照 `FrameView` 的 device scale factor 用同一個函式選。不選的做法：計算 style 時就選，device scale factor 要放進 `ComputeContext` 和 matched properties cache 的 key，scale 改變時要重算所有用到它的元素的 style。理由：Blink 也是算完 style 才選（`ElementStyleResources`），style 的快取不依 scale。
3. **字型的規則全部在 `layout`**：`text_style()`（和 `canvas_text.rs`）把 `font-family` 交給 parley 之前，展開成要試的名稱清單：Blink 的替代（所有平台）、Skia 的尺寸相容等價類（只在 Linux，寫死的表）、Linux 的通用字型照 Chrome 的預設；不改 fork。不選的做法：Skia 那一層改 fontique 的 fontconfig backend，照 Skia 問 fontconfig、只接受同名或等價類裡的結果。理由：一般的系統上兩者結果相同（fontconfig 的設定本來就把 Arial 對到 Liberation Sans、Arimo）；不改 fork 的 C FFI 和 fontique 的快取（它會快取「找不到」）；規則是純函式，所有平台同一份程式。代價：等價類不跟著系統的 fontconfig 設定。
4. **`type()` 只認 kun 解得開的 MIME type**：`image/png`、`image/jpeg`、`image/gif`、`image/svg+xml`（不分大小寫），其他的選項跳過。不選的做法：照 Chrome 的清單（多 `image/webp`、`image/avif`、`image/bmp`……），kun 解不開時畫不出來。理由：寧可選一張畫得出來的圖；代價是在 kun 沒有 decoder 的格式上，選到的圖和 Chrome 不同。
5. **字型的對應用純函式測，真的選到的字型在驗收時探測**（2026-10-23，第 2 段開始前；使用者：「照建議」）：`font-family` 展開成要試的名稱清單是純函式，照表測（Blink 的替代、Linux 的等價類和順序、Linux 的通用字型和找不到時的 standard 字型、Windows 的 Courier New）；fontique 真的找到哪個字型，用 `details.md` 的那組名稱和 Chrome 比一次，記進 `progress.md`，`docs/test-gaps.md` 的「只做過手動確認」記下它。不選的做法：再加一個用系統字型、平常 `#[ignore]` 的測試（會被跳過的測試讓人以為有守住，workspace 也沒有這種用法）；讓測試模式多登記假裝成 Liberation 等名稱的字型（測的是假的名稱，不是 fontconfig）。Blink 的 web test 一樣不依賴系統字型。

## 待決定

無。

## 之後的可能

- `system-ui` 照 Chrome（Linux 上從 GTK 的設定拿）。
- 使用者改預設字型的設定。
