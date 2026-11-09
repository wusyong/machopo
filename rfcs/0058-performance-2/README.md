# RFC 0058：效能的第二輪：幾件小的優化，和 handle 的 root 表

- 開始日期：2026-10-18
- 狀態：✅ 已完成：2026-10-19（`NodeId` 的表用 FxHash；文字的 cluster 從上一次的位置往後找字元；選字型記住排第一個的字型，parley fork 改；表單狀態只算真的受影響的元素，debug build 照原本的做法比對；fontconfig、斷行的 `LineState`、`NodeIdMap` 延後登記量過之後不做；handle 的 root 表（A1）做了兩小段、量過之後先不做，程式碼收在 `git stash`；在 Linux 上做）
- 細節：`details.md`（一開始的量測、表單狀態的拆分、root 表的取樣、handle 的設計比較）
- 進度和驗收：`progress.md`

## 摘要

RFC 0051 之後，Speedometer 的 19 個 suite 裡 kun 每個 iteration 比 Chrome 多約 866 ms：script（JS 和它呼叫的 DOM）約一半，style 加上建 box tree 約一半，parse、paint、等 Viz 已經沒有差距（`details.md` 的「量測」）。這份 RFC 分三段：
- **第 1 段，純粹變快、不加新條件的**：以 `NodeId` 為 key 的表全部改用 FxHash（1a）；文字把 parley 的 byte 位置換回 UTF-16 位置時，從上一個 cluster 的位置往後找，不每次二分搜尋（1c）。
- **第 2 段，加上「可以跳過」的條件、每件配 debug build 的交叉檢查的**：選字型時，排第一個的字型完整涵蓋 cluster 就直接用它（2c，改 parley fork）；表單狀態和 UA shadow tree 只處理真的受影響的元素（2a）。
- **第 3 段，handle 的 root 從 cppgc 的 `Persistent` 換成 kun 自己的 root 表（A1）**：做了 `Member` 的寫入在 Rust 裡做（3a）和強的 root 表（3b），但 root 表在 marking 中寫入時當場標記物件，造成 floating garbage，TodoMVC-jQuery 反而慢約 9%，先不做（第 22 條），程式碼收在 `git stash`，修法放進「之後的可能」。
- 量過之後不做的：fontconfig 改用 `FcFontList`（1b）、斷行的 `LineState`（1d）、`NodeIdMap` 延後登記（2b）。

結果（2026-10-19，`ci.py speedometer`，和這份 RFC 開始前的 `main` 各量兩次取平均，Chrome 量一次；雜訊大，`progress.md` 的「效能的量測」）：18 個 suite（不含 Editor-TipTap 和 Perf-Dashboard）kun 合計 1867 → 1757 ms（−110 ms，−5.9%），kun / Chrome 的合計比值 2.22 → 2.09，各 suite 比值的幾何平均 2.20 → 2.08；Editor-TipTap 259 → 235 ms。

## 動機

2026-10-18 量的（`details.md`）：

- **script 的差距主要在 kun 的 DOM 那一側，不是 V8**：jQuery、ES6-Webpack、ES5 這幾個差距最大的 suite，取樣有一半以上在 `dom_api`、`dom`、`dom_js`、`bindings` 和 Rust 呼叫的 V8 API 裡；V8 執行 JS 本身為主的 React-Stockcharts，script 只比 Chrome 多約 30 ms。
- **DOM 那一側很分散**，但有幾組明顯不必要的工作：表單狀態的維護（`update_form_states` 等，jQuery、ES6 約 6%，RFC 0051 的 B3 之後還剩的部分）、每建一個節點就在 `NodeIdMap` 放一個 `WeakPersistent`（jQuery 1.8%）、`NodeId` 的 SipHash（1.7%）。
- **handle 和 root 本身**約佔 jQuery 的 6%、ES6 和 ES5 的 3–5%：每個 handle 一次 `Rc<RootCell>` 的配置，加上兩次改 `Persistent` 指向的 C++ 呼叫（從 spare 取出、放回），spare 用完時新建 `Persistent`。
- **文字的 shaping 和斷行比 RFC 0051 估的大**：Editor-TipTap 一個 suite 就比 Chrome 多約 190 ms，其中約 175 ms 是建 box tree 和 layout，取樣的 82% 在文字上（parley 的 shaping、harfrust、斷行時 clone `LineState`、每個 cluster 重新選字型、kun 自己的 `InlineContent::fragments`）。RFC 0051 第 27 條估計 40–60 ms 而不做。

目標：

- 拿掉上面那些不必要的工作，DOM 那一側和文字的時間變少，行為不變。
- handle 的建立、clone、丟掉不呼叫 C++（`NodeHandle` 等和事件分派的 `EventListenerHandle`），API 不變，`dom_api`、`dom_js`、`dom_wasm` 不用改。沒達到：第 3 段先不做（第 22 條）。
- 每個會加上「跳過」或「沿用」條件的改動，都有 debug build 的交叉檢查，並寫進 `docs/architecture.md` 的「快取和增量：加新東西時要跟著改的地方」，以後加新功能時漏改會被測試抓到。

不在範圍內：

- `ComputedStyle` 的分組共用（RFC 0051 的 D8）、後代的 invalidation set（D5、D6）、box tree 的重建：style 加 box tree 的另一半，另開 RFC。
- shaping 的結果按字詞重用（Blink 的 `ShapeCache` 的 word cache）：要先評估，見「之後的可能」。
- bindings 本身（`dom_js` 的轉換、wrapper 的建立，jQuery 約 14%）：A1 不處理，量過第 3 段之後再看。
- V8 的 build 設定（official build、PGO）。

要解決的 `docs/limitations.md` 裡的限制：無。

## 設計概要

對網頁和使用者：行為不變，只是變快。

對之後改這部分程式的人：

- `NodeHandle`、`DocumentHandle` 等照舊是 cppgc 的 `Persistent`（第 3 段先不做，第 22 條）；`NodeId` 照舊是每個節點一建立就登記（2b 不做，第 14 條）。
- 以 `NodeId` 為 key 的表用 `dom` 的 `NodeIdHashMap` / `NodeIdHashSet`（FxHash），新加的照用。
- 第 2 段加上的條件（剛插入、沒有要算的狀態的表單元素只記 UA shadow tree；沒建過 `<option>` 時不往上找 option；選字型記住排第一個的字型），在 debug build 裡每次都照原本的做法再算一次比對，不同就 panic；`docs/architecture.md` 的「快取和增量：加新東西時要跟著改的地方」寫明加新的表單控制項、新的字型選擇的輸入時要跟著改哪裡，`docs/testing.md` 的交叉檢查表列出它們。

## 詳細設計

### 第 1 段：純粹變快的（不加新的條件）

| # | 做法 | 依據（2026-10-18 的取樣） | 對照 Chromium |
| --- | --- | --- | --- |
| 1a | 以 `NodeId` 為 key 的表全部改用 FxHash（`rustc-hash`，RFC 0051 的 B1 已經引進）：`dom` 的 `NodeIdHashMap` / `NodeIdHashSet`，八個 crate 約 80 處（第 4 條；第 2 段又找到三個漏掉的） | jQuery 1.7% | Blink 沒有這些表 |
| 1b | ~~fontconfig 改用 `FcFontList`~~：不做（第 5 條），移到「之後的可能」 | 約佔全部取樣的 0.5% | Chromium 經過 Skia 的 `SkFontConfigInterfaceDirect::matchFamilyName`：先 `FcFontMatch`，不合格才 `FcFontSort`，不用 `FcFontList` |
| 1c | `TextSource::clusters` 把 byte 位置換回 UTF-16 位置時，記住上一次找到的位置，不每個 cluster 做兩次二分搜尋（第 6、8 條） | TipTap 10.6%（`InlineContent::fragments`，其中約六成是這兩次搜尋） | — |
| 1d | ~~parley 斷行時的 `LineState`~~：不做（第 9 條），移到「之後的可能」 | TipTap 7.3% | Blink 的 `LineBreaker` 不在每個字元記斷行機會，溢出時才照 `overflow-wrap` 重做 |

這一段的改動都只改記憶體的用法和查表的方式，結果和現在一樣，以後的功能不需要知道它們。

### 第 2 段：加上條件的，每件配 debug build 的交叉檢查

| # | 做法 | 依據 | 對照 Chromium | 以後怎樣會漏改，怎麼防 |
| --- | --- | --- | --- | --- |
| 2a | 查過每次呼叫在做什麼之後（`details.md` 的「表單狀態的拆分」）做六件（第 15、16 條）：`update_pending` 先跳過沒有、也不需要 UA shadow tree 的元素再建 handle；`update_form_states` 對剛插入、不是 option 或 select、也不依賴整棵樹的元素只記 UA shadow tree，而且只記有或需要 tree 的；被移除的這類元素不標記；isolate 建過 `<option>` 之前不往上找 option；attribute change steps 比對名稱用 `is_attr`。`update_elements` 自己的時間沒拆（第 17 條） | `update_form_states` 加 `Affected::element`、`form_states_children_changed` 約 6%；另外三個 3.2%、1.4%、2.1%（jQuery） | Blink 的 `:checked` 等在比對 selector 時問元素，狀態改了才 `PseudoStateChanged`；`HTMLFormElement` 只在 listed element 進出時更新 | 新的表單控制項或狀態不在「受影響」的判斷裡：debug build 每次從頭算一次比對 |
| 2b（量過之後不做，第 14 條） | `NodeIdMap` 改成節點被要求 id 時才登記（第一次 `NodeRef::id()` 這類），不在 `create_node` 一律放 `WeakPersistent` | jQuery 1.8% | Blink 的 `DOMNodeIds::IdForNode` 第一次被要求時才配 id | 新功能用 id 找節點，卻沒經過會登記的路：拿 id 只有一條路（型別），編不過 |
| 2c | 選字型（parley 的 `FontSelector::select_font`）的快速路徑：記住目前這組 family 和屬性底下排第一個有 charmap 的字型，它完整涵蓋這個 cluster 時直接用它，不重跑 query。query 照順序走、第一個完整涵蓋的字型就是答案，所以結果和完整的 query 一樣。不用「上一個 cluster 用的字型」：它可能是排在後面的 fallback，換回主要字型涵蓋的字元時會選錯。改 parley fork | TipTap 4.1% | Blink 的 `HarfBuzzShaper` 先用主要字型 shape 整段，只對沒有字形的部分照 `FontFallbackIterator` 往下試 | query 加了新的輸入（例如 `unicode-range`）卻沒讓記住的字型失效：debug build 用完整的 query 比對 |

2a 的兩個條件和 2c 都加進 `docs/architecture.md` 的「快取和增量：加新東西時要跟著改的地方」和 `docs/testing.md` 的交叉檢查表；2c 只算有 charmap 的字型這個條件測不到（測試字型都有），記在 `docs/test-gaps.md` 的「能補、還沒補的測試空缺」。

`querySelector` 的捷徑和 layerization 在結構沒變時跳過也是之前列過的小項：不做，見「已決定的事項」第 13 條。

### 第 3 段：handle 的 root 表（A1）：先不做

原本的設計（第 1、18–21 條）：把 `Handle<T>` 的 root 從 cppgc 的 `Persistent`（`crates/dom/src/handle.rs` 的 `RootCell`，每個 handle 一次 `Rc` 的配置、兩次 `Persistent::Assign`，留著重用的用完時再建一個）換成 kun 自己的 root 表，API 不變，分三小段：

| | 做什麼 | 結果 |
| --- | --- | --- |
| 3a | `Member` 的寫入在 Rust 裡做：relaxed atomic store 壓縮過的值，cppgc 的全域旗標（`WriteBarrier::write_barrier_enabled_`，fork 的 `binding.cc` 用 explicit instantiation 不檢查存取權的寫法交出位址）開著時才呼叫 `CombinedWriteBarrierSlow`，和 cppgc inline 的 `Member::operator=` 一樣（第 19 條） | 做完：`cppgc__Member__Assign` 的 1.06% 不見了，cycle 約 −2%；要發新的 V8 artifact |
| 3b | 強的 root 表：fork 的 `MemberSlot` 放在 Rust heap 上固定不動的 chunk（read scope 裡也能建 root，第 21 條），chunk 只往後接，由錨點的 GC 物件擁有、trace；marking 中寫入時當場標記物件（slot 在 cage 外，cppgc 的 barrier 略過） | 做完、測試和 mutation test 通過，但 jQuery 的 cycle 多約 9%：floating garbage（第 22 條，`details.md` 的「root 表的取樣」） |
| 3c | 弱的 slot 表取代 `NodeIdMap` 的 `WeakPersistent`（jQuery 2.19%） | 沒開始 |

3a、3b 的程式碼收在 kun 和 rusty_v8 fork 的 `git stash`（`progress.md` 的第 3 段），接著做的方向在「之後的可能」的 A1。

### 測試

- 第 1 段：行為不變，現有的測試照舊通過；1c 的 debug build 每次和二分搜尋比對，`layout` 的測試涵蓋跨好幾行和從右到左的文字。
- 第 2 段：每件的 debug build 交叉檢查本身就是測試（`cargo test`、reftest、情境和 WPT 都用 debug build 跑）；2c 加 `layout` 的 `font_selection::`（缺字換下一個字型、之後換回來，換 family、換屬性，各有 inline 和 `display: contents` 的版本），2a 的條件由 `dom_api` 和 `shell` 的表單測試守著。每件都跑了 mutation test。
- 第 3 段：做了的 3a、3b 的測試和 mutation test 記在 `progress.md`，和程式碼一起收在 stash。
- 量測不排進測試計畫（`docs/benchmarks.md` 的「效能的量測」）；完成時由使用者要求量了一次，結果在 `progress.md`。

### 分階段

1. **第 1 段**：1a、1c，彼此獨立（1b、1d 不做，第 5、9 條），不改 parley fork。
2. **第 2 段**：2c、2a（先查再改）；2b 量過之後不做（第 14 條）。2c 改 parley fork。
3. **第 3 段**：3a、3b 做完、量過之後先不做（第 22 條），3c 沒開始。

第 1、2 段各自審核、merge 回 `main`；第 3 段的程式碼沒有進 `main`。

## 缺點

- 第 2 段的每個條件，都是以後加功能時要記得跟著改的地方：多了交叉檢查和清單的維護。
- 第 2 段的 2c 改 parley fork（`shape/mod.rs`），約幾十行，fork 和上游的差異變多；每件照 fork 的規則加 `kun:` 註解、寫進 `README.machopo.md`、更新 `third_party.toml`，fork 用它自己的 toolchain 也要能編、上游的測試照樣通過。
- handle 的成本（jQuery 約 3%）和 `NodeIdMap` 的 `WeakPersistent`（2.19%）還在：第 3 段沒做。做的話，會多一份 kun 自己維護的 GC root 機制和論證（cppgc 的 `PersistentRegion` 有 V8 的測試，我們的表只有自己的），3a 還依賴 cppgc 的內部（private 的全域旗標和它的版面、slim write barrier）。

## 理由和其他做法

- **handle 的重新設計選 A1，不選 A2 或 B4**（第 1 條）：A1 的 API 不變；A2（範圍的 `HandleScope`）和 B4（不 root 的 `NodeRef`）每個 handle 更便宜，但 `dom_api`、`dom_js`、`dom_wasm`、`renderer` 有約 715 個帶 handle 的結構欄位、34 個 `async fn`，handle 存起來或跨過 `await` 是常態，兩者都要全面改型別；B4 還要放寬 `soundness.md` 的 I1。完整的比較在 `details.md`。
- **不直接用 V8 的 `HandleScope` 和 `Local`**：`Local` 只能指向 V8 heap 上的東西，DOM 節點是 cppgc 的物件；經過 JS wrapper 間接 root 的話，每個被 Rust 碰到的節點都要有 wrapper、每次存取都要 unwrap，而且 wasm 的文件沒有 V8（`dom_wasm` 用 standalone 的 heap），`dom_api` 刻意和 engine 無關。
- **不照 Blink 保守地掃描 stack**：裸指標一旦存進 Rust 的 heap（`Vec`、`Box`）就掃不到，型別擋不住，等於放棄 safe code 不會造成 UB。
- **小項放在最前面**：效益確定、改動小，不必等 root 表；而且第 2 段的交叉檢查要先有，之後的優化才有同一套防護。

## 先例

- cppgc 的 `PersistentRegion`：free list 的 node 表，`Persistent` 是指向 node 的指標；A1 等於把它搬到 Rust 裡讓它能 inline，結構相同。V8 的 global handles 也是 node block 加 free list。
- V8 的 `HandleScope`、`Local`（A2 的原型）；Blink 的 Oilpan 保守掃描 stack（B4 想達到的效果）。
- Blink 的 `DOMNodeIds`（2b）、`FontFallbackIterator`（2c）、`:checked` 等在比對時問元素（2a）。

## 已決定的事項

2026-10-18：

1. **handle 的 root 換成 kun 自己的 root 表（A1）**，API 不變。不選範圍的 `HandleScope`（A2）、不 root 的 `NodeRef`（B4）：兩者都要全面改型別，放進「之後的可能」；不用 V8 的 `HandleScope` 和 `Local`（指不到 cppgc 的物件，wasm 沒有 V8）；不保守地掃描 stack（型別擋不住）。
2. **之前列出的小而確定的優化排在最前面**：純粹變快的是第 1 段，加上條件的是第 2 段，A1 是第 3 段。
3. **加上條件的優化都配 debug build 的交叉檢查**，並寫進 `docs/architecture.md` 和 `docs/testing.md` 的清單，和 RFC 0051 的快取同一套做法。

2026-10-18，第 1 段開始前：

4. **1a 換掉所有以 `NodeId` 為 key 的 `std` 的 `HashMap`、`HashSet`**，不只熱的那幾個：`dom` 定義 `NodeIdHashMap<V>`、`NodeIdHashSet`（FxHash；`NodeIdMap` 已經是 id 表的名字），以後新加的照用。實際做的時候是 `dom`、`dom_api`、`html`、`style`、`layout`、`paint`、`compositor`、`renderer` 約 80 處（決定時以為只有約 20 個）；key 是 `(NodeId, PseudoId)` 的直接用 `FxHashMap`；`dom` 的測試裡的模型（`crates/dom/tests/common/mod.rs`）照舊。行為不變：SipHash 的走訪順序每個 process 都不同，現有的程式本來就不依賴它；`NodeId` 是 kun 自己發的流水號，網頁控制不了，沒有刻意製造碰撞的問題。
5. **1b 不做**，移到「之後的可能」：估計只省約 5 ms（19 個 suite 合計），卻要改 parley fork，還要保證取出的 face 完全一樣（`FcFontList` 列出任何一個 family 名稱相同的，現在只取第一個名稱相同的；`FcFontSort` 照接近程度排序，`FcFontList` 沒有順序；重複安裝的優先順序）。
6. **1c 先查再改**：取樣裡 `InlineContent::fragments` 自己（連同 inline 進來的 `text_fragment`）佔 TipTap 的 10.2%，配置只有 0.3%，複製的 style 是 `Arc`；先用 `perf annotate` 找到指令層級的熱點，再決定怎麼改。原本寫的「每一行、每個 run 都配置」不是主因。
7. **1d 先用 `clone_from` 覆寫上一個 `PrevBoundaryState`**，重用它已經配置的空間（`LineBoxMetrics::subtrees` 是 `SmallVec<[_; 2]>`，巢狀的行內元素一多就要配置 heap），fork 只改幾行、行為不變；用 `--sample --against` 比指令數，不夠再考慮只保存退回時需要的欄位。

2026-10-19，查過 1c、1d 之後：

8. **1c：`TextSource::clusters` 記住上一次找到的位置**。熱點不是配置，是每個 cluster 兩次二分搜尋（`char_at`、`char_before` 對文字節點的全部字元做 `partition_point`，把 parley 的 byte 位置換回 DOM 文字的 UTF-16 位置）：cluster 照順序走，這一個的結尾是下一個的開頭，所以從上一次的位置往後走幾步，順序倒過來（從右到左的文字）時才退回二分搜尋；結果和 `partition_point` 一樣，debug build 每次比對。第 6 條原本寫的「每一行、每個 run 都配置」不是原因。
9. **1d 不做**，移到「之後的可能」（取代第 7 條）：成本不在配置，在 `overflow-wrap: break-word`（TipTap 的 ProseMirror）讓 parley 在每個字元都記一次緊急斷行機會、clone 一整份 `LineState`，`clone_from` 省不到。要拿掉它得照 Blink 不記、溢出時重做，改的是 parley 斷行的核心，只換 TipTap 約 12 ms，風險不成比例；以後處理文字的 RFC 和 shaping 的字詞快取一起設計。

2026-10-19，第 2 段開始前：

10. **順序是 2c、2b 的計數、2a**：2c 的設計已經定了，又是 TipTap 最大的一項；2b 先量；2a 要先查，查完還有一輪決定，放在最後。
11. **2c 照「詳細設計」做**：記住目前這組 family、屬性和 fallback 底下排第一個有 charmap 的字型，它完整涵蓋 cluster 時直接用它；debug build 用完整的 query 比對。
12. **2b 先量再決定**：`NodeId` 本來就在節點上，`NodeIdMap` 只用來從 id 找回節點，而 style 和 layout 對每個要畫的節點都會拿 id（兩個 crate 約 120 處），「第一次拿 id 才登記」大概只省到從來沒畫過的節點；而且拿 id 是在 read scope 裡用 `&self`，登記要改成 interior mutability，每次拿 id 多一個判斷。先數 jQuery 裡建出來的節點有幾成從沒被拿過 id：大多數都被拿過的話，2b 不做，改由第 3 段（「待決定」第 1 項選 (a) 時）的弱 slot 表取代 `NodeIdMap` 的 `WeakPersistent`。
13. **`querySelector` 的捷徑和 layerization 在結構沒變時跳過不做**（原本的「待決定」第 3 項）：效益小（ES6-Webpack 約 2.5%、Complex-DOM 的 0.2–0.3%），卻各多一個以後加功能時要跟著改的條件。

2026-10-19，數過 2b 之後：

14. **2b 不做**（照第 12 條）：TodoMVC-jQuery 跑 10 個 iteration 建了約 310 萬個節點，99.8% 被拿過 id（暫時的計數器，數 `NodeRef::id()` 和 `NodeHandle::id()`；`dom` 自己直接讀欄位的還沒算進去），延後登記幾乎省不到。另外 `NodeHandle::id()` 不用 scope 就能呼叫，那裡拿不到 `NodeIdMap`，本來就沒辦法在那裡登記。`NodeIdMap` 的 `WeakPersistent` 留給第 3 段的弱 slot 表（「之後的可能」）。

2026-10-19，查過 2a 之後（原本的「待決定」第 3 項；查到的在 `details.md` 的「表單狀態的拆分」）：

15. **2a 做四件**：(1) `update_pending` 建 handle 之前先跳過沒有 UA shadow tree、也不需要的元素（和 `update` 原本提早返回的判斷相同，不加條件）；(2) `update_form_states` 對剛插入、不是 option 或 select、也不依賴整棵樹的元素，只記 UA shadow tree 的更新，debug build 照原本的做法再算一次比對；(3) `form_states_children_changed` 在 isolate 建過 `<option>` 之後才往上找 option，debug build 照樣找、確認沒有；(4) attribute change steps 比對名稱用 `is_attr`，不建 `QualName`。`update_elements` 自己的時間做完這四件再取樣看。

2026-10-19，2a 的四件取樣之後：

16. **再做兩件**：(1') `update_form_states` 記 UA shadow tree 的更新時就跳過沒有、也不需要 tree 的元素（那時元素剛讀過；等到 `update_pending` 才判斷，第一次讀它的 cache miss 抵掉了省下的），debug build 把跳過的另外記下，`update_pending` 確認它們沒有再被記、也仍然不需要 tree；(2') 被移除的、不是 option 或 select、也不依賴整棵樹的元素不標記、不更新 UA shadow tree：它已經不在任何文件裡，style 不會走到它，再插入時照剛插入的整個重算；`Plan::check` 確認除了這些元素以外，兩種做法一樣。(3) 留著：jQuery 載入時就建了 `<option>`，省不到，但 ES5 和 ES6-Webpack-Complex-DOM 沒有。

2026-10-19，第二輪取樣之後：

17. **第 2 段到這裡結束**：`update_elements` 自己的 2.61% 和 `reset_form_owner` 的 1.07%（jQuery）放進「之後的可能」。要拆開得先有看得到指令層級熱點的方法（`perf annotate` 在整個 suite 的取樣上跑不完），再決定改什麼；`update_form_states` 剩下的是每個 change 本來就要做的（第一次用 id 找節點、找 form owner），捷徑省不掉。

2026-10-19，第 3 段開始前（原本的「待決定」第 1、2 項）：

18. **root 表放在 GC 的 heap 上**：slot 是 `EdgeChunks<Edge>`，寫入是 `Member` 的寫入，marking 中新加的 root 由它的 barrier 標記，concurrent marking 時 marker 讀 slot 和讀任何 edge 一樣（fork 的 `MemberSlot`）。不選 Rust 的 heap 上的陣列：長大時會搬家，concurrent marking 時只能把 trace 延到 main thread，還要 fork 加 trace 裸指標和 barrier 的入口，論證都要新寫。不照 Blink 用 cppgc 的 `PersistentRegion`（atomic pause 時重新掃，不需要 barrier）：它的分配是 inline 的 C++，Rust 碰不到，自己加一個 region 要動 cppgc 的內部。
19. **barrier 的快速路徑讀 cppgc 的全域旗標**（`WriteBarrier::write_barrier_enabled_`），位址由 fork 的 `binding.cc` 交給 Rust；relaxed load，和 cppgc 自己 inline 的寫入一樣。incremental 和 concurrent 的 marking 開始時都會打開它，`CppHeap` 和 standalone 的 `cppgc::Heap` 都是（`marker.cc` 的 `EnterIncrementalMarkingIfNeeded`）；打開它的是 heap 自己的 thread，寫 slot 的也是，所以同一個 thread 的程式順序就看得到，背景的 marker 不寫 slot。別的 isolate 打開的只是多走一次慢速路徑，它再看物件所在的 heap。前提是寫 slot 的一律是 heap 的 thread（寫入都要 `&mut DomMutationScope`），寫進 `soundness.md`。不選每次呼叫 C++ 問 `IsEnabled()`（還是每次呼叫 C++）；不選 Rust 直接連 mangled 的符號（不用重編 V8，但 Itanium 和 MSVC 的名稱不同，比較脆弱）。
20. **第 3 段也做弱的 slot 表（3c）**，`NodeIdMap` 的 `WeakPersistent` 換過去：`EdgeChunks<WeakEdge>` 已經有了（`WeakSet`），而它是現在最大的單一項（jQuery 的 2.19%）。分三小段：3a `Member` 寫入的快速路徑、3b 強的 root 表、3c 弱的 slot 表。

2026-10-19，做完 3a、開始 3b 之前：

21. **修正第 18 條：slot 放在 Rust heap 上固定不動的 chunk**，不放在 GC 的 heap 上。root 常在 read scope 裡建立（`to_handle` 這類約 234 處），而 read scope 存在時 heap 不能 GC、不能執行 finalizer（`soundness.md` 的 I2）；表在 GC heap 上長大就要在那裡配置，違反它。cppgc 的 `NoGarbageCollectionScope` 擋不住：配置時照樣順手 sweep、執行 finalizer。改成 Rust heap 上的 chunk（`Box` 的固定陣列，用 `AtomicPtr` 串成只往後接的鏈，chunk 由錨點的 GC 物件擁有、它的 finalizer 釋放），長大是 malloc，哪裡都可以（現在的 `Persistent::new` 也是）；chunk 不搬動、只往後接，marker thread 走鏈時只看得到接好的一段（release / acquire），不需要延到 main thread 才 trace（第 18 條不選 Rust heap 的主要理由是 `Vec` 會搬家）。slot 不在 cppgc 的 cage 裡，`CombinedWriteBarrierSlow` 會略過它，所以寫入之後旗標開著時另外標記新的物件（fork 加一個 C 函式，和 `WeakObjectTable` 的 `MarkIfMarking` 一樣）。不選「表在 GC heap 上，只在沒有 scope 時長大，用完時退回 `Persistent`」：最熱的 scope（innerHTML 的 parse 在一個 scope 裡建上千個 root）最容易用完，而且兩種 root 並存。結構最接近 Blink 的 `PersistentRegion`（heap 外的 node block 加 free list），差別是用 barrier 代替 atomic pause 時重新掃描。

2026-10-19，3b 量過之後：

22. **第 3 段先不做**，3a、3b 的程式碼收進 kun 和 rusty_v8 fork 的 `git stash`，3c 沒開始。3b 照第 21 條做完、測試和 mutation test 都通過，但 TodoMVC-jQuery 取樣兩次都是 cycle 多約 9%、IPC 從 2.56 掉到 2.22，`recalc_style` 從 11% 變成 21%，GC 的樣本也幾乎加倍（指令反而少 4%）。原因是 floating garbage：marking 中每寫入一個 root 就當場標記它的物件（`MarkValue`），jQuery 每次 DOM 呼叫都建一個短命的 handle，所以 marking 期間碰過的物件全部多活一輪，heap 變大、cache 變差；`Persistent` 只算 marking 開始和最後的 pause 時還活著的 root。暫時拿掉 `MarkValue`（不安全，只為了量）時 cycle 比 3a 少 4.5%，確認是它。能修的做法是寫入時只 store、表已經 trace 過時對錨點做一次 Steele barrier 讓它之後再 trace（和 `WeakObjectTable` 的 `Enter` 一樣），要再加一套 mutator 和 marker 之間的 SeqCst 協定和論證；3a 單獨約 1–2%，卻要發新的 V8 artifact。兩者都放進「之後的可能」。

## 待決定

沒有。

## 之後的可能

- **A1：handle 的 root 表**（第 3 段，第 22 條先不做）：3b 的結構照第 21 條（Rust heap 上固定不動的 chunk，錨點擁有、trace），但 marking 中寫入 slot 不能當場標記物件：改成只 store，表已經 trace 過、還沒排重 trace 時對錨點做一次 Steele barrier（`pending` 旗標，mutator 先 store slot 再 `swap(true)`，錨點的 `trace` 先 `swap(false)` 再讀 slot，都是 SeqCst），最後的 pause 前表一定再 trace 一次。拿掉 `MarkValue` 時 jQuery 的 cycle 比 3a 少 4.5%，是要達到的目標。3a（`Member` 的寫入在 Rust 裡做，cycle 約 −2%）可以單獨先做，但要發新的 V8 artifact。stash 裡的程式碼是起點。
- **A2：範圍的 `HandleScope`**：只在一個函式裡用的暫時 handle，綁在一個 scope 上，scope 結束時整批退回 root 表，沒有計數。A1 之後，在量出來最熱的路徑（bindings 回傳節點、parser 的 sink）上局部使用，不全面改型別。
- **B4：不 root 的 `NodeRef`**：`dom_api` 和 bindings 的內部用 lifetime 綁住的裸指標，只在可能觸發 GC 的地方之前才 root；要放寬 `crates/dom/soundness.md` 的 I1、重寫論證、補 compile-fail 測試。A1 之後 root 的成本還明顯才考慮。
- **bindings 本身**（下一份 RFC 的候選）：`dom_js` 的轉換、wrapper 的建立、`properties::call`、`indexed_item`；19 個 suite 合計約 185 ms，最集中的是 Charts-chartjs 的 54 ms（一直呼叫 canvas 2D 的方法，每次都走通用的宣告表、`Value` 的轉換和 `ScriptHost` 的動態分派）和 jQuery 的 37 ms。照 Blink 的做法，每個方法各自產生專用的 binding 函式，先從呼叫最頻繁的 canvas 2D 開始。
- **style 加 box tree 的另一半**：`ComputedStyle` 的分組共用（RFC 0051 的 D8，照 Blink 的 `DataRef` 群組或 Stylo 的 `Arc` 的 style struct）、後代的 invalidation set、box tree 的重建；扣掉 TipTap 約 300 ms，分散在每個 suite。
- **shaping 的結果按字詞重用**（Blink 的 `ShapeCache` 的 word cache）：TipTap 的 shaping 本身約 26%；要先評估 key 和命中率。
- **V8 的 build 設定**：C++ 的 PGO 和 ThinLTO。2026-10-18 量過（`details.md` 的「V8 本身」）：tier up 和 deopt 和 Chrome 一樣，JIT 的程式和 builtin 不比較慢，V8 的 C++ runtime 慢 13–71%（JSON、regexp、字串、`Map` 的雜湊），估計約 20–40 ms；要先編插樁的 V8、用 Speedometer 訓練、再重編，效益不夠，先不做。builtin 的 PGO 要求開著 V8 sandbox，kun 沒有開。
- **斷行的緊急斷行機會**（原本的 1d，第 9 條不做）：`overflow-wrap` 不是 `normal` 時，parley 在每個字元 clone 一份 `LineState`（TipTap 7.3%）；照 Blink 不記、溢出時從行首重做。
- **fontconfig 的冷啟動**（原本的 1b，第 5 條不做）：fontique 找一個 family 的字型時用 `FcFontSort` 排整個系統的字型（每個 process 每個 family 一次）；改用 `FcFontList` 要保證取出的 face 和現在一樣。
- **`update_elements` 和 `reset_form_owner`**（第 17 條）：TodoMVC-jQuery 的 2.61% 和 1.07%，還沒拆開是哪幾步；先用只取樣這兩個函式（或暫時的計數器）找出熱點，再看能不能像表單狀態的捷徑一樣，跳過不受影響的元素。
- **canvas 的 raster 移出 main thread**（RFC 0051 的 F1）：Charts-chartjs 約 50 ms、Perf-Dashboard 約 60 ms。
