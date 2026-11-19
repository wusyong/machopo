# RFC 0104 的進度：frame 樹和每個 frame 的導覽由 Browser 擁有

<!-- 怎麼更新見 docs/README.md 的「完成一步時」和「一份 RFC 做完時」。設計理由寫在 README.md，目前仍然成立的限制寫在 docs/limitations.md。 -->

開始日期：2026-11-18。完成日期：2026-11-19（macOS，Apple M4）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 1a：`FrameToken`、engine 抽 id、renderer 送建立和 detach 的訊息、DevTools 和 WebDriver 的 frame 的 id | 完成 | 2026-11-18 |
| 1b：Browser 的 `FrameTree`、檢查、壞掉的訊息 | 完成 | 2026-11-18 |
| 1c：第 1 階段的檢查和文件 | 完成 | 2026-11-18 |
| 2a：renderer 的訊息帶著 renderer 的身分 | 完成 | 2026-11-18 |
| 2b：`BeginNavigation`、網址的檢查（主 frame 的路） | 完成 | 2026-11-18 |
| 2c：子 frame 一般的導覽經過 Browser、`NavigationDropped`、`file:` 照發起的文件檢查 | 完成 | 2026-11-18 |
| 2d：`about:blank`、`about:srcdoc` 和 `DidChangeSrcDoc` | 完成 | 2026-11-18 |
| 2e：子 frame 同一份文件內的導覽的回報 | 完成 | 2026-11-19 |
| 2f：第 2 階段的檢查和文件 | 完成 | 2026-11-19 |
| 3a：iframe 的名稱和 unique name | 完成 | 2026-11-19 |
| 3b：entry 的樹、子 frame 跨文件的導覽進歷史 | 完成 | 2026-11-19 |
| 3c：子 frame 同一份文件內的導覽進歷史 | 完成 | 2026-11-19 |
| 3d：上一頁、下一頁跨 frame 走 | 完成 | 2026-11-19 |
| 3e：第 3 階段的檢查和文件 | 完成 | 2026-11-19 |
| 4a：commit 的回報的檢查 | 完成 | 2026-11-19 |
| 4b：WPT 的目錄 | 完成 | 2026-11-19 |
| 4c：文件、RFC 和第 4 階段的檢查 | 完成 | 2026-11-19 |

## 驗收

| 領域 | 項目 | 怎麼驗證 |
| --- | --- | --- |
| frame 樹 | renderer 建立、拿掉子 frame 時告訴 Browser（父 frame、名稱、是不是 script 建的），Browser 的樹和 renderer 的一致；主 frame 換文件時舊的子 frame 都拿掉；renderer 結束時清回只剩根 | `cargo test -p browser`（`frame_tree::tests`、`links::the_browser_hears_of_child_frames_coming_and_going`、`a_renderer_gone_takes_its_frames`）、`dom_js` 的 `frames::a_frame_goes_with_its_token` |
| 壞掉的訊息 | 樹接不上的、導覽和 commit 給不在的 frame、Browser 沒送過的 commit 或別的網址、renderer 自己的 commit 不是子 frame 的 `javascript:`、`srcdoc` 和捲動模式給不在的 frame、同一份文件內到它不能有的網址、IO thread 收到文件的請求：結束 renderer、分頁 crashed、下一次導覽重開；結束了的 renderer 晚到的訊息不算 | `links::a_bad_message_ends_the_renderer`（15 種）、`frame_tree::tests::a_commit_is_one_the_browser_sent`、`an_ended_renderers_late_messages_count_for_nothing`、`shell` 的 `browser_view::an_ended_renderers_late_message_counts_for_nothing` |
| 導覽 | 任何 frame 換文件都是 `BeginNavigation`（誰發起的）、Browser 檢查網址（scheme、`data:`、`about:`、`file:` 照發起的文件）、載入、redirect、commit；不收的、被取代的、停掉的送 `NavigationDropped`，父文件的 `load` 不再等；同一個 frame 只 commit 最新的 | `links::a_child_frames_document_is_the_browsers_to_load`、`a_child_frames_navigation_the_browser_refuses_goes_nowhere`、`a_replaced_child_navigation_dropped_leaves_the_later`、`a_child_frame_commits_its_latest_navigation_alone`、`a_page_may_navigate_to_about_blank_alone`、`a_navigation_tells_the_browser_who_asked`、`dom_js` 的 `links::a_navigation_says_who_asked` |
| 錯誤頁和 origin | 子 frame 載入失敗是錯誤頁（`chrome-error://chromewebdata/`、opaque 的 origin，Browser 記失敗的網址）；`about:blank` 是發起者的 origin、`srcdoc` 是父 frame 的、不放行的 `about:srcdoc` 是錯誤頁；本機檔案的權限照送出的文件 | `a_child_frames_failed_navigation_commits_an_error_page_of_no_origin`、`a_child_frames_srcdoc_is_the_browsers_to_commit`、`a_frame_of_another_origin_may_not_navigate_to_about_srcdoc`、`a_pages_about_blank_has_its_origin`、`a_local_files_child_frame_opens_local_files`、`shell` 的 `documents::about_blank_has_its_initiators_origin`、`dom_js` 的 `frames::a_document_has_its_origin_from_the_one_it_inherits` |
| 同一份文件內 | 每個 frame 的 fragment、`pushState`、`replaceState` 告訴 Browser，網址照「can have its URL rewritten」檢查；子 frame 的重新載入照它最後的網址；`javascript:` 的 `src` 告訴 Browser | `frame_tree::tests::a_document_navigates_within_itself_as_html_lets_it`、`links::a_child_frames_reload_loads_the_url_it_has_within_its_document`、`a_child_frames_javascript_src_is_told` |
| unique name | Blink 的 `UniqueNameHelper` 的格式：名稱、路徑、位置和編號、長名稱的雜湊、script 的隨機名稱；之後改 `name` 不變 | `unique_name::tests`、`links::a_child_frame_is_told_with_its_name` |
| joint session history | entry 是位置的樹、項目共用、沒人用的清掉；子 frame 的第一份文件、取代、重新載入在目前的 entry 裡，之後的是新的 entry；同一份文件內的是新的 entry，initial about:blank 上的取代；`history.length` 每個 frame 一樣，commit 的位置在送出時算；子 frame 自己的捲動模式和 state | `navigation_controller::tests`（17 個）、`links::a_child_frames_documents_are_in_the_history`、`a_child_frames_push_state_is_in_the_history`、`a_child_frames_new_entry_counts_the_entries_made_meanwhile`、`scroll_restoration_is_the_entry_s`、`dom_js` 的 `history::a_child_frames_entries_are_the_pages` |
| 上一頁、下一頁 | 照 Chromium 的 `FindFramesToNavigate` 只動 item 不同的 frame（同一份文件內的 `popstate`、別的文件重新載入、`back_forward`）；新的子 frame 照 entry 還原、`load` 之後的照 `src`；重新載入的文件沿用項目的文件序號；回到目前的 entry 停掉載入；要動的 frame 都不在時只換 entry | `links::back_and_forward_move_a_child_frame`、`back_restores_the_child_frames_of_the_entry`、`back_and_forward_keep_a_documents_items_and_stop_at_the_current_entry`、`back_over_a_frame_gone_moves_the_entry_alone` |
| DevTools 和 WebDriver | 子 frame 的 id 是 token；子 frame 的 `frameStartedNavigating`、文件的 `Network` 事件、`navigatedWithinDocument` 由 Browser 發，`loaderId` 是 Browser 的導覽 id | `shell` 的 `devtools::frames_coming_and_going_are_told`、`a_child_frames_navigation_within_its_document_is_told`、`devtools_lifecycle::the_child_frames_are_told`、`devtools_initiators::`、frame 的 WebDriver 測試 |
| WPT | `html/browsers/` 的 history、跨文件的導覽、重疊的導覽和 traversal、`nested-browsing-contexts` 整個；原本因為子 frame 不能換文件而停掉的 35 個重新跑 | `python3 scripts/test.py wpt html/browsers/`（0 個不預期） |
| 視窗 | 69 份情境在真的視窗裡（`frames` 的子 frame 和孫 frame、`iframes`、`iframe-loads`、`navigation`、`sad-tab`） | `python3 scripts/test.py scenarios --window` |

## 做過的檢查

macOS、Apple M4，2026-11-18 到 2026-11-19。

### 測試和 mutation test

每一步都照 `docs/testing.md` 跑了 mutation test（`python3 scripts/test.py mutate`），共 189 個：抓到 177 個，9 個等價、3 個只靠審查，記在 `docs/test-gaps.md`（子 frame 的請求用 `Document`、取代時告訴 renderer、子 frame 的本機文件給權限、crashed 時照樣處理舊 renderer 的建立和 detach、headless 不過濾舊 renderer、token 到時 engine 已經在、`srcdoc` 的 frame 被同源的重新載入、renderer 在任何載入的文件都標記照歷史還原、DevTools 只回答 main frame 的導覽；IO thread 擋文件請求、renderer 結束時留著 main frame 的導覽、換 engine 時丟掉舊的文件）。第一次沒抓到、補了測試再抓到的：測試頁裡還有別的 task 讓 `load` 照樣發出（2c）、失敗的網址本來就跨源（2c）、origin 檢查得太晚（2d）、V8 的 token 只有父 frame 發起的情況（2d）、重新載入的測試在實作壞掉時一直重新載入（2e，改成重新載入一次就回報）、子 frame 的重新載入和 main frame 同一份文件內的新 entry 的位置、`SetHistoryOffsetAndLength` 只在導覽開始之後 entry 又變了才看得出來（3b）、initial about:blank 的規則被「還沒 completely loaded」蓋住（3c）、序號 0 的合併、沒有 frame 要動時的 entry 和 renderer 的位置（4b）。每一步的 mutant 寫在各步的 commit message 和當時的 `progress.md`（`git log`）。

### 找到的 bug

- 舊 renderer 結束前已經排進佇列的訊息被當成新 renderer 的（1b 找到，2a 讓訊息帶著 renderer 的身分）；`BrowserView` 在分頁已經關掉時也丟掉訊息，`RendererGone` 到不了 DevTools（2a）。
- 子 frame 的 `javascript:` 的 `src` 送到 Browser、沒有 `load`（WPT 的 `history_pushstate_url_rewriting.html` 逾時，2c，改回 renderer 自己 commit 並告訴 Browser）；讀本機檔案的權限在 `DidCommitNavigation` 時重算有一段空窗（2c，改成送出 commit 時決定）。
- commit 的 index 和 length 在導覽開始時算，子 frame 的 commit 把整頁的 `history.length` 設回舊的（WPT 的 `history-traversal-navigate-parent-while-child-loading.html`，3b）；同一份文件內的新 entry 之後沒清項目（3c）。
- 同一份文件內 traverse 的 frame 不往下看（WPT 的 `history-traversal-navigates-multiple-frames.html`，3d）。
- 上一頁、下一頁沒有 frame 要動時重新載入 main frame，回到目前的 entry 時也是（WPT 的 `overlapping-navigations-and-traversals` 的測試頁自己重新載入，4b）；歷史載入的文件換了序號，同一份文件的其他 entry 又重新載入（`same-document-traversal-cross-document-traversal.html`，4b）。

### WPT

- `html/browsers/history/joint-session-history/`、`the-session-history-of-browsing-contexts/`、`html/browsers/browsing-the-web/overlapping-navigations-and-traversals/`、`navigating-across-documents/`、`html/browsers/windows/nested-browsing-contexts/` 整個加進清單（第 34 條），逾時的照原因標 `disabled`（`pageshow`、表單送出、`window.open`、`javascript:` 網址的結果、`Referer`、`ImageData` 的 clone、`pageswap`、`window.name`、沒連上文件的 `<a>`、跨 origin 的 fragment 導覽的 `load`、上一頁接著下一頁）。
- 原本因為「子 frame 不能換文件、沒有自己的歷史、連結不導覽子 frame」停掉的 35 個重新跑：24 個跑完（記下結果），其他照真正的原因重新標。`history-traversal-navigates-multiple-frames.html`、`same-document-traversal-cross-document-traversal.html` 整個通過。
- 對照 wpt.fyi（2026-11-19）：Edge（Chromium）在 `overlapping-navigations-and-traversals/` 的結果和 machopo 一致（跨文件的那組通過、同一份文件內的那組逾時）。
- `html/rendering/widgets/` 的兩份不預期的通過（按鈕的排版，2c 起就有）和這份無關，沒有更新預期結果。

### 不穩定的測試

沒有。`links::a_child_frames_new_entry_counts_the_entries_made_meanwhile` 靠「renderer 忙 1 秒、回應延遲 100 ms」的餘裕，連跑 10 次都通過。

### 跑過的檢查

- 每個階段做完時：`cargo test --workspace`、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`python3 scripts/test.py scenarios --window`（69 份）。clippy 找到的都改了寫法、沒有加 `#[expect]`：`FrameToken::parse` 的 `as_chunks`、`dom_js` 的 `Frame::new` 的參數（1c）；`commit_timing` 的參數、`set_navigation` 的錯誤型別、IO thread 的 callback 的型別（2f）；`unique_name` 的長度運算（3e）；`drain` 的 `next_back`（4c）。
- 最後（4c）：真視窗的情境（`scenarios --window`）第一次跑時螢幕鎖住，視窗畫不出 frame，大多是「no frame was drawn」而失敗（環境的問題），停掉了；解鎖之後（merge 回 `main` 之後、`cargo clean` 重編）重跑，69 份全部通過，截圖看過 `frames`。`cargo test --workspace` 4136 個通過（57 個測試 binary；`bindings` 的 `gc_platform` 印出的兩個 panic 是測試刻意的），fmt、clippy 通過；`test.py wpt html/browsers/` 沒有不預期的結果；`sanity.py` 的 `deps`、`spans`、`boundary`、`unsafe`、`tests`、`numerics`、`docs` 通過。
- 沒有動到 GC：`matrix`、`fuzz` 照 CLAUDE.md 由每週的 CI 跑。
