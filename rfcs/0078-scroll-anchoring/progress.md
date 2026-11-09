# RFC 0078 的進度：scroll anchoring

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-23，Fedora 44。在 `0078` 上做。

完成日期：2026-10-23，Fedora 44（AMD Ryzen AI 9 HX 370）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 `overflow-anchor`，viewport 和捲動容器的 anchoring：選 anchor、調整、suppression trigger（第 1 到 4 條） | 完成 | 2026-10-23 |
| 2 priority candidate；WPT；TipTap 的驗收（第 5 到 7 條） | 完成 | 2026-10-23 |

## 驗收

**`overflow-anchor`**

| 項目 | 怎麼驗證 |
| --- | --- |
| `auto`、`none`，不繼承；CSSOM 讀回、computed value、無效的寫法照 Chrome 152 | `cargo test -p style --test it -- cssom::overflow_anchor computed::overflow_anchor_is_not_inherited` |
| 捲動容器自己是 `none` 時沒有 anchoring（設了之後 anchor 也不見），`<body>` 上的不影響裡面的容器；inline 元素是 `none` 時連同裡面的文字跳過 | `cargo test -p renderer --lib -- overflow_anchor_none`；WPT 的 `opt-out*`、`exclude-inline` |

**選 anchor**

| 項目 | 怎麼驗證 |
| --- | --- |
| 照 Blink 的 `FindAnchorRecursive` 和 `Examine`：tree order 第一個在可見範圍裡的，完全可見的就是它、部分可見的往裡面找；不看 `overflow-anchor: none`（連同裡面）、fixed、sticky、沒有寬或高的；盒子往下延伸到沒被裁切的 overflow；out-of-flow 的盒子在 DOM 的 parent 那裡看，containing block 那裡後備；匿名的盒子和 inline 元素往下走到文字；文字是 anchor；一行裡先行、再左到右；transform；別的捲動容器裡也找；容器的可見範圍是 padding box | `cargo test -p layout --test it -- scroll_anchor`（Chrome 152 上用行為對照，`details.md`） |
| priority candidate：有 focus 的 contenteditable（和在它裡面的、inline 的 editing host 往上第一個有盒子的）先，部分可見也是，在它裡面往下找；`<textarea>`、`<input>`、tabindex 不算；在 `overflow-anchor: none` 或 sticky 裡、不在這個捲動框裡的不算 | `cargo test -p renderer --lib -- the_focused_editable_element_first`；WPT 的 `focused-element-*`（`focus-prioritized` 卡在 Window 的 named access） |

**調整和 suppression**

| 項目 | 怎麼驗證 |
| --- | --- |
| layout 之後照 anchor 移動的距離捲，在裝置像素上取整；在頂端不選；script 和 compositor 的捲動清掉、在新的位置重新選；clamp 和調整本身不清；anchor 被移除、`display: none`、盒子重建、`overflow-anchor: none` 時在上一次的 layout 裡重新選；不動的 anchor 留著；viewport 先、容器由外往內；調整之後 clamp；`scroll` 事件；整棵 box tree 重建（viewport 的 `overflow` 改了時留著 anchor，root 的盒子重建時不調整） | `cargo test -p renderer --lib -- content_above at_the_top a_scroll_picks a_scroll_container_keeps the_scroll_is_in an_anchor_without overflow_anchor_none an_anchor_stays the_move_is_clamped a_containers_anchor the_whole_box_tree`（預期值都是 Chrome 152 的，`details.md`） |
| suppression trigger：anchor 到捲動框那條路上的 inset、margin、padding、寬高和 min、max、`position`、transform 改了時不跟、清掉 anchor，border 照跟；捲動框裡有盒子（捲動容器本身也是）進出 flow 時也不跟；inline 元素也算；容器外面的改變不影響容器的 | `cargo test -p renderer --lib -- a_change_on_the_way a_box_leaving what_keeps` |

**整個瀏覽器**

| 項目 | 怎麼驗證 |
| --- | --- |
| 上面插入內容之後 script 讀到的已經調整好、一個 `scroll` 事件、畫面最上面還是同一個元素；script 捲動之後照新的位置 | `cargo test -p shell --test it -- scroll_anchoring` |
| `about:demo` 的「Add a line above」：框在頂端時新的一行出現；捲下去之後最上面那一行留在原位 | 情境 `scroll-anchoring`（`python3 scripts/test.py scenarios scroll-anchoring`，看截圖 `anchored.png`） |
| WPT 的 `css/css-scroll-anchoring/`：91 個 subtest 通過 57 個（開始前 15 個），其他的原因在 `.ini` | `python3 scripts/test.py wpt css/css-scroll-anchoring/` |
| Editor-TipTap 不再走 `resetScrollPos` | `python3 scripts/bench.py speedometer --suites Editor-TipTap --profile`：兩個瀏覽器的 profile 都沒有它（10-22 kun 在它上面 176.9 ms）；這個 suite 在計時區間裡 kun 161.0 ms、Chrome 60.5 ms，多的在 TipTap 的 `layout`、`setValue`（`docs/performance-gaps.md`） |

## 做過的檢查

2026-10-23，Fedora 44（AMD Ryzen AI 9 HX 370），兩段都在這台上做。

- **做了什麼**（設計見 `README.md` 的「詳細設計」）：`css`、`computed_style` 的 `overflow-anchor`。`layout` 的 `scroll_anchor.rs`（`find_scroll_anchor`、`scroll_anchor_offset`）和 box tree 更新時記下的 `ScrollAnchoringChanges`。renderer 的 `ScrollState::anchors`、`scroll_anchor.rs` 的 `pick_scroll_anchors`、`adjust_for_scroll_anchors`，接在 `FrameView::update_own_style_and_layout`。`about:demo` 的「Add a line above」，情境 `scroll-anchoring`。WPT 的 `css/css-scroll-anchoring/` 加進清單。
- **和 Blink、Chrome 對照之後改的**（讀了 Blink 的 `scroll_anchor.cc`、`layout_object.cc`、`local_frame_view.cc`、`document.cc`、`paint_layer_scrollable_area.cc`、`css_properties.json5`，再用 Chrome 152 的行為確認，`details.md`）：
  - 別的捲動容器一開始當成一個盒子、不往裡面找；探測時 viewport 跟著容器裡的內容移動（10 對 40），改成往裡面找，調整照 Blink 的 queue 的順序（viewport 先）。
  - out-of-flow 的盒子一開始在 containing block 那裡看，改成在 DOM 的 parent 那裡（`BoxFragment::out_of_flow`），containing block 那裡是後備。盒子的範圍照 `RelativeBounds` 延伸到沒有裁切的 overflow。一行裡的候選一開始照頂端排（inline-block 對齊 baseline 時比文字高），改成「第幾行、再左到右」。
  - suppression：Blink 的 `UpdateAfterReinsert` 做了規範的第二個 trigger，照第 4 條做；比對的 property 照 `css_properties.json5` 裡 invalidate `scroll-anchor` 的那幾個加上 transform。
  - anchor 被移除、`display: none` 時一開始只清掉、不調整；Blink 在舊的位置重新選，所以會調整（150 對 50），改了。盒子重建也一樣：情境 `shadow-dom` 抓到（標題換 slot 之後 viewport 跟著它的文字走）。選 anchor 時用 DOM 現在的 `overflow-anchor`，不用上一次 fragment 的舊 style。
  - 整棵 box tree 重建時一開始沒有比較 style，suppression 不見（150 對 190）；root 的盒子重建時 Blink 清掉所有 anchor（105 對 125）。兩個都是 mutation test 沒抓到之後補探測發現的。
  - WPT 抓到的：捲動容器自己的 `overflow-anchor: none` 沒有關掉它的 anchoring（`opt-out*`），inline 元素的 `overflow-anchor: none` 只看最內層的 inline 元素（`exclude-inline`）。
  - 2 倍時 Chrome 的模擬（`Emulation.setDeviceMetricsOverride`）在 CSS px 上取整，kun 在裝置像素上：kun 的 script 捲動本來就照 Blink 的 `SnapScrollOffsetToPhysicalPixels`，anchoring 照 Blink 的 `ComputeAdjustment`（zoom-for-dsf 時在裝置像素上），保留。
- **Chrome 152 的探測**（`details.md`）：65 種情況，最後 kun 每一種讀到的值都和 Chrome 一樣，除了在 inline 中間插入 block 的那一種（Blink 的新匿名 block 整段跳過，記在 `docs/limitations.md`）。WPT 的 `css/css-scroll-anchoring/` 也在 Chrome 上跑了一次（靜態的 server），`dirty-contents-reselect-anchor`、`multicol-fragmented-anchor`、`anchor-inside-textarea` Chrome 自己也沒過。
- **順帶找到的、和這份無關的**（記在 `docs/limitations.md`）：iframe 的初始 `about:blank` 換成 `srcdoc` 時 kun 建新的 Window，之前加在 `contentWindow` 上的 `load` listener 收不到（`anchor-inside-iframe` 因此 timeout）；設 `location.hash` 之後不是當場捲動；`focus()` 把部分在畫面裡的元素也捲進來。Window 的 named access 卡住這個目錄約 15 個測試，照第 6 條另外寫 RFC 草稿。
- **情境改的**：`resize` 的兩個 `expect-differs` 比的是元素現在的盒子：視窗變窄時 anchoring 讓畫面停在原位，`#calc-bar` 和 `#media-swatch` 變窄之後的盒子在舊的 frame 裡是同一塊顏色，永遠等不到；改成比整個 frame，和反過來在變寬之後比 swatch。`demo-script` 的按鈕清單多一個。
- **mutation test**：
  - `mutants-1.toml`（第 1 段，最後 64 個）：第一次的輸出被 `tail` 截掉，只留下沒抓到的最後 5 個，所以整份重跑：`display: none` 的判斷和容器不再捲動時清掉 anchor（都等價，拿掉程式）；文字看 parent 的 `overflow-anchor`、捲動容器本身進出 flow、整棵重建（補了測試，最後一個發現上面的 bug）。第二次 66 個抓到 58 個：零高度、完全可見的盒子（補）；`overflow: hidden` 的判斷（`extend_overflow` 本來就不延伸，拿掉程式）；containing block 的後備（mutant 只改了回傳值，改成讓迴圈不跑）；沒移動的 anchor 留著（測試裡兩個改變在同一次 layout，改成中間讀一次）；整棵重建時保留記下的改變（root 重建時清掉 anchor 之後不需要了）。第三次 65 個抓到 63 個，2 個等價的記在 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。第 2 段之後 4 個的原文不在了：3 個改成現在的寫法、重跑都抓到，「文字看 parent 的」由第 2 段的「只看節點自己的」取代。
  - `mutants-2.toml`（第 2 段，12 個）：第一次 14 個沒抓到 4 個：取鏈上第一個有盒子的、路上的 `overflow-anchor: none`（`skip` 已經用現在的 style 檢查）、不在畫面裡的 priority candidate（`examine_candidate` 本來就跳過）是等價的，前兩個改寫或拿掉程式、mutant 跟著換；在 priority candidate 裡面往下找沒抓到，補了測試（Chrome 也是 700）。第二次 12 個全部抓到。
  - 每次跑完都確認檔案還原（`target/mutate/originals/` 不存在）。
- **跑過的**：改到的 crate 全部（`css`、`computed_style`、`style`、`layout`、`renderer`、`shell`）；情境全部（70 個）；`python3 scripts/test.py wpt css/css-scroll-anchoring/ css/cssom-view/ css/css-overflow/ css/css-position/sticky/`（2702 個檢查，一個非預期：`css/css-overflow/clip-003.html` 通過，`.ini` 是 2026-10-20 在 macOS 上標的失敗，原因是 opacity 混色的捨入差 1；頁面沒有捲動、不會選 anchor，連跑三次都通過，判斷是 Linux 上本來就通過，`.ini` 沒改）。每段結束時：`cargo test --workspace --no-fail-fast`（最後一次 3630 個通過、3 個 ignored）、`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`（第 1 段第一次抓到測試裡兩處，改了）、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`（`docs` 只剩 README 的 `../third_party/rusty_v8_build` 在這台不存在，和這次無關）。
- **不穩定的**：沒有。
