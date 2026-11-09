# RFC 0019 的細節：`location`、`history` 和同一份文件內的導覽

`README.md` 放決定和設計；這裡放現況的盤點、Chromium 的做法、選項的比較和 WPT 的盤點。

## 1. 現況的盤點

2026-10-08，`main` 的 `24cc9b3`。

- **browser 的 session history**：`crates/browser/src/navigation_controller.rs` 的 `NavigationController`（Chromium 的 `content::NavigationController`）有 entry 的清單（`NavigationEntry`：網址和標題）、目前的 entry 和還沒 commit 的導覽（`NavigationType`：`NewEntry`、`HistoryIndex`、`Reload`）。上一頁、下一頁（`WebContents::go_back` / `go_forward` → `go_to_offset`）和重新整理一律重新抓取、重新載入；entry 沒有「屬於哪份文件」的資訊。
- **同一份文件內的導覽**：只有點連結到自己的 `#fragment`。renderer 的 `FrameView::follow`（`crates/renderer/src/frame_view.rs`）比對去掉 fragment 的網址，相同就設 `fragment_directive`、要求捲動、改 `FrameView.url`，`RenderThread` 送 `RendererHostMsg::DidCommitSameDocumentNavigation { url }`；browser 的 `NavigationController::did_commit_same_document` 照一般的新 entry 加在後面（`push`：丟掉前面的 entry）。沒有 `hashchange`。
- **文件的網址**：`dom` 的 `NodeState::Document` 有 `url`（`document.URL`、`documentURI` 經過 `DomReadScope::document_info` 讀），建 document 時給，之後沒有函式能改；renderer 的 `FrameView` 另存一份 `url`（解析連結、比對 fragment 用）。所以點 `#fragment` 之後兩份不一致：`document.URL` 還是舊的（讀程式得到，第 2 段寫測試確認）。
- **導覽的入口**：`<a>` 的 activation behavior 經過 `FrameClient::follow_link`（`crates/dom_api/src/frame.rs`），renderer 在 event loop 的最上層處理（同一份文件內的捲動，其他的送 `OpenUrl` 給 browser）；子 frame 的 `src` / `srcdoc` 經過 `navigate_child_frame`，由 renderer 自己載入（RFC 0009：browser 不知道子 frame）。
- **URL 的解析**：workspace 的 `url = "2"`（`Cargo.lock` 是 2.5.8），`net`、renderer、`dom_api`、`dom_js` 都用；沒有 `URL`、`URLSearchParams` 的 interface。
- **宣告表**（`crates/dom_api/README.md` 的「怎麼新增一個 API」）：有 constructor、`[LegacyUnforgeable]`、`[ValueIterable]`（`iterable<T>`）；沒有 `iterable<K, V>`、stringifier 的寫法要確認、union 和 record 的參數、static 方法。`Window` 在 `interfaces/core_frame.rs`。
- **V8 的 `ValueSerializer`**：rusty_v8 有（`v8::ValueSerializer`、`ValueSerializerHelper`），kun 還沒用過。

## 2. Chromium 的做法

- **`DOMURL`、`URLSearchParams`**（`third_party/blink/renderer/core/url/`）：`DOMURL` 拿著 `KURL` 和一個 `URLSearchParams`（`search_params_`，GC 的 `Member`），`URLSearchParams` 拿著 `url_object_`（`WeakMember`，指回 `DOMURL`）；改 `searchParams` 時呼叫 `DOMURL::SetSearchInternal`，改 `search` 時 `URLSearchParams::SetInputWithoutUpdate`。解析是 `KURL`（`url/` 的 `GURL`）。
- **`Location`**（`core/frame/location.h`）：每個 `DOMWindow` 一個；讀的成員從 `Document::Url()` 算；`setHash` 等組出新的網址再呼叫 `SetLocation`，只差 fragment 的導覽在 `FrameLoader` 變成同一份文件內的導覽。
- **`History`**（`core/frame/history.h`）：`pushState` / `replaceState` 先用 `SerializedScriptValue::Serialize` 序列化 state，再 `FrameLoader::UpdateForSameDocumentNavigation`：`Document::SetURL`、設定 `HistoryItem` 的 state、通知 browser（`DidCommitSameDocumentNavigation`，帶 `kPushState` / `kReplaceState`）。`state` 從目前的 `HistoryItem` 的 `SerializedScriptValue` 反序列化，同一個 item 快取同一個物件。
- **browser**：`NavigationControllerImpl` 的 `NavigationEntryImpl` 有一棵 `FrameNavigationEntry`（每個 frame 一個），記 document sequence number、item sequence number 和 `PageState`（序列化的 state、捲動位置、表單）。上一頁、下一頁時比對 document sequence number，同一份文件就送 `CommitSameDocumentNavigation`（history navigation），renderer 的 `FrameLoader` 分派 `popstate`、`hashchange`。
- **待確認（對照原始碼和規範，各段開始前）**：`hashchange` 是排進 task（HTML 規範：DOM manipulation task source）還是同步；同一份文件內的 traversal 時 `popstate` 和 `hashchange` 的順序；`history.state` 的大小上限。

## 3. 選項的比較

決定和不選的理由寫在 `README.md` 的「已決定的事項」，這裡補充比較時的考量。

### URL 的解析（第 2 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **`url` crate**（選） | 照 WHATWG，維護中，Servo 用；kun 已經依賴它 | 少數 WPT 的邊界情況可能沒過，要等上游 |
| 自己寫 | 完全掌控 | 量大，而且導覽、`net` 用的是 `url` crate，兩個 parser 會不一致 |

### `URL` 和 `URLSearchParams` 放在哪裡（第 3 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **`dom` 的 GC 型別、宣告表**（選） | 兩種 engine 共用；互相的參照由 GC 處理，和 Blink 的 `Member` / `WeakMember` 一樣 | 新的 GC 型別要照 soundness 的規則，完成時跑 `matrix`、`fuzz` |
| 只在 `dom_js` 實作 | 不碰 GC | 違反 `dom_api` 和 engine 無關的分層；wasm 沒有 |

### session history（第 6 條）和 state（第 7 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **browser 擁有全部的 entry，state 序列化存在 entry**（選） | 和 Chromium、Firefox 一樣；工具列和 `history.length` 一個來源；重新整理之後 state 還在 | 多兩種訊息；state 每次經過 IPC |
| renderer 記同一份文件內的 entry 和 state 的 `v8::Global` | 不用新訊息、不用序列化 | 工具列的上一頁要問 renderer；重新整理或跨文件之後 state 沒了；`Global` 的生命週期要另外管 |

### 子 frame（第 9 條）

| 做法 | 好處 | 壞處 |
| --- | --- | --- |
| **子 frame 的改變只在子 frame 裡生效**（選） | 範圍小；Speedometer 不用子 frame 的歷史 | 子 frame 的 `pushState` 不進 session history，上一頁回不去 |
| 子 frame 的導覽和歷史經過 browser | 照規範（joint session history） | browser 要知道 frame tree，是 RFC 0009 移到「之後的可能」的那一大塊 |

## 4. WPT 的盤點

2026-10-08，`../third_party/wpt` 的 `0f195499f`。數字是檔案數（含 helper 頁面），還沒試跑。

| 目錄 | 檔案 | 用 `fetch` / XHR | 要 `window.open`、`.py`、`.sub` 等 |
| --- | --- | --- | --- |
| `url/` | 32 | 9 | 2 |
| `html/browsers/history/the-location-interface/` | 82 | 0 | 13 |
| `html/browsers/history/the-history-interface/` | 92 | 1 | 21 |
| `html/browsers/browsing-the-web/history-traversal/` | 34 | 1 | 13 |

- `url/` 用 `fetch` 讀資料的是主要的幾份：`url-constructor`、`url-setters`、`url-origin`、`url-setters-a-area`、`percent-encoding`、`toascii`、`IdnaTestV2*`。它們的資料（`url/resources/` 的 `urltestdata.json`、`urltestdata-javascript-only.json`、`setters_tests.json`、`percent-encoding.json`、`toascii.json`、`IdnaTestV2.json`）由 `dom_api` 的 Rust 測試直接讀（第 4 條）。
- `url/` 不用 `fetch` 的：`url-searchparams`、`urlsearchparams-*`（12 個）、`urlencoded-parser`、`url-statics-canparse`、`url-statics-parse`、`url-tojson`、`url-setters-stripping`、`historical`、`idlharness`、`javascript-urls.window.js`（要導覽，第 2 段再看）。

## 5. 跨文件的導覽：和 Chromium 不同的地方（第 24 條）

Chromium：`Location::SetLocation` → `FrameLoader::StartNavigation`；只差 fragment 的在 renderer 裡同步做完，其他的先跑 `beforeunload`，再經過 `LocalFrameClient::BeginNavigation` 在呼叫的當下送出 `BeginNavigation`（帶 `should_replace_current_entry`、reload 的種類、referrer、initiator）；browser 建 `NavigationRequest`、抓取、commit（可能換 process）。`javascript:` 網址在 renderer 排 task 執行，結果是字串時取代 document。

| | kun | Chromium | 看得出來嗎 |
| --- | --- | --- | --- |
| 什麼時候交給 browser | task 結束、event loop 的最上層 | 呼叫的當下 | 幾乎看不出：兩邊的 script 都照常執行，導覽都是非同步的 |
| 同一個 task 裡導覽好幾次 | 只留最後一個 | 後一個取代前一個 | 一樣 |
| 訊息 | 沿用 `OpenUrl`（加 `replace`），另加 `Reload` | `BeginNavigation`（`OpenURL` 是開新視窗、換 process 用的） | 看不出，名字不同 |
| 自動改成取代 | 文件還沒載入完、網址一樣；沒有手勢的概念 | 一樣，有手勢 | 有使用者手勢時才不同 |
| `beforeunload`、`unload`、`pagehide` | 不分派 | 分派 | 有，記成限制 |
| `javascript:` 的字串結果 | 丟掉（和連結一樣） | 取代 document | 有，既有的限制 |
| referrer、initiator、transition type | 不帶 | 帶 | 等 kun 有 referrer 才看得出來 |
| 子 frame | 沒有作用（第 9 條） | 經過 browser | 有，已決定 |
| process | 同一個分頁一直是同一個 renderer | 可能換（site isolation） | 看不出 |
