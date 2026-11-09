# RFC 0051：效能的優化：和 Chrome 差在哪、怎麼追

- 開始日期：2026-10-16
- 狀態：✅ 已完成：2026-10-18（八段都在這份裡做，每段做完 merge 回 `main`；剩下的大項放進「之後的可能」，第 28 條）
- 細節：`details.md`（量測的證據、每一項在程式裡的位置、舊盤點的去向）

## 摘要

RFC 0012 做完時最後一次的 trace（20 個 default suite，兩邊各 3 個 iteration）裡，kun 在 Speedometer 計時的區間比 Chrome 多花約 9400 ms（19 個 suite 合計，Perf-Dashboard 不算）。對照 Chrome 的架構，差距幾乎都來自 kun **做了 Chrome 不做的事，或每次做的範圍比 Chrome 大**，不是同一件事做得慢：style 每次重算整個子樹、每個 frame 走好幾遍整棵樹、每個 task 結束時再走一遍、每個 DOM 操作配置和釋放好幾個 GC root、fragment 的 parse 先建一份暫時的文件再搬兩次。這份 RFC 列出差距、和 Blink 的對照、每一區的做法和順序，是優化的路線圖：小的段落在這裡做，大的另開 RFC。它取代 RFC 0012 的 `architecture-audit.md`（那份的 7 項的去向在 `details.md`）。

做完時（2026-10-18）：Speedometer 的 20 個 suite 裡，kun 對 Chrome 的總時間比從 3.54（第 2 段的 2d 之後，18 個 suite）降到 2.85（19 個；各 suite 比值的幾何平均 3.51 → 2.57），paint 和 layout 已經和 Chrome 差不多，GC 比 Chrome 少；剩下的差距七成五在測試的 sync 時間（JS 呼叫的 DOM、parse、script 要的 style），大項（DOM 的 handle、每個元素的 style 成本）放進「之後的可能」。數字在 `progress.md`。

## 動機

**差距**（`rfcs/0012-speedometer/progress.md` 的「收尾前最後一次的 trace」，計時區間裡的 self time，19 個 suite 合計，ms）：

| 類別 | kun | Chrome | 差 | 主要在 |
| --- | --- | --- | --- | --- |
| JS 和它呼叫的 DOM API | 5060 | 2240 | 2820 | jQuery、ES6-Webpack、chartjs、ES5、React |
| style（重算和 layout tree） | 2965 | 418 | 2550 | NewsSite、TipTap、Complex-DOM |
| HTML 的 parse（`ParseHTML`） | 1615 | 459 | 1155 | jQuery、ES6-Webpack、ES5 |
| paint 這一路（含 canvas） | 1318 | 371 | 950 | Complex-DOM、chartjs |
| 其他（`RunTask` 自己、frame 自己） | 554 | 411 | 140 | NewsSite、Complex-DOM |
| layout、GC、custom element | 812 | 836 | 0 | 已經和 Chrome 差不多 |

另外有一塊 trace 算不到：計時區間裡沒有被任何 span 蓋到的時間，kun 的 Preact-Complex-DOM 有 26%、ES6-Webpack 10%、NewsSite-Next 9%，Chrome 不到 1%（jQuery 兩邊都有一些等 frame 的閒置）。裡面混著閒置和 event loop 頂端沒有 span 的工作；第 1 段補上 span 再取樣，確認幾乎都是閒置：kun 一個 frame 做得比較久，之後等下一個 frame 的時間點（`details.md` 的「看不到的時間」）。

**比次數更重要的是每次的範圍**：NewsSite-Nuxt 裡 script 強制的 style 兩邊都是約 460 次，kun 每次 0.75 ms、Chrome 0.06 ms；jQuery 的 frame 裡的 style 每次 5.6 ms 對 0.5 ms。kun 做的次數不多，是每次做太多。

**目標**：照 Blink 的做法，先拿掉 kun 多做的事（全樹走訪、整個子樹的重算、多餘的配置和複製），再處理每單位的成本。每一段用 `ci.py speedometer --trace` 和 `--sample` 量前後，數字記在各自的 RFC。

**不在範圍內**：V8 本身（JIT 的 JS 在 jQuery 的樣本裡只佔幾 %）；layout 和 GC 的演算法（已經和 Chrome 差不多）；新的功能；Speedometer 以外的 benchmark。

要解決的 `docs/limitations.md` 裡的限制：「Invalidation」的以子樹為單位重算（D1、D2：只往下到繼承的值有變的地方；後代的 invalidation set 沒做，限制改寫）、structural selector 重算 parent 的子樹（D3：照旗標和 sibling invalidation set 只重算受影響的兄弟）、沒有 `MatchedPropertiesCache`（D4，限制改寫成它的條件）。沒解決的：`var()` 替換的快取（`MatchedPropertiesCache` 命中時整個跳過，第 20 條決定不另外做）、style sharing、Perf-Dashboard 的 rendering 時間偏少（沒查，放進「之後的可能」）。

## 設計概要

做完之後，網頁的行為不變，變的是 kun 在同一件事上做的工作量。對改這部分程式的人來說，原則是三條：

1. **工作量跟著改變的量走，不跟著文件的大小走**：改一個 class 只重算受影響的元素；frame 只走有變化的子樹；task 結束時不走整份文件。對應 Blink 的 invalidation set、`StyleRecalcChange`、`PrePaintTreeWalk` 的 dirty bit、`PaintChunk` 帶著的 hit-test data。
2. **熱的路徑上不配置**：DOM 操作裡拿到的節點不建 GC root、讀指標不是 FFI 呼叫、不複製字串和屬性。對應 Blink 的 stack 上的裸指標（conservative stack scanning）、inline 的 `Member` 讀取、`AtomicString`。
3. **能快取的就快取**：重算前後相同的部分沿用（`MatchedPropertiesCache`、`ShapeResult`、paint 的 subsequence），而不是比較完整的結果。

## 詳細設計

六區，每區列出 kun 現在怎麼做、Blink 怎麼做、做法（有選項的列選項和建議）、量到的影響和成本。程式的位置和量測的細節在 `details.md` 的同名小節。「大小」是實作的工作量（小：一兩天、一份小 RFC；中：一份 RFC；大：好幾段）。

### A. 每個 frame、每個 task 的全樹走訪（便宜、確定）

| # | kun 現在 | Blink | 做法 | 影響（量到的） | 大小 |
| --- | --- | --- | --- | --- | --- |
| A1 | 每次 paint 都走整棵樹算 wheel 的 handler region，頁面沒有 wheel listener 也走（`may_have_wheel_listeners` 只在算完之後用） | `EventHandlerRegistry` 記著哪裡有 listener，只標記受影響的物件；沒有 listener 就沒有 region | 沒有 wheel listener 時跳過；有時只在 listener 或 layout 變了時重算 | 204 ms（Complex-DOM 每個 suite 20 到 38 ms） | 小 |
| A2 | 每次 paint 都無條件走整棵樹找「被後面畫的東西蓋住的 composited scroller」，O(scroller × target) 加配置 | hit-test data 在 paint 時記進 `PaintChunk`（`ScrollHitTestDisplayItem`），沒有重畫的 chunk 沿用 | 先做：沒有 composited scroller 時跳過；之後照 E1 | 204 ms | 小（跳過）／大（E1） |
| A3 | 有 composited scroller 時，每次 paint 再做一次完整的 pre-paint（「unscrolled」的 property tree） | 一棵 property tree，composited 的捲動只改那個 scroll 的 translation | 合成一次 pre-paint，或只換掉 scroll offset 的 translation | 包在 `PrePaint` 的 220 ms 裡，要量 | 中 |
| A4 | 每次 script 跑完（26 個呼叫的地方），`start_loads` 走整份文件找 `<link>`、找 `<base>`、為了背景圖片先算一次 style 再走一遍 | `<link>` 在插入時開始載入（`HTMLLinkElement::InsertedInto`），背景圖片在 style 重算時登記（`StyleResourceLoader`），`<base>` 在它改變時更新 | `<link>`、`<base>` 改成在插入和屬性改變時記下；背景圖片在 style 重算時收集，不另外算一次 style、不另外走一遍 | 四次走訪：NewsSite-Next 的樣本約 10%、Preact 約 4%（第 1 段之後的取樣）。那一次 style 本來就要在 frame 裡算，先算掉只是挪了位置（Preact 的 frame 裡 lifecycle 就不必再算），不算多出來的 | 中 |
| A5 | 每一輪 event loop（不管有沒有 GC）`NodeIdMap::prune` 對整張 id 表的每一項做一次 FFI 呼叫 | `DOMNodeIds` 用到才編號，沒有每個節點的 weak persistent | 只在 GC 之後 prune（記下 GC 的次數）；之後照 B4 拿掉這張表的大部分用途 | 不在任何 span 裡（「看不到的時間」）；和存活的節點數成正比 | 小 |

**結果**：A1、A2 的跳過、A5 是第 1 段；A4 是第 5 段的 5a（`<link>`、`<base>` 在插入和屬性改變時記下，背景圖片在 style 重算時收集）；A3 是第 6 段的 6a1（一份 pre-paint，composited 的 scroller 只換掉捲動的 translation）；A2 的根本解 E1（第 24 條）讓 region 從 paint 記下的目標算，A1 的 wheel region 也是。

### B. DOM 的 handle 和 bindings

一次 `el.firstChild` 在 kun 約有 4 到 5 次 `Persistent` 的配置和釋放、2 次 `NodeId` 的 SipHash、2 次 tag 名稱的 SipHash、15 到 20 次不能 inline 的 cppgc FFI 呼叫；Blink 是 3 次 inline 的讀取加上 wrapper 的讀取（`details.md` 的「B. DOM 的 handle 和 bindings」）。jQuery 的計時區間裡 cppgc 的 handle 操作、配置、hash 合計約三成。原因是 `crates/dom/soundness.md` 的 I1：scope 以外不能有沒 root 的指標，`NodeHandle`（`cppgc::Persistent`）是唯一能活過 scope 的東西；V8 是預先編好的靜態庫，ThinLTO 跨不進去。

| # | 做法 | Blink | 影響 | 大小 |
| --- | --- | --- | --- | --- |
| B1 | `NodeIdMap`、`tag_kinds` 等以 id 或名稱為 key 的表改用 FxHash（`rustc-hash` 2，經由其他套件已經在 `Cargo.lock` 裡；kun 自己的 crate 原本沒有直接用）；`kind()` 存在節點上，不每次查 tag 名稱 | 沒有這些表；元素的型別是 C++ 的類別 | 樣本裡 hash 約 4% | 小 |
| B2 | 回傳節點時，wrapper 已經存在就不算 realm（`realm_context` 現在每次都算：三個 `Persistent`、一次 `get_creation_context`） | `DOMDataStore` 的 main world 快速路徑：直接讀物件裡的 wrapper | `realm_window` 在 jQuery 的樣本 1.5% | 小 |
| B3 | 表單的 pseudo-class（`:checked`、`:disabled`、`:default`……）在比對 selector 時才算，不在每次呼叫結束時走子樹重算、存起來（`update_form_states`） | `:checked` 等在比對時問元素（`IsChecked()`），狀態改了才 `PseudoStateChanged`；`:default` 走那個 form 自己的 listed element | `update_form_states` 在 jQuery 的樣本 6.7%（`update_elements` 的 12.1% 的大半；id 表的查詢 1.7% 歸 B1） | 中：動到 style，排在 RFC 0050 之後 |
| B4 | **少建 `Persistent`**：bindings 和 DOM API 的內部用 stack 上的 `UnsafePtr` / `NodeRef`，只在真的要活過 JS 的呼叫時才 root；parser 的 open elements 用 heap 上 traced 的 stack | Oilpan 掃 stack（conservative），stack 上是裸指標；`HTMLStackItem` 是 traced 的 `Member` | handle 的配置和 FFI 是最大的一塊（parse 裡 24%） | 大：要改 `soundness.md` 的 I1 和它的論證 |
| B5 | **`Member::Get` 和 `Persistent::Get` 在 Rust 裡 inline**：fork 公開 cage base，Rust 照 pointer compression 自己解壓縮；`Persistent` 不另外 `new` | `Member<T>` 的讀取是 inline 的載入（加上解壓縮），寫入的 barrier 有 inline 的快速路徑 | `Member::Get` 5.1%、`Persistent::Get` 3.3%、`WeakPersistent::Get` 等（self） | 中：fork 的 C++，要重編 V8 |
| B6 | 節點的狀態（屬性、id、class、文字）放進 GC 的物件，拿掉節點的 Rust finalizer | `Node` 幾乎都是 trivially destructible，屬性在 GC 的 `ElementData`，sweep 可以在別的 thread | 2026-10-13 量到 jQuery 7.5%；最後一次的 trace 的 GC 已經和 Chrome 差不多，排在最後 | 大 |

**結果**：B1、B2 是第 1 段；B5 是第 3 段（fork 公開 cage base，`Member`、`Persistent` 的讀取在 Rust 裡 inline）；B4 不放寬 I1，改做 `insert_before` 不為比較建 root、`Persistent` 重複使用、root 的計數和錨點 pool、clone 只加 count（第 6、8、10 條），完整的 B4 放進「之後的可能」；B3 是第 5 段的 5b2（表單的 pseudo-class 在比對時才算）；第 7 段的 7a 另外讓 bindings 照 interface 的型別取 `this` 的 wrapper（第 26 條）。B6 不做：GC 已經比 Chrome 少（第 26 條）。

### C. HTML 的 fragment 的 parse

jQuery 每次 fragment 的 parse kun 約 0.59 ms、Chrome 約 0.16 ms。kun 每次：建一份暫時的 `Document`、html5ever 的 tokenizer 和 tree builder、把整份字串複製進 tendril；每個元素的屬性複製兩次（`attributes()` 一次、`attrs.clone()` 一次），`create_node` 約 10 次 `Member` 的建構、一個 `WeakPersistent`、一個 `Persistent`、一次 SipHash 的插入；html5ever 每插入一個元素 clone 4 次 handle（每次一個 `Persistent`）；每次 append 都是完整的 `insert_before`；parse 完先把節點 adopt 進 context 的文件的 fragment（走整個子樹），再由 `replace_all` 再插一次。Blink 直接 parse 進 context 文件的 `DocumentFragment`，常見的 markup 走 `HTMLDocumentParserFastPath`（沒有 tokenizer 的狀態機），慢的路徑的 `HTMLConstructionSite` 用 `ParserAppendChild`（不做檢查）。

| # | 做法 | 影響 | 大小 |
| --- | --- | --- | --- |
| C1 | 直接 parse 進 context 文件的 fragment：不建暫時的文件、不 adopt；屬性只複製一次 | 每個節點少一趟 adopt 和兩次屬性的複製 | 中 |
| C2 | parser 專用的 append（像 `ParserAppendChild`：新節點、沒有 parent、同一份文件，跳過 `insert_before` 的檢查和走訪） | 每次 append 少兩次 `document_of`、到 root 的走訪 | 中 |
| C3 | 照 Blink 加 fast path：jQuery、ES5 的 template 這類簡單的 markup（沒有 `<script>`、`<table>` 的特殊規則）不經過 html5ever 的狀態機 | Chrome 在這些 suite 走完整的 parser，所以 C3 是超過 Chrome 的部分；先做 C1、C2、B4 再量 | 中到大 |

**結果**：C2、C1 是第 4 段（第 7 條）；C3 是第 8 段（第 27 條）：照 Blink 的 `HTMLDocumentParserFastPath`，fragment 和沒有 DOCTYPE 的整份文件（`DOMParser`）先掃過一遍，簡單的 markup 不經過 html5ever。

### D. style

kun 的重算以子樹為單位：元素一被重算，整個子樹都跟著重算（不管繼承的值有沒有變）；任何屬性改變都標記重算（不看有沒有 selector 用到、值有沒有變）；一個 tree scope 裡只要有一條 structural selector（`:first-child`、`~`、`:nth-*`），插入和屬性改變就重算 parent 的整個子樹；每個被重算的元素重新比對、cascade、套用每個 property、重新替換每個 `var()`。`StyleEngine::RebuildLayoutTree` 裡還包括 inline 內容的 shaping（Chrome 算在 `Layout`）。Blink：`StyleInvalidator` 和 invalidation set 只標記 selector 真的會影響的元素，`StyleRecalcChange` 只在繼承的值變了才往下，`MatchedPropertiesCache` 在比對結果和 parent 的繼承值相同時跳過套用，`ComputedStyle` 的欄位分組共用（`DataRef`）。

| # | 做法 | Blink | 影響 | 大小 |
| --- | --- | --- | --- | --- |
| D1 | **重算只往下到繼承的值有變的地方**：元素自己重算之後，比較新舊 style 的繼承部分，相同就不強制子元素 | `StyleRecalcChange`、`ComputedStyle::ComputeDifference`、`kLocalStyleChange` | 一個在高處的 class 改變（TipTap 的 `ProseMirror-focused`、`focus()` 觸發的 `:focus-visible`）從整個子樹變成一個元素 | 中 |
| D2 | **class、id、屬性的 invalidation set**：只在有 selector 提到那個 class 或屬性時才標記，class 比新舊兩組的差；設成一樣的值不標記 | `RuleFeatureSet`、`InvalidationSet`、`ClassChangedForElement`、`AttributeChangedForElement` | `data-*`、`aria-*`、沒人用的 class 不再重算 | 中到大 |
| D3 | **structural 的依賴照元素記**：比對時記下哪些元素受位置影響（`ChildrenAffectedBy*`），只重算受影響的兄弟 | sibling invalidation set、`ChildrenAffectedBy{First,Last,Forward,Backward}PositionalRules`、`CheckForSiblingStyleChanges` | Complex-DOM 的 CSS 約 40 條 `:first-child` 類、97 個兄弟 combinator：每次插入不再重算整個列表；NewsSite 的 `:checked ~` | 中 |
| D4 | `MatchedPropertiesCache`；`var()` 替換的結果快取 | `MatchedPropertiesCache`、`CSSVariableResolver` 的快取 | 每個仍要重算的元素的成本；NewsSite 有 379 個 `var(` | 中 |
| D5 | `display: none` 的子樹不先算 style（用到才算） | `EnsureComputedStyle` | 隱藏的選單、對話框 | 小 |
| D6 | 樣式表改變時，沒改的 `<style>` 不重新 parse，只對新規則的 feature 失效 | `text_to_sheet_cache_`、`CompareActiveStyleSheets`、`InvalidateForRuleSetChanges` | TipTap 的 `injectCSS` 讓整份文件重算 | 中 |
| D7 | `RebuildLayoutTree` 只在影響文字的改變時重新 shaping，shaping 的結果快取 | `NeedsCollectInlines`、`ShapeResult` 的快取 | TipTap 的 `RebuildLayoutTree` 加 `Layout` 是 Chrome 的 3.3 倍（531 對 160 ms） | 中 |
| D8 | `ComputedStyle` 的欄位分組、copy-on-write，`initial()` 共用 | `ComputedStyle` 的 `DataRef` 群組 | 每個元素的配置（`initial()` 約 34 個 `Arc`）和 118 個欄位的比較 | 大 |

**結果**：第 2 段做了 D1、D2 的元素自己的部分（feature 收集、「改了什麼」）、base URL、D3（2c、2d）；後代的 invalidation set 和 D5 沒做。第 5 段做了 D4（5c：cascade 的浪費和 `MatchedPropertiesCache`）、D7（5d2：shaping 的快取），連同 CSS counters 照清單編號（5d1）；D6 沒做。第 7 段的 7b 讓 `querySelector` 一次走訪只建一份 matching 的狀態。D8 和其他沒做的放進「之後的可能」。

### E. paint 這一路

kun 的 layout 是增量的，之後的每一步都不是：任何 style 或 box tree 的改變都讓 layout 標成 dirty，接著 pre-paint 從頭重建整份 property tree（`pre_paint_frame_tree`，節點的 id 是位置，前面加一個節點後面全部位移）；paint 走每一個 box，沒變的複製舊的 item（id 一位移就全部重畫）；之後兩個 region 各走一遍整棵樹（A1、A2）；layerization 每次重做。Blink：`PrePaintTreeWalk` 只往下走有 `NeedsPaintPropertyUpdate` 等標記的地方，property node 照物件持有、id 穩定；`SubsequenceRecorder` 跳過整個乾淨的 `PaintLayer`；hit-test data 在 paint 時記進 chunk；`PaintArtifactCompositor` 只在 `NeedsUpdate()` 時重新 layerize。

| # | 做法 | 影響 | 大小 |
| --- | --- | --- | --- |
| E1 | hit-test data（scroll、wheel、touch-action）在 paint 時記進 paint chunk，沒重畫的沿用 | A1、A2 的根本解；和 E3 一起才有完整的效果 | 大 |
| E2 | 增量的 pre-paint：property node 照物件持有、id 穩定，只更新標記的子樹；只有 paint 改變（顏色）時不做 layout 和 pre-paint | `PrePaint` 220 對 4 ms；id 穩定之後 paint 的快取也命中 | 大 |
| E3 | paint 的 subsequence 快取：乾淨的 stacking context 整段沿用、移動而不是複製 | `Paint` 266 對 108 ms | 大 |
| E4 | layerization 只在 layer 的結構可能改變時做，其他時候只更新重畫的 layer | `PaintArtifactCompositor::Update` 102 ms（Chrome 39 ms 加上 cc 那一側的 135 ms，這一項 kun 已經不慢） | 中 |

**結果**：全部在第 6 段做，沒有另開 RFC：E2 是 6a1、6a2（第 22 條），E3 是 6b1、6b2（第 23 條），E1 是 E1a、E1b（第 24 條），E4 做成 raster invalidation 以 chunk 為單位（第 25 條；layerization 跳過放進「之後的可能」）。第 6 段之後 paint 這一路和 Chrome 差不多。

### F. canvas

kun 的 2D canvas 把指令錄成 paint op，每個 frame 在 renderer 的 main thread 上用 CPU 的 Skia 畫進像素（`raster::draw_into`），再整張複製給 texture layer。Chrome 的 accelerated canvas 把錄好的 `PaintRecord` 序列化給 GPU process 畫（OOP-R），main thread 只序列化，texture layer 拿到 SharedImage，不複製像素。

| # | 做法 | 影響 | 大小 |
| --- | --- | --- | --- |
| F1 | canvas 的 raster 移到 raster thread（或 Viz）：main thread 只交出錄好的指令；texture 不整張複製 | chartjs 的 `raster::draw_into` 321 ms（main thread 上，Chrome 0） | 大：牽涉 process 的邊界（`docs/architecture.md` 的「Process 的邊界」）和 `getImageData` 的同步讀回 |

**結果**：沒做，放進「之後的可能」（第 26 條：最多拿到 Charts-chartjs 的 canvas 的 95 ms，成本是最高的一級）。

### 測試

每一段是自己的 RFC，照 `docs/testing.md` 有行為的測試和 mutation test；這裡要求的是**工作量的測試**：像 `cargo test -p style --test it matching::few_selectors_are_matched` 那樣數次數，不量時間，例如「改一個沒人用的 class 不重算任何元素」「插入一個 list item 只重算它和受 `:first-child` 影響的兄弟」「沒有 wheel listener 時不走 region」「frame 裡只動顏色時不 pre-paint」「parse 一個 fragment 的 `Persistent` 數量」。時間的前後由使用者要求時的 `ci.py speedometer --trace`、`--sample` 量，記在各自的 RFC 的 `progress.md`（Speedometer 平常不量，見 `docs/benchmarks.md` 的「效能的量測」）。

### 分階段

全部在這份 RFC 裡做（分支 `0051`，每段做完經過審核 merge 回 `main`）；原本說大的（B4、E、F）另開 RFC，最後 E 在這裡分段做，B4 的一部分和 F 放進「之後的可能」。每一段開始前照流程決定它的設計（第 2 條）：

1. **全樹走訪的跳過和 hash**：event loop 頂端的工作補上 span，A1、A2（跳過）、A5、B1、B2。小、行為不變、照 Blink 的結果。
2. **style 的範圍**：D1、D2（2a），base URL（2b），D3（2c、2d）；後代的 invalidation set 和 D5 沒做。
3. **handle 的成本**：B5（fork），接著 `insert_before` 不為比較建 handle、`Persistent` 重複使用（第 6 條）；第 4 段之後再一段：root 的計數、錨點 pool、不查 id 表建 root（第 8 條）。
4. **fragment 的 parse**：C2（先讓一般的插入便宜，再加 parser 專用的 append）、C1（第 7 條），量過再決定 C3（root table 見第 8 條）。
5. **每個 task 的收尾和 style 的快取**：A4（5a）、B3（5b）、D4（5c）、D7 和 CSS counters（5d）；A3 移到第 6 段，D6 沒做。
6. **增量的 paint**：E2（6a）、E3（6b）、E1、E4。
7. **便宜的兩項**（第 26 條）：7a bindings 取 `this` 的 wrapper 照 interface 的型別，7b `querySelector` 的快速路徑和快取。
8. **fragment 的 parse**（第 26、27 條）：C3；文字（shaping 和斷行）放進「之後的可能」。


## 缺點

- **增量的做法要有失效的正確性**：漏了一個標記就是畫面不更新，這類 bug 比慢更難找（Blink 為此有大量的 invalidation 測試）。每一段要帶著它的失效測試，WPT 的 `css/selectors/invalidation/` 要跑。
- **B4 動到 GC 的 soundness 論證**：`crates/dom/soundness.md` 的 I1 是整個 DOM 安全的基礎，放寬它要重寫論證、補 compile-fail 測試，跑 `test.py matrix`、`fuzz`。
- **B5 改 fork 的 C++**：要重編 V8、發 release，升級 V8 時要跟著 cppgc 的 pointer compression 的布局。
- **E、F 是大的重寫**：paint 和 canvas 的資料結構都要換，期間行為要保持一樣（reftest、WPT 的 reftest 守著）。

## 理由和其他做法

- **照 Blink，不自己發明**：差距幾乎都是 kun 做了 Blink 不做的事，Blink 的做法是現成的答案，名稱和分層也照它（`CLAUDE.md`），之後讀兩邊的 trace 才對得起來。
- **先拿掉多做的，再降每單位的成本**：範圍縮小是倍數的改善（style 每次 0.75 ms 對 0.06 ms，是範圍的問題），降低常數（hash、配置）是百分比的改善；先縮範圍，常數的效果才看得出來。
- **不從 V8 下手**：jQuery 的樣本裡 JIT 的 JS 只有幾 %，V8 的 C++ 和 builtin 也不多，差距在 V8 叫進來的 Rust。
- **不選「一份 RFC 做完全部」**：六區改的 crate 不同、設計各自有選項，放在一起會變成好幾個月的 branch；照 RFC 0012 的路線圖的做法，一區一份或一段一份。
- **不選先做 E（paint 的增量）**：它是 paint 這一路的根本解，但工作量最大，而 A1、A2 的跳過幾天內就拿到這一區量得到的差距的約四成（408 對 950 ms）。

## 先例

- Blink：`StyleInvalidator` / `InvalidationSet`、`StyleRecalcChange`、`MatchedPropertiesCache`、`PrePaintTreeWalk`、`PaintController` 的 subsequence、`HitTestData`、`HTMLDocumentParserFastPath`、Oilpan 的 conservative stack scanning。
- Stylo（Servo、Firefox）：restyle hint（invalidation）、style sharing cache、rule tree；`var()` 的替換在 custom property 的計算時一次做完。
- WebKit：`StyleInvalidator`、`MatchedDeclarationsCache`（`MatchedPropertiesCache` 的同類）。

## 已決定的事項

1. **順序：照碰到的程式和 RFC 0050 平行，不照編號**（2026-10-16）：RFC 0050（表單控制項的繪製，CSS 線）同時在另一台裝置上做，它的第 2、3 段改 style（UA stylesheet、`::placeholder`）、layout、paint（theme）和 `dom` 的 UA shadow root。這份先做不碰 style 和 paint 的段落：第 1 段（全樹走訪的跳過和 hash），接著第 3 段（handle）、第 4 段（fragment 的 parse），都在 `dom`、`dom_api`、`bindings`、`html`、fork；第 2 段（style 的範圍）、第 5 段（task 的收尾和 style 的快取）、第 6 段（增量的 paint）等 RFC 0050 merge 回 `main` 之後再開始，開始前重量一次當基準。理由：RFC 0050 的第 2 段讓 kun 開始畫 Chrome 本來就畫的控制項（TodoMVC 每頁都有 checkbox 和輸入框），之後 style、paint 的工作量會變；先優化再補上控制項，前後的數字會被它攪亂。不選「RFC 0050 停到這份做完」：這份是七段的路線圖，會停很久，而真正重疊的只有第 2、5、6 段。
2. **這份 RFC 是路線圖；第 1 段的設計**（2026-10-16，第 1 段開始前）：小的段落在這份 RFC 裡做（分支 `0051`，做完一段先 merge 回 `main`，和 RFC 0012 一樣），B4、增量的 paint、canvas 這類大的另開 RFC，最後一段做完時收尾。不選每段各開一份 RFC：第 1 段這種一兩天的改動，流程比實作重。第 1 段：
   - **量測的補強**：event loop 頂端在 task 之間的工作補上 span（kun 的名稱，`RenderThread::…`），「看不到的時間」裡有工作的部分看得到；第 1 段做之前和做之後各用 `--sample` 取樣 Preact-Complex-DOM、NewsSite-Next、jQuery（使用者同意跑）。
   - **A1**：`may_have_wheel_listeners()` 是 false 時給空的 region、不走樹（和 Blink 一樣：沒有 listener 就沒有 region）。不選快取 region、只在 listener 或 layout 變了時重算：要記 layout 的版本，而有 listener 的頁面的根本解是 E1。
   - **A2**：沒有 composited scroller 時不呼叫 `covered_scrollers`（它的結果只用在 composited 的 scroller 上，結果一樣）。
   - **A5**：攤銷：id 表比上次 prune 之後大一倍才 prune，每次插入平均 O(1)、和 engine 無關；死掉的節點的項目最多多留一倍，`get` 本來就濾掉死的。不選只在 major GC 之後：要在 `bindings` 接 V8 的 GC callback，wasm 的 engine 也要各自接；不選每輪 prune 一段：結果差不多、要多記游標。
   - **B1**：`NodeIdMap`、`StoreTable`、`tag_kinds` 改用 FxHash（`rustc-hash` 2，經由其他套件已經在 `Cargo.lock` 裡；kun 自己的 crate 原本沒有直接用，草稿誤寫成 `css` 已經用）：key 是內部的流水號或只查不插的固定表，沒有 HashDoS 的問題。不選以 id 為索引的 `Vec`：id 不重用，`Vec` 隨建過的節點數一直長。
   - **B2**：回傳節點時 wrapper 已經存在就不算 realm，新建 wrapper 時才算（Blink 的 `DOMDataStore` 的快速路徑）。
   - **B3 改成表單的 pseudo-class 在比對時才算**，移到第 5 段：jQuery 的取樣裡 `update_elements` 的 12.1% 大半是 `update_form_states`（6.7%），「沒有紀錄時跳過」省不到什麼，id 表的查詢（1.7%）歸 B1；比對時才算是 Blink 的做法，動到 style，排在 RFC 0050 之後。
   - **測試**：照 `style` 的 `matched_selectors_for_testing`，在走訪的地方加計數，測試比較條件不同時的次數（沒有 wheel listener 時 region 不走、沒有 composited scroller 時 `covered_scrollers` 不走、插入很多節點之後 prune 的次數是對數級、回傳已有 wrapper 的節點時不建 realm 的 handle）；行為由現有的 hit test、捲動、wheel 的測試和情境守著。
3. **A4 提早，接在第 1 段之後做**（2026-10-16，第 1 段做完時）：第 1 段做之前的取樣裡，`start_loads` 每次 script 跑完為了找背景圖片先算的那一次 style，在 Preact-Complex-DOM 佔 23.6%、NewsSite-Next 11.5%、jQuery 3.6%，比第 1 段每一項都大。原本排在第 5 段（第 1 條：碰到 style，等 RFC 0050），但它和 RFC 0050 重疊的地方不大：RFC 0050 改的是 UA stylesheet、`::placeholder`、theme，A4 改的是 style 重算時順便登記背景圖片和 `FrameView::start_loads`。不選照原計畫等 RFC 0050：目前量到最大的單一項要等很久。
4. **A4 放回第 5 段**（2026-10-16，A4 開始前）：細看第 1 段之後的取樣，第 3 條的數字算錯了地方：Preact、jQuery 的 `start_loads` 幾乎都是 rAF callback 之後、同一個 frame 裡呼叫的，它算的那次 style 本來就是那個 frame 要算的（Preact 的 frame 裡 `Document::UpdateStyle` 21 次全在這裡、lifecycle 裡沒有），拿掉只會挪回 lifecycle，和 Chrome 在 frame 裡算的一樣多。真正多出來的是四次全樹走訪（`<base>`、`<link>`、module preload、背景圖片）：NewsSite-Next 約 10%、Preact 約 4%、jQuery 幾乎沒有。照這個量，回到第 1 條的順序，等 RFC 0050 之後和第 5 段一起做。
5. **第 3 段的 B5：cppgc 的讀取在 Rust 裡 inline**（2026-10-16，第 3 段開始前）：第 1 段之後的取樣裡，四個讀取用的 FFI 函式自己佔 jQuery 11.5%（`Member::Get` 5.7%、`Persistent::Get` 3.6%、`WeakPersistent::Get` 2.2%）、NewsSite-Next 5.8%、Preact 2.8%。x64、arm64 上 cppgc 有 caged heap，因此開著自己的 pointer compression：`Member::Get` 是 `((i32 as i64) << shift) & cage_base`（`CompressedPointer::Decompress`），cage base 在第一個 heap 建好之後不變；`Persistent::Get` 是讀 `PersistentBase` 的第一個欄位 `raw_`。Blink 的 `Member::Get` 就是 C++ 標頭裡 inline 的同一件事。
   - **範圍**：只做讀取（`Member`、`WeakMember`、`Persistent`、`WeakPersistent` 的 `Get`）；寫入（write barrier）和 `Persistent` 的建立、刪除留在 C++：persistent region 的 node 記著 `Persistent` 自己的位址，它不能移動，所以 fork 用 `new` 放在 C++ 的 heap 上，少建是 B4 的事。
   - **cage base 和 shift**：fork 加 `cppgc__CageBase()`，`binding.hpp` 匯出 shift 和是否壓縮（和 `cppgc__Member_SIZE` 一樣經過 bindgen），Rust 第一次讀時查一次 cage base、快取起來。不選 Rust 從 heap 上的指標反推、自己寫死 shift：完全依賴 V8 內部的常數，升級 V8 時可能默默算錯。
   - **正確性**：Rust 的 debug build 裡每次 inline 的讀取都再呼叫一次 FFI，不同就 panic；release 只走 inline。`raw_` 是 protected 的，`binding.cc` 不能 `offsetof`，靠這個比對守著（所有測試的每一次 DOM 存取都比一次）。
   - **release**：改了 fork 的 C++，發 `kun-v152.2.0-9`；先用本機重編的 V8 測過。V8 本身沒變，prebuilt 的大小應該不變。
   - **量測**：做完後同樣取樣 jQuery、Preact-Complex-DOM、NewsSite-Next（使用者同意）。
6. **B4 不放寬 I1；第 3 段改做兩件不碰 soundness 的事**（2026-10-16，B5 做完、量過之後）：B5 之後的取樣裡，建立和刪除 `Persistent`（連同它們呼叫的 malloc、free）佔 jQuery 12.4%、Preact 1.1%、NewsSite-Next 1.0%；jQuery 的這一塊照呼叫的地方分：parser 的 sink 5.5%、`insert_before` 約 4%、`dom_api` 和 `to_handle` 3.4%；建一個 `Persistent` 的成本約七成是 fork 用 `new` 配置它的 malloc，persistent region 自己的 free list 很便宜。
   - **`insert_before` 直接比 document 的指標**：現在每次插入都為 parent 和 child 的 document 各建一個 handle，只為了比是不是同一份；改成在 scope 裡讀兩個 edge、比位址（中間不配置），要 adopt 時才建 handle。Blink 的 `Node::insertBefore` 也是比 `GetDocument()` 的指標。
   - **`Persistent` 重複使用**：handle 丟掉時，它的 `Persistent` 設成 null（node 還給 persistent region，不再是 root）、留在 `DomIsolate` 的 pool 裡（有上限），下次建 handle 時拿出來設到新的物件，pool 空了才配置。用 fork 原有的 `Persistent::set`，不改 C++、不重編 V8。不選在 fork 的 `binding.cc` 裡做 free list：效果一樣，但要重編 V8、發新的 release、再換一次大家共用的 prebuilt。
   - **parser 的 handle 移到第 4 段**：和 C1、C2 一起照 Blink 的 `HTMLStackItem` 做（open elements 放在 heap 上 traced 的 stack），也不用改 I1。
   - **不做完整的 B4**（stack 上的裸指標、放寬 I1）：上面三件做完，估計 jQuery 剩約 3% 到 4%、Preact 和 NewsSite-Next 約 1%，不值得重寫整個 GC 的論證、另開 RFC。記進「之後的可能」，之後的取樣顯示 handle 又成了熱點再考慮。Blink 不需要這些，是因為 Oilpan 保守地掃 stack。

7. **第 4 段：C2、C1；parser 的 handle 改由之後的 root table 處理**（2026-10-16，第 4 段開始前）：第 3 段之後 jQuery 的取樣（兩次合計）裡 `innerHTML` 的 setter 佔 56.4%：html5ever 的 tokenizer 14.5%、tree builder 11.4%，append（`insert_before`）13.5%（parser 的 sink 7.3%、parse 完 adopt 進 context 文件的 fragment 4.2%、`replace_all` 1.9%），建節點 6.8%，root 4.8%，屬性的 hook 2.2%。append 裡最大的一項是 `has_style_sheet_owner`（3.3%）：每次插入都配置一個 `Vec`、走整棵插入的子樹找 `<style>`、`<link>`。
   - **C2 先讓一般的 `insert_before` 便宜**：`has_style_sheet_owner` 不配置、只有一個節點時不走；插入時的子樹走訪（style sheet、iframe、表單）合成一趟（做的時候：iframe 的那一趟走進 shadow tree、順序不同，而且只在有 browsing context 的文件裡走，留著分開，只讓單一節點不配置）。JS 的 `appendChild`、`replace_all` 也受益，不改介面。Blink 插入時對子樹也是一趟（`NotifyNodeInserted`）。
   - **接著 parser 專用的 append**（Blink 的 `ParserAppendChild`）：新節點、沒有 parent、同一份文件（debug 檢查），跳過前置的合法性檢查、往上找祖先、拆下和 adopt；style 的旗標、iframe 和表單的紀錄、mutation record 照做（頁面的 parse 也走這個 sink，MutationObserver 要看得到）。
   - **C1：sink 的 fragment 模式**：節點直接建在 context 的文件裡，html5ever 的「document」換成新的 `DocumentFragment`；它的 `<html>` 接在 fragment 底下，parse 完把 `<html>` 的子節點搬到 fragment（同一份文件，不 adopt、不走子樹），屬性只複製一次（做的時候：`<html>` 不接上去，接到它底下的直接接到 fragment，連搬都不用；屬性的第二次複製沒省：hook 在建好之後要名稱和值，而 `create_element` 拿走 `Vec`，要改 `dom` 的介面，量過再說）。fragment 模式給屬性的 hook 的是「沒有文件」：現在暫時的文件沒有 window，`<body onload>` 不會設到 window 上，結果不變；custom element 和圖片照 WPT 的相關目錄確認。不選 fork html5ever、讓 fragment 當 open elements 的底（Blink 的 `HTMLConstructionSite` 的做法）：多一個 fork，只省下搬最上層節點的那一步。
   - **parser 的 handle 不在這一段做**（改掉第 6 條的「移到第 4 段」）：jQuery 剩下的 root 10.2% 裡 parser 只有 3.6%、`dom_api` 5.2%，只替 parser 做一張 traced 的表解決不到一半。改成之後考慮一份 root table 的 RFC：每個 isolate 一張 traced 的 root 表（一個 GC 物件，自己用一個 `Persistent` root 住），`NodeHandle` 是一格的編號加上 reference count，建、丟 root 是寫一格 `Member`，clone 只加 count；所有 root 都受益，parser 不用改。I1 不變（handle 仍是 root），但它的論證、I8、`Weak` 要重寫，跑 GC 的壓力測試，所以另開 RFC；第 4 段量過（C1、C2 會讓 parser 和插入建的 root 變少）、root 仍是明顯的一塊再開。V8 的 `HandleScope` 也是「handle 是表格裡的一格」。
   - **量測**：做完照第 3 段的做法，同一段時間裡新舊的程式交替各跑兩次，比 `set_inner_html` 和 append 的樣本，不比總數。

8. **root table 不做；改做計數、錨點 pool、不查 id 表**（2026-10-16，第 4 段做完、量過之後）：第 4 段之後 jQuery 的 root 仍是 9.5% 到 10.3%（`progress.md` 的第 4 段），第 7 條說的條件成立。開 RFC 之前，請兩個 subagent 各自獨立評估（第二個不知道第一個的結論，拿到同一組候選做法自己排序），兩者的主要結論一致，第 7 條的 root table 被否決：
   - **traced root table 不做**：表格是 GC 物件，長大時要配置 GC 記憶體，而 `NodeRef::to_handle` 在 read scope 裡建 root（I2：read scope 裡不 GC），`handle_of` 常在手上還有 edge 讀出的 `&Node` 時呼叫（I3）；只能預留空格、用完退回 `Persistent`，handle 變成兩種表示。表格只在 marking 開始和最後各看一次，之後寫的每一格都要 write barrier（`Persistent` 是在 atomic pause 重新掃的 root，不用），這種寫入永遠不能 inline。它多出來的好處只有 clone 只加 count，下面的做法不用 GC 表也拿得到。
   - **錨點 pool**：cppgc 的 `Persistent::Assign` 在舊值、新值都有效時只改指標、留著 persistent region 的 node（`persistent.h` 的 `Assign`）；第 6 條的 pool 把空閒的 `Persistent` 清成 null，每個 root 因此還、拿 region 的 node 各一次（量到兩次 assign 約 4%）。改成每個 isolate 一個常駐的錨點 GC 物件，空閒的 `Persistent` 指向它，建、丟 root 都是「有效 → 有效」。第二個評估找到的前提，照著做：錨點自己由 `Roots` 的一個 `Persistent` root 住（只靠空閒的指著它，pool 拿空時它會被收、位址被重用）；空閒的 `Persistent` 的型別是錨點的，設到新物件之後才轉型，絕不讀成 `Node`；「root 的物件和錨點在同一個 heap」成了前提（每個 `DomIsolate` 一個 pool，現在成立，寫成不變式）；空閒的 `Persistent` 丟掉時會碰 heap（還 region 的 node），要在 heap 之前（I8）；錨點在建 isolate 時配置、立刻 root（I3 的清單）。估計省 2.5% 到 3%（兩個評估一樣，推估）。
   - **不查 id 表建 root**：`to_handle`、`handle_of` 現在先查 `NodeIdMap`（hash 表，讀它的 `WeakPersistent`）才拿到 cppgc 要的指標；fork 的 `RustObjConcrete` 是 `#[repr(C)]`，從 `&T` 減去 value 的位移就是 `*mut RustObj`，fork 加一個只改 Rust 的 helper。估計 0.5% 到 2%（推估）。
   - **計數**：debug 的計數器分開數 clone、從參考建立、pool 的落空；寫成工作量的測試（parse 一個 fragment 每個元素建幾次、clone 幾次）。release 的取樣裡 clone 是 `Root::new::<Persistent>`、從 id 表建立是 `Root::try_new::<WeakPersistent>`，第 4 段之後各約 1.3%。
   - **之後看量的**：clone 明顯的話，做 pool 化的 `Rc` root cell（handle 裡是一個 `Rc`，clone 只加 count，所有呼叫端都受益；第二個評估提的），或只把 parser 的 handle 換成 `Rc<NodeHandle>`；錨點 pool 之後 `Persistent::Assign` 的 FFI 仍超過約 0.5% 時，把「有效 → 有效」的寫入 inline 到 Rust（strong persistent 只在 main thread 被讀，debug 和 C++ 比對）。`this` 和參數不建 root（callback 的 `Local` 已經撐住它們）要改 I1 的寫法，另開 RFC；off-heap 的 root 陣列（不用 barrier、長大不 GC，要改 `binding.cc`、重編 V8）兩個評估意見不同，root 仍是明顯的一塊時再比。完整的 B4 兩個都否決（單獨的 heap 不掃 stack，第 6 條）。
   - 這取代第 7 條「之後考慮 root table 的 RFC」。Blink 不需要這些（Oilpan 保守地掃 stack）；`HandleScope` 的類比不準：V8 的 `HandleScope` 是 LIFO 的 arena，root table 比較像 V8 的 global handles，也就是 cppgc 已經有的 persistent region。

9. **量測的方法：固定週期的 cycle 和指令數、自動交替的比較**（2026-10-17，第 3 段（續）量過之後）：第 3、4 段和第 3 段（續）的比較，總樣本數都比不出結果：`--sample` 的 `perf record` 是固定頻率的 `cycles:u`，樣本數跟著時間走（CPU 的頻率、溫度、背景的程式），同一段程式換一個 build（inline、函式的位置）也可能差一兩成（第 3 段（續）裡 tokenizer 沒改，舊的 build 卻快四成：推測，還沒驗證），每邊又只跑兩次，只好改看直接動到的部分、手動換檔案交替跑。
   - **A：兩個事件、固定週期**：`perf record -e cycles:u -e instructions:u -c 1000000`，樣本數就是 cycle 數和指令數（各以百萬計），不再跟著時間；`sample/` 裡 cycle 的照舊是 `<suite>-kun.folded`，指令的是 `<suite>-kun.instructions.folded`，`sample.md` 兩者都列（和 IPC）。指令數幾乎不受頻率和 build 的佈局影響，同樣的工作應該很接近；但它看不到 cache miss 這類記憶體的成本，所以和 cycle 並列，不取代它。用 `perf stat` 確認過：固定週期的樣本數等於總數除以週期。
   - **B：`--sample --against REV`**：在 repo 旁邊的一個 git worktree（`../kun-against`，`../third_party` 照樣找得到）checkout `REV`，用另一個 target 目錄 build，和現在的工作目錄交替各跑 `--runs` 次（預設 3），寫出 `compare.md`：每個 suite 兩邊每次的 cycle、指令數，中位數、範圍和中位數的差，以及指令數差最多的函式。兩邊共用 `../third_party` 的 checkout，所以 `REV` 的 `third_party.toml` 和 `.cargo/config.toml` 要和現在的一樣，不一樣就停下。Chromium 的 Pinpoint 比較兩個版本也是兩邊各重複跑很多次。
   - **C：機器的狀態**（固定 CPU 頻率、關 turbo、綁 core）要 root、只對一台機器有效，寫進 `docs/benchmarks.md` 的「效能的量測」當作量之前的建議，不放進工具。
   - 不選只多跑幾次（不改事件）：跑得越多越慢，而 build 佈局的差異不會因此消失；不選只看指令數：記憶體的成本看不到。

10. **clone 只加 count：pool 化的 `Rc` root cell**（2026-10-17，量測的方法做完之後）：第 3 段（續）之後，用指令數算，jQuery 的 clone（`Root::new` 從一個 `Persistent`）約 0.75%、丟 root 約 1.69%；fragment parse 的 root 約八成是 html5ever 的 clone。
   - **做法**：`Root` 是一個 `Rc<RootCell<T>>`，`RootCell` 持有 `Persistent`、`RootCounter` 和物件的指標。clone 是 `Rc` 的 count 加一，不經過 FFI；丟掉時只有最後一個把 `Persistent` 指回錨點、整個 cell 放回 pool（不再 malloc）。所有呼叫端都受益，只改 `dom` 的 `handle.rs`；`Handle` 從 16 bytes 變成 8。不選只把 parser 的 handle 換成 `Rc<NodeHandle>`：每個元素多一次 malloc，而且只有 parser 受益。Blink 的 `Persistent` 複製時也在 region 裡登記一個新的 root，這比它再省一步。
   - **cell 存物件的指標**：讀物件時 `Rc` → cell 裡的指標 → 物件，和現在一樣兩層（不存的話要再經過 C++ 的 `Persistent`）。`Persistent` 撐著物件，DOM 的 space 不 compact（A2），所以指標一直有效；debug build 每次讀都和 `Persistent` 比對。
   - **pool 裡的 cell**：型別照舊是 `RootCell<Node>`（只留節點的，用 `Any` 比對型別，不需要第 8 條的 `retype`），`Persistent` 指著錨點、物件的指標清成 null；只有 `Roots` 拿得到，取出來先設定才交出去。cell 持有 `RootCounter`，pool 又在 `Roots` 裡，形成循環，由 `DomIsolate` 拆掉時的 `drop_spare` 打斷。
   - **活著的 root 數 cell**（不同的 root），不再數 handle；拆掉時還有就 abort 照舊。
   - **量測**：`--sample --against main`，看 jQuery 的指令數（A/A 之下差超過約 2% 才算）。估計拿掉 clone 和丟 root 合計 2.4% 裡的 1.5% 到 2%（推估）。

11. **第 2 段的基準和 D1 的設計**（2026-10-17，第 2 段開始前；RFC 0050 已經做完）：
   - **基準**：20 個預設的 suite 各取樣一次（`--sample`、3 個 iteration，cycle 和指令數），記下每個 suite 的 style 佔多少，第 5、6 段也用它；之後每一步用 `--against` 比，suite 照基準挑（預計 Preact-Complex-DOM、NewsSite-Next、Editor-TipTap 和一個 Complex-DOM，各 3 次）。不選只量幾個：RFC 0050 改了 style、layout、paint，第 5、6 段開始時還要再量。
   - **D1 先**（「待決定」的第 1 條）：改動集中在 `style` 的走訪，class、屬性、state 每一種改變都受益；D2 要 RuleSet 收集 feature，和 D3 一起。
   - **D1 的條件**（第 12 條改寫了它的前提：D1 依賴 invalidation set；Blink 的 `ComputedStyle::ComputeDifference`、`StyleRecalcChange`）：元素重算之後，只在這三種情況強制子元素重算：會繼承的欄位變了（`ComputedStyle::inherit` 那些，連同 custom property）；`display` 變了（子元素的 blockification、`display: contents`）；子元素用 `inherit` 或 `unset` 取了 parent 不繼承的屬性（cascade 時在 parent 上記下，Blink 的 `ChildHasExplicitInheritance`），而 parent 有不繼承的欄位變了。會繼承的欄位只寫一份清單，同時產生 `inherit()` 和比較，新增的繼承屬性不會只改了一邊。不選只在新舊完全一樣時不往下：抓不到最常見的「class 只改了自己的背景」；不選每個子元素記下讀了 parent 的哪些屬性：每個元素多一份資料。
   - **測試**：計數（重算了幾個元素），三種要往下的情況各一個。

12. **D1 依賴 invalidation set：第 2 段分成 2a、2b、2c**（2026-10-17，D1 開始寫之前）：元素重算時強制整棵子樹，擋住的不只繼承，還有後代的 selector：`.big span` 在 `#b` 加上 `big` 時，`#b` 自己的 `width` 不繼承，照第 11 條的 D1 不往下，`#b1` 卻要因為 `.big span` 重算（`style` 的 `invalidation::a_changed_attribute_styles_the_element_and_its_subtree_again`）。Blink 的這一半由 invalidation set 負責（`RuleFeatureSet` 收集 selector 用到的 feature，`ClassChangedForElement` 等標記受影響的後代），`StyleRecalcChange` 只管繼承。第 11 條把 D1 排在 D2 前、當作可以單獨做，是評估錯了。
   - **2a：feature 收集和「改了什麼」，加上 D1**：style 收集出現在 combinator 左邊（祖先或前面的兄弟的位置）的 class、id、屬性名和 state（Blink 的 `RuleFeatureSet`；`:has()` 或分析不了的照舊當作全部都用到）；dom 記下每個元素改了哪個 class（新舊的差）、id、屬性，`dom` 不依賴 `style`，所以記在清單裡、重算時取走（和 `element_changes` 一樣）。改到的有出現在祖先的位置，照舊重算整棵子樹；沒有的話用第 11 條的 D1 判斷子元素。state 的改變一樣。兄弟的部分照舊（structural）。
   - **2b：後代的 invalidation set**：只重算真的符合的後代（Blink 的 descendant invalidation set），不是整棵子樹。
   - **2c：兄弟（D3）**：照元素記位置的依賴、sibling invalidation set。
   - 每一步用 `--against` 比 NewsSite-Nuxt、TodoMVC-Backbone、NewsSite-Next、TodoMVC-Preact-Complex-DOM（第 2 段的基準）。正確性由 `style` 的 invalidation 測試守著：每次改動之後拿整份重算的結果比對每個元素。不另開 RFC：它就是第 2 段，照前幾段分步做、每步量。

13. **2b 改成 base URL，2c structural，後代的 invalidation set 延後**（2026-10-17，2a 之後、2b 開始前）：暫時在 style 裡數每個重算的元素是哪個原因（整份重算的哪一種、新節點、structural、祖先位置的屬性或 state、繼承、只有自己），每次重算一行，照順序對上 trace 的 `Document::UpdateStyle`，只加計時區間裡的（`progress.md` 裡 2b 開始前的那一節）。四個 suite 裡第 12 條的 2b（祖先位置的屬性用後代的 invalidation set）的範圍都是 0：NewsSite 的是 base URL 改了的整份重算（約 58%，`pushState` 換頁）和 structural 的子節點 state（約 42%，`:checked ~`），Preact、Backbone 的是插入加上 structural（約 90%、85%）。所以：
   - **2b：base URL 改了只重算用到它的元素**：cascade 時用文件的 base URL 解析過相對 `url()` 的元素在 `ComputedStyle` 記下（像 `has_viewport_units`），base URL 改變時照樣走過每個元素，只重算記了的。不選看所有 sheet 和 `style` 屬性有沒有相對的 `url()`、都沒有就不重算：頁面有一個就照舊整份重算。
   - **2c：structural（D3）**，插入和子節點的 state 都在內，設計等 2b 量過再列。
   - **後代的 invalidation set 延後**：計時區間裡量不到它的範圍，之後的取樣看到再做。
   - **後來發現歸因錯了**（2b 做完量過之後）：統計時「被強制」優先於「新節點」，NewsSite 換頁時 `pushState` 和換掉整頁的內容在同一次重算裡，那些本來就要算的新節點被記成了 base URL 的；新節點優先再數一次，base URL 的是 0，原本的「base URL」大多是新節點和 structural（`progress.md` 的 2b 那一節）。2b 照樣做完、留著（只改網址、不換內容時不再整份重算），但在這些 suite 裡沒有效果；NewsSite 剩下的是新節點和 structural，2c 的範圍。

14. **2c：插入和移除照比對時記下的旗標，只重算受影響的兄弟；兄弟位置的 state、屬性延後成 2d**（2026-10-17，2c 開始前）：暫時數了計時區間裡每次 structural 重算的範圍（parent 的整棵子樹、改變的子節點起之後的兄弟、改變的子節點和下一個）：子節點改變時 Backbone 21642、17139、8901，Preact 3741、3687、1923，NewsSite-Nuxt 16569、24、24，NewsSite-Next 三者都是 10422（移除：找不到改變的子節點）；子節點的 state（`:checked ~`）NewsSite 8550、8550、3600。
   - **旗標**：比對時要 `selectors` 的 `ElementSelectorFlags`（`NeedsSelectorFlags::Yes`），記在節點上（parent 的：`:nth-last-*` 等要所有子節點、`:nth-child`、`+`、`~` 要之後的兄弟、`:first-child`、`:last-child` 要第一個和最後一個；自己的：`:empty`）。子節點改變時照 parent 的旗標只強制那些子節點（連同子樹），parent 自己只在 `:empty` 時重算。`selectors` 的 `+` 和 `~` 是同一個旗標：tree scope 的規則沒有 `~`、沒有往前數的位置（`:nth-child` 等）時，「之後的兄弟」縮成接在改變後面最長的一串 `+` 那麼多個（每個 tree scope 收集一次，和 2a 的 feature 一起）。Servo 用同樣的旗標；Blink 的 `CheckForSiblingStyleChanges` 照 `ChildrenAffectedBy*` 分得更細（`+` 只重算下一個），`~` 和往前數的位置一樣重算 parent 的子樹。
   - **改變的位置**：dom 在插入和移除時給改變點後面的兄弟、前面的兄弟各記一個旗標，屬性、state 改變時給後面的兄弟記（`.a + .b`）；現在只有 parent 的一個旗標，移除時不知道在哪裡（NewsSite-Next）。Blink 也在當下把前後的兄弟交給 `CheckForSiblingStyleChanges`。
   - **2d：兄弟位置的 state、屬性用 sibling invalidation set**（NewsSite 的 `:checked ~ .dropdown-label …`：之後的兄弟就是整棵子樹，只有只重算符合的兄弟和其下符合的後代才省得到），和延後的後代 invalidation set 是同一套機制，之後再決定是不是另開 RFC。不選這次只重算之後的兄弟：NewsSite 省不到。
   - 量測照樣用 `--against`（Preact、Backbone、NewsSite-Nuxt）和計時區間的統計。

15. **2d：兄弟位置的 state、屬性改變用 sibling invalidation set，照 feature 記，在重算的走訪裡套用；留在這份 RFC**（2026-10-17，2c 做完、2d 開始前）：暫時數了 2c 之後計時區間裡經過兄弟的 state 重算的元素和其中結果沒變的：NewsSite-Nuxt、NewsSite-Next 都是 4950 個裡 4500 個沒變（全部重算的 10842、14258 個的四成左右），Preact 910 裡 608，Backbone 0。NewsSite 的只有一種：dropdown 的 `input` 換 `CHECKED`，2c 重算它後面的 `label`（子樹 7 個元素）和 `ul`（11 個），規則只用到 `ul` 自己（`:checked ~ ._dropdown-content`）和 `label` 底下的 `span`（`:checked ~ ._dropdown-label div span span:first-child`）。
   - **範圍**：兄弟 combinator 左邊的 compound 用到的 state、屬性（class、id、其他屬性）改了時，用 sibling invalidation set 找出之後的兄弟裡要重算的，連同它底下的後代部分（上面的 `span`：Blink 的 sibling descendant set）。元素自己的後代 invalidation set（`.a .b` 改了 `.a`）照第 13 條延後；插入和移除照 2c 的旗標；set 表達不了的（`:nth-child(… of :checked)` 這類）照 2c 從改變點之後全部重算。不選連元素自己的後代 set 一起做：同一套資料結構，但這幾個 suite 量不到效果；不選只做符合的兄弟本身：NewsSite 的 `label` 那半省不到。
   - **形式：照 feature 記**（Blink 的 `RuleFeatureSet`、`SiblingInvalidationSet`、`DescendantInvalidationSet`）：收集 tree scope 的規則時，兄弟 combinator 左邊的每個 feature 對到一個 set：要找的兄弟（它的 compound 的 class、id、tag、屬性；都沒有就是任何元素）、往後幾個（`+` 的長度，`~` 不限）、兄弟自己要不要重算、它底下要找的後代（最右邊的 compound 的 feature，沒有就是整棵子樹）。改變時只查 feature，不跑 selector matching、不要改變前的值，偏保守；收集和 2a 的 feature 共用，UA 那份只算一次。不選 Servo 的 `InvalidationMap` 加 snapshot（照改變前後實際比對 combinator 左邊的部分）：比較精確，但 dom 要留改變前的 state 和屬性，每次改變都跑 matching，程式量大很多。
   - **在重算的走訪裡套用**：parent 處理子節點的 state、屬性改變時算出 sibling set，推子節點時給每個兄弟帶上「自己重算」和「帶進子樹的後代 set」（`Visit` 多一個欄位），走到後代時比對 feature、符合才重算；parent 要先讀子節點的 `AttributeChanges`，dom 加一個不取走的讀法。不選 Blink 的 `StyleInvalidator`（改變當下把 set 掛在元素上，重算前另一趟轉成 dirty flag）：kun 的 dom 不知道規則（第 12 條：dom 記改了什麼，style 判斷），等於多走一趟。
   - **留在這份 RFC**：目標是量得到的 NewsSite，範圍不大。
   - 量測：`--against main`（NewsSite-Nuxt、NewsSite-Next、Preact）和計時區間裡結果沒變的重算數。
   - **做的時候補上的**（2026-10-17）：改屬性不再設 parent 的 `children_changed`，改設 `children_attributes_changed`（和 `children_state_changed` 對稱），否則每次屬性改變都走 2c 的路；`:link` 這類的 pseudo-class 記成 `href` 屬性、`:lang()` 記成 `lang`；`:empty` 也是一種 key，元素的子節點改了時由它自己把 set 套到之後的兄弟（parent 看不到孫節點的改變，走訪裡多一張等著的兄弟的表）。
16. **`SoftwareRenderer` 的 1:1 tile 和純色走快速路徑**（2026-10-17，第 2 段的 2d 之後照階段量過、開始查等 Viz 的時間時）：Speedometer 的量測用 `--automation-pipe`，沒有視窗，Viz 用 `SoftwareRenderer`，它每個 frame 整張重畫、每個像素做浮點的 bilinear 取樣，1200×900 的畫面一個 frame 30–100 ms（NewsSite-Nuxt 約 95 ms：77 個 quad，重畫面積是畫面的 4 倍，texture quad 全是 1:1、整數位置的 tile）。暫時讓它不畫（沒有 commit），NewsSite-Nuxt 的中位數 374.8 → 226.6 ms、Preact 98.2 → 73.6，Backbone、Svelte 不變；有視窗時用 wgpu，不受影響，headless、WPT、reftest、情境用的也是它。
   - **選了**：整數位置、1:1 的 texture quad 逐列取 texel（bilinear 在那裡的權重是 0 和 1，取到的就是 texel，輸出一個 bit 都不變），純色 quad 逐列填；不透明的直接複製，其他照原本的整數混色。Chromium 的 `viz::SoftwareRenderer` 用 Skia 畫，1:1 時走 Skia 的 blitter，是同一類的捷徑。
   - **不選（先不做）**：去掉被不透明的 quad 整個蓋住的（Chromium 的 `Display::RemoveOverdrawQuads`）：重畫面積從 4 倍降到約 1.5 倍，做完這條再量、還明顯時再做；只重畫有變的區域（damage rect 和 scissor）：aggregator 要算跨 surface 的 damage、截圖要整張，改動大；在 `viz` 用 Skia 合成：多一個依賴、整條重寫。
17. **第 5 段的順序和 5a 的做法**（2026-10-17，第 16 條之後）：20 個 suite 的取樣（cycle）裡，建 box tree 和 shaping（D7）佔 10.2%、cascade（D4）9.0%、`update_form_states`（B3）5.2%（jQuery 18.4%）、每次 script 之後的走訪（A4）2.4%（NewsSite 8–9%）、pre-paint（A3 在裡面）1.8%。jQuery 的 `update_form_states` 裡算 state 只佔約 5%，13% 是替每個文字輸入框建 UA shadow tree，其中將近一半（jQuery 的 6%）是每次重新 parse 同樣的 `style` 字串。
   - **順序**：5a 是 A4 和 UA shadow tree 的固定 style（小）；5b 是 B3（表單的 state 在比對時才算，和 UA shadow tree 剩下的成本：比值最差的 jQuery）；5c 是 D4；5d 是 D7。A3、D6 延後（A3 的比重小；D6 只影響 TipTap，這次它無效）。不選先做 D4、D7：比重大，但各自是中到大的改動。
   - **`<base>`：base URL 存著，`<base>` 插入、移除、`href` 改了才重新找**（Chromium 的 `Document::ProcessBaseElement`）。不選每次照舊走、走到 `<body>` 就停：不合規格。
   - **`<link>`：插入、移除、屬性改了時記下那個元素，`FrameView` 取走**（Chromium 的 `HTMLLinkElement::InsertedInto`、`ParseAttribute` 呼叫 `Process`；kun 的 `<img>`、`<script>` 已經是這樣：`take_image_fetches`、`take_prepared_scripts`）。不選文件的版本號加整份的走訪：只是走得少，每一步都動到 `<link>` 的頁面還是每次走整份，不合第 1 條（工作量跟著改變的量走）。
   - **背景圖片：重算時把新算出來、會畫出來（不在 `display: none` 裡）的元素的背景圖片的網址交給 `FrameView`**（Chromium 的 `ElementStyleResources::LoadPendingResources`：算 style 時載入 computed style 裡還沒載入的圖片）。不選重算過背景就設旗標、`FrameView` 再走整份：理由同上。
   - **UA shadow tree 的固定 style 搬進 UA stylesheet，以內部的 pseudo id 比對**（Chromium 的 UA shadow tree 的部件用 UA stylesheet 的規則，inner editor 的 style 在程式裡建，都不 parse 字串）；placeholder 的 `!important` 照舊是 `style` 屬性（Chromium 的 `UpdatePlaceholderVisibility` 也是）。從 inline 的優先順序變成 UA 的，作者選不到內部的 pseudo id，看不出差別。不選有上限的 parse 快取：Chromium 沒有，只是把重複的 parse 藏起來。
   - 第一次的建議（`<link>`、背景圖片用版本號或旗標加走訪，style 用 parse 快取）只看了改動的大小，沒有照 Chromium 的分層和第 1 條，使用者指出之後改成上面的。
   - **做的時候改的**（2026-10-17）：`<link>` 不經過 `dom_api`（`<img>`、`<script>` 那樣），改成 `dom` 記下、renderer 直接取走（和 `<title>` 的 `take_title_changes` 一樣）：沒有 script 的引擎的 document 沒有 window，`dom_api` 也找不到它的 frame。只記插入和屬性改變：移除不用載入什麼。
18. **5b：表單的 pseudo-class 在比對時才算（B3），UA shadow tree 延後到 style 更新前才建**（2026-10-17，5a 之後）：取樣（cycle）裡 `update_form_states` 佔 jQuery 13.7%（建 UA shadow tree 7.6%、算 state 約 6%）、Backbone 7.1%、ES5 5.6%、ES6-Webpack 5.2%。jQuery 的 TodoMVC 每次改動都用 `$('#todo-list').html(...)` 換掉整個清單，加 100 個項目建了約 5000 個 `<input>`，留下的只有 100 個，kun 在每次呼叫結束時替它們算 state、建 UA shadow tree。
   - **state：比對時問元素**（Chromium 的 `SelectorChecker` 問 `IsChecked()`、`IsDisabledFormControl()`，可能改變時對受影響的元素 `PseudoStateChanged`）：style 定義問表單 state 的介面，renderer 把 `dom_api` 的實作交給 `StyleEngine`，`dom_api` 自己的 selector 比對也用它；`dom_api` 在可能改變時只標記受影響的元素（設 `state_changed`，沿用 `Affected` 找它們），不算值、不存；走樹才知道的（`:disabled` 的 fieldset、`:default` 的 form 的第一個 submit、radio 的 `:indeterminate`）照 Chromium 存快取，改變時清掉。不選延後到 style 更新前一次算好存著：存的值在修改和 style 更新之間是舊的，每個讀它的地方（`querySelector`、`matches()`、`closest()`、CSSOM、之後新增的）都要先更新，漏一處就讀到錯的值；而且每個插入的表單元素都要記、都要算全部的 state。不選照舊、只把它改快：被丟掉的元素照樣算。
   - **UA shadow tree：延後到 style 更新前才建、才同步**（WebKit 的 `<input>` 的 UA shadow tree：插入時排定，下一次 style flush 才建），只替那時還在文件裡的控制項做。Chromium 的時機沒有查到可靠的出處（記得的「建 layout tree 時才建」的旗標查不到）。只替會畫出來的控制項建（不替 `display: none` 的建）先不做：要在 style 之後改 DOM、再補算 style，SerenityOS 因此改回插入時建；延後之後再量。
   - **規模**：比 B3 原本估的「中」大，分成幾小段。
19. **5b2 的做法**（2026-10-17，5b1 之後）：
   - **介面**：style 和 layout 透過介面問元素的表單 state，renderer 把 `dom_api` 的實作交給 `StyleEngine` 和 layout，`dom_api` 自己的 `querySelector`、`matches()` 也用它（Chromium 的 `SelectorChecker` 直接呼叫元素的 `IsChecked()`、`IsDisabledFormControl()`、`MatchesDefaultPseudoClass()`：元素的類別和 style 在同一個模組）。不選把邏輯搬進 `dom`：`dom` 是資料和樹，規範的演算法在 `dom_api`。
   - **快取**：要看元素以外的三項照 Chromium 快取，存在 `dom`，相關的東西改變時清掉：控制項的祖先 fieldset 的 disabled（Chromium 的 `ancestor_disabled_state_`）、form 的預設按鈕（`HTMLFormElement::default_button_`）、radio 群組有沒有選中的（`RadioButtonGroupScope`）。不選只快取後兩項（和 Chromium 不同）、都不快取（預設按鈕和 radio 群組每次掃整個 form）。
   - **失效**：`dom_api` 照現在的 `Affected` 找出受影響的元素，標記它們的 `state_changed`（Chromium 的 `PseudoStateChanged`），不比對新舊值，標記全部的表單 state；新插入的元素不用標記。不選照每種改變只標記可能變的 state：分支多，寫錯就漏掉重算；style 只在有規則用到那個 state 時才重算，多標記的成本小。
   - **照舊存著的**：清單的 `:open`、`:-internal-multi-select-focus`（`dom_api` 在 UI 動作發生時設，不是從屬性算出來的），focus、hover。
   - **一段做完**（使用者決定），不分成介面和快取兩段。
20. **5c：cascade 的浪費和 `MatchedPropertiesCache`（D4）**（2026-10-17，5b 之後）：20 個 suite 的取樣（cycle，2d 之後的那份）裡 cascade 佔 9.2%（NewsSite 21–23%，Vue、Svelte、Backbone、observable-plot 17–19%）；其中 `cascade_blocks` 約 51%（`apply` 24%、`cascade_start` 14%）、selector 比對 14%、`var()` 的解析約 7%、transition 的 `style_change` 約 6%。`apply` 的約 67% 是 CSS-wide keyword（`inherit`、`initial`、`unset`）每次建兩份整個 initial style、`copy` 再 clone 整個 style 和它們的 drop；`inherit` 的 76% 是 `ComputedStyle::initial()` 每次配置的清單。暫時加的計數（沒有 commit）：跨 recalc 的快取裡，宣告區塊和 parent 的繼承值都相同的 cascade，NewsSite 94%、Svelte 和 React 的 Complex-DOM 97%、Backbone 91%、Vue 89%、observable-plot 59%、Stockcharts 25%；只比宣告區塊（partial hit 的上限）只多 1% 以內。
   - **順序**：5c1 先修浪費：CSS-wide keyword 逐個 property 從 parent 或共用的 initial style 取值（Blink 的 `StyleBuilder` 的 `ApplyInherit`、`ApplyInitial`），initial style 只建一次（`ComputedStyle::GetInitialStyleSingleton`）；transition 的 `style_change` 對 `transition: all` 每個 longhand 把時間轉成字串再 parse、線性找重複的，一起修。5c2 做 `MatchedPropertiesCache`。`var()` 的快取不另外做：命中時整個跳過，沒命中的部分不值得一份快取。不選只做快取：沒命中的（Stockcharts 的七成五）照樣浪費。
   - **存什麼、怎麼命中**：照 Blink 存調整（blockification、`z-index`、theme）之前的 style，命中之後照樣調整；只做 full hit（parent 是同一個，或繼承的值相同）。key 是區塊的身分、`!important`、層級，presentational hints 比內容，加上 root 的 font size、viewport、system colors。不快取：有 keyframe 或動畫的 font size 的、`inherit` 了非繼承 property 的（看 parent 的非繼承值）、有 `style` 屬性的（每個元素一份，各 suite 0.1–4%）。命中時 clone（kun 的 `ComputedStyle` 是一整個 struct，Blink 的分成共用的群組、複製便宜）。不選 partial hit（Blink 有；這裡多的命中在 1% 以內，要把 `apply` 分成只套繼承的）。不選共用 `Arc` 不 clone（Stylo）：動畫的處理和之後改 style 的地方都要改成 copy-on-write，等 clone 在取樣裡明顯再做。
   - **範圍和失效**：每個 `StyleEngine` 一份，跨 recalc（Blink 是每個 document 的 `StyleResolver`）；條目持有樣式表的 `Arc`（區塊的位址不會被重用），樣式表改變時整個清掉，條數到上限也清掉。不選每次 recalc 一份：只有同一次 recalc 裡相同的才命中。
   - **測試**：測試用的模式在命中時也重新 cascade、比對兩者相同，跑 style 的測試和 reftest。
   - **做的時候改的**（2026-10-17，5c2）：第一版每個 hash 一串條目、hints 不進 hash、base URL 逐個區塊比字串，Stockcharts 的指令數 +220%（屬性不同的 SVG 元素全在同一串，每次比對路徑資料）、NewsSite 比只做 5c1 時還慢。改成：照 Blink 每個 hash 一條、後來的換掉前面的；hints 用元素的屬性（`id`、`class`、`style` 以外）算摘要進 hash；base URL 比位置（document 的 base 在 engine 裡同一格，改了時清掉快取；sheet 自己的由規則的 contents 決定）；hash 用 `rustc_hash`（SipHash 太慢）；parent 是共用的 `Arc` 時存它、先比位置。另外：同一個 hash 第一次沒找到只記下，第二次才存（Blink 沒有；只出現一次的 cascade 不 clone，Stockcharts 從持平變成 −4.3%，重複的元素晚一次命中，NewsSite 少約 0.7 個百分點）。shadow tree 的 sheet 變了不清：新的 shadow root 一直來，清了命中率就沒了；條目拿著規則，不清也不會錯。initial style 用 `thread_local!`（`LazyLock` 是整個 process 共用的狀態；Blink 的 renderer 一個 process 一個 main thread）。

21. **5d：CSS counters 的範圍、shaping 的快取（D7）**（2026-10-17，5c 之後）：20 個 suite 的取樣（cycle，5c 之後的 `main`）裡 box tree 佔 11.0%、parley 9.7%（TipTap 63%、Preact 16%、WebComponents 14%、NewsSite 12–13%、CodeMirror 13%）；parley 的約 70% 是 `InlineContent::new` 的 shaping（新建的 box 29%、增量更新 20%、pseudo-element 9%），斷行 17%。`build_into_layout` 裡 harfrust 的 shaping 38%、選字型約 30%（其中約 8% 是 fontconfig 的 `FcFontSort`，冷啟動的：見「順序」）、斷行和 bidi 的分析 10%、複製 10%。暫時加的計數（沒有 commit）：TipTap 每次執行 shape 488 KB 的文字，多半是插入的內容，Highlight 一步把同樣的文字切成上色的 span、整段再 shape；NewsSite 的重新收集多半來自 pseudo-element（730 次）和 block 的 Layout 差異（670 次），但文字少，增量更新只佔它的 0.5%。
   - **順序**（修正，2026-10-17，開始做之前）：5d1 讓 CSS counters 只在和它有關的東西改了時才重算；5d2 做 shaping 的快取。原本的 5d1（fontique 記下找不到的 fallback）拿掉：查證之後 fontique 找不到的 family 和 fallback 都有記下（fallback 的查詢一次執行只有 12 次，都存了），那些 `FcFontSort` 是每個 process 第一次用到一個 family 時的 `family_uncached`，冷啟動的成本，約佔全部的 0.5%（每個 suite 是新的 kun process）。重新評估時找到 counters：只要有內容要重新收集、文件裡又有任何 list item（`<li>` 就是），`update_counters` 就整份文件算一次，佔全部的約 2.1%（NewsSite 10.3–10.7%、Complex-DOM 的 TodoMVC 2.5–3.9%）；NewsSite 一次執行 156 次裡只有 4 次有文字改變或有元素被移除，151 次只是重新收集的容器底下有 list item。
   - **counters 什麼時候重算**（第一版，2026-10-17）：照 Blink 的 `StyleEngine::MarkCountersDirty`，box tree 的走訪記下要重算的時候（重建的元素的子樹裡有和 counter 有關的、counter 相關的值改了、上次的參與者沒有 box 了）。Complex-DOM 的 TodoMVC 的 counters 34 → 21（指令數的樣本），NewsSite 沒有效果：它的選單 `<ul>` 每一步在 `display: none` 和 `flex` 之間切換，清單整個進出，照這個規則就要重算；暫時加的分類（沒有 commit）顯示 NewsSite、Svelte 的參與者只有 UA 給了 `counter-reset: list-item` 的 `<ul>` 和裡面的 `<li>`。
   - **改成照 Blink 把清單的編號拆出來**（2026-10-17，使用者決定；不選讓 pass 變快：NewsSite 估計只少 4–5%；不選「整份文件只有清單時才走清單模式」：文件裡有一個作者寫的 counter 就整個退回）。查過的 Blink（`main`）：`ListItemOrdinal` 以 flat tree 上最近的 `<ol>`、`<ul>`、`<menu>` 為清單，項目插入、移除、改 style 時只標記它和之後的項目，要顯示時才算、存著（`CSSListCounterAccountingEnabled` 時也算清單的 `counter-reset: list-item`、項目的 `counter-set`、中間元素的 `counter-increment`）；整份文件的 `UpdateCounters` 只在明寫的 counter 指令來去或改變（`Element::SetComputedStyle`、`StyleEngine::NodeWillBeRemoved`）、`contain: style` 改變時跑；Blink 的 UA stylesheet 沒有 `ol, ul, menu { counter-reset: list-item }`（留著 TODO），所以清單進出不會走整份文件。kun 照 css-lists 3 有這條，`list-item` 是一般的 counter。
     - **結構**：清單的編號永遠以清單為單位，不管文件裡有沒有別的 counter；一般的 pass 只算 `list-item` 以外的 counter，只在它們的指令或顯示來去、改變時才走整份文件。
     - **範圍**：照 Blink 只標記改變的項目和它之後的，marker 要顯示時才算、存著（不選重算受影響的整個清單：每次都走過所有項目，而 NewsSite 的 15392 個 `<li>` 裡顯示 marker 的只有 0–18 個）。
     - **和 Blink 不同的地方**：Blink 的「清單」和規範不同的只有清單外的 list item（Blink 以 parent 為清單）；清單裡規範和「最近的清單」一樣：清單裡每個元素的 counter 集合都有清單的（或清單裡的清單的），前面的兄弟的同名 counter 不會取代它（css-lists 3 的「Inheriting Counters」）。kun 遇到清單外的 list item、別的元素或 pseudo-element 改變或顯示 `list-item` 時，整份文件的 pass 連 `list-item` 一起算，照規範（之後都這樣，直到文件換掉）。
     - **正確性**：debug build 每次建完、更新完也跑一次整份文件的 pass，比對每個 marker 和 `counter()` 的文字。
     - **做的時候改的**（2026-10-18，5d1）：「只標記改變的項目和它之後的」改成清單整個忘掉、要顯示時一次走過整個清單（存每個項目的值）：kun 不像 Blink 從 DOM 往前找上一個項目，要找出改變的位置也得走清單，走過的成本一樣；只算 marker 顯示數字的清單（`list-style-type: none` 或字串的不算）是效益的主要來源。一開始以為「清單裡的清單的 counter 漏給它之後的兄弟」也要退回，查證 kun 的 inheriting counters 之後不會（上一條），拿掉。不規則的用法在需要時才發現：編號時遇到（清單外的 list item、清單裡別的元素改變 `list-item`），一般的 pass 遇到（`counter(list-item)`），或 pseudo-element 的 style 改了；元素自己的改變由 `restyle` 比對它在 `list-item` 裡的角色。marker 的 `content` 是字串時直接用，不經過 pass（WPT 的 `css/css-lists/marker-dynamic-content-change.html` 抓到）。
     - **5d2 做的時候改的**（2026-10-18）：命中時除了複製 `ShapedText`，每個字元和 shaped cluster 的 style 編號換成這次的（`parley_engine` 的 `ShapedText::set_style_indices`：只差顏色的文字，style 的編號也不同）；key 裡的 font stack、features、variations 用解析之後的值（`Resolved` 是每次 build 的表的索引），也比對分析的結果（script、斷行、bidi 的層級）；上限 2^18 個字元，滿了整個清掉（TipTap 一個 iteration 約 47 萬個字元，跨 iteration 的命中因此拿不到，這是重複造成的效益，不追）；debug build 每次命中也再 shape 一次比對。
   - **shaping 快取的效益從哪裡來**（暫時加的計數，沒有 commit；byte 加權）：同一個 iteration 裡（頁面上重複的文字：側欄、選單）Preact、Svelte 76%、Lit 59%、WebComponents 48%、Vue 34%、NewsSite 28–31%、CodeMirror 8%、TipTap 和 React-Redux 0%；3 個 iteration 時 60–91%，TipTap 的 67% 全是 Speedometer 重複放進同一份文件。以 shaping item 為單位、或不在 inline 元素的邊緣切開，命中率幾乎一樣（TipTap 的 Highlight 改的是粗體、斜體這些影響 shaping 的 style）。
   - **shaping 快取的粒度**：parley 的 `shape_text` 以整個 inline formatting context 為單位，key 是文字、每段影響 shaping 的 style（font stack、粗細、斜體、寬度、大小、features、variations、locale、字距、詞距；不含顏色）和 inline box 的位置，命中時複製上次的 `ShapedText`（跳過選字型和 shaping，斷行的分析照做）；存在每個 renderer thread 的 `LayoutContext`，跨文件，有大小上限。和 Chromium 不同：LayoutNG 只在同一個節點重新收集時沿用前一次的 `ShapeResult`（`ReusingTextShaper`），跨節點的只有 `Font` 的字詞快取（`ShapeCache`，canvas 和舊的 layout 用）。不選以 shaping item 為單位（命中率較高，但要改 `parley_engine` 的 shaper 寫入的方式，fork 的改動大很多）、只沿用同一個節點的（新元素都沒幫助：TodoMVC、TipTap 的 Long、NewsSite 的主要成本）、字詞層級（跨空白的 kerning 和 ligature 會不同，Blink 也只在字型允許時用）。

22. **第 6 段的順序和 6a（E2）的方向**（2026-10-18，第 5 段之後）：20 個 suite 的取樣（cycle，`main` 的 `43d186cb`，renderer 的 main thread）裡 pre-paint 佔 1.96%（Complex-DOM 的 TodoMVC 4.9–7.7%、NewsSite 6.3–7.3%）、paint 2.46%（Complex-DOM 8.8–14.0%）、hit-test 的區域 2.04%（Complex-DOM 5.5–14.0%，`covered_scrollers` 走整棵樹：它們的側欄是 composited scroller）、layerization 0.78%；三者每個 frame 都走過每個 box。暫時加的計數（沒有 commit）：paint 的快取大多命中（Svelte-Complex 9800 個 box 命中、473 個因為 property 的 id 位移而沒命中、1892 個其他沒命中），NewsSite-Nuxt 位移的較多（命中 209、位移 791）：pre-paint 每次從空的 property tree 建，id 照走訪的順序，前面多一個 node 後面的都變。
   - **順序**：照第 6 段原本的 E2、E3、E1、E4（Chromium 的分層一樣：增量的 `PrePaintTreeWalk`、`PaintController` 的 subsequence、`PaintChunk` 的 `HitTestData`、`PaintArtifactCompositor` 沒有結構改變時只更新重畫的 layer）。E2 是基礎：E3 的 subsequence 要用「property 狀態相同」判斷一段沒變，E1 沿用 chunk 也是。不選 E1 先做（它在 React-Complex 和 paint 一樣貴，從 chunk 算被蓋住的 scroller 能早拿到效益，但之後 E2、E3 要一起維護它）。
   - **6a（E2）**：照 Blink：property node 歸它的 box（以元素、pseudo-element 和種類為 key），在跨 frame 保留的 property tree 裡，id 穩定（拿掉的留空位再用）；pre-paint 只走有改變的子樹（fragment 換了、捲動位置改了、祖先的 property 狀態改了），沒變的沿用上次的 `FragmentProperties` 和 node（Blink 的 `ObjectPaintProperties`、`PaintPropertyTreeBuilder` 就地更新、`NeedsPaintPropertyUpdate` 和 `DescendantNeedsPaintPropertyUpdate`、`PrePaintTreeWalk` 跳過乾淨的子樹）。不選照舊整棵走、只讓 id 照擁有者穩定（修掉位移，pre-paint 自己的成本不變）。細節（2026-10-18，6a 開始前，使用者決定都照建議）：
     - **node 的身分**：以擁有者（元素和 pseudo-element；SVG 內部的以 SVG 元素）加上種類（box 的 transform、opacity、隱藏的 clip、overflow clip、捲動的 translation 和 scroll node、replaced 的內圓角 clip、fixed 的 transform、iframe 的 clip、原點和 viewport 的捲動）為 key；擁有者不見了 node 就釋放，空位給之後的用；「parent 在前」的不變式拿掉，`to_root` 這類改成不依賴順序（之後改回，見本條最後一項）。
     - **重走的範圍**：子 fragment 是同一個 `Arc`、輸入（位置、parent 的狀態、所在的 scroll node、fixed 的狀態）也一樣就整棵沿用（和 `PaintController` 判斷沒變的方式一樣；Blink 用 `NeedsPaintPropertyUpdate`，kun 的 fragment 身分帶著同樣的資訊）。
     - **捲動**：只改捲動 translation 的矩陣，不重走（Blink 只更新 `ScrollTranslation`）。
     - **composited scroller 不捲動的那次 pre-paint**：從同一份 tree 複製、把那些 scroller 的捲動 translation（和合成的 iframe 的 viewport 捲動）歸零，不再走第二次。
     - **`FragmentProperties` 的存放**：紀錄拿著 fragment 的 `Arc`（位址不會被重用），條目長到上次清理的兩倍時清掉只剩這裡拿著的。
     - **正確性**：debug build 每次增量的 pre-paint 之後再從頭做一次，逐個 fragment 比對解析之後的狀態（到 root 的矩陣、clip 鏈、effect 鏈、scroll node）。
     - **分段**：6a1 id 穩定和不走第二次；6a2 只重走改變的子樹、捲動就地更新。
     - **6a2 消失的 node 怎麼釋放**（2026-10-18，6a2 開始前，使用者決定）：整棵沿用的子樹不再走進去，6a1 的「這次沒給到的位置就釋放」不能用（scroll node 不能留著：`scroll_nodes()` 就是捲動容器的清單）。重走一個 fragment 時把它的新舊 children 對起來：同一個 `Arc` 的照沿用的規則，同一個擁有者（元素和 pseudo-element）的重走，對不上的舊子樹整棵拿掉；拿掉時紀錄直接刪掉，它的 node 列成候選，這次 pre-paint 結束時還沒被同一個 key 拿回去的才釋放。工作量和改變的部分成正比（Blink：`LayoutObject` 銷毀時它的 `ObjectPaintProperties` 和 `FragmentData` 一起釋放，`PrePaintTreeWalk` 跳過乾淨的子樹）。紀錄因此精確地刪除，上面「`FragmentProperties` 的存放」那一項不再需要長到兩倍才清理（紀錄照樣拿著 fragment 的 `Arc`，位址不會被重用）。不選每次照樣走過每個 box、沿用的只標記（不會漏，但每個 frame 仍對每個 box 查一次 hash）。debug build 另外檢查活著的 node 和紀錄剛好是從頭走會有的那些。
     - **改回 parent 在前**（2026-10-18，6a1 量過之後，使用者決定）：不依賴順序的 `to_root` 在 Vue 的後半段每次都走慢的路徑（暫時加的計數：1600 次呼叫裡 761 次，樹約 105 個 node；`to_root` 39 → 54 個指令數的樣本），因為一個 node 一直排在它的 parent 前面。改成每個 node 排在 parent 之後（cc 的 property tree 也這樣要求，照 id 的順序一趟更新；kun 的 compositor 端也是）：空位只給排在 parent 之後的 node；沿用的位置在 parent 之前（外面新多了一層）時換一個新的位置，那個 node 和它的子孫的 id 各變一次，那個 frame 的 paint 快取沒命中。`to_root`、`composited_scroller_of`、`scroll_nodes` 回到一趟走完。不選每次 pre-paint 結束時把 parent 在前的順序存起來（每次多一層 index；tree 的欄位是公開的，存的順序要和它一起維持），也不選維持現狀、等 6a2 再快取 `to_root`（Blink 的 `GeometryMapper` 的快取，可以之後另外做）。這樣一來新的 node 在排在後面的 parent 底下時用不到前面的空位：Vue 的清單沒有項目時隱藏，每個 iteration 它的 node 是新的、排在最後，100 個項目接在後面，樹每個 iteration 長 105（107 → 212 → 317，暫時加的計數）。所以空位比 node 多（至少 32 個）時，下一次 pre-paint 從頭編號（那個 frame 的 paint 快取不命中，和之前每個 frame 一樣），和 `FragmentProperties` 長到兩倍才清理同樣的做法。

23. **6b（E3）：paint 的 subsequence 快取**（2026-10-18，6a 做完之後，使用者決定都照建議）：6a 之後 paint 佔 Complex-DOM 的 TodoMVC 的 10–16%（cycle），大多是每個 box 的 hash 查詢、沿用 item 的複製（配置和搬資料約佔 paint 的 16%）和 SVG 的內容每次重畫。暫時加的計數（沒有 commit；每個 frame 約 2000 個 box）：整段沒變的 stacking context 裡的 box 佔 Complex-DOM 的 46–62%、NewsSite-Next 19%、Nuxt 4%、Vue 11%，加上 float、flex/grid item、atomic inline 這些整個畫完的單位只多約 1%。
   - **單位**：每一個 stacking context 和 positioned 的 box（每一次 `stacking_context()`）是一段，照 Blink 在每個 `PaintLayer` 用 `SubsequenceRecorder`。不加整個畫完的單位（多約 1%，和 Blink 不同）。
   - **判斷沒變**：根是同一個 fragment（子樹的 layout 沒變），pre-paint 從上次 paint 之後沒重走它（整棵的 property 一樣；看 pre-paint 的世代），底下沒有 child frame，裡面沒有待重畫的元素，canvas 背景的來源一樣。待重畫的元素原本定成「有就這次全部不用 subsequence」（以為只有圖片載入）；做的時候發現畫過的 canvas 每個 frame 也待重畫，所以改照 Blink 往上標（`PaintLayer::SetNeedsRepaint`）：從它上次所在的 stacking context 一路標到 root，只有這些重畫；找不到它的 box 時（inline 元素的背景圖）這次全部不用。整段沿用包括 SVG 的內容。Blink 用 `PaintLayer` 的 `NeedsRepaint` 旗標，kun 的 fragment 身分和 pre-paint 的紀錄帶著同樣的資訊。不選逐個 box 比對（照樣走過每個 box）。
   - **沿用的 item**：`compositor_paint` 的 `DrawGlyphs`、path 這類大的資料改成 `Arc` 共用，複製一個 item 只加 refcount（Blink 的 `PaintRecord`、`SkTextBlob` 也是 refcount 共用）；逐個 box 沿用的也受惠。不選照 Blink 把 item 從上一次的 artifact 搬走（上一次的 artifact 由 `PaintController` 和舊的 `PaintedLayer` 共用，要改成 paint 時能獨佔，牽動 `FrameView` 和 layer 的持有方式），也不選照舊複製。
   - **分段**：6b1 subsequence；6b2 共用大的資料。
   - **6b2 不共用 path**（2026-10-18，量過之後）：先把 `gfx::Path` 的點改成 `Arc` 共用（寫的時候才複製，像 `SkPath` 的 `SkPathRef`），canvas 建 path 時每加一個點都要經過 `Arc::make_mut`，Charts-chartjs 的指令數 +0.9%（`make_mut` 和 `arc_to` 裡 inline 的約佔 0.9%）；改成 `PaintOp` 的 `DrawPath`、`ClipPath` 拿 `Arc<Path>`，canvas 每記錄一個 path 要多配置一次，仍然 +1.0%。只共用 glyph、位置、旋轉和邊框的多邊形時，Complex-DOM 的效果一樣（−0.4% 到 −1.8%），Charts-chartjs +0.1%，所以 path 不共用（邊框的 path 和 SVG 的形狀在沿用時照舊複製）。

24. **E1：hit-test 的資料在 paint 時記下**（2026-10-18，6b 做完之後，使用者決定）：6b2 之後的取樣（指令數）裡，`main_thread_scroll_hit_test_region` 佔 Complex-DOM 的 TodoMVC 6.3–9.9%，比 paint 本身（3.7–7.2%）還多：它們有 composited scroller，每次 paint 都走一遍整棵樹找可能蓋住它的東西（4.1–6.3%），再對每個 composited scroller 逐一比對畫在它後面的每個目標（其餘的部分）。React-Complex 的 root 有 blocking 的 `wheel` listener，wheel 的 region 也每次走整棵樹（4.9%）。
   - **被蓋住的 composited scroller 在主執行緒照 chunk 算**（照建議）：paint 時把 hit test 找得到的東西記進 artifact 的 hit-test 資料，region 只看這份資料；每個 composited scroller 只和畫在它後面的 chunk 比，先用 chunk 的外框排除，外框重疊時才看裡面每個目標的矩形，結果和現在相同。compositor 和 protocol 不變。Chromium 的 non-composited scroller 由 Blink 從 chunk 的 `HitTestData` 收集（和這裡一樣），被蓋住的判斷則在 cc 滾輪來時用 layer 的 hit-test 範圍（`IsInitialScrollHitTestReliable`）；不選照 cc：要改 compositor 和 protocol，layer 的範圍比 box 粗，會有比較多滾輪等主執行緒。
   - **記什麼**（照建議）：hit test 找得到的每個目標（border box、文字、inline box、SVG 的形狀和字元、子 frame 的 canvas）一個矩形，連同它的 property 狀態和所在的 scroll container；同一個狀態和 scroll container 的連續目標是一個 chunk，帶著外框；scroll container 自己的 border box 是自己的 chunk（Blink 的 `ScrollHitTestDisplayItem`），「畫在它後面的」就是排在它後面的 chunk。沿用的 box、整段沿用的 subsequence 連同它們的目標一起複製。記在 artifact 的另一份清單，不放進 display item：raster 和 layerization 不必看它，沒有畫東西的 box 也不會多出空的 chunk。不選每個 chunk 只記外框：scroller 上下的兄弟在同一個 chunk 時外框跨過它，會誤判成被蓋住。
   - **wheel listener 的範圍做成 style 的 bit**（使用者選了照 Blink、只重畫受影響的子樹；不選 listener 一改那次 paint 就整個不沿用）：Blink 的 `InsideBlockingWheelEventHandler` 是 `LayoutObject` 的 bit，listener 改變時 `MarkBlockingWheelEventHandlerChanged` 標記它和祖先，`PrePaintTreeWalk` 沿著 layout tree（flat tree 的順序）往下更新，值變了的物件重畫 hit-test 的資料。kun 的 pre-paint 走的是 fragment tree（照 containing block，absolute positioned 的 box 不在它的 DOM parent 底下），照 flat tree 繼承的地方是 style：computed style 帶一個 bit（parent 的，或元素自己有 blocking 的 listener；shadow root 底下的第一層也看 shadow root 的；root element 也看 document 和 window 的），listener 的 blocking 與否改變時只標記那個元素重算 style，bit 和繼承的值一樣往下傳；它只影響 paint（layout 的 `StyleDifference::Paint`）。paint 記目標時順便記這個 bit，wheel 的 region 也從同一份資料算。結果和 Blink 一樣：只有範圍改變的子樹重畫。
   - **正確性**：debug build 每次 paint 之後也用原本的走訪收集目標，比對和 paint 記下的完全相同。
   - **分段**：E1a 記下目標、被蓋住的 scroller 從 chunk 算；E1b wheel 的 style bit 和 wheel 的 region。
   - **做的時候改的**（2026-10-18）：E1a 的外框直接用 `RectF::union`（空的矩形本來就被跳過，沒有位置的文字的 NaN 寬度在 `min`、`max` 裡被略過），不另外篩。E1b 的 bit 在 `Recascade` 裡加（`MatchedPropertiesCache` 之後，動畫重新 cascade 的也帶著）；SVG 的字元照 `<text>` 的 bit（字元沒有自己的 style），`<tspan>` 自己的 listener 不算。debug 的目標比對在情境裡發現 iframe 的 child frame 不見之後，它的行那一步照樣搬上一次 paint 的 item（舊的 child frame 的內容），一起修掉。

25. **E4：raster invalidation 以 chunk 為單位**（2026-10-18，E1 做完之後，使用者決定都照建議）：E1b 之後的取樣（指令數）裡，`PaintArtifactCompositor::Update` 佔 Complex-DOM 的 TodoMVC 2.9–5.1%，其中 layerize 只有 0.2–0.3%，raster invalidation 佔 2.4–4.3%：每次 paint，每個 layer 的每個 item 都換算 rect、用 SipHash 算序號（`PaintedLayer::new`），再把舊的 item 建成表逐個比對（`invalidation`），整段沿用的 stacking context 也一樣。
   - **做法**：照 Blink 的 `RasterInvalidator`，以 chunk 為單位對新舊：chunk 的 id 是它第一個 item 的元素和種類（同樣的第幾個）；新的 chunk 的 item 整段是從舊的同一個 chunk 照順序搬來的（`PaintArtifact::cached`）、狀態（矩陣、clip、opacity）也一樣時，整個沿用舊的紀錄，不換算、不比對；狀態不同的 chunk 新舊都整個重畫；其他的在對上的 chunk 裡逐個比 item（id 在 chunk 裡算序號）；對不上的 chunk 整個重畫，順序變了的也是。表改用 FxHash。和原本不同：item 換到另一個 chunk 時當成一個去、一個來，兩邊的 rect 都重畫（Blink 也是在 chunk 裡比 item），多 raster 一點，不會少。不選只換 FxHash、少配置（每次照樣走過每個 item，只省約 0.8%）。
   - **layerization 不跳過**：layerize 本身 0.2–0.3%；照 Blink 的 `NeedsUpdate` 在結構沒變時沿用上次的 pending layer，要比對 chunk 的狀態序列，多一份失效的正確性，效益太小，記在「之後的可能」。

26. **第 6 段之後的順序**（2026-10-18，使用者決定）：第 6 段之後照階段對照 Chrome（`progress.md` 裡第 6 段之後照階段對照 Chrome 的那一節：trace，19 個 suite，每個 iteration 的 ms）：kun 比 Chrome 多 script 699（40%）、style 540（31%，TipTap 的 shaping 和斷行 126）、parse 214（12%）、等 Viz 182（10%）、canvas 95（5%）；layout 多 34，paint 一樣，GC 少 11。原本的第 7 段（F1、B6）效益很低：GC 已經比 Chrome 少，B6 沒有可拿的；F1 最多拿到 canvas 的 95 ms（幾乎都在 Charts-chartjs），成本卻是最高的一級（process 的邊界、`getImageData` 的同步讀回）。照「效益 ÷ 成本」重排：
   - **第 7 段（小）**：7a bindings 取 `this` 的 wrapper：`templates::call_member` 用 `convert::object` 對 59 個型別的 tag 逐一試（每一個都 `try_from`、`is_api_wrapper`、`unwrap` 三次 FFI），Charts-chartjs 的取樣有約 18% 在這裡（canvas 的 context 排在後面），約 60 ms；Blink 的 `V8X::ToWrappable` 照型別直接取。7b `querySelector`：ES6-Webpack 的取樣有約 17% 在 `SelectorQuery::matches_in_scope` 和屬性比對，約 55 ms；Blink 有 `SelectorQueryCache` 和 id、class、tag 的捷徑。
   - **第 8 段（中）**：文字（TipTap 的 style 加 layout 202 對 46，取樣幾乎都在 parley 和 harfrust 的 shaping、斷行時 clone `LineState`、`InlineContent::fragments`、選字型），約 100–150 ms；C3（parse 多 214，jQuery 的取樣有 46% 在 parse；原本說 C1、C2 做完再量，量到 jQuery 的 parse 仍是 Chrome 的 2.1 倍），約 100–150 ms。
   - **之後量過再決定**：等 Viz（182 ms，只影響 headless 和 automation 的 `SoftwareRenderer`，視窗模式不會這樣等：做了 Speedometer 的數字變好，實際使用不變）；每個元素的 style 成本（D8，TipTap 以外的 style 約 410 ms，分散）；B4（jQuery、ES5、ES6 的 script 裡 `dom_api`、`dom`、cppgc、hash、配置約三成，要改 soundness 的論證）。F1 等 canvas 成為重點時再做；B6 不做（記在「之後的可能」）。
   - 不選照原本的第 7 段做：兩項合計約 5%，比新的第 7 段（約 115 ms、6–7%）少，成本高得多。
   - **7a 的做法**（2026-10-18，使用者決定照建議）：`dom_api` 加 `ObjectKind::tags`（每個 kind 是哪個 GC 型別，窮舉的 match，新的 kind 不寫就編不過；`NodeList` 是 live 的 collection 或 static 的兩種），建立 template 時替每個 interface 算好它接受的 kind 的型別，`call_member` 只用這些 tag 去取 `this`（多半一個；static operation 的是 window）。取不到時，debug build 再試每個型別，確認不是對照漏了。`dom` 加 `ApiWrapper`（確認過是 API wrapper 的物件，只能經過那個檢查做出來）和 `BindingContext::unwrap_wrapper`，一個值試幾個型別時只檢查一次（`convert::object` 也受惠）。Blink 的 `V8X::ToWrappable` 照 interface 的型別直接取。不選每個 member 記住上次的 tag（多一份狀態，第一次照樣全部試），也不選只把兩個檢查移出迴圈（canvas 仍要試幾十個型別）。
   - **7b 的做法**（2026-10-18，使用者決定照建議）：查過的取樣裡，ES6-Webpack 的 `querySelector` 有約 20% 在每個元素各建、各丟一份 `SelectorCaches` 和 `MatchingContext`（真正的比對約 5%，parse 幾乎是 0）。`SelectorQuery::with_matcher` 替一整次走訪（`querySelector`、`querySelectorAll`、`closest`）建一份 caches 和一個 context（Stylo 的 `query_selector` 也是一個 context；Blink 的 `SelectorQuery` 一個 `SelectorChecker`），每個元素只比對；走訪期間 DOM 不變，`:nth-child` 的快取對整次都對。不選加 id、class、tag、屬性的捷徑（多約 5%，要自己處理 quirks mode 和屬性名稱的大小寫），量過再看；不選 parse 的快取（Blink 的 `SelectorQueryCache`：要每個 document 一份狀態，取樣裡 parse 幾乎是 0）。

27. **第 8 段：先做 C3，文字之後**（2026-10-18，使用者說先不管做法、照建議做下去）：
   - **順序**：文字（TipTap 的 shaping 和斷行）查過之後放後面：暫時加的計數（沒有 commit）顯示每個段落只 shape 一次（一次執行約 49 萬個字元，正是兩步各 25 萬），斷行約 2.6 次（含 min/max content），轉成 fragment 的只有 4096 行；成本是每個字元的（harfrust 21%、parley 選字型 7%、分析 7%）和每一行的（`fragments` 約每行 8.5 µs），多半在 parley 的 fork，要再細查。C3 的目標明確（jQuery 的取樣有 46% 在 parse、ES5 33%、ES6-Webpack 25%），先做。
   - **C3 的做法**：照 Blink 的 `HTMLDocumentParserFastPath`：`html::parse_fragment` 在 context 是「in body」、沒有東西開著的 HTML 元素（`body`、`div`、`span`、`p`、`li`、`ul`、`ol`、`section` 這類）時，先整段掃過一遍：只認常見的元素（`div`、`span`、`p`、`li`、`ul`、`ol`、`a`、`b`、`i`、`em`、`strong`、`label`、`button`、`input`、`img`、`br`、標題這類）、小寫的名字、屬性（引號或不加引號、不重複）、文字和五個具名、數字的字元參照；樹建出來和 tree builder 一定一樣的才收：end tag 必須是目前元素的（不需要 implied end tags 或 adoption agency），會關掉開著的元素的 start tag（`p` 開著時的 block、`li` 裡的 `li`、標題裡的標題、`a` 裡的 `a`、`button` 裡的 `button`）、註解、`\r`、NUL 都放棄。掃完全部收了才建節點（和 sink 一樣：建元素、跑屬性的 hook、`parser_append_child`），放棄時什麼都還沒建，交給 html5ever。Blink 的 fast path 邊 parse 邊建，失敗時清掉重來；這裡先掃再建，放棄的成本只有掃描。
   - **正確性**：debug build 每次 fast path 收下時，也用 tokenizer 和 tree builder parse 一次，比對兩棵樹（節點種類、名字、屬性和順序、文字）。
   - **整份文件也走 fast path**（2026-10-18，小項目，使用者同意做）：C3 只讓 ES5 少 2.7%：它用 `DOMParser().parseFromString(html, "text/html")` parse 整份文件再搬 `body` 的子節點，不是 fragment parse。沒有 DOCTYPE 的整份文件，tree builder 設成 quirks mode、跳過開頭的空白、補上 `<html>`、空的 `<head>` 和 `<body>`，fast path 收得下的內容全進 `<body>`：`html::parse_document_with` 先試，照這個樣子建（屬性的 hook 拿到 document、行號連同跳過的行），debug build 一樣完整 parse 一次比對（連同 quirks mode）。
   - **文字不做，放進「之後的可能」**（2026-10-18，C3 做完之後，使用者決定）：照取樣，TipTap 的文字成本是轉成 fragment 17%（kun 的 `layout`）、harfrust 的 shaping 23%、斷行 11%（parley 每個斷行機會 clone `LineState`）、選字型 7%（每個 cluster 重新查）、shaping 的簿記 8.5%、icu 的分析 7%；kun 這一側和 parley 的兩個小改估計拿掉三到四成，約 40–60 ms，只在 TipTap 一個 suite，約剩下差距的 2–3%，還要動 fork。harfrust 本身要再快得在上游最佳化或換成 HarfBuzz，更大。

28. **收尾**（2026-10-18，第 8 段之後，使用者決定）：小項目做完就收尾，大項目都放進「之後的可能」。
   - **小項目**：整份文件也走 fast path（第 27 條，ES5 −17.7%）；`querySelector` 的 id、class、tag、屬性的捷徑量過不做：7b 之後 ES6-Webpack 的 `querySelector` 剩下的 20% 裡，走過幾千個元素 12.7%、比對屬性的值 4.4%，捷徑都還要做，能省的只有 selectors crate 的比對框架約 2.5%（約 5 ms）；Blink 的捷徑也只有 id、class、tag，屬性的照樣逐個比，而自己比要處理 quirks mode、HTML 屬性名稱和值的大小寫。
   - **放進「之後的可能」**：等 Viz（headless 的 `SoftwareRenderer`）、每個元素的 style 成本（D8）、B4，連同之前放進去的文字、F1、B6。原本的「待決定」：C3 做了（第 27 條）；F1 的方向和新載入的頁面第一次 rendering 落在計時區間裡的問題，一起放進「之後的可能」。

## 待決定

沒有：原本的三項，C3 做了（第 27 條），F1 的方向和新載入的頁面第一次 rendering 落在計時區間裡的問題放進「之後的可能」（第 28 條）。

## 之後的可能

- **等 Viz**（第 28 條）：headless 和 automation 的 `SoftwareRenderer` 每個 frame 整個畫面重畫、不拿掉被蓋住的 quad，第 6 段之後的 trace 裡 main thread 在計時區間裡等它多 182 ms（Complex-DOM 的 TodoMVC、NewsSite 各 25–30 ms）。只影響量測的模式（有 GPU 的視窗不這樣等），做了是 Speedometer 的數字變好、實際使用不變。
- **每個元素的 style 成本（D8）**（第 28 條）：`ComputedStyle` 的欄位分組和 copy-on-write；第 6 段之後 TipTap 以外的 style 還比 Chrome 多約 410 ms，分散在各 suite（取樣裡的 `ComputedStyle::clone`、cascade）。
- **後代的 invalidation set、D5、D6**：第 2 段延後的後代的 invalidation set（`.a .b` 在祖先上改了 class 時只重算符合的後代）；`display: none` 的子樹不先算 style；樣式表改變時沒改的 `<style>` 不重新 parse、只對新規則的 feature 失效。
- **新載入的頁面的第一次 rendering 落在計時區間裡**（原本的待決定第 3 項）：Preact 的第 2、3 個 iteration，新的 iframe 的整個 box tree（23、33 ms）建在第一步的計時區間裡，Chrome 的在區間外：載入之後 kun 沒有先畫一個 frame。
- **量測本身**：Perf-Dashboard 在 kun 上 rendering 的時間比 Chrome 少（`docs/limitations.md`，可能少畫了，沒查）；Editor-CodeMirror 的第 0 個 iteration 少兩個 `div`，比較時無效；完整的 `ci.py speedometer` 裡 Charts-chartjs 量到 310–323 ms，只跑 kun 時約 220 ms（`progress.md`）；同一台筆電上兩次完整的量測，合計差 10%、單一 suite 差到三成。
- **`querySelector` 的捷徑**（第 28 條量過不做）：id、class、tag、屬性的 selector 不經過 selectors crate 的比對框架，約省 ES6-Webpack 的 2.5%；或照 Blink 的 `TreeScope` 維護 id 表。
- **文字的 shaping 和斷行**（第 27 條不做）：TipTap 的 style 加 layout 是 Chrome 的四倍多；每個段落只 shape 一次，成本是每個字元和每一行的：kun 的 `InlineContent::fragments`（每行約 8.5 µs，每個 run 都配置）、parley 每個斷行機會 clone `LineState`、每個 cluster 重新選字型（照 Blink 的 `FontFallbackIterator` 先試上一個字型）；估計約 40–60 ms。harfrust 的 shaping 本身另外約 23%。
- **B6：節點的狀態放進 GC 的物件、拿掉 Rust finalizer**（第 26 條不做）：第 6 段之後的 trace 裡 kun 的 GC 比 Chrome 少（32 對 44 ms）。GC 再出現在 trace 上時再看。
- **F1：canvas 的 raster 移出 main thread**（第 26 條延後）：第 6 段之後最多拿到 95 ms（Charts-chartjs）；先比較 raster thread 和 Viz（OOP-R）。
- **layerization 在結構沒變時跳過**（第 25 條沒做）：Blink 的 `PaintArtifactCompositor::NeedsUpdate`，paint 的 chunk 的狀態序列和上次一樣時沿用 pending layer，只更新 bounds 和重畫的 layer。E1b 之後 layerize 只佔 Complex-DOM 的 0.2–0.3%。
- **fontconfig 的冷啟動**：fontique 找一個 family 的字型時用 `FcFontSort` 排整個系統的字型（每個 process 每個 family 一次，約佔全部取樣的 0.5%，每個 suite 是新的 kun process）；改成 `FcFontList` 只列出那個 family 的會便宜得多。要改 fork，和上游的行為要對得上（目前取排序後連續的同名字型）。
- Speedometer 以外的量測（MotionMark 這類 rendering 為主的、JetStream 這類 JS 為主的）。
- CI 上的工作量的回歸測試（數次數的測試已經在每天的 CI 裡，時間的不在）。
- **cppgc 的寫入也在 Rust 裡做**（B5 只做了讀取，第 5 條）：`Member::Assign` 是壓縮之後的 atomic store 加上 write barrier；我們的設定開著 slim write barrier（`cppgc_enable_slim_write_barrier`），快速路徑只讀一個全域的旗標（`WriteBarrier::IsEnabled()`：正在 marking 嗎），所以可以快速路徑在 Rust、慢速路徑（`CombinedWriteBarrierSlow`）留在 C++。B5 沒做：漏了 barrier 不會讓任何一次寫入看起來不對，只在 marking 進行中、時序剛好時讓物件被誤收（use-after-free），不能像讀取那樣每次和 C++ 比對，要靠 GC 的壓力測試（`test.py matrix`、`fuzz`）；而 2026-10-16 的取樣裡 `Member::Assign` 只佔 jQuery 的 0.2%（`Member` 的建構也是 0.2%），讀取的二十分之一。其他開銷降下來之後，取樣顯示寫入成了明顯的熱點再做，配上 GC 的壓力測試。`Persistent` 的建立和刪除不在這裡：它的 node 記著自己的位址，在 Rust 做要接管 persistent region 的 free list；第 6 條改成重複使用 `Persistent`。
- **完整的 B4：stack 上的裸指標**（第 6 條沒做）：bindings 和 DOM API 的內部不建 `Persistent`，只在真的要活過 JS 的呼叫時才 root，要放寬 `crates/dom/soundness.md` 的 I1、重寫它的論證、補 compile-fail 測試，另開 RFC。第 6 條估計三件做完之後 jQuery 剩約 3% 到 4%；實際量到前兩件做完時剩 10.2%（`dom_api` 5.2%、parser 的 sink 3.6%、`update_elements` 1.0%），第 4 段拿掉 parser 的之後約 6.6%，其他 suite 約 1%：重複使用的 `Persistent` 每次要 `set` 兩次，比估的貴（`progress.md` 的第 3 段）。第 4 段之後再量一次，決定要不要做；第 8 條改做不放寬 I1 的幾件小事。
