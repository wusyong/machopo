# RFC 0019 的進度：`location`、`history` 和同一份文件內的導覽

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-08。完成：2026-10-08，Windows 11（Rust 1.99.0）。三段都做完；第 1、2 段做完時一起 merge 回 `main`，第 3 段 rebase 到 RFC 0018 的第 2 段之後 merge 回 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a 宣告表：`[PairIterable]`（第 13 條）、`Record<V>`、有 sequence 和 record 成員的 union（第 14 條）；`dom_js`、`dom_wasm`、WIT 的轉換 | 完成 | 2026-10-08 |
| 1b `URL`、`URLSearchParams`：`dom` 的 GC 型別、`dom_api` 的演算法和宣告、JS 和 wasm 的測試 | 完成 | 2026-10-08 |
| 1c URL 的資料測試（第 15 條）、WPT 的 `url/`、文件 | 完成 | 2026-10-08 |
| 2a `dom` 和宣告表：`Location`（第 18 條）、`[LegacyUnforgeable]`（第 19 條）、`set_document_url`（第 20 條）、「navigate to a fragment」和導覽的 `FrameClient` 方法（第 21、24、25 條）、`HashChangeEvent` 和 `onhashchange`（第 23 條） | 完成 | 2026-10-08 |
| 2b renderer 和 browser：`FrameView` 從 document 讀網址、`hashchange` 的 task（第 22 條）、連結經過同一條路、`OpenUrl` 的 `replace`、`Reload`、`NavigationController` 的取代 | 完成 | 2026-10-08 |
| 2c WPT、`about:demo` 和情境、文件（第 27 條） | 完成（第 1、2 段 merge 回 `main`） | 2026-10-08 |
| 3a `History`（第 29 條）、`SerializedScriptValue` 和 `ValueSerializer`（第 30、31 條）、`pushState` / `replaceState`（第 32 條）、`PopStateEvent` 和 `onpopstate`；JS 和 wasm | 完成 | 2026-10-08 |
| 3b browser 的 session history：document 的識別和 state（第 33 條）、`length` 和 index（第 34 條）、`back` / `forward` / `go`（第 35 條）、traversal 和 `popstate`（第 36 條）、`scrollRestoration`（第 37 條）、子 frame（第 38 條） | 完成 | 2026-10-08 |
| 3c WPT（第 39 條）、導覽到 fragment 的 `popstate`、`about:demo` 和情境、文件 | 完成 | 2026-10-08 |

## 驗收

每一段做完時都在 Windows 11 上跑過 fmt、clippy（沒有警告）、`cargo test --workspace`、`sanity.py deps`、`boundary`、`unsafe`、`docs`、`test.py wpt`、`reftest`、`scenarios`（沒有平台視窗的情境）；第 2 段和 RFC 完成時另跑 `test.py automation`（真的視窗）。

**宣告表和前端**

| 項目 | 怎麼驗證 |
| --- | --- |
| `[PairIterable]`、`Record<V>`、有 sequence 和 record 成員的 union | `dom_api` 的 `records`；JS 的 `bindings` 的三個 URL 測試；wasm 的 guest `url.c`；Lua 的 reftest W9 |
| interface 層級的 `[LegacyUnforgeable]`、`[PutForwards]`、`Location` 自己的 `valueOf` 和 `@@toPrimitive`、不能換的 prototype | `dom_api` 的 `api`；JS 的 `location_is_unforgeable` |
| `SerializedScriptValue`：`ValueSerializer` 的來回、`DataCloneError`（DOM 物件、函式、`SharedArrayBuffer`）、讀不了的 bytes 是 `null`、照 id 快取、在物件的 realm；WIT 的 `list<u8>`、Lua 的字串 | JS 的 `history_and_its_state`、`an_unreadable_state_is_null`、`realms`；wasm 的 guest `history.c`；Lua 的 reftest W9 |
| enumeration 當回傳值、attribute 的 setter 忽略不是它的值的字串 | `dom_api` 的 `history`；JS 的 `history_and_its_state`；wasm 的 guest `history.c` |

**`dom`、`dom_api`**

| 項目 | 怎麼驗證 |
| --- | --- |
| `URL`、`URLSearchParams` 的行為 | `dom_api` 的 `url`；WPT 的 `url/` 18 份（14 份全過）；WPT 的 URL 資料（`url_data`，不過的在 golden file） |
| GC：URL 撐住 params、params 不撐住 URL；window 撐住 `Location` 和 `History`、它們撐住 window；traversal 的 `popstate` 撐住它的 history | `dom` 的 `url`、`location`、`history`、`script_objects`；GC 的工作負載；`test.py matrix`、`fuzz` |
| `Location` 的成員、導覽、取代、`reload` | `dom_api` 的 `location`；wasm 的 guest `location.c`；WPT 的 `the-location-interface/` |
| `History`：`pushState` / `replaceState`（解析、「can have its URL rewritten」、`SecurityError`）、`go`、`scrollRestoration`、discard 之後的 `SecurityError`；導覽到 fragment 之後 state 是 `null`；`PopStateEvent` | `dom_api` 的 `history`；JS 的 `pop_state_events`；WPT 的 `the-history-interface/` |

**renderer 和 browser**

| 項目 | 怎麼驗證 |
| --- | --- |
| 同一份文件內的導覽：網址當場改、`popstate` 當場、`hashchange` 是 task、連結也一樣；`pushState` 不捲動 | browser 的 `navigation`（`location_hash_navigates_to_a_fragment`、`navigations_between_fragments_fire_popstate_then_hashchange`、`a_link_to_a_fragment_fires_popstate` 等）；`shell` 的 `history`；WPT 的 `history-traversal/event-order/` |
| 跨文件的導覽、取代、`reload`、`javascript:`、子 frame 的導覽 | browser 的 `navigation` 的 `links` |
| session history：同一份文件的 entry 之間的上一頁、下一頁（`popstate`、`hashchange`、不重新載入）、state 跟著 entry（重新整理、從別的文件回來）、`length`、script 的 `back()`（子 frame 的也是）、捲動恢復的模式跟著 entry、renderer 換了文件時改成載入、commit 之後再設 length | browser 的 `navigation` 的 `links`、`NavigationController` 的單元測試；`shell` 的 `history`（traversal 捲到 fragment，`manual` 時不捲） |
| 視窗裡：`location.hash`、`pushState` 和工具列的上一頁 | `about:demo` 的兩個按鈕和 `demo-script` 情境（沒有平台視窗和真的視窗） |

## 做過的檢查

2026-10-08，Windows 11。每一段的設計和更正寫進了 `README.md`；這裡是濃縮的紀錄。

- **WPT**：從 964 個（第 1 段開始前）到 1190 個執行（含 `main` 上 RFC 0018 第 2 段加的），每一段連跑兩次以上結果一樣。每一段先把候選的目錄試跑一次（第 4、27、39 條）。
  - 第 1 段：`url/` 18 份（不用 `fetch` 的），14 份全過；`fetch` 讀資料的那些由 `dom_api` 的 `url_data` 跑同樣的資料，不過的在 `crates/dom_api/tests/url-expected-failures.txt`（`url` crate 和它的 `idna`）。
  - 第 2 段：`the-location-interface/` 整個目錄、`history-traversal/` 裡和 `hashchange` 有關的 4 份、`url/javascript-urls.window.js`：40 個執行，25 個全過；32 個進 skip（導覽子 frame 到別的文件、`.sub`、`window.open` 和 Navigation API、manual）。有了 `location`，別的目錄有 testharness.js 的測試名稱從 `Untitled` 變成檔名、幾份全過。
  - 第 3 段：`the-history-interface/`、`history-traversal/` 整個目錄：117 個執行，51 個全過（`event-order/` 的全部）；17 個進 skip（導覽或走子 frame 的歷史、`window.open`，逾時；一個失敗訊息有 server 的 port）。有了 `history`，別的目錄 4 份全過、4 份換了失敗的原因，`location_reload` 進 skip。
  - 進 skip 和照失敗寫 baseline 的原因在 `tests/wpt/README.md` 和 skip 清單的註解。
- **Mutation test**：共 132 個（1a 6、1b 39、1c 2、2a 24、2b 15、2c 2、3a 21、3b 21、3c 2；補了測試的另外重跑），都抓到或說明了等價，跑完都確認檔案還原了。
  - 一開始沒抓到、補了測試之後抓到的：1b 不穩定的排序（加 200 個配對）、不合法的 base 被忽略（加絕對網址配不合法的 base）；2a 別的元素的 `onhashchange` 屬性變成它自己的 handler（在 div 上也分派）；3b 跨文件的 traversal 不換 document（`traversals_within_the_document` 的最後）。
  - 沒抓到、記進 `docs/testing.md` 的：JS 的 record 遇到重複的 key（JS 做不出來，等價）；`Value::Record` 轉成 JS 和 guest 的值（沒有成員回傳 record，走不到）。
- **找到的 bug**：
  - **isolate 拆掉時的 use-after-free**（RFC 0015 的 fuzz 找到，這份開始前修好）：rusty_v8 的 fork 的 `Weak::drop`，見 `../third_party/rusty_v8/README.machopo.md`。
  - **沒有 fragment 時 `location.hash = ''` 會導覽**（2c，WPT 找到）：照 Blink 的 `Location::setHash`，沒有 fragment 和空的 fragment 算相同。
  - **DOM 物件被 `ValueSerializer` 複製成 `{}`**（3a，自己的測試找到）：cppgc 的 wrapper 沒有 embedder field，V8 不問 delegate；改成自己用 `is_api_wrapper` 判斷 host object。
  - **state 的 id 是 process 共用的計數器**（3a，`sanity.py boundary` 擋下）：改成每個 DOM 一個。
  - **導覽到 fragment 不分派 `popstate`**（3c，WPT 的 `event-order/` 找到）：HTML 的「navigate to a fragment」和 Chromium 都同步分派；`location`、連結都補上（`history::fire_pop_state`）。
  - **更正**：1b 寫的「清空 query 時 opaque path 拿掉結尾的空白」是舊版規範的步驟（1c）。
- **測試的寫法**：browser 的 `navigation` 的 `report()` 導覽到 `/report?…`，`wait_for` 把這個訊息也交給 `WebContents`，所以每次回報會真的導覽走，和上一頁、下一頁競爭（3b 一開始的逾時）；和 traversal 有關的測試改用 browser 不開的 `kunreport:`（`logged()`），記進 `docs/testing.md`。
- **`kun:dom`**：0.7.0 沒變（只有新增：`url`、`url-search-params`、`location`、`history`、`hash-change-event`、`pop-state-event` 等，`scroll-restoration` 的 enum，`SecurityError` 和 `DataCloneError` 的代碼 15、16）；`record` 是 WIT 的關鍵字，產生器跳脫它。每次改 WIT 都用 `ci.py guests` 重編 guest，Lua 的產生器多了配對的 union、`list<u8>` 和 enumeration 的回傳值。
- **rebase**：第 1、2 段 merge 前 rebase 一次（`main` 多了 RFC 0018 的第 1 段等）：兩邊都加了 GC 型別和 `Object` 的成員，都留下，重新產生 WIT 和 guest；有了 `location`、`scrollY` 之後兩份 baseline 換了內容，`location_hash.html` 的那項記進 `docs/limitations.md`。
- **不穩定的測試**：rebase 之後 `test.py automation` 第一次 21/22，失敗的那份沒記下（只留了輸出的最後幾行），之後連跑 28 次都通過，沒有重現；以後失敗時保留完整的輸出。
- **RFC 完成時**（`docs/README.md` 的「一份 RFC 做完時」）：`test.py matrix` 8/8（614 個測試）；`test.py fuzz` 20/20（seed `0x0d6850ee`）；`test.py automation` 22/22（真的視窗，看過 `demo-script` 的截圖：walk 列出 22 個按鈕）。最後一次全部的檢查：`cargo fmt`、clippy（`-D warnings`）、`cargo test --workspace`（1358 個）、`sanity.py` 的四項、`test.py wpt`（1145 個執行、0 個不符）、`reftest`（526 個通過、62 個照預期失敗）、`scenarios`（44 次）都通過。第 2 段因為等 `history` 而 skip 的 12 份，有了 `history` 之後再試一次，都還是導覽子 frame 而逾時，留在 skip。
- **第 3 段 merge 前的 rebase**（2026-10-08）：`main` 多了 RFC 0018 的第 2 段（`offset*`、捲動、`scrollTo`、`scroll` 事件、`elementFromPoint`）和 GC 壓力測試的擴充。衝突是兩邊各加一項（`onscroll` 和 `onpopstate`、`scroll` 和 `history` 的模組、WPT 的清單、`about:demo` 的按鈕變成 24 個），和文件同一段落裡的不同句子；重新產生 WIT 的 golden、重編 guest。有了 `scrollTo`，`scroll-restoration-navigation-samedoc` 全過、`scroll-restoration-order` 失敗在捲動位置的恢復（第 36 條），`scroll-restoration-navigation-cross-origin` 走到子 frame 的導覽而逾時，進 skip。重跑：`cargo fmt`、clippy（`-D warnings`）、`cargo test --workspace`（1384 個）、`sanity.py` 的四項、`test.py wpt`（1190 個執行、0 個不符）、`reftest`（526 個通過、62 個照預期失敗）、`scenarios`（44 次）、`test.py automation` 都通過。
