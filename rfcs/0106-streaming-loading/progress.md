# RFC 0106 的進度：串流的載入

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-20。完成日期：2026-11-20（macOS，Apple M4）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a：data pipe（`ipc::data_pipe`） | 完成 | 2026-11-20 |
| 1b：回應分成 head 和 body，`net` 的 `URLLoaderClient`，`fetch(done)` 照舊 | 完成 | 2026-11-20 |
| 1c：HTTP 的 body 一段一段讀、解壓、sniff，`file:` 一段一段讀、`data:` 走管道 | 完成 | 2026-11-20 |
| 1d：第 1 階段的檢查和文件 | 完成 | 2026-11-20 |
| 2a：`CommitNavigation` 帶開頭和管道、Browser 收到 head 和開頭的 1024 bytes 就 commit、結束轉告 renderer；renderer 讀到結尾才 parse | 完成 | 2026-11-20 |
| 2b：收到一半的錯誤（中止 parser）、取消、DevTools 和 Navigation Timing 的時機 | 完成 | 2026-11-20 |
| 2c：第 2 階段的檢查和文件 | 完成 | 2026-11-20 |
| 3a：renderer 收到 commit 就建文件、邊收邊 parse（文件拿著管道和解碼器、parser 等下一段、等 script 時照讀）、編碼照開頭、Navigation Timing 和 DevTools 的結束、有 `<body>` 才畫 | 完成 | 2026-11-20 |
| 3b：parser 的時間預算（切片、超過預算或有輸入就讓出、進了 `<body>` 才讓出） | 完成 | 2026-11-20 |
| 3c：開頭 1024 bytes 的測試、`moving-between-documents/` 等 WPT 的預期結果、第 3 階段的檢查和文件 | 完成 | 2026-11-20 |
| 4a：舊畫面的時限（commit 之後 4 秒還畫不了就送只有背景的空 frame，放行等著的輸入） | 完成 | 2026-11-20 |
| 4b：`<head>` 先到、body 晚到的情境，整份 RFC 的檢查和文件 | 完成 | 2026-11-20 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 資料管道 | 照順序、上限照 byte 嚴格守住、背壓、兩端的結束、`watch` 的通知、跨 thread | `cargo test -p ipc --test it data_pipe::` |
| 網路 | `load`：head 和 body 的管道、結束（錯誤、傳輸的大小、時間）、沒有回應的只有結束、body 沒人讀時等著、放掉管道就停、`HEAD`、等或自己跟 redirect、取消 | `cargo test -p net --test it client::` |
| 網路 | body 一段一段來：head 在 body 之前、收到一段交一段（壓縮的也是）、解不開的在 head 之後從結束來、沒有 `Content-Type` 時等開頭的 1024 bytes（或結尾）、redirect 的 body 不等、`file:` 分段讀；`fetch` 收齊才交回 | `cargo test -p net --test it client:: load:: content_encoding::` |
| 網路 | `Content-Encoding` 一段一段解，一次給一個 byte 和整份一樣：gzip 的 header（選用的欄位）和 trailer、`deflate` 的 zlib 和 raw、brotli、截斷和之後多出來的 | `cargo test -p net --lib content_encoding::` |
| 導覽 | Browser 收到 head 和開頭就 commit：其餘的 body 經過管道（子 frame 也是、比管道的容量大的也是）、短的整份帶著、結束在 commit 之後到；只等 head 的導覽被取代時不 commit | `cargo test -p browser --test it body:: a_replaced_navigations_response_commits_nothing` |
| 導覽 | 開頭的 1024 bytes：伺服器在開頭之內停一陣子，engine（wasm 的 `<meta>`）和編碼（`<meta charset>`）照樣是開頭說的 | `cargo test -p browser --test it the_encoding_is`；`cargo test -p shell --test it the_engine_is` |
| 邊收邊 parse | 收到的部分在其餘的到之前就 parse（main frame 和子 frame）、`DOMContentLoaded` 等到結尾、parser 等 script 時照樣讀管道（比管道的容量大的也讀完）、一個 task 讀不完的下一個 task 接著讀、commit 之後被取代的文件不再 parse 其餘的；`load` 時 Navigation Timing 有 body 的大小和 `responseEnd` | `cargo test -p browser --test it body::` |
| 邊收邊 parse | 解碼：字元切在兩段之間、BOM 自己一段、開頭結尾被切斷的字元照樣是 UTF-8、開頭之後的字元不改變編碼；XHTML 的 CDATA 標記切在兩段之間、純文字只有一個 `<pre>` | `cargo test -p renderer --lib encoding:: document::` |
| 邊收邊 parse | parser 的預算：有了 body 之後讓出（body 裡的計時器在兩個 script 之間跑）、`<head>` 裡不讓出、讓出之前至少 parse 一片、預算在讓出 6 次之後變大；文字照整個字元切片 | `cargo test -p browser --test it body::`；`cargo test -p renderer --lib budget input_is_given` |
| 收到一半停下 | 出錯的留下收到的部分、中止 parser（readyState 到 `interactive`、`complete`，沒有 `DOMContentLoaded` 和 `load`）、一開始就出錯的照樣 commit（不是錯誤頁）、載入照樣結束（`DidFinishLoad`）、子 frame 的不擋住 parent 的 `load`、停止載入停下 body 還在來的文件 | `cargo test -p browser --test it body::` |
| 畫面 | 有 `<body>` 才畫：只有 `<head>` 的文件不畫、有了 body 之後 body 還沒收完也畫 | `cargo test -p shell --test it drawn` |
| 畫面 | 舊畫面的時限：畫不了的新文件在等待時間之後先顯示空的 frame（沒有 tile）、之後畫出來；等著 frame 的輸入的 ack 跟著空的 frame；畫出來過的文件之後被 sheet 擋住時不換成空的 | `cargo test -p browser --test it -- a_new_document_that_cant a_drawn_document_blocked an_ack_goes_with_the_empty` |
| DevTools 和 trace | 每段一個 `dataReceived`、結束時 `loadingFinished`；body 出錯的只有一個 `loadingFailed`、被停下的帶 `canceled`；頁面的 log 有「Failed to load resource: net::…」；被中止的文件的原文是收到的部分；trace 的 `responseEnd` 在 body 收完時、時間在 `fetchStart` 和 `domInteractive` 之間 | `cargo test -p renderer --lib network_agent::`；`cargo test -p shell --test it devtools stops_short trace::marks` |
| 視窗 | body 分兩段、隔三秒到的頁面：停頓期間畫出前一段（readyState 是 `loading`），之後加上其餘的 | `python3 scripts/test.py scenarios streaming`（`--window` 也是） |
| WPT | `moving-between-documents/` 全部通過（after-prepare 的兩份照舊 known intermittent）、`navigation-timing/` 的 `responseEnd` 在 `load` 之前是 0 | `python3 scripts/test.py wpt html/semantics/scripting-1/the-script-element/moving-between-documents/ navigation-timing/` |

## 做過的檢查

macOS、Apple M4，2026-11-20。

### 測試和 mutation test

每一段都照 `docs/testing.md` 跑了 mutation test（`python3 scripts/test.py mutate`，spec 是這個資料夾的 `mutants-*.toml`），共 114 個，抓到 102 個。2a 沒抓到的「commit 不等 body 的開頭」在 3c 補了測試、抓到了。其餘沒抓到的：等價的記在 `docs/test-gaps.md` 的「等價、不用處理的 mutation」（inflate 輸出滿了就停、Browser 在 commit 之前聽到結束、子 frame 丟掉比它的文件舊的 commit、空的 frame 只在被擋住時才要）；補不了或還沒補的記在「只靠審查」和「能補、還沒補的測試空缺」（子 frame 在 commit 時不畫、parser 停下時馬上開始載入、有輸入等著時讓出、script 之後縮短預算、要接著 parse 的不被資料推動、loop 照空的 frame 的時間醒來）。2a 的「馬上 commit 的不丟掉同一個 frame 等著的舊 commit」：那段程式（`forget_before`）在 3a 文件拿著 body 時拿掉了。沒有作用的程式在 mutation test 找到時拿掉了（body 收完時告訴 `Page` agent、空的 frame 之後重設 `painted`）。

### 找到的 bug

- 新文件 commit 時先停下舊文件的 load、再送新的 commit，renderer 把只收到一半的舊文件 commit 了（2a）：改成先送 commit。
- main frame 丟掉舊的 commit 之前先清掉 DevTools 留著的、顯示中的文件的 body 和 log；子 frame 只擋得住自己請求的導覽的舊 commit（2b）。
- parser 停下來等其餘的 body 時沒有開始它做出的元素要的載入，`<link>` 的 sheet 晚到一輪，compositor 的 frame 先跑時文件先排版了沒有 sheet 的樣子：`a_child_frame_waits_for_its_stylesheets` 約六次失敗一次（3a 帶進來、3b 找到）。
- 預算是 0 時 parser 一片都沒給就讓出，永遠不前進（3b，寫的時候）。

### 不穩定的測試

- `a_child_frame_waits_for_its_stylesheets`：上面的 bug，修了之後 `browser` 的整套連跑 20 次沒有失敗。
- 2a 的「被取代的導覽」：`navigate` 有時趕在 Browser 讀到開頭之前，原本的導覽就不 commit；改成先等它 commit 再取代（3a）。
- WPT 的 `replace-before-load/` 的 `history-pushstate.html`、`history-pushstate-during-load.html`、`iframe-src.html`：子 frame 的 `pushState` 被 browser 晚到的 `SetHistoryOffsetAndLength` 蓋回去，原本一直失敗、現在有時通過（10 次 1 到 4 次）；不在這份的範圍，`.ini` 標成 known intermittent、附上原因（3c）。

### WPT

- 變成通過的（拿掉或改了 `.ini`，各連跑 10 次都通過）：`moving-between-documents/` 的 18 份（`before-prepare-*-success-empty-src-*` 四份和 move-back 的十四份），`navigation-timing/` 的 5 份（`nav2-test-response-end-and-duration-before-during-and-after-load-event.html`，和前一份 RFC 做到、那時沒更新的 4 份；`nav2-test-timing-persistent.html` 剩 Part 1，`redirectStart` 是 0，還沒查）。
- 跑過的目錄：`fetch/api/basic/`、`fetch/api/redirect/`、`navigation-timing/`、`html/browsers/browsing-the-web/navigating-across-documents/`、`html/semantics/scripting-1/the-script-element/`、`html/dom/documents/dom-tree-accessors/`、`html/syntax/`、`html/webappapis/timers/`：更新之後 0 個意外的結果。

### 最後的檢查

- `cargo test --workspace`：4264 個通過、0 個失敗、1 個 ignored。`cargo clippy --workspace --all-targets -- -D warnings` 沒有警告。`cargo fmt --check` 在每個階段的最後抓到的換行都照 rustfmt 改了。
- `python3 scripts/test.py scenarios`：96 次（75 份，按鍵的兩種快捷鍵各一次），0 個失敗；`streaming` 的截圖看得到停頓期間畫出的前一段。
- `python3 scripts/test.py scenarios --window`：72 次，0 個失敗（`caret` 要讓 caret 閃爍，跳過），pipe 的 `--window` 也通過；`target/automation-results/streaming/` 的截圖在真的視窗裡看得到停頓期間的前一段（「still loading: loading」）和之後的整份。
- `sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。
