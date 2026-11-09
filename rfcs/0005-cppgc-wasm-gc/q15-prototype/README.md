# Store 執行中的 GC：弱參照表的原型（Q15 的退路）

2026-10-04 做的原型，不合併。要驗證 `../../0001-wasm-scripts/wasmtime-questions.md` 的 Q15 在有 V8 的設定裡的退路：V8 隨時可能 GC（wasm 分派給 JS 的 listener、JS 配置），wasm 的 frame 可能還在 stack 上，而 cppgc 不認得 wasm stack 上的參照。退路是**正在執行的 Store，這次 GC 把它配置過的物件全部當成活的**，只回收沒在執行的 Store 的垃圾。

## 做法

rusty_v8 fork 新加的 C++ 類別 `WeakObjectTable`（`v8::cppgc::wasm_gc`，`README.machopo.md` 的「wasm GC objects and a weak table」），每個 Store 一張，由 Store 的 holder（RFC 的 `StoreHolder`）指著：

- **記什麼**：Store 配置的 wasm 物件（`add_wasm`），和 **host 交給 Store 的物件**（`add_host`，例如 DOM 節點）。每筆是 4 byte 的壓縮指標，wasm 物件和 host 物件分兩個 vector（trace 時要知道目標的類別）。
- **Store 沒在執行時是弱的**：trace 只登記一個 weak callback，GC 收尾時刪掉死的（和重複交進來的 host 物件）。
- **執行中是強的**（`enter`、`exit`，可以巢狀）：trace 走過整張表；在 concurrent marker 上時延到主 thread 做（`DeferTraceToMutatorThreadIfConcurrent`），表只在主 thread 上被碰。
- **marking 進行到一半才進入**：這次 GC 已經弱地 trace 過這張表時，用 Steele barrier 讓它再被 trace 一次（同一次 GC 只做一次）；執行中新增的物件用 Dijkstra barrier 馬上標記。

`src/main.rs` 模擬：3 個 Store，各有 holder（表和 8 個 global，代表 wasm 自己的 root：table、global）；一疊執行中的 frame，各有一個 GC 看不到的「wasm stack」（Rust 的 `Vec`）；一份文件（DOM 節點）。執行中的 Store 配置物件、讀寫欄位、把物件從 heap 取出到 stack 上（讀出再清掉那一格）、從 host 拿到節點、設 listener、呼叫另一個 Store（巢狀）、回傳。另外有 10 萬個物件的 ballast，每個 Store 的前 4 個 global 各指向 1,024 個物件的 array，讓 marking 要走很多步。物件那一層用 Q16 的 `wasm.rs`。

GC 一步一步推進（不接 isolate 的 `CppHeap`，testing API），每兩步之間做 0–40 個操作，在任意時刻收尾（**大多數時候有 Store 正在執行**），收尾時不掃描 stack。之後：從文件、所有 global、**每個 frame 的 stack** 能到的物件都要完好（header 的型別、`id`、每個欄位，節點沒被 finalize）。最後所有 frame 回傳、強制 GC 兩次，每個 Store 的表剛好剩下它配置的活物件加上交給它的活節點，觀察用的表剛好剩下可達的物件。

## 跑法

```sh
RUSTY_V8_ARCHIVE=../../../../third_party/rusty_v8_artifacts CARGO_TARGET_DIR=../../../target cargo run --release -- all [seed]
```

（fork 的 C++ 還沒發布：先在 machopo 的根目錄 `python3 scripts/ci.py v8 --profile release`；連結找不到 symbol 時見 `../q16-prototype/README.md` 的「跑法」。）

## 結果（2026-10-04，macOS，Apple M4，release）

- **`fuzz-incremental`**（主 thread marking）、**`fuzz-concurrent`**（marking 全在 marker thread 上）：各 300 次 GC，278–284 次在 Store 執行中收尾，每次 30 個 marking 步驟；約 4.8 萬個物件、最多約 1.2 萬個可達。六個 seed 都通過。
- **`fuzz-unrecorded`**：host 交給 Store 的節點不記進表，**失敗**：只在 Store 的 stack 上的節點被回收了。所以表不只要記 Store 配置的物件，也要記交給它的 host 物件。
- **mutation test**（暫時改 fork 的 C++、重編，再還原）：
  - 拿掉 `enter` 的 Steele barrier：一開始沒抓到。程式自己的圖太小，第一個 marking 步驟就全部標記完，「從還沒 trace 的物件裡取出、只留在 stack 上」不會發生；加了每個 Store 的大 array 和「取出」操作之後，8 次裡 6 次失敗。
  - 拿掉 `add` 的 Dijkstra barrier：`fuzz-incremental` 4 次都失敗；`fuzz-concurrent` 抓不到，因為主 thread 不做 marking 時，延到主 thread 的強 trace 到收尾才做、正好把新物件也標記了（實際的 V8 主 thread 也做 marking，這個情況由 `fuzz-incremental` 抓）。
  - trace 永遠是弱的：7/8 失敗。
- **`cost`**：
  - 配置一個 struct 6.5–6.6 ns，加上記進表 7.8–8.2 ns（多約 1.5 ns）。
  - `enter` 加 `exit` 3.5 ns（沒在 marking 時）。
  - 有 1 萬、10 萬筆的表（加上 10 萬個物件的 ballast）的 GC：marking 約 1.0 ms、1.9 ms，收尾 0.17 ms、0.42 ms；marking 中途進入（多 trace 一次表）多 0.01–0.1 ms。
  - 每筆 4 byte，加上 vector 的空間。

## 發現

1. **退路成立**：Store 執行中的 GC 不需要知道 wasm stack 上有什麼，代價是每次配置多記一筆（約 1.5 ns、4 byte），和執行中的 Store 的垃圾等到下一次 GC。
2. **host 交給 Store 的物件也要記**（DOM 節點、之後的 `externref`）：否則 wasm stack 上拿著的節點，在它從文件上被拿掉之後就可能被回收。`machopo:dom` 每個回傳節點的函式都要做；同一個節點交進來很多次，表在 GC 收尾時去掉重複的。
3. **兩個 barrier 都需要**：marking 中途進入的 Store 要讓表再被 trace 一次（Steele），執行中新配置的物件要馬上標記（Dijkstra）。
4. **測試要讓「物件從 heap 搬到 stack」常常發生**，而且 marking 要慢到還沒走到那些物件：這是 Steele barrier 的 mutation 一開始沒被抓到的原因。
