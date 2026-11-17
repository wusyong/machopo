# RFC 0102 的進度：瀏覽器 UI 的 view 樹和可以換的版面

開始日期：2026-11-16。完成日期：2026-11-17，macOS（Apple Silicon）。視窗的毛玻璃和畫進標題列（原本的 3b）延後（RFC 的第 34 條）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 第 1 階段：`ui` 的保留的樹，工具列和分頁列搬上去（1a 樹、1b 搬上去、1c 文件） | 做完 | 2026-11-17 |
| 第 2 階段：宣告式的一層、版面管理和側邊欄（2a 宣告式、2b 側邊欄） | 做完 | 2026-11-17 |
| 第 2 階段之後：工具列的圖示改成線條 | 做完 | 2026-11-17 |
| 第 3 階段：外觀（3a 深色、3d 捲動、3c 收合和網頁卡片） | 做完 | 2026-11-17 |
| 3b：視窗：畫進標題列、透明和毛玻璃 | 延後 | |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| `ui` 的樹 | id 和世代、flex 和 grid 的排版、要求重排、隱藏的 view 的子 view 照隱藏排；hit test、事件往上找、capture（連同那個 view 被拿掉時）、hover 的順序、焦點和按鍵、游標 | `cargo test -p ui` |
| `ui` 的畫 | 只重錄、只重 raster 變了的 view，變了的範圍（重畫、搬動、縮小、隱藏、拿掉、新的大小、新的主題、沒有 rect 的 op）；debug build 的每次 paint 檢查留著的錄製沒有過期（`shell` 的測試和情境都經過它） | `cargo test -p ui`；`cargo test -p shell --test it` |
| `ui` 的宣告式的一層 | 第一次描述建立 view、Props 相等不碰、新的 Props 給 view、有 key 的搬動不重建、沒有 key 的照位置、換型別重建、拿掉、名字找得到帶型別的 view、`find_all` | `cargo test -p ui` |
| `ui` 的捲動和 layer | 滾輪捲在內容的範圍裡、捲到底交給外層；`ScrollView` 的子 view 和 `paints_to_layer` 的 view 自己的 layer（大小、位置、裁切、巢狀時取交集和跟著外層移動）；捲動不重畫；layer 裡變了的範圍；hit test、`absolute_bounds` 跟著位移；捲完的 hover；收住指標時滾輪照樣給底下的；內容變小時夾住；捲到看得到；`is_within` | `cargo test -p ui` |
| 邊界 | `ui` 只依賴允許的 crate；renderer 那一側不依賴 `ui` | `python3 scripts/sanity.py deps`、`python3 scripts/sanity.py boundary` |
| 橫式的 UI | 從 `UiState` 描述：分頁列在上、工具列在下，位置和改寫前一樣；按鈕要可以按才按得下、按下和放開都在按鈕上、只有可以按的按鈕 hover 時亮；網址列點一下拿到焦點和外框、Cmd+L 再全選、輸入法的位置；分頁列送出的訊息；hover 一個按鈕只重 raster 那個按鈕；圖示的線、頭和弧 | `cargo test -p shell --lib -- chrome:: toolbar`；`cargo test -p shell --test it -- browser_view:: windowless::` |
| 外觀不變（第 1、2 階段） | 改之前、改之後的截圖逐像素比較（14 種狀態、4 種倍率，見「做過的檢查」） | 暫時的情境，沒有 commit |
| 網頁 view 的位置 | 從樹來：橫式在工具列下、整數像素（1x、1.5x、1.33x、比工具列矮的視窗），側邊欄的卡片在它右邊，拖到半個像素時四捨五入；兩個網頁 view 並排（從奇數像素開始、重疊） | `cargo test -p shell --lib chrome`；`cargo test -p shell --test it -- browser_view::` |
| 側邊欄 | 按鈕、網址列、New Tab、每個分頁一列的位置；分頁開關搬動時列跟著分頁；點分頁、關閉鈕、中鍵、New Tab；拖縫中間的邊（夾在 200 到 min(600, 視窗的一半)）；快捷鍵切換版面、網頁跟著移動 | `cargo test -p shell --lib chrome`；`cargo test -p shell --test it -- browser_view::`；情境 `sidebar` |
| 分頁清單捲動 | 自己的 layer、按鈕和網址列不動、捲到底停住、捲過之後點到的是那一列、切換分頁或回到側邊欄時捲到目前的分頁、改標題不捲、關掉分頁時夾住；compositor 照 layer 的位置畫、裁在清單裡 | `cargo test -p shell --lib chrome`；`cargo test -p shell --test it -- browser_view::`；情境 `sidebar-scroll` |
| 網頁卡片 | 留縫、圓角（網頁和 sad tab 的角是縫的顏色）；compositor 的 `SurfaceLayer` 的圓角是 surface quad 的 `rounded_clip`（physical px） | `cargo test -p compositor --lib surface_layer`；`cargo test -p shell --test it -- browser_view::` |
| 收合和浮出 | 網頁佔滿、側邊欄藏起來；左邊緣浮出、在上面停留、點得到列、離開收回；Cmd+L 浮出、Escape 收回；按著時不收；離開視窗收回；指標位置未知時不浮出；橫式按了沒作用；展開、藏起來、浮出是同一個網址列 | `cargo test -p shell --lib chrome`；`cargo test -p shell --test it -- browser_view::`；情境 `sidebar-collapse` |
| 深色主題 | 換成深色時整個 UI 重畫，和一開始就是深色的一樣，同一種不重畫；分頁列、工具列、網址列、側邊欄和目前的列的像素，網頁不變，換回淺色 | `cargo test -p shell --lib chrome`；`cargo test -p shell --test it -- browser_view::` |
| 命令列 | `--ui-layout`（預設橫式）、`--ui-theme`（預設跟著系統）：不認得的值、和 `--automation-pipe` 一起都報錯 | `cargo test -p shell --bin machopo` |
| 真的視窗 | 情境照樣跑 | `python3 scripts/test.py scenarios --window sidebar sidebar-scroll sidebar-collapse tab-strip navigation demo-busy rounded demo-script gpu-crash` |

## 做過的檢查

都在 macOS（Apple Silicon）上，2026-11-16 到 17。

### Mutation test

每一段一份 spec（`mutants-*.toml`），用 `python3 scripts/test.py mutate --keep-going <spec>` 跑。

| spec | 第一次 | 沒抓到的和處理 |
| --- | --- | --- |
| `mutants-1a.toml`（樹） | 32 個抓到 28 個 | 拿掉 view 時不清焦點、capture：讀的地方都先問 id 在不在，刪掉那段，由世代守住。放進 layer 時用錄製記著的位置：放進之前兩者一定一樣，等價（記在 `docs/test-gaps.md`）。隱藏的 view 被 hit：補排版之前 hit test 的測試。同時發現 view 的變了的範圍用 display list 的 `bounds()` 會漏掉沒帶 rect 的 op，改成每個 op 的 visual rect 的聯集。重跑都抓到；spec 31 個 |
| `mutants-1b.toml`（搬上樹） | 23 個抓到 19 個 | 不能按的按鈕也收下按下（按下之後才變成可以按時會被當成按了）：補測試。換倍率時讓 UI 整個重畫：compositor 本來就整個重 raster，刪掉它和 `ViewTree::invalidate_layer`。UI 的 layer 每次都整個重 raster：加 `chrome_invalidation_for_testing` 和只重 raster 按鈕的測試。輸入法的位置少了內距：補位置的檢查。重跑都抓到；spec 22 個 |
| `mutants-2a.toml`（宣告式） | 17 個抓到 16 個 | 按鈕建立時不照 Props 的 `enabled`：測試的狀態剛好和預設一樣，補有上一頁時建立的測試，重跑抓到 |
| `mutants-2b.toml`（側邊欄） | 19 個抓到 16 個 | New Tab 在別處放開也開、網頁的起點往下取整、列用位置當成分頁的索引：各補測試，重跑都抓到 |
| `mutants-icons.toml`（圖示） | 7 個抓到 5 個 | 上一頁朝右、箭頭的頭是一直線：原本只看有沒有一個點在左端，改成直接檢查頭的三個點，重跑都抓到 |
| `mutants-3a.toml`（深色） | 13 個都抓到 | |
| `mutants-3d.toml`（捲動） | 24 個抓到 23 個 | 比清單高的範圍貼齊底部：測試裡那個範圍在原本的位移上面，兩種寫法都貼齊頂端，改成先捲回開頭，重跑抓到 |
| `mutants-3c.toml`（收合、卡片） | 26 個抓到 25 個 | 畫到自己的 layer 的 view 不跟著外層的 layer 移動：只有在捲動的清單裡看得出來，補測試，重跑抓到 |

### 外觀

- **第 1、2 階段不變**：一份暫時的情境把 UI 擺成 14 種狀態（剛打開、各種 hover、網址列全選和打字和太長、標題淡出、16 個分頁擠到最窄），在 1x、1.25x、1.5x、2x 各截一張，macOS 和其他平台的快捷鍵各跑一次，共 112 張；改之前先跑兩次確認截圖穩定。1b：1x、2x 的完全相同，1.25x、1.5x 有 1 到 18 個像素的某個顏色差 1，都在抗鋸齒的邊上（每個 view 用自己的座標錄再平移，浮點數加法的順序不同）。2a、2b：112 張完全相同。情境和比較的腳本沒有 commit。
- **之後的**：側邊欄、深色、捲動、收合和浮出的樣子用情境的截圖和整合測試的 harness 讀回來的畫面（2 倍，暫時的測試，沒有 commit）看過；圖示在 1x 和 2x 放大看過。

### 實作時改的設計

- `Component::create` 拿一個 context（主題和文字），網址列建立時就要排網址。
- `BrowserViewLayout` 拿掉：版面就是描述函式；測試用的兩個網頁 view 並排的版面改寫成描述函式。側邊欄的寬度在算 `UiState` 時才夾進範圍，視窗太窄時預設的 260 也縮到一半。
- 深色：有毛玻璃的那組值移到 3b；主題照深淺色建（`BrowserTheme::new`），兩份的大小共用。深色的值對照 Chromium 100 的原始碼（`details.md` 的「3a. 深色主題的值」）：文字照原始碼是白色，不是第 30 條原本寫的 `#E8EAED`；分頁的關閉鈕的 hover 照淺色的比例和目前的分頁同色，加深到 18%。
- 捲動的 layer 不另外做 compositor 的 scroll 節點：瀏覽器的 compositor 和 UI 在同一個 thread，位移直接放進 layer 的位置。
- 側邊欄的版面是一格的 grid（`minmax(0, 1fr)`）：底下是側邊欄的顏色、縫和網頁卡片那一列，上面疊面板；面板在展開、藏起來、浮出時都是同一個 view。

### 找到的 bug

- `ui`：view 的變了的範圍漏掉沒帶 rect 的 op（1a）；排 `display: none` 的 view 時它的子 view 照自己的 display 排，碰到 taffy 的 `unreachable!`（3c；之前藏起來的都是葉子，改成 hidden layout 的模式下一律照隱藏排，和 `TaffyTree` 一樣）。
- grid 的軌道用 `1fr` 時最小值是內容的大小，面板被分頁撐高、清單不捲（3c，改成 `minmax(0, 1fr)`）。
- 指標的位置一開始是 (0, 0)，收合時被當成在左邊緣而浮出：情境拿掉「碰左邊緣」那一行照樣通過才發現（3c，改成記住指標在不在視窗裡）。
- 和原本不同的兩處行為（1b，邊角的情況，留著）：在分頁列上按著拖到工具列的按鈕上時按鈕會亮；不能按的按鈕在指標下面變成可以按時立刻亮。

### 不穩定的測試

和這份 RFC 無關，它們和它們測的程式這份 RFC 都沒有動過：

- `shell` 的 `devtools_initiators::script_asks_with_its_stack`：1b 時單獨跑 3 次失敗 1 次；第 3 階段結束時 `shell` 的整合測試連跑 3 次失敗 1 次（`no frames: null`）。RFC 0007、0092、0095 都記過。原因照 0092 的推測（沒有驗證）：沒被 load 拿走的 stack 在 render thread 一輪的最後才清掉，兩個 `Runtime.evaluate` 落在同一輪時第二個拿到第一個的。
- `browser` 的 `navigation::a_srcdoc_document_commits_with_nothing_else_to_wake_the_renderer`：第 3 階段結束時的 `cargo test --workspace` 失敗一次；等 srcdoc 的 title 只給 5 秒，失敗那次 `browser` 的整合測試整體跑了 14 秒（平常 4 秒左右）。之後單獨跑 5 次、整個 `browser` 的整合測試連跑 6 次都通過。推測是負載重的時候超過期限，沒有驗證。

### 只靠審查的

`window.rs` 的三條路，情境和自動化走不到，記在 `docs/test-gaps.md`：直接開的視窗照 `--ui-layout=sidebar` 從側邊欄開始；跟著系統的深淺色（`Window::theme()`、`ThemeChanged`）；指標離開視窗（`CursorLeft`）。

### 各階段結束時跑的

| | `cargo test --workspace` | 其他 |
| --- | --- | --- |
| 第 1 階段 | 3933 個通過 | `cargo fmt`（排了 9 個檔案）、clippy 沒有警告、`scenarios --window` 6 個 |
| 第 2 階段 | 3954 個通過 | fmt、clippy、`scenarios --window` 7 個 |
| 第 3 階段 | `--no-fail-fast`：3982 個通過、1 個失敗（上面的不穩定測試）；第一次在另一個不穩定測試停下 | `cargo fmt`（排了這一階段的程式）、clippy 沒有警告、`scenarios --window` 9 個 |

每一段都跑了改到的 crate 的測試和 `sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs`；`deps` 和 `boundary` 的新規則各弄壞一次確認會報錯（1a）。最後一次：`ui` 48 個、`shell` 的單元測試 91 個、`machopo` 10 個、`shell` 的整合測試 579 個。
