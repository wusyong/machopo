# RFC 0002 的進度：網頁在視窗裡的位置

開始日期：2026-10-06

完成：2026-10-06，Windows 11（自動測試和真視窗的情境）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| A2（一段做完，`README.md` 第 11 條） | 完成 | 2026-10-06 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 預設的版面：工具列在上、網頁在下，起點取整到 physical pixel（1x、1.33x、1.5x、2x） | `cargo test -p shell --lib browser_view_layout`；`cargo test -p shell --test browser_view`（`the_toolbar_is_over_the_page`、`clicks_in_the_page_are_below_the_toolbar`，1x、1.5x、2x） |
| 網頁 view 畫在自己的 bounds 上，renderer 的大小就是 bounds 的大小，起點不是整數的 CSS px 也一樣；工具列跟著視窗的 tab | `cargo test -p shell --test browser_view`（`web_views_are_drawn_on_their_bounds_at_their_size`，1x、1.5x、2x） |
| 點擊送給指標下面的網頁 view，座標是它自己的；空隙不送給任何網頁；後面的網頁 view 蓋住前面的（畫和點都是）；`web_view_to_window` 按網頁 view 換算 | `cargo test -p shell --test browser_view`（`clicks_go_to_the_web_view_under_them_in_its_coordinates`、`a_later_web_view_is_over_an_earlier_one`） |
| 按下的網頁 view 拿到 capture：之後的移動、放開、游標都是它的；每個網頁 view 有自己的游標；放開之後 capture 清掉 | `cargo test -p shell --test browser_view`（`the_web_view_a_drag_starts_in_has_it`，1.5x） |
| 滾輪給指標下面的網頁 view，不看 capture，座標是它自己的 | `cargo test -p shell --test browser_view`（`the_wheel_goes_to_the_web_view_under_it`，1.5x） |
| 按鍵給最後按下（任何按鈕）的網頁 view，一開始是視窗的 tab；拿掉網頁 view 時 focus 和 capture 回來，視窗的 tab 拿不掉，拿掉的不再畫 | `cargo test -p shell --test browser_view`（`keys_go_to_the_web_view_pressed_last`） |
| 情境的 `click`、`wheel` 碰到中心不在網頁上的元素時失敗（第 13 條） | `cargo test -p shell --test windowless`（`failures_name_the_line_and_what_was_seen`）；`python3 scripts/test.py scenarios` |

## 做過的檢查

### A2（2026-10-06，Windows 11）

- **做了什麼**：`gfx::Rect`；`shell` 的 `BrowserViewLayout`（`DefaultLayout`、`toolbar_height`）；`BrowserView` 改成 `WebContents` 的清單和網頁 view（bounds、游標），一個 `hit_test` 和 `in_target` 取代 `area_at`、`in_page`，capture 記 `Target`，`focused` 記按鍵的網頁 view，`set_layout`、`add_web_view`、`remove_web_view`、`contents_mut`、`web_view_bounds`；`on_renderer_message` 帶 `TabId`（`window.rs`、`windowless.rs`、`AutomationHost` 跟著改），`page_to_window` 改成 `web_view_to_window`。
- **新的測試**：`browser_view` 的測試用自己的版面（`SideBySide`：兩個網頁 view，第二個的起點在奇數的 physical px 上，可以重疊），網頁把點擊的座標寫進 `#out`，用 automation 的查詢讀回（renderer 只在解析完時送一次標題，script 改的標題不會送出）；`browser_view_layout` 的單元測試；`windowless` 的視窗外的點擊。
- **途中找到的問題**：
  - `demo-script` 和 `wasm-demo` 兩份情境一直在點視窗下緣以外的元素（`#now`、回 `about:demo` 的連結）：原本的 `area_at` 把工具列以下的任何一點都當成網頁。改成照第 5 條之後這兩次點擊被丟掉，情境穩定地失敗（兩種快捷鍵都是）。處理見 `README.md` 第 13 條。
- **mutation test**（29 個；跑的期間請使用者不要 stage 或 commit，跑完用 checksum 確認三個檔案都還原了）：
  - hit test 和換算：第一個網頁 view 在上面、空隙算第一個網頁 view、x 或 y 不扣起點、起點當成 CSS px，都抓到。
  - 版面和畫：renderer 拿視窗的大小、surface 畫在 x = 0、前面的畫在上面、每個 surface 都是 tab 的、工具列的高度無條件進位，都抓到。
  - 指標：移動、放開、游標不跟著 capture，放開之後 capture 留著，所有網頁 view 共用一個游標，滾輪跟著 capture、用視窗座標，都抓到。
  - focus：按下不給 focus、只有左鍵給、按鍵總是給 tab，都抓到。
  - 拿掉網頁 view：focus 留著、tab 也能拿掉，抓到；**capture 留著第一次沒抓到**：測試先放開按鈕才移動，放開本來就清掉 capture。改成按著按鈕移動之後抓到。
  - 訊息和自動化：訊息都給 tab、`web_view_to_window` 不移 x、總是用 tab 的、`web_view_bounds` 沒除 scale，都抓到；不移 y 由情境抓到；情境的點擊不檢查是否在網頁上，由 `windowless` 抓到。
  - mutation 的腳本在 Windows 上用文字模式寫回檔案，把 LF 的 `browser_view_layout.rs` 寫成了 CRLF（內容不變）：checksum 抓到，轉回 LF 之後一致。
- **真的視窗**：`test.py automation` 的 9 個情境都通過（2026-10-06，Windows 11）；截圖裡工具列和網頁的位置和之前一樣。
- **跑過的檢查**：`cargo fmt --check`、clippy（沒有警告）、`cargo test --workspace` 878 個全部通過（情境 18 次）、`sanity.py deps`、`boundary`、`unsafe`、`docs`。沒有動到 GC。
