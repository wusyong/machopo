# RFC 0058 的進度：效能的第二輪

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-18。完成：2026-10-19，Fedora 44（AMD Ryzen AI 9 HX 370；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0058` 上做，第 1、2 段各自審核、merge 回 `main`（fast-forward）；第 3 段沒有進 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 純粹變快的（第 4–9 條）：1a `NodeId` 的表用 FxHash，1c `TextSource::clusters` 記住上一次的位置；1b、1d 不做 | 完成 | 2026-10-19 |
| 2 加上條件、配 debug 交叉檢查的（第 10–17 條）：2c 選字型記住第一個字型，2a 表單狀態（先查）；2b `NodeIdMap` 延後登記量過之後不做 | 完成 | 2026-10-19 |
| 3 handle 的 root 表（A1，第 18–22 條）：3a `Member` 的寫入在 Rust 裡做，3b 強的 root 表，3c 弱的 slot 表（`NodeIdMap`） | 先不做（第 22 條）：3a、3b 做完、量過，程式碼收在 `git stash`；3c 沒開始 | 2026-10-19 |

## 驗收

### 文字

| 項目 | 怎麼驗證 |
| --- | --- |
| 把 parley 的 byte 位置換回 DOM 文字的 UTF-16 位置時，從上一次的位置往後找，結果和二分搜尋一樣：長的文字跨好幾行、從右到左的文字；debug build 每次比對 | `cargo test -p layout --test it -- text_rects::a_long_text_over_lines text_rects::right_to_left_text` |
| 選字型時，排第一個的字型完整涵蓋 cluster 就直接用它，結果和完整的 query 一樣：缺字的換下一個字型、之後換回來；同一段 shaping 裡換了 family 或屬性（`display: contents` 的元素）的選自己的；debug build 每次用完整的 query 比對 | `cargo test -p layout --test it -- font_selection::` |
| 只算有 charmap 的字型這個條件 | 測不到（測試字型都有）：`docs/test-gaps.md` 的「能補、還沒補的測試空缺」 |

### 表單狀態

| 項目 | 怎麼驗證 |
| --- | --- |
| 剛插入、不是 option 或 select、也不依賴整棵樹的元素只記 UA shadow tree（有或需要 tree 的），被移除的這類元素不標記；debug build 每次照原本的做法再算一次，比對要標記的元素、select 和 option 自己的狀態、要更新的 UA shadow tree | `cargo test -p dom_api --test it -- forms::`、`cargo test -p shell --test it -- forms::`；`Plan::check` 的交叉檢查在所有 debug build 的測試裡 |
| `update_pending` 跳過的元素沒有 UA shadow tree、也不需要；debug build 確認 `update_form_states` 跳過的沒有再被記 | 同上；`cargo test -p shell --test it -- focus:: keys:: mouse:: selections::` |
| isolate 建過 `<option>` 之前不往上找 option；debug build 照樣找、確認找不到 | `cargo test -p dom_api --test it -- forms:: select_type::` |
| attribute change steps 用 `is_attr` 比對名稱：iframe 的 `srcdoc`、canvas 的 `width` 這類照樣生效 | `cargo test -p dom_api` |

### 其他

| 項目 | 怎麼驗證 |
| --- | --- |
| 以 `NodeId` 為 key 的表用 FxHash：只換雜湊函式，行為不變 | 現有的測試（`cargo test --workspace`） |

## 做過的檢查

**第 1 段（2026-10-18 到 19，Fedora 44）**：
- 1a：以 `NodeId` 為 key 的表換成 `NodeIdHashMap`、`NodeIdHashSet`，八個 crate 約 80 處（第 2 段又找到三個型別是推斷出來的：`text_control_tree::update_pending` 的 `seen`、`layout` 的 `changed_lists`、`renderer` 的 `canvas::flush_shown` 的 `kept`）。只換雜湊函式，沒有新的測試和 mutation test：SipHash 的走訪順序每個 process 都不同，現有的程式本來就不依賴它。
- 1c：`perf annotate` 看 TipTap 的取樣，熱點是 `TextSource::clusters` 每個 cluster 兩次 `partition_point`；改成從上一次的位置往後走（最多 8 步，之後二分搜尋剩下的），倒退時二分搜尋整個。新的 `text_rects::a_long_text_over_lines`、`right_to_left_text`；寫後者時發現 `text_rects` 對從右到左的文字一個框都不給，原本的程式也一樣（`docs/limitations.md` 已經寫著）。mutation test 5 條都抓到；不更新 hint、拿掉 8 步之後的二分搜尋是等價的（只是變慢）。
- 1b、1d、2b 查過、量過之後不做（README 第 5、9、14 條；2b 的計數：TodoMVC-jQuery 10 個 iteration 建了約 310 萬個節點，99.8% 被拿過 id）。

**第 2 段（2026-10-19，Fedora 44）**：
- 2c：parley fork 的 `FontSelector` 把上游的 `query_yields_font` 換成記住那個字型本身（`first_font`），在上游清掉的地方清掉；query 搬成 `best_font`，debug build 走快速路徑時也跑它比對。`font_selection::` 的三個測試，換 family、換屬性的各有 inline 和 `display: contents` 的版本：只有 inline 時，拿掉換 family、換屬性時的清除沒有測試失敗（inline 元素的邊本來就切開 shaping 的 item），所以補上 `contents` 的。mutation test 4 條抓到 3 條，沒抓到的是「只算有 charmap 的字型」（測不到，上面的驗收表）。fork 自己的 parley、parley_engine、fontique 測試通過；fork 的 `parley_tests` 編不過（`InlineBox` 少了 `line_break`，33 處），這次的改動之前就是這樣。
- 2a：先取樣、加暫時的計數器查（`details.md` 的「表單狀態的拆分」），再分兩輪做（README 第 15、16 條）。mutation test 第一輪 10 條、第二輪 3 條都抓到（第二輪改寫之後第一輪的重跑過）；`inclusive_option_ancestor` 在 release build 提早返回的那一行，debug build 一律往上找，靠 assert 確認旗標；拿掉 `left_out` 之後的 `return`、拿掉交叉檢查本身是等價的。取樣（TodoMVC-jQuery，和 2a 開始前比）：`update_pending` 4.41% → 2.00%，`update_form_states` 4.24% → 3.81%，`attribute_changed` 2.80% → 2.21%，`focus_fixup` 8.08% → 5.98%；整個 suite 的 cycle 樣本 −2.2%、指令 −2.8%。找 option 的祖先在 jQuery 省不到（它一載入就建了 `<option>`），ES5 和 ES6-Webpack-Complex-DOM 省得到。`update_elements` 自己的 2.61% 和 `reset_form_owner` 的 1.07% 沒拆（第 17 條）。
- WPT 和 reftest：`test.py wpt css/css-text/ css/css-fonts/ css/CSS2/linebox/`（570 個測試）、`test.py wpt html/semantics/ css/selectors/ html/rendering/`（1079 個測試）、`test.py reftest`（168 個），都沒有非預期的結果。
- 階段做完時，clippy 抓到一處：`text_changed` 裡 rustfmt 把 `unsafe` 換到下一行，`// SAFETY:` 不再緊接在它前面。完成之後的取樣才發現第 2 段留下一個 release build 的 warning：`Plan::left_out` 只有 debug build 的檢查讀，`cargo test` 和 clippy 都是 debug build，所以沒看到；加上 `cfg_attr(not(debug_assertions), allow(dead_code))`。

**第 3 段（2026-10-19，Fedora 44，先不做）**：程式碼收在兩個 repo 的 `git stash`（kun：`RFC 0058 stage 3 (3a, 3b), not done: kun side`；`../third_party/rusty_v8`：`… rusty_v8 side`），以 kun 的 `0058`（第 2 段 merge 之後）和 fork 的 `kun`（`kun-v152.2.0-11`）為底。接著做時：兩邊 `git stash pop`，`ci.py v8` 重編 V8（release 增量約 10 秒；debug 的 build 目錄舊的話要整個重編，這次 49 分鐘），`RUSTY_V8_ARCHIVE` 指向本機的 artifact。本機的 `../third_party/rusty_v8_artifacts` 是含 3b 的 fork 編的（多了幾個 C 函式；平常的 build 用下載的 prebuilt）。
- 3a：fork 的 `MemberSlot`、`WeakMemberSlot` 在 Rust 裡寫入；`binding.cc` 交出旗標的位址、慢速路徑和 C++ 的 `IsEnabled()`，`static_assert` 確認版面；kun 的 `bindings` 的 `build.rs` 比對照抄的 cppgc 程式碼，`soundness.md` 加 A11。新的 `detached_marking::the_write_barrier_flag_is_set_while_marking`；mutation test 7 條都抓到。取樣：`cppgc__Member__Assign` 1.06%、`CONSTRUCT` 0.23% 不見了，cycle −2.3%、指令 −1.1%。
- 3b：`dom` 的 `src/root_table.rs`（Rust heap 上 256 個一組的 chunk，錨點擁有、trace；`Root<T>` 是 entry 的指標加上 `UnsafePtr<T>`），`persistent()` 換成 `ptr()`（`dom` 裡 204 處），fork 加 `assign_off_heap` 和 `cppgc__WriteBarrier__MarkValue`；`soundness.md` 的 I1、I7、I8 和 root 的一節改寫。新的 `tree::the_root_table_grows_by_chunks_and_reuses_entries`、`detached_marking::nodes_rooted_after_marking_completes_survive`；mutation test 9 條都抓到；release 和 debug 的 V8 下 `bindings`、`dom`、`dom_api`、`dom_js`、`html`、`dom_wasm` 通過。取樣兩次 cycle 都多約 9%：floating garbage（README 第 22 條，`details.md` 的「root 表的取樣」）。

**完成時（2026-10-19，Fedora 44）**：`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings` 乾淨；`cargo test --workspace` 通過（45 個 binary、3245 個測試，1 分 25 秒）；`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs` 通過。第 2 段動到 `dom` 的 `unsafe`（`inclusive_option_ancestor`），GC 的 `matrix`、`fuzz` 照規則由 merge 之後的 CI 跑。

## 效能的量測

2026-10-19，Fedora 44（AMD Ryzen AI 9 HX 370，接著電源），使用者要求量的。`ci.py speedometer`：20 個預設的 suite，每個 suite 開新的瀏覽器、10 個 iteration，取每個 iteration 的 suite 總時間的中位數；kun 是 `perf` profile（ThinLTO），Chrome for Testing 152.0.7977.82。比較的是這份 RFC 完成時的 `main`（`a80a22dc`）和開始前的 `main`（`c76da5c4`，RFC 0057 完成時）。`c76da5c4` 釘的是舊的 parley，`--against` 要求兩邊釘的 `../third_party` 一樣，所以改成在 `../kun-against` 裡把 parley 暫時換回 `4a3f621` 跑 `--browser kun`，量完換回來。兩邊各量兩次、取平均；Chrome 量一次（kun 那次和它同一輪）。同一個版本兩次就差很多（TodoMVC-jQuery 開始前 350 和 470 ms，Charts-chartjs 完成時 196 和 238 ms），一個 suite 的差距只能看大概。

| | 開始前（兩次） | 完成時（兩次） | 差 | Chrome |
| --- | ---: | ---: | ---: | ---: |
| 18 個 suite 合計（不含 Editor-TipTap、Perf-Dashboard） | 1867 | 1757 | −110（−5.9%） | 841 |
| kun / Chrome，合計 | 2.22 | 2.09 | | |
| kun / Chrome，各 suite 的幾何平均 | 2.20 | 2.08 | | |
| Editor-TipTap（Chrome 那次自己的檢查沒過：iteration 2 少了 `ProseMirror-focused`，沒有比值） | 259（251、266） | 235（只有第二次） | −24 | — |
| Perf-Dashboard（canvas 的 raster，雜訊最大） | 136 | 184 | +48 | 48 |

各 suite（ms，兩次的平均）：

| suite | 開始前 | 完成時 | 差 |
| --- | ---: | ---: | ---: |
| TodoMVC-jQuery | 410（350、470） | 329（319、339） | −81 |
| NewsSite-Next | 158（135、182） | 130 | −28 |
| TodoMVC-Backbone | 58 | 42 | −16 |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 169 | 154（140、168） | −15 |
| TodoMVC-WebComponents | 62 | 52 | −10 |
| TodoMVC-Angular-Complex-DOM | 74（88、61） | 66 | −8 |
| Editor-CodeMirror | 40 | 33 | −7 |
| TodoMVC-JavaScript-ES5 | 87（99、74） | 82（68、95） | −5 |
| 其他 6 個 | | | −3–+1 |
| TodoMVC-React-Complex-DOM | 61 | 66 | +5 |
| React-Stockcharts-SVG | 149（141、157） | 158 | +9 |
| TodoMVC-React-Redux | 57 | 73（88、58） | +16 |
| Charts-chartjs | 176（194、159） | 217（196、238） | +41 |

兩邊都穩定、差距明顯的是表單和 DOM 操作重的 TodoMVC（Backbone、WebComponents、jQuery）和 TipTap；Charts-chartjs、Perf-Dashboard 是 canvas，React-Redux 的 +16 是完成時第一次的 88 ms（第二次 58），都在雜訊裡。React-Stockcharts-SVG 兩次都是 158，開始前兩次是 141 和 157，沒有再追。

另外，和 2026-10-18 那份 `measure.md`（`4d391d27`，在 RFC 0057 之前）比，同樣的 18 個 suite 的合計比值是 2.44 → 2.02，但那包括 RFC 0057 和兩天的機器狀態，不只是這份 RFC。
