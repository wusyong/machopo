# RFC 0091 的進度：DevTools 的 Elements：`DOM`、`CSS` 和 `Overlay`

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-09，macOS（Apple Silicon）。在 `0091` 上做。

## 步驟


| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 原型：停住時借 `Dom` 和 layout、停住時重算 style 和 layout（第 5 節） | 完成 | 2026-11-09 |
| 1a 樹：nodeId、lazy push、改動的掛點和事件、停住時回答、不用 JS 的查詢（第 1 到 5 節） | 完成 | 2026-11-09 |
| 1b 和 JS 的連接：`resolveNode`、`requestNode`、`$0`、Console 顯示節點、Event Listeners、`performSearch`（第 6 節） | 完成 | 2026-11-09 |
| 2a Overlay：框節點、選取元素、`getBoxModel`、`getContentQuads`、`getNodeForLocation` | 完成 | 2026-11-09 |
| 2b Computed：`CSS.enable`、`getComputedStyleForNode`、`getPlatformFontsForNode`、元素自己的規則、跟著更新、標籤的樣式 | 完成 | 2026-11-09 |
| 2c 停住時照樣畫、「Paused in debugger」：scheduler、畫 frame 的狀態搬進 `Frame` | 完成 | 2026-11-09 |
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
| 停住時 renderer 拿得到 layout、重算之後的 box 跟著 script 的改動：停在 `Runtime.evaluate` 的 script、`setTimeout`、`click()` 和 `focus()` 呼叫的 listener、`requestAnimationFrame`、ResizeObserver 的 callback 時 `getBoxModel` 的高度（1a 是原型的命令，2a 換成 `getBoxModel`） | `cargo test -p shell --test it devtools_overlay::box_models_are_there_while_paused` |
| 盒模型：四個框的 quad（margin 在 border 外）、border box 的大小；捲動、fixed、旋轉、iframe 裡的；行內元素（Ahem）；content quad；沒有 box 的；`isScrollable` | `cargo test -p shell --test it devtools_overlay::nodes_have_box_models` |
| 某一點的節點：元素、文字的元素、iframe 裡的（和它的 frame）、user-agent shadow tree 的歸到 host（要的時候不歸）、viewport 外的找不到、捲動之後照文件的座標 | `cargo test -p shell --test it devtools_overlay::nodes_are_found_at_points` |
| overlay：要先 `DOM`；沒有 config 是 invalid params；照要的顏色畫出四個框（每個框只畫它和裡面那個框之間的一圈，2b 改）、標籤畫在下面、不要資訊時沒有標籤；跟著 script 移動的元素；`hideHighlight`；`getHighlightObjectForTest` 的路徑、顏色、外框、`elementInfo` | `cargo test -p shell --test it devtools_overlay::highlighted_nodes_are_drawn` |
| 選取元素的模式：未知的模式是錯誤；滑鼠移動框出底下的元素、送 `nodeHighlightRequested`；按下送 `inspectNodeRequested`；頁面收不到；關掉之後框拿掉、頁面收得到 | `cargo test -p shell --test it devtools_overlay::the_inspect_mode_takes_the_mouse` |
| 不畫的 overlay：要求不畫時成功，要求畫時是錯誤 | `cargo test -p shell --test it devtools_overlay::other_overlays_are_answered` |
| 和 Chrome 一樣：絕對定位和旋轉的元素的盒模型、content quad、highlight 的資料、某一點的節點、`isScrollable`、錯誤（只差 accessibility 的欄位、`layoutObjectName` 和錯誤的 data 裡的位置，已知） | `python3 scripts/test.py cdp overlay` |
| 「說明」：元素符合的規則照 cascade 的順序（UA 的在前、`!important` 的在它一般的位置）、各帶符合的選擇器（pseudo element 的不算、bloom filter 有祖先）、presentational hint 和 inline style 分開；shadow tree 的、host 的 `:host`、slot 的 `::slotted()`；adopt 兩次的規則列兩次；不在樹上的、別的文件的、文字節點沒有；不留下東西 | `cargo test -p style --test it explain::` |
| DevTools 列的選擇器（文字、specificity）和宣告（值、priority、implicit、設定它的 shorthand） | `cargo test -p style_resolver --test it -- selectors_are_listed declarations_are_listed` |
| 字型：照第一次出現的順序、問的 run 的 glyph 數、`name` table 的名字 | `cargo test -p layout --test it platform_fonts::` |
| `CSS` 要先 `DOM`、`enable` 之前和 `disable` 之後不回答 | `cargo test -p shell --test it devtools_css::css_needs_dom_and_to_be_on` |
| computed：`getComputedStyle` 的每個 property（數量一樣）、layout 決定的值、自訂 property、文字節點的是父元素的、`display: none` 的、跟著 script、沒有 frame 的沒有 | `cargo test -p shell --test it devtools_css::computed_styles_are_read` |
| 套用的規則：順序、符合的選擇器、specificity、宣告和 shorthand、`!important`、inline style、presentational hint、空的 inline style、layout 上的父元素（跳過 `display: contents`）、`:host`、錯誤 | `cargo test -p shell --test it devtools_css::matched_rules_are_listed` |
| 字型：元素自己、子元素的文字、產生的內容；不算孫元素的；文字節點的 | `cargo test -p shell --test it devtools_css::fonts_are_counted` |
| 跟著 computed style：自己的改動、繼承的改動送事件；別的節點不送；停了之後不送 | `cargo test -p shell --test it devtools_css::computed_style_updates_are_told` |
| 標籤的樣式：顏色一律 `#RRGGBBAA` 和分數、字型、padding、margin、背景；沒有文字的元素沒有顏色和字型；畫出那幾列（黑色的值） | `cargo test -p shell --test it devtools_css::labels_show_styles`、`cargo test -p renderer --lib overlay_agent` |
| 停住時的 computed 和規則照 script 到那時為止的改動 | `cargo test -p shell --test it devtools_css::styles_are_there_while_paused` |
| 和 Chrome 一樣：套用的規則（作者的照選擇器、specificity、符合的選擇器、longhand 的值；UA 的一條完整比）、inline style、presentational hint、layout 上的父元素、回答的 key、computed 的值、字型、`computedStyleUpdated`、標籤的樣式、錯誤（只差 `!important` 寫在值裡，已知） | `python3 scripts/test.py cdp css` |
| main thread 的 scheduler：每個 queue 的 task 照送進來的順序；停住時只取 DevTools 和 rendering 的（main frame、`RequestVisualState`），其他的（Browser 的訊息、輸入、canvas）留著、不亂順序；等的時候看得到之後送來的、有 timeout 的會放棄；要打斷 script 的照舊單獨取出；停住留下的放回最前面；main thread 走了之後送不進去 | `cargo test -p renderer --lib task_queue` |
| 停住時照樣畫：Console 的改動、框的節點、拿掉的框；`setPausedInDebuggerMessage` 的提示（頁面蓋上一層、上方中間的淡黃色框），`Overlay` 關著的 session 的不畫、拿掉之後沒有；繼續之後照常畫 | `cargo test -p shell --test it devtools_overlay::the_page_is_drawn_while_paused` |
| 停住時的輸入等到繼續才處理 | `cargo test -p shell --test it devtools_overlay::input_waits_for_the_pause` |
| 停住時滾輪在 compositor 上捲動：停住時畫的 frame 留著捲動的位置、script 讀得到 | `cargo test -p shell --test it devtools_overlay::a_scroll_while_paused_stays` |
| scheduler 和 widget 搬家之後，視窗裡的情境和 pipe 照舊 | `python3 scripts/test.py automation` |

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

### 2a Overlay（2026-11-09，macOS）

- **新的測試**：`shell` 的 `devtools_overlay`（7 個；1a 的原型測試換成 `getBoxModel`、搬到這裡）；`test.py cdp` 加 `overlay` 一組。原型的 `Prototype.pausedPage` 拿掉了。
- **和 Chrome 比**：`overlay` 只剩已知的差異（`elementInfo` 的 accessibility 欄位和 `layoutObjectName`、invalid params 的 data 裡的位置）。一開始比的時候：
  - 行內的 `span` 差很多，都是它前面的文字和行高（兩邊的字型不同），不是 inspector 的；從比對拿掉，改在 `shell` 的測試用 Ahem 驗。
  - `getNodeForLocation` 點在 (5, 430)：Chrome 找不到、machopo 找到 `<html>`。先猜是根元素的框外面就沒有節點，用 Chrome 試過之後是 viewport 外面就沒有（Chrome headless 的 viewport 只有 413 高）；照 Chrome 改，比對改用兩邊都在 viewport 外的點。
  - `highlightNode` 沒有 config：Chrome 是協定那一層的 `-32602`，照它改。
- **找到的 bug**：`hideHighlight` 之後框不拿掉：`layer()` 用 `?` 把「現在沒有 overlay」當成「沒有變」（`highlighted_nodes_are_drawn` 抓到）。`outlineColor` 一開始只看 alpha，Blink 是和整個透明色比（`rgba(128, 0, 0, 0)` 照樣送）。
- **mutation test**（`mutants-2a.toml`，28 條）：抓到 26 條，漏了 2 條：
  - viewport 外的點照樣找：原本測的點下面什麼都沒有，改用捲到之前的 `#far`，重跑抓到；
  - overlay 不要 frame：截圖自己就要一個 main frame，看得到 overlay 的測試都會蓋過它；不是等價的（框要等別的原因要的 frame 才畫），補不了測試，記進 `docs/test-gaps.md` 的「只靠審查」。
- **不穩定的測試**：跑 `shell` 全部時 `modules::subresources_from_another_origin_go_by_cors` 約五次失敗一次（畫面是紅的）。讓頁面印出收到的事件，失敗那次 `log` 少了 `inline-anon`：頁面等 14 個 `load`、`error` 都到了就檢查，但 inline 的 module 沒有事件可等，偶爾在最後一個事件之後才執行。和 DevTools 無關；讓它執行時也算一項，之後連跑 30 次都過。
- **跑過的檢查**：`cargo test -p layout -p renderer -p shell` 全部通過；`test.py cdp` 每一組照舊；`sanity.py` 的 7 項都通過。

### 2b Computed（2026-11-09，macOS）

- **開始前定的事**：讀 devtools-frontend 的 `ComputedStyleWidget.ts`，Computed 要 `getMatchedStylesForNode` 的結果（null 時整個是空的，預設的清單也照它）：把「說明」的路裡元素自己的部分提前（「已決定的事項」第 14、15 條）。
- **新的測試**：`style` 的 `explain`（4 個）；`style_resolver` 的 `selectors_are_listed_with_their_specificity`、`declarations_are_listed_for_devtools`；`layout` 的 `platform_fonts`（2 個）；`shell` 的 `devtools_css`（7 個）；`renderer` 的 `overlay_agent::tests`（2 個）；`test.py cdp` 加 `css` 一組。
- **和 Chrome 比**：只剩已知的一項（作者的規則 Chrome 照原文寫值，`red !important`）。一開始比的時候：
  - 作者的規則 Chrome 有原文（位置、照寫的 shorthand、`text`），和 machopo 沒有原文的形狀比不了：比對改成比選擇器、specificity、符合的選擇器和 longhand 的值，另外完整比一條 UA 的規則（兩邊都沒有原文），形狀一樣。
  - 照 Chrome 改：沒有 `style` 屬性也送空的 `inlineStyle`；加 `parentLayoutNodeId` 和 `cssAtRules`，拿掉 `cssFunctionRules`、`cssPositionTryRules`；`shorthandEntries` 的 `important` 只在 true 時送；標籤的樣式加 `-unclamped-rgba`。
  - 字型的名字是空的：macOS 的 Times.ttc 只有 Macintosh 的 `name` 紀錄，加上 Macintosh 的英文那一層。Chrome 的預設字型照系統語言（這台是 PingFang TC），比對的頁面改用 Times。
  - 比對本身的錯：拿 `#x` 的文字節點時，第一個 `setChildNodes` 是 `querySelector` 送路時的，改用 `describeNode` 和 `pushNodesByBackendIdsToFrontend` 拿。
- **找到的 bug（2a 的）**：標籤畫好之後和 Chrome 的截圖並排，machopo 的 margin 蓋住了整個元素，Chrome 是一圈一圈的。`inspector_overlay` 的 `drawHighlight` 每畫一個框就挖掉裡面那個（`destination-out`）；machopo 改用 even-odd 的路徑只畫那一圈。2a 的 `highlighted_nodes_are_drawn` 把錯的畫法寫成預期（border 那一圈是 margin 的顏色），改成四個框各自的顏色。標籤也照 `createElementDescription` 改：大小靠右、列的順序是 Color、Font、Background、Margin、Padding，顏色照 `colorFormat` 寫（Blink 一律送 `#RRGGBBAA`，前端的 overlay 頁面自己換格式，原本照 `colorFormat` 送是錯的）。
- **mutation test**（`mutants-2b.toml`，39 條）：第一次抓到 32 條，漏了 7 條：
  - 另一個文件的元素也說明、字型照反的 tree order、標籤的值沒畫：測試沒走到，補了測試（另一個文件的 `<html>`、兩個 block 的字型、標籤裡黑色的像素）。
  - 每個 frame 都說 style 換了、停了之後還跟著：測試的「兩個 frame 之後」沒有 `awaitPromise`，其實沒等；改成真的等兩個 frame，抓到。
  - 「說明」設 selector 的 flag：等價（recalc 已經設過一樣的），記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。
  - 只有 Macintosh 名字的字型：測試的字型都有 Unicode 的名字，只有 `test.py cdp css` 在 macOS 上守著，記進 `docs/test-gaps.md` 的「只靠審查」。
  - 重跑：37 條抓到，漏的是上面兩條。
  - 另外「同一個選擇器記兩次」第一次跑（停在第一個漏的）沒抓到：補了 adopt 兩次的測試，之後抓到。
- **跑過的檢查**：`cargo test -p style -p style_resolver -p layout -p dom_api -p dom_js -p renderer -p shell` 全部通過；`test.py wpt css/cssom` 沒有不預期的結果（`getComputedStyle` 借 layout 的方式改了）；`test.py cdp css overlay dom dom_objects` 只有已知的差異；`sanity.py` 的 7 項都通過。

### 2c 停住時照樣畫（2026-11-09，macOS）

- **開始前定的事**：「已決定的事項」第 16 條（Q1：main thread 的 scheduler）、第 17 條（S3：畫 frame 的狀態搬進 `Frame`）。原本建議的 F1（另一個格子）使用者不想多一把鎖，問停住的迴圈是不是設計問題：巢狀的迴圈本身躲不掉，問題在 task 分在兩個地方，所以改成一個 scheduler。
- **新的測試**：`renderer` 的 `task_queue` 的測試（4 個，取代原本 DevTools 佇列的 2 個）；`shell` 的 `devtools_overlay::the_page_is_drawn_while_paused`、`input_waits_for_the_pause`、`a_scroll_while_paused_stays`。
- **找到的 bug**：停住時截圖偶爾等不到（六次失敗一次）：`RequestVisualState` 要了 frame，停住的迴圈等下一個之前（`waiting`）看到 DOM 和 overlay 都沒變，又把 frame 取消了。等著 frame 的 promise 也算要畫，之後連跑二十次都過。
- **以為是 bug、其實不是**：測試停住時的輸入，按鈕一直沒收到 click，沒停住時也一樣：還沒畫過任何 frame 時 compositor 不知道輸入是哪份文件的（`document` 是 `None`），renderer 照 Chrome 丟掉對著別的文件的輸入。測試先截一張圖。
- **和 Chrome 比**（Chrome for Testing 152）：停住時 Console 的改動、框的節點照樣畫（RFC 寫的）；停住時 CSS 動畫照樣往前走（`margin-left` 的動畫，半秒走了將近兩百 px），machopo 不推進，記成限制（「已決定的事項」第 18 條）；停在 rAF 的 callback 裡時 Chrome 的截圖也不回答（machopo 一樣）。`test.py cdp` 的每一組照舊（相同或已知）。
- **mutation test**（`mutants-2c.toml`，18 條）：第一次抓到 10 條、1 條卡住（「有 timeout 的等永遠等下去」：測試卡住，算抓到），漏了 7 條：
  - 停住時也取輸入：那條只跑 scheduler 的測試，測試裡沒有輸入；加進去，抓到。
  - 停住時不套用 compositor 的捲動：沒有測試在停住時捲動；補了 `a_scroll_while_paused_stays`，抓到。
  - 沒開 `Overlay` 的 session 的提示也畫：`disable` 整個拿掉 session，提示跟著沒了；補了關掉之後再給提示的情況，抓到。
  - 停住時 DOM 改了不要 frame、新的提示不要 frame、不記畫過的 tree version：截圖自己就要 frame，蓋過了這幾條，和 2a 的「overlay 要 frame」同一列記進 `docs/test-gaps.md` 的「只靠審查」。
  - 停住而 layout 被借出去時照樣畫：現在走不到，記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。
  - 重跑：12 條抓到、1 條卡住，漏的是上面記下的 4 條；補了 `Overlay` 關掉的情況之後那一條也抓到。
- **第 2 段做完時的檢查**：`cargo test --workspace` 全部通過；`cargo fmt` 之後 `--check` 沒有差異；`cargo clippy --workspace --all-targets -- -D warnings` 修了兩處（測試沒用到的變數、`frame()` 不用寫出的 lifetime）之後通過；`test.py automation`：第一次 60 個情境過了 37 個，前面連續 23 個說沒畫出 frame（「is the window hidden?」），同樣的 commit 之前的版本照跑全過、那幾個重跑也過、整套重跑 60 個全過，pipe 的測試也過，是那時的顯示器（第一次跑時視窗畫不出來），不是程式；`sanity.py` 的 7 項都通過。
