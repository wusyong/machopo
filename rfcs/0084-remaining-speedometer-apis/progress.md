# RFC 0084 的進度：Speedometer 用到、kun 還沒有的 API

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-24，Fedora 44。在 `0084` 上做。

完成日期：2026-10-25，Fedora 44（AMD Ryzen AI 9 HX 370）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a `window.opener` | 完成 | 2026-10-24 |
| 1b `MouseEvent` 的 `fromElement`、`toElement` | 完成 | 2026-10-24 |
| 1c `navigator.connection` | 完成 | 2026-10-24 |
| 1d `navigator.geolocation` | 完成 | 2026-10-24 |
| 1e `navigator.wakeLock` | 完成 | 2026-10-24 |
| 1f `document.prerendering` | 完成 | 2026-10-24 |
| 2a `isInputPending()`：renderer 的計數、鍵盤和最上層的 document、`navigator.scheduling` | 完成 | 2026-10-24 |
| 2b `isInputPending()`：paint 的對象區域、compositor 的判斷 | 完成 | 2026-10-24 |
| 3a `IntersectionObserver` v2：屬性、檢查、`delay` 的節流 | 完成 | 2026-10-24 |
| 3b `isVisible`：扭曲的效果 | 完成 | 2026-10-24 |
| 3c `isVisible`：範圍的 hit test、蓋住的判斷 | 完成 | 2026-10-24 |
| 4 `File`、`FileReader` | 完成 | 2026-10-24 |
| 第 4 段找到的：callback 之後的 microtask checkpoint、timer 的時間 | 完成 | 2026-10-24 |
| 5 `document.all`（fork 的 `MarkAsUndetectable`） | 完成 | 2026-10-24 |
| 6a `PerformanceObserver` | 完成 | 2026-10-25 |
| 6b Navigation Timing、`performance.timing`、`performance.navigation` | 完成 | 2026-10-25 |
| 盤點找到的：flex 和 grid container 的 shrink-to-fit 寬度 | 完成 | 2026-10-25 |

## 驗收

預期值是 Chrome 152 的（`details.md`，探測的頁面在 `probes/`）。

**小的 API**

| 項目 | 怎麼驗證 |
| --- | --- |
| `opener` 是 `null`、window 自己的 accessor，賦值（`null`、`undefined` 也算）變成自己的 data property；setter 的名稱、`length` 和 `this` 的檢查 | `cargo test -p dom_js --test it bindings::opener_is_null_and_replaceable` |
| `fromElement`、`toElement`：沒有分派的、`mouseout`、`mouseleave`、`mouseover`、`click`，target 或 `relatedTarget` 是 window 時是 `null`；在 `MouseEvent.prototype` 上 | `cargo test -p dom_js --test it bindings::from_element_and_to_element` |
| `navigator.connection`：同一個 `NetworkInformation`、是 event target，`4g`、`rtt` 0、`downlink` 10、`saveData` `false`、`onchange` 是 `null`，沒有 `type`；prototype 的成員照 Chrome 的順序；不能 `new` | `cargo test -p dom_js --test it navigator::connection` |
| Geolocation 的 interface：成員照 Chrome 的順序、參數錯誤是 `TypeError`、`GeolocationPositionError` 不能 `new` | `cargo test -p dom_js --test it geolocation::interfaces` |
| 每個請求都被拒絕：比請求晚一個 task 到；`timeout` 0（負的 clamp 成 0）先 timeout，watch 之後也被拒絕；一次性的先、watch 的後；`clearWatch` 的拿不到；watch id 從 1 起；同一個錯誤物件、JSON 是 `{}`；callback 丟的例外照樣回報；拒絕之後再要一次權限 | `cargo test -p dom_js --test it geolocation::every_request_is_denied` |
| 頁面看不見時的請求等到看得見才開始 | `cargo test -p dom_js --test it geolocation::a_hidden_page_waits` |
| navigator 撐住 `connection` 和 `geolocation`；請求的 callback 在等的期間活著、失敗之後被回收；一次性的、timeout、`clearWatch`、隱藏時的請求、權限的詢問 | `cargo test -p dom --test it navigator::` |
| 兩個新的 GC 型別：slot 撐住、只當自己的型別讀回、wrapper 只 unwrap 成自己 | `cargo test -p dom --test it script_objects::geolocation script_objects::geolocation_position_error script::every_type_unwraps_only_as_itself` |
| Wake Lock 的 interface：成員照 Chrome 的順序、sentinel 是 event target、不能 `new`、`system` 是 `TypeError` | `cargo test -p dom_js --test it wake_lock::interfaces` |
| lock 比請求晚兩個 task 給（權限的一來一回）、預設 `screen`；`release()` 在回傳前發 trusted、不 bubble 的 `release`，`released` 已經是 `true`，每次一個新的 promise；兩個 lock 是兩個物件 | `cargo test -p dom_js --test it wake_lock::a_lock_is_granted_and_released` |
| 頁面看不見時在 `visibilitychange` 之前 release 全部的 lock；看不見時的請求當場拒絕，回答時已經看不見的也拒絕 | `cargo test -p dom_js --test it wake_lock::a_hidden_page_has_no_locks` |
| `WakeLock` 撐住 active 的 lock、照取得的順序列出，release 之後放掉、其他的接回去，第二次 release 沒有作用 | `cargo test -p dom --test it navigator::a_wake_lock_keeps_its_locks_until_they_are_released` |
| `document.prerendering` 是 `false`；`onprerenderingchange`、`onvisibilitychange` 是 document 的 event handler | `cargo test -p dom_js --test it bindings::a_document_is_never_prerendering` |
| WPT 的 `netinfo/`、`geolocation/`、`screen-wake-lock/` 照 `.ini` 的預期（新加進清單） | `python3 scripts/test.py wpt netinfo geolocation screen-wake-lock` |

**`isInputPending()`**

| 項目 | 怎麼驗證 |
| --- | --- |
| `navigator.scheduling`：同一個 `Scheduling`、不能 `new`；對象是這個 document 的看得到，不在的 node 的、沒有 focus 時的鍵盤看不到 | `cargo test -p dom_js --test it scheduling::` |
| 計數：交出時加、取出時減，連續的另外算，「不知道」的不記 | `cargo test -p renderer --lib pending_user_input` |
| 整個瀏覽器裡，忙著的頁面看得到等著的按下、鍵盤；移動只在 `includeContinuous` 時，取出之後到 frame 分派之前還算；同 origin 的 iframe 上的看得到，跨 origin 的、蓋在它上面的看不到，旁邊的看得到；成為 layer 的容器裡的照樣 | `cargo test -p shell --test it scheduling::` |
| 對象區域：蓋住的規則（`pointer-events: none`、`visibility: hidden` 不蓋，空的、`opacity: 0` 的蓋）、mask、子 frame 裡的子 frame、成為 layer 的容器、clip | `cargo test -p paint --test it child_frames::` |
| compositor 照點、捲動容器、sticky 和 viewport 的捲動找對象，放棄的規則 | `cargo test -p compositor --lib input_is_for a_rect_in_a` |
| WPT 的 `is-input-pending/` 照 `.ini` 的預期（新加進清單） | `python3 scripts/test.py wpt is-input-pending` |

**`IntersectionObserver` v2**

| 項目 | 怎麼驗證 |
| --- | --- |
| 屬性：`delay` 往 0 取整、飽和，`trackVisibility`；`trackVisibility` 配上小於 100 的 `delay` 是 `NotSupportedError`、比 margin 先檢查；prototype 的成員照 Chrome 的順序；entry 的 `isVisible` 必填 | `cargo test -p dom_js --test it intersection_observers::` |
| `isVisible` 變了帶來 entry、不相交的不算看得見、entry 的 `time` 是算的那一刻；observation 記下上次算的時間和 `isVisible` | `cargo test -p dom_js --test it intersection_observers::` 和 `cargo test -p dom --test it intersection_observers::` |
| `delay`：兩次計算至少隔那麼久，到期時有 frame 來算 | `cargo test -p shell --test it intersection_observers::a_delay_holds_the_next_computation_back` |
| 扭曲的效果：opacity、filter（祖先的、子 frame 外面的）、只有平移和兩軸一樣的放大 | `cargo test -p paint --test it intersection::what_distorts` |
| 蓋住：Chrome 152 實測的 21 種、子 frame、捲動過和 fixed、SVG text、iframe 當 target | `cargo test -p paint --test it intersection::what_covers intersection::an_iframe_as_the_target` |
| 整個瀏覽器裡：扭曲、蓋住、之後才加的 shadow、target 裡面的不算 | `cargo test -p shell --test it intersection_observers::` |
| WPT 的 `intersection-observer/`（`v2/` 新加進來）照 `.ini` 的預期 | `python3 scripts/test.py wpt intersection-observer` |

**`File`、`FileReader`**

| 項目 | 怎麼驗證 |
| --- | --- |
| `File`、`FileReader`、`ProgressEvent` 的 interface、建構子的錯誤；檔案的名稱、時間、type；讀的事件和四種結果；讀著時再讀、`abort()`、handler 裡開始新的讀；解碼的優先順序 | `cargo test -p dom_js --test it file_reader::` |
| reader 的狀態和讀的編號、`File` 的資料 | `cargo test -p dom --test it file_readers::` 和 `cargo test -p dom_api --test it files::` |
| 整個瀏覽器裡：現在的時間、經過 event loop 讀 | `cargo test -p shell --test it file_reader::` |
| WPT 的 `FileAPI/` 照 `.ini` 的預期（`file/`、`FileReader/`、`reading-data-section/` 新加進清單） | `python3 scripts/test.py wpt FileAPI` |

**callback 之後的 microtask checkpoint、timer 的時間**

| 項目 | 怎麼驗證 |
| --- | --- |
| task 分派的事件的每個 listener、event handler、observer 的 callback 回來之後跑 microtask，script 裡的 `dispatchEvent` 不跑；先回報 `error`，兩個 `error` listener 之間不跑 | `cargo test -p dom_js --test it dispatch::microtasks_after_each_listener_of_a_task`；`cargo test -p bindings --lib the_outermost_entry_into_script` |
| timer 的 microtask 裡設的 timer 不算巢狀在那個 timer 裡 | `cargo test -p shell --test it script::a_timer_set_in_a_microtask_is_not_nested` |
| timer、DOM 的 task、idle callback 的 timeout 從設定、排入的那一刻算（每次要時間時讀時鐘） | `cargo test -p dom --test it timers::the_clock_is_read_when_asked`；`cargo test -p shell --test it script::a_timer_counts_from_when_it_is_set` |
| WPT：`filereader_events.any.js`、`filereader_abort.any.js`、`checkpoint-after-window-onerror*.html`、`evaluation-order-1.html`、`timer-nesting-not-inherited-in-microtask.html` | `python3 scripts/test.py wpt FileAPI html/webappapis` |

**`document.all`**

| 項目 | 怎麼驗證 |
| --- | --- |
| `typeof` 是 `"undefined"`、`== null`、轉成 boolean 是 `false`；可以當函式呼叫；`length`、index、`item`（array index 的字串照 index）、`namedItem`；`id` 和 14 種元素的 `name`，空字串也是名稱；好幾個時是 `HTMLCollection`，同一個名稱是同一個；prototype 的成員照 Chrome 的順序 | `cargo test -p dom_js --test it document_all::` |
| `document.all` 由節點撐住、每次同一個；同名的集合弱指著 | `cargo test -p dom --test it collections::a_node_keeps_each_of_its_collections collections::the_all_named_collection_is_kept_weakly` |
| array index 的判斷 | `cargo test -p dom_api --lib collection::tests::array_indices` |
| WPT 的 `htmlallcollection.html`、`document-all.html`（新加進清單），wdspec 的 `test_html_all_collection` | `python3 scripts/test.py wpt html/infrastructure/common-dom-interfaces/collections`；`test.py wdspec webdriver/tests/classic/execute_script/collections.py webdriver/tests/classic/execute_async_script/collections.py` |

**`PerformanceObserver`**

| 項目 | 怎麼驗證 |
| --- | --- |
| interface、參數錯誤的 `TypeError` 和 `InvalidModificationError`、console 的警告、`supportedEntryTypes` 凍結、每次同一個 | `cargo test -p dom_js --test it performance_observers::the_interface_and_its_errors` |
| 一個 task 把每個 observer 的 buffer 交出去，照第一次拿到 entry 的順序；只收觀察的種類；`takeRecords` | `cargo test -p dom_js --test it performance_observers::a_task_delivers_the_buffers` |
| `buffered` 照時間的順序；`disconnect()` 留著觀察的方式 | `cargo test -p dom_js --test it performance_observers::buffered_and_disconnect` |
| `performance` 撐住註冊和有 entry 要送的 observer，entry list 撐住它的 entry | `cargo test -p dom --test it performance::a_performance_keeps_its_observers performance::an_entry_list_keeps_its_entries` |
| console 的警告進 Log agent，`[console]` | `cargo test -p shell --test it devtools_log::script_warnings_are_logged` |
| WPT 的 `performance-timeline/` 照 `.ini` 的預期（新加進清單） | `python3 scripts/test.py wpt performance-timeline` |

**Navigation Timing**

| 項目 | 怎麼驗證 |
| --- | --- |
| 網路的時間：沒有解析的網址、重用的連線在 fetch 開始，`requestStart` 是連線結束，HTTPS 的 `secureConnectionStart`；redirect 照 Chrome 的規則（都同 origin，或每一跳都 opt in）；文件的時間只記第一次 | `cargo test -p renderer --lib navigation_timing` |
| navigation entry：成員、時間的 coarsen、`duration`、`type`、大小、`confidence`；`performance.timing` 和 `performance.navigation` 的 epoch 毫秒；`measure()` 收 PerformanceTiming 的名稱，還沒發生的是 `InvalidAccessError` | `cargo test -p dom_js --test it navigation_timings::the_navigation_entry navigation_timings::the_old_navigation_timing` |
| `load` 的 listener 跑完之後才有 `loadEventEnd`，observer 那時才拿到 entry | `cargo test -p dom_js --test it navigation_timings::observers_get_the_entry_once_loaded` |
| `confidence` 照 rate 隨機 | `cargo test -p dom_api --lib navigation_timing` |
| `performance` 撐住 `timing` 和 `navigation`，`confidence` 每次新的 | `cargo test -p dom --test it performance::a_performance_keeps_its_timing` |
| 整個瀏覽器裡：redirect（主 frame 和子 frame）、時間的先後、大小、observer、reload；子 frame 有自己的 entry | `cargo test -p shell --test it navigation_timing:: performance::` |
| 每個時間點也是 trace 的 `blink.user_timing` 的 mark | `cargo test -p shell --test it trace::` |
| WPT 的 `navigation-timing/`（新加進清單）、`user-timing/`、`hr-time/` 照 `.ini` 的預期 | `python3 scripts/test.py wpt navigation-timing user-timing hr-time` |

**Speedometer 的盤點**

| 項目 | 怎麼驗證 |
| --- | --- |
| kun 缺的 API 只剩 `XMLHttpRequest`（Backbone、jQuery）和 Trusted Types（Lit） | `python3 scripts/bench.py speedometer --inventory`（2026-10-25，`1ca45184`，結果在下面的「做過的檢查」） |
| 20 個 default suite 在 debug build 都跑完 | 同上（修了 flex 和 grid 的寬度之後） |

**盤點找到的：flex 和 grid container 的 shrink-to-fit 寬度**

| 項目 | 怎麼驗證 |
| --- | --- |
| row flexbox 在確定的空間裡 shrink-to-fit：在 float、絕對定位的盒子裡或本身就是，在 inline-block 裡、`width: fit-content`、column 的子項；`flex-basis: 0` 的 item 照樣不窄過 min-content；margin 佔空間，padding 算在寬度裡 | `cargo test -p layout --test it flex::a_row_shrinks_to_fit_its_room` |
| 換行的 column flexbox 也是，在 float 裡或本身就是 float | `cargo test -p layout --test it flex::a_column_that_wraps_shrinks_to_fit_its_room` |
| `fr` 的 grid 也是，`fr` 的 track 平分空間，不窄過 min-content | `cargo test -p layout --test it grid::flexible_tracks_shrink_to_fit_the_room` |
| 量寬度時不斷行，只在拿到的寬度斷行 | `cargo test -p layout --test it flex::what_shrinks_to_fit_breaks_its_lines_at_its_width` |
| WPT 的 `css/css-flexbox/negative-margins-001.html` 通過，其他的照 `.ini` 的預期 | `python3 scripts/test.py wpt css/css-flexbox css/css-grid` |

## 做過的檢查

2026-10-24 到 25，Fedora 44（AMD Ryzen AI 9 HX 370），六段都在這台上做。

- **做了什麼**（設計見 `README.md` 的「詳細設計」）：
  - 第 1 段：`window.opener`（`[Replaceable]`）、`fromElement`、`toElement`；`dom` 的 `EventTargetKind::NetworkInformation`、`WakeLockSentinel`，`Geolocation`、`GeolocationPositionError`、`WakeLock` 三個 script object（請求用 `Edge` 串成鏈、`CallbackSlot` 存 callback）；`dom_api` 的 `ClampedU32`、`TaskSource::Geolocation`、visibility 改變時通知 geolocation 和 wake lock；document 專屬的 event handler 的清單。
  - 第 2 段：`renderer` 的 `PendingUserInput`（compositor 的 closure 交出時加、`run_task` 取出時減，合併的 `mousemove` 到分派才減）；`dom_api` 的 `scheduling.rs` 和 `FrameClient::pending_user_input`；`paint` 的 `frame_attribution_region`；`compositor` 的 `PropertyTrees::frame_attribution`，結果放進 `ToMainThread::Input` 的 `attribution`。
  - 第 3 段：observer 的 `delay`、`trackVisibility` 和節流（renderer 記下最早到期的時間要一個 main frame）；`paint` 的 `intersections` 判斷扭曲的效果，`hit_test::occluded` 用點擊的 `Walker` 判斷蓋住（`VisualOverflow`、`glyph_reach`、`svg_glyph_reach`）；entry 的 `time` 是算的那一刻。
  - 第 4 段：`File` 和 `Blob` 同一個 GC 型別（`FileInfo`）、`EventTargetKind::FileReader`、`TaskSource::FileReading`、`EventKind::Progress`；`dom_api` 的 `file_reader` 模組（`encoding_rs`）；binding 的 `Kept<T>`（Blink 的 `[CachedAttribute]`）；WIT 的回傳的 union 可以有 `ArrayBuffer`，Lua 的 generator 多兩種形式。
  - 第 4 段之後：`ScriptHost::clean_up_after_callback`（V8 的 host 在 `bindings::trace::in_script` 是假時做 checkpoint，先 `finished_timer`），回報例外時分派的 `error` 進一個 script 的 entry；`Timers::set_clock`，renderer 的 `update_clock` 拿掉。
  - 第 5 段：rusty_v8 的 fork 的 `ObjectTemplate::mark_as_undetectable`（tag `kun-v152.2.0-20`）；`CollectionFilter::DocumentAll`、`DocumentAllNamed`；宣告表的 `[LegacyCaller]`（只給 JS：WIT 裡匿名的 indexed getter 也叫 `item`）和 `[IsHTMLDDA]`；`dom_js` 在 instance template 上設 call handler、`mark_as_undetectable`。
  - 第 6 段：
    - `dom` 的 `PerformanceObserver`、`PerformanceObserverEntryList`、`PerformanceTiming`（`PerformanceTimingKind`）三個 GC 型別，`PerformanceData` 的兩個 observer 列表（`EdgeChunks` 加原子的計數），`TaskSource::PerformanceTimeline`，`PerformanceEntryType::Navigation`。
    - `dom_api` 的 `performance_observer`、`navigation_timing` 模組，`ExceptionName::InvalidModificationError`（WIT 的錯誤碼 25），`FrameClient::console_warning`、`navigation_timing`；宣告表的 `static attribute`（`MemberKind::StaticGetter`）。
    - `protocol` 的 `NavigationLoad` 多了 `LoadTiming`、`RedirectHop`、`protocol`、`Content-Encoding`，`CommitNavigation` 多了 `NavigationTimingType`（`loaded` 改成 `Box`）；browser 的 `PendingNavigation` 收集 redirect。
    - `renderer` 的 `navigation_timing` 模組（`network_timing`、`Milestone`），render thread 記下文件的時間點和 trace 的 mark、子 frame 的 redirect（`frame_redirects`）；`FrameSettings::console_warnings` 接到 Log agent。
  - 盤點之後：`layout` 的 `BoxTree::fit_content_width`，flex（`column_flex_width` 不處理的）和 grid container 在確定的空間裡寬度未知時取 fit-content，排版和量高度時用那個寬度。
  - IDL 的檢查加上 `netinfo.idl`、`prerendering-revamped.idl`、`xhr.idl`、`entries-api.idl`、`navigation-timing.idl`、`resource-timing.idl`、`server-timing.idl` 等；缺的進 `spec-missing.txt`，刻意不同的進 `spec-differences.txt`。WIT 每段改過都重新產生 golden、`ci.py guests` 重編；`dom_wasm` 的 `typed_list.rs` 先手動把新成員之後的編號加上去再重新寫（`crates/dom_wasm/README.md` 改寫成這個做法）。
  - `dom` 的 `soundness.md`：`geolocation.rs` 一節、I14 的型別清單（補上原本漏掉的 `StyleSheetList`）、`FileReader` 的 `#[no_trace]` 和新的 `NoTrace` impl。GC stress 的 workload 加上 `connection`、geolocation、`churn_performance_observers`、navigation timing。
- **實作時改的、找到的 bug**（決定記在 `README.md` 的「已決定的事項」第 8 到 20 條）：
  - `document.onvisibilitychange = f` 一直丟 `NotSupportedError`：document 的 handler 用元素那一組，只認 `GlobalEventHandlers` 的名稱。照 HTML 加了 document 的清單。
  - text 的 item 的 `visual_rect` 是 raster invalidation 用的保守範圍，直接拿來算蓋住時字附近的 target 都看不見；SVG 圖形的框多留了 stroke 的空間；target 是 iframe 時子 frame 畫的被當成蓋住它的。都照 Blink 改了。
  - `FileReader`：前後有空白的 label 照 Chrome 不認、讀成 UTF-8；`abort()` 留著 `result`、`abort` 的 `ProgressEvent` 是 0/0，照 Chrome。讀的編號一開始用了整個 process 共用的 `static`（`sanity.py boundary` 抓到），改成每個 reader 自己數。
  - **第 4 段找到、修好的兩個問題**：task 分派的事件的 listener 之間沒有 microtask checkpoint（Chrome 有，`probes/micro.html`、`fr7.html`）；`setTimeout` 從設它的 task 開始時算，不是從呼叫的時候（`probes/tm.html`）。DOM 的 task 和 timer 必須用同一個時鐘：只改 timer 的話，`setTimeout(0)` 之後的 `postMessage` 跑到前面。一開始少了 `error` 的那個 entry，WPT 的 `evaluation-order-1.html` 和 `checkpoint-after-window-onerror.html` 失敗了。`shell` 的 `a_timer_set_in_a_microtask_is_not_nested` 失敗之後決定了第 15 條。
  - `document.all`：prototype 上 `item` 和 `namedItem` 的順序、6b 的 `PerformanceNavigation` 的常數，一開始和 Chrome 不同，已經照它調了。
  - `PerformanceObserver` 的 callback 一開始照註冊的順序（Chrome 照第一次拿到 entry 的順序，Blink 的 `active_observers_`）；`disconnect()` 一開始把觀察的方式也重設了。都照 Chrome 改了。
  - Navigation Timing 一開始沒有收集子 frame 的 redirect（WPT 的 `test-timing-server-redirect.html` 這些失敗），TAO 的規則也照 Chrome 改了（`details.md`）。
  - **盤點找到、修好的 layout 的 bug**（`README.md` 的「盤點找到的：flex 和 grid container 的 shrink-to-fit 寬度」）：
    - 經過：NewsSite-Next、NewsSite-Nuxt、Editor-CodeMirror 在 debug build 停在 column flexbox 的寬度和 taffy 的比對。在檢查失敗時印出子樹，看出 column 的子項是 row flexbox（Next、CodeMirror）和 `1fr 1fr` 的 grid（Next、Nuxt），taffy 在 364、752、780 px 的空間裡回傳它們的 max-content 寬度。0084 之前的 `main`（`c60a7027`）一樣失敗。
    - 探測：`probes/fit.html` 的 39 種情況，Chrome 和 kun 一起跑。
    - 修法改了兩次：
      - 第一版只處理只量寬度的 row flexbox，mutation test 有三個沒抓到。
      - 為了查它們，暫時讓那些情況 panic，跑 reftest 和 WPT。結果 taffy 排本身是 float 的 grid 時，直接在寬度未知時排它（`grid-auto-repeat-min-size-003`、`-004`），換行的 column 也錯。
      - 改成現在的做法之後，探測的 39 種情況裡，除了 `P`（換行的 flexbox 的 item 有 `flex-basis`，taffy 的 intrinsic 寬度）和 `U`（scrollbar 的寬度）都和 Chrome 一樣。這兩個修之前就不同。
- **Chrome 152 的探測**：每段 kun 跑同樣的頁面，除了錯誤訊息的格式、照「已決定的事項」刻意不同的、和 `docs/limitations.md` 記的以外都一樣。
- **不穩定的測試，找到的原因**：
  - `shell` 的 `scheduling::`：busy 的迴圈原本在第一個 frame 的 rAF 之後開始，輸入偶爾在它開始前就被處理掉（40 次裡錯 1 次）。改成在一個 key 的 `keydown` 裡跑，要測的輸入跟在後面，最後送一個頁面上的 move 當標記，迴圈等到它在等為止，連跑 40 次通過。迴圈裡先問 discrete、再問它的超集（`includeContinuous`）。
  - WPT 的 `intersection-observer/v2/delay-test.html`：entry 的 `time` 是 commit 之後建 entry 時的 `performance.now()`，兩次的落差不一樣，差可以比 `delay` 小。照 Blink 的 `GetTimeStamp` 改成算的那一刻，連跑 10 次通過。
- **mutation test**（每段一份 spec，`mutants-*.toml`）：

  | spec | 弄壞的 | 結果 | 第一次沒抓到、補了什麼 |
  | --- | --- | --- | --- |
  | `1a` | 2 | 都抓到 | |
  | `1b` | 4 | 都抓到 | |
  | `1c` | 4 | 都抓到 | |
  | `1d` | 9 | 都抓到 | |
  | `1e` | 9 | 都抓到 | 看不見時照樣接受請求：測試改成確認當場拒絕、沒有排 task |
  | `1f` | 4 | 都抓到 | |
  | `2a` | 14 | 都抓到 | move 取出時就扣：補了取出之後、分派之前的檢查 |
  | `2b` | 16 | 都抓到 | 捲動容器、sticky、viewport 的捲動、不套 clip：補了捲動之後的點、sticky 的空間、`overflow: clip` 和 `hidden` |
  | `3a` | 13 | 都抓到 | |
  | `3b` | 12 | 11 個，1 個等價 | 旋轉和 skew 不算：補了 `scale(2) rotate(10deg)` 和 `skew(10deg)` |
  | `3c` | 18 | 15 個，3 個等價 | SVG 圖形的框、SVG text 的空間：改了測試的 `overflow` 和位置；「沒有東西蓋住」的 mutant 改寫 |
  | `4` | 23 | 都抓到 | 舊的讀也完成：補了直接呼叫 `dom` 的測試；`File` 在 script 裡是 `Blob`：補了 kind 的測試；多餘的一行和它的 mutant 拿掉 |
  | `4-fixes` | 13 | 都抓到 | idle 的 timeout 照固定的時間：`the_clock_is_read_when_asked` 補上 idle |
  | `5` | 17 | 都抓到 | 同名的集合不是弱指著：補了 `dom` 的 `the_all_named_collection_is_kept_weakly` |
  | `6a` | 23 | 22 個，1 個等價 | 每個 observer 都收所有種類、`buffered` 照新的在前：補了只觀察 mark 的、三個 mark |
  | `6b` | 24 | 都抓到 | 子 frame 的 redirect 不記：補了 `shell` 的 `a_child_frames_redirected_load`；`high` 讀成 `low`：補了 `high` |
  | `fit` | 10 | 8 個，2 個等價 | 第一版的 7 個裡 3 個沒抓到，查出修法不完整、改寫（上面）；grid 照 flex 排：grid 的兩個 item 改成不同寬；用排版代替量寬度：補了斷行次數的 `what_shrinks_to_fit_breaks_its_lines_at_its_width` |

  等價的（記進 `docs/test-gaps.md`）：只看 target 所在的 effect（pre-paint 只在有效果時建 effect node）；「邊相接也算」只改外框（接著的軸比較一樣嚴格）；「只有 paint 變了不重算」、「不記下有在追蹤可見」（改 style 一律重跑 layout 和 pre-paint）；「`takeRecords` 不把計數歸零」（slot 照樣清掉，只多用記憶體）；`fit_content_width` 在寬度已知、自己的 `width` 有值時照樣量（量出來的就是那個寬度，只多量兩次）。每次跑完 `target/mutate/originals` 都不在，檔案都還原了。
- **WPT**：每段加進清單的目錄用 `MACHOPO_UPDATE_EXPECTATIONS` 寫預期結果，失敗的原因寫在每份 `.ini`，連跑兩次（`intersection-observer/v2/` 三次）沒有意外的結果。
  - 加進清單的：`netinfo/`、`geolocation/`、`screen-wake-lock/`、`is-input-pending/`、`intersection-observer/v2/`、`FileAPI/` 的 `file/`、`FileReader/`、`reading-data-section/`、`fileReader.any.js`、`historical.https.html`、`htmlallcollection.html`、`document-all.html`、`performance-timeline/`、`navigation-timing/`。
  - 失敗的主要原因：machopodriver 沒有 Set Permission（testdriver 的 `set_permission`）；沒有 Permissions Policy、Permissions API、worker、service worker、`window.open`、back-forward cache、Resource Timing、`toJSON()`；kun 的跨 origin 的 window 沒有 `postMessage`；machopodriver 的視窗是 800×526；`document.scripts`；子 frame 導覽到別的文件、reload、back；body 收完才 commit。
  - 因為這份而通過的原本就在清單裡的：`FileAPI/blob/` 的三份、`shadow-dom/` 的兩份各一項、`selection/textcontrols/focus.html`（從 ERROR 變成可以跑）、`Event-dispatch-throwing-multiple-globals.html`、`user-timing/` 的 `measure_*navigation_timing*` 四份和 `measure.html`、`hr-time/test_cross_frame_start.html`、`performance-timeline/not-clonable.html`。
  - 預期結果是舊的、HEAD 本來就不同的（先 `git stash` 回 HEAD 跑過確認）：`Blob-methods-from-detached-frame.html`（TIMEOUT）、`custom-elements/` 的四份、`event-global-event-targets.html` 的 `FileReader`、`messageerror.html`。
  - 改了 document 的 event handler、microtask 和 timer 之後也跑了 `html/webappapis/`、`dom/events/`、`html/browsers/the-window-object/`、`custom-elements/`、`resize-observer/`、`requestidlecallback/`、`webmessaging/`、`hr-time/`：沒有意外的結果（`webmessaging/{with,without}-ports/018.html` 的 TIMEOUT 是 `.ini` 列的 known intermittent）。
  - wdspec 的 `execute_script/collections.py`、`execute_async_script/collections.py` 的 `test_html_all_collection` 通過，改了預期結果，只跑這兩份。
  - 修了 flex 和 grid 的寬度之後，跑了 kun 自己的 reftest（全部）和 WPT 的 `css/css-flexbox`、`css-grid`、`css-sizing`、`css-position`、`CSS2`、`css-align`、`css-overflow`、`css-display`、`css-contain`、`css-text`、`css-pseudo`、`css-ui`、`css-values`、`css-gaps`、`css-box`、`css-inline`、`css-logical`、`css-transforms`、`css-backgrounds`、`css-masking`、`css-text-decor`、`css-images`、`html/rendering`：5683 個測試，只有 `negative-margins-001.html` 從 FAIL 變成 PASS，拿掉它的 `.ini`。
- **V8**：第 5 段的 fork 的 binding 先在本機 `ci.py v8 --profile release` 編（第一次整個編，49 分鐘），用 `RUSTY_V8_ARCHIVE` 連結做完 kun 這一側；之後 push fork 和 tag `kun-v152.2.0-20`，`RUSTY_V8_MIRROR_TAG` 換過去，拿掉 `RUSTY_V8_ARCHIVE`、下載 Linux release 的 artifact 重跑 `cargo test --workspace`，一樣通過。
- **各段的收尾**（`cargo test --workspace --no-fail-fast`、clippy、`cargo fmt --check` 連同 `sdk/lua/generator`、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`）：

  | 段 | `cargo test --workspace` | 收尾時修的 |
  | --- | --- | --- |
  | 1 | 3813 通過、0 失敗、3 ignored | `api::the_table_describes_each_interface` 沒列新的 7 個 interface |
  | 2 | 3833 通過 | clippy：paint 的 `len() - 1`、`update` 的參數數目（帶理由的 `#[expect]`） |
  | 3 | 3845 通過 | clippy：`dom` 的測試的 tuple 改成具名的 type |
  | 4 | 3858 通過 | `sanity.py boundary`：讀的編號的 `static` |
  | 4 之後 | 3929 通過，158 秒 | `cargo fmt` |
  | 5 | 3932 通過，293 秒（本機的 V8）；251 秒（release 的 `-20`） | `cargo fmt` |
  | 6 | 3951 通過，104 秒；修完之後 119 秒 | clippy：兩個 `unsafe` 的 `SAFETY` 註解、`is_multiple_of`、`trace_milestone` 在 `tests` 之後、`MainThreadTask` 的 variant 太大（`loaded` 改成 `Box`） |
  | 盤點之後的 layout | 3955 通過，53 秒 | 沒有 |

  `sanity.py docs` 在第 1 到 4 段只有本來就有的 `README.md` 裡指向 rusty_v8 的 build 目錄的路徑（那時這台機器還沒編過 V8），第 5 段起都通過。收尾的修改之後都重跑了改到的 crate 的測試。
- **情境**：`test.py scenarios` 72 個都通過（第 4 段之後 46 秒，第 6 段 79 秒）。
- **Speedometer 的盤點**（`bench.py speedometer --inventory`，2026-10-25，`1ca45184` 加上 layout 的修正，Chrome for Testing 152.0.7977.82）：
  - kun 缺的 API 只剩 `new XMLHttpRequest`（TodoMVC-Backbone、jQuery）和 Trusted Types 的四個（TodoMVC-Lit）。
  - 20 個 default suite 在 debug build 都跑完。修之前 17 個跑完：NewsSite-Next 和 Editor-CodeMirror 的 renderer abort，NewsSite-Nuxt 在 panic 之後沒有回報（上面的「盤點找到、修好的 layout 的 bug」）。
  - API 的盤點是另一頁的探測，不受 layout 的影響。
