# RFC 0076 的進度：`image-set()` 和字型名稱的對應照 Chrome

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-23，Fedora 44。在 `0076` 上做。

完成日期：2026-10-23，Fedora 44（AMD Ryzen AI 9 HX 370）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 `image-set()`：解析、計算、序列化、選圖、畫、scale 改變（第 2、4 條） | 完成 | 2026-10-23 |
| 2 字型名稱的對應（第 3、5 條） | 完成 | 2026-10-23 |

## 驗收

**`image-set()`**

| 項目 | 怎麼驗證 |
| --- | --- |
| 解析和 CSSOM 讀回照 Chrome 152：`-webkit-image-set()` 和大小寫讀回 `image-set()`、字串讀回 `url("…")`、省略的解析度是 `1x`、單位照寫的（math function 讀回 `calc(<n>dppx)`，負的也是）、`type()` 在解析度後面；無效的寫法（空的、結尾的逗號、負的解析度、兩個解析度或 `type()`、巢狀、`none`、長度、解析度加長度）；`background`、`border-image-source`，`cursor` 只收 `url()` 的選項 | `cargo test -p style --test it -- cssom::image_sets cssom::invalid_values_are_not_set`；WPT 的 `css/css-images/image-set/image-set-parsing.html`、`css/css-ui/parsing/cursor-valid.html`（`python3 scripts/test.py wpt css/css-images/image-set/ css/css-ui/`） |
| computed value 照 Chrome 152：整個 set、絕對網址、解析度換成 dppx（`1dpcm` 是 `0.0264583dppx`，`calc()` 算好、負的是 0）、漸層計算好；`cursor`、`border-image-source` 也是 | `cargo test -p style --test it -- computed::image_sets_compute_to_dppx`；WPT 的 `image-set-computed.sub.html` |
| `url()` 和字串照 stylesheet 的 base URL 解析，每個收 `image-set()` 的 property 都是 | `cargo test -p style --test it -- cascade::an_image_sets_urls_resolve_against_the_base_url` |
| 選圖照 Chrome 152：至少 scale 的最小解析度，沒有時最大的，一樣時先寫的；`type()` 不支援的跳過（不分大小寫，只認 kun 解得開的）；`0dppx` 不畫 | `cargo test -p computed_style --lib -- tests::an_image_set_shows_the_image_chrome_picks`（11 個 Chrome 的例子 × 1x、2x、3x）；WPT 的 `image-set/*-rendering*.html` |
| 畫選中的圖，自然大小除以解析度；只下載選中的那張（2x 的螢幕一開始就只下載 2x 的） | `cargo test -p shell --test it -- network::an_image_set_shows_and_loads_the_image_for_the_scale` |
| scale 改變時載入新選中的圖、圖到了重畫；回到原本的 scale 時已經有的圖馬上顯示；每張圖只下載一次 | `cargo test -p shell --test it -- browser_view::a_new_scale_factor_shows_an_image_sets_image_for_it` |
| `about:demo` 的方塊在 2x 的螢幕上畫 2x 的綠色圖，寬 16 CSS px | 情境 `image-set`（`python3 scripts/test.py scenarios image-set`，看截圖 `image-set-2x.png`） |

**字型名稱的對應**

| 項目 | 怎麼驗證 |
| --- | --- |
| `font-family` 展開成要試的名稱，照 Chrome 152：Blink 的替代名稱（所有平台，Windows 上 Courier New 不換回 Courier）、Linux 上 Skia 的尺寸相容字型（fontconfig 有對應的那幾類，照它的順序，不分大小寫）、Linux 的通用字型照 Chrome 的出廠設定、清單最後是 standard 字型，每個名稱一次 | `cargo test -p layout --lib -- font_families` |
| 頁面的文字在 Linux 上選到的字型和 Chrome 一樣（57 個 `font-family`，除了不在範圍內的 `system-ui`、`emoji`） | 驗收時探測（`details.md` 的「字型：Chrome 和 kun 的對照」的方法）；`docs/test-gaps.md` 的「只做過手動確認」 |
| Speedometer 的結果頁的字和儀表板和 Chrome 一樣 | 驗收時跑一次官方的 Speedometer，截圖對照 |

## 做過的檢查

2026-10-23，Fedora 44（AMD Ryzen AI 9 HX 370），兩段都在這台上做。

- **做了什麼**（設計和理由見 `README.md` 的「已決定的事項」第 1 到 5 條）：
  - `image-set()`：`css` 的 `SpecifiedImage::Set`（`image-set()` 和 `-webkit-image-set()`，選項是 `url()`、字串或漸層，解析度和 `type()` 順序不拘），解析、specified 和 computed 的序列化；calc 多一種解析度（`Kind::Resolution`，`x`、`dppx`、`dpi`、`dpcm`，媒體查詢共用換算）；`url()` 的解析、比較經過 `SpecifiedImage` 的 `urls_mut`、`has_unresolved_urls`、`as_written`；`cursor` 的圖片可以是 `url()` 或只有 `url()` 的 `image-set()`。`computed_style` 的 `BackgroundImage::Set`、`ImageSetOption`、`BackgroundImage::shown` 和 `shown_url`，`background_image_urls` 帶 scale，`CursorImage` 的圖是 `BackgroundImage`。`style` 收集網址照 style engine 的 device pixel ratio；renderer 的 `start_loads` 先拿視窗的 scale，scale 變了時重新收集；`elements_with_background` 照 frame 的 scale；`paint` 的 `layer_paint` 畫選中的圖、自然大小除以解析度。`about:demo` 多一個 `image-set()` 的方塊，情境 `image-set`（2x）。
  - 字型：`layout` 的 `font_families`（`font_stack`），`text.rs` 的 `text_style` 和 `canvas_text.rs` 的 `properties` 都用它。
  - WPT：`css/css-images/image-set/` 加進清單（39 個，34 個整份通過；失敗的是 `content`、`list-style-image` 的圖片和 kun 沒有的 `sign()`、`sibling-index()`、`conic-gradient()`、一個色停點的漸層，`.ini` 有註解）；`cursor-valid.html` 的兩個 `image-set()` 變成通過。
- **Chrome 152 的探測**（`details.md`）：`image-set()` 的解析和序列化 29 個、`cursor` 和其他 property 等 27 個、選圖 11 個 × 1x、2x、3x；字型 57 個 `font-family`。探測時發現 kun 的舊測試以為 Chrome 不收 `cursor` 的 `image-set()`，其實收（只收 `url()` 的選項）：測試改成 Chrome 不收的漸層。開始第 2 段前另外查了 Chrome 在 Linux 的出廠字型（`locale_settings_linux.grd`）和 fontconfig 的 `30-metric-aliases.conf` 的順序。
- **設計上改的**：
  - scale 改變時原本打算另外標記 `image-set()` 的元素重畫；mutation test 發現 `PaintController` 在 scale 改變時本來就丟掉整份快取，拿掉。
  - Skia 的等價類原本整張照抄；探測裡 `SimSun` 選到 Noto Serif CJK SC、Chrome 是 Liberation Serif（Skia 只看 fontconfig 給的第一個結果，這台的 fontconfig 把 SimSun 對到 Noto Serif），改成只收 fontconfig 自己的設定有對應的那幾類。
- **驗收的探測**：字型第一組 45 個名稱 44 個和 Chrome 一樣（`system-ui` 不在範圍內；改之前只有 5 個一樣），第二組 12 個 11 個一樣（`emoji`：kun 沒有這個通用字型）。官方的 Speedometer 跑 1 個 iteration（kun 12.13、Chrome 12.54），結果頁的儀表板的刻度和數字、分數的字型都和 Chrome 一樣；剩下的差別是 headless 的兩邊 viewport 高度不同，框在 viewport 裡垂直置中。
- **mutation test**：
  - `mutants-1.toml`（`image-set()`，22 個）：第一次 23 個抓到 20 個。「scale 改變時不另外標記重畫」是等價的（上面），拿掉那段程式和 mutant；「圖到時用 1x 找顯示它的元素」沒走到（`b.png` 在 2x 的第一個 frame 之前就到了），`b.png` 改成延遲 300 ms 回應之後抓到；「解析度可以加長度」在 `image-set()` 裡結果還是無效，但 `width: calc(1px + 1x)` 會被接受，補了這一條後抓到。其他：選圖的每個條件、自然大小的除法、style 和第一次 `start_loads` 用的 scale、scale 改變時重新收集、負的 math function、`dpi` 的換算、`-webkit-` 的別名、巢狀、字串的選項、兩個解析度或 `type()`、`cursor` 的漸層、`type()` 的序列化、`image-set()` 裡的 `url()` 的解析和「還沒解析」的判斷，都有測試失敗。
  - `mutants-2.toml`（字型，10 個）：替代名稱、尺寸相容只在 Linux、等價類不分大小寫、Windows 的 Courier New、Linux 的通用字型、最後的 standard 字型、每個名稱一次、`sans-serif` 的名稱、只收同一類、類裡的順序，全部抓到。
  - 兩次跑完都確認檔案還原（`target/mutate/originals/` 不存在）。
- **跑過的**：每段的 crate（`css`、`style`、`paint`、`computed_style`、`renderer`、`layout`、`shell`）全部通過；`python3 scripts/test.py wpt css/css-images/ css/css-backgrounds/ css/css-ui/ css/css-values/`；情境全部。兩段結束時各跑一次 `cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`，和 `cargo test --workspace --no-fail-fast`（最後一次 3596 個通過、3 個 ignored）。
- **不穩定的**：
  - 第一次 `cargo test --workspace` 時 `browser` 的 `navigation::links::idle_callbacks` 失敗（第一個 `chain` 的 `timeRemaining()` 不在 (0, 50]），單獨跑五次都過，再跑整個 workspace 也過；和 `docs/test-gaps.md` 記的那一條同一個症狀，這個測試的 tab 是 1x，和這份的改動無關。
  - 第 2 段第一次跑全部的情境時 69 個裡 1 個失敗，輸出被接著的重跑蓋掉，沒記下是哪一個；之後連跑 5 次都通過。情境用測試字型（`windowless.rs` 的 `test_fonts: true`），和字型的改動無關；原因沒查到。
- **順帶查的、和這份無關的**：WPT 的 `css/css-images/object-fit-none-png-00{1,2}c.html`、`object-fit-scale-down-png-00{1,2}c.html` 在 Linux 上失敗。二分到把它們加進清單的 commit（RFC 0064 2a，當時在 macOS 上做）也一樣失敗：在 Linux 上從來沒有通過過。原因是 canvas 之後畫的東西落到透明的「之後」layer，虛線邊框的抗鋸齒像素差 1 個色階（每頁 552 個像素）；`.ini` 用 `fuzzy` 容許（`main` 上的另一個 commit），根本的做法寫成 RFC 0077 的草稿。二分時暫時 `git stash` 了 `../third_party/rusty_v8` 裡一個沒 commit 的改動（使用者同意），查完放回去。
