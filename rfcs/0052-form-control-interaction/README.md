# RFC 0052：表單控制項的互動

- 開始日期：2026-10-17
- 狀態：✅ 已完成：2026-10-17（第 1 到 5 條 2026-10-17 接受；三段都做完，驗收在 `progress.md`，限制在 `docs/limitations.md`）
- 細節：`details.md`；進度：`progress.md`

## 摘要

RFC 0050 畫出了表單控制項，但有幾個部件只能看：收起來的 `<select>` 點了不打開選單、鍵盤也不能換；listbox 點 option 不改選取；search 的清除鈕和 number 的上下鈕點了不做事。這份照 Blink 做它們的互動：選單的下拉清單（popup）和鍵盤、listbox 的滑鼠和鍵盤（`multiple` 的 Ctrl、Shift）、清除鈕、上下鈕（連同它要的 `stepUp()`、`stepDown()`），每一種在 Chrome 發的 `input`、`change`、`search` 事件的時機都一樣。下拉清單由 Browser 畫（第 1 條，Blink 的 `ExternalPopupMenu` 的形狀）。

## 動機

- **使用者**：頁面上的 `<select>` 是最常見的表單元件之一（國家、日期、排序），現在只能靠 script 改；listbox 點了沒反應；number 欄位的上下鈕看得到、按了沒用。
- **WPT**：`html/semantics/forms/` 和 `html/rendering/widgets/` 裡用 testdriver 點選單、listbox，或按方向鍵、上下鈕的測試還不能過。
- RFC 0050 的第 19、26 條把這些留到之後（`docs/limitations.md` 裡表單控制項的那一條）。

要解決的 `docs/limitations.md` 的限制：「表單控制項的行為」的「清除鈕和上下鈕點了不做事」、「`<select>`：使用者還不能選（收起來的選單點了不打開下拉的選單，listbox 點 option 不改選取，鍵盤也不能）」。

**目標**：

- 收起來的選單：滑鼠按下打開下拉清單（再按一次關上），Alt+↑↓、F4、Space、Enter 打開；在清單裡 hover、方向鍵、Enter、Esc、點外面關上；關著時方向鍵、PageUp/Down、Home/End 直接換選取；選了不同的 option 時發 `input`、`change`。`showPicker()`。
- listbox：按下選 option，`multiple` 時 Ctrl 加減、Shift 選一段、拖曳延伸；方向鍵、PageUp/Down、Home/End、Shift、Ctrl+Space；放開滑鼠（鍵盤是每一下）發 `input`、`change`；選到的 option 捲進看得到的地方。
- search 的清除鈕：click 清空、發 `input`（或 `change`）和 `search`；Esc 也是。
- number 的上下鈕：按下照上下半邊 `stepUp()`、`stepDown()`，按住 250 ms 之後每 50 ms 一次，每次發 `input`、放開時 `change`；有焦點時 ↑↓ 也加減（滾輪之後，第 4 條）。`stepUp()`、`stepDown()`、`valueAsNumber` 和 HTML 的 step 規則。

**不在範圍內**：type-ahead（打字跳到那個 option）；listbox 拖曳到邊緣時的自動捲動；`appearance: base-select` 的 popover 選單；date、color、file 的選擇器；觸控；macOS 的選單行為（Mac 的 Chrome 方向鍵打開選單、Enter 送出表單）。

## 設計概要

網頁的作者看到的：

- 點 `<select>`，下面出現 Chrome 樣子的清單（至少和 select 一樣寬，下面放不下時放在上面）；點一個 option，select 換成它，先 `input` 再 `change`，然後是 `mouseup` 和 `click`（Chrome 的順序）。Tab 到 select、按 ↓ 直接換到下一個，每一下都有 `input`、`change`。
- `<select multiple>` 點、Ctrl 點、Shift 點和 Chrome 一樣；放開滑鼠時才有 `input`、`change`。
- search 欄位按 `×` 清空，`search` 事件照發；number 欄位按住上下鈕會連續加減，`step`、`min`、`max` 照 HTML 的規則。
- `select.showPicker()` 在有 user activation 時打開清單。

改 kun 的人看到的：`dom_api` 的 select、spin button、清除鈕有自己的 `DefaultEventHandler`（Blink 的 `MenuListSelectType`、`ListBoxSelectType`、`SpinButtonElement`、`SearchFieldCancelButtonElement`）；renderer 向 Browser 要求打開、關上一個 popup 清單，Browser 回答選了哪一個（`protocol` 多三個訊息）；shell 在網頁 view 上面畫清單（第 1 條的 P2）；`dom` 有 pointer capture 的最小部分（上下鈕要它）。

## 詳細設計

### 下拉清單畫在哪裡（第 1 條，比較在 `details.md` 的「下拉清單」）

| 選項 | 做什麼 | Chromium |
| --- | --- | --- |
| P1 另一個 widget 的 HTML 頁面 | renderer 為清單建第二個 page 和 document（Blink 的 `WebPagePopupImpl` 和 `list_picker.js`），Browser 開一個 popup 的 surface 放它 | Windows、Linux、ChromeOS 的 `InternalPopupMenu` |
| **P2 Browser 畫的原生清單**（選了） | renderer 把 option 的 label、停用、分組、選取、select 的字型和位置送給 Browser（新訊息），shell 在網頁 view 上面畫一個清單的 layer、處理它的滑鼠和鍵盤，選好時把 index 送回 renderer | Mac、Android 的 `ExternalPopupMenu` |
| P3 畫在頁面裡 | select 的 UA shadow tree 多一個清單，放在 top layer | `appearance: base-select` 的 `::picker(select)`（popover） |

選 P2 的理由：跨 process 的形狀和 Chromium 的 `ExternalPopupMenu` 一樣（renderer 只送資料、收結果，不碰視窗），shell 已經有畫 UI 文字和 layer 的程式（分頁列）；P1 要 renderer 能有第二個 widget、Browser 要有 popup 的 surface，是另一份 RFC 的大小；P3 要 kun 還沒有的 top layer，清單也超不出頁面，和 `appearance: auto` 的 Chrome 不同。P2 的代價：清單的樣子是 shell 畫的，照 Chrome 的 `list_picker` 的樣子畫（字型、高度、選到和 hover 的顏色），不跟著 option 的作者樣式（Chrome 的 popup 跟一部分）。

做法：`dom_api` 的 `select_type` 經過 `FrameClient::show_popup_menu` 要清單，renderer 在沒有 script 執行時算 select 的位置（main frame 的 viewport）、字級和顏色，送 `RendererHostMsg::ShowPopupMenu`；shell 的 `popup_menu` 畫它、收輸入，用 `RendererMsg::PopupMenuClosed` 送回結果。在網頁上按下時由 renderer 關上清單、照樣分派這次按下，同一次按下要打開同一個 select 的清單時不打開（Blink 的 `WebViewImpl::HandleMouseDown`）。清單的 layer 標 `over_surfaces`，畫在網頁的 surface 上面（kun 的 compositor 原本一律把 surface 畫在最上面）。

### 收起來的選單

`MenuListSelectType::DefaultEventHandler` 的 Windows 和 Linux 的行為（細節在 `details.md`）：左鍵按下時 focus、清單開著就關上，沒開就打開；keydown 的 Alt+↑↓、F4，keypress 的 Space、Enter 打開；關著時沒有修飾鍵的 ↓→ 下一個、↑← 上一個（跳過停用的）、PageDown/Up 三個、Home/End 第一個和最後一個，選到不同的就發 `input`、`change`。清單的結果：點了一個 option（或 Enter、Tab）時選它，不同時發 `input`、`change`，再發合成的 `mouseup`（鍵盤不發）和 `click`；Esc 關上、不選（Chrome 保留清單裡用鍵盤移過的，kun 不保留，記在限制）；按頁面的別處、滾輪、失焦、視窗大小改變、切換分頁、視窗失去 focus 時關上。清單開著的 select 是 `:open`（第 5 條，也比對有 `open` attribute 的 `<details>`、`<dialog>`；那個 attribute 因此是用到 `:open` 的規則的 feature，`<details>` 開關時下面的元素重算 style）。`showPicker()` 照 Chrome 的檢查（跨 origin 的 iframe、停用、沒有 user activation、沒有畫出來）。

### listbox

`ListBoxSelectType` 的：左鍵按下時 focus，選那個 option（Shift 是一段、Ctrl 加減、單選一律只留它），記下 anchor 和 end；按著移動時延伸 end；放開時比對按下前的選取，不同就發 `input`、`change`（Blink 的 `ListBoxOnChange`）。鍵盤：↑↓ 一個、PageUp/Down 是 `size − 1` 個、Home/End，Shift 從 anchor 延伸，`multiple` 時 Ctrl+方向鍵只移 focus 的 option（`-internal-multi-select-focus`，畫 focus ring）、Ctrl+Space 加減；每一下發 `input`、`change`。選到（或 focus）的 option 捲進 listbox 看得到的地方。拖曳到邊緣的自動捲動不做。做法：`dom_api` 的 `select_type` 是 select 的 default event handler（default event handler 因此也拿到分派時的 target，按在 option 的 UA shadow tree 裡也找得到 option），狀態是 `dom` 的 `SelectState`，捲動經過 `LayoutClient::scroll_into_view_after_layout` 排到 renderer 的下一次 layout 之後；kun 的 Ctrl+Space 沒有 `keypress`（Chrome 有），改在它的 `keydown` 加減。

### 清除鈕和上下鈕

- **清除鈕**（`SearchFieldCancelButtonElement`）：左鍵的 `click` 在沒有停用、不是唯讀時清空值（`setValueForUser`：有焦點時發 `input`，沒有時 `change`，值本來就是空的就不發），再發會冒泡的 `search`；在有值的 search 欄位按 Esc 也是。
- **上下鈕**（`SpinButtonElement`）：左鍵按下時 focus 欄位、拿 pointer capture（Chrome 也全選欄位的文字，kun 畫不出選取，沒做），照游標在上半或下半 `stepUp()` 或 `stepDown()`，每一步發 `input`；按住 250 ms 之後每 50 ms 再一步（移動時照游標重新判斷上下）；放開時放掉 capture、停計時器，值和第一次編輯前不同就發 `change`；元素移走、失焦、變成唯讀時只停下。↑↓ 加減，每一下發 `input`、`change`。有焦點時的滾輪（Chrome 照方向加減）之後再做（第 4 條的 W2）。
- **step 的規則**：HTML 的 `stepUp()`、`stepDown()`、`valueAsNumber`、allowed value step（`step` 和 `any`、step base、`min`、`max`），number 和 range 的（日期和時間的型別之後）；range 的鍵盤（←→↑↓、PageUp/Down、Home/End）用同一套。

做法：step 的規則是 `dom_api` 的 `step_range`，清除鈕、上下鈕和 search 的 Escape 是 `inner_elements`（UA shadow tree 的部件也有 default event handler）；使用者改的值經過 `input::set_value_for_user`（Blink 的 `SetValueForUser`）。range 的 step base 照 Blink 是它的最小值（HTML 是 `min` attribute，沒有時 `value` attribute）。算術是 double，結果照 step 和 step base 的小數位數四捨五入（Blink 是十進位的 `Decimal`）。做的時候發現空的文字欄位的 inner editor 是 0px 高，上下鈕按不到：Chrome 的 inner editor 是可編輯的 block，空的也有一行（`HasLineIfEmpty`），layout 照它補上。

### pointer capture

上下鈕要的最小部分：`dom` 記住一個元素拿著滑鼠（Blink 的 `PointerEventManager` 的 capture，這裡只給 UA 的元素用，不做 `setPointerCapture()` 的 API），renderer 送滑鼠事件時目標換成它。script 的 `setPointerCapture()` 和 pointer event 之後另外做。

### 測試

- **`dom_api` 的整合測試**：選單、listbox、清除鈕、上下鈕的每一種輸入（模擬的滑鼠和鍵盤事件）之後的選取、值和事件的順序（`input`、`change`、`search`、合成的 `mouseup`、`click`）；`stepUp()`、`stepDown()`、`valueAsNumber` 的邊界（`step="any"`、step base、`min` 和 `max` 的夾值、小數）。
- **renderer、protocol 和 shell**：打開清單的訊息帶的內容、選好之後送回的 index；shell 的清單的版面、hover、鍵盤、點外面關上。
- **`style`**：`:-internal-multi-select-focus`、`:open` 的比對和改變時重算的範圍。
- **情境**：`list-box`（listbox 的點和鍵盤、選單的清單）和 `demo-forms`（`about:demo` 的選單、number、range、search）。清單的樣子由 `shell` 的 `browser_view` 測試在真的 tab 和 Viz 裡看像素。
- **WPT**：`html/semantics/forms/` 的 `stepUp`、`stepDown`、`valueAsNumber`、range 的 step，`show-picker-user-gesture.html`，`details-open-pseudo-001.html`。
- **mutation test**：每段一份。

驗收的結果在 `progress.md`。

### 分階段（第 2 條）

1. **listbox 和選單的鍵盤**：`dom_api` 裡就做得完（沒有 popup）；pointer 的路徑已經有。
2. **清除鈕、上下鈕和 step**：`stepUp()`、`stepDown()`、`valueAsNumber`、pointer capture、上下鈕的計時器。
3. **下拉清單**：`protocol` 的訊息、shell 的清單、`showPicker()`。

## 缺點

- P2 的清單由 shell 畫，樣子要照 Chrome 的 `list_picker` 另外寫一份，option 的作者樣式（顏色、字型）不跟（Chrome 的 popup 跟一部分）；之後 Browser 的 UI 改用 kun 自己的 renderer 畫時（`docs/architecture.md` 的和 Chromium 不同的地方那張表），可以換成 P1。
- 計時器和 pointer capture 是 renderer 這一側新的狀態，renderer 拆成 process 時跟著它走。

## 理由和其他做法

見 `details.md`。不做的話，畫出來的選單、listbox、上下鈕都按了沒反應，比畫不出來更讓人困惑。

## 先例

- **Blink**：`MenuListSelectType`、`ListBoxSelectType`（`html/forms/select_type.cc`），`InternalPopupMenu`（PagePopup 和 `list_picker.js`）、`ExternalPopupMenu`（Mac、Android：Browser 畫原生的選單），`SearchFieldCancelButtonElement`、`SpinButtonElement`（`text_control_inner_elements.cc`、`spin_button_element.cc`），`StepRange`、`InputType::StepUp`。
- **Gecko**：選單的清單是 parent process 畫的（`SelectParent`、`SelectChild` 送 option 的資料，和 P2 同形）。
- **WebKit**：`PopupMenu` 由各平台的 UI 畫（Mac 是 `NSMenu`）。

## 已決定的事項

2026-10-17 確認（選項的比較在 `details.md`）：

1. **下拉清單由 Browser 畫**（P2，Blink 的 `ExternalPopupMenu`，Firefox 的 `SelectParent`）：renderer 送 option 的資料和 select 的位置、收回選了哪一個（或取消），照 Blink 的 `SelectOptionByPopup` 發事件；shell 在網頁 view 上面畫清單、處理它的輸入，樣子照 Chrome 的 `list_picker`。不選另一個 widget 的 HTML 頁面（P1，Windows、Linux 的 Chrome）：最像 Chrome，但要先有 renderer 的第二個 widget 和 Browser 的 popup surface，這份會大兩倍以上；之後 Browser 的 UI 改用 kun 的 renderer 畫時再換。不選畫在頁面裡（P3）：要先做 top layer，清單超不出 viewport，和 `appearance: auto` 的 Chrome 不同。
2. **分三段，照這個順序**：listbox 和選單的鍵盤（`dom_api` 裡做得完）、清除鈕和上下鈕和 step 的規則（計時器和 pointer capture）、下拉清單和 `showPicker()`（`protocol` 和 shell）。不選下拉清單先做：最常用的最早能用，但最大、最不確定的一段在最前面。
3. **第 1 段的三件事**（比較在 `details.md` 的「第 1 段的選項」）：
   - select 的互動狀態（anchor、end、按下前的選取、上一次 `change` 的選取和 option）是 `dom` 的 `FormControlData::Select`，option 用 `NodeId` 記（X1，照 Blink 放在元素上的 `SelectType`）。不選 `dom_api` 的表（X2）：元素移走、document 換掉時要另外清，和其他控制項的狀態不在同一處。
   - `multiple` 的 listbox 用 Ctrl+方向鍵移動的 option 畫 focus ring：option 多一個 element state，`:-internal-multi-select-focus` 只有 UA stylesheet 收，UA 規則照 Chrome（E1）。不選不畫（E2）：鍵盤的使用者看不到在哪。
   - 選到（或 focus）的 option 在下一次 frame 的 layout 之後才捲進 listbox 看得到的地方，一次只捲最後記下的（S2，Blink 的 `ScrollToOptionTask`）。不選當下捲（S1）：為了捲動強制 layout，一次事件裡選好幾次就排好幾次。

4. **第 2 段的四件事**（比較在 `details.md` 的「第 2 段的選項」）：
   - pointer capture 是 `dom` 記著的拿著滑鼠的元素（每個 isolate 一個，元素移走時清掉），renderer 的 `mouse_target` 在按鈕按著時回傳它，放開時放掉（C1，Blink 的 `PointerEventManager` 的 capture）；之後的 `setPointerCapture()` 用同一份。不選經過 `FrameClient` 由 renderer 記（C2）：元素移走時要另外通知；不選不做 capture（C3）：移到外面時不重新判斷上下，形狀和 Blink 不同。
   - 上下鈕按住時的計時器是 `dom` 的 `DomTasks` 裡有延遲的 task，每一步排下一個，欄位記著號碼，放開時換掉，舊的到了不做事（T1，Blink 的 `TaskRunnerTimer`）。不選 renderer 的計時器（T2）：邏輯分在兩邊，多一個進入 `dom_api` 的入口。
   - 日期和時間的型別在做之前當成還沒有：`valueAsNumber` 是 `NaN`、設它丟 `InvalidStateError`，`stepUp()`、`stepDown()` 丟 `InvalidStateError`，記進限制（D1）。不選這段一起做（D2）：五種語法和它們的 sanitization 本身是另一件事，這段會大一倍以上。
   - 有焦點的 number 的滾輪之後再做，滾輪照舊捲頁面，記進限制（W2）。不選這段做（W1）：要動 compositor 的 wheel event handler region 和沒有 listener 時的快路徑。

5. **第 3 段的四件事**（比較在 `details.md` 的「第 3 段的選項」）：
   - 清單是 `BrowserView` 最上面的 layer，可以蓋過工具列和分頁列，上下的空間照整個視窗算（L2）；開著時視窗的輸入都先給它。不選只在網頁 view 裡（L1）：網頁 view 矮時清單要捲，離 Chrome（另一個視窗，可以超出瀏覽器的視窗）比較遠。
   - `:open` 一起做：清單開著的 select（新的 element state，`select_type` 記著清單開著，`form_states` 照它設）和有 `open` attribute 的 `<details>`、`<dialog>`（O1，Chrome 133 的 `:open`）。不選不做（O2）：`select:open` 的規則整條無效。
   - `showPicker()` 只有 select 的：照 Chrome 檢查（跨 origin 的 iframe、停用、沒有 user activation、沒有畫出來）再打開清單（S1）。不選連 `<input>` 的一起（S2）：kun 沒有任何選擇器，date、color 的頁面會偵測到它卻打不開。
   - `<select multiple size=1>` 點了、按開清單的鍵都不打開，記進限制（M1）。不選清單也收複選（M2）：清單多一種模式，Chrome 的是 `base-select` 那一套 popover，要另外查。

## 待決定

（無）

## 之後的可能

- type-ahead、listbox 拖曳時的自動捲動。
- script 的 `setPointerCapture()` 和 pointer event。
- 日期和時間的 `stepUp()`；date、color、file 的選擇器。
- Browser 的 UI 改用 kun 自己的 renderer 畫之後，把清單換成 P1。
- macOS 的選單行為。
