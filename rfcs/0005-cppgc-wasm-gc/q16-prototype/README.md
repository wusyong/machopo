# cppgc 上的 wasm GC 物件：原型（Q16）

2026-10-04 做的原型，不合併。要回答 `../../0001-wasm-scripts/wasmtime-questions.md` 的 Q16：wasm 的 struct 和 array 能不能直接是 cppgc 上的物件（不定長度、`trace()` 照型別表走），和 DOM 節點互相指向時一起回收，incremental marking 中的寫入不漏標。不碰 wasmtime。

## 做法

1. **參照欄位就是 cppgc 的 `Member`**（壓縮指標），trace 和寫入 barrier 用 cppgc 現成的；`anyref` 欄位用最低位元 1 當 i31 的 tag（`value << 1 | 1`）。不選「相對於 cage 基底的位置」（wasmtime 現在 `VMGcRef` 的樣子）：trace 和 barrier 都要自己換算。
2. **wasm 物件是 rusty_v8 fork 新加的 C++ 類別 `WasmGcObject`**（`v8::cppgc::wasm_gc`，`README.machopo.md` 的「wasm GC objects and a weak table」）：header 只有型別索引和長度（8 byte），欄位接在後面，在建構子裡清成 0（concurrent marker 不會讀到沒寫過的 byte，0 是 null 的 `Member`）；`Trace` 在 C++ 裡照 Rust 登記的型別表（`set_types`）走，不用每個物件跨一次 FFI。參照欄位分三種，各自寫明目標的類別，cppgc 靜態地 trace：`Ref`（指向 wasm 物件）、`HostRef`（指向 Rust 的 GC 物件，例如 DOM 節點，經過 `RustObj` 的 virtual `Trace`）、`AnyRef`（i31 或 wasm 物件）。
   - 第一版用 rusty_v8 的 `RustObj` 加上 Rust 新加的「帶尾巴」配置（不用重編 V8），每個物件多 40–50 byte，從 `&self` 讀尾巴也是 Stacked Borrows 不允許的寫法；改成 C++ 物件之後兩個問題都沒了，Rust 的那些 API 也拿掉了。
3. **barrier 用不接 isolate 的 `CppHeap` 測**（testing API 一步一步推進 marking），**自動回收用單獨的 `cppgc::Heap`**（Q18 的 `StandaloneHeap`）；DOM 節點用原型自己的型別（在 cppgc 看來和 `dom` 的節點一樣，是 Rust 的 GC 物件）。

`src/wasm.rs`：型別表（struct 有 `i32`、`f64`、`(ref null $t)`、`anyref`、DOM 節點的欄位；`anyref` 的 array；只有數字的 struct）和存取欄位的函式；每個物件帶一個 `id`（只給檢查用：struct 放在欄位裡，array 放在第 0 個元素的 i31）。`src/main.rs`：檢查。

**怎麼知道誰活著**：wasm 物件沒有 Rust 的 finalizer，所以用兩種方式：可達的物件內容（header 的型別和長度、`id`、每個欄位）都要和影子圖一致，被回收的物件開頭會被 cppgc 的 free list 蓋掉、或記憶體被重用；一張從不進入的 `WeakObjectTable`（Q15 的表）記下所有物件，強制 GC 之後剩下的筆數要剛好等於可達的物件數，平常也不能比可達的少。節點是 Rust 物件，另外記下被 finalize 的 id。

## 跑法

要 fork 裡還沒發布的改動（C++）：先在 machopo 的根目錄 `python3 scripts/ci.py v8 --profile release`，再連結編好的程式庫：

```sh
RUSTY_V8_ARCHIVE=../../../../third_party/rusty_v8_artifacts CARGO_TARGET_DIR=../../../target cargo run --release -- all [seed]
```

和 machopo 共用 target 目錄時，machopo 平常的 build（沒有 `RUSTY_V8_ARCHIVE`）會把 `target/release/gn_out/obj/librusty_v8.a` 換回 mirror 的版本，之後這裡連結時找不到 fork 新加的 symbol；`touch ../../../../third_party/rusty_v8/build.rs` 讓 `v8` 的 build script 重跑一次就好。

## 結果（2026-10-04，macOS，Apple M4，release）

- **`cycles`**：兩個 struct 互相指向、struct 指向節點、節點（像 listener）指回 struct、array 裡同時有物件和 i31（0、-1、1、i31 的最大和最小值）。GC 之後全部照原樣讀回來；拿掉 root 之後一次 GC 四個都回收。
- **`fuzz-incremental`**：一張隨機的圖（骨架是 8 個各 4,096 格的 `anyref` array，加上 struct、array、只有數字的 struct、節點），大約 2.4 萬個可達的物件；40 次 incremental GC，**每個 marking 步驟之前做 50 次改寫**（接上、剪斷、i31 和物件互換、新物件、移動 root），一共約 920 個步驟。每次 GC 之後，可達的物件全部完好；最後強制 GC 兩次，剩下的剛好是可達的那些。
- **`fuzz-concurrent`**：一樣，但 marking 全部在 concurrent marker 的 thread 上（主 thread 不做 marking，收尾前才打開），4,000 個步驟。通過。
- **`fuzz-standalone`**：一樣的圖放在 `StandaloneHeap` 上，不呼叫 GC，4,000 批改寫，每批之後執行 cppgc 排進來的 task（約 4 萬個）。通過。
- 四個 seed（預設、1、2、3）都通過。
- **測試抓得到錯**：`anyref` 寫入物件時不經過 barrier（直接寫壓縮值），`fuzz-incremental` 失敗（可達的物件被回收）。第一版（`RustObj`）也確認過 `trace` 不走 `anyref` 欄位會失敗。
- **`size`**：100 萬個物件、用 RSS 量：4 個欄位（16 byte）的 struct 約 33 byte；8 個 `anyref` 的 array（32 byte）約 49 byte。也就是 cppgc 的 header（8）、`WasmGcObject` 的 header（8）、欄位，對齊到 16。wasmtime 的 DRC 的 header 是 16 byte。第一版經過 `RustObj` 是 82 和 98 byte。

## 發現

1. **machopo 的 cppgc 壓縮指標的 shift 是 3，不是 1**：GN 的 `cppgc_enable_larger_cage` 預設打開（`!is_ios`），cage 是 16 GiB，壓縮值是 `地址 >> 3`。8 byte 對齊的物件，壓縮值的最低位元可能是 1，和 i31 的 tag 撞在一起（原型一開始就是這樣錯的：寫進去的物件讀回來變成 i31）。**修法：wasm 物件一律 16 byte 對齊**（`WasmGcObject` 是 `alignas(16)`，cppgc 支援到 16），最低位元就一定是 0；代價是每個物件最多多 8 byte。另一個做法是關掉 `cppgc_enable_larger_cage`（shift 1、cage 4 GiB），整個 process 的 cppgc heap 就只有 4 GiB，不選。
2. **`anyref` 的 trace 只能讀一次**：heap 的 thread 隨時可能把 i31 寫到原本是指標的欄位上，而 cppgc 的 `Visitor::Trace(member)` 會自己再讀一次 slot。所以 `Trace` 先原子地讀一次，是指標的話把讀到的值放進 stack 上的一個 `Member` 再交給 cppgc（強參照的 trace 不記 slot 的位置）。i31 的 0 編碼成 1，正好是 cppgc 的 sentinel，所以 i31 絕不能交給 cppgc。
3. **寫入**：物件和 null 用 `Member` 的指派（原子的寫入加上 barrier）；i31 是普通的原子寫入，不需要 barrier（cppgc 的 barrier 是插入式的，蓋掉舊的指標不需要）。只有數字的欄位只有 heap 的 thread 讀寫，marker 不看。
4. **`Member` 要寫明目標的類別**：一開始想用 `Member<GarbageCollectedMixin>` 指向任何 GC 物件（cppgc 從物件的 header 找到它的 trace），結果 segfault：這條路要靠「物件起點的 bitmap」找 header，而 cppgc 只替 mixin 類別的物件設那個 bit（`MayContainMixins`）。`WasmGcObject`、`RustObj` 都不是 mixin，所以參照欄位按目標分成 `Ref` 和 `HostRef`。
5. **young generation 一定要關著**：machopo 的 V8 編進了 `CPPGC_YOUNG_GENERATION`（執行時預設不開）。開了之後，寫入 barrier 會記下 slot 的位置、之後當成指標讀回來，放著 i31 的 `anyref` 欄位會被讀錯。
6. **debug 版的 V8 還沒試**：`anyref` 的 trace 在 marker thread 上建立一個暫時的 `Member`，debug 版的 V8（`CPPGC_ENABLE_SLOW_API_CHECKS`）可能不接受；之後用 `test.py matrix` 的 debug V8 跑。

還沒做：和真正的 `dom` 節點互相指向（`dom` 還不能放在 `StandaloneHeap` 上，見 Q18）。
