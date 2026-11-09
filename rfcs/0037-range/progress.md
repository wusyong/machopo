# RFC 0037 的進度：`Range` 和 `StaticRange`

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-12。完成：2026-10-12，Fedora 44（Linux x86_64；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0037` 上分兩段做，第 1 段做完、經過審核時先 merge 回 `main`。動到 GC 而要跑的 `test.py matrix`、`fuzz` 在 merge 之後看 CI。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a `Range`、`StaticRange` 的 GC 型別和弱集合、`createRange`、建構子、設定、`collapse`、`select*`、`cloneRange`、`detach`；JS 和 wasm | 完成，merge 回 `main` | 2026-10-12 |
| 1b 插入、移除（含 adopt）的調整 | 完成，merge 回 `main` | 2026-10-12 |
| 1c 字元資料的原語（`replace_data`、`split_text`、`normalize` 的合併）和調整 | 完成，merge 回 `main` | 2026-10-12 |
| 1d 比較、`toString`、WPT、文件、限制和第 1 段的檢查 | 完成，merge 回 `main` | 2026-10-12 |
| 2a `deleteContents`、`extractContents`、`cloneContents` | 完成，merge 回 `main` | 2026-10-12 |
| 2b `insertNode`、`surroundContents` | 完成，merge 回 `main` | 2026-10-12 |
| 2c `createContextualFragment`、WPT、文件、限制和第 2 段的檢查 | 完成，merge 回 `main` | 2026-10-12 |

## 驗收

### 資料和 GC

| 項目 | 怎麼驗證 |
| --- | --- |
| range 讀得回 boundary point（static 的任何 offset）；live range 在它的 start 的 document 的集合、static 的不在；設到別的 document 時搬過去和搬回來；static 的不能再設；range 撐住節點、document 不撐住 range、一直建了又回收的 range 會重用集合的空格 | `cargo test -p dom --test it ranges::` |
| `Range` 是 GC 型別的共通檢查；工作負載的 live range 在留著的節點之間（offset 隨機落在子節點或文字之間）、detached 的子樹只靠 range 撐住、static 的 | `cargo test -p dom --test it script_objects::range`、`script_objects::static_range`、`script::every_type_unwraps_only_as_itself`、`standalone_heap::`、`detached_marking::`、`cargo test -p dom --test incremental_stress` |

### live range

| 項目 | 怎麼驗證 |
| --- | --- |
| 插入時 parent 裡 offset 大於新節點的 index 的往後一格（在 index 上、在前面、別的節點的、append 的不動）；移除時被移除的節點裡和底下的移到 (parent, index)、parent 裡大於 index 的往前一格（別的節點的、被移除的 host 的 shadow tree 裡的不動）；adopt 之後 range 在新 document 的集合、跟著它的插入移動；replace data 時被取代的那段裡的移到開頭、那段之後的照長度的差移動（UTF-16），設整串都回到 0；split 時原節點裡超過 offset 的移進新節點、parent 裡在它後面一格的往後一格；static 的都不動 | `cargo test -p dom --test it ranges::` |
| 工作負載的 live range 在隨機的插入、移除和字元資料的修改（也會變短）之後，offset 不超過節點的長度 | `cargo test -p dom --test it standalone_heap::`、`detached_marking::` |
| 經過 JS 的 API 移動 live range：`insertBefore`、fragment、`remove`、`removeChild`、`textContent`、`adoptNode`；`data`、`appendData`、`insertData`、`deleteData`、`replaceData`、`splitText`、`normalize`（被併的 text 和 parent 裡在它們位置上的移進第一個，第一個自己的不動）；`StaticRange` 不動；原本的字元資料的行為和錯誤照舊 | `cargo test -p dom_js --test it ranges::live_ranges_move_with_the_tree`、`ranges::character_data_moves_live_ranges`、`cargo test -p dom_api --test it api::character_data_methods`、`api::text_nodes` |

### 設定、比較和 `toString`

| 項目 | 怎麼驗證 |
| --- | --- |
| `AbstractRange`、`Range`、`StaticRange` 的 interface；`createRange()`、`new Range()`；`setStart`、`setEnd` 的規則（另一端跟著、另一棵樹、祖先的子節點的 index、UTF-16 的 offset、`IndexSizeError`、`InvalidNodeTypeError`）；`setStartBefore` 等、`collapse`、`selectNode`、`selectNodeContents`、`cloneRange`、`detach`；`StaticRange` 的檢查、沒有 setter、`Range` 的方法不收它 | `cargo test -p dom_js --test it ranges::` |
| `commonAncestorContainer`、`compareBoundaryPoints`（四種 `how`、`unsigned short` 的轉換：65536 是 0、`-1` 和 `"-1"` 是 65535；`NotSupportedError`、`WrongDocumentError`；和自己比）、`isPointInRange`、`comparePoint`、`intersectsNode`（邊界上的節點）；`toString`（UTF-16、不算 comment） | `cargo test -p dom_js --test it ranges::ranges_compare`、`ranges::ranges_stringify_their_text` |
| `unsigned long` 照 WebIDL 的 ToUint32，字串的負數也取 2^32 的餘數；`compareDocumentPosition` 和 range 共用 tree order | `cargo test -p dom_js --test it bindings::arguments_convert_by_webidl`、`cargo test -p dom_api --test it api::` |

### 內容

| 項目 | 怎麼驗證 |
| --- | --- |
| `deleteContents`、`extractContents`、`cloneContents`：一個 text 裡的、跨元素的（部分選到的 text、部分選到的元素的淺的複本裝著其餘的、完全選到的節點移進去或深的複本）、收起的位置、collapse 的 range 的空 fragment、doctype 在動任何東西之前是 `HierarchyRequestError`（delete 照樣刪） | `cargo test -p dom_js --test it ranges::ranges_delete_extract_and_clone_their_contents` |
| `insertNode`（split、offset、fragment、collapse 與否、節點自己是 reference 或在它前面、`HierarchyRequestError` 在 split 之前）；`surroundContents`（包住元素、部分選到的 text、跨兩個 text、清空新的 parent、選起它；`InvalidStateError`、`InvalidNodeTypeError` 在動任何東西之前） | `cargo test -p dom_js --test it ranges::ranges_insert_nodes`、`ranges::ranges_surround_contents` |
| `createContextualFragment`：context（起點的元素、text 的 parent、`<table>` 讓 `<tr>` 進 `<tbody>`、document 的 `<body>`）、fragment 的 document、custom element 在回來之前建構、XML 文件是 `NotSupportedError` | `cargo test -p dom_js --test it ranges::ranges_make_contextual_fragments` |
| `createContextualFragment` 的 script（巢狀的也是）插進文件時執行，`innerHTML` 的不執行 | `cargo test -p shell --test it inserted_scripts::contextual_fragments_scripts_run` |
| 內容的方法是 `[CEReactions]`（被移除的 custom element 的 `disconnectedCallback` 在方法回來之前）、`MutationObserver` 的紀錄（被移除的節點的子節點不另外記），和規範的 WebIDL 一樣 | `cargo test -p dom_js --test it ranges::content_methods_call_back_and_are_observed`、`cargo test -p dom_api --test it spec::` |

### wasm 和 Lua

| 項目 | 怎麼驗證 |
| --- | --- |
| wasm 的 guest 的 `create-range`、`set-start`、`set-end`、錯誤的代碼（`IndexSizeError` 8、`InvalidNodeTypeError` 22）、`static-range`、`select-node-contents`、`extract-contents`；WIT 只加不改（`WrongDocumentError` 是 23） | `cargo test -p dom_wasm --test it engine::a_guest_uses_ranges`、`cargo test -p dom_api --test it wit::` |
| Lua 的 `create_range`、`set_start`、`set_end`、錯誤的代碼、`dom.StaticRange.new` 的 table（必填的物件的欄位，沒給 table 是錯誤） | `cargo test -p shell --test it wasm_reftests::w9_lua` |

### WPT

| 項目 | 怎麼驗證 |
| --- | --- |
| `dom/ranges/`（`tentative/` `disabled`）照預期結果：53 個測試、38 個全過、44508 個 subtest 過 41687 個；剩下的等 `Selection`、window 的具名屬性、XML parser | `python3 scripts/test.py wpt dom/ranges/` |
| `domparsing/createContextualFragment.html`、`Node-normalize-2`、`MutationObserver-characterData`、`-childList`、`custom-elements/reactions/Range`、`range-and-constructors`、`shadow-dom/Range-prototype-insertNode` 全過，`css/cssom-view/` 六份跑得完 | `python3 scripts/test.py wpt domparsing/ dom/nodes/ custom-elements/ shadow-dom/ css/cssom-view/` |

## 做過的檢查

兩段做完時都跑過 `cargo fmt --check`（連同 `sdk/lua/generator`、`sdk/rust`）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（第 1 段 2352 個、rebase 到 RFC 0036 第 2 段之後 2388 個、第 2 段 2394 個，42 個 binary，全部通過）、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`，和上面的 WPT（0 個不符）。每一步都跑了改到的 crate 的測試和 mutation test（`mutants-1a.toml` 到 `mutants-2c.toml`）。

- **做了什麼**：
  - 第 1 段：`dom` 的 `src/range.rs`：`Range` 和 `StaticRange` 是同一個 GC 型別（`is_static`，`dom_api` 照它分成兩種 interface，`AbstractRange` 的成員收同一個 `RangeHandle`，照 `PerformanceEntry`），live range 在它的 start 的 document 的 rare data 的弱集合（第 2 條）；`WeakSet` 多了 `insert_unordered`（從上次的位置找空格，找過一遍都沒有時先往後加、等集合長到兩倍再找；原本的 `insert` 照第一個空格放，slot 的 manual assignment 靠這個順序）。插入、移除、adopt 的調整在 `insert_before`、`detach_with`（和 `NodeIterator` 同一處）、`move_live_ranges`；字元資料的 `replace_data`（UTF-16）、`split_text`、`merge_text_ranges` 是 `dom` 的新原語，`dom_api` 的 `CharacterData`、`splitText`、`normalize`（改成在 run 的第一個的尾端 replace data，原本設整串會把它自己的 range 移到 0）改呼叫它們。`dom_api` 的 `range` 模組：DOM 的「length」、「index」、tree order（`compareDocumentPosition` 共用）、「position of a boundary point」、設定和比較、`toString`。新的例外 `InvalidNodeTypeError`、`WrongDocumentError`（WIT 的代碼 22、23）。宣告表沒有 16 位元的整數，`compareBoundaryPoints` 的 `how` 宣告成 `unsigned long`、取 2^16 的餘數。
  - 第 2 段：DOM 的「extract」和「clone the contents」（一個函式）、`deleteContents`、「insert」、`surroundContents`，用既有的插入、移除、replace data、clone 組成；HTML 的 `createContextualFragment`，用 `markup` 的 fragment parsing，清掉 script 的 already started 和 parser，fragment 裡的 custom element 照名稱 upgrade。內容的方法都是 `[CEReactions]`。
  - WIT 每一步只加不改，`ci.py guests` 重編了七次（9 到 17 分鐘，和 mutation test 或 WPT 同時跑時比較慢）。Lua 的 generator 多了 record 裡必填的物件欄位（`StaticRangeInit` 的 container），整個 table 沒給時報錯，不把 handle 0 傳給 host（會 trap、停掉整個 Lua runtime）。
- **找到的 bug**：`dom_js` 的 `unsigned long` 用 V8 的 `Uint32Value`，它不把字串的負數取 2^32 的餘數（WPT 的 `compareBoundaryPoints` 有 93 個因此失敗）；改成照 WebIDL 自己算，所有收 `unsigned long` 的成員都受影響。
- **寫測試時的錯**：好幾處預期結果我照規範推錯，照規範改：`dom_js` 的 boundary point 的位置兩處（1a）、`compareBoundaryPoints` 的 START_TO_END 和 `comparePoint` 超過長度（1d）；`dom` 的「空格重用」原本測「GC 之後的下一個就用舊的空格」，和「長到兩倍再找」的設計不符，改測集合不會一直長（1a）；doctype 的 `InvalidNodeTypeError` 要用同一棵樹裡的 doctype（不在樹裡的先是 `WrongDocumentError`）；`createContextualFragment` 的 custom element 的檢查原本繞了一圈匿名 class 的名字。
- **mutation test**（七份，共 120 條）：
  - 每份寫之前都先想過每條 mutant 會不會被抓到，補了測試的邊界（例如祖先的子節點的 index 剛好等於 offset、同一個節點不同 offset 的 `collapsed`、`normalize` 的第一個 text 自己的 range、`intersectsNode` 剛好從 end 開始的節點、`insertNode` 的五個邊界、`surroundContents` 跨兩個 text）。
  - 跑完漏網、補了測試或換了測試指令之後抓到的：`StaticRange` 被當成 `Range`（`this` 的檢查，1a）；「移除的 range 移到 parent 的開頭」（`dom` 的測試裡剛好在 index 0，交給 `dom_js`，1b）；「replace data 不移動 range」在工作負載裡（測試指令原本寫成沒有工作負載的 `gc_stress::`，工作負載的資料原本只會變長、range 很少落在被改的 text 上，1c）；`END_TO_START` 改成比兩個 end、`toString` 從 offset 0 開始（1d）；在一個 text 裡 delete、doctype 前面有 comment（2a）；fragment 當新的 parent、extract 之後不先收起（2b）；text 的起點的 context、巢狀的 script（2c）。
  - 等價的（記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」，拿掉那條 mutant）：「replace data」不把超過尾端的 count 截短（1c）、「insert」不先擋 comment 和 processing instruction 的起點（2b）。
  - 用 WPT 的測試抓到的：「起點的節點算成完全選到」（`Range-extractContents`、`-cloneContents`）、「`<html>` 自己當 context」（`domparsing/createContextualFragment.html`）。
  - 每段做完時 fmt 重排之後對不上的 `from` 跟著改（第 1 段 15 條、第 2 段 11 條），全部重跑：1a 28 條、1b 12 條、1c 18 條、1d 22 條、2a 19 條、2b 14 條、2c 7 條，共 120 條，約 24 分鐘，全部抓到。
- **WPT**：第 1 段試跑 `test.py wpt --all dom/ranges/`（沒有逾時、crash），`tentative/` 是 `OpaqueRange`（提案）`disabled`，整個收進 `include.txt`；數字記進 `tests/wpt/README.md`。最後 53 個測試、38 個全過、44508 個 subtest 過 41687 個，剩下的等 `Selection`（`Range-mutations-*` 每份一半的 subtest 是 selection 裡的 range）、window 的具名屬性（`Range-extractContents-dynamic-end`）、XML parser（`StaticRange-constructor` 的 CDATA、`createContextualFragment-xhtml` 的一個）。連帶變成通過或跑得完的：`domparsing/createContextualFragment.html`（35 個）、`Node-normalize-2`、`MutationObserver-characterData`、`-childList`、`custom-elements/reactions/Range`、`range-and-constructors`、`shadow-dom/Range-prototype-insertNode`、`css/cssom-view/` 六份（開頭用 `createRange` 或 `attachShadow`），預期結果更新，沒有退步。
- **rebase**：第 1 段 merge 之前 `main` 多了 RFC 0036 的第 2 段（`IntersectionObserver`），衝突都是兩邊各加了自己的 GC 型別和文件（`soundness.md` 的 I3、I14、型別的數量、`object.rs` 的 import、`testing.md`、`tests/wpt/README.md`），兩邊都留；之後 fmt、clippy、`cargo test --workspace`、sanity、WPT 的 `dom/ranges/`、`dom/nodes/`、`intersection-observer/` 都過。
- **畫面**：沒有看得到的改變（幾何不在這份，第 1 條），沒有新的情境；`python3 scripts/test.py automation` 不用跑（沒有動到視窗那一層）。
