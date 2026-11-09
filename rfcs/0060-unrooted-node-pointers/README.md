# RFC 0060：不 root 的節點指標：scope 裡的裸指標，GC 延到 scope 之間

- 開始日期：2026-10-19
- 狀態：✅ 已完成：2026-10-19（只讀的節點不再建 root：新 wrapper 的 realm 在 read scope 裡找，插入、移除表單元素先用讀取判斷有沒有事要做；集合的 root 也重用 cell；TodoMVC-jQuery 的取樣 cycle −3.2%、指令 −4.7%。不 root 的節點指標 `NodePtr<'b>`（branded scope 加 cppgc 的 no-GC scope）做完、量過，只多省約 1–2% 的 cycle，卻要一處依賴 V8 private 成員的 fork 改動，先不進 `main`，程式碼在 branch `0060-node-ptr`；在 Linux 上做）
- 進度和驗收：`progress.md`
- 細節：`details.md`（cppgc 和 V8 的 GC 入口、kun 的借用結構、handle 的成本和 API 的範圍）

## 摘要

`dom` 的操作只要把節點交出 scope，或配置時要撐住它，就要一個 root（`NodeHandle`，背後是 cppgc 的 `Persistent`），jQuery 的取樣裡 handle 本身約 3%。這份 RFC 原本要加一個不 root 的節點指標 `NodePtr<'b>`：只在 closure 式的 branded scope 裡拿得到的裸指標，closure 期間打開 cppgc 的 no-GC scope，和 Rust 的 gc-arena 一樣「GC 只在 mutate 之間」。它做完了，也安全，但量下來 jQuery 剩下的 root 大多不是「一次操作裡建很多節點」，而是「只為了讀一下就 root」：

- **新 wrapper 的 realm**：原本中間的節點、document、realm 的 window 各 root 一次，只為了拿 window 的 wrapper；改成在一個 read scope 裡讀到底。
- **插入、移除表單元素**：jQuery 一次插入約 120 萬個沒有 form owner 的列表元素，每一個都 root 再走一遍重設 owner，什麼都沒改；改成先用讀取判斷有沒有事要做。
- **集合的 root**：`item`、`length` 每次都把集合當 receiver root 一次；和節點的 root 一樣重用 cell，不在 C++ 建了又釋放。

這三項進了 `main`。`NodePtr` 只再多省約 1–2% 的 cycle，代價是 rusty_v8 fork 的一處 C++ 改動（要取 `CppHeap` 兩個 private 成員）和依賴 cppgc 內部行為的安全論證，先不進 `main`（第 9 條）。

## 動機

- **handle 的成本**：2026-10-19 TodoMVC-jQuery 的取樣（RFC 0058 第 2 段之後），`Root::try_new` 1.47%、`Persistent` 的 Assign、建立、銷毀 0.85%、0.63%、0.31%、`Rc` 的 cell 0.35%，合計約 3%（`details.md` 的「handle 的成本」）。建 handle 的地方幾乎都在一次 DOM 操作裡用完：建立節點（`create_node`、`create_element_of`）、parser（`html::fast_path::build`、sink）、`replace_all`、`update_elements`、表單狀態、集合的 `item`。
- **RFC 0058 的 A1（kun 自己的 root 表）先不做**：寫入 root 表時要當場標記物件，造成 floating garbage，jQuery 反而慢約 9%（RFC 0058 第 22 條）。修法要再加一套 mutator 和 marker 之間的協定；而且 A1 只把 root 變便宜，root 還是要建。
- **保守掃描 stack（Blink 的做法）不能照搬**：裸指標存進 Rust 的 heap（`Vec`、closure、被 box 的 future）就掃不到，trait 和 auto trait 都限制不了位置；Blink 靠 clang plugin（`STACK_ALLOCATED`）檢查，也不完整。對 kun 等於放棄「safe code 不會造成 UB」（`crates/dom/soundness.md` 的目標）。另外 `dom_wasm` 的 standalone heap 不掃 stack，wasmtime 的 async 呼叫跑在自己的 fiber stack 上，cppgc 只掃目前 thread 的那一段（`details.md` 的「保守掃描」）。

目標：

- 一次 DOM 操作裡用的節點不建 root，`dom_api`、`html`、bindings 的熱路徑上 handle 的成本大部分消失。
- 安全論證照舊成立：不寫 `unsafe` 的程式碼不可能讓 `NodePtr` 指向已經釋放的物件。
- `dom_wasm`（standalone heap、wasmtime）和接上 V8 的文件一樣能用。

不在範圍內：

- `NodeIdMap` 的 `WeakPersistent`（jQuery 2.19%）：見「之後的可能」。
- 把所有 `NodeHandle` 換掉：要存起來、跨過 `await`、交給 V8 的照舊是 handle。
- 其他 GC 型別（`Event`、集合、`Range`…）的不 root 指標（集合的 root 只是重用 cell）。
- bindings 本身的轉換和宣告表（RFC 0058「之後的可能」的「bindings 本身」）。

要解決的 `docs/limitations.md` 裡的限制：無。

## 設計概要

對網頁和使用者：行為不變，只是變快。

對之後改這部分程式的人：**只讀的節點不建 root**。一次操作裡只要讀（找 realm、判斷有沒有事要做）的節點，留在 read scope 裡用 `NodeRef` 讀；只有要存起來、跨過 `.await`、交給 V8，或拿去做會配置的 mutation 時才 root。現有的 API 照舊。

## 詳細設計

### 新 wrapper 的 realm（第 5 條的 (a)）

bindings 替 DOM 物件建新的 wrapper 時，要建在物件所屬的 realm（節點的 document 的 realm window）的 context 裡（`dom_js::convert::realm_global`）。原本經過 `Object::realm_window`：中間的節點（集合的根、`style` 的元素……）`to_handle()`、document `to_handle()`、realm 的 window root 一次，只為了拿 window 的 wrapper。

現在 `dom` 的 `BindingContext::node_realm_global(f)` 在同一個 read scope 裡讓 `f` 讀出決定 realm 的節點，從它的 document 讀到 realm window，再拿 window 的 wrapper：V8 只從 traced reference 建一個 `Local`，兩個 heap 都不配置，不會 GC。`dom_api` 的 `Object::realm_of` 回傳由什麼決定 realm：一個節點（借用 read scope 的 `NodeRef`）、一個已經 root 的節點，或一個 window；`realm_window` 改成用它實作，行為不變。`f` 的型別是 `for<'r> FnOnce(&'r DomReadScope<'c>) -> Option<NodeRef<'r>>`，`'c` 是那次 `&mut self` 的借用，讓 `f` 能回傳借用外面的物件的節點；論證和 `read()` 一樣（`soundness.md` 的 `src/v8_engine.rs` 一節）。

### 插入、移除表單元素（第 5 條的 (b)）

列表元素被插入或移除時要重設 form owner（`dom_api` 的 `listed_element::update`）。暫時的計數器數 TodoMVC-jQuery 一次：約 120 萬次插入，幾乎全部是待辦清單的 `<input>`、`<button>`，全部沒有 form owner、也找不到 form，原本每一個都 `handle_by_id` 建 root、`reset_form_owner` 設成同一個 owner（只清 parser inserted 的旗標，而只有旗標沒設時才會走到這裡），什麼都沒改。

現在 `listed_element::nothing_to_do` 先用讀取判斷（`node_by_id`，不建 root）：插入的沒有要換的 owner、不是勾選的 radio、不是 option、沒有 `id`，移除的沒有 form owner、不是 form、不是 option、沒有 `id`（別的元素的 `form` 屬性可能指向它），就不走一遍。debug build 照樣走一遍，比對 DOM 的版本和 batch 都沒變（`docs/testing.md` 的「快取和增量的交叉檢查」）；判斷寫漏的條件會在所有 debug build 的測試裡被抓到。

### 集合的 root 重用 cell（第 7 條）

節點的 root 丟掉時，它的 `Persistent` 改指向一個錨點、留給下一個節點的 root 用（換指標只是 cppgc 的 `Assign`，建一個新的是 `malloc`、釋放還給 persistent region）。集合的 `item`、`length` 每次呼叫都把集合當 receiver root 一次（jQuery 0.49%），所以 `Roots` 多一個集合的池；`pool::<T>()` 照型別找池，取、還、拆掉 isolate 時清空都兩個池一起。照建 root 的型別分，其他型別都不到 0.03%，照舊。

### `NodePtr<'b>`：做完、量過，沒有進 `main`（第 2–4、6、9 條）

程式碼在本機的 branch `0060-node-ptr`（第 1 步和以上三項，`8fdb3931`），rusty_v8 fork 的 `39619d4`，V8 的 release `kun-v152.2.0-12`。fork 的 `kun` 分支之後 revert 了它（`5b98c50`），`main` 釘的是 revert，和 `kun-v152.2.0-11` 一樣；`-12` 的 tag 和 release 留著，給那個 branch 用。設計：

- **branded scope**：`DomMutationScope::with_ptrs(f)` 用 `for<'b> FnOnce(&mut BrandedScope<'_, 's, 'b>) -> R` 呼叫 `f`。`'b` 是 higher-ranked、invariant 的 brand（`PhantomData<fn(&'b ()) -> &'b ()>`），`NodePtr<'b>` 出不了 closure、傳不進別的 scope（包括別的 isolate 的）；`BrandedScope` 經由 `DerefMut` 交出 `&mut DomMutationScope`，closure 能用 `std::mem::swap` 換掉底下的 isolate，所以每個接 `NodePtr` 的操作先比對 `Roots` 的指標。
- **closure 期間不完成 GC**：kun 自己的 GC 入口、script、V8 都要 scope 借用著的東西；cppgc 配置時的入口（`CppHeap` 回報配置量、standalone heap 的上限、配置失敗的重試）都檢查 `IsGCAllowed()`，用 cppgc 的 `NoGarbageCollectionScope` 擋住；配置時推進 marking 不收尾、sweep 只碰上一輪的垃圾（`details.md` 的「GC 的入口」）。fork 包 `Enter/Leave`。
- **離開時照節奏補報配置量**：no-GC scope 裡 `CppHeap` 不把配置量回報給 V8，之後一次回報只推進 incremental marking 一步，`recalc_style` 的 cache miss 多了約 60%、cycle +3%。fork 的 `Leave` 把累積的量（private 的 `buffered_allocated_bytes_`）切成 V8 的檢查間隔（128 KB）一份一份交給 private 的 `ReportBufferedAllocationSizeIfPossible`（explicit instantiation 不檢查存取權的寫法）。
- **用上它的**：`innerHTML` 的 fast path 整個 parse 在一個 `with_ptrs` 裡，open elements 的 stack 是 `Vec<NodePtr>`；`BrandedScope` 的 `create_element`、`create_text`、`parser_append_child`、`ptr`、`node`、`root`，接 `AsNode`（sealed，`NodePtr` 和 `&NodeHandle`）。原本打算讓 `DomMutationScope` 的 61 個操作都接 `AsNode`（第 1 條），實際只加了 parser 要的幾個。
- **安全論證**：`soundness.md` 的 A11（no-GC scope 擋住哪些）、I19（closure 期間不完成 GC）、I1 的範圍和 `src/node_ptr.rs` 的審查重點，都在那個 branch 上。

量下來它自己的效益：做和不做第 1 步，jQuery 的 cycle 差約 1.5–2%、指令約 0.7%；整體的 kun / Chrome 比值看不出來。原本想再用它省的地方量過、數過都很小（第 7 條）。

### 安全論證（`crates/dom/soundness.md`）

- `src/v8_engine.rs` 一節：`node_realm_global` 在 read scope 裡也用 V8 的 scope，只建 `Local`，不在 JS 的 heap 上配置。
- `handle.rs` 的留著的 cell：集合的也留，各型別一個池，拿出來的 cell 型別一定對；拆掉時清空兩個池。

### 測試

- (a)：建 wrapper 的現有測試（`dom_js` 的 `iframes`，跨 realm 的節點）；(b)：`dom_api` 的 `forms`，加上 debug build 的交叉檢查；集合的 cell：`dom` 的 `collections::dropped_collection_roots_are_reused_and_root_nothing` 和 `handle.rs` 的單元測試。
- `dom` 的 `slots::fallback_changes_are_signaled`：slot 沒有指派到節點時，fallback content 的插入和移除要 signal（原本沒有測試，`NodePtr` 那一段的 mutation test 漏掉才補）。
- mutation test（`progress.md`）。

### 分階段

1. **原型**（第 3、4、6 條）：fork 的 no-GC scope 和照節奏補報；`with_ptrs`、`NodePtr`；fast path 換成 `NodePtr`，取樣 jQuery。在 `0060-node-ptr`，沒有進 `main`（第 9 條）。
2. **jQuery 剩下的 handle 成本**（第 5、7 條）：realm 的 window、插入和移除的表單元素、集合的 cell；(c)、(d) 量過、數過之後不做。
3. bindings 回傳節點的快速路徑：數過之後不做（第 7 條）。

## 缺點

- **(b) 的判斷要跟著規格**：會讓插入或移除的元素做事的新條件（新的 owner 規則、新的元素種類）要加進 `nothing_to_do`；漏了的話 release build 會少做事，靠 debug build 的交叉檢查抓（`docs/architecture.md` 的「快取和增量：加新東西時要跟著改的地方」）。
- **一次操作裡建很多節點的 root 還在**：`innerHTML` 的 fast path 每個節點照舊 root 一次（jQuery 約 1–2% 的 cycle），等 `NodePtr` 值得時再拿回來。

## 理由和其他做法

- **不選保守掃描**：見「動機」；完整的分析在 `details.md` 的「保守掃描」。
- **不先修 A1**：A1 讓 root 便宜，這份 RFC 讓 root 不用建；兩者不衝突，A1 剩下的效益之後再量。
- **`NodePtr` 不進 `main`**（第 9 條）：它省的是「一次操作裡建立、接上很多節點」的 root，jQuery 只有 `innerHTML` 的 fast path 是這種，1–2%；要付的是 fork 的 C++ 改動（取 V8 的 private 成員，換 V8 版本時要跟著搬、重新確認）、依賴 cppgc 內部行為的安全論證（`build.rs` 比對得到程式碼，比對不到語意）、多一套 API 和 closure 期間延後的 GC。
- **省 root 不一定要 `NodePtr`**：realm 的 window 和表單元素的 root，是一次操作裡只讀不寫的節點，在 read scope 裡讀完就知道答案；集合的 root 不是節點，重用 cell 就省掉 C++ 的建立和釋放。

## 先例

- Rust 的 gc-arena：`Gc<'gc, T>` 只在 `mutate` 的 closure 裡用（`'gc` 是 higher-ranked、invariant 的 brand），GC 只在 `mutate` 之間；`NodePtr` 是同一個模型，只是 kun 的 GC 是 cppgc，靠 no-GC scope 擋住 closure 裡的 GC。
- Blink 的 Oilpan：裸指標加上保守掃描 stack，heap 上的參照用 `Member`，`STACK_ALLOCATED` 由 clang plugin 檢查。
- V8 的 `HandleScope` 和 `Local`：handle 綁在 scope 上（RFC 0058 的 A2 的原型）。
- cppgc 自己的 `NoGarbageCollectionScope`：Blink 在不能 GC 的地方（例如某些 finalizer、序列化）用它。

## 已決定的事項

2026-10-19：

1. **mutation 的操作接 `AsNode` trait**（`&NodeHandle` 和 `NodePtr` 都實作），不各加一個 `_ptr` 版本：入口一份，呼叫的地方不用改名。最後只有 `BrandedScope` 的操作接它（在 `0060-node-ptr`）。
2. **跨 isolate 用 closure 式的 scope 擋**：`NodePtr` 只在 branded scope 的 closure 裡拿得到（原本叫 `mutate_with`，做成 `DomMutationScope::with_ptrs`），lifetime 是那個 closure 專屬、invariant 的 brand（gc-arena 的做法），傳給別的 scope 編不過。不選在 `Node` 記著 isolate、執行期比對（每個節點多一個欄位），不選從 cppgc 的 page 找 heap（每次一次 C++ 呼叫）。
3. **no-GC scope 由 fork 包 cppgc 公開的 `NoGarbageCollectionScope::Enter/Leave`**，每個 scope 兩次 C++ 呼叫；原型量過再決定要不要像 RFC 0058 的 3a 一樣讓 Rust 直接改計數器（第 8 條：不改）。
4. **原型先只換 `html` 的 sink**，取樣 TodoMVC-jQuery 看效益，再決定第 2、3 段做哪些。實際換的是 `innerHTML` 的 fast path：jQuery 的 `innerHTML` 都走它，sink 是 0%。

2026-10-19，原型量過之後：

5. **第 2 段照 jQuery 剩下的 handle 成本排順序**，html5ever 的 sink 不先做：(a) realm 的 `Window`（`Object::realm_window`、`dom_js::convert::realm_context`，0.77%：每次 binding 呼叫 root 一次，它不是節點，realm 的 global 撐著它）、(b) `dom_api` 內部建了又丟的 handle（`update_elements`、`text_control_tree::update_pending`、表單狀態，約 0.9%）、(c) fast path 交給 hooks 的 root（0.48%，`ElementHooks` 改成接 `NodePtr`）、(d) `replace_all` 和集合的 `item`（0.53%）；bindings 呼叫本身（參數 unwrap、回傳時 wrap，約 1%，`dom_api` 的簽名要大改）留到最後再看。每做完一項和 `main` 輪流各取樣三次。
6. **離開 no-GC scope 時照 V8 的檢查間隔補回報配置量**（fork）：一次回報一大筆只推進 marking 一步，`recalc_style` 的 cache miss 多了約 60%（原型的取樣，`progress.md`）。

2026-10-19，第 2 段做完之後：

7. **第 2 段不做 (c)、(d)，第 3 段不做；改成集合的 root 也重用 cell**：(c) 量過反而慢（TodoMVC 範本裡帶屬性的元素大半本來就是 hooks 在意的，每個屬性卻多跑一次判斷）；(d) 的 `replace_all` 一次 jQuery 不到 2 萬次；第 3 段的回傳路徑只對已經有 wrapper 的節點有用，而 `wrapper()` 有 87% 是新建的。剩下的 root 成本照型別分，集合的丟掉（0.49%）是節點之外唯一顯著的，給它和節點一樣的 cell 池，改動只在 `handle.rs`。數字在 `progress.md`。
8. **no-GC scope 不改成 Rust 直接改計數器**（第 3 條的後半）：進出連同補報佔 jQuery cycle 的 0.13%–0.20%，大多是補報（推進 V8 的 marking，本來就要做的 GC 工作），自己幾乎是 0；能省的只有兩次 C++ 呼叫，卻要 Rust 知道 cppgc 內部欄位的位置。

2026-10-19，整份做完之後：

9. **`NodePtr` 不進 `main`，進的只有 (a)、(b) 和集合的 cell**：拆開量，四項裡最大的是 (b)（指令約 −2.5%），其次 (a)（約 −1.5%）、集合的 cell（約 −1.3%），都不需要 `NodePtr`；`NodePtr` 自己只有約 −0.7% 的指令、1–2% 的 cycle，而要付的是 fork 的 C++ 改動（取 `CppHeap` 的兩個 private 成員）、依賴 cppgc 內部行為的安全論證、多一套 API。不選全部合併：以 kun 目前的規模，這筆效益換不了那些維護成本。程式碼留在 branch `0060-node-ptr`，`main` 照舊連 `kun-v152.2.0-11`。

## 待決定

無。

## 之後的可能

- **`NodePtr` 拿回來**（`0060-node-ptr`）：一次操作裡建立、接上很多節點的路變熱時（html5ever 的 sink 處理整份文件、`replace_all` 換大量子節點的頁面），或 V8 公開了不用取 private 成員就能照節奏回報的方法時，再量一次。要接上的還有 html5ever 的 sink（`Handle` 要對 `DomAccess` 泛型化）。
- **bindings 回傳節點的快速路徑**（原本的第 3 段）：節點已經有 wrapper 時直接拿，不 root。jQuery 只有 13% 的回傳走得到；換成很多次拿同一批節點的頁面再量。
- **其他「只讀卻 root」的地方**：熱路徑上可能還有為了呼叫接 handle 的函式而 `to_handle()` 的讀取；jQuery 剩下的都不到 0.1%，換頁面量到時再改。
- **其他 GC 型別的不 root 指標**：`Event`、`Range` 這些在 bindings 和事件分派裡也常建 handle。
- **`NodeIdMap` 的 `WeakPersistent`**（jQuery 2.19%）：RFC 0058 的 3c 想用弱的 slot 表取代；或照 Blink 的 `DOMNodeIds` 只在被要求 id 時才登記（RFC 0058 的 2b 量過，99.8% 的節點都被拿過 id，所以不做）。
- **A1（RFC 0058 的 root 表）**：剩下的 handle 大多是要存起來的，量過再決定要不要修好 A1（stash 裡的程式碼和 RFC 0058 第 22 條的修法）。
