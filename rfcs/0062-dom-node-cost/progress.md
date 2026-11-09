# RFC 0062 的進度：DOM 節點的成本

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-20。完成：2026-10-20，Fedora 44（AMD Ryzen AI 9 HX 370；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0062` 上做；三段做完時各自 merge 回 `main`（`152be6f5`、`d743b815`、`625c6437`）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 fragment 只接指標：parser 往 fragment 裡只接指標、fragment 的旗標、拿出子節點時只拆指標（live range、node iterator、observer 照做）、debug build 的交叉檢查 | 完成 | 2026-10-20 |
| 2 parser 的配置和 attribute 的 hook（第 3 條）：scan 只記位置、attribute 不再為了 hook 複製、hook 照名稱判斷有沒有步驟、`fragment_append_child` 由 parser 傳 fragment | 完成 | 2026-10-20 |
| 3 表單的紀錄和子樹的走訪（第 4 條）：不在 document 裡的元素不記表單的 pseudo-class 的紀錄、沒事的插入和移除在 `dom` 就不記、插入和移除只走一趟子樹 | 完成 | 2026-10-20 |
| 4 tokenizer、配置時的 GC、`Member` 的 FFI | 量過，不做（第 5 條） | 2026-10-20 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| fragment | parser 往標記過的 fragment 裡只接指標，不走訪、不記紀錄；不是剛建的節點、沒標記的 fragment 照一般的插入；parent 不在給的 fragment 裡時 debug build 擋下 | `cargo test -p dom --test it -- tree::unnotified_fragments_link_and_give_up_by_pointers` |
| | 只有空的、自己的 fragment（不是 shadow root）能標記 | `cargo test -p dom --test it -- tree::only_empty_fragments_of_their_own_hold_unnotified_children` |
| | `innerHTML`、`insertAdjacentHTML`、`outerHTML` parse 出的節點只在接上目標時走訪一次 | `cargo test -p dom_api --test it -- markup::parsed_nodes_are_walked_once_inserted` |
| | `createContextualFragment` 給 script 的 fragment，子節點接到別處時 range、NodeIterator 照樣移動，subtree 的 observer 照樣有 transient registration | `cargo test -p dom_js --test it -- ranges::parsed_fragments_give_their_children_up_as_removed` |
| | 跳過的 append 只會記接上目標時會再記的紀錄 | debug build 的 fast path 每次用一般的 append 再建一次檢查（所有經過 fast path 的 debug 測試） |
| parser | `ParserHooks` 只對可能有步驟的 attribute 說有（`on…`、`<input>` 的任何 attribute、各模組 `ATTRIBUTES` 的名稱），`attribute_changed` 對其他的立刻返回 | `cargo test -p dom_api --test it -- markup::the_parser_hooks_have_steps_for_some_attributes`；各模組的 attribute change steps 的測試（`forms::`、`markup::`、`frames::` 等） |
| 走訪 | 插入和移除只走一趟子樹：有 browsing context 時走進 shadow tree，iframe、script 照 shadow-including 的順序，表單的只看 light tree | `cargo test -p dom --test it -- tree::moved_subtrees_are_walked_once` |
| 表單的紀錄 | 插入、移除之後沒事的 form owner 變化不記；表單的 pseudo-class 只記 document 裡的元素，沒事的插入只記 UA shadow tree、移除不記 | `cargo test -p dom --test it -- tree::form_elements_are_recorded_one_by_one tree::form_state_elements_are_recorded_one_by_one`；`dom_api` 的 `forms::removing_the_first_with_an_id_names_the_next` 和其他 `forms::` 的測試；debug build 檢查跳過的（`take_form_changes_skipped`、`take_form_states_skipped`） |
| 行為不變 | parse 和插入的結果不變 | `cargo test -p dom -p html -p dom_api -p dom_js`；`test.py wpt dom/ domparsing/ html/semantics/forms/ custom-elements/ shadow-dom/ html/semantics/embedded-content/the-img-element/ html/semantics/embedded-content/the-iframe-element/ html/semantics/scripting-1/` |
| 效能 | 基準頁的 innerHTML 和 Chrome 的倍數 3.3 → 2.4；Speedometer 的幾何平均和 Chrome 的比值 2.15 → 1.68（含 RFC 0061） | 基準頁（`details.md` 的「基準頁」）、`ci.py speedometer`（使用者要求時量，數字在「做過的檢查」和 `details.md` 的「完成時」） |

## 做過的檢查

**完成時（2026-10-20，Fedora 44）**：每段做完時各跑過 `cargo test --workspace`（122、104、124 秒）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check` 和 `sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`；第 3 段（`625c6437`）之後只改了文件：`sanity.py docs` 通過。改到的 `unsafe` 只在 `dom` 的樹的走訪和指標，fragment 多的旗標是 `#[no_trace]`，GC 的 trace 沒變；`matrix`、`fuzz` 照常由 CI 跑。

**實作時的發現**：
- 第 1 段：debug build 的交叉檢查一開始把整個完整的 parse 包起來比對，建元素時就記下的 `FormStateChange::Element`（兩條路都有）也被算進去，改成只看 append。`sanity.py boundary` 抓到測試用的計數器 `MOVED_SUBTREE_WALKS`（thread-local），和 `dom::testing` 其他的計數器一樣加進 `STATICS_ALLOWED`。
- 第 2 段：原本把 `<select>`、`<option>`、`<optgroup>` 當成任何 attribute 都可能有事，mutation test 發現 `select` 的步驟只看 `selected`、`disabled`、`multiple`、`size`，改成它自己的 `ATTRIBUTES`。
- 第 3 段：`dom_api` 確認跳過的 form owner 紀錄時，一開始比對整個 batch；`forms::an_empty_id_names_nothing` 裡 form 插入之後才設 `id=""`，走一遍時 batch 多看了那棵樹、什麼都沒改，改成只看 DOM 的版本和要更新的 select。原本的 `tree::form_elements_are_recorded_one_by_one`、`tree::form_state_elements_are_recorded_one_by_one` 寫死每個元素都記，照新的規則改寫。

**WPT**（都沒有非預期的結果，除了下面那一次）：
- 第 1 段：`dom/`、`domparsing/`、`html/semantics/forms/`、`custom-elements/`、`the-img-element/`（1413 個測試）。`html/syntax/parsing/` 不在 `include.txt`，沒跑。
- 第 2 段：再加 `the-iframe-element/`、`html/semantics/scripting-1/`、`html/canvas/element/`、`html/webappapis/scripting/events/`（2864 個測試）。九個目錄一起跑時，非預期的 9 個全在 `scripting-1/the-script-element/moving-between-documents/after-prepare-*`：script 在搬走之前就執行了。這些測試用牆上的時間排順序（`step_timeout` 等 2 秒，伺服器用 `time.sleep` 延後 stylesheet 和 script），機器忙的時候 script 會先執行。它們不經過 innerHTML 和 fast path。只跑那個目錄：第 2 段 9 次裡 2 次各有 1 個（每次不同的測試），`main`（第 1 段）7 次都沒有；後來兩邊輪流各 4 次，都沒有。兩次失敗都在別的大工作之後跑。沒有標成 known intermittent（`main` 上沒看到），看 CI。
- 第 3 段：`dom/`、`domparsing/`、`html/semantics/forms/`、`custom-elements/`、`shadow-dom/`、`the-iframe-element/`、`html/semantics/scripting-1/`、`html/interaction/focus/`、`html/semantics/document-metadata/`、`html/dom/`（2089 個測試）。

**mutation test**（spec 不放 repo，`cargo test -q -p dom -p html -p dom_api -p dom_js`）71 條：
- 第 1 段 18 條，15 條一開始就抓到（只接指標的 append 接進任何 parent、接任何節點、漏了前一個兄弟、漏了 last child；拿出子節點的路從不用、任何 fragment 都用、漏了 node iterator、live range、transient registration、留著下一個兄弟的 previous sibling；有子節點的 fragment 也能標記；fast path 不標記；插入多記一種接上目標時不會再記的紀錄；`innerHTML`、`outerHTML` 一個一個移除）。沒抓到的 3 條：
  - `insertAdjacentHTML` 一個一個移除：測試最後用 `outerHTML` 換掉的正好是 `insertAdjacentHTML` 插進來的項目。改成換掉第一個項目，重跑抓到。
  - 只接指標的 append、只拆指標的拿出不加 `dom_tree_version`：等價。之後一定接著插進目標，那裡會加；中間沒有東西讀以版本為 key 的快取。照樣加，維持「改樹就加版本」。
- 第 2 段 16 條，之後改形狀再跑 4 條：
  - 抓到：`on…`、`<input>`、frame、canvas、form states、image（含 `loading`）的名稱拿掉，`select` 的 `size`、`<option>` 的 `selected` 拿掉，fast path 不交任何 attribute 給 hook、元素拿到前面的元素的 attribute、重複的 attribute 往前找到別的元素的、字元參照不 decode、`fragment_append_child` 把任何 fragment 當成標記過。
  - 第一輪沒抓到 `<select>`、`<option>`、`<optgroup>` 從「任何 attribute」拿掉：改成 `select` 自己的 `ATTRIBUTES`（見上）。
  - 等價：`script` 的清單（只有 `src`，也在 `frame` 和 `image` 的清單裡）、`select` 的清單（四個名稱都在 form states 的清單裡）從判斷拿掉，結果一樣。各模組照樣列出自己的，免得之後只改了一份。
- 第 3 段 33 條，29 條一開始就抓到（走訪不進 shadow tree、離開 shadow tree 漏了 host 的子節點或還當作在 shadow tree 裡、shadow tree 的節點當成 light tree 的、shadow tree 裡的 `title`、`<meta>` 也算、不記 autofocus、移除也記 script；插入時有 `id` 或 `form` attribute、radio、有 owner、有祖先 form 的也跳過，parser 關聯的照樣記，沒記 `forms_seen`、只看 parent 找祖先的 form；移除時有 owner 的、form 也跳過；不在 document 裡也記、在 document 裡也不記、插進不在 document 裡的樹也記；有 owner、radio、select 也算 plain；textarea、optgroup 不要 UA shadow tree、checkbox 要、text input 不要；plain 的移除也記；legend 進出不在 document 裡的 fieldset 也記）。沒抓到的 4 條補了測試，重跑都抓到：
  - 移除有 `id` 的也跳過：`form` attribute 指的是那個 ID 的第一個元素，第一個是 input 時 button 沒有 owner，移除 input 之後換成後面的 form；補 `forms::removing_the_first_with_an_id_names_the_next`。
  - 不在 document 裡的 textarea 的文字也記、移除時不在 document 裡也記：只多記、`dom_api` 照樣處理，結果一樣；`tree::form_state_elements_are_recorded_one_by_one` 加上改文字和不在 document 裡的移除。
  - plain 的元素有 shadow root 不算：checkbox 還留著別的 type 的 UA shadow tree 時插入，要更新它；同一個測試加上。

**量測**（基準頁是 `details.md` 的「基準頁」；同一個版本每次開的時間差可到 30%，指令數比較穩，段落之間以指令數為準）：
- 第 1 段：基準頁 `main`（`4e353bf9`）和第 1 段輪流各開五次，`innerHTML` 1.078 → 0.733 ms（−32%），只 parse 進不在 document 裡的元素 1.113 → 0.731（−34%），`textContent = ''` −7%，`appendChild(template.content)` 和 `createElement` 加 `appendChild` 在 ±4% 以內。估計是 −19%。指令數（`set_inner_html` 底下每個節點）5735 → 4932（−14%）。
- 第 2 段：指令數 4931 → 4770（−3.3%），cycle 1876 → 1703（−9%）。attribute 的複製 163 → 26、hook 307 → 222、`fragment_append_child` 138 → 91、malloc / free 260 → 209；tokenizer 1072 → 1022，掃描本身（約 440）沒變。估計是 −6%。
- 第 3 段：`list.innerHTML = html` 每個節點（含呼叫結束時的 `after_call`）5533 → 4733（−14%），`after_call` 744 → 0；插入時的走訪 236 → 230、移除時 189 → 141（走訪本身比兩趟少，多了判斷要不要記的函式，插入約 76）。`list.textContent = ''` 373 → 323（−13%）；script 建同一棵樹 −20%。
- 完成時（`625c6437`）：基準頁和 Chrome 輪流各開五次，innerHTML 0.752 對 0.318 ms（2.4 倍，開始時 3.3）。`ci.py speedometer`：各 suite 的幾何平均 kun 78.8 → 65.4 ms，和 Chrome 的比值 2.15 → 1.68（`cedd95d5` 起算，含 RFC 0061）；TodoMVC-jQuery 393 → 206 ms（4.52 → 2.53 倍）。Editor-TipTap 在 Chrome 那邊無效（第 5 個 iteration 沒有 `ProseMirror-focused`），不算。各 suite 的數字在 `details.md` 的「完成時」。
