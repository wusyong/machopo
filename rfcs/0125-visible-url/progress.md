# RFC 0125 的進度：網址列只顯示已經 commit 的網址（頁面發起的導覽）

開始日期：2026-11-27。完成日期：2026-11-27（Linux：Fedora 45，x86-64，AMD Radeon 890M）。

## 步驟

| 步驟 | 狀態 | 完成日期 |
| --- | --- | --- |
| 0. 草稿、盤點、用臨時的測試確認偽造（`details.md` 的「確認」）、對照 Chromium 和 chromedriver、已決定的事項第 1 到 3 條 | 做完 | 2026-11-27 |
| 1. 最後 commit 的和顯示的網址；用到的地方照第 2 條；測試、mutation test、文件 | 做完 | 2026-11-27 |

## 驗收

| 項目 | 怎麼驗證 |
| --- | --- |
| 頁面自己發起的導覽在 commit 之前（包括送出 commit、renderer 還沒回報的那一段）顯示的網址不變，commit 之後換 | `cargo test -p browser --test it visible_url::a_pages_navigation_shows_once_it_commits` |
| 一直發起、互相取代的導覽從來不顯示要去的網址（偽造） | `visible_url::a_page_that_keeps_navigating_never_shows_where_to` |
| Browser 發起的新導覽載入時、送出 commit 時都顯示要去的網址；停下之後回到目前的頁面的 | `visible_url::the_browsers_navigation_shows_where_it_goes_till_it_stops` |
| 頁面的導覽取代 Browser 的：顯示目前的頁面的 | `visible_url::a_pages_navigation_in_place_of_the_browsers_shows_the_page` |
| 上一頁、下一頁、重新整理在 commit 之前顯示目前的頁面的 | `visible_url::history_navigations_show_once_they_commit` |
| 什麼都還沒顯示、或只顯示了第一份文件的分頁：頁面的導覽也顯示要去的網址 | `visible_url::a_tabs_first_navigation_shows_where_it_goes` |
| 視窗：網址列、沒有標題的分頁上的文字在點連結之後、commit 之前是目前的頁面的；網址列輸入的載入時就顯示；什麼都還沒顯示的分頁的文字是要去的網址 | `cargo test -p shell --test it browser_view::a_pages_navigation_shows_in_the_address_field_once_it_commits` |
| DevTools 的 target 的 `url` 是網址列的；`Page.frameStartedNavigating` 是那一次導覽的網址 | `automation_pipe::the_targets_url_is_the_address_bars` |
| WebDriver 的 Get Current URL 是 commit 了的文件的，Browser 發起的導覽還在路上時也是 | `webdriver::the_current_url_is_the_documents_shown` |

## 做過的檢查

### 第 0 步（2026-11-27，Linux：Fedora 45，x86-64）

- 臨時的測試（加在 `crates/browser/tests/it/navigation.rs`，試完還原）：頁面每 50 ms 執行 `location = '/victim'`、`/victim` 延遲 5 秒回應，3 秒內 `WebContents::url()` 一直是 `/victim`，frame 樹的 root 一直是頁面 `/`、標題一直是它的，沒有 commit。
- 沒有改程式。`python3 scripts/sanity.py docs` 通過。

### 第 1 步（2026-11-27，Linux：Fedora 45，x86-64，AMD Radeon 890M）

- **新的測試**：`browser` 的 `tests/it/visible_url.rs`（6 個：renderer 另一端沒有東西，commit 由測試回報，送出 commit 到回報之間的那一段也看得到）；`shell` 的 `browser_view::a_pages_navigation_shows_in_the_address_field_once_it_commits`、`automation_pipe::the_targets_url_is_the_address_bars`、`webdriver::the_current_url_is_the_documents_shown`。
- **mutation test**（`mutants.toml`，18 個）：16 個抓到。沒抓到的兩個：`frame_tree.rs` 的 `uncommitted_url` 拿掉導覽編號的比對（等價：main frame 的每一次導覽都重設 `shown_navigation`）、reftest 的結果的網址改用顯示的（等價：headless 讀的時候沒有導覽在路上），記進 `docs/test-gaps.md` 的「等價、不用處理的 mutation」。另外 `Network.getCookies` 的最上層網址改用顯示的，沒有測試失敗（只在 Browser 發起、往別的網站的導覽還在路上、而且有 partitioned cookie 時不同），記進 `docs/test-gaps.md` 的「能補、還沒補的測試空缺」。格式化之後重跑了文字變了的 5 個，都抓到。
- **改了 machopodriver**：`python3 scripts/test.py wpt html/browsers/history/the-location-interface/ html/browsers/browsing-the-web/overlapping-navigations-and-traversals/ html/browsers/browsing-the-web/history-traversal/`：153 個測試，一個不符預期的結果：`overlapping-navigations-and-traversals/cross-document-traversal-cross-document-nav.html` 的「slow cross-document traversal and then fast cross-document navigation」預期 FAIL、這裡通過，連跑 6 次都通過；把這一步的改動收起來、用 `main`（`0a0d7c9e`）的程式跑 3 次也都通過，所以不是這一步造成的。每天的 CI（`ci` profile）照樣失敗、和預期一致，推測是 debug build 的時間讓兩次導覽的先後不同；不追（2026-11-27 使用者決定：不影響這一步，CI 上的結果和預期一致）。
- 改到的 crate 的測試：`cargo test -p browser`、`-p driver`、`-p shell` 共 1,143 個通過（加新的測試之前）。
- 階段結束的檢查：`cargo test --workspace` 5,037 個通過（1 個 ignored）；`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`sanity.py` 的每一項都通過。
- 改了自動化的 pipe 的 CDP 方法（target 的 `url`、`Page.frameStartedNavigating`），跑了量測工具的測試：`blink_perf_test.py`、`raster_test.py`、`census_test.py` 通過；`speedometer_test.py` 10 次裡 1 次失敗，是 `test_the_windows_are_the_runners_times`（trace 裡 runner 的標記之間的時間比 runner 回報的多 0.195 ms，容許 0.15 ms）。把這一步的改動收起來、用 `main`（`0a0d7c9e`）的程式只跑這一個測試 12 次，失敗 3 次、每次都差 0.19 到 0.20 ms：`main` 上本來就有，不是這一步造成的。工具沒有讀 target 的 `url` 或 `Page.frameStartedNavigating`。原因沒有查；不追（2026-11-27 使用者決定：這個工具的測試不在 CI 裡，誤差只在容許值上下）。

