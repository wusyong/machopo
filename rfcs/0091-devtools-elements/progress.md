# RFC 0091 的進度：DevTools 的 Elements：`DOM`、`CSS` 和 `Overlay`

<!-- 排程（開始實作）時複製進 RFC 的資料夾。怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-09，macOS（Apple Silicon）。在 `0091` 上做。

完成日期：2026-11-10，macOS（Apple M4）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1 原型：停住時借 `Dom` 和 layout、停住時重算 style 和 layout（第 5 節） | 完成 | 2026-11-09 |
| 1a 樹：nodeId、lazy push、改動的掛點和事件、停住時回答、不用 JS 的查詢（第 1 到 5 節） | 完成 | 2026-11-09 |
| 1b 和 JS 的連接：`resolveNode`、`requestNode`、`$0`、Console 顯示節點、Event Listeners、`performSearch`（第 6 節） | 完成 | 2026-11-09 |
| 2a Overlay：框節點、選取元素、`getBoxModel`、`getContentQuads`、`getNodeForLocation` | 完成 | 2026-11-09 |
| 2b Computed：`CSS.enable`、`getComputedStyleForNode`、`getPlatformFontsForNode`、元素自己的規則、跟著更新、標籤的樣式 | 完成 | 2026-11-09 |
| 2c 停住時照樣畫、「Paused in debugger」：scheduler、畫 frame 的狀態搬進 `Frame` | 完成 | 2026-11-09 |
| 3a stylesheet：`styleSheetAdded`、`styleSheetRemoved`、規則的 `styleSheetId`、`getStyleSheetText`、`<link>` 的原文和再抓、`<style>` 的位置 | 完成 | 2026-11-09 |
| 3b 位置：`range`、劃掉的宣告、`@media`、`@supports`、inline style 的 stylesheet、`styleSheetChanged` | 完成 | 2026-11-09 |
| 3c 其他的規則：繼承來的、pseudo element 的規則和節點、`getAnimatedStylesForNode`、`@keyframes` | 完成 | 2026-11-10 |
| 3d `:hov`：`forcePseudoState` | 完成 | 2026-11-10 |

## 驗收

**停住時：借 DOM 和 layout、scheduler、照樣畫**

| 項目 | 怎麼驗證 |
| --- | --- |
| 停住時 host 讀得到 DOM：停在 script、`click()` 呼叫的 listener、`createElement` 裡的 custom element constructor、MutationObserver 的 callback，每次都是 script 到那時為止的 DOM | `cargo test -p dom_js --test it devtools::the_host_reads_the_dom_while_paused` |
| 停住時先送 script 到那時為止的改動，命令照那時的 DOM 回答 | `cargo test -p shell --test it devtools_dom::the_dom_is_there_while_paused` |
| 停住時也給得出節點的物件 | `cargo test -p shell --test it devtools_dom::the_dom_is_there_while_paused` |
| 停住時 renderer 拿得到 layout、重算之後的 box 跟著 script 的改動：停在 `Runtime.evaluate` 的 script、`setTimeout`、`click()` 和 `focus()` 呼叫的 listener、`requestAnimationFrame`、ResizeObserver 的 callback 時 `getBoxModel` 的高度（1a 是原型的命令，2a 換成 `getBoxModel`） | `cargo test -p shell --test it devtools_overlay::box_models_are_there_while_paused` |
| 停住時的 computed 和規則照 script 到那時為止的改動 | `cargo test -p shell --test it devtools_css::styles_are_there_while_paused` |
| main thread 的 scheduler：每個 queue 的 task 照送進來的順序；停住時只取 DevTools 和 rendering 的（main frame、`RequestVisualState`），其他的（Browser 的訊息、輸入、canvas）留著、不亂順序；等的時候看得到之後送來的、有 timeout 的會放棄；要打斷 script 的照舊單獨取出；停住留下的放回最前面；main thread 走了之後送不進去 | `cargo test -p renderer --lib task_queue` |
| 停住時照樣畫：Console 的改動、框的節點、拿掉的框；`setPausedInDebuggerMessage` 的提示（頁面蓋上一層、上方中間的淡黃色框），`Overlay` 關著的 session 的不畫、拿掉之後沒有；繼續之後照常畫 | `cargo test -p shell --test it devtools_overlay::the_page_is_drawn_while_paused` |
| 停住時的輸入等到繼續才處理 | `cargo test -p shell --test it devtools_overlay::input_waits_for_the_pause` |
| 停住時滾輪在 compositor 上捲動：停住時畫的 frame 留著捲動的位置、script 讀得到 | `cargo test -p shell --test it devtools_overlay::a_scroll_while_paused_stays` |
| scheduler 和 widget 搬家之後，視窗裡的情境和 pipe 照舊 | `python3 scripts/test.py automation` |

**DOM 樹和改動的事件**

| 項目 | 怎麼驗證 |
| --- | --- |
| DOM 記下樹的改動：有人要時依序記插入（連同前一個兄弟節點）、移除、屬性、文字、inline style、shadow root，拿走一次；沒人要時不記，不要了時丟掉記著的 | `cargo test -p dom --test it probes::` |
| `getDocument` 預設兩層、每個容器帶子節點數、只有空白的文字不送、只有一個文字子節點的元素帶著它、root 元素帶 frame；`requestChildNodes` 的 `setChildNodes` 在回答之前；深度 0 是錯誤 | `cargo test -p shell --test it devtools_dom::the_document_is_sent_as_asked_for` |
| script 的改動對 session 有的節點送事件：插入（前一個是 session 有的最近的兄弟節點）、屬性的設定和移除、文字、沒送子節點的節點的數量、inline style、shadow root（先送 host 的子節點）、移除；只有空白的文字不送 | `cargo test -p shell --test it devtools_dom::script_changes_are_told` |
| 一批的改動照一批結束時的 DOM：倒過來插入的節點照 DOM 的順序排，插入又拿掉的不送 | `cargo test -p shell --test it devtools_dom::a_batch_is_told_in_the_doms_order` |
| 查詢送出找到的節點以上的路；`describeNode` 用 backend id、不給新的編號；`getOuterHTML`、`getAttributes`、`collectClassNamesFromSubtree`、`pushNodesByBackendIdsToFrontend`（找不到的是 0）；iframe 的 frame、文件和 `getFrameOwner`；錯誤的文字 | `cargo test -p shell --test it devtools_dom::queries_send_the_path_to_what_they_find` |
| iframe 換了文件時送它的移除和插入；main frame 的新文件解析完時送 `documentUpdated`，舊的編號不再有效 | `cargo test -p shell --test it devtools_dom::new_documents_are_told` |
| 每個 session 的編號各自從 1 起；`disable` 兩次是錯誤；最後一個打開 `DOM` 的 session 關掉或離開時 DOM 不再記改動 | `cargo test -p shell --test it devtools_dom::each_session_has_its_own_numbers`、`cargo test -p renderer --lib dom_agent::` |
| 和 Chrome 一樣：樹、`setChildNodes`、每個 task 一個改動時的事件、查詢、錯誤 | `python3 scripts/test.py cdp dom` |

**節點和 JS 的連接**

| 項目 | 怎麼驗證 |
| --- | --- |
| Console 的節點的物件是 `node`，名字照 Chrome（`div#a.x.y`、`#document`、`<!DOCTYPE html>`、`#text`）；物件的節點（連同到它的路）、節點的物件（照 nodeId 和 backend id，在要的 object group 裡）；錯誤的文字；`$0`；不在文件的樹裡的節點以 `parentId` 0 送它的子樹的根，script 放掉、GC 跑過之後 session 照樣留著 | `cargo test -p shell --test it devtools_dom::nodes_and_their_objects_are_linked` |
| listener：capture 的在前、照型別最早加入的順序、屬性的 handler 先編譯、物件的 `handleEvent`、旗標、位置、物件包在要的 group、`depth` 以下的節點、window 的沒有節點 | `cargo test -p shell --test it devtools_dom::event_listeners_are_listed` |
| 搜尋：selector、文字、標籤（`<p>` 整個名字）、屬性的名字和值（`"code"` 整個值）；結果照要的範圍送出、範圍檢查、丟掉 | `cargo test -p shell --test it devtools_dom::the_document_is_searched` |
| 和 Chrome 一樣：Console 的節點、`requestNode`、`resolveNode`、`$0`、detached 的節點、listener、搜尋（只差 `HTMLDocument`，已知） | `python3 scripts/test.py cdp dom_objects` |

**Overlay 和盒模型**

| 項目 | 怎麼驗證 |
| --- | --- |
| 盒模型：四個框的 quad（margin 在 border 外）、border box 的大小；捲動、fixed、旋轉、iframe 裡的；行內元素（Ahem）；content quad；沒有 box 的；`isScrollable` | `cargo test -p shell --test it devtools_overlay::nodes_have_box_models` |
| 某一點的節點：元素、文字的元素、iframe 裡的（和它的 frame）、user-agent shadow tree 的歸到 host（要的時候不歸）、viewport 外的找不到、捲動之後照文件的座標 | `cargo test -p shell --test it devtools_overlay::nodes_are_found_at_points` |
| overlay：要先 `DOM`；沒有 config 是 invalid params；照要的顏色畫出四個框（每個框只畫它和裡面那個框之間的一圈，2b 改）、標籤畫在下面、不要資訊時沒有標籤；跟著 script 移動的元素；`hideHighlight`；`getHighlightObjectForTest` 的路徑、顏色、外框、`elementInfo` | `cargo test -p shell --test it devtools_overlay::highlighted_nodes_are_drawn` |
| 選取元素的模式：未知的模式是錯誤；滑鼠移動框出底下的元素、送 `nodeHighlightRequested`；按下送 `inspectNodeRequested`；頁面收不到；關掉之後框拿掉、頁面收得到 | `cargo test -p shell --test it devtools_overlay::the_inspect_mode_takes_the_mouse` |
| 不畫的 overlay：要求不畫時成功，要求畫時是錯誤 | `cargo test -p shell --test it devtools_overlay::other_overlays_are_answered` |
| 和 Chrome 一樣：絕對定位和旋轉的元素的盒模型、content quad、highlight 的資料、某一點的節點、`isScrollable`、錯誤（只差 accessibility 的欄位、`layoutObjectName` 和錯誤的 data 裡的位置，已知） | `python3 scripts/test.py cdp overlay` |

**Computed 和元素自己的規則**

| 項目 | 怎麼驗證 |
| --- | --- |
| 「說明」：元素符合的規則照 cascade 的順序（UA 的在前、`!important` 的在它一般的位置）、各帶符合的選擇器（pseudo element 的不算、bloom filter 有祖先）、presentational hint 和 inline style 分開；shadow tree 的、host 的 `:host`、slot 的 `::slotted()`；adopt 兩次的規則列兩次；不在樹上的、別的文件的、文字節點沒有；不留下東西 | `cargo test -p style --test it explain::` |
| DevTools 列的選擇器（文字、specificity）和宣告（值、priority、implicit、設定它的 shorthand） | `cargo test -p style_resolver --test it -- selectors_are_listed declarations_are_listed` |
| 字型：照第一次出現的順序、問的 run 的 glyph 數、`name` table 的名字 | `cargo test -p layout --test it platform_fonts::` |
| `CSS` 要先 `DOM`、`enable` 之前和 `disable` 之後不回答 | `cargo test -p shell --test it devtools_css::css_needs_dom_and_to_be_on` |
| computed：`getComputedStyle` 的每個 property（數量一樣）、layout 決定的值、自訂 property、文字節點的是父元素的、`display: none` 的、跟著 script、沒有 frame 的沒有 | `cargo test -p shell --test it devtools_css::computed_styles_are_read` |
| 套用的規則：順序、符合的選擇器、specificity、宣告和 shorthand、`!important`、inline style、presentational hint、空的 inline style、layout 上的父元素（跳過 `display: contents`）、`:host`、錯誤 | `cargo test -p shell --test it devtools_css::matched_rules_are_listed` |
| 字型：元素自己、子元素的文字、產生的內容；不算孫元素的；文字節點的 | `cargo test -p shell --test it devtools_css::fonts_are_counted` |
| 跟著 computed style：自己的改動、繼承的改動送事件；別的節點不送；停了之後不送 | `cargo test -p shell --test it devtools_css::computed_style_updates_are_told` |
| 標籤的樣式：顏色一律 `#RRGGBBAA` 和分數、字型、padding、margin、背景；沒有文字的元素沒有顏色和字型；畫出那幾列（黑色的值） | `cargo test -p shell --test it devtools_css::labels_show_styles`、`cargo test -p renderer --lib overlay_agent` |
| 和 Chrome 一樣：套用的規則（作者的照選擇器、specificity、符合的選擇器、longhand 的值；UA 的一條完整比）、inline style、presentational hint、layout 上的父元素、回答的 key、computed 的值、字型、`computedStyleUpdated`、標籤的樣式、錯誤（只差 `!important` 寫在值裡，已知） | `python3 scripts/test.py cdp css` |

**stylesheet、原文和位置**

| 項目 | 怎麼驗證 |
| --- | --- |
| 規則屬於哪一份 stylesheet：併成一個 index 的幾份小的各是各的、同樣文字的兩個 `<style>` 各是各的、adopted 的、拿掉再放回去的 `<style>` 是新的；大的（自己一個 index）、shadow tree 的、host 的 `:host`、slot 的 `::slotted()` 是它的 shadow root 的那一份（不套用的那份不算） | `cargo test -p style --test it -- explain::rules_know_their_sheets explain::large_and_shadow_sheets_are_known` |
| parser 記下 `<style>` 的開始標籤結束的那一行（跨行的標籤是最後一行），script 建的沒有 | `cargo test -p html --test it sink::the_document_parser_notes_where_styles_start` |
| `<style>` 的文字在原文裡的位置：那一行的 `<style` 的 `>` 之後（UTF-16 的欄），幾個裡選文字接得上的、都接不上時第一個，CR LF，文字裡的 `>`；結束的位置 | `cargo test -p renderer --lib css_agent` |
| stylesheet 的 header 和原文：`<link>`（原文、`ownerNode`、`loadingFailed`）、`<style>`（位置、`title`）、adopted（序列化）、shadow root 的；同一個 frame、id 不重複；規則和它的 style 帶著它的 stylesheet 的 id，UA 的沒有；找不到的 id；連上時載入的 `<link>` 不再抓 | `cargo test -p shell --test it devtools_css::style_sheets_are_told_with_their_texts` |
| stylesheet 的來去：`<style>` 的文字改了先送移除再送一份新的、`<link>` 拿掉、adopt 一份、iframe 的和它的改動；沒有改動不送；`disable` 之後不送，再 `enable` 送現在的 | `cargo test -p shell --test it devtools_css::sheets_coming_and_going_are_told` |
| 連上之前載入的 `<link>`：第一次要時照它的 `<link>` 再抓（`crossorigin` 用 CORS），同時要的兩次一起等一次，規則一樣時給原文、改過時給序列化的，之後不再抓 | `cargo test -p shell --test it devtools_css::link_sheets_loaded_before_devtools_are_fetched_again` |
| 和 Chrome 一樣：每份 stylesheet 的 header、原文、規則的 `styleSheetId`、`<style>` 的文字改了和 `<link>` 拿掉時的事件、找不到的 id（只差 `innerHTML` 建的 `<style>` 的位置，已知） | `python3 scripts/test.py cdp css_sheets` |
| 規則在原文裡的位置：留下的規則照 parser 的順序、巢在 group rule 裡，prelude、每個選擇器（函式裡的逗號不切開）、區塊；丟掉的規則沒有；位置是 byte；宣告照原文（`!important`、解析不了的、註解裡的、最後一個到區塊的結尾、不認得的 property 的註解不算） | `cargo test -p css --lib inspector` |
| 規則的位置（行從 0、欄是 UTF-16）：選擇器（到逗號）和 `selectorList.text`、區塊、宣告照原文（shorthand 的 longhand、`!important`、解析不了的、註解掉的）再照解析的結果；`@media`、`@supports` 從裡到外和 stylesheet 自己的 media、`ruleTypes` | `cargo test -p shell --test it devtools_css::rules_have_their_places` |
| inline style 是一份自己的 stylesheet：同一個元素同一個 id、不送 `styleSheetAdded`、原文是屬性的值、宣告的位置；沒有屬性時是空的；`var()` 的 shorthand 沒有 longhand；script 改了時送 `styleSheetChanged`，沒改時不送 | `cargo test -p shell --test it devtools_css::inline_styles_are_sheets_of_their_own` |
| script 改了 stylesheet 的規則：送 `styleSheetChanged`，原文從此是序列化的規則（`<link>` 的也是），位置照它；`insertRule` 填的空 `<style>` 的原文是它的規則 | `cargo test -p shell --test it devtools_css::edited_sheets_are_their_rules` |
| 補抓的 `<link>`：抓到之前的位置照序列化的原文，抓到之後照原本的原文；script 改過的不抓；抓不到時是序列化的；抓到的規則和現在的一樣時才用 | `cargo test -p shell --test it devtools_css::link_sheets_loaded_before_devtools_are_fetched_again`、`cargo test -p renderer --lib css_agent` |
| 和 Chrome 一樣：規則和 inline style 的位置、照原文的宣告、`@media`、`@supports`、`ruleTypes`、inline style 的原文、CSSOM 改了之後送的事件（只差 `specificity.components`、`mediaList`、改過的 stylesheet 的原文、inline style 改了時送的次數，已知）；套用的規則、computed 等 | `python3 scripts/test.py cdp css_source css` |

**繼承來的規則、pseudo element、動畫**

| 項目 | 怎麼驗證 |
| --- | --- |
| pseudo element 的規則：`::before`、`::after`、`::marker`、`::selection` 各自的規則、符合的選擇器和 stylesheet，和元素自己的分開 | `cargo test -p style --test it explain::pseudo_elements_rules_are_apart` |
| `@keyframes` 的每個 keyframe 在原文裡的位置 | `cargo test -p css --lib inspector` |
| pseudo element 是節點：list item 的 `::marker` 在前，有 `content` 的 `::before`、`::after`（沒有 `content`、`display: none` 的不是），編號、backend id 在所有節點之上、照兩者都描述得出；樣式變了時送來去（只送 session 有的元素的），在 task 裡重算 style（`getComputedStyle`）的照 frame 之後的樣子送；元素拿掉之後就沒了 | `cargo test -p shell --test it devtools_dom::pseudo_elements_are_nodes` |
| 元素的 pseudo element 的規則（`::marker` 只有 list item 的）、祖先的規則和 inline style（近的先）、祖先的 `::selection` 的規則 | `cargo test -p shell --test it devtools_css::pseudo_elements_and_ancestors_rules_are_listed` |
| 選了 pseudo element：它的規則、從它的元素開始繼承、沒有 inline style、layout 上的父元素是它的元素、computed 是它的、字型、`getBoxModel` 是它的盒子、`getInlineStylesForNode` 是錯誤、框和標籤是它的 | `cargo test -p shell --test it devtools_css::a_pseudo_element_is_styled_as_itself` |
| 動畫的樣式：每個動畫各自的值和名字（只有生效中的），transition 合成一組（沒有在跑的時是空的），祖先各一份；`@keyframes` 每個名字一次，名字、keyframe 和宣告的位置 | `cargo test -p shell --test it devtools_css::animations_are_listed` |
| 和 Chrome 一樣：pseudo element 和祖先的規則、pseudo element 的節點、它的樣式和框、來去的事件、`@keyframes`、動畫的樣式的名字和 property（只差 inline box 的高度，已知） | `python3 scripts/test.py cdp css_pseudo` |

**強制的 pseudo-class（`:hov`）**

| 項目 | 怎麼驗證 |
| --- | --- |
| 強制的 pseudo-class：只在那個元素上成立（祖先的 `:hover` 不跟著，`:has()` 跟著）、在它的規則和 computed 裡、script 的 `matches()` 看得到、畫出來；強制 `:focus`、`:focus-within` 時祖先的 `:focus-within` 成立（強制 `:focus` 的元素自己的不成立）；表單狀態的對立面不成立、其他照元素的狀態；`:visited` 收下不生效；不認得的名字不理；每次取代那個元素的整組；不是元素是錯誤，不是陣列是 invalid params | `cargo test -p shell --test it devtools_css::forced_pseudo_classes_match` |
| 強制的 pseudo-class 收回：元素離開它的樹（子樹的也是，連同祖先的 `:focus-within`）、`CSS.disable`、`DOM.disable`、session 離開；兩個 session 的合在一起，一個收回時另一個的還在 | `cargo test -p shell --test it devtools_css::forced_pseudo_classes_go` |
| 強制的 pseudo-class 改了時重算受影響的元素（沒有 `:has()` 讓整份重算的文件）：強制時才來的規則、`:required`、父元素之上的祖先的 `:focus-within`（強制和元素離開樹時）；`:visited` 跟著別的一起強制時也不成立 | `cargo test -p shell --test it devtools_css::forced_pseudo_classes_restyle_what_they_change` |
| 和 Chrome 一樣：強制的 pseudo-class 在 computed、規則和 script 裡，表單狀態和它們的對立面，收回，錯誤（只差 Chrome 沒重算的兩處和錯誤的 data 裡的位置，已知） | `python3 scripts/test.py cdp css_force` |

## 做過的檢查

2026-11-09 到 2026-11-10，macOS（Apple M4），全部在這台上做；和 Chrome 比、手動確認都用 Chrome for Testing 152.0.7977.82。

- **做了什麼**（設計見 `README.md` 的「詳細設計」）：
  - renderer 的三個 agent：`dom_agent`（節點的編號、lazy push、改動的事件、pseudo element 的節點、查詢、和 JS 的連接、Event Listeners、搜尋）、`css_agent`（stylesheet 的清單和原文、套用的規則和位置、computed、字型、動畫的樣式、強制的 pseudo-class）、`overlay_agent`（框、標籤、選取元素、盒模型、某一點的節點、停住時的提示）。
  - `dom`：樹的改動、pseudo element 的來去的掛點（`probe`，有 session 打開 `DOM` 時才記），強制的 pseudo-class，CSSOM 改過的 stylesheet 的旗標，`<style>` 的開始標籤的行。
  - `style`：「說明」的路（`explain`，和 cascade 共用收集 block 的程式，記下符合的選擇器、規則屬於哪一份 stylesheet、pseudo element 的規則），match 時看強制的 pseudo-class。`css`：記位置的模式（`inspector`，同一個 parser 掛 recorder）。`layout`：盒模型、content quad、pseudo element 的框、字型的清單。`animation`：每個動畫單獨的值（`sampled_alone`）。
  - 停住時：原型確認了停在 script 時借得到 `Dom` 和 layout、重算 style 和 layout 沒問題，P1+ 不用重新安排擁有權（第 5 條）。`dom_api` 的 `DevToolsPage` trait（停住時也要經過 isolate 借）；main thread 改成一個 scheduler（`task_queue`，第 16 條），畫 frame 的狀態搬進 `Frame`（第 17 條），停住的迴圈照樣畫。
- **對照 Chrome 改的**（`test.py cdp`，每段開始前也先用 Chrome 試過要做的命令）：一批一批送的改動（第 6 條）；`style` 經過 CSSOM 改時只送 `inlineStyleInvalidated`；viewport 外的點沒有節點；`highlightNode`、`forcePseudoState` 參數錯時是協定那一層的 `-32602`；`outlineColor` 和整個透明色比；回答的形狀（空的 `inlineStyle`、`parentLayoutNodeId`、`cssAtRules`、只在 true 時送的旗標、`-unclamped-rgba`）；選擇器的範圍到逗號、`selectorList.text`、最後一個宣告的範圍、`implicit`、值帶 ` !important`、`var()` 的 shorthand 沒有 longhand、stylesheet 自己的 media 沒有 `mediaList`；改過文字的 `<style>` 的位置照開始標籤找；`::marker` 只有 list item 的、`transitionsStyle` 一律送；強制 `:focus-within` 也讓祖先的成立。剩下的差異在 `cdp_compare.py` 的 `KNOWN`（`HTMLDocument`、accessibility 的欄位、`specificity.components`、`mediaList`、改過的 stylesheet 的原文、inline style 改了時送的次數、`innerHTML` 建的 `<style>` 的位置、inline box 的高度、Chrome 沒重算 style 的兩處、錯誤的 data 裡 CBOR 的位置），限制寫在 `docs/limitations.md`。
- **做的時候的發現**：
  - 標籤畫好之後和 Chrome 並排，machopo 的 margin 蓋住整個元素：`inspector_overlay` 每個框挖掉裡面那個，改成 even-odd 的路徑只畫那一圈，2a 的測試原本把錯的畫法寫成預期；標籤的列、顏色的格式（Blink 一律送 `#RRGGBBAA`）和位置（第 1、2 段手動確認時發現，照 `drawElementTitle` 改成 `place_label`）。
  - `hideHighlight` 之後框不拿掉（`layer()` 把「沒有 overlay」當成「沒有變」）；停住時截圖偶爾等不到（停住的迴圈把 `RequestVisualState` 要的 frame 取消了），等著 frame 的 promise 也算要畫。
  - html5ever 給的行是開始標籤結束的那一行；再抓的 `<link>` 照 stylesheet 的內容對回去，不照清單的位置；macOS 的 Times 只有 Macintosh 的 `name` 紀錄。
  - DevTools 前端的做法：Computed 要 `getMatchedStylesForNode` 的結果（元素自己的規則因此提前到第 2 段，第 14 條）；`getAnimatedStylesForNode` 只用在 Computed 的預設清單，Styles 沒有動畫的區塊（Chrome 自己也沒有，3c 手動確認時對照過）；沒有可繼承 property 的祖先不列。
  - 和 Chrome 不同、記成限制的：停住時 CSS 動畫不往前走（第 18 條）；強制的 `:visited` 不生效（第 28 條）；inline box 的高度（20px 的 Times 的 `<span>`，machopo 20、Chrome 23，原因還沒查）。和這份無關、另外處理的：系統字型的 `font` 的 longhand（Chrome 全部是 pending，machopo 只有五個是）。
  - 比對和測試本身的錯：Chrome 的預設字型照系統語言（比對的頁面改用 Times）；拿文字節點時的 `setChildNodes`；規則的 id 包在 tuple 裡沒換成佔位；transition 要等兩個 frame 再問（有一次多一處不同，十幾次重跑沒再出現）；「兩個 frame 之後」沒有 `awaitPromise`；元素移走之後要重新找 `nodeId`；還沒畫過 frame 時 compositor 丟掉輸入，測試先截一張圖。
- **mutation test**（每一段都跑，最後每一條都抓到或記成等價、只靠審查）：
  - `mutants-1a.toml`（34 條）漏 5 條：同一個節點的子節點送兩次、`describeNode` 給編號、`getDocument` 留著舊的編號、移除的節點留著編號、停住時回答之前不先送改動，都補了測試。`mutants-1b.toml`（30 條）漏 3 條：不存在的 backend id、加引號的值，補了測試；不是物件的值回答另一個錯誤是等價的。
  - `mutants-2a.toml`（28 條）漏 2 條：viewport 外的點（改用捲到之前的元素）；overlay 不要 frame（截圖自己要 frame，只靠審查）。`mutants-2b.toml`（39 條）漏 7 條：另一個文件的元素、字型的順序、標籤的值、等兩個 frame 的測試沒真的等，都補了；「說明」設 selector 的 flag 等價；只有 Macintosh 名字的字型只靠審查（`test.py cdp css` 守著）；同一個選擇器記兩次補了 adopt 兩次。`mutants-2c.toml`（23 條，含標籤的位置的 5 條）漏 7 條：停住時取輸入、停住時捲動、沒開 `Overlay` 的提示，補了測試；停住時 DOM 改了、新的提示不要 frame、不記畫過的 tree version 只靠審查；layout 被借出去時照樣畫走不到，等價。
  - `mutants-3a.toml`（29 條）漏 4 條：一串小的 stylesheet 不從第一份算起、只改 shadow root 的 stylesheet、文字裡的 `>`，補了測試；沒變的 stylesheet 的 id 等價。`mutants-3b.toml`（31 條）漏 2 條：改過的 `<link>` 照用留著的原文是多餘的條件（拿掉）；`@media` 從外到裡（多包一層）。`mutants-3c.toml`（34 條）漏 4 條：規則的 stylesheet 不看它是為誰 match 的是多餘的欄位（拿掉）；沒有 layout 時的 pseudo element 的改動（補了 task 裡重算 style 的情況）；`highlightNode` 框 pseudo element（補了截圖）。`mutants-3d.toml`（29 條）漏 7 條：測試頁的 `:has()` 讓任何狀態改變都重算整份，蓋住了 invalidation 的錯（補了沒有 `:has()` 的測試）；子樹裡的元素先強制再搬是測試的錯。
  - 等價和只靠審查的那幾條在 `docs/test-gaps.md`。
- **不穩定的測試**：`modules::subresources_from_another_origin_go_by_cors` 約五次一次失敗：inline 的 module 沒有事件可等，偶爾在最後一個事件之後才執行（和 DevTools 無關），讓它執行時也算一項，之後連跑 30 次都過。`browser` 的 `navigation::a_srcdoc_document_commits_with_nothing_else_to_wake_the_renderer` 在負載下超過它的 5 秒期限（單獨跑多半 2.5 秒內，8 個一起跑時全部逾時，RFC 之前的 `7b34544` 一樣；慢的是主文件 commit 之前那段），`test.py automation` 的 `pipe --window` 有一次逾時、重跑通過：都不是這份造成的，先不查。`test.py wpt css/cssom css/cssom-view` 的 `scrollIntoView-should-treat-slot-as-scroll-container.html` 一直失敗，3b 的 commit 上也一樣，還沒查。
- **WPT**：`css/cssom`（`getComputedStyle` 借 layout 的方式改了）、`css/selectors`（match 時看強制的 pseudo-class）沒有預期之外的結果。
- **`test.py automation`**：第 2 段（scheduler 和 widget 搬家）和第 3 段做完時，60 個情境和 `pipe --window` 都通過（第 2 段第一次跑時視窗畫不出來，重跑全過；第 3 段那次 `pipe --window` 逾時一次，重跑通過）。
- **DevTools 手動確認**（`machopo --remote-debugging-port=9222` 開試用的頁面，Chrome for Testing 152 的 `chrome://inspect` 連上，兩個視窗並排：machopo 的視窗被蓋住時不畫）：
  - Elements：樹（iframe 的文件、shadow root、`::before`、文字、`::after`）、跟著按鈕的改動、`$0`、Console 的節點滑過時框出、Reveal in Elements panel、Event Listeners（Passive、Ancestors 的 window 的 `resize`）、搜尋、Properties。
  - 框和選取元素：四個顏色的圈和標籤、選取元素的模式、box model 的圖；`::before` 框出那顆星、標籤是 `div#card.card::before`。
  - Computed：box model、預設的清單、property 的來源、Show all、跟著動畫變的值、Rendered Fonts。
  - Styles 和 Sources：Sources 列出 `<link>`（原檔，連上之前載入的補抓）和 `(index)`；規則的「檔名:行」、跳到 Sources；劃掉的宣告、註解掉的是沒打勾的一條、`@media`、`@supports`；Console 改 `style`、`insertRule`、`<style>` 的文字、`<link>` 時跟著變；pseudo element 的規則、繼承來的規則、祖先的 `::selection`、`@keyframes`。
  - `:hov`：`:hover`、`:focus`（祖先的 `:focus-within`）、表單的 `:disabled`、`:required`；`:visited` 不生效（限制）。
  - 停在 `debugger`：頁面蓋上一層、「Paused in debugger」；停住時 Console 改的背景、滑過的節點照樣畫；繼續之後提示拿掉、動畫接著走。
- **最後的驗證**（第 3 段做完時）：`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；`cargo fmt --check`（第 3 段的 13 個檔案整理過）、`cargo clippy --workspace --all-targets -- -D warnings`（第 1 段 4 處、第 2 段 2 處、第 3 段 5 處，都在各段的程式裡修了）；`cargo test --workspace`（3760 個）；`test.py cdp` 的每一組只有已知的差異。
