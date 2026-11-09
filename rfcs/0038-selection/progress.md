# RFC 0038 的進度：`Selection` 和 `Range` 的幾何

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-12。完成：2026-10-12，Fedora 44（Linux x86_64；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0038` 上分兩段做；第 1 段在 1b 之後應要求先 merge 回 `main`（「階段的 merge」的例外，1c 之後再 merge 一次）。動到 GC 而要跑的 `test.py matrix`、`fuzz` 在 merge 之後看 CI。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a `Selection` 的資料和方法、`getSelection()`、range 換到別的樹時清空；JS | 完成，merge 回 `main` | 2026-10-12 |
| 1b `selectionchange`、editing host 的 focus | 完成，merge 回 `main` | 2026-10-12 |
| 1c wasm、WPT 的 `selection/`、文件、第 1 段的檢查 | 完成，merge 回 `main` | 2026-10-12 |
| 2a `layout` 的 text 節點和 UTF-16 的對應、text fragment 的原文和 cluster、`layout::text_rects` | 完成 | 2026-10-12 |
| 2b `LayoutClient::text_rects`、`Range.getClientRects()` / `getBoundingClientRect()`（JS 和 wasm）、WPT | 完成 | 2026-10-12 |
| 2c Speedometer 的兩個 Editor、animation frame callback 的順序、文件、第 2 段的檢查 | 完成 | 2026-10-12 |

## 驗收

### 選取的資料和 GC

| 項目 | 怎麼驗證 |
| --- | --- |
| 每份 document 一個選取、它的 range 和方向、`selection_of_range`；選取跟著它的 range 移動；document 撐住選取、選取撐住 range、空了之後放開 | `cargo test -p dom --test it selections::` |
| `Selection` 是 GC 型別的共通檢查和工作負載（選取拿走工作負載的 range、之後只有它撐住） | `cargo test -p dom --test it script_objects::selection`、`script::every_type_unwraps_only_as_itself`、`gc_stress::` |
| 宣告表的 `Selection`（`toString` 是 stringifier，沒有 `modify()`、`getComposedRanges()`）、`Range` 的兩個幾何的成員；WIT 只加不改 | `cargo test -p dom_api --test it spec:: wit:: api::` |

### Selection API

| 項目 | 怎麼驗證 |
| --- | --- |
| `getSelection()`（`window` 的同一個、沒有 window 是 `null`）；每個方法建新的 range、anchor 和 focus、方向和 `type`（Chrome 的）；檢查的種類和 WPT 的順序；`addRange`、`removeRange` 收 range 本身；script 改選取的 range 改選取、移出 document 時變空；`containsNode`、`deleteFromDocument`、`toString`；shadow tree，host 被移除之後的選取對 script 是空的 | `cargo test -p dom_js --test it selections::` |
| `selectionchange` 是 document 上的 task（不冒泡、不能取消、trusted），到它執行前只排一次；同樣的參數不排；script 改選取的 range、字元資料（`insertData`、`splitText`、`normalize`）移動選取時排，資料在選取之後、插入和移除節點、其他 range 移動時不排 | `cargo test -p shell --test it selections::selectionchange_is_a_task` |
| editing host 沒有 `tabindex` 也能 focus（`tabIndex` 是 -1），裡面的可編輯元素和 `contenteditable=false` 不能；`focus` 的 listener 執行時選取已經在它的第一個 text 的開頭，已經在裡面時不動；一般元素拿到 focus 不動選取 | `cargo test -p shell --test it selections::an_editing_host_takes_the_focus` |
| Tab 停在 editing host、不停在裡面的和 `contenteditable=false` 的；使用者的 focus（不經過宣告表）當場排 `selectionchange` | `cargo test -p shell --test it keys::tab_reaches_an_editing_host` |

### `Range` 的幾何

| 項目 | 怎麼驗證 |
| --- | --- |
| text 節點的一段的盒子（Ahem）：從範圍的開頭到結尾、字型的高度，collapse 的範圍寬度 0；摺疊掉的空白和掛在行尾的空白不佔位置；跨行時每行一個；換行（軟的和保留的 `\n`）的位置在下一行的開頭；同一個元素的第二個 text 節點、`position: relative` 的 inline 元素裡的；UTF-16 的 offset（點在 surrogate pair 中間時算整個字元）；沒有 box 的沒有；`transform` 和捲動容器的 offset | `cargo test -p layout --test it text_rects::` |
| `Range` 取哪些盒子（照 Blink：整個包含、不含頭尾 container 的最外層元素，每個 text 節點的一段；collapse 在空元素裡是它的；不在 document 裡沒有）；bounding 略過沒有寬高的、全是的話第一個 | `cargo test -p dom_js --test it ranges::a_ranges_boxes` |
| renderer 的 layout：跨行的文字、collapse 的範圍、viewport 的捲動、`position: fixed` 的文字不扣捲動、元素和它的文字 | `cargo test -p shell --test it geometry::a_ranges_boxes` |

### animation frame callback 的順序

| 項目 | 怎麼驗證 |
| --- | --- |
| 父 frame 的 callback 替子 frame 排的在同一個 frame、父的 timer 之前跑，父的先、子的後（子的先排也一樣）；子 frame 照 iframe 的 tree order；被移除的 frame 的 callback 不再要求 frame | `cargo test -p shell --test it script::a_child_frames_callback_runs_in_the_frame_its_parent_asked_in`、`script::child_frames_go_in_the_tree_order_of_their_iframes`、`script::a_removed_frames_callbacks_ask_for_no_frames`、`performance::` |

### wasm 和 Lua

| 項目 | 怎麼驗證 |
| --- | --- |
| guest 的 `get-selection`、空的選取的 `extend`（代碼 5）、`collapse` 和 `extend`、`to-string`、backward、`get-range-at`、`contains-node`、`remove-all-ranges` | `cargo test -p dom_wasm --test it engine::a_guest_uses_the_selection` |
| guest 的 range 的 `get-client-rects`、`get-bounding-client-rect` | `cargo test -p dom_wasm --test it engine::a_guest_reads_geometry` |
| Lua 的 `get_selection`、`collapse`（`nil` 也可以）、`extend`、錯誤的代碼 | `cargo test -p shell --test it wasm_reftests::w9_lua` |

### WPT 和 Speedometer

| 項目 | 怎麼驗證 |
| --- | --- |
| `selection/` 整個收（188 個跑、103 個整份通過、子測試 33891/34280）；`dom/ranges/`（51 個整份通過）、`css/cssom-view/`（6 個多整份通過）、`html/webappapis/animation-frames/` 和 `update-rendering/`（新收）等的預期結果；不更新再跑 0 個預期外 | `python3 scripts/test.py wpt selection/ dom/ranges/ css/cssom-view/ html/webappapis/animation-frames/ html/webappapis/update-rendering/` |
| wdspec 的 `new_window`、`new_tab` 的 `test_initial_selection_for_contenteditable` | `python3 scripts/test.py wdspec /webdriver/tests/classic/new_window/` |
| 兩個 Editor 跑完、不丟例外（M6）；TipTap 有效，CodeMirror 的 DOM 差 2 行 | `python3 scripts/ci.py speedometer --inventory --suites Editor-CodeMirror,Editor-TipTap`、`ci.py speedometer --suites … --rounds 1 --iterations 3`（RFC 0012 的 `progress.md`） |

## 做過的檢查

- **Chrome 152 的實測**（`details.md`）：每個方法的方向（collapse 的選取沒有方向、`addRange` 和 script 改 range 之後是 `forward`，和規範不同）、`containsNode`、`selectionchange` 的時機（字元資料移動選取時分派，插入和移除不分派）、editing host 的 focus 和選取、collapse 的 range 的盒子。檢查的順序照 WPT 的 `collapse.js`、`extend.js`、`setBaseAndExtent.html`、`selectAllChildren.html`。
- **改過的做法**：`selectionchange` 一開始用 `select` 那種 element task，`shell` 的測試裡事件晚於之後的 `setTimeout(0)`（renderer 在一輪裡先跑完到期的 timer），改成 DOM task（1b）。文字的盒子在 2b 時被 WPT 抓到兩處和 Chrome 不同（點在 surrogate pair 中間、換行處的 collapse），`TextCluster` 多記原文的結尾、collapse 的位置優先取它所在的 run。
- **WPT 抓到的其他行為**：shadow host 被移除時選取留在 shadow tree 裡，script 讀的時候看兩端在不在 document 裡（1c）；`uievents/textInput/` 的兩個從 FAIL 變成 TIMEOUT（editing host 拿得到 focus、還不能打字，`.ini` 註明）。
- **Speedometer 找到的 bug**（2c，第 14 條）：TipTap 的 DOM 摘要少了 `ProseMirror-focused`，原因是 renderer 在 frame 開始時就拿走每份 document 的 animation frame callback；用 `--automation-pipe` 直接開 TipTap 的頁面確認 focus 和選取本身沒問題之後，照 HTML 改成每份 document 輪到時才拿、子 frame 照 iframe 的 tree order（WPT 的 `child-document-raf-order` 抓到第二點）。`performance::timelines_start_when_navigations_do` 原本在父的 callback 裡讀子的 callback 設的值，靠的是舊的順序，改成在下一個 task 回報。修正之後 TipTap 有效（kun 743.9 ms、Chrome 86.4 ms）；CodeMirror 照舊差 2 個 `div`，是排版（行高、字型）的差異。
- **提前 merge**（1b 之後）：rebase 到 RFC 0036 做完的 `main`，衝突只有編好的 guest，在 1a 的 commit 上重編（`ci.py guests` 117 秒，之前 9 到 10 分鐘：新的做法 C 的 guest 共用一份 binding，每個都連結整個 world 的型別，WIT 一改全部都變）；RFC 的第 5 條原本寫升版，照 `crates/dom_api/README.md` 的規則改成不升。
- **mutation test**：`mutants-1a.toml`（33 條）、`1b`（20）、`1c`（4）、`2a`（18）、`2b`（13）、`2c`（3），91 條都抓到，第 1、2 段做完時照最後的程式重新指向錨點、重跑。沒抓到、補了測試的：沒有 window 的 document 的 `getSelection()`（`Selection` 的 `toString()` 讓 `null` 和空的選取在 log 裡一樣）、完全在外面的節點的部分包含、使用者的 focus 當場排 `selectionchange`（測試裡 listener 的 `getSelection()` 蓋過了）、其他 range 被字元資料移動、一般元素拿到 focus、bounding 全是空的盒子、子 frame 的順序。等價、記進 `docs/testing.md` 的：collapse 的範圍找到就停、丟掉沒有 frame 顯示的 document 的 callback；等價、拿掉程式的：結尾在 cluster 裡（cluster 一個接一個）。寫錯編不過的 1 條改寫。
- **跑過的測試**：`cargo test --workspace`：42 個 binary、2424 個通過（63 秒，第 2 段做完時；第 1 段時 2411 個）；`cargo fmt --check`（連同 `sdk/lua/generator`、`sdk/rust`）、`cargo clippy --workspace --all-targets -- -D warnings`（修了 `layout` 的回傳型別太複雜、兩個測試的 `drop(scope)`）；`sanity.py deps`、`boundary`、`unsafe`、`tests`、`docs`；`test.py reftest tests/reftests/text tests/reftests/inline`（0 個預期外）；上面的 WPT 和 wdspec。
- **畫面**：沒有看得到的改變（選取畫不出來），情境不需要。
