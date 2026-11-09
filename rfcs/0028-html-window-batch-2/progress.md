# RFC 0028 的進度：HTML 元素和 Window 的雜項（H2）

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-10。完成：2026-10-10，Fedora 44（Linux x86-64，Rust 1.99.0）。五段都在 `0028` 上做；第 1 到 4 段每一段（第 4 段的三小段各自）做完時 commit、審核，和 RFC 0027、0029 一起 rebase 到 `main` 之後 merge 回去；第 5 段的三小段（5a 到 5c）和 RFC 做完時的整理一起 commit。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. `console`（第 2 條） | 完成，merge 回 `main` | 2026-10-10 |
| 2. 節點和 HTML 元素的小成員（第 3 到 5 條） | 完成，merge 回 `main` | 2026-10-10 |
| 3. `<a>` 和 `<area>` 的網址成員（第 6 條） | 完成，merge 回 `main` | 2026-10-10 |
| 4a. `<img>` 的載入和成員、`Image()`（第 7、10 條） | 完成，merge 回 `main` | 2026-10-10 |
| 4b. `srcset`、`sizes`、`<picture>`，環境改變時重新選（第 8、11 到 13 條） | 完成，merge 回 `main` | 2026-10-10 |
| 4c. lazy loading、`sizes="auto"`（第 9、14 到 16 條） | 完成，merge 回 `main` | 2026-10-10 |
| 5a. `crypto` 和宣告表的 `ArrayBufferView`（第 17 到 19 條） | 完成 | 2026-10-10 |
| 5b. `requestIdleCallback`（第 20 條） | 完成 | 2026-10-10 |
| 5c. `Blob`、`blob:` 網址、bytes 的 body（第 21、22 條） | 完成 | 2026-10-10 |

## 驗收

每一段做完時跑 `CLAUDE.md` 列的檢查：`cargo test --workspace`、`sanity.py` 的四項、`test.py wpt`；動到畫面的第 4 段也跑 `test.py reftest`，第 3 段（`<a>.href` 是 wdspec 的限制裡列的）和 rebase 時跑 `wdspec`、`smoke`。`cargo fmt --check` 和 clippy 在第 1、2、3 段和第 4、5 段結束時跑。

| 項目 | 怎麼驗證 |
| --- | --- |
| **`console`** | |
| 宣告表的 namespace：`namespaces!`、`NamespaceDesc`、`NAMESPACES`；`[JsOnly]` 的不在 `kun:dom` 裡；`spec` 測試的 WebIDL reader 讀 namespace，和 `console.idl` 比對，參數不同、IDL 沒有、表沒有的都找得到 | `cargo test -p dom_api --test wit namespaces_are_left_out`、`--test spec`（`the_table_against_the_specs`、`each_kind_of_difference_is_found`） |
| `console` 是 namespace 物件：不是函式，prototype 是空物件、再上去是 `%Object.prototype%`，class string、descriptor、operation 的 `length`、不能 `new`、拿下來單獨呼叫 | `cargo test -p dom_js --test console console_is_a_namespace_object` |
| 訊息照 Chrome 印到終端機的：引數怎麼寫（物件、陣列攤平和循環、symbol、BigInt、函式、錯誤、`undefined`）、不套用 `%s`、沒有引數時；`assert`、`count`、`countReset`、`group`、`trace`、`time`、`timeLog`、`timeEnd` 和它們的警告；count map 和 timer table 每個 realm 一份 | `cargo test -p dom_js --test console`（`messages_are_written_as_chrome_writes_them`、`assert_count_group_and_timers`） |
| 頁面的 `console` 經過 renderer 到 stderr（`[console] …`），不在 pipe 上 | `cargo test -p shell --test automation_pipe the_pages_output_stays_off_the_pipe` |
| WPT 的 `console/`：12 個，6 個整份通過，其他的停在 testdriver 的 BiDi log 和 `fetch` | `python3 scripts/test.py wpt console/` |
| Speedometer：NewsSite-Next、NewsSite-Nuxt、Charts-chartjs 不再停在 `console.error`，印出它們真正的錯誤 | `python3 scripts/ci.py speedometer --inventory --reuse-chrome --suites NewsSite-Next,NewsSite-Nuxt,Charts-chartjs`（看 `kun.log` 的 `[console]`） |
| **節點和 HTML 元素** | |
| 宣告表的別名：`legacy_window_alias(Name)`，JS 的 global 上是同一個 interface object、不 enumerable、做得出能用的 observer；`spec` 測試讀 IDL 的 `[LegacyWindowAlias]`（一個或一串），表有、IDL 沒有的是差異，IDL 有、表沒有的列進 `spec-missing.txt` | `cargo test -p dom_js --test bindings a_legacy_window_alias_is_the_interface_object`、`cargo test -p dom_api --test spec`（`legacy_window_aliases_are_read`、`each_kind_of_difference_is_found`） |
| `HTMLElement` 的 global attribute 和 Chrome 152 對同一段 script 的結果一樣：reflection、`translate` / `spellcheck` / `isContentEditable` 的繼承（SVG 元素不算）、`hidden` 的 union、`draggable` 的預設、`autocapitalize` 和 form owner、`contentEditable` 的關鍵字和 `SyntaxError` | `cargo test -p dom_js --test html_element` |
| `isConnected`、`template.content`、`getElementsByName()`（live、只有 HTML 元素、`NodeList`）、`document.dir` | `cargo test -p dom_js --test html_element nodes_templates_names_and_the_documents_dir` |
| 複製的元素有 event handler content attribute 給的 handler（template 的內容複製出來的也是），`<input>` 的 checkedness 照舊複製 | `cargo test -p dom_js --test bindings clone_compare_and_processing_instructions`、WPT 的 `the-input-element/clone.html` |
| 分頁的可見性：commit 之前就隱藏的分頁的第一份文件是 `hidden`，沒有 frame 的文件是 `hidden`；顯示和隱藏時每份文件（主 frame 先）收到 trusted、bubble、不能取消的 `visibilitychange`，那時都已經是新的狀態；告訴它已經是的狀態時不發 | `cargo test -p browser --test navigation documents_have_the_tabs_visibility` |
| 背景開的分頁（連結的中鍵、`open_tab` 的 background）在 commit 之前隱藏，切過去之後是 `visible` | `cargo test -p shell --test browser_view a_tab_opened_behind_starts_hidden` |
| `kun:dom` 和 Lua：只有值的 union 可以同時是參數和回傳值；Lua 的 boolean、數字、字串、`nil` 進出 `hidden` | `cargo test -p dom_api --test wit`、`python3 scripts/test.py reftest wasm/w9`（第 23 項） |
| WPT：`html/dom/reflection-*.html`、`translate`、`autocapitalize`、`contenteditable`、`spellcheck`、`getElementsByName`、`document-dir.html`；每個元素的 global attribute 都通過（`nonce` 除外） | `python3 scripts/test.py wpt html/dom/reflection html/dom/elements/global-attributes/ html/editing/editing-0/ getElementsByName` |
| **`<a>` 和 `<area>`** | |
| URL 成員和 Chrome 152 對同一段 script 的結果一樣（照規範的三處除外）：每個 getter，在完整、沒有、解析不了、opaque path、相對的網址上的每個 setter，`<base>` 的 base URL，stringifier | `cargo test -p dom_js --test html_element links_url_members` |
| 不能改網址的 setter 不寫 `href`（前後有空白的 `href` 照原樣） | `cargo test -p dom_js --test html_element a_setter_that_cant_change_the_url_leaves_the_href` |
| 兩個元素自己的成員、`relList` 支援的 link type、`referrerPolicy` 的值；`document.links`（HTML 的 `<a>`、`<area>` 有 `href` 的）和 `anchors`（有 `name` 的 `<a>`），每次同一個 | `cargo test -p dom_js --test html_element links_own_members_and_the_documents_links` |
| WPT 的 URL 資料（`urltestdata`、`setters_tests`）經過 `<a>` 和 `<area>`：失敗都是 `url` crate 的，列在 golden file | `cargo test -p dom_api --test url_data` |
| WPT：`the-a-element/`、`links-created-by-a-and-area-elements/`、`document.links.html`、`url/javascript-urls.window.js`；reflection 檔案裡 `<a>` 全部通過、`<area>` 只剩 `hreflang` 和 `type` | `python3 scripts/test.py wpt the-a-element links-created-by-a-and-area document.links javascript-urls html/dom/reflection` |
| **`<img>`：載入和成員** | |
| 和 Chrome 152 對同一個頁面的結果一樣（逐行）：載入中和載入後的 `complete`、natural size、`currentSrc`，快取的圖當場可用（`load` 仍在 task），壞圖、404、空的、拿掉的、解析不了的 `src`，有沒有 box 時的 `width` / `height`，`decode()`（可用、壞、沒有 `src`、呼叫後換 `src`、載入中換 `src`），同一個 task 裡兩次更新只有一個事件，載入中換 `src`，同一個 `src` 再設，reflection，`Image()` | `cargo test -p browser --test navigation img_elements` |
| `<img>` 的 request 延後 window 的 `load`，不在文件裡的 `new Image()` 也是（Chrome 152 一樣） | `cargo test -p browser --test navigation img_loads_delay_the_window_load` |
| `loading`、`decoding`、`fetchPriority` 沒有 attribute 和無效值時（`loading` 照規範是 `eager`，Chrome 是 `auto`） | `cargo test -p dom_js --test html_element img_enumerated_reflections` |
| 錯誤狀態碼的回應但內容是圖時照樣顯示 | `cargo test -p shell --test network an_image_with_an_error_status_shows` |
| WPT：`the-img-element/` 整個目錄；設了 `src` 之後插入的 `<base>` 也算、已經有的圖當場可用和當場 layout、404 的圖 | `python3 scripts/test.py wpt the-img-element` |
| **`<img>`：`srcset`、`sizes`、`<picture>`** | |
| media query 和 `sizes`：feature、範圍寫法、單位、Level 4 的三值邏輯（和 Chrome 的 `matchMedia` 比過）、`<any-value>` 裡沒配對的括號、正方形是 portrait、`sizes` 的每一項 | `cargo test -p style --lib media` |
| `srcset` 的 parser（HTML 的每一步、描述子的錯誤）和選候選（密度、幾何平均、有 `w` 時不算 `src`、已經載入的比較密的優先） | `cargo test -p dom_api --lib srcset` |
| 和 Chrome 152 對同一個頁面的結果一樣（逐行）：`x` 和 `w` 描述子、`sizes`、`<picture>` 的 `media` 和 `type`、壞的和空的 `srcset`，natural size 和 layout 的大小除以密度，800 寬的 1x、2x；resize 到 500 寬之後重新選（同一張只改密度）、`<source>` 的 `media` 改了之後 | `cargo test -p browser --test navigation srcset` |
| 新建的 iframe 的 `about:blank` 在插入時的 `onload` 裡照父 document 建立時的 base URL 解析，之後改了 `<base>` 也一樣 | `cargo test -p browser --test navigation a_new_iframes_about_blank_has_its_parents_base_url` |
| 子 frame 裡的 `<picture>` 一開始就照 iframe 的大小選（還沒 layout 的子 frame 先 layout），不會先抓窄的那張 | `cargo test -p browser --test navigation a_child_frames_images_select_for_its_iframes_size` |
| 載入中把 `src` 設成同一個網址：留著同一個 request，`decode()` 照樣 resolve、只有一次 `load`（Chrome 152 實測） | `cargo test -p browser --test navigation img_elements` |
| WPT：`the-img-element/` 的 `srcset/`、`sizes/`、relevant mutation（`<picture>`、`<source>`）、`update-media.html`、`adoption.html`、`null-image-source.html`、`decode/`；iframe 的大小改變時重新選（`srcset-w-reselect-on-resize.html`）；`reflection-embedded.html` 的 `<source>` | `python3 scripts/test.py wpt the-img-element reflection-embedded` |
| **`<img>`：lazy loading 和 `sizes="auto"`** | |
| 和 Chrome 152 對同一個頁面的結果一樣（逐行）：viewport 裡的 lazy 圖不延後 `load`、遠的等著（`complete` false）、捲到附近才載、iframe 在畫面外時裡面的不載、側邊 625px 內的載、改成 eager 立刻載（只一次）、`sizes="auto"` 用 content box、寬度變了重新選、不是 lazy 的 `auto` 是 `100vw`；抓了的網址也一樣（`display: none` 的、不在文件裡的沒抓） | `cargo test -p browser --test navigation lazy_images` |
| WPT：lazy loading 的那些（不靠 `vh` 的都通過）、`sizes/sizes-auto*.html`、`img.complete.html` | `python3 scripts/test.py wpt the-img-element` |
| **`crypto` 和宣告表的 `ArrayBufferView`** | |
| `crypto` 和 `QuotaExceededError` 和 Chrome 152 對同一段 script 的結果一樣：每一種 typed array、原地寫入和 offset、detach、長度的邊界、不是 view 的參數、`randomUUID` 的版本；`QuotaExceededError` 的建構子、屬性和原型鏈 | `cargo test -p dom_js --test crypto` |
| `window.crypto` 每次同一個、window 和它互相撐住；每個 GC 型別都有 wrap / unwrap 的測試 | `cargo test -p dom --test crypto --test script_objects --test script` |
| wasm：guest 的 `typed-array` 寫好交回、元素型別不變、錯誤的代碼 20、21；Lua 的 table 形式 | `cargo test -p dom_wasm --test engine a_guest_uses_crypto`、`cargo test -p shell --test wasm_reftests` |
| WPT：`WebCryptoAPI/getRandomValues.any.js`、`randomUUID.https.any.js`、`historical.any.js` 整份通過（worker 版本除外），`idlharness.https.any.js` 只剩 `subtle` | `python3 scripts/test.py wpt WebCryptoAPI/` |
| **`requestIdleCallback`** | |
| 和 Chrome 152 對同一個頁面的結果一樣（逐行）：沒有東西要畫時剩不超過 50 ms、callback 裡排的到下一段才跑、`requestAnimationFrame` 一直在跑時剩不到一個 frame、`timeout` 在連續的 `setTimeout(0)` 忙完之前就跑（`didTimeout`、剩 0）、取消的不跑、順序、`IdleDeadline` 的成員、建構子和 `length` | `cargo test -p browser --test navigation idle_callbacks` |
| 宣告表的成員層級 `[JsOnly]`：不進 `kun:dom`；`requestIdleCallback`、`IdleRequestOptions` 和 `requestidlecallback.idl` 比對 | `cargo test -p dom_api --test wit`、`--test spec` |
| WPT：`requestidlecallback/` 20 份，14 份整份通過；`deadline-max-*-dynamic.html` 兩份和 Chrome 一樣失敗（deadline 固定），跨 realm 的 `timeRemaining`、`window.open`、XHR 的沒有，跨源的子 frame 要 `postMessage`（disabled） | `python3 scripts/test.py wpt requestidlecallback/` |
| **`Blob`、`blob:` 網址和 bytes 的 body** | |
| `Blob` 和宣告表的 bytes 和 Chrome 152 對同一段 script 的結果一樣：各種 part（字串、typed array、`ArrayBuffer`、`DataView`、blob、其他值）、不是 iterable 的、`endings`、type 的正規化、`slice` 的正負、`NaN`、`[Clamp]`；resizable 的 buffer（`getRandomValues` 也是）；body 的 `Content-Type` | `cargo test -p dom_js --test blob` |
| 和 Chrome 152 對同一個頁面的結果一樣（逐行）：讀取在 microtask 之後的 task、`Response.blob()` 的 type、`arrayBuffer()` / `bytes()`、`fetch()` 一個 blob URL（status、headers、range、fragment、`POST`、沒有 type）、撤銷前後、`<img>` / `<script>` / iframe 設好 `src` 之後立刻撤銷、子 frame 的網址跟著文件走、另一份文件撤銷、主 frame 導覽到剛撤銷的 blob URL（origin 是頁面的），之後再導覽過去是錯誤頁 | `cargo test -p browser --test navigation blobs` |
| `createObjectURL` 的網址（origin、`null`、每次新的 UUID）登記給 frame；只撤銷同 origin 的 | `cargo test -p dom_api --test blob_urls` |
| renderer 的 `BlobUrls`：登記當場送、撤銷在 renderer 當場生效、flush 才送到 Browser、丟棄的文件的網址 | `cargo test -p renderer --lib loader` |
| Browser 的 blob URL 表：只給登記的 renderer、導覽不限、撤銷的兩步、renderer 結束時清掉、`GET` 以外、range 的每一種、fragment | `cargo test -p net --lib blob` |
| `BlobData` 的 slice 共用 bytes；`Blob` 的 wrap / unwrap | `cargo test -p dom --lib blob`、`--test script_objects --test script` |
| wasm：`blob-part`、guest 的 bytes、`s64`、讀取的 `async func`、`body-init` 的 blob 和 bytes、`create-object-url`；Lua 的 W9 第 33 項 | `cargo test -p dom_wasm --test engine a_guests_blobs`、`cargo test -p shell --test wasm_reftests` |
| WPT：`FileAPI/blob/`、`FileAPI/url/`、`fetch/range/blob.any.js`，以及 `fetch/api/` 原本因為沒有 `Blob` 整份 ERROR 的 | `python3 scripts/test.py wpt FileAPI/blob FileAPI/url fetch/range/blob.any.js fetch/api/basic fetch/api/request fetch/api/response` |

## 做過的檢查

- **決定**：第 1 到 22 條，A、B 在開始前，其他在各段開始前確認。實作時改了、寫進那一條的：第 5 條（可見性的狀態放在整頁、背景分頁先 `was_hidden` 再導覽）、第 15 條（子 frame 的 lazy 圖對最上層的 viewport 算，實測 Chrome）、第 21 條（Browser 照 renderer 檢查子資源，撤銷分兩步）。X3、F1c 是使用者看過利弊（F1c 的缺點）之後選的。
- **實測 Chrome**：每一段都先在 Chrome for Testing 152 跑同一段 script 或頁面，測試照它的結果比對（`details.md` 每一段一節；browser 測試的頁面在 `crates/browser/tests/resources/`，Chrome 跑三次一樣才用）。照 Chrome、不照規範的：`console` 照它印到終端機的樣子（不套用 `%s`、group 不縮排）；`autocapitalize` 不認得的值、`isContentEditable` 不在文件裡也算；`<img>` 的 `currentSrc` 在圖來之前是空的、選不到來源時只有有 `src` 屬性才 `error`、同一個網址只換密度不重新載入、留著同一張圖的 request；lazy 的圖不延後 `load`、不是 lazy 的 `sizes="auto"` 是 `100vw`；`QuotaExceededError` 不檢查負數；`requestIdleCallback` 的 deadline 在 period 開始時固定；`Response.blob()` 的 type、blob 的 `Content-Type`、`Range` 單位的大小寫。照規範、不照 Chrome 的：`<a>` 的三處（`protocol` 不在 special 和其他 scheme 之間切換、`file:` 的 `origin`、不能改網址的 setter 不寫 `href`）、`<img>` 的 `loading` 的預設值。
- **WPT 找到的 bug**（修了才收）：namespace 物件的 prototype 少一層；`cloneNode` 複製 attribute 沒有跑 change steps（`main` 上本來就有：複本沒有 `onclick` 的 handler），放在 cloning steps 之前（之後的話 `<input>` 的 checkedness 被重設）；`<img>` 的網址解析的時機、`decode()` 的時機、`width` 當場更新、404 的圖照樣顯示、adopt 之後 load event delay count 減錯文件；`<picture>` 的 relevant mutation 太寬、`sizes` 的 `<general-enclosed>` 收了沒配對的 `]`；script 建的 iframe 的 `about:blank` 沒有 base URL（`main` 上本來就有）、還沒 layout 的子 frame 的 viewport 是 0×0；lazy 圖恢復時重新解析網址、`complete` 只看 `src`；`console` 印一千萬個元素的陣列（wptrunner 的 CRASH，照 inspector 的上限）；`requestIdleCallback` 的 timeout 被連續的 `setTimeout(0)` 擋住（timer 一次跑完，改成照到期時間交錯）；iframe 移除之後同一個 task 裡還 fetch 得到它的 blob URL、`fetch()` 不收 `blob:`。
- **和 RFC 0027、0029 rebase**（merge 前）：`main` 多了 RFC 0027 的 `Response`、`fetch()`、`Request`、CORS 和 RFC 0029 的 wptrunner。六個 commit 逐一 rebase，每個都轉成 `include.txt` 和 `metadata/` 的 `.ini`，自己就過得了它的 WPT；`Task::SettlePromise` 換成 `ReadBody` 和 `Reject`，`FrameSettings` 兩邊各加的欄位，W9 兩邊加的項目重新編號；文件的長表格列用切在標點的三方合併。第 5 段 merge 前再 rebase 一次，到 RFC 0027 的收尾、RFC 0030、0031 之後的 `main`：`UserActivation`（另一種 GC 型別）和 `Crypto`、`Blob` 在同樣幾個清單上衝突（GC 型別變成 33 種），`web_contents.rs` 的 scheme 判斷改成 `match`（擋 `data:`，加上 `blob`）；編好的 guest 照合併後的 WIT 重編。`main` 有了同源的 `postMessage`，`cross-global-revoke.sub.html` 的同源撤銷和 `history_pushstate_url_rewriting.html` 的 sandboxed 幾項跟著通過，`FileAPI/url/sandboxed-iframe.html` 從 ERROR 變成等滿 long timeout（sandbox 的 iframe 是跨源，`postMessage` 不到），設成 disabled。
- **WIT 和 Lua**：`kun:dom` 每一段都只有新增，不升版；`ci.py guests` 每次照新的 WIT 重編。Lua 的產生器補了只有值的 union、回傳 record、bytes 的欄位、`s64`、可以省略的物件或字串的陣列、`list<u8>` 的 `async func` 結果，enum 的 case 在說明的行首也認得；union 裡的 bytes 的 case 不給 Lua 用（Lua 的字串走字串的 case）。`dom` 的 compile-fail 預期輸出重新產生過一次（help 列的型別多了 `BlobData`，錯誤的種類沒變）。
- **mutation test**（`mutants-1.toml` 到 `-5c.toml`：33、45、19、20、39、22、19、14、58 條）：除了一條，最後都抓到。補了測試之後才抓到的：第 1 段的 namespace 的三種差異、stderr 的輸出；第 2 段的 `<object>` 不繼承 `autocapitalize`；第 4 段的 10 條（同一個 task 兩次更新、`decode()` 再換 `src`、`loading` 的無效值、三值邏輯、正方形是 portrait、`w` 時不算 `src`、同一個網址再設、新 iframe 的 base URL、非 lazy 的 `auto`、`auto` 扣 padding）；`requestIdleCallback` 的 `Timers` 的 3 條（頁面上和 Chrome 比不出來，改用 `dom` 的單元測試）；`Blob` 的 2 條（`arrayBuffer()` 的 constructor、撤銷別的 origin 的網址）。等價、從清單拿掉的（`image_fetched` 的網址檢查、`write_view` 只寫到 view 的長度、timeout 時直接回 0、JS 的 `[Clamp]` 的 `NaN` 等）和理由，在各段 commit 時的這份進度裡（git 的紀錄）。沒抓到的一條（Browser 的 IO thread 標記撤銷）記進 `docs/test-gaps.md` 的「測試守不住的地方」。fmt 和後面的小段改了程式時，對不上的 `from` 照現在的程式改寫、整份重跑。每次跑完都確認檔案還原了。
- **WPT**：加了 `console/`、`html/dom/reflection-*.html` 等 global attribute 的、`the-a-element/` 和 `links-created-by-a-and-area-elements/`、`the-img-element/` 整個目錄、`WebCryptoAPI/` 的 4 份、`requestidlecallback/`、`FileAPI/blob/` 和 `FileAPI/url/`、`fetch/range/blob.any.js`。等 kun 沒有的功能而整份逾時的是 `disabled`，寫了原因；靠 `vh` 的 `lazy-out-of-band-load.html` 的四項記成 `[FAIL, PASS]`。原本就跑的測試跟著變好、更新了預期結果（reflection 的 `<a>`、`<img>`、`<source>`，`url-setters-a-area`，`fetch/api/` 原本因為沒有 `Blob` 整份 ERROR 的，用 blob URL 建 iframe 的 `scrollingElement.html` 等五份）；沒有退步。wdspec 有 17 個 subtest 因為 `<a>.href`、`isConnected` 變成通過。
- **找到、修掉的既有問題**（不是這份的功能，但它的測試碰到）：`a_click_on_the_old_page_doesnt_reach_the_new_one` 在機器忙時失敗（4c 原本每次 rendering update 都看 DOM 變了沒有，改成只在通知了 `dom_api` 時看）；`shell` 的 `focus` 測試在負載下偶爾截到空標題（標題比 `ReftestWaitEnded` 晚送：改成 `BeginMainFrame` 在 animation frame callback 之後、commit 之前就送，Blink 的 `DidChangeTitle` 也是當下送；負載下 40 次失敗 1 次變成 80 次都通過）；`requestIdleCallback` 的測試本身的兩個時序問題（測試的 Viz 連續發 `BeginFrame`、coarsen 的 0.1 ms）。
- **不穩定的測試**（都在沒有這份的 `main` 上比過，是既有的，原因還沒查的寫在這裡）：`windowless` 的 `a_change_the_input_made_is_waited_for`（整個 binary 6 到 10 次失敗一次）；`automation_pipe` 的截圖（20 次 4 次，截圖還是改之前的畫面）；`webdriver` 的 `elements_are_found_after_an_implicit_wait`（機器忙時）；WPT 的 `focus-dynamic-type-change*.html`、`timer-nesting-not-inherited-in-microtask.html`；wdspec 的 `test_link_from_toplevel_context_with_target[_blank]`。`set_window_rect` 的 `test_partial_input` 是 Wayland 上的限制。`dom_wasm` 的 `a_guest_resumes_while_the_dom_calls_it`（RFC 0027 的測試）在 fuzz 的一組設定裡失敗一次：guest `wait` 的 timer callback（`run(2)`）讀到已經拿掉的 `BODY`（`set in start`）。timer 的那次呼叫裡 wasmtime 一直 poll 到 store 沒有工作，等著的 task 若比 export 的呼叫先排上，就一路讓出、dispatch、跑完、拿掉 `BODY`，`run(2)` 才進去；guest 假設呼叫先跑，靠的是 wasmtime 的排程順序（fuzz 的重跑、一般 build 600 次、debug V8 100 次都沒重現）。merge 之後修了：timer 有自己的一份 body（`TIMER_BODY`，`start` 再向 document 拿一次，callback 用完拿掉），哪個先跑都一樣；加了先讓 task 跑完、再跑 timer 的 `a_guests_timer_runs_after_its_task_is_done`，舊的 guest 在它上面一定失敗（同樣是 `set in start`），新的通過。
- **Speedometer**（每段做完時 `ci.py speedometer --inventory --reuse-chrome`，前一次的放在 `target/rfc0028/speedometer-before-*`）：第 1 段之後 NewsSite-Next、NewsSite-Nuxt、Charts-chartjs 不再停在 `console.error`，印出真正的錯誤；5a 之後 NewsSite-Next 從逾時變成跑完 3 個測試；這份的成員都不在缺少的清單裡了，runner 量完之後的 `Blob is not defined` 也沒有了。其他 suite 停在別的代號（`CSSStyleSheet`、`SVGElement`、Canvas、`Selection`、`alert`）。
- **完成時的驗證**（2026-10-10，Fedora 44，Rust 1.99.0）：`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`（沒有警告）、`cargo test --workspace` 1836 個全部通過（rebase 之後 1879 個，只有 4a 記下的 `elements_are_found_after_an_implicit_wait` 失敗一次，單獨重跑 5 次都通過）、`sanity.py deps`、`boundary`、`unsafe`、`docs`（只有 `main` 上既有的 RFC 0021 的 `progress.md` 那一項）、`test.py wpt`（只有上面那些既有的不穩定測試）。GC（`Crypto`、`Blob` 兩種新的 GC 型別）：`test.py matrix` 的 8 種設定都通過（832 個測試，用 debug 的 V8）；`test.py fuzz --iterations 20`（`third_party.toml` 改釘 fork 的 `8703527` 之後：原本釘的 `5c8e7a1` 不在本機的 fork 裡，`8703527` 在 fork 的 `origin/kun` 上，`ci.py third-party` 確認預編的 V8 不受影響）20 組裡 19 組通過，1 組（`concurrent`、4 條 thread、V8 的預設 flag）是 `dom_wasm` 的 `a_guest_resumes_while_the_dom_calls_it` 的測試 guest 自己的時序問題，見「不穩定的測試」（已修）。
