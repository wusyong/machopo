# RFC 0011 的進度：分頁

開始日期：2026-10-06

完成：2026-10-06，Windows 11（自動測試和真視窗的情境）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1. 多個分頁 | 完成 | 2026-10-06 |
| 2. 分頁列 | 完成 | 2026-10-06 |
| 3a. 連結開新分頁 | 完成 | 2026-10-06 |
| 3b. sad tab | 完成 | 2026-10-06 |
| 拖拉排序 | 之後 | |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| **分頁和切換** | |
| 快捷鍵開、切換（頭尾循環）、關閉分頁；新分頁是 `about:blank`、網址列有 focus；網址列、標題、畫面跟著目前的分頁；關掉目前的分頁顯示右邊或左邊的；最後一個分頁關掉視窗 | `cargo test -p shell --test browser_view`（`tabs_open_switch_and_close_with_shortcuts`）；`cargo test -p shell --lib tab_strip_model` |
| 各平台的分頁快捷鍵 | `cargo test -p shell --lib`（`tab_shortcuts_per_platform`） |
| 切換分頁時 capture 放掉、按鍵給目前的分頁；分頁的 id 不重用 | `cargo test -p shell --test browser_view`（`keys_and_the_pointer_go_to_the_shown_tab`、`a_closed_tabs_id_isnt_given_again`） |
| 自動化等的是目前的分頁的載入，別的分頁的載入完成和 renderer 結束不影響 | `cargo test -p shell --test windowless`（`waiting_for_a_load_waits_for_the_shown_tabs`） |
| **隱藏的分頁** | |
| 隱藏的分頁不要 `BeginFrame`（動畫照樣想要 frame 時也一樣），再顯示時要、重畫；切換時網址列失去 focus | `cargo test -p shell --test browser_view`（`a_hidden_tab_asks_for_no_frames`）；`cargo test -p compositor --lib`（`a_hidden_compositor_asks_for_no_begin_frames`） |
| **Viz 和 raster** | |
| Viz 在執行中登記和拿掉 frame sink：登記的收到 `BeginFrame`，拿掉的不再收到、surface 也不在；視窗的分頁建立和關閉時告訴 Viz | `cargo test -p viz --lib`（`frame_sinks_come_and_go_while_viz_runs`、`a_removed_frame_sink_takes_its_surface`）；`cargo test -p shell --lib`（`window_tabs_tell_viz_of_their_frame_sinks`） |
| raster 的快取每個 compositor 一份，兩份互不影響；`sanity.py boundary` 的允許清單沒有 `TYPEFACES`、`IMAGES` | `cargo test -p raster --lib`（`a_decoded_image_is_dropped_with_its_data`）；`python3 scripts/sanity.py boundary` |
| **分頁列** | |
| 幾何：分頁的寬度（最寬 240、最窄 56、一起縮、超出右邊）、`+` 按鈕跟在最後一個分頁後面、窄的分頁只有目前的有關閉按鈕、點到的部分 | `cargo test -p shell --lib tab_strip`（`tabs_are_as_wide_as_there_is_room_for`、`narrow_tabs_have_a_close_button_only_when_active`、`parts_and_their_places`） |
| 滑鼠：按下就切換、關閉和 `+` 按下又放開在同一個按鈕上才算、中鍵關閉、hover 和離開 | `cargo test -p shell --lib tab_strip`（`clicks_activate_close_and_open`、`hovering_shows`）；`cargo test -p shell --test browser_view`（`the_tab_strip_switches_closes_and_opens_tabs`） |
| 畫出來的樣子：目前的分頁是工具列的顏色，長標題淡出；沒有標題時是網址；分頁列和工具列在同一個 layer、各在版面給的位置；從網址列導覽之後網址列顯示新的網址 | `cargo test -p shell --lib tab_strip`（`a_title_too_long_fades_out`、`the_active_tab_is_the_toolbars_color`）；`cargo test -p shell --test browser_view`（`the_toolbar_is_over_the_page`、`the_tab_strip_switches_closes_and_opens_tabs`）；`cargo test -p compositor_paint`（`a_list_pushed_at_an_offset_is_drawn_there`） |
| **連結開新分頁** | |
| 開在哪裡：中鍵、各平台的新分頁鍵、Shift、`target`（兩種平台）；使用者的 `click` 和中鍵的 `auxclick` 照它開，script 的點擊不開新分頁 | `cargo test -p dom_api --test events`（`where_a_click_on_a_link_goes`、`a_click_follows_a_link_where_it_says`） |
| renderer：每個按鈕各自的點擊、中鍵開背景的新分頁、開新分頁的連結不在這裡導覽；Browser：開新分頁的連結做同樣的檢查後交給嵌入的一方 | `cargo test -p browser --test navigation`（`a_click_is_the_primary_button_down_and_up_on_one_link`、`a_link_to_a_new_tab_is_handed_to_the_embedder`） |
| 分頁：`target` 開前景、中鍵和新分頁鍵開背景（隱藏、和網頁 view 一樣大）、加 Shift 開前景；放在打開它的分頁和它之前開的之後；關掉時回到打開它的分頁；被取消的 `auxclick` 不開 | `cargo test -p shell --test browser_view`（`links_open_tabs_where_the_click_says`）；`cargo test -p shell --lib tab_strip_model` |
| `MouseEvent` 的修飾鍵（建構子、WIT） | `cargo test -p dom_api --test events`（`event_constructors`）；`cargo test -p dom_js --test bindings`（`event_constructors`）；`python3 scripts/test.py reftest`（W6） |
| **renderer 當掉和 sad tab** | |
| renderer 結束後分頁留著，下一次導覽重開 renderer：同樣的歷史、更新的 surface、Viz 重新登記；`about:crash` 當下就記下當掉、舊的 renderer 不再轉送訊息；`shutdown` 之後訊息的 channel 關閉 | `cargo test -p browser --test navigation`（`the_next_navigation_after_a_crash_launches_a_renderer`、`the_renderer_going_is_the_last_message`） |
| sad tab：畫在網頁 view 的位置、按鈕和重新整理重開 renderer、重開時先是空白（不是舊的 frame）、新的 renderer 知道大小、最後一個分頁也不關視窗、背景的分頁切過去才看到 | `cargo test -p shell --test browser_view`（`a_tab_whose_renderer_has_gone_shows_its_sad_tab`）；`cargo test -p shell --lib sad_tab` |
| **在真的視窗裡** | |
| 快捷鍵操作分頁、`click-at` 點分頁列、點 `target="_blank"` 的連結、`about:crash` 再重新整理；分頁列和 sad tab 的截圖 | `python3 scripts/test.py scenarios tabs tab-strip link-tabs sad-tab`；`python3 scripts/test.py automation`（`tab-strip`、`sad-tab` 的截圖） |

## 做過的檢查

2026-10-06，Windows 11。四段（1、2、3a、3b）各自跑過 mutation test、完整的檢查和 `test.py automation`。

- **mutation test**：共 109 個（第 1 段 34、第 2 段 28、3a 29、3b 18），全部抓到；每次跑的期間請使用者不要 stage 或 commit，跑完用 checksum 確認檔案都還原了。第一輪沒抓到、補了測試之後抓到的：
  - 切換時按鍵留給舊的分頁：測試在切換前先在第一個分頁按了滑鼠，按鍵本來就該是它的。
  - 一個分頁的載入完成回答了別的分頁的等待：另一個分頁的載入都在等待之前。
  - renderer 把中鍵 dispatch 成 `click`：測試用的 `onauxclick` 屬性 kun 沒有，`preventDefault` 根本沒執行，「被取消的 `auxclick` 不開」只是剛好通過。改用 `addEventListener`。
  - 重開 renderer 之後沒送大小：Viz 畫出存著的舊 frame（見下面）。
  - 等價的和只靠審查的，記在 `docs/testing.md`。
- **途中找到、修掉的問題**：
  - 分頁的 id 重用（第 1 段，3a 修掉）：`next_tab_id` 取目前分頁的 id 最大值加一，關掉最後面的分頁之後新分頁重用它的 id 和 `FrameSinkId`，舊 renderer 晚到的 `RendererGone` 會關掉新分頁。改成只增不減。
  - `BrowserView::navigate` 沒讓網址列放掉 focus（第 2 段，真視窗的截圖看到網址列還是 `about:blank`），也沒有重畫（3b，`about:crash` 之後 sad tab 沒畫出來）。
  - `WebContents` 為了重開 renderer 握著訊息的 callback，`shutdown` 之後 channel 不關（3b）。
  - 重開 renderer 時只推 `LocalSurfaceId`，Viz 照樣畫出存著的舊 frame（3b）：改成先丟掉 frame sink 和它的 surface（`README.md` 第 25 條）。
  - 隱藏的分頁的 `DidFinishLoad` 跟著 frame 送，要等再顯示才送（第 1 段）：記在「和 Chromium 不同的地方」和 `docs/limitations.md`。
- **測試卡住兩次**，都要請使用者停掉程序（自動模式擋下了結束程序的指令），被中斷的 mutation 用 checksum 確認、手動還原：
  - 第 1 段：等待的 helper 每收一則訊息只等 30 秒，隱藏的分頁一直送 frame 時永遠等不到逾時。`until_viz`、`until_from`、`until_frame_from` 改成整體 30 秒的上限。
  - 3b：`the_renderer_going_is_the_last_message` 用 `host_rx.iter()` 等 channel 關閉，而 3b 讓它關不了（上面的 callback）；那時 3b 改完還沒跑過完整的測試。改成最多等 30 秒，channel 不關就失敗。教訓：跑 mutation test 之前先跑完整的測試；等待一律要有整體的上限。
- **跟著改的**：網頁往下移了 34 CSS px（分頁列）：`browser_view` 測試的位置、情境 `navigation` 和 `demo-busy` 的座標。`kun:dom` 升到 0.4.0，所有 guest 用 `ci.py guests` 重編，Rust 的 reftest guest W6 補上新欄位；`spec-missing.txt`、`machopo-dom.wit`、WPT 的 `Event-subclasses-constructors` 的 baseline 重新產生。mutation 的腳本改成照位元組讀寫、保留檔案原本的換行字元。
- **真的視窗**：`test.py automation` 的 13 個情境都通過，包括 `tabs`、`tab-strip`、`link-tabs`、`sad-tab`；分頁列和 sad tab 的截圖給使用者看過。
- **最後一次完整的檢查**（3b 之後）：`cargo fmt --check`、clippy（沒有警告）、`cargo test --workspace` 911 個全部通過（情境 26 次）、`sanity.py deps`、`boundary`、`unsafe`、`docs`。3a 動到 `dom` 的 `MouseEventData`（GC 物件上的資料，多了四個欄位），沒有改 GC 的邏輯，`matrix` 和 `fuzz` 沒跑。
