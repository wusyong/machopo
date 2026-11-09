# RFC 0009 的進度：iframe 和多個 browsing context

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-10-06。完成：2026-10-08，macOS arm64（Rust 1.99.0）；第 1 階段和 2a 在 Windows 11 上做（GC 的檢查在 macOS 上），2b 在 macOS 上。第 3 階段（3a、3b）移到 README 的「之後的可能」（「已決定的事項」第 59 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a 每個 window 的 realm：document 的 realm、wrapper 建在物件的 realm、同源的 security token、測試的第二個 window | 完成 | 2026-10-06 |
| 1b timer 和 rAF 記得自己的 window | 完成 | 2026-10-06 |
| 1c 從元素的 document 找 frame：`<a>` 的換頁、event handler 的編譯 | 完成 | 2026-10-06 |
| 1d `Window` 的 fully active 狀態 | 完成 | 2026-10-06 |
| 1e frame 的 id 和 document 的 id | 完成 | 2026-10-06 |
| 1f mutation test 的補齊、文件 | 完成 | 2026-10-06 |
| 2a-1 同步建子 frame：insertion steps 的 hook、在 script 中建 realm、renderer 的 frame tree、初始的 `about:blank`、`contentWindow` 等成員 | 完成 | 2026-10-06 |
| 2a-2 `src`、`srcdoc` 的載入、`load` 的順序、移除時丟棄、跨源 | 完成 | 2026-10-06 |
| 2a-3 WPT（放回等 iframe 的測試）、mutation test、文件（README 的「和 Chromium 不同的地方」裡 2a 的幾項寫進 `docs/limitations.md`） | 完成 | 2026-10-06 |
| 2b-1a 每份 document 一個 `StyleEngine`、看自己 document 的 style flag | 完成 | 2026-10-07 |
| 2b-1b iframe 是 replaced element（300×150、`width` / `height`、`inset` 的邊框） | 完成 | 2026-10-07 |
| 2b-1c 子 frame 的 style 和 layout、情境的 `frame` 指令 | 完成 | 2026-10-07 |
| 2b-2a 子 frame paint 進父 frame | 完成 | 2026-10-07 |
| 2b-2b reftest 和 WPT、mutation test、文件 | 完成 | 2026-10-07 |
| 2b-3a hit test 和滑鼠事件進子 frame、子 frame 的 `#fragment` 連結 | 完成 | 2026-10-07 |
| 2b-3b 子 frame 的捲動 | 完成 | 2026-10-07 |
| 2b-3c 情境、reftest、WPT、mutation test、文件 | 完成 | 2026-10-08 |
| 3a 子 frame 的導覽和歷史經過 browser | 移到「之後的可能」（第 59 條） | |
| 3b `WindowProxy` 的身分 | 移到「之後的可能」（第 59 條） | |

## 驗收

2026-10-06 在 Windows 11 上：fmt、clippy（沒有警告）、`cargo test --workspace` 843 個（`wpt_testharness` 另計：429 個都和預期相符、139 個不跑，和第 1 階段之前一樣）、`sanity.py deps`、`unsafe`、`docs`。只有一個 window 的頁面行為不變。GC 的檢查在 macOS 上用 debug 的 V8（有 DCHECK）：`test.py matrix`、`test.py fuzz --minutes 60`，都通過（2026-10-06）。

| 項目 | 怎麼驗證 |
| --- | --- |
| document 的 realm window：有 window 的是自己的、沒有的由建立它的一方設、template contents owner document 沿用；只靠 inert document 撐住的 realm window 和它的 document 經過 GC 還在；GC 的 workload 用到這條 edge | `cargo test -p dom --test tree`（`documents_have_a_realm_window`）；`cargo test -p dom`（`gc_stress` 等）；`python3 scripts/test.py matrix`、`fuzz`（macOS） |
| `createHTMLDocument`、`createDocument` 用 `implementation` 的 document 的 realm，`new Document()` 用執行中的 realm，複製的 document 用原本的 | `cargo test -p dom_api --test api`（`documents_take_their_makers_realm`） |
| timer 和 animation frame 記著擁有者（window 的 document）；別的 window 清不掉、取消不了；清掉一份 document 只清它的 | `cargo test -p dom --test timers`（`timers_belong_to_their_window`） |
| `this` 的 window 擁有 timer，不是執行中的 realm 的（一個 realm 自己的 `clearTimeout`、`requestAnimationFrame` 配上另一個 window 當 `this`） | `cargo test -p dom_js --test realms`（`timers_belong_to_the_window_they_are_set_on`） |
| document 到 frame 的表：登記、查、拿掉；兩個 engine 的 `create_realm` 都登記 | `cargo test -p dom --test tree`（`documents_are_shown_in_frames`）；`cargo test -p dom_wasm --test engine`（`a_realm_records_its_documents_frame`）；`cargo test -p dom_js --test realms` |
| `<a>` 的換頁交給它的 document 的 frame，不是執行中的 realm 的；event handler 在元素的 realm 裡編譯，scope 裡是元素的 document（不加前綴的 `URL` 是 `document.URL`） | `cargo test -p dom_api --test events`（`links_are_followed_only_in_a_document_with_a_window`）；`cargo test -p dom_js --test realms`（`links_and_handlers_belong_to_their_elements_document`） |
| fully active：有 window 而且沒被丟棄；`discard_document` 之後 window 還在、不再 fully active，它的 timer、animation frame、frame 都清掉，別的 document 的不動 | `cargo test -p dom --test tree`（`discarding_a_document`） |
| 被丟棄的 document 裡 `<a>` 不換頁、`onclick` 不編譯 | `cargo test -p dom_api --test events`（`a_discarded_document_runs_no_handlers_and_follows_no_links`）；`cargo test -p dom_js --test realms`（`a_discarded_documents_links_and_handlers_do_nothing`） |
| 「哪份 document」用 renderer 給的 `DocumentId`：新的頁面從頂端開始、舊頁面交出的捲動不移動新頁面、舊頁面還在畫面上時的點擊不打到新頁面 | `cargo test -p browser --test navigation`（`a_new_page_starts_at_the_top_whatever_the_old_one_scrolled`、`scrolling_handed_over_for_the_old_page_doesnt_move_the_new_one`、`a_click_on_the_old_page_doesnt_reach_the_new_one`）；`cargo test -p compositor` |
| 一個 realm 自己的函式用在另一個 realm 的物件上，wrapper 照樣是物件的 realm 的（節點、集合、`NodeList`、`NamedNodeMap`、`DOMTokenList`、`DOMImplementation`）；同源的 realm 互相存取得到，跨源（含 scheme 不同、opaque 的 origin）的被擋 | `cargo test -p dom_js --test realms` |
| HTML 的 `iframe` 變成 connected、disconnected 時照樹的順序記下（子樹、搬移是先出後進；不在 document 裡、template 的內容、別的 namespace 不記）；iframe 和 content document 的對應；丟棄 document 時連 iframe 的 document 一起丟棄；document 的 origin（2a-1） | `cargo test -p dom --test tree`（`iframes_are_recorded_as_they_connect_and_disconnect`、`iframes_and_their_content_documents`、`discarding_a_document_discards_its_iframes_documents`、`documents_keep_their_origin`） |
| 經過宣告表插入的 iframe 在呼叫結束時有子 frame（初始的 `about:blank`、quirks mode、繼承的 origin、frame 被告知）；`contentDocument`、`contentWindow`、`frames`、`length`、`window[i]`、`parent`、`top`、`frameElement`；巢狀；沒有子 frame 的情況；移除時丟棄（setter 也算）、搬移時換新的；跨源的 script 拿到 `null`（2a-1） | `cargo test -p dom_api --test frames` |
| V8 上：iframe 在 `appendChild` 的呼叫中就有另一個 realm 的 window；`window[i]` 的屬性描述和 own key；opaque origin 的父子互相存取得到；子 frame 的 script 插入的 iframe；移除後是 `null`、子 frame 的 timer 清掉（2a-1） | `cargo test -p dom_js --test iframes` |
| renderer：parser 插入的 iframe 在下一個 script 之前就有子 frame；子 window（parser 的、script 的、孫的）的 timer 和 animation frame 在它的 frame 執行（2a-1） | `python3 scripts/test.py scenarios iframes` |
| `src`、`srcdoc` 的屬性改變只在 iframe connected 時記下；插入時照屬性導覽（`srcdoc` 優先、沒有 `src` 或 `about:blank` 時不導覽）、屬性改變時再導覽（規則見第 30 條）；`HTMLIFrameElement` 的 `src`（反映成 URL）、`srcdoc`（2a-2） | `cargo test -p dom --test tree`（`iframe_attribute_changes_are_recorded_while_connected`）；`cargo test -p dom_api --test frames`（`an_inserted_iframe_navigates_by_its_attributes`、`an_iframes_attribute_changes_navigate_it`、`an_iframes_src_is_a_url`） |
| 沒有 `src` 或 `about:blank` 的 iframe 在 `appendChild` 之中分派 trusted、不 bubble 的 `load`（2a-2） | `cargo test -p dom_js --test iframes`（`an_about_blank_iframe_loads_as_it_is_inserted`） |
| 被移除的 iframe 的 realm 的 listener 不再執行（target 的 global：`new EventTarget()`、節點、window；callback 的 global）；callback 的例外在 callback 的 realm 回報；`document.defaultView`；`EventTargetImpl` 記得它的 realm 的 window（GC 的 workload 用到這條 edge）（2a-3） | `cargo test -p dom_js --test iframes`（`listeners_of_a_removed_iframes_realm_dont_run`、`a_callbacks_exception_is_reported_in_its_realm`）；`cargo test -p dom_api --test frames`（`a_documents_default_view`）；`cargo test -p dom --test events`（`event_targets_remember_their_realm_window`）；`python3 scripts/test.py matrix`、`fuzz`（macOS） |
| WPT：原本等 iframe 的測試都跑（失敗的在 baseline），加上 iframe 和 window 目錄裡測 2a 的 10 個檔案（2a-3） | `python3 scripts/test.py wpt` |
| 在真的視窗裡跑 iframe 的兩個情境（2a-3） | `python3 scripts/test.py automation` |
| renderer 載入子 frame：`src`（另一個檔案，跨源）、`srcdoc`（同源、執行 script、不是 quirks mode）、子 frame 自己的子資源；iframe 的 `load` 在 window 的之前、巢狀的孫 frame 先載完；載入失敗照樣 `load`；重新導覽換成新的 window、舊的丟棄；後來的導覽取代進行中的；之後設成 `about:blank`；`srcdoc` 和初始 `about:blank` 裡的相對 `src` 照父頁面解析；載入中被移除的 iframe 不再拖住、也不 `load`；跨語言的子 document 不執行 script（2a-2） | `python3 scripts/test.py scenarios iframe-loads` |
| 每份 document 一個 `StyleEngine`（UA stylesheet 共用），看自己 document 的 style flag（2b-1a） | `cargo test -p style --test invalidation`（`each_document_has_its_own_engine`） |
| iframe 是 replaced element：300×150、沒有 natural ratio、`width` / `height` / `frameborder`、`inset` 的 UA 邊框（畫成 `solid`）、子節點不建 box（2b-1b） | `cargo test -p layout --test inline_block`（`iframes_are_replaced_elements`）、`-p style --test cascade`、`--test cssom`；reftest `frames/f1` |
| 子 frame 照 iframe 的內容框做 style 和 layout（沒有 box 時 0×0），iframe 變大小、子 document 改了都重新 layout；stylesheet 載入中的子 frame 等，之前 layout 過的照上次的畫（2b-1c、2b-2a） | `cargo test -p browser --test navigation`（`query_element::in_child_frames`、`a_child_frame_waits_for_its_stylesheets`）；情境 `frames` |
| 子 frame 畫在 iframe 的內容框裡：canvas、clip 是內容框不含 padding、沒有背景時透明、巢狀、`#fragment` 的捲動、旁邊有成為 layer 的捲動容器、iframe 沒變時子 frame 照樣重畫（2b-2a） | `cargo test -p paint --test child_frames`；reftest `frames/f2` 到 `f6`；`query_element::child_frames_stay_where_they_are_after_a_compositor_scroll` |
| WPT 的 iframe reftest：34 個，20 個 `body-margin-*` 照預期失敗（2b-2b） | `python3 scripts/test.py reftest` |
| hit test 進子 frame（元素、canvas、邊框和 padding、被裁掉的部分、巢狀和 transform）；點擊在子 frame 的 realm 分派、`clientX` / `clientY`；游標；`#fragment` 連結捲動、中鍵和換頁不做事（2b-3a） | `cargo test -p paint --test child_frames`（`hit_testing_*`）；`query_element::clicks_go_into_child_frames` |
| 子 frame 的 viewport 和容器是 scroll node；滾輪由 compositor 或 main thread 捲子 frame、捲到底往外傳；孫 frame；`scrolling="no"`、`overflow: hidden`；被蓋到時交給 main thread（2b-3b） | `cargo test -p paint --test child_frames`、`-p renderer --lib`（`iframes_scrolling`）；`query_element::wheels_scroll_child_frames`、`wheels_scroll_nested_child_frames` |
| 自動化：`QueryElement` 和 `ScrollIntoViewIfNeeded` 帶 `frames`，從裡到外捲；情境的 `frame` 指令，子 frame 裡的 `click`、`wheel`、`scroll-to`（2b-1c、2b-3c） | `query_element::an_element_in_a_child_frame_is_scrolled_into_view`；`cargo test -p shell --lib`（`scenario::tests::frames`、`automation::tests::a_child_frames_element_is_scrolled_to_and_clicked`）；情境 `frames` |
| 在真的視窗裡跑 iframe 的三個情境，看截圖 `frames`、`frames-far`（2b） | `python3 scripts/test.py automation` |

## 做過的檢查

2b 最後一次（2026-10-08，macOS arm64）：fmt、clippy（沒有警告）、`cargo test --workspace` 1227 個、`sanity.py deps`、`boundary`、`unsafe`、`docs`；`test.py reftest` 525 個通過、63 個照預期失敗；`test.py wpt` 945 個都和預期相符；`test.py scenarios` 42 次；`test.py automation` 21 個情境。

- **Mutation test**：共 240 個（第 1 階段 51：1a 20、1b 10、1c 10、1d 7、1e 4；2a 95：2a-1 52、2a-2 32、2a-3 11；2b 94：2b-1a 2、2b-1b 8、2b-1c 20、2b-2a 23、2b-3a 12、2b-3b 19、2b-3c 10）。一開始沒抓到、補了測試之後抓到的，大多是測試的情境碰巧讓兩種做法一樣：一個 realm 用另一個 realm 自己的函式（1a、1b，改成 `Document.prototype.createElement.call(other.document, …)` 這種寫法）、舊頁面還在畫面上時的點擊（1e）、兩個並排的 iframe（2a-1）、iframe 不在捲動容器裡（2b-3b）、元素在視窗外（看不出子 frame 的 viewport 那一層，2b-3c）等。等價的記在 `docs/test-gaps.md` 的「等價、不用處理的 mutation」：renderer 只有一個 window 時導覽丟棄和執行時檢查互相涵蓋（1b 到 1d）、子 frame 的連結只差一行 log（2b-3a）。測不到、記在「能補、還沒補的測試空缺」的：載入失敗的 document 的 opaque origin（2a-2）、沒有 box 的 iframe 照樣 layout（2b-1c，等 D 的 layout 查詢）、main thread 捲動成為 layer 的容器之後的 pre-paint（2b-2a，P4.2 起就是）、子 frame 的點擊分派的 realm（2b-3a，等 F 的 `MouseEvent.view`）；只靠審查的：`FrameView` drop 時丟掉還沒收進 frame tree 的子 frame（2a-1）。
- **找到、修掉的 bug**：V8 的 security token 比對的是同一個物件，不是內容（1a，改用 internalized 的字串）；`srcdoc` 和初始 `about:blank` 用自己的網址當 base URL（2a-2）；`DomIsolate` drop 時 embedder 的 frame 拿著 root（2a-1）；WIT 的 `HTMLIFrameElement` 名稱（2a-1）。2b 的設計在對照 Blink 時改了一處：第 44 條原本「沒有 box 時不跑 layout」，Blink 不 throttle 同 process 的 `display: none` 的 frame，改成 0×0 照樣跑。
- **WPT**：testharness.js 從 429 個（139 個不跑）到 476 個（102 個不跑，2a-3 放回等 iframe 的測試、加上 the-iframe-element 和 the-window-object 裡的 10 個檔案），baseline 只有失敗變少；2b 沒有改變 testharness.js 的結果。reftest 加了 34 個有 iframe 的（「已決定的事項」第 51、52 條；挑法見 `tests/wpt/README.md`），第一次試跑 46 個。
- **GC 的檢查**（macOS，debug 的 V8）：動到 GC 的 1a（document 的 `realm` edge）和 2a-3（`EventTargetImpl` 的 `realm` edge）各在做完時跑了 `test.py matrix` 和 `test.py fuzz --minutes 60`，都通過；2b 沒有動到 GC，完成時不再跑（第 59 條）。
- **真的視窗**（`test.py automation`）：2a-3、2b-1c、2b-2b、2b-3c 都跑過，看過 `frames`、`frames-far` 的截圖（子 frame、孫 frame 畫在 iframe 裡，超出的被裁掉；捲到遠處的子 frame 點到按鈕）。
- **不穩定的測試**：情境 `demo-script` 在整套測試平行跑時，`click` 之後的 `wait-change` 可能拿到已經改過的文字當基準（2a-3，和 iframe 無關，情境的寫法有競態；單獨跑都通過）。
- **測試寫錯、不是 bug 的**：子 frame 的 body 有預設的 8px margin、body 的 margin 讓按鈕不在 (10, 5)、滾輪放在子 frame 的捲動容器上（容器先吃掉）、縮放的 iframe 畫在中心的一半。
