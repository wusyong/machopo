# RFC 0110 的進度：renderer 和 engine 的狀態只有一個擁有者

開始日期：2026-11-23

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0a. 盤點：副本、便條、外圈的收尾、共享的 `Rc`、`RefCell`，對照 Blink 和 content 層 | 做完，待審查（標「沒對」的幾項沒對原始碼） | 2026-11-23 |
| 0b. 原型：`fetch()` 的登記，做法一 | 做完，待審查 | 2026-11-23 |
| 0c. 原型：`fetch()` 的登記，做法二 | 做完，待審查；換 engine 時會 panic | 2026-11-23 |
| 0c'. 獨立的審查；原型：`fetch()` 的登記，做法三 | 做完，待審查 | 2026-11-23 |
| 0d. 交給使用者決定做法和範圍 | 做完：做法第 5 條，DevTools agent 第 6 條，範圍第 7 條 | 2026-11-23 |
| 1. 請求和 loader 交給 `dom`（已決定的事項第 7 條的第 1 步，第 10 條） | 做完，待審查 | 2026-11-23 |
| 2. frame 的表只有一份，建立和移除當場做完（第 7 條的第 2 步，第 11 條） | 進行中 | |
| 2a. 必壞的測試：A1、A5；移除 iframe 時 parent 的 `load` 和子 frame 的 unload（先對照 Chrome） | A1、A5 的做完，待審查；移除的兩個留到 2e | 2026-11-23 |
| 2b. renderer 向 engine 查 frame 的 token、parent、owner；拿掉 `ChildFrames` 的樹和 token、`FrameDocument.child_of`；WebDriver 和 DevTools 的 frame 清單讀 engine（A5、A1） | 做完，待審查（兩條 mutation 沒抓到，見做過的檢查） | 2026-11-23 |
| 2c. 建立和移除時 child view 和給 Browser 的訊息由 hook 當場做 | 做完，待審查 | 2026-11-23 |
| 2d. 文件的載入狀態（readiness、還在等的載入）搬到 DOM，「載完了沒」的判斷移到 `dom_api` | 做完，待審查（第 13 到 16 條；兩條 mutation 沒抓到，見做過的檢查） | 2026-11-23 |
| 2e. 移除時 engine 當場發子 frame 的 `pagehide`、`unload`，檢查 parent、排它的 `load` | 沒開始（開始前列出要決定的事） | |
| 2f. 初始的 `about:blank` 不先建 `FrameDocument`；拿掉 `Created`、`Removed` 的便條 | 沒開始 | |
| 3. 同一份文件內的捲動當場做，拿掉 renderer 那份文件網址的副本（第 7 條的第 3 步） | 沒開始 | |
| 4. `javascript:` 網址在 `dom_api` 裡排成綁在 Document 上的 task（第 4 步） | 沒開始 | |
| 5. intersection 的通知排成真的 task（第 5 步） | 沒開始 | |
| 6. 圖片和 script 開始載入的記錄交給 `dom`（第 6 步） | 沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 盤點 | `details.md` 的表；不改程式，所以沒有測試 |
| 原型 | 各自的 patch 在 `prototypes/`；A2、A3 的放大測試必過，比較改動、借用、`unsafe` |

## 做過的檢查

### 第 0 階段（2026-11-23，Linux：Fedora 45、debug build，`87fc238` 的複本）

**盤點**：照程式讀；Blink 對了 152.0.7977.82 的十一個檔（`details.md` 的「參考資料」），標「沒對」的沒有對。

**原型**（patch 在 `prototypes/`；編譯用 `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`，`MACHOPO_GPU_ADAPTER=cpu`）：

| | A2 的必壞測試 | A3 的必壞測試 | `devtools_network` | `renderer` | `shell` 整組 | `browser` | `dom_js` 整組 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 沒改（`tests.diff` 而已） | 3 次壞 3 次 | 3 次壞 3 次 | | | | | |
| V0 | 5 次壞 0 次 | 5 次壞 0 次 | 12 個通過 | 128 個通過 | 643 個通過 | 191 個通過 | 550 個通過，1 個 ignored |
| V1 | 5 次壞 0 次 | 5 次壞 0 次 | 12 個通過 | 128 個通過 | 643 個通過 | 191 個通過 | （和 V0 相同的 `dom_js`） |
| V2 | 5 次壞 0 次 | 5 次壞 0 次 | 12 個通過 | 128 個通過 | 643 個通過 | 191 個通過 | （和 V0 相同的 `dom_js`） |
| V3 | 5 次壞 0 次 | 5 次壞 0 次 | （在 `shell` 整組裡） | 128 個通過 | 643 個通過 | 191 個通過 | 550 個通過，1 個 ignored；`dom` 254 個通過 |

**換 engine 時 unload 裡的 `fetch()`**（`tests-unload.diff`，審查之後加的）：沒改的、V0、V1 各 3 次都通過；V2 3 次都在 sink 的 `expect`（`documents.rs:5114`）panic，panic 在不能 unwind 的地方，測試的 process 中止；V3 5 次都通過。V1 的請求編號從 1 重來，這個測試看不出來（讀程式確認）。

- `shell` 整組包括 `wasm_documents`：V1 裡 wasm 的 engine 沒有 sink，狀態留在 render thread 的那條路有跑到。
- 原型不是要合併的程式，所以沒有 mutation test，也沒有跑 clippy 和 WPT；效能沒有量。
- patch 照順序套回 `87fc238` 之後，和複本裡的 branch 一樣（V2 的新檔要 `--index`）。
- 做法二的第一版（有 `Drop` 的 guard）沒有編過，見 `details.md` 的「原型」；沒有留它的 patch。
- **審查**：獨立的 subagent 唯讀地審了盤點和三個原型（`details.md` 的「審查」）。它的主要說法我對過程式；V2 的 panic 用上面的測試重現了。
- **第二次審查**：另一個 subagent 唯讀地審了 V3、做法三用在 frame 的樹上和範圍「乙」（`details.md` 的「第二次審查」），沒有執行任何東西；它的關鍵說法我對過程式。

### 第 1 步：請求和 loader 交給 `dom`（2026-11-23，Linux：Fedora 45、debug build，`9e7d93d` 之上）

照原型 V3（`prototypes/v3-dom-owns.diff`）做，另外：拿掉 V0 給 V1、V2 用的 `ScriptEngine::load_sink_mut` 和 `LoadSink::as_any_mut`；拿掉 `FrameRequest` 的 `Fetch`、`FetchSent`、`CancelFetch`（已決定的事項第 10 條），送不出去的 `fetch()`（撤銷了的 blob URL）改由 sink 排一個 `MainThreadTask::FetchFailed`；取消是 `LoadSink::cancel_fetch`；文件的 loader 和帶來它的導覽記在 DOM（`Cx::document_loader`，子 frame 初始的 `about:blank` 走它的 creator 的），cookie 和 worker 的 hook 由 engine 帶 loader，renderer 的 `DocumentLoaders` 和 `FrameDocument.loader_id` 拿掉；頂層的文件換掉時（同一個 engine，和換 engine）取消它們還在路上的 `fetch()`。`sanity.py boundary` 的 `STATICS_ALLOWED` 加了每個 thread 的請求編號。

**新的測試**：`shell` 的 `devtools_network::a_fetch_is_told_as_it_is_sent_before_its_redirect`（A2）、`a_fetch_is_stamped_as_it_is_sent`（A3）、`a_replaced_documents_fetches_are_cancelled`（同一個 engine 和換 engine）、`documents::a_new_frames_fetch_goes_through_its_creators_loader`、`wasm_documents::a_js_documents_unload_may_fetch_as_a_wasm_document_replaces_it`；`dom_js` 的 `fetch.rs` 改用測試的 load sink（`Sent`、`Sink`），`tasks.rs`、`devtools.rs` 跟著改。新的四個 `shell` 測試各跑 3 次都通過。

**一個量不出東西的測試**（沒有留下）：原本用 TestServer 看 `fetch()` 的請求有沒有被放棄來驗證取消，但 `run`、`render` 回來之後 machopo 就結束、關掉每一條連線，沒有取消照樣被記成放棄（拿掉取消時兩個測試都還是通過）。改成 DevTools 的 `loadingFailed`（`canceled`）：拿掉任一條路的取消，請求都照樣完成（`loadingFinished`），測試失敗。這也確認了「頂層的文件換掉時不取消」是原本就有的問題（`docs/limitations.md` 那一條刪了）。

**Mutation test**（`mutants-1.toml`）：16 條都抓到。`a cookie set through no loader` 第一次只跑 `shell`，沒抓到；寫入再讀回的測試在 `browser` 的 `cookies::document_cookie_and_cookie_enabled`，spec 改成跑它之後抓到。跑完檔案都還原了（`target/mutate/originals` 是空的）。

**測試**：`dom` 254 個、`dom_api` 201 個、`dom_js` 550 個（1 個 ignored）、`renderer` 128 個、`browser` 191 個、`shell` 649 個都通過（加上新的測試之後 `shell` 是 650 個，新的那一個另外跑過）。WPT：`fetch/api/basic/`、`abort/`、`redirect/`、`cors/`、`cookies/`、`document-cookie.html`，3742 項，非預期的 0 個。`sanity.py` 的 deps、spans、boundary、unsafe、tests、numerics、docs 通過。`cargo test --workspace`、`cargo fmt --check`、clippy 照規定在整個階段做完時跑。

### 第 2a、2b 步：必壞的測試；renderer 向 engine 查 frame（2026-11-23，Linux：Fedora 45、debug build，`56cb7e1` 之上）

**2a**：
- A5（`automation_pipe::a_removed_frame_is_gone_for_the_next_command`）：頁面的 script 移除 iframe，用 `postMessage` 排一個忙 300 ms 的 task，下一個 WebDriver 指令在 renderer 處理 `Removed` 之前進來。只在 timer 裡移除、或在 microtask 裡忙，都不會壞（回覆在 microtask checkpoint 之後才送）。改之前 10 次壞 10 次，症狀和 A5 一樣（`javascript error`，不是 `no such window`）。
- A1（`devtools_css::a_new_frames_sheet_is_told_though_it_is_drawn_as_it_commits`，0109 的 `det-test.diff`）：加暫時的 log 量到 rendering 時 sheet 的版本有子 frame 的 srcdoc 文件，DOM agent 的清單卻還是它初始的 `about:blank`。改之前 10 次壞 10 次（0109 量時 9 次壞 5 次：第 1 步之後時序變了）。
- 移除 iframe 時 parent 的 `load` 和子 frame 的 unload 對照 Chrome 的兩個，留到 2e 改到它們時寫。

**2b**：`ScriptEngine` 加了 `frame_token`、`frame_of_token`、`frame_parent`、`child_frames`（dom_js 照它的 `Frames` 答，`dom_wasm` 用預設的：只有頂層的 frame）；`FrameRequest::Removed` 帶 parent 和一起拿掉的每個 frame（`gone`）；renderer 拿掉 `ChildFrames` 的樹和 token、`FrameDocument.child_of`，各處改問 engine（`child_token`、`child_of_token`、`child_parent`、`child_frames_below`、`place_of`），WebDriver 的 frame 和 DevTools 的 frame 清單也是；rendering 列 sheet 時用當下算的 frame 清單（`CssAgent::sheets_updated` 的 `frames`；debugger 暫停時照舊是 DOM agent 的）。A1、A5 的測試改之後各 5 次都通過。`dom_js` 的 `frames::a_frame_goes_with_its_token` 加了 engine 的查詢和 `gone` 的檢查。

**Mutation test**（`mutants-2b.toml`）：8 條，6 條抓到。`a Removed names the frame alone` 第一次沒抓到：換文件時的測試沒有巢狀的 frame，補了一層之後抓到。沒抓到的兩條：
- `a gone frame taken for the top-level one`（`place_of`）：要走到，iframe 移除之後它的文件還得載完或中止；移除之後文件不再 fully active，它的 window `load` task 不跑，被丟掉的導覽在 `navigation_dropped` 一開始就因為查不到 frame 返回。推論為走不到的路，等價；沒有證明。
- `a navigated child's frames before kept by the renderer`（`commit_child` 當場清掉舊文件裡 frame 的帳）：engine 在 `create_window` 時照樣為它們送 `Removed`，下一次處理便條時清掉，差別只在中間的空檔；空檔裡舊文件的 parser 會不會被續跑沒有排除。2f 拿掉便條、重新設計清帳時一起處理，補測試。

跑完檔案都還原了。**測試**：`dom_api` 201 個、`dom_js` 550 個（1 個 ignored）、`renderer` 128 個、`browser` 191 個、`shell` 652 個都通過。

### 第 2c 步：給 Browser 的訊息和 child view 當場做（2026-11-23，Linux：Fedora 45、debug build，`3e21253` 之上）

使用者選了新的 `FrameClient`（Blink 的 `LocalFrameClient`：`CreateFrame`、`Detached` 都在它上面，content 的 `RenderFrameImpl` 當場送 `CreateChildFrame`、`Detach`），不加在 `LoadSink` 上。

**做的事**：`dom_api` 加 `FrameClient`（`child_frame_created`、`frame_detached`）和 `ScriptEngine::set_frame_client`（預設不做事：wasm 的 engine 沒有子 frame）；dom_js 在 `create_child` 帶 parent 的 token（最上層的是 Browser 給的，`set_main_frame_token`）呼叫它，`Frames::tell_removed`（iframe 拿掉、文件換掉）每個直接的子 frame 呼叫一次。`FrameLayout` 加 `child_frame_removed`，`discard_child` 當場呼叫，renderer 拿掉 child view；文件換掉時 child view 跟著換掉的 view 走，不另外呼叫。renderer 的 `RendererFrameClient` 送 `DidCreateChildFrame`、`DidDetachFrame`，`Created`、`Removed` 的處理不再送這兩個、不再動 child view；`Created` 補建 child view 的那一段換成 `debug_assert`（layout 從第一份文件起在 engine 裡，DevTools 暫停時拿走的在執行 script 前放回；整組測試沒有碰到）。

**連帶改的**：WebDriver 的 script 在它自己的 frame 被拿掉之後才交回結果時（`parent.frameElement.remove()`），以前 child view 晚一點才拿掉，結果照樣送回；view 當場拿掉之後變成 `javascript error`（`webdriver::frames_are_switched_to_and_from` 抓到）。照 WebDriver 規範，script 跑完後不再檢查 browsing context，所以改成：沒有 context 時，結果裡不指向文件的值照樣交回，只有 element 給原來的錯誤（`value_out` 的 context 改成 `Option`）。

**Mutation test**（`mutants-2c.toml`）：7 條，第一次 6 條抓到。`the browser not told of a frame gone` 沒抓到：Browser 的 `frame_tree.remove` 沒有網頁看得到的效果，補了 `browser_view::the_browser_hears_of_a_removed_frame`（看 renderer 送出的訊息：拿掉的 frame 的 `DidDetachFrame` 帶著建它時的 token），之後抓到。跑完檔案都還原了。

**測試**：`dom_js` 551 個（1 個 ignored）、`renderer` 128 個、`shell` 653 個、`dom_api` 201 個、`browser` 191 個都通過。

### 第 2d 步：文件的載入狀態在 DOM，「載完了沒」是 `dom_api` 的（2026-11-23，Linux：Fedora 45、debug build，`693e686` 之上）

逐條決定（已決定的事項第 13 到 16 條；第 14 條實作前從 B 更正成 B'：`Loaded` 不能從 `readyState` 推，renderer 在 iframe 的 `load` 分派之後才設）。

**做的事**：`dom` 的文件資料加了 host 替它抓、會擋 `load` 的請求（`add_load_request`、`remove_load_request`、`has_load_requests`，照編號）、`DOMContentLoaded` 發過了（`document_ready(ContentLoaded)` 設）、`load` 排了（`queue_window_load` 設）、host 結束了它的載入（`set_load_ended`）、它的 frame 的導覽在路上（`set_frame_navigating`，記在子 frame 的文件上：commit 換來新的文件，frame 拿掉時文件跟著不在，不會留下舊的旗標），`load_progress` 從這些和 `readyState` 推出五個值。`dom_api` 加了 `check_completed`（`LoadCheck`：`Queued`、`Waits`、`WaitsForImages`）。renderer 拿掉 `Readiness` 和 `readiness`、`ChildFrames::loading`、`children_loading`；`finish_load` 先送 `<link>`，呼叫 `check_completed`，只剩圖片在等時送出圖片的請求、再呼叫一次；`<link>` 的 sheet 開始載入、收到回應時同步 DOM 的請求；子 frame 的導覽和排著的 commit 每次改動之後同步旗標（`sync_child_navigating`）；`document_loaded` 和中止時設 `set_load_ended`。

**和第 16 條說的不同的地方**：原本寫 renderer 照舊先更新 style、送圖片，再呼叫 `check_completed`。那樣每次 `finish_load` 都會更新 style，連還在 parse 的文件也是；改成 `check_completed` 先看前提、通過了才更新 style，回 `WaitsForImages` 時 renderer 送圖片、再問一次，和原本的順序一樣。

**順帶修的 2c 的錯**：同一個 call 裡建立又拿掉的 iframe，view 在拿掉時當場拿掉了，renderer 之後處理它的 `Created` 時 2c 加的 `debug_assert` 失敗（WPT 的 `initial-empty-document/iframe-nosrc.html` 等 4 個 crash，2c 的整組測試沒有這種情況）。`Created` 遇到 engine 已經沒有的 frame 時整筆跳過；補了 `automation_pipe::a_frame_made_and_removed_in_one_script_leaves_nothing`。

**WPT**：`html/semantics/embedded-content/the-iframe-element/`、`html/semantics/document-metadata/the-link-element/`、`html/syntax/parsing/`、`html/dom/documents/`、`html/browsers/browsing-the-web/`、`html/webappapis/scripting/` 267 個，0 個非預期的結果（修掉上面的 crash 之後）；`html/semantics/scripting-1/`、`html/semantics/embedded-content/the-img-element/` 有 7 個非預期的 PASS（`script-crossorigin-network.sub.html` 的 6 個、`module/credentials.sub.html` 的 1 個），在 `main`（`9e7d93d`）上同樣通過，和這份 RFC 無關，沒有改預期結果（CI 上照 FAIL 寫，可能和這台機器有關）。

**Mutation test**（`mutants-2d.toml`）：第一次用 900 秒的上限，會讓 `load` 永遠不發的 mutant 每條要等滿，停掉改成 240 秒（停掉時留下的改動用 runner 存的原檔還原、確認過）。12 條，第一次 6 條抓到、2 條 hung（算抓到）、4 條沒抓到：
- `an initial about:blank not loaded`：等價：初始的 `about:blank` 的 `readyState` 一建好就是 complete，最後的 fallback 本來就是 `Loaded`。拿掉多餘的 `initial_about_blank` 判斷，mutant 改成弄壞那個 fallback，之後抓到。
- `a dropped child navigation still delays load`：補了 `documents::a_dropped_child_navigation_delays_no_load`（iframe 導覽到沒有 handler 的 scheme，Browser 丟掉；先試的 204 在 machopo 會 commit 一份文件，不是丟掉），之後抓到。
- `an aborted document's load never ends`：補了 `documents::a_child_whose_body_stops_short_delays_no_load`，仍然沒抓到：中止的文件 `readyState` 是 complete、沒有排 `load`，fallback 照樣是 `Loaded`；這個 bit 只在中止時 interactive 的 `readystatechange` 的 handler 裡有差（B' 的用意），現在那時沒有人判斷。2e 之前等價，2e 讓 engine 在 handler 裡判斷時補測試。
- `load not checked again once the images are asked for`：補了 `documents::a_revoked_blob_urls_background_delays_no_load`，仍然沒抓到：拿掉時 `load` 延到下一個 task 跑完（`run_due_tasks` 的 `finish_loads`），測試的頁面之後總有 task。記進 `docs/test-gaps.md` 的「能補、還沒補的測試空缺」。

跑完檔案都還原了。另外記進 `docs/limitations.md` 的「Parser 和載入」：背景圖的載入記在整個 DOM，每份文件的 `load` 都等它。

**測試**：`dom` 254 個、`dom_api` 201 個、`dom_js` 551 個（1 個 ignored）、`renderer` 128 個、`browser` 191 個、`shell` 657 個都通過。

