# RFC 0005 的進度：kun 自己的 GC 當 wasmtime 的 GC

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-01，Windows 11（x86_64）。第 1 段在 elise（`../third_party/elise`）裡做，第 2、3 段在 fork 的 `kun` 分支（`../third_party/wasmtime`，基底 v50.0.1）和 elise 裡做，kun 這邊到第 4 段之前沒有程式的改動（決定 13）。

完成日期：2026-11-08，Fedora 44（Linux 7.2，x86_64）。第 1 到 3 段和第一次的 4a、4b 在 Windows 11 上做；2026-11-02 起暫停（決定 20），2026-11-08 在 Linux 上重做 4a、做完第 4、5 段（elise 的 `2937ade`，fork 的 `eea40683a4`）。

下面的「決定 N」是 `README.md` 的「已決定的事項」第 N 條。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a 執行時登記的型別、照型別編號配置、型別的退休（決定 6 到 9） | 完成（elise 的 `dfa6ff8`） | 2026-11-01 |
| 1b 可能放著 `i31ref` 的邊（決定 10） | 完成（elise 的 `f9981d0`） | 2026-11-01 |
| 1c 外面給的 root 的 GC、不經過 `mutate` 的 bits 入口（決定 11、12） | 完成（elise 的 `f9981d0`） | 2026-11-01 |
| 2a fork 的 runtime：layout、配置、header、讀寫欄位、i31、`externref` 的 host data（決定 13、14） | 完成（審核過；`externref` 的 elise 的 `Id` 和死掉之後的 `dealloc` 在 2b 做） | 2026-11-02 |
| 2b fork 的 runtime：GC（root 的交接、配置失敗時的 GC、stress 模式、`externref` 的 host data 跟著物件死掉）（決定 15） | 完成（審核過） | 2026-11-02 |
| 3 `GcCompiler`：barrier、配置、spec test 和固定種子的 fuzz（決定 16） | 完成（審核過；elise 的 `5eb5ed7`，fork 的 `169069b139`） | 2026-11-02 |
| 4a kun 換成 elise（決定 17、18） | 完成（審核過；elise 的 `6e15a53`） | 2026-11-02 |
| 4a-2 elise 同步 kun 之後的 GC，4a 在現在的 `main` 上重做（決定 21） | 完成 | 2026-11-08 |
| 4b kun 用 fork 的 wasmtime 50.0.1（決定 17、19、20、21） | 完成 | 2026-11-08 |
| 4c runtime 的 `&mut` 和背景的 marker（決定 17、22） | 完成 | 2026-11-08 |
| 4d-1 elise 的外面的 root 的來源、`dom` 借出 heap（決定 23） | 完成（審核過；elise 的 `af880d1`） | 2026-11-08 |
| 4d-2 fork 的 provider、`StoreSet` 和它的 root 的來源、進出 wasm 的 frame 和檢查（決定 23、24、25） | 完成（審核過；elise 的 `2937ade`，fork 的 `56620a46a9`） | 2026-11-08 |
| 4d-3 `dom_wasm` 接上共用的 heap（決定 23、26） | 完成（審核過；fork 的 `ee4aab1477`） | 2026-11-08 |
| 4e heap 的上限和收尾（決定 17、27） | 完成（審核過） | 2026-11-08 |
| 5 驗收（`README.md` 的「分階段」，決定 28） | 完成（fork 的 `85976e0060`） | 2026-11-08 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 1a：執行時的型別的物件（struct、邊的陣列、資料的陣列）經過 scavenge 和完整的 GC 留下來、整個搬動，配置時是 0 | `../third_party/elise/crates/elise/tests/it/raw.rs`，每個 heap 每次 GC 之後跑 verifier |
| 1a：兩種 barrier 都在執行時的型別的邊上；Rust 的物件和它們互相指 | 同上；verifier 走執行時的型別的邊（`heap.rs` 的單元測試） |
| 1a：錯的 layout 登記不了，編號在自己的範圍裡只能登記一次，太大的陣列配置不了 | 同上 |
| 1a：退休的型別等每個 heap 做完一次退休之後才開始的完整 GC 才放掉、編號才能重用 | 同一份檔案在子程序裡單獨跑的三個測試；`types.rs` 的單元測試（不靠全域狀態） |
| 1b：struct 和陣列的邊裡的 i31 和物件並排，經過 scavenge 和完整的 GC 不變；marker、scavenge（包括經過 slot set 的）、verifier 都不把 i31 當物件 | `raw.rs`；每個 i31 的 bits 都落在一個死物件裡面，當成物件就會讀到不是 header 的地方，或留下死物件 |
| 1b：寫邊的兩種 barrier 都跳過 i31：新值不記 slot set、舊值不經過 SATB | `raw.rs`；`heap.rs` 的單元測試看 slot set |
| 1b：`I31` 的轉換和邊的 bits 照 wasmtime 的 `I31` 和 `VMGcRef` | `raw.rs` |
| 1c：frame 裡的安全點不開始 scavenge、full GC、marking 的開始和收尾（nursery 滿了配置到 old），離開 frame 之後的安全點照做；frame 裡的 `collect`、`scavenge` panic | `raw.rs` |
| 1c：交出 root 的 scavenge 和 full GC 留下 root 到得了的、寫回搬動之後的 bits，0 和 `I31` 不變；marking 在交出 root 時開始和收尾，SATB 留下 frame 從邊拿走的，verifier 從 frame 的 root 走 | `raw.rs`，每個 heap 開著 verifier；`heap.rs` 的 verifier 單元測試 |
| 1c：bits 的配置不 GC：GC 該做時回 `GcDue`、交出 root 的 GC 之後緊接著的那次成功，太大的陣列先回 `TooLarge`；bits 的寫經過兩種 barrier | `raw.rs` |
| 2a：wasm 的 GC 物件照它的型別配置、讀寫（struct 的欄位、array 的元素、`i31ref`、GC 參照、`externref` 的 host data），型別跟著 engine 的登記和放掉，pooling allocator 給下一個 Store 的 heap 是新的，太大的 array trap，Store 帶著 heap 換 thread，heap 的容量是 cage | fork 的 `tests/all/elise.rs` 的 `a_structs_fields_are_read_as_written`、`an_arrays_elements_are_read_as_written`、`an_externrefs_host_data_is_its_own`、`a_type_let_go_and_made_again_is_allocated`、`a_pooled_heap_is_the_next_stores_afresh`、`an_array_too_large_for_elise_traps`、`a_store_moves_to_another_thread_with_its_heap`、`the_heaps_capacity_is_the_cage`；型別表的單元測試（`-p wasmtime --lib elise::`） |
| 2b：GC 交出 Store 的 root（host 的、global、table）、留下它們到得了的，`externref` 死了之後 host data 還回去；配置要的 GC 是 elise 選的，embedder 的是 full GC；marking 在 host 的寫入之間一步一步跑 | fork 的 `tests/all/elise.rs` 的 `a_gc_keeps_what_the_roots_reach`、`an_externrefs_host_data_goes_with_it`、`globals_and_tables_hold_what_they_refer_to`、`an_allocations_gc_is_elises_the_embedders_is_full`、`a_pooled_heap_keeps_no_externref_of_the_last_store`、`a_marking_steps_between_the_hosts_writes`；上游的 GC 測試用 elise 跑（`machopo/elise_tests.py`，`elise` 和 `elise-stress`） |
| 3：產生的碼的寫入經過兩種 barrier（`struct.set`、`array.set`、`array.fill`、`array.copy`、大的 array 的初始元素），local 跟著 GC 搬動，global 和 table 的寫入留住它們指的，marking 在產生的碼的寫入之間一步一步跑、SATB 留下還沒標到的，配置照長度 | fork 的 `tests/all/elise.rs` 的 `compiled_writes_keep_the_young_objects_old_ones_refer_to`、`a_large_arrays_initial_elements_survive_a_scavenge`、`compiled_codes_locals_follow_the_objects_a_gc_moves`、`compiled_writes_of_globals_and_tables_hold_what_they_refer_to`、`a_marking_steps_between_compiled_writes`、`compiled_writes_keep_what_a_marking_hasnt_reached`、`compiled_code_allocates_an_array_of_its_length`；GC 型別的 spec test 用 elise 跑（verifier 和 stress，Cranelift 和 Pulley）；固定種子的差分 fuzz（`gc_ops_with_elise`、`gc_ops_elise_agrees`，對 copying collector） |
| 4a、4b：kun 換成 elise、換成 fork 的 wasmtime，行為不變 | `cargo test --workspace` |
| 4c：runtime 讀寫物件不建 `&mut`，背景的 marker 和 host 的寫入同時跑 | fork 的 `tests/all/elise.rs` 的 `a_marking_on_workers_runs_alongside_the_hosts_writes`（使用者在 Miri 底下跑，`README.machopo.md` 的「Tests」） |
| 4d-1：heap 之外的 root 的來源：每個讀 root 的 GC 都讀它、寫回，給不了時不 GC；`Heap` 上的 engine 函式；`HeapAlive`；在 frame 裡 drop 的 heap leak | elise 的 `tests/it/raw.rs`；kun 的 `dom` 的 `gc::the_heap_a_dom_lends_is_its_own` |
| 4d-2：Store 借 embedder 的 heap：embedder 的 GC 和每個 Store 的 GC 留下每個 Store 的物件，別的 Store 借出中時、Store 借出中 embedder 的安全點不 GC，沒有 GC heap 的 Store 的 GC，不在 set 裡的 Store 拿不到 GC heap，換了 thread、heap 走了、鑰匙換了 heap 時失敗，一個 heap 一個 set 的來源 | fork 的 `tests/all/elise/shared.rs` 的 `the_embedders_gcs_keep_every_stores_objects`、`a_stores_gcs_keep_the_other_stores_objects`、`no_gc_reads_the_roots_while_another_store_is_lent`、`the_embedders_safepoints_run_no_gc_while_a_store_is_lent`、`a_store_with_no_gc_heap_gcs_the_sets_heap`、`a_store_outside_a_set_gets_no_gc_heap`、`a_store_is_used_on_its_heaps_thread_only`、`a_store_whose_heap_went_fails`、`a_store_lent_another_heap_fails`、`a_heap_takes_one_sets_source` |
| 4d-3：`dom_wasm` 的 guest 和用 GC 型別的 Store 在同一個 heap：guest 的 DOM 工作在它的呼叫裡 GC、留下別的 Store 的物件，該 GC 時下一個會配置的 host 呼叫做，別的 Store 的 GC 留下 guest 握著的節點，別的 Store 借出中時不 GC，呼叫之間 DOM 的 full GC 兩邊都留下 | `dom_wasm` 的 `tests/it/shared.rs`；`tests/it/guest.rs`（guest 經過 set 呼叫） |
| 5：共用的 heap 的 concurrent marking 一次幾個物件時，兩個 Store 的產生的碼搬動、掛上、拿回物件，物件都在；產生的碼在 marker 還沒到的物件上剪掉的邊，只靠 SATB 留下 | fork 的 `tests/all/elise/shared.rs` 的 `a_marking_of_the_shared_heap_steps_between_compiled_writes`、`the_satb_barrier_on_the_shared_heap_keeps_what_compiled_code_cut` |
| 不涵蓋：和 DOM 之間經過 handle 的循環（要等 `externref` 指向 DOM 節點）；長時間的 cargo-fuzz（決定 28 第 2 點）；Miri（使用者跑）；上游的 stack switching 的 bug（kun 不開，不回報） | — |

## 做過的檢查

每一段都跑了改到的 repo 的測試、clippy、fmt 和 sanity，新的行為都跑了 mutation test（第 1 到 3 段 elise 和 fork 沒有 `test.py mutate`，用暫時的腳本照同樣的做法：先確認原本的程式通過，跑完和備份比對檔案；第 4、5 段用 kun 的 `test.py mutate`）。Miri 在第 1 到 3 段跑了新的 elise 的測試（沒有 UB），第 4 段起由使用者跑；fork 的 Miri 在 Windows 上編不起來（cargo 用 argfile 呼叫 rustc，cargo-miri 不支援），也交給使用者。長時間的 cargo-fuzz 沒跑（決定 28 第 2 點）。

### 1：elise 的新能力（2026-11-01，Windows 11）

- **1a 執行時登記的型別**：`tests/it/raw.rs` 17 個（3 個在子程序裡單獨跑）、`types.rs` 7 個、verifier 的單元測試 2 個。mutation test 44 個，第一輪 4 個沒抓到：verifier 少看執行時的型別的邊或 nursery（正確的 heap 少看也不失敗，補了故意弄壞 heap 的單元測試）、full GC 和 marking 開始時不記次數（另一個 heap 擋住了放掉，拆成兩個測試）。Miri 下查型別每次借出整張 2 MiB 的表，新的測試要半小時，改成照編號取那一格（決定 9 第 6 點）。
- **1b `i31ref`**：`raw.rs` 5 個、`heap.rs` 1 個；mutation test 18 個都抓到（marker、`evacuate`、verifier、兩種 barrier 各自不跳過 `I31`，`I31` 的轉換）。
- **1c 外面給的 root**：`raw.rs` 11 個、verifier 2 個；mutation test 27 個，verifier 不從 frame 的 root 走的 3 個第一輪沒抓到，補了單元測試。Miri 下填滿 nursery 的測試配置上萬個物件跑不完，改成配置 4 KiB 的陣列。

### 2：fork 的 runtime（2026-11-01 到 11-02，Windows 11）

- **elise**：`raw::EngineHeap`（`tests/it/engine.rs` 10 個；scavenge 和 full GC 對調的 mutant 第一輪沒抓到，補了看用量的測試）、`new_id`、`id_lives`（2 個，mutation test 3 個都抓到）。
- **fork 2a**：`tests/all/elise.rs` 9 個、型別表的單元測試 3 個；上游的 `structs`、`arrays`、`gc`、`exnrefs`、`eqref` 換成 elise（`machopo/elise_tests.py`）。mutation test 37 個，留給 2b、第 3 段的 6 個之後都抓到，等價的 3 個：detach 時不丟掉 elise 的 heap（attach 一定建新的）、不檢查參照在 cage 裡（host 的 API 給不出壞掉的參照）、`externref` 的 host data 寫在 `VMGcHeader` 的型別欄位上（沒人讀）。編 `--test all` 時才發現 Winch 也 match `Collector`。
- **fork 2b**：`elise.rs` 6 個（root 的交接和寫回、`externref` 的 host data 跟著死、global 和 table、配置要的 GC 和 embedder 的、pooling、marking 在 host 的寫入之間一步一步走）；`elise_tests.py` 加 `elise-stress`；mutation test 12 個都抓到。

### 3：`GcCompiler`（2026-11-02，Windows 11）

- **elise**：barrier state、`satb`、`remember`（`engine.rs` 3 個；mutation test 8 個，等價的 2 個：`satb` 不看旗標（`Inner::satb` 自己看）、`remember` 收下 `I31`（scavenge 跳過））。
- **fork**：`elise.rs` 8 個（四種寫入的分代 barrier、大的 array 的初始元素、local 跟著 GC 搬動、global 和 table、marking 在產生的碼的寫入之間、只有 SATB 留得住的黑的大 array、帶 `v128` 的 exception、64 MiB 以上的 array）；固定種子的差分 fuzz 2 個（`gc_ops` 200 個種子，對 copying collector）；`elise_tests.py` 加上 GC 型別的 spec test（Cranelift native 和 Pulley），預期失敗的清單清空。
- **找到的問題**：`v128` 的欄位讓共用的 layout 把 align 算成 16，elise 的物件只有 8 對齊，照 copying collector 寫成 8（決定 16）；隨機的 fuzz 設定多半沒開 GC，`gc_ops` 的 case 100 個只有 5 個編得過，固定種子的測試把它們打開。
- **mutation test** 18 個，第一輪 6 個沒抓到：SATB 的 3 個（marker 每一步都做完，碼寫的時候從不在半途，補了黑的大 array 的測試，第一版用 `move` 時「交給 SATB 的是新值」還是抓不到，改用初始化）、exception 的 align、`alloc_raw` 把位元組數當長度；等價的 2 個（寫入 `I31` 時不跳過，`remember` 自己再查；格子直接用位址的低 32 位元，cage 對齊 4 GiB）。

### 4：和 DOM 共用 heap（2026-11-02，Windows 11；2026-11-08，Linux）

- **4a、4b 第一次**（Windows）：kun 換成 elise、換成 fork 的 wasmtime 50.0.1；4b 時舊的 `dom_wasm` 碰到上游的 `deferred host scope has no call context`，還原、暫停（決定 20）。
- **4a-2**（決定 21）：elise 和 kun 分開之後的兩邊用三方合併同步（3 處衝突、3 處語意上的接合），補了走訪執行時的型別的測試（mutation test 3 個都抓到）；kun 的 8 個 crate 改用 elise、刪掉 `crates/gc` 和 `gc_macros`。`Cargo.lock` 變了之後 `skia-bindings` 要重新下載，GitHub 一直回 504，這台用之前解開的同一份重新打包、`SKIA_BINARIES_URL` 指向它。Miri 下 `alloc::the_fast_paths_local_drops_run_on_the_heaps_thread` 等 worker 的上限 10 秒不夠，改成 600 秒。
- **4b-2**：換成 fork 的 50.0.1，`dom_wasm` 不用改程式。
- **4c**（決定 22）：`VMGcObjectData` 換成 cell，先看過 runtime 經過它的每個寫入（GC 參照都經過 `write_gc_ref_in`、`init_gc_ref_in`，`array.copy` 這些由 JIT 做）；測試是背景的 marker 和 host 的寫入同時跑；mutation test 6 個，少了 `gc::` 時整段複製的 2 個沒抓到，加上之後都抓到。
- **4d-1**（決定 23）：elise 的 heap 之外的 root 的來源（`raw.rs` 7 個；mutation test 20 個，`stress` 下 marking 收尾算成到了上限的沒抓到，補了檢查；driven 的 heap 加 `stress` 的步數是等價的，改成那樣）；kun 的 `Dom::heap_mut`。
- **4d-2**（決定 24、25）：寫的時候發現照決定 23、24 記憶體安全會靠 kun 的 safe 的程式，改成 fork 和 elise 自己守（決定 25）：`StoreSet`、`EliseHeapProvider`、借出時的 frame、thread 和存活的檢查，elise 的 `HeapAlive`、`unsafe` 的 `leave_frames` 和 `set_roots_source`、在 frame 裡 drop 時 leak。一開始用 cage 的位址認 heap，舊的 drop 之後新的可能 map 在同一個位址，改用存活記號。測試：fork 的 `tests/all/elise/shared.rs` 9 個、elise 4 個；mutation test 27 個，等價的 2 個（借 heap 的也自己做一個 `EngineHeap`；`satb_in` 沒有 marking 也做：old 的物件在兩次 marking 之間都標著，試過剛配置的、scavenge 升上去的、GC 之後的）。`resume_fiber` 的檢查是 abort，測不到，只靠審查（fork 的 `README.machopo.md` 的「Tests」）。
- **4d-3 之前**：`elise-stress` 下 stack switching 的兩個失敗是上游 v50.0.1 的 bug：一串兩層以上的 continuation 暫停時 GC，走那串 stack 時 panic；DRC 在同一個時間點從 host 呼叫 GC 也一樣。kun 不開 stack switching，使用者決定不回報。
- **4d-3**（決定 26）：`dom_wasm::configure`、`WasmDom::store_set`、host 函式之後經過 Store 的 GC，fork 的沒有 GC heap 的 Store 的 GC；`dom_wasm` 的 `tests/it/shared.rs` 5 個、fork 1 個；mutation test 9 個，成員的和 `create-document` 的 GC 互相掩護、provider 借錯 DOM 只差在錯誤訊息，補了測試之後都抓到。
- **4e**（決定 27）：不加上限；第 4 段做完時 `cargo test --workspace` 3694 個通過，`cargo fmt --check`、clippy 沒有問題。

### 5：驗收（2026-11-08，Linux，決定 28）

驗收表補齊；fork 的 `shared.rs` 加 2 個共用的 heap 在 marking 中的測試（marker 一步 64 個物件，隨機的搬動、掛上、拿回，和只有 SATB 留得住的確定的情境）。mutation test 4 個（產生的碼的兩種 barrier、兩個 libcall），只用這 2 個測試都抓到；第一版 marker 每一步都做完、只有隨機的操作時，SATB 的 2 個沒抓到。
