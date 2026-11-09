# RFC 0060 的進度：不 root 的節點指標

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-19。完成：2026-10-19，Fedora 44（AMD Ryzen AI 9 HX 370；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0060` 上做。進 `main` 的是 (a)、(b) 和集合的 cell（第 9 條）；第 1 步的 `NodePtr` 連同它們在本機的 branch `0060-node-ptr`（`8fdb3931`，連 rusty_v8 fork 的 `kun-v152.2.0-12`），沒有進 `main`。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 原型（第 3、4、6 條）：fork 包 cppgc 的 no-GC scope、離開時照節奏回報；`with_ptrs`、`NodePtr`、`BrandedScope`、`AsNode`；`innerHTML` 的 fast path 換成 `NodePtr` | 做完、量過，不進 `main`（第 9 條，`0060-node-ptr`） | 2026-10-19 |
| 2 jQuery 剩下的 handle 成本（第 5、7 條）：(a) realm 的 window、(b) 插入、移除的表單元素先用讀取判斷、集合的 root 也重用 cell；(c) 量過不留，(d) 不做 | 完成 | 2026-10-19 |
| 3 bindings 回傳節點的快速路徑 | 不做（第 7 條） | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 新 wrapper 的 realm 不建 root 也找得到（節點決定的、已經 root 的節點決定的、window 的） | `cargo test -p dom_js --test it -- iframes::`、`cargo test -p dom_api` |
| 插入、移除表單元素時，沒有事要做的不走一遍；debug build 照樣走一遍，確認 DOM 的版本和 batch 都沒變 | `cargo test -p dom_api --test it -- forms::`、`cargo test -p shell --test it -- forms:: focus:: keys::`；交叉檢查在所有 debug build 的測試裡 |
| 集合的 root 和節點的一樣重用 cell：丟掉的集合被回收、之後的不配置 `Persistent`；拆掉 isolate 時兩個池都清空 | `cargo test -p dom --test it -- collections::dropped_collection_roots_are_reused_and_root_nothing`、`cargo test -p dom --lib` |
| slot 沒有指派到節點時，fallback content 的插入和移除要 signal，指派到之後不用 | `cargo test -p dom --test it -- slots::fallback_changes_are_signaled` |
| TodoMVC-jQuery 和 `main` 輪流各取樣三次：cycle −3.2%、指令 −4.7% | `ci.py speedometer --sample --against main --suites TodoMVC-jQuery`（使用者要求時量，數字在「做過的檢查」） |

## 做過的檢查

**完成時（2026-10-19，Fedora 44）**：拆開之後的 `0060`：`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings` 乾淨；`cargo test --workspace` 通過（131 秒，`systemd-run` 限制記憶體）；`sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs` 通過。V8 照舊是 `kun-v152.2.0-11`；rusty_v8 fork 的 `kun` 分支 revert 了 no-GC scope（`5b98c50`，和 `-11` 的樹一樣），`third_party.toml` 的 `rev` 指著它，`ci.py third-party` 通過。這份 RFC 進 `main` 的部分沒有動到 GC 的 `unsafe`，`matrix`、`fuzz` 照常由 CI 跑。

**拆開之後**（第 9 條）：`0060` 從 `cd3677cd` 重新開，只搬 (a)、(b)、集合的 cell、`slots::fallback_changes_are_signaled` 和它們的文件，不需要第 1 步的任何東西（`cargo check` 直接通過）。`cargo test -p dom -p dom_api -p dom_js` 通過（109 秒）。mutation test 用 `test.py mutate` 重跑 11 條，都抓到：(a)、(b)（`cargo test -q -p dom_api -p dom_js`）8 條：realm 用 document 的 window 而不是 realm window、已經 root 的節點決定的 realm 拿不到、插入的元素一律當成 owner 不變、勾選的 radio 當成不用 uncheck、插入的 option 跳過、插入的有 `id` 的跳過、移除的元素的 form owner 不看、移除的 form 跳過（後六條是 debug build 的交叉檢查抓到的）；集合的 cell（`cargo test -q -p dom`）3 條：集合沒有池、拆掉時不清集合的池、還回的集合 cell 沒指回錨點。取樣：和 `main` 輪流各三次，cycle −3.2%、指令 −4.7%（含 `NodePtr` 的那一輪是 −4.7%、−5.3%：`NodePtr` 的份量約 0.6% 的指令；cycle 的雜訊比較大，current 三次 14396–16098）。`reset_form_owner` 478 → 0、`drop_glue::<Handle<Node>>` 494 → 360、`cppgc__Persistent__CONSTRUCT` 和 `DESTRUCT` 各約 70 → 2、`v8::Object::GetCreationContext` 127 → 39 個樣本。

**`0060-node-ptr` 上的檢查**（合併之前，含第 1 步）：`cargo test --workspace` 通過（220 秒）；`cargo fmt --check`、clippy、`sanity.py` 的五項通過；換成 `kun-v152.2.0-12` 之後下載的 V8 跑 `cargo test -p bindings -p dom` 通過；debug 的 V8（`ci.py v8 --profile debug`）跑過 `V8_FORCE_DEBUG=1` 的 `cargo test -p dom -p html -p bindings -p dom_js`（13 個 binary、895 個測試）；`test.py wpt html/semantics/forms/ html/webappapis/ dom/nodes/`（722 個測試）沒有非預期的結果。mutation test 第 1 步 12 條都抓到：closure 不在 no-GC scope 裡、closure 裡 stress GC 沒關、之後沒開回來（一開始漏掉：測試在新的 scope 裡檢查，而每個 scope 從 isolate 複製 `gc_stress`；改成在同一個 scope 裡）、swap 了沒發現、handle 不檢查 isolate 就變成指標、新節點的 document edge 沒寫、parent 的 edge 指向子節點自己（測試無限繞圈，手動結束之後記成抓到）、下一個兄弟節點指向子節點自己、別的 document 的節點走 parser 的快速路徑、template 的 content 沒有 host、slot 的 fallback 變化沒記下（原本沒有測試，補了 `slots::fallback_changes_are_signaled`）、fast path 帶屬性的元素跳過 hooks。離開 no-GC scope 時照節奏回報，拿掉它只影響效能，測試看不到。

**跑 mutation test 時兩次被 OOM 打斷**：「下一個兄弟節點指向子節點自己」讓 `dom` 的測試長到 58 GB，OOM 連帶結束了跑 `test.py` 的 Claude Code，兩次都留下改壞的 `scope.rs`（`test.py mutate --restore` 放回去）。之後測試指令加上記憶體的上限，那條 51 秒就被結束、記成抓到（寫進 `docs/testing.md` 的「Mutation test」）。timeout 一開始設 40 分鐘，卡住的 mutation 要等滿，改成 5 分鐘（基準那一輪 18 秒）。

**原型時查到、改了設計的**（`0060-node-ptr`）：
- **swap**：`BrandedScope` 給 `&mut DomMutationScope`（`DerefMut`，hooks 要），closure 可以用 `std::mem::swap` 把它和捕捉到的另一個 isolate 的 scope 對調，brand 不變、底下換了 isolate。所以每個接 `NodePtr` 的操作先比對 `Roots` 的指標，離開 no-GC scope 用進來時記下的 heap。
- **讀 `NodePtr` 只經過 `BrandedScope::node`**：不和任意的 read scope 配，否則別的 isolate 的 read scope 會把它 root 進那個 isolate。
- **standalone heap 本來就不會在配置時 GC**：它不掃 stack，cppgc 用 `GCInvoker` 把配置觸發的 GC 改成排一個 task（只在 scope 外的 `run_platform_tasks` 執行），no-GC scope 在這裡多一層保險。要守的是接上 V8 的 heap。
- **只有沒有子節點的節點走快速路徑**：`parser_append_child` 的子節點已經有子節點、或在別的 document 時走 `insert_before`，那裡還接 handle，`NodePtr` 會先 root（parser 的順序是建好就接上，fast path 不會走到）。
- **「只有 `NodePtr` 指著的節點還讀得到」不能當證據**：GC 認定死掉的節點要等它那一頁被 sweep 才 finalize（canary 才設），那時可能已經過了；測試改數 finalize 的節點。

**no-GC scope 和 V8 的 marking 節奏**：原型一開始指令 −1.9%，但 cycle +3.0%，幾乎都在 `recalc_style` 自己（三次的中位數 1452 → 2283）；同樣的程式拿掉 no-GC scope（不安全，只為了量）是 cycle −1.2%。原因是 closure 期間的配置不回報給 V8（`ReportBufferedAllocationSizeIfPossible` 檢查 `IsGCAllowed()`），之後一次回報只推進 incremental marking 一步。fork 改成離開時把累積的配置量（`CppHeap` private 的 `buffered_allocated_bytes_`）切成 128 KB（V8 的 `kIncrementalMarkingCheckInterval`）一份一份回報（explicit instantiation 不檢查存取權的寫法取得兩個 private 成員，V8 的原始碼不改）之後，cycle −0.8%、`recalc_style` 1418 → 1419，推測證實（第 6 條）。巢狀時外層還在 no-GC scope 裡，每次回報都直接返回、不清空，累積量留給外層。`signal_slot_changes` 自己 162 → 345 是歸屬的差別：`link_child` 連同它呼叫的 9.65% → 9.57%，多的正好是 inline 進去的 `form_states_children_changed`、`mark_ancestors` 少的。

**每一項的取樣**（TodoMVC-jQuery，`--against main`，輪流各三次；累計到那一項為止）：

| 到哪一項 | cycle | 指令 | 看得到的 |
| --- | ---: | ---: | --- |
| 第 1 步（照節奏回報之後） | −0.8% | −0.7% | 建 handle、`Persistent`、`create_node` 的成本不見了 |
| (a) realm 的 window | −0.9% | −2.2% | `realm_window` 加 `realm_context` 248、211、275 → 60、59、60 個樣本 |
| (b) 表單元素先用讀取判斷 | −5.0% | −4.7% | `update_elements` 底下建 root 的 138、101、86 → 15、17、16 |
| (c) fast path 交給 hooks 的 root（不留） | −3.7% | −4.3% | 比 (b) 差：TodoMVC 範本裡帶屬性的元素大半本來就是 hooks 在意的，每個屬性卻多跑八個判斷；`fast_path::build` 底下的 root 只從約 550 降到約 450 |
| 集合的 root 也重用 cell（另一輪，同樣的設定只到 (b) 是 −2.9%、−4.0%） | −4.7% | −5.3% | `drop_glue::<Handle<Node>>` 511 → 159、`cppgc__Persistent__Assign` 267 → 153 |

(b) 之前用暫時的計數器（量完拿掉）數 TodoMVC-jQuery 一次的 `FormChange::Inserted`：約 120 萬個，幾乎全部是列表的 `<input>`、`<button>`，全部沒有 form owner、也找不到 form，option 80 個、有 `id` 的 42 個；每一個原本都建 root 再 `reset_form_owner`，什麼都沒改。(d) 和第 3 步也是先數過：`replace_all` 一次不到 2 萬次；回傳節點時的 `wrapper()` 約 140 萬次，87% 是新建 wrapper。集合之前照建 root 的型別分：節點的新建 1.62%、丟掉 1.17%，集合的丟掉 0.49%，其他型別都不到 0.03%。no-GC scope 的進出連同它呼叫的（大多是補回報、推進 marking）佔 cycle 的 0.13%–0.20%，自己幾乎是 0（第 8 條）。(c) 的程式碼收在 `git stash`（`RFC 0060 2c (hooks' concerns): measured, not kept`），沒有 commit。

V8 在本機從原始碼編：`cargo clean` 之後 release 整個重編 43 分鐘、debug 49 分鐘；發 `kun-v152.2.0-12` 由 fork 的 release workflow 編。

## 效能的量測

2026-10-19，Fedora 44（AMD Ryzen AI 9 HX 370，接著電源），使用者要求量的。`ci.py speedometer`：20 個預設的 suite，每個 suite 開新的瀏覽器、10 個 iteration，取每個 iteration 的 suite 總時間的中位數；kun 是 `perf` profile（ThinLTO），`4990ae44`（`0060-node-ptr`，含 `NodePtr`；拆開之後沒有重量），量一次；Chrome for Testing 152.0.7977.82，同一輪。只量這個版本，不和開始前比；「RFC 0058 完成時」那欄是那份 `progress.md` 的數字（`a80a22dc`，兩次的平均；和這份開始前的 `main` 只差 RFC 0059 的文件），不是同一輪，只能看大概。

| | 這次 | RFC 0058 完成時 |
| --- | ---: | ---: |
| kun，18 個 suite 合計（不含 Editor-TipTap、Perf-Dashboard） | 1861 | 1757 |
| Chrome，同上 | 887 | 841 |
| kun / Chrome，合計 | 2.10 | 2.09 |
| kun / Chrome，各 suite 的幾何平均 | 2.18 | 2.08 |

| suite | kun | Chrome | kun / Chrome | RFC 0058 完成時的 kun |
| --- | ---: | ---: | ---: | ---: |
| TodoMVC-jQuery | 303 | 116 | 2.62 | 329 |
| Charts-chartjs | 315 | 150 | 2.10 | 217 |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 165 | 30 | 5.51 | 154 |
| React-Stockcharts-SVG | 159 | 88 | 1.80 | 158 |
| NewsSite-Next | 132 | 94 | 1.40 | 130 |
| NewsSite-Nuxt | 118 | 72 | 1.63 | |
| TodoMVC-JavaScript-ES5 | 93 | 36 | 2.56 | 82 |
| Charts-observable-plot | 81 | 52 | 1.55 | |
| TodoMVC-Angular-Complex-DOM | 76 | 54 | 1.42 | 66 |
| TodoMVC-React-Complex-DOM | 62 | 34 | 1.85 | 66 |
| TodoMVC-WebComponents | 61 | 13 | 4.69 | 52 |
| TodoMVC-React-Redux | 59 | 37 | 1.58 | 73 |
| TodoMVC-Lit-Complex-DOM | 47 | 15 | 3.05 | |
| TodoMVC-Vue | 46 | 28 | 1.69 | |
| TodoMVC-Svelte-Complex-DOM | 39 | 10 | 3.86 | |
| TodoMVC-Backbone | 39 | 21 | 1.83 | 42 |
| TodoMVC-Preact-Complex-DOM | 35 | 15 | 2.29 | |
| Editor-CodeMirror | 33 | 22 | 1.53 | 33 |
| Perf-Dashboard（不算在合計裡：canvas 的 raster，雜訊最大） | 195 | 68 | 2.87 | 184 |
| Editor-TipTap（Chrome 自己的檢查沒過：iteration 9 少了 `ProseMirror-focused`，沒有比值） | | | | 235 |

RFC 0058 的 `progress.md` 沒有個別列出的六個 suite 留空。合計的比值和 RFC 0058 完成時差不多：TodoMVC-jQuery 303 ms（329）和取樣的方向一致，但合計多出的約 100 ms 大半是 Charts-chartjs（canvas 的 raster，這份 RFC 沒碰；RFC 0058 完成時兩次就是 196 和 238），同一輪的 Chrome 也慢了約 5%。這份 RFC 的效益集中在 DOM 操作重的 suite，一次的量測分不出來，看取樣。
