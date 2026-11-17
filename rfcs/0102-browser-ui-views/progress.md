# RFC 0102 的進度：瀏覽器 UI 的 view 樹和可以換的版面

開始日期：2026-11-16

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 第 1 階段：`ui` 的保留的樹，工具列和分頁列搬上去 | 做完 | 2026-11-17 |
| 1a：`ui` 的樹、排版、畫、事件、主題；`ui_text` 搬進來 | 做完 | 2026-11-16 |
| 1b：工具列和分頁列改寫成 view，`BrowserView` 走樹 | 做完 | 2026-11-17 |
| 1c：文件和階段結束的檢查 | 做完 | 2026-11-17 |
| 第 2 階段：宣告式的一層、版面管理和側邊欄 | 做完 | 2026-11-17 |
| 2a：宣告式的一層，橫式 UI 用函式描述 | 做完 | 2026-11-17 |
| 2b：直的分頁清單、側邊欄的版面和切換 | 做完 | 2026-11-17 |
| 第 3 階段：外觀 | 進行中 | |
| 3a：深色主題、跟著系統的深淺色 | 做完 | 2026-11-17 |
| 3b：視窗：畫進標題列、透明和毛玻璃 | 還沒開始 | |
| 3c：側邊欄收合、網頁卡片 | 還沒開始 | |
| 3d：分頁清單捲動 | 還沒開始 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| `ui` 的樹：id 和世代、flex 的排版、要求重排、hit test、事件往上找、capture、hover 的順序、焦點和按鍵、游標、只重畫變了的 view 和變了的範圍（重畫、搬動、縮小、隱藏、拿掉、新的大小、新的主題、沒有 rect 的 op）、留著的錄製沒有過期 | `cargo test -p ui` |
| `ui` 只依賴允許的 crate；renderer 那一側不依賴 `ui` | `python3 scripts/sanity.py deps`、`python3 scripts/sanity.py boundary` |
| `shell` 改用 `ui` 的 `text`、`Clipboard` 之後行為不變 | `cargo test -p shell --lib`；`cargo test -p shell --test it -- browser_view:: windowless::` |
| 工具列（按鈕和網址列是 view，flex 排）、分頁列（一個 view）在樹上：位置和改寫前一樣、按鈕要可以按才按得下、按下和放開都在按鈕上、只有可以按的按鈕 hover 時亮、網址列點一下拿到焦點和外框、Cmd+L 再全選、分頁列送出的訊息、工具列的背景收住指標 | `cargo test -p shell --lib chrome::` |
| `BrowserView` 走樹：網址列、按鈕、分頁列的操作和游標照舊；hover 一個按鈕只重 raster 那個按鈕；輸入法的位置在網址列的文字裡 | `cargo test -p shell --test it -- browser_view:: windowless::` |
| 每次 paint 都檢查 view 留著的錄製沒有過期（debug build），`shell` 的測試和情境都經過它 | `cargo test -p shell --test it`；`python3 scripts/test.py scenarios tab-strip navigation demo-busy rounded demo-script gpu-crash` |
| 外觀不變 | 見「做過的檢查」的 1b：改之前、改之後的截圖逐像素比較 |
| 宣告式的一層：第一次描述建立 view、Props 相等不碰、新的 Props 給 view、有 key 的搬動不重建、沒有 key 的照位置、換型別重建、拿掉、名字找得到帶型別的 view、拿掉的名字找不到 | `cargo test -p ui` |
| 橫式 UI 從 `UiState` 描述：分頁列在上、工具列在下，狀態變了同一批 view 被告知、按鈕照 Props 建立 | `cargo test -p shell --lib chrome` |
| 網頁 view 的位置從樹來：橫式在工具列下、整數像素（1x、1.5x、1.33x、比工具列矮的視窗），側邊欄在它和邊緣的右邊，拖到半個像素時四捨五入；兩個網頁 view 並排（從奇數像素開始、重疊）照舊 | `cargo test -p shell --lib chrome`；`cargo test -p shell --test it -- browser_view::` |
| 側邊欄：按鈕、網址列、New Tab、每個分頁一列的位置；分頁開關搬動時列跟著分頁；點分頁、關閉鈕（指標在上面或是目前的分頁時）、中鍵、New Tab（按下放開都在上面）；拖邊緣送出指標下的寬度、游標 | `cargo test -p shell --lib chrome` |
| 快捷鍵切換版面、網頁跟著移動；點側邊欄的列切換到那個分頁（第一個分頁關掉之後也是）；拖邊緣時網頁跟著縮放，夾在 200 到 min(600, 視窗的一半)；窄視窗裡側邊欄是視窗的一半 | `cargo test -p shell --test it -- browser_view::`；情境 `sidebar` |
| `--ui-layout`：預設橫式，`sidebar`；不認得的值、和 `--automation-pipe` 一起都報錯 | `cargo test -p shell --bin machopo` |
| 深色主題：換成深色時整個 UI 重畫，和一開始就是深色的一樣；同一種不重畫 | `cargo test -p shell --lib chrome` |
| `BrowserView` 換成深色：分頁列、工具列、網址列、側邊欄和目前的列是深色的值，網頁不變；換回淺色 | `cargo test -p shell --test it -- browser_view::` |
| `--ui-theme`：預設跟著系統，`light`、`dark`；不認得的值、和 `--automation-pipe` 一起都報錯 | `cargo test -p shell --bin machopo` |
| `BrowserView` 用 `UiState` 取代 `sync_toolbar`，行為不變 | `cargo test -p shell --test it`；情境 `tab-strip`、`navigation`、`demo-busy`、`rounded`、`demo-script`、`gpu-crash` |

## 做過的檢查

每一步一段：日期和平台；新的測試；mutation test（弄壞了什麼、哪個測試失敗、沒抓到的和原因）；視窗裡的手動確認；找到的 bug；不穩定的測試；跑過的檢查和測試數字。

### 1a

2026-11-16，macOS（Apple Silicon）。

- **新的測試**：`ui` 的 26 個（`crates/ui/src/tests.rs` 25 個，`theme.rs` 1 個）。`shell` 的單元測試 68 個、整合測試的 `browser_view::`、`windowless::` 64 個照舊通過。
- **mutation test**（`mutants-1a.toml`，`python3 scripts/test.py mutate --keep-going rfcs/0102-browser-ui-views/mutants-1a.toml`）：第一次 32 個抓到 28 個。沒抓到的四個：
  - 拿掉 view 時不清焦點、不清 capture：讀它們的地方（`focused()`、`captured()`、`pointer()`）都先問那個 id 還在不在，清掉是多餘的。刪掉那段程式，由世代守住（拿掉的 view 的 id 查不到，位置重用時也查不到新的）。
  - 放進 layer 時用錄製記著的位置：放進之前兩者一定一樣，等價，記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」，從 spec 拿掉。
  - 隱藏的 view 被 hit：排過版之後隱藏的 view 大小是 0，本來就點不到；沒排版就 hit test 時只有這個判斷擋住。補測試：隱藏之後、排版之前 hit test。
  - 同時找到的問題：view 的變了的範圍原本用 display list 的 `bounds()`，沒帶 rect 錄的 op（可能畫在任何地方）不算在裡面，範圍會太小。改成每個 op 的 visual rect 的聯集、夾在 layer 裡，補測試 `an_op_recorded_without_a_rect_damages_the_whole_layer`，spec 加兩個 mutant。
  - 改完之後重跑改到的 7 個：全部抓到。spec 現在 31 個。
- **跑過的檢查**：`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；`deps` 和 `boundary` 新的規則各弄壞一次（`ui` 依賴 `protocol`、`paint` 依賴 `ui`），都報錯。

### 1b

2026-11-17，macOS（Apple Silicon）。

- **外觀**（「已決定的事項」第 19 條）：一份暫時的情境把 UI 擺成 14 種狀態（剛打開；hover 在重新載入、不能按的上一頁、上一頁上；hover 在分頁、關閉鈕、新分頁鈕上；網址列全選、打字後游標在中間、太長捲到後面；標題淡出；16 個分頁擠到最窄、hover、指標在頁面上），在 1x、1.25x、1.5x、2x 各截一張；用到按鍵的照 macOS 和其他平台的快捷鍵各跑一次，共 112 張。先在 1a 的 commit（`66220ab`）上跑兩次，確認每次的截圖完全相同，再在改完的程式上跑。結果：1x、2x 的 64 張完全相同；1.25x 的 20 張、1.5x 的 28 張各有 1 到 18 個像素不同，全部是某個顏色的值差 1，在字和圖示抗鋸齒的邊上。原因是浮點數加法的順序：每個 view 用自己的座標錄、再平移過去，原本在工具列的座標裡一次算；位置一樣，只差最後一位的捨入。1x、2x 的平移都是整數個像素，所以完全相同。情境和比較的腳本沒有 commit。
- **和原本不同的兩處行為**（邊角的情況）：
  - 在分頁列上按著拖到工具列的按鈕上時，按鈕會亮（hover 跟著指標下面的 view；原本被分頁列收住時工具列不亮）。
  - 不能按的按鈕在指標下面變成可以按時（頁面載入完，上一頁剛好在指標下面），立刻亮；原本要等滑鼠再動一下。
- **新的測試**：`shell` 的 `chrome::` 7 個（工具列的位置、按鈕的按下、不能按的按鈕按下之後才變成可以按也不算、只有可以按的按鈕亮、網址列的焦點和外框和 Cmd+L、工具列的背景收住指標、分頁列送出的訊息）；`browser_view` 的 `hovering_a_toolbar_button_rasters_only_the_button_again`，`typing_in_the_address_field_navigates` 加上輸入法位置的檢查。`ui` 的 2 個（debug build 的檢查找到改了卻沒要求重畫的 view 時 panic、沒有時不 panic）。原本 `toolbar.rs` 的 3 個測試跟著舊的 `Toolbar` 拿掉，由 `chrome::` 的取代；網址列的測試改成直接驅動它的編輯（點擊的那個改在樹上）。
- **跑過的測試**：`ui` 28 個；`shell` 的單元測試 72 個；`shell` 的整合測試全部 570 個（加了上面那個之前）和之後的 `browser_view::`、`windowless::` 65 個；情境 `tab-strip`、`navigation`、`demo-busy`、`rounded`、`demo-script`、`gpu-crash` 和外觀比較用的暫時情境。
- **不穩定的測試**：`shell` 的 `devtools_initiators::script_asks_with_its_stack` 單獨跑 3 次失敗 1 次（兩次失敗在不同的行）。RFC 0007、0092、0095 都記過它在乾淨的 tree 上也會失敗、推測的原因；這次的改動沒有碰它測的程式。沒有再查。
- **mutation test**（`mutants-1b.toml`，`python3 scripts/test.py mutate --keep-going rfcs/0102-browser-ui-views/mutants-1b.toml`）：第一次 23 個抓到 19 個。沒抓到的四個：
  - 不能按的按鈕也收下按下：放開時還要可以按才算，所以多半一樣；但按下之後才變成可以按時（頁面載入完）會被當成按了上一頁。補測試 `a_press_on_a_disabled_button_is_the_toolbar_s_even_once_it_is_enabled`。
  - 換了倍率不讓 UI 整個重畫：compositor 自己在倍率變了時重新 raster 整個 layer（tile 只在同樣的倍率下沿用），這行是多餘的。刪掉它和 `ui` 的 `ViewTree::invalidate_layer`，從 spec 拿掉。
  - UI 的 layer 每次都整個重新 raster：結果一樣，只是多做事，沒有計數。`BrowserView` 加 `chrome_invalidation_for_testing`，補測試 hover 一個按鈕只重 raster 那個按鈕。
  - 輸入法的位置少了網址列的內距：原本的測試只看有沒有位置。補位置的檢查。
  - 補完之後重跑這三個：全部抓到。spec 現在 22 個。
- **跑過的檢查**：`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。

### 1c 和第 1 階段結束的檢查

2026-11-17，macOS（Apple Silicon）。

- **文件**：新增 `crates/ui/README.md`（怎麼運作、用法、寫 view 的規則、還沒有的、測試）；`docs/architecture.md` 的「和 Chromium 刻意不同的地方」的瀏覽器 UI 那一列改成現在的樣子；`crates/shell/README.md` 的「鍵盤和滑鼠」改到哪些函式要跟著改表的清單加上 `NavButton::on_pointer`。`docs/limitations.md`、`docs/testing.md` 和 `crates` 的 crate 表在 1a、1b 改過。
- **`cargo test --workspace`**：3933 個通過，沒有失敗。
- **`cargo fmt --check`**：1a、1b 的 9 個檔案沒排，`cargo fmt` 排好（只動排版）之後重跑 `ui`、`shell` 的單元測試，通過。
- **`cargo clippy --workspace --all-targets -- -D warnings`**：沒有警告。
- **真的視窗**（`python3 scripts/test.py scenarios --window tab-strip navigation demo-busy rounded demo-script gpu-crash`）：6 個都通過；看過 `target/automation-results/tab-strip/tab-strip.png`（2 倍、系統字型），分頁列、工具列、按鈕和網址列和改寫前一樣。
- `sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 都通過。

### 2a

2026-11-17，macOS（Apple Silicon）。

- **實作時改的設計**：`Component::create` 拿一個 context（`CreateCtx`：主題和文字的 context），不只拿主題：網址列建立時就要設網址，要用到字型的 context。
- **外觀**：照 1b 的做法（「已決定的事項」第 19 條），同樣 14 種狀態、4 種倍率的暫時情境，先在 1c 的 commit（`d875f46`）上截圖，改完再截：112 張全部完全相同（這次只改了 UI 怎麼建立，view 和畫法沒變）。情境沒有 commit。
- **新的測試**：`ui` 的 8 個（描述的建立、Props 相等不碰和新的 Props、有 key 的搬動、照位置和換型別、巢狀、名字找不到拿掉的 view）；`shell` 的 `chrome` 的 2 個（從 `UiState` 描述出來的樣子和狀態變了的更新、按鈕照 Props 建立）。原本 `chrome` 的測試改成用 `UiState` 描述，不再命令式地設網址、上一頁和分頁。
- **跑過的測試**：`ui` 34 個；`shell` 的單元測試 74 個；`shell` 的整合測試 571 個；情境 `tab-strip`、`navigation`、`demo-busy`、`rounded`、`demo-script`、`gpu-crash` 和外觀比較的暫時情境。
- **mutation test**（`mutants-2a.toml`，`python3 scripts/test.py mutate --keep-going rfcs/0102-browser-ui-views/mutants-2a.toml`）：17 個抓到 16 個。沒抓到的：按鈕建立時不照 Props 的 `enabled`。測試裡的按鈕都從預設的狀態建立（上一頁不能按、重新載入可以），剛好和 `NavButton::new` 的預設一樣；2b 換版面時按鈕會在有上一頁的時候重新建立。補測試 `buttons_made_from_a_state_with_history_are_enabled`，重跑抓到。
- **跑過的檢查**：`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`。

### 2b

2026-11-17，macOS（Apple Silicon）。

- **實作時改的設計**：
  - 側邊欄的寬度在算 `UiState` 時才夾進範圍（`BrowserView::sidebar_width_in_window`），不只在拖的時候：視窗太窄時預設的 260 也縮到視窗的一半，視窗再拉寬時回到拖的寬度。
  - `BrowserViewLayout` 這個 trait 拿掉了：版面就是描述函式（第 24 條）。`browser_view_layout.rs` 只剩橫式的分頁列、工具列各是幾個 physical px。測試用的兩個網頁 view 並排的版面改寫成描述函式（`side_by_side_layout`），預期的位置改成直接用 px 算，不再從版面本身來。
  - 側邊欄的分頁那一列在指標按下時已經是 hover 的（樹在分派之前更新 hover），所以「沒 hover 的分頁被按到關閉鈕的位置」不會發生；測試照實際會發生的寫。
- **外觀**：橫式照 1b 的做法比較（同樣 14 種狀態、4 種倍率的暫時情境，對 2a 之前的截圖）：112 張完全相同，網頁 view 進樹沒有改到橫式。側邊欄的樣子看過情境 `sidebar` 的截圖。暫時的情境沒有 commit。
- **新的測試**：`ui` 的 1 個（`find_all`）；`shell` 的 `chrome` 的側邊欄 6 個、網頁位置 2 個；`machopo` 的 `--ui-layout` 1 個；整合測試 4 個（快捷鍵切換、側邊欄的列切換分頁、拖邊緣、窄視窗）；情境 `sidebar`（照 macOS 和其他平台的快捷鍵各跑一次）。原本 `browser_view_layout.rs` 的單元測試搬到 `chrome` 的網頁位置的測試，數字不變。
- **跑過的測試**：`ui` 35 個；`shell` 的單元測試 81 個；`shell` 的整合測試 575 個；情境 `sidebar`、`tab-strip`、`navigation` 和外觀比較的暫時情境。
- **mutation test**（`mutants-2b.toml`，`python3 scripts/test.py mutate --keep-going rfcs/0102-browser-ui-views/mutants-2b.toml`）：19 個抓到 16 個。沒抓到的三個都補了測試，重跑都抓到：
  - New Tab 那一列在別處放開也開新分頁：補「按下在它上面、放開在別處，不開」。
  - 網頁的起點往下取整：測試的位置剛好都是整數像素；補一個落在半個像素上的（側邊欄拖到 261、1.5 倍，397.5 → 398）。
  - 側邊欄的列用位置當成分頁的索引：測試裡分頁的 id 和索引剛好一樣；補「第一個分頁關掉之後，點第一列是 id 1 的那個」。
- **只靠審查的**：直接開的視窗照 `--ui-layout=sidebar` 從側邊欄開始（`window.rs`）：情境一律從橫式開始，記進 `docs/test-gaps.md`。
- **跑過的檢查**：`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；`cargo fmt --check`；`cargo clippy --workspace --all-targets -- -D warnings`（改了一處：`ui` 的 `insert_boxed` 直接回傳 `match`）。

### 第 2 階段結束的檢查

2026-11-17，macOS（Apple Silicon）。

- **`cargo test --workspace`**：3954 個通過，沒有失敗。
- **`cargo fmt --check`**、**`cargo clippy --workspace --all-targets -- -D warnings`**：見 2b。
- **真的視窗**（`python3 scripts/test.py scenarios --window sidebar tab-strip navigation demo-busy rounded demo-script gpu-crash`）：7 個都通過；看過 `target/automation-results/sidebar/sidebar.png`（2 倍、系統字型）：按鈕、網址列、New Tab、兩列分頁（目前的白底、有關閉鈕），網頁在側邊欄和邊緣的右邊。
- `sanity.py` 的 7 項都通過。

### 第 2 階段之後：工具列的圖示

2026-11-17，macOS（Apple Silicon）。使用者看了覺得按鈕的圖示怪，尤其是重新整理。

- **原本的問題**（放大截圖看的）：上一頁、下一頁是實心的大三角形加上 2 px 的細桿，粗細不一致；重新整理的三角形浮在圓環缺口的右上方，沒有接在弧上、方向也不順著弧。
- **改成線條式的圖示**（`toolbar.rs` 的 `icon`、`icon_flags`）：所有線 2 px、圓頭、圓角接合（Chrome、Material 的線條圖示的畫法）。箭頭是一條桿子加一個開口的頭（兩邊各 45 度）。重新整理照常見的「順時針旋轉」線條圖示（Lucide 的 `rotate-cw` 這類）：半徑 6.5 的弧從右邊往下 30 度起、順時針到右上方，接到一個和座標軸對齊的直角頭（往上、往左各 4），角就是尖端；右邊留一段缺口。中間試過的：頭是沿著切線的開口箭頭，擠在圓的正上方、超出按鈕的範圍；弧從正右方開始，1x 時缺口被 2 px 的線蓋掉。1x 和 2x 的截圖放大看過。
- **新的測試**：`toolbar` 的 4 個（線的寬度、圓頭、圓角；圖示在按鈕裡的範圍；箭頭的頭在哪一邊、兩邊在尖端後面成 45 度；重新整理的弧的起訖和直角頭）。原本畫實心多邊形的程式和它們沒有對應的測試。
- **mutation test**（`mutants-icons.toml`）：7 個抓到 5 個；沒抓到的「上一頁朝右」和「箭頭的頭是一直線」：原本的測試只看有沒有一個點在左端（桿子兩端都有點）。改成直接檢查頭的三個點，重跑都抓到。
- **跑過的**：`shell` 的單元測試 85 個；整合測試的 `browser_view::`、`windowless::` 69 個；情境 `tab-strip`、`sidebar`、`navigation`；`cargo clippy -p shell -p ui --all-targets -- -D warnings`；`sanity.py` 的 `numerics`、`docs`。

### 3a

2026-11-17，macOS（Apple Silicon）。

- **深色的值**：對照 Chromium 100 的原始碼，每個 token 的來源和對不上的在 `details.md` 的「3a. 深色主題的值」。和第 30 條原本寫的不同的：文字照原始碼是白色，不是 `#E8EAED`；分頁的關閉鈕的 hover 照淺色的比例算出來和目前的分頁同色，加深到 18%。
- **實作時改的設計**：
  - 有毛玻璃的那組值移到 3b（第 29 條）：要視窗透明了才看得到、才能檢查，值也看 vibrancy 還是 Liquid Glass。主題照深淺色建（`BrowserTheme::new(ColorMode)`），兩份的大小共用（`sizes()`）。
  - 換主題用 `ViewTree::set_theme`（第 1 階段就有：全部重畫）；大小不變，描述不用重做。同一種深淺色不呼叫它，不重畫。
  - `--ui-theme` 固定時，視窗也照它建（winit 的 `with_theme`），macOS 的標題列跟著；跟著系統時，系統切換有 `ThemeChanged`。
  - sad tab 和 `<select>` 的選單照舊是淺色的：它們在網頁的範圍裡，和網頁一起跟著深淺色（之後的「網頁跟著深淺色」）。
- **外觀**：用整合測試的 harness 把 2 倍、深色的畫面讀回來看過（暫時的測試，沒有 commit）：橫式的分頁列、目前的分頁和工具列接在一起、hover 的按鈕、網址列；側邊欄的按鈕、網址列、New Tab、目前的列。
- **新的測試**：`chrome` 的 1 個（換成深色時全部重畫、和一開始就是深色的一樣，同一種不重畫）；`machopo` 的 `--ui-theme` 1 個；整合測試 1 個（換成深色、檢查分頁列、工具列、網址列、側邊欄、目前的列的像素和網頁，再換回淺色）。
- **mutation test**（`mutants-3a.toml`）：13 個都抓到。
- **只靠審查和手動確認的**：直接開的視窗跟著系統、系統切換時跟著換（`window.rs`）：自動化一律淺色，記進 `docs/test-gaps.md`。
- **跑過的**：`shell` 的單元測試 86 個、`machopo` 的 10 個；整合測試的 `browser_view::`、`windowless::` 70 個；真的視窗的情境 `sidebar`、`tab-strip`、`navigation`（`window.rs` 改了）；`sanity.py` 的 7 項。
