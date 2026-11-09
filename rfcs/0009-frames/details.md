# RFC 0009 的細節：iframe 和多個 browsing context

這份是 `README.md` 的補充，放支撐它的細節。`README.md` 裡的結論和決定以那邊為準，這裡不重複。

（調查的結果照 `README.md` 的「待決定」第 1 到 10 項寫在這裡，每一節寫日期和看的版本。）

## 1. 現況的盤點

2026-10-06，commit `377a763`（RFC 0008 做完時）。一個唯讀的 agent 掃過各 crate，關鍵的幾條（context 的建立、wrapper 在哪個 context 建、換語言時整套換掉、event handler 的 scope）再人工對照過。難度是「之後要改有多難」：低是局部的修改，中是範圍大但機械式，高是牽涉設計的選擇。

| 假設 | 在哪裡 | 多個 frame 時要怎樣 | 難度 |
| --- | --- | --- | --- |
| **每份 document 一個新的 context 和 global**：建 context 時沒給既有的 `global_object`、沒設 security token；`bind_window` 斷言 window 還沒有 wrapper | `crates/bindings/src/script_state.rs` 的 `create`；`crates/dom/src/v8_engine.rs` 的 `bind_window` | `WindowProxy`：子 frame 導覽時，父 frame 拿著的 `contentWindow` 要是同一個物件，所以 global proxy 要跨導覽保留（V8 的 `Context::New` 帶既有的 global、`DetachGlobal`）；同源的 context 之間設同一個 security token。vendored 的 rusty_v8 有 `global_object` 和 `set_security_token`，沒有 `DetachGlobal` | 高 |
| **JS 和 wasm 的文件是兩套 DOM、不同的 heap**；換語言時 renderer 整套換掉 engine 和 DOM | `crates/renderer/src/render_thread.rs` 的 `use_script_language`；RFC 0001 的第 26 條 | JS 的頁面裡有 wasm 文件的 iframe（或反過來）時，兩份 document 在不同的 heap，節點不能互相存取；要決定政策（見第 3 項） | 高 |
| **wrapper 在目前的 context 建**，不是物件的 relevant realm | `crates/dom/src/v8_engine.rs` 的 `wrapper_of`、`crates/dom_js/src/convert.rs` | 父 frame 的 script 讀子 frame 的節點時，wrapper 要用子 frame 的 prototype（`frames[0].document.body instanceof frames[0].HTMLElement`）：要有 document 到 `ScriptState` 的對應，建 wrapper 前進入那個 context。每一條 binding 的路徑都經過它 | 中到高 |
| **只有一個 `FrameView`**：`RenderThread` 只有一個 `frame_view`、一個 `LayerTreeHost`、一份 visual properties 和游標；約 20 個方法以 `let Some(view) = &mut self.frame_view` 開頭 | `crates/renderer/src/render_thread.rs` | frame tree，每個 local frame 一個 `FrameView`；事件迴圈的每一步走過它 | 中 |
| **lifecycle 和 commit 只有一份**：`update_rendering` 跑一次 lifecycle、一次 commit，設一個 `document_id` 和一個 viewport 的 scroll node | `render_thread.rs` 的 `update_rendering` | 子 frame 照 Chromium paint 進父 frame 的結果；父 document 的 `load` 等子 frame | 中到高 |
| **replaced element 只有 `<img>`** | `crates/layout`（`is_image`、`Replaced`）、`crates/paint` 的 `DrawImage` | iframe 是新的 replaced element，嵌入子 frame 的 paint 結果和 property tree 的子樹（或一個 surface） | 中 |
| **timer 和 rAF 每個 heap 一份、不記得屬於哪個 window**，callback 一律在主 frame 的 realm 執行；導覽 commit 時清掉全部 | `crates/dom/src/timers.rs`、`isolate.rs`；`render_thread.rs` 的 timer 和 rAF、commit | timer 記得自己的 window（HTML 的 map of active timers 是每個 global 一份）；只清掉那份 document 的 | 中 |
| **協定沒有 frame 的 id**：`DidCommitNavigation`、`TestharnessResults` 只帶整個分頁的 `navigation_id`，`navigation_id` 也被當成 document 的 id；`TabId` 的註解說它暫代 frame tree 的 routing id | `crates/protocol/src/renderer.rs`、`lib.rs` | 訊息帶 frame 的 id；document 有自己的 id | 中 |
| **`Window` 沒有 browsing context 的狀態**：只有 document、listener、current event、wrapper；「document 有 window」被當成「fully active」 | `crates/dom/src/event.rs`；`crates/dom_api/src/handlers.rs` 的 `has_browsing_context` | 被移除的 iframe 的 document 要變成不是 fully active（window 還在、browsing context 沒了） | 中 |
| **`ScriptHost::frame()` 是執行中的 realm 的 frame**：`<a>` 的 activation 跟著它；`window.parent` 是自己 | `crates/dom_api/src/host.rs`、`event.rs` 的 `activation_behavior`、`window.rs` | 換頁要用元素的 document 的 frame（父 frame 的 script `click()` 子 frame 的 `<a>` 不能讓父 frame 換頁）；`parent`、`top`、`frames`、`frameElement` | 低到中 |
| **event handler 用目前 global 的 `document` 編譯** | `crates/dom_js/src/host.rs` 的 `compile_event_handler` | 用元素的 document（Blink 的做法） | 低 |
| **`NavigationController` 是平的一串、`OpenUrl` 導覽整個分頁**；本機檔案的權限是每個 process 一個 flag | `crates/browser`（`web_contents.rs`、`render_process_host.rs`） | 每個 frame 的導覽；joint session history | 中 |
| **compositor 只有一個 viewport 的 scroll node 和 `document_id`** | `crates/compositor` 的 `LayerTreeHost`、`input_handler.rs` | 子 frame 的 viewport 是普通的 scroll node | 中 |
| **一個 `StyleEngine`，author stylesheet 的快取只記一份 document 的** | `crates/style` | 每份 document 一份快取，否則每次換一份 document 重算都重新 parse | 低 |

**已經準備好的**：
- 一個 heap 可以有多份 document，`NodeId` 在 heap 裡唯一（compositor 的 element id 不會衝突）。
- 每份 document 一個 `Window`（`document_window` / `window_document`）；`FrameRequests` 和 `FrameClient` 每個 realm 一份（V8 context 的 slot；wasm 每個 Store 一份）。
- `callback_window` 取 callback 的 creation context；`realm_window` 照規範。
- V8 的 template 每個 isolate 一份、各 context 共用；rejected promise 在它自己的 realm 通知。
- microtask queue 每個 isolate 一個（policy 是 explicit），checkpoint 在 renderer 的事件迴圈和 `V8Engine` 裡；同站的 frame 共用一個 isolate 時，這正好是規範的「每個 agent 一個」。
- `FrameView::is_ours` 已經會過濾不屬於這份 document 的節點。
- `viz` 的 `SurfaceAggregator` 會嵌入 surface（位移、clip、透明度、擋住循環，沒有 transform）；compositor 的 `SurfaceLayer` 一律畫在所有 picture layer 上面。跨 process 的 iframe 可以用，同 process 的照 Chromium 不需要它。

**原型：同一個 isolate 的兩個 context 互相存取**（2026-10-06，一個暫時的 `bindings` 測試，跑完刪掉）：把 B 的 global 放進 A，A 的 script 讀寫它。
- 不設 security token（V8 預設每個 context 不同）：A 一碰 B 的 global 就丟 `TypeError: no access`。
- 兩個 context 設同一個 token：讀寫都可以，B 看得到 A 寫的值；`other.Array === Array` 是 false（每個 context 有自己的內建物件，和規範一樣）。原型的 token 是單一字元的 `"t"`，V8 把單一字元的字串共用成同一個物件，所以兩個 `String::new("t")` 剛好相同；1a 實作時發現 V8 比對 token 是看是不是同一個物件，內容一樣的兩個字串不算，要用 internalized 的字串。
- 設不同的 token：一樣丟 `TypeError: no access`。
- 結論：同源的 frame 一定要設同一個 security token（`Context::set_security_token`，vendored 的 rusty_v8 已經有，不用改 fork）；跨源的照預設就會被擋，之後要的是 Blink 那種 cross-origin 的 interceptor，讓 `postMessage`、`location` 這些少數成員能用。

## 2. Chromium 的模型

2026-10-06。標「原始碼」的對照了 Chromium 的 `main`（GitHub 的鏡像，當天的版本）；其他是對 Chromium 的了解，實作 RFC 之前要再對照。

**Browser 端**（`content/browser`）：
- `FrameTree` 由 `FrameTreeNode` 組成，每個 frame 一個，跨導覽保留；每份 document 一個 `RenderFrameHostImpl`（導覽換 document 時換新）；在別的 process 的 frame 用 `RenderFrameProxyHost` 代表。導覽以 `FrameTreeNode` 為單位（`NavigationRequest`），iframe 的 `src` 也一樣經過 browser。
- 歷史是整棵 frame tree 一起記的（joint session history：`NavigationEntry` 帶著每個 frame 的 `FrameNavigationEntry`）。

**Renderer 端**（`third_party/blink/renderer/core/frame`）：
- `Page` 對應一個分頁，裡面是 frame tree：`LocalFrame`（在這個 process 裡）和 `RemoteFrame`（在別的 process 的佔位）。每個 `LocalFrame` 有一個 `LocalDOMWindow`（每份 document 一個）和一個 `LocalFrameView`。
- **`WindowProxy`**（原始碼，`bindings/core/v8/window_proxy.h`）：split window。script 看到的是外層的 global proxy，跨導覽保留；導覽到新的 document 時換新的內層 global（`Window`），例外是從初始的空白 document 導覽到同源的 document 時沿用。同源的存取轉給目前 document 的內層 global，跨源的經過安全檢查和 cross-origin interceptor。每個 frame 每個 world 一個；`LocalWindowProxy` 給同 process 的 frame，`RemoteWindowProxy` 給別的 process 的。
- **agent**（原始碼，`core/execution_context/agent.h`）：`Agent` 是「能同步存取彼此的一組 browsing context」，同站（same-site）的 iframe 共用一個，worker 各自一個；它擁有事件迴圈（`scheduler::EventLoop`）、isolate 的指標、cluster 的 id。`EventLoop`（原始碼，`platform/scheduler/public/event_loop.h`）只管 microtask：有自己的 `v8::MicrotaskQueue`，「能同步存取彼此的 browsing context 必須共用同一個」；task queue 則是每個 frame 一組，註解說這是刻意違反規範。
- **`MutationObserver`**（原始碼，`core/dom/mutation_observer.cc`）：待通知的 observer 清單和「已經排了 microtask」的狀態在 `MutationObserverAgentData`（`Agent` 的 supplement），所以是每個 agent 一個 microtask，不是每個 window。
- **lifecycle**（原始碼，`core/frame/local_frame_view.h`）：`UpdateAllLifecyclePhases` 從 local frame root 跑，經過 `ForAllChildLocalFrameViews`、`UpdateStyleAndLayoutIfNeededRecursive` 遞迴到同 process 的子 frame；子 frame 的內容 paint 進 local root 的同一份 `PaintArtifact`（座標在父 frame 的空間裡），不另外成為一個 compositor 的 frame。別的 process 的子 frame（`RemoteFrameView`）由 compositor 以 surface 嵌入（`SurfaceLayer`，Viz 用 `SurfaceId` 合成）。
- **local root 和 widget**：每個 local root 一個 `WebFrameWidget`（一棵 compositor 的 layer tree、一組輸入）。site isolation 之前整棵 frame tree 在同一個 renderer，只有主 frame 是 local root，所以一個分頁只有一棵 layer tree、一個 widget；輸入由主 frame 的 hit test 往下找到子 frame 的 document（`LayoutEmbeddedContent`）。
- **載入**：iframe 插入 document 時同步建立子 frame 和初始的空白 document（`about:blank`），之後才照 `src` 導覽；父 document 的 `load` 等子 frame 的 `load`（子 frame 的載入延後父 document 的 load event）。

**和 kun 的對照**：
- kun 的 renderer 現在相當於 site isolation 之前的 Chromium、只有主 frame：一個 widget（`LayerTreeHost`）、一棵 layer tree。同 process 的 iframe 照 Chromium 是 paint 進父 frame，不需要新的 compositor 的 frame；`viz` 已經會嵌入 surface，留給跨 process 的 iframe。
- kun 的 microtask queue 是 isolate 的預設 queue（每個 isolate 一個）。同站的 frame 共用一個 agent、一個 microtask queue，只要同站的 frame 放在同一個 isolate，就對得上（第 4 項）。
- Blink 有、kun 沒有的：`WindowProxy`（第 1 項的第一條）、frame tree、每個 frame 的 task queue（kun 只有 renderer 的一條 task queue，規範允許：一個事件迴圈可以服務多個 window）。

## 3. DOM 和 script 的資料模型

2026-10-06。各點列出選項；「看法」是當天的傾向，還沒確認。

**3a. timer、lifecycle、`dom_tree_version`**
- timer 改成每個 window 一份（HTML 的 map of active timers 屬於 global），callback 在那個 window 的 realm 執行，導覽時只清掉那份 document 的；rAF 也照 window 分，renderer 在 rendering update 時照 frame tree 的順序跑（HTML 的「update the rendering」對每個 fully active 的 document）。這沒有替代方案，只有時機。
- `dom_tree_version`：每份 document 一份，或照舊每個 heap 一份（一份改了，全部的 frame 都跑一次 style，結果不變，只是浪費）。看法：先照舊，frame 多了再改；它只影響效能（`docs/limitations.md` 已經記著）。

**3b. `WindowProxy`（script 看到的 window 的身分）**
- **A：照 Blink**：每個 frame 一個 global proxy，跨導覽保留（新的 document 用 `Context::New` 帶既有的 global proxy，舊的 `DetachGlobal`），同源的 context 設同一個 security token。要改 rusty_v8 的 fork：`DetachGlobal` 沒有 binding，C++ 的部分要重編 V8（約 40 分鐘，共用的 artifact 跟著換）。
- **B：先不做**：每份 document 一個新的 global，子 frame 導覽之後，父 frame 拿著的舊 `contentWindow` 指向已經離開的 window（規範是同一個物件、看到新的 document）。WPT 裡用 iframe 的測試多半在 `load` 之後拿一次 `contentDocument` 就用，跨導覽保留身分的只有少數。
- 看法：B 先做，A 列進分階段的後面（或另開）。security token 不管哪個都要設：第 1 項的原型確認不設就互相存取不了。

**3c. wrapper 建在哪個 context**
- Blink：呼叫 `ToV8()` 時目前的 context 必須是物件的 relevant realm（原始碼，`bindings/core/v8/V8BindingDesign.md`：讀 `contentDocument` 時 document 的 wrapper 要建在 iframe 的 context）。產生的 binding 依 holder 的 creation context 進入正確的 context。
- kun 的 `wrapper_of` 用目前的 context。要改成：document 對應到它的 window 的 `ScriptState`（沒有 window 的 document 用建立它的那份 document 的，和 Blink 一樣），建 wrapper 前進入那個 context。節點 adopt 到別的 document 之後，已經有的 wrapper 不換 prototype（Blink 也不換；規範的 relevant realm 跟著 wrapper 建立時決定）。
- 這是每一條 binding 的路徑都會經過的地方，DOM API 的下一批不會多碰它，但 iframe 一定要做，沒有替代方案。

**3d. JS 和 wasm 的文件在不同的 heap**（RFC 0001 的第 26 條）
- **A：不同語言的 frame 當成「遠端」的 frame**：子 frame 的語言和父 frame 不同時，兩邊不能同步存取（`contentDocument` 是 `null`，和跨源一樣），畫面用 surface 嵌入（跨 process 的 iframe 的做法，`viz` 已經會）。renderer 要能同時拿著兩套 engine 和 DOM，或把它放到另一個 renderer（等 RFC 0004）。
- **B：先不支援**：iframe 的 document 是另一種語言時，不執行它的 script（或顯示空白），記成限制。
- **C：兩種文件共用一個 heap**：推翻 RFC 0001 的第 26 條，代價最大，不考慮。
- 看法：B 先做；A 是跨 process 的 iframe 的特例，和 RFC 0004 一起設計。

## 4. agent 和 microtask

2026-10-06。

- **規範**：microtask queue 屬於 agent 的事件迴圈；同一個 agent cluster 裡能同步存取的 window（similar-origin window agent）共用一個 agent。`MutationObserver` 的「microtask 已經排了」和待通知的 observer 清單是每個 agent 一份（Blink 的 `MutationObserverAgentData`，第 2 項）。`queueMicrotask`、promise 的 job 也排進同一個 queue。
- **kun 對應**：JS 的文件在一個 renderer 一個 V8 isolate、一個 `dom` 的 heap、一個 microtask queue（第 1 項）。只做同 process 的 iframe 時，所有 frame 都在這個 isolate，對應成「一個 heap 一個 agent」。wasm 的文件在自己的 heap，自己一個 agent（它沒有 microtask checkpoint，第 1 項）。
  - 規範上跨站的 frame 是不同的 agent，要不同的 microtask queue；同 process 時它們共用一個。site isolation 之前的 Chromium 也是這樣，只有很刻意的測試看得出差別；跨站的 frame 分到別的 process 之後（RFC 0004）自然就分開了。
- **給 DOM API 下一批的約束**：
  - `MutationObserver` 的待通知清單和「已經排了 microtask」的旗標放在 `dom` 的 heap（`DomIsolate` 的狀態，相當於 Blink 的 agent），不要放在 `Window`；通知排進 isolate 的 microtask queue（JS），wasm 的 heap 另外處理（它沒有 checkpoint，要在 wasm 的 task 結束時通知）。
  - `queueMicrotask` 用同一個 queue；把 callback 存在呼叫它的 realm。
  - 需要「目前的 window」的地方用 `realm_window`（呼叫者的 realm）或 `callback_window`，需要「這個節點的 frame」的地方從節點的 document 找，不要用 `ScriptHost::frame()`（第 1 項：它是執行中的 realm 的 frame）。
  - 會排 task 的 API（`AbortSignal.timeout()`）照 3a，把 timer 記在呼叫者的 window。

## 5. render 和輸入

2026-10-06。

- **layout**：iframe 是 replaced element（預設 300×150，Blink 的 `LayoutEmbeddedContent`），子 frame 的 document 有自己的 box tree 和 layout（自己的 `FrameView`，寬度是 iframe 的內容框），不併進父 frame 的 box tree。kun 的 replaced element 現在只有 `<img>`（`crates/layout` 的 `is_image`、`Replaced`），要多一種。
- **paint 和合成**，兩個做法：
  - **A：照 Chromium 同 process 的做法，子 frame paint 進父 frame**：子 frame 的 paint 結果和 property tree 接在父 frame 的 iframe 底下（transform 是 iframe 的位置，clip 是內容框，子 frame 的 viewport 變成一個普通的捲動容器），整個分頁照舊一次 commit、一棵 layer tree。kun 的 `paint` 和 `compositing.rs`（`PaintArtifactCompositor`）處理的是一份結果，要能把子 frame 的接進來。
  - **B：子 frame 自己一棵 layer tree，Viz 用 surface 嵌入**（跨 process 的 iframe 的做法）：`SurfaceAggregator` 已經會嵌入，但沒有 transform，而且 `SurfaceLayer` 一律畫在所有 picture layer 上面（第 1 項），蓋在 iframe 上面的父 frame 的內容會畫錯；每個 frame 也要一個 `LayerTreeHost` 和 compositor 的 thread 端。
  - 看法：A。B 留給跨 process（或跨語言）的 frame，那時再處理 surface 的 transform 和順序。
- **lifecycle**：renderer 的 rendering update 從主 frame 跑，遞迴到子 frame（Blink 的 `UpdateAllLifecyclePhases` 經過 `ForAllChildLocalFrameViews`）；子 frame 的 layout 要在父 frame 的 layout 之後（要知道 iframe 的大小），paint 時一起收。rAF 和 timer 照第 3a 項每個 window 一份。
- **hit testing 和輸入**：父 frame 的 hit test 碰到 iframe 的盒子時，把座標換到子 frame，在子 frame 的 fragment tree 繼續找；事件在子 frame 的 document 裡分派（事件的路徑不跨 frame）。滾輪先給子 frame 的捲動容器，捲到底再往父 frame 傳（kun 的捲動容器已經會往外傳）。游標和連結照被點到的 document：子 frame 裡的 `<a>` 讓子 frame 導覽（`target` 另外處理），不是整個分頁。
- **focus**：鍵盤的焦點在哪個 frame（Blink 的 focused frame）；kun 現在網頁裡沒有 focus（`docs/limitations.md`），等 focus 的設計。

## 6. 導覽和載入

2026-10-06。

- **iframe 插入時**：同步建立子 frame、它的 window 和初始的空白 document（`about:blank`），所以 `iframe.contentDocument` 插入後馬上有值；之後才照 `src` 導覽（`srcdoc` 是另一種內容來源）。移除 iframe 時子 frame 和它的 browsing context 消失，document 變成不是 fully active（第 1 項：kun 現在把「有 window」當成 fully active）。
- **`src` 的導覽**，兩個做法：
  - **A：經過 browser，照 Chromium**：browser 有 frame tree（`FrameTreeNode`），iframe 的導覽是一個 `NavigationRequest`，回應交給 renderer 的那個 frame 去 commit。協定要有 frame 的 id，`NavigationController` 要記每個 frame 的 entry（joint session history）。site isolation（跨站的 frame 在別的 process）一定要這條路。
  - **B：renderer 自己載入，當成一種子資源**：`RequestDestination` 多一種 `Iframe`（現在有 `Document`、`Style`、`Image`、`Script`），經過 browser 的 `net` 抓回來，renderer 在子 frame 裡 parse。browser 不知道子 frame，沒有子 frame 的歷史，子 frame 裡的 script 改 `location` 也要 renderer 自己處理。
  - 看法：B 先做（同源、同 process 的 WPT 測試只要載入一份 document），協定照樣先加上 frame 的 id；A 和 site isolation（RFC 0004）一起做。Chromium 在 2017 年以前，iframe 的導覽也是由 renderer 開始、browser 只追蹤 frame tree。
- **`load` 的順序**：子 frame 的載入延後父 document 的 `load`。kun 的 `FrameView` 已經各自追蹤自己的載入（`pending`、`check_load_finished`），子 frame 可以當成父 frame 的一個 pending 項目，子 frame 的 `load` 分派完才拿掉。iframe 的 `load` 事件在 iframe 元素上分派。
- **`DidFinishLoad`、testharness 的結果**：只在主 frame 報告（現在的訊息不帶 frame 的 id，加了 id 之後照樣只有主 frame 的會用到）。

## 7. process 模型

2026-10-06。只做同源、同 process 的 iframe 時，哪些設計不能擋住 site isolation（RFC 0004）：
- **協定帶 frame 的 id**（第 6 項），不要假設 frame 一定在同一個 renderer。
- **跨源的 frame 一開始就被擋**：同一個 renderer 裡的跨源 frame（第一階段如果允許載入），security token 照 origin 設，互相存取一律 `TypeError`（第 1 項的原型）；不要為了方便讓它們能存取，之後拆到別的 process 時行為才不會變。
- **遠端 frame 的佔位**：跨源（或跨語言，第 3d 項）的子 frame 在父 frame 那邊是一個不能同步存取的 window（Blink 的 `RemoteFrame` / `RemoteWindowProxy`）。第一階段可以只是「`contentWindow` 存在、碰了就丟例外」，之後換成真的跨 process 的實作。
- **輸入的分派**：跨 process 的 iframe 的輸入要在 browser 端照 surface 分派（Chromium 的 `RenderWidgetHostInputEventRouter`）。同 process 的照第 5 項在 renderer 裡分派，不衝突。

## 8. WPT 的收益

2026-10-06，WPT `0f195499`。在 `dom/collections`、`dom/lists`、`dom/nodes`、`dom/events`、`html/webappapis/timers`（已經匯入）和 `dom/traversal`、`dom/ranges`、`dom/abort`、`dom/observable`（還沒匯入）裡，數原始碼用到 `<iframe>`、`contentWindow`、`contentDocument`、`frames`、`window.open` 的測試檔：

| 目錄 | 測試檔 | 用到 frame | 其中要跨源 | 在 skip 清單 |
| --- | --- | --- | --- | --- |
| `dom/nodes` | 349 | 47 | 1 | 13 |
| `dom/events` | 181 | 16 | 6 | 10 |
| `html/webappapis/timers` | 16 | 3 | 0 | 3 |
| `dom/collections`、`dom/lists` | 16 | 0 | 0 | 0 |
| `dom/ranges` | 67 | 7 | 0 | 0 |
| `dom/observable` | 29 | 4 | 0 | 0 |
| `dom/traversal` | 18 | 3 | 0 | 0 |
| `dom/abort` | 6 | 2 | 0 | 0 |
| 合計 | 682 | 82 | 7 | 26 |

- 要跨源的只有 7 個（`get-host-info`、`www1` 這類），`window.open` 只有 1 個：只做同源、同 process 的 iframe 就涵蓋約九成。
- 已經匯入的目錄裡，baseline 有約 490 個 subtest 讀不到 `contentDocument` / `contentWindow`（RFC 0008 的 `progress.md`），skip 清單裡 26 個是整個檔案在等 iframe 的（加上 iframe 裡用 testdriver 的）。
- 有幾個用到 frame 的測試現在已經全部通過（13 個）：只在 feature detection 或一小段用到，或用 `createHTMLDocument` 也過得了。
- 數字是用原始碼的字樣找的，會多算（例如註解裡的 iframe）；實作時以 baseline 的變化為準。

## 9. shadow DOM 的影響

2026-10-06。只評估，不設計。

- **樹的模型**：`ShadowRoot` 是接在 host 上、不是它的子節點的 fragment；Blink 的 `Node::tree_scope_` 指向 `Document` 或 `ShadowRoot`，document 經過 tree scope 取得。kun 的 RFC 0008 第 44 條是每個節點一條指向 document 的 edge，加 shadow DOM 時有兩個做法：多一條 tree scope 的 edge（照 Blink），或需要時從節點往上走到 `ShadowRoot` 或 document（多數 API 只要 document，往上走的只有 `getRootNode`、事件的路徑、style 的 scope）。第 44 條不擋任何一種。
- **connected**：「shadow-including」的根：在 shadow tree 裡的節點也算在 document 裡（`isConnected`、插入和移除的步驟、style 和 layout 要不要處理它）。kun 現在以「根是 document」判斷，要改成 shadow-including 的。
- **事件**：路徑經過 shadow root 到 host（`composed` 的事件才出 shadow tree），每個 listener 看到 retarget 過的 `target`、`relatedTarget`；`composedPath()` 照 shadow tree 的 mode 過濾。kun 的 `event_path` 要多 shadow root 到 host 的那一步，`EventInit` 的 `composed` 已經有。
- **slot**：slot 的 assignment、`slotchange` 事件；`slotchange` 和 `MutationObserver` 共用同一個每個 agent 一份的 microtask（Blink 的 `MutationObserverAgentData` 也放 `active_slot_change_list_`，第 2 項）。**這是給 DOM API 下一批的約束**：`MutationObserver` 的那份 agent 狀態要留位置給 slot change。
- **style**：selector 只在同一個 tree scope 裡比對，加上 `:host`、`::slotted`、`::part`；繼承照 flat tree。kun 的 `style` 用 Servo 的 `selectors` crate，它的 `Element` trait 已經有 shadow DOM 的接口（`containing_shadow_host`、`is_html_slot_element`，kun 現在回傳沒有），規則的收集和比對要改。
- **layout**：box tree 從 flat tree 建（host 的子節點只有被 assign 到 slot 的才畫，畫在 slot 的位置）。kun 的 box tree 現在走 DOM 的子節點，要換成 flat tree 的走法；hit test 會碰到 shadow tree 裡的節點，事件分派時 retarget。
- **和 DOM API 下一批的關係**：`MutationObserver`（`slotchange` 共用 microtask，見上）；`Range`（邊界可以在 shadow tree 裡，`getComposedRanges`）；`innerHTML` / `getHTML`（declarative shadow DOM：`<template shadowrootmode>`、`serializableShadowRoots`）；`TreeWalker` 不進 shadow tree，沒有關係。
- **WPT**：已經匯入的目錄裡 17 個測試檔卡在 `attachShadow`，skip 清單裡 6 個整個檔案要 shadow DOM；`shadow-dom/` 目錄本身沒有匯入。

## 10. 結論

2026-10-06 的看法，還沒確認。

**很難事後改的決定**（照影響排）：
1. **script 看到的 window 的身分**（第 3b 項）：先每份 document 一個 global（B），還是一開始就照 Blink 做 `WindowProxy`（A，要改 rusty_v8 的 C++、重編 V8）。
2. **跨語言的 frame**（第 3d 項）：先不支援（B），之後當成遠端的 frame。
3. **wrapper 建在物件的 realm**（第 3c 項）：沒有替代方案，但碰到每條 binding 的路徑，越早做越少地方要改。
4. **子 frame 的合成**（第 5 項）：paint 進父 frame（A），還是 surface 嵌入（B）。
5. **子 frame 的導覽**（第 6 項）：renderer 自己載入（B），還是經過 browser 的 frame tree（A）。
6. **frame 的 id 和 document 的 id**（第 1、6、7 項）：協定和 renderer 裡分開這兩個 id；`navigation_id` 不再兼當 document 的 id。

**給 DOM API 下一批的約束**（RFC 0008 的第 50 條；不必等 iframe）：
- `MutationObserver` 的待通知清單和「已經排了 microtask」的旗標放在 `dom` 的 heap（相當於 Blink 的 agent），留位置給 shadow DOM 的 `slotchange`；不要放在 `Window`（第 4、9 項）。
- 需要「節點所在的 frame」時從節點的 document 找，不用 `ScriptHost::frame()`；需要「呼叫者的 window」時用 `realm_window`（第 1、4 項）。
- 會排 timer 的 API 把 timer 記在呼叫者的 window（第 3a 項）；`queueMicrotask` 用 isolate 的 queue。
- `Range`、`innerHTML` 先不處理 shadow tree（`getComposedRanges`、declarative shadow DOM），但不要寫死「根一定是 document 或 fragment」。

**分階段的提議**（如果要做 iframe）：
1. **打底，畫面上看不出差別**：同源的 context 設同一個 security token；wrapper 建在物件的 realm；timer 和 rAF 記得自己的 window；`<a>` 的換頁和 event handler 的編譯改用元素的 document；`Window` 有 browsing context 的狀態（fully active）；協定和 renderer 分開 frame 的 id 和 document 的 id。每一項都獨立，也讓 DOM API 的下一批照正確的模型寫。
2. **同源、同 process 的 iframe**：renderer 的 frame tree、每個 frame 一個 `FrameView`；iframe 是 replaced element，子 frame paint 進父 frame（第 5 項的 A）；hit test 和輸入進子 frame；`src` 由 renderer 載入（第 6 項的 B）、初始的 `about:blank`、`load` 的順序；`contentWindow`、`contentDocument`、`frames`、`parent`、`top`、`frameElement`；跨源的 frame 碰了就丟例外。匯入 iframe 解開的 WPT。
3. **之後**：子 frame 的導覽和歷史（第 6 項的 A）、`WindowProxy` 的身分（第 3b 項的 A）、跨語言的 frame（第 3d 項的 A），和 RFC 0004 的 site isolation 一起。
- shadow DOM 另開 RFC；它不依賴 iframe，可以和第 2 階段之後的任何一個並行。

## 參考資料

- HTML：browsing context、navigable、`WindowProxy`、agent 和 agent cluster、event loop。
- Chromium：`content/browser/renderer_host/frame_tree_node.h`、`render_frame_host_impl.h`；`third_party/blink/renderer/core/frame/local_frame.h`、`remote_frame.h`、`local_dom_window.h`；`third_party/blink/renderer/bindings/core/v8/window_proxy.h`；Chromium 原始碼裡 site isolation 的設計文件（docs 目錄的 process_model_and_site_isolation.md）。
