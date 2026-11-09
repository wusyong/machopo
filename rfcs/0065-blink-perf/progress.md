# RFC 0065 的進度：用 blink_perf 和 Chrome 比

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-20。完成：2026-10-20，Fedora 44（AMD Ryzen AI 9 HX 370；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0065` 上做；三段做完之後一起 merge 回 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 共用的程式拆成 `common.py`、`perf_tests/` 固定在 `../third_party/`、`bench.py blink-perf` 跑出數值和報告 | 完成 | 2026-10-20 |
| 2 有效的檢查、排除、頁面清單（第 4、8–13 條：清單從 10 頁開始、照領域分段；兩個瀏覽器綁在最快的 CPU 上） | 完成 | 2026-10-20 |
| 3 `--against`（第 14、15 條：不判斷單頁變快變慢） | 完成 | 2026-10-20 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 來源 | `ci.py third-party` 把 `perf_tests/` 下載到 `../third_party/blink_perf/`，換 commit 時重新下載，不是它下載的目錄不動 | 手動（`docs/test-gaps.md` 的「只做過手動確認」） |
| 共用的程式 | 拆出 `common.py`、搬過 `--against` 的 worktree 和 build、共用摘要之後，Speedometer 和 raster 的工具行為不變 | `speedometer_test.py`、`raster_test.py` |
| 跑頁 | 每頁兩邊各開一個新的瀏覽器；hook 交回 runner 的數值、log 和 document 的摘要（摘要的函式接在 `runner.js` 後面，不 `import()`），fatal 優先於統計，沒有 catch 的例外 | `blink_perf_test.py` 的 `ServerTests`、`RunTests`（假的 `perf_tests/`，dev build 的 kun 和 Chrome for Testing） |
| 有效 | 兩邊都有數值、單位相同、摘要相同；摘要失敗時數值照樣交回、頁面無效 | `ValidTests`、`RunTests`（兩邊做的事不同的頁、摘要失敗的頁） |
| 排除和標記 | 只有 Chromium 的 harness 有的東西、自己交統計的頁不跑；會吞例外的、kun 快 2 倍以上的標出來，不算進幾何平均 | `ScanTests`、`ValidTests`、`ReportTests` |
| 清單 | 預設跑清單，給路徑時只跑那些路徑底下的；清單上變無效的列在報告最前面 | `CommandTests`、`ReportTests` |
| CPU | Linux 上兩個瀏覽器（連同取樣器）用 `taskset` 綁在最高時脈的 CPU 上，每顆一樣快或不知道時不綁 | `CpuTests`；12 頁綁之前和之後各連跑兩次（下面） |
| 兩個版本 | kun 兩個版本輪流、另一個先，pin 不同時不 build；單位和摘要以 `REV` 的第一次為準；報告的區間、變化、時間的幾何平均（runs/s 倒過來）、每次的中位數 | `AgainstTests`、`RunTests::test_two_versions_of_kun`；`--against HEAD` 跑一次（下面） |

## 做過的檢查

**完成時（2026-10-20，Fedora 44）**：三段做完時各跑過 `cargo test --workspace`（3311 個通過；這份只改了腳本和文件）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check`（連同 `sdk/lua/generator`）和 `sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`；`blink_perf_test.py` 31 個、`speedometer_test.py` 85 個（skip 1，Windows 專用）、`raster_test.py` 9 個通過。rebase 到 `main` 的 `1fcc2426`（RFC 0064 的第 1–3 段）之後全部再跑一次：`cargo test --workspace` 3382 個通過，其他照樣通過。

**量過的**：
- **`dom` 目錄**（第 1 段，kun `75f224e3`）：19 頁中 14 頁兩邊都有數字，kun 慢 5.72 倍（幾何平均），最慢的 `range-select-node` 179 倍。
- **和 kun 的範圍相關的 13 個目錄**（第 2 段，kun `1f68e780`，那時 hook 還用 `import()`、時限 120 秒）：457 頁（`details.md` 估 424：沒算子目錄裡的），2 小時 15 分。有效 250 頁（可疑 22、會吞例外 2），無效 140，排除 67（tracing 57、自己交統計 5、`testRunner` 3、`GCController` 1、`gpuBenchmarking` 1）；不算可疑的 226 頁，kun 慢 3.48 倍。
  - 無效的原因（kun 缺的，頁數）：window 的具名屬性約 55；`XMLHttpRequest` 35（20 多頁是 `runner.js` 的 `loadFile` 用同步 XHR）；`Worker` 8；`display_locking/` 和 `layout/large-table-with-collapsed-borders-and-colspans*` 的 `Cannot read properties of undefined (reading '0')` 6（還沒查）；Declarative Shadow DOM 5；`execCommand`、`selection.modify` 6；`atob` 3；`document.write`、`caretRangeFromPoint`、`isInputPending`、`WebGLRenderingContext` 各 2；XML 的 `DOMParser` 1。
  - 摘要不同的 5 頁（`layout/flexbox-input`、`flexbox-with-stretch-layout`、`multicol/balance-tall-*`）都設定 `innerText`，kun 沒有（DOM 不變）：`flexbox-input` 的文字 kun 51 字、Chrome 15069 字，kun 每秒 186 萬次、Chrome 7 萬次。有效的檢查抓到了少做事的頁。
  - 逾時：`layout/line-layout-repeat-append-select`、`shadow_dom/shadow-style-share-with-distribution`、`display_locking/outer_sizes_change_iframe`、`svg/SvgNestedUse`（兩邊）在 90 秒照樣逾時；Chrome 逾時的 `bindings/node-list-access`（kun 有數值）和 `SvgNestedUse` 原因還沒查。
  - 可疑的 22 頁（kun 慢的倍數 < 0.5）還沒人工確認：`layout/anchor-large` 0.004、`layout/grid-with-block-constraints-dependence` 0.008、`layout/gap-decorations-multicol-lots-of-columns` 0.009、`editing/textarea-dom` 0.014、`parser/css-parser-yui` 0.016、`layout/grid-nested-baseline` 0.036、`layout/flexbox-row-stretch-height-indefinite` 0.129、`css/CSSQuotesCreate` 0.148、`parser/not-in-button-scope-deeply-nested` 0.149、`shadow_dom/v1-small-deep-distribution` 0.158、`bindings/typed-array-construct-from-same-type` 0.161、`layout/flexbox-deeply-nested-with-ortho-flow` 0.161、`layout/character_fallback` 0.183、`bindings/typed-array-construct-from-typed` 0.207、`parser/in-button-scope-deeply-nested` 0.240、`layout/nested-percent-height-tables` 0.311、`shadow_dom/v1-mutate-shallow-tree-then-slot-assigned-nodes` 0.333、`css/NestingIdentNonProperty` 0.364、`bindings/typed-array-construct-from-array` 0.396、`layout/hittest-nested-inline-blocks-listbased` 0.430、`display_locking/unlock_top_items_nested` 0.433、`layout/ruby` 0.464。線索：兩頁 `grid-*` 用了 `innerText`；`bindings/typed-array-*` 只跑 JS（兩邊同一個 V8，可能是真的）；`layout/ruby`、`anchor-large` 用到 kun 不一定有的排版。
  - 會吞例外的 2 頁人工看過：`bindings/structured-clone-long-string-{serialize,deserialize}` 的 `catch` 交給 promise 的 reject，runner 收得到。
  - 有效、不可疑的 228 頁加上 `dom/range-select-node` 是第一版清單（`1cf1dc00` 的 `pages.txt`），現在是之後加頁的候選。用它跑第二次到 121 頁時停下（約 53 分鐘，全部估 1 小時 40 分）：兩次都有數字的 143 頁，倍數的比中位數 1.20、九成在 1.60 以內，最大 83.5（`dom/select-multiple-add` 2.43 → 203）；3 頁在可疑的門檻附近來回；`display_locking/fieldset_container_width_changes` 量測本身約 74 秒，加上載入和熱身超過 90 秒的時限。
- **大小核**（第 13 條）：12 頁的起點連跑兩次，同一頁的倍數差到 1.5 倍（`bindings/set-attribute` 10.8 和 7.21）：kun 和 Chrome 各自整頁落在 Zen 5（CPU 0–3、12–15，5.16 GHz）或 Zen 5c（3.29 GHz）上，kun 的 `set-attribute` 是 209 或 314 runs/s。都綁在快核上 7.2 和 7.1 倍，都綁在慢核上 7.4 倍。綁了之後連跑兩次，10 頁的倍數差在 16% 以內（中位數 7%）；`css/AttributeDescendantSelector`（kun 每個 process 落在 74、125 或 230 runs/s，原因還沒查）和 `dom/select-single-add`（Chrome 同一次裡在約 1900 和 4000 之間跳）拿掉，清單剩 10 頁，跑一次約 6 分鐘。
- **`--against HEAD`**（第 15 條，kun `c41d5cf6`，同樣的程式碼，10 頁各 3 次，15 分鐘，含重新 build）：時間的幾何平均 1.000，但 5 頁的區間不重疊：`css/PseudoClassSelectors` +33.8%（一個 build 三次都在 1750–1775，另一個都在 2310–2400）、`layout/line-layout-line-height` +15%、`bindings/create-element` −5.7%、`set-attribute`、`insert-before` 約 −3%。兩個 build 裡程式碼的位置不同；拿掉單頁的變快、變慢。

**實作時的發現**：
- `runner.js` 的 `logFatalError` 先呼叫 `finish`（寫出手上的統計）：hook 一開始在 fatal 時交了空的數值，改成先記下 fatal、統計時送錯誤。
- hook 一開始用 `import()` 載入摘要的程式：`dom/range-select-node` 的 20 輪在 36 秒跑完，kun 的 `import()` 再過 285 秒才 resolve（Chrome 約 1 秒），整頁逾時；改成 server 把 `summarize` 接在 `runner.js` 和 hook 之間（這頁之後 183 倍）。`import()` 慢記在 `docs/limitations.md` 的「Script」。
- 取樣時兩邊都逾時的 `mutation/` 5 頁自己呼叫 `logStatistics`、沒有 `"Time:"`，量兩輪：排除（第 10 條）。
- 第 2 段原本把第一次有效的 229 頁全放進清單，訂時限 90 秒時只算了量測、沒算載入和熱身；使用者覺得一次跑全部太久，改成從小的一組開始（第 12 條）。
- kun 的 bug：`dom/deep-tree-clonenode`（深度 10000 的 `cloneNode(true)`）讓 renderer thread 的 stack 溢位、整個 abort，記在 `docs/limitations.md` 的「Bindings 和 DOM API」。
- `test.py mutate` 的 bug：Python 的 bytecode 快取照原始檔的大小和修改時間的整秒判斷，和原文一樣長、一秒內測完的 mutation 還原之後，測試照樣跑 mutation 的程式（`a / b` 換成 `b / a` 那條還原之後，`test_the_comparison` 失敗）。改成寫回 `.py` 時刪掉它的快取，拿掉修改確認會重現、加上之後不會；這份的四份 spec 用修好的版本重跑。

**mutation test**（spec 不放 repo，scratchpad 的 `mutants/0065-{1,2,2c,3}.toml`；用修好的 `test.py` 重跑，81 條全部抓到）：
- 第 1 段 19 條：比值的方向（`ms` 當成越大越好、`runs/s` 除反、`percent` 也比、單位不同也比）、hook 沒接上、`resources/` 的頁也跑、指定兩次跑兩次、指定的頁不存在時略過、空的數值當成結果、log 沒留、報告放整段錯誤、目錄和總的幾何平均、fatal 時送統計、fatal 不送、不送 log、沒有 catch 的例外不送、server 送 root 外的檔案、`.wasm` 的 MIME type。第 2 段之後報告只替有效的頁分目錄，「沒有數字的目錄也列表」那條不成立，拿掉。
- 第 2 段 34 條：每一條排除的規則、`PerfTestRunner.gc()` 不算、帶 `"Time:"` 的不排除、`.catch(`、掃載入的 script 但不掃 `runner.js`、排除的頁照樣跑（兩個地方）、摘要和單位不比、沒有摘要也算有效、2 倍也算可疑、會吞例外不標、幾何平均算進可疑的、清單上變無效的、排除的不列、清單的註解、預設不跑清單、報告不知道清單、`result` 沒留摘要、hook 不送摘要、log 的文字算進摘要、`summarize` 沒接上、留著 `export`（整段 script 語法錯，假頁全部逾時，424 秒）、摘要丟例外時不送、`common.Server` 不送 `summary.mjs`、Speedometer 的 server 不轉給它、摘要不比文字。
- 路徑和 CPU 13 條：路徑不過濾、照名字的開頭比、單頁不算、沒有斜線的資料夾不算、清單裡沒有那些路徑時照樣跑、報告只拿到跑的那幾頁；挑最慢的 CPU、每顆一樣快時也綁、不知道時脈的 CPU 略過、範圍不合併、kun 或 Chrome 沒綁、沒有 `taskset`。
- 第 3 段 15 條：`--against` 沒接上、current 先、pin 不檢查、沒跑的版本、錯誤、兩個版本的單位和摘要不比、沒有摘要也算、參考是各自的第一次、只用第一次的數值、不列不比的頁、排除的頁不留、摘要寫成 Chrome、runs/s 的時間沒倒過來（一開始沒抓到：測試裡那頁兩個版本的中位數一樣，改成會變）、Speedometer 的 pin 檢查。原本還有 4 條是單頁變快變慢的判斷，第 15 條拿掉判斷之後一起拿掉。

**不穩定的測試**：`speedometer_test.py` 跑四次有一次一個測試失敗、沒有留下是哪一個；拆 `common.py` 時查過同一個現象：`ChromeTests.test_the_page_has_focus`（headless 的 Chrome 有時頁面沒有 focus）約 10–15 次失敗一次，在 `origin/main` 的 worktree 上也重現，和這份無關，還沒修。
