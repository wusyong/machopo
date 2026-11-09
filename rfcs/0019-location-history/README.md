# RFC 0019：`location`、`history` 和同一份文件內的導覽

- 開始日期：2026-10-08
- 狀態：✅ 已完成：2026-10-08（第 1 到 9 條 2026-10-08 接受；第 1 段的第 10 到 17 條、第 2 段的第 18 到 27 條、第 3 段的第 28 到 39 條 2026-10-08 確認；三段都做完，驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`

這份是 RFC 0012 的路線圖（`rfcs/0012-speedometer/roadmap.md`）的代號 B，雜項線的第二項（第一項 A 是 RFC 0015，做完了）。M1 要它的 `URL`、`URLSearchParams`、`location` 和 `hashchange`，M5 要 `history`。

## 摘要

加上 `URL`、`URLSearchParams`、`Location`、`History`、`hashchange` 和 `popstate`，讓同一份文件內的導覽（設定 `hash`、`pushState`、`replaceState`，和它們之間的上一頁、下一頁）不重新載入文件。URL 用 kun 本來就用的 `url` crate 解析；`URL` 和 `URLSearchParams` 是 `dom` 的 GC 型別，經過宣告表，JS 和 wasm 都有。文件的網址只存在 `dom` 的 document。browser 的 session history 記下同一份文件內的 entry，上一頁、下一頁到同一份文件的 entry 時交給 renderer 處理，`history.state` 用 V8 的 `ValueSerializer` 序列化，存在 browser 的 entry，和 Chromium 一樣。

## 動機

**現況（2026-10-08，`main` 的 `24cc9b3`；細節在 `details.md` 第 1 節）**：
- 沒有 `URL`、`URLSearchParams`、`Location`、`History`，也不分派 `hashchange`、`popstate`。
- 同一份文件內的導覽只有點 `#fragment` 連結一種：renderer 捲過去、告訴 browser，browser 的 session history 多一個 entry。但上一頁、下一頁、重新整理一律重新載入，同一份文件的 entry 之間也是（`docs/limitations.md` 的「連結導航和 hit testing」）。
- 文件的網址有兩份：`dom` 的 document（`document.URL`）和 renderer 的 `FrameView`；點 `#fragment` 只更新後者，`dom` 也沒有改網址的函式，所以之後 `document.URL` 是舊的。
- **Speedometer 要**：runner 的 `params.mjs` 用 `URLSearchParams` 讀 `location.search`，`main.mjs` 用 `hash` 切換畫面；TodoMVC 的 12 個 suite 和 NewsSite 的 2 個用 `location`，8 個用 `history`，2 個用 `onhashchange`（RFC 0012 的 `README.md` 和 `details.md` 的盤點）。

目標：
- `URL`、`URLSearchParams` 照 URL Standard，`Location`、`History` 照 HTML，成員照 Chromium 的類別和 Blink 的目錄放進宣告表，JS 和 wasm 都有（RFC 0012 的第 7 條；例外見第 8 條）。
- 同一份文件內的導覽改文件的網址、分派 `hashchange` / `popstate`，不重新載入；在 session history 裡前後走也一樣。
- 文件的網址只有一份。

不在範圍內（「之後的可能」）：
- Navigation API（`window.navigation`）、back-forward cache、捲動位置的恢復（`history.scrollRestoration` 只存值）。
- 子 frame 的導覽和歷史經過 browser（RFC 0009 的「之後的可能」）；這份只讓子 frame 的同一份文件內的改變在子 frame 裡生效（第 9 條）。
- 跨源的 `Location`（kun 的 iframe 都是同源的，RFC 0009）；`pageshow`、`pagehide`、`beforeunload`、`unload`。

## 設計概要

對寫網頁的人：
- `new URL("../a?x=1", base)`、`url.searchParams.get("x")`、`new URLSearchParams(location.search)` 照規範；改 `url.searchParams` 會改 `url.search`，反過來也是。
- `location.href`、`search`、`hash` 等反映文件現在的網址；`location.hash = "#b"` 捲到 `#b`、改網址、之後分派 `hashchange`，不重新載入；設定 `href` 或呼叫 `assign` 到別的文件就導覽過去，`replace` 不留下目前的 entry，`reload` 重新載入。
- `history.pushState(state, "", "/x")` 改網址、新增一個 entry，`history.state` 是 `state` 的複本；按上一頁回到前一個 entry 時不重新載入，分派 `popstate`（`event.state` 是那個 entry 的 state）。`history.length`、`back()`、`forward()`、`go(n)` 照 session history。

對改 kun 的人：
- `dom` 多了 `URL`、`URLSearchParams` 的 GC 型別，document 的網址可以改；`dom_api` 有 URL 的演算法（包著 `url` crate）和 `Location`、`History`。
- browser 的 `NavigationController` 的每個 entry 記住它屬於哪份文件和它的 state；renderer 和 browser 之間多了同一份文件內的 commit（帶 replace 和 state）和 traversal 的訊息。

## 詳細設計

（各項的選項和比較在 `details.md` 第 3 節，理由在「已決定的事項」。）

- **URL 的解析**（第 2 條）：`url` crate（2.5，照 WHATWG URL Standard），`dom_api` 包一層 URL Standard 的 API（setter、`origin`、`searchParams` 的 `application/x-www-form-urlencoded`）；導覽、連結、`document.URL` 和這些物件用同一個 parser。
- **`URL` 和 `URLSearchParams`**（第 3 條）：`dom` 的 GC 型別（Blink 的 `core/url` 的 `DOMURL`、`URLSearchParams`），`URL` 撐住它的 `searchParams`，`URLSearchParams` 弱參照它的 `URL`（第 12 條），改一邊時更新另一邊（URL Standard 的「update steps」）。宣告表新增 `[PairIterable]`（第 13 條）、`Record<V>` 和有 sequence、record 成員的 union（第 14 條）。
- **文件的網址**（第 5 條）：只存在 `dom` 的 document，`dom` 加一個改網址的函式；`FrameView` 不另存，需要時從 document 讀。fragment、`pushState`、`replaceState` 經過 `dom_api` 改它，和 Blink 的 `Document::SetURL` 一樣。
- **`Location`**（第 18 到 27 條）：每個 window 一個（Blink 的 `core/frame/location.h`），成員是 `[LegacyUnforgeable]`。讀的成員從 document 的網址算；設定 `hash` 和只差 fragment 的網址是同一份文件內的導覽（HTML 的「navigate to a fragment」：當場改網址、`history.state` 變成 `null`、當場分派 `popstate`，下一個 frame 捲動，告訴 browser，排一個分派 `hashchange` 的 task），點 `#x` 連結也一樣；其他的經過現有的 `OpenUrl`（加上 replace）或新的 `Reload`，在 task 結束時交給 browser。導覽到 fragment 時的 `popstate` 是第 3 段試跑 WPT 的 `event-order/` 時補上的（HTML 和 Chromium 都分派，第 2 段只分派 `hashchange`）。
- **session history**（第 6 條、第 33 到 38 條）：browser 的 `NavigationEntry` 多記屬於哪份文件（commit 那份文件的 `navigation_id`，Chromium 的 document sequence number）、state 的 bytes 和捲動恢復的模式。renderer 告訴 browser 同一份文件內的 commit（`DidCommitSameDocumentNavigation`）時帶上 replace 和 state；上一頁、下一頁到同一份文件的 entry 時，browser 送 `CommitSameDocumentNavigation`，renderer 確認還是那份文件（不是時回 `SameDocumentNavigationFailed`，browser 改成載入），改網址、`history.state` 和 index，同步分派 `popstate`，捲動恢復的模式不是 `manual` 時捲到 fragment，fragment 變了再分派 `hashchange`；跨文件的照舊重新載入，commit 帶著 entry 的 state、模式、index 和 length（`HistoryEntryParams`），commit 之後 browser 再送 `SetHistoryOffsetAndLength`。renderer 自己記 index 和 length（`SessionHistory`，所有 frame 共用），`history.go()` 等照順序和同一份文件內的 commit 一起在 task 結束時送 `GoToEntryAtOffset`，`scrollRestoration` 送 `UpdateState`。
- **`history.state`**（第 7 條、第 30 到 32 條）：宣告表的 `SerializedScriptValue`，JS 的前端在 `dom_js` 用 V8 的 `ValueSerializer` 序列化，只收它能存的值（DOM 物件靠 `is_api_wrapper` 認出來，和函式、`SharedArrayBuffer`、`WebAssembly.Module` 一樣是 `DataCloneError`）；bytes 存在 `dom` 的 `History` 和 browser 的 entry，`history.state` 讀的時候反序列化，照 state 的 id 快取在 `History` 的 wrapper 上（Blink 的 `[CachedAttribute]`），traversal 的 `popstate` 的 `state` 在還是目前的 state 時就是它。wasm 的 state 是 guest 自己序列化的 bytes（第 8 條）。
- **子 frame**（第 9 條）：有自己的 `Location` 和 `History`，讀取都照規範；子 frame 裡同一份文件內的改變改子 document 的網址、分派事件，但不進 browser 的 session history，`history.length` 和 `back()` 是主 frame 的；子 frame 跨文件的 `location.href = …` 照舊沒有作用（`docs/limitations.md` 的「iframe」）。
- **Process 的邊界**：state 的 bytes 和新的訊息經過 `protocol`，可以序列化；renderer 不留跨 process 才有意義的狀態（`docs/architecture.md` 的「Process 的邊界」）。

### 測試

- **URL**（第 4 條）：`dom_api` 的 Rust 測試直接讀 `../third_party/wpt/url/resources/` 的 `urltestdata.json`、`setters_tests.json` 等，用 kun 的 URL 函式跑，沒過的列在測試裡、寫明原因；WPT 加 `url/` 裡不用 `fetch` 的（`urlsearchparams-*`、`url-statics-*`、`url-tojson` 等），用 `fetch` 讀資料的等 L（`fetch`）做完再加。
- `dom`、`dom_api`、`dom_js`、`dom_wasm` 的單元測試（新的 GC 型別、binding、wasm 每種新物件一個 guest 測試）；browser 的 `NavigationController` 的測試（同一份文件內的 entry、traversal、replace、state）；browser 的 `navigation` 和 `shell` 的端到端測試（`hashchange`、`pushState` 之後上一頁不重新載入、`popstate` 的 state、重新整理之後 state 還在）。
- **WPT**：`html/browsers/history/the-location-interface/`、`the-history-interface/`、`browsing-the-web/history-traversal/` 裡用得到的部分，每段開始時試跑（RFC 0015 的做法）。
- **情境**：`about:demo` 加 `hash` 和 `pushState` 的按鈕，按工具列的上一頁回來，確認沒有重新載入、`popstate` 的內容。
- 新增 GC 型別，所以 RFC 完成時跑 `test.py matrix` 和 `fuzz`；mutation test 照舊。

### 分階段

第 1 條：

1. **第 1 段（M1）**：`URL`（含 static 的 `canParse`、`parse`）、`URLSearchParams`；宣告表要的新功能；URL 的測試（第 4 條）。
2. **第 2 段（M1）**：文件的網址只存一份（修掉 `document.URL` 不跟著 `#fragment` 變）；`Location`；設定 `hash` 和 `hashchange`、`HashChangeEvent`；`assign`、`replace`、`reload`。
3. **第 3 段（M5）**：`History`（`pushState`、`replaceState`、`state`、`length`、`back`、`forward`、`go`、`scrollRestoration`）、`PopStateEvent`；browser 的 session history 記同一份文件內的 entry 和 state，同一份文件內的 traversal；`ValueSerializer`。

每段開始前只決定那一段的事。

## 缺點

- 新的 GC 型別（`URL`、`URLSearchParams`）和 WIT 的成員，維護量和 RFC 0016、0018 差不多；動到 GC，完成時要跑 `matrix` 和 `fuzz`。
- `url` crate 的結果和 Chrome 的 `GURL` 在邊界情況不同（Chrome 自己也沒過全部的 WPT）：kun 照規範，不照 Chrome。
- state 要經過序列化和 IPC，大的 state 每次 `pushState` 都複製一次（Chromium、Firefox 也是）；不設上限（第 32 條，和 Blink 一樣）。
- 子 frame 的 `history` 和規範不同（第 9 條），記進 `docs/limitations.md`。

## 理由和其他做法

`details.md` 第 3 節有完整的比較。要點：URL 用現成、照規範的 `url` crate（第 2 條）；`URL` 和 `URLSearchParams` 互相連著，要 GC，放在 `dom` 才能兩種 engine 共用（第 3 條）；session history 照 Chromium 由 browser 擁有，工具列的上一頁和 `history.length` 只有一個來源（第 6 條）；state 序列化後存在 browser，重新整理和跨文件之後還在，也不多一種 `v8::Global` 的生命週期（第 7 條）。

## 先例

- **Chromium**：`DOMURL`、`URLSearchParams` 在 `core/url`，用 `KURL`（`url/` 的 `GURL` 的包裝；不完全照 WHATWG）。`Location`、`History` 在 `core/frame`；`History::pushState` 經過 `FrameLoader::UpdateForSameDocumentNavigation` 改 `Document::url_`、通知 browser；session history 在 browser 的 `NavigationControllerImpl`，entry 的 `FrameNavigationEntry` 有 document sequence number 和 `PageState`（含序列化的 state）；同一份文件內的 traversal 是 `CommitSameDocumentNavigation`。kun 照這些做，URL 的解析除外（第 2 條）。
- **Firefox**：`nsDocShell` 的 session history 在 parent process（SHIP），state 是 structured clone 的 bytes，和 Chromium 一樣不在 content process 保存。
- **Servo**：也用 `url` crate。

## 已決定的事項

2026-10-08 確認：

1. **R1：一份 RFC，分三段**：第 1 段（M1）`URL`、`URLSearchParams`；第 2 段（M1）文件的網址只存一份、`Location`、設定 `hash` 和 `hashchange`；第 3 段（M5）`History`、session history、`popstate`。第 1 段和導覽無關，可以先 merge。不選「`URL` 另開一份 RFC」：它很小，而且 M1 一定和 `Location` 一起才用得到（runner 用 `URLSearchParams` 讀 `location.search`）。
2. **U1：URL 用 `url` crate 解析**：照 WHATWG URL Standard（Servo 用的），kun 的 `net`、renderer、`dom_api` 本來就用它，物件和導覽用同一個 parser。Chromium 用自己的 `GURL` / `KURL`，不完全照 WHATWG，WPT 的 `url/` 也有不少沒過；kun 照規範。不選自己寫 parser。
3. **U2：`URL` 和 `URLSearchParams` 是 `dom` 的 GC 型別**：像 `AbortController`（`dom` 的 `abort.rs`），經過宣告表，JS 和 wasm 都有；`url.searchParams` 和它的 `URL` 互相連著，改一邊另一邊跟著變，這個循環要 GC。和 Chromium 一樣是 `ScriptWrappable`（`core/url`）。不選「只在 `dom_js` 裡實作」：違反 `dom_api` 和 engine 無關的分層。新增 GC 型別，所以完成時跑 `matrix` 和 `fuzz`。
4. **U3：沒有 `fetch` 時，URL 的 WPT 資料由 Rust 測試直接讀**：`dom_api` 的測試讀 `../third_party/wpt/url/resources/` 的 `urltestdata.json`、`setters_tests.json`，用 kun 的 URL 函式跑（同一份資料，不經過 JS，不改 WPT）；WPT 先加不用 `fetch` 的檔案，要 `fetch` 的等 L 做完。不選「等 L 或把 L 提前」：M1 會卡在 L 上。
5. **D1：文件的網址只存在 `dom` 的 document**：`FrameView` 從它讀；同一份文件內的導覽（fragment、`pushState`、`replaceState`）經過 `dom_api` 改它，和 Blink 的 `Document::url_`、`Document::SetURL` 一樣。順便修掉點 `#fragment` 之後 `document.URL` 不變的 bug。不選「保留兩份、互相同步」：容易再漏。
6. **H1：session history 由 browser 擁有，同一份文件內的 entry 也記**：每個 entry 記住屬於哪份文件（Chromium 的 `FrameNavigationEntry` 的 document sequence number）；上一頁、下一頁到同一份文件的 entry 時，browser 送同一份文件內的 traversal（Chromium 的 `CommitSameDocumentNavigation`），renderer 不重新載入，改網址和 `history.state`、分派 `popstate`（fragment 變了再分派 `hashchange`）；跨文件的照舊重新載入（沒有 bfcache）。不選「renderer 自己記同一份文件內的 entry」：和 Chromium 不同，工具列的上一頁和 `history.length` 會有兩個來源。
7. **H2：`history.state` 用 V8 的 `ValueSerializer` 序列化，存在 browser 的 entry**：重新整理、從別的文件回來時 state 還在，和規範、Chromium（`SerializedScriptValue` 建在 `ValueSerializer` 上，存在 entry 的 `PageState`）一樣；只支援 `ValueSerializer` 本身能處理的值，DOM 物件是 `DataCloneError`；序列化是 V8 專屬的，放在 `bindings` / `dom_js`。不選「renderer 用 `v8::Global` 拿著每個 entry 的值」：重新整理或跨文件之後 state 就沒了，又多一種 `Global` 的生命週期要管。
8. **W1：全部經過宣告表，JS 和 wasm 都有；state 是例外**：JS 的 state 是 `any`，wasm 的是 `list<u8>`，由 guest 自己序列化；browser 只存 bytes，不管是哪種 engine 產生的（同一份文件只有一種 engine）。不選「wasm 沒有 state，只有 `pushState(url)`」。
9. **F1：子 frame 有自己的 `Location` 和 `History`，同一份文件內的改變只在子 frame 裡生效**：讀取都照規範；子 frame 的 `hash`、`pushState` 改子 document 的網址、分派事件，但不進 browser 的 session history；子 frame 跨文件的 `location.href = …` 照舊沒有作用，記進 `docs/limitations.md`。RFC 0009 把「子 frame 的導覽和歷史經過 browser」移到了「之後的可能」，Speedometer 也用不到。不選「在這份把子 frame 的導覽也做進 browser」：範圍大很多。

第 1 段，2026-10-08 確認：

10. **A1：第 1 段的範圍**：`URL` 的 constructor、`URL.parse`、`URL.canParse`、`href`（stringifier）、`origin`、`protocol`、`username`、`password`、`host`、`hostname`、`port`、`pathname`、`search`、`searchParams`、`hash`、`toJSON`；`URLSearchParams` 的 constructor、`size`、`append`、`delete` / `has`（帶 value 的形式）、`get`、`getAll`、`set`、`sort`、`toString` 和 iterable。不做 `URL.createObjectURL` / `revokeObjectURL`：要 `Blob`（路線圖的 H2），列進 `spec-missing.txt`。
11. **A2：`dom` 的 `URL` 物件存解析好的 `url::Url`**：`dom` 因此依賴 `url` crate，只負責存取；URL Standard 的演算法（setter、update steps）在 `dom_api`。和 Blink 的 `DOMURL` 拿著 `KURL` 一樣。不選「只存 href 字串，每次在 `dom_api` 重新 parse」：每個 getter 都多 parse 一次。
12. **A3：`URL` 用 `Edge` 撐住它的 `searchParams`，`URLSearchParams` 用 `WeakEdge` 指回 `URL`**：照 Blink 的 `Member` 和 `WeakMember`；只拿著 `searchParams` 時，`URL` 被回收之後沒有人看得到它的更新。`URLSearchParams` 的 list 存在 `dom`。不選兩邊都強參照：GC 收得了，但和 Blink 不同，也沒有好處。
13. **A4：宣告表的 `iterable<K, V>` 是 `[PairIterable]`**：interface 有三個成員 `iterableLength`、`iterableKey(i)`、`iterableValue(i)`（標成 `[IterableLength]`、`[IterableKey]`、`[IterableValue]`），每次照當下的 list 讀，所以是 live 的，和 Blink 的 `PairSyncIterable` 的 iteration source 一樣。JS 的前端照 WebIDL 建 default iterator（每個 interface 一個 iterator prototype，`[[Prototype]]` 是 `%IteratorPrototype%`，有 `next` 和 `Symbol.toStringTag`）和 `entries`、`keys`、`values`、`forEach`、`Symbol.iterator`，三個成員不在 prototype 上；WIT 有那三個函式，guest 自己照 index 讀。不選「每次複製成陣列，用 `Array.prototype` 的 iterator」：不是 live 的（WPT 的 `urlsearchparams-foreach` 在迭代中刪除），prototype 也不對。
14. **A5：宣告表加 `Record<V>`（`record<DOMString, V>`），union 可以有 sequence 和 record 成員，union 參數可以有預設值**：JS 照 WebIDL 取 own、enumerable 的 key 依序轉換；union 遇到物件時，有 `Symbol.iterator` 是 sequence，否則是 record，再來才是字串。WIT 的 record 是 `list<tuple<string, T>>`。不選「`URLSearchParams` 的參數降成 `Any`、在 JS 的前端解析」：wasm 就沒有這個 constructor。
15. **A6：URL 的資料測試**：`dom_api` 的測試用 `serde_json`（dev-dependency）讀 `../third_party/wpt/url/resources/` 的 `urltestdata.json`、`urltestdata-javascript-only.json`、`setters_tests.json`、`toascii.json`、`IdnaTestV2.json`（和 `-removed`），經過 kun 的 constructor 和 getter / setter 跑，檢查和 JS 版的測試一樣的東西；沒過的寫進 golden file `crates/dom_api/tests/url-expected-failures.txt`（`MACHOPO_UPDATE_GOLDEN=1` 重新產生）。不跑 `percent-encoding.json`：它測非 UTF-8 文件裡 `<a>` 的編碼，`URL` 物件一律 UTF-8。
16. **A7：`url` crate 和規範不一致時不 fork、不在 kun 這邊修 parser**：記進 expected failures 和 `docs/limitations.md`，影響 Speedometer 時再討論；只有 URL Standard 的 API 層（例如 `URL.parse` 回傳 `null`）自己寫。
17. **A8：放在哪裡**：宣告在新的 `interfaces/core_url.rs`（Blink 的 `core/url`），`dom_api` 的模組是 `dom_url.rs`、`url_search_params.rs`，`dom` 的 GC 型別是 `DomUrl`、`UrlSearchParams`。

第 2 段，2026-10-08 確認：

18. **L1：`Location` 是 `dom` 的 GC 型別**：指向它的 window，window 用 `Edge` 撐住它（每次同一個）；`window.location` 和 `document.location` 是同一個，document 沒有 browsing context 時 `document.location` 是 `null`；JS 和 wasm 都有。和 Blink 的 `core/frame/location.h`（window 持有的 `ScriptWrappable`）一樣。
19. **L2：宣告表完整支援 `[LegacyUnforgeable]`**：可寫的 attribute、方法也能加，也能和 `[PutForwards]` 一起用（`window.location`、`document.location` 是 `[PutForwards=href, LegacyUnforgeable]`）；JS 的前端照 HTML 在 `Location` 物件上放自己的 `valueOf` 和 `Symbol.toPrimitive`。不選「放在 prototype 上」：只有刻意改它的 script 看得出來，但和規範不同。
20. **L3：文件的網址只存在 `dom`（第 5 條的做法）**：`dom` 加 `set_document_url`；`FrameView` 拿掉自己的 `url`，從 document 讀，網址變了就重算 base URL。
21. **L4：同一份文件內的導覽只有一條路**：`dom_api` 一個函式做 HTML 的「navigate to a fragment」：當場改 document 的網址、告訴 frame（捲動、告訴 browser）、fragment 變了才排 `hashchange`；`location.hash`、只差 fragment 的 `href` / `assign` / `replace` 和點 `#x` 連結都經過它（連結照舊在 event loop 的最上層處理）。和 Chromium 的 `DocumentLoader::CommitSameDocumentNavigation`（同步改網址）一樣。
22. **L5：`hashchange` 是 task**：記在 frame 上，renderer 在 event loop 的最上層分派；和規範（DOM manipulation task source）、Blink 的 `EnqueueHashchangeEvent` 一樣。不選同步或 microtask。
23. **L6：`HashChangeEvent`（`oldURL`、`newURL`、constructor 和 `HashChangeEventInit`）、window 的 `onhashchange`、`<body onhashchange>` 轉給 window**：和 Blink 的 `core/events/hash_change_event.h` 一樣。
24. **L7：跨文件的導覽經過 browser，非同步**：`href` 設成別的文件、`assign`、`replace`、`reload` 和 `hash` 以外的 setter，由 `FrameClient` 的新方法記下（帶 replace 或 reload），同一個 task 裡只留最後一個（Chromium 是後一個取代前一個）；renderer 在 event loop 的最上層送 `OpenUrl`（加上 `replace`）或新的 `Reload`，browser 的 `NavigationController` 加上取代目前 entry 的導覽；commit 之前舊的文件照舊執行。照 HTML 的「Location-object navigate」，文件還沒載入完（kun 沒有使用者手勢的概念，一律當成沒有）或網址和現在的一樣時自動改成取代，和 Chromium 一樣。`javascript:` 網址和現在的連結一樣在 renderer 執行；子 frame 沒有作用、記 log（第 9 條）。和 Chromium 不同的地方在 `details.md` 第 5 節：交給 browser 的時間（task 結束時，Chromium 是呼叫的當下）、訊息（沿用 `OpenUrl`，Chromium 是 `BeginNavigation`）、不分派 `beforeunload` / `unload` / `pagehide`、`javascript:` 的字串結果不取代 document、不帶 referrer 和 initiator。
25. **L8：同一份文件內的「取代」這一段就做**：`DidCommitSameDocumentNavigation` 加上 `replace`，`location.replace("#x")` 取代目前的 entry。
26. **L9：不做的**：`ancestorOrigins`（要 `DOMStringList`，列進 `spec-missing.txt`）、跨源的安全檢查（第 9 條）；上一頁、下一頁在 fragment 的 entry 之間照舊重新載入，也不分派 `hashchange`，等第 3 段（先記成限制）。
27. **L10：測試**：試跑 WPT 的 `the-location-interface/`、`history-traversal/` 裡和 `hashchange` 有關的、`url/javascript-urls.window.js`，照第 1 段的做法決定加哪些；`about:demo` 加設定 `hash` 和 `href` 的按鈕，情境檢查 `hashchange`、`document.URL` 和沒有重新載入。

第 3 段，2026-10-08 確認：

28. **P1：分三步**：3a `History` 物件、state 的序列化（JS 和 wasm）、`pushState` / `replaceState` 改網址和 `history.state`，browser 照現有的方式記 entry；3b browser 的 session history（document 的識別、entry 的 state、同一份文件內的 traversal 和 `popstate`、`length` / `back` / `forward` / `go`、跨文件的 commit 帶 state）；3c WPT、`about:demo` 和情境、文件。3a 不動 browser，可以先單獨測完。
29. **P2：`History` 是 `dom` 的 GC 型別**：每個 window 一個，window 用 `Edge` 撐住它（和 `Location` 一樣，Blink 的 `History` 是 `LocalDOMWindow` 持有的 `ScriptWrappable`）；記著目前 entry 的 state（bytes），browser 的 entry 是正本；JS 和 wasm 都有。不選「state 存在 `Window`、`History` 只是 binding 的殼」：kun 的 wrapper 都對應 `dom` 的物件，要為它開例外，wasm 也沒有東西可拿。
30. **P3：宣告表新增 `SerializedScriptValue`**（Blink 的名字）：`dom_api` 只看到 bytes；JS 的前端在 `dom_js` 用 V8 的 `ValueSerializer` 序列化（DOM 物件、函式等它處理不了的值是 `DataCloneError`），讀的時候用 `ValueDeserializer`，失敗（例如 wasm 的 guest 寫的 bytes）是 `null`，和 Blink 一樣；WIT 是 `list<u8>`，guest 自己序列化（第 8 條）。Blink 的 IDL 是 `any`，在 `History::pushState` 裡序列化；kun 把轉換放在 binding 那一層，因為 `dom_api` 不能碰 V8。不選「參數是 `any`，`ScriptHost` 加 serialize / deserialize」：`any` 只有 JS 有，wasm 要另外宣告一組成員。不選「只有 JS 有 state」：違反第 8 條。
31. **P4：`history.state` 的快取和 `PopStateEvent.state`**：每份 state 帶一個識別（renderer 在 state 設定或收到時從遞增的計數器取號，不經過 IPC）；JS 的前端照識別把反序列化的結果快取在 `History` 的 wrapper 上，所以 state 改變之前 `history.state` 每次是同一個物件（Blink 的 `History::StateHelper` 用 private property 和 `last_state_object_requested_`）。traversal 分派的 `popstate` 記著 state 的識別，還是目前的 state 時 `event.state === history.state`（Blink 的 `IsSameAsCurrentState`），否則自己反序列化、快取。`new PopStateEvent(type, {state})` 的 `state` 原樣存成 `ScriptValue`（和 `CustomEvent.detail` 一樣），`e.state === obj`；所以 `PopStateEvent.state` 只有 JS 有，不在 WIT 裡（和 `ErrorEvent.error` 一樣），wasm 讀 `history.state`。不選每次讀都反序列化：每次是新物件，`history.state.x = 1` 之後再讀就不見了。
32. **P5：`pushState` / `replaceState` 照規範**：文件不是 fully active 是 `SecurityError`；序列化失敗是 `DataCloneError`；網址照 base URL 解析，解析失敗或不能改寫（HTML 的「can have its URL rewritten」）是 `SecurityError`（Blink 的 `CanChangeToUrlForHistoryApi`）；網址當場改，不捲動、不分派事件；`title` 不用。導覽到 fragment 的新 entry 的 state 是 `null`，所以之後的 `history.state` 是 `null`（和 Chrome 一樣）。**不設 state 的大小上限**：和 Blink 一樣（`history.cc` 沒有上限，只有 IPC 的），記進 `docs/limitations.md`，出問題再設。**不做 Chromium 的 navigation rate limiter**：目前不需要。
33. **P6：document 的識別和訊息**：browser 的 entry 多記 document sequence number，值是建立那份 document 的跨文件 commit 的 `navigation_id`，同一份文件內的 entry 沿用；重新整理、錯誤頁、`about:` 頁、renderer 重開之後都是新的。`CommitNavigation` 加上 entry 的 state、`history_index`、`history_length`；`DidCommitSameDocumentNavigation` 加上 state 和 traversal 的 `navigation_id`（browser 發起的才有）；新的 `RendererMsg::CommitSameDocumentNavigation` 帶 `navigation_id`、document 的識別、網址、state、index、length。renderer 已經換了文件（識別不符）時回 `SameDocumentNavigationFailed`，browser 改成跨文件載入（Chromium 的 `CommitResult::RestartCrossDocument`）。browser 等 renderer 回報 commit 才把 traversal 算成完成（pending 照舊），不先改目前的 entry，和 Chromium 一樣。不選只比對網址：重新整理之後網址一樣卻是新文件，`pushState` 改了 path 卻是同一份。不選識別不符時忽略：上一頁沒有反應，pending 一直留著。
34. **P7：renderer 自己記 `history.length` 和目前的 index**：browser 在 commit 和 traversal 時送來；同一份文件內的新 entry（`pushState`、連結和 `location.hash` 的 fragment）時 renderer 自己更新（index 加一、丟掉前面的），`replace` 的不變。整個分頁一份（Chromium 的 `Page` / `WebView` 那一層，`CommitNavigationParams` 的 `current_history_list_offset` / `length`），由 browser 的訊息設定，符合 process 的邊界。不選每次問 browser：要同步 IPC。
35. **P8：`back()`、`forward()`、`go(n)`**：renderer 先用自己記的 index 和 length 擋掉超出範圍的（Blink 的 `NavigateBackForward`），其他的記在 `FrameRequests`，在 event loop 的最上層送 `GoToEntryAtOffset(n)`，每次呼叫都依序送（連按兩次退兩步：browser 從還在進行的那次往後算），而且和同一份文件內的 commit 排在同一個依序的佇列（`pushState(); back()` 要回到 push 之前的 entry）；browser 送出前再檢查一次範圍。`go(0)`、`go()` 是 reload（Blink 相同）。工具列的上一頁也走同一條路：document 的識別和目前的一樣就送同一份文件內的 traversal，否則跨文件載入、帶那個 entry 的 state。和 Chromium 不同的只有送出的時機（呼叫的當下），和第 24 條一樣看不出來。不選「同一個 task 只留最後一個」：`back(); back()` 只退一步。
36. **P9：traversal 時事件的順序和捲動**：收到同一份文件內的 traversal（一個 task）時，改網址、`history.state`、index，在同一個 task 裡同步分派 `popstate`，fragment 變了再排一個 `hashchange` 的 task；照規範，和 Blink 一樣。第一次載入不分派 `popstate`。捲動：`scrollRestoration` 是 `"auto"` 且網址有 fragment 時捲到 fragment，否則不捲；這是沒有捲動位置恢復時的近似（Chrome 恢復之前的位置），記成限制。不選都不捲：在 fragment 之間上一頁時畫面不動，比現在（重新載入）還差。
37. **P10：`history.scrollRestoration` 存在 browser 的 entry**：和 state 放在一起（Chromium 的 `PageState`），新 entry 沿用目前 entry 的值，traversal 時跟著回來；設定時 renderer 送 `UpdateState` 改目前 entry 的值（Chromium 的 `DidUpdateCurrentHistoryItem`）。只決定第 36 條捲不捲到 fragment，不恢復捲動位置。不選「只存在 renderer 的 `History`」：traversal 之後讀到的值不對，捲動也會照錯的值。
38. **P11：子 frame（細化第 9 條）**：子 frame 的 `pushState` / `replaceState` 改子 document 的網址，state 只存在子 frame 的 `History`，不進 browser，所以不分派 `popstate`、重新整理之後不見，`history.length` 不變；子 frame 的 `length`、`back()`、`go()` 是主 frame 的（joint session history 的近似）。Chromium 每個 frame 都有 `FrameNavigationEntry`，子 frame 的 entry 也回得去；記進 `docs/limitations.md`。
39. **P12：測試**：browser 的 `NavigationController` 單元測試（document 的識別、state、同一份文件內的 traversal、改成跨文件）；`shell` 的端到端測試（`pushState` 之後上一頁不重新載入、`popstate` 的 state、重新整理之後 state 還在、從別的文件回來恢復 state、fragment 之間上一頁分派 `hashchange`、`history.length`、`go(0)`）；`dom_js` 的 `ValueSerializer`（`Map`、`Date`、循環參照的來回，DOM 物件和函式是 `DataCloneError`，`history.state` 每次同一個，`event.state === history.state`）；wasm 的 guest `history.c`（bytes 的 state），Lua 的 generator 支援 `list<u8>`（Lua 字串）；試跑 WPT 的 `the-history-interface/` 和 `history-traversal/` 其餘的檔案；`about:demo` 加 `pushState` 的按鈕，情境按工具列的上一頁、下一頁，檢查沒有重新載入、`popstate` 的 state 和網址。RFC 完成時跑 `matrix` 和 `fuzz`。

## 待決定

沒有（`hashchange` 的時機是第 22 條，`popstate` 的是第 36 條，訊息和 document 的識別是第 33 條，state 的大小上限是第 32 條）。

## 之後的可能

- Navigation API（`window.navigation`）、back-forward cache、捲動位置的恢復（`history.scrollRestoration` 真的作用）。
- 子 frame 的導覽和歷史經過 browser（RFC 0009 的「之後的可能」）；跨源的 `Location`。
- L（`fetch`）做完之後，加上 `url/` 裡用 `fetch` 讀資料的 WPT。
- J（`MessageChannel`、`postMessage`）的 structured clone 可以沿用第 7 條的 `ValueSerializer`。
