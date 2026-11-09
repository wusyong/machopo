# wasm GC 的 lowering：原型（Q14）

2026-10-04 做的原型，不合併。要回答 `../../0001-wasm-scripts/wasmtime-questions.md` 的 Q14：cppgc 當 wasmtime 的 collector 時，`wasmtime-cranelift` 要產生什麼樣的程式碼，和現在的 collector 比成本如何。

`src/main.rs` 用 wasmtime 49.0.2 把一個小的 wasm GC module（`struct.get` 一個 i32 和一個參照欄位、`struct.set` 一個參照欄位、`array.get` 一個 `anyref` 元素、`struct.new`）分別用 null、DRC、copying 三種 collector 編成這台機器的程式碼，再用 `llvm-objdump` 反組譯：

```sh
CARGO_TARGET_DIR=../../../target cargo run --release
```

## 現在產生的程式碼（arm64，Apple M4）

**null collector 的 `struct.get`**（核心部分）：

```
cbz  w4, trap            ; null 檢查
ldr  x7, [x2, #0x8]      ; VMStoreContext（可以移出迴圈）
ldr  x7, [x7, #0x20]     ; GC heap 的基底（可以移出迴圈）
add  x7, x7, w4, uxtw    ; 基底 + 32 位元索引
ldr  w2, [x7, #0x8]      ; 欄位
```

沒有邊界檢查：GC heap 的保留區加上 guard 區夠大，32 位元的索引怎麼都不會跑出去。`struct.set` 一樣，最後是 `str`，沒有 barrier（null 和 copying 都不需要：一個不回收、一個 stop-the-world）。

**DRC 的 `struct.set`** 多了十幾個指令：新值的 ref count 加一、舊值的 ref count 減一，減到零時呼叫 `drop_gc_ref`。DRC 的讀取也有 barrier（放進 over-approximated stack roots）。

**配置**：null 在 JIT 裡直接 bump 配置（約 25 個指令，不夠時呼叫 `grow_gc_heap`）；DRC、copying 呼叫 `gc_alloc_raw` libcall。

**`array.get`**：讀長度、和索引比較、幾個溢位檢查，再算位址。

## cppgc 版要的東西

1. **物件格式**：header 就是 wasmtime 的 `VMGcHeader`（kind 和 `VMSharedTypeIndex`，8 byte），array 的長度在第 8 byte，和 null collector 的 layout 一樣，只是 header 對齊到 16（i31 的 tag，Q16）。`ref.test`、`ref.cast` 是共用的程式碼，照 `VM_GC_HEADER_*` 的位置讀 header，所以不用改。i31 的編碼（`value << 1 | 1`）和 null（0）也和 wasmtime 一樣。
2. **參照換成位址**：wasmtime 假設 GC 參照是 heap 裡的 byte 索引（`base + zext(ref)`），cppgc 的參照是壓縮指標。cppgc 的 cage 對齊到 16 GiB，基底存的時候低 34 位元全是 1，解壓縮是 `(sext(ref) << 3) & base`；arm64 上兩個指令（`sbfiz`、`and`），x64 三個。對合法的指標它等於 `(cage 基底 − 16 GiB) + (zext(ref) << 3)`，arm64 的 `add Xd, Xn, Wm, UXTW #3` 和 x64 的 `lea` 一個指令就做得到，**和現在的 `add` 一樣便宜**。差別是 AND 的版本保證算出來的位址在 cage 裡，ADD 的版本對壞掉的參照不保證；照「已決定的事項」第 15 條，至少要留在 cage 裡，所以用 AND，多一個指令。兩種都不需要邊界檢查。
   - 要改的地方（共用的程式碼）：`FuncEnvironment::prepare_gc_ref_access`、陣列批次操作算基底的地方（`CheckedEntity::Array`）、`emit_defensive_array_bounds_check`（heap 的基底和大小換成 cage 的）；runtime 那邊 `GcHeap` 用 `heap_index()` 的預設方法（`header`、`index`、`object_range`、`gc_object_data`）要在 cppgc 的 `GcHeap` 裡改寫成解壓縮。
3. **寫入 barrier**（`translate_write_gc_reference`）：寫入之後，讀 cppgc 的全域旗標（`WriteBarrier::write_barrier_enabled_`，process 共用；它的位址放在 `VMStoreContext`，載入可以移出迴圈），沒在 marking 時就結束：**比 null 多兩個指令**（`ldr`、`cbnz`），比 DRC 少很多。在 marking 時呼叫 libcall：值是 null 或 i31 就返回（i31 交給 cppgc 會被當成指標，Q16），否則 `WriteBarrier::CombinedWriteBarrierSlow(slot)`。初始化的寫入（`struct.new` 的欄位）不需要 barrier：新物件在 marking 中是白的，之後被 trace 時會看到那些值。
4. **讀取**：沒有 barrier（cppgc 沒有讀 barrier），和 null 一樣。也不需要 stack map：GC 不是 wasmtime 發起的，stack 上的參照由安全點或 Q15 的表處理；共用程式碼宣告的 stack map 可以關掉，省掉為它們做的 spill。
5. **配置**：`gc_alloc_raw` libcall（和 DRC 一樣），runtime 那邊呼叫 cppgc 配置 `WasmGcObject`。cppgc 的快速路徑（linear allocation buffer）在 C++ 裡，JIT 拿不到，所以不能像 null 那樣在 JIT 裡 bump。Q15 量過經過 Rust 配置一個 struct 約 6.5 ns，加上 libcall 的進出。
6. **型別表**：`WasmGcObject` 的 `Trace` 照型別找參照欄位（Q16），型別索引是 `VMSharedTypeIndex`，所以 engine 登記型別時要把它的 layout 也登記進 C++ 的表。

## 還沒決定

- **`externref` 指向 DOM 節點**：wasmtime 把 `externref` 包成一個 GC 物件（`VMExternRef`，加上 host 資料的 id）。machopo 可以照做（交給 wasm 的每個節點多一個小物件），或直接指向節點；後者的話，節點經過 `any.convert_extern` 會進到 `anyref` 欄位，所以 DOM 物件也要 16 byte 對齊。等第二層實作時和 GC 版的 `machopo:dom` 一起決定。
- **配置的成本**：libcall 的進出加上 cppgc 的配置，和 null 的 bump、DRC 的 libcall 比，要等真的接上 wasmtime 才能量。
