# RFC 0052 的細節：表單控制項的互動

## 現況（2026-10-17）

- `dom_api` 的 `activation` 有 DOM 的 activation behavior 和 Blink 的 `DefaultEventHandler`（`<a>`、`<label>`、`<input>`、`<button>` 的鍵盤），dispatch 在沒被取消的 trusted event 之後先叫目標的、再叫路徑上的祖先的。`<select>`、清除鈕、上下鈕都沒有。
- `<select>` 的選取只有 script 改得到（`selectedIndex`、`value`、`option.selected`）；RFC 0050 的 UA shadow tree 照選取顯示。listbox 畫出來了（`size` 列、選取的顏色），點了不改。
- 沒有 `stepUp()`、`stepDown()`、`valueAsNumber`（`core_html.rs` 的 `HTMLInputElement` 的說明寫著還沒有）；number 和 range 的值有 sanitization。
- 沒有 pointer capture（`setPointerCapture` 和 renderer 這一側的都沒有）、沒有 top layer（popover、`<dialog>`）。
- Browser 的 UI 是原生的過渡版本（shell 自己畫分頁列、工具列和網址列，`ui_text` 畫文字），網頁 view 是平的清單；沒有 popup 的 widget 或 surface。

## Blink 的行為（2026-10-17 的 Chromium main，`third_party/blink/renderer/core/`）

### 收起來的選單（`html/forms/select_type.cc` 的 `MenuListSelectType`）

- 打開：左鍵 mousedown 時 focus（`kMouse`），清單開著就 `HidePopup`，沒開就 `SaveLastSelection()`、`ShowPopup`；mouseup、click 不做事。keydown 的 Alt+↑↓、F4（沒有 Alt、Ctrl），keypress 的 Space（沒有 type-ahead 時）和 Enter（Windows、Linux；Mac 是送出表單）。清單已經開著時不再開。`LayoutTheme::PopsMenuByArrowKeys()` 是 false（Mac 是 true）。
- 關著時的鍵盤：沒有修飾鍵的 ↓→ +1、↑← −1（`NextValidOption` 跳過停用的）、PageDown/Up ±3、Home/End；`SelectOption` 帶 `kDispatchInputAndChangeEventFlag`，`DidSelectOption` 在和 `last_on_change_option_` 不同時馬上發 `input` 再 `change`。有 Shift、Ctrl、Alt、Meta 時不處理；其他的鍵是 type-ahead。
- 清單（`page/chrome_client_impl.cc` 的 `OpenPopupMenu`）：Windows、Linux 是 `InternalPopupMenu`：`OpenPagePopup` 建一個 `WebPagePopupImpl`（另一個 widget，有自己的 Page 和 document），寫進 picker 的 CSS、`window.dialogArguments`（`selectedIndex`、`baseStyle`、`children`：option、optgroup、`<hr>` 的 label、index、停用和它們的樣式，`anchorRectInScreen`、`scaleFactor`、`isRTL`），再載 `html/forms/resources/picker_common.js`、`list_picker.js`，它建一個清單裡的 listbox。Mac、Android 是 `ExternalPopupMenu`（Browser 畫原生的選單）。select 被 visual viewport 切掉、已經有別的 popup 時不開；開著時 `:open` 成立。
- 位置（`ListPicker.fixWindowSize_`、`picker_common.js` 的 `adjustWindowRect`）：寬是內容和 select 的寬的大者；放在 select 下面，下面放不下而上面空間比較多時放上面（高度夾在可用的空間裡）；左邊對齊 select，會被切掉或 RTL 時右邊對齊；select 移動時跟著移。
- 結果：hover 只改清單裡的選取；mouseup、Enter、Tab 時 `setValueAndClosePopup` → `SelectOptionByPopup`：和現在選的不同時 `input` 再 `change`；關上之後發合成的 `mouseup`（鍵盤不發）和 `click`（原始碼的註解：Angular 1.2 要 `change` 在 `mouseup`、`click` 之前）。清單裡的鍵盤移動會暫時改 select 顯示的文字；Esc 關上，但鍵盤移過的保留。Alt+↑↓ 在 `setTimeout(0)` 之後關上。
- 從外面關上（`exported/web_view_impl.cc`、`frame/web_frame_widget_impl.cc` 的 `CancelPagePopup`）：頁面上任何左鍵按下（按在同一個 select 上會再關上）、tap、**每一次滾輪**、widget 失焦、main frame 的大小改變、device scale 改變。`MenuListSelectType::DidBlur` 發還沒發的 `input`、`change`，關上清單。
- `appearance: base-select` 的 popover 只在 select 和 `::picker` 都是 `base-select` 時用（和非 Android 的選單式 `<select multiple>`）。

### listbox（`ListBoxSelectType`）

- 左鍵 mousedown：focus；停用的不做；按在可選的 option 上時 `UpdateSelectedState`：Shift 是 `kRange`、Ctrl（Mac 是 Meta）是 `kNotChangeOthers`、其他是 `kDeselectOthers`，單選一律 `kDeselectOthers`；`SaveLastSelection()`；`kNotChangeOthers` 按在已選的上時取消它（拖曳也是取消）；不是 `kRange` 時 anchor 設成它，end 設成它，`UpdateListBoxSelection`。
- 按著移動：（`StartAutoscrollForSelection`）只有在這個元素上按下時；`multiple` 時移 end（有 anchor 時），單選時 anchor 和 end 一起；`UpdateListBoxSelection` 套 anchor 到 end 的範圍，範圍外的回到按下前的（`cached_state_for_active_selection_`），跳過停用的。
- mouseup：停自動捲動，`HandleMouseRelease` → `ListBoxOnChange`：和 `last_on_change_selection_` 比，不同時 `input` 再 `change`（快取是空的或長度不同時只有 `change`）。
- keydown：方向照 writing mode；PageUp/Down 是 `ListBoxSize() − 1`；Home/End；Shift 從 anchor 延伸；`multiple` 時 Ctrl+方向鍵只移 focus（`is_in_non_contiguous_selection_`、`UpdateMultiSelectFocus`，option 的 `-internal-multi-select-focus`），其他時候取消其他的、anchor = end；每一下馬上 `ListBoxOnChange`。Ctrl+Space 加減 end 的那個；keypress 的 Space 在不連續的模式加減，Enter 送出表單。
- UA 的 `select:-internal-list-box:focus-visible option:-internal-multi-select-focus`：auto 的 outline、`outline-offset: -1px`。`DidBlur` 清掉 `last_on_change_selection_`。

### 清除鈕（`html/forms/text_control_inner_elements.cc` 的 `SearchFieldCancelButtonElement`）

- 左鍵的 `click`（註解說 mouseup，程式看的是 `click`），欄位沒有停用、不是唯讀時：`SetValueForUser("")`（`kDispatchChangeEvent`：有焦點時發 `input`，沒有時 `change`；值沒變時不發），清掉 autofill 的狀態，`OnSearch()` 發冒泡的 `search`。沒有 mousedown 的處理、capture 或明確的 focus（focus 是 host 的 mousedown 給的）。值是空的時 `opacity: 0; pointer-events: none`。
- 有值時按 Esc 也清空、發 `search`（`SearchInputType::HandleKeydownEvent`）。

### 上下鈕（`html/forms/spin_button_element.cc`）

- 左鍵 mousedown：focus 欄位、全選（`FocusAndSelectSpinButtonOwner`），先開計時器，照游標在上半或下半（水平的 writing mode；直的看 x）`DoStepAction(±1)` → `StepUpFromLayoutObject` → `ApplyStep(..., kDispatchChangeEvent)`：有焦點，所以每一步發 `input`。`SetPointerCapture(kMouseId, this)`；移動時重新算上下。
- 計時器：`ScrollbarTheme::InitialAutoscrollTimerDelay()` 250 ms，之後每 `1 / kAutoscrollMultiplier` 50 ms（`cc/input/scroll_utils.h`；Mac 另有值）。
- 放開（mouseup、移走、要開 popup 時）：放掉 capture、停計時器，`SpinButtonDidReleaseMouseCapture` → `DispatchFormControlChangeEvent`：值和 `value_before_first_user_edit_` 不同時發 `change`。
- 滾輪：沒有停用、不是唯讀、欄位有焦點時，照 `wheelDeltaY` 的正負加減，事件算是處理過（不捲頁面）；只發 `input`，`change` 在失焦時。
- 鍵盤（`NumberInputType::HandleKeydownEvent`）：↑ 加、↓（沒有 Alt）減，照 writing mode；發 `input` 再 `change`，每一下。

### `showPicker()`（`html/forms/html_select_element.cc`）

依序：跨 origin 的 iframe 是 `SecurityError`、停用是 `InvalidStateError`、沒有 transient user activation 是 `NotAllowedError`、沒有畫出來（沒有 layout box 或 display-locked）是 `NotSupportedError`；消耗 activation；選單的 `ShowPicker` 更新 style、`SaveLastSelection()`、`ShowPopup`，listbox 的什麼都不做。

## 下拉清單

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| P1 另一個 widget 的 HTML 頁面（Windows、Linux 的 Chrome） | renderer 為清單建第二個 page、document 和 widget，畫 picker 的 HTML；Browser 開一個 popup 的 surface 放它的 compositor frame，輸入送給那個 widget | 和桌面的 Chrome 一樣，清單的樣子是 HTML 和 CSS，跟著 option 的樣式；之後 date、color 的選擇器也用同一條路 | renderer 要能有一個以上的 widget（kun 一個 renderer 一份 frame tree），Browser 要有 popup 的 surface、它的 hit test 和 input；picker 的 HTML 和 script 要另外寫一份；這份 RFC 會大兩倍以上 |
| **P2 Browser 畫的原生清單（選了，第 1 條；Mac、Android 的 Chrome，Firefox）** | renderer 送 option 的資料（label、停用、optgroup、`<hr>`、選取，select 的字型、大小、顏色和它在網頁 view 裡的位置）；shell 在網頁 view 上面畫清單的 layer，處理它的 hover、捲動、滑鼠和鍵盤，選好或取消時把 index（或取消）送回去；renderer 照 Blink 的 `SelectOptionByPopup` 發事件 | 跨 process 的形狀和 Chromium 的 `ExternalPopupMenu` 一樣（renderer 只送資料、收結果，不碰視窗）；shell 已經畫 UI 的文字和 layer（分頁列）；情境測試照常跑（`BrowserWindow` 在 headless 也有 shell 的 UI） | 樣子是 shell 照 `list_picker` 另外畫的（字型、行高、hover 和選取的顏色），option 的作者樣式不跟；清單在網頁 view 的範圍裡（Chrome 的 popup 可以超出視窗）；protocol 多三個訊息（打開、關上和結果） |
| P3 畫在頁面裡（`base-select` 的 popover） | select 的 UA shadow tree 多一個清單，放在 top layer、照 anchor 放 | 不用 Browser 和 protocol；跟著作者的樣式 | kun 沒有 top layer（popover、`<dialog>`），要先做；清單超不出 viewport；和 `appearance: auto` 的 Chrome 不同（Chrome 只有 `base-select` 用 popover） |

選了 P2（2026-10-17，第 1 條）：要做的事最少、和 Chromium 有對應的做法，之後 Browser 的 UI 改成 kun 自己的 renderer 畫時可以換 P1。

## 分階段

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **listbox 和選單的鍵盤 → 清除鈕和上下鈕 → 下拉清單（選了，第 2 條）** | 先做 `dom_api` 裡就做得完的，再做要計時器和 pointer capture 的，最後做跨 Browser 的 | 每段小；第 1 段就讓 listbox 和選單的鍵盤能用 | 選單點了能打開要到第 3 段 |
| 下拉清單先做 | 先做最常用的（點選單） | 最常見的操作最早能用 | 最大、最不確定的一段在最前面 |

## 第 1 段的選項

kun 現有的（2026-10-17）：`dom` 的 `FormControlData` 有 `<input>`、`<option>`（selectedness、dirtiness）、`<textarea>`、`<output>`、`<form>` 的狀態，沒有 `<select>` 的；滑鼠事件有 `buttons` 和修飾鍵；`dom_api` 的 `scrollIntoView` 經過 `LayoutClient::scroll_into_view`，可以只捲最近的捲動容器（`nearest_only`），renderer 先排好 layout。

### select 的互動狀態放在哪裡

Blink 的 `ListBoxSelectType` 記著 anchor、end、按下前每個 option 的選取（`cached_state_for_active_selection_`）、上一次 `change` 時的選取（`last_on_change_selection_`）、是不是在不連續的選取裡；`MenuListSelectType` 記著上一次 `change` 的 option。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **X1 `FormControlData::Select`（選了，第 3 條）** | `dom` 的 select 多一份狀態，option 用 `NodeId` 記（不是 handle：option 被移走時查不到就當沒有） | 和 `<input>`、`<option>` 的狀態同一個地方，元素被 GC 時一起走；照 Blink 放在元素上 | `dom` 多一個型別 |
| X2 `dom_api` 的表 | `dom_api` 每份 document 一張以 select 的 `NodeId` 為鍵的表 | `dom` 不用改 | 元素移走、document 換掉時要另外清；和其他控制項的狀態不在同一處 |

### `multiple` 的 listbox 裡 Ctrl+方向鍵移動的 focus ring

Blink 的 UA 規則 `select:-internal-list-box:focus-visible option:-internal-multi-select-focus` 給那個 option 畫 focus ring（不連續的選取時，它不一定是選到的）。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **E1 照 Blink 做（選了，第 3 條）** | option 多一個 element state（`dom_api` 設），`:-internal-multi-select-focus` 是只有 UA stylesheet 收的 pseudo-class，UA 規則照 Chrome | 鍵盤的使用者看得到現在在哪一個 | 一個 element state 的 bit 和一個 pseudo-class |
| E2 不畫 | Ctrl+方向鍵照樣移、照樣 Ctrl+Space 加減，但看不到在哪 | 少一點 | 鍵盤用不太了 |

### 選到的 option 什麼時候捲進看得到的地方

Blink 的 `ListBoxSelectType::ScrollToOption` 排一個 task，在下一次 layout 之後捲（`ScrollToOptionTask`），一次事件裡選好幾次只捲最後一次。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| S1 當下捲 | 每次改了 end，就呼叫 `scroll_into_view`（nearest、只捲 listbox），renderer 先排 layout | 程式最少，用現成的 | 一次事件裡選好幾次時排好幾次 layout（鍵盤、滑鼠的頻率不高） |
| **S2 照 Blink 排到 layout 之後（選了，第 3 條）** | select 記下要捲到哪個 option，renderer 在下一次 frame 的 layout 之後捲（和 `scroll_to_fragment` 一樣的時機） | 和 Blink 一樣，不為捲動強制 layout，script 在同一個 task 裡改了 option 也照最後的捲 | 多一個「之後要捲」的記號，renderer 多看一處 |

照 Blink 做、不另外決定的：拖曳只在這個 select 上按下之後算（Blink 的 `MouseDownMayStartAutoscroll`），游標在 listbox 外時不延伸（沒有自動捲動，RFC 的「不在範圍內」）；`change` 的時機和「選取的數目不同時只發 `change`」照 `ListBoxOnChange`；停用的 option 和 optgroup 跳過；選單關著時的 Alt+↑↓、F4、Space、Enter 在第 3 段才打開清單，第 1 段不處理。

## 第 2 段的選項

kun 現有的（2026-10-17）：search 的清除鈕和 number 的上下鈕已經在 UA shadow tree 裡（`::-webkit-search-cancel-button`、`::-webkit-inner-spin-button`），畫了原生外觀，點了不做事；`<input>` 的 `InputState` 有 `value_before_edit`（commit 時比對、發 `change`）；`dom` 的 `DomTasks` 收有延遲的 task（`AbortSignal.timeout()` 用它）；renderer 只有 frame 層級的 capture（`FrameView::captured_frame`：在子 frame 按下時，放開前事件都送到那個 frame）；滾輪只在有 `wheel` listener 的地方才送到 main thread（compositor 的 wheel event handler region）；number 打不進字（文字編輯的限制）。

### pointer capture 記在哪裡

上下鈕按下之後，移到外面、在外面放開，事件都要到它（Blink 的 `SetPointerCapture(kMouseId, this)`，`PointerEventManager` 的 capture）。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **C1 `dom` 記著（選了，第 4 條）** | `dom` 每個 isolate 記一個拿著滑鼠的元素（`set_mouse_capture`，和 hover、active 元素一樣是 `dom` 的狀態，元素移走之後不算），renderer 的 `mouse_target` 在按鈕按著時回傳它，放開時放掉 | 和 `captured_frame` 同一個地方判斷；之後 `setPointerCapture()` 用同一份；renderer 拆成 process 之後也還在 renderer 裡 | `dom` 多一個欄位，renderer 多看一處 |
| C2 經過 `FrameClient` | `dom_api` 呼叫 frame 的 `capture_mouse(element)`，renderer 自己記 | `dom` 不用改 | 元素移走時要另外通知 renderer；之後的 `setPointerCapture()` 還是要在 `dom` 記 |
| C3 不做 capture | renderer 在任何放開時告訴 `dom_api` 停掉上下鈕 | 最少 | 移到外面時不重新判斷上下；和 Blink 的形狀不同，之後的 capture 要重做 |

### 上下鈕的計時器

按住 250 ms 之後每 50 ms 一步（Blink 的 `SpinButtonElement::repeating_timer_`，`TaskRunnerTimer`，script 看不到）。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **T1 `dom` 的 `DomTasks`（選了，第 4 條）** | `dom_api` 排一個有延遲的 DOM task（user interaction 的 task source），每一步再排下一個；欄位的狀態記一個號碼，放開時換號碼，舊的 task 到了不做事 | 用現成的，event loop 已經照時間叫醒；不受 script 的 timer 的巢狀限制和 clamp；document 不在了 task 一起走 | 放開之後還有一個到了才丟掉的 task |
| T2 renderer 的計時器 | renderer 自己排 `MainThreadTask`，到了呼叫 `dom_api` | 停的時候直接取消 | renderer 多一種計時器和進入 `dom_api` 的入口；邏輯分在兩邊 |

### 日期和時間的型別在這段做到哪裡

RFC 已經說 step 的規則先做 number 和 range，日期和時間的型別之後。要決定的是在那之前它們的 `stepUp()`、`valueAsNumber` 怎麼樣。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **D1 當成還沒有（選了，第 4 條）** | `valueAsNumber` 是 `NaN`、設它丟 `InvalidStateError`，`stepUp()` 丟 `InvalidStateError`（和 step 不適用的型別一樣），記進限制 | 不做一半的日期解析；行為簡單、一致 | 和 Chrome 不同（Chrome 照日期算） |
| D2 這段一起做 | HTML 的 date、month、week、time、datetime-local 的語法、step 的預設和單位（天、月、週、秒），`valueAsDate` | 和 Chrome 一樣 | 這段大一倍以上；五種語法和它們的 sanitization 本身是另一件事 |

### 有焦點的 number 的滾輪

Chrome 在 number 有焦點時用滾輪加減（`SpinButtonElement::ForwardEvent` 的 wheel，取消頁面的捲動）。kun 的滾輪只在 compositor 的 wheel event handler region 裡才送到 main thread。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| W1 這段做 | 有焦點、有上下鈕的 number 算進 wheel event handler region（blocking），它的 default event handler 照方向 `stepUp()`、`stepDown()`、發 `input`，取消捲動 | 和 Chrome 一樣 | 要動 compositor 的 region 和 `may_have_wheel_listeners` 的快路徑；焦點改變要讓 region 重算 |
| **W2 之後再做（選了，第 4 條）** | 滾輪照舊捲頁面，記進限制 | 這段不碰 compositor 和捲動 | 和 Chrome 不同 |

照 Blink 做、不另外決定的：清除鈕在左鍵的 `click`（不是 mouseup）、欄位沒有停用和唯讀時清空，有焦點時發 `input`、沒有時發 `change`，值本來就空時不發，再發冒泡的 `search`；有值時按 Esc 也一樣。上下鈕在左鍵按下時 focus 欄位（全選畫不出來，不做），照游標在上半或下半走一步，每一步有焦點時發 `input`，放開、移走、欄位停用或變唯讀時停下，值和第一次改之前不同就發 `change`；移動時照游標重新判斷上下。number 和 range 的 ↑↓（range 也有 ←→、PageUp/Down、Home、End）每一下發 `input`、`change`。step 的規則照 HTML：allowed value step（`step`、`any`、預設和倍數）、step base（`min`，沒有時 `value` attribute，再沒有時 0）、`min`、`max` 的夾值，`stepUp(n)`、`stepDown(n)`，`valueAsNumber` 的 getter 和 setter。

## 第 3 段的選項

kun 現有的（2026-10-17）：Browser 的 compositor 的 root layer 畫分頁列和工具列，每個網頁 view 是一個 `SurfaceLayer`（`shell` 的 `BrowserView`）；視窗的滑鼠和鍵盤都先到 `BrowserView` 再分給網頁；renderer 和 Browser 之間的訊息在 `protocol`（`RendererHostMsg`、`RendererMsg`）。第 1 條已經定了清單由 Browser 畫（`ExternalPopupMenu` 的形狀）；type-ahead 在 RFC 的「之後的可能」，這段不做。

### 清單可以畫到哪裡

Chrome 的清單是另一個視窗，可以超出瀏覽器的視窗；放在 select 下面，下面放不下而上面空間比較多時放上面（`ListPicker.fixWindowSize_`、`adjustWindowRect`）。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| L1 網頁 view 裡 | 清單的 layer 在網頁 view 的範圍裡，上下的空間照網頁 view 算 | 座標和 hit test 都在網頁 view 裡 | 網頁 view 矮時清單要捲；離 Chrome 比較遠 |
| **L2 整個視窗裡（選了，第 5 條）** | 清單的 layer 在 `BrowserView` 的最上面，可以蓋過工具列和分頁列，上下的空間照視窗算 | 比較接近 Chrome（只差在超不出視窗）；`BrowserView` 本來就收整個視窗的輸入 | 清單開著時視窗的輸入都先給它（點工具列也是關上清單） |

### `:open`

Chrome 的 `select:open` 在清單開著時成立（CSS 的 `:open`，Chrome 133 起；它也比對有 `open` 的 `<details>`、`<dialog>`）。kun 還沒有 `:open`。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **O1 一起做（選了，第 5 條）** | `:open` 比對清單開著的 select（新的 element state，`select_type` 記著清單開著，`form_states` 照它設）和有 `open` attribute 的 `<details>`、`<dialog>` | 和 Chrome 一樣；作者常用它改箭頭的樣子 | 一個 element state 的 bit；`<details>`、`<dialog>` 的部分是照 attribute 比對 |
| O2 不做 | `:open` 照舊不收，記進限制 | 這段小一點 | `select:open` 的規則整條無效 |

### `showPicker()` 做到哪裡

`HTMLSelectElement.showPicker()` 是這段的；`HTMLInputElement.showPicker()` 在 Chrome 打開 date、color、file 的選擇器，其他型別什麼都不做。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **S1 只有 select 的（選了，第 5 條）** | select 的 `showPicker()` 照 Chrome 檢查（跨 origin 的 iframe 是 `SecurityError`、停用是 `InvalidStateError`、沒有 user activation 是 `NotAllowedError`、沒有畫出來是 `NotSupportedError`），再打開清單 | 不留一個打不開選擇器、卻偵測得到的 API | `<input>` 照舊沒有 `showPicker` |
| S2 連 input 的一起 | input 的也照 Chrome 檢查，但 kun 沒有任何選擇器，都不做事 | WPT 的 `show-picker-*` 多通過一些 | date、color 的頁面偵測到它，卻打不開 |

### `<select multiple size=1>`

它是收起來的選單（`uses_menu_list`），Chrome 在 Windows、Linux 上點了打開一個可以複選的 popover（`base-select` 的那一套）。

| 選項 | 做什麼 | 好處 | 代價 |
| --- | --- | --- | --- |
| **M1 不打開（選了，第 5 條）** | 點了、開清單的鍵都不做事，記進限制 | 清單只有單選的一種 | 和 Chrome 不同 |
| M2 Browser 的清單也收複選 | 清單裡點一下加減一個、不關上，關上時把選了哪些送回去 | 這種 select 能用 | 清單多一種模式；Chrome 的樣子是另一套（popover），照它畫要另外查 |

照 Blink 做、不另外決定的：打開（左鍵按下、Alt+↑↓、F4、Space、Enter；已經開著時按下關上）、清單裡的滑鼠和鍵盤（hover 只改清單裡的選取，↑↓、PageUp/Down、Home、End 移動，Enter、Tab、點一下選它，Esc 取消；Chrome 留著鍵盤移過的，kun 不留，記在限制）、結果（不同時 `input`、`change`，再發合成的 `mouseup`（鍵盤不發）和 `click`）、從外面關上（頁面別處按下、滾輪、select 失焦、視窗大小改變、分頁切走）、位置（寬是內容和 select 的寬的大者，左邊對齊 select，下面放不下而上面空間比較多時放上面）和樣子（select 的字型、`list_picker` 的行高和顏色）；一次只有一個清單，renderer 記著是哪個 select，select 移走或停用時關上；`protocol` 的訊息是打開（option、optgroup、`<hr>` 的 label、停用、選到的 index、select 的字型和大小、顏色、在網頁 view 裡的位置、方向）和結果（選了哪一個或取消）。
