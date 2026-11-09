# RFC 0082 的進度：DevTools 的頁面和網路：`Network`、`Log` 和頁面的生命週期

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-23，macOS（Apple Silicon）。在 `0082` 上做。

完成日期：2026-10-24，macOS（Apple M4）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a `net`：redirect 每一跳停下來問（第 3 節）、時間（第 4 節，先做歸屬的原型）、body 改成 `Bytes`（第 2 節） | 完成 | 2026-10-23 |
| 1b renderer 的 network agent：子資源的事件、`getResponseBody`、`getRequestPostData`、第 11 節的命令；`ipc::TimeTicks`、`net` 自己解壓、Fetch 的過濾搬到 renderer | 完成 | 2026-10-23 |
| 1c browser 送導覽的事件、commit 時送文件的 `dataReceived` 和 `loadingFinished`、`Page.domContentEventFired`、`loadEventFired`；導覽在 UI thread 上 commit | 完成 | 2026-10-23 |
| 2 發起者、`Log`、Sources 的子資源（第 5 到 7 節） | 完成 | 2026-10-24 |
| 3 生命週期、`Page.navigate`、`Target`（第 8 到 10 節） | 完成 | 2026-10-24 |

## 驗收

**回應的時間、redirect、解壓（`net`、`ipc`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 回應帶著時間：新開的連線有 DNS（由名字解析的）和連線（從 DNS 結束時算），重用的沒有；等伺服器的時間在連線和 header 之間，body 慢來時收完在 header 之後；時間從給的起點算；伺服器的位址、`http/1.1`；`data`、`file`、`blob` 整段計時、協定是 scheme | `cargo test -p net --test it timing::` |
| 只記到 DNS、沒記到連線的請求（新連線還在開時舊的先空出來）不算；連線從 DNS 結束時算；HTTP 版本的名字 | `cargo test -p net --lib http::tests` |
| `fetch_with_redirects` 停在每個 redirect、交出 redirect 的回應（狀態、header、時間、空的 body）和下一個請求（網址、303 之後的 `GET`），`follow_redirect` 才繼續；鏈上每一跳各等一次；停住時取消就結束；redirect 之前的 follow 不算；`fetch` 自己跟 | `cargo test -p net --test it redirects::` |
| `TimeTicks` 和 `Instant` 一起往前走；讀數移動、序列化；另一個 process 讀到同一個時鐘 | `cargo test -p ipc` |
| `net` 自己解壓：gzip、`x-gzip`、zlib 和 raw 的 deflate、br、多層照反序、不認得的編碼原樣、空的 body、壞掉的資料是 `ERR_CONTENT_DECODING_FAILED`；`Content-Encoding`、`Content-Length` 留著；壓縮前的 body 長度、照 HTTP/1 算的 head 長度；`Accept-Encoding`，`Range` 的是 `identity`；本機的 scheme 不佔傳輸 | `cargo test -p net --lib content_encoding`、`cargo test -p net --test it content_encoding::` |
| renderer 拿到的回應：CORS 的有全部的 header、opaque 的 `fetch()` 有 status 和 header（body 清掉），每個回應都沒有 `Set-Cookie` | `cargo test -p net --test it load::cors_requests load::no_cors_and_same_origin_modes` |
| Fetch 給 script 看的過濾：safelisted、exposed 的名字、`*` 在帶 credentials 時不算 | `cargo test -p protocol --test it network::a_cors_filtered_responses_headers`、`cargo test -p shell --test it fetch::a_star_exposes_every_header_but_with_credentials` |

**redirect 等的地方（`browser`、`shell`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 子資源的 redirect 等 renderer 回答：main thread 在跑 script 時下一個請求不送出 | `cargo test -p shell --test it fetch::a_redirect_waits_for_the_renderer` |
| 導覽的 redirect 由分頁在 UI thread 上繼續；被取代的導覽的 redirect 不讓任何導覽繼續 | `cargo test -p browser --test it navigation::a_navigation_waits_at_a_redirect_for_the_tab navigation::http_with_redirect_and_charset` |
| 視窗（`BrowserView`）和 headless 把導覽的 redirect 交給分頁 | `cargo test -p shell --test it browser_view::a_navigation_through_a_redirect_loads_where_it_leads network::a_page_reached_through_a_redirect_renders` |
| 導覽在 UI thread 上 commit；被取代的導覽的結果晚到也不 commit | `cargo test -p browser --test it navigation::a_replaced_navigations_end_commits_nothing` |

**`Network` 的事件（renderer 的 agent 和 browser）**

| 項目 | 怎麼驗證 |
| --- | --- |
| `Network` 的事件：子資源和 `fetch()` 的 `requestWillBeSent`（frame、文件、`loaderId`、類型、id 的格式）、`responseReceived`（全部的 header、時間、位址、協定）、`dataReceived`、`loadingFinished`；`getResponseBody` 的文字和 base64、不認得的 id；換文件時 body 清掉；POST 的 body 在上限內時帶著 | `cargo test -p shell --test it devtools_network::requests_are_told_with_their_responses_and_bodies` |
| redirect 是同一個 id 的另一個 `requestWillBeSent`（帶 `redirectResponse`）；連不上的 `loadingFailed`、取消的帶 `canceled` | `cargo test -p shell --test it devtools_network::redirects_and_failures_are_told` |
| 每個 session 各自的上限、留著的 body、請求的 body、打開和關掉；沒打開的 session 聽不到 | `cargo test -p shell --test it devtools_network::each_session_keeps_its_own` |
| iframe 的文件是子 frame 的請求，它的 id 是那份文件的 `loaderId`（`Page.frameNavigated` 的也是） | `cargo test -p shell --test it devtools_network::an_iframes_document_is_its_own_request` |
| 做不到的命令是錯誤、本來就成立的成功（renderer 和 browser 各自回答的） | `cargo test -p shell --test it devtools_network::commands_kun_cant_carry_out_are_errors` |
| main frame 的導覽：browser 送的 `requestWillBeSent`（文件、`loaderId`、frame）、redirect、`responseReceived`，renderer 送的 `dataReceived`（壓縮前的長度）、`loadingFinished`，依序；文件的 body；`Page.domContentEventFired`、`loadEventFired` 和 `Page.lifecycleEvent` 在同一個時鐘上 | `cargo test -p shell --test it devtools_network::a_navigation_is_told_by_the_browser_and_the_renderer` |
| 失敗的導覽是 `loadingFailed`，沒有 `responseReceived`；`Network.disable` 之後 browser 不再送 | `cargo test -p shell --test it devtools_network::a_navigation_that_fails_is_a_failed_request` |
| kun 自己的 `about:` 頁沒有請求 | `cargo test -p shell --test it devtools_network::the_browsers_own_pages_are_no_requests` |
| `Page.reload` 收 DevTools 送的參數：重新載入、聽得到它的請求；`scriptToEvaluateOnLoad`、不是顯示中的文件的 `loaderId` 是錯誤 | `cargo test -p shell --test it devtools_network::devtools_reloads_the_page` |

**發起者、`Log`、Sources 的子資源（`bindings`、`renderer`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| inspector 抓的 stack：沒打開 `Runtime` 時一個 frame、打開時整個；`await` 之後帶 async 的 parent；沒有 script 在跑、沒告訴 inspector 的 realm、只有 async parent 的空 stack 都沒有 | `cargo test -p bindings --test it stack_traces::` |
| 發起者：parser 的元素帶開始標籤的行（parser 結束之後才發的也是）、stylesheet 的 `url()` 帶 stylesheet 的網址、`<style>` 和 `style` 屬性的帶文件的網址（不是 base URL）、static import 帶 importer | `cargo test -p shell --test it devtools_initiators::the_parser_and_stylesheets_ask`、`cargo test -p renderer --lib modulator::`、`cargo test -p style --test it cascade::adopted_sheets_cascade_after_the_documents_own` |
| script 要的帶 stack（`<img>`、`<link>`、`<script>`、`<iframe>`、`await` 之後的 `fetch()`、`import()`），沒要 stack 時是 `other`；沒用上的 stack 不留到下一次；script 建的 iframe 的 `frameAttached` 帶 stack | `cargo test -p shell --test it devtools_initiators::script_asks_with_its_stack` |
| `Log`：404、連不上（印到 stderr、帶網址和 `networkRequestId`）、晚打開時補送、`clear`、換文件時清掉；文件本身的 404；做不到的命令另外送警告（renderer 的和 browser 的），沒打開 `Log` 的 session 不送、不留 | `cargo test -p shell --test it devtools_log::` |
| 留最近 1000 則、補送前先說丟了幾則、補送在回答之前 | `cargo test -p renderer --lib log_agent::` |
| `getResourceTree` 列出子資源（類型、MIME、大小、失敗的），不列 `fetch()`；`getResourceContent` 從 `Network` 留著的 body 給（文字、base64），打開之前載入的沒有；一個網址一個 | `cargo test -p shell --test it devtools::subresources_are_listed_with_their_content`、`cargo test -p renderer --lib frame_view::tests::a_documents_resources_are_one_per_url` |

**生命週期、`Page.navigate`、`Target`、machopodriver**

| 項目 | 怎麼驗證 |
| --- | --- |
| 生命週期由 renderer 送：每個 frame 的 `frameStartedLoading`、`init`（在 `frameNavigated` 之前，iframe 的初始文件也有）、`DOMContentLoaded`、`load`、`frameStoppedLoading`、`networkAlmostIdle`、`networkIdle`（帶 `Page.navigate` 的 `loaderId`；iframe 的是它自己的；`fetch()` 延後，時間是安靜開始時） | `cargo test -p shell --test it devtools_lifecycle::a_navigation_is_told_through_its_lifecycle` |
| 進行中的子資源也讓網路不安靜 | `cargo test -p shell --test it devtools_lifecycle::a_subresource_on_its_way_keeps_the_network_busy` |
| 沒有 `src` 的 iframe 的初始文件送完整的一輪；文件要來的只送初始文件的 `init` | `cargo test -p shell --test it devtools_lifecycle::an_iframe_that_stays_blank_has_loaded` |
| 每次打開都補送 `commit` 和到目前為止的事件（`Page` 沒打開也補送）；之後的事件要 `Page`；`Page.disable` 也關掉 | `cargo test -p shell --test it devtools_lifecycle::the_lifecycle_so_far_is_told_as_it_is_turned_on` |
| 安靜的計算：不超過 2 個、0 個，持續 500 ms，各一次，時間是安靜開始時 | `cargo test -p renderer --lib idleness_detector` |
| renderer 重開之後 `Page` 和 lifecycle 照舊打開，browser 補送的命令的回答不給 client | `cargo test -p shell --test it devtools_lifecycle::a_new_renderer_has_the_lifecycle_on`、`webdriver::a_crashed_tab_is_told` |
| `Page.navigate` 在 ready-to-commit 回答（回應的事件在前）、`isDownload`、失敗的 `errorText`、被取代的 `net::ERR_ABORTED`、`about:` 頁立刻回答、分頁關掉時是錯誤 | `cargo test -p shell --test it devtools_lifecycle::a_navigation_is_answered_as_it_is_ready_to_commit devtools_lifecycle::flat_sessions_detach` |
| `Target.setAutoAttach` 回答成功；`detachFromTarget` 拿掉 session、送 `detachedFromTarget`；分頁關掉時 flat session 收到它 | `cargo test -p shell --test it devtools_lifecycle::flat_sessions_detach` |
| machopodriver：導覽失敗是 `unknown error`（`net::ERR_…`）；`Kun.waitForLoad` 是 session 的分頁的 | `cargo test -p shell --test it webdriver::a_navigation_that_fails_is_an_error automation_pipe::waiting_for_a_load_is_the_sessions_tabs` |
| 和 Chrome 比：每個 frame 的生命週期、請求和回應、`Log` 的補送 | `python3 scripts/test.py cdp lifecycle network log` |

## 做過的檢查

2026-10-23 到 2026-10-24，macOS（Apple M4），全部在這台上做；手動確認用 Chrome for Testing 152。

- **做了什麼**（設計見 `README.md` 的「詳細設計」）：
  - `net`：`fetch_with_redirects` 停在每一跳、`follow_redirect` 才繼續；`LoadTiming`（DNS、連線、等伺服器、收完，記在請求自己開的連線上）；body 是 `bytes::Bytes`；自己解壓（reqwest 的 `gzip`、`brotli`、`deflate` 拿掉，改用 `flate2`、`brotli-decompressor`），壓縮前的長度給 `dataReceived`；`Set-Cookie` 不交給 renderer。測試伺服器加了 keep-alive（`start_keep_alive`）和 header 與 body 之間的延遲（`Route::with_body_delay`）。
  - 新的 crate `ipc`（`TimeTicks`）：Linux、macOS 是 `rustix` 的 `clock_gettime`，Windows 是 `QueryPerformanceCounter`（Windows 的那一支這台機器編不到，由 CI 編譯和執行）。
  - renderer 的 `network_agent`、`log_agent`、page agent 的資源和生命週期、`idleness_detector`、`initiator`；Fetch 給 script 看的 header 過濾搬到 `dom_api`。
  - browser：導覽的結果經過 `ContentsEvent::NavigationCompleted` 在 UI thread 上 commit（`CommitNavigation` 帶 `NavigationLoad`），session 送導覽的請求的事件、`Page.navigate` 在 ready-to-commit 回答、`Target.setAutoAttach` 和 `detachFromTarget`；`Network` 的物件的 JSON 在 `protocol::devtools_network`，兩邊共用。
  - rusty_v8 的 fork：`V8Inspector::capture_stack_trace`、`V8StackTrace::is_empty`、`build_inspector_object`（release `kun-v152.2.0-19`）。
- **原型**：
  - 1a 的時間歸屬（`timing-prototype/`，reqwest 0.13.5、hyper-util 0.1.21）：HTTP/1.1 依序、同時、競爭，HTTP/2 cleartext，HTTPS 由 ALPN 選的 HTTP/2，連跑 20 次都通過，時間都記到請求自己開的連線上，重用的不記。結果在原型的 `README.md`。
  - 第 2 段抓 stack：`ci.py v8 --profile release` 只重編 binding，11 秒；沒有 session 打開 `Runtime` 時只有最上面的一個 frame（V8 的 `V8Debugger::captureStackTrace`），打開時整個 stack；`Debugger.setAsyncCallStackDepth` 之後 `await` 之後的 stack 帶 `parent`。I2 做得到，不用退回 I1。
  - 第 3 段開始前用 Chrome 錄了生命週期的規則（`setLifecycleEventsEnabled` 晚打開時補送 `commit` 和到目前為止的事件、每次打開都補送、沒有 `Page.enable` 時不送之後的事件、`Page.disable` 也關掉它們；導覽時 `frameStartedLoading`、`init`、`frameNavigated`；失敗的導覽的回答帶 `loaderId` 和 `errorText`；`detachedFromTarget` 帶 `sessionId`、`targetId`）。
- **做的時候的發現**：
  - iframe 的文件的 `loaderId` 改成它的請求的 id（「已決定的事項」第 11 條）。
  - `<img>` 在 microtask 裡才要圖片，那時 stack 已經空了，所以排 microtask 時就抓（Blink 的 `ImageLoader` 也在排程時記下）；`<link>` 的變動多一種 `ElementChange::Link`。parser 在請求發出之前就結束，它的行號留到那一輪的請求發完。子 frame 換文件時 `frameAttached` 的 stack 跟著帶過去。
  - machopodriver 等的是 browser 在畫面畫出來時送的 `load`（RFC 原本寫它等 `Kun.waitForLoad`，寫錯了），所以另外決定了第 30 條；`Kun.waitForLoad` 帶 session 時等 session 的分頁。`DidDispatchDOMContentLoaded` 沒有人用了，連同它的測試拿掉。renderer 重開之後 browser 補送 `Page.enable` 和 lifecycle。沒有 `src` 的 iframe 照 Chrome 送完整的一輪。`Page.navigate` 只差 fragment 時也重新載入，第 27 條的「不帶 `loaderId`」用不到，記進 `docs/limitations.md`。
  - 舊的測試工具靠 browser 自己送的 lifecycle：`devtools::load` 照 Chrome 的規則暫時打開 `Page`，`automation_pipe` 的 `load` 也等 `Kun.waitForLoad`，`cdp_compare.py` 的 `Run.load` 原本拿到的是補送的舊文件的 `load`，一起改。
  - 測試本身的錯（寫的時候改掉）：同時發出的兩個 `fetch()` 先後不定；先到的事件留在 client 的暫存（加了 `Client::take_events`）；後面的請求已經在前一批裡；`<p>` 的 `style` 屬性蓋過了 `<style>` 的規則。`devtools_network` 連跑 10 次、`devtools_initiators` 和 `devtools_lifecycle` 連跑 5 次都通過。
- **手動確認時找到的錯**：DevTools 的 Reload Page 沒有反應：它送的 `Page.reload` 帶著 `ignoreCache`，browser 只收不帶參數的。改成收 `ignoreCache`、`scriptToEvaluateOnLoad`、`loaderId`，加了 `devtools_network::devtools_reloads_the_page`。
- **mutation test**（每一條最後都抓到）：
  - `mutants-1a.toml`（30 條）：「body 收完的時間等於收到 header 的時間」第一輪沒抓到（測試伺服器的 header 和 body 一起送出），加了 `with_body_delay` 和 `timing::the_response_ends_once_its_body_has_come`。新的導覽、關掉分頁時取消舊導覽的載入測不到，記進 `docs/test-gaps.md`。
  - `mutants-1b.toml`（30 條）：「credentials mode 沒交給 `fetch()` 的回應」沒抓到（只測了過濾的函式），加了 `fetch::a_star_exposes_every_header_but_with_credentials`。
  - `mutants-1c.toml`（11 條）：「被取代的導覽的結果也 commit」沒抓到（平常不會晚到），加了拿住舊導覽的結果的測試。
  - `mutants-2.toml`（34 條）：「空的 stack 也算一個」沒抓到（V8 沒有 frame 也沒有 async parent 時本來就回答 null），加了只有 async parent 的 stack。
  - `mutants-3.toml`（33 條）：四條沒抓到：初始文件沒有 `init`（測試的 iframe 有 `src`，連帶找到上面那個不足，加了 `an_iframe_that_stays_blank_has_loaded`）；`Page.disable` 沒關掉 lifecycle（測試沒讀 socket）；安靜的時間記成結束時（mutant 指定錯了測試，另外加了 shell 的上限）；`Kun.waitForLoad` 不看 session（測試改成送出不等）。
- **和 Chrome 比**：`test.py cdp` 新加 `lifecycle`、`network`、`log`，都和 Chrome 一樣（Chrome 自己要的 favicon 和 paint 的事件除外）；`page` 的已知差異裡「沒有 `frameAttached` 的 stack」拿掉。
- **WPT**（經過 machopodriver）：`fetch/api/` 478 個測試，`multi-globals/construct-in-detached-frame.window.html` 從 FAIL 變成通過（和這份無關，之前的改動沒更新預期結果），更新了預期結果；`fetch/api/redirect/` 46 個、`css/cssom/` 和 `fetch/api/basic/` 210 個、最後的 `css/css-images/image-set/` 和 `fetch/api/basic/` 158 個：都沒有預期之外的結果。
- **`test.py automation`**：第 1 段 55 個情境和 `pipe --window`，最後 56 個，都通過，看過 `demo-script`、`backgrounds/gradient` 的截圖。
- **DevTools 手動確認**（`kun --remote-debugging-port` 開有 stylesheet、圖片、script、iframe、404、module、`fetch()` 按鈕的試用頁）：
  - Network：頁面的請求和 DOMContentLoaded、load 的線，Headers、Response、Timing、圖片的 Preview，redirect（302 和之後的 200 兩列）、gzip、POST 的 Payload、404、連不上、慢的回應、Reload Page。
  - Initiator：parser 的是 `(index):行`，背景圖是 `style.css`，`dep.js` 是 `main.js`；按鈕插入的圖片、iframe 的文件、`fetch()` 滑過去是 stack（`await in loadData` 接著）。
  - Console：兩則載入失敗（連到 Initiator）；選「Slow 4G」時兩則警告（Chrome 152 的 DevTools 送 `Network.emulateNetworkConditionsByRule`（browser 回答）和 `overrideNetworkState`（renderer 回答））。
  - Sources 列出圖片，stylesheet 沒有（前端從 `CSS` domain 拿，記進 `docs/limitations.md`）；Application → Frames 有 Frame Creation Stack Trace，按類型列出子資源。
  - 看到的、這份沒改的：寫在屬性裡的 handler 在 stack 裡是 `(anonymous) @ VM13:27`，不是 Chrome 的 `onclick @ (index):27`（`dom_js` 編譯它時沒給網址和函式名），記進 `docs/limitations.md`。
- **merge**：第 1 段 rebase 到 `main`（0081 做完、0083 的草稿）時 `docs/architecture.md` 的 thread 表有衝突，兩邊都留；0081 的 `ImageData::pending` 補上 `ImageData::shared`。第 2、3 段 rebase 到 `main`（0080、0083 做完）時，0080 把 `<style>`、`<link>` 的 sheet 改成由 `dom` 解析、依文字和 base URL 共用，第 2 段的 referrer 跟著搬過去：`dom` 的 sheet 的 key 和 `style_sheet_contents`、`style_sheet` 改用 `css::UrlContext`（`<link>` 的是 `UrlContext::of_sheet`），`<link>` 的請求帶 0080 的 CORS 設定和第 2 段的發起者。之後 fmt、clippy 乾淨，`cargo test --workspace` 3863 個通過，1 個失敗：0083 的 `layout` 的 `parley_words::system_fonts_shape_words_as_whole_text` 在這台 Mac 的「GB18030 Bitmap」字型上逐字和整段 shaping 的結果不同（0083 在 Windows 上驗收，`layout` 這份沒有改）；WPT `css/cssom/` 290 個測試都照預期。
- **最後的驗證**（第 3 段之後，連結 release 的 `kun-v152.2.0-19`）：`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`；`cargo test --workspace`（3780 個）。各段收尾時的是 3730（第 1 段）、3759（merge 之後）、3770（第 2 段）個。
