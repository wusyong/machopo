# RFC 0091 的細節

## RFC 開始前（2026-11-09）machopo 有什麼

1. **renderer 的 DevTools**（`crates/renderer/src/render_thread/devtools.rs`）：
   - **agent**：page、network、log 三個 agent 是 `Rc`，狀態放在 `RefCell` 裡。
   - **分派**：命令依序問它們（`handle(session, message) -> bool`，自己解析 JSON、自己回答），都不接時才交給 engine（V8 的 `V8InspectorSession::dispatchProtocolMessage`）。V8 有那個 session 時，不認得的 domain 由 V8 回答 -32601。
   - **回答和事件**：都是 `RendererHostMsg::DevToolsMessage`。browser 的 `AutomationSession::dispatch_to_client` 只檢查是不是這個分頁的 session，原樣交給 client。
   - **停在斷點時**：script 停住（或跑著時被打斷）的巢狀迴圈只經過 `DevToolsQueueHandle::answer_here` 問那三個 agent，拿不到 engine 和 `Dom`。所以 page agent 是從快取的 frame 資訊（`tell_frames`）回答的。
   - **attach、detach、renderer 重開**：
     - `devtools_detach` 逐個呼叫 agent 的 `detach`。
     - renderer 重開時，browser 只再送 `Page.enable` 和 `setLifecycleEventsEnabled`（`Attached` 的旗標），其他的 domain 由 client 再送。
   - **事件的掛點**：沒有 probe 的登記表，是 render thread 在對的地方直接呼叫 agent（`network_agent.will_send`、`page_agent.dom_content_loaded`……）。
2. **DOM**（`crates/dom`，在 elise 上）：
   - **節點的 id**：
     - `dom::NodeId`（`crates/gfx/src/node_id.rs`）第一次被問時才發，經過 elise 的 weak id 表找回節點，不重用。
     - 一個分頁的所有文件（包括 iframe 的）在同一個 `Dom` 裡。
     - WebDriver 的 element reference 就是它。
   - **留住節點的三種方式**：`NodeRoot`（strong 的 `Persistent`，要比 heap 早 drop）、`WeakPersistent`、id 表。`NodeRef<'gc>` 不能留過一次操作。
   - **改動的共同掛點**：`Cx` 的 `inserted`、`removed`、`attribute_changed`、`data_changed`、`shadow_attached`（`crates/dom/src/style.rs`）。
     - CSSOM 改 inline style 走 `set_inline_style`，不經過 `attribute_changed`。
     - MutationObserver 只在有 observer 時建 record（`crates/dom/src/mutation.rs`）。
     - `ElementChange` 只記 engine 要跑的步驟。
     - 沒有 probe。
   - **最接近的先例**：DevTools 抓 stack 的旗標和 buffer（`crates/dom/src/initiator.rs`）。
   - **pseudo element**：`::before`、`::after`、`::marker` 只在 style 和 layout 裡，不是 DOM 的物件。
   - **listener**：存在節點上（`crates/dom/src/event.rs`），callback 是 `JsEdge`。還沒編譯的屬性 handler 只有原始碼。
   - **JS 的 wrapper**：
     - 節點到 wrapper：`dom_js` 的 `webdriver::node_wrapper`、`listeners::wrap`。
     - wrapper 到節點：bindings 的 unwrap。
3. **style**：
   - **stylesheet**（`crates/css/src/stylesheet.rs`）：`StyleSheetContents` 只留規則和 `RuleSet`。
     - 不支援的 at-rule（`@import`、`@layer`、`@font-face`、`@container`、`@scope`、`@property`）、不支援的 property 和無效的值都在解析時丟掉。
     - 不留原文：`<style>` 的在 DOM 的文字裡，`<link>` 的在 `sheet_loaded` 解析完就丟。
     - 不記位置：parser 的位置只用在錯誤上。
   - **stylesheet 物件**：`CssStyleSheet`（`crates/dom/src/style_sheet.rs`）有一個活著時不變、不重用的 id。`<style>` 的文字改了就建新的物件（`crates/dom/src/cssom.rs`）。
   - **cascade 的過程**（`crates/style/src/lib.rs` 的 `Walk::cascade`）：
     - 每個 block 帶著來源（`BlockSource::Rule`）和 `CascadePriority`。
     - 但 `cascade_declarations` 只交出 `ComputedStyle`，誰贏了不留。
     - matching 只回答每條規則最高的 specificity，不回答是哪個選擇器。
     - 文件裡小的 stylesheet 併成一個 index，規則看不出屬於哪份。
     - debug build 的 `check_passed_over` 已經會在一般的走法之外，為一個元素重新跑 match 和 cascade。
   - **computed style**：`crates/dom_api/src/computed_style.rs` 的 property 清單和取值（`pub(crate)`）。
   - **pseudo-class 的狀態**：存在 DOM 的 bit 上，`crates/style/src/selector.rs` 的 `match_non_ts_pseudo_class` 讀它們。改了經過 `state_changed` 通知，invalidation 照 state 的依賴只重算受影響的元素。
4. **幾何和 hit test**：
   - **框**：`FrameLayout` 的 `element_box`、`client_rects`、`hit_test`、`elements_at`（`crates/renderer/src/frame_view.rs`），只要 pre-paint，不重新 layout。
   - **quad**：`layout::geometry` 的 `box_to_root`（`pub(crate)`）給 border box 到 root 的完整 transform，可以算出 quad。
   - **margin**：fragment 有 padding、border、margin；行內的 box 沒有 margin。
   - **hit test**：`crates/paint/src/hit_test.rs` 有不看 `pointer-events: none` 的模式。
5. **paint 和 compositing**：
   - **commit**：`FrameView` 的 paint 經過 `PaintArtifactCompositor` 分成 layer，renderer 的 `documents.rs` 交給 `LayerTreeHost::set_layers`；`PaintOutput` 沒變時跳過。
   - **畫在頁面上面的東西**：caret、選取、focus ring 都是頁面的 paint。不屬於頁面、畫在上面的只有 browser 的 UI（`<select>` 的選單，`crates/shell/src/browser_view.rs`）。
   - **可以用的現成東西**：
     - `LayerProperties` 的 `fixed` 讓 layer 畫在 viewport 的座標裡（`crates/compositor/src/layer.rs`）；
     - `PaintOp` 有矩形、路徑和 glyph；
     - `layout` 的 `shape_canvas_text`（`crates/layout/src/canvas_text.rs`）排文字。
   - **停在斷點時**：render thread 在 engine 的呼叫裡，碰不到 `LayerTreeHost`，不畫新的 frame。
6. **V8 的 inspector**（`crates/bindings/src/devtools.rs`）：
   - **現在用到的**：`V8InspectorSession` 只用了 `dispatchProtocolMessage`。
   - **已經有、還沒用的 binding**：rusty_v8 有 `wrap_object`、`unwrap_object`、`add_inspected_object`、`release_object_group`、`V8InspectorClient` 的 `value_subtype` 和 `description_for_value_subtype`。它們不在 fork 的「Local modifications」裡，是上游的。
   - **沒有記下來的**：每個 frame 的 context id。`wrap_object` 直接收 context，`dom_js` 每個 frame 有自己的 context，用不到 id。
7. **WebDriver**：machopodriver 不用 `DOM` 和 `Runtime`，用私有的 `Machopo.*`（`Machopo.queryElement`、`Machopo.scrollIntoViewIfNeeded` 等）。這份不動它們。

## Chromium 的對照

2026-11-09 對照 Chromium 的 `main`（路徑相對於 `third_party/blink/renderer/core/`）和 devtools-frontend 的 `main`。

1. **`InspectorDOMAgent`**（`inspector/inspector_dom_agent.cc`）：
   - **nodeId 的表**：
     - `document_node_to_id_map_`、`id_to_node_` 都是 strong 的 `Member<Node>`；`Bind` 從 1 往上發，同一個 agent 裡不重用。
     - 不在文件裡的節點（`requestNode` 一個 detached 的物件）另外放一張 dangling 的表，送 `setChildNodes(parentId=0)`。
     - `DiscardFrontendBindings` 在 `getDocument`、`getFlattenedDocument`、`SetDocument`、main frame 的 `DOMContentLoaded` 時清掉全部；`disable` 是 `SetDocument(nullptr)`。
     - `getDocument` 自己打開 `DOM`。
   - **`backendNodeId`**：`DOMNodeIds::IdForNode`（`dom/dom_node_ids.cc`），process 一份的 weak 表，第一次被問時才發。
   - **lazy push**：
     - `getDocument` 的預設深度在程式裡是 `depth.value_or(2)`，pdl 寫預設 1。
     - 只有一個文字子節點的直接帶上；shadow root、template 的內容、pseudo element 一律帶到下一層。
     - `requestChildNodes` 在回答之前同步送 `setChildNodes`。
   - **改動的事件**（probe）：
     - probe 的呼叫點：每個 probe 是 `build/scripts/templates/instrumenting_probes_inl.h.tmpl` 產生的 inline 包裝，先問 `CoreProbeSink::HasAgentsGlobal(mask)`（`probe_sink.h.tmpl`：`s_existingAgents` 是 `static std::atomic<unsigned>`，整個 process 哪幾種 agent 存在，`memory_order_acquire` 的 load），沒有就 return，有才呼叫 `…Impl` 找那個 frame 的 sink 和 agent。
     - `DidInsertDOMNode`、`WillRemoveDOMNode`：父節點沒綁的不送；子節點沒送過的送 `childNodeCountUpdated`；否則送 `childNodeInserted`、`childNodeRemoved`。移除時 `Unbind` 那個子樹。
     - `DidModifyDOMAttr`、`DidRemoveDOMAttr`：送 `attributeModified`、`attributeRemoved`，沒有改變的不送。
     - `CharacterDataModified`：送 `characterDataModified`；文字節點是不是只有空白改變了時，改送插入或移除。
     - `DidInvalidateStyleAttr`：用延遲 0 的 timer 併成一個 `inlineStyleInvalidated`。
     - `DidPushShadowRoot`、`WillPopShadowRoot`、`PseudoElementCreated`、`PseudoElementDestroyed`、`DidPerformSlotDistribution`、`TopLayerElementsChanged`：各自的事件。
   - **只有空白的文字節點**：`ShouldSkipNode` 跳過，除非 `enable({includeWhitespace: "all"})`。
   - **`documentUpdated`**：`SetDocument` 只在文件不在解析時送，所以導覽之後實際上在 `DOMContentLoaded` 送。子 frame commit 時對 `<iframe>` 送移除再插入（`InvalidateFrameOwnerElement`）。
   - **iframe 和 shadow root**：
     - 同一個 process 的 iframe 的文件在同一個 id 空間（`InnerParentNode` 回答 owner）。
     - shadow root 不論種類都送，帶 `shadowRootType`；前端的「Show user agent shadow DOM」決定顯不顯示 user-agent 的。
     - user-agent shadow tree、shadow root 和 pseudo element 不能編輯。
   - **pseudo element**：`::before`、`::after`、`::marker`、`::backdrop`、`::checkmark`、scroll marker、view transition 等放在宿主的 `pseudoElements`，帶 `pseudoType`。
2. **要 V8 的地方**：
   - **`resolveNode`**（`inspector/resolve_node.cc`）：`V8InspectorSession::wrapObject`，main world 或給的 `executionContextId`。
   - **`requestNode`、`describeNode` 和帶 `objectId` 的 `highlightNode`**：`unwrapObject` 加 `V8Node::ToWrappable`。
   - **`setInspectedNode`**：`addInspectedObject(InspectableNode)`，留的是 backend id。
   - **Console 顯示節點**：`ThreadDebuggerCommonImpl::valueSubtype` 對節點回答 `node`。
   - **`DOMDebugger.getEventListeners`**（`inspector/inspector_dom_debugger_agent.cc`）：`EventListenersInfoForTarget`，`v8::Function::GetScriptLocation` 給位置，`handler` 和 `originalHandler` 包在呼叫者的 object group；capture 的在前。
3. **`InspectorCSSAgent`、`InspectorStyleSheet`**：
   - **`enable`**：
     - 要先有 `DOM.enable`，否則回答「DOM agent needs to be enabled first.」。
     - 非同步：`InspectorResourceContentLoader` 用 `kOnlyIfCached` 重新拿文件和每個 stylesheet，之後對每份 stylesheet 送 `styleSheetAdded`，最後才回答。
   - **`getMatchedStylesForNode`**：
     - 先更新 style 和 layout，`InspectorStyleResolver` 呼叫 `StyleResolver::PseudoCSSRulesForElement`。
     - match 用 `ElementRuleCollector` 的 `SelectorChecker::kCollectingCSSRules` 模式。
     - 繼承來的：沿 flat tree 的父元素各跑一次，加上它們的 inline style。
     - pseudo element：每個網頁看得到的 `PseudoId` 跑一次。
   - **位置**：`InspectorStyleSheet::ParseText` 用 `CSSParser::ParseSheetForInspector` 和 `InspectorCSSParserObserver` 對原文重新解析，得到 `CSSRuleSourceData`；再用 cssText 的 LCS 對到 CSSOM 的規則（`InspectorDiff::FindLCSMapping`）。
   - **原文**：依序從編輯過的文字、`<style>` 的文字、`InspectorResourceContainer` 和 network agent 留著的內容拿，最後才用 CSSOM 序列化。
   - **`forcePseudoState`**：
     - 依 DOM 的 nodeId 記；`:focus`、`:focus-within` 也調整祖先的計數。
     - `StyleEngine::MarkAllElementsForStyleRecalc` 重算。
     - `css/selector_checker.cc` 經過 `probe::ForcePseudoState` 問 agent。
   - **computed style 和字型**：
     - `getComputedStyleForNode` 是全部 property 的 `CSSComputedStyleDeclaration`；
     - `getPlatformFontsForNode` 從 `InlineCursor` 的 `ShapeResultView` 拿字型和 glyph 數。
4. **`InspectorOverlayAgent`**（`inspector/inspector_overlay_agent.cc`、`inspect_tools.cc`、`inspector_highlight.cc`）：
   - **overlay 頁面**：
     - 用 `Page::CreateNonOrdinary` 建另一個頁面，載入 devtools-frontend 的 `inspector_overlay`（`main.ts`、`tool_highlight.ts`、`tool_paused.ts` 等）編好的 JS。
     - backend 用 `EvaluateInOverlay` 呼叫它的 `dispatch`：`setOverlay`、`reset`、`drawHighlight`、`drawPausedInDebuggerMessage`。
   - **怎麼畫到畫面上**：`FrameOverlay` 在 paint 時 layout 那個頁面，`RecordForeignLayer(kForeignLayerDevToolsOverlay)` 成一個 `cc::PictureLayer`。
   - **highlight 的資料**（`InspectorHighlight::AsProtocolValue`）：
     - `paths`：content、padding、border、margin 的路徑和顏色；
     - `elementInfo`：名字、id、class、大小、`layoutObjectName`、能不能用鍵盤 focus、accessible name 和 role、style、對比度；
     - 需要時還有 `gridInfo`、`flexInfo`。
   - **選取元素**：`setInspectMode` 的 `searchForNode` 用 `SearchingForNodeTool`：
     - 滑鼠移動時 hit test，往上找到 shadow host（`searchForUAShadowDOM` 時不找），先 `PushNodePathToFrontend` 再送 `nodeHighlightRequested`；
     - 按下時送 `inspectNodeRequested(backendNodeId)`；
     - 輸入從 `WebFrameWidgetImpl` 經過 `WebDevToolsAgentImpl::HandleInputEvent` 先給它。
   - **停在斷點時**：
     - `PausedInDebuggerTool` 把輸入交給 overlay 頁面；
     - 按鈕呼叫 `InspectorOverlayHost.send('resume')` 等；
     - agent 在 `kInternalInspector` 的 task runner 上呼叫 `V8InspectorSession::resume`、`stepOver`。

## DevTools 前端送的命令

devtools-frontend 的 `front_end/core/sdk/`（2026-11-09）：

- **連上 frame 的 target 時**：`DOMModel`、`CSSModel`、`OverlayModel` 是 `autostart`，建立時不等回答就送：
  - `DOM.enable({})`；
  - `CSS.enable()`；
  - `Overlay.enable()`、`Overlay.setShowViewportSizeOnResize`，加上設定打開時的 `setShow*`。停住時也送 `setPausedInDebuggerMessage`。
  - `DOMDebuggerModel`、`AccessibilityModel` 不是 `autostart`。
- **打開 Elements 時**：
  - 每次 `documentUpdated` 之後送一次 `DOM.getDocument({})`。
  - 預設選 `body`，沒有就選 `documentElement`。重新載入之後先用 `DOM.pushNodeByPathToFrontend` 找回原本選的，0、250、500、1000、1500 ms 各試一次。
  - 點開節點送 `DOM.requestChildNodes`。
  - 滑過節點送 `Overlay.highlightNode` 或 `hideHighlight`。
  - 「選取元素」的按鈕送 `Overlay.setInspectMode`。
  - 另外送 `DOM.getTopLayerElements`。
- **選了節點時**：
  - `DOM.setInspectedNode`；
  - `CSS.getMatchedStylesForNode`、`getComputedStyleForNode`、`getPlatformFontsForNode`，加上 `getEnvironmentVariables`、`getMediaQueries`、`getLayersForNode`、`getBackgroundColors`、`trackComputedStyleUpdatesForNode`、有旗標時的 `getAnimatedStylesForNode`；
  - Event Listeners：對節點 `DOM.resolveNode`，經過 `Runtime.callFunctionOn` 拿祖先和 window（group `event-listeners-panel`），再 `DOMDebugger.getEventListeners`；
  - Accessibility：`Accessibility.enable`、`getPartialAXTree` 等。
- **錯誤的影響**：前端的 `InspectorBackend.ts` 不讓錯誤丟出例外，回答錯誤時多半當成沒有結果。
  - 會讓面板壞掉的：
    - `getDocument` 錯誤時，樹是空的；
    - `setChildNodes` 晚於 `requestChildNodes` 的回答時，點不開；
    - `childNodeCount` 不對時，展開的箭頭不對；
    - `CSS.enable` 失敗時，Styles 和 Computed 都是空的；
    - 規則沒有 `styleSheetId` 和 `range` 時，不能編輯，也沒有「檔名:行」（`panels/elements/StylePropertiesSection.ts`）。
  - 只是那一塊空著的：`Overlay`、`DOMDebugger`、`Accessibility` 和大部分其他的 `CSS` 命令。

## 各個選項的比較

### 不在樹上的節點怎麼留住

| | K1：strong（一個 `HeapVector`） | K1'：每個節點一個 `Persistent` | K2：只留 `NodeId` |
| --- | --- | --- | --- |
| 和 Chrome | 一樣 | 一樣 | detached 的節點可能被收掉，之後回答找不到 |
| GC 多花的 | 一個 root；每個綁著的節點一條邊 | 每個節點一格 root 表，每次 scavenge 和 full GC 都掃 | 幾乎沒有 |
| 留住的東西 | 前端知道的不在樹上的節點（連同子樹、wrapper），到解除為止 | 同左 | 沒有 |
| 要注意 | root 要比 heap 早 drop（agent 跟著 engine 收） | 同左 | `$0`、`inspect()` 的節點在 Console 裡還看得到，Elements 卻找不到 |

### 改動的事件的選項

| | M1：`dom` 的掛點加旗標的 buffer | M2：比對前後的樹 | M3：內部的 MutationObserver |
| --- | --- | --- | --- |
| 沒有 DevTools 時 | 每個掛點多讀一次旗標 | 沒有 | `observed()` 一打開就不關，整頁都付 record 的成本 |
| DevTools 開著時 | 每個改動推一筆 | 每個 task 走一遍前端知道的樹 | 每個改動建一個 GC 的 record |
| 看得到的 | 每一個改動，依序 | 只看得到最後的結果；移動看起來像沒變 | 不跨 shadow root，看不到 shadow root 的建立和 inline style 的 CSSOM 改動 |
| 之後的 DOM 斷點 | 用同樣的掛點 | 做不到 | 做不到停在改動的當下 |
| 對照 Chromium | probe，但一批一批地送 | 沒有 | 沒有 |

### 停在斷點時的選項

| | P1：只借 `Dom`（樹） | P1+：`Dom` 和 `FrameView`（樹、layout、style） | P2：繼續之後才回答 |
| --- | --- | --- | --- |
| 和 Chrome | 停住時看得到樹、`$0`；Styles、Computed、`getBoxModel` 等繼續 | 一樣：停住時看樹、看 style | 停住時 Elements 凍住 |
| 要做的 | engine 在 `run_paused` 裡把 `Dom` 交給 renderer 的 agent；要先確認停住時沒有 Rust 的程式正借著 `Dom`；agent 分出停住時能答、要排著的命令 | P1 的全部，加上停住的迴圈拿得到 `FrameView`（決定時以為要重新安排 `RenderThread` 的擁有權；2026-11-09 的原型發現它在 engine 的 isolate 的狀態裡，和 `Dom` 一起借得到） | 沒有：命令照現在排著 |
| 風險 | 小 | 停住時重算 style 和 layout：Chrome 也會，但 machopo 的 lifecycle 有沒有假設 engine 呼叫期間不重算要確認；碰到 render thread 和 engine 之間的介面 | 沒有 |

### Overlay 的選項

| | O1：renderer 的一個 layer | O2：照 Chrome 跑 overlay 頁面 | O3：browser 畫 |
| --- | --- | --- | --- |
| 畫出來的樣子 | machopo 自己畫，和 Chrome 接近（顏色由前端給） | 和 Chrome 一模一樣，包括 grid、flex 的 overlay | 同 O1 |
| 要做的 | commit 時附加一個 layer，quad 從 fragment 算，標籤用 `shape_canvas_text` | 第二個頁面、內部的 context、host 的橋，帶著 devtools-frontend 的 bundle 和它的授權 | renderer 交出 quad 和標籤，browser 像 `<select>` 的選單那樣畫 |
| 更新的成本 | 只重畫 overlay 的 layer | 跑 overlay 頁面的 JS 和 layout | 一個 IPC 加 browser 的 layer |
| 停在斷點時 | 畫不出來：renderer 不 commit（另外處理） | 一樣畫不出來（JS 停住時照樣能跑，但 renderer 不 commit） | 畫得出來 |
| 拆成 process 之後 | 不用改 | 不用改 | quad 是 renderer 給的，browser 照畫（不信任也只影響那個分頁的畫面） |

「Paused in debugger」的 PM1 是 O3 的一小部分：只有那條提示和按鈕由 browser 畫。

### stylesheet 原文的選項

| | T1：一律留 | T2：DevTools 連上時才留 | T3：`CSS.enable` 時重新抓 | T4：CSSOM 序列化 |
| --- | --- | --- | --- | --- |
| 原文 | 都有 | 連上之後載入的有 | 有，但可能和載入時的不同 | 沒有：作者的排版和註解都不見 |
| 行號 | 對 | 連上之後載入的對 | 通常對 | 對不上原檔 |
| 沒有 DevTools 時 | 每份 `<link>` 和 constructed stylesheet 多留原文（`<link>` 的 `Bytes` 共用不複製） | 不多 | 不多 | 不多 |
| 對照 Chromium | Chrome 從快取拿，大多數情況都有 | | Chrome 的 `kOnlyIfCached`；machopo 沒有快取，會真的再發請求 | Chrome 最後的退路 |

### 規則的位置的選項

| | R1：parser 一律記 | R2：要的時候重新解析 | R3：不給 |
| --- | --- | --- | --- |
| 成本 | 每次解析、每條規則和宣告都多記 | 只在 DevTools 要的時候，每份 stylesheet 解析一次 | 沒有 |
| machopo 丟掉的宣告和 at-rule | 看不到 | 從原文找得回來，Styles 照 Chrome 劃掉 | 看不到 |
| 要的原文 | 不用（但「檔名:行」還是要對到原檔） | 要（第 11 條） | 不用 |
| 對照 Chromium | 沒有 | 一樣（`InspectorCSSParserObserver`） | 沒有 |

## 其他實作

- **WebKit 的 Web Inspector**：CDP 的 `DOM`、`CSS` 從它的協定分出來，`InspectorDOMAgent` 的模型相同。
- **chobitsu**（`liriliri/chobitsu`，在頁面裡用 JS 跑的 CDP，接真的 devtools-frontend，chii、eruda 用它）：
  - nodeId 用 strong 的 `Map`（`src/lib/nodeManager.ts`）；
  - `getMatchedStylesForNode` 只有套用的規則和 inline style；
  - highlight 用 `luna-dom-highlighter` 畫在頁面裡。
- **Lightpanda**（Zig）：`DOM` 只有 Puppeteer 要的一部分（`getDocument`、`describeNode`、`resolveNode`、`getBoxModel` 等），`CSS` 幾乎沒有。
- **Ladybird**（`Libraries/LibDevTools`）、**Servo**：講 Firefox 的 Remote Debugging Protocol，模型不同。
