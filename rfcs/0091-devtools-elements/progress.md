# RFC 0091 的進度：DevTools 的 Elements：`DOM`、`CSS` 和 `Overlay`

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-09，macOS（Apple Silicon）。在 `0091` 上做。

## 步驟


| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 原型：停住時借 `Dom` 和 layout、停住時重算 style 和 layout（第 5 節） | 完成 | 2026-11-09 |
| 1a 樹：nodeId、lazy push、改動的掛點和事件、停住時回答、不用 JS 的查詢（第 1 到 5 節） | 完成 | 2026-11-09 |
| 1b 和 JS 的連接：`resolveNode`、`requestNode`、`$0`、Console 顯示節點、Event Listeners、`performSearch`（第 6 節） | 完成 | 2026-11-09 |
| 2 Overlay 和 Computed | 還沒開始 | |
| 3 Styles | 還沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 停住時 host 讀得到 DOM：停在 script、`click()` 呼叫的 listener、`createElement` 裡的 custom element constructor、MutationObserver 的 callback，每次都是 script 到那時為止的 DOM | `cargo test -p dom_js --test it devtools::the_host_reads_the_dom_while_paused` |
| DOM 記下樹的改動：有人要時依序記插入（連同前一個兄弟節點）、移除、屬性、文字、inline style、shadow root，拿走一次；沒人要時不記，不要了時丟掉記著的 | `cargo test -p dom --test it probes::` |
| `getDocument` 預設兩層、每個容器帶子節點數、只有空白的文字不送、只有一個文字子節點的元素帶著它、root 元素帶 frame；`requestChildNodes` 的 `setChildNodes` 在回答之前；深度 0 是錯誤 | `cargo test -p shell --test it devtools_dom::the_document_is_sent_as_asked_for` |
| script 的改動對 session 有的節點送事件：插入（前一個是 session 有的最近的兄弟節點）、屬性的設定和移除、文字、沒送子節點的節點的數量、inline style、shadow root（先送 host 的子節點）、移除；只有空白的文字不送 | `cargo test -p shell --test it devtools_dom::script_changes_are_told` |
| 一批的改動照一批結束時的 DOM：倒過來插入的節點照 DOM 的順序排，插入又拿掉的不送 | `cargo test -p shell --test it devtools_dom::a_batch_is_told_in_the_doms_order` |
| 查詢送出找到的節點以上的路；`describeNode` 用 backend id、不給新的編號；`getOuterHTML`、`getAttributes`、`collectClassNamesFromSubtree`、`pushNodesByBackendIdsToFrontend`（找不到的是 0）；iframe 的 frame、文件和 `getFrameOwner`；錯誤的文字 | `cargo test -p shell --test it devtools_dom::queries_send_the_path_to_what_they_find` |
| iframe 換了文件時送它的移除和插入；main frame 的新文件解析完時送 `documentUpdated`，舊的編號不再有效 | `cargo test -p shell --test it devtools_dom::new_documents_are_told` |
| 停住時先送 script 到那時為止的改動，命令照那時的 DOM 回答 | `cargo test -p shell --test it devtools_dom::the_dom_is_there_while_paused` |
| 每個 session 的編號各自從 1 起；`disable` 兩次是錯誤；最後一個打開 `DOM` 的 session 關掉或離開時 DOM 不再記改動 | `cargo test -p shell --test it devtools_dom::each_session_has_its_own_numbers`、`cargo test -p renderer --lib dom_agent::` |
| 和 Chrome 一樣：樹、`setChildNodes`、每個 task 一個改動時的事件、查詢、錯誤 | `python3 scripts/test.py cdp dom` |
| Console 的節點的物件是 `node`，名字照 Chrome（`div#a.x.y`、`#document`、`<!DOCTYPE html>`、`#text`）；物件的節點（連同到它的路）、節點的物件（照 nodeId 和 backend id，在要的 object group 裡）；錯誤的文字；`$0`；不在文件的樹裡的節點以 `parentId` 0 送它的子樹的根，script 放掉、GC 跑過之後 session 照樣留著 | `cargo test -p shell --test it devtools_dom::nodes_and_their_objects_are_linked` |
| 停住時也給得出節點的物件 | `cargo test -p shell --test it devtools_dom::the_dom_is_there_while_paused` |
| listener：capture 的在前、照型別最早加入的順序、屬性的 handler 先編譯、物件的 `handleEvent`、旗標、位置、物件包在要的 group、`depth` 以下的節點、window 的沒有節點 | `cargo test -p shell --test it devtools_dom::event_listeners_are_listed` |
| 搜尋：selector、文字、標籤（`<p>` 整個名字）、屬性的名字和值（`"code"` 整個值）；結果照要的範圍送出、範圍檢查、丟掉 | `cargo test -p shell --test it devtools_dom::the_document_is_searched` |
| 和 Chrome 一樣：Console 的節點、`requestNode`、`resolveNode`、`$0`、detached 的節點、listener、搜尋（只差 `HTMLDocument`，已知） | `python3 scripts/test.py cdp dom_objects` |
| 停住時 renderer 拿得到 layout、重算之後的 box 跟著 script 的改動：停在 `Runtime.evaluate` 的 script、`setTimeout`、`click()` 和 `focus()` 呼叫的 listener、`requestAnimationFrame`、ResizeObserver 的 callback（原型的 `Prototype.pausedPage`，DOM agent 做好之後換成它的命令） | `cargo test -p shell --test it devtools::prototype_the_page_is_lent_while_paused` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 原型（2026-11-09，macOS）

- **停住時借 `Dom`**：`Dom` 在 `dom_js` 的 isolate 的狀態裡，JS 呼叫 DOM API 時每次從那裡借，dispatch 事件時每呼叫一個 listener 之前也放開，所以停在 JS 的敘述時沒有 Rust 的程式借著它（`crates/bindings/soundness.md` 本來就這樣論證巢狀的 callback）。`bindings` 的 `run_message_loop_on_pause` 把 isolate 交給停住的迴圈，`dom_js` 經過 `isolate_state_mut` 借出 `Dom` 和 layout（`dom_api::PausedPage`）。`dom_js` 的測試停在四種地方都讀得到當時的 DOM。
- **停住時借 layout、重算**：layout（renderer 的 `Frame`）也在 isolate 的狀態裡（`LayoutCell`），照 operation 借它的方式 take、回答完 set 回去。`dom_api` 借 layout 的地方（`with_layout`、`with_style`、`lend_layout`）都不在借著時執行 JS，所以實際上一直拿得到。原型的命令重算 style 和 layout 之後讀 root 的 box，停在六種地方都跟著 script 加的 div 變高。script 中途的強制 layout（`getBoundingClientRect`）本來就走同一條路。
- **結論**：P1+ 不用重新安排 `RenderThread` 的擁有權，第 1 段不拆（「已決定的事項」第 5 條）。
- **`valueSubtype`、`wrap_object`、`unwrap_object`、`add_inspected_object`**：rusty_v8 的 binding 和上游的測試（`tests/test_api.rs` 的 `inspector_wrap_object`、`inspector_unwrap_object`、`inspector_value_subtype`）都在；machopo 這一側要加的是判斷 wrapper 是不是節點，實作時用測試確認。
- 原型的 `Prototype.pausedPage`（`renderer` 的 `render_thread/devtools.rs`，標著 `PROTOTYPE`）在 DOM agent 做好之後拿掉，測試換成 DOM agent 的命令。

### 1a 樹（2026-11-09，macOS）

- **新的測試**：`dom` 的 `probes`（1 個）；`shell` 的 `devtools_dom`（7 個）；`renderer` 的 `dom_agent` 的單元測試（1 個）；`test.py cdp` 加 `dom` 一組（`normalize` 把節點的編號照同一組佔位符換掉，`parentId` 是數字時是節點的編號，不是 frame 的 id）。
- **和 Chrome 比**：`test.py cdp dom` 一樣（Chrome for Testing 152.0.7977.82）。一開始在一個 evaluate 裡做好幾個改動，差了 40 處，都是一批一批送的差別（「已決定的事項」第 6 條：插入又拿掉的 span 不送、數量的更新在一批的最後、shadow root 帶著同一批裡加的文字）；改成每個 task 一個改動之後一樣，一批的行為由 `devtools_dom::a_batch_is_told_in_the_doms_order` 守著。其他的組（`values` 到 `log`）照舊，已知的差異沒有增加。
- **對照 Blink 修的**：`style` 屬性經過 CSSOM 改時只送 `inlineStyleInvalidated`，不送 `attributeModified`（一開始測試寫錯了，Blink 也只送前者）。
- **mutation test**（`mutants-1a.toml`，34 條）：第一輪抓到 29 條，漏了 5 條，都補了測試：
  - 同一個節點的子節點要第二次又送一次：`the_document_is_sent_as_asked_for` 加第二次 `requestChildNodes`；
  - `describeNode` 給它的節點編號：同一個測試加 `describeNode` 的 `depth`，沒送過的子節點是 0；
  - `getDocument` 留著舊的編號：同一個測試再 `getDocument` 一次，root 的編號不同、舊的找不到；
  - 移除的節點留著編號：`script_changes_are_told` 加移除之後 `getAttributes` 找不到；
  - 回答命令之前不先送改動：平常每個 task 結束時也會送，所以沒抓到；停住時沒有 task 結束，`the_dom_is_there_while_paused` 加停住時執行的 script 改了 DOM，緊接著的 `querySelector` 之前要先收到插入。
  補了之後重跑這 5 條，都抓到。
- **跑過的檢查**：`cargo test -p dom -p dom_api -p dom_js -p bindings -p renderer -p shell` 全部通過；`sanity.py deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 都通過。

### 1b 和 JS 的連接（2026-11-09，macOS）

- **介面的改變**：1a 的 `PausedPage`（帶著 `&mut Dom` 和 layout 的 struct）改成 `DevToolsPage` trait：停住時也要呼叫 inspector，struct 拿著 DOM 的借用就不能再經過 isolate 借。不停住時 renderer 的命令也走同一個 page（`ScriptEngine::devtools_page`）。`crates/bindings/soundness.md` 的論證跟著改。
- **新的測試**：`shell` 的 `devtools_dom` 加 3 個（共 10 個），`the_dom_is_there_while_paused` 加停住時的 `resolveNode`；`test.py cdp` 加 `dom_objects` 一組。
- **和 Chrome 比**：`test.py cdp dom_objects` 只差 `document` 的 `className`（machopo 沒有 `HTMLDocument`，記進 `KNOWN` 和 `docs/limitations.md`）；`dom` 照舊一樣。
- **和 Chrome 不同、照舊的**：屬性的 handler 編譯時沒有名字（`function (event) { go() }`，Chrome 是 `function onclick(event)`），本來就記在 `docs/limitations.md`；測試照 machopo 的寫。
- **mutation test**（`mutants-1b.toml`，30 條）：抓到 27 條，漏了 3 條：
  - `resolveNode` 不檢查節點還在：補了不存在的 backend id 的錯誤（原本落到「不屬於文件」的錯誤）；
  - 加引號的值當成一部分比對：搜尋的頁面加一個值裡有 `code` 的屬性，`"code"` 只找到整個值的那個；
  - 不是物件的值回答另一個錯誤：等價（V8 的 inspector 只給物件 `objectId`），記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。
  補了之後重跑那 2 條，都抓到；檔案都還原了。
- **跑過的檢查**：`cargo test -p dom -p dom_api -p dom_js -p bindings -p renderer -p shell` 全部通過；`sanity.py deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 都通過。

### 第 1 段做完時（2026-11-09，macOS）

- `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` 都通過（clippy 抓到的 4 處在這一段的程式裡修了：飽和的減法和加法、`as_chunks`、多餘的 `map_or_else`）。
- `test.py cdp` 每一組都照舊：`dom` 一樣，`dom_objects` 只差已知的 `HTMLDocument`，其他組的已知差異沒有增加。
- 還沒做的：在 Chrome for Testing 152 的 DevTools 裡手動看過 Elements 面板（樹、改動、`$0`、Event Listeners、搜尋、停在斷點時）；原型的 `Prototype.pausedPage` 留到第 2 段的 `getBoxModel` 做好時換掉。
