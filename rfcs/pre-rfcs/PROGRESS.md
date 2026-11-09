# 進度

各階段的範圍見 `MVP.md`。

目前進度：**P4 Chromium 式 compositor 完成**（2026-10-04，在 macOS 上驗收）：renderer 有自己的 compositor thread，Viz 的 `BeginFrame` 和 Browser 的 input 先到那裡，滾輪和按鍵由它直接捲 viewport 和成為 layer 的捲動容器，JS 卡住時照樣能捲；paint 依 property tree 把捲動容器切成獨立的 layer，捲動不重新 raster；邊緣對齊 device pixel；DOM 改了之後，style、box tree、layout、paint、raster 都只重做受影響的部分。**MVP 到此完成**：原本排的 P5（flex 和 grid）和 P6（multi-process）改成 RFC 0003、0004，還沒排進任何階段（`MVP.md` 的「里程碑」）。**P3 基本 JS 完成**（2026-10-02，在 macOS 上驗收）：頁面的 `<script>`（inline、外部、`defer`、`async`）會執行，JS 經過與 engine 無關的 DOM 層（`dom_api`）和它的宣告表使用 DOM；timer 和 `requestAnimationFrame`，frame 由 Viz 的 `BeginFrame` 驅動；DOM 的事件分派、`onclick` / `onload`，使用者的點擊是 `click` 事件，連結是 `<a>` 的 activation behavior；V8 改成從 rusty_v8 的 fork 自己編。**P2 像個瀏覽器完成**（2026-10-01，在 macOS 上驗收；Windows 上的驗收延後）。經 HTTP / HTTPS 載入頁面、stylesheet 和圖片（PNG、JPEG、GIF），排出 block 和 inline（包括 inline 元素自己的 box），可以捲動、點連結、上一頁 / 下一頁，視窗上方有 Browser 自己畫的工具列和網址列；自己的 reftest 和匯入的 WPT reftest 全部通過。各階段的範圍和設計見 `P1.md`、`P2.md`、`P3.md`、`P4.md`。

## Thread 配置

2026-10-04 搬到 `docs/architecture.md` 的「Process 和 thread」。

## Crate

2026-10-04 搬到 `docs/architecture.md` 的「Crate」。

## 測試守不住的地方

2026-10-04 搬到 `docs/test-gaps.md` 的「測試守不住的地方」。

## P0 骨架

寫死的 display list 經 compositor tiling → skia-safe raster → `CompositorFrame` → Viz（wgpu）顯示在視窗上；Renderer main thread 上的 V8 isolate 已掛上 CppHeap。

### 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 寫死的 display list 經 skia-safe raster 顯示在視窗上 | `cargo run` |
| 同一條 pipeline 的像素輸出 | `cargo run -- --headless out.png` |
| V8 可以執行 script、錯誤回報、microtask 由我們觸發 | `cargo test -p bindings` |
| CppHeap 已掛上 isolate，V8 一次 GC 就能回收沒人引用的 wrapper | `cargo test -p bindings`（`cpp_heap_collects_unreachable_wrappers`） |
| Compositor tiling、raster、Viz 合成 | `cargo test -p compositor -p raster -p viz` |

2026-09-29 在 Windows 11 上全部通過（Skia raster，Viz 走 wgpu Vulkan backend，AMD Radeon RX 7900 XTX）。2026-09-30 在 macOS 26.6（Apple M4）上也全部通過，Viz 走 wgpu 的 Metal backend；`cargo run` 的視窗能開啟、沒有錯誤；畫面到 2026-10-01 才目視確認過（見 P2 的驗收）。

### 已知限制（刻意簡化）

2026-10-04 和其他階段的合併，搬到 `docs/limitations.md`，只留目前仍然成立的。

## P1 靜態渲染

範圍、子階段和設計見 `P1.md`。P1.0 GC 基礎之後，依序是 P1.1 骨幹切片、P1.2 Cascade、P1.3 文字、P1.4 Property trees 與收尾，2026-09-30 全部完成。新增的 crate：`dom_macros`（P1.0）、`html`、`computed_style`、`style`、`layout`、`paint`（P1.1 起）；`bindings` 原名 `script`。

### 驗收

2026-09-30 在 Windows 11 上全部通過。最後用 `rust-toolchain.toml` 釘住的 Rust 1.88.0 重跑過一次：build、fmt、clippy（沒有警告）、所有測試、`ci.py unsafe` 和 `matrix` 6/6。

同一天在 macOS 26.6（Apple M4，ARM64）上，用同一個 toolchain 重跑同一組檢查，也全部通過：build、fmt、clippy（沒有警告）、`cargo test --workspace`（reftest 同樣是 56 個通過、2 個預期中的失敗）、`ci.py unsafe` 和 `matrix` 6/6；`about:demo` 在系統字型和 `--test-fonts` 下的 headless 輸出都正確。過程中找到並修正了一個只在 macOS 出現的字型 bug，見「做過的檢查」。

**GC 基礎（`dom`、`bindings`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 樹操作、id、root 計數、template content、teardown | `cargo test -p dom --test tree` |
| 另一個 `DomIsolate` 的 handle 一律被拒絕，拒絕前沒有改動任何東西 | `cargo test -p dom --test cross_isolate` |
| 每次配置後強制 GC，隨機修改並對照 Rust 端模型；只靠 parent edge 保住 detached subtree | `cargo test -p dom --test gc_stress` |
| 不用 forced GC，incremental marking 的每一輪都有完成並 sweep | `cargo test -p dom --test incremental_stress` |
| 一步一步控制 marking（`StandaloneTestingHeap`）：marking 完成後才接上新節點、每一步之間修改、concurrent marking 時 main thread 持續修改 | `cargo test -p dom --test detached_marking` |
| `ComputedStyle` 存在 element 上：讀寫、替換；節點被回收時 finalizer 放掉最後一個 `Arc` | `cargo test -p dom --test computed_style` |
| 自己的 `MemberSlot` / `WeakMemberSlot`（2026-10-02 起在 rusty_v8 fork 裡）：放在任何位置都對齊，寫入、trace、weak 清除、barrier 都正確 | `cargo test -p bindings --test cppgc_member` |
| Stack 掃描的前提 | `cargo test -p dom --lib` |
| 16 種違規寫法都編譯失敗，且失敗原因正確（包括在被 trace 的區域放 `RefCell`） | `cargo test -p dom --test compile_fail` |
| Concurrent marking 模式；每種 marking 模式 × thread 數；隨機組合種子、thread 數、marking 模式和 GC flag | `MACHOPO_CPPGC_MARKING=concurrent cargo test -p dom`、`python scripts/ci.py matrix`、`python scripts/ci.py fuzz` |
| `unsafe` 只在 `dom` 和 `bindings` | `python scripts/ci.py unsafe` |

**Parser 和 navigation（`html`、`browser`、`renderer`、`shell`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| WPT 的 tree-construction 測試：3267 個通過，197 個已知失敗，206 個 fragment 測試跳過；同一組測試在每次配置節點後都強制 GC，結果不變 | `cargo test -p html --test tree_construction` |
| 基本的 parse、quirks mode、明確的 `</option>` 會填入 `<selectedcontent>` | `cargo test -p html --test parse` |
| Browser 讀取 `file://`、`about:demo`、`about:blank`，Renderer parse 之後回報 `<title>`；讀不到的檔案和不支援的 scheme 會 commit 錯誤頁 | `cargo test -p browser --test navigation` |
| `file://` URL 和路徑互轉（含空白和非 ASCII，P2.0 搬到 `net`）；非 HTML 的內容以純文字顯示 | `cargo test -p net --lib`、`cargo test -p renderer --lib` |
| `machopo` 的參數：URL 或檔案路徑、各選項的解析和錯誤訊息、`--help` 列出每個選項和環境變數 | `cargo test -p shell --bin machopo` |

197 個 tree-construction 的已知失敗都列在 `tests/wpt/tree-construction-expectations.txt`（當時在 `html/syntax/parsing/` 底下），依原因分組，沒有一個是我們的 sink 造成的：175 個是 processing instruction（HTML 規範新加入、html5ever 還沒支援）；16 個是 html5ever 和規範不同的行為（`<selectedcontent>` 遇到隱含關閉的 `<option>`、`<template>` 之後的 `<frameset>`、template 裡 table 中的 `<form>`）；6 個是 `scripted_*.dat`，需要在 parse 時執行 script（tree-construction 的測試不經過 engine）。

**Style、layout、paint**

| 項目 | 怎麼驗證 |
| --- | --- |
| Cascade（specificity、順序、`!important`、`style` attribute、UA stylesheet 含 UA 的 `!important`、root 的 blockify、CSS-wide keyword、quirks mode 的 class 比對）；繼承和 `currentcolor` 晚解析；各 property 的值（border、`font` shorthand、`font-size` / `font-weight` 的相對值、`line-height`、`opacity`、`overflow`、`transform`）；pseudo-class 和 `:is()` / `:where()` / `:has()`；UA stylesheet 的 box 和文字規則；忽略不支援的東西 | `cargo test -p style` |
| Block layout（auto 寬高、padding、`%`、`min-*` / `max-*`、`box-sizing`、border、margin collapsing、`overflow: hidden` 的 BFC、root 的 margin、不產生 box 的元素、文件大小） | `cargo test -p layout --test block` |
| 空白處理（跨元素合併、行首和結尾、強制換行、`pre` / `pre-wrap` / `pre-line`） | `cargo test -p layout --lib` |
| Inline layout（行框、換行時行尾空白 hang、對齊、half-leading、`<br>`、anonymous block、Lato 有比例寬度） | `cargo test -p layout --test inline` |
| Paint（canvas background、邊緣對齊、border 的形狀和分組、同色 border 是一個 ring、文字畫在所有 background 之後、裝飾線、property tree 的節點、一個 effect 跨多個 chunk 只開一個 layer、clip 和 transform 的 op 順序） | `cargo test -p paint` |
| Raster（`DrawPath` 沒有接縫、Ahem 的 glyph 是銳利的方塊、`SetMatrix` 相對於 tile、`SaveLayerAlpha` 只混色一次）；`gfx::Transform` 的組合順序 | `cargo test -p raster -p gfx` |
| `about:demo` 由真正的 pipeline 畫出來，文字、半透明、裁切和位移都正確；系統字型和 `--test-fonts` 都可以 | `cargo run`、`cargo run -- --headless out.png [--test-fonts]` |

**Reftest（`shell`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| WPT 語意：metadata、fuzzy、多個 reference 的判定、expectation 檔、不支援的寫法會報錯 | `cargo test -p shell --lib` |
| 自己的 32 個 reftest（B1–B6、C1–C6、T1–T8、E1–E5，加上 harness、canvas background、`scale()` / `matrix()`、Lato 冒煙測試）全部通過 | `cargo test -p shell` 加上 `--test reftest`（`4ff8d6e` 時的指令） |
| 匯入的 26 個 WPT reftest：24 個通過，2 個需要 inline box 的 border / padding / background（P2.1 改成通過，見 P2 的驗收） | `cargo test -p shell` 加上 `--test reftest -- wpt`（`4ff8d6e` 時的指令） |

### 做過的檢查

測試會不會其實沒測到：每個 GC 相關的測試和每一批 reftest，都用 mutation test 確認過（暫時弄壞實作，應該有測試失敗）。

- **GC**：分別弄壞 trace、write barrier、weak trace、derive 對 enum variant 的 trace、跨 isolate 的檢查、`MemberSlot` 的 `Sync`，都有測試失敗或編譯失敗（細節見 `crates/dom/soundness.md` 的「怎麼驗證的」）。其中兩次發現原本的測試其實沒測到：對每個節點都持有 handle，以及只建立一般 element。
- **抓不到的 mutation**：拿掉 `MemberSlot::assign` 的 release fence，在 ARM64（Apple M4）上用 concurrent marking、8 個 thread、隨機種子反覆跑 `gc_stress`、`detached_marking`、`incremental_stress` 3 分鐘，共 1242 次，全部通過。錯誤要硬體剛好重排兩個寫入才會出現，stress 碰不上，所以記憶體順序改由審查守住（`soundness.md` 的「需要人工審查的地方」）。2026-10-02 起 fork 在建構子裡寫入新物件的資料，兩個 fence 都拿掉了。
- **`ComputedStyle` 的 soundness 守門**：在 `ComputedStyle` 加一個 `Rc` 欄位，`dom` 會在 `Send + Sync + 'static` 的斷言處編譯失敗；讓 `set_computed_style` 多留一個 `Arc`，`computed_style` 測試會失敗。
- **Parser**：暫時讓 sink 不更新 `<selectedcontent>`，`parse` 的測試會失敗；GC stress 確實有作用（有 stress 時跑完約 350 ms，沒有時約 34 ms）。
- **Cascade**：分別弄壞 `!important` 的層級、`style` attribute 的優先、specificity、root 的 blockify、UA stylesheet、繼承、`inherit` 的來源、`prev_sibling_element` / `parent_element`、「讓 `border-color: inherit` 繼承解析後的顏色」，每次都有測試失敗。
- **Layout 和 paint**：分別讓 layout 忽略 margin、padding、`min-*` / `max-*`、border 寬度，`%` 不除以 100，替 `display: none` 產生 box，關掉 canvas background、border、「style 是 `none` 時寬度為 0」，每次都有對應的 reftest 失敗。
- **文字**：分別讓空白永遠不合併、`pre` 不保留換行、`nowrap` 會換行、忽略 `line-height` / `text-align`、不產生 anonymous block、不畫文字，每次都有對應的 reftest 失敗。
- **Effect**：分別拿掉 clip、transform 的位置、transform-origin、opacity、stacking context 的延後繪製、「文字畫在所有 background 之後」、「一個 effect 只開一個 layer」，每次都有對應的 reftest 失敗。最後一個原本抓不到（E3 的兩個 child 在同一個 chunk），所以 E3 加了跨 chunk 的第二部分。
- **Harness**：讓每個 box 都走 leaf layout 時，test 和 reference 都是空白，B2 照樣通過，抓到它的是 `harness/mismatch.html`，這正是放這個測試的原因。Runner 在非預期失敗、非預期通過、列了不存在的測試時 exit code 都不是 0。
- **C1 的 `head` 那一半**在 P1.2 時抓不到拿掉 `head { display: none }` 的 mutation：`head` 會變成 inline，而那時 inline 還不排版。P1.3 有了 inline layout 之後就抓得到了。
- **Safe code 能觸發 UB 的漏洞**：重新審查安全論證時找到兩個，都已修正並補上測試：修改操作接受另一個 `DomIsolate` 的 handle；測試用的 cppgc 函式（當時的 `bindings::cppgc_testing`）是 safe 的，卻接受接了 isolate 的 heap。
- **V8 的行為**：A1–A3 和 concurrent marking 相關的假設，都對照 `v8` crate 附的 V8 原始碼確認過。GC 確實有跑：incremental stress 約 190 輪 incremental Mark-Compact 都由 platform task 完成；concurrent 測試確實在背景 thread 上執行了 `trace()`（`detached_marking` 約 4 萬次）。Fuzz 同一串 80 組隨機組合（`--seed 0x5eed0001`）全部通過。
- **ARM64**：2026-09-30 在 macOS（Apple M4）上，補上 `MemberSlot::trace` 的 acquire fence 之後，`cargo test --workspace`、`python scripts/ci.py matrix`（6 種設定）和同一串 80 組 fuzz 全部通過；`detached_marking` 在背景 thread 上 trace 了 42081 次。Fence 的成本：release build、concurrent、8 個 thread，200k 個節點做 30 次 full GC，加 acquire fence 前 231–264 ms、加之後 220–274 ms，在誤差範圍內。
- **WPT 找到的兩個 bug**，都已修正，對應的測試留在匯入的子集裡當回歸測試：clip rect 沒有對齊像素（`margin-collapse-009`）；同色 border 的四個梯形在斜角頂點漏出一個像素的背景（`height-113`）。拿掉任一個修正，對應的測試都會失敗。
- **macOS 找到的字型 bug**，已修正：系統字型模式下，`about:demo` 的粗體標題沒有畫出來。粗體對到 `Helvetica.ttc` 這個字型集裡的第 2 個字型（index 1），而 Skia 在 macOS 上的 CoreText font manager 只載入字型集的第 1 個（`SkTypeface_mac_ct.cpp` 遇到 index ≠ 0 直接回傳 null）；FreeType（Linux）和 DirectWrite（Windows）都沒有這個限制。修正：`raster` 載入失敗時，把那個字型從字型集抽成獨立的字型檔再交給 Skia。Reftest 沒抓到，因為測試字型模式只用單一字型的 Ahem 和 Lato。`raster` 的 `draws_a_face_after_the_first_of_a_collection` 把兩者打包成字型集來測，拿掉修正後在 macOS 上會失敗。
- **XHTML**：一開始 148 個 WPT 候選只過了 45 個，大多是因為 reference 把 CSS 包在 CDATA 裡；拿掉 CDATA 標記之後過了 142 個，修好上面兩個 bug 之後 144 個。

### 已知限制

2026-10-04 和其他階段的合併，搬到 `docs/limitations.md`，只留目前仍然成立的。

## P2 像個瀏覽器

範圍、子階段和設計見 `P2.md`。P2.0 網路與子資源（2026-09-30）、P2.1 Inline box、P2.2 圖片、P2.3 捲動、P2.4a 連結導航、P2.4b 工具列和網址列（2026-10-01），之後擴充匯入的 WPT 子集。新增的 crate：`net`（P2.0）；parley 改用 vendor 的 `third_party/parley`（2026-10-01 改成 fork 的 `machopo` 分支，放在 machopo 旁邊的 `../third_party/parley`）（P2.1）。

### 驗收

2026-10-01 在 macOS 26.6（Apple M4）上用 Rust 1.88.0 全部通過：build、fmt、clippy（沒有警告）、`cargo test --workspace` 228 個、reftest 101 個（自己的 49 個、WPT 52 個，沒有預期中的失敗）、`ci.py unsafe` 和 `matrix` 6/6。

**Windows 上還沒跑過，延後到之後**：第一次 build 時要確認新的 dependency（aws-lc-sys 照原始碼只需要 C 編譯器；arboard）不用另外裝工具；也還要試只有 Windows 才有的快捷鍵（Alt+← / Alt+→、Ctrl+R、Ctrl+L、Alt+D）和網址列的中文輸入法。

**視窗模式的手動檢查**：2026-10-01 在 macOS 26.6（Apple M4，Retina）上目視確認：
- `about:demo`（系統字型的粗體標題）；text.npr.org、SQLite 和 Gutenberg 快照的載入、inline box 和圖片；
- 滾輪、觸控板和鍵盤的捲動、巢狀的捲動容器、縮放視窗時的捲動位置；
- CERN 的連結、上一頁 / 下一頁 / 重新整理的快捷鍵、連結上的游標、Gutenberg 目錄的 `#` 連結、text.npr.org 點進文章再回來；
- 工具列的外觀和按鈕的底色、網址列的 focus 和選取、輸入網址和本機路徑導航、Esc 還原、中文輸入法（組字的底線、候選字視窗的位置）、和其他 App 之間的複製和貼上、上一頁 / 下一頁 / 重新整理按鈕、網址列裡的 Cmd+← / Cmd+→、三種游標。

**網路和載入**

| 項目 | 怎麼驗證 |
| --- | --- |
| `net`：HTTP 的狀態碼、header、`Content-Type` 和 `charset`；沒有 `Content-Type` 時的判斷；錯誤狀態碼照樣有 body；redirect（保留 fragment、上限 20 次、不准跳到 `file:`、沒有 `Location` 的 3xx）；連線被拒、DNS 失敗；`file:`、不合法的 URL 和不支援的 scheme。全部對本機的 test server，不連外網 | `cargo test -p net` |
| Navigation 是非同步的：HTTP 經過 redirect、只在 header 宣告的編碼（Shift_JIS）；`file:`、`about:`；錯誤頁（找不到檔案、連線被拒、不支援的 scheme）；後發的 navigation 贏，先發的晚到也不會蓋掉它 | `cargo test -p browser --test navigation` |
| 編碼：BOM、HTTP `charset`、`<meta>` 預掃（註解、其他標籤的 attribute、1024 bytes 的範圍）、沒有宣告時的預設；stylesheet 的 BOM、`charset`、`@charset` 和 document 的編碼；`<base href>` | `cargo test -p renderer` |
| `<link rel=stylesheet>` 套用，相對網址照 document 解析（L1）；和 `<style>` 照 tree order 決定勝負，載入失敗的、`alternate`、`type` 不對的、`disabled` 的、不是 stylesheet 的 `<link>` 都不套用（L2） | `cargo test -p shell` 加上 `--test reftest -- link`（`4ff8d6e` 時的指令） |
| 只在 HTTP 上測得到的：錯誤狀態碼和不是 `text/css` 的 stylesheet 不套用，quirks mode 例外；stylesheet 載入完之前一個 frame 都不畫；`http:` 的頁面不能載入本機檔案；`DidFinishLoad` 等圖片（延遲回應的圖片也在截圖裡）；狀態碼 404 的圖片算失敗，即使 body 是合法的 PNG | `cargo test -p shell --test network` |
| 快照：CERN 的 5 頁都能載入、標題正確、沒有失敗的子資源；SQLite 的 `sqlite.css` 有套用，只有 SVG 的 logo 載入失敗；Gutenberg 的 42 個 `<img>`（37 個 PNG 和 JPEG 檔）全部解碼成功 | `cargo test -p shell --test sites` |
| 真實網站：`https://text.npr.org/` 經 HTTPS（HTTP/2）載入，外部 stylesheet 有套用 | `cargo run -- --headless out.png https://text.npr.org/`（手動，要連網） |

**Inline box 和圖片**

| 項目 | 怎麼驗證 |
| --- | --- |
| parley 換成 vendor 的 main（`../third_party/parley`）：換過去、還沒加 patch 時，所有測試和 reftest 的結果都不變；vendor 的內容等於上游那個 commit 加上 `patches/` 的 patch（2026-10-01 改成 fork：`machopo` 分支等於上游那個 commit 加上兩個修改的 commit，所有測試和 reftest 的結果也不變） | `cargo test --workspace`；`../third_party/parley/README.machopo.md` 的更新步驟 |
| parley 的 patch：開頭 / 結尾的邊界跟著前後的內容換行、放得下時不多出斷行點、連續的開頭、空的元素、開頭後面緊接著 atomic inline、min / max-content 寬度；邊界不算進行高 | `cargo test -p layout --test parley_inline_box` |
| Tag 在空白合併裡的位置；被 block 拆開的 inline 元素；百分比的 padding；文字記得自己的元素；圖片的尺寸（長寬比、百分比的寬度、百分比的高度當成 `auto`、`box-sizing: border-box`、`max-width`、沒有圖片時只看 style） | `cargo test -p layout` |
| `<img>` 的 `width` / `height` 是 presentational hint：px 和 %、不合法的值忽略、任何 author 規則和 `style` attribute 都蓋得過它、只對 `<img>` | `cargo test -p style` |
| Raster 把圖片縮放畫進它的位置、讀出 PNG / JPEG / GIF 的尺寸、快取在圖片不再被用時清掉、無法解碼的不畫 | `cargo test -p raster` |
| Inline 元素的 background（I1）；左右的 margin / border / padding 佔寬度（I2）；跨行的切片、邊界跟著它的字換行（I3）；上下的 padding 不改變行高、蓋到上一行（I4）；巢狀的 background 和文字的順序（I5）；行尾拿掉的空白沒有 background 也沒有底線（I6） | `cargo test -p shell` 加上 `--test reftest -- inline`（`4ff8d6e` 時的指令） |
| 自然尺寸（M1）；只給一邊時照長寬比、attribute 是 hint、inline 和 block 兩條路（M2）；inline 圖片的底邊對齊 baseline、行框長高（M3）；block 圖片的 margin（M4）；載入失敗的圖片只佔 attribute 的空間、沒有 attribute 時是 0×0（M5）；JPEG 和 GIF（M6） | `cargo test -p shell` 加上 `--test reftest -- images`（`4ff8d6e` 時的指令） |

**捲動**

| 項目 | 怎麼驗證 |
| --- | --- |
| Viewport 捲動不 commit：quad 跟著位移、捲回去重用原本的 tile、viewport 外一個 viewport 的範圍先 raster 好、更遠的 tile 釋放、背景色沒變時不 commit | `cargo test -p compositor` |
| `overflow` 的 `auto` / `scroll` / `overlay`，另一軸不是 `visible` 時 `visible` 變成 `auto`；捲動容器的 scrollable overflow（padding box 加上內容和結尾的 padding） | `cargo test -p style`；`cargo test -p layout --test block` |
| 捲動容器有 scroll translation 和 scroll node，`overflow: hidden` 也有 | `cargo test -p paint` |
| 滾輪用 hit testing 找游標底下的容器，捲不完的交給外層容器、再交給 viewport，被外層 clip 掉的容器不會被捲；`overflow: hidden` 不讓使用者捲，但 `#fragment` 可以；`overflow` 從 root 或 `<body>` 傳給 viewport，傳出去的元素不是捲動容器、不 clip；方向鍵、PageUp / PageDown、Space、Home / End；layout 改變後把位移夾回合法範圍；`#fragment` 的目標（`id`、`<a name>`、percent-decode、`#` 和 `#top`） | `cargo test -p renderer` |
| `overflow: auto` 沒捲動時和 `hidden` 畫得一樣（S1）；`#target` 捲 viewport（S2）；`#target` 在捲動容器裡時，容器和 viewport 都捲（S3） | `cargo test -p shell` 加上 `--test reftest -- scroll`（`4ff8d6e` 時的指令） |
| 從 Browser 送 input 到畫面：滾輪捲容器、以行為單位的滾輪捲 viewport、End 捲到底 | `cargo test -p shell --test network` |

**連結和歷史紀錄**

| 項目 | 怎麼驗證 |
| --- | --- |
| `Transform` 的反矩陣 | `cargo test -p gfx` |
| Hit testing：text、inline box、巢狀的 inline 元素、block、anonymous block 算外面的元素、只有 canvas 的地方；transform；被 clip 掉的內容點不到；捲動位移；點到的捲動容器；stacking context、行和 background 的上下順序 | `cargo test -p paint --test hit_test` |
| 連結是最近的、有 `href` 的 `<a>` | `cargo test -p renderer` |
| `NavigationController`：新的 entry 丟掉前面的、上一頁 / 下一頁、連按兩次上一頁、redirect 改 entry 的網址、重新整理、同一份文件內的 entry、要去的 entry 被丟掉的歷史 navigation | `cargo test -p browser --lib` |
| 點連結、上一頁、下一頁、重新整理；按下和放開要是主按鍵、在同一個連結上（P3.4 起是點擊送到兩者的共同祖先，落在連結裡才導航）；同一份文件內的 `#fragment` 不重新載入；網頁開不了本機檔案、`mailto:`，`javascript:` 不做事；本機檔案可以開另一個本機檔案；游標在連結上是手指、換文件時重設 | `cargo test -p browser --test navigation` |
| 從 Browser 送點擊到畫面：點連結裡的文字，等新的一頁（包括延遲回應的圖片）載入完；`#target` 捲到目標；`#top` 捲回頂端 | `cargo test -p shell --test network` |
| CERN 快照：點「hypermedia」進到 `WhatIs.html`；Gutenberg 快照：點目錄的「13」，畫面和直接載入 `#Page_13` 相同 | `cargo test -p shell --test sites` |

**視窗和工具列**

| 項目 | 怎麼驗證 |
| --- | --- |
| 合成：嵌入的 surface 移到它的 quad、clip 到裡面、墊上它的背景；兩個 client 相同的 `ResourceId` 分開；合成後的 id 在 resource 還被用時不變、不用就忘掉；還沒有 frame 時是預設的背景；嵌到自己時停下來；只有被嵌入的 surface 送來新 frame 也會重畫 | `cargo test -p viz` |
| `SurfaceLayer`：在 root layer 前面、乘上 device scale factor；沒變時不 commit | `cargo test -p compositor` |
| 網址輸入框：focus 時全選、打字取代、Enter、Esc 還原、編輯中不被新網址蓋掉；macOS 和其他平台的游標移動和刪除；複製、剪下、貼上（換行變空白）；IME 組字和送出；第一次點擊全選、之後放游標、拖曳選取；文字太長時捲到游標。網址的補全：有 scheme 的照用、`localhost` 和 IP 是 `http`、其他主機是 `https`、存在的路徑是 `file:`。工具列：按鈕和輸入框的位置、按鈕要按下和放開在同一個上面而且能用、點輸入框會 focus。快捷鍵在各平台的對應，輸入框有 focus 時 Cmd+← / Cmd+→ 不是上一頁 | `cargo test -p shell --lib` |
| 視窗（`BrowserView` 加上真的分頁，經 Viz 合成）：1 倍和 2 倍時工具列在上、網頁從它下面開始到視窗底部；網頁單獨送的 frame 照樣畫出來，Browser 不用送；網頁的 viewport 是視窗扣掉工具列；點網頁的位置扣掉工具列；在網址列打 `IP:port/path` 導航；Esc 和點網頁都還原網址；複製和貼上；上一頁、下一頁按鈕；游標 | `cargo test -p shell --test browser_view` |
| `cargo run` 的視窗能開啟、載入頁面，log 沒有錯誤 | `cargo run`（手動） |

**Reftest 和 WPT**

| 項目 | 怎麼驗證 |
| --- | --- |
| 自己的 49 個 reftest：P1 的 32 個，加上 L1–L2、I1–I6、M1–M6、S1–S3（`<meta name="machopo-fragment">` 讓 runner 載入 `test.html#target`）；全部通過 | `cargo test -p shell` 加上 `--test reftest`（`4ff8d6e` 時的指令） |
| 匯入的 52 個 WPT reftest 全部通過：P1 的 26 個（它留下的兩個失敗 `margin-bottom-applies-to-008`、`blocks-018` 在 P2.1 改成通過），加上 `css/CSS2/linebox`、`css/CSS2/visudet`、`css/css-overflow` 和 P1 的目錄裡用到圖片的 26 個；WPT reftest 的 expectations（2026-10-05 起是 `4ff8d6e:tests/wpt/reftest-expectations.txt`）裡沒有預期中的失敗 | `cargo test -p shell` 加上 `--test reftest -- wpt`（`4ff8d6e` 時的指令） |
| Headless 和 reftest 等 `DidFinishLoad` 之後的 frame 再截圖；送 input 的測試等 renderer 回齊 `InputEventAck` 再等 ack 帶的 frame（P4.1 第 3a 段之前是固定等下一個 frame） | `cargo test -p shell` |

### 做過的檢查

- **Mutation test**：每個子階段都弄壞實作的規則，確認有測試失敗，結果如下。
  - **網路與子資源（P2.0）**：14 條，每一條都抓到了：不套用 `<link>`、不排除 `alternate` / `disabled` → reftest L1 / L2；不擋住畫面、錯誤狀態碼照樣套用、不檢查 MIME、quirks mode 也要求 `text/css`、不檢查讀本機檔案的權限 → `shell` 的 `network`；保留過期的 commit、忽略 HTTP 的 `charset` → `browser`；redirect 不保留 fragment、多跟一次、允許跳到 `file:` → `net`；忽略 `<base href>` → `renderer`。
  - **Inline box（P2.1）**：13 條；parley 的 patch 拆成 8 個部分逐一拿掉，每一個都讓 `parley_inline_box` 失敗。第一輪有 3 條沒被抓到：原本的 I3 只有黑色的 Ahem 文字，多畫的 border 被文字蓋住，也沒有讓邊界和字被拆開的寬度；改寫 I3（透明的文字加 background，再加兩段剛好會被拆開的寬度）之後都抓到了。
  - **圖片（P2.2）**：10 條。第一輪有 2 條沒被抓到：M2 全是 block 的圖片（走 taffy 的 `aspect_ratio`，不經過 layout 自己的尺寸公式），而蓋過 hint 的只有 `style` attribute（它本來就一定贏）；M2 加上 inline 的圖片和一條低 specificity 的 author 規則之後都抓到了。
  - **捲動（P2.3）**：13 條。第一輪有 3 條沒被抓到：滾輪捲完第一個容器就停（原本只有一層容器）、只 raster 看得到的 tile（先 raster 好的 tile 不會出現在 frame 裡）、背景色沒變照樣 commit（像素一樣）；補上巢狀容器的滾輪測試、數 raster 過的 tile、檢查不需要 commit 的測試之後都抓到了。
  - **連結導航（P2.4a）**：27 條。第一輪有 2 條沒被抓到：hit testing 忽略 clip 時，滾輪的測試照樣通過，因為外層容器下面接著的 block 的 background 本來就蓋在突出去的內層容器上；改成外層容器下面留一段 margin。任何按鍵都算點擊也沒抓到：測試只用中鍵按下又放開，而中鍵按下時不記錄連結；加上「主按鍵按下、中鍵放開」。寫清單時另外發現 6 條沒有測試，先補上了：重疊的 stacking context、巢狀的 inline 元素、`javascript:`、`#top`、換文件時重設游標、headless 等新一頁的載入完成。
  - **工具列和網址列（P2.4b）**：26 條，第一輪就全部抓到。寫測試時先發現一條原本測不到：網頁的 viewport 沒扣掉工具列時，多出來的部分被 clip 掉，看不出來；改成捲到底之後檢查最後 10px 剛好在視窗底部。
  - **WPT 找到的 bug（擴充 WPT 子集）**：3 個修正各自弄壞時，都有新匯入的 WPT 測試和單元測試失敗；「傳出 `overflow` 的元素照樣 clip」只有 `renderer` 的 `scroll` 測試抓到。
- **Render-blocking 原本測不到**：headless 本來就等 `DidFinishLoad`，所以拿掉 render-blocking，reftest 照樣通過。為此讓 headless 回報畫了幾個 frame，`network` 的測試用延遲回應的 stylesheet 檢查只畫了一個。
- **讀本機檔案的權限原本沒有測試**，寫 P2.0 的 mutation test 清單時才發現，補上了 `network` 裡的測試。
- **擴充 WPT 找到的 3 個 bug**，都已修正，對應的測試留在匯入的子集裡：inline 元素的邊界撐大行高（`line-height-125`，修在 parley 的第二個修改，「Don't let inline box edges size the line」）；root 的 `overflow` 不是 `visible` 時 `<body>` 沒有 clip（`overflow-body-propagation-012`）；`overflow: overlay` 沒有解析（`overflow-overlay`）。
- **Headless 的 race**：送 input 的兩個 `#fragment` 測試，在全部測試一起跑時偶爾失敗：frame 和 `DidCommitSameDocumentNavigation` 走不同的 channel，frame 先到時網址還沒更新。加上 `InputEventAck` 之後，在額外負載下連跑兩次全部測試都沒有再出現。
- **TLS 的 dependency**：只有一份 rustls（0.23），加密後端是 aws-lc-rs，沒有 ring 和 OpenSSL。

### 已知限制

2026-10-04 和其他階段的合併，搬到 `docs/limitations.md`，只留目前仍然成立的。

## P3 基本 JS

範圍、子階段和設計見 `P3.md`。P3.0 GC 前置（2026-10-01）、P3.1 Bindings 基礎、P3.2 Script 執行、P3.3 Event loop 和 timer、P3.4 事件（2026-10-02）。新增的 crate：`dom_api`、`dom_bindings`（P3.1）；V8 改成依賴 rusty_v8 的 fork（`../third_party/rusty_v8` 的 `machopo` 分支），從原始碼編（P3.1，`P3.md` 的「已決定的事項」第 10 條）。

### 驗收

2026-10-02 在 macOS 26.6（Apple M4）上用 Rust 1.88.0 全部通過：build、fmt、clippy（沒有警告）、`cargo test --workspace` 362 個、reftest 163 個（自己的 64 個，其中 `harness/chain-broken*.html` 2 個是預期中的失敗；WPT 99 個：71 個測試，加上鏈上 28 個本身也是測試的 reference）、`ci.py deps`、`ci.py unsafe` 和 `matrix` 6/6；P3.1 完成時另外跑了 `ci.py fuzz` 30/30（`--seed 0x5eed0003`）。測試和 debug build 連結 debug 的 V8（有 DCHECK）；2026-10-02 起平常改成連結 release 的，見「做過的檢查」的「預設用 release 的 V8」。

**視窗模式的手動檢查**（2026-10-02，macOS 26.6，Apple M4）：
- `about:demo`：按鈕計數、被 `preventDefault` 擋住的連結、兩秒後變綠的方塊、`setInterval` 的秒數、一直移動的方塊、`load` 的訊息；
- 視窗模式的 `BeginFrame`（Viz thread 每 16.7 ms 送一次，沒有自動測試）：rAF 的動畫不停地順暢移動、`setInterval` 每秒加一、`setTimeout` 準時、捲動時動畫照樣在跑、連到 fragment 和 `javascript:` 的連結正常，靜態頁面（`tests/sites` 的網站）照常顯示和捲動。

**GC 和 `dom`**

| 項目 | 怎麼驗證 |
| --- | --- |
| `TracedSlot`：讀寫和清除；owner 活著時保住值，清除後、owner 死掉後回收；跨兩個 heap 的循環（wrapper ↔ GC 物件）被回收；teardown 時 slot 還有值也沒問題 | `cargo test -p bindings --test cppgc_traced` |
| Marking 進行中寫入 `TracedSlot`，值都沒有被回收；用 V8 的 GC callback 確認寫入時真的在 marking（10157 次寫入）、cycle 都有完成（回收了 10456 個 holder） | `cargo test -p bindings --test cppgc_traced_marking` |
| Event listener：add 不重複加、保持順序、remove、快照（之後移除的標成已移除，之後新增的不在裡面）、listener 保住 callback、`once` / `passive`、用 handle 移除、event handler 的位置和值（沒編譯的保住元素，detached DOM 也能加）、GC stress、另一個 isolate 的值被拒絕、detached DOM 沒有 JS 值、teardown | `cargo test -p dom --test listeners` |
| Listener 的新增和移除與 incremental marking 交錯，節點只靠樹的 edge、callback 只靠 listener 保住（回收了 80000 個垃圾節點） | `cargo test -p dom --test incremental_stress` |
| `Window` 保住 document、document 保住 window；`Event` 的資料和狀態；`target` / `currentTarget`（節點或 window）保住它們；window 上的 listener；另一個 isolate 的 handle 被拒絕 | `cargo test -p dom --test events` |
| `Timers`：最早到期的先、同時到期的照設定順序、清除、負的 timeout 是 0、`setInterval` 重複直到清除、巢狀超過 5 層至少 4 ms（`setInterval` 每次深一層）；animation frame 的快照、同一個 frame 裡取消、frame 途中要求的等下一個 | `cargo test -p dom --test timers` |
| Template 不能移進自己的 content，再深也不行（host-including inclusive ancestor）；`gc_stress` 和 `incremental_stress` 的模型也照這條檢查 | `cargo test -p dom --test tree` |
| `v8` crate 和 V8 裡 `TracedSlot` 依賴的程式碼沒有改變 | `bindings` 的 `build.rs`（比對失敗就編譯失敗） |

**執行 script 的入口和 wrapper**

| 項目 | 怎麼驗證 |
| --- | --- |
| Script 只從 `DomIsolate` 的入口執行，binding 經 `BindingContext` 改 DOM：script 改 DOM、巢狀的 script、轉換參數時執行的 JS（`toString` 再呼叫 binding）、microtask 和 platform task（FinalizationRegistry）裡的 binding、入口以外的 script 拿不到 DOM、binding 裡的 GC stress | `cargo test -p dom --test script` |
| 持有 DOM scope 時執行 script 會編譯失敗（最外層和 binding 裡各一個），compile-fail 測試共 18 個 | `cargo test -p dom --test compile_fail` |
| Wrapper：同一個物件永遠是同一個 wrapper、JS 加的屬性在 GC 之後還在、wrapper 保住物件、物件和 wrapper 的循環被回收、不是節點 wrapper 的值（一般物件、沒有 wrap 的 API 物件、用 V8 內部的 tag wrap 的物件、原型或 Proxy）被拒絕、`create` 途中建立的 wrapper 優先；global 是 window 的 wrapper、event 的 wrapper，三種 tag 互相分得開 | `cargo test -p dom --test script` |
| 自己編的 V8（開 pointer compression）在 `unwrap` 時檢查 wrap 的 tag：用另一個 tag `unwrap` 得到 `None`；第一個 isolate 建立時也檢查一次，不檢查就 panic；fork 的 `Object::is_wrapping` 不管 tag 都看得到 wrap；錯誤的行號從 script 在檔案裡開始的那一行算 | `cargo test -p bindings --test v8_smoke`；自我檢查在每個建立 isolate 的測試裡執行 |

**`dom_api` 和 `dom_bindings`**

| 項目 | 怎麼驗證 |
| --- | --- |
| `dom_api` 不經過任何 engine：宣告表的描述（interface、父 interface、成員的種類和型別、`[Global]`、`[LegacyUnforgeable]`、建構子）、每種物件最衍生的 interface、呼叫入口的轉換和型別檢查（`this` 不對、參數型別不對、缺少的可選參數）；`parentNode` / `firstChild` / `nextSibling`、`textContent` 的讀寫、`appendChild` / `insertBefore`（fragment 搬移子節點、document 的子節點依插入位置的規則、reference 不是子節點、插在自己前面）/ `removeChild`、attribute（小寫、名稱檢查）、`classList` 的四個方法和 token list 的序列化、`getElementById`、`querySelector`、`createElement`、`createTextNode`、`documentElement`、`body`；字典（描述、預設值、`undefined` 和 `null` 是空字典、父字典的成員、`required`、型別不對）；事件和 window（建構子、`Event` / `MouseEvent` / `Window` 的成員、`preventDefault` 的條件、參數的預設值、每個 interface 只接受自己的物件當 `this`、事件的路徑） | `cargo test -p dom_api` |
| Script 經過宣告表使用 DOM：interface object 和 prototype chain、不能當 constructor、`document` 不能改；建樹、讀樹、attribute、查找；wrapper 的身分；accessor 的屬性和 `length`；照 WebIDL 轉換參數（`ToString`、nullable、落單的 surrogate、型別不對、參數不夠），轉換時丟出例外或呼叫 DOM；`this` 不對；入口以外的 script；`DOMException`（`name`、`code`、常數、prototype、`this` 檢查、換掉 global 也不影響丟出的例外）；`classList`；膠水不受頁面換掉內建函式影響；global 是 window（prototype chain、`[Global]` 的成員在 global 上、`[LegacyUnforgeable]`、window 的 listener）；`Window` 的 timer 和 animation frame 成員（`length`、handle 是正的而且每次不同、字串的 handler 和不是 function 的 callback 丟 `TypeError`、清除不存在的不做事）；事件的建構子（字典的轉換順序和預設值、沒有 `new`、`length`、JS 的子類別、建立的事件在 DOM 裡）和成員（Rust 建立的滑鼠事件、`preventDefault`、`stopPropagation`、`this` 的檢查）；`addEventListener` 的 options 照 WebIDL 的字典轉換（getter 各讀一次、經過 prototype chain、getter 丟出的例外、先轉 callback、function 是物件不是 boolean）；`V8Host::call`（`this`、參數、回報例外） | `cargo test -p dom_js --test bindings` |
| 分派：capture → target → bubble 的順序和 `eventPhase`、不 bubble、`load` 不到 window、`stopPropagation` / `stopImmediatePropagation`、取消和 passive、`once`（`Object.prototype.once` 不影響 boolean 的 options）、途中加入和移除的 listener、例外不中斷、重新進入（`InvalidStateError`、巢狀分派別的事件）、從 Rust 分派；連結的 activation behavior（`MouseEvent` 的 `click` 才算、bubble 時找祖先、取消時不導航、沒有 `href` 的 `<a>`）；`load` 的 target 是 document；`onclick` / `onload` 的 IDL attribute 和 content attribute（位置、`return false`、scope、只編譯一次、語法錯誤、`<body onload>` 是 window 的） | `cargo test -p dom_js --test dispatch` |
| JS 參與的 GC：隨機的 script 經過 binding 建樹、拆樹，每次配置節點都強制 GC；JS 用 wrapper 上的屬性、只放在陣列裡的節點、listener 的 closure 抓住節點，之後都還完整，丟掉的節點都被回收 | `cargo test -p dom_js --test gc_stress` |
| V8 只在 `bindings`、`dom_bindings` 和 `dom` 的 V8 模組：其他 crate 不依賴 V8、不用 `dom` 裡屬於 V8 的項目，`dom` 的其他檔案只用 `v8::cppgc`；renderer 只透過 `ScriptEngine` 使用 engine，只有選 engine 的 `shell` 和 `browser` 的測試依賴 `dom_bindings` | `python3 scripts/ci.py deps` |

**Script 的載入和執行**

| 項目 | 怎麼驗證 |
| --- | --- |
| `Parser` 停在每個 `</script>`，之後的內容還沒 parse；停著時 DOM 可以自由修改；輸入可以分段，連 script 中間也能切開；被輸入結尾截斷的 script 不回傳；結果和一次 parse 完相同；每個 script 開始標籤結束的行號；`onclick` / `onload` 的 content attribute 變成沒編譯的 handler（行號、`<body onload>` 是 window 的） | `cargo test -p html --test parser` |
| 「prepare the script element」：inline、空的、`type=module`、其他 type、空的 `src`、擋 parser / `defer` / `async`、沒有 `src` 時 `defer` 無效、`nomodule` 照樣執行、template 的內容不執行；`defer` 等 parse 完照順序、`async` 載好就執行、載入失敗的不執行 | `cargo test -p renderer --lib script` |
| 只執行 2xx 的回應，MIME 是圖片等的不執行；parser 停下來時前面的 stylesheet 已經開始載入（script 把它移除也一樣） | `cargo test -p shell --test network` |
| `javascript:` 連結在 document 裡執行、不導航；載入失敗的 script 列在 `DidFinishLoad` 的失敗清單；一直不拿掉 `reftest-wait`、又一直跑 rAF 的頁面，600 個 frame 之後是錯誤，不會卡住 | `cargo test -p shell --test script` |
| `BeginFrameSource`：只送給要的 client，回答之前不送下一個，`SetNeedsBeginFrame(false)` 之後不送 | `cargo test -p viz --lib` |

**事件和導航**

| 項目 | 怎麼驗證 |
| --- | --- |
| 點擊是送到指標下元素的 trusted `MouseEvent`（座標）；`load` 在 `DidFinishLoad` 之前 | `cargo test -p shell --test script` |
| `click` 送到按下和放開的共同祖先，連結只在點擊落在它裡面時導航；listener 或 handler 取消點擊時不導航；script 送 `MouseEvent` 的 `click` 會導航（`Event` 不會），從訊息、timer、animation frame 裡都行；`defer` 的 script → `DOMContentLoaded`（document、trusted、bubble）→ `load`（target 是 document）；`load` 等延遲 300 ms 的圖片（頁面自己用 `Date.now()` 量，不算瀏覽器啟動的時間） | `cargo test -p browser --test navigation` |

**Reftest 和 WPT**

| 項目 | 怎麼驗證 |
| --- | --- |
| J1–J3：inline script 改它前面的 DOM、看不到後面的；每個 script 之後做 microtask checkpoint；外部 script 先執行才 parse 後面的、載入失敗照常繼續、`defer` 在 parse 完之後、`async` 載好就執行；沒被接住的例外只停掉那個 script | `cargo test -p shell` 加上 `--test reftest -- script`（`4ff8d6e` 時的指令） |
| J4–J5：timer 的順序、`clearTimeout`、額外參數和 strict 的 `this`、`setInterval` 剛好 3 次、巢狀 timer、每個 timer 之後的 microtask checkpoint；rAF 每個 frame 一次、第 5 個 frame 的位置、虛擬時間的間隔、`cancelAnimationFrame`；都用 `reftest-wait` | `cargo test -p shell` 加上 `--test reftest -- script`（`4ff8d6e` 時的指令） |
| J6–J8：capture → target → bubble 的順序和 `eventPhase`、`stopPropagation`、不 bubble 的事件（listener 依序寫進 DOM 的列）；inline script → `defer` 的 script → `DOMContentLoaded`（document、bubble、trusted、parse 完）→ `load`（target 是 document、不重複到 window）；script 送的 `click`：被取消時 `dispatchEvent` 回傳 false、連結不導航，`Event` 的 `click` 不導航（`reftest-wait`） | `cargo test -p shell` 加上 `--test reftest -- script`（`4ff8d6e` 時的指令） |
| 匯入的 19 個有 script 的 WPT reftest（`css/CSS2/normal-flow` 的 `block-in-inline-insert-*`、`-remove-*`、`-whitespace-001b`：在 `<body onload>` 用 `insertBefore` / `appendChild` / `removeChild` 改樹），加上它們鏈上 28 個本身也是測試的 reference，全部通過；挑選方式在 `tests/wpt/README.md` | `cargo test -p shell` 加上 `--test reftest -- wpt`（`4ff8d6e` 時的指令） |
| Reftest runner 支援 reference 的鏈：reference 還要通過它自己的 reference；互相指向的 reference 做過的比較跳過（和 wptrunner 一樣），會結束；`harness/chain-broken*.html` 預期失敗 | `cargo test -p shell` 加上 `--test reftest -- harness`（`4ff8d6e` 時的指令）、`cargo test -p shell --lib reftest` |

### 做過的檢查

- **Mutation test**：每一批改動都弄壞實作的規則，確認有測試失敗。第一輪沒抓到的，原因和怎麼補都記在下面。
  - **GC 前置（P3.0）**：18 條，17 條抓到（細節見 `crates/dom/soundness.md` 的「怎麼驗證的」）：`TracedSlot` 的 5 條 → `cppgc_traced`；`JsEdge` 不 trace、listener 的 9 條規則、host-including 的兩條 → `listeners`、`incremental_stress`、`tree`、`gc_stress`；`build.rs` 比對的 release store 改成 relaxed → 編譯失敗。抓不到的：`TracedSlot` 的 `Drop` 不 destroy V8 的節點。Finalizer 在 sweeping 中執行，V8 這時本來就不釋放節點，留給下一輪 GC；heap 銷毀時整張表一起釋放，所以測試看不出差別。
  - **執行 script 的入口和 wrapper（P3.1）**：入口 5 條（離開時不拿掉 slot、microtask checkpoint 和 platform task 不當成入口、從不設 slot、binding 裡忽略 GC stress）、wrapper 6 條、`CallbackSlot` 3 條加 `ScriptCallback::assert_isolate` 1 條、tag 登記表 5 條。「binding 裡忽略 GC stress」第一次沒抓到：測試只檢查結果；改成在 script 執行途中留下垃圾節點、檢查它們在 script 結束前就被回收。沒抓到的三條：拿掉 `create` 之前的檢查、`ScriptCallback::assert_isolate` 什麼都不做都是等價的（之後的檢查回傳同一個 wrapper；轉成 `Local` 時 `v8` crate 也會檢查 isolate）；`wrapper()` 不檢查新物件已經 wrap 了東西，只有 binding 傳進來的 `create` 回傳已經 wrap 過的物件才會觸發，而 binding 裡的 panic 會讓 process abort，要用子 process 的測試才測得到，拿掉也不會 UB（原因見 `crates/dom/soundness.md`）。
  - **`dom_api` 和 `dom_bindings`（P3.1）**：`dom_api` 15 條、`dom_bindings` 17 條（包括膠水的 7 條）都抓到。`getElementById` 比對空的 id 第一次沒抓到（測試裡沒有 `id=""` 的元素）；`classList.toggle` 忽略 `force` 第一次沒抓到（測試用的 token 本來就在）。寫測試時另外發現實作錯了一條規則：把 doctype 附加到已經有 element 的 document，規範要求丟 `HierarchyRequestError`，原本的實作和測試都允許。
  - **依賴規則和 JS 參與的 GC（P3.1）**：`ci.py deps` 4 條都擋下，只出現在註解裡的不擋；GC stress 5 條各用 3 個種子都抓到。膠水把 `classList` 存在 `Map`（強引用，洩漏）第一次只抓到 2 個種子：丟掉的節點不一定用過 `classList`，改成每個新元素都用一次。
  - **Script 執行（P3.2）**：16 條，14 條抓到。「parser 停下來時不開始載入」第一次沒抓到，補測試時還發現實作錯了：inline script 是先執行、才開始載入它前面的 stylesheet，所以被 script 移除的 stylesheet 永遠不會被要求，改成 parser 一停就開始載入。沒抓到、而且是等價的：圖片重複要求（同一個網址本來就只要求一次）；「只在 task 結尾開始載入」除了上面那種情況，送出要求的時間只差在同一個 task 裡。
  - **Event loop 和 timer（P3.3）**：18 條，17 條抓到。「timer handler 的 `this` 不是 global」第一次沒抓到：J4 的 handler 不是 strict mode，`this` 傳 `undefined` 也會變成 global，改成 strict。沒抓到的：換 document 時不清 timer。舊 document 的 callback 只改得到已經不顯示的舊 document，新頁面看不出差別（實際的影響是舊頁面的 rAF 迴圈一直跑、浪費 CPU）。2026-10-02 補上了測試（P4 的「做過的檢查」）。
  - **事件（P3.4）**：字典 12 條、`dom` 的 GC 型別 9 條、宣告表 22 條、分派和 event handler 27 條、renderer 15 條、測試和文件 13 條、`insertBefore` 和 reference 的鏈 8 條，最後都抓到了。第一輪沒抓到的：
    - 字典的 3 條當時是等價的（成員不照字典序讀、`undefined` 的成員照型別轉換、膠水給 boolean 建的字典有 prototype）：要有多個成員、預設值和 `undefined` 轉出來不同、或有 `once` 的字典才分得出來，有了 `MouseEventInit` 和 `once` 之後補上測試。
    - `stopImmediatePropagation` 不設 stop propagation、`stopImmediatePropagation` 被忽略：測試先呼叫 `stopPropagation`，或前面留下的 listener 讓事件到不了要測的地方；改成從乾淨的狀態、在 target 上單獨測。
    - 處理連結請求的三個呼叫點（每個訊息之後、timer 之後、animation frame 之後）：拿掉任何一個，剩下的都會在下一個訊息時接手。合成每一輪結尾的一處（加上回 input 的 ack 之前），「從不處理」就抓得到。
    - `load` 不等子資源：測試從 `navigate` 開始量時間，瀏覽器啟動的約 3 秒蓋過了圖片延遲的 300 ms；改成頁面自己用 `Date.now()` 從 script 開始量。
    - 不跳過做過的比較：互相指向的 reference 讓 runner 無限遞迴（stack overflow），也算抓到。`insertBefore` 一律加在最後、`<body onload>` 留在 body 時，匯入的 WPT reftest 也會失敗。
- **找到的 bug**：
  - parse 一結束就判斷載入完成，那時圖片還沒開始要求，`DidFinishLoad` 太早送出（`shell` 的網路測試）。改成只在 `start_loads` 之後判斷。
  - 把連結請求移到 event loop 每一輪的結尾之後，`InputEventAck` 可能比 fragment 的捲動先送出，`shell` 的 `a_link_to_top_scrolls_to_the_top` 偶爾截到捲動之前的畫面。改成回 input 的 ack 之前先處理，之後 `network` 的連結測試連跑 20 次都通過。
- **不穩定的測試**：
  - `ci.py matrix` 找到 `listeners` 偶爾以 `Must use --expose-gc` 失敗：同一個檔案裡 detached DOM 的測試如果最先執行，會用空的 flag 初始化 V8。改成先建立接 isolate 的 DOM 之後，用 concurrent、1 個 thread、8 個 process 平行連跑 1200 次都沒有再出現。
  - `browser_view` 一開始另開 thread 回應 `BeginFrame` 並轉送 Viz 的訊息，frame 因此可能比 host channel 上它後面的 `DidFinishLoad` 晚到，測試把載入時的舊 frame 當成捲動後的新 frame。改成在讀 Viz 訊息的同一個 thread 回應之後，連跑 5 次都通過。
- **fuzz 的「卡住」是休眠**：第一次跑 30 組，只跑完 13 組就到了背景任務 1 小時的上限。用同樣的設定連跑 25 次都沒有卡住，其中 4 次的牆上時間 103～926 秒，卻沒有觸發 300 秒的上限：上限用 monotonic clock，macOS 休眠時它不前進，所以是電腦在測試途中休眠了。用 `caffeinate -i` 重跑 30/30 通過，每組 7～12 秒。新加的 `--gc-interval`、`--random-gc-interval`、`--cppgc-random-gc-interval` 要 `V8_ENABLE_ALLOCATION_TIMEOUT`，我們的 debug V8 有；用 `--trace-gc` 看，`gc_stress` 多觸發了 25～40 次 GC。
- **自己編 V8 的評估**（macOS，2026-10-02，Apple M4）：
  - 查 V8 原始碼時發現當時用的 prebuilt 沒有 pointer compression，`unwrap` 不檢查 wrap 的 tag（soundness.md 的 A9），所以評估自己編。
  - 從 rusty_v8 的 fork 開 `v8_enable_pointer_compression` 編：debug 版第一次 23 分鐘（ninja 2474 步），V8 的 build 目錄 1.4 GB、fork 連同工具 2.4 GB、static library 441 MB；release 版 17 分鐘、build 目錄 994 MB、static library 152 MB，`cargo test --release -p bindings -p dom` 全部通過。用它跑 `cargo test --workspace`、`ci.py matrix` 6/6 都通過，`bindings` 的 `build.rs` 對照 fork 的原始碼也通過；先用 rusty_v8 發布的 ptrcomp prebuilt 跑過一次，結果相同。
  - 重編的坑：`-p bindings` 和 `--workspace` 讓 `v8` 的 build script 的依賴有不同的 feature，每次切換都重跑 build script 並重編整個 workspace，那時用固定 `miniz_oxide` 的 feature 繞過。`bindings` 的 C++ glue（和它的 `cc`）拿掉之後再量，各種組合之間切換都不再重編任何 crate，所以沒有保留那個固定的 feature。
  - Fork 的修改之後，弄壞 fork 裡的 slot 照樣抓得到：`MemberSlot::trace` 什麼都不做 → `detached_marking` crash（SIGBUS）；`TracedSlot::trace` 一律跳過 → `cppgc_traced` 失敗。Debug V8 的 DCHECK 抓到一個測試的錯：`cppgc_member` 的對齊測試在建立任何 heap 之前就建立 slot，而壓縮的 slot 要 cppgc 的 cage 先設好（`CageBaseGlobal::IsSet()`）；改成先建立 heap。
- **V8 的行為**：`TracedSlot` 依賴的 V8 行為（soundness.md 的 A7 和 C4）都對照 `v8` crate 附的 V8 原始碼確認過，`build.rs` 也比對那幾段。
- **升級到 Rust 1.99.0**（2026-10-02，P3 完成之後）：`rust-toolchain.toml` 和 workspace 的 `rust-version` 一起改。build、fmt、clippy（沒有警告）、`cargo test --workspace` 362 個、reftest 163 個、`ci.py deps`、`ci.py unsafe` 和 `matrix` 6/6 重跑都通過。新的 clippy lint 改了兩處測試（`cloned_ref_to_slice_refs`、`chunks_exact_to_as_chunks`）；`dom` 的 compile-fail 預期輸出重新產生，錯誤的種類和位置都沒變，只有 help 的寫法、note 的路徑和行號欄的排版不同。rusty_v8 的 fork 在 1.99 下有兩個上游程式碼的警告（`fetch_update` 改名成 `try_update`、`Cargo.toml` 把 `build.rs` 同時列成測試），在 fork 裡改掉了（清單在 fork 的 `README.machopo.md`）；`try_update` 在 fork 自己釘住的 1.91 還不能用，所以是對 `fetch_update` 加 `#[allow(deprecated)]`，fork 用 1.91 和 1.99 都能編。
- **換成 `crossbeam-channel`**（2026-10-02，P4.1 之前，`P4.md` 的「已決定的事項」第 14 條）：thread 之間的 `std::sync::mpsc` 全部換成 `crossbeam-channel`，只換型別（`NetworkService` 的 tokio channel 和 winit 的 `EventLoopProxy` 不換）。fmt、clippy（沒有警告）、`cargo test --workspace` 362 個、reftest（`harness/chain-broken*.html` 照樣是預期中的失敗）、`ci.py deps`、`ci.py unsafe`、`ci.py docs` 都通過。沒有新的行為，所以沒有 mutation test。視窗模式的 Viz thread（`spawn_viz_compositor_thread`）沒有自動測試，2026-10-02 用 `cargo run` 手動確認過。
- **預設用 release 的 V8**（2026-10-02，P3 完成之後，`P3.md` 的「已決定的事項」第 10 條）。量了三次，前兩次互相矛盾：
  - **第一次**（每種跑兩次，沒有暖身；release 的 V8 用另一個 target 目錄，靠 symlink 讓 dev build 連結它）：改了 renderer 之後重新 build 和 link，debug 16.9、18.4 秒，release 3.1、7.1 秒；跑全部測試，debug 206.6、193.9 秒，release 82.5、49.4 秒。據此改成預設用 release。
  - **第二次**（改了之後，在新的設定下重量，每種兩次，沒有暖身）：全部測試 release 171.4、23.5 秒，debug 124.5、29.1 秒；重新 link 三次都在 3.0～4.8 秒。據此判斷量不出差別，撤回了改動。為什麼這次和其他兩次不同，沒有查到。
  - **第三次**：先刪掉 V8 的 build 目錄，從頭重編兩種 V8（debug 1269 秒、release 1142 秒；兩個 static library 的大小和之前相同，debug 和 release 的 binding 檔逐位元組相同），確認兩邊連結的 archive 正確，兩種各暖身一次，再交替量四輪（每輪的先後順序輪替）。中位數（範圍）：重新 build 和 link，debug 10.8 秒（5.0～21.1）、release 4.4 秒（2.1～7.2）；全部測試，debug 140.8 秒（78.1～218.8）、release 35.9 秒（16.5～56.5），每一輪 release 快 3.7～4.7 倍；單獨的 reftest，debug 26.9 秒（25.2～35.2）、release 7.4 秒（6.6～21.6）。最穩定的指標是 reftest 以外的測試 binary 回報的執行時間合計：debug 每次 30.2～35.8 秒，release 2.5～4.4 秒（暖身那次 17.5 秒，是 trybuild 第一次編譯）；每個啟動 renderer 和 V8 的 binary 都慢，例如 `browser` 的 navigation 測試 debug 約 4.5 秒、release 約 0.5 秒。不重新 link 再量兩組：debug 230、44 秒，release 69、8 秒。兩邊都是 362 個通過。
  - **雜訊**：同一個設定的全部測試從 8 秒到 230 秒都有，load average 在 3～20 之間（量的時候 Claude 的桌面 app 本身就佔了約一個核心）。剛 link 好的 binary 第一次跑比較慢（navigation 第一次 4.7 秒、第二次 2.1 秒）；`--no-verify-snapshot-checksum` 沒有影響（2.0 秒對 2.1 秒）。所以只比較相鄰的兩次，並先暖身。
  - **改了之後的驗證**：預設的 build 連結 release 的 archive、`ci.py debug-v8` 連結 debug 的（都直接比對過檔案）；兩邊的 `cargo test --workspace` 都是 362 個通過，reftest 照樣只有 `harness/chain-broken*.html` 是預期中的失敗；`ci.py matrix`（改用 debug 的 V8）6/6、clippy（沒有警告）、fmt、`ci.py deps`、`ci.py unsafe`、`ci.py docs` 都通過；crate 底下沒有多出 `target/` 目錄。`target/v8-debug` 約 15 GB。

### 已知限制

2026-10-04 和其他階段的合併，搬到 `docs/limitations.md`，只留目前仍然成立的。

## P4 Chromium 式 compositor

範圍、子階段和設計見 `P4.md`。P4.1 Compositor thread（2026-10-02 至 03）、P4.2 Layerize（2026-10-03）、P4.3 Invalidation（2026-10-03 至 04），各再分成幾小段，共 18 段（`P4.md` 的「子階段」）。新增的 thread：renderer 的 `Compositor`（cc 的 impl 端）；沒有新的 crate。驗收之後追加 P4.4 記憶體（`P4.md` 的「已決定的事項」第 75 條），2026-10-04 完成：

| 步驟 | 狀態 |
| --- | --- |
| P4.4 第 4.4a 段：layer 的範圍改成它的 chunk 的聯集 | 完成（2026-10-04） |
| P4.4 第 4.4b 段：空的和單色的 tile 不留像素 | 完成（2026-10-04） |
| P4.4 第 4.4c 段：tile 只留一份 | 完成（2026-10-04） |
| P4.4 第 4.4d 段：GPU device 遺失之後恢復 | 完成（2026-10-04） |

### 驗收

2026-10-04 在 macOS 26.6.2（Apple M4）上用 Rust 1.99.0 全部通過（commit `099b008`）：build、fmt、clippy（沒有警告）、`cargo test --workspace` 559 個、reftest 184 個（`harness/chain-broken*.html` 2 個是預期中的失敗）、`ci.py deps`、`ci.py unsafe`、`ci.py docs`、`ci.py debug-v8`（有 DCHECK 的 V8，559 個）、`ci.py matrix`（6 種 GC 設定）。Windows 的驗收照舊延後。

**視窗模式的手動檢查**（2026-10-04，macOS 26.6.2，Apple M4，`about:demo`）：
- compositor thread：整頁的滾輪和鍵盤（方向鍵、PageDown、Space、End、Home）捲動順暢；「Busy for 3 seconds」期間整頁照樣能捲，時鐘和藍色方塊停住，結束後恢復；
- layerize：A 框順暢、忙碌期間照樣能捲，捲到底後傳給整頁，捲過之後點擊顯示指標下的那一列；B 框（被綠色方塊蓋住右下角）綠色方塊一直在上面，忙碌期間等 main thread；2x 時 0.5px 的細框和 16 條細紋清楚均勻；
- invalidation（`RUST_LOG=compositor=debug,style=debug,layout=debug,paint=debug`）：藍色方塊每個 frame 都是 style `restyled=2`、box tree `built=0 shaped=0 restyled=1`、layout `laid_out=7`、paint `painted=1 cached=65`、raster 1–2 個 tile（`kept=611` 左右），時鐘那一秒多 shape 一段、多重排和重畫一個 box；「Busy for 3 seconds」結束時說明文字從兩行變一行，下面全部往上移，那個 frame 只重排 8 個 box，但重畫 52 個、raster 269 個 tile（位置變了，和 Blink 一樣要重畫）；整段沒有 error 或 warning；
- 時鐘、兩秒後變綠的方塊、「Hide or show」、「Click me」、被 `preventDefault` 擋住的連結、改視窗大小之後重新換行都正常，沒有殘影或沒更新的地方；連結、上一頁、下一頁、重新整理都正常。

**Compositor thread 和捲動**

| 項目 | 怎麼驗證 |
| --- | --- |
| JS 卡住時捲動仍然順暢：script 忙等時，compositor 捲 viewport、畫出來、回 ack，都比點擊的 ack 先到；之後點擊打到捲過的位置 | `cargo test -p browser --test navigation scrolling_goes_on_while_script_keeps_the_main_thread_busy` |
| `ProxyImpl` 和 `ProxyMain`：等 main thread 和不等的兩種模式、commit、pending / active tree、activate、abort、swap promise 照 commit 的順序兌現、display list 沒變時保留 tile、要不要 `BeginFrame` 和 main frame 只在改變時送 | `cargo test -p compositor proxy` |
| 位移同步（`SyncedScrollOffset`，viewport 和每個成為 layer 的容器）：交給 main、commit 對帳、abort、照 main 套用的順序逐筆夾值、換 document 或不再是 layer 時丟掉；仿照 cc 的 `LayerTreeHostScrollTest` | `cargo test -p compositor` |
| 捲動的計算（`ScrollNode`、按鍵和滾輪換算成位移）；`InputHandler`：滑鼠轉給 main，範圍裡的滾輪轉給 main，其他的從游標下最裡面的容器捲、往外傳到 viewport，ack 帶畫出它的 frame | `cargo test -p compositor` |
| Input 的分派和 ack 的端到端：導航前後的 input 只給畫出來的 document、還沒畫出 frame 時點擊被丟掉、ack 等它要的 frame（render-blocked 時也等）、`RendererGone` 是最後一則訊息、換 document 時清掉 timer | `cargo test -p browser --test navigation` |

**Layer 和 property tree**

| 項目 | 怎麼驗證 |
| --- | --- |
| 哪些捲動容器成為 layer（到畫面只有平移、沒有 opacity、有東西可捲），chunk 照順序切成「之前、容器、之後」，document 的 layer 一定在最前面；其他 layer 的範圍是它畫到的聯集，對齊到 tile 的格線 | `cargo test -p paint --test layerize` |
| K2：捲動容器成為 layer 之後的位置、clip、巢狀、畫在它之後的東西在它上面 | `cargo test -p shell` 加上 `--test reftest -- scroll/k2`（`4ff8d6e` 時的指令） |
| 捲動容器的捲動不重新 raster（被縮放的照樣重新 raster，當作對照） | `cargo test -p browser --test navigation scrolling_a_container_layer_rasters_nothing_again` |
| JS 卡住時成為 layer 的容器照樣捲；被蓋到的容器等 main；之後點擊打到捲過的位置 | `cargo test -p browser --test navigation a_container_layer_scrolls_while_script_keeps_the_main_thread_busy` |
| compositor 的判斷在每一點和 main 一致（要嘛轉給 main，要嘛捲同一串容器），包括 compositor 捲過、範圍還是之前畫的時 | `cargo test -p paint --test hit_test` |
| paint 的 layer 轉成 compositor 的 layer 和 property tree、scroll tree 和範圍；layer 保有 id、帶要重新 raster 的範圍，沒變的沿用 display list | `cargo test -p renderer --lib compositing` |

**Device pixel**

| 項目 | 怎麼驗證 |
| --- | --- |
| K3：2x 和 1.5x 時邊緣、border、圖片、捲動容器的 layer 對齊 device pixel | `cargo test -p shell` 加上 `--test reftest -- hidpi`（`4ff8d6e` 時的指令） |
| 邊緣、border 寬度、baseline、clip、捲動容器的位移、layer 的範圍取整到 device pixel | `cargo test -p paint --test artifact`、`cargo test -p paint --test layerize a_scroller_layer_is_on_the_device_pixel_grid` |
| 換了 device scale factor、CSS px 的大小不變時重新 paint | `cargo test -p shell --test browser_view a_new_scale_factor_paints_the_page_on_its_device_pixels` |

**Invalidation**

| 項目 | 怎麼驗證 |
| --- | --- |
| 數得出重新 raster 的 tile 數：script 改一個小方塊，畫面上的 4 個 tile 只重新 raster 1 個 | `cargo test -p browser --test navigation a_change_rasters_only_the_tile_it_is_in` |
| 比對新舊 display item 得出要重新 raster 的範圍（有變、移動、新增、刪除、順序、transform / clip / opacity、只有那一行、clip、反鋸齒） | `cargo test -p paint --test raster_invalidation` |
| compositor 只重新 raster 範圍碰到的 tile、layer 變大時保留沒變的、另一個 layer 或 document 什麼都不留；damage rect | `cargo test -p compositor layer_tree_host_impl` |
| DOM 的修改設好 style 和 layout tree 的 flag、DOM tree version 每次修改加一；style recalc 只標值變了的元素 | `cargo test -p dom --test style_flags`、`cargo test -p dom --test layout_tree_flags`、`cargo test -p style --test invalidation only_other_values_flag_the_boxes` |
| 只重算受影響的元素（改屬性、插入、看結構的選擇器、`:empty`、`:has()`、stylesheet、`rem`），結果和整份重算一樣 | `cargo test -p style --test invalidation` |
| script 跑了但沒改 DOM 時不畫新的 frame；改了的每個 frame 都畫 | `cargo test -p browser --test navigation script_that_changes_nothing_draws_nothing` |
| box tree 只更新 flag 指到的部分（就地換 style、只重新收集最近的 block box）、只重排變了的 box 和它的祖先、沿用沒變的 fragment，結果和整份重建、重排的一樣 | `cargo test -p layout --test update` |
| 自己畫的東西沒變的 box 沿用上次的 display item，結果、要重新 raster 的範圍都和整份重畫的一樣 | `cargo test -p paint --test paint_controller` |
| K4：改顏色、移動、刪除和加長、改文字、捲動容器裡的變化、class、`:first-child`、兄弟選擇器、`:empty`、stylesheet、`rem`、`:has()`、`display`、插入、變高推動後面的、變窄重新換行之後，畫面和整份重畫的一樣 | `cargo test -p shell` 加上 `--test reftest -- invalidation`（`4ff8d6e` 時的指令） |

### 做過的檢查

- **Mutation test**：每一段都弄壞實作的規則，確認有測試失敗；先跑最相關的測試，第一條沒抓到就停下來，補了測試或確認等價之後再繼續。第一輪沒抓到的，原因和怎麼補都記在下面。
  - **Compositor thread（P4.1）**：第 2b 段 16 條、2c 11 條、之後補的兩個測試空缺 4 條、`RendererGone` 3 條、3a 16 條、3b 18 條、3c-1 22 條、3c-2 22 條、第 4 段 4 條。補了測試之後抓到的：換了 bounds 還保留 tile（測試同時換了背景色，重畫是背景色造成的）；還沒有 document 時不 abort（其他測試都先導航）；夾值的 y 軸沒有下限、容器兩軸的「能不能被使用者捲」對調、範圍用 border box（測試只測了 x 軸、兩軸一樣、沒有 border）；捲動不要求 `BeginFrame`（測試手動送 `BeginFrame`）；main 套用別的 document 的捲動（舊頁面的捲動到 main 時新頁面還沒 layout，看不出差別）。拿掉的多餘程式：轉 input 之後的 `update_needs_begin_frame()`；main 套用 compositor 的捲動之後要求 main frame 和 commit（第 38 條之後 main 的夾值不會吃掉任何量，更正了第 36 條；和 Chromium 的 `kFinishedNoUpdates` 相同）。headless 等最後一個 ack 帶的 frame 而不是最新的那條測不到，見「測試守不住的地方」。
  - **Layerize（P4.2）**：4.2a 16 條、4.2b-1 19 條、4.2b-2 27 條、4.2c 17 條。「從後往前畫」第一輪只被 K2 抓到：compositor 單元測試的順序斷言在順序反過來時是空的範圍、自動成立，補強之後也抓到。補了測試之後抓到的：只改 x 軸的那條（測試頁面沒有橫向溢出）、兩條範圍的空間（外層在原點、裡面的一開始就在外層的 port 裡）、scroll tree 沒有 parent 和容器沒有範圍（只有端到端的測試涵蓋不到，補了 `compositing.rs` 的單元測試）、圖片取整到 CSS px（補了 `k3-image`）。4.2b-1 沒抓到的「main 套用之後不立刻重跑 pre-paint」由 4.2b-2 的端到端測試抓到。容器已經不是 layer 時不要求 main frame 的時序做不出來，見「測試守不住的地方」；兩條等價，見 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。
  - **Invalidation（P4.3）**：4.3a 29 條、4.3b 30 條、4.3c-1 62 條、4.3c-2 24 條、4.3d 21 條。
    - 多餘的程式，拿掉了：舊的 layer 沒有記錄的分支、比範圍（範圍由 item 的內容和 chunk 的狀態決定）；`rem` 變了強制其他元素、document 的子節點變了（style 和 box tree 各一條，root 來去就是 root 變了）、還沒有 style 的元素；標要重排時也標要重做 fragment（cache 被清掉的 box 一定會被排版）、viewport 變了時清 `LayoutView` 的 cache（viewport 是 taffy cache 的 key）；paint 比對 box 的 role、border、padding（另外比對了「背景給不給 canvas」、style、大小）。
    - 補了測試之後抓到的：raster invalidation 不比順序（原本的測試用調換 DOM 改順序，連節點都換了）、不數同一個節點的第幾個、不多放 1px、不被 clip 裁、範圍沒換算到 physical pixel（原本只數 tile）、damage 的兩條、layer id 不唯一（測試多加的捲動容器沒有背景、不產生 layer，斷言對空的集合永遠成立）；style 的兄弟、`:nth-*`、`:is()` 這類巢狀、`:nth-*(of S)`（原本的測試同時用了好幾種看結構的選擇器）；box tree 的 `display: none` 子樹被當成 inline 走、`border-style` 不算只影響畫面（machopo 只支援 `none` 和 `solid`，原本用的 `dotted` 被忽略）、釋放 box 時清掉了元素的新 box、重建的 block 裡沿用舊的 box（測試把子樹往後搬，舊的 box 先被釋放，蓋掉了錯誤）；replaced 的 box 用一般 block 的 taffy style、匿名 block 不換顏色、`LayoutView` 的大小（這三條一開始只被 reftest 抓到，補了 `layout` 的測試）；paint 沿用的條件裡固定大小的 box 多了或少了文字、跟著內容長大的容器、圖片到了、捲動容器的背景和文字的狀態對調、和更早的 paint 比對。
    - 只被量過、沒被排版的 box 也重做 fragment，machopo 現在沒有只量不排的情況，見「測試守不住的地方」；其餘沒抓到的是等價的，見 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。
- **找到的 bug**：
  - **視窗被遮住一陣子之後畫面卡住、整個作業系統變慢**（2026-10-03，使用者回報）：Viz 先把新的 tile 上傳到 GPU 才向 surface 要畫面；視窗被遮住時 wgpu 在 Metal 上立刻回 `Occluded`，我們跳過那個 frame、沒有 submit，而 wgpu 的上傳要等下一次 submit 才送出和釋放，被遮住的期間上傳都堆在 wgpu 裡（當時還沒有 invalidation，每個 frame 整頁重新 raster，一分鐘可以堆到數十 GB）。改成先要畫面，要不到就整個 frame 跳過、不上傳；加了取不到畫面、恢復、等太久的診斷 log。手動確認：切到別的 App 或縮小幾秒到 25 秒，切回來畫面立刻恢復，記憶體平穩。
  - 寫測試時發現的實作錯誤：等待中的 ack 在 render-blocked 的 abort 時留在 main thread，原本沒有測試守住（3a，完成後審查時發現）；頁面在容器外面什麼都沒畫時沒有 document 的 layer，範圍跟著不見（4.2a，改成第一個 layer 一定是 document 的）；子節點變了的元素自己也要重算（`:empty`）、改屬性也要標父節點（`.x + li`）（4.3b）；block 的 style 有排版上的變化時一律重新收集內容（繼承的 `white-space` 可能讓被摺疊掉的空白產生匿名 block，4.3c-1）。
- **行為的改變**（像素都不變，除了最後一點）：single-threaded 的 compositor 也走新的 commit，display list 沒變時不再重新 raster；還沒畫出任何 frame 時點擊被丟掉；End 鍵直接夾成最大位移（可能差 1 ulp）；1x 時小數寬度的 border 往下取整（例如 1.5px 原本畫成 2px、現在是 1px，沒有測試用到）。
- **不穩定的測試**：4.3a 重跑 mutation 時，`navigation` 有 5 個以上的端到端測試同時失敗，包括和那條 mutation 無關的；當時沒留下錯誤訊息，之後重現不出來（同一個 mutation 單獨跑兩次、完整重編之後的 `cargo test --workspace` 兩次、`navigation` 同時跑 4 個和 8 個共 28 次都通過）。mutation 的腳本之後都保留每條的完整輸出。每段的端到端測試都連跑三次確認過；寫測試時幾次寫錯時序（`run_until` 收走了後面要等的 ack、等到的是新頁面的 frame），都不是實作的問題。
- **工具和跑法**：
  - mutation 的腳本把改壞前的檔案搬回來時，修改時間比改壞後的 build 舊，cargo 繼續用改壞的程式（4.2b-2 時發現，之後的 mutation 都帶著前一條的錯）。改成搬回來之後更新修改時間，受影響的重跑。
  - macOS 每次啟動剛連結好的執行檔都會掃描它（`syspolicyd`、`XprotectService`），每個測試 binary 都靜態連結 V8，一次完整的 `cargo test --workspace` 有大約 4 分鐘花在這裡（303 秒中）。把終端機加進「開發者工具」之後，在終端機分頁裡跑就不再被掃描（185 秒）；長時間的測試和 mutation test 都在那裡跑。mutation test 改成先跑最相關的測試、抓到就停，每條從 3–4 分鐘降到約 25 秒。
  - **端到端的測試 binary 每個要等 13～19 秒才開始**（2026-10-04）：不是測試本身，是 `raster` 第一次建立 Skia 的字型管理器（`sk::FontMgr::new()`）時，CoreText 把執行檔所在的目錄當成 app 的 bundle，讀整個目錄好幾次；測試 binary 都在 `target/debug/deps`，那裡有 65 萬個檔案（103 GB），其中 63.8 萬個是 `.o`。macOS 的 debug build 預設 `split-debuginfo = "unpacked"`，DWARF 留在每個測試和執行檔自己的 `.o` 裡，每次重新 link 都留下新的一批，舊的沒有人刪，mutation test 累積得最快（library 的 object 在 `.rlib` 裡面，debug map 指向 rlib 的成員，散落的 `.o` 只有執行檔的有用）。同一個 binary 複製到小目錄跑只要 0.17 秒（原本 5.8～19 秒）。`packed` 每次 link 多跑 `dsymutil`（連結 V8 的 binary 每個約 9 秒），`off` 讓 backtrace 沒有檔名和行號（用小的 crate 試過），所以都不用；改成加了 `python3 scripts/ci.py prune`：刪掉 `target/*/deps` 裡沒有任何執行檔的 debug map（`N_OSO`）參照的 `.o`，期間拿著 cargo 的 build 目錄鎖。用一個 lib 加一個在 lib 裡 panic 的測試確認過：prune 前後、重新 build 再 prune 之後，lib 和測試的 backtrace 都有檔名和行號；cargo 拿著鎖時 prune 跳過那個目錄；prune 之後 `target/debug` 每個執行檔參照的檔案都還在（`target/v8-debug` 只有一個 10 月 2 日、cargo 已經不追蹤的舊 `machopo` binary 本來就缺）。另外每個 crate 都沒有 doctest，`cargo test` 照樣對每個 crate 跑 rustdoc（不用重編時 13～24 秒，重編之後約 100 秒），所以每個 crate 都設了 `doctest = false`。清掉 `target/debug` 重編之後，`cargo test --workspace` 從約 4 分鐘（build 42 秒、測試 binary 94 秒、reftest 約 39 秒、doctest）降到不用重編時 10～11 秒。`CLAUDE.md` 列的整套檢查一次 156 秒，其中 `ci.py debug-v8` 佔 103 秒（28 秒是重編）、`matrix` 43 秒，之後拿掉了 `debug-v8`（`P3.md` 的「已決定的事項」第 10 條）。
  - log 預設帶 ANSI 的顏色控制碼，接到 pipe 也一樣，要先濾掉才能 grep（`perl -pe 's/\e\[[0-9;]*m//g'`）。

- **P4.4 第 4.4a 段**（2026-10-04）：layer 的範圍改成它的 chunk 的聯集（`P4.md` 的「已決定的事項」第 75–77 條）。不是捲動容器的 layer（document 自己的除外）不再和整份 document 一樣大：範圍是它的 item 經過 transform、被 clip 裁過的聯集，裁在 document 裡，往外對齊到 256 device px 的格線（`paint::LAYER_GRID`，等於 compositor 的 tile 大小）。範圍在 `layerize` 時算好、存在 `PendingLayer` 上。之前的測試和 reftest 的結果都不變。
  - **新的測試**：`paint` 的 `layerize` 6 個（捲動框之後的 layer 只有它畫到的大小，1x 和 2x 的格線、不在格線上的 x、在格線內長大不動、跨過格線才變大；被 clip 裁掉的不算、完全被裁掉的不算、被 transform 移動的照移動後的位置；反鋸齒多出的 1px 跨過格線；超出 document 的部分；四個邊各由不同的盒子決定的聯集），原本的測試改成新的 API；renderer 的 `compositing` 1 個（格線等於 compositor 的 tile 大小）。
  - **寫的時候發現的**：clip 的測試一開始用 `overflow: hidden` 加上很高的內容，它本身就是有東西可捲的捲動容器，成了自己的 layer；加上 `opacity` 讓它不成為 layer 之後才測到 clip。對齊格線前的「空的就回傳空的」是多餘的（空的照樣是原點的空矩形），拿掉了。
  - **Mutation test**：18 條，第一輪 17 條抓到；「聯集只留第一個」沒抓到（測試裡每個 layer 幾乎只有一個 item），補了四個邊各由不同盒子決定的測試之後抓到，它也抓到了聯集四個方向各自的 4 條。
  - **記憶體**（視窗模式，`about:demo`，2x）：tile 從約 612 個降到約 336 個，`footprint` 從 412 MB 降到 298 MB。
  - fmt、clippy（沒有警告）、`cargo test --workspace` 565 個、reftest 184 個（`harness/chain-broken*.html` 照樣是預期中的失敗）、`ci.py deps`、`ci.py unsafe`、`ci.py docs` 都通過；沒有動到 `dom`、GC 和 binding，所以沒有跑 `debug-v8` 和 `matrix`。
  - **視窗模式**（2026-10-04 手動確認，`cargo run`）：`about:demo` 看起來和之前一樣：B 框和壓在它右下角的綠色方塊、下面的說明文字、細框、細紋和頁尾都完整；捲動整頁和兩個框、改視窗大小之後沒有缺塊或殘影。

- **P4.4 第 4.4b 段**（2026-10-04）：空的和單色的 tile 不留像素（`P4.md` 的「已決定的事項」第 78、79 條）。compositor 照常 raster，raster 完檢查像素：每個像素都一樣、完全不透明的 tile 丟掉 bitmap，畫成 Viz 已經有的 `SolidColor` quad；全透明的不畫；其他的照舊。tile 改記 `TileDrawInfo`（cc 的同名類別），`prepared tiles` 的 log 多記單色的 tile 數。reftest 的結果都不變（單色的 quad 和原本的 texture 畫出來一樣）。
  - **新的測試**：`compositor` 3 個（什麼算單色：不透明的單色、全透明、兩種顏色、部分透明、半透明、空的；同一個 layer 裡單色、有內容、沒東西的 tile 分別畫成 `SolidColor` quad、texture 和不畫；單色的 tile 照樣被保留，只重新 raster 改了的那個）。compositor 原本的測試和 `browser` 的 `a_change_rasters_only_the_tile_it_is_in` 改了：測試共用的內容改成底色加上點，讓 tile 照樣走 raster 和 resource 的路徑；端到端的那個改成數有像素的 tile（畫面下半部整片藍色，現在是單色的）。
  - **Mutation test**：8 條，第一輪 7 條抓到；「保留的 tile 丟掉它的顏色」沒抓到（重新 raster 畫出來一樣，只是多做事，原本的測試看不到 raster 了幾個），加了 `TilesPrepared` 和「單色的 tile 照樣被保留」的測試之後抓到。
  - **記憶體**（視窗模式，`about:demo`，2x）：336 個 tile 裡 142 個是單色，`footprint` 從 298 MB 降到 254 MB。
  - fmt、clippy（沒有警告）、`cargo test --workspace` 568 個、reftest 184 個（`harness/chain-broken*.html` 照樣是預期中的失敗）、`ci.py deps`、`ci.py unsafe` 都通過；沒有動到 `dom`、GC 和 binding，所以沒有跑 `debug-v8` 和 `matrix`。
  - **視窗模式**（2026-10-04 手動確認，`RUST_LOG=compositor=debug cargo run`，wgpu 的 renderer）：`about:demo` 看起來和之前一樣，大片的空白和背景色沒有變色、縫隙或接縫；捲動整頁和兩個框時 tile 交界處沒有閃爍或缺塊；改視窗大小之後畫面正確。改大小的期間每個 frame 重新 raster 一兩百個 tile，其中約四成是單色的；停下來之後藍色方塊每個 frame 照舊 `rastered=1` 或 `2`。

- **P4.4 第 4.4c 段**（2026-10-04）：tile 只留一份（`P4.md` 的「已決定的事項」第 80、82 條）。wgpu 的模式下，compositor raster 完就透過 `SharedImageInterface`（Viz 的 `GpuSharedImages`）在 Viz 的 GPU device 上建立 texture，丟掉 CPU 的那份；frame 的 `TransferableResource` 只帶 `SharedImage`，Viz 直接拿來畫。software 的模式（headless、reftest）照舊。視窗把介面交給分頁的 renderer 和工具列的 compositor；Viz 建好 device 之前照舊留像素。reftest 的結果都不變（不經過 wgpu）。
  - **新的測試**：`compositor` 3 個，用假的介面（有介面時 tile 不留像素、frame 帶著用 tile 的像素建立的 shared image；介面不給時照舊留像素；捲遠之後 tile 被釋放，shared image 也跟著釋放）。
  - **視窗確認時發現的**：改視窗大小時 Viz thread panic（wgpu 的 `Surface::configure`：「Failed to wait for GPU to come idle」）：Viz 重新設定 surface 的同時，compositor 正在別的 thread 上傳 tile、submit。改成兩邊共用一個 lock（`P4.md` 第 82 條）。測試不經過 wgpu 和視窗，抓不到，記在「測試守不住的地方」的視窗模式那一列。
  - **Mutation test**：5 條（不建立 shared image、有 shared image 還留像素、用空的像素建立、frame 不帶 shared image、拿不到 backing），第一輪全部抓到。
  - **記憶體**（視窗模式，`about:demo`，2x）：`footprint` 從 254 MB 降到 234 MB（CPU 那份 74 → 23 MB，GPU 那份 115 → 146 MB：所有有像素的 tile 現在都在 GPU 上）。活著的 texture 和照算的一樣（約 49 MB）；多出的約 95 MB 跟著動畫出現，靜態的頁面沒有。試了照 cc 的 `ResourcePool` 重用 texture，pool 確實用上了，GPU 那份照樣 148 MB，所以拿掉了，記成已知限制（`P4.md` 第 83 條）。
  - fmt、clippy（沒有警告）、`cargo test --workspace` 571 個、reftest 184 個（`harness/chain-broken*.html` 照樣是預期中的失敗）、`ci.py deps`、`ci.py unsafe`、`ci.py docs` 都通過；沒有動到 `dom`、GC 和 binding，所以沒有跑 `debug-v8` 和 `matrix`。
  - **視窗模式**（2026-10-04 手動確認，`RUST_LOG=compositor=debug cargo run`，wgpu 的 renderer）：連續改視窗大小十幾秒（快慢都試、最大化再還原），每個 frame 重新 raster 一兩百到三百多個 tile，沒有 panic；`about:demo` 看起來和之前一樣，沒有黑色或空白的 tile；捲動整頁和兩個框沒有缺塊或殘影；網址列打字、選取文字正常；切到別的 App 或縮小再切回來，畫面立刻恢復。

- **P4.4 第 4.4d 段**（2026-10-04）：GPU device 遺失之後恢復（`P4.md` 的「已決定的事項」第 81、84、85 條；Chromium 的部分對照了原始碼）。Viz 偵測到遺失（wgpu 的 device-lost callback，或 `about:gpucrash`）時重建 device、surface 和 pipeline，再透過 `SharedImageInterface` 上的 `ContextLostObserver` 通知每個 compositor：分頁的轉成 compositor thread 的 `DidLoseLayerTreeFrameSink` task，工具列的經 event loop 轉到 UI thread。compositor 丟掉有 resource 的 tile，重新 raster 到新的 device，整個畫面標成 damage。這段期間 Viz 不畫舊 device 上的 shared image。重建失敗就重試，3 次都失敗就結束程式。`about:gpucrash` 只處理使用者自己輸入的；網頁裡的連結照一般的 navigation 走。`about:demo` 加了說明和一個指向它的連結。
  - **新的測試**：`compositor` 4 個（遺失之後只重新 raster 有 resource 的 tile、舊的 shared image 被放掉、damage 是整個畫面；`ProxyImpl` 要求 `BeginFrame`、畫出新的 resource；遺失的通知轉成 compositor thread 的 task，host 不在了就不再通知；跑 thread 的 round-trip 多一段遺失）；`viz` 4 個（沒有 device 時不建立 shared image；還在的 client 被通知、不在的被清掉；`Display` 轉給 renderer 並要求重畫；Viz thread 把 `LoseContext` 交給 renderer）；`browser` 2 個（`about:gpucrash` 送出 `LoseContext`、不 navigate；連結到它只是 navigation）；`shell` 1 個（工具列在遺失之後重新 raster、送出新的 frame）。測試用的假 `SharedImageInterface` 移到 `compositor` 的 `fake_gpu`，多了模擬遺失。
  - **寫的時候發現的**：網頁裡的連結原本會走到 `WebContents::navigate`，也就是任何網頁都能用一個連結讓 GPU 遺失；改成連結直接開始 navigation，只有使用者自己的 navigation 處理 debug URL（Chromium 也是），加了測試。
  - **Mutation test**：21 條，全部抓到。寫測試時先補了兩處原本會漏掉的：compositor thread 的迴圈分派 task（延伸了跑 thread 的 round-trip 測試）、工具列遺失之後真的重新 raster（`composite` 不管有沒有丟掉 tile 都會送 frame，改用會計數的假介面確認）。
  - **冒煙測試**：用 `about:gpucrash` 當起始網址啟動，Viz 在下一個 frame 前做出新的 device（`generation=1`），沒有 panic。
  - fmt、clippy（沒有警告）、`cargo test --workspace` 581 個、reftest 184 個（`harness/chain-broken*.html` 照樣是預期中的失敗）、`ci.py deps`、`ci.py unsafe`、`ci.py docs` 都通過；沒有動到 `dom`、GC 和 binding，所以沒有跑 `debug-v8` 和 `matrix`。
  - **視窗模式**（2026-10-04 手動確認，`RUST_LOG=info cargo run`，wgpu 的 renderer）：在網址列輸入 `about:gpucrash`，Viz 讓 device 遺失，3 ms 後做出新的（`generation=1`），頁面和工具列恢復正常，藍色方塊繼續動；再輸入一次是 `generation=2`，照樣恢復；網址列和歷史紀錄都沒有留下它。點 `about:demo` 裡指向它的連結只是一次 navigation，載入失敗、顯示錯誤頁（`ERR_UNKNOWN_URL_SCHEME`），GPU 沒有遺失。捲動和改視窗大小正常。

### 已知限制

2026-10-04 和其他階段的合併，搬到 `docs/limitations.md`，只留目前仍然成立的。
