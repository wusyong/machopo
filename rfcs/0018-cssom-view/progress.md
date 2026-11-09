# RFC 0018 的進度：CSSOM View 和 script 觸發的 layout

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-08（第 1 段）。完成：2026-10-08。平台：1a 到 3a 在 macOS（Apple Silicon）、3b 和 3c 在 Windows 11、3d 和完成時的驗收在 Fedora 44（Linux x86_64，Rust 1.99.0）；三個平台各跑過一次完整的 WPT 和 reftest（README 的「已決定的事項」第 40 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a 借出去的路：`ScriptEngine` 交出 `LayoutClient`、`DomState` 借出、realm 先複製出來 | 完成 | 2026-10-08 |
| 1b renderer 的同步更新：只更新自己和祖先、收進剛建的子 frame、render-blocked 照樣更新 | 完成（測試在 1e） | 2026-10-08 |
| 1c `layout` 的 `geometry`：bounding rect、client rects，和 pre-paint 比對 | 完成 | 2026-10-08 |
| 1d DOM API：`DOMRect` 系列、`getBoundingClientRect`、`getClientRects`、`innerWidth` 等 | 完成 | 2026-10-08 |
| 1e 端對端的測試、WPT、mutation test、文件 | 完成 | 2026-10-08 |
| 2a 讀盒子的大小和捲動：`offset*`、`offsetParent`、`client*`、`scroll*` | 完成 | 2026-10-08 |
| 2b 寫捲動位置：`scrollTop` / `scrollLeft`、`scrollTo` / `scrollBy` / `scroll` | 完成 | 2026-10-08 |
| 2c `scroll` 事件 | 完成 | 2026-10-08 |
| 2d-1 pre-paint 變成 lifecycle 的階段，輸入的 hit test 先更新到它 | 完成 | 2026-10-08 |
| 2d-2 `elementFromPoint` / `elementsFromPoint` | 完成 | 2026-10-08 |
| 2e 端對端的測試、WPT、mutation test、文件 | 完成 | 2026-10-08 |
| 3a `scrollIntoView`：`block` / `inline` 的對齊、沿著 frame 往上捲 | 完成 | 2026-10-08 |
| 3b 匯入整個 `css/cssom-view/`（含 reftest） | 完成 | 2026-10-08 |
| 3c 評估 `check-layout-th.js`，收全過的 | 完成 | 2026-10-08 |
| 3d `scrollIntoView` 的視窗示範、mutation test、文件；`matrix` 和 `fuzz` | 完成 | 2026-10-08 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| 借出的機制 | 借出期間 `dom_api` 拿得到 `LayoutClient`、之外拿不到、可以巢狀、panic 時放回原本的 | `cargo test -p dom --test layout_client`、`cargo test -p dom_js --test layout_client`、`cargo test -p dom_wasm --test engine the_client_is_lent` |
| 借出的機制 | 每個執行 script 的入口都借出 layout：classic（inline、外部）、module、`import()`、timer、animation frame、事件（`load`、點擊、iframe 的 `load`）、`javascript:` URL、mutation observer、之後的 microtask；wasm script | `cargo test -p shell --test geometry`、`cargo test -p shell --test wasm_scripts layout` |
| GC 型別 | `DOMRectReadOnly`、`DOMRect`、`DOMRectList`：建構子、`fromRect`、值和邊、setter、list 的 index 每次是同一個物件（JS 和 wasm） | `cargo test -p dom --test script_objects`、`cargo test -p dom_api --test geometry`、`cargo test -p dom_js --test geometry`、`cargo test -p dom_wasm --test engine geometry` |
| `dom_api` 的規則 | 讀幾何的成員先更新 layout 再問，`devicePixelRatio` 不更新；沒有 frame 時的值 | `cargo test -p dom_api --test geometry` |
| `dom_api` 的規則 | `offset*`、`offsetParent`、`client*`、`scroll*`、`scrollingElement` 的規則，quirks mode 是 `body` | `cargo test -p dom_api --test geometry`、`cargo test -p dom_js --test geometry box_members`、`cargo test -p shell --test geometry box_members` |
| `dom_api` 的規則 | Script 的捲動：setter、`scrollTo` / `scrollBy` / `scroll`（JS 的 `(x, y)` overload、promise）、沒給的軸、非有限的數、對齊 device pixel、scrolling element 交給 window | `cargo test -p dom_api --test geometry scrolling`、`cargo test -p dom_js --test geometry` |
| `dom_api` 的規則 | `scrollIntoView` 的參數：`block` / `inline`、boolean 的 union、`container: nearest`、dictionary 的預設值（`inline` 是 `nearest`） | `cargo test -p dom_api --test geometry scroll_into_view_asks_the_client`、`cargo test -p dom_api --test api the_table_describes_each_interface`、`cargo test -p dom_js --test geometry scroll_into_view_from_script` |
| `dom_api` 的規則 | `elementFromPoint` / `elementsFromPoint`：子 frame 裡的換成 iframe、去掉重複、最後補上根元素 | `cargo test -p dom_api --test geometry hit_testing`、`cargo test -p dom_js --test geometry hit_testing` |
| WIT 和 guest | `unrestricted double`、被子 interface 蓋掉的成員只輸出一次、`from-rect` 是 static operation、record 用到的 enum 也輸出、dictionary 的預設值由 Lua 的 SDK 填 | `cargo test -p dom_api --test wit`、`python3 scripts/ci.py guests`、`python3 scripts/test.py reftest wasm/w9-lua` |
| `layout` 的幾何 | Client rects 和 bounding rect 的規則（CSSOM View：沒有 rect、都沒有寬高、有寬高的聯集）、`box_transform` 以中心為準和百分比的軸 | `cargo test -p paint --test geometry` |
| `layout` 的幾何 | `element_box`：沿著 containing block 加 offset、不算 transform 和捲動、inline 元素是各行的外框 | `cargo test -p paint --test geometry element_boxes_are_before_transforms_and_scrolling` |
| `layout` 的幾何 | Scrollable overflow：padding box、子盒子的 border box，加上 in-flow 內容（行和子盒子的 margin box）外面一層 padding；穿過底部摺疊的 margin 不算 | `cargo test -p layout --test block scrollable_overflow`、`cargo test -p layout --test block scrollable_overflow_pads_the_margin_boxes` |
| `layout` 的幾何 | 根元素的 margin 不和子元素的摺疊（2a 找到的 bug） | `cargo test -p layout --test block root_margins_do_not_collapse_with_the_viewport`、`cargo test -p paint --test geometry the_roots_margins_dont_collapse_with_its_childrens` |
| 兩套座標一致 | `layout` 算的 rect 和 pre-paint 的 `element_bounds` 在同一批頁面上一樣（transform：平移、縮放、矩陣、巢狀；捲動容器：巢狀、有 transform、直接含文字的；跨行的 inline、flex） | `cargo test -p paint --test geometry` |
| renderer 的同步更新 | 強制更新只到自己和祖先的 frame，兄弟的不動 | `cargo test -p renderer --lib a_forced_update_lays_out_the_chain_only` |
| renderer 的同步更新 | 元素的 rect（transform、跨行的 inline、`display: none`、不在 document 裡、剛改過 style）、window 的 viewport、捲到 `#fragment` 之後 | `cargo test -p shell --test geometry` |
| renderer 的同步更新 | 子 frame：自己的 viewport、祖先先 layout、剛插入的 iframe、沒有 box 的 iframe；render-blocked 的 document 照樣 layout | `cargo test -p shell --test geometry` |
| renderer 的捲動 | Script 的捲動照滾輪同一個 `ScrollNode` clamp、`overflow: hidden` 照樣捲得動、標記要重畫或重做 pre-paint、告訴 event loop；compositor 的位移疊在上面 | `cargo test -p renderer --lib scroll`、`cargo test -p shell --test geometry scrolling` |
| renderer 的捲動 | `scrollIntoView` 的對齊照 Blink 的 `GetScrollOffsetToExpose`（四種對齊和 `nearest` 的三種情況）、沿著捲動框和 frame 往上捲、`container: nearest` 只捲最近的 | `cargo test -p renderer --lib each_alignment_exposes_a_span_as_blink_does`、`cargo test -p renderer --lib script_scrolls_into_view_as_asked`、`cargo test -p shell --test geometry scroll_into_view` |
| `scroll` 事件 | 位置真的變了才記下目標、去重、照順序；下一個 frame、rAF 之前分派，document 的 bubble、元素的不 bubble、`onscroll`、子 frame 的；layout 的 clamp 和 compositor 捲的也發 | `cargo test -p renderer --lib what_moved_is_noted_for_its_scroll_event`、`cargo test -p shell --test geometry scroll`、`cargo test -p shell --test geometry layout_clamping_fires_a_scroll_event` |
| pre-paint 的階段 | 只在 layout、捲動、子 frame、縮放變了時重做；輸入和 script 的 hit test 先更新到它 | `cargo test -p renderer --lib frame_view`、`cargo test -p shell --test script a_click_skips_a_node_moved_to_another_document` |
| hit test | 最上面的和全部（不算文字和 anonymous box）、子 frame 換成 iframe、canvas 是根元素、viewport 外、捲動過的頁面和子 frame | `cargo test -p paint --test geometry hit_test_all`、`cargo test -p paint --test child_frames hit_testing`、`cargo test -p shell --test geometry hit_testing` |
| 視窗裡（情境） | 滾輪之後的 `scrollY`、改大小之後的 `innerWidth`、沒有 box 的 iframe 的 `devicePixelRatio`；script 捲動 window、滾輪和 script 捲動的 `scroll` 事件、滾輪之後 `elementFromPoint` 用的捲動位置 | `python3 scripts/test.py scenarios geometry` |
| 視窗裡（`about:demo`） | getBoundingClientRect 量到 element.style 改過的寬度、`scrollBy` 捲到底、`elementsFromPoint` 列出一點上的元素、`scrollIntoView` 在框裡和視窗裡都把 A4 居中 | `python3 scripts/test.py scenarios demo-script`、`python3 scripts/test.py automation` |
| 視窗那一層（3d 修的） | swap chain 選到每通道 8 bit、不是 sRGB 的格式（Wayland 的 surface 先給 `Rgba16Float`），readback 才讀得回畫面 | `cargo test -p viz --lib the_swap_chain_format_has_eight_bits_a_channel` |
| 視窗那一層（3d 修的） | 真視窗的 `resize`：平台當場改好、不會再來 `Resized` 時（Wayland），也要把新的大小交給 browser、Viz 和 renderer | `python3 scripts/test.py automation resize frames geometry`（只有真視窗走這條路） |
| WPT | `css/geometry/` 的 `DOMRect-*`、`DOMRectList`，和整個 `css/cssom-view/`（227 個 testharness.js、11 個 reftest） | `python3 scripts/test.py wpt -- css/geometry css/cssom-view`、`python3 scripts/test.py reftest wpt/css/cssom-view` |
| WPT | `check-layout-th.js` 解開的 `css/` 測試裡全過的 106 個 | `python3 scripts/test.py wpt -- css/` |

## 做過的檢查

### 找到的 bug

WPT 找到的都先修再記 baseline。

- **根元素的 margin 和子元素摺疊了**（2a）：`body` 的 8px margin 讓 `html` 從 y = 8 開始，CSS 2.2 §8.3.1 和 Chrome 都是 0。根元素的 box 在 taffy 裡用 paint containment 建立 block formatting context（`layout` 的 `role_taffy_style`）；`layout` 的 `root_margins_do_not_collapse_with_the_viewport` 原本記下的是錯的行為，跟著改正。reftest 沒有變化。
- **強制 layout 之後的 hit test 配到舊的 pre-paint**（2b，第 1 段帶進來的）：script 強制 layout 換掉了 `FrameView::fragment`，hit test、滾輪和情境的查詢卻拿它配上一次 paint 的 pre-paint，在 `PrePaint::properties` 查不到而 panic（情境 `geometry` 的頁面每 50ms 改字、讀 `innerWidth`，點擊時撞到）。pre-paint 改成帶著它自己的 fragment（`pre: Option<(Arc<BoxFragment>, PrePaint)>`），hit testing 照畫出來的那一份；2d-1 把 pre-paint 變成 lifecycle 的階段之後，這個不一致整個消失。
- **`scroll` 事件的 listener 插入的 iframe 不會載入**（2c）：scroll steps 之後原本沒有 `start_loads`，iframe 的導覽一直沒開始（WPT 的 `scroll-event-fired-to-iframe` 第二個 subtest 因此逾時，加上之後整個檔案都過了）。
- **`elementsFromPoint` 把文字點到的算成它所在的元素**（2e，WPT 的 `negativeMargins` 抓到）：Blink 的 list 裡只有元素自己的 box，`TreeScope::ElementsFromHitTestResult` 跳過文字節點，anonymous box 也沒有 node。`paint` 的 hit test 目標多一個 `own_box`，`hit_test_all` 只收它；子 frame 的 canvas 也不算。單點的 `hit_test`（`elementFromPoint` 和點擊）不變。
- **Scrollable overflow 的尾端 padding 加在子盒子的 border box 上**（2e，`scrollWidthHeight-overflow-visible-negative-margins` 的 50 個子測試抓到）：改成 Blink 的 `ScrollableOverflowCalculator`：padding box、子盒子的 border box（和它們溢出的），加上 in-flow 內容（行和子盒子的 margin box）外面一層 padding。第一次只對捲動容器加 padding，`scrollWidthHeight-contain-layout` 從通過變成失敗（Chrome 的 `overflow: visible` 也有 padding）；改成每個盒子都加之後 `-margin-collapsing` 失敗：auto 高度的 block 最後一個子盒子穿過底部摺疊的 margin 是它自己的，不在裡面（Blink 放在 margin strut），所以只在 auto 高度時把 margin box 限制在 padding box 裡。這個函式也決定使用者能捲多遠（使用者確認要改共用的函式），reftest 和情境沒有變化。
- **`<body onload>` 這種 window 的 event handler 的 scope**（3c 找到，在 `fix-window-handler-scope` 查清楚後修正）：anchor positioning 的 3 個 WPT 頁面有全域的 `function onload()` 和 `<body onload="onload()">`，在 kun 裡無限遞迴（`Maximum call stack size exceeded`），Chrome 不會。HTML 的「getting the current value of the event handler」在 handler 屬於 window 時 element 是 null，scope 只有 global；Blink 的 `GetCompiledHandler` 也看 event target。在 Chrome 152 裡實際試過：`<body onload>` 裡 `onload` 是全域的函式、`appendChild` 是 `undefined`；`<body onclick>` 照常看得到 `<body>` 的成員。kun 的 `dom_js` 原本一律放 `<body>` 和 document，`onload` 因此指到 handler 自己。修正：`ScriptHost::compile_event_handler` 多一個 `EventHandlerKind`（`Element`、`Window`、`WindowOnError`，取代 `compile_error_event_handler`），`dom_api` 依 target 和事件種類決定，`dom_js` 只對 `Element` 放 context extension（測試 `dom_js` 的 `dispatch::bodys_window_handlers_are_scoped_by_the_global`）。
- **`history.pushState(new SharedArrayBuffer(1))` 在 debug 的 V8 下崩**（3d，完成時的 `matrix` 抓到的，8 組設定都一樣；`dom_js` 的 `bindings` 的 `history_and_its_state`）：`Isolate::Throw` 在已經有 pending exception 時的檢查（`DCHECK(IsTerminationException(exception()))`）。`dom_js` 的 serializer delegate 丟 `DataCloneError` 再回傳 `None`（和 Blink 的 `V8ScriptValueSerializer::GetSharedArrayBufferId` 一樣），而 `v8` crate 的 shim 把 `None` 轉成「呼叫 V8 自己的 delegate，由它丟 `DataCloneError`」（`binding.cc` 的註解就這樣寫），所以丟了兩次；release 的 V8 留著第一個例外，所以平常的測試看不出來。先在 kun 這一側繞過（丟了例外之後回傳一個 id），再照使用者的要求修到根源：fork 的 `src/binding.cc` 只在沒有 pending exception 時才轉呼叫 V8 自己的 delegate（`isolate->HasPendingException()`，記在 fork 的 `README.machopo.md` 的「Local modifications」和 `crates/dom/soundness.md` 的「可以回報上游的修改」第 5 項），kun 這一側的繞法拿掉。動到 C++，所以用 `ci.py v8` 在本機重編 debug 和 release 的 V8，之後的驗收都用 `RUSTY_V8_ARCHIVE` 連本機這份；prebuilt 的 release 還沒發（見下面的「完成時的驗收」）。wasm 模組那一個 delegate 的 `None` 直接回到 V8，沒有這個問題。
- **真視窗在 Linux 上一有 `capture` 就讓 Viz thread panic**（3d，`test.py automation` 第一次在 Linux 上跑時發現）：wgpu 的驗證錯誤（`bytes_per_row` 比一整列小）。`viz` 原本取「surface 提供的第一個不是 sRGB 的格式」，而這台的 Wayland surface 提供 `[Bgra8UnormSrgb, Rgba8UnormSrgb, Rgba16Float, Rgba16Unorm, Rgb10a2Unorm, Bgra8Unorm, Rgba8Unorm]`，選到的是每像素 8 byte 的 `Rgba16Float`，而 readback 假設 4 byte（macOS 的第一個非 sRGB 格式就是 `Bgra8Unorm`，所以一直沒遇到）。改成 `swap_chain_format`：先找每通道 8 bit 的（`Bgra8Unorm`、`Rgba8Unorm`），找不到才退回原本的規則；readback 另外檢查格式真的是每像素 4 byte，不是就只記 log、不開 readback。automation 從 6/22 變成 17/22。
- **真視窗的 `resize` 在 Wayland 上沒有作用**（3d，同一次 `test.py automation` 發現）：winit 的 Wayland 後端自己改大小、回傳 `Some(目前的大小)`，而且不會再來 `Resized`（macOS、Windows、X11 是把請求交給系統、回 `None`，之後才來 `Resized`）；kun 的 `Input::Resize` 只把「回傳想要的大小」當成「做完了」，而唯一把新大小告訴 browser、Viz 和 renderer 的地方是 `WindowEvent::Resized` 的 handler，所以視窗真的變了、kun 自己不知道（量到的：`now=1365x1024`、`wanted=1067x800`、`applied=1067x800`、之後 `inner_size()` 是 1067x800）。改成當場套用的情況就地呼叫 `browser.resize`，和那個事件做一樣的事。`geometry` 情境另外改了：它原本在導覽之前 `resize`，而 Wayland 的 compositor 在第一次 configure 時會把它蓋回去（實測 30ms 後又 configure 成原本的大小），所以改成載入之後才 resize。`resize`、`frames`、`geometry` 三份真視窗的情境因此從失敗變成通過。
- **`demo-script` 的紅綠燈檢查在真視窗裡不成立**（3d，不是 kun 的 bug）：它寫 `window size=800x1000`，但真視窗用系統給的大小（這台是 1024×768 CSS，螢幕也只有 768 CSS 高），整個 script box 放不下，中途 `click` 把按鈕捲進視野時頁面就滑下去，`#light` 跑到視窗外，它的區域因此「沒有變化」（截圖看出來的）。第一次改成檢查前加 `scroll-to #light` 把它捲回來，mutation test（讓燈永遠不變綠）發現**在真視窗裡那個檢查變成空的**——捲動本身就讓區域和 capture 的那一幀對不上，不管燈有沒有變綠都「不一樣」。改成把檢查移到最前面：時鐘的「1 s」之後、任何點擊之前（燈在 script 跑後 2 秒變綠），頁面還停在最上面、和 capture 的那一幀對得上。再跑一次 mutation test，windowless 和真視窗都抓到了。
- **`demo-busy` 的測試頁在系統字型下文字溢出到捲動框上**（3d，不是 kun 的 bug）：真視窗裡，主執行緒忙碌時滾輪沒讓第一個盒子動。一路追下去：compositor 的 `scroll_hit_test` 回 `MainThread`，因為兩個捲動框都進了 `main_thread_scroll_hit_test_region`；兩個都是 composited（`overflows`、只有平移、沒有 effect），所以是 `covered`：第二個是綠色方塊蓋住的（情境本來就要測的），第一個是 `<p id="note">idle</p>` 的文字——`<p>` 只有 20px 高，系統字型的行高 21.8px，墨水框到 y≈61.7，壓進第一個框（60 起）1.7px。kun 的行為和 cc 一樣保守（可能被蓋住就交給主執行緒），所以改的是測試頁：按鈕和 note 各 30px 高（盒子的頁面座標不變），文字就不會碰到盒子。windowless 用測試字型，行高塞得進 20px，所以一直沒發現。
- **`about:demo` 兩個捲動的示範在分數縮放的螢幕上對不起來**（3d）：捲動位置對齊 device pixel（第 24 條），所以在 1.3333 倍的螢幕上 `scrollBy(0, 40)` 之後 `scrollTop` 是 39.75、`scrollIntoView` 的 80 是 80.25。2b 的 `scrollBy` 示範和 3d 新加的 scrollIntoView 示範都改成四捨五入再寫進頁面，情境的文字因此和螢幕的縮放無關（1x、2x 的值不變，windowless 的情境沒有變化）。
- **找到、照第 37 條記下不在這份處理的**：`CSS2/normal-flow/containing-block-percent-padding-*` 的 `padding-top: 10%` 沒有反映在高度上（預期 100、得到 50）。

### Mutation test

共 191 條，分 13 輪（1a 8、1c 10、1d 24、1e 27、2a 20、2b 19、2c 13、2d-1 9、2d-2 15、2e 11、3a 20、`<body onload>` 的修正 5、3d 10）。每一輪跑的期間使用者沒有 commit，跑完都確認檔案還原了。抓不到而補了測試的（補完都抓到）：

- `box_transform` 兩邊共用，比對 pre-paint 的測試抓不到「不以中心為準」和「百分比用錯軸」，補手算的值；行內的 box 沒套捲動位移，補「直接含文字的捲動容器」（1c）。
- V8 的 classic script、microtask checkpoint、module、`import()`、wasm 借出的是 `NoLayout`：1a 當時還沒有成員會在 script 裡用到 client，1d 和 1e 有了 `getBoundingClientRect` 之後補齊（外部 script、iframe 的 `load`、parser 觸發的 mutation observer、wasm script 的測試）（1a、1d、1e）。
- 縮放變了沒標記重做 pre-paint，補 `a_new_scale_pre_paints_again`（同樣的 CSS viewport、不同的縮放，捲動位移對齊不同的 device pixel）（2d-1）。
- 「clamp 之後還有待發的 `scroll` 事件就再要一個 frame」被 WPT 那個測試的 rAF 蓋住，補 `layout_clamping_fires_a_scroll_event`（只縮小內容、不讀幾何）（2c）。
- 子 frame 捲動之後 `elementFromPoint` 的點沒加捲動位置，補子 frame 捲動過的情況（2d-2）；子 frame 的 canvas 列進 `elementsFromPoint`、行不算進 scrollable overflow，補 `paint` 的子 frame 測試、文字直接在 outer 裡的情況和 `layout` 的文字溢出（2e）。
- `scrollIntoView` 的 4 條：iframe 加上邊框和 padding（內容框的位置、整個 iframe 兩條，`block: start` 時剛好一樣）、跨三行的 inline 元素、`FrameView` 的單元測試確認旗標（3a）。
- 3d 的 5 條（都在 `renderer` 的 `scroll.rs`，由 `demo-script` 情境抓）：`center` 當成 `start`、viewport 不捲、`nearest_only` 以外也不往外捲、rect 沒換到 padding box、viewport 用 `start` 對齊。第一版的情境只檢查「元素在視窗裡」，抓不到「不往外捲」（點按鈕本來就把框捲進視窗了），改成檢查「在框裡和在視窗裡都居中」之後 5 條都抓到（README 的第 39 條）。
- 3d 的 fork 1 條：把 `binding.cc` 的 `HasPendingException()` 檢查拿掉、重編 debug 的 V8，`history_and_its_state` 又崩在同一個檢查；放回去重編就通過（kun 這一側沒有繞法）。release 的 V8 兩種寫法都通過，所以守住它的只有 `test.py matrix`、`fuzz`。
- 3d 的 viz 2 條：格式選擇改回「第一個不是 sRGB 的」，新的單元測試抓到；readback 不檢查「每像素 4 byte」沒有測試失敗（有了格式的修正之後走不到的防禦，記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」）。`about:demo` 的四捨五入拿掉只有分數縮放的真視窗抓得到（1x、2x 的值一樣），windowless 的情境守不住它。
- 3d 的視窗那一層 1 條：`Input::Resize` 的「當場套用」分支不呼叫 `browser.resize`，`test.py automation` 的 `resize`、`frames`、`geometry` 三份就回到失敗（windowless 的情境和 `cargo test --workspace` 都抓不到：那條路只有真視窗走）。

抓不到、是等價的（記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」）：`dom_rect_list_item` 的上界改成 `>`（超過 `len` 的格子從來沒寫過）、拿掉子 frame 時標記要重做 pre-paint（父 frame 的 layout 一定也換了）、情境的查詢先更新到 pre-paint（查詢本來就等到畫出改變之後）。抓不到、只靠審查的（記進「測試守不住的地方」）：每個 task 之後和 V8 platform task 之後的 microtask checkpoint、`run_tasks` 借出 frame tree，event loop 讀 `scrolled_by_script`。2a 的「根元素沒有 `offsetParent`」那個檢查是多的（根元素沒有祖先元素可以找，Blink 走 layout tree 才需要），直接拿掉。

收尾時也重新跑過一條舊的：「main thread 自己捲動成為 layer 的容器之後只重跑 pre-paint」那一列（RFC 0009 的 2b-2a 留下的）已經被 2d-1 解決——捲動不標記 `ScrollState::needs_pre_paint` 時，`browser` 和 `shell` 有 8 個測試失敗（包括那一列提到的 `a_container_layer_scrolls_while_script_keeps_the_main_thread_busy`），而它寫的 `pre_paint_as_painted` 也已經不存在，所以刪掉那一列。

### 寫測試時自己的錯（留著免得再犯）

`position: absolute` 在 kun 還沒有，測試頁面改用 transform（2d-2）；負 margin 的 layout 測試第一次用 block（兄弟的負 margin 會摺疊），改成和 WPT 一樣的 flex column（2e）；情境裡 `elementFromPoint(5, 5)` 點到沒有 id 的元素（2e）；`FrameView` 的測試忘了 `<body>` 的 8px margin、跨行的 inline 元素比較位置時差了浮點數的誤差、Lua 的檢查用了第一段 script 的區域變數（3a）。

### WPT 的匯入

- **第 1 段**（1e）：`css/geometry/` 和 `css/cssom-view/` 裡測第 1 段成員的 21 個檔案，11 個全過；其他照 baseline（`Range`、`scrollTo`、`toJSON`、`document.fonts`、`position`、SVG，和在行上沒有 box 的 inline 元素——最後一項記進 `docs/limitations.md`）。既有的 12 個測試因為有了 `getBoundingClientRect` 而走得更遠，5 個改寫 baseline，7 個 `moveBefore` 的 CSS transition 測試從立刻失敗變成等 kun 不會發的 `transitionrun` 而逾時，移進 `testharness-skip.txt`。
- **第 2 段**（2e）：`css/cssom-view/` 裡測第 2 段成員的 48 個檔案，22 個全過（修掉上面兩個 bug 之後 `negativeMargins`、`scrollWidthHeight-overflow-visible-negative-margins`、`-contain-layout` 也全過）。其他失敗的原因：window 的 named property（最多）、`URL`、`element.animate`、smooth scroll、table、表單元件、SVG、shadow DOM、`position`、`MouseEvent` 的 `x` / `pageX`；`column-reverse` 往上溢出的內容捲不到，記進 `docs/limitations.md`。
- **第 3 段**（3b，Windows）：`testharness-include.txt` 和 `reftest-include.txt` 改成整個 `css/cssom-view/` 目錄。227 個執行、51 個全過，176 個照失敗寫 baseline（140 個新的）；13 個逾時，連跑三次都一樣、也用 CI 的 `MACHOPO_WPT_TIMEOUT_MULTIPLIER=3` 跑過，照 2e 的做法留著 baseline、沒有進 skip。reftest 11 個，6 個通過、5 個照預期失敗（smooth scroll 和 `background-attachment: fixed`、`writing-mode`、兩個用 `position: absolute` 的方塊蓋住量到的位置、shadow DOM 的 `<slot>`），連跑兩次一樣。通過的 `scrollTop-display-change` 其實沒有測到（script 第一行就用 window 的 named property，丟例外之後什麼都沒捲，畫面正好和 reference 一樣），記在 `tests/wpt/README.md`。
- **`check-layout-th.js`**（3c，Windows）：`css/` 裡用它的 1133 個檔案全部跑一次，依第一個失敗的原因分類（`details.md` 的「`check-layout-th.js` 的評估」）：全過 106（收進清單）、停在 `document.fonts.ready` 294、量到的值不同 634（其中 604 個用了 kun 不支援的 CSS，`position` 最多）、harness 的錯誤 44、少了 API 54、逾時 1。收進的 106 個連跑兩次、也用 CI 的 timeout 倍數跑過都通過，其他 `css/` 測試的結果沒變。

### 每一段跑過的檢查

每一段都跑 `cargo fmt --check`（動到 `sdk/lua/generator` 時連同它）、`cargo clippy --workspace --all-targets -- -D warnings`（沒有警告）、`cargo test --workspace`、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`docs`。

- **第 1 段**（macOS）：`cargo test` 1258 個；`test.py reftest` 525 個通過、63 個照預期失敗；`test.py wpt` 959 個相符、201 個跳過；`test.py scenarios` 44 次；`test.py automation`（2x 的 Retina 螢幕）第一次 `demo-script` 和 `geometry` 失敗，兩份都寫死了 windowless 的視窗大小（真的視窗用螢幕的縮放、一開始 1024 寬），`geometry` 開頭改成先 `resize 800 600`、`about:demo` 把視窗的資訊拆到另一個 span，之後 22 份都通過。rebase 到 `main`（RFC 0015 第 4 段、RFC 0019 接受）之後重跑：`cargo test` 1282 個、`wpt` 979 個（198 個跳過）、reftest 和情境都通過；`main` 新加的 `isolate_teardown` 要 rusty_v8 fork 的 `abe009b`。
- **第 2 段**（macOS）：`cargo test` 1307 個；reftest 525 / 63（scrollable overflow 改了，捲動範圍的 reftest 沒有變化）；`wpt` 1025 個相符、200 個跳過；情境 44 次；`automation` 22 份全部通過。rebase 到 `main`（RFC 0019 的 `URL`、`Location`、`hashchange` 和 GC 的壓力測試）之後重跑：`cargo test` 1355 個、`wpt` 1079 個（233 個跳過）都通過；`about:demo` 多了一列，`demo-script` 的 elementsFromPoint 按鈕改成先用 `scrollBy` 把視窗捲到框（視窗外的點什麼都點不到）。
- **3a**（macOS）：`cargo test` 1361 個、reftest 525 / 63、`wpt` 1079 個相符、情境 44 次。rebase 到 `main`（RFC 0019 的 `History`、`popstate`）之後重跑：`cargo test` 1390 個、`wpt` 1191 個（251 個跳過）、reftest、情境都通過；reftest `w9-lua` 兩邊都加了第 19 項，這裡的改成第 20 項。
- **3b、3c**（Windows 11）：沒有程式碼的改動，沒有跑 mutation test，也沒有重跑 `cargo test --workspace`。`test.py wpt` 3b 時 1352 個執行、3c 收進 106 個之後 1458 個執行，兩次都是 0 個不符；`reftest` 532 個通過、67 個照預期失敗。
- **3d 和完成時的驗收**（2026-10-08，Fedora 44，Linux x86_64，Rust 1.99.0）：見下面。

### 完成時的驗收（Linux）

2026-10-08，Fedora 44（Linux x86_64，Rust 1.99.0），`main` 的 `9ee60f2` 加上 3d 的改動：

- `cargo fmt --check`（連同 `sdk/lua/generator`）、clippy 沒有警告、`cargo test --workspace` 1404 個全部通過、`sanity.py` 的 `deps`、`boundary`、`unsafe`、`docs` 都通過。
- `test.py wpt`：1529 個執行、0 個不符、206 個跳過。`test.py reftest`：542 個通過、67 個照預期失敗、0 個意外。`test.py scenarios`：44 次（22 份，各兩種快捷鍵）全部通過。
- `test.py matrix`：8 組設定全部通過（每組 627 個測試、70 個 test binary）。第一次跑時 8 組都在 `dom_js` 的 `bindings` 的 `history_and_its_state` 崩（上面的 `SharedArrayBuffer` 那個 bug）；修到根源（fork 的 `binding.cc`）、用 `ci.py v8` 在本機重編 V8 之後 8/8 通過，kun 這一側沒有繞法。
- `test.py fuzz --iterations 40 --minutes 45`：40 組全部通過。先用 kun 這一側的繞法跑過一次（seed `0xd0f5f898`），`binding.cc` 修好、重編 V8 之後再跑一次，也是 40/40（這一次只留下結尾，沒有記到 seed）。
- 這一輪連的 V8 是 `ci.py v8` 在本機編的（`RUSTY_V8_ARCHIVE`），內容和 fork 的 `8703527` 一樣；fork 之後發了 `kun-v152.2.0-7`，`main` 也換過去了（下面 rebase 的那一段用的就是它的 prebuilt）。
- `test.py automation`（真的視窗，Linux / Wayland / Mesa 第一次跑）：**22 份全部通過**（一開始只有 6 份）。中間修了兩個視窗那一層的 bug（swap chain 的格式、`resize` 不通知 browser）和三份情境的假設（`geometry` 在 configure 之前 resize、`demo-script` 的紅綠燈被捲出視窗、`demo-busy` 的測試頁文字溢出到捲動框上）。windowless 的 44 次也照舊通過。
- 平台的差別：1a 到 3a 的 WPT 和 reftest 在 macOS 上跑、3b 和 3c 在 Windows 上，這一次在 Linux 上，三個平台都沒有意外的差異。3b 留下的「在 macOS 上重跑一次 `test.py wpt` 和 `reftest`」還沒做（README 的第 40 條）：字型和時間的差別由 `main` 上每天的 CI 顧。macOS 上的 `test.py automation` 是第 2 段時跑的 22/22。

### rebase 到 `main` 之後重跑（Linux）

2026-10-08，rebase 到 `main` 的 `1212b32`（RFC 0017 的五個階段、RFC 0021 的接受，和換到 rusty_v8 的 `kun-v152.2.0-7`：這份 RFC 的 `SharedArrayBuffer` 修正就在裡面）之後，不設 `RUSTY_V8_ARCHIVE`、連 release 出來的 prebuilt 重跑：

- 衝突只有兩處，都是文件：`rfcs/README.md` 的 0017（`main` 改成已完成）和 0018 兩列，`docs/test-gaps.md` 的「等價、不用處理的 mutation」（`main` 加了 RFC 0017 的兩條，這裡加了 viz 的一條，合起來）。
- `ci.py third-party` 通過（fork 在 `kun-v152.2.0-7`）；`cargo fmt --check`、clippy 沒有警告、`cargo test --workspace` 1438 個全部通過、`sanity.py` 四項通過。
- `test.py wpt`：1598 個執行、0 個不符、206 個跳過。`test.py reftest`：542 個通過、67 個照預期失敗、0 個意外。`test.py scenarios`：44 次全部通過。
- `test.py matrix`：8 組全部通過（每組 655 個測試、77 個 test binary）。`test.py fuzz --iterations 40 --minutes 45`：40 組全部通過。兩個都連 release 出來的 prebuilt，也就是帶著這份 RFC 的 `SharedArrayBuffer` 修正的那一版 V8。
- `test.py automation`（真的視窗）：22 份全部通過，和 rebase 之前一樣。
