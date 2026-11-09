# RFC 0078：scroll anchoring

- 開始日期：2026-10-23
- 狀態：✅ 已完成：2026-10-23（驗收和做過的檢查見 `progress.md`）
- 細節：`details.md`（Chrome 152 的探測：65 種情況，WPT 在 Chrome 上的結果）

## 摘要

照 Blink 的 `ScrollAnchor` 和 CSS Scroll Anchoring：每個捲動中的捲動框（viewport 和捲動容器）在 layout 之前選一個 anchor，記下它相對於可見範圍的位置；layout 之後 anchor 在 block 方向（kun 只有水平書寫，就是垂直方向）移動了多少，就把捲動位置調整多少，使用者看到的內容不跳。加上 `overflow-anchor: auto | none`。

## 動機

**開始前**（當時 `docs/limitations.md`「捲動」那一條）：kun 沒有 scroll anchoring，也沒有 `overflow-anchor`：上面的內容變高或變矮時，捲動位置不動，看到的內容跟著跳。

**問題**：

- **畫面**：圖片晚到、廣告插入、留言展開這類「可見的內容上方變了」的情況，Chrome 保持使用者看的位置，kun 跳開。
- **框架走的路和 Chrome 不同**：ProseMirror（Speedometer 的 Editor-TipTap）判斷 `this.dom.style.overflowAnchor == null`，沒有這個屬性時每次更新前後自己記住、恢復捲動位置（`storeScrollPos` / `resetScrollPos`），每次都用 `getBoundingClientRect` 強迫 layout。2026-10-22 的 `--profile`：TipTap 的 `resetScrollPos` 在 kun 每個 iteration 176.9 ms（Linux）、96.4 ms（Windows），Chrome 是 0，是 kun 和 Chrome 差最多的一個 JS 函式。只加 property、不做 anchoring 會騙過偵測，ProseMirror 就不再自己保持，捲動位置沒人管。
- **WPT**：`css/css-scroll-anchoring/` 的 79 個測試（91 個 subtest）在 kun 上只過 15 個 subtest（2026-10-23）。

**目標**：

- `overflow-anchor` 照 Chrome 解析、計算、`getComputedStyle`、`CSS.supports`；`style.overflowAnchor` 存在。
- viewport 和捲動容器照 Blink 選 anchor、在 layout 之後調整，跳過 `overflow-anchor: none` 的子樹、fixed 和 sticky 的盒子；anchor 和它到捲動框之間的祖先的位置、大小類 style 改變時不調整（suppression trigger）；有 focus 的可編輯元素優先（priority candidate）。
- 調整經過現有的捲動路徑：clamp、`scroll` 事件、和 compositor 同步。
- WPT 的 `css/css-scroll-anchoring/` 加進清單，和 Chrome 的差別寫進 `.ini`。
- Editor-TipTap 不再走 `resetScrollPos`。

**要解決的 `docs/limitations.md` 的限制**：「捲動」那一條的「沒有 scroll anchoring …… 也沒有 `overflow-anchor`」。

**不在範圍內**：

- 歷史紀錄和重新載入時照 anchor 恢復捲動位置（Blink 的 `GetSerializedAnchor` / `RestoreAnchor`）：kun 本來就不恢復捲動位置（`docs/limitations.md` 的「連結導航和 hit testing」）。
- find-in-page 的 priority candidate：kun 沒有 find-in-page。
- 垂直書寫：kun 沒有 `writing-mode`。
- `content-visibility: auto` 的 anchor：kun 沒有 `content-visibility`。

## 設計概要

```html
<div id=ads></div>
<p>第 1 段 ……</p> ……
<script>
  scrollTo(0, 2000);           // 使用者在看第 30 段
  ads.style.height = "300px";  // 上面多了 300px
</script>
```

Chrome 和做完之後的 kun：下一次 layout 之後 `scrollY` 是 2300，畫面上還是第 30 段；`#ads` 加上 `overflow-anchor: none`（或它的祖先有）也一樣，因為 anchor 是第 30 段，不是 `#ads`。只有 anchor 自己或它的祖先的位置、大小類 property 改了（例如第 30 段被設了 `margin-top`）時不調整。

```js
el.style.overflowAnchor        // ""（以前是 undefined）
getComputedStyle(el).overflowAnchor  // "auto"
```

## 詳細設計

### 1. `overflow-anchor`

- `auto | none`，不繼承，放在 `RareNonInherited`；`layout` 的 `StyleDifference::between` 當它只影響 paint（不重新 layout），它改了的元素記下來（下面第 4 節）。

### 2. 每個捲動框的 anchor（`renderer` 的 `scroll.rs`、`scroll_anchor.rs`）

- `ScrollState::anchors`：viewport（`None`）和每個捲動容器各一個 anchor，節點（元素，或文字的 text node，`layout::ScrollAnchorNode`）和它的頂端在捲動框可見範圍頂端下面多少（Blink 的 `saved_relative_offset_`）。
- `set_viewport` / `set_container`（使用者、script、compositor、fragment、`scrollIntoView` 的捲動都經過它們）清掉那個捲動框的 anchor（Blink 的 `UpdateScrollOffset` 對 explicit 的捲動清掉）；clamp 和 anchoring 自己的調整用不清的 `move_scroller`。

### 3. 選 anchor 和調整（`FrameView::update_own_style_and_layout`）

這個函式也是 script 強迫 layout（`getBoundingClientRect`、`scrollY` 等）時走的路，所以兩種 layout 都調整（Blink 的 `Document::UpdateStyleAndLayout` 也呼叫 `PerformScrollAnchoringAdjustments`）。

- **之前**（`pick_scroll_anchors`，Blink 的 `NotifyBeforeLayout`）：box tree 更新之後、layout 之前，在上一次 layout 的 fragment tree 裡選：
  - 捲動位置在 block 方向是 0、或捲動容器自己是 `overflow-anchor: none`（Blink 的 `ShouldPerformScrollAnchoring`）時沒有 anchor。
  - 已經有 anchor 的留著，除非它沒有原本的盒子（不在 document 裡、或它或它的祖先的盒子這次重建：插入、移動、重新分配 slot、`display` 改變，Blink 的 anchor 是 layout object 的弱指標），或捲動框裡有元素的 `overflow-anchor` 改了（Blink 的 `ClearAncestorScrollAnchors`）。
  - 沒有的選一個（`layout::find_scroll_anchor`，Blink 的 `FindAnchorRecursive` 和 `Examine`）：tree order 第一個在可見範圍裡的；完全在裡面的就是它，部分在裡面的往裡面找，找不到就是它，之後的不看。盒子的範圍是 border box 往下延伸到沒被裁切的 overflow（`RelativeBounds`）；沒有寬或高的、fixed、sticky、`overflow-anchor: none` 的連同裡面跳過；匿名的盒子和 inline 元素往下走到文字（一個 text node 在一個 block 裡的所有 run 算一個框）；out-of-flow 的盒子在它 DOM 的 parent 那裡照 tree order 看（`BoxFragment::out_of_flow`），containing block 那裡再當後備（`FindAnchorInOOFs`）；一行裡的候選照第幾行、再左到右；別的捲動容器裡面也找（Blink 的搜尋不停在捲動容器）。上一次的 fragment 帶的是舊的 style，所以沒有原本的盒子的、現在往上到捲動框的路上有 `overflow-anchor: none` 的（照 DOM 現在的 style）跳過。
  - priority candidate 先（Blink 的 `FindAnchorInPriorityCandidates`）：document 的 focused element 在 editable 的內容裡時，它和它往上（flat tree）第一個有盒子的；它在這個捲動框裡、往上到捲動框的路上沒有 fixed、sticky、也不被跳過時，在它裡面照上面的規則找，找不到就是它本身。
- **之後**（`adjust_for_scroll_anchors`，Blink 的 `Adjust`）：layout、clamp、sticky 之後，viewport 先、容器照 tree order（Blink 的 queue 的順序：外面的先量），量 anchor 現在的位置（`layout::scroll_anchor_offset`），和記下的差在裝置像素上取整（Blink 的 `ComputeAdjustment`），經過 `move_scroller` 捲，最後再 clamp；`scroll` 事件和 compositor 的同步是現成的路。量不到（沒有盒子）的 anchor 清掉。

### 4. 不調整的時候（suppression trigger）

- `layout` 的 box tree 更新時，`BoxTree::restyle` 比較每個 style 變了的盒子（和 inline 元素）的新舊 style（`note_scroll_anchoring_changes`），記進 `ScrollAnchoringChanges`：
  - `disabled`：inset、margin、padding、寬高和它們的 min、max、`position`、`transform` 改了的元素（Blink 的 `css_properties.json5` 裡 invalidate `scroll-anchor` 的那幾個加上 transform；`contain-intrinsic-size` kun 沒有）；盒子進出 flow（absolute、fixed 和其他之間）時，記它外面最近的捲動容器、或 root element（Blink 的 `UpdateAfterReinsert`，規範的第二個 trigger）。
  - `overflow_anchor`：`overflow-anchor` 改了的元素。
  - `rebuilt`：盒子重建的元素。
- 整棵 box tree 重建時：為了傳給 viewport 的東西（`DocumentBoxes`：`overflow` 等）重建的，逐個比較新舊 block 盒子的 style，anchor 留著（Blink 不重建 layout object）；root element 的盒子重建的，記 root 為重建，所有的 anchor 都不見（Blink 整棵重新 attach）。
- 調整時，anchor 或它往上到捲動框（含）的路上有 `disabled` 的元素，而且調整不是 0 時，不調整、清掉 anchor（Blink 的 `ComputeScrollAnchorDisablingStyleChanged`，只在調整不是 0 時清）。

### 5. compositor

- 主 thread 改的捲動位置照舊在 commit 時推給 compositor（`SyncedScrollOffset::push_from_main`），compositor 還沒交回的使用者捲動疊在上面再 clamp：和 cc 對 Blink 的 anchoring 調整一樣，不用新的訊息。

### 測試

- `style`：`overflow-anchor` 的 CSSOM、computed value、無效的寫法。
- `layout`：`tests/it/scroll_anchor.rs`，選 anchor 的每條規則。
- `renderer`：`frame_view.rs` 的測試，照探測的步驟（捲動、改 DOM 或 style、layout、讀捲動位置），預期值是 Chrome 152 的：調整、清掉和重新選、suppression 的每一類、priority candidate、`overflow-anchor: none` 的捲動框和 inline 元素、整棵 box tree 重建、clamp、裝置像素。
- `shell`：`tests/it/scroll_anchoring.rs`，端到端（script 讀到的已經調整好、`scroll` 事件、畫面最上面還是同一個元素）。
- 情境 `scroll-anchoring`（`about:demo` 的「Add a line above」）。
- WPT 的 `css/css-scroll-anchoring/`：91 個 subtest 通過 57 個（開始前 15 個），沒過的原因在 `.ini`。
- mutation test：`mutants-1.toml`、`mutants-2.toml`。

### 分階段

1. `overflow-anchor`，viewport 和捲動容器的 anchoring：選 anchor（不含 priority candidate）、調整、suppression trigger。
2. priority candidate（有 focus 的可編輯元素）；WPT 加進清單、預期結果；TipTap 的驗收。

## 缺點

- 每次 layout 前後都要對捲動中的捲動框找 anchor、量它的位置：走 fragment tree 一段（還沒有 anchor 時才找），捲動框多的頁面會多一點時間（Blink 一樣）。
- 調整讓「改了 DOM 之後的捲動位置」變得和以前不同：情境 `resize` 的兩個比對（元素的盒子在 resize 前後的 frame 裡看起來一樣）改了寫法。
- 和 Chrome 還不同的地方記在 `docs/limitations.md` 的「捲動」：一行裡的候選的順序、在 inline 中間插入 block 時的匿名 block、被搬走的 text node、SVG 裡的元素、`scroll-padding`、find-in-page。

## 理由和其他做法

- **只加 `overflow-anchor` property**：騙過 ProseMirror 的偵測，但內容在上方變化時畫面會跳，比現在更糟（ProseMirror 本來會自己保持）。
- **照規範的 DOM 走訪選 anchor**（「for each DOM child N」）而不照 Blink 的 layout tree：WPT 和網站看到的是 Chrome 的行為，照 Blink（「已決定的事項」第 2 條）。
- **實作時對照 Blink 和 Chrome 改的**（每一條都用 Chrome 152 的行為確認，`details.md`；經過見 `progress.md`）：別的捲動容器原本當成一個盒子、不往裡面找；out-of-flow 的盒子原本在 containing block 那裡看；anchor 被移除、盒子重建時原本只清掉、不調整（Blink 在舊的位置重新選，所以會調整）；選 anchor 時原本用上一次的 fragment 的舊 style；整棵 box tree 重建時原本漏掉 suppression；捲動容器自己的 `overflow-anchor: none` 和 inline 元素的 `overflow-anchor: none` 原本沒看。

## 先例

- **Chromium**：`ScrollAnchor`（`scroll_anchor.cc`：`NotifyBeforeLayout`、`FindAnchor`、`Examine`、`ExaminePriorityCandidate`、`ComputeAdjustment`、`Adjust`、`ClearSelf`），`LocalFrameView::PerformScrollAnchoringAdjustments`，`ComputedStyle` 的 scroll anchor disabling 的比對。
- **規範**：CSS Scroll Anchoring Module Level 1（anchor node selection、suppression triggers、`overflow-anchor`）。

## 已決定的事項

2026-10-23 確認（使用者：1 到 3「照建議」，4 看過選項之後「好」）：

1. **分兩段**（「分階段」）：第 1 段 `overflow-anchor` 和 viewport、捲動容器的 anchoring（選 anchor、調整、suppression trigger）；第 2 段 priority candidate、WPT、TipTap 的驗收。TipTap 的編輯區在 `overflow-y: auto` 的捲動容器裡捲動（2026-10-23 在 Chrome 裡查的），所以捲動容器在第 1 段；有 focus 的 `.ProseMirror` 是 priority candidate。不選的做法：一段做完。
2. **照 Blink 走 layout 的結果選 anchor**：走 fragment tree，文字的行也可以是 anchor，inline 往下走（Blink 的 `FindAnchorRecursive`、`Examine`）。不選的做法：照規範走 DOM 的子節點、用每個節點的盒子判斷。理由：WPT 和網站看到的是 Chrome 的行為；kun 的 fragment tree 和 Blink 的 layout tree 對得上（每個盒子有 `node`）。
3. **每次 layout 的前後選和調整**：在 `FrameView::update_own_style_and_layout` 的 layout 前後，rendering update 和 script 強迫的 layout 都走這裡（Blink 的 `NotifyBeforeLayout`、`PerformScrollAnchoringAdjustments`）。不選的做法：只在 rendering update 的 layout 之後一次（規範的 suppression window 到 event loop 的一次 iteration 結束）。理由：框架在 script 裡強迫 layout、讀位置時，Chrome 已經調整好；只在 rendering update 調整的話，那時讀到的是沒調整的位置。
4. **suppression 在 layout 更新 box tree、比較新舊 style 的地方記**：`layout` 的 `StyleDifference::between` 本來就對每個 style 變了的盒子比較新舊 style，在那裡多看那幾類 property（`top`、`right`、`bottom`、`left`、`margin`、`padding`、寬高和它們的 min、max、`position`、`transform` 類），改了就把節點記進這次更新的集合；`position` 在 absolute 和其他之間改了另外記一個旗標（規範的第二個 trigger）。layout 之後檢查 anchor 到捲動框那條路上有沒有記到的節點。不選的做法：選 anchor 時記下那條路上的 style、調整前比對（做不到第二個 trigger）；style 重算時在元素上設旗標（Blink 的做法，在 style 最熱的路徑上、沒有 anchor 也照樣做）；不做 suppression（WPT 的 suppression 測試失敗，頁面自己調整位置時和瀏覽器互相拉扯）。理由：結果和 Blink 一樣，只在 style 真的變了的盒子上多比一次，而且是已經在比的地方，不碰 `style` 和 `dom`；只有盒子會當 anchor 和它的祖先。

2026-10-23 確認第 2 段的事（使用者：「都照建議」）：

5. **priority candidate 照 Blink 的 `FindAnchorInPriorityCandidates`**：有 focus 的元素在 editable 的內容裡（contenteditable 的 editing host 和它裡面）時，從它往上找第一個有盒子、不是 inline 的元素當候選；每個捲動框各自判斷（Blink 的 `ExaminePriorityCandidate`：往上到這個捲動框的路上有 `overflow-anchor: none`、fixed、sticky，或候選不在這個捲動框裡時不用），可以用時先在它裡面照一般的規則找，找不到就是它本身。focus 從 frame 的 document 的 focused element 拿。不選的做法：文字控制項（`<textarea>`、`<input>`）也算。理由：Chrome 152 的探測只有 contenteditable 算，`<textarea>`、`<input>`、tabindex 都不算（`details.md`）。find-in-page 的結果是 Blink 的另一個 priority candidate，kun 沒有 find-in-page，不做。
6. **WPT 的 `css/css-scroll-anchoring/` 的失敗**：scroll anchoring 本身的問題在這一段修（捲動框自己的 `overflow-anchor: none`、`exclude-inline`、`fragment-scrolling-anchors`、`dirty-contents-reselect-anchor`、`anchor-inside-iframe` 和兩個 crash test 的 timeout 查原因）；缺的是別的功能的（Window 的 named access、`execCommand`、`window.open`、直書、multicol、`scroll-padding`）標成預期失敗、附註解，不在這份 RFC 做。Window 的 named access 另外寫一份 RFC 草稿（它影響的不只這個目錄），不在這一段做。
7. **TipTap 的驗收用 `bench.py speedometer --suites Editor-TipTap --profile` 跑一次**：看 profile 裡沒有 `resetScrollPos`，順便看時間。不選的做法：在 kun 裡開頁面、用 script 數呼叫（打包過的程式不好包住函式）。

## 待決定

無。

## 之後的可能

- 歷史紀錄和重新載入時照 anchor 恢復捲動位置（kun 先要恢復捲動位置本身）。
- find-in-page 有了之後，它的 active match 當 priority candidate。
