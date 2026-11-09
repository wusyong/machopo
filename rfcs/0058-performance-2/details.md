# RFC 0058 的細節：量測的證據和 handle 的設計比較

這份放 `README.md` 的根據：2026-10-18 的 trace 和取樣的細項，以及 handle 的幾種重新設計的完整比較。`README.md` 裡的結論和決定以那邊為準。

## 量測

### 來源

- **trace**：`main` 的 `4fe16e84`，`python3 scripts/ci.py speedometer --trace --iterations 3`（20 個 default suite，兩邊各一份），Chrome for Testing 152.0.7977.82，Fedora 44，AMD Ryzen AI 9 HX 370，kun 用 wgpu 的 Vulkan（Radeon 890M）。分析用暫時的腳本：取記著 runner 的 User Timing mark 的那一條 track（Chrome 有好幾條叫 `CrRendererMain` 的 thread，各是一個 renderer process），照 `speedometer.py` 的 `mark_windows` 找計時區間，算每個 slice 名稱在區間裡的 self time，除以 3 是每個 iteration 的。
- **取樣**：同一個 commit，`ci.py speedometer --sample --iterations 3 --suites TodoMVC-jQuery,TodoMVC-JavaScript-ES6-Webpack-Complex-DOM,React-Stockcharts-SVG,NewsSite-Next,Editor-TipTap`（`perf` profile 加 frame pointer，`cycles:u` 每一百萬個一個樣本）。ES5 的數字是同一天中午、RFC 0055 完成時的取樣，只當參考。
- **歸類**：每個樣本照最內層、屬於某一類的 frame 歸類（`JS:`、`RegExp.` 是 JS；`Builtins_` 是 V8 的 builtin；`v8::internal::` 等是 V8 的 C++；`v8__*`、`cppgc__*` 是 Rust 呼叫的 V8 API；其他照 crate 的名字）；allocator、`mem*`、`core`、`std`、`hashbrown` 算在呼叫它們的那一層。

### 各階段（每個 iteration 的 ms，19 個 suite，不含 Perf-Dashboard）

slice 的歸類：script 是 `v8.callFunction`、`FunctionCall`、`RunMicrotasks`、`EventDispatch` 等和 `V8.*`（GC 的除外）；GC 是名字裡有 GC 或 scavenge 的；parse 是 `ParseHTML`、`HTMLDocumentParser::*`；建 box tree 是 kun 的 `StyleEngine::RebuildLayoutTree` 和 Chrome 的 `Document::rebuildLayoutTree`；style 是 `Document::UpdateStyle`、`Document::recalcStyle` 等；layout 是 `Layout`、`LocalFrameView::performLayout`、`InlineNode::*`；paint 是 `Paint`、`PrePaint`、`PaintArtifactCompositor::Update`、`LayerTreeHost::DoUpdateLayers` 等；canvas 是 `raster::*`、`canvas::*` 和 Chrome 的 `Canvas*`。

| | script | parse | GC | style | 建 box tree | layout | paint | canvas | 其他 | 合計 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| kun | 1080 | 174 | 27 | 282 | 267 | 184 | 81 | 80 | 70 | 2245 |
| Chrome | 648 | 174 | 45 | 98 | 15 | 164 | 129 | 29 | 77 | 1379 |
| kun 多的 | 432 | 0 | −18 | 184 | 253 | 20 | −48 | 50 | −7 | 866 |
| 佔差距 | 50% | 0% | −2% | 21% | 29% | 2% | −6% | 6% | −1% | |

和 RFC 0051 第 6 段之後的同樣的量（`rfcs/0051-performance/progress.md`）比：差距約 1765 → 866 ms；parse 214 → 0（RFC 0051 的 C3）、等 Viz 182 → 0（RFC 0056 拿掉了 `SoftwareRenderer`，這次的 trace 裡沒有等它的時間）、script 699 → 432（RFC 0051 的 7a、7b）、style 加 box tree 540 → 437、canvas 95 → 50。

### 各 suite（kun 多的 ms，每個 iteration）

| suite | script | style | 建 box tree | layout | canvas | 合計 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Editor-TipTap | 15 | 3 | 135 | 40 | 0 | 192 |
| TodoMVC-jQuery | 98 | 24 | 5 | −5 | 0 | 132 |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 76 | 12 | 3 | 0 | 0 | 105 |
| Perf-Dashboard（不在合計裡） | 44 | 7 | 7 | −10 | 62 | 91 |
| NewsSite-Next | 28 | 24 | 14 | 12 | 0 | 65 |
| React-Stockcharts-SVG | 30 | 18 | 19 | −1 | 0 | 63 |
| TodoMVC-WebComponents | 16 | 14 | 11 | −3 | 0 | 43 |
| NewsSite-Nuxt | 10 | 17 | 10 | 13 | 0 | 37 |
| Charts-observable-plot | 24 | 13 | 2 | −3 | 0 | 34 |
| TodoMVC-JavaScript-ES5 | 36 | 4 | 3 | −4 | 0 | 30 |
| TodoMVC-React-Complex-DOM | 17 | 7 | 6 | −3 | 0 | 27 |
| Charts-chartjs | 28 | −1 | 0 | −1 | 50 | 25 |
| 其他 8 個 | 5–14 | 2–12 | 4–8 | −5–−1 | 0 | −9–23 |

### 時間花在誰身上（佔各 suite 樣本的比例）

| suite | kun 的 DOM 和 bindings | V8 執行 JS（JIT、builtin、C++） | style | box tree 和 layout | 文字 |
| --- | ---: | ---: | ---: | ---: | ---: |
| TodoMVC-jQuery | 63% | 17% | 9% | 1% | 1% |
| TodoMVC-JavaScript-ES6-Webpack-Complex-DOM | 51% | 15% | 19% | 4% | 1% |
| React-Stockcharts-SVG | 17% | 48% | 18% | 7% | 7% |
| NewsSite-Next | 17% | 18% | 22% | 24% | 12% |
| Editor-TipTap | 3% | 10% | 2% | 18% | 64% |

「kun 的 DOM 和 bindings」是 `dom_api`、`dom`、`dom_js`、`bindings` 和 Rust 呼叫的 V8 API。jQuery 另有 parse 9%。

### DOM 那一側的拆分

| | jQuery | ES6-Webpack | ES5（中午） |
| --- | ---: | ---: | ---: |
| handle 和 root（`Root`、`Handle`、`Persistent` / `WeakPersistent` 的建立、drop；不含 `NodeId` 的雜湊） | 約 8% | 約 5% | 約 6% |
| bindings（`dom_js`、`bindings`、`v8__*`） | 14% | 5% | 9% |
| `dom_api` 的邏輯 | 21% | 28% | 17% |
| `dom` 的樹操作 | 19% | 13% | 13% |

換算成時間：jQuery 每個 iteration 338 ms 的約 22%（約 74 ms）是 handle 加 bindings，它和 Chrome 差 132 ms；ES6 174 ms 的約 10%（約 17 ms），差 105 ms；ES5 108 ms 的約 15%（約 16 ms），差 30 ms。

handle 和 root 的細項（jQuery）：

| 函式 | % | 是什麼 |
| --- | ---: | --- |
| `cppgc__WeakPersistent__CONSTRUCT` | 1.8 | 每建一個節點，`create_node` 在 `NodeIdMap` 放一個 `WeakPersistent`（不是 handle 本身；第 2 段的 2b） |
| `hash_one::<&NodeId>`（`RandomState`） | 1.7 | 幾乎都是表單狀態的 `HashSet<NodeId>`（`Affected::element` 33、`update_form_states` 19、`update_elements` 10 個樣本），不是 handle（第 1 段的 1a） |
| `cppgc__Persistent__CONSTRUCT` | 1.6 | spare 用完時新建的 `Persistent` |
| `Root::try_new` | 1.5–2 | 取 spare、建 `Rc<RootCell>`、計數 |
| `Handle` 的 drop | 1.1 | `Rc` 減一、放回 spare |
| `cppgc__Persistent__Assign` | 1.0 | spare 取出、放回時改指向 |

DOM 那一側其他較大的（jQuery / ES6）：`update_form_states` 2.7% / 5.2%，`Affected::element` 1.9% / —，`form_states_children_changed` 1.7% / 0.8%，`update_elements` 3.2% / 1.5%，`link_child` 3.1% / 2.6%，`detach_with` 2.5% / 1.7%，`handlers::attribute_changed` 2.1% / —，`text_control_tree::update` 1.4% / 0.7%；ES6 的 `parent_node::query_selector` 13.1%（比對本身）。

### 表單狀態的拆分

TodoMVC-jQuery，2026-10-19 在 1a 之後重新取樣；計數是暫時的計數器，跑 10 個 iteration。做了什麼、之後的數字見 README 的第 15–17 條和 `progress.md` 的第 2 段。

- `text_control_tree::update_pending`（4.41%，全部從每個 frame 的 `focus_fixup` 呼叫：focus 在新增的輸入框上，它先更新 UA shadow tree 和 layout，lifecycle 之後就不用再做）：72 次呼叫，記下的 id 約 100 萬、不重複的約 50 萬，每個都先建一個 handle（`Root::try_new` 1.31%、drop 0.41%），`update` 裡 67% 什麼都不做（沒有 UA shadow tree、也不需要）。
- `update_form_states`（4.24%：`Affected::element` 1.83%、自己 1.50%）：約 1 萬次呼叫，處理約 104 萬個元素；一半是剛插入的，其中 99.98% 不是 option、select，也不依賴整棵樹（radio、有 form 的 submit button），這些只需要記 UA shadow tree 的更新，卻照樣建 handle、放進集合、算狀態。記下的 UA shadow tree 更新只有 33% 是有 tree 的元素。
- `form_states_children_changed`（1.28%）：每次插入都從 parent 往上找 `<option>`，約 376 萬次呼叫、1054 萬步，一次都沒找到（TodoMVC 沒有 `<option>`）。
- `handlers::attribute_changed`（2.80%）：parser 設的每個屬性都經過七個模組的 attribute change steps；`frame`、`script`、`canvas` 比對屬性名稱時每次都建一個 `QualName` 再丟掉（drop 0.79%）。
- `update_elements` 自己的 2.36% 和 `reset_form_owner` 的 0.89% 還沒拆開（`perf annotate` 在這份取樣上跑不完）。

### root 表的取樣

TodoMVC-jQuery，`ci.py speedometer --sample`，2026-10-19，Fedora 44，連結本機用 `ci.py v8` 編的 V8（`RUSTY_V8_ARCHIVE`）。每列一次取樣（30 個計時的區間）；取樣有雜訊，3b 取了兩次。

| | cycle 樣本 | 指令樣本 | IPC |
| --- | ---: | ---: | ---: |
| 第 2 段結束（2a 之後） | 15423 | 38981 | — |
| 3a（`Member` 的寫入在 Rust 裡做） | 15069 | 38560 | 2.56 |
| 3b（root 表，marking 中寫入時當場標記物件） | 16605 | 36911 | 2.22 |
| 3b，再一次 | 16445 | 36827 | 2.24 |
| 3b，暫時拿掉當場標記（不安全，只為了量） | 14398 | 36407 | 2.53 |

3a 之後，`cppgc__Member__Assign`（1.06%）和 `cppgc__Member__CONSTRUCT`（0.23%）不見了。

3b 之後（和 3a 比，佔 cycle 樣本的比例）：

| 函式 | 3a | 3b |
| --- | ---: | ---: |
| `Root::try_new` | 1.43 | 0 |
| `cppgc__Persistent__Assign` / `CONSTRUCT` / `DESTRUCT` | 0.56 / 0.69 / 0.29 | 0 |
| `RootTable::take`（自己 1.29，`cppgc__WriteBarrier__MarkValue` 1.02，幾乎都在 `DijkstraMarkingBarrierSlow`） | — | 2.31 |
| `StyleEngine::recalc_style`（自己 9.40 → 19.53，`style::cascade` 1.18 → 1.08） | 11.13 | 21.09 |
| 其中 `getComputedStyle` 的 `property_value` | 1.77 | 6.58 |

root 本身的成本少了約 3%，但 `recalc_style` 自己的時間加倍，cascade 不變（重算的元素一樣多），指令也沒變多：多的是 cache miss。GC 的樣本（cppgc 和 V8 的 marking、sweeping）從 552 增加到 1039。拿掉當場標記之後，`recalc_style` 回到 11.43%，IPC 回到 2.53：marking 中建立過 root 的物件都被標記、多活一輪（floating garbage），heap 變大、locality 變差。

### 文字（Editor-TipTap，佔它的樣本）

| 函式 | % |
| --- | ---: |
| `parley::shape::shape_text` | 14.3 |
| `layout::text::InlineContent::fragments` | 10.6 |
| `parley::layout::line_break::LineState` 的 clone | 7.3 |
| `harfrust::hb::ot_layout::apply_forward` | 6.7 |
| `parley::shape::FontSelector::select_font` | 4.1 |
| `parley::layout::run::Clusters::next` | 3.3 |
| `layout::text::InlineContent::new` | 3.1 |
| `parley::builder::build_into_layout` | 2.8 |
| harfrust 的 GPOS pair、chained context | 3.9 |
| icu 的斷行和斷字 | 3.5 |

### V8 本身

kun 的 script 時間（19 個 suite，每個 iteration）照取樣拆開：JS 和 V8 約 490 ms、bindings 185、handle 和 root 54、`dom_api` 244、`dom` 72、其他 35，合計 1080；Chrome 的 script 合計 648（Chrome for Testing 沒有符號，拆不開）。JS 為主的 suite 裡 kun 光 JS 和 V8 就和 Chrome 整個 script 差不多（Stockcharts 81 對 79 ms），所以查了 V8 本身是不是比較慢：

**只有 JS 的 microbenchmark**（暫時的頁面，不碰 DOM；每項先跑 3 次暖機，再跑 10 次取中位數；兩邊各開 3 次新的 process，再取中位數；data URL）：

| 測試 | kun ms | Chrome ms | kun / Chrome | 主要在哪裡 |
| --- | ---: | ---: | ---: | --- |
| 形狀很多的物件讀屬性（megamorphic） | 5.5 | 17.6 | 0.31 | JIT 的程式、IC |
| class 和方法 | 11.7 | 16.3 | 0.72 | JIT 的程式 |
| 陣列的 `map` / `filter` / `reduce` | 14.6 | 15.8 | 0.92 | builtin |
| closure、`fib(25)` | 1.8 | 9.8 | 0.18 | JIT 的程式 |
| 字串 | 2.8 | 2.4 | 1.17 | V8 的 C++ |
| `Map`、`Set`（字串 key） | 26.9 | 23.9 | 1.13 | V8 的 C++（雜湊） |
| 正規表達式 | 2.9 | 2.3 | 1.26 | regexp 引擎、V8 的 C++ |
| `JSON.stringify` / `parse` | 10.6 | 6.2 | 1.71 | V8 的 C++ |

Chrome 加上 `--disable-renderer-backgrounding`、`--disable-background-timer-throttling`、`--js-flags=--no-efficiency-mode` 結果不變（不是背景節流或 efficiency mode）。JIT 的程式在 kun 快得多的原因沒有確認（兩邊的 V8 設定不同，見下面；microbenchmark 也不代表實際的頁面）。

**tier up**（`--log-code --log-deopt` 寫進檔案，數 JS 的 code-creation 的 tier 和 deopt；一個 suite 10 個 iteration，Chrome 加 `--no-sandbox` 才能寫檔案）：

| suite | | Ignition | Sparkplug | Maglev | TurboFan | deopt |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| TodoMVC-React-Redux | kun | 658 | 402 | 234 | 20 | 94 |
| | Chrome | 1058 | 510 | 236 | 20 | 107 |
| React-Stockcharts-SVG | kun | 1842 | 716 | 725 | 44 | 548 |
| | Chrome | 1847 | 733 | 702 | 51 | 533 |

兩邊最佳化的數量和 deopt 幾乎一樣（Chrome 在 React-Redux 多的 Ignition 應該是它自己的 script）：kun 的 bindings 沒有讓程式比較晚最佳化或比較常 deopt。取樣裡 script 階段的 V8 時間大多在 Sparkplug 的 builtin（`LoadICGenericBaseline`、`LoadIC_Megamorphic`、`KeyedLoadIC_Megamorphic`、`StrictEqual_Generic_Baseline`）：Speedometer 的 test 都短，Chrome 也一樣。

**build 設定**（V8 15.2.124，kun 的是 `.1`、Chrome 152 的是 `.21`）：

| | kun | Chrome |
| --- | --- | --- |
| pointer compression | 開 | 開 |
| V8 sandbox | 關 | 開 |
| C++ 的 PGO、ThinLTO | 沒有（不是 official build） | 有 |
| builtin 的 PGO（`tools/builtins-pgo`） | 沒有 | 有：V8 的 `gni/v8.gni` 只在 `build_with_chromium && chrome_pgo_phase == 2` 時打開，還要求 sandbox 和 external code space；profile 要另外下載 |

結論：JIT 的程式和 builtin 在 kun 不比較慢；V8 的 C++ runtime 慢 13–71%，和沒有 PGO、ThinLTO 對得上。script 階段裡 V8 的 C++ 佔 kun 的樣本 10–18%（JS 為主的 suite），照這個比例，因為沒有 PGO 多出來的約 20–40 ms（19 個 suite 合計）。script 的差距主要還是 kun 的 DOM 那一側（bindings、`dom_api`、`dom`、handle，合計約 555 ms）。

## handle 的設計比較

現在每個 handle 的一生：從 spare 取一個 `RootCell` 或新建（`Rc` 的配置、`Persistent` 的建立）、`Persistent::Assign`（C++）、計數；clone 是 `Rc` 加一；最後一個丟掉時計數、`Persistent::Assign` 回錨點（C++）、放回 spare。讀取不經過 C++（cell 裡的指標；`Member`、`Persistent` 的讀取在 Rust 裡 inline）。

| | A1：kun 的 root 表 | A2：範圍的 `HandleScope` | B4：不 root 的 `NodeRef` | V8 的 `HandleScope` 和 `Local` | 保守掃描 stack（Blink） |
| --- | --- | --- | --- | --- | --- |
| 做法 | slot 的陣列加 free list，一個 GC 物件的 `trace` 走過它；handle 是 slot 的索引 | handle 綁在 scope 上，scope 結束時整批退出；活得比 scope 久的轉成長期的 root | `dom_api` 和 bindings 內部用 lifetime 綁住的裸指標，可能觸發 GC 的地方之前才 root | 節點經過它的 JS wrapper 的 `Local` 撐住 | stack 上放裸指標，cppgc 掃 stack |
| 每個 handle 的成本 | 陣列存取加計數；marking 中新加 root 時一次 barrier | 推進陣列，scope 結束一次清掉 | 大部分是零 | 建 wrapper（沒有的話）、每次存取 unwrap（V8 API） | 零 |
| API | 不變 | 帶 lifetime；回傳要 escape | `dom_api` 大多數函式的簽名 | 綁死 V8 | 不變 |
| 能不能用 | 能 | 能，但要全面改型別 | 能，但要全面改型別 | wasm 的文件沒有 V8（`dom_wasm` 用 standalone 的 heap），`dom_api` 和 engine 無關 | 存進 Rust 的 heap 的裸指標掃不到，型別擋不住 |
| 安全論證 | root 的實作換掉，補兩條（marking 中新加的 root、atomic pause 時表完整） | scope 的 lifetime 擋住 handle 逃出 | 放寬 I1，每個 GC 點都要標出 | — | 放棄 safe code 不會造成 UB |
| 預期效益（jQuery） | handle 成本的約三分之二，約 4%（約 13 ms） | 比 A1 多一些 | 約 6%（約 20 ms） | 比現在差 | — |

不全面改型別的根據（2026-10-18 的 `main`）：`dom_api`、`dom_js`、`dom_wasm`、`renderer` 裡型別帶 `Handle` 的結構欄位約 715 個（listener、observer、排隊的 task、timer、mutation record、promise 的 callback），`dom_api` 有 34 個 `async fn`（handle 跨過 `await`），`NodeHandle` 在這幾個 crate 出現約 1300 次。

A1 和 A2 可以疊：A1 之後，在量出來最熱的路徑上給暫時的 handle 一個 scope，結束時整批退回 root 表（`README.md` 的「之後的可能」）。

## 參考資料

- cppgc 的 `PersistentRegion`（`v8/include/cppgc/internal/persistent-node.h`、`v8/src/heap/cppgc-internal/persistent-node.cc`）：A1 的表和它同樣的結構。
- cppgc 的 `subtle::HeapConsistency`（`v8/include/cppgc/heap-consistency.h`）：Dijkstra 和 Steele 的 barrier；fork 的 `src/binding.cc` 已經用它的 `SteeleWriteBarrier`。
- Chromium 的 font service（`components/services/font/font_service_app.cc`）呼叫 Skia 的 `SkFontConfigInterfaceDirect::matchFamilyName`（`src/ports/SkFontConfigInterface_direct.cpp`）：先 `FcFontMatch`，結果不合格（`isAcceptableMatch`）才 `FcFontSort`、取第一個有效的；不用 `FcFontList`（2026-10-18 對照兩者 `main` 上的原始碼）。
- Blink 的 `DOMNodeIds`（`third_party/blink/renderer/core/dom/dom_node_ids.h`）、`HarfBuzzShaper` 和 `FontFallbackIterator`（`platform/fonts/shaping/`）。
