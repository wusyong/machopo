# RFC 0072 的進度：原地建的 persistent

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-22。完成：2026-10-22，Windows 11（Intel Core i9-13900K；Rust 釘在 `rust-toolchain.toml` 的版本）。在 `0072` 上做，三段各一個 commit，加上量測之後的一個修正；rusty_v8 fork 多了兩個 commit（`759eedc` 的 `WeakPersistentSlot`，發了 `kun-v152.2.0-17`；`b9667c5` 只改格式，kun 的 `third_party.toml` 用 `rev` 指過去）。整個過程連本機 `ci.py v8` 編的 V8（`RUSTY_V8_ARCHIVE`）做。release 的 workflow（GitHub Actions 的 run 39717841819）五個建置都成功（Windows 的 release 1 小時 50 分鐘最慢）；merge 之前拿掉 `RUSTY_V8_ARCHIVE`、下載 `kun-v152.2.0-17` 的 Windows prebuilt，`cargo test -p bindings -p dom` 都通過（`bindings` 的 unit 2、兩個獨立的 binary 各 1、`tests/it` 34；`dom` 的 unit 11、`compile_fail` 1、`incremental_stress` 2、`tests/it` 395）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 fork：`WeakPersistentSlot`、`bindings` 的測試，發 `kun-v152.2.0-17`，kun 換過去（第 2、5 條） | 完成 | 2026-10-22 |
| 2 id 表：`NodeIdMap` 改成 256 格一塊的 slab（第 3、6 條） | 完成 | 2026-10-22 |
| 3 所有型別的 root 的 spare pool（第 1、4、7 條） | 完成 | 2026-10-22 |
| 量測之後：id 表的空格子從編號最小的發（第 6 條） | 完成 | 2026-10-22 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| fork 的 slot | 空的照值建、可以搬；設、改設、清掉、再設；目標死掉時 GC 清掉它、活著的照舊；chunk 的每一格經過 `index` 都是自己的 root；drop 之後 node 還回去（GC 不再碰那塊記憶體） | `cargo test -p bindings --test it -- cppgc_weak_persistent` |
| id 表 | 每個節點一格 slot，prune 之後被回收的節點的格子重用（再建同樣多個，chunk 不增加）；prune 的時機照舊（長到上次的兩倍）；`node_by_id`、`handle_by_id` 照舊 | `cargo test -p dom --test it -- tree::the_id_tables_slots_are_reused tree::the_id_table_is_pruned_as_it_doubles`；`dom` 的其他測試（`node_by_id`、GC） |
| root 的 pool | 節點和 collection 以外的型別也重用放掉的 root（反覆 root 只建一個 `Persistent`）；其他型別最多留 64 個、節點留 4096 個；拆掉 isolate 時三處的 cell 都放掉、`Roots` 被釋放 | `cargo test -p dom --test it -- tree::every_types_roots_are_reused tree::other_types_keep_64_spare_roots tree::nodes_keep_more_spare_roots`；`cargo test -p dom --lib -- handle::tests::the_roots_go_with_the_isolate`（debug build 才計數） |
| 效能 | Windows 上 persistent 的建立和刪除不再經過 NT heap；id 表發的格子照位址的順序 | `bench.py speedometer --sample`（Windows，要使用者要求才量；下面「做過的檢查」的量測） |

## 做過的檢查

**完成時（2026-10-22，Windows 11，i9-13900K，連本機 `ci.py v8` 編的 V8）**：`cargo fmt --check`（連同 `sdk/lua/generator`）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --no-fail-fast`（45 個測試 binary，3537 個通過、3 個略過，5 分 37 秒）和 `sanity.py` 的 `deps`、`boundary`、`unsafe`、`tests`、`docs`。這三項在第 1、2 段做完時漏跑了，三段一起跑：fmt 抓到 kun 的三處、fork 的 `slot.rs` 兩處（照 fork 的 `.rustfmt.toml`），kun 的併進第 3 段的 commit，fork 的是 tag 之後的 `b9667c5`（只改 Rust，不用發 release）。

**本機編 V8**：`ci.py v8 --profile release` 第一次在 Windows 上跑。條件：fork 的 submodule、`tools/win`、Visual Studio Community 2026（Chromium 的 `vs_toolchain.py` 認得）、Windows SDK 10.0.26100；clang 和 bindgen 的 libclang 由 `build.rs` 下載 Chromium 的版本。從零開始 15 分鐘（ninja 3272 步，經過 sccache），之後改 `binding.cc`、`binding.hpp` 約 20 秒。第一次失敗在 V8 編完之後的 bindgen：`binding.hpp` 沒有 include `cppgc/persistent.h`。

**找到的、改掉的**：
- persistent 是 32 bytes，不是草稿寫的 16：cppgc 的 object names 在桌面平台預設開著，連 release 也帶 `SourceLocation`（`cppgc__WeakPersistent_SIZE`）。README、`details.md` 的數字跟著改，設計不受影響（大小從 build 讀）。
- 「drop 之後 node 還回去」原本沒有測試抓得到（漏還的 node 只會在之後的 GC 寫進已釋放的記憶體）：測試改成 drop 之後保留那塊記憶體、在 `raw_` 寫一個將死的物件的位址再 GC，node 還在的話 GC 會把它清成 null。
- `NodeIdMap::get` 對被回收的節點回傳 `None` 的檢查是多餘的（`node` 讀到空的 slot、`NodeHandle::try_new` 看到 null 都給 `None`；原本的 `WeakPersistent` 版本也有），mutation test 抓不到，拿掉。
- 節點和 collection 的 pool 的上限（4096）原本沒有測試守著，補了 `nodes_keep_more_spare_roots`。
- id 表的 free list 原本後進先出，量了之後改成從編號最小的發（下面的量測）。

**mutation test**（`mutants-2.toml`、`mutants-3.toml` 用 `test.py mutate`；fork 那一段 fork 還有沒 commit 的改動，`ci.py third-party` 把它留著，`test.py` 不跑，用一個照同樣做法的腳本）：
- fork 的 `slot.rs`：drop 不還 node、`set` 不做事、`clear` 不做事、讀取永遠是 null、`index` 永遠是第一格，5 個都被 `cppgc_weak_persistent` 抓到。
- id 表：prune 不放回格子、prune 掉活著的項目、`insert` 不設值、讀到下一格、新 chunk 的第一格發兩次，5 個都抓到。等價的：`clear` 不丟 chunk（`DomIsolate` 的欄位順序本來就讓 id 表在 heap 之前 drop，記在 `docs/test-gaps.md`）。
- root 的 pool：不從表裡拿、不放進表、其他型別多留一個、少留一個、節點和 collection 也用 64 的上限、拆掉時不取出表（`Roots` 洩漏，unit test 抓到），6 個都抓到。
- free list 不排序只會變慢，計數守不住，記在 `docs/test-gaps.md` 的「沒有計數守著的優化」。

**量測（2026-10-22，Windows 11，i9-13900K；使用者要求的）**：`bench.py speedometer --sample`，8 個 suite 各 5 個 iteration，和當天早上（kun `f06bce27`、release 的 prebuilt）比。中間還 merge 了 RFC 0071、V8 換成本機編的，各 suite 的總時間不能全算在這份；NT heap 裡照呼叫者分出來的那幾行只和這份有關。每個 iteration 的 ms：

| NT heap，照呼叫者分 | 早上 | 三段做完 |
| --- | ---: | ---: |
| `WeakPersistent` 的 `CONSTRUCT`（id 表） | 13.1 | 0.0 |
| `Persistent` 的 `CONSTRUCT`（root） | 7.8 | 1.3 |
| root 的 `free` | 5.1 | 0.0 |
| Skia | 5.4 | 8.0 |
| V8 和 cppgc 自己的 | 3.9 | 7.1 |
| 合計 | 36.8 | 19.3 |

| suite 的 CPU 時間（main thread，計時的區間） | 早上 | 三段做完 | free list 排序之後 |
| --- | ---: | ---: | ---: |
| TodoMVC-jQuery | 193.2 | 213.6 | 190.9 |
| Charts-chartjs | 128.2 | 124.3 | |
| Perf-Dashboard | 126.9 | 126.1 | |
| NewsSite-Next | 125.6 | 125.1 | |
| Editor-TipTap | 116.0 | 121.6 | |
| NewsSite-Nuxt | 97.0 | 96.3 | |
| React-Stockcharts-SVG | 87.2 | 91.6 | |
| ES6-Webpack-Complex-DOM | 67.2 | 61.9 | |

- 三段做完時 persistent 的配置照預期消失了，但 jQuery 多了 20 ms：id 表設值的 `BasicPersistent::Assign` 自己佔 26.9 ms，每個節點約 87 ns，是 cache miss 的時間。格子在 prune 時照 hash map 的順序放回 free list，後進先出等於在整張表裡隨機發格子。prune 之後排序、從最小的發，`Assign` 剩 0.4 ms，jQuery 190.9 ms（NT heap 13.9 → 1.2 ms，innerHTML 建節點的 `html` 83.2 → 68.5 ms）；排序在取樣裡看不到。其他 7 個 suite 沒有用排序之後的版本再取樣（它們建的節點少得多）。
- Charts-chartjs −3.9、ES6-Webpack −5.3 是 root 的 pool 和 id 表；TipTap、Stockcharts 的增加在 layout 和 style，Skia 和 V8 自己的 NT heap 時間的增加也是，和這份無關，混著 RFC 0071 和 V8 的來源不同，沒有拆開。
